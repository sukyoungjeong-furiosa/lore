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
        self.read(&self.file(backend), &connection_id(config))
    }

    /// Reads a service credential only for its recorded API endpoint.
    pub fn get_connection(
        &self,
        namespace: &str,
        name: &str,
        endpoint: &str,
    ) -> Result<Option<String>> {
        self.read(
            &self.connection_file(namespace, name),
            &hash(format!("{}:{namespace}:{endpoint}", namespace.len())),
        )
    }

    fn connection_file(&self, namespace: &str, name: &str) -> PathBuf {
        self.directory.join(format!(
            "{}.json",
            hash(format!("connection:{}:{namespace}:{name}", namespace.len()))
        ))
    }

    fn read(&self, file: &Path, connection: &str) -> Result<Option<String>> {
        let bytes = match std::fs::read(file) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if std::fs::metadata(file)?.permissions().mode() & 0o077 != 0 {
                return Err(Error::new(
                    ErrorKind::PermissionDenied,
                    "Lore credential files must be accessible only to their owner",
                ));
            }
        }
        let credential: Credential = serde_json::from_slice(&bytes)
            .map_err(|_| Error::new(ErrorKind::Corrupt, "Invalid Lore credential file"))?;
        if credential.connection != connection {
            return Err(Error::new(
                ErrorKind::PermissionDenied,
                "Stored credential belongs to a different API endpoint; set it again",
            ));
        }
        Ok(Some(credential.token))
    }

    /// Saves a connection token outside repository, cache, and workspace state.
    pub fn set(&self, backend: &str, config: &BackendConfig, token: &str) -> Result<()> {
        self.write(&self.file(backend), connection_id(config), token)
    }

    pub fn set_connection(
        &self,
        namespace: &str,
        name: &str,
        endpoint: &str,
        token: &str,
    ) -> Result<()> {
        self.write(
            &self.connection_file(namespace, name),
            hash(format!("{}:{namespace}:{endpoint}", namespace.len())),
            token,
        )
    }

    fn write(&self, file: &Path, connection: String, token: &str) -> Result<()> {
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
            connection,
            token: token.into(),
        };
        atomic_write(file, &serde_json::to_vec(&credential)?)
    }

    pub fn remove(&self, backend: &str) -> Result<()> {
        self.remove_file(&self.file(backend))
    }

    pub fn remove_connection(&self, namespace: &str, name: &str) -> Result<()> {
        self.remove_file(&self.connection_file(namespace, name))
    }

    fn remove_file(&self, file: &Path) -> Result<()> {
        match std::fs::remove_file(file) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}
