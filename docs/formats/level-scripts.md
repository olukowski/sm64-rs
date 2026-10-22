# Segments, level declarations and static collision

This is data-format inspection, not a reimplementation of the script VM or game
logic. Implementations are written independently in `sm64-level`.

## Segments

The high byte of a segmented address selects one of 32 slots; the low 24 bits are
a byte offset in the slot. Loads name end-exclusive canonical ROM ranges. Raw
loads copy their bytes; compressed loads decode MIO0 before resolving pointers.
A compressed-segment offset is **not** an offset in the ROM file.

Each load is bounds checked, limited to 16 MiB decoded bytes, and recorded.
Resident segments are limited to 64 MiB in total, with at most 512 loads.
Replacement updates the memory accounting. Unloaded/out-of-range addresses fail.

## Revision-pinned entry

Only the exact US revision 0 hash is accepted for course inspection. The loader
maps the verified root script ROM range `0x2abca0..0x2ac6b0` into segment 0x15.
It reads the current-level dispatch at `0x15000278`, finds the equality test for
course ID 9 (BOB), and follows its EXECUTE wrapper. The course script's ROM range
and entry point therefore come from that wrapper, not an asset-signature guess.

This deliberately selects an existing course declaration. It does not run the
original boot sequence, menu, C callbacks, heap operations or a gameplay VM.

## Supported traversal

Commands start with opcode and byte length; lengths are multiples of four.
All multibyte values are big-endian. Every supported instruction has its exact
length checked, and unsupported control flow fails rather than being guessed.

| Opcode | Bytes | Interpretation |
| --- | --- | --- |
| 0x00 | 16 | Root course EXECUTE: segment at +2, ROM range +4/+8, entry +12 |
| 0x05 / 0x06 | 8 | Jump / call a declaration sublist, target +4 |
| 0x07 | 4 | Return from sublist |
| 0x0c | 12 | Root dispatch equality test: comparator +2, course +4, target +8 |
| 0x11 / 0x12 | 8 | Native call / repeated call: stop inspection, never execute |
| 0x17 | 12 | Raw load: segment +2, ROM start/end +4/+8 |
| 0x18 / 0x1a | 12 | MIO0 / MIO0 texture load, same fields |
| 0x1f | 8 | Area index +2 and geometry-layout address +4 |
| 0x20 | 4 | End area |
| 0x24 | 24 | Object acts/model +2/+3, signed XYZ +4, rotation in degrees +10, behavior parameter +16, behavior address +20 |
| 0x2e | 8 | Area collision address +4 |

Other known, non-control declarations are length-checked but not interpreted;
their opcode counts are included in diagnostics. These include Mario setup/position,
warps, music, terrain type and macro-object pointers. Model bindings are retained,
including the layer encoded in direct-display-list declarations. Resident decoded
segments are retained read-only and excluded from level serialization.

Geometry layouts are decoded separately by the [geometry module](geometry-layouts.md);
the level-script traverser only records their pointers.

Traversal has a 20,000-instruction budget, 64-call depth, 8 areas and 4,096 object
declarations per area. These are tool resource limits, not claims about original
engine behavior. Runtime actions, actor spawning, act selection and behavior
callbacks are not simulated.

## Static collision section

Words are big-endian 16-bit values:

1. `0x0040`, vertex count, then signed XYZ triples.
2. Surface batches: surface ID, triangle count, then vertex-index triples.
3. Surfaces `0x0004, 0x000e, 0x0024, 0x0025, 0x0027, 0x002c, 0x002d` add one
   parameter/force word per triangle, whose bits are preserved.
4. `0x0041` ends the triangle section.

Indices must reference the loaded vertex array. Triangle counts, byte ranges and
a 200,000-triangle resource budget are checked. Degenerate triangles are preserved
by the parser; presentation may omit them.

**0x0041 is not the whole stream's end.** Special objects and environment boxes
may follow, terminated by 0x0042. Their variable-size records are intentionally
uninterpreted here. `CollisionMesh::consumed` includes the triangle-stop word but
not this tail. Native object collision meshes are also absent.

## Sources

Format macros and metadata, read for understanding rather than copied as game code:

- <https://github.com/n64decomp/sm64/blob/9921382a68bb0c865e5e45eb594d9c64db59b1af/include/level_commands.h>
- <https://github.com/n64decomp/sm64/blob/9921382a68bb0c865e5e45eb594d9c64db59b1af/include/surface_terrains.h>
- <https://github.com/n64decomp/sm64/blob/9921382a68bb0c865e5e45eb594d9c64db59b1af/sm64.ld>
- <https://github.com/n64decomp/sm64/blob/9921382a68bb0c865e5e45eb594d9c64db59b1af/levels/scripts.c>
- <https://github.com/n64decomp/sm64/blob/9921382a68bb0c865e5e45eb594d9c64db59b1af/levels/bob/script.c>
- <https://hack64.net/wiki/doku.php?id=super_mario_64:collision_data>

All public tests use authored synthetic bytes. ROM-dependent tests are ignored
and require a locally supplied reference ROM.
