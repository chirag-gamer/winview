//! File-type detection (FR-15/16): special names → extensions → content sniff.

use std::path::Path;

/// What renderer should handle this file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Code,
    Markdown,
    Mermaid,
    Json,
    JsonLines,
    Table,
    Sqlite,
    Log,
    Env,
    Sql,
    Notebook,
    Archive,
    Xml,
    Plist,
    Diff,
    PackageJson,
    Image,
    Audio,
    Video,
    Pdf,
}

/// Special filenames checked before extensions (case-insensitive).
fn special_name(name: &str, upper_name: &str) -> Option<Kind> {
    // .env and .env.*
    if name.eq_ignore_ascii_case(".env") || name.to_ascii_lowercase().starts_with(".env.") {
        return Some(Kind::Env);
    }
    match upper_name {
        "DOCKERFILE" | "CONTAINERFILE" => return Some(Kind::Code),
        "MAKEFILE" | "GNUMAKEFILE" => return Some(Kind::Code),
        "RUSTFMT.TOML" | "CARGO.TOML" => return Some(Kind::Code),
        "PACKAGE.JSON" => return Some(Kind::PackageJson),
        ".GITIGNORE" | ".GITATTRIBUTES" | ".EDITORCONFIG" | ".NPMRC" | ".BAK" => {
            return Some(Kind::Code)
        }
        _ => {}
    }
    // tsconfig*.json / jsconfig.json
    let lower = name.to_ascii_lowercase();
    if lower.starts_with("tsconfig") && lower.ends_with(".json") {
        return Some(Kind::Json);
    }
    if lower == "jsconfig.json" {
        return Some(Kind::Json);
    }
    None
}

fn by_extension(ext: &str) -> Option<Kind> {
    let e = ext.to_ascii_lowercase();
    Some(match e.as_str() {
        "md" | "markdown" => Kind::Markdown,
        "mmd" | "mermaid" => Kind::Mermaid,
        "json" => Kind::Json,
        "jsonc" => Kind::Json,
        "jsonl" | "ndjson" => Kind::JsonLines,
        "csv" | "tsv" | "tab" | "psv" => Kind::Table,
        "sqlite" | "sqlite3" | "db" => Kind::Sqlite,
        "log" | "out" => Kind::Log,
        "sql" => Kind::Sql,
        "ipynb" => Kind::Notebook,
        "zip" | "tar" | "tgz" | "gz" => Kind::Archive,
        "xml" | "xaml" | "csproj" | "vbproj" | "props" | "targets" => Kind::Xml,
        "plist" | "entitlements" => Kind::Plist,
        "diff" | "patch" => Kind::Diff,
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "svg" | "ico" => Kind::Image,
        "mid" | "midi" | "mp3" | "wav" | "flac" | "ogg" => Kind::Audio,
        "mp4" | "mkv" | "avi" | "mov" | "webm" => Kind::Video,
        "pdf" => Kind::Pdf,
        _ => {
            if is_code_extension(&e) {
                Kind::Code
            } else {
                return None;
            }
        }
    })
}

/// Extensions syntect handles (language list matches two-face's set).
pub fn is_code_extension(ext: &str) -> bool {
    matches!(
        ext,
        "rs" | "py" | "pyw" | "js" | "mjs" | "cjs" | "jsx" | "ts" | "tsx" | "go" | "java"
            | "kt" | "kts" | "c" | "h" | "cpp" | "hpp" | "cc" | "m" | "mm" | "cs" | "rb"
            | "erb" | "php" | "css" | "scss" | "sass" | "less" | "lua" | "pl" | "pm"
            | "dart" | "scala" | "ps1" | "psm1" | "sh" | "bash" | "zsh" | "fish" | "tf"
            | "tfvars" | "hcl" | "toml" | "yaml" | "yml" | "ini" | "cfg" | "conf"
            | "dockerfile" | "mk" | "gradle" | "swift" | "vb" | "fs" | "fsx" | "ex"
            | "exs" | "elm" | "jl" | "r" | "nim" | "zig" | "vue" | "svelte" | "asm"
            | "bat" | "cmd" | "make" | "sql" | "groovy" | "perl" | "proto" | "graphql"
            | "env"
    )
}

/// Human label for code files (badge).
pub fn language_label(ext: &str) -> String {
    ext.to_ascii_uppercase()
}

/// Full detection: special names → extension → (caller sniffs for text/binary).
pub fn detect(path: &Path) -> Option<Kind> {
    let name = path.file_name()?.to_string_lossy().into_owned();
    let upper = name.to_ascii_uppercase();
    if let Some(k) = special_name(&name, &upper) {
        return Some(k);
    }
    let ext = path.extension()?.to_string_lossy().into_owned();
    by_extension(&ext)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn special_names_first() {
        assert_eq!(detect(&p(".env")), Some(Kind::Env));
        assert_eq!(detect(&p(".env.local")), Some(Kind::Env));
        assert_eq!(detect(&p("Dockerfile")), Some(Kind::Code));
        assert_eq!(detect(&p("dockerfile")), Some(Kind::Code));
        assert_eq!(detect(&p("Makefile")), Some(Kind::Code));
        assert_eq!(detect(&p("package.json")), Some(Kind::PackageJson));
        assert_eq!(detect(&p("tsconfig.app.json")), Some(Kind::Json));
        assert_eq!(detect(&p("TSCONFIG.build.json")), Some(Kind::Json));
    }

    #[test]
    fn extensions() {
        assert_eq!(detect(&p("readme.md")), Some(Kind::Markdown));
        assert_eq!(detect(&p("graph.mmd")), Some(Kind::Mermaid));
        assert_eq!(detect(&p("a.jsonl")), Some(Kind::JsonLines));
        assert_eq!(detect(&p("a.tsv")), Some(Kind::Table));
        assert_eq!(detect(&p("x.sqlite3")), Some(Kind::Sqlite));
        assert_eq!(detect(&p("server.log")), Some(Kind::Log));
        assert_eq!(detect(&p("q.sql")), Some(Kind::Sql));
        assert_eq!(detect(&p("n.ipynb")), Some(Kind::Notebook));
        assert_eq!(detect(&p("a.zip")), Some(Kind::Archive));
        assert_eq!(detect(&p("a.tar.gz")), Some(Kind::Archive));
        assert_eq!(detect(&p("a.plist")), Some(Kind::Plist));
        assert_eq!(detect(&p("a.patch")), Some(Kind::Diff));
        assert_eq!(detect(&p("main.rs")), Some(Kind::Code));
        assert_eq!(detect(&p("style.scss")), Some(Kind::Code));
    }

    #[test]
    fn unknown_extensions_undetected() {
        assert_eq!(detect(&p("x.xyz123")), None);
        assert_eq!(detect(&p("x.rs")), Some(Kind::Code));
        assert_eq!(detect(&p(".env")), Some(Kind::Env));
    }
}

