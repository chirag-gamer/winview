//! Jupyter notebook previews (FR-27): nbformat 4 (fallback parse), richest
//! output selection, ANSI error colouring, Script tab with `# %%` separators.

use serde::Serialize;
use serde_json::Value;

#[derive(Serialize)]
pub struct NotebookPreview {
    pub cells: Vec<NotebookCell>,
    pub script: String,
    pub error: Option<String>,
}

#[derive(Serialize)]
pub struct NotebookCell {
    pub cell_type: String,
    /// Rendered HTML for markdown cells; highlighted HTML for code cells.
    pub html: String,
    /// Rendered outputs (each already the richest-format HTML).
    pub outputs: Vec<String>,
    pub execution_count: Option<i64>,
}

pub fn build(content: &str) -> NotebookPreview {
    let v: Value = match serde_json::from_str(content) {
        Ok(v) => v,
        Err(e) => {
            return NotebookPreview { cells: Vec::new(), script: String::new(), error: Some(e.to_string()) };
        }
    };
    // nbformat 4 nests under "cells"; nbformat 3 uses "worksheets".
    let cells_json: Vec<Value> = v
        .get("cells")
        .and_then(|c| c.as_array())
        .cloned()
        .or_else(|| {
            v.get("worksheets")
                .and_then(|w| w.as_array())
                .and_then(|w| w.first())
                .and_then(|w| w.get("cells"))
                .and_then(|c| c.as_array())
                .cloned()
        })
        .unwrap_or_default();

    let mut cells = Vec::new();
    let mut script = String::new();
    for cell in &cells_json {
        let cell_type = cell
            .get("cell_type")
            .and_then(|c| c.as_str())
            .unwrap_or("raw")
            .to_string();
        let source = source_of(cell);
        match cell_type.as_str() {
            "markdown" => {
                let payload = crate::markdown::render(&std::path::PathBuf::from("x.md"), &source);
                cells.push(NotebookCell {
                    cell_type: "markdown".into(),
                    html: payload.html,
                    outputs: Vec::new(),
                    execution_count: None,
                });
                script.push_str(&comment_block(&source));
            }
            "code" => {
                let exec = cell.get("execution_count").and_then(|e| e.as_i64());
                let lang = cell
                    .pointer("/metadata/language_info/name")
                    .and_then(|l| l.as_str())
                    .unwrap_or("python")
                    .to_string();
                let highlighted =
                    crate::code::highlight(&std::path::PathBuf::from(format!("cell.{}", lang)), &source);
                let outputs: Vec<String> = cell
                    .get("outputs")
                    .and_then(|o| o.as_array())
                    .map(|arr| arr.iter().filter_map(render_output).collect())
                    .unwrap_or_default();
                cells.push(NotebookCell {
                    cell_type: "code".into(),
                    html: highlighted.html,
                    outputs,
                    execution_count: exec,
                });
                script.push_str("# %%\n");
                script.push_str(&source);
                if !script.ends_with('\n') {
                    script.push('\n');
                }
                script.push('\n');
            }
            _ => {
                cells.push(NotebookCell {
                    cell_type,
                    html: format!(
                        "<pre class=\"nb-raw\">{}</pre>",
                        crate::code::escape_html(&source)
                    ),
                    outputs: Vec::new(),
                    execution_count: None,
                });
            }
        }
    }
    NotebookPreview { cells, script, error: None }
}

/// Extract a text value: a JSON string or an array of strings (nbformat).
fn text_of(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Array(a) => a
            .iter()
            .filter_map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}

fn source_of(cell: &Value) -> String {
    match cell.get("source") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}

/// Pick the richest output per spec §4: svg > png/jpeg > html > markdown >
/// latex > json > text.
fn render_output(out: &Value) -> Option<String> {
    let out_type = out.get("output_type").and_then(|t| t.as_str())?;
    match out_type {
        "stream" => {
            let text = out.get("text").map(text_of).unwrap_or_default();
            Some(format!(
                "<pre class=\"nb-out\">{}</pre>",
                crate::code::escape_html(&text)
            ))
        }
        "error" => {
            // ANSI tracebacks -> coloured text (FR-27)
            let trace = out.get("traceback").and_then(|t| t.as_array()).map(|a| {
                a.iter()
                    .filter_map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join("\n")
            });
            let text = trace.unwrap_or_default();
            let html = ansi_to_html::convert(&text).unwrap_or_else(|_| {
                crate::code::escape_html(&text).replace('\n', "<br>")
            });
            Some(format!("<div class=\"nb-error\">{html}</div>"))
        }
        "display_data" | "execute_result" => {
            let data = out.get("data")?.as_object()?;
            // Prefer image/svg then html then markdown then latex then json then text
            if let Some(Value::String(svg)) = data.get("image/svg+xml") {
                return Some(format!("<div class=\"nb-svg\">{svg}</div>"));
            }
            for key in ["image/png", "image/jpeg"] {
                if let Some(Value::String(b64)) = data.get(key) {
                    let mime = key.split('/').nth(1).unwrap_or("png");
                    return Some(format!(
                        "<img class=\"nb-img\" src=\"data:image/{mime};base64,{b64}\"/>"
                    ));
                }
            }
            if let Some(Value::String(h)) = data.get("text/html") {
                return Some(sanitize_nb_html(h));
            }
            if let Some(Value::String(m)) = data.get("text/markdown") {
                let payload = crate::markdown::render(&std::path::PathBuf::from("x.md"), m);
                return Some(payload.html);
            }
            if let Some(Value::String(l)) = data.get("text/latex") {
                return Some(format!(
                    "<pre class=\"nb-latex\">{}</pre>",
                    crate::code::escape_html(l)
                ));
            }
            if let Some(j) = data.get("application/json") {
                return Some(format!(
                    "<pre class=\"nb-out\">{}</pre>",
                    crate::code::escape_html(&j.to_string())
                ));
            }
            if let Some(t) = data.get("text/plain") {
                let text = text_of(t);
                return Some(format!(
                    "<pre class=\"nb-out\">{}</pre>",
                    crate::code::escape_html(&text)
                ));
            }
            None
        }
        _ => None,
    }
}

/// Notebook HTML outputs are sanitized with the markdown sanitizer config.
fn sanitize_nb_html(h: &str) -> String {
    // Reuse the same strict posture as markdown: strip scripts/handlers.
    let cleaned = ammonia::Builder::default()
        .generic_attribute_prefixes(std::collections::HashSet::from(["data-"]))
        .clean(h)
        .to_string();
    cleaned
}

fn comment_block(src: &str) -> String {
    let mut out = String::new();
    for line in src.lines() {
        out.push_str(&format!("# {line}\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const NB: &str = r##"{
  "cells": [
    {"cell_type": "markdown", "source": ["# Title\n", "text"]},
    {"cell_type": "code", "execution_count": 1, "source": ["print('hi')\n"],
     "outputs": [
       {"output_type": "stream", "text": ["hi\n"]},
       {"output_type": "error", "ename": "ValueError", "evalue": "boom",
        "traceback": ["\u001b[0;31mValueError\u001b[0m boom"]},
       {"output_type": "display_data", "data": {"image/png": "aGk="}},
       {"output_type": "execute_result", "data": {"text/plain": ["42"]}}
     ]}
  ],
  "metadata": {},
  "nbformat": 4
}"##;

    #[test]
    fn dbg_outputs() {
        let nb = build(NB);
        println!("CELL1-HTML: {:?}", nb.cells[1].html.chars().take(80).collect::<String>());
        println!("OUTS: {:?}", nb.cells[1].outputs.iter().map(|o| o.chars().take(60).collect::<String>()).collect::<Vec<_>>());
        let probe = serde_json::json!({"output_type": "stream", "text": ["hi
"]});
        println!("PROBE-TEXT: {:?}", probe.get("text").map(source_of));
        println!("NB-STREAM: {:?}", serde_json::from_str::<serde_json::Value>(NB).unwrap()
            .pointer("/cells/1/outputs/0").map(source_of));
    }

    #[test]
    fn renders_cells_and_outputs() {
        let nb = build(NB);
        assert!(nb.error.is_none());
        assert_eq!(nb.cells.len(), 2);
        assert!(nb.cells[0].html.contains("markdown-body") || nb.cells[0].html.contains("Title"));
        assert_eq!(nb.cells[1].execution_count, Some(1));
        assert_eq!(nb.cells[1].outputs.len(), 4);
        assert!(nb.cells[1].outputs[0].contains("hi"));
        assert!(nb.cells[1].outputs[1].contains("nb-error"));
        assert!(nb.cells[1].outputs[2].contains("data:image/png;base64,aGk="));
        assert!(nb.cells[1].outputs[3].contains("42"));
    }

    #[test]
    fn script_tab_compiles() {
        let nb = build(NB);
        assert!(nb.script.contains("# %%"));
        assert!(nb.script.contains("print('hi')"));
        assert!(nb.script.contains("# # Title"));
    }

    #[test]
    fn invalid_json_reports_error() {
        let nb = build("{not json");
        assert!(nb.error.is_some());
    }

    #[test]
    fn sanitizes_html_outputs() {
        let nb = build(
            r#"{"cells":[{"cell_type":"code","source":["x"],"outputs":[{"output_type":"display_data","data":{"text/html":"<b>ok</b><script>evil()</script>"}}]}]}"#,
        );
        let out = &nb.cells[0].outputs[0];
        assert!(out.contains("<b>ok</b>"));
        assert!(!out.contains("evil()"));
    }
}
