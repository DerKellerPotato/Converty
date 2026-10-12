use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0} is password protected")]
    Encrypted(PathBuf),
    #[error("Wrong password for {0}")]
    WrongPassword(PathBuf),
    #[error("{0} is damaged and could not be read")]
    Corrupt(PathBuf),
    #[error("{0} is not a supported file type for this tool")]
    WrongType(PathBuf),
    #[error("This tool needs “{program}”. Install the package “{package}”.")]
    MissingDependency { program: &'static str, package: &'static str },
    #[error("Invalid page selection: {0}")]
    InvalidRange(String),
    #[error("Invalid options: {0}")]
    InvalidOptions(String),
    #[error("Cancelled")]
    Cancelled,
    #[error("{program} failed: {message}")]
    External { program: &'static str, message: String },
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("PDF error: {0}")]
    Pdf(String),
    #[error("Image error: {0}")]
    Image(String),
}

pub type Result<T> = std::result::Result<T, Error>;
