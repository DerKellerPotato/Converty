use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Dep {
    /// Program name, e.g. "gs".
    pub program: &'static str,
    /// Package to install on Debian/Ubuntu/Pop!_OS, Fedora and Arch (usually the same).
    pub package: &'static str,
    pub found: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DepStatus {
    pub deps: Vec<Dep>,
}

impl DepStatus {
    pub fn has(&self, program: &str) -> bool {
        self.deps.iter().any(|d| d.program == program && d.found)
    }
}

/// Looks up gs, pdftoppm, pdfimages and soffice on PATH (also inside Flatpak: /app/bin).
pub fn deps() -> DepStatus {
    const WANTED: [(&str, &str); 4] = [
        ("gs", "ghostscript"),
        ("pdftoppm", "poppler-utils"),
        ("pdfimages", "poppler-utils"),
        ("soffice", "libreoffice"),
    ];
    DepStatus {
        deps: WANTED
            .iter()
            .map(|&(program, package)| Dep { program, package, found: find_program(program).is_some() })
            .collect(),
    }
}

/// Finds a program on PATH, in `/app/bin` (Flatpak), or in the usual install
/// folders on Windows and macOS (GUI apps there often get a minimal PATH).
pub(crate) fn find_program(program: &str) -> Option<std::path::PathBuf> {
    if let Ok(p) = which::which(program) {
        return Some(p);
    }
    platform_candidates(program).into_iter().find(|p| p.is_file())
}

#[cfg(target_os = "linux")]
fn platform_candidates(program: &str) -> Vec<std::path::PathBuf> {
    vec![std::path::Path::new("/app/bin").join(program)]
}

#[cfg(target_os = "macos")]
fn platform_candidates(program: &str) -> Vec<std::path::PathBuf> {
    use std::path::PathBuf;
    if program == "soffice" {
        return vec![PathBuf::from("/Applications/LibreOffice.app/Contents/MacOS/soffice")];
    }
    ["/opt/homebrew/bin", "/usr/local/bin", "/opt/local/bin"].iter().map(|d| PathBuf::from(d).join(program)).collect()
}

#[cfg(windows)]
fn platform_candidates(program: &str) -> Vec<std::path::PathBuf> {
    use std::path::PathBuf;
    let roots: Vec<PathBuf> = ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"]
        .iter()
        .filter_map(|v| std::env::var_os(v).map(PathBuf::from))
        .collect();
    let mut out = Vec::new();
    match program {
        "gs" => {
            for exe in ["gswin64c.exe", "gswin32c.exe"] {
                if let Ok(p) = which::which(exe) {
                    out.push(p);
                }
            }
            // Ghostscript installs into <ProgramFiles>\gs\gs<version>\bin; prefer the newest.
            for root in &roots {
                let mut versions: Vec<PathBuf> = std::fs::read_dir(root.join("gs"))
                    .map(|rd| rd.flatten().map(|e| e.path()).collect())
                    .unwrap_or_default();
                versions.sort();
                for v in versions.iter().rev() {
                    out.push(v.join("bin").join("gswin64c.exe"));
                    out.push(v.join("bin").join("gswin32c.exe"));
                }
            }
        }
        "soffice" => out.extend(roots.iter().map(|r| r.join("LibreOffice").join("program").join("soffice.exe"))),
        _ => {
            // Poppler for Windows is unpacked anywhere; check a few common places.
            let exe = format!("{program}.exe");
            for base in roots.iter().cloned().chain(std::env::var_os("LOCALAPPDATA").map(PathBuf::from)) {
                out.push(base.join("poppler").join("Library").join("bin").join(&exe));
                out.push(base.join("poppler").join("bin").join(&exe));
            }
            out.push(PathBuf::from(r"C:\poppler\Library\bin").join(&exe));
        }
    }
    out
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn platform_candidates(_program: &str) -> Vec<std::path::PathBuf> {
    Vec::new()
}
