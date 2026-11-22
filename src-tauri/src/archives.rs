//! Archive previews (FR-28): non-extracting listing for zip/tar/tgz, bounded
//! entries and total-size caps against zip bombs; .gz text preview.

use serde::Serialize;

const MAX_ENTRIES: usize = 2_000;
/// Total uncompressed-bytes ceiling while scanning (zip-bomb guard).
const MAX_TOTAL_UNCOMPRESSED: u64 = 2 * 1024 * 1024 * 1024; // 2 GiB

#[derive(Serialize)]
pub struct ArchivePreview {
    pub kind: String,
    pub entries: Vec<ArchiveEntry>,
    pub total_files: usize,
    pub total_bytes: u64,
    pub truncated: bool,
    pub error: Option<String>,
}

#[derive(Serialize)]
pub struct ArchiveEntry {
    pub path: String,
    pub size: u64,
    pub compressed: Option<u64>,
}

pub fn build(path: &std::path::Path, content_sniff: Option<&[u8]>) -> ArchivePreview {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();

    if ext == "zip" {
        return list_zip(path);
    }
    if ext == "tar" || name.ends_with(".tar.gz") || name.ends_with(".tgz") || ext == "tgz" {
        return list_tar(path);
    }
    let _ = content_sniff;
    ArchivePreview {
        kind: "archive".into(),
        entries: Vec::new(),
        total_files: 0,
        total_bytes: 0,
        truncated: false,
        error: Some("unsupported archive type".into()),
    }
}

fn list_zip(path: &std::path::Path) -> ArchivePreview {
    let f = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) => {
            return ArchivePreview {
                kind: "zip".into(),
                entries: Vec::new(),
                total_files: 0,
                total_bytes: 0,
                truncated: false,
                error: Some(e.to_string()),
            };
        }
    };
    let mut zip = match zip::ZipArchive::new(f) {
        Ok(z) => z,
        Err(e) => {
            return ArchivePreview {
                kind: "zip".into(),
                entries: Vec::new(),
                total_files: 0,
                total_bytes: 0,
                truncated: false,
                error: Some(e.to_string()),
            };
        }
    };
    let count = zip.len();
    let mut entries = Vec::new();
    let mut total_bytes = 0u64;
    let mut truncated = false;
    for i in 0..count.min(MAX_ENTRIES) {
        if let Ok(file) = zip.by_index(i) {
            total_bytes = total_bytes.saturating_add(file.size());
            if total_bytes > MAX_TOTAL_UNCOMPRESSED {
                truncated = true;
                break;
            }
            entries.push(ArchiveEntry {
                path: file
                    .enclosed_name()
                    .map(|p| p.to_string_lossy().replace('\\', "/"))
                    .unwrap_or_default(),
                size: file.size(),
                compressed: Some(file.compressed_size()),
            });
        }
    }
    if count > MAX_ENTRIES {
        truncated = true;
    }
    ArchivePreview {
        kind: "zip".into(),
        entries,
        total_files: count,
        total_bytes,
        truncated,
        error: None,
    }
}

fn list_tar(path: &std::path::Path) -> ArchivePreview {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let kind = if ext == "tar" { "tar".to_string() } else { "tar.gz".into() };

    let open_reader = || -> Result<Box<dyn std::io::Read>, String> {
        let f = std::fs::File::open(path).map_err(|e| e.to_string())?;
        if kind == "tar" {
            Ok(Box::new(f))
        } else {
            Ok(Box::new(flate2::read::GzDecoder::new(f)))
        }
    };
    let reader = match open_reader() {
        Ok(r) => r,
        Err(e) => {
            return ArchivePreview {
                kind,
                entries: Vec::new(),
                total_files: 0,
                total_bytes: 0,
                truncated: false,
                error: Some(e),
            };
        }
    };
    let mut archive = tar::Archive::new(reader);
    let mut entries = Vec::new();
    let mut total_files = 0usize;
    let mut total_bytes = 0u64;
    let mut truncated = false;
    let it = archive.entries();
    match it {
        Ok(entries_iter) => {
            for entry in entries_iter {
                let Ok(entry) = entry else { continue };
                total_files += 1;
                if total_files <= MAX_ENTRIES {
                    let is_file = entry.header().entry_type().is_file();
                    let size = entry.header().size().unwrap_or(0);
                    if is_file {
                        total_bytes = total_bytes.saturating_add(size);
                        if total_bytes > MAX_TOTAL_UNCOMPRESSED {
                            truncated = true;
                            break;
                        }
                        entries.push(ArchiveEntry {
                            path: entry
                                .path()
                                .map(|p| p.to_string_lossy().into_owned())
                                .unwrap_or_default(),
                            size,
                            compressed: None,
                        });
                    }
                } else {
                    truncated = true;
                    break;
                }
            }
        }
        Err(e) => {
            return ArchivePreview {
                kind,
                entries: Vec::new(),
                total_files: 0,
                total_bytes: 0,
                truncated: false,
                error: Some(e.to_string()),
            };
        }
    }
    ArchivePreview { kind, entries, total_files, total_bytes, truncated, error: None }
}

/// First 1 MB of a .gz file, decompressed.
pub fn gz_text(path: &std::path::Path, limit: usize) -> Result<String, String> {
    use std::io::Read;
    let f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut decoder = flate2::read::GzDecoder::new(f);
    let mut buf = Vec::with_capacity(limit.min(1 << 20));
    decoder
        .by_ref()
        .take(limit as u64)
        .read_to_end(&mut buf)
        .map_err(|e| e.to_string())?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "winview-arc-{}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            name
        ))
    }

    #[test]
    fn lists_zip_without_extraction() {
        let p = temp("t.zip");
        let f = std::fs::File::create(&p).unwrap();
        let mut w = zip::ZipWriter::new(f);
        w.start_file("a/b.txt", zip::write::SimpleFileOptions::default()).unwrap();
        w.write_all(b"hello inside").unwrap();
        w.start_file("c.txt", zip::write::SimpleFileOptions::default()).unwrap();
        w.write_all(b"second").unwrap();
        w.finish().unwrap();
        let ap = build(&p, None);
        println!("ENTRIES: {:?}", ap.entries.iter().map(|e| e.path.clone()).collect::<Vec<_>>());
        assert!(ap.error.is_none(), "{:?}", ap.error);
        assert_eq!(ap.kind, "zip");
        assert_eq!(ap.total_files, 2);
        assert_eq!(ap.total_bytes, 18);
        assert!(ap.entries.iter().any(|e| e.path == "a/b.txt"));
        assert!(!p.with_extension("extracted").exists());
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn lists_tar_gz() {
        let p = temp("t.tar.gz");
        let f = std::fs::File::create(&p).unwrap();
        let enc = flate2::write::GzEncoder::new(f, flate2::Compression::default());
        let mut b = tar::Builder::new(enc);
        let mut h = tar::Header::new_gnu();
        h.set_size(5);
        h.set_entry_type(tar::EntryType::Regular);
        h.set_cksum();
        b.append_data(&mut h, "x/y.txt", b"12345".as_ref()).unwrap();
        b.into_inner().unwrap();
        let ap = build(&p, None);
        assert!(ap.error.is_none(), "{:?}", ap.error);
        assert_eq!(ap.kind, "tar.gz");
        assert_eq!(ap.total_files, 1);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn gz_text_reads() {
        let p = temp("t.txt.gz");
        let f = std::fs::File::create(&p).unwrap();
        let mut w = flate2::write::GzEncoder::new(f, flate2::Compression::default());
        w.write_all(b"gz inner text\n").unwrap();
        w.finish().unwrap();
        let t = gz_text(&p, 1024).unwrap();
        assert!(t.starts_with("gz inner text"));
        let _ = std::fs::remove_file(&p);
    }
}
