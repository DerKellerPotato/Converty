//! End-to-end tests of the engine, flow by flow (F01–F15 in replica/test-plan.md).
//!
//! Inputs are built once per test run in `target/tmp`: a LibreOffice-made multi-page PDF
//! with text and an image, an object-stream PDF written by Ghostscript, an encrypted PDF made
//! by the Protect tool, a truncated PDF, PNG/JPG images and a 200-page PDF. Tests that need
//! gs / soffice / pdftoppm skip themselves when the program is missing.

use converty_core::*;
use lopdf::content::{Content, Operation};
use lopdf::{dictionary, Document, Object, Stream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

const PW: &str = "s3cret pw \"quoted\" \\ ü€";

// ---------------------------------------------------------------- fixtures

/// `n` pages; page i shows "Page i" and is (500 + i) points wide.
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

struct Fx {
    /// 7 plain pages (lopdf)
    plain: PathBuf,
    /// 200 pages (lopdf)
    big: PathBuf,
    png: PathBuf,
    jpg: PathBuf,
    /// 4 pages, text + image, made by LibreOffice
    realistic: Option<PathBuf>,
    /// 12 pages in an object-stream PDF written by gs
    objstm: Option<PathBuf>,
    /// 5 pages, user password PW
    enc: Option<PathBuf>,
    /// plain.pdf cut off in the middle
    corrupt: PathBuf,
    /// plain.pdf without its last 100 bytes
    tail_cut: PathBuf,
    html: PathBuf,
    odt: Option<PathBuf>,
    docx: Option<PathBuf>,
}

fn soffice(profile: &Path, args: &[&str], cwd: &Path) {
    let st = Command::new("soffice")
        .arg(format!("-env:UserInstallation=file://{}", profile.display()))
        .args(["--headless", "--norestore"])
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("run soffice");
    assert!(st.status.success(), "soffice failed: {}", String::from_utf8_lossy(&st.stderr));
}

fn fx() -> &'static Fx {
    static FX: OnceLock<Fx> = OnceLock::new();
    FX.get_or_init(|| {
        let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("e2e-fixtures");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let has = deps();

        let plain = dir.join("plain.pdf");
        make_pdf(&plain, 7);
        let big = dir.join("big200.pdf");
        make_pdf(&big, 200);

        // images through the image crate
        let png = dir.join("pic.png");
        let jpg = dir.join("pic.jpg");
        let img = image::RgbImage::from_fn(300, 200, |x, y| image::Rgb([(x * 255 / 300) as u8, (y * 255 / 200) as u8, 128]));
        img.save(&png).unwrap();
        img.save(&jpg).unwrap();

        // HTML with text and an image, one section per page
        let html = dir.join("doc.html");
        let body: String = (1..=4)
            .map(|i| {
                format!(
                    "<h1>Section {i}</h1><p>{}</p><p style=\"page-break-after:always\"><img src=\"pic.png\" width=300></p>",
                    "Lorem ipsum dolor sit amet. ".repeat(60)
                )
            })
            .collect();
        std::fs::write(&html, format!("<html><body>{body}</body></html>")).unwrap();

        let (mut realistic, mut odt, mut docx) = (None, None, None);
        if has.has("soffice") {
            let profile = dir.join("lo-profile");
            soffice(&profile, &["--convert-to", "pdf", "--outdir", dir.to_str().unwrap(), "doc.html"], &dir);
            let r = dir.join("realistic.pdf");
            std::fs::rename(dir.join("doc.pdf"), &r).unwrap();
            realistic = Some(r);
            soffice(&profile, &["--convert-to", "odt:writer8", "--outdir", dir.to_str().unwrap(), "doc.html"], &dir);
            odt = Some(dir.join("doc.odt"));
            soffice(&profile, &["--convert-to", "docx:MS Word 2007 XML", "--outdir", dir.to_str().unwrap(), "doc.html"], &dir);
            docx = Some(dir.join("doc.docx"));
        }

        // Object streams + cross-reference stream (PDF 1.5). gs 10.02's pdfwrite cannot write
        // them, so lopdf's `save_modern` produces this fixture.
        let objstm = {
            let src = dir.join("twelve.pdf");
            make_pdf(&src, 12);
            let out = dir.join("objstm.pdf");
            let mut doc = Document::load(&src).unwrap();
            doc.save_modern(&mut std::fs::File::create(&out).unwrap()).unwrap();
            let bytes = std::fs::read(&out).unwrap();
            assert!(bytes.windows(7).any(|w| w == b"/ObjStm"), "no object stream written");
            Some(out)
        };

        let mut enc = None;
        if has.has("gs") {
            let src = dir.join("five.pdf");
            make_pdf(&src, 5);
            let req = JobRequest {
                tool: Tool::Protect,
                inputs: vec![Input { path: src, password: None }],
                options: ToolOptions::Protect(ProtectOptions { user_password: PW.into(), owner_password: "owner-pw".into(), ..Default::default() }),
                output_dir: dir.clone(),
                output_stem: "enc".into(),
            };
            let r = run(&req, &|_, _| {}, &AtomicBool::new(false)).expect("protect fixture");
            enc = Some(r.outputs[0].clone());
        }

        let bytes = std::fs::read(&plain).unwrap();
        // cut off before the end: trailer, xref and (for lopdf output) the page tree are gone
        let corrupt = dir.join("corrupt.pdf");
        std::fs::write(&corrupt, &bytes[..bytes.len() * 4 / 10]).unwrap();
        // only the last ~100 bytes (trailer and startxref) are missing
        let tail_cut = dir.join("tailcut.pdf");
        std::fs::write(&tail_cut, &bytes[..bytes.len() - 100]).unwrap();

        Fx { plain, big, png, jpg, realistic, objstm, enc, corrupt, tail_cut, html, odt, docx }
    })
}

// ---------------------------------------------------------------- helpers

fn inp(p: &Path) -> Input {
    Input { path: p.to_path_buf(), password: None }
}

fn inp_pw(p: &Path, pw: &str) -> Input {
    Input { path: p.to_path_buf(), password: Some(pw.into()) }
}

fn job_in(tool: Tool, inputs: Vec<Input>, options: ToolOptions, dir: &Path, stem: &str) -> Result<JobResult> {
    run(
        &JobRequest { tool, inputs, options, output_dir: dir.to_path_buf(), output_stem: stem.into() },
        &|_, _| {},
        &AtomicBool::new(false),
    )
}

fn job(tool: Tool, inputs: &[&Path], options: ToolOptions, dir: &Path, stem: &str) -> Result<JobResult> {
    job_in(tool, inputs.iter().map(|p| inp(p)).collect(), options, dir, stem)
}

fn out() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

fn pages(p: &Path) -> usize {
    Document::load(p).unwrap().get_pages().len()
}

fn ls(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    v.sort();
    v
}

fn assert_empty(dir: &Path) {
    assert_eq!(ls(dir), Vec::<String>::new(), "output folder should be empty");
}

fn page_text(p: &Path, n: u32) -> String {
    let doc = Document::load(p).unwrap();
    let ids = doc.get_pages();
    String::from_utf8_lossy(&doc.get_page_content(ids[&n]).unwrap()).to_string()
}

fn has(p: &str) -> bool {
    deps().has(p)
}

/// The output opens in lopdf with `n` pages; with poppler also renders its last page.
fn assert_valid(p: &Path, n: usize) {
    assert_eq!(pages(p), n, "{}", p.display());
    if has("pdftoppm") {
        render_page(p, n as u32, 80, None).unwrap_or_else(|e| panic!("render {}: {e}", p.display()));
    }
}

fn all_sources() -> Vec<(&'static str, PathBuf, usize)> {
    let f = fx();
    let mut v = vec![("plain", f.plain.clone(), 7)];
    if let Some(p) = &f.realistic {
        v.push(("realistic", p.clone(), 4));
    }
    if let Some(p) = &f.objstm {
        v.push(("objstm", p.clone(), 12));
    }
    v
}

// ---------------------------------------------------------------- fixtures sanity

#[test]
fn fixtures_are_what_we_say() {
    let f = fx();
    assert_eq!(inspect(&f.plain, None).unwrap().pages, Some(7));
    assert_eq!(inspect(&f.big, None).unwrap().pages, Some(200));
    if let Some(p) = &f.realistic {
        assert_eq!(inspect(p, None).unwrap().pages, Some(4));
        // really has an image
        if has("pdfimages") {
            let o = Command::new("pdfimages").arg("-list").arg(p).output().unwrap();
            assert!(String::from_utf8_lossy(&o.stdout).lines().count() > 2, "no image in the LibreOffice PDF");
        }
    }
    if let Some(p) = &f.objstm {
        assert_eq!(inspect(p, None).unwrap().pages, Some(12));
    }
    if let Some(p) = &f.enc {
        let i = inspect(p, None).unwrap();
        assert!(i.encrypted && i.pages.is_none());
    }
    assert_eq!(image::image_dimensions(&f.png).unwrap(), (300, 200));
    assert_eq!(image::image_dimensions(&f.jpg).unwrap(), (300, 200));
}

// ---------------------------------------------------------------- F01 merge

#[test]
fn f01_merge_mixed_sources_and_collisions() {
    let o = out();
    let srcs = all_sources();
    let total: usize = srcs.iter().map(|s| s.2).sum();
    let paths: Vec<&Path> = srcs.iter().map(|s| s.1.as_path()).collect();
    let r = job(Tool::Merge, &paths, ToolOptions::Merge, o.path(), "merged").unwrap();
    assert_valid(&r.outputs[0], total);
    // second and third run never overwrite
    let r2 = job(Tool::Merge, &paths, ToolOptions::Merge, o.path(), "merged").unwrap();
    let r3 = job(Tool::Merge, &paths, ToolOptions::Merge, o.path(), "merged").unwrap();
    assert_eq!(r2.outputs[0].file_name().unwrap(), "merged (2).pdf");
    assert_eq!(r3.outputs[0].file_name().unwrap(), "merged (3).pdf");
    // a stem that already carries a counter
    let r4 = job(Tool::Merge, &paths, ToolOptions::Merge, o.path(), "merged (2)").unwrap();
    assert_eq!(r4.outputs[0].file_name().unwrap(), "merged (2) (2).pdf");
    assert_eq!(ls(o.path()).len(), 4);
    // the first file is untouched by later runs
    assert_valid(&r.outputs[0], total);
}

#[test]
fn f01_merge_same_file_twice_and_order() {
    let o = out();
    let f = fx();
    let r = job(Tool::Merge, &[&f.plain, &f.plain], ToolOptions::Merge, o.path(), "dup").unwrap();
    assert_valid(&r.outputs[0], 14);
    if let Some(obj) = &f.objstm {
        let r = job(Tool::Merge, &[obj, &f.plain], ToolOptions::Merge, o.path(), "ord").unwrap();
        // first page of the second file (width 501) comes after the 12 pages of the first
        assert!(page_text(&r.outputs[0], 13).contains("Page 1"));
        assert!(page_text(&r.outputs[0], 1).contains("Page 1"));
        assert!(page_text(&r.outputs[0], 12).contains("Page 12"));
    }
}

#[test]
fn f01_merge_needs_two_files_and_pdfs_only() {
    let o = out();
    let f = fx();
    assert!(matches!(job(Tool::Merge, &[&f.plain], ToolOptions::Merge, o.path(), "x"), Err(Error::InvalidOptions(_))));
    assert!(matches!(job(Tool::Merge, &[], ToolOptions::Merge, o.path(), "x"), Err(Error::InvalidOptions(_))));
    assert!(matches!(job(Tool::Merge, &[&f.plain, &f.png], ToolOptions::Merge, o.path(), "x"), Err(Error::WrongType(_))));
    let missing = o.path().join("nope.pdf");
    assert!(matches!(job(Tool::Merge, &[&f.plain, &missing], ToolOptions::Merge, o.path(), "x"), Err(Error::Io(_))));
    assert_empty(o.path());
}

#[test]
fn f01_merge_encrypted_without_password_wrong_password_and_right_password() {
    if !has("gs") {
        return;
    }
    let o = out();
    let f = fx();
    let enc = f.enc.as_ref().unwrap();
    match job(Tool::Merge, &[&f.plain, enc], ToolOptions::Merge, o.path(), "m") {
        Err(Error::Encrypted(p)) => assert_eq!(&p, enc),
        other => panic!("expected Encrypted, got {other:?}"),
    }
    match job_in(Tool::Merge, vec![inp(&f.plain), inp_pw(enc, "wrong")], ToolOptions::Merge, o.path(), "m") {
        Err(Error::WrongPassword(p)) => assert_eq!(&p, enc),
        other => panic!("expected WrongPassword, got {other:?}"),
    }
    // empty password string counts as none
    assert!(matches!(
        job_in(Tool::Merge, vec![inp(&f.plain), inp_pw(enc, "")], ToolOptions::Merge, o.path(), "m"),
        Err(Error::Encrypted(_))
    ));
    assert_empty(o.path());
    let r = job_in(Tool::Merge, vec![inp(&f.plain), inp_pw(enc, PW)], ToolOptions::Merge, o.path(), "m").unwrap();
    assert_valid(&r.outputs[0], 12);
    // the merged result is no longer encrypted
    assert!(!inspect(&r.outputs[0], None).unwrap().encrypted);
    // the owner password also opens the file
    let r = job_in(Tool::Merge, vec![inp(&f.plain), inp_pw(enc, "owner-pw")], ToolOptions::Merge, o.path(), "m2").unwrap();
    assert_valid(&r.outputs[0], 12);
}

#[test]
fn f01_merge_with_corrupt_input_fails_cleanly() {
    let o = out();
    let f = fx();
    match job(Tool::Merge, &[&f.plain, &f.corrupt], ToolOptions::Merge, o.path(), "m") {
        Err(Error::Corrupt(p)) => assert_eq!(p, f.corrupt),
        other => panic!("expected Corrupt, got {other:?}"),
    }
    assert_empty(o.path());
}

#[test]
fn f01_owner_only_protection_opens_without_password() {
    if !has("gs") {
        return;
    }
    let o = out();
    let f = fx();
    let r = job(
        Tool::Protect,
        &[&f.plain],
        ToolOptions::Protect(ProtectOptions {
            user_password: String::new(),
            owner_password: "own".into(),
            permissions: Permissions { allow_print: false, ..Default::default() },
        }),
        o.path(),
        "ownerlocked",
    )
    .unwrap();
    let i = inspect(&r.outputs[0], None).unwrap();
    assert!(i.encrypted);
    assert_eq!(i.pages, Some(7));
    let m = job(Tool::Merge, &[&r.outputs[0], &f.plain], ToolOptions::Merge, o.path(), "merged").unwrap();
    assert_valid(&m.outputs[0], 14);
}

// ---------------------------------------------------------------- F02 split, ranges

fn split_ranges(r: &[&str], merge_all: bool) -> ToolOptions {
    ToolOptions::Split(SplitOptions::Ranges { ranges: r.iter().map(|s| s.to_string()).collect(), merge_all })
}

#[test]
fn f02_split_range_edge_cases() {
    let o = out();
    let f = fx();
    for bad in ["8", "1-9", "0", "5-3", "x", "3-", "-", ","] {
        let r = job(Tool::Split, &[&f.plain], split_ranges(&[bad], false), o.path(), "s");
        if bad == "3-" {
            assert_eq!(pages(&r.unwrap().outputs[0]), 5, "open end");
            continue;
        }
        assert!(matches!(r, Err(Error::InvalidRange(_))), "{bad:?} should be rejected, got {r:?}");
    }
    assert!(ls(o.path()).iter().filter(|n| n.starts_with("s")).count() <= 1);
    let o = out();
    // empty ranges list / only blanks
    assert!(matches!(job(Tool::Split, &[&f.plain], split_ranges(&[], false), o.path(), "s"), Err(Error::InvalidRange(_))));
    assert!(matches!(job(Tool::Split, &[&f.plain], split_ranges(&["", "  "], false), o.path(), "s"), Err(Error::InvalidRange(_))));
    // one good range after a bad one: nothing written
    assert!(job(Tool::Split, &[&f.plain], split_ranges(&["1-2", "9"], false), o.path(), "s").is_err());
    assert_empty(o.path());
}

#[test]
fn f02_split_overlapping_and_merged_ranges() {
    let o = out();
    let f = fx();
    let r = job(Tool::Split, &[&f.plain], split_ranges(&["1-4", "3-6"], false), o.path(), "ov").unwrap();
    assert_eq!(r.outputs.len(), 2);
    assert_eq!((pages(&r.outputs[0]), pages(&r.outputs[1])), (4, 4));
    let r = job(Tool::Split, &[&f.plain], split_ranges(&["1-4", "3-6"], true), o.path(), "mg").unwrap();
    assert_eq!(r.outputs.len(), 1);
    assert_eq!(pages(&r.outputs[0]), 6, "overlap is not duplicated");
    // spaces, semicolons, whitespace inside ranges
    let r = job(Tool::Split, &[&f.plain], split_ranges(&[" 2 - 3 ; 7 "], false), o.path(), "sp").unwrap();
    assert_eq!(pages(&r.outputs[0]), 3);
}

#[test]
fn f02_split_fixed_and_every_page() {
    let o = out();
    let f = fx();
    let r = job(Tool::Split, &[&f.plain], ToolOptions::Split(SplitOptions::Fixed { every: 3 }), o.path(), "fx").unwrap();
    let counts: Vec<usize> = r.outputs.iter().map(|p| pages(p)).collect();
    assert_eq!(counts, vec![3, 3, 1]);
    assert_eq!(r.outputs[2].file_name().unwrap(), "fx_3.pdf");
    // larger than the document: one file with everything, named without a number
    let r = job(Tool::Split, &[&f.plain], ToolOptions::Split(SplitOptions::Fixed { every: 99 }), o.path(), "all").unwrap();
    assert_eq!(r.outputs.len(), 1);
    assert_eq!(r.outputs[0].file_name().unwrap(), "all.pdf");
    assert!(matches!(
        job(Tool::Split, &[&f.plain], ToolOptions::Split(SplitOptions::Fixed { every: 0 }), o.path(), "z"),
        Err(Error::InvalidOptions(_))
    ));
    // one-page document, every page
    let one = o.path().join("one.pdf");
    make_pdf(&one, 1);
    let r = job(Tool::Split, &[&one], ToolOptions::Split(SplitOptions::EveryPage), o.path(), "ep").unwrap();
    assert_eq!(r.outputs.len(), 1);
}

#[test]
fn f02_split_many_outputs_and_collisions() {
    let o = out();
    let f = fx();
    let t = Instant::now();
    let r = job(Tool::Split, &[&f.big], ToolOptions::Split(SplitOptions::EveryPage), o.path(), "p").unwrap();
    eprintln!("split 200 pages -> 200 files: {:?}", t.elapsed());
    assert_eq!(r.outputs.len(), 200);
    assert_eq!(ls(o.path()).len(), 200);
    assert!(o.path().join("p_200.pdf").is_file());
    assert!(t.elapsed() < Duration::from_secs(120));
    // second run: every file gets " (2)"
    let r = job(Tool::Split, &[&f.big], ToolOptions::Split(SplitOptions::Fixed { every: 100 }), o.path(), "p").unwrap();
    assert_eq!(r.outputs.len(), 2);
    assert_eq!(r.outputs[0].file_name().unwrap(), "p_1 (2).pdf");
    assert_eq!(pages(&r.outputs[1]), 100);
}

#[test]
fn f02_remove_and_extract_pages() {
    let o = out();
    for (name, src, n) in all_sources() {
        let n = n as u32;
        let r = job(Tool::RemovePages, &[&src], ToolOptions::RemovePages { pages: "1".into() }, o.path(), &format!("rm-{name}")).unwrap();
        assert_valid(&r.outputs[0], n as usize - 1);
        let r = job(Tool::ExtractPages, &[&src], ToolOptions::ExtractPages { pages: format!("{n},1"), separate_files: false }, o.path(), &format!("ex-{name}")).unwrap();
        assert_valid(&r.outputs[0], 2);
        // page order as written: last page first
        assert!(page_text(&r.outputs[0], 1).contains(&format!("Page {n}")) || name == "realistic");
    }
    let f = fx();
    let o = out();
    for (tool, opt) in [
        (Tool::RemovePages, ToolOptions::RemovePages { pages: "8".into() }),
        (Tool::RemovePages, ToolOptions::RemovePages { pages: "1-7".into() }),
        (Tool::RemovePages, ToolOptions::RemovePages { pages: "".into() }),
        (Tool::RemovePages, ToolOptions::RemovePages { pages: "6-2".into() }),
        (Tool::ExtractPages, ToolOptions::ExtractPages { pages: "100".into(), separate_files: true }),
        (Tool::ExtractPages, ToolOptions::ExtractPages { pages: "3-1".into(), separate_files: false }),
    ] {
        let r = job(tool, &[&f.plain], opt.clone(), o.path(), "bad");
        assert!(matches!(r, Err(Error::InvalidRange(_))), "{opt:?} -> {r:?}");
    }
    assert_empty(o.path());
    // separate files, one page selected: one file without a number
    let r = job(Tool::ExtractPages, &[&f.plain], ToolOptions::ExtractPages { pages: "2".into(), separate_files: true }, o.path(), "one").unwrap();
    assert_eq!(r.outputs.len(), 1);
    let r = job(Tool::ExtractPages, &[&f.plain], ToolOptions::ExtractPages { pages: "2-4".into(), separate_files: true }, o.path(), "sep").unwrap();
    assert_eq!(r.outputs.len(), 3);
    assert!(page_text(&r.outputs[2], 1).contains("Page 4"));
    // empty selection = all pages
    let r = job(Tool::ExtractPages, &[&f.plain], ToolOptions::ExtractPages { pages: "".into(), separate_files: false }, o.path(), "all").unwrap();
    assert_eq!(pages(&r.outputs[0]), 7);
}

// ---------------------------------------------------------------- F03 compress

#[test]
fn f03_compress_levels_on_realistic_pdf() {
    if !has("gs") {
        return;
    }
    let o = out();
    let f = fx();
    let src = f.realistic.as_ref().unwrap_or(&f.plain);
    let n = pages(src);
    for (lvl, name) in [(CompressLevel::Extreme, "x"), (CompressLevel::Recommended, "r"), (CompressLevel::Low, "l")] {
        let r = job(Tool::Compress, &[src], ToolOptions::Compress { level: lvl }, o.path(), name).unwrap();
        assert_valid(&r.outputs[0], n);
        assert!(r.output_bytes > 0 && r.input_bytes > 0);
    }
}

#[test]
fn f03_compress_that_grows_keeps_original() {
    if !has("gs") {
        return;
    }
    let o = out();
    let tiny = o.path().join("tiny.pdf");
    make_pdf(&tiny, 1);
    let dest = out();
    let r = job(Tool::Compress, &[&tiny], ToolOptions::Compress { level: CompressLevel::Low }, dest.path(), "t").unwrap();
    assert_eq!(r.outputs.len(), 1);
    assert!(r.notes.iter().any(|n| n.contains("not smaller")), "notes: {:?}", r.notes);
    assert_eq!(std::fs::read(&r.outputs[0]).unwrap(), std::fs::read(&tiny).unwrap(), "the original bytes are the result");
    assert_eq!(r.input_bytes, r.output_bytes);
}

#[test]
fn f03_compress_multi_input_encrypted_and_corrupt() {
    if !has("gs") {
        return;
    }
    let o = out();
    let f = fx();
    let enc = f.enc.as_ref().unwrap();
    // several files -> numbered outputs
    let r = job(Tool::Compress, &[&f.plain, f.objstm.as_ref().unwrap()], ToolOptions::Compress { level: CompressLevel::Recommended }, o.path(), "c").unwrap();
    assert_eq!(r.outputs.len(), 2);
    assert!(r.outputs[1].file_name().unwrap().to_string_lossy().starts_with("c_2"));
    let o = out();
    assert!(matches!(
        job(Tool::Compress, &[enc], ToolOptions::Compress { level: CompressLevel::Extreme }, o.path(), "c"),
        Err(Error::Encrypted(_))
    ));
    assert!(matches!(
        job_in(Tool::Compress, vec![inp_pw(enc, "nope")], ToolOptions::Compress { level: CompressLevel::Extreme }, o.path(), "c"),
        Err(Error::WrongPassword(_))
    ));
    let r = job_in(Tool::Compress, vec![inp_pw(enc, PW)], ToolOptions::Compress { level: CompressLevel::Low }, o.path(), "c").unwrap();
    // tiny file: gs output is bigger, so the original (still locked with the same password) is kept
    let kept = r.notes.iter().any(|n| n.contains("not smaller"));
    if kept {
        assert_eq!(inspect(&r.outputs[0], Some(PW)).unwrap().pages, Some(5));
    } else {
        assert_valid(&r.outputs[0], 5);
    }
    // a damaged file fails and a good first file leaves nothing behind
    let o = out();
    assert!(job(Tool::Compress, &[&f.plain, &f.corrupt], ToolOptions::Compress { level: CompressLevel::Low }, o.path(), "c").is_err());
    assert_empty(o.path());
}

// ---------------------------------------------------------------- F04 PDF to images

#[test]
fn f04_pdf_to_images_modes_and_ranges() {
    if !has("pdftoppm") {
        return;
    }
    let o = out();
    let f = fx();
    let opts = |pages: &str, format| ToolOptions::PdfToImages(PdfToImagesOptions { pages: pages.into(), dpi: 72, format, render_pages: true });
    let r = job(Tool::PdfToImages, &[&f.plain], opts("2-4", ImageFormat::Png), o.path(), "pg").unwrap();
    let names: Vec<String> = r.outputs.iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
    assert_eq!(names, vec!["pg_2.png", "pg_3.png", "pg_4.png"]);
    for p in &r.outputs {
        image::open(p).unwrap();
    }
    for bad in ["8", "1-99", "0", "4-2", "abc"] {
        assert!(matches!(job(Tool::PdfToImages, &[&f.plain], opts(bad, ImageFormat::Jpg), o.path(), "bad"), Err(Error::InvalidRange(_))), "{bad}");
    }
    assert_eq!(ls(o.path()).len(), 3);
    // dpi is clamped, not an error
    let r = job(
        Tool::PdfToImages,
        &[&f.plain],
        ToolOptions::PdfToImages(PdfToImagesOptions { pages: "1".into(), dpi: 5, ..Default::default() }),
        o.path(),
        "lowdpi",
    )
    .unwrap();
    assert_eq!(r.outputs.len(), 1);
    // several PDFs: names carry file index and page
    let o2 = out();
    let r = job(Tool::PdfToImages, &[&f.plain, &f.plain], opts("1", ImageFormat::Jpg), o2.path(), "m").unwrap();
    assert_eq!(ls(o2.path()), vec!["m_1_1.jpg", "m_2_1.jpg"]);
    assert_eq!(r.outputs.len(), 2);
    // collision with an existing file
    let r = job(Tool::PdfToImages, &[&f.plain], opts("2", ImageFormat::Png), o.path(), "pg").unwrap();
    assert_eq!(r.outputs[0].file_name().unwrap(), "pg.png", "a single page gets no number");
    let r = job(Tool::PdfToImages, &[&f.plain], opts("2", ImageFormat::Png), o.path(), "pg").unwrap();
    assert_eq!(r.outputs[0].file_name().unwrap(), "pg (2).png");
}

#[test]
fn f04_extract_images_and_none_found() {
    if !has("pdfimages") || !has("pdftoppm") {
        return;
    }
    let o = out();
    let f = fx();
    let ex = |p: &str| ToolOptions::PdfToImages(PdfToImagesOptions { render_pages: false, pages: p.into(), ..Default::default() });
    // a PDF without images
    match job(Tool::PdfToImages, &[&f.plain], ex(""), o.path(), "e") {
        Err(Error::InvalidOptions(m)) => assert!(m.contains("No embedded images"), "{m}"),
        other => panic!("{other:?}"),
    }
    assert_empty(o.path());
    // images -> PDF -> extract again
    let pdf = o.path().join("imgs.pdf");
    let tmp = out();
    let r = job(Tool::ImagesToPdf, &[&f.png, &f.jpg], ToolOptions::ImagesToPdf(ImagesToPdfOptions::default()), tmp.path(), "imgs").unwrap();
    std::fs::copy(&r.outputs[0], &pdf).unwrap();
    let dest = out();
    let r = job(Tool::PdfToImages, &[&pdf], ex(""), dest.path(), "x").unwrap();
    assert_eq!(r.outputs.len(), 2);
    assert!(matches!(job(Tool::PdfToImages, &[&pdf], ex("3"), dest.path(), "bad"), Err(Error::InvalidRange(_))));
    // the LibreOffice document has an image on every page
    if let Some(real) = &f.realistic {
        let r = job(Tool::PdfToImages, &[real], ex("2-3"), dest.path(), "real").unwrap();
        assert!(!r.outputs.is_empty());
    }
}

#[test]
fn f04_pdf_to_images_encrypted_needs_password() {
    if !has("pdftoppm") || !has("gs") {
        return;
    }
    let o = out();
    let f = fx();
    let enc = f.enc.as_ref().unwrap();
    let opts = ToolOptions::PdfToImages(PdfToImagesOptions { pages: "1-2".into(), dpi: 72, ..Default::default() });
    assert!(matches!(job(Tool::PdfToImages, &[enc], opts.clone(), o.path(), "e"), Err(Error::Encrypted(_))));
    assert!(matches!(job_in(Tool::PdfToImages, vec![inp_pw(enc, "no")], opts.clone(), o.path(), "e"), Err(Error::WrongPassword(_))));
    let r = job_in(Tool::PdfToImages, vec![inp_pw(enc, PW)], opts, o.path(), "e").unwrap();
    assert_eq!(r.outputs.len(), 2);
    // thumbnails of locked files
    assert!(matches!(render_page(enc, 1, 100, None), Err(Error::Encrypted(_))));
    assert!(matches!(render_page(enc, 1, 100, Some("bad")), Err(Error::WrongPassword(_))));
    let (w, h, px) = render_page(enc, 1, 100, Some(PW)).unwrap();
    assert_eq!(px.len(), (w * h * 4) as usize);
}

#[test]
fn f04_render_page_edge_cases() {
    if !has("pdftoppm") {
        return;
    }
    let f = fx();
    assert!(matches!(render_page(&f.plain, 0, 100, None), Err(Error::InvalidRange(_))));
    assert!(matches!(render_page(&f.plain, 8, 100, None), Err(Error::InvalidRange(_))));
    assert!(render_page(&f.corrupt, 1, 100, None).is_err());
    assert!(matches!(render_page(&f.html, 1, 100, None), Err(Error::WrongType(_))));
    assert!(matches!(render_page(&f.png, 2, 100, None), Err(Error::InvalidRange(_))));
    let (w, h, _) = render_page(&f.jpg, 1, 100, None).unwrap();
    assert_eq!((w, h), (100, 67));
    // never upscales an image
    let (w, _, _) = render_page(&f.png, 1, 5000, None).unwrap();
    assert_eq!(w, 300);
    // max_px 0 behaves like 1
    render_page(&f.plain, 1, 0, None).unwrap();
}

// ---------------------------------------------------------------- F05 Office / HTML -> PDF

#[test]
fn f05_office_and_html_to_pdf() {
    if !has("soffice") {
        return;
    }
    let o = out();
    let f = fx();
    let r = job(Tool::HtmlToPdf, &[&f.html], ToolOptions::HtmlToPdf, o.path(), "web").unwrap();
    assert!(pages(&r.outputs[0]) >= 1);
    let r = job(Tool::OfficeToPdf, &[f.odt.as_ref().unwrap(), f.docx.as_ref().unwrap()], ToolOptions::OfficeToPdf, o.path(), "off").unwrap();
    assert_eq!(r.outputs.len(), 2);
    for p in &r.outputs {
        assert!(pages(p) >= 4, "{}: {} pages", p.display(), pages(p));
    }
    assert_eq!(r.outputs[0].file_name().unwrap(), "off_1.pdf");
    // wrong types
    assert!(matches!(job(Tool::OfficeToPdf, &[&f.html], ToolOptions::OfficeToPdf, o.path(), "x"), Err(Error::WrongType(_))));
    assert!(matches!(job(Tool::OfficeToPdf, &[&f.plain], ToolOptions::OfficeToPdf, o.path(), "x"), Err(Error::WrongType(_))));
    let odd = o.path().join("data.xyz");
    std::fs::write(&odd, "hi").unwrap();
    assert!(matches!(job(Tool::OfficeToPdf, &[&odd], ToolOptions::OfficeToPdf, o.path(), "x"), Err(Error::WrongType(_))));
}

#[test]
fn f05_broken_office_file_neither_hangs_nor_leaves_files() {
    if !has("soffice") {
        return;
    }
    let o = out();
    let bad = o.path().join("broken.docx");
    std::fs::write(&bad, b"this is not a zip").unwrap();
    let dest = out();
    let t = Instant::now();
    // LibreOffice opens unknown content as plain text, so success is fine; a hang or a
    // half-written file is not
    let r = job(Tool::OfficeToPdf, &[&bad], ToolOptions::OfficeToPdf, dest.path(), "b");
    assert!(t.elapsed() < Duration::from_secs(90));
    match r {
        Ok(r) => assert!(pages(&r.outputs[0]) >= 1),
        Err(_) => assert_empty(dest.path()),
    }
}

#[test]
fn f05_pdf_to_office_and_pdfa() {
    let f = fx();
    let o = out();
    if has("soffice") {
        let src = f.realistic.as_ref().unwrap();
        let r = job(Tool::PdfToOffice, &[src], ToolOptions::PdfToOffice { target: OfficeTarget::Docx }, o.path(), "word").unwrap();
        assert_eq!(&std::fs::read(&r.outputs[0]).unwrap()[..2], b"PK");
        assert!(r.notes.iter().any(|n| n.contains("lossy")));
        let r = job(Tool::PdfToOffice, &[src], ToolOptions::PdfToOffice { target: OfficeTarget::Odt }, o.path(), "odt").unwrap();
        assert_eq!(&std::fs::read(&r.outputs[0]).unwrap()[..2], b"PK");
    }
    if has("gs") {
        let r = job(Tool::PdfToPdfA, &[&f.plain], ToolOptions::PdfToPdfA { level: PdfALevel::B2 }, o.path(), "a").unwrap();
        assert_valid(&r.outputs[0], 7);
        let bytes = std::fs::read(&r.outputs[0]).unwrap();
        assert!(bytes.windows(6).any(|w| w == b"pdfaid"), "no PDF/A identification in the output");
    }
}

// ---------------------------------------------------------------- F06 organize

#[test]
fn f06_organize_reorder_rotate_blank_duplicate() {
    let o = out();
    for (name, src, n) in all_sources() {
        let n = n as u32;
        let ops = vec![
            PageOp::Page { source: n, rotate: Rotation::R90 },
            PageOp::Blank,
            PageOp::Page { source: 1, rotate: Rotation::R0 },
            PageOp::Page { source: 1, rotate: Rotation::R180 },
        ];
        let r = job(Tool::Organize, &[&src], ToolOptions::Organize { pages: ops }, o.path(), &format!("org-{name}")).unwrap();
        assert_valid(&r.outputs[0], 4);
        let doc = Document::load(&r.outputs[0]).unwrap();
        let ids: Vec<_> = doc.get_pages().into_values().collect();
        let rot = |i: usize| doc.get_dictionary(ids[i]).unwrap().get(b"Rotate").and_then(|o| o.as_i64()).unwrap_or(0);
        assert_eq!((rot(0), rot(1), rot(2), rot(3)), (90, 0, 0, 180));
    }
    let f = fx();
    for bad in [
        vec![PageOp::Page { source: 0, rotate: Rotation::R0 }],
        vec![PageOp::Page { source: 8, rotate: Rotation::R0 }],
        vec![],
    ] {
        let r = job(Tool::Organize, &[&f.plain], ToolOptions::Organize { pages: bad.clone() }, o.path(), "bad");
        assert!(matches!(r, Err(Error::InvalidRange(_) | Error::InvalidOptions(_))), "{bad:?} -> {r:?}");
    }
    // only blank pages is a valid (if odd) document
    let r = job(Tool::Organize, &[&f.plain], ToolOptions::Organize { pages: vec![PageOp::Blank, PageOp::Blank] }, o.path(), "blank").unwrap();
    assert_valid(&r.outputs[0], 2);
}

#[test]
fn f06_organize_200_pages_performance() {
    let o = out();
    let f = fx();
    let ops: Vec<PageOp> = (1..=200).rev().map(|source| PageOp::Page { source, rotate: Rotation::R0 }).collect();
    let t = Instant::now();
    let r = job(Tool::Organize, &[&f.big], ToolOptions::Organize { pages: ops }, o.path(), "rev").unwrap();
    eprintln!("organize 200 pages reversed: {:?}", t.elapsed());
    assert!(t.elapsed() < Duration::from_secs(60));
    assert_eq!(pages(&r.outputs[0]), 200);
    assert!(page_text(&r.outputs[0], 1).contains("Page 200"));
    assert!(page_text(&r.outputs[0], 200).contains("Page 1"));
}

// ---------------------------------------------------------------- F07 rotate, crop

#[test]
fn f07_rotate_pages_accumulate_and_wrap() {
    let o = out();
    let f = fx();
    let rot_of = |p: &Path, n: usize| {
        let doc = Document::load(p).unwrap();
        let ids: Vec<_> = doc.get_pages().into_values().collect();
        doc.get_dictionary(ids[n - 1]).unwrap().get(b"Rotate").and_then(|o| o.as_i64()).unwrap_or(0)
    };
    let r = job(Tool::Rotate, &[&f.plain], ToolOptions::Rotate { degrees: Rotation::R90, pages: "2,4-5".into() }, o.path(), "r1").unwrap();
    assert_eq!((rot_of(&r.outputs[0], 1), rot_of(&r.outputs[0], 2), rot_of(&r.outputs[0], 5), rot_of(&r.outputs[0], 6)), (0, 90, 90, 0));
    let r2 = job(Tool::Rotate, &[&r.outputs[0]], ToolOptions::Rotate { degrees: Rotation::R270, pages: "".into() }, o.path(), "r2").unwrap();
    assert_eq!((rot_of(&r2.outputs[0], 1), rot_of(&r2.outputs[0], 2), rot_of(&r2.outputs[0], 7)), (270, 0, 270), "90 + 270 wraps to 0");
    // visual size follows the rotation
    let i = inspect(&r.outputs[0], None).unwrap();
    assert_eq!(i.first_page_size, Some((501.0, 800.0)));
    let p2 = {
        let doc = Document::load(&r.outputs[0]).unwrap();
        let id = *doc.get_pages().get(&2).unwrap();
        let (w, h) = (doc.get_dictionary(id).unwrap().get(b"MediaBox").unwrap().as_array().unwrap()[2].as_i64().unwrap(), 800);
        (w, h)
    };
    assert_eq!(p2, (502, 800));
    for bad in ["8", "0", "5-2"] {
        assert!(matches!(
            job(Tool::Rotate, &[&f.plain], ToolOptions::Rotate { degrees: Rotation::R90, pages: bad.into() }, o.path(), "bad"),
            Err(Error::InvalidRange(_))
        ));
    }
    // several files at once, mixed lengths, same selection valid in both
    let r = job(Tool::Rotate, &[&f.plain, &f.big], ToolOptions::Rotate { degrees: Rotation::R180, pages: "1-3".into() }, o.path(), "multi").unwrap();
    assert_eq!(r.outputs.len(), 2);
    // selection past the shorter file fails the whole job and leaves nothing
    let o2 = out();
    assert!(matches!(
        job(Tool::Rotate, &[&f.big, &f.plain], ToolOptions::Rotate { degrees: Rotation::R180, pages: "50".into() }, o2.path(), "multi"),
        Err(Error::InvalidRange(_))
    ));
    assert_empty(o2.path());
}

#[test]
fn f07_crop_edge_cases() {
    let o = out();
    let f = fx();
    let crop = |t, r, b, l, p: &str| ToolOptions::Crop(CropOptions { top: t, right: r, bottom: b, left: l, pages: p.into() });
    let res = job(Tool::Crop, &[&f.plain], crop(10.0, 20.0, 30.0, 40.0, ""), o.path(), "c").unwrap();
    assert_valid(&res.outputs[0], 7);
    let doc = Document::load(&res.outputs[0]).unwrap();
    let id = *doc.get_pages().get(&1).unwrap();
    let cb: Vec<f32> = doc.get_dictionary(id).unwrap().get(b"CropBox").unwrap().as_array().unwrap().iter().map(|o| o.as_float().unwrap()).collect();
    assert_eq!(cb, vec![40.0, 30.0, 501.0 - 20.0, 790.0]);
    // margins eat the page
    assert!(matches!(job(Tool::Crop, &[&f.plain], crop(400.0, 0.0, 400.0, 0.0, ""), o.path(), "bad"), Err(Error::InvalidOptions(_))));
    // negative margins are treated as zero
    let r = job(Tool::Crop, &[&f.plain], crop(-5.0, -5.0, -5.0, -5.0, "1"), o.path(), "neg").unwrap();
    assert_valid(&r.outputs[0], 7);
    assert!(matches!(job(Tool::Crop, &[&f.plain], crop(1.0, 1.0, 1.0, 1.0, "9"), o.path(), "bad"), Err(Error::InvalidRange(_))));
    // crop of an already rotated page uses visual margins
    let rotated = job(Tool::Rotate, &[&f.plain], ToolOptions::Rotate { degrees: Rotation::R90, pages: "1".into() }, o.path(), "rot").unwrap();
    let r = job(Tool::Crop, &[&rotated.outputs[0]], crop(100.0, 0.0, 0.0, 0.0, "1"), o.path(), "rc").unwrap();
    let info = inspect(&r.outputs[0], None).unwrap();
    let (w, h) = info.first_page_size.unwrap();
    assert_eq!((w, h), (800.0, 401.0));
}

// ---------------------------------------------------------------- F08 stamps

#[test]
fn f08_page_numbers_ranges_and_rotated_pages() {
    let o = out();
    let f = fx();
    let rotated = job(Tool::Rotate, &[&f.plain], ToolOptions::Rotate { degrees: Rotation::R90, pages: "2-3".into() }, o.path(), "rot").unwrap();
    let opts = |p: &str| ToolOptions::PageNumbers(PageNumberOptions { pages: p.into(), template: "[{n}/{p}]".into(), ..Default::default() });
    let r = job(Tool::PageNumbers, &[&rotated.outputs[0]], opts("2-4"), o.path(), "num").unwrap();
    assert_valid(&r.outputs[0], 7);
    assert!(page_text(&r.outputs[0], 2).contains("[2/7]"));
    assert!(page_text(&r.outputs[0], 4).contains("[4/7]"));
    assert!(!page_text(&r.outputs[0], 1).contains("[1/7]"));
    assert!(!page_text(&r.outputs[0], 5).contains("[5/7]"));
    for bad in ["8", "0", "4-2"] {
        assert!(matches!(job(Tool::PageNumbers, &[&f.plain], opts(bad), o.path(), "bad"), Err(Error::InvalidRange(_))), "{bad}");
    }
    // every position works on odd-sized pages
    for pos in Position::ALL {
        let r = job(
            Tool::PageNumbers,
            &[&f.plain],
            ToolOptions::PageNumbers(PageNumberOptions { position: pos, facing_pages: true, ..Default::default() }),
            o.path(),
            "pos",
        )
        .unwrap();
        assert_valid(&r.outputs[0], 7);
    }
    // a template without placeholders is allowed, huge font and margin must not panic
    let r = job(
        Tool::PageNumbers,
        &[&f.plain],
        ToolOptions::PageNumbers(PageNumberOptions { template: "fixed".into(), font_size: 900.0, margin: 5000.0, ..Default::default() }),
        o.path(),
        "extreme",
    );
    assert!(r.is_ok() || matches!(r, Err(Error::InvalidOptions(_))));
    // objstm and realistic sources
    for (name, src, n) in all_sources() {
        let r = job(Tool::PageNumbers, &[&src], ToolOptions::PageNumbers(PageNumberOptions::default()), o.path(), &format!("n-{name}")).unwrap();
        assert_valid(&r.outputs[0], n);
    }
}

#[test]
fn f08_watermark_variants_and_pages() {
    let o = out();
    let f = fx();
    for (name, src, n) in all_sources() {
        let r = job(Tool::Watermark, &[&src], ToolOptions::Watermark(WatermarkOptions { pages: "1".into(), ..Default::default() }), o.path(), &format!("w-{name}")).unwrap();
        assert_valid(&r.outputs[0], n);
        assert!(page_text(&r.outputs[0], 1).contains("CONFIDENTIAL") || name == "realistic" || name == "objstm");
    }
    for (op, below, mosaic, rot) in [(0.0, true, true, 0.0), (1.0, false, false, 90.0), (0.5, true, false, 33.3), (0.4, false, true, -45.0)] {
        let r = job(
            Tool::Watermark,
            &[&f.plain],
            ToolOptions::Watermark(WatermarkOptions { opacity: op, below_content: below, mosaic, rotation_deg: rot, ..Default::default() }),
            o.path(),
            "wv",
        )
        .unwrap();
        assert_valid(&r.outputs[0], 7);
    }
    // image watermark from png and jpg
    for img in [&f.png, &f.jpg] {
        let r = job(
            Tool::Watermark,
            &[&f.plain],
            ToolOptions::Watermark(WatermarkOptions { content: WatermarkContent::Image { path: img.clone(), scale: 0.5 }, ..Default::default() }),
            o.path(),
            "wi",
        )
        .unwrap();
        assert_valid(&r.outputs[0], 7);
    }
    // errors
    let blank = ToolOptions::Watermark(WatermarkOptions { content: WatermarkContent::Text { text: "  ".into(), font_size: 40.0, color: Rgb(0, 0, 0), bold: false }, ..Default::default() });
    assert!(matches!(job(Tool::Watermark, &[&f.plain], blank, o.path(), "b"), Err(Error::InvalidOptions(_))));
    let nofile = ToolOptions::Watermark(WatermarkOptions { content: WatermarkContent::Image { path: o.path().join("none.png"), scale: 0.5 }, ..Default::default() });
    assert!(job(Tool::Watermark, &[&f.plain], nofile, o.path(), "b").is_err());
    let notimg = ToolOptions::Watermark(WatermarkOptions { content: WatermarkContent::Image { path: f.corrupt.clone(), scale: 0.5 }, ..Default::default() });
    assert!(matches!(job(Tool::Watermark, &[&f.plain], notimg, o.path(), "b"), Err(Error::Image(_) | Error::Io(_))));
    assert!(matches!(
        job(Tool::Watermark, &[&f.plain], ToolOptions::Watermark(WatermarkOptions { pages: "99".into(), ..Default::default() }), o.path(), "b"),
        Err(Error::InvalidRange(_))
    ));
}

#[test]
fn f08_non_latin_text_is_not_silently_lost() {
    let o = out();
    let f = fx();
    let wm = ToolOptions::Watermark(WatermarkOptions {
        content: WatermarkContent::Text { text: "日本語 Привет".into(), font_size: 40.0, color: Rgb(0, 0, 0), bold: false },
        ..Default::default()
    });
    let r = job(Tool::Watermark, &[&f.plain], wm, o.path(), "jp").unwrap();
    assert_valid(&r.outputs[0], 7);
    assert!(r.notes.iter().any(|n| n.contains("?")), "the user is told that characters were replaced: {:?}", r.notes);
    let sig = ToolOptions::Sign(SignOptions { signature: SignatureKind::Typed { name: "Zoë Müller".into() }, ..Default::default() });
    let r = job(Tool::Sign, &[&f.plain], sig, o.path(), "ok").unwrap();
    assert!(r.notes.is_empty(), "Latin-1 names need no warning: {:?}", r.notes);
    let pn = ToolOptions::PageNumbers(PageNumberOptions { template: "第{n}頁".into(), ..Default::default() });
    let r = job(Tool::PageNumbers, &[&f.plain], pn, o.path(), "pn").unwrap();
    assert!(!r.notes.is_empty());
}

#[test]
fn f10_sign_pages_and_errors() {
    let o = out();
    let f = fx();
    let typed = |name: &str, page| SignOptions { signature: SignatureKind::Typed { name: name.into() }, page, ..Default::default() };
    // page 0 = last page
    let r = job(Tool::Sign, &[&f.plain], ToolOptions::Sign(typed("Max Mustermann", 0)), o.path(), "last").unwrap();
    assert!(page_text(&r.outputs[0], 7).contains("Max Mustermann"));
    assert!(!page_text(&r.outputs[0], 1).contains("Max Mustermann"));
    let r = job(Tool::Sign, &[&f.plain], ToolOptions::Sign(typed("Max Mustermann", 3)), o.path(), "p3").unwrap();
    assert!(page_text(&r.outputs[0], 3).contains("Max Mustermann"));
    assert!(matches!(job(Tool::Sign, &[&f.plain], ToolOptions::Sign(typed("T", 8)), o.path(), "bad"), Err(Error::InvalidRange(_))));
    assert!(matches!(job(Tool::Sign, &[&f.plain], ToolOptions::Sign(typed("  ", 1)), o.path(), "bad"), Err(Error::InvalidOptions(_))));
    // image signature, every position, wider than the page
    for pos in Position::ALL {
        let r = job(
            Tool::Sign,
            &[&f.plain],
            ToolOptions::Sign(SignOptions { signature: SignatureKind::Image { path: f.png.clone() }, position: pos, width: 5000.0, ..Default::default() }),
            o.path(),
            "img",
        );
        assert!(r.is_ok(), "{pos:?}: {r:?}");
        assert_valid(&r.unwrap().outputs[0], 7);
    }
    assert!(job(Tool::Sign, &[&f.plain], ToolOptions::Sign(SignOptions { signature: SignatureKind::Image { path: f.corrupt.clone() }, ..Default::default() }), o.path(), "bad").is_err());
    // signing an object-stream PDF and a LibreOffice PDF
    for (name, src, n) in all_sources() {
        let r = job(Tool::Sign, &[&src], ToolOptions::Sign(typed("Sig", 1)), o.path(), &format!("s-{name}")).unwrap();
        assert_valid(&r.outputs[0], n);
    }
}

#[test]
fn f10_sign_encrypted_file() {
    if !has("gs") {
        return;
    }
    let o = out();
    let f = fx();
    let enc = f.enc.as_ref().unwrap();
    let opt = ToolOptions::Sign(SignOptions { signature: SignatureKind::Typed { name: "Max".into() }, ..Default::default() });
    assert!(matches!(job(Tool::Sign, &[enc], opt.clone(), o.path(), "s"), Err(Error::Encrypted(_))));
    let r = job_in(Tool::Sign, vec![inp_pw(enc, PW)], opt, o.path(), "s").unwrap();
    assert_valid(&r.outputs[0], 5);
}

// ---------------------------------------------------------------- F09 protect / unlock

#[test]
fn f09_protect_unlock_roundtrip_with_awkward_passwords() {
    if !has("gs") {
        return;
    }
    let f = fx();
    for pw in ["simple", "with space", "quote\"inside", "single'quote", "back\\slash", "percent%d $HOME `x` #hash", "ümläut €uro 日本", "a"] {
        let o = out();
        let r = job(
            Tool::Protect,
            &[&f.plain],
            ToolOptions::Protect(ProtectOptions { user_password: pw.into(), owner_password: String::new(), ..Default::default() }),
            o.path(),
            "p",
        )
        .unwrap();
        let locked = &r.outputs[0];
        let i = inspect(locked, None).unwrap();
        assert!(i.encrypted && i.pages.is_none(), "{pw}");
        // an independent reader (gs on its own command line) agrees on the password
        let chk = Command::new("gs")
            .args(["-dBATCH", "-dNOPAUSE", "-dQUIET", "-sDEVICE=nullpage"])
            .arg(format!("-sPDFPassword={pw}"))
            .arg(locked)
            .output()
            .unwrap();
        assert!(chk.status.success() && !String::from_utf8_lossy(&chk.stderr).to_lowercase().contains("password"), "{pw}: {}", String::from_utf8_lossy(&chk.stderr));
        assert!(matches!(inspect(locked, Some("wrong")), Err(Error::WrongPassword(_))));
        assert_eq!(inspect(locked, Some(pw)).unwrap().pages, Some(7));
        let u = job_in(Tool::Unlock, vec![inp_pw(locked, pw)], ToolOptions::Unlock, o.path(), "u").unwrap();
        assert!(!inspect(&u.outputs[0], None).unwrap().encrypted, "{pw}");
        assert_valid(&u.outputs[0], 7);
    }
}

#[test]
fn f09_unlock_errors_and_double_protect() {
    if !has("gs") {
        return;
    }
    let o = out();
    let f = fx();
    let enc = f.enc.as_ref().unwrap();
    assert!(matches!(job(Tool::Unlock, &[enc], ToolOptions::Unlock, o.path(), "u"), Err(Error::Encrypted(_))));
    assert!(matches!(job_in(Tool::Unlock, vec![inp_pw(enc, "nope")], ToolOptions::Unlock, o.path(), "u"), Err(Error::WrongPassword(_))));
    assert_empty(o.path());
    // unlocking an unprotected file just copies it
    let r = job(Tool::Unlock, &[&f.plain], ToolOptions::Unlock, o.path(), "plain").unwrap();
    assert_valid(&r.outputs[0], 7);
    // protecting an already protected file needs its old password, then replaces it
    assert!(matches!(
        job(Tool::Protect, &[enc], ToolOptions::Protect(ProtectOptions { user_password: "new".into(), ..Default::default() }), o.path(), "pp"),
        Err(Error::Encrypted(_))
    ));
    let r = job_in(Tool::Protect, vec![inp_pw(enc, PW)], ToolOptions::Protect(ProtectOptions { user_password: "new".into(), ..Default::default() }), o.path(), "pp").unwrap();
    assert_eq!(inspect(&r.outputs[0], Some("new")).unwrap().pages, Some(5));
    assert!(matches!(inspect(&r.outputs[0], Some(PW)), Err(Error::WrongPassword(_))));
    // no password at all
    assert!(matches!(
        job(Tool::Protect, &[&f.plain], ToolOptions::Protect(ProtectOptions::default()), o.path(), "np"),
        Err(Error::InvalidOptions(_))
    ));
    // unlock keeps content
    let un = job_in(Tool::Unlock, vec![inp_pw(enc, PW)], ToolOptions::Unlock, o.path(), "un").unwrap();
    assert!(page_text(&un.outputs[0], 3).contains("Page 3"));
}

#[test]
fn f09_permission_bits() {
    let none = Permissions { allow_print: false, allow_copy: false, allow_modify: false, allow_annotate: false };
    let all = Permissions { allow_print: true, allow_copy: true, allow_modify: true, allow_annotate: true };
    let get = |p: &Permissions| {
        // exercise through the public engine: protect and read /P back
        let o = out();
        let f = fx();
        let r = job(
            Tool::Protect,
            &[&f.plain],
            ToolOptions::Protect(ProtectOptions { user_password: "x".into(), owner_password: "y".into(), permissions: p.clone() }),
            o.path(),
            "perm",
        )
        .unwrap();
        let bytes = std::fs::read(&r.outputs[0]).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        let at = text.find("/P ").expect("/P entry");
        text[at + 3..].split(|c: char| !(c.is_ascii_digit() || c == '-')).next().unwrap().parse::<i32>().unwrap()
    };
    if !has("gs") {
        return;
    }
    let (n, a) = (get(&none), get(&all));
    assert_eq!(n & 4, 0, "print bit");
    assert_eq!(n & 16, 0, "copy bit");
    assert_eq!(n & 8, 0, "modify bit");
    assert_eq!(n & 32, 0, "annotate bit");
    assert_ne!(a & 4, 0);
    assert_ne!(a & 16, 0);
    assert_ne!(a & 8, 0);
    assert_ne!(a & 32, 0);
}

/// F09 / security: the password never shows up in the command line of a child process.
#[test]
#[cfg(unix)] // Unix permissions / ps
fn f09_passwords_are_not_visible_in_ps() {
    if !has("gs") || !Path::new("/proc/self/cmdline").exists() {
        return;
    }
    let f = fx();
    let secret = "UNIQUE-secret-7f3a9c";
    let stop = AtomicBool::new(false);
    let o = out();
    let (mut seen_gs, mut seen_pdftoppm) = (0usize, 0usize);
    let mut leaked: Vec<String> = Vec::new();
    let mut argfiles: Vec<String> = Vec::new();
    std::thread::scope(|s| {
        let sampler = s.spawn(|| {
            let mut gs = 0usize;
            let mut pp = 0usize;
            let mut leaks = Vec::new();
            let mut files = Vec::new();
            while !stop.load(Ordering::Relaxed) {
                for e in std::fs::read_dir("/proc").unwrap().flatten() {
                    if !e.file_name().to_string_lossy().chars().all(|c| c.is_ascii_digit()) {
                        continue;
                    }
                    let Ok(raw) = std::fs::read(e.path().join("cmdline")) else { continue };
                    let args: Vec<String> = raw.split(|b| *b == 0).map(|a| String::from_utf8_lossy(a).into_owned()).collect();
                    let prog = args.first().map(|a| a.rsplit('/').next().unwrap_or("").to_string()).unwrap_or_default();
                    if prog == "gs" && args.iter().any(|a| a.contains("pdfwrite")) {
                        gs += 1;
                        if args.iter().any(|a| a.contains(secret)) {
                            leaks.push(args.join(" "));
                        }
                        for a in &args {
                            if let Some(p) = a.strip_prefix('@') {
                                // only our own argument files (other tests run gs in parallel)
                                let ours = std::fs::read_to_string(p).is_ok_and(|t| t.contains(secret));
                                if ours && !files.contains(&p.to_string()) {
                                    // argument file must be private while it exists
                                    if let Ok(m) = std::fs::metadata(p) {
                                        use std::os::unix::fs::PermissionsExt;
                                        assert_eq!(m.permissions().mode() & 0o077, 0, "argument file is readable by others");
                                    }
                                    files.push(p.to_string());
                                }
                            }
                        }
                    }
                    if prog == "pdftoppm" || prog == "pdfimages" {
                        pp += 1;
                        if args.iter().any(|a| a.contains(secret) || a == "-upw" || a == "-opw") {
                            leaks.push(args.join(" "));
                        }
                    }
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            (gs, pp, leaks, files)
        });
        // protect, unlock, compress and render a 200-page file so gs runs long enough to be seen
        let big = f.big.as_path();
        let p = job(
            Tool::Protect,
            &[big],
            ToolOptions::Protect(ProtectOptions { user_password: secret.into(), owner_password: format!("{secret}-owner"), ..Default::default() }),
            o.path(),
            "locked",
        )
        .unwrap();
        let locked = p.outputs[0].clone();
        job_in(Tool::Unlock, vec![inp_pw(&locked, secret)], ToolOptions::Unlock, o.path(), "unlocked").unwrap();
        job_in(Tool::Compress, vec![inp_pw(&locked, secret)], ToolOptions::Compress { level: CompressLevel::Extreme }, o.path(), "small").unwrap();
        if has("pdftoppm") {
            job_in(
                Tool::PdfToImages,
                vec![inp_pw(&locked, secret)],
                ToolOptions::PdfToImages(PdfToImagesOptions { pages: "1-3".into(), dpi: 72, ..Default::default() }),
                o.path(),
                "img",
            )
            .unwrap();
            render_page(&locked, 1, 100, Some(secret)).unwrap();
        }
        stop.store(true, Ordering::Relaxed);
        let (g, p, l, a) = sampler.join().unwrap();
        seen_gs = g;
        seen_pdftoppm = p;
        leaked = l;
        argfiles = a;
    });
    eprintln!("sampled gs {seen_gs}x, poppler {seen_pdftoppm}x, argument files {}", argfiles.len());
    assert!(leaked.is_empty(), "password visible in ps: {leaked:?}");
    assert!(seen_gs > 0, "the sampler never caught a gs process, the test proves nothing");
    assert!(!argfiles.is_empty(), "gs should be started with an @argument file");
    for f in &argfiles {
        assert!(!Path::new(f).exists(), "argument file {f} was not deleted");
    }
}

// ---------------------------------------------------------------- F15 corrupt / encrypted input

#[test]
fn f15_inspect_states() {
    let f = fx();
    let o = out();
    assert!(matches!(inspect(&f.corrupt, None), Err(Error::Corrupt(_))));
    let empty = o.path().join("empty.pdf");
    std::fs::write(&empty, b"").unwrap();
    assert!(matches!(inspect(&empty, None), Err(Error::Corrupt(_))));
    let junk = o.path().join("junk.pdf");
    std::fs::write(&junk, b"%PDF-1.4\n\x00\x01\x02 garbage").unwrap();
    assert!(matches!(inspect(&junk, None), Err(Error::Corrupt(_))));
    assert!(matches!(inspect(&o.path().join("missing.pdf"), None), Err(Error::Io(_))));
    // not a PDF at all: only metadata
    let i = inspect(&f.png, None).unwrap();
    assert_eq!((i.kind, i.pages), (FileKind::Image, None));
    if let Some(enc) = &f.enc {
        let i = inspect(enc, None).unwrap();
        assert!(i.encrypted && i.pages.is_none());
        assert!(matches!(inspect(enc, Some("x")), Err(Error::WrongPassword(_))));
        let i = inspect(enc, Some(PW)).unwrap();
        assert_eq!((i.pages, i.encrypted), (Some(5), true));
        // an empty password string behaves like none
        assert!(inspect(enc, Some("")).unwrap().pages.is_none());
    }
}

#[test]
fn f15_corrupt_then_repair() {
    let o = out();
    let f = fx();
    for tool in [Tool::Rotate, Tool::Split, Tool::Merge, Tool::PageNumbers] {
        let inputs: Vec<&Path> = if tool == Tool::Merge { vec![&f.corrupt, &f.plain] } else { vec![&f.corrupt] };
        let opts = match tool {
            Tool::Rotate => ToolOptions::Rotate { degrees: Rotation::R90, pages: "".into() },
            Tool::Split => ToolOptions::Split(SplitOptions::EveryPage),
            Tool::Merge => ToolOptions::Merge,
            _ => ToolOptions::PageNumbers(PageNumberOptions::default()),
        };
        match job(tool, &inputs, opts, o.path(), "x") {
            Err(Error::Corrupt(p)) => assert_eq!(p, f.corrupt, "{tool:?}"),
            other => panic!("{tool:?}: expected Corrupt, got {other:?}"),
        }
    }
    assert_empty(o.path());
    if !has("gs") {
        return;
    }
    // missing trailer: rescued with all pages
    let r = job(Tool::Repair, &[&f.tail_cut], ToolOptions::Repair, o.path(), "fixed").unwrap();
    assert_valid(&r.outputs[0], 7);
    // page tree lost: either rescued or a clean Corrupt, never a panic or a half-written file
    match job(Tool::Repair, &[&f.corrupt], ToolOptions::Repair, o.path(), "fixed2") {
        Ok(r) => assert!(pages(&r.outputs[0]) >= 1),
        Err(Error::Corrupt(p)) => assert_eq!(p, f.corrupt),
        Err(e) => panic!("{e}"),
    }
    assert!(ls(o.path()).iter().all(|n| !n.ends_with(".part")));
}

#[test]
fn f15_repair_garbage_encrypted_and_healthy() {
    if !has("gs") {
        return;
    }
    let o = out();
    let f = fx();
    let junk = o.path().join("junk.pdf");
    std::fs::write(&junk, b"just some text, no pdf here").unwrap();
    let dest = out();
    assert!(matches!(job(Tool::Repair, &[&junk], ToolOptions::Repair, dest.path(), "r"), Err(Error::Corrupt(_))));
    assert_empty(dest.path());
    let enc = f.enc.as_ref().unwrap();
    assert!(matches!(job(Tool::Repair, &[enc], ToolOptions::Repair, dest.path(), "r"), Err(Error::Encrypted(_))));
    assert!(matches!(job_in(Tool::Repair, vec![inp_pw(enc, "bad")], ToolOptions::Repair, dest.path(), "r"), Err(Error::WrongPassword(_))));
    // a healthy file passes through the repair tool unchanged in page count
    for (_, src, n) in all_sources() {
        let r = job(Tool::Repair, &[&src], ToolOptions::Repair, dest.path(), "ok").unwrap();
        assert_valid(&r.outputs[0], n);
    }
    // a good and a bad file: whole job fails, nothing is left
    let dest = out();
    assert!(job(Tool::Repair, &[&f.plain, &junk], ToolOptions::Repair, dest.path(), "r").is_err());
    assert_empty(dest.path());
}

// ---------------------------------------------------------------- images -> PDF

#[test]
fn images_to_pdf_options_and_odd_inputs() {
    let o = out();
    let f = fx();
    let tiff = o.path().join("t.tiff");
    let bmp = o.path().join("b.bmp");
    let gif = o.path().join("g.gif");
    let webp = o.path().join("w.webp");
    let rgb = image::RgbImage::from_fn(40, 30, |x, y| image::Rgb([x as u8 * 6, y as u8 * 8, 99]));
    rgb.save(&tiff).unwrap();
    rgb.save(&bmp).unwrap();
    rgb.save(&gif).unwrap();
    let _ = image::DynamicImage::ImageRgb8(rgb.clone()).save(&webp);
    let rgba = o.path().join("alpha.png");
    image::RgbaImage::from_fn(50, 50, |x, _| image::Rgba([255, 0, 0, (x * 5) as u8])).save(&rgba).unwrap();
    let mut all: Vec<&Path> = vec![&f.png, &f.jpg, &tiff, &bmp, &gif, &rgba];
    if webp.exists() {
        all.push(&webp);
    }
    let dest = out();
    let r = job(Tool::ImagesToPdf, &all, ToolOptions::ImagesToPdf(ImagesToPdfOptions::default()), dest.path(), "all").unwrap();
    assert_valid(&r.outputs[0], all.len());
    // one PDF per image
    let r = job(Tool::ImagesToPdf, &all, ToolOptions::ImagesToPdf(ImagesToPdfOptions { merge: false, ..Default::default() }), dest.path(), "sep").unwrap();
    assert_eq!(r.outputs.len(), all.len());
    for ps in [PageSize::Fit, PageSize::A4, PageSize::Letter] {
        for ori in [Orientation::Auto, Orientation::Portrait, Orientation::Landscape] {
            for m in [Margin::None, Margin::Small, Margin::Big] {
                let r = job(Tool::ImagesToPdf, &[&f.png, &f.jpg], ToolOptions::ImagesToPdf(ImagesToPdfOptions { page_size: ps, orientation: ori, margin: m, merge: true }), dest.path(), "grid").unwrap();
                assert_eq!(pages(&r.outputs[0]), 2);
            }
        }
    }
    // A4 landscape really is wider than tall
    let r = job(
        Tool::ImagesToPdf,
        &[&f.png],
        ToolOptions::ImagesToPdf(ImagesToPdfOptions { page_size: PageSize::A4, orientation: Orientation::Landscape, margin: Margin::None, merge: true }),
        dest.path(),
        "land",
    )
    .unwrap();
    let (w, h) = inspect(&r.outputs[0], None).unwrap().first_page_size.unwrap();
    assert!(w > h);
    // not an image / truncated image
    let bad = o.path().join("bad.png");
    std::fs::write(&bad, b"\x89PNG\r\n\x1a\nnope").unwrap();
    let dest2 = out();
    assert!(matches!(job(Tool::ImagesToPdf, &[&f.png, &bad], ToolOptions::ImagesToPdf(ImagesToPdfOptions::default()), dest2.path(), "x"), Err(Error::Image(_))));
    assert_empty(dest2.path());
    assert!(matches!(job(Tool::ImagesToPdf, &[&f.plain], ToolOptions::ImagesToPdf(ImagesToPdfOptions::default()), dest2.path(), "x"), Err(Error::WrongType(_))));
    // JPEG is embedded as is (no re-encode): output is not much bigger than the jpg
    let r = job(Tool::ImagesToPdf, &[&f.jpg], ToolOptions::ImagesToPdf(ImagesToPdfOptions::default()), dest2.path(), "jpg").unwrap();
    assert!(r.output_bytes < r.input_bytes + 4000, "{} vs {}", r.output_bytes, r.input_bytes);
}

#[test]
fn images_with_exif_rotation_and_huge_dimensions() {
    let o = out();
    // a very wide, very flat image must not panic
    let wide = o.path().join("wide.png");
    image::RgbImage::from_pixel(6000, 3, image::Rgb([1, 2, 3])).save(&wide).unwrap();
    let tall = o.path().join("tall.png");
    image::RgbImage::from_pixel(3, 6000, image::Rgb([1, 2, 3])).save(&tall).unwrap();
    let one = o.path().join("one.png");
    image::RgbImage::from_pixel(1, 1, image::Rgb([9, 9, 9])).save(&one).unwrap();
    let dest = out();
    for ps in [PageSize::Fit, PageSize::A4] {
        let r = job(Tool::ImagesToPdf, &[&wide, &tall, &one], ToolOptions::ImagesToPdf(ImagesToPdfOptions { page_size: ps, ..Default::default() }), dest.path(), "ext").unwrap();
        assert_eq!(pages(&r.outputs[0]), 3);
    }
}

// ---------------------------------------------------------------- performance, cancel

#[test]
fn performance_200_pages() {
    let o = out();
    let f = fx();
    let t = Instant::now();
    let i = inspect(&f.big, None).unwrap();
    assert_eq!(i.pages, Some(200));
    assert!(t.elapsed() < Duration::from_secs(5), "inspect {:?}", t.elapsed());
    let t = Instant::now();
    let m = job(Tool::Merge, &[&f.big, &f.big], ToolOptions::Merge, o.path(), "m").unwrap();
    eprintln!("merge 2x200: {:?}", t.elapsed());
    assert_eq!(pages(&m.outputs[0]), 400);
    assert!(t.elapsed() < Duration::from_secs(60));
    let t = Instant::now();
    let r = job(Tool::PageNumbers, &[&f.big], ToolOptions::PageNumbers(PageNumberOptions::default()), o.path(), "n").unwrap();
    eprintln!("numbers on 200: {:?}", t.elapsed());
    assert_valid(&r.outputs[0], 200);
    let t = Instant::now();
    let r = job(Tool::Watermark, &[&f.big], ToolOptions::Watermark(WatermarkOptions::default()), o.path(), "w").unwrap();
    eprintln!("watermark on 200: {:?}", t.elapsed());
    assert_valid(&r.outputs[0], 200);
    let t = Instant::now();
    let r = job(Tool::RemovePages, &[&f.big], ToolOptions::RemovePages { pages: "2-199".into() }, o.path(), "rm").unwrap();
    assert_eq!(pages(&r.outputs[0]), 2);
    eprintln!("remove 198 of 200: {:?}", t.elapsed());
    if has("gs") {
        let t = Instant::now();
        job(Tool::Compress, &[&f.big], ToolOptions::Compress { level: CompressLevel::Recommended }, o.path(), "c").unwrap();
        eprintln!("compress 200: {:?}", t.elapsed());
        assert!(t.elapsed() < Duration::from_secs(120));
    }
    if has("pdftoppm") {
        let t = Instant::now();
        let r = job(
            Tool::PdfToImages,
            &[&f.big],
            ToolOptions::PdfToImages(PdfToImagesOptions { dpi: 72, ..Default::default() }),
            o.path(),
            "img",
        )
        .unwrap();
        eprintln!("200 pages to jpg @72dpi: {:?}", t.elapsed());
        assert_eq!(r.outputs.len(), 200);
        assert!(t.elapsed() < Duration::from_secs(180));
    }
}

fn cancel_after(ms: u64, cancel: &AtomicBool) {
    std::thread::sleep(Duration::from_millis(ms));
    cancel.store(true, Ordering::Relaxed);
}

#[test]
fn cancel_mid_job_pdf_to_images_leaves_nothing() {
    if !has("pdftoppm") {
        return;
    }
    let o = out();
    let f = fx();
    let cancel = AtomicBool::new(false);
    let progress_seen = std::sync::atomic::AtomicUsize::new(0);
    let req = JobRequest {
        tool: Tool::PdfToImages,
        inputs: vec![inp(&f.big)],
        options: ToolOptions::PdfToImages(PdfToImagesOptions { dpi: 150, ..Default::default() }),
        output_dir: o.path().to_path_buf(),
        output_stem: "pg".into(),
    };
    let t = Instant::now();
    let res = std::thread::scope(|s| {
        // cancel once some pages have been written, so the cleanup has real work to do
        s.spawn(|| {
            let start = Instant::now();
            while progress_seen.load(Ordering::Relaxed) < 5 && start.elapsed() < Duration::from_secs(30) {
                std::thread::sleep(Duration::from_millis(5));
            }
            cancel.store(true, Ordering::Relaxed);
        });
        run(&req, &|_, _| { progress_seen.fetch_add(1, Ordering::Relaxed); }, &cancel)
    });
    let took = t.elapsed();
    assert!(matches!(res, Err(Error::Cancelled)), "{res:?}");
    assert!(progress_seen.load(Ordering::Relaxed) >= 5, "job was cancelled before it started");
    assert!(took < Duration::from_secs(20), "cancel took {took:?}");
    assert_empty(o.path());
}

#[test]
fn cancel_kills_ghostscript() {
    if !has("gs") {
        return;
    }
    let o = out();
    let f = fx();
    let cancel = AtomicBool::new(false);
    let req = JobRequest {
        tool: Tool::Compress,
        inputs: vec![inp(&f.big), inp(&f.big), inp(&f.big)],
        options: ToolOptions::Compress { level: CompressLevel::Extreme },
        output_dir: o.path().to_path_buf(),
        output_stem: "c".into(),
    };
    let t = Instant::now();
    let res = std::thread::scope(|s| {
        s.spawn(|| cancel_after(150, &cancel));
        run(&req, &|_, _| {}, &cancel)
    });
    assert!(matches!(res, Err(Error::Cancelled)), "{res:?}");
    assert!(t.elapsed() < Duration::from_secs(10));
    assert_empty(o.path());
    std::thread::sleep(Duration::from_millis(300));
}

#[test]
fn cancel_before_start_and_during_split() {
    let o = out();
    let f = fx();
    let cancel = AtomicBool::new(true);
    let req = JobRequest {
        tool: Tool::Split,
        inputs: vec![inp(&f.big)],
        options: ToolOptions::Split(SplitOptions::EveryPage),
        output_dir: o.path().to_path_buf(),
        output_stem: "s".into(),
    };
    assert!(matches!(run(&req, &|_, _| {}, &cancel), Err(Error::Cancelled)));
    assert_empty(o.path());
    // cancel from the progress callback after 20 files were written
    let cancel = AtomicBool::new(false);
    let n = std::sync::atomic::AtomicUsize::new(0);
    let res = run(
        &req,
        &|_, msg| {
            if msg.starts_with("Writing part") && n.fetch_add(1, Ordering::Relaxed) == 20 {
                cancel.store(true, Ordering::Relaxed);
            }
        },
        &cancel,
    );
    assert!(matches!(res, Err(Error::Cancelled)), "{res:?}");
    assert_empty(o.path());
}

// ---------------------------------------------------------------- paths and folders

#[test]
fn unicode_names_and_spaces_in_paths() {
    let root = out();
    let dir = root.path().join("Ünï cödé dir").join("fo lder – 日本語");
    std::fs::create_dir_all(&dir).unwrap();
    let outd = root.path().join("Ausgabe (neu) 100%");
    std::fs::create_dir_all(&outd).unwrap();
    let f = fx();
    let names = ["Büro Rechnung März – final (2).pdf", "50% off & more #1.pdf", "日本語のファイル.pdf", "it's \"quoted\".pdf", "-leading dash.pdf", "tab\tname.pdf"];
    let mut copies = Vec::new();
    for n in names {
        let p = dir.join(n);
        std::fs::copy(&f.plain, &p).unwrap();
        copies.push(p);
    }
    let refs: Vec<&Path> = copies.iter().map(|p| p.as_path()).collect();
    let r = job(Tool::Merge, &refs, ToolOptions::Merge, &outd, "Zusammen gefügt – 100%").unwrap();
    assert_eq!(pages(&r.outputs[0]), 7 * names.len());
    assert_eq!(r.outputs[0].file_name().unwrap().to_string_lossy(), "Zusammen gefügt – 100%.pdf");
    for p in &copies {
        let r = job(Tool::Split, &[p], ToolOptions::Split(SplitOptions::Fixed { every: 4 }), &outd, &format!("{}_part", p.file_stem().unwrap().to_string_lossy())).unwrap();
        assert_eq!(r.outputs.len(), 2);
        if has("gs") {
            let r = job(Tool::Compress, &[p], ToolOptions::Compress { level: CompressLevel::Recommended }, &outd, "50%_c").unwrap();
            assert_eq!(pages(&r.outputs[0]), 7);
            let pr = job(Tool::Protect, &[p], ToolOptions::Protect(ProtectOptions { user_password: "pw".into(), ..Default::default() }), &outd, "100% geschützt").unwrap();
            assert_eq!(inspect(&pr.outputs[0], Some("pw")).unwrap().pages, Some(7));
        }
        if has("pdftoppm") {
            render_page(p, 2, 60, None).unwrap();
            let r = job(
                Tool::PdfToImages,
                &[p],
                ToolOptions::PdfToImages(PdfToImagesOptions { pages: "1".into(), dpi: 72, ..Default::default() }),
                &outd,
                "Bild 50%",
            )
            .unwrap();
            assert!(r.outputs[0].is_file());
        }
    }
    // inspect and the image path too
    let img = dir.join("Bild – ü.png");
    std::fs::copy(&f.png, &img).unwrap();
    let r = job(Tool::ImagesToPdf, &[&img], ToolOptions::ImagesToPdf(ImagesToPdfOptions::default()), &outd, "ü").unwrap();
    assert_eq!(pages(&r.outputs[0]), 1);
    // an output stem with a path separator must stay inside the output folder or be rejected
    let evil = job(Tool::Merge, &[&f.plain, &f.plain], ToolOptions::Merge, &outd, "../escape");
    let escaped = root.path().join("escape.pdf");
    assert!(!escaped.exists(), "output escaped the output folder: {evil:?}");
}

#[test]
#[cfg(unix)] // Unix permissions / ps
fn output_dir_read_only_or_missing_gives_io_error() {
    use std::os::unix::fs::PermissionsExt;
    let root = out();
    let f = fx();
    let ro = root.path().join("readonly");
    std::fs::create_dir(&ro).unwrap();
    std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o555)).unwrap();
    let writable_anyway = std::fs::write(ro.join("probe"), b"x").is_ok(); // running as root
    if !writable_anyway {
        for (tool, opts, inputs) in [
            (Tool::Merge, ToolOptions::Merge, vec![f.plain.clone(), f.plain.clone()]),
            (Tool::Split, ToolOptions::Split(SplitOptions::EveryPage), vec![f.plain.clone()]),
            (Tool::Rotate, ToolOptions::Rotate { degrees: Rotation::R90, pages: "".into() }, vec![f.plain.clone()]),
            (Tool::ImagesToPdf, ToolOptions::ImagesToPdf(ImagesToPdfOptions::default()), vec![f.png.clone()]),
        ] {
            let refs: Vec<&Path> = inputs.iter().map(|p| p.as_path()).collect();
            let r = job(tool, &refs, opts, &ro, "x");
            assert!(matches!(r, Err(Error::Io(_))), "{tool:?}: {r:?}");
            if let Err(e) = r {
                assert!(!e.to_string().is_empty());
            }
        }
        if has("gs") {
            let r = job(Tool::Compress, &[&f.plain], ToolOptions::Compress { level: CompressLevel::Low }, &ro, "x");
            assert!(matches!(r, Err(Error::Io(_))), "{r:?}");
            let r = job(Tool::Protect, &[&f.plain], ToolOptions::Protect(ProtectOptions { user_password: "a".into(), ..Default::default() }), &ro, "x");
            assert!(matches!(r, Err(Error::Io(_))), "{r:?}");
        }
        // it became writable halfway: earlier outputs are removed again
    }
    std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_empty(&ro);
    // missing folder, folder that is a file
    let r = job(Tool::Merge, &[&f.plain, &f.plain], ToolOptions::Merge, &root.path().join("nope"), "x");
    assert!(matches!(r, Err(Error::Io(_))), "{r:?}");
    let file_as_dir = root.path().join("file");
    std::fs::write(&file_as_dir, b"x").unwrap();
    let r = job(Tool::Merge, &[&f.plain, &f.plain], ToolOptions::Merge, &file_as_dir, "x");
    assert!(matches!(r, Err(Error::Io(_))), "{r:?}");
}

#[test]
#[cfg(unix)] // Unix permissions / ps
fn output_dir_fills_up_midway_removes_partial_outputs() {
    // The folder turns read-only after the first output file of a multi-file job.
    use std::os::unix::fs::PermissionsExt;
    let root = out();
    let f = fx();
    let d = root.path().join("flip");
    std::fs::create_dir(&d).unwrap();
    if std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o555)).is_ok() && std::fs::write(d.join("p"), b"x").is_ok() {
        return; // root
    }
    std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o755)).unwrap();
    let flipped = AtomicBool::new(false);
    let res = run(
        &JobRequest {
            tool: Tool::Split,
            inputs: vec![inp(&f.plain)],
            options: ToolOptions::Split(SplitOptions::EveryPage),
            output_dir: d.clone(),
            output_stem: "s".into(),
        },
        &|_, msg| {
            if msg.starts_with("Writing part 3") && !flipped.swap(true, Ordering::Relaxed) {
                std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o555)).unwrap();
            }
        },
        &AtomicBool::new(false),
    );
    std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(matches!(res, Err(Error::Io(_))), "{res:?}");
    // the two files written before the failure could not be deleted from a read-only folder;
    // they are reported through the error, nothing half-written (.part) remains
    assert!(ls(&d).iter().all(|n| !n.ends_with(".part")), "{:?}", ls(&d));
}

// ---------------------------------------------------------------- F12 chaining, F13 types, F14 data

#[test]
fn f12_chain_results_into_next_tool() {
    let o = out();
    let f = fx();
    // images -> pdf -> split -> merge -> rotate -> numbers
    let a = job(Tool::ImagesToPdf, &[&f.png, &f.jpg, &f.png], ToolOptions::ImagesToPdf(ImagesToPdfOptions::default()), o.path(), "scan").unwrap();
    let b = job(Tool::Split, &[&a.outputs[0]], ToolOptions::Split(SplitOptions::EveryPage), o.path(), "sp").unwrap();
    assert_eq!(b.outputs.len(), 3);
    let refs: Vec<&Path> = b.outputs.iter().rev().map(|p| p.as_path()).collect();
    let c = job(Tool::Merge, &refs, ToolOptions::Merge, o.path(), "back").unwrap();
    let d = job(Tool::Rotate, &[&c.outputs[0]], ToolOptions::Rotate { degrees: Rotation::R90, pages: "".into() }, o.path(), "rot").unwrap();
    let e = job(Tool::PageNumbers, &[&d.outputs[0]], ToolOptions::PageNumbers(PageNumberOptions::default()), o.path(), "num").unwrap();
    assert_valid(&e.outputs[0], 3);
    if has("gs") {
        let g = job(Tool::Compress, &[&e.outputs[0]], ToolOptions::Compress { level: CompressLevel::Recommended }, o.path(), "cmp").unwrap();
        let h = job(Tool::Protect, &[&g.outputs[0]], ToolOptions::Protect(ProtectOptions { user_password: "chain".into(), ..Default::default() }), o.path(), "prot").unwrap();
        let i = job_in(Tool::Unlock, vec![inp_pw(&h.outputs[0], "chain")], ToolOptions::Unlock, o.path(), "unl").unwrap();
        assert_valid(&i.outputs[0], 3);
    }
    if has("pdftoppm") {
        // PDF -> images -> PDF
        let im = job(Tool::PdfToImages, &[&e.outputs[0]], ToolOptions::PdfToImages(PdfToImagesOptions { dpi: 72, ..Default::default() }), o.path(), "pg").unwrap();
        assert_eq!(im.outputs.len(), 3);
        let refs: Vec<&Path> = im.outputs.iter().map(|p| p.as_path()).collect();
        let back = job(Tool::ImagesToPdf, &refs, ToolOptions::ImagesToPdf(ImagesToPdfOptions::default()), o.path(), "round").unwrap();
        assert_valid(&back.outputs[0], 3);
    }
}

#[test]
fn f13_every_tool_rejects_wrong_kinds_and_accepts_its_own() {
    let o = out();
    let f = fx();
    let txt = o.path().join("a.txt");
    std::fs::write(&txt, "x").unwrap();
    let dest = out();
    for m in tools() {
        let opts = ToolOptions::default_for(m.tool);
        let inputs: Vec<&Path> = vec![&txt; m.min_files.max(1)];
        let r = job(m.tool, &inputs, opts.clone(), dest.path(), "x");
        assert!(matches!(r, Err(Error::WrongType(_))), "{:?} accepted a .txt: {r:?}", m.tool);
        // a kind it does not accept (pick one that is not in `accepts`)
        let other: &Path = if m.accepts.contains(&FileKind::Pdf) { &f.png } else { &f.plain };
        let inputs: Vec<&Path> = vec![other; m.min_files.max(1)];
        let r = job(m.tool, &inputs, opts, dest.path(), "x");
        assert!(matches!(r, Err(Error::WrongType(_))), "{:?}: {r:?}", m.tool);
    }
    assert_empty(dest.path());
    // metadata table sanity
    for m in tools() {
        assert!(m.min_files >= 1 && (m.multi || m.min_files == 1), "{:?}", m.tool);
        assert!(!m.accepts.is_empty());
        assert_eq!(m.tool.meta().name, m.name);
    }
    // options don't match the tool
    assert!(matches!(job(Tool::Merge, &[&f.plain, &f.plain], ToolOptions::Repair, dest.path(), "x"), Err(Error::InvalidOptions(_))));
    // single-file tools refuse several inputs
    assert!(matches!(job(Tool::Split, &[&f.plain, &f.plain], ToolOptions::Split(SplitOptions::EveryPage), dest.path(), "x"), Err(Error::InvalidOptions(_))));
}

#[test]
fn f13_file_kind_by_extension_edge_cases() {
    let k = |s: &str| FileKind::from_path(Path::new(s));
    assert_eq!(k("a.PDF"), FileKind::Pdf);
    assert_eq!(k("/x/y.z/doc.Pdf"), FileKind::Pdf);
    assert_eq!(k("noext"), FileKind::Other);
    assert_eq!(k(".pdf"), FileKind::Other, "a dotfile named .pdf has no extension");
    assert_eq!(k("archive.pdf.zip"), FileKind::Other);
    assert_eq!(k("scan.TIFF"), FileKind::Image);
    assert_eq!(k("scan.tif"), FileKind::Image);
    assert_eq!(k("sheet.XLSX"), FileKind::Office);
    assert_eq!(k("page.HTM"), FileKind::Html);
    assert_eq!(k("ünï.pdf"), FileKind::Pdf);
    assert_eq!(k(""), FileKind::Other);
}

#[test]
fn f14_jobs_serialize_without_passwords() {
    let secret = "TOP-SECRET-123";
    let req = JobRequest {
        tool: Tool::Protect,
        inputs: vec![Input { path: "/a b/ü.pdf".into(), password: Some(secret.into()) }],
        options: ToolOptions::Protect(ProtectOptions { user_password: secret.into(), owner_password: secret.into(), ..Default::default() }),
        output_dir: "/tmp".into(),
        output_stem: "x".into(),
    };
    let json = serde_json::to_string(&req).unwrap();
    assert!(!json.contains(secret), "{json}");
    let back: JobRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(back.inputs[0].path, PathBuf::from("/a b/ü.pdf"));
    assert!(back.inputs[0].password.is_none());
    let ToolOptions::Protect(p) = back.options else { panic!() };
    assert!(p.user_password.is_empty() && p.owner_password.is_empty());
    // every default option set round-trips (recent.json "run again")
    for m in tools() {
        let o = ToolOptions::default_for(m.tool);
        let s = serde_json::to_string(&o).unwrap();
        let _: ToolOptions = serde_json::from_str(&s).unwrap();
    }
}

#[test]
fn parse_ranges_property_checks() {
    // everything parse_ranges accepts is inside 1..=n, unique, and non-empty
    let samples = ["", " ", "1", "1-", "-1", "2-3,1", "1,1,1", "1-3,2-5", " 4 - 6 ", "1;3", "1,,3", ",1", "9", "0", "a", "1-a", "--", "3-1", "1-2-3", "１", "+1", "1e1"];
    for n in [0u32, 1, 3, 10] {
        for s in samples {
            if let Ok(v) = parse_ranges(s, n) {
                assert!(!v.is_empty() || n == 0, "{s:?} n={n}");
                assert!(v.iter().all(|p| (1..=n).contains(p)), "{s:?} n={n}: {v:?}");
                let mut u = v.clone();
                u.sort();
                u.dedup();
                assert_eq!(u.len(), v.len(), "{s:?}: duplicates");
            }
        }
    }
    assert_eq!(parse_ranges("", 0).unwrap(), Vec::<u32>::new());
    assert!(parse_ranges("1", 0).is_err());
    assert!(parse_ranges("1-", 0).is_err());
    let msg = |s: &str, n| parse_ranges(s, n).unwrap_err().to_string();
    assert!(msg("9", 5).contains("5 pages"));
    assert!(msg("9", 1).contains("1 page)"));
    assert!(msg("4-2", 9).contains("backwards"));
    assert!(parse_ranges("4294967296", 5).is_err());
    assert!(parse_ranges("99999999999999999999", 5).is_err());
}

#[test]
fn deps_report_matches_path() {
    let d = deps();
    for dep in &d.deps {
        assert_eq!(dep.found, which_exists(dep.program), "{}", dep.program);
        assert!(!dep.package.is_empty());
    }
    assert!(!d.has("definitely-not-a-program"));
}

fn which_exists(p: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|paths| std::env::split_paths(&paths).any(|d| d.join(p).is_file()))
}
