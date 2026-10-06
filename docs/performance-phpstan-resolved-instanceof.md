# Resolved instanceof operand projection

Status: accepted bounded instruction checkpoint with explicit control tradeoffs;
overall PHPStan instruction/time parity open.
Baseline: clean `b7052e5b`, source
`37ccaf9d6be808d4d9cf138ced425674b6bbe65e9bfc08382b801aa9a3116b89`,
executable
`1389315dc329005b2ff8eb398041fc9c14161d73592c486146346dbfdd3e5086`.
Confirmed analysis takes 75.9427 billion instructions / 6.9602 seconds against
PHP's 10.3364 billion / 0.9340 seconds.

## Outcome, evidence and hypothesis

Use the existing successful literal target-ID resolution and receiver ID before
projecting a class-name operand that numeric membership does not consume. Keep
the same canonical ancestry query, including its unresolved-link refresh. No
new relation memo, hotness condition, workload recognizer, IR operation or layout
is admitted by this checkpoint.

The exact previous getter executable's analysis-limited instruction-event
profile estimates 1.684 billion exclusive instructions in op_instanceof. The
subsequent TMP checkpoint changes only the result writer, leaving this handler's
source unchanged; its native size is 2,725 versus 2,724 bytes. Sampling is an
estimate with instruction skid and eighteen lost samples, not a per-line budget.
The older exact phase profile records about seven million instanceof entries
and 1.074 billion exclusive / 1.599 billion inclusive instructions.

A separate current-baseline default-feature diagnostic records 7,099,827 checks
over the whole command, with identical PHP/baseline/diagnostic output. There are
6,537,782 registered literal receiver/target pairs: 403,273 same-class and
6,134,509 ancestry queries. The remaining cases include 219,461 relative scopes,
8,302 runtime targets, 7,278 literal closure operands, 2,396 unresolved literal
targets and 324,608 unregistered or non-object operands. A private validated
relation-memo simulator has only 2,529,887 monomorphic hits, 4,004,060 first or
changed receivers and 3,835 class-table generation misses; these are simulated
reuse opportunities, not production optimized coverage. The present slice
removes operand/name projection for registered literal pairs without caching
membership results or introducing a new invalidation contract.

## Semantic envelope and ownership

The existing constant-target cache remains opcode-local and retains only a
successful non-relative declaration ID. Missing target names are resolved on
each check so later declarations and class_alias publication remain visible.
Relative scope errors, runtime strings/objects, closures and class-id-zero
objects retain their canonical operand/name behavior. The same class_is_a_ids
query remains authoritative, with complete ancestry immutable and unresolved
ancestry refreshed when the class table grows.

Read the receiver's class ID only after the existing Object tag proof, within
the live frame's non-reentrant metadata query. No PHP initializer, autoload,
error handler or callback occurs on the registered numeric branch. Finish through
the existing result writer with the same slot, value and ownership order.
No borrowed cache reference survives the result write or a canonical callback.
Keep Value/frame/opcode/cache/native ABIs and unsafe preconditions unchanged.

The sole integrating task owns baseline_dispatch_cold.rs and focused fixtures
in the isolated performance checkout. The TMP checkpoint is accepted and closed;
this is the only active implementation goal.

## Gates and rejection

Use focused default/no-default/all-feature instanceof, class-alias/dynamic class
and relevant lazy-object tests, plus exact PHP/baseline/candidate CLI cases for
changing receivers, references, unknown/late names, aliases, closures, runtime
RHS operands and relative scope errors. Preserve any preexisting PHP gap as
baseline-equal and visibly failing. Check formatting, the unchanged unsafe
inventory and all-target/all-feature compilation. Avoid redundant test expansion.

Freeze source for a fresh identical-policy eighteen-input PGO build, excluding
PHPStan and every holdout. Measure native analysis-only instructions and fresh
analysis timers, whole-command/RSS and independent controls. Confirm changes
outside the noise envelope with separately retained windows and count actual
registered-ID completions on a distinct diagnostic build. Native evidence is
x86-64; ARM64 is unavailable. All expensive work uses the verified 6 GiB/no-swap
aggregate boundary and exclusive benchmark lock with mandatory cleanup.

Reject semantic or ownership changes, altered dynamic/relative/closure behavior,
unresolved-name or ancestry invalidation loss, a broader unsafe invariant, failed
focused gates or no reduction in application instructions. Retain modest measured
control tradeoffs under instruction priority, never an unexplained large timing
regression. Preserve exact sources/binaries/raw evidence and continue parity.

## Result

An already resolved non-relative literal now enters the existing numeric
ancestry query directly from the receiver's class ID. Canonical target/name
projection remains for every unresolved, relative, runtime, closure and
unregistered operand. There is no new relation cache or invalidation rule.

Analysis-only hardware instructions fall **75.9568 to 75.5536 billion** in the
first window and **75.9768 to 75.5715 billion (-0.533%)** in reversed-order
confirmation. PHP takes **10.3373 billion** in confirmation. Four samples per
RPHP executable and two PHP samples give analysis medians **7.02202 to 6.96963
seconds (-0.746%)**, against PHP's **0.91947 seconds**. Whole-command medians
fall 94.5278 to 94.1237 billion (-0.428%); RSS rises 212 KiB. All valid samples
remain, including the slower first candidate analysis. Fresh analysis storage,
five files, twenty findings, ordinary output and exit status match PHP.

Actual default-feature diagnostic coverage records **6,535,341 numeric
completions** and 564,496 canonical entries, totaling 7,099,837 checks over the
whole command. Forced canonical execution completes all 7,099,837 through the
original handler, with identical output. Both compact and seventy-CV fixtures
exercise 54 direct completions and 34 canonical entries. Diagnostic counts
include bootstrap and use a distinct test-fast executable, not a native timing
build.

The initial count-equality check failed by ten bootstrap queries. A separate
four-run site diagnostic localized the difference to five DI/schema sites:
the canonical-mode environment flag added one key to PHPStan's environment
configuration. Both modes now carry the same key with zero/one values and the
original exact-count equality assertion passes. The failed run, its boundary,
site probe and corrected coverage are retained separately. No production code
changed to accommodate the diagnostic.

All **129 focused feature executions**, sixteen exact PHP/baseline/PGO-candidate
CLI cases, formatting, unsafe inventory and all-target/all-feature checks pass.
The initial wide wrapper changed top-level self into a named-function expression
and exposed an existing compile-time gap: PHP exits 255, while baseline and
candidate catch a runtime Error. Its failed preflight and minimal fixture remain
visible. Corrected padding enlarges only resolvedCheck and preserves the global
expression. This gap and the preexisting last-owner callback mismatch remain
baseline-equal failures; neither is counted as a PHP pass.

Every first control outside the one-percent envelope, including gains, has an
independent five-pair confirmation. Declared relations improve **3.813%
instructions and 2.664% time**. The alternating getter control improves 5.222%
time with essentially unchanged instructions. Shared-frame time regresses
**6.620% with unchanged instructions**; inherited-method time regresses **1.978%
with 0.185% more instructions**. These are explicit instruction-priority
tradeoffs, not a general speedup claim. Separately scaled windows are not pooled.

The independent frontend window retains shared-frame time +6.854%, empty
frontend slots +100.245%, operation-cache misses +123.779% and instruction-cache
misses +18.332%, with unchanged instructions. Inherited-method empty slots rise
8.060% while instruction-cache misses fall 3.094%. These support an
instruction-supply association; exact causal code placement is unproven.
The main executor stays 390,511 bytes and its retirement helper 1,979 bytes;
the resolved instanceof handler grows from 2,725 to 4,102 bytes.

Source is
`9f4ebb69821b9505671cd633adba6c577427d31fe903966d6bffc293d6988664`;
executable is
`70599ddfc0c916698ea94ae9b29e486b8f716db3dbdc41365fb8fac96bac4a5d`.
Fresh eighteen-input PGO excludes PHPStan and every holdout. Preparation peaks
at 5,600,673,792 bytes under the verified 6 GiB/no-swap boundary; no OOM occurs.
Local cleanup hooks run in both checkouts and preserve exact binaries, sources,
profiles and all valid/failed evidence. No private benchmark host is configured.
Native evidence is x86-64 only; ARM64 remains unavailable.

The [complete packet](performance-phpstan-resolved-instanceof-samples.json)
retains build identities, all distributions, failures and controls. This is a
small reduction, leaving **7.31x** analysis instructions against PHP. The overall
parity goal remains open and returns to a fresh exact profile before the next
larger executor/ownership slice.
