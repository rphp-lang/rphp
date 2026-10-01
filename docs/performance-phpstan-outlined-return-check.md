# Outlined return validation

Status: accepted on the performance branch; the PHP instruction/time parity goal remains open.
Baseline: clean `deea219c`, source
`3cab6798a792c094cf773bf3bb6e9cd59b5efff7fc79c154d61b27b988570bdd`.
Candidate source:
`f86729eb00a0b37058fdb228745baebf3933dae975aca2d44de071575c144146`.

## Contract and measured cause

The preceding exact analysis has 3,551,230 full return-preparation calls,
with 0.8845 billion inclusive instructions in preparation, before the caller's
inspection clone and its retirement. Return opcode bodies separately account
for at least 2.301 billion own instructions. These costs overlap and cannot be
added. Successful type inspection does not need to become a PHP owner.

Keep the pure canonical exact checker in one non-inlined function returning
`bool`. Successful inspection borrows the live operand without retaining and
retiring a snapshot. The borrow ends before any conversion, warning handler,
exception unwind or PHP callback. Failure takes the same owned snapshot and
runs the complete original preparation, including its repeated exact check.
That repeat preserves exact union precedence when cloning unwraps additional
internal reference levels. Coercion, diagnostic snapshots, lexical/called
scope, reference returns and finally validation remain canonical.

The sole integrating task owns the executor, focused fixtures and independent
return benchmark on the isolated performance branch. There are no new workload
recognizers, dynamic admission rules, caches, layouts, ABI changes or unsafe
invariants. Typed/JIT coverage is unchanged. Native measurements are x86-64;
there is no ARM64 performance claim.

Require at least a 0.5% full ordinary-release instruction saving before fresh
PGO and feature expansion. Reject changed supported output or lifetime, a
borrow surviving PHP re-entry, or confirmed unjustified control regressions
above 1%. Use identical fixed training selection, interleaved full application
measurements, independent controls and focused feature gates. Preserve every
valid sample, and run all expensive work under the exclusive lock and verified
6 GiB/no-swap aggregate memory boundary.

## Rejected representation and retained variant

The first outlined variant returned `Option<Value>`, combining the pure probe
and failed snapshot acquisition. Its full ordinary-release saving was 0.4884%,
below the predeclared filter, so it was removed before this variant started.
Its exact-path assembly retains the aggregate destination and source, writes
the option tag and passes through a shared epilogue. The helper occupies 198
bytes. The retained boolean helper occupies 63 bytes and returns in a register;
failed inspection keeps the original caller-owned clone.

Ordinary-release full analysis instructions fall 75.0405 to 74.6498 billion
(-0.5207%). Fresh PGO uses the same independent 18 programs as the baseline,
excluding PHPStan and all acceptance controls. Its first window falls 69.0422
to 68.6697 billion (-0.5394%), with analysis time 6.4641 to 6.3407 seconds.
Independent confirmation falls 69.0510 to 68.6894 billion (-0.5236%), with time
6.4377 to 6.3610 seconds (-1.1919%). PHP uses 10.3377 billion instructions and
0.9294 seconds in that confirmation. All full application outputs and exit
signatures agree. The remaining instruction ratio is approximately 6.64x.

The executor grows by 102 bytes (391,786 to 391,888); owned preparation
remains 386 bytes. The PGO executable grows by 1,248 bytes including metadata.
There is no new steady-state allocation or owner operation on exact success.

## Controls and verification

All eight controls retain their first three-pair window and the independent
five-pair review: 128 valid output-checked observations. The first window has
trait-property, inherited-method and shared-temporary time increases of
3.34%, 4.95% and 1.87%. These do not repeat in the full independent review;
the cause of the transient difference is not isolated. The review's largest
time increase is inherited methods +0.725%, with unchanged instructions.
No review control exceeds the 1% regression gate.

The independent object/array/nullable/union return workload uses 2.929% fewer
instructions and runs 5.475% faster in review. Relative-return scope uses
2.726% fewer instructions and runs 1.622% faster. The other controls retain
essentially unchanged instruction counts. This is a scoped return-boundary
improvement, not a claim of general PHP parity.

Fifty-six PGO CLI observations agree exactly across PHP, baseline, candidate
and forced canonical execution. They cover compact/wide lifetime and reentrant
conversion cases, references, COW, lexical scopes and finally completion.
All 403 actual focused test executions pass across default, no-default and
all-features configurations. All-target/all-feature compilation, formatting
and unchanged unsafe-policy checks pass (1,749 blocks / 321 functions). Build/training and diagnostic results remain separate from native
timing. No OOM has occurred. Exact binaries, rejected diffs and source snapshots
are retained privately; published evidence contains hashes and all valid
measurements without private source or connectivity.

Local cleanup hooks ran in both checkouts. The superseded PGO Cargo target
was removed after preserving exact executables and sources. The ordinary
feedback cache remains for the active next comparison. No private benchmark
host is configured. The source fingerprint above identifies this checkpoint;
main integration remains separate. See [all observations](performance-phpstan-outlined-return-check-samples.json).
