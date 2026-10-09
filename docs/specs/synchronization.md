# Synchronization

Synchronization exchanges Lore-managed mapping revisions between a workspace and a shared source through Fetch, Pull, and Push.

Published mapping revisions are immutable; updates create new revisions. External document changes follow their owning backend's contract. These operations do not synchronize document content.

## Fetch

- Fetch retrieves mapping revisions without changing active mapping revisions, local edits, or synchronization baselines.
- Fetching a mapping does not implicitly fetch its document. Document retrieval does not advance a mapping baseline.

## Pull

- When a mapping has no local changes, Pull activates the selected shared revision.
- Local-only changes are preserved. If local and shared states both changed from the baseline, Pull reports a conflict rather than resolving it silently.
- Successful incorporation advances the baseline; unresolved conflicts do not.

## Push

- Push captures and publishes one mapping's recorded workspace state, including its document reference, anchors, and explanatory context. Unrecorded client edits are excluded.
- Publication requires the shared revision to match the workspace's baseline, or the identity to be unused for a new mapping.
- Conflicts preserve local changes. Successful publication identifies the final published revision, including any reconciled code references.
- Document retrieval failure alone does not prevent publication of a mapping that retains its document reference.

## Server Reconciliation

- The server attempts to reconcile submitted anchors with the configured canonical ref for their code repository, such as `upstream/master`.
- Reconciliation uses [code projection](code-projection.md). Confirmed correspondence is recorded against the canonical code revision while preserving the submitted reference.
- If correspondence cannot be established, the original reference is published with explicit uncertainty.
- Reconciliation does not modify the referenced document or establish human confirmation of the mapping.

## Scope and Failure

- Pull can select mappings by document reference or [workspace code scope](workspace.md#knowledge-availability). Push targets one mapping. Results are reported per mapping.
- Applying results must not discard changes made during the operation. Retrying uncertain operations must not duplicate publications or lose local changes.
- Conflicting versions and their shared baseline remain available for inspection. Humans or agents may propose resolutions; application is explicit and requires the relevant states to remain unchanged since preparation.

## Lore-Owned Documents

Document storage and synchronization are separate backend operations. A Lore document backend preserves revision history and must not silently overwrite concurrent edits. Publishing a mapping does not implicitly publish or synchronize its document.
