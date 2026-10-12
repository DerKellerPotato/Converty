//! S07: recent jobs (F14) – open results, show in folder, run again, remove.

use crate::app::{App, Message};
use crate::config::{relative_time, JobStatus};
use crate::engine;
use crate::widgets::{category_icon, spacing, tone_text, Tone};
use cosmic::iced::{Alignment, Length};
use cosmic::widget::{self, button};
use cosmic::Element;

impl App {
    pub fn view_recent(&self) -> Element<'_, Message> {
        let s = spacing();
        let mut col = widget::column::with_capacity(self.recent.len() + 2).spacing(s.space_s).width(Length::Fill);
        col = col.push(
            widget::row::with_capacity(3)
                .push(widget::text::title3("Recent jobs").width(Length::Fill))
                .push(button::destructive("Clear list").on_press_maybe((!self.recent.is_empty()).then_some(Message::RecentClear)))
                .align_y(Alignment::Center),
        );
        if self.recent.is_empty() {
            col = col.push(
                widget::container(
                    widget::column::with_capacity(2)
                        .push(widget::icon::from_name("document-open-recent-symbolic").size(48))
                        .push(widget::text::body("Nothing here yet. Finished jobs show up in this list."))
                        .spacing(s.space_s)
                        .align_x(Alignment::Center),
                )
                .center_x(Length::Fill)
                .padding(s.space_xl),
            );
        }
        for (i, job) in self.recent.iter().enumerate() {
            let meta = job.tool.meta();
            let inputs: Vec<String> = job
                .inputs
                .iter()
                .take(2)
                .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
                .collect();
            let more = job.inputs.len().saturating_sub(2);
            let inputs = if more > 0 { format!("{} +{more}", inputs.join(", ")) } else { inputs.join(", ") };
            let first_out = job.outputs.first().cloned();
            let out_exists = first_out.as_ref().is_some_and(|p| p.exists());
            let status: Element<'_, Message> = match &job.status {
                JobStatus::Done if out_exists => widget::text::caption(format!(
                    "{} result{} · {}",
                    job.outputs.len(),
                    if job.outputs.len() == 1 { "" } else { "s" },
                    engine::format_bytes(job.output_bytes)
                ))
                .into(),
                JobStatus::Done => tone_text("Result was moved or deleted", Tone::Warning),
                JobStatus::Failed(m) => tone_text(format!("Failed: {m}"), Tone::Danger),
            };
            let mut actions = widget::row::with_capacity(4).spacing(s.space_xxs).align_y(Alignment::Center);
            if let Some(p) = &first_out {
                actions = actions
                    .push(button::text("Open").on_press_maybe(out_exists.then(|| Message::OpenPath(p.clone()))))
                    .push(widget::tooltip(
                        button::icon(widget::icon::from_name("folder-open-symbolic"))
                            .on_press_maybe(out_exists.then(|| Message::ShowInFolder(p.clone()))),
                        widget::text::caption("Show in folder"),
                        widget::tooltip::Position::Top,
                    ));
            }
            actions = actions
                .push(widget::tooltip(
                    button::icon(widget::icon::from_name("view-refresh-symbolic")).on_press(Message::RecentRerun(i)),
                    widget::text::caption("Run again with the same files and options"),
                    widget::tooltip::Position::Top,
                ))
                .push(widget::tooltip(
                    button::icon(widget::icon::from_name("window-close-symbolic")).on_press(Message::RecentRemove(i)),
                    widget::text::caption("Remove from list"),
                    widget::tooltip::Position::Top,
                ));
            let row = widget::row::with_capacity(3)
                .push(category_icon(meta.icon, meta.category, 24))
                .push(
                    widget::column::with_capacity(3)
                        .push(
                            widget::row::with_capacity(2)
                                .push(widget::text::heading(meta.name))
                                .push(widget::text::caption(relative_time(job.finished_at)))
                                .spacing(s.space_xs)
                                .align_y(Alignment::Center),
                        )
                        .push(widget::text::caption(inputs))
                        .push(status)
                        .width(Length::Fill),
                )
                .push(actions)
                .spacing(s.space_s)
                .align_y(Alignment::Center);
            col = col.push(widget::container(row).padding(s.space_s).class(cosmic::style::Container::Card));
        }
        widget::scrollable(widget::container(widget::container(col).max_width(860.0)).center_x(Length::Fill).padding(s.space_l))
            .height(Length::Fill)
            .into()
    }
}
