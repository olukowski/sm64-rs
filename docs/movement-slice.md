# First native movement slice: bounded flat walking

## What works

`sm64-sim::movement::FlatWalk` is an engine-free `TickModel` implementing one
short US revision 0 sequence:

1. Rest in **idle**, with up to five neutral ticks.
2. Apply raw stick **(0, 80)** for **10–30 ticks**, facing binary yaw `-32768`
   (straight along -Z).
3. Release to neutral; brake until the **first braking-stop observation**.

No buttons are supported. The model starts at idle/rest and finishes at that
first stop; it deliberately refuses another tick. It does **not** simulate the
stop animation, return-to-idle timing, sleep timers, turning, variable stick
strength, resuming during braking, jumping, slopes, walls, water, objects or camera.
The five/30-tick budgets bound this experiment's evidence, not game rules.
Releasing before ten walking ticks is explicitly unsupported: low-speed release
uses a different deceleration action, not this slice's braking action.

The viewer is still a static terrain viewer. This milestone is headless native
movement, not a playable game or a general collision implementation.

## Try it without a ROM

```sh
cargo run -p sm64x -- movement run docs/examples/authored-movement.json

# Optionally retain the emitted native observation trace privately:
mkdir -p .cache
cargo run -p sm64x -- movement run docs/examples/authored-movement.json \
  > .cache/authored-native.json
cargo run -p sm64x -- trace validate .cache/authored-native.json
```

The scenario's state, lane and inputs are **authored synthetic values**, not a
captured game fixture. Its output is correctly labeled `native`: it is computed
by the Rust model, with explicitly synthetic initialization. The command never
opens a ROM or reference trace, even if `SM64_ROM` is set. It validates/runs the
whole sequence before writing JSON to stdout. Shell redirection is the caller's
responsibility; use a new ignored destination. Broken output pipes fail.

## Initialization and terrain contract

A strict version-1 scenario declares:

- Scenario/protocol identity, initialization description and starting tick.
- Initial observed state and previous raw input (neutral for this slice).
- An explicit `FlatLane`: finite X/Z bounds and one horizontal floor height.
- Input segments with explicit tick counts; the sequence must end at its first
  stop. Zero-length, incomplete and overlong sequences are errors.

Native initialization is performed **once**, before the sequence. There is no
per-tick reading, correction, fitting, or lookup of reference output states.
The state must be grounded/resting in idle, with fixed -Z facing. There are no
unmodeled action/animation timers affecting the supported output transitions:
execution ends before the stop animation would require them. This is why Trace
v1 can remain an observation format rather than pretending to be a save-state.

The lane is a **caller assertion** of ordinary, unobstructed, static flat ground,
not imported castle geometry or invented gameplay collision. Every quarter-step
is checked against its bounds; leaving it is an error, not a ledge/wall response.
The original floor search, spatial traversal, step rules and wall pushes remain
unimplemented. `collision::World` diagnostic probes are not used by this model.
Nothing in this API can validate a caller's assertion about arbitrary terrain.

## Evidence and independently written arithmetic

Compatibility is gated on canonical US0 SHA-256
`17ce077343c6133f8c9f2d6d6d9a4ab62c8cd2aa57c40aea1f490b4c8bb21d91`.
The [pinned local reference runner](reference-emulator.md) observes entry
`0x80254830` and its runtime caller return, using MarioState at `0x8033b170`.
Action IDs, field offsets and surface layout are documented metadata from the
pinned references linked there; no game implementation bodies were used.

The implementation was written afresh from paired runtime observations. The
supported observations are reproduced by this recurrence:

- Entering walking starts from forward speed 8 and accelerates in that same tick.
- Walking: `v = f32(v + f32(1.1_f32 - f32(v / 43_f32)))`.
- Neutral braking subtracts 4 per tick, clamped to zero. Release changes the
  action and brakes in the **same** tick. Reaching zero emits braking-stop.
- Velocity is `[+0, +0, -v]`, including **negative zero** at rest.
- Position is advanced with **four separately rounded f32 quarter-additions**,
  not one full addition. Y remains the asserted floor height.

These are observations/inferences within the declared envelope, not proof that
this is the only possible internal arithmetic or the complete original solver.
The quarter-addition inference distinguishes the observed positions from a
single-addition model. No hardware timing or cross-platform bit-exactness claim
is made. Out-of-envelope inputs fail instead of invoking a generic substitute.

Capture diagnostics additionally record floor type/normal/vertices/height,
ceiling/water height, wall/held/ridden object pointers, squish/quicksand state,
processed stick, intended magnitude/yaw, and selected action/dialog state. All such game-derived output remains ignored.
A post-update intended magnitude of 32 and yaw `-32768` corroborates the full
forward input in this scenario; general stick processing and camera math are
**not** implemented by recognizing just this one raw input.

## Local differential validation

Two independent empty-save cold boots were captured with the authored
`docs/examples/castle-walk-jump-inputs.json` schedule. The selected movement window
starts after 1,957 action calls: five neutral ticks, 30 walking ticks, then seven
braking ticks through the first stop. This is castle ground, **not BOB**; no BOB
collision queries are mixed into the test.

All **42 transitions**, their initial observation and previous input matched
between both reference runs and a separately executed native scenario, using the
Rust comparator's **exact f32 bits** policy for every declared float, and exact
inputs/yaw/action. No tolerance was used. Additional independent cold boots tested
**every integer hold duration from 10 through 30 ticks**: 21 cases and 653
transitions. Each native sequence matched all observations through its first stop
exactly. One- and nine-tick release probes
exposed a different low-speed deceleration action, now rejected rather than
misrepresented as braking. The native run consumed only an explicit
initial rest observation, flat-lane assertion and authored input segments—not
reference post-states, a ROM, or emulator state. This validates that initialized
slice; it does not validate native boot/intro or hidden full-game state.

Reproduce reference acquisition with a new output directory for each run:

```sh
export SM64_ROM="/path/to/your/rom.z64"
python3 -m scripts.reference_emulator.capture \
  --out .cache/emulator/flat-a --profile us0-action \
  --schedule docs/examples/castle-walk-jump-inputs.json \
  --updates 1999 --max-vis 5200 --timeout 240 \
  --export-trace --trace-start 1957 --scenario flat-straight-v1
# Repeat into .cache/emulator/flat-b, then compare both trace.json files.
```

Keep the explicitly initialized native scenario under `.cache/` as well. Use the
synthetic example's schema, neutral previous input, the observed protocol boundary
strings, and the independently corroborated initial idle/rest state and safe lane.
Do not commit actual captured initialization, terrain vertices or output traces.
Validate that the private reference actually satisfies the declared envelope,
then run the native model independently and compare:

```sh
python3 -m scripts.reference_emulator.movement_check \
  --probe .cache/emulator/flat-a/probe.json \
  --scenario .cache/flat-native-scenario.json --out .cache/flat-a-checked
cargo run -p sm64x -- movement run .cache/flat-native-scenario.json \
  > .cache/flat-native.json
cargo run -p sm64x -- trace compare \
  .cache/flat-a-checked/trace.json .cache/flat-native.json --json
```

The envelope checker verifies the full capture chain/schedule, actual predecessor,
one-time initial-state bits, input protocol, and each selected before/after
environment. It conservatively requires the entire lane rectangle inside every
observed ordinary horizontal floor triangle, no wall/dialog/held/ridden/squish/
quicksand condition, ceiling clearance of at least 1,000 units, and water more than
160 units below the floor. Those clearance limits are safety assertions, not
original game rules. It does **not** calculate native output states, validate
capture authenticity, or prove the absence of all possible interactions.

Changing a reference field, native input, or model arithmetic must produce a divergence or
explicit unsupported-case error—not silently adjust the native run.

Authored ROM-free tests cover every supported hold duration, state/input/phase
validation, signed zero, quarter rounding, horizon limits, lane escape, tick
failure atomicity and complete/incomplete scenarios. Local reference evidence
is separate from those synthetic tests. See [simulation boundaries](simulation.md)
and [provenance policy](adr/0001-provenance.md).

## Next extensions

Add repeatable independent initialization in BOB, additional stick strengths and
camera-relative directions, then original floor/ground-step resolution before
slopes or wall contact. Animation-aware stop-to-idle, jump/landing and a complete
versioned hidden-state restore format are separate milestones. Do not relax this
model's explicit rejection boundaries without new reference evidence.
