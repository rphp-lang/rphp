#!/usr/bin/env python3
"""Fast, exploratory perf instruction comparisons of already-built interpreters.

Run in a memory-limited user-systemd service. Output is private evidence: raw
program output and native profiles can contain source names or input data.
"""
import argparse
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import signal
import statistics
import subprocess
import tempfile
import time


def sha_file(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def run_group(command, *, cwd, env, timeout):
    process = subprocess.Popen(command, cwd=cwd, env=env, stdout=subprocess.PIPE,
                               stderr=subprocess.PIPE, start_new_session=True)
    try:
        stdout, stderr = process.communicate(timeout=timeout)
    except BaseException:
        # perf and time wrap the interpreter; terminate descendants too.
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        process.communicate()
        raise
    return process.returncode, stdout, stderr


def parse_count(text):
    rows = [line.split(",") for line in text.splitlines()
            if ",instructions:u," in line]
    if len(rows) != 1 or len(rows[0]) < 5:
        raise RuntimeError("missing or ambiguous instructions:u counter")
    row = rows[0]
    if not row[0].strip().isdigit() or float(row[4]) != 100.0:
        raise RuntimeError("unsupported, uncounted or multiplexed instruction counter")
    count = int(row[0])
    if count <= 0:
        raise RuntimeError("empty instruction count; analysis boundary was not measured")
    return count


def ordinary_stderr(data):
    return re.sub(rb"^(?:RPHP_BENCH_(?:ANALYSIS|FILES)=[^\n]*|Events (?:enabled|disabled)\.?)\n",
                  b"", data, flags=re.M)


def signature(result):
    status, stdout, stderr = result
    return {"exit": status, "stdout_sha256": hashlib.sha256(stdout).hexdigest(),
            "stderr_sha256": hashlib.sha256(ordinary_stderr(stderr)).hexdigest()}


def boundary():
    group = next(line.split(":", 2)[2] for line in Path("/proc/self/cgroup")
                 .read_text().splitlines() if line.startswith("0:"))
    root = Path("/sys/fs/cgroup") / group.lstrip("/")
    limit = (root / "memory.max").read_text().strip()
    if (not limit.isdigit() or not 0 < int(limit) <= 6 * 1024**3
            or (root / "memory.swap.max").read_text().strip() != "0"
            or (root / "memory.oom.group").read_text().strip() != "1"):
        raise RuntimeError("run inside an aggregate <=6 GiB/no-swap OOM-group boundary")
    return root


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--variant", action="append", default=[], metavar="LABEL=EXECUTABLE")
    parser.add_argument("--reference", default="php", help="reference interpreter")
    parser.add_argument("--project", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True, help="fresh private evidence directory")
    parser.add_argument("--phase", action="store_true", help="analysis FIFO hooks and timing marker required")
    parser.add_argument("--rounds", type=int, default=1, help="alternating pairs; exploratory default: 1")
    parser.add_argument("--cpu", type=int, default=2)
    parser.add_argument("--timeout", type=float, default=120)
    parser.add_argument("--profile", help="label for a separate native instruction-sampling run")
    parser.add_argument("--period", type=int, default=1000003)
    parser.add_argument("--input", action="append", type=Path, default=[], help="immutable input identity")
    parser.add_argument("command", nargs=argparse.REMAINDER, help="-- interpreter arguments")
    args = parser.parse_args(argv)
    if args.command[:1] == ["--"]:
        args.command.pop(0)
    if args.rounds < 1 or args.timeout <= 0 or args.period < 1 or not args.command:
        parser.error("positive rounds/timeout/period and interpreter arguments are required")
    if args.output.exists():
        parser.error("output directory must be fresh")
    variants = {}
    for spec in args.variant:
        label, separator, binary = spec.partition("=")
        if not separator or not re.fullmatch(r"[A-Za-z0-9_-]+", label) or label in variants or label == "reference":
            parser.error("use unique LABEL=EXECUTABLE; reference is reserved")
        variants[label] = Path(binary).resolve(strict=True)
    reference = shutil.which(args.reference)
    if not reference or not variants or (args.profile and args.profile not in variants):
        parser.error("reference, variants and selected profile must exist")
    variants["reference"] = Path(reference).resolve(strict=True)
    inputs = [path.resolve(strict=True) for path in args.input]
    cleanup = Path(__file__).resolve().with_name("cleanup-builds.sh")
    workspace_target = cleanup.parent.parent / "target"
    if any(binary.is_relative_to(workspace_target) for binary in variants.values()):
        parser.error("retain active executables outside workspace target before automatic cleanup")
    args.project = args.project.resolve(strict=True)
    args.output = args.output.resolve()
    root = boundary()
    if args.cpu not in os.sched_getaffinity(0):
        parser.error("CPU is outside the current affinity")
    os.sched_setaffinity(0, {args.cpu})
    args.output.mkdir(parents=True)
    env = dict(os.environ, LC_ALL="C")
    # Exact baselines may be retained across checkpoints. Superseded targets
    # are removed explicitly; age cleanup must not delete an active pair.
    env.setdefault("RPHP_BUILD_STALE_DAYS", "36500")
    # A desktop's bundled LLVM can break the host perf binary's dynamic link.
    env["LD_LIBRARY_PATH"] = "/usr/lib/x86_64-linux-gnu:/lib/x86_64-linux-gnu" if os.uname().machine == "x86_64" else "/usr/lib/aarch64-linux-gnu:/lib/aarch64-linux-gnu"
    for name in ("RPHP_PERF_CONTROL", "RPHP_PERF_ACK", "RPHP_HEAP"):
        env.pop(name, None)
    report = {"complete": False, "purpose": "exploratory instruction feedback, not an acceptance gate",
              "event": "instructions:u", "counter_scope": "analysis FIFO" if args.phase else "whole command",
              "cpu": args.cpu, "rounds": args.rounds, "warmup": "none; fresh TMPDIR per run",
              "memory_max_bytes": int((root / "memory.max").read_text()),
              "memory_swap_max_bytes": 0,
              "outlier_policy": "retain every valid output-checked sample", "samples": [],
              "binaries": {label: sha_file(binary) for label, binary in variants.items()},
              "inputs": [sha_file(path) for path in inputs]}

    def save():
        (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")

    def measured(label, name, profile=False):
        binary = variants[label]
        if sha_file(binary) != report["binaries"][label]:
            raise RuntimeError("binary changed during comparison")
        with tempfile.TemporaryDirectory(prefix="run-", dir=args.output) as temporary:
            tmp = Path(temporary)
            run_env = dict(env, TMPDIR=temporary)
            target = args.output / (name + (".perf.data" if profile else ".counters.txt"))
            command = (["perf", "record", "--quiet", "-c", str(args.period)] if profile
                       else ["perf", "stat", "-x", ","])
            command += ["-e", "instructions:u", "--output", str(target)]
            if args.phase:
                control, ack = tmp / "control", tmp / "ack"
                os.mkfifo(control)
                os.mkfifo(ack)
                command += ["-D", "-1", "--control", f"fifo:{control},{ack}"]
                run_env.update(RPHP_PERF_CONTROL=str(control), RPHP_PERF_ACK=str(ack))
            before = time.monotonic()
            result = run_group(command + ["--", str(binary), *args.command],
                               cwd=args.project, env=run_env, timeout=args.timeout)
            elapsed = time.monotonic() - before
        for suffix, data in zip(("stdout", "stderr"), result[1:]):
            (args.output / (name + "." + suffix)).write_bytes(data)
        observed = signature(result)
        if observed["exit"] not in (0, 1):
            raise RuntimeError(f"runtime/profiler failed: {label}; see retained output")
        timings = re.findall(rb"^RPHP_BENCH_ANALYSIS=([0-9.]+)$", result[2], re.M)
        if args.phase and len(timings) != 1:
            raise RuntimeError("analysis timing marker absent or repeated")
        row = {"runtime": label, "wall_seconds": elapsed, "output": observed}
        if timings:
            row["analysis_seconds"] = float(timings[0])
        if not profile:
            row["instructions"] = parse_count(target.read_text())
            row["counter_running_percent"] = 100.0
        return row

    with open("/tmp/rphp-benchmark.lock", "a") as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as error:
            report["failure"] = "exclusive benchmark lock is held by another job"
            save()
            raise RuntimeError(report["failure"]) from error
        try:
            save()
            probe = args.output / "perf-preflight.txt"
            result = run_group(["perf", "stat", "-x", ",", "-e", "instructions:u",
                                "--output", str(probe), "--", "/usr/bin/true"],
                               cwd=args.project, env=env, timeout=15)
            (args.output / "perf-preflight.stderr").write_bytes(result[2])
            if result[0] != 0:
                raise RuntimeError("hardware counter unavailable; see perf-preflight.stderr")
            parse_count(probe.read_text())
            subprocess.run([str(cleanup)], cwd=cleanup.parent.parent, env=env, check=True)
            reference_row = measured("reference", "reference")
            report["reference"] = reference_row
            save()
            labels = [label for label in variants if label != "reference"]
            for round_index in range(args.rounds):
                order = labels if round_index % 2 == 0 else list(reversed(labels))
                for label in order:
                    row = measured(label, f"round-{round_index}-{label}")
                    row["round"] = round_index
                    if row["output"] != reference_row["output"]:
                        raise RuntimeError(f"reference output mismatch: {label}")
                    report["samples"].append(row)
                    save()
                    print(f"{label}: {row['instructions'] / 1e9:.6f} G instructions", flush=True)
            report["medians"] = {label: statistics.median(row["instructions"] for row in report["samples"]
                                                         if row["runtime"] == label) for label in labels}
            baseline = report["medians"][labels[0]]
            report["delta_percent"] = {label: 100 * (count / baseline - 1)
                                       for label, count in report["medians"].items()}
            if args.profile:
                row = measured(args.profile, "instruction-profile", profile=True)
                if row["output"] != reference_row["output"]:
                    raise RuntimeError("profile output mismatch")
                row.update(period=args.period, scope=report["counter_scope"],
                           attribution="sampled instruction share; not exact per-function counts")
                report["profile"] = row
                result = run_group(["perf", "report", "--stdio", "--no-children", "--percent-limit", "0.5",
                                    "--sort", "dso,symbol", "-i", str(args.output / "instruction-profile.perf.data")],
                                   cwd=args.project, env=env, timeout=30)
                (args.output / "instruction-profile.txt").write_bytes(result[1] + result[2])
                if result[0] != 0:
                    raise RuntimeError("native profile decoding failed")
            if (any(sha_file(path) != report["binaries"][label] for label, path in variants.items())
                    or [sha_file(path) for path in inputs] != report["inputs"]):
                raise RuntimeError("input or binary changed during comparison")
            report["complete"] = True
            print("Instruction deltas (%):", report["delta_percent"], flush=True)
        except BaseException as error:
            report["failure"] = type(error).__name__ + ": " + str(error)
            raise
        finally:
            subprocess.run([str(cleanup)], cwd=cleanup.parent.parent, env=env, check=True)
            report["memory_peak_bytes"] = int((root / "memory.peak").read_text())
            report["memory_events"] = {key: int(value) for key, value in
                                       (line.split() for line in (root / "memory.events").read_text().splitlines())}
            save()


if __name__ == "__main__":
    main()
