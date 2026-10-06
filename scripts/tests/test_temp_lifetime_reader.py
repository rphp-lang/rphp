import importlib.util
from pathlib import Path
import unittest


spec = importlib.util.spec_from_file_location("temp_lifetime_reader",
    Path(__file__).resolve().parents[1] / "analyze-temp-lifetimes.py")
reader = importlib.util.module_from_spec(spec)
spec.loader.exec_module(reader)
NAMES = {0: "FetchObjR", 1: "FetchDimR", 2: "ReleaseTemps", 3: "AssignCv",
         4: "SendRef", 5: "DoFcall", 6: "Sub"}


def body(fields, blocks=None):
    return {"id": 7, "cv_slots": 2, "tmp_slots": 3,
            "instruction_fields": fields, "site_counts": [3] * len(fields),
            "instructions": len(fields), "decoded_steps": 3 * len(fields),
            "block_ranges": blocks if blocks is not None else [[0, len(fields) - 1]]}


PROPERTY = [0, 4, 1, 2, 0, 0, 2, 0, 0]
DIMENSION = [1, 2, 4, 2, 2, 1, 3, 0, 0]
CONSUME_ARRAY = [2, 2, 2, 0, 2, 3, 0, 8, 0]
MOVE_RESULT = [3, 4, 2, 0, 1, 3, 0, 2, 0]
COMPLETE = [2, 2, 2, 0, 2, 4, 0, 0, 0]


class LifetimeReaderTests(unittest.TestCase):
    def test_heap_read_consumption_needs_both_lifetime_events(self):
        counters, ranges = reader.audit_body(body(
            [PROPERTY, DIMENSION, CONSUME_ARRAY, MOVE_RESULT, COMPLETE]), NAMES)
        self.assertEqual(counters["local_empty_release_hits"], 3)
        self.assertEqual(counters["unproved_release_hits"], 3)
        self.assertEqual([(r["pc"], r["retired_slots"]) for r in ranges], [(4, 2)])

    def test_moving_only_one_source_does_not_retire_the_other_owner(self):
        counters, ranges = reader.audit_body(body(
            [PROPERTY, DIMENSION, MOVE_RESULT, COMPLETE]), NAMES)
        self.assertEqual(counters["local_empty_release_hits"], 0)
        self.assertFalse(ranges)

    def test_address_escape_rejects_the_same_syntactic_chain(self):
        send_reference = [4, 2, 0, 0, 3, 0, 0, 0, 0]
        counters, ranges = reader.audit_body(body(
            [PROPERTY, DIMENSION, CONSUME_ARRAY, send_reference, MOVE_RESULT, COMPLETE]), NAMES)
        self.assertEqual(counters["local_empty_release_hits"], 0)
        self.assertFalse(ranges)

    def test_join_and_unknown_effect_do_not_inherit_retirement_facts(self):
        chain = [PROPERTY, DIMENSION, CONSUME_ARRAY, MOVE_RESULT, COMPLETE]
        counters, ranges = reader.audit_body(body(chain, [[0, 2], [3, 4]]), NAMES)
        self.assertEqual(counters["local_empty_release_hits"], 0)
        call = [5, 0, 0, 0, 0, 0, 0, 0, 0]
        counters, ranges = reader.audit_body(body(chain[:3] + [call] + chain[3:]), NAMES)
        self.assertEqual(counters["local_empty_release_hits"], 0)

    def test_scalar_result_and_effect_marker_have_distinct_roles(self):
        scalar = [6, 4, 1, 2, 0, 0, 2, 0, 0]
        release = [2, 2, 2, 0, 2, 3, 0, 0, 0]
        counters, ranges = reader.audit_body(body([scalar, release]), NAMES)
        self.assertEqual(counters["local_empty_release_hits"], 3)
        self.assertEqual(ranges[0]["scalar_slots"], 1)
        effect = release.copy()
        effect[7] = 16
        counters, ranges = reader.audit_body(body([scalar, effect]), NAMES)
        self.assertEqual(counters["effect_release_hits"], 3)
        self.assertFalse(ranges)

    def test_frame_entry_does_not_imply_a_vacant_result(self):
        # Raw argument cells may overlap compiler TMP addresses at entry.
        counters, _ = reader.audit_body(body([PROPERTY, COMPLETE]), NAMES)
        self.assertEqual(counters["local_vacant_read_result_hits"], 0)

    def test_malformed_range_is_rejected(self):
        broken = COMPLETE.copy()
        broken[5] = 6
        with self.assertRaises(AssertionError):
            reader.audit_body(body([broken]), NAMES)


if __name__ == "__main__":
    unittest.main()
