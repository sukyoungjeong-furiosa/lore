# Codex Adapter

Optional command hooks for preparing Lore workspaces and introducing related document references. Requires an installed `lore` CLI, Python 3, and POSIX file locking.

1. In [hooks.json](hooks.json), replace `/absolute/path/to/lore` with this checkout's absolute path.
2. Merge its SessionStart and PostToolUse entries into your host's hook configuration, then review and enable them through `/hooks` in a host supporting command hooks.
3. Optionally install the replaceable [Korean example skill](../../examples/skills/lore-ko/SKILL.md) using your agent's skill installation mechanism.

`LORE_EXECUTABLE` selects the CLI; `LORE_HOME` selects its data store. Backend connections and credentials are shared with the CLI and editor using that home. Saved tokens are plaintext files with owner-only Unix permissions; a configured token environment variable takes precedence.

SessionStart prepares the workspace and schedules mapping and title refresh. PostToolUse inspects path mentions and introduces cached references without reading document bodies or projecting code. `LORE_CODEX_REFRESH=0` disables automatic backend refresh. Deduplication requires a stable session identity and is best-effort after interruptions.

The Git-backed plugin marketplace package is planned. Host behavior follows the [Codex hooks contract](https://learn.chatgpt.com/docs/hooks); synthetic event tests do not establish support in every host.
