# Architecture

```text
sm64x                       CLI inspection and local extraction
  ├── sm64-rom               byte order, header, exact identity, ROM bytes
  ├── sm64-mio0              bounded MIO0 decoding (no ROM dependency)
  └── sm64-level             segment mapping, static course declarations,
                            static collision triangle sections

sm64-viewer (Bevy)           ROM-backed BOB collision visualization and camera
  └── sm64-level             the same format pipeline as the headless CLI

future:
headless simulation         Mario, original collision rules, camera, objects,
                            progression; explicit original-game ticks
rendering assets            geometry layouts, display lists, textures, animation
```

Format crates have no engine dependency. The viewer presents decoded data; it does
not own a gameplay simulation or use generic physics to approximate Mario.
The diagnostic free camera is a viewer tool, not a recreation of Lakitu's camera.

## Identity and addresses

Generic ROM inspection accepts recognized N64 byte orders and normalizes in memory
to big-endian (.z64) order. Hashes and ROM offsets use that canonical representation;
never infer byte order from the extension.

Revision-specific course inspection requires the exact US revision 0 hash before
using bootstrap locations. It selects BOB from the root current-level dispatch,
loads segments from commands, and resolves low-24-bit offsets within their decoded
bytes. This is static declaration traversal, not execution of the native boot/menu
sequence or runtime callbacks. Unsupported control flow errors rather than guesses.

MIO0 signatures remain candidates, not an authoritative asset table. The independent
verification script requires an explicitly selected established decoder executable,
never automatically fetches one, and retains private evidence under ignored cache.

## Collision is not rendering geometry

The initial viewer uses the static triangle section named by the course's TERRAIN
command. Geometry-layout pointers are recorded but not decoded. Collision tails,
macro/special objects, behaviors, moving objects, textures and original materials
are not represented. Diagnostic colors must not be described as original visuals.

## Testing

Synthetic parser, CLI and mesh tests run without a ROM, window or GPU. Ignored
reference tests use `SM64_ROM` and skip with a message if absent. A live screenshot
smoke test is a separate local validation; it does not establish rendering fidelity.
All ROM-derived output stays in ignored directories and no tool modifies the ROM.
