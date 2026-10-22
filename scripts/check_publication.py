#!/usr/bin/env python3
"""ROM-free publication guard: tracked worktree files, optionally reachable history.

Heuristics, not proof of provenance or a replacement for human review. Authored
JSON examples are allowed; captured data must never be disguised as examples.
Diagnostics deliberately omit matched content (which might be a secret).
"""

import argparse
from pathlib import Path, PurePosixPath
import re
import subprocess
import sys

PRIVATE_DIRS = frozenset({
    '.cache', '.pi', '.ssh', '.aws', 'roms', 'extracted', 'traces', 'captures',
    'screenshots', 'saves', 'savestates', 'logs', 'target', '__pycache__',
})
PROHIBITED_SUFFIXES = frozenset({
    '.z64', '.v64', '.n64', '.rom', '.bin', '.zip', '.7z', '.rar', '.tar',
    '.gz', '.xz', '.bz2', '.png', '.jpg', '.jpeg', '.webp', '.gif', '.bmp',
    '.wav', '.ogg', '.mp3', '.mp4', '.mkv', '.sav', '.sra', '.eep', '.fla',
    '.state', '.st', '.m64', '.log', '.dmp', '.core', '.pyc', '.o', '.so',
    '.dll', '.exe', '.elf', '.wasm', '.pem', '.key', '.p12', '.pfx',
})
# Match workstation profiles, not technical addresses, URLs, /usr or /path/to.
WORKSTATION_PATH = re.compile(
    r'(?<![\w:])/(?:home|Users)/[^\s/<>"\x27`]+(?:/|\b)'
    r'|(?<![\w:])/(?:root)/'
    r'|\b[A-Za-z]:[\\/](?:Users|Documents and Settings)[\\/][^\s\\/<>"\x27`]+',
    re.IGNORECASE,
)
SECRET_PATTERNS = (
    re.compile(r'-----BEGIN (?:RSA |EC |OPENSSH |DSA )?PRIVATE KEY-----'),
    re.compile(r'\b(?:gh[pousr]_[A-Za-z0-9]{30,}|github_pat_[A-Za-z0-9_]{40,})\b'),
    re.compile(r'\b(?:AKIA|ASIA)[A-Z0-9]{16}\b'),
    re.compile(r'\bxox[baprs]-[A-Za-z0-9-]{20,}\b'),
    re.compile(r'https?://[^\s/:@]+:[^\s/@]+@', re.IGNORECASE),
    re.compile(r'''(?ix)\b(?:api[_-]?key|access[_-]?token|client[_-]?secret|password)
        \s*[=:]\s*["']([A-Za-z0-9_+/=-]{20,})["']'''),
)


def check_name(name):
    """Return violation descriptions; never blanket-reject JSON/source fixtures."""
    path = PurePosixPath(name)
    parts = [part.lower() for part in path.parts]
    issues = []
    if path.is_absolute() or '..' in parts or WORKSTATION_PATH.search(name):
        issues.append('absolute or unsafe publication path')
    if any(part in PRIVATE_DIRS for part in parts[:-1]):
        issues.append('private/generated directory')
    base = parts[-1] if parts else ''
    if (path.suffix.lower() in PROHIBITED_SUFFIXES or base == '.env'
            or (base.startswith('.env.') and base != '.env.example')
            or base in {'id_rsa', 'id_ed25519', 'core'}
            or re.fullmatch(r'core\.\d+', base)):
        issues.append('prohibited artifact or credential filename')
    return issues


def check_content(data):
    """Inspect bytes without dumping sensitive matches into CI logs."""
    if b'\0' in data:
        return ['binary/NUL content (public fixtures must be authored text)']
    try:
        text = data.decode('utf-8')
    except UnicodeDecodeError:
        return ['non-UTF-8 artifact']
    issues = []
    if WORKSTATION_PATH.search(text):
        issues.append('workstation absolute path')
    if any(pattern.search(text) for pattern in SECRET_PATTERNS):
        issues.append('possible secret/embedded credential')
    return issues


def git(root, *args):
    return subprocess.run(['git', '-C', str(root), *args], check=True,
                          stdout=subprocess.PIPE, stderr=subprocess.PIPE).stdout


def scan(root, history=False):
    """Check tracked worktree contents; history adds all reachable commit trees.

    Untracked/ignored files are deliberately untouched. Git author identities and
    commit messages are not inspected or rewritten. History includes deleted paths
    and old blobs reachable through local branches/tags, not unreachable objects.
    """
    failures = []

    def report(label, issues):
        failures.extend(f'{label!r}: {issue}' for issue in issues)

    for raw in git(root, 'ls-files', '-z').split(b'\0'):
        if not raw:
            continue
        name = raw.decode('utf-8', errors='surrogateescape')
        report(name, check_name(name))
        path = root / name
        if path.is_symlink():
            report(name, ['symlink not allowed in public source'])
        elif path.is_file():
            report(name, check_content(path.read_bytes()))
        elif path.exists():
            report(name, ['non-file tracked entry (submodules are not scanned)'])
        # Missing tracked paths are local deletions, not publication contents.

    if history:
        seen_blobs = set()
        trees = set(git(root, 'log', '--all', '--format=%T').splitlines())
        for tree in sorted(trees):
            for entry in git(root, 'ls-tree', '-rz', tree.decode()).split(b'\0'):
                if not entry:
                    continue
                metadata, raw_name = entry.split(b'\t', 1)
                mode, kind, oid = metadata.split()
                name = raw_name.decode('utf-8', errors='surrogateescape')
                label = f'history {tree.decode()[:12]}:{name}'
                report(label, check_name(name))
                if kind != b'blob' or mode == b'120000':
                    report(label, ['non-regular history entry'])
                elif oid not in seen_blobs:
                    seen_blobs.add(oid)
                    report(label, check_content(git(root, 'cat-file', 'blob', oid.decode())))
    return failures


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--history', action='store_true',
                        help='also scan all reachable commit trees/blobs; no rewriting')
    args = parser.parse_args()
    try:
        root = Path(subprocess.run(['git', 'rev-parse', '--show-toplevel'],
                                  check=True, stdout=subprocess.PIPE,
                                  stderr=subprocess.PIPE).stdout.decode().strip())
        failures = scan(root, args.history)
    except (OSError, subprocess.CalledProcessError, ValueError) as exc:
        print(f'Publication check could not complete: {type(exc).__name__}', file=sys.stderr)
        return 2
    if failures:
        print('\n'.join(failures), file=sys.stderr)
        print(f'Publication check failed: {len(failures)} issue(s).', file=sys.stderr)
        return 1
    print('Publication check passed (heuristic; provenance review still required).')
    return 0


if __name__ == '__main__':
    sys.exit(main())
