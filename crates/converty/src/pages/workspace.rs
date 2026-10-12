//! S02 (empty drop zone) and S03 (file cards or page thumbnails + options sidebar).

use crate::app::{App, FilePickTarget, Message, Thumb};
use crate::engine::{self, ErrKind, UiError};
use crate::pages::options::{OptMsg, SegKey};
use crate::widgets::{self, card_button_class, A11y, category_icon, spacing, tone_icon, tone_text, Tone};
use converty_core::{FileInfo, FileKind, PageOp, Rotation, Tool, ToolOptions};
use cosmic::iced::{Alignment, Length, Radians};
use cosmic::widget::{self, button, segmented_button};
use cosmic::Element;
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

/// Approximate page cell size, used to work out which thumbnails are visible.
pub const CELL_W: f32 = 148.0;
pub const CELL_H: f32 = 214.0;
const THUMB_W: f32 = 118.0;
const THUMB_H: f32 = 156.0;

pub struct FileEntry {
    pub path: PathBuf,
    pub info: Option<FileInfo>,
    /// In memory only.
    pub password: Option<String>,
    pub error: Option<UiError>,
}

impl FileEntry {
    pub fn new(path: PathBuf) -> Self {
        Self { path, info: None, password: None, error: None }
    }

    pub fn name(&self) -> String {
        self.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
    }

    pub fn stem(&self) -> String {
        self.path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "output".into())
    }

    pub fn needs_password(&self) -> bool {
        self.error.is_none()
            && self.password.is_none()
            && self.info.as_ref().is_some_and(|i| i.encrypted && i.pages.is_none())
    }

    pub fn pages(&self) -> Option<u32> {
        self.info.as_ref().and_then(|i| i.pages)
    }
}

/// Result of [`Workspace::accept_files`].
#[derive(Debug, Default)]
pub struct AcceptedFiles {
    /// Newly added paths (need inspecting).
    pub added: Vec<PathBuf>,
    /// Wrong type for this tool.
    pub rejected: usize,
    /// Surplus files for a single-file tool.
    pub ignored: usize,
}

pub struct Workspace {
    pub tool: Tool,
    pub files: Vec<FileEntry>,
    pub options: ToolOptions,
    pub segs: HashMap<SegKey, segmented_button::SingleSelectModel>,
    /// Selected pages (remove / extract / rotate) – mirrors the range text.
    pub selected: BTreeSet<u32>,
    pub org_selected: Option<usize>,
    pub range_error: Option<String>,
    pub split_errors: Vec<Option<String>>,
    pub confirm_password: String,
    pub hide_password: bool,
    pub output_stem: String,
    pub stem_edited: bool,
    /// Why the primary action is disabled, if it is.
    pub blocker: Option<String>,
}

impl Workspace {
    pub fn new(tool: Tool) -> Self {
        let mut ws = Self {
            tool,
            files: Vec::new(),
            options: ToolOptions::default_for(tool),
            segs: HashMap::new(),
            selected: BTreeSet::new(),
            org_selected: None,
            range_error: None,
            split_errors: Vec::new(),
            confirm_password: String::new(),
            hide_password: true,
            output_stem: String::new(),
            stem_edited: false,
            blocker: None,
        };
        ws.rebuild_segs();
        ws
    }

    pub fn default_stem(&self) -> String {
        match self.files.first() {
            Some(f) => format!("{}_{}", f.stem(), crate::pages::tool_suffix(self.tool)),
            None => String::new(),
        }
    }

    /// Single-PDF page tools show a thumbnail grid instead of file cards.
    pub fn page_grid(&self) -> bool {
        matches!(self.tool, Tool::RemovePages | Tool::ExtractPages | Tool::Organize | Tool::Rotate)
            && self.files.len() == 1
            && self.files[0].pages().is_some()
    }

    pub fn page_count(&self) -> Option<u32> {
        self.files.iter().filter_map(FileEntry::pages).min()
    }

    /// Filters `paths` by the tool's accepted kinds and adds the new ones. A single-file
    /// tool keeps only the first new file of the batch (it replaces the current one).
    pub fn accept_files(&mut self, paths: Vec<PathBuf>) -> AcceptedFiles {
        let meta = self.tool.meta();
        let mut out = AcceptedFiles::default();
        for path in paths {
            if !meta.accepts.contains(&FileKind::from_path(&path)) {
                out.rejected += 1;
                continue;
            }
            if self.files.iter().any(|f| f.path == path) {
                continue;
            }
            if !meta.multi {
                if !out.added.is_empty() {
                    out.ignored += 1;
                    continue;
                }
                if !self.files.is_empty() {
                    self.files.clear();
                    self.reset_pages();
                }
            }
            self.files.push(FileEntry::new(path.clone()));
            out.added.push(path);
        }
        out
    }

    pub fn reset_pages(&mut self) {
        self.selected.clear();
        self.org_selected = None;
        if let ToolOptions::Organize { pages } = &mut self.options {
            pages.clear();
        }
    }

    /// Called once a file's page count is known.
    pub fn on_file_ready(&mut self, idx: usize) {
        if idx != 0 {
            return;
        }
        let n = self.files.first().and_then(FileEntry::pages).unwrap_or(0);
        // A page selection typed before the file was known (or kept from the previous file)
        // has to be reflected in the thumbnail grid.
        self.sync_selection_from_text();
        if let ToolOptions::Organize { pages } = &mut self.options {
            if pages.is_empty() {
                *pages = (1..=n).map(|source| PageOp::Page { source, rotate: Rotation::R0 }).collect();
            }
        }
        if let ToolOptions::Split(converty_core::SplitOptions::Ranges { ranges, .. }) = &mut self.options {
            if ranges.len() == 1 && ranges[0] == "1-1" && n > 1 {
                ranges[0] = format!("1-{}", n.div_ceil(2));
                ranges.push(format!("{}-{}", n.div_ceil(2) + 1, n));
            }
        }
    }

    pub fn toggle_page(&mut self, p: u32) {
        if !self.selected.remove(&p) {
            self.selected.insert(p);
        }
        let text = engine::ranges_text(self.selected.iter().copied());
        if let Some(t) = self.range_text_mut() {
            *t = text;
        }
    }

    pub fn select_all(&mut self, all: bool) {
        let n = self.page_count().unwrap_or(0);
        self.selected = if all { (1..=n).collect() } else { BTreeSet::new() };
        let text = engine::ranges_text(self.selected.iter().copied());
        if let Some(t) = self.range_text_mut() {
            *t = text;
        }
    }

    /// The main page-selection field of the current tool, if it has one.
    pub fn range_text_mut(&mut self) -> Option<&mut String> {
        match &mut self.options {
            ToolOptions::RemovePages { pages }
            | ToolOptions::ExtractPages { pages, .. }
            | ToolOptions::Rotate { pages, .. } => Some(pages),
            ToolOptions::PdfToImages(o) => Some(&mut o.pages),
            ToolOptions::PageNumbers(o) => Some(&mut o.pages),
            ToolOptions::Watermark(o) => Some(&mut o.pages),
            ToolOptions::Crop(o) => Some(&mut o.pages),
            _ => None,
        }
    }

    pub fn range_text(&self) -> Option<&str> {
        match &self.options {
            ToolOptions::RemovePages { pages }
            | ToolOptions::ExtractPages { pages, .. }
            | ToolOptions::Rotate { pages, .. } => Some(pages),
            ToolOptions::PdfToImages(o) => Some(&o.pages),
            ToolOptions::PageNumbers(o) => Some(&o.pages),
            ToolOptions::Watermark(o) => Some(&o.pages),
            ToolOptions::Crop(o) => Some(&o.pages),
            _ => None,
        }
    }

    /// Range text typed by hand → page selection (when it parses).
    pub fn sync_selection_from_text(&mut self) {
        let (Some(n), Some(text)) = (self.page_count(), self.range_text().map(str::to_owned)) else { return };
        if text.trim().is_empty() {
            self.selected.clear();
        } else if let Ok(pages) = engine::parse_ranges(&text, n) {
            self.selected = pages.into_iter().collect();
        }
    }

    pub fn set_image(&mut self, target: FilePickTarget, path: PathBuf) {
        match (&mut self.options, target) {
            (ToolOptions::Watermark(o), FilePickTarget::WatermarkImage) => {
                let scale = match &o.content {
                    converty_core::WatermarkContent::Image { scale, .. } => *scale,
                    _ => 0.4,
                };
                o.content = converty_core::WatermarkContent::Image { path, scale };
            }
            (ToolOptions::Sign(o), FilePickTarget::SignatureImage) => {
                o.signature = converty_core::SignatureKind::Image { path };
            }
            _ => {}
        }
    }
}

impl App {
    pub fn view_workspace(&self) -> Element<'_, Message> {
        let Some(ws) = &self.ws else { return widget::text("").into() };
        if ws.files.is_empty() {
            return self.view_drop_zone(ws);
        }
        let s = spacing();
        let left: Element<'_, Message> = if ws.page_grid() { self.view_page_grid(ws) } else { self.view_file_list(ws) };
        widget::row::with_capacity(2)
            .push(widget::container(left).width(Length::Fill).height(Length::Fill))
            .push(self.view_sidebar(ws))
            .spacing(s.space_s)
            .padding([0, s.space_s, s.space_s, s.space_s])
            .into()
    }

    fn view_drop_zone<'a>(&'a self, ws: &'a Workspace) -> Element<'a, Message> {
        let s = spacing();
        let meta = ws.tool.meta();
        let kinds: Vec<&str> = meta.accepts.iter().map(|k| engine::kind_globs(*k).0).collect();
        let what = if meta.multi {
            format!("{} – you can pick several at once", kinds.join(", "))
        } else {
            format!("One file: {}", kinds.join(", ").to_lowercase())
        };
        let missing = self.missing_deps(ws.tool);
        let mut col = widget::column::with_capacity(8)
            .push(category_icon(meta.icon, meta.category, 64))
            .push(widget::text::title3(format!("Add files to {}", meta.name.to_lowercase())))
            .push(widget::text::body(meta.blurb))
            .push(widget::text::caption(what))
            .push(
                button::suggested("Choose files…")
                    .leading_icon(widget::icon::from_name("document-open-symbolic"))
                    .on_press(Message::ChooseFiles(FilePickTarget::Inputs)),
            )
            .push(widget::text::caption("or drop them anywhere in this window  ·  Ctrl+O"))
            .spacing(s.space_s)
            .align_x(Alignment::Center);
        if !missing.is_empty() {
            col = col.push(tone_text(
                format!("This tool needs {} – see Settings for how to install it.", missing.join(", ")),
                Tone::Warning,
            ));
        }
        let zone = widget::container(col)
            .padding(s.space_xl)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .class(widgets::drop_zone_class(self.drag_hover));
        widget::container(zone).padding(s.space_l).width(Length::Fill).height(Length::Fill).into()
    }

    fn thumb_view(&self, path: &Path, page: u32, rotation: Rotation, w: f32, h: f32) -> Element<'_, Message> {
        let key = (path.to_path_buf(), page);
        let inner: Element<'_, Message> = match self.thumbs.get(&key) {
            Some(Thumb::Ready(handle)) => widget::image(handle.clone())
                .width(Length::Fill)
                .height(Length::Fill)
                .content_fit(cosmic::iced::ContentFit::Contain)
                .rotation(cosmic::iced::Rotation::Solid(Radians((rotation.degrees() as f32).to_radians())))
                .into(),
            Some(Thumb::Failed) => widget::icon::from_name("image-missing-symbolic").size(32).into(),
            _ => widget::icon::from_name("content-loading-symbolic").size(24).into(),
        };
        widget::container(inner).width(Length::Fixed(w)).height(Length::Fixed(h)).center_x(Length::Fixed(w)).center_y(Length::Fixed(h)).into()
    }

    fn view_file_list<'a>(&'a self, ws: &'a Workspace) -> Element<'a, Message> {
        let s = spacing();
        let meta = ws.tool.meta();
        let mut toolbar = widget::row::with_capacity(4)
            .push(widget::text::heading(format!(
                "{} file{}",
                ws.files.len(),
                if ws.files.len() == 1 { "" } else { "s" }
            )))
            .push(widget::space::horizontal())
            .spacing(s.space_xs)
            .align_y(Alignment::Center);
        if meta.multi && ws.files.len() > 1 {
            toolbar = toolbar.push(
                button::text("Sort by name")
                    .leading_icon(widget::icon::from_name("view-sort-ascending-symbolic"))
                    .on_press(Message::SortByName),
            );
        }
        toolbar = toolbar.push(
            button::standard(if meta.multi { "Add more…" } else { "Replace…" })
                .leading_icon(widget::icon::from_name("list-add-symbolic"))
                .on_press(Message::ChooseFiles(FilePickTarget::Inputs)),
        );

        let n = ws.files.len();
        let mut list = widget::column::with_capacity(n).spacing(s.space_xs);
        for (i, f) in ws.files.iter().enumerate() {
            let kind = f.info.as_ref().map(|i| i.kind).unwrap_or_else(|| FileKind::from_path(&f.path));
            let thumb: Element<'_, Message> = if matches!(kind, FileKind::Pdf | FileKind::Image) && !f.needs_password() && f.error.is_none() {
                self.thumb_view(&f.path, 1, Rotation::R0, 60.0, 80.0)
            } else {
                widget::container(widget::icon::from_name(match kind {
                    FileKind::Office => "x-office-document-symbolic",
                    FileKind::Html => "text-html-symbolic",
                    _ => "application-pdf-symbolic",
                }).size(32))
                .center_x(Length::Fixed(60.0))
                .center_y(Length::Fixed(80.0))
                .into()
            };
            let detail: Element<'_, Message> = match (&f.info, &f.error) {
                (_, Some(e)) => tone_text(e.message.clone(), Tone::Danger),
                (None, None) => widget::text::caption("Reading…").into(),
                (Some(info), None) => {
                    let mut parts = Vec::new();
                    if let Some(p) = info.pages {
                        parts.push(format!("{p} page{}", if p == 1 { "" } else { "s" }));
                    }
                    parts.push(engine::format_bytes(info.size_bytes));
                    if info.encrypted {
                        parts.push(if f.password.is_some() { "unlocked with password".into() } else { "locked".into() });
                    }
                    widget::text::caption(parts.join(" · ")).into()
                }
            };
            let mut info_col = widget::column::with_capacity(3)
                .push(widget::text::body(f.name()).wrapping(cosmic::iced::widget::text::Wrapping::None))
                .push(detail)
                .spacing(s.space_xxxs)
                .width(Length::Fill);
            if let Some(UiError { kind: ErrKind::Corrupt(_), .. }) = &f.error {
                if ws.tool != Tool::Repair {
                    info_col = info_col.push(button::link("Try to repair it").on_press(Message::RepairFile(f.path.clone())));
                }
            }
            let mut actions = widget::row::with_capacity(4).spacing(s.space_xxs).align_y(Alignment::Center);
            if f.info.as_ref().is_some_and(|i| i.encrypted) && f.password.is_some() {
                actions = actions.push(tone_icon("changes-allow-symbolic", Tone::Success, 16));
            }
            if meta.multi && n > 1 {
                actions = actions
                    .push(widget::tooltip(
                        button::icon(widget::icon::from_name("go-up-symbolic")).on_press_maybe((i > 0).then_some(Message::MoveFile(i, -1))),
                        widget::text::caption("Move up"),
                        widget::tooltip::Position::Top,
                    ))
                    .push(widget::tooltip(
                        button::icon(widget::icon::from_name("go-down-symbolic")).on_press_maybe((i + 1 < n).then_some(Message::MoveFile(i, 1))),
                        widget::text::caption("Move down"),
                        widget::tooltip::Position::Top,
                    ));
            }
            actions = actions.push(widget::tooltip(
                button::icon(widget::icon::from_name("window-close-symbolic")).on_press(Message::RemoveFile(i)),
                widget::text::caption("Remove from list"),
                widget::tooltip::Position::Top,
            ));
            let order: Element<'_, Message> = if meta.multi && n > 1 {
                widget::text::caption_heading(format!("{}", i + 1)).width(Length::Fixed(20.0)).into()
            } else {
                widget::Space::new().width(Length::Fixed(0.0)).into()
            };
            let card = widget::row::with_capacity(4)
                .push(order)
                .push(thumb)
                .push(info_col)
                .push(actions)
                .spacing(s.space_s)
                .align_y(Alignment::Center);
            list = list.push(widget::container(card).padding(s.space_xs).class(cosmic::style::Container::Card));
        }
        let mut col = widget::column::with_capacity(4).push(toolbar).spacing(s.space_s);
        if meta.min_files > n {
            col = col.push(tone_text(format!("Add at least {} files.", meta.min_files), Tone::Warning));
        }
        col.push(widget::scrollable(list).height(Length::Fill)).padding([s.space_xs, 0]).into()
    }

    fn view_page_grid<'a>(&'a self, ws: &'a Workspace) -> Element<'a, Message> {
        let s = spacing();
        let f = &ws.files[0];
        let total = f.pages().unwrap_or(0);
        let path = &f.path;
        let mut toolbar = widget::row::with_capacity(6).spacing(s.space_xs).align_y(Alignment::Center);
        let mut cells: Vec<Element<'_, Message>> = Vec::new();

        if let ToolOptions::Organize { pages } = &ws.options {
            toolbar = toolbar
                .push(widget::text::heading(format!("{} · {} pages", f.name(), pages.len())))
                .push(widget::space::horizontal());
            if let Some(i) = ws.org_selected.filter(|i| *i < pages.len()) {
                let ib = |icon: &'static str, tip: &'static str, msg: Option<Message>| -> Element<'_, Message> {
                    widget::tooltip(
                        button::icon(widget::icon::from_name(icon)).on_press_maybe(msg),
                        widget::text::caption(tip),
                        widget::tooltip::Position::Bottom,
                    )
                    .into()
                };
                let is_page = matches!(pages[i], PageOp::Page { .. });
                toolbar = toolbar
                    .push(ib("go-previous-symbolic", "Move left", (i > 0).then_some(Message::OrgMove(i, -1))))
                    .push(ib("go-next-symbolic", "Move right", (i + 1 < pages.len()).then_some(Message::OrgMove(i, 1))))
                    .push(ib("object-rotate-left-symbolic", "Rotate left", is_page.then_some(Message::OrgRotate(i, false))))
                    .push(ib("object-rotate-right-symbolic", "Rotate right", is_page.then_some(Message::OrgRotate(i, true))))
                    .push(ib("edit-copy-symbolic", "Duplicate", Some(Message::OrgDuplicate(i))))
                    .push(ib("document-new-symbolic", "Insert blank page after", Some(Message::OrgInsertBlank(i))))
                    .push(ib("edit-delete-symbolic", "Delete page", (pages.len() > 1).then_some(Message::OrgDelete(i))));
            } else {
                toolbar = toolbar.push(widget::text::caption("Click a page to move, rotate or delete it"));
            }
            for (i, op) in pages.iter().enumerate() {
                let sel = ws.org_selected == Some(i);
                let (img, label): (Element<'_, Message>, String) = match op {
                    PageOp::Page { source, rotate } => (
                        self.thumb_view(path, *source, *rotate, THUMB_W, THUMB_H),
                        if *rotate == Rotation::R0 { format!("Page {source}") } else { format!("Page {source} · {}°", rotate.degrees()) },
                    ),
                    PageOp::Blank => (
                        widget::container(widget::icon::from_name("document-new-symbolic").size(32))
                            .center_x(Length::Fixed(THUMB_W))
                            .center_y(Length::Fixed(THUMB_H))
                            .into(),
                        "Blank page".into(),
                    ),
                };
                cells.push(page_cell(img, format!("{}. {label}", i + 1), sel, Message::OrgSelect(i)));
            }
        } else {
            let (preview_rot, verb) = match &ws.options {
                ToolOptions::Rotate { degrees, .. } => (Some(*degrees), "rotate"),
                ToolOptions::RemovePages { .. } => (None, "remove"),
                _ => (None, "keep"),
            };
            toolbar = toolbar
                .push(widget::text::heading(format!("{} · {total} pages", f.name())))
                .push(widget::space::horizontal())
                .push(widget::text::caption(if ws.selected.is_empty() {
                    if ws.tool == Tool::Rotate { "No pages picked: all pages turn".to_string() } else { format!("Click pages to {verb}") }
                } else {
                    format!("{} selected", ws.selected.len())
                }))
                .push(button::text("Select all").on_press(Message::SelectAllPages(true)))
                .push(button::text("Clear").on_press_maybe((!ws.selected.is_empty()).then_some(Message::SelectAllPages(false))));
            for p in 1..=total {
                let sel = ws.selected.contains(&p);
                let rot = match preview_rot {
                    Some(r) if sel || ws.selected.is_empty() => r,
                    _ => Rotation::R0,
                };
                let img = self.thumb_view(path, p, rot, THUMB_W, THUMB_H);
                cells.push(page_cell(img, format!("Page {p}"), sel, Message::TogglePage(p)));
            }
        }

        let grid = widget::flex_row(cells).row_spacing(s.space_s).column_spacing(s.space_s);
        widget::column::with_capacity(2)
            .push(toolbar)
            .push(
                widget::scrollable(grid)
                    .on_scroll(Message::ThumbScroll)
                    .width(Length::Fill)
                    .height(Length::Fill),
            )
            .spacing(s.space_s)
            .padding([s.space_xs, 0])
            .into()
    }

    pub fn segmented<'a>(&'a self, ws: &'a Workspace, key: SegKey) -> Element<'a, Message> {
        match ws.segs.get(&key) {
            Some(model) => widget::segmented_control::horizontal(model)
                .on_activate(move |e| Message::Seg(key, e))
                .width(Length::Fill)
                .into(),
            None => widget::Space::new().into(),
        }
    }

    /// Range field with live validation (parse_ranges) and inline error.
    pub fn range_input<'a>(&'a self, ws: &'a Workspace, label: &'a str, empty_means: &'a str) -> Element<'a, Message> {
        let text = ws.range_text().unwrap_or("");
        let mut input = widget::text_input("e.g. 1,3-5,8-", text)
            .label(label)
            .on_input(|t| Message::Opt(OptMsg::RangeText(t)))
            .width(Length::Fill);
        input = match &ws.range_error {
            Some(e) => input.error(e.as_str()),
            None => input.helper_text(if text.trim().is_empty() { empty_means } else { "Pages and ranges, separated by commas" }),
        };
        input.into()
    }
}

fn page_cell<'a>(img: Element<'a, Message>, label: String, selected: bool, msg: Message) -> Element<'a, Message> {
    let s = spacing();
    let content = widget::column::with_capacity(2)
        .push(img)
        .push(widget::text::caption(label))
        .spacing(s.space_xxs)
        .align_x(Alignment::Center);
    let mut b = button::custom(content)
        .padding(s.space_xs)
        .width(Length::Fixed(CELL_W - 8.0))
        .class(card_button_class(selected))
        .selected(selected)
        .on_press(msg);
    if selected {
        b = b.a11y_name("selected page");
    }
    b.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(names: &[&str]) -> Vec<PathBuf> {
        names.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn single_file_tool_keeps_first_new_file_only() {
        let mut ws = Workspace::new(Tool::Split);
        let r = ws.accept_files(paths(&["/x/a.pdf", "/x/b.pdf", "/x/c.png", "/x/D.PDF"]));
        assert_eq!(r.added, paths(&["/x/a.pdf"]));
        assert_eq!((r.rejected, r.ignored), (1, 2));
        assert_eq!(ws.files.len(), 1);
        // a new file replaces the old one and the page selection is reset
        ws.selected.insert(3);
        let r = ws.accept_files(paths(&["/x/e.pdf"]));
        assert_eq!(r.added, paths(&["/x/e.pdf"]));
        assert_eq!(ws.files.len(), 1);
        assert_eq!(ws.files[0].path, PathBuf::from("/x/e.pdf"));
        assert!(ws.selected.is_empty());
        // the same file again changes nothing
        let r = ws.accept_files(paths(&["/x/e.pdf"]));
        assert!(r.added.is_empty());
        assert_eq!(ws.files.len(), 1);
    }

    #[test]
    fn multi_file_tool_keeps_all_in_order() {
        let mut ws = Workspace::new(Tool::Merge);
        let r = ws.accept_files(paths(&["/x/b.pdf", "/x/a.pdf", "/x/a.pdf", "/x/notes.txt"]));
        assert_eq!(r.added.len(), 2);
        assert_eq!(r.rejected, 1);
        assert_eq!(ws.files[0].name(), "b.pdf");
        let mut imgs = Workspace::new(Tool::ImagesToPdf);
        let r = imgs.accept_files(paths(&["/x/a.JPG", "/x/b.webp", "/x/c.pdf"]));
        assert_eq!((r.added.len(), r.rejected), (2, 1));
    }

    #[test]
    fn typed_page_selection_follows_a_replaced_file() {
        let mut ws = Workspace::new(Tool::RemovePages);
        ws.accept_files(paths(&["/x/a.pdf"]));
        ws.files[0].info = Some(FileInfo { kind: FileKind::Pdf, size_bytes: 1, pages: Some(6), encrypted: false, first_page_size: None });
        ws.on_file_ready(0);
        ws.toggle_page(2);
        ws.toggle_page(3);
        assert_eq!(ws.range_text(), Some("2-3"));
        ws.accept_files(paths(&["/x/b.pdf"]));
        assert!(ws.selected.is_empty());
        ws.files[0].info = Some(FileInfo { kind: FileKind::Pdf, size_bytes: 1, pages: Some(8), encrypted: false, first_page_size: None });
        ws.on_file_ready(0);
        assert_eq!(ws.selected.iter().copied().collect::<Vec<_>>(), vec![2, 3]);
    }

    #[test]
    fn locked_file_needs_password_until_unlocked() {
        let mut f = FileEntry::new("/x/l.pdf".into());
        f.info = Some(FileInfo { kind: FileKind::Pdf, size_bytes: 1, pages: None, encrypted: true, first_page_size: None });
        assert!(f.needs_password());
        f.password = Some("pw".into());
        assert!(!f.needs_password());
    }
}
