# Vacant temporary owner writes

Status: accepted bounded instruction checkpoint with explicit control tradeoffs;
overall PHPStan instruction/time parity remains open.
Baseline: clean `54f2e19f`, source
`88887d4a07c385345b4508e0ae507c848922c997a219409b939ffc7a9f8c2b0b`,
executable
`80dbcc8339ba54b471a48eedadc2ddd9b5e82a9eb40054dd836f84820e834836`.
Confirmed analysis is 76.1537 billion instructions / 6.9655 seconds,
versus reference PHP's 10.3380 billion / 0.9321 seconds.

## Evidence and hypothesis

A fresh instruction-event profile of this exact executable, enabled only during
analysis, samples 37.52% in the main executor. This is sampled attribution, not
exact per-line accounting; eighteen lost samples and instruction skid remain
visible limitations. Result writes and ownership bookkeeping occur in the
high-sample blocks. The earlier exact phase profile records 37.826 million
calls to bitmap_drop_and_update, with 1.452 billion inclusive instructions;
that older total is a hypothesis input, not the current executable's budget.

A separate default-feature diagnostic snapshot of the current baseline records
57,182,630 generic TMP writes over the full command with identical PHP/baseline
output. Of these, 10,195,942 are heap writes into a clear tracked-prefix bit.
The initialized tail has 3,807,092 heap and 13,375,024 scalar writes into a value
whose needs_cleanup flag is clear. Only 830,854 tail and 575,042 prefix writes
have previous owners. These are actual write classifications, not native timings
or analysis-only counts.

The TMP writer currently calls the outlined retirement routine for every incoming
heap value or tail slot, including proven vacant destinations. Reuse the same
clear-bit proof already used by the general frame-slot and indexed TMP writers.
For the initialized tail, the existing Value ownership flag proves whether a
previous Rust owner exists. Preserve ordinary replacement for occupied slots.
Reduce repeated ownership bookkeeping without a new source recognizer, layout,
allocation, execution tier or workload identity condition.

## Semantic envelope, ownership and gates

The live frame and result pointer retain their existing valid-slot precondition.
Prefix TMP bytes may be uninitialized: inspect only their bitmap, never the old
value. A clear prefix bit proves no old owner; mark an incoming heap edge before
the normal write. Slots beyond the prefix retain the allocator's initialized
Value guarantee, so inspect their ownership flag before deciding whether the
existing retirement boundary is needed. An occupied slot retains identical
drop, reference/GC admission and replacement order. First heap writes, borrowed
receiver metadata, wide-frame initialization, conservative has_heap_slots,
external handlers and all other writers remain unchanged.

The sole integrating task owns this TMP writer and focused verification in the
isolated performance worktree. The getter checkpoint is accepted and closed.
No second implementation goal is active. No additional unsafe precondition,
Value/frame/opcode/native ABI change or new production unsafe block is allowed.

Use the existing meaningful ownership-unit case across prefix boundaries and
wide geometries, focused frame cleanup/prefix/reference/single-temp/generator
lifetime suites under default/no-default/all features, exact PHP/baseline/candidate
CLI cases, formatting, unsafe inventory and all-target compilation. Keep the
known callback-last-owner gap baseline-equal and explicitly failing. Avoid new
tests that merely duplicate the writer's branch structure.

Freeze source for a fresh identical-policy eighteen-input PGO build, excluding
PHPStan and holdouts. Interleave exact analysis-phase counters, hot analysis
timers, whole command and established independent controls. Retain all valid
samples, independently confirm changes outside the noise envelope, and measure
actual avoided retirement entries in a separate diagnostic. Native evidence is
x86-64; ARM64 is unavailable. Use the verified 6 GiB/no-swap aggregate process
boundary and exclusive benchmark lock, followed by mandatory cleanup.

Reject changed owner/reference/destructor/exception behavior, reading uninitialized
prefix bytes, loss of wide initialized-slot fallback, expanded unsafe invariants,
failed focused gates or no reduction in application instructions. Visible modest
control timing tradeoffs follow the user's instruction priority. Preserve exact
sources/binaries/raw evidence and continue the overall parity goal after either
acceptance or rejection.

## Result

The generic TMP writer now marks a vacant tracked-prefix destination directly
and reads only the existing ownership flag for initialized tail destinations.
Previous owners retain the original retirement routine and order. Prefix TMP
bytes are never inspected to establish vacancy. Other writers, first heap
admission, frame layouts and unsafe preconditions remain unchanged.

Analysis-only hardware instructions improve **76.1751 to 75.9425 billion** in
the first window and **76.1797 to 75.9427 billion (-0.311%)** in reversed-order
confirmation. PHP takes **10.3364 billion** instructions in that confirmation.
Four samples per RPHP executable and two PHP samples give analysis medians
**7.08563 to 6.96017 seconds (-1.771%)**, against PHP's **0.93401 seconds**.
Whole-command counters improve 94.7426 to 94.5326 billion (-0.222%). RSS rises
304 KiB. Fresh analysis storage, five files, twenty findings, ordinary output
and exit status match. No valid sample is discarded, including the slower
candidate's first application run with more activity on the paired CPU.

Separate actual-path counters preserve all 57,182,630 write classifications
from the baseline diagnostic. The writer invokes retirement **1,405,896** times
for actual previous owners and avoids **27,378,058** formerly unnecessary
entries. The original predicate would enter 28,783,954 times. These counts
include bootstrap and come from a distinct default-feature test-fast diagnostic;
they are not analysis-only timing claims. Fourteen compact/wide programs match
PHP and both exact PGO binaries. Occupied prefix and tail paths remain exercised.
The preexisting callback-last-owner gap is byte-equal to baseline and explicitly
fails PHP comparison.

Every initial control outside the one-percent timing envelope, including gains,
is confirmed with five pairs. Independently confirmed shared temporary reads
use **1.33% fewer instructions but 3.85% more time**. Scalar frame returns use
**0.75% more instructions and 1.65% more time**. Shared frame retirement uses
0.74% more instructions and 6.52% less time; this reverses the first window's
small timing regression, so no stable shared-frame timing improvement is claimed.
Relative return and inherited-method time improve 5.77% and 5.66%; regex time
rises 0.59%. The remaining first-window controls and all distributions are
retained, with separately scaled inputs never pooled.

A distinct instruction-supply diagnostic window gives shared temporary reads
2.54% more time with 8.29% more frontend empty slots, despite 1.33% fewer retired
instructions. Instruction-cache misses fall 9.55%; an instruction-cache-miss
increase cannot explain this result. Scalar-frame time differs only 0.16% in
that diagnostic while its +0.75% instruction difference persists. These
observations do not establish a specific causal placement explanation. The
integrating task accepts the visible control tradeoffs under the user's
instruction priority, without claiming all workloads improve.

All **159** focused default/no-default/all-feature executions, formatting,
unsafe inventory and all-target/all-feature compilation pass. Existing boundary
tests cover uninitialized prefix storage, indices 63/64/129, old and new owners,
references, frame retirement and generator/aggregate lifetimes. No redundant new
test suite is added. The main executor shrinks **390,624 to 390,511 bytes**;
the retirement helper shrinks 2,025 to 1,979 bytes.

Exact source is
`37ccaf9d6be808d4d9cf138ced425674b6bbe65e9bfc08382b801aa9a3116b89`;
executable is
`1389315dc329005b2ff8eb398041fc9c14161d73592c486146346dbfdd3e5086`.
Fresh PGO uses the unchanged eighteen independent inputs, excluding PHPStan
and every holdout. The training-input checksum preflight used reference PHP and
the older `9790b30a` method-memo executable; the recorded hashes match the fresh
instrumented candidate's outputs. It was initially mislabeled as the performance
baseline in the build metadata. The packet now distinguishes that preflight
executable from the actual `54f2e19f` A/B baseline; all phase and application
comparisons used the correct latter executable. This correction changes no
native sample, PGO input or source fingerprint. Twenty-six missing-profile
warnings concern only the
untrained build script; no executable profile mismatch is reported. Preparation
peaks at 5,539,594,240 bytes under the verified 6 GiB/no-swap boundary without OOM.
Mandatory cleanup runs in both checkouts and deletes only disposable Cargo
targets, retaining exact sources, binaries, profiles and raw measurements.
No private benchmark host is configured. Native evidence remains x86-64 only;
ARM64 measurements are unavailable. All evidence is in
[the checkpoint packet](performance-phpstan-vacant-tmp-writes-samples.json).
The remaining **7.35x** analysis instruction ratio leaves overall parity open.
