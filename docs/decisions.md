# Design Decisions

## Document Cache and Fetch

Cached documents are observations of backend-owned content, not Lore-owned revisions.

- Reading uses a cached observation when available and retrieves missing content on demand. Cache age and incomplete content remain visible.
- Fetch explicitly refreshes a document's body, or the tree and title index under a backend's configured roots. Fetching the tree does not download every body; bulk body retrieval is a separate choice.
- Failed refreshes remain distinguishable from success and do not discard the previous cached content. Batch retrieval reports failures per document.
- Cache-only access performs no backend requests. Missing content is distinguishable from an empty document or discovery result.
- Document retrieval does not modify mappings, local document edits, or synchronization baselines.

Backend document access is separate from [mapping synchronization](architecture/synchronization.md). Read-only backends such as Notion do not require a document Pull or Push operation.
