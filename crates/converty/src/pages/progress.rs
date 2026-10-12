//! S04: progress of the running job with Cancel.

use crate::app::{App, Message};
use crate::widgets::{category_icon, spacing};
use cosmic::iced::{Alignment, Length};
use cosmic::widget::{self, button};
use cosmic::Element;

impl App {
    pub fn view_progress(&self) -> Element<'_, Message> {
        let s = spacing();
        let Some(job) = &self.job else { return widget::text("").into() };
        let meta = job.tool.meta();
        let n = job.request.inputs.len();
        let cancelling = job.cancel.load(std::sync::atomic::Ordering::Relaxed);
        let col = widget::column::with_capacity(6)
            .push(category_icon(meta.icon, meta.category, 64))
            .push(widget::text::title3(format!("{} – {} file{}", meta.name, n, if n == 1 { "" } else { "s" })))
            .push(if job.progress <= 0.0 {
                widget::progress_bar::indeterminate_linear().girth(8.0).width(Length::Fill)
            } else {
                widget::progress_bar::determinate_linear(job.progress).girth(8.0).width(Length::Fill)
            })
            .push(widget::text::body(format!("{:.0}%  ·  {}", job.progress * 100.0, job.status)))
            .push(
                button::standard(if cancelling { "Cancelling…" } else { "Cancel" })
                    .on_press_maybe((!cancelling).then_some(Message::Cancel)),
            )
            .push(widget::text::caption("Nothing is written until the job finishes."))
            .spacing(s.space_m)
            .align_x(Alignment::Center)
            .max_width(480.0);
        widget::container(col).center_x(Length::Fill).center_y(Length::Fill).padding(s.space_l).into()
    }
}
