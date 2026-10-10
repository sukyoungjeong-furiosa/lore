use crate::{HttpMappingRemote, RemoteRegistry, Workspace, atomic_write};
use lore_core::*;
use std::path::{Path, PathBuf};

impl Workspace {
    fn knowledge_remote(&self, home: &Path, name: &str) -> Result<(HttpMappingRemote, PathBuf)> {
        let remote = RemoteRegistry::new(home).get(name)?;
        let path = home.join("knowledge-remotes").join(hash(format!(
            "{}:{}",
            remote.store, self.code.repository_id
        )));
        Ok((HttpMappingRemote::new(home, name, remote)?, path))
    }

    pub(crate) fn cached_remote_knowledge(
        &self,
        home: &Path,
        name: &str,
    ) -> Result<Option<KnowledgeVersion>> {
        let (_, path) = self.knowledge_remote(home, name)?;
        match std::fs::read(path.join("fetched.json")) {
            Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn remote_version(
        &self,
        home: &Path,
        name: &str,
        version: Option<String>,
    ) -> Result<KnowledgeVersion> {
        let (client, _) = self.knowledge_remote(home, name)?;
        match client.request_knowledge(&KnowledgeRequest::Read {
            store: client.connection.store.clone(),
            repository: self.code.repository_id.clone(),
            version,
        })? {
            KnowledgeResponse::Version { version } => {
                version.snapshot.validate(&self.code.repository_id)?;
                Ok(version)
            }
            _ => Err(Error::invalid("Unexpected knowledge response")),
        }
    }

    pub fn read_remote_document(
        &self,
        home: &Path,
        name: &str,
        id: &DocumentId,
    ) -> Result<DocumentView> {
        let version = match self.cached_remote_knowledge(home, name)? {
            Some(version) => version,
            None => {
                self.fetch_remote_knowledge(home, name)?;
                self.cached_remote_knowledge(home, name)?.unwrap()
            }
        };
        let record = version
            .snapshot
            .documents
            .get(id)
            .ok_or_else(|| {
                Error::new(
                    ErrorKind::NotFound,
                    "Document is not in this Lore remote's fetched central knowledge",
                )
            })?
            .clone();
        Ok(DocumentView::new(record.id()?, record))
    }

    pub fn fetch_remote_knowledge(&self, home: &Path, name: &str) -> Result<serde_json::Value> {
        let version = self.remote_version(home, name, None)?;
        let (client, _) = self.knowledge_remote(home, name)?;
        self.connect_remote(&client.connection.store, name)?;
        let (_, path) = self.knowledge_remote(home, name)?;
        atomic_write(&path.join("fetched.json"), &serde_json::to_vec(&version)?)?;
        Ok(serde_json::json!({"remote":name,"knowledge":version}))
    }

    pub fn push_remote_knowledge(
        &self,
        home: &Path,
        name: &str,
        documents: &[DocumentId],
        mappings: &[MappingId],
        all: bool,
    ) -> Result<serde_json::Value> {
        if !all && documents.is_empty() && mappings.is_empty() {
            return Err(Error::invalid(
                "Select --document, --mapping-id, or --all for remote submission",
            ));
        }
        let (client, path) = self.knowledge_remote(home, name)?;
        let baseline: KnowledgeVersion =
            serde_json::from_slice(&std::fs::read(path.join("baseline.json")).map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    Error::new(
                        ErrorKind::NotInitialized,
                        "Pull from this Lore remote before submitting local central knowledge",
                    )
                } else {
                    e.into()
                }
            })?)?;
        let central = self
            .knowledge_history(&self.state()?)?
            .ok_or_else(|| Error::new(ErrorKind::NotInitialized, "Run lore init"))?
            .read_version("main")?;
        let proposed = if all {
            central.snapshot
        } else {
            let mut proposed = baseline.snapshot.clone();
            for id in documents {
                proposed.documents.insert(
                    id.clone(),
                    central
                        .snapshot
                        .documents
                        .get(id)
                        .ok_or_else(|| {
                            Error::new(
                                ErrorKind::NotFound,
                                "Selected document is not in local central knowledge",
                            )
                        })?
                        .clone(),
                );
            }
            for id in mappings {
                proposed.mappings.insert(
                    id.clone(),
                    central
                        .snapshot
                        .mappings
                        .get(id)
                        .ok_or_else(|| {
                            Error::new(
                                ErrorKind::NotFound,
                                "Selected mapping is not in local central knowledge",
                            )
                        })?
                        .clone(),
                );
            }
            proposed
        };
        let operation_id = hash(serde_json::to_vec(&(baseline.version.as_str(), &proposed))?);
        let request = KnowledgeRequest::Submit {
            store: client.connection.store.clone(),
            repository: self.code.repository_id.clone(),
            operation_id,
            base: baseline.version,
            snapshot: proposed,
        };
        match client.request_knowledge(&request)? {
            KnowledgeResponse::Proposal { proposal } => {
                Ok(serde_json::json!({"status":"submitted","remote":name,"proposal":proposal}))
            }
            _ => Err(Error::invalid("Unexpected knowledge submission response")),
        }
    }

    pub fn pull_remote_knowledge(&self, home: &Path, name: &str) -> Result<serde_json::Value> {
        let (_, path) = self.knowledge_remote(home, name)?;
        std::fs::create_dir_all(&path)?;
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path.join("sync.lock"))?;
        lock.lock()?;
        let version = self.remote_version(home, name, None)?;
        atomic_write(&path.join("fetched.json"), &serde_json::to_vec(&version)?)?;
        let base = match std::fs::read(path.join("baseline.json")) {
            Ok(bytes) => serde_json::from_slice::<KnowledgeVersion>(&bytes)?.snapshot,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => KnowledgeSnapshot::default(),
            Err(e) => return Err(e.into()),
        };
        let history = self
            .knowledge_history(&self.state()?)?
            .ok_or_else(|| Error::new(ErrorKind::NotInitialized, "Run lore init"))?;
        let central = history.read_version("main")?;
        let merged = merge_knowledge(&base, &central.snapshot, &version.snapshot)?;
        let operation = hash(format!(
            "pull:{name}:{}:{}",
            version.version, central.version
        ));
        let proposal = history.submit(&operation, &central.version, &merged)?;
        let incorporated = history.integrate(&proposal.id, &central.version)?;
        atomic_write(&path.join("baseline.json"), &serde_json::to_vec(&version)?)?;
        let (client, _) = self.knowledge_remote(home, name)?;
        self.connect_remote(&client.connection.store, name)?;
        Ok(
            serde_json::json!({"remote":name,"central":incorporated,"source_version":version.version}),
        )
    }

    pub fn remote_knowledge_proposal(
        &self,
        home: &Path,
        name: &str,
        action: Option<(&str, Option<&str>)>,
    ) -> Result<serde_json::Value> {
        let (client, _) = self.knowledge_remote(home, name)?;
        let store = client.connection.store.clone();
        let repository = self.code.repository_id.clone();
        match action {
            None => Ok(serde_json::to_value(client.request_knowledge(
                &KnowledgeRequest::Proposals { store, repository },
            )?)?),
            Some((id, Some(if_match))) => Ok(serde_json::to_value(client.request_knowledge(
                &KnowledgeRequest::Integrate {
                    store,
                    repository,
                    proposal: id.into(),
                    if_match: if_match.into(),
                },
            )?)?),
            Some((id, None)) => {
                let proposals = match client
                    .request_knowledge(&KnowledgeRequest::Proposals { store, repository })?
                {
                    KnowledgeResponse::Proposals { proposals } => proposals,
                    _ => return Err(Error::invalid("Unexpected proposal response")),
                };
                let proposal = proposals
                    .into_iter()
                    .find(|p| p.id == id)
                    .ok_or_else(|| Error::new(ErrorKind::NotFound, "Proposal does not exist"))?;
                Ok(
                    serde_json::json!({"base":self.remote_version(home,name,Some(proposal.base.clone()))?,"proposed":self.remote_version(home,name,Some(proposal.head.clone()))?,"central":self.remote_version(home,name,None)?,"proposal":proposal}),
                )
            }
        }
    }
}
