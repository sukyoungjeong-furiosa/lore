#!/usr/bin/env python3
"""Optional POSIX Codex adapter; Lore itself exposes ordinary discovery operations."""
import argparse
import contextlib
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import time


CLI = os.environ.get('LORE_EXECUTABLE', 'lore')
LIMIT = 8


def home():
    if os.environ.get('LORE_HOME'):
        return Path(os.environ['LORE_HOME']).resolve()
    if os.environ.get('XDG_DATA_HOME'):
        return Path(os.environ['XDG_DATA_HOME']).resolve() / 'lore'
    return Path.home() / '.local/share/lore'


def digest(value):
    return hashlib.sha256(value.encode()).hexdigest()


def atomic_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    descriptor, temporary = tempfile.mkstemp(dir=path.parent)
    try:
        with os.fdopen(descriptor, 'w') as stream:
            json.dump(value, stream, ensure_ascii=False)
        os.replace(temporary, path)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


@contextlib.contextmanager
def locked(path, nonblocking=False):
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open('a+') as stream:
        try:
            fcntl.flock(stream, fcntl.LOCK_EX | (fcntl.LOCK_NB if nonblocking else 0))
        except BlockingIOError:
            yield False
            return
        try:
            yield True
        finally:
            fcntl.flock(stream, fcntl.LOCK_UN)


def run(repo, *args, timeout=3):
    completed = subprocess.run([CLI, '--repo', str(repo), '--json', *args],
                               capture_output=True, timeout=timeout, check=False)
    if completed.returncode:
        # Do not propagate tool output, configuration values, or credentials into context.
        raise RuntimeError('Lore operation failed: ' + ' '.join(args[:2]))
    return json.loads(completed.stdout)


def git_root(path):
    path = Path(path)
    while not path.is_dir() and path != path.parent:
        path = path.parent
    result = subprocess.run(['git', '-C', str(path), 'rev-parse', '--show-toplevel'],
                            capture_output=True, timeout=2, check=False)
    if result.returncode:
        return None
    return Path(os.fsdecode(result.stdout).strip()).resolve()


def prepare(repo):
    return run(repo, 'init')['workspace']


def refresh_directory(repository):
    return home() / 'integrations/codex/refresh' / digest(repository)


def schedule_refresh(repo, repository):
    if os.environ.get('LORE_CODEX_REFRESH', '1') == '0':
        return
    directory = refresh_directory(repository)
    with locked(directory / 'schedule.lock'):
        state_path = directory / 'scheduled.json'
        state = json.loads(state_path.read_text()) if state_path.exists() else {}
        if time.time() - state.get('scheduled_at', 0) < 300:
            return
        subprocess.Popen([sys.executable, str(Path(__file__).resolve()), '--refresh', '--repo', str(repo)],
                         stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                         start_new_session=True)
        atomic_json(state_path, {'scheduled_at': time.time()})


def refresh(repo):
    state = prepare(repo)
    directory = refresh_directory(state['repository_id'])
    with locked(directory / 'worker.lock', nonblocking=True) as acquired:
        if not acquired:
            return
        results = []
        operations = [('mappings', ['map', 'fetch', '--max-age-seconds', '300'], 15)]
        for backend in run(repo, 'backend', 'list'):
            operations.append((backend, ['docs', 'fetch', backend, '--max-age-seconds', '900'], 900))
        for source, args, timeout in operations:
            try:
                value = run(repo, *args, timeout=timeout)
                complete = value.get('value', {}).get('complete', True)
                results.append({'source': source, 'status': 'refreshed' if complete else 'partial'})
            except (RuntimeError, subprocess.TimeoutExpired):
                results.append({'source': source, 'status': 'failed'})
            atomic_json(directory / 'result.json', {'completed_at': time.time(), 'results': results})


def path_mentions(value):
    found = []
    budget = 64 * 1024

    def add(path):
        if isinstance(path, str) and len(path.encode()) <= 1024 and path not in found and len(found) < 32:
            found.append(path)

    def explicit(value):
        if isinstance(value, str):
            add(value)
        elif isinstance(value, list):
            for child in value:
                explicit(child)

    def visit(value):
        nonlocal budget
        if len(found) >= 32 or budget <= 0:
            return
        if isinstance(value, str):
            text = value.encode()[:budget].decode(errors='ignore')
            budget -= len(text.encode())
            for word in re.split(r'''[\s"'`<>()\[\]{};,=|]+''', text):
                word = word.removeprefix('file://').split(':', 1)[0]
                if '/' in word or '.' in word:
                    add(word)
        elif isinstance(value, list):
            for child in value:
                visit(child)
        elif isinstance(value, dict):
            for key, child in value.items():
                if key in ('path', 'file_path', 'filename', 'paths', 'uri'):
                    explicit(child)
                else:
                    visit(child)

    visit(value)
    return found


def paths_by_root(event):
    cwd = Path(event['cwd']).resolve()
    tool_input = event.get('tool_input') or {}
    override = (tool_input.get('workdir') or tool_input.get('cwd')) if isinstance(tool_input, dict) else None
    base = (cwd / override).resolve() if isinstance(override, str) else cwd
    root = git_root(base)
    roots = [root] if root else []
    groups = {}
    for mention in path_mentions([tool_input, event.get('tool_response')]):
        path = Path(mention.removeprefix('file://'))
        if '..' in path.parts:
            continue
        path = path if path.is_absolute() else base / path
        # A lexical mention is not proof of file access; never inspect its contents.
        matched = next((root for root in roots if path.is_relative_to(root)), None)
        if matched is None and len(roots) < 4:
            matched = git_root(path)
            if matched and matched not in roots:
                roots.append(matched)
        if matched is None or not path.is_relative_to(matched):
            continue
        relative = path.relative_to(matched).as_posix()
        if relative == '.' or relative.startswith('/') or '\\' in relative:
            continue
        groups.setdefault(matched, set()).add(relative)
    return groups


def introduce(documents, session, limit):
    directory = home() / 'integrations/codex/sessions' / digest(session)
    with locked(directory / 'session.lock'):
        path = directory / 'introduced.json'
        seen = {json.dumps(reference, sort_keys=True, separators=(',', ':')) for reference in json.loads(path.read_text())} if path.exists() else set()
        selected, remaining = [], 6000
        for document in documents:
            identity = json.dumps(document['document'], sort_keys=True, separators=(',', ':'))
            if identity in seen:
                continue
            item = dict(document)
            item['matches'] = item['matches'][:3]
            item['mappings'] = item.get('mappings', [])[:3]
            if item.get('title'):
                item['title'] = item['title'][:160]
            size = len(json.dumps(item, ensure_ascii=False).encode())
            while size > remaining and len(item['matches']) > 1:
                item['matches'].pop()
                size = len(json.dumps(item, ensure_ascii=False).encode())
            if size > remaining:
                continue
            remaining -= size
            seen.add(identity)
            selected.append(item)
            if len(selected) == limit:
                break
        if selected:
            # Claim before emission: at most once, with best-effort delivery after interruptions.
            atomic_json(path, [json.loads(reference) for reference in sorted(seen)])
        return selected


def handle(event, session=None, limit=LIMIT):
    event_name = event.get('hook_event_name')
    if event_name == 'SessionStart':
        root = git_root(event['cwd'])
        if root:
            state = prepare(root)
            schedule_refresh(root, state['repository_id'])
        return None
    if event_name != 'PostToolUse':
        return None
    identity = session or json.dumps([event.get('session_id'), event.get('agent_id')])
    if not event.get('session_id') or len(identity.encode()) > 4096:
        raise ValueError('A stable agent session identity is required')
    documents = []
    exclusions = home() / 'integrations/codex/sessions' / digest(identity) / 'introduced.json'
    for repo, paths in paths_by_root(event).items():
        state = prepare(repo)
        schedule_refresh(repo, state['repository_id'])
        args = ['context', 'discover', '--cached']
        if exclusions.exists():
            args.extend(['--exclude-file', str(exclusions)])
        result = run(repo, *args, *sorted(paths))
        documents.extend(result['documents'])
        if result.get('warnings'):
            print('Lore: shared discovery index unavailable; refresh is scheduled.', file=sys.stderr)
    selected = introduce(documents, identity, limit) if documents else []
    if not selected:
        return None
    context = 'Related Lore references (discovery data; recorded anchor lines are not projected):\n'
    context += json.dumps(selected, ensure_ascii=False, separators=(',', ':'))
    return {'hookSpecificOutput': {'hookEventName': 'PostToolUse', 'additionalContext': context}}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--session')
    parser.add_argument('--limit', type=int, default=LIMIT, choices=range(1, 21))
    parser.add_argument('--refresh', action='store_true')
    parser.add_argument('--repo', type=Path)
    args = parser.parse_args()
    try:
        if args.refresh:
            if args.repo is None:
                parser.error('--refresh requires --repo')
            refresh(args.repo)
            return
        payload = sys.stdin.buffer.read(1024 * 1024 + 1)
        if len(payload) > 1024 * 1024:
            raise ValueError('Hook event exceeds 1 MB')
        result = handle(json.loads(payload), args.session, args.limit)
        if result:
            print(json.dumps(result, ensure_ascii=False))
    except (OSError, ValueError, KeyError, RuntimeError, subprocess.TimeoutExpired):
        print('Lore context hook failed; explicit Lore discovery remains available.', file=sys.stderr)
        sys.exit(1)


if __name__ == '__main__':
    main()
