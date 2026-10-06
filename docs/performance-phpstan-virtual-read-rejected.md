# Rejected virtual read snapshot

Eliminating a proven temporary Array snapshot preserves focused PHP behavior,
but independently confirmed ordinary release instructions increase
**73.7793 to 74.6685 billion (+1.205%)**. Both two-pair windows agree:
**+1.205% and +1.202%**, with all eight valid observations retained. The
candidate is rejected and was never applied to production. The accepted PGO
scorecard remains **67.5210 billion / 6.0081 seconds**, versus PHP roughly
10.3380 billion / 0.9445 seconds; simultaneous parity remains open.

This is evidence about a private ordinary release candidate, not a new PGO
result or a general speedup. Its combined analysis time medians are
6.6477/6.5946 seconds (-0.799%); a small timing decrease does not satisfy the
instruction-priority goal. There is no PGO rebuild, broad feature matrix or
architecture performance claim after rejection.

## Removed work and semantic boundary

The planner proves an adjacent cached property read, array dimension read and
completed-operand release. It checks a unique nonescaping TMP, original
instruction identities, declared frame bounds and a live CV receiver. Runtime
uses the existing class/scope/layout and array-key cache guards. Present
primitive results are written through the canonical tracked slot helper.
Missing/invalid keys, references, heap results, old destination owners, finite
budgets, pending exceptions and interrupt crossings remain canonical before
publication. Wide frames retain their initialized-tail invariant.

The live receiver retains the original Array. The candidate avoids constructing
and retaining its temporary snapshot, but reproduces the normal shared-drop
cycle-root admission at the original cleanup point. Canonical cloning clears
the internal argument-snapshot bit, so virtual retirement deliberately ignores
that source bit. Logical TMP undef state, heap metadata, monotone frame state,
cleanup origin and interrupt ticks are restored. No claim depends on Rust
padding bytes. No PHP-visible callback runs inside the elided lifetime.

The full instrumented request admits **1,133** plans, completes
**3,694,968** reads and takes **495,977** fallbacks. Exactly 3,694,968 frame
writes and heap-value writes disappear. Compared with the retained baseline
diagnostic, call counts, frame initialization/cleanup counts and existing
quick/JIT execution/admission counters are unchanged. This rules out a change
in those counted operations, not every possible code-generation effect.

The first unfinished planner admitted zero executions because it required an
unmarked cleanup. Compiler source proves completed operands use the plain
subexpression marker. That generic admission omission was corrected before
native selection; the zero-entry source/results remain retained and are not
counted as successful execution of the optimized path.

## Why the slice does not pay for itself

Separate instruction samples over the exact ordinary binaries attribute about
**2.2200 to 1.9182 billion** self instructions to the original temporary
release helper, a reduction of approximately 0.3019 billion. The new read
helper closure adds approximately **0.4553 billion** self instructions and
main self increases approximately **24.8035 to 24.9733 billion**.

Those approximate self observations establish that removing millions of
copies also introduces substantial execution work. They do not exactly
partition the whole 0.8892-billion increase, nor identify every new guard or
spill. Sampling skid, separate runs and compiler outlining prevent such a
claim. Native whole-analysis counts remain the acceptance evidence.

An isolated ownership fusion is too small a parity strategy when its entry,
validation, lookup and publication machinery costs more than the work removed.
The preceding eager graph also retained those protocols behind a second
interpreter. Neither failed design justifies stacking compensating hot-path
conditions or claiming that fewer bytecode dispatches guarantee fewer native
instructions. Broader execution changes require a quantified common cost and
actual native benefit; no parity date follows from these probes.

## Verification and reproduction

Nine controls produce **36** exact PHP/baseline/candidate/forced-canonical
observations. Five controls enter the optimized path, including primitive
projection, scope/typed-property guards, wide slots, callback replacement of a
property after an invalid key, and GC/destructor activity. Other controls
exercise fallback or unchanged arithmetic, references and value kinds.
Four whole-application native pairs and both instruction profiles preserve the
same five-file/twenty-finding output and expected exit status.

Fourteen focused library tests pass, including positive planner selection and
a canonical clone/drop-versus-virtual-retirement model covering enabled,
disabled/re-enabled and startup-disabled collection, repeated admissions,
immutable empty arrays, snapshot flags, thresholds and collector callbacks.
Formatting and the unsafe gate pass at the unchanged **1,749 blocks / 321
functions**. These checks are focused evidence, not full certification.

[Every native observation and exact identity](performance-phpstan-virtual-read-rejected-samples.json)
is retained with output hashes, build metadata, coverage and profile rows.
[The zero-context source patch](performance-phpstan-virtual-read-rejected.patch)
reconstructs the exact rejected source from `49aa44a6` with
`git apply --unidiff-zero`; it is diagnostic evidence, not an accepted runtime
change. Patch reconstruction is checked against the candidate source hash.

Builds and request/profile runs use the pinned toolchain, offline/locked Cargo,
verified 6 GiB / zero-swap aggregate OOM-group services and the exclusive
benchmark lock. Native counters cover only the analysis FIFO, use CPU 2,
fresh TMPDIR per run and 100% running counters. Coverage covers the whole
instrumented request and must not be substituted for that native budget.
No OOM or timeout occurs. Exact source, binaries and profiles are retained;
the disposable candidate target is removed and both local cleanup hooks run.
No private benchmark host is configured. Production Rust and main are unchanged.
