import * as vscode from 'vscode';
import { randomBytes } from 'node:crypto';
import { Knowledge, KnowledgeEntry } from './knowledge';

interface FilterState { query: string; regex: boolean; visible?: boolean }
interface Row {
  id: string; label: string; description?: string; tooltip?: string; depth: number;
  icon?: string; expandable: boolean; expanded: boolean; openable: boolean; context?: string;
}

export class KnowledgeView implements vscode.WebviewViewProvider, vscode.Disposable {
  private view?: vscode.WebviewView;
  private state: FilterState;
  private error?: string;
  private ready = false;
  private focusRequested = false;
  private generation = 0;
  private expanded = new Map<string, boolean>();
  private entries = new Map<string, KnowledgeEntry>();
  private subscriptions: vscode.Disposable[] = [];
  description = '';
  constructor(private readonly context: vscode.ExtensionContext, private readonly knowledge: Knowledge) {
    this.state = context.workspaceState.get<FilterState>('knowledgeFilter') || { query: '', regex: false };
    this.state.visible ??= !!this.state.query;
    this.error = knowledge.setFilter(this.state.query, this.state.regex);
    this.subscriptions.push(knowledge.onDidChangeTreeData(() => { void this.render(); }));
  }
  resolveWebviewView(view: vscode.WebviewView): void {
    this.view = view; this.ready = false;
    view.description = this.description;
    const nonce = randomBytes(16).toString('hex');
    const resource = (name: string) => view.webview.asWebviewUri(vscode.Uri.joinPath(this.context.extensionUri, 'media', name));
    view.webview.options = { enableScripts: true, localResourceRoots: [vscode.Uri.joinPath(this.context.extensionUri, 'media')] };
    view.webview.html = `<!doctype html><html><head><meta charset="UTF-8"><meta name="viewport" content="width=device-width, initial-scale=1"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src ${view.webview.cspSource}; script-src 'nonce-${nonce}';"><link rel="stylesheet" href="${resource('knowledge-view.css')}"></head><body>
      <section id="knowledge-filter" hidden><div class="input"><input id="knowledge-query" type="text" placeholder="Filter titles" aria-label="Filter knowledge titles" aria-describedby="filter-error" spellcheck="false" autocomplete="off"><button id="knowledge-regex" type="button" aria-label="Use regular expression" aria-pressed="false" title="Use regular expression">.*</button></div><div id="filter-error" role="alert" hidden></div></section>
      <div id="tree-error" role="alert" hidden></div><div id="knowledge-tree" role="tree" aria-label="Knowledge"></div><p id="knowledge-empty" hidden>No matching documents</p>
      <script nonce="${nonce}" src="${resource('knowledge-view.js')}"></script></body></html>`;
    this.subscriptions.push(view.webview.onDidReceiveMessage(message => {
      void this.handle(message).catch(error => { void vscode.window.showErrorMessage(`Lore: ${(error as Error).message}`); });
    }), view.onDidDispose(() => { if (this.view === view) { this.view = undefined; this.ready = false; this.generation++; } }));
  }
  private async handle(message: Record<string, unknown>): Promise<void> {
    if (message.type === 'ready') { this.ready = true; this.postState(); this.focusInput(); await this.render(); return; }
    if (message.type === 'filter') {
      if (typeof message.query !== 'string' || typeof message.regex !== 'boolean' || !Number.isSafeInteger(message.sequence)) return;
      this.state.query = message.query; this.state.regex = message.regex;
      this.error = this.knowledge.setFilter(this.state.query, this.state.regex);
      void this.context.workspaceState.update('knowledgeFilter', this.state);
      this.postState(message.sequence as number);
      return;
    }
    if (typeof message.id !== 'string') return;
    const entry = this.entries.get(message.id); if (!entry) return;
    const item = this.knowledge.getTreeItem(entry);
    if (message.type === 'expand' && item.collapsibleState !== vscode.TreeItemCollapsibleState.None) {
      if (typeof message.expanded !== 'boolean') return;
      this.expanded.set(message.id, message.expanded); await this.render();
    } else if (message.type === 'open' && item.command) {
      await vscode.commands.executeCommand(item.command.command, ...(item.command.arguments || []));
    }
  }
  private key(entry: KnowledgeEntry, parent: string): string {
    const item = this.knowledge.getTreeItem(entry);
    if (item.id) return item.id;
    if (entry.kind === 'document') return `${parent}/${entry.entry.view?.document.id || entry.entry.label}`;
    return `${parent}/${entry.kind}`;
  }
  private async render(): Promise<void> {
    if (!this.ready || !this.view) return;
    const generation = ++this.generation;
    const rows: Row[] = []; const entries = new Map<string, KnowledgeEntry>();
    const visit = async (items: KnowledgeEntry[], parent: string, depth: number): Promise<void> => {
      for (const entry of items) {
        if (generation !== this.generation) return;
        const item = this.knowledge.getTreeItem(entry); const id = this.key(entry, parent);
        const expandable = item.collapsibleState !== undefined && item.collapsibleState !== vscode.TreeItemCollapsibleState.None;
        const expanded = expandable && (this.expanded.get(id) ?? item.collapsibleState === vscode.TreeItemCollapsibleState.Expanded);
        rows.push({ id, label: typeof item.label === 'string' ? item.label : item.label?.label || '',
          description: typeof item.description === 'string' ? item.description : undefined,
          tooltip: typeof item.tooltip === 'string' ? item.tooltip : undefined, depth,
          icon: item.iconPath instanceof vscode.ThemeIcon ? item.iconPath.id : undefined,
          expandable, expanded, openable: !!item.command, context: item.contextValue });
        entries.set(id, entry);
        if (expanded) await visit(await this.knowledge.getChildren(entry), id, depth + 1);
      }
    };
    try {
      await visit(await this.knowledge.getChildren(), '', 0);
      if (generation !== this.generation) return;
      this.entries = entries;
      this.view.description = this.description;
      void this.view.webview.postMessage({ type: 'tree', rows });
    } catch (error) {
      if (generation === this.generation) void this.view?.webview.postMessage({ type: 'treeError', message: (error as Error).message });
    }
  }
  entry(id: string): KnowledgeEntry | undefined { return this.entries.get(id); }
  private postState(sequence?: number): void {
    void this.view?.webview.postMessage({ type: 'state', ...this.state, error: this.error, sequence });
  }
  async toggleFilter(): Promise<void> {
    this.state.visible = !this.state.visible;
    this.focusRequested = !!this.state.visible;
    void this.context.workspaceState.update('knowledgeFilter', this.state);
    await vscode.commands.executeCommand('lore.knowledge.focus');
    this.view?.show(true); this.postState(); this.focusInput();
  }
  private focusInput(): void {
    if (this.ready && this.focusRequested) { this.focusRequested = false; void this.view?.webview.postMessage({ type: 'focus' }); }
  }
  dispose(): void { this.generation++; for (const subscription of this.subscriptions) subscription.dispose(); this.view = undefined; }
}
