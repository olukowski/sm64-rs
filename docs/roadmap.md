# Roadmap

## M0 — Foundation
- [x] Local Git repository, Rust workspace, licenses and provenance policy
- [x] Install-free CI
- [x] ROM inspector: canonical identity, N64 header, MIO0 inventory and decoding
- [x] Validate inspector against a local ROM; keep output private (US revision 0; 79/79 MIO0 candidates decode)
- [ ] Independently compare decompressed bytes against another decoder

## M1 — See a course
- [ ] Verified US revision: segment mapping, level scripts and geometry layouts
- [ ] Display lists, textures and a Bob-omb Battlefield viewer

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
