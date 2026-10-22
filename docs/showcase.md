# Static BOB showcase

A reproducible presentation scene: ROM-backed Mario in a sampled center-idle pose,
textured Bob-omb Battlefield, ocean sky, approximate shading/fog and an authored
soft contact shadow. **It is not playable.** There are no enemies, gameplay
objects, animation playback, original camera or general Mario collision rules.
The validated headless [movement experiment](movement-slice.md) is separate.

## Launch

After installing the [build dependencies](../README.md#quick-start):

```sh
cargo run -p sm64-viewer -- --rom /path/to/your/rom.z64 --showcase
```

Or set `SM64_ROM` and omit `--rom`. The exact canonical US revision 0 is required;
byte order is detected from content. No ROM or asset is downloaded, and no emulator is needed; all game content
comes from your own legally supplied image. The ROM is opened read-only.

`--showcase` enables Mario, sky/fog, a composed home camera and hidden diagnostic
labels. **RMB** looks, **WASD/QE** flies, **Shift** speeds up, **R** restores the
home pose, **H** toggles labels, **V** switches diagnostic collision view,
**C** toggles collision edges, and **Esc** exits. The camera is an inspection tool,
not Lakitu's behavior; Mario is deliberately static.

The preset cannot initially combine with `--mode collision` or
`--collision-overlay`. Use those options without `--showcase` for diagnostics.
`--mario` and `--skybox` are independently selectable outside the preset.

## Fixed-resolution screenshots

```sh
mkdir -p .cache
cargo run -p sm64-viewer -- --rom /path/to/your/rom.z64 --showcase \
  --shot .cache/showcase.png
```

The PNG defaults to **1920 × 1080**. `--shot` always renders to an image target
**without creating a window**, rather than capturing a resized desktop window.
It does not require DISPLAY/Wayland, but still needs a working graphics adapter
and driver. A graphical session is needed for the interactive viewer only.

```sh
# Explicit alternate dimensions; non-aligned row widths are supported.
cargo run -p sm64-viewer -- --showcase --width 1001 --height 777 \
  --shot .cache/showcase-alternate.png
```

Use a new PNG path each time; parents must exist. Existing files/symlinks and ROM
aliases are refused. Readback dimensions are checked before encoding; the process
exits only after PNG write/flush/sync and atomic no-clobber publication, or with
an error. Encoding/I/O failures do not create an incomplete destination: a sibling
exclusive temporary file is used, then hard-linked to the requested new path.
The destination filesystem must support hard links. Normal error paths clean up
the temporary; abrupt process termination may leave a sibling `.sm64-shot-*.tmp`.
No ordinary rename or overwrite fallback is used. A 60-second
capture timeout bounds failure. A graphics backend panic/device failure is still
a program failure, not a successful image. Maximum target area is 16 megapixels.

The static pose, camera, MSAA sample count and disabled temporal dither avoid
frame-dependent presentation. Two independent local headless captures at
1920 × 1080 were byte-identical; an additional 1001 × 777 capture verified padded
GPU-readback rows. This is one Linux/Vulkan backend check, **not** cross-platform
pixel equivalence or comparison with original N64 pixels. Normal CI uses authored
ROM-free fixtures and does not run a GPU/emulator.

## Preset and presentation boundaries

The showcase camera is an authored composition in original game units:

- Eye `(-4800, 1224, 500)`, target `(0, 1500, -3000)`, vertical FOV 60 degrees.
- Mario is placed at X/Z `(-4500, 0)` on a checked horizontal plateau. The source
  terrain gives Y 1024; center and footprint-corner probes must agree.
- Presentation yaw is zero degrees. `--mario-yaw DEGREES` changes it explicitly.
- `--camera=X,Y,Z --look-at=X,Y,Z` overrides the home view as a pair. Width/height
  set capture dimensions, not original-game viewport or physics parameters.

The original source geometry is retained; no collision mesh is substituted for
visible terrain. Mario's lowest sampled vertex is grounded on the plateau; this
is visual placement, not a native spawn, floor search or movement transition.
Materials/lighting, two-pass decal RGB, filtering, contact shadow, fog and sky
projection remain bounded host approximations. Source triangle counts do not
include the extra host decal-overlay/contact-shadow passes.

Details and evidence: [Mario/animation format](formats/mario.md),
[Fast3D materials](formats/fast3d.md), [skybox tiles](formats/skyboxes.md).

## Sharing and publication

The [README showcase screenshot](images/showcase.png) is the sole bundled image:
a 1920 × 1080 Linux/Vulkan capture of this preset, clearly labelled as derived
work containing Nintendo-owned imagery. It is not an authored test fixture and
is not covered by the project's code licenses. No extracted assets are bundled.
Other captures belong in ignored `.cache/`, `extracted/` or `traces/` directories.
Hosting imagery publicly is a separate publication/rights decision; the capture
command does not upload, publish, add a remote or create a release.

Before sharing source, run the [publication checks](../CONTRIBUTING.md), including
reachable-history scanning. Logs can contain local paths and adapter information;
keep them private and review anything you intend to share.
