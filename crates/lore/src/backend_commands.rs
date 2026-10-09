use clap::Subcommand;
use lore_core::{DocumentReference, Error, ErrorKind, Result};
use lore_local::{BackendConfig, BackendRegistry, CredentialStore, DiscoveryScope, IndexProgress};
use lore_notion::{NotionBackend, page_id};
use serde_json::{Value, json};
use std::{io::Read, path::Path};

#[derive(Subcommand)]
pub enum BackendOperation {
    /// Change the display name without changing the connection's document-reference identity.
    SetName {
        backend: String,
        name: String,
    },
    /// Manage shared connection credentials for CLI, editor, and hook access.
    Auth {
        #[command(subcommand)]
        operation: AuthOperation,
    },
    AddNotion {
        name: String,
        #[arg(long, required = true)]
        root: Vec<String>,
        #[arg(long, default_value = "NOTION_TOKEN")]
        token_env: String,
        #[arg(long, default_value = "https://api.notion.com/v1")]
        api_url: String,
    },
    List,
    Remove {
        name: String,
    },
}

#[derive(Subcommand)]
pub enum AuthOperation {
    /// Save a token supplied through stdin; the token is never included in command arguments.
    Set {
        name: String,
        #[arg(long, required = true)]
        stdin: bool,
    },
    /// Report credential availability without returning token values.
    Status {
        name: String,
    },
    Remove {
        name: String,
    },
}

#[derive(Subcommand)]
pub enum DocsOperation {
    /// Enable or disable direct-child discovery for one page; reading remains available.
    Discovery {
        backend: String,
        page: String,
        #[arg(long, conflicts_with = "disable", required_unless_present = "disable")]
        enable: bool,
        #[arg(long, conflicts_with = "enable")]
        disable: bool,
    },
    /// Read document metadata independently of its body.
    Describe {
        backend: String,
        page: String,
        #[arg(long)]
        cached: bool,
        #[arg(long, conflicts_with = "cached")]
        refresh: bool,
    },
    /// List configured roots, or documents directly contained in a page.
    Browse {
        backend: String,
        page: Option<String>,
        #[arg(long)]
        cached: bool,
        #[arg(long, conflicts_with = "cached")]
        refresh: bool,
    },
    /// Read a cached observation, retrieving it on a cache miss. Refresh is explicit.
    Show {
        backend: String,
        page: String,
        #[arg(long)]
        cached: bool,
        #[arg(long, conflicts_with = "cached")]
        refresh: bool,
        #[arg(long, conflicts_with = "json")]
        raw: bool,
    },
    /// Refresh titles from configured pages and explicitly enabled child-discovery pages.
    Index {
        backend: String,
        /// Emit progress as JSON lines on stderr; stdout remains the result.
        #[arg(long)]
        progress: bool,
    },
    /// Refresh a document, or the title index within configured discovery scope.
    Fetch {
        backend: String,
        page: Option<String>,
        #[arg(long, conflicts_with = "page")]
        with_content: bool,
        /// Reuse a sufficiently recent title index; applies only to metadata fetch.
        #[arg(long, conflicts_with_all = ["page", "with_content"])]
        max_age_seconds: Option<u64>,
        /// Emit progress as JSON lines on stderr; stdout remains the result.
        #[arg(long)]
        progress: bool,
    },
    /// Search cached titles within the configured discovery scope. Run docs index first.
    Search {
        backend: String,
        #[arg(default_value = "")]
        query: String,
    },
}

pub fn configure(home: &Path, command: &BackendOperation) -> Result<Value> {
    let registry = BackendRegistry::new(home);
    match command {
        BackendOperation::SetName { backend, name } => {
            registry.set_display_name(backend, name.clone())?;
            Ok(json!({"backend":backend, "display_name":name.trim()}))
        }
        BackendOperation::Auth { operation } => {
            let store = CredentialStore::new(home);
            match operation {
                AuthOperation::Set { name, .. } => {
                    let config = registry.get(name)?;
                    let mut token = String::new();
                    std::io::stdin()
                        .take(16 * 1024 + 1)
                        .read_to_string(&mut token)
                        .map_err(|_| {
                            Error::invalid("Could not read connection token from stdin")
                        })?;
                    if token.len() > 16 * 1024 {
                        return Err(Error::invalid("Connection token input exceeds 16 KiB"));
                    }
                    store.set(name, &config, &token)?;
                    Ok(json!({"backend": name, "stored": true}))
                }
                AuthOperation::Status { name } => {
                    let config = registry.get(name)?;
                    let BackendConfig::Notion { token_env, .. } = &config;
                    let environment =
                        std::env::var(token_env).is_ok_and(|token| !token.trim().is_empty());
                    let stored = store.get(name, &config)?.is_some();
                    Ok(
                        json!({"backend": name, "stored": stored, "environment": environment,
                        "source": if environment { "environment" } else if stored { "stored" } else { "none" }}),
                    )
                }
                AuthOperation::Remove { name } => {
                    registry.get(name)?;
                    store.remove(name)?;
                    Ok(json!({"backend": name, "stored": false}))
                }
            }
        }
        BackendOperation::List => Ok(serde_json::to_value(registry.list()?)?),
        BackendOperation::Remove { name } => {
            registry.remove(name)?;
            CredentialStore::new(home).remove(name)?;
            Ok(json!({"removed":name,"mappings_preserved":true}))
        }
        BackendOperation::AddNotion {
            name,
            root,
            token_env,
            api_url,
        } => {
            if token_env.is_empty()
                || !token_env
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_')
            {
                return Err(Error::invalid("Token environment variable name is invalid"));
            }
            let _ = NotionBackend::new(name.clone(), "configuration-validation".into(), api_url)?;
            let mut roots = root
                .iter()
                .map(|id| page_id(id))
                .collect::<Result<Vec<_>>>()?;
            roots.sort();
            roots.dedup();
            registry.add(
                name,
                BackendConfig::Notion {
                    display_name: None,
                    roots,
                    discovery_pages: vec![],
                    token_env: token_env.clone(),
                    api_url: api_url.clone(),
                },
            )?;
            Ok(json!({"backend":name,"configured":true,"credentials_stored":false}))
        }
    }
}

fn configured(
    home: &Path,
    name: &str,
    cached: bool,
) -> Result<(Vec<DocumentReference>, Option<NotionBackend>)> {
    let config = BackendRegistry::new(home).get(name)?;
    let BackendConfig::Notion {
        roots,
        token_env,
        api_url,
        ..
    } = &config;
    let roots = roots
        .iter()
        .cloned()
        .map(|id| DocumentReference {
            backend: name.into(),
            id,
        })
        .collect();
    let backend = if cached {
        None
    } else {
        let token = match std::env::var(token_env) {
            Ok(token) if !token.trim().is_empty() => token,
            _ => CredentialStore::new(home).get(name, &config)?.ok_or_else(|| {
                Error::new(ErrorKind::PermissionDenied,
                    format!("Set a shared token with `lore backend auth set {name} --stdin`, or set {token_env}"))
            })?,
        };
        Some(NotionBackend::new(name.into(), token, api_url)?)
    };
    Ok((roots, backend))
}

pub fn reference(home: &Path, backend: &str, page: &str) -> Result<DocumentReference> {
    BackendRegistry::new(home).get(backend)?;
    Ok(DocumentReference {
        backend: backend.into(),
        id: page_id(page)?,
    })
}

fn prefer_cache<T>(
    cached: bool,
    refresh: bool,
    read_cached: impl FnOnce() -> Result<T>,
    retrieve: impl FnOnce() -> Result<T>,
) -> Result<T> {
    if !refresh {
        match read_cached() {
            Ok(value) => return Ok(value),
            Err(error) if cached || error.kind != ErrorKind::NotCached => return Err(error),
            Err(_) => {}
        }
    }
    retrieve()
}

pub fn documents(home: &Path, command: &DocsOperation) -> Result<Value> {
    let registry = BackendRegistry::new(home);
    if let DocsOperation::Discovery {
        backend,
        page,
        enable,
        ..
    } = command
    {
        let page = page_id(page)?;
        registry.set_child_discovery(backend, page.clone(), *enable)?;
        return Ok(json!({"backend":backend,"page":page,"enabled":enable}));
    }
    let name = match command {
        DocsOperation::Describe { backend, .. }
        | DocsOperation::Browse { backend, .. }
        | DocsOperation::Show { backend, .. }
        | DocsOperation::Index { backend, .. }
        | DocsOperation::Fetch { backend, .. }
        | DocsOperation::Search { backend, .. } => backend,
        DocsOperation::Discovery { .. } => unreachable!(),
    };
    let (roots, _) = configured(home, name, true)?;
    let BackendConfig::Notion {
        discovery_pages, ..
    } = registry.get(name)?;
    let scope = DiscoveryScope {
        roots: roots.clone(),
        discovery_pages: discovery_pages
            .into_iter()
            .map(|id| DocumentReference {
                backend: name.clone(),
                id,
            })
            .collect(),
    };
    let cache = registry.cache(name);
    let reporting = matches!(
        command,
        DocsOperation::Index { progress: true, .. } | DocsOperation::Fetch { progress: true, .. }
    );
    let report = |progress: IndexProgress| {
        if reporting {
            eprintln!("{}", json!({"progress": progress}));
        }
    };
    let backend = || {
        configured(home, name, false).map(|(_, backend)| {
            let backend = backend.unwrap();
            if reporting {
                backend.with_request_observer(|requests| {
                    eprintln!(
                        "{}",
                        json!({"progress": IndexProgress::Request { requests }})
                    )
                })
            } else {
                backend
            }
        })
    };
    match command {
        DocsOperation::Describe {
            page,
            cached,
            refresh,
            ..
        } => {
            let reference = reference(home, name, page)?;
            Ok(serde_json::to_value(prefer_cache(
                *cached,
                *refresh,
                || cache.metadata(None, &reference),
                || cache.metadata(Some(&backend()?), &reference),
            )?)?)
        }
        DocsOperation::Browse {
            page,
            cached,
            refresh,
            ..
        } => match page {
            Some(page) => {
                let reference = reference(home, name, page)?;
                if !scope.discovery_pages.contains(&reference) {
                    return Err(Error::new(
                        ErrorKind::Unsupported,
                        "Child discovery is disabled for this page; enable it with `docs discovery --enable`, or read the page with `docs show`",
                    ));
                }
                Ok(serde_json::to_value(prefer_cache(
                    *cached,
                    *refresh,
                    || cache.children(None, &reference),
                    || cache.children(Some(&backend()?), &reference),
                )?)?)
            }
            None => Ok(serde_json::to_value(prefer_cache(
                *cached,
                *refresh,
                || cache.roots(None, &roots),
                || cache.roots(Some(&backend()?), &roots),
            )?)?),
        },
        DocsOperation::Show {
            page,
            cached,
            refresh,
            ..
        } => {
            let reference = reference(home, name, page)?;
            Ok(serde_json::to_value(prefer_cache(
                *cached,
                *refresh,
                || cache.read(None, &reference),
                || cache.read(Some(&backend()?), &reference),
            )?)?)
        }
        DocsOperation::Index { .. } => Ok(serde_json::to_value(
            cache.fetch_index_with_progress(&scope, None, backend, report)?,
        )?),
        DocsOperation::Search { query, .. } => {
            Ok(serde_json::to_value(cache.search(query, &scope)?)?)
        }
        DocsOperation::Fetch {
            page,
            with_content,
            max_age_seconds,
            ..
        } => {
            if let Some(page) = page {
                let reference = reference(home, name, page)?;
                let content = cache.read(Some(&backend()?), &reference)?;
                Ok(serde_json::to_value(content)?)
            } else {
                let index =
                    cache.fetch_index_with_progress(&scope, *max_age_seconds, backend, report)?;
                let mut results = vec![];
                if *with_content {
                    let backend = configured(home, name, false)?.1.unwrap();
                    for document in &index.value.documents {
                        results.push(match cache.read(Some(&backend), &document.reference) {
                            Ok(content) => json!({
                                "status": "fetched",
                                "document": document.reference,
                                "partial": content.value.partial,
                                "warnings": content.value.warnings,
                            }),
                            Err(error) => json!({
                                "status": "failed",
                                "document": document.reference,
                                "error": {"kind": error.kind, "message": error.message},
                            }),
                        });
                    }
                }
                let mut value = serde_json::to_value(index)?;
                value["results"] = json!(results);
                Ok(value)
            }
        }
        DocsOperation::Discovery { .. } => unreachable!(),
    }
}

pub fn print(value: &Value, command: &DocsOperation) {
    match command {
        DocsOperation::Discovery { .. } => println!(
            "Child discovery {} for {}:{}",
            if value["enabled"] == true {
                "enabled"
            } else {
                "disabled"
            },
            value["backend"].as_str().unwrap(),
            value["page"].as_str().unwrap()
        ),
        DocsOperation::Describe { .. } => println!(
            "{}  {}",
            value["value"]["reference"]["id"].as_str().unwrap(),
            value["value"]["title"].as_str().unwrap()
        ),
        DocsOperation::Show { raw, .. } => {
            if !raw {
                println!(
                    "{}\n{}\n",
                    value["value"]["document"]["title"]
                        .as_str()
                        .unwrap_or_default(),
                    value["value"]["document"]["url"]
                        .as_str()
                        .unwrap_or_default()
                );
            }
            println!("{}", value["value"]["content"].as_str().unwrap_or_default());
        }
        DocsOperation::Fetch { page: Some(_), .. } => println!(
            "Fetched {}",
            value["value"]["document"]["title"].as_str().unwrap()
        ),
        _ => {
            for doc in value["value"]["documents"].as_array().unwrap() {
                println!(
                    "{}  {}",
                    doc["reference"]["id"].as_str().unwrap(),
                    doc["title"].as_str().unwrap()
                );
            }
        }
    }
    if value["cached"] == true {
        eprintln!(
            "Cached observation from {} ms since Unix epoch",
            value["fetched_at_ms"]
        );
    }
    if value["value"]["partial"] == true {
        eprintln!("Partial content; open the original in Notion");
    }
    if let Some(warnings) = value["value"]["warnings"].as_array() {
        for warning in warnings {
            eprintln!("Warning: {}", warning.as_str().unwrap_or_default());
        }
    }
    if let Some(results) = value["results"].as_array() {
        for result in results {
            if result["status"] == "failed" {
                eprintln!(
                    "Failed to fetch {}: {}",
                    result["document"]["id"].as_str().unwrap(),
                    result["error"]["message"].as_str().unwrap()
                );
            } else if let Some(warnings) = result["warnings"].as_array() {
                for warning in warnings {
                    eprintln!(
                        "Warning for {}: {}",
                        result["document"]["id"].as_str().unwrap(),
                        warning.as_str().unwrap_or_default()
                    );
                }
            }
        }
    }
}
