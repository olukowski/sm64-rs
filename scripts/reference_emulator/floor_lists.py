"""Bounded read-only partition snapshots, not observed solver traversal.

US0 addresses: pinned STROOP MappingUS.map. Layout: pinned include/types.h
and src/engine/surface_load.h declarations; see docs/research/floor-queries.md.
Only category zero (declared floors) is recorded, for ALL 16x16 index pairs in
both partitions. No coordinate-to-cell formula or sentinel semantics is assumed.
"""
from __future__ import annotations

import copy
import hashlib
import json

from .floor_probe import _ram

PARTITIONS = (("static", 0x8038BE98), ("dynamic", 0x8038D698))
CELLS = 16
CELL_BYTES = 24
MAX_NODES = 10000
MAX_LINKS = 100000
MAX_READS = 128000000
MAX_STORED_BYTES = 32 * 1024 * 1024
MAX_SNAPSHOTS = 1024


def read_surface(core, address):
    """Raw declared fields; classification/eligibility is deliberately absent."""
    _ram(address, 0x30, "partition surface pointer")
    return {
        "address": address,
        "type": core.i16(address),
        "force": core.i16(address + 2),
        "flags_room_lower_y_bits": core.u32(address + 4),
        "upper_y": core.i16(address + 8),
        "vertices": [[core.i16(address + 0xA + vertex * 6 + axis * 2)
                      for axis in range(3)] for vertex in range(3)],
        "normal_bits": [core.u32(address + offset) for offset in (0x1C, 0x20, 0x24)],
        "origin_offset_bits": core.u32(address + 0x28),
        "object_address_bits": core.u32(address + 0x2C),
    }


class FloorLists:
    """Every requested snapshot is read afresh; only storage is deduplicated.

    The fixed budgets are tooling limits, not inferred game capacities. A budget
    failure aborts acquisition instead of quietly sampling/truncating evidence.
    """
    def __init__(self, core):
        self.core = core
        self.reads = 0
        self.stored_bytes = 0
        self.snapshots = []
        self._ids = {}

    def _read(self, method, address):
        if self.reads >= MAX_READS:
            raise RuntimeError("floor-list read budget exceeded (not sampling)")
        self.reads += 1
        return getattr(self.core, method)(address)

    def u32(self, address):
        return self._read("u32", address)

    def i16(self, address):
        return self._read("i16", address)

    def snapshot(self):
        heads, nodes, surfaces = {}, {}, {}
        links = 0
        for name, base in PARTITIONS:
            partition = []
            for outer in range(CELLS):
                for inner in range(CELLS):
                    address = base + (outer * CELLS + inner) * CELL_BYTES
                    # Both embedded-head words are retained, even though the
                    # second may not be a live surface. Only next is followed.
                    pointer, raw_surface = self.u32(address), self.u32(address + 4)
                    partition.append([pointer, raw_surface])
                    seen = set()
                    while pointer:
                        if links >= MAX_LINKS:
                            raise RuntimeError("floor-list link budget exceeded (not sampling)")
                        links += 1
                        _ram(pointer, 8, "partition node pointer")
                        if pointer in seen:
                            raise ValueError("cycle in floor-list next chain")
                        seen.add(pointer)
                        if pointer not in nodes:
                            if len(nodes) >= MAX_NODES:
                                raise RuntimeError("floor-list node budget exceeded (not sampling)")
                            next_pointer, surface = self.u32(pointer), self.u32(pointer + 4)
                            if not surface:
                                raise ValueError("null partition node surface")
                            if surface not in surfaces:
                                surfaces[surface] = read_surface(self, surface)
                            nodes[pointer] = [next_pointer, surface]
                        pointer = nodes[pointer][0]
            heads[name] = partition
        snapshot = {"heads": heads, "nodes": nodes, "surfaces": surfaces}
        encoded = json.dumps(snapshot, sort_keys=True, separators=(",", ":"),
                             allow_nan=False).encode()
        digest = hashlib.sha256(encoded).hexdigest()
        if digest in self._ids:
            return self._ids[digest]
        if len(self.snapshots) >= MAX_SNAPSHOTS or self.stored_bytes + len(encoded) > MAX_STORED_BYTES:
            raise RuntimeError("floor-list storage budget exceeded (not sampling)")
        index = len(self.snapshots)
        self._ids[digest] = index
        self.stored_bytes += len(encoded)
        self.snapshots.append({"sha256": digest, **snapshot})
        return index

    def report(self):
        return copy.deepcopy({
            "version": 1, "status": "exploratory_floor_list_memory",
            "memory_write": False,
            "scope": "All category-zero embedded heads; both words retained, next chains followed. "
                     "Memory-linked order is NOT observed resolver traversal or eligibility. "
                     "All heads/nodes/surfaces reread at each boundary; identical storage deduplicated.",
            "layout": {"partitions": dict(PARTITIONS), "cells": CELLS,
                       "cell_bytes": CELL_BYTES, "category": 0, "node_bytes": 8,
                       "head_order": "outer index then inner index; axis orientation unresolved"},
            "budgets": {"max_nodes_per_snapshot": MAX_NODES, "max_reads": MAX_READS,
                        "max_links_per_snapshot": MAX_LINKS,
                        "max_stored_bytes": MAX_STORED_BYTES, "max_snapshots": MAX_SNAPSHOTS},
            "reads": self.reads, "stored_bytes": self.stored_bytes,
            "snapshots": self.snapshots,
        })
