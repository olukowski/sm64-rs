"""Optional local-ROM BOB baseline research, not game code or a general controller.

Run all five authored short holds twice from independent cold boots. Only the
unique START_POLL BEFORE observation initializes native movement, once; reference
poststates are used solely by the independent checker/comparator. All artifacts
(including failures) must stay in a new ignored .cache directory. No downloads,
game-memory writes, or save-state restores are performed.
"""
from __future__ import annotations

import argparse
from copy import deepcopy
import os
from pathlib import Path
import subprocess
import sys

from . import bob_route, capture, movement_check, setup, trace_export

RELEASES = (9, 10, 11, 12, 13)  # Preinvestigated first-stop durations, not fitted here.


def _schedule(document):
    """Pure counterpart of capture.schedule_read's bounded JSON contract."""
    require = movement_check.require
    require(isinstance(document, dict) and set(document) == {"version", "segments"}
            and type(document["version"]) is int and document["version"] == 1,
            "unsupported input schedule")
    segments = document["segments"]
    require(isinstance(segments, list) and len(segments) <= 10000,
            "input schedule must be a bounded segment array")
    total = 0
    for segment in segments:
        movement_check.fields(segment, "polls buttons stick_x stick_y")
        total += movement_check.integer(segment["polls"], 1, 100000)
        trace_export._input({key: segment[key] for key in bob_route.NEUTRAL})
        require(total <= 100000, "schedule exceeds 100000 controller polls")
    return total


def prepare_case(probe, generated_schedule, hold):
    """Build detached authored inputs/scenario without reading any poststate.

    Lane offsets are a caller assertion/safety box, NOT collision physics. The
    independent movement checker must subsequently verify every corner against
    every observed floor triangle.
    """
    require = movement_check.require
    require(type(hold) is int and 1 <= hold <= 5, "hold must be an integer in 1..5")
    observations = probe["observations"]
    require(isinstance(observations, list), "observations must be an array")
    starts = [obs for obs in observations
              if type(obs.get("entry_poll")) is int and obs["entry_poll"] == bob_route.START_POLL]
    require(len(starts) == 1, "expected exactly one START_POLL observation")
    obs = starts[0]
    state, extra = obs["before"], obs["extra_before"]
    trace_export._state(state)
    tick = movement_check.integer(extra["global_timer"], 0, 0xFFFFFFFF)
    require(state["action"] == movement_check.IDLE and state["face_yaw"] == 24576
            and state["velocity"] == [0, 0, 0] and state["forward_velocity"] == 0,
            "initial state must be idle/rest at yaw 24576")
    require(all(type(extra[key]) is int for key in ("level_num", "area_index", "act_num"))
            and (extra["level_num"], extra["area_index"], extra["act_num"]) == (9, 1, 1),
            "initial scene must be BOB 9/1/1")
    floor = extra["floor"]
    require(isinstance(floor, dict) and type(floor["type"]) is int and floor["type"] == 0
            and floor["normal"] == [0, 1, 0], "initial floor must be ordinary and horizontal")
    x, y, z = state["position"]
    scenario = {
        "version": 1, "scenario": f"bob-flat-short-hold-{hold}-v1",
        "initialization": (
            f"One-time initialization from unique BEFORE observation at entry_poll={bob_route.START_POLL}; "
            "not native boot or save-state restore. No reference poststates consumed for native generation."),
        "input_sampling": trace_export.INPUT_SAMPLING,
        "state_boundary": trace_export.STATE_BOUNDARY,
        "start_tick": tick, "initial_state": deepcopy(state),
        "previous_input": dict(bob_route.NEUTRAL),
        "lane": {"min_x": x - 2, "max_x": x + 98, "min_z": z - 98,
                 "max_z": z + 4, "height": y},
        "segments": [{"ticks": ticks, "input": dict(raw)} for ticks, raw in (
            (5, bob_route.NEUTRAL), (hold, bob_route.FORWARD),
            (RELEASES[hold - 1], bob_route.NEUTRAL))],
    }
    require(_schedule(generated_schedule) >= bob_route.WALK_POLL,
            "generated schedule does not cover WALK_POLL prefix")
    prefix, remaining = [], bob_route.WALK_POLL
    for segment in generated_schedule["segments"]:
        if not remaining:
            break
        copied = deepcopy(segment)
        copied["polls"] = min(remaining, copied["polls"])
        prefix.append(copied)
        remaining -= copied["polls"]
    final = {"version": 1, "segments": prefix + [
        dict(polls=hold, **bob_route.FORWARD), dict(polls=40, **bob_route.NEUTRAL)]}
    _schedule(final)
    return scenario, final


def _run(command, output, timeout, *, native=False):
    """Keep bounded subprocess diagnostics; never truncate an existing artifact."""
    environment = dict(os.environ)
    if native:
        environment.pop("SM64_ROM", None)
    with output.open("xb") as stdout, output.with_suffix(".stderr.log").open("xb") as stderr:
        subprocess.run([str(part) for part in command], cwd=setup.PROJECT,
                       timeout=timeout, check=True, stdout=stdout, stderr=stderr,
                       env=environment)


def run(out, rom, manifest, timeout):
    """Execute a complete sweep; publish summary only on total success."""
    out = setup.checked_path(out)
    movement_check.require(not out.exists(), "output must be a NEW directory")
    movement_check.integer(timeout, 1, 600)
    rom = Path(rom).resolve(strict=True)
    manifest = setup.checked_path(manifest)
    capture.rom_bytes(rom)
    capture.checked_manifest(manifest)
    boot_schedule = setup.PROJECT / "docs/examples/castle-walk-jump-inputs.json"
    capture.schedule_read(boot_schedule)
    out.mkdir(parents=True)
    base = [sys.executable, "-m", "scripts.reference_emulator.capture", "--rom", rom,
            "--manifest", manifest, "--profile", "us0-action", "--timeout", str(timeout)]
    bootstrap = out / "bootstrap"
    _run(base + ["--out", bootstrap, "--input-policy", bob_route.NAME,
                 "--policy-hold", "5", "--updates", "3500", "--max-vis", "10000",
                 "--schedule", boot_schedule], out / "bootstrap.log", timeout + 10)
    probe = capture.json_read(bootstrap / "probe.json", limit=64 * 1024 * 1024)
    generated = capture.json_read(bootstrap / "inputs.json")
    cases = []
    cargo = ["cargo", "run", "-q", "-p", "sm64x", "--"]
    for hold in range(1, 6):
        case = out / f"hold-{hold}"
        case.mkdir()
        scenario, schedule = prepare_case(probe, generated, hold)
        frames = sum(segment["ticks"] for segment in scenario["segments"])
        scenario_path, schedule_path = case / "scenario.json", case / "inputs.json"
        capture.write_new(scenario_path, scenario)
        capture.write_new(schedule_path, schedule)
        native = case / "native.json"
        _run(cargo + ["movement", "run", scenario_path], native, timeout, native=True)
        references = []
        for repeat in ("a", "b"):
            directory = case / repeat
            _run(base + ["--out", directory, "--schedule", schedule_path,
                         "--record-from-poll", str(bob_route.START_POLL),
                         "--updates", str(frames), "--max-vis", "8800"],
                 case / f"{repeat}.log", timeout + 10)
            replay = capture.json_read(directory / "probe.json", limit=64 * 1024 * 1024)
            movement_check.require(len(replay["observations"]) == frames,
                                   "replay must stop at exact requested boundary")
            trace = movement_check.validate(replay, scenario)
            reference = directory / "reference.json"
            capture.write_new(reference, trace)
            references.append(reference)
        comparisons = {}
        for label, left, right in (("a-native", references[0], native),
                                   ("b-native", references[1], native),
                                   ("a-b", *references)):
            result = case / f"{label}.json"
            _run(cargo + ["trace", "compare", left, right, "--json"], result,
                 timeout, native=True)
            report = capture.json_read(result)
            movement_check.require(report["float_policy"] == "exact_f32_bits" and
                                   report["status"] == "equal_observations",
                                   f"{label} did not compare exactly")
            comparisons[label] = report["status"]
        cases.append({"hold": hold, "frames": frames, "status": "equal_observations",
                      "comparisons": comparisons})
    per_sweep = sum(case["frames"] for case in cases)
    summary = {"version": 1, "status": "equal_observations", "cases": cases,
               "transitions_per_sweep": per_sweep,
               "reference_transitions": per_sweep * 2,
               "limitations": "Optional pinned local US0 emulator research only; authored safety box, "
               "not collision physics, a general controller, native boot, or original-hardware proof. "
               "One-time BEFORE initialization only; no reference poststates generate native movement."}
    capture.write_new(out / "summary.json", summary)
    return summary


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, required=True, help="NEW ignored .cache directory")
    parser.add_argument("--rom", type=Path, default=os.environ.get("SM64_ROM"))
    parser.add_argument("--manifest", type=Path, default=setup.CACHE / "emulator/manifest.json")
    parser.add_argument("--timeout", type=int, default=240, help="per-process seconds, 1..600")
    args = parser.parse_args()
    try:
        movement_check.require(args.rom is not None, "select --rom or SM64_ROM; no downloads")
        run(args.out, args.rom, args.manifest, args.timeout)
        print(setup.checked_path(args.out) / "summary.json")
    except (OSError, ValueError, RuntimeError, KeyError, TypeError, IndexError,
            OverflowError, subprocess.SubprocessError) as exc:
        parser.exit(1, f"bob_baseline: {exc}\n")


if __name__ == "__main__":
    main()
