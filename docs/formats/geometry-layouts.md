# Static geometry layouts

`sm64_level::geo::inspect_geo(&SegmentMap, entry)` reads a segmented geometry
layout, not collision data or a model-address scan. Use the geometry address
retained by the level script's area declaration. The BOB area root is supported;
object model layouts are not automatically instantiated.

## References and provenance

Format research used the public descriptions/macros in
[n64decomp/sm64 `include/geo_commands.h`](https://github.com/n64decomp/sm64/blob/9921382a68bb0c865e5e45eb594d9c64db59b1af/include/geo_commands.h)
and the structural example in
[`levels/bob/areas/1/geo.inc.c`](https://github.com/n64decomp/sm64/blob/9921382a68bb0c865e5e45eb594d9c64db59b1af/levels/bob/areas/1/geo.inc.c).
The decoder and synthetic test fixtures are independently authored Rust; no game
implementation or extracted geometry is incorporated. Research downloads and
local-ROM output belong in ignored `.cache/`, not fixtures or documentation.
These references describe the command format, not a claim of runtime fidelity.

## Reading and traversal

Commands start on four-byte boundaries and contain big-endian fields. Command
length depends on opcode and sometimes parameter bits; it is **not** stored in a
universal length byte. The top byte of a segmented pointer selects a resident
segment; its low 24 bits select an offset. Every command, branch target and
non-null display-list reference is bounds checked. Display-list references must
have at least eight resident bytes; this module does not interpret Fast3D.

Supported control flow:

| Opcodes | Meaning |
| --- | --- |
| `00`, `01` | Branch-and-link, matching end; root end finishes traversal |
| `02`, `03` | Tail branch or branch with return address, matching return |
| `04`, `05` | Open children of the most recently created node, close children |

Every node is relative to its parent, not its previous sibling. Open pushes the
last node's transform; close restores the enclosing parent context. Calls inherit
the current context. This static subset requires calls to balance their node
opens/closes and match their respective terminator; a call cannot close a caller's
node. Unmatched returns, missing nodes, unbalanced ends, bad branch modes and
unknown opcode lengths are errors. Repeated calls to a shared layout are allowed;
no global visited-address set incorrectly discards legitimate instances.

## Draw output and transforms

Each `GeoDraw` contains a segmented display-list address, layer `0..7`, and a
column-major homogeneous `[[f32; 4]; 4]` matrix. Translation is in original SM64
units; the decoder performs no viewer-unit scaling, camera inversion or axis
conversion. Draws remain in traversal order, with their original layer; consumers
must implement layer ordering/material behavior themselves.

* `10`: full translation/rotation, translation-only, rotation-only, Y-only
  compact variants, with optional display list.
* `11`, `12`: translation or rotation, with optional display list.
* `15`: display list using the current parent transform.
* `1d`: uniform unsigned 16.16 scale, with optional display list.
* `0b`: identity grouping node; `1a`, `1e`, `1f`: fixed-size no-ops.

Optional pointers follow the variant's fixed fields and are enabled by parameter
bit 7. Low parameter bits carry the layer. Null display-list pointers produce no
draw. Local transforms are composed as translation times Y rotation times X
rotation times Z rotation (Z acts first on a column vector); parent transforms
multiply on the left. Rotation fields are signed degrees. Floating-point
trigonometry approximates these rotations: original angle-unit conversion and
sine-table quantization are **not** reproduced bit for bit.

## Opt-in actor snapshots

`inspect_posed_geo` is separate from strict terrain inspection. It requires a
complete `JointPose`, explicit switch callback selections, a signed half-open LOD
range test, and an allowlist of native callbacks that may be omitted. Animated
parts consume root translation plus rotational channels in traversal order;
missing/extra channels fail. Inactive branches do not consume channels. All
native code remains unexecuted. See [the Mario profile and evidence](mario.md)
for the exact supported snapshot, XYZ joint convention and limitations.

## Explicit omissions and failures

Screen-area, orthographic, frustum, camera and zbuffer commands are decoded at
their correct lengths but do not change world-space geometry. Diagnostics explain
that projection, camera pose/callbacks and depth-state settings are omitted. Their
static descendants remain traversable. A viewer supplies its own camera and depth
policy; the extracted list is not an exact reconstruction of all render state.

Backgrounds/skyboxes, generated ASM display lists, render-object traversal and
shadows are omitted with address-bearing diagnostics. Native function pointers
are never resolved as segmented data or executed. Culling radii are diagnosed and
ignored (static descendants remain, without visibility tests).

In strict `inspect_geo`, the following fail explicitly rather than choosing an arbitrary visible branch
or rendering an invented bind pose: range/LOD (`0d`), switch/case (`0e`), animated
parts (`13`), billboards (`14`), held objects (`1c`), view registration/copy (`06`,
`1b`), and node flag modification (`07`). These commands may affect transforms,
visibility or shared children. Unknown commands also fail. A failure returns
`Error`, not an apparently complete partial scene.

Limits per inspection are 100,000 commands, 64 open nodes, 64 outstanding calls
and 16,384 draws. These bounds stop loops/recursion and unbounded output; non-finite
transform accumulation is rejected. No native callbacks or unsafe code are used.

## Tests

Authored fixtures cover transform hierarchy/sibling restoration, transform field
variants, call/end and call/return nesting, tail branches, inherited transforms,
unknown commands, truncated prefixes, unsupported dynamic subtrees, bad pointers,
unbalanced nodes, loops, recursion and resource limits. They require no ROM.

An ignored `geo::tests::local_bob_geo` test uses `SM64_ROM`, loads the revision-pinned
BOB area, and prints only draw references, transforms, diagnostics and counts. Run
locally with output redirected to ignored storage:

```sh
CARGO_TARGET_DIR=.cache/target-geo cargo test -p sm64-level
SM64_ROM='/path/to/user-supplied-ROM' CARGO_TARGET_DIR=.cache/target-geo \
  cargo test -p sm64-level geo::tests::local_bob_geo -- --ignored --nocapture \
  > .cache/geo-bob.log 2>&1
```

Real-ROM output is never embedded as a synthetic fixture. Terrain textures,
Fast3D interpretation, moving level objects, gameplay, skybox reconstruction and
native environment effects are outside this module.
