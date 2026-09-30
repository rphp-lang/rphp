#!/usr/bin/env python3
"""Focused integrity checks for the fast instruction runner."""
import importlib.util
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("bench_instructions", Path(__file__).with_name("bench-instructions.py"))
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)


class InstructionRunnerTests(unittest.TestCase):
    def test_exact_counter_and_rejected_invalid_counters(self):
        self.assertEqual(runner.parse_count("12345,,instructions:u,900,100.00,,\n"), 12345)
        for text in ("", "0,,instructions:u,900,100.00,,", "<not supported>,,instructions:u,0,0.00,,",
                     "12345,,instructions:u,900,99.99,,", "12,,instructions:u,900,100.00,,\n13,,instructions:u,900,100.00,,"):
            with self.subTest(text=text), self.assertRaises(RuntimeError):
                runner.parse_count(text)

    def test_only_benchmark_diagnostics_are_removed(self):
        data = b"RPHP_BENCH_ANALYSIS=1.0\nEvents enabled\nPHP warning\nRPHP_BENCH_FILES=5:0.8\nEvents disabled.\n"
        self.assertEqual(runner.ordinary_stderr(data), b"PHP warning\n")
        self.assertNotEqual(runner.signature((1, b"same", b"warning")), runner.signature((1, b"same", b"")))

    def test_timeout_terminates_descendant_group(self):
        with tempfile.TemporaryDirectory() as temporary:
            pid_file = Path(temporary) / "child.pid"
            code = "import subprocess,time,pathlib,sys; p=subprocess.Popen(['sleep','30']); pathlib.Path(sys.argv[1]).write_text(str(p.pid)); time.sleep(30)"
            with self.assertRaises(subprocess.TimeoutExpired):
                runner.run_group([sys.executable, "-c", code, str(pid_file)], cwd=temporary,
                                 env=os.environ, timeout=0.5)
            self.assertTrue(pid_file.exists())
            status = Path("/proc") / pid_file.read_text() / "status"
            try:
                state = status.read_text()
            except FileNotFoundError:
                state = None
            if state is not None:
                self.assertRegex(state, r"(?m)^State:\s+[ZX] ")

    def simulation(self, *, mismatch=False, denied=False, phase=False):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "memory.peak").write_text("1024")
            (root / "memory.max").write_text(str(6 * 1024**3))
            (root / "memory.events").write_text("oom 0\noom_kill 0\n")
            output = root / "evidence"
            calls = []

            def fake_run(command, **kwargs):
                if command[:2] == ["perf", "report"]:
                    return 0, b"sampled functions\n", b""
                target = Path(command[command.index("--output") + 1])
                preflight = command[-1] == "/usr/bin/true"
                if preflight and denied:
                    return 255, b"", b"counter access denied"
                target.write_text("12345,,instructions:u,900,100.00,,\n")
                if not preflight:
                    calls.append(target.name)
                stderr = b"RPHP_BENCH_ANALYSIS=0.1\n" if phase and not preflight else b""
                if phase and not preflight:
                    self.assertIn("--control", command)
                    self.assertTrue(Path(kwargs["env"]["RPHP_PERF_CONTROL"]).exists())
                bad = mismatch and target.name.startswith("round-0-candidate")
                return 0, b"bad" if bad else b"same", stderr

            argv = ["--variant", "baseline=" + sys.executable, "--variant", "candidate=" + sys.executable,
                    "--reference", sys.executable, "--project", str(root), "--output", str(output),
                    "--rounds", "2", "--cpu", "2", "--profile", "candidate"]
            if phase:
                argv.append("--phase")
            argv += ["--", "-c", "print('same')"]
            with patch.object(runner, "boundary", return_value=root), patch.object(runner, "run_group", side_effect=fake_run), \
                 patch.object(runner.os, "sched_getaffinity", return_value={2}), patch.object(runner.os, "sched_setaffinity"), \
                 patch.object(runner.subprocess, "run"), patch.object(runner.fcntl, "flock"):
                if mismatch or denied:
                    with self.assertRaises(RuntimeError):
                        runner.main(argv)
                else:
                    runner.main(argv)
            report = runner.json.loads((output / "report.json").read_text())
            if mismatch or denied:
                self.assertFalse(report["complete"])
                self.assertIn("failure", report)
            else:
                self.assertTrue(report["complete"])
                self.assertEqual(report["delta_percent"], {"baseline": 0.0, "candidate": 0.0})
                self.assertEqual(calls, ["reference.counters.txt", "round-0-baseline.counters.txt",
                    "round-0-candidate.counters.txt", "round-1-candidate.counters.txt",
                    "round-1-baseline.counters.txt", "instruction-profile.perf.data"])

    def test_order_output_and_profile_protocol(self):
        self.simulation(phase=True)

    def test_wrong_output_remains_failure(self):
        self.simulation(mismatch=True)

    def test_denied_counter_remains_failure(self):
        self.simulation(denied=True)


if __name__ == "__main__":
    unittest.main()
