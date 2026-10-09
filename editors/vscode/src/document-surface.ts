import * as vscode from 'vscode';
import { randomBytes } from 'node:crypto';
import { DocumentReference, View } from './client';

export interface SelectedDocument {
  root: string;
  title: string;
  source: { kind: 'backend'; reference: DocumentReference } | { kind: 'local'; id: string; revision?: string };
}

export class DocumentSurface implements vscode.WebviewViewProvider, vscode.Disposable {
  private documentEvent = new vscode.EventEmitter<SelectedDocument | undefined>();
  readonly onDidChangeDocument = this.documentEvent.event;
  private view?: vscode.WebviewView;
  private owner?: object;
  private handler?: (message: Record<string, unknown>) => Promise<void>;
  private pendingHandler?: (message: Record<string, unknown>) => Promise<void>;
  private waiting: (() => void)[] = [];
  private lastMessage?: unknown;
  private title = 'Document';
  private backendDocument = false;
  constructor(private readonly extension: vscode.Uri) {}
  resolveWebviewView(view: vscode.WebviewView): void {
    this.view = view;
    view.webview.options = { enableScripts: true, localResourceRoots: [vscode.Uri.joinPath(this.extension, 'media')] };
    const nonce = randomBytes(16).toString('hex');
    const resource = (name: string) => view.webview.asWebviewUri(vscode.Uri.joinPath(this.extension, 'media', name));
    view.webview.html = `<!doctype html><html><head><meta charset="UTF-8"><meta name="viewport" content="width=device-width, initial-scale=1"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src ${view.webview.cspSource}; script-src 'nonce-${nonce}';"><link rel="stylesheet" href="${resource('panel.css')}"></head><body>
      <section id="empty-mode">Select a document from Lore.</section>
      <section id="local-mode" hidden>
      <header><button id="back">←</button><span>Local document</span><button id="new">New</button><button id="refresh">Refresh</button></header>
      <form id="search-form"><input id="query" placeholder="Search local knowledge"><select id="curation"><option value="all">All contributions</option><option value="curated">Human curated</option><option value="non-agent">Exclude agent contributions</option></select><label><input type="checkbox" id="include-archived"> Archived</label><button>Search</button></form>
      <div id="error" role="alert"></div><section id="results" hidden></section>
      <main hidden id="document"><h1 id="title"></h1><p id="status"></p><nav><button id="edit">Edit</button><button id="save" hidden>Save</button><button id="cancel" hidden>Cancel</button><button id="curate">Curate</button><button id="archive">Archive</button><button id="history">History</button></nav>
      <section id="reader"></section><section id="editor" hidden><input id="edit-title" aria-label="Document title"><div id="edit-tags"></div><input id="tag-input" placeholder="Add a tag" aria-label="New tag"><button id="add-tag">Add tag</button><textarea id="body" aria-label="Document content" spellcheck="false"></textarea><p id="draft-status"></p></section>
      <section id="references"><h2>Code</h2><div id="anchors"></div><h2>Links</h2><div id="links"></div><h2>Backlinks</h2><div id="backlinks"></div></section></main></section>
      <section id="backend-mode" hidden>
      <h1 id="backend-title"></h1><p id="backend-status"></p><div id="backend-warnings"></div><section id="backend-reader"></section><p>Notion Markdown preview. Native blocks and images are available in Notion.</p></section>
      <script nonce="${nonce}">window.loreApi=acquireVsCodeApi();</script>
      <script nonce="${nonce}" src="${resource('panel.js')}"></script><script nonce="${nonce}" src="${resource('backend-panel.js')}"></script><script nonce="${nonce}" src="${resource('document-mode.js')}"></script></body></html>`;
    view.title = this.title;
    view.webview.onDidReceiveMessage(message => {
      if (message.type === 'surfaceReady') { if (this.lastMessage) void view.webview.postMessage(this.lastMessage); return; }
      void this.handler?.(message).catch(error => { if (!this.handler && this.owner) this.fail(this.owner, error.message); void vscode.window.showErrorMessage(`Lore: ${error.message}`); });
    });
    view.onDidDispose(() => { if (this.view === view) { this.view = undefined; this.setBackendDocument(false); } });
    for (const ready of this.waiting.splice(0)) ready();
  }
  async select(owner: object, handler: (message: Record<string, unknown>) => Promise<void>): Promise<void> {
    this.setBackendDocument(false);
    this.documentEvent.fire(undefined);
    this.owner = owner; this.pendingHandler = handler; this.handler = undefined;
    if (!this.view) {
      const resolved = new Promise<void>(resolve => this.waiting.push(resolve));
      await vscode.commands.executeCommand('lore.reader.focus');
      await resolved;
    }
    if (!this.active(owner)) return;
    this.view!.show(true);
    this.lastMessage = { type: 'surfaceLoading' };
    void this.view!.webview.postMessage(this.lastMessage);
  }
  fail(owner: object, message: string): void {
    if (!this.active(owner)) return;
    this.setBackendDocument(false);
    this.lastMessage = { type: 'surfaceError', message };
    if (this.view) void this.view.webview.postMessage(this.lastMessage);
  }
  active(owner: object): boolean { return this.owner === owner; }
  post(owner: object, message: Record<string, unknown>, title?: string): void {
    const document = ['document', 'backendDocument'].includes(message.type as string);
    if (document && !this.active(owner)) return;
    if (document) { this.setBackendDocument(message.type === 'backendDocument'); this.handler = this.pendingHandler; this.lastMessage = message; if (title) { this.title = title; if (this.view) this.view.title = title; } }
    if (document && typeof message.root === 'string') {
      if (message.type === 'backendDocument') this.documentEvent.fire({ root: message.root, title: message.title as string, source: { kind: 'backend', reference: message.reference as DocumentReference } });
      else {
        const view = message.view as View;
        this.documentEvent.fire({ root: message.root, title: view.document.title, source: { kind: 'local', id: view.document.id, revision: message.historical as string | undefined } });
      }
    }
    if (this.view) void this.view.webview.postMessage(message);
  }
  async invokeBackendAction(action: 'fetch' | 'original'): Promise<void> {
    if (this.backendDocument && this.view) await this.handler?.({ action });
  }
  private setBackendDocument(value: boolean): void {
    this.backendDocument = value;
    void vscode.commands.executeCommand('setContext', 'lore.readerIsBackendDocument', value);
  }
  dispose(): void { this.handler = undefined; this.view = undefined; this.setBackendDocument(false); this.documentEvent.dispose(); }
}
