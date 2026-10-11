use crate::atomic_write;
use lore_core::{Error, ErrorKind, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

/// Model authentication is independent of document backend and mapping-store credentials.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelConnection {
    pub base_url: String,
    pub token_env: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolverProfile {
    pub connection: String,
    pub model: String,
    pub system_prompt: Option<String>,
    pub timeout_seconds: Option<u64>,
    pub max_input_bytes: Option<usize>,
    pub max_output_tokens: Option<usize>,
    pub request_options: Option<serde_json::Map<String, serde_json::Value>>,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolverSettings {
    pub connections: BTreeMap<String, ModelConnection>,
    pub profiles: BTreeMap<String, ResolverProfile>,
    pub default_profile: Option<String>,
}

pub struct ResolverRegistry {
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
            "Resolver names require ASCII letters, digits, hyphens, or underscores",
        ));
    }
    Ok(())
}

impl ModelConnection {
    pub fn validate(&self) -> Result<()> {
        let url = reqwest::Url::parse(&self.base_url)
            .map_err(|_| Error::invalid("Invalid model API URL"))?;
        if !matches!(url.scheme(), "https" | "http")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || self.token_env.is_empty()
            || !self
                .token_env
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            return Err(Error::invalid(
                "Invalid model connection; use an HTTP(S) endpoint without embedded credentials",
            ));
        }
        Ok(())
    }
}

impl ResolverProfile {
    fn validate(&self) -> Result<()> {
        validate_name(&self.connection)?;
        if self.model.trim().is_empty()
            || self.model.chars().any(char::is_control)
            || self
                .system_prompt
                .as_ref()
                .is_some_and(|s| s.trim().is_empty())
            || self.timeout_seconds == Some(0)
            || self.max_input_bytes == Some(0)
            || self.max_output_tokens == Some(0)
        {
            return Err(Error::invalid(
                "Resolver profiles require a model and positive request limits",
            ));
        }
        Ok(())
    }
}

impl ResolverSettings {
    fn validate(&self) -> Result<()> {
        for (name, connection) in &self.connections {
            validate_name(name)?;
            connection.validate()?;
        }
        for (name, profile) in &self.profiles {
            validate_name(name)?;
            profile.validate()?;
            if !self.connections.contains_key(&profile.connection) {
                return Err(Error::invalid(
                    "Resolver profile references an unconfigured connection",
                ));
            }
        }
        if self
            .default_profile
            .as_ref()
            .is_some_and(|name| !self.profiles.contains_key(name))
        {
            return Err(Error::invalid("Default resolver profile is not configured"));
        }
        Ok(())
    }

    pub fn connection(&self, name: &str) -> Result<&ModelConnection> {
        self.connections
            .get(name)
            .ok_or_else(|| Error::new(ErrorKind::NotFound, "Model connection is not configured"))
    }

    pub fn profile(&self, name: &str) -> Result<&ResolverProfile> {
        self.profiles
            .get(name)
            .ok_or_else(|| Error::new(ErrorKind::NotFound, "Resolver profile is not configured"))
    }
}

impl ResolverRegistry {
    pub fn new(home: &Path) -> Self {
        Self {
            directory: home.join("resolvers"),
        }
    }

    pub fn read(&self) -> Result<ResolverSettings> {
        let settings: ResolverSettings = match std::fs::read(self.directory.join("config.json")) {
            Ok(bytes) => serde_json::from_slice(&bytes)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(ResolverSettings::default());
            }
            Err(e) => return Err(e.into()),
        };
        settings.validate()?;
        Ok(settings)
    }

    fn update(
        &self,
        change: impl FnOnce(&mut ResolverSettings) -> Result<()>,
    ) -> Result<ResolverSettings> {
        std::fs::create_dir_all(&self.directory)?;
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.directory.join("config.lock"))?;
        lock.lock()?;
        let mut settings = self.read()?;
        change(&mut settings)?;
        settings.validate()?;
        atomic_write(
            &self.directory.join("config.json"),
            &serde_json::to_vec_pretty(&settings)?,
        )?;
        Ok(settings)
    }

    /// Updates a connection without storing credentials or changing its profiles.
    pub fn set_connection(
        &self,
        name: &str,
        connection: ModelConnection,
    ) -> Result<ResolverSettings> {
        validate_name(name)?;
        self.update(|s| {
            s.connections.insert(name.into(), connection);
            Ok(())
        })
    }

    /// Unspecified prompt, limits, and request options retain their existing values.
    pub fn set_profile(
        &self,
        name: &str,
        mut profile: ResolverProfile,
    ) -> Result<ResolverSettings> {
        validate_name(name)?;
        self.update(|s| {
            if let Some(existing) = s.profiles.get(name) {
                profile.system_prompt = profile
                    .system_prompt
                    .or_else(|| existing.system_prompt.clone());
                profile.timeout_seconds = profile.timeout_seconds.or(existing.timeout_seconds);
                profile.max_input_bytes = profile.max_input_bytes.or(existing.max_input_bytes);
                profile.max_output_tokens =
                    profile.max_output_tokens.or(existing.max_output_tokens);
                profile.request_options = profile
                    .request_options
                    .or_else(|| existing.request_options.clone());
            }
            s.profiles.insert(name.into(), profile);
            Ok(())
        })
    }

    /// Selects the default for explicit model resolution; discovery remains unchanged.
    pub fn use_profile(&self, name: &str) -> Result<ResolverSettings> {
        self.update(|s| {
            s.profile(name)?;
            s.default_profile = Some(name.into());
            Ok(())
        })
    }

    pub fn clear_default(&self) -> Result<ResolverSettings> {
        self.update(|s| {
            s.default_profile = None;
            Ok(())
        })
    }

    pub fn remove_profile(&self, name: &str) -> Result<ResolverSettings> {
        self.update(|s| {
            if s.default_profile.as_deref() == Some(name) {
                return Err(Error::new(
                    ErrorKind::Conflict,
                    "Select another default profile or use --clear before removing this one",
                ));
            }
            if s.profiles.remove(name).is_none() {
                return Err(Error::new(
                    ErrorKind::NotFound,
                    "Resolver profile is not configured",
                ));
            }
            Ok(())
        })
    }

    pub fn remove_connection(&self, name: &str) -> Result<ResolverSettings> {
        self.update(|s| {
            if s.profiles.values().any(|p| p.connection == name) {
                return Err(Error::new(
                    ErrorKind::Conflict,
                    "Remove or reconfigure profiles using this connection first",
                ));
            }
            if s.connections.remove(name).is_none() {
                return Err(Error::new(
                    ErrorKind::NotFound,
                    "Model connection is not configured",
                ));
            }
            Ok(())
        })
    }
}
