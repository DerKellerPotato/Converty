//! Application shell: state, messages, navigation, header bar, keyboard, drops.

use crate::config::{self, OutputMode, RecentJob, Settings};
use crate::engine::{self, ErrKind, UiError};
use crate::pages::options::{OptMsg, SegKey};
use crate::pages::workspace::Workspace;
use converty_core::{
    Category, DepStatus, FileInfo, FileKind, Input, JobRequest, JobResult, PageOp, Tool, ToolOptions,
};
use cosmic::app::context_drawer::{self, ContextDrawer};
use cosmic::app::{Core, Task};
use cosmic::dialog::file_chooser::{self, FileFilter};
use cosmic::iced::keyboard::{self, key::Named, Key};
use cosmic::iced::{event, window, Event, Length, Subscription};
use cosmic::widget::{self, about::About, image, segmented_button, Toast, ToastId, Toasts};
use cosmic::{Application, ApplicationExt, Element};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub const APP_ID: &str = "io.github.converty.Converty";
/// Longest side of rendered thumbnails, in pixels.
pub const THUMB_PX: u32 = 220;
const THUMB_PARALLEL: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Home,
    Workspace,
    Processing,
    Result,
    Recent,
    Settings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FilePickTarget {
    /// Inputs of the current tool.
    Inputs,
    /// Watermark image.
    WatermarkImage,
    /// Signature image.
    SignatureImage,
}

pub type ThumbKey = (PathBuf, u32);

pub enum Thumb {
    Loading,
    Ready(image::Handle),
    Failed,
}

pub struct JobState {
    pub id: u64,
    pub tool: Tool,
    pub cancel: Arc<AtomicBool>,
    pub progress: f32,
    pub status: String,
    pub request: JobRequest,
}

pub struct PasswordPrompt {
    pub path: PathBuf,
    pub input: String,
    pub hidden: bool,
    pub error: Option<String>,
    pub checking: bool,
}

pub enum DialogKind {
    Password(PasswordPrompt),
    ContinueWith,
    Error { title: String, body: String },
}

#[derive(Debug, Clone)]
pub enum Message {
    // Navigation
    Back,
    OpenRecent,
    OpenSettings,
    ToggleAbout,
    OpenUrl(String),
    Escape,
    FocusSearch,
    // Home
    Search(String),
    Category(Option<Category>),
    OpenTool(Tool),
    ClearDropped,
    // Drops
    DndEnter,
    DndLeave,
    DndDrop(Vec<u8>),
    FilesDropped(Vec<PathBuf>),
    // Files
    ChooseFiles(FilePickTarget),
    FilesChosen(FilePickTarget, Vec<PathBuf>),
    PickerFailed(String),
    Inspected(PathBuf, Result<FileInfo, UiError>),
    RemoveFile(usize),
    MoveFile(usize, isize),
    SortByName,
    RepairFile(PathBuf),
    // Thumbnails
    ThumbLoaded(ThumbKey, Option<(u32, u32, Vec<u8>)>),
    ThumbScroll(cosmic::iced::widget::scrollable::Viewport),
    // Page grid
    TogglePage(u32),
    SelectAllPages(bool),
    OrgSelect(usize),
    OrgMove(usize, isize),
    OrgRotate(usize, bool),
    OrgDelete(usize),
    OrgDuplicate(usize),
    OrgInsertBlank(usize),
    OrgReset,
    // Options
    Seg(SegKey, segmented_button::Entity),
    Opt(OptMsg),
    // Password dialog (F15)
    PwInput(String),
    PwToggleHidden,
    PwSubmit,
    PwCancel,
    PwChecked(PathBuf, String, Result<FileInfo, UiError>),
    // Run (S04)
    Run,
    RunIn(PathBuf),
    Progress(u64, f32, String),
    JobDone(u64, Result<JobResult, UiError>),
    Cancel,
    // Result (S05)
    OpenPath(PathBuf),
    ShowInFolder(PathBuf),
    ContinueWith,
    ContinueTool(Tool),
    StartOver,
    BackToOptions,
    GoToTool(Tool, PathBuf),
    CloseDialog,
    // Recent (S07)
    RecentRerun(usize),
    RecentRemove(usize),
    RecentClear,
    // Settings (S06)
    SetOutputMode(u8),
    ChooseOutputFolder,
    OutputFolderChosen(PathBuf),
    SetOpenResult(bool),
    RefreshDeps,
    DepsLoaded(Option<DepStatus>),
    // Misc
    Key(keyboard::Key, keyboard::Modifiers),
    CloseToast(ToastId),
    Noop,
}

pub struct App {
    pub core: Core,
    pub page: Page,
    /// Where Back goes from Recent / Settings.
    pub return_to: Page,
    pub about: About,
    pub show_about: bool,
    pub settings: Settings,
    pub recent: Vec<RecentJob>,
    pub deps: Option<DepStatus>,
    pub deps_loading: bool,
    // Home
    pub search: String,
    pub search_id: widget::Id,
    pub category: Option<Category>,
    pub dropped: Vec<PathBuf>,
    pub drag_hover: bool,
    // Workspace
    pub ws: Option<Workspace>,
    pub thumbs: HashMap<ThumbKey, Thumb>,
    pub thumb_queue: VecDeque<(ThumbKey, Option<String>)>,
    pub thumb_inflight: usize,
    pub window_width: f32,
    // Jobs
    pub next_job: u64,
    pub job: Option<JobState>,
    pub result: Option<(Tool, Result<JobResult, UiError>)>,
    pub dialog: Option<DialogKind>,
    pub toasts: Toasts<Message>,
}

impl App {
    pub fn toast(&mut self, text: impl Into<String>) -> Task<Message> {
        self.toasts.push(Toast::new(text)).map(cosmic::Action::App)
    }

    pub fn deps_has(&self, program: &str) -> bool {
        // Unknown status (engine not ready) never blocks a tool.
        self.deps.as_ref().is_none_or(|d| d.has(program))
    }

    pub fn missing_deps(&self, tool: Tool) -> Vec<&'static str> {
        tool.meta().needs.iter().copied().filter(|p| !self.deps_has(p)).collect()
    }

    fn set_page(&mut self, page: Page) -> Task<Message> {
        self.page = page;
        let title = match page {
            Page::Home => "Converty".to_string(),
            Page::Workspace => self.ws.as_ref().map_or("Converty".into(), |w| w.tool.meta().name.to_string()),
            Page::Processing => "Working…".into(),
            Page::Result => "Done".into(),
            Page::Recent => "Recent jobs".into(),
            Page::Settings => "Settings".into(),
        };
        self.set_header_title(title.clone());
        match self.core.main_window_id() {
            Some(id) => self.set_window_title(if page == Page::Home { title } else { format!("{title} – Converty") }, id),
            None => Task::none(),
        }
    }

    /// Opens a tool's workspace, optionally pre-loaded with files (F12, F13, F14).
    pub fn open_tool(&mut self, tool: Tool, files: Vec<PathBuf>, options: Option<ToolOptions>) -> Task<Message> {
        let mut ws = Workspace::new(tool);
        if let Some(o) = options {
            ws.options = o;
            ws.rebuild_segs();
        }
        self.ws = Some(ws);
        self.result = None;
        let mut tasks = vec![self.set_page(Page::Workspace)];
        tasks.push(self.add_files(files));
        Task::batch(tasks)
    }

    /// Adds files to the workspace: filters by accepted kinds, inspects each one.
    pub fn add_files(&mut self, paths: Vec<PathBuf>) -> Task<Message> {
        let Some(ws) = self.ws.as_mut() else { return Task::none() };
        let meta = ws.tool.meta();
        let accepted = ws.accept_files(paths);
        let mut tasks = Vec::new();
        for path in accepted.added {
            // A file with the same name may have changed on disk: forget old thumbnails.
            self.thumbs.retain(|(p, _), _| *p != path);
            tasks.push(inspect_task(path, None));
        }
        let ws = self.ws.as_mut().expect("workspace exists");
        if ws.output_stem.is_empty() || !ws.stem_edited {
            ws.output_stem = ws.default_stem();
        }
        let rejected = accepted.rejected;
        if rejected > 0 {
            let kinds: Vec<&str> = meta.accepts.iter().map(|k| engine::kind_globs(*k).0).collect();
            tasks.push(self.toast(format!(
                "Skipped {rejected} file{} – {} takes {}",
                if rejected == 1 { "" } else { "s" },
                meta.name,
                kinds.join(", ").to_lowercase()
            )));
        }
        if accepted.ignored > 0 {
            tasks.push(self.toast(format!("{} works on one file, so only the first one was added", meta.name)));
        }
        self.revalidate();
        Task::batch(tasks)
    }

    pub fn revalidate(&mut self) {
        if let Some(ws) = self.ws.as_mut() {
            ws.validate();
        }
    }

    /// Queues thumbnails and starts as many renders as allowed.
    pub fn request_thumbs(&mut self, path: &Path, pages: impl IntoIterator<Item = u32>, pw: Option<String>) -> Task<Message> {
        for page in pages {
            let key = (path.to_path_buf(), page);
            if self.thumbs.contains_key(&key) {
                continue;
            }
            self.thumbs.insert(key.clone(), Thumb::Loading);
            self.thumb_queue.push_back((key, pw.clone()));
        }
        self.pump_thumbs()
    }

    fn pump_thumbs(&mut self) -> Task<Message> {
        let mut tasks = Vec::new();
        while self.thumb_inflight < THUMB_PARALLEL {
            let Some(((path, page), pw)) = self.thumb_queue.pop_front() else { break };
            self.thumb_inflight += 1;
            tasks.push(cosmic::task::future(async move {
                let p = path.clone();
                let r = engine::blocking(move || converty_core::render_page(&p, page, THUMB_PX, pw.as_deref())).await;
                Message::ThumbLoaded((path, page), r.ok())
            }));
        }
        Task::batch(tasks)
    }

    fn file_filters(tool: Tool) -> Vec<FileFilter> {
        tool.meta()
            .accepts
            .iter()
            .map(|k| {
                let (name, globs) = engine::kind_globs(*k);
                globs.iter().fold(FileFilter::new(name), |f, g| crate::widgets::filter_glob(f, g))
            })
            .collect()
    }

    fn choose_files(&mut self, target: FilePickTarget) -> Task<Message> {
        let (title, filters, multi) = match target {
            FilePickTarget::Inputs => {
                let Some(ws) = &self.ws else { return Task::none() };
                ("Choose files", Self::file_filters(ws.tool), ws.tool.meta().multi)
            }
            FilePickTarget::WatermarkImage | FilePickTarget::SignatureImage => {
                let f = ["*.png", "*.jpg", "*.jpeg", "*.PNG", "*.JPG"]
                    .iter()
                    .fold(FileFilter::new("PNG or JPG images"), |f, g| crate::widgets::filter_glob(f, g));
                ("Choose an image", vec![f], false)
            }
        };
        cosmic::task::future(async move {
            let mut dialog = file_chooser::open::Dialog::new().title(title);
            for f in filters {
                dialog = dialog.filter(f);
            }
            let urls: Result<Vec<url::Url>, _> = if multi {
                dialog.open_files().await.map(|r| r.urls().to_vec())
            } else {
                dialog.open_file().await.map(|r| vec![r.url().clone()])
            };
            match urls {
                Ok(urls) => Message::FilesChosen(target, urls.iter().filter_map(|u| u.to_file_path().ok()).collect()),
                Err(file_chooser::Error::Cancelled) => Message::Noop,
                Err(e) => Message::PickerFailed(e.to_string()),
            }
        })
    }

    fn choose_folder(then_run: bool) -> Task<Message> {
        cosmic::task::future(async move {
            let dialog = file_chooser::open::Dialog::new().title(if then_run {
                "Where should the result go?"
            } else {
                "Choose the output folder"
            });
            match dialog.open_folder().await {
                Ok(r) => match r.url().to_file_path() {
                    Ok(p) if then_run => Message::RunIn(p),
                    Ok(p) => Message::OutputFolderChosen(p),
                    Err(()) => Message::PickerFailed("That folder is not on this computer".into()),
                },
                Err(file_chooser::Error::Cancelled) => Message::Noop,
                Err(e) => Message::PickerFailed(e.to_string()),
            }
        })
    }

    /// Closes a password prompt whose file has left the workspace (removed or replaced),
    /// then asks for the next locked file. Returns whether a prompt was closed.
    fn close_stale_password_dialog(&mut self) -> bool {
        let stale = match (&self.dialog, &self.ws) {
            (Some(DialogKind::Password(p)), Some(ws)) => !ws.files.iter().any(|f| f.path == p.path),
            (Some(DialogKind::Password(_)), None) => true,
            _ => false,
        };
        if stale {
            self.dialog = None;
            self.next_password_prompt();
        }
        stale
    }

    fn next_password_prompt(&mut self) {
        if self.dialog.is_some() {
            return;
        }
        let Some(ws) = &self.ws else { return };
        if let Some(f) = ws.files.iter().find(|f| f.needs_password()) {
            self.dialog = Some(DialogKind::Password(PasswordPrompt {
                path: f.path.clone(),
                input: String::new(),
                hidden: true,
                error: None,
                checking: false,
            }));
        }
    }

    /// Thumbnail requests after a file has been inspected.
    fn thumbs_for_file(&mut self, idx: usize) -> Task<Message> {
        let Some(ws) = &self.ws else { return Task::none() };
        let Some(f) = ws.files.get(idx) else { return Task::none() };
        let Some(info) = &f.info else { return Task::none() };
        if info.encrypted && f.password.is_none() {
            return Task::none();
        }
        let path = f.path.clone();
        let pw = f.password.clone();
        let pages = if ws.page_grid() { info.pages.unwrap_or(1).min(24) } else { 1 };
        self.request_thumbs(&path, 1..=pages, pw)
    }

    fn start_job(&mut self, output_dir: PathBuf) -> Task<Message> {
        let Some(ws) = &self.ws else { return Task::none() };
        if self.job.is_some() {
            // Never start a second job: the first one would keep running unobserved.
            return Task::none();
        }
        if !output_dir.is_dir() {
            return self.toast(format!("The folder {} does not exist", output_dir.display()));
        }
        let request = JobRequest {
            tool: ws.tool,
            inputs: ws.files.iter().map(|f| Input { path: f.path.clone(), password: f.password.clone() }).collect(),
            options: ws.options.clone(),
            output_dir,
            output_stem: ws.output_stem.trim().to_string(),
        };
        self.next_job += 1;
        let id = self.next_job;
        let cancel = Arc::new(AtomicBool::new(false));
        self.job = Some(JobState {
            id,
            tool: ws.tool,
            cancel: cancel.clone(),
            progress: 0.0,
            status: "Starting…".into(),
            request: request.clone(),
        });

        let (tx, rx) = cosmic::iced::futures::channel::mpsc::unbounded::<Message>();
        let work = cosmic::task::future(async move {
            let tx = std::sync::Mutex::new(tx);
            let result = engine::blocking(move || {
                let report = |f: f32, s: &str| {
                    if let Ok(tx) = tx.lock() {
                        let _ = tx.unbounded_send(Message::Progress(id, f, s.to_string()));
                    }
                };
                converty_core::run(&request, &report, &cancel)
            })
            .await;
            Message::JobDone(id, result)
        });
        Task::batch([cosmic::task::stream(rx), work, self.set_page(Page::Processing)])
    }

    fn output_dir_for(&self) -> Option<PathBuf> {
        let ws = self.ws.as_ref()?;
        match &self.settings.output_mode {
            OutputMode::SameAsInput => ws.files.first().and_then(|f| f.path.parent().map(Path::to_path_buf)),
            OutputMode::Folder(p) => Some(p.clone()),
            OutputMode::Ask => None,
        }
    }

    fn record_recent(&mut self, job: &JobState, result: &Result<JobResult, UiError>) {
        let (outputs, status, input_bytes, output_bytes) = match result {
            Ok(r) => (r.outputs.clone(), config::JobStatus::Done, r.input_bytes, r.output_bytes),
            Err(e) => (Vec::new(), config::JobStatus::Failed(e.message.clone()), 0, 0),
        };
        self.recent.insert(
            0,
            RecentJob {
                id: config::now_secs() * 1000 + job.id,
                tool: job.tool,
                inputs: job.request.inputs.iter().map(|i| i.path.clone()).collect(),
                outputs,
                finished_at: config::now_secs(),
                status,
                input_bytes,
                output_bytes,
                options: Some(job.request.options.clone()),
            },
        );
        self.recent.truncate(self.settings.recent_limit.max(1) as usize);
        config::save_recent(&self.recent);
    }

    fn handle_drop(&mut self, paths: Vec<PathBuf>) -> Task<Message> {
        self.drag_hover = false;
        let paths: Vec<PathBuf> = paths.into_iter().filter(|p| p.is_file()).collect();
        if paths.is_empty() {
            return Task::none();
        }
        match self.page {
            Page::Workspace => self.add_files(paths),
            Page::Processing => Task::none(),
            _ => {
                // F13: remember the files and show only tools that can take them.
                self.dropped = paths;
                self.category = None;
                self.search.clear();
                let unusable = self.dropped.iter().all(|p| FileKind::from_path(p) == FileKind::Other);
                let t = self.set_page(Page::Home);
                if unusable {
                    let n = self.toast("None of the tools can open these files");
                    self.dropped.clear();
                    return Task::batch([t, n]);
                }
                t
            }
        }
    }

    fn back(&mut self) -> Task<Message> {
        match self.page {
            Page::Home => {
                if !self.dropped.is_empty() {
                    self.dropped.clear();
                } else if !self.search.is_empty() {
                    self.search.clear();
                }
                Task::none()
            }
            Page::Workspace => self.set_page(Page::Home),
            Page::Processing => {
                if let Some(j) = &self.job {
                    j.cancel.store(true, Ordering::Relaxed);
                }
                Task::none()
            }
            Page::Result => self.set_page(if self.ws.is_some() { Page::Workspace } else { Page::Home }),
            Page::Recent | Page::Settings => {
                let to = self.return_to;
                self.set_page(to)
            }
        }
    }
}

fn inspect_task(path: PathBuf, pw: Option<String>) -> Task<Message> {
    cosmic::task::future(async move {
        let p = path.clone();
        let r = engine::blocking(move || converty_core::inspect(&p, pw.as_deref())).await;
        Message::Inspected(path, r)
    })
}

fn load_deps() -> Task<Message> {
    cosmic::task::future(async move { Message::DepsLoaded(engine::blocking_infallible(converty_core::deps).await) })
}

/// Parses a `text/uri-list` payload into local paths.
fn parse_uri_list(data: &[u8]) -> Vec<PathBuf> {
    String::from_utf8_lossy(data)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| url::Url::parse(l).ok())
        .filter_map(|u| u.to_file_path().ok())
        .collect()
}

impl Application for App {
    type Executor = cosmic::executor::Default;
    type Flags = ();
    type Message = Message;
    const APP_ID: &'static str = APP_ID;

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(core: Core, _flags: ()) -> (Self, Task<Message>) {
        let about = About::default()
            .name("Converty")
            .icon(widget::icon::from_name("x-office-document-symbolic").handle())
            .version(env!("CARGO_PKG_VERSION"))
            .comments("Everyday PDF tools that run on your own computer. No uploads, no accounts.")
            .license("GPL-3.0-or-later")
            .license_url("https://www.gnu.org/licenses/gpl-3.0.html")
            .links([("Source code", "https://github.com/DerKellerPotato/Converty")]);

        let mut app = App {
            core,
            page: Page::Home,
            return_to: Page::Home,
            about,
            show_about: false,
            settings: config::load_settings(),
            recent: config::load_recent(),
            deps: None,
            deps_loading: true,
            search: String::new(),
            search_id: widget::Id::new("tool-search"),
            category: None,
            dropped: Vec::new(),
            drag_hover: false,
            ws: None,
            thumbs: HashMap::new(),
            thumb_queue: VecDeque::new(),
            thumb_inflight: 0,
            window_width: 1100.0,
            next_job: 0,
            job: None,
            result: None,
            dialog: None,
            toasts: Toasts::new(Message::CloseToast),
        };
        let title = app.set_page(Page::Home);
        (app, Task::batch([title, load_deps()]))
    }

    fn header_start(&self) -> Vec<Element<'_, Message>> {
        if self.page == Page::Home || self.page == Page::Processing {
            return vec![];
        }
        vec![widget::tooltip(
            widget::button::icon(widget::icon::from_name("go-previous-symbolic")).on_press(Message::Back),
            widget::text::caption("Back (Esc)"),
            widget::tooltip::Position::Bottom,
        )
        .into()]
    }

    fn header_end(&self) -> Vec<Element<'_, Message>> {
        let btn = |icon: &'static str, tip: &'static str, msg: Message, active: bool| -> Element<'_, Message> {
            widget::tooltip(
                widget::button::icon(widget::icon::from_name(icon)).selected(active).on_press(msg),
                widget::text::caption(tip),
                widget::tooltip::Position::Bottom,
            )
            .into()
        };
        vec![
            btn("document-open-recent-symbolic", "Recent jobs", Message::OpenRecent, self.page == Page::Recent),
            btn("emblem-system-symbolic", "Settings (Ctrl+,)", Message::OpenSettings, self.page == Page::Settings),
            btn("help-about-symbolic", "About Converty", Message::ToggleAbout, self.show_about),
        ]
    }

    fn context_drawer(&self) -> Option<ContextDrawer<'_, Message>> {
        self.show_about
            .then(|| context_drawer::about(&self.about, |url| Message::OpenUrl(url.to_owned()), Message::ToggleAbout))
    }

    fn dialog(&self) -> Option<Element<'_, Message>> {
        self.view_dialog()
    }

    fn on_escape(&mut self) -> Task<Message> {
        self.update(Message::Escape)
    }

    fn on_search(&mut self) -> Task<Message> {
        self.update(Message::FocusSearch)
    }

    fn on_window_resize(&mut self, _id: window::Id, width: f32, _height: f32) {
        self.window_width = width;
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            cosmic::iced::keyboard::listen().filter_map(|e| match e {
                keyboard::Event::KeyPressed { key, modifiers, .. } if modifiers.control() => Some(Message::Key(key, modifiers)),
                _ => None,
            }),
            // X11 (and any backend that reports window file drops).
            event::listen_with(|e, _status, _id| match e {
                Event::Window(window::Event::FileDropped(paths)) => Some(Message::FilesDropped(paths)),
                _ => None,
            }),
        ])
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        let task = self.update_inner(message);
        if self.page == Page::Workspace {
            self.revalidate();
        }
        self.close_stale_password_dialog();
        task
    }

    fn view(&self) -> Element<'_, Message> {
        let content: Element<'_, Message> = match self.page {
            Page::Home => self.view_home(),
            Page::Workspace => self.view_workspace(),
            Page::Processing => self.view_progress(),
            Page::Result => self.view_result(),
            Page::Recent => self.view_recent(),
            Page::Settings => self.view_settings(),
        };
        let content = widget::toaster(&self.toasts, content);
        // Wayland drag-and-drop of files from any file manager.
        widget::dnd_destination(
            widget::container(content).width(Length::Fill).height(Length::Fill),
            vec!["text/uri-list".into()],
        )
        .on_enter(|_, _, _| Message::DndEnter)
        .on_leave(|| Message::DndLeave)
        .on_finish(|_mime, data, _action, _x, _y| Message::DndDrop(data))
        .into()
    }
}

impl App {
    fn update_inner(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Noop => {}
            Message::Back => return self.back(),
            Message::OpenRecent => {
                if self.page == Page::Processing {
                    return Task::none();
                }
                if !matches!(self.page, Page::Recent | Page::Settings) {
                    self.return_to = self.page;
                }
                return self.set_page(Page::Recent);
            }
            Message::OpenSettings => {
                if self.page == Page::Processing {
                    return Task::none();
                }
                if !matches!(self.page, Page::Recent | Page::Settings) {
                    self.return_to = self.page;
                }
                return Task::batch([self.set_page(Page::Settings), load_deps()]);
            }
            Message::ToggleAbout => {
                self.show_about = !self.show_about;
                self.set_show_context(self.show_about);
            }
            Message::OpenUrl(url) => {
                let _ = open::that_detached(url);
            }
            Message::Escape => {
                if self.dialog.is_some() {
                    return self.update_inner(match self.dialog {
                        Some(DialogKind::Password(_)) => Message::PwCancel,
                        _ => Message::CloseDialog,
                    });
                }
                if self.show_about {
                    self.show_about = false;
                    self.set_show_context(false);
                    return Task::none();
                }
                return self.back();
            }
            Message::FocusSearch => {
                if self.page == Page::Processing {
                    return Task::none();
                }
                let t = if self.page != Page::Home { self.set_page(Page::Home) } else { Task::none() };
                return Task::batch([t, widget::text_input::focus(self.search_id.clone())]);
            }
            Message::Key(key, _mods) => match key.as_ref() {
                Key::Character("o") if self.page == Page::Workspace => return self.choose_files(FilePickTarget::Inputs),
                Key::Named(Named::Enter) if self.page == Page::Workspace => return self.update_inner(Message::Run),
                Key::Character(",") => return self.update_inner(Message::OpenSettings),
                _ => {}
            },

            // Home
            Message::Search(s) => self.search = s,
            Message::Category(c) => self.category = c,
            Message::OpenTool(tool) => {
                let files: Vec<PathBuf> = std::mem::take(&mut self.dropped)
                    .into_iter()
                    .filter(|p| tool.meta().accepts.contains(&FileKind::from_path(p)))
                    .collect();
                return self.open_tool(tool, files, None);
            }
            Message::ClearDropped => self.dropped.clear(),

            // Drops
            Message::DndEnter => self.drag_hover = true,
            Message::DndLeave => self.drag_hover = false,
            Message::DndDrop(data) => return self.handle_drop(parse_uri_list(&data)),
            Message::FilesDropped(paths) => return self.handle_drop(paths),

            // Files
            Message::ChooseFiles(target) => return self.choose_files(target),
            Message::FilesChosen(target, paths) => match target {
                FilePickTarget::Inputs => return self.add_files(paths),
                FilePickTarget::WatermarkImage | FilePickTarget::SignatureImage => {
                    if let (Some(ws), Some(p)) = (self.ws.as_mut(), paths.into_iter().next()) {
                        ws.set_image(target, p);
                    }
                }
            },
            Message::PickerFailed(e) => return self.toast(format!("The file dialog could not open: {e}")),
            Message::Inspected(path, result) => {
                let Some(ws) = self.ws.as_mut() else { return Task::none() };
                let Some(idx) = ws.files.iter().position(|f| f.path == path) else { return Task::none() };
                match result {
                    Ok(info) => {
                        ws.files[idx].info = Some(info);
                        ws.files[idx].error = None;
                        ws.on_file_ready(idx);
                    }
                    Err(e) => ws.files[idx].error = Some(e),
                }
                self.next_password_prompt();
                return self.thumbs_for_file(idx);
            }
            Message::RemoveFile(i) => {
                if let Some(ws) = self.ws.as_mut() {
                    if i < ws.files.len() {
                        ws.files.remove(i);
                        if ws.files.is_empty() {
                            ws.reset_pages();
                        }
                        if !ws.stem_edited {
                            ws.output_stem = ws.default_stem();
                        }
                    }
                }
            }
            Message::MoveFile(i, d) => {
                if let Some(ws) = self.ws.as_mut() {
                    let j = i as isize + d;
                    if j >= 0 && (j as usize) < ws.files.len() {
                        ws.files.swap(i, j as usize);
                        if !ws.stem_edited {
                            ws.output_stem = ws.default_stem();
                        }
                    }
                }
            }
            Message::SortByName => {
                if let Some(ws) = self.ws.as_mut() {
                    ws.files.sort_by_key(|f| f.name().to_lowercase());
                    if !ws.stem_edited {
                        ws.output_stem = ws.default_stem();
                    }
                }
            }
            Message::RepairFile(path) => return self.open_tool(Tool::Repair, vec![path], None),

            // Thumbnails
            Message::ThumbLoaded(key, data) => {
                self.thumb_inflight = self.thumb_inflight.saturating_sub(1);
                let t = match data {
                    Some((w, h, px)) => Thumb::Ready(image::Handle::from_rgba(w, h, px)),
                    None => Thumb::Failed,
                };
                self.thumbs.insert(key, t);
                return self.pump_thumbs();
            }
            Message::ThumbScroll(viewport) => {
                let Some(ws) = &self.ws else { return Task::none() };
                if !ws.page_grid() {
                    return Task::none();
                }
                let f = &ws.files[0];
                let total = f.info.as_ref().and_then(|i| i.pages).unwrap_or(0);
                let grid_w = (self.window_width - 360.0).max(200.0);
                let cols = (grid_w / crate::pages::workspace::CELL_W).floor().max(1.0) as u32;
                let bottom = viewport.absolute_offset().y + viewport.bounds().height;
                let rows = (bottom / crate::pages::workspace::CELL_H).ceil() as u32 + 2;
                let upto = (rows * cols).min(total);
                let (path, pw) = (f.path.clone(), f.password.clone());
                return self.request_thumbs(&path, 1..=upto, pw);
            }

            // Page grid
            Message::TogglePage(p) => {
                if let Some(ws) = self.ws.as_mut() {
                    ws.toggle_page(p);
                }
            }
            Message::SelectAllPages(all) => {
                if let Some(ws) = self.ws.as_mut() {
                    ws.select_all(all);
                }
            }
            Message::OrgSelect(i) => {
                if let Some(ws) = self.ws.as_mut() {
                    ws.org_selected = if ws.org_selected == Some(i) { None } else { Some(i) };
                }
            }
            Message::OrgMove(i, d) => {
                if let Some(ws) = self.ws.as_mut() {
                    if let ToolOptions::Organize { pages } = &mut ws.options {
                        let j = i as isize + d;
                        if j >= 0 && (j as usize) < pages.len() {
                            pages.swap(i, j as usize);
                            ws.org_selected = Some(j as usize);
                        }
                    }
                }
            }
            Message::OrgRotate(i, cw) => {
                if let Some(ws) = self.ws.as_mut() {
                    if let ToolOptions::Organize { pages } = &mut ws.options {
                        if let Some(PageOp::Page { rotate, .. }) = pages.get_mut(i) {
                            *rotate = if cw { rotate.cw() } else { rotate.ccw() };
                        }
                    }
                }
            }
            Message::OrgDelete(i) => {
                if let Some(ws) = self.ws.as_mut() {
                    if let ToolOptions::Organize { pages } = &mut ws.options {
                        if i < pages.len() {
                            pages.remove(i);
                            ws.org_selected = if pages.is_empty() { None } else { Some(i.min(pages.len() - 1)) };
                        }
                    }
                }
            }
            Message::OrgDuplicate(i) => {
                if let Some(ws) = self.ws.as_mut() {
                    if let ToolOptions::Organize { pages } = &mut ws.options {
                        if let Some(op) = pages.get(i).copied() {
                            pages.insert(i + 1, op);
                            ws.org_selected = Some(i + 1);
                        }
                    }
                }
            }
            Message::OrgInsertBlank(i) => {
                if let Some(ws) = self.ws.as_mut() {
                    if let ToolOptions::Organize { pages } = &mut ws.options {
                        let at = (i + 1).min(pages.len());
                        pages.insert(at, PageOp::Blank);
                        ws.org_selected = Some(at);
                    }
                }
            }
            Message::OrgReset => {
                if let Some(ws) = self.ws.as_mut() {
                    ws.reset_pages();
                    ws.on_file_ready(0);
                }
            }

            // Options
            Message::Seg(key, entity) => {
                if let Some(ws) = self.ws.as_mut() {
                    ws.on_seg(key, entity);
                }
            }
            Message::Opt(m) => {
                if let OptMsg::ChooseImage(target) = m {
                    return self.choose_files(target);
                }
                if let Some(ws) = self.ws.as_mut() {
                    ws.on_opt(m);
                }
            }

            // Password dialog
            Message::PwInput(s) => {
                if let Some(DialogKind::Password(p)) = self.dialog.as_mut() {
                    p.input = s;
                    p.error = None;
                }
            }
            Message::PwToggleHidden => {
                if let Some(DialogKind::Password(p)) = self.dialog.as_mut() {
                    p.hidden = !p.hidden;
                }
            }
            Message::PwSubmit => {
                if let Some(DialogKind::Password(p)) = self.dialog.as_mut() {
                    if p.input.is_empty() || p.checking {
                        return Task::none();
                    }
                    p.checking = true;
                    let (path, pw) = (p.path.clone(), p.input.clone());
                    return cosmic::task::future(async move {
                        let (pp, pw2) = (path.clone(), pw.clone());
                        let r = engine::blocking(move || converty_core::inspect(&pp, Some(&pw2))).await;
                        Message::PwChecked(path, pw, r)
                    });
                }
            }
            Message::PwChecked(path, pw, result) => {
                match result {
                    Ok(info) => {
                        self.dialog = None;
                        if let Some(ws) = self.ws.as_mut() {
                            if let Some(idx) = ws.files.iter().position(|f| f.path == path) {
                                ws.files[idx].info = Some(info);
                                ws.files[idx].password = Some(pw);
                                ws.on_file_ready(idx);
                                self.next_password_prompt();
                                return self.thumbs_for_file(idx);
                            }
                        }
                    }
                    Err(e) => {
                        if let Some(DialogKind::Password(p)) = self.dialog.as_mut() {
                            p.checking = false;
                            p.error = Some(if matches!(e.kind, ErrKind::WrongPassword(_)) {
                                "That password did not work. Try again.".into()
                            } else {
                                e.message
                            });
                        }
                    }
                }
            }
            Message::PwCancel => {
                if let Some(DialogKind::Password(p)) = self.dialog.take() {
                    if let Some(ws) = self.ws.as_mut() {
                        ws.files.retain(|f| f.path != p.path);
                        if !ws.stem_edited {
                            ws.output_stem = ws.default_stem();
                        }
                    }
                    let t = self.toast("Locked file removed. Add it again when you know the password.");
                    self.next_password_prompt();
                    return t;
                }
            }

            // Run
            Message::Run => {
                let Some(ws) = &self.ws else { return Task::none() };
                if self.page != Page::Workspace {
                    return Task::none();
                }
                if let Some(reason) = &ws.blocker {
                    return self.toast(reason.clone());
                }
                let missing = self.missing_deps(ws.tool);
                if !missing.is_empty() {
                    let packages: Vec<&str> = self
                        .deps
                        .iter()
                        .flat_map(|d| d.deps.iter())
                        .filter(|d| missing.contains(&d.program))
                        .map(|d| d.package)
                        .collect();
                    self.dialog = Some(DialogKind::Error {
                        title: format!("{} needs {}", ws.tool.meta().name, missing.join(", ")),
                        body: format!(
                            "Install the package {} with your software manager or a terminal (e.g. sudo apt install {}), then press “Check again” in Settings.",
                            packages.join(", "),
                            packages.join(" ")
                        ),
                    });
                    return Task::none();
                }
                return match self.output_dir_for() {
                    Some(dir) => self.start_job(dir),
                    None => Self::choose_folder(true),
                };
            }
            Message::RunIn(dir) => {
                // The folder dialog is asynchronous: the workspace may have changed meanwhile.
                if self.page != Page::Workspace || self.ws.as_ref().is_none_or(|w| w.blocker.is_some()) {
                    return Task::none();
                }
                return self.start_job(dir);
            }
            Message::Progress(id, f, s) => {
                if let Some(j) = self.job.as_mut().filter(|j| j.id == id) {
                    j.progress = f.clamp(0.0, 1.0);
                    j.status = s;
                }
            }
            Message::JobDone(id, result) => {
                let Some(job) = self.job.take_if(|j| j.id == id) else { return Task::none() };
                if matches!(&result, Err(e) if e.kind == ErrKind::Cancelled) {
                    let t = self.set_page(Page::Workspace);
                    return Task::batch([t, self.toast("Cancelled. Nothing was written.")]);
                }
                self.record_recent(&job, &result);
                if let (Ok(r), true) = (&result, self.settings.open_result) {
                    if let Some(first) = r.outputs.first() {
                        let _ = open::that_detached(first);
                    }
                }
                self.result = Some((job.tool, result));
                return self.set_page(Page::Result);
            }
            Message::Cancel => {
                if let Some(j) = &self.job {
                    j.cancel.store(true, Ordering::Relaxed);
                    if let Some(j) = self.job.as_mut() {
                        j.status = "Cancelling…".into();
                    }
                }
            }

            // Result
            Message::OpenPath(p) => {
                if !p.exists() {
                    return self.toast("That file is gone. It may have been moved or deleted.");
                }
                if let Err(e) = open::that_detached(&p) {
                    return self.toast(format!("Could not open it: {e}"));
                }
            }
            Message::ShowInFolder(p) => {
                if !p.exists() {
                    return self.toast("That file is gone. It may have been moved or deleted.");
                }
                return cosmic::task::future(async move {
                    engine::show_in_folder(p).await;
                    Message::Noop
                });
            }
            Message::ContinueWith => self.dialog = Some(DialogKind::ContinueWith),
            Message::ContinueTool(tool) => {
                self.dialog = None;
                let outputs = match &self.result {
                    Some((_, Ok(r))) => r.outputs.clone(),
                    _ => Vec::new(),
                };
                let n = outputs.len();
                let files: Vec<PathBuf> = if tool.meta().multi { outputs } else { outputs.into_iter().take(1).collect() };
                let t = self.open_tool(tool, files, None);
                if !tool.meta().multi && n > 1 {
                    return Task::batch([t, self.toast(format!("{} works on one file, so the first result was loaded", tool.meta().name))]);
                }
                return t;
            }
            Message::StartOver => {
                self.ws = None;
                self.result = None;
                self.dropped.clear();
                return self.set_page(Page::Home);
            }
            Message::BackToOptions => return self.set_page(if self.ws.is_some() { Page::Workspace } else { Page::Home }),
            Message::GoToTool(tool, path) => return self.open_tool(tool, vec![path], None),
            Message::CloseDialog => self.dialog = None,

            // Recent
            Message::RecentRerun(i) => {
                if let Some(job) = self.recent.get(i).cloned() {
                    let existing: Vec<PathBuf> = job.inputs.iter().filter(|p| p.exists()).cloned().collect();
                    let missing = job.inputs.len() - existing.len();
                    let t = self.open_tool(job.tool, existing, job.options.clone());
                    if missing > 0 {
                        return Task::batch([t, self.toast(format!("{missing} input file(s) could not be found"))]);
                    }
                    return t;
                }
            }
            Message::RecentRemove(i) => {
                if i < self.recent.len() {
                    self.recent.remove(i);
                    config::save_recent(&self.recent);
                }
            }
            Message::RecentClear => {
                self.recent.clear();
                config::save_recent(&self.recent);
            }

            // Settings
            Message::SetOutputMode(m) => {
                match m {
                    0 => self.settings.output_mode = OutputMode::SameAsInput,
                    1 => {
                        if !matches!(self.settings.output_mode, OutputMode::Folder(_)) {
                            return Self::choose_folder(false);
                        }
                    }
                    _ => self.settings.output_mode = OutputMode::Ask,
                }
                config::save_settings(&self.settings);
            }
            Message::ChooseOutputFolder => return Self::choose_folder(false),
            Message::OutputFolderChosen(p) => {
                self.settings.output_mode = OutputMode::Folder(p);
                config::save_settings(&self.settings);
            }
            Message::SetOpenResult(b) => {
                self.settings.open_result = b;
                config::save_settings(&self.settings);
            }
            Message::RefreshDeps => {
                self.deps_loading = true;
                return load_deps();
            }
            Message::DepsLoaded(d) => {
                self.deps_loading = false;
                if d.is_some() {
                    self.deps = d;
                }
            }
            Message::CloseToast(id) => self.toasts.remove(id),
        }
        Task::none()
    }
}
