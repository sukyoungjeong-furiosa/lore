import * as vscode from 'vscode';
import { randomBytes } from 'node:crypto';
import { DocumentReference, View } from './client';

export interface SelectedDocument {
  root: string;
  title: string;
  source: { kind: 'backend'; reference: DocumentReference } | { kind: 'local'; id: string; revision?: string; working?: boolean };
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
  private selected?: SelectedDocument;
  constructor(private readonly extension: vscode.Uri) {}
  resolveWebviewView(view: vscode.WebviewView): void {
    this.view = view;
    view.webview.options = { enableScripts: true, localResourceRoots: [vscode.Uri.joinPath(this.extension, 'media')] };
    const nonce = randomBytes(16).toString('hex');
    const resource = (name: string) => view.webview.asWebviewUri(vscode.Uri.joinPath(this.extension, 'media', name));
    view.webview.html = `<!doctype html><html><head><meta charset="UTF-8"><meta name="viewport" content="width=device-width, initial-scale=1"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src ${view.webview.cspSource}; script-src 'nonce-${nonce}';"><link rel="stylesheet" href="${resource('panel.css')}"></head><body>
      <section id="empty-mode">Select a document from Lore.</section>
      <section id="local-mode" hidden>
      <div id="error" role="alert"></div>
      <main hidden id="document"><h1 id="title"></h1><p id="status"></p>
      <section id="reader"></section><section id="editor" hidden><input id="edit-title" aria-label="Document title"><input id="edit-directory" placeholder="Directory (root when empty)" aria-label="Document directory"><textarea id="body" aria-label="Document content" spellcheck="false"></textarea><nav><button id="save">Save</button><button id="cancel">Cancel</button></nav><p id="draft-status"></p></section>
      <section id="references" hidden><h2>Backlinks</h2><div id="backlinks"></div></section></main></section>
      <section id="backend-mode" hidden>
      <h1 id="backend-title"></h1><p id="backend-status"></p><div id="backend-warnings"></div><section id="backend-reader"></section><p>Notion Markdown preview. Native blocks and images are available in Notion.</p></section>
      <script nonce="${nonce}">window.loreApi=acquireVsCodeApi();</script>
      <script nonce="${nonce}" src="${resource('panel.js')}"></script><script nonce="${nonce}" src="${resource('backend-panel.js')}"></script><script nonce="${nonce}" src="${resource('document-mode.js')}"></script></body></html>`;
    view.title = this.title;
    view.webview.onDidReceiveMessage(message => {
      if (message.type === 'surfaceReady') { if (this.lastMessage) void view.webview.postMessage(this.lastMessage); return; }
      if (message.type === 'localEditorState') {
        if (this.selected?.source.kind === 'local' && message.id === this.selected.source.id && message.root === this.selected.root) {
          void vscode.commands.executeCommand('setContext', 'lore.readerEditing', message.editing === true);
          void vscode.commands.executeCommand('setContext', 'lore.readerDirty', message.dirty === true);
          void vscode.commands.executeCommand('setContext', 'lore.readerBusy', message.busy === true);
        }
        return;
      }
      void this.handler?.(message).catch(error => { if (!this.handler && this.owner) this.fail(this.owner, error.message); void vscode.window.showErrorMessage(`Lore: ${error.message}`); });
    });
    view.onDidDispose(() => { if (this.view === view) { this.view = undefined; this.setBackendDocument(false); this.selectDocument(undefined); } });
    for (const ready of this.waiting.splice(0)) ready();
  }
  async select(owner: object, handler: (message: Record<string, unknown>) => Promise<void>): Promise<void> {
    this.setBackendDocument(false);
    this.selectDocument(undefined);
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
    this.selectDocument(undefined);
    this.lastMessage = { type: 'surfaceError', message };
    if (this.view) void this.view.webview.postMessage(this.lastMessage);
  }
  active(owner: object): boolean { return this.owner === owner; }
  post(owner: object, message: Record<string, unknown>, title?: string): void {
    const document = ['document', 'backendDocument'].includes(message.type as string);
    if (document && !this.active(owner)) return;
    if (document) { this.setBackendDocument(message.type === 'backendDocument'); this.setChildDiscovery(message.childDiscovery === true); this.handler = this.pendingHandler; this.lastMessage = message; if (title) { this.title = title; if (this.view) this.view.title = title; } }
    if (document && typeof message.root === 'string') {
      if (message.type === 'backendDocument') this.selectDocument({ root: message.root, title: message.title as string, source: { kind: 'backend', reference: message.reference as DocumentReference } });
      else {
        const view = message.view as View;
        this.selectDocument({ root: message.root, title: view.document.title, source: { kind: 'local', id: view.document.id, revision: message.historical as string | undefined, working: message.working === true } });
      }
    }
    if (document) {
      void vscode.commands.executeCommand('setContext', 'lore.readerWorking', message.working === true);
      void vscode.commands.executeCommand('setContext', 'lore.readerIsHistorical', !!message.historical);
      void vscode.commands.executeCommand('setContext', 'lore.readerArchived', message.type === 'document' && (message.view as View).document.archived);
    }
    if (this.view) void this.view.webview.postMessage(message);
  }
  async invokeLocalAction(action: 'edit' | 'archive' | 'history' | 'publish'): Promise<void> {
    const selected = this.selected;
    if (selected?.source.kind !== 'local') return;
    await this.view?.webview.postMessage({ type: 'localAction', action, root: selected.root, id: selected.source.id });
  }
  async invokeBackendAction(action: 'fetch' | 'original'): Promise<void> {
    if (this.backendDocument && this.view) await this.handler?.({ action });
  }
  async reload(): Promise<void> {
    if (!this.selected) return;
    if (this.backendDocument) await this.invokeBackendAction('fetch');
    else await this.handler?.({ type: 'refresh' });
  }
  private selectDocument(document?: SelectedDocument): void {
    this.selected = document;
    void vscode.commands.executeCommand('setContext', 'lore.readerIsLocalDocument', document?.source.kind === 'local');
    for (const name of ['readerEditing', 'readerDirty', 'readerBusy', 'readerIsHistorical', 'readerArchived', 'readerWorking']) void vscode.commands.executeCommand('setContext', `lore.${name}`, false);
    void vscode.commands.executeCommand('setContext', 'lore.readerHasDocument', !!document);
    this.documentEvent.fire(document);
  }
  setChildDiscovery(enabled: boolean): void {
    void vscode.commands.executeCommand('setContext', 'lore.readerChildDiscovery', enabled);
  }
  private setBackendDocument(value: boolean): void {
    this.backendDocument = value;
    this.setChildDiscovery(false);
    void vscode.commands.executeCommand('setContext', 'lore.readerIsBackendDocument', value);
  }
  dispose(): void { this.handler = undefined; this.view = undefined; this.setBackendDocument(false); this.selectDocument(undefined); this.documentEvent.dispose(); }
}
