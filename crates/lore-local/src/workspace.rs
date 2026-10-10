use crate::{FileRepository, GitRepository, absolute, atomic_write, now_ms};
use lore_core::{
    Actor, AnchorResolver, Document, DocumentId, DocumentView, Error, ErrorKind,
    KnowledgeRepository, Projection, Result, Revision, RevisionId, hash, links, targets,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharedBaseline {
    pub source: String,
    pub revision: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActiveDocument {
    pub revision: RevisionId,
    pub shared_baseline: Option<SharedBaseline>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkspaceState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub knowledge_worktree: Option<crate::KnowledgeWorktree>,
    pub schema_version: u32,
    pub checkout: PathBuf,
    pub repository_id: String,
    /// Immutable revision cache location, independent of the selected mapping stores.
    pub store: PathBuf,
    pub documents: BTreeMap<DocumentId, ActiveDocument>,
    #[serde(default)]
    pub mappings: BTreeMap<lore_core::MappingId, ActiveDocument>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub store_mappings:
        BTreeMap<lore_core::KnowledgeStoreId, BTreeMap<lore_core::MappingId, ActiveDocument>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub connected_remotes: BTreeMap<lore_core::KnowledgeStoreId, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_store: Option<crate::LocalStoreConnection>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub connected_local_stores: BTreeMap<lore_core::KnowledgeStoreId, PathBuf>,
}

#[derive(Serialize, Deserialize)]
struct CheckoutDefaults {
    repository_id: String,
    store: PathBuf,
    #[serde(default)]
    local_store: Option<crate::LocalStoreConnection>,
}

pub struct Workspace {
    pub central_view: bool,
    pub state_path: PathBuf,
    pub code: GitRepository,
    pub repository: Box<dyn KnowledgeRepository>,
    pub mapping_repository: Box<dyn lore_core::MappingRepository>,
    pub mapping_store: Option<lore_core::KnowledgeStoreId>,
}

#[derive(Clone, Debug, Serialize)]
pub struct LocatedDocument {
    pub view: DocumentView,
    pub projections: Vec<Projection>,
}

#[derive(Clone, Debug, Serialize)]
pub struct SearchHit {
    pub id: DocumentId,
    pub revision: RevisionId,
    pub title: String,
    pub directory: String,
    pub authorship: Vec<Actor>,
    pub archived: bool,
    pub snippet: String,
    pub score: usize,
}

#[derive(Default)]
pub struct SearchOptions {
    pub include_archived: bool,
    pub exclude_agent: bool,
    pub directory: Option<String>,
}

impl Workspace {
    fn state_path(home: &Path, root: &Path) -> PathBuf {
        home.join("workspaces")
            .join(hash(root.to_string_lossy().as_bytes()))
            .join("workspace.json")
    }

    pub fn init(
        home: &Path,
        checkout: &Path,
        store: Option<&Path>,
        identity: Option<String>,
    ) -> Result<Self> {
        Self::init_with_local_store(home, checkout, store, identity, None)
    }

    pub fn init_with_local_store(
        home: &Path,
        checkout: &Path,
        store: Option<&Path>,
        identity: Option<String>,
        local_store_name: Option<&str>,
    ) -> Result<Self> {
        let mut code = GitRepository::discover(checkout, identity.clone())?;
        let home = absolute(home)?;
        if home.starts_with(&code.root) || home.starts_with(&code.common_directory) {
            return Err(Error::invalid(
                "Lore data must be stored outside the code checkout and Git directory",
            ));
        }
        let defaults_directory = home
            .join("checkout-defaults")
            .join(hash(code.common_directory.to_string_lossy().as_bytes()));
        std::fs::create_dir_all(&defaults_directory)?;
        let defaults_lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(defaults_directory.join("defaults.lock"))?;
        defaults_lock.lock()?;
        let defaults_path = defaults_directory.join("settings.json");
        let defaults: Option<CheckoutDefaults> = if defaults_path.exists() {
            Some(serde_json::from_slice(&std::fs::read(&defaults_path)?)?)
        } else {
            None
        };
        let existing_path = Self::state_path(&home, &code.root);
        let existing: Option<WorkspaceState> = if existing_path.exists() {
            Some(serde_json::from_slice(&std::fs::read(&existing_path)?)?)
        } else {
            None
        };
        if identity.is_none() {
            if let Some(state) = &existing {
                code.repository_id = state.repository_id.clone();
            } else if let Some(defaults) = &defaults {
                code.repository_id = defaults.repository_id.clone();
            }
        }
        if code.repository_id.trim().is_empty() {
            return Err(Error::invalid("Repository identity must not be empty"));
        }
        let directory = match store {
            Some(path) => absolute(path)?,
            None => existing
                .as_ref()
                .map(|s| s.store.clone())
                .or_else(|| {
                    defaults
                        .as_ref()
                        .filter(|defaults| defaults.repository_id == code.repository_id)
                        .map(|defaults| defaults.store.clone())
                })
                .unwrap_or_else(|| home.join("repositories").join(hash(&code.repository_id))),
        };
        if home.starts_with(&code.root)
            || directory.starts_with(&code.root)
            || home.starts_with(&code.common_directory)
            || directory.starts_with(&code.common_directory)
        {
            return Err(Error::invalid(
                "Lore data must be stored outside the code checkout and Git directory",
            ));
        }
        let registry = crate::LocalStoreRegistry::new(&home);
        let local_store = if let Some(name) = local_store_name {
            registry.get(name)?
        } else if let Some(connection) = existing
            .as_ref()
            .and_then(|state| state.local_store.clone())
        {
            connection
        } else if let Some(connection) = defaults
            .as_ref()
            .and_then(|defaults| defaults.local_store.clone())
        {
            connection
        } else {
            registry.ensure_personal()?
        };
        let local_store =
            crate::LocalStoreConnection::open(&local_store.directory).and_then(|observed| {
                if observed.store != local_store.store {
                    Err(Error::new(
                        ErrorKind::Conflict,
                        "Local store identity changed; select a store explicitly",
                    ))
                } else {
                    Ok(observed)
                }
            })?;
        local_store.register(std::slice::from_ref(&code.repository_id), &[])?;
        let state_path = Self::state_path(&home, &code.root);
        std::fs::create_dir_all(state_path.parent().unwrap())?;
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(state_path.with_extension("lock"))?;
        lock.lock()?;
        if state_path.exists() {
            let state: WorkspaceState = serde_json::from_slice(&std::fs::read(&state_path)?)?;
            if state.repository_id != code.repository_id || state.store != directory {
                return Err(Error::new(
                    ErrorKind::Conflict,
                    "Workspace already initialized with different settings",
                ));
            }
        } else {
            let state = WorkspaceState {
                schema_version: 1,
                knowledge_worktree: None,
                checkout: code.root.clone(),
                repository_id: code.repository_id.clone(),
                store: directory,
                documents: BTreeMap::new(),
                mappings: BTreeMap::new(),
                store_mappings: BTreeMap::new(),
                connected_remotes: BTreeMap::new(),
                local_store: None,
                connected_local_stores: BTreeMap::new(),
            };
            atomic_write(&state_path, &serde_json::to_vec_pretty(&state)?)?;
        }
        let mut state: WorkspaceState = serde_json::from_slice(&std::fs::read(&state_path)?)?;
        if state.local_store.as_ref() != Some(&local_store)
            || state.connected_local_stores.get(&local_store.store) != Some(&local_store.directory)
        {
            state
                .connected_local_stores
                .insert(local_store.store.clone(), local_store.directory.clone());
            state.local_store = Some(local_store);
            atomic_write(&state_path, &serde_json::to_vec_pretty(&state)?)?;
        }
        let settings = CheckoutDefaults {
            repository_id: state.repository_id.clone(),
            store: state.store.clone(),
            local_store: state.local_store.clone(),
        };
        drop(lock);
        let workspace = Self::from_state(state_path, code, state)?;
        workspace.ensure_knowledge_worktree()?;
        if defaults.is_none() {
            atomic_write(&defaults_path, &serde_json::to_vec(&settings)?)?;
        }
        Ok(workspace)
    }

    pub fn open(home: &Path, checkout: &Path) -> Result<Self> {
        let discovered = GitRepository::discover(checkout, None)?;
        let state_path = Self::state_path(&absolute(home)?, &discovered.root);
        let bytes = std::fs::read(&state_path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                Error::new(
                    ErrorKind::NotInitialized,
                    "Run lore init in this checkout first",
                )
            } else {
                error.into()
            }
        })?;
        let state: WorkspaceState = serde_json::from_slice(&bytes)?;
        Self::from_state(state_path, discovered, state)
    }

    fn from_state(
        state_path: PathBuf,
        mut code: GitRepository,
        state: WorkspaceState,
    ) -> Result<Self> {
        if state.schema_version != 1 || state.checkout != code.root {
            return Err(Error::new(
                ErrorKind::Unsupported,
                "Workspace schema or checkout does not match",
            ));
        }
        if state.repository_id.trim().is_empty() {
            return Err(Error::invalid("Repository identity must not be empty"));
        }
        code.repository_id = state.repository_id;
        Ok(Self {
            state_path,
            central_view: false,
            code,
            mapping_store: None,
            mapping_repository: Box::new(crate::FileMappingRepository {
                directory: state.store.clone(),
            }),
            repository: Box::new(FileRepository {
                directory: state.store,
            }),
        })
    }

    pub fn state(&self) -> Result<WorkspaceState> {
        Ok(serde_json::from_slice(&std::fs::read(&self.state_path)?)?)
    }

    pub(crate) fn lock(&self) -> Result<std::fs::File> {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(self.state_path.with_extension("lock"))?;
        file.lock()?;
        Ok(file)
    }

    pub fn list(&self, include_archived: bool) -> Result<Vec<DocumentView>> {
        let state = self.state()?;
        let mut found: Vec<_> = self
            .available_documents(&state)?
            .into_iter()
            .filter(|view| include_archived || !view.document.archived)
            .collect();
        found.sort_by(|a, b| {
            a.document
                .title
                .to_lowercase()
                .cmp(&b.document.title.to_lowercase())
                .then(a.document.id.cmp(&b.document.id))
        });
        Ok(found)
    }

    pub fn read(&self, id: &DocumentId, revision: Option<&RevisionId>) -> Result<DocumentView> {
        let state = self.state()?;
        if self.central_view {
            return self.read_committed_document(id, revision);
        }
        let Some(active) = state.documents.get(id) else {
            return self.read_committed_document(id, revision);
        };
        let selected = revision.unwrap_or(&active.revision);
        let revision = self.repository.load(id, selected)?;
        Ok(DocumentView::new(selected.clone(), revision))
    }

    pub fn create(&self, document: Document, actor: Actor) -> Result<DocumentView> {
        let _lock = self.lock()?;
        let mut state = self.state()?;
        if state.documents.contains_key(&document.id) {
            return Err(Error::new(
                ErrorKind::Conflict,
                "Document identity already exists in this workspace",
            ));
        }
        let revision = Revision {
            parent: None,
            actor,
            at_ms: now_ms(),
            document: document.clone(),
        };
        let id = self.repository.record(&revision)?;
        state.documents.insert(
            document.id.clone(),
            ActiveDocument {
                revision: id.clone(),
                shared_baseline: None,
            },
        );
        atomic_write(&self.state_path, &serde_json::to_vec_pretty(&state)?)?;
        Ok(DocumentView::new(id, revision))
    }

    /// Applies an edit only while the active revision still matches the caller's observation.
    pub fn update(
        &self,
        id: &DocumentId,
        expected: &RevisionId,
        actor: Actor,
        edit: impl FnOnce(&mut Document) -> Result<()>,
    ) -> Result<DocumentView> {
        let _lock = self.lock()?;
        let mut state = self.state()?;
        self.prepare_document_edit(&mut state, id, expected)?;
        let active = state.documents.get_mut(id).ok_or_else(|| {
            Error::new(
                ErrorKind::NotFound,
                "Document is not active in this workspace",
            )
        })?;
        if active.revision != *expected {
            return Err(Error::new(
                ErrorKind::Conflict,
                "Document changed since it was read; reread before editing",
            ));
        }
        let previous = self.repository.load(id, &active.revision)?;
        let mut document = previous.document.clone();
        edit(&mut document)?;
        if document.id != *id {
            return Err(Error::invalid("An edit cannot change document identity"));
        }
        if document.content != previous.document.content
            || document.content_format != previous.document.content_format
        {
            document.record_content_author(actor);
        }
        document.validate()?;
        if document == previous.document {
            return Ok(DocumentView::new(active.revision.clone(), previous));
        }
        document.retire_legacy_metadata();
        let revision = Revision {
            parent: Some(active.revision.clone()),
            actor,
            at_ms: now_ms(),
            document: document.clone(),
        };
        let revision_id = self.repository.record(&revision)?;
        active.revision = revision_id.clone();
        atomic_write(&self.state_path, &serde_json::to_vec_pretty(&state)?)?;
        Ok(DocumentView::new(revision_id, revision))
    }

    pub fn history(&self, id: &DocumentId) -> Result<Vec<(RevisionId, Revision)>> {
        let view = self.read(id, None)?;
        let mut next = Some(view.revision);
        let mut history = vec![];
        while let Some(id_revision) = next {
            let revision = self.repository.load(id, &id_revision)?;
            next = revision.parent.clone();
            history.push((id_revision, revision));
        }
        Ok(history)
    }

    pub fn activate(&self, id: &DocumentId, revision: &RevisionId) -> Result<DocumentView> {
        let _lock = self.lock()?;
        let mut state = self.state()?;
        if let Some(active) = state.documents.get(id)
            && active.revision != *revision
        {
            return Err(Error::new(
                ErrorKind::Conflict,
                "An active local version already exists",
            ));
        }
        let value = self.repository.load(id, revision)?;
        state.documents.entry(id.clone()).or_insert(ActiveDocument {
            revision: revision.clone(),
            shared_baseline: None,
        });
        atomic_write(&self.state_path, &serde_json::to_vec_pretty(&state)?)?;
        Ok(DocumentView::new(revision.clone(), value))
    }

    pub fn search(&self, query: &str, options: &SearchOptions) -> Result<Vec<SearchHit>> {
        if let Some(directory) = &options.directory
            && !directory.is_empty()
        {
            lore_core::validate_relative_path(directory)?;
        }
        let terms: Vec<_> = query.split_whitespace().map(str::to_lowercase).collect();
        let mut hits = vec![];
        for view in self.list(options.include_archived)? {
            let d = &view.document;
            if options.exclude_agent && d.authorship.contains(&Actor::Agent) {
                continue;
            }
            if options.directory.as_ref().is_some_and(|directory| {
                !directory.is_empty()
                    && d.directory != *directory
                    && !d.directory.starts_with(&format!("{directory}/"))
            }) {
                continue;
            }
            let title = d.title.to_lowercase();
            let content = d.content.to_lowercase();
            let directory = d.directory.to_lowercase();
            let paths = d
                .anchors
                .iter()
                .map(|a| a.path.as_str())
                .collect::<Vec<_>>()
                .join(" ")
                .to_lowercase();
            if terms.iter().any(|t| {
                !title.contains(t)
                    && !content.contains(t)
                    && !directory.contains(t)
                    && !paths.contains(t)
            }) {
                continue;
            }
            let score = terms
                .iter()
                .map(|t| {
                    if title.contains(t) {
                        20
                    } else if directory.contains(t) {
                        8
                    } else if paths.contains(t) {
                        5
                    } else {
                        1
                    }
                })
                .sum();
            hits.push(SearchHit {
                id: d.id.clone(),
                revision: view.revision,
                title: d.title.clone(),
                directory: d.directory.clone(),
                authorship: d.authorship.clone(),
                archived: d.archived,
                snippet: d.content.chars().take(200).collect(),
                score,
            });
        }
        hits.sort_by(|a, b| {
            b.score
                .cmp(&a.score)
                .then(a.title.cmp(&b.title))
                .then(a.id.cmp(&b.id))
        });
        Ok(hits)
    }

    pub fn locate(
        &self,
        scope: &str,
        recursive: bool,
        include_archived: bool,
        resolver: &dyn AnchorResolver,
    ) -> Result<Vec<LocatedDocument>> {
        let mut found = vec![];
        let matches_scope = |path: &str| {
            scope.is_empty() || path == scope || recursive && path.starts_with(&format!("{scope}/"))
        };
        for view in self.list(include_archived)? {
            let mut projections = vec![];
            let mut relevant = false;
            for anchor in &view.document.anchors {
                let projection = resolver.resolve(anchor, &self.code, Some(scope))?;
                if anchor.context.repository == self.code.repository_id
                    && matches_scope(&anchor.path)
                    || projection
                        .possible_locations()
                        .any(|l| matches_scope(&l.path))
                {
                    relevant = true;
                }
                projections.push(projection);
            }
            if relevant {
                found.push(LocatedDocument { view, projections });
            }
        }
        Ok(found)
    }

    pub fn connections(
        &self,
        id: &DocumentId,
        revision: Option<&RevisionId>,
    ) -> Result<serde_json::Value> {
        let view = self.read(id, revision)?;
        let documents: Vec<_> = self.list(true)?.into_iter().map(|v| v.document).collect();
        let outgoing: Vec<_> = links(&view.document.content)
            .into_iter()
            .map(|link| {
                let resolved = targets(&link.target, &documents);
                serde_json::json!({
                    "target": link.target,
                    "label": link.label,
                    "matches": resolved.iter().map(|d| serde_json::json!({
                        "id": d.id,
                        "title": d.title,
                        "archived": d.archived,
                    })).collect::<Vec<_>>(),
                })
            })
            .collect();
        let backlinks: Vec<_> = documents
            .iter()
            .filter(|d| d.id != *id)
            .filter(|d| {
                links(&d.content).iter().any(|l| {
                    let resolved = targets(&l.target, &documents);
                    resolved.len() == 1 && resolved[0].id == *id
                })
            })
            .map(|d| serde_json::json!({"id": d.id, "title": d.title, "archived": d.archived}))
            .collect();
        Ok(serde_json::json!({"outgoing":outgoing,"backlinks":backlinks}))
    }
}
