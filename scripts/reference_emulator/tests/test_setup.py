"""Synthetic setup tests: no network, compiler, emulator, ROM, or GPU required."""
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import tarfile
import tempfile
import time
import unittest
from unittest.mock import Mock, patch


SPEC = importlib.util.spec_from_file_location("reference_setup", Path(__file__).parents[1] / "setup.py")
setup = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(setup)


class SetupTests(unittest.TestCase):
    def setUp(self):
        setup.CACHE.mkdir(exist_ok=True)
        self.temp = tempfile.TemporaryDirectory(prefix="setup-test-", dir=setup.CACHE)
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)

    def archive(self, members=None, key="core"):
        source = dict(setup.SOURCES[key])
        top = f"{source['repo']}-{source['commit']}"
        target = self.root / source["archive"]
        with tarfile.open(target, "w:gz") as tar:
            for name, kind in members or [(top + "/src/example.c", "file")]:
                member = tarfile.TarInfo(name.replace("TOP", top))
                if kind == "file":
                    member.size = 3
                    tar.addfile(member, io.BytesIO(b"abc"))
                else:
                    member.type = {"symlink": tarfile.SYMTYPE, "hardlink": tarfile.LNKTYPE,
                                   "device": tarfile.CHRTYPE, "fifo": tarfile.FIFOTYPE}[kind]
                    member.linkname = "../../outside"
                    tar.addfile(member)
        source["sha256"] = hashlib.sha256(target.read_bytes()).hexdigest()
        return target, source

    def test_hash_mismatch_before_extraction(self):
        archive, source = self.archive()
        source["sha256"] = "0" * 64
        with self.assertRaisesRegex(ValueError, "SHA256"):
            setup.extract_source(archive, self.root / "extracted", source)
        self.assertFalse((self.root / "extracted").exists())

    def test_reject_unsafe_members_before_any_extraction(self):
        cases = [("TOP/../../escape", "file"), ("/absolute", "file"),
                 ("wrong-root/file", "file"), ("TOP/link", "symlink"),
                 ("TOP/link", "hardlink"), ("TOP/dev", "device"),
                 ("TOP/fifo", "fifo")]
        for member in cases:
            with self.subTest(member=member):
                archive, source = self.archive([("TOP/good", "file"), member])
                with self.assertRaisesRegex(ValueError, "unsafe archive"):
                    setup.extract_source(archive, self.root / "extracted", source)
                self.assertFalse((self.root / "extracted").exists())

    def test_fresh_extraction_and_no_overwrite(self):
        archive, source = self.archive()
        destination = self.root / "extracted"
        tree = setup.extract_source(archive, destination, source)
        self.assertEqual((tree / "src/example.c").read_bytes(), b"abc")
        (tree / "src/example.c").write_text("modified")
        with self.assertRaises(FileExistsError):
            setup.extract_source(archive, destination, source)
        self.assertEqual((tree / "src/example.c").read_text(), "modified")

    def test_outside_cache_and_symlinks(self):
        with self.assertRaisesRegex(ValueError, "below"):
            setup.prepare_root(setup.PROJECT / "outside")
        with self.assertRaisesRegex(ValueError, "below"):
            setup.prepare_root(setup.CACHE / "../outside")
        link = self.root / "link"
        link.symlink_to(self.root, target_is_directory=True)
        with self.assertRaisesRegex(ValueError, "symlink"):
            setup.prepare_root(link / "nested")
        downloads = self.root / "downloads"
        downloads.symlink_to(self.root, target_is_directory=True)
        with self.assertRaisesRegex(ValueError, "symlink"):
            setup.fetch_archive(self.root, setup.SOURCES["core"], True)

    def test_offline_missing_and_cache_verification(self):
        with patch.object(setup.urllib.request, "urlopen") as network:
            with self.assertRaisesRegex(ValueError, "offline cache miss"):
                setup.fetch_archive(self.root, setup.SOURCES["core"], True)
            network.assert_not_called()
        target = self.root / "downloads" / setup.SOURCES["core"]["archive"]
        target.write_bytes(b"bad cache")
        with self.assertRaisesRegex(ValueError, "SHA256"):
            setup.fetch_archive(self.root, setup.SOURCES["core"], False)
        target.unlink()
        target.symlink_to(self.root / "absent")
        with self.assertRaisesRegex(ValueError, "symlink"):
            setup.fetch_archive(self.root, setup.SOURCES["core"], True)

    def test_download_limit_and_hash_failure_publish_nothing(self):
        for data, limit in [(b"wrong hash", 1024), (b"too big", 1)]:
            response = io.BytesIO(data)
            response.geturl = lambda: "https://codeload.github.com/test"
            with patch.object(setup.urllib.request, "urlopen", return_value=response), \
                    patch.object(setup, "MAX_DOWNLOAD", limit):
                with self.assertRaises(ValueError):
                    setup.fetch_archive(self.root, setup.SOURCES["core"], False)
            self.assertEqual(list((self.root / "downloads").iterdir()), [])

    def test_download_deadline_interrupts_a_blocked_read(self):
        class SlowResponse(io.BytesIO):
            def geturl(self):
                return "https://codeload.github.com/authored-test"
            def read(self, size=-1):
                time.sleep(0.5)  # synthetic blocked read, no network
                return super().read(size)
        response = SlowResponse(b"authored")
        previous = setup.signal.getsignal(setup.signal.SIGALRM)
        with patch.object(setup.urllib.request, "urlopen", return_value=response), \
                patch.object(setup, "DOWNLOAD_TIMEOUT", 0.01):
            with self.assertRaisesRegex(TimeoutError, "wall-clock"):
                setup.fetch_archive(self.root, setup.SOURCES["core"], False)
        self.assertEqual(list((self.root / "downloads").iterdir()), [])
        self.assertEqual(setup.signal.getitimer(setup.signal.ITIMER_REAL)[0], 0)
        self.assertEqual(setup.signal.getsignal(setup.signal.SIGALRM), previous)

    def test_interrupted_build_kills_and_reaps_process_group(self):
        for index, error in enumerate([KeyboardInterrupt(), setup.subprocess.TimeoutExpired("test", 1)]):
            process = Mock(pid=12345)
            process.wait.side_effect = [error, 0]
            expected_error = KeyboardInterrupt if index == 0 else RuntimeError
            with patch.object(setup.subprocess, "Popen", return_value=process), \
                    patch.object(setup.os, "killpg") as kill:
                with self.assertRaises(expected_error):
                    setup.run(["authored-compiler"], self.root, self.root / f"build-{index}.log")
                kill.assert_called_once_with(12345, setup.signal.SIGKILL)
                self.assertEqual(process.wait.call_count, 2)

    def test_manifest_ownership_symlink_and_hashes(self):
        binary = self.root / "core.so"
        binary.write_bytes(b"synthetic ELF")
        artifact = setup.artifact(binary)
        self.assertEqual(artifact["sha256"], hashlib.sha256(binary.read_bytes()).hexdigest())
        path = self.root / "manifest.json"
        manifest = {"owner": setup.OWNER, "artifacts": {"core": artifact}}
        setup.write_manifest(path, manifest)
        setup.write_manifest(path, manifest)
        self.assertEqual(json.loads(path.read_text()), manifest)
        path.write_text('{"owner":"someone else"}')
        with self.assertRaisesRegex(ValueError, "non-tool"):
            setup.write_manifest(path, manifest)
        path.unlink()
        path.symlink_to(binary)
        with self.assertRaisesRegex(ValueError, "symlink"):
            setup.write_manifest(path, manifest)
        self.assertEqual(binary.read_bytes(), b"synthetic ELF")

    def test_mocked_build_manifest(self):
        sources = {}
        downloads = self.root / "downloads"
        downloads.mkdir()
        for key in setup.SOURCES:
            archive, source = self.archive(key=key)
            archive.rename(downloads / source["archive"])
            sources[key] = source

        def fake_run(command, cwd, log):
            if command[:2] == ["make", "all"]:
                name = ("libmupen64plus.so.2.0.0" if "core" in str(cwd)
                        else "mupen64plus-video-rice.so" if "video" in str(cwd)
                        else "mupen64plus-rsp-hle.so")
                (cwd / name).write_bytes(name.encode())
            elif command[0] == "cc":
                Path(command[-1]).write_bytes(b"input")

        # Add only the build directories expected by the mocked toolchain.
        original_extract = setup.extract_source

        def extract(*args):
            tree = original_extract(*args)
            (tree / "projects/unix").mkdir(parents=True)
            (tree / "data").mkdir()
            for name in ("mupen64plus.ini", "font.ttf", "RiceVideoLinux.ini"):
                (tree / "data" / name).write_bytes(b"authored test metadata")
            return tree

        with patch.object(setup, "SOURCES", sources), \
                patch.object(setup, "run", side_effect=fake_run) as run, \
                patch.object(setup, "extract_source", side_effect=extract), \
                patch.object(setup, "minizip_flags", return_value=[]), \
                patch.object(setup.subprocess, "check_output", return_value="test cc"):
            result = setup.setup(self.root, 2, True)
        manifest = json.loads(result.read_text())
        self.assertEqual(set(manifest["artifacts"]), {"core", "rsp", "input", "video"})
        for item in manifest["artifacts"].values():
            self.assertEqual(setup.sha256(Path(item["path"])), item["sha256"])
        self.assertEqual(run.call_count, 7)
        self.assertNotIn("install", str(manifest["commands"]))

    def test_minizip_does_not_disable_bundled_sources(self):
        with patch.object(setup.subprocess, "check_output", return_value="/usr/include\n"), \
                patch.object(Path, "is_file", return_value=True):
            self.assertEqual(setup.minizip_flags(), ["CPPFLAGS=-I/usr/include/minizip"])


if __name__ == "__main__":
    unittest.main()
