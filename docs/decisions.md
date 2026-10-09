# Design Decisions

## Distribution

- Initial distribution targets team members. The VS Code extension is distributed as a VSIX with explicit installation instructions, without publishing to the public Marketplace.
- CLI binaries and VSIX packages are provided through GitHub Releases. Access follows the repository's visibility and permissions.
- VSIX installation and updates are explicit initially. A private VS Code Marketplace is the intended later channel for automatic extension updates; its account requirements and Remote SSH installation flow must be verified before rollout.
- The release packaging target is a platform-specific VSIX containing a compatible Lore CLI. Standalone CLI installations remain separately managed.
- The optional Codex adapter and replaceable example skills are distributed through a Git-backed plugin marketplace. Plugin refresh and activation behavior must be verified with the supported host; unattended updates are not assumed.
- Updates preserve backend credentials, cached knowledge, and workspace state. These user data are stored outside release packages.

Release automation, CLI bundling, and the Git-backed plugin package are planned. Current development installs use a separately installed CLI and manually configured hooks.

## Connection Names

A connection has a stable identity used in document references and a separately configurable display name. Changing the display name does not change references, credentials, caches, or mappings.

## Connection Credentials

- Backend credentials are shared within one Lore home across CLI, editors, hooks, and worktrees. They are separate from document caches and synchronization state.
- A configured token environment variable overrides the saved credential for that process. Cache-only reads require neither.
- Credential commands report availability without returning tokens. Tokens are submitted through stdin, not command arguments.
- Saved credentials are bound to their backend identity and API endpoint. Changing discovery roots does not invalidate them. Removing a backend removes its saved credential.

The local implementation uses plaintext files under `LORE_HOME/credentials`, with owner-only directory and file permissions on Unix. It does not use the OS keychain. VS Code migrates its previous SecretStorage tokens after a successful shared save, preserving any existing shared credential.

## Document Discovery

- Pages are readable without enabling discovery, including configured entry pages.
- Child discovery is enabled explicitly for individual pages in Lore's backend configuration. It lists directly contained documents; discovery is not inherited by those documents.
- Links in document content support reading and navigation, not discovery traversal. A linked page can be enabled separately.
- Refresh updates entry-page titles and the enabled discovery lists. Ordinary child pages are listed by title and identity without retrieving their body or detailed metadata. Containers within the selected page and directly contained databases may require additional requests.
- Disabling discovery changes the active index scope without deleting cached bodies or code mappings. An index for different discovery settings is not presented as current.

## Document Cache and Fetch

Cached documents are observations of backend-owned content, not Lore-owned revisions.

- Reading uses a cached observation when available and retrieves missing content on demand. The document reader displays source modification time rather than retrieval time; incomplete content remains visible.
- Backend metadata preserves source modification time, page creator, and last editor when available. Creator and last editor are distinct from a complete authorship history. Missing names preserve source user IDs without preventing document access.
- Fetch explicitly refreshes a document's body, or the title index within its explicitly configured discovery scope. Fetching the tree does not download every body; bulk body retrieval is a separate choice.
- Failed refreshes remain distinguishable from success and do not discard the previous cached content. Batch retrieval reports failures per document.
- A metadata fetch with a maximum cache age may reuse a recent partial index, preserving its warnings and completeness status. Explicit refresh still requests a new observation. Concurrent refreshes waiting for the same backend share a newly completed result.
- Cancelled traversal preserves the last completed title index; per-page observations already cached may remain.
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
