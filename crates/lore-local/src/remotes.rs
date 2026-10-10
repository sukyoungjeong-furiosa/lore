use crate::atomic_write;
use lore_core::{Error, ErrorKind, MappingStoreId, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

/// Connects to one store; repository and backend registrations belong to that store.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoreRemote {
    pub server_url: String,
    pub store: MappingStoreId,
}

impl LoreRemote {
    pub fn validate(&self) -> Result<()> {
        let url = reqwest::Url::parse(&self.server_url)
            .map_err(|_| Error::invalid("Invalid Lore server URL"))?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(Error::invalid(
                "Lore remotes require an HTTP(S) server URL without embedded credentials",
            ));
        }
        Ok(())
    }
}

pub struct RemoteRegistry {
    directory: PathBuf,
}

fn validate_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 100
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(Error::invalid(
            "Lore remote names require ASCII letters, digits, hyphens, or underscores",
        ));
    }
    Ok(())
}

impl RemoteRegistry {
    pub fn new(home: &Path) -> Self {
        Self {
            directory: home.join("remotes"),
        }
    }

    pub fn list(&self) -> Result<BTreeMap<String, LoreRemote>> {
        let remotes: BTreeMap<String, LoreRemote> =
            match std::fs::read(self.directory.join("config.json")) {
                Ok(bytes) => serde_json::from_slice(&bytes)?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    return Ok(BTreeMap::new());
                }
                Err(error) => return Err(error.into()),
            };
        for (name, remote) in &remotes {
            validate_name(name)?;
            remote.validate()?;
        }
        Ok(remotes)
    }

    pub fn get(&self, name: &str) -> Result<LoreRemote> {
        self.list()?
            .remove(name)
            .ok_or_else(|| Error::new(ErrorKind::NotFound, "Lore remote is not configured"))
    }

    fn update<T>(
        &self,
        change: impl FnOnce(&mut BTreeMap<String, LoreRemote>) -> Result<T>,
    ) -> Result<T> {
        std::fs::create_dir_all(&self.directory)?;
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.directory.join("config.lock"))?;
        lock.lock()?;
        let mut remotes = self.list()?;
        let result = change(&mut remotes)?;
        for (name, remote) in &remotes {
            validate_name(name)?;
            remote.validate()?;
        }
        atomic_write(
            &self.directory.join("config.json"),
            &serde_json::to_vec_pretty(&remotes)?,
        )?;
        Ok(result)
    }

    /// Records connection settings without contacting a server or activating workspace state.
    pub fn add(&self, name: &str, remote: LoreRemote) -> Result<LoreRemote> {
        self.update(|remotes| {
            if remotes.contains_key(name) {
                return Err(Error::new(
                    ErrorKind::Conflict,
                    "Lore remote already exists; remove it explicitly before changing its store",
                ));
            }
            remotes.insert(name.into(), remote.clone());
            Ok(remote)
        })
    }

    /// Changes an endpoint while preserving the selected store identity.
    pub fn set_url(&self, name: &str, server_url: String) -> Result<LoreRemote> {
        self.update(|remotes| {
            let remote = remotes
                .get_mut(name)
                .ok_or_else(|| Error::new(ErrorKind::NotFound, "Lore remote is not configured"))?;
            remote.server_url = server_url;
            Ok(remote.clone())
        })
    }

    pub fn remove(&self, name: &str) -> Result<()> {
        self.update(|remotes| {
            if remotes.remove(name).is_none() {
                return Err(Error::new(
                    ErrorKind::NotFound,
                    "Lore remote is not configured",
                ));
            }
            Ok(())
        })
    }
}
