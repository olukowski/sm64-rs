"""Build reference::Trace v1 from paired US0 update observations, without I/O.

These are assertion-based consistency checks, not original-hardware proof or
native-model verification. The caller must verify the capture profile, entry
address (US0 0x80254830), ROM identity and input-schedule binding first. Passing
an exploratory probe here does not establish that its boundary is verified.
VIs are diagnostic counters, never ticks. Minimal observations are not saves.
"""
from __future__ import annotations

from copy import deepcopy
import json
import math
import struct

US_SHA256 = "17ce077343c6133f8c9f2d6d6d9a4ab62c8cd2aa57c40aea1f490b4c8bb21d91"
STATE_ADDRESS = 0x8033B170
INPUT_SAMPLING = "Raw controller input at US0 entry 0x80254830; unchanged through caller return."
STATE_BOUNDARY = "MarioState immediately before US0 entry 0x80254830 and at caller return."
LIMITATIONS = ("Rice offscreen graphics, discarded audio, RSP-HLE; minimal observation is not save-state; "
               "third-party field metadata and runtime checks, no native model verified")


class Error(ValueError):
    """An observation fails the export contract."""


def _require(condition, message):
    if not condition:
        raise Error(message)


def _integer(value, low, high, label):
    _require(type(value) is int and low <= value <= high, f"invalid {label}")
    return value


def _mapping(value, label):
    _require(isinstance(value, dict), f"{label} must be an object")
    return value


def _input(value):
    _mapping(value, "input")
    _require(set(value) == {"buttons", "stick_x", "stick_y"}, "incomplete/unknown input fields")
    for key, low, high in (("buttons", 0, 65535), ("stick_x", -128, 127), ("stick_y", -128, 127)):
        _integer(value[key], low, high, f"input.{key}")


def _float_bits(value):
    _require(type(value) in (int, float), "state float must be numeric, not boolean")
    try:
        _require(math.isfinite(value), "nonfinite state float")
        bits = struct.pack(">f", value)
        _require(math.isfinite(struct.unpack(">f", bits)[0]), "state float overflows f32")
        return bits
    except (OverflowError, struct.error) as exc:
        raise Error("state float overflows f32") from exc


def _state(value):
    _mapping(value, "state")
    _require(set(value) == {"position", "velocity", "forward_velocity", "face_yaw", "action"},
             "incomplete/unknown MarioState fields")
    bits = []
    for key in ("position", "velocity"):
        _require(isinstance(value[key], list) and len(value[key]) == 3, f"{key} must have three components")
        bits.extend(_float_bits(v) for v in value[key])
    bits.append(_float_bits(value["forward_velocity"]))
    bits.append(_integer(value["face_yaw"], -32768, 32767, "face_yaw"))
    bits.append(_integer(value["action"], 0, 0xFFFFFFFF, "action"))
    return bits


def build_trace(observations, manifest, scenario, initialization, *, previous_input, start=0, count=None):
    """Return an independent trace after validating the *entire* observation chain.

    ``previous_input`` is the raw sample preceding observation zero, even when
    exporting a later slice. Profile/address and schedule verification belong to
    the caller; this function cannot promote exploratory evidence to verified.
    """
    for name, value in (("scenario", scenario), ("initialization", initialization)):
        _require(isinstance(value, str) and bool(value.strip()), f"{name} must be nonblank")
    _require(isinstance(observations, list), "observations must be an array")
    _integer(start, 0, len(observations), "start")
    if count is None:
        count = len(observations) - start
    _integer(count, 1, 10000, "frame count")
    _require(start + count <= len(observations), "slice exceeds observations")
    _mapping(manifest, "manifest")
    _input(previous_input)
    prior_input = previous_input
    prior_after = None
    prior_timer = prior_poll = prior_vi = return_pc = None
    for index, obs in enumerate(observations):
        _mapping(obs, f"observation {index}")
        required = {"before", "after", "input", "extra_before", "extra_after", "state_address",
                    "return_pc", "entry_poll", "return_poll", "entry_vi", "return_vi"}
        _require(required <= set(obs), f"incomplete observation {index}")
        before, after = _state(obs["before"]), _state(obs["after"])
        _input(obs["input"])
        _integer(obs["state_address"], STATE_ADDRESS, STATE_ADDRESS, "state_address")
        pc = _integer(obs["return_pc"], 0x80000000, 0x803FFFFC, "return_pc")
        _require(pc % 4 == 0 and pc != 0x80254830, "invalid caller return_pc")
        _require(return_pc is None or pc == return_pc, "changed return_pc")
        return_pc = pc
        edge = obs["input"]["buttons"] & (~prior_input["buttons"] & 0xFFFF)
        timers = []
        for key in ("extra_before", "extra_after"):
            extra = _mapping(obs[key], key)
            _require({"global_state_pointer", "global_timer", "button_pressed"} <= set(extra), f"incomplete {key}")
            _integer(extra["global_state_pointer"], STATE_ADDRESS, STATE_ADDRESS, "global_state_pointer")
            timers.append(_integer(extra["global_timer"], 0, 0xFFFFFFFF, "global_timer"))
            pressed = _integer(extra["button_pressed"], 0, 65535, "button_pressed")
            _require(pressed == edge, f"incorrect button_pressed in {key}")
        _require(timers[0] == timers[1], "global_timer changed during update")
        _require(prior_timer is None or timers[0] == prior_timer + 1, "global_timer not consecutive (or wrapped)")
        entry_poll = _integer(obs["entry_poll"], 1, 0xFFFFFFFFFFFFFFFF, "entry_poll")
        exit_poll = _integer(obs["return_poll"], 1, 0xFFFFFFFFFFFFFFFF, "return_poll")
        _require(entry_poll == exit_poll, "poll during update")
        _require(prior_poll is None or entry_poll == prior_poll + 1, "polls not consecutive")
        entry_vi = _integer(obs["entry_vi"], 0, 0xFFFFFFFFFFFFFFFF, "entry_vi")
        exit_vi = _integer(obs["return_vi"], 0, 0xFFFFFFFFFFFFFFFF, "return_vi")
        _require(entry_vi <= exit_vi and (prior_vi is None or prior_vi <= entry_vi), "nonmonotonic VIs")
        _require(prior_after is None or prior_after == before, "discontinuous observed MarioState")
        prior_input, prior_after = obs["input"], after
        prior_timer, prior_poll, prior_vi = timers[0], exit_poll, exit_vi

    settings = {"setup_manifest": deepcopy(manifest), "limitations": LIMITATIONS,
                "evidence": "Observed ROM execution, not equivalence with original hardware; assertion-based consistency validation.",
                "timing": "global_timer labels transitions; VIs are not ticks."}
    try:
        settings_json = json.dumps(settings, sort_keys=True, allow_nan=False)
    except (TypeError, ValueError, OverflowError) as exc:
        raise Error("manifest must be finite JSON metadata") from exc
    selected = observations[start:start + count]
    return deepcopy({
        "version": 1,
        "manifest": {"rom_sha256": US_SHA256, "source": "reference", "scenario": scenario,
                     "producer": "sm64-rs Mupen64Plus capture", "version": "1",
                     "settings": settings_json, "initialization": initialization,
                     "input_sampling": INPUT_SAMPLING, "state_boundary": STATE_BOUNDARY},
        "start_tick": selected[0]["extra_before"]["global_timer"],
        "initial_state": selected[0]["before"],
        "previous_input": previous_input if start == 0 else observations[start - 1]["input"],
        "frames": [{"tick": obs["extra_before"]["global_timer"], "input": obs["input"], "state": obs["after"]}
                   for obs in selected],
    })
