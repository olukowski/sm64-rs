# Independent MIO0 output verification

## Observed result

The local 8,388,608-byte pair-swapped US0 ROM was opened read-only, normalized
in ignored cache, and checked against the established **queueRAM/sm64tools**
standalone C `mio0` decoder. No reference decoder was newly implemented.

- Canonical SHA-256: `17ce077343c6133f8c9f2d6d6d9a4ab62c8cd2aa57c40aea1f490b4c8bb21d91`
- Source SHA-256: `f3b0fc9f565f995368b6fa5aefc8ec1e5194c488948b41f3534dece4abcec2b4`
- Candidates / valid / compared / byte-for-byte matches: **79 / 79 / 79 / 79**.
- Invalid / skipped / unchecked / mismatches: **0 / 0 / 0 / 0**.
- Total decoded output: **7,463,840 bytes** per decoder.
- All corresponding output lengths also equal the inventory's decoded sizes.
- Source and cached canonical ROM hashes were unchanged after comparison.

Each retained local run uses `.cache/reference-mio0/run-<unique-id>/`, containing
`inventory.json`, `comparison.json`, normalized input and both outputs for every
block. The comparison report includes per-block offsets, sizes and SHA-256s.
Equality is checked directly on bytes, not inferred from those hashes.
No game-derived files are tracked.

## Reference provenance and license

Source URL: <https://github.com/queueRAM/sm64tools>

Pinned commit: `81de9e5a8f0fa96686a16441d5b9f25742f4d17d`.

The repository's `LICENSE` was inspected **before compiling or executing**:
MIT, copyright (c) 2015 Q. It permits use, modification and redistribution
subject to retaining its notice. No upstream code is vendored or copied into
this project's source/documentation. Only the upstream source checkout was
acquired, inside ignored `.cache/reference-mio0`; no release executable, ROM or
game content was downloaded. The checkout's tracked source was not modified.

SHA-256 identifiers:

| Item | SHA-256 |
| --- | --- |
| `LICENSE` | `fcd24a0025e568e0b1b84379969b5c8b47c7d5dcf9c4df803c3f75dc4762fc4f` |
| `libmio0.c` | `40270b61b9921968a350c9cf025c641232c7f049a06a7587bc42947f6380e488` |
| `libmio0.h` | `bc93ed07c6a010aa87dce8109e72a79f886894acc78bf080d15ccf877da7e562` |
| `Makefile` | `44e6432665f6f04cc90186c6987b09b4933f9b48503d4d15f901751910b0c0f6` |
| Built `mio0` | `de96fc016901a3c0f2a24f0e02b3df3c36f0c101f1037264c41bb13f6eb45e53` |
| Observed `target/debug/sm64x` | `454143c2b0892d64522174b98412c00e87462e8a0236c1bc408449efe492cc97` |

Upstream `make mio0` compiled only `libmio0.c`, using GCC
`16.2.1 20260810`, with:

```text
gcc -Wall -Wextra -Wno-format-overflow -O2 -ffunction-sections -fdata-sections -I./ext -MMD -DMIO0_STANDALONE -s -Wl,--gc-sections -o mio0 libmio0.c
```

No system packages were installed. Binary hashes can vary with toolchain.

## Reproduce

Acquisition is an explicit manual step, **not performed by the verifier**:

```sh
mkdir -p .cache/reference-mio0
git clone https://github.com/queueRAM/sm64tools.git .cache/reference-mio0/sm64tools
git -C .cache/reference-mio0/sm64tools checkout --detach 81de9e5a8f0fa96686a16441d5b9f25742f4d17d
cat .cache/reference-mio0/sm64tools/LICENSE
# Review license before proceeding.
make -C .cache/reference-mio0/sm64tools mio0
cargo build -p sm64x --locked
python3 scripts/verify-mio0.py \
  --rom '/path/to/your/rom.z64' \
  --reference-decoder .cache/reference-mio0/sm64tools/mio0
```

Alternatively set `SM64_ROM`; `--rom` overrides it. `--inspector PATH` overrides
`target/debug/sm64x`. Reference decoder path is always required and must name a
trusted executable with the pinned sm64tools CLI:

```text
mio0 -d -o 0xOFFSET CANONICAL_ROM OUTPUT
```

The verifier accepts native big-endian, pair-swapped or word-reversed input,
but requires the exact canonical US0 hash before invoking C. It independently
counts signature offsets, requires the validated inventory to contain all 79
in order without truncation, and invokes Rust extraction plus external C
extraction at every offset. C receives the whole canonical ROM, not an encoded
slice bounded by Rust's consumed-length calculation. Each invocation has a
60-second timeout. Subprocess failures, incomplete inventory and mismatches
return nonzero. It never passes the supplied ROM to the C executable.

Every run uses a new private directory under ignored `.cache/reference-mio0`.
Default: retain evidence, including on failure. `--clean`: remove that run's
directory on exit, including failure, but leave the reference checkout/build.
The script neither fetches nor builds programs. It records executable hashes
but does not authenticate a user-selected decoder against the pinned build.

## Additional validation performed

- A second successful run using `SM64_ROM` and `--clean`: 79 matches; cleanup
  confirmed.
- Negative test: an ignored wrapper invoked the real decoder then flipped the
  first output byte. All 79 mismatches were reported; exit status 1 and cleanup
  confirmed. This wrapper is not an alternative decoder or reference evidence.
- Missing `--rom` / `SM64_ROM`: actionable argument error, exit status 2.
- At the MIO0-verification milestone, before the course/viewer additions,
  `cargo test -p sm64-mio0 -p sm64x --locked` had 35 passing tests and 1
  ROM-dependent test ignored; no failures. This is a historical subset count,
  not the current workspace total. `--help` works without a ROM.

## Limits

This establishes agreement of decoded bytes for all 79 MIO0 signatures in this
exact image, strengthening the earlier acceptance-only observation in
[reference-rom.md](reference-rom.md). It is not a complete asset map, proof of
correctness for arbitrary or malformed streams, consumed-length equivalence,
compression equivalence, or gameplay fidelity. The legacy C decoder is not a
hardened untrusted-input parser; the exact-image gate is intentional. Executing
an explicitly selected binary still requires trusting that binary. No game
code, decompilation or extracted assets are included here.
