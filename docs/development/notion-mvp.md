# Notion MVP

Browse backend-owned documents, read them beside code, and persist code connections independently of their content. No per-document import is required.

## Components

- `lore-core`: Document references, backend access and mapping storage ports; reusable code anchors and resolver port.
- `lore-notion`: Read-only Notion adapter; page metadata, paginated child blocks, nested containers, database data sources, and Markdown retrieval.
- `lore-local`: Backend configuration, explicit observed caches, title indexes, immutable mapping revisions, and worktree-specific activation.
- `lore`: CLI operations used by humans, editors, and agents.
- `editors/vscode`: Unified knowledge tree, sidebar Markdown reader, selected-document code locations, CodeLens, and SecretStorage for local credentials.
- `examples/skills/lore-ko`: Optional agent workflow example.

The previous Lore-owned document prototype is preserved. It is separate from external document access and independent mapping storage; no automatic migration occurs.

## Setup

```bash
cargo install --path crates/lore --locked
lore init
lore backend add-notion team --root <root-page-url-or-id>
```

For CLI use, set `NOTION_TOKEN` in the invoking process's environment. `--token-env` selects another variable; the configuration stores its name, never its value. Internal connection tokens, personal access tokens, and valid OAuth access tokens use the same bearer transport. OAuth authorization and refresh are not implemented.

An internal connection needs read-content capability and access to the root page. Access to children is inherited. A personal access token uses its creator's permissions, subject to workspace policy. Configured roots define discovery scope, not an additional authorization boundary.

For VS Code, install the local VSIX and set `lore.executable` if needed. Use **Lore: Set Notion Connection Token** once for the configured backend. The value is stored in VS Code SecretStorage and provided only to document CLI calls. It is not automatically available to unrelated terminals or agents. **Lore: Connect Notion** configures a new backend and prompts for its credential.

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
lore map ls compiler/scheduler.py
lore map resolve <mapping-id>
lore map show <mapping-id>
lore map edit <mapping-id> --if-match <revision> --context 'Updated explanation'
lore map history <mapping-id>
```

In VS Code, expand **Knowledge**, click a document to read it below the trees in **Document**, and follow Notion page links. **Reload from Notion** in the view title refreshes the current body. Use **Lore: Link Notion Document to Selection** from the code editor to create mappings. **Open in Notion** delegates document editing to the backend. CodeLens returns from code to the connected document. **Mappings** shows the selected document’s code locations and navigates directly to code. Mapping editing, history, and archive/restore remain CLI operations.

The current-file toggle uses saved code and includes recorded references and known projections. Cached title observations may label connections; missing cached titles fall back to stable page IDs. Dirty editor buffers must be saved before linking or projecting.

Use `--json` for structured results. `--author agent` records agent mapping contributions without implying human confirmation. Mapping edits use `--if-match`; conflicts exit with status 3 and preserve the current revision.

Other worktrees start with separate active mappings. Install by target code scope:

```bash
lore --repo <other-worktree> init
lore --repo <other-worktree> map install --from <source-worktree> --path compiler
```

## Limits

- Discovery is scoped to configured roots and accessible children. Nested block or database failures produce warnings; title indexes report incomplete traversal.
- Search covers cached titles, not full document bodies. Indexing can take time and is explicit.
- Reads use cached observations by default; a cache miss retrieves once. `--cached` prohibits retrieval, while `--refresh` or `docs fetch` forces it. Failed refreshes preserve previous observations and report failure. Caches are derivative snapshots, not Lore-owned document revisions. See [Document Cache and Fetch](../decisions.md#document-cache-and-fetch).
- Notion Markdown is previewed safely; unsupported native blocks, images, and partial content remain available in Notion. It is not a full Notion editor.
- Projection uses exact text and Git rename discovery. Large semantic changes may remain unresolved; correspondence does not validate document explanations.
- No shared server, Push/Pull, OAuth login, or model-based agent resolver is included yet.

## Verification

Rust integration tests exercise paginated discovery, nested pages and data sources, lazy bodies, partial content, failures, offline caches, mapping history/CAS, and worktree isolation with renamed paths. An isolated VS Code test uses a mock Notion API and real editor/webview to verify browsing unmapped pages, embedded reading, links, CodeLens, caching, document-scoped code locations, and navigation after renames. Actual workspace access requires separately supplied credentials.
