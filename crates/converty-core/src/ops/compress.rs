//! Ghostscript-based tools: compress, repair, PDF/A.

use super::util::{file_size, os, run_gs, run_gs_with};
use super::{save_pdf, Ctx};
use crate::{CompressLevel, Error, PdfALevel, Result};
use std::path::{Path, PathBuf};

pub(super) fn compress(ctx: &mut Ctx<'_>, level: CompressLevel) -> Result<()> {
    let preset = match level {
        CompressLevel::Extreme => "/screen",
        CompressLevel::Recommended => "/ebook",
        CompressLevel::Low => "/printer",
    };
    let n = ctx.req.inputs.len();
    let tmp = tempfile::tempdir()?;
    for i in 0..n {
        ctx.check()?;
        ctx.step(i, n, &format!("Compressing {}", ctx.name(i)));
        let input = ctx.input_path(i).to_path_buf();
        let out = tmp.path().join(format!("out{i}.pdf"));
        let extra = vec![os("-dCompatibilityLevel=1.5"), os(format!("-dPDFSETTINGS={preset}"))];
        run_gs(&extra, &input, &out, ctx.password(i), ctx.cancel)?;
        let (orig, new) = (file_size(&input), file_size(&out));
        let stem = ctx.stem_n(i, n);
        if new >= orig {
            ctx.notes.push(format!(
                "{}: the compressed file was not smaller ({} vs {} bytes), the original was kept",
                ctx.name(i),
                new,
                orig
            ));
            ctx.emit_file(&stem, "pdf", &input)?;
        } else {
            ctx.emit_file(&stem, "pdf", &out)?;
        }
    }
    Ok(())
}

pub(super) fn repair(ctx: &mut Ctx<'_>) -> Result<()> {
    let n = ctx.req.inputs.len();
    let tmp = tempfile::tempdir()?;
    for i in 0..n {
        ctx.check()?;
        ctx.step(i, n, &format!("Repairing {}", ctx.name(i)));
        let stem = ctx.stem_n(i, n);
        let input = ctx.input_path(i).to_path_buf();
        match ctx.load(i) {
            Ok(mut doc) if !doc.get_pages().is_empty() => {
                let out = tmp.path().join(format!("r{i}.pdf"));
                if save_pdf(&mut doc, &out).is_ok() && lopdf::Document::load(&out).map(|d| !d.get_pages().is_empty()).unwrap_or(false) {
                    ctx.emit_file(&stem, "pdf", &out)?;
                    continue;
                }
            }
            Err(e @ (Error::Encrypted(_) | Error::WrongPassword(_) | Error::Cancelled)) => return Err(e),
            _ => {}
        }
        // fallback: let Ghostscript rewrite the file
        let out = tmp.path().join(format!("g{i}.pdf"));
        run_gs(&[], &input, &out, ctx.password(i), ctx.cancel).map_err(|e| match e {
            Error::External { .. } => Error::Corrupt(input.clone()),
            e => e,
        })?;
        if file_size(&out) == 0 {
            return Err(Error::Corrupt(input));
        }
        ctx.notes.push(format!("{}: rebuilt with Ghostscript", ctx.name(i)));
        ctx.emit_file(&stem, "pdf", &out)?;
    }
    Ok(())
}

fn find_icc() -> Option<PathBuf> {
    for root in ["/usr/share/ghostscript", "/app/share/ghostscript", "/usr/local/share/ghostscript"] {
        let Ok(rd) = std::fs::read_dir(root) else { continue };
        for e in rd.flatten() {
            let p = e.path().join("iccprofiles").join("default_rgb.icc");
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

fn write_pdfa_def(dir: &Path, icc: &Path) -> Result<PathBuf> {
    let icc = icc.to_string_lossy().replace('\\', "\\\\").replace('(', "\\(").replace(')', "\\)");
    let ps = format!(
        "%!\n/ICCProfile ({icc}) def\n\
[/_objdef {{icc_PDFA}} /type /stream /OBJ pdfmark\n\
[{{icc_PDFA}} <</N 3>> /PUT pdfmark\n\
[{{icc_PDFA}} ICCProfile (r) file /PUT pdfmark\n\
[/_objdef {{OutputIntent_PDFA}} /type /dict /OBJ pdfmark\n\
[{{OutputIntent_PDFA}} <<\n/Type /OutputIntent\n/S /GTS_PDFA1\n/DestOutputProfile {{icc_PDFA}}\n/OutputConditionIdentifier (sRGB)\n/Info (sRGB)\n>> /PUT pdfmark\n\
[{{Catalog}} <</OutputIntents [ {{OutputIntent_PDFA}} ]>> /PUT pdfmark\n"
    );
    let p = dir.join("PDFA_def.ps");
    std::fs::write(&p, ps)?;
    Ok(p)
}

pub(super) fn pdfa(ctx: &mut Ctx<'_>, level: PdfALevel) -> Result<()> {
    let n = match level {
        PdfALevel::B1 => 1,
        PdfALevel::B2 => 2,
        PdfALevel::B3 => 3,
    };
    let count = ctx.req.inputs.len();
    let tmp = tempfile::tempdir()?;
    let icc = find_icc();
    for i in 0..count {
        ctx.check()?;
        ctx.step(i, count, &format!("Converting {}", ctx.name(i)));
        let input = ctx.input_path(i).to_path_buf();
        let out = tmp.path().join(format!("a{i}.pdf"));
        let base = vec![
            os(format!("-dPDFA={n}")),
            os("-dPDFACompatibilityPolicy=1"),
            os("-sColorConversionStrategy=RGB"),
            os("-dUseCIEColor"),
        ];
        let mut done = false;
        if let Some(icc) = &icc {
            if let Ok(def) = write_pdfa_def(tmp.path(), icc) {
                let mut extra = vec![
                    os(format!("--permit-file-read={}", icc.display())),
                    os(format!("--permit-file-read={}", def.display())),
                ];
                extra.extend(base.iter().cloned());
                match run_gs_with(&extra, &[def], &input, &out, ctx.password(i), ctx.cancel) {
                    Ok(()) if file_size(&out) > 0 => done = true,
                    Err(e @ (Error::Cancelled | Error::WrongPassword(_) | Error::Encrypted(_))) => return Err(e),
                    _ => {}
                }
            }
        }
        if !done {
            run_gs(&base, &input, &out, ctx.password(i), ctx.cancel)?;
            ctx.notes.push(format!("{}: converted without an embedded colour profile, strict PDF/A validators may complain", ctx.name(i)));
        }
        let stem = ctx.stem_n(i, count);
        ctx.emit_file(&stem, "pdf", &out)?;
    }
    Ok(())
}
