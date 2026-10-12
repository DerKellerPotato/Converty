//! Settings and recent jobs, stored as JSON in the XDG config/data dirs.
//! Passwords are never written: `Input::password` and the protect passwords are `#[serde(skip)]`.

use converty_core::{Tool, ToolOptions};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum OutputMode {
    #[default]
    SameAsInput,
    Folder(PathBuf),
    Ask,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub output_mode: OutputMode,
    pub open_result: bool,
    pub recent_limit: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self { output_mode: OutputMode::SameAsInput, open_result: false, recent_limit: 30 }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobStatus {
    Done,
    Failed(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentJob {
    pub id: u64,
    pub tool: Tool,
    pub inputs: Vec<PathBuf>,
    pub outputs: Vec<PathBuf>,
    /// Unix seconds, UTC.
    pub finished_at: u64,
    pub status: JobStatus,
    pub input_bytes: u64,
    pub output_bytes: u64,
    /// Options used, so "Run again" can restore them. Passwords are skipped by serde.
    #[serde(default)]
    pub options: Option<ToolOptions>,
}

fn settings_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("converty").join("settings.json"))
}

fn recent_path() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("converty").join("recent.json"))
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = serde_json::to_string_pretty(value).map_err(std::io::Error::other)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(tmp, path)
}

/// Moves an unreadable file aside (`x.json.corrupt`) so the next save does not destroy it.
fn backup_corrupt(path: &Path) {
    let to = path.with_extension("json.corrupt");
    match std::fs::rename(path, &to) {
        Ok(()) => eprintln!("converty: {} was unreadable, kept as {}", path.display(), to.display()),
        Err(e) => eprintln!("converty: {} was unreadable: {e}", path.display()),
    }
}

pub fn load_settings() -> Settings {
    let Some(path) = settings_path() else { return Settings::default() };
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|_| {
            backup_corrupt(&path);
            Settings::default()
        }),
        Err(_) => Settings::default(),
    }
}

pub fn save_settings(settings: &Settings) {
    if let Some(p) = settings_path() {
        if let Err(e) = write_json(&p, settings) {
            eprintln!("converty: could not save settings: {e}");
        }
    }
}

/// Parses recent.json entry by entry, so one entry that no longer fits (e.g. a tool
/// removed in a newer version) does not wipe the whole history. `None` = not a JSON list.
pub fn parse_recent(text: &str) -> Option<Vec<RecentJob>> {
    let values: Vec<serde_json::Value> = serde_json::from_str(text).ok()?;
    Some(values.into_iter().filter_map(|v| serde_json::from_value(v).ok()).collect())
}

pub fn load_recent() -> Vec<RecentJob> {
    let Some(path) = recent_path() else { return Vec::new() };
    match std::fs::read_to_string(&path) {
        Ok(text) => parse_recent(&text).unwrap_or_else(|| {
            backup_corrupt(&path);
            Vec::new()
        }),
        Err(_) => Vec::new(),
    }
}

pub fn save_recent(jobs: &[RecentJob]) {
    if let Some(p) = recent_path() {
        if let Err(e) = write_json(&p, &jobs) {
            eprintln!("converty: could not save recent jobs: {e}");
        }
    }
}

pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// "just now", "5 min ago", "3 h ago", "2 days ago".
pub fn relative_time(then: u64) -> String {
    let diff = now_secs().saturating_sub(then);
    match diff {
        0..=59 => "just now".into(),
        60..=3599 => format!("{} min ago", diff / 60),
        3600..=86_399 => format!("{} h ago", diff / 3600),
        86_400..=172_799 => "yesterday".into(),
        _ => format!("{} days ago", diff / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job_json(tool: &str) -> String {
        format!(
            r#"{{"id":1,"tool":"{tool}","inputs":["/a.pdf"],"outputs":["/b.pdf"],"finished_at":5,"status":"Done","input_bytes":1,"output_bytes":2}}"#
        )
    }

    #[test]
    fn recent_survives_one_bad_entry() {
        let text = format!("[{}, {}, {{\"junk\":true}}]", job_json("Merge"), job_json("NoSuchTool"));
        let jobs = parse_recent(&text).unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].tool, Tool::Merge);
        assert!(jobs[0].options.is_none());
    }

    #[test]
    fn recent_garbage_is_reported() {
        assert!(parse_recent("{ not json").is_none());
        assert!(parse_recent("").is_none());
        assert!(parse_recent("{}").is_none());
        assert_eq!(parse_recent("[]").unwrap().len(), 0);
    }

    #[test]
    fn settings_tolerate_missing_fields() {
        let s: Settings = serde_json::from_str(r#"{"open_result":true}"#).unwrap();
        assert!(s.open_result);
        assert_eq!(s.recent_limit, 30);
        assert_eq!(s.output_mode, OutputMode::SameAsInput);
    }

    #[test]
    fn relative_times() {
        assert_eq!(relative_time(now_secs()), "just now");
        assert_eq!(relative_time(now_secs() - 300), "5 min ago");
        assert_eq!(relative_time(now_secs() + 100), "just now");
    }
}
