//! Content-stream stamping: page numbers, watermark, signature.

use super::images::embed_image;
use super::pdfdoc::{self, add_font, add_opacity_gs, add_overlay, add_resource, fmt, geom, pdf_string, text_width, Face, Geom, Wrap};
use super::Ctx;
use crate::{
    parse_ranges, Error, PageNumberOptions, Position, Result, Rgb, SignOptions, SignatureKind, WatermarkContent,
    WatermarkOptions,
};
use lopdf::{Document, ObjectId};

const LOSSY_NOTE: &str = "Some characters are not in the built-in font and were drawn as “?”";

/// True when `text` has characters the built-in WinAnsi fonts cannot show.
fn lossy(text: &str) -> bool {
    pdfdoc::winansi(text).iter().zip(text.chars()).any(|(b, c)| *b == b'?' && c != '?')
}

fn note_lossy(ctx: &mut Ctx<'_>, text: &str) {
    if lossy(text) && !ctx.notes.iter().any(|n| n == LOSSY_NOTE) {
        ctx.notes.push(LOSSY_NOTE.to_string());
    }
}

fn col(c: Rgb) -> String {
    format!("{} {} {} rg\n", fmt(c.0 as f64 / 255.0), fmt(c.1 as f64 / 255.0), fmt(c.2 as f64 / 255.0))
}

#[derive(Clone, Copy, PartialEq)]
enum H {
    L,
    C,
    R,
}
#[derive(Clone, Copy, PartialEq)]
enum V {
    T,
    M,
    B,
}

fn split_pos(p: Position) -> (H, V) {
    use Position::*;
    match p {
        TopLeft => (H::L, V::T),
        TopCenter => (H::C, V::T),
        TopRight => (H::R, V::T),
        MiddleLeft => (H::L, V::M),
        Center => (H::C, V::M),
        MiddleRight => (H::R, V::M),
        BottomLeft => (H::L, V::B),
        BottomCenter => (H::C, V::B),
        BottomRight => (H::R, V::B),
    }
}

/// Lower-left corner of a `bw` x `bh` box on a `w` x `h` page.
fn place(h: H, v: V, w: f64, ph: f64, bw: f64, bh: f64, margin: f64) -> (f64, f64) {
    let x = match h {
        H::L => margin,
        H::C => (w - bw) / 2.0,
        H::R => w - margin - bw,
    };
    let y = match v {
        V::T => ph - margin - bh,
        V::M => (ph - bh) / 2.0,
        V::B => margin,
    };
    (x, y)
}

fn page_ids(doc: &Document, sel: &str) -> Result<Vec<(u32, ObjectId)>> {
    let ids = pdfdoc::pages_vec(doc);
    Ok(parse_ranges(sel, ids.len() as u32)?.into_iter().map(|p| (p, ids[p as usize - 1])).collect())
}

pub(super) fn page_numbers(ctx: &mut Ctx<'_>, o: &PageNumberOptions) -> Result<()> {
    let mut doc = ctx.load(0)?;
    let all = pdfdoc::pages_vec(&doc);
    let total = all.len();
    let sel = page_ids(&doc, &o.pages)?;
    let font = add_font(&mut doc, Face::Helvetica.base_font());
    let wrap = Wrap::new(&mut doc);
    let fs = o.font_size.clamp(4.0, 120.0) as f64;
    let (h0, v) = split_pos(o.position);
    let n = sel.len();
    note_lossy(ctx, &o.template);
    for (i, (p, id)) in sel.iter().enumerate() {
        ctx.check()?;
        if i % 20 == 0 {
            ctx.step(i, n, "Adding page numbers");
        }
        let num = o.first_number as u64 + (*p as u64 - 1);
        let text = o.template.replace("{n}", &num.to_string()).replace("{p}", &total.to_string());
        let mut hpos = h0;
        if o.facing_pages && p % 2 == 0 {
            hpos = match h0 {
                H::L => H::R,
                H::R => H::L,
                c => c,
            };
        }
        let g = geom(&doc, *id);
        let tw = text_width(&text, Face::Helvetica, fs);
        let (x, y) = place(hpos, v, g.w, g.h, tw, fs, o.margin as f64);
        let mut c = Vec::new();
        c.extend(format!("q\n{}{}BT /CvF1 {} Tf {} {} Td ", g.base_cm(), col(o.color), fmt(fs), fmt(x), fmt(y + fs * 0.22)).bytes());
        c.extend(pdf_string(&text));
        c.extend(b" Tj ET\nQ\n");
        add_resource(&mut doc, *id, "Font", "CvF1", font)?;
        add_overlay(&mut doc, *id, c, false, &wrap)?;
    }
    let stem = ctx.req.output_stem.clone();
    ctx.emit_pdf(&stem, &mut doc)?;
    Ok(())
}

pub(super) fn watermark(ctx: &mut Ctx<'_>, o: &WatermarkOptions) -> Result<()> {
    let mut doc = ctx.load(0)?;
    let sel = page_ids(&doc, &o.pages)?;
    let wrap = Wrap::new(&mut doc);
    let gs = (o.opacity < 0.999).then(|| add_opacity_gs(&mut doc, o.opacity));

    enum Kind {
        Text { text: String, fs: f64, color: Rgb, font: ObjectId, bw: f64, bh: f64 },
        Image { img: ObjectId, aspect: f64, scale: f64 },
    }
    let kind = match &o.content {
        WatermarkContent::Text { text, font_size, color, bold } => {
            if text.trim().is_empty() {
                return Err(Error::InvalidOptions("Enter the watermark text".into()));
            }
            note_lossy(ctx, text);
            let face = if *bold { Face::HelveticaBold } else { Face::Helvetica };
            let fs = (*font_size as f64).clamp(4.0, 400.0);
            let font = add_font(&mut doc, face.base_font());
            Kind::Text { text: text.clone(), fs, color: *color, font, bw: text_width(text, face, fs), bh: fs }
        }
        WatermarkContent::Image { path, scale } => {
            let (img, w, h) = embed_image(&mut doc, path)?;
            Kind::Image { img, aspect: h as f64 / w as f64, scale: (*scale as f64).clamp(0.02, 1.0) }
        }
    };
    let angle = (o.rotation_deg as f64).to_radians();
    let (sin, cos) = angle.sin_cos();
    let (hp, vp) = split_pos(o.position);
    let n = sel.len();

    for (i, (_, id)) in sel.iter().enumerate() {
        ctx.check()?;
        if i % 20 == 0 {
            ctx.step(i, n, "Adding watermark");
        }
        let g: Geom = geom(&doc, *id);
        // size of one mark
        let (bw, bh) = match &kind {
            Kind::Text { bw, bh, .. } => (*bw, *bh),
            Kind::Image { aspect, scale, .. } => (g.w * scale, g.w * scale * aspect),
        };
        let mut c = format!("q\n{}", g.base_cm());
        if gs.is_some() {
            c.push_str("/CvGS1 gs\n");
        }
        if let Kind::Text { color, .. } = &kind {
            c.push_str(&col(*color));
        }
        // draws one mark centred on the current origin, shifted by (dx, dy)
        let mark = |dx: f64, dy: f64| -> Vec<u8> {
            match &kind {
                Kind::Text { text, fs, .. } => {
                    let mut b = format!("BT /CvF1 {} Tf 1 0 0 1 {} {} Tm ", fmt(*fs), fmt(dx - bw / 2.0), fmt(dy - fs * 0.35)).into_bytes();
                    b.extend(pdf_string(text));
                    b.extend(b" Tj ET\n");
                    b
                }
                Kind::Image { .. } => {
                    format!("q {} 0 0 {} {} {} cm /CvIm1 Do Q\n", fmt(bw), fmt(bh), fmt(dx - bw / 2.0), fmt(dy - bh / 2.0)).into_bytes()
                }
            }
        };
        let mut body: Vec<u8> = Vec::new();
        if o.mosaic {
            c.push_str(&format!("1 0 0 1 {} {} cm\n{} {} {} {} 0 0 cm\n", fmt(g.w / 2.0), fmt(g.h / 2.0), fmt(cos), fmt(sin), fmt(-sin), fmt(cos)));
            let gap = (bh * 1.2).max(16.0);
            let (cw, ch) = ((bw + gap).max(8.0), (bh + gap).max(8.0));
            let radius = (g.w * g.w + g.h * g.h).sqrt() / 2.0 + bw.max(bh);
            let (mut cw, mut ch) = (cw, ch);
            while (2.0 * radius / cw) * (2.0 * radius / ch) > 6000.0 {
                cw *= 1.5;
                ch *= 1.5;
            }
            let (nx, ny) = ((radius / cw).ceil() as i64, (radius / ch).ceil() as i64);
            for iy in -ny..=ny {
                let off = if iy.rem_euclid(2) == 1 { cw / 2.0 } else { 0.0 };
                for ix in -nx..=nx {
                    body.extend(mark(ix as f64 * cw + off, iy as f64 * ch));
                }
            }
        } else {
            let ex = (bw * cos).abs() + (bh * sin).abs();
            let ey = (bw * sin).abs() + (bh * cos).abs();
            let pad = 24.0;
            let cx = match hp {
                H::L => pad + ex / 2.0,
                H::C => g.w / 2.0,
                H::R => g.w - pad - ex / 2.0,
            };
            let cy = match vp {
                V::T => g.h - pad - ey / 2.0,
                V::M => g.h / 2.0,
                V::B => pad + ey / 2.0,
            };
            c.push_str(&format!("1 0 0 1 {} {} cm\n{} {} {} {} 0 0 cm\n", fmt(cx), fmt(cy), fmt(cos), fmt(sin), fmt(-sin), fmt(cos)));
            body.extend(mark(0.0, 0.0));
        }
        let mut content = c.into_bytes();
        content.extend(body);
        content.extend(b"Q\n");
        match &kind {
            Kind::Text { font, .. } => add_resource(&mut doc, *id, "Font", "CvF1", *font)?,
            Kind::Image { img, .. } => add_resource(&mut doc, *id, "XObject", "CvIm1", *img)?,
        }
        if let Some(gs) = gs {
            add_resource(&mut doc, *id, "ExtGState", "CvGS1", gs)?;
        }
        add_overlay(&mut doc, *id, content, o.below_content, &wrap)?;
    }
    let stem = ctx.req.output_stem.clone();
    ctx.emit_pdf(&stem, &mut doc)?;
    Ok(())
}

/// Today as YYYY-MM-DD (local time through `date`, UTC fallback).
fn today() -> String {
    #[cfg(unix)]
    if let Ok(out) = std::process::Command::new("date").arg("+%Y-%m-%d").output() {
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if out.status.success() && s.len() == 10 {
            return s;
        }
    }
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let z = (secs / 86400) as i64 + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let mut y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    if m <= 2 {
        y += 1;
    }
    format!("{y:04}-{m:02}-{d:02}")
}

pub(super) fn sign(ctx: &mut Ctx<'_>, o: &SignOptions) -> Result<()> {
    let mut doc = ctx.load(0)?;
    let ids = pdfdoc::pages_vec(&doc);
    let page = if o.page == 0 { ids.len() as u32 } else { o.page };
    if page == 0 || page as usize > ids.len() {
        return Err(Error::InvalidRange(format!("Page {page} does not exist (the document has {} pages)", ids.len())));
    }
    let id = ids[page as usize - 1];
    let g = geom(&doc, id);
    let wrap = Wrap::new(&mut doc);
    let width = (o.width as f64).clamp(20.0, g.w.max(21.0));
    let date_h = if o.add_date { 14.0 } else { 0.0 };
    let date = today();
    let (hp, vp) = split_pos(o.position);
    let margin = 36.0;

    let mut c = format!("q\n{}", g.base_cm()).into_bytes();
    match &o.signature {
        SignatureKind::Typed { name } => {
            if name.trim().is_empty() {
                return Err(Error::InvalidOptions("Type your name to sign".into()));
            }
            note_lossy(ctx, name);
            let unit = text_width(name, Face::TimesItalic, 1.0).max(0.1);
            let fs = (width / unit).clamp(8.0, 64.0);
            let bw = text_width(name, Face::TimesItalic, fs).max(if o.add_date { 70.0 } else { 0.0 });
            let bh = fs * 1.15 + date_h;
            let (x, y) = place(hp, vp, g.w, g.h, bw, bh, margin);
            let font = add_font(&mut doc, Face::TimesItalic.base_font());
            add_resource(&mut doc, id, "Font", "CvF2", font)?;
            c.extend(format!("0.05 0.12 0.45 rg\nBT /CvF2 {} Tf {} {} Td ", fmt(fs), fmt(x), fmt(y + date_h + fs * 0.25)).bytes());
            c.extend(pdf_string(name));
            c.extend(b" Tj ET\n");
            if o.add_date {
                let hf = add_font(&mut doc, Face::Helvetica.base_font());
                add_resource(&mut doc, id, "Font", "CvF1", hf)?;
                c.extend(format!("0.25 0.25 0.25 rg\nBT /CvF1 9 Tf {} {} Td ", fmt(x), fmt(y + 2.0)).bytes());
                c.extend(pdf_string(&date));
                c.extend(b" Tj ET\n");
            }
        }
        SignatureKind::Image { path } => {
            let (img, iw, ih) = embed_image(&mut doc, path)?;
            let bh_img = width * ih as f64 / iw as f64;
            let bh = bh_img + date_h;
            let (x, y) = place(hp, vp, g.w, g.h, width, bh, margin);
            add_resource(&mut doc, id, "XObject", "CvIm1", img)?;
            c.extend(format!("q {} 0 0 {} {} {} cm /CvIm1 Do Q\n", fmt(width), fmt(bh_img), fmt(x), fmt(y + date_h)).bytes());
            if o.add_date {
                let hf = add_font(&mut doc, Face::Helvetica.base_font());
                add_resource(&mut doc, id, "Font", "CvF1", hf)?;
                c.extend(format!("0.25 0.25 0.25 rg\nBT /CvF1 9 Tf {} {} Td ", fmt(x), fmt(y + 2.0)).bytes());
                c.extend(pdf_string(&date));
                c.extend(b" Tj ET\n");
            }
        }
    }
    c.extend(b"Q\n");
    add_overlay(&mut doc, id, c, false, &wrap)?;
    let stem = ctx.req.output_stem.clone();
    ctx.emit_pdf(&stem, &mut doc)?;
    Ok(())
}
