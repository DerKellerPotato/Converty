//! S01: tool grid with category chips, search and drop-to-suggest (F13).

use crate::app::{App, Message};
use crate::widgets::{card_button_class, A11y, category_icon, spacing, tone_text, Tone};
use converty_core::{tools, Category, FileKind, ToolMeta};
use cosmic::iced::{Alignment, Length};
use cosmic::widget::{self, button};
use cosmic::Element;
use std::collections::HashSet;

const CARD_W: f32 = 228.0;
const CARD_H: f32 = 156.0;

impl App {
    fn tool_visible(&self, meta: &ToolMeta, dropped_kinds: &HashSet<FileKind>) -> bool {
        if let Some(c) = self.category {
            if meta.category != c {
                return false;
            }
        }
        if !dropped_kinds.is_empty() && !dropped_kinds.iter().any(|k| meta.accepts.contains(k)) {
            return false;
        }
        let q = self.search.trim().to_lowercase();
        q.is_empty()
            || meta.name.to_lowercase().contains(&q)
            || meta.blurb.to_lowercase().contains(&q)
            || meta.category.label().to_lowercase().contains(&q)
    }

    pub fn view_home(&self) -> Element<'_, Message> {
        let s = spacing();
        let dropped_kinds: HashSet<FileKind> = self
            .dropped
            .iter()
            .map(|p| FileKind::from_path(p.as_path()))
            .filter(|k| *k != FileKind::Other)
            .collect();

        let search = widget::search_input("Search tools", &self.search)
            .id(self.search_id.clone())
            .on_input(Message::Search)
            .on_clear(Message::Search(String::new()))
            .width(Length::Fixed(320.0));

        let chip = |label: &'static str, value: Option<Category>| -> Element<'_, Message> {
            let selected = self.category == value;
            let b = if selected { button::suggested(label) } else { button::standard(label) };
            b.on_press(Message::Category(value)).into()
        };
        let mut chips: Vec<Element<'_, Message>> = vec![chip("All", None)];
        chips.extend(Category::ALL.iter().map(|c| chip(c.label(), Some(*c))));
        let chips = widget::flex_row(chips).row_spacing(s.space_xs).column_spacing(s.space_xs);

        let mut col = widget::column::with_capacity(6).spacing(s.space_m).width(Length::Fill);

        col = col.push(
            widget::column::with_capacity(2)
                .push(widget::text::title2("What do you want to do?"))
                .push(widget::text::body("Every tool runs on this computer. Your files never leave it.")),
        );

        if !self.dropped.is_empty() {
            let names: Vec<String> = self
                .dropped
                .iter()
                .take(3)
                .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
                .collect();
            let more = self.dropped.len().saturating_sub(3);
            let text = if more > 0 { format!("{} and {more} more", names.join(", ")) } else { names.join(", ") };
            col = col.push(
                widget::container(
                    widget::row::with_capacity(3)
                        .push(widget::icon::from_name("document-send-symbolic").size(24))
                        .push(
                            widget::column::with_capacity(2)
                                .push(widget::text::heading(format!(
                                    "{} file{} ready – pick a tool",
                                    self.dropped.len(),
                                    if self.dropped.len() == 1 { "" } else { "s" }
                                )))
                                .push(widget::text::caption(text))
                                .width(Length::Fill),
                        )
                        .push(button::standard("Clear").on_press(Message::ClearDropped))
                        .spacing(s.space_s)
                        .align_y(Alignment::Center),
                )
                .padding(s.space_s)
                .class(cosmic::style::Container::Card),
            );
        }

        col = col.push(
            widget::row::with_capacity(2)
                .push(search)
                .push(widget::container(chips).width(Length::Fill))
                .spacing(s.space_m)
                .align_y(Alignment::Center),
        );

        let cards: Vec<Element<'_, Message>> = tools()
            .iter()
            .filter(|m| self.tool_visible(m, &dropped_kinds))
            .map(|m| self.tool_card(m))
            .collect();

        if cards.is_empty() {
            col = col.push(
                widget::container(
                    widget::column::with_capacity(2)
                        .push(widget::icon::from_name("edit-find-symbolic").size(48))
                        .push(widget::text::body("No tool matches. Try another word or category."))
                        .spacing(s.space_s)
                        .align_x(Alignment::Center),
                )
                .center_x(Length::Fill)
                .padding(s.space_xl),
            );
        } else {
            col = col.push(
                widget::flex_row(cards)
                    .row_spacing(s.space_s)
                    .column_spacing(s.space_s)
                    .width(Length::Fill),
            );
        }

        widget::scrollable(widget::container(col).padding([s.space_s, s.space_l]).max_width(1400.0).center_x(Length::Fill))
            .height(Length::Fill)
            .into()
    }

    fn tool_card<'a>(&'a self, meta: &'static ToolMeta) -> Element<'a, Message> {
        let s = spacing();
        let missing = self.missing_deps(meta.tool);
        let mut content = widget::column::with_capacity(4)
            .push(category_icon(meta.icon, meta.category, 32))
            .push(widget::text::heading(meta.name))
            .push(widget::text::caption(meta.blurb))
            .spacing(s.space_xxs)
            .width(Length::Fill);
        if !missing.is_empty() {
            content = content.push(tone_text(format!("Needs {}", missing.join(", ")), Tone::Warning));
        }
        let card = button::custom(content)
            .padding(s.space_m)
            .width(Length::Fixed(CARD_W))
            .height(Length::Fixed(CARD_H))
            .class(card_button_class(false))
            .a11y_name(meta.name)
            .a11y_description(meta.blurb)
            .on_press_maybe(missing.is_empty().then_some(Message::OpenTool(meta.tool)));
        if missing.is_empty() {
            card.into()
        } else {
            widget::tooltip(
                card,
                widget::text::caption("Install the missing program, then reopen Converty or refresh in Settings"),
                widget::tooltip::Position::Bottom,
            )
            .into()
        }
    }
}
