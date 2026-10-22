# sm64-rs

A behavior-faithful native reimplementation of Super Mario 64 in Rust. Bevy is the
presentation layer, not the source of gameplay physics. The current viewer shows
textured static terrain with approximate materials and diagnostic collision views,
plus a ROM-backed static Mario idle/sky showcase and fixed-resolution windowless
captures. Showcase placement/camera/fog/shadow are presentation-only policies.
A bounded headless flat-walk slice exists; the viewer is not playable and general
movement/original collision remain unimplemented. This is not an emulator.

## Provenance and safety

- Never commit ROMs, extracted assets, game code, disassembly, decompiler output,
  or reference traces. Game-derived output belongs in ignored `.cache/`,
  `extracted/`, or `traces/` directories. The one exception to this rule is showcase screenshots,
  those are allowed but must be clearly labelled as derived work.
- Open the user's ROM read-only; never patch or modify it. Do not download ROMs.
- Format knowledge may come from documented prior art; credit it. Public decompilation
  does not automatically grant permission to copy or relicense game code.
- Write implementations afresh. Cite the ROM revision/address or documented reference
  for behavior, and distinguish observations from assumptions.
- Pin compatibility to a canonical ROM hash, not its filename or header alone.
- See `docs/adr/0001-provenance.md`.

## Architecture

- Pure Rust format crates have no Bevy dependency. No unsafe code.
- Simulation is headless and advances in explicit original-game ticks.
  Rendering can interpolate independently.
- Do not substitute generic physics or camera behavior for the original rules.
- Inspect data with `sm64x`; keep private output out of Git.
- Unsupported revisions may be inspected generically, but must not use US-specific
  asset offsets without exact revision identification.

## Checks

Run `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
and `cargo test --workspace` before committing.
Tests must pass without a ROM. Use authored synthetic fixtures.
ROM-dependent tests are ignored by default, use `SM64_ROM`, and skip with a
message if it is unset. Never embed game bytes in fixtures.

## Working practices

Keep changes and documentation aligned. Commit at logical milestones.
Do not add a remote or publish unless explicitly requested.
