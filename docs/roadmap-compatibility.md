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

The `0eed303` declaration-contract checkpoint reaches 1,962 passes with 3,246
failures, 110 skips, one XFAIL, 280 unsupported cases, zero timeouts and zero
crashes. `never`/`void` parameter and value-return restrictions are enforced
while compiling every declaration shape, and generator return types must
statically contain a `Generator` supertype. The exact PHP 8.5.6 delta is
+10/-0 with no stage movement; the full feature matrix and unsafe gate pass.
No performance gate applies because validation is compile-time only.

The `cde7195` Generator debug-info checkpoint reaches 1,968 passes with 3,240
failures, 110 skips, one XFAIL, 280 unsupported cases, zero timeouts and zero
crashes. Generator dumps expose the retained public function name in every
lifecycle state without storing a duplicate property. The exact PHP 8.5.6
delta is +6/-0 with no stage movement; the full feature matrix and unsafe gate
pass. No performance gate applies because the work occurs only in `var_dump()`.

The `c1f7ad2` Generator lifecycle checkpoint reaches 1,972 passes with 3,236
failures, 110 skips, one XFAIL, 280 unsupported cases, zero timeouts and zero
crashes. Explicit integer yield keys advance, but never rewind, the next
implicit key and preserve signed overflow behavior. `Generator::getReturn()`
auto-primes a new generator and distinguishes a normal return from exceptional
closure with PHP's catchable incomplete-state exception. The exact PHP 8.5.6
delta is +4/-0; the full feature matrix and unsafe gate pass. No performance
gate applies because the ordinary implicit-yield path is unchanged and return
inspection is cold.

The `d0cd311` Generator exception-injection checkpoint reaches 1,978 passes
with 3,230 failures, 110 skips, one XFAIL, 280 unsupported cases, zero timeouts
and zero crashes. `Generator::throw()` auto-primes new generators, injects a
`Throwable` into direct and delegated suspension points, returns the next
yield after a catch, and propagates escaped or closed-generator exceptions to
the caller. The exact PHP 8.5.6 delta is +6/-0; the full feature matrix and
unsafe gate pass. No performance gate applies because injection is an explicit
cold method and ordinary generator advancement remains unchanged.

The `5bf280a` Generator object-invariant checkpoint reaches 1,983 passes with
3,225 failures, 110 skips, one XFAIL, 280 unsupported cases, zero timeouts and
zero crashes. Generator cloning and dynamic properties are rejected, engine
serialization is forbidden, rewind eligibility survives only through the
first suspension, and `count()` implements the PHP 8.5 array/`Countable`
contract with receiver lifetime preservation. The exact PHP 8.5.6 delta is
+5/-0. The feature matrix and unsafe gate pass; the established 200-pair
generator resume gate measured -2.155%, within the one-percent regression
ceiling without treating the negative sample as an optimization claim.

The `f444173` internal exception-trace checkpoint reaches 1,989 passes with
3,219 failures, 110 skips, one XFAIL, 280 unsupported cases, zero timeouts and
zero crashes. Internal call frames are captured before cleanup, detached
generator rethrows reconnect their generator/method/caller chain, and supplied
Throwable objects keep their creation trace. The exact PHP 8.5.6 delta is
+6/-0. The feature matrix and unsafe gate pass, reducing the unsafe-block
inventory from 1,623 to 1,620. The 100-pair hot `strlen()` internal-call gate
measured -3.762%, within the one-percent regression ceiling without claiming
the negative sample as an optimization.

The `258f03f` reentrant Generator checkpoint reaches 1,992 passes with 3,216
failures, 110 skips, one XFAIL, 280 unsupported cases, zero timeouts and zero
crashes. `Generator::next()`, `send()` and `throw()` now expose a catchable
PHP `Error` while an instance is already running, and escaped traces join their
internal prefix to the detached generator and live caller continuation. The
exact PHP 8.5.6 delta is +3/-0. All five CI feature configurations, the
all-target check and unsafe gate pass. The 200-pair generator resume control
measured -1.216%, within the one-percent regression ceiling without claiming
the negative sample as an optimization.

The `2b90e00` generator self-delegation checkpoint reaches 1,993 passes with
3,215 failures, 110 skips, one XFAIL, 280 unsupported cases, zero timeouts and
zero crashes. A running generator now rejects `yield from` itself with PHP
8.5's catchable `Error` before any conflicting state borrow or partial
delegation, removing the prior Rust panic. The exact PHP 8.5.6 delta is +1/-0.
All five CI feature configurations, the all-target check and unsafe gate pass.
The 200-pair generator resume control measured +0.436%, within the one-percent
regression ceiling.

The `73539fa` keyword/yield precedence checkpoint reaches 1,999 passes with
3,209 failures, 110 skips, one XFAIL, 280 unsupported cases, zero timeouts and
zero crashes. Lexer tokens now distinguish keyword `and`/`or`, all three
keyword logical operators bind below assignment and yield, and yield operands
admit assignment, nesting and surrounding multiplicative use. The exact PHP
8.5.6 delta is +6/-0 and runtime reach rises to 83.007%. All five CI feature
configurations, the all-target check and unsafe gate pass. No runtime
performance gate applies because generated bytecode and execution paths are
unchanged.

The `64dfea6` binary-right-assignment checkpoint reaches 2,001 passes with
3,207 failures, 110 skips, one XFAIL, 280 unsupported cases, zero timeouts and
zero crashes. Power, multiplicative, additive, shift and bitwise operators now
bind a value-producing assignment inside their right operand as PHP does. The
exact PHP 8.5.6 delta is +2/-0 and runtime reach rises to 83.065%. All five CI
feature configurations, the all-target check and unsafe gate pass. No runtime
performance gate applies because generated bytecode and execution paths are
unchanged.

The `91b8804` asymmetric-set-visibility checkpoint reaches 2,017 passes with
3,191 failures, 110 skips, one XFAIL, 280 unsupported cases, zero timeouts and
zero crashes. Typed instance and promoted properties preserve separate read
and `private(set)`/`protected(set)`/`public(set)` scopes across assignment,
indirect array mutation, references, unsets and inheritance. The exact PHP
8.5.6 delta is +16/-0 and runtime reach rises to 83.698%. Static asymmetric
properties, hooks and some declaration diagnostics remain explicit follow-up
work. All five CI feature configurations, the all-target and unsafe gates pass.
The instance-property A/B control is +0.448%; all specialized property lanes
remain below their five-percent regression ceiling.

The `925bc63` static-asymmetric-set checkpoint reaches 2,018 passes with 3,190
failures, 110 skips, one XFAIL, 280 unsupported cases, zero timeouts and zero
crashes. Static properties now enforce their separate set scope across direct
and indirect writes, references and warmed caches while preserving nested
object mutation and silent unset behavior. The exact PHP 8.5.6 delta is +1/-0
and runtime reach rises to 83.717%. All five CI feature configurations, the
all-target and unsafe gates pass. Static `self::` and `static::` read controls
measure +0.068% and +0.188%; specialized read/write lanes remain under their
five-percent ceiling. Hooks and source-located declaration diagnostics remain
the asymmetric-property follow-up.

The `11f2046` property-declaration-location checkpoint reaches 2,046 passes
with 3,162 failures, 110 skips, one XFAIL, 280 unsupported cases, zero timeouts
and zero crashes. Property AST and cold declaration metadata retain exact
source lines, duplicate asymmetric modifiers become located compile fatals,
and asymmetric, readonly and invariant inheritance diagnostics use PHP's
file/line shape. The exact PHP 8.5.6 delta is +28/-0. Fifteen remaining
diagnostic failures move from the runner's runtime category to the correct
compile stage because their location now matches; no executable pass regresses.
All five feature configurations, all-target and unsafe gates pass. The general
property A/B control is -0.281%, and four specialized lanes range from -0.742%
to +3.264%, below their five-percent ceiling. Property hooks and the remaining
message/type-normalization slices are explicit follow-up work.

The `3d2dad8` invariant-property-type checkpoint reaches 2,060 passes with
3,148 failures, 110 skips, one XFAIL, 280 unsupported cases, zero timeouts and
zero crashes. Property invariance now uses mutual subtype reduction for class
inheritance, intersections, unions, link-time aliases and `iterable`; invalid
parent types use canonical DNF/built-in/omitted rendering and the child class
line. The exact PHP 8.5.6 delta is +14/-0 with no remaining-failure stage move.
All five feature configurations, all-target and unsafe gates pass. No runtime
performance gate applies because execution paths and runtime layouts are
unchanged. Runtime-published aliases still require delayed class linking;
property hooks remain the larger language frontier.

The `e1f9658` delayed-property-linking checkpoint reaches 2,061 passes with
3,147 failures, 110 skips, one XFAIL, 280 unsupported cases, zero timeouts and
zero crashes. Named children whose invariant property contracts depend on an
earlier runtime `class_alias()` remain unpublished until the alias resolves
them; unresolved or incompatible contracts still fail at request completion.
The exact PHP 8.5.6 delta is +1/-0, with the complete prior pass set preserved.
All five feature configurations, all-target and unsafe gates pass. The queue is
cold request-global class metadata and does not alter object, value, frame or
successful property-access paths, so no hot-path performance gate applies.
Property hooks remain the nearby language frontier.

The `0fc54e9` property-getter-hook checkpoint reaches 2,068 passes with 3,140
failures, 110 skips, one XFAIL, 280 unsupported cases, zero timeouts and zero
crashes. Block-form getters execute through ordinary user-method bytecode,
distinguish backed reentrance from virtual read-only properties, preserve hook
magic names and inheritance variance, and stay hidden from ordinary method
introspection. The exact PHP 8.5.6 delta is +7/-0. All feature configurations,
all-target and unsafe gates pass. The final ordinary property A/B result is
-0.718%; a 40-pair typed/untyped property-method lane is +2.176%, under its
five-percent ceiling. Setter, arrow, abstract/final, by-reference and Reflection
forms remain the next property-hook slices.

The `2b1df95` property-setter-hook checkpoint reaches 2,088 passes with 3,120
failures, 110 skips, one XFAIL, 280 unsupported cases, zero timeouts and zero
crashes. Block-form setters accept implicit or explicit value parameters,
preserve assignment-expression input, distinguish backed implicit reads from
virtual write-only properties and share a backing guard with getters. The exact
PHP 8.5.6 delta is +20/-0. All feature configurations, all-target and unsafe
gates pass. The ordinary property A/B workload is -1.824%; specialized lanes
range from -0.020% to +2.074%, within their five-percent ceiling. Arrow,
abstract/final, by-reference, parent-hook and Reflection forms remain next.

The `8d43a0b` property-hook-arrow checkpoint reaches 2,111 passes with 3,097
failures, 110 skips, one XFAIL, 280 unsupported cases, zero timeouts and zero
crashes. Arrow getters return their expression and arrow setters write the
transformed expression result through the existing backing guard while the
assignment retains its input value. The exact PHP 8.5.6 delta is +23/-0. All
five feature configurations, all-target and unsafe gates pass. The ordinary
property A/B workload is -1.565%; specialized lanes range from -1.066% to
+2.190%, within their five-percent ceiling. Abstract/final, by-reference,
parent-hook and Reflection forms remain next.

The `c92122f` final-property-hook checkpoint reaches 2,118 passes with 3,090
failures, 110 skips, one XFAIL, 280 unsupported cases, zero timeouts and zero
crashes. Final properties and hooks retain cold declaration metadata through
inheritance linking and report property-specific override and invalid-modifier
diagnostics. The exact PHP 8.5.6 delta is +7/-0. All five feature
configurations, all-target and unsafe gates pass. A cold flag reuses the source-
line metadata word so the hot property-definition size and layout remain
unchanged; the 40-pair ordinary property A/B workload is -2.109%, and
specialized lanes range from -1.409% to +1.636%, within their five-percent
ceiling. Abstract properties, by-reference hooks, parent-hook calls and
Reflection remain next.

The `7c0c535` abstract-property-hook checkpoint reaches 2,161 passes with 3,047
failures, 110 skips, one XFAIL, 280 unsupported cases, zero timeouts and zero
crashes. Abstract class, trait and interface properties retain body-less getter
and setter requirements as non-executable hook contracts; plain properties,
partial concrete overrides and readonly interface implementations are validated
against those capabilities. The exact PHP 8.5.6 delta is +43/-0. All five
feature configurations, all-target and unsafe gates pass. Abstract capability
flags share the existing cold source metadata word and validation helpers are
cold/non-inlined; the final ordinary property A/B workload is +0.571%, while
specialized lanes range from -0.774% to +1.218%.

The `08ad7a5` reference-property-hook checkpoint reaches 2,170 passes with
3,038 failures, 110 skips, one XFAIL, 280 unsupported cases, zero timeouts and
zero crashes. Reference-returning getters preserve the returned alias for
backed and virtual properties, plain properties satisfy reference-getter
interface contracts, and invalid by-value implementations, reference setter
parameters and backed inherited getter/setter combinations receive PHP 8.5
diagnostics. Virtual hook storage is also excluded from ordinary object dumps.
The exact PHP 8.5.6 delta is +9/-0, bringing the cumulative gain from the
initial baseline to +355/-0. All five feature configurations, all-target and
unsafe gates pass. The ordinary property A/B workload is -0.755%; specialized
read and write lanes are -0.120% and +0.013%, while method and constructor
lanes are +2.529% and +1.051%, within their five-percent ceiling. Indirect
array modification through reference getters, by-reference object iteration,
parent-hook calls and Reflection remain next.

The `2ad9fa5` indirect-reference-hook checkpoint reaches 2,174 passes with
3,034 failures, 110 skips, one XFAIL, 280 unsupported cases, zero timeouts and
zero crashes. Array mutation through a by-value getter now fails before
detached storage can change, while object results remain mutable. By-reference
object iteration invokes getter hooks, binds reference results, rejects value
results and retains typed-property constraints on ordinary aliases. The exact
PHP 8.5.6 delta is +4/-0, bringing the cumulative gain from the initial
baseline to +359/-0. All five feature configurations, all-target and unsafe
gates pass. The ordinary property A/B workload is -1.074%; a 20-pair hookless
object-foreach control is +0.102%, and specialized read/write/method/constructor
lanes are +0.039%, +0.118%, +2.381% and +0.435%. Full hooked object-iteration
ordering, parent-hook calls and Reflection remain next.

The `9e9141f` property-hook-declaration checkpoint reaches 2,184 passes with
3,024 failures, 110 skips, one XFAIL, 280 unsupported cases, zero timeouts and
zero crashes. Empty and duplicate hook lists, unknown hook names, illegal hook
modifiers, static hooked properties, getter parameter lists and invalid setter
parameter shapes now fail at declaration compilation with PHP 8.5's class,
property, hook and source-location diagnostics. The exact PHP 8.5.6 delta is
+10/-0, bringing the cumulative gain from the initial baseline to +369/-0.
All five feature configurations, all-target and unsafe gates pass. No runtime
performance gate applies because valid declarations retain identical bytecode
and the checks are parser/cold compiler work. Constructor-promoted hooks, full
hooked iteration ordering, parent-hook calls and Reflection remain next.

The `3854982` constructor-promoted-property-hook checkpoint reaches 2,194
passes with 3,014 failures, 110 skips, one XFAIL, 280 unsupported cases, zero
timeouts and zero crashes. Promoted parameters now retain full property and
hook declarations; implicit constructor writes invoke setters before the user
body, hook-only parameters are public, and final/readonly rules match PHP 8.5.
The exact PHP 8.5.6 delta is +10/-0, bringing the cumulative gain from the
initial baseline to +379/-0. All five feature configurations, all-target and
unsafe gates pass. The ordinary property A/B workload is +0.457%; specialized
read/write/method/constructor lanes are +0.522%, +0.387%, +2.991% and +2.526%,
within their ceilings. Reflection property-default support, assertion AST
rendering, full hooked iteration ordering and parent-hook calls remain next.

The `29f9a44` property-reflection-metadata checkpoint reaches 2,197 passes with
3,011 failures, 110 skips, one XFAIL, 280 unsupported cases, zero timeouts and
zero crashes. `ReflectionProperty` now distinguishes implicit-null, explicit,
typed-uninitialized and promoted defaults, emits the PHP 8.5 missing-default
deprecation, and exposes final, abstract and virtual hook flags. The exact PHP
8.5.6 delta is +3/-0, bringing the cumulative gain from the initial baseline to
+382/-0. All five feature configurations, all-target and unsafe gates pass.
The ordinary property A/B workload is -1.976%; specialized
read/write/method/constructor lanes are -0.183%, -0.206%, +1.869% and +1.934%,
within their ceilings. Hook method reflection, raw-value access, assertion AST
rendering, full hooked iteration ordering and parent-hook calls remain next.

The `ba8a678` explicit-parent-property-hook checkpoint reaches 2,203 passes
with 3,005 failures, 110 skips, one XFAIL, 280 unsupported cases, zero timeouts
and zero crashes. Matching `parent::$property::get()` and `set()` forms now use
the exact parent hook method while preserving `$this`; mismatched hook,
property and outside-hook forms receive PHP 8.5 compile diagnostics. The exact
PHP 8.5.6 delta is +6/-0, bringing the cumulative gain from the initial
baseline to +388/-0. All five feature configurations, all-target and unsafe
gates pass. No runtime performance gate applies because existing bytecode and
dispatch are unchanged. Implicit parent accessors for plain storage, hook
method/raw-value Reflection, assertion AST rendering and full hooked iteration
ordering remain next.

The `ba41208` parent-hook-front-end-diagnostic checkpoint reaches 2,205 passes
with 3,003 failures, 110 skips, one XFAIL, 280 unsupported cases, zero timeouts
and zero crashes. Parent hook syntax outside class scope and writable call
results now fail at compilation with PHP 8.5 diagnostics. The exact PHP 8.5.6
delta is +2/-0, bringing the cumulative gain from the initial baseline to
+390/-0. All five feature configurations, all-target and unsafe gates pass; no
runtime performance gate applies because valid bytecode is unchanged.
Implicit parent accessors for plain storage remain the next parent-hook slice.

The `d85aaa9` implicit-parent-property-accessor checkpoint reaches 2,218 passes
with 2,990 failures, 110 skips, one XFAIL, 280 unsupported cases, zero timeouts
and zero crashes. Plain backed parent properties now expose implicit exact-arity
get/set accessors to overriding hooks, use the parent's backing slot without
child redispatch, and produce catchable property visibility, undefined-property
and missing-parent diagnostics. Explicit hooks retain user-function argument
behavior. The exact PHP 8.5.6 delta is +13/-0, bringing the cumulative gain
from the initial baseline to +403/-0. All five feature configurations,
all-target, unsafe, Composer S0, Symfony S1 and warmed-kernel S2 gates pass.
Typed/untyped read/write/method/constructor lanes are -0.642%, -0.742%,
+0.982% and +2.863%, within their five-percent ceiling. Parenthesized parent
property-call syntax remains a separate parser/AST slice.

The `dffa9c0` parenthesized-static-property checkpoint reaches 2,219 passes
with 2,989 failures, 110 skips, one XFAIL, 280 unsupported cases, zero timeouts
and zero crashes. Parser AST metadata now distinguishes a direct parent
property-hook call from a parenthesized static-property value followed by a
dynamic static call. The exact PHP 8.5.6 delta is +1/-0, bringing the cumulative
gain from the initial baseline to +404/-0. All five feature configurations,
all-target and unsafe gates pass. No runtime performance gate applies because
the change does not alter runtime structures, bytecode or VM dispatch.

The `f07d8be` hooked-property-unset checkpoint reaches 2,220 passes with 2,988
failures, 110 skips, one XFAIL, 280 unsupported cases, zero timeouts and zero
crashes. Accessible backed, virtual, uninitialized and inherited hooked
properties now reject `unset()` with PHP 8.5's catchable property-specific
error before storage mutation or `__unset`; ordinary properties and visibility
precedence are preserved. The exact PHP 8.5.6 delta is +1/-0, bringing the
cumulative gain from the initial baseline to +405/-0. All five feature
configurations, all-target, unsafe, Composer S0, Symfony S1 and warmed-kernel
S2 gates pass. No ordinary property performance gate applies because the new
metadata check is confined to the cold `UnsetObj` handler.

The `4ffc25a` hidden-property-hook-callback checkpoint reaches 2,221 passes
with 2,987 failures, 110 skips, one XFAIL, 280 unsupported cases, zero timeouts
and zero crashes. Direct object callbacks can no longer invoke internal
`$property::get` or `$property::set` implementations; undefined-method
diagnostics and public `__call` fallback match PHP 8.5. The exact PHP 8.5.6
delta is +1/-0, bringing the cumulative gain from the initial baseline to
+406/-0. All five feature configurations, all-target, unsafe, Composer S0,
Symfony S1 and warmed-kernel S2 gates pass. The check remains in dynamic
callback resolution, leaving ordinary cached method dispatch unchanged.

The `attribute-trace-origin` checkpoint reaches 2,864 passes with 2,384
failures, 110 skips, one XFAIL, 240 unsupported cases, zero timeouts and zero
crashes. Attribute constructors now retain their logical
`ReflectionAttribute::newInstance()` caller and attribute use-site while their
physical detached frame still exits the baseline executor safely. Strict
argument failures snapshot the pending constructor before execution, and
internal Reflection diagnostics retain canonical method spelling. The exact
PHP 8.5.6 delta is +8/-0. All five feature configurations, all-target, unsafe,
Composer S0, four Symfony S1 gates, warmed-kernel S2 and cold-build S3 pass.
The affected grouped and retained callback controls move by +0.83% and +0.19%
across 1,003 alternating AMD64 release pairs, below their five-percent ceiling.

The `startup-error-reporting` checkpoint reaches 2,878 passes with 2,402
failures, 110 skips, one XFAIL, 208 unsupported cases, zero timeouts and zero
crashes. Repeated CLI `-d error_reporting=...` definitions now initialize the
request-local diagnostic mask using PHP 8.5's INI integer grammar, and the PHPT
runner admits all 32 affected cases. Fourteen become exact passes and 18 expose
pre-existing independent failures instead of remaining hidden as unsupported;
the exact pass-set delta is +14/-0. The larger attempted denominator moves the
headline rate from 54.573% to 54.508%. The implementation remains in cold CLI,
startup and explicit INI parsing paths, so no execution-path performance gate
applies. All five feature configurations, all-target, unsafe, Composer S0,
four Symfony S1 gates, warmed-kernel S2 and cold-build S3 pass.

The `exception-ini-presentation` checkpoint reaches 2,883 passes with 2,400
failures, 110 skips, one XFAIL, 205 unsupported cases, zero timeouts and zero
crashes. Startup `zend.exception_ignore_args` and
`zend.exception_string_param_max_len` now govern stored Throwable traces, live
backtrace rendering and unmatched `match` diagnostics through one escaping and
byte-limit contract. Three unsupported cases and two existing output failures
become exact passes for a +5/-0 pass-set delta. The change is confined to cold
startup and diagnostic formatting paths, so no execution-path performance gate
applies. All five feature configurations, all-target, unsafe, Composer S0,
four Symfony S1 gates, warmed-kernel S2 and cold-build S3 pass.

The `precision-ini` checkpoint reaches 2,902 passes with 2,395 failures, 114
skips, one XFAIL, 187 unsupported cases, zero timeouts and zero crashes.
Request-local startup and mutable `precision` govern significant-digit float
conversion across the VM and string-oriented library paths, while `var_dump()`
uses PHP's independent round-trip representation and excessive valid settings
cannot drive unbounded allocation. Eighteen previously unsupported cases are
now attempted, and corrected default formatting converts eleven additional
failures into passes for an exact +19/-0 pass-set delta. All five feature
configurations, all-target, unsafe, Composer S0, four Symfony S1 gates,
warmed-kernel S2 and cold-build S3 pass. Twenty alternating AMD64 release pairs
put common exact float-to-string conversion at a 1.012 median and 1.044 p90
candidate/control ratio with identical checksums, below the five-percent
ceiling.

The `deprecated-callable` checkpoint reaches 2,923 passes with 2,374 failures,
114 skips, one XFAIL, 187 unsupported cases, zero timeouts and zero crashes.
PHP 8.5's built-in `Deprecated` metadata, readonly constructor contract,
declaration validation and attempted-call diagnostics now cover ordinary and
magic functions, methods, closures, constructors and destructors. Runtime
attribute arguments, strict coercion, handlers, suppression and source origins
reuse the canonical attribute/diagnostic machinery. Twenty-one failures become
exact passes with no lost pass or other status/stage movement; constants,
trait-use diagnostics, exception-handler lifecycle, adjacent Reflection and
Random extension dependencies remain outside this checkpoint. All five feature
configurations, all-target, unsafe, Composer S0, four Symfony S1 gates,
warmed-kernel S2 and cold-build S3 pass. Twenty-one alternating AMD64 release
pairs keep the ordinary five-million-call control at +2.14% by independent
medians and +1.55% by the paired-ratio median, with identical checksums and both
below the five-percent ceiling.

The `deprecated-symbols` checkpoint reaches 2,955 passes with 2,342 failures,
114 skips, one XFAIL, 187 unsupported cases, zero timeouts and zero crashes.
PHP 8.5 Deprecated diagnostics now cover global and class constants, enum cases
and direct trait composition, including dependency-expression ordering,
deferred values, throwing handlers, recursive messages, nested traits and
`insteadof` exclusions. The exact full-corpus delta is +32/-0 and the focused
Deprecated symbol cluster is 21/21; the complete Deprecated directory is now
43/47. Exception-handler entry, callable-to-Closure metadata propagation and
the Random enum type-validation dependency remain explicit gaps. All five
feature configurations, all-target, unsafe, Composer S0, four Symfony S1 gates
and warmed-kernel S2 pass. Twenty alternating disabled-JIT/quick-loop AMD64
release pairs put ordinary global-constant reads at +1.680% and class-constant
reads at +1.852%, below their five-percent ceiling with identical checksums.

The `deprecated-callable-flow` checkpoint reaches 2,973 passes with 2,324
failures, 114 skips, one XFAIL, 187 unsupported cases, zero timeouts and zero
crashes. Uncaught exception handlers now use PHP 8.5's engine-dispatched
lifecycle, including execution before main-scope destructors, temporary
unregistration, synthetic diagnostic/trace origins and replacement-exception
propagation. Handler setters reject invalid callbacks before mutating their
stacks, while `ReflectionMethod` over callable-derived closures exposes source
attributes and deprecation metadata without losing its public `Closure`
identity. The exact full-corpus delta is +18/-0, the complete Deprecated
directory is 46/47 and the standard exception-handler directory is 10/10; the
remaining Deprecated failure depends on Random enum argument validation. All
five feature configurations, all-target, unsafe, Composer S0, four Symfony S1
gates and warmed-kernel S2 pass. Thirty-one balanced ABBA/BAAB AMD64 release
pairs put an ordinary file request at +1.837% by independent medians and
+1.863% by the paired-ratio median, with paired p10/p90 of -1.174%/+4.257% and
identical output, below the five-percent ceiling.

The `random-interval-boundary` checkpoint reaches 2,975 passes with 2,322
failures, 114 skips, one XFAIL, 187 unsupported cases, zero timeouts and zero
crashes. PHP 8.5's internal `Random\IntervalBoundary` unit enum supplies its
four canonical cases, stable identity, readonly name, ordered `cases()` and
Reflection contract; enum inheritance is rejected and traces render enum
arguments as `Class::Case`. The exact full-corpus delta is +2/-0, comprising
the final Deprecated type-validation case and the adjacent enum trace case, so
the complete Deprecated directory is 47/47. `NoDiscard` and
`Randomizer::getFloat()` remain separate gaps. All five feature configurations,
all-target, unsafe, Composer S0, four Symfony S1 gates and warmed-kernel S2
pass. A 63-pair balanced AMD64 release confirmation over 150 ordinary file
requests per executable and pair places the candidate at +0.588% by independent
medians and +0.898% by paired-ratio median, with order medians below +1% and
identical output. No outliers were removed; paired p10/p90 is
-3.921%/+6.646%, while the independent p90 delta is +0.904%, so the median
five-percent gate passes with the noisier paired upper tail disclosed.

The `no-discard-attribute` checkpoint reaches 3,002 passes with 2,295
failures, 114 skips, one XFAIL, 187 unsupported cases, zero timeouts and zero
crashes. PHP 8.5's final internal `NoDiscard` attribute now provides its
readonly nullable-message ABI, declaration validators and warning semantics
for direct, static, magic, closure, trait and detached `call_user_func*()`
calls. Assigned results and explicit `(void)` discards remain silent; all six
pinned void-cast cases pass. The exact full-corpus delta is +27/-0: 19
NoDiscard-directory cases, two delayed-validation interactions and six void-
cast cases. The NoDiscard directory is 20/25; `DateTimeImmutable`, two
`zend_test` execute-hook cases and two skipped native-extension cases remain
outside this checkpoint, while two broader delayed-validation failures need
`ReflectionProperty::getHook()`. All five feature configurations, all-target,
unsafe, Composer S0, four Symfony S1 gates, warmed-kernel S2 and cold-build S3
pass. Sixty-three balanced AMD64 release pairs put the empty-output file-
request median at -0.457% independently and -0.422% paired, while the ordinary
call control is -0.740% independently and -0.337% paired; both retain exact
outputs/checksums and remain below the five-percent ceiling without removing
outliers.

The `reflection-property-hooks` checkpoint reaches 3,003 passes with 2,294
failures, 114 skips, one XFAIL, 187 unsupported cases, zero timeouts and zero
crashes. PHP 8.5's internal backed `PropertyHookType` enum and
`ReflectionProperty::getHook()`, `getHooks()` and `hasHook()` expose canonical
hook metadata without inventing accessors for ordinary properties. Reflected
hooks retain attributes and implicit signatures, and ReflectionMethod string
rendering closes the delayed `NoDiscard` validator case. The exact full-corpus
delta is +1/-0; four inspected runtime failures advance to later output
comparisons and no pass is lost. Exact internal-enum object handles and broader
Reflection output remain separate work; `__PROPERTY__` is closed by the
following checkpoint. All five feature configurations, all-target, unsafe,
Composer S0, four Symfony S1 gates,
warmed-kernel S2 and cold-build S3 pass. A 63-pair empty-request confirmation
places the candidate at +0.100% independently and +0.266% paired; a 31-pair
ordinary-call control is +0.859% independently and +0.665% paired. Both retain
exact observable results and remain below the five-percent median ceiling;
startup-cost recovery is deferred to the combined performance pass requested
after the compatibility push.

The `property-magic-constant` checkpoint reaches 3,007 passes with 2,290
failures, 114 skips, one XFAIL, 187 unsupported cases, zero timeouts and zero
crashes. PHP 8.5's `__PROPERTY__` now preserves the immediate property scope in
defaults, property and hook attributes, hook bodies and hook-parameter
attributes, including interface and trait declarations; ordinary and nested
callable scopes correctly receive the empty string. All four upstream cases
containing this magic constant move to exact passes, for a full-corpus delta of
+4/-0 with no category regression. All five feature configurations,
all-target, unsafe, Composer S0, four Symfony S1 gates, warmed-kernel S2 and
cold-build S3 pass. A 63-pair empty-request confirmation places the candidate
at -0.386% independently and +0.063% paired; a 31-pair ordinary-call control is
-1.042% independently and -1.013% paired. Both retain exact observable results
and remain below the five-percent median ceiling.

The `property-hook-declaration-invariants` checkpoint reaches 3,017 passes
with 2,280 failures, 114 skips, one XFAIL, 187 unsupported cases, zero
timeouts and zero crashes. Readonly hooks, defaults on virtual hooked
properties and hooked class/trait or trait/trait composition conflicts now
produce PHP's declaration diagnostics. Hook declarations that override a
visible backed parent property correctly retain storage, default presence,
implicit accessors, enumeration behavior and non-virtual Reflection metadata.
The `property_hooks` directory rises from 151 to 161 exact passes; the exact
full-corpus delta is +10/-0 with no category regression. All five feature
configurations, all-target, unsafe, Composer S0, four Symfony S1 gates,
warmed-kernel S2 and cold-build S3 pass. A 63-pair empty-request confirmation
places the candidate at -0.688% independently and -0.407% paired; a 31-pair
ordinary-call control is -1.392% independently and -1.148% paired. Both retain
exact observable results and remain below the five-percent median ceiling.

The `property-hook-setter-variance` checkpoint reaches 3,024 passes with 2,273
failures, 114 skips, one XFAIL, 187 unsupported cases, zero timeouts and zero
crashes. Explicit set-hook parameters now preserve typedness and are checked
contravariantly against their property type; relation-dependent class names
use delayed linking or runtime-include autoload without loading exact
unresolved types. Set-hook inheritance diagnostics expose the implicit `void`
contract while synthetic plain-property setters retain their parent-call
value. The `property_hooks` directory rises from 161 to 168 exact passes; the
exact full-corpus delta is +7/-0 with no category regression. All five feature
configurations, all-target, unsafe, Composer S0, four Symfony S1 gates,
warmed-kernel S2 and cold-build S3 pass. A 63-pair empty-request confirmation
places the candidate at +0.850% independently and +1.239% paired; a 31-pair
ordinary-call control is +0.031% independently and -0.151% paired. Both retain
exact observable results and remain below the five-percent median ceiling;
the fixed startup cost remains visible for the planned controlled refactoring.

The `property-hook-inheritance-variance` checkpoint reaches 3,027 passes with
2,270 failures, 114 skips, one XFAIL, 187 unsupported cases, zero timeouts and
zero crashes. Virtual get-only parent properties accept covariant child types,
and virtual set-only parents accept contravariant types, including children
that add the opposite hook. Backed and ordinary storage remain invariant. The
same directional rule governs delayed linking for unresolved class-like types.
The `property_hooks` directory rises from 168 to 171 exact passes; the complete
PHP 8.5.6 corpus delta is +3/-0 with no category regression. All five feature
configurations, all-target, unsafe, Composer S0, four Symfony S1 gates,
warmed-kernel S2 and cold-build S3 pass. A 63-pair empty-request confirmation
places the candidate at -1.457% independently and -1.333% paired; a 31-pair
ordinary-call control is -0.128% independently and -0.245% paired. Both retain
exact output and remain below the five-percent median ceiling without making
an optimization claim.

The `property-prototype-inheritance` checkpoint reaches 3,037 passes with
2,260 failures, 114 skips, one XFAIL, 187 unsupported cases, zero timeouts and
zero crashes. The oldest non-private property prototype now governs protected
scope across sibling implementations, and its actual set capability governs
whether a child may introduce asymmetric write visibility. Plain child storage
inherits concrete parent hooks that it does not replace. The `property_hooks`
directory rises from 171 to 179 exact passes; the ordinary and asymmetric
GH-19044 cases plus the plain-property hook inheritance case make the exact
full-corpus delta +10/-0 with no other status or category movement. All five
feature configurations, all-target, unsafe, Composer S0, four Symfony S1 gates,
warmed-kernel S2 and cold-build S3 pass. CPU-pinned release A/B measurements
put empty-request startup at +0.837%, ordinary calls at -0.645%, instance-
property reads at +0.351% and writes at -0.297% by independent medians, with
exact observable results and every lane below the five-percent ceiling.

The `property-hook-reference-backing` checkpoint reaches 3,040 passes with
2,257 failures, 114 skips, one XFAIL, 187 unsupported cases, zero timeouts and
zero crashes. Reference-returning property and magic getters now expose aliases
that indirect dimension writes mutate in place without a synthetic setter,
while access-local provenance preserves ordinary by-reference property
writebacks and asymmetric visibility. Backing-property detection includes the
reference-bind opcode, and direct reference assignment invokes the getter
before reporting PHP's overloaded-object error. The `property_hooks` directory
rises from 179 to 181 exact passes; two property-hook cases and one general
coalesce case make the exact PHP 8.5.6 corpus delta +3/-0 with no other status
movement. All five feature configurations, all-target, unsafe, Composer S0,
four Symfony S1 gates, warmed-kernel S2 and cold-build S3 pass. CPU-pinned
32-pair release A/B medians put empty-request startup at -1.229%, indexed array
append at -0.322%, irregular integer-dimension assignment at +0.425%, ordinary
calls at -0.040% and append-by-reference at +0.113% independently; all paired
and independent medians remain below the five-percent ceiling, with outliers
retained.

The `override-attribute` checkpoint reaches 3,070 passes with 2,227 failures,
114 skips, one XFAIL, 187 unsupported cases, zero timeouts and zero crashes.
PHP 8.5's final internal `Override` attribute now validates methods, properties,
property hooks and promoted properties against effective parent, interface and
abstract-trait contracts. It preserves delayed target validation, applies
trait precedence and aliases, recognizes implicit accessors on backed
properties, and exposes the expected Reflection metadata. All 50 dedicated
override cases and all 59 corpus cases using the marker pass; the exact full-
corpus delta is +30/-0 with no other status or failure-stage movement. All five
feature configurations, all-target, unsafe, Composer S0, four Symfony S1 gates,
warmed-kernel S2 and cold-build S3 pass. CPU-pinned release A/B medians put
empty-request startup at -0.671% independently and -0.681% paired, while the
ordinary-call control is +0.259% independently and +0.277% paired. Exact output
is retained and every median remains below the five-percent ceiling.

The `weak-objects-core` checkpoint reaches 3,213 passes with 2,084 failures,
114 skips, one XFAIL, 187 unsupported cases, zero timeouts and zero crashes.
Final internal `WeakReference`, `WeakMap` and `InternalIterator` classes cover
ordinary-object and Closure targets/keys, cached wrappers, immediate final-owner
notification, key-before-value destructor order, insertion-ordered iteration,
by-reference values, aliases, clone separation, dumps and the exercised
construction, dynamic-property, append and serialization restrictions. The
35-case `Zend/tests/weakrefs` cluster rises from 0 to 18 exact passes; six
adjacent weak-object users and three corrected `ArrayAccess::empty()` cases
also pass, for an exact full-corpus delta of +27/-0 with all 3,186 prior passes
retained. Sixteen failures advance from missing-class or silent boundaries to
later output or diagnostic comparison. Eleven focused cases plus the final
section of `weakmap_weakness.phpt` still require general cyclic collection;
broader reference/header destructor behavior, Reflection construction, an
internal destructor stack frame and source-located final-class diagnostics are
separate boundaries. All five feature configurations, all-target, unsafe,
Composer S0, four Symfony S1 gates, warmed-kernel S2 and cold-build S3 pass.
CPU-pinned 32-pair release A/B medians put 150 empty requests at +0.885%
independently and +0.869% paired, and a million ordinary declared-object
lifecycles at +0.651% independently and +1.005% paired; exact outputs remain
and all regression medians are below the five-percent ceiling. A new 2,000-key
WeakMap insert/read plus 1,000-removal lane is approximately 19.9x PHP 8.5.6's
median and is retained as explicit indexed-sidecar optimization debt, not a
regression or parity claim.

The `cycle-collector-core` checkpoint reaches 3,232 passes with 2,065 failures,
114 skips, one XFAIL, 187 unsupported cases, zero timeouts and zero crashes.
Explicit `gc_collect_cycles()` now collects unreachable graphs composed of
ordinary objects, arrays, Closures and owned references, applies WeakMap
ephemeron reachability, preserves weak identities through destructor execution,
rechecks resurrection before releasing graph edges and returns PHP 8.5's
exercised strongly connected component counts. It also remains explicit while
automatic GC is disabled. Fifteen focused GC/weak-reference cases and four
adjacent Fiber, generator and magic-method lifecycle cases become exact, for a
full-corpus delta of +19/-0 from exact base `a022a0c5`; all 3,213 prior passes
remain. Four cases advance to known later boundaries without an unexplained
category move. Five original E2E tests, all five feature configurations,
all-target, formatting, unsafe policy, Composer S0, four Symfony S1 gates,
warmed-kernel S2 and cold-build S3 pass. CPU-pinned 32-pair release A/B medians
put 150 empty requests at -0.719% independently and -0.704% paired, a million
declared-object lifecycles at +0.214% independently and -0.945% paired, and the
balanced ordered-workload ratio at -0.133%; exact outputs remain and all
relevant regression medians are below the five-percent ceiling. A new explicit
collector lane is approximately 13.9x PHP 8.5.6's median and is retained as an
optimization baseline, not a regression or parity claim. Automatic threshold
collection, complete `gc_status()` telemetry, generator/Fiber/resource cycle
breadth, WeakMap indexing, exact root-color ordering after transient weak
upgrades and compound-Echo temporary lifetime remain separate checkpoints.

The `enum-serialization-core` checkpoint reaches 3,239 passes with 2,058
failures, 114 skips, one XFAIL, 187 unsupported cases, zero timeouts and zero
crashes. Registered enum singletons now serialize with PHP 8.5's dedicated
`E:<length>:"Class:Case";` form, preserve repeated references, autoload during
unserialization and remain admissible when ordinary classes are disabled.
Case names stay exact while class lookup stays case-insensitive. Missing-colon,
missing-class, non-enum, ordinary-constant, undefined-case and malformed-length
failures use the exercised warning text, error-handler path and byte offsets.
The 152-case enum slice rises from 71 to 78 exact passes and the full-corpus
delta from exact base `0f84ee36` is +7/-0, with all 3,232 prior passes retained
and no other status or failure-stage movement. Four original E2E tests, all five
feature configurations, all-target, formatting, unsafe policy, Composer S0,
four Symfony S1 gates, warmed-kernel S2 and cold-build S3 pass. The change is
confined to explicitly invoked serialization/unserialization and diagnostics,
with no hot-path or layout change, so no performance regression lane applies.
Exact enum `debug_zval_dump()` refcounts, SplObjectStorage's custom wire format,
generic trailing-data handling, Reflection and remaining enum declaration or
readonly contracts remain separate checkpoints.

The `enum-declaration-shape` checkpoint reaches 3,250 passes with 2,047
failures, 114 skips, one XFAIL, 187 unsupported cases, zero timeouts and zero
crashes. Enum backing declarations now carry compound type syntax into compiler
validation, accept only one `int` or `string` type and use PHP's canonical type
spelling. Backed cases require a value, unit-enum cases reject one, and instance,
static, typed, untyped or hooked properties reach the declaration-stage enum
diagnostic instead of an earlier parser error. The 152-case enum slice rises
from 78 to 89 exact passes and the full-corpus delta from exact base `8bbd98b9`
is +11/-0, with all 3,239 prior passes retained and no other status or
failure-stage movement. Four original E2E tests, all five feature
configurations, all-target, formatting, unsafe policy, Composer S0, four
Symfony S1 gates, warmed-kernel S2 and cold-build S3 pass. The change stops at
parser AST and compiler diagnostics, so no runtime performance lane applies.
Lazy duplicate/mismatched backing validation, runtime enum-property mutation
and reference diagnostics, Reflection and SplObjectStorage remain separate
checkpoints.

The `enum-interface-contracts` checkpoint reaches 3,260 passes with 2,037
failures, 114 skips, one XFAIL, 187 unsupported cases, zero timeouts and zero
crashes. Concrete declarations now validate PHP 8.5's `UnitEnum`, `BackedEnum`,
`Throwable` and `Serializable` restrictions through direct, inherited and
separately included interface graphs while preserving legal user-interface
diamonds. Explicit enum interfaces and non-backed misuse report the canonical
declaration kind, source and line. Concrete legacy `Serializable` classes and
forbidden enums emit the required deprecation unless an effective modern magic
pair suppresses it for a class. The enum slice rises from 89 to 98 exact passes;
`serialize/serializable_deprecation.phpt` is the tenth gain. The exact
full-corpus delta from base `7aea4a1f` is +10/-0, with every prior pass retained
and no other status or failure-category movement. Four remaining failures gain
the required preceding deprecation at the same output stage while retaining
their independent later gaps. Four original E2E tests, all five feature
configurations, all-target, formatting, unsafe policy, Composer S0, four
Symfony S1 gates, warmed-kernel S2 and cold-build S3 pass. Compiler validation
and cold class linking change no executor or runtime data layout, so no runtime
performance lane applies. Lazy duplicate/mismatched backing values, runtime
enum-property mutation/reference diagnostics, Reflection and SplObjectStorage
remain separate checkpoints.

The `fiber-bailout-shutdown` checkpoint reaches 3,186 passes with 2,111
failures, 114 skips, one XFAIL, 187 unsupported cases, zero timeouts and zero
crashes. A lazy request-local FIFO retains validated shutdown callbacks,
arguments, lexical scope and live receivers; it accepts callbacks appended
during shutdown and runs after success, `exit` or a displayed fatal. Handled
shutdown exceptions continue the queue. VM bailout is now a distinct Fiber
terminal state, so shutdown status and `getReturn()` match PHP 8.5 after direct,
nested, multiple-Fiber, E_USER_ERROR and fallible-allocation fatals. The Fiber
directory reaches 65/110 and ten adjacent shutdown/diagnostic cases also become
exact, for a full-corpus delta of +16/-0. Four failures advance from output to
runtime: `bug41026.phpt` still needs relative `self` callable diagnostics,
`bug51827.phpt` and `bug71221.phpt` need the exact inactive-file synthetic
trace, and `bug78396.phpt` needs filesystem flags. All five feature
configurations, all-target, unsafe, Composer S0, four Symfony S1 gates,
warmed-kernel S2 and cold-build S3 pass. CPU-pinned 32-pair release A/B medians
put 150 empty requests at -0.665% independently and -0.800% paired, and 20,000
Fiber switches at +0.815% independently and +0.470% paired. Exact outputs are
retained; paired p10/p90 ranges are -1.333%/-0.062% and -1.441%/+1.966%, and
both medians remain below the five-percent ceiling. General cycle collection,
destructor Fibers, generator/internal crossings, signals, ticks and broader
OOM emulation remain separate checkpoints.

The `fiber-force-close` checkpoint reaches 3,170 passes with 2,127 failures,
114 skips, one XFAIL, 187 unsupported cases, zero timeouts and zero crashes.
Last external Fiber release now delivers an uncatchable internal exit through
nested `finally` regions, rejects re-suspension with the exact `FiberError`,
runs stack-local object destructors, preserves multiple replacement exceptions
and request-shutdown traces, and accounts for Fiber self references retained by
detached frames. Eight `gh9735` cases, `invocable-class.phpt`, five direct and
shutdown force-close diagnostics, and four unfinished-Fiber `finally` cases
become exact; the Fiber directory reaches 59/110 and the full corpus delta is
+18/-0 with no other movement. All five feature configurations, all-target,
unsafe, Composer S0, four Symfony S1 gates, warmed-kernel S2 and cold-build S3
pass. CPU-pinned 32-pair release A/B medians put 20,000 Fiber switches at
-0.114% independently and +0.023% paired, with paired p10/p90
-1.916%/+2.475% and exact output. General cycle collection, destructor Fibers,
generator/internal crossings, signals, ticks and bailout/OOM cleanup remain
separate checkpoints.

The `fiber-error-state` checkpoint reaches 3,152 passes with 2,145 failures,
114 skips, one XFAIL, 187 unsupported cases, zero timeouts and zero crashes.
Detached Fiber/coroutine state now owns `error_reporting` and active `@`
frames, inherits the unsuppressed caller mask on first entry and restores both
contexts across suspension. General nested `@` also intersects rather than
replaces the active reporting mask. Both Fiber silence cases and
`Zend/tests/bug34786.phpt` become exact; the Fiber directory reaches 41/110 and
the full corpus delta is +3/-0 with no other movement. All five feature
configurations, all-target, unsafe, Composer S0, four Symfony S1 gates,
warmed-kernel S2 and cold-build S3 pass. CPU-pinned 32-pair release A/B medians
put 20,000 Fiber switches at -0.934% independently and -1.267% paired, and a
five-million suppressed-call control at +0.127% and +0.030%. Exact outputs are
retained and both remain below the five-percent ceiling. Destruction, GC,
force-close and generator crossings remain separate Fiber checkpoints.

The `fiber-core-lifecycle` checkpoint reaches 3,149 passes with 2,148 failures,
114 skips, one XFAIL, 187 unsupported cases, zero timeouts and zero crashes.
Pinned alternate VM stacks implement construction, start, suspend, resume,
throw, return, current identity, status and nested logical backtraces for user
callbacks. The complete `Zend/tests/fibers` directory rises from zero to 39
exact passes and the full corpus delta is +39/-0; 29 remaining Fiber failures
advance from runtime to output without unrelated stage movement. GC cycles,
shutdown/forced close, generator/internal/magic callback roots, generator
crossings, ticks, signals and exact OOM cleanup remain named boundaries. All
five feature configurations, all-target, unsafe, Composer S0, four Symfony S1
gates, warmed-kernel S2 and cold-build S3 pass. CPU-pinned release controls put
empty-request startup at +1.001% independently and +1.222% paired, ordinary
calls at -0.649% and -0.673%, and `str_repeat()` at +1.090% and +1.079%; exact
outputs are retained and every regression median stays below the five-percent
ceiling. The newly admitted 20,000-cycle Fiber path retains PHP 8.5.6's exact
checksum and records a 0.014491-second candidate median versus PHP's 0.005082
seconds as explicit optimization debt rather than a regression claim.

The `trait-constant-composition` checkpoint reaches 3,110 passes with 2,187
failures, 114 skips, one XFAIL, 187 unsupported cases, zero timeouts and zero
crashes. Trait constants now preserve PHP 8.5 value/type/visibility/finality
identity, exact origins, Reflection values and source diagnostics. Named
trait-consuming classes and their descendants link at their runtime declaration
marker, including deferred autoload and retryable missing-trait errors. The
`Zend/tests/traits` directory rises from 92 to 110 exact passes and the full
corpus delta is +40/-0 with no lost pass; six inspected remaining failures reach
a later output or declaration-check stage. Doc comments remain an explicit
separate metadata gap. All five feature configurations, all-target, unsafe,
Composer S0, four Symfony S1 gates, warmed-kernel S2 and cold-build S3 pass.
CPU-pinned 32-pair release A/B medians put empty-request startup at -4.652%,
ordinary calls at -1.453% and a 32-consumer trait cold-link control at -1.795%
independently; paired medians are -4.658%, -1.320% and -1.791%. Exact outputs
are retained and every median remains below the five-percent ceiling.

The `trait-method-identity` checkpoint reaches 3,395 passes with 1,902
failures, 114 skips, one XFAIL, 187 unsupported cases, zero timeouts and zero
crashes. Shared trait op-arrays keep the trait as their lexical visibility
owner, while cold trace and type-diagnostic paths recover the concrete composing
class and selected original or alias name from the live call site. Instance,
static and inherited identities therefore match PHP 8.5 without per-call side
state or per-composition bytecode copies. Three backtrace cases and two trait
diagnostic cases become exact for a full-corpus delta of +5/-0 with no other
movement. `bug69180-backtrace.phpt` retains a separate missing magic-property
entry frame. All five feature configurations, all-target, formatting, unsafe,
Composer S0, four Symfony S1 gates and exact PHP 8.5.6 S2/S3 pass. CPU-pinned
32-pair release controls put 32-consumer/eight-method cold linking at -0.874%
independently and -0.249% paired, and a five-million-call trait method at
-0.033% and +0.009%; all medians remain below the five-percent ceiling. A
semantically equivalent deep-clone prototype was rejected at +35.774% paired
cold-link regression.

The `magic-property-entry-frame` checkpoint reaches 3,396 passes with 1,901
failures, 114 skips, one XFAIL, 187 unsupported cases, zero timeouts and zero
crashes. Engine-dispatched `__get`, `__set`, `__isset` and `__unset` activations
keep their detached return boundary but publish the active property instruction
as a logical trace caller. Live traces therefore retain the missing outer magic
method and exact source site without widening `ExecuteData` or changing stored
Throwable reconnection. `IssetObj`, `UnsetObj` and silent intermediate reads
retain their line in the existing sparse compiler table. The original
four-operation/inheritance regression and `bug69180-backtrace.phpt` pass; the
full-corpus delta is +1/-0 with no other movement, and two final manifests are
byte-identical. All five feature
configurations, all-target, formatting, unsafe, Composer S0, four Symfony S1
gates and exact PHP 8.5.6 S2/S3 pass. A CPU-pinned 32-pair control of one
million repeated missing-property reads measures -3.865% independently and
-3.876% paired, with paired p10/p90 -10.050%/+5.239%; both medians remain below
the five-percent ceiling and the declared-property path is untouched.

The `reserved-class-names` checkpoint reaches 3,423 passes with 1,874
failures, 114 skips, one XFAIL, 187 unsupported cases, zero timeouts and zero
crashes. One shared terminal-segment classifier enforces PHP 8.5's reserved
class-like names across declarations, class imports and runtime
`class_alias()` strings while preserving raw diagnostic spelling, declaration
kind, the complete `use` statement line and original/internal lookup priority.
Class, trait, interface and enum `_` declarations plus an unqualified `_`
runtime alias emit the 8.4 deprecation; qualified runtime aliases and class
imports named `_` remain allowed. Two classifier unit tests, six original E2E
tests and a 24-case focused slice cover positive, negative, boundary and
interaction paths. The exact full-corpus delta from `91133161` is +27/-0 with
all prior passes retained; two final manifests are byte-identical.
`restore_error_reporting.phpt` reaches the
correct reserved-name failure but remains blocked by independent eval
compile-error wrapping. All five feature configurations, all-target,
formatting, unsafe policy, Composer S0, four Symfony S1 gates and exact PHP
8.5.6 S2/S3 pass. A CPU-pinned 32-pair control over 10,000 independently
compiled class declarations measures +0.187% independently and +0.119%
paired, with paired p10/p90 -1.549%/+1.837%; both medians remain below the
five-percent ceiling and no executor path or runtime layout changed.

The `eval-compile-fatal` checkpoint reaches 3,425 passes with 1,872 failures,
114 skips, one XFAIL, 187 unsupported cases, zero timeouts and zero crashes.
Valid-syntax compiler failures from eval and included source units now bypass
Throwable catches as PHP compile fatals without the former duplicated
`Parse error: Compile error in ...` wrapper; syntax failures remain catchable
`ParseError` objects. A dedicated eval flag applies `@` reporting through the
synchronous source unit, restores it after success or a caught parse error and
keeps the fatal-only mask through shutdown bailout. Three original E2E tests,
one CLI lifecycle test and the exact focused slice cover normal, catchable,
fatal, include and shutdown boundaries. `bug55007.phpt` and
`restore_error_reporting.phpt` become exact for a full-corpus delta of +2/-0
from `0de2e3c4`; every prior pass is retained and two final manifests are
byte-for-byte identical. `gh13931.phpt`, `gh8841.phpt` and `gh7771_3.phpt`
advance to their known later diagnostic/trace boundaries. All five feature
configurations, all-target, formatting, unsafe, Composer S0, four Symfony S1
gates and exact PHP 8.5.6 S2/S3 pass. A CPU-pinned 32-pair control of 20,000
successful eval
cycles measures -0.320% independently and -0.726% paired, with paired p10/p90
-2.860%/+2.551%; all samples retain exact output and no hot layout changes.

The `internal-class-alias` checkpoint reaches 3,429 passes with 1,868
failures, 114 skips, one XFAIL, 187 unsupported cases, zero timeouts and zero
crashes. PHP 8.5 internal classes and interfaces now enter the existing shared
alias registry after original lookup and the general reserved/`_` validation,
preserving one class identity, canonical object names, class-like kind and
internal metadata. `ReflectionClass` construction resolves aliases back to the
definition's canonical public name instead of exposing the requested alias.
Original E2E tests cover internal class and interface identity, Reflection,
missing-original priority, reserved fatals and qualified/unqualified `_`.
The complete 27-case alias slice rises from 17 to 21 passes, and the full-
corpus delta from `459f8481` is +4/-0: `class_alias_006.phpt`, both `gh16665`
cases and `gh15976/alias-names.phpt` become exact, with every prior pass and
all other categories unchanged. Two final manifests are byte-identical. All
five feature configurations, all-target, formatting, unsafe policy, Composer
S0, four Symfony S1 gates and exact PHP 8.5.6 S2/S3 pass. No performance or
layout gate applies because the change stays in explicitly invoked cold
builtins, reuses the established alias registry and does not change executor,
value, object or ordinary lookup paths. Five unrelated output-different
`class_alias` cases and one `memory_limit` capability case remain explicit
follow-up work.

The `class-alias-collision-origin` checkpoint reaches 3,433 passes with 1,864
failures, 114 skips, one XFAIL, 187 unsupported cases, zero timeouts and zero
crashes. Alias name conflicts now use PHP 8.5's `Cannot redeclare` warning,
preserve the original class/interface/trait/enum kind and caller alias spelling,
and append the aliased user symbol's declaration file and line. Internal
sources omit that parenthetical because they have no userland origin. The
existing `ClassDef` metadata is read only after the established registry
returns `NameConflict`; successful publication and ordinary lookup are
unchanged. Original E2E coverage exercises user class/interface origins,
internal omission, handler arguments and false returns. The complete 27-case
alias slice rises from 21 to 25 passes, and the full-corpus delta from
`6592be1b` is +4/-0: `class_alias_002.phpt`, `class_alias_004.phpt`,
`class_alias_010.phpt` and `class_alias_019.phpt` become exact with every prior
pass and all other categories unchanged. Two final manifests are
byte-for-byte identical. All five feature configurations, all-target,
formatting, unsafe
policy, Composer S0, four Symfony S1 gates and exact PHP 8.5.6 S2/S3 pass. No
performance or layout gate applies because the successful alias path is
unchanged and the new lookup is confined to the cold conflict diagnostic.
`class_alias_017.phpt` and the `memory_limit` capability case remain explicit
follow-up work.

The `get-class-noarg-deprecation` checkpoint reaches 3,436 passes with 1,861
failures, 114 skips, one XFAIL, 187 unsupported cases, zero timeouts and zero
crashes. Within class scope, no-argument `get_class()` now dispatches PHP 8.5's
deprecation before returning the lexical declaring class; a throwing handler
interrupts that return. Outside class scope the existing direct `Error`
remains undecorated, and explicit-object calls are unchanged. Original E2E
coverage verifies handler interruption, restored diagnostics, static/instance
lexical scope, physical source lines, the outside error and runtime aliases.
The complete alias directory plus the two other no-argument peers forms a
28-case focused gate with 27 passes and one explicit `memory_limit`
unsupported case, so every supported upstream `class_alias` case is exact.
The full-corpus delta from `099be260` is +3/-0:
`class_alias_017.phpt`, `generator_static_method.phpt` and
`get_class_basic.phpt` become exact with every prior pass and all other
categories unchanged; two final manifests are byte-for-byte identical. All
five feature configurations, all-target, formatting, unsafe policy, Composer
S0, four Symfony S1 gates and exact PHP 8.5.6 S2/S3 pass. No performance or
layout gate applies because the explicit-object path is unchanged and all new
work is confined to the deprecated no-argument branch. No-argument
`get_parent_class()` and the missing CLI capability remain separate work.

The `get-parent-class-noarg-deprecation` checkpoint reaches 3,438 passes with
1,859 failures, 114 skips, one XFAIL, 187 unsupported cases, zero timeouts and
zero crashes. Every no-argument `get_parent_class()` call now dispatches PHP
8.5's deprecation before resolving lexical class scope or returning false from
global scope, and a throwing handler interrupts the call before either result.
Parentless and inherited lexical methods retain their established values,
while the explicit object-or-class argument path is unchanged. An original E2E
regression covers global and class handlers, restored reporting, physical
source lines, parentless and inherited classes, and an explicit late-static
argument. The complete four-case selected cluster makes both no-argument Zend
tests exact; its trait output and `bug21961.phpt` compile failures remain at
independent boundaries. The full-corpus delta from `a82bfeb5` is +2/-0:
`get_parent_class_001.phpt` and `get_parent_class_basic.phpt` become exact with
every prior pass and all other categories unchanged. Two final manifests are
byte-for-byte identical. All five feature configurations, all-target,
formatting, unsafe policy, Composer S0, four Symfony S1 gates and exact PHP
8.5.6 S2/S3 pass. No performance or layout gate applies because the explicit
argument path is unchanged and the new work is confined to the deprecated
no-argument branch.

The `trait-magic-class-scope` checkpoint reaches 3,441 passes with 1,856
failures, 114 skips, one XFAIL, 187 unsupported cases, zero timeouts and zero
crashes. Trait method and closure `__CLASS__` reads now bind to the exact final
composition selected by dispatch, while inherited methods keep the parent's
composition, explicit trait reuse creates a new one and nested traits follow
their final consumer. This lexical value remains separate from late-static
scope; aliases, private calls and reentrant parent calls preserve the selected
body, while `__TRAIT__`, `__FUNCTION__` and `__METHOD__` retain trait identity.
An original E2E regression covers those boundaries plus alternating caches,
static calls and escaping closures. The exact full-corpus delta from
`462a7bd0` is +3/-0: `bug65419.phpt`, `bug76773.phpt` and
`gh14009_005.phpt` become exact with every prior pass and all other categories
unchanged. Two final manifests and summaries are byte-for-byte identical. All
five feature configurations, all-target, formatting, unsafe policy, Composer
S0, four Symfony S1 gates and PHP 8.5 S2/S3 pass. A CPU-pinned 32-pair release
control of five million affected calls measures +4.553% independently and
+4.646% paired, below the +5% gate; the 1,000-composition cold-link control
measures -0.645% independently and -0.298% paired. Trait property-default magic
constants, deprecated parent callables and missing standard-library functions
remain separate work.

The `trait-property-magic-class` checkpoint reaches 3,443 passes with 1,854
failures, 114 skips, one XFAIL, 187 unsupported cases, zero timeouts and zero
crashes. Ordinary class property `__CLASS__` defaults use their declaring
class, while trait property defaults containing `__CLASS__` or `self::class`
rebind at every composition boundary. Nested traits, inheritance, explicit
reuse, first-compatible collision order and unrelated consumers follow PHP
8.5. Inherited static storage keeps its parent value while copied Reflection
default metadata is child-relative. The compiler retains rebinding expressions
in an existing cold sidecar without enlarging common class/property metadata.
The exact full-corpus delta from `e9ac0b43` is +2/-0:
`bug55214.phpt` and `bug76539.phpt` become exact with every prior pass and all
other categories unchanged. Two final manifests and summaries are
byte-for-byte identical. All five feature configurations, all-target,
formatting, unsafe policy, Composer S0, four Symfony S1 gates and PHP 8.5 S2/S3
pass. A CPU-pinned 32-pair release control, with five 1,000-composition
requests per observation, measures -0.911% independently, -0.508% by paired
means and +0.324% by paired median, below the +5% gate. Relative
`parent::class` defaults and other declaration magic constants remain separate
work.

Composer S0 and the four bounded Symfony S1 gates plus warmed FrameworkBundle
S2 pass on AMD64. The exact PHP 8.5 cold FrameworkBundle 7.4.16 S3 gate also
passes after adding the missing `ReflectionParameter::__toString()` contract
used while Symfony exports controller argument types. The gate covers clean,
cached, deleted, malformed and concurrent cache publication/load behavior.
None of these bounded gates establishes general PHP, extension, SAPI or
production compatibility.

The `builtin-write-context` checkpoint reaches 3,987 exact passes with 1,312
failures, 115 skips, no XFAIL, 185 unsupported cases, zero timeouts and zero
crashes. A compiler-only metadata table reproduces PHP 8.5's special global
built-in call shapes without coupling the source diagnostic to RPHP's runtime
lowering. Indexed/append writes, coalescing and compound mutation, direct
references and references to indexed special results, `unset()` and
by-reference iteration reject the special
temporary before any source code executes; namespace shadows and ordinary
call shapes remain writable. The exact full-corpus delta from integration base
`ba435240` is +3/-0 with no other status or category movement, and two final
manifests and summaries are byte-identical. All five feature configurations,
all-target, formatting, unsafe policy, Composer S0, four Symfony S1 gates and
PHP 8.5.9 S2/S3 pass. A CPU-pinned 32-pair release control puts the paired
median empty-request ratio at 0.998261 and the 1,000-source-unit compile/write
ratio at 1.009096, both below the +5% gate. Temporary non-call write
diagnostics, false-to-array conversion, string append diagnostics and
clone-result spelling remain independent checkpoints.

The `temporary-write-context` checkpoint reaches 3,988 exact passes with 1,311
failures, 115 skips, no XFAIL, 185 unsupported cases, zero timeouts and zero
crashes. A parser-level root classifier preserves mutable l-values and ordinary
call-result writes while rejecting literal, operator, control-expression,
assignment, `new`, pipe and constant temporaries before source execution.
Indexed/append, compound/coalescing, increment/decrement, unset and reference
forms share the PHP 8.5 temporary diagnostic; `clone` uses the distinct Zend
built-in-result diagnostic, including by-reference foreach. The exact
full-corpus delta from `b19f32fa` is +1/-0, solely
`varSyntax/writeToTempExpr.phpt`, with no other status or category movement and
two byte-identical final manifests/summaries. All five feature configurations,
all-target, formatting, unsafe policy, Composer S0, four Symfony S1 gates and
PHP 8.5.9 S2/S3 pass. A CPU-pinned 32-pair release control puts the paired
median empty-request ratio at 0.995201 and the 1,000-source-unit compile/write
ratio at 1.001989, below the +5% gate. Dynamic by-reference argument errors,
destructuring writable-value diagnostics, nested temporary by-reference
foreach sources, false-to-array conversion and string append diagnostics
remain independent checkpoints.

The `false-array-autovivification` checkpoint reaches 3,992 exact passes with
1,307 failures, 115 skips, no XFAIL, 185 unsupported cases, zero timeouts and
zero crashes. False indexed, append, reference, nested, property,
destructuring, foreach and compound write destinations publish an empty array,
emit PHP 8.5's deprecation through the normal reentrant handler pipeline and
abandon stale nested writeback when the handler replaces the converted
location. Terminal `[]` is also admitted for compound assignment and by-value
foreach targets; `unset(false[$key])` deprecates without conversion. The exact
full-corpus delta from `67e45bcd` is +4/-0:
`concat/bug32833.phpt`, both previously failing `falsetoarray` cases and
`fe_fetch_op2_live_range.phpt` become exact with every prior pass preserved.
Four remaining failures move to documented independent ArrayAccess, max-key/
catchable assignment, recursive serialization and string-append boundaries;
no other category moves, and two final manifests/summaries are byte-identical.
All five feature configurations, all-target, formatting, unsafe, Composer S0,
four Symfony S1 gates and PHP 8.5.9 S2/S3 pass. CPU-pinned 32-pair release
controls put paired median candidate/baseline ratios at 0.996750 for 100 empty
requests, 0.983890 for three million indexed-plus-append writes and 0.961226
for one and a half million nested writes, below the +5% gate. Throwing-handler
commit timing, direct effectful terminal keys, typed boolean properties,
by-reference foreach teardown and the four later failure boundaries remain
separate checkpoints.

The `integer-literal-overflow` checkpoint reaches 4,003 exact passes with 1,296
failures, 115 skips, no XFAIL, 185 unsupported cases and zero timeouts or
crashes. Decimal, binary, hexadecimal, explicit octal and legacy octal literals
now retain AMD64 integers through `PHP_INT_MAX`, promote through a cold
PHP-compatible finite/infinite double conversion after overflow, and preserve
numeric separators plus legacy invalid-octal diagnostics. The exact full-corpus
delta from `5da46076` is +11/-0: the broad binary/decimal/octal cases, both
64-bit boundary regressions, all three 64-bit integer-literal suites and the
invalid-legacy-octal case become exact with every prior pass preserved. One
remaining case advances to its independent reentrant float-key output boundary;
an error-suppressed assignment case reaches a separate parser restriction. Two
final manifests/summaries are byte-identical. All five feature configurations,
all-feature/all-target, formatting, PHPT runner, unsafe, Composer S0, four
Symfony S1 gates and PHP 8.5.9 S2/S3 pass. CPU-pinned 32-pair release controls
put independent/paired median candidate-to-baseline changes at +0.008%/+0.028%
for 100 empty requests and +0.978%/+0.801% for 50,000 ordinary integer
expressions, below the +5% gate. Malformed explicit-prefix diagnostics, 32-bit
boundaries and the two later blockers remain separate checkpoints.

The `keyed-destructuring-expressions` checkpoint reaches 4,023 exact passes
with 1,276 failures, 115 skips, no XFAIL, 185 unsupported cases and zero
timeouts or crashes. Legacy `list()` is now a value-producing expression;
keyed patterns accept arbitrary key expressions and writable reference,
nested, append, dimension, object-property and static-property targets while
preserving PHP 8.5 source/key/fetch/destination order. Arrow capture,
referenceability and compile-time empty/mixed/style/writable-target diagnostics
are covered by original regressions. The exact full-corpus delta from
`5b4142ac` is +20/-0: twelve parse, six output and two runtime failures become
exact with every prior pass preserved. Two remaining list cases advance to
independent `ArrayObject` mutation and `data:` stream-wrapper boundaries; no
other status or category moves, and two final manifests/summaries are
byte-identical. All five feature configurations, all-feature/all-target,
formatting, PHPT runner, unsafe, Composer S0, four Symfony S1 gates and PHP
8.5.9 S2/S3 pass. CPU-pinned 32-pair release controls put independent/paired
median changes at -0.355%/-0.248% for 100 empty requests and
-1.722%/-1.857% for two million positional-plus-keyed literal destructuring
iterations, below the +5% gate. The two later SPL/stream blockers and broader
compatibility remain separate checkpoints.

The `compiler-halt-directive` checkpoint reaches 4,041 exact passes with 1,258
failures, 115 skips, no XFAIL, 185 unsupported cases and zero timeouts or
crashes. A case-insensitive outermost `__halt_compiler();` now terminates
lexing at the exact post-semicolon byte; direct, absolute and dynamic
`__COMPILER_HALT_OFFSET__` behavior follows PHP 8.5 across main, include and
eval source units, including repeated eval source names. Reserved-name,
namespace, nested-scope fatal and catchable undefined-constant behavior are
covered by original regressions. The exact full-corpus delta from `ae447c20` is
+18/-0: all fifteen targeted halt/compiler-offset cases and three adjacent
undefined-constant cases become exact with every prior pass preserved. One
closure case advances to its independent `print_r()` formatting gap; no other
non-pass status or category moves, and two final manifests/summaries are
byte-identical. All five feature configurations, all-feature/all-target,
formatting, PHPT runner, unsafe, Composer S0, four Symfony S1 gates and PHP
8.5.9 S2/S3 pass. CPU-pinned 32-pair release controls put independent/paired
median changes at -1.450%/-1.462% for 100 empty requests, -0.745%/-0.681% for
1,000 ordinary variable writes and +0.440%/+0.565% for 1,000 named constant
identifiers, below the +5% gate. Legacy invalid-byte offsets, closure
formatting and broader compatibility remain separate checkpoints.

The `new-expression-grammar` checkpoint reaches 4,053 exact passes with 1,246
failures, 115 skips, no XFAIL, 185 unsupported cases and zero timeouts or
crashes. PHP 8.5 dynamic grouped and static-property class operands now bind
inside `new`; constructor parentheses control result postfix availability, and
invalid bare assignment, unset and unparenthesized postfix forms produce exact
context-sensitive diagnostics. Dynamic class operands execute before arguments
and survive unpacking or suspension without duplicated effects. The exact
full-corpus delta from `cee13fc9` is +12/-0: five parse, six output and one
runtime failure become exact with every prior pass preserved and no other
non-pass movement. Two final manifests/summaries are byte-identical. All five
feature configurations, all-feature/all-target, formatting, PHPT runner,
unsafe, Composer S0, four Symfony S1 gates and PHP 8.5.9 S2/S3 pass. CPU-pinned
32-pair release controls put independent/paired median changes at
+0.584%/+0.655% for 100 empty requests, +1.079%/+0.778% for compiling 25,000
named new expressions and -3.349%/-3.305% for dynamic property class
construction, below the +5% gate. Broader SPL/`ArrayObject` behavior,
independent diagnostics outside the covered forms and broader compatibility
remain separate checkpoints.

The `exit-keyword-contract` checkpoint reaches 4,067 exact passes with 1,232
failures, 115 skips, no XFAIL, 185 unsupported cases and zero timeouts or
crashes. Case-insensitive unqualified `exit` and `die` now share PHP 8.5's
reserved keyword, direct-function and first-class callable identity while
relaxed member, enum-case and named-argument contexts preserve their spelling.
The direct internal `string|int` contract covers strict calls, weak scalar and
float boundaries, diagnostics, stringable objects and catchable conversion
exceptions. The exact full-corpus delta from `ac0bdf91` is +14/-0: eleven
output and three runtime failures under `Zend/tests/exit` become exact with
every prior pass preserved. `throw/leaks.phpt` advances to its independent
`error_reporting()` output boundary after bare exit becomes executable; no
other non-pass movement occurs, and two final manifests/summaries are
byte-identical. All five feature configurations, all-feature/all-target,
formatting, PHPT runner, unsafe, Composer S0, four Symfony S1 gates and PHP
8.5.9 S2/S3 pass. CPU-pinned 32-pair release controls put independent/paired
median changes at +0.916%/+1.017% for 100 empty requests, +0.608%/+0.421% for
1,000 ordinary identifier writes and +0.392%/+0.451% for 25,000
short-circuited exit expressions, below the +5% gate. Process helpers, CLI INI,
output-buffer chunk callbacks and broader compatibility remain separate
checkpoints.

The `floating-classification-contract` checkpoint reaches 4,069 exact passes
with 1,230 failures, 115 skips, no XFAIL, 185 unsupported cases and zero
timeouts or crashes. `is_nan()`, `is_finite()` and `is_infinite()` share PHP
8.5's strict and weak internal `float $num` conversion boundary, including
integer widening, complete numeric strings, null deprecations and exact invalid
type diagnostics; `M_PI` shares the `pi()` value. The exact full-corpus delta
from `f80af9ae` is +2/-0: `bug42143.phpt` and `bug73954.phpt` become exact with
every prior pass preserved. Three other runtime failures advance to independent
string-offset and precision-zero comparison output boundaries; no other
non-pass movement occurs, and two final manifests/summaries are byte-identical.
All five feature configurations, all-feature/all-target, formatting, PHPT
runner, unsafe, Composer S0, four Symfony S1 gates and PHP 8.5.9 S2/S3 pass.
CPU-pinned 32-pair release controls put independent/paired median changes at
-0.442%/-0.345% for 100 empty requests, -3.516%/-3.440% for two million
existing `is_float()` classifications and -0.905%/-0.821% for 500,000 existing
constant lookups, below the +5% gate. `base_convert()`, the later output gaps
and broader compatibility remain separate checkpoints.

The `float-string-comparison-precision` checkpoint reaches 4,070 exact passes
with 1,229 failures, 115 skips, no XFAIL, 185 unsupported cases and zero
timeouts or crashes. Dynamic float-to-nonnumeric-string equality and ordering,
including reversed and nested compound operands, now use request-local PHP
`precision`; numeric strings retain numeric comparison. Scalar constant
comparisons snapshot precision at source-unit compilation, preserving startup,
runtime `ini_set()`, main, include and eval boundaries. The exact full-corpus
delta from `cb3941a3` is +1/-0: `string_to_number_comparison.phpt` moves from
output failure to exact pass with every prior pass preserved, no other non-pass
movement, and two byte-identical final manifests/summaries. All five feature
configurations, all-feature/all-target, formatting, PHPT runner, unsafe,
Composer S0, four Symfony S1 gates and PHP 8.5.9 S2/S3 pass. CPU-pinned 32-pair
release controls put independent/paired median changes at +0.509%/+0.433% for
100 empty requests, -0.323%/-0.923% for five million existing integer
comparisons, +0.022%/-0.033% for one million changed-path comparisons and
+0.477%/+0.192% for compiling and executing 5,000 dynamic comparisons, below
the +5% gate. String offsets, `base_convert()`, deferred Reflection snapshots
and broader compatibility remain separate checkpoints.

The `ini-quantity-contract` checkpoint reaches 4,072 exact passes with 1,227
failures, 115 skips, no XFAIL, 185 unsupported cases and zero timeouts or
crashes. `ini_parse_quantity()` implements PHP 8.5's signed decimal, octal,
binary and hexadecimal grammar, K/M/G multipliers, historical overflow result,
warning taxonomy and byte-oriented diagnostic text. Its strict and weak
`string $shorthand` boundary covers scalar coercion, null deprecation,
stringable objects, concrete invalid types and exceptions from diagnostics or
conversion. The exact full-corpus delta from `a2491a71` is +2/-0:
`gh16886.phpt` and `gh16892.phpt` become exact with every prior pass preserved,
no other non-pass movement, and two byte-identical final manifests/summaries.
All five feature configurations, all-feature/all-target, formatting, PHPT
runner, unsafe, Composer S0, four Symfony S1 gates and PHP 8.5.9 S2/S3 pass. A
5,832-input clean-room ASCII sweep is byte-identical to PHP 8.5.9. CPU-pinned
32-pair release controls put independent/paired median changes at
+0.212%/+0.239% for 100 empty requests and +2.362%/+2.462% for 200,000
existing `parse_ini_string()` calls, below the +5% gate. CLI/INI setting
integration, unavailable `zend_test` helpers, ordinary Reflection snapshots
and broader compatibility remain separate checkpoints.

The `base-convert-contract` checkpoint reaches 4,073 exact passes with 1,226
failures, 115 skips, no XFAIL, 185 unsupported cases and zero timeouts or
crashes. `base_convert()` covers PHP 8.5's bases 2 through 36, matching binary,
octal and hexadecimal prefixes, ignored-character deprecation, strict/weak
`string`, `int`, `int` arguments and the signed-integer-to-double precision
boundary. Undefined static-method diagnostics now render the canonical
declared class name after case-insensitive resolution. The exact full-corpus
delta from `cfe92d09` is +1/-0: `bug70124.phpt` becomes exact with every prior
pass preserved and no other non-pass movement; two final manifests/summaries
are byte-identical. All five feature configurations, all-feature/all-target,
formatting, PHPT runner, unsafe, Composer S0, four Symfony S1 gates and PHP
8.5.9 S2/S3 pass. A 502,245-case clean-room conversion sweep and a separate
argument-boundary matrix are byte-identical to PHP 8.5.9. CPU-pinned 32-pair
release controls put independent/paired median changes at -0.626%/-0.737% for
100 empty requests and +1.866%/+1.831% for two million existing `is_float()`
calls, below the +5% gate. Companion base-conversion functions, arbitrary
precision and broader compatibility remain separate checkpoints.

The `defined-function-inventory` checkpoint reaches 4,074 exact passes with
1,225 failures, 115 skips, no XFAIL, 185 unsupported cases and zero timeouts
or crashes. `get_defined_functions()` exposes the live RPHP internal/user
function inventory, excludes methods and runtime closures, and implements PHP
8.5's deprecated optional-bool call boundary without claiming php-src's full
extension set or list order. The exact full-corpus delta from `73147de1` is
+1/-0: `get_defined_functions_basic.phpt` becomes exact with every prior pass
preserved and no other status or category movement; two final manifests and
summaries are byte-identical. Both arginfo mismatch probes advance to an
independent `chunk_split()` failure without changing category. All five
feature configurations, all-feature/all-target, formatting, PHPT runner,
unsafe, Composer S0, four Symfony S1 gates and PHP 8.5.9 S2/S3 pass. The one
new cold validated function-table pointer raises the production inventory to
1,619 unsafe blocks and 331 SAFETY annotations while retaining 289 unsafe
functions. CPU-pinned 32-pair release controls put independent/paired median
changes at +1.320%/+0.082% for 100 empty requests and -2.642%/-2.618% for two
million existing `is_float()` calls, below the +5% gate. Disabled-function
configuration, exact extension inventory, `chunk_split()` and broader
compatibility remain separate checkpoints.

The `array-fill-keys-contract` checkpoint reaches 4,075 exact passes with
1,224 failures, 115 skips, no XFAIL, 185 unsupported cases and zero timeouts
or crashes. `array_fill_keys()` implements PHP 8.5 integer/canonical-string
key identity, scalar/resource/array/Stringable conversions, duplicate order,
mixed-value object identity and array COW, reference detachment, diagnostics
and throwing conversion paths. The exact full-corpus delta from `cb107ee1` is
+1/-0: `bug45877.phpt` becomes exact with every prior pass preserved and no
other status or category movement; two final manifests and summaries are
byte-identical. The supplying Zend case and all five adjacent unmodified
`ext/standard` PHPTs pass. All five feature configurations,
all-feature/all-target, formatting, PHPT runner, unsafe, Composer S0, four
Symfony S1 gates and PHP 8.5.9 S2/S3 pass. No unsafe, opcode, layout,
dependency or hot array-path change is made. CPU-pinned 32-pair release
controls put independent/paired median changes at +0.457%/+0.382% for 100
empty requests and -3.459%/-3.857% for 500,000 existing `array_fill()` calls,
below the +5% gate. Huge-allocation failures, the complete standard array
suite, companion array functions and broader compatibility remain separate
checkpoints.

The `spl-object-hash-contract` checkpoint reaches 4,076 exact passes with
1,223 failures, 115 skips, no XFAIL, 185 unsupported cases and zero timeouts
or crashes. `spl_object_hash()` exposes PHP 8.5's stable 32-character
lower-case hexadecimal encoding of the existing request-local object handle
for ordinary objects and closures; live identities remain distinct and the
encoded handle agrees with `spl_object_id()`. Both functions share the exact
object-only argument boundary, including boolean diagnostic spelling, without
changing object representation or lifecycle. The exact full-corpus delta from
`53ffec3b` is +1/-0: `bug60598.phpt` becomes exact with every prior pass
preserved and no other status or category movement; two final manifests and
summaries are byte-identical. All five feature configurations,
all-feature/all-target, formatting, PHPT runner, unsafe, Composer S0, four
Symfony S1 gates and PHP 8.5.9 S2/S3 pass. No unsafe, opcode, layout,
dependency or object-lifecycle change is made. CPU-pinned 32-pair release
controls put independent/paired median changes at -0.430%/-0.450% for 100
empty requests and +1.488%/+1.524% for two million existing `spl_object_id()`
calls, below the +5% gate. Cross-request uniqueness, mandated handle-reuse
timing, the complete SPL object suite and broader compatibility remain
separate checkpoints.

The `stristr-contract` checkpoint reaches 4,077 exact passes with 1,222
failures, 115 skips, no XFAIL, 185 unsupported cases and zero timeouts or
crashes. `stristr()` implements PHP 8.5 binary-safe first-match suffix/prefix
selection, empty and NUL needles, locale-independent ASCII folding,
non-ASCII byte identity, scalar/Stringable conversion, null/NAN diagnostics,
throwing-handler order and parameter-specific weak/strict TypeErrors. The
exact full-corpus delta from `bc9bc620` is +1/-0: `gh12457.phpt` becomes exact
with every prior pass preserved and no other status or category movement; two
final manifests and summaries are byte-identical. Three adjacent unmodified
`ext/standard` PHPTs also pass, while the fourth validates its leading
`stristr()` results before reaching independent missing `md5()`. All five
feature configurations, all-feature/all-target, formatting, PHPT runner,
unsafe, Composer S0, four Symfony S1 gates and PHP 8.5.9 S2/S3 pass. No unsafe,
compiler, opcode, layout, dependency or existing handler change is made.
CPU-pinned 32-pair release controls put independent/paired median changes at
+0.531%/+0.327% for 100 empty requests and +1.407%/+1.392% for two million
existing `strstr()` calls, below the +5% gate. Unicode folding, `md5()`, strict
metadata propagation through detached internal calls, the complete string
suite and broader compatibility remain separate checkpoints.

The `md5-contract` checkpoint reaches 4,078 exact passes with 1,221 failures,
115 skips, no XFAIL, 185 unsupported cases and zero timeouts or crashes. A
clean-room RFC 1321 digest powers PHP 8.5-compatible `md5()` hexadecimal and
raw byte output plus valid one-shot `hash('md5', ...)` calls. The typed weak
and strict boundary covers scalar/Stringable conversion, null/NAN diagnostics,
throwing-handler order, invalid `__toString()` returns and parameter-specific
TypeErrors. The exact full-corpus delta from `0ccfebf1` is +1/-0:
`gh19280.phpt` becomes exact with every prior pass preserved and no other
status or category movement; two final manifests and summaries are byte-
identical. Four upstream standard MD5 PHPTs, an adjacent hash PHPT and the
previously blocked `stristr.phpt` also pass. All five feature configurations,
all-feature/all-target, formatting, PHPT runner, unsafe, Composer S0, four
Symfony S1 gates and PHP 8.5.9 S2/S3 pass. No unsafe, compiler, opcode, layout
or dependency change is made. CPU-pinned 32-pair release controls put
independent/paired median changes at -0.900%/-0.778% for 100 empty requests and
-1.477%/-1.466% for two million existing `hash('xxh128', ...)` calls, below
the +5% gate. Password/security suitability, file/HMAC/streaming hashing,
broader binary-string consumers, detached-call strict metadata, the complete
hash extension and broader compatibility remain separate checkpoints.

The `similar-text-contract` checkpoint reaches 4,079 exact passes with 1,220
failures, 115 skips, no XFAIL, 185 unsupported cases and zero timeouts or
crashes. PHP's byte-oriented Oliver algorithm supplies the first longest
common contiguous substring in each recursively partitioned region, including
its order-sensitive tie behavior, while an explicit work stack avoids native
recursion. The optional output reference receives the PHP percentage; weak and
strict typed strings, binary inputs, diagnostics, Stringable conversion and
exact TypeErrors are covered. The compiler also treats internal by-reference
metadata case-insensitively without an allocation for ordinary lower-case
names, and evaluates then rejects a simple assignment result supplied to any
by-reference parameter. The exact full-corpus delta from `8395f772` is +1/-0:
`bug78154.phpt` becomes exact with every prior pass preserved and no other
status or category movement; two final manifests and summaries are byte-
identical. An independent upstream `similar_text` PHPT and an exhaustive
14,641-pair clean-room PHP 8.5.9 differential sweep pass. All five feature
configurations, all-feature/all-target, formatting, PHPT runner, unsafe,
Composer S0, four Symfony S1 gates and PHP 8.5.9 S2/S3 pass. No opcode,
ABI/value layout, dependency or unsafe change is made. CPU-pinned 32-pair
release controls put independent/paired median changes at +1.177%/+0.826% for
100 empty requests and +3.110%/+3.042% for two million existing
`levenshtein()` calls, below the +5% gate. Faster or Unicode-aware similarity,
large/untrusted-input suitability, every complex by-reference expression form,
detached-call strict metadata, the complete string suite and broader
compatibility remain separate checkpoints.

The `strtok-shuffle-batch` checkpoint reaches 4,081 exact passes with 1,218
failures, 115 skips, no XFAIL, 185 unsupported cases and zero timeouts or
crashes. `strtok()` implements PHP's request-local byte cursor, per-call
delimiter sets, source-copy isolation, exhausted/invalidated warning states,
nullable continuation overload and atomic weak/strict typed boundary.
`str_shuffle()` supplies an unbiased byte permutation, while existing
by-reference `shuffle()` shares its lazily seeded request PRNG, list reindexing
and exact array TypeError. The exact full-corpus delta from `9b56808a` is
+2/-0: `bug76047.phpt` and `gh13145.phpt` become exact with every prior pass
preserved and no other status or category movement; two final manifests and
summaries are byte-identical. Seventeen focused upstream PHPTs pass, and a
7,161-case clean-room differential sweep reproduces PHP 8.5.9's token digest
and all 8,848 warning transitions. Two broader shuffle PHPTs advance to the
independent missing `sprintf('%0.3f')` and `array_diff_assoc()` surfaces. All
five feature configurations, all-feature/all-target, formatting, PHPT runner,
unsafe, Composer S0, four Symfony S1 gates and PHP 8.5.9 S2/S3 pass. A single
cold null sidecar word is added to `ExecutorGlobals`; no opcode, call-frame,
PHP Value/object layout, dependency or unsafe change is made. CPU-pinned
32-pair release controls put independent/paired median changes at
-0.905%/-0.424% for 100 empty requests and -3.137%/-3.446% for 500,000 existing
eight-element `shuffle()` calls. Cryptographic or PHP-exact seeded randomness,
other random APIs, the two later blockers, reentrant tokenizer mutation, the
complete string/random suites and broader compatibility remain separate
checkpoints.

The `unix-process-helper-batch` checkpoint reaches 4,088 exact passes with
1,211 failures, 115 skips, no XFAIL, 185 unsupported cases and zero timeouts
or crashes. On AMD64 Unix in C/POSIX locale, `escapeshellarg()` and
`escapeshellcmd()` implement PHP's byte quoting, quote-state, invalid-byte,
null-byte and length contracts; `exec()` and `shell_exec()` synchronously use
`/bin/sh -c` with binary stdout and PHP-compatible last-line, output-array,
status and no-output behavior. Positional/named by-reference metadata,
weak/strict command conversion, the one-array-argument `implode()`/`join()`
overload and the supplying `-n`/`--no-php-ini`/`-e`/truthful `-a` CLI paths are
included. The exact full-corpus delta from `80eaec31` is +7/-0: `bug40236`,
`bug80811`, the three named/statement/value exit cases plus `bug60978`, and
`gh21504` become exact with every previous pass preserved. Five old failures
advance from runtime to their independent output mismatches without changing
status; two final manifests and summaries are byte-identical. Fourteen focused
upstream PHPTs pass, including both 64 MiB escape limits, while three stop at
the independent `.5` literal, `uniqid()` and `system()` surfaces. A 510-case
byte oracle and 74,898-case clean-room quote/control/non-ASCII sweep are byte-
identical to PHP 8.5.9. All five feature configurations, all-feature/all-
target, formatting, PHPT runner, unsafe, Composer S0, four Symfony S1 gates
and PHP 8.5.9 S2/S3 pass. One documented production unsafe block raises the
inventory to 1,620/289/332, and the fixed function-table envelope moves from
896 to 1,792 slots; no opcode definition, call-frame, PHP Value/object layout
or dependency change is made. CPU-pinned 32-pair release controls put
independent/paired median changes at +0.665%/+0.745% for 100 empty requests and
+2.300%/+2.048% for two million existing two-argument `implode()` calls, below
the +5% gate.
Windows/locales beyond C, untrusted-input safety, async/timeout/stream process
APIs, `system()`/`passthru()`/`proc_open()`/backticks, real `-e` extended info,
readline `-a`, internal Reflection arginfo, the three later blockers and
broader compatibility remain separate checkpoints.

The `source-text-filter-batch` checkpoint reaches 4,097 exact passes with
1,206 failures, 115 skips, no XFAIL, 181 unsupported cases and zero timeouts
or crashes. A shared cold scanner supplies `strip_tags()`,
`highlight_string()`, `highlight_file()`/`show_source()` and
`php_strip_whitespace()` across the exercised PHP/HTML transitions, comments,
quoted strings, heredoc/nowdoc, allowed tags, malformed brackets, color
overrides and local-file diagnostics. The five real `highlight.*` CLI-INI
directives are now admitted by the runner. The exact full-corpus delta from
`e861099a` is +9/-0 with every previous pass preserved: five runtime failures
and four unsupported CLI-INI cases become exact, while `bug36513.phpt`
advances only to its independent `eval()` output mismatch. Two final manifests
and summaries are byte-identical. Nine of ten focused Zend PHPTs and 21 of 44
adjacent `ext/standard` samples pass; the remainder delimit independent parser,
stdlib and rarer scanner work. All five feature configurations, all-feature/
all-target, formatting, PHPT runner, the unchanged 1,620/289/332 unsafe
ratchet, Composer S0, four Symfony S1 gates and PHP 8.5.9 S2/S3 pass. No
opcode, call-frame, Value/object layout, dependency or unsafe change is made.
CPU-pinned 32-pair release startup controls put independent/paired median
changes at -3.305%/-3.634%, below the +5% gate. Complete lexer fidelity,
remote/custom wrappers, broader path diagnostics, every historical malformed
`strip_tags()` state, sanitizer suitability, the `eval()` blocker and broader
compatibility remain separate checkpoints.

The `recursive-array-combiner-batch` checkpoint reaches 4,098 exact passes
with 1,205 failures, 115 skips, no XFAIL, 181 unsupported cases and zero
timeouts or crashes. `array_merge_recursive()` and
`array_replace_recursive()` implement the exercised PHP 8.5 key, scalar,
object, copy-on-write, reference, recursion and next-index-overflow contracts
without changing the existing non-recursive array functions. The exact
full-corpus delta from `bfcc6bd1` is +1/-0: the Zend next-key-overflow case
becomes exact with every previous pass preserved and no other status or
category movement; two final manifests and summaries are byte-identical.
Twenty-one of 28 focused upstream PHPTs pass, and a 260-case clean-room matrix
is byte-identical to PHP 8.5.9. All five feature configurations, all-feature/
all-target, formatting, PHPT runner, the unchanged 1,620/289/332 unsafe
ratchet, Composer S0, four Symfony S1 gates and PHP 8.5.9 S2/S3 pass. CPU-
pinned 32-pair release controls put independent/paired median changes at
+0.559%/+0.419% for 100 empty requests, -0.873%/-1.160% for 500,000 existing
`array_replace()` calls and -0.322%/-0.460% for 500,000 adjacent
`array_merge()` calls, below the +5% gate. The independent parser and string-
escape blockers, missing array-intersection family, adversarial recursion,
the complete array suite and broader compatibility remain separate work.

The `array-traversal-batch` checkpoint adds the six PHP 8.4+/8.5 array
traversal functions `array_find()`, `array_find_key()`, `array_any()`,
`array_all()`, `array_first()` and `array_last()`. The callback functions
preserve insertion-order value/key dispatch, exact short-circuiting, structural
snapshot semantics, live referenced elements, copy-on-write and exception/
diagnostic behavior; first/last return dereferenced endpoint values. The
supplying nine unmodified `ext/standard` PHPTs move from 0/9 to 9/9 in debug
and release, and a 240-case clean-room matrix is byte-identical to PHP 8.5.9.
The first/last fixture additionally closes the directly reached JSON
prerequisite by dereferencing elements and retaining PHP array insertion order.
Because these extension tests are outside the pinned `Zend/tests` plus
`tests/lang` corpus, two byte-identical full manifests intentionally remain at
4,098 pass, 1,205 fail, 115 skip and 181 unsupported with zero timeouts or
crashes and an exact +0/-0 pass-set delta from `8d038d4a`.

All five feature configurations, all-feature/all-target, formatting, PHPT
runner, the unchanged 1,620/289/332 unsafe ratchet, Composer S0, four Symfony
S1 gates and PHP 8.5.9 S2/S3 pass. No compiler/opcode, call-frame, PHP
Value/object layout, dependency or unsafe change is made. CPU-pinned 32-pair
release controls put independent/paired median changes at +0.244%/+0.168% for
100 empty requests, +2.088%/+2.045% for 500,000 existing `array_filter()`
callback calls and -24.993%/-25.284% for 500,000 directly affected ordered
`json_encode()` calls, below the +5% gate. Complete array/JSON suites, object-
property ordering beyond the retained projection and broader compatibility
remain separate work.

The `array-assoc-set-batch` checkpoint adds `array_diff_assoc()`,
`array_diff_uassoc()`, `array_diff_ukey()`, `array_intersect_assoc()`,
`array_intersect_uassoc()` and `array_intersect_ukey()`. The ordinary variants
compare exact keys and PHP string-converted values; the user variants preserve
the exercised callback-validation precedence, small-array comparator schedule,
short-circuiting, exception propagation, structural snapshots, live reference
cells and first-array insertion order. A 300-result clean-room matrix is byte-
identical to PHP 8.5.9. Of 55 supplying unmodified `ext/standard` PHPTs, all
42 runtime-reachable cases pass in debug and release; 13 stop at independent
leading-dot numeric or binary-string parser gaps, while the PHP oracle passes
55/55. Because this cluster is outside the pinned Zend/lang corpus, two byte-
identical full manifests remain at 4,098 pass, 1,205 fail, 115 skip and 181
unsupported with zero timeouts or crashes and an exact +0/-0 delta from
`78846be8`.

All five feature configurations, all-feature/all-target, formatting, PHPT
runner, the unchanged 1,620/289/332 unsafe ratchet, Composer S0, four Symfony
S1 gates and PHP 8.5.9 S2/S3 pass. No compiler/opcode, call-frame, PHP
Value/object layout, dependency or unsafe change is made. CPU-pinned 32-pair
release controls put independent/paired median changes at +0.077%/+0.094% for
100 empty requests and -0.675%/-0.830% for 500,000 existing
`array_diff_key()` calls, below the +5% gate. The stable O(n log n) large-array
baseline does not claim PHP's exact arbitrary-large comparator trace. The 13
parser forms, `array_change_key_case()`, six `array_udiff*`/`array_uintersect*`
functions, the complete array suite and broader compatibility remain separate
work.

The `array-user-value-set-batch` checkpoint adds `array_udiff()`,
`array_udiff_assoc()`, `array_udiff_uassoc()`, `array_uintersect()`,
`array_uintersect_assoc()` and `array_uintersect_uassoc()`. The variants cover
user-value, exact-key plus user-value, and user-key plus user-value comparison
while preserving PHP 8.5 duplicates, first-array order, callback validation,
exceptions, structural snapshots and live reference cells. It also corrects
`array_diff_key()` and `array_intersect_key()` to their one-required-array
variadic signatures and identities. A 300-result clean-room matrix is byte-
identical to PHP 8.5.9. Of 33 supplying unmodified `ext/standard` PHPTs, 17
pass in both debug and release; 12 stop at independent parser gaps, three at
pre-existing output formatting, and one at a later ordinary three-array
`array_intersect()` call. The PHP oracle passes 33/33. Because this cluster is
outside the pinned Zend/lang corpus, two byte-identical full manifests remain
at 4,098 pass, 1,205 fail, 115 skip and 181 unsupported with zero timeouts or
crashes and an exact +0/-0 delta from `c8c43701`.

All five feature configurations, all-feature/all-target, formatting, PHPT
runner, the unchanged 1,620/289/332 unsafe ratchet, Composer S0, four Symfony
S1 gates and PHP 8.5.9 S2/S3 pass. No compiler/opcode, call-frame, PHP
Value/object layout, dependency or unsafe change is made. CPU-pinned 32-pair
release controls put independent/paired median changes at -0.118%/+0.137% for
100 empty requests, -0.543%/+0.072% for one 500,000-entry
`array_diff_key()` request and -2.325%/-2.046% for 500,000 unchanged
`array_intersect()` calls, below the +5% gate. An attempted ordinary variadic
`array_intersect()` prerequisite was reverted after approximately +154% and,
with a scalar fast path, approximately +20% regressions. The stable O(n log n)
large-array baseline does not claim PHP's exact arbitrary-large comparator
trace. The 16 independent focused blockers, `array_change_key_case()`, ordinary
variadic `array_intersect()`, the complete array suite and broader compatibility
remain separate work.

The `array-lookup-aggregate-batch` checkpoint advances `in_array()`,
`array_search()`, `array_key_exists()`/`key_exists()`, `array_sum()` and
`array_product()` together. The admitted contract covers reflected arity and
parameter names, typed arguments, weak and strict conversion, canonical keys,
ordered loose/strict search, exact large numeric-string comparison, recursive
array errors, references/COW, reentrant warnings and throwing handlers,
sequential numeric-kind transitions and integer-overflow promotion. Adjacent
checked comparison also covers recursive `array_keys()` filtering and PHP's
source-dependent CV/CV commutative operator diagnostic order. No php-src or
third-party implementation or test is copied or mechanically translated.

The 42-case focused cluster moves from 15 to 31 passes, an exact +16/-0 delta;
PHP 8.5.9 records 37 passes, one local FFI configuration failure and four
extension skips. The complete recursive 842-case array audit moves from 546
to 564 passes (+18/-0), with 264 failures, 13 skips, one unsupported case and
zero timeouts or crashes. The pinned Zend/lang corpus moves from 4,108 to
4,109 passes (+1/-0), with 1,194 failures, 115 skips, 181 unsupported cases
and zero timeouts or crashes. Two serial focused, array and main manifests and
summaries are respectively byte-identical. Five original E2E programs produce
the same 3,503 bytes under RPHP and PHP 8.5.9.

All five feature configurations, all-feature/all-target, formatting, PHPT
runner, the unchanged 1,623/289 unsafe ratchet, Composer S0, four Symfony S1
gates and PHP 8.5.9 S2/S3 pass. No opcode, frame, PHP value/object/array
layout, dependency or production unsafe change is made. CPU-pinned 32-pair
balanced release controls put paired medians between -51.410% and +0.313% for
startup, lookup, key existence, homogeneous sums/products and scalar
comparison, below the +5% gate with exact output. A mixed integer/double sum
lane is separately disclosed at +249.168% because the baseline violates the
newly proven sequential numeric-kind contract; optimizing that correct path
is handed to the execution/performance workstream.

Three focused parser failures and two missing-`opendir()` failures remain.
Complete internal Reflection type/default/return metadata, the remaining
array suite and broader compatibility are not claimed.

The `cardinality-extrema-batch` checkpoint advances `count()`/`sizeof()` and
`min()`/`max()` together. The admitted PHP 8.5 contract covers reflected
signatures and constants, typed and weak modes, validation precedence,
Countable dispatch, iterative recursive counting with structural snapshots,
shared-array identity, active-cycle warnings, reentrant handlers and deep
inputs. Extrema cover one-array and variadic forms, errors, references, exact
numeric strings, arrays, objects, recursive comparisons, ties and NaN.
Checked null/string ordering and the observable distinction between direct
binary and dynamic extrema calls are preserved. The direct case reuses the
existing binary internal-call ABI; no opcode is added, and no php-src or
third-party implementation or test is copied or translated.

The 23-case focused cluster moves from 1 to 22 passes (+21/-0); PHP 8.5.9
passes 23/23, while RPHP's remaining case proceeds through recursive count and
then stops at missing `opendir()`. The complete 842-case recursive array audit
moves from 564 to 585 passes (+21/-0), with 243 failures, 13 skips, one
unsupported case and zero timeouts or crashes. The pinned Zend/lang corpus is
byte-identical at 4,109 passes, 1,194 failures, 115 skips and 181 unsupported
cases with zero timeouts or crashes. Two serial focused, array and main
manifests and summaries are respectively byte-identical. Five original E2E
programs produce the same 2,401 bytes under RPHP and PHP 8.5.9.

All five feature configurations, all-feature/all-target, formatting, PHPT
runner, the unchanged 1,623/289 unsafe ratchet, Composer S0, four Symfony S1
gates and PHP 8.5.9 S2/S3 pass. No frame, PHP value/object/array layout,
dependency or production unsafe change is made. CPU-pinned 32-pair balanced
release controls put paired medians at +0.141% for startup, +1.892% for flat
count, -57.694% for direct binary extrema and +0.801% for unchanged scalar
comparison, below the +5% gate with exact output. Recursive count, array-form
extrema and variadic extrema are disclosed without A/B ratios because the
baseline lacks those contracts. Missing `opendir()`, complete internal
Reflection type/default/return metadata, the remaining array suite and broader
compatibility are not claimed.

The `directory-stream-functions-batch` checkpoint advances `chdir()`,
`opendir()`, `readdir()`, `rewinddir()`, `closedir()` and `scandir()` together.
The admitted PHP 8.5 contract covers reflected arity and parameter names,
strict/weak inputs, the three scan-order constants and modes, stream-context
validation, warning re-entry and exceptions, request-owned resources with the
public `stream` type, EOF/rewind/close behavior and the deprecated implicit
last-directory handle. A request-local initial-CWD guard restores the process
working directory at shutdown without consuming a PHP resource id. The work is
independent and does not copy or mechanically translate php-src source/tests or
an external named algorithm.

The 39-case focused cluster moves from 0 to 30 passes (+30/-0); PHP 8.5.9
records 38 passes and one local filesystem-order output mismatch. The complete
49-case non-Windows directory audit moves from no passes, 44 failures and five
skips to 27 passes, 17 failures and five skips (+27/-0). The complete recursive
842-case array audit moves from 585 to 590 passes (+5/-0), closing five fixture
setup failures. The pinned Zend/lang corpus retains exactly 4,109 passes with
no lost pass, timeout or crash; one still-failing test moves from its former
missing-function runtime stop to a later independent output mismatch. Two
focused, directory, array and main runs have byte-identical manifests and
summaries. Three original regressions pass, and an 881-byte clean-room
transcript is byte-identical to PHP 8.5.9.

All five feature configurations, all-feature/all-target, formatting, PHPT
runner, the unchanged 1,623/289 unsafe ratchet, Composer S0, four Symfony S1
gates and PHP 8.5.9 S2/S3 pass. No opcode, frame, PHP value/object/array
layout, dependency or production unsafe change is made. CPU-pinned 32-pair
balanced A/B controls put paired medians at -0.032% for startup, -1.751% for
unchanged `getcwd()` and +1.542% for unchanged `file_exists()`, below the +5%
gate. New-only `scandir()` and `opendir()`/`readdir()` workloads match PHP
output and are disclosed without baseline ratios.

Native enumeration order remains filesystem-dependent. The `dir()` object,
reverse `fclose()` validation, remaining `SKIPIF`/parser prerequisites and
broader directory/array compatibility remain separate work.

The `stream-context-default-batch` checkpoint promotes
`stream_context_create()`, `stream_context_get_default()`,
`stream_context_get_options()`, `stream_context_get_params()`,
`stream_context_set_default()`, `stream_context_set_option()`,
`stream_context_set_options()` and `stream_context_set_params()` together. The
admitted PHP 8.5 contract covers reflected arity/names, request-local resource
identity, option/parameter merging and mutation, callback diagnostics, the
legacy two-argument deprecation and extended `fopen()` validation. The ordinary
two-argument `fopen()` body remains its established path; stream contexts are
lazily boxed only when stream-local state is first mutated. The implementation
is independent and does not copy or mechanically translate php-src source/tests
or an external named algorithm.

The 12-case focus moves from zero to eight passes (+8/-0). The complete
158-case streams audit moves from 7 to 17 passes (+10/-0), while the 49-case
non-Windows directory audit gains `scandir_basic.phpt` and reaches 28 passes
(+1/-0). The 842-case array and serial 897-case file audits retain exact pass
sets of 590 and 92 respectively, and the pinned Zend/lang corpus remains byte-
identical at 4,109 passes, 1,194 failures, 115 skips and 181 unsupported cases.
No corpus has a lost pass, timeout or crash; two remaining failures advance to
later explained `preg_replace()` and `unlink()` gaps. Two final runs of every
corpus are byte-identical. An original regression and a 631-byte clean-room
PHP/RPHP transcript cover the promoted surface and diagnostics.

All five feature configurations, all-feature/all-target, formatting, PHPT
runner, the unchanged 1,623/289 unsafe ratchet, Composer S0, four Symfony S1
gates and PHP 8.5.9 S2/S3 pass. The initial inline context representation was
rejected after a +7.338% to +9.420% `php://memory` regression. With lazy boxed
state, two final CPU-pinned 32-pair runs put paired medians at
+0.764%/+0.852% for startup, -0.077%/+0.673% for `php://memory`,
+0.348%/+0.495% for ordinary file streams and -3.026%/-2.890% for unchanged
`file_exists()`, below the +5% gate. The new-only context API is disclosed
without a baseline ratio.

TCP/UDP/socket wrappers and their three focused cases remain unavailable.
`stream_context_set_options_error.phpt` remains blocked by missing
`proc_open()`, and broader stream/file/directory/array compatibility is not
claimed.

The `stream-operations-default-batch` checkpoint promotes
`stream_get_filters()`, `stream_get_transports()`, `stream_get_wrappers()`,
`stream_is_local()`, `stream_get_contents()`, `stream_copy_to_stream()`,
`stream_get_line()` and `ftruncate()` together. The admitted PHP 8.5 contract
covers reflected arity/names, named arguments, truthful registry/locality
policy, bounded and offset reads, fixed-chunk copies, arbitrary byte endings,
cursor/EOF behavior, memory/temp/file truncation and covered diagnostics. The
`stream_is_local()` parameter metadata is corrected to `$stream`. The existing
implementations remain independently selectable under `--no-default-features`;
the promotion adds no dependency, lockfile or unsafe change and introduces no
new external algorithm.

The 93-case focus moves from 0 to 17 passes (+17/-0); five additional cases
truthfully move from failed prerequisites to skips. The complete 158-case
streams audit moves from 17 to 22 passes (+5/-0), and the complete 897-case
file audit moves from 92 to 104 passes (+12/-0). The 842-case array and 49-case
non-Windows directory manifests remain byte-identical at 590 and 28 passes.
The pinned Zend/lang corpus also remains byte-identical at 4,109 passes, 1,194
failures, 115 skips and 181 unsupported cases, with zero timeout or crash; two
final full runs are byte-identical. The original default stream suite runs 33
cases, and a 698-byte clean-room transcript is byte-identical to PHP 8.5.9.

All five feature configurations, all-feature/all-target, formatting, PHPT
runner, the unchanged 1,623/289 unsafe ratchet, Composer S0, four Symfony S1
gates and PHP 8.5.9 S2/S3 pass. CPU-pinned 32-pair release controls put paired
medians at -0.400% for startup, -0.927% for `php://memory`, -0.151% for
ordinary file streams, +1.088% for unchanged `file_exists()` and +0.577% for
the unchanged stream-context API, below the +5% gate. New-only operation and
registry workloads match PHP output and are disclosed without baseline ratios.

Network wrappers/transports, filters, custom wrappers, sockets/nonblocking I/O,
complete internal return-type metadata and helper-blocked remaining stream/file
cases stay separate work.

The `file-include-csv-default-batch` checkpoint promotes
`get_include_path()`, `set_include_path()`, `stream_resolve_include_path()` and
`fputcsv()`, together with the extended `file_get_contents()`,
`file_put_contents()`, `file()`, `fopen()` and `fgetcsv()` contracts. The
admitted PHP 8.5 behavior covers signatures/named arguments, ordered and
canonical local include resolution, offsets/lengths, file flags/contexts, CSV
quoting/custom endings, argument errors, omitted-escape deprecations and the
array-field warning with throwing-handler side-effect boundaries. The
implementations remain separately selectable under `--no-default-features`.

The 265-case focus moves from 53 to 85 passes (+32/-0), the complete 897-case
file audit from 104 to 135 (+31/-0), and the complete 158-case streams audit
from 22 to 23 (+1/-0), with no timeout or crash. The pinned Zend/lang corpus
moves from 4,109 to 4,110 passes (+1/-0 through `gh10232.phpt`), with 1,193
failures, 115 skips and 181 unsupported cases; two final full manifests and
summaries are byte-identical. A 454-byte original signature/behavior transcript
is byte-identical to PHP 8.5.9.

All five feature configurations, all-feature/all-target, formatting, PHPT
runner, the unchanged 1,623/289/339 unsafe ratchet, Composer S0, four Symfony
S1 gates and PHP 8.5.9 S2/S3 pass. The first performance candidate was rejected
at +38.764% for ordinary `file()` and +11.846% for explicit `fgetcsv()`; the
accepted bulk-read and validated fast paths put the seven comparable paired
medians between -3.306% and +0.942%, below the +5% gate. New-only include-path
and `fputcsv()` workloads match PHP output and are disclosed without baseline
ratios. No dependency, opcode, frame, value/object/array layout or unsafe
change is made.

CLI-INI include-path restoration, network/custom wrappers, filters, sockets,
resource lifetime, complete internal return-type metadata, the independent
compiler/reference boundary in later complex CSV cases and remaining file/
stream compatibility stay separate work.

The `formatted-io-batch` checkpoint advances `fprintf()`, `vfprintf()`,
`sscanf()` and `fscanf()` together behind an independently selectable feature
that is also enabled by default. Shared cold parsers cover the admitted output
flags, widths, precision, padding and conversions, and input literals,
whitespace, widths, suppression, positions, bases, floating values, scansets,
`%n`, typed arrays, multiple reference outputs and physical stream-line/EOF
behavior. A narrowly opt-in raw variadic policy keeps every scanf output alias
live while readable inputs are snapshotted; other internal variadics retain
their existing one-extra raw or packed ABI.

The 105-case PHP 8.5 focus moves from 0 to 84 passes (+84/-0), with eight
failures and 13 skips; PHP 8.5.9 passes all 92 ordinary attempts. The complete
897-case file audit moves from 135 to 181 passes (+46/-0). The pinned 5,599-case
Zend/lang corpus moves from 4,110 to 4,111 passes (+1/-0 through
`Zend/tests/dim_assign_001.phpt`), with 1,192 failures, 115 skips, 181
unsupported cases and no timeout or crash. Two final main manifests and
summaries are byte-identical. Five original E2E tests and three scanner/
formatter unit tests cover the admitted behavior without copying php-src tests.

All five feature configurations, all-feature/all-target, formatting, PHPT
runner, the unchanged 1,623/289/339 unsafe ratchet, Composer S0, four Symfony
S1 gates and PHP 8.5.9 S2/S3 pass. CPU-pinned 32-pair release controls put
paired median changes at -0.142% for startup, +1.691% for unchanged
single-value `sprintf()` and +0.925% for unchanged multi-value `sprintf()`,
below the +5% gate. New-only `sscanf()` and `fprintf()` workloads remain about
3.8 to 5.1 times slower than PHP 8.5.9 and are disclosed as optimization
opportunities rather than improvements. No dependency, lockfile, opcode,
`ExecuteData`, PHP value/object/array layout or unsafe block is added.

Eight focused cases remain blocked by independent `touch()`/`proc_open()`,
stream warning, `fwrite()` object conversion, parser, array-dimension reference
and older `printf()` conversion gaps. Complete printf/scanf edge behavior,
internal Reflection return types and broader standard-library compatibility
remain separate work.

The `reflection-invocation-metadata-batch` checkpoint advances
`ReflectionFunction::invoke()` and `invokeArgs()`,
`ReflectionMethod::invokeArgs()`, and the receiver, named-argument and true
variadic behavior of `ReflectionMethod::invoke()` together. Function metadata
adds short and namespace names, namespace membership, original closure names
and closure scope classes, including PHP's dummy `Closure` scope after
object-only binding. Public `ReflectionClass` debug projection completes the
admitted metadata slice.

The 14-case PHP 8.5 focus moves from one to 12 passes (+11/-0), the complete
493-case Reflection audit from 78 to 89 (+11/-0), and the adjacent 163-case
closure/dynamic-call audit from 95 to 100 (+5/-0), without a timeout, crash or
lost pass. The pinned 5,599-case Zend/lang corpus moves from 4,111 to 4,117
passes (+6/-0), with 1,186 failures, 115 skips and 181 unsupported cases; two
final full manifests and summaries are byte-identical. Five original E2E tests
cover the admitted function, method and closure behavior without copying
php-src source or tests.

All five feature configurations, all-feature/all-target, formatting, PHPT
runner, the unchanged 1,623/289 unsafe ratchet with 341 SAFETY annotations,
Composer S0, four Symfony S1 gates and PHP 8.5.9 S2/S3 pass. No dependency,
lockfile, opcode, frame or PHP value/object/array layout is added; `PhpClosure`
uses existing AMD64 padding and the four-byte `CallPlan` uses its final spare
flag for static-method metadata.

The first generic invocation implementation was rejected after regressing the
unchanged Reflection method control by about 96%. The accepted narrow raw
internal-variadic path preserves the packed ABI for wider and named calls.
CPU-pinned 32-pair release controls put paired medians at -0.006% for startup,
-0.258% for closure calls, +0.472% for closure binding and +1.162% for
unchanged Reflection method invocation, below the +5% gate with exact output.

General unknown named forwarding in `call_user_func()`, `DateTime`, complete
Reflection type/default/return metadata, closure serialization and broader
compatibility remain separate work.

The `array-key-value-batch` checkpoint advances `array_keys()`,
`array_values()`, `array_flip()`, `array_count_values()` and `array_rand()`
together. The admitted contract covers reflected arity and parameter names,
typed errors, weak integer/bool conversion, loose and strict value filtering,
canonical decimal keys, duplicate replacement and insertion order, skipped-
value warnings and throwing handlers, reference/COW-preserving packed value
projection, uniform random selection without replacement, cardinality and
ValueError precedence. General loose comparison now handles null/string pairs
as PHP 8.5 does, including null not equalling `"0"`. No php-src implementation
or test is copied or mechanically translated.

The 33-case unmodified PHP 8.5 focused cluster moves from 10 to 26 passes, an
exact +16/-0 delta; PHP 8.5.9 records 32 passes and one architecture skip. The
complete recursive 842-case array audit moves from 525 to 546 passes with no
lost pass, reducing failures from 303 to 282 while 13 skips, one unsupported
case and zero timeouts or crashes remain. Five adjacent cases pass outside the
focused cluster. The pinned Zend/lang corpus moves from 4,107 to 4,108 passes,
an exact +1/-0 delta, with 1,195 failures, 115 skips, 181 unsupported cases and
zero timeouts or crashes. Two serial focused, array and main manifests and
summaries are respectively byte-identical. Four original E2E tests match PHP
8.5.9 across signatures, filtering, keys, warnings, handler exceptions,
references/COW, random subset invariants and error paths.

All five feature configurations, all-feature/all-target, formatting, PHPT
runner, the unchanged 1,623/289 unsafe ratchet, Composer S0, four Symfony S1
gates and PHP 8.5.9 S2/S3 pass. No opcode, frame, PHP value/object/array layout,
dependency or production unsafe change is made. CPU-pinned 32-pair balanced
release controls put median changes at -0.392% for empty requests, -15.653%
for key/value projection, -42.381% for count/flip, +3.572% for random
selection, +1.648% for loose comparison and +0.624% for the unchanged array
build/read control, below the +5% gate with exact output. An inlined generic
projection design was rejected after materially regressing string-key
projection; an outlined no-filter collector and direct private-storage value
projection preserve both semantics and the hot path.

Six focused binary-string/parser, missing-`opendir()` and case-insensitive
boolean-literal failures remain separate. Complete internal Reflection
type/default/return metadata, successful near-limit allocation, the remaining
array suite and broader compatibility are not claimed.

The `array-mutation-batch` checkpoint advances `array_push()`, `array_pop()`,
`array_shift()`, `array_unshift()` and `array_splice()` together and corrects
the first-negative-key append sequence. The admitted contract covers zero,
one and many variadic values; typed by-reference targets; return values;
integer reindexing and preserved string keys; nullable/extreme splice bounds;
scalar, array and object replacement; references and COW; cursor reset; safe
size and next-key errors; discarded-result destructors; reentrant mutation;
and live iterator-position translation for outer and nested by-reference
`foreach` loops. No php-src implementation or test is copied or translated.

The 59-case unmodified PHP 8.5 focused cluster moves from 22 to 50 passes, an
exact +28/-0 delta; PHP 8.5.9 passes all 59. The complete recursive 842-case
array audit moves from 496 to 525 passes with no lost pass, reducing failures
from 332 to 303 while 13 skips, one unsupported case and zero timeouts or
crashes remain. The pinned Zend/lang corpus moves from 4,103 to 4,107 passes,
an exact +4/-0 delta, with 1,196 failures, 115 skips, 181 unsupported cases and
zero timeouts or crashes. Two serial focused, array and main runs are
respectively byte-identical. Five original E2E tests and a 48-line clean-room
transcript match PHP 8.5.9 exactly.

All five feature configurations, all-feature/all-target, formatting, PHPT
runner, the unchanged 1,623/289 unsafe ratchet, Composer S0, four Symfony S1
gates and PHP 8.5.9 S2/S3 pass. The general internal positional-variadic ABI
keeps named and multi-value packing canonical; the existing four-byte call-plan
flag and a spare array-cursor metadata bit avoid opcode, frame and `PhpArray`
layout growth. CPU-pinned 32-pair release controls put paired median changes at
+1.300% for empty requests, +0.010% for ordinary append, +2.926% for
`array_push()`/`array_pop()`, -4.857% for `array_unshift()`/`array_shift()` and
-0.446% for `array_splice()`, below the +5% gate with exact output.

Nine focused leading-dot parser, temporary-write notice, escape/resource output
and restricted-`$GLOBALS` fatal-envelope failures remain separate. General
by-reference write-context enforcement, complete internal Reflection metadata,
successful near-limit allocation, the remaining array suite and broader
compatibility are not claimed.

The `array-construction-batch` checkpoint advances `range()`, `array_fill()`,
`array_combine()` and `array_merge()` together and corrects ordinary
`$array[]` next-key exhaustion. The admitted contract covers integer,
floating, numeric-string and byte-character ranges; optional signed steps;
finite, zero and safe-size validation; fill count/start overflow; canonical
combine keys; structural COW snapshots; live references; objects; variadic
merge validation; integer-key reindexing; preserved string keys; weak/strict
conversion; diagnostics, exceptions, signatures and named arguments. Packed
and scalar-key fast paths preserve the observable contract. The implementation
is independent and does not copy or translate php-src code.

The 52-case unmodified PHP 8.5 focused cluster moves from 14 to 43 passes and
from six timeouts to none, an exact +29/-0 delta; PHP 8.5.9 records 50 passes
and two 32-bit skips. The complete recursive 842-case array audit moves from
463 to 496 passes with no lost pass, reducing failures from 359 to 332 and
timeouts from six to none while 13 skips, one unsupported case and zero crashes
remain. Two serial focused manifests and two serial full-array manifests are
respectively byte-identical. The pinned Zend/lang corpus moves from 4,100 to
4,103 passes, an exact +3/-0 delta, with 1,200 failures, 115 skips, 181
unsupported cases and zero timeouts or crashes in two byte-identical final
manifests. A 53-line, 3,229-byte clean-room transcript and four original E2E
tests match PHP 8.5.9.

All five feature configurations, all-feature/all-target, formatting, PHPT
runner, the unchanged 1,620/289/332 unsafe ratchet, Composer S0, four Symfony
S1 gates and PHP 8.5.9 S2/S3 pass. No parser/compiler, opcode definition,
frame, Value/object layout, dependency or production unsafe change is made.
CPU-pinned 32-pair release controls put paired median changes at -0.824% for
100 empty requests, +0.008% for integer `range()`, +0.295% for
`array_fill()`, +3.300%/-21.252% for integer/string-key `array_combine()`,
+4.349% for `array_merge()` and +0.018% for append, below the +5% gate. The
new-only floating-step range smoke matches PHP output; its baseline rejects
the third argument, so no A/B ratio is claimed.

Seven focused parser, escape/resource output, inherited dump-order and earlier
arithmetic-prelude failures remain separate. Successful near-limit
multi-gibibyte allocation, complete reflected internal type/default/return
metadata, exhaustive multibyte character ranges, by-reference append overflow,
the remaining array suite and broader compatibility are not claimed.

The `array-structural-projection-batch` checkpoint advances `array_chunk()`,
`array_slice()`, `array_reverse()` and `array_pad()` together. The shared
projection layer covers each API's string- and integer-key policy, positive,
negative and extreme slice bounds, nullable length, left/right/no-op padding,
the PHP maximum pad length, packed and mixed arrays, structural snapshots,
live reference cells, nested copy-on-write values, weak/strict integer
conversion, diagnostics, exceptions, arity, parameter names and named
arguments. Packed-array fast paths preserve the contract while avoiding
per-entry generic key construction. The implementation is independent and
does not copy or translate php-src code.

The 55-case unmodified PHP 8.5 focused cluster moves from 6 to 50 passes and
from one timeout to none, an exact +44/-0 delta; PHP 8.5.9 passes all 55. The
complete recursive 842-case array audit moves from 418 to 463 passes with no
lost pass, reducing failures from 403 to 359 and inherited timeouts from seven
to six while 13 skips, one unsupported case and zero crashes remain. Two
serial focused manifests and two serial full-array manifests are respectively
byte-identical. The pinned Zend/lang corpus moves from 4,098 to 4,100 passes,
an exact +2/-0 delta, with 1,203 failures, 115 skips, 181 unsupported cases
and zero timeouts or crashes in two byte-identical final manifests.

All five feature configurations, all-feature/all-target, formatting, PHPT
runner, the unchanged 1,620/289/332 unsafe ratchet, Composer S0, four Symfony
S1 gates and PHP 8.5.9 S2/S3 pass. No compiler/opcode, frame, Value/object
layout, dependency or production unsafe change is made. CPU-pinned 32-pair
release controls put paired median changes at +0.308% for 100 empty requests,
-1.797% for `array_chunk()`, -0.087% for `array_slice()`, -66.441% for
`array_reverse()` and +1.506% for `array_pad()`, below the +5% gate.

Five focused literal/parser and later output/resource failures and six
inherited array-suite timeouts remain separate. Complete reflected internal
type/default/return metadata, successful near-limit allocation, mutating
`array_splice()`, the complete array suite and broader compatibility are not
claimed.

The `array-ordinary-sort-batch` checkpoint advances `sort()`, `rsort()`,
`asort()`, `arsort()`, `ksort()`, `krsort()` and `array_multisort()` together.
The shared PHP-aware comparison layer covers regular, numeric, string,
C-locale string and natural flags, stable ties, ascending/descending direction,
key preservation or reindexing, multi-column movement, structural snapshots,
live reference cells, numeric diagnostics, Stringable conversion, exceptions,
recursive comparisons and duplicate `array_multisort()` flag rejection. The
general value path uses an independently implemented stable bottom-up merge;
a guarded homogeneous-long path preserves the relevant performance gate. No
php-src implementation is copied or translated.

The 98-case unmodified `ext/standard/tests/array/sort` cluster moves from 40
to 78 passes, an exact +38/-0 release delta; PHP 8.5.9 records 97 passes and
one architecture skip. The complete recursive 842-case array audit moves from
378 to 418 passes with no lost pass, reducing failures from 443 to 403 while
13 skips, one unsupported case, seven inherited timeouts and zero crashes
remain. Two serial array manifests are byte-identical. An original 2,671-result
clean-room matrix, including 1,103-entry boundaries, is byte-identical to PHP
8.5.9 across 1,048,734 bytes. Because the supplying cases are outside the
pinned Zend/lang corpus, two byte-identical full manifests remain at 4,098
pass, 1,205 fail, 115 skip and 181 unsupported with zero timeouts or crashes
and exact +0/-0 movement from `0ed17346`.

All five feature configurations, all-feature/all-target, formatting, PHPT
runner, the unchanged 1,620/289/332 unsafe ratchet, Composer S0, four Symfony
S1 gates and PHP 8.5.9 S2/S3 pass. No compiler/opcode, frame, Value/object
layout, dependency or production unsafe change is made. CPU-pinned 32-pair
release controls put independent/paired median changes at +0.240%/+0.243% for
100 empty requests, -1.963%/-1.657% for one 300,000-entry integer `sort()`,
-6.619%/-6.954% for one 200,000-entry natural case-insensitive `ksort()` and
-0.595%/+0.272% for one 150,000-entry `array_multisort()`, below the +5% gate.

Nineteen focused failures remain separate: eight leading-dot numeric parser
forms, six double-quoted escape forms and five cases whose non-transitive mixed
values expose PHP's implementation-specific comparison schedule. The admitted
contract does not claim locale-specific collation, PHP's exact arbitrary
mixed-value comparator or magic-method call trace, partial permutation after a
throwing comparison, the seven inherited array-suite timeout paths, the
complete array suite or broader compatibility.

The `array-case-natural-batch` checkpoint adds `array_change_key_case()`, the
`key_exists()` alias, `strnatcasecmp()`, `natsort()` and `natcasesort()`, and
corrects the shared `strnatcmp()`/natural-sort comparison rule. The admitted
contract covers signatures, weak string/int conversion, ASCII key case,
collisions, mixed keys, stable key-preserving order, structural snapshots,
live reference cells, object string conversion, exceptions and direct-call
by-reference metadata. A clean-room matrix of 1,444 string pairs, 80 natural
sorts and 160 key transformations is byte-identical to PHP 8.5.9 with SHA-256
`3cb2f1f3310163f4d3a9069114ea54ed5cb714233695cd9ee7841dcc202062bd`.

The adjacent 39-case unmodified `ext/standard` cluster moves from 6 to 30
passes, an exact +24/-0 release delta; PHP 8.5.9 records 38 passes and one
architecture skip. The complete 842-case recursive array audit moves from 355
to 378 passes with no lost pass, reducing failures from 466 to 443 while the
same 13 skips, one unsupported case, seven inherited timeouts and zero crashes
remain. Because these supplying cases are outside the pinned Zend/lang corpus,
two byte-identical full manifests intentionally remain at 4,098 pass, 1,205
fail, 115 skip and 181 unsupported with zero timeouts or crashes and exact
+0/-0 pass-set movement from `291b9987`.

All five feature configurations, all-feature/all-target, formatting, PHPT
runner, the unchanged 1,620/289/332 unsafe ratchet, Composer S0, four Symfony
S1 gates and PHP 8.5.9 S2/S3 pass. The only compiler change is existing
direct-call reference metadata; no opcode, frame, value/object layout,
dependency or production unsafe change is made. CPU-pinned 32-pair release
controls put independent/paired median changes at +0.214%/+0.222% for 100
empty requests, -2.517%/-2.638% for 500,000 existing `strnatcmp()` calls and
+2.554%/+2.629% for 100,000 existing natural case-insensitive `asort()` calls,
below the +5% gate.

The remaining adjacent failures stay separate: four older
`array_key_exists()` edges, two missing string-escape forms, one leading-dot
numeric parser form and the ordinary `sort()` prelude in `natsort_basic.phpt`.
The stable O(n log n) sort does not claim PHP's exact arbitrary-large
comparator/`__toString()` trace or partial permutation after a thrown
comparison. The inherited array-suite timeouts, complete ordinary sorting,
remaining array-suite failures and broader compatibility remain work.

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

The `array-suite-zero-failure-batch` checkpoint closes all six ordinary
failures in the complete recursive 842-case `ext/standard/tests/array`
selection through shared object enumeration, unsigned formatting, lexer
precedence, callback trace, `ArrayObject` append and deprecated object-cursor
contracts. Six original regressions protect the general boundaries without
copying php-src source or recognizing a supplying path.

The supplying cases move from 0/6 to 6/6 against PHP 8.5.9. The array audit
moves from 822 to 828 pass (+6/-0), with zero fail, 13 skip and one unsupported;
strings moves from 305 to 309 pass (+4/-0); and Zend/lang moves from 4,196 to
4,201 pass (+5/-0), with no lost pass, timeout or crash. All five Cargo
configurations, all-target, formatting, runner, unsafe, Composer S0, four
Symfony S1 gates and PHP 8.5 S2/S3 pass. `PhpObject` remains 72 bytes and the
unsafe inventory remains 1,623 blocks/289 functions.

Two CPU-pinned 32-pair release controls for ordinary non-scalar-plan
`array_map()` callbacks put paired/order-balanced medians at
+4.286%/+3.564% and +3.146%/+3.314%, below the +5% gate with exact checksums.
The complete formatting surface, broader `ArrayObject`, deprecated object-
cursor mutation/reentrancy edges, 32-bit execution, the strings timeout and
remaining strings/Zend gaps remain explicit nonclaims.

The `htmlspecialchars-decode-decbin-binary-batch` checkpoint gives
`htmlspecialchars_decode()` PHP 8.5's quote/document flags, named and numeric
special-character references, single-pass behavior and byte-exact output.
`decbin()` covers AMD64 two's-complement boundaries and typed conversion;
`chr()` and `ord()` now round-trip explicitly materialized bytes while
retaining weak scalar `ord()` calls and PHP 8.5's `chr()` deprecations. Six
original E2E regressions cover the admitted flag, byte, typing, diagnostic and
integer boundaries without copying php-src source or tests.

The 11 supplying cases move from zero passes and ten failures to ten passes
plus one 32-bit-only skip, exactly matching PHP 8.5.9. The complete 733-case
strings audit moves from 296 to 305 passes, an exact +9/-0 delta with no other
status/category movement. Array remains byte-identical at 822/842 and
Zend/lang at 4,196/5,599, without a new timeout or crash.

All five Cargo configurations, all-target, formatting, runner, unsafe,
Composer S0, four Symfony S1 gates and PHP 8.5 S2/S3 pass. Two independent
CPU-pinned 32-pair release controls put paired medians at -1.871%/-1.912% for
startup, -6.365%/-6.491% for the string control, -0.899%/-0.667% for default
UTF-8 entity decode, -1.763%/-1.589% for `htmlspecialchars()`,
-1.911%/-1.349% for `htmlspecialchars_decode()` and -1.851%/-0.892% for
`ord()`, below the +5% gate with exact checksums. Candidate-only `decbin()`
reaches 6.905/6.830 million iterations per second; no unavailable-baseline
ratio is claimed. Source-literal binary provenance, remaining `ord()`
deprecations, 32-bit execution, other base conversion, full named entities,
malformed-UTF-8 replacement and the remaining corpus gaps stay explicit
nonclaims.

The `html-entity-numeric-legacy-encoding-batch` checkpoint gives
`html_entity_decode()` a byte-exact numeric-reference path for UTF-8 and the
PHP 8.5 single-byte encodings ISO-8859-1, ISO-8859-15, ISO-8859-5, CP866,
KOI8-R, MacRoman, Windows-1251 and Windows-1252. Public WHATWG indexes, the PHP
manual and PHP 8.5.9 black-box observations define the clean-room mappings,
aliases, document modes, quote flags, invalid forms and warning fallback.
Undefined slots and unrepresentable code points remain verbatim entities.

All eight supplying cases pass. The complete 733-case strings audit moves from
286 to 296 passes, an exact +10/-0 delta with no other status/category movement;
the two adjacent gains are `html_entity_decode2.phpt` and
`html_entity_decode3.phpt`. Array remains byte-identical at 822/842 and
Zend/lang at 4,196/5,599, without a new timeout or crash. Four mapping-level
unit tests cover all ASCII, defined and undefined table slots; six original
E2E tests cover the admitted parser, flag, invalid-input and binary-output
boundaries.

All five Cargo configurations, all-target, formatting, runner, unsafe,
Composer S0, four Symfony S1 gates and PHP 8.5 S2/S3 pass. Two independent
CPU-pinned 32-pair release controls put paired medians at +0.669%/+0.816% for
startup, -0.265%/-0.191% for the string control, -8.869%/-8.970% for default
UTF-8 entity decode and -0.353%/+0.269% for the `htmlspecialchars()` control,
below the +5% gate with exact checksums. Candidate-only legacy decode reaches
1.685/1.669 million iterations per second; no unavailable-baseline ratio is
claimed. Full named-entity tables, multi-code-point entities,
`ENT_DISALLOWED`, malformed-UTF-8 replacement, optional encoding extensions
and the remaining corpus gaps stay explicit nonclaims.

The `binary-pack-unpack-format-contract-batch` checkpoint adds the complete
core PHP 8.5 `pack()`/`unpack()` format alphabet, repeat and naming grammar,
cursor/offset behavior, integer and float boundary semantics, diagnostics,
typing and Reflection signatures. A spare provenance bit preserves packed
bytes through `strlen()`, `bin2hex()`, `unpack()` and direct output without
growing the 16-byte value layout. Eight original E2E tests include byte-exact
raw output and all admitted format boundaries; the 11 focused upstream cases
pass 9/9 with two expected 32-bit platform skips.

The complete array audit moves from 821 to 822 pass (+1/-0), and the 733-case
strings audit moves from 267 to 286 pass (+19/-0). Both repeated manifests are
byte-identical with no lost pass or other category movement. Two byte-identical
Zend/lang manifests remain at 4,196 pass, 1,107 fail, 115 skip and 181
unsupported, with no XFAIL, timeout or crash. The strings corpus retains its
pre-existing `dirname_multi.phpt` timeout outside this feature boundary.

All five Cargo configurations, all-target, formatting, runner, unsafe,
Composer S0, four Symfony S1 gates and PHP 8.5 S2/S3 pass. Two independent
CPU-pinned 32-pair release controls put paired medians at +3.997%/+4.044% for
startup, -6.546%/-6.693% for scalar work, +1.580%/+1.348% for the unchanged
repository string workload and -1.665%/-1.563% for scalar `sprintf()`, below
the +5% gate with exact checksums. Candidate-only pack/unpack throughput is
1.487/1.445 million iterations per second; no ratio is claimed against the
preceding binary. Complete binary propagation through unrelated legacy string
APIs, optional-extension dependants, six remaining array failures, 362 strings
failures, 30 strings unsupported cases, the retained timeout and broader
compatibility remain explicit nonclaims.

The `mixed-internal-sort-zend-schedule-batch` checkpoint gives internal
`sort()`/`rsort()`/`asort()`/`arsort()`/`ksort()`/`krsort()` and
`array_multisort()` one stable PHP 8.5 observation-derived comparison
scheduler. It preserves original positions for comparator equality and routes
heterogeneous, warning/hook/NaN and non-transitive domains through the exact
observable schedule. A general scalar-total proof keeps uniformly numeric or
non-numeric regular inputs, pure numeric casts, string/natural inputs and pure
multi-column sorts on guarded stable host paths. No fixture, class, test,
benchmark or precomputed permutation is recognized.

Four original regressions cover pairwise mixed comparison, non-transitive
string cycles, duplicate stability, both directions, key policy, lengths
5/6/10/16/17/22, numeric-warning lengths through 17, pivot boundaries
1,023/1,024 and exact array/object numeric/string diagnostics. PHP 8.5.9 and
RPHP pass all five focused PHPTs. The complete 842-case array audit moves from
816 to 821 pass, an exact +5/-0 delta with seven failures, 13 skips, one
unsupported case and no other status/category movement. Two candidate array
manifests are byte-identical. Two byte-identical 5,599-case Zend/lang manifests
remain at 4,196 pass, 1,107 fail, 115 skip and 181 unsupported. Neither corpus
has an XFAIL, timeout, crash or lost pass.

All five Cargo configurations, all-target, formatting, runner, unsafe,
Composer S0, four Symfony S1 gates and PHP 8.5 S2/S3 pass. Two independent
CPU-pinned 32-pair release controls put paired medians respectively at
+0.027%/-0.327% for startup, -0.091%/-0.019% for scalar Long,
-15.499%/-17.841% for numeric, -7.592%/-12.691% for string,
-76.581%/-76.322% for mixed regular and -17.970%/-16.348% for multi-column
`array_multisort()`, below the +5% gate with exact checksums. Throwing-
comparison partial permutation, seven non-sort array failures and broader
compatibility remain explicit nonclaims.

The `debug-zval-refcount-renderer-batch` checkpoint separates
`debug_zval_dump()` from the public `var_dump()` renderer and derives cold
diagnostic ownership from PHP-visible frame, global, static, dynamic,
constant, property and container roots. Compiler-interned strings and
immutable array literals use two type-specific spare `Value` bits; first COW
transfers literal-source ownership to direct refcounted children without
growing the 16-byte value layout. Three original E2E regressions cover dynamic
and interned strings, array/object aliases and references, enum-case lifetime,
and nested literal children across repeated separation.

All five focused PHPTs pass. The complete 842-case array audit moves from 814
to 816 pass (+2/-0), with 12 failures, 13 skips and one unsupported case. The
5,599-case Zend/lang audit moves from 4,194 to 4,196 pass (+2/-0), with 1,107
failures, 115 skips and 181 unsupported. Both repeated manifests are
byte-identical and neither corpus has an XFAIL, timeout, crash, lost pass or
other category movement. All five Cargo configurations, all-target,
formatting, runner, unsafe, Composer S0, four Symfony S1 gates and PHP 8.5
S2/S3 pass.

Because first literal separation and ordinary array mutation share a branch,
two independent CPU-pinned 32-pair release controls cover startup, three
million ordinary appends and 250,000 nested literal COW cycles. Their paired
medians are respectively -5.463%/-4.924%, +1.301%/+1.024% and
+2.813%/+1.736%, below the +5% gate with exact checksums. Literal-COW p90 is
noisy at +8.532%/+8.417% and remains disclosed rather than promoted to a gate.
`arginfo`/`chunk_split()` prerequisite cases, `bug60825.phpt` and
unrepresentable Zend ownership states remain explicit nonclaims.

The `array-user-sort-small-schedule-batch` checkpoint gives canonical
`usort()`, `uasort()` and `uksort()` callbacks PHP 8.5's stable observable
two-to-five-element comparison schedule and stops it immediately after a
callback exception. The guarded scalar-Long path and six-or-more-element
canonical insertion schedule remain unchanged. Release reproduction also
localized an independent ordinary-object `print_r()` gap, so the shared cold
renderer now emits inherited visibility-qualified and dynamic properties,
nested values and recursion markers without recognizing the supplying class.

The supplying PHP 8.5 case passes 1/1. The complete recursive 842-case array
audit moves from 813 to 814 passes, an exact +1/-0 delta, with 14 failures, 13
skips, one unsupported case and no XFAIL, timeout or crash. Two final array
manifests are byte-identical. Two byte-identical Zend/lang manifests move from
4,187 to 4,194 pass, an exact +7/-0 delta, with 1,109 failures, 115 skips and
181 unsupported and no XFAIL, timeout or crash. All five feature
configurations, all-feature/all-target, formatting, PHPT-runner, unsafe,
Composer S0, four Symfony S1 gates and PHP 8.5 S2/S3 pass. CPU-pinned 32-pair
release controls put accepted paired medians at -1.951% for scalar-Long
`usort()` and +0.424% for the impure canonical callback, below the +5% gate.
Custom `__debugInfo()`/lazy/property-hook projections, throwing-comparison
partial permutation and five mixed-value sort schedule cases remain nonclaims.

The `recursive-array-boolean-diagnostic-batch` checkpoint makes the shared
rejected-argument path for `array_merge_recursive()` and
`array_replace_recursive()` render boolean values as PHP 8.5's `true given`
and `false given`, while retaining every other type name, argument number and
the two APIs' distinct first-parameter naming contract. An original E2E matrix
covers both boolean values at fixed/variadic positions and successful calls
after each rejected series. The change is local to an existing cold type-error
helper; successful combiners, startup and runtime layouts are unchanged, so no
runtime performance lane applies.

The two supplying PHP 8.5 cases pass 2/2. The complete recursive 842-case array
audit moves from 811 to 813 passes, an exact +2/-0 delta, with 15 failures, 13
skips, one unsupported case and no XFAIL, timeout or crash. Two final array
manifests are byte-identical. Two byte-identical Zend/lang manifests remain at
4,187 pass, 1,116 fail, 115 skip and 181 unsupported, with no status movement,
timeout or crash. All five feature configurations, all-feature/all-target,
formatting, PHPT-runner, unsafe, Composer S0, four Symfony S1 gates and PHP 8.5
S2/S3 pass.

The `array-multisort-argument-contract-batch` checkpoint makes fixed and
variadic `array_multisort()` arguments share a stateful PHP 8.5 classifier for
columns, direction/comparison flags, duplicate valid flags, invalid integer
flags and other types. It also confines PHP 8.5's observable stable small-sort
comparison schedule to two-to-five-row `array_multisort()` inputs, preserving
outer state across nested diagnostics while leaving the six-or-more-row merge
path unchanged. Two original E2E regressions cover validation order, aliases
and reentrancy, and the published 150,000-row two-column benchmark exercises
the unchanged large-input path.

The four supplying unmodified PHP 8.5 cases pass 4/4. The complete recursive
842-case array audit moves from 807 to 811 passes, an exact +4/-0 delta, with
17 failures, 13 skips, one unsupported case and no XFAIL, timeout or crash.
Two final array manifests are byte-identical. Two byte-identical Zend/lang
manifests remain at 4,187 pass, 1,116 fail, 115 skip and 181 unsupported, with
no status movement, timeout or crash. All five feature configurations,
all-feature/all-target, formatting, PHPT-runner, unsafe, Composer S0, four
Symfony S1 gates and PHP 8.5 S2/S3 pass. CPU-pinned 32-pair release controls
put paired medians at -0.251% for startup and +0.195% for multi-column
`array_multisort()`, below the +5% gate. The five non-transitive mixed-sort
schedule cases and throwing-comparison partial permutation remain explicit
nonclaims.

The current retained AMD64 PHP 8.5 selection baseline has no array-suite
process hazard or ordinary array failure: `ext/standard/tests/array` is 828
pass, zero fail, 13 skip and one unsupported. The 733-case strings selection
is 343 pass, 305 fail, 54 skip and 30 unsupported, with one retained timeout;
the complete `stripos()`/`strrpos()`/`strripos()` cluster is 38/38. `Zend/tests`
plus `tests/lang` is 4,201 pass, 1,102 fail, 115 skip and 181 unsupported, with
no timeout or crash. The next goal should take the highest-yield general root-
cause cluster from the remaining strings or Zend/lang manifests rather than
extending the already-zero array failure count or the closed position-search
family.

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
