#!/usr/bin/env python3
"""Compare every US0 MIO0 candidate with queueRAM/sm64tools' standalone mio0.

No downloads or builds. All generated files stay in ignored .cache/reference-mio0.
See docs/research/mio0-verification.md for the pinned reference and limitations.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
CACHE = ROOT / ".cache/reference-mio0"
US0_SHA256 = "17ce077343c6133f8c9f2d6d6d9a4ab62c8cd2aa57c40aea1f490b4c8bb21d91"


def digest(data):
    return hashlib.sha256(data).hexdigest()


def run(argv):
    return subprocess.run(
        [str(arg) for arg in argv], check=True, capture_output=True,
        text=True, timeout=60,
    ).stdout


def verify(args, work):
    rom = Path(args.rom).resolve(strict=True)
    inspector = args.inspector.resolve(strict=True)
    reference = args.reference_decoder.resolve(strict=True)
    expected_size = 8 * 1024 * 1024
    if rom.stat().st_size != expected_size:
        raise ValueError("expected an 8 MiB US0 ROM")
    # Bound the read even if the file changes after stat; never hand the original
    # file to the C tool. The exact-image digest gate follows before execution.
    with rom.open("rb") as source:
        original = source.read(expected_size + 1)
    if len(original) != expected_size:
        raise ValueError("ROM size changed while reading")
    source_hash = digest(original)
    marker = original[:4]
    canonical = bytearray(original)
    if marker == bytes.fromhex("37804012"):
        canonical[0::2], canonical[1::2] = original[1::2], original[0::2]
    elif marker == bytes.fromhex("40123780"):
        for i in range(0, len(original), 4):
            canonical[i:i + 4] = original[i:i + 4][::-1]
    elif marker != bytes.fromhex("80371240"):
        raise ValueError("unrecognized N64 byte order")
    if digest(canonical) != US0_SHA256:
        raise ValueError("canonical ROM is not the pinned US0 image; refusing C decoder")
    normalized = work / "canonical.z64"
    normalized.write_bytes(canonical)
    normalized.chmod(0o400)
    base = [inspector, "--rom", normalized, "--json"]
    inventory_text = run(base + ["mio0", "list", "--validate"])
    (work / "inventory.json").write_text(inventory_text)
    inventory = json.loads(inventory_text)
    # Independently enumerate signatures, not compression semantics. Do not let a
    # truncated/filtered Rust inventory silently turn into a successful subset.
    offsets = [i for i in range(len(canonical) - 3) if canonical[i:i + 4] == b"MIO0"]
    entries = inventory["entries"]
    if (len(offsets) != 79 or inventory["candidate_count"] != 79
            or inventory["reported_count"] != 79 or inventory["valid_count"] != 79
            or inventory["scan_truncated"] or not inventory["validation_requested"]
            or any(inventory[k] for k in (
                "invalid_count", "skipped_count", "not_checked_count"))
            or [entry["offset"] for entry in entries] != offsets
            or any(entry["status"] != "valid" for entry in entries)):
        raise ValueError("expected a complete validated inventory of all 79 US0 signatures")
    results = []
    for entry in entries:
        offset = entry["offset"]
        rust_out = work / f"{offset:08x}.rust.bin"
        ref_out = work / f"{offset:08x}.reference.bin"
        run(base + ["mio0", "decode", hex(offset), "--out", rust_out])
        # Give C the entire ROM tail, not a Rust-selected encoded length.
        run([reference, "-d", "-o", hex(offset), normalized, ref_out])
        rust_bytes, ref_bytes = rust_out.read_bytes(), ref_out.read_bytes()
        matches = rust_bytes == ref_bytes and len(rust_bytes) == entry["decoded_size"]
        results.append({
            "offset": offset, "rust_size": len(rust_bytes),
            "reference_size": len(ref_bytes), "match": matches,
            "rust_sha256": digest(rust_bytes), "reference_sha256": digest(ref_bytes),
        })
    unchanged = digest(rom.read_bytes()) == source_hash
    normalized_unchanged = digest(normalized.read_bytes()) == US0_SHA256
    matched = sum(item["match"] for item in results)
    report = {
        "source_sha256": source_hash, "canonical_sha256": US0_SHA256,
        "source_unchanged": unchanged, "canonical_unchanged": normalized_unchanged,
        "inspector": str(inspector), "inspector_sha256": digest(inspector.read_bytes()),
        "reference_decoder": str(reference),
        "reference_decoder_sha256": digest(reference.read_bytes()),
        "candidate_count": len(entries), "matched_count": matched,
        "mismatched_count": len(entries) - matched,
        "total_decoded_bytes": sum(item["rust_size"] for item in results),
        "blocks": results,
    }
    (work / "comparison.json").write_text(json.dumps(report, indent=2) + "\n")
    print(f"Candidates: {len(entries)}; compared: {len(results)}; "
          f"matched: {matched}; mismatched: {len(entries) - matched}; "
          f"decoded bytes: {report['total_decoded_bytes']}")
    if not unchanged or not normalized_unchanged:
        raise ValueError("ROM hash changed during verification")
    return 0 if matched == 79 else 1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--rom", default=os.environ.get("SM64_ROM"))
    parser.add_argument("--reference-decoder", type=Path, required=True,
                        help="explicit path to sm64tools standalone mio0 (trusted executable)")
    parser.add_argument("--inspector", type=Path, default=ROOT / "target/debug/sm64x")
    parser.add_argument("--clean", action="store_true",
                        help="remove this run's files even on failure (default: retain)")
    args = parser.parse_args()
    if not args.rom:
        parser.error("pass --rom or set SM64_ROM; no ROM is downloaded")
    # Ensure private output remains ignored even if repository ignore rules change.
    try:
        run(["git", "-C", ROOT, "check-ignore", "--", CACHE / "probe"])
        CACHE.mkdir(parents=True, exist_ok=True)
        work = Path(tempfile.mkdtemp(prefix="run-", dir=CACHE))
    except (OSError, subprocess.SubprocessError) as error:
        print(f"Cannot create ignored workspace: {error}", file=sys.stderr)
        return 1
    print(f"Workspace: {work}", flush=True)
    try:
        return verify(args, work)
    except (OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
        print(f"Verification failed: {error}", file=sys.stderr)
        if isinstance(error, subprocess.CalledProcessError):
            print(error.stderr, file=sys.stderr)
        return 1
    finally:
        if args.clean:
            shutil.rmtree(work)
            print("Removed run workspace")


if __name__ == "__main__":
    sys.exit(main())
