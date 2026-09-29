# PHPStan instruction and runtime parity

Status: active; partial checkpoints do not complete this goal.

The outcome is the same five-file PHPStan analysis, with identical diagnostics,
exit status and PHP semantics, at the reference PHP instruction count and
analysis time. Startup is measured separately. A result-cache hit is not an
analysis and cannot satisfy this goal.

Baseline: `2ee8e73dd532051b73ddf0ff70255a48923aeec7`, default features,
`max-perf`, PHP 8.5.11 NTS with CLI OPcache/JIT disabled. Both runtimes use the
same PHAR, fixture, process-disabling settings and `zend.exception_ignore_args=0`.
Each analysis receives a fresh temporary directory. The previous accepted
checkpoint measured 11.509 seconds for RPHP's analysis versus 0.926 seconds for
PHP; its memory correction did not resolve the performance gap.

The earlier whole-application profile at `ffe29165` counted 155.357 billion
instructions versus PHP's 15.577 billion. Approximately 30.027 billion belonged
to main dispatch, 21.866 billion to other VM call/frame operations, 21.614
billion to explicit release planning, and 24.004 billion to general containers,
hashing, drop and string support. Those old totals are evidence for investigation,
not the instruction count of the current baseline. A new source-attributed
whole-application profile will establish the current budget.

The completed current-baseline profile counts **152,418,412,307** instructions
for RPHP and **15,606,913,817** for PHP, a 9.766-fold ratio. Both used the
unmodified PHAR and identical fresh analysis settings; diagnostics, stderr and
exit status matched. The RPHP diagnostic build adds line tables to `max-perf`.
All 8,629 RPHP text symbols have the same sizes as the previously accepted
build; native A/B comparisons still use matching flags on both sides. The
profile service completed without OOM or timeout, with a 3.859 GB aggregate
peak including its build. The central opcode match accounts for 0.824 billion
instructions, so dispatch selection alone cannot explain the remaining gap.

Implementation hypotheses must remove repeated runtime work through general
ownership, representation or immutable metadata. No PHPStan-specific names,
types, source patterns or reduced semantic checks are admissible. Existing
diagnostics, aliases, COW, exception/destructor ordering, dynamic binding,
autoload and suspended activations remain part of the semantic envelope.

This integrating task temporarily owns affected compiler, value, runtime and VM
files in its isolated performance checkout. It has no concurrent editing agent.
Each implementation is evaluated against the whole application before broad
test expansion. A small microbenchmark improvement cannot replace that gate.
Focused differential cases cover the changed semantic boundary. Accepted
checkpoints additionally pass formatting, the applicable unsafe policy and
feature checks, plus independent runtime holdouts. Architecture-independent
changes must not introduce architecture-specific behavior; unavailable native
architecture measurements are recorded as unavailable.

Profiles, builds and benchmark cycles use an exclusive lock and a separate
aggregate memory boundary with 6 GiB maximum memory, no swap and process-group
cleanup. Every valid sample, output mismatch, timeout and OOM remains evidence.
Diagnostic line-table builds are identified separately from native acceptance
builds. Native A/B builds use identical profiles and toolchains.

The first implementation checkpoint replaces repeated string-based class
relations with the existing stable declaration IDs. The old application made
18.289 million `class_is_a` calls, responsible for 36.581 million class-name
lookups. Objects already carry their declaration ID; constant `instanceof`
operands can retain successful resolution in their instruction's cache.
Ancestry is immutable after publication. Unresolved declarations and dynamic
class expressions must preserve normal lookup behavior, including later alias
publication; misses cannot be cached as permanent failures. Focused gates cover
parent/interface relations, aliases, anonymous classes, implicit Stringable,
relative scopes, dynamic targets and class type hints.

Reject an implementation if its predicted application budget is not removed,
if it changes observable behavior, or if its regression cannot be justified by
the integrating task. Rejection returns to the application profile; it does not
complete or suspend the parity goal. Commit and push coherent verified
checkpoints while continuing toward the full outcome.

## Accepted checkpoint: resolved class identities

Objects now use their existing class ID for class type checks. Transitive
ancestry retains numeric membership alongside names needed for unresolved
declarations. Constant `instanceof` operands retain successful resolution in
their existing instruction cache; unresolved names continue to observe later
declarations and aliases. Incomplete ancestry is rebuilt when the class table
grows. Dynamic operands and relative scopes retain their normal resolution.

The whole command falls from **152.418 to 147.854 billion Callgrind
instructions (-2.99%)**. Reduced class lookup, case folding and comparison
account for the improvement; new ancestry membership work is included in the
total. Reference PHP uses 15.607 billion instructions under that profiler.

Two native measured runs per RPHP binary in ABBA order, after one warmup per
runtime, give analysis medians **11.521 to 11.141 seconds (-3.30%)**. Every run
has fresh result-cache storage, five analyzed files, twenty identical findings
and the same exit status. PHP's measured analysis is 0.939 seconds. Candidate
RSS increases from 592,744 to 594,696 KiB, about 1.9 MiB for ancestry metadata.

An independent hardware-counter pass counts **150.036 to 145.542 billion
retired user instructions (-3.00%)**, with PHP at 15.474 billion. All counters
ran for 100% of their enabled interval. This separate window measures analysis
medians of 12.159 and 11.881 seconds; these samples are retained separately from
the earlier native window. Both instruction protocols count the complete
command, whereas analysis timers exclude interpreter and PHAR startup. Their
totals must not be mixed.

Validation: 20 instanceof/catch cases under default, no-default and all features;
174 type-hint cases and 12 class-alias cases under default features; two focused
PHP differential programs; formatting, unsafe policy and all-target/all-feature
checks. All pass. Three randomized pairs for each independent holdout show
changes from -0.68% to +1.03%; these small changes are retained, with the positive
whole-application result accepted by the integrating task. No OOM or timeout
occurred. ARM64 native measurements are unavailable; the change introduces no
architecture-specific implementation. Exact samples and build identities are
in [the checkpoint data](performance-phpstan-class-identities-samples.json).

This checkpoint removes approximately 4.5 billion instructions. More than
130 billion excess instructions remain. It does **not** complete parity.
