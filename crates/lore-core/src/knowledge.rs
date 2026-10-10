use crate::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Lore-owned content and code relationships in one code repository's knowledge branch.
/// Backend-owned document bodies, credentials, and projections are excluded.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeSnapshot {
    pub documents: BTreeMap<DocumentId, Revision>,
    pub mappings: BTreeMap<MappingId, MappingRevision>,
}

impl KnowledgeSnapshot {
    pub fn validate(&self, repository: &str) -> Result<()> {
        for (id, record) in &self.documents {
            record.document.validate()?;
            if *id != record.document.id {
                return Err(Error::invalid("Document key does not match its identity"));
            }
        }
        for (id, record) in &self.mappings {
            record.mapping.validate()?;
            if *id != record.mapping.id || record.mapping.code_repository()? != repository {
                return Err(Error::invalid(
                    "Mapping key or code repository does not match",
                ));
            }
        }
        Ok(())
    }
}

/// The version token is opaque outside its originating store.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KnowledgeVersion {
    pub version: String,
    pub snapshot: KnowledgeSnapshot,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KnowledgeProposal {
    pub id: String,
    pub base: String,
    pub head: String,
    pub source: String,
    pub integrated: Option<String>,
}

/// Storage and transport do not determine when a proposal is accepted.
pub trait KnowledgeStore: Send + Sync {
    fn read_version(&self, version: &str) -> Result<KnowledgeVersion>;
    fn submit(
        &self,
        operation: &str,
        base: &str,
        snapshot: &KnowledgeSnapshot,
    ) -> Result<KnowledgeProposal>;
    fn proposals(&self) -> Result<Vec<KnowledgeProposal>>;
    fn integrate(&self, proposal: &str, expected_central: &str) -> Result<KnowledgeVersion>;
}

/// A service selects storage independently of its client transport.
pub trait KnowledgeStorage: Send + Sync {
    fn open(&self, store: &KnowledgeStoreId, repository: &str) -> Result<Box<dyn KnowledgeStore>>;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum KnowledgeRequest {
    Read {
        store: KnowledgeStoreId,
        repository: String,
        version: Option<String>,
    },
    Submit {
        store: KnowledgeStoreId,
        repository: String,
        operation_id: String,
        base: String,
        snapshot: KnowledgeSnapshot,
    },
    Proposals {
        store: KnowledgeStoreId,
        repository: String,
    },
    Integrate {
        store: KnowledgeStoreId,
        repository: String,
        proposal: String,
        if_match: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum KnowledgeResponse {
    Version { version: KnowledgeVersion },
    Proposal { proposal: KnowledgeProposal },
    Proposals { proposals: Vec<KnowledgeProposal> },
}

fn same_document(a: &Revision, b: &Revision) -> bool {
    a.document == b.document && a.actor == b.actor && a.at_ms == b.at_ms
}

fn same_mapping(a: &MappingRevision, b: &MappingRevision) -> bool {
    a.mapping == b.mapping && a.actor == b.actor && a.at_ms == b.at_ms
}

/// Combines independent entity changes; concurrent changes to one entity require explicit resolution.
/// History tokens are not content changes: different stores may record different parent revisions.
pub fn merge_knowledge(
    base: &KnowledgeSnapshot,
    ours: &KnowledgeSnapshot,
    theirs: &KnowledgeSnapshot,
) -> Result<KnowledgeSnapshot> {
    fn merge<K: Ord + Clone + std::fmt::Display, V: Clone>(
        base: &BTreeMap<K, V>,
        ours: &BTreeMap<K, V>,
        theirs: &BTreeMap<K, V>,
        same: impl Fn(&V, &V) -> bool,
        kind: &str,
    ) -> Result<BTreeMap<K, V>> {
        let keys: BTreeSet<_> = base
            .keys()
            .chain(ours.keys())
            .chain(theirs.keys())
            .cloned()
            .collect();
        let equal = |a: Option<&V>, b: Option<&V>| match (a, b) {
            (Some(a), Some(b)) => same(a, b),
            (None, None) => true,
            _ => false,
        };
        let mut result = BTreeMap::new();
        for key in keys {
            let (b, o, t) = (base.get(&key), ours.get(&key), theirs.get(&key));
            let chosen = if equal(o, t) || equal(b, t) {
                o
            } else if equal(b, o) {
                t
            } else {
                return Err(Error::new(
                    ErrorKind::Conflict,
                    format!(
                        "Concurrent {kind} changes: {key}; inspect the proposal and both branches"
                    ),
                ));
            };
            if let Some(value) = chosen {
                result.insert(key, value.clone());
            }
        }
        Ok(result)
    }
    Ok(KnowledgeSnapshot {
        documents: merge(
            &base.documents,
            &ours.documents,
            &theirs.documents,
            same_document,
            "document",
        )?,
        mappings: merge(
            &base.mappings,
            &ours.mappings,
            &theirs.mappings,
            same_mapping,
            "mapping",
        )?,
    })
}

/// Reparents changed records in the destination's history without changing knowledge or anchors.
pub fn normalize_knowledge(
    previous: &KnowledgeSnapshot,
    mut next: KnowledgeSnapshot,
) -> Result<KnowledgeSnapshot> {
    for (id, record) in &mut next.documents {
        record.parent = match previous.documents.get(id) {
            Some(old) if same_document(old, record) => old.parent.clone(),
            Some(old) => Some(old.id()?),
            None => None,
        };
    }
    for (id, record) in &mut next.mappings {
        record.parent = match previous.mappings.get(id) {
            Some(old) if same_mapping(old, record) => old.parent.clone(),
            Some(old) => Some(old.id()?),
            None => None,
        };
    }
    Ok(next)
}
