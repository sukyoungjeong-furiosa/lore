import * as vscode from 'vscode';
import { Backends, BackendEntry } from './backends';

export interface LocalEntry { label: string; root: vscode.WorkspaceFolder; children?: LocalEntry[] }
export type KnowledgeEntry = { kind: 'backend'; entry: BackendEntry } | { kind: 'local'; root: vscode.WorkspaceFolder } | { kind: 'document'; entry: LocalEntry };
export class Knowledge implements vscode.TreeDataProvider<KnowledgeEntry>, vscode.Disposable {
  private event = new vscode.EventEmitter<KnowledgeEntry | undefined>();
  readonly onDidChangeTreeData = this.event.event;
  constructor(private readonly backends: Backends, private readonly documents: vscode.TreeDataProvider<LocalEntry>) {}
  refresh(): void { this.event.fire(undefined); }
  getTreeItem(item: KnowledgeEntry): vscode.TreeItem {
    if (item.kind === 'backend') {
      const tree = this.backends.getTreeItem(item.entry);
      if (!item.entry.document && (vscode.workspace.workspaceFolders?.length || 0) > 1) tree.description = item.entry.root.name;
      tree.contextValue = item.entry.document ? 'lore.backendDocument' : item.entry.warning ? 'lore.warning' : 'lore.backend';
      return tree;
    }
    if (item.kind === 'document') return this.documents.getTreeItem(item.entry) as vscode.TreeItem;
    const tree = new vscode.TreeItem('Local Documents', vscode.TreeItemCollapsibleState.Collapsed);
    tree.description = (vscode.workspace.workspaceFolders?.length || 0) > 1 ? item.root.name : undefined;
    tree.id = `local:${item.root.uri.toString()}`;
    tree.iconPath = new vscode.ThemeIcon('folder-library'); tree.contextValue = 'lore.local';
    return tree;
  }
  async getChildren(item?: KnowledgeEntry): Promise<KnowledgeEntry[]> {
    if (!item) return [
      ...(await this.backends.getChildren()).map(entry => ({ kind: 'backend' as const, entry })),
      ...(vscode.workspace.workspaceFolders || []).map(root => ({ kind: 'local' as const, root })),
    ];
    if (item.kind === 'backend') return (await this.backends.getChildren(item.entry)).map(entry => ({ kind: 'backend', entry }));
    if (item.kind === 'document') return (await this.documents.getChildren(item.entry) || []).map(entry => ({ kind: 'document', entry }));
    const entries = await this.documents.getChildren() || [];
    const scoped = entries.filter(entry => entry.root.uri.toString() === item.root.uri.toString());
    const children = (vscode.workspace.workspaceFolders?.length || 0) > 1 ? scoped.flatMap(entry => entry.children || [entry]) : scoped;
    return children.map(entry => ({ kind: 'document', entry }));
  }
  dispose(): void { this.event.dispose(); }
}
