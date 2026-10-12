//! Modal dialogs: password for locked PDFs (F15), "Continue with…" (F12), errors (S08).

use crate::app::{App, DialogKind, Message};
use crate::widgets::{category_icon, spacing};
use cosmic::iced::{Alignment, Length};
use cosmic::widget::{self, button};
use cosmic::Element;

impl App {
    pub fn view_dialog(&self) -> Option<Element<'_, Message>> {
        let s = spacing();
        let dialog = self.dialog.as_ref()?;
        Some(match dialog {
            DialogKind::Password(p) => {
                let name = p.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let mut input = widget::secure_input("Password", &p.input, Some(Message::PwToggleHidden), p.hidden)
                    .on_input(Message::PwInput)
                    .on_submit(|_| Message::PwSubmit)
                    .width(Length::Fill);
                if let Some(e) = &p.error {
                    input = input.error(e.as_str());
                }
                widget::dialog()
                    .title("This PDF is locked")
                    .icon(widget::icon::from_name("dialog-password-symbolic").size(48))
                    .body(format!("“{name}” needs a password before Converty can read it."))
                    .control(input)
                    .primary_action(
                        button::suggested(if p.checking { "Checking…" } else { "Unlock" })
                            .on_press_maybe((!p.input.is_empty() && !p.checking).then_some(Message::PwSubmit)),
                    )
                    .secondary_action(button::standard("Skip this file").on_press(Message::PwCancel))
                    .into()
            }
            DialogKind::ContinueWith => {
                let tools = self.continue_tools();
                let mut list = widget::column::with_capacity(tools.len()).spacing(s.space_xxs);
                for t in tools {
                    let m = t.meta();
                    list = list.push(
                        button::custom(
                            widget::row::with_capacity(2)
                                .push(category_icon(m.icon, m.category, 20))
                                .push(
                                    widget::column::with_capacity(2)
                                        .push(widget::text::body(m.name))
                                        .push(widget::text::caption(m.blurb)),
                                )
                                .spacing(s.space_s)
                                .align_y(Alignment::Center),
                        )
                        .class(crate::widgets::card_button_class(false))
                        .padding(s.space_xs)
                        .width(Length::Fill)
                        .on_press(Message::ContinueTool(t)),
                    );
                }
                widget::dialog()
                    .title("Continue with another tool")
                    .body("The result is loaded into the tool you pick.")
                    .control(widget::scrollable(list).height(Length::Fixed(360.0)))
                    .secondary_action(button::standard("Close").on_press(Message::CloseDialog))
                    .into()
            }
            DialogKind::Error { title, body } => widget::dialog()
                .title(title.as_str())
                .icon(widget::icon::from_name("dialog-error-symbolic").size(48))
                .body(body.as_str())
                .primary_action(button::suggested("OK").on_press(Message::CloseDialog))
                .into(),
        })
    }
}
