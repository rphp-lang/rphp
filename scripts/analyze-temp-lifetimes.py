#!/usr/bin/env python3
"""Screen normal-path TMP lifetime facts in a retained bytecode census.

This is an offline design envelope, not permission to elide cleanup. The packet
does not contain complete exception, suspension or entry-edge metadata.
"""
import argparse
import collections
import hashlib
import json
from pathlib import Path


# Successful results of these operators cannot be heap owners. String bitwise
# operations, Add (array union), increments and casts deliberately stay out.
SCALAR_RESULTS = {
    "Sub", "Mul", "Div", "Mod", "Pow", "ShiftLeft", "ShiftRight",
    "BoolNot", "IsEqual", "IsNotEqual", "IsIdentical", "IsNotIdentical",
    "IsSmaller", "IsSmallerOrEqual", "Spaceship", "Instanceof", "Strlen",
    "Sub_CvConst", "Sub_TmpTmp", "IsSmaller_CvConst", "IsEqual_CvConst",
    "Add_LongLong", "Sub_LongLong", "Mul_LongLong", "Mod_LongLong",
    "BitwiseXor_LongLong", "BitwiseAnd_LongLong", "BitwiseOr_LongLong",
    "Strlen_Cv", "Strlen_String",
}
READ_RESULTS = {"FetchCvR", "FetchObjR", "FetchDimR"}
READ_ONLY_USES = SCALAR_RESULTS | READ_RESULTS | {
    "Add", "Add_CvConst", "Add_TmpTmp", "Add_CvTmp", "Concat",
    "BitwiseAnd", "BitwiseOr", "BitwiseXor", "BitwiseNot",
    "JmpZ", "JmpNZ", "Return", "Echo", "Echo_String", "Echo_Long",
}
UNKNOWN, SCALAR, RETIRED, MAY_OWN = range(4)


def plain_release(i):
    return i[1] == i[2] == 2 and i[3] == 0 and i[7] in (0, 8)


def plain_read(name, i):
    # Other contexts can construct references, retain mutable l-values or
    # encode a direct region entry. No such ownership claim is inferred here.
    return name in READ_RESULTS and i[7] == 0 and i[8] == 0


def audit_body(record, names):
    fields, counts = record["instruction_fields"], record["site_counts"]
    total = record["cv_slots"] + record["tmp_slots"]
    first_tmp = record["cv_slots"]
    assert len(fields) == len(counts) == record["instructions"]
    assert sum(counts) == record["decoded_steps"]
    assert all(len(i) == 9 and i[0] in names for i in fields)
    definitions = collections.Counter()
    excluded = set()
    for i in fields:
        name = names[i[0]]
        if name == "ReleaseTemps":
            assert i[4] <= i[5] <= total
            if not plain_release(i):
                excluded.update(range(max(first_tmp, i[4]), i[5]))
            continue
        for kind, slot in ((i[1], i[4]), (i[2], i[5]), (i[3], i[6])):
            if kind not in (2, 3):
                continue
            assert first_tmp <= slot < total
            if kind == 3:
                excluded.add(slot)
        if i[3] in (2, 3):
            definitions[i[6]] += 1
        uses = {slot for kind, slot in ((i[1], i[4]), (i[2], i[5])) if kind in (2, 3)}
        consuming_assignment = name == "AssignCv" and i[1] == 4 and i[3] == 0 and i[7] == 2
        if name not in READ_ONLY_USES and not consuming_assignment:
            excluded.update(uses)
        if name in READ_RESULTS and not plain_read(name, i):
            excluded.update(uses)
            if i[3] in (2, 3):
                excluded.add(i[6])
    private = {slot for slot, n in definitions.items() if n == 1 and slot not in excluded}
    result = collections.Counter()
    ranges = []
    cursor = 0
    for start, end in record["block_ranges"]:
        assert start == cursor and start <= end < len(fields)
        cursor = end + 1
        # Unknown predecessors never become an assumed fresh-frame proof.
        states = [UNKNOWN] * total
        for pc in range(start, end + 1):
            i, hits = fields[pc], counts[pc]
            name = names[i[0]]
            if name == "ReleaseTemps":
                result["release_hits"] += hits
                if not plain_release(i):
                    states = [UNKNOWN] * total
                    result["effect_release_hits"] += hits
                    continue
                slots = list(range(i[4], i[5]))
                allowed = all(slot in private and states[slot] in (SCALAR, RETIRED) for slot in slots)
                if allowed:
                    result["local_empty_release_hits"] += hits
                    if hits:
                        ranges.append({"body_id": record["id"], "pc": pc, "hits": hits,
                                       "first": i[4], "end": i[5],
                                       "scalar_slots": sum(states[s] == SCALAR for s in slots),
                                       "retired_slots": sum(states[s] == RETIRED for s in slots)})
                else:
                    result["unproved_release_hits"] += hits
                    result["unproved_private_slot_hits"] += hits * sum(s not in private for s in slots)
                    result["unproved_state_slot_hits"] += hits * sum(states[s] == UNKNOWN for s in slots if s in private)
                    result["live_owner_slot_hits"] += hits * sum(states[s] == MAY_OWN for s in slots if s in private)
                for slot in slots:
                    states[slot] = RETIRED if slot in private else UNKNOWN
                continue
            if name == "AssignCv" and i[1] == 4 and i[3] == 0 and i[7] == 2 and i[2] in (2, 3):
                states[i[5]] = RETIRED if i[5] in private else UNKNOWN
                result["consuming_assignment_hits"] += hits
                continue
            is_read = plain_read(name, i)
            if name in SCALAR_RESULTS or is_read or name in READ_ONLY_USES:
                if i[3] in (2, 3):
                    slot = i[6]
                    if is_read:
                        result["read_result_hits"] += hits
                        if slot in private and states[slot] in (SCALAR, RETIRED):
                            result["local_vacant_read_result_hits"] += hits
                    states[slot] = (SCALAR if name in SCALAR_RESULTS else MAY_OWN) if slot in private else UNKNOWN
                continue
            # An unmodelled effect can have hidden temporary writes. Keep it a
            # barrier, even when the explicit instruction result looks scalar.
            states = [UNKNOWN] * total
            result["unknown_effect_hits"] += hits
    assert cursor == len(fields)
    assert result["release_hits"] == (result["effect_release_hits"] +
        result["local_empty_release_hits"] + result["unproved_release_hits"])
    return result, ranges


def audit(packet):
    assert packet["complete"] and not packet["native_performance_acceptance"]
    assert packet["instruction_field_order"] == ["opcode", "op1_type", "op2_type", "result_type",
        "op1", "op2", "result", "flags", "extended"]
    names = {int(k): v for k, v in packet["opnames"].items()}
    totals = collections.Counter()
    ranges = []
    body_rows = []
    records = packet["records"]
    assert len({r["id"] for r in records}) == len(records) == packet["totals"]["record_count"]
    assert sum(r["decoded_steps"] for r in records) == packet["totals"]["decoded_steps"]
    for record in records:
        counters, local_ranges = audit_body(record, names)
        totals.update(counters)
        ranges.extend(local_ranges)
        if counters["local_empty_release_hits"]:
            body_rows.append({"body_id": record["id"], "empty_release_hits": counters["local_empty_release_hits"]})
    expected = dict(packet["totals"]["opcodes"])[next(k for k, v in names.items() if v == "ReleaseTemps")]
    assert totals["release_hits"] == expected
    totals.setdefault("local_empty_release_hits", 0)
    totals.setdefault("local_vacant_read_result_hits", 0)
    return {"status": "offline local normal-path envelope; not an execution proof",
        "native_performance_acceptance": False, "runtime_implementation_admitted": False,
        "production_changed": False, "parity_complete": False,
        "census_source_sha256": packet["source_sha256"], "census_binary_sha256": packet["binary_sha256"],
        "decoded_steps": packet["totals"]["decoded_steps"], "body_count": len(records),
        "totals": dict(totals), "range_count": len(ranges),
        "bodies": sorted(body_rows, key=lambda r: -r["empty_release_hits"]),
        "ranges": ranges,
        "limits": ["whole-request frequencies are not native instruction costs",
            "packet lacks complete exception, suspension and arbitrary entry metadata",
            "no fresh-frame or across-block vacancy is assumed",
            "private-slot classification is conservative syntax, not a complete alias proof",
            "unknown effects and modifying/reference read contexts remain barriers",
            "operand/result publication, cleanup origin and pending exceptions still need execution proof"]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    raw = args.input.read_bytes()
    report = audit(json.loads(raw))
    report["census_sha256"] = hashlib.sha256(raw).hexdigest()
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({k: report[k] for k in ("status", "totals", "range_count", "runtime_implementation_admitted")}))


if __name__ == "__main__":
    main()
