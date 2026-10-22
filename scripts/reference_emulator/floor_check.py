"""Inventory repeated natural BOB floor observations, never infer a solver.

Reports remain private. Exact repeatability and memory-list membership are
consistency evidence, not capture authentication or observed traversal order.
"""
from __future__ import annotations

import argparse
from collections import Counter
import hashlib
import json
from pathlib import Path
import struct

from . import capture, floor_lists, floor_probe, trace_export

MAX_REPORT = 256 * 1024 * 1024


def require(condition, message):
    if not condition:
        raise ValueError(message)


def integer(value, low, high, label):
    require(type(value) is int and low <= value <= high, f"invalid {label}")
    return value


def exact(value):
    # JSON preserves signed zero, unlike Python's numeric/list equality.
    return json.dumps(value, sort_keys=True, separators=(",", ":"), allow_nan=False).encode()


def float_bits(bits):
    integer(bits, 0, 0xFFFFFFFF, "float bits")
    return floor_probe._float(bits)


def decoded(bits, value):
    require(type(value) in (int, float), "decoded float must be numeric")
    require(trace_export._float_bits(value) == struct.pack(">I", bits), "decoded float differs from bits")


def surface(value):
    require(isinstance(value, dict), "surface must be an object")
    floor_probe._ram(value["address"], 0x30, "surface pointer")
    integer(value["type"], -32768, 32767, "surface type")
    vertices = value["vertices"]
    require(isinstance(vertices, list) and len(vertices) == 3, "surface needs three vertices")
    for vertex in vertices:
        require(isinstance(vertex, list) and len(vertex) == 3, "vertex needs three coordinates")
        for coordinate in vertex:
            integer(coordinate, -32768, 32767, "vertex coordinate")
    normal = value["normal_bits"]
    require(isinstance(normal, list) and len(normal) == 3, "normal needs three components")
    for bits in normal + [value["origin_offset_bits"]]:
        float_bits(bits)


def header(report, status):
    require(type(report["version"]) is int and report["version"] == 1 and report["status"] == status,
            "unsupported report")
    require(report["rom_sha256"] == capture.US_SHA256, "not canonical US0")
    require(report.get("memory_write") is False, "read-only report required")
    integer(report["record_from_poll"], 1, 100000, "recording gate")


def list_inventory(report):
    header(report, "exploratory_floor_list_memory")
    layout = report["layout"]
    require(layout["partitions"] == dict(floor_lists.PARTITIONS) and
            (layout["cells"], layout["cell_bytes"], layout["category"], layout["node_bytes"]) ==
            (16, 24, 0, 8), "unsupported partition layout")
    snapshots = report["snapshots"]
    require(isinstance(snapshots, list) and 1 <= len(snapshots) <= floor_lists.MAX_SNAPSHOTS,
            "invalid snapshot count")
    memberships = []
    for snapshot in snapshots:
        data = {key: snapshot[key] for key in ("heads", "nodes", "surfaces")}
        require(hashlib.sha256(exact(data)).hexdigest() == snapshot["sha256"],
                "snapshot digest differs")
        nodes, surfaces, heads = data["nodes"], data["surfaces"], data["heads"]
        require(isinstance(nodes, dict) and len(nodes) <= floor_lists.MAX_NODES,
                "invalid node count")
        require(isinstance(surfaces, dict) and set(heads) == {"static", "dynamic"},
                "incomplete partition snapshot")
        for address, node in nodes.items():
            floor_probe._ram(int(address), 8, "node pointer")
            require(str(int(address)) == address and isinstance(node, list) and len(node) == 2,
                    "invalid node record")
            if node[0]:
                floor_probe._ram(node[0], 8, "next pointer")
            floor_probe._ram(node[1], 0x30, "node surface pointer")
        for address, value in surfaces.items():
            surface(value)
            require(str(value["address"]) == address, "surface address key differs")
        membership, reached, used_surfaces = {}, set(), set()
        links = 0
        for partition, partition_heads in heads.items():
            require(isinstance(partition_heads, list) and len(partition_heads) == 256,
                    "incomplete partition heads")
            for cell, head in enumerate(partition_heads):
                require(isinstance(head, list) and len(head) == 2, "invalid embedded head")
                for word in head:
                    integer(word, 0, 0xFFFFFFFF, "head word")
                pointer, seen, ordinal = head[0], set(), 0
                while pointer:
                    require(links < floor_lists.MAX_LINKS, "snapshot link budget exceeded")
                    links += 1
                    require(pointer not in seen, "cycle in snapshot")
                    seen.add(pointer)
                    reached.add(str(pointer))
                    node = nodes[str(pointer)]
                    address = str(node[1])
                    require(address in surfaces, "missing node surface")
                    used_surfaces.add(address)
                    membership.setdefault(address, []).append([partition, cell, ordinal])
                    pointer, ordinal = node[0], ordinal + 1
        require(reached == set(nodes) and used_surfaces == set(surfaces), "orphan snapshot records")
        memberships.append(membership)
    return memberships


def inventory(floors, probe, lists=None):
    header(floors, "exploratory_floor_calls")
    require(floors["entry"] == floor_probe.ENTRY, "incorrect floor entry")
    require(probe["version"] == 1 and probe["entry"] == 0x80254830 and
            probe["rom_sha256"] == capture.US_SHA256 and probe["input_policy"] is None,
            "requires open-loop US0 action capture")
    require(probe["record_from_poll"] == floors["record_from_poll"] and
            exact(probe["emulator"]) == exact(floors["emulator"]), "unbound action report")
    observations = probe["observations"]
    require(isinstance(observations, list) and observations, "empty action observations")
    require(probe["counts"]["entries"] == probe["counts"]["returns"] == len(observations),
            "incomplete action capture")
    segments, end = [], 0
    for segment in probe["input_schedule"]:
        integer(segment["end_poll"], end + 1, 100000, "schedule end")
        trace_export._input(segment["input"])
        end = segment["end_poll"]
        segments.append((end, segment["input"]))
    for obs in observations:
        require(obs["input"] == capture.input_at(segments, obs["entry_poll"] - 1),
                "action input differs from schedule")
        for key in ("extra_before", "extra_after"):
            require(tuple(obs[key][field] for field in ("level_num", "area_index", "act_num")) ==
                    (9, 1, 1), "action scene is not BOB 9/1/1")
    trace_export.build_trace(observations, probe["emulator"], "bob-floor-inventory-v1",
                             "Independent open-loop observation consistency check, not native initialization.",
                             previous_input=capture.input_at(segments, observations[0]["entry_poll"] - 2))
    memberships = None
    if lists is not None:
        require(floors.get("list_report") == "floor-lists.json", "missing list binding")
        require(lists["record_from_poll"] == floors["record_from_poll"] and
                exact(lists["emulator"]) == exact(floors["emulator"]), "unbound list report")
        memberships = list_inventory(lists)
    else:
        require("list_report" not in floors, "list sidecar required")
    calls = floors["calls"]
    limit = integer(floors["max_calls"], 1, 100000, "max_calls")
    require(isinstance(calls, list) and 1 <= len(calls) <= limit, "invalid floor call count")
    require(probe["floor_probe"] == {"file": "floor-queries.json", "calls": len(calls)},
            "action floor-call count differs")
    counts, callers, unique_surfaces, planes = Counter(), {}, set(), set()
    previous, prior_context = {}, None
    for call in calls:
        before, after = call["context_before"], call["context_after"]
        caller = callers.setdefault(f"0x{call['return_pc']:08x}", Counter())
        caller["calls"] += 1
        require(exact(before) == exact(after), "context changed during floor call")
        for key in ("global_timer", "controller_polls", "vi", "action_entries"):
            integer(before[key], 0, 0xFFFFFFFFFFFFFFFF, key)
            require(prior_context is None or before[key] >= prior_context[key], "nonmonotonic floor context")
        require(type(before["inside_action"]) is bool, "invalid inside_action")
        require(tuple(before[field] for field in ("level_num", "area_index", "act_num")) == (9, 1, 1),
                "floor scene is not BOB 9/1/1")
        require(before["controller_polls"] >= floors["record_from_poll"] and
                before["action_entries"] <= len(observations), "floor context outside recording")
        prior_context = before
        if before["inside_action"]:
            index = integer(before["action_entries"], 1, len(observations), "action ordinal") - 1
            obs = observations[index]
            require(before["global_timer"] == obs["extra_before"]["global_timer"] and
                    before["controller_polls"] == obs["entry_poll"] and
                    obs["entry_vi"] <= before["vi"] <= obs["return_vi"], "unbound inside-action floor call")
            counts["inside_action"] += 1
            caller["inside_action"] += 1
        floor_probe._ram(call["return_pc"], 4, "floor return PC")
        floor_probe._ram(call["output_address"], 4, "floor output pointer")
        bits, values = call["input_bits"], call["input_values"]
        require(isinstance(bits, list) and isinstance(values, list) and len(bits) == len(values) == 3,
                "invalid floor arguments")
        for raw, value in zip(bits, values):
            float_bits(raw)
            decoded(raw, value)
        float_bits(call["height_bits"])
        decoded(call["height_bits"], call["height"])
        counts["fractional_xz"] += any(value != int(value) for value in (values[0], values[2]))
        counts["negative_fractional_xz"] += any(value < 0 and value != int(value) for value in (values[0], values[2]))
        returned = call["surface"]
        if returned is None:
            counts["no_floor"] += 1
        else:
            surface(returned)
            unique_surfaces.add(returned["address"])
            planes.add(tuple(returned["normal_bits"]) + (returned["origin_offset_bits"],))
            counts["sloped_return"] += any(float_bits(returned["normal_bits"][axis]) != 0 for axis in (0, 2))
            caller["sloped_return"] += any(float_bits(returned["normal_bits"][axis]) != 0 for axis in (0, 2))
        # These are successive observations per caller/context kind, NOT a claim
        # that the solver crossed a seam or evaluated only these two surfaces.
        key = (call["return_pc"], before["inside_action"])
        old = previous.get(key)
        if old is not None:
            counts["same_caller_height_changes"] += old["height_bits"] != call["height_bits"]
            left = old["surface"]
            if left is not None and returned is not None and left["address"] != returned["address"]:
                counts["same_caller_surface_changes"] += 1
                caller["surface_changes"] += 1
                shared = set(map(tuple, left["vertices"])) & set(map(tuple, returned["vertices"]))
                counts["changes_sharing_two_vertices"] += len(shared) == 2
                caller["changes_sharing_two_vertices"] += len(shared) == 2
        previous[key] = call
        if memberships is not None:
            ids = [integer(call[field], 0, len(memberships) - 1, field)
                   for field in ("list_snapshot_before", "list_snapshot_after")]
            counts["changed_list_snapshot_during_call"] += (
                lists["snapshots"][ids[0]]["sha256"] != lists["snapshots"][ids[1]]["sha256"])
            if returned is not None:
                for label, index in zip(("before", "after"), ids):
                    locations = memberships[index].get(str(returned["address"]), [])
                    counts[f"returned_missing_from_lists_{label}"] += not locations
                    for partition in {location[0] for location in locations}:
                        counts[f"returned_in_{partition}_{label}"] += 1
                    if locations:
                        recorded = lists["snapshots"][index]["surfaces"][str(returned["address"])]
                        require(exact(returned) == exact({key: recorded[key] for key in returned}),
                                "returned surface differs from list snapshot")
        else:
            require("list_snapshot_before" not in call and "list_snapshot_after" not in call,
                    "unbound list snapshot reference")
    return {"calls": len(calls), "action_transitions": len(observations),
            "unique_surfaces": len(unique_surfaces), "unique_planes": len(planes),
            "counts": dict(sorted(counts.items())),
            "callers": {key: dict(sorted(value.items())) for key, value in sorted(callers.items())},
            "list_snapshots": len(memberships) if memberships is not None else None}


def load_run(directory):
    floors = capture.json_read(directory / "floor-queries.json", MAX_REPORT)
    probe = capture.json_read(directory / "probe.json", MAX_REPORT)
    lists = capture.json_read(directory / "floor-lists.json", MAX_REPORT) if "list_report" in floors else None
    summary = inventory(floors, probe, lists)
    return floors, probe, lists, summary


def check(run, repeat, control=None):
    require(run.resolve() != repeat.resolve(), "repeat must use a distinct run directory")
    if control is not None:
        require(control.resolve() not in (run.resolve(), repeat.resolve()),
                "control must use a distinct run directory")
    first, second = load_run(run), load_run(repeat)
    for index, label in enumerate(("floor", "action", "list")):
        require(exact(first[index]) == exact(second[index]), f"{label} reports do not repeat exactly")
    if control is not None:
        baseline = load_run(control)
        def without_lists(report):
            return {key: ([{k: v for k, v in call.items() if k not in
                           ("list_snapshot_before", "list_snapshot_after")} for call in value]
                          if key == "calls" else value)
                    for key, value in report.items() if key != "list_report"}
        require(exact(without_lists(first[0])) == exact(without_lists(baseline[0])) and
                exact(first[1]) == exact(baseline[1]), "list instrumentation differs from control")
    return {"version": 1, "status": "repeatable_exploratory_floor_observations",
            "canonical_report_sha256": {label: hashlib.sha256(exact(first[index])).hexdigest()
                                        for index, label in enumerate(("floor", "action", "list"))},
            "control_equal": control is not None, "inventory": first[3],
            "limitations": "Consistency only, not authentication or native collision equivalence. "
                            "Same-caller changes are observations, not proven seam crossings. "
                            "List next order is memory layout, not executed traversal, selection or precedence."}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run", type=Path, required=True)
    parser.add_argument("--repeat", type=Path, required=True)
    parser.add_argument("--control", type=Path, help="same capture without floor-list snapshots")
    parser.add_argument("--out", type=Path, required=True, help="NEW ignored JSON output")
    args = parser.parse_args()
    try:
        output = capture.setup.checked_path(args.out)
        require(not output.exists(), "output must be new")
        report = check(args.run, args.repeat, args.control)
        capture.write_new(output, report)
        print(output)
    except (OSError, ValueError, RuntimeError, KeyError, TypeError, IndexError, OverflowError) as exc:
        parser.exit(1, f"floor_check: {exc}\n")


if __name__ == "__main__":
    main()
