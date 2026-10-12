//! Converty engine: every PDF tool runs locally, no network.
//!
//! The UI only talks to this crate through the items re-exported here:
//! [`tools()`], [`inspect`], [`render_page`], [`parse_ranges`], [`deps`] and [`run`].
//! Everything is blocking; the UI calls it from a worker thread / `spawn_blocking`.

pub mod deps;
pub mod error;
pub mod inspect;
pub mod options;
pub mod ranges;
pub mod render;
pub mod tools;

mod ops;

pub use deps::{deps, Dep, DepStatus};
pub use error::{Error, Result};
pub use inspect::{inspect, FileInfo, FileKind};
pub use options::*;
pub use ranges::parse_ranges;
pub use render::render_page;
pub use tools::{tools, Category, Tool, ToolMeta};

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

/// One input file of a job.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Input {
    pub path: PathBuf,
    /// Password for encrypted PDFs. Never persisted.
    #[serde(skip)]
    pub password: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct JobRequest {
    pub tool: Tool,
    pub inputs: Vec<Input>,
    pub options: ToolOptions,
    /// Directory the results are written to. Must exist.
    pub output_dir: PathBuf,
    /// Stem for output files, e.g. "report_merged". Engine appends
    /// "_1", "_2"... for multi-output tools and never overwrites an existing file
    /// (it appends " (2)", " (3)"... instead).
    pub output_stem: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct JobResult {
    pub outputs: Vec<PathBuf>,
    /// Sum of input sizes in bytes.
    pub input_bytes: u64,
    /// Sum of output sizes in bytes.
    pub output_bytes: u64,
    /// Human-readable notes, e.g. "Result was larger than the original, kept original".
    pub notes: Vec<String>,
}

/// Progress callback: fraction 0.0..=1.0 and a short status text.
pub type Progress<'a> = &'a (dyn Fn(f32, &str) + Sync + 'a);

/// Runs a job. Checks `cancel` between steps (and kills child processes when set),
/// returning [`Error::Cancelled`]. Never leaves half-written outputs behind.
pub fn run(req: &JobRequest, progress: Progress<'_>, cancel: &AtomicBool) -> Result<JobResult> {
    ops::run(req, progress, cancel)
}
