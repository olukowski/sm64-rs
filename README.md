# sm64-rs

An independent native Rust reimplementation of **Super Mario 64**, working toward
behavior fidelity. Content comes from a ROM you legally supply: no ROMs, Nintendo
code, extracted assets or captured game data are included. This is not an emulator.

**Current scope:** inspect the recognized US revision 0 ROM and view Bob-omb
Battlefield's textured **static terrain**, plus run a small reference-validated
**headless movement slice**. The viewer is not playable; materials and lighting
are approximate. Format and simulation crates are separate from Bevy presentation.

## Quick start

Install a recent stable [Rust toolchain](https://rustup.rs/). On Linux, building
the viewer/workspace requires a C toolchain and window-system development libraries:

```sh
# Debian/Ubuntu
sudo apt-get install build-essential pkg-config libx11-dev libxkbcommon-dev libwayland-dev
# Arch equivalents: base-devel pkgconf libx11 libxkbcommon wayland
```

The viewer also needs a graphical session and working graphics driver. Linux has
been smoke-tested; other platforms are not validated. The CLI alone can be built
with `cargo build -p sm64x` without Bevy's platform dependencies.

```sh
# Use your own unpacked ROM; no particular filename or extension is required.
export SM64_ROM="/path/to/your/rom.z64"
cargo run -p sm64x -- info
cargo run -p sm64x -- level bob
cargo run -p sm64-viewer
```

Or pass `--rom /path/to/your/rom.z64` to select a ROM explicitly. Big-endian,
pair-swapped and word-reversed images are recognized by bytes, not extensions.
Archives are not supported; nothing downloads, patches or writes to your ROM.
Course inspection/viewing requires the exact canonical **US revision 0** identity.

Viewer controls: **RMB** look · **WASD** fly · **Q/E** down/up · **Shift** faster ·
**R** reset · **C** collision edges · **V** textured/collision view · **Esc** exit.
This is static geometry: no Mario, enemies, moving objects, gameplay, audio or
original camera. Skybox and native geometry callbacks are omitted with diagnostics;
this is not pixel-exact N64 rendering.

```sh
# Optional collision view or a new private screenshot (never overwrites).
cargo run -p sm64-viewer -- --mode collision
mkdir -p .cache
cargo run -p sm64-viewer -- --shot .cache/bob.png
# Fixed pose in original SM64 units, not the viewer's 1:100 scale:
cargo run -p sm64-viewer -- \
  --camera=-8500,6500,11000 --look-at=0,1000,0 --shot .cache/bob-fixed.png
```

Screenshot parents must exist. The compositor may override dimensions; fixed
pose does not imply pixel-identical or fixed-resolution offscreen captures.

## Headless tools

| Command (`cargo run -p sm64x -- …`) | Purpose |
| --- | --- |
| `info` | Header, byte order, canonical hashes and exact revision recognition |
| `level bob [--visuals]` | Static declarations/collision; optional geometry and texture metadata |
| `collision bob --at X,Y,Z` | Diagnostic static floor/ceiling/wall probes, not original collision resolution |
| `mio0 list --validate` | Scan signature candidates and validate/decode streams |
| `mio0 decode 0x108a40` | Decode the first US0 block to a new ignored cache file |
| `movement run SCENARIO` | Run the bounded native flat-walk slice without a ROM |
| `trace validate PATH` | Validate an observation trace without a ROM |
| `trace compare EXPECTED ACTUAL` | Report first divergence; exact float bits by default, no ROM needed |

Use command `--help` for options and `--json` for machine-readable reports.
Keep all ROM-derived reports/output in ignored `.cache/`, `extracted/` or `traces/`.
MIO0 signatures are not a complete asset directory; header CRCs are stored values,
not verified checksums. Unsupported static format/control-flow features fail
explicitly; diagnostics distinguish omitted rendering features.

The narrow [movement slice](docs/movement-slice.md) is headless only:
idle → full-forward straight acceleration → release/braking to stop, inside an
explicit caller-supplied flat lane. It fixes yaw toward **-Z**, accepts only
neutral or **(0,80)** sticks with no buttons, and is **not** general collision
or playable viewer movement. It supports 10–30 forward ticks through the first
braking stop; low-speed release and stop animation are rejected. See the slice
documentation for exact evidence and
limits; an authored ROM-free example is:

```sh
cargo run -p sm64x -- movement run docs/examples/authored-movement.json
```

Trace examples are authored synthetic data, not original-game captures:

```sh
cargo run -p sm64x -- trace validate docs/examples/authored-trace.json
cargo run -p sm64x -- trace compare \
  docs/examples/authored-trace.json docs/examples/authored-trace.json --json
```

The optional [local reference emulator adapter](docs/reference-emulator.md) is
separate from normal builds/tests. Private observations are not public fixtures
or proof of general gameplay fidelity.

## Development and documentation

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
python3 scripts/check_publication.py
python3 -m unittest discover -s scripts/tests -v
python3 -m unittest discover -s scripts/reference_emulator/tests -v
```

Normal tests need no ROM, emulator, window or GPU at runtime. Ignored ROM tests
are opt-in (`cargo test --workspace --locked -- --ignored --nocapture`) and skip
when `SM64_ROM` is unset. See [CONTRIBUTING.md](CONTRIBUTING.md) before sharing data.

- [Architecture](docs/architecture.md) · [Roadmap](docs/roadmap.md)
- [Simulation/evidence boundaries](docs/simulation.md) · [Movement slice](docs/movement-slice.md)
- Formats: [ROM](docs/formats/n64-rom.md), [MIO0](docs/formats/mio0.md),
  [scripts/collision](docs/formats/level-scripts.md),
  [geometry](docs/formats/geometry-layouts.md), [Fast3D](docs/formats/fast3d.md)
- Evidence: [79 independent MIO0 matches](docs/research/mio0-verification.md),
  [BOB inspection](docs/research/bob-course.md), [textured terrain](docs/research/textured-bob.md),
  [headless foundation](docs/research/headless-foundation.md)
- [Provenance policy](docs/adr/0001-provenance.md) · [Contributor/agent rules](AGENTS.md)

## License

Original project code: **MIT OR Apache-2.0**. Super Mario 64 and its content belong
to Nintendo. This unofficial project is not affiliated with or endorsed by Nintendo.
See [LEGAL.md](LEGAL.md).
