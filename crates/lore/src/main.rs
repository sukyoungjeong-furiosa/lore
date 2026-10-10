use clap::{Parser, Subcommand, ValueEnum};
use lore_core::{
    Actor, AnchorResolver, Document, DocumentId, Error, ErrorKind, Result, RevisionId, TextResolver,
};
use lore_local::{SearchOptions, Workspace, home};
use serde_json::{Value, json};
use std::{io::Read, path::PathBuf};
mod backend_commands;
mod context_commands;
mod mapping_commands;
mod remote_commands;
mod resolver_commands;
mod server_commands;
mod store_commands;
mod sync_commands;

#[derive(Parser)]
#[command(version, about = "What the code doesn't tell you.")]
struct Cli {
    #[arg(long, global = true, default_value = ".")]
    repo: PathBuf,
    #[arg(long, global = true)]
    json: bool,
    /// Read accepted local knowledge while projecting against this code checkout.
    #[arg(long, global = true)]
    central: bool,
    #[arg(long, global = true, value_enum, default_value = "unknown")]
    author: Author,
    #[command(subcommand)]
    command: Operation,
}

#[derive(Clone, Copy, ValueEnum)]
enum Author {
    Human,
    Agent,
    Unknown,
}

#[derive(Clone, Copy, ValueEnum)]
enum DocumentChoice {
    Local,
    Shared,
}

impl From<Author> for Actor {
    fn from(value: Author) -> Self {
        match value {
            Author::Human => Self::Human,
            Author::Agent => Self::Agent,
            Author::Unknown => Self::Unknown,
        }
    }
}

#[derive(Subcommand)]
enum Operation {
    /// Submit committed knowledge to the local central branch, or a Lore server.
    Push {
        id: Option<lore_core::MappingId>,
        #[arg(long)]
        proposal: bool,
        #[arg(long, conflicts_with = "id")]
        document: Vec<DocumentId>,
        #[arg(long, conflicts_with = "id")]
        mapping_id: Vec<lore_core::MappingId>,
        #[arg(long, conflicts_with_all = ["document","mapping_id","id"])]
        all: bool,
        #[arg(long)]
        remote: Option<String>,
        #[arg(long, conflicts_with = "remote")]
        local_store: Option<String>,
        #[arg(long)]
        from_remote: Option<String>,
        #[arg(long, conflicts_with = "from_remote")]
        from_local_store: Option<String>,
    },
    /// Incorporate shared mappings for this repository without overwriting local changes.
    Pull {
        #[arg(long)]
        remote: Option<String>,
        #[arg(long, conflicts_with = "remote")]
        local_store: Option<String>,
        #[arg(long, conflicts_with = "path")]
        id: Option<lore_core::MappingId>,
        #[arg(long)]
        path: Option<PathBuf>,
        #[arg(long)]
        backend: Option<String>,
    },
    /// Cache shared mapping metadata or history without changing active revisions.
    Fetch {
        #[arg(long)]
        remote: Option<String>,
        #[arg(long, conflicts_with = "remote")]
        local_store: Option<String>,
        #[arg(long)]
        id: Option<lore_core::MappingId>,
    },
    /// Manage filesystem mapping stores independently of code repositories.
    Store {
        #[command(subcommand)]
        command: store_commands::StoreOperation,
    },
    Server {
        #[command(subcommand)]
        command: server_commands::ServerOperation,
    },
    /// Configure mapping-sharing connections independently of document backends.
    Remote {
        #[command(subcommand)]
        command: remote_commands::RemoteOperation,
    },
    /// Configure replaceable model resolvers and user-local model credentials.
    Resolver {
        #[command(subcommand)]
        command: resolver_commands::ResolverOperation,
    },
    /// Discover related Lore without reading document bodies.
    Context {
        #[command(subcommand)]
        command: context_commands::ContextOperation,
    },
    /// Configure document backends and shared connection credentials.
    Backend {
        #[command(subcommand)]
        command: backend_commands::BackendOperation,
    },
    /// Browse backend-owned documents without importing them.
    Docs {
        #[command(subcommand)]
        command: backend_commands::DocsOperation,
    },
    /// Maintain independent code-to-document mappings.
    Map {
        #[arg(long, global = true)]
        remote: Option<String>,
        #[arg(long, global = true, conflicts_with = "remote")]
        store: Option<lore_core::KnowledgeStoreId>,
        #[arg(long, global = true, conflicts_with_all = ["remote", "store"])]
        local_store: Option<String>,
        #[command(subcommand)]
        command: mapping_commands::MapOperation,
    },
    /// Initialize knowledge outside this Git checkout.
    Init {
        /// Override the immutable revision cache location; does not select a shared mapping store.
        #[arg(long = "cache-dir", alias = "store")]
        store: Option<PathBuf>,
        #[arg(long)]
        repository_id: Option<String>,
        #[arg(long)]
        local_store: Option<String>,
    },
    Status,
    /// Record one document or mapping on this workspace's knowledge branch.
    Commit {
        id: String,
        #[arg(long)]
        mapping: bool,
        #[arg(long)]
        if_match: RevisionId,
    },
    /// Select a committed document revision without discarding local changes.
    Checkout {
        target: String,
        #[arg(long)]
        create: bool,
    },
    /// Inspect knowledge worktrees without changing code worktrees.
    Worktree,
    /// Inspect or accept knowledge change proposals.
    Proposal {
        #[arg(long)]
        remote: Option<String>,
        #[command(subcommand)]
        command: ProposalOperation,
    },
    /// Resolve document versions explicitly; choosing local does not commit it.
    ResolveConflict {
        id: DocumentId,
        #[arg(long)]
        if_match: RevisionId,
        #[arg(long)]
        if_shared: RevisionId,
        #[arg(long, value_enum)]
        take: DocumentChoice,
    },
    Create {
        title: String,
        #[arg(long, conflicts_with = "stdin")]
        body_file: Option<PathBuf>,
        #[arg(long)]
        stdin: bool,
        #[arg(long)]
        directory: Option<String>,
    },
    /// Record a source reference, including its current code snapshot.
    Attach {
        id: DocumentId,
        location: String,
        #[arg(long)]
        if_match: Option<RevisionId>,
    },
    Search {
        #[arg(default_value = "")]
        query: String,
        #[arg(long)]
        include_archived: bool,
        #[arg(long)]
        exclude_agent: bool,
        #[arg(long)]
        directory: Option<String>,
    },
    /// Discover locally available documents by current or recorded code location.
    Ls {
        path: Option<PathBuf>,
        #[arg(long)]
        recursive: bool,
        #[arg(long)]
        include_archived: bool,
    },
    Show {
        id: DocumentId,
        #[arg(long, conflicts_with_all = ["committed", "revision"])]
        remote: Option<String>,
        #[arg(long)]
        committed: bool,
        #[arg(long)]
        revision: Option<RevisionId>,
        #[arg(long, conflicts_with = "json")]
        raw: bool,
    },
    /// Apply an edit against the revision observed by the caller.
    Edit {
        id: DocumentId,
        #[arg(long)]
        if_match: RevisionId,
        #[arg(long, conflicts_with = "stdin")]
        body_file: Option<PathBuf>,
        #[arg(long)]
        stdin: bool,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        directory: Option<String>,
    },
    Archive {
        id: DocumentId,
        #[arg(long)]
        if_match: RevisionId,
    },
    Restore {
        id: DocumentId,
        #[arg(long)]
        if_match: RevisionId,
    },
    History {
        id: DocumentId,
    },
    Links {
        id: DocumentId,
        #[arg(long)]
        revision: Option<RevisionId>,
    },
    Resolve {
        id: DocumentId,
    },
    /// Install selected immutable revisions from another local workspace.
    Install {
        id: Option<DocumentId>,
        #[arg(long)]
        from: PathBuf,
        #[arg(long, conflicts_with = "id")]
        path: Option<PathBuf>,
        #[arg(long, conflicts_with_all = ["id", "path"])]
        all: bool,
        #[arg(long, requires = "id")]
        revision: Option<RevisionId>,
    },
}

#[derive(Subcommand)]
enum ProposalOperation {
    List,
    Show {
        id: String,
    },
    Integrate {
        id: String,
        #[arg(long)]
        if_match: String,
    },
}

fn body(file: Option<&PathBuf>, stdin: bool) -> Result<Option<String>> {
    if let Some(file) = file {
        return Ok(Some(std::fs::read_to_string(file)?));
    }
    if stdin {
        let mut text = String::new();
        std::io::stdin().read_to_string(&mut text)?;
        return Ok(Some(text));
    }
    Ok(None)
}

fn scope(workspace: &Workspace, path: &std::path::Path) -> Result<String> {
    if path == std::path::Path::new(".") || path == workspace.code.root {
        Ok(String::new())
    } else {
        workspace.code.relative(path)
    }
}

fn execute(cli: &Cli) -> Result<Value> {
    let data_home = home()?;
    match &cli.command {
        Operation::Server { command } => return server_commands::execute(command),
        Operation::Store { command } => return store_commands::execute(&data_home, command),
        Operation::Remote { command } => {
            return remote_commands::execute(&data_home, &cli.repo, command);
        }
        Operation::Resolver { command } => {
            return resolver_commands::configure(&data_home, command);
        }
        Operation::Backend { command } => return backend_commands::configure(&data_home, command),
        Operation::Docs { command } => return backend_commands::documents(&data_home, command),
        _ => {}
    }
    if let Operation::Init {
        store,
        repository_id,
        local_store,
    } = &cli.command
    {
        let workspace = Workspace::init_with_local_store(
            &data_home,
            &cli.repo,
            store.as_deref(),
            repository_id.clone(),
            local_store.as_deref(),
        )?;
        return Ok(json!({"state_path":workspace.state_path,"workspace":workspace.state()?}));
    }
    let workspace = Workspace::open(&data_home, &cli.repo)?;
    let workspace = if cli.central {
        workspace.with_central_view()
    } else {
        workspace
    };
    if cli.central
        && matches!(
            cli.command,
            Operation::Commit { .. }
                | Operation::Create { .. }
                | Operation::Edit { .. }
                | Operation::Attach { .. }
                | Operation::Archive { .. }
                | Operation::Restore { .. }
                | Operation::Checkout { .. }
                | Operation::ResolveConflict { .. }
        )
    {
        return Err(Error::invalid(
            "Central knowledge is read-only; edit on a working branch and submit a proposal",
        ));
    }
    if cli.central
        && matches!(&cli.command, Operation::Map { command, .. } if matches!(command,
        mapping_commands::MapOperation::Create { .. } | mapping_commands::MapOperation::Edit { .. }
        | mapping_commands::MapOperation::Attach { .. } | mapping_commands::MapOperation::Archive { .. }
        | mapping_commands::MapOperation::Restore { .. } | mapping_commands::MapOperation::Publish { .. }
        | mapping_commands::MapOperation::Install { .. } | mapping_commands::MapOperation::ResolveConflict { .. }))
    {
        return Err(Error::invalid(
            "Central knowledge is read-only; edit mappings on a working branch",
        ));
    }
    let workspace = if let Operation::Map {
        store: Some(store), ..
    } = &cli.command
    {
        workspace.with_mapping_store(store.clone())
    } else if let Operation::Map {
        local_store: Some(name),
        ..
    } = &cli.command
    {
        workspace.with_mapping_store(
            lore_local::LocalStoreRegistry::new(&data_home)
                .get(name)?
                .store,
        )
    } else if let Operation::Map {
        remote: Some(name), ..
    } = &cli.command
    {
        sync_commands::scoped_workspace(workspace, &data_home, name)?
    } else {
        workspace
    };
    let actor: Actor = cli.author.into();
    match &cli.command {
        Operation::Push {
            remote,
            id,
            local_store,
            from_remote,
            from_local_store,
            proposal,
            document,
            mapping_id,
            all,
        } if id.is_none() => {
            if local_store.is_some() || from_remote.is_some() || from_local_store.is_some() {
                return Err(Error::invalid(
                    "Knowledge submission uses the selected store; mapping transfer flags require a mapping ID",
                ));
            }
            if let Some(remote) = remote {
                workspace.push_remote_knowledge(&data_home, remote, document, mapping_id, *all)
            } else {
                if !document.is_empty() || !mapping_id.is_empty() || *all {
                    return Err(Error::invalid(
                        "Selection flags apply to remote submission; local submission proposes the working branch",
                    ));
                }
                workspace.push_knowledge(*proposal)
            }
        }
        Operation::Push {
            id,
            remote,
            local_store,
            from_remote,
            from_local_store,
            ..
        } => sync_commands::push(
            &workspace,
            &data_home,
            id.as_ref().unwrap(),
            remote.as_deref(),
            local_store.as_deref(),
            from_remote.as_deref(),
            from_local_store.as_deref(),
        ),
        Operation::Pull {
            remote,
            local_store,
            id,
            path,
            backend,
        } if id.is_none() && path.is_none() && backend.is_none() && local_store.is_none() => {
            if let Some(remote) = remote {
                workspace.pull_remote_knowledge(&data_home, remote)
            } else {
                let mut result =
                    sync_commands::pull(&workspace, &data_home, None, None, None, None, None)?;
                result["knowledge"] = serde_json::to_value(workspace.pull_knowledge()?)?;
                Ok(result)
            }
        }
        Operation::Pull {
            remote,
            local_store,
            id,
            path,
            backend,
        } => sync_commands::pull(
            &workspace,
            &data_home,
            remote.as_deref(),
            local_store.as_deref(),
            id.as_ref(),
            path.as_ref(),
            backend.as_deref(),
        ),
        Operation::Fetch {
            remote,
            local_store,
            id,
        } if id.is_none() && remote.is_some() && local_store.is_none() => {
            workspace.fetch_remote_knowledge(&data_home, remote.as_ref().unwrap())
        }
        Operation::Fetch {
            remote,
            local_store,
            id,
        } => sync_commands::fetch(
            &workspace,
            &data_home,
            remote.as_deref(),
            local_store.as_deref(),
            id.as_ref(),
        ),
        Operation::Context { command } => {
            context_commands::execute(&workspace, &data_home, command)
        }
        Operation::Backend { .. }
        | Operation::Docs { .. }
        | Operation::Resolver { .. }
        | Operation::Remote { .. } => {
            unreachable!()
        }
        Operation::Server { .. } | Operation::Store { .. } => unreachable!(),
        Operation::Map {
            command,
            remote,
            store,
            local_store,
        } => {
            if let mapping_commands::MapOperation::ResolveConflict {
                id,
                if_match,
                if_shared,
                take,
            } = command
            {
                if store.is_some() {
                    return Err(Error::invalid(
                        "Conflict resolution requires a store connection, rather than --store",
                    ));
                }
                if remote.is_none() && local_store.is_none() && {
                    let state = workspace.state()?;
                    state.knowledge_worktree.as_ref().is_some_and(|worktree| {
                        state
                            .mappings
                            .get(id)
                            .and_then(|a| a.shared_baseline.as_ref())
                            .is_some_and(|baseline| {
                                baseline.source
                                    == format!("{}:branch:{}", worktree.store, worktree.branch)
                            })
                    })
                } {
                    return Ok(serde_json::to_value(
                        workspace.resolve_knowledge_mapping_conflict(
                            id,
                            if_match,
                            if_shared,
                            match take {
                                mapping_commands::ConflictChoice::Local => {
                                    lore_local::ConflictResolutionChoice::Local
                                }
                                mapping_commands::ConflictChoice::Shared => {
                                    lore_local::ConflictResolutionChoice::Shared
                                }
                            },
                            actor,
                        )?,
                    )?);
                }
                let target = sync_commands::select(
                    &workspace,
                    &data_home,
                    remote.as_deref(),
                    local_store.as_deref(),
                )?;
                return Ok(serde_json::to_value(workspace.resolve_mapping_conflict(
                    target.client.as_ref(),
                    &lore_core::SharedMappingKey {
                        store: target.store,
                        mapping: id.clone(),
                    },
                    if_match,
                    if_shared,
                    match take {
                        mapping_commands::ConflictChoice::Local => {
                            lore_local::ConflictResolutionChoice::Local
                        }
                        mapping_commands::ConflictChoice::Shared => {
                            lore_local::ConflictResolutionChoice::Shared
                        }
                    },
                    actor,
                )?)?);
            }
            if (remote.is_some() || local_store.is_some())
                && matches!(
                    command,
                    mapping_commands::MapOperation::Catalog { .. }
                        | mapping_commands::MapOperation::Publish { .. }
                )
            {
                return Err(Error::invalid(
                    "Use push for Lore remotes; legacy catalogs are configured separately",
                ));
            }
            if let mapping_commands::MapOperation::Fetch {
                id,
                max_age_seconds,
            } = command
                && (remote.is_some() || local_store.is_some())
            {
                if max_age_seconds.is_some() {
                    return Err(Error::invalid(
                        "--max-age-seconds is supported only by the legacy catalog; remote fetch refreshes explicitly",
                    ));
                }
                return sync_commands::fetch(
                    &workspace,
                    &data_home,
                    remote.as_deref(),
                    local_store.as_deref(),
                    id.as_ref(),
                );
            }
            if store.is_some()
                && matches!(
                    command,
                    mapping_commands::MapOperation::Fetch { .. }
                        | mapping_commands::MapOperation::Catalog { .. }
                        | mapping_commands::MapOperation::Publish { .. }
                )
            {
                return Err(Error::invalid(
                    "--store selects local mapping state; use --remote for fetch or push for publication",
                ));
            }
            mapping_commands::execute(&workspace, &data_home, actor, command)
        }
        Operation::Init { .. } => unreachable!(),
        Operation::Status => Ok(
            json!({"state_path":workspace.state_path,"workspace":workspace.state()?, "document_store_state_path": workspace.document_store_state_path()?, "knowledge":workspace.knowledge_status()?}),
        ),
        Operation::Commit {
            id,
            if_match,
            mapping,
        } => {
            if *mapping {
                Ok(serde_json::to_value(
                    workspace.commit_mapping_knowledge(&id.parse()?, if_match)?,
                )?)
            } else {
                Ok(serde_json::to_value(
                    workspace.commit_document(&id.parse()?, if_match)?,
                )?)
            }
        }
        Operation::Checkout { target, create } => {
            if let Ok(id) = target.parse::<DocumentId>() {
                if *create {
                    return Err(Error::invalid("--create applies to knowledge branches"));
                }
                Ok(serde_json::to_value(workspace.checkout_document(&id)?)?)
            } else {
                Ok(serde_json::to_value(
                    workspace.checkout_knowledge_branch(target, *create)?,
                )?)
            }
        }
        Operation::Worktree => workspace.knowledge_worktree_status(),
        Operation::Proposal { remote, command } => {
            if let Some(remote) = remote {
                return workspace.remote_knowledge_proposal(
                    &data_home,
                    remote,
                    match command {
                        ProposalOperation::List => None,
                        ProposalOperation::Show { id } => Some((id, None)),
                        ProposalOperation::Integrate { id, if_match } => Some((id, Some(if_match))),
                    },
                );
            }
            match command {
                ProposalOperation::List => {
                    Ok(serde_json::json!({"proposals":workspace.knowledge_proposals()?}))
                }
                ProposalOperation::Show { id } => workspace.knowledge_proposal(id),
                ProposalOperation::Integrate { id, if_match } => Ok(serde_json::to_value(
                    workspace.integrate_knowledge(id, if_match)?,
                )?),
            }
        }
        Operation::ResolveConflict {
            id,
            if_match,
            if_shared,
            take,
        } => {
            let choice = match take {
                DocumentChoice::Local => lore_local::ConflictResolutionChoice::Local,
                DocumentChoice::Shared => lore_local::ConflictResolutionChoice::Shared,
            };
            Ok(serde_json::to_value(workspace.resolve_document_conflict(
                id, if_match, if_shared, choice, actor,
            )?)?)
        }
        Operation::Create {
            title,
            body_file,
            stdin,
            directory,
        } => {
            let mut document = Document::new(
                title.clone(),
                body(body_file.as_ref(), *stdin)?.unwrap_or_default(),
                actor,
            )?;
            document.directory = directory.clone().unwrap_or_default();
            Ok(serde_json::to_value(workspace.create(document, actor)?)?)
        }
        Operation::Attach {
            id,
            location,
            if_match,
        } => {
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
            let path = workspace.code.relative(std::path::Path::new(path))?;
            let anchor = workspace.code.capture(&path, start, end)?;
            let observed = workspace.read(id, None)?;
            let expected = if_match.as_ref().unwrap_or(&observed.revision);
            Ok(serde_json::to_value(workspace.update(
                id,
                expected,
                actor,
                |d| {
                    d.anchors.push(anchor);
                    Ok(())
                },
            )?)?)
        }
        Operation::Search {
            query,
            include_archived,
            exclude_agent,
            directory,
        } => Ok(serde_json::to_value(workspace.search(
            query,
            &SearchOptions {
                include_archived: *include_archived,
                exclude_agent: *exclude_agent,
                directory: directory.clone(),
            },
        )?)?),
        Operation::Ls {
            path,
            recursive,
            include_archived,
        } => {
            let documents = if let Some(path) = path {
                let requested = scope(&workspace, path)?;
                workspace.locate(
                    &requested,
                    *recursive || workspace.code.root.join(path).is_dir(),
                    *include_archived,
                    &TextResolver,
                )?
            } else {
                workspace
                    .list(*include_archived)?
                    .into_iter()
                    .map(|view| lore_local::LocatedDocument {
                        view,
                        projections: vec![],
                    })
                    .collect()
            };
            Ok(json!({
                "documents": documents,
                "coverage": "workspace and committed local documents",
                "resolver": "exact text with Git rename discovery",
            }))
        }
        Operation::Show {
            id,
            revision,
            remote,
            committed,
            ..
        } => Ok(serde_json::to_value(if let Some(name) = remote {
            workspace.read_remote_document(&data_home, name, id)?
        } else if *committed {
            workspace.read_committed_document(id, revision.as_ref())?
        } else {
            workspace.read(id, revision.as_ref())?
        })?),
        Operation::Edit {
            id,
            if_match,
            body_file,
            stdin,
            title,
            directory,
        } => {
            let content = body(body_file.as_ref(), *stdin)?;
            if content.is_none() && title.is_none() && directory.is_none() {
                return Err(Error::invalid(
                    "Select content, title, or directory to edit",
                ));
            }
            Ok(serde_json::to_value(workspace.update(
                id,
                if_match,
                actor,
                |d| {
                    if let Some(content) = content {
                        d.content = content;
                    }
                    if let Some(title) = title {
                        d.title = title.clone();
                    }
                    if let Some(directory) = directory {
                        d.directory = directory.clone();
                    }
                    Ok(())
                },
            )?)?)
        }
        Operation::Archive { id, if_match } | Operation::Restore { id, if_match } => {
            let archived = matches!(cli.command, Operation::Archive { .. });
            Ok(serde_json::to_value(workspace.update(
                id,
                if_match,
                actor,
                |d| {
                    d.archived = archived;
                    Ok(())
                },
            )?)?)
        }
        Operation::History { id } => Ok(json!(
            workspace
                .history(id)?
                .into_iter()
                .map(|(revision, value)| json!({"revision":revision,"value":value}))
                .collect::<Vec<_>>()
        )),
        Operation::Links { id, revision } => workspace.connections(id, revision.as_ref()),
        Operation::Resolve { id } => {
            let document = workspace.read(id, None)?;
            let projections = document
                .document
                .anchors
                .iter()
                .map(|a| TextResolver.resolve(a, &workspace.code, None))
                .collect::<Result<Vec<_>>>()?;
            Ok(json!({"view":document,"projections":projections}))
        }
        Operation::Install {
            id,
            from,
            path,
            all,
            revision,
        } => {
            if id.is_none() && path.is_none() && !all {
                return Err(Error::invalid(
                    "Select a document, --path, or explicitly --all",
                ));
            }
            let source = Workspace::open(&data_home, from)?;
            if source.code.repository_id != workspace.code.repository_id {
                return Err(Error::invalid(
                    "Source and target repository identities differ",
                ));
            }
            let mut selected = if let Some(id) = id {
                vec![source.read(id, revision.as_ref())?]
            } else {
                source.list(false)?
            };
            if let Some(path) = path {
                let requested = scope(&workspace, path)?;
                let recursive = workspace.code.root.join(path).is_dir();
                let mut scoped = vec![];
                for view in selected {
                    let mut matches = false;
                    for anchor in &view.document.anchors {
                        let projection =
                            TextResolver.resolve(anchor, &workspace.code, Some(&requested))?;
                        let in_scope = |p: &str| {
                            requested.is_empty()
                                || p == requested
                                || recursive && p.starts_with(&format!("{requested}/"))
                        };
                        matches |= in_scope(&anchor.path)
                            || projection.possible_locations().any(|l| in_scope(&l.path));
                    }
                    if matches {
                        scoped.push(view);
                    }
                }
                selected = scoped;
            }
            let mut results = vec![];
            for view in selected {
                let installed = (|| -> Result<_> {
                    let mut next = Some(view.revision.clone());
                    while let Some(revision) = next {
                        let value = source.repository.load(&view.document.id, &revision)?;
                        workspace.repository.record(&value)?;
                        next = value.parent;
                    }
                    workspace.activate(&view.document.id, &view.revision)
                })();
                results.push(match installed {
                    Ok(view) => json!({"status":"installed","view":view}),
                    Err(e) => json!({
                        "status": "failed",
                        "id": view.document.id,
                        "error": {"kind": e.kind, "message": e.message},
                    }),
                });
            }
            Ok(json!({"results":results}))
        }
    }
}

fn print_human(value: &Value, command: &Operation) {
    match command {
        Operation::Context { .. } => context_commands::print(value),
        Operation::Docs { command } => backend_commands::print(value, command),
        Operation::Map { command, .. } => mapping_commands::print(value, command),
        Operation::Show { raw, .. } => {
            let d = &value["document"];
            if !raw {
                println!(
                    "{}\nID: {}\nRevision: {}\n{}\n",
                    d["title"].as_str().unwrap_or_default(),
                    d["id"].as_str().unwrap_or_default(),
                    value["revision"].as_str().unwrap_or_default(),
                    if d["archived"] == true {
                        "archived"
                    } else {
                        "active"
                    }
                );
            }
            print!("{}", d["content"].as_str().unwrap_or_default());
            if !raw {
                println!();
            }
        }
        Operation::Search { .. } => {
            for d in value.as_array().unwrap() {
                println!(
                    "{}  {}{}",
                    d["id"].as_str().unwrap(),
                    d["title"].as_str().unwrap(),
                    if d["archived"] == true {
                        " [archived]"
                    } else {
                        ""
                    }
                );
            }
        }
        Operation::Ls { .. } => {
            for item in value["documents"].as_array().unwrap() {
                let d = &item["view"]["document"];
                println!(
                    "{}  {}",
                    d["id"].as_str().unwrap(),
                    d["title"].as_str().unwrap()
                );
            }
        }
        _ if value.get("document").is_some() => println!(
            "{}  {}\nRevision: {}",
            value["document"]["id"].as_str().unwrap(),
            value["document"]["title"].as_str().unwrap(),
            value["revision"].as_str().unwrap()
        ),
        _ => println!("{}", serde_json::to_string_pretty(value).unwrap()),
    }
}

fn main() {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            let informational = matches!(
                error.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            );
            let json_requested = std::env::args()
                .take_while(|arg| arg != "--")
                .any(|arg| arg == "--json");
            if json_requested && !informational {
                eprintln!(
                    "{}",
                    json!({"error":{"kind":"invalid_input","message":error.to_string()}})
                );
                std::process::exit(2);
            }
            error.exit();
        }
    };
    match execute(&cli) {
        Ok(value) => {
            if cli.json {
                println!("{}", value);
            } else {
                print_human(&value, &cli.command);
            }
            if value["status"] == "conflict" {
                std::process::exit(3);
            }
            if value["results"]
                .as_array()
                .is_some_and(|r| r.iter().any(|v| v["status"] == "failed"))
            {
                std::process::exit(1);
            }
            if value["integration"] == "conflict"
                || value["results"].as_array().is_some_and(|results| {
                    results.iter().any(|result| {
                        result["status"] == "conflict" || result["status"] == "local_changed"
                    })
                })
            {
                std::process::exit(3);
            }
        }
        Err(error) => {
            if cli.json {
                eprintln!(
                    "{}",
                    json!({"error":{"kind":error.kind,"message":error.message}})
                );
            } else {
                eprintln!("Lore: {}", error.message);
            }
            std::process::exit(if error.kind == ErrorKind::Conflict {
                3
            } else {
                1
            });
        }
    }
}
