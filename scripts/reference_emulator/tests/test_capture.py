"""Authored, ROM-/GPU-/emulator-free capture-tool tests."""
import contextlib
import copy
import hashlib
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from scripts.reference_emulator import capture, core, setup


class CaptureTests(unittest.TestCase):
    def setUp(self):
        setup.CACHE.mkdir(exist_ok=True)
        self.temporary = tempfile.TemporaryDirectory(prefix="capture-test-", dir=setup.CACHE)
        self.root = Path(self.temporary.name)
        self.addCleanup(self.temporary.cleanup)

    def json_file(self, value):
        path = self.root / "input.json"
        path.write_text(json.dumps(value))
        return path

    def test_normalization_is_content_based_and_revision_gate_is_separate(self):
        authored = bytes.fromhex("80371240") + bytes(range(60))
        for order in [2, 4]:
            swapped = b"".join(authored[i:i+order][::-1] for i in range(0, len(authored), order))
            self.assertEqual(capture.normalize_rom(swapped), authored)
        self.assertEqual(capture.normalize_rom(authored), authored)
        path = self.root / "authored.data"
        path.write_bytes(authored)
        with self.assertRaisesRegex(ValueError, "exact canonical"):
            capture.rom_bytes(path)
        self.assertEqual(path.read_bytes(), authored)
        for bad in [b"", authored[:63], authored + b"x", bytes(64)]:
            with self.assertRaises(ValueError):
                capture.normalize_rom(bad)

    def test_raw_schedule_ranges_endpoints_and_neutral_tail(self):
        path = self.json_file({"version": 1, "segments": [
            {"polls": 2, "buttons": 65535, "stick_x": -128, "stick_y": 127},
            {"polls": 1, "buttons": 32768, "stick_x": 0, "stick_y": 0}]})
        segments = capture.schedule_read(path)
        self.assertEqual(capture.input_at(segments, -1), {"buttons": 0, "stick_x": 0, "stick_y": 0})
        self.assertEqual(capture.input_at(segments, 1)["stick_x"], -128)
        self.assertEqual(capture.input_at(segments, 2)["buttons"], 32768)
        self.assertEqual(capture.input_at(segments, 3), {"buttons": 0, "stick_x": 0, "stick_y": 0})
        self.assertEqual(capture.schedule_read(None), [])
        valid = json.loads(path.read_text())
        for mutate in [
            lambda v: v.update(version=True),
            lambda v: v.update(unknown=0),
            lambda v: v["segments"][0].update(buttons=True),
            lambda v: v["segments"][0].update(buttons=65536),
            lambda v: v["segments"][0].update(stick_x=-129),
            lambda v: v["segments"][0].update(stick_y=128),
            lambda v: v["segments"][0].update(polls=0),
            lambda v: v["segments"][0].update(polls=100001),
            lambda v: v["segments"][0].update(extra=0),
        ]:
            value = copy.deepcopy(valid)
            mutate(value)
            with self.assertRaises(ValueError):
                capture.schedule_read(self.json_file(value))
        for value in [[], None, {"version": 1, "segments": "bad"}]:
            with self.assertRaises(ValueError):
                capture.schedule_read(self.json_file(value))

    def test_strict_json_and_bounded_reads(self):
        path = self.root / "input.json"
        for data in [b'{"a":1,"a":2}', b'{"a":NaN}', b'{"a":Infinity}', b'{']:
            path.write_bytes(data)
            with self.assertRaises(ValueError):
                capture.json_read(path)
        path.write_bytes(b" " * 128)
        with self.assertRaisesRegex(ValueError, "oversized"):
            capture.json_read(path, limit=64)

    def test_reports_never_overwrite_or_follow_symlinks(self):
        path = self.root / "report.json"
        capture.write_new(path, {"authored": True})
        before = path.read_bytes()
        with self.assertRaises(FileExistsError):
            capture.write_new(path, {"different": True})
        self.assertEqual(path.read_bytes(), before)
        link = self.root / "alias.json"
        link.symlink_to(path)
        with self.assertRaises(ValueError):
            capture.write_new(link, {})
        self.assertEqual(path.read_bytes(), before)
        bad = self.root / "invalid.json"
        with self.assertRaises(ValueError):
            capture.write_new(bad, {"bad": float("nan")})
        self.assertFalse(bad.exists())

    def test_manifest_hash_checks_precede_dynamic_loading(self):
        artifacts = {}
        for name in ("core", "rsp", "input", "video"):
            path = self.root / (name + ".synthetic")
            path.write_bytes(b"authored placeholder, not executable")
            artifacts[name] = setup.artifact(path)
        data = self.root / "data"
        data.mkdir()
        file = data / "synthetic.ini"
        file.write_text("authored")
        manifest = {"owner": setup.OWNER, "schema_version": 1, "artifacts": artifacts,
                    "core_data": str(data), "data_files": {file.name: setup.artifact(file)}}
        path = self.json_file(manifest)
        self.assertEqual(capture.checked_manifest(path), manifest)
        Path(artifacts["core"]["path"]).write_bytes(b"changed")
        with self.assertRaisesRegex(ValueError, "artifact no longer"):
            capture.checked_manifest(path)
        manifest["artifacts"]["core"]["sha256"] = hashlib.sha256(b"changed").hexdigest()
        self.json_file(manifest)
        file.write_text("changed")
        with self.assertRaisesRegex(ValueError, "data file no longer"):
            capture.checked_manifest(path)

    def test_diagnostics_use_declared_offsets_and_reject_invalid_controller(self):
        class Memory:
            def __init__(self):
                self.floor = 0x80002000
                self.controller = 0x80003000
                self.lib = self
            def u32(self, address):
                return {0x80001068: self.floor, 0x8000109C: self.controller}.get(address, 0)
            def i16(self, address):
                return address & 0x7FFF
            def f32(self, address):
                return float(address & 0xFFFF)
            def DebugMemRead16(self, address):
                return address & 0xFFFF
            def DebugMemRead8(self, address):
                return address & 0xFF
        memory = Memory()
        data = capture.diagnostics(memory, 0x80001000)
        self.assertEqual(data["floor"]["address"], memory.floor)
        self.assertEqual(data["floor"]["normal"], [8220.0, 8224.0, 8228.0])
        self.assertEqual(data["processed_stick"], [12292.0, 12296.0, 12300.0])
        self.assertEqual(data["floor_height"], 4208.0)
        memory.floor = 0
        self.assertIsNone(capture.diagnostics(memory, 0x80001000)["floor"])
        memory.controller = 0
        with self.assertRaisesRegex(ValueError, "controller pointer"):
            capture.diagnostics(memory, 0x80001000)
        with self.assertRaisesRegex(ValueError, "state pointer"):
            capture.diagnostics(memory, 0)

    def test_callback_errors_are_not_silently_ignored_by_ctypes(self):
        calls = []
        class Library:
            def CoreDoCommand(self, *args):
                calls.append(("command", *args))
                return 0
            def DebugSetRunState(self, state):
                calls.append(("resume", state))
                return 0
        instance = core.Core.__new__(core.Core)
        instance.lib = Library()
        instance.callback_error = None
        def failure():
            raise ValueError("authored failure")
        guarded = instance.guarded(failure)
        with contextlib.redirect_stderr(io.StringIO()):
            guarded()
        self.assertIsInstance(instance.callback_error, ValueError)
        self.assertEqual(calls, [("command", 6, 0, None), ("resume", 2)])
        # Initialization can fail before EMU_RUNNING. Subsequent callbacks must
        # retry stopping rather than silently leaving the emulator active/paused.
        guarded()
        self.assertEqual(len(calls), 4)


if __name__ == "__main__":
    unittest.main()
