use clap::Subcommand;
use lore_core::{KnowledgeStoreDescriptor, KnowledgeStoreId, RepositoryRegistration, Result};
use lore_server::{PrincipalConfig, ServerConfig, StoreGrant};
use serde_json::{Value, json};
use std::{collections::BTreeMap, net::SocketAddr, path::PathBuf};

#[derive(Subcommand)]
pub enum ServerOperation {
    /// Print a starting configuration; edit it to add stores, principals, and narrower grants.
    Config {
        #[arg(long)]
        store: Option<KnowledgeStoreId>,
        #[arg(long, required = true)]
        repository: Vec<String>,
        #[arg(long, required = true)]
        backend: Vec<String>,
        #[arg(long, default_value = "owner")]
        principal: String,
        #[arg(long, default_value = "LORE_SERVER_TOKEN")]
        token_env: String,
    },
    /// Serve authenticated mapping discovery and synchronization.
    Serve {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        data_dir: PathBuf,
        #[arg(long, default_value = "127.0.0.1:7766")]
        listen: SocketAddr,
    },
}

pub fn execute(command: &ServerOperation) -> Result<Value> {
    match command {
        ServerOperation::Config {
            store,
            repository,
            backend,
            principal,
            token_env,
        } => {
            let store = store.clone().unwrap_or_default();
            let descriptor = KnowledgeStoreDescriptor {
                id: store.clone(),
                integration_policy: Default::default(),
                repositories: repository
                    .iter()
                    .map(|id| (id.clone(), RepositoryRegistration::default()))
                    .collect::<BTreeMap<_, _>>(),
                backends: backend.iter().cloned().collect(),
            };
            descriptor.validate()?;
            let config = ServerConfig {
                stores: vec![descriptor],
                principals: vec![PrincipalConfig {
                    name: principal.clone(),
                    token_env: token_env.clone(),
                    grants: vec![StoreGrant {
                        store: store.clone(),
                        repositories: repository.iter().cloned().collect(),
                        backends: backend.iter().cloned().collect(),
                        write: true,
                    }],
                }],
            };
            Ok(serde_json::to_value(config)?)
        }
        ServerOperation::Serve {
            config,
            data_dir,
            listen,
        } => {
            lore_server::serve(config, data_dir, *listen)?;
            Ok(json!({"stopped": true}))
        }
    }
}
