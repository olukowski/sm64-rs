# Architecture

```text
sm64x                       CLI identity, extraction, course/visual metadata reports
  ├── sm64-rom               canonical byte order, header and exact ROM identity
  ├── sm64-mio0              bounded MIO0 decoding
  ├── sm64-level             segments, level declarations, static collision,
  │                         model bindings and geometry-layout hierarchy
  └── sm64-gfx               old Fast3D state → triangle batches and RGBA textures

sm64-viewer (Bevy)           textured static BOB terrain, diagnostic collision
                            view/edges, free camera and PNG capture

sm64-sim                    engine-free explicit-tick/input/state foundation;
                            static geometric probes (not original collision rules)

future gameplay models      Mario actions, original collision rules/camera,
                            objects and progression; no movement model yet
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
and render layers. Static parent transforms are preserved; geometry callbacks,
skybox generation and render-object traversal are diagnosed but not executed.
Ambiguous dynamic subtrees fail explicitly.

Fast3D decoding retains cache, light, tile, texture-memory and material state across
nested display lists. It returns flat original-unit triangles and decoded texture
images. Loaded texture bytes are tied to LOAD_BLOCK state, not merely SET_IMAGE.
Unknown commands and unsupported texture/state combinations fail explicitly.

Bevy applies the geometry matrices and then a shared 1:100 unit scale, preserving
X/Y/Z (Y up). It uploads vertex colors/UVs and deduplicates equal texture images and
sampler modes. Materials are unlit because lighting is already baked by the decoder.
Host bilinear sampling, RGB combiner multiplication, alpha/layer/decal handling and
lighting are approximations, not an RSP/RDP renderer. No generated mipmaps yet.

The decoder enforces per-list budgets; viewer/report consumers also pass a shared
remaining allowance, capped at 1,000,000 commands including geometry traversal.
The viewer additionally caps output at 1,000,000 triangles, 512 distinct textures
and 128 MiB RGBA data. These are tool limits, not original-engine behavior.

## Collision remains separate

The TERRAIN command names the collision triangle section, not visible render
geometry. Collision data is never used to invent textured terrain. A diagnostic
filled view and cyan line overlay remain available. Edges get a small normal offset
in presentation only; original collision vertices remain unchanged. Line meshes
must not use triangle-only depth bias.

Collision tails, macro/special objects, native behaviors, moving objects, original
camera/audio/gameplay, skyboxes and native environment effects remain absent.

## Headless foundation

`sm64-sim` advances only when explicitly stepped with caller-supplied raw input
and a `TickModel`. There is no built-in movement model. Its static geometric
queries are diagnostic primitives, not the original floor/ceiling/wall solver.
State uses original units and stays independent of Bevy and viewer transforms.
See [simulation boundaries and evidence gate](simulation.md).

## Verification

Authored parser/CLI/mesh tests need no ROM, window or GPU at runtime. Ignored tests
use `SM64_ROM` and skip if absent. Independent MIO0 verification uses an explicitly
selected established decoder; it never fetches or builds programs itself.

Local GPU smoke tests cover textured terrain, collision edges and collision-only
mode. Fixed camera arguments use original game units; matching screenshot dimensions
is necessary for visual comparisons because a window manager can resize the window.
Screenshots are not pixel-exact original-game comparisons or cross-platform proof.
All game-derived output stays ignored, and the source ROM is never modified.
