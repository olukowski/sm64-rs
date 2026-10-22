# Architecture

```text
sm64x                       CLI identity, extraction, course/visual/probe reports,
                            ROM-free trace validation/comparison and flat-walk execution
  ├── sm64-rom               canonical byte order, header and exact ROM identity
  ├── sm64-mio0              bounded MIO0 decoding
  ├── sm64-level             segments, level declarations, static collision,
  │                         model bindings, geometry hierarchy and bounded actor/animation sampling
  ├── sm64-gfx               old Fast3D state → triangle batches/material metadata,
  │                         RGBA textures and skybox tile panoramas
  └── sm64-sim               explicit-tick/input/state, diagnostic probes,
                            versioned traces, model replay and bounded flat walking

sm64-viewer (Bevy)           static Mario/BOB showcase, ROM sky, diagnostic collision,
                            inspection camera and fixed-resolution windowless PNG capture

sm64-sim                    engine-free explicit-tick/input/state foundation;
                            static geometric probes (not original collision rules),
                            explicit flat-lane movement experiment

future gameplay models      Mario actions, original collision rules/camera,
                            objects and progression beyond the first bounded slice
```

All format crates are engine-free. The viewer presents parsed content; it does not
own gameplay simulation or substitute generic physics for Mario. Its free camera
is a tool, not a recreation of Lakitu's camera.

## Identity and addresses

Generic inspection normalizes recognized N64 byte orders in memory to big-endian
(.z64). Hashes/ROM offsets use that order, not filename conventions.

Course inspection gates bootstrap offsets on the exact US revision 0 identity,
selects BOB from the root current-level dispatch, then loads segments from commands.
Low-24-bit offsets resolve inside those **decoded** segments. Resident segment bytes
are retained read-only for visual loading and explicitly excluded from level
serialization. CLI reports contain metadata, not binary/mesh/texture dumps.

This is static declaration traversal, not execution of native boot/menu behavior
or runtime callbacks. Unsupported control flow errors rather than guesses.

## Visual asset pipeline

Area geometry-layout pointers lead to hierarchical draw references, transforms
and render layers. Static parent transforms are preserved; native geometry/skybox
callbacks and general render-object traversal are diagnosed but not executed.
Strict terrain inspection rejects ambiguous dynamic subtrees. A separate, explicit
Mario profile supplies branch choices and a sampled center-idle pose from the
ROM's animation DMA table; it never executes native callbacks. Root/channel and
XYZ joint-matrix semantics were checked against read-only original execution.
See [Mario asset boundaries](formats/mario.md).

Fast3D decoding retains cache, light, tile, texture-memory and material state across
nested display lists. It returns flat original-unit triangles and decoded texture
images. Loaded texture bytes are tied to LOAD_BLOCK state, not merely SET_IMAGE.
Unknown commands and unsupported texture/state combinations fail explicitly.

Bevy applies the geometry matrices and then a shared 1:100 unit scale, preserving
X/Y/Z (Y up). It uploads vertex colors/UVs and deduplicates equal texture images and
sampler modes. Materials are unlit because lighting is already baked by the decoder;
posed joints supply a normal transform and encoded shade RGB is converted for host
linear materials. Recognized opaque decal RGB uses a shaded base plus an untinted
alpha-blended texture pass. Unsupported transparent/alpha-tested decal combinations
fail explicitly. Host filtering, unknown combiner fallback, blending and lighting
remain approximations, not an RSP/RDP renderer. No generated mipmaps yet.

The ocean-sky pointer grid is decoded into a panorama, then resampled to a host
cubemap. Its projection, distance fog and authored contact blob are presentation
policies, not the native skybox/shadow generators. The showcase camera and Mario
placement are authored and checked against static flat terrain. See [showcase
reproduction and limits](showcase.md).

The decoder enforces per-list budgets; viewer/report consumers also pass a shared
remaining allowance, capped at 1,000,000 commands including geometry traversal.
The viewer additionally caps ROM geometry at 1,000,000 source triangles,
512 distinct decoded textures and 128 MiB decoded RGBA data. Host decal/contact
passes are extra presentation geometry. Offscreen targets have a separate
16-megapixel area limit. These are tool limits, not original-engine behavior.

## Collision remains separate

The TERRAIN command names the collision triangle section, not visible render
geometry. Collision data is never used to invent textured terrain. A diagnostic
filled view and cyan line overlay remain available. Edges get a small normal offset
in presentation only; original collision vertices remain unchanged. Line meshes
must not use triangle-only depth bias.

Collision tails, macro/special objects, native behaviors, moving objects, original
camera/audio, general gameplay and native environment effects remain absent.
Static Mario and sky assets do not instantiate their original behaviors.

## Headless foundation

`sm64-sim` advances only when explicitly stepped with caller-supplied raw input
and a `TickModel`. `movement::FlatWalk` implements the first bounded, fixed-yaw
idle/walking/braking sequence in an explicitly asserted flat lane. Its static geometric
queries are diagnostic primitives, not the original floor/ceiling/wall solver.
Versioned traces compare exact float bits by default, validate protocol alignment,
and can replay raw inputs through a caller-initialized model. CLI trace commands
need no ROM; `movement run` generates independent native traces from an explicit
scenario rather than reading expected outputs. No general gameplay replay is offered.
State uses original units and stays independent of Bevy and viewer transforms.
See [simulation boundaries and evidence gate](simulation.md). Optional
`scripts/reference_emulator/` tooling acquires a separate pinned emulator and
captures private US0 action observations; it is not linked into the Rust runtime.
See [reference setup and evidence](reference-emulator.md).

## Verification

Authored parser/CLI/mesh tests need no ROM, window or GPU at runtime. Ignored tests
use `SM64_ROM` and skip if absent. Independent MIO0 verification uses an explicitly
selected established decoder; it never fetches or builds programs itself.

Local GPU checks cover terrain/collision modes and the static showcase.
`--shot` disables the window runner, renders to an exact-size image, and reads it
back through Bevy's screenshot API. A same-directory exclusive temporary file is
encoded/flushed/synced, then hard-linked atomically to the new destination without
clobbering an existing path. A GPU is required, not a display server. Independent
same-host captures matched byte-for-byte, including tests of non-aligned image row
widths. Screenshots are not original-game pixel comparisons or cross-platform proof.
All game-derived output stays ignored, and the source ROM is never modified.
