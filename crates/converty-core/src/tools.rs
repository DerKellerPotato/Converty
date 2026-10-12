use crate::inspect::FileKind;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Category {
    Organize,
    Optimize,
    ConvertToPdf,
    ConvertFromPdf,
    Edit,
    Security,
}

impl Category {
    pub const ALL: [Category; 6] = [
        Category::Organize,
        Category::Optimize,
        Category::ConvertToPdf,
        Category::ConvertFromPdf,
        Category::Edit,
        Category::Security,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Category::Organize => "Organize",
            Category::Optimize => "Optimize",
            Category::ConvertToPdf => "Convert to PDF",
            Category::ConvertFromPdf => "Convert from PDF",
            Category::Edit => "Edit",
            Category::Security => "Security",
        }
    }
}

/// Every tool Converty offers. Order here is the order on the home grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Tool {
    Merge,
    Split,
    RemovePages,
    ExtractPages,
    Organize,
    Compress,
    Repair,
    ImagesToPdf,
    OfficeToPdf,
    HtmlToPdf,
    PdfToImages,
    PdfToOffice,
    PdfToPdfA,
    Rotate,
    PageNumbers,
    Watermark,
    Crop,
    Sign,
    Unlock,
    Protect,
}

#[derive(Debug, Clone)]
pub struct ToolMeta {
    pub tool: Tool,
    pub category: Category,
    pub name: &'static str,
    /// One short sentence for the home card, written fresh for Converty.
    pub blurb: &'static str,
    /// Freedesktop symbolic icon name (exists in Adwaita, Breeze and Cosmic icon themes).
    pub icon: &'static str,
    pub accepts: &'static [FileKind],
    /// Minimum number of input files to enable the run button.
    pub min_files: usize,
    /// Whether more than one input file is allowed.
    pub multi: bool,
    /// External programs needed ("gs", "soffice", "pdftoppm"). Empty = pure Rust.
    pub needs: &'static [&'static str],
}

pub fn tools() -> &'static [ToolMeta] {
    use FileKind::*;
    const PDF: &[FileKind] = &[Pdf];
    static TOOLS: &[ToolMeta] = &[
        ToolMeta { tool: Tool::Merge, category: Category::Organize, name: "Merge PDF", blurb: "Join several PDFs into one, in the order you choose.", icon: "list-add-symbolic", accepts: PDF, min_files: 2, multi: true, needs: &[] },
        ToolMeta { tool: Tool::Split, category: Category::Organize, name: "Split PDF", blurb: "Cut a PDF into ranges or fixed-size parts.", icon: "edit-cut-symbolic", accepts: PDF, min_files: 1, multi: false, needs: &[] },
        ToolMeta { tool: Tool::RemovePages, category: Category::Organize, name: "Remove pages", blurb: "Drop the pages you do not need.", icon: "edit-delete-symbolic", accepts: PDF, min_files: 1, multi: false, needs: &[] },
        ToolMeta { tool: Tool::ExtractPages, category: Category::Organize, name: "Extract pages", blurb: "Pull selected pages out into a new file.", icon: "edit-copy-symbolic", accepts: PDF, min_files: 1, multi: false, needs: &[] },
        ToolMeta { tool: Tool::Organize, category: Category::Organize, name: "Organize PDF", blurb: "Reorder, rotate, delete or add blank pages.", icon: "view-grid-symbolic", accepts: PDF, min_files: 1, multi: false, needs: &[] },
        ToolMeta { tool: Tool::Compress, category: Category::Optimize, name: "Compress PDF", blurb: "Make a PDF smaller while keeping it readable.", icon: "package-x-generic-symbolic", accepts: PDF, min_files: 1, multi: true, needs: &["gs"] },
        ToolMeta { tool: Tool::Repair, category: Category::Optimize, name: "Repair PDF", blurb: "Rebuild a damaged PDF and save what can be saved.", icon: "applications-engineering-symbolic", accepts: PDF, min_files: 1, multi: true, needs: &[] },
        ToolMeta { tool: Tool::ImagesToPdf, category: Category::ConvertToPdf, name: "Images to PDF", blurb: "Turn JPG, PNG and other images into a PDF.", icon: "image-x-generic-symbolic", accepts: &[Image], min_files: 1, multi: true, needs: &[] },
        ToolMeta { tool: Tool::OfficeToPdf, category: Category::ConvertToPdf, name: "Office to PDF", blurb: "Convert Word, Excel, PowerPoint and ODF files.", icon: "x-office-document-symbolic", accepts: &[Office], min_files: 1, multi: true, needs: &["soffice"] },
        ToolMeta { tool: Tool::HtmlToPdf, category: Category::ConvertToPdf, name: "HTML to PDF", blurb: "Print a local web page to PDF.", icon: "text-html-symbolic", accepts: &[Html], min_files: 1, multi: true, needs: &["soffice"] },
        ToolMeta { tool: Tool::PdfToImages, category: Category::ConvertFromPdf, name: "PDF to images", blurb: "Save every page as JPG or PNG, or pull out embedded images.", icon: "camera-photo-symbolic", accepts: PDF, min_files: 1, multi: true, needs: &["pdftoppm"] },
        ToolMeta { tool: Tool::PdfToOffice, category: Category::ConvertFromPdf, name: "PDF to Office", blurb: "Get an editable document back (layout may shift).", icon: "x-office-spreadsheet-symbolic", accepts: PDF, min_files: 1, multi: true, needs: &["soffice"] },
        ToolMeta { tool: Tool::PdfToPdfA, category: Category::ConvertFromPdf, name: "PDF to PDF/A", blurb: "Archive-ready PDF for long-term storage.", icon: "document-save-symbolic", accepts: PDF, min_files: 1, multi: true, needs: &["gs"] },
        ToolMeta { tool: Tool::Rotate, category: Category::Edit, name: "Rotate PDF", blurb: "Turn all pages or just some of them.", icon: "object-rotate-right-symbolic", accepts: PDF, min_files: 1, multi: true, needs: &[] },
        ToolMeta { tool: Tool::PageNumbers, category: Category::Edit, name: "Page numbers", blurb: "Stamp numbers where you want them.", icon: "format-ordered-list-symbolic", accepts: PDF, min_files: 1, multi: false, needs: &[] },
        ToolMeta { tool: Tool::Watermark, category: Category::Edit, name: "Watermark", blurb: "Put text or an image over or under your pages.", icon: "insert-text-symbolic", accepts: PDF, min_files: 1, multi: false, needs: &[] },
        ToolMeta { tool: Tool::Crop, category: Category::Edit, name: "Crop PDF", blurb: "Trim page margins.", icon: "zoom-fit-best-symbolic", accepts: PDF, min_files: 1, multi: false, needs: &[] },
        ToolMeta { tool: Tool::Sign, category: Category::Edit, name: "Sign PDF", blurb: "Place a typed or image signature on a page.", icon: "document-edit-symbolic", accepts: PDF, min_files: 1, multi: false, needs: &[] },
        ToolMeta { tool: Tool::Unlock, category: Category::Security, name: "Unlock PDF", blurb: "Remove the password from a PDF you can open.", icon: "changes-allow-symbolic", accepts: PDF, min_files: 1, multi: false, needs: &["gs"] },
        ToolMeta { tool: Tool::Protect, category: Category::Security, name: "Protect PDF", blurb: "Lock a PDF with a password and permissions.", icon: "changes-prevent-symbolic", accepts: PDF, min_files: 1, multi: false, needs: &["gs"] },
    ];
    TOOLS
}

impl Tool {
    pub fn meta(self) -> &'static ToolMeta {
        tools().iter().find(|m| m.tool == self).expect("every tool has meta")
    }
}
