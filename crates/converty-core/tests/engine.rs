use converty_core::*;
use lopdf::content::{Content, Operation};
use lopdf::{dictionary, Document, Object, Stream};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

/// Builds a PDF with `n` pages; page i shows "Page i" and has a different width.
fn make_pdf(path: &Path, n: u32) {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let font = doc.add_object(dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica" });
    let res = doc.add_object(dictionary! { "Font" => dictionary! { "F1" => font } });
    let mut kids = vec![];
    for i in 1..=n {
        let content = Content {
            operations: vec![
                Operation::new("BT", vec![]),
                Operation::new("Tf", vec!["F1".into(), 24.into()]),
                Operation::new("Td", vec![50.into(), 700.into()]),
                Operation::new("Tj", vec![Object::string_literal(format!("Page {i}"))]),
                Operation::new("ET", vec![]),
            ],
        };
        let cid = doc.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
        let pid = doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages_id, "Contents" => cid, "Resources" => res,
            "MediaBox" => vec![0.into(), 0.into(), (500 + i as i64).into(), 800.into()],
        });
        kids.push(Object::Reference(pid));
    }
    doc.objects.insert(pages_id, Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => n as i64 }));
    let cat = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", cat);
    doc.save(path).unwrap();
}

fn job(tool: Tool, inputs: &[&Path], options: ToolOptions, dir: &Path, stem: &str) -> Result<JobResult> {
    let req = JobRequest {
        tool,
        inputs: inputs.iter().map(|p| Input { path: p.to_path_buf(), password: None }).collect(),
        options,
        output_dir: dir.to_path_buf(),
        output_stem: stem.into(),
    };
    run(&req, &|_, _| {}, &AtomicBool::new(false))
}

fn pages(p: &Path) -> usize {
    Document::load(p).unwrap().get_pages().len()
}

fn setup(n: u32) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join("in.pdf");
    make_pdf(&f, n);
    (dir, f)
}

fn out_dir(dir: &tempfile::TempDir) -> PathBuf {
    let o = dir.path().join("out");
    std::fs::create_dir_all(&o).unwrap();
    o
}

#[test]
fn ranges() {
    assert_eq!(parse_ranges("1,3-5,8-", 9).unwrap(), vec![1, 3, 4, 5, 8, 9]);
    assert_eq!(parse_ranges("", 3).unwrap(), vec![1, 2, 3]);
    assert_eq!(parse_ranges("-2", 5).unwrap(), vec![1, 2]);
    assert_eq!(parse_ranges("3, 1 ,3,2-3", 5).unwrap(), vec![3, 1, 2]);
    assert!(matches!(parse_ranges("9", 5), Err(Error::InvalidRange(_))));
    assert!(matches!(parse_ranges("0", 5), Err(Error::InvalidRange(_))));
    assert!(matches!(parse_ranges("a-b", 5), Err(Error::InvalidRange(_))));
    assert!(matches!(parse_ranges("4-2", 5), Err(Error::InvalidRange(_))));
}

#[test]
fn file_kinds_and_inspect() {
    assert_eq!(FileKind::from_path(Path::new("A.PDF")), FileKind::Pdf);
    assert_eq!(FileKind::from_path(Path::new("a.JPeG")), FileKind::Image);
    assert_eq!(FileKind::from_path(Path::new("a.docx")), FileKind::Office);
    assert_eq!(FileKind::from_path(Path::new("a.htm")), FileKind::Html);
    assert_eq!(FileKind::from_path(Path::new("a.zip")), FileKind::Other);
    let (_d, f) = setup(3);
    let info = inspect(&f, None).unwrap();
    assert_eq!(info.pages, Some(3));
    assert!(!info.encrypted);
    assert_eq!(info.first_page_size, Some((501.0, 800.0)));
    let bad = _d.path().join("bad.pdf");
    std::fs::write(&bad, b"not a pdf").unwrap();
    assert!(matches!(inspect(&bad, None), Err(Error::Corrupt(_))));
}

#[test]
fn merge_counts() {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.pdf"), dir.path().join("b.pdf"));
    make_pdf(&a, 3);
    make_pdf(&b, 2);
    let out = out_dir(&dir);
    let r = job(Tool::Merge, &[&a, &b], ToolOptions::Merge, &out, "merged").unwrap();
    assert_eq!(r.outputs.len(), 1);
    assert_eq!(pages(&r.outputs[0]), 5);
    // never overwrite
    let r2 = job(Tool::Merge, &[&a, &b], ToolOptions::Merge, &out, "merged").unwrap();
    assert_eq!(r2.outputs[0].file_name().unwrap(), "merged (2).pdf");
    // content of page 4 is page 1 of b (width 501)
    let doc = Document::load(&r.outputs[0]).unwrap();
    let ids = doc.get_pages();
    let text = String::from_utf8_lossy(&doc.get_page_content(ids[&4]).unwrap()).to_string();
    assert!(text.contains("Page 1"), "{text}");
}

#[test]
fn split_modes() {
    let (d, f) = setup(7);
    let out = out_dir(&d);
    let r = job(Tool::Split, &[&f], ToolOptions::Split(SplitOptions::Ranges { ranges: vec!["1-3".into(), "4-".into()], merge_all: false }), &out, "s").unwrap();
    assert_eq!(r.outputs.len(), 2);
    assert_eq!((pages(&r.outputs[0]), pages(&r.outputs[1])), (3, 4));
    assert!(r.outputs[0].file_name().unwrap().to_string_lossy().starts_with("s_1"));
    let r = job(Tool::Split, &[&f], ToolOptions::Split(SplitOptions::Ranges { ranges: vec!["1-2".into(), "6".into()], merge_all: true }), &out, "m").unwrap();
    assert_eq!(r.outputs.len(), 1);
    assert_eq!(pages(&r.outputs[0]), 3);
    let r = job(Tool::Split, &[&f], ToolOptions::Split(SplitOptions::Fixed { every: 3 }), &out, "f").unwrap();
    assert_eq!(r.outputs.len(), 3);
    assert_eq!(pages(&r.outputs[2]), 1);
    let r = job(Tool::Split, &[&f], ToolOptions::Split(SplitOptions::EveryPage), &out, "e").unwrap();
    assert_eq!(r.outputs.len(), 7);
    assert!(job(Tool::Split, &[&f], ToolOptions::Split(SplitOptions::Ranges { ranges: vec!["9".into()], merge_all: false }), &out, "x").is_err());
}

#[test]
fn remove_extract_organize() {
    let (d, f) = setup(5);
    let out = out_dir(&d);
    let r = job(Tool::RemovePages, &[&f], ToolOptions::RemovePages { pages: "2,4-5".into() }, &out, "rm").unwrap();
    assert_eq!(pages(&r.outputs[0]), 2);
    assert!(job(Tool::RemovePages, &[&f], ToolOptions::RemovePages { pages: "1-5".into() }, &out, "rm2").is_err());
    let r = job(Tool::ExtractPages, &[&f], ToolOptions::ExtractPages { pages: "5,1".into(), separate_files: false }, &out, "ex").unwrap();
    assert_eq!(pages(&r.outputs[0]), 2);
    // order kept as written: first page is old page 5 (width 505)
    let info = inspect(&r.outputs[0], None).unwrap();
    assert_eq!(info.first_page_size, Some((505.0, 800.0)));
    let r = job(Tool::ExtractPages, &[&f], ToolOptions::ExtractPages { pages: "1-3".into(), separate_files: true }, &out, "sep").unwrap();
    assert_eq!(r.outputs.len(), 3);
    let ops = vec![
        PageOp::Page { source: 3, rotate: Rotation::R0 },
        PageOp::Blank,
        PageOp::Page { source: 1, rotate: Rotation::R90 },
        PageOp::Page { source: 3, rotate: Rotation::R0 },
    ];
    let r = job(Tool::Organize, &[&f], ToolOptions::Organize { pages: ops }, &out, "org").unwrap();
    assert_eq!(pages(&r.outputs[0]), 4);
    let doc = Document::load(&r.outputs[0]).unwrap();
    let ids: Vec<_> = doc.get_pages().into_values().collect();
    let blank = doc.get_dictionary(ids[1]).unwrap();
    let mb: Vec<f32> = blank.get(b"MediaBox").unwrap().as_array().unwrap().iter().map(|o| o.as_float().unwrap()).collect();
    assert_eq!(mb, vec![0.0, 0.0, 503.0, 800.0]);
    assert_eq!(doc.get_dictionary(ids[2]).unwrap().get(b"Rotate").unwrap().as_i64().unwrap(), 90);
}

#[test]
fn rotate_and_crop() {
    let (d, f) = setup(3);
    let out = out_dir(&d);
    let r = job(Tool::Rotate, &[&f], ToolOptions::Rotate { degrees: Rotation::R90, pages: "2".into() }, &out, "rot").unwrap();
    let doc = Document::load(&r.outputs[0]).unwrap();
    let ids: Vec<_> = doc.get_pages().into_values().collect();
    assert!(doc.get_dictionary(ids[0]).unwrap().get(b"Rotate").is_err());
    assert_eq!(doc.get_dictionary(ids[1]).unwrap().get(b"Rotate").unwrap().as_i64().unwrap(), 90);
    // rotating again by 270 returns to 0 (mod 360)
    let r2 = job(Tool::Rotate, &[&r.outputs[0]], ToolOptions::Rotate { degrees: Rotation::R270, pages: "".into() }, &out, "rot2").unwrap();
    let doc = Document::load(&r2.outputs[0]).unwrap();
    let ids: Vec<_> = doc.get_pages().into_values().collect();
    assert_eq!(doc.get_dictionary(ids[1]).unwrap().get(b"Rotate").unwrap().as_i64().unwrap(), 0);
    assert_eq!(doc.get_dictionary(ids[0]).unwrap().get(b"Rotate").unwrap().as_i64().unwrap(), 270);

    let r = job(Tool::Crop, &[&f], ToolOptions::Crop(CropOptions { top: 10.0, right: 20.0, bottom: 30.0, left: 40.0, pages: "".into() }), &out, "crop").unwrap();
    let info = inspect(&r.outputs[0], None).unwrap();
    assert_eq!(info.first_page_size, Some((501.0 - 60.0, 800.0 - 40.0)));
    assert!(job(Tool::Crop, &[&f], ToolOptions::Crop(CropOptions { top: 500.0, bottom: 400.0, ..Default::default() }), &out, "bad").is_err());
}

fn page_text(p: &Path, n: u32) -> String {
    let doc = Document::load(p).unwrap();
    let ids = doc.get_pages();
    String::from_utf8_lossy(&doc.get_page_content(ids[&n]).unwrap()).to_string()
}

fn png(path: &Path, w: u32, h: u32, alpha: bool) {
    let mut img = image::RgbaImage::new(w, h);
    for (x, y, p) in img.enumerate_pixels_mut() {
        *p = image::Rgba([(x * 255 / w) as u8, (y * 255 / h) as u8, 120, if alpha && x < w / 2 { 80 } else { 255 }]);
    }
    img.save(path).unwrap();
}

#[test]
fn stamps() {
    let (d, f) = setup(4);
    let out = out_dir(&d);
    let r = job(
        Tool::PageNumbers,
        &[&f],
        ToolOptions::PageNumbers(PageNumberOptions { template: "Seite {n} von {p} (x)".into(), first_number: 5, pages: "2-4".into(), ..Default::default() }),
        &out,
        "num",
    )
    .unwrap();
    assert_eq!(pages(&r.outputs[0]), 4);
    assert!(page_text(&r.outputs[0], 2).contains("(Seite 6 von 4 \\(x\\))"));
    assert!(!page_text(&r.outputs[0], 1).contains("Seite"));

    let r = job(Tool::Watermark, &[&f], ToolOptions::Watermark(WatermarkOptions::default()), &out, "wm").unwrap();
    assert_eq!(pages(&r.outputs[0]), 4);
    assert!(page_text(&r.outputs[0], 1).contains("CONFIDENTIAL"));
    let o = WatermarkOptions { mosaic: true, below_content: true, ..Default::default() };
    let r = job(Tool::Watermark, &[&f], ToolOptions::Watermark(o), &out, "wm2").unwrap();
    assert_eq!(pages(&r.outputs[0]), 4);
    assert!(page_text(&r.outputs[0], 1).matches("CONFIDENTIAL").count() > 4);

    let img = d.path().join("logo.png");
    png(&img, 40, 20, true);
    let o = WatermarkOptions { content: WatermarkContent::Image { path: img.clone(), scale: 0.3 }, ..Default::default() };
    let r = job(Tool::Watermark, &[&f], ToolOptions::Watermark(o), &out, "wm3").unwrap();
    assert_eq!(pages(&r.outputs[0]), 4);
    assert!(page_text(&r.outputs[0], 1).contains("CvIm1 Do"));

    let r = job(Tool::Sign, &[&f], ToolOptions::Sign(SignOptions { signature: SignatureKind::Typed { name: "Max Mustermann".into() }, ..Default::default() }), &out, "sig").unwrap();
    assert_eq!(pages(&r.outputs[0]), 4);
    let t = page_text(&r.outputs[0], 4);
    assert!(t.contains("(Max Mustermann)"));
    let r = job(Tool::Sign, &[&f], ToolOptions::Sign(SignOptions { signature: SignatureKind::Image { path: img }, page: 1, position: Position::TopLeft, ..Default::default() }), &out, "sig2").unwrap();
    assert!(page_text(&r.outputs[0], 1).contains("CvIm1 Do"));
    // a rotated page still stamps
    let rr = job(Tool::Rotate, &[&f], ToolOptions::Rotate { degrees: Rotation::R90, pages: "".into() }, &out, "r").unwrap();
    let r = job(Tool::PageNumbers, &[&rr.outputs[0]], ToolOptions::PageNumbers(PageNumberOptions::default()), &out, "rn").unwrap();
    assert_eq!(pages(&r.outputs[0]), 4);
}

#[test]
fn images_to_pdf_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let out = out_dir(&dir);
    let (a, b, c) = (dir.path().join("a.png"), dir.path().join("b.png"), dir.path().join("c.jpg"));
    png(&a, 64, 32, true);
    png(&b, 20, 40, false);
    image::open(&b).unwrap().to_rgb8().save(&c).unwrap();
    let r = job(Tool::ImagesToPdf, &[&a, &b, &c], ToolOptions::ImagesToPdf(ImagesToPdfOptions::default()), &out, "img").unwrap();
    assert_eq!(r.outputs.len(), 1);
    assert_eq!(pages(&r.outputs[0]), 3);
    let doc = Document::load(&r.outputs[0]).unwrap();
    // JPEG embedded as DCT, alpha PNG has an SMask
    let mut dct = 0;
    let mut smask = 0;
    for o in doc.objects.values() {
        if let Ok(s) = o.as_stream() {
            if s.dict.get(b"Filter").ok().and_then(|f| f.as_name().ok()) == Some(b"DCTDecode") {
                dct += 1;
            }
            if s.dict.has(b"SMask") {
                smask += 1;
            }
        }
    }
    assert_eq!((dct, smask), (1, 1));
    // landscape A4 for the wide image in Auto
    let first = doc.get_pages()[&1];
    let mb = doc.get_dictionary(first).unwrap().get(b"MediaBox").unwrap().as_array().unwrap().clone();
    assert!(mb[2].as_float().unwrap() > mb[3].as_float().unwrap());
    let r = job(Tool::ImagesToPdf, &[&a, &b], ToolOptions::ImagesToPdf(ImagesToPdfOptions { merge: false, page_size: PageSize::Fit, margin: Margin::Small, ..Default::default() }), &out, "sep").unwrap();
    assert_eq!(r.outputs.len(), 2);
    assert!(matches!(job(Tool::ImagesToPdf, &[&f_pdf(&dir)], ToolOptions::ImagesToPdf(ImagesToPdfOptions::default()), &out, "x"), Err(Error::WrongType(_))));
}

fn f_pdf(d: &tempfile::TempDir) -> PathBuf {
    let p = d.path().join("w.pdf");
    make_pdf(&p, 1);
    p
}

#[test]
fn cancel_and_wrong_options() {
    let (d, f) = setup(3);
    let out = out_dir(&d);
    let req = JobRequest {
        tool: Tool::Merge,
        inputs: vec![Input { path: f.clone(), password: None }, Input { path: f.clone(), password: None }],
        options: ToolOptions::Merge,
        output_dir: out.clone(),
        output_stem: "x".into(),
    };
    let cancel = AtomicBool::new(true);
    assert!(matches!(run(&req, &|_, _| {}, &cancel), Err(Error::Cancelled)));
    assert!(std::fs::read_dir(&out).unwrap().next().is_none());
    assert!(matches!(job(Tool::Merge, &[&f], ToolOptions::Merge, &out, "x"), Err(Error::InvalidOptions(_))));
    assert!(matches!(job(Tool::Split, &[&f], ToolOptions::Merge, &out, "x"), Err(Error::InvalidOptions(_))));
}

#[test]
fn render_and_images_via_poppler() {
    if !deps().has("pdftoppm") {
        return;
    }
    let (d, f) = setup(3);
    let (w, h, px) = render_page(&f, 2, 200, None).unwrap();
    assert_eq!(h, 200);
    assert!(w < 200 && w > 100);
    assert_eq!(px.len(), (w * h * 4) as usize);
    assert!(render_page(&f, 9, 100, None).is_err());
    let img = d.path().join("i.png");
    png(&img, 300, 100, false);
    let (w, h, _) = render_page(&img, 1, 150, None).unwrap();
    assert_eq!((w, h), (150, 50));
    let out = out_dir(&d);
    let r = job(Tool::PdfToImages, &[&f], ToolOptions::PdfToImages(PdfToImagesOptions { pages: "1-2".into(), dpi: 72, ..Default::default() }), &out, "pg").unwrap();
    assert_eq!(r.outputs.len(), 2);
    assert!(r.outputs[0].extension().unwrap() == "jpg");
    let r = job(Tool::PdfToImages, &[&f], ToolOptions::PdfToImages(PdfToImagesOptions { format: ImageFormat::Png, dpi: 72, pages: "3".into(), ..Default::default() }), &out, "one").unwrap();
    assert_eq!(r.outputs[0].file_name().unwrap(), "one.png");
    // embedded images
    if deps().has("pdfimages") {
        let imgpdf = d.path().join("imgs.pdf");
        let a = d.path().join("a.jpg");
        image::open(&img).unwrap().to_rgb8().save(&a).unwrap();
        job(Tool::ImagesToPdf, &[&a], ToolOptions::ImagesToPdf(ImagesToPdfOptions::default()), d.path(), "imgs").unwrap();
        assert!(imgpdf.exists());
        let r = job(Tool::PdfToImages, &[&imgpdf], ToolOptions::PdfToImages(PdfToImagesOptions { render_pages: false, ..Default::default() }), &out, "ex").unwrap();
        assert_eq!(r.outputs.len(), 1);
        assert!(job(Tool::PdfToImages, &[&f], ToolOptions::PdfToImages(PdfToImagesOptions { render_pages: false, ..Default::default() }), &out, "none").is_err());
    }
}

#[test]
fn gs_compress_protect_unlock() {
    if !deps().has("gs") {
        return;
    }
    let (d, f) = setup(3);
    let out = out_dir(&d);
    let r = job(Tool::Compress, &[&f], ToolOptions::Compress { level: CompressLevel::Recommended }, &out, "c").unwrap();
    assert_eq!(r.outputs.len(), 1);
    assert_eq!(pages(&r.outputs[0]), 3);

    let opts = ProtectOptions { user_password: "secret".into(), owner_password: "owner".into(), ..Default::default() };
    let r = job(Tool::Protect, &[&f], ToolOptions::Protect(opts), &out, "p").unwrap();
    let locked = r.outputs[0].clone();
    let info = inspect(&locked, None).unwrap();
    assert!(info.encrypted);
    assert_eq!(info.pages, None);
    assert!(matches!(inspect(&locked, Some("nope")), Err(Error::WrongPassword(_))));
    let info = inspect(&locked, Some("secret")).unwrap();
    assert_eq!(info.pages, Some(3));

    // other tools read it with a password
    let req = JobRequest {
        tool: Tool::Rotate,
        inputs: vec![Input { path: locked.clone(), password: Some("secret".into()) }],
        options: ToolOptions::Rotate { degrees: Rotation::R90, pages: "".into() },
        output_dir: out.clone(),
        output_stem: "rl".into(),
    };
    let r = run(&req, &|_, _| {}, &AtomicBool::new(false)).unwrap();
    assert_eq!(pages(&r.outputs[0]), 3);
    assert!(!inspect(&r.outputs[0], None).unwrap().encrypted);
    assert!(matches!(job(Tool::Unlock, &[&locked], ToolOptions::Unlock, &out, "u0"), Err(Error::Encrypted(_))));

    let req = JobRequest {
        tool: Tool::Unlock,
        inputs: vec![Input { path: locked.clone(), password: Some("bad".into()) }],
        options: ToolOptions::Unlock,
        output_dir: out.clone(),
        output_stem: "u".into(),
    };
    assert!(matches!(run(&req, &|_, _| {}, &AtomicBool::new(false)), Err(Error::WrongPassword(_))));
    let req = JobRequest { inputs: vec![Input { path: locked, password: Some("secret".into()) }], ..req };
    let r = run(&req, &|_, _| {}, &AtomicBool::new(false)).unwrap();
    let info = inspect(&r.outputs[0], None).unwrap();
    assert!(!info.encrypted);
    assert_eq!(info.pages, Some(3));

    // repair + PDF/A
    let r = job(Tool::Repair, &[&f], ToolOptions::Repair, &out, "rep").unwrap();
    assert_eq!(pages(&r.outputs[0]), 3);
    let r = job(Tool::PdfToPdfA, &[&f], ToolOptions::PdfToPdfA { level: PdfALevel::B2 }, &out, "a").unwrap();
    assert_eq!(pages(&r.outputs[0]), 3);
}

#[test]
fn repair_damaged() {
    if !deps().has("gs") {
        return;
    }
    let (d, f) = setup(2);
    let out = out_dir(&d);
    let mut bytes = std::fs::read(&f).unwrap();
    // chop off xref/trailer
    let cut = bytes.len() - 120;
    bytes.truncate(cut);
    let bad = d.path().join("bad.pdf");
    std::fs::write(&bad, bytes).unwrap();
    let r = job(Tool::Repair, &[&bad], ToolOptions::Repair, &out, "fixed");
    if let Ok(r) = r {
        assert_eq!(pages(&r.outputs[0]), 2);
    }
}

#[test]
fn soffice_conversions() {
    if !deps().has("soffice") || std::env::var("CONVERTY_TEST_SOFFICE").is_err() {
        return; // slow: opt in with CONVERTY_TEST_SOFFICE=1
    }
    let dir = tempfile::tempdir().unwrap();
    let out = out_dir(&dir);
    let html = dir.path().join("p.html");
    std::fs::write(&html, "<html><body><h1>Hello</h1><p>World</p></body></html>").unwrap();
    let r = job(Tool::HtmlToPdf, &[&html], ToolOptions::HtmlToPdf, &out, "h").unwrap();
    assert!(pages(&r.outputs[0]) >= 1);
    let txt = dir.path().join("t.csv");
    std::fs::write(&txt, "a,b\n1,2\n").unwrap();
    let r = job(Tool::OfficeToPdf, &[&txt], ToolOptions::OfficeToPdf, &out, "o").unwrap();
    assert!(pages(&r.outputs[0]) >= 1);
    let r2 = job(Tool::PdfToOffice, &[&r.outputs[0]], ToolOptions::PdfToOffice { target: OfficeTarget::Docx }, &out, "d").unwrap();
    assert!(r2.outputs[0].extension().unwrap() == "docx");
    for t in [OfficeTarget::Xlsx, OfficeTarget::Pptx, OfficeTarget::Odt] {
        job(Tool::PdfToOffice, &[&r.outputs[0]], ToolOptions::PdfToOffice { target: t }, &out, "t").unwrap();
    }
}
