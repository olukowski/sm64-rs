# Bob-omb Battlefield — initial static inspection

## Observed on the canonical US revision 0

- Root current-level dispatch selects course ID 9 through a wrapper at
  `0x15000458`.
- Wrapper loads raw script segment 0x0e from ROM `0x405a60..0x405fb0`;
  script entry is `0x0e000264`.
- Course commands load MIO0 segment 7 from ROM `0x3fc2b0..0x405a60`,
  producing 71,618 bytes.
- Area 1 names geometry layout `0x0e000488` and collision `0x0700e958`.
- Static traversal follows local/global declaration calls: 109 commands,
  11 segment loads, one area and 30 object declarations.
- Collision triangle section: 570 vertices, 1,060 triangles, 11 surface IDs,
  9,830 bytes through triangle stop. No force-word surfaces occur in this mesh.
- The following tail contains special objects; it is not decoded.

These are compatibility identifiers and aggregate counts, not embedded assets.
Private JSON and extracted data stay in ignored cache directories.

## Reproduce

```sh
# Set SM64_ROM to your own US revision 0 file.
cargo run -p sm64x -- level bob
cargo run -p sm64x -- level bob --json
cargo test -p sm64-level -- --ignored --nocapture
```

## Scope

Static declarations are not live entities. Macro/special objects, native behaviors,
moving platforms and object collision are not instantiated. The course's visible
rendering geometry is separate from the static collision mesh. The initial Bevy
viewer visualized this collision mesh with diagnostic colors, not original
textures or N64 materials. The subsequent [textured-terrain milestone](textured-bob.md)
adds separate original render geometry/textures and keeps collision as a view/overlay.

## Initial collision-viewer smoke test

The Bevy viewer was built and launched locally against this ROM. The rendered
PNG shows all 1,060 nondegenerate triangles, a bounds-fitted overview and clear
static-collision/diagnostic-color labeling. Screenshots remain ignored in
`.cache/`; they are not shipped assets or an original-rendering comparison.
This verifies one local GPU/window-system run, not cross-platform support.
The verified image is `.cache/bob-viewer-smoke.png`; the corresponding local log
is `.cache/reports/viewer-smoke.log`. These ignored artifacts may be regenerated.

At the initial course-inspection milestone, formatting and workspace Clippy passed;
58 install-free tests and both ignored ROM integration tests passed. This is
that milestone's validation snapshot, not a promise about future test counts.

The loader does not emulate boot/menu behavior. It selects BOB in the verified
root dispatch table and stops before the native initialization/update call.
See [format notes](../formats/level-scripts.md) for sources and validation limits.
