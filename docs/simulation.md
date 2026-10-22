# Headless simulation and reference validation

## Status and boundaries

`sm64-sim` provides engine-free explicit ticks, raw state/button edges, diagnostic
geometric probes, and a [bounded native flat-walk model](movement-slice.md).
No Bevy time, physics engine, render scale or wall-clock accumulator participates.
Callers explicitly select a `TickModel`; there is no implicit general controller.

`MarioState` currently stores position, velocity, forward velocity, binary facing
angle and uninterpreted action bits. This is a small observation surface, not a
complete original state or save-state: action timers, collision references, object
state, camera-relative intended movement, animation and other state are absent.
Default action zero does not mean idle. Coordinates are original game units.

A transition numbered N consumes raw input N and produces post-transition state N.
The runner's counter identifies the **next** transition; it increments once per
successful `step`. Restoring a snapshot must also restore previous raw input to
avoid turning held buttons into new presses. Stick values remain signed bytes;
normalization/deadzones are not guessed. All 16 raw button bits are preserved.

Invalid initial/output floats, model errors and counter overflow fail explicitly.
State, input history and tick commit together. Model-internal effects cannot be
rolled back; models needing retry determinism must manage those separately.
Finiteness alone does not establish correct gameplay or portable bitwise results.

## Static geometric probes

`collision::World` consumes `sm64-level::CollisionMesh`, validates indices and
retains source triangle index, surface ID, force and winding. Degenerates are
reported and skipped without renumbering surviving triangles. Integer vertices
are converted to f64 for independently written geometric calculations. Height
queries retain unnormalized integer-derived plane coefficients so source vertices
remain on inclusive height boundaries; normalized normals are only used for
classification and reporting.

The caller explicitly chooses a vertical-normal threshold. Normals above it are
floor candidates, below its negative are ceiling candidates; others are walls.
This is a diagnostic policy, **not a verified original-engine classifier**.
No surface-ID exclusions or gameplay effects are applied.

- `vertical(x,z,min_y,max_y)`: highest upward-facing and lowest downward-facing
  triangles within an inclusive interval. Both windings are tested in XZ;
  edges/vertices are included. Equal-height ties retain source order.
- `walls(position,radius)`: two-sided candidates whose horizontal triangle slice
  is within the requested XZ radius. Returns closest points, not a pushed position.
  Corners may yield multiple contacts; coplanar triangles are not deduplicated.

These are not Mario floor/ceiling/wall collision rules. Original coordinate
conversion, spatial partition traversal, wall projection/push ordering, one-sided
contacts, step tolerances, special-surface exclusions and moving collision still
need revision-pinned behavioral evidence and separate tests. Do not use these
probes as a substitute for that solver. Linear traversal is intentional for the
small static BOB mesh, not an imitation of original broad-phase ordering.

Tool limits: 200,000 triangles; finite query coordinates, bounds and radius within
+/-1,000,000 units; nonnegative radius. These are not original game boundaries.

## Reference trace format v1

`reference::Trace` is a strict, versioned JSON observation stream. It requires the
canonical US revision 0 SHA-256, a declared source (`reference`, `native`, or
`authored`), scenario, producer/version/settings, initialization, input sampling
point and state observation boundary. Metadata is a producer's assertion, not
proof of execution or provenance. Authored tests are always labeled `authored`.

A trace stores an initial observation, previous raw input, start tick and one
input/post-state frame per transition. Frame N consumes input N; the initial state
is before the start tick. There are no missing/duplicate ticks and no implicit
frame-to-tick conversion. A trace is **not** a save-state. Initialize hidden model
state and world separately before using `reference::replay`.

`Trace::read` caps encoded input at 32 MiB before parsing. Traces require 1..=100,000
frames; unsupported versions/revisions, unknown or missing fields, blank metadata,
nonfinite state and tick overflow fail explicitly. `Trace::write` validates before
writing; it does not own output paths or provide filesystem atomicity.

`reference::compare` validates both complete streams, checks compatible scenario,
sampling point, observation boundary and start tick, then reports the first input
or state difference (including initial state/input and unequal lengths). Producer,
source, settings and initialization are retained for human review, not required to
be identical across different implementations. Equal observations do not establish
equivalent hidden state or capture correctness.

Floats default to exact f32 **bit** comparison, including signed zero. An explicit
absolute tolerance is available; it must be finite/nonnegative and applies only
to floats. Integers, binary facing angle, action and controller bits compare
exactly. Difference reports include float bits. `reference::replay` runs a
caller-supplied model against recorded inputs. The bounded `movement::FlatWalk`
model requires its explicit flat-lane/idle initialization contract; replay cannot
restore arbitrary original-game state.

### CLI and authored example

```sh
# These commands do not open a ROM, even if SM64_ROM is set.
cargo run -p sm64x -- trace validate docs/examples/authored-trace.json --json
cargo run -p sm64x -- trace compare \
  docs/examples/authored-trace.json docs/examples/authored-trace.json --json
```

[The example](examples/authored-trace.json) consists solely of invented values and
must not be presented as an original-game trace. Trace tools open inputs read-only
and write reports to stdout, never create/truncate an output file. JSON comparison
reports identify the float policy, sources, lengths and first differing tick/field.
A successfully reported equality exits zero; divergence, malformed traces,
incompatible protocols and comparison report-write failures exit nonzero. Unlike
plain inspection commands, comparisons do not suppress broken-pipe failures. Invalid traces produce diagnostics on stderr rather than a comparison
report. Use shell redirection only into ignored local destinations for real data.
`sm64x movement run SCENARIO` separately executes the native flat-walk model
from explicit initialization and authored inputs, without consuming reference
outputs. It emits a native Trace v1 for these comparison tools.

### Capture adapter handoff

The optional [local Mupen64Plus adapter](reference-emulator.md) now produces this
schema for checked US0 action boundaries. Other adapters/cases should use the same
schema rather than force the comparator to infer memory layout or poll timing.
Record all of the following before claiming a reference comparison:

- Emulator/core/plugin versions, relevant settings, tool build/version and ROM
  canonical hash in the manifest. Use `source: reference` only for real captures.
- Reproducible scenario identity and boot/save/warp/act initialization. Real save
  data and runtime traces stay ignored, even when the schema/example is public.
- Measured input and post-Mario-update boundaries, including paused/lag frames;
  do not emit a duplicate tick just because a renderer/controller poll advanced.
- Raw buttons/sticks before processing. Keep previous input from before the first
  recorded transition so button edges are reproducible.
- Independently checked runtime field addresses, widths, signedness and endianness
  for every observed state field. Do not infer correctness from a plausible value
  at one address. Record revision/address evidence in private research logs.
- At least two repeated captures from equivalent initialization whose observations
  compare bit-exactly. Then independently generate a native trace for comparison.

The minimal current schema will need an explicitly versioned extension before
restoring action timers and other hidden state. The first movement slice stops
before animation-dependent stop-to-idle timing and refuses arbitrary restores;
no fields are silently added to Trace v1. Unknown fields are rejected. Its SHA-256 identifies the intended ROM,
not the authenticity of the captured state. Disclose unverified fields and do not
fill gaps with invented constants.

## Evidence and next gate

Authored tests establish infrastructure semantics and mathematical geometry only.
Ignored ROM tests inspect BOB read-only and check probe consistency; they do not
compare against an original-game execution. Collision asset format references
remain in [level-scripts.md](formats/level-scripts.md); geometric intersection
math and the explicit policies above are new implementation choices, not claims
inferred from the format documentation.

For authentic movement research, require a reference runner with:

1. Exact canonical US revision 0 identity (never infer from filename).
2. Reproducible initialization and raw controller injection.
3. An explicitly measured input sampling point and state observation boundary.
   A video frame or controller poll must not be assumed to equal one game tick.
4. A revision-pinned runtime address/field map independently checked against state
   changes. Record observations, uncertainties, runner versions and settings.
5. Short repeatable idle, stick threshold, acceleration, braking, turning,
   jump/landing, slope and wall-contact sequences.

A pinned local US0 action-capture adapter and initial runtime/layout corroboration
are now available; two independent cold boots produced bit-equal movement windows.
See [setup, evidence and limitations](reference-emulator.md). This is not a full
runtime-state map or hardware equivalence proof. The [first native slice](movement-slice.md)
now matches a bounded flat-ground sequence against repeated reference observations;
that result does not validate the remaining actions or original collision solver.
The [floor-query research](research/floor-queries.md) adds paired reference calls
and a bounded native returned-plane arithmetic component. Its plane coefficients
and selected surface come from the caller/oracle; it is not floor search and is
not used to resolve movement. Static/dynamic ordering, eligibility, edges and
geometry-to-plane construction remain unimplemented.
Do not replace missing observations with generic acceleration/gravity constants.
Keep all game-derived captures under ignored `traces/` or `.cache/`; commit only
capture tooling, documentation and authored synthetic tests. See
[ADR 0001](adr/0001-provenance.md).
