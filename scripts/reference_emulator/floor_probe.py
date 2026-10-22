"""Read-only, exploratory US0 find_floor call evidence; not a solver or Trace v1.

Only the pinned symbol map and fetched declaration identify the function. The
O32 register assignments below are candidates to corroborate at runtime, not
claims derived from inspecting the original implementation.
"""
from __future__ import annotations

import copy
import math
import struct

ENTRY = 0x80381900
RAM_START = 0x80000000
RAM_END = 0x80400000


def _ram(address: int, size: int, label: str):
    if (type(address) is not int or address % 4 or
            not RAM_START <= address <= RAM_END - size):
        raise ValueError(f"invalid {label}: expected aligned bounded RAM pointer")


def _float(bits: int) -> float:
    value = struct.unpack(">f", struct.pack(">I", bits))[0]
    if not math.isfinite(value):
        raise ValueError("non-finite floor-call float")
    return value


class FloorProbe:
    def __init__(self, core, max_calls=100000):
        if type(max_calls) is not int or not 1 <= max_calls <= 100000:
            raise ValueError("max_calls must be an integer in 1..100000")
        self.core = core
        self.max_calls = max_calls
        self.calls = []
        self.pending = None
        self._entry_breakpoint = None

    def install(self):
        """Install from the emulator's debug-init callback, never before it."""
        if self._entry_breakpoint is not None:
            raise RuntimeError("floor probe already installed")
        self._entry_breakpoint = self.core.breakpoint(ENTRY)

    def handle(self, pc, context_factory, *, enabled):
        if pc == ENTRY:
            if not enabled:
                return True
            if self.pending is not None:
                raise RuntimeError("nested floor call while return pending")
            if len(self.calls) >= self.max_calls:
                raise RuntimeError("floor max_calls limit reached (not sampling)")
            return_pc = self.core.register(31) & 0xFFFFFFFF
            _ram(return_pc, 4, "return PC")
            if return_pc == ENTRY:
                raise ValueError("invalid return PC: floor entry")
            bits = [self.core.fpr_bits(12), self.core.fpr_bits(14),
                    self.core.register(6) & 0xFFFFFFFF]
            values = [_float(value) for value in bits]
            output_address = self.core.register(7) & 0xFFFFFFFF
            _ram(output_address, 4, "output pointer")
            context = copy.deepcopy(context_factory())
            breakpoint = self.core.breakpoint(return_pc)
            self.pending = dict(context_before=context, input_bits=bits,
                                input_values=values, output_address=output_address,
                                return_pc=return_pc, breakpoint=breakpoint)
            return True
        if self.pending is None or pc != self.pending["return_pc"]:
            return False
        pending = self.pending
        self.core.remove_breakpoint(pending["breakpoint"])
        height_bits = self.core.fpr_bits(0)
        height = _float(height_bits)
        address = self.core.u32(pending["output_address"])
        surface = None
        if address != 0:
            _ram(address, 0x30, "returned surface pointer")
            surface = {
                "address": address,
                "type": self.core.i16(address),
                "vertices": [[self.core.i16(address + 0xA + vertex * 6 + axis * 2)
                              for axis in range(3)] for vertex in range(3)],
                "normal_bits": [self.core.u32(address + offset)
                                for offset in (0x1C, 0x20, 0x24)],
                "origin_offset_bits": self.core.u32(address + 0x28),
            }
        record = {key: value for key, value in pending.items() if key != "breakpoint"}
        record.update(context_after=copy.deepcopy(context_factory()),
                      surface=surface, height_bits=height_bits, height=height)
        self.calls.append(record)
        self.pending = None
        return True

    def report(self):
        if self.pending is not None:
            raise RuntimeError("incomplete floor call: return pending")
        return copy.deepcopy({
            "version": 1,
            "status": "exploratory_floor_calls",
            "entry": ENTRY,
            "max_calls": self.max_calls,
            "memory_write": False,
            "abi": {
                "status": "candidate O32 ABI; requires runtime corroboration",
                "input_bits": ["FPR12", "FPR14", "low32 GPR6"],
                "output_pointer": "low32 GPR7 (Surface **)",
                "height_bits": "FPR0",
                "caller_return": "low32 GPR31",
                "symbol_map": "https://github.com/SM64-TAS-ABC/STROOP/blob/6771197ad0f50718d3d4a13387e01d1e6d9c0d1a/STROOP/Mappings/MappingUS.map",
                "declaration": "https://github.com/n64decomp/sm64/blob/9921382a68bb0c865e5e45eb594d9c64db59b1af/src/engine/surface_collision.h",
                "prototype": "float find_floor(float x, float y, float z, Surface **)",
            },
            "calls": self.calls,
        })
