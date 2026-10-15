use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FileKind {
    Pdf,
    /// jpg, jpeg, png, webp, tiff, bmp, gif
    Image,
    /// doc, docx, odt, rtf, xls, xlsx, ods, csv, ppt, pptx, odp
    Office,
    /// html, htm
    Html,
    /// mp4, m4v, mkv, webm, mov, avi, wmv, flv, mpg, mpeg, ts, m2ts, mts, 3gp, ogv
    Video,
    Other,
}

impl FileKind {
    /// By extension (case-insensitive).
    pub fn from_path(path: &Path) -> FileKind {
        let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        match ext.as_str() {
            "pdf" => FileKind::Pdf,
            "jpg" | "jpeg" | "png" | "webp" | "tif" | "tiff" | "bmp" | "gif" => FileKind::Image,
            "doc" | "docx" | "odt" | "rtf" | "xls" | "xlsx" | "ods" | "csv" | "ppt" | "pptx" | "odp" => FileKind::Office,
            "html" | "htm" => FileKind::Html,
            "mp4" | "m4v" | "mkv" | "webm" | "mov" | "avi" | "wmv" | "flv" | "mpg" | "mpeg" | "ts" | "m2ts" | "mts" | "3gp" | "ogv" => {
                FileKind::Video
            }
            _ => FileKind::Other,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileInfo {
    pub kind: FileKind,
    pub size_bytes: u64,
    /// Page count for PDFs (None if encrypted without password, or not a PDF).
    pub pages: Option<u32>,
    pub encrypted: bool,
    /// Size of page 1 in points (width, height), after rotation.
    pub first_page_size: Option<(f32, f32)>,
}

/// Cheap metadata read. Fast enough to call on every dropped file.
/// For an encrypted PDF without `password`: Ok with `encrypted = true, pages = None`.
/// With a wrong password: `Err(Error::WrongPassword)`. Unreadable PDF: `Err(Error::Corrupt)`.
pub fn inspect(path: &Path, password: Option<&str>) -> Result<FileInfo> {
    let meta = std::fs::metadata(path)?;
    let kind = FileKind::from_path(path);
    let mut info = FileInfo { kind, size_bytes: meta.len(), pages: None, encrypted: false, first_page_size: None };
    if kind != FileKind::Pdf {
        return Ok(info);
    }
    let never = std::sync::atomic::AtomicBool::new(false);
    // Detect encryption from the trailer first: it also tells us when no password is needed.
    let raw = lopdf::Document::load(path).map_err(|_| Error::Corrupt(path.to_path_buf()))?;
    info.encrypted = raw.trailer.has(b"Encrypt");
    let doc = if info.encrypted && raw.encryption_state.is_none() {
        match password {
            Some(p) if !p.is_empty() => crate::ops::util::load_pdf(path, Some(p), &never)?,
            _ => return Ok(info),
        }
    } else {
        raw
    };
    let ids = crate::ops::pdfdoc::pages_vec(&doc);
    info.pages = Some(ids.len() as u32);
    if let Some(&first) = ids.first() {
        let (w, h) = crate::ops::pdfdoc::visual_size(&doc, first);
        info.first_page_size = Some((w as f32, h as f32));
    }
    if ids.is_empty() {
        return Err(Error::Corrupt(path.to_path_buf()));
    }
    Ok(info)
}
