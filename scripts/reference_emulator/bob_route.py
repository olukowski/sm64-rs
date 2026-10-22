"""Authored feedback controller for local US0 research, NOT original game logic.

Boot/menu input comes from the caller's authored castle schedule. Approximate
waypoints merely steer the reference game through doors and into a painting.
Only raw controller input is produced; no game memory is written. Do not use
this approximate navigation/camera policy in native simulation.
"""
from __future__ import annotations

import math

NAME = "bob-flat-v1"
NEUTRAL = {"buttons": 0, "stick_x": 0, "stick_y": 0}
FORWARD = {**NEUTRAL, "stick_y": 80}
START_POLL = 3879  # recording count; five neutral ticks before selected walk
WALK_POLL = 3883   # zero-based injection index

# Hand-authored route targets, not extracted collision or captured state fixtures.
OUTSIDE = ((0, 3000), (0, -6000))
INSIDE = ((-1800, 1000), (-2900, -100), (-2900, -800),
          (-3900, -460), (-4900, -460), (-5400, -460))


class BobRoute:
    def __init__(self, hold):
        if type(hold) is not int or not 1 <= hold <= 5:
            raise ValueError("BOB route hold must be in 1..5")
        self.hold = hold
        self.latest = None
        self.level = None
        self.waypoint = 0

    def observe(self, state, extra):
        # References are read-only; capture owns the snapshots.
        self.latest = state, extra

    def validate_completed(self, polls):
        if self.latest is None or polls <= WALK_POLL + self.hold:
            raise ValueError("BOB route incomplete; increase the bounded update/VI budget")
        state, extra = self.latest
        floor = extra.get("floor")
        if ((extra["level_num"], extra["area_index"], extra["act_num"]) != (9, 1, 1) or
                state["action"] != 0x0C400201 or state["forward_velocity"] != 0 or
                state["face_yaw"] != 24576 or not isinstance(floor, dict) or floor["type"] != 0):
            raise ValueError("BOB route did not finish resting on its ordinary flat lane")

    def input(self, poll, scheduled):
        raw = dict(scheduled)
        if poll < 2304 or self.latest is None:
            return raw
        state, extra = self.latest
        level = extra["level_num"]
        if level != self.level:
            self.level, self.waypoint = level, 0
        if level == 9:
            # Fixed timing belongs only to this pinned empty-save research route.
            # Traverse the spawn/ramp first, then rest on an ordinary flat floor.
            moving = 3768 <= poll < 3803 or WALK_POLL <= poll < WALK_POLL + self.hold
            raw = dict(FORWARD if moving else NEUTRAL)
            if poll >= START_POLL - 1:
                if (extra["area_index"], extra["act_num"]) != (1, 1):
                    raise ValueError("BOB route reached an unexpected area/act")
                if poll < WALK_POLL and (state["action"] != 0x0C400201 or
                                         state["forward_velocity"] != 0 or
                                         state["face_yaw"] != 24576):
                    raise ValueError("BOB route did not reach its resting diagonal start")
        elif level in (16, 6):
            if poll >= 3768:
                raise ValueError("BOB route did not reach BOB before movement deadline")
            targets = OUTSIDE if level == 16 else INSIDE
            x, z = targets[self.waypoint]
            dx, dz = x - state["position"][0], z - state["position"][2]
            if math.hypot(dx, dz) < 100 and self.waypoint + 1 < len(targets):
                self.waypoint += 1
                x, z = targets[self.waypoint]
                dx, dz = x - state["position"][0], z - state["position"][2]
            px, py, magnitude = extra["processed_stick"]
            relative = math.atan2(-px, py) if magnitude else 0
            camera = extra["intended_yaw"] * math.tau / 65536 - relative
            delta = math.atan2(dx, dz) - camera
            # An approximate controller inverse, not original stick/camera math.
            def axis(value):
                return round(value + 6 if value > 0 else value - 6 if value < 0 else 0)
            raw = dict(buttons=0, stick_x=axis(-74 * math.sin(delta)),
                       stick_y=axis(74 * math.cos(delta)))
            if level == 6 and state["position"][0] < -4500:
                raw["buttons"] = 32768 if poll % 32 < 4 else 0
        else:
            raise ValueError("unexpected scene in BOB route")
        if extra["dialog_id"] >= 0:
            raw = dict(buttons=32768 if poll % 24 < 4 else 0, stick_x=0, stick_y=0)
        return raw


def applied_schedule(events, polls):
    """Encode actual applied inputs, including the last neutral tail, for replay.

    Generated input is game-derived research output and must remain ignored.
    A replay uses this file as input, not feedback or reference output states.
    """
    segments = []
    for index, event in enumerate(events):
        end = events[index + 1]["poll"] if index + 1 < len(events) else polls
        segments.append(dict(polls=end - event["poll"], **event["input"]))
    return dict(version=1, segments=segments)
