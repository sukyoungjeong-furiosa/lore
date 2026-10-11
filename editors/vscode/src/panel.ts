import * as vscode from 'vscode';
import { DocumentSurface } from './document-surface';
import { call, Connections, View, LoreError } from './client';
import { markdown } from './markdown';
export class KnowledgePanel implements vscode.Disposable {
  private sequence = 0;
  private disposed = false;
  private current?: View;
  private working = false;
  private workingDocuments = new Set<string>();
  markWorking(id: string): void { this.workingDocuments.add(id); }
  constructor(private readonly root: string, private readonly surface: DocumentSurface, private readonly changed: () => void) {}
  isDisposed(): boolean { return this.disposed; }
  private send(message: Record<string, unknown>): void { if (!this.disposed) this.surface.post(this, { ...message, root: this.root }, this.current ? `Document · ${this.current.document.title}` : undefined); }
  async open(id: string, options?: { saved?: number; savedRevision?: string; historical?: string }): Promise<void> {
    await this.surface.select(this, async message => { try { await this.handle(message); } catch (error) { this.send({ type: 'error', message: (error as Error).message }); } });
    const sequence = ++this.sequence;
    try {
      let working=this.workingDocuments.has(id);
      let view: View;
      const args = ['show', id]; if (options?.historical) args.push('--revision', options.historical);
      const linkArgs = ['links', id]; if (options?.historical) linkArgs.push('--revision', options.historical);
      if (!working) {
        try { view = await call<View>(this.root, ['--central', ...args]); }
        catch (error) {
          if (!(error instanceof LoreError) || error.kind !== 'not_found') throw error;
          working=true; view=await call<View>(this.root,args);
        }
      } else view=await call<View>(this.root,args);
      this.working=working; if (working) this.workingDocuments.add(id);
      const connections=await call<Connections>(this.root, working ? linkArgs : ['--central',...linkArgs]);
      if (sequence !== this.sequence || this.disposed || !this.surface.active(this)) return;
      this.current = view;
      this.send({ type: 'document', view, connections, working,
        html: view.document.content_format === 'markdown' ? markdown.render(view.document.content) : undefined,
        saved: options?.saved, savedRevision: options?.savedRevision, historical: options?.historical });
    } catch (error) { if (sequence === this.sequence) this.surface.fail(this, (error as Error).message); throw error; }
  }
  private async handle(m: Record<string, unknown>): Promise<void> {
    switch (m.type) {
      case 'ready': break;
      case 'open': if (typeof m.id === 'string') await this.open(m.id); break;
      case 'refresh': if (this.current) await this.open(this.current.document.id); break;
      case 'save': {
        if (typeof m.id !== 'string' || typeof m.revision !== 'string' || typeof m.content !== 'string' || typeof m.title !== 'string' || typeof m.serial !== 'number' || typeof m.directory !== 'string') return;
        let revision=m.revision;
        if (!this.working) {
          const central=await call<View>(this.root,['--central','show',m.id]);
          if (central.revision!==m.revision) throw new Error('Central document changed since it was read. Reload before saving.');
          const status = await call<{ workspace: { documents: Record<string, { revision: string; shared_baseline?: { revision: string } }> } }>(this.root,['status']);
          const active = status.workspace.documents[m.id];
          if (active && active.shared_baseline?.revision !== active.revision) throw new Error('Document changed since it was read. Inspect the working copy before saving.');
          await call(this.root,['pull']);
          const selected=await call<View>(this.root,['checkout',m.id]); revision=selected.revision;
        }
        const args = ['edit', m.id, '--if-match', revision, '--title', m.title, '--stdin'];
        args.push('--directory', m.directory);
        const view = await call<View>(this.root, args, m.content);
        this.working=true; this.workingDocuments.add(view.document.id);
        this.changed();
        if (this.surface.active(this) && this.current?.document.id === view.document.id) await this.open(view.document.id, { saved: m.serial, savedRevision: view.revision });
        else this.send({ type: 'saved', id: view.document.id, revision: view.revision, serial: m.serial });
        break;
      }
      case 'publish': {
        if (typeof m.id !== 'string' || typeof m.revision !== 'string' || !this.working) return;
        const pending = await call<{ unsubmitted_documents: string[]; unsubmitted_mappings: string[] }>(this.root,['worktree']);
        if (pending.unsubmitted_documents.some(id => id !== m.id) || pending.unsubmitted_mappings.length) throw new Error('This branch has other committed changes. Inspect and submit the branch with the Lore CLI before publishing this document.');
        const committed=await call<View>(this.root,['commit',m.id,'--if-match',m.revision]);
        await call(this.root,['push']);
        const current=await call<View>(this.root,['show',m.id]);
        if (current.revision===committed.revision) this.workingDocuments.delete(m.id);
        this.changed(); await this.open(m.id); break;
      }
      case 'discard': {
        if (typeof m.id !== 'string') return;
        const confirmation = await vscode.window.showWarningMessage('Discard this Lore editor draft?', { modal: true }, 'Discard');
        if (confirmation === 'Discard') this.send({ type: 'discarded', id: m.id });
        break;
      }
      case 'external': {
        if (typeof m.url !== 'string') return;
        const url = vscode.Uri.parse(m.url);
        if (['https', 'http', 'mailto'].includes(url.scheme)) await vscode.env.openExternal(url);
        else this.send({ type: 'error', message: 'Only HTTP, HTTPS, and mail links can be opened externally.' });
        break;
      }
      case 'route': {
        if (typeof m.target !== 'string' || !this.current) return;
        const graph = await call<Connections>(this.root, ['links', this.current.document.id, '--revision', this.current.revision]);
        const link = graph.outgoing.find(l => l.target === m.target);
        if (!link?.matches.length) { this.send({ type: 'error', message: 'Linked document is unavailable locally. Search or create a document; no server is connected.' }); return; }
        const target = link.matches.length === 1 ? link.matches[0] : await vscode.window.showQuickPick(link.matches.map(d => ({ label: d.title, description: d.id, id: d.id })), { placeHolder: 'Multiple matching titles. Choose a document.' });
        if (target) await this.open(target.id); break;
      }
      case 'archive': {
        if (typeof m.id !== 'string' || typeof m.revision !== 'string') return;
        let view = await call<View>(this.root, this.working ? ['show', m.id] : ['--central','show',m.id]);
        if (view.revision !== m.revision) throw new Error('Document changed. Reload before applying this action.');
        if (!this.working) { await call(this.root,['pull']); view=await call<View>(this.root,['checkout',m.id]); }
        await call(this.root, [view.document.archived ? 'restore' : 'archive', m.id, '--if-match', view.revision]);
        this.workingDocuments.add(m.id);
        this.changed(); if (this.surface.active(this) && this.current?.document.id === m.id) await this.open(m.id); break;
      }
      case 'history': {
        if (typeof m.id !== 'string') return;
        const history = await call<{ revision: string; value: { at_ms: number; actor: string } }[]>(this.root, this.working ? ['history', m.id] : ['--central','history',m.id]);
        const chosen = await vscode.window.showQuickPick(history.map(r => ({ label: new Date(r.value.at_ms).toLocaleString(), description: `${r.value.actor} · ${r.revision.slice(0, 12)}`, revision: r.revision })), { placeHolder: 'Read a previous revision (read only)' });
        if (chosen) await this.open(m.id, { historical: chosen.revision }); break;
      }
    }
  }
  dispose(): void { this.disposed = true; }
}
