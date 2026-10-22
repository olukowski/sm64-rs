# Static Mario idle snapshot (US revision 0)

This is a presentation asset path, not native Mario behavior, a character
controller, or an original render-graph interpreter. Game bytes, pose samples,
textures and screenshots stay in memory or ignored local output.

## Provenance and asset identity

All revision-specific reads require the canonical US0 identity enforced by
`sm64-rom`. Documented metadata references are pinned to
[n64decomp/sm64 revision 9921382a68bb0c865e5e45eb594d9c64db59b1af](https://github.com/n64decomp/sm64/tree/9921382a68bb0c865e5e45eb594d9c64db59b1af):

- [`include/level_commands.h`](https://github.com/n64decomp/sm64/blob/9921382a68bb0c865e5e45eb594d9c64db59b1af/include/level_commands.h)
  and [`sm64.ld`](https://github.com/n64decomp/sm64/blob/9921382a68bb0c865e5e45eb594d9c64db59b1af/sm64.ld): segment/model declaration formats and group0 residency.
- [`include/geo_commands.h`](https://github.com/n64decomp/sm64/blob/9921382a68bb0c865e5e45eb594d9c64db59b1af/include/geo_commands.h): geometry node layouts.
- [`include/types.h`](https://github.com/n64decomp/sm64/blob/9921382a68bb0c865e5e45eb594d9c64db59b1af/include/types.h)
  and [`src/game/memory.h`](https://github.com/n64decomp/sm64/blob/9921382a68bb0c865e5e45eb594d9c64db59b1af/src/game/memory.h): Animation/DMA table declarations, flags and channel-count metadata.
- [`include/mario_animation_ids.h`](https://github.com/n64decomp/sm64/blob/9921382a68bb0c865e5e45eb594d9c64db59b1af/include/mario_animation_ids.h): animation names/IDs.

These are format/layout declarations, not permission to copy game implementation
bodies. The parsers and synthetic fixtures were written afresh; no game code,
disassembly, decompiler output or downloaded asset is part of the implementation.

The root segment at canonical ROM `0x2abca0..0x2ac6b0` contains:

| Declaration | Meaning |
| --- | --- |
| `0x15000000` | Compressed segment 04, graphics range `0x114750..0x1279b0` |
| `0x15000018` | Raw segment 17, layouts range `0x1279b0..0x12a7e0` |
| `0x15000040` | Model 1 geometry binding, `0x17002dd4` |

The loader checks command types/lengths/slots and obtains ranges and the geometry
entry **from these declarations**, not an asset signature search. It never runs
the boot script, native callbacks or object behaviors.

## Animation DMA table and sampling

Canonical US0 animation data occupies `0x4ec000..0x579c20`. The first eight bytes
are a big-endian descriptor count (209) and stored source address (zero), followed
by 209 `(offset,length)` pairs. Offsets are relative to the **table start**;
payloads may overlap, so adjacent descriptor offsets do not imply lengths.
Structural corroboration checked all descriptors against their animation lengths,
indices and value bounds, with the next demo DMA table starting at `0x579c20`.

The selected snapshot is explicitly **MARIO_ANIM_IDLE_HEAD_CENTER (`0xc5`), frame
0**. Left/right idle variants also exist; this is not a claim about which variant
gameplay chooses first. Descriptor `0x4ec630` selects payload
`0x57344c..0x573cf6`.

An animation begins with a 24-byte big-endian header: flags, Y translation
divisor, start/loop-start/end frame fields, declared bone count, values/index
pointers and payload length. Here the stored pointers are **payload-relative byte
offsets**, not segmented/RAM/ROM-absolute addresses. The selected payload declares
20 rotational joints, 30 frames, divisor 189 and NOLOOP only.

Its 63 index pairs `(count,valueIndex)` represent three root-translation channels
followed by 20 XYZ rotational triplets. Every channel range is checked, even when
sampling frame zero. A nonnegative frame selects
`values[valueIndex + min(frame,count-1)]`; there is no interpolation. Root samples
remain signed source-unit translations; rotation samples are signed binary angles
(one turn = 65,536). The sampler rejects unsupported root-translation flags,
nonzero start/loop-start semantics, invalid descriptors and malformed indices.

The selected geometry contains 20 active animated parts, including null-DL
joints. Each consumes its rotation triplet; the first **also** adds root
translation. This correspondence is corroborated by the selected skeleton and
channel counts and by a read-only original-runtime probe at
`geo_process_animated_part` (`0x8027ca70`). The first null-DL part advanced the
attribute pointer by six pairs, each later part by three; all 63 pairs were used.
Runtime local matrices at `mtxf_rotate_xyz_and_translate` (`0x803795f0`) corroborated
column-vector **Rz × Ry × Rx** for animated joints, unlike static geo-node ZXY
rotation. The fresh sampler uses regular trigonometry, not original sine-table
quantization. Symbol addresses came from the pinned STROOP map cited in
[reference-emulator.md](../reference-emulator.md); no implementation bodies were read.

The same C5/frame-0 runtime observation corroborated object Y-translation 189,
divisor 189 and multiplier 1.0. Generic non-unit runtime animation-height/divisor
scaling is **not reconstructed**. The static
viewer uses the unmodified source translation and places the resulting model's
lowest vertex on independently checked horizontal terrain. This visual grounding
is not a gameplay floor collision, spawn position or locomotion rule.

## Explicit graph profile

Strict terrain `inspect_geo` still rejects ambiguous animated/switch/LOD nodes.
The opt-in `inspect_posed_geo` requires a complete joint pose, explicit callback
switch selections, render distance and an omission allowlist. Missing/out-of-range
selections, wrong joint counts and unknown active ASM callbacks fail. Inactive
branches consume no joints or draw commands; their structure remains bounded.

The Mario profile selects normal, near-detail, opaque, cap-on, open-eye and normal
hand branches. It explicitly suppresses the two wing display-list nodes
`0x170003cc/0x17000404` (both `0x0401ca00`), rather than assuming omitted native
callbacks will hide them. It omits only the known torso/head/hand/material callbacks, plus an
empty held-object node, with diagnostics. Native pointers are metadata, never
executed. No blinking, torso/head look, hand scaling, cap effects, metal/wing forms,
held objects, original shadows or animation state machine are reproduced.

## Rendering and checks

The same bounded Fast3D texture/triangle pipeline renders both terrain and actor.
Actor roots preserve inherited material/light/texture state; vertex-cache reuse
across independently transformed joints is rejected instead of applying a new
joint transform to old vertices. Materials and object-space baked lighting remain
approximate, not an RSP/RDP renderer. Actor layers 0–3 are explicitly opaque;
zero vertex alpha must not make ordinary opaque limbs disappear. Selected
primitive/environment alpha replaces shade alpha instead of multiplying a raw
zero vertex byte.

Authored tests cover descriptors, bounds, channel sampling, pose hierarchy,
selection/LOD, callbacks and failure limits without a ROM. Optional ignored tests
use `SM64_ROM` and validate actor declarations, the idle snapshot and selected
Fast3D roots. Real-ROM output is never embedded as a public test fixture.
