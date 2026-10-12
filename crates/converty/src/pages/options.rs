//! The options sidebar (S03, right column): one panel per tool, validation and
//! the pinned primary action.

use crate::app::{App, FilePickTarget, Message};
use crate::config::OutputMode;
use crate::engine;
use crate::pages::workspace::Workspace;
use crate::widgets::{category_icon, color_swatches, labelled, position_grid, seg_model, spacing, tone_text, Tone};
use converty_core::*;
use cosmic::iced::{Alignment, Length};
use cosmic::widget::{self, button, segmented_button};
use cosmic::Element;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SegKey {
    SplitMode,
    Compress,
    PageSize,
    Orientation,
    ImgMargin,
    PdfImgMode,
    ImgFormat,
    OfficeTarget,
    PdfA,
    RotateBy,
    PnTemplate,
    PnMargin,
    WmKind,
    WmAngle,
    SignKind,
    SignPage,
}

#[derive(Debug, Clone)]
pub enum OptMsg {
    RangeText(String),
    SplitRange(usize, String),
    SplitAdd,
    SplitRemove(usize),
    SplitMergeAll(bool),
    SplitEvery(u32),
    ExtractSeparate(bool),
    ImagesMerge(bool),
    Dpi(u32),
    PnPosition(Position),
    PnFirst(u32),
    PnTemplate(String),
    PnSize(f32),
    PnColor(Rgb),
    PnFacing(bool),
    WmText(String),
    WmSize(f32),
    WmColor(Rgb),
    WmBold(bool),
    WmScale(f32),
    WmPosition(Position),
    WmOpacity(f32),
    WmRotation(f32),
    WmMosaic(bool),
    WmBelow(bool),
    Crop(u8, f32),
    SignName(String),
    SignPage(u32),
    SignPosition(Position),
    SignWidth(f32),
    SignDate(bool),
    UserPw(String),
    ConfirmPw(String),
    OwnerPw(String),
    ToggleHidePw,
    Perm(u8, bool),
    OutputStem(String),
    ChooseImage(FilePickTarget),
}

const TEMPLATES: [&str; 4] = ["{n}", "Page {n}", "Page {n} of {p}", "{n} / {p}"];
const PN_MARGINS: [f32; 3] = [16.0, 28.0, 44.0];
const WM_ANGLES: [f32; 3] = [0.0, 45.0, 90.0];

impl Workspace {
    /// (Re)creates the segmented-control models from the current options.
    pub fn rebuild_segs(&mut self) {
        use SegKey as K;
        let segs = &mut self.segs;
        segs.clear();
        let mut put = |k: K, labels: &[&'static str], active: usize| {
            segs.insert(k, seg_model(labels, active));
        };
        match &self.options {
            ToolOptions::Split(o) => put(
                K::SplitMode,
                &["Ranges", "Every N pages", "Every page"],
                match o {
                    SplitOptions::Ranges { .. } => 0,
                    SplitOptions::Fixed { .. } => 1,
                    SplitOptions::EveryPage => 2,
                },
            ),
            ToolOptions::Compress { level } => put(
                K::Compress,
                &["Strong", "Balanced", "Light"],
                match level {
                    CompressLevel::Extreme => 0,
                    CompressLevel::Recommended => 1,
                    CompressLevel::Low => 2,
                },
            ),
            ToolOptions::ImagesToPdf(o) => {
                put(K::PageSize, &["Fit image", "A4", "Letter"], o.page_size as usize);
                put(K::Orientation, &["Auto", "Portrait", "Landscape"], o.orientation as usize);
                put(K::ImgMargin, &["None", "Small", "Big"], o.margin as usize);
            }
            ToolOptions::PdfToImages(o) => {
                put(K::PdfImgMode, &["Whole pages", "Embedded images"], usize::from(!o.render_pages));
                put(K::ImgFormat, &["JPG", "PNG"], o.format as usize);
            }
            ToolOptions::PdfToOffice { target } => {
                put(K::OfficeTarget, &["Word", "ODT", "Excel", "PowerPoint"], *target as usize)
            }
            ToolOptions::PdfToPdfA { level } => put(K::PdfA, &["PDF/A-1b", "PDF/A-2b", "PDF/A-3b"], *level as usize),
            ToolOptions::Rotate { degrees, .. } => put(
                K::RotateBy,
                &["90° right", "180°", "90° left"],
                match degrees {
                    Rotation::R180 => 1,
                    Rotation::R270 => 2,
                    _ => 0,
                },
            ),
            ToolOptions::PageNumbers(o) => {
                put(
                    K::PnTemplate,
                    &["1", "Page 1", "Page 1 of 9", "1 / 9"],
                    TEMPLATES.iter().position(|t| *t == o.template).unwrap_or(usize::MAX),
                );
                let m = PN_MARGINS.iter().position(|m| (*m - o.margin).abs() < 0.5).unwrap_or(1);
                put(K::PnMargin, &["Small", "Medium", "Large"], m);
            }
            ToolOptions::Watermark(o) => {
                put(K::WmKind, &["Text", "Image"], usize::from(matches!(o.content, WatermarkContent::Image { .. })));
                put(
                    K::WmAngle,
                    &["Flat", "45°", "Upright"],
                    WM_ANGLES.iter().position(|a| (*a - o.rotation_deg).abs() < 0.5).unwrap_or(usize::MAX),
                );
            }
            ToolOptions::Sign(o) => {
                put(K::SignKind, &["Type it", "Use an image"], usize::from(matches!(o.signature, SignatureKind::Image { .. })));
                put(
                    K::SignPage,
                    &["Last page", "First page", "Other"],
                    match o.page {
                        0 => 0,
                        1 => 1,
                        _ => 2,
                    },
                );
            }
            _ => {}
        }
    }

    pub fn on_seg(&mut self, key: SegKey, entity: segmented_button::Entity) {
        let Some(model) = self.segs.get_mut(&key) else { return };
        model.activate(entity);
        let Some(&i) = model.data::<usize>(entity) else { return };
        let n = self.page_count().unwrap_or(1);
        match (&mut self.options, key) {
            (ToolOptions::Split(o), SegKey::SplitMode) => {
                *o = match i {
                    0 => SplitOptions::default(),
                    1 => SplitOptions::Fixed { every: 1 },
                    _ => SplitOptions::EveryPage,
                };
                if i == 0 {
                    self.on_file_ready(0);
                }
            }
            (ToolOptions::Compress { level }, _) => {
                *level = [CompressLevel::Extreme, CompressLevel::Recommended, CompressLevel::Low][i.min(2)]
            }
            (ToolOptions::ImagesToPdf(o), SegKey::PageSize) => o.page_size = [PageSize::Fit, PageSize::A4, PageSize::Letter][i.min(2)],
            (ToolOptions::ImagesToPdf(o), SegKey::Orientation) => {
                o.orientation = [Orientation::Auto, Orientation::Portrait, Orientation::Landscape][i.min(2)]
            }
            (ToolOptions::ImagesToPdf(o), SegKey::ImgMargin) => o.margin = [Margin::None, Margin::Small, Margin::Big][i.min(2)],
            (ToolOptions::PdfToImages(o), SegKey::PdfImgMode) => o.render_pages = i == 0,
            (ToolOptions::PdfToImages(o), SegKey::ImgFormat) => o.format = [ImageFormat::Jpg, ImageFormat::Png][i.min(1)],
            (ToolOptions::PdfToOffice { target }, _) => {
                *target = [OfficeTarget::Docx, OfficeTarget::Odt, OfficeTarget::Xlsx, OfficeTarget::Pptx][i.min(3)]
            }
            (ToolOptions::PdfToPdfA { level }, _) => *level = [PdfALevel::B1, PdfALevel::B2, PdfALevel::B3][i.min(2)],
            (ToolOptions::Rotate { degrees, .. }, _) => *degrees = [Rotation::R90, Rotation::R180, Rotation::R270][i.min(2)],
            (ToolOptions::PageNumbers(o), SegKey::PnTemplate) => o.template = TEMPLATES[i.min(3)].to_string(),
            (ToolOptions::PageNumbers(o), SegKey::PnMargin) => o.margin = PN_MARGINS[i.min(2)],
            (ToolOptions::Watermark(o), SegKey::WmKind) => {
                o.content = if i == 0 {
                    WatermarkContent::Text { text: "CONFIDENTIAL".into(), font_size: 48.0, color: Rgb(200, 30, 30), bold: true }
                } else {
                    WatermarkContent::Image { path: Default::default(), scale: 0.4 }
                }
            }
            (ToolOptions::Watermark(o), SegKey::WmAngle) => o.rotation_deg = WM_ANGLES[i.min(2)],
            (ToolOptions::Sign(o), SegKey::SignKind) => {
                o.signature = if i == 0 {
                    SignatureKind::Typed { name: String::new() }
                } else {
                    SignatureKind::Image { path: Default::default() }
                }
            }
            (ToolOptions::Sign(o), SegKey::SignPage) => {
                o.page = match i {
                    0 => 0,
                    1 => 1,
                    _ => o.page.max(2).min(n.max(2)),
                }
            }
            _ => {}
        }
    }

    pub fn on_opt(&mut self, m: OptMsg) {
        use OptMsg as O;
        match (m, &mut self.options) {
            (O::RangeText(t), _) => {
                if let Some(r) = self.range_text_mut() {
                    *r = t;
                }
                self.sync_selection_from_text();
            }
            (O::SplitRange(i, t), ToolOptions::Split(SplitOptions::Ranges { ranges, .. })) => {
                if let Some(r) = ranges.get_mut(i) {
                    *r = t;
                }
            }
            (O::SplitAdd, ToolOptions::Split(SplitOptions::Ranges { ranges, .. })) => ranges.push(String::new()),
            (O::SplitRemove(i), ToolOptions::Split(SplitOptions::Ranges { ranges, .. })) => {
                if ranges.len() > 1 && i < ranges.len() {
                    ranges.remove(i);
                }
            }
            (O::SplitMergeAll(b), ToolOptions::Split(SplitOptions::Ranges { merge_all, .. })) => *merge_all = b,
            (O::SplitEvery(n), ToolOptions::Split(SplitOptions::Fixed { every })) => *every = n.max(1),
            (O::ExtractSeparate(b), ToolOptions::ExtractPages { separate_files, .. }) => *separate_files = b,
            (O::ImagesMerge(b), ToolOptions::ImagesToPdf(o)) => o.merge = b,
            (O::Dpi(d), ToolOptions::PdfToImages(o)) => o.dpi = d.clamp(72, 600),
            (O::PnPosition(p), ToolOptions::PageNumbers(o)) => o.position = p,
            (O::PnFirst(n), ToolOptions::PageNumbers(o)) => o.first_number = n,
            (O::PnTemplate(t), ToolOptions::PageNumbers(o)) => {
                o.template = t;
                let idx = TEMPLATES.iter().position(|x| *x == o.template);
                if let Some(model) = self.segs.get_mut(&SegKey::PnTemplate) {
                    match idx.and_then(|i| model.iter().nth(i)) {
                        Some(e) => model.activate(e),
                        None => model.deactivate(),
                    }
                }
            }
            (O::PnSize(s), ToolOptions::PageNumbers(o)) => o.font_size = s,
            (O::PnColor(c), ToolOptions::PageNumbers(o)) => o.color = c,
            (O::PnFacing(b), ToolOptions::PageNumbers(o)) => o.facing_pages = b,
            (O::WmText(t), ToolOptions::Watermark(o)) => {
                if let WatermarkContent::Text { text, .. } = &mut o.content {
                    *text = t;
                }
            }
            (O::WmSize(s), ToolOptions::Watermark(o)) => {
                if let WatermarkContent::Text { font_size, .. } = &mut o.content {
                    *font_size = s;
                }
            }
            (O::WmColor(c), ToolOptions::Watermark(o)) => {
                if let WatermarkContent::Text { color, .. } = &mut o.content {
                    *color = c;
                }
            }
            (O::WmBold(b), ToolOptions::Watermark(o)) => {
                if let WatermarkContent::Text { bold, .. } = &mut o.content {
                    *bold = b;
                }
            }
            (O::WmScale(s), ToolOptions::Watermark(o)) => {
                if let WatermarkContent::Image { scale, .. } = &mut o.content {
                    *scale = s.clamp(0.05, 1.0);
                }
            }
            (O::WmPosition(p), ToolOptions::Watermark(o)) => o.position = p,
            (O::WmOpacity(v), ToolOptions::Watermark(o)) => o.opacity = v.clamp(0.0, 1.0),
            (O::WmRotation(v), ToolOptions::Watermark(o)) => {
                o.rotation_deg = v;
                if let Some(model) = self.segs.get_mut(&SegKey::WmAngle) {
                    match WM_ANGLES.iter().position(|a| (*a - v).abs() < 0.5).and_then(|i| model.iter().nth(i)) {
                        Some(e) => model.activate(e),
                        None => model.deactivate(),
                    }
                }
            }
            (O::WmMosaic(b), ToolOptions::Watermark(o)) => o.mosaic = b,
            (O::WmBelow(b), ToolOptions::Watermark(o)) => o.below_content = b,
            (O::Crop(side, v), ToolOptions::Crop(o)) => match side {
                0 => o.top = v,
                1 => o.right = v,
                2 => o.bottom = v,
                _ => o.left = v,
            },
            (O::SignName(t), ToolOptions::Sign(o)) => o.signature = SignatureKind::Typed { name: t },
            (O::SignPage(p), ToolOptions::Sign(o)) => o.page = p.max(1),
            (O::SignPosition(p), ToolOptions::Sign(o)) => o.position = p,
            (O::SignWidth(w), ToolOptions::Sign(o)) => o.width = w,
            (O::SignDate(b), ToolOptions::Sign(o)) => o.add_date = b,
            (O::UserPw(t), ToolOptions::Protect(o)) => o.user_password = t,
            (O::ConfirmPw(t), ToolOptions::Protect(_)) => self.confirm_password = t,
            (O::OwnerPw(t), ToolOptions::Protect(o)) => o.owner_password = t,
            (O::ToggleHidePw, _) => self.hide_password = !self.hide_password,
            (O::Perm(k, b), ToolOptions::Protect(o)) => match k {
                0 => o.permissions.allow_print = b,
                1 => o.permissions.allow_copy = b,
                2 => o.permissions.allow_modify = b,
                _ => o.permissions.allow_annotate = b,
            },
            (O::OutputStem(t), _) => {
                self.output_stem = t;
                self.stem_edited = true;
            }
            _ => {}
        }
    }

    /// Recomputes inline errors and the reason the primary action is disabled.
    pub fn validate(&mut self) {
        let meta = self.tool.meta();
        let pages = self.page_count();
        self.range_error = None;
        self.split_errors.clear();

        // Range fields (live parse_ranges).
        if let (Some(n), Some(text)) = (pages, self.range_text()) {
            if !text.trim().is_empty() {
                if let Err(e) = engine::parse_ranges(text, n) {
                    self.range_error = Some(e);
                }
            }
        }
        if let (Some(n), ToolOptions::Split(SplitOptions::Ranges { ranges, .. })) = (pages, &self.options) {
            self.split_errors = ranges
                .iter()
                .map(|r| {
                    if r.trim().is_empty() {
                        Some("Enter a range like 1-3".to_string())
                    } else {
                        engine::parse_ranges(r, n).err()
                    }
                })
                .collect();
        }

        let mut blocker: Option<String> = None;
        let mut block = |s: &str| {
            if blocker.is_none() {
                blocker = Some(s.to_string());
            }
        };
        if self.files.is_empty() {
            block("Add a file to start");
        } else if self.files.len() < meta.min_files {
            block(&format!("Add at least {} files", meta.min_files));
        }
        if self.files.iter().any(|f| f.info.is_none() && f.error.is_none()) {
            block("Reading files…");
        }
        if self.files.iter().any(|f| f.error.is_some()) {
            block("Remove the file marked in red first");
        }
        if self.files.iter().any(|f| f.needs_password()) {
            block("A locked file still needs its password");
        }
        if self.range_error.is_some() {
            block("Fix the page selection");
        }
        match &self.options {
            ToolOptions::RemovePages { pages: text } => {
                if text.trim().is_empty() {
                    block("Pick the pages to remove");
                } else if let Some(n) = pages {
                    if engine::parse_ranges(text, n).is_ok_and(|v| v.len() as u32 >= n) {
                        block("At least one page has to stay");
                    }
                }
            }
            ToolOptions::Split(SplitOptions::Ranges { ranges, .. }) => {
                if ranges.is_empty() || self.split_errors.iter().any(Option::is_some) {
                    block("Fix the ranges to split");
                }
            }
            ToolOptions::Organize { pages: ops } => {
                if ops.is_empty() && !self.files.is_empty() {
                    block("Keep at least one page");
                }
            }
            ToolOptions::Watermark(o) => match &o.content {
                WatermarkContent::Text { text, .. } if text.trim().is_empty() => block("Type the watermark text"),
                WatermarkContent::Image { path, .. } if path.as_os_str().is_empty() => block("Choose a watermark image"),
                _ => {}
            },
            ToolOptions::Sign(o) => {
                match &o.signature {
                    SignatureKind::Typed { name } if name.trim().is_empty() => block("Type your name to sign"),
                    SignatureKind::Image { path } if path.as_os_str().is_empty() => block("Choose a signature image"),
                    _ => {}
                }
                if let Some(n) = pages {
                    if o.page > n {
                        block("That page does not exist");
                    }
                }
            }
            ToolOptions::Protect(o) => {
                if o.user_password.is_empty() {
                    block("Choose a password");
                } else if o.user_password != self.confirm_password {
                    block("The passwords do not match");
                }
            }
            _ => {}
        }
        if self.output_stem.trim().is_empty() && !self.files.is_empty() {
            block("Give the result a name");
        }
        self.blocker = blocker;
    }
}

impl App {
    pub fn view_sidebar<'a>(&'a self, ws: &'a Workspace) -> Element<'a, Message> {
        let s = spacing();
        let meta = ws.tool.meta();
        let header = widget::row::with_capacity(2)
            .push(category_icon(meta.icon, meta.category, 32))
            .push(
                widget::column::with_capacity(2)
                    .push(widget::text::title4(meta.name))
                    .push(widget::text::caption(meta.blurb)),
            )
            .spacing(s.space_s)
            .align_y(Alignment::Center);

        let output_note = match &self.settings.output_mode {
            OutputMode::SameAsInput => "Saved next to the first file".to_string(),
            OutputMode::Folder(p) => format!("Saved in {}", p.display()),
            OutputMode::Ask => "You pick the folder when it runs".to_string(),
        };
        let multi_out = matches!(
            (&ws.tool, &ws.options),
            (Tool::Split, _)
                | (Tool::PdfToImages, _)
                | (Tool::ExtractPages, ToolOptions::ExtractPages { separate_files: true, .. })
        ) || (ws.files.len() > 1 && !matches!(ws.tool, Tool::Merge | Tool::ImagesToPdf));
        let stem = widget::text_input("File name", &ws.output_stem)
            .label("Result name")
            .helper_text(if multi_out { "Numbers are added for each file" } else { "Existing files are never overwritten" })
            .on_input(|t| Message::Opt(OptMsg::OutputStem(t)))
            .width(Length::Fill);

        let body = widget::column::with_capacity(8)
            .push(header)
            .push(widget::divider::horizontal::default())
            .push(self.options_panel(ws))
            .push(widget::divider::horizontal::default())
            .push(stem)
            .push(widget::text::caption(output_note))
            .spacing(s.space_m)
            .padding([s.space_s, s.space_m]);

        let missing = self.missing_deps(ws.tool);
        let reason: Option<String> = if !missing.is_empty() {
            Some(format!("Needs {} – install it first", missing.join(", ")))
        } else {
            ws.blocker.clone()
        };
        let mut footer = widget::column::with_capacity(2).spacing(s.space_xs);
        if let Some(r) = &reason {
            footer = footer.push(tone_text(r.clone(), Tone::Warning));
        }
        footer = footer.push(
            button::suggested(crate::pages::action_label(ws.tool))
                .width(Length::Fill)
                .on_press_maybe(reason.is_none().then_some(Message::Run)),
        );
        footer = footer.push(widget::text::caption("Ctrl+Enter").width(Length::Fill).align_x(Alignment::Center));

        widget::layer_container(
            widget::column::with_capacity(3)
                .push(widget::scrollable(body).height(Length::Fill))
                .push(widget::divider::horizontal::default())
                .push(widget::container(footer).padding(s.space_m)),
        )
        .layer(cosmic::cosmic_theme::Layer::Secondary)
        .width(Length::Fixed(320.0))
        .height(Length::Fill)
        .into()
    }

    fn options_panel<'a>(&'a self, ws: &'a Workspace) -> Element<'a, Message> {
        let s = spacing();
        let mut col = widget::column::with_capacity(12).spacing(s.space_m).width(Length::Fill);
        let pages = ws.page_count();
        let toggle = |label: &'static str, v: bool, f: fn(bool) -> OptMsg| -> Element<'a, Message> {
            widget::toggler(v)
                .label(label.to_string())
                .on_toggle(move |b| Message::Opt(f(b)))
                .width(Length::Fill)
                .into()
        };
        let slider = |label: String, range: std::ops::RangeInclusive<f32>, v: f32, step: f32, f: fn(f32) -> OptMsg| -> Element<'a, Message> {
            widget::column::with_capacity(2)
                .push(
                    widget::row::with_capacity(2)
                        .push(widget::text::caption_heading(label))
                        .push(widget::space::horizontal()),
                )
                .push(widget::slider(range, v, move |x| Message::Opt(f(x))).step(step))
                .spacing(s.space_xxs)
                .into()
        };

        match &ws.options {
            ToolOptions::Merge => {
                col = col.push(widget::text::body("Files are joined from top to bottom. Use the arrows or “Sort by name” to change the order."));
            }
            ToolOptions::Split(o) => {
                col = col.push(labelled("Split by", self.segmented(ws, SegKey::SplitMode)));
                match o {
                    SplitOptions::Ranges { ranges, merge_all } => {
                        let mut list = widget::column::with_capacity(ranges.len() + 1).spacing(s.space_xs);
                        for (i, r) in ranges.iter().enumerate() {
                            let mut input = widget::text_input("e.g. 1-3", r)
                                .label(format!("Part {}", i + 1))
                                .on_input(move |t| Message::Opt(OptMsg::SplitRange(i, t)))
                                .width(Length::Fill);
                            if let Some(Some(e)) = ws.split_errors.get(i) {
                                input = input.error(e.clone());
                            }
                            list = list.push(
                                widget::row::with_capacity(2)
                                    .push(input)
                                    .push(
                                        button::icon(widget::icon::from_name("list-remove-symbolic"))
                                            .on_press_maybe((ranges.len() > 1).then_some(Message::Opt(OptMsg::SplitRemove(i)))),
                                    )
                                    .spacing(s.space_xxs)
                                    .align_y(Alignment::End),
                            );
                        }
                        list = list.push(
                            button::text("Add range")
                                .leading_icon(widget::icon::from_name("list-add-symbolic"))
                                .on_press(Message::Opt(OptMsg::SplitAdd)),
                        );
                        col = col.push(list).push(toggle("Put all ranges in one PDF", *merge_all, OptMsg::SplitMergeAll));
                    }
                    SplitOptions::Fixed { every } => {
                        let parts = pages.map(|n| n.div_ceil(*every)).unwrap_or(0);
                        col = col
                            .push(labelled(
                                "Pages per file",
                                crate::widgets::spin_button(every.to_string(), "Pages per file", *every, 1, 1, pages.unwrap_or(9999).max(1), |v| {
                                    Message::Opt(OptMsg::SplitEvery(v))
                                }),
                            ))
                            .push(widget::text::caption(format!("Makes {parts} file{}", if parts == 1 { "" } else { "s" })));
                    }
                    SplitOptions::EveryPage => {
                        col = col.push(widget::text::caption(format!("Makes {} files, one per page", pages.unwrap_or(0))));
                    }
                }
            }
            ToolOptions::RemovePages { .. } => {
                col = col.push(self.range_input(ws, "Pages to remove", "Click pages on the left or type them here"));
            }
            ToolOptions::ExtractPages { separate_files, .. } => {
                col = col
                    .push(self.range_input(ws, "Pages to extract", "Empty means every page"))
                    .push(toggle("One file per page", *separate_files, OptMsg::ExtractSeparate));
            }
            ToolOptions::Organize { .. } => {
                col = col
                    .push(widget::text::body("Pick a page on the left, then move, turn, copy or delete it. You can also add blank pages."))
                    .push(
                        button::standard("Undo all changes")
                            .leading_icon(widget::icon::from_name("edit-undo-symbolic"))
                            .on_press(Message::OrgReset),
                    );
            }
            ToolOptions::Compress { level } => {
                col = col.push(labelled("Compression", self.segmented(ws, SegKey::Compress))).push(widget::text::caption(match level {
                    CompressLevel::Extreme => "Smallest file. Images get noticeably softer.",
                    CompressLevel::Recommended => "Good size and still sharp on screen.",
                    CompressLevel::Low => "Keeps print quality, saves less space.",
                }));
                col = col.push(widget::text::caption("If the result would be bigger, the original is kept."));
            }
            ToolOptions::Repair => {
                col = col.push(widget::text::body("Converty rebuilds the file structure and keeps every page it can read."));
            }
            ToolOptions::ImagesToPdf(o) => {
                col = col
                    .push(labelled("Page size", self.segmented(ws, SegKey::PageSize)))
                    .push(labelled("Orientation", self.segmented(ws, SegKey::Orientation)))
                    .push(labelled("Margin", self.segmented(ws, SegKey::ImgMargin)))
                    .push(toggle("All images in one PDF", o.merge, OptMsg::ImagesMerge));
            }
            ToolOptions::OfficeToPdf | ToolOptions::HtmlToPdf => {
                col = col.push(widget::text::body("Converted with LibreOffice in the background. The first run can take a few seconds."));
            }
            ToolOptions::PdfToImages(o) => {
                col = col.push(labelled("Save", self.segmented(ws, SegKey::PdfImgMode)));
                if o.render_pages {
                    col = col
                        .push(labelled("Format", self.segmented(ws, SegKey::ImgFormat)))
                        .push(slider(format!("Resolution: {} dpi", o.dpi), 72.0..=600.0, o.dpi as f32, 1.0, |v| OptMsg::Dpi(v.round() as u32)))
                        .push(widget::text::caption("150 dpi is fine for screens, 300 dpi for print."));
                } else {
                    col = col.push(widget::text::caption("Pictures inside the PDF are saved as they are."));
                }
                col = col.push(self.range_input(ws, "Pages", "Empty means every page"));
            }
            ToolOptions::PdfToOffice { .. } => {
                col = col
                    .push(labelled("Convert to", self.segmented(ws, SegKey::OfficeTarget)))
                    .push(widget::text::caption("The layout may shift a little. Scanned pages stay images."));
            }
            ToolOptions::PdfToPdfA { .. } => {
                col = col
                    .push(labelled("Conformance", self.segmented(ws, SegKey::PdfA)))
                    .push(widget::text::caption("PDF/A-2b suits most archives."));
            }
            ToolOptions::Rotate { .. } => {
                col = col.push(labelled("Turn by", self.segmented(ws, SegKey::RotateBy))).push(self.range_input(
                    ws,
                    "Pages to turn",
                    "Empty means every page",
                ));
            }
            ToolOptions::PageNumbers(o) => {
                col = col
                    .push(labelled("Position", position_grid(o.position, |p| Message::Opt(OptMsg::PnPosition(p)))))
                    .push(labelled("Distance from edge", self.segmented(ws, SegKey::PnMargin)))
                    .push(labelled("Style", self.segmented(ws, SegKey::PnTemplate)))
                    .push(
                        widget::text_input("{n}", &o.template)
                            .label("Text")
                            .helper_text("{n} is the number, {p} the page total")
                            .on_input(|t| Message::Opt(OptMsg::PnTemplate(t))),
                    )
                    .push(labelled(
                        "First number",
                        crate::widgets::spin_button(o.first_number.to_string(), "First number", o.first_number, 1, 0, 99_999, |v| {
                            Message::Opt(OptMsg::PnFirst(v))
                        }),
                    ))
                    .push(self.range_input(ws, "Pages with a number", "Empty means every page"))
                    .push(slider(format!("Text size: {:.0} pt", o.font_size), 6.0..=36.0, o.font_size, 1.0, OptMsg::PnSize))
                    .push(labelled("Colour", color_swatches(o.color, |c| Message::Opt(OptMsg::PnColor(c)))))
                    .push(toggle("Mirror on facing pages", o.facing_pages, OptMsg::PnFacing));
            }
            ToolOptions::Watermark(o) => {
                col = col.push(labelled("Watermark", self.segmented(ws, SegKey::WmKind)));
                match &o.content {
                    WatermarkContent::Text { text, font_size, color, bold } => {
                        col = col
                            .push(widget::text_input("Text", text).label("Text").on_input(|t| Message::Opt(OptMsg::WmText(t))))
                            .push(slider(format!("Text size: {font_size:.0} pt"), 8.0..=144.0, *font_size, 1.0, OptMsg::WmSize))
                            .push(labelled("Colour", color_swatches(*color, |c| Message::Opt(OptMsg::WmColor(c)))))
                            .push(toggle("Bold", *bold, OptMsg::WmBold));
                    }
                    WatermarkContent::Image { path, scale } => {
                        col = col
                            .push(image_picker(path, FilePickTarget::WatermarkImage))
                            .push(slider(format!("Width: {:.0}% of the page", scale * 100.0), 0.05..=1.0, *scale, 0.01, OptMsg::WmScale));
                    }
                }
                col = col
                    .push(labelled("Position", position_grid(o.position, |p| Message::Opt(OptMsg::WmPosition(p)))))
                    .push(slider(format!("Opacity: {:.0}%", o.opacity * 100.0), 0.05..=1.0, o.opacity, 0.01, OptMsg::WmOpacity))
                    .push(labelled("Angle", self.segmented(ws, SegKey::WmAngle)))
                    .push(slider(format!("Rotation: {:.0}°", o.rotation_deg), -180.0..=180.0, o.rotation_deg, 1.0, OptMsg::WmRotation))
                    .push(toggle("Repeat across the page", o.mosaic, OptMsg::WmMosaic))
                    .push(toggle("Put it behind the content", o.below_content, OptMsg::WmBelow))
                    .push(self.range_input(ws, "Pages", "Empty means every page"));
            }
            ToolOptions::Crop(o) => {
                let mm = |pt: f32| pt * 25.4 / 72.0;
                col = col
                    .push(widget::text::caption("Trim this much from each edge (1 pt ≈ 0.35 mm)."))
                    .push(slider(format!("Top: {:.0} pt ({:.0} mm)", o.top, mm(o.top)), 0.0..=200.0, o.top, 1.0, |v| OptMsg::Crop(0, v)))
                    .push(slider(format!("Right: {:.0} pt ({:.0} mm)", o.right, mm(o.right)), 0.0..=200.0, o.right, 1.0, |v| OptMsg::Crop(1, v)))
                    .push(slider(format!("Bottom: {:.0} pt ({:.0} mm)", o.bottom, mm(o.bottom)), 0.0..=200.0, o.bottom, 1.0, |v| OptMsg::Crop(2, v)))
                    .push(slider(format!("Left: {:.0} pt ({:.0} mm)", o.left, mm(o.left)), 0.0..=200.0, o.left, 1.0, |v| OptMsg::Crop(3, v)))
                    .push(self.range_input(ws, "Pages", "Empty means every page"));
            }
            ToolOptions::Sign(o) => {
                col = col.push(labelled("Signature", self.segmented(ws, SegKey::SignKind)));
                match &o.signature {
                    SignatureKind::Typed { name } => {
                        col = col.push(
                            widget::text_input("Your name", name)
                                .label("Name")
                                .helper_text("Set in a handwriting-like italic")
                                .on_input(|t| Message::Opt(OptMsg::SignName(t))),
                        );
                    }
                    SignatureKind::Image { path } => col = col.push(image_picker(path, FilePickTarget::SignatureImage)),
                }
                col = col.push(labelled("Page", self.segmented(ws, SegKey::SignPage)));
                if o.page >= 2 {
                    let max = pages.unwrap_or(9999).max(2);
                    col = col.push(crate::widgets::spin_button(format!("Page {}", o.page), "Page", o.page, 1, 1, max, |v| {
                        Message::Opt(OptMsg::SignPage(v))
                    }));
                }
                col = col
                    .push(labelled("Position", position_grid(o.position, |p| Message::Opt(OptMsg::SignPosition(p)))))
                    .push(slider(format!("Width: {:.0} pt", o.width), 60.0..=400.0, o.width, 1.0, OptMsg::SignWidth))
                    .push(toggle("Add today’s date", o.add_date, OptMsg::SignDate));
            }
            ToolOptions::Unlock => {
                let f = ws.files.first();
                let text = match f {
                    Some(f) if f.password.is_some() => "Password accepted. A copy without it will be saved.",
                    Some(f) if f.info.as_ref().is_some_and(|i| !i.encrypted) => "This PDF has no password. Unlocking just makes a copy.",
                    _ => "Add the locked PDF. You will be asked for its password.",
                };
                col = col.push(widget::text::body(text));
            }
            ToolOptions::Protect(o) => {
                let mismatch = !ws.confirm_password.is_empty() && o.user_password != ws.confirm_password;
                let mut confirm = widget::secure_input("Repeat password", &ws.confirm_password, Some(Message::Opt(OptMsg::ToggleHidePw)), ws.hide_password)
                    .label("Repeat password")
                    .on_input(|t| Message::Opt(OptMsg::ConfirmPw(t)));
                if mismatch {
                    confirm = confirm.error("The passwords do not match");
                }
                let weak = !o.user_password.is_empty() && o.user_password.chars().count() < 8;
                let mut pw = widget::secure_input("Password", &o.user_password, Some(Message::Opt(OptMsg::ToggleHidePw)), ws.hide_password)
                    .label("Password to open")
                    .on_input(|t| Message::Opt(OptMsg::UserPw(t)));
                if weak {
                    pw = pw.helper_text("Tip: 8 or more characters are safer");
                }
                let p = &o.permissions;
                let perm = |label: &'static str, v: bool, k: u8| -> Element<'a, Message> {
                    widget::checkbox(v).label(label).on_toggle(move |b| Message::Opt(OptMsg::Perm(k, b))).into()
                };
                col = col
                    .push(pw)
                    .push(confirm)
                    .push(
                        widget::secure_input("Optional", &o.owner_password, Some(Message::Opt(OptMsg::ToggleHidePw)), ws.hide_password)
                            .label("Permissions password")
                            .helper_text("Needed to change the rights below. Empty = same as above.")
                            .on_input(|t| Message::Opt(OptMsg::OwnerPw(t))),
                    )
                    .push(
                        labelled(
                            "Readers may",
                            widget::column::with_capacity(4)
                                .push(perm("Print", p.allow_print, 0))
                                .push(perm("Copy text and images", p.allow_copy, 1))
                                .push(perm("Change the document", p.allow_modify, 2))
                                .push(perm("Add comments and fill forms", p.allow_annotate, 3))
                                .spacing(s.space_xxs),
                        ),
                    )
                    .push(widget::text::caption("Passwords are never saved by Converty."));
            }
        }
        col.into()
    }
}

fn image_picker<'a>(path: &'a std::path::Path, target: FilePickTarget) -> Element<'a, Message> {
    let s = spacing();
    let name = if path.as_os_str().is_empty() {
        "No image chosen".to_string()
    } else {
        path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
    };
    labelled(
        "Image",
        widget::row::with_capacity(2)
            .push(widget::text::body(name).width(Length::Fill))
            .push(
                button::standard("Choose…")
                    .leading_icon(widget::icon::from_name("image-x-generic-symbolic"))
                    .on_press(Message::Opt(OptMsg::ChooseImage(target))),
            )
            .spacing(s.space_xs)
            .align_y(Alignment::Center),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pages::workspace::FileEntry;

    fn ws_with_pdf(tool: Tool, pages: u32) -> Workspace {
        let mut ws = Workspace::new(tool);
        let mut f = FileEntry::new("/tmp/a.pdf".into());
        f.info = Some(FileInfo { kind: FileKind::Pdf, size_bytes: 10, pages: Some(pages), encrypted: false, first_page_size: None });
        ws.files.push(f);
        ws.output_stem = ws.default_stem();
        ws.on_file_ready(0);
        ws
    }

    #[test]
    fn remove_all_pages_is_blocked() {
        let mut ws = ws_with_pdf(Tool::RemovePages, 3);
        ws.validate();
        assert!(ws.blocker.is_some());
        ws.on_opt(OptMsg::RangeText("1-3".into()));
        ws.validate();
        assert_eq!(ws.blocker.as_deref(), Some("At least one page has to stay"));
        ws.on_opt(OptMsg::RangeText("2".into()));
        ws.validate();
        assert!(ws.blocker.is_none());
        assert_eq!(ws.selected.iter().copied().collect::<Vec<_>>(), vec![2]);
        ws.on_opt(OptMsg::RangeText("9".into()));
        ws.validate();
        assert!(ws.range_error.is_some());
    }

    #[test]
    fn protect_needs_matching_passwords() {
        let mut ws = ws_with_pdf(Tool::Protect, 1);
        ws.on_opt(OptMsg::UserPw("secret123".into()));
        ws.on_opt(OptMsg::ConfirmPw("secret12".into()));
        ws.validate();
        assert_eq!(ws.blocker.as_deref(), Some("The passwords do not match"));
        ws.on_opt(OptMsg::ConfirmPw("secret123".into()));
        ws.validate();
        assert!(ws.blocker.is_none());
    }

    #[test]
    fn page_clicks_sync_to_text_and_organize_fills() {
        let mut ws = ws_with_pdf(Tool::ExtractPages, 6);
        for p in [1, 3, 4, 5] {
            ws.toggle_page(p);
        }
        assert_eq!(ws.range_text(), Some("1,3-5"));
        let org = ws_with_pdf(Tool::Organize, 4);
        assert!(matches!(&org.options, ToolOptions::Organize { pages } if pages.len() == 4));
        assert_eq!(org.default_stem(), "a_organized");
    }
}
