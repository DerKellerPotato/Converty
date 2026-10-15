//! Thin, panic-safe glue between the UI and `converty-core`.
//!
//! Every engine call that may block runs on `spawn_blocking`; a panic inside the
//! engine turns into a [`UiError`] instead of taking the window down.

use converty_core::{Error, FileKind};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrKind {
    Encrypted(std::path::PathBuf),
    WrongPassword(std::path::PathBuf),
    Corrupt(std::path::PathBuf),
    WrongType,
    MissingDependency { program: String, package: String },
    Cancelled,
    Internal,
    Other,
}

/// Cloneable error for messages and views.
#[derive(Debug, Clone)]
pub struct UiError {
    pub kind: ErrKind,
    pub message: String,
}

impl UiError {
    pub fn internal(message: impl Into<String>) -> Self {
        Self { kind: ErrKind::Internal, message: message.into() }
    }
}

impl From<Error> for UiError {
    fn from(e: Error) -> Self {
        let message = e.to_string();
        let kind = match e {
            Error::Encrypted(p) => ErrKind::Encrypted(p),
            Error::WrongPassword(p) => ErrKind::WrongPassword(p),
            Error::Corrupt(p) => ErrKind::Corrupt(p),
            Error::WrongType(_) => ErrKind::WrongType,
            Error::MissingDependency { program, package } => {
                ErrKind::MissingDependency { program: program.into(), package: package.into() }
            }
            Error::Cancelled => ErrKind::Cancelled,
            _ => ErrKind::Other,
        };
        Self { kind, message }
    }
}

fn panic_text(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".into()
    }
}

/// Runs an engine call on the blocking pool. Panics become [`ErrKind::Internal`].
pub async fn blocking<T, F>(f: F) -> Result<T, UiError>
where
    T: Send + 'static,
    F: FnOnce() -> converty_core::Result<T> + Send + 'static,
{
    match tokio::task::spawn_blocking(f).await {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(e)) => Err(e.into()),
        Err(join) if join.is_panic() => Err(UiError::internal(format!(
            "The engine stopped unexpectedly: {}",
            panic_text(&*join.into_panic())
        ))),
        Err(join) => Err(UiError::internal(join.to_string())),
    }
}

/// Same as [`blocking`] for calls that cannot fail.
pub async fn blocking_infallible<T, F>(f: F) -> Option<T>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    tokio::task::spawn_blocking(f).await.ok()
}

/// Validates a page selection. Uses the engine's parser, falls back to a local
/// one while the engine is still unimplemented (so the UI stays usable).
pub fn parse_ranges(text: &str, pages: u32) -> Result<Vec<u32>, String> {
    let owned = text.to_string();
    match std::panic::catch_unwind(move || converty_core::parse_ranges(&owned, pages)) {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(Error::InvalidRange(m))) => Err(m),
        Ok(Err(e)) => Err(e.to_string()),
        Err(_) => fallback_parse_ranges(text, pages),
    }
}

fn fallback_parse_ranges(text: &str, pages: u32) -> Result<Vec<u32>, String> {
    if text.trim().is_empty() {
        return Ok((1..=pages).collect());
    }
    let mut out = Vec::new();
    for part in text.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let num = |s: &str| -> Result<u32, String> {
            s.trim().parse::<u32>().map_err(|_| format!("“{part}” is not a page number"))
        };
        let (a, b) = match part.split_once('-') {
            Some((a, b)) => {
                let a = if a.trim().is_empty() { 1 } else { num(a)? };
                let b = if b.trim().is_empty() { pages } else { num(b)? };
                (a, b)
            }
            None => {
                let n = num(part)?;
                (n, n)
            }
        };
        if a == 0 || b == 0 || a > pages || b > pages {
            return Err(format!("“{part}” is outside 1–{pages}"));
        }
        if a > b {
            return Err(format!("“{part}” goes backwards"));
        }
        for p in a..=b {
            if !out.contains(&p) {
                out.push(p);
            }
        }
    }
    Ok(out)
}

pub fn kind_globs(kind: FileKind) -> (&'static str, &'static [&'static str]) {
    match kind {
        FileKind::Pdf => ("PDF documents", &["*.pdf", "*.PDF"]),
        FileKind::Image => (
            "Images",
            &["*.jpg", "*.jpeg", "*.png", "*.webp", "*.tif", "*.tiff", "*.bmp", "*.gif", "*.JPG", "*.JPEG", "*.PNG"],
        ),
        FileKind::Office => (
            "Office documents",
            &["*.doc", "*.docx", "*.odt", "*.rtf", "*.xls", "*.xlsx", "*.ods", "*.csv", "*.ppt", "*.pptx", "*.odp"],
        ),
        FileKind::Html => ("Web pages", &["*.html", "*.htm"]),
        FileKind::Video => (
            "Videos",
            &[
                "*.mp4", "*.m4v", "*.mkv", "*.webm", "*.mov", "*.avi", "*.wmv", "*.flv", "*.mpg", "*.mpeg", "*.ts", "*.m2ts",
                "*.mts", "*.3gp", "*.ogv", "*.MP4", "*.MKV", "*.MOV", "*.AVI",
            ],
        ),
        FileKind::Other => ("All files", &["*"]),
    }
}

pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{bytes} B")
    } else {
        format!("{v:.1} {}", UNITS[i])
    }
}

/// Sorted page numbers → compact text, e.g. [1,3,4,5,8] → "1,3-5,8".
pub fn ranges_text(pages: impl IntoIterator<Item = u32>) -> String {
    let mut v: Vec<u32> = pages.into_iter().collect();
    v.sort_unstable();
    v.dedup();
    let mut parts = Vec::new();
    let mut i = 0;
    while i < v.len() {
        let start = v[i];
        let mut end = start;
        while i + 1 < v.len() && v[i + 1] == end + 1 {
            i += 1;
            end = v[i];
        }
        parts.push(if start == end { start.to_string() } else { format!("{start}-{end}") });
        i += 1;
    }
    parts.join(",")
}

/// Shows a file in the file manager (FileManager1 over D-Bus), else opens its folder.
#[cfg(target_os = "linux")]
pub async fn show_in_folder(path: std::path::PathBuf) {
    let uri = url::Url::from_file_path(&path).map(|u| u.to_string()).unwrap_or_default();
    let shown = async {
        let conn = zbus::Connection::session().await?;
        conn.call_method(
            Some("org.freedesktop.FileManager1"),
            "/org/freedesktop/FileManager1",
            Some("org.freedesktop.FileManager1"),
            "ShowItems",
            &(vec![uri.as_str()], ""),
        )
        .await?;
        Ok::<(), zbus::Error>(())
    }
    .await;
    if shown.is_err() {
        open_parent(path);
    }
}

/// Shows a file in Explorer (selected) or Finder (revealed), else opens its folder.
#[cfg(not(target_os = "linux"))]
pub async fn show_in_folder(path: std::path::PathBuf) {
    #[cfg(windows)]
    let shown = {
        use std::os::windows::process::CommandExt;
        std::process::Command::new("explorer").raw_arg(format!("/select,\"{}\"", path.display())).spawn().is_ok()
    };
    #[cfg(target_os = "macos")]
    let shown = std::process::Command::new("open").arg("-R").arg(&path).spawn().is_ok();
    #[cfg(not(any(windows, target_os = "macos")))]
    let shown = false;
    if !shown {
        open_parent(path);
    }
}

fn open_parent(path: std::path::PathBuf) {
    let dir = if path.is_dir() { path.clone() } else { path.parent().map(Path::to_path_buf).unwrap_or(path) };
    let _ = open::that_detached(dir);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_ranges() {
        assert_eq!(ranges_text([5, 1, 3, 4, 8]), "1,3-5,8");
        assert_eq!(ranges_text([]), "");
    }

    #[test]
    fn fallback_parser() {
        assert_eq!(fallback_parse_ranges("1,3-4,6-", 7).unwrap(), vec![1, 3, 4, 6, 7]);
        assert!(fallback_parse_ranges("9", 3).is_err());
        assert!(fallback_parse_ranges("x", 3).is_err());
    }
}
