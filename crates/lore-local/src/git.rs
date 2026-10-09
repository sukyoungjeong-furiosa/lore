use lore_core::{
    Anchor, CodeContext, CodeRepository, DocumentId, Error, ErrorKind, Result, hash,
    validate_relative_path,
};
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct GitRepository {
    pub root: PathBuf,
    pub common_directory: PathBuf,
    pub repository_id: String,
    pub commit: Option<String>,
}

impl GitRepository {
    pub fn discover(path: &Path, identity: Option<String>) -> Result<Self> {
        let raw = command(path, &["rev-parse", "--show-toplevel"])?;
        let root = PathBuf::from(raw.trim()).canonicalize()?;
        let common = command(&root, &["rev-parse", "--git-common-dir"])?;
        let common_path = PathBuf::from(common.trim());
        let common_directory = if common_path.is_absolute() {
            common_path
        } else {
            root.join(common_path)
        }
        .canonicalize()?;
        let commit = command(&root, &["rev-parse", "--verify", "HEAD"])
            .ok()
            .map(|s| s.trim().into());
        let discovered = ["upstream", "origin"].into_iter().find_map(|remote| {
            command(&root, &["remote", "get-url", remote])
                .ok()
                .map(|url| remote_identity(url.trim()))
        });
        let repository_id = identity
            .or(discovered)
            .unwrap_or_else(|| format!("git:{}", common_directory.display()));
        if repository_id.trim().is_empty() {
            return Err(Error::invalid("Repository identity must not be empty"));
        }
        Ok(Self {
            root,
            common_directory,
            repository_id,
            commit,
        })
    }

    pub fn relative(&self, path: &Path) -> Result<String> {
        let absolute = if path.is_absolute() {
            path.to_owned()
        } else {
            self.root.join(path)
        };
        let relative = absolute
            .strip_prefix(&self.root)
            .map_err(|_| Error::invalid("Path is outside the code checkout"))?;
        let value = relative
            .to_str()
            .ok_or_else(|| Error::invalid("Non-UTF-8 code paths are unsupported"))?
            .trim_end_matches('/')
            .to_string();
        validate_relative_path(&value)?;
        Ok(value)
    }

    pub fn capture(&self, path: &str, start: usize, end: usize) -> Result<Anchor> {
        validate_relative_path(path)?;
        let text = self.read(path)?.ok_or_else(|| {
            Error::new(
                ErrorKind::NotFound,
                "Code file is missing or not UTF-8 text",
            )
        })?;
        if start == 0 || end < start || end > text.lines().count() {
            return Err(Error::invalid("Anchor line range exceeds the code file"));
        }
        let context = CodeContext {
            repository: self.repository_id.clone(),
            head: self.commit.clone(),
            working_tree: self.committed_content(path)?.as_deref() != Some(&text),
            content_digest: hash(&text),
        };
        Ok(Anchor {
            id: DocumentId::new(),
            path: path.into(),
            start,
            end,
            context,
            snapshot: text,
        })
    }

    pub fn files(&self, scope: Option<&str>) -> Result<Vec<String>> {
        let mut args = vec![
            "--literal-pathspecs",
            "ls-files",
            "-co",
            "--exclude-standard",
            "-z",
            "--",
        ];
        if let Some(scope) = scope.filter(|scope| !scope.is_empty()) {
            validate_relative_path(scope)?;
            args.push(scope);
        }
        Ok(command(&self.root, &args)?
            .split('\0')
            .filter(|path| !path.is_empty())
            .map(str::to_owned)
            .collect())
    }
}

impl CodeRepository for GitRepository {
    fn identity(&self) -> &str {
        &self.repository_id
    }

    fn head(&self) -> Option<String> {
        self.commit.clone()
    }

    fn read(&self, path: &str) -> Result<Option<String>> {
        validate_relative_path(path)?;
        let joined = self.root.join(path);
        let canonical = match joined.canonicalize() {
            Ok(path) => path,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        if !canonical.starts_with(&self.root) || !canonical.is_file() {
            return Ok(None);
        }
        let bytes = std::fs::read(canonical)?;
        if bytes.contains(&0) {
            return Ok(None);
        }
        Ok(String::from_utf8(bytes).ok())
    }

    fn committed_content(&self, path: &str) -> Result<Option<String>> {
        validate_relative_path(path)?;
        let Some(commit) = &self.commit else {
            return Ok(None);
        };
        Ok(command(&self.root, &["show", &format!("{commit}:{path}")]).ok())
    }

    fn paths(&self, anchor: &Anchor, requested: Option<&str>) -> Result<Vec<String>> {
        let mut paths = std::collections::BTreeSet::new();
        if self.root.join(&anchor.path).is_file() {
            paths.insert(anchor.path.clone());
        }
        if let Some(commit) = &anchor.context.head
            && let Ok(changes) = command(
                &self.root,
                &["diff", "--name-status", "-z", "-M", commit, "--"],
            )
        {
            let fields: Vec<_> = changes.split('\0').collect();
            let mut index = 0;
            while index < fields.len() && !fields[index].is_empty() {
                let status = fields[index];
                index += 1;
                if status.starts_with('R') || status.starts_with('C') {
                    if index + 1 >= fields.len() {
                        break;
                    }
                    if fields[index] == anchor.path {
                        paths.insert(fields[index + 1].into());
                    }
                    index += 2;
                } else {
                    index += 1;
                }
            }
        }
        if requested.is_some() || paths.is_empty() {
            let mut args = vec![
                "--literal-pathspecs",
                "ls-files",
                "-co",
                "--exclude-standard",
                "-z",
                "--",
            ];
            if let Some(requested) = requested.filter(|path| !path.is_empty()) {
                args.push(requested);
            }
            for file in command(&self.root, &args)?
                .split('\0')
                .filter(|s| !s.is_empty())
            {
                paths.insert(file.into());
            }
            if let Some(requested) = requested
                && self.root.join(requested).is_file()
            {
                paths.insert(requested.into());
            }
        }
        Ok(paths.into_iter().collect())
    }
}

fn command(root: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()?;
    if !output.status.success() {
        return Err(Error::new(
            ErrorKind::Unavailable,
            String::from_utf8_lossy(&output.stderr).trim(),
        ));
    }
    String::from_utf8(output.stdout)
        .map_err(|_| Error::new(ErrorKind::Unsupported, "Git returned non-UTF-8 data"))
}

fn remote_identity(url: &str) -> String {
    let without_scheme = url.split_once("://").map(|(_, rest)| rest).unwrap_or(url);
    let without_user = without_scheme
        .rsplit_once('@')
        .map(|(_, rest)| rest)
        .unwrap_or(without_scheme);
    format!(
        "git:{}",
        without_user
            .trim_end_matches('/')
            .trim_end_matches(".git")
            .replacen(':', "/", 1)
    )
}
