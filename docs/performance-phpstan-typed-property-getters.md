# Exact typed property getter checkpoint

Status: accepted bounded instruction/time checkpoint. Overall PHPStan instruction/time parity remains open.
Baseline: clean `9790b30a`, source
`bdf1e318157656cd70248ffbc058565247689600f8df2f21c1c446a78818b3d9`,
exact executable
`5718f9bfb75856c4d0c44624c928a9118fcc2c17f5fa6fcac90886086b194565`.
Confirmed analysis budget is 76.7068 billion instructions and 6.9115 s,
versus reference PHP's 10.3380 billion and 0.9248 s.

## Evidence and outcome

A separate snapshot of this exact baseline, with attempted-frame counters,
records 756,478 frames for three-opcode property getters. Only 20,047 belong
to currently admitted getter plans. End-of-command cache states prove 734,499
attempts have readable declared-property cache words and either public access
or matching fixed function scope; 714,452 of those are unadmitted. These counts
bound an opportunity, not successful native admissions or instruction savings.
The diagnostic output agrees with PHP and the exact baseline.

Extend the existing structural property getter plan to exact typed results.
Do not add another source recognizer: retain the same zero-argument
FetchObjR/explicit Return/implicit Return body and existing cache-based read.
A pure borrowed representation/type proof may materialize its result in the
caller without creating a callee frame. Invalid, coercible, callable, reference-
return, diagnostic-attribute or otherwise unsupported boundaries resume the
canonical method before effects or ownership changes.

## Semantic envelope and ownership

Admit only zero-argument instance methods with one receiver CV, no by-reference
or variadic signature, no globals/statics/try/generator state and the exact
existing body. Reject callable/void/never hints recursively. Scalar, array,
nominal, nullable, union and intersection hints use the canonical pure type
query with strict representation acceptance; widening and weak conversions
remain canonical. Use the immutable declaring scope for self/parent, actual
receiver class for static, and decline unresolved or trait-relative scope.
No caller scope may substitute for the callee's scope.

Property cache class/read flags remain authoritative. A scoped entry must
match the exact callee function; rebound/trait-dependent scope falls back.
An undefined property, lazy object/proxy, missing or changing class cache and
active generic member contract retain ordinary execution. Clone heap/reference
results and maintain caller slot ownership through the existing writers.
The composed Long consumer must also prove the getter's exact return contract
before mutation; it cannot treat a coercible Bool/String/Float getter as Long.
Do not replay PHP reads, callbacks, diagnostics, arguments or committed writes.

The integrating task solely owns compiler getter admission, cold method-cache
classification, existing baseline/hot getter, composed-call and quick resolved materializers,
focused fixtures and this evidence. Method memo checkpoint is accepted and
closed; no second implementation is active. Value/opcode/frame/JIT layouts,
new PHP syntax and unsafe invariants remain unchanged.

## Gates and rejection rules

Compare new success/failure, nullable/nominal/relative-scope, private shadow,
reference/COW, uninitialized/lazy, magic, coercion, diagnostic and composed-call
fixtures against PHP and exact baseline before measuring. Retain baseline gaps
as failures; never call an optimized-only semantic fix a passing differential.
Use focused default/no-default/all-feature suites, meaningful generic receiver
contracts, formatting, unsafe inventory and all-target compilation under the
verified 6 GiB aggregate memory boundary. Avoid a broad unrelated test matrix.

Build fresh identical-policy PGO from the unchanged eighteen independent inputs.
Exclude PHPStan and independent typed-getter holdouts. Measure phase instructions,
analysis time/RSS, whole command, established controls and actual getter coverage.
Interleave native samples, retain every valid result and independently confirm
changes outside the established noise band. Visible modest timing tradeoffs
follow the user's instruction priority, not an all-program speedup claim.
Native architecture available is x86-64; ARM64 remains an explicit limitation.

Reject changed PHP behavior, effects before fallback, scope/reference/ownership
mistakes, discarded generic boundaries, layout/unsafe expansion, failed focused
gates or no instruction reduction. Freeze source during measurements. Preserve
exact source/binaries/raw evidence and run mandatory cleanup at checkpoint end.

## Accepted result

The exact candidate source is
`88887d4a07c385345b4508e0ae507c848922c997a219409b939ffc7a9f8c2b0b`,
with executable
`80dbcc8339ba54b471a48eedadc2ddd9b5e82a9eb40054dd836f84820e834836`.
Fresh PGO uses the same eighteen independent programs; neither PHPStan nor the
two new independent getter controls enter training. Its merged profile is
`ba24fff8433b2f7f66c33340adcc18036c3053b742b2ed663bba45741a471ba2`.

The first phase measurement falls from 76.6897 to 76.1714 billion instructions.
The independently ordered confirmation falls from **76.6928 to 76.1537 billion
(-0.703%)**, with reference PHP at **10.3380 billion**. Hardware counters run
for their entire enabled analysis interval. Whole-command confirmation medians
fall from 95.3377 to 94.7467 billion (-0.620%); these include startup and are not
the analysis instruction target.

Analysis timer medians fall from **7.0172 to 6.9655 seconds (-0.737%)**, versus
PHP's **0.9321 seconds** in this window. All valid samples remain, including the
candidate's 7.1679-second observation. Each run analyzes five files using fresh
result-cache storage; diagnostics, ordinary stderr and exit status match.
Median RSS increases by 228 KiB. The remaining **7.37x** phase instruction gap
keeps the overall parity goal open.

A separate default-plus-vm-stats diagnostic executable records **767,068**
completed direct getter calls in the full PHPStan command. The same diagnostic
with direct getters and composed property calls disabled records zero such
completions and identical output. These counts are actual executions, but the
diagnostic build is not a native timing sample or a baseline frame subtraction.
Composed and quick getter completions are zero on this application.

Seven PHP fixtures exercise exact scalar/container/nominal/union/intersection
results, relative nullable scope, private shadowing, references and COW,
coercion before a composed Long call, undefined/magic properties, lazy objects
and hooks, diagnostic attributes and extra arguments. Fourteen exact PGO CLI
comparisons pass at ordinary and wide caller geometry. The 104 initial focused
feature executions pass; three additional feature executions validate the
strengthened exact-types fixture, for **107** total. All-target/all-feature
compilation, formatting and unsafe inventory also pass.

The first exact-types fixture alternated receiver class on every iteration and
recorded zero direct admissions. Its mandatory coverage assertion failed; that
failure is retained. Grouping repeated reads before each class change produces
48 direct completions and unchanged PHP output. Nullable/reference fixtures
record eight/four completions, and the coercion fixture records twenty failed
exact-return probes, with identical forced-canonical outputs. Unsupported
attributes and extra arguments record no admission.

## Independent controls and tradeoff

The original alternating-class getter holdout is preserved. At the separately
confirmed 2.5-million-iteration scale, it takes **2.68% more time** and executes
**0.0356% more instructions**. It cannot exploit a property cache continually
changed by the previous receiver class. A second control with sixty-four reads
before changing receiver class falls **17.85% in instructions** and **18.88%
in time**. Both checksums match PHP, both first/confirmation windows remain,
and neither control is used for training.

Every first-window control median outside the one-percent timing envelope,
including improvements, receives independent confirmation. Scalar-frame time
improves 6.03% with unchanged instructions; regex time changes -0.385% on its
larger confirmation scale. Other established controls remain visible in the
first window. Different iteration scales are never pooled.

A separately retained frontend diagnostic on the alternating getter control
records more op-cache misses, instruction-cache misses and frontend no-op slots.
Its time difference is smaller than the confirmation window. This supports an
instruction-supply contribution, but does not prove a specific placement cause.
The integrating task accepts the visible modest control tradeoff under the
user's instruction priority and the verified application instruction reduction;
this is not an all-program speedup claim.

The main executor grows by 367 bytes to 390,624 bytes; the shared exact-return
helper is 299 bytes. No Value, frame, opcode or native ABI layout changes and no
new production unsafe operations are introduced. Native evidence is x86-64;
ARM64 remains unmeasured. The existing callback-last-owner PHP mismatch remains
baseline-equal and is explicitly not counted as a PHP pass.

Build/check/benchmark jobs remain in verified 6 GiB, zero-swap process-group
boundaries. The explicitly failed initial coverage assertion is not a pass;
subsequent coverage and confirmation pass. No OOM or timeout occurs. Cleanup
runs in both local checkouts and removes superseded disposable build targets,
while retaining exact binaries, source snapshots, raw PGO and every valid sample.
No private benchmark host is configured. Full identities, samples, resource
results and the retained coverage failure are in
[the checkpoint data](performance-phpstan-typed-property-getters-samples.json).
