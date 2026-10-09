import { spawn } from 'node:child_process';
import * as vscode from 'vscode';
export interface Anchor { id: string; path: string; start: number; end: number; context: { head?: string; working_tree: boolean } }
export interface Document { id: string; title: string; content: string; content_format: string; tags: string[]; authorship: string[]; archived: boolean; anchors: Anchor[] }
export interface View { revision: string; document: Document; curated: boolean }
export interface Location { path: string; start: number; end: number }
export interface Projection { anchor: string; locations: Location[]; alternatives: Location[][]; explanation: string }
export interface Located { view: View; projections: Projection[] }
export interface Connections { outgoing: { target: string; label: string; matches: { id: string; title: string; archived: boolean }[] }[]; backlinks: { id: string; title: string; archived: boolean }[] }
export interface DocumentReference { backend: string; id: string }
export interface BackendDocument { reference: DocumentReference; title: string; url: string; observed_revision?: string }
export interface Retrieved<T> { cached: boolean; fetched_at_ms: number; value: T }
export interface DocumentListing { documents: BackendDocument[]; warnings: string[] }
export interface DocumentContent { document: BackendDocument; content: string; content_format: string; partial: boolean; warnings: string[] }
export interface MappingView { revision: string; record: { actor: string; at_ms: number; mapping: { id: string; document: DocumentReference; context: string; archived: boolean; anchors: Anchor[]; scopes?: { repository: string; glob: string }[] } } }
export interface LocatedMapping { view: MappingView; projections: Projection[] }
export interface BackendConfiguration { kind: string; roots: string[]; token_env: string; api_url: string }
let secretStorage: vscode.SecretStorage | undefined;
export function configureSecrets(storage: vscode.SecretStorage): void { secretStorage = storage; }
const migrations = new Map<string, Promise<void>>();
export async function migrateBackendTokens(root: string): Promise<void> {
  if (!secretStorage) return;
  const configs = await call<Record<string, BackendConfiguration>>(root, ['backend', 'list']);
  const home = vscode.workspace.getConfiguration('lore', vscode.Uri.file(root)).get<string>('dataHome') || process.env.LORE_HOME || '';
  for (const backend of Object.keys(configs)) {
    const key = `${home}:${backend}`;
    let migration = migrations.get(key);
    if (!migration) {
      migration = (async () => {
        const legacyKey = `lore.backend.${backend}.token`;
        const token = await secretStorage!.get(legacyKey);
        if (!token) return;
        const status = await call<{ stored: boolean }>(root, ['backend', 'auth', 'status', backend]);
        if (!status.stored) await call(root, ['backend', 'auth', 'set', backend, '--stdin'], token);
        await secretStorage!.delete(legacyKey);
      })();
      migrations.set(key, migration);
      migration.catch(() => migrations.delete(key));
    }
    await migration;
  }
}
export class LoreError extends Error {
  constructor(public readonly kind: string, message: string, public readonly result?: unknown) { super(message); }
}
export async function call<T>(root: string, args: string[], input?: string): Promise<T> {
  const config = vscode.workspace.getConfiguration('lore', vscode.Uri.file(root));
  const env = { ...process.env };
  if (config.get<string>('dataHome')) env.LORE_HOME = config.get<string>('dataHome');
  if (args[0] === 'docs') await migrateBackendTokens(root);
  return new Promise((resolve, reject) => {
    const child = spawn(config.get<string>('executable') || 'lore', ['--repo', root, '--json', '--author', 'human', ...args], { env, windowsHide: true });
    let out = '', error = ''; let size = 0;
    const timeout = args[0] === 'docs' ? (['index', 'fetch'].includes(args[1]) ? 15 * 60 * 1000 : 120000) : 30000;
    const timer = setTimeout(() => { child.kill(); reject(new LoreError('unavailable', 'Lore CLI timed out')); }, timeout);
    child.stdout.on('data', chunk => { size += chunk.length; if (size > 32 * 1024 * 1024) { child.kill(); reject(new LoreError('unsupported', 'Lore result exceeds 32 MB')); } else out += chunk; });
    child.stderr.on('data', chunk => { error += chunk; });
    child.on('error', e => { clearTimeout(timer); reject(new LoreError('unavailable', `Cannot run Lore CLI: ${e.message}`)); });
    child.on('close', code => {
      clearTimeout(timer);
      if (code !== 0) {
        try { const parsed = JSON.parse(error).error; reject(new LoreError(parsed.kind, parsed.message)); }
        catch {
          let result: unknown; try { result = JSON.parse(out); } catch { /* Partial JSON is not a completed result. */ }
          reject(new LoreError('unavailable', error.trim() || (result ? 'Some items could not be fetched; successful items remain cached.' : 'Lore CLI failed'), result));
        }
      } else {
        try { resolve(JSON.parse(out) as T); }
        catch { reject(new LoreError('corrupt', 'Lore CLI returned invalid JSON')); }
      }
    });
    child.stdin.on('error', () => { /* Process errors are reported through the exit result. */ });
    child.stdin.end(input);
  });
}
