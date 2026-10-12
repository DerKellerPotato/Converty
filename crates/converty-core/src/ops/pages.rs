//! Structure tools: merge, split, remove, extract, organize, rotate, crop.

use super::pdfdoc::{self, assemble, Pick};
use super::util::pdf_err;
use super::Ctx;
use crate::{parse_ranges, CropOptions, Error, PageOp, Result, Rotation, SplitOptions};
use lopdf::{Document, Object};

fn picks_for(pages: &[u32], rotate: i64) -> Vec<Pick> {
    pages.iter().map(|&p| Pick::Page { src: 0, page: p, rotate }).collect()
}

fn count(doc: &Document) -> u32 {
    doc.get_pages().len() as u32
}

pub(super) fn merge(ctx: &mut Ctx<'_>) -> Result<()> {
    let n = ctx.req.inputs.len();
    let mut docs = Vec::with_capacity(n);
    for i in 0..n {
        ctx.check()?;
        ctx.step(i, n + 1, &format!("Reading {}", ctx.name(i)));
        docs.push(ctx.load(i)?);
    }
    let mut picks = Vec::new();
    for (s, d) in docs.iter().enumerate() {
        for p in 1..=count(d) {
            picks.push(Pick::Page { src: s, page: p, rotate: 0 });
        }
    }
    if picks.is_empty() {
        return Err(Error::Pdf("The files contain no pages".into()));
    }
    ctx.step(n, n + 1, "Merging");
    let mut out = assemble(&docs, &picks)?;
    let stem = ctx.req.output_stem.clone();
    ctx.emit_pdf(&stem, &mut out)?;
    Ok(())
}

pub(super) fn split(ctx: &mut Ctx<'_>, opts: &SplitOptions) -> Result<()> {
    let doc = ctx.load(0)?;
    let total = count(&doc);
    let src = std::slice::from_ref(&doc);
    // each group becomes one output
    let groups: Vec<Vec<u32>> = match opts {
        SplitOptions::Ranges { ranges, merge_all } => {
            let ranges: Vec<&String> = ranges.iter().filter(|r| !r.trim().is_empty()).collect();
            if ranges.is_empty() {
                return Err(Error::InvalidRange("Enter at least one range".into()));
            }
            let mut g = Vec::new();
            for r in ranges {
                g.push(parse_ranges(r, total)?);
            }
            if *merge_all {
                let mut seen = std::collections::HashSet::new();
                let all: Vec<u32> = g.into_iter().flatten().filter(|p| seen.insert(*p)).collect();
                vec![all]
            } else {
                g
            }
        }
        SplitOptions::Fixed { every } => {
            if *every == 0 {
                return Err(Error::InvalidOptions("Pages per part must be at least 1".into()));
            }
            (1..=total).collect::<Vec<_>>().chunks(*every as usize).map(|c| c.to_vec()).collect()
        }
        SplitOptions::EveryPage => (1..=total).map(|p| vec![p]).collect(),
    };
    let n = groups.len();
    for (i, g) in groups.iter().enumerate() {
        ctx.check()?;
        ctx.step(i, n, &format!("Writing part {} of {}", i + 1, n));
        let mut out = assemble(src, &picks_for(g, 0))?;
        let stem = ctx.stem_n(i, n);
        ctx.emit_pdf(&stem, &mut out)?;
    }
    Ok(())
}

pub(super) fn remove(ctx: &mut Ctx<'_>, pages: &str) -> Result<()> {
    if pages.trim().is_empty() {
        return Err(Error::InvalidRange("Select the pages to remove".into()));
    }
    let doc = ctx.load(0)?;
    let total = count(&doc);
    let drop: std::collections::HashSet<u32> = parse_ranges(pages, total)?.into_iter().collect();
    let keep: Vec<u32> = (1..=total).filter(|p| !drop.contains(p)).collect();
    if keep.is_empty() {
        return Err(Error::InvalidRange("You cannot remove every page".into()));
    }
    ctx.report(0.3, "Rebuilding");
    let mut out = assemble(std::slice::from_ref(&doc), &picks_for(&keep, 0))?;
    let stem = ctx.req.output_stem.clone();
    ctx.emit_pdf(&stem, &mut out)?;
    Ok(())
}

pub(super) fn extract(ctx: &mut Ctx<'_>, pages: &str, separate: bool) -> Result<()> {
    let doc = ctx.load(0)?;
    let sel = parse_ranges(pages, count(&doc))?;
    let src = std::slice::from_ref(&doc);
    if separate && sel.len() > 1 {
        let n = sel.len();
        for (i, p) in sel.iter().enumerate() {
            ctx.check()?;
            ctx.step(i, n, &format!("Extracting page {p}"));
            let mut out = assemble(src, &picks_for(&[*p], 0))?;
            let stem = ctx.stem_n(i, n);
            ctx.emit_pdf(&stem, &mut out)?;
        }
    } else {
        let mut out = assemble(src, &picks_for(&sel, 0))?;
        let stem = ctx.req.output_stem.clone();
        ctx.emit_pdf(&stem, &mut out)?;
    }
    Ok(())
}

pub(super) fn organize(ctx: &mut Ctx<'_>, ops: &[PageOp]) -> Result<()> {
    if ops.is_empty() {
        return Err(Error::InvalidOptions("The page list is empty".into()));
    }
    let doc = ctx.load(0)?;
    let ids = pdfdoc::pages_vec(&doc);
    let total = ids.len() as u32;
    let mut picks = Vec::with_capacity(ops.len());
    let mut last = pdfdoc::A4;
    for op in ops {
        match *op {
            PageOp::Page { source, rotate } => {
                if source == 0 || source > total {
                    return Err(Error::InvalidRange(format!("Page {source} does not exist (the document has {total} pages)")));
                }
                let (w, h) = pdfdoc::visual_size(&doc, ids[source as usize - 1]);
                last = if rotate.degrees() % 180 == 90 { (h, w) } else { (w, h) };
                picks.push(Pick::Page { src: 0, page: source, rotate: rotate.degrees() });
            }
            PageOp::Blank => picks.push(Pick::Blank { w: last.0, h: last.1 }),
        }
    }
    let mut out = assemble(std::slice::from_ref(&doc), &picks)?;
    let stem = ctx.req.output_stem.clone();
    ctx.emit_pdf(&stem, &mut out)?;
    Ok(())
}

pub(super) fn rotate(ctx: &mut Ctx<'_>, deg: Rotation, pages: &str) -> Result<()> {
    let n = ctx.req.inputs.len();
    for i in 0..n {
        ctx.check()?;
        ctx.step(i, n, &format!("Rotating {}", ctx.name(i)));
        let mut doc = ctx.load(i)?;
        let ids = pdfdoc::pages_vec(&doc);
        for p in parse_ranges(pages, ids.len() as u32)? {
            let id = ids[p as usize - 1];
            let new = (pdfdoc::page_rotation(&doc, id) + deg.degrees()).rem_euclid(360);
            doc.get_dictionary_mut(id).map_err(pdf_err)?.set("Rotate", Object::Integer(new));
        }
        let stem = ctx.stem_n(i, n);
        ctx.emit_pdf(&stem, &mut doc)?;
    }
    Ok(())
}

pub(super) fn crop(ctx: &mut Ctx<'_>, o: &CropOptions) -> Result<()> {
    let mut doc = ctx.load(0)?;
    let ids = pdfdoc::pages_vec(&doc);
    let (t, r, b, l) = (o.top.max(0.0) as f64, o.right.max(0.0) as f64, o.bottom.max(0.0) as f64, o.left.max(0.0) as f64);
    for p in parse_ranges(&o.pages, ids.len() as u32)? {
        let id = ids[p as usize - 1];
        let bx = pdfdoc::page_box(&doc, id);
        // visual margins -> user space margins (left, bottom, right, top)
        let (ml, mb, mr, mt) = match pdfdoc::page_rotation(&doc, id) {
            90 => (t, l, b, r),
            180 => (r, t, l, b),
            270 => (b, r, t, l),
            _ => (l, b, r, t),
        };
        let nb = [bx[0] + ml, bx[1] + mb, bx[2] - mr, bx[3] - mt];
        if nb[2] - nb[0] < 1.0 || nb[3] - nb[1] < 1.0 {
            return Err(Error::InvalidOptions(format!("The margins leave nothing of page {p}")));
        }
        let arr = nb.iter().map(|v| Object::Real(*v as f32)).collect::<Vec<_>>();
        doc.get_dictionary_mut(id).map_err(pdf_err)?.set("CropBox", Object::Array(arr));
    }
    let stem = ctx.req.output_stem.clone();
    ctx.emit_pdf(&stem, &mut doc)?;
    Ok(())
}
