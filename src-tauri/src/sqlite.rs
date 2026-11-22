//! Read-only SQLite inspector (FR-23): immutable URI open, schema listing,
//! content capped at 1,000 rows/table, WAL warning.

use rusqlite::OpenFlags;
use serde::Serialize;

#[derive(Serialize)]
pub struct SqliteSchema {
    pub tables: Vec<TableInfo>,
    pub wal_warning: bool,
    pub error: Option<String>,
}

#[derive(Serialize, Clone)]
pub struct TableInfo {
    pub name: String,
    pub kind: String, // table | view
    pub columns: Vec<Column>,
    pub indexes: Vec<String>,
    pub row_count: u64,
}

#[derive(Serialize, Clone)]
pub struct Column {
    pub name: String,
    pub col_type: String,
    pub pk: bool,
    pub notnull: bool,
}

/// Open read-only via immutable URI (never writes, never journals — D-013 spec §5).
fn open(path: &str) -> Result<rusqlite::Connection, String> {
    let uri = format!("file://{}?immutable=1", to_uri_path(path));
    rusqlite::Connection::open_with_flags(
        uri,
        OpenFlags::SQLITE_OPEN_URI
            | OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| e.to_string())
}

fn to_uri_path(path: &str) -> String {
    let p = path.replace('\\', "/");
    if p.starts_with('/') {
        p
    } else {
        format!("/{p}")
    }
}

pub fn schema(path: &str) -> SqliteSchema {
    let wal_warning = std::path::Path::new(&format!("{path}-wal"))
        .metadata()
        .map(|m| m.len() > 0)
        .unwrap_or(false);

    let conn = match open(path) {
        Ok(c) => c,
        Err(e) => {
            return SqliteSchema { tables: Vec::new(), wal_warning, error: Some(e) };
        }
    };

    let mut tables = Vec::new();
    let query = "SELECT name, type FROM sqlite_schema WHERE type IN ('table','view') AND name NOT LIKE 'sqlite_%' ORDER BY name";
    let Ok(mut stmt) = conn.prepare(query) else {
        return SqliteSchema { tables, wal_warning, error: Some("schema query failed".into()) };
    };
    let names: Vec<(String, String)> = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .ok()
        .map(|rows| rows.filter_map(|r| r.ok()).collect())
        .unwrap_or_default();

    for (name, kind) in names {
        let columns = columns_of(&conn, &name);
        let indexes = indexes_of(&conn, &name);
        let row_count = count_of(&conn, &name);
        tables.push(TableInfo { name, kind, columns, indexes, row_count });
    }
    SqliteSchema { tables, wal_warning, error: None }
}

fn columns_of(conn: &rusqlite::Connection, table: &str) -> Vec<Column> {
    let sql = format!("PRAGMA table_xinfo({})", quote_ident(table));
    let Ok(mut stmt) = conn.prepare(&sql) else { return Vec::new() };
    let rows = stmt.query_map([], |r| {
        let name: String = r.get(1)?;
        let col_type: String = r.get(2)?;
        let notnull: i64 = r.get(3)?;
        let pk: i64 = r.get(5)?;
        Ok(Column { name, col_type, pk: pk > 0, notnull: notnull > 0 })
    });
    match rows {
        Ok(rows) => rows.filter_map(|r| r.ok()).collect(),
        Err(_) => Vec::new(),
    }
}

fn indexes_of(conn: &rusqlite::Connection, table: &str) -> Vec<String> {
    let sql = format!("PRAGMA index_list({})", quote_ident(table));
    let Ok(mut stmt) = conn.prepare(&sql) else { return Vec::new() };
    let rows = stmt.query_map([], |r| r.get::<_, String>(1));
    match rows {
        Ok(rows) => rows.filter_map(|r| r.ok()).collect(),
        Err(_) => Vec::new(),
    }
}

fn count_of(conn: &rusqlite::Connection, table: &str) -> u64 {
    let sql = format!("SELECT count(*) FROM {}", quote_ident(table));
    conn.query_row(&sql, [], |r| r.get::<_, i64>(0))
        .map(|v| v.max(0) as u64)
        .unwrap_or(0)
}

/// Content query capped at 1,000 rows (FR-23), identifier-quoted.
pub fn rows(path: &str, table: &str, offset: usize, limit: usize) -> Result<(Vec<Vec<String>>, usize), String> {
    let limit = limit.min(1000);
    let conn = open(path)?;
    let count: u64 = count_of(&conn, table);
    let sql = format!(
        "SELECT * FROM {} LIMIT {} OFFSET {}",
        quote_ident(table),
        limit,
        offset
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let width = stmt.column_count();
    let mut rows = Vec::new();
    let mut q = stmt.query([]).map_err(|e| e.to_string())?;
    let mut progress = 0usize;
    while let Some(row) = q.next().map_err(|e| e.to_string())? {
        if progress >= limit {
            break;
        }
        progress += 1;
        let mut vals = Vec::with_capacity(width);
        for i in 0..width {
            let v = row.get_ref(i).map_err(|e| e.to_string())?;
            let s = match v {
                rusqlite::types::ValueRef::Null => "NULL".to_string(),
                rusqlite::types::ValueRef::Integer(n) => n.to_string(),
                rusqlite::types::ValueRef::Real(f) => f.to_string(),
                rusqlite::types::ValueRef::Text(t) => {
                    String::from_utf8_lossy(t).into_owned()
                }
                rusqlite::types::ValueRef::Blob(b) => format!("<blob {} bytes>", b.len()),
            };
            vals.push(s);
        }
        rows.push(vals);
    }
    Ok((rows, count as usize))
}

/// Double-quoted identifier with embedded quote doubling.
pub fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_db() -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!(
            "winview-test-{}.db",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_file(&p);
        let conn = rusqlite::Connection::open(&p).unwrap();
        conn.execute_batch(
            "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
             INSERT INTO users VALUES (1, 'alice'), (2, 'bob');
             CREATE INDEX idx_name ON users(name);
             CREATE VIEW v AS SELECT name FROM users;",
        )
        .unwrap();
        p
    }

    #[test]
    fn schema_and_rows_readonly() {
        let p = temp_db();
        let s = schema(p.to_str().unwrap());
        assert!(s.error.is_none());
        assert_eq!(s.tables.len(), 2);
        let users = s.tables.iter().find(|t| t.name == "users").unwrap();
        assert_eq!(users.columns.len(), 2);
        assert!(users.columns[0].pk);
        assert!(users.columns[1].notnull);
        assert!(users.indexes.iter().any(|i| i.contains("idx_name")));
        assert_eq!(users.row_count, 2);
        let (rows, count) = rows(p.to_str().unwrap(), "users", 0, 1000).unwrap();
        assert_eq!(count, 2);
        assert_eq!(rows[0][1], "alice");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn no_journal_files_created() {
        let p = temp_db();
        let s = schema(p.to_str().unwrap());
        assert!(s.error.is_none());
        assert!(!p.with_extension("db-journal").exists());
        assert!(!p.with_extension("db-wal").exists());
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn quoted_identifiers() {
        assert_eq!(quote_ident("we\"ird"), "\"we\"\"ird\"");
    }

    #[test]
    fn wal_warning_detected() {
        let p = temp_db();
        let wal = p.with_extension("db-wal");
        let mut f = std::fs::File::create(&wal).unwrap();
        f.write_all(b"x").unwrap();
        let s = schema(p.to_str().unwrap());
        assert!(s.wal_warning);
        let _ = std::fs::remove_file(&p);
        let _ = std::fs::remove_file(&wal);
    }
}
