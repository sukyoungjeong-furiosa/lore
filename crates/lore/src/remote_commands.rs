use clap::Subcommand;
use lore_core::{MappingStoreId, Result};
use lore_local::{LoreRemote, RemoteRegistry};
use serde_json::{Value, json};
use std::path::Path;

#[derive(Subcommand)]
pub enum RemoteOperation {
    /// Record a mapping-store connection; server discovery and synchronization are not implemented yet.
    Add {
        name: String,
        #[arg(long)]
        url: String,
        #[arg(long)]
        store: MappingStoreId,
    },
    /// List user-local connections; configured does not mean connected or authorized.
    Ls,
    Show {
        name: String,
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

pub fn execute(home: &Path, command: &RemoteOperation) -> Result<Value> {
    let registry = RemoteRegistry::new(home);
    match command {
        RemoteOperation::Add { name, url, store } => {
            let remote = registry.add(
                name,
                LoreRemote {
                    server_url: url.clone(),
                    store: store.clone(),
                },
            )?;
            Ok(json!({"name": name, "remote": remote}))
        }
        RemoteOperation::Ls => Ok(json!({"remotes": registry.list()?})),
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
