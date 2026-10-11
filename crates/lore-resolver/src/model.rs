use crate::{CodeFile, ProjectionInput, ProjectionProposer};
use lore_core::{
    Anchor, AnchorResolver, CodeContext, CodeRepository, Error, ErrorKind, Projection, Result,
    TargetFile, TextResolver, hash, validate_relative_path,
};
use std::collections::BTreeSet;

pub struct ModelResolver<P> {
    pub proposer: P,
    pub max_input_bytes: usize,
}

impl<P: ProjectionProposer> AnchorResolver for ModelResolver<P> {
    fn resolve(
        &self,
        anchor: &Anchor,
        code: &dyn CodeRepository,
        requested: Option<&str>,
    ) -> Result<Projection> {
        let mut result = TextResolver.resolve(anchor, code, requested)?;
        if !result.locations.is_empty() || anchor.context.repository != code.identity() {
            return Ok(result);
        }
        let paths: BTreeSet<_> = code.paths(anchor, requested)?.into_iter().collect();
        let mut input = ProjectionInput {
            source: anchor.clone(),
            target: Vec::new(),
        };
        let head = code.head();
        let mut size = anchor.snapshot.len();
        for path in &paths {
            validate_relative_path(path)?;
            let Some(content) = code.read(path)? else {
                continue;
            };
            size = size.saturating_add(content.len());
            if size > self.max_input_bytes {
                return Err(Error::invalid(
                    "Projection code exceeds max_input_bytes; narrow --path",
                ));
            }
            input.target.push(CodeFile {
                reference: TargetFile {
                    path: path.clone(),
                    context: CodeContext {
                        repository: code.identity().into(),
                        head: head.clone(),
                        working_tree: code.committed_content(path)?.as_deref() != Some(&content),
                        content_digest: hash(&content),
                    },
                },
                content,
            });
        }
        for previous in &result.target {
            if !input.target.iter().any(|file| {
                file.reference.path == previous.path
                    && file.reference.context.content_digest == previous.context.content_digest
            }) {
                return Err(Error::new(
                    ErrorKind::Conflict,
                    "Code changed while preparing model projection; resolve again",
                ));
            }
        }
        if input.target.is_empty() {
            return Ok(result);
        }
        let proposal = self.proposer.propose(&input)?;
        if proposal.explanation.trim().is_empty() {
            return Err(Error::new(
                ErrorKind::Corrupt,
                "Model proposal has no explanation",
            ));
        }
        let mut alternatives = result.alternatives;
        for candidate in proposal.candidates {
            if candidate.is_empty() {
                return Err(Error::new(
                    ErrorKind::Corrupt,
                    "Model proposal contains an empty candidate",
                ));
            }
            let mut locations = Vec::new();
            for proposed in candidate {
                let location = proposed.location;
                let file = input
                    .target
                    .iter()
                    .find(|file| file.reference.path == location.path)
                    .ok_or_else(|| {
                        Error::new(
                            ErrorKind::Corrupt,
                            "Model proposed an uncaptured target file",
                        )
                    })?;
                let lines: Vec<_> = file.content.lines().collect();
                if location.start == 0
                    || location.end < location.start
                    || location.end > lines.len()
                    || lines[location.start - 1..location.end].join("\n") != proposed.excerpt
                {
                    return Err(Error::new(
                        ErrorKind::Corrupt,
                        "Model location does not match captured code",
                    ));
                }
                locations.push(location);
            }
            let signature: BTreeSet<_> = locations
                .iter()
                .map(|location| (&location.path, location.start, location.end))
                .collect();
            if !alternatives.iter().any(|existing| {
                existing
                    .iter()
                    .map(|location| (&location.path, location.start, location.end))
                    .collect::<BTreeSet<_>>()
                    == signature
            }) {
                alternatives.push(locations);
            }
        }
        if code.head() != head
            || code
                .paths(anchor, requested)?
                .into_iter()
                .collect::<BTreeSet<_>>()
                != paths
        {
            return Err(Error::new(
                ErrorKind::Conflict,
                "Code changed during model projection; resolve again",
            ));
        }
        for file in &input.target {
            if code.read(&file.reference.path)?.as_deref() != Some(&file.content) {
                return Err(Error::new(
                    ErrorKind::Conflict,
                    "Code changed during model projection; resolve again",
                ));
            }
        }
        result.target = input
            .target
            .into_iter()
            .map(|file| file.reference)
            .collect();
        result.alternatives = alternatives;
        result.explanation = format!(
            "Model-assisted correspondence; candidates are unconfirmed and explanation validity was not assessed. {}",
            proposal.explanation
        );
        Ok(result)
    }
}
