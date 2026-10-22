"""Invented RAM layouts only; never loads game bytes or an emulator."""
import json
import unittest
from unittest.mock import Mock, patch

from scripts.reference_emulator import floor_lists
from scripts.reference_emulator.floor_probe import ENTRY, FloorProbe


class Memory:
    def __init__(self):
        self.words = {}
        self.reads = []

    def u32(self, address):
        self.reads.append(address)
        return self.words.get(address, 0)

    def i16(self, address):
        self.reads.append(address)
        # Authored coordinates/metadata, not an extracted surface.
        return -(address & 0x7FFF)


class FloorListsTests(unittest.TestCase):
    def setUp(self):
        self.memory = Memory()
        self.observer = floor_lists.FloorLists(self.memory)
        self.base = floor_lists.PARTITIONS[0][1]
        self.node, self.surface = 0x80001000, 0x80002000

    def chain(self):
        self.memory.words.update({self.base: self.node, self.base + 4: 0xDEADBEEF,
                                  self.node: 0, self.node + 4: self.surface})

    def test_all_heads_axis_agnostic_strides_and_raw_words(self):
        self.chain()
        head = self.base + (2 * 16 + 5) * 24
        self.memory.words[head] = self.node
        self.memory.words[floor_lists.PARTITIONS[1][1]] = self.node
        self.assertEqual(self.observer.snapshot(), 0)
        report = self.observer.report()
        snapshot = report["snapshots"][0]
        self.assertEqual(len(snapshot["heads"]["static"]), 256)
        self.assertEqual(len(snapshot["heads"]["dynamic"]), 256)
        self.assertEqual(snapshot["heads"]["static"][0], [self.node, 0xDEADBEEF])
        self.assertEqual(snapshot["heads"]["static"][37], [self.node, 0])
        self.assertEqual(snapshot["heads"]["dynamic"][0], [self.node, 0])
        self.assertEqual(snapshot["nodes"], {self.node: [0, self.surface]})
        self.assertEqual(len(snapshot["surfaces"]), 1)
        surface = snapshot["surfaces"][self.surface]
        self.assertEqual(surface["vertices"], [[-8202, -8204, -8206],
                                              [-8208, -8210, -8212],
                                              [-8214, -8216, -8218]])
        self.assertEqual(surface["upper_y"], -8200)
        self.assertIn("NOT observed", report["scope"])
        self.assertFalse(report["memory_write"])
        json.dumps(report, allow_nan=False)
        report["snapshots"].clear()
        self.assertEqual(len(self.observer.snapshots), 1)
        self.assertNotIn(0xDEADBEEF, self.memory.reads)
        self.assertNotIn(self.base + 8, self.memory.reads)  # not ceilings
        self.assertNotIn(self.base + 16, self.memory.reads)  # not walls

    def test_storage_deduplication_does_not_cache_memory_reads(self):
        self.chain()
        self.assertEqual(self.observer.snapshot(), 0)
        reads, stored = self.observer.reads, self.observer.stored_bytes
        self.assertEqual(self.observer.snapshot(), 0)
        self.assertEqual(self.observer.reads, reads * 2)
        self.assertEqual(self.observer.stored_bytes, stored)
        self.memory.words[self.surface + 0x28] = 0x3F800000
        self.assertEqual(self.observer.snapshot(), 1)
        self.memory.words[self.surface + 0x28] = 0
        self.assertEqual(self.observer.snapshot(), 0)
        self.assertEqual(len(self.observer.snapshots), 2)

    def test_next_order_preserved_without_sorting(self):
        self.chain()
        next_node = self.node - 8
        self.memory.words[self.node] = next_node
        self.memory.words.update({next_node: 0, next_node + 4: self.surface})
        self.observer.snapshot()
        self.assertEqual(self.observer.snapshots[0]["nodes"], {
            self.node: [next_node, self.surface], next_node: [0, self.surface]})

    def test_cycles_self_and_long_and_shared_tail(self):
        self.chain()
        for next_pointer in (self.node, self.node + 8):
            self.memory.words[self.node] = next_pointer
            self.memory.words.update({self.node + 8: self.node, self.node + 12: self.surface})
            with self.assertRaisesRegex(ValueError, "cycle"):
                self.observer.snapshot()
            self.assertEqual(self.observer.snapshots, [])

    def test_invalid_node_and_surface_pointers_are_not_dereferenced(self):
        for pointer in (1, 0x80000002, 0x7FFFFFFC, 0x803FFFFC, 0x80400000):
            self.memory.words[self.base] = pointer
            self.memory.reads.clear()
            with self.assertRaisesRegex(ValueError, "node pointer"):
                self.observer.snapshot()
            self.assertNotIn(pointer, self.memory.reads)
        self.chain()
        for pointer in (0, 1, 0x80000002, 0x803FFFD4, 0x80400000):
            self.memory.words[self.node + 4] = pointer
            self.memory.reads.clear()
            with self.assertRaisesRegex(ValueError, "surface"):
                self.observer.snapshot()
            self.assertNotIn(pointer, self.memory.reads)

    def test_each_budget_fails_explicitly_before_storing_partial_snapshot(self):
        self.chain()
        for name in ("MAX_NODES", "MAX_LINKS", "MAX_READS", "MAX_STORED_BYTES", "MAX_SNAPSHOTS"):
            with self.subTest(name=name), patch.object(floor_lists, name, 0):
                observer = floor_lists.FloorLists(self.memory)
                with self.assertRaisesRegex(RuntimeError, "budget"):
                    observer.snapshot()
                self.assertEqual(observer.snapshots, [])
        self.assertEqual(self.observer.snapshot(), 0)
        with patch.object(floor_lists, "MAX_SNAPSHOTS", 1):
            self.assertEqual(self.observer.snapshot(), 0)  # storage reused, not skipped
            self.memory.words[self.surface + 0x28] = 1
            with self.assertRaisesRegex(RuntimeError, "storage budget"):
                self.observer.snapshot()
            self.assertEqual(len(self.observer.snapshots), 1)

    def test_floor_pair_references_entry_and_return_snapshots(self):
        core, lists = Mock(), Mock()
        registers = {31: 0x80240000, 6: 0, 7: 0x80000100}
        core.register.side_effect = registers.__getitem__
        core.fpr_bits.return_value = 0
        core.u32.return_value = 0
        lists.snapshot.side_effect = [2, 3]
        probe = FloorProbe(core, lists=lists)
        context = lambda: {"authored": True}
        probe.handle(ENTRY, context, enabled=False)
        lists.snapshot.assert_not_called()
        probe.handle(ENTRY, context, enabled=True)
        self.assertEqual(lists.snapshot.call_count, 1)
        probe.handle(0x80240000, context, enabled=False)
        call = probe.report()["calls"][0]
        self.assertEqual(call["list_snapshot_before"], 2)
        self.assertEqual(call["list_snapshot_after"], 3)
        self.assertEqual(lists.snapshot.call_count, 2)

    def test_snapshot_error_cannot_complete_a_floor_pair(self):
        core, lists = Mock(), Mock()
        core.register.side_effect = {31: 0x80240000, 6: 0, 7: 0x80000100}.__getitem__
        core.fpr_bits.return_value = 0
        core.u32.return_value = 0
        lists.snapshot.side_effect = [0, RuntimeError("authored budget failure")]
        probe = FloorProbe(core, lists=lists)
        probe.handle(ENTRY, lambda: {}, enabled=True)
        with self.assertRaisesRegex(RuntimeError, "budget"):
            probe.handle(0x80240000, lambda: {}, enabled=True)
        self.assertEqual(probe.calls, [])
        with self.assertRaisesRegex(RuntimeError, "incomplete"):
            probe.report()


if __name__ == "__main__":
    unittest.main()
