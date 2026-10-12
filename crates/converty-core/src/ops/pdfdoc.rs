//! lopdf helpers: page geometry, page-tree rebuilding, content stream stamping.

use crate::{Error, Result};
use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};
use std::collections::{HashMap, HashSet};

pub(crate) fn num(o: &Object) -> Option<f64> {
    match o {
        Object::Integer(i) => Some(*i as f64),
        Object::Real(r) => Some(*r as f64),
        _ => None,
    }
}

/// Inherited page attribute (Resources, MediaBox, CropBox, Rotate), dereferenced and cloned.
pub(crate) fn inherited(doc: &Document, page: ObjectId, key: &[u8]) -> Option<Object> {
    let mut id = page;
    for _ in 0..32 {
        let dict = doc.get_dictionary(id).ok()?;
        if let Ok(v) = dict.get(key) {
            return doc.dereference(v).ok().map(|(_, o)| o.clone());
        }
        id = dict.get(b"Parent").ok()?.as_reference().ok()?;
    }
    None
}

fn rect_of(doc: &Document, obj: &Object) -> Option<[f64; 4]> {
    let arr = obj.as_array().ok()?;
    if arr.len() < 4 {
        return None;
    }
    let mut v = [0.0; 4];
    for (i, o) in arr.iter().take(4).enumerate() {
        let (_, o) = doc.dereference(o).ok()?;
        v[i] = num(o)?;
    }
    Some([v[0].min(v[2]), v[1].min(v[3]), v[0].max(v[2]), v[1].max(v[3])])
}

pub(crate) const A4: (f64, f64) = (595.0, 842.0);

pub(crate) fn media_box(doc: &Document, page: ObjectId) -> [f64; 4] {
    inherited(doc, page, b"MediaBox")
        .and_then(|o| rect_of(doc, &o))
        .filter(|r| r[2] - r[0] > 0.0 && r[3] - r[1] > 0.0)
        .unwrap_or([0.0, 0.0, A4.0, A4.1])
}

/// Visible box: CropBox (clipped to MediaBox) or MediaBox.
pub(crate) fn page_box(doc: &Document, page: ObjectId) -> [f64; 4] {
    let m = media_box(doc, page);
    match inherited(doc, page, b"CropBox").and_then(|o| rect_of(doc, &o)) {
        Some(c) => {
            let r = [c[0].max(m[0]), c[1].max(m[1]), c[2].min(m[2]), c[3].min(m[3])];
            if r[2] - r[0] > 0.0 && r[3] - r[1] > 0.0 {
                r
            } else {
                m
            }
        }
        None => m,
    }
}

pub(crate) fn page_rotation(doc: &Document, page: ObjectId) -> i64 {
    let r = inherited(doc, page, b"Rotate").and_then(|o| num(&o)).unwrap_or(0.0) as i64;
    (r.rem_euclid(360) / 90) * 90
}

/// Displayed size (after rotation).
pub(crate) fn visual_size(doc: &Document, page: ObjectId) -> (f64, f64) {
    let b = page_box(doc, page);
    let (w, h) = (b[2] - b[0], b[3] - b[1]);
    if page_rotation(doc, page) % 180 == 90 {
        (h, w)
    } else {
        (w, h)
    }
}

pub(crate) fn pages_vec(doc: &Document) -> Vec<ObjectId> {
    doc.get_pages().into_values().collect()
}

// ---------------------------------------------------------------------------
// Rebuilding a document from picked pages

#[derive(Debug, Clone, Copy)]
pub(crate) enum Pick {
    /// `src` indexes into the source slice, `page` is 1-based, `rotate` is extra degrees.
    Page { src: usize, page: u32, rotate: i64 },
    Blank { w: f64, h: f64 },
}

struct Builder<'a> {
    srcs: &'a [Document],
    out: Document,
    map: HashMap<(usize, ObjectId), ObjectId>,
    all_pages: Vec<HashSet<ObjectId>>,
    first_pick: HashMap<(usize, ObjectId), ObjectId>,
    queue: Vec<(usize, ObjectId, ObjectId)>,
}

impl Builder<'_> {
    fn conv(&mut self, src: usize, obj: &Object) -> Object {
        match obj {
            Object::Reference(id) => {
                let id = *id;
                if self.all_pages[src].contains(&id) {
                    return match self.first_pick.get(&(src, id)) {
                        Some(n) => Object::Reference(*n),
                        None => Object::Null,
                    };
                }
                if let Some(n) = self.map.get(&(src, id)) {
                    return Object::Reference(*n);
                }
                let Some(target) = self.srcs[src].objects.get(&id) else { return Object::Null };
                if matches!(target.type_name().unwrap_or(b""), b"Pages" | b"Catalog" | b"ObjStm" | b"XRef") {
                    return Object::Null;
                }
                let n = self.out.new_object_id();
                self.map.insert((src, id), n);
                self.queue.push((src, id, n));
                Object::Reference(n)
            }
            Object::Array(a) => Object::Array(a.iter().map(|o| self.conv(src, o)).collect()),
            Object::Dictionary(d) => Object::Dictionary(self.conv_dict(src, d)),
            Object::Stream(s) => {
                let mut ns = s.clone();
                ns.dict = self.conv_dict(src, &s.dict);
                Object::Stream(ns)
            }
            o => o.clone(),
        }
    }

    fn conv_dict(&mut self, src: usize, d: &Dictionary) -> Dictionary {
        let mut nd = Dictionary::new();
        for (k, v) in d.iter() {
            nd.set(k.clone(), self.conv(src, v));
        }
        nd
    }

    fn drain(&mut self) {
        while let Some((src, old, new)) = self.queue.pop() {
            let obj = self.srcs[src].objects.get(&old).cloned().unwrap_or(Object::Null);
            let converted = self.conv(src, &obj);
            self.out.objects.insert(new, converted);
        }
    }
}

/// Builds a brand-new document that contains only the picked pages (in order) and
/// what they reference. Inherited attributes are pushed down onto each page.
pub(crate) fn assemble(srcs: &[Document], picks: &[Pick]) -> Result<Document> {
    let page_maps: Vec<Vec<ObjectId>> = srcs.iter().map(pages_vec).collect();
    let mut b = Builder {
        srcs,
        out: Document::with_version("1.7"),
        map: HashMap::new(),
        all_pages: page_maps.iter().map(|v| v.iter().copied().collect()).collect(),
        first_pick: HashMap::new(),
        queue: Vec::new(),
    };
    let pages_id = b.out.new_object_id();

    // phase 1: allocate ids for every pick
    let mut new_ids = Vec::with_capacity(picks.len());
    for p in picks {
        let id = b.out.new_object_id();
        if let Pick::Page { src, page, .. } = *p {
            let old = *page_maps
                .get(src)
                .and_then(|v| v.get(page as usize - 1))
                .ok_or_else(|| Error::InvalidRange(format!("Page {page} does not exist")))?;
            b.first_pick.entry((src, old)).or_insert(id);
        }
        new_ids.push(id);
    }

    // phase 2: build page dictionaries
    for (p, &new_id) in picks.iter().zip(&new_ids) {
        let dict = match *p {
            Pick::Blank { w, h } => dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "MediaBox" => vec![0.into(), 0.into(), Object::Real(w as f32), Object::Real(h as f32)],
                "Resources" => Dictionary::new(),
            },
            Pick::Page { src, page, rotate } => {
                let old = page_maps[src][page as usize - 1];
                let doc = &srcs[src];
                let mut d = doc.get_dictionary(old).map_err(|_| Error::Pdf("page object missing".into()))?.clone();
                for key in [&b"Resources"[..], b"MediaBox", b"CropBox"] {
                    if !d.has(key) {
                        if let Some(v) = inherited(doc, old, key) {
                            d.set(key.to_vec(), v);
                        }
                    }
                }
                let rot = (page_rotation(doc, old) + rotate).rem_euclid(360);
                if rot != 0 {
                    d.set("Rotate", rot);
                } else {
                    d.remove(b"Rotate");
                }
                d.remove(b"Parent");
                if !d.has(b"MediaBox") {
                    d.set("MediaBox", vec![0.into(), 0.into(), 595.into(), 842.into()]);
                }
                let mut nd = b.conv_dict(src, &d);
                nd.set("Parent", pages_id);
                nd
            }
        };
        b.out.objects.insert(new_id, Object::Dictionary(dict));
    }
    b.drain();

    let mut out = b.out;
    out.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => new_ids.iter().map(|i| Object::Reference(*i)).collect::<Vec<_>>(),
            "Count" => new_ids.len() as i64,
        }),
    );
    let catalog = out.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    out.trailer.set("Root", catalog);
    Ok(out)
}

// ---------------------------------------------------------------------------
// Stamping

#[derive(Debug, Clone, Copy)]
pub(crate) struct Geom {
    pub x0: f64,
    pub y0: f64,
    /// user-space box size
    pub bw: f64,
    pub bh: f64,
    pub rot: i64,
    /// visual size
    pub w: f64,
    pub h: f64,
}

pub(crate) fn geom(doc: &Document, page: ObjectId) -> Geom {
    let b = page_box(doc, page);
    let (bw, bh) = (b[2] - b[0], b[3] - b[1]);
    let rot = page_rotation(doc, page);
    let (w, h) = if rot % 180 == 90 { (bh, bw) } else { (bw, bh) };
    Geom { x0: b[0], y0: b[1], bw, bh, rot, w, h }
}

impl Geom {
    /// `cm` operator mapping visual coordinates (origin bottom-left of the displayed page)
    /// to user space.
    pub fn base_cm(&self) -> String {
        let (a, b, c, d, e, f) = match self.rot {
            90 => (0.0, 1.0, -1.0, 0.0, self.bw, 0.0),
            180 => (-1.0, 0.0, 0.0, -1.0, self.bw, self.bh),
            270 => (0.0, -1.0, 1.0, 0.0, 0.0, self.bh),
            _ => (1.0, 0.0, 0.0, 1.0, 0.0, 0.0),
        };
        format!("{} {} {} {} {} {} cm\n", fmt(a), fmt(b), fmt(c), fmt(d), fmt(e + self.x0), fmt(f + self.y0))
    }
}

pub(crate) fn fmt(v: f64) -> String {
    let s = format!("{:.3}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-" || s == "-0" {
        "0".into()
    } else {
        s.to_string()
    }
}

/// Makes the page's Resources a direct dictionary whose Font/XObject/ExtGState entries are
/// direct too, so entries can be added without touching shared objects.
fn ensure_direct_resources(doc: &mut Document, page: ObjectId) -> Result<()> {
    let res = match doc.get_dictionary(page).map_err(super::util::pdf_err)?.get(b"Resources") {
        Ok(o) => doc.dereference(o).ok().map(|(_, o)| o.clone()),
        Err(_) => inherited(doc, page, b"Resources"),
    };
    let mut res = res.and_then(|o| o.as_dict().ok().cloned()).unwrap_or_default();
    for cat in [&b"Font"[..], b"XObject", b"ExtGState"] {
        if let Ok(v) = res.get(cat) {
            if let Ok((_, o)) = doc.dereference(v) {
                if let Ok(d) = o.as_dict() {
                    let d = d.clone();
                    res.set(cat.to_vec(), Object::Dictionary(d));
                }
            }
        }
    }
    let p = doc.get_dictionary_mut(page).map_err(super::util::pdf_err)?;
    p.set("Resources", Object::Dictionary(res));
    Ok(())
}

pub(crate) fn add_resource(doc: &mut Document, page: ObjectId, category: &str, name: &str, value: ObjectId) -> Result<()> {
    let direct = doc
        .get_dictionary(page)
        .ok()
        .and_then(|d| d.get(b"Resources").ok())
        .and_then(|o| o.as_dict().ok())
        .map(|r| {
            [&b"Font"[..], b"XObject", b"ExtGState"]
                .iter()
                .all(|c| r.get(c).map(|o| o.as_dict().is_ok()).unwrap_or(true))
        })
        .unwrap_or(false);
    if !direct {
        ensure_direct_resources(doc, page)?;
    }
    let p = doc.get_dictionary_mut(page).map_err(super::util::pdf_err)?;
    let res = p.get_mut(b"Resources").and_then(Object::as_dict_mut).map_err(super::util::pdf_err)?;
    if !res.get(category.as_bytes()).map(|o| o.as_dict().is_ok()).unwrap_or(false) {
        res.set(category, Object::Dictionary(Dictionary::new()));
    }
    let cat = res.get_mut(category.as_bytes()).and_then(Object::as_dict_mut).map_err(super::util::pdf_err)?;
    cat.set(name, Object::Reference(value));
    Ok(())
}

/// Shared `q` / `Q` wrapper streams.
pub(crate) struct Wrap {
    q: ObjectId,
    big_q: ObjectId,
}

impl Wrap {
    pub fn new(doc: &mut Document) -> Wrap {
        let q = doc.add_object(Stream::new(Dictionary::new(), b"q\n".to_vec()));
        let big_q = doc.add_object(Stream::new(Dictionary::new(), b"Q\n".to_vec()));
        Wrap { q, big_q }
    }
}

/// Adds a content stream to the page, over (wrapped so the page's graphics state is
/// balanced) or under the existing content.
pub(crate) fn add_overlay(doc: &mut Document, page: ObjectId, content: Vec<u8>, below: bool, wrap: &Wrap) -> Result<()> {
    let mut stream = Stream::new(Dictionary::new(), content);
    let _ = stream.compress();
    let sid = doc.add_object(stream);
    let existing: Vec<Object> = match doc.get_dictionary(page).map_err(super::util::pdf_err)?.get(b"Contents") {
        Ok(Object::Array(a)) => a.clone(),
        Ok(Object::Reference(r)) => match doc.get_object(*r) {
            Ok(Object::Array(a)) => a.clone(),
            _ => vec![Object::Reference(*r)],
        },
        _ => vec![],
    };
    let mut list = Vec::new();
    if below {
        list.push(Object::Reference(sid));
        list.extend(existing);
    } else {
        list.push(Object::Reference(wrap.q));
        list.extend(existing);
        list.push(Object::Reference(wrap.big_q));
        list.push(Object::Reference(sid));
    }
    doc.get_dictionary_mut(page).map_err(super::util::pdf_err)?.set("Contents", Object::Array(list));
    Ok(())
}

pub(crate) fn add_font(doc: &mut Document, base_font: &str) -> ObjectId {
    doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => base_font,
        "Encoding" => "WinAnsiEncoding",
    })
}

pub(crate) fn add_opacity_gs(doc: &mut Document, opacity: f32) -> ObjectId {
    let o = opacity.clamp(0.0, 1.0);
    doc.add_object(dictionary! {
        "Type" => "ExtGState",
        "ca" => Object::Real(o),
        "CA" => Object::Real(o),
    })
}

// ---------------------------------------------------------------------------
// Fonts

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Face {
    Helvetica,
    HelveticaBold,
    TimesItalic,
}

impl Face {
    pub fn base_font(self) -> &'static str {
        match self {
            Face::Helvetica => "Helvetica",
            Face::HelveticaBold => "Helvetica-Bold",
            Face::TimesItalic => "Times-Italic",
        }
    }
}

// widths for ASCII 32..=126
const HELV: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556, 556, 556, 556, 556, 556,
    556, 556, 278, 278, 584, 584, 584, 556, 1015, 667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833, 722, 778,
    667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 278, 278, 278, 469, 556, 333, 556, 556, 500, 556, 556, 278, 556,
    556, 222, 222, 500, 222, 833, 556, 556, 556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584,
];
const HELV_B: [u16; 95] = [
    278, 333, 474, 556, 556, 889, 722, 238, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556, 556, 556, 556, 556, 556,
    556, 556, 333, 333, 584, 584, 584, 611, 975, 722, 722, 722, 722, 667, 611, 778, 722, 278, 556, 722, 611, 833, 722, 778,
    667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 333, 278, 333, 584, 556, 333, 556, 611, 556, 611, 556, 333, 611,
    611, 278, 278, 556, 278, 889, 611, 611, 611, 611, 389, 556, 333, 611, 556, 778, 556, 556, 500, 389, 280, 389, 584,
];
const TIMES_I: [u16; 95] = [
    250, 333, 420, 500, 500, 833, 778, 214, 333, 333, 500, 675, 250, 333, 250, 278, 500, 500, 500, 500, 500, 500, 500, 500,
    500, 500, 333, 333, 675, 675, 675, 500, 920, 611, 611, 667, 722, 611, 611, 722, 722, 333, 444, 667, 556, 833, 667, 722,
    611, 722, 611, 500, 556, 722, 611, 833, 611, 556, 556, 389, 278, 389, 422, 500, 333, 500, 500, 444, 500, 444, 278, 500,
    500, 278, 278, 444, 278, 722, 500, 500, 500, 500, 389, 389, 278, 500, 444, 667, 444, 444, 389, 400, 275, 400, 541,
];

/// Chars -> WinAnsi bytes (unknown chars become '?').
pub(crate) fn winansi(text: &str) -> Vec<u8> {
    text.chars()
        .map(|c| match c {
            '\u{20}'..='\u{7e}' | '\u{a0}'..='\u{ff}' => c as u32 as u8,
            '€' => 0x80,
            '…' => 0x85,
            '‘' => 0x91,
            '’' => 0x92,
            '“' => 0x93,
            '”' => 0x94,
            '•' => 0x95,
            '–' => 0x96,
            '—' => 0x97,
            '™' => 0x99,
            _ => b'?',
        })
        .collect()
}

/// Width of `text` in points.
pub(crate) fn text_width(text: &str, face: Face, size: f64) -> f64 {
    let table = match face {
        Face::Helvetica => &HELV,
        Face::HelveticaBold => &HELV_B,
        Face::TimesItalic => &TIMES_I,
    };
    let fallback = if face == Face::TimesItalic { 500 } else { 556 };
    let total: u32 = winansi(text)
        .iter()
        .map(|&b| if (32..=126).contains(&b) { table[b as usize - 32] as u32 } else { fallback })
        .sum();
    total as f64 * size / 1000.0
}

/// PDF literal string for WinAnsi text, as content-stream bytes.
pub(crate) fn pdf_string(text: &str) -> Vec<u8> {
    let mut out = vec![b'('];
    for b in winansi(text) {
        match b {
            b'(' | b')' | b'\\' => {
                out.push(b'\\');
                out.push(b);
            }
            0x20..=0x7e => out.push(b),
            _ => out.extend(format!("\\{:03o}", b).bytes()),
        }
    }
    out.push(b')');
    out
}
