# Synchronization

Lore distinguishes working commits, change proposals, and accepted central knowledge. These contracts apply from an agent workspace to a local central store and from a person's local central store to a Lore remote.

## Commit and Submission

- Commit records selected Lore-owned document or mapping changes on a working branch. It does not publish them to central knowledge.
- Push submits committed knowledge as an inspectable proposal against an observed accepted baseline. Submission and integration are distinct outcomes.
- Local Push may integrate a non-conflicting proposal immediately. Proposal-only submission is available; remote submission does not imply acceptance.
- Remote submission selects documents or mappings explicitly, or explicitly selects all accepted local knowledge. It does not implicitly publish pending workspace changes or copy external document bodies.
- Repeating the same submission has one durable outcome. Reusing its operation identity with different content is rejected.

## Integration

- Integration checks the central version observed during preparation. Independent document and mapping changes can combine; concurrent changes to one entity remain conflicts.
- Conflicts preserve the proposal, its baseline, and central knowledge. Resolutions are explicit and conditional on the observed states.
- Accepted revisions are immutable. Integration identifies the actual accepted version; retries do not duplicate acceptance.
- Applying results must not discard changes made while an operation was running.

## Fetch and Pull

- Fetch retrieves shared knowledge without changing a working branch, pending edits, or incorporation baselines.
- Pull incorporates accepted knowledge and advances its source baseline only after successful incorporation. Conflicting local and shared changes are not silently resolved.
- Remote Pull updates local central knowledge. Working branches incorporate those changes with their own Pull.
- External document fetch and editing follow the owning backend's contract independently of knowledge synchronization.

## Code Reconciliation

Integration may associate submitted anchors with the configured canonical code revision through [code projection](code-projection.md). Original references are preserved. Unresolved correspondence does not prevent acceptance; model availability is not required. Reconciliation does not rewrite document content or establish its correctness.

## Storage and Transport

Commit, branch, proposal, and integration are Lore contracts. Git may implement history and worktrees; it does not require Git transport, GitHub, or a hosted PR. A Lore server can accept and integrate proposals through its own authenticated API.

Earlier mapping-only synchronization remains a compatibility interface. Its direct publication does not represent the proposal workflow.
