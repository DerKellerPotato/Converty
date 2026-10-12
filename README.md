<div align="center">

<img src="res/icons/png/converty-128.png" width="128" alt="Converty icon">

# Converty

**An offline, open-source PDF toolbox for Linux, Windows and macOS.**

Merge, split, compress, convert, watermark, sign and password-protect PDF files,
without uploading a single page anywhere.

[![CI](https://github.com/DerKellerPotato/Converty/actions/workflows/ci.yml/badge.svg)](https://github.com/DerKellerPotato/Converty/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/DerKellerPotato/Converty?sort=semver)](https://github.com/DerKellerPotato/Converty/releases/latest)
[![License: GPL v3+](https://img.shields.io/badge/license-GPL--3.0--or--later-blue.svg)](LICENSE)

[**Download**](https://github.com/DerKellerPotato/Converty/releases/latest) ·
[Features](#features) ·
[Install](#install) ·
[Build from source](#build-from-source)

</div>

---

Online PDF services are handy, but they mean sending your contracts, invoices and scans to someone else's server. Converty gives you the same everyday tools as a native desktop app built with [libcosmic](https://github.com/pop-os/libcosmic) (the toolkit behind the COSMIC desktop). Every file is processed on your own machine.

## Features

| Category | Tools |
| --- | --- |
| **Organize** | Merge PDFs · Split PDF (by ranges, every *n* pages or into single pages) · Remove pages · Extract pages · Reorder pages and insert blank pages |
| **Optimize** | Compress PDF (three quality levels) · Repair damaged PDFs |
| **Convert to PDF** | Images (JPG, PNG, WebP, TIFF, BMP, GIF) · Office documents (Word, Excel, PowerPoint, ODF) · HTML |
| **Convert from PDF** | PDF to JPG/PNG (or extract embedded images) · PDF to Word, ODT, Excel or PowerPoint · PDF to PDF/A (1b, 2b, 3b) |
| **Edit** | Rotate pages · Add page numbers · Add text or image watermarks · Crop pages |
| **Security** | Sign with a typed name or a signature image · Protect with a password and permissions · Unlock |

And around the tools:

- **Page thumbnails** to pick, reorder and rotate pages visually.
- **Chain tools**: send a result straight into the next tool.
- **Recent jobs** and a choice of output folder (next to the input, a fixed folder, or ask every time).
- **Follows your system theme**: dark mode and accent colour.
- **Private by design**: no uploads, no analytics, no account, works fully offline.

## Install

Grab the file for your system from the [latest release](https://github.com/DerKellerPotato/Converty/releases/latest).

| System | Files |
| --- | --- |
| **Windows 10/11 (x64)** | `…-windows-x64-setup.exe` (installer) · `…-windows-x64.msi` · `…-windows-x64-portable.zip` |
| **macOS 11+ (Apple Silicon and Intel)** | `…-macos-universal.dmg` · `…-macos-universal.zip` |
| **Debian, Ubuntu, Pop!_OS, Mint** | `converty_…_amd64.deb` / `_arm64.deb` |
| **Fedora, openSUSE, RHEL** | `converty-….x86_64.rpm` / `.aarch64.rpm` |
| **Arch Linux, Manjaro** | `converty-…-x86_64.pkg.tar.zst` (or build [`packaging/arch/PKGBUILD`](packaging/arch/PKGBUILD)) |
| **Any Linux** | `Converty-…-x86_64.AppImage` / `-aarch64.AppImage` · `converty-…-linux-*.tar.gz` · `Converty-…-x86_64.flatpak` |

Every release ships a `SHA256SUMS.txt` to verify your download.

### Helper programs

Converty does the PDF work itself where it can (merge, split, pages, rotate, crop, watermarks, page numbers, signing, images to PDF, repair). A few tools use well-known open-source programs. Tools whose helper is missing are shown as unavailable, and **Settings** lists what is installed.

| Program | Used for |
| --- | --- |
| [Ghostscript](https://ghostscript.com/) | Compress, PDF/A, Protect, Unlock |
| [Poppler](https://poppler.freedesktop.org/) (`pdftoppm`, `pdfimages`) | Page thumbnails, PDF to images |
| [LibreOffice](https://www.libreoffice.org/) | Office and HTML conversions (optional) |

<details>
<summary><b>Linux</b></summary>

The `.deb`, `.rpm` and Arch packages pull in Ghostscript and Poppler automatically. For the AppImage and tar.gz:

```sh
# Debian / Ubuntu / Pop!_OS
sudo apt install ghostscript poppler-utils libreoffice-core
# Fedora
sudo dnf install ghostscript poppler-utils libreoffice-core
# Arch
sudo pacman -S ghostscript poppler libreoffice-fresh
```

The Flatpak bundles Ghostscript and Poppler. LibreOffice is not included there, so Office and HTML conversions are not available in the Flatpak build.

</details>

<details>
<summary><b>Windows</b></summary>

Install with [winget](https://learn.microsoft.com/windows/package-manager/):

```powershell
winget install ArtifexSoftware.GhostScript oschwartz10612.Poppler TheDocumentFoundation.LibreOffice
```

Or download them by hand: [Ghostscript](https://ghostscript.com/releases/gsdnld.html), [Poppler for Windows](https://github.com/oschwartz10612/poppler-windows/releases) and [LibreOffice](https://www.libreoffice.org/download/). Converty finds Ghostscript and LibreOffice in their default install folders. Poppler is found on `PATH` or in `C:\poppler\Library\bin`.

Windows SmartScreen may warn about the installer because it is not code-signed. Choose **More info → Run anyway**.

</details>

<details>
<summary><b>macOS</b></summary>

Install the helpers with [Homebrew](https://brew.sh/):

```sh
brew install ghostscript poppler
brew install --cask libreoffice   # optional, for Office conversions
```

The app is not notarized by Apple. After dragging Converty to *Applications*, open it once with right-click → **Open**, or run:

```sh
xattr -dr com.apple.quarantine /Applications/Converty.app
```

</details>

## Build from source

You need a recent stable [Rust](https://rustup.rs/) toolchain. On Linux, also install the development headers: `sudo apt install pkg-config libxkbcommon-dev libwayland-dev` (or the equivalent for your distribution).

```sh
git clone https://github.com/DerKellerPotato/Converty.git
cd Converty
cargo run --release -p converty
```

With [`just`](https://github.com/casey/just) on Linux:

```sh
just build            # release build
just check            # clippy + all tests
just install-local    # install into ~/.local (no sudo)
sudo just install     # install into /usr
just appimage         # build an AppImage into dist/ (needs appimagetool)
```

Packaging scripts for every release format live in [`packaging/`](packaging) and [`flatpak/`](flatpak). The [release workflow](.github/workflows/release.yml) shows how each one is built.

### Project layout

```
crates/converty-core   PDF engine: every tool, options, page ranges, helper detection (no GUI)
crates/converty        libcosmic desktop app
res/                   desktop entry, AppStream metainfo, icons
packaging/             AppImage, Windows (Inno Setup, WiX), macOS bundle, Arch PKGBUILD
flatpak/               Flatpak manifest
```

### Tests

```sh
cargo test --workspace
```

The suite covers each tool end to end, including odd inputs (damaged files, Unicode paths, awkward passwords, cancellation, 200-page documents). Tests that need Ghostscript, Poppler or LibreOffice skip themselves when the program is missing. Set `CONVERTY_TEST_SOFFICE=1` to also run the slow LibreOffice tests.

## Releasing

Bump `version` in `Cargo.toml` and add a `<release>` entry to `res/io.github.converty.Converty.metainfo.xml`. Then push a tag:

```sh
git tag v0.2.0 && git push origin v0.2.0
```

GitHub Actions builds every package for every platform and publishes them as a GitHub release.

## Contributing

Bug reports and pull requests are welcome. Please run `cargo clippy --workspace --all-targets` and `cargo test --workspace` before opening a PR.

## License

Converty is free software, licensed under the [GNU General Public License v3.0 or later](LICENSE).
