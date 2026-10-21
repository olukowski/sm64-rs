# Headless foundation validation

## Implemented scope

The headless-foundation and trace-comparison milestones established engine-free tick/input/state APIs,
static geometric probes, versioned observation traces, caller-model replay and
read-only inspector commands. They do not implement Mario actions or the original
collision solver. See [simulation.md](../simulation.md) for exact semantics and
remaining evidence requirements.

## Local checks

- `cargo fmt --all --check` passed.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` passed.
- With `SM64_ROM` unset, `cargo test --workspace --locked` passed: 116 tests,
  seven ignored ROM-dependent tests. Fixtures are authored synthetic data.
- With the locally selected reference ROM, all seven ignored workspace tests
  passed. Identity was reconfirmed as canonical US revision 0; the `.n64` filename
  still contains pair-swapped data, normalized only in memory.
- The new BOB test constructs the query world and samples a bounded XZ grid,
  checking finite floor results and preserved source surface IDs.
- The new CLI collision smoke test verifies metadata, invalid-query failures and
  byte-for-byte unchanged ROM contents before/after the probe.
- `trace validate` and `trace compare` passed on the explicitly authored example.
  End-to-end tests use an invalid `SM64_ROM` path to verify these commands do not
  open a ROM. Injected divergence exits nonzero with a structured first mismatch.
- Trace tests exercise strict fields/version/revision, finite state, input history,
  tick alignment/overflow, signed-zero float bits, explicit tolerance, unequal
  lengths, bounded reads and rejected oversized output.

An independent review found two edge cases, reproduced with authored data before
fixing: sloped source vertices could round outside inclusive height bounds, and a
closed stdout pipe could mask trace divergence as success. Regression tests now
cover both windings at sloped vertices and closed-pipe comparisons. Height queries
use unnormalized plane coefficients, and comparison output failures remain errors.

These results validate tool behavior, mathematical queries and test-model replay.
They are **not** original-game state comparisons or cross-platform bit-exactness
proof. No native game function was executed by this validation.

All local test logs and ROM-derived output remain ignored in `.cache/`; the
checked-in JSON example is independently authored, not a captured trace.

## Reproduce

```sh
env -u SM64_ROM cargo test --workspace --locked
# Set SM64_ROM explicitly to the user's legally supplied US revision 0 image.
cargo test --workspace --locked -- --ignored --nocapture
cargo run -p sm64x --locked -- collision bob --at 0,1000,0 --json
cargo run -p sm64x --locked -- trace validate docs/examples/authored-trace.json
cargo run -p sm64x --locked -- trace compare \
  docs/examples/authored-trace.json docs/examples/authored-trace.json --json
```

## Next external dependency

At these foundation commits, no reference capture runner was configured. A bounded local check found none of
the checked common N64 emulator executables on PATH; that is not proof no emulator
exists anywhere on the machine. The user ROM is available, but a trusted debug/
emulator runner and independently verified runtime field/tick mapping are still
needed. Capture initialization and timing must be validated before implementing
and claiming faithful walking/jumping behavior. Installing/downloading a ROM is
neither needed nor allowed. Subsequent work supplied a pinned local runner and
repeatable US0 action captures; see [reference emulator notes](../reference-emulator.md).
