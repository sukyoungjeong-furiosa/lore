# Lore for VS Code

Browse Notion documents beside code and maintain persistent code connections through the Lore Rust CLI. Document contents stay in Notion; Lore stores mappings and derivative read caches.

Install the CLI with `cargo install --path crates/lore --locked`. Set **Lore: Executable** if necessary and initialize the checkout with **Lore: Initialize Workspace**.

The initial team distribution uses VSIX installation, with a private Marketplace planned later. Install or update a downloaded package in desktop VS Code with:

```bash
code --install-extension /path/to/lore.vsix --force
```

For Remote SSH, connect to the target host and use **Extensions: Install from VSIX...** in that window, selecting the SSH target if prompted. VSIX updates are explicit; automatic delivery requires the later Marketplace channel. The current package requires a separately installed CLI on the machine hosting the checkout. CLI bundling and GitHub Release packages are planned; see [Distribution](../../docs/decisions.md#distribution).

Use **Lore: Connect Notion** to configure a stable backend identity and root page, then enter a connection token or personal access token. For an existing backend, use **Lore: Set Notion Connection Token**. Credentials are shared with CLI and agent hooks in the same Lore home; old SecretStorage tokens migrate automatically. The local store uses plaintext files with owner-only Unix permissions, outside workspace settings and document caches. OAuth login is not implemented.

**Knowledge** lists each configured backend (for example, `notion`) alongside **Local Documents**. Click a document to read it in **Document**, below the trees in the Lore sidebar. No editor tab is opened. Follow Notion links there, or use the **Open in Notion** icon beside the view title for native editing. Use **Lore: Link Document to Code Selection** from a code editor to connect the selected Lore document. Local document reading uses the same title, metadata, and Markdown layout. **Edit Document** beside the Document title opens a plain text editor with Save and Cancel; Archive/Restore and Document History are in its title menu. Backlinks appear only when present. Local and backend documents use the same linking command; if no document is selected, choose one from local documents or cached backend titles. CodeLens navigates back from code.

Reads use the local cache first and retrieve a missing body once. The reader shows the source modification time, page creator, and last editor; **Reload Document** beside the view title explicitly refreshes that document, or reloads the saved revision of a local document while retaining editor drafts. Pages are read-only browsing entries by default, including the configured root. Right-click a page and choose **Discover Child Documents**, or use the checked action in the **Document** title menu. This enables its immediate document list; children remain plain readable pages unless enabled separately. **Stop Discovering Child Documents** reverses the setting without deleting cached bodies or mappings. The **Knowledge** refresh button updates entry-page titles and explicitly enabled discovery lists. It does not recursively explore child documents or download every body. Child listings use titles and IDs; detailed metadata is retrieved when reading a document. Progress reports API request counts and checked pages, with cancellation available. Repeated clicks share one operation; cancelling retains the previous completed title index. **Lore: Fetch All Document Content** in the Command Palette additionally caches bodies for offline reading. A failed Fetch preserves the previously cached body and reports the failure.

The filter button beside **Knowledge** reveals a single input row immediately beneath its title. It filters titles as you type, without Enter or an apply step. Press the filter button again to hide the row while retaining the query. The query remains active while navigating documents or code; erase it to restore the tree. The `.*` toggle enables case-insensitive regular expressions, such as `scheduler|imperative`. Nonmatching documents and empty groups disappear, with matches listed directly under their source. Invalid expressions show an error and preserve the last valid results. Filtering uses local documents, browsed backend titles, and any cached title index without contacting Notion.

**Mappings** shows the code locations connected to the selected document. Its title displays the document title and recorded code-link count even when collapsed; the count includes anchors and glob scopes and does not imply confirmed correspondence. Selecting a document or following a document link updates the list; clicking a location opens that code range without replacing the reader. Unresolved locations retain their recorded path and are labeled explicitly.

**Local Documents** supports Markdown editing, nested directories, wiki links, and revision history. Set the directory path while editing; an empty path places the document at the root. Search and creation are in Knowledge; reloading uses the native Document title action. Code connections appear only in Mappings. Editor drafts survive navigation to backend documents and mappings. Notion content remains owned by Notion.

Code projection uses saved UTF-8 files, exact text, and Git rename discovery. Unsaved buffers and model-based resolution are not supported. Notion Markdown is a preview; native blocks and images are available in Notion. This local VSIX has not been published to a marketplace.

**Include Archived Documents** in the Knowledge menu shows a checkmark when enabled, and the title displays **Archived included**. Connection commands remain in the Command Palette.

Connection display names are independent of document-reference identities. Use `lore backend set-name <connection-id> <display-name>` to change a name without breaking existing mappings or cached documents.
