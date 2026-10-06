# Native instruction feedback

Status: tooling checkpoint; the PHPStan instruction/time parity goal remains open.
No interpreter or allocator source changes are part of this checkpoint.

`scripts/bench-instructions.py` provides one reusable comparison command instead
of a separate driver per candidate. Already-built reference, baseline and
candidate executables run with identical arguments. It records native
`instructions:u`, validates the reference exit/stdout/stderr, uses a fresh
TMPDIR and one pinned CPU, and reverses pair order in successive rounds.
The default is one exploratory pair. Final feature, correctness and PGO
acceptance gates are separate.

The optional FIFO protocol excludes startup and requires an analysis timing
marker. A separate native instruction-sampling run produces a function report
for diagnosis. Sample shares are estimates; they are never labeled exact
per-function instruction counts. Callgrind remains available for instruction
sites and call edges, without making every edit pay its emulation cost.

The runner verifies the aggregate memory boundary, owns the exclusive benchmark
lock and runs cleanup. A timeout kills the whole process group, including the
profiler's runtime descendants. Active binaries must be retained outside the
workspace target. Binary and declared-input identities must remain unchanged.
Unsupported, zero or multiplexed counters and wrong output reject the run.
Evidence directories are private because raw diagnostics may contain input data.

Six final focused checks pass: exact/invalid counter parsing, diagnostic-output
filtering, actual process-group timeout cleanup, simulated FIFO/pair ordering and
profile protocol, wrong-output rejection and counter-denial rejection. The initial
test harness incorrectly nested the benchmark lock; two subsequent checks exposed
Linux process-state/read races after successful termination. Those failures remain
private evidence. The final timeout check accepts an absent, dead or zombie
descendant; it does not accept a running process. Python syntax and diff checks pass.

The initial real host preflight failed visibly with `perf_event_paranoid=4` and
recorded no measurement samples. After the host setting changed to 2, the native
end-to-end self-comparison and instruction profile passed in **30.751 seconds**.
The same executable measured **74.2860 and 74.2752 billion** analysis instructions,
a **0.0145%** difference, against PHP's **10.3378 billion**. Both comparison runs
and the separate profile have identical reference exit/stdout/stderr. The profile
records 74 thousand samples with zero lost samples. This validates the feedback
loop, not a runtime optimization. The aggregate peak is 650,764,288 bytes, with
no OOM or swap. Host perf also requires
system library search paths because desktop-bundled LLVM otherwise causes a
loader error; the runner selects system libraries.
No fake test counter is application performance evidence.

The accepted application result remains **74.2814 billion hardware analysis
instructions**, versus PHP **10.3374 billion**. The separately completed compiler
no-owner diagnostic admits zero PHPStan intervals, while its positive scalar
control admits one. It changes no production cleanup behavior and is not a new
performance result. Usage is documented in [benchmarking.md](benchmarking.md).
