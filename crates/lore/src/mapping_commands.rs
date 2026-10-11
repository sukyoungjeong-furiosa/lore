use clap::{Subcommand, ValueEnum};
use lore_core::{
    Actor, AnchorResolver, Error, MappingId, PathScope, Result, RevisionId, TextResolver,
};
use lore_local::{LocatedMapping, Workspace};
use lore_resolver::{ChatProposer, ModelResolver};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, ValueEnum)]
pub enum ResolverKind {
    Text,
    Model,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum ConflictChoice {
    Local,
    Shared,
}

#[derive(Subcommand)]
pub enum MapOperation {
    /// Resolve a conflict in the default local store, or select a store connection explicitly.
    ResolveConflict {
        id: MappingId,
        #[arg(long)]
        if_match: RevisionId,
        #[arg(long)]
        if_shared: RevisionId,
        #[arg(long, value_enum)]
        take: ConflictChoice,
    },
    /// Retired: use Lore remote connections and proposal-based sharing.
    #[command(alias = "source", hide = true)]
    Catalog {
        #[command(subcommand)]
        command: CatalogOperation,
    },
    #[command(hide = true)]
    Fetch {
        #[arg(long)]
        id: Option<MappingId>,
        #[arg(long)]
        max_age_seconds: Option<u64>,
    },
    #[command(hide = true)]
    Publish {
        id: MappingId,
        #[arg(long)]
        if_match: RevisionId,
        #[arg(long)]
        if_shared: Option<RevisionId>,
    },
    Create {
        backend: String,
        document: String,
        #[arg(required_unless_present = "glob")]
        location: Option<String>,
        #[arg(long)]
        glob: Vec<String>,
        #[arg(long, default_value = "")]
        context: String,
    },
    Ls {
        path: Option<PathBuf>,
        #[arg(long)]
        recursive: bool,
        #[arg(long)]
        include_archived: bool,
        #[arg(long, requires = "document")]
        backend: Option<String>,
        #[arg(long, requires = "backend")]
        document: Option<String>,
    },
    Show {
        id: MappingId,
        #[arg(long)]
        revision: Option<RevisionId>,
    },
    Edit {
        id: MappingId,
        #[arg(long)]
        if_match: RevisionId,
        #[arg(long)]
        context: String,
    },
    Attach {
        id: MappingId,
        #[arg(required_unless_present = "glob")]
        location: Option<String>,
        #[arg(long)]
        glob: Vec<String>,
        #[arg(long)]
        if_match: RevisionId,
    },
    Archive {
        id: MappingId,
        #[arg(long)]
        if_match: RevisionId,
    },
    Restore {
        id: MappingId,
        #[arg(long)]
        if_match: RevisionId,
    },
    History {
        id: MappingId,
    },
    Resolve {
        id: MappingId,
        /// Model mode uses exact-text resolution first, then proposes correspondence through the configured API.
        #[arg(long, value_enum, default_value = "text")]
        resolver: ResolverKind,
        /// Candidate file or directory; paths are literal, not glob patterns.
        #[arg(long)]
        path: Option<PathBuf>,
        /// JSON model configuration; omitted fields use Furiosa Access defaults.
        #[arg(long)]
        resolver_config: Option<PathBuf>,
        /// Named model profile; omitted uses the selected default profile.
        #[arg(long, conflicts_with = "resolver_config")]
        profile: Option<String>,
    },
    Install {
        #[arg(long)]
        from: PathBuf,
        #[arg(long)]
        path: PathBuf,
    },
}

#[derive(Subcommand)]
pub enum CatalogOperation {
    Set {
        url: String,
        #[arg(long)]
        token_env: Option<String>,
    },
    Show,
    Remove,
}

fn reference(
    workspace: &Workspace,
    home: &Path,
    backend: &str,
    document: &str,
) -> Result<lore_core::DocumentReference> {
    if backend == "local" {
        let id: lore_core::DocumentId = document.parse()?;
        workspace.read(&id, None)?;
        Ok(lore_core::DocumentReference {
            backend: backend.into(),
            id: id.to_string(),
        })
    } else {
        super::backend_commands::reference(home, backend, document)
    }
}

fn capture(workspace: &Workspace, location: &str) -> Result<lore_core::Anchor> {
    let (path, range) = location
        .rsplit_once(':')
        .ok_or_else(|| Error::invalid("Use path:line or path:start-end"))?;
    let (start, end) = range.split_once('-').unwrap_or((range, range));
    let start = start
        .parse()
        .map_err(|_| Error::invalid("Invalid first line"))?;
    let end = end
        .parse()
        .map_err(|_| Error::invalid("Invalid last line"))?;
    let path = workspace.code.relative(Path::new(path))?;
    workspace.code.capture(&path, start, end)
}

fn scopes(workspace: &Workspace, patterns: &[String]) -> Vec<PathScope> {
    patterns
        .iter()
        .map(|glob| PathScope {
            repository: workspace.code.repository_id.clone(),
            glob: glob.clone(),
        })
        .collect()
}

pub fn print(value: &Value, command: &MapOperation) {
    match command {
        MapOperation::Catalog { .. } => {
            println!(
                "Mapping catalog connection: {}",
                serde_json::to_string_pretty(&value["source"]).unwrap()
            );
        }
        MapOperation::Fetch { .. } => {
            println!("{}", serde_json::to_string_pretty(value).unwrap());
        }
        MapOperation::Ls { .. } => {
            for item in value["mappings"].as_array().unwrap() {
                if let Some(store) = item["store"].as_str() {
                    println!("Store: {store}");
                }
                print_view(&item["view"]);
            }
        }
        MapOperation::History { .. } => {
            for item in value.as_array().unwrap() {
                print_view(item);
            }
        }
        MapOperation::Resolve { .. } => {
            print_view(&value["view"]);
            for projection in value["projections"].as_array().unwrap() {
                println!("{}", projection["explanation"].as_str().unwrap());
                for location in projection["locations"].as_array().unwrap() {
                    println!(
                        "  {}:{}-{}",
                        location["path"].as_str().unwrap(),
                        location["start"],
                        location["end"]
                    );
                }
                for (index, candidate) in projection["alternatives"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .enumerate()
                {
                    println!("  Candidate {} (unconfirmed):", index + 1);
                    for location in candidate.as_array().unwrap() {
                        println!(
                            "    {}:{}-{}",
                            location["path"].as_str().unwrap(),
                            location["start"],
                            location["end"]
                        );
                    }
                }
            }
        }
        _ if value.get("record").is_some() => print_view(value),
        _ => println!("{}", serde_json::to_string_pretty(value).unwrap()),
    }
}

fn print_view(view: &Value) {
    let mapping = &view["record"]["mapping"];
    println!(
        "{}  {}:{}{}\nRevision: {}\n{}",
        mapping["id"].as_str().unwrap(),
        mapping["document"]["backend"].as_str().unwrap(),
        mapping["document"]["id"].as_str().unwrap(),
        if mapping["archived"] == true {
            " [archived]"
        } else {
            ""
        },
        view["revision"].as_str().unwrap(),
        mapping["context"].as_str().unwrap()
    );
    for anchor in mapping["anchors"].as_array().unwrap() {
        println!(
            "  Recorded: {}:{}-{}",
            anchor["path"].as_str().unwrap(),
            anchor["start"],
            anchor["end"]
        );
    }
    if let Some(scopes) = mapping["scopes"].as_array() {
        for scope in scopes {
            println!("  Scope: {}", scope["glob"].as_str().unwrap());
        }
    }
}

pub fn execute(
    workspace: &Workspace,
    home: &Path,
    actor: Actor,
    command: &MapOperation,
) -> Result<Value> {
    match command {
        MapOperation::ResolveConflict { .. } => unreachable!(),
        MapOperation::Catalog { .. }
        | MapOperation::Fetch { .. }
        | MapOperation::Publish { .. } => Err(super::sync_commands::retired()),
        MapOperation::Create {
            backend,
            document,
            location,
            glob,
            context,
        } => {
            let reference = reference(workspace, home, backend, document)?;
            Ok(serde_json::to_value(
                workspace.create_mapping(
                    reference,
                    location
                        .as_deref()
                        .map(|location| capture(workspace, location))
                        .transpose()?
                        .into_iter()
                        .collect(),
                    scopes(workspace, glob),
                    context.clone(),
                    actor,
                )?,
            )?)
        }
        MapOperation::Ls {
            path,
            recursive,
            include_archived,
            backend,
            document,
        } => {
            let mut mappings = if let Some(path) = path {
                workspace.locate_mappings(
                    Some(&super::scope(workspace, path)?),
                    *recursive || workspace.code.root.join(path).is_dir(),
                    *include_archived,
                    &TextResolver,
                )?
            } else {
                workspace
                    .list_scoped_mappings(*include_archived)?
                    .into_iter()
                    .map(|item| LocatedMapping {
                        store: item.store,
                        view: item.view.into(),
                        projections: vec![],
                    })
                    .collect()
            };
            if let (Some(backend), Some(document)) = (backend, document) {
                let reference = reference(workspace, home, backend, document)?;
                mappings.retain(|item| item.view.record.mapping.document == reference);
            }
            Ok(json!({
                "mappings": mappings,
                "coverage": "active workspace mappings; original references and exact-text projections",
            }))
        }
        MapOperation::Show { id, revision } => Ok(serde_json::to_value(
            workspace.read_mapping(id, revision.as_ref())?,
        )?),
        MapOperation::Edit {
            id,
            if_match,
            context,
        } => Ok(serde_json::to_value(workspace.update_mapping(
            id,
            if_match,
            actor,
            |mapping| {
                mapping.context = context.clone();
                Ok(())
            },
        )?)?),
        MapOperation::Attach {
            id,
            location,
            glob,
            if_match,
        } => {
            let anchor = location
                .as_deref()
                .map(|location| capture(workspace, location))
                .transpose()?;
            let scopes = scopes(workspace, glob);
            Ok(serde_json::to_value(workspace.update_mapping(
                id,
                if_match,
                actor,
                |mapping| {
                    mapping.anchors.extend(anchor);
                    mapping.scopes.extend(scopes);
                    Ok(())
                },
            )?)?)
        }
        MapOperation::Archive { id, if_match } | MapOperation::Restore { id, if_match } => {
            let archived = matches!(command, MapOperation::Archive { .. });
            Ok(serde_json::to_value(workspace.update_mapping(
                id,
                if_match,
                actor,
                |mapping| {
                    mapping.archived = archived;
                    Ok(())
                },
            )?)?)
        }
        MapOperation::History { id } => Ok(serde_json::to_value(workspace.mapping_history(id)?)?),
        MapOperation::Resolve {
            id,
            resolver,
            path,
            resolver_config,
            profile,
        } => {
            let view = workspace.read_mapping(id, None)?;
            let requested = path
                .as_ref()
                .map(|path| super::scope(workspace, path))
                .transpose()?;
            let (resolver, details): (Box<dyn AnchorResolver>, Value) = match resolver {
                ResolverKind::Text => {
                    if resolver_config.is_some() || profile.is_some() {
                        return Err(Error::invalid(
                            "--resolver-config and --profile require --resolver model",
                        ));
                    }
                    (Box::new(TextResolver), json!({"kind":"text"}))
                }
                ResolverKind::Model => {
                    let selected = super::resolver_commands::model_config(
                        home,
                        profile.as_deref(),
                        resolver_config.as_deref(),
                    )?;
                    let details = json!({
                        "kind":"model",
                        "model":selected.config.model,
                        "profile":selected.profile,
                        "model_connection":selected.connection,
                        "config_digest":lore_core::hash(serde_json::to_vec(&selected.config)?),
                    });
                    let max_input_bytes = selected.config.max_input_bytes;
                    (
                        Box::new(ModelResolver {
                            proposer: ChatProposer::with_token(selected.config, selected.token)?,
                            max_input_bytes,
                        }),
                        details,
                    )
                }
            };
            let projections = view
                .record
                .mapping
                .anchors
                .iter()
                .map(|anchor| resolver.resolve(anchor, &workspace.code, requested.as_deref()))
                .collect::<Result<Vec<_>>>()?;
            Ok(
                json!({"store": workspace.mapping_store_for(id)?, "view":lore_core::MappingSummaryView::from(view),"projections":projections,"resolver":details}),
            )
        }
        MapOperation::Install { from, path } => {
            let source = Workspace::open(home, from)?;
            let selected = source.list_mappings(false)?;
            let requested = super::scope(workspace, path)?;
            let recursive = workspace.code.root.join(path).is_dir();
            let in_scope = |p: &str| {
                requested.is_empty()
                    || p == requested
                    || recursive && p.starts_with(&format!("{requested}/"))
            };
            let mut results = vec![];
            for view in selected {
                let mut relevant =
                    workspace.mapping_scope_matches(&view.record.mapping, &requested, recursive)?;
                for anchor in &view.record.mapping.anchors {
                    let projection =
                        TextResolver.resolve(anchor, &workspace.code, Some(&requested))?;
                    relevant |= anchor.context.repository == workspace.code.repository_id
                        && in_scope(&anchor.path)
                        || projection.possible_locations().any(|l| in_scope(&l.path));
                }
                if relevant {
                    results.push(
                        match workspace.install_mapping(&source, &view.record.mapping.id) {
                            Ok(view) => json!({"status":"installed","view":view}),
                            Err(error) => json!({
                                "status": "failed",
                                "id": view.record.mapping.id,
                                "error": {"kind": error.kind, "message": error.message},
                            }),
                        },
                    );
                }
            }
            Ok(json!({"results":results}))
        }
    }
}
