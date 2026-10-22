"""Authored call/return evidence only: no ROM, emulator or game implementation."""
import json
import math
import unittest
from unittest.mock import Mock

from scripts.reference_emulator.floor_probe import ENTRY, FloorProbe


class FloorProbeTests(unittest.TestCase):
    def setUp(self):
        self.core = Mock()
        self.registers = {31: 0xFFFFFFFF80240000, 6: 0x12345678C0200000,
                          7: 0xFFFFFFFF80001000}
        self.fprs = {12: 0x80000000, 14: 0x3FC00000, 0: 0xC1000000}
        self.memory = {0x80001000: 0x80002000,
                       0x8000201C: 0x80000000, 0x80002020: 0x3F800000,
                       0x80002024: 0, 0x80002028: 0x41000000}
        self.core.register.side_effect = self.registers.__getitem__
        self.core.fpr_bits.side_effect = self.fprs.__getitem__
        self.core.u32.side_effect = self.memory.__getitem__
        self.core.i16.side_effect = lambda address: -(address - 0x80002000)
        self.core.breakpoint.side_effect = range(100)
        self.probe = FloorProbe(self.core)
        self.context = {"global_timer": 7, "level": 9, "area": 1, "act": 1,
                        "poll": 22, "vi": 24, "action_entry_counter": 3}
        self.factory = Mock(side_effect=lambda: self.context)

    def entry(self):
        return self.probe.handle(ENTRY, self.factory, enabled=True)

    def finish(self, enabled=True):
        return self.probe.handle(self.registers[31] & 0xFFFFFFFF,
                                 self.factory, enabled=enabled)

    def test_two_caller_pairs_contexts_raw_bits_and_surface(self):
        self.probe.install()
        self.core.breakpoint.assert_called_once_with(ENTRY)
        self.assertTrue(self.entry())
        self.core.breakpoint.assert_called_with(0x80240000)
        self.context["global_timer"] = 8
        self.assertTrue(self.finish())
        self.core.remove_breakpoint.assert_called_once_with(1)
        self.registers[31] = 0x80250000
        self.memory[0x80001000] = 0
        self.fprs[0] = 0x80000000
        self.assertTrue(self.entry())
        self.context["poll"] = 23
        # A pending return still completes if entry recording has been disabled.
        self.assertTrue(self.finish(enabled=False))
        report = self.probe.report()
        self.assertEqual(report["version"], 1)
        self.assertEqual(report["entry"], ENTRY)
        self.assertFalse(report["memory_write"])
        self.assertEqual(report["max_calls"], 100000)
        self.assertIn("candidate", report["abi"]["status"])
        first, second = report["calls"]
        self.assertEqual(first["input_bits"], [0x80000000, 0x3FC00000, 0xC0200000])
        self.assertEqual(first["input_values"], [-0.0, 1.5, -2.5])
        self.assertEqual(math.copysign(1, first["input_values"][0]), -1)
        self.assertEqual(first["context_before"]["global_timer"], 7)
        self.assertEqual(first["context_after"]["global_timer"], 8)
        self.assertEqual(first["output_address"], 0x80001000)
        self.assertEqual(first["height_bits"], 0xC1000000)
        self.assertEqual(first["height"], -8.0)
        self.assertEqual(first["surface"], {
            "address": 0x80002000, "type": 0,
            "vertices": [[-10, -12, -14], [-16, -18, -20], [-22, -24, -26]],
            "normal_bits": [0x80000000, 0x3F800000, 0],
            "origin_offset_bits": 0x41000000})
        self.assertEqual(second["return_pc"], 0x80250000)
        self.assertIsNone(second["surface"])
        self.assertEqual(second["height_bits"], 0x80000000)
        self.assertEqual(second["context_before"]["poll"], 22)
        self.assertEqual(second["context_after"]["poll"], 23)
        json.dumps(report, allow_nan=False)
        report["calls"].clear()
        self.assertEqual(len(self.probe.calls), 2)
        self.core.resume.assert_not_called()
        self.core.stop.assert_not_called()

    def test_disabled_and_unrelated_pc_do_not_inspect(self):
        self.assertTrue(self.probe.handle(ENTRY, self.factory, enabled=False))
        self.assertFalse(self.probe.handle(0x80240000, self.factory, enabled=True))
        self.assertFalse(self.probe.handle(0, self.factory, enabled=False))
        self.assertEqual(self.core.mock_calls, [])
        self.factory.assert_not_called()

    def test_nested_incomplete_and_unrelated_pc_while_pending(self):
        self.entry()
        with self.assertRaisesRegex(RuntimeError, "nested"):
            self.entry()
        with self.assertRaisesRegex(RuntimeError, "incomplete"):
            self.probe.report()
        self.core.reset_mock()
        self.assertFalse(self.probe.handle(0x80241000, self.factory, enabled=True))
        self.assertEqual(self.core.mock_calls, [])
        self.assertTrue(self.finish())

    def test_limit_is_failure_not_sampling_and_bounds_are_strict(self):
        for bad in [0, -1, 100001, True, 1.0, None]:
            with self.assertRaises(ValueError):
                FloorProbe(self.core, bad)
        FloorProbe(self.core, 100000)
        self.probe = FloorProbe(self.core, 1)
        self.entry()
        self.finish()
        with self.assertRaisesRegex(RuntimeError, "limit"):
            self.entry()
        self.assertEqual(len(self.probe.calls), 1)

    def test_invalid_return_and_output_pointers(self):
        for register, label in [(31, "return PC"), (7, "output pointer")]:
            original = self.registers[register]
            bad_values = [0, 0x7FFFFFFC, 0x80400000, 0x80000001, 0xA0001000]
            if register == 31:
                bad_values.append(ENTRY)
            for bad in bad_values:
                with self.subTest(register=register, bad=hex(bad)):
                    self.registers[register] = bad
                    with self.assertRaisesRegex(ValueError, label):
                        self.entry()
                    self.assertIsNone(self.probe.pending)
                    self.assertEqual(self.probe.calls, [])
            self.registers[register] = original
        self.factory.assert_not_called()
        self.core.breakpoint.assert_not_called()

    def test_invalid_surface_pointer_and_size_fail_without_surface_reads(self):
        for bad in [1, 0x80002002, 0x7FFFFFFC, 0x80400000, 0x803FFFD4]:
            with self.subTest(bad=hex(bad)):
                self.probe = FloorProbe(self.core)
                self.entry()
                self.memory[0x80001000] = bad
                with self.assertRaisesRegex(ValueError, "surface pointer"):
                    self.finish()
                self.assertEqual(self.probe.calls, [])
                self.core.i16.assert_not_called()
        # Last complete 0x30-byte object is valid (without interpreting normals).
        self.probe = FloorProbe(self.core)
        self.memory[0x80001000] = 0x803FFFD0
        for offset in (0x1C, 0x20, 0x24, 0x28):
            self.memory[0x803FFFD0 + offset] = 0x7FC00000
        self.entry()
        self.finish()
        self.assertEqual(self.probe.calls[0]["surface"]["normal_bits"], [0x7FC00000] * 3)

    def test_nonfinite_argument_and_return_rejected(self):
        for register in (12, 14):
            old = self.fprs[register]
            self.fprs[register] = 0x7FC00000
            with self.assertRaisesRegex(ValueError, "non-finite"):
                self.entry()
            self.fprs[register] = old
        old = self.registers[6]
        self.registers[6] = 0xFF800000
        with self.assertRaisesRegex(ValueError, "non-finite"):
            self.entry()
        self.registers[6] = old
        self.entry()
        self.fprs[0] = 0x7F800000
        with self.assertRaisesRegex(ValueError, "non-finite"):
            self.finish()
        self.assertEqual(self.probe.calls, [])
        with self.assertRaisesRegex(RuntimeError, "incomplete"):
            self.probe.report()

    def test_duplicate_install_fails(self):
        self.probe.install()
        with self.assertRaisesRegex(RuntimeError, "already installed"):
            self.probe.install()


if __name__ == "__main__":
    unittest.main()
