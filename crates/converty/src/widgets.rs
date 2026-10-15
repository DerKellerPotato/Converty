//! Small reusable pieces: card button style, category colours, 3x3 position grid,
//! colour swatches, labelled rows and segmented-control models.

use crate::app::Message;
use converty_core::{Category, Position, Rgb};
use cosmic::iced::{Alignment, Background, Border, Color, Length};
use cosmic::theme;
use cosmic::widget::{self, button, segmented_button};
use cosmic::Element;

/// Spacing tokens of the active theme.
pub fn spacing() -> cosmic::cosmic_theme::Spacing {
    theme::spacing()
}

/// COSMIC palette colour for a tool category.
pub fn category_color(t: &cosmic::Theme, c: Category) -> Color {
    let p = &t.cosmic().palette;
    let c = match c {
        Category::Organize => p.accent_blue,
        Category::Optimize => p.accent_green,
        Category::ConvertToPdf => p.accent_orange,
        Category::ConvertFromPdf => p.accent_purple,
        Category::Edit => p.accent_red,
        Category::Security => p.accent_warm_grey,
        Category::Video => p.accent_pink,
    };
    c.into()
}

/// A symbolic icon tinted with the category colour.
pub fn category_icon<'a>(name: &'static str, c: Category, size: u16) -> Element<'a, Message> {
    widget::icon::from_name(name)
        .size(size)
        .icon()
        .class(theme::Svg::custom(move |t| cosmic::iced::widget::svg::Style {
            color: Some(category_color(t, c)),
        }))
        .into()
}

/// Icon tinted with a semantic theme colour (success / warning / destructive / accent).
#[derive(Clone, Copy)]
pub enum Tone {
    Success,
    Warning,
    Danger,
}

pub fn tone_color(t: &cosmic::Theme, tone: Tone) -> Color {
    let c = t.cosmic();
    match tone {
        Tone::Success => c.success_text_color().into(),
        Tone::Warning => c.warning_text_color().into(),
        Tone::Danger => c.destructive_text_color().into(),
    }
}

pub fn tone_icon<'a>(name: &'static str, tone: Tone, size: u16) -> Element<'a, Message> {
    widget::icon::from_name(name)
        .size(size)
        .icon()
        .class(theme::Svg::custom(move |t| cosmic::iced::widget::svg::Style {
            color: Some(tone_color(t, tone)),
        }))
        .into()
}

/// Caption text in a semantic colour.
pub fn tone_text<'a>(text: impl Into<std::borrow::Cow<'a, str>> + 'a, tone: Tone) -> Element<'a, Message> {
    let f: fn(&cosmic::Theme) -> cosmic::iced::widget::text::Style = match tone {
        Tone::Success => |t| cosmic::iced::widget::text::Style { color: Some(tone_color(t, Tone::Success)), ..Default::default() },
        Tone::Warning => |t| cosmic::iced::widget::text::Style { color: Some(tone_color(t, Tone::Warning)), ..Default::default() },
        Tone::Danger => |t| cosmic::iced::widget::text::Style { color: Some(tone_color(t, Tone::Danger)), ..Default::default() },
    };
    widget::text::caption(text).class(theme::Text::Custom(f)).into()
}

/// Card-like button: surface colour of the current layer, hover/pressed states,
/// accent focus ring, dimmed when disabled.
pub fn card_button_class(selected: bool) -> theme::Button {
    fn style(t: &cosmic::Theme, state: u8, focused: bool, selected: bool) -> button::Style {
        let c = t.cosmic();
        let bgc = c.background(false);
        let comp = &bgc.component;
        let bg = match state {
            1 => comp.hover,
            2 => comp.pressed,
            _ => comp.base,
        };
        let mut s = button::Style::new();
        s.background = Some(Background::Color(bg.into()));
        s.border_radius = c.corner_radii.radius_m.into();
        s.text_color = Some(comp.on.into());
        if focused || selected {
            s.border_width = 2.0;
            s.border_color = c.accent.base.into();
        }
        if state == 3 {
            let mut on = Color::from(comp.on);
            on.a = 0.45;
            s.text_color = Some(on);
            s.background = Some(Background::Color(comp.base.into()));
        }
        s
    }
    theme::Button::Custom {
        active: Box::new(move |focused, t| style(t, 0, focused, selected)),
        disabled: Box::new(move |t| style(t, 3, false, selected)),
        hovered: Box::new(move |focused, t| style(t, 1, focused, selected)),
        pressed: Box::new(move |focused, t| style(t, 2, focused, selected)),
    }
}

/// Dashed-looking drop zone container style (accent border while hovering a drag).
pub fn drop_zone_class<'a>(active: bool) -> theme::Container<'a> {
    theme::Container::custom(move |t: &cosmic::Theme| {
        let c = t.cosmic();
        let bgc = c.background(false);
        let comp = &bgc.component;
        cosmic::iced::widget::container::Style {
            background: Some(Background::Color(if active { comp.hover.into() } else { comp.base.into() })),
            border: Border {
                radius: c.corner_radii.radius_l.into(),
                width: 2.0,
                color: if active { c.accent.base.into() } else { comp.divider.into() },
            },
            text_color: Some(comp.on.into()),
            icon_color: Some(comp.on.into()),
            ..Default::default()
        }
    })
}

/// "Label ............ control" row used in the options sidebar.
pub fn labelled<'a>(label: &'a str, control: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    widget::column::with_capacity(2)
        .push(widget::text::caption_heading(label))
        .push(control)
        .spacing(spacing().space_xxs)
        .width(Length::Fill)
        .into()
}

/// 3x3 grid of toggle buttons for page placement.
pub fn position_grid<'a>(selected: Position, on: fn(Position) -> Message) -> Element<'a, Message> {
    let s = spacing();
    let mut col = widget::column::with_capacity(3).spacing(s.space_xxs);
    for row_items in Position::ALL.chunks(3) {
        let mut row = widget::row::with_capacity(3).spacing(s.space_xxs);
        for &p in row_items {
            let is = p == selected;
            let dot = widget::icon::from_name(if is { "radio-checked-symbolic" } else { "radio-symbolic" })
                .size(16);
            row = row.push(
                button::custom(
                    widget::container(dot.icon())
                        .center_x(Length::Fixed(44.0))
                        .center_y(Length::Fixed(32.0)),
                )
                .padding(0)
                .class(card_button_class(is))
                .selected(is)
                .on_press(on(p)),
            );
        }
        col = col.push(row);
    }
    col.into()
}

pub const COLOR_PRESETS: [(&str, Rgb); 6] = [
    ("Black", Rgb(0, 0, 0)),
    ("Grey", Rgb(110, 110, 110)),
    ("Red", Rgb(200, 30, 30)),
    ("Blue", Rgb(30, 80, 200)),
    ("Green", Rgb(20, 140, 60)),
    ("White", Rgb(255, 255, 255)),
];

/// Row of colour swatches. These colours are document data (ink colour), not UI theme colours.
pub fn color_swatches<'a>(selected: Rgb, on: fn(Rgb) -> Message) -> Element<'a, Message> {
    let s = spacing();
    let mut row = widget::row::with_capacity(COLOR_PRESETS.len()).spacing(s.space_xxs);
    for (name, rgb) in COLOR_PRESETS {
        let is = rgb == selected;
        let swatch = widget::container(widget::Space::new())
            .width(Length::Fixed(22.0))
            .height(Length::Fixed(22.0))
            .class(theme::Container::custom(move |t: &cosmic::Theme| {
                let c = t.cosmic();
                cosmic::iced::widget::container::Style {
                    background: Some(Background::Color(Color::from_rgb8(rgb.0, rgb.1, rgb.2))),
                    border: Border {
                        radius: c.corner_radii.radius_xl.into(),
                        width: 1.0,
                        color: c.background(false).component.divider.into(),
                    },
                    ..Default::default()
                }
            }));
        row = row.push(
            widget::tooltip(
                button::custom(swatch)
                    .padding(s.space_xxs)
                    .class(card_button_class(is))
                    .selected(is)
                    .on_press(on(rgb)),
                widget::text::caption(name),
                widget::tooltip::Position::Bottom,
            ),
        );
    }
    row.align_y(Alignment::Center).into()
}

/// Builds a single-select segmented model from labels with `active` selected.
pub fn seg_model(labels: &[&'static str], active: usize) -> segmented_button::SingleSelectModel {
    let mut model = segmented_button::SingleSelectModel::default();
    for (i, label) in labels.iter().enumerate() {
        let e = model.insert().text(*label).data(i);
        if i == active {
            e.activate();
        }
    }
    model
}

/// Accessibility names and descriptions for buttons. libcosmic only has them with its
/// `a11y` feature, which is enabled on Linux; elsewhere these are no-ops.
pub trait A11y<'a> {
    fn a11y_name(self, name: impl Into<std::borrow::Cow<'a, str>>) -> Self;
    fn a11y_description(self, description: impl Into<std::borrow::Cow<'a, str>>) -> Self;
}

impl<'a, M: Clone + 'a> A11y<'a> for cosmic::widget::Button<'a, M> {
    #[cfg(target_os = "linux")]
    fn a11y_name(self, name: impl Into<std::borrow::Cow<'a, str>>) -> Self {
        self.name(name)
    }
    #[cfg(not(target_os = "linux"))]
    fn a11y_name(self, _name: impl Into<std::borrow::Cow<'a, str>>) -> Self {
        self
    }
    #[cfg(target_os = "linux")]
    fn a11y_description(self, description: impl Into<std::borrow::Cow<'a, str>>) -> Self {
        self.description(description)
    }
    #[cfg(not(target_os = "linux"))]
    fn a11y_description(self, _description: impl Into<std::borrow::Cow<'a, str>>) -> Self {
        self
    }
}

/// Spin button with an accessible name (the name is only used on Linux, see [`A11y`]).
pub fn spin_button<'a, T, M>(
    label: impl Into<std::borrow::Cow<'a, str>>,
    name: impl Into<std::borrow::Cow<'a, str>>,
    value: T,
    step: T,
    min: T,
    max: T,
    on_press: impl Fn(T) -> M + 'static,
) -> widget::SpinButton<'a, T, M>
where
    T: Copy + std::ops::Sub<Output = T> + std::ops::Add<Output = T> + PartialOrd,
{
    #[cfg(target_os = "linux")]
    return widget::spin_button(label, name, value, step, min, max, on_press);
    #[cfg(not(target_os = "linux"))]
    {
        let _ = name.into();
        widget::spin_button(label, value, step, min, max, on_press)
    }
}

/// Adds a `*.ext` glob to a file filter (XDG portal on Linux, extension for rfd elsewhere).
pub fn filter_glob(
    f: cosmic::dialog::file_chooser::FileFilter,
    glob: &str,
) -> cosmic::dialog::file_chooser::FileFilter {
    #[cfg(target_os = "linux")]
    return f.glob(glob);
    #[cfg(not(target_os = "linux"))]
    {
        let ext = glob.trim_start_matches("*.");
        // rfd matches extensions case-insensitively, so skip upper-case duplicates.
        if ext.chars().any(|c| c.is_ascii_uppercase()) {
            f
        } else {
            f.extension(ext)
        }
    }
}
