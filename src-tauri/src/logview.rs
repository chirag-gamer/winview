//! Log-file previews (FR-25): tail-first for large files, original line
//! numbers preserved, level/timestamp/JSON-line highlighting, errors filter.

use serde::Serialize;

/// Tail window for large logs (spec §4: last 4 MB).
const TAIL_BYTES: usize = 4 * 1024 * 1024;
/// Hard cap on rendered lines (frontend scrolls the rest virtually).
const MAX_LINES: usize = 20_000;

#[derive(Serialize)]
pub struct LogPreview {
    pub html: String,
    pub first_line_no: usize,
    pub total_lines: usize,
    pub truncated: bool,
    /// True when the whole file was too large to keep in memory for filtering.
    pub tail_only: bool,
}

/// Read the file (tail for big ones) and render lines.
pub fn build(path: &std::path::Path) -> LogPreview {
    let meta = std::fs::metadata(path);
    let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
    let tail_only = size > TAIL_BYTES as u64;
    let bytes = if tail_only {
        let f = std::fs::File::open(path);
        let mut buf = Vec::with_capacity(TAIL_BYTES);
        if let Ok(mut f) = f {
            use std::io::{Read, Seek, SeekFrom};
            if f.seek(SeekFrom::End(-(TAIL_BYTES as i64))).is_ok() {
                let _ = f.read_to_end(&mut buf);
            }
        }
        buf
    } else {
        std::fs::read(path).unwrap_or_default()
    };

    let text = String::from_utf8_lossy(&bytes);
    // Preserve original line numbers: count newlines before the tail start.
    let mut skipped = 0usize;
    if tail_only {
        let full = std::fs::read_to_string(path).unwrap_or_default();
        skipped = memchr::memchr_iter(b'\n', full.as_bytes()).count();
        skipped = skipped.saturating_sub(
            memchr::memchr_iter(b'\n', &bytes).count(),
        );
    }

    let lines: Vec<&str> = text.lines().collect();
    let total_lines = skipped + lines.len();
    let first_line_no = skipped + 1;
    let truncated = lines.len() > MAX_LINES;
    let start = if truncated { lines.len() - MAX_LINES } else { 0 };

    let mut out = String::with_capacity(bytes.len() + lines.len() * 24);
    for (i, line) in lines.iter().enumerate().skip(start) {
        let class = classify_line(line);
        let escaped = crate::code::escape_html(line);
        out.push_str(&format!(
            "<div class=\"log-line {class}\" data-line=\"{}\">{escaped}</div>",
            first_line_no + i
        ));
    }
    LogPreview {
        html: out,
        first_line_no: first_line_no + start,
        total_lines,
        truncated,
        tail_only,
    }
}

/// CSS class per line: error / warn / info / debug / json / trace-continuation.
fn classify_line(line: &str) -> &'static str {
    let indented = line.starts_with(' ') || line.starts_with('\t');
    let t = line.trim();
    if indented && (t.starts_with("at ") || t.starts_with("... ") || t.starts_with("Caused by")) {
        return "log-trace";
    }
    // Structured JSON log lines win over embedded level words.
    if t.starts_with('{') && t.ends_with('}') {
        return "log-json";
    }
    let lower = t.to_ascii_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    let has = |kw: &str| words.contains(&kw);
    let pair = |a: &str, b: &str| words.windows(2).any(|w| w[0] == a && w[1] == b);
    if has("error") || has("fatal") || has("panic") || pair("level", "error") {
        return "log-error";
    }
    if has("warn") || has("warning") || pair("level", "warn") {
        return "log-warn";
    }
    if has("info") || pair("level", "info") {
        return "log-info";
    }
    if has("debug") || has("trace") || pair("level", "debug") {
        return "log-debug";
    }
    "log-plain"
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_log(content: &[u8]) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!(
            "winview-log-{}.log",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut f = std::fs::File::create(&p).unwrap();
        f.write_all(content).unwrap();
        p
    }

    #[test]
    fn level_classification() {
        let p = temp_log(
            b"2026-01-01 INFO started\n2026-01-01 ERROR db down\n  at db.rs:10\n2026-01-01 WARN slow\n2026-01-01 DEBUG x\n{\"level\":\"info\",\"msg\":\"json line\"}\n",
        );
        let lp = build(&p);
        assert!(lp.html.contains("log-error"));
        assert!(lp.html.contains("log-trace"));
        assert!(lp.html.contains("log-warn"));
        assert!(lp.html.contains("log-debug"));
        assert!(lp.html.contains("log-json"));
        assert!(lp.html.contains("data-line=\"1\""));
        assert_eq!(lp.first_line_no, 1);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn tail_preserves_line_numbers() {
        // Build a log > 4 MB where the tail starts mid-file.
        let line = "INFO filler line to grow the log file beyond the tail window\n";
        let mut big = Vec::with_capacity(TAIL_BYTES + line.len() * 10);
        for i in 0..90_000 {
            big.extend_from_slice(format!("{i} {line}").as_bytes());
        }
        let p = temp_log(&big);
        let lp = build(&p);
        assert!(lp.tail_only);
        assert!(lp.first_line_no > 1, "first line number must reflect the skip");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn max_lines_cap() {
        let mut big = Vec::new();
        for i in 0..25_000 {
            big.extend_from_slice(format!("line {i}\n").as_bytes());
        }
        let p = temp_log(&big);
        let lp = build(&p);
        assert!(lp.truncated);
        assert_eq!(lp.total_lines, 25_000);
        let _ = std::fs::remove_file(&p);
    }
}
