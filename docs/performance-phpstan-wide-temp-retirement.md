# Width-independent sole-owner temporary retirement

Status: accepted bounded instruction checkpoint; full PHPStan parity open.
Baseline: `4e79d905`, source
`9f4ebb69821b9505671cd633adba6c577427d31fe903966d6bffc293d6988664`.

## Outcome, evidence and hypothesis

Use the existing single-owner release proof for every valid statement range,
including its initialized tail. Remove repeated generic range and nested
release planning for shared owners without changing ownership representation,
native ABI, language behavior or introducing a workload recognizer.

The fresh exact analysis profile assigns 4.4735 billion exclusive main-body
instructions to ReleaseTemps. An independent baseline diagnostic records
5,252,123 direct-drop opportunities beyond the prefix, among 5,320,516
single-owner intervals. The potential budget is quantified by that affected
area and entry count; it is not assumed to be fully removable.

## Semantic envelope and ownership

Keep the existing prefix bitmap proof without inspecting uninitialized prefix
bytes. The established initialized-tail guarantee permits needs_cleanup reads
there. Find at most two owners: only an exactly sole owner can use the existing
String/callback-free Resource/shared Array or Object proof. Pending argument
cleanup precedes the same final strong-count check. References, closures, final
owners, marked foreach sources and multiple-owner ranges retain canonical
planning. Preserve the exact read-snapshot/GC suppression query, Rust drop,
zeroing, prefix-bit clearing, destructor order and exception context. No borrowed
proof survives PHP re-entry; no additional unsafe invariant is admitted.

The sole integrating task owns call_frames.rs. The preceding implementation
checkpoint is accepted and closed; this is the only active implementation.

## Gates and stop rule

First preserve exact default-feature baseline/candidate test-fast executables
and compare fresh analysis-only hardware counters, full output and relevant
compact/wide controls. This is an exploratory gate, not release acceptance.
Reject the approach early if application instructions do not materially fall,
before spending time on fresh PGO or a broad matrix.

After a useful application result, run existing single-temp, frame-prefix,
frame/reference cleanup and relevant lazy/weak/return boundaries in the focused
default/no-default/all-feature matrix. Validate exact CLI PHP behavior and
forced canonical execution, keeping known PHP gaps visible and baseline-equal.
Run formatting, unchanged unsafe inventory and all-target/all-feature checks.
Freeze sources for identical-policy PGO excluding PHPStan and holdouts; retain
all native windows, instructions, time, memory, code size, actual completions
and independently confirmed control changes. Native evidence is x86-64 only;
ARM64 is unavailable. All expensive work stays inside the verified 6 GiB/no-swap
boundary and exclusive lock, with automatic cleanup before and after cycles.

Reject semantic changes, skipped final/alias/callback retirement, prefix-byte
reads, an expanded unsafe assumption, failed focused gates or a final release
instruction result too small to justify this larger follow-up. Never hide the
existing last-owner callback or named-function self compile-time gaps as passes.

## Accepted result

Fresh same-policy PGO confirmation reduces analysis-only hardware instructions
from **75.5740 to 74.2814 billion (-1.710%)**, versus PHP's **10.3374 billion**.
The separate first window is 75.5317 to 74.2520 billion (-1.694%). Confirmation
analysis medians are **6.8690 to 6.7981 seconds (-1.033%)**, with PHP at 0.9188.
The remaining **7.19x instruction gap** keeps the overarching parity goal open.
Whole-command medians are 94.1168 to 92.7967 billion; these include startup and
are never pooled with analysis-only counters. RSS increases by 186 KiB.

The cheap initial gate used default-feature test-fast with optimization level
one, debug assertions and overflow checks. Its 119.0671 to 117.1300 billion
(-1.627%) analysis counts selected the implementation before expensive PGO;
they are a distinct build, not a regression from the optimized baseline.

A private default-feature diagnostic records **5,252,123 actual tail-range
completions**. Forced canonical execution records zero, with exactly the same
16,838,367 release entries, 5,445,153 tail-range entries and complete PHP output.
Prefix completions remain 9,329,155 in both modes. Both modes and all runtimes
have equal environment key sets. Four existing lifetime contracts run with
zero and seventy padding CVs; all eight exact PGO CLI comparisons match PHP.
The known callback-last-owner and named-function self compile-time gaps remain
baseline-equal failures, never passes. All **168 focused feature executions**,
formatting, unchanged unsafe inventory and all-target/all-feature checks pass.
No new architecture-specific code, frame layout or unsafe invariant is added.

The independent wide shared-read holdout improves **7.728% instructions** and
4.967% time. Every first-pass control outside the one-percent time envelope,
including gains, has an independent five-pair confirmation. Shared-frame time
+4.096% with instructions -0.195%, inherited-method time +6.081% with instructions
-0.083%, and shared-temp time +2.518% with instructions +0.450% remain explicit
accepted instruction-priority tradeoffs. Separate frontend windows associate
these changes with instruction supply; exact causal code placement is unproven.
This is an application instruction win, not a broad speedup claim.

The main executor grows 390,511 to 390,795 bytes, and release_statement_temps
17,101 to 17,290 bytes; return-owner retirement and frame pop sizes stay constant.
The eighteen-input fresh PGO training excludes PHPStan and all controls.
Instrumented/profile-use builds take 250.55/191.05 seconds. Twenty-six missing
profile warnings are confined to the untrained build script plus one summary;
no executable profile mismatch appears. The preparation boundary peaks at
5,571,551,232 bytes, within the verified 6 GiB/no-swap limit; every boundary
completes without OOM or timeout. Mandatory primary and isolated checkout
cleanup runs after the checkpoint; superseded PGO and diagnostic Cargo targets
are removed while exact sources, binaries, profiles and all samples remain.
Native evidence is x86-64 only; ARM64 measurement is unavailable.

Exact build identities, distributions, selection profile, exploratory windows,
forced canonical counts, controls, frontend diagnostics and resource results
are in [the checkpoint packet](performance-phpstan-wide-temp-retirement-samples.json).
The integrating task accepts this checkpoint and continues the original parity
goal with the larger measured call/frame costs.
