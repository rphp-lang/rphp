# PHPStan temporary-owner retirement checkpoint

Status: accepted by integrating task. Baseline `6d717176`
on `codex/perf-phpstan-cold`. Overall PHP parity remains open.

## Outcome and evidence

Reduce the general cost of retiring temporary `Value` owners after completed
expressions without changing PHP-visible destructor, reference, COW, exception,
or callback timing. The exact baseline PGO executable executes 82,538,948,509
user instructions during the five-file PHPStan analysis; PHP executes
10,336,332,736. Their output is identical. The baseline's exact Callgrind run
records 38,176,316 `ReleaseTemps` dispatches and at least 7,091,730,630
instructions in its inlined opcode body. The latter excludes called functions
and shared executor tails. Machine branch counts indicate that many ranges
contain owned values; eliminating the marker without an ownership proof is
unsound. Native sampling also places the executor among the leading costs.

## Hypothesis and semantic envelope

The first fresh-cache diagnostic counts only 1,314,583 prefix single-string
ranges outside marked foreach return cleanup. Reject a string-only change as
too small for the application gap. It also counts 6,041,558 prefix single-array
and 2,332,717 prefix single-object ranges. Count their remaining owners before
admitting a broader proof: a direct array or object with another physical owner
cannot lose its children when this single temporary retires. The existing
nested-operand planner already uses this ownership proof. A single-owner path
can apply it without rescanning the range or preparing a release graph.

Preserve ordinary `Value::drop` and its GC admission; prove read-snapshot
suppression immediately before that drop, using the current canonical predicate.
For nested operand cleanup, abandon pending calls before checking the remaining
owners. Final owners, references, marked foreach return sources and ambiguous
ranges retain the existing planner. No ownership or snapshot proof survives a
PHP callback. No source, class, literal, benchmark, or function name participates
in the decision. Feature-gated diagnostics add no ordinary-build branch.

The second fresh-cache diagnostic confirms 5,730,192 prefix single-array and
2,284,111 prefix single-object ranges with another physical owner. Of these,
7,564,567 occur outside nested pending-call cleanup. The remaining 449,736 are
only potential admissions: dropping pending call arguments can change their
owner counts, so the runtime must check again after cleanup. Instrumentation
records 38,176,261 `ReleaseTemps` executions with identical PHP output; it is
used for shape counts, not as a timed performance candidate.

The baseline preflight exposes an existing compatibility gap in
`tests/fixtures/single_temp_release/callback-last-owner.php`: replacing a nested
array during a by-reference callback omits `drop:old|` on baseline RPHP. PHP's
expected output is retained beside the reproducer. The preflight failure is
preserved; the candidate must keep the exact baseline result for this unsupported
case, which is never counted as a PHP pass. A separate differential covers
callback replacement while another PHP owner still holds the old array. Repair
of the last-owner callback gap requires a separate compatibility checkpoint;
this optimization does not encode its source shape or alter its result.

This checkpoint owns `src/vm/execute/call_frames.rs`, the relevant
release helper and optional `vm-stats` instrumentation in this isolated
worktree. No other agent is editing the checkout. The canonical baseline path
remains the semantic oracle; no new unsafe invariant or frame layout is planned.

## Gates and stop rule

Compare direct-release fixtures with PHP for strings, references, COW aliases,
destructor-containing arrays, objects, resources, exception/finally cleanup,
and callback re-entry. Run focused tests, default/no-default/all-feature checks,
formatting, unsafe-policy and all-target checks. Build exact independent PGO
baseline/candidate binaries, compare application and independent control output,
analysis-only hardware instructions, time, RSS, holdout programs and code size
under the repository's aggregate memory boundary and benchmark lock. Include
all valid measurements and investigate any corpus regression over one percent.

Reject and remove the candidate if the diagnostic finds too few shared owners,
correctness changes, or the measured instruction win is below noise or
costs material memory/time on independent controls. Keep this checkpoint local
until the joint gates pass; do not claim overall PHP parity from a narrow win.

## Implementation and verified result

Extend the existing single-bit temporary ownership proof to direct strings and
nonfinal arrays/objects. Callback-free resources retain their existing admission.
Nested pending calls retire before testing the current physical owner count.
The current read-snapshot predicate immediately precedes ordinary `Value::drop`;
the slot then becomes undefined and its ownership bit clears. Final owners,
references, multiple roots, wide ranges and marked foreach return sources retain
the canonical planner. There is no new unsafe block, layout or execution-tier
admission. Ownership-shape counters are compiled only with `vm-stats`.

In the same analysis-only counter window, baseline RPHP uses **82.4797 billion**
instructions, candidate **80.6218 billion (-2.25%)**, and PHP **10.3338 billion**.
Whole-command confirmation medians are 101.1572 versus 99.2534 billion
instructions (-1.88%). Confirmation analysis times are **7.4976 versus 7.3226
seconds (-2.33%)**, against PHP's 0.9172 seconds. The first time window shows a
smaller 0.66% improvement; every valid sample is retained. Confirmation RSS
increases by 304 KiB (+0.05%). Output, five files, twenty findings, stderr and
status remain identical. Each runtime uses a fresh temporary cache directory.

The independent shared temporary-read holdout uses **17.80% fewer instructions**
and 15.43% less time in the enlarged confirmation run. It is excluded from the
unchanged 18-program PGO training. Existing shared-frame, relative-self, nested
regex and inherited-method controls use 2.12–4.90% fewer instructions. Property
reads instead use 0.40% more instructions and take 2.05% more time in confirmation;
the first time window also regresses. Scalar-frame instructions increase 0.79%
with effectively unchanged confirmed time. The integrating task accepts this
explicit, reproducible control tradeoff under the user's instruction priority.
This is an application and ownership-cost improvement, not an all-program win.

Validation records **356 focused test executions**: 94 each under default,
no-default and all features, plus 37 resource/callback contracts each under
default and all features. Eight compact/wide PHP differential cases match;
the exact PGO binary preserves the known last-owner callback gap documented
above. Formatting, all-target/all-feature compilation and unchanged unsafe
inventory pass. The original failed baseline preflight and the diagnostic run
with an inadvertently shared result-cache directory remain retained as failures
or rejected evidence, not silently replaced by later passing results.

Source SHA-256:
`6b40e5f07116b0db3d6293a2b27d3e0355995318911b22612bf7b51405ea6549`.
Executable SHA-256:
`91f6a8424590da64f45347fa5858aeb20c29c367bcbfadd9f0cca50c68c31fc4`.
Fresh instrumented and profile-use builds take 255.90 and 193.13 seconds. The
executable shrinks by 1040 bytes and the executor by 618 bytes. Preparation
peaks at 4,896,120,832 bytes under the 6 GiB aggregate limit; all relevant jobs
have zero OOM events. Native measurements remain limited to x86-64. Full
distributions, exact checks, diagnostic counts and preserved failures are in
[the review packet](performance-phpstan-temp-release-samples.json).

Integrating review confirms that the single physical-owner proof pins all
children, pending cleanup precedes that proof, and the read-snapshot predicate
does not cross PHP re-entry. Both local cleanup hooks finish; disposable
diagnostic/profile-use Cargo targets are removed only after preserving exact
sources, executables and profiles. No private benchmark host is configured.
The staged diff and public-data checks are required before commit and push.

The remaining analysis instruction ratio is **7.80x PHP**. The next selection
requires a fresh analysis-only profile of the exact candidate rather than
extrapolating opcode counts into an explanation of the complete remaining gap.
