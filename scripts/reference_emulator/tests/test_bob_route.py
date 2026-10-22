"""Synthetic navigation-policy tests, not ROM-derived route fixtures."""
import copy
import unittest

from scripts.reference_emulator import bob_route as route, capture


def observed(level=16, position=(-300, 400, 1000), **changes):
    state = dict(position=list(position), action=0x0C400201, forward_velocity=0, face_yaw=24576)
    extra = dict(level_num=level, area_index=1, act_num=1, processed_stick=[0, 64, 64],
                 intended_yaw=-32768, dialog_id=-1)
    extra.update(changes)
    return state, extra


class BobRouteTests(unittest.TestCase):
    def test_boot_passthrough_and_read_only_feedback(self):
        policy = route.BobRoute(1)
        raw = dict(buttons=4096, stick_x=-12, stick_y=45)
        self.assertEqual(policy.input(100, raw), raw)
        policy.observe(*observed())
        self.assertEqual(policy.input(2303, raw), raw)
        before = copy.deepcopy(policy.latest)
        result = policy.input(2304, raw)
        self.assertEqual(result["buttons"], 0)
        self.assertTrue(all(-80 <= result[key] <= 80 for key in ("stick_x", "stick_y")))
        self.assertEqual(policy.latest, before)
        self.assertEqual(raw, dict(buttons=4096, stick_x=-12, stick_y=45))

    def test_waypoint_progress_scene_reset_and_dialog_edges(self):
        policy = route.BobRoute(1)
        policy.observe(*observed(position=(0, 500, 3000)))
        policy.input(2304, route.NEUTRAL)
        self.assertEqual(policy.waypoint, 1)
        policy.observe(*observed(level=6, dialog_id=10))
        self.assertEqual(policy.input(2400, route.FORWARD), dict(route.NEUTRAL, buttons=32768))
        self.assertEqual(policy.waypoint, 0)
        self.assertEqual(policy.input(2405, route.FORWARD), route.NEUTRAL)
        policy.observe(*observed(level=7))
        with self.assertRaisesRegex(ValueError, "unexpected scene"):
            policy.input(2406, route.NEUTRAL)
        policy.observe(*observed(level=6))
        with self.assertRaisesRegex(ValueError, "deadline"):
            policy.input(3768, route.NEUTRAL)

    def test_bob_timing_holds_and_conservative_start_guards(self):
        for hold in range(1, 6):
            policy = route.BobRoute(hold)
            state, extra = observed(level=9)
            policy.observe(state, extra)
            for poll, expected in ((3767, route.NEUTRAL), (3768, route.FORWARD),
                                   (3802, route.FORWARD), (3803, route.NEUTRAL),
                                   (route.WALK_POLL - 1, route.NEUTRAL),
                                   (route.WALK_POLL, route.FORWARD),
                                   (route.WALK_POLL + hold - 1, route.FORWARD),
                                   (route.WALK_POLL + hold, route.NEUTRAL)):
                self.assertEqual(policy.input(poll, route.NEUTRAL), expected)
            for field, value in (("face_yaw", 0), ("action", 0), ("forward_velocity", 1)):
                bad = dict(state, **{field: value})
                policy.observe(bad, extra)
                with self.assertRaisesRegex(ValueError, "resting diagonal"):
                    policy.input(route.START_POLL - 1, route.NEUTRAL)
            policy.observe(state, dict(extra, act_num=2))
            with self.assertRaisesRegex(ValueError, "area/act"):
                policy.input(route.START_POLL - 1, route.NEUTRAL)
        for hold in (True, 0, 6, 1.0):
            with self.assertRaises(ValueError):
                route.BobRoute(hold)

    def test_incomplete_bootstrap_cannot_publish_replay_inputs(self):
        policy = route.BobRoute(1)
        with self.assertRaisesRegex(ValueError, "incomplete"):
            policy.validate_completed(4000)
        state, extra = observed(level=9, floor=dict(type=0))
        policy.observe(state, extra)
        with self.assertRaisesRegex(ValueError, "incomplete"):
            policy.validate_completed(route.WALK_POLL)
        policy.validate_completed(4000)
        for bad in (dict(extra, level_num=6), dict(extra, floor=None), dict(extra, floor=dict(type=41))):
            policy.observe(state, bad)
            with self.assertRaisesRegex(ValueError, "did not finish"):
                policy.validate_completed(4000)

    def test_applied_schedule_replays_changes_and_final_tail(self):
        events = [dict(poll=0, input=dict(route.NEUTRAL)),
                  dict(poll=2, input=dict(route.FORWARD)),
                  dict(poll=3, input=dict(route.NEUTRAL))]
        original = copy.deepcopy(events)
        result = route.applied_schedule(events, 8)
        self.assertEqual([s["polls"] for s in result["segments"]], [2, 1, 5])
        end, segments = 0, []
        for segment in result["segments"]:
            end += segment["polls"]
            segments.append((end, {k: segment[k] for k in route.NEUTRAL}))
        for poll in range(9):
            self.assertEqual(capture.input_at(segments, poll), route.FORWARD if poll == 2 else route.NEUTRAL)
        result["segments"][0]["buttons"] = 1
        self.assertEqual(events, original)


if __name__ == "__main__":
    unittest.main()
