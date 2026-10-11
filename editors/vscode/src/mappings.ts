import * as vscode from 'vscode';
import { Anchor, call, LocatedMapping, Location, LoreError, Projection, View } from './client';
import { SelectedDocument } from './document-surface';

export interface MappingEntry {
  label: string;
  description?: string;
  detail?: string;
  selection?: SelectedDocument;
  anchor?: string;
  mapping?: string;
  store?: string | null;
  location?: Location;
}
interface ConnectionsState { view?: View; mappings?: LocatedMapping[] }
export class Mappings implements vscode.TreeDataProvider<MappingEntry>, vscode.Disposable {
  private event = new vscode.EventEmitter<MappingEntry | undefined>();
  readonly onDidChangeTreeData = this.event.event;
  private summaryEvent = new vscode.EventEmitter<void>();
  readonly onDidChangeSummary = this.summaryEvent.event;
  private state?: Promise<ConnectionsState>;
  private generation = 0;
  summary?: { count?: number; unavailable?: boolean };
  selected?: SelectedDocument;
  select(document?: SelectedDocument): void { this.selected = document; this.refresh(); }
  refresh(): void {
    const selected = this.selected;
    const generation = ++this.generation;
    this.summary = selected ? {} : undefined;
    this.state = selected ? this.load(selected) : undefined;
    void this.state?.then(state => {
      if (generation !== this.generation) return;
      const count = (state.view?.document.anchors.length || 0) + (state.mappings || []).reduce((n, { view }) => n + view.record.mapping.anchors.length + (view.record.mapping.scopes || []).length, 0);
      this.summary = { count }; this.summaryEvent.fire();
    }, () => {
      if (generation !== this.generation) return;
      this.summary = { unavailable: true }; this.summaryEvent.fire();
    });
    this.summaryEvent.fire();
    this.event.fire(undefined);
  }
  get description(): string | undefined {
    if (!this.selected) return undefined;
    const count = this.summary?.count;
    const status = this.summary?.unavailable ? 'Code links unavailable' : count === undefined ? 'Loading code links…' : `${count} code link${count === 1 ? '' : 's'}`;
    return `${this.selected.title} · ${status}`;
  }
  private async load(selected: SelectedDocument): Promise<ConnectionsState> {
    const central = selected.source.kind === 'local' && selected.source.working ? [] : ['--central'];
    const reference = selected.source.kind === 'backend' ? selected.source.reference : { backend: 'local', id: selected.source.id };
    const mappings = selected.source.kind === 'local' && selected.source.revision ? [] : (await call<{ mappings: LocatedMapping[] }>(selected.root, [...central, 'map', 'ls'])).mappings.filter(({ view }) => view.record.mapping.document.backend === reference.backend && view.record.mapping.document.id === reference.id);
    if (selected.source.kind === 'backend') return { mappings };
    const args = [...central, 'show', selected.source.id];
    if (selected.source.revision) args.push('--revision', selected.source.revision);
    return { view: await call<View>(selected.root, args), mappings };
  }

  getTreeItem(entry: MappingEntry): vscode.TreeItem {
    const tree = new vscode.TreeItem(entry.label);
    tree.description = entry.description;
    tree.tooltip = entry.detail;
    if (entry.location) {
      tree.iconPath = new vscode.ThemeIcon('file-code');
      tree.command = { command: 'lore.openCodeLocation', title: 'Open connected code', arguments: [entry] };
    } else if (entry.anchor) tree.iconPath = new vscode.ThemeIcon('warning');
    return tree;
  }
  async getChildren(entry?: MappingEntry): Promise<MappingEntry[]> {
    if (entry) return [];
    const selected = this.selected;
    const generation = this.generation;
    if (!selected) return [{ label: 'Select a document to see connected code' }];
    try {
      const state = await this.state!;
      let result: MappingEntry[];
      const central = selected.source.kind === 'local' && selected.source.working ? [] : ['--central'];
      const located = await Promise.all((state.mappings || []).map(({ view, store }) => call<LocatedMapping>(selected.root, [...central, 'map', 'resolve', view.record.mapping.id, ...(store ? ['--store', store] : [])])));
      result = located.flatMap(({ view, projections, store }) => [
        ...this.entries(selected, view.record.mapping.anchors, projections, view.record.mapping.context, view.record.mapping.id).map(entry => ({ ...entry, store, detail: [entry.detail, store ? `Store: ${store}` : ''].filter(Boolean).join('\n') })),
        ...(view.record.mapping.scopes || []).map(scope => ({ label: scope.glob, description: 'Code scope', detail: [scope.repository, view.record.mapping.context].filter(Boolean).join('\n') }))
      ]);
      if (selected.source.kind === 'local') {
        const projections = selected.source.revision ? [] : (await call<{ projections: Projection[] }>(selected.root, [...central, 'resolve', selected.source.id])).projections;
        result.push(...this.entries(selected, state.view!.document.anchors, projections, '', undefined, !!selected.source.revision));
      }
      if (generation !== this.generation) return this.getChildren();
      return result.length ? result : [{ label: 'No connected code' }];
    } catch (error) {
      if (generation !== this.generation) return this.getChildren();
      return [{ label: error instanceof LoreError && error.kind === 'not_initialized' ? 'Initialize Lore workspace to see connected code' : 'Code connections unavailable', detail: (error as Error).message }];
    }
  }
  private entries(selection: SelectedDocument, anchors: Anchor[], projections: Projection[], context: string, mapping?: string, historical = false): MappingEntry[] {
    return anchors.flatMap<MappingEntry>(anchor => {
      const projection = projections.find(p => p.anchor === anchor.id);
      const original = `${anchor.path}:${anchor.start}-${anchor.end}`;
      const detail = [context, `Recorded: ${original}`, projection?.explanation].filter(Boolean).join('\n');
      if (!projection?.locations.length) return [{ label: original, description: historical ? 'Recorded location' : 'Location unresolved', detail, selection, anchor: anchor.id, mapping }];
      return projection.locations.map(location => ({ label: `${location.path}:${location.start}-${location.end}`, detail, selection, anchor: anchor.id, mapping, location }));
    });
  }
  async openLocation(entry: MappingEntry): Promise<void> {
    const selected = entry.selection;
    if (!selected || !entry.location || !entry.anchor) return;
    if (this.selected !== selected) throw new Error('The selected document changed. Choose its current code connection.');
    if (!entry.mapping && selected.source.kind !== 'local') return;
    const result = entry.mapping
      ? await call<LocatedMapping>(selected.root, [...(selected.source.kind==='local' && selected.source.working ? [] : ['--central']), 'map', 'resolve', entry.mapping, ...(entry.store ? ['--store', entry.store] : [])])
      : await call<{ projections: Projection[] }>(selected.root, [...(selected.source.kind==='local' && selected.source.working ? [] : ['--central']), 'resolve', selected.source.kind === 'local' ? selected.source.id : '']);
    const location = result.projections.find(p => p.anchor === entry.anchor)?.locations.find(l => l.path === entry.location!.path && l.start === entry.location!.start && l.end === entry.location!.end);
    if (this.selected !== selected) return;
    if (!location) { this.refresh(); throw new Error('Code correspondence changed. Choose a refreshed code connection.'); }
    const document = await vscode.workspace.openTextDocument(vscode.Uri.joinPath(vscode.Uri.file(selected.root), location.path));
    if (document.isDirty) throw new Error('Save this code file before navigating to a recorded line range.');
    await vscode.window.showTextDocument(document, { viewColumn: vscode.ViewColumn.One, selection: new vscode.Range(location.start - 1, 0, location.end - 1, 0) });
  }
  dispose(): void { this.generation++; this.event.dispose(); this.summaryEvent.dispose(); }
}
