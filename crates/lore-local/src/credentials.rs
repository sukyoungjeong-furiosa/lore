use crate::{BackendConfig, atomic_write};
use lore_core::{Error, ErrorKind, Result, hash};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub struct CredentialStore {
    directory: PathBuf,
}

#[derive(Serialize, Deserialize)]
struct Credential {
    connection: String,
    token: String,
}

fn connection_id(config: &BackendConfig) -> String {
    match config {
        BackendConfig::Notion { api_url, .. } => hash(format!("notion:{api_url}")),
    }
}

impl CredentialStore {
    pub fn new(home: &Path) -> Self {
        Self {
            directory: home.join("credentials"),
        }
    }

    fn file(&self, backend: &str) -> PathBuf {
        self.directory.join(format!("{}.json", hash(backend)))
    }

    /// Reads a token only for the backend API endpoint under which it was saved.
    pub fn get(&self, backend: &str, config: &BackendConfig) -> Result<Option<String>> {
        let file = self.file(backend);
        let bytes = match std::fs::read(&file) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if std::fs::metadata(&file)?.permissions().mode() & 0o077 != 0 {
                return Err(Error::new(
                    ErrorKind::PermissionDenied,
                    "Lore credential files must be accessible only to their owner",
                ));
            }
        }
        let credential: Credential = serde_json::from_slice(&bytes)
            .map_err(|_| Error::new(ErrorKind::Corrupt, "Invalid Lore credential file"))?;
        if credential.connection != connection_id(config) {
            return Err(Error::new(
                ErrorKind::PermissionDenied,
                "Stored credential belongs to a different backend API endpoint; set it again",
            ));
        }
        Ok(Some(credential.token))
    }

    /// Saves a connection token outside repository, cache, and workspace state.
    pub fn set(&self, backend: &str, config: &BackendConfig, token: &str) -> Result<()> {
        let token = token.trim();
        if token.is_empty() || token.len() > 16 * 1024 || token.chars().any(char::is_control) {
            return Err(Error::invalid(
                "Connection token must be nonempty text without control characters, at most 16 KiB",
            ));
        }
        std::fs::create_dir_all(&self.directory)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&self.directory, std::fs::Permissions::from_mode(0o700))?;
        }
        let credential = Credential {
            connection: connection_id(config),
            token: token.into(),
        };
        atomic_write(&self.file(backend), &serde_json::to_vec(&credential)?)
    }

    pub fn remove(&self, backend: &str) -> Result<()> {
        match std::fs::remove_file(self.file(backend)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}
