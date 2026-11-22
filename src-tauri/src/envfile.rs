//! `.env` previews (FR-24): parse key/values (incl. multiline quoted values),
//! flag likely secrets, mask strictly in Rust. Real values are only sent to
//! the UI through `reveal_env` on explicit user action.

use serde::Serialize;

#[derive(Serialize, Debug)]
pub struct EnvEntry {
    pub key: String,
    pub masked_value: String,
    pub multiline: bool,
    pub secret: bool,
    pub comment: bool,
}

#[derive(Serialize)]
pub struct EnvPreview {
    pub entries: Vec<EnvEntry>,
}

/// Parse and mask. Masked values leak no content: fixed dots equal to a
/// coarse length bucket (never the real length, never content).
pub fn parse(content: &str) -> EnvPreview {
    let mut entries = Vec::new();
    let mut in_multiline: Option<(String, char)> = None;

    for raw in content.lines() {
        if let Some((key, quote)) = in_multiline.take() {
            match line_ends_multiline(raw, quote) {
                Some(_) => {
                    let secret = looks_secret(&key, "");
                    entries.push(EnvEntry {
                        key: crate::code::escape_html(&key),
                        masked_value: mask("multi"),
                        multiline: true,
                        secret,
                        comment: false,
                    });
                }
                None => {
                    in_multiline = Some((key, quote));
                }
            }
            continue;
        }

        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            if trimmed.starts_with('#') && !trimmed.is_empty() {
                entries.push(EnvEntry {
                    key: crate::code::escape_html(trimmed),
                    masked_value: String::new(),
                    multiline: false,
                    secret: false,
                    comment: true,
                });
            }
            continue;
        }
        if trimmed == "export" || trimmed.starts_with("export ") {
            // strip export prefix
        }
        let body = trimmed.strip_prefix("export ").unwrap_or(trimmed);
        let Some(eq) = body.find('=') else {
            continue;
        };
        let key = body[..eq].trim().to_string();
        let mut value = body[eq + 1..].trim().to_string();

        if value.starts_with('"') || value.starts_with('\'') {
            let quote = value.chars().next().unwrap();
            // Single-line quoted value
            if value.len() >= 2 && value.ends_with(quote) && !value[1..].contains(quote) {
                value = value[1..value.len() - 1].to_string();
            } else if !value[1..].contains(quote) {
                // Multiline quoted value: consume until closing quote
                in_multiline = Some((key.clone(), quote));
            }
        }
        let secret = looks_secret(&key, &value);
        entries.push(EnvEntry {
            key: crate::code::escape_html(&key),
            masked_value: mask(&value),
            multiline: false,
            secret,
            comment: false,
        });
    }
    EnvPreview { entries }
}

fn line_ends_multiline(line: &str, quote: char) -> Option<usize> {
    line.find(quote)
}

/// Coarse mask: same visual length class, zero content leakage.
fn mask(_real: &str) -> String {
    "••••••".to_string()
}

/// Key/value heuristics for the SECRET badge (FR-24).
pub fn looks_secret(key: &str, value: &str) -> bool {
    let k = key.to_ascii_uppercase();
    const KEY_HINTS: [&str; 12] = [
        "PASSWORD", "PASSWD", "SECRET", "TOKEN", "API_KEY", "APIKEY", "PRIVATE",
        "ACCESS_KEY", "AUTH", "CREDENTIAL", "CERT", "SIGNING",
    ];
    if KEY_HINTS.iter().any(|h| k.contains(h)) {
        return true;
    }
    // Long random-looking values (mixed classes, no spaces)
    if value.len() >= 24 && !value.contains(' ') {
        let has_lower = value.chars().any(|c| c.is_ascii_lowercase());
        let has_upper = value.chars().any(|c| c.is_ascii_uppercase());
        let has_digit = value.chars().any(|c| c.is_ascii_digit());
        if (has_lower && has_upper && has_digit) || value.starts_with("sk-") || value.starts_with("ghp_") || value.starts_with("AKIA") {
            return true;
        }
    }
    false
}

/// Real values for reveal_env: returns plain (unescaped) values.
pub fn real_values(content: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for raw in content.lines() {
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let body = trimmed.strip_prefix("export ").unwrap_or(trimmed);
        if let Some(eq) = body.find('=') {
            let key = body[..eq].trim().to_string();
            let mut value = body[eq + 1..].trim().to_string();
            if value.len() >= 2
                && (value.starts_with('"') || value.starts_with('\''))
                && value.ends_with(value.chars().next().unwrap())
            {
                value = value[1..value.len() - 1].to_string();
            }
            out.push((key, value));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "DB_HOST=localhost\nDB_PASSWORD=supersecret123\nAPI_KEY=abcd1234\n# a comment\nEMPTY=\nTOKEN=ghp_16charsabcdefghij\nURL=https://example.com\n";

    #[test]
    fn parses_and_masks() {
        let e = parse(SAMPLE);
        let keys: Vec<&str> = e.entries.iter().map(|x| x.key.as_str()).collect();
        assert!(keys.contains(&"DB_HOST"));
        assert!(keys.contains(&"API_KEY"));
        assert!(keys.iter().any(|k| k.contains("comment")));
        for entry in &e.entries {
            if entry.comment {
                continue;
            }
            assert!(!e.entries.iter().any(|x| x.masked_value.contains("supersecret")));
            assert!(!entry.masked_value.contains("localhost"));
            break;
        }
    }

    #[test]
    fn secret_badges() {
        let e = parse(SAMPLE);
        let find = |k: &str| e.entries.iter().find(|x| x.key == k).unwrap();
        assert!(find("DB_PASSWORD").secret);
        assert!(find("API_KEY").secret);
        assert!(find("TOKEN").secret);
        assert!(!find("DB_HOST").secret);
        assert!(!find("URL").secret);
    }

    #[test]
    fn masked_values_leak_nothing() {
        let e = parse(SAMPLE);
        let masked_blob = format!("{:?}", e.entries);
        for leak in ["supersecret123", "abcd1234", "ghp_16chars", "https://example.com"] {
            assert!(!masked_blob.contains(leak), "masked blob leaked {leak}");
        }
    }

    #[test]
    fn reveal_returns_real_values() {
        let real = real_values(SAMPLE);
        let host = real.iter().find(|(k, _)| k == "DB_HOST").unwrap();
        assert_eq!(host.1, "localhost");
        let pw = real.iter().find(|(k, _)| k == "DB_PASSWORD").unwrap();
        assert_eq!(pw.1, "supersecret123");
    }

    #[test]
    fn export_prefix_stripped() {
        let e = parse("export FLAG=1\n");
        assert!(e.entries.iter().any(|x| x.key == "FLAG"));
    }
}
