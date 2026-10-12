# Synchronization

Lore distinguishes working commits, change proposals, and accepted central knowledge. These contracts apply from an agent workspace to a local central store and from a person's local central store to a Lore remote.

## Commit and Submission

- Commit records selected Lore-owned document or mapping changes on a working branch. It does not publish them to central knowledge.
- Direct mapping publication is retired; documents and mappings use the same submission and integration path.
- Push submits an immutable proposal with a fixed baseline and proposed state. Submission and integration are distinct outcomes.
- Local proposals use the working branch's common ancestor with central history. Remote proposals use the destination state last incorporated by Pull.
- Local Push may integrate a non-conflicting proposal immediately. Proposal-only submission is available; remote submission does not imply acceptance.
- Remote submission selects documents or mappings explicitly, or explicitly selects all accepted local knowledge. It does not implicitly publish pending workspace changes or copy external document bodies.
- Selected mappings require their Lore-owned documents in the destination or in the submission; missing dependencies are reported, not implicitly published.
- Repeating the same submission has one durable outcome. Reusing its operation identity with different content is rejected.

## Integration

- Integration conditionally updates the observed central version and publishes the whole proposal or nothing. Independent or identical entity changes can combine; divergent changes to one entity remain conflicts.
- Final integrated knowledge preserves mappings' Lore-owned document references, including archived documents.
- Conflicts preserve the proposal, its baseline, and central knowledge. Resolutions are explicit and conditional on the observed states.
- Integrated revisions are immutable. Repeated integration of an accepted proposal returns its original result even if central knowledge has advanced.
- `conflict_free` accepts non-conflicting proposals; `fast_forward_only` requires a new proposal's baseline to equal the current central version. Neither policy requires an agent.
- Stored proposals and integration results remain inspectable and reachable until explicitly removed under a retention policy.
- Applying results must not discard changes made while an operation was running.

## Fetch and Pull

- Fetch retrieves shared knowledge without changing a working branch, pending edits, or incorporation baselines.
- Pull incorporates accepted knowledge and advances its source baseline only after successful incorporation. Conflicting local and shared changes are not silently resolved.
- Remote Pull updates local central knowledge. Working branches incorporate those changes with their own Pull.
- An interrupted incorporation is recovered before newer source state is applied; successful integration must not be mistaken for an unapplied change because baseline publication was interrupted.
- Baselines are scoped to the local store, repository, and remote connection. Foreign version tokens or matching object IDs do not establish a common ancestor.
- External document fetch and editing follow the owning backend's contract independently of knowledge synchronization.

## Mapping Artifacts

- An artifact carries selected external-document mappings and complete source revision chains, independently of the transport used to distribute its file. Source stores remain identified; copying a mapping does not establish shared local knowledge history.
- Import validates the whole selection before activating working revisions. Repetition is idempotent. Updates require descendant history from the same source and local content unchanged since the preceding import, even when local commits normalize revision parents.
- Import baselines belong to the destination store and knowledge working branch. Changing branches neither advances nor discards another branch's baseline.
- Conflicts preserve working state. Import does not commit, integrate, retrieve external documents, or remove mappings omitted from the artifact. Archived revisions can be distributed explicitly.
- Artifacts preserve captured anchors and can contain full source files and historical explanatory context. Document bodies, credentials, workspace paths, and projections are excluded.

## Code Reconciliation

An optional worker may propose correspondence to a canonical code revision through [code projection](code-projection.md), preserving the original anchor identity and the actual target revision. This is separate from integration; unresolved correspondence or worker unavailability does not prevent submission or conflict-free integration. Code reconciliation neither rewrites document content nor resolves semantic contradictions between documents.
