# Trait receiver proofs in declared property reads

Repeated private/protected reads in instance trait methods previously resolved
lexical scope and property metadata on every access. The existing property
cache required a fixed function scope or explicit closure scope, so ordinary
trait compositions never warmed. This general cache proof reduces the same
PHPStan analysis **73.4659 to 72.1651 billion instructions (-1.771%)**, independently
confirmed. The original instruction and one-second targets remain open.

The canonical scope resolver now also returns the hidden instance receiver
class when that class determines trait composition. Constant-name property
entries encode this proof in the existing scope word. Hits validate both the
property target class and the independent hidden receiver class. This matters
when two trait consumers read different private slots on the same target class.
Changed consumers or rebound closures miss before storage access. Static trait
frames and bodies with a hidden lexical-scope TMP retain canonical resolution.
Frame-free getter and JIT admission remain conservative. The compiler, cache,
frame and Value sizes and production unsafe inventory remain unchanged.

The selection diagnostic records 340,536 accessible nonpublic whole-command
reads without a fixed function or closure proof, out of 636,863 slow reads.
The preceding accepted analysis profile budgets the complete slow reader at
2.2281 billion inclusive instructions for 584,894 calls. These scopes differ;
nested costs overlap, and the diagnostic counts are not weighted phase costs.
The same-policy default test-fast comparison improves 115.3572 to 111.9730
billion (-2.934%), qualifying the change for fresh PGO evaluation.

Baseline is `ef167a36`; exact source and executable identities are in
[the sample packet](performance-phpstan-trait-property-scope-samples.json).
Native analysis measurements use the same five files, complete exact PHP
output, fresh temporary directories, no warmup, alternating pairs, one CPU
and 100% hardware counter running time. Every valid sample is retained.

| Independent PGO window | Baseline instructions | Candidate instructions | Change | Baseline analysis | Candidate analysis |
|---|---:|---:|---:|---:|---:|
| First, two pairs | 73.4523 G | 72.1693 G | -1.747% | See every sample | See every sample |
| Confirmation, two pairs | 73.4659 G | 72.1651 G | -1.771% | 6.7218 s | 6.6739 s |

The confirmation reference is PHP at 10.3379 billion instructions and 0.9575 s:
RPHP still has about a **6.98x instruction gap**. A separate whole-command check
records 91.9763 to 90.6334 billion instructions and RSS 622,604 to 622,260 KiB.
Whole-command counts include startup and are not analysis-only results.

Five independent confirmation pairs on each of five PHP-validated controls
retain all checksums. The mixed trait-consumer holdout improves 11.550% in
instructions and 10.905% in time. Existing control instruction regressions peak
at +0.128%; time regressions peak at +0.902%, within the declared one-percent
limit. The earlier larger shared-frame and foreach timing regressions do not
repeat. Fresh PGO uses the baseline's 18 unchanged independent inputs; PHPStan
and all controls are excluded. Executable text grows 368 bytes and the complete
binary 2,472 bytes. Native evidence is x86-64 only; no new lowering is added.

Two PHP fixtures cover alternating consumers with one target class, inherited
and recomposed methods, callable and rebound closures, COW arrays, hidden
lexical scope, static fallback, readonly diagnostics and typed unset properties.
Their original baseline and PHP outputs agree. Default, no-default and
all-feature configurations pass 405 focused executions, plus 55 preparation
repeats: 460 actual passes. Formatting, unsafe policy and all-target all-feature
compilation pass. Thirteen exact PGO CLI cases agree with PHP; the two documented
baseline PHP gaps remain failures.

The first attempted comparison used an all-feature Cargo binary incorrectly
retained after a later gate. It failed at the existing parametric-LSP diagnostic
before analysis and supplies no valid counts. Rebuilding the exact accepted
default source reproduces the original baseline hash. Missing-control and
missing-archive diagnostic runs and two failed diagnostic compilation attempts
are retained as infrastructure failures, rather than successful measurements.

All expensive jobs use verified aggregate 6 GiB/no-swap limits and the exclusive
benchmark lock. There are no OOM events. Cleanup ran after the checkpoint;
superseded build targets are removed while exact source/binary evidence is
retained. No private benchmark host is configured. The next cost selection uses
the fresh instruction profile, including attribution of memory-copy callers.
