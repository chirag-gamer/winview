//! Delimited-table previews (FR-22): separator detection, header/numeric
//! detection, up to 200k rows streamed in batches via `tauri::ipc::Channel`.

use serde::Serialize;

pub const MAX_ROWS: usize = 200_000;
pub const BATCH_SIZE: usize = 2_000;

#[derive(Serialize)]
pub struct TableMeta {
    pub columns: Vec<String>,
    pub numeric: Vec<bool>,
    pub separator: String,
    pub total_rows: usize,
    pub sample_rows: Vec<Vec<String>>,
    pub decimal_comma: bool,
    pub truncated: bool,
}

/// Candidate separators, tried in order.
const SEPARATORS: [char; 4] = [',', '\t', ';', '|'];

/// Count separator occurrences outside quoted sections over a sample.
fn count_outside_quotes(sample: &str, sep: char) -> usize {
    let mut count = 0usize;
    let mut in_quotes = false;
    for c in sample.chars() {
        match c {
            '"' => in_quotes = !in_quotes,
            _ if c == sep && !in_quotes => count += 1,
            _ => {}
        }
    }
    count
}

/// Detect the separator across the first ~50 lines (highest count wins).
pub fn detect_separator(content: &str) -> char {
    let sample: String = content.lines().take(50).collect::<Vec<_>>().join("\n");
    let mut best = ',';
    let mut best_count = 0usize;
    for sep in SEPARATORS {
        let c = count_outside_quotes(&sample, sep);
        if c > best_count {
            best = sep;
            best_count = c;
        }
    }
    best
}

/// Parse (headers, data rows) with the given separator using the csv crate.
fn read_rows(content: &str, sep: char, max: usize) -> (Vec<String>, Vec<Vec<String>>, bool) {
    let delim = if sep == '\t' { b'\t' } else { sep as u8 };
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(delim)
        .flexible(true)
        .has_headers(true)
        .from_reader(content.as_bytes());
    let headers: Vec<String> = reader
        .headers()
        .map(|h| h.iter().map(|s| s.to_string()).collect())
        .unwrap_or_default();
    let mut rows = Vec::new();
    for rec in reader.records().by_ref().take(max) {
        match rec {
            Ok(r) => rows.push(r.iter().map(|s| s.to_string()).collect()),
            Err(_) => continue,
        }
    }
    let drained = reader.records().count() > 0;
    (headers, rows, drained)
}

pub fn analyze(content: &str) -> TableMeta {
    let sep = detect_separator(content);
    let (columns, data, more) = read_rows(content, sep, MAX_ROWS);
    let numeric = numeric_columns(&columns, &data);
    let sample = data.iter().take(100).cloned().collect();
    let decimal_comma = sep == ';';
    TableMeta {
        columns,
        numeric,
        separator: sep.to_string(),
        total_rows: data.len(),
        sample_rows: sample,
        decimal_comma,
        truncated: more || data.len() >= MAX_ROWS,
    }
}

/// A column is numeric when ≥90% of non-empty sample cells parse as numbers
/// (decimal comma supported).
fn numeric_columns(columns: &[String], rows: &[Vec<String>]) -> Vec<bool> {
    let width = columns.len();
    let mut flags = vec![true; width];
    let mut any = vec![0usize; width];
    let mut numeric = vec![0usize; width];
    let sample = rows.iter().take(1000);
    for row in sample {
        for (i, cell) in row.iter().enumerate().take(width) {
            let t = cell.trim();
            if t.is_empty() {
                continue;
            }
            any[i] += 1;
            if is_number(t) {
                numeric[i] += 1;
            }
        }
    }
    for i in 0..width {
        if any[i] == 0 || numeric[i] * 10 < any[i] * 9 {
            flags[i] = false;
        }
    }
    flags
}

fn is_number(s: &str) -> bool {
    let t = s.trim().trim_matches('"');
    if t.is_empty() {
        return false;
    }
    let normalized = t.replace(',', ".");
    normalized.parse::<f64>().is_ok()
        && !t.chars().next().unwrap_or('x').is_alphabetic()
}

/// Stream rows in batches for the frontend grid.
pub fn rows_batch(content: &str, sep: char, batch_index: usize) -> Vec<Vec<String>> {
    let (_, data, _) = read_rows(content, sep, MAX_ROWS);
    data.iter()
        .skip(batch_index * BATCH_SIZE)
        .take(BATCH_SIZE)
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_comma() {
        assert_eq!(detect_separator("a,b,c\n1,2,3\n"), ',');
    }

    #[test]
    fn detects_tab() {
        assert_eq!(detect_separator("a\tb\n1\t2\n"), '\t');
    }

    #[test]
    fn detects_semicolon_with_decimal_comma() {
        let content = "name;value\nfoo;1,5\nbar;2,0\n";
        let meta = analyze(content);
        assert_eq!(meta.separator, ";");
        assert!(meta.decimal_comma);
    }

    #[test]
    fn numeric_column_detection() {
        let meta = analyze("id,name,score\n1,Alice,9.5\n2,Bob,7.2\n3,Cal,8.1\n");
        assert_eq!(meta.columns, ["id", "name", "score"]);
        assert!(meta.numeric[0], "id numeric");
        assert!(!meta.numeric[1], "name not numeric");
        assert!(meta.numeric[2], "score numeric");
        assert_eq!(meta.total_rows, 3);
    }

    #[test]
    fn quoted_cells_with_separators() {
        let meta = analyze("name,note\n\"Doe, John\",\"likes commas, a lot\"\n");
        assert_eq!(meta.columns, ["name", "note"]);
        assert_eq!(meta.sample_rows[0][0], "Doe, John");
        assert_eq!(meta.sample_rows[0][1], "likes commas, a lot");
    }

    #[test]
    fn batches() {
        let mut content = String::from("n\n");
        for i in 0..5000 {
            content.push_str(&format!("{i}\n"));
        }
        let b0 = rows_batch(&content, ',', 0);
        let b2 = rows_batch(&content, ',', 2);
        assert_eq!(b0.len(), BATCH_SIZE);
        assert_eq!(b2.len(), 1000);
        assert_eq!(b2[0][0], "4000");
    }
}
