"""Only synthetic authored data; no ROMs, credentials or captured traces."""

import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location(
    'check_publication', Path(__file__).resolve().parents[1] / 'check_publication.py')
checker = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(checker)


class RulesTests(unittest.TestCase):
    def test_private_names_case_insensitive(self):
        for name in ['.cache/report.json', 'nested/ROMS/image.Z64',
                     'traces/reference.json', '.pi/session.json',
                     'screenshots/view.PNG', 'save.SRA', 'archive.zip',
                     '.env', '.env.local', 'id_ed25519', 'core.123']:
            with self.subTest(name=name):
                self.assertTrue(checker.check_name(name))

    def test_authored_examples_and_source_not_hidden(self):
        for name in ['docs/examples/authored-trace.json',
                     'docs/examples/authored-movement.json',
                     'docs/examples/castle-walk-jump-inputs.json',
                     'crates/example/tests/fixtures/literals.json',
                     'Cargo.lock', 'LICENSE-MIT', 'scripts/check_publication.py',
                     'scripts/reference_emulator/core.py', '.env.example']:
            with self.subTest(name=name):
                self.assertEqual(checker.check_name(name), [])

    def test_workstation_paths(self):
        # Assemble fake profile paths so these tests remain publishable themselves.
        for value in ['/' + 'home/synthetic/rom.z64',
                      '/' + 'Users/synthetic/input', '/' + 'root/private',
                      'C:' + '\\Users\\synthetic\\input',
                      'D:/' + 'Users/synthetic/input']:
            with self.subTest(style=value[:3]):
                self.assertIn('workstation absolute path',
                              checker.check_content(value.encode()))

    def test_safe_technical_metadata(self):
        text = '''0x8033b170 0x15000458 -8500,6500,11000
https://github.com/n64decomp/sm64/blob/9921382a68bb0c865e5e45eb594d9c64db59b1af/sm64.us.sha1
https://www.apache.org/licenses/LICENSE-2.0
export SM64_ROM="/path/to/your/rom.z64"
/usr/bin/python3 .cache/reference-mio0/run-<unique-id>/
{"version":1,"stick_x":0,"stick_y":80,"buttons":0}
SHA-256: 17ce077343c6133f8c9f2d6d6d9a4ab62c8cd2aa57c40aea1f490b4c8bb21d91
'''
        self.assertEqual(checker.check_content(text.encode()), [])

    def test_synthetic_secrets(self):
        for value in ['gh' + 'p_' + 'x' * 36,
                      'github_' + 'pat_' + 'x' * 50,
                      'AK' + 'IA' + 'Z' * 16,
                      'xox' + 'b-' + '0' * 24,
                      '-----BEGIN ' + 'PRIVATE KEY-----',
                      'https://' + 'synthetic:credential@example.invalid/',
                      'api_key="' + 'x' * 24 + '"']:
            self.assertTrue(checker.check_content(value.encode()))

    def test_checker_and_tests_are_publishable(self):
        for path in [Path(SPEC.origin), Path(__file__)]:
            self.assertEqual(checker.check_content(path.read_bytes()), [])

    def test_showcase_exception_is_exact_and_requires_png(self):
        name = 'docs/images/showcase.png'
        data = b'\x89PNG\r\n\x1a\n' + b'\0synthetic'
        self.assertEqual(checker.check_name(name), [])
        self.assertEqual(checker.check_content(data, name), [])
        self.assertTrue(checker.check_content(b'not a PNG', name))
        for other in ['docs/images/other.png', 'other/showcase.png',
                      'docs/images/showcase.PNG', '.cache/showcase.png']:
            self.assertTrue(checker.check_name(other))
            self.assertTrue(checker.check_content(data, other))

    def test_binary_and_invalid_utf8(self):
        self.assertTrue(checker.check_content(bytes([0x80, 0x37, 0x12, 0x40])))
        self.assertTrue(checker.check_content(b'authored\0binary'))


class RepositoryTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.git('init', '-q')
        # Synthetic identity in isolated test repository, never global config.
        self.git('config', 'user.name', 'Synthetic Test')
        self.git('config', 'user.email', 'test@example.invalid')
        self.git('config', 'commit.gpgsign', 'false')

    def git(self, *args):
        return subprocess.run(['git', '-C', str(self.root), *args], check=True,
                              stdout=subprocess.PIPE, stderr=subprocess.PIPE).stdout

    def put(self, name, content):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content)

    def test_tracked_worktree_not_untracked_or_ignore_rules(self):
        self.put('.gitignore', '.cache/\n')
        self.put('docs/examples/authored-trace.json', '{"version":1}\n')
        self.git('add', '.')
        self.put('.cache/private.json', '/' + 'home/synthetic/private')
        self.assertEqual(checker.scan(self.root), [])
        self.put('docs/examples/authored-trace.json', '/' + 'home/synthetic/private')
        self.assertTrue(checker.scan(self.root))  # unstaged tracked content
        self.put('.cache/forced.json', '{}')
        self.git('add', '-f', '.cache/forced.json')
        self.assertTrue(any('private/generated' in issue for issue in checker.scan(self.root)))

    def test_history_includes_deleted_files_and_old_content(self):
        self.put('notes.md', '/' + 'home/synthetic/private')
        self.put('traces/deleted.json', '{}')
        self.git('add', '.')
        self.git('commit', '-qm', 'Synthetic unsafe snapshot')
        self.put('notes.md', 'Authored safe notes\n')
        self.git('rm', 'traces/deleted.json')
        self.git('add', 'notes.md')
        self.git('commit', '-qm', 'Synthetic clean snapshot')
        self.assertEqual(checker.scan(self.root), [])
        failures = checker.scan(self.root, history=True)
        self.assertTrue(any('private/generated' in issue for issue in failures))
        self.assertTrue(any('workstation absolute' in issue for issue in failures))
        # Read-only checker must not alter history or author attribution.
        before = self.git('log', '--format=%H %an %ae')
        checker.scan(self.root, history=True)
        self.assertEqual(before, self.git('log', '--format=%H %an %ae'))

    def test_showcase_in_worktree_and_history(self):
        # Synthetic signature-only payload, not game-derived imagery.
        data = b'\x89PNG\r\n\x1a\n' + b'\0synthetic'
        path = self.root / 'docs/images/showcase.png'
        path.parent.mkdir(parents=True)
        path.write_bytes(data)
        self.git('add', '.')
        self.git('commit', '-qm', 'Synthetic showcase exception')
        self.assertEqual(checker.scan(self.root, history=True), [])
        # The same blob at a non-allowlisted path must still fail history checks.
        (self.root / 'other.png').write_bytes(data)
        self.git('add', '.')
        self.git('commit', '-qm', 'Synthetic prohibited duplicate')
        self.git('rm', 'other.png')
        self.git('commit', '-qm', 'Remove synthetic duplicate')
        self.assertEqual(checker.scan(self.root), [])
        failures = checker.scan(self.root, history=True)
        self.assertTrue(any('binary/NUL' in issue for issue in failures))

    def test_tracked_symlink_is_rejected_without_reading_target(self):
        (self.root / 'link.md').symlink_to('missing-private-target')
        self.git('add', 'link.md')
        self.assertTrue(any('symlink' in issue for issue in checker.scan(self.root)))

    def test_git_failure_is_not_a_clean_scan(self):
        with tempfile.TemporaryDirectory() as other:
            with self.assertRaises(subprocess.CalledProcessError):
                checker.scan(Path(other))


if __name__ == '__main__':
    unittest.main()
