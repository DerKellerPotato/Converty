//! S05: result summary and actions, or the error state (S08) when the job failed.

use crate::app::{App, Message};
use crate::engine::{self, ErrKind, UiError};
use crate::widgets::{spacing, tone_icon, tone_text, Tone};
use converty_core::{FileKind, JobResult, Tool};
use cosmic::iced::{Alignment, Length};
use cosmic::widget::{self, button};
use cosmic::Element;

impl App {
    pub fn view_result(&self) -> Element<'_, Message> {
        let s = spacing();
        let Some((tool, result)) = &self.result else { return widget::text("").into() };
        let body = match result {
            Ok(r) => self.view_success(*tool, r),
            Err(e) => self.view_failure(*tool, e),
        };
        widget::scrollable(
            widget::container(widget::container(body).max_width(640.0))
                .center_x(Length::Fill)
                .padding(s.space_l),
        )
        .height(Length::Fill)
        .into()
    }

    fn view_success<'a>(&'a self, tool: Tool, r: &'a JobResult) -> Element<'a, Message> {
        let s = spacing();
        let n = r.outputs.len();
        let mut col = widget::column::with_capacity(10)
            .push(tone_icon("emblem-ok-symbolic", Tone::Success, 64))
            .push(widget::text::title2(match n {
                0 => "Finished".to_string(),
                1 => "Your file is ready".to_string(),
                _ => format!("{n} files are ready"),
            }))
            .spacing(s.space_m)
            .align_x(Alignment::Center)
            .width(Length::Fill);

        // Size summary; compression shows before → after with the saving.
        let size_line = if tool == Tool::Compress && r.input_bytes > 0 {
            let saved = 100.0 - (r.output_bytes as f64 / r.input_bytes as f64 * 100.0);
            format!(
                "{} → {}  ·  {}",
                engine::format_bytes(r.input_bytes),
                engine::format_bytes(r.output_bytes),
                if saved >= 0.5 { format!("{saved:.0}% smaller") } else { "no saving".into() }
            )
        } else {
            format!("{} in, {} out", engine::format_bytes(r.input_bytes), engine::format_bytes(r.output_bytes))
        };
        col = col.push(widget::text::body(size_line));
        for note in &r.notes {
            col = col.push(tone_text(note.as_str(), Tone::Warning));
        }

        // Output list (first few) with per-file actions.
        let mut list = widget::column::with_capacity(n.min(8)).spacing(s.space_xxs);
        for p in r.outputs.iter().take(8) {
            let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            list = list.push(
                widget::container(
                    widget::row::with_capacity(3)
                        .push(widget::icon::from_name("text-x-generic-symbolic").size(16))
                        .push(widget::text::body(name).width(Length::Fill))
                        .push(button::text("Open").on_press(Message::OpenPath(p.clone())))
                        .spacing(s.space_xs)
                        .align_y(Alignment::Center),
                )
                .padding([s.space_xxs, s.space_s])
                .class(cosmic::style::Container::Card),
            );
        }
        if n > 8 {
            list = list.push(widget::text::caption(format!("…and {} more in the same folder", n - 8)));
        }
        if let Some(dir) = r.outputs.first().and_then(|p| p.parent()) {
            list = list.push(widget::text::caption(format!("Saved in {}", dir.display())));
        }
        col = col.push(list);

        let first = r.outputs.first().cloned();
        let mut actions = widget::row::with_capacity(4).spacing(s.space_s);
        if let Some(first) = &first {
            actions = actions
                .push(
                    button::suggested(if n == 1 { "Open" } else { "Open first" })
                        .leading_icon(widget::icon::from_name("document-open-symbolic"))
                        .on_press(Message::OpenPath(first.clone())),
                )
                .push(
                    button::standard("Show in folder")
                        .leading_icon(widget::icon::from_name("folder-open-symbolic"))
                        .on_press(Message::ShowInFolder(first.clone())),
                );
        }
        let can_continue = !self.continue_tools().is_empty();
        actions = actions
            .push(
                button::standard("Continue with…")
                    .leading_icon(widget::icon::from_name("go-next-symbolic"))
                    .on_press_maybe(can_continue.then_some(Message::ContinueWith)),
            )
            .push(button::text("Start over").on_press(Message::StartOver));
        col.push(widget::flex_row(vec![actions.into()])).into()
    }

    fn view_failure<'a>(&'a self, tool: Tool, e: &'a UiError) -> Element<'a, Message> {
        let s = spacing();
        let (title, hint): (&str, String) = match &e.kind {
            ErrKind::MissingDependency { program, package } => (
                "A helper program is missing",
                format!("Install the package “{package}” (it provides “{program}”), then try again. On Pop!_OS or Ubuntu: sudo apt install {package}"),
            ),
            ErrKind::Encrypted(_) => ("This PDF is locked", "Unlock it first, then come back to this tool.".into()),
            ErrKind::WrongPassword(_) => ("Wrong password", "Remove the file and add it again with the right password.".into()),
            ErrKind::Corrupt(_) => ("The file is damaged", "Repair PDF can often rescue the readable pages.".into()),
            ErrKind::WrongType => ("This tool can’t use that file", "Pick a tool that takes this kind of file.".into()),
            ErrKind::Internal => ("Something went wrong inside Converty", "This is a bug. Your files were not changed.".into()),
            _ => ("That didn’t work", "Your original files were not changed.".into()),
        };
        let col = widget::column::with_capacity(8)
            .push(tone_icon("dialog-error-symbolic", Tone::Danger, 64))
            .push(widget::text::title2(title))
            .push(widget::text::body(e.message.as_str()))
            .push(widget::text::caption(hint))
            .spacing(s.space_m)
            .align_x(Alignment::Center)
            .width(Length::Fill);

        let mut actions = widget::row::with_capacity(4).spacing(s.space_s);
        match &e.kind {
            ErrKind::Encrypted(p) if tool != Tool::Unlock => {
                actions = actions.push(button::suggested("Unlock it").on_press(Message::GoToTool(Tool::Unlock, p.clone())));
            }
            ErrKind::Corrupt(p) if tool != Tool::Repair => {
                actions = actions.push(button::suggested("Repair it").on_press(Message::GoToTool(Tool::Repair, p.clone())));
            }
            ErrKind::MissingDependency { .. } => {
                actions = actions.push(button::suggested("Open Settings").on_press(Message::OpenSettings));
            }
            _ => {}
        }
        actions = actions
            .push(button::standard("Back to options").on_press(Message::BackToOptions))
            .push(button::text("Start over").on_press(Message::StartOver));
        col.push(actions).into()
    }

    /// Tools that can take the current results as input (F12).
    pub fn continue_tools(&self) -> Vec<Tool> {
        let Some((from, Ok(r))) = &self.result else { return Vec::new() };
        let kinds: Vec<_> = r.outputs.iter().map(|p| FileKind::from_path(p.as_path())).collect();
        if kinds.is_empty() {
            return Vec::new();
        }
        converty_core::tools()
            .iter()
            .filter(|m| m.tool != *from || m.tool == Tool::Merge)
            .filter(|m| kinds.iter().all(|k| m.accepts.contains(k)))
            .filter(|m| self.missing_deps(m.tool).is_empty())
            .map(|m| m.tool)
            .collect()
    }
}
