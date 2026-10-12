//! Images to PDF and image embedding.

use super::util::pdf_err;
use super::Ctx;
use crate::{Error, ImagesToPdfOptions, Margin, Orientation, PageSize, Result};
use image::{ColorType, DynamicImage, ImageDecoder, ImageReader};
use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};
use std::io::Cursor;
use std::path::Path;

fn img_err(path: &Path, e: impl std::fmt::Display) -> Error {
    Error::Image(format!("{}: {}", path.display(), e))
}

/// (components, width, height) from the first JPEG SOF marker.
fn jpeg_info(b: &[u8]) -> Option<(u8, u32, u32)> {
    if b.len() < 4 || b[0] != 0xFF || b[1] != 0xD8 {
        return None;
    }
    let mut i = 2;
    while i + 4 <= b.len() {
        if b[i] != 0xFF {
            i += 1;
            continue;
        }
        let m = b[i + 1];
        if m == 0xFF {
            i += 1;
            continue;
        }
        if m == 0xD8 || m == 0x01 || (0xD0..=0xD7).contains(&m) {
            i += 2;
            continue;
        }
        let len = u16::from_be_bytes([b[i + 2], b[i + 3]]) as usize;
        if (0xC0..=0xCF).contains(&m) && m != 0xC4 && m != 0xC8 && m != 0xCC {
            if i + 10 > b.len() {
                return None;
            }
            let h = u16::from_be_bytes([b[i + 5], b[i + 6]]) as u32;
            let w = u16::from_be_bytes([b[i + 7], b[i + 8]]) as u32;
            return Some((b[i + 9], w, h));
        }
        i += 2 + len;
    }
    None
}

/// Embeds an image file as an XObject. JPEGs go in as-is (DCTDecode); everything else is
/// decoded to Flate RGB/Gray with an SMask for transparency.
/// Returns the object id and the pixel size (after EXIF orientation).
pub(crate) fn embed_image(doc: &mut Document, path: &Path) -> Result<(ObjectId, u32, u32)> {
    let bytes = std::fs::read(path)?;
    let orientation = {
        let reader = ImageReader::new(Cursor::new(&bytes)).with_guessed_format().map_err(|e| img_err(path, e))?;
        let mut decoder = reader.into_decoder().map_err(|e| img_err(path, e))?;
        decoder.orientation().unwrap_or(image::metadata::Orientation::NoTransforms)
    };

    if orientation == image::metadata::Orientation::NoTransforms {
        if let Some((comps, w, h)) = jpeg_info(&bytes) {
            if matches!(comps, 1 | 3 | 4) {
                let mut d = dictionary! {
                    "Type" => "XObject",
                    "Subtype" => "Image",
                    "Width" => w as i64,
                    "Height" => h as i64,
                    "BitsPerComponent" => 8,
                    "Filter" => "DCTDecode",
                    "ColorSpace" => match comps { 1 => "DeviceGray", 4 => "DeviceCMYK", _ => "DeviceRGB" },
                };
                if comps == 4 {
                    d.set("Decode", vec![1.into(), 0.into(), 1.into(), 0.into(), 1.into(), 0.into(), 1.into(), 0.into()]);
                }
                let id = doc.add_object(Stream::new(d, bytes).with_compression(false));
                return Ok((id, w, h));
            }
        }
    }

    let decoder = ImageReader::new(Cursor::new(&bytes))
        .with_guessed_format()
        .map_err(|e| img_err(path, e))?
        .into_decoder()
        .map_err(|e| img_err(path, e))?;
    let mut img = DynamicImage::from_decoder(decoder).map_err(|e| img_err(path, e))?;
    img.apply_orientation(orientation);
    let (w, h) = (img.width(), img.height());
    let color = img.color();
    let gray = matches!(color, ColorType::L8 | ColorType::La8 | ColorType::L16 | ColorType::La16);
    let alpha = color.has_alpha() && {
        // fully opaque alpha channels do not need an SMask
        let a = img.to_rgba8();
        a.pixels().any(|p| p.0[3] != 255)
    };

    let (data, smask): (Vec<u8>, Option<Vec<u8>>) = if gray {
        if alpha {
            let la = img.to_luma_alpha8();
            let (mut g, mut a) = (Vec::new(), Vec::new());
            for p in la.pixels() {
                g.push(p.0[0]);
                a.push(p.0[1]);
            }
            (g, Some(a))
        } else {
            (img.to_luma8().into_raw(), None)
        }
    } else if alpha {
        let rgba = img.to_rgba8();
        let (mut c, mut a) = (Vec::with_capacity(rgba.len() / 4 * 3), Vec::with_capacity(rgba.len() / 4));
        for p in rgba.pixels() {
            c.extend_from_slice(&p.0[..3]);
            a.push(p.0[3]);
        }
        (c, Some(a))
    } else {
        (img.to_rgb8().into_raw(), None)
    };

    let mut d = dictionary! {
        "Type" => "XObject",
        "Subtype" => "Image",
        "Width" => w as i64,
        "Height" => h as i64,
        "BitsPerComponent" => 8,
        "ColorSpace" => if gray { "DeviceGray" } else { "DeviceRGB" },
    };
    if let Some(a) = smask {
        let mut s = Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Image", "Width" => w as i64, "Height" => h as i64,
                "BitsPerComponent" => 8, "ColorSpace" => "DeviceGray",
            },
            a,
        );
        s.compress().map_err(pdf_err)?;
        let sid = doc.add_object(s);
        d.set("SMask", sid);
    }
    let mut s = Stream::new(d, data);
    s.compress().map_err(pdf_err)?;
    Ok((doc.add_object(s), w, h))
}

pub(super) fn images_to_pdf(ctx: &mut Ctx<'_>, o: &ImagesToPdfOptions) -> Result<()> {
    let n = ctx.req.inputs.len();
    let margin = match o.margin {
        Margin::None => 0.0,
        Margin::Small => 18.0,
        Margin::Big => 36.0,
    };
    let groups: Vec<Vec<usize>> = if o.merge { vec![(0..n).collect()] } else { (0..n).map(|i| vec![i]).collect() };
    let total = groups.len();
    let mut done = 0usize;
    for (gi, group) in groups.iter().enumerate() {
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let mut kids = Vec::new();
        for &i in group {
            ctx.check()?;
            ctx.step(done, n, &format!("Adding {}", ctx.name(i)));
            done += 1;
            let (img, iw, ih) = embed_image(&mut doc, ctx.input_path(i))?;
            let (iw, ih) = (iw as f64, ih as f64);
            let (pw, ph) = match o.page_size {
                PageSize::Fit => (iw * 0.75 + 2.0 * margin, ih * 0.75 + 2.0 * margin),
                size => {
                    let (a, b) = if size == PageSize::A4 { (595.28, 841.89) } else { (612.0, 792.0) };
                    let landscape = match o.orientation {
                        Orientation::Auto => iw > ih,
                        Orientation::Landscape => true,
                        Orientation::Portrait => false,
                    };
                    if landscape {
                        (b, a)
                    } else {
                        (a, b)
                    }
                }
            };
            let (cw, ch) = (pw - 2.0 * margin, ph - 2.0 * margin);
            let scale = (cw / iw).min(ch / ih);
            let (dw, dh) = (iw * scale, ih * scale);
            let (x, y) = ((pw - dw) / 2.0, (ph - dh) / 2.0);
            let content = format!("q {:.3} 0 0 {:.3} {:.3} {:.3} cm /Im1 Do Q\n", dw, dh, x, y);
            let cid = doc.add_object(Stream::new(Dictionary::new(), content.into_bytes()));
            let page = doc.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "MediaBox" => vec![0.into(), 0.into(), Object::Real(pw as f32), Object::Real(ph as f32)],
                "Contents" => cid,
                "Resources" => dictionary! { "XObject" => dictionary! { "Im1" => img } },
            });
            kids.push(Object::Reference(page));
        }
        let count = kids.len() as i64;
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => count }),
        );
        let cat = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", cat);
        let stem = ctx.stem_n(gi, total);
        ctx.emit_pdf(&stem, &mut doc)?;
    }
    Ok(())
}
