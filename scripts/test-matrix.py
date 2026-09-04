#!/usr/bin/env python3
"""Run the complete feature matrix, retaining fingerprinted, resumable evidence."""

import argparse
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time


ROOT = Path(__file__).resolve().parent.parent
CONFIGURATIONS = {
    "default": [],
    "no-default": ["--no-default-features"],
    "erased": ["--features", "php-generics-erased"],
    "reified": ["--features", "php-generics-reified"],
    "all-features": ["--all-features"],
}


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def fingerprint(env):
    paths = subprocess.check_output(
        ["git", "ls-files", "-z", "--cached", "--others", "--exclude-standard"],
        cwd=ROOT,
    ).decode().split("\0")
    inputs = {}
    for name in sorted(set(paths)):
        if name and (name.startswith(("src/", "tests/", ".cargo/", "rust-toolchain"))
                     or name in ("Cargo.toml", "Cargo.lock", "build.rs", "scripts/test-matrix.py",
                                 "scripts/cleanup-builds.sh")):
            path = ROOT / name
            inputs[name] = digest(path) if path.is_file() else "missing"
    for tool in ("cargo", "rustc"):
        inputs[tool] = subprocess.check_output([tool, "--version", "--verbose"], cwd=ROOT).decode()
    cargo_home = Path(env.get("CARGO_HOME", str(Path.home() / ".cargo")))
    for name in ("config", "config.toml"):
        path = cargo_home / name
        inputs[f"global-cargo-{name}"] = digest(path) if path.is_file() else "missing"
    inputs["environment"] = {
        k: v for k, v in sorted(env.items())
        if k.startswith(("CARGO_", "RUST", "CC", "CFLAGS", "CXX", "LDFLAGS", "RPHP_", "PHP"))
        or k in ("PATH", "LANG", "LC_ALL", "TZ")
    }
    return hashlib.sha256(json.dumps(inputs, sort_keys=True).encode()).hexdigest()


def test_totals(output):
    groups = re.findall(
        r"test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out",
        output,
    )
    if not groups or any(int(g[1]) or int(g[4]) for g in groups):
        raise ValueError("missing test summary, failing tests or filtered tests")
    return dict(zip(("passed", "failed", "ignored", "measured", "filtered"),
                    [sum(int(g[i]) for g in groups) for i in range(5)]))


def reusable(record, directory, current_fingerprint):
    if (record.get("exit") != 0 or not record.get("validated")
            or record.get("fingerprint") != current_fingerprint):
        return False
    log = directory / record["log"]
    return log.is_file() and digest(log) == record.get("log_sha256")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, help="new evidence directory (ignored locally by default)")
    parser.add_argument("--resume", action="store_true", help="reuse successful, unchanged, hash-verified gates")
    parser.add_argument("--offline", action="store_true")
    parser.add_argument("--jobs", type=int, default=min(os.cpu_count() or 2, 16))
    args = parser.parse_args()
    if args.jobs < 1:
        parser.error("--jobs must be positive")
    if args.resume and args.output is None:
        parser.error("--resume needs --output")
    output = (args.output or ROOT / ".local-evidence" /
              f"test-matrix-{time.strftime('%Y%m%dT%H%M%S', time.gmtime())}-{os.getpid()}").resolve()
    state_path = output / "matrix.json"
    if output.exists() and not args.resume:
        parser.error("output already exists; use --resume or a new directory")
    output.mkdir(parents=True, exist_ok=True)
    lock_dir = ROOT / ".local-evidence"
    lock_dir.mkdir(exist_ok=True)
    lock = (lock_dir / "test-matrix.lock").open("a")
    try:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        parser.error("another test matrix owns this workspace")
    # Also protect custom in-repository evidence directories from accidental staging.
    (output / ".gitignore").write_text("*\n")
    env = dict(os.environ, CARGO_BUILD_JOBS=str(args.jobs), CARGO_TERM_COLOR="never")
    # A fast profile must never disable the runtime safety checks exercised by tests.
    env.update(CARGO_PROFILE_TEST_FAST_DEBUG_ASSERTIONS="true",
               CARGO_PROFILE_TEST_FAST_OVERFLOW_CHECKS="true")
    if env.get("CARGO_TARGET_DIR"):
        parser.error("unset CARGO_TARGET_DIR: the cleanup hook owns this workspace's target")
    current = fingerprint(env)
    state = json.loads(state_path.read_text()) if state_path.exists() else {"gates": {}}
    if state.get("fingerprint", current) != current:
        parser.error("build/test inputs changed; use a new evidence directory")
    state.update(fingerprint=current, profile="test-fast", jobs=args.jobs, complete=False)

    def save():
        temporary = state_path.with_suffix(".tmp")
        temporary.write_text(json.dumps(state, indent=2) + "\n")
        temporary.replace(state_path)

    def cleanup():
        subprocess.run(["bash", "scripts/cleanup-builds.sh"], cwd=ROOT, env=env, check=True)
        minimum = int(env.get("RPHP_BUILD_MIN_FREE_GIB", "20")) * 1024**3
        space = os.statvfs(ROOT)
        if space.f_bavail * space.f_frsize < minimum:
            raise RuntimeError("insufficient disk reserve after cleanup; stopping before next build")

    save()
    cleanup()
    try:
        cargo = ["cargo"]
        common = ["--locked", "--profile", "test-fast"] + (["--offline"] if args.offline else [])
        commands = [(name, cargo + ["test"] + common + flags)
                    for name, flags in CONFIGURATIONS.items()]
        commands.append(("all-targets", cargo + ["check"] + common + ["--all-features", "--all-targets"]))
        for name, command in commands:
            record = state["gates"].get(name, {})
            if args.resume and reusable(record, output, current):
                print(f"REUSE {name}: unchanged successful evidence", flush=True)
                continue
            cleanup()
            log_path = output / f"{name}.log"
            print(f"START {name}: {' '.join(command)}", flush=True)
            started = time.monotonic()
            with log_path.open("w") as log:
                result = subprocess.run(command, cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT)
            record = dict(command=command, exit=result.returncode,
                          elapsed_seconds=round(time.monotonic() - started, 3),
                          fingerprint=current, log=log_path.name, log_sha256=digest(log_path))
            state["gates"][name] = record
            save()
            if result.returncode:
                print(log_path.read_text()[-12000:], file=sys.stderr)
                return result.returncode
            if name in CONFIGURATIONS:
                try:
                    record["tests"] = test_totals(log_path.read_text())
                except ValueError:
                    record["exit"] = 1
                    save()
                    raise
            record["validated"] = True
            save()
            print(f"PASS {name}: {record['elapsed_seconds']}s {record.get('tests', '')}", flush=True)
        if fingerprint(env) != current:
            raise RuntimeError("sources changed during matrix; evidence is not an acceptance gate")
        state["complete"] = True
        save()
        print(f"Complete evidence: {state_path}", flush=True)
    finally:
        cleanup()
    return 0


if __name__ == "__main__":
    sys.exit(main())
