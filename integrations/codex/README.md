# Codex Adapter

Optional command hooks for preparing Lore workspaces, introducing related document references, and checking for reusable knowledge after a turn. Requires an installed `lore` CLI, Python 3, and POSIX file locking.

1. In [hooks.json](hooks.json), replace `/absolute/path/to/lore` with this checkout's absolute path.
2. Merge its SessionStart and PostToolUse entries, and optionally Stop, into your host's hook configuration, then review and enable them through `/hooks` in a host supporting command hooks.
3. Install the replaceable [reading skill](../../examples/skills/lore-ko/SKILL.md) using your agent's skill installation mechanism. The Stop hook points directly to the separate [recording skill](../../examples/skills/lore-record-ko/SKILL.md); it can also be installed for explicit use.

`LORE_EXECUTABLE` selects the CLI; `LORE_HOME` selects its local data directory. Document backend connections and credentials are shared with the CLI and editor using that home. Saved tokens are plaintext files with owner-only Unix permissions; a configured token environment variable takes precedence.

SessionStart prepares the workspace and schedules mapping and title refresh. PostToolUse inspects path mentions and introduces accepted central references without reading document bodies or projecting code. `LORE_CODEX_REFRESH=0` disables automatic backend refresh. Deduplication requires a stable session identity and is best-effort after interruptions.

Stop requests one recording assessment per turn in an initialized workspace. The agent follows the recording skill, updating or creating a document, committing on its working branch, and submitting to local central knowledge only when there is new reusable knowledge. Re-entry and duplicate events are skipped; interrupted delivery can skip an assessment. The hook does not read transcripts, fetch documents, call a model, or write document content. `lore_record.py --skill <path>` selects another recording policy. Commit alone stays private to a working branch. Successful local integration makes knowledge discoverable centrally; other working branches Pull it. Remote submission stays separate. Conflicts preserve committed proposals and pending changes for inspection.

The Git-backed plugin marketplace package is planned. Host behavior follows the [Codex hooks contract](https://learn.chatgpt.com/docs/hooks); synthetic event tests do not establish support in every host.
