//! `winview` custom protocol (spec §3.3).
//!
//! On Windows the origin is `http://winview.localhost/…`. Serves:
//! - `/img/<req_id>/<rel>`   — images referenced by the current Markdown file
//!   (resolved against that file's folder; path containment enforced)
//! - `/media/<req_id>/<name>` — the current preview file itself as bytes with
//!   HTTP range support (image / PDF / audio / video)
//!
//! Tokens are per-preview request IDs; stale tokens serve nothing.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use tauri::Builder;

type TokenMap = HashMap<u64, (Option<PathBuf>, Option<PathBuf>)>;

static TOKENS: Mutex<Option<TokenMap>> = Mutex::new(None);

fn with_tokens<R>(f: impl FnOnce(&mut TokenMap) -> R) -> Option<R> {
    let mut guard = TOKENS.lock().ok()?;
    let map = guard.get_or_insert_with(HashMap::new);
    Some(f(map))
}

/// Register the image base directory (markdown folder) for this request.
pub fn set_image_base(req_id: u64, dir: PathBuf) {
    with_tokens(|t| {
        let e = t.entry(req_id).or_default();
        e.0 = Some(dir);
    });
}

/// Register the media file for this request.
pub fn set_media_file(req_id: u64, file: PathBuf) {
    with_tokens(|t| {
        let e = t.entry(req_id).or_default();
        e.1 = Some(file);
    });
}

fn take(req_id: u64) -> (Option<PathBuf>, Option<PathBuf>) {
    with_tokens(|t| t.get(&req_id).cloned().unwrap_or((None, None))).unwrap_or((None, None))
}

/// Drop the token state for a finished preview (keeps the map bounded).
pub fn clear_token(req_id: u64) {
    with_tokens(|t| {
        t.remove(&req_id);
    });
}

pub fn register(builder: Builder<tauri::Wry>) -> Builder<tauri::Wry> {
    builder.register_asynchronous_uri_scheme_protocol("winview", move |_ctx, request, responder| {
        let uri = request.uri().to_string();
        let range_header = request
            .headers()
            .get("range")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        let response = handle(&uri, range_header.as_deref());
        responder.respond(response);
    })
}

fn handle(uri: &str, range: Option<&str>) -> tauri::http::Response<Vec<u8>> {
    // http://winview.localhost/img/<req>/<rel> or /media/<req>/<name>
    let path_part = uri
        .split("winview.localhost/")
        .nth(1)
        .unwrap_or_default();
    let (kind, tail) = match path_part.split_once('/') {
        Some((k, t)) => (k, t),
        None => return not_found(),
    };
    let (req_str, _name) = match tail.split_once('/') {
        Some((r, n)) => (r, n),
        None => return not_found(),
    };
    let Ok(req_id) = req_str.parse::<u64>() else { return not_found() };
    let (image_base, media_file) = take(req_id);

    match kind {
        "img" => serve_image(req_id, image_base.as_deref(), uri, tail),
        "media" => serve_media(media_file.as_deref(), range),
        _ => not_found(),
    }
}

fn serve_image(
    _req_id: u64,
    base: Option<&Path>,
    _uri: &str,
    tail: &str,
) -> tauri::http::Response<Vec<u8>> {
    let Some(base) = base else { return not_found() };
    // Reject traversal before joining.
    let rel = percent_decode(tail.split('?').next().unwrap_or(tail));
    if rel.contains("..") || rel.starts_with('/') || rel.contains('\\') && rel.contains("..\\") {
        return forbidden();
    }
    let resolved = base.join(&rel);
    // Canonicalized containment check (spec §5).
    let Ok(canon) = resolved.canonicalize() else { return not_found() };
    let Ok(base_canon) = base.canonicalize() else { return not_found() };
    if !canon.starts_with(&base_canon) {
        return forbidden();
    }
    match std::fs::read(&canon) {
        Ok(bytes) => {
            let mime = mime_for_path(&canon);
            ok(bytes, &mime)
        }
        Err(_) => not_found(),
    }
}

fn serve_media(file: Option<&Path>, range: Option<&str>) -> tauri::http::Response<Vec<u8>> {
    let Some(file) = file else { return not_found() };
    let Ok(meta) = std::fs::metadata(file) else { return not_found() };
    let total = meta.len() as usize;
    let mime = mime_for_path(file);
    let bytes = match std::fs::read(file) {
        Ok(b) => b,
        Err(_) => return not_found(),
    };
    if let Some(range) = range {
        // bytes=<start>-<end>
        if let Some(spec) = range.strip_prefix("bytes=") {
            let (start_s, end_s) = spec.split_once('-').unwrap_or((spec, ""));
            let start: usize = start_s.parse().unwrap_or(0);
            let end: usize = if end_s.is_empty() {
                total.saturating_sub(1)
            } else {
                end_s.parse().unwrap_or(total.saturating_sub(1))
            };
            let end = end.min(total.saturating_sub(1));
            if start <= end {
                return tauri::http::Response::builder()
                    .status(206)
                    .header("Content-Type", mime)
                    .header("Content-Range", &format!("bytes {start}-{end}/{total}"))
                    .header("Accept-Ranges", "bytes")
                    .body(bytes[start..=end].to_vec())
                    .unwrap();
            }
        }
    }
    ok(bytes, &mime)
}

fn mime_for_path(p: &Path) -> String {
    let ext = p
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let m = match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "pdf" => "application/pdf",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "mid" | "midi" => "audio/midi",
        "flac" => "audio/flac",
        "ogg" => "audio/ogg",
        "mp4" => "video/mp4",
        "mkv" => "video/x-matroska",
        "avi" => "video/x-msvideo",
        "webm" => "video/webm",
        _ => "application/octet-stream",
    };
    m.to_string()
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = &s[i + 1..i + 3];
            if let Ok(b) = u8::from_str_radix(hex, 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn ok(bytes: Vec<u8>, mime: &str) -> tauri::http::Response<Vec<u8>> {
    tauri::http::Response::builder()
        .header("Content-Type", mime)
        .header("Access-Control-Allow-Origin", "*")
        .body(bytes)
        .unwrap()
}

fn not_found() -> tauri::http::Response<Vec<u8>> {
    tauri::http::Response::builder().status(404).body(Vec::new()).unwrap()
}

fn forbidden() -> tauri::http::Response<Vec<u8>> {
    tauri::http::Response::builder().status(403).body(Vec::new()).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_decode_roundtrip() {
        assert_eq!(percent_decode("a%20b"), "a b");
        assert_eq!(percent_decode("plain"), "plain");
        assert_eq!(percent_decode("100%"), "100%");
    }

    #[test]
    fn traversal_rejected() {
        set_image_base(1, std::env::temp_dir());
        let r = handle("http://winview.localhost/img/1/../../etc", None);
        assert_eq!(r.status(), 403);
        clear_token(1);
    }

    #[test]
    fn stale_token_serves_nothing() {
        let r = handle("http://winview.localhost/media/9999/x.png", None);
        assert_eq!(r.status(), 404);
    }
}
