"""Assert an observed US0 flat-lane envelope and export an independent Trace v1.

No gameplay arithmetic, native-state generation, or original-hardware proof.
"""
from __future__ import annotations

import argparse
import json
import math
from pathlib import Path

from . import capture, setup, trace_export as export

IDLE, WALKING, BRAKING, STOP = 0x0C400201, 0x04000440, 0x04000445, 0x0C00023D
DECELERATING = 0x0400044A
NEUTRAL = {"buttons": 0, "stick_x": 0, "stick_y": 0}
FORWARD = {**NEUTRAL, "stick_y": 80}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def integer(value, low, high):
    require(type(value) is int and low <= value <= high, "integer outside supported range")
    return value


def fields(value, names):
    require(isinstance(value, dict) and set(value) == set(names.split()), "missing/unknown fields")


def number(value):
    require(type(value) in (int, float) and math.isfinite(value), "invalid finite number")
    return value


def check_environment(state, extra, raw, lane, *, after, yaw):
    x, y, z = state["position"]
    require(lane["min_x"] <= x <= lane["max_x"] and
            lane["min_z"] <= z <= lane["max_z"] and y == lane["height"], "position outside lane")
    require(state["face_yaw"] == yaw, "unsupported observed yaw")
    if yaw == 24576:
        require(all(type(extra[key]) is int for key in ("level_num", "area_index", "act_num")) and
                (extra["level_num"], extra["area_index"], extra["act_num"]) == (9, 1, 1),
                "diagonal reference must remain in BOB area 1 act 1")
    require(state["action"] in (IDLE, WALKING, DECELERATING, BRAKING, STOP), "unsupported observed action")
    floor = extra["floor"]
    require(isinstance(floor, dict) and type(floor["type"]) is int and floor["type"] == 0,
            "not an ordinary floor")
    require(floor["normal"] == [0, 1, 0], "sloped floor")
    require(number(extra["floor_height"]) == lane["height"], "floor height mismatch")
    vertices = floor["vertices"]
    require(isinstance(vertices, list) and len(vertices) == 3, "invalid floor triangle")
    for vertex in vertices:
        require(isinstance(vertex, list) and len(vertex) == 3, "invalid vertex")
        for coordinate in vertex:
            integer(coordinate, -32768, 32767)
        require(vertex[1] == lane["height"], "nonhorizontal vertices")
    a, b, c = vertices
    def cross(a, b, x, z):
        return (b[0] - a[0]) * (z - a[2]) - (b[2] - a[2]) * (x - a[0])
    require(cross(a, b, c[0], c[2]) != 0, "degenerate floor triangle")
    for cx in (lane["min_x"], lane["max_x"]):
        for cz in (lane["min_z"], lane["max_z"]):
            signs = [cross(u, v, cx, cz) for u, v in ((a, b), (b, c), (c, a))]
            require(all(s >= 0 for s in signs) or all(s <= 0 for s in signs), "unsafe lane corner")
    for key, expected in (("wall", 0), ("dialog_id", -1), ("squish_timer", 0),
                          ("quicksand_depth", 0), ("held_object", 0), ("ridden_object", 0)):
        require(number(extra[key]) == expected, f"unsafe {key}")
    require(number(extra["ceiling_height"]) >= lane["height"] + 1000, "insufficient ceiling clearance")
    require(number(extra["water_level"]) < lane["height"] - 160, "unsafe water level")
    expected_stick = [0, 64, 64] if raw == FORWARD else [0, 0, 0]
    require(extra["processed_stick"] == expected_stick, "processed stick mismatch")
    require(number(extra["intended_magnitude"]) in (0, 32), "unsupported intended magnitude")
    if after:
        require(extra["intended_yaw"] == yaw, "unsupported intended yaw")
        require(number(extra["intended_magnitude"]) == (32 if raw == FORWARD else 0),
                "intended magnitude mismatch")


def validate(probe, scenario):
    """Validate report binding, entire observation chain, then selected environment."""
    require(probe["version"] == 1 and type(probe["version"]) is int and
            probe["status"] == "exploratory_boundary_probe_not_validated_game_trace",
            "unsupported probe protocol")
    require(probe["rom_sha256"] == export.US_SHA256 and probe["entry"] == 0x80254830,
            "wrong US ROM or entry")
    require(probe.get("input_policy") is None,
            "feedback-policy probes are exploratory; replay generated inputs in an independent cold boot")
    record_from_poll = integer(probe.get("record_from_poll", 0), 0, 100000)
    observations = probe["observations"]
    require(isinstance(observations, list) and 1 <= len(observations) <= 10000, "invalid observation count")
    counts = probe["counts"]
    fields(counts, "vi entries returns controller_polls")
    for key in counts:
        integer(counts[key], 0, 100000)
    require(counts["entries"] == counts["returns"] == len(observations), "counts mismatch")
    segments, end = [], 0
    require(isinstance(probe["input_schedule"], list) and len(probe["input_schedule"]) <= 10000,
            "invalid schedule")
    for segment in probe["input_schedule"]:
        fields(segment, "end_poll input")
        end = integer(segment["end_poll"], end + 1, 100000)
        export._input(segment["input"])
        segments.append((end, segment["input"]))
    # Reconstruct the plugin's change-only event log, including startup poll zero.
    expected = []
    prior = None
    for poll in range(counts["controller_polls"]):
        raw = capture.input_at(segments, poll)
        if raw != prior:
            expected.append((poll, raw))
            prior = raw
    events = probe["input_events"]
    require(isinstance(events, list) and len(events) == len(expected), "input event count mismatch")
    last_vi = 0
    for event, (poll, raw) in zip(events, expected):
        fields(event, "poll vi input")
        integer(event["poll"], poll, poll)
        require(event["input"] == raw, "input event mismatch")
        last_vi = integer(event["vi"], last_vi, counts["vi"])
    for obs in observations:
        poll = integer(obs["entry_poll"], max(1, record_from_poll), counts["controller_polls"])
        require(obs["input"] == capture.input_at(segments, poll - 1), "schedule/sample mismatch")
        integer(obs["return_vi"], 0, counts["vi"])
        for event in events:
            require(not (event["poll"] < poll and event["vi"] > obs["entry_vi"]), "event timing mismatch")
            require(not (event["poll"] >= poll and event["vi"] < obs["return_vi"]), "event timing mismatch")
    fields(scenario, "version scenario initialization input_sampling state_boundary start_tick initial_state previous_input lane segments")
    integer(scenario["version"], 1, 1)
    require(scenario["input_sampling"] == export.INPUT_SAMPLING and
            scenario["state_boundary"] == export.STATE_BOUNDARY, "scenario protocol mismatch")
    integer(scenario["start_tick"], 0, 0xFFFFFFFF)
    lane = scenario["lane"]
    fields(lane, "min_x max_x min_z max_z height")
    require(all(abs(number(v)) <= 1000000 for v in lane.values()) and
            lane["min_x"] < lane["max_x"] and lane["min_z"] < lane["max_z"], "invalid lane")
    require(isinstance(scenario["segments"], list) and 1 <= len(scenario["segments"]) <= 43, "invalid segments")
    inputs = []
    for segment in scenario["segments"]:
        fields(segment, "ticks input")
        ticks = integer(segment["ticks"], 1, 43)
        export._input(segment["input"])
        require(segment["input"] in (NEUTRAL, FORWARD), "unsupported raw input")
        inputs.extend([segment["input"]] * ticks)
    require(len(inputs) <= 43, "too many ticks")
    require(1 <= inputs.count(FORWARD) <= 30, "forward hold must be 1..30 ticks")
    short = inputs.count(FORWARD) < 10
    release_action, stop_action = (DECELERATING, IDLE) if short else (BRAKING, STOP)
    export._input(scenario["previous_input"])
    require(scenario["previous_input"] == NEUTRAL, "previous input must be neutral")
    starts = [i for i, obs in enumerate(observations) if obs["extra_before"]["global_timer"] == scenario["start_tick"]]
    require(len(starts) == 1, "start_tick not uniquely observed")
    start = starts[0]
    previous = capture.input_at(segments, observations[0]["entry_poll"] - 2)
    trace = export.build_trace(observations, probe["emulator"], scenario["scenario"],
                               scenario["initialization"], previous_input=previous, start=start, count=len(inputs))
    require(export._state(scenario["initial_state"]) == export._state(trace["initial_state"]), "initial state bits mismatch")
    require(trace["previous_input"] == scenario["previous_input"], "actual predecessor mismatch")
    initial = trace["initial_state"]
    yaw = initial["face_yaw"]
    require(initial["action"] == IDLE and yaw in (-32768, 24576) and
            initial["velocity"] == [0, 0, 0] and initial["forward_velocity"] == 0, "initial state not idle/rest")
    require(yaw != 24576 or inputs.count(FORWARD) <= 5, "diagonal hold must be 1..5 ticks")
    # Check declared phase/action envelope only; do not replay speeds or positions.
    phase, idle, walking = "idle", 0, 0
    for index, (obs, raw) in enumerate(zip(observations[start:start + len(inputs)], inputs)):
        require(obs["input"] == raw, "scenario input mismatch")
        before_action = {"idle": IDLE, "walk": WALKING, "release": release_action}.get(phase)
        require(obs["before"]["action"] == before_action, "observed action phase mismatch")
        if raw == FORWARD:
            require(phase in ("idle", "walk"), "resumed walking")
            walking += 1
            phase = "walk"
            allowed = (WALKING,)
        elif phase == "idle":
            idle += 1
            allowed = (IDLE,)
        else:
            phase = "release"
            allowed = (stop_action,) if index == len(inputs) - 1 else (release_action,)
        require(idle <= 5 and walking <= 30 and obs["after"]["action"] in allowed,
                "unsupported phase/action budget")
        for boundary in ("before", "after"):
            check_environment(obs[boundary], obs["extra_" + boundary], raw, lane,
                              after=boundary == "after", yaw=yaw)
    final = trace["frames"][-1]["state"]
    require(phase == "release" and walking >= 1 and final["action"] == stop_action and
            final["forward_velocity"] == 0, "incomplete release sequence")
    settings = json.loads(trace["manifest"]["settings"])
    settings["movement_envelope"] = {
        "lane": lane, "face_yaw": yaw, "record_from_poll": record_from_poll, "assertions": "Every selected before/after: ordinary horizontal triangle contains whole rectangle; no walls, interactions, squish, quicksand; ceiling clearance >=1000; water < height-160. Conservative safety bounds, not game rules.",
        "scope": "Independent observed reference only; no native arithmetic replay/fitting, no original-hardware proof. Initial state matched bitwise once to selected BEFORE; actual preceding input verified."}
    trace["manifest"]["settings"] = json.dumps(settings, sort_keys=True, allow_nan=False)
    return trace


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--probe", required=True, type=Path)
    parser.add_argument("--scenario", required=True, type=Path)
    parser.add_argument("--out", required=True, type=Path, help="NEW ignored .cache directory")
    args = parser.parse_args()
    try:
        out = setup.checked_path(args.out)
        require(not out.exists(), "output must be new")
        trace = validate(capture.json_read(args.probe, limit=64 * 1024 * 1024), capture.json_read(args.scenario))
        out.mkdir(parents=True)
        capture.write_new(out / "trace.json", trace)
        print(out / "trace.json")
    except (OSError, ValueError, KeyError, TypeError, IndexError, OverflowError) as exc:
        parser.exit(1, f"movement_check: {exc}\n")


if __name__ == "__main__":
    main()
