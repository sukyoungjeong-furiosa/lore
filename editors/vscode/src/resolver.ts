import * as vscode from 'vscode';
import { call } from './client';

interface ModelConnection { base_url: string; token_env: string }
interface Profile { connection: string; model: string }
interface Settings { connections: Record<string, ModelConnection>; profiles: Record<string, Profile>; default_profile?: string }
type Register = (name: string, handler: (...args: any[]) => Promise<unknown> | unknown) => void;

const furiosa = {
  url: 'https://endpoint.access.furiosa.dev/v1',
  model: 'furiosa-ai/K-EXAONE-236B-A23B-NVFP4A16',
  tokenEnv: 'FURIOSA_ACCESS_API_KEY',
};

function validateName(value: string): string | undefined {
  return /^[a-zA-Z0-9_-]{1,100}$/.test(value) ? undefined : 'Use letters, digits, hyphens, or underscores (at most 100 characters).';
}

export function registerResolverCommands(register: Register, rootFor: (path?: string) => Promise<string>): void {
  register('lore.configureResolver', async () => {
    const root = await rootFor();
    const settings = await call<Settings>(root, ['resolver', 'list']);
    const choice = await vscode.window.showQuickPick([
      ...Object.entries(settings.profiles).map(([name, profile]) => ({ label: name, description: `${profile.model}${name === settings.default_profile ? ' · default' : ''}`, name })),
      { label: 'New resolver profile…', name: undefined },
    ], { placeHolder: 'Configure a model resolver profile' });
    if (!choice) return;
    const name = choice.name || await vscode.window.showInputBox({ prompt: 'Resolver profile name', value: Object.keys(settings.profiles).length ? '' : 'furiosa', validateInput: value => validateName(value) || (settings.profiles[value] ? 'This profile already exists.' : undefined) });
    if (!name) return;
    const previous = settings.profiles[name];
    const connectionChoice = await vscode.window.showQuickPick([
      ...Object.entries(settings.connections).sort(([a], [b]) => Number(b === previous?.connection) - Number(a === previous?.connection)).map(([id, connection]) => ({ label: id, description: connection.base_url, name: id })),
      { label: 'New model connection…', name: undefined },
    ], { placeHolder: 'Select the model connection for this profile' });
    if (!connectionChoice) return;
    let connection = connectionChoice.name;
    let url: string | undefined, tokenEnv: string | undefined, token: string | undefined;
    if (!connection) {
      connection = await vscode.window.showInputBox({ prompt: 'Model connection name', value: Object.keys(settings.connections).length ? '' : 'furiosa', validateInput: value => validateName(value) || (settings.connections[value] ? 'This connection already exists.' : undefined) });
      if (!connection) return;
      url = await vscode.window.showInputBox({ prompt: 'OpenAI-compatible API base URL (without /chat/completions)', value: furiosa.url, validateInput: value => {
        try {
          const parsed = new URL(value);
          if (['https:', 'http:'].includes(parsed.protocol) && !parsed.username && !parsed.password && !parsed.search && !parsed.hash) return undefined;
        } catch { /* The input remains editable until the URL is valid. */ }
        return 'Use an HTTP(S) URL without credentials, query parameters, or a fragment.';
      } });
      if (!url) return;
      tokenEnv = await vscode.window.showInputBox({ prompt: 'Environment variable that overrides the saved token', value: url.replace(/\/$/, '') === furiosa.url ? furiosa.tokenEnv : 'LORE_MODEL_API_KEY', validateInput: value => /^[a-zA-Z0-9_]+$/.test(value) ? undefined : 'Use letters, digits, or underscores.' });
      if (!tokenEnv) return;
    }
    const model = await vscode.window.showInputBox({ prompt: 'Model ID', value: previous?.model || (url?.replace(/\/$/, '') === furiosa.url || settings.connections[connection]?.base_url === furiosa.url ? furiosa.model : ''), validateInput: value => value.trim() && !/[\x00-\x1f\x7f]/.test(value) ? undefined : 'Enter a model ID.' });
    if (!model) return;
    if (url) {
      token = await vscode.window.showInputBox({ prompt: 'API token. Leave blank to use the environment variable. Saved on the host running Lore CLI.', password: true, ignoreFocusOut: true });
      if (token === undefined) return;
      await call(root, ['resolver', 'connection', 'set', connection, '--url', url, '--token-env', tokenEnv!]);
      if (token) await call(root, ['resolver', 'auth', 'set', connection, '--stdin'], token);
    }
    await call(root, ['resolver', 'profile', 'set', name, '--connection', connection, '--model', model]);
    await call(root, ['resolver', 'use', name]);
    void vscode.window.showInformationMessage(`Lore: ${name} is the default for model-assisted projection.`);
  });

  register('lore.selectResolver', async () => {
    const root = await rootFor();
    const settings = await call<Settings>(root, ['resolver', 'list']);
    if (!Object.keys(settings.profiles).length) { await vscode.commands.executeCommand('lore.configureResolver'); return; }
    const choice = await vscode.window.showQuickPick(Object.entries(settings.profiles).map(([name, profile]) => ({ label: name, description: `${profile.model}${name === settings.default_profile ? ' · default' : ''}`, name })), { placeHolder: 'Select the default model resolver' });
    if (choice) await call(root, ['resolver', 'use', choice.name]);
  });

  register('lore.setResolverToken', async (name?: string, path?: string) => {
    const root = await rootFor(path);
    const settings = await call<Settings>(root, ['resolver', 'list']);
    if (!Object.keys(settings.connections).length) { await vscode.commands.executeCommand('lore.configureResolver'); return; }
    const selected = name ? { name } : await vscode.window.showQuickPick(Object.entries(settings.connections).map(([name, connection]) => ({ label: name, description: connection.base_url, name })), { placeHolder: 'Select the model connection to authenticate' });
    if (!selected) return;
    const token = await vscode.window.showInputBox({ prompt: `API token for ${selected.name}. Shared with Lore CLI and agents on the host running Lore CLI.`, password: true, ignoreFocusOut: true });
    if (token) await call(root, ['resolver', 'auth', 'set', selected.name, '--stdin'], token);
  });
}
