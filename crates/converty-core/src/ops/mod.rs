//! Job execution. `run` validates the request, dispatches on the tool and cleans up
//! partial outputs on failure.

mod compress;
mod convert;
mod images;
mod pages;
pub(crate) mod pdfdoc;
mod security;
mod stamp;
pub(crate) mod util;

pub(crate) fn poppler_source(
    input: &Path,
    pw: Option<&str>,
    dir: &Path,
    name: &str,
    cancel: &AtomicBool,
) -> crate::Result<PathBuf> {
    convert::poppler_source(input, pw, dir, name, cancel)
}

use crate::{Error, FileKind, JobRequest, JobResult, Progress, Result, Tool, ToolOptions};
use lopdf::Document;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

pub(crate) struct Ctx<'a> {
    pub req: &'a JobRequest,
    progress: Progress<'a>,
    pub cancel: &'a AtomicBool,
    pub notes: Vec<String>,
    pub outputs: Vec<PathBuf>,
}

impl Ctx<'_> {
    pub fn check(&self) -> Result<()> {
        util::check(self.cancel)
    }

    pub fn report(&self, fraction: f32, msg: &str) {
        (self.progress)(fraction.clamp(0.0, 1.0), msg);
    }

    /// Progress for step `i` of `n` (0-based, before doing it).
    pub fn step(&self, i: usize, n: usize, msg: &str) {
        self.report(i as f32 / n.max(1) as f32, msg);
    }

    pub fn input_path(&self, i: usize) -> &Path {
        &self.req.inputs[i].path
    }

    pub fn password(&self, i: usize) -> Option<&str> {
        self.req.inputs[i].password.as_deref()
    }

    pub fn load(&self, i: usize) -> Result<Document> {
        util::load_pdf(self.input_path(i), self.password(i), self.cancel)
    }

    pub fn name(&self, i: usize) -> String {
        util::file_stem(self.input_path(i))
    }

    /// Output stem for output `i` of `n`.
    pub fn stem_n(&self, i: usize, n: usize) -> String {
        if n <= 1 {
            self.req.output_stem.clone()
        } else {
            format!("{}_{}", self.req.output_stem, i + 1)
        }
    }

    /// Writes one output file with a unique name.
    pub fn emit(&mut self, stem: &str, ext: &str, write: impl FnOnce(&Path) -> Result<()>) -> Result<PathBuf> {
        self.check()?;
        let p = util::write_unique(&self.req.output_dir, stem, ext, write)?;
        self.outputs.push(p.clone());
        Ok(p)
    }

    /// Moves an existing file (e.g. from a temp dir) into the output dir.
    pub fn emit_file(&mut self, stem: &str, ext: &str, src: &Path) -> Result<PathBuf> {
        self.emit(stem, ext, |tmp| {
            std::fs::copy(src, tmp)?;
            Ok(())
        })
    }

    pub fn emit_pdf(&mut self, stem: &str, doc: &mut Document) -> Result<PathBuf> {
        self.emit(stem, "pdf", |p| save_pdf(doc, p))
    }
}

pub(crate) fn save_pdf(doc: &mut Document, path: &Path) -> Result<()> {
    doc.compress();
    doc.save(path).map_err(|e| Error::Pdf(e.to_string()))?;
    Ok(())
}

pub(crate) fn run(req: &JobRequest, progress: Progress<'_>, cancel: &AtomicBool) -> Result<JobResult> {
    let meta = req.tool.meta();
    if req.inputs.is_empty() || req.inputs.len() < meta.min_files {
        return Err(Error::InvalidOptions(format!("{} needs at least {} file(s)", meta.name, meta.min_files.max(1))));
    }
    if !meta.multi && req.inputs.len() > 1 {
        return Err(Error::InvalidOptions(format!("{} works on one file at a time", meta.name)));
    }
    for inp in &req.inputs {
        if !meta.accepts.contains(&FileKind::from_path(&inp.path)) {
            return Err(Error::WrongType(inp.path.clone()));
        }
        if !inp.path.is_file() {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("{} not found", inp.path.display()),
            )));
        }
    }
    for prog in meta.needs {
        util::require(prog)?;
    }
    if !req.output_dir.is_dir() {
        return Err(Error::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "output folder does not exist")));
    }
    util::check(cancel)?;

    let mut ctx = Ctx { req, progress, cancel, notes: Vec::new(), outputs: Vec::new() };
    ctx.report(0.0, "Starting");
    let result = dispatch(&mut ctx);
    match result {
        Ok(()) => {
            ctx.report(1.0, "Done");
            let input_bytes = req.inputs.iter().map(|i| util::file_size(&i.path)).sum();
            let output_bytes = ctx.outputs.iter().map(|p| util::file_size(p)).sum();
            Ok(JobResult { outputs: ctx.outputs, input_bytes, output_bytes, notes: ctx.notes })
        }
        Err(e) => {
            for p in &ctx.outputs {
                let _ = std::fs::remove_file(p);
            }
            Err(e)
        }
    }
}

fn mismatch(tool: Tool) -> Error {
    Error::InvalidOptions(format!("options do not match {:?}", tool))
}

fn dispatch(ctx: &mut Ctx<'_>) -> Result<()> {
    let tool = ctx.req.tool;
    let req = ctx.req;
    match (&req.options, tool) {
        (ToolOptions::Merge, Tool::Merge) => pages::merge(ctx),
        (ToolOptions::Split(o), Tool::Split) => pages::split(ctx, o),
        (ToolOptions::RemovePages { pages }, Tool::RemovePages) => pages::remove(ctx, pages),
        (ToolOptions::ExtractPages { pages, separate_files }, Tool::ExtractPages) => pages::extract(ctx, pages, *separate_files),
        (ToolOptions::Organize { pages }, Tool::Organize) => pages::organize(ctx, pages),
        (ToolOptions::Rotate { degrees, pages }, Tool::Rotate) => pages::rotate(ctx, *degrees, pages),
        (ToolOptions::Crop(o), Tool::Crop) => pages::crop(ctx, o),
        (ToolOptions::Compress { level }, Tool::Compress) => compress::compress(ctx, *level),
        (ToolOptions::Repair, Tool::Repair) => compress::repair(ctx),
        (ToolOptions::PdfToPdfA { level }, Tool::PdfToPdfA) => compress::pdfa(ctx, *level),
        (ToolOptions::ImagesToPdf(o), Tool::ImagesToPdf) => images::images_to_pdf(ctx, o),
        (ToolOptions::OfficeToPdf, Tool::OfficeToPdf) => convert::to_pdf(ctx, false),
        (ToolOptions::HtmlToPdf, Tool::HtmlToPdf) => convert::to_pdf(ctx, true),
        (ToolOptions::PdfToImages(o), Tool::PdfToImages) => convert::pdf_to_images(ctx, o),
        (ToolOptions::PdfToOffice { target }, Tool::PdfToOffice) => convert::pdf_to_office(ctx, *target),
        (ToolOptions::PageNumbers(o), Tool::PageNumbers) => stamp::page_numbers(ctx, o),
        (ToolOptions::Watermark(o), Tool::Watermark) => stamp::watermark(ctx, o),
        (ToolOptions::Sign(o), Tool::Sign) => stamp::sign(ctx, o),
        (ToolOptions::Unlock, Tool::Unlock) => security::unlock(ctx),
        (ToolOptions::Protect(o), Tool::Protect) => security::protect(ctx, o),
        _ => Err(mismatch(tool)),
    }
}
