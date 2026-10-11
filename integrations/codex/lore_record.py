#!/usr/bin/env python3
"""Request one recording assessment per turn in an initialized Lore workspace."""
import argparse
import json
from pathlib import Path
import subprocess
import sys

from lore_context import atomic_json, digest, git_root, home, locked, run


DEFAULT_SKILL = Path(__file__).resolve().parents[2] / 'examples/skills/lore-record-ko/SKILL.md'


def handle(event, skill=DEFAULT_SKILL):
    if event.get('hook_event_name') != 'Stop' or event.get('stop_hook_active'):
        return {}
    session, turn = event.get('session_id'), event.get('turn_id')
    if not all(isinstance(value, str) and 0 < len(value.encode()) <= 4096
               for value in (session, turn)):
        return {}
    cwd = event.get('cwd')
    if not isinstance(cwd, str) or not cwd:
        return {}
    root = git_root(cwd)
    if root is None:
        return {}
    skill = skill.resolve()
    if not skill.is_file():
        raise ValueError('Recording skill is unavailable')
    # An absent workspace or CLI must not turn an ordinary Stop into setup work.
    try:
        run(root, 'status', timeout=2)
    except (OSError, RuntimeError, subprocess.TimeoutExpired):
        return {}
    directory = home() / 'integrations/codex/sessions' / digest(session) / 'recording'
    with locked(directory / 'checkpoint.lock'):
        checkpoint = directory / (digest(turn) + '.json')
        if checkpoint.exists():
            return {}
        reason = (
            f'Lore recording checkpoint. Starting checkout: {json.dumps(str(root))}. '
            f'Read the recording skill at {json.dumps(str(skill))} and apply it to '
            'the completed work. Decide whether there is new knowledge worth recording; '
            'finish after this assessment. If there is nothing new to record, finish quietly.'
        )
        # Claim before emission; interrupted delivery may skip this assessment.
        atomic_json(checkpoint, {'requested': True})
        return {'decision': 'block', 'reason': reason}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--skill', type=Path, default=DEFAULT_SKILL)
    args = parser.parse_args()
    result = {}
    try:
        payload = sys.stdin.buffer.read(1024 * 1024 + 1)
        if len(payload) > 1024 * 1024:
            raise ValueError('Hook event exceeds 1 MB')
        event = json.loads(payload)
        if not isinstance(event, dict):
            raise ValueError('Hook event must be an object')
        result = handle(event, args.skill)
    except (OSError, ValueError, KeyError, RuntimeError, subprocess.TimeoutExpired):
        print('Lore recording checkpoint unavailable; explicit recording remains available.', file=sys.stderr)
    print(json.dumps(result))


if __name__ == '__main__':
    main()
