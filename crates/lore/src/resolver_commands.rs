use clap::Subcommand;
use lore_core::Result;
use lore_local::{CredentialStore, ModelConnection, ResolverProfile, ResolverRegistry};
use lore_resolver::ChatConfig;
use serde_json::{Value, json};
use std::{
    io::Read,
    path::{Path, PathBuf},
};

const CREDENTIAL_NAMESPACE: &str = "resolver";

#[derive(Subcommand)]
pub enum ResolverOperation {
    /// List connections, profiles, and the default. Credentials are excluded.
    List,
    /// Select the default profile for explicit model-assisted projection.
    Use {
        #[arg(required_unless_present = "clear", conflicts_with = "clear")]
        name: Option<String>,
        /// Remove the selected default, restoring built-in model configuration.
        #[arg(long)]
        clear: bool,
    },
    /// Configure model API endpoints and authentication-variable names.
    Connection {
        #[command(subcommand)]
        command: ConnectionOperation,
    },
    /// Configure models, prompts, and request limits independently of credentials.
    Profile {
        #[command(subcommand)]
        command: ProfileOperation,
    },
    /// Manage user-local credentials for model connections.
    Auth {
        #[command(subcommand)]
        command: AuthOperation,
    },
}

#[derive(Subcommand)]
pub enum ConnectionOperation {
    Set {
        name: String,
        #[arg(long, default_value = "https://endpoint.access.furiosa.dev/v1")]
        url: String,
        #[arg(long, default_value = "FURIOSA_ACCESS_API_KEY")]
        token_env: String,
    },
    Remove {
        name: String,
    },
}

#[derive(Subcommand)]
pub enum ProfileOperation {
    /// Create or update a profile. Unspecified prompt, limits, and request options retain existing values.
    Set {
        name: String,
        #[arg(long)]
        connection: String,
        #[arg(long)]
        model: String,
        #[arg(long)]
        prompt_file: Option<PathBuf>,
        #[arg(long)]
        timeout_seconds: Option<u64>,
        #[arg(long)]
        max_input_bytes: Option<usize>,
        #[arg(long)]
        max_output_tokens: Option<usize>,
        /// JSON object of non-secret API parameters, such as sampling or chat-template options.
        #[arg(long)]
        request_options_file: Option<PathBuf>,
    },
    Remove {
        name: String,
    },
}

#[derive(Subcommand)]
pub enum AuthOperation {
    /// Read a model connection token from standard input, outside command arguments.
    Set {
        name: String,
        #[arg(long, required = true)]
        stdin: bool,
    },
    Status {
        name: String,
    },
    Remove {
        name: String,
    },
}

pub fn configure(home: &Path, command: &ResolverOperation) -> Result<Value> {
    let registry = ResolverRegistry::new(home);
    let credentials = CredentialStore::new(home);
    let settings = match command {
        ResolverOperation::List => registry.read()?,
        ResolverOperation::Use { name, .. } => match name {
            Some(name) => registry.use_profile(name)?,
            None => registry.clear_default()?,
        },
        ResolverOperation::Connection { command } => match command {
            ConnectionOperation::Set {
                name,
                url,
                token_env,
            } => registry.set_connection(
                name,
                ModelConnection {
                    base_url: url.trim_end_matches('/').into(),
                    token_env: token_env.clone(),
                },
            )?,
            ConnectionOperation::Remove { name } => {
                let settings = registry.remove_connection(name)?;
                credentials.remove_connection(CREDENTIAL_NAMESPACE, name)?;
                settings
            }
        },
        ResolverOperation::Profile { command } => match command {
            ProfileOperation::Set {
                name,
                connection,
                model,
                prompt_file,
                timeout_seconds,
                max_input_bytes,
                max_output_tokens,
                request_options_file,
            } => {
                let profile = ResolverProfile {
                    connection: connection.clone(),
                    model: model.clone(),
                    system_prompt: match prompt_file {
                        Some(path) => Some(std::fs::read_to_string(path)?),
                        None => None,
                    },
                    timeout_seconds: *timeout_seconds,
                    max_input_bytes: *max_input_bytes,
                    max_output_tokens: *max_output_tokens,
                    request_options: match request_options_file {
                        Some(path) => Some(serde_json::from_slice(&std::fs::read(path)?)?),
                        None => None,
                    },
                };
                registry.set_profile(name, profile)?
            }
            ProfileOperation::Remove { name } => registry.remove_profile(name)?,
        },
        ResolverOperation::Auth { command } => {
            let settings = registry.read()?;
            let name = match command {
                AuthOperation::Set { name, .. }
                | AuthOperation::Status { name }
                | AuthOperation::Remove { name } => name,
            };
            let connection = settings.connection(name)?;
            return match command {
                AuthOperation::Set { .. } => {
                    let mut token = String::new();
                    std::io::stdin()
                        .take(16 * 1024 + 1)
                        .read_to_string(&mut token)?;
                    credentials.set_connection(
                        CREDENTIAL_NAMESPACE,
                        name,
                        &connection.base_url,
                        &token,
                    )?;
                    Ok(json!({"connection":name,"stored":true}))
                }
                AuthOperation::Remove { .. } => {
                    credentials.remove_connection(CREDENTIAL_NAMESPACE, name)?;
                    Ok(json!({"connection":name,"stored":false}))
                }
                AuthOperation::Status { .. } => {
                    let stored = credentials
                        .get_connection(CREDENTIAL_NAMESPACE, name, &connection.base_url)?
                        .is_some();
                    let env =
                        std::env::var(&connection.token_env).is_ok_and(|s| !s.trim().is_empty());
                    Ok(
                        json!({"connection":name,"stored":stored,"source":if env {"environment"} else if stored {"stored"} else {"missing"}}),
                    )
                }
            };
        }
    };
    Ok(serde_json::to_value(settings)?)
}

pub struct ConfiguredModel {
    pub config: ChatConfig,
    pub token: Option<String>,
    pub profile: Option<String>,
    pub connection: Option<String>,
}

/// Explicit configuration bypasses named profiles; otherwise the selected default is used.
pub fn model_config(
    home: &Path,
    profile: Option<&str>,
    file: Option<&Path>,
) -> Result<ConfiguredModel> {
    if let Some(file) = file {
        return Ok(ConfiguredModel {
            config: serde_json::from_slice(&std::fs::read(file)?)?,
            token: None,
            profile: None,
            connection: None,
        });
    }
    let settings = ResolverRegistry::new(home).read()?;
    let Some(name) = profile.or(settings.default_profile.as_deref()) else {
        return Ok(ConfiguredModel {
            config: ChatConfig::default(),
            token: None,
            profile: None,
            connection: None,
        });
    };
    let profile = settings.profile(name)?;
    let connection = settings.connection(&profile.connection)?;
    let defaults = ChatConfig::default();
    let config = ChatConfig {
        base_url: connection.base_url.clone(),
        token_env: connection.token_env.clone(),
        model: profile.model.clone(),
        system_prompt: profile
            .system_prompt
            .clone()
            .unwrap_or(defaults.system_prompt),
        timeout_seconds: profile.timeout_seconds.unwrap_or(defaults.timeout_seconds),
        max_input_bytes: profile.max_input_bytes.unwrap_or(defaults.max_input_bytes),
        max_output_tokens: profile
            .max_output_tokens
            .unwrap_or(defaults.max_output_tokens),
        request_options: profile.request_options.clone().unwrap_or_default(),
    };
    let token = if std::env::var(&connection.token_env).is_ok_and(|s| !s.trim().is_empty()) {
        None
    } else {
        CredentialStore::new(home).get_connection(
            CREDENTIAL_NAMESPACE,
            &profile.connection,
            &connection.base_url,
        )?
    };
    Ok(ConfiguredModel {
        config,
        token,
        profile: Some(name.into()),
        connection: Some(profile.connection.clone()),
    })
}
