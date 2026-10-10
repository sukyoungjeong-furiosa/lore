mod backends;
mod catalog;
mod credentials;
mod git;
mod mappings;
mod resolver_settings;
mod routing;
mod store;
mod workspace;

pub use backends::*;
pub use catalog::*;
pub use credentials::CredentialStore;
pub use git::GitRepository;
pub use mappings::*;
pub use resolver_settings::*;
pub use routing::*;
pub use store::FileRepository;
pub use workspace::*;

use lore_core::{Error, ErrorKind, Result};
use std::path::{Path, PathBuf};

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub fn home() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("LORE_HOME") {
        return absolute(&PathBuf::from(path));
    }
    if let Some(path) = std::env::var_os("XDG_DATA_HOME") {
        return absolute(&PathBuf::from(path).join("lore"));
    }
    let base = std::env::var_os("HOME")
        .ok_or_else(|| Error::new(ErrorKind::Unavailable, "Set LORE_HOME or HOME"))?;
    absolute(&PathBuf::from(base).join(".local/share/lore"))
}

pub(crate) fn absolute(path: &Path) -> Result<PathBuf> {
    let path = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            std::path::Component::CurDir => {}
            other => normalized.push(other.as_os_str()),
        }
    }
    let mut parent = normalized.as_path();
    let mut suffix = vec![];
    while !parent.exists() {
        suffix.push(
            parent
                .file_name()
                .ok_or_else(|| Error::invalid("Invalid path"))?
                .to_owned(),
        );
        parent = parent
            .parent()
            .ok_or_else(|| Error::invalid("Invalid path"))?;
    }
    let mut resolved = parent.canonicalize()?;
    for item in suffix.into_iter().rev() {
        resolved.push(item);
    }
    Ok(resolved)
}

pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let parent = path
        .parent()
        .ok_or_else(|| Error::invalid("Missing parent directory"))?;
    std::fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(bytes)?;
    file.as_file().sync_all()?;
    file.persist(path).map_err(|e| Error::from(e.error))?;
    Ok(())
}
