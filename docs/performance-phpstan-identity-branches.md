# PHPStan: direct strict identity branches

Accepted portable-Rust checkpoint on `codex/perf-phpstan-cold`, baseline
`8b06523f`. Independent confirmation reduces analysis **69.8710 to 69.6827
billion instructions (-0.270%)**, with exact PHP output. Analysis medians are
**6.4252/6.4348 seconds**, versus PHP's 10.3379 billion instructions/0.9454
seconds. There is no confirmed analysis-time improvement. The remaining
**6.74x instruction gap** keeps simultaneous instruction/time parity active.
[Every retained sample and build identity](performance-phpstan-identity-branches-samples.json)
is available; this result covers the identical five-file serial analysis.

## Representation and proof

The earlier exact profiler records 9,323,821 strict identity comparisons and
715,882,182 instructions in their uniquely reachable executor body, excluding
called helpers and shared dispatch. Those profiler counts are evidence for
selection, not native counts of this new candidate.

Compiler finalization combines `IsIdentical` or `IsNotIdentical` with its
adjacent sole-consumer `JmpZ`/`JmpNZ`. The general `JmpZ_Identical` and
`JmpNZ_Identical` bytecodes avoid one scalar scratch write/read and dispatch.
The canonical checked comparison still handles every PHP value type and
recursive-array errors. There is no new runtime eligibility guard, source-name
recognition, ASM, allocation, ownership operation or frame/Value/cache ABI.

The proof requires exactly one physical scratch definition and use, an
unmarked adjacent jump, bounded destinations, and no independent control or
exception entry into that jump. Hidden foreach, diagnostic and trait slots
count conservatively. Control targets are decoded by their opcode ABI;
Return provenance is not an instruction index. SnapshotDiagnosticWrite and
FetchDimR diagnostic continuation targets are protected too. Dynamic finally
continuations veto this local transformation.

Instruction positions, the original adjacent jump, source sites and release
intervals stay intact. Metadata retains the original scalar result slot, so
existing typed planners reconstruct the canonical comparison and keep their
original conditions, live maps and side-exit positions. The baseline hot
executor already declines strict comparisons; it still exits before effects.
Both native backends consume the unchanged typed plans. There is no new native
lowering or ARM64 performance claim. Value/frame/cache/Instruction sizes remain
16/64/16/16 bytes; unsafe inventory remains 1,749 blocks/321 functions.

## Selection and validation

The repaired ordinary release selector saves **0.520%**. Fresh fixed-manifest
PGO saves **0.262%** initially and **0.270%** independently. Both native windows
use two alternating pairs on CPU 2, no warmup and fresh TMPDIR, with complete
`instructions:u` running time and exact PHP stdout/stderr/exit. Analysis FIFO
counts exclude startup; the separate whole-command observation is 88.1270/
87.9739 billion instructions, 8.4391/8.4603 seconds wall time and 624,448/
620,508 KiB peak RSS. It also validates that the five files produce a result
cache, rather than consuming a previous result.

Rust 1.98.1/LLVM 22.1.8, default features, max-perf fat LTO, one codegen unit,
line tables and function alignment 6 are identical on both sides. Fresh PGO
uses the same eighteen independent programs; PHPStan and all seven controls
are excluded. Instrumented/final builds take 258.13/195.56 seconds. All 54
training observations agree with PHP. The 27 final build-script profile warning
lines remain visible and are not a runtime-output failure. Full environment,
PHP 8.5.11 extension/configuration data and input hashes are in the packet.

Four compiler proof tests and eight focused integration packets pass under
default, no-default and all features. The final recursive-error regression
passes separately in all three configurations: **878 actual successful test
executions**. Coverage includes mixed types and NaN, aliases/references/COW,
short circuiting and loops, temporary destructor and warning order, finally,
exceptions, Fiber and recursive identity failure. Seventeen programs match PHP
on the final PGO CLI; seven also match with existing quick/direct/composed
execution disabled. All-target/all-feature compilation, formatting and unsafe
diff gates pass. Canonical execution remains independently tested.

Two early proof gates fail because an overly broad target scan interprets
numeric Return metadata as a jump target. The original coverage assertion is
preserved and the target decoder is corrected. A later audit finds missing
diagnostic continuation targets before all-target completion; that entire
service is terminated and the interrupted gate is not counted as a pass.
The corrected source then repeats all relevant gates, selection and fresh PGO.
The pre-repair exploratory source is hash-reconstructed and its original
reports remain; its exploratory binary was superseded. It is not acceptance
evidence. The exact repaired baseline/candidate pair and source are retained.

## Independent controls and limits

First/confirmation windows use three randomized pairs each. Timing excursions
trigger one predetermined, unselective five-pair review of all seven controls.
All **154** valid observations are retained. The shared-frame review time
**+1.735%** is an explicit integrating-task tradeoff under the user's instruction
priority, with practically unchanged instructions. It does not pass the
one-percent time gate. No layout or latency cause is established.

| Control | Review instructions | First time | Confirmation time | Review time |
| --- | ---: | ---: | ---: | ---: |
| class constants | +0.000% | -0.276% | -0.808% | -1.015% |
| inherited methods | -0.224% | -3.158% | -1.683% | -4.740% |
| nested regex | -0.286% | -2.551% | -1.735% | -3.398% |
| return scope | +0.058% | +1.954% | -2.205% | -1.516% |
| shared frames | +0.000% | +1.984% | -0.209% | +1.735% |
| shared TMP reads | +0.000% | -0.877% | -1.369% | -0.516% |
| trait property scope | +0.014% | +1.842% | +0.898% | +0.970% |

Executable size is 76,500,664/76,514,248 bytes; executor code grows 76 bytes,
391,095/391,171. Release/pop helper sizes remain identical. Native sampling
retains 69,675 observations and exact-binary inline attribution. Shared LLVM
tail provenance and inclusive helpers overlap; neither is an exact additive
instruction budget or proof that a literal Value store costs thousands of
instructions.

Every expensive job runs in a verified separate 6 GiB/no-swap OOM-group
boundary with exclusive benchmark lock and whole-group timeout termination.
The largest recorded peak is 5,858,508,800 bytes, without OOM or timeout.
Cleanup runs before/after checks and on checkpoint completion. No private
benchmark host is configured. Main integration remains separate; the original
PHPStan parity goal continues with measured general frame/runtime costs.
