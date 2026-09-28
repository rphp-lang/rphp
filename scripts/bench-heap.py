#!/usr/bin/env python3
"""Interleave already-built heap_alloc examples; reject failed or differing runs.

Keep compilation outside the timing cycle. Each --variant is LABEL=EXECUTABLE.
The caller must hold the shared benchmark lock for the entire build/run cycle.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import random
import statistics
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--variant", action="append", required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--rounds", type=int, default=15)
    parser.add_argument("--count", type=int, default=50_000_000)
    parser.add_argument("--cpu", type=int, default=min(os.sched_getaffinity(0)))
    parser.add_argument("--seed", type=int, default=270926)
    args = parser.parse_args()
    if args.rounds < 1 or args.count < 1:
        parser.error("rounds and count must be positive")
    if args.output.exists():
        parser.error("output already exists; retain previous samples")
    variants = {}
    for item in args.variant:
        label, separator, path = item.partition("=")
        if not separator or not label or label in variants:
            parser.error("use a unique LABEL=EXECUTABLE for each variant")
        variants[label] = Path(path).resolve(strict=True)
    if len(variants) < 2:
        parser.error("at least two variants are required")
    cases = ("lifo", "mixed", "burst", "specialized")
    checksums = {}

    def run(label, case, count):
        result = subprocess.run(
            ["taskset", "-c", str(args.cpu), str(variants[label]), case, str(count)],
            capture_output=True, text=True, check=True, timeout=120,
        )
        words = result.stdout.split()
        actual = (count + 255) // 256 * 256 if case == "burst" else count
        if len(words) != 4 or words[:2] != [case, str(actual)]:
            raise ValueError(f"unexpected output from {label}: {result.stdout!r}")
        checksum, elapsed = map(int, words[2:])
        key = (case, actual)
        if checksums.setdefault(key, checksum) != checksum or elapsed <= 0:
            raise ValueError(f"checksum/time mismatch: {label} {case}")
        return actual, checksum, elapsed

    report = {
        "cpu": args.cpu, "seed": args.seed, "rounds": args.rounds,
        "count": args.count, "warmup_count": 1_000_000,
        "outlier_policy": "retain all successful, checksum-validated runs",
        "binaries": {label: hashlib.sha256(path.read_bytes()).hexdigest()
                     for label, path in variants.items()},
        "samples": [],
    }
    for case in cases:
        for label in variants:
            run(label, case, report["warmup_count"])
    rng = random.Random(args.seed)
    for round_number in range(args.rounds):
        for case in cases:
            order = list(variants)
            rng.shuffle(order)
            for label in order:
                count, checksum, elapsed = run(label, case, args.count)
                report["samples"].append(dict(
                    round=round_number, case=case, variant=label,
                    count=count, checksum=checksum, ns=elapsed,
                ))
        args.output.write_text(json.dumps(report, indent=2) + "\n")
        print(f"completed round {round_number + 1}/{args.rounds}", flush=True)
    for case in cases:
        for label in variants:
            values = sorted(s["ns"] / 1e6 for s in report["samples"]
                            if s["case"] == case and s["variant"] == label)
            lo, hi = values[int((len(values) - 1) * .1)], values[int((len(values) - 1) * .9)]
            print(f"{case:11} {label:20} median={statistics.median(values):.3f} ms "
                  f"p10={lo:.3f} p90={hi:.3f}")


if __name__ == "__main__":
    main()
