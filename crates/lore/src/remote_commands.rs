use clap::Subcommand;
use lore_core::{Error, KnowledgeStoreId, Result};
use lore_local::{CredentialStore, LoreRemote, RemoteRegistry, Workspace};
use serde_json::{Value, json};
use std::path::Path;

#[derive(Subcommand)]
pub enum RemoteOperation {
    /// Record a mapping-store connection without fetching or activating mappings.
    Add {
        name: String,
        #[arg(long)]
        url: String,
        #[arg(long)]
        store: KnowledgeStoreId,
        #[arg(long)]
        token_env: Option<String>,
    },
    /// List user-local connections; configured does not mean connected or authorized.
    Ls,
    Show {
        name: String,
    },
    /// Read the server's accessible repository and backend registrations.
    Inspect {
        name: String,
    },
    /// Enable metadata discovery from this remote in the current workspace.
    Connect {
        name: String,
    },
    /// Disable discovery without discarding working mappings or baselines.
    Disconnect {
        name: String,
    },
    Auth {
        #[command(subcommand)]
        command: RemoteAuth,
    },
    /// Change the server endpoint without selecting a different store.
    SetUrl {
        name: String,
        url: String,
    },
    Remove {
        name: String,
    },
}

#[derive(Subcommand)]
pub enum RemoteAuth {
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

pub fn execute(home: &Path, repo: &Path, command: &RemoteOperation) -> Result<Value> {
    let registry = RemoteRegistry::new(home);
    match command {
        RemoteOperation::Add {
            name,
            url,
            store,
            token_env,
        } => {
            let remote = registry.add(
                name,
                LoreRemote {
                    server_url: url.clone(),
                    store: store.clone(),
                    token_env: token_env.clone(),
                },
            )?;
            Ok(json!({"name": name, "remote": remote}))
        }
        RemoteOperation::Ls => Ok(json!({"remotes": registry.list()?})),
        RemoteOperation::Inspect { name } => super::sync_commands::inspect(home, name),
        RemoteOperation::Connect { name } | RemoteOperation::Disconnect { name } => {
            let remote = registry.get(name)?;
            let workspace = Workspace::open(home, repo)?;
            if matches!(command, RemoteOperation::Connect { .. }) {
                workspace.connect_remote(&remote.store, name)?;
            } else {
                workspace.disconnect_remote(&remote.store)?;
            }
            Ok(json!({"connected_remotes": workspace.state()?.connected_remotes}))
        }
        RemoteOperation::Auth { command } => {
            let name = match command {
                RemoteAuth::Set { name, .. }
                | RemoteAuth::Status { name }
                | RemoteAuth::Remove { name } => name,
            };
            let remote = registry.get(name)?;
            let credentials = CredentialStore::new(home);
            match command {
                RemoteAuth::Set { stdin, .. } => {
                    let token = super::body(None, *stdin)?
                        .ok_or_else(|| Error::invalid("Read the remote token from stdin"))?;
                    credentials.set_connection("remote", name, &remote.server_url, &token)?;
                    Ok(json!({"name":name,"stored":true}))
                }
                RemoteAuth::Remove { .. } => {
                    credentials.remove_connection("remote", name)?;
                    Ok(json!({"name":name,"removed":true}))
                }
                RemoteAuth::Status { .. } => {
                    let variable = remote.token_env.as_deref().unwrap_or("LORE_REMOTE_TOKEN");
                    let source =
                        if std::env::var(variable).is_ok_and(|token| !token.trim().is_empty()) {
                            "environment"
                        } else if credentials
                            .get_connection("remote", name, &remote.server_url)?
                            .is_some()
                        {
                            "stored"
                        } else {
                            "missing"
                        };
                    Ok(json!({"name":name,"source":source}))
                }
            }
        }
        RemoteOperation::Show { name } => Ok(json!({"name": name, "remote": registry.get(name)?})),
        RemoteOperation::SetUrl { name, url } => {
            let remote = registry.set_url(name, url.clone())?;
            Ok(json!({"name": name, "remote": remote}))
        }
        RemoteOperation::Remove { name } => {
            registry.remove(name)?;
            Ok(json!({"removed": name}))
        }
    }
}
