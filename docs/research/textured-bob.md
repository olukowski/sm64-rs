# Textured static Bob-omb Battlefield milestone

## Observed pipeline

The exact US revision 0 ROM is normalized read-only, loaded through its course
dispatch and decoded using the area's geometry pointer, not its collision mesh.

- Geometry layout: 32 commands, 6 static draw references, all identity transforms.
- Display lists: 1,521 commands, 1,101 render triangles.
- All 1,101 triangles reference decoded textures.
- 23 per-list texture entries become 18 distinct uploaded images/samplers.
- Actual texture formats used: RGBA16 and IA16, with 32×32 and 32×64 rectangles.
- Static collision stays independent: 570 vertices and 1,060 triangles.

These are aggregate compatibility observations, not embedded assets.
Sources and details: [geometry layouts](../formats/geometry-layouts.md),
[Fast3D](../formats/fast3d.md), [course inspection](bob-course.md).

## Local GPU checks

The viewer was run on Linux/Wayland with an NVIDIA Vulkan driver. This is one local driver/backend check,
not a hardware requirement or cross-platform validation.
The saved PNGs were inspected: original grass/rock/path/bridge/fence textures are
visible on the original terrain meshes; diagnostic cyan collision edges align
with the terrain while exposing differences between collision and visual geometry.
Collision-only mode remains functional.

Private artifacts:

- `.cache/bob-textured-fixed.png` — textured, fixed camera.
- `.cache/bob-textured-overlay.png` — same camera, collision edges.
- `.cache/bob-collision-mode.png` — collision-only overview.
- `.cache/reports/bob-visuals-first.json` — metadata-only headless visual inventory.

No images/assets or extracted ROM bytes are tracked. The source ROM's SHA-256 was
checked unchanged after all rendering runs.

## Reproduce

Set `SM64_ROM` to your own US revision 0 ROM. Use a **new** output filename for
each capture; screenshots never overwrite existing files.

```sh
mkdir -p .cache
cargo run -p sm64x -- level bob --visuals --json
cargo run -p sm64-viewer -- \
  --camera=-8500,6500,11000 --look-at=0,1000,0 --shot .cache/bob-fixed-new.png
cargo run -p sm64-viewer -- \
  --camera=-8500,6500,11000 --look-at=0,1000,0 \
  --collision-overlay --shot .cache/bob-overlay-new.png
cargo run -p sm64-viewer -- --mode collision
```

C toggles edges; V toggles textured/collision-only mode. Camera arguments reproduce
the pose, not original-game pixels. At this historical terrain milestone,
screenshots used a compositor-sized window. The subsequent
[showcase capture path](../showcase.md) now renders fixed-resolution offscreen
images without a window; use that path for new comparisons.

## Validation and limitations

Workspace formatting, Clippy and synthetic tests pass. Five ignored local-ROM tests
exercise identity/MIO0, course collision, geometry hierarchy, Fast3D roots and the
CLI visual report; they require the local ROM, not CI fixtures.

One initial overlay smoke test failed GPU validation because depth bias is invalid
for LineList topology. During the resulting shutdown, an NVIDIA compiler worker
also segfaulted. The application-side invalid state was removed,
a material regression test added, and overlay/collision captures rerun successfully
with clean render logs. The driver stack alone does not establish its internal
failure mechanism. No system configuration or driver packages were changed.

This milestone does not implement objects, Mario, gameplay, audio, original camera,
skybox or generated environment effects. Native geometry callbacks are never
executed. RGB combiners, lighting, alpha/layer state and host bilinear filtering
are approximate; no RDP three-point filtering, fog fidelity, integer rasterization,
generated mipmaps or pixel-exact original-renderer comparison is claimed.
