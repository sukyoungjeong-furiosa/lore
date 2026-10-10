mod backend;
mod catalog;
mod mapping;
mod model;
mod projection;
mod repository;
mod routing;
mod sharing;
mod synchronization;
mod wiki;

pub use backend::*;
pub use catalog::*;
pub use mapping::*;
pub use model::*;
pub use projection::*;
pub use repository::*;
pub use routing::*;
pub use sharing::*;
pub use synchronization::*;
pub use wiki::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    InvalidInput,
    NotFound,
    Conflict,
    NotInitialized,
    Unsupported,
    Unavailable,
    Corrupt,
    PermissionDenied,
    NotCached,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Error {
    pub kind: ErrorKind,
    pub message: String,
}

impl Error {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::InvalidInput, message)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::new(ErrorKind::Unavailable, error.to_string())
    }
}

impl From<serde_json::Error> for Error {
    fn from(error: serde_json::Error) -> Self {
        Self::new(ErrorKind::Corrupt, error.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;
