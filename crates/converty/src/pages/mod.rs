pub mod dialogs;
pub mod home;
pub mod options;
pub mod progress;
pub mod recent;
pub mod result;
pub mod settings;
pub mod workspace;

use converty_core::Tool;

/// Label of the primary action button per tool.
pub fn action_label(tool: Tool) -> &'static str {
    match tool {
        Tool::Merge => "Merge PDFs",
        Tool::Split => "Split PDF",
        Tool::RemovePages => "Remove pages",
        Tool::ExtractPages => "Extract pages",
        Tool::Organize => "Save new order",
        Tool::Compress => "Compress",
        Tool::Repair => "Repair",
        Tool::ImagesToPdf => "Make PDF",
        Tool::OfficeToPdf | Tool::HtmlToPdf => "Convert to PDF",
        Tool::PdfToImages => "Convert to images",
        Tool::PdfToOffice => "Convert",
        Tool::PdfToPdfA => "Convert to PDF/A",
        Tool::Rotate => "Rotate",
        Tool::PageNumbers => "Add numbers",
        Tool::Watermark => "Add watermark",
        Tool::Crop => "Crop",
        Tool::Sign => "Sign",
        Tool::Unlock => "Unlock",
        Tool::Protect => "Protect",
    }
}

/// Suffix used for the default output name: "<first input stem>_<suffix>".
pub fn tool_suffix(tool: Tool) -> &'static str {
    match tool {
        Tool::Merge => "merged",
        Tool::Split => "part",
        Tool::RemovePages => "trimmed",
        Tool::ExtractPages => "extract",
        Tool::Organize => "organized",
        Tool::Compress => "compressed",
        Tool::Repair => "repaired",
        Tool::ImagesToPdf => "images",
        Tool::OfficeToPdf | Tool::HtmlToPdf | Tool::PdfToOffice => "converted",
        Tool::PdfToImages => "page",
        Tool::PdfToPdfA => "pdfa",
        Tool::Rotate => "rotated",
        Tool::PageNumbers => "numbered",
        Tool::Watermark => "watermarked",
        Tool::Crop => "cropped",
        Tool::Sign => "signed",
        Tool::Unlock => "unlocked",
        Tool::Protect => "protected",
    }
}
