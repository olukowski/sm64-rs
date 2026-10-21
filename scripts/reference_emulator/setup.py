#!/usr/bin/env python3
"""Build pinned research-only Mupen64Plus libraries in the ignored project cache.

No package installation, ROM execution, or system installation is performed.
Every invocation builds fresh sources from hash-checked archives; existing manual
source/build trees are deliberately not trusted. Only downloads are reused.
"""
from __future__ import annotations

import argparse
from contextlib import contextmanager
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import platform
import shutil
import signal
import subprocess
import tarfile
import tempfile
import urllib.request

PROJECT = Path(__file__).resolve().parents[2]
CACHE = PROJECT / ".cache"
OWNER = "sm64-rs/reference-emulator/setup-v1"
MAX_DOWNLOAD = 32 * 1024 * 1024
MAX_EXPANDED = 256 * 1024 * 1024
BUILD_TIMEOUT = 300
DOWNLOAD_TIMEOUT = 90
SOURCES = {
    "video": {
        "repo": "mupen64plus-video-rice",
        "commit": "fcf00779f08a9503ef30d26422f6b0350684820d",
        "sha256": "0279be2fdc2f2c2f037b12ee88107b8fe1130168a4df288629ba413b6eaf3933",
        "archive": "video-rice-fcf0077.tar.gz",
    },
    "core": {
        "repo": "mupen64plus-core",
        "commit": "b0d68c20f49b8f833afa21450e0e8874c87c13c4",
        "sha256": "4173762419a3136f0ff3d3dee01e0ee242fa1c3b4cc1c0f7d25a6e2b1e46f1ba",
        "archive": "core-b0d68c2.tar.gz",
    },
    "rsp": {
        "repo": "mupen64plus-rsp-hle",
        "commit": "2798e65d6fc89d89aace0b0d779af6406809b940",
        "sha256": "90b12c946008dbc3807d66b196c37a17f1c106731a35d27cd3f3ecb02a7c9404",
        "archive": "rsp-hle-2798e65.tar.gz",
    },
}
CORE_FLAGS = [
    "DEBUGGER=1", "DEBUG=1", "NO_ASM=1", "OSD=0", "VULKAN=0",
    "KEYBINDINGS=0", "ACCURATE_FPU=1", "OPTFLAGS=-O2",
]


def checked_path(path: Path, *, cache: Path = CACHE) -> Path:
    """Require a lexical child of .cache, with no existing symlink component."""
    path = Path(os.path.abspath(path))
    cache = Path(os.path.abspath(cache))
    if path == cache or not path.is_relative_to(cache):
        raise ValueError(f"path must be below {cache}: {path}")
    for component in [path, *path.parents]:
        if component.is_symlink():
            raise ValueError(f"symlink refused: {component}")
    return path


def prepare_root(path: Path) -> Path:
    path = checked_path(path)
    path.mkdir(parents=True, exist_ok=True)
    return path


def sha256(path: Path) -> str:
    checked_path(path)
    if not path.is_file():
        raise ValueError(f"not a regular file: {path}")
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


@contextmanager
def download_deadline(seconds):
    """Linux main-thread wall deadline, including blocked HTTP reads."""
    if signal.getitimer(signal.ITIMER_REAL)[0] != 0:
        raise ValueError("refusing to replace an existing process alarm")
    previous = signal.getsignal(signal.SIGALRM)
    def expired(signum, frame):
        raise TimeoutError("download exceeded wall-clock deadline")
    signal.signal(signal.SIGALRM, expired)
    try:
        signal.setitimer(signal.ITIMER_REAL, seconds)
        yield
    finally:
        signal.setitimer(signal.ITIMER_REAL, 0)
        signal.signal(signal.SIGALRM, previous)


def fetch_archive(root: Path, source: dict, offline: bool) -> Path:
    downloads = checked_path(root / "downloads")
    downloads.mkdir(exist_ok=True)
    target = checked_path(downloads / source["archive"])
    if target.exists():
        if sha256(target) != source["sha256"]:
            raise ValueError(f"archive SHA256 mismatch: {target}")
        return target
    if offline:
        raise ValueError(f"offline cache miss: {target}")
    url = f"https://codeload.github.com/mupen64plus/{source['repo']}/tar.gz/{source['commit']}"
    fd, name = tempfile.mkstemp(prefix=".download-", dir=downloads)
    temporary = Path(name)
    try:
        total = 0
        with os.fdopen(fd, "wb") as output:
            with download_deadline(DOWNLOAD_TIMEOUT), urllib.request.urlopen(url, timeout=30) as response:
                if not response.geturl().startswith("https://"):
                    raise ValueError("non-HTTPS download refused")
                while True:
                    chunk = response.read(64 * 1024)
                    if not chunk:
                        break
                    total += len(chunk)
                    if total > MAX_DOWNLOAD:
                        raise ValueError("download exceeded size limit")
                    output.write(chunk)
        if sha256(temporary) != source["sha256"]:
            raise ValueError(f"archive SHA256 mismatch: {url}")
        checked_path(target)
        # Hard-link publication refuses any existing destination, including links.
        os.link(temporary, target)
        return target
    finally:
        temporary.unlink(missing_ok=True)


def extract_source(archive: Path, destination: Path, source: dict) -> Path:
    """Validate the complete archive before writing any member, never follow links."""
    checked_path(archive)
    checked_path(destination)
    top = f"{source['repo']}-{source['commit']}"
    with archive.open("rb") as stream:
        if hashlib.file_digest(stream, "sha256").hexdigest() != source["sha256"]:
            raise ValueError(f"archive SHA256 mismatch: {archive}")
        stream.seek(0)
        with tarfile.open(fileobj=stream, mode="r:gz") as tar:
            members = []
            seen = set()
            expanded = 0
            for member in tar:
                path = PurePosixPath(member.name)
                if (path.is_absolute() or ".." in path.parts or "\\" in member.name
                        or not path.parts or path.parts[0] != top
                        or not (member.isdir() or member.isfile())
                        or path in seen):
                    raise ValueError(f"unsafe archive member: {member.name}")
                seen.add(path)
                expanded += member.size
                if expanded > MAX_EXPANDED or len(seen) > 30000:
                    raise ValueError("archive expansion limit exceeded")
                members.append((member, path))
            destination.mkdir()  # fresh private workspace only
            for member, relative in members:
                target = checked_path(destination / str(relative))
                if member.isdir():
                    target.mkdir(parents=True, exist_ok=True)
                else:
                    target.parent.mkdir(parents=True, exist_ok=True)
                    with tar.extractfile(member) as src, target.open("xb") as dst:
                        shutil.copyfileobj(src, dst)
                    target.chmod(0o755 if member.mode & 0o111 else 0o644)
    return destination / top


def run(command: list[str], cwd: Path, log: Path) -> None:
    checked_path(cwd)
    checked_path(log)
    with log.open("xb") as output:
        output.write(("$ " + repr(command) + "\n").encode())
        output.flush()
        try:
            # A process group also bounds compiler children on a timeout.
            process = subprocess.Popen(command, cwd=cwd, stdout=output,
                                       stderr=subprocess.STDOUT, start_new_session=True)
            try:
                status = process.wait(timeout=BUILD_TIMEOUT)
            except BaseException as error:
                # A separate process group needs explicit cleanup on Ctrl-C as
                # well as timeout; otherwise compiler children keep running.
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                process.wait()
                if isinstance(error, subprocess.TimeoutExpired):
                    raise RuntimeError(f"build timed out; see {log}") from None
                raise
            if status:
                raise RuntimeError(f"command exited {status}; see {log}")
        except OSError as error:
            raise RuntimeError(f"could not run {command[0]}; see {log}: {error}") from error


def minizip_flags() -> list[str]:
    """Fix pkg-config installations that omit the minizip header subdirectory.

    Do NOT set MINIZIP_CFLAGS/LDLIBS: their mere definition disables the
    Makefile's bundled ioapi.c/zip.c/unzip.c fallback when pkg-config is absent.
    CPPFLAGS adds the missing include without changing that source selection.
    """
    candidates = []
    try:
        includedir = subprocess.check_output(
            ["pkg-config", "--variable=includedir", "minizip"],
            text=True, stderr=subprocess.DEVNULL, timeout=10).strip()
        if includedir:
            candidates.append(Path(includedir) / "minizip")
    except (OSError, subprocess.SubprocessError):
        pass
    candidates.append(Path("/usr/include/minizip"))
    for candidate in candidates:
        if (candidate / "unzip.h").is_file():
            return [f"CPPFLAGS=-I{candidate}"]
    return []


def artifact(path: Path) -> dict:
    path = checked_path(path)
    return {"path": str(path), "sha256": sha256(path)}


def write_manifest(path: Path, manifest: dict) -> None:
    path = checked_path(path)
    if path.exists():
        if not path.is_file():
            raise ValueError(f"manifest is not a regular file: {path}")
        try:
            old = json.loads(path.read_text())
        except (ValueError, UnicodeError) as error:
            raise ValueError(f"refusing non-tool manifest: {path}") from error
        if not isinstance(old, dict) or old.get("owner") != OWNER:
            raise ValueError(f"refusing non-tool manifest: {path}")
    fd, name = tempfile.mkstemp(prefix=".manifest-", dir=path.parent)
    temporary = Path(name)
    try:
        with os.fdopen(fd, "w") as output:
            json.dump(manifest, output, indent=2, sort_keys=True)
            output.write("\n")
        checked_path(path)
        os.replace(temporary, path)
    finally:
        temporary.unlink(missing_ok=True)


def setup(root: Path, jobs: int, offline: bool) -> Path:
    if not 1 <= jobs <= 16:
        raise ValueError("jobs must be between 1 and 16")
    root = prepare_root(root)
    archives = {key: fetch_archive(root, source, offline)
                for key, source in SOURCES.items()}
    # Unique workspace: never use an unverified manual tree or stale object flags.
    work = Path(tempfile.mkdtemp(prefix="build-", dir=root))
    trees = {key: extract_source(archives[key], work / key, source)
             for key, source in SOURCES.items()}
    core = trees["core"]
    api = core / "src/api"
    core_flags = CORE_FLAGS + minizip_flags()
    rsp_flags = ["DEBUG=1", "OPTFLAGS=-O2", f"APIDIR={api}"]
    commands = {}
    # GCC 16 speculative devirtualization otherwise references undefined, unused
    # Rice framebuffer subclass vtables. This changes graphics only, not CPU/FPU.
    video_flags = ["OPTFLAGS=-O2 -fno-devirtualize -fno-devirtualize-speculatively", f"APIDIR={api}"]
    for key, flags in [("core", core_flags), ("rsp", rsp_flags), ("video", video_flags)]:
        cwd = trees[key] / "projects/unix"
        run(["make", "clean", *flags], cwd, work / f"{key}-clean.log")
        commands[key] = ["make", "all", *flags, f"-j{jobs}"]
        run(commands[key], cwd, work / f"{key}-build.log")
    bridge = PROJECT / "scripts/reference_emulator/input_bridge.c"
    # Never truncate an existing output: even a regular file may alias user data.
    # The unique private workspace also keeps old manifests/artifacts usable.
    bridge_output = checked_path(work / "input_bridge.so")
    commands["input"] = ["cc", "-std=c11", "-shared", "-fPIC", "-Wall", "-Wextra",
                         "-Werror", f"-I{api}", str(bridge), "-o", str(bridge_output)]
    run(commands["input"], work, work / "input-build.log")
    compiler_versions = {
        name: subprocess.check_output([name, "--version"], text=True, timeout=10).strip()
        for name in ("cc", "gcc", "g++")
    }
    data = checked_path(work / "data")
    data.mkdir()
    for source in [core / "data/mupen64plus.ini", core / "data/font.ttf",
                   trees["video"] / "data/RiceVideoLinux.ini"]:
        shutil.copyfile(source, data / source.name)
    manifest = {
        "owner": OWNER, "schema_version": 1,
        "sources": {key: {**source, "archive_path": str(archives[key]),
                          "source_path": str(trees[key])} for key, source in SOURCES.items()},
        "commands": commands, "compiler_version": compiler_versions["cc"],
        "compiler_versions": compiler_versions,
        "host": {"platform": platform.platform(), "machine": platform.machine()},
        "core_data": str(data),
        "data_files": {file.name: artifact(file) for file in data.iterdir()},
        "artifacts": {
            "core": artifact(core / "projects/unix/libmupen64plus.so.2.0.0"),
            "rsp": artifact(trees["rsp"] / "projects/unix/mupen64plus-rsp-hle.so"),
            "video": artifact(trees["video"] / "projects/unix/mupen64plus-video-rice.so"),
            "input": artifact(bridge_output),
        },
    }
    manifest_path = root / "manifest.json"
    write_manifest(manifest_path, manifest)
    return manifest_path


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=CACHE / "emulator")
    parser.add_argument("--jobs", type=int, choices=range(1, 17), default=4)
    parser.add_argument("--offline", action="store_true", help="refuse download cache misses")
    args = parser.parse_args()
    try:
        print(setup(args.root, args.jobs, args.offline))
    except (OSError, ValueError, RuntimeError, tarfile.TarError, subprocess.SubprocessError) as error:
        parser.exit(1, f"setup: {error}\n")


if __name__ == "__main__":
    main()
