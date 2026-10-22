# Roadmap

## M0 — Foundation
- [x] Local Git repository, Rust workspace, licenses and provenance policy
- [x] Install-free CI
- [x] ROM inspector: canonical identity, N64 header, MIO0 inventory and decoding
- [x] Validate inspector against a local ROM; keep output private (US revision 0; 79/79 MIO0 candidates decode)
- [x] Independently compare decompressed bytes against another decoder (79/79 byte-for-byte matches)

## M1 — See a course
- [x] Verified US revision: segment mapping and static BOB level declarations
- [x] Static collision triangle decoding (570 vertices, 1,060 triangles on BOB)
- [x] First Bevy collision-mesh viewer and local screenshot smoke test
- [x] Preserve decoded segment access/model bindings for visual asset loading
- [x] Decode BOB's static geometry-layout hierarchy, transforms and render layers
- [x] Decode the Fast3D display-list subset used by BOB (vertices, triangles, nested lists, render state)
- [x] Decode the texture formats/material state used by BOB and render textured static terrain
- [x] Add a collision overlay and fixed-camera visual checks (window dimensions must match for comparisons)

### Static-viewer milestone

A textured static Bob-omb Battlefield viewer, with collision visualization still
available for alignment checks. Verified locally: 1,101 render triangles and 18
uploaded textures. See [validation notes](research/textured-bob.md). Start from the actual geometry-layout pointer,
not the collision mesh: these are different assets. Native geometry callbacks,
animated objects and unsupported commands must be reported rather than silently
guessed. Expanding course/rendering coverage remains separate from native simulation.
Rendering currently approximates N64 material effects; differences are labeled.
Future rendering work may add fixed-resolution offscreen regression captures,
more accurate combiners/lighting/filtering, skybox and generated effects.

## Presentation showcase
- [x] ROM-backed static Mario center-idle snapshot, explicit normal-cap graph profile
- [x] ROM skybox and coherent approximate presentation lighting/fog
- [x] Showcase camera, clean mode and fixed-resolution offscreen screenshots
- [x] Simple public-facing demo instructions and publication hygiene recheck

The screenshot-worthy stopping point is reached: [static Mario/BOB showcase](showcase.md).
This is not a playable slice; source publication and any public hosting of images
remain explicit, separate decisions.

## M2 — Authentic Mario
- [x] Engine-free explicit-tick/input/state foundation (caller-supplied model)
- [x] Authored static geometric probe tests (not original collision rules)
- [ ] Headless, fixed-tick Mario actions and original collision rules
- [x] Versioned observation/input traces, caller-model replay and first-divergence comparisons (authored tests)
- [x] Local pinned US0 reference capture adapter, initial field/boundary corroboration and repeatable movement observations
- [x] First bounded native flat-walk model and exact original-versus-native comparison
- [x] Short-release extension: all 1–9-tick holds independently repeated and matched through first idle
- [ ] Expand native actions/input coverage and implement original collision resolution
- [ ] Animation and camera integration

See [simulation.md](simulation.md) for implemented boundaries and the reference
capture contract; [local emulator notes](reference-emulator.md) document the first
repeatable original-game observations. Mathematical probes and toy replay tests must not be
reported as verified Mario behavior. The [first movement slice](movement-slice.md)
validates idle → straight acceleration → braking to first stop on an explicitly
verified flat lane; it is not a general controller or playable viewer.

## M3 — First playable slice
- [ ] Objects, coins, damage/death, one earnable star
- [ ] Controller input, HUD and initial audio

## Later
Castle and course transitions, saves, remaining courses/behaviors, all 120 stars,
audio and rendering fidelity. No timetable until the first pipeline is validated.
