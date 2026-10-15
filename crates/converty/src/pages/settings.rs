//! S06: output location, open-after-run, dependency status with install hints.

use crate::app::{App, Message};
use crate::config::OutputMode;
use crate::widgets::{spacing, tone_icon, tone_text, Tone};
use cosmic::iced::{Alignment, Length};
use cosmic::widget::{self, button, settings};
use cosmic::Element;

/// What each helper program is used for.
fn used_for(program: &str) -> &'static str {
    match program {
        "gs" => "Compress, PDF/A, Protect, Unlock",
        "pdftoppm" => "Thumbnails and PDF to images",
        "pdfimages" => "Pulling images out of a PDF",
        "soffice" => "Office and HTML conversions",
        "ffmpeg" => "Video conversion and audio extraction",
        "ffprobe" => "Reading video files",
        _ => "Some tools",
    }
}

impl App {
    pub fn view_settings(&self) -> Element<'_, Message> {
        let s = spacing();
        let mode = match &self.settings.output_mode {
            OutputMode::SameAsInput => 0u8,
            OutputMode::Folder(_) => 1,
            OutputMode::Ask => 2,
        };
        let folder_label = match &self.settings.output_mode {
            OutputMode::Folder(p) => format!("Always this folder: {}", p.display()),
            _ => "Always the same folder…".to_string(),
        };
        let mut output = settings::section()
            .title("Where results go")
            .add(widget::radio(widget::text::body("Next to the original file"), 0u8, Some(mode), Message::SetOutputMode))
            .add(
                widget::row::with_capacity(2)
                    .push(widget::radio(widget::text::body(folder_label), 1u8, Some(mode), Message::SetOutputMode).width(Length::Fill))
                    .push_maybe((mode == 1).then(|| button::standard("Change…").on_press(Message::ChooseOutputFolder)))
                    .align_y(Alignment::Center),
            )
            .add(widget::radio(widget::text::body("Ask every time"), 2u8, Some(mode), Message::SetOutputMode));
        output = output.add(settings::item(
            "Open the result when a job finishes",
            widget::toggler(self.settings.open_result).on_toggle(Message::SetOpenResult),
        ));

        let mut deps = settings::section().title("Helper programs");
        match &self.deps {
            None if self.deps_loading => deps = deps.add(widget::text::body("Checking…")),
            None => {
                deps = deps.add(tone_text("Could not check the helper programs right now.", Tone::Warning));
            }
            Some(status) => {
                for d in &status.deps {
                    let state: Element<'_, Message> = if d.found {
                        widget::row::with_capacity(2)
                            .push(tone_icon("emblem-ok-symbolic", Tone::Success, 16))
                            .push(widget::text::caption("Installed"))
                            .spacing(s.space_xxs)
                            .align_y(Alignment::Center)
                            .into()
                    } else {
                        widget::column::with_capacity(2)
                            .push(tone_text("Not found", Tone::Warning))
                            .push(widget::text::monotext(format!("sudo apt install {}", d.package)))
                            .push(widget::text::caption(format!("Fedora: dnf install {0} · Arch: pacman -S {0}", d.package)))
                            .align_x(Alignment::End)
                            .into()
                    };
                    deps = deps.add(
                        widget::row::with_capacity(2)
                            .push(
                                widget::column::with_capacity(2)
                                    .push(widget::text::body(d.program))
                                    .push(widget::text::caption(used_for(d.program)))
                                    .width(Length::Fill),
                            )
                            .push(state)
                            .align_y(Alignment::Center),
                    );
                }
            }
        }
        deps = deps.add(
            widget::row::with_capacity(2)
                .push(widget::text::caption("Installed something new?").width(Length::Fill))
                .push(button::standard("Check again").on_press_maybe((!self.deps_loading).then_some(Message::RefreshDeps)))
                .align_y(Alignment::Center),
        );

        let privacy = settings::section().title("Privacy").add(widget::text::body(
            "Converty works offline. Passwords stay in memory only and are never written to disk.",
        ));

        let col = settings::view_column(vec![widget::text::title3("Settings").into(), output.into(), deps.into(), privacy.into()]);
        widget::scrollable(widget::container(widget::container(col).max_width(760.0)).center_x(Length::Fill).padding(s.space_l))
            .height(Length::Fill)
            .into()
    }
}
