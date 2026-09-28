#!/usr/bin/env python3
"""Compare already-built PHPStan runtimes with validated cold/warm cache cycles.

Hold the project's exclusive benchmark lock around this runner. Paths and raw
project output are kept out of the JSON report. Use a private output directory
when the input project is private; failed output is retained there for diagnosis.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import random
import shutil
import signal
import subprocess
import tempfile
import time


def sha(data):
    return hashlib.sha256(data).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--variant", action="append", default=[], metavar="LABEL=EXECUTABLE")
    parser.add_argument("--system-variant", action="append", default=[], metavar="LABEL=EXECUTABLE")
    parser.add_argument("--php", default="php")
    parser.add_argument("--phar", required=True, type=Path)
    parser.add_argument("--project", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--rounds", type=int, default=7)
    parser.add_argument("--seed", type=int, default=280926)
    args = parser.parse_args()
    if args.rounds < 1 or args.output.exists():
        parser.error("positive rounds and a fresh output path are required")
    variants = {}
    for spec, system in [(s, False) for s in args.variant] + [(s, True) for s in args.system_variant]:
        label, sep, path = spec.partition("=")
        if not sep or not label or label in variants or label == "php":
            parser.error("use unique LABEL=EXECUTABLE values; php is reserved")
        variants[label] = (Path(path).resolve(strict=True), system)
    php = shutil.which(args.php)
    if not php or not variants:
        parser.error("a reference PHP executable and at least one variant are required")
    variants["php"] = (Path(php).resolve(strict=True), False)
    phar = args.phar.resolve(strict=True)
    project = args.project.resolve(strict=True)
    flags = ["-d", "disable_functions=proc_open,pcntl_signal"]
    references = {}
    report = {
        "rounds": args.rounds, "seed": args.seed,
        "warmup": "one validated boot/cold/warm cycle per executable",
        "cold_cache": "fresh TMPDIR; filesystem caches warm",
        "warm_cache": "same TMPDIR after cold; resultCache.php existence checked",
        "affinity": "inherited", "timeout_seconds": 120,
        "outlier_policy": "retain all successful output-validated samples",
        "phar_sha256": sha(phar.read_bytes()),
        "binaries": {name: {"sha256": sha(path.read_bytes()), "system_fallback": system}
                     for name, (path, system) in variants.items()},
        "samples": [], "complete": False,
    }

    def cycle(label, rnd, reference=False):
        binary, system = variants[label]
        with tempfile.TemporaryDirectory(prefix="phpstan-bench-", dir=args.output.parent) as tmp:
            env = dict(os.environ, TMPDIR=tmp)
            env.pop("RPHP_HEAP", None)
            if system:
                env["RPHP_HEAP"] = "system"
            for case in ("boot", "cold", "warm"):
                arguments = ["--version"] if case == "boot" else ["analyse", "--no-progress", "--no-ansi"]
                metric = Path(tmp) / "time.txt"
                before = time.perf_counter_ns()
                p = subprocess.Popen(
                    ["/usr/bin/time", "-f", "%M %R %U %S", "-o", str(metric),
                     str(binary), *flags, str(phar), *arguments],
                    cwd=project, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                    start_new_session=True,
                )
                try:
                    stdout, _ = p.communicate(timeout=120)
                except subprocess.TimeoutExpired:
                    # time(1) wraps the runtime: terminate its entire group.
                    os.killpg(p.pid, signal.SIGKILL)
                    stdout, _ = p.communicate()
                    args.output.with_suffix(".failed-output.txt").write_bytes(stdout)
                    raise
                wall = (time.perf_counter_ns() - before) / 1e9
                valid = p.returncode == 0 if case == "boot" else p.returncode in (0, 1)
                if reference and valid:
                    references[case] = (p.returncode, stdout)
                if not valid or (p.returncode, stdout) != references.get(case):
                    args.output.with_suffix(".failed-output.txt").write_bytes(stdout)
                    raise RuntimeError(f"PHPStan output/exit mismatch: {label} {case}, exit={p.returncode}")
                rss, faults, user, system_time = metric.read_text().splitlines()[-1].split()
                caches = list(Path(tmp).rglob("resultCache.php"))
                if case in ("cold", "warm") and not caches:
                    raise RuntimeError(f"result cache absent after {label} {case}")
                if rnd is not None:
                    report["samples"].append(dict(
                        variant=label, round=rnd, workload=case, wall=wall,
                        rss_kib=int(rss), minor_faults=int(faults),
                        user_seconds=float(user), system_seconds=float(system_time),
                        exit=p.returncode, output_sha256=sha(stdout),
                    ))

    cycle("php", None, reference=True)
    if references["cold"] != references["warm"]:
        raise RuntimeError("reference PHP cold and warm output differ")
    for label in variants:
        if label != "php":
            cycle(label, None)
    report["reference"] = {case: {"exit": rc, "output_sha256": sha(out)}
                           for case, (rc, out) in references.items()}
    rng = random.Random(args.seed)
    for rnd in range(args.rounds):
        order = list(variants)
        rng.shuffle(order)
        for label in order:
            cycle(label, rnd)
        args.output.write_text(json.dumps(report, indent=2) + "\n")
        print(f"completed PHPStan round {rnd + 1}/{args.rounds}", flush=True)
    report["complete"] = True
    args.output.write_text(json.dumps(report, indent=2) + "\n")


if __name__ == "__main__":
    main()
