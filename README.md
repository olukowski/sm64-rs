# sm64-rs

A behavior-faithful native reimplementation of **Super Mario 64** in Rust,
using a user-supplied ROM for content. No ROMs, Nintendo code, or game assets
are included. This is not an emulator.

**Status:** the Bevy viewer renders Bob-omb Battlefield's **textured static terrain**
from its geometry layouts, Fast3D display lists and ROM textures. Collision remains
available as a diagnostic view/overlay. Materials and lighting are approximate;
this is not a full N64 renderer or playable game. All format crates are engine-free.

## Try the ROM inspector

Requires a recent stable Rust toolchain and a ROM you are legally entitled to use.
No ROM is bundled, searched for online, or downloaded.

```sh
# Point at your own legally supplied ROM.
export SM64_ROM="/path/to/your/rom.z64"

cargo run -p sm64x -- info
cargo run -p sm64x -- mio0 list --validate

# Decode the first MIO0 block in the recognized US revision 0 ROM.
# Creates .cache/mio0/00108a40.bin; refuses to overwrite existing files.
cargo run -p sm64x -- mio0 decode 0x108a40
```

Alternatively, pass `--rom "/path/to/your/rom"` before or after the command.
An explicit path overrides `SM64_ROM`. ZIP/7z containers are not supported;
select an already unpacked ROM.

## Static terrain viewer

```sh
# Uses SM64_ROM from above; exact US revision 0 is required.
cargo run -p sm64-viewer

# Capture a NEW PNG and exit after it has been saved.
mkdir -p .cache
cargo run -p sm64-viewer -- --shot .cache/bob.png
```

Controls: **RMB** look · **WASD** fly · **Q/E** down/up · **Shift** faster ·
**R** reset · **C** toggle collision edges · **V** toggle textured/collision-only
view · **Esc** exit. The initial overview fits the render geometry's bounds.

```sh
cargo run -p sm64-viewer -- --collision-overlay
cargo run -p sm64-viewer -- --mode collision

# Fixed camera in original SM64 units, not the viewer's 1:100 scale.
cargo run -p sm64-viewer -- \
  --camera=-8500,6500,11000 --look-at=0,1000,0 --shot .cache/bob-fixed.png
```

`--area 1`, `--width` and `--height` are available. Screenshot parents must exist;
existing files and symlinks are refused. Fixed camera position/target must be
provided together. Screenshots capture whichever mode/overlay the CLI selects. The window compositor
may override requested dimensions; match captured dimensions when comparing fixed
camera images. Fixed-resolution offscreen captures are not implemented.

This is static course geometry, not spawned objects: no Mario, enemies, animated
platforms, macro/special objects, audio, original camera or gameplay. Native geometry
callbacks, generated environment effects and skybox are omitted with diagnostics.
Lighting, combiner effects, bilinear host filtering (without mipmaps) and render
layers are approximations, not
pixel-identical N64 output. The collision-only view uses invented diagnostic colors.

Inspect the underlying course without opening a window:

```sh
cargo run -p sm64x -- level bob
cargo run -p sm64x -- level bob --visuals --json
```

### Inspector commands

| Command | Purpose |
| --- | --- |
| `info` | Size, source byte order, canonical SHA-1/SHA-256, exact revision recognition, N64 header |
| `level bob [--visuals]` | Segment loads, static declarations and collision; optionally decode geometry/display lists/textures and report metadata |
| `collision bob --at X,Y,Z` | Diagnostic static floor/ceiling/wall probes; not original gameplay collision |
| `trace validate PATH` | Validate a versioned observation trace; no ROM needed |
| `trace compare EXPECTED ACTUAL` | First input/state divergence; exact float bits by default; no ROM needed |
| `mio0 list` | Find signature candidates and validate their headers |
| `mio0 list --validate` | Additionally decode candidates to check their stream structure |
| `mio0 decode OFFSET [--out PATH]` | Decode one block into a **new** output file |

Use `--json` with any command for machine-readable output. Offsets accept decimal
or `0x`-prefixed hexadecimal. CLI errors go to stderr with a nonzero exit status.
A scan reports invalid/skipped candidates in its results; their presence is not
itself a command failure.

```sh
mkdir -p .cache/reports
cargo run -p sm64x -- info --json > .cache/reports/info.json
cargo run -p sm64x -- mio0 list --validate --json > .cache/reports/mio0.json
cargo run -p sm64x -- mio0 decode --help
```

Keep extracted content and reports in ignored `.cache/` or `extracted/`.
Decoding never overwrites an existing file, including symlinks, and never writes
to the source ROM.

### Identity and limitations

- Detects big-endian `.z64`, pair-swapped `.v64`, and word-reversed `.n64` from
  their magic bytes, **not their filename extensions**. Normalizes only in memory.
- All reported offsets and identity hashes refer to canonical big-endian bytes.
- Exact recognition currently covers **US revision 0**. Other N64 ROMs can be
  inspected generically; a title or region byte does not establish authenticity.
- CRC1/CRC2 are **stored header values, not recomputed/verified checksums**.
- MIO0 signatures are candidates, not a complete asset directory. Successful
  decoding does not identify a block's contents.
- ROM input is capped at 64 MiB. MIO0 output defaults to 16 MiB per block;
  `--max-output-mib 1..64` adjusts it. Scans additionally cap attempted decoded
  output at 256 MiB and report at most 4,096 candidates. All candidates are
  counted; truncation and skipped validation are explicit.
- JSON `encoded_size` is the extent actually read from a block, including gaps,
  not an aligned storage size or proof of the next block's location.
- Course inspection supports a bounded static subset of level scripts, selects
  BOB in the root dispatch and stops before native callbacks. It does not emulate
  boot/menu behavior or a gameplay VM. Unsupported control flow fails explicitly.
- Collision parsing stops at the triangle-stop marker; special objects and
  environment-box tails are not decoded.
- Static geometry-layout/Fast3D support targets BOB. Unsupported dynamic subtrees,
  display-list opcodes and texture state fail explicitly rather than inventing data.
- No general asset type identification, native boot checksum verification,
  complete RSP/RDP emulation, skybox or gameplay implementation yet.

All 79 US MIO0 outputs match an independently built established decoder
byte-for-byte ([verification](docs/research/mio0-verification.md)). The course
parser resolves 570 collision vertices and 1,060 collision triangles for BOB area 1.
The textured viewer renders 1,101 visual triangles and 18 uploaded textures, with
local GPU checks for textured/overlay/collision modes. See
[textured milestone notes](docs/research/textured-bob.md).

## Headless foundation

`sm64-sim` now provides engine-free explicit ticks, raw input/button edges and
state validation with a caller-supplied transition model. It does **not** implement
Mario movement yet. Static geometric probes have authored slope/ledge/corner tests;
these are not the original game's collision solver.

```sh
cargo run -p sm64x -- collision bob --at 0,1000,0 --json
# Highest floor / lowest ceiling within an explicit inclusive height interval:
cargo run -p sm64x -- collision bob --at 0,1000,0 --min-y=-1000 --max-y=1000
```

Y in `--at` selects the wall probe slice. The floor and ceiling are selected across
the height interval, not necessarily below/above that Y. Original coordinate
quantization, surface exclusions and wall pushes are not implemented. See
[simulation boundaries and reference-validation gate](docs/simulation.md).

Trace tools work without a ROM. This checked-in example is **authored synthetic
data**, not an original-game capture:

```sh
cargo run -p sm64x -- trace validate docs/examples/authored-trace.json
cargo run -p sm64x -- trace compare \
  docs/examples/authored-trace.json docs/examples/authored-trace.json --json

# For future private captures; exits nonzero on divergence or invalid input.
# cargo run -p sm64x -- trace compare traces/reference.json traces/native.json
# Add --absolute-tolerance 0.001 only when intentionally relaxing float equality.
```

The library can replay inputs against a caller-supplied model; no gameplay replay
CLI is offered until a real model exists. A trace is an observation stream, not a
save-state or proof of capture authenticity. An original-game capture adapter and
verified runtime field map are still needed before validating movement.

## Development

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked

# Optional; selects the US reference ROM via SM64_ROM. Never runs in CI.
cargo test --workspace --locked -- --ignored --nocapture
```

Normal tests use authored synthetic data and require no ROM, window or GPU at
runtime. Ignored ROM-dependent tests skip with a message when `SM64_ROM` is unset. Building the viewer/tests still requires Bevy's platform
build dependencies.

### Linux build requirements

The viewer needs a working graphics driver and graphical session. The local
smoke test used Linux/Wayland with an NVIDIA Vulkan driver; other platforms have
not been validated.

For Debian/Ubuntu, the CI build dependencies are:

```sh
sudo apt-get install build-essential pkg-config libx11-dev libxkbcommon-dev libwayland-dev
```

On Arch Linux, the corresponding packages are `base-devel`, `pkgconf`, `libx11`,
`libxkbcommon` and `wayland`. Format-only commands can be built separately with
`cargo build -p sm64x` without compiling the Bevy viewer.

- [Architecture](docs/architecture.md)
- [Roadmap](docs/roadmap.md)
- [Simulation foundation and evidence boundaries](docs/simulation.md)
- [N64 ROM format notes](docs/formats/n64-rom.md)
- [MIO0 format notes](docs/formats/mio0.md)
- [Segments, scripts and collision](docs/formats/level-scripts.md)
- [Static geometry layouts](docs/formats/geometry-layouts.md)
- [Fast3D display lists and textures](docs/formats/fast3d.md)
- [Provenance policy](docs/adr/0001-provenance.md)
- [Contributor/agent rules](AGENTS.md)

## License

Original project code is licensed under MIT OR Apache-2.0.
Super Mario 64 and its content belong to Nintendo. This is an independent,
unofficial project, not affiliated with or endorsed by Nintendo.
See [LEGAL.md](LEGAL.md).
