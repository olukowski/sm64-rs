# Initial local reference check

## Observed

A user-selected 8,388,608-byte image was inspected read-only. Its filename ended
in `.n64`, but the byte-order marker identified pair-swapped (`.v64`) data.
After in-memory normalization:

- SHA-1: `9bef1128717f958171a4afac3ed78ee2bb4e86ce`
- SHA-256: `17ce077343c6133f8c9f2d6d6d9a4ab62c8cd2aa57c40aea1f490b4c8bb21d91`
- Exact identity: Super Mario 64, US revision 0.
- 79 occurrences of the MIO0 signature; all 79 decoded successfully within the
  default per-block limit and aggregate scan budget. None were skipped.

The first candidate is at canonical offset `0x108a40`, used by the README's
example command. Offsets do not describe the source file's byte-swapped ordering.

The source ROM, extracted output and machine-readable inventory remain local and
ignored. Hashes, counts, and compatibility identifiers are metadata, not fixtures.

## What this establishes

The normalization/identity pipeline recognizes this reference image, and our
decoder accepts all observed candidate streams. Synthetic tests independently
exercise literals, references, overlap, truncation and limits.

## What it does not establish

This is not a complete asset inventory, independent byte-for-byte comparison
against a second decoder, N64 boot/checksum verification, or gameplay fidelity.
No level scripts or native game functions have been interpreted yet.

## Reproduce locally

Set `SM64_ROM` to a user-supplied US revision 0 ROM, then run:

```sh
cargo run -p sm64x -- info
cargo run -p sm64x -- mio0 list --validate
cargo test --workspace -- --ignored --nocapture
```

If the environment variable is absent, the ignored reference test skips with a
message. If it selects a different revision or a corrupt file, that test fails.
