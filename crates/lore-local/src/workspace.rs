use crate::{FileRepository, GitRepository, absolute, atomic_write, now_ms};
use lore_core::{
    Actor, AnchorResolver, Document, DocumentId, DocumentView, Error, ErrorKind,
    KnowledgeRepository, Projection, Result, Revision, RevisionId, hash, links, targets,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SharedBaseline {
    pub source: String,
    pub revision: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ActiveDocument {
    pub revision: RevisionId,
    pub shared_baseline: Option<SharedBaseline>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkspaceState {
    pub schema_version: u32,
    pub checkout: PathBuf,
    pub repository_id: String,
    pub store: PathBuf,
    pub documents: BTreeMap<DocumentId, ActiveDocument>,
    #[serde(default)]
    pub mappings: BTreeMap<lore_core::MappingId, ActiveDocument>,
}

pub struct Workspace {
    pub state_path: PathBuf,
    pub code: GitRepository,
    pub repository: Box<dyn KnowledgeRepository>,
    pub mapping_repository: Box<dyn lore_core::MappingRepository>,
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
    pub tags: Vec<String>,
    pub authorship: Vec<Actor>,
    pub curated: bool,
    pub archived: bool,
    pub snippet: String,
    pub score: usize,
}

#[derive(Default)]
pub struct SearchOptions {
    pub include_archived: bool,
    pub curated_only: bool,
    pub exclude_agent: bool,
    pub tag: Option<String>,
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
        let mut code = GitRepository::discover(checkout, identity.clone())?;
        let home = absolute(home)?;
        let existing_path = Self::state_path(&home, &code.root);
        let existing: Option<WorkspaceState> = if existing_path.exists() {
            Some(serde_json::from_slice(&std::fs::read(&existing_path)?)?)
        } else {
            None
        };
        if identity.is_none()
            && let Some(state) = &existing
        {
            code.repository_id = state.repository_id.clone();
        }
        let directory = match store {
            Some(path) => absolute(path)?,
            None => existing
                .as_ref()
                .map(|s| s.store.clone())
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
                checkout: code.root.clone(),
                repository_id: code.repository_id,
                store: directory,
                documents: BTreeMap::new(),
                mappings: BTreeMap::new(),
            };
            atomic_write(&state_path, &serde_json::to_vec_pretty(&state)?)?;
        }
        Self::open(&home, checkout)
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
        if state.schema_version != 1 || state.checkout != discovered.root {
            return Err(Error::new(
                ErrorKind::Unsupported,
                "Workspace schema or checkout does not match",
            ));
        }
        let code = GitRepository::discover(checkout, Some(state.repository_id.clone()))?;
        Ok(Self {
            state_path,
            code,
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
        let mut found = vec![];
        for (id, active) in state.documents {
            let revision = self.repository.load(&id, &active.revision)?;
            if !include_archived && revision.document.archived {
                continue;
            }
            found.push(DocumentView::new(active.revision, revision.document));
        }
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
        let active = state.documents.get(id).ok_or_else(|| {
            Error::new(
                ErrorKind::NotFound,
                "Document is not active in this workspace",
            )
        })?;
        let selected = revision.unwrap_or(&active.revision);
        let revision = self.repository.load(id, selected)?;
        Ok(DocumentView::new(selected.clone(), revision.document))
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
        Ok(DocumentView::new(id, document))
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
            return Ok(DocumentView::new(active.revision.clone(), document));
        }
        let revision = Revision {
            parent: Some(active.revision.clone()),
            actor,
            at_ms: now_ms(),
            document: document.clone(),
        };
        let revision_id = self.repository.record(&revision)?;
        active.revision = revision_id.clone();
        atomic_write(&self.state_path, &serde_json::to_vec_pretty(&state)?)?;
        Ok(DocumentView::new(revision_id, document))
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
        Ok(DocumentView::new(revision.clone(), value.document))
    }

    pub fn search(&self, query: &str, options: &SearchOptions) -> Result<Vec<SearchHit>> {
        let terms: Vec<_> = query.split_whitespace().map(str::to_lowercase).collect();
        let mut hits = vec![];
        for view in self.list(options.include_archived)? {
            let d = &view.document;
            if options.curated_only && !view.curated {
                continue;
            }
            if options.exclude_agent && d.authorship.contains(&Actor::Agent) {
                continue;
            }
            if options
                .tag
                .as_ref()
                .is_some_and(|tag| !d.tags.contains(tag))
            {
                continue;
            }
            let title = d.title.to_lowercase();
            let content = d.content.to_lowercase();
            let tags = d.tags.join(" ").to_lowercase();
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
                    && !tags.contains(t)
                    && !paths.contains(t)
            }) {
                continue;
            }
            let score = terms
                .iter()
                .map(|t| {
                    if title.contains(t) {
                        20
                    } else if tags.contains(t) {
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
                tags: d.tags.clone(),
                authorship: d.authorship.clone(),
                curated: view.curated,
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
