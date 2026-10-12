//! Conversions through LibreOffice and poppler.

use super::util::{os, run_cmd, CmdOut};
use super::Ctx;
use crate::{parse_ranges, Error, ImageFormat, OfficeTarget, PdfToImagesOptions, Result};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

fn profile_arg(dir: &Path) -> OsString {
    #[cfg(windows)]
    let url = format!("file:///{}", dir.display().to_string().replace('\\', "/").replace(' ', "%20"));
    #[cfg(not(windows))]
    let url = format!("file://{}", dir.display());
    os(format!("-env:UserInstallation={url}"))
}

fn find_with_ext(dir: &Path, ext: &str) -> Option<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().map(|e| e.eq_ignore_ascii_case(ext)).unwrap_or(false))
        .collect();
    found.sort();
    found.into_iter().next()
}

/// Runs one soffice conversion; returns the produced file.
fn soffice_convert(
    ctx: &Ctx<'_>,
    profile: &Path,
    input: &Path,
    outdir: &Path,
    target: &str,
    ext: &str,
    infilter: Option<&str>,
) -> Result<PathBuf> {
    std::fs::create_dir_all(outdir)?;
    let mut args = vec![profile_arg(profile), os("--headless"), os("--norestore"), os("--nolockcheck")];
    if let Some(f) = infilter {
        args.push(os(format!("--infilter={f}")));
    }
    args.extend([os("--convert-to"), os(target), os("--outdir"), outdir.as_os_str().to_os_string(), input.as_os_str().to_os_string()]);
    let out = run_cmd("soffice", &args, ctx.cancel)?;
    if let Some(p) = find_with_ext(outdir, ext) {
        return Ok(p);
    }
    let msg = if out.message() == "unknown error" { "LibreOffice produced no output".to_string() } else { out.message() };
    Err(Error::External { program: "soffice", message: msg })
}

pub(super) fn to_pdf(ctx: &mut Ctx<'_>, html: bool) -> Result<()> {
    let n = ctx.req.inputs.len();
    let tmp = tempfile::tempdir()?;
    let profile = tmp.path().join("profile");
    for i in 0..n {
        ctx.check()?;
        ctx.step(i, n, &format!("Converting {}", ctx.name(i)));
        let input = ctx.input_path(i).to_path_buf();
        let outdir = tmp.path().join(format!("out{i}"));
        let first = if html { "pdf:writer_web_pdf_Export" } else { "pdf" };
        let produced = match soffice_convert(ctx, &profile, &input, &outdir, first, "pdf", None) {
            Ok(p) => p,
            Err(Error::External { .. }) if html => soffice_convert(ctx, &profile, &input, &outdir, "pdf", "pdf", None)?,
            Err(e) => return Err(e),
        };
        let stem = ctx.stem_n(i, n);
        ctx.emit_file(&stem, "pdf", &produced)?;
    }
    Ok(())
}

pub(super) fn pdf_to_office(ctx: &mut Ctx<'_>, target: OfficeTarget) -> Result<()> {
    let n = ctx.req.inputs.len();
    let tmp = tempfile::tempdir()?;
    let profile = tmp.path().join("profile");
    let (ext, infilter) = match target {
        OfficeTarget::Docx => ("docx", "writer_pdf_import"),
        OfficeTarget::Odt => ("odt", "writer_pdf_import"),
        OfficeTarget::Xlsx => ("xlsx", "writer_pdf_import"),
        OfficeTarget::Pptx => ("pptx", "impress_pdf_import"),
    };
    for i in 0..n {
        ctx.check()?;
        ctx.step(i, n, &format!("Converting {}", ctx.name(i)));
        let input = ctx.input_path(i).to_path_buf();
        // LibreOffice cannot decrypt on import: unlock first when a password was given
        let mut source = input.clone();
        if let Some(pw) = ctx.password(i).filter(|p| !p.is_empty()) {
            let dec = tmp.path().join(format!("dec{i}.pdf"));
            super::util::run_gs(&[], &input, &dec, Some(pw), ctx.cancel)?;
            source = dec;
        }
        let outdir = tmp.path().join(format!("out{i}"));
        let produced = if target == OfficeTarget::Xlsx {
            // Writer has no spreadsheet export: go through HTML and let Calc import the tables
            let hdir = tmp.path().join(format!("html{i}"));
            let html = soffice_convert(ctx, &profile, &source, &hdir, "html", "html", Some(infilter))?;
            soffice_convert(ctx, &profile, &html, &outdir, "xlsx", "xlsx", Some("calc_HTML_WebQuery"))?
        } else {
            soffice_convert(ctx, &profile, &source, &outdir, ext, ext, Some(infilter))?
        };
        let stem = ctx.stem_n(i, n);
        ctx.emit_file(&stem, ext, &produced)?;
        ctx.notes.push(format!("{}: PDF to {} is lossy, check the layout", ctx.name(i), ext.to_uppercase()));
    }
    Ok(())
}

fn poppler_err(out: CmdOut, program: &'static str, path: &Path, pw: Option<&str>) -> Error {
    let low = out.text_lower();
    if low.contains("password") {
        return if pw.is_none_or(str::is_empty) { Error::Encrypted(path.to_path_buf()) } else { Error::WrongPassword(path.to_path_buf()) };
    }
    Error::External { program, message: out.message() }
}

/// poppler only takes passwords as command-line arguments (visible in `ps`), so an
/// encrypted input is decrypted with Ghostscript (password via argument file) into `dir`
/// and poppler reads that copy. Without a password the input is returned as is.
pub(crate) fn poppler_source(input: &Path, pw: Option<&str>, dir: &Path, name: &str, cancel: &std::sync::atomic::AtomicBool) -> Result<PathBuf> {
    match pw.filter(|p| !p.is_empty()) {
        Some(p) => {
            let dec = dir.join(name);
            super::util::run_gs(&[], input, &dec, Some(p), cancel)?;
            Ok(dec)
        }
        None => Ok(input.to_path_buf()),
    }
}

pub(super) fn pdf_to_images(ctx: &mut Ctx<'_>, o: &PdfToImagesOptions) -> Result<()> {
    let n = ctx.req.inputs.len();
    let tmp = tempfile::tempdir()?;
    let dpi = o.dpi.clamp(72, 600);
    let ext = match o.format {
        ImageFormat::Jpg => "jpg",
        ImageFormat::Png => "png",
    };
    // (path, extension) of extracted files
    let mut extracted: Vec<(PathBuf, String)> = Vec::new();

    for i in 0..n {
        ctx.check()?;
        let input = ctx.input_path(i).to_path_buf();
        let pw = ctx.password(i).map(str::to_owned);
        let pw = pw.as_deref();
        let total = ctx.load(i)?.get_pages().len() as u32;
        let source = poppler_source(&input, pw, tmp.path(), &format!("dec{i}.pdf"), ctx.cancel)?;
        let pages = parse_ranges(&o.pages, total)?;
        if o.render_pages {
            let count = pages.len();
            for (k, p) in pages.iter().enumerate() {
                ctx.check()?;
                ctx.report((i * count + k) as f32 / (n * count).max(1) as f32, &format!("Rendering page {p}"));
                let prefix = tmp.path().join(format!("pg{i}_{p}"));
                let mut args = vec![os(if ext == "jpg" { "-jpeg" } else { "-png" })];
                if ext == "jpg" {
                    args.extend([os("-jpegopt"), os("quality=90")]);
                }
                args.extend([os("-r"), os(dpi.to_string()), os("-f"), os(p.to_string()), os("-l"), os(p.to_string()), os("-singlefile")]);
                args.push(source.as_os_str().to_os_string());
                args.push(prefix.as_os_str().to_os_string());
                let out = run_cmd("pdftoppm", &args, ctx.cancel)?;
                if !out.success {
                    return Err(poppler_err(out, "pdftoppm", &input, pw));
                }
                let file = prefix.with_extension(ext);
                if !file.is_file() {
                    return Err(Error::External { program: "pdftoppm", message: format!("page {p} was not rendered") });
                }
                let stem = if n * count == 1 {
                    ctx.req.output_stem.clone()
                } else if n == 1 {
                    format!("{}_{}", ctx.req.output_stem, p)
                } else {
                    format!("{}_{}_{}", ctx.req.output_stem, i + 1, p)
                };
                ctx.emit_file(&stem, ext, &file)?;
            }
        } else {
            ctx.step(i, n, &format!("Extracting images from {}", ctx.name(i)));
            let dir = tmp.path().join(format!("img{i}"));
            std::fs::create_dir_all(&dir)?;
            let mut args = vec![os("-all")];
            if let (Some(a), Some(b)) = (pages.iter().min(), pages.iter().max()) {
                args.extend([os("-f"), os(a.to_string()), os("-l"), os(b.to_string())]);
            }
            args.push(source.as_os_str().to_os_string());
            args.push(dir.join("img").into_os_string());
            let out = run_cmd("pdfimages", &args, ctx.cancel)?;
            if !out.success {
                return Err(poppler_err(out, "pdfimages", &input, pw));
            }
            let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)?.flatten().map(|e| e.path()).collect();
            files.sort();
            for f in files {
                let e = f.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_else(|| "bin".into());
                extracted.push((f, e));
            }
        }
    }
    if !o.render_pages {
        if extracted.is_empty() {
            return Err(Error::InvalidOptions("No embedded images were found in the selected pages".into()));
        }
        let total = extracted.len();
        for (k, (f, e)) in extracted.iter().enumerate() {
            ctx.check()?;
            ctx.step(k, total, "Saving images");
            let stem = ctx.stem_n(k, total);
            ctx.emit_file(&stem, e, f)?;
        }
    }
    Ok(())
}
