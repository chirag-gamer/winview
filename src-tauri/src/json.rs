//! JSON / JSONC / JSONL previews (FR-21): Rust-built tree (key order +
//! numeric precision preserved via serde_json preserve_order/arbitrary_precision).

use serde::Serialize;
use serde_json::Value;

/// Cap the rendered node count; beyond it containers collapse into summaries.
const MAX_NODES: usize = 4000;

#[derive(Serialize)]
pub struct JsonTree {
    pub html: String,
    pub node_count: usize,
    pub truncated: bool,
}

pub fn tree(value: &Value) -> JsonTree {
    let mut out = String::with_capacity(4096);
    let mut count = 0usize;
    render_value(value, &mut out, &mut count, "$");
    JsonTree { html: out, node_count: count, truncated: count >= MAX_NODES }
}

fn render_value(v: &Value, out: &mut String, count: &mut usize, path: &str) {
    *count += 1;
    if *count > MAX_NODES {
        out.push_str("<div class=\"jtrunc\">…</div>");
        return;
    }
    match v {
        Value::Object(map) => {
            out.push_str(&format!(
                "<div class=\"jnode\" data-path=\"{}\"><span class=\"jbrace\">{{</span>",
                escape_attr(path)
            ));
            if map.is_empty() {
                out.push_str("<span class=\"jbrace\">}</span></div>");
                return;
            }
            out.push_str("<div class=\"jchildren\">");
            for (k, val) in map {
                out.push_str(&format!(
                    "<div class=\"jpair\"><span class=\"jkey\">{}</span>: ",
                    crate::code::escape_html(k)
                ));
                render_value(val, out, count, &format!("{path}.{k}"));
                out.push_str("</div>");
            }
            out.push_str("<div class=\"jbrace\">}</div></div>");
        }
        Value::Array(items) => {
            out.push_str(&format!(
                "<div class=\"jnode\" data-path=\"{}\"><span class=\"jbrace\">[</span>",
                escape_attr(path)
            ));
            if items.is_empty() {
                out.push_str("<span class=\"jbrace\">]</span></div>");
                return;
            }
            out.push_str("<div class=\"jchildren\">");
            for (i, val) in items.iter().enumerate() {
                out.push_str(&format!(
                    "<div class=\"jpair\"><span class=\"jindex\">{i}</span>: "
                ));
                render_value(val, out, count, &format!("{path}[{i}]"));
                out.push_str("</div>");
            }
            out.push_str("<div class=\"jbrace\">]</div></div>");
        }
        Value::String(s) => {
            out.push_str(&format!(
                "<span class=\"jval jstr\">{}</span></div>",
                crate::code::escape_html(&format!("\"{s}\""))
            ));
        }
        Value::Number(n) => {
            out.push_str(&format!(
                "<span class=\"jval jnum\">{}</span></div>",
                crate::code::escape_html(&n.to_string())
            ));
        }
        Value::Bool(b) => {
            out.push_str(&format!(
                "<span class=\"jval jbool\">{b}</span></div>"
            ));
        }
        Value::Null => {
            out.push_str("<span class=\"jval jnull\">null</span></div>");
        }
    }
}

fn escape_attr(s: &str) -> String {
    crate::code::escape_html(s)
}

/// JSONL: one record per line; the frontend lazy-expands via `jsonl_record`.
pub fn jsonl_summary(content: &str) -> (usize, Vec<String>) {
    let lines: Vec<String> = content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(crate::code::escape_html)
        .collect();
    (lines.len(), lines)
}

pub fn expand_record(content: &str, line_no: usize) -> Result<String, String> {
    let lines: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();
    let line = lines
        .get(line_no)
        .ok_or_else(|| format!("line {line_no} out of range"))?;
    let v: Value = jsonc_parse(line)?;
    Ok(tree(&v).html)
}

/// Parse JSON or JSONC (comments tolerated).
pub fn jsonc_parse(text: &str) -> Result<Value, String> {
    match serde_json::from_str::<Value>(text) {
        Ok(v) => Ok(v),
        Err(e1) => jsonc_parser::parse_to_serde_value::<Value>(
            text,
            &jsonc_parser::ParseOptions::default(),
        )
        .map_err(|e2| format!("{e1} / {e2}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_key_order() {
        let v = jsonc_parse("{\"zz\": 1, \"aa\": 2}").unwrap();
        let t = tree(&v);
        let zz = t.html.find("zz").unwrap();
        let aa = t.html.find("aa").unwrap();
        assert!(zz < aa, "preserve_order must keep insertion order");
    }

    #[test]
    fn parses_jsonc_comments() {
        let v = jsonc_parse("{\n  // hi\n  \"a\": 1 /* tail */\n}").unwrap();
        assert_eq!(v.get("a").and_then(|x| x.as_i64()), Some(1));
    }

    #[test]
    fn big_numbers_survive() {
        let v = jsonc_parse("{\"big\": 123456789012345678901234567890}").unwrap();
        let s = v.get("big").map(|x| x.to_string()).unwrap_or_default();
        assert!(s.contains("123456789012345678901234567890"), "{s}");
    }

    #[test]
    fn jsonl_expand() {
        let content = "{\"a\": 1}\n{\"b\": [1,2,3]}\n";
        let (n, lines) = jsonl_summary(content);
        assert_eq!(n, 2);
        assert_eq!(lines.len(), 2);
        let expanded = expand_record(content, 1).unwrap();
        assert!(expanded.contains("jkey") && expanded.contains("b"));
    }
}
