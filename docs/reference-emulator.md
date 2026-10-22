# Local reference emulator and capture

## Scope

The optional Linux research runner builds **Mupen64Plus 2.6.0** locally. It is
separate from the Rust reimplementation: no emulator, upstream source, ROM, save,
screenshot or captured state is bundled or committed. Only original setup/capture
tooling, authored inputs and synthetic tests belong in Git.

The user ROM is opened read-only, normalized in memory and gated on the canonical
US revision 0 SHA-256 before any revision-specific address is used. Nothing is
installed system-wide; no repository remote is added. Capture binds no cheat,
debugger-memory-write or ROM-patching API.

## Setup

Requires Linux, Python 3.11+, GNU make, GCC/G++, pkg-config and development headers
for SDL2, OpenGL, libpng and zlib. Minizip uses the host library when available or
the core's bundled fallback. The script does **not** install missing dependencies.
The validated host was little-endian x86-64 Linux with SDL's offscreen OpenGL
backend; other hosts/drivers are unverified. No visible window is needed.

```sh
# Explicit network acquisition from upstream, pinned by commit AND archive hash.
python3 scripts/reference_emulator/setup.py --jobs 4
# Rebuild only from already verified cached archives; no network access:
python3 scripts/reference_emulator/setup.py --offline --jobs 4
```

Outputs live below ignored `.cache/emulator/`. Each invocation extracts fresh
sources into a unique build directory, never trusts an edited/stale source tree,
and records source pins, compiler versions, flags and artifact/data SHA-256 values
in `.cache/emulator/manifest.json`. Existing captures keep their old artifacts.
Manifest replacement is restricted to tool-owned regular JSON; existing symlinks
are refused. Downloads have size and wall-clock limits; archive traversal, links,
devices and excessive expansion are rejected. Builds have process-group timeouts
and interruption cleanup. No `make install` is used.

Upstream pins (full archive SHA-256 values are in `setup.py`):

| Component | Repository | Commit |
| --- | --- | --- |
| Core | mupen64plus/mupen64plus-core | `b0d68c20f49b8f833afa21450e0e8874c87c13c4` |
| RSP-HLE | mupen64plus/mupen64plus-rsp-hle | `2798e65d6fc89d89aace0b0d779af6406809b940` |
| Rice graphics | mupen64plus/mupen64plus-video-rice | `fcf00779f08a9503ef30d26422f6b0350684820d` |

CPU settings: cached interpreter, debugger enabled, assembly/dynarec disabled,
accurate FPU, randomized PI/SI interrupt timing disabled. Rice uses offscreen
OpenGL; audio output is discarded, while RSP-HLE processes tasks. GCC 16 required
disabling Rice's speculative devirtualization to avoid unresolved unused vtables;
this graphics-only flag is recorded in the build manifest. The input bridge is
fresh C implementing the documented plugin ABI, not game code or Rust gameplay.
Upstream licenses remain with their separately downloaded sources.

## Reproduce the controlled capture

```sh
export SM64_ROM="/path/to/your/rom.z64"
python3 -m scripts.reference_emulator.capture \
  --out .cache/emulator/my-castle-a --profile us0-action \
  --schedule docs/examples/castle-walk-jump-inputs.json \
  --updates 2600 --max-vis 7000 --timeout 240 \
  --export-trace --trace-start 1900 --scenario castle-walk-jump-v1

# Repeat with a NEW --out, then compare:
cargo run -p sm64x -- trace compare \
  .cache/emulator/my-castle-a/trace.json \
  .cache/emulator/my-castle-b/trace.json --json
```

The input JSON is **authored controller input**, not captured game data. It boots
an empty save, navigates the menu/intro, then applies movement, release and jump
inputs in the castle grounds. Segments count controller polls, NOT game ticks;
A is game-visible `0x8000`, Start is `0x1000`, axes are raw signed bytes. Inputs
return to neutral after the last segment. Do not generalize this menu timing to
other cores, ROMs or saved games.

Every run requires a NEW ignored directory. A bounded child process owns isolated
HOME/XDG paths, config, shared-data copies and empty EEPROM/paks. Core dumps are
disabled; CPU/address-space and parent wall-clock limits are applied. Reports are
created exclusively, never overwritten. Artifact/data hashes are checked before
loading. This is containment for trusted local tooling, **not a security sandbox**
for arbitrary libraries. A failed/partial run is diagnostic output, not a valid
complete reference capture.

`probe.json` records counters, input transitions, paired snapshots and limitations.
`trace.json` is emitted only on explicit request with the documented action
profile and successful consistency checks. Generic `--entry`/`--state-address`
probes remain exploratory. `--screenshot` optionally requests an ignored diagnostic
PNG near the VI limit; pixels are not a simulation-state oracle.

## Boundary and field provenance

Documented metadata, not copied implementation bodies:

- STROOP [US symbol map](https://github.com/SM64-TAS-ABC/STROOP/blob/6771197ad0f50718d3d4a13387e01d1e6d9c0d1a/STROOP/Mappings/MappingUS.map).
- n64decomp/sm64 [declarations](https://github.com/n64decomp/sm64/blob/9921382a68bb0c865e5e45eb594d9c64db59b1af/src/game/mario.h)
  and [structure layout metadata](https://github.com/n64decomp/sm64/blob/9921382a68bb0c865e5e45eb594d9c64db59b1af/include/types.h).

US0 profile: entry `0x80254830` (`execute_mario_action`), state storage
`0x8033b170`, global state pointer variable `0x8032d93c`, timer `0x8032d5d4`.
**The entry argument is an Object pointer, not MarioState.** The profile uses
explicit state storage and checks the global pointer, rather than interpreting
`a0` as state. It pairs entry with the runtime caller return from register `ra`.
The observed return was `0x8029ca70`, inside the documented Mario behavior update.

MarioState offsets: action `0x0c`, yaw `0x2e`, position `0x3c`, velocity `0x48`,
forward velocity `0x54`, controller pointer `0x9c`. Controller raw signed sticks
are at `0x00/02`, held/pressed halfwords at `0x10/12`. Diagnostic action state/timer
and dialog fields stay in private probes, not silently added to Trace v1.
The movement diagnostics also use MarioState intended magnitude/yaw `0x20/24`,
wall/floor pointers `0x60/68`, ceiling/floor height `0x6c/70`, Controller processed
stick floats `0x04/08/0c`, and Surface type/vertices/normal `0x00/0x0a/0x1c`.
They corroborate the explicit flat-lane contract, not arbitrary collision behavior.
Additional conservative envelope checks read water level `0x76`, held/ridden object
pointers `0x7c/84`, squish timer `0xb4`, and quicksand depth `0xc0`. The
`movement_check` exporter validates the full capture chain and selected environment
without generating native states; see the movement reproduction commands.

Export checks the **entire** captured stream: paired calls, stable state/caller
addresses, unchanged timer/input poll during each call, consecutive global timer
and controller polls, exact observed-state continuity, game-visible input matching
the applied schedule, and pressed-button edges matching previous raw input.
It preserves the previous input when slicing. VI counters are diagnostic only.
No relationship between VIs, rendered images and game updates is assumed.

Runtime corroboration: the authored sequence produced neutral, movement, braking,
vertical jump/landing and terrain-height changes; raw sticks/buttons and pressed
edges agreed with injection. Two independent empty-save boots captured 2,600
paired updates each; their exported 700-update movement windows matched using the
Rust comparator's **exact f32 bits** policy. This establishes repeatability for
these observations on this build/host, not hardware equivalence or correctness of
a native Mario model. No captured states are fixtures.

REA's current Ghidra MIPS lane rejected the MIPS III analysis container; no static
native-code conclusion is claimed, and the ISA header was not misrepresented to
bypass that limitation. Address names/layouts above are documented metadata with
runtime consistency checks, not a decompiler-derived implementation.

## Tests and next boundary

```sh
# Standard-library tests: no download, ROM, emulator, window or GPU required.
python3 -m unittest discover -s scripts/reference_emulator/tests -v
```

Synthetic Python tests, Rust formatting/Clippy, normal Rust tests and optional
ignored ROM-dependent tests are run locally. Normal CI never acquires or runs
the emulator; test counts grow as the tooling evolves.

A [minimal native movement slice](movement-slice.md) now reproduces the flat
idle → straight acceleration → braking-to-stop window with exact observed bits.
Still absent: general native actions, original collision resolution, a full
initialization/save-state schema, verified original camera, BOB gameplay capture,
and comparisons against original hardware or independently configured emulators.
The static mathematical probes must not be substituted for the original solver.
