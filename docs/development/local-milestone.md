# Local Milestone

The current prototype supports one person using Lore-owned documents through the CLI, VS Code, and an optional agent skill.

It predates the backend-and-mapping architecture: anchors are still stored inside document revisions. External knowledge backends and independent mapping storage are now implemented separately; see [Notion MVP](notion-mvp.md). The behavior below describes this prototype, rather than the revised target architecture.

## Components

| Component | Responsibility |
| --- | --- |
| `lore-core` | Document identities, immutable revisions, wiki links, code references, and repository/resolver contracts. |
| `lore-local` | File storage, Git code access, workspace isolation, and conditional edits. |
| `lore` | Human-readable commands and structured JSON access for clients and agents. |
| `editors/vscode` | Code navigation and embedded reading/editing through the CLI. |
| `examples/skills/lore-ko` | Replaceable Korean guidance for agents using Lore. |

Knowledge revisions are stored outside code checkouts. Each worktree maintains its own active revisions. Installation copies complete revisions and their history; it does not replace an existing different local version. File and directory selection uses recorded paths and correspondence in the target checkout.

The file-backed `KnowledgeRepository`, Git `CodeRepository`, and `AnchorResolver` are separate boundaries. A shared server and its synchronization operations remain future work; immutable storage alone does not implement server publication or conflict reconciliation.

## Behavior and Limits

- Documents have independent identity and can exist without code anchors. Directory paths organize documents in a tree.
- Wiki links use `[[Title]]` or `[[id:<document-id>|label]]`. Duplicate titles require selection; stable ID links survive renaming. Discovery and backlinks cover active local documents.
- Edit operations require the observed revision. Concurrent edits conflict without discarding drafts or recorded changes.
- Authorship records human and agent contributions independently of document organization and archive state.
- Search excludes archives by default; archived documents remain directly readable. An optional filter excludes documents with recorded agent contributions, including mixed authorship.
- The text resolver preserves unchanged files' recorded locations. Otherwise it searches exact excerpts using Git rename discovery and requested paths. Competing matches remain alternatives. Major code transformations may require a future agent resolver.
- Code references capture committed or uncommitted UTF-8 text on disk. VS Code does not project unsaved code buffers in this milestone.
- Markdown is the first supported editing/rendering format. The model carries a format identifier for future formats.
- CLI author records are self-declared local information, not authenticated identities.

## Validation

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd editors/vscode
npm ci
npm test
npm run package
```

On Linux without a display, run `xvfb-run -a npm test`. `LORE_VSCODE_EXECUTABLE` can point to an existing VS Code executable; otherwise the test runner downloads its pinned version. The extension test uses an isolated checkout, data store, user profile, and extension directory.
