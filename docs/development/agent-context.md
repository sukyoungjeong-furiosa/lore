# Agent Context

Lore exposes discovery operations. The optional executable adapter in `integrations/codex` handles Codex events, initialization, refresh scheduling, and session deduplication.

## Shared Discovery and Local Work

```bash
lore map create team <page-id> --glob 'src/scheduler/**' --context 'Scheduler design'
lore map publish <mapping-id> --if-match <local-revision>
lore --repo <other-worktree> --json context discover src/scheduler/plan.rs
```

The default shared source is a local, repository-scoped catalog under `LORE_HOME`. Publication makes a mapping discoverable across worktrees without installing it. Creation and editing remain local; updates to a published mapping also require `--if-shared <observed-shared-revision>`. Publication does not publish document content. Backend configuration and observed document caches are global within the same Lore home.

The first successful `init` registers identity and storage defaults for the common Git directory. Fresh sibling worktrees inherit those defaults; existing workspaces retain their own settings and local state. Initially discovered identity comes from Git remotes or the common Git directory, and explicit `init` options can configure it. For pre-existing custom setups, run `init` in the configured checkout once before automatically preparing new siblings.

Discovery combines the shared catalog with active local mappings and local document anchors. Active mappings override shared records with the same ID, including local archive. Shared discovery does not activate mapping revisions, advance baselines, or overwrite local changes. Projection remains specific to current code and is requested separately.

Scopes use checkout-relative glob syntax: `*` does not cross directories; `**` can. Scopes match current paths without projection. Recorded anchor lines refer to their captured code state, not necessarily current locations. Results include document references, cached titles, matching scopes or anchor lines, and mapping IDs.

## Fetch and Pull

| Operation | Retrieves | Changes local working revisions or baselines? |
| --- | --- | --- |
| `docs fetch <backend>` | Remote tree and title metadata | No |
| `docs fetch <backend> <page-id>` | One document body | No |
| `docs fetch <backend> --with-content` | Tree and all available bodies | No |
| `map fetch` | Shared mapping discovery metadata | No |
| Pull | Incorporates selected mapping revisions | Yes; conflicts must preserve local changes |

Pull remains an architecture contract, not an implemented command. Read-only Notion documents use retrieval, not Lore mapping Pull. Bodies are retrieved lazily by `docs show`; bulk body caching is an explicit choice. `--max-age-seconds` allows metadata refreshes to reuse recent observations across worktrees.

## Remote Mapping Source

```bash
lore map source set https://example.org/mapping-index.json --token-env LORE_MAPPING_TOKEN
lore map fetch
lore --json context discover src/scheduler/plan.rs
lore --json context discover --cached src/scheduler/plan.rs
```

One HTTP(S) source can be configured per code repository, globally across worktrees. It replaces the default local catalog for discovery. The endpoint serves a `MappingCatalog` JSON object: `schema_version: 1`, the matching `repository` identity, and `mappings` keyed by mapping ID. Each entry contains a snapshot-free `MappingSummaryView` in `view` and an optional `title`. The `catalog` property returned by local `map fetch` has this format. It contains no document bodies or captured code snapshots.

The client validates repository identity and references, preserves the prior cache on failure, bounds responses to 8 MB, and supports an environment-provided bearer token. Explicit discovery retrieves a missing index once; `--refresh` forces retrieval. `--cached` performs no network requests and reports unavailable shared coverage through `shared_available` and `warnings`, alongside available local results. Cached observations expose their retrieval time; availability does not imply freshness or complete backend coverage.

This is a read-only index client, tested with a mock endpoint. A hosted Mapping server, remote publication, full revision retrieval, and remote Pull are not implemented. Use `map source remove` to return to the local shared catalog. Local `map ls`, `map show`, and `map resolve` still operate on active mappings; a discovered shared index entry is not automatically installed for editing or projection.

## Optional Codex Hook

Install the CLI with `cargo install --path crates/lore --locked`. In [hooks.json](../../integrations/codex/hooks.json), replace the adapter's placeholder with its absolute path and merge both event entries into the desired hooks configuration. Review and enable through `/hooks` in a host supporting local command hooks, following the [Codex hooks contract](https://learn.chatgpt.com/docs/hooks). This adapter requires Python 3 and POSIX file locking; it uses ordinary CLI commands, not a Lore hook subcommand.

- SessionStart runs idempotent `lore init` and schedules background discovery refresh. PostToolUse also prepares encountered worktrees, including worktrees created during the session.
- Background refresh retrieves mapping metadata and configured backends' title indexes. Scheduling is limited to once per repository per five minutes; fresh title indexes are reused for fifteen minutes. Results, including failure or partial traversal, are recorded under `LORE_HOME/integrations/codex/refresh`. Existing observations remain readable after failures. The first tool hook may run before initial remote discovery finishes.
- PostToolUse inspects structured path fields and lexical path mentions without parsing shell commands. It invokes `context discover --cached`, with no body retrieval, model calls, or projection calculation. A mention is not proof that a file entered model context.
- Extraction is bounded to a 1 MB event, 64 KiB of inspected text, 32 mentions, and four checkouts. Computed paths, unusual quoting, and wildcard-prefixed directory scopes may be missed.
- Session identity uses `session_id` plus `agent_id` when available. Hosts sharing a parent session without a distinct agent ID share its ledger. Transcript changes and worktree switches do not reset it. `--session` accepts an explicit stable identity.
- Introduction defaults to eight documents, at most three locations and mapping IDs each, with shortened titles and a byte budget. Documents omitted by limits remain eligible for later introduction. Concurrent calls claim documents before emission: at most once, with best-effort delivery after interruptions.

`LORE_EXECUTABLE` selects the CLI. `LORE_CODEX_REFRESH=0` disables automatic remote refresh while retaining cached discovery. CLI, editor, and workers use shared backend credentials in the same Lore home. A configured environment variable overrides the saved credential for that process. Explicit discovery and reading remain available regardless of the introduction ledger.

The ordinary discovery filter `--exclude-file <path>` accepts a JSON array of document references. The adapter uses it to avoid matching and reading titles for previously introduced documents; session policy remains in the adapter.

The optional [Korean example skill](../../examples/skills/lore-ko/SKILL.md) guides reading and interpretation independently of the hook. Neither is installed automatically. Adapter behavior, including background remote refresh, is tested with synthetic Codex events and mock APIs; live host context injection remains unverified.

An isolated release-build measurement with 300 published documents and no active mappings in the target worktree yielded median tool-hook times of 80 ms for eight new introductions and 67 ms when every match was already introduced (30 calls each). These include Python and CLI process startup and are observations on the development machine, not latency guarantees.
