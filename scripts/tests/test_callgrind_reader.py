"""Position/association regressions independent of runtime performance."""

from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from callgrind_reader import read_profile


class CallgrindReaderTests(unittest.TestCase):
    def read(self, source):
        return read_profile(source.splitlines())

    def test_call_source_does_not_shift_next_native_instruction(self):
        profile = self.read("""
positions: instr line
events: Ir
fn=(1) caller
0x100 9 2
cfn=(2) callee
calls=1 0x200 10
+1 * 30
+2 * 4
totals: 6
""")
        self.assertEqual(profile.records[1].position, (0x101, 9))
        self.assertEqual(profile.records[1].target_position, (0x200, 10))
        self.assertEqual(profile.records[2].position, (0x102, 9))
        self.assertEqual(profile.totals, (6,))

    def test_jump_source_and_target_share_ordinary_base(self):
        profile = self.read("""
positions: instr line
events: Ir
fn=caller
0x100 9 3
jcnd=2/3 +16 +1
+1 *
jump=1 +32 +2
+2 *
+4 +3 5
totals: 8
""")
        self.assertEqual(profile.records[1].target_position, (0x110, 10))
        self.assertEqual(profile.records[1].position, (0x101, 9))
        self.assertEqual(profile.records[1].count, 2)
        self.assertEqual(profile.records[1].executions, 3)
        self.assertEqual(profile.records[2].target_position, (0x120, 11))
        self.assertEqual(profile.records[3].position, (0x104, 12))

    def test_missing_events_are_zero_and_ordinary_zero_changes_base(self):
        profile = self.read("""
events: Ir Dr
fn=(1) caller
10 7 2
12
cfn=(1)
calls=1 20
+1 100 10
+2 3
totals: 10 2
""")
        self.assertEqual(profile.records[1].costs, (0, 0))
        self.assertEqual(profile.records[3].position, (14,))
        self.assertEqual(profile.records[2].target_function, "caller")
        self.assertEqual(profile.self_costs()["", "caller", (14,)], 3)

    def test_objects_keep_equal_function_names_and_positions_separate(self):
        profile = self.read("""
events: Ir
ob=(1) first.so
fn=(1) function
10 2
ob=(2) second.so
fn=(1)
10 3
totals: 5
""")
        self.assertEqual(profile.self_costs()["first.so", "function", (10,)], 2)
        self.assertEqual(profile.self_costs()["second.so", "function", (10,)], 3)

    def test_bad_totals_and_unfinished_association_are_failures(self):
        with self.assertRaisesRegex(ValueError, "totals disagree"):
            self.read("events: Ir\nfn=caller\n1 2\ntotals: 3")
        with self.assertRaisesRegex(ValueError, "final association"):
            self.read("events: Ir\nfn=caller\n1 2\ncalls=1 9")


if __name__ == "__main__":
    unittest.main()
