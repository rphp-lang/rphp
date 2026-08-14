# Compatibility status

RPHP implements a growing, tested subset of PHP. Its public dependency-platform
identity is PHP 8.2.0; some newer language behavior remains available as an
experimental RPHP extension, but it is outside that compatibility contract.
RPHP is not certified for a complete PHP version and must not be treated as a
drop-in PHP replacement. Passing a script is evidence only for the exercised
behavior.

## Official PHP 8.2 php-src PHPT contract baseline

The public contract baseline runs the unmodified `Zend/tests` and `tests/lang`
suites from PHP 8.2.33 commit
`651db3ebfa622cae0c4e6b39766812efbd274ced` against all-features RPHP commit
`8bcc5480a89eaef667612e138d4450d6bc498cc5`, using the same runner commit. The
recorded run used arm64 and a three-second per-process timeout. It discovered
4,345 PHPT cases.

| Suite | Pass | Fail | Skip | XFAIL | Unsupported | Timeout | Crash | Headline pass rate |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `Zend/tests` | 972 | 2,792 | 65 | 1 | 221 | 0 | 0 | 25.824% |
| `tests/lang` | 88 | 180 | 10 | 0 | 16 | 0 | 0 | 32.836% |
| **Combined** | **1,060** | **2,972** | **75** | **1** | **237** | **0** | **0** | **26.290%** |

The headline follows the published gate definition exactly:
`pass / (pass + fail)`. It does not count skips, the known upstream `XFAIL`,
unsupported cases, timeouts or crashes as passes. A stricter whole-corpus view
is 1,060 / 4,345, or **24.396%**; including crashes and timeouts in the attempted
denominator gives **26.290%**. These numbers are intentionally pre-alpha and do
not support a complete PHP 8.2 claim.

The schema-5 execution profile makes the strict score less easy to mistake for
language coverage. Of 4,032 attempted cases, six fail during `SKIPIF` before
the test body, 749 are rejected in the observed parse/compile stage, and 3,277
(**81.275%**) execute the test's `FILE` section past that stage. This is not a
second compatibility score: invalid-source PHPT cases are supposed to stop in
the front end, and reaching runtime says nothing about correct semantics or
diagnostic text.

The largest failure groups are 1,192 runtime failures, 1,056 output mismatches,
567 parse failures, 151 compile failures and six failed `SKIPIF` evaluations.
No case terminates by signal or times out. Of the 75 skips, 45 require
unavailable extensions and 30 are selected by `SKIPIF`. Unsupported cases
remain in the total: 234 require per-process `INI` behavior that the RPHP CLI
does not expose, while three require PHPDBG or CGI/header sections outside this
CLI gate.

The complete official PHP 8.2.33 CLI oracle run produced 4,255 passes, zero
ordinary failures, 86 skips, the one upstream `XFAIL`, three unsupported SAPI
sections, zero timeouts and zero crashes. Five representative cases also pass
through php-src's official `run-tests.php`. Two independent RPHP executions
with a matching native PHP 8.2.33 runner produced byte-identical manifests with
SHA-256
`57959b27305707558a81d7e57e770b2b88255294195746d06d75f045851506d3`
and byte-identical summaries with SHA-256
`ad95a739ab281225a38808ef38929e85725f7fd85463016ac038b8f3d3930b7c`.

Relative to the retained `0afbae3` baseline, this checkpoint adds 21 exact
passes without losing a previous pass or adding a crash or timeout. PHP source
argument unpacking now uses a dedicated compiler and baseline-VM protocol
instead of treating `...` as `call_user_func_array()`. It preserves PHP array
reference cells and copy-on-write separation, consumes arrays and
`Traversable` objects with their integer or string keys, retains evaluation and
exception order, diagnoses invalid sources and key order, maps named and
variadic parameters, and grows detached call frames for large argument lists.
The contract applies to ordinary and dynamic functions, instance and static
methods, constructors and relevant internal variadics. Original clean-room
regressions cover those boundaries plus iterator errors, by-reference mutation,
named variadics and multi-array `array_map(null, ...)`.

The exact additions are all 14 cases under `Zend/tests/arg_unpack`, plus
`Zend/tests/bug75786.phpt`,
`Zend/tests/named_params/unknown_named_param.phpt`,
`Zend/tests/named_params/unpack.phpt`,
`Zend/tests/named_params/unpack_and_named_1.phpt`,
`Zend/tests/named_params/unpack_and_named_2.phpt`,
`Zend/tests/unpack_iterator_by_ref_type_check.phpt` and
`Zend/tests/vm_stack_with_arg_extend.phpt`. The pinned Symfony FrameworkBundle
7.4.16 S3 gate remains green against PHP 8.2.33 across cold and cached loads,
health and missing-route requests, deleted and malformed caches, and concurrent
atomic publication. This checkpoint does not claim complete call, iterator,
diagnostic-location or standard-library compatibility.

The preceding `0afbae3` checkpoint, relative to the retained `1f4352b`
baseline, adds the exact pass `Zend/tests/bug64660.phpt` without losing a
previous pass or adding a timeout. RPHP now measures structural delimiter
nesting before recursive descent, gives valid moderately deep sources a
dedicated parser stack, and turns an excessive source unit into PHP's
source-qualified `Parse error: memory exhausted` diagnostic. The same
diagnostic remains a catchable `ParseError` when the source is loaded through
`include`. This removes the selected corpus's last signal termination: the
pinned `Zend/tests` plus `tests/lang` gate now has zero crashes and zero
timeouts. It does not claim that every PHP program or every parser resource
boundary is supported.

The preceding `1f4352b` checkpoint, relative to the retained `a30abdd`
baseline, adds 31 exact passes without losing a previous pass or adding a
timeout. Concrete child
methods now enforce the same parameter contravariance, return covariance,
visibility, staticness and reference-mode contract as their effective concrete
parent declaration before the child is published. The shared variance path
resolves lexical `self` and `parent`, consuming-class trait scope, late-bound
`static`, class aliases, implicit `Stringable`, `iterable` as
`array|Traversable`, union/intersection relations and fixed-to-variadic
subsumption. The formerly recursive
`Zend/tests/type_declarations/variance/infinite_recursion.phpt` now produces
PHP's declaration fatal instead of overflowing the native stack, reducing the
crash count from two to one. The 43-case variance directory advances from four
to ten exact passes; its delayed class-loading and trait-composition failures
remain visible rather than being claimed as complete variance support.

The exact additions are `Zend/tests/bug51421.phpt`, `objects_008.phpt`,
`return_types/bug71978.phpt`, `return_types/inheritance001.phpt`,
`return_types/inheritance002.phpt`, `return_types/never_no_variance.phpt`,
`stringable_trait.phpt`; intersection-variance cases `invalid1`, `invalid2`,
`invalid3`, `invalid_covariance_drop_type1` and
`invalid_covariance_drop_type2`; mixed-inheritance parameter cases `error1`,
`error2` and `error4` plus return cases `error1`, `error2` and `error3`; both
`type_declarations/parameter_type_variance` cases; union-variance cases
`invalid_001`, `invalid_002` and `invalid_003`; variance cases
`class_order_autoload_error1`, `class_order_autoload_error2`,
`class_order_autoload_error3`, `infinite_recursion`,
`parent_in_class_failure2` and `static_variance_failure`; plus
`variadic/illegal_variadic_override_ref` and
`variadic/illegal_variadic_override_type`. At that checkpoint the remaining
signal termination was `Zend/tests/bug64660.phpt`; the parser-resource
checkpoint closes it. Full delayed class linking/autoload validation remains
separate work.

The preceding `a30abdd` checkpoint, relative to the retained `8aade89`
baseline, added seven exact passes without losing a previous pass or adding a
crash or timeout. It also
removed the previous timeout in `Zend/tests/try/finally_goto_005.phpt`.
Non-local `goto`, `break` and `continue` now run every intervening `finally`
block before reaching their destination; a `return` or `throw` from that
`finally` supersedes the pending transfer. The other exact additions are
`Zend/tests/bug66608.phpt`, `try/try_catch_finally_005.phpt`,
`try/try_catch_finally_006.phpt`, `try/try_catch_finally_007.phpt`,
`try/try_finally_014.phpt` and `try/try_finally_018.phpt`.

The retained Symfony S3 checkpoint left its then-current exact PHPT pass set
unchanged while closing the general semantics that prevented a valid cold
route cache. List
destructuring can assign ordinary, dynamic and nested `$this` properties;
namespaced `callable|false` types keep the reserved literal type; by-value
`foreach` writes through an existing reference, while by-reference `foreach`
rebinds its value CV for every element. Appending a referenced array element
also rebinds the local CV instead of overwriting its previous external target.
These contracts preserve Symfony EventDispatcher's independently captured lazy
listeners, so the kernel request invokes routing and publishes
`url_matching_routes.php` rather than silently reusing the final listener.
Ordinary proven frame-local by-value loops retain a separate branch-free
opcode; paired controls showed no regression above the one-percent checkpoint
ceiling.

This checkpoint does not claim complete control-flow, reference, destructuring
or literal-type compatibility, nor the runtime-resolved call, reference-return,
indirect `ArrayAccess`, `WeakMap` or general uncaught stack-trace contracts
already visible in the retained failure manifest.

The preceding `8aade89` checkpoint, relative to the retained `86ac187`
baseline, added one exact pass without losing a previous pass or adding a crash
or timeout. A positional call to a statically resolved function may use an
intermediate empty dimension such as `$input[][$key]`. A known by-value
parameter evaluates the base and key before raising the catchable PHP 8.2
`Cannot use [] for reading` `Error`, without appending an element. A known
by-reference parameter binds direct and multidimensional appended paths,
evaluates keys before the append, and publishes nested-array or object-property
writeback before entering the callee so mutations persist if the callee throws.
The exact addition is `Zend/tests/func_arg_fetch_optimization.phpt`.

The preceding `86ac187` checkpoint, relative to the retained `8c7107e`
baseline, added one exact pass without losing a previous pass or adding a crash
or timeout. A positional call to a statically resolved function may bind an
appended array slot to a known by-reference parameter. The baseline VM preserves
the owned reference cell and writes modified nested-array and object-property
containers back after the call. The exact addition is `Zend/tests/032.phpt`;
the adjacent by-value diagnostic in `Zend/tests/031.phpt` remains an exact pass.

The preceding `8c7107e` checkpoint, relative to the retained `2621cc3`
baseline, added 12 exact passes without losing a previous pass or adding a crash
or timeout. PHP 8.2 empty array dimensions now produce compile-stage
`Cannot use [] for reading`
or `Cannot use [] for unsetting` diagnostics, including constant-dead code,
coalescing and constant-expression contexts. The diagnostic retains the base
expression's source line across a multiline suffix. Direct and nested final
append writes remain valid.

The exact additions are `Zend/tests/031.phpt`, `bug41351`, `bug41351_2`,
`bug41351_3`, `bug70183`, `bug70912`,
`constant_expressions_coalesce_empty_dim`, `errmsg_006`, `errmsg_007`,
`errmsg_008`, and `restrict_globals/invalid_append_isset` plus
`restrict_globals/invalid_append_unset`. Five previous parse failures now reach
a later runtime failure: `032`, `ArrayAccess_indirect_append`, `bug34064`,
`func_arg_fetch_optimization`, and `weakrefs/weakmap_error_conditions`. At that
checkpoint they exposed separate append-lvalue contracts for by-reference calls
and returns, indirect `ArrayAccess`, catchable argument fetches, and intermediate
empty dimensions such as `$map[][1]`; none was counted as a pass there.

The preceding `2621cc3` checkpoint, relative to the retained `f1fb5e9`
baseline, added 12 exact
passes without losing a previous pass or adding a crash or timeout. PHP 8.2's
case-sensitive `$GLOBALS` root is now distinct from an ordinary `$globals`
local. Reading the root materializes a by-value snapshot of the active global
symbol table, while direct and nested dimensions retain writeback to that
table. Direct root assignment, append, compound/coalescing assignment,
increment, destructuring, `foreach`, `unset`, reference acquisition and closure
capture fail at PHP's compile stage, including code in a constant-dead branch.
Passing the root to a positional user-function reference parameter raises the
catchable PHP 8.2 `Error`, including calls compiled before the function
declaration.

The exact additions are `Zend/tests/array_add_indirect.phpt`,
`Zend/tests/closure_use_auto_global.phpt`, and ten cases under
`Zend/tests/restrict_globals`: `invalid_append`, `invalid_assign`,
`invalid_assign_list`, `invalid_assign_op`, `invalid_assign_ref_lhs`,
`invalid_assign_ref_rhs`, `invalid_foreach`, `invalid_foreach_ref`,
`invalid_pass_by_ref`, and `invalid_unset`. The retained failure
`Zend/tests/bug63882_2.phpt` moves from the public `output` category to
`compile` only because the existing runner heuristic sees the word
`Unsupported` in a runtime operand-type fatal; a direct probe confirms that
the test body reached comparison execution, so this is not a front-end
regression.

This does not claim complete `$GLOBALS` compatibility. The selected 17-case
root/capture cluster passes 14 cases. References inside destructuring syntax,
variable-variable syntax such as `${1}`, generic undefined-variable warnings,
and reference-argument forms beyond the exercised positional user-function
calls remain separate work.

The retained `f1fb5e9` checkpoint, relative to `9db688b`, added four exact
passes without losing a previous pass or moving another case to a worse status.
Direct `$GLOBALS` dimensions now operate on the request's global symbol table,
canonicalize PHP array keys to symbol names, and synchronize with globals bound
to the main frame. Reads, `isset`, assignment, `unset`, increment, compound and
append writeback, `??=`, and references are covered by an original clean-room
regression. The exact additions are `Zend/tests/035.phpt`,
`Zend/tests/arrow_functions/004.phpt`, `Zend/tests/unset_cv02.phpt`, and
`tests/lang/030.phpt`. `Zend/tests/closure_019.phpt` now executes its global
callable accesses and advances from a runtime failure to a later output mismatch.

The retained `9db688b` checkpoint, relative to `5173160`, added nine exact
passes without losing a previous pass or moving another case to a worse status.
An `include` now invalidates caller-local scalar and receiver facts before later
specialization, so included code may unset or replace a previously proven local
without stale bytecode or an unchecked read. Generic `echo` reports an undefined
local at its original file and line and respects the active `E_WARNING` mask.
Suppressed calls carry PHP 8.2's reporting mask through their callee frame,
restore it on return or exception unwind, and preserve an explicit
`error_reporting()` change made inside that frame.

The compiler also rejects the case-sensitive reserved `$this` parameter before
it can alias hidden receiver storage, retains the declaration line, and surfaces
a nested declaration error even when it is discovered in the final statement of
a function or method. The exact additions are `bug41117_1`, `bug71737`,
`error_reporting03`, `error_reporting08`, `this_as_parameter`, `unset_cv01`,
`unset_cv03`, `unset_cv04` and `tests/lang/bug23584`; `unset_cv04` changes from
signal termination to pass. Two pre-existing crashes and one timeout remain
explicitly visible in the coverage map. General non-call `@` warning routing
and complete user error-handler dispatch remain separate compatibility work.

The authoritative per-path result is
[`8bcc548-arm64-manifest.jsonl`](../tests/php-src/results/php-8.2.33/8bcc548-arm64-manifest.jsonl),
with aggregate metadata in
[`8bcc548-arm64-summary.json`](../tests/php-src/results/php-8.2.33/8bcc548-arm64-summary.json),
a directory/status navigation map and exact hazard list in
[`8bcc548-arm64-coverage-map.json`](../tests/php-src/results/php-8.2.33/8bcc548-arm64-coverage-map.json),
and the full reference aggregate in
[`reference-arm64-summary.json`](../tests/php-src/results/php-8.2.33/reference-arm64-summary.json),
with image and official-runner cross-checks in
[`reference-validation.json`](../tests/php-src/results/php-8.2.33/reference-validation.json).
Every upstream path remains visible; the rollup never replaces the manifest.

To reproduce the RPHP contract run from the exact external checkout:

```sh
cargo build --locked --release --all-features
RPHP_PHPT_PHP_SRC_COMMIT=651db3ebfa622cae0c4e6b39766812efbd274ced \
RPHP_PHPT_REFERENCE_PHP=/path/to/php-8.2.33 \
RPHP_PHPT_TIMEOUT=3 scripts/run-php-src-phpt.sh \
  /path/to/php-src target/release/rphp /tmp/rphp-phpt-results 4
```

### Retained PHP 8.4 trend history

The older PHP 8.4.21 audit remains useful as a non-contract trend line. Its
counts are not directly comparable with the PHP 8.2.33 contract corpus.

Relative to the retained `c470183` baseline, this run adds 24 passing cases
without losing a previous pass. PHP class aliases now share the exact
reference-counted class identity rather than copying a class definition, so
alias chains preserve methods, static state, inheritance, callbacks and
`instanceof` behavior across classes, interfaces, traits and enums.
`stdClass` is registered as a real internal class; `is_a()` and
`is_subclass_of()` honor string/autoload behavior and shared alias identity;
`function_exists()` accepts a leading namespace separator; and `var_dump()` is
variadic. Three previously crashing `try` cases now terminate normally, taking
the crash total from seven to four. The same 2,700 test bodies get past the
front end: this slice converts 24 already-runtime-reaching cases to exact
passes, reduces ordinary runtime failures by 51 and moves 30 cases to later
output comparison.

Relative to the retained `0fafdd4` baseline, the current run adds 36 passing
cases without losing a previous pass. The retained `c470183` checkpoint
isolates 12 of them. Request-local SPL callback stacks preserve
registration order, prepend, duplicate and unregister identity, guard recursive
lookups and propagate loader exceptions. `class_exists()`, `interface_exists()`,
`trait_exists()` and `enum_exists()` autoload the correct symbol kind while
honoring the opt-out argument and case-insensitive names. `method_exists()` now
autoloads string owners and sees abstract or non-public declarations instead of
using the stricter callback-callability test. Object-method callbacks can load
classes through ordinary unmodified `require`, matching Composer's primary
loader shape.

## Composer and Symfony gates

The reproducible Composer S0 gate now pins Composer 2.8.12 by version and PHAR
SHA-256, generates `vendor/` under reference PHP, and runs the resulting
unmodified `vendor/autoload.php` under RPHP. Its fixture verifies the returned
Composer loader object, the exact PHP 8.2.0 constants/function identity, direct
static PSR-4 class autoloading, and a Composer `files` function. A second
Composer-generated platform check requiring PHP 8.2.1 must fail under RPHP with
a `RuntimeException` that reports 8.2.0. This exercises include-expression
return values, direct static-call autoload, both forms of `strtr()`, and both
sides of Composer's generated PHP-version gate; it does not claim that Composer
itself runs under RPHP. Symfony support is admitted separately: the first
bounded S1 gates now execute the unmodified EventDispatcher 7.4.15,
HttpFoundation 7.4.16 and compiled Routing 7.4.15 components through
Composer's generated autoloader, plus a Symfony DependencyInjection 7.4.16
prebuilt container generated by reference PHP. The next bounded S2 diagnostic
loads a complete production FrameworkBundle 7.4.16 container and route cache
warmed by reference PHP and handles one synthetic `/health` request.
`extension_loaded()` remains conservatively false until individual extension
contracts are admitted.

The pinned Symfony EventDispatcher S1 fixture installs
`symfony/event-dispatcher` 7.4.15 and its locked dependencies with reference
PHP, then runs the unmodified vendor sources under both PHP and RPHP. Its
priority-ordered static callable scenario must produce `high>low|same`. This
admits the component's exercised dispatch path, including lazy `??=` writes,
by-reference listener sorting, nested append/writeback and dynamic callable
arrays; it is not a blanket compatibility claim for every EventDispatcher API
branch or the rest of Symfony.

The pinned Symfony HttpFoundation S1 fixture creates an unmodified synthetic
CLI `Request` and `Response` with query, form and header inputs, then compares
reference PHP and RPHP byte-for-byte. Its required output is
`201|http-foundation|POST|/hello|RPHP|value|alpha`. This admits only the
exercised request/response and header-bag path; files, cookies, sessions,
trusted proxies, streamed responses and a real HTTP SAPI remain separate
gates.

The pinned Symfony Routing S1 fixture builds real `Route` objects, dumps their
compiled matcher tables and executes the unmodified `CompiledUrlMatcher`. It
compares a static GET, a constrained dynamic route with extracted defaults, a
405 including its allowed methods and a 404 against reference PHP. Its exact
output is `health|article_show:rphp-8:html|405:GET,HEAD|404`. This admits the
exercised compiled matcher path, including Symfony's branch-reset/MARK regex;
URL generation, localized/host routes, condition expressions and broader route
loader formats remain separate gates.

The pinned Symfony DependencyInjection S1 fixture asks the unmodified
`ContainerBuilder` and `PhpDumper` 7.4.16 to compile a small graph under
reference PHP, then executes the generated PHP unchanged under RPHP. It checks
constructor injection, an inlined private dependency, a public alias, shared
service identity, a compiled parameter lookup and a missing-service probe. Its exact
output is `hello RPHP|same|hello|missing`. The generated class deliberately
uses a minimal fixture base so this gate does not claim the full Symfony
`Container`, lazy services, service locators, environment processors,
Reflection-driven compilation or a cold RPHP container build; those remain
S2/S3 gates.

The pinned Symfony warmed-kernel S2 fixture installs FrameworkBundle 7.4.16
and its exact locked closure with checksum-pinned Composer 2.8.12. Reference
PHP boots a `prod`, `debug=false` kernel and generates the container and
compiled route cache. RPHP loads those generated files unchanged, handles an
in-memory GET `/health`, runs kernel termination and shutdown, and must match
reference PHP exactly at `200|warmed|OK`; the gate also verifies that RPHP did
not mutate the warmed cache. This admits only the exercised short-lived CLI
lifecycle and does not by itself admit cold cache generation. The separate S3
gate below covers the pinned cold build; HTTP and repeated-worker behavior
remain S4 work.

The pinned Symfony cold-kernel S3 fixture installs the same unmodified
FrameworkBundle 7.4.16 dependency closure from its exact lock with
checksum-pinned Composer 2.8.12. RPHP now builds both the production container
and compiled route cache from the PHP fixture, and a fresh RPHP process returns
the PHP 8.2 oracle results for `/health` and a missing route. The gate also
compares cold, cached, deleted-cache and deliberately malformed route-cache
transitions, verifies same-runtime cache immutability, lints every generated
PHP file, and runs two concurrent cold publishers before a fresh cache load.
Generated container, ghost and service-locator suffixes are normalized; cache
metadata paths are fixture-relative and Reflection resource hashes are omitted.
The two concurrent cold-publisher captures are compared as an unordered exact
multiset because the operating system does not assign the cache-builder role
to a stable process number; each process's status, diagnostics and canonical
output remain in its capture signature. Array `str_replace()` sanitization is
covered independently so random container hashes cannot publish a `/` as a
namespace separator.

Included-file and declared-symbol comparison is restricted to the generated
container, route loader/cache and health service boundary so host-selected
environment and secrets loader services do not redefine the fixture. The full
normalized cache file manifest is still compared. This admits only the named
short-lived Symfony CLI kernel; an HTTP SAPI, request superglobals and
repeated-worker isolation remain S4 work.

## Retained implementation and PHP 8.4 trend notes

The following implementation notes and pass-set comparisons describe retained
checkpoints from the PHP 8.4.21 trend audit.

`use function` imports have their own case-insensitive alias table, separate
from class imports. Direct calls support default, explicit and comma-separated
aliases, retain imports inside methods and closures, and suppress the ordinary
namespaced-to-global fallback for an explicitly imported target. Imports remain
lexical, so string names passed to `function_exists()` are unchanged.

Includes inside functions and methods inherit caller locals, `$this`, and
lexical private-access scope without leaking local bridge values into the real
global table. Explicit `global` declarations still bind the real global.
Included throws, missing required files, and parse failures propagate through
the caller's catch table; `ParseError` is registered under `CompileError`, which
extends `Error`.

Relative to the retained `102800d` baseline, the current run adds 40 passing
cases without losing a previous pass. The retained `0fafdd4` checkpoint isolates
four of them: object-property chains became legal `isset` targets, intermediate
null/scalar/property reads remain silent, inaccessible and uninitialized
properties do not leak, multiple operands short-circuit, and
`__isset`/`__get` run in PHP order with catchable exceptions. The same exception
propagation repair also closes an older ordinary `__get` regression.

Relative to the retained `9c8812d` baseline, this run adds 98 passing cases
without losing a previous pass. A single postfix loop now covers offsets,
dynamic calls, object access and class constants for every primary atom instead
of selected parser entry points. Source-aware, case-insensitive magic constants
cover file, directory, line, namespace, class, trait, function and method scope;
fully-qualified built-in constants also discard their global namespace marker
before lookup. Includes receive their own canonical source context. These
shared changes reduce runtime failures by 169 and parser failures by 42; some
tests now reach later output or compile-time checks, so those classifications
increase even though the exact pass set only grows.

Relative to the retained `1a5a270` baseline, this run adds 277 passing cases
without losing a previous pass, reduces parser failures by 786 and reduces
signal-terminated cases from 14 to 4. The measured change covers standard
comma-separated `echo`, standalone `print` statements and generator-safe call
argument suspension, empty/no-op and general expression statements, heredoc and
nowdoc strings, body-less abstract class and trait method contracts, and
class-like constants across classes, interfaces, traits and enums. Constant
support includes visibility, `final` and typed declarations, inheritance and
composition checks, late-static reads, enum aliases and common constant
expressions; repeated runtime reads use an ID-and-index inline cache.
Dynamic class-constant fetch now accepts runtime string or object owners,
braced names, `self`/`parent`/late-static scope, enum cases and constant
expressions while preserving PHP's `::class`, visibility, type-error and
left-to-right evaluation rules. Owner/name cache entries are revalidated
independently. Function-call results also continue through ordinary postfix
method/property chains, which accounts for several generator-suite passes.
Interface and abstract declarations now share compile-time LSP validation for
arity, variadics, reference mode, visibility, staticness and declared types. It
does not infer support for the remaining downstream behavior of every case that
now parses. The retained `f6a20c1`, `1bc6650`, `18e4dde`, `81de421`, `7ab1941`,
`21f2d98`, `67a924d`, `e107bf2`, `42ea718`, `9c8812d`, `102800d`, `0fafdd4` and
`c470183` results isolate the earlier syntax, abstract-method, shared-contract,
string-literal, class-constant, postfix, magic-constant, `isset` and autoload
uplifts.

An audit found that the earlier runner did not preserve php-src's generated
`.php` basename and section-ending newline, omitted the `%0` `EXPECTF`
placeholder, and did not reproduce the reference CLI's INI defaults. Schema 2
fixes those issues and reports upstream `XFAIL` separately. A full oracle run
through the corrected runner on the exact official PHP 8.4.21 CLI produced
5,132 passes, zero ordinary failures, 121 skips, five unsupported SAPI sections
and the one upstream `XFAIL`. The set of 522 RPHP passes is byte-for-byte
unchanged from `e107bf2`; the correction therefore validates the low strict
rate instead of inflating it.

The largest first-error parser clusters in the latest audit are property hook
or braced-member shapes (232), reference assignment expressions (155),
anonymous classes (80), unpacking expressions (74), identifier grammar (66),
variable-variable names (59), and returned-by-reference declarations (57).
PHPT is an exact-output conformance suite: a correct rejection with different
diagnostic text still fails, so this strict percentage is deliberately lower
than feature reach.

A fresh all-features rerun of retained commit `102800d` over all 5,259 pinned
cases reproduced every published status and aggregate exactly. Together with
the 5,132-pass, zero-failure reference-PHP oracle, this rules out sharding,
discovery, and expectation matching as the cause of the low strict rate. The
remaining gap is in RPHP behavior and PHP-compatible diagnostics, not a hidden
runner divisor.

The current schema-4 audit also found a measurement bug that affected using
PHP 8.5 as a local oracle for the pinned PHP 8.4 suite: PHP 8.5 enables
compile-time fatal backtraces by default. The runner now restores the PHP 8.4
diagnostic profile for reference runs and covers it with a regression fixture.
It also records the runner commit independently and separates failed
pre-execution setup and exact negative-test passes in the execution profile.
Rerunning RPHP changed none of the 5,259 path statuses or categories, so this
fix corrects calibration and descriptive reach without inflating the 627-pass
compatibility result.

The dependency-free project runner supports `FILE`, `FILEEOF`,
`FILE_EXTERNAL`, `EXPECT`, `EXPECTF`, `EXPECTREGEX`, `SKIPIF`, `INI`, `ENV`,
`ARGS`, `STDIN`, `CLEAN` and extension declarations, with explicit capability
classification where the runtime cannot execute a section. Its expectation and
section handling are continuously covered by local fixtures and the complete
oracle result above.

The complete machine-readable result is committed as
[`80137b6-arm64-manifest.jsonl`](../tests/php-src/results/php-8.4.21/80137b6-arm64-manifest.jsonl),
with aggregate metadata in
[`80137b6-arm64-summary.json`](../tests/php-src/results/php-8.4.21/80137b6-arm64-summary.json).
Every upstream path remains visible with its pass/fail/skip/XFAIL/unsupported/
timeout/crash status and classification.

To reproduce the run from an external checkout pinned to the commit above:

```sh
cargo build --locked --release --all-features
RPHP_PHPT_TIMEOUT=3 scripts/run-php-src-phpt.sh \
  /path/to/php-src target/release/rphp /tmp/rphp-phpt-results 4
```

## Default build

End-to-end tests cover representative behavior in these areas:

- scalar values, arithmetic, comparisons, strings, arrays, references, and
  copy-on-write paths;
- conditionals, loops, `foreach`, `switch`, `match`, null coalescing, and
  nullsafe access;
- functions, closures, arrow functions, variadics, named arguments,
  generators, selected callable forms, and request-local SPL autoload stacks;
- classes, constructors, inheritance, interfaces, traits, visibility, clone,
  magic methods, constants, namespaces, and `instanceof`;
- scalar/object type declarations, strict types, exceptions, `try`/`catch`/
  `finally`, and selected reflection behavior;
- selected standard-library functions, JSON, regular expressions, and core
  stream behavior.

This list describes tested areas, not complete coverage of each PHP feature.
Exact supported edge cases are defined by the source and tests on the commit
being run.

## Opt-in surfaces

| Cargo feature | Status |
| --- | --- |
| `jit-prototype` | Native execution for selected guarded regions on macOS/AArch64 and Linux/x86-64 |
| `coroutines` | Experimental structured cooperative tasks, channels, timers, and readiness APIs |
| `php-generics-erased` | Experimental generic syntax with bound-erased runtime behavior |
| `php-generics-reified` | Experimental generic syntax with reified runtime sidecars and checks |
| `file-contents`, `file-write`, `file-lines`, `include-path` | Incremental file/include functionality |
| `stream-*`, `csv-*`, `resource-lifetime`, `value-errors` | Incremental stream, resource, and error behavior |
| `vm-stats` | Internal execution and JIT diagnostics |

The generic syntax is an RPHP experiment, not part of the default PHP
compatibility claim. Feature names and APIs can change without deprecation
during pre-alpha development.

## Known major gaps

- complete PHP syntax, edge-case semantics, standard library, and extension
  coverage;
- Composer packages and framework applications, including Symfony;
- HTTP/FPM or another production web SAPI, WebSockets, and server lifecycle;
- databases, Redis, cURL, and most external integrations;
- production cycle collection, resource limits, sandboxing, and security
  hardening;
- BigInt/BigDecimal and broad specialist extension compatibility;
- complete JIT coverage and supported native backends beyond the two primary
  CI targets;
- stable embedding, extension, bytecode, CLI, or Rust APIs.

Missing behavior may fail at lexing, parsing, compilation, or execution. Error
messages and exit codes are not yet fully PHP-compatible.

## Reporting compatibility bugs

Include the exact RPHP commit, build command and feature flags, target and OS,
a minimal PHP program, actual output, expected output, and the reference PHP
version/configuration. Do not report security-sensitive cases publicly; use
the process in `SECURITY.md`.
