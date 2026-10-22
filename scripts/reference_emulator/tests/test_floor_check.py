"""Authored reports only: no ROM, emulator, or captured evidence required."""
from copy import deepcopy
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from scripts.reference_emulator import capture, floor_check as check, floor_lists, floor_probe, setup


def bits(value):
    import struct
    return struct.unpack(">I", struct.pack(">f", value))[0]


def digest(snapshot):
    data = {k: snapshot[k] for k in ("heads", "nodes", "surfaces")}
    snapshot["sha256"] = hashlib.sha256(check.exact(data)).hexdigest()


def fixture(with_lists=False):
    """Two consecutive action boundaries and three natural-shaped floor returns."""
    metadata = {"provenance": "authored fixture", "sources": {"fixture": {"commit": "authored"}},
                "commands": [["synthetic"]], "artifacts": {"fixture": {"sha256": "authored"}},
                "host": {"platform": "synthetic"}}
    pad = {"buttons": 0, "stick_x": 0, "stick_y": 0}
    def state(x):
        return {"position": [float(x), 0.0, 0.0], "velocity": [0.0] * 3,
                "forward_velocity": 0.0, "face_yaw": 0, "action": 0}
    observations = []
    for i in range(2):
        extra = {"global_state_pointer": 0x8033B170, "global_timer": 40 + i,
                 "button_pressed": 0, "level_num": 9, "area_index": 1, "act_num": 1}
        observations.append(dict(before=state(i), after=state(i + 1), input=deepcopy(pad),
                                 state_address=0x8033B170, return_pc=0x80250000,
                                 entry_poll=i + 1, return_poll=i + 1, entry_vi=10 + 3 * i,
                                 return_vi=12 + 3 * i, extra_before=deepcopy(extra),
                                 extra_after=deepcopy(extra)))
    common = dict(rom_sha256=capture.US_SHA256, record_from_poll=1, emulator=metadata)
    probe = dict(version=1, entry=0x80254830, input_policy=None, **deepcopy(common),
                 status="exploratory_boundary_probe_not_validated_game_trace",
                 input_schedule=[{"end_poll": 2, "input": pad}], observations=observations,
                 counts={"entries": 2, "returns": 2, "vi": 15, "controller_polls": 2},
                 floor_probe={"file": "floor-queries.json", "calls": 3})
    surfaces = [dict(address=0x80002000 + i * 0x40, type=0,
                     vertices=[[0, 0, 0], [10, 0, 0], [0, i + 1, 10]],
                     normal_bits=[bits(0.5), bits(0.75), bits(0.0)],
                     origin_offset_bits=bits(-1.0)) for i in range(2)]
    floors = floor_probe.FloorProbe(None).report()
    floors.update(deepcopy(common))
    for i, returned in enumerate([*surfaces, None]):
        ordinal = min(i + 1, 2)
        context = dict(global_timer=39 + ordinal, controller_polls=ordinal,
                       vi=10 + (ordinal - 1) * 3, action_entries=ordinal,
                       inside_action=True, level_num=9, area_index=1, act_num=1)
        values = [-0.5, 10.0, -0.0]
        height = [1.0, 2.0, -11000.0][i]
        floors["calls"].append(dict(context_before=deepcopy(context), context_after=context,
                                    return_pc=0x80240000, output_address=0x80000100,
                                    input_bits=list(map(bits, values)), input_values=values,
                                    height_bits=bits(height), height=height, surface=returned))
    lists = None
    if with_lists:
        floors["list_report"] = "floor-lists.json"
        lists = floor_lists.FloorLists(None).report()
        lists.update(deepcopy(common))
        heads = {p: [[0, 0] for _ in range(256)] for p in ("static", "dynamic")}
        heads["static"][0] = [0x80001000, 0xDEADBEEF]
        heads["dynamic"][7] = [0x80001008, 0]
        snapshot = dict(heads=heads, nodes={0x80001000: [0x80001008, surfaces[0]["address"]],
                                          0x80001008: [0, surfaces[1]["address"]]},
                        surfaces={s["address"]: dict(deepcopy(s), force=0,
                                  flags_room_lower_y_bits=0, upper_y=10,
                                  object_address_bits=0) for s in surfaces})
        # Match on-disk JSON keys, rather than accidentally testing integer-key dictionaries.
        snapshot = json.loads(json.dumps(snapshot))
        digest(snapshot)
        other = deepcopy(snapshot)
        other["heads"]["static"][255][1] = 1  # unrelated raw embedded-head word
        digest(other)
        lists["snapshots"] = [snapshot, other]
        for call in floors["calls"]:
            call.update(list_snapshot_before=0, list_snapshot_after=1)
    return floors, probe, lists


class FloorCheckTests(unittest.TestCase):
    def test_valid_inventory_and_membership(self):
        for listed in (False, True):
            with self.subTest(listed=listed):
                reports = fixture(listed)
                saved = deepcopy(reports)
                result = check.inventory(*reports)
                self.assertEqual(reports, saved)
                self.assertEqual((result["calls"], result["action_transitions"],
                                  result["unique_surfaces"], result["unique_planes"]), (3, 2, 2, 1))
                self.assertEqual(result["callers"]["0x80240000"], {
                    "calls": 3, "inside_action": 3, "sloped_return": 2,
                    "surface_changes": 1, "changes_sharing_two_vertices": 1})
                counts = result["counts"]
                for name, expected in {"inside_action": 3, "no_floor": 1, "sloped_return": 2,
                                       "fractional_xz": 3, "negative_fractional_xz": 3,
                                       "same_caller_height_changes": 2,
                                       "same_caller_surface_changes": 1,
                                       "changes_sharing_two_vertices": 1}.items():
                    self.assertEqual(counts[name], expected, name)
                self.assertEqual(result["list_snapshots"], 2 if listed else None)
                if listed:
                    self.assertEqual(counts["changed_list_snapshot_during_call"], 3)
                    for boundary in ("before", "after"):
                        self.assertEqual(counts[f"returned_in_static_{boundary}"], 2)
                        self.assertEqual(counts[f"returned_in_dynamic_{boundary}"], 1)
                        self.assertEqual(counts[f"returned_missing_from_lists_{boundary}"], 0)

    def test_report_mutations_rejected(self):
        cases = [((0, "calls", 0, "context_after", "vi"), 11),
                 ((0, "calls", 0, "input_values", 0), -0.25),
                 ((0, "calls", 0, "input_values", 2), 0.0),
                 ((0, "calls", 0, "height"), 9.0),
                 ((0, "calls", 0, "input_bits", 0), 0x7F800000),
                 ((0, "calls", 0, "height_bits"), 0x7FC00000),
                 ((0, "calls", 0, "surface", "normal_bits", 0), 0xFF800000),
                 ((0, "record_from_poll"), 2),
                 ((1, "record_from_poll"), 2),
                 ((1, "input_policy"), "closed-loop"),
                 ((1, "observations", 0, "extra_before", "level_num"), 8),
                 ((1, "observations", 0, "extra_after", "area_index"), 2),
                 ((1, "observations", 1, "before", "position", 0), 99.0),
                 ((1, "observations", 0, "extra_after", "global_state_pointer"), 0),
                 ((1, "input_schedule", 0, "input", "buttons"), 1),
                 ((1, "input_schedule", 0, "end_poll"), 0),
                 ((1, "counts", "returns"), 1),
                 ((1, "floor_probe", "calls"), 2)]
        for path, value in cases:
            with self.subTest(path=path, value=value):
                reports = fixture()
                target = reports
                for key in path[:-1]:
                    target = target[key]
                target[path[-1]] = value
                with self.assertRaises(ValueError):
                    check.inventory(*reports)

    def test_context_scene_gate_and_action_binding(self):
        for key, value in (("level_num", 8), ("area_index", 2), ("act_num", 2),
                           ("global_timer", 99), ("controller_polls", 0),
                           ("action_entries", 0), ("vi", 13)):
            with self.subTest(key=key):
                reports = fixture()
                for side in ("context_before", "context_after"):
                    reports[0]["calls"][0][side][key] = value
                with self.assertRaises(ValueError):
                    check.inventory(*reports)

    def test_list_reference_and_binding_failures(self):
        for value in (-1, 2, True, None):
            with self.subTest(value=value):
                reports = fixture(True)
                reports[0]["calls"][0]["list_snapshot_before"] = value
                with self.assertRaises(ValueError):
                    check.inventory(*reports)
        reports = fixture(True)
        del reports[0]["calls"][0]["list_snapshot_after"]
        with self.assertRaises(KeyError):
            check.inventory(*reports)
        with self.assertRaisesRegex(ValueError, "sidecar"):
            check.inventory(*fixture(True)[:2])
        reports = fixture()
        reports[0]["calls"][0]["list_snapshot_before"] = 0
        with self.assertRaisesRegex(ValueError, "unbound"):
            check.inventory(*reports)

    def test_snapshot_corruption_and_surface_disagreement(self):
        for kind in ("digest", "cycle", "missing_node", "missing_surface", "orphan_surface", "metadata"):
            with self.subTest(kind=kind):
                reports = fixture(True)
                snap = reports[2]["snapshots"][0]
                if kind == "digest":
                    snap["sha256"] = "0" * 64
                elif kind == "cycle":
                    snap["nodes"][str(0x80001008)][0] = 0x80001000
                elif kind == "missing_node":
                    del snap["nodes"][str(0x80001008)]
                elif kind == "missing_surface":
                    del snap["surfaces"][str(0x80002000)]
                elif kind == "orphan_surface":
                    extra = deepcopy(snap["surfaces"][str(0x80002000)])
                    extra["address"] += 0x100
                    snap["surfaces"][str(extra["address"])] = extra
                else:
                    snap["surfaces"][str(0x80002000)]["type"] = 1
                if kind != "digest":
                    digest(snap)
                with self.assertRaises((ValueError, KeyError)):
                    check.inventory(*reports)

    def test_exact_repeat_and_control_only_strip_list_fields(self):
        for run, repeat, control in ((Path("same"), Path("same"), None),
                                     (Path("run"), Path("repeat"), Path("run"))):
            with self.assertRaisesRegex(ValueError, "distinct run directory"):
                check.check(run, repeat, control)
        reports = fixture(True)
        first = (*reports, check.inventory(*reports))
        control = (*fixture(), check.inventory(*fixture()))
        with patch.object(check, "load_run", side_effect=[first, deepcopy(first), control]):
            self.assertTrue(check.check(Path("run"), Path("repeat"), Path("control"))["control_equal"])
        for index, path, value in ((0, ("calls", 0, "height_bits"), bits(2.0)),
                                   (0, ("calls", 0, "input_values", 2), 0.0),
                                   (1, ("observations", 0, "before", "position", 1), -0.0),
                                   (2, ("snapshots", 0, "sha256"), "different")):
            changed = deepcopy(first)
            target = changed[index]
            for key in path[:-1]:
                target = target[key]
            target[path[-1]] = value
            with self.subTest(index=index, path=path):
                with patch.object(check, "load_run", side_effect=[first, changed]):
                    with self.assertRaisesRegex(ValueError, "repeat exactly"):
                        check.check(Path("run"), Path("repeat"))
        changed = deepcopy(control)
        changed[0]["abi"]["status"] = "different"
        with patch.object(check, "load_run", side_effect=[first, first, changed]):
            with self.assertRaisesRegex(ValueError, "control"):
                check.check(Path("run"), Path("repeat"), Path("control"))

    def test_real_files_cli_success_failure_and_no_overwrite(self):
        setup.CACHE.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=setup.CACHE) as temporary:
            root = Path(temporary)
            for name in ("run", "repeat"):
                directory = root / name
                directory.mkdir()
                for filename, report in zip(("floor-queries.json", "probe.json", "floor-lists.json"),
                                            fixture(True)):
                    (directory / filename).write_text(json.dumps(report))
            self.assertEqual(check.load_run(root / "run")[3]["calls"], 3)
            output = root / "inventory.json"
            argv = ["floor_check", "--run", str(root / "run"), "--repeat", str(root / "repeat"),
                    "--out", str(output)]
            with patch("sys.argv", argv), patch("builtins.print"):
                check.main()
            original = output.read_bytes()
            self.assertEqual(json.loads(original)["status"], "repeatable_exploratory_floor_observations")
            with patch("sys.argv", argv), self.assertRaises(SystemExit) as raised:
                check.main()
            self.assertEqual(raised.exception.code, 1)
            self.assertEqual(output.read_bytes(), original)
            bad = json.loads((root / "repeat" / "floor-queries.json").read_text())
            bad["calls"][0]["height"] = 999.0
            (root / "repeat" / "floor-queries.json").write_text(json.dumps(bad))
            argv[-1] = str(root / "failure.json")
            with patch("sys.argv", argv), self.assertRaises(SystemExit) as raised:
                check.main()
            self.assertEqual(raised.exception.code, 1)
            self.assertFalse((root / "failure.json").exists())


if __name__ == "__main__":
    unittest.main()
