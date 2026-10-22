# Static old-Fast3D display lists

`sm64-gfx` is a fresh, safe Rust format decoder, not an RSP/RDP emulator or a
copy of game renderer code. It accepts resident `sm64_level::segments::SegmentMap`
data and returns flat triangle batches, decoded RGBA textures, and approximation
diagnostics. It does not read a ROM itself. Geometry-layout transforms and layer
selection belong to the caller; there are no native callbacks or gameplay.

## Format references / provenance

Format understanding was checked against these public references (cached locally
under ignored `.cache/references/`, not vendored):

- [Nintendo GBI format header, public sm64 repository](https://github.com/n64decomp/sm64/blob/9921382a68bb0c865e5e45eb594d9c64db59b1af/include/PR/gbi.h),
  specifically the **non-F3DEX / non-F3DEX2** macros and texture format constants.
- MIT [queueRAM/sm64tools f3d.c](https://github.com/queueRAM/sm64tools/blob/81de9e5a8f0fa96686a16441d5b9f25742f4d17d/f3d.c)
  for old command field interpretation.
- MIT [queueRAM/sm64tools n64graphics.c](https://github.com/queueRAM/sm64tools/blob/81de9e5a8f0fa96686a16441d5b9f25742f4d17d/n64graphics.c)
  for texture-channel format understanding.

No game/decompiler implementation, extracted asset, ROM bytes, or reference trace
is included in this crate or its authored synthetic fixtures. ROM-dependent tests
require the exact US revision 0 through `load_bob`'s hash gate, are ignored by
default, and skip with a message when `SM64_ROM` is absent.

## Commands and state

Commands are two big-endian words at eight-byte-aligned segmented addresses.
Supported operations:

| Opcode | Operation |
| --- | --- |
| `00` | SP no-op |
| `03` | MOVEMEM light slots `86..94` |
| `04` | VTX into a retained 16-entry cache |
| `06` | Display-list call or tail branch |
| `b6`, `b7` | Clear/set geometry flags |
| `b8` | End list / return |
| `b9`, `ba` | Low/high other-mode bitfield updates (high-mode rendering approximated) |
| `bb` | Texture enable, render tile, S/T scales |
| `bc` | MOVEWORD light count; fog parameters diagnosed and ignored |
| `be` | CULLDL visibility hint, diagnosed and ignored to retain static geometry |
| `bf` | TRI1 |
| `e6..e8` | Load/pipe/tile synchronization |
| `f2`, `f3`, `f5` | Tile rectangle, LOAD_BLOCK, tile descriptor |
| `f8` | Fog color, diagnosed and ignored |
| `fa`, `fb`, `fc`, `fd` | Primitive/environment colors, combiner, texture source image |

For VTX, `n = ((w0 >> 20) & 15) + 1`, `v0 = (w0 >> 16) & 15`;
the low word half must equal `16*n`. Each 16-byte vertex carries signed XYZ,
signed S/T, and either unsigned RGBA or signed normals plus unsigned alpha.
TRI1 indices are the low three bytes of `w1`, each divided by **10** (not 2).
Nonmultiples of 10 and unloaded references fail. These are not F3DEX2 encodings.

A nested call retains vertex, material, tile, light, and TMEM state, including
changes when it returns. Tail branches do not push a return address. Active-path
cycles fail; sequential calls to a shared sublist remain legal.

## Textures and UVs

SET_IMAGE only changes the source pointer. LOAD_BLOCK copies its source bytes
into a bounded logical TMEM store at the load tile's TMEM offset. Subsequent
SET_IMAGE commands cannot retroactively change already loaded texels. Rendering
uses the selected render tile's format, line stride, rectangle, and loaded TMEM,
not simply the most recently specified image. Partial overlapping loads replace
only the affected bytes. Missing source bytes or unread TMEM fail explicitly.

Supported channels: RGBA16 (5/5/5/1), RGBA32, IA4 (3/1), IA8 (4/4), IA16
(8/8), I4, I8. Intensity expands to all RGBA channels. CI/TLUT, YUV and invalid
format/size combinations produce errors, not replacement textures. Actual tested
BOB roots use RGBA16 and IA16; CI was not needed.

Rectangle bounds are unsigned 10.2 fixed point, inclusive at the last texel.
Vertex S/T is signed 10.5, multiplied by unsigned 0.16 texture scale **when the
vertex is loaded**, preserving cached coordinates across scale changes. At draw
time tile shifts are applied, rectangle origin subtracted, and a half-texel
center added before normalizing by texture dimensions. This is a host-sampling
approximation, not exact RDP rasterization. Shift values 0..10 divide by powers
of two; 11..15 multiply by `2^(16-shift)`.

Wrap/clamp/mirror flags and zero-mask clamping become sampler modes. A repeating
mask period different from the decoded rectangle is explicitly unsupported.
Nonzero LOAD_BLOCK source origins, image/load size mismatches, mip levels, and
generated texture coordinates fail. Logical source bytes are not RDP-swizzled;
no bank-swizzle, TLUT, three-point filter, or general TMEM hardware emulation is
claimed.

## Rendering contract / approximations

- Positions remain in original game units. Normals are normalized when lighting
  is enabled, otherwise zero (RGBA vertices have no normal).
- Unlit RGBA vertex colors are retained. Lighting is baked at VTX load from
  normalized object-space directional lights plus ambient; missing inherited
  light state uses a diagnosed neutral approximation. There is no camera-space
  light transform or integer RSP quantization. Actor streams can supply a
  per-joint inverse-transpose normal transform for coherent approximate world-space
  directional shading; positions/normals returned to the caller stay local.
- `Batch::color_combiner` distinguishes a common decal RGB form from the
  default texture-times-shade approximation. Decal is recognized only when both
  encoded RGB cycles are `(TEXEL0 - SHADE) * TEXEL0_ALPHA + SHADE`; mixed/unknown
  muxes retain an explicit fallback diagnostic. The viewer uses an opaque
  shaded base plus an untinted alpha-blended texture overlay for this bounded
  expression, preserving cap logos/buttons/face decals. Host linear blending and
  filtering are still approximations. Transparent/alpha-tested decal layer
  combinations are explicitly unsupported rather than silently modulated. Other combiner RGB uses texture times baked shade. Selected primitive
  or environment alpha replaces shade alpha when selected, rather than being
  multiplied by a raw vertex-alpha byte. Arbitrary RGB mux expressions are **not**
  evaluated; every SET_COMBINE produces a deduplicated approximation diagnostic.
- Batches split on texture, back-face culling, and estimated alpha mode. Alpha
  mode uses low other-mode alpha comparison / force blending, vertex and selected
  color alpha, and decoded texture transparency. Caller geometry layers remain
  authoritative where present. Front-face culling is explicitly unsupported.
- Fog, RDP dithering, cycle, perspective and filtering details are diagnosed as
  ignored/approximated. Host materials should not light these baked colors again.
- Unknown opcodes, matrix commands, unsupported MOVEWORD/MOVEMEM state and
  invalid texture data fail with contextual addresses. No plausible fake assets
  or silent unknown geometry commands are emitted.

`DisplayListStream` preserves lights/materials/TMEM across independent actor roots,
with per-root output/texture indices and explicit failure poisoning. Joint roots
must reload vertices; cross-root vertex-cache reuse fails. The viewer respects
opaque geometry layers and converts encoded shade RGB to host linear vertex colors;
this is a presentation approximation, not a second generic light source.

## Budgets and tests

`decode_display_list_with_budget` additionally accepts a caller's remaining
scene-wide command allowance (zero fails before reading). The viewer and visual
CLI report share a 1,000,000-command aggregate budget, including geometry traversal.
Null display-list branch/call targets fail rather than masquerading as end markers.

Per decode: at most 200,000 commands, 64 pushed calls, 100,000 triangles,
1,048,576 pixels per texture, 4,096 texture cache entries, and 64 MiB decoded
texture cache bytes. TMEM is exactly 4,096 logical bytes. Segment resolution,
vertex reads and list advancement are checked without crossing segment bounds.
Output and visited-path storage are bounded by command/triangle budgets. Cache
keys include complete render-tile state and TMEM load generation; cache memory
is owned by the returned display list, never a global resident asset cache.

Authored tests cover nested calls/branches and state retention, vertex RGBA and
lighting, cached scale timing, TRI1 validation, channel expansion, dimensions,
UV origin/shift/center, wrap, malformed segment/source/TMEM offsets, unknown
opcodes, cycles and command/stack/triangle/cache budgets.

Local validation commands (no workspace formatting):

```sh
CARGO_TARGET_DIR=.cache/target-gfx cargo test -p sm64-gfx
CARGO_TARGET_DIR=.cache/target-gfx cargo clippy -p sm64-gfx --all-targets -- -D warnings
SM64_ROM=/path/to/user-supplied-rom CARGO_TARGET_DIR=.cache/target-gfx \
  cargo test -p sm64-gfx local_bob_roots -- --ignored --nocapture
```

On canonical US0, all six area roots decoded successfully:

| Segmented root | Layer | Commands | Triangles | Texture entries |
| --- | ---: | ---: | ---: | ---: |
| `07004390` | 1 | 250 | 184 | 3 |
| `07009d80` | 1 | 629 | 473 | 10 |
| `0700a470` | 6 | 57 | 34 | 1 |
| `0700a920` | 4 | 60 | 24 | 2 |
| `0700dd18` | 1 | 462 | 354 | 6 |
| `0700e338` | 1 | 63 | 32 | 1 |

Total: 1,521 inspected commands, 1,101 triangles, 23 per-decode texture entries.
These are observations, not byte fixtures or a claim of pixel-exact rendering.
The ignored test also follows statically reachable resident model layouts:
21 unique roots decoded with no unsupported opcodes, including `06002410`,
`060212e8`, `0700e458`, `0700e768`, `0700e8a0`, `080048e0`, `080057f8`,
`08006660`, `0800c718`, `0800d680`, `0800dfe0`, `08022d08`, `08024bb8`,
`08025f08`, and `08028b78` in addition to the six above. Dynamic callback-created
geometry and animated runtime model selection are outside this test's scope.
