# Knowledge Model

Lore models documents, their metadata and links, and persistent mappings to code anchors.

## Document

A document is knowledge with a stable identity, a title, and content owned by a document backend.

- A document reference identifies its backend and its identity within that backend. Titles need not be unique; titles and access credentials are not document identities.
- Renaming or relocating a document within its backend does not change its reference. Copying it to another backend does not implicitly preserve identity.
- Documents can exist without code mappings. Referencing or caching a document does not transfer its ownership to Lore.
- Content formats, editing, permissions, and available history follow the owning backend's contract. Unsupported capabilities must not be implied.
- Lore-owned documents have a supported content format, with Markdown as the default, and preserve inspectable revision history.

## Metadata

Document metadata describes authorship, change history, and archive state where available.

- Backend document metadata remains distinct from Lore-managed mapping metadata. Unavailable metadata remains unknown.
- Archiving a Lore-owned document preserves its identity, content, and references and is reversible.

## Organization

- Documents may be organized in a hierarchy within their owning backend.
- Lore-owned documents have one directory location, with the root as the default. Directories may be nested.
- A directory location is independent of document identity and code paths. Moving a document preserves its links and code connections.
- External documents follow their backend's hierarchy; Lore does not impose a separate classification.

## Link

Links connect documents using conventions supported by their content format and backend.

- Links to identified documents preserve their targets across renaming or relocation within the owning backend.
- Ambiguous title references must not silently resolve to one document.
- Unresolved or unavailable references remain visible without preventing available content from being read.
- Lore may expose links and backlinks where available; their absence does not imply that all backend relationships have been enumerated.

## Mapping

A mapping has a stable identity within its knowledge store, one document reference, one or more code anchors or path scopes in one code repository, and optional explanatory context.

- A document can have zero or more mappings. Creating or editing a mapping does not modify its document.
- Lore preserves mapping revision history. Mapping authorship and any human confirmation remain distinct from document authorship.
- A recorded document state describes what was observed when forming the relationship; it does not imply that the backend can retrieve that historical content.
- Failure to retrieve a document does not remove its mappings or establish that it was deleted.
- Archiving a mapping preserves its identity, explanatory context, references, and history and is reversible. It does not archive its document.

## Anchor

An anchor records a code repository, revision or captured working state, and location.

- Anchors within a mapping may refer to different files or code states in the same repository. The same document can have separate mappings for other repositories.
- Projection preserves the original reference without modifying documents or code.
- Finding a corresponding location does not establish the validity of the explanation or extend human confirmation to changed code or document content.

## Path Scope

A path scope associates a document with checkout-relative paths in a code repository using a glob pattern.

- Scopes match current paths without claiming a particular code revision or line location.
- Scopes and anchors may coexist in a mapping. Matching either discovers a relationship; it does not validate the document's explanation.
