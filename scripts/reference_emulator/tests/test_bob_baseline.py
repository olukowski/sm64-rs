"""ROM-free baseline construction/orchestration tests; all coordinates invented."""
from copy import deepcopy
import math
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

from scripts.reference_emulator import bob_baseline as baseline
from scripts.reference_emulator import bob_route, capture, setup, trace_export


def fixture():
    state = {"position": [123.5, 64.0, -321.25], "velocity": [-0.0, 0.0, -0.0],
             "forward_velocity": -0.0, "face_yaw": 24576, "action": 0x0C400201}
    probe = {"observations": [
        {"entry_poll": 17, "after": "never read"},
        {"entry_poll": bob_route.START_POLL, "before": state,
         "extra_before": {"global_timer": 12345, "level_num": 9, "area_index": 1,
                          "act_num": 1, "floor": {"type": 0, "normal": [0, 1, 0]}},
         "after": "never read", "extra_after": "never read"}]}
    schedule = {"version": 1, "segments": [
        dict(polls=10, **bob_route.FORWARD),
        dict(polls=4000, **bob_route.NEUTRAL)]}
    return probe, schedule


class PrepareTests(unittest.TestCase):
    def test_all_holds_timing_truncation_and_no_mutation(self):
        probe, schedule = fixture()
        originals = deepcopy((probe, schedule))
        counts = []
        for hold in range(1, 6):
            scenario, final = baseline.prepare_case(probe, schedule, hold)
            self.assertEqual(scenario["start_tick"], 12345)
            self.assertEqual(scenario["input_sampling"], trace_export.INPUT_SAMPLING)
            self.assertEqual(scenario["state_boundary"], trace_export.STATE_BOUNDARY)
            self.assertEqual(scenario["previous_input"], bob_route.NEUTRAL)
            self.assertEqual([s["ticks"] for s in scenario["segments"]],
                             [5, hold, 8 + hold])
            counts.append(sum(s["ticks"] for s in scenario["segments"]))
            self.assertEqual(scenario["lane"], {"min_x": 121.5, "max_x": 221.5,
                             "min_z": -419.25, "max_z": -317.25, "height": 64.0})
            self.assertEqual([s["polls"] for s in final["segments"]],
                             [10, bob_route.WALK_POLL - 10, hold, 40])
            # Recording count is one-based; forward injection index is zero-based.
            for poll in range(bob_route.START_POLL - 1, bob_route.WALK_POLL):
                self.assertEqual(self.input_at(final, poll), bob_route.NEUTRAL)
            self.assertEqual(self.input_at(final, bob_route.WALK_POLL), bob_route.FORWARD)
            self.assertEqual(self.input_at(final, bob_route.WALK_POLL + hold), bob_route.NEUTRAL)
            self.assertEqual(math.copysign(1, scenario["initial_state"]["velocity"][0]), -1)
            self.assertEqual(math.copysign(1, scenario["initial_state"]["forward_velocity"]), -1)
            self.assertIn("One-time", scenario["initialization"])
            self.assertIn("not native boot", scenario["initialization"])
        self.assertEqual(sum(counts), 95)
        self.assertEqual((probe, schedule), originals)
        scenario["initial_state"]["position"][0] = 999
        scenario["segments"][0]["input"]["buttons"] = 1
        self.assertEqual(scenario["segments"][2]["input"], bob_route.NEUTRAL)
        final["segments"][0]["buttons"] = 42
        final["segments"][1]["polls"] = 1
        self.assertEqual((probe, schedule), originals)
        other, _ = baseline.prepare_case(probe, schedule, 5)
        self.assertEqual(other["initial_state"]["position"][0], 123.5)

    @staticmethod
    def input_at(schedule, poll):
        end, segments = 0, []
        for segment in schedule["segments"]:
            end += segment["polls"]
            segments.append((end, {key: segment[key] for key in bob_route.NEUTRAL}))
        return capture.input_at(segments, poll)

    def test_invalid_holds_and_start(self):
        for hold in (True, False, 0, 6, -1, 1.0, "1", None):
            with self.subTest(hold=hold), self.assertRaises(ValueError):
                baseline.prepare_case(*fixture(), hold)
        for mode in ("missing", "duplicate"):
            probe, schedule = fixture()
            if mode == "missing":
                probe["observations"].pop()
            else:
                probe["observations"].append(deepcopy(probe["observations"][-1]))
            with self.assertRaisesRegex(ValueError, "exactly one"):
                baseline.prepare_case(probe, schedule, 1)

    def test_invalid_initial_and_schedule(self):
        for key, value in (("action", 0), ("velocity", [1, 0, 0]),
                           ("face_yaw", 0), ("forward_velocity", 1)):
            probe, schedule = fixture()
            probe["observations"][-1]["before"][key] = value
            with self.subTest(key=key), self.assertRaises(ValueError):
                baseline.prepare_case(probe, schedule, 1)
        for key, value in (("level_num", 16), ("area_index", 2), ("act_num", True),
                           ("global_timer", -1), ("floor", {"type": 1, "normal": [0, 1, 0]}),
                           ("floor", {"type": 0, "normal": [1, 0, 0]})):
            probe, schedule = fixture()
            probe["observations"][-1]["extra_before"][key] = value
            with self.subTest(key=key), self.assertRaises(ValueError):
                baseline.prepare_case(probe, schedule, 1)
        for invalid in ({"version": True, "segments": []},
                        {"version": 1, "segments": []},
                        {"version": 1, "segments": [dict(polls=4000, buttons=0, stick_x=0, stick_y=True)]}):
            with self.assertRaises(ValueError):
                baseline.prepare_case(fixture()[0], invalid, 1)


class OrchestrationTests(unittest.TestCase):
    def test_paths_and_bounds_fail_before_children(self):
        setup.CACHE.mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(dir=setup.CACHE) as temporary, \
             patch.object(baseline, "_run") as child:
            root = Path(temporary)
            for timeout in (0, 601, True):
                with self.assertRaises(ValueError):
                    baseline.run(root / "new", root / "missing.rom", root / "manifest", timeout)
            with self.assertRaises(ValueError):
                baseline.run(setup.PROJECT / "not-cache", root / "missing.rom", root / "manifest", 240)
            with self.assertRaises(OSError):
                baseline.run(root / "new", root / "missing.rom", root / "manifest", 240)
            self.assertFalse((root / "new").exists())
            child.assert_not_called()

    def test_sweep_and_fail_closed(self):
        setup.CACHE.mkdir(exist_ok=True)
        for failure in (None, "compare", "capture"):
            with self.subTest(failure=failure), tempfile.TemporaryDirectory(dir=setup.CACHE) as temporary:
                root = Path(temporary)
                rom = root / "invented.rom"
                rom.touch()
                out = root / "new"
                calls = []

                def fake(command, output, timeout, *, native=False):
                    command = [str(part) for part in command]
                    calls.append((command, timeout, native))
                    if "scripts.reference_emulator.capture" in command:
                        directory = Path(command[command.index("--out") + 1])
                        directory.mkdir()
                        if "--input-policy" in command:
                            probe, schedule = fixture()
                            capture.write_new(directory / "probe.json", probe)
                            capture.write_new(directory / "inputs.json", schedule)
                        else:
                            if failure == "capture":
                                raise subprocess.CalledProcessError(1, command)
                            frames = int(command[command.index("--updates") + 1])
                            capture.write_new(directory / "probe.json", {"observations": [{}] * frames})
                    elif "compare" in command:
                        capture.write_new(output, {"status": "different" if failure == "compare" else
                                                  "equal_observations", "float_policy": "exact_f32_bits"})
                    else:
                        self.assertTrue(native)
                        self.assertNotIn(str(rom), command)
                        self.assertNotIn("reference.json", " ".join(command))
                        capture.write_new(output, {})

                with patch.object(baseline, "_run", side_effect=fake), \
                     patch.object(capture, "rom_bytes"), patch.object(capture, "checked_manifest"), \
                     patch.object(baseline.movement_check, "validate", return_value={}):
                    if failure:
                        with self.assertRaises((ValueError, subprocess.CalledProcessError)):
                            baseline.run(out, rom, root / "manifest.json", 240)
                        self.assertFalse((out / "summary.json").exists())
                        continue
                    result = baseline.run(out, rom, root / "manifest.json", 240)
                self.assertEqual(result["transitions_per_sweep"], 95)
                self.assertEqual(result["reference_transitions"], 190)
                self.assertEqual(len(result["cases"]), 5)
                self.assertEqual(len(calls), 31)  # bootstrap + 5 * (native + 2 replay + 3 compare)
                bootstrap = calls[0][0]
                self.assertEqual(bootstrap[bootstrap.index("--policy-hold") + 1], "5")
                self.assertEqual(bootstrap[bootstrap.index("--updates") + 1], "3500")
                for command, _, _ in calls:
                    if "--record-from-poll" in command:
                        self.assertEqual(command[command.index("--record-from-poll") + 1], "3879")
                        self.assertNotIn("--input-policy", command)
                with self.assertRaisesRegex(ValueError, "NEW"):
                    baseline.run(out, rom, root / "manifest.json", 240)

    def test_subprocess_is_bounded_checked_and_exclusive(self):
        setup.CACHE.mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(dir=setup.CACHE) as temporary:
            output = Path(temporary) / "native.json"
            with patch.object(subprocess, "run") as child:
                baseline._run(["cargo", "example"], output, 17, native=True)
                kwargs = child.call_args.kwargs
                self.assertEqual(kwargs["cwd"], setup.PROJECT)
                self.assertEqual(kwargs["timeout"], 17)
                self.assertTrue(kwargs["check"])
                self.assertNotIn("SM64_ROM", kwargs["env"])
                with self.assertRaises(FileExistsError):
                    baseline._run(["cargo"], output, 17)
                self.assertEqual(child.call_count, 1)


if __name__ == "__main__":
    unittest.main()
