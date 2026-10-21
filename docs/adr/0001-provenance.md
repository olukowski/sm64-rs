# ADR 0001 — Provenance and ROM handling

Status: accepted

## Decision

1. Content comes from a user-supplied ROM, opened read-only. Never acquire ROMs
   or commit game-derived files.
2. Prefer parsing data in place. Deterministic extraction tools may write to
   ignored local output directories; generated data is not source code.
3. Reverse engineering and documented community research inform understanding.
   Implement fresh Rust, not copied or mechanically translated game/decompiler code.
4. Use hashes to identify exact canonical revisions. Byte-swapped copies should
   identify identically after normalization.
5. Verify behavior using the original as an oracle where possible. Commit capture
   tools and authored tests, not captured game data.
6. Clearly separate parser validation, exact revision recognition, and behavioral
   fidelity. One does not establish the others.

## Consequences

CI runs without a ROM. Private, ignored integration tests can inspect a local ROM.
Public documentation records format knowledge in our own words, with sources,
not raw game bytes. A recognized header alone never establishes authenticity.
