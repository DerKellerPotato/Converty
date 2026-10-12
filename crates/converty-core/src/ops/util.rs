//! Shared helpers: subprocesses, cancellation, output files, PDF loading.

use crate::{Error, Result};
use lopdf::Document;
use std::ffi::{OsStr, OsString};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub(crate) fn check(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::Relaxed) {
        Err(Error::Cancelled)
    } else {
        Ok(())
    }
}

pub(crate) fn package_for(program: &str) -> &'static str {
    match program {
        "gs" => "ghostscript",
        "pdftoppm" | "pdfimages" => "poppler-utils",
        "soffice" => "libreoffice",
        _ => "",
    }
}

pub(crate) fn require(program: &'static str) -> Result<PathBuf> {
    crate::deps::find_program(program)
        .ok_or(Error::MissingDependency { program, package: package_for(program) })
}

pub(crate) fn os<S: AsRef<OsStr>>(s: S) -> OsString {
    s.as_ref().to_os_string()
}

pub(crate) struct CmdOut {
    pub stdout: String,
    pub stderr: String,
    pub success: bool,
}

impl CmdOut {
    /// Both streams, lowercase, for matching known messages.
    pub fn text_lower(&self) -> String {
        format!("{}\n{}", self.stderr, self.stdout).to_lowercase()
    }
    pub fn message(&self) -> String {
        let src = if self.stderr.trim().is_empty() { &self.stdout } else { &self.stderr };
        let lines: Vec<&str> = src.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
        let start = lines.len().saturating_sub(4);
        let m = lines[start..].join(" ");
        if m.is_empty() {
            "unknown error".into()
        } else {
            m
        }
    }
}

/// Runs a program, polling (2 ms growing to 100 ms) and killing it when `cancel` is set.
pub(crate) fn run_cmd(program: &'static str, args: &[OsString], cancel: &AtomicBool) -> Result<CmdOut> {
    let exe = require(program)?;
    check(cancel)?;
    let mut cmd = Command::new(exe);
    #[cfg(windows)]
    {
        // CREATE_NO_WINDOW: no console window flashing up for every gs/pdftoppm call.
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let mut child = cmd
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Error::MissingDependency { program, package: package_for(program) }
            } else {
                Error::Io(e)
            }
        })?;
    fn drain<R: Read + Send + 'static>(r: Option<R>) -> std::thread::JoinHandle<Vec<u8>> {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut r) = r {
                let _ = r.read_to_end(&mut buf);
            }
            buf
        })
    }
    let out_h = drain(child.stdout.take());
    let err_h = drain(child.stderr.take());
    // Short polls first: poppler calls for thumbnails finish within a few milliseconds.
    let mut wait = Duration::from_millis(2);
    let status = loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = out_h.join();
            let _ = err_h.join();
            return Err(Error::Cancelled);
        }
        match child.try_wait()? {
            Some(s) => break s,
            None => {
                std::thread::sleep(wait);
                wait = (wait * 2).min(Duration::from_millis(100));
            }
        }
    };
    let stdout = String::from_utf8_lossy(&out_h.join().unwrap_or_default()).into_owned();
    let stderr = String::from_utf8_lossy(&err_h.join().unwrap_or_default()).into_owned();
    Ok(CmdOut { stdout, stderr, success: status.success() })
}

/// Escapes `%` so gs does not read it as a page-number format.
fn gs_out_arg(out: &Path) -> OsString {
    let mut s = OsString::from("-sOutputFile=");
    s.push(out.as_os_str().to_string_lossy().replace('%', "%%"));
    s
}

/// Writes gs arguments, each double-quoted, to a private (0600) temp file.
fn write_gs_argfile(tokens: &[String]) -> Result<tempfile::NamedTempFile> {
    use std::io::Write;
    let mut f = tempfile::Builder::new().prefix("converty-gs-").suffix(".args").tempfile()?;
    for t in tokens {
        // gs treats `\"` as an escaped quote and every other backslash literally
        let q = t.replace('"', "\\\"");
        writeln!(f, "\"{q}\"")?;
    }
    f.flush()?;
    Ok(f)
}

/// Runs Ghostscript's pdfwrite from `input` to `output`.
pub(crate) fn run_gs(
    extra: &[OsString],
    input: &Path,
    output: &Path,
    password: Option<&str>,
    cancel: &AtomicBool,
) -> Result<()> {
    run_gs_with(extra, &[], input, output, password, cancel)
}

/// Like [`run_gs`], with PostScript files run before the input (after the output is set).
pub(crate) fn run_gs_with(
    extra: &[OsString],
    pre_files: &[PathBuf],
    input: &Path,
    output: &Path,
    password: Option<&str>,
    cancel: &AtomicBool,
) -> Result<()> {
    let mut args: Vec<OsString> =
        ["-dBATCH", "-dNOPAUSE", "-dQUIET", "-dSAFER", "-sDEVICE=pdfwrite"].iter().map(os).collect();
    // Passwords must not show up in `ps`: they go into a 0600 argument file (`gs @file`)
    // that lives only for the duration of the call.
    let mut secrets: Vec<String> = Vec::new();
    if let Some(pw) = password.filter(|p| !p.is_empty()) {
        secrets.push(format!("-sPDFPassword={pw}"));
    }
    for a in extra {
        let s = a.to_string_lossy();
        if ["-sOwnerPassword=", "-sUserPassword=", "-sPDFPassword="].iter().any(|k| s.starts_with(k)) {
            secrets.push(s.into_owned());
        } else {
            args.push(a.clone());
        }
    }
    let argfile = if secrets.is_empty() { None } else { Some(write_gs_argfile(&secrets)?) };
    if let Some(f) = &argfile {
        let mut a = OsString::from("@");
        a.push(f.path());
        args.push(a);
    }
    args.push(gs_out_arg(output));
    for f in pre_files {
        args.push(os("-f"));
        args.push(f.as_os_str().to_os_string());
    }
    args.push(os("-f"));
    args.push(input.as_os_str().to_os_string());
    let out = run_cmd("gs", &args, cancel)?;
    let low = out.text_lower();
    // gs often exits 0 after printing an error, so look at the text as well
    if low.contains("requires a password") || low.contains("password did not work") || low.contains("cannot decrypt") {
        return Err(if password.is_none_or(str::is_empty) {
            Error::Encrypted(input.to_path_buf())
        } else {
            Error::WrongPassword(input.to_path_buf())
        });
    }
    if !out.success || low.contains("no pages will be processed") || file_size(output) == 0 {
        return Err(Error::External { program: "gs", message: out.message() });
    }
    Ok(())
}

pub(crate) fn pdf_err(e: lopdf::Error) -> Error {
    Error::Pdf(e.to_string())
}

/// Loads a PDF fully decrypted. Encrypted files with a non-empty user password are
/// decrypted through Ghostscript into a temp file first.
pub(crate) fn load_pdf(path: &Path, password: Option<&str>, cancel: &AtomicBool) -> Result<Document> {
    let mut doc = Document::load(path).map_err(|e| match e {
        lopdf::Error::IO(io) if io.kind() == std::io::ErrorKind::NotFound => Error::Io(io),
        _ => Error::Corrupt(path.to_path_buf()),
    })?;
    if !doc.trailer.has(b"Encrypt") {
        return Ok(doc);
    }
    if doc.encryption_state.is_some() {
        // opened with the empty user password: already decrypted in memory
        strip_encryption(&mut doc);
        return Ok(doc);
    }
    let pw = match password {
        Some(p) if !p.is_empty() => p,
        _ => return Err(Error::Encrypted(path.to_path_buf())),
    };
    // lopdf and Ghostscript disagree on how non-ASCII passwords are encoded (Ghostscript
    // matches what other tools write), so a lopdf miss is not final: Ghostscript decides and
    // reports WrongPassword itself.
    let _ = doc.authenticate_password(pw);
    let tmp = tempfile::Builder::new().suffix(".pdf").tempfile()?;
    run_gs(&[], path, tmp.path(), Some(pw), cancel)?;
    let mut doc = Document::load(tmp.path()).map_err(|_| Error::Corrupt(path.to_path_buf()))?;
    if doc.trailer.has(b"Encrypt") {
        strip_encryption(&mut doc);
    }
    Ok(doc)
}

fn strip_encryption(doc: &mut Document) {
    if let Some(obj) = doc.trailer.remove(b"Encrypt") {
        if let Ok(id) = obj.as_reference() {
            doc.objects.remove(&id);
        }
    }
    doc.encryption_state = None;
}

pub(crate) fn file_size(p: &Path) -> u64 {
    std::fs::metadata(p).map(|m| m.len()).unwrap_or(0)
}

/// Writes a file through a temp file in `dir` and renames it to a unique name
/// (`stem.ext`, `stem (2).ext`, ...). Never overwrites.
pub(crate) fn write_unique(
    dir: &Path,
    stem: &str,
    ext: &str,
    write: impl FnOnce(&Path) -> Result<()>,
) -> Result<PathBuf> {
    // The name is user input: it must stay a single file name inside `dir`.
    let stem: String = stem.chars().map(|c| if matches!(c, '/' | '\0') { '_' } else { c }).collect();
    let stem = if stem.trim().is_empty() { "output" } else { stem.as_str() };
    let tmp = tempfile::Builder::new().prefix(".converty-").suffix(".part").tempfile_in(dir)?;
    write(tmp.path())?;
    let ext_part = if ext.is_empty() { String::new() } else { format!(".{ext}") };
    let mut n = 1u32;
    let mut tmp = tmp;
    loop {
        let name = if n == 1 { format!("{stem}{ext_part}") } else { format!("{stem} ({n}){ext_part}") };
        let target = dir.join(name);
        match tmp.persist_noclobber(&target) {
            Ok(_) => return Ok(target),
            Err(e) if e.error.kind() == std::io::ErrorKind::AlreadyExists => {
                tmp = e.file;
                n += 1;
            }
            Err(e) => return Err(Error::Io(e.error)),
        }
    }
}

pub(crate) fn file_stem(p: &Path) -> String {
    p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "output".into())
}
