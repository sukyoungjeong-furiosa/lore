# Experience

Lore makes related knowledge available while working on code, without requiring documents to be imported into the code repository.

## Reading and Discovery

- Documents open beside code, with navigation between documents and their code connections.
- Local documents and external backends share a browsing experience. Editing external documents stays with their owning backend.
- A connected Notion page is readable by default. Discovering its immediate documents is enabled explicitly per page and is not inherited by children. Following a document link does not recursively discover its descendants.
- Title filtering operates on known documents without contacting the backend. It hides nonmatches as the query changes and optionally supports regular expressions.

## Cache and Refresh

- Reading uses cached content when available and retrieves missing content on demand. Cached content may be older than its source.
- Refreshing Knowledge updates titles and enabled discovery lists, without downloading every document body. Reloading a document refreshes that document; bulk body caching is an explicit choice.
- Failed or cancelled refreshes preserve available cached knowledge and remain distinguishable from success.
- The reader shows source modification time, creator, and last editor when available. Unknown metadata remains unknown.

## Local Work and Agent Context

- Worktrees share backend connections and document caches while keeping local changes and code projections separate. Updates preserve this data.
- Unsaved document drafts survive navigation. Conflicting saves preserve edits rather than overwriting another revision.
- Within an agent session, each Lore document is automatically introduced at most once. Explicit discovery and reading are always available.
- Automatic introduction supplies related document references and recorded code scopes or lines. Path mentions are best-effort observations; they do not establish file access, current correspondence, or explanation validity.
