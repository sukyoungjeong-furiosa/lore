# Lore for VS Code

Browse Notion documents beside code and maintain persistent code connections through the Lore Rust CLI. Document contents stay in Notion; Lore stores mappings and derivative read caches.

Install the CLI with `cargo install --path crates/lore --locked`. Set **Lore: Executable** if necessary and initialize the checkout with **Lore: Initialize Workspace**.

Use **Lore: Connect Notion** to configure a stable backend identity and root page, then enter a connection token or personal access token. For an existing backend, use **Lore: Set Notion Connection Token**. Credentials are shared with CLI and agent hooks in the same Lore home; old SecretStorage tokens migrate automatically. The local store uses plaintext files with owner-only Unix permissions, outside workspace settings and document caches. OAuth login is not implemented.

**Knowledge** lists each configured backend (for example, `furiosa`) alongside **Local Documents**. Click a document to read it in **Document**, below the trees in the Lore sidebar. No editor tab is opened. Follow Notion links there, or use the **Open in Notion** icon beside the view title for native editing. Use **Lore: Link Notion Document to Selection** from a code editor to create a mapping. CodeLens navigates back from code.

Reads use the local cache first and retrieve a missing body once. The reader shows the observation time; **Reload from Notion** beside the view title explicitly refreshes that document. Use the cloud-download action on a backend to refresh its tree and title index without downloading all bodies. **Fetch All Document Content** additionally caches bodies for offline reading. **Refresh** redraws workspace views; it does not force backend retrieval. A failed Fetch preserves the previously cached body and reports the failure.

The filter button beside **Knowledge** reveals a single input row immediately beneath its title. It filters titles as you type, without Enter or an apply step. Press the filter button again to hide the row while retaining the query. The query remains active while navigating documents or code; erase it to restore the tree. The `.*` toggle enables case-insensitive regular expressions, such as `scheduler|imperative`. Nonmatching documents and empty groups disappear, with matches listed directly under their source. Invalid expressions show an error and preserve the last valid results. Filtering uses local documents, browsed backend titles, and any cached title index without contacting Notion.

**Mappings** shows the code locations connected to the selected document. Selecting a document or following a document link updates the list; clicking a location opens that code range without replacing the reader. Unresolved locations retain their recorded path and are labeled explicitly.

**Local Documents** retains the initial document prototype: Markdown editing, wiki links, curation, and revision history. Editor drafts survive navigation to backend documents and mappings. Notion content remains owned by Notion.

Code projection uses saved UTF-8 files, exact text, and Git rename discovery. Unsaved buffers and model-based resolution are not supported. Notion Markdown is a preview; native blocks and images are available in Notion. This local VSIX has not been published to a marketplace.
