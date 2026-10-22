# Skybox tile grids

`sm64_gfx::skybox::decode_panorama(&SegmentMap, table: u32) -> Result<Texture, Error>`
reconstructs a logical 256 × 256 RGBA8 panorama from already resident data.
It is pure Rust, independent of Bevy, and does not load ROM segments, execute
callbacks, or interpret a display list. `wrap_s` is `Repeat`; `wrap_t` is `Clamp`.

## Provenance and revision

Metadata references are pinned to n64decomp/sm64 revision
[`9921382a68bb0c865e5e45eb594d9c64db59b1af`](https://github.com/n64decomp/sm64/tree/9921382a68bb0c865e5e45eb594d9c64db59b1af):

- [`assets.json`](https://github.com/n64decomp/sm64/blob/9921382a68bb0c865e5e45eb594d9c64db59b1af/assets.json)
  declares `textures/skyboxes/water.png`: decoded size 131392 and US ROM
  offset 2803376 (`0x2ac6b0`).
- [`include/geo_commands.h`](https://github.com/n64decomp/sm64/blob/9921382a68bb0c865e5e45eb594d9c64db59b1af/include/geo_commands.h)
  declares `BACKGROUND_OCEAN_SKY = 0` and command `0x19`: signed 16-bit
  background ID at byte 2, callback pointer at byte 4.
- [`src/game/skybox.h`](https://github.com/n64decomp/sm64/blob/9921382a68bb0c865e5e45eb594d9c64db59b1af/src/game/skybox.h)
  declares the camera-facing skybox interface; it does not specify a cubemap.

No skybox implementation bodies were consulted or copied. The parser and
synthetic fixtures are independently authored. Local research used read-only
canonical US revision 0 bytes verified by
`scripts.reference_emulator.capture.rom_bytes`, the existing generic Rust MIO0
CLI, and temporary helpers/output only under ignored `.cache/`. No ROM bytes,
image assets, or extracted fixture files are distributed.

## Observed BOB layout

The level-script loader retains segment `0x0a`, compressed ROM range
`0x2ac6b0..0x2b8f10`, decoded size `0x20140`. Direct inspection of its actual
80-word table at segmented address `0x0a020000` confirmed eight rows of ten
big-endian pointers. For row `r = 0..7` and column `c = 0..9`, every observed
pointer is exactly:

```text
0x0a000000 + r * 0x4000 + (c % 8) * 0x800
```

Thus row 0 begins `0x0a000000, 0x0a000800`; row 7 begins
`0x0a01c000, 0x0a01c800`. The last distinct tile is `0x0a01f800`.
Columns 8 and 9 repeat columns 0 and 1 **in every row**, by pointer equality,
not merely pixel similarity. The 64 distinct 32 × 32 RGBA16 tiles occupy
`0x20000` bytes; the 320-byte pointer table follows immediately. This formula
reports measured metadata; the decoder reads and checks pointers rather than
assuming this storage order.

BOB's resident root geometry at `0x0e000488` contains the background command
at `0x0e0004a8`: opcode `0x19`, background ID **0**, non-null callback
`0x802763d4`. The pinned command declarations identify ID 0 as ocean sky.
The callback is not executed or inspected.

## Validation and output convention

The table must be four-byte aligned with all 320 bytes resident. Each of its
80 pointers must be eight-byte aligned (texture/DMA alignment) and resolve a
complete 2048-byte tile inside one loaded segment. Invalid slots, unloaded
segments, short tiles, truncated tables, and bad wrap references fail. The
wrap columns are bounds checked even though they are not copied. Table and
tile segments need not match; no fixed BOB storage addresses are assumed.
Interior tiles may repeat; only the two wrap-pointer equalities are required.

For each row, columns 0 through 7 are assembled left-to-right; row 0 is placed
at the top, with each tile's linear scanlines unchanged. No vertical flip or
projection is inferred. RGBA5551 is big-endian: five-bit RGB channels expand
with `(v << 3) | (v >> 2)`, and the alpha bit becomes 0 or 255.

This is a generic panorama reconstruction, **not** the original render
projection. Host panorama/cubemap mapping will approximate the original
screen-aligned scrolling background. Camera-angle mapping, field of view,
viewport cropping, filtering, seams, color processing, fog, and lighting are
host policies; no N64 hardware-fidelity claim is made.

## Viewer presentation policy

`sm64-viewer --skybox` validates BOB's ocean-sky declaration and converts the
panorama into a six-face host cubemap (+X, -X, +Y, -Y, +Z, -Z). A bilinear
CPU resample uses an explicitly equirectangular mapping; it is not the original
scrolling/projection. The cubemap is camera-translation independent. Brightness
compensates the fixed default camera exposure to preserve source image intensity.

Approximate distance fog starts at 8,000 original units and reaches its selected
pale-blue color at 20,000 units. It is host presentation depth cueing, not decoded
original fog behavior. Mario's posed normal transforms feed the same decoded
lights used for shading, and the viewer converts encoded shade RGB for linear
host materials. The authored soft contact blob under Mario is explicitly **not**
the game's shadow generator. No physical lighting, camera or collision behavior
is substituted for native gameplay.

## Tests

`cargo test -p sm64-gfx --test skybox` exercises authored shuffled tile storage,
per-pixel color/alpha/orientation, all 80 malformed references, truncation,
alignment, pointer-based wrap validation, and legal repeated interior tiles.
The ignored `local_bob_panorama` test loads the level from `SM64_ROM`, checks
its resident table through the same decoder, and asserts dimensions only:

```sh
SM64_ROM=/path/to/your/rom.z64 cargo test -p sm64-gfx --test skybox -- --ignored
```
