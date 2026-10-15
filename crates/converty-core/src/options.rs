//! Options for every tool. `ToolOptions::default_for(tool)` gives the UI its starting state.

use crate::tools::Tool;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ToolOptions {
    Merge,
    Split(SplitOptions),
    /// Page selection text, e.g. "1,3-5,8-" (1-based, "-" open end).
    RemovePages { pages: String },
    ExtractPages { pages: String, separate_files: bool },
    Organize { pages: Vec<PageOp> },
    Compress { level: CompressLevel },
    Repair,
    ImagesToPdf(ImagesToPdfOptions),
    OfficeToPdf,
    HtmlToPdf,
    PdfToImages(PdfToImagesOptions),
    PdfToOffice { target: OfficeTarget },
    PdfToPdfA { level: PdfALevel },
    /// Rotation is added to the existing page rotation. Empty `pages` = all.
    Rotate { degrees: Rotation, pages: String },
    PageNumbers(PageNumberOptions),
    Watermark(WatermarkOptions),
    Crop(CropOptions),
    Sign(SignOptions),
    Unlock,
    Protect(ProtectOptions),
    ConvertVideo(ConvertVideoOptions),
    VideoToAudio { format: AudioFormat },
}

impl ToolOptions {
    pub fn default_for(tool: Tool) -> Self {
        match tool {
            Tool::Merge => Self::Merge,
            Tool::Split => Self::Split(SplitOptions::default()),
            Tool::RemovePages => Self::RemovePages { pages: String::new() },
            Tool::ExtractPages => Self::ExtractPages { pages: String::new(), separate_files: false },
            Tool::Organize => Self::Organize { pages: Vec::new() },
            Tool::Compress => Self::Compress { level: CompressLevel::Recommended },
            Tool::Repair => Self::Repair,
            Tool::ImagesToPdf => Self::ImagesToPdf(ImagesToPdfOptions::default()),
            Tool::OfficeToPdf => Self::OfficeToPdf,
            Tool::HtmlToPdf => Self::HtmlToPdf,
            Tool::PdfToImages => Self::PdfToImages(PdfToImagesOptions::default()),
            Tool::PdfToOffice => Self::PdfToOffice { target: OfficeTarget::Docx },
            Tool::PdfToPdfA => Self::PdfToPdfA { level: PdfALevel::B2 },
            Tool::Rotate => Self::Rotate { degrees: Rotation::R90, pages: String::new() },
            Tool::PageNumbers => Self::PageNumbers(PageNumberOptions::default()),
            Tool::Watermark => Self::Watermark(WatermarkOptions::default()),
            Tool::Crop => Self::Crop(CropOptions::default()),
            Tool::Sign => Self::Sign(SignOptions::default()),
            Tool::Unlock => Self::Unlock,
            Tool::Protect => Self::Protect(ProtectOptions::default()),
            Tool::ConvertVideo => Self::ConvertVideo(ConvertVideoOptions::default()),
            Tool::VideoToAudio => Self::VideoToAudio { format: AudioFormat::Mp3 },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SplitOptions {
    /// Each range string ("1-3", "4-", "7") becomes one file, unless `merge_all`.
    Ranges { ranges: Vec<String>, merge_all: bool },
    /// Every `n` pages a new file.
    Fixed { every: u32 },
    /// One file per page.
    EveryPage,
}
impl Default for SplitOptions {
    fn default() -> Self {
        Self::Ranges { ranges: vec!["1-1".into()], merge_all: false }
    }
}

/// One page in the new order of an Organize job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PageOp {
    /// 1-based source page, plus extra rotation.
    Page { source: u32, rotate: Rotation },
    /// A blank page the size of the previous page (A4 if first).
    Blank,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Rotation {
    #[default]
    R0,
    R90,
    R180,
    R270,
}
impl Rotation {
    pub fn degrees(self) -> i64 {
        match self { Self::R0 => 0, Self::R90 => 90, Self::R180 => 180, Self::R270 => 270 }
    }
    pub fn cw(self) -> Self {
        match self { Self::R0 => Self::R90, Self::R90 => Self::R180, Self::R180 => Self::R270, Self::R270 => Self::R0 }
    }
    pub fn ccw(self) -> Self {
        self.cw().cw().cw()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompressLevel {
    /// Strongest, lowest image quality (~72 dpi).
    Extreme,
    /// Good balance (~150 dpi).
    Recommended,
    /// Light, keeps quality (~300 dpi).
    Low,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PageSize { Fit, A4, Letter }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Orientation { Auto, Portrait, Landscape }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Margin { None, Small, Big }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImagesToPdfOptions {
    pub page_size: PageSize,
    pub orientation: Orientation,
    pub margin: Margin,
    /// One PDF for all images (true) or one PDF per image.
    pub merge: bool,
}
impl Default for ImagesToPdfOptions {
    fn default() -> Self {
        Self { page_size: PageSize::A4, orientation: Orientation::Auto, margin: Margin::None, merge: true }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImageFormat { Jpg, Png }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PdfToImagesOptions {
    /// true = render pages; false = extract embedded images.
    pub render_pages: bool,
    pub format: ImageFormat,
    /// 72..=600.
    pub dpi: u32,
    /// Empty = all pages.
    pub pages: String,
}
impl Default for PdfToImagesOptions {
    fn default() -> Self {
        Self { render_pages: true, format: ImageFormat::Jpg, dpi: 150, pages: String::new() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OfficeTarget { Docx, Odt, Xlsx, Pptx }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PdfALevel { B1, B2, B3 }

/// 3x3 placement grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Position {
    TopLeft, TopCenter, TopRight,
    MiddleLeft, Center, MiddleRight,
    BottomLeft, BottomCenter, BottomRight,
}
impl Position {
    pub const ALL: [Position; 9] = [
        Self::TopLeft, Self::TopCenter, Self::TopRight,
        Self::MiddleLeft, Self::Center, Self::MiddleRight,
        Self::BottomLeft, Self::BottomCenter, Self::BottomRight,
    ];
}

/// RGB, 0..=255.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rgb(pub u8, pub u8, pub u8);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageNumberOptions {
    pub position: Position,
    /// Distance from the page edge in points.
    pub margin: f32,
    pub first_number: u32,
    /// Pages that get a number; empty = all.
    pub pages: String,
    /// Template with `{n}` (number) and `{p}` (total), e.g. "Page {n} of {p}".
    pub template: String,
    pub font_size: f32,
    pub color: Rgb,
    /// Mirror left/right positions on even pages.
    pub facing_pages: bool,
}
impl Default for PageNumberOptions {
    fn default() -> Self {
        Self {
            position: Position::BottomCenter,
            margin: 28.0,
            first_number: 1,
            pages: String::new(),
            template: "{n}".into(),
            font_size: 11.0,
            color: Rgb(0, 0, 0),
            facing_pages: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WatermarkContent {
    Text { text: String, font_size: f32, color: Rgb, bold: bool },
    /// PNG or JPG; `scale` relative to page width (0.05..=1.0).
    Image { path: std::path::PathBuf, scale: f32 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatermarkOptions {
    pub content: WatermarkContent,
    pub position: Position,
    /// 0.0 (invisible) ..= 1.0 (solid).
    pub opacity: f32,
    pub rotation_deg: f32,
    /// Repeat across the whole page.
    pub mosaic: bool,
    /// true = below page content.
    pub below_content: bool,
    pub pages: String,
}
impl Default for WatermarkOptions {
    fn default() -> Self {
        Self {
            content: WatermarkContent::Text { text: "CONFIDENTIAL".into(), font_size: 48.0, color: Rgb(200, 30, 30), bold: true },
            position: Position::Center,
            opacity: 0.4,
            rotation_deg: 45.0,
            mosaic: false,
            below_content: false,
            pages: String::new(),
        }
    }
}

/// Margins to trim, in points, applied to the CropBox.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CropOptions {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
    pub pages: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SignatureKind {
    /// Rendered with a built-in script-like PDF font (Times-Italic).
    Typed { name: String },
    /// PNG (transparency kept) or JPG.
    Image { path: std::path::PathBuf },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignOptions {
    pub signature: SignatureKind,
    /// 1-based page; 0 = last page.
    pub page: u32,
    pub position: Position,
    /// Width of the signature box in points.
    pub width: f32,
    /// Also stamp today's date under the signature.
    pub add_date: bool,
}
impl Default for SignOptions {
    fn default() -> Self {
        Self { signature: SignatureKind::Typed { name: String::new() }, page: 0, position: Position::BottomRight, width: 160.0, add_date: true }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Permissions {
    pub allow_print: bool,
    pub allow_copy: bool,
    pub allow_modify: bool,
    pub allow_annotate: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtectOptions {
    #[serde(skip)]
    pub user_password: String,
    /// Empty = same as user password.
    #[serde(skip)]
    pub owner_password: String,
    pub permissions: Permissions,
}
impl Default for ProtectOptions {
    fn default() -> Self {
        Self {
            user_password: String::new(),
            owner_password: String::new(),
            permissions: Permissions { allow_print: true, allow_copy: true, allow_modify: false, allow_annotate: true },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VideoFormat { Mp4, Mkv, Webm, Mov, Avi }
impl VideoFormat {
    pub const ALL: [VideoFormat; 5] = [Self::Mp4, Self::Mkv, Self::Webm, Self::Mov, Self::Avi];
    pub fn ext(self) -> &'static str {
        match self { Self::Mp4 => "mp4", Self::Mkv => "mkv", Self::Webm => "webm", Self::Mov => "mov", Self::Avi => "avi" }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VideoQuality {
    /// Copy streams that fit the new container unchanged; re-encode the rest at high quality.
    Original,
    High,
    Medium,
    Small,
}

/// Maximum height of the result. Videos are never scaled up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VideoResolution { Original, P1080, P720, P480 }
impl VideoResolution {
    pub fn max_height(self) -> Option<u32> {
        match self { Self::Original => None, Self::P1080 => Some(1080), Self::P720 => Some(720), Self::P480 => Some(480) }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConvertVideoOptions {
    pub format: VideoFormat,
    pub quality: VideoQuality,
    pub resolution: VideoResolution,
    pub remove_audio: bool,
}
impl Default for ConvertVideoOptions {
    fn default() -> Self {
        Self { format: VideoFormat::Mp4, quality: VideoQuality::Original, resolution: VideoResolution::Original, remove_audio: false }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AudioFormat { Mp3, M4a, Wav, Flac, Opus }
impl AudioFormat {
    pub const ALL: [AudioFormat; 5] = [Self::Mp3, Self::M4a, Self::Wav, Self::Flac, Self::Opus];
    pub fn ext(self) -> &'static str {
        match self { Self::Mp3 => "mp3", Self::M4a => "m4a", Self::Wav => "wav", Self::Flac => "flac", Self::Opus => "opus" }
    }
}
