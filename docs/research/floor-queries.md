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
