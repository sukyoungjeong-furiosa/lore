# Knowledge Model

Lore models documents, their metadata and links, and persistent mappings to code anchors.

## Document

A document is knowledge with a stable identity, a title, and content owned by a knowledge backend.

- A document reference identifies its backend and its identity within that backend. Titles need not be unique; titles and access credentials are not document identities.
- Renaming or relocating a document within its backend does not change its reference. Copying it to another backend does not implicitly preserve identity.
- Documents can exist without code mappings. Referencing or caching a document does not transfer its ownership to Lore.
- Content formats, editing, permissions, and available history follow the owning backend's contract. Unsupported capabilities must not be implied.
- Lore-owned documents have a supported content format, with Markdown as the default, and preserve inspectable revision history.

## Metadata

Document metadata describes authorship, change history, tags, curation, and archive state where available.

- Backend document metadata remains distinct from Lore-managed mapping metadata. Unavailable metadata remains unknown.
- Curation indicates explicit human curation, independently of authorship. It does not certify correctness or restrict use or sharing.
- Lore-owned documents may have multiple tags. Changing tags or archive state does not change their curation status.
- Archiving a Lore-owned document preserves its identity, content, and references and is reversible.

## Link

Links connect documents using conventions supported by their content format and backend.

- Links to identified documents preserve their targets across renaming or relocation within the owning backend.
- Ambiguous title references must not silently resolve to one document.
- Unresolved or unavailable references remain visible without preventing available content from being read.
- Lore may expose links and backlinks where available; their absence does not imply that all backend relationships have been enumerated.

## Mapping

A mapping has a stable identity, a document reference, one or more code anchors, and optional explanatory context.

- A document can have zero or more mappings. Creating or editing a mapping does not modify its document.
- Lore preserves mapping revision history. Mapping authorship and any human confirmation remain distinct from document authorship and curation.
- A recorded document state describes what was observed when forming the relationship; it does not imply that the backend can retrieve that historical content.
- Failure to retrieve a document does not remove its mappings or establish that it was deleted.
- Archiving a mapping preserves its identity, explanatory context, references, and history and is reversible. It does not archive its document.

## Anchor

An anchor records a code repository, revision or captured working state, and location.

- Different anchors may refer to different files, repositories, or code states.
- Projection preserves the original reference without modifying documents or code.
- Finding a corresponding location does not establish the validity of the explanation or extend human confirmation to changed code or document content.
