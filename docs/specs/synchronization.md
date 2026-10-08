# Synchronization

Synchronization exchanges document revisions between a workspace and a shared
source through Fetch, Pull, and Push.

Published revisions are immutable; updates create new revisions.

## Fetch

- Fetch retrieves document revisions without changing the workspace's active document revision, local edits, or synchronization baseline.
- Linked documents and historical revisions can be fetched on demand.

## Pull

- When a document has no local changes, Pull activates the selected shared revision.
- Local-only changes are preserved. If local and shared states both changed from the baseline, Pull reports a conflict rather than resolving it silently.
- Successful incorporation advances the baseline; unresolved conflicts do not.

## Push

- Push captures and publishes one document's recorded workspace state, including content, metadata, and anchors. Unrecorded client edits are excluded.
- Publication requires the shared revision to match the workspace's baseline, or the identity to be unused for a new document.
- Conflicts preserve local changes.
- Successful publication identifies the final published revision, including any reconciled code references.

## Server Reconciliation

- The server attempts to reconcile submitted anchors with the configured canonical ref for their code repository, such as `upstream/master`.
- Reconciliation uses [code projection](code-projection.md). Confirmed correspondence is recorded against the canonical code revision while preserving the submitted reference.
- If correspondence cannot be established, the original reference is published with explicit uncertainty, without assuming a canonical location.
- Reconciliation does not modify the document's title, content, or curation status.

## Scope and Failure

- Pull can select documents or code files and directories using [workspace scope](workspace.md#knowledge-availability). Push targets one document.
- Referenced documents are not implicitly included. Pull results are reported per document.
- Applying synchronization results must not discard changes made during the operation.
- Retrying uncertain operations must not duplicate publications or lose local changes.

## Conflict Resolution

- Both versions and their shared baseline remain available for inspection.
- A human or agent can propose a resolution. Applying it is an explicit change, not a side effect of fetching.
- Application requires that the relevant local and shared states still match those used to prepare the resolution.
