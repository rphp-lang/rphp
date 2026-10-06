# PHPStan: replay validated ordinary class constants

Accepted instruction checkpoint on `codex/perf-phpstan-cold`, baseline
`90e4e9fb`. The original parity goal remains active: independently confirmed
analysis executes **70.6793 billion instructions versus PHP's
10.3379 billion (6.84x)**. This is a narrow instruction
improvement; analysis time remains approximately 6.6 seconds versus PHP's
0.9475 seconds. Every retained sample is in
[the evidence packet](performance-phpstan-class-constant-cache-replay-samples.json).

## Measured cause and implementation

A temporary exact-output caller probe of the baseline whole command observes
3,058,407 class-name lookups in the class-constant pre-cache autoload check.
The current baseline retired-instruction profile samples the ordinary wrapper
327 times and its result-publication closure 656 times out of 71,457 samples.
An older exact phase attributes 581,975,806 exclusive and 1,353,832,701 inclusive
instructions to 2,951,973 constant reads. These overlapping, differently scoped
budgets are evidence of the affected cost, not additive predicted savings.

Ordinary `FetchClassConst` now replays an existing positive cache entry before
operand dereference, class-name lookup and full resolution. Both operands must
be immutable `Const`, the registered class ID must be nonzero and the existing
constant flag must equal 1: visibility already checked, immediate value and no
deprecation effect. The same declaration Value is cloned and the canonical
result writer retires the old owner. No new cache format or growth policy is
introduced. The complete implementation is shared by the four wrappers instead
of duplicating its large error handling in each wrapper.

Late/relative scopes and dynamic names/owners use their original opcode
wrappers. Empty cache, deferred values, deprecated definitions and enum cases
retain complete canonical execution. Anonymous private accesses remain
uncached, so rebound closures repeat the scope check. No cache borrow crosses
PHP callbacks. One additional unsafe block documents the existing live-frame,
compiler-sized cache and positive declaration invariants; the unchanged policy
passes at 1,749 blocks and 321 unsafe functions. Value, frame, cache and opcode
sizes remain 16, 64, 16 and 16 bytes. This changes portable baseline Rust, with
native evidence on x86-64 only; no ABI, typed-IR or JIT lowering changes.

## Selection and independent confirmation

| Measurement | Baseline instructions | Candidate instructions | Change |
| --- | ---: | ---: | ---: |
| Ordinary release, two interleaved pairs | 77.8886 G | 77.0697 G | -1.051% |
| Independent 18-input PGO, first two pairs | 71.4743 G | 70.6834 G | -1.107% |
| PGO independent confirmation, two pairs | 71.4670 G | 70.6793 G | -1.102% |

The first PGO analysis medians are 6.5843/6.5536 seconds; independent confirmation
is 6.5773/6.5895 seconds. Timing does not demonstrate a material analysis gain.
The whole-command one-observation check is 89.8370/89.0164 billion instructions,
8.5333/8.5047 seconds wall time and 622,692/622,448 KiB peak RSS. PHP is
15.4741 billion whole-command instructions and 177,604 KiB RSS. Whole-command
and analysis-only counts are distinct and must not be compared interchangeably.
All outputs preserve the same five files, twenty findings and expected exit 1.

Hardware counters use `instructions:u`, CPU 2, a phase FIFO boundary and 100%
counter running time. Two interleaved pairs per phase experiment and three
randomized pairs per independent control experiment retain every valid run;
there is no favorable-outlier selection. Each process has a fresh result cache.
PGO uses the unchanged fixed eighteen training inputs; PHPStan and all six
controls are excluded. Rust 1.98.1/LLVM 22.1.8, `max-perf`, fat LTO, one codegen
unit and function alignment 6 match the named baseline. Instrumented and
profile-use compilation take 260.673/196.307 seconds. Profile use reports 26
build-script profile warnings plus their summary; no runtime profile checksum
warning is hidden.

## Independent controls and integration tradeoff

| Untrained control | Confirmation instructions | First time | Confirmation time |
| --- | ---: | ---: | ---: |
| `bench_class_constant_replay.php` | -31.2137% | -28.863% | -29.370% |
| `bench_inherited_method_metadata.php` | -0.0000% | -0.176% | -0.252% |
| `bench_regex_nested_continuation.php` | -0.0000% | +1.346% | +2.863% |
| `bench_shared_frame_release.php` | -0.0000% | +1.466% | +0.332% |
| `bench_shared_temp_read_results.php` | +0.0001% | -2.394% | -3.315% |
| `bench_trait_property_scope.php` | +0.0139% | +0.009% | -1.434% |

The sole integrator accepts the confirmed regex timing regression under the
user's explicit instruction priority. Its instruction count is unchanged,
while the independent constant holdout saves 31.214% instructions and about
29.371% time in confirmation. There is no regex or main-executor source edit.
Changed code placement is a plausible timing explanation, not a demonstrated
cause. The 2.863% confirmed regex time cost exceeds the ordinary one-percent
gate and is an explicit bounded tradeoff, not a passing regression check.
Shared-frame timing is +1.466% first and +0.332% in independent confirmation;
all samples remain visible. No compensating runtime patches are stacked.

The main executor remains 390,878 bytes. The ordinary constant wrapper shrinks
14,225 to 1,583 bytes; complete false/true implementations are shared once.
Text shrinks 30,411 bytes and the executable shrinks 120,152 bytes, to
76,496,544 bytes. No unbounded cache, TLS or request metadata is added.

## Correctness, fallback and cleanup

Four exact CLI contracts match PHP and the unmodified canonical baseline in
both ordinary release and final PGO. The new public contract exercises repeated
scalar/array constants, COW mutation, alias autoload, missing-class failure then
later definition, inherited protected values, dynamic late owners, alternating
allowed/denied rebound closures, and repeated deprecation callbacks with nested
constant reads. Cold cache and the complete dynamic/deprecated paths exercise
canonical fallback in the same source. Existing frame admission, numeric result
and reference/COW getter contracts also retain exact output.

Eight focused integration packets pass with default, no-default and all
features: **154 per configuration, 462 actual executions**. All-target,
all-feature compilation, formatting and unsafe-diff checks pass. No broad
unrelated matrix is added. Existing PHP compatibility gaps remain failures.

Every expensive build, diagnostic and benchmark runs under a verified separate
6 GiB aggregate user-systemd boundary, no swap, whole-cgroup kill and an
exclusive benchmark lock. All recorded boundaries have zero OOM events.
Cleanup runs before/after checkpoints; superseded PGO/test targets are removed.
Exact binaries and source snapshots remain, along with only the active ordinary
release dependency cache. No private benchmark host is configured. Staged
source/evidence hygiene is verified before commit; no main-branch merge occurs.

## Faster candidate iteration

An ordinary `opt-level=3` release selector now replaces the misleading
`test-fast` instruction proxy. On the same phase the ordinary baseline is
77.8884 G versus 71.4715 G PGO (+8.978%); the old checked opt-level=1 proxy is
111.08 G and differs by 55.42%. This is a selector, not the final release claim.
Cold preparation takes 80.331 seconds, the current source rebuild 72.260
seconds and two native pairs approximately forty seconds. A predefined 0.5%
saving is required before spending time on independent PGO and focused gates.

A prior resolved-TMP publication prototype was rejected at -0.369%, below that
filter, with exact PHP output. Its source was fully restored before this intake;
no PGO or later feature pass is claimed for it. Continue the active parity goal
with quantified general executor/call costs rather than accepting smaller
unconfirmed publication rewrites.
