# US0 floor-query observation boundary

## Scope

This is read-only reference instrumentation, **not an original collision solver**.
`collision::World` remains a diagnostic geometric probe. Native movement still
uses an explicitly corroborated flat-lane assertion; it does not call the game's
floor function or read its per-tick outputs.

The existing action boundary is insufficient to reconstruct floor selection:
there may be many floor queries per Mario update, and other callers run outside
that update. The optional floor observer therefore records a separate exploratory
`floor-queries.json`, never Trace v1. All output remains ignored under `.cache/`.

## Metadata and ABI hypothesis

Canonical ROM: US revision 0, SHA-256
`17ce077343c6133f8c9f2d6d6d9a4ab62c8cd2aa57c40aea1f490b4c8bb21d91`.
No revision-specific address is used before the capture runner's exact hash gate.

Metadata references, not game implementation bodies:

- [Pinned STROOP US map](https://github.com/SM64-TAS-ABC/STROOP/blob/6771197ad0f50718d3d4a13387e01d1e6d9c0d1a/STROOP/Mappings/MappingUS.map):
  `find_floor` at `0x80381900`.
- [Pinned declarations](https://github.com/n64decomp/sm64/blob/9921382a68bb0c865e5e45eb594d9c64db59b1af/src/engine/surface_collision.h):
  three f32 coordinates followed by a Surface-pointer output parameter; f32 result.
- [Pinned Surface layout](https://github.com/n64decomp/sm64/blob/9921382a68bb0c865e5e45eb594d9c64db59b1af/include/types.h):
  type at `0x00`, ordered signed-i16 vertices at `0x0a`, f32 normal at `0x1c`,
  and f32 plane origin offset at `0x28`.
- The runner's pinned Mupen64Plus public debugger ABI (`m64p_types.h`,
  `m64p_debugger.h`, and `cp1.h`) exposes single-register storage through
  `DebugGetCPUDataPtr(7)` as a `float **`. The binding reads pointed-to float bits;
  it neither assumes a flat host FPR array nor binds register/memory writes.

The O32 calling-convention hypothesis is FPR12/FPR14 for X/Y, low 32 bits of GPR6
for Z, GPR7 for the output address, GPR31 for the caller return, and FPR0 for the
height result. Declarations alone do not prove that this is the right runtime
boundary or ABI; use the observations and limitations below.

## Capture contract

First produce the private BOB baseline/input schedule as described in
[the movement slice](../movement-slice.md#bob-differential-baseline). Then replay
one selected case into a **new** directory with `--observe-floor`, the US0 action
profile, and a positive `--record-from-poll` after all loading/warps. For example:

```sh
python3 -m scripts.reference_emulator.capture \
  --out .cache/emulator/bob-floor-a --profile us0-action \
  --schedule .cache/emulator/my-bob-baseline/hold-5/inputs.json \
  --record-from-poll 3879 --updates 23 --max-vis 8800 --timeout 240 \
  --observe-floor
# Repeat into bob-floor-b; inspect both action and floor-query reports privately.
```

Each paired floor call records input bits/decoded coordinates, runtime caller,
output address, returned height bits, returned surface pointer and its ordered
vertices/type/normal/plane bits. Before/after contexts contain timer, scene,
controller-poll and VI counters, and whether an action call is pending. Floor
calls are not treated as game ticks or automatically attributed to Mario merely
because their timer matches. No artificial queries are injected.

Pointers, finite floats, register indices, nesting and call budgets fail explicitly.
`--floor-max-calls` defaults to 100,000; exceeding it fails rather than sampling.
An unpaired call cannot produce a complete floor report. Standard synthetic tests
exercise ABI storage and observer lifecycle without a ROM, emulator or GPU.

## Local runtime corroboration

Two independent open-loop cold boots replayed the five-tick BOB hold. Both
recorded **23 action transitions** and **1,171 paired floor calls**; their complete
floor reports matched exactly, including coordinate/height bits, caller/output
addresses, returned surfaces and contexts. The instrumented action window also
still matched the independently generated native movement trace bit-for-bit.

Only **101** floor calls occurred while an action call was pending; **1,070**
occurred outside it. This is why a floor-call counter cannot be called a Mario tick
counter. All observed before/after contexts agreed within each call.

For one caller-return site (`0x80253acc`), all 23 coordinate triples matched the
corresponding pre-action Mario position bits, the output parameter addressed
MarioState's documented floor-pointer slot, and the returned surface agreed with
the independent action diagnostics. Another return site (`0x80255b58`) produced
68 queries at the separately rounded horizontal quarter-step positions. These
observations corroborate the coordinate/output ABI and floor boundary within this
window; they do not identify every caller's purpose or prove complete behavior.

The natural call corpus also contains 20 distinct observed plane-coefficient
sets and 723 queries with fractional X/Z coordinates, including negative values.
For the **already selected, returned plane**, every observed height is reproduced
by truncating X/Z toward zero, rounding each product/sum/division to f32, and
computing `-((nx*x + nz*z) + origin_offset) / ny` in that order. Using unquantized
coordinates or reassociating the additions produces disagreements. This is a
bounded arithmetic inference using oracle-selected plane coefficients, **not**
independent floor selection or a collision-equivalence result. Conversion width,
overflow behavior, eligibility and the other rules below remain unresolved.

## Native returned-plane arithmetic component

`sm64_sim::floor::FloorPlane` implements that arithmetic independently in Rust.
It takes an **already selected** plane's coefficients; it does not normalize them,
construct original planes from asset triangles, search cells/lists, or decide
whether the query may stand on that plane. It is not wired into `FlatWalk`.

The API rejects nonfinite coefficients, nonpositive Y coefficients, nonfinite
results and queries outside an authored **±8192** support envelope. That bound and
positive-Y requirement are conservative API constraints, not inferred original
world limits, overflow behavior or classification thresholds. Within that domain,
integer conversion is only a way to express observed truncation toward zero; its
use of Rust i32 does not establish the original conversion width.

An ignored local evidence test reproduced **all 1,171 returned heights exactly**
for each independent report, including signed zero and sloped-plane rounding:

```sh
export SM64_FLOOR_QUERIES="$PWD/.cache/emulator/bob-floor-a/floor-queries.json"
cargo test -p sm64-sim --test floor_plane -- --ignored --nocapture
# SM64_ROM must also be set; repeat with bob-floor-b.
```

The test reads private oracle-selected plane coefficients and input/result bits;
that is deliberately only a **plane-arithmetic comparison**, not independent
collision resolution or capture authentication. Normal tests use invented planes
and coordinates and require no ROM. They cover truncation, signed zero,
non-reassociation, invalid input and explicit overflow/unsupported errors.

## Optional floor-list memory snapshots

`--observe-floor-lists` additionally requires `--observe-floor`. At **every**
recorded natural floor entry and return it rereads all category-zero embedded
heads in both partitions, follows their `next` chains and records declared surface
metadata. Each call references its before/after snapshot IDs in a separate private
`floor-lists.json`. Identical snapshot storage is deduplicated by SHA-256; memory
reads are never cached between boundaries. Trace v1 is unchanged.

Metadata provenance (declarations only, not implementation bodies):

- The pinned STROOP map above labels `gStaticSurfacePartition` at `0x8038be98`
  and `gDynamicSurfacePartition` at `0x8038d698`.
- Pinned `surface_load.h` (linked below) declares `SurfaceNode` as next/surface
  pointers, `SpatialPartitionCell` as three embedded nodes, and floors as category
  zero. With N64 32-bit pointers this gives 8-byte nodes, 24-byte cells and two
  16×16 partitions. The map's address gap independently agrees with `0x1800` bytes.
- [Pinned surface-load declarations](https://github.com/n64decomp/sm64/blob/9921382a68bb0c865e5e45eb594d9c64db59b1af/src/engine/surface_load.h)
  declare partition dimensions and the 8192/1024 boundary/cell constants. These
  declarations do **not** prove coordinate conversion or index-axis orientation.

Both embedded-head words are retained without interpreting the head's surface
word as a live pointer. Following `next` is a provisional memory-layout walk,
**not observed resolver execution order**, eligibility, precedence or tie rules.
Heads are indexed by declaration outer/inner indices, not labelled X/Z.
Surface records also retain force, raw flags/room/lower-Y bits, upper Y and object
pointer bits; these are observations, not implemented surface behavior.

Pointers are aligned and bounded to 4 MiB RDRAM; null node surfaces and cycles
fail. Tooling limits are 10,000 distinct nodes and 100,000 followed links per
snapshot, 128 million memory reads per run, 1,024 distinct stored snapshots and
32 MiB compact snapshot data.
Exhaustion aborts the capture, never samples or silently truncates it. Normal
synthetic tests cover shared tails, pointer rejection, cycles, fresh reads,
storage deduplication, before/after binding and every budget without a ROM.

### Repeatability and coverage checker

`python3 -m scripts.reference_emulator.floor_check --run RUN_A --repeat RUN_B
--control CONTROL --out .cache/floor-inventory.json` validates two distinct,
complete open-loop BOB captures and optionally a third capture made without list
snapshots. Use identical schedules, recording gates and update counts. Output
must be a new ignored path. Reports are size-bounded and strict JSON; failures do
not write a success report.

The checker binds floor/list reports to their action report, validates the full
consecutive action chain and scheduled inputs, requires BOB scene 9/1/1, checks
unchanged floor-call contexts, decoded float/bit agreement (including signed zero)
and inside-action ordinal/timer/poll binding. It verifies snapshot digests,
complete reachable next chains and returned-surface metadata/membership. All
floor, action and list reports must repeat exactly. A control comparison removes
only list-sidecar references; all other floor/action observations must agree.
This is consistency checking, **not capture authentication**.

The private inventory counts slopes, fractional/negative queries, absent floors,
returned surfaces/planes and successive same-caller surface/height-bit changes.
Changes sharing two ordered-triangle vertices are only candidate seam witnesses:
a repeated caller may serve different objects, and a pair does not establish
continuous movement or the solver's rejected candidates. List membership and
unchanged before/after snapshots do not establish selection or traversal order.

## Repeated natural BOB ramp window

A broader **51-action** open-loop window, recording from poll count **3768**,
replays the baseline's existing approach across flat ground, a ramp and raised
flat ground. Two independent empty-save cold boots recorded **2,893 paired floor
calls** each; their entire floor, action and list reports repeated exactly. A
third cold boot without list snapshots matched every other floor/action field.
This is a collision research corpus, not an expansion of native movement support.

Reproduce into a new directory, using the generated baseline schedule:

```sh
python3 -m scripts.reference_emulator.capture \
  --out .cache/emulator/bob-ramp-a --profile us0-action \
  --schedule .cache/emulator/my-bob-baseline/hold-5/inputs.json \
  --record-from-poll 3768 --updates 51 --max-vis 8800 --timeout 240 \
  --observe-floor --observe-floor-lists
# Repeat into bob-ramp-b. For bob-ramp-control omit only --observe-floor-lists.
python3 -m scripts.reference_emulator.floor_check \
  --run .cache/emulator/bob-ramp-a --repeat .cache/emulator/bob-ramp-b \
  --control .cache/emulator/bob-ramp-control \
  --out .cache/emulator/bob-ramp-inventory.json
```

Inventory: **31 returned surfaces**, **23 returned plane sets**, **928 sloped
returns** and **1,723 fractional-X/Z calls**, all of those including a negative
fractional coordinate. Only **289** queries were inside an action; **2,604** were
outside. At previously corroborated return site `0x80253acc`, all **51** inputs
matched the pre-action position bits and addressed MarioState's floor-pointer
slot; all returned pointers matched its independent before-action diagnostic.
That site observed **12** sloped returns and **three** successive surface changes
whose triangles shared two vertices. No arbitrary query injection was used.

Every returned surface appeared in static next chains with identical recorded
metadata at both boundaries. All **5,786** boundary snapshots deduplicated to one
unchanged memory image: **3,773** reachable nodes and **621** distinct floor
surfaces. This corroborates declared list layout and membership within the window,
not executed traversal, cell-axis mapping, eligibility or static/dynamic precedence.
No returned dynamic surface or absent-floor case was observed; those are explicit
coverage gaps. Query counts are not Mario ticks.

The final capture used **114,261,928** bounded guest reads. Earlier 61-action
attempts exhausted first the initial 32-million and then the 128-million read
budget and were preserved only as ignored failure diagnostics. The successful
scope was explicitly narrowed to 51 actions; failed runs are not evidence.

## Remaining collision questions

The observer does not establish coordinate quantization, floor classification,
height acceptance/sentinel rules, edge inclusion, spatial partition membership,
linked-list insertion/traversal/tie ordering, special-surface filtering, or
static/dynamic precedence. A returned surface alone does not reveal all rejected
candidates. Ordinary flat agreement cannot establish slope arithmetic.

Before implementing original floor resolution, collect repeated natural seam,
slope and height-transition queries and independently corroborate candidate-list
layouts/order. Keep diagnostic probe policies separate. No game/decompiler
implementation bodies, disassembly, extracted geometry or traces belong in Git.
