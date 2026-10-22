#!/usr/bin/env python3
"""Bounded offscreen US0 reference probes; no native gameplay implementation.

A probe at video interrupts is NOT a game-tick trace. Action observations require
an explicit, independently verified entry address, paired with its caller return.
All emulator configuration, saves, reports and traces stay in a NEW ignored run.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import resource
import shutil
import signal
import subprocess
import sys

if __package__ in (None, ""):
    sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from scripts.reference_emulator import core as api
from scripts.reference_emulator import bob_route, floor_lists, floor_probe, setup, trace_export

US_SHA256 = "17ce077343c6133f8c9f2d6d6d9a4ab62c8cd2aa57c40aea1f490b4c8bb21d91"
MAX_ROM = 64 * 1024 * 1024
MAX_JSON = 1024 * 1024
# Layout metadata only: n64decomp/sm64 include/types.h, MarioState/Controller.
# Every use remains a candidate field interpretation until runtime corroboration.
OFFSETS = {"position": 0x3C, "velocity": 0x48, "forward_velocity": 0x54,
           "face_yaw": 0x2E, "action": 0x0C, "controller": 0x9C}


def json_read(path: Path, limit=MAX_JSON):
    with path.open("rb") as source:
        data = source.read(limit + 1)
    if len(data) > limit:
        raise ValueError(f"oversized JSON: {path}")
    def unique(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise ValueError(f"duplicate JSON key: {key}")
            result[key] = value
        return result
    return json.loads(data, object_pairs_hook=unique,
                      parse_constant=lambda value: (_ for _ in ()).throw(ValueError(value)))


def normalize_rom(data: bytes) -> bytes:
    if len(data) > MAX_ROM or len(data) < 64 or len(data) % 4:
        raise ValueError("ROM is oversized or not word-aligned")
    magic = data[:4]
    if magic == b"\x37\x80\x40\x12":
        normalized = bytearray(data)
        normalized[0::2], normalized[1::2] = data[1::2], data[0::2]
        data = bytes(normalized)
    elif magic == b"\x40\x12\x37\x80":
        normalized = bytearray(data)
        for i in range(4):
            normalized[i::4] = data[3-i::4]
        data = bytes(normalized)
    elif magic != b"\x80\x37\x12\x40":
        raise ValueError("unrecognized ROM byte order")
    return data


def rom_bytes(path: Path) -> bytes:
    with path.open("rb") as source:
        data = normalize_rom(source.read(MAX_ROM + 1))
    if len(data) != 8 * 1024 * 1024 or hashlib.sha256(data).hexdigest() != US_SHA256:
        raise ValueError("capture requires exact canonical US revision 0 SHA-256")
    return data


def schedule_read(path: Path | None):
    if path is None:
        return []
    document = json_read(path)
    if not isinstance(document, dict) or set(document) != {"version", "segments"} or type(document["version"]) is not int or document["version"] != 1:
        raise ValueError("unsupported input schedule")
    segments = document["segments"]
    if not isinstance(segments, list) or len(segments) > 10000:
        raise ValueError("input schedule must be a bounded segment array")
    result = []
    end = 0
    for segment in segments:
        if not isinstance(segment, dict) or set(segment) != {"polls", "buttons", "stick_x", "stick_y"}:
            raise ValueError("input segment has missing/unknown fields")
        for name, low, high in [("polls", 1, 100000), ("buttons", 0, 65535),
                                ("stick_x", -128, 127), ("stick_y", -128, 127)]:
            if type(segment[name]) is not int or not low <= segment[name] <= high:
                raise ValueError(f"invalid schedule {name}")
        end += segment["polls"]
        if end > 100000:
            raise ValueError("schedule exceeds 100000 controller polls")
        result.append((end, {key: segment[key] for key in ("buttons", "stick_x", "stick_y")}))
    return result


def input_at(segments, poll: int):
    if poll < 0:
        # PluginStartup initializes neutral input before the first hardware poll.
        return {"buttons": 0, "stick_x": 0, "stick_y": 0}
    for end, raw in segments:
        if poll < end:
            return raw
    return {"buttons": 0, "stick_x": 0, "stick_y": 0}


def checked_manifest(path: Path):
    setup.checked_path(path)
    manifest = json_read(path)
    if manifest.get("owner") != setup.OWNER or manifest.get("schema_version") != 1:
        raise ValueError("unrecognized emulator manifest")
    for name in ("core", "rsp", "input", "video"):
        artifact = manifest["artifacts"][name]
        if setup.sha256(Path(artifact["path"])) != artifact["sha256"]:
            raise ValueError(f"{name} artifact no longer matches manifest")
    setup.checked_path(Path(manifest["core_data"]))
    for name, artifact in manifest["data_files"].items():
        if Path(name).name != name or name in (".", ".."):
            raise ValueError("invalid data filename")
        if setup.sha256(Path(artifact["path"])) != artifact["sha256"]:
            raise ValueError(f"{name} data file no longer matches manifest")
    return manifest


def write_new(path: Path, data):
    setup.checked_path(path)
    # JSON is validated/encoded before any output is created. Never follows links.
    encoded = json.dumps(data, indent=2, allow_nan=False).encode() + b"\n"
    with path.open("xb") as target:
        target.write(encoded)


def ram_address(address, size=1):
    return 0x80000000 <= address and address + size <= 0x80400000


def diagnostics(core, pointer):
    if not ram_address(pointer, 0xC8) or pointer % 4:
        raise ValueError("invalid diagnostic state pointer")
    controller_address = core.u32(pointer + 0x9C)
    if not ram_address(controller_address, 0x14) or controller_address % 4:
        raise ValueError("invalid diagnostic controller pointer")
    # Declaration-only offsets: pinned include/types.h linked in the docs.
    # These diagnostics do not extend Trace v1 or claim save-state coverage.
    floor = core.u32(pointer + 0x68)
    surface = None
    if ram_address(floor, 0x30) and floor % 4 == 0:
        surface = {"address": floor, "type": core.i16(floor),
                   "normal": [core.f32(floor + 0x1C + 4*i) for i in range(3)],
                   "vertices": [[core.i16(floor + 0x0A + 6*i + 2*j)
                                 for j in range(3)] for i in range(3)]}
    return {"floor": surface, "floor_height": core.f32(pointer + 0x70),
            "water_level": core.i16(pointer + 0x76),
            "quicksand_depth": core.f32(pointer + 0xC0),
            "held_object": core.u32(pointer + 0x7C),
            "ridden_object": core.u32(pointer + 0x84),
            "squish_timer": int(core.lib.DebugMemRead8(pointer + 0xB4)),
            "ceiling_height": core.f32(pointer + 0x6C),
            "wall": core.u32(pointer + 0x60),
            "intended_magnitude": core.f32(pointer + 0x20),
            "intended_yaw": core.i16(pointer + 0x24),
            "processed_stick": [core.f32(controller_address + offset) for offset in (4, 8, 12)],
            "action_state": int(core.lib.DebugMemRead16(pointer + 0x18)),
            "action_timer": int(core.lib.DebugMemRead16(pointer + 0x1A)),
            "global_state_pointer": core.u32(0x8032D93C),
            "global_timer": core.u32(0x8032D5D4),
            # STROOP's pinned US map: scene identity, not native initialization.
            "level_num": core.i16(0x8032DDF8),
            "area_index": core.i16(0x8033BACA),
            "act_num": core.i16(0x8033BAC8),
            "dialog_id": core.i16(0x80331484),
            "dialog_box_state_byte": int(core.lib.DebugMemRead8(0x80331470)),
            "button_pressed": int(core.lib.DebugMemRead16(core.u32(pointer + 0x9C) + 0x12))}


def state(core, address):
    if not ram_address(address, 0xC8) or address % 4:
        raise ValueError(f"invalid candidate MarioState pointer {address:#x}")
    result = {
        "position": [core.f32(address + OFFSETS["position"] + i*4) for i in range(3)],
        "velocity": [core.f32(address + OFFSETS["velocity"] + i*4) for i in range(3)],
        "forward_velocity": core.f32(address + OFFSETS["forward_velocity"]),
        "face_yaw": core.i16(address + OFFSETS["face_yaw"]),
        "action": core.u32(address + OFFSETS["action"]),
    }
    if not all(math.isfinite(v) for v in result["position"] + result["velocity"] + [result["forward_velocity"]]):
        raise ValueError("nonfinite candidate state")
    return result


def controller(core, address):
    pointer = core.u32(address + OFFSETS["controller"])
    if not ram_address(pointer, 0x1C) or pointer % 4:
        raise ValueError(f"invalid controller pointer {pointer:#x}")
    raw = {"buttons": int(core.lib.DebugMemRead16(pointer + 0x10)),
           "stick_x": core.i16(pointer), "stick_y": core.i16(pointer + 2)}
    if not all(-128 <= raw[key] <= 127 for key in ("stick_x", "stick_y")):
        raise ValueError("candidate raw stick fields exceed signed-byte range")
    return raw


def worker(args):
    # No core dumps containing game memory; the parent additionally bounds time.
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    resource.setrlimit(resource.RLIMIT_AS, (8 * 1024**3, 8 * 1024**3))
    resource.setrlimit(resource.RLIMIT_CPU, (args.timeout, args.timeout + 1))
    manifest = checked_manifest(args.manifest)
    rom = rom_bytes(args.rom)
    segments = schedule_read(args.schedule)
    policy = bob_route.BobRoute(args.policy_hold) if args.input_policy else None
    artifacts = manifest["artifacts"]
    core = api.Core(*(Path(artifacts[name]["path"]) for name in ("core", "rsp", "input", "video")))
    lists = floor_lists.FloorLists(core) if args.observe_floor_lists else None
    observer = floor_probe.FloorProbe(core, args.floor_max_calls, lists=lists) if args.observe_floor else None
    output = args.out
    data = output / "data"
    data.mkdir()
    for name, artifact in manifest["data_files"].items():
        shutil.copyfile(artifact["path"], data / name)
    counts = {"vi": 0, "entries": 0, "returns": 0, "controller_polls": 0}
    events = []
    snapshots = []
    pending = None
    entry_breakpoint = None
    last_input = None
    logs = []

    def logger(level, message):
        if level <= 2 and len(logs) < 100:
            print(f"emulator[{level}]: {message}", file=sys.stderr, flush=True)
            logs.append({"level": level, "message": message})

    def initialize():
        nonlocal entry_breakpoint
        print("debugger initialized", file=sys.stderr, flush=True)
        if args.entry is not None:
            entry_breakpoint = core.breakpoint(args.entry)
        if observer is not None:
            observer.install()
        core.resume()

    def poll(index):
        nonlocal last_input
        raw = input_at(segments, index)
        if policy is not None:
            raw = policy.input(index, raw)
        if raw != last_input:
            if len(events) >= 10001:
                raise ValueError("too many input events")
            events.append({"poll": index, "vi": counts["vi"], "input": raw})
            last_input = raw
        core.bridge.CaptureSetInput(raw["buttons"], raw["stick_x"], raw["stick_y"])
        counts["controller_polls"] = index + 1

    def extra(pointer):
        return diagnostics(core, pointer)

    def floor_context():
        return {"global_timer": core.u32(0x8032D5D4),
                "level_num": core.i16(0x8032DDF8), "area_index": core.i16(0x8033BACA),
                "act_num": core.i16(0x8033BAC8), "controller_polls": counts["controller_polls"],
                "vi": counts["vi"], "action_entries": counts["entries"],
                "inside_action": pending is not None}

    def update(pc):
        nonlocal pending
        if observer is not None and observer.handle(
                pc, floor_context, enabled=counts["controller_polls"] >= args.record_from_poll):
            # A tail caller could share the action's return PC. Observe both
            # boundaries in that case instead of dropping the outer return.
            if pc != args.entry and not (pending is not None and pc == pending["return_pc"]):
                core.resume()
                return
        if pc == args.entry:
            if counts["controller_polls"] < args.record_from_poll:
                core.resume()
                return
            if pending is not None:
                raise ValueError("nested entry: chosen boundary is not one update per call")
            counts["entries"] += 1
            pointer = args.state_address if args.state_address is not None else core.register(4) & 0xFFFFFFFF
            return_pc = core.register(31) & 0xFFFFFFFF
            if not ram_address(return_pc, 4) or return_pc % 4 or return_pc == args.entry:
                raise ValueError("invalid update return address")
            initial = state(core, pointer)
            raw = controller(core, pointer)
            bp = core.breakpoint(return_pc)
            pending = {"state_address": pointer, "return_pc": return_pc, "breakpoint": bp,
                       "before": initial, "extra_before": extra(pointer), "input": raw, "entry_vi": counts["vi"],
                       "entry_poll": counts["controller_polls"]}
        elif pending is not None and pc == pending["return_pc"]:
            core.remove_breakpoint(pending["breakpoint"])
            post = state(core, pending["state_address"])
            snapshots.append({**{k: v for k,v in pending.items() if k != "breakpoint"},
                              "after": post, "extra_after": extra(pending["state_address"]), "return_vi": counts["vi"],
                              "return_poll": counts["controller_polls"]})
            if policy is not None:
                policy.observe(post, snapshots[-1]["extra_after"])
            counts["returns"] += 1
            pending = None
            if counts["returns"] >= args.updates:
                core.stop()
        else:
            raise ValueError(f"unexpected breakpoint PC {pc:#x}")
        core.resume()

    def vi():
        counts["vi"] += 1
        if counts["vi"] == 1:
            # CoreStartup's debugger init precedes EMU_RUNNING; this command is
            # valid only after execution has actually started.
            core.command(17, 5, api.C.byref(api.C.c_int(0)))
        if counts["vi"] % 120 == 0:
            print(f"progress: {counts}", file=sys.stderr, flush=True)
        if args.screenshot and counts["vi"] == max(1, args.max_vis - 120):
            core.command(16)  # screenshot after next real renderer callback
        if counts["vi"] >= args.max_vis:
            core.stop()

    try:
        core.startup(output / "config", data, rom, logger)
        core.callbacks(initialize, update, vi, poll)
        core.execute()
    finally:
        core.close()
    generated = bob_route.applied_schedule(events, counts["controller_polls"]) if policy is not None else None
    if generated is not None:
        end, segments = 0, []
        for segment in generated["segments"]:
            end += segment["polls"]
            segments.append((end, {key: segment[key] for key in ("buttons", "stick_x", "stick_y")}))
    report = {
        "version": 1, "status": "exploratory_boundary_probe_not_validated_game_trace",
        "rom_sha256": US_SHA256, "entry": args.entry, "counts": counts,
        "record_from_poll": args.record_from_poll,
        "input_policy": {"name": args.input_policy, "hold": args.policy_hold} if policy is not None else None,
        "input_schedule": [{"end_poll": end, "input": raw} for end, raw in segments],
        "emulator": manifest, "input_events": events, "observations": snapshots,
        "warnings": logs,
        "limitations": "Rice graphics, discarded audio output and RSP-HLE; entry/field interpretations require corroboration. Controller polls and VIs are not assumed to be game ticks. No memory writes, cheats, save-state loads or ROM patches used.",
    }
    if observer is not None:
        floor_report = observer.report()
        floor_report.update(rom_sha256=US_SHA256, emulator=manifest,
                            record_from_poll=args.record_from_poll)
        if lists is not None:
            list_report = lists.report()
            list_report.update(rom_sha256=US_SHA256, emulator=manifest,
                               record_from_poll=args.record_from_poll)
            write_new(output / "floor-lists.json", list_report)
            floor_report["list_report"] = "floor-lists.json"
        write_new(output / "floor-queries.json", floor_report)
        report["floor_probe"] = {"file": "floor-queries.json", "calls": len(observer.calls)}
    write_new(output / "probe.json", report)
    if args.entry is not None and counts["returns"] != args.updates:
        raise ValueError(f"only {counts['returns']} of {args.updates} requested boundaries reached; inspect probe.json")
    if generated is not None:
        policy.validate_completed(counts["controller_polls"])
        write_new(output / "inputs.json", generated)
    if args.export_trace:
        for snapshot in snapshots:
            if snapshot["input"] != input_at(segments, snapshot["entry_poll"] - 1):
                raise ValueError("game-visible input differs from applied controller schedule")
        previous = input_at(segments, snapshots[0]["entry_poll"] - 2)
        schedule_hash = hashlib.sha256(json.dumps(segments, sort_keys=True).encode()).hexdigest()
        trace = trace_export.build_trace(
            snapshots, manifest, args.scenario,
            f"Cold boot with empty private EEPROM/paks; no state load or game-memory writes; "
            f"input schedule SHA256 {schedule_hash}; recording began at controller-poll count "
            f"{args.record_from_poll}; {args.trace_start} preceding recorded action calls. "
            "Initial observation is not a complete save-state.",
            previous_input=previous, start=args.trace_start)
        write_new(output / "trace.json", trace)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=Path, default=setup.CACHE / "emulator/manifest.json")
    parser.add_argument("--rom", type=Path, default=os.environ.get("SM64_ROM"))
    parser.add_argument("--out", type=Path, required=True, help="NEW directory below .cache")
    parser.add_argument("--schedule", type=Path, help="authored raw input segments indexed by controller poll")
    parser.add_argument("--input-policy", choices=[bob_route.NAME],
                        help="approximate research-only BOB controller; emits ignored inputs.json for independent replay")
    parser.add_argument("--policy-hold", type=int, default=1, help="BOB policy's final full-forward hold, 1..5 ticks")
    parser.add_argument("--profile", choices=["us0-action"], help="documented US0 execute_mario_action boundary and state address")
    parser.add_argument("--entry", type=lambda v: int(v, 0), help="exploratory entry breakpoint; a0 is used only without --state-address")
    parser.add_argument("--state-address", type=lambda v: int(v, 0), help="explicit candidate MarioState address when entry does not take a0")
    parser.add_argument("--updates", type=int, default=120)
    parser.add_argument("--record-from-poll", type=int, default=0,
                        help="record action calls only once the controller-poll count reaches this value; boot inputs still run")
    parser.add_argument("--observe-floor", action="store_true",
                        help="record exploratory US0 find_floor calls separately during a post-boot action capture")
    parser.add_argument("--floor-max-calls", type=int, default=100000,
                        help="fail, rather than sample, beyond this many floor calls (1..100000)")
    parser.add_argument("--observe-floor-lists", action="store_true",
                        help="snapshot all static/dynamic floor-head next chains at every floor entry/return")
    parser.add_argument("--max-vis", type=int, default=4000)
    parser.add_argument("--timeout", type=int, default=120)
    parser.add_argument("--export-trace", action="store_true", help="export Trace v1 only for the checked US0 action profile")
    parser.add_argument("--trace-start", type=int, default=0, help="skip this many captured updates in exported trace")
    parser.add_argument("--scenario", default="cold-boot-action-observations", help="stable comparison protocol scenario label")
    parser.add_argument("--screenshot", action="store_true", help="diagnostic screenshot near VI limit, stays ignored in run config")
    parser.add_argument("--worker", action="store_true", help=argparse.SUPPRESS)
    args = parser.parse_args()
    try:
        if args.rom is None:
            raise ValueError("select --rom or SM64_ROM; no ROM is downloaded")
        if args.profile:
            if args.entry is not None or args.state_address is not None:
                raise ValueError("profile cannot be combined with custom entry/state addresses")
            args.entry, args.state_address = 0x80254830, 0x8033B170
        if args.observe_floor_lists and not args.observe_floor:
            raise ValueError("observe-floor-lists requires observe-floor")
        if args.observe_floor:
            if (args.entry, args.state_address) != (0x80254830, 0x8033B170) or not args.record_from_poll:
                raise ValueError("floor observation requires the US0 action profile and a positive recording gate")
            if not 1 <= args.floor_max_calls <= 100000:
                raise ValueError("floor-max-calls must be in 1..100000")
        elif args.floor_max_calls != 100000:
            raise ValueError("floor-max-calls requires observe-floor")
        if args.input_policy:
            if (args.entry, args.state_address) != (0x80254830, 0x8033B170) or args.schedule is None:
                raise ValueError("BOB input policy requires the US0 action profile and authored boot schedule")
            if args.export_trace or args.record_from_poll:
                raise ValueError("BOB input policy requires full recording and no trace export; replay generated inputs separately")
            if not 1 <= args.policy_hold <= 5:
                raise ValueError("BOB policy hold must be in 1..5")
        elif args.policy_hold != 1:
            raise ValueError("policy-hold requires an input policy")
        if args.export_trace and (args.entry != 0x80254830 or args.state_address != 0x8033B170):
            raise ValueError("trace export requires the documented US0 action profile")
        if args.export_trace and (not args.scenario.strip() or not 0 <= args.trace_start < args.updates):
            raise ValueError("trace export needs a nonblank scenario and valid start update")
        if not 0 <= args.record_from_poll <= 100000:
            raise ValueError("record-from-poll must be in 0..100000")
        if args.record_from_poll and args.entry is None:
            raise ValueError("record-from-poll requires an action entry")
        if not 1 <= args.updates <= 10000 or not 1 <= args.max_vis <= 100000 or not 1 <= args.timeout <= 600:
            raise ValueError("invalid update/VI/time bound")
        if args.entry is not None and (not ram_address(args.entry, 4) or args.entry % 4):
            raise ValueError("entry must be an aligned US0 RDRAM execution address")
        if args.state_address is not None and (args.entry is None or not ram_address(args.state_address, 0xC8) or args.state_address % 4):
            raise ValueError("state-address requires an entry and aligned RDRAM state")
        args.out = setup.checked_path(args.out)
        args.manifest = setup.checked_path(args.manifest)
        args.rom = args.rom.resolve(strict=True)
        if args.schedule:
            args.schedule = args.schedule.resolve(strict=True)
        if args.worker:
            worker(args)
            return
        checked_manifest(args.manifest)
        rom_bytes(args.rom)
        schedule_read(args.schedule)
        args.out.mkdir(parents=True)  # no exist_ok; never reuse configuration/saves
        (args.out / "home").mkdir()
        environment = {**os.environ, "HOME": str(args.out / "home"),
                       "XDG_CONFIG_HOME": str(args.out / "home/config"),
                       "XDG_DATA_HOME": str(args.out / "home/data"),
                       "SDL_VIDEODRIVER": "offscreen", "SDL_AUDIODRIVER": "dummy"}
        command = [sys.executable, str(Path(__file__).resolve()), "--worker", "--manifest", str(args.manifest),
                   "--rom", str(args.rom), "--out", str(args.out), "--updates", str(args.updates),
                   "--max-vis", str(args.max_vis), "--timeout", str(args.timeout),
                   "--record-from-poll", str(args.record_from_poll)]
        if args.observe_floor:
            command += ["--observe-floor", "--floor-max-calls", str(args.floor_max_calls)]
        if args.observe_floor_lists:
            command += ["--observe-floor-lists"]
        if args.input_policy:
            command += ["--input-policy", args.input_policy, "--policy-hold", str(args.policy_hold)]
        if args.export_trace:
            command += ["--export-trace", "--trace-start", str(args.trace_start), "--scenario", args.scenario]
        if args.screenshot:
            command += ["--screenshot"]
        if args.entry is not None:
            command += ["--entry", hex(args.entry)]
        if args.state_address is not None:
            command += ["--state-address", hex(args.state_address)]
        if args.schedule:
            command += ["--schedule", str(args.schedule)]
        with (args.out / "worker.log").open("xb") as log:
            process = subprocess.Popen(command, stdout=log, stderr=subprocess.STDOUT,
                                       env=environment, cwd=setup.PROJECT, start_new_session=True)
            try:
                status = process.wait(timeout=args.timeout + 5)
            except BaseException:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait()
                raise
        if status:
            raise ValueError(f"capture exited {status}; see {args.out / 'worker.log'}")
        print(args.out / "probe.json")
    except (OSError, ValueError, RuntimeError, KeyError, TypeError, subprocess.SubprocessError) as error:
        parser.exit(1, f"capture: {error}\n")


if __name__ == "__main__":
    main()
