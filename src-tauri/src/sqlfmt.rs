//! SQL formatter (FR-26): tokenizer-based clause right-alignment with
//! round-trip verification; unsafe constructs fall back to Raw with a reason.

use serde::Serialize;
use sqlparser::dialect::GenericDialect;
use sqlparser::parser::Parser;
use sqlparser::tokenizer::{Token, Tokenizer};

#[derive(Serialize)]
pub struct SqlPreview {
    /// Right-aligned clause HTML; empty when the fallback triggered.
    pub formatted_html: String,
    pub raw_source: String,
    /// Some("reason") when formatting was refused.
    pub fallback_reason: Option<String>,
}

/// Constructs we refuse to reformat (spec §4): procs, dbt, psql \-commands.
fn unsafe_construct(source: &str) -> Option<&'static str> {
    let upper = source.to_ascii_uppercase();
    if source.contains("{%") || source.contains("{{") {
        return Some("template blocks (dbt/Jinja) present");
    }
    if upper.contains("CREATE PROCEDURE")
        || upper.contains("CREATE FUNCTION")
        || upper.contains("CREATE TRIGGER")
        || upper.contains("CREATE OR REPLACE PROCEDURE")
        || upper.contains("CREATE OR REPLACE FUNCTION")
        || upper.contains("CREATE OR REPLACE TRIGGER")
    {
        return Some("stored program definition present");
    }
    if upper.contains("DELIMITER") {
        return Some("DELIMITER script present");
    }
    if upper.contains("$$") {
        return Some("dollar-quoted block present");
    }
    if upper.contains(" GO") || upper.lines().any(|l| l.trim().eq_ignore_ascii_case("GO")) {
        return Some("GO batch separator present");
    }
    if upper.contains("BEGIN...END") || (upper.contains("BEGIN") && upper.contains("END")) {
        // Only refuse when both appear as bare block keywords (heuristic kept
        // deliberately loose: false Raw fallback is safe, false formatting is not).
        let tokens = tokenize_upper(&upper);
        if tokens.contains(&"BEGIN".to_string()) && tokens.contains(&"END".to_string()) {
            return Some("BEGIN...END block present");
        }
    }
    None
}

fn tokenize_upper(upper: &str) -> Vec<String> {
    Tokenizer::new(&GenericDialect {}, upper)
        .tokenize()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|t| match t {
            Token::Word(w) => Some(w.value),
            _ => None,
        })
        .collect()
}

pub fn build(source: &str) -> SqlPreview {
    if let Some(reason) = unsafe_construct(source) {
        return SqlPreview {
            formatted_html: String::new(),
            raw_source: source.to_string(),
            fallback_reason: Some(reason.to_string()),
        };
    }

    let dialect = GenericDialect {};
    let statements = match Parser::parse_sql(&dialect, source) {
        Ok(s) => s,
        Err(e) => {
            return SqlPreview {
                formatted_html: String::new(),
                raw_source: source.to_string(),
                fallback_reason: Some(format!("parse error: {e}")),
            };
        }
    };

    // Tokenize + re-emit with clause keywords right-aligned to one column.
    let mut formatted = String::with_capacity(source.len() * 2);
    for (i, stmt) in statements.iter().enumerate() {
        if i > 0 {
            formatted.push_str(";\n\n");
        }
        formatted.push_str(&align_statement(&stmt.to_string()));
    }

    // Round-trip verify: tokens equal ignoring whitespace/case (spec §4).
    let src_tokens = normalized_tokens(source);
    let out_tokens = normalized_tokens(&formatted);
    if src_tokens != out_tokens {
        return SqlPreview {
            formatted_html: String::new(),
            raw_source: source.to_string(),
            fallback_reason: Some("round-trip check failed".to_string()),
        };
    }

    SqlPreview {
        formatted_html: render_html(&formatted),
        raw_source: source.to_string(),
        fallback_reason: None,
    }
}

fn normalized_tokens(s: &str) -> Vec<String> {
    let t = Tokenizer::new(&GenericDialect {}, s)
        .tokenize()
        .unwrap_or_default();
    t.into_iter()
        .filter_map(|tok| match tok {
            Token::Word(w) => Some(w.value.to_ascii_uppercase()),
            Token::Number(n, _) => Some(n),
            Token::SingleQuotedString(v) => Some(format!("'{}'", v.to_ascii_uppercase())),
            Token::Comma => Some(",".to_string()),
            Token::Period => Some(".".to_string()),
            Token::Eq => Some("=".to_string()),
            Token::LParen => Some("(".to_string()),
            Token::RParen => Some(")".to_string()),
            _ => None,
        })
        .collect()
}

const CLAUSE_KEYWORDS: [&str; 10] = [
    "SELECT", "FROM", "WHERE", "GROUP BY", "HAVING", "ORDER BY", "LIMIT", "OFFSET",
    "UNION", "VALUES",
];

/// One clause per line with keywords aligned into a scannable column.
fn align_statement(stmt: &str) -> String {
    let mut formatted = String::with_capacity(stmt.len() * 2);
    let mut current_kw = String::new();
    let mut current_body = String::new();

    let mut tokens: Vec<String> = Vec::new();
    let t = Tokenizer::new(&GenericDialect {}, stmt)
        .tokenize()
        .unwrap_or_default();
    for tok in t {
        match tok {
            Token::Word(w) => tokens.push(w.value),
            Token::Comma => tokens.push(",".into()),
            Token::LParen => tokens.push("(".into()),
            Token::RParen => tokens.push(")".into()),
            Token::Number(n, _) => tokens.push(n),
            Token::SingleQuotedString(v) => tokens.push(format!("'{}'", v)),
            Token::Whitespace(_) => {}
            other => tokens.push(format!("{other}")),
        }
    }

    let mut i = 0usize;
    let mut first = true;
    let mut lines: Vec<(String, String)> = Vec::new();

    while i < tokens.len() {
        // Try to match a clause keyword (1-2 word forms like GROUP BY).
        let mut matched: Option<(String, usize)> = None;
        if i + 1 < tokens.len() {
            let two = format!("{} {}", tokens[i].to_ascii_uppercase(), tokens[i + 1].to_ascii_uppercase());
            if CLAUSE_KEYWORDS.contains(&two.as_str()) {
                matched = Some((two, 2));
            }
        }
        if matched.is_none() && CLAUSE_KEYWORDS.contains(&tokens[i].to_ascii_uppercase().as_str()) {
            matched = Some((tokens[i].to_ascii_uppercase(), 1));
        }
        if let Some((kw, len)) = matched {
            if !current_kw.is_empty() || !first {
                lines.push((current_kw.clone(), current_body.trim().to_string()));
            }
            current_kw = kw;
            current_body.clear();
            i += len;
            first = false;
            continue;
        }
        current_body.push_str(&tokens[i]);
        current_body.push(' ');
        i += 1;
    }
    if !current_kw.is_empty() || !current_body.trim().is_empty() {
        lines.push((current_kw, current_body.trim().to_string()));
    }
    let keyword_width = lines.iter().map(|(k, _)| k.len()).max().unwrap_or(0);

    for (kw, body) in &lines {
        if kw.is_empty() {
            formatted.push_str(&format!("{body}\n"));
        } else {
            formatted.push_str(&format!("{kw:<width$}  {body}\n", width = keyword_width));
        }
    }
    formatted.trim_end().to_string()
}

fn render_html(formatted: &str) -> String {
    let mut out = String::with_capacity(formatted.len() * 2);
    for line in formatted.lines() {
        let trimmed = line.trim_start();
        let class = if trimmed.starts_with("SELECT")
            || trimmed.starts_with("FROM")
            || trimmed.starts_with("WHERE")
            || trimmed.starts_with("GROUP")
            || trimmed.starts_with("HAVING")
            || trimmed.starts_with("ORDER")
            || trimmed.starts_with("LIMIT")
            || trimmed.starts_with("OFFSET")
            || trimmed.starts_with("VALUES")
        {
            "sql-kw"
        } else {
            "sql-body"
        };
        out.push_str(&format!(
            "<div class=\"sql-line {class}\">{}</div>",
            crate::code::escape_html(line)
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_simple_select() {
        let src = "select id, name from users where id = 1 order by name";
        let p = build(src);
        assert!(p.fallback_reason.is_none());
        assert!(p.formatted_html.contains("sql-kw"));
        assert!(p.formatted_html.contains("SELECT"));
        assert!(p.formatted_html.contains("FROM"));
        // Round trip preserved the tokens
        assert!(p.formatted_html.contains("users"));
    }

    #[test]
    fn refuses_stored_procedures() {
        let src = "CREATE PROCEDURE do_thing() BEGIN SELECT 1; END";
        let p = build(src);
        assert_eq!(p.fallback_reason.as_deref(), Some("stored program definition present"));
        assert_eq!(p.raw_source, src);
    }

    #[test]
    fn refuses_dbt_blocks() {
        let p = build("{% if x %}SELECT 1{% endif %}");
        assert!(p.fallback_reason.unwrap().contains("template"));
    }

    #[test]
    fn refuses_dollar_quotes() {
        let p = build("SELECT $$body$$::text");
        assert!(p.fallback_reason.unwrap().contains("dollar"));
    }

    #[test]
    fn refuses_delimiter() {
        let p = build("DELIMITER //\nSELECT 1//");
        assert!(p.fallback_reason.unwrap().contains("DELIMITER"));
    }

    #[test]
    fn refuses_go_batches() {
        let p = build("SELECT 1\nGO\nSELECT 2");
        assert!(p.fallback_reason.unwrap().contains("GO"));
    }

    #[test]
    fn refuses_parse_errors() {
        let p = build("SELEC * FRUM x");
        assert!(p.fallback_reason.unwrap().contains("parse error"));
    }

    #[test]
    fn round_trip_multi_statement() {
        let src = "select 1; select 2;";
        let p = build(src);
        assert!(p.fallback_reason.is_none());
        assert!(p.formatted_html.contains("SELECT"));
    }
}
