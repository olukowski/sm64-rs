"""Internally authored synthetic snapshots only: no original-game capture evidence."""
from copy import deepcopy
import json
import unittest

from scripts.reference_emulator.trace_export import build_trace, Error, LIMITATIONS, US_SHA256


PREVIOUS = {"buttons": 1, "stick_x": 0, "stick_y": 0}
MANIFEST = {"provenance": "Internally authored synthetic fixture metadata, not an original capture",
            "sources": {"fixture": {"commit": "authored"}}, "commands": [["authored fixture"]],
            "artifacts": {"fixture": {"sha256": "authored placeholder"}},
            "host": {"platform": "synthetic"}}


def state(x):
    return {"position": [float(x), 0.0, 0.0], "velocity": [0.0, 0.0, 0.0],
            "forward_velocity": 0.0, "face_yaw": -32768, "action": 0xFFFFFFFF}


def chain():
    result = []
    previous = PREVIOUS["buttons"]
    for i, buttons in enumerate((3, 2, 6)):
        extra = {"global_state_pointer": 0x8033B170, "global_timer": 40 + i,
                 "button_pressed": buttons & ~previous}
        result.append({"before": state(i), "after": state(i + 1),
                       "input": {"buttons": buttons, "stick_x": -128, "stick_y": 127},
                       "state_address": 0x8033B170, "return_pc": 0x80250000,
                       "entry_poll": i + 1, "return_poll": i + 1,
                       # Deliberately not one VI per tick.
                       "entry_vi": 10 + i * 3, "return_vi": 12 + i * 3,
                       "extra_before": deepcopy(extra), "extra_after": deepcopy(extra)})
        previous = buttons
    return result


class TraceExportTests(unittest.TestCase):
    def build(self, observations=None, **kwargs):
        args = dict(previous_input=deepcopy(PREVIOUS))
        args.update(kwargs)
        return build_trace(chain() if observations is None else observations,
                           deepcopy(MANIFEST), "synthetic builder test", "authored snapshots", **args)

    def test_minimal_accepted_chain_and_schema(self):
        trace = self.build(chain()[:1])
        self.assertEqual(set(trace), {"version", "manifest", "start_tick", "initial_state", "previous_input", "frames"})
        self.assertEqual(trace["version"], 1)
        self.assertEqual(trace["start_tick"], 40)
        self.assertEqual(trace["initial_state"], state(0))
        self.assertEqual(trace["frames"], [{"tick": 40, "input": chain()[0]["input"], "state": state(1)}])
        self.assertEqual(trace["previous_input"], PREVIOUS)
        m = trace["manifest"]
        self.assertEqual(m["source"], "reference")  # Builder schema, not fixture provenance.
        self.assertEqual(m["rom_sha256"], US_SHA256)
        self.assertEqual(m["producer"], "sm64-rs Mupen64Plus capture")
        self.assertEqual(m["version"], "1")
        settings = json.loads(m["settings"])
        self.assertEqual(settings["setup_manifest"], MANIFEST)
        self.assertEqual(settings["limitations"], LIMITATIONS)
        for key in ("input_sampling", "state_boundary"):
            self.assertIn("US0 entry 0x80254830", m[key])
            self.assertIn("caller return", m[key])
        json.dumps(trace, allow_nan=False)

    def test_slice_and_actual_timer(self):
        observations = chain()
        trace = self.build(observations, start=1, count=2)
        self.assertEqual(trace["start_tick"], 41)
        self.assertEqual([f["tick"] for f in trace["frames"]], [41, 42])
        self.assertEqual(trace["initial_state"], observations[1]["before"])
        self.assertEqual(trace["previous_input"], observations[0]["input"])
        self.assertEqual(len(self.build(start=2)["frames"]), 1)

    def test_no_mutation_or_aliasing(self):
        obs, manifest, previous = chain(), deepcopy(MANIFEST), deepcopy(PREVIOUS)
        saved = deepcopy((obs, manifest, previous))
        trace = build_trace(obs, manifest, "synthetic", "authored", previous_input=previous)
        self.assertEqual((obs, manifest, previous), saved)
        trace["initial_state"]["position"][0] = 99
        trace["frames"][0]["state"]["position"][0] = 99
        trace["frames"][0]["input"]["buttons"] = 99
        trace["previous_input"]["buttons"] = 99
        self.assertEqual((obs, manifest, previous), saved)
        sliced = self.build(obs, start=1)
        sliced["previous_input"]["buttons"] = 99
        self.assertEqual(obs, saved[0])

    def reject_change(self, path, value, **kwargs):
        obs = chain()
        target = obs
        for key in path[:-1]:
            target = target[key]
        target[path[-1]] = value
        with self.assertRaises(ValueError):
            self.build(obs, **kwargs)

    def test_observation_invariants_individually(self):
        cases = [
            ((0, "state_address"), 0x8033B174),
            ((0, "state_address"), True),
            ((0, "return_pc"), 0x80250001),
            ((0, "return_pc"), 0x80400000),
            ((0, "return_pc"), 0x7FFFFFFC),
            ((0, "return_pc"), 0x80254830),
            ((1, "return_pc"), 0x80250004),
            ((0, "entry_poll"), 0),
            ((0, "return_poll"), 2),
            ((1, "entry_poll"), 3),
            ((0, "entry_poll"), True),
            ((0, "entry_vi"), -1),
            ((0, "entry_vi"), 13),
            ((1, "entry_vi"), 11),
            ((0, "return_vi"), 9),
            ((0, "return_vi"), 12.0),
            ((0, "extra_before", "global_state_pointer"), 0),
            ((0, "extra_after", "global_state_pointer"), 0),
            ((0, "extra_after", "global_timer"), 41),
            ((1, "extra_before", "global_timer"), 42),
            ((0, "extra_before", "global_timer"), -1),
            ((0, "extra_before", "global_timer"), 2**32),
            ((0, "extra_after", "global_timer"), True),
            ((0, "extra_before", "button_pressed"), 3),
            ((0, "extra_after", "button_pressed"), 0),
            ((1, "extra_before", "button_pressed"), 2),
            ((0, "extra_before", "button_pressed"), 65536),
            ((1, "before", "position", 0), 55.0),
            ((1, "before", "velocity", 1), -0.0),
            ((1, "before", "forward_velocity"), -0.0),
            ((1, "before", "face_yaw"), 0),
            ((1, "before", "action"), 0),
        ]
        for path, value in cases:
            with self.subTest(path=path, value=value):
                self.reject_change(path, value)

    def test_joint_counter_failures(self):
        for counter, values in (("global_timer", (0xFFFFFFFF, 0, 1)),):
            obs = chain()
            for item, value in zip(obs, values):
                for side in ("extra_before", "extra_after"):
                    item[side][counter] = value
            with self.assertRaises(ValueError):
                self.build(obs)
        for polls in ((1, 1, 2), (1, 3, 4)):
            obs = chain()
            for item, poll in zip(obs, polls):
                item["entry_poll"] = item["return_poll"] = poll
            with self.assertRaises(ValueError):
                self.build(obs)

    def test_complete_states_types_ranges_and_f32(self):
        for side in ("before", "after"):
            for field in state(0):
                obs = chain()
                del obs[0][side][field]
                with self.subTest(side=side, missing=field), self.assertRaises(ValueError):
                    self.build(obs)
            invalid = [("position", []), ("velocity", [0, 0]), ("velocity", "abc"),
                       ("face_yaw", -32769), ("face_yaw", 32768), ("face_yaw", True),
                       ("face_yaw", 1.0), ("action", -1), ("action", 2**32), ("action", False)]
            for field, value in invalid:
                with self.subTest(side=side, field=field, value=value):
                    self.reject_change((0, side, field), value)
            for value in (float("nan"), float("inf"), -float("inf"), 3.5e38, 10**1000, True, "0", None):
                for path in ((0, side, "position", 0), (0, side, "velocity", 2), (0, side, "forward_velocity")):
                    with self.subTest(path=path, value=str(value)[:30]):
                        self.reject_change(path, value)

    def test_float_comparison_uses_f32_bits(self):
        obs = chain()
        obs[1]["before"]["position"][0] = 1.0 + 2**-30
        self.build(obs)  # Same f32, though Python doubles differ.
        self.reject_change((1, "before", "position", 0), 1.0 + 2**-22)

    def test_raw_input_and_previous_input(self):
        for field, invalid in (("buttons", (-1, 65536, True, 1.0)),
                               ("stick_x", (-129, 128, False, 0.0)),
                               ("stick_y", (-129, 128, True, "0"))):
            for value in invalid:
                with self.subTest(field=field, value=value):
                    self.reject_change((0, "input", field), value)
                    previous = deepcopy(PREVIOUS)
                    previous[field] = value
                    with self.assertRaises(ValueError):
                        self.build(previous_input=previous)
        for raw in ({}, {**PREVIOUS, "extra": 1}, None):
            with self.assertRaises(ValueError):
                self.build(previous_input=raw)
        with self.assertRaises(ValueError):
            self.build(previous_input={**PREVIOUS, "buttons": 0})

    def test_all_observations_validated_outside_slice(self):
        self.reject_change((0, "extra_before", "button_pressed"), 0, start=1, count=1)
        self.reject_change((2, "before", "position", 0), 99, start=0, count=1)
        self.reject_change((2, "after", "action"), -1, start=0, count=1)
        self.reject_change((2, "return_pc"), 0, start=0, count=1)

    def test_bad_slices_and_frame_bounds(self):
        for kwargs in ({"start": -1}, {"start": 3}, {"start": 4}, {"start": True},
                       {"start": 1.0}, {"count": 0}, {"count": -1}, {"count": True},
                       {"count": 1.0}, {"count": 10001}, {"start": 2, "count": 2}):
            with self.subTest(kwargs=kwargs), self.assertRaises(ValueError):
                self.build(**kwargs)
        with self.assertRaises(ValueError):
            self.build([])

    def test_missing_fields_and_blank_metadata(self):
        for field in chain()[0]:
            obs = chain()
            del obs[0][field]
            with self.subTest(field=field), self.assertRaises(ValueError):
                self.build(obs)
        for side in ("extra_before", "extra_after"):
            for field in chain()[0][side]:
                obs = chain()
                del obs[0][side][field]
                with self.subTest(side=side, field=field), self.assertRaises(ValueError):
                    self.build(obs)
        for value in ("", " \n", None, 1):
            for scenario, initialization in ((value, "authored"), ("synthetic", value)):
                with self.assertRaises(ValueError):
                    build_trace(chain(), MANIFEST, scenario, initialization, previous_input=PREVIOUS)
        with self.assertRaises(Error):
            build_trace(chain(), {"bad": float("nan")}, "synthetic", "authored", previous_input=PREVIOUS)


if __name__ == "__main__":
    unittest.main()
