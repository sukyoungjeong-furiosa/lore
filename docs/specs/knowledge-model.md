# Knowledge Model

Lore models knowledge through four concepts: Document, Metadata, Link, and Anchor.

## Document

A document is a piece of knowledge with a stable identity, a title, and content.

- Documents have a supported content format, with Markdown as the default.
- Identity remains stable when the title, content, or metadata changes.
- Titles need not be unique.
- Documents can be read and edited without a code connection.
- Documents preserve revision history, allowing previous versions and changes to be inspected.

## Metadata

Metadata describes a document's authorship, change history, tags, curation status,
and archive state.

- Documents may have multiple tags for classification.
- Curation status indicates whether knowledge has been explicitly curated by a human, independently of its authorship.
- Curation status does not certify correctness or restrict use or sharing.
- Changing tags, anchors, or archive state does not change curation status.
- Archiving preserves a document's identity, content, and references and is reversible.

## Link

Links connect documents and support navigation in both directions through backlinks.

- Links are expressed in document content using conventions supported by its format.
- Links to identified documents preserve their targets across renaming or relocation.
- Ambiguous title references must not silently resolve to one document.
- Referenced documents can be fetched on demand when accessible.
- Unresolved or unavailable references remain visible without preventing the source document from being read.

## Anchor

An anchor relates a document to a code location at a recorded code revision.

- A document can have zero or more anchors.
- Each anchor identifies the code repository, revision, and location it refers to. Different anchors can refer to different files or revisions.
- Resolving an anchor against changed code preserves the original reference without modifying the document.
- Finding a corresponding code location does not establish the validity of the document's explanation.
