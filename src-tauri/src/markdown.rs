//! GitHub-flavoured Markdown pipeline (FR-18..FR-20).
//!
//! Order matters:
//! 1. pre-scan fenced code blocks from the source (lang + content),
//! 2. comrak render (GFM: tables, tasklists, strikethrough, autolinks,
//!    footnotes, alerts, math, shortcodes, header ids),
//! 3. ammonia sanitize the comrak output (raw-HTML allowlisting),
//! 4. replace `<pre><code…>` blocks in order with trusted, highlighted HTML
//!    (mermaid blocks become placeholder divs the frontend renders),
//! 5. rewrite relative image URLs to the scoped `winview` scheme and strip
//!    remote ones.

use comrak::Options;

use crate::code;

pub struct MarkdownPayload {
    pub html: String,
    /// Fenced `mermaid` block sources, in placeholder order.
    pub mermaid: Vec<String>,
}

pub fn render(path: &std::path::Path, content: &str) -> MarkdownPayload {
    let fenced = extract_fenced_blocks(content);
    let raw_html = comrak_html(content);
    let sanitized = sanitize(&raw_html);
    let mut mermaid = Vec::new();
    let html = replace_code_blocks(&sanitized, &fenced, &mut mermaid);
    let html = rewrite_images(&html, path);

    MarkdownPayload { html, mermaid }
}

/// comrak GFM render (shared by render() and tests).
fn comrak_html(content: &str) -> String {
    let mut options = Options::default();
    options.extension.table = true;
    options.extension.strikethrough = true;
    options.extension.autolink = true;
    options.extension.tasklist = true;
    options.extension.footnotes = true;
    options.extension.alerts = true;
    options.extension.math_dollars = true;
    options.extension.math_code = true;
    options.extension.shortcodes = true;
    options.extension.header_id_prefix = Some("hv-".to_string());
    options.parse.smart = true;
    options.render.github_pre_lang = true;

    let arena = comrak::Arena::new();
    let root = comrak::parse_document(&arena, content, &options);
    let mut raw_html = String::new();
    let _ = comrak::html::format_document(root, &options, &mut raw_html);
    raw_html
}

/// Extract fenced blocks: (lang, content). Handles ``` and ~~~ fences.
fn extract_fenced_blocks(src: &str) -> Vec<(String, String)> {
    let mut blocks = Vec::new();
    let mut fence: Option<(char, usize)> = None;
    let mut current = String::new();
    let mut lang = String::new();
    for line in src.lines() {
        let trimmed = line.trim_start();
        match fence {
            None => {
                if let Some((marker, len)) = fence_of(trimmed) {
                    fence = Some((marker, len));
                    lang = trimmed[len..].trim().to_string();
                    current.clear();
                }
            }
            Some((marker, len)) => {
                if trimmed.chars().all(|c| c == marker) && trimmed.chars().count() >= len {
                    blocks.push((lang.to_ascii_lowercase(), current.clone()));
                    fence = None;
                } else {
                    current.push_str(line);
                    current.push('\n');
                }
            }
        }
    }
    blocks
}

fn fence_of(line: &str) -> Option<(char, usize)> {
    let mut chars = line.chars();
    let first = chars.next()?;
    if first != '`' && first != '~' {
        return None;
    }
    let run = line.chars().take_while(|&c| c == first).count();
    if run < 3 {
        return None;
    }
    Some((first, run))
}

/// Sanitizer config: comrak-generated classes + structural tags survive;
/// scripts, event handlers, and unknown classes do not.
fn sanitize(html: &str) -> String {
    use ammonia::Builder;
    use std::collections::{HashMap, HashSet};
    let alerts: HashSet<&str> = [
        "markdown-alert",
        "markdown-alert-note",
        "markdown-alert-tip",
        "markdown-alert-important",
        "markdown-alert-warning",
        "markdown-alert-caution",
        "markdown-alert-title",
        "octicon",
        "octicon-info",
        "octicon-light-bulb",
        "octicon-report",
        "octicon-alert",
        "octicon-x",
        "contains-task-list",
        "task-list-item",
        "footnotes",
        "footnote-backref",
        "math",
        "math-inline",
        "math-display",
        "winview-mermaid",
    ]
    .into_iter()
    .collect();
    let mut classes: HashMap<&str, HashSet<&str>> = HashMap::new();
    classes.insert("div", alerts.clone());
    classes.insert(
        "span",
        ["math", "math-inline", "math-display", "octicon"].into_iter().collect(),
    );
    Builder::default()
        .allowed_classes(classes)
        .generic_attribute_prefixes(HashSet::from(["data-"]))
        .url_relative(ammonia::UrlRelative::PassThrough)
        .clean(html)
        .to_string()
}

/// Replace `<pre lang="…"><code…>…</code></pre>` sequences with highlighted
/// HTML (or mermaid placeholders). The comrak `github_pre_lang` output carries
/// the language on the `<pre>`; fenced-block order is the fallback.
fn replace_code_blocks(
    html: &str,
    fenced: &[(String, String)],
    mermaid: &mut Vec<String>,
) -> String {
    let mut out = String::with_capacity(html.len() + 1024);
    let mut rest = html;
    let mut idx = 0usize;

    while let Some(pre_start) = rest.find("<pre") {
        // Only a real opening tag: "<pre>" or "<pre attr…>"
        let after_lt = &rest[pre_start + 4..];
        if !after_lt.starts_with('>') && !after_lt.starts_with(' ') {
            out.push_str(&rest[..pre_start + 4]);
            rest = &rest[pre_start + 4..];
            continue;
        }
        let Some(gt_rel) = after_lt.find('>') else { break };
        let pre_tag = &rest[pre_start..pre_start + 4 + gt_rel + 1];
        let after_pre = &rest[pre_start + 4 + gt_rel + 1..];
        let Some(code_start) = after_pre.find("<code") else { break };
        let code_tag_end = match after_pre[code_start..].find('>') {
            Some(i) => code_start + i + 1,
            None => break,
        };
        let Some(close) = after_pre[code_tag_end..].find("</code>") else { break };
        let body = &after_pre[code_tag_end..code_tag_end + close];
        let block_end_rel = code_tag_end + close + "</code>".len();
        let block_end_rel = if after_pre[block_end_rel..].starts_with("</pre>") {
            block_end_rel + "</pre>".len()
        } else {
            block_end_rel
        };
        let block_end = pre_start + 4 + gt_rel + 1 + block_end_rel;

        out.push_str(&rest[..pre_start]);

        let decoded = decode_entities(body);
        let lang = attr_value(pre_tag, "lang")
            .map(|s| s.to_string())
            .or_else(|| fenced.get(idx).map(|(l, _)| l.clone()))
            .unwrap_or_default();
        idx += 1;

        if lang == "mermaid" {
            let token = mermaid.len();
            mermaid.push(decoded.trim().to_string());
            out.push_str(&format!(
                "<div class=\"winview-mermaid\" data-token=\"{token}\"></div>"
            ));
        } else if lang.is_empty() {
            out.push_str(&rest[pre_start..block_end]);
        } else {
            // Resolve the syntax the same way a file with that language would.
            let tmp = std::path::PathBuf::from(format!("x.{}", lang));
            let highlighted = code::highlight(&tmp, &decoded);
            out.push_str(&format!(
                "<div class=\"code-block theme-auto\" data-lang=\"{}\"><div class=\"code-copy\" title=\"Copy code\">Copy</div>{}</div>",
                escape_attr(&lang),
                highlighted.html
            ));
        }
        rest = &rest[block_end..];
    }
    out.push_str(rest);
    out
}

/// Minimal HTML entity decoder for code bodies (comrak escapes these).
fn decode_entities(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

fn escape_attr(s: &str) -> String {
    s.replace('"', "&quot;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Rewrite relative image srcs to the scoped scheme; drop remote ones.
fn rewrite_images(html: &str, md_path: &std::path::Path) -> String {
    let base = md_path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(pos) = rest.find("<img") {
        let tag_start = pos;
        let Some(tag_end_rel) = rest[pos..].find('>') else {
            out.push_str(&rest[..tag_start]);
            rest = &rest[tag_start..];
            break;
        };
        let tag_end = pos + tag_end_rel + 1;
        let tag = &rest[tag_start..tag_end];
        out.push_str(&rest[..tag_start]);

        if let Some(src) = attr_value(tag, "src") {
            let is_remote = src.starts_with("http://")
                || src.starts_with("https://")
                || src.starts_with("//");
            let new_src = if is_remote {
                // Remote images are never fetched (FR-20).
                "".to_string()
            } else {
                let rel = src.trim_start_matches(['/', '\\']);
                format!(
                    "http://winview.localhost/img/{}/{}",
                    current_token(),
                    rel
                )
            };
            let _ = &base;
            let new_tag = replace_attr(tag, "src", &new_src);
            out.push_str(&new_tag);
        } else {
            out.push_str(tag);
        }
        rest = &rest[tag_end..];
    }
    out.push_str(rest);
    out
}

/// Per-preview token for the image scheme (set by preview::build).
static CURRENT_TOKEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn set_image_token(req_id: u64) {
    CURRENT_TOKEN.store(req_id, std::sync::atomic::Ordering::Relaxed);
}

fn current_token() -> u64 {
    CURRENT_TOKEN.load(std::sync::atomic::Ordering::Relaxed)
}

fn attr_value<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let needle = format!("{name}=\"");
    let start = tag.find(&needle)? + needle.len();
    let end = tag[start..].find('"')? + start;
    Some(&tag[start..end])
}

fn replace_attr(tag: &str, name: &str, value: &str) -> String {
    let needle = format!("{name}=\"");
    if let Some(start) = tag.find(&needle) {
        let vstart = start + needle.len();
        if let Some(rel) = tag[vstart..].find('"') {
            let vend = vstart + rel + 1;
            return format!("{}{}=\"{}\"{}", &tag[..start], name, value, &tag[vend..]);
        }
    }
    format!("{tag}{name}=\"{value}\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fenced_extraction() {
        let src = "before\n```rust\nfn a() {}\n```\nafter\n~~~js\nlet x;\n~~~\n";
        let b = extract_fenced_blocks(src);
        assert_eq!(b.len(), 2);
        assert_eq!(b[0].0, "rust");
        assert_eq!(b[0].1, "fn a() {}\n");
        assert_eq!(b[1].0, "js");
    }

    #[test]
    fn renders_and_sanitizes() {
        let src = "# Hi\n\n> [!NOTE]\n> note body\n\n<script>alert(1)</script>\n\n```rust\nlet x = 1;\n```\n";
        let p = std::path::Path::new("/tmp/readme.md");
        let out = render(p, src);
        assert!(out.html.contains("markdown-alert"));
        assert!(!out.html.contains("<script>"));
        assert!(out.html.contains("code-block"));
    }

    #[test]
    fn debug_comrak_output() {
        let raw = comrak_html("```rust
let x = 1;
```
");
        println!("RAW: {raw:?}");
        println!("SAN: {:?}", sanitize(&raw));
        let fenced = extract_fenced_blocks("```rust
let x = 1;
```
");
        println!("FENCED: {fenced:?}");
    }

    #[test]
    fn mermaid_placeholder() {
        let src = "```mermaid\ngraph TD; A-->B;\n```\n";
        let out = render(std::path::Path::new("/tmp/x.md"), src);
        assert!(out.html.contains("winview-mermaid"));
        assert_eq!(out.mermaid.len(), 1);
        assert!(out.mermaid[0].contains("graph TD"));
    }

    #[test]
    fn image_rewrite_and_remote_strip() {
        let src = "![a](pic.png) ![b](https://evil.com/x.png)\n";
        set_image_token(42);
        let out = render(std::path::Path::new("/tmp/doc.md"), src);
        assert!(out.html.contains("http://winview.localhost/img/42/pic.png"));
        assert!(!out.html.contains("evil.com/x.png\""));
    }
}
