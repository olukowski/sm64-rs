"""Authored synthetic observations only: no ROM or recorded game fixtures."""
import copy
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from scripts.reference_emulator import movement_check as check, trace_export as export


def fixture():
    lane = dict(min_x=-1, max_x=1, min_z=-1, max_z=1, height=0)
    def state(action):
        return dict(position=[0.0, 0.0, 0.0], velocity=[0.0, 0.0, -0.0],
                    forward_velocity=0.0, face_yaw=-32768, action=action)
    def extra(tick, forward):
        return dict(global_state_pointer=export.STATE_ADDRESS, global_timer=tick, button_pressed=0,
                    floor=dict(address=0x80300000, type=0, normal=[0, 1, 0],
                               vertices=[[-10, 0, -10], [10, 0, -10], [0, 0, 10]]),
                    floor_height=0, water_level=-1000, ceiling_height=2000, wall=0,
                    dialog_id=-1, squish_timer=0, quicksand_depth=0, held_object=0, ridden_object=0,
                    intended_yaw=-32768, intended_magnitude=32 if forward else 0,
                    processed_stick=[0, 64, 64] if forward else [0, 0, 0])
    observations = []
    actions = [check.IDLE] + [check.WALKING] * 10 + [check.BRAKING, check.BRAKING, check.STOP]
    for i in range(13):
        observations.append(dict(before=state(actions[i]), after=state(actions[i+1]),
                                 input=copy.deepcopy(check.FORWARD if i < 10 else check.NEUTRAL),
                                 extra_before=extra(100+i, i < 10), extra_after=extra(100+i, i < 10),
                                 state_address=export.STATE_ADDRESS, return_pc=0x80240000,
                                 entry_poll=i+2, return_poll=i+2, entry_vi=i+2, return_vi=i+2))
    probe = dict(version=1, status="exploratory_boundary_probe_not_validated_game_trace",
                 rom_sha256=export.US_SHA256, entry=0x80254830,
                 counts=dict(vi=14, entries=13, returns=13, controller_polls=14),
                 input_schedule=[dict(end_poll=1, input=copy.deepcopy(check.NEUTRAL)),
                                 dict(end_poll=11, input=copy.deepcopy(check.FORWARD))],
                 input_events=[dict(poll=0, vi=1, input=copy.deepcopy(check.NEUTRAL)),
                               dict(poll=1, vi=2, input=copy.deepcopy(check.FORWARD)),
                               dict(poll=11, vi=12, input=copy.deepcopy(check.NEUTRAL))],
                 observations=observations, emulator={"synthetic": True})
    scenario = dict(version=1, scenario="synthetic-envelope", initialization="Authored one-time rest state",
                    input_sampling=export.INPUT_SAMPLING, state_boundary=export.STATE_BOUNDARY,
                    start_tick=100, initial_state=state(check.IDLE), previous_input=copy.deepcopy(check.NEUTRAL),
                    lane=lane, segments=[dict(ticks=10, input=copy.deepcopy(check.FORWARD)),
                                         dict(ticks=3, input=copy.deepcopy(check.NEUTRAL))])
    return probe, scenario


class MovementCheckTests(unittest.TestCase):
    def test_independent_reference_not_dynamics(self):
        probe, scenario = fixture()
        original = copy.deepcopy((probe, scenario))
        trace = check.validate(probe, scenario)
        self.assertEqual(trace["manifest"]["source"], "reference")
        self.assertEqual(len(trace["frames"]), 13)
        self.assertEqual(trace["initial_state"], scenario["initial_state"])
        self.assertIn("movement_envelope", json.loads(trace["manifest"]["settings"]))
        self.assertEqual((probe, scenario), original)

    def test_environment_failures_both_boundaries(self):
        changes = [("floor.type", 1), ("floor.normal", [0, .9, .1]),
                   ("floor.vertices", [[-10, 1, -10], [10, 0, -10], [0, 0, 10]]),
                   ("floor.vertices", [[0, 0, 0], [1, 0, 0], [0, 0, 1]]),
                   ("floor.vertices", [[0, 0, 0]] * 3),
                   ("floor_height", 1), ("water_level", -160), ("wall", 1),
                   ("ceiling_height", 999), ("dialog_id", 0), ("squish_timer", 1),
                   ("quicksand_depth", .1), ("held_object", 1), ("ridden_object", 1),
                   ("processed_stick", [0, 0, 0])]
        for boundary in ("extra_before", "extra_after"):
            for path, value in changes:
                with self.subTest(boundary=boundary, path=path):
                    p, s = fixture()
                    target = p["observations"][0][boundary]
                    keys = path.split(".")
                    for key in keys[:-1]:
                        target = target[key]
                    target[keys[-1]] = value
                    with self.assertRaises(ValueError):
                        check.validate(p, s)

    def test_report_scenario_and_chain_negatives(self):
        changes = [
            ("p.rom_sha256", "0"*64), ("p.entry", 0x80254834), ("p.version", 2),
            ("p.status", "validated"), ("p.counts.entries", 14), ("p.counts.controller_polls", 4),
            ("p.input_schedule.1.end_poll", 1), ("p.input_events.1.poll", 2),
            ("p.input_events.1.vi", 5), ("p.observations.0.input.stick_y", 0),
            ("p.observations.3.extra_after.global_timer", 200),
            ("p.observations.3.return_poll", 6), ("p.observations.2.before.position", [1, 0, 0]),
            ("p.observations.0.extra_after.intended_yaw", 0),
            ("p.observations.0.extra_after.intended_magnitude", 0),
            ("p.observations.0.extra_before.button_pressed", 1),
            ("p.observations.0.state_address", 0x8033B174),
            ("s.initial_state.velocity.2", 0.0),  # signed-zero mismatch
            ("s.initial_state.action", check.WALKING), ("s.previous_input.stick_y", 80),
            ("s.input_sampling", "other"), ("s.state_boundary", "other"),
            ("s.start_tick", 99), ("s.start_tick", 101), ("s.lane.max_x", 100),
            ("s.lane.height", 1), ("s.lane.min_x", 2),
            ("s.segments.0.input.buttons", 1), ("s.segments.0.input.stick_y", 79),
            ("s.segments.0.ticks", 44), ("s.segments.1.ticks", 2),
            ("p.observations.12.after.action", check.BRAKING),
        ]
        for path, value in changes:
            with self.subTest(path=path):
                p, s = fixture()
                target = {"p": p, "s": s}
                keys = path.split(".")
                for key in keys[:-1]:
                    target = target[int(key)] if isinstance(target, list) else target[key]
                key = int(keys[-1]) if isinstance(target, list) else keys[-1]
                target[key] = value
                with self.assertRaises(ValueError):
                    check.validate(p, s)

    def test_actual_predecessor_and_full_unselected_chain(self):
        p, s = fixture()
        # An extra idle observation before the selected window, with nonneutral
        # predecessor supplied by the schedule (not invented by the exporter).
        first = copy.deepcopy(p["observations"][0])
        first.update(input=copy.deepcopy(check.NEUTRAL), entry_poll=1, return_poll=1, entry_vi=1, return_vi=1)
        first["after"] = copy.deepcopy(first["before"])
        for side in ("extra_before", "extra_after"):
            first[side]["global_timer"] = 99
            first[side]["processed_stick"] = [0, 0, 0]
            first[side]["intended_magnitude"] = 0
        p["observations"].insert(0, first)
        p["counts"].update(entries=14, returns=14)
        self.assertEqual(check.validate(p, s)["start_tick"], 100)
        saved = copy.deepcopy(p)
        p["observations"][0]["extra_after"]["button_pressed"] = 1
        with self.assertRaises(ValueError):
            check.validate(p, s)
        p = saved
        p["input_schedule"] = [dict(end_poll=11, input=copy.deepcopy(check.FORWARD))]
        p["input_events"] = [dict(poll=0, vi=1, input=copy.deepcopy(check.FORWARD)),
                             dict(poll=11, vi=12, input=copy.deepcopy(check.NEUTRAL))]
        p["observations"][0]["input"] = copy.deepcopy(check.FORWARD)
        with self.assertRaisesRegex(ValueError, "actual predecessor"):
            check.validate(p, s)

    def test_hold_nine_rejected_without_replaying_dynamics(self):
        p, s = fixture()
        del p["observations"][1]
        for i, obs in enumerate(p["observations"]):
            obs.update(entry_poll=i+2, return_poll=i+2, entry_vi=i+2, return_vi=i+2)
            for side in ("extra_before", "extra_after"):
                obs[side]["global_timer"] = 100+i
        p["counts"].update(entries=12, returns=12, vi=13, controller_polls=13)
        p["input_schedule"][1]["end_poll"] = 10
        p["input_events"][2].update(poll=10, vi=11)
        s["segments"][0]["ticks"] = 9
        with self.assertRaises(ValueError):
            check.validate(p, s)

    def test_cli_new_ignored_output_and_read_only_inputs(self):
        from scripts.reference_emulator import setup
        with tempfile.TemporaryDirectory(dir=setup.CACHE) as directory:
            root = Path(directory)
            p, s = fixture()
            for name, value in (("probe", p), ("scenario", s)):
                (root / (name + ".json")).write_text(json.dumps(value))
            command = [sys.executable, "-m", "scripts.reference_emulator.movement_check",
                       "--probe", str(root / "probe.json"), "--scenario", str(root / "scenario.json"),
                       "--out", str(root / "new")]
            result = subprocess.run(command, capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertTrue((root / "new/trace.json").exists())
            self.assertNotEqual(subprocess.run(command, capture_output=True).returncode, 0)
            self.assertEqual(json.loads((root / "probe.json").read_text()), p)


if __name__ == "__main__":
    unittest.main()
