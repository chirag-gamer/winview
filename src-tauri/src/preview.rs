//! File-type detection and Phase 1 preview building (FR-8/FR-16).
//!
//! Phase 1 dispatches: cloud-only cards, plain text (≤1 MB whole, head
//! truncation above), binary info cards. The full type table lands in
//! Phase 2+; detection helpers here are the foundation it builds on.

use serde::Serialize;
use std::io::Read;
use std::os::windows::fs::MetadataExt;
use std::path::Path;

/// 1 MB whole-file limit for text (FR-16).
pub const TEXT_LIMIT: u64 = 1024 * 1024;
/// Sniff window for text/binary discrimination.
pub const SNIFF_LEN: usize = 8 * 1024;

pub const FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS: u32 = 0x0040_0000; // D-013

#[derive(Debug, Serialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum Preview {
    Text { content: String, truncated: bool },
    Code { html: String, lang: String, truncated: bool },
    Markdown { html: String, mermaid: Vec<String>, raw: String, truncated: bool },
    Diff { html: String, truncated: bool },
    PackageJson { summary: serde_json::Value, code_html: String, truncated: bool },
    JsonTree { html: String, raw_html: String, truncated: bool },
    JsonLines { lines: Vec<String>, count: usize },
    Table { columns: Vec<String>, numeric: Vec<bool>, total_rows: usize, sample_rows: Vec<Vec<String>>, decimal_comma: bool, truncated: bool },
    Sqlite { tables: Vec<serde_json::Value>, wal_warning: bool, error: Option<String> },
    Log { html: String, first_line_no: usize, total_lines: usize, truncated: bool, tail_only: bool },
    Env { entries: Vec<serde_json::Value> },
    Sql { formatted_html: String, raw_html: String, fallback_reason: Option<String> },
    Notebook { cells: Vec<serde_json::Value>, script: String, error: Option<String> },
    Archive { kind: String, entries: Vec<serde_json::Value>, total_files: usize, total_bytes: u64, truncated: bool, error: Option<String> },
    Xml { tree_html: String, raw_html: String, error: Option<String> },
    Plist { entries: Vec<serde_json::Value>, error: Option<String> },
    MermaidFile { source: String },
    Media { media_kind: String, url: String },
    Binary { size: u64, likely_type: String },
    Cloud,
    Error { message: String },
}

#[derive(Serialize)]
pub struct Envelope {
    pub req_id: u64,
    pub path: String,
    pub name: String,
    pub size: u64,
    pub preview: Preview,
}

/// Build the preview payload for a path. Runs on the Tauri blocking pool.
/// `settings` gates file categories (FR-32).
pub fn build(path: &Path, req_id: u64, settings: &crate::settings::Settings) -> Envelope {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let (size, attrs) = match std::fs::metadata(path) {
        Ok(m) => (m.len(), m.file_attributes()),
        Err(e) => {
            return envelope(req_id, path, name, 0, Preview::Error { message: e.to_string() });
        }
    };
    let envelope = |p: Preview| envelope(req_id, path, name.clone(), size, p);
    let kind = crate::detection::detect(path);

    // OneDrive cloud-only: never open for read (D-013 / FR-8).
    if attrs & FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS != 0 {
        return envelope(Preview::Cloud);
    }
    if size == 0 {
        return envelope(Preview::Text { content: String::new(), truncated: false });
    }

    match std::fs::File::open(path) {
        Ok(mut f) => {
            let mut head = vec![0u8; SNIFF_LEN.min(size as usize)];
            if let Err(e) = f.read_exact(&mut head) {
                return envelope(Preview::Error { message: e.to_string() });
            }
            if is_binary(&head) && !matches!(kind, Some(crate::detection::Kind::PackageJson)) {
                return envelope(Preview::Binary { size, likely_type: likely_binary_type(path) });
            }
            // Read whole up to the limit (head bytes already read).
            let mut text_bytes = head;
            if size > SNIFF_LEN as u64 {
                let remaining = (size as usize).min(TEXT_LIMIT as usize) - text_bytes.len();
                let mut rest = vec![0u8; remaining];
                if f.read_exact(&mut rest).is_ok() {
                    text_bytes.extend_from_slice(&rest);
                }
            }
            let truncated = size > TEXT_LIMIT;
            let content = decode_text(&text_bytes);
            dispatch(envelope, path, kind, &content, size, truncated, settings, req_id)
        }
        Err(e) => envelope(Preview::Error { message: e.to_string() }),
    }
}

/// Route by detected kind. Phase 4+ kinds fall back to the text preview
/// until their renderers land.
#[allow(clippy::too_many_arguments)]
fn dispatch(
    envelope: impl Fn(Preview) -> Envelope,
    path: &Path,
    kind: Option<crate::detection::Kind>,
    content: &str,
    size: u64,
    truncated: bool,
    settings: &crate::settings::Settings,
    req_id: u64,
) -> Envelope {
    let _ = size;
    use crate::detection::Kind;
    match kind {
        Some(Kind::Markdown) => {
            if !settings.category_enabled("markdown") {
                return envelope(Preview::Error { message: "markdown previews disabled".into() });
            }
            crate::markdown::set_image_token(req_id);
            if let Some(dir) = path.parent() {
                crate::scheme::set_image_base(req_id, dir.to_path_buf());
            }
            let payload = crate::markdown::render(path, content);
            envelope(Preview::Markdown {
                html: payload.html,
                mermaid: payload.mermaid,
                raw: content.to_string(),
                truncated,
            })
        }
        Some(Kind::Diff) => envelope(Preview::Diff {
            html: render_diff(content),
            truncated,
        }),
        Some(Kind::PackageJson) => {
            let summary = match serde_json::from_str::<serde_json::Value>(content) {
                Ok(v) => package_json_summary(&v),
                Err(e) => serde_json::json!({ "error": e.to_string() }),
            };
            let code = crate::code::highlight(path, content);
            envelope(Preview::PackageJson { summary, code_html: code.html, truncated })
        }
        Some(Kind::Code) => {
            if !settings.category_enabled("code") {
                return envelope(Preview::Error { message: "code previews disabled".into() });
            }
            let code = crate::code::highlight(path, content);
            envelope(Preview::Code { html: code.html, lang: code.lang, truncated })
        }
        Some(Kind::Json) => {
            if !settings.category_enabled("json") {
                return envelope(Preview::Error { message: "json previews disabled".into() });
            }
            let raw = crate::code::highlight(path, content);
            match crate::json::jsonc_parse(content) {
                Ok(v) => {
                    let t = crate::json::tree(&v);
                    envelope(Preview::JsonTree { html: t.html, raw_html: raw.html, truncated })
                }
                Err(e) => envelope(Preview::Error { message: format!("invalid JSON: {e}") }),
            }
        }
        Some(Kind::JsonLines) => {
            if !settings.category_enabled("json") {
                return envelope(Preview::Error { message: "json previews disabled".into() });
            }
            let (count, lines) = crate::json::jsonl_summary(content);
            envelope(Preview::JsonLines { lines, count })
        }
        Some(Kind::Table) => {
            if !settings.category_enabled("table") {
                return envelope(Preview::Error { message: "table previews disabled".into() });
            }
            let meta = crate::table::analyze(content);
            envelope(Preview::Table {
                columns: meta.columns,
                numeric: meta.numeric,
                total_rows: meta.total_rows,
                sample_rows: meta.sample_rows,
                decimal_comma: meta.decimal_comma,
                truncated: meta.truncated,
            })
        }
        Some(Kind::Sqlite) => {
            if !settings.category_enabled("sqlite") {
                return envelope(Preview::Error { message: "sqlite previews disabled".into() });
            }
            let s = crate::sqlite::schema(path.to_string_lossy().as_ref());
            let tables: Vec<serde_json::Value> = s
                .tables
                .iter()
                .map(|t| serde_json::to_value(t).unwrap_or(serde_json::Value::Null))
                .collect();
            envelope(Preview::Sqlite { tables, wal_warning: s.wal_warning, error: s.error })
        }
        Some(Kind::Log) => {
            if !settings.category_enabled("log") {
                return envelope(Preview::Error { message: "log previews disabled".into() });
            }
            let lp = crate::logview::build(path);
            envelope(Preview::Log {
                html: lp.html,
                first_line_no: lp.first_line_no,
                total_lines: lp.total_lines,
                truncated: lp.truncated,
                tail_only: lp.tail_only,
            })
        }
        Some(Kind::Env) => {
            if !settings.category_enabled("env") {
                return envelope(Preview::Error { message: "env previews disabled".into() });
            }
            let e = crate::envfile::parse(content);
            let entries: Vec<serde_json::Value> = e
                .entries
                .iter()
                .map(|x| serde_json::to_value(x).unwrap_or(serde_json::Value::Null))
                .collect();
            envelope(Preview::Env { entries })
        }
        Some(Kind::Sql) => {
            if !settings.category_enabled("sql") {
                return envelope(Preview::Error { message: "sql previews disabled".into() });
            }
            let sp = crate::sqlfmt::build(content);
            let raw = crate::code::highlight(path, &sp.raw_source);
            envelope(Preview::Sql {
                formatted_html: sp.formatted_html,
                raw_html: raw.html,
                fallback_reason: sp.fallback_reason,
            })
        }
        Some(Kind::Notebook) => {
            if !settings.category_enabled("notebook") {
                return envelope(Preview::Error { message: "notebook previews disabled".into() });
            }
            let nb = crate::notebook::build(content);
            let cells: Vec<serde_json::Value> = nb
                .cells
                .iter()
                .map(|c| serde_json::to_value(c).unwrap_or(serde_json::Value::Null))
                .collect();
            envelope(Preview::Notebook { cells, script: nb.script, error: nb.error })
        }
        Some(Kind::Archive) => {
            if !settings.category_enabled("archive") {
                return envelope(Preview::Error { message: "archive previews disabled".into() });
            }
            if path.extension().map(|e| e.eq_ignore_ascii_case("gz")).unwrap_or(false)
                && !path.to_string_lossy().to_lowercase().ends_with(".tar.gz")
                && !path.to_string_lossy().to_lowercase().ends_with(".tgz")
            {
                // .gz: first 1 MB decompressed as text (FR-28)
                match crate::archives::gz_text(path, 1024 * 1024) {
                    Ok(text) => {
                        return envelope(Preview::Text {
                            content: text,
                            truncated: true,
                        });
                    }
                    Err(e) => {
                        return envelope(Preview::Error { message: e });
                    }
                }
            }
            let ap = crate::archives::build(path, None);
            let entries: Vec<serde_json::Value> = ap
                .entries
                .iter()
                .map(|e| serde_json::to_value(e).unwrap_or(serde_json::Value::Null))
                .collect();
            envelope(Preview::Archive {
                kind: ap.kind,
                entries,
                total_files: ap.total_files,
                total_bytes: ap.total_bytes,
                truncated: ap.truncated,
                error: ap.error,
            })
        }
        Some(Kind::Xml) => {
            let x = crate::xml_plist::xml_tree(content);
            let raw = crate::code::highlight(path, content);
            envelope(Preview::Xml { tree_html: x.tree_html, raw_html: raw.html, error: x.error })
        }
        Some(Kind::Plist) => {
            let bytes = std::fs::read(path).unwrap_or_default();
            let p = crate::xml_plist::plist_entries(&bytes);
            let entries: Vec<serde_json::Value> = p
                .entries
                .iter()
                .map(|e| serde_json::to_value(e).unwrap_or(serde_json::Value::Null))
                .collect();
            envelope(Preview::Plist { entries, error: p.error })
        }
        Some(Kind::Mermaid) => {
            if !settings.category_enabled("markdown") {
                return envelope(Preview::Error { message: "markdown previews disabled".into() });
            }
            envelope(Preview::MermaidFile { source: content.to_string() })
        }
        Some(Kind::Image | Kind::Audio | Kind::Video | Kind::Pdf) => {
            // Stream the file itself through the scheme with range support.
            crate::scheme::set_media_file(req_id, path.to_path_buf());
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let url = format!("http://winview.localhost/media/{req_id}/{name}");
            envelope(Preview::Media { media_kind: media_kind_of(kind.unwrap_or(crate::detection::Kind::Image)), url })
        }
        None => envelope(Preview::Text {
            content: content.to_string(),
            truncated,
        }),
    }
}

/// Recover the request id for image-token scoping (markdown images).
fn package_json_summary(v: &serde_json::Value) -> serde_json::Value {
    let scripts = v.get("scripts").and_then(|s| s.as_object()).map(|o| {
        o.iter()
            .map(|(k, val)| {
                serde_json::json!({ "name": k, "command": val.as_str().unwrap_or("") })
            })
            .collect::<Vec<_>>()
    });
    serde_json::json!({
        "name": v.get("name").and_then(|s| s.as_str()).unwrap_or(""),
        "version": v.get("version").and_then(|s| s.as_str()).unwrap_or(""),
        "description": v.get("description").and_then(|s| s.as_str()).unwrap_or(""),
        "dependencies": v.get("dependencies").and_then(|d| d.as_object()).map(|o| o.len()).unwrap_or(0),
        "devDependencies": v.get("devDependencies").and_then(|d| d.as_object()).map(|o| o.len()).unwrap_or(0),
        "scripts": scripts.unwrap_or_default(),
    })
}

fn media_kind_of(kind: crate::detection::Kind) -> String {
    use crate::detection::Kind;
    match kind {
        Kind::Image => "image".into(),
        Kind::Audio => "audio".into(),
        Kind::Video => "video".into(),
        Kind::Pdf => "pdf".into(),
        _ => "binary".into(),
    }
}

/// Diff rendering: escaped lines with add/remove/hunk classes (FR-17).
pub fn render_diff(content: &str) -> String {
    let mut out = String::with_capacity(content.len() + 256);
    for line in content.lines() {
        let class = if line.starts_with('+') && !line.starts_with("+++") {
            "diff-add"
        } else if line.starts_with('-') && !line.starts_with("---") {
            "diff-del"
        } else if line.starts_with("@@") {
            "diff-hunk"
        } else {
            "diff-ctx"
        };
        out.push_str(&format!(
            "<div class=\"{class}\">{}</div>",
            crate::code::escape_html(line)
        ));
    }
    out
}

fn envelope(req_id: u64, path: &Path, name: String, size: u64, preview: Preview) -> Envelope {
    Envelope {
        req_id,
        path: path.to_string_lossy().into_owned(),
        name,
        size,
        preview,
    }
}

/// Binary discrimination: NUL bytes (any, incl. UTF-16 without BOM and PE/PNG
/// headers) or a >30% share of ASCII control characters in the sniff window.
pub fn is_binary(head: &[u8]) -> bool {
    if head.contains(&0) {
        return true;
    }
    let suspicious = head
        .iter()
        .filter(|&&b| {
            !(0x20..=0x7E).contains(&b)
                && !matches!(b, b'\t' | b'\n' | b'\r' | 0x0C | 0x1B)
                && b < 0x80
        })
        .count();
    let ctrl_share = suspicious as f64 / head.len().max(1) as f64;
    ctrl_share > 0.30
}

/// Text decoding: UTF-8 BOM → UTF-8 (lossy), UTF-16 LE/BE BOM → UTF-16,
/// otherwise UTF-8 lossy (covers ASCII + most modern text; Windows-1252
/// mojibake risk accepted for Phase 1, refined with encoding_rs in Phase 2).
pub fn decode_text(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xFF, 0xFE]) {
        let utf16: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        return String::from_utf16_lossy(&utf16);
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        let utf16: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        return String::from_utf16_lossy(&utf16);
    }
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF][..]).unwrap_or(bytes);
    String::from_utf8_lossy(bytes).into_owned()
}

fn likely_binary_type(path: &Path) -> String {
    match path
        .extension()
        .map(|e| e.to_ascii_lowercase().to_string_lossy().into_owned())
        .as_deref()
    {
        Some("exe") | Some("dll") | Some("sys") => "Windows executable/library".into(),
        Some("zip") | Some("jar") | Some("apk") => "ZIP archive".into(),
        Some("png") => "PNG image".into(),
        Some("jpg") | Some("jpeg") => "JPEG image".into(),
        Some("pdf") => "PDF document".into(),
        Some("sqlite") | Some("sqlite3") | Some("db") => "SQLite database".into(),
        _ => "Binary file".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_detection() {
        assert!(is_binary(b"MZ\x90\x00\x03\x00\x00\x00")); // PE header
        assert!(is_binary(b"\x89PNG\r\n\x1a\n\x00\x00\x00\r"));
        assert!(!is_binary(b"# Sample\n\nplain text file\n"));
        assert!(!is_binary(b""));
        assert!(!is_binary(b"{\"json\": true}\n"));
    }

    #[test]
    fn text_decoding() {
        assert_eq!(decode_text(b"hello"), "hello");
        assert_eq!(decode_text(&[0xEF, 0xBB, 0xBF, b'a']), "a");
        assert_eq!(decode_text(&[0xFF, 0xFE, 0x68, 0x00]), "h"); // UTF-16 LE
        assert_eq!(decode_text(&[0xFE, 0xFF, 0x00, 0x68]), "h"); // UTF-16 BE
        assert_eq!(decode_text(&[0x68, 0xC3, 0xA9]), "hé"); // valid UTF-8
        assert_eq!(decode_text(&[0x68, 0xE9]), "h\u{FFFD}"); // invalid → replacement
    }

    #[test]
    fn empty_file_is_text() {
        // handled by the size==0 branch; decode_text of empty is empty
        assert_eq!(decode_text(b""), "");
    }
}
