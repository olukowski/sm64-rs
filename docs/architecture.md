# Architecture

```text
sm64x                       CLI inspection and local extraction
  ├── sm64-rom               byte order, header, identity, ROM access
  └── sm64-mio0              bounded MIO0 decoding (no ROM dependency)

planned:
Bevy presentation           rendering, input, audio
headless simulation         Mario, collision, camera, objects, progression
ROM assets                  level/behavior scripts, geometry, textures, animation
```

No engine dependency belongs in the ROM or codec crates. The future simulation
owns gameplay state and advances in original-game ticks; Bevy presents it.

Inspection accepts recognized N64 byte orders and normalizes in memory to
big-endian (.z64) order. Identity hashes and reported ROM offsets refer to that
canonical representation; never infer byte order from the extension.

Generic inspection must remain separate from revision-specific interpretation.
MIO0 signatures are candidates, not an authoritative asset table.
