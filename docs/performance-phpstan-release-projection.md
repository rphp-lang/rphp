# PHPStan: compile release interval ownership masks

Accepted instruction checkpoint on `codex/perf-phpstan-cold`, baseline `b5890a2e`.
Independent confirmation reduces analysis **70.2264 to 69.8711 billion
instructions (-0.506%)**, with exact PHP output.
Analysis medians are **6.4870/6.5052 seconds**;
PHP is 10.3379 billion instructions/0.9257 seconds.
The **6.76x instruction gap** keeps parity active. The initial
PGO window is slightly faster and confirmation slightly slower; this checkpoint
does not claim a confirmed PHPStan time improvement. Every retained window is
in [the packet](performance-phpstan-release-projection-samples.json).

## Attribution and representation

The exact accepted PGO Callgrind profile records 36,411,017 ReleaseTemps
entries and 3,975,145,657 instructions in its uniquely reachable machine-code
body. This is a conservative CFG lower bound excluding called helpers and
shared tails, within a 71,590,566,285-instruction profiler budget. Profiler and
native counts are distinct. Two reference harness failures omitted the FIFO;
corrected same-PHAR PHP validation matches the profiled stdout, stderr and exit.
Those original failures remain visible and are not passing profile gates.

The compiler now projects each immutable absolute release interval into the
otherwise unused property words of its existing opcode-local InlineCache.
Its function pointer stays null; cache initialization and reinitialization
populate the mask, and offset finalization refreshes it after relative TMPs
become physical slots. Release cache entries own no retained name on drop.

Dispatch tests live prefix ownership with that mask. A wide range probes only
its initialized tail beyond slot 63. The canonical release helper reuses the
same mask for sole-owner classification and refreshes the live bitmap after
callbacks before capturing read-snapshot proofs. Bounded interval projection
already excludes high-half class metadata of compact frames. Internal dynamic
intervals compute the same mask through the existing geometry wrapper.

Every release mode, pending call, reference/COW rule, snapshot/GC admission,
callback/destructor ordering, exception poll, finally and Fiber boundary remains
canonical. No opcode, heap/frame/cache/Instruction layout, workload recognizer,
new type guard or unsafe inventory is added. Value/frame/cache/Instruction
sizes remain 16/64/16/16 bytes and inventory stays 1,749 blocks/321 functions.
This is portable Rust; native evidence is x86-64, with no ARM64 speed claim.

## Selection and verification

The geometry-only first attempt saves 0.475%, below the predetermined 0.5%
selector, and is fully restored before the follow-up. Projecting ownership
inside the helper also removes its redundant frame-scope geometry. Ordinary
release then saves **0.606%**, PGO selection **0.517%**, independent confirmation
**0.506%**. Two alternating pairs per window retain all exact-output samples;
no result-cache hit is admitted. The final production source differs from the
initial selector only in a cfg(test) fixture correction, not runtime code.

PGO retains the fixed eighteen-input recipe, Rust 1.98.1/LLVM 22.1.8, fat LTO,
one codegen unit and function alignment 6. Neither PHPStan nor the seven controls
train it. Build-script profile warnings remain visible; no runtime checksum
mismatch occurs. The one-observation whole-command check is 88.5457/88.1720
billion instructions, 8.4993/8.4125 seconds wall time and 622,392/622,568 KiB RSS.
Whole-command counts and the analysis-only paired result remain separate.

Two metadata tests and eight targeted integration packets pass under default,
no-default and all features: **377 actual executions**. They cover empty,
embedded-scope and 63/64/tail masks, absent/recreated caches, shared/final owners,
references, throwing destructors, pending operands, finally and Fiber re-entry.
The initial lifecycle test used Echo, which emitted no release marker; its
mandatory assertion rejected missing coverage. The same call is changed to an
expression statement and the assertion stays. The failed run is retained.
All-target/all-feature compilation, formatting, unsafe diff and ten exact PHP
CLI contracts on the final PGO executable pass.

## All untrained controls

The first and independent windows each use three randomized pairs. Inconsistent
trait timing triggers a larger unselective five-pair review of all seven.
All 154 valid observations remain visible. The confirmed inherited-method time
regression is a **1.134% tradeoff**, while instructions improve **0.805%**.
Under the user's explicit instruction priority it is accepted as an exception,
not reported as a passing one-percent time control. No causally proven placement
or cache-latency explanation is claimed. All control instruction budgets decrease.

| Control | Review instructions | First time | Confirmation time | Review time |
| --- | ---: | ---: | ---: | ---: |
| `bench_class_constant_replay.php` | -4.856% | -3.598% | -3.858% | -3.872% |
| `bench_inherited_method_metadata.php` | -0.805% | +1.425% | +1.060% | +1.134% |
| `bench_regex_nested_continuation.php` | -0.462% | -1.555% | -3.115% | -1.253% |
| `bench_return_scope_projection.php` | -2.879% | +0.267% | +1.408% | +0.204% |
| `bench_shared_frame_release.php` | -1.362% | +1.006% | -2.440% | -2.841% |
| `bench_shared_temp_read_results.php` | -3.297% | -3.350% | -3.289% | -1.749% |
| `bench_trait_property_scope.php` | -0.505% | +0.378% | +5.462% | +0.203% |

Every expensive job uses a separate verified 6 GiB/no-swap user-systemd cgroup,
whole-group OOM/timeout kill and exclusive benchmark lock. Recorded boundaries
have no OOM events. Cleanup runs before/after checks; superseded PGO/test targets
are removed. Exact baseline/current binaries and snapshots remain, alongside the
active ordinary-release build cache. Public evidence omits host and private paths.

Fast native inline attribution completes in 0.892 seconds after a native sample
run; inclusive helper shares overlap and LLVM shared-tail provenance is not an
exact opcode budget. Continue with measured general frame/runtime ownership
costs. Main integration and the original simultaneous time/instruction parity
goal remain outstanding.
