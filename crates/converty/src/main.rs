//! Converty: offline PDF tools for the desktop (COSMIC, KDE Plasma, GNOME, Windows, macOS).

// No console window next to the app on Windows.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod app;
mod config;
mod engine;
mod pages;
mod widgets;

use std::collections::HashSet;
use std::sync::Mutex;

fn main() -> cosmic::iced::Result {
    // Print each distinct engine panic once: the UI catches them and shows an error,
    // so repeating the same backtrace on every call is just noise.
    let seen: Mutex<HashSet<String>> = Mutex::new(HashSet::new());
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let key = info.location().map(|l| l.to_string()).unwrap_or_default();
        if seen.lock().map(|mut s| s.insert(key)).unwrap_or(true) {
            default_hook(info);
        }
    }));

    let settings = cosmic::app::Settings::default()
        .size(cosmic::iced::Size::new(1180.0, 800.0))
        .size_limits(cosmic::iced::Limits::NONE.min_width(640.0).min_height(480.0));
    cosmic::app::run::<app::App>(settings, ())
}
