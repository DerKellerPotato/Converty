use crate::ops::util::{os, run_cmd};
use crate::{Error, FileKind, Result};
use std::path::Path;
use std::sync::atomic::AtomicBool;

/// Renders one page (1-based) of a PDF so that its longer side is `max_px` pixels.
/// Returns (width, height, RGBA8 pixels). Used for thumbnails and previews.
/// Uses `pdftoppm` (poppler-utils). Images (FileKind::Image) are also accepted
/// (page must be 1) so the UI can thumbnail them with the same call.
pub fn render_page(path: &Path, page: u32, max_px: u32, password: Option<&str>) -> Result<(u32, u32, Vec<u8>)> {
    let max_px = max_px.max(1);
    match FileKind::from_path(path) {
        FileKind::Image => {
            if page > 1 {
                return Err(Error::InvalidRange(format!("Page {page} does not exist")));
            }
            let bytes = std::fs::read(path)?;
            let img_err = |e: image::ImageError| Error::Image(format!("{}: {e}", path.display()));
            let mut decoder = image::ImageReader::new(std::io::Cursor::new(&bytes))
                .with_guessed_format()?
                .into_decoder()
                .map_err(img_err)?;
            let orientation = image::ImageDecoder::orientation(&mut decoder).unwrap_or(image::metadata::Orientation::NoTransforms);
            let mut img = image::DynamicImage::from_decoder(decoder).map_err(img_err)?;
            img.apply_orientation(orientation);
            if img.width().max(img.height()) > max_px {
                img = img.resize(max_px, max_px, image::imageops::FilterType::Triangle);
            }
            let rgba = img.to_rgba8();
            Ok((rgba.width(), rgba.height(), rgba.into_raw()))
        }
        FileKind::Pdf => {
            if page == 0 {
                return Err(Error::InvalidRange("Pages start at 1".into()));
            }
            let tmp = tempfile::tempdir()?;
            let prefix = tmp.path().join("page");
            let mut args = vec![
                os("-png"),
                os("-f"),
                os(page.to_string()),
                os("-l"),
                os(page.to_string()),
                os("-scale-to"),
                os(max_px.to_string()),
                os("-singlefile"),
            ];
            let never = AtomicBool::new(false);
            let source = crate::ops::poppler_source(path, password, tmp.path(), "dec.pdf", &never)?;
            args.push(source.as_os_str().to_os_string());
            args.push(prefix.as_os_str().to_os_string());
            let out = run_cmd("pdftoppm", &args, &never)?;
            if !out.success {
                let low = out.text_lower();
                if low.contains("password") {
                    return Err(if password.is_none_or(str::is_empty) {
                        Error::Encrypted(path.to_path_buf())
                    } else {
                        Error::WrongPassword(path.to_path_buf())
                    });
                }
                if low.contains("wrong page range") || low.contains("page range") {
                    return Err(Error::InvalidRange(format!("Page {page} does not exist")));
                }
                return Err(Error::External { program: "pdftoppm", message: out.message() });
            }
            let img = image::open(prefix.with_extension("png")).map_err(|e| Error::Image(e.to_string()))?;
            let rgba = img.to_rgba8();
            Ok((rgba.width(), rgba.height(), rgba.into_raw()))
        }
        _ => Err(Error::WrongType(path.to_path_buf())),
    }
}
