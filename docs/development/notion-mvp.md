# Notion MVP

Browse backend-owned documents, read them beside code, and persist code connections independently of their content. No per-document import is required.

## Components

- `lore-core`: Document references, backend access and mapping storage ports; reusable code anchors and resolver port.
- `lore-notion`: Read-only Notion adapter; page metadata, paginated child blocks, nested containers, database data sources, and Markdown retrieval.
- `lore-local`: Backend configuration, explicit observed caches, title indexes, immutable mapping revisions, and worktree-specific activation.
- `lore`: CLI operations used by humans, editors, and agents.
- `editors/vscode`: Unified knowledge tree with native title filtering, sidebar Markdown reader, selected-document code locations, and CodeLens.
- `integrations/codex`: Optional executable lifecycle adapter, shared discovery refresh, and session introduction ledger.
- `examples/skills/lore-ko`: Optional reading and mapping guidance.

The previous Lore-owned document prototype is preserved. It is separate from external document access and independent mapping storage; no automatic migration occurs.

## Setup

```bash
cargo install --path crates/lore --locked
lore init
lore backend add-notion team --root <root-page-url-or-id>
```

For CLI use, set `NOTION_TOKEN` in the invoking process's environment. `--token-env` selects another variable; the configuration stores its name, never its value. Internal connection tokens, personal access tokens, and valid OAuth access tokens use the same bearer transport. OAuth authorization and refresh are not implemented.

An internal connection needs read-content capability and access to the root page. Access to children is inherited. A personal access token uses its creator's permissions, subject to workspace policy. Configured roots define discovery scope, not an additional authorization boundary.

For VS Code, install the local VSIX and set `lore.executable` if needed. Use **Lore: Set Notion Connection Token** once for the configured backend. CLI, editor, and hooks share the saved credential when using the same Lore home on the same machine. Existing editor-only credentials migrate automatically on activation or document access; migration failure preserves the old token. **Lore: Connect Notion** configures a new backend and prompts for its credential.

In a terminal, `lore backend auth set team --stdin` accepts a token through stdin. `lore backend auth status team` reports availability; `lore backend auth remove team` removes the saved token. The configured environment variable remains a process-specific override. See [Connection Credentials](../decisions.md#connection-credentials) for storage details.

The backend name is a stable document-reference namespace. Preserve it across credential rotation. Removing backend configuration preserves mappings and observed caches. Do not reuse the same name for a different source.

## Experience

```bash
lore docs browse team                 # Configured roots
lore docs browse team <page-id>       # Contained documents, without body import
lore docs show team <page-id>         # Cache first; retrieve a missing body once
lore docs fetch team                  # Refresh tree and titles, without bodies
lore docs fetch team <page-id>        # Refresh one body
lore docs fetch team --with-content   # Explicit bulk body cache for offline reading
lore docs search team scheduler       # Search cached titles
lore map create team <page-id> compiler/scheduler.py:100-150 --context 'Scheduler constraints'
lore map create team <page-id> --glob 'compiler/scheduler/**' --context 'Scheduler subsystem'
lore map ls compiler/scheduler.py
lore --json context discover compiler/scheduler.py
lore map resolve <mapping-id>
lore map show <mapping-id>
lore map edit <mapping-id> --if-match <revision> --context 'Updated explanation'
lore map history <mapping-id>
lore map publish <mapping-id> --if-match <revision>
lore map fetch
```

In VS Code, expand **Knowledge**, click a document to read it below the trees in **Document**, and follow Notion page links. **Reload from Notion** in the view title refreshes the current body. Use **Lore: Link Notion Document to Selection** from the code editor to create mappings. **Open in Notion** delegates document editing to the backend. CodeLens returns from code to the connected document. **Mappings** shows the selected document’s code locations and navigates directly to code. Mapping editing, history, and archive/restore remain CLI operations.

The current-file toggle uses saved code and includes recorded references and known projections. Cached title observations may label connections; missing cached titles fall back to stable page IDs. Dirty editor buffers must be saved before linking or projecting.

The Knowledge view's filter button toggles an inline input row beneath its title. Input continuously filters titles without Enter or an apply step; hiding the row retains its active query. Its query and regex mode remain active when focus moves to code or documents; emptying the input restores the tree. The `.*` toggle enables case-insensitive regular expressions. Nonmatching documents and empty groups disappear, and matches appear directly under their source. Invalid regex preserves the last valid results. Filtering uses local documents, browsed backend titles, and existing cached title indexes without contacting Notion.

Use `--json` for structured results. `--author agent` records agent mapping contributions without implying human confirmation. Mapping edits use `--if-match`; conflicts exit with status 3 and preserve the current revision.

Other worktrees start with separate active mappings, while published relationships remain discoverable through `context discover`. To activate mappings for local editing or projection, install by target code scope:

```bash
lore --repo <other-worktree> init
lore --repo <other-worktree> map install --from <source-worktree> --path compiler
```

Glob scopes appear beside precise locations in the selected document's Mappings view. They do not create line-level CodeLens or imply projected locations. See [Agent Context](agent-context.md) for explicit discovery, session deduplication, and the optional Codex hook.

## Limits

- Discovery is scoped to configured roots and accessible children. Nested block or database failures produce warnings; title indexes report incomplete traversal.
- CLI backend search covers cached titles, not full document bodies. Indexing can take time and is explicit. The editor filter covers locally known titles; it does not discover uncached backend documents.
- Reads use cached observations by default; a cache miss retrieves once. `--cached` prohibits retrieval, while `--refresh` or `docs fetch` forces it. Failed refreshes preserve previous observations and report failure. Caches are derivative snapshots, not Lore-owned document revisions. See [Document Cache and Fetch](../decisions.md#document-cache-and-fetch).
- Notion Markdown is previewed safely; unsupported native blocks, images, and partial content remain available in Notion. It is not a full Notion editor.
- Projection uses exact text and Git rename discovery. Large semantic changes may remain unresolved; correspondence does not validate document explanations.
- A local shared discovery catalog and read-only HTTP mapping-index client are available. No hosted shared server, remote Push/Pull, OAuth login, or model-based agent resolver is included yet.

## Verification

Rust integration tests exercise paginated discovery, nested pages and data sources, lazy bodies, partial content, failures, offline caches, mapping history/CAS, and worktree isolation with renamed paths. An isolated VS Code test uses a mock Notion API and real editor/webview to verify browsing unmapped pages, embedded reading, links, CodeLens, caching, document-scoped code locations, and navigation after renames. Actual workspace access requires separately supplied credentials.

Context tests cover shared and remote discovery without activation, background metadata refresh, automatic worktree preparation, local overlay isolation, stable document identity, concurrent introduction, output limits, agent/session isolation, archiving, and compatibility with existing immutable mapping revisions.
