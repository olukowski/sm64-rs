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
- [ ] Preserve decoded segment access/model bindings for visual asset loading
- [ ] Decode BOB's static geometry-layout hierarchy, transforms and render layers
- [ ] Decode the Fast3D display-list subset used by BOB (vertices, triangles, nested lists, render state)
- [ ] Decode the texture formats/material state used by BOB and render textured static terrain
- [ ] Add a collision overlay and repeatable fixed-camera visual checks

### Recommended next stopping point

A textured static Bob-omb Battlefield viewer, with collision visualization still
available for alignment checks. Start from the actual geometry-layout pointer,
not the collision mesh: these are different assets. Native geometry callbacks,
animated objects and unsupported commands must be reported rather than silently
guessed. Do not expand to every course or start gameplay in this rendering batch.
Rendering may initially approximate N64 material effects; label those differences.

## M2 — Authentic Mario
- [ ] Headless, fixed-tick Mario actions and original collision rules
- [ ] Reference-input capture/replay and state comparisons
- [ ] Animation and camera integration

## M3 — First playable slice
- [ ] Objects, coins, damage/death, one earnable star
- [ ] Controller input, HUD and initial audio

## Later
Castle and course transitions, saves, remaining courses/behaviors, all 120 stars,
audio and rendering fidelity. No timetable until the first pipeline is validated.
