import * as vscode from 'vscode';
import { Backends, BackendEntry } from './backends';
import { call, BackendDocument, Retrieved, View } from './client';

export interface LocalEntry { label: string; root: vscode.WorkspaceFolder; children?: LocalEntry[]; view?: View }
export type KnowledgeEntry = { kind: 'backend'; entry: BackendEntry } | { kind: 'local'; root: vscode.WorkspaceFolder } | { kind: 'document'; entry: LocalEntry };
export class Knowledge implements vscode.TreeDataProvider<KnowledgeEntry>, vscode.Disposable {
  private event = new vscode.EventEmitter<KnowledgeEntry | undefined>();
  readonly onDidChangeTreeData = this.event.event;
  private loadedBranches = new Map<string, BackendEntry[]>();
  private filteredSources: KnowledgeEntry[] = [];
  private filteredChildren = new Map<KnowledgeEntry, KnowledgeEntry[]>();
  private snapshot?: Promise<void>;
  private generation = 0;
  private matcher?: (title: string) => boolean;
  filter = '';
  regex = false;
  constructor(private readonly backends: Backends, private readonly documents: vscode.TreeDataProvider<LocalEntry>) {}
  refresh(): void { this.generation++; this.snapshot = undefined; this.event.fire(undefined); }
  clearBrowsedTitles(): void { this.loadedBranches.clear(); }
  setFilter(value: string, regex: boolean): string | undefined {
    let matcher: ((title: string) => boolean) | undefined;
    if (value) {
      if (regex) {
        let expression: RegExp;
        try { expression = new RegExp(value, 'i'); }
        catch (error) { return `Invalid regular expression: ${(error as Error).message}`; }
        matcher = title => expression.test(title);
      } else {
        const query = value.toLocaleLowerCase();
        matcher = title => title.toLocaleLowerCase().includes(query);
      }
    }
    this.filter = value; this.regex = regex; this.matcher = matcher;
    this.event.fire(undefined);
  }
  private key(entry: BackendEntry): string {
    return JSON.stringify([entry.root.uri.toString(), entry.backend, entry.document?.reference.id]);
  }
  private async prepareSnapshot(): Promise<void> {
    const generation = this.generation;
    const sources = await this.sources();
    const local = await this.documents.getChildren() || [];
    const children = new Map<KnowledgeEntry, KnowledgeEntry[]>();
    for (const source of sources) {
      if (source.kind === 'backend') {
        const root = source.entry;
        if (root.warning || root.label) continue;
        const known = new Map<string, BackendEntry>();
        for (const branch of this.loadedBranches.values()) for (const entry of branch) {
          if (entry.document && entry.backend === root.backend && entry.root.uri.toString() === root.root.uri.toString())
            known.set(entry.document.reference.id, entry);
        }
        try {
          const index = await call<Retrieved<{ documents: BackendDocument[] }>>(root.root.uri.fsPath, ['docs', 'search', root.backend, '']);
          for (const document of index.value.documents) {
            const previous = known.get(document.reference.id);
            known.set(document.reference.id, { ...root, ...previous, discoveryPages: root.discoveryPages, document });
          }
        } catch { /* Browsed titles remain filterable before a title index is fetched. */ }
        children.set(source, [...known.values()].map(entry => ({ kind: 'backend', entry })));
      } else if (source.kind === 'local') {
        const known = new Map<string, LocalEntry>();
        const visit = (entries: LocalEntry[]) => {
          for (const entry of entries) {
            if (entry.view && entry.root.uri.toString() === source.root.uri.toString()) known.set(entry.view.document.id, entry);
            if (entry.children) visit(entry.children);
          }
        };
        visit(local);
        children.set(source, [...known.values()].map(entry => ({ kind: 'document', entry })));
      }
    }
    if (generation === this.generation) { this.filteredSources = sources; this.filteredChildren = children; }
  }
  private async sources(): Promise<KnowledgeEntry[]> {
    return [
      ...(await this.backends.getChildren()).map(entry => ({ kind: 'backend' as const, entry })),
      ...(vscode.workspace.workspaceFolders || []).map(root => ({ kind: 'local' as const, root })),
    ];
  }
  private matches(item: KnowledgeEntry): boolean {
    const title = item.kind === 'backend' ? item.entry.document?.title : item.kind === 'document' ? item.entry.view?.document.title : undefined;
    return title !== undefined && !!this.matcher?.(title);
  }
  getTreeItem(item: KnowledgeEntry): vscode.TreeItem {
    if (item.kind === 'backend') {
      const tree = this.backends.getTreeItem(item.entry);
      if (!item.entry.document && (vscode.workspace.workspaceFolders?.length || 0) > 1) tree.description = item.entry.root.name;
      tree.contextValue = item.entry.document ? (this.backends.childDiscovery(item.entry) ? 'lore.discoveryPage' : 'lore.backendDocument') : item.entry.warning ? 'lore.warning' : 'lore.backend';
      if (this.matcher) {
        tree.id = `filtered:${tree.id}`;
        tree.collapsibleState = item.entry.document ? vscode.TreeItemCollapsibleState.None : vscode.TreeItemCollapsibleState.Expanded;
      }
      return tree;
    }
    if (item.kind === 'document') return this.documents.getTreeItem(item.entry) as vscode.TreeItem;
    const tree = new vscode.TreeItem('Local Documents', vscode.TreeItemCollapsibleState.Collapsed);
    tree.description = (vscode.workspace.workspaceFolders?.length || 0) > 1 ? item.root.name : undefined;
    tree.id = `local:${item.root.uri.toString()}`;
    if (this.matcher) { tree.id = `filtered:${tree.id}`; tree.collapsibleState = vscode.TreeItemCollapsibleState.Expanded; }
    tree.iconPath = new vscode.ThemeIcon('folder-library'); tree.contextValue = 'lore.local';
    return tree;
  }
  async getChildren(item?: KnowledgeEntry): Promise<KnowledgeEntry[]> {
    if (this.matcher) {
      for (;;) {
        const pending = this.snapshot ??= this.prepareSnapshot();
        try { await pending; }
        catch (error) { if (this.snapshot === pending) this.snapshot = undefined; throw error; }
        if (this.snapshot === pending) break;
      }
      if (!this.matcher) return this.getChildren(item);
      if (!item) return this.filteredSources.filter(source => this.filteredChildren.get(source)?.some(child => this.matches(child)));
      return (this.filteredChildren.get(item) || []).filter(child => this.matches(child));
    }
    if (!item) return this.sources();
    if (item.kind === 'backend') {
      const entries = await this.backends.getChildren(item.entry);
      this.loadedBranches.set(this.key(item.entry), entries); this.snapshot = undefined;
      return entries.map(entry => ({ kind: 'backend', entry }));
    }
    if (item.kind === 'document') return (await this.documents.getChildren(item.entry) || []).map(entry => ({ kind: 'document', entry }));
    const entries = await this.documents.getChildren() || [];
    const scoped = entries.filter(entry => entry.root.uri.toString() === item.root.uri.toString());
    const children = (vscode.workspace.workspaceFolders?.length || 0) > 1 ? scoped.flatMap(entry => entry.children || [entry]) : scoped;
    return children.map(entry => ({ kind: 'document', entry }));
  }
  dispose(): void { this.event.dispose(); }
}
