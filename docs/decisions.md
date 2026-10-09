# Design Decisions

## Connection Credentials

- Backend credentials are shared within one Lore home across CLI, editors, hooks, and worktrees. They are separate from document caches and synchronization state.
- A configured token environment variable overrides the saved credential for that process. Cache-only reads require neither.
- Credential commands report availability without returning tokens. Tokens are submitted through stdin, not command arguments.
- Saved credentials are bound to their backend identity and API endpoint. Changing discovery roots does not invalidate them. Removing a backend removes its saved credential.

The local implementation uses plaintext files under `LORE_HOME/credentials`, with owner-only directory and file permissions on Unix. It does not use the OS keychain. VS Code migrates its previous SecretStorage tokens after a successful shared save, preserving any existing shared credential.

## Document Cache and Fetch

Cached documents are observations of backend-owned content, not Lore-owned revisions.

- Reading uses a cached observation when available and retrieves missing content on demand. Cache age and incomplete content remain visible.
- Fetch explicitly refreshes a document's body, or the tree and title index under a backend's configured roots. Fetching the tree does not download every body; bulk body retrieval is a separate choice.
- Failed refreshes remain distinguishable from success and do not discard the previous cached content. Batch retrieval reports failures per document.
- Cache-only access performs no backend requests. Missing content is distinguishable from an empty document or discovery result.
- Document retrieval does not modify mappings, local document edits, or synchronization baselines.

Backend document access is separate from [mapping synchronization](architecture/synchronization.md). Read-only backends such as Notion do not require a document Pull or Push operation.

## Agent Context Discovery

Within an agent session, each Lore document is automatically introduced at most once. Explicit discovery and reading are always available.

- Discovery includes a repository's shared Mapping index without requiring workspace installation. Active local mappings override shared records with the same identity; local changes remain isolated.
- Shared discovery metadata and document observations are cached across worktrees. Fetch updates observations without activating revisions or advancing local baselines. Pull incorporates selected revisions under conflict checks.
- Introduction lists document identities, cached titles, mapping IDs, and matching scopes or recorded anchor locations. The adapter schedules metadata refresh separately; tool-time introduction reads caches without body retrieval or projection.
- Backend bodies remain lazy by default. Full body caching is an explicit option rather than a prerequisite for discovery.
- Introduction is deduplicated by document identity across mappings and worktrees, not by revision or tool call. Compaction does not reset it when the host preserves session identity.
- Concurrent introductions claim documents before emission. Delivery is best-effort; an interrupted hook may miss an introduction, but explicit access remains available. Documents omitted by output limits are not claimed.
- Host adapters identify path mentions on a best-effort basis. A mention is neither proof of file access nor a guarantee that all relevant paths were discovered.

Session initialization prepares workspace state without installing mappings or resetting local changes. Locally created mappings enter the shared discovery catalog only through explicit publication.

The optional [Codex adapter](development/agent-context.md) supplies discovery data. The replaceable example skill guides reading and interpretation independently of that adapter.
