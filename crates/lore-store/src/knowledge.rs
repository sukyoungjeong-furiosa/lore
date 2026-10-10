use crate::atomic;
use lore_core::*;
use serde::{Deserialize, Serialize};
use std::{
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

fn git_command() -> Command {
    let mut command = Command::new("git");
    for name in [
        "GIT_DIR",
        "GIT_COMMON_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    ] {
        command.env_remove(name);
    }
    command.env("GIT_TERMINAL_PROMPT", "0");
    command
}

/// Git implements local history only. Sharing uses KnowledgeStore proposals, not Git remotes.
pub struct GitKnowledgeStore {
    pub directory: PathBuf,
    pub repository: String,
}

#[derive(Serialize, Deserialize)]
struct StoredProposal {
    proposal: KnowledgeProposal,
    digest: String,
}

impl GitKnowledgeStore {
    pub fn git_directory(&self) -> PathBuf {
        self.directory.join("history.git")
    }

    pub fn central_state_path(&self) -> PathBuf {
        self.git_directory().join("refs/heads/main")
    }

    fn command(&self) -> Command {
        let mut command = git_command();
        command.arg("--git-dir").arg(self.git_directory()).args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "commit.gpgSign=false",
        ]);
        command
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_AUTHOR_NAME", "Lore")
            .env("GIT_AUTHOR_EMAIL", "lore@localhost")
            .env("GIT_COMMITTER_NAME", "Lore")
            .env("GIT_COMMITTER_EMAIL", "lore@localhost");
        command
    }

    fn output(&self, args: &[&str]) -> Result<String> {
        let output = self.command().args(args).output()?;
        if !output.status.success() {
            return Err(Error::new(
                ErrorKind::Unavailable,
                format!(
                    "Knowledge history operation failed: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ),
            ));
        }
        String::from_utf8(output.stdout)
            .map_err(|_| Error::new(ErrorKind::Corrupt, "Knowledge history is not UTF-8"))
    }

    fn input(&self, args: &[&str], bytes: &[u8]) -> Result<String> {
        let mut child = self
            .command()
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let mut stdin = child.stdin.take().unwrap();
        let write = stdin.write_all(bytes);
        drop(stdin);
        let output = child.wait_with_output()?;
        write?;
        if !output.status.success() {
            return Err(Error::new(
                ErrorKind::Unavailable,
                format!(
                    "Knowledge history operation failed: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ),
            ));
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    }

    fn lock(&self) -> Result<std::fs::File> {
        std::fs::create_dir_all(&self.directory)?;
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.directory.join("history.lock"))?;
        lock.lock()?;
        Ok(lock)
    }

    pub fn initialize(&self) -> Result<()> {
        let _lock = self.lock()?;
        if !self.git_directory().exists() {
            let output = git_command()
                .args(["init", "--bare", "--initial-branch=main", "--template="])
                .arg(self.git_directory())
                .output()?;
            if !output.status.success() {
                return Err(Error::new(
                    ErrorKind::Unavailable,
                    "Cannot initialize knowledge history",
                ));
            }
        }
        if self.resolve("main").is_err() {
            let initial =
                self.make_commit(&KnowledgeSnapshot::default(), &[], "Initialize knowledge")?;
            self.output(&[
                "update-ref",
                "refs/heads/main",
                &initial,
                &"0".repeat(initial.len()),
            ])?;
        }
        Ok(())
    }

    fn validate_name(name: &str) -> Result<()> {
        if name.is_empty()
            || name.len() > 200
            || name.starts_with('-')
            || name.contains("..")
            || name
                .split('/')
                .any(|p| p.is_empty() || p.starts_with('.') || p.ends_with('.'))
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"/-_.".contains(&b))
        {
            return Err(Error::invalid("Invalid knowledge branch or version"));
        }
        Ok(())
    }

    pub fn resolve(&self, name: &str) -> Result<String> {
        Self::validate_name(name)?;
        self.output(&["rev-parse", "--verify", &format!("{name}^{{commit}}")])
            .map(|v| v.trim().to_owned())
            .map_err(|_| {
                Error::new(
                    ErrorKind::NotFound,
                    "Knowledge branch or version does not exist",
                )
            })
    }

    fn read_at(&self, commit: &str) -> Result<KnowledgeSnapshot> {
        #[derive(Serialize, Deserialize)]
        struct Cached {
            digest: String,
            snapshot: KnowledgeSnapshot,
        }
        let path = self.directory.join("cache").join(format!("{commit}.json"));
        if let Ok(bytes) = std::fs::read(&path)
            && let Ok(cached) = serde_json::from_slice::<Cached>(&bytes)
            && hash(serde_json::to_vec(&cached.snapshot)?) == cached.digest
            && cached.snapshot.validate(&self.repository).is_ok()
        {
            return Ok(cached.snapshot);
        }
        let mut snapshot = KnowledgeSnapshot::default();
        for path in self
            .output(&["ls-tree", "-r", "--name-only", commit])?
            .lines()
        {
            if path.starts_with("documents/") && path.ends_with("/record.json") {
                let mut record: Revision =
                    serde_json::from_str(&self.output(&["show", &format!("{commit}:{path}")])?)?;
                let content_path = path.replace(
                    "record.json",
                    if record.document.content_format == "markdown" {
                        "content.md"
                    } else {
                        "content"
                    },
                );
                record.document.content =
                    self.output(&["show", &format!("{commit}:{content_path}")])?;
                if path != format!("documents/{}/record.json", record.document.id) {
                    return Err(Error::new(
                        ErrorKind::Corrupt,
                        "Document path does not match identity",
                    ));
                }
                snapshot
                    .documents
                    .insert(record.document.id.clone(), record);
            } else if path.starts_with("mappings/") && path.ends_with(".json") {
                let record: MappingRevision =
                    serde_json::from_str(&self.output(&["show", &format!("{commit}:{path}")])?)?;
                if path != format!("mappings/{}.json", record.mapping.id) {
                    return Err(Error::new(
                        ErrorKind::Corrupt,
                        "Mapping path does not match identity",
                    ));
                }
                snapshot.mappings.insert(record.mapping.id.clone(), record);
            }
        }
        snapshot.validate(&self.repository)?;
        atomic(
            &path,
            &serde_json::to_vec(&Cached {
                digest: hash(serde_json::to_vec(&snapshot)?),
                snapshot: snapshot.clone(),
            })?,
        )?;
        Ok(snapshot)
    }

    fn make_commit(
        &self,
        snapshot: &KnowledgeSnapshot,
        parents: &[&str],
        message: &str,
    ) -> Result<String> {
        snapshot.validate(&self.repository)?;
        let temporary = tempfile::tempdir_in(&self.directory)?;
        let index = temporary.path().join("index");
        let run_index = |args: &[&str], input: Option<&[u8]>| -> Result<String> {
            let mut command = self.command();
            command.env("GIT_INDEX_FILE", &index).args(args);
            let mut child = command
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()?;
            let mut stdin = child.stdin.take().unwrap();
            let result = if let Some(input) = input {
                stdin.write_all(input)
            } else {
                Ok(())
            };
            drop(stdin);
            let output = child.wait_with_output()?;
            result?;
            if !output.status.success() {
                return Err(Error::new(
                    ErrorKind::Unavailable,
                    String::from_utf8_lossy(&output.stderr).trim(),
                ));
            }
            Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
        };
        run_index(&["read-tree", "--empty"], None)?;
        let mut entries = Vec::new();
        let mut blob = |path: String, bytes: &[u8]| -> Result<()> {
            let object = self.input(&["hash-object", "-w", "--stdin"], bytes)?;
            entries.extend_from_slice(format!("100644 {object}\t{path}\0").as_bytes());
            Ok(())
        };
        for (id, record) in &snapshot.documents {
            let mut metadata = record.clone();
            metadata.document.content.clear();
            blob(
                format!("documents/{id}/record.json"),
                &serde_json::to_vec_pretty(&metadata)?,
            )?;
            blob(
                format!(
                    "documents/{id}/{}",
                    if record.document.content_format == "markdown" {
                        "content.md"
                    } else {
                        "content"
                    }
                ),
                record.document.content.as_bytes(),
            )?;
        }
        for (id, record) in &snapshot.mappings {
            blob(
                format!("mappings/{id}.json"),
                &serde_json::to_vec_pretty(record)?,
            )?;
        }
        run_index(&["update-index", "-z", "--index-info"], Some(&entries))?;
        let tree = run_index(&["write-tree"], None)?;
        let mut args = vec!["commit-tree", tree.as_str()];
        for parent in parents {
            args.extend(["-p", parent]);
        }
        self.input(&args, message.as_bytes())
    }

    pub fn create_worktree(&self, path: &Path, branch: &str) -> Result<()> {
        self.initialize()?;
        Self::validate_name(branch)?;
        let _lock = self.lock()?;
        if path.exists() {
            if self.verify_worktree(path, branch).is_err() {
                let link = std::fs::read_to_string(path.join(".git"))?;
                let old = PathBuf::from(link.trim().strip_prefix("gitdir: ").ok_or_else(|| {
                    Error::new(ErrorKind::Conflict, "Invalid knowledge worktree link")
                })?);
                let name = old
                    .file_name()
                    .ok_or_else(|| Error::invalid("Invalid worktree administration path"))?;
                let administration = self.git_directory().join("worktrees").join(name);
                let expected = std::fs::read_to_string(administration.join("gitdir"))?;
                if old.exists() || Path::new(expected.trim()) != path.join(".git") {
                    return Err(Error::new(
                        ErrorKind::Conflict,
                        "Knowledge worktree belongs to another store",
                    ));
                }
                self.output(&[
                    "worktree",
                    "repair",
                    path.to_str()
                        .ok_or_else(|| Error::invalid("Non UTF-8 worktree path"))?,
                ])?;
            }
            self.verify_worktree(path, branch)?;
            return Ok(());
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let target = self.resolve("main")?;
        if self.resolve(branch).is_err() {
            self.output(&[
                "update-ref",
                &format!("refs/heads/{branch}"),
                &target,
                &"0".repeat(target.len()),
            ])?;
        }
        self.output(&[
            "worktree",
            "add",
            path.to_str()
                .ok_or_else(|| Error::invalid("Non UTF-8 knowledge path"))?,
            branch,
        ])?;
        Ok(())
    }

    pub fn verify_worktree(&self, path: &Path, branch: &str) -> Result<()> {
        let output = git_command()
            .arg("-C")
            .arg(path)
            .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
            .output()?;
        if !output.status.success()
            || PathBuf::from(String::from_utf8_lossy(&output.stdout).trim()).canonicalize()?
                != self.git_directory().canonicalize()?
        {
            return Err(Error::new(
                ErrorKind::Conflict,
                "Knowledge worktree belongs to another store",
            ));
        }
        let current = git_command()
            .arg("-C")
            .arg(path)
            .args(["symbolic-ref", "--short", "HEAD"])
            .output()?;
        if !current.status.success() || String::from_utf8_lossy(&current.stdout).trim() != branch {
            return Err(Error::new(
                ErrorKind::Conflict,
                "Knowledge worktree branch changed outside Lore",
            ));
        }
        Ok(())
    }

    pub fn ensure_clean_worktree(&self, path: &Path, branch: &str) -> Result<()> {
        self.verify_worktree(path, branch)?;
        let output = git_command()
            .arg("-C")
            .arg(path)
            .args(["status", "--porcelain"])
            .output()?;
        if !output.status.success() || !output.stdout.is_empty() {
            return Err(Error::new(
                ErrorKind::Conflict,
                "Knowledge worktree has direct file changes; preserve and resolve them before Lore changes its branch",
            ));
        }
        Ok(())
    }

    pub fn refresh_worktree(&self, path: &Path) -> Result<()> {
        let output = git_command()
            .arg("-C")
            .arg(path)
            .args(["-c", "core.hooksPath=/dev/null", "reset", "--hard", "HEAD"])
            .output()?;
        if !output.status.success() {
            return Err(Error::new(
                ErrorKind::Unavailable,
                "Knowledge commit is durable, but refreshing the worktree failed",
            ));
        }
        Ok(())
    }

    pub fn commit_branch(
        &self,
        branch: &str,
        expected: &str,
        snapshot: KnowledgeSnapshot,
        message: &str,
    ) -> Result<KnowledgeVersion> {
        Self::validate_name(branch)?;
        if branch == "main" {
            return Err(Error::invalid(
                "Central knowledge accepts proposals; commit on a working branch",
            ));
        }
        let _lock = self.lock()?;
        let current = self.resolve(branch)?;
        if current != expected {
            return Err(Error::new(
                ErrorKind::Conflict,
                "Knowledge branch changed since it was read",
            ));
        }
        let snapshot = normalize_knowledge(&self.read_at(&current)?, snapshot)?;
        let commit = self.make_commit(&snapshot, &[&current], message)?;
        self.output(&[
            "update-ref",
            &format!("refs/heads/{branch}"),
            &commit,
            &current,
        ])?;
        Ok(KnowledgeVersion {
            version: commit,
            snapshot,
        })
    }

    fn proposal_path(&self, id: &str) -> Result<PathBuf> {
        Self::validate_name(id)?;
        if id.contains('/') {
            return Err(Error::invalid("Invalid proposal ID"));
        }
        Ok(self.directory.join("proposals").join(format!("{id}.json")))
    }

    fn load_proposal(&self, id: &str) -> Result<StoredProposal> {
        let bytes = std::fs::read(self.proposal_path(id)?).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Error::new(ErrorKind::NotFound, "Knowledge proposal does not exist")
            } else {
                e.into()
            }
        })?;
        let mut stored: StoredProposal = serde_json::from_slice(&bytes)?;
        if let Ok(accepted) = self.resolve(&format!("refs/lore/integrated/{id}")) {
            stored.proposal.integrated = Some(accepted);
        }
        Ok(stored)
    }

    fn record_proposal(
        &self,
        id: &str,
        base: &str,
        head: &str,
        source: &str,
        digest: String,
    ) -> Result<KnowledgeProposal> {
        let proposal = KnowledgeProposal {
            id: id.into(),
            base: base.into(),
            head: head.into(),
            source: source.into(),
            integrated: None,
        };
        self.output(&["update-ref", &format!("refs/lore/proposals/{id}"), head])?;
        atomic(
            &self.proposal_path(id)?,
            &serde_json::to_vec(&StoredProposal {
                proposal: proposal.clone(),
                digest,
            })?,
        )?;
        Ok(proposal)
    }

    fn common_base(&self, ours: &str, theirs: &str) -> Result<String> {
        let bases = self.output(&["merge-base", "--all", ours, theirs])?;
        let mut bases = bases.lines();
        let base = bases.next().ok_or_else(|| {
            Error::new(
                ErrorKind::Conflict,
                "Knowledge histories have no common baseline",
            )
        })?;
        if bases.next().is_some() {
            return Err(Error::new(
                ErrorKind::Conflict,
                "Knowledge histories have multiple common baselines; explicit resolution is required",
            ));
        }
        Ok(base.into())
    }

    pub fn submit_branch(&self, branch: &str) -> Result<KnowledgeProposal> {
        if branch == "main" {
            return Err(Error::invalid("Central knowledge is not its own proposal"));
        }
        let _lock = self.lock()?;
        let head = self.resolve(branch)?;
        let central = self.resolve("main")?;
        let base = self.common_base(&head, &central)?;
        let id = hash(format!("{branch}:{head}"));
        if self.proposal_path(&id)?.exists() {
            return Ok(self.load_proposal(&id)?.proposal);
        }
        self.record_proposal(&id, &base, &head, branch, hash(&head))
    }

    pub fn pull_branch(&self, branch: &str) -> Result<KnowledgeVersion> {
        let _lock = self.lock()?;
        let ours = self.resolve(branch)?;
        let theirs = self.resolve("main")?;
        let base = self.common_base(&ours, &theirs)?;
        let old = self.read_at(&ours)?;
        let merged = normalize_knowledge(
            &old,
            merge_knowledge(&self.read_at(&base)?, &old, &self.read_at(&theirs)?)?,
        )?;
        if base == theirs {
            return Ok(KnowledgeVersion {
                version: ours,
                snapshot: old,
            });
        }
        let commit = self.make_commit(&merged, &[&ours, &theirs], "Pull central knowledge")?;
        self.output(&[
            "update-ref",
            &format!("refs/heads/{branch}"),
            &commit,
            &ours,
        ])?;
        Ok(KnowledgeVersion {
            version: commit,
            snapshot: merged,
        })
    }

    pub fn pull_resolving_document(
        &self,
        branch: &str,
        expected_branch: &str,
        expected_central: &str,
        id: &DocumentId,
    ) -> Result<KnowledgeVersion> {
        self.pull_resolving(
            branch,
            expected_branch,
            expected_central,
            |working, central| {
                if let Some(record) = central.documents.get(id) {
                    working.documents.insert(id.clone(), record.clone());
                } else {
                    working.documents.remove(id);
                }
            },
        )
    }

    pub fn pull_resolving_mapping(
        &self,
        branch: &str,
        expected_branch: &str,
        expected_central: &str,
        id: &MappingId,
    ) -> Result<KnowledgeVersion> {
        self.pull_resolving(
            branch,
            expected_branch,
            expected_central,
            |working, central| {
                if let Some(record) = central.mappings.get(id) {
                    working.mappings.insert(id.clone(), record.clone());
                } else {
                    working.mappings.remove(id);
                }
            },
        )
    }

    fn pull_resolving(
        &self,
        branch: &str,
        expected_branch: &str,
        expected_central: &str,
        prepare: impl FnOnce(&mut KnowledgeSnapshot, &KnowledgeSnapshot),
    ) -> Result<KnowledgeVersion> {
        let _lock = self.lock()?;
        let ours = self.resolve(branch)?;
        let theirs = self.resolve("main")?;
        if ours != expected_branch || theirs != expected_central {
            return Err(Error::new(
                ErrorKind::Conflict,
                "Knowledge changed during conflict resolution",
            ));
        }
        let base = self.common_base(&ours, &theirs)?;
        let old = self.read_at(&ours)?;
        let central = self.read_at(&theirs)?;
        let mut resolving = old.clone();
        prepare(&mut resolving, &central);
        let merged = normalize_knowledge(
            &old,
            merge_knowledge(&self.read_at(&base)?, &resolving, &central)?,
        )?;
        let commit = self.make_commit(
            &merged,
            &[&ours, &theirs],
            "Resolve knowledge changes against central knowledge",
        )?;
        self.output(&[
            "update-ref",
            &format!("refs/heads/{branch}"),
            &commit,
            &ours,
        ])?;
        Ok(KnowledgeVersion {
            version: commit,
            snapshot: merged,
        })
    }

    pub fn checkout_worktree(&self, path: &Path, branch: &str, create: bool) -> Result<()> {
        if branch == "main" {
            return Err(Error::invalid(
                "Use --central to read central knowledge; working branches cannot check out main",
            ));
        }
        Self::validate_name(branch)?;
        let _lock = self.lock()?;
        let mut command = git_command();
        command
            .arg("-C")
            .arg(path)
            .args(["-c", "core.hooksPath=/dev/null", "checkout"]);
        if create {
            command.arg("-b");
        }
        command.arg(branch);
        let output = command.output()?;
        if !output.status.success() {
            return Err(Error::new(
                ErrorKind::Conflict,
                String::from_utf8_lossy(&output.stderr).trim(),
            ));
        }
        Ok(())
    }

    pub fn mapping_records(&self, branch: &str, id: &MappingId) -> Result<Vec<MappingRevision>> {
        let head = self.resolve(branch)?;
        let commits = self.output(&[
            "log",
            "--full-history",
            "--format=%H",
            &head,
            "--",
            &format!("mappings/{id}.json"),
        ])?;
        let mut records = std::collections::BTreeMap::new();
        for commit in commits.lines() {
            if let Some(record) = self.read_at(commit)?.mappings.remove(id) {
                records.insert(record.id()?.to_string(), record);
            }
        }
        let current = self.read_at(&head)?.mappings.remove(id).ok_or_else(|| {
            Error::new(
                ErrorKind::NotFound,
                "Mapping is not committed on this knowledge branch",
            )
        })?;
        let mut next = Some(current.id()?);
        let mut result = Vec::new();
        while let Some(revision) = next {
            if result.len() >= 10000 {
                return Err(Error::new(
                    ErrorKind::Corrupt,
                    "Mapping history exceeds its limit",
                ));
            }
            let record = records
                .get(revision.as_str())
                .ok_or_else(|| {
                    Error::new(
                        ErrorKind::Corrupt,
                        "Mapping ancestry is missing from committed history",
                    )
                })?
                .clone();
            next = record.parent.clone();
            result.push(record);
        }
        Ok(result)
    }

    pub fn document_bundle(
        &self,
        branch: &str,
        id: &DocumentId,
        selected: Option<&RevisionId>,
    ) -> Result<DocumentBundle> {
        let head = self.resolve(branch)?;
        let commits = self.output(&[
            "log",
            "--full-history",
            "--format=%H",
            &head,
            "--",
            &format!("documents/{id}/record.json"),
        ])?;
        let mut records = std::collections::BTreeMap::new();
        for commit in commits.lines() {
            if let Some(record) = self.read_at(commit)?.documents.remove(id) {
                records.insert(record.id()?.to_string(), record);
            }
        }
        let current = self.read_at(&head)?.documents.remove(id).ok_or_else(|| {
            Error::new(
                ErrorKind::NotFound,
                "Document is not committed on this knowledge branch",
            )
        })?;
        let mut next = Some(selected.cloned().unwrap_or(current.id()?));
        let mut revisions = vec![];
        while let Some(id) = next {
            let record = records
                .get(id.as_str())
                .ok_or_else(|| {
                    Error::new(
                        ErrorKind::NotFound,
                        "Document revision is not in this branch's history",
                    )
                })?
                .clone();
            if revisions.len() >= 10000 {
                return Err(Error::new(
                    ErrorKind::Corrupt,
                    "Document history exceeds its limit",
                ));
            }
            next = record.parent.clone();
            revisions.push(record);
        }
        let bundle = DocumentBundle { revisions };
        bundle.validate()?;
        Ok(bundle)
    }
}

impl KnowledgeStore for GitKnowledgeStore {
    fn read_version(&self, version: &str) -> Result<KnowledgeVersion> {
        let version = self.resolve(version)?;
        Ok(KnowledgeVersion {
            snapshot: self.read_at(&version)?,
            version,
        })
    }

    fn submit(
        &self,
        operation: &str,
        base: &str,
        snapshot: &KnowledgeSnapshot,
    ) -> Result<KnowledgeProposal> {
        snapshot.validate(&self.repository)?;
        let _lock = self.lock()?;
        let base = self.resolve(base)?;
        // Remote clients may propose only against accepted history, never another user's private branch.
        if !self
            .command()
            .args(["merge-base", "--is-ancestor", &base, "main"])
            .output()?
            .status
            .success()
        {
            return Err(Error::invalid(
                "Proposal baseline is not in central history",
            ));
        }
        let id = hash(operation);
        let digest = hash(serde_json::to_vec(&(base.as_str(), snapshot))?);
        if self.proposal_path(&id)?.exists() {
            let stored = self.load_proposal(&id)?;
            if stored.digest != digest {
                return Err(Error::new(
                    ErrorKind::Conflict,
                    "Proposal operation was reused with different content",
                ));
            }
            return Ok(stored.proposal);
        }
        let proposed = normalize_knowledge(&self.read_at(&base)?, snapshot.clone())?;
        let head = self.make_commit(&proposed, &[&base], "Submitted knowledge proposal")?;
        self.record_proposal(&id, &base, &head, "submitted", digest)
    }

    fn proposals(&self) -> Result<Vec<KnowledgeProposal>> {
        let mut proposals = vec![];
        let path = self.directory.join("proposals");
        if !path.exists() {
            return Ok(proposals);
        }
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            if let Some(id) = entry.path().file_stem().and_then(|v| v.to_str()) {
                proposals.push(self.load_proposal(id)?.proposal);
            }
        }
        proposals.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(proposals)
    }

    fn integrate(&self, proposal: &str, expected_central: &str) -> Result<KnowledgeVersion> {
        let _lock = self.lock()?;
        let stored = self.load_proposal(proposal)?;
        let proposal = &stored.proposal;
        if let Some(accepted) = &proposal.integrated {
            return self.read_version(accepted);
        }
        let central = self.resolve("main")?;
        if central != expected_central {
            return Err(Error::new(
                ErrorKind::Conflict,
                "Central knowledge changed since integration was prepared",
            ));
        }
        let previous = self.read_at(&central)?;
        let merged = normalize_knowledge(
            &previous,
            merge_knowledge(
                &self.read_at(&proposal.base)?,
                &previous,
                &self.read_at(&proposal.head)?,
            )?,
        )?;
        let commit = self.make_commit(
            &merged,
            &[&central, &proposal.head],
            "Integrate knowledge proposal",
        )?;
        // Central head and the retry receipt are one atomic reference transaction.
        self.input(&["update-ref","--stdin"],format!("start\nupdate refs/heads/main {commit} {central}\ncreate refs/lore/integrated/{} {commit}\nprepare\ncommit\n",proposal.id).as_bytes())?;
        Ok(KnowledgeVersion {
            version: commit,
            snapshot: merged,
        })
    }
}

pub struct GitKnowledgeStorage {
    pub directory: PathBuf,
}
impl KnowledgeStorage for GitKnowledgeStorage {
    fn open(&self, store: &KnowledgeStoreId, repository: &str) -> Result<Box<dyn KnowledgeStore>> {
        let history = GitKnowledgeStore {
            directory: self
                .directory
                .join(store.as_str())
                .join("knowledge")
                .join(hash(repository)),
            repository: repository.into(),
        };
        history.initialize()?;
        Ok(Box::new(history))
    }
}
