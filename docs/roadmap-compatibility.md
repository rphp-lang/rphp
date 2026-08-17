# RPHP compatibility roadmap

Status: active compatibility direction

See the [project coordination map](roadmap.md), the
[Compatibility Agent strategy](agent-strategy-compatibility.md), and the shared
[goal contract](agent-goal-contract.md) for assignment and integration rules.

## Mission and public contract

Make RPHP an increasingly complete, independently implemented PHP runtime with
PHP 8.5 as its public compatibility contract. `PHP_VERSION`, Composer platform
checks, documented behavior, diagnostics, and compatibility claims must agree
with that contract. Experimental RPHP syntax and opt-in features remain outside
it unless they are admitted explicitly.

RPHP is pre-alpha. A passing program proves only the behavior exercised by its
test. No milestone below permits a blanket PHP, Composer, Symfony, extension,
SAPI, or production-readiness claim beyond its exact differential gate.

## Non-negotiable rules

1. Baseline bytecode execution is the semantic source of truth. Implement PHP
   behavior there before an optimized tier may depend on it.
2. Compare observable behavior with an exact reference PHP oracle: output,
   errors, exit status, side effects, references, ordering, and lifetime.
3. Implement general PHP contracts. Never recognize a framework, package,
   class, method, fixture, or benchmark name to make a gate pass.
4. Keep every failure visible. Skips, exclusions, unsupported sections,
   timeouts, crashes, setup failures, and vendor patches are never passes.
5. Preserve previous exact passes. A gain that loses an unrelated pass is not
   an accepted compatibility checkpoint until the regression is understood.
6. Do not copy or mechanically translate php-src or another implementation.
   Public specifications and clean-room differential observation define the
   expected behavior; repository-owned regression tests must be original.

## Starting evidence

The public platform identity is PHP 8.5.0. The reproducible contract corpus is
pinned to php-src 8.5.6 commit `fcc29c8`. The retained PHP 8.2.33 and 8.4.21
results are historical regression and trend evidence; neither defines current
PHP behavior.

The initial AMD64 PHP 8.5 baseline uses RPHP `298e4c7` and discovers 5,599
unmodified `Zend/tests` and `tests/lang` cases: 1,815 pass, 3,390 fail, 110
skip, one upstream XFAIL, 280 are unsupported, one times out and two crash.
The matching PHP 8.5.6 oracle has 5,440 passes, zero ordinary failures, 153
skips, one XFAIL and five unsupported SAPI sections. The manifest therefore
starts with explicit crash/hang work in `gh18572.phpt`,
`recursive_array_comparison.phpt` and `gh13178_4.phpt`, followed by the
highest-fanout front-end and runtime clusters.

The `79b754f` recursive-comparison checkpoint removes both initial crashes and
adds one adjacent pass without losing an exact pass: 1,818 pass, 3,389 fail,
110 skip, one XFAIL, 280 unsupported, one timeout and zero crashes. Active
compound-value tracking converts recursive array/object comparison into the
PHP 8.5 catchable nesting error while preserving self-identity. The remaining
process hazard is `gh13178_4.phpt`.

The `523f934` array-cursor checkpoint converts that final timeout into an exact
pass with no lost pass: 1,819 pass, 3,389 fail, 110 skip, one XFAIL, 280
unsupported, zero timeouts and zero crashes. Removing an ordered array entry
now adjusts a positional internal cursor only when the removed entry preceded
it, preserving PHP's current/next semantics across storage transitions.

The `4b419b2` PHP 8.5 pipe checkpoint adds 16 exact passes with no lost pass:
1,835 pass, 3,373 fail, 110 skip, one XFAIL, 280 unsupported, zero timeouts and
zero crashes. The dedicated lexer/parser precedence layer and baseline dynamic
call lowering preserve input-before-callable evaluation, mixed callable forms,
left-associative chaining and non-referenceable pipe arguments. Fourteen pipe
cases remain explicit follow-up work across generators, assertion source
rendering, compile diagnostics, call diagnostics and one CLI-INI capability.

The `d29db36` call-diagnostic checkpoint adds three exact passes with no lost
pass: 1,838 pass, 3,370 fail, 110 skip, one XFAIL, 280 unsupported, zero
timeouts and zero crashes. User-function errors recover original declaration
spelling from cold metadata, while non-referenceable by-reference arguments
use PHP 8.5's diagnostic wording across positional, named and unpacked calls.
This closes the pipe `call_by_ref.phpt` case plus adjacent nullsafe and
restricted-`$GLOBALS` cases without changing the ordinary call path.

The `cc3738a` pipe-arrow diagnostic checkpoint adds three exact passes with no
lost pass: 1,841 pass, 3,367 fail, 110 skip, one XFAIL, 280 unsupported, zero
timeouts and zero crashes. The parser preserves PHP 8.5's compile-time fatal
error for a bare arrow function on the pipe RHS while accepting the explicitly
parenthesized form. The check runs only during parsing and does not alter the
runtime call path.

The `547409b` call-type checkpoint adds 11 exact passes with no lost pass:
1,852 pass, 3,356 fail, 110 skip, one XFAIL, 280 unsupported, zero timeouts and
zero crashes. Supplied values are type-checked before a later missing argument,
user-call diagnostics include parameter and caller metadata, and weak scalar
arguments use the same canonical coercion table as typed writes. Exact compact
calls are unchanged; the new work stays on mismatched cold call preparation.

The `3804063` iterator-to-array checkpoint adds one exact pass with no lost
pass: 1,853 pass, 3,355 fail, 110 skip, one XFAIL, 280 unsupported, zero
timeouts and zero crashes. The builtin reuses the canonical traversable
collector for Generator, IteratorAggregate and Iterator, also accepts arrays,
and implements preserve/reindex key behavior. This closes the pipe generator
chain; remaining users are blocked by earlier independent SPL/Reflection gaps.

The `5a4b509` destructuring-write checkpoint adds one exact pass with no lost
pass: 1,854 pass, 3,354 fail, 110 skip, one XFAIL, 280 unsupported, zero
timeouts and zero crashes. Calls and pipe expressions in destructuring targets
now parse as complete expressions and retain PHP 8.5's compile-time function-
result write-context diagnostic, including in dead code. Ordinary assignment
and runtime call paths are unchanged.

The `8f5744b` baseline-assert checkpoint adds three exact passes with no lost
pass: 1,857 pass, 3,351 fail, 110 skip, one XFAIL, 280 unsupported, zero
timeouts and zero crashes. The global builtin works through ordinary,
namespaced-fallback and first-class callable dispatch, returns true on success,
raises the built-in `AssertionError` on failure, preserves a supplied Throwable
and validates invalid descriptions. Compile-time assertion source text,
`assert_options()` and CLI-controlled assertion elimination remain explicit
follow-up work.

The `e4f08d9` strict-internal-string checkpoint adds two exact passes with no
lost pass: 1,859 pass, 3,349 fail, 110 skip, one XFAIL, 280 unsupported, zero
timeouts and zero crashes. `strlen()` and `ord()` now reject non-string values
at internal-call boundaries in `strict_types=1`, including dynamic callbacks
and pipe chains, while weak calls retain their frame-free lowering and scalar
coercions. Invalid array callbacks also report the canonical first-member
diagnostic before iteration.

The `9acedad` closure-trace checkpoint adds 11 exact passes with no lost pass:
1,870 pass, 3,338 fail, 110 skip, one XFAIL, 280 unsupported, zero timeouts and
zero crashes. Function and arrow tokens retain their declaration line through
the AST, and anonymous callables expose PHP 8.5's source-, method- and nested-
closure trace names. The metadata stays inside the existing immutable internal
name, does not enlarge `OpArray`, remains hidden from closure dumps and magic
constants, and preserves bound-closure `Closure->` rendering.

The `966e936` static-reference checkpoint adds two exact passes with no lost
pass: 1,872 pass, 3,336 fail, 110 skip, one XFAIL, 280 unsupported, zero
timeouts and zero crashes. Full-return synchronization now inspects the raw
static CV wrapper instead of dereferencing it and replacing its shared cell.
Generic concat reads referenced operands through that cell, preserving aliases
through first-class callables and PHP 8.5 pipe forwarders. All feature matrices
and the unsafe-policy gate pass; seven five-million-concatenation release runs
retain the 0.31-second median and exact output.

The `68e380b` array-multisort checkpoint adds five exact passes with no lost
pass: 1,877 pass, 3,331 fail, 110 skip, one XFAIL, 280 unsupported, zero
timeouts and zero crashes. `array_multisort()` supplies multi-column ordering,
sort flags, PHP key rebuilding and its legacy prefer-reference signature.
Variadic packing and `call_user_func_array()` retain explicit aliases through
detached callback dispatch. The full feature matrix and unsafe-policy gate
pass; seven ten-million dynamic-call release controls move from a 1.26-second
preceding median to 1.24 seconds with identical output.

The `1f679c6` invalid-unpack diagnostic checkpoint reaches 1,889 passes with
3,319 failures, 110 skips, one XFAIL, 280 unsupported cases, zero timeouts and
zero crashes. Rejected array and argument unpack operands now name their PHP
scalar type or concrete object class across compile-time and catchable runtime
paths. The exact PHP 8.5.6 delta is +3/-0; valid unpack execution is unchanged,
and the full feature matrix plus unsafe-policy gate pass.

The `06f26cb` typed-return checkpoint reaches 1,911 passes with 3,297 failures,
110 skips, one XFAIL, 280 unsupported cases, zero timeouts and zero crashes.
Declared `mixed`, nullable and ordinary typed returns now distinguish an
explicit `null` from no value, source-level bare returns fail during compilation,
and generators retain their separate return-value semantics. Canonical function
and method names plus throwable origin metadata close the adjacent return-error
cluster. The exact PHP 8.5.6 delta is +22/-0 with no remaining-fail stage moves;
the feature matrix and unsafe gate pass, while the relevant ten-million-call
baseline control retains an identical 0.31-second median and checksum.

The `40d51bc` Throwable-string checkpoint reaches 1,917 passes with 3,291
failures, 110 skips, one XFAIL, 280 unsupported cases, zero timeouts and zero
crashes. Built-in Throwable families share one previous-chain renderer across
explicit string conversion and uncaught output, while stored trace arguments
use PHP's byte-oriented escaping. The exact PHP 8.5.6 delta is +6/-0. Eleven
remaining failures move from object-conversion runtime errors to their correct
later output stage and stay visible under independent diagnostic/control-flow
gaps. All feature configurations and the unsafe-policy gate pass; no performance
gate applies because formatting is confined to an explicitly requested cold
Throwable path.

The `ed6dab4` type-diagnostic checkpoint reaches 1,922 passes with 3,286
failures, 110 skips, one XFAIL, 280 unsupported cases, zero timeouts and zero
crashes. A separate cold-path renderer supplies concrete object class names and
PHP's canonical `Traversable|array` spelling without changing hot type checks.
The exact PHP 8.5.6 delta is +5/-0 with no remaining-failure stage movement;
the full feature matrix and unsafe-policy gate pass, and no performance gate is
required for cold error construction.

The `c404c71` unmatched-match checkpoint reaches 1,925 passes with 3,283
failures, 110 skips, one XFAIL, 280 unsupported cases, zero timeouts and zero
crashes. The compiler retains the discriminant and exact `match` source line
for the cold throw edge, where PHP-compatible scalar or concrete-type text is
used to construct `UnhandledMatchError`. The exact PHP 8.5.6 delta is +3/-0
with no remaining-failure stage movement; the full feature matrix and unsafe
gate pass. No runtime performance gate applies because successful match arms
retain their existing execution path.

The `777552e` argument-TypeError checkpoint reaches 1,941 passes with 3,267
failures, 110 skips, one XFAIL, 280 unsupported cases, zero timeouts and zero
crashes. Functions, methods and closures retain declaration-line metadata in
their existing cold source map; rejected pending calls are snapshotted before
cleanup so Throwable origin and frame-zero call details match PHP. The exact
PHP 8.5.6 delta is +16/-0 with no stage movement. The feature matrix and unsafe
gate pass, with the production unsafe-block inventory reduced from 1,623 to
1,622. No runtime performance gate applies because successful calls retain one
existing post-validation error branch and the new work is confined to failure.

The `f985350` finally-exception checkpoint reaches 1,952 passes with 3,256
failures, 110 skips, one XFAIL, 280 unsupported cases, zero timeouts and zero
crashes. Pending exceptions are suspended in frame-scoped cold state while
`finally` runs, so an escaping replacement preserves PHP's explicit and
implicit `previous` order without cycles, while a locally caught exception
does not disturb the pending one. The exact PHP 8.5.6 delta is +11/-0 with no
stage movement; the full feature matrix and unsafe gate pass. No performance
gate applies because the new work is confined to active finally/throw paths.

Composer S0 and the four bounded Symfony S1 gates plus warmed FrameworkBundle
S2 pass on AMD64. An exact PHP 8.5.6 cold FrameworkBundle S3 revalidation did
not complete because RPHP remained in the initial cold gate execution for more
than ten minutes. The retained PHP 8.2 S3 result is therefore historical only;
the hang is compatibility work, not a PHP 8.5 contract claim. None of these
bounded gates establishes general PHP, extension, SAPI or production
compatibility.

## Measurement system

### Contract baseline

Pin one public php-src PHP 8.5 commit, the reference CLI build/configuration,
the RPHP commit, runner commit, feature set, architecture, timeout policy, and
test-suite selection. Run unmodified `Zend/tests` and `tests/lang` first; add
other core and extension suites only as separately named capability gates.

The runner must validate itself against the matching reference PHP before an
RPHP result is admitted. It must preserve source filenames, streams, exit
status, environment, INI behavior, cleanup, and expectation semantics. The
published manifest must contain every discovered path and classify at least:

- pass, ordinary failure, skip, expected failure, and unsupported capability;
- setup/`SKIPIF`, lex/parse, compile/link, runtime, and output/diagnostic stage;
- timeout, signal termination, panic, and other crash modes.

The headline rate remains `pass / (pass + fail)`. Also publish the whole-corpus
rate, execution-stage reach, exact pass-set delta, crash count, and root-cause
clusters. None replaces the exact manifest.

### Goal backlog

Generate work from evidence rather than feature intuition. For each failing
path record the earliest shared root cause, affected language/library area,
stage, estimated fanout, framework dependency, and regression risk. Select the
next goal by this order unless an explicit project objective overrides it:

1. crashes, hangs, panics, state corruption, and nondeterminism;
2. runner/oracle uncertainty and incorrect diagnostics for already-supported
   behavior;
3. blockers on the active Composer/Symfony gate;
4. high-fanout PHP 8.5 language and baseline-semantic clusters;
5. Reflection and standard-library clusters with measured dependency reach;
6. isolated low-fanout features and optional extensions.

Every accepted goal updates its focused regression corpus and the failure
manifest. A broader PHPT rerun is required when the expected fanout is large,
the parser/compiler changes, failure staging changes, or a release checkpoint
is claimed.

## Workstreams and exit gates

The workstreams can overlap, but each change must be a complete vertical slice
through every affected layer. Milestones are evidence gates, not dates.

### C0 — Trustworthy PHP 8.5 differential baseline

Build and retain the pinned PHP 8.5 contract run alongside the historical audits.
Add original runner regressions for every harness discrepancy found.

Exit gate:

- the reference-PHP oracle has no unexplained ordinary failures;
- repeated RPHP runs discover the same paths and reproduce classifications;
- every case appears exactly once in a versioned summary and manifest;
- setup failures, unsupported capabilities, timeouts, and crashes are reported
  separately and no private host or filesystem data is published.

### C1 — Deterministic front end and diagnostics

Systematically close lexer, parser, name-resolution, declaration, and compile-
time validation clusters. Cover valid and invalid syntax together, including
precedence, postfix/dynamic forms, references, attributes, anonymous classes,
types, traits, enums, constant expressions, and source-location semantics.

Exit gate for each cluster:

- positive programs execute with reference-equivalent behavior;
- negative programs fail at the same stage with compatible type, message
  shape, filename, line, streams, and exit status;
- the grammar change is general and has focused boundary/interaction tests;
- the selected PHPT cluster has no unexplained failure or lost previous pass.

### C2 — Compiler and baseline semantic completeness

Close execution semantics in dependency order: values and conversions; calls,
arguments and returns; references and copy-on-write; arrays and iteration;
objects, properties and class state; closures and generators; exceptions and
unwinding; includes/eval; resources and request-local lifecycle. Preserve PHP
evaluation order and commit side effects exactly once.

Exit gate for each semantic family:

- compiler and baseline VM cover the behavior without an optimization tier;
- aliases, references, errors, magic behavior, cleanup, and exception paths
  match the reference oracle;
- focused tests include success, failure, interaction, and repeated-execution
  cases;
- default, no-default, relevant feature, and all-feature correctness gates
  pass for the affected surface.

### C3 — Standard library, SPL, errors, and Reflection

Maintain inventories by function/class/method/constant and by signature,
flags, warnings, references, side effects, and edge behavior. Prioritize core
families reached by Composer and Symfony: arrays/iterators, strings/PCRE,
serialization/export, hashing, filesystem/streams, process/error state,
Throwable, SPL protocols, and ordinary Reflection metadata/invocation.

An internal symbol with placeholder behavior is a failure. Extensions must
report unavailable until their public contract is implemented and admitted.

Exit gate for each family:

- the versioned inventory has no unknown reached symbols;
- every admitted symbol has differential signature, normal, boundary, warning/
  exception, reference/side-effect, and repeated-call coverage as applicable;
- Reflection reports real source and runtime metadata rather than fixture data;
- serialization and executable export are byte/behavior compatible for their
  admitted object, visibility, reference, and hook cases.

### C4 — Composer and Symfony cold kernel (S3)

Preserve S0, S1, and S2 while completing the pinned Symfony 7.4 cold build.
First correct the general export/call semantic behind the malformed compiled-
route value. Continue from each newly localized failure as a general PHP,
Reflection, standard-library, or lifecycle goal with an independent regression
test.

S3 exit gate:

- unmodified pinned vendor sources are installed from the exact lock;
- RPHP builds the container and route cache from PHP configuration without a
  reference-built cache or vendor patch;
- a fresh RPHP process reloads the generated files and returns exact `/health`
  and missing-route results;
- cold, cached, deleted-cache, and malformed-cache transitions match reference
  PHP for output, diagnostics, exit status, declared symbols, included files,
  container/route manifests, and documented normalized fields;
- cache publication and locking are atomic, and focused regressions preserve
  every general contract discovered on the path.

Passing S3 permits only the named pinned Symfony CLI-kernel claim.

Current retained S3 evidence includes the general array form of
`str_replace()`: Symfony's base64 container hash sanitizer now replaces both
`/` and `+`, so a random slash cannot become an invalid namespace or cache
path. Concurrent cold-publisher results are compared as an exact unordered
multiset because publisher process numbering is scheduler-dependent; status,
diagnostics, output, generated-file manifests and the fresh cache load remain
mandatory. The current cold build additionally proves that list destructuring
can write ordinary, dynamic and nested `$this` properties, that namespaced
`callable|false` retains its literal type, and that PHP reference assignment
distinguishes writing through an existing alias from rebinding a CV. In
particular, a by-reference `foreach` target is rebound for each listener, so
Symfony EventDispatcher preserves every lazy listener and RouterListener
publishes a valid compiled route cache.
The cold build also preserves a `$this` property passed to a runtime-resolved
by-reference method parameter, and destructuring a null or other non-array
scalar source yields null elements without scalar-offset diagnostics. These
contracts keep `PhpDumper`'s recursive call map live and permit its synthetic
`kernel` and `service_container` entries to remain null without warnings.

### C5 — Request/response and repeated runtime (S4)

Add an HTTP-facing adapter separately from baseline language compatibility.
Initialize superglobals and request body accurately; publish status, duplicate
headers, cookies, and binary body; run kernel termination and reset request-
local state.

S4 exit gate:

- single-request behavior matches the pinned reference fixture end to end;
- repeated requests do not leak superglobals, handlers, buffers, statics,
  services, resources, cache entries, or observable response state;
- failures, cancellation, destructors, and shutdown behavior are covered;
- memory growth is bounded under the defined worker workload.

FPM/CGI protocol compatibility and production hardening remain separate claims.

### C6 — PHP 8.5 contract convergence and ecosystem expansion

Drive the PHP 8.5 failure manifest toward zero by root-cause cluster. Admit a
suite or extension only when its unmodified differential gate has no ordinary
failure, crash, or timeout and all unsupported capabilities are named. Expand
the ecosystem after S3 through separately pinned gates such as Console/Dotenv,
Twig, PDO/Doctrine, Security/sessions, caches, HttpClient/cURL, Messenger, and
long-lived workers.

A broad PHP 8.5 compatibility label requires an explicit release manifest:
complete selected suites, exact platform/configuration, zero hidden exclusions,
zero unexplained crash/timeout, and a published list of every unsupported SAPI
and extension. Until then, documentation must continue to say “tested subset.”

## Cross-roadmap integration

Compatibility owns observable PHP behavior and baseline semantics. The
execution/performance roadmap may optimize only an already-proven contract and
must retain exact deoptimization to the baseline VM. If both tracks need a
shared compiler, VM, value, or object-layout file, assign one temporary owner,
integrate the compatibility checkpoint first, then rebase and remeasure the
performance change.

Compatibility changes must preserve established performance controls. A
regression outside normal variance triggers profiling and joint review; it does
not justify a semantic shortcut. Conversely, a missing optimized region never
blocks acceptance of correct baseline behavior unless the stated goal includes
a measured performance gate.

## Compatibility checkpoint record

Keep roadmap history out of this document. Each accepted checkpoint should
leave a compact commit/PR record containing:

- supplied goal and root cause;
- public reference oracle and exact fixture/corpus identity;
- observable contract implemented and explicit non-claims;
- focused and broad test commands with results;
- manifest/pass-set delta, crash/timeout delta, and remaining blockers;
- performance/layout evidence when a hot path changed.

The current compatibility status and reproducible measured results belong in
[`compatibility.md`](compatibility.md), not as an accumulating implementation
diary here.
