# Compatibility status

RPHP implements a growing, tested subset of PHP. Its public dependency-platform
identity is PHP 8.5.0; some experimental language behavior remains available as an
experimental RPHP extension, but it is outside that compatibility contract.
RPHP is not certified for a complete PHP version and must not be treated as a
drop-in PHP replacement. Passing a script is evidence only for the exercised
behavior.

The latest measured AMD64 PHP 8.5 contract checkpoint is pinned to php-src
8.5.6 commit `fcc29c8`. Across all 5,599 unmodified `Zend/tests` and
`tests/lang` cases, 3,003 pass, 2,294 fail, 114 skip, one is an upstream XFAIL,
187 are unsupported, and none time out or crash. The headline pass rate is
56.692%; 4,619 of 5,297 attempted cases reach runtime (87.200%). Relative to
the preceding 3,002-pass checkpoint, the exact pass-set delta is +1/-0. The
initial PHP 8.5 corpus continues to have no process hazard.

PHP 8.5's internal string-backed `PropertyHookType` enum now exposes canonical
`Get`/`Set` cases, readonly `name`/`value`, `BackedEnum`/`UnitEnum` identity and
the `cases()`, `from()` and `tryFrom()` contracts. `ReflectionProperty` exposes
only explicitly declared property hooks through `getHook()`, `getHooks()` and
`hasHook()`; returned `ReflectionMethod` objects retain hook attributes,
visibility, parameters, declaring class and the implicit getter/setter return
signature. The shared method renderer also makes those reflected hooks
stringable with PHP's source and signature layout.

The exact PHP 8.5.6 delta is +1/-0:
`attributes/delayed_target_validation/validator_NoDiscard.phpt` advances from
an undefined-method runtime failure to an exact pass. Two directly related
cases now reach their later output comparison, and two adjacent pre-existing
ReflectionMethod string-conversion failures also advance from runtime to
output; no pass is lost and no timeout or crash appears. Full
`__PROPERTY__` constant-expression support, exact internal-enum object-handle
numbering and broader Reflection string formatting remain explicit follow-up
work. All five Cargo configurations, all-feature/all-target, formatting and
the exact unsafe ratchet pass. Composer S0, all four Symfony S1 gates,
warmed-kernel S2 and cold-build S3 also pass on AMD64.

The exact base `68f190c5` and release candidate were compared without removing
outliers. A 63-pair ABBA/BAAB confirmation over 150 empty-output file requests
per executable measured baseline p10/median/p90
0.200333/0.204751/0.208571 seconds and candidate
0.201749/0.204956/0.209275 seconds: +0.100% by independent medians and +0.266%
by the paired-ratio median, whose p10/p90 is -2.246%/+2.732%. A separate
31-pair `bench_calls.php` control retained checksum `37500007500000` and
measured baseline 0.357227/0.362015/0.370270 seconds versus candidate
0.359827/0.365124/0.372064 seconds: +0.859% independently and +0.665% paired,
with paired p10/p90 -1.907%/+2.650%. The compatibility-first registration and
cold Reflection expansion remain below the five-percent median regression
ceiling; recovery of the fixed startup cost is deferred to the later combined
performance pass.

PHP 8.5's final internal `NoDiscard` attribute now exposes its nullable
`message` constructor contract and public protected(set) readonly property.
Unused direct, static, magic, closure and trait-method results emit PHP's
`E_USER_WARNING` diagnostic before the body runs; assigned results and an
explicit `(void)` discard remain silent. `call_user_func()` and
`call_user_func_array()` propagate that source-level discard state through
their detached callback frame, while `Deprecated` continues to diagnose first.
Throwing handlers, strict and weak attribute-argument validation, repeated
attributes, unsupported lifecycle methods, property hooks and `void`/`never`
returns retain PHP's error ordering and source locations. The six pinned PHP
8.5 `(void)` cases now pass, including destruction timing, assertion AST text,
for-list placement and the invalid assignment/final-condition boundaries.

The exact additions comprise 19 cases in `attributes/nodiscard`, two delayed-
target-validation interactions and six void-cast cases. The NoDiscard
directory is 20/25: its remaining ordinary failure needs the unimplemented
`DateTimeImmutable` class, two `zend_test` execute-hook cases remain explicitly
unsupported and two native-extension cases skip. The two broader delayed-
validation failures still needed `ReflectionProperty::getHook()` at that
checkpoint; arbitrary nested `(void)` parse diagnostics are not claimed by
this slice. All five Cargo
configurations, all-feature/all-target, formatting and the exact unsafe ratchet
pass. Composer S0, all four Symfony S1 gates, warmed-kernel S2 and cold-build S3
also pass against PHP 8.5 on AMD64.

The exact base `3610743c` and final release candidate were compared on one
physical Ryzen 9 7950X core with no outlier removal. Sixty-three balanced
ABBA/BAAB pairs of 150 empty-output file requests measured baseline
p10/median/p90 0.191630/0.193140/0.194568 seconds and candidate
0.191056/0.192258/0.194409 seconds: -0.457% by independent medians and -0.422%
by the paired-ratio median, whose p10/p90 is -1.082%/+0.451%. A separate
63-pair `bench_calls.php` control retained computed result `37500007500000` and
measured baseline 0.365947/0.370798/0.385319 seconds versus candidate
0.364929/0.368054/0.385935 seconds: -0.740% independently and -0.337% paired,
with paired p10/p90 -3.919%/+2.204%. Both common-path controls remain below the
five-percent median regression ceiling.

The preceding `random-interval-boundary` checkpoint added PHP 8.5's internal
unit enum `Random\IntervalBoundary` with the four
canonical cases `ClosedOpen`, `ClosedClosed`, `OpenClosed` and `OpenOpen` with
stable identity, readonly `name`, `UnitEnum` membership and ordered `cases()`.
Reflection reports the enum as internal and non-final, while the class linker
still rejects every attempt to extend an enum. Throwable and debug traces now
render enum arguments symbolically as `Class::Case`, matching PHP rather than
the ordinary object placeholder. These contracts turn
`Zend/tests/attributes/deprecated/type_validation_004.phpt` and
`Zend/tests/enum/enum_in_stack_trace.phpt` into exact passes with no lost pass,
so the complete PHP 8.5 `Deprecated` directory is now 47/47. At that
checkpoint, `NoDiscard` remained an independent missing built-in attribute;
`Randomizer::getFloat()` is still not claimed. All five Cargo configurations,
all-feature/all-target, formatting and the exact unsafe ratchet pass, as do
Composer S0, all four Symfony S1 gates and warmed-kernel S2. On AMD64, an
independent 63-pair balanced ABBA/BAAB release confirmation over 150 ordinary
file requests per executable and pair retains identical empty output. The
candidate is +0.588% by independent medians and +0.898% by the paired-ratio
median; both order medians remain below +1%. The no-outlier paired p10/p90
spread is -3.921%/+6.646%, while the independent candidate-versus-control p90
is +0.904%; the median regression gate passes and the noisy upper paired tail
is retained explicitly. The added common-path work is fixed startup
registration whose registry capacity is reserved and regression-tested; enum
trace formatting remains a cold path.

PHP 8.5's final internal `Deprecated` attribute exposes its nullable
`message` and `since` constructor contract and public protected(set) readonly
properties without declaration defaults. Applying it to a function, method,
closure, constructor, destructor or magic-call implementation emits the
callable-specific `E_USER_DEPRECATED` diagnostic before argument validation;
runtime constants, strict coercion, requested magic names, error handlers and
`@` suppression share the ordinary attribute and diagnostic paths. Class,
interface and enum applications stop at PHP's compile-time validation, while
direct construction and constructorless Reflection initialization preserve the
built-in readonly lifecycle. Global constants, class constants and enum cases
now diagnose direct, dynamic and dependency-expression reads in PHP order,
including deferred values, recursive messages and throwing handlers. Direct
deprecated-trait composition reports at the declaration's runtime position;
nested composition, case-insensitive lookup and `insteadof` exclusions share
the ordinary trait linker, while direct trait-constant access remains illegal
and `defined()` stays silent. Engine-dispatched uncaught-exception handlers now
run before main-scope destructors, observe themselves as unregistered, retain
PHP's synthetic `Unknown:0` diagnostic origin and `[internal function]` trace
frame, and preserve replacement exceptions thrown by the handler. Error and
exception handler setters validate callbacks before mutating their stacks.
`ReflectionMethod($closure, '__invoke')` keeps its public `Closure` identity
while exposing the attributes and deprecation status carried through
`Closure::fromCallable()`. The complete 47-case PHP 8.5 `Deprecated` attribute
directory has 47 exact passes, and the standard exception-handler directory
is 10/10. The callable-flow corpus gain was 18 exact passes without losing one.
All five Cargo configurations, all-feature/all-target, formatting and the exact
unsafe ratchet pass, as do Composer S0, all four Symfony S1 gates and
warmed-kernel S2. On
AMD64, 31 balanced ABBA/BAAB release pairs over ordinary file requests retain
identical output and place the candidate at +1.837% by independent medians and
+1.863% by the paired-ratio median; paired p10/p90 is -1.174%/+4.257%, below
the five-percent regression ceiling. The added common-path work is one
request-end handler check; callback resolution, synthetic frames and metadata
formatting remain cold paths.

CLI `-d precision=...`, `ini_get()` and mutable `ini_set()` now share a
request-local significant-digit setting. Float conversion uses PHP 8.5 fixed
versus scientific cutoffs, rounding and uppercase exponent spelling for echo,
casts, concatenation, `print_r()`, string-oriented formatting and collection
joins; double concatenation remains a runtime operation because startup
precision cannot be folded at compile time. `var_dump()` independently uses
round-trip binary64 rendering, and enormous valid precision values are bounded
by the finite exact binary64 expansion instead of allocating from the raw INI
value. Admitting all 18 precision-only PHPTs yields eight passes, four platform
skips and six honest failures in unrelated parser/runtime gaps. Correcting the
shared default and `var_dump()` format turns another eleven existing failures
into passes, with no lost pass, timeout or crash. All five Cargo configurations,
all-feature/all-target, formatting and unsafe gates pass, as do Composer S0,
all four Symfony S1 gates, warmed-kernel S2 and cold-build S3 against PHP 8.5.
On AMD64, 20 alternating release pairs over one million common exact
float-to-string conversions retain the same 5,500,000 checksum and place the
candidate/control ratio at 1.012 median, 0.990 p10 and 1.044 p90, below the
five-percent regression ceiling.

Startup `zend.exception_ignore_args` and
`zend.exception_string_param_max_len` now control both immutable Throwable
traces and live diagnostic rendering. Throwable strings, uncaught chains,
`debug_print_backtrace()` and unmatched `match` diagnostics share the selected
byte limit and escaping contract; zero reports string match subjects by type,
while ignore-args reports every unmatched subject by type and omits stored call
arguments. Boolean INI values accept canonical names and numeric truth values
without treating arbitrary text as true. Three previously unsupported cases
and two existing output failures become exact passes, with no lost pass, moved
remaining failure stage, timeout or crash. All five Cargo configurations,
all-feature/all-target, formatting and unsafe gates pass, as do Composer S0,
all four Symfony S1 gates, warmed-kernel S2 and cold-build S3 against PHP 8.5.

Repeated CLI `-d error_reporting=...` definitions now initialize the same
request-local diagnostic mask exposed by `error_reporting()` and `ini_get()`.
Numeric, boolean, named-constant and bitwise INI values use the PHP 8.5 INI
integer grammar, including its 30,719 `E_ALL` value; the last definition wins.
The PHPT runner therefore attempts 32 cases that were previously classified as
unsupported: 14 now pass, while the remaining 18 honestly expose independent
compile, runtime or output gaps. No previous pass is lost and no existing
attempted case changes failure stage. Expanding the attempted denominator by
32 lowers the headline rate from 54.573% to 54.508% despite the 14 new passes.
All five Cargo configurations, all-feature/all-target, formatting and unsafe
gates pass, as do Composer S0, all four Symfony S1 gates, warmed-kernel S2 and
cold-build S3 against PHP 8.5.

Attribute construction now retains its logical call chain while the physical
callback frame remains detached from the interpreter's return protocol.
`debug_backtrace()`, `debug_print_backtrace()` and Throwable snapshots expose
the pending attribute constructor at the attribute use-site followed by the
live `ReflectionAttribute->newInstance()` frame, including PHP's canonical
method spelling, object/argument projections and key order. A strict argument
failure snapshots that same pending constructor before its body runs while
keeping the constructor declaration as the TypeError origin. Sparse
request-local sidecars carry only the exceptional logical caller, source and
canonical internal name, so ordinary `ExecuteData` and callback traces remain
unchanged. This adds eight exact PHP 8.5.6 passes with no lost pass, moved
failure stage, timeout or crash. All five Cargo configurations, the
all-target/all-features and exact unsafe-policy gates pass, as do Composer S0,
all four Symfony S1 gates, warmed-kernel S2 and cold-build S3 against PHP 8.5.
On AMD64, 1,003 alternating release pairs place the directly affected grouped
regex callback at +0.83% and retained callback at +0.19%; all adjacent regex
controls remain below the five-percent regression ceiling.

One-use object receivers now reach their Zend lifetime boundary immediately
after `DoFcall`, before a fluent expression continues or a returned temporary
receives its next handle number. The compiler emits an exact one-slot temporary
release only for TMP receivers; ordinary CV method calls retain their existing
bytecode and hot dispatch. A destructor exception raised at that boundary now
re-enters the shared throw path instead of remaining pending, while the compact
object-array planner consumes only the matching release of a virtualized
property receiver. This adds eight exact PHP 8.5.6 passes with no lost pass,
moved failure stage, timeout or crash. All five Cargo configurations,
all-target/all-features and unsafe gates pass, as do Composer S0, all four
Symfony S1 gates, warmed-kernel S2 and cold-build S3. Nine alternating AMD64
release runs keep the ordinary CV method-call median within noise (0.794 to
0.784 milliseconds per 10,000 calls). The intentionally affected ephemeral
receiver workload moves from 1.960 to 2.131 seconds per million calls (+8.8%);
that localized compatibility-first cost remains explicit performance debt for
the later whole-runtime optimization pass.

Attribute arguments that depend on runtime constants, autoloaded classes or
class constants now retain their constant-expression AST and lexical namespace,
import, class, parent and source-file scope for cold evaluation by
`ReflectionAttribute::getArguments()` and `newInstance()`. Trait method and
bound Closure reflection rebind that scope to the effective consumer/called
class, and anonymous classes expose the same public name to reflection and
`self::class`. This adds four exact PHP 8.5.6 passes with no lost pass, moved
failure stage, timeout or crash. ReflectionParameter scope is kept in a weak
request-local sidecar so its observable object shape and ordinary objects stay
unchanged. Adding the missing `ReflectionParameter::__toString()` surface also
removes the pre-existing Symfony cold-container failure; the complete
FrameworkBundle 7.4.16 S3 gate now passes against PHP 8.5.9. All five Cargo
configurations, all-target/all-features and unsafe gates pass. The added work is
confined to cold compilation and explicit Reflection paths, so no ordinary
execution-path benchmark applies.

Replacing the final CV handle to an object now commits the opcode-specific
variable or reference state and invokes `__destruct()` at PHP's observable
boundary. This covers ordinary assignment, reference rebinding, `foreach` and
function-static binding, including throwing destructors and re-entrant
`$GLOBALS` writes. Direct object RHS temporaries transfer ownership into their
destination without changing reference-returning expression semantics, and
dirty-global synchronization writes through a referenced CV without treating
its external cell as frame storage. This adds six exact PHP 8.5.6 passes with
no lost pass or moved failure stage. All five Cargo configurations,
all-target/all-features and unsafe gates pass. Alternating AMD64 release runs
showed unchanged medians for ten million scalar assignments (0.05 seconds) and
five million object assignments (0.93 seconds).

Instance and static property replacement now commit the new value or reference
cell before invoking the final old value's destructor, so re-entrant writes see
PHP 8.5's assignment state. A standalone property assignment transfers an
unobserved TMP/VAR owner into storage instead of retaining a compiler-only
object alias; reference-returning RHS values remain dereferenced ordinary
assignments. This closes all ten previously failing `Zend/tests/gh10168`
property cases with no lost pass, timeout, crash or moved failure stage. The
property E2E suite passes in all five Cargo feature configurations, as do the
all-target/all-features compile and unsafe-policy gates. Nine alternating AMD64
release runs retain identical medians for ten million scalar property writes
(0.19 seconds) and 500,000 object replacements (0.08 seconds). Generator
finalization and request-shutdown destruction remain separate lifecycle work.

Each anonymous Closure object now owns its function-static cells. Repeated
creation of the same declaration produces independent cells, ordinary aliases
of one Closure share them, and `bindTo()` snapshots their current values.
Closure generators retain the same cells across creation and suspension, and
detached callback paths preserve the ownership contract. This adds the exact
`Zend/tests/bug64979.phpt` pass without losing a prior pass or moving another
failure stage. All five Cargo configurations, all-target/all-features and
unsafe gates pass. Static-free closures allocate no additional storage; seven
alternating release runs showed unchanged medians for five million ordinary
closure calls (0.33 seconds) and two million ordinary closure creations (0.11
seconds).

Function-static storage in trait methods is now owned by each composed method:
different consuming classes and aliases no longer share one trait declaration's
cell. Only static-bearing trait methods are cloned during cold class
registration; ordinary trait methods retain their shared compiled body. A
function-local `static` declaration can also rebind a CV previously bound by
`global` without copying the static value back over the global at return. This
adds four exact PHP 8.5.6 passes without losing a prior pass or moving another
failure stage. All five Cargo configurations, all-target/all-features and unsafe
gates pass. The trait work is confined to class registration and the global
collision adds work only to functions declaring both bindings, so no ordinary
call-path performance benchmark applies.

Static-variable initializers now execute lazily and at most once after a
successful commit. An explicit in-progress cell preserves PHP 8.5's recursive
initializer behavior and leaves a throwing initializer retryable, while an
initialized cell skips the complete expression on later calls. Duplicate
static declarations and collisions with explicit closure captures fail during
compilation at the declaration line; standalone anonymous-function expression
statements reach that validation. This adds five exact passes without losing a
prior pass: three `Zend/tests/static_variables` cases and both
`tests/lang/static_basic_*` cases. Runtime reach falls by one case because the
newly passing closure-capture collision now stops at its required compile
stage. All five Cargo configurations and the all-target/all-features check
pass. Static-bearing functions are already excluded from optimized call/JIT
plans, and the added branch replaces repeated initializer execution rather
than affecting ordinary calls, so no runtime benchmark applies.

Closure debug metadata now exposes compile-time-known static-variable defaults
before the first invocation and switches to the request-owned runtime cells
after execution. Dynamic initializers remain `NULL` until their first runtime
evaluation, matching PHP 8.5's lazy boundary. This adds the exact
`Zend/tests/closures/closure_const_expr/static_variable.phpt` pass without
losing a prior pass or changing runtime reach. All five Cargo test
configurations pass. The new constant evaluation is confined to cold
compilation and the metadata is read only by explicit closure `var_dump()`, so
ordinary calls and generated code are unchanged and no runtime benchmark
applies.

Anonymous Closure dumps now expose PHP 8.5's source-qualified `name`, `file`
and declaration `line` fields before captures, receiver and parameter metadata.
Named functions, methods and internal first-class callables retain their
distinct `function` field, and closure identity, capture/reference state and
recursion tracking are unchanged. This adds eighteen exact passes without
losing a prior pass or moving a remaining failure stage. All five Cargo
configurations, all-target, all-features check, unsafe, Composer S0, all four
Symfony S1 gates and warmed-kernel S2 pass. The added work runs only during an
explicit closure `var_dump()` and reads existing immutable source metadata, so
no runtime performance benchmark applies.

Parameters before the final parameter without a default now follow PHP 8.5's
required-arity contract, including named calls, while declaration deprecations
identify each affected optional parameter and coexist with later compile or
class-link fatals. Typed literal defaults are validated during compilation,
integer defaults widen to `float`, symbolic constant and `new` defaults retain
their deferred runtime behavior, and built-in type names normalize
case-insensitively. Parent-method compatibility errors also recover the child
method's declaration line from existing cold source metadata. This adds
seventeen exact passes without losing a prior pass. One closure case advances
to its independent incomplete debug-dump metadata. All five Cargo
configurations, all-target, all-features check, unsafe, Composer S0, all four
Symfony S1 gates and warmed-kernel S2 pass. The changes are confined to cold
compilation/linking and deprecated/defaulted signatures; ordinary required
calls and generated code are unchanged, so no runtime performance benchmark
applies.

Legacy parameter declarations spelled `T $value = null` now retain PHP 8.5's
implicitly nullable callable contract while emitting the declaration-time
deprecation with the canonical function, method or closure name and source
line. The same diagnostics are emitted for included compilation units, and
nullable intersection diagnostics use `(A&B)|null`. This adds nine exact
passes without losing a prior pass. All five Cargo configurations, all-target,
all-features check,
unsafe, Composer S0, all four Symfony S1 gates and warmed-kernel S2 pass. The
new work is confined to cold declaration compilation and deprecated signatures;
ordinary explicitly typed call dispatch and generated code are unchanged, so
no runtime performance benchmark applies.

Standalone `null`, `true` and `false` are now recognized as declaration-type
starts in parameters, returns, properties, nullable forms and unions, including
null-led DNF types. Union diagnostics retain parentheses around intersection
arms, standalone `null` keeps its canonical property spelling, and failed
literal-boolean property writes report `true` or `false` rather than `bool`.
This adds fifteen exact passes without losing a prior pass; two implicit
nullability cases now reach runtime and remain a separate deprecation-contract
checkpoint. All five Cargo configurations, all-target, all-features check,
unsafe, Composer S0, all four Symfony S1 gates and warmed-kernel S2 pass. The
runtime changes are confined to mismatch diagnostic formatting, so successful
property writes and generated code are unchanged and no performance benchmark
applies.

Declaration validation now requires `mixed`, `void` and `never` to remain
standalone in nullable and union types, with PHP 8.5's type-specific diagnostic
ordering. Properties reject `void`, `never` and every callable shape before
RPHP creates its internal accessors, so ordinary, nullable and promoted
properties retain PHP's property-qualified fatal. This adds fifteen exact
passes without losing a prior pass. All five Cargo configurations, all-target,
all-features check, unsafe, Composer S0, all four Symfony S1 gates and
warmed-kernel S2 pass. The change is confined to cold declaration validation;
successful declarations, runtime bytecode and generated code are unchanged, so
no runtime performance benchmark applies.

Declared unions, intersections and DNF types now use one compile-time semantic
normalization for case-insensitive names, namespace imports, `self`, `parent`,
literal booleans, `iterable` expansion, `object` coverage and intersection-set
subsumption. Redundant members fail with PHP 8.5's canonical ordering and
spelling, including `Traversable|array`; valid nonredundant DNF declarations
remain executable. This adds 39 exact passes without losing a prior pass: all
32 cases in the three redundant-type directories plus seven adjacent iterable
alias cases. Runtime reach decreases from 88.114% to 87.366% because the newly
passing negative tests now stop at their correct compile stage. All five Cargo
configurations, all-target, all-features check, unsafe, Composer S0, all four
Symfony S1 gates and warmed-kernel S2 pass. Successful declarations produce the
same bytecode and runtime checks, so no runtime performance benchmark applies.

Intersection declarations now reject scalar, literal, array, callable, mixed,
never, null, object, iterable and static members during compilation, including
inside DNF types. Diagnostics use PHP 8.5's canonical type spelling, notably
`Traversable|array` for `iterable`, and retain source context across functions,
closures, methods, parameters and properties. This adds all fifteen non-nullable
invalid-member PHPTs with no lost pass; the separate nullable-intersection parse
case remains visible. Runtime reach decreases from 88.402% to 88.114% because
the newly passing negative tests now stop at their correct compile stage. All
five Cargo configurations, all-target, all-features check, unsafe, Composer S0,
all four Symfony S1 gates and warmed-kernel S2 pass. Valid declaration bytecode,
runtime type checks and generated code are unchanged, so no runtime performance
benchmark applies.

Enums now retain their declaration line through parsing and cold class-link
metadata, so traits that contribute properties or forbidden magic methods fail
with PHP 8.5's enum source location. The same validation covers nested trait
composition and aliases that introduce a forbidden magic name, while invocation
magic methods remain permitted. This adds the three `traits-no-*` enum cases
with no lost pass. All five Cargo configurations, all-target, all-features
check, unsafe, Composer S0, all four Symfony S1 gates and warmed-kernel S2 pass.
The added metadata is confined to class declarations and the validation runs
only while traits are composed; ordinary objects, calls, dispatch and generated
code are unchanged, so no runtime performance benchmark applies.

Magic-method declarations now enforce PHP 8.5's covariant return contracts in
classes, interfaces and traits, including `never`, literal booleans, nullable
debug arrays and object subtypes. Constructors and destructors reject every
declared return type. Direct enum declarations permit only the invocation magic
methods and reject lifecycle and state magic methods with source-qualified
diagnostics. This adds 28 exact passes without losing a prior pass: fourteen
enum restrictions, thirteen return-type cases and the invalid Stringable trait
case. All five Cargo configurations, all-target, all-features check, unsafe,
Composer S0, all four Symfony S1 gates and warmed-kernel S2 pass. The change is
confined to cold declaration validation and does not alter runtime layouts,
generated code or dispatch, so no runtime performance benchmark applies. Magic
methods imported into enums through traits remain a separate composition and
source-location cluster.

Union-typed calls now test exact members before PHP's scalar coercion
precedence, preserve integer values for `int|float`, select floating-point for
decimal numeric strings, and fall back from non-finite integer conversion to
string or bool. Strict calls retain PHP's allowed int-to-float widening, a
standalone `null` union member no longer accepts arbitrary values, built-in
members have canonical diagnostic order, booleans use `true`/`false` in
declared-type errors, and closure errors expose their source-qualified public
name. Float string output also uses PHP's `INF`, `-INF` and `NAN` spellings.
This adds six exact passes without losing a prior pass: both union type-checking
cases, `closure_027.phpt`, `scalar_none.phpt`, `scalar_null.phpt` and
`scalar_weak_reference.phpt`. All five Cargo configurations, all-target,
unsafe, Composer S0, all four Symfony S1 gates and warmed-kernel S2 pass. Ten
interleaved five-million exact-union-call runs move the median from 0.92862 s to
0.93402 s (+0.58%); five-million ordinary double-to-string conversions move
from 0.51029 s to 0.51042 s (+0.03%), both within observed run distributions
with identical checksums. The mismatch-only coercion selector is cold and
non-inlined so the ordinary exact-member path remains compact.

The canonical baseline and internal diagnostic routers now retain PHP's
request-local last unhandled error for `error_get_last()`, while
`error_clear_last()` resets it. Diagnostics hidden by `@` or the reporting mask
remain observable, a user handler that accepts a diagnostic leaves the prior
record intact, and returned arrays are detached snapshots. The exact pass set
is unchanged (+0/-0), but both strict and weak union-type checking PHPTs move
from a missing-function runtime failure to their later, independent union
coercion and eval-closure diagnostic output differences. All five Cargo feature
configurations, all-target, unsafe, Composer S0, all four Symfony S1 gates and
warmed-kernel S2 pass. Legacy warnings that still bypass the canonical routers
are not claimed by this checkpoint. The new state is cold request metadata and
does not alter values, objects, call frames or generated native code, so no
runtime performance benchmark applies.

Anonymous class names exposed through `get_class()` now use PHP 8.5's parent-
or first-interface-derived public prefix and retain a request-unique opaque
suffix after NUL. The projected name remains valid for `class_alias()`, and an
anonymous subclass with inherited abstract requirements reports its public
name, method list and source location. Binary-safe `strstr()`, including its
empty-needle and before-needle behavior, and the
`JSON_PRESERVE_ZERO_FRACTION` constant complete the adjacent PHP contract.
This adds `Zend/tests/anon/anon_class_name.phpt` and `gh15994.phpt` with no lost
pass. All five Cargo feature configurations, all-target, unsafe, Composer S0,
all four Symfony S1 gates and warmed-kernel S2 pass. Named `get_class()` calls,
ordinary object lookup, object layout and generated native code are unchanged;
the anonymous-name fallback runs only after a cold class-table miss, so no
runtime performance benchmark applies.

Anonymous classes now accept trait composition, including aliases and
visibility adaptations, through the same class-linking contract as named
classes. Their `var_dump()` class and private-property owner labels hide the
runtime-only identity suffix, explicit abstract methods fail at declaration,
and a final assignment may omit its semicolon at end of source as PHP permits.
This adds nine exact passes without losing a prior pass: two trait-use cases,
six anonymous object dumps, and the explicit-abstract diagnostic. All five
Cargo feature configurations,
all-target, unsafe, Composer S0, Symfony S1 and warmed-kernel S2 gates pass.
The executable object layout, ordinary object access and generated native code
are unchanged; added formatting is confined to the explicitly requested cold
`var_dump()` path and the remaining work is parser/compiler class metadata.

Dynamic-property creation now follows the PHP 8.5 diagnostic contract across
direct assignment, compound assignment, increment/decrement, array append and
clone-with updates. The deprecation occurs only on first creation and before an
undefined-property warning; existing members, `stdClass`, a consuming `__set`
and classes carrying or inheriting `#[AllowDynamicProperties]` are exempt. The
lexer retains that built-in attribute for class declarations, including its
fully-qualified spelling, while continuing to discard unrelated attributes.
This adds `Zend/tests/dynamic_prop_deprecation.phpt` and
`Zend/tests/clone/clone_with_002.phpt` with no lost pass. All five Cargo feature
configurations, all-target, unsafe, Composer S0, Symfony S1 and warmed-kernel S2
gates pass. Fifteen interleaved declared-property-write measurements preserve
the exact result and move the median from 0.181461 s to 0.182137 s (+0.373%);
the candidate p10/p90 interval is 0.178616/0.191200 s, within measurement noise.

The retained `AllowDynamicProperties` declaration marker now also reaches
interface, trait, enum and readonly-class declarations and emits PHP 8.5's
compile-time target diagnostic for each forbidden kind. Ordinary classes and
delayed validation on non-declaration members remain unchanged. The four exact
target-validation PHPTs move to pass with no lost pass. All five Cargo feature
configurations, all-target, unsafe, Composer S0, Symfony S1 and warmed-kernel S2
gates pass. This is a lexer/parser-only cold compile-path change; it does not
alter runtime dispatch, object layout or generated native code, so no runtime
performance gate applies.

Readonly classes now reject every attempted dynamic-property creation before
mutation across direct, compound, increment/decrement and reference write
paths, while a consuming `__set` remains legal. Anonymous class parsing retains
the readonly and `AllowDynamicProperties` modifiers, accepts valid anonymous
readonly classes, and emits PHP 8.5's compile-time errors for the forbidden
attribute, abstract/final modifiers and duplicate readonly modifier. Anonymous
property diagnostics expose PHP's stable `class@anonymous` name rather than the
runtime identity suffix. This adds all five `gh10377` variants plus
`readonly_class_dynamic_property.phpt`, with no lost pass. All five Cargo
feature configurations, all-target, unsafe, Composer S0, Symfony S1 and
warmed-kernel S2 gates pass. The object layout and native code are unchanged;
the named-class slow path retains its allocation-free shared class name.

Automatically invoked `__clone` now grants each initialized readonly property
one successful direct reinitialization on the cloned receiver. Failed type
checks do not consume the grant, a second or indirect array update is rejected,
and manually calling `__clone` receives no grant. Clone-with snapshots the
readonly properties initialized after `__clone` and permits each named update
once, so a hook that initializes another readonly property cannot make a later
array entry overwrite it. Frame-local cold sidecars preserve the 72-byte object
layout and are cleared on normal completion and exception paths. This adds
seven exact PHP 8.5 passes without losing a prior pass. The remaining
`readonly_clone_success2.phpt` executes the expected unset/reassign values but
still differs in pre-existing object-handle numbering. All five feature
configurations, all-target and unsafe gates pass. Ten-pair typed/untyped
property read, write, method and constructor controls measure +0.451%, -1.880%,
+0.684% and +1.950%, all within the five-percent ceiling.

PHP 8.5's `clone($object, $withProperties)` form now validates both operands
before cloning, rejects live reference aliases, runs `__clone`, and then applies
the update array in insertion order through the ordinary scoped property-write
path. Consequently visibility, asymmetric setters, typed properties, hooks,
magic methods and exceptions keep their existing baseline semantics. The same
checkpoint replaces the legacy non-object clone error with PHP 8.5's catchable
`TypeError`, while preserving the older `clone (new C)->property` grammar. The
exact corpus delta is +13/-0: nine clone-with cases and four adjacent clone
diagnostic cases. Four clone-with cases remain visible behind independently
missing dynamic-property deprecation, readonly reinitialization during
`__clone`, lazy-object Reflection and Random extension support. All five feature
configurations, all-target, unsafe, Composer S0, Symfony S1 and warmed-kernel S2
gates pass. No performance gate applies because the new validation and loop are
emitted only for the new cold clone-with construct; ordinary clone and property
hot paths are unchanged.

Property hook implementation names such as `$property::get` and
`$property::set` are now hidden from direct object callbacks. A direct call
therefore reports PHP 8.5's undefined-method error, while a public `__call`
still receives the requested name and arguments. The shared dynamic callback
resolver enforces the rule for variable and braced method names without adding
work to ordinary cached method dispatch. This adds the exact
`direct_hook_call.phpt` pass without losing a prior pass. All five feature
configurations, all-target, unsafe, Composer S0, Symfony S1 and warmed-kernel
S2 gates pass. No ordinary method performance gate applies because its hot
resolver and inline-cache path are unchanged.

Unsetting an accessible hooked property now raises PHP 8.5's catchable
`Cannot unset hooked property` error before changing backed storage or invoking
`__unset`. The rule covers backed, virtual, uninitialized and inherited hooks,
while ordinary properties retain their existing unset behavior and visibility
checks remain authoritative. This adds the exact property-hook `unset.phpt`
pass without losing a prior pass. All five feature configurations, all-target,
unsafe, Composer S0, Symfony S1 and warmed-kernel S2 gates pass. No ordinary
property performance gate applies because the additional metadata lookup is
confined to the cold `UnsetObj` handler.

Parenthesized static-property expressions now retain their value-call boundary
in the parser AST. Consequently `(parent::$property)::method()` fetches the
parent's static property and invokes the method on the resulting class name,
while unparenthesized `parent::$property::get()` and `set()` retain their PHP
8.5 property-hook meaning. This adds the exact `parent_syntax.phpt` pass without
losing a prior pass. All five feature configurations, all-target and unsafe
gates pass. No runtime performance gate applies because bytecode and VM paths
are unchanged; the distinction is cold parser metadata only.

Plain backed properties now provide PHP 8.5's implicit
`parent::$property::get()` and `set()` accessors to overriding hooks. They use
the parent's backing slot without redispatching into the child, enforce the
internal-accessor exact arity, and report catchable property diagnostics for
missing or inaccessible storage and a missing parent scope. Explicit user
hooks retain their normal surplus-argument behavior. This adds 13 exact passes
without losing a prior pass, including default-value inheritance and generator
hook interactions. All five feature configurations, all-target, unsafe,
Composer S0, Symfony S1 and warmed-kernel S2 gates pass. Typed/untyped
read/write/method/constructor performance lanes measure -0.642%, -0.742%,
+0.982% and +2.863%, within their five-percent ceiling.

Parent property-hook syntax without an active class scope now reports PHP
8.5's class-scope compile fatal before hook-context validation. Incrementing or
decrementing any function or method return is likewise represented as PHP's
compile-time write-context error rather than leaking an internal AST message.
This adds two exact passes without losing a prior pass. All feature, target and
unsafe gates pass. No performance gate applies because valid-program bytecode
and runtime dispatch are unchanged.

Explicit `parent::$property::get()` and `set()` calls inside matching property
hooks now lower to the existing exact parent-method call protocol, preserving
the current object receiver and case-insensitive hook names. Calls outside a
hook or from a different property/hook fail during compilation with PHP 8.5's
diagnostics. This adds six exact passes without losing a prior pass. All
feature, target and unsafe gates pass. No runtime performance gate applies:
ordinary property bytecode and VM dispatch are unchanged, and only the new
source form enters the established static parent-call path.

`ReflectionProperty` now exposes PHP 8.5 default-value presence and value,
including the distinction between implicit-null untyped declarations,
uninitialized typed declarations and constructor-promoted properties. Calling
`getDefaultValue()` without a default emits PHP's deprecation at the callsite.
Final, abstract and virtual hook flags are available through both modifiers and
their predicates. The metadata reuses the cold declaration flag word, retaining
the established `PropertyDefinition` size and object execution layout. This
adds three exact passes without losing a prior pass. All feature, target and
unsafe gates pass. The 40-pair ordinary property A/B workload is -1.976%;
specialized typed/untyped read, write, method and constructor lanes are -0.183%,
-0.206%, +1.869% and +1.934%, within their ceilings. Hook method reflection,
raw-value access and further Reflection metadata remain follow-up work.

Constructor-promoted properties now retain their complete declaration shape,
including PHP 8.5 `final`, asymmetric visibility, type, readonly and hook
metadata. Promoted getter/setter bodies are compiled as property hooks, and the
implicit constructor assignment invokes the setter before the user constructor
body. Hook-only parameters use PHP's implicit public promotion. This adds ten
exact passes without losing a prior pass. All feature, target and unsafe gates
pass. The 40-pair ordinary property A/B workload is +0.457%; specialized
typed/untyped read, write, method and constructor lanes are +0.522%, +0.387%,
+2.991% and +2.526%, within their respective one- and five-percent ceilings.
The remaining `cpp.phpt` case depends on Reflection property-default support;
AST-rendering cases remain separate follow-up work.

Property-hook declaration validation now reports PHP 8.5 compile fatals for
empty or duplicate hook lists, unknown hook names, hook visibility/static
modifiers, hooks on static properties, getter parameter lists, and invalid
setter arity, defaults or variadics. Parsing retains enough cold
declaration shape to include the class and property in each diagnostic without
exposing invalid parameters to executable code. This adds ten exact passes
without losing a prior pass. All feature, target and unsafe gates pass. No
runtime performance gate applies because valid declarations generate unchanged
bytecode and the new checks remain in parsing and cold class compilation.

Indirect mutation through a by-value property getter now raises PHP 8.5's
property-specific error before modifying a detached array, while objects
returned by value remain legally mutable. By-reference object iteration invokes
hook getters, preserves their alias identity, rejects by-value hook results and
attaches typed-property constraints to ordinary property aliases. This adds four
exact passes without losing a prior pass. The 40-pair ordinary property A/B
workload is -1.074%; a separate 20-pair hookless object-foreach control is
+0.102%. Typed/untyped read, write, method and constructor lanes are +0.039%,
+0.118%, +2.381% and +0.435%, within their five-percent ceiling. Full hooked
object-iteration ordering, constructor-promoted hooks, parent-hook calls and
Reflection remain follow-up work.

Abstract properties and body-less hooks now form real class, trait and
interface contracts. A plain property supplies both ordinary accessors, a
partial hooked override inherits a missing concrete parent hook, and unresolved
hooks remain non-executable abstract stubs with complete singular/plural
diagnostics. Interface getter contracts admit readonly storage, while an
interface setter rejects it through the corresponding set-access contract.
Cold capability flags reuse existing source metadata and validation helpers are
kept off hot paths. This adds 43 exact passes without losing a prior pass. The
40-pair ordinary property A/B workload is +0.571%; typed/untyped read, write,
method and constructor lanes are -0.278%, -0.774%, +1.218% and +1.018%, within
their ceilings. Reference-returning hooks and their direct alias contracts are
covered by the following checkpoints; full hooked iteration ordering,
parent-hook calls and Reflection remain follow-up work.

Final properties and final property hooks now retain their declaration contract
through parsing, compilation and inheritance linking. Child redeclarations fail
with PHP 8.5's property- or hook-specific diagnostic, while invalid private and
body-less final forms fail during declaration compilation. Finality occupies a
cold flag in existing property source metadata, preserving the established hot
metadata size and layout. This adds seven exact passes without losing a prior
pass. The 40-pair ordinary property A/B workload is -2.109%; typed/untyped read,
write, method and constructor lanes are +0.392%, -1.409%, +1.636% and +0.136%,
within their five-percent ceiling. By-reference hooks, parent-hook calls and
Reflection remain follow-up work.

Property `get` and `set` hooks now accept PHP 8.5's arrow form. Getter
expressions return through the existing hook method contract; setter
expressions transform the value written to backing storage while the assignment
expression itself retains its input value. The syntax reuses the existing hook
reentrance guard and method lowering rather than adding a runtime fast path.
This adds 23 exact passes without losing a prior pass. The ordinary property A/B
workload is -1.565%; typed/untyped read, write, method and constructor lanes are
-1.066%, -0.572%, +2.190% and -0.143%, within their five-percent ceiling.
By-reference, parent-hook and Reflection forms remain follow-up work.

Block-form property `set` hooks now accept PHP's implicit `$value` or one
explicit, independently typed parameter and execute before ordinary backing
storage writes. Access to the same property from either hook bypasses both
hooks through one reentrance guard; this supplies implicit reads for backed
setters while virtual set-only properties raise the canonical write-only error
for reads and `isset()`. Assignment expressions retain their original input
value even when the hook transforms backing storage. This adds 20 exact passes
without losing a prior pass. The ordinary property A/B workload is -1.824%;
typed/untyped read, write, method and constructor lanes are -0.020%, +0.548%,
+2.074% and +0.366%, all within their five-percent ceiling. Abstract, final,
by-reference, parent-hook and Reflection forms remain follow-up work.

Explicit block-form property `get` hooks now compile through the ordinary user
method engine and execute on the cold declared-property path. Reentrant
`$this->property` access reaches backing storage, while a virtual getter remains
read-only; hook return contracts, magic constants, static locals, closure
capture and inheritance variance reuse their general method semantics. Hook
methods remain hidden from `get_class_methods()`. This adds seven exact passes
without losing a prior pass. The final 20-pair ordinary property A/B gate is
-0.718%; the 40-pair typed/untyped method lane is +2.176%, below its five-percent
ceiling. Abstract, final, by-reference and Reflection hook forms
remain explicit follow-up work.

Named child classes whose invariant property types depend on an alias published
by an earlier runtime `class_alias()` now wait in a cold linking queue. Alias
publication retries every resolvable declaration, while unresolved or invalid
contracts still raise their original declaration fatal when top-level execution
finishes. This adds `property_types_early_bind.phpt` without losing a prior pass;
`class_exists(..., false)` also preserves PHP's pre-alias visibility. The queue
is request-global metadata read only during class/alias registration and request
completion; object, value, frame and successful property-access paths are
unchanged, so no hot-path performance gate applies.

Invariant property types now compare their reduced PHP value sets in both
directions, including inheritance-reduced intersections, redundant unions,
aliases available at link time and `iterable` as `array|Traversable`. Invalid
declarations render parent DNF, built-in union and omitted-type contracts in
PHP's canonical form, and inheritance errors point to the child class line.
This adds 14 exact passes without losing a pass or moving a remaining failure
stage. The change is confined to parsing and cold class linking; runtime value,
object, property-cache and executor layouts and successful property execution
paths are unchanged, so no runtime performance gate applies.

Property declarations now retain their source line through compilation and
cold class linking. Asymmetric set-scope, readonly, final and invariant type
errors therefore include PHP's file/line suffix; a second asymmetric access
modifier is emitted as a located compile fatal. This adds 28 exact diagnostic
passes without losing a pass. Fifteen remaining failures move from the
runner's `runtime` classification to the correct front-end `compile` stage
because their location now matches, while another diagnostic detail remains
different. The general property workload measured -0.281%; typed/untyped read,
write, method and constructor lanes ranged from -0.742% to +3.264%, within
their five-percent ceiling.

Static properties now enforce the same asymmetric set visibility across direct
assignment, increment, compound and dimension mutation, reference access and
warmed inline-cache execution. Mutating an object stored in a readable static
property remains legal, and unsetting a member through an uninitialized typed
static property remains a no-op. This adds one exact pass without losing a
pass. Static read controls measured +0.068% for `self::` and +0.188% for
`static::`; the specialized property read and write lanes remained within their
five-percent ceiling. Property hooks and several source-located declaration
diagnostics remain incomplete.

Typed instance properties and promoted constructor properties now retain a
separate PHP 8.5 `private(set)`, `protected(set)` or `public(set)` visibility.
Reads keep their declared visibility while assignments, indirect array writes,
references and unsets enforce the narrower write scope; inheritance also
retains final `private(set)` and set-scope variance. This adds 16 exact passes
without losing a pass. Property-hook interaction and several source-located
declaration diagnostics remain incomplete. The
instance-property A/B control measured +0.448%, and all four specialized
typed/untyped property lanes remained within their five-percent ceiling.

Binary operators now admit PHP's value-producing assignment expression in
their right operand across power, multiplicative, additive, shift and bitwise
precedence layers. This adds two exact PHP 8.5.6 passes without losing a pass.
All five CI feature configurations, the all-target check and unsafe gate pass;
no runtime performance gate applies because execution paths are unchanged.

Type errors now use PHP's canonical diagnostic names independently of source
declaration spelling. Concrete objects report their runtime class, and
`iterable` expands to `Traversable|array`, including its nullable form. This
cold error-rendering change adds five exact PHP 8.5.6 passes without losing a
pass or moving any remaining failure between stages.

An unmatched `match` expression now retains its discriminant and source line
until the cold throw path constructs `UnhandledMatchError`. Scalar values use
PHP's value-specific spelling and compound values name their concrete type;
successful arms keep their existing comparison and result path. This adds
three exact PHP 8.5.6 passes without losing a pass or moving a failure stage.

Argument type failures now retain each user function, method or closure's
declaration line in the existing cold source map. Verification snapshots the
pending callee before cleanup, so Throwable origin points to the declaration
while trace frame zero still contains the call site and rejected arguments.
This adds 16 exact PHP 8.5.6 passes without a lost pass or stage movement and
also removes one redundant unsafe cleanup block.

Exceptions suspended while a `finally` block executes now live in a cold,
frame-scoped sidecar instead of occupying the active VM exception slot. A new
exception can therefore escape the block, retains its explicit `previous`
chain, and appends the displaced exception without creating recursive chains;
locally caught exceptions leave the suspended value untouched. Coroutine
exchange and frame cleanup carry the same state. This adds 11 exact PHP 8.5.6
passes without losing a pass or moving a remaining failure stage.

`never` and `void` are now rejected in parameter positions, and any explicit
value return from a function, method or closure with either return contract is
rejected while compiling the declaration, including dead and uncalled code.
Generator declarations likewise prove that their declared type contains a
`Generator` supertype before execution; compatible nullable, union and
intersection forms remain admitted. This adds ten exact PHP 8.5.6 passes with
no lost pass or failure-stage movement.

`var_dump()` now exposes a Generator's retained public function identity as
PHP's synthetic `function` debug property before execution, while suspended
and after completion. Named functions, methods, anonymous-class methods and
closures reuse their existing public trace names without adding mutable object
state. This adds six exact PHP 8.5.6 passes with no lost pass or stage movement.

Generator key allocation now advances its next implicit integer after an
explicit integer key without rewinding for lower or string keys, including
PHP's signed wrap after `PHP_INT_MAX`. `Generator::getReturn()` also starts a
new generator on demand and distinguishes normal completion from exceptional
closure, reporting the incomplete state as a catchable `Exception`. Together
these contracts add four exact PHP 8.5.6 passes with no lost pass.

`Generator::throw()` now auto-primes a new generator and injects a `Throwable`
at its active suspension point. A caught exception resumes to the next yield,
an escaped exception closes the generator and reaches the caller, and active
`yield from` chains forward injection through both generator and array
delegates. Closed generators rethrow the supplied value, while non-Throwable
arguments raise the canonical `TypeError`. This adds six exact PHP 8.5.6
passes with no lost pass.

Generator objects now enforce their engine-owned invariants: they cannot be
cloned, cannot receive dynamic properties, and cannot be serialized or
materialized by `unserialize()`. Rewind remains legal through the first
suspension point but is rejected after advancement, including delegated array
yields, and `count()` accepts only arrays or `Countable` objects while safely
retaining a receiver that releases itself during `Countable::count()`. This
adds five exact PHP 8.5.6 passes with no lost pass. The 200-pair generator
resume gate measured -2.155% versus the prior binary, so no speedup is claimed
but the one-percent regression ceiling is satisfied.

Exceptions created by internal functions and methods now snapshot their live
internal call frame before cleanup, while exceptions supplied by the caller
retain their original immutable trace. Detached generator failures reconnect
the generator, internal method and user caller only for that cold snapshot,
including rethrows from `Generator::throw()`. Public trace names recover the
canonical declaring-class spelling. This adds six exact PHP 8.5.6 passes with
no lost pass. A 100-pair hot `strlen()` internal-call gate measured -3.762%
against the prior binary; no speedup is claimed, but the one-percent regression
ceiling is satisfied.

Reentrant `Generator::next()`, `send()` and `throw()` calls now raise the
catchable PHP `Error` required by PHP 8.5 instead of silently returning or
escaping as an internal VM fatal. When that error leaves the detached
generator, its existing internal-method trace prefix is joined to the
generator, outer method and user caller continuation without changing the
ordinary resume path. This adds three exact PHP 8.5.6 passes with no lost pass.
The 200-pair generator resume gate measured -1.216% against the prior binary;
no speedup is claimed, but the one-percent regression ceiling is satisfied.

A generator that attempts to `yield from` itself while it is running now fails
at the delegation boundary with PHP 8.5's catchable `Error`. The identity guard
runs before either generator state is borrowed, eliminating the prior public
Rust `RefCell` panic without creating a partial delegation. This adds one exact
PHP 8.5.6 pass with no lost pass. The 200-pair generator resume control measured
+0.436%, within the one-percent regression ceiling.

Keyword `and`, `xor` and `or` now occupy PHP's three distinct precedence
levels below assignment and `yield`, independently of symbolic `&&` and `||`.
Yield operands admit assignments and nested yields, while a valueless yield can
participate in the surrounding binary expression. This corrects assignment
side effects, short-circuit order, keyed nested yields and unary/multiplicative
forms through the lexer and parser without changing runtime dispatch. It adds
six exact PHP 8.5.6 passes with no lost pass and moves additional valid cases
from front-end rejection to runtime. No runtime performance gate applies to
this lexer/parser-only checkpoint.

Recursive array identity and loose object comparison now track active compound
values and enforce a bounded comparison depth. Cycles between distinct values
raise the PHP 8.5-compatible catchable `Error` instead of overflowing the Rust
stack, while comparing an array or object with itself remains true.
Array removal also preserves the internal cursor's logical entry across
packed-to-hash transitions and ordered-entry compaction, allowing a subsequent
append to become current when the last prior entry was removed.

The PHP 8.5 pipe operator now has a distinct token and precedence layer between
concatenation and comparisons. Its baseline lowering evaluates the input, then
the callable expression, then invokes it with one non-referenceable argument.
Direct `assert()` calls also synthesize PHP's canonical expression description,
including pipe-parenthesization, first-class callables and named arguments.
This admits 20 of the 30 pinned pipe tests plus the related assertion callable
and named-parameter tests. One pipe CLI-INI case remains unsupported; the other
remaining cases stay visible under independent diagnostics or runtime gaps.

Legacy `assert_options()` now keeps PHP's active, callback, bail, warning and
exception settings per request, returns each previous value and invokes the
callback with source metadata and the synthesized assertion description. Bail
mode emits the assertion warning before an uncaught callback error and exits
with PHP's status. This adds both PHP 8.5 GH-16293 cases with no lost pass. The
PHP 8.3+ deprecation diagnostics for the legacy function and constants remain
outside this checkpoint.

The frontend now retains three additional PHP compile-time contracts even in
dead code: a free-standing custom `assert()` declaration is forbidden,
`new class(...)` cannot create a first-class callable, and a first-class
callable cannot be used as an attribute argument. Attribute groups remain
otherwise unobserved, but an invalid Closure-producing argument is no longer
silently discarded. This adds all three affected PHP 8.5 tests with no lost
pass and completes the reached assertion failure cluster.

Invalid array and argument unpack operands now report PHP 8.5's concrete
source type, including runtime object class names, while preserving the
distinct catchable `Error`, `TypeError` and compile-time fatal paths. Type-name
construction remains confined to rejected cold paths. This adds the three
`arg_unpack` and `array_unpack` contract cases with no lost pass.

Declared return contracts now distinguish an absent type from `mixed`, require
an explicit value at an implicit function exit, and reject source-level bare
returns during compilation with nullable and `never` diagnostics matching PHP
8.5. Generator bodies retain their independent `Generator` return contract.
Runtime return errors include the declaring function or method name and an
uncaught origin/trace. This adds 22 exact tests with no lost pass or remaining
failure-stage movement. A disabled-JIT/quick-loop ten-million-call control kept
the same 0.31-second median and checksum before and after the change.

Built-in Throwable families now expose PHP's `__toString()` representation by
sharing the uncaught formatter's class, message, immutable origin, stored trace
and oldest-first previous chain without its `Uncaught` or final `thrown` text.
Trace string arguments escape backslashes, control bytes and non-ASCII UTF-8
bytes using PHP's byte-oriented notation. This adds six exact tests with no
lost pass. Eleven other tests now reach their later output comparison instead
of failing object-to-string conversion; their independent diagnostic,
named-argument, match and try/finally differences remain explicit failures.

Static locals returned by reference now keep one request-owned cell across
full return synchronization, first-class callable invocation and pipe
forwarding. Generic concatenation dereferences its operands before applying
the ordinary string fast paths, so compound writes through the returned alias
retain PHP identity. This adds the pipe reference-context and global-static
tests with no lost pass. Seven five-million-concatenation release controls
measured equal 0.31-second medians before and after the change with identical
output.

`array_multisort()` now implements lexicographic multi-column permutation,
per-column direction and comparison flags, PHP key rebuilding and the legacy
prefer-by-reference signature used by pipe and callback calls. Variadic
reference packing and `call_user_func_array()` preserve explicit array-element
aliases instead of cloning their values. This adds five exact passes with no
lost pass, including two broader callback-dispatch cases. Seven ten-million
dynamic-call release controls measured a 1.26-second preceding median and a
1.24-second candidate median with identical output.

Call errors now retain the declaration spelling of user-function names instead
of exposing the lowercase lookup key, and non-referenceable arguments use PHP
8.5's `could not be passed by reference` diagnostic. This adds the pipe
by-reference, nullsafe-property and restricted-`$GLOBALS` cases without losing
an exact pass; the behavior remains on cold error paths.

An unparenthesized arrow function immediately after `|>` is now retained as a
located compile-time error even in dead code, matching PHP 8.5's required
parentheses rule. Parenthesized arrow callables remain valid. This adds all
three affected precedence diagnostics with no pass-set loss and no runtime
dispatch change.

Canonical user calls now validate supplied argument types before reporting a
later missing required argument, retain parameter names and caller locations
in `TypeError`, and apply PHP's weak scalar conversion table before that
decision. The checkpoint adds 11 exact passes across pipe calls, generators,
first-class callables, reference initialization and strict/weak include
boundaries without losing a previous pass. Exact hot calls remain on their
existing compact paths; the additional work is confined to mismatched cold
call preparation.

`iterator_to_array()` now consumes arrays and the canonical Generator,
IteratorAggregate and Iterator protocols, preserves or reindexes keys as
requested, and propagates traversal exceptions. This closes the PHP 8.5 pipe
generator chain without adding a pipe-specific path. Six other reached tests
using the builtin remain blocked earlier by independent SPL or Reflection
gaps and stay ordinary failures.

Lazy Reflection property operations now validate declared, static, virtual,
dynamic, typed and readonly targets before mutating lazy state. Raw writes
commit their value before running a replaced receiver's destructor, preserve a
lazy slot when coercion or `__toString()` fails, and follow initialized nested
proxy chains to their terminal instance. Ghost defaults become visible while
the initializer runs, when the object is also presented as ordinary rather
than with a lazy dump prefix. Against the pinned 223-case PHP 8.5.6 lazy-object
cluster this moves the exact pass set from 166 to 177, an exact +11/-0 delta.
The three focused `realize*`, `setRawValueWithoutLazyInitialization*` and
`skipLazyInitialization*` families pass 22 of 25 cases; the remaining three
retain independent inherited-private-property and Reflection-output gaps.

All five Cargo feature configurations, the all-feature/all-target check,
formatting, Composer S0, all four Symfony S1 gates and warmed-kernel S2 pass on
AMD64. The production unsafe inventory remains 1,620 blocks below its 1,623
ceiling and 289 functions at its ceiling. Twenty alternating release pairs of
`bench_property.php` measured baseline p10/median/p90 of
1.282391/1.292819/1.339814 seconds and candidate
1.260106/1.275084/1.301365 seconds (median -1.37 percent), with the identical
`12499997500000` result.

Lazy resets now retire only the declared storage visible through the reflected
class, while preserving additional subclass properties and initialized
readonly storage inherited from another declaring class. An initialized proxy
is detached before its old real instance is destructed; a throwing destructor
restores the proxy relation and leaves the retired lifecycle marked so it is
not invoked again. Successful resets remove typed-reference constraints,
release nested declared and dynamic values after the reset commit, and reject
re-entry while an initializer is active. Main-scope CV and request-global
mirrors are also counted as one logical ownership boundary for replacement and
shutdown destructors. The exact 223-case lazy-object pass set moves from 177 to
187, a further +10/-0 delta; 12 of the 14 `reset_as_lazy*` cases now pass. The
remaining two expose independent closure-reference-capture and dynamic-value
destructor-order gaps.

All five Cargo feature configurations, the all-feature/all-target check,
formatting, Composer S0, all four Symfony S1 gates and warmed-kernel S2 pass on
AMD64, with the unsafe inventory unchanged. A final 20-pair candidate-first
confirmation of `bench_property.php` measured baseline p10/median/p90 of
1.273414/1.284652/1.333661 seconds and candidate
1.268104/1.279233/1.295493 seconds (median -0.42 percent), with identical
output.

Lazy serialization now lets `__serialize()` inspect no object state without
realizing a ghost or proxy, while `__sleep()` may trigger initialization by
observing a lazy property even under `SKIP_INITIALIZATION_ON_SERIALIZE`.
Property lists returned by `__sleep()` are resolved against the hook's
declaring scope and serialized with canonical public, protected and private
wire names from the terminal real instance. Public hook resolution is retained
across the availability check and invocation, avoiding a duplicate method
lookup. All 14 lazy serialization PHPTs pass, and the exact 223-case lazy
object pass set moves from 187 to 190, a +3/-0 delta.

The five Cargo feature configurations, all-feature/all-target check,
formatting, unsafe inventory, Composer S0, all four Symfony S1 gates and the
warmed-kernel S2 gate pass on the final AMD64 source. Thirty candidate-first
pairs of a 300,000-iteration object-serialization benchmark produced the same
`27600000` checksum. Baseline p10/median/p90 was
0.684384/0.734046/0.762439 seconds and the candidate measured
0.677617/0.708024/0.749808 seconds (median -3.55 percent).

Object projections used by `json_encode()` and by-value `foreach` now invoke
public property getters, including virtual getters and reference-returning
backed getters, while preserving visible declared-property order and dynamic
properties. Getter exceptions stop the projection and propagate through the
calling operation. The exact 223-case lazy-object pass set moves from 190 to
191, a +1/-0 delta: `init_trigger_json_encode_hooks.phpt` is the sole changed
status. The corresponding `foreach` hook behavior is covered by an original
regression, while its complete upstream case remains blocked later by the
independent closure-reference-capture gap noted above.

All five Cargo feature configurations, the all-feature/all-target check,
formatting, unsafe inventory, Composer S0, all four Symfony S1 gates and the
warmed-kernel S2 gate pass on AMD64. Thirty candidate-first release pairs of
300,000 ordinary object projections retained identical checksums. JSON
baseline p10/median/p90 was 0.282304/0.286399/0.291069 seconds versus
0.279966/0.284564/0.288662 seconds for the candidate (median -0.64 percent);
by-value `foreach` was 0.304920/0.308074/0.313435 seconds versus
0.303629/0.306894/0.317169 seconds (median -0.38 percent).

Cloned callback descriptors now retain the shared cell behind explicit closure
captures such as `use (&$state)`. Lazy initializer assignments and retry
counters therefore remain visible to their defining scope across registration,
failed initialization and later invocation. Property reads and object
projections also follow initialized proxy-of-proxy chains to their terminal
instance, initialize each reached lazy endpoint when the operation requires
it, stop safely on cycles, and retain per-property skip and magic-method
initialization rules. The exact 223-case lazy-object pass set moves from 191 to
194, a +3/-0 delta: `gh15823.phpt`, `init_trigger_foreach_hooks.phpt` and
`reset_as_lazy_real_instance.phpt` are the only changed statuses.

All five Cargo feature configurations, the all-feature/all-target check,
formatting, unsafe inventory, Composer S0, all four Symfony S1 gates and the
warmed-kernel S2 gate pass on AMD64. Twenty candidate-first release pairs of a
300,000-read initialized lazy-proxy control retained the `2100000` checksum;
baseline p10/median/p90 was 0.204670/0.221255/0.233671 seconds versus
0.203410/0.217748/0.228415 seconds for the candidate (median -1.59 percent).
The established five-million-operation ordinary property control retained the
`12499997500000` checksum and measured 1.275476/1.284359/1.299421 seconds
versus 1.277888/1.294315/1.329062 seconds (median +0.78 percent).

Object comparison now follows PHP's three-way object handler instead of
rejecting non-scalar relational operands. Distinct same-class lazy objects are
realized through their terminal proxy chains before their properties are
compared, initializer exceptions remain catchable, and source left-to-right
initializer order for `>` and `>=` survives the compiler's internal operand
swap. Equality, `<`, `<=` and `<=>` retain Zend's right-operand-first handler
order. Identity and class-mismatch decisions do not initialize either object.
String/object comparison invokes `__toString()` on the lazy shell without
eager realization, so initialization occurs only if that method observes lazy
state. The exact 223-case lazy-object pass set moves from 194 to 196, a +2/-0
delta: `get_properties.phpt` and `init_trigger_compare.phpt` are the only
changed statuses.

All five Cargo feature configurations, the all-feature/all-target check,
formatting, unsafe self-test and ratchet, Composer S0, all four Symfony S1
gates and warmed-kernel S2 pass on AMD64. Twenty alternating release pairs of
the established five-million-operation property control measured baseline
p10/median/p90 of 1.272029/1.286560/1.318402 seconds versus
1.268573/1.292946/1.313119 seconds for the candidate (overall median +0.50
percent; balanced order-specific median ratio +0.266 percent).

`ReflectionClass::getProperty()` now applies the same inherited-private
visibility boundary as property enumeration. Instance and static private
properties remain reflectable through their declaring class but are not
reported as properties of a child class; inherited protected properties remain
available. This lets lazy-property skipping distinguish a missing child
property from a private parent slot. The exact 223-case lazy-object pass set
moves from 196 to 197, a +1/-0 delta whose only changed status is
`realize_skipped.phpt`. All five Cargo feature configurations, the
all-feature/all-target check, formatting, unsafe self-test and ratchet,
Composer S0, all four Symfony S1 gates and warmed-kernel S2 pass on AMD64.

`stdClass` remains classified as internal by Reflection but is now admitted
through the lazy-object eligibility check, matching PHP's sole internal-class
exception. With no declared slots its lazy ghost and proxy are immediately
realized and do not invoke their initializer; other internal classes remain
rejected. The exact 223-case lazy-object pass set moves from 197 to 198, a
+1/-0 delta whose only changed status is `support_stdClass.phpt`. All five
Cargo feature configurations, the all-feature/all-target check, formatting,
unsafe self-test and ratchet, Composer S0, all four Symfony S1 gates and
warmed-kernel S2 pass on AMD64.

Lazy-initializer transaction snapshots now retain owned reference cells as
engine-internal aliases, so taking a snapshot does not change PHP-visible
alias cardinality. A failed ghost initializer restores declared and dynamic
storage, removes type owners introduced by partial writes, and reattaches the
original typed-property owners. A successful proxy initialization instead
detaches the shell's type owners, leaving external aliases to pre-initialized
shell properties unconstrained as required. The exact 223-case lazy-object
pass set moves from 198 to 200, a +2/-0 delta consisting only of
`init_handles_ref_source_types.phpt` and
`init_handles_ref_source_types_exception.phpt`.

All five Cargo feature configurations, the all-feature/all-target check,
formatting, unsafe self-test and ratchet, Composer S0, all four Symfony S1
gates and warmed-kernel S2 pass on AMD64. Twenty alternating release pairs of
the established five-million-operation property control measured baseline
p10/median/p90 of 1.285298/1.288852/1.331500 seconds versus
1.263447/1.280755/1.311348 seconds for the candidate; the balanced
order-specific median ratio was -0.886 percent, inside the one-percent gate.

Final object release now dispatches an initialized lazy proxy's destructor on
the final real instance rather than its shell. After an object's own
destructor, the cold release path also visits acyclic nested object properties
whose remaining handles all belong to the dying owner; shared children remain
live, aliases are grouped, and a receiver resurrected by user code keeps its
property tree. Back-edges deliberately remain cycle-collector work. The exact
223-case lazy-object pass set moves from 200 to 203, a +3/-0 delta consisting
only of `dtor_called_if_init.phpt`, `gc_006.phpt` and
`reset_as_lazy_resets_dynamic_props.phpt`, with zero crashes or timeouts.

All five Cargo feature configurations, the all-feature/all-target check,
formatting, unsafe self-test and ratchet, Composer S0, all four Symfony S1
gates and warmed-kernel S2 pass on AMD64. Twenty alternating release pairs
against the preceding checkpoint measured a balanced order-specific ratio of
-1.166 percent for the declared-object lifecycle control and -0.077 percent
for the established five-million-operation property control, both within the
one-percent regression gate.

The `%s` conversion slots of `sprintf()`, `vsprintf()`, `printf()` and
`vprintf()` now enter PHP object string conversion in argument order. A
throwing `__toString()` aborts formatting before partial output is published,
while scalar slots retain their direct append path. This lets a caught built-in
`Error` render its canonical message and trace through `printf()` and moves the
exact 223-case lazy-object pass set from 203 to 204, an exact +1/-0 delta whose
only changed status is `init_may_leave_props_uninit.phpt`.

All five Cargo feature configurations, the all-feature/all-target check,
formatting, unsafe self-test and ratchet, Composer S0, all four Symfony S1
gates and warmed-kernel S2 pass on AMD64. No runtime benchmark is required for
this checkpoint: scalar formatting retains its prior branch and allocation
behavior, and VM re-entry exists only for compound values that previously
bypassed their required conversion contract.

`ReflectionClass`, `ReflectionObject` and `ReflectionProperty` now expose
metadata-backed string conversion. Property rendering preserves PHP 8.5
modifier, asymmetric visibility, type, default-value and hook syntax, while
`getProperties()` restores child-first source declaration order across the
separate instance/static metadata tables. Purely virtual properties no longer
claim an implicit null default. `ReflectionObject::__toString()` inspects class
metadata and dynamic-property names without observing the reflected object's
lazy slots, so ghosts and proxies remain uninitialized. Complete byte-for-byte
class/method/parameter rendering outside the admitted cases remains separate
Reflection work.

Normal debug CLI execution also no longer appends hot-executor coverage
statistics to stderr; those diagnostics had become observable PHPT output once
a previously failing test reached normal shutdown. The exact 223-case PHP 8.5.6
lazy-object pass set moves from 204 to 206, an exact +2/-0 delta consisting of
`init_trigger_reflection_object_toString.phpt` and
`skipLazyInitialization.phpt`, with 17 visible failures and no crash or timeout.
All five Cargo feature configurations, formatting, unsafe-policy and
all-feature/all-target checks, Composer S0, all four Symfony S1 gates and
warmed-kernel S2 pass on AMD64. No runtime benchmark is required because all
new rendering and ordering work is confined to explicit cold Reflection calls;
ordinary property storage and dispatch are unchanged.

Runtime object-member operations now share one PHP 8.5 property-name
conversion rule across reads, writes, reference binding, `isset()` and
`unset()`. A stringable object is converted exactly once, an unconvertible
object or closure raises the canonical catchable `Error`, and a Throwable from
`__toString()` propagates without redirecting the operation when conversion
rebinds the receiver variable. The ordinary string-name path retains its prior
ownership behavior; only object, closure and array conversions preserve an
owned receiver across re-entry.

A direct parent/candidate rerun of all 5,599 pinned PHP 8.5.6 cases moves from
2,691 to 2,693 passes, an exact +2/-0 delta consisting of
`Zend/tests/lazy_objects/typed_properties_001.phpt` and
`Zend/tests/type_declarations/typed_properties_093.phpt`. The remaining
`Zend/tests/class_properties_const.phpt` failure advances from output mismatch
to its correct runtime `Closure` conversion error but still exposes an earlier
warning ordering gap. The candidate distribution is 2,693 passes, 2,514 ordinary
failures, 110 skips, one XFAIL and 280 unsupported cases, with zero timeouts
and one pre-existing `Zend/tests/generators/yield_from_deep_recursion.phpt`
stack-overflow crash that reproduces identically on the parent. The focused
lazy-object pass set moves from 206 to 207 out of 223, an exact +1/-0 delta.

All five Cargo feature configurations, formatting, unsafe-policy and
all-feature/all-target checks, Composer S0, all four Symfony S1 gates and
warmed-kernel S2 pass on AMD64. Twenty alternating release A/B pairs of the
five-million-iteration property read/write workload measure a balanced
candidate/parent delta of -3.365%, within the +1% regression ceiling and with
identical output.

Generator delegation now uses an explicit heap-backed resume stack instead of
nested Rust calls. Send values, injected exceptions, yielded values and final
return values propagate across deep `yield from` chains; indirect
delegation cycles still raise PHP's catchable `Error`. Completed and abandoned
chains release their retained frame snapshots iteratively, so both completing
and discarding a suspended 50,000-level chain avoid a native stack overflow.
The ordinary non-delegating resume path stays separate and allocation-free;
cycle lookup switches from a small linear scan to a hash set only after 32
active delegates.

A direct parent/candidate rerun of all 5,599 pinned PHP 8.5.6 cases moves from
2,693 to 2,694 passes, an exact +1/-0 delta whose only changed status is
`Zend/tests/generators/yield_from_deep_recursion.phpt`. The candidate
distribution is 2,694 passes, 2,514 ordinary failures, 110 skips, one XFAIL
and 280 unsupported cases, with zero timeouts and zero crashes. The complete
184-case generator cluster likewise moves from 99 to 100 passes with no lost
pass.

All five Cargo feature configurations, formatting, unsafe-policy and
all-feature/all-target checks, Composer S0, all four Symfony S1 gates and
warmed-kernel S2 pass on AMD64. The established 200-pair generator-resume
control measures a balanced candidate/parent delta of -1.500%, within the +1%
regression ceiling; no speedup is claimed. The new delegated path is likewise
not an optimization claim: an exploratory one-level `yield from` control
remains slower and is reserved for general performance work after semantic
compatibility.

Shared generator delegation now observes the live leaf value and key across
every attached `yield from` parent instead of exposing a stale per-parent
snapshot. A normally completed shared leaf retains the parent's last visible
value while its key becomes `null`. Exceptional completion distinguishes a
parent that was already attached, which receives `ClosedGeneratorException`,
from a new delegation attempt, which receives PHP's catchable `Error`.
Delegated exception traces retain supplied-Throwable creation history, add
each traversed generator frame once, include the supplied argument list, and
use the source line of the corresponding `yield from` expression.

The final release rerun of all 5,599 pinned PHP 8.5.6 cases moves from 2,694
to 2,704 passes, an exact +10/-0 delta. The candidate distribution is 2,704
passes, 2,504 ordinary failures, 110 skips, one XFAIL and 280 unsupported
cases, with zero timeouts and zero crashes. The complete 184-case generator
cluster moves from 100 to 110 passes with no lost pass.

All five Cargo feature configurations, formatting, unsafe-policy and
all-feature/all-target checks, Composer S0, all four Symfony S1 gates and
warmed-kernel S2 pass on AMD64. The unsafe inventory is 1,622 production
blocks against a ceiling of 1,623. The 200-pair order-balanced direct generator
resume control measures a candidate/parent delta of -2.158%, within the +1%
regression ceiling; no speedup is claimed.

IEEE-754 floating-point division is now available through `fdiv()`, including
signed zero, infinity, NaN, weak numeric operands, named arguments and the
ordinary internal-function arity diagnostic. PHP floating-point literals may
have an empty fractional part such as `10.`, and `var_dump()` uses PHP's
uppercase `INF`, `-INF` and `NAN` spellings. The pinned
`ext/standard/tests/math/fdiv.phpt` capability test moves from a front-end
failure to an exact pass. The 23 attempted Zend cases that were blocked by the
missing function now reach their separate conversion, `settype()` or comparison
differences instead of being hidden behind an undefined-function error.

Because the separately gated extension test is outside the 5,599-case contract
corpus, the complete release rerun intentionally retains the exact 2,704-pass
set: 2,704 passes, 2,504 ordinary failures, 110 skips, one XFAIL and 280
unsupported cases, with zero timeouts, zero crashes and an exact +0/-0 pass-set
delta. All remaining failure-stage movements were inspected. All five Cargo
feature configurations, formatting, unsafe-policy, all-feature/all-target,
Composer S0, all four Symfony S1 and warmed-kernel S2 gates pass on AMD64; the
stdlib registration-capacity test also confirms that the additional builtin
does not rehash the fixed function registry. No runtime performance gate applies:
the lexer work is compile-time, `fdiv()` is a new opt-in call, and the output
formatting change is confined to explicit `var_dump()`.

`settype()` now applies PHP 8.5's case-insensitive scalar, array, object and
null conversion rules, including array/object projection, object-to-number
warnings and the rejected `resource` and invalid-type targets. NaN conversions
emit the target-specific warning before assignment and preserve PHP's
re-entrant reference behavior: scalar targets convert the call-entry snapshot,
while array and object targets observe writes or unsets performed by the error
handler. The system-backed `random_bytes()` subset required by the warning
handlers supplies ordinary positive-length data and the canonical invalid-length
`ValueError`.

The complete 26-case `Zend/tests/type_coercion/settype` directory moves from
3 passes, 22 failures and one unsupported case to 25 passes, zero failures and
one unsupported CLI-INI case. A final release rerun of all 5,599 pinned PHP
8.5.6 cases reaches 2,726 passes, 2,482 ordinary failures, 110 skips, one XFAIL
and 280 unsupported cases, with zero timeouts and zero crashes. The exact
pass-set delta is +22/-0; runtime is reached by 4,540 of 5,208 attempted cases
(87.174%). All five Cargo feature configurations, formatting, unsafe-policy,
all-feature/all-target, Composer S0, all four Symfony S1 and warmed-kernel S2
gates pass on AMD64. The unsafe inventory remains below its ceiling at 1,622
production blocks, and the fixed stdlib registry-capacity gate passes. No
runtime performance gate applies because these are explicit cold/new builtin
paths. Complete binary-string representation, the exact exception class for an
operating-system random-source failure, and per-process CLI-INI support for
`settype_double.phpt` remain explicit non-claims.

The CLI now accepts repeated separate or attached `-d NAME[=VALUE]` settings
and transports the admitted assertion subset through request compilation and
execution. `zend.assertions=-1` removes assertion evaluation while preserving
generator classification, mode `0` retains guarded bytecode for a later
runtime switch to mode `1`, and attempts to cross the completely-disabled
`-1` boundary at runtime emit PHP 8.5's warning and fail. Included files inherit
the startup compilation mode, `assert.exception` initializes the request-local
exception policy, and failed assertions render the newly exercised `new`,
empty anonymous-class and `instanceof` expression forms.

The complete 29-case `Zend/tests/assert` directory moves from 3 passes and 26
unsupported cases to 24 passes, four ordinary failures and one unsupported
`disable_functions` case. A final release rerun of all 5,599 pinned PHP 8.5.6
cases reaches 2,748 passes, 2,500 ordinary failures, 110 skips, one XFAIL and
240 unsupported cases, with zero timeouts and zero crashes. All 40 changed
statuses were previously unsupported: 22 become exact passes and 18 expose
later independent failures, with no lost pass. Runtime is reached by 4,576 of
5,248 attempted cases (87.195%).

All five Cargo feature configurations, formatting, unsafe-policy,
all-feature/all-target, Composer S0, all four Symfony S1 and warmed-kernel S2
gates pass on AMD64. The unsafe inventory is 1,621 production blocks against a
ceiling of 1,623. Twenty-one alternating release pairs of the unaffected
five-million-iteration call control measured baseline p10/median/p90 of
0.357937/0.365312/0.369293 seconds and candidate
0.358686/0.364591/0.369860 seconds (median -0.20%; balanced pair delta -0.09%)
with the identical `37500007500000` checksum. Unknown CLI INI directives,
`disable_functions`, the complete assertion AST source printer, backtick and
interpolated-string parsing, array-target `??=`, and the startup deprecation
contract for `assert.exception=0` remain explicit non-claims.

Assertion descriptions now render the PHP 8.5 canonical source forms retained
by the RPHP AST for floating-point literals, strings, unary and binary
operators, compound assignments, calls, `new`, closures, arrow functions,
`match`, `exit`/`die`, anonymous classes and asymmetric property visibility.
The renderer preserves multiline indentation and the parentheses required when
a closure is immediately invoked or used as a pipe operand. Two direct
regressions are built from the original PHP 8.5 assertion specimens and compare
their complete warning output.

A focused 42-case assertion-source slice moves from 24 passes, 17 failures and
one unsupported case to 33 passes, eight failures and one unsupported case. A
final release rerun of all 5,599 pinned PHP 8.5.6 cases reaches 2,757 passes,
2,491 ordinary failures, 110 skips, one XFAIL and 240 unsupported cases, with
zero timeouts and zero crashes. The exact pass-set delta is +9/-0; runtime is
still reached by 4,576 of 5,248 attempted cases (87.195%). All five Cargo
feature configurations, formatting, unsafe-policy, all-feature/all-target,
Composer S0, all four Symfony S1 and warmed-kernel S2 gates pass on AMD64. No
runtime performance gate applies because this checkpoint only changes cold
assertion-description synthesis and leaves ordinary generated code and runtime
execution unchanged. Attribute metadata discarded by the lexer, interpolated
strings lowered before source rendering, backtick parsing, array-target `??=`,
and unrelated arrow-function and implicit-nullability diagnostics remain
explicit non-claims.

Malformed numeric separators now follow PHP 8.5's lexical boundary: a base
prefix without a valid first digit leaves the prefix text as an identifier,
while trailing, doubled, fractional and exponent-adjacent underscores likewise
surface the exact unexpected identifier. Source-aware parsing emits the
canonical message, filename and line for top-level CLI input. Repeated
`eval()` and included-source failures preserve a message without duplicated
location text while `ParseError::getFile()` and `getLine()` retain the actual
origin. Source-less parser embedders keep their structural diagnostics.

The complete 11-case `Zend/tests/numeric_literal_separator` directory moves
from one pass and ten failures to 11 passes. The same general rule also admits
`Zend/tests/grammar/bug77993.phpt` and the repeated-eval
`Zend/tests/bug75252.phpt`. A final release rerun of all 5,599 pinned PHP 8.5.6
cases reaches 2,769 passes, 2,479 ordinary failures, 110 skips, one XFAIL and
240 unsupported cases, with zero timeouts and zero crashes. The exact pass-set
delta is +12/-0; runtime remains reached by 4,576 of 5,248 attempted cases
(87.195%). All five Cargo feature configurations, formatting, unsafe-policy,
all-feature/all-target, Composer S0, all four Symfony S1 and warmed-kernel S2
gates pass on AMD64. No runtime performance gate applies because valid numeric
tokens, generated bytecode and runtime dispatch are unchanged; all new work is
confined to lexing, parsing and the cold include/eval error path. Canonical
diagnostics for other unexpected token kinds and unrelated numeric-overflow
contracts remain explicit non-claims.

Heredoc and nowdoc lexing now preserves PHP 8.5's source line for mixed
tab/space indentation, shallow body indentation and missing terminators. The
diagnostics distinguish an empty document from one whose body has started,
carry the required indentation depth, and remain catchable with the correct
file and line through `eval()` and include. Historical `b<<<` and `B<<<`
prefixes are accepted, a missing opening-label quote reports the canonical
unexpected `<<` token, and an uncaught object whose exact runtime class is
`ParseError` uses PHP's `Parse error` envelope; user subclasses retain the
ordinary uncaught-throwable form.

The 65-case `Zend/tests/heredoc_nowdoc` slice moves from 28 passes, 33 failures
and four unsupported cases to 53 passes, eight failures and four unsupported
cases. A final release rerun of all 5,599 pinned PHP 8.5.6 cases reaches 2,796
passes, 2,452 ordinary failures, 110 skips, one XFAIL and 240 unsupported
cases, with zero timeouts and zero crashes. The exact pass-set delta is +27/-0:
25 cases from the focused slice plus `Zend/tests/bug69640.phpt` and
`Zend/tests/grammar/bug78363.phpt`. Runtime is reached by 4,561 of 5,248
attempted cases (86.909%). All five Cargo feature configurations, formatting,
unsafe-policy, all-feature/all-target, Composer S0, all four Symfony S1 and
warmed-kernel S2 gates pass on AMD64. No runtime performance gate applies
because valid document strings retain the existing tokens and bytecode; the
new paths are confined to lexing, parsing and cold error rendering. Nested
document interpolation, scan-ahead warning ordering and unrelated runtime
string semantics remain explicit non-claims.

Nested heredoc interpolation now keeps a recursive document boundary instead
of accepting a same-named marker that belongs to `${...}`. The scanner also
ignores label-shaped lines inside ordinary interpolated expressions while
keeping nowdoc content non-interpolated. Deprecated `${var}` and `${expr}`
forms retain their distinct PHP 8.5 variable and variable-variable semantics,
emit source-located compile-time deprecations even in dead code, and propagate
diagnostics from nested document expressions. Standalone compound statements
share their surrounding scope, including within a namespace.

The 65-case `Zend/tests/heredoc_nowdoc` slice advances from 53 to 57 passes,
with four failures and four unsupported cases; adding the adjacent deprecated
interpolation case produces 58 passes across 66 focused cases. A final release
rerun of all 5,599 pinned PHP 8.5.6 cases reaches 2,803 passes, 2,445 ordinary
failures, 110 skips, one XFAIL and 240 unsupported cases, with zero timeouts
and zero crashes. The exact pass-set delta is +7/-0: four complex heredoc
cases, both general deprecated-interpolation cases and the namespaced compound
block case. Runtime is reached by 4,565 of 5,248 attempted cases (86.986%). All
five Cargo feature configurations, formatting, unsafe-policy,
all-feature/all-target, Composer S0, all four Symfony S1 and warmed-kernel S2
gates pass on AMD64. No runtime performance gate applies: ordinary string
tokens and bytecode are unchanged, standalone blocks lower to their existing
statement sequence, and the additional work is confined to source-unit
lexing/parsing and compile-time diagnostics. Heredoc scan-ahead warning order
and unrelated runtime string behavior remain explicit non-claims.

Document strings now recognize lone `CR`, `LF` and `CRLF` as PHP line
boundaries while preserving the original bytes and flexible indentation.
Simple unbraced interpolation accepts literal, negative, identifier and
variable array indexes, including non-canonical numeric-string keys, while a
quoted index fails during parsing with its source location. A heredoc start
adjacent to an already complete value retains its own parse token instead of
becoming synthetic call parentheses, and source-unit parse failures use PHP's
255 exit status. Overflowing three-digit octal escapes keep their low byte and
emit source-ordered compile warnings before nested `${expr}` deprecations and
runtime diagnostics.

The 65-case `Zend/tests/heredoc_nowdoc` slice now has 61 passes, no ordinary
failures and four explicit unsupported CLI-INI cases. A final release rerun of
all 5,599 pinned PHP 8.5.6 cases reaches 2,811 passes, 2,437 ordinary failures,
110 skips, one XFAIL and 240 unsupported cases, with zero timeouts and zero
crashes. The exact pass-set delta is +8/-0 and has no remaining-failure stage
movement: the four formerly failing heredoc cases plus `Zend/tests/bug72918.phpt`,
`Zend/tests/numeric_strings/neg_num_string.phpt`,
`Zend/tests/oct_overflow_char.phpt` and `tests/lang/bug21820.phpt`. Runtime is
reached by 4,563 of 5,248 attempted cases (86.947%). All five Cargo feature
configurations, formatting, unsafe-policy, all-feature/all-target, Composer S0,
all four Symfony S1 and warmed-kernel S2 gates pass on AMD64. No runtime
performance gate applies: ordinary valid string bytecode and VM dispatch are
unchanged, and the new work is confined to lexing, parsing, compile diagnostics
and CLI parse termination. The four unsupported focused cases still require
the named highlighting, multibyte and encoding INI capabilities.

Malformed `\\u{...}` escapes in double-quoted strings and heredocs now use
PHP 8.5's `Invalid UTF-8 codepoint escape sequence` parse diagnostic, with the
separate `Codepoint too large` form for values above `U+10FFFF` and arithmetic
overflow. The diagnostic retains the escape's source line across multiline
double-quoted strings and document strings, while valid codepoints through
`U+10FFFF` and legacy unbraced `\\u` text keep their existing behavior.

The nine-case `tests/lang/string` Unicode-escape slice now has eight passes and
one ordinary failure. A final release rerun of all 5,599 pinned PHP 8.5.6 cases
reaches 2,817 passes, 2,431 ordinary failures, 110 skips, one XFAIL and 240
unsupported cases, with zero timeouts and zero crashes. The exact pass-set
delta is +6/-0 with no remaining-failure stage movement: the empty, incomplete,
too-large, positive-sign, negative-sign and whitespace Unicode-escape cases.
Runtime is reached by 4,563 of 5,248 attempted cases (86.947%). All five Cargo
feature configurations, formatting, unsafe-policy, all-feature/all-target,
Composer S0, all four Symfony S1 and warmed-kernel S2 gates pass on AMD64. No
runtime performance gate applies because valid string tokens, bytecode and VM
dispatch are unchanged. Surrogate-half escapes remain an explicit failure
until RPHP can preserve their non-well-formed CESU-8 bytes rather than forcing
them through Rust's UTF-8 `String` representation.

Source-level user attributes now survive lexing, parsing, namespace and alias
resolution, constant-expression evaluation and compilation into ordinary
Reflection metadata. This covers grouped attributes, positional and named
arguments, classes, interfaces, traits, enums, functions, closures, methods,
properties, parameters, class constants and source-level global constants.
`ReflectionAttribute` exposes names, arguments, targets, repetition state,
exact-name and `IS_INSTANCEOF` filtering, while `newInstance()` defers class,
target, repeatability and constructor validation to the PHP 8.5 boundary.
Reflection payloads remain in a sparse request-local side table so ordinary
objects retain their established layout and only the public `name` projection
is visible on a reflected attribute object.

The 204-case `Zend/tests/attributes` slice now has 55 passes, 141 failures,
five skips and three unsupported cases, with zero timeouts or crashes. Its
exact pass-set delta is +22/-0; 51 of 80 attempted ordinary-profile cases pass.
The full 5,599-case release rerun reaches 2,841 passes and adds two adjacent
class-name/trait-constant cases for a total delta of +24/-0. Five negative
attribute-expression cases now reject in the front end instead of executing
after the old lexer discarded their groups; their still-inexact diagnostic
text remains explicit work. Other remaining-stage movements advance within
failing attribute semantics, and no prior pass is lost.

All five Cargo feature configurations, formatting, unsafe-policy,
all-feature/all-target, Composer S0, all four Symfony S1 and warmed-kernel S2
gates pass on AMD64. Attribute fields follow every established execution field
and a layout regression test protects those offsets, but the metadata widens
the containing cold declaration records and the additional implementation
changes release code placement. Twenty alternating call and Closure-storage
pairs measured +0.549% and +0.264%. The general property control exceeded its
one-percent gate at +2.295%; an independent 40-pair CPU-pinned rerun measured
+3.427%. This is retained as explicit temporary performance debt under the
current compatibility-first priority, not as a performance-neutral claim.
Dynamic trait/closure binding scopes, deferred runtime constant expressions,
exact remaining diagnostics and full `ReflectionAttribute` immutability remain
separate compatibility work.

The built-in `Attribute` marker now has its public typed `flags` property,
defaults direct construction to `TARGET_ALL`, and preserves the self-marker's
`TARGET_CLASS` value through `ReflectionAttribute::newInstance()`. Deferred
marker validation rejects a non-integer constructor argument with PHP 8.5's
exact `TypeError` and rejects flag bits outside the target/repeatable mask with
`Invalid attribute flags specified` before target validation. The focused
attribute slice reaches 58 passes and the full release corpus reaches 2,844,
an exact +3/-0 delta with no other status or failure-stage movement. All five
Cargo configurations, formatting, unsafe-policy, all-feature/all-target,
Composer S0, all four Symfony S1 and warmed-kernel S2 gates pass on AMD64. The
work is confined to built-in startup metadata and explicit construction or
Reflection instantiation of `Attribute`; it does not change ordinary bytecode
or VM dispatch, so the preceding compatibility-first performance debt remains
the current measured control rather than being recharacterized here. Runtime
evaluation of unresolved class constants in attribute arguments remains a
separate compatibility boundary.

The matching PHP 8.5.6 CLI oracle produces 5,440 passes, zero ordinary
failures, 153 skips, one XFAIL, five unsupported SAPI sections, zero timeouts
and zero crashes. The source archive checksum, build configuration, exact
summary, manifest and coverage map are published under
`tests/php-src/results/php-8.5.6/`. The older PHP 8.2.33 and 8.4.21 results remain
historical evidence and do not define the current public contract.

Composer S0, all four Symfony S1 gates and warmed-kernel S2 pass against the
PHP 8.5 identity. A local cold-kernel S3 revalidation against an exact PHP
8.5.6 CLI did not complete: RPHP remained in the initial cold `s3-gate.php`
execution for more than ten minutes. Consequently the retained PHP 8.2 S3
result is historical evidence only, and no PHP 8.5 S3 claim is made here.

To reproduce the AMD64 RPHP run from the exact external checkout:

```sh
cargo build --locked --release
RPHP_PHPT_REFERENCE_PHP=/path/to/php-8.5.6 \
RPHP_PHPT_FEATURES=default \
RPHP_PHPT_TIMEOUT=3 scripts/run-php-src-phpt.sh \
  /path/to/php-src target/release/rphp /tmp/rphp-phpt-results 4
```

The current AMD64 request-local INI checkpoint, based on `5101a5a`, runs the
default-feature 4,345-case PHP 8.2.33 corpus and records 1,779 passes, 2,251
failures, 77 skips, one upstream XFAIL, 237 unsupported cases, zero timeouts
and zero crashes. Its exact pass-set delta is +3/-0:
`Zend/tests/bug26698.phpt`, `exception_ignore_args.phpt` and `gc_003.phpt`.

`ini_set()` now admits a bounded request-local subset, returns each previous
string value, rejects unsupported names and updates the existing GC control
state. Throwable creation honors `zend.exception_ignore_args` by omitting
arguments from the stored trace, so changing the option later cannot disclose
them. Original coverage checks the mutation and previous-value contract plus
that immutable trace behavior. A temporary depth guard converts the currently
recursive deep `yield from` bridge from a Rust stack-overflow crash into an
ordinary explicit failure; stack-safe 50,000-level delegation remains open,
as do real `memory_limit` enforcement and Fiber stack sizing.

All five Cargo feature configurations, the all-feature/all-target check,
formatting and unsafe policy pass; the production unsafe inventory remains
1,623 blocks and 289 functions. Composer S0, all four Symfony S1 gates and
warmed-kernel S2 pass on AMD64. The manually dispatched cold-kernel S3 frontier
was not rerun. Seven 500-million-addition release controls measured a
0.70-second median for both the preceding and candidate binaries, with
identical output.

The current AMD64 alias-interface identity checkpoint, based on `9376324`,
runs the default-feature 4,345-case PHP 8.2.33 corpus and records 1,776 passes,
2,254 failures, 77 skips, one upstream XFAIL, 237 unsupported cases, zero
timeouts and zero crashes. Its exact pass-set delta is +1/-0:
`Zend/tests/class_alias_009.phpt`. All 19 pinned `class_alias_*` cases now pass.

Publishing an interface alias now rechecks the canonical class IDs reachable
through every direct and inherited interface edge. A class, interface or enum
therefore cannot implement the same identity twice under original and alias
spellings; the fatal names the consumer and canonical interface. The check
walks only stable real-class IDs rather than alias table entries, skips symbols
with fewer than two edges, bounds recursive closure traversal and remains on
the cold interface-alias boundary. Original coverage checks direct alias
duplication and a transitive diamond through two differently spelled parents.

All five Cargo feature configurations, the all-feature/all-target check,
formatting and unsafe policy pass; the production unsafe inventory remains
1,623 blocks and 289 functions. Composer S0, all four Symfony S1 gates and
warmed-kernel S2 pass on AMD64. The manually dispatched cold-kernel S3 frontier
was not rerun. Nine 5,000-interface-alias release controls measured a
0.23-second median for both the preceding and candidate binaries, with
identical output.

The current AMD64 late-parent method-link checkpoint, based on `62017d3`, runs
the default-feature 4,345-case PHP 8.2.33 corpus and records 1,775 passes, 2,255
failures, 77 skips, one upstream XFAIL, 237 unsupported cases, zero timeouts
and zero crashes. Its exact pass-set delta is +2/-0:
`Zend/tests/class_alias_017.phpt` and `lsb_016.phpt`.

An exact method-table miss now follows a class's currently resolvable parent
chain. This supplies inherited instance, static, constructor and magic methods
when eager top-level registration encountered the parent before a preceding
runtime `class_alias()` or before the parent's own declaration was linked.
Ordinary inheritance remains flattened and exact-hit dispatch is unchanged;
the bounded fallback is cold and preserves child overrides. Original coverage
checks a directly late parent, an alias parent, a transitive grandchild,
constructor dispatch and canonical declaring-class identity. Duplicate
interface detection through aliases remains the separate `class_alias_009`
boundary.

All five Cargo feature configurations, the all-feature/all-target check,
formatting and unsafe policy pass; the production unsafe inventory remains
1,623 blocks and 289 functions. Composer S0, all four Symfony S1 gates and
warmed-kernel S2 pass on AMD64. The manually dispatched cold-kernel S3 frontier
was not rerun. Nine 5,000-alias release controls measured a 0.17-second median
for both the preceding and candidate binaries, with identical output.

The current AMD64 missing-class construction checkpoint, based on `4e76f45`,
runs the default-feature 4,345-case PHP 8.2.33 corpus and records 1,773 passes,
2,257 failures, 77 skips, one upstream XFAIL, 237 unsupported cases, zero
timeouts and zero crashes. Its exact pass-set delta is +5/-0:
`Zend/tests/class_alias_016.phpt`, `class_alias_020.phpt`, `ns_004.phpt`,
`prop_const_expr/lhs_class_not_found.phpt` and
`prop_const_expr/lhs_class_not_found_nullsafe.phpt`.

An unresolved class at literal or runtime-named `new` now creates PHP's normal
`Error`, attaches the physical source location and enters the ordinary
Throwable path. User code can therefore catch it, while uncaught cases render
the PHP file, line and stack trace instead of terminating through a raw VM
fatal. Original coverage checks both literal and dynamic construction and the
observable message, file and line. Alias inheritance and duplicate-interface
linking remain separate boundaries in `class_alias_017` and `_009`.

All five Cargo feature configurations, the all-feature/all-target check,
formatting and unsafe policy pass; the production unsafe inventory remains
1,623 blocks and 289 functions. Composer S0, all four Symfony S1 gates and
warmed-kernel S2 pass on AMD64. The manually dispatched cold-kernel S3 frontier
was not rerun. Nine alternating one-million-object release controls measured a
0.16-second median for both the preceding and candidate binaries, with
identical output and a 0.16--0.17-second range for each.

The current AMD64 class-alias diagnostic checkpoint, based on `91c46a8`, runs
the default-feature 4,345-case PHP 8.2.33 corpus and records 1,768 passes, 2,262
failures, 77 skips, one upstream XFAIL, 237 unsupported cases, zero timeouts
and zero crashes. Its exact pass-set delta is +6/-0:
`Zend/tests/class_alias_002.phpt`, `class_alias_004.phpt`,
`class_alias_006.phpt`, `class_alias_010.phpt`, `class_alias_014.phpt` and
`class_alias_019.phpt`.

Missing originals and alias-name collisions now use PHP's ordinary localized
`E_WARNING` path, including the active error handler and physical source
location. Collision messages retain the original declaration kind for
classes, interfaces, traits and enums. Aliasing an internal class raises the
PHP 8.2 `ValueError` with its canonical message. Original coverage checks
handled and unhandled warnings, declaration-kind diagnostics and the catchable
internal-class boundary. The remaining `class_alias_009`, `_016`, `_017` and
`_020` failures require separate namespace/linker work.

All five Cargo feature configurations, the all-feature/all-target check,
formatting and unsafe policy pass; the production unsafe inventory remains
1,623 blocks and 289 functions. Composer S0, all four Symfony S1 gates and
warmed-kernel S2 pass on AMD64. The manually dispatched cold-kernel S3 frontier
was not rerun. Seven 5,000-alias release controls measured a 0.12-second median
for both the preceding and candidate binaries, with identical output.

The current AMD64 canonical class-alias checkpoint, based on `d283542`, runs
the default-feature 4,345-case PHP 8.2.33 corpus and records 1,762 passes, 2,268
failures, 77 skips, one upstream XFAIL, 237 unsupported cases, zero timeouts
and zero crashes. Its exact pass-set delta is +2/-0:
`Zend/tests/class_alias_001.phpt` and `class_alias_008.phpt`.

Objects created through `class_alias()` retain the original declaration's
canonical class identity in non-instantiable interface, abstract-class and
enum diagnostics. A dynamic `instanceof $object` target now resolves through
the RHS object's canonical runtime class, so aliases compare as the same class
without changing object identity or strict equality. Original coverage checks
both directions of object-target `instanceof` plus runtime-named alias
instantiation. Remaining alias warning localization, namespace resolution and
inherited-method publication are separate visible boundaries.

All five Cargo feature configurations, the all-feature/all-target check,
formatting and unsafe policy pass; the production unsafe inventory remains
1,623 blocks and 289 functions. Composer S0, all four Symfony S1 gates and
warmed-kernel S2 pass on AMD64. The manually dispatched cold-kernel S3 frontier
was not rerun. Nine alternating five-million-iteration dynamic-string
`instanceof` release pairs measured a 0.36-second median for both the preceding
and candidate binaries, with identical output.

The current AMD64 throw-validation checkpoint, based on `fcbfed1`, runs the
default-feature 4,345-case PHP 8.2.33 corpus and records 1,760 passes, 2,270
failures, 77 skips, one upstream XFAIL, 237 unsupported cases, zero timeouts
and zero crashes. Its exact pass-set delta is +6/-0:
`Zend/tests/bug33318.phpt`, `exception_004.phpt`, `exception_005.phpt`,
`exception_006.phpt`, `generators/errors/generator_instantiate_error.phpt` and
`instantiate_all_classes.phpt`.

Throwing a scalar or a non-Throwable object now raises PHP's ordinary catchable
`Error` at the throw opcode, with the PHP 8.2 message and source metadata.
Attempts to instantiate interfaces, abstract classes or the internal-only
`Generator` class use the same catchable error path at `new`. Uncaught cases
therefore share normal Throwable file, line and trace rendering, while caught
cases preserve surrounding finally/control flow. Original coverage checks
scalar, null and object operands plus literal and runtime-named non-instantiable
classes. Canonical naming through a noncanonical `class_alias()` remains a
separate visible boundary.

All five Cargo feature configurations, the all-feature/all-target check,
formatting and unsafe policy pass; the production unsafe inventory remains
1,623 blocks and 289 functions. Composer S0, all four Symfony S1 gates and
warmed-kernel S2 pass on AMD64. The manually dispatched cold-kernel S3 frontier
was not rerun. Nine alternating 200,000-iteration valid caught-exception
release pairs measured a 0.15-second median for both the preceding and
candidate binaries, with identical output.

The current AMD64 Throwable-chain rendering checkpoint, based on `106fa63`,
runs the default-feature 4,345-case PHP 8.2.33 corpus and records 1,754 passes,
2,276 failures, 77 skips, one upstream XFAIL, 237 unsupported cases, zero
timeouts and zero crashes. Its exact pass-set delta is +3/-0:
`Zend/tests/exception_007.phpt`, `throwable_001.phpt` and
`throwable_002.phpt`.

An uncaught Throwable now follows the private `previous` chain and renders it
from the oldest cause to the final exception. Every segment retains its own
class, optional message, creation file, line and trace; later segments use
PHP's `Next` separator, while the final `thrown` location belongs only to the
outermost exception. Identity tracking bounds malformed cyclic chains without
changing ordinary acyclic output. Original coverage checks mixed Error and
Exception subclasses, empty messages, ordering and per-segment origins.

All five Cargo feature configurations, the all-feature/all-target check,
formatting and unsafe policy pass; the production unsafe inventory remains
1,623 blocks and 289 functions. Composer S0, all four Symfony S1 gates and
warmed-kernel S2 pass on AMD64. The manually dispatched cold-kernel S3 frontier
was not rerun. Nine alternating 200,000-iteration caught-exception release
pairs measured a 0.15-second median for both the preceding and candidate
binaries, with identical output.

The current AMD64 `ErrorException` metadata checkpoint, based on `fd8efb2`,
runs the default-feature 4,345-case PHP 8.2.33 corpus and records 1,751 passes,
2,279 failures, 77 skips, one upstream XFAIL, 237 unsupported cases, zero
timeouts and zero crashes. Its exact pass-set delta is +3/-0:
`Zend/tests/ErrorException_construct.phpt`, `bug41209.phpt` and
`undef_var_in_verify_return.phpt`.

`ErrorException` now participates in the built-in Throwable hierarchy with its
protected severity state, `getSeverity()` and PHP's six-parameter constructor.
The constructor preserves its creation origin by default, applies nullable
filename and line overrides in PHP order, retains the previous throwable, and
accepts the public named arguments. Original coverage checks default, partial,
complete and named metadata forms. The separate upstream `getSeverity()` PHPT
remains classified unsupported because it requires the not-yet-admitted CLI
per-test INI surface.

All five Cargo feature configurations, the all-feature/all-target check,
formatting and unsafe policy pass; the production unsafe inventory remains
1,623 blocks and 289 functions. Composer S0, all four Symfony S1 gates and
warmed-kernel S2 pass on AMD64. The manually dispatched cold-kernel S3 frontier
was not rerun. Nine alternating 200,000-instance ordinary `Exception`
construction release pairs measured medians of 0.13 seconds for the preceding
binary and 0.12 seconds for the candidate, with identical output.

The current AMD64 detached-callback reference-capture checkpoint, based on
`dd49a9a`, runs the default-feature 4,345-case PHP 8.2.33 corpus and records
1,748 passes, 2,282 failures, 77 skips, one upstream XFAIL, 237 unsupported
cases, zero timeouts and zero crashes. Its exact pass-set delta is +1/-0:
`Zend/tests/bug79793.phpt`.

Borrowed callback invocation now preserves the identity of explicit trailing
closure reference captures while continuing to clone public call arguments by
value. Mutations made through `use (&$value)` therefore survive detached
invocation by error handlers and internal callback consumers. Original
coverage exercises both `set_error_handler()` and `array_map()`.

All five Cargo feature configurations, the all-feature/all-target check,
formatting and unsafe policy pass; the production unsafe inventory remains
1,623 blocks and 289 functions. Composer S0, all four Symfony S1 gates and
warmed-kernel S2 pass on AMD64. The manually dispatched cold-kernel S3 frontier
was not rerun. Nine alternating 500,000-iteration `array_map()` release pairs
with a by-value closure capture measured a 0.20-second median for both the
preceding and candidate binaries, with identical output.

The current AMD64 compound-array-root diagnostic checkpoint, based on
`fb19845`, runs the default-feature 4,345-case PHP 8.2.33 corpus and records
1,747 passes, 2,283 failures, 77 skips, one upstream XFAIL, 237 unsupported
cases, zero timeouts and zero crashes. Its exact pass-set delta is +1/-0:
`Zend/tests/assign_dim_op_undef.phpt`.

Array compound assignment and array-element increment/decrement now diagnose
an undefined root before reading the key and missing dimension. The lowering
keeps this read-modify-write contract separate from silent mutation and
reference-materialization contexts, so passing an undefined element by
reference still initializes it without the root warning. Original coverage
checks the PHP order `undefined root`, `undefined key variable`, `undefined
array key` and the resulting autovivified element.

All five Cargo feature configurations, the all-feature/all-target check,
formatting and unsafe policy pass; the production unsafe inventory remains
1,623 blocks and 289 functions. Composer S0, all four Symfony S1 gates and
warmed-kernel S2 pass on AMD64. The manually dispatched cold-kernel S3 frontier
was not rerun. Nine alternating five-million-iteration defined array-compound
release pairs measured a 0.08-second median for both the preceding and
candidate binaries, with identical output.

The current AMD64 output-string-conversion checkpoint, based on `cb92f68`,
runs the default-feature 4,345-case PHP 8.2.33 corpus and records 1,746 passes,
2,284 failures, 77 skips, one upstream XFAIL, 237 unsupported cases, zero
timeouts and zero crashes. Its exact pass-set delta is +2/-0:
`Zend/tests/closure_015.phpt` and `die_string_cast_exception.phpt`.

`echo`, `print` and `exit`/`die` now share PHP string-conversion behavior.
Objects invoke `__toString`, propagate a method exception before validating the
result, require a string return, and otherwise raise a catchable `Error` rather
than printing an internal object description. Closures use the same named
conversion error. Echoing an array reports `Array to string conversion` before
writing `Array`, and a throwing user error handler prevents the write. Echo
bytecode retains its source line for the diagnostic. Original coverage checks
successful, missing and invalid `__toString` paths plus warning-handler order.

All five Cargo feature configurations, the all-feature/all-target check,
formatting and unsafe policy pass; the production unsafe inventory remains
1,623 blocks and 289 functions. Composer S0, all four Symfony S1 gates and
warmed-kernel S2 pass on AMD64. The manually dispatched cold-kernel S3 frontier
was not rerun. Nine alternating five-million-write generic-echo release pairs
measured a 0.15-second median for both the preceding and candidate binaries,
with identical SHA-256 output.

The current AMD64 array-to-string diagnostic checkpoint, based on `b1d35db`,
runs the default-feature 4,345-case PHP 8.2.33 corpus and records 1,744 passes,
2,286 failures, 77 skips, one upstream XFAIL, 237 unsupported cases, zero
timeouts and zero crashes. Its exact pass-set delta is +5/-0:
`Zend/tests/assign_concat_array_empty_string.phpt`, `bug37811.phpt`,
`cast_to_string.phpt`, `settype_string.phpt` and
`temporary_cleaning_015.phpt`.

Explicit string casts, `strval()`, `settype()`, concatenation and compound
concatenation now report PHP's `Array to string conversion` warning before
producing `Array`. Internal conversion preserves PHP's mutation order when a
user error handler throws. Object conversions share the ordinary `__toString`
call path, require a string result, and propagate a method exception before
return-type validation. Cast bytecode now retains its source line. Original
coverage exercises array and object casts, both internal functions and the
throwing-handler mutation boundary.

All five Cargo feature configurations, the all-feature/all-target check,
formatting and unsafe policy pass; the production unsafe inventory remains
1,623 blocks and 289 functions. Composer S0, all four Symfony S1 gates and
warmed-kernel S2 pass on AMD64. The optional cold-kernel S3 frontier was not
accepted for this checkpoint: a local rerun blocked on its cache lock and was
stopped without a result. That manually dispatched frontier remains outside
the ordinary push CI. Seven five-million-iteration constant-string concat
release controls measured medians of 0.35 seconds for the preceding binary and
0.36 seconds for the candidate, with identical output; the difference is within
the observed 0.34--0.43-second run spread.

The current AMD64 arithmetic-operator-error checkpoint, based on `6402dd1`,
runs the default-feature 4,345-case PHP 8.2.33 corpus and records 1,739 passes,
2,291 failures, 77 skips, one upstream XFAIL, 237 unsupported cases, zero
timeouts and zero crashes. Its exact pass-set delta is +6/-0:
`Zend/tests/bug76667.phpt`, `compound_assign_with_numeric_strings.phpt`,
`div_by_zero_compound_refcounted.phpt`,
`div_by_zero_compound_with_conversion.phpt`, `shift_001.phpt` and
`shift_002.phpt`.

`DivisionByZeroError` and `ArithmeticError` now have their PHP parent classes
and inherited Throwable methods. Division and modulo zero paths create located,
catchable exceptions before compound writeback, preserving the target value.
Bit shifts reject negative distances with `ArithmeticError`, return zero or a
sign-filled result at and beyond the 64-bit width, warn for numeric-prefix
strings, and reject unsupported operand types with `TypeError`. Native double
and modulo regions still side-exit to this canonical behavior.

All five Cargo feature configurations, the all-feature/all-target check,
formatting and unsafe policy pass; the production unsafe inventory remains
1,619 blocks and 289 functions. Composer S0, all four Symfony S1 gates,
warmed-kernel S2 and cold-kernel S3 pass on AMD64. Nine alternating
five-million-iteration shift controls measured a 0.05-second median for both
the preceding and candidate binaries with identical output.

The current AMD64 typed-reference assignment-result checkpoint, based on
`d59fa83`, runs the default-feature 4,345-case PHP 8.2.33 corpus and records
1,733 passes, 2,297 failures, 77 skips, one upstream XFAIL, 237 unsupported
cases, zero timeouts and zero crashes. Its exact pass-set delta is +1/-0:
`Zend/tests/assign_typed_ref_result.phpt`.

When an array element aliases a typed property, weak type coercion now updates
both the stored reference cell and the value produced by the assignment
expression. A compiler flag limits result writeback to the dedicated temporary
of value-producing array assignments, so statement assignments do not mutate
their source variable. Original E2E coverage includes direct, flat-array and
nested-array writes and verifies that the RHS CV retains its original type.

All five Cargo feature configurations, the all-feature/all-target check,
formatting and unsafe policy pass; the production unsafe inventory is 1,619
blocks and 289 functions. Composer S0, all four Symfony S1 gates, warmed-kernel
S2 and cold-kernel S3 pass on AMD64. Nine alternating five-million-element
array-write release controls measured 0.04 seconds for both the preceding and
candidate binaries, and the AMD64 native JIT suites remain green.

The current AMD64 typed-instance-reference checkpoint, based on `b8d6dd0`,
runs the default-feature 4,345-case PHP 8.2.33 corpus and records 1,722 passes,
2,308 failures, 77 skips, one upstream XFAIL, 237 unsupported cases, zero
timeouts and zero crashes. Its exact pass-set delta is +11/-0:
`Zend/tests/bug35239.phpt`, `Zend/tests/bug68191.phpt`,
`Zend/tests/bug71539_1.phpt`, `Zend/tests/objects_019.phpt`,
`Zend/tests/type_declarations/typed_properties_066.phpt`,
`typed_properties_071.phpt`, `typed_properties_076.phpt`,
`typed_properties_081.phpt`, `typed_properties_096.phpt`,
`typed_properties_106.phpt` and
`typed_properties_reference_coercion_leak.phpt`.

Typed declared instance properties now attach their contract to an owned
reference cell, compose multiple property types as an intersection, coerce
valid weak assignments and reject incompatible writes before mutation. Rebind,
unset and object destruction remove exactly that property's ownership; object
clone preserves a genuinely aliased property cell and its independent typed
owner, while an unaliased historical reference wrapper is copied by value.
Runtime-named declared-property write caches compare the current name with the
cached slot before reuse, preventing one call site from writing a later name
into an earlier slot. Original E2E coverage includes nullable/non-nullable
initialization, weak conversion, incompatible intersections, runtime-name cache
reuse, rebind, clone and destruction.

All five Cargo feature configurations, the all-feature/all-target check,
formatting and unsafe policy pass; the production unsafe inventory is 1,616
blocks and 289 functions. Composer S0, all four Symfony S1 gates and warmed
kernel S2 also pass. The broader cold-kernel S3 gate currently fails in the
same `PhpDumper` service-map path on both base `b8d6dd0` and this candidate, so
it is not attributed to or claimed by this checkpoint. Thirty-one alternating
pinned-core release pairs measured untyped fixed-name instance writes at
baseline p10/median/p90 0.169407/0.173816/0.181983 seconds and candidate
0.173159/0.176327/0.200415 seconds (+1.44 percent median), typed fixed-name
writes at 0.168438/0.172390/0.183697 and
0.171785/0.174767/0.190799 seconds (+1.38 percent), and runtime-name writes at
0.677460/0.681391/0.691501 and 0.711077/0.713742/0.732626 seconds (+4.75
percent). Checksums are identical. The localized runtime-name cost is retained
for exact slot selection; magic `__get` references remain a separate cluster.

The current AMD64 typed-reference-constraint checkpoint, based on `7aa97ae`,
runs the default-feature 4,345-case PHP 8.2.33 corpus and records 1,711 passes,
2,319 failures, 77 skips, one upstream XFAIL, 237 unsupported cases, zero
timeouts and zero crashes. It adds the exact passes
`Zend/tests/type_declarations/typed_properties_068.phpt` and
`Zend/tests/type_declarations/typed_properties_082.phpt`, plus compound static
property reference assignment in
`Zend/tests/type_declarations/typed_properties_102.phpt`, without losing a
previous pass. The compact 16-byte `Value` layout is unchanged: the existing
heap-owned reference cell now carries the typed-property constraints of every
property alias that holds it. Compatible nullable and union constraints compose
as an intersection; incompatible property types reject rebinding before either
cell changes. Rebinding a property removes only that property's constraint from
the old cell.

Writes through CV, compound/inc-dec, array, dynamic-variable and global aliases
validate and weakly coerce against the shared constraints before mutation.
Diagnostics distinguish an ordinary property write from a write through a
reference held by a typed property, and call results declared by reference can
become the property cell without a value-only detour. Original E2E tests cover
coercion, rejection without mutation, multiple compatible and incompatible
properties, call-return binding, escaped container/global aliases and
constraint removal after rebind.

The five Cargo feature configurations, all-target check, formatting and unsafe
policy pass, as do Composer S0, all four Symfony S1 gates and warmed-kernel S2
on AMD64. Consolidating the touched dispatch regions lowers the production
unsafe inventory from 1,623 to 1,614 blocks while retaining 289 unsafe
functions. Twenty-one alternating release pairs measured
`bench_binary_assign_loop.php` at baseline p10/median/p90 of
0.742980/0.765674/0.785737 seconds and candidate
0.724232/0.739269/0.776922 seconds; `bench_static_self_property.php` at
0.157578/0.161757/0.163799 and 0.156428/0.160949/0.164837 seconds; and
`bench_calls.php` at 0.340791/0.346270/0.351421 and
0.337340/0.342806/0.351303 seconds. The additional reference-state guard on
static writes is measurable: `bench_static_self_property_write.php` moves from
a 0.112475-second median to 0.113967 seconds (+1.33 percent), and its typed
counterpart moves from 0.113995 to 0.115863 seconds (+1.64 percent). Every
result checksum is identical. This bounded cost is retained for baseline
correctness; optimizing it must not bypass constraint attachment. Typed
instance-property reference acquisition and the static-local reference-return
edge remain separate compatibility clusters.

The current AMD64 static-property-reference checkpoint, based on `4b818a0`,
runs the default-feature 4,345-case PHP 8.2.33 corpus and records 1,708 passes,
2,322 failures, 77 skips, one upstream XFAIL, 237 unsupported cases, zero
timeouts and zero crashes. It adds the exact pass
`Zend/tests/type_declarations/typed_properties_087.phpt` without losing a
previous pass. Static properties can now be acquired and rebound through the
same owned reference cells as other mutable l-values, including inherited,
dynamic-owner and dynamic-name forms. Acquiring an uninitialized nullable or
untyped property initializes it to `null`; an uninitialized non-nullable
property throws PHP's dedicated `Error` instead. Reference assignment still
validates the initial value before replacing typed property storage, so a
failed call-source assignment leaves the property uninitialized.

Original E2E tests cover shared writes through base and child names, dynamic
owner/name rebinding, nullable initialization, the non-nullable diagnostic and
failed typed initialization. The five Cargo feature configurations, all-target
check, formatting and unsafe-policy gates pass, as do Composer S0, all four
Symfony S1 component gates and warmed-kernel S2 on AMD64. The production unsafe
inventory is 1,623 blocks and 289 functions, exactly within the existing
ceilings. Forty-one alternating release pairs measured
`bench_static_self_property.php` at baseline p10/median/p90 of
0.158443/0.164086/0.168612 seconds and candidate
0.156718/0.161713/0.167562 seconds (median -1.45 percent), with identical
results. Full typed-reference constraints, including the remaining
`typed_properties_068.phpt` and `typed_properties_082.phpt`, require reference-
cell type metadata and remain a separate compatibility slice. S3 was not
rerun at that checkpoint; its pre-existing cold `PhpDumper` array-state
divergence was the next boundary.

The current AMD64 typed-property object-name checkpoint, based on `d35d125`,
runs the default-feature 4,345-case PHP 8.2.33 corpus and records 1,732
passes, 2,298 failures, 77 skips, one upstream XFAIL, 237 unsupported cases,
zero timeouts and zero crashes. Relative to the exact 1,725-pass base it adds
seven passes without losing a previous pass:
`intersection_types/assigning_intersection_types.phpt`,
`intersection_types/typed_reference.phpt`, `typed_properties_004.phpt`,
`typed_properties_005.phpt`, `typed_properties_039.phpt`,
`typed_properties_078.phpt` and `typed_properties_079.phpt`.

Typed-property and property-reference errors now name the assigned object's
concrete runtime class instead of the generic `object` value kind. Anonymous
class identities keep their private numeric suffix internally while property
diagnostics, trace arguments and trace callables expose PHP's stable
`class@anonymous` name. The formatter reads the immutable class name without
borrowing property storage, preserving self-assignment and re-entrant object
boundaries. Original tests cover ordinary, anonymous and intersection-typed
property failures plus anonymous callable traces. All five Cargo feature
configurations, all-target checking, unsafe policy, Composer S0 and Symfony S1
through S3 pass on AMD64; the existing native JIT controls remain green.

The preceding AMD64 Symfony S3 service-map checkpoint, based on `2b2b6df`,
passes the complete pinned FrameworkBundle 7.4.16 cold-build gate against PHP
8.2.33. Runtime-resolved method calls now preserve a mutable `$this` property
when the selected parameter is by-reference while retaining ordinary by-value
call behavior. List and short destructuring of null and non-array scalar values
produce null elements without ordinary offset warnings; missing keys in actual
arrays retain their diagnostics. These two general PHP contracts remove the
`PhpDumper` service-call and synthetic-service warnings without vendor patches.

The default-feature 4,345-case PHP 8.2.33 corpus records 1,725 passes, 2,305
failures, 77 skips, one upstream XFAIL, 237 unsupported cases, zero timeouts
and zero crashes. Relative to the exact `2b2b6df` base it adds three passes
without losing a previous pass: `Zend/tests/bug39304.phpt`,
`Zend/tests/foreach_list_002.phpt` and `Zend/tests/list_005.phpt`. Formatting,
unsafe policy, all-target/all-feature checking, all five Cargo CI feature
configurations, Composer S0 and Symfony S1 through S3 pass on AMD64. The
existing native order-pipeline control also remains compiled and green; a
broader eager property-reference approach was rejected because it displaced
that established native region. Runtime-resolved external object-property
arguments remain a separate compatibility boundary.

The preceding AMD64 static-property-origin checkpoint, based on `8e1fb7b`, runs
the default-feature 4,345-case PHP 8.2.33 corpus and records 1,707 passes,
2,323 failures, 77 skips, one upstream XFAIL, 237 unsupported cases, zero
timeouts and zero crashes. It adds four exact passes without losing a previous
pass: `Zend/tests/exception_013.phpt`, `Zend/tests/objects_029.phpt`,
`Zend/tests/objects_030.phpt` and `tests/lang/041.phpt`. Static-property source
lines now survive the parser and every ordinary, dynamic, coalesce, foreach,
array-root and unset compiler path, so the existing shared throwable boundary
records the correct file, line and trace for read, write and unset errors.
Original E2E coverage checks undeclared, inaccessible and uninitialized reads,
an undeclared write and unset against the exact PHP 8.2.33 result. The PHPT
runner also classifies a rendered `Fatal error: Uncaught ...` as runtime before
scanning conservative compile keywords; this corrects 49 false compile labels
caused by words in source paths, without changing their pass/fail status. The
execution profile therefore records 509 front-end rejections and 3,521 runtime-
reaching cases instead of the previous 561 and 3,469.

Five Cargo configurations, the two default-feature generics combinations,
all-target, formatting and unsafe checks, Composer S0, Symfony S1 and warmed-
kernel S2 pass on AMD64; the production unsafe inventory remains 1,621 blocks
and 289 functions within the 1,623/289 ceilings. An exact PHP 8.2.33 S3 run
still exposes the pre-existing cold `PhpDumper` array-state divergence on both
the parent and candidate, so this checkpoint makes no new S3 claim. Forty-one
alternating release pairs measured `bench_static_self_property.php` at baseline
p10/median/p90 of 0.160518/0.164225/0.169785 seconds and candidate
0.159540/0.163147/0.168556 seconds (median -0.66 percent), with identical
results. Successful static-property opcodes, inline caches, runtime frames and
value/object layouts are unchanged; only sparse compiler source metadata is
added. Static-property reference binding and the cold Symfony array-state
boundary remain separate compatibility work.

The current AMD64 property-modify-origin checkpoint, based on `3c889f9`, runs
the default-feature 4,345-case PHP 8.2.33 corpus and records 1,703 passes,
2,327 failures, 77 skips, one upstream XFAIL, 237 unsupported cases, zero
timeouts and zero crashes. It adds three exact passes without losing a previous
pass: `Zend/tests/bug38624.phpt`, `Zend/tests/closure_038.phpt` and
`Zend/tests/closure_039.phpt`. The shared mutable-property source/writeback
compiler now carries the property-token line through `FetchObjR` and
`AssignObjProp`, including dynamic properties, so compound and increment/
decrement visibility failures retain their creation origin. Throwable trace
formatting additionally identifies an anonymous closure frame with a live
bound `$this` as `Closure->{closure}()`, while unbound closures retain the
ordinary `{closure}()` form. Original E2E coverage checks the exact origin and
bound-Closure trace for an inaccessible pre-increment. Five Cargo test
configurations, all-target and unsafe checks, Composer S0, Symfony S1 and
warmed-kernel S2 pass on AMD64; the production unsafe inventory remains 1,621
blocks, below the 1,623 ceiling. The S3 cold-kernel gate was not rerun because
this machine has no exact PHP 8.2 oracle. Forty-one alternating release pairs
measured `bench_property.php` at baseline p10/median/p90 of
0.739691/0.744723/0.757043 seconds and candidate
0.752086/0.759542/0.775840 seconds (median +1.99 percent), with identical
checksums. No successful property opcode, cache, frame, object or value layout
changed; the measured binary-layout movement is retained as explicit temporary
performance debt under the current compatibility-first priority. Static-
property origins, deeper multi-detached trace extension and remaining mutable
array/static writebacks are separate compatibility boundaries.

The preceding AMD64 detached-throwable-trace checkpoint, based on `b8f28d3`,
runs the default-feature 4,345-case PHP 8.2.33 corpus and records 1,700 passes,
2,330 failures, 77 skips, one upstream XFAIL, 237 unsupported cases, zero
timeouts and zero crashes. It adds two exact passes without losing a previous
pass: `Zend/tests/bug48248.phpt` and `Zend/tests/bug76025.phpt`. When a detached
user callback creates a located throwable, its frame is now temporarily
reconnected to the suspended caller before cleanup so an otherwise empty
creation trace records both the callback and its real call site. The linkage is
restored before returning, and an existing non-empty trace from a deeper frame
remains immutable. This covers magic-property methods and error handlers while
preserving the detached `Return` boundary. Original E2E coverage checks the
exact uncaught file, line, method name, argument, call-site frame and `{main}`
sentinel for an error raised inside `__get`. Five Cargo test configurations,
all-target and unsafe checks, Composer S0, Symfony S1 and warmed-kernel S2 pass
on AMD64; the production unsafe inventory remains 1,621 blocks, below the
1,623 ceiling. The S3 cold-kernel gate was not rerun because this machine has
no exact PHP 8.2 oracle. Sixty-one alternating release pairs measured
`bench_callback_array_walk_by_ref.php` at baseline p10/median/p90 of
0.013402/0.013677/0.014404 seconds and candidate
0.013379/0.013764/0.014357 seconds (median +0.63 percent), with identical
checksums; the movement is within the overlapping distribution. Runtime frame,
object and value layouts are unchanged, and successful callbacks execute only
one additional not-taken error check. Extending an already non-empty trace
across multiple detached boundaries, Closure property-inc/dec errors and
static-property origins remain separate compatibility boundaries.

The preceding AMD64 property-write-origin checkpoint, based on `43c522d`, runs
the default-feature 4,345-case PHP 8.2.33 corpus and records 1,698 passes,
2,332 failures, 77 skips, one upstream XFAIL, 237 unsupported cases, zero
timeouts and zero crashes. It adds three exact passes without losing a
previous pass: `Zend/tests/objects_017.phpt`,
`Zend/tests/type_declarations/typed_properties_029.phpt` and
`Zend/tests/type_declarations/typed_properties_060.phpt`. Ordinary and dynamic
instance-property assignment and reference-binding forms now preserve their
property-token source line from the AST through `AssignObjProp` and
`BindObjPropRef` bytecode. Catchable and uncaught visibility and typed-property
errors consequently expose the correct file, line and root trace instead of
line zero or an unlocated fatal. Original E2E coverage inspects the emitted
source-line table and validates runtime origins for read, write and reference
binding. Five Cargo test configurations, all-target and unsafe checks,
Composer S0, Symfony S1 and warmed-kernel S2 pass on AMD64; the production
unsafe inventory remains 1,621 blocks, below the 1,623 ceiling. The S3
cold-kernel gate was not rerun because this machine has no exact PHP 8.2
oracle. Twenty-one alternating release pairs measured
`bench_instance_property_write.php` at baseline p10/median/p90 of
0.165227/0.168968/0.171955 seconds and candidate
0.160270/0.167953/0.172789 seconds (median -0.60 percent), with identical final
property values. The runtime object/value and inline-cache layouts are
unchanged; the compiler stores sparse metadata only for the newly located
opcodes. Nested magic-method and Closure trace propagation, static-property
origins and remaining writeback forms are separate compatibility boundaries.

The preceding AMD64 instance-property-visibility-error checkpoint, based on
`17442b4`, runs the default-feature 4,345-case PHP 8.2.33 corpus and records
1,695 passes, 2,335 failures, 77 skips, one upstream XFAIL, 237 unsupported
cases, zero timeouts and zero crashes. It adds four exact passes without losing
a previous pass: `Zend/tests/bug29674.phpt`, `Zend/tests/closure_020.phpt`,
`Zend/tests/exception_014.phpt` and
`Zend/tests/readonly_props/visibility_change.phpt`. Inaccessible instance
property reads, writes and by-reference binds now raise a regular PHP `Error`
through the shared exception machinery instead of escaping as an internal VM
fatal. They can therefore be caught, inspected and rendered as uncaught
throwables; reads retain their exact opcode file, line and stack trace.
Original E2E coverage exercises all three operations, catchability, messages
and read-origin metadata. Five Cargo test configurations, all-target and
unsafe checks, Composer S0, Symfony S1 and warmed-kernel S2 pass on AMD64; the
production unsafe inventory remains 1,621 blocks, below the 1,623 ceiling.
The S3 cold-kernel gate was not rerun because this machine has no exact PHP
8.2 oracle. The change is confined to cold property-error branches and does
not alter successful property caches, object/value layouts, callback dispatch
or JIT plans. Assignment and reference-binding opcodes still need complete
source-line metadata, while remaining Closure binding traces, static-property
origins and property-hook behavior are separate compatibility boundaries.

The preceding AMD64 Closure-class-identity checkpoint, based on `7a0bf97`, runs
the default-feature 4,345-case PHP 8.2.33 corpus and records 1,691 passes,
2,339 failures, 77 skips, one upstream XFAIL, 237 unsupported cases, zero
timeouts and zero crashes. It adds two exact passes without losing a previous
pass: `Zend/tests/bug52060.phpt` and `Zend/tests/bug77627.phpt`. Closure values
now expose the built-in `Closure` class consistently through `get_class()`,
case-insensitive `method_exists()`, `is_a()`, `is_subclass_of()`,
`class_implements()`, `class_parents()` and `class_uses()`. Original E2E
coverage checks class naming, `__invoke` declaration lookup, case folding,
same-class versus subclass identity and the empty relation sets. The larger
`Zend/tests/closure_020.phpt` now reaches the expected `is_a()` result and
remains blocked only by the separate private-property fatal diagnostic and
throwable-origin contract. Five Cargo test configurations, all-target and
unsafe checks, Composer S0, Symfony S1 and warmed-kernel S2 pass on AMD64; the
production unsafe inventory remains 1,621 blocks, below the 1,623 ceiling.
The S3 cold-kernel gate was not rerun because this machine has no exact PHP
8.2 oracle. The change is confined to explicitly requested introspection
builtins and does not alter `Value`, `PhpClosure`, callback dispatch or JIT
layouts; ordinary call-path performance is therefore outside the changed
execution path. Closure object-handle reuse, private-property throwable
origins, Fiber/SPL dependencies and remaining binding diagnostics are separate
compatibility boundaries.

The preceding AMD64 explicit-Closure-invocation checkpoint, based on `172d52f`,
runs the default-feature 4,345-case PHP 8.2.33 corpus and records 1,689 passes,
2,341 failures, 77 skips, one upstream XFAIL, 237 unsupported cases, zero
timeouts and zero crashes. It adds three exact passes without losing a
previous pass: `Zend/tests/bug69212.phpt`,
`Zend/tests/call_user_func_003.phpt` and `Zend/tests/closure_013.phpt`.
`Closure::__invoke()` is now a registered variadic method whose cold handler
forwards positional and named arguments through the same receiver-, scope- and
capture-aware callback path as ordinary Closure invocation. Direct and dynamic
case-insensitive method syntax works for anonymous closures and first-class
bound method closures, including private method scope. Original E2E coverage
checks default and named arguments, lexical captures, dynamic method spelling
and a bound private method. Five Cargo test configurations, all-target and
unsafe checks, Composer S0, Symfony S1 and warmed-kernel S2 pass on AMD64; the
production unsafe inventory remains 1,621 blocks, below the 1,623 ceiling. The
S3 cold-kernel gate was not rerun because this machine has no exact PHP 8.2
oracle. The preceding binary rejects the new
`bench_closure_explicit_invoke.php` workload; 21 candidate runs measured
p10/median/p90 of 0.106465/0.108349/0.110535 seconds with one stable checksum.
As an unaffected call-path control, 21 alternating release pairs measured
`bench_calls.php` at baseline 0.333776/0.338475/0.341823 seconds and candidate
0.334091/0.336399/0.340974 seconds (median -0.61 percent), with identical
computed results. Complete by-reference invocation, Closure `is_a` identity,
private-property throwable origins, Fiber/SPL dependencies and remaining
binding diagnostics are separate compatibility boundaries.

The current AMD64 Closure-debug-info checkpoint, based on `d3977b4`, runs the
default-feature 4,345-case PHP 8.2.33 corpus and records 1,686 passes, 2,344
failures, 77 skips, one upstream XFAIL, 237 unsupported cases, zero timeouts
and zero crashes. It adds 12 exact passes without losing a previous pass:
`Zend/tests/bug60738.phpt`, `Zend/tests/bug60738_variation.phpt`,
`Zend/tests/bug70321.phpt`, `Zend/tests/bug75290.phpt`,
`Zend/tests/bug81076.phpt`, `Zend/tests/closure_026.phpt`,
`Zend/tests/closure_034.phpt`, `Zend/tests/closure_035.phpt`,
`Zend/tests/first_class_callable_optimization.phpt`,
`Zend/tests/gh8083.phpt`, `Zend/tests/return_types/011.phpt` and
`Zend/tests/return_types/012.phpt`. `var_dump()` now treats Closure values as
PHP objects with stable recursion identity and derives PHP 8.2's `function`,
`static`, `this` and `parameter` debug properties from registered function,
capture, binding and signature metadata. Named user, internal and method
closures retain their callable name; anonymous closures omit it. Captured and
function-static values keep reference/recursion behavior, while undefined
implicit arrow captures do not create false debug entries. Original E2E
coverage checks named and method closures, a bound receiver, lexical captures
and required/optional parameters. The compact `PhpClosure` layout is unchanged.
Five Cargo test configurations, all-target and unsafe checks, Composer S0,
Symfony S1 and warmed-kernel S2 pass on AMD64; the production unsafe inventory
is 1,621 blocks, below the 1,623 ceiling. The S3 cold-kernel gate was not rerun
because this machine has no exact PHP 8.2 oracle. Twenty-one alternating
release pairs measured `bench_closure_storage.php` at baseline p10/median/p90
of 0.008086/0.008294/0.008744 seconds and candidate
0.007766/0.008222/0.008796 seconds (median -0.87 percent), with identical
computed results. Constant-AST static placeholders, Closure object-handle
reuse, direct `->__invoke()` and later PHP source-location debug fields remain
separate compatibility boundaries.

The current AMD64 Closure-`__invoke`-array checkpoint, based on `84e1270`,
runs the default-feature 4,345-case PHP 8.2.33 corpus and records 1,674 passes,
2,356 failures, 77 skips, one upstream XFAIL, 237 unsupported cases, zero
timeouts and zero crashes. It adds the exact pass `Zend/tests/bug78689.phpt`
without losing a previous pass. A two-element `[Closure, "__invoke"]` array is
now a regular callback for direct dynamic calls, stdlib callback consumers and
`Closure::fromCallable()`, including case-insensitive method spelling, lexical
captures and strict preservation of the original Closure identity. Other
method names retain PHP's `Call to undefined method Closure::method()` error.
Original E2E coverage checks direct and `call_user_func` invocation,
`is_callable`, identity, captures, case folding and the negative diagnostic.
Five Cargo test configurations, all-target and unsafe checks, Composer S0,
Symfony S1 and warmed-kernel S2 pass on AMD64; the production unsafe inventory
remains 1,621 blocks, below the 1,623 ceiling. The S3 cold-kernel gate was not
rerun because this machine has no exact PHP 8.2 oracle. The preceding binary
rejects the newly added `bench_closure_invoke_array.php` workload; 21 candidate
runs measured p10/median/p90 of 0.030047/0.030429/0.030881 seconds with one
stable checksum. As an unaffected call-path control, 21 alternating release
pairs measured `bench_calls.php` at baseline 0.331043/0.335047/0.339307 seconds
and candidate 0.331849/0.335320/0.339843 seconds (median +0.08 percent), with
identical computed results. General Closure reflection metadata and the
remaining object-`__invoke`, binding and reference-warning cases remain
separate compatibility boundaries.

The current AMD64 `ReflectionFunction::getClosure()` checkpoint, based on
`2fd6190`, runs the default-feature 4,345-case PHP 8.2.33 corpus and records
1,673 passes, 2,357 failures, 77 skips, one upstream XFAIL, 237 unsupported
cases, zero timeouts and zero crashes. It adds the exact pass
`Zend/tests/bug75474.phpt` without losing a previous pass. Reflection of a
named user or internal function now produces an invokable Closure while
retaining the registered function identity, so independent reflected closures
and direct calls share function-static state. Reflection constructed from an
existing Closure returns that same Closure object and therefore preserves its
receiver, lexical scope and captures. Original E2E coverage checks shared
static state, internal invocation, captured values and strict identity. Five
Cargo test configurations, all-target and unsafe checks, Composer S0, Symfony
S1 and warmed-kernel S2 pass on AMD64; the production unsafe inventory remains
1,621 blocks, below the 1,623 ceiling. The S3 cold-kernel gate was not rerun
because this machine has no exact PHP 8.2 oracle. A 41-pair alternating release
confirmation measured `bench_calls.php` at baseline p10/median/p90 of
0.330568/0.335190/0.340155 seconds and candidate
0.332254/0.336496/0.343031 seconds (median +0.39 percent), with identical
computed results. The changed cold Reflection path is inactive in that
workload; the movement is retained as explicit temporary binary-layout debt.
Missing reflected internal functions, complete Closure metadata, SPL classes,
attributes and remaining Closure binding behavior are separate compatibility
boundaries.

The current AMD64 `Closure::fromCallable()` checkpoint, based on `4492764`,
runs the default-feature 4,345-case PHP 8.2.33 corpus and records 1,672 passes,
2,358 failures, 77 skips, one upstream XFAIL, 237 unsupported cases, zero
timeouts and zero crashes. It adds ten exact passes without losing a previous
pass: `Zend/tests/bug81626.phpt`, `Zend/tests/closure_063.phpt`,
`Zend/tests/closures/bug80929.phpt`,
`Zend/tests/closures/closure_from_callable_basic.phpt`,
`Zend/tests/closures/closure_from_callable_error.phpt`,
`Zend/tests/closures/closure_from_callable_gc.phpt`,
`Zend/tests/closures/closure_from_callable_lsb.phpt`,
`Zend/tests/closures/closure_from_callable_non_static_statically.phpt`,
`Zend/tests/closures/closure_from_callable_rebinding.phpt` and
`Zend/tests/closures/closure_from_callable_reflection.phpt`.
`Closure::fromCallable()` now preserves an existing Closure's identity and
creates closures for functions, static methods, bound instance methods and
magic static callbacks with PHP-compatible receiver, visibility and called
scope. Deprecated `self::` and `parent::` callback strings retain their PHP 8.2
lexical resolution and diagnostic, invalid callbacks raise the matching
`TypeError`, and method closures reject rebinding to an incompatible object.
First-class callables share the same closure-construction rule, including the
ability to bind an ordinary function closure to an object. Original E2E
coverage checks invocation, identity, binding, compatible rebinding, static
and magic methods and the invalid non-static diagnostic. Five Cargo test
configurations, all-target and unsafe checks, Composer S0, Symfony S1 and
warmed-kernel S2 pass on AMD64; the production unsafe inventory is 1,621
blocks, below the 1,623 ceiling. The S3 cold-kernel gate was not rerun because
this machine has no exact PHP 8.2 oracle. Twenty-one alternating release pairs
measured `bench_calls.php` at baseline p10/median/p90 of
0.333631/0.337829/0.349077 seconds and candidate
0.332684/0.336998/0.342547 seconds (median -0.25 percent), with identical
computed results. `ReflectionFunction::getClosure()`, dynamic invocation of
`[Closure, "__invoke"]`, dynamic-property-name parsing, `DateTime`, loose
closure equality and complete Closure reflection metadata remain separate
compatibility boundaries.

The current AMD64 `Closure::call()` checkpoint, based on `bdef267`, runs the
same default-feature 4,345-case PHP 8.2.33 corpus and records 1,662 passes,
2,368 failures, 77 skips, one upstream XFAIL, 237 unsupported cases, zero
timeouts and zero crashes. It adds four exact passes without losing a previous
pass: `Zend/tests/closure_060.phpt`, `Zend/tests/closure_call.phpt`,
`Zend/tests/return_types/025.phpt` and
`Zend/tests/type_declarations/typed_properties_048.phpt`. `Closure::call()` now
temporarily binds its object and visibility scope, forwards variadic arguments,
supports `self` return types and restores the closure's original binding after
the call. Static-property inline caches are detached only for the synchronous
rebound invocation and restored afterwards, so a private-property decision
cannot leak between scopes. Static-property silent probes now use a dedicated
instruction flag rather than aliasing the compact late-static-scope bit.
Original E2E coverage checks private instance access, arguments, binding
restoration, temporary static visibility, cache restoration and the non-object
TypeError. Five Cargo test configurations, all-target and unsafe checks,
Composer S0, Symfony S1 and warmed-kernel S2 pass on AMD64; the production
unsafe-block inventory decreases from 1,623 to 1,620 through consolidation of
an existing callback-cache proof. The S3 cold-kernel gate was not rerun because
this machine has no exact PHP 8.2 oracle. Forty-one alternating release pairs
measured `bench_static_self_property_write_typed.php` medians of 0.113724
seconds for the preceding binary and 0.114053 seconds for the candidate (+0.29
percent), with identical results. `Closure::fromCallable()` for internal
methods and the related `closure_call_internal.phpt` remain a separate
compatibility boundary.

The current AMD64 magic-call-trampoline checkpoint, based on `db61930`, runs
the same 4,345-case PHP 8.2.33 corpus with the default Cargo feature set and
records 1,658 passes, 2,372 failures,
77 skips, one upstream XFAIL, 237 unsupported cases, zero timeouts and zero
crashes. It adds 23 exact passes without losing a previous pass:
`Zend/tests/access_modifiers_012.phpt`, `Zend/tests/bug19859.phpt`,
`Zend/tests/bug31683.phpt`, `Zend/tests/bug34260.phpt`,
`Zend/tests/bug50383.phpt`, `Zend/tests/bug55247.phpt`,
`Zend/tests/bug69025.phpt`, `Zend/tests/bug71474.phpt`,
`Zend/tests/call_static.phpt`, `Zend/tests/call_static_002.phpt`,
`Zend/tests/call_static_004.phpt`, `Zend/tests/enum/__call.phpt`,
`Zend/tests/enum/__callStatic.phpt`,
`Zend/tests/first_class_callable_005.phpt`,
`Zend/tests/first_class_callable_016.phpt`, `Zend/tests/fr47160.phpt`,
`Zend/tests/generators/generator_trampoline.phpt`, `Zend/tests/gh16515.phpt`,
`Zend/tests/indirect_call_array_004.phpt`,
`Zend/tests/is_callable_trampoline_uaf.phpt`,
`Zend/tests/object_handlers.phpt`, `Zend/tests/objects_021.phpt` and
`Zend/tests/traits/static_004.phpt`. Missing or inaccessible instance and
static methods now dispatch through public `__call` and `__callStatic` for
direct, dynamic, first-class and `call_user_func` calls. The trampoline
preserves the originally requested method, packs positional and named
arguments into PHP's two magic parameters and retains inherited static scope.
Original E2E coverage exercises instance, static, inherited, first-class and
callback-array paths. The persistent `PhpClosure` layout remains 72 bytes in
fresh baseline and candidate DWARF. Five Cargo test configurations,
all-target and unsafe checks, Composer S0, Symfony S1 and warmed-kernel S2 pass
on AMD64. The S3 cold-kernel gate was not rerun because this machine has no
exact PHP 8.2 oracle. Twenty-one alternating release pairs measured
`bench_calls.php` medians of 0.345492 seconds for the preceding binary and
0.351058 seconds for the candidate (+1.61 percent), with identical computed
results. The latest static-call measurement was 1.588284 versus 1.580534
seconds (-0.49 percent). The ordinary-call movement is explicit temporary
performance debt for the later whole-runtime optimization phase. Recursive
overload suppression and remaining parser, signature and Reflection cases
remain separate compatibility boundaries.

The current AMD64 first-class-callable checkpoint, based on `ea71e47`, runs
the same 4,345-case PHP 8.2.33 corpus and records 1,635 passes, 2,395 failures,
77 skips, one upstream XFAIL, 237 unsupported cases, zero timeouts and zero
crashes. It adds three exact passes without losing a previous pass:
`Zend/tests/first_class_callable_006.phpt`,
`Zend/tests/first_class_callable_009.phpt` and
`Zend/tests/first_class_callable_errors.phpt`. Failed Closure creation now
retains PHP's concrete `Error` reason for invalid value types, missing
functions, missing classes or methods, inaccessible methods and non-static
class callbacks, including throwable source metadata. Creating a first-class
callable from an existing Closure or its `__invoke` method returns the same
Closure object, and strict identity compares Closure payload identity just as
it does ordinary object identity. Original E2E coverage checks the diagnostic
families, allowed private/protected access, invocation and all three identity
comparisons. Magic `__call` trampolines, parser restrictions, strict-call-site
typing and complete Reflection rendering remain separate boundaries. Five
Cargo test configurations, all-target and unsafe checks, Composer S0, Symfony
S1 and warmed-kernel S2 pass on AMD64. The S3 cold-kernel gate was not rerun
because this machine has no exact PHP 8.2 oracle. Twenty-one alternating
release pairs measured `bench_calls.php` medians of 0.347761 seconds for the
preceding binary and 0.346145 seconds for the candidate (-0.47 percent), while
`bench_closure_copy.php` moved from 0.000426 to 0.000425 seconds (-0.22
percent). A 41-pair confirmation measured the closure-storage microbenchmark
at 0.008256 versus 0.008487 seconds (+2.80 percent) despite the changed branch
being inactive in that workload; this remains explicit temporary binary-layout
debt for the later whole-runtime optimization phase. All workloads retained
identical computed results.

The current AMD64 non-static-class-callback checkpoint, based on `30bbd40`,
runs the same 4,345-case PHP 8.2.33 corpus and records 1,632 passes, 2,398
failures, 77 skips, one upstream XFAIL, 237 unsupported cases, zero timeouts
and zero crashes. It adds four exact passes without losing a previous pass:
`Zend/tests/dynamic_call_non_static.phpt`,
`Zend/tests/incompat_ctx_user.phpt`,
`Zend/tests/indirect_call_array_005.phpt` and
`Zend/tests/indirect_call_string_003.phpt`. Direct non-static `Class::method()`
calls now require a compatible current receiver and preserve PHP's permitted
`self` and `parent` instance forwarding. Dynamic class-array and class-string
callbacks never borrow the caller's receiver and throw a catchable PHP `Error`
with the exact non-static-method diagnostic; instance-only `__call` fallbacks
follow the same rule while `__callStatic` remains eligible. Original E2E
coverage checks compatible, inherited, incompatible, array, string and magic
boundaries. The absent `ArrayIterator::current` implementation, visibility and
first-class-callable behavior remain separate boundaries. Five Cargo test
configurations, all-target and unsafe checks, Composer S0, Symfony S1 and
warmed-kernel S2 pass on AMD64. The S3 cold-kernel gate was not rerun because
this machine has no exact PHP 8.2 oracle. Twenty-one alternating release pairs
measured `bench_calls.php` medians of 0.344594 seconds for the preceding binary
and 0.344486 seconds for the candidate (-0.03 percent). The late-static and
static-`self` microbenchmarks moved from 0.153653 to 0.155800 seconds (+1.40
percent) and from 0.455137 to 0.501345 seconds (+10.15 percent), respectively,
with identical computed results. This is explicit temporary performance debt
for the later whole-runtime optimization phase, not an unmeasured claim of
neutral cost.

The current AMD64 dynamic-call-source checkpoint, based on `cb0211d`, runs the
same 4,345-case PHP 8.2.33 corpus and records 1,628 passes, 2,402 failures, 77
skips, one upstream XFAIL, 237 unsupported cases, zero timeouts and zero
crashes. It adds seven exact passes without losing a previous pass:
`Zend/tests/028.phpt`, `Zend/tests/dynamic_call_002.phpt`,
`Zend/tests/dynamic_call_003.phpt`, `Zend/tests/dynamic_call_004.phpt`,
`Zend/tests/indirect_call_array_001.phpt`, `tests/lang/043.phpt` and
`tests/lang/044.phpt`. `InitDynamicCall` now retains its source line, so
uncaught invalid-call errors expose the physical file, line and main-frame
trace. Dynamic static calls validate their already-evaluated class operand
before evaluating the method expression; an invalid class therefore neither
reads nor warns for the method expression. A rare `InitArray` flag carries this
ordering contract without adding work to ordinary array elements. Original E2E
coverage checks the bytecode source line, exact error and absence of the later
undefined-method-variable warning. Non-static callbacks and complete
reference-returning `Closure::call` behavior remain separate boundaries. Five
Cargo test configurations, all-target and unsafe checks, Composer S0, Symfony
S1 and warmed-kernel S2 pass on AMD64. The S3 cold-kernel gate was not rerun
because this machine has no exact PHP 8.2 oracle. Twenty-one alternating
release pairs measured `bench_calls.php` medians of 0.347410 seconds for the
preceding binary and 0.349505 seconds for the candidate. The directly affected
`bench_hash_dynamic_string_cv_array_loop.php` medians were 0.106917 and
0.107093 seconds. Both retained identical computed results.

The preceding AMD64 dynamic-call-error checkpoint, based on `76bb019`, runs the
same 4,345-case PHP 8.2.33 corpus and records 1,621 passes, 2,409 failures, 77
skips, one upstream XFAIL, 237 unsupported cases, zero timeouts and zero
crashes. It adds the exact passes `Zend/tests/bug64966.phpt` and
`Zend/tests/dynamic_call_freeing.phpt` without losing a previous pass. Invalid
dynamic calls through null and other scalar values, undefined string function
names, malformed callback arrays and objects without `__invoke` now throw a
catchable PHP `Error` with PHP 8.2 type names and messages. Temporary callable
expressions are still evaluated and released before control reaches the
handler. Original E2E coverage checks all four invalid value families and
continued execution after every catch. The missing uncaught source trace in
`Zend/tests/028.phpt`, non-static callback validation and several method/class
callback diagnostics remain separate boundaries. Five Cargo test
configurations, all-target and unsafe checks, Composer S0, Symfony S1 and
warmed-kernel S2 pass on AMD64. The S3 cold-kernel gate was not rerun because
this machine has no exact PHP 8.2 oracle. Nine alternating release runs of
`bench_calls.php` measured medians of 0.352278 seconds for the preceding binary
and 0.344349 seconds for the candidate, with the same computed result.

The preceding AMD64 illegal-array-offset checkpoint, based on `eed927c`, runs the
same 4,345-case PHP 8.2.33 corpus and records 1,619 passes, 2,411 failures, 77
skips, one upstream XFAIL, 237 unsupported cases, zero timeouts and zero
crashes. It adds six exact passes without losing a previous pass or moving
another failure category: `Zend/tests/036.phpt`, `Zend/tests/038.phpt`,
`Zend/tests/bug79790.phpt`, `Zend/tests/bug79947.phpt`,
`Zend/tests/illegal_offset_unset_isset_empty.phpt` and
`Zend/tests/init_array_illegal_offset_type.phpt`. Invalid array-key types now
throw a catchable PHP `TypeError` instead of escaping as a VM fatal error.
Literal construction, reads, writes, compound writes, references,
`isset`/`empty`, `unset` and property-dimension writes use PHP's exact general
or contextual message, attach the throwable origin and do not publish a partial
mutation. A dedicated `empty()` bytecode flag distinguishes its diagnostic
context from value-preserving silent probes such as null coalescing. Original
E2E coverage checks every operation family and catches the exact messages.
`$GLOBALS` symbol-table offsets and the missing uncaught source trace for a
standalone invalid constant-expression key remain separate boundaries. Five
Cargo feature configurations, all-target and unsafe checks, Composer S0,
Symfony S1 and warmed-kernel S2 pass on AMD64. The S3 cold-kernel gate was not
rerun because this machine has no exact PHP 8.2 oracle. Nine alternating release
runs of `bench_calls.php` measured medians of 0.342195 seconds for the preceding
binary and 0.344369 seconds for the candidate. The directly affected
`bench_hash_dynamic_string_array_loop.php` medians were 0.101398 and 0.101335
seconds. Both retained identical computed results.

The preceding AMD64 core-diagnostics checkpoint, based on `fadebc3`, runs the
same 4,345-case PHP 8.2.33 corpus and records 1,613 passes, 2,417 failures, 77
skips, one upstream XFAIL, 237 unsupported cases, zero timeouts and zero
crashes. It adds five exact passes without losing a previous pass or moving
another failure category: `Zend/tests/008.phpt`, `Zend/tests/015.phpt`,
`Zend/tests/018.phpt`, `Zend/tests/nowdoc_015.phpt` and
`tests/lang/bug21094.phpt`. Runtime constant functions now reject invalid name
types, report built-in and user-constant collisions through the ordinary PHP
warning path, and throw a catchable `Error` for an undefined `constant()` name.
Unhandled `trigger_error()` notice, warning and deprecation levels now emit the
PHP 8.2 diagnostic with the physical caller file and line; eligible handlers,
including object-method handlers and `E_USER_ERROR`, receive the same source
metadata exactly once. Original E2E coverage checks exception types and
messages, handler masks, collision returns, unhandled formatting and source
lines. Five Cargo feature configurations, all-target and unsafe checks,
Composer S0, Symfony S1 and warmed-kernel S2 pass on AMD64. The S3 cold-kernel
gate was not rerun because this machine has no exact PHP 8.2 oracle. Nine
alternating release runs of `bench_calls.php` measured medians of 0.342488
seconds for the preceding binary and 0.347672 seconds for the candidate, with
the same computed result.

The preceding AMD64 PHP-8.2-get-class checkpoint, based on `413bd3c`, runs the
same 4,345-case PHP 8.2.33 corpus and records 1,608 passes, 2,422 failures, 77
skips, one upstream XFAIL, 237 unsupported cases, zero timeouts and zero
crashes. It adds the exact passes `Zend/tests/009.phpt` and
`Zend/tests/generators/generator_static_method.phpt` without losing a previous
pass or moving another failure category. In PHP 8.2, calling `get_class()`
without an argument returns the lexical class name without a diagnostic when
executed in class scope; the omission deprecation begins in PHP 8.3 and must not
leak into RPHP's PHP 8.2 public contract. Global calls retain their catchable
`Error`, explicit non-object arguments retain their `TypeError`, and inherited
static, instance and generator frames preserve their existing lexical scope.
Original E2E coverage checks these boundaries. Five Cargo feature
configurations, all-target and unsafe checks, Composer S0, Symfony S1 and
warmed-kernel S2 pass on AMD64. The S3 cold-kernel gate was not rerun because
this machine has no exact PHP 8.2 oracle. Nine alternating release runs of
`bench_calls.php` measured medians of 0.346073 seconds for the preceding binary
and 0.345184 seconds for the candidate, with the same computed result.

The preceding AMD64 internal-null-contract checkpoint, based on `fb8b465`, runs
the same 4,345-case PHP 8.2.33 corpus and records 1,606 passes, 2,424 failures,
77 skips, one upstream XFAIL, 237 unsupported cases, zero timeouts and zero
crashes. It adds three exact passes without losing a previous pass or moving
another failure category:
`Zend/tests/call_user_func_array_invalid_type.phpt`,
`Zend/tests/null_to_non_nullable_special_func.phpt` and
`Zend/tests/nullsafe_operator/013.phpt`. Excess arguments to non-variadic
internal functions now throw catchable `ArgumentCountError` instances with
PHP-compatible exact or maximum arity messages, including detached callback
calls. Compiler-lowered calls retain their source line, and packed literal
`call_user_func_array()` calls retain their public API identity for callback
diagnostics. Null passed to scalar parameters of `strlen`, `ord`, `chr`,
`defined` and the `array_slice` offset now follows PHP 8.2 deprecation handling;
a null `array_slice` length remains an omitted length. Relevant invalid argument
types for `array_key_exists`, `array_slice`, `get_class` and
`call_user_func_array` now produce PHP-compatible `TypeError` messages. Original
E2E coverage checks catchability, exact messages, handler delivery and physical
source lines. Five Cargo feature configurations, all-target and unsafe checks,
Composer S0, Symfony S1 and warmed-kernel S2 pass on AMD64. The S3 cold-kernel
gate was not rerun because this machine has no exact PHP 8.2 oracle. Nine
alternating release runs of `bench_calls.php` measured medians of 0.347632
seconds for the preceding binary and 0.346857 seconds for the candidate. The
directly affected `bench_stdclass_string_property_strlen.php` medians were
0.619518 and 0.637207 seconds. Both retained identical computed results.

The preceding AMD64 nullsafe-string-interpolation checkpoint, based on
`44861ba`, runs the same
4,345-case PHP 8.2.33 corpus and records 1,603 passes, 2,427 failures, 77 skips,
one upstream XFAIL, 237 unsupported cases, zero timeouts and zero crashes. It
adds the exact pass `Zend/tests/nullsafe_operator/033.phpt` without losing a
previous pass or moving another failure category. Simple double-quoted and
heredoc interpolation now recognizes `?->` as the same property-only boundary
as `->`: the selected property is converted to text, while following call
parentheses remain literal text. Braced interpolation continues through the
ordinary expression parser and now retains its physical source line when the
embedded expression is re-lexed, so property and method diagnostics identify
the original string line. Original lexer and E2E coverage checks null and
object receivers, simple and braced forms, property/method boundaries, warning
order and exact source location. Five Cargo feature configurations, all-target
and unsafe checks, Composer S0, Symfony S1 and warmed-kernel S2 pass on AMD64.
The S3 cold-kernel gate was not rerun because this machine has no exact PHP 8.2
oracle. Nine release runs of `bench_calls.php` measured medians of 0.341958
seconds for the preceding binary and 0.344068 seconds for the candidate, with
the same computed result.

The preceding AMD64 referenced-nullsafe-receiver checkpoint, based on
`65ba50a`, runs the same
4,345-case PHP 8.2.33 corpus and records 1,602 passes, 2,428 failures, 77 skips,
one upstream XFAIL, 237 unsupported cases, zero timeouts and zero crashes. It
adds the exact pass `Zend/tests/nullsafe_operator/040.phpt` without losing a
previous pass or moving another failure category. Nullsafe receiver checks now
classify the value stored inside a PHP reference instead of the reference
wrapper. A referenced null therefore short-circuits silently, while referenced
objects use ordinary property and method access and referenced scalars retain
their normal warning or catchable method error. The receiver cell remains live
for the following opcode, preserving alias identity and by-reference return
writeback. Original E2E coverage checks null, object and scalar references,
both property and method paths, exact handled diagnostics and alias-return
behavior. Five Cargo feature configurations, all-target and unsafe checks,
Composer S0, Symfony S1 and warmed-kernel S2 pass on AMD64. The S3 cold-kernel
gate was not rerun because this machine has no exact PHP 8.2 oracle. Nine
release runs of `bench_calls.php` measured medians of 0.345714 seconds for the
preceding binary and 0.350360 seconds for the candidate, with the same computed
result.

The preceding AMD64 magic-constant-postfix checkpoint, based on `6d9d0c6`,
runs the same 4,345-case PHP 8.2.33 corpus and records 1,601 passes, 2,429
failures, 77 skips,
one upstream XFAIL, 237 unsupported cases, zero timeouts and zero crashes. It
adds four exact passes without losing a previous pass or moving another failure
category: `Zend/tests/gh9136_2.phpt`,
`Zend/tests/nullsafe_operator/029.phpt`,
`Zend/tests/prop_const_expr/lhs_non_object.phpt` and
`Zend/tests/varSyntax/magic_const_deref.phpt`. Magic constants at statement
level now enter the ordinary expression and postfix parser, including
nullsafe property access. A nullsafe check short-circuits only a null receiver;
non-null scalar property reads continue through the normal object-read opcode,
which evaluates a dynamic property name once and emits PHP's exact property,
receiver-type and source-location diagnostic. Scalar nullsafe method calls
retain their fatal error. Original E2E coverage checks bare magic constants,
integer and string receivers, exact handled warnings and one-time dynamic-name
evaluation. Five Cargo feature configurations, all-target and unsafe checks,
Composer S0, Symfony S1 and warmed-kernel S2 pass on AMD64. The S3 cold-kernel
gate was not rerun because this machine has no exact PHP 8.2 oracle. Nine
release runs of `bench_calls.php` measured medians of 0.361834 seconds for the
preceding binary and 0.347734 seconds for the candidate, with the same computed
result.

The preceding AMD64 nullsafe foreach-reference checkpoint, based on `223bbc5`,
runs the same 4,345-case PHP 8.2.33 corpus and records 1,597 passes, 2,433
failures, 77 skips,
one upstream XFAIL, 237 unsupported cases, zero timeouts and zero crashes. It
adds six exact passes without losing a previous pass or moving another failure
category: `Zend/tests/dead_array_type_inference.phpt`,
`Zend/tests/foreach_over_null.phpt`, `Zend/tests/foreach_undefined.phpt`,
`Zend/tests/nullsafe_operator/023.phpt`, `tests/lang/bug27439.phpt` and
`tests/lang/foreachLoop.003.phpt`. A by-reference `foreach` over a nullsafe
receiver now iterates a detached outer value and deliberately discards its
writeback. Ordinary element mutations therefore do not convert or modify the
source property, while interior reference cells retain their shared effects.
The receiver is evaluated once and a null result follows the normal foreach
warning path. That path now uses the common PHP diagnostic dispatcher, adding
source location, error-handler invocation and exception propagation instead
of writing an unlocated warning directly. Original E2E coverage checks null
and object receivers, detached writes, interior references, one-time receiver
evaluation and handled warnings; existing scalar foreach expectations now
assert the PHP-compatible location. Five Cargo feature configurations,
all-target and unsafe checks, Composer S0, Symfony S1 and warmed-kernel S2 pass
on AMD64. The S3 cold-kernel gate was not rerun because this machine has no
exact PHP 8.2 oracle. Nine release runs measured `bench_calls.php` medians of
0.344343 seconds for the preceding binary and 0.346679 seconds for the
candidate; `bench_foreach.php` medians were 0.017796 and 0.018245 seconds.
Both retained the same computed results.

The preceding AMD64 nullsafe-destructuring checkpoint, based on `f5a697f`,
runs the same 4,345-case PHP 8.2.33 corpus and records 1,591 passes, 2,439
failures, 77 skips, one upstream XFAIL, 237 unsupported cases, zero timeouts
and zero crashes. It
adds the exact pass `Zend/tests/nullsafe_operator/021.phpt` without losing a
previous pass or moving another failure category. Short and `list()`
destructuring assignments now reject a target whose writable receiver spine
contains a nullsafe operator, including nested and append targets. Ordinary
targets use PHP 8.2's `Assignments can only happen to writable values` fatal;
reference targets retain the distinct non-referenceable-value diagnostic. The
parser consumes the complete l-value and records a deferred compile error, so
neither top-level code nor the assignment RHS executes. Original E2E coverage
checks both syntaxes, nested and append forms, reference targets, source
location and pre-execution failure. Five Cargo feature configurations,
all-target and unsafe checks, Composer S0, Symfony S1 and warmed-kernel S2 pass
on AMD64. The S3 cold-kernel gate was not rerun because this machine has no
exact PHP 8.2 oracle. Nine release runs of `bench_calls.php` measured medians
of 0.345851 seconds for the preceding binary and 0.338875 seconds for the
candidate, with the same computed result.

The preceding AMD64 nullsafe reference-return checkpoint, based on `483575d`,
runs the same 4,345-case PHP 8.2.33 corpus and records 1,590 passes, 2,440
failures, 77 skips, one upstream XFAIL, 237 unsupported cases, zero timeouts
and zero crashes. It
adds the exact pass `Zend/tests/nullsafe_operator/017.phpt` without losing a
previous pass or moving another failure category. Functions, methods and
closures declared to return by reference now reject any nullsafe receiver
chain during compilation with PHP 8.2's located fatal diagnostic. The shared
compiler walk follows ordinary property, array and dynamic-static postfixes
back to the originating nullsafe operator; compilation fails before top-level
side effects execute. Ordinary returns and non-nullsafe reference-return
aliasing remain unchanged. Original E2E coverage checks all three declaration
forms, a nested receiver spine, source location and pre-execution failure.
Five Cargo feature configurations, all-target and unsafe checks, Composer S0,
Symfony S1 and warmed-kernel S2 pass on AMD64. The S3 cold-kernel gate was not
rerun because this machine has no exact PHP 8.2 oracle. Nine release runs of
`bench_calls.php` measured medians of 0.337833 seconds for the preceding binary
and 0.342665 seconds for the candidate, with the same computed result.

The preceding AMD64 nullsafe-argument checkpoint, based on `465e80b`, runs the
same 4,345-case PHP 8.2.33 corpus and records 1,589 passes, 2,441 failures, 77 skips,
one upstream XFAIL, 237 unsupported cases, zero timeouts and zero crashes. It
adds the exact pass `Zend/tests/nullsafe_operator/016.phpt` without losing a
previous pass or moving another failure category. A nullsafe chain used as a
call argument is evaluated once as an ordinary value but is marked
non-referenceable across known and runtime-resolved signatures. Selecting a
by-reference positional, named or variadic parameter therefore raises PHP
8.2's catchable argument `Error`, whether the receiver is null or an object;
by-value parameters retain the evaluated result. Original E2E coverage checks
known and dynamic callables, positional and named arguments, both receiver
branches, evaluation order and the by-value control. Five Cargo feature
configurations, all-target and unsafe checks, Composer S0, Symfony S1 and
warmed-kernel S2 pass on AMD64. The S3 cold-kernel gate was not rerun because
this machine has no exact PHP 8.2 oracle. Nine release runs of
`bench_calls.php` measured medians of 0.341740 seconds for the preceding binary
and 0.341299 seconds for the candidate, with the same computed result.

The preceding AMD64 dynamic-variable re-entry checkpoint, based on `91f599c`,
runs the same 4,345-case PHP 8.2.33 corpus and records 1,588 passes, 2,442
failures, 77 skips, one upstream XFAIL, 237 unsupported cases, zero timeouts
and zero crashes. It adds three exact passes without losing a previous pass:
`Zend/tests/bug68118.phpt`, `Zend/tests/oss_fuzz_54325.phpt` and
`Zend/tests/type_declarations/closure_with_variadic.phpt`. Read-modify-write
operations on a variable-variable now evaluate and convert the dynamic name
once, retain that selected symbol before an undefined-variable handler can
re-enter the caller, and write back to the original target even if the handler
mutates the source CV. Object names invoke `__toString()` once. Closures now
consume and release request-local Zend object-store handles alongside ordinary
objects, so later object diagnostics observe PHP-compatible numbering.
Original E2E coverage exercises post/pre increment, compound
assignment, handler re-entry and one-time object-name conversion. Five Cargo
feature configurations, all-target and unsafe checks, Composer S0, Symfony S1
and warmed-kernel S2 pass on AMD64. The S3 cold-kernel gate was not rerun
because this machine has no exact PHP 8.2 oracle. Nine release runs measured
`bench_calls.php` medians of 0.3460 seconds for the preceding binary and 0.3432
seconds for the candidate. The directly affected closure-copy medians were
0.0004175 and 0.0004218 seconds; closure-storage medians were 0.007821 and
0.008496 seconds. Every benchmark retained the same computed result.

The preceding AMD64 string-reversal checkpoint, based on `b692f58`, runs the
same 4,345-case PHP 8.2.33 corpus and records 1,585 passes, 2,445 failures, 77
skips, one upstream XFAIL, 237 unsupported cases, zero timeouts and zero
crashes. It adds the exact pass
`Zend/tests/generators/send_returns_current.phpt` without losing a previous
pass or moving another failure category. The existing string reversal
implementation is now exposed under PHP's `strrev()` name instead of the
non-PHP `str_rev` spelling; its signature supports the ordinary positional,
named and weak scalar-string paths. Two other tests now execute past the
missing-symbol boundary and expose separate pre-existing nullsafe by-reference
argument and re-entrant variable-variable writeback failures; they remain
ordinary failures rather than being hidden as gains. Arbitrary binary strings
whose byte reversal is not valid UTF-8 remain outside this checkpoint until
the UTF-8-backed `Value` string representation is replaced or extended with a
byte-string form. Five Cargo feature configurations, all-target and unsafe
checks, Composer S0, Symfony S1 and warmed-kernel S2 pass on AMD64. The S3
cold-kernel gate was not rerun because this machine has no exact PHP 8.2 oracle.
Nine release runs of `bench_calls.php` measured medians of 0.3419 seconds for
the preceding binary and 0.3424 seconds for the candidate, with the same
computed result.

The preceding AMD64 dynamic-nullsafe-property checkpoint, based on `555e262`,
runs the same 4,345-case PHP 8.2.33 corpus and records 1,584 passes, 2,446
failures, 77 skips,
one upstream XFAIL, 237 unsupported cases, zero timeouts and zero crashes. It
adds six exact passes without losing a previous pass or moving another failure
category: `Zend/tests/bug81216.phpt`, `gh10570.phpt`,
`prop_const_expr/basic_nullsafe.phpt`, `prop_const_expr/rhs_object.phpt`,
`prop_const_expr/rhs_object_nullsafe.phpt` and
`type_declarations/typed_properties_086.phpt`. Braced dynamic nullsafe property
reads now retain the nullsafe AST flag and skip property-name evaluation when
the receiver is null. The baseline object-read path converts scalar, array and
object property names according to the ordinary PHP path, including one
`__toString()` call or a catchable `Error` for an unconvertible object. It owns
both operands across that re-entrant conversion, so rebinding the receiver
cannot change the object already selected for the read. Original parser and
E2E regressions cover short-circuiting, one-time evaluation, integer names,
successful and failed object conversion, and receiver rebinding. Five Cargo
feature configurations, all-target and unsafe checks, Composer S0, Symfony S1
and warmed-kernel S2 pass on AMD64. The S3 cold-kernel gate was not rerun
because this machine has no exact PHP 8.2 oracle. Nine release runs measured
`bench_calls.php` medians of 0.3488 seconds for the preceding binary and 0.3499
seconds for the candidate; the directly relevant `bench_declared_property_reads.php`
medians were 0.3893 and 0.3905 seconds. Both retained identical results.

The preceding AMD64 nullsafe-context checkpoint, based on `7ed185b`, runs the same
4,345-case PHP 8.2.33 corpus and records 1,578 passes, 2,452 failures, 77 skips,
one upstream XFAIL, 237 unsupported cases, zero timeouts and zero crashes. It
adds four exact passes without losing a previous pass or moving another
failure category: `Zend/tests/nullsafe_operator/009.phpt`, `010.phpt`,
`022.phpt` and `024.phpt`. Taking a reference to a nullsafe property or method
chain now fails during compilation with PHP 8.2's dedicated diagnostic;
`unset()` and nullsafe `foreach` write targets use the write-context fatal.
One shared receiver-spine check follows nested properties, methods, arrays and
dynamic static postfixes, including the statement parser's direct reference
assignment path. Parsing completes, but no target, loop body or referenced
expression is evaluated. Original parser and E2E regressions cover direct,
nested and method/static-call boundaries. Five Cargo feature configurations,
all-target and unsafe checks, Composer S0, Symfony S1 and warmed-kernel S2 pass
on AMD64. The S3 cold-kernel gate was not rerun because this machine has no
exact PHP 8.2 oracle. Nine release runs of `bench_calls.php` measured medians
of 0.3449 seconds for the preceding binary and 0.3385 seconds for the candidate,
with the same computed result.

The preceding AMD64 nullsafe-write checkpoint, based on `1755721`, runs the
same 4,345-case PHP 8.2.33 corpus and records 1,574 passes, 2,456 failures, 77
skips, one upstream XFAIL, 237 unsupported cases, zero timeouts and zero
crashes. It adds six exact passes without losing a previous pass or moving
another failure category: `Zend/tests/nullsafe_operator/004.phpt` through
`008.phpt` and `nullsafe_operator/020.phpt`. Assignment, compound assignment,
pre/post increment and null-coalescing assignment now defer nullsafe
write-context rejection to compilation and report PHP 8.2's fatal message with
the source filename and line. The check walks ordinary property and array
suffixes, so a nullsafe operator hidden earlier in the mutable receiver chain
cannot escape validation; the right-hand expression is parsed but never
executed. Original parser and E2E regressions cover direct and nested targets.
Five Cargo feature configurations, all-target and unsafe checks, Composer S0,
Symfony S1 and warmed-kernel S2 pass on AMD64. Nine release runs of
`bench_calls.php` measured medians of 0.3415 seconds for the preceding binary
and 0.3363 seconds for the candidate, with the same computed result.

The preceding AMD64 nullsafe-static checkpoint, based on `6bf807c`, runs the same
4,345-case PHP 8.2.33 corpus and records 1,568 passes, 2,462 failures, 77 skips,
one upstream XFAIL, 237 unsupported cases, zero timeouts and zero crashes. It
adds the exact pass `Zend/tests/nullsafe_operator/001.phpt` without losing a
previous pass or moving another failure category. Expression-based static
method calls now retain distinct class and method operands in the AST and
compiler instead of masquerading as user callable-array literals. A nullsafe
receiver can therefore short-circuit named, braced-dynamic and variable
`::method()` forms before evaluating the method name or arguments, while the
continuing path preserves class-string dispatch and evaluation order. Original
parser and E2E regressions cover both paths. Five Cargo feature configurations,
all-target and unsafe checks, Composer S0, Symfony S1 and warmed-kernel S2 pass
on AMD64. Nine release runs of `bench_calls.php` measured medians of 0.3367
seconds for the preceding binary and 0.3353 seconds for the candidate, with
the same computed result.

The preceding AMD64 nullsafe-method checkpoint, based on `d017413`, runs the same
4,345-case PHP 8.2.33 corpus and records 1,567 passes, 2,463 failures, 77 skips,
one upstream XFAIL, 237 unsupported cases, zero timeouts and zero crashes. It
adds eleven exact passes to the retained 1,556-pass checkpoint without losing a
previous pass. Nullsafe method calls on scalar receivers now throw a located,
catchable `Error` with the PHP 8.2 method and type names. Ordinary method calls
on null and scalar receivers share that diagnostic contract. Compiler-owned
nullsafe jumps follow the receiver spine across subsequent regular method,
property and array postfixes, while nullsafe expressions in arguments retain
independent short-circuit boundaries. Three still-failing type-declaration
cases move from the runner's runtime category to its compile category only
because the newly present runtime source path contains `type_declarations` and
matches the runner's existing broad stage heuristic; their failure message and
underlying missing Reflection/Closure methods are unchanged. Dynamic static
`::` calls after a short-circuited receiver remain a separate boundary. Five
Cargo feature configurations, all-target and unsafe checks, Composer S0,
Symfony S1 and warmed-kernel S2 pass on AMD64. Nine release runs of
`bench_calls.php` measured medians of 0.3387 seconds for the preceding binary
and 0.3346 seconds for the candidate, with the same computed result.
The exact additions are `Zend/tests/dereference_002.phpt`,
`indirect_method_call_002.phpt`, `methods-on-non-objects-catch.phpt`,
`methods-on-non-objects-usort.phpt`, `methods-on-non-objects.phpt`,
`nullsafe_operator/002.phpt`, `nullsafe_operator/014.phpt`,
`nullsafe_operator/constant_propagation.phpt`, `traits/bugs/alias01.phpt`,
`varSyntax/constant_object_deref.phpt` and
`varSyntax/method_call_on_string_literal.phpt`.

The preceding AMD64 object-array checkpoint, based on `523b670`, runs the same
4,345-case PHP 8.2.33 corpus and records 1,556 passes, 2,474 failures, 77 skips,
one upstream XFAIL, 237 unsupported cases, zero timeouts and zero crashes. It
adds three exact passes to the retained 1,553-pass checkpoint without losing a
previous pass. Plain objects and closures used as array dimensions now raise a
located, catchable `Error` consistently across read, write, append, reference,
property and unset paths. Flat and nested `unset()` retain PHP's distinct
undefined-variable warning, false-to-array deprecation and scalar/string/object
diagnostics without detaching a live CV during eval or foreach mutation.
Destructuring bytecode carries the assignment source line, including nested and
foreach forms. Original regressions cover the catchable mutation surface,
closure destructuring and undefined/null/scalar unset behavior. Five Cargo
feature configurations, all-target and unsafe checks, Composer S0, Symfony S1
and warmed-kernel S2 pass on AMD64. A five-million array-write release control
measured 0.04 seconds for both the preceding and candidate binaries.

The preceding AMD64 clone checkpoint, based on `b571b61`, runs the same
4,345-case PHP 8.2.33 corpus and records 1,553 passes, 2,477 failures, 77 skips,
one upstream XFAIL, 237 unsupported cases, zero timeouts and zero crashes. It
adds 11 exact passes to the retained 1,542-pass checkpoint without losing a
previous pass. Nested `clone` expressions now apply assignment at PHP's operand
precedence, and `CloneObj` retains source provenance so cloning a non-object
raises a catchable `Error` with the correct file, line and trace. Dynamic
construction accepts an object as its class prototype while rejecting other
non-string operands with PHP's catchable diagnostic. Reading a dimension from
a non-`ArrayAccess` object likewise raises a located `Error` rather than an
uncatchable VM fatal. Original regressions cover all three contracts; all four
base clone PHPTs, the five Cargo feature configurations, all-target check,
unsafe policy, Composer S0, Symfony S1 and warmed-kernel S2 gates pass on AMD64.

The preceding AMD64 object-lifecycle checkpoint, based on `7300c28`, runs the
same 4,345-case PHP 8.2.33 corpus and records 1,542 passes, 2,488 failures, 77 skips,
one upstream XFAIL, 237 unsupported cases, zero timeouts and zero crashes. It
adds 20 exact passes to the retained 1,522-pass checkpoint without losing a
previous pass. Request-local object-store handles now retain alias identity and
Zend-compatible recycling in `var_dump()`. Compiler scratch values release
objects at their logical PHP lifetime boundary, and re-entrant magic property
access retains its receiver across user code that rebinds the originating
variable. Original tests cover shared and recycled handles; the focused PHPT
regression set, five Cargo feature configurations, all-target check, unsafe
policy, Composer S0, Symfony S1 and warmed-kernel S2 gates pass on AMD64.

This correctness checkpoint deliberately leaves object-allocation throughput
for the execution/performance workstream: a one-million-allocation release
control measured 0.15 seconds for the candidate versus 0.09 seconds for the
preceding binary on the same AMD64 host. The PHP-visible lifecycle contract is
the optimization boundary; a later fast path must preserve handle identity,
recycling, destructor order and request reset behavior.

## Official PHP 8.2 php-src PHPT contract baseline

The public contract baseline runs the unmodified `Zend/tests` and `tests/lang`
suites from PHP 8.2.33 commit
`651db3ebfa622cae0c4e6b39766812efbd274ced` against default-release RPHP commit
`85b6503538343f18b3a747c3d567a575e7c6823b`, using the same runner commit. The
recorded run used arm64 and a three-second per-process timeout; the complete
ordinary default (`quick-loops+jit-prototype`), explicit typed-only
(`--no-default-features --features quick-loops`), no-default-features and
all-features Cargo matrix passed separately. It discovered 4,345 PHPT cases.

| Suite | Pass | Fail | Skip | XFAIL | Unsupported | Timeout | Crash | Headline pass rate |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `Zend/tests` | 1,098 | 2,666 | 65 | 1 | 221 | 0 | 0 | 29.171% |
| `tests/lang` | 91 | 177 | 10 | 0 | 16 | 0 | 0 | 33.955% |
| **Combined** | **1,189** | **2,843** | **75** | **1** | **237** | **0** | **0** | **29.489%** |

The headline follows the published gate definition exactly:
`pass / (pass + fail)`. It does not count skips, the known upstream `XFAIL`,
unsupported cases, timeouts or crashes as passes. A stricter whole-corpus view
is 1,189 / 4,345, or **27.365%**; including crashes and timeouts in the attempted
denominator gives **29.489%**. These numbers are intentionally pre-alpha and do
not support a complete PHP 8.2 claim.

The schema-5 execution profile makes the strict score less easy to mistake for
language coverage. Of 4,032 attempted cases, six fail during `SKIPIF` before
the test body, 694 are rejected in the observed parse/compile stage, and 3,332
(**82.639%**) execute the test's `FILE` section past that stage. This is not a
second compatibility score: invalid-source PHPT cases are supposed to stop in
the front end, and reaching runtime says nothing about correct semantics or
diagnostic text.

The largest failure groups are 1,145 runtime failures, 1,034 output mismatches,
488 parse failures, 170 compile failures and six failed `SKIPIF` evaluations.
No case terminates by signal or times out. Of the 75 skips, 45 require
unavailable extensions and 30 are selected by `SKIPIF`. Unsupported cases
remain in the total: 234 require per-process `INI` behavior that the RPHP CLI
does not expose, while three require PHPDBG or CGI/header sections outside this
CLI gate.

The complete official PHP 8.2.33 CLI oracle run produced 4,255 passes, zero
ordinary failures, 86 skips, the one upstream `XFAIL`, three unsupported SAPI
sections, zero timeouts and zero crashes. Five representative cases also pass
through php-src's official `run-tests.php`. Two independent RPHP executions
with a matching native PHP 8.2.33 runner produced byte-identical manifests and
identical summary evidence for the preceding baseline after excluding the
expected commit identity fields from its pre-commit run. The current hotfix was
run against the same validated runner and exact oracle, and its pass set was
compared path-by-path with that retained baseline. Four already-failing raw
executions retain known non-semantic output variation from unordered object-
property state; run
durations also vary, but neither enters the compact published artifacts. The
manifest has SHA-256
`25664914da748d0360a2e8f3eeb7a69dd7c22cc0d0e6304904781b20f8d05bcd` and
its summary has SHA-256
`2b6edcb2dd7a728e3a18b3cb56ec50bb09763acba21342860a3e84fef2097abd`.

The complete machine-readable result is committed as
[`85b6503-arm64-manifest.jsonl`](../tests/php-src/results/php-8.2.33/85b6503-arm64-manifest.jsonl),
with aggregate metadata in
[`85b6503-arm64-summary.json`](../tests/php-src/results/php-8.2.33/85b6503-arm64-summary.json).

Relative to the retained `5196535` baseline, this hotfix adds the exact pass
`Zend/tests/assign_obj_ref_return.phpt` without losing a previous pass or adding
a crash or timeout. Object `var_dump()` now observes property reference cells
directly instead of dereferencing them through an ordinary value clone.
Compiler-only reference CVs are counted separately from PHP-visible storage, so
array, property, dynamic-variable, append and nested-append bindings no longer
retain a false `&` marker after the last visible alias is unset. Original
clean-room regressions cover live declared and dynamic object-property aliases,
last-visible array/property/dynamic aliases, and flat plus nested by-reference
append arguments against the exact PHP 8.2.33 output.

The preceding `5196535` checkpoint, relative to the retained `6c1d43f`
baseline, added nine exact passes without losing a previous pass or adding a
crash or timeout. An array-
element reference assignment now traverses the same general mutable-l-value
path as other nested writes, binds the destination CV to the canonical element
cell, and writes rebuilt containers back through property, global and nested
array roots. Rebinding or unsetting the last external alias makes the retained
array cell an ordinary value again for `var_dump()`, while a live nested alias
retains PHP's `&` marker. Recursive reference output remains one recursion
sentinel rather than acquiring a spurious marker.

Original clean-room regressions cover direct rebinding, mutation after rebind,
last-alias unset, nested object-property and `$GLOBALS` writeback, nested
reference display and top-level reference transparency. Their output is byte-
identical to the pinned PHP 8.2.33 CLI. The complete four-configuration Cargo
matrix, PHPT runner fixtures, formatting and unsafe-policy ratchet pass; the
unsafe inventory decreases to 1,622 production blocks while retaining 289
unsafe functions. The exact additions are `Zend/tests/bug33282.phpt`,
`bug35163.phpt`, `bug71539.phpt`, `bug71539_2.phpt`, `bug71539_3.phpt`,
`bug71539_4.phpt`, `bug71539_5.phpt`, `enum/var_dump-reference.phpt` and
`type_declarations/typed_properties_011.phpt`. This does not claim complete
reference, reference-return, copy-on-write, PHP 8.2 or framework compatibility;
the corpus-convergence goal remains active.

The preceding `6c1d43f` checkpoint, relative to the retained `7675f09`
baseline, adds five exact passes without losing a previous pass or adding a
crash or timeout. A static
local assignment such as `$alias =& $value` now promotes the source to one
stable PHP reference cell and binds both CV names to it. Ordinary writes
through either name update that cell, while a later `=&` rebinds only the
destination name; `unset()` detaches one name without destroying the value
seen by remaining aliases. Undefined and self sources, `$this`, assignment as
an l-value expression, array copy-on-write and destructuring from an aliased
RHS retain PHP 8.2 behavior.

Original clean-room regressions cover two-way writes, rebind, unset, missing
and self sources, by-reference expression use, `$this`, array copy-on-write and
retained destructuring input. The complete four-configuration Cargo matrix and
unsafe-policy ratchet pass; the latter remains at 1,623 production blocks and
289 unsafe functions. The exact additions are
`Zend/tests/assign_to_var_001.phpt`, `assign_to_var_002.phpt`,
`bug71030.phpt`, `dereference_003.phpt` and `try/bug72629.phpt`. This does not
claim complete reference, array-element, foreach copy-on-write, PHP 8.2 or
framework compatibility; the corpus-convergence goal remains active.

The preceding `7675f09` checkpoint, relative to the retained `f8fc462`
baseline, added 43 exact passes without losing a previous pass or adding a
crash or timeout. PHP's
`$$name` and `${expression}` forms now retain their right-associated syntax and
resolve against the active frame symbol table. Reads, writes, `isset`, `unset`,
`??=`, compound mutation, `global`, destructuring, callable postfixes and
reference binding share one runtime-name rule, including scalar coercion,
`__toString()` and exception propagation. Runtime-only locals remain
frame-scoped, indirect `$this` is readable but cannot be rebound, and dynamic
object/static property selection preserves PHP's indirection depth and error
staging.

Original clean-room regressions cover global and local symbol tables, runtime-
created names, references, self-referential array appends, dynamic globals,
coalesce/writeback, object/static members, destructuring, nested name
evaluation, object conversion and the `$this` boundary. Serialization and
`var_dump()` now terminate repeated array-reference identities consistently,
and string increment implements PHP's alphanumeric carry needed by nested
runtime names. The complete four-configuration Cargo matrix and unsafe-policy
ratchet pass; the latter remains at 1,623 production blocks and 289 unsafe
functions rather than raising either ceiling.

All 57 PHPT paths previously selected for the leading-dollar lexer rejection
now execute their `FILE` section past that front-end boundary. Thirty-one pass
exactly; the other 26 remain visible failures in separate callable binding,
copy-on-write, string interpolation/offset, Fiber/Closure, library or negative-
diagnostic clusters. Across the complete manifest, 39 still-failing paths move
from parse rejection to runtime/output comparison. Fifteen already-failing
diagnostic tests are classified as compile failures by the runner even though
their `FILE` sections execute and reach a runtime `TypeError`; this is a known
stage-classification artifact, not a newly rejected front end.

The exact additions are `Zend/tests/023.phpt`, `025.phpt`, `027.phpt`,
`anon/008.phpt`, `arrow_functions/003.phpt`, `bug26802.phpt`,
`bug27669.phpt`, `bug35163_2.phpt`, `bug35470.phpt`, `bug38211.phpt`,
`bug52001.phpt`, `bug53347.phpt`, `bug62653.phpt`, `bug68162.phpt`,
`bug69989_3.phpt`, `bug78151.phpt`, `exception_before_fatal.phpt`,
`generators/generator_symtable_leak.phpt`, `generators/gh11028_2.phpt`,
`global_to_string_exception.phpt`, `global_with_side_effect_name.phpt`,
`grammar/regression_013.phpt`, `indirect_reference_this.phpt`,
`int_static_prop_name.phpt`, `isset_001.phpt`, `isset_002.phpt`,
`nullsafe_operator/026.phpt`, `restrict_globals/key_canonicalization.phpt`,
`self_method_or_prop_outside_class.phpt`,
`static_method_non_existing_class.phpt`,
`symtable_cache_recursive_dtor.phpt`, `temporary_cleaning_012.phpt`,
`this_reassign.phpt`, `traits/bug75607a.phpt`,
`varSyntax/class_constant_static_deref.phpt`, `varSyntax/staticMember.phpt`,
`varSyntax/static_prop_on_expr_class.phpt`,
`varSyntax/static_prop_on_expr_class_with_backslash.phpt`,
`variable_with_boolean_name.phpt`, `variable_with_integer_name.phpt`,
`varvars_by_ref.phpt`, `tests/lang/bug24396.phpt` and
`tests/lang/engine_assignExecutionOrder_003.phpt`. This does not claim complete
variable syntax, references, symbol-table introspection, PHP 8.2 or framework
compatibility; the corpus-convergence goal remains active.

The preceding `f8fc462` checkpoint, relative to the retained `b12526d`
baseline, added 26 exact passes without losing a previous pass or adding a
crash or timeout. Ordinary
undefined-variable rvalues now snapshot null and report PHP 8.2's `E_WARNING`
with the original file and line, including runtime-resolved positional, named
and variadic sends. `@` applies its active reporting mask before a declined
built-in diagnostic is emitted, while user handlers still observe the PHP 8.2
suppressed mask and may explicitly re-enable reporting. Handler reentrancy
publishes global mutations without changing the already-snapshotted value.

Reference acquisition remains a distinct silent context: missing CVs become
null for direct and unpacked by-reference arguments, closure captures, array
elements, `$GLOBALS` aliases and by-reference returns. Global ordinary
assignment writes through an existing alias, `global` creates a missing null
binding without replacing an initialized top-level CV, and runtime argument
layout shifts preserve closure-capture reference identity. Increment/decrement
and compound assignment retain PHP evaluation order and result kinds when a
handler mutates or unsets the operand; conservative definite-initialization
joins keep proven function-local hot loops on compact CV operands.

The exact additions are `Zend/tests/array_unpack/undef_var.phpt`,
`arrow_functions/002.phpt`, `assign_coalesce_007.phpt`, `bug30162.phpt`,
`bug35017.phpt`, `bug63206_1.phpt`, `bug67314.phpt`, `bug70785.phpt`,
`bug79599.phpt`, `bug79791.phpt`, `closure_012.phpt`,
`code_before_loop_var_free.phpt`, `entry_block_with_predecessors.phpt`,
`error_reporting09.phpt`, `incdec_undef.phpt`,
`inference_infinite_loop.phpt`, `match/042.phpt`,
`named_params/undef_var.phpt`, `named_params/variadic.phpt`,
`nullsafe_operator/018.phpt`, `nullsafe_operator/039.phpt`,
`remove_predecessor_of_pi_node.phpt`,
`type_declarations/typed_properties_056.phpt`,
`type_declarations/typed_properties_057.phpt`,
`unreachable_phi_cycle.phpt` and
`tests/lang/operators/overloaded_property_ref.phpt`. This does not claim
complete undefined-variable, call/reference, PHP 8.2 or framework
compatibility; constructor/precompiled/generator argument edges and other
visible failure clusters remain separate work.

The preceding `b12526d` checkpoint, relative to the retained `e077e12`
baseline, added 13 exact passes without losing a previous pass or adding a
crash or timeout. A
`Throwable` now snapshots its complete live PHP call chain when it is created,
not when it is later thrown. Named functions, instance and static methods,
constructors, dynamic calls and closures retain PHP's call-site line rule;
method frames retain their `->` or `::` staticness and argument snapshot.
`getTrace()`, `getTraceAsString()` and the top-level uncaught diagnostic share
that stored trace, including PHP's public `{closure}` name, argument rendering,
anonymous-class normalization and final `{main}` sentinel. Throwing or
rethrowing the object later does not replace its creation origin or trace.

Original clean-room regressions cover nested functions, method arguments,
multiline named/static/constructor calls, closure naming, anonymous-class
arguments and preservation across a later throw. Differential probes match the
pinned PHP 8.2.33 CLI for the structured trace and rendered string. The exact
additions are `Zend/tests/bug29368_1.phpt`, `bug48228.phpt`, `bug48408.phpt`,
`exception_023.phpt`, `gh8810_1.phpt` through `gh8810_4.phpt`,
`gh8810_6.phpt`, `gh8810_7.phpt`, `return_types/020.phpt`,
`try/try_finally_001.phpt` and
`uncaught_exception_error_supression.phpt`.

An alternating release control of 50 million ordinary object creations measured
0.27 s for both the retained baseline and this candidate. A deliberately
trace-heavy loop constructing 100,000 nested exceptions measured 0.05 s on the
baseline, 0.11 s on this candidate and 0.02 s on the PHP 8.2.33 oracle. The
isolated creation cost is the required eager trace snapshot and remains a named
optimization boundary; the ordinary object path did not regress in this
control. Internal callback frames, suspended generator/coroutine histories,
exception-chain rendering and per-request INI controls such as
`zend.exception_ignore_args` remain separate visible boundaries. The pinned Symfony FrameworkBundle
7.4.16 S3 gate remains green against PHP 8.2.33 across cold and cached loads,
health and missing-route requests, deleted and malformed caches, and concurrent
atomic publication. This checkpoint does not claim complete Throwable,
diagnostic or PHP 8.2 compatibility, and the broader PHP 8.2 corpus-convergence
goal remains active.

The preceding `e077e12` checkpoint, relative to the retained `e0d94aa`
baseline, added eight exact passes without losing a previous pass or adding a
crash or timeout. `new` and
`throw` retain independent source lines through the lexer, AST and compiler;
the compiler publishes only the sparse locations needed by observable
opcodes, while `Instruction` remains 16 bytes. Every newly constructed
`Throwable` receives its file and line before a user constructor can run, and
later throws preserve that creation origin. A root-created uncaught Throwable
with a provably empty root trace now renders PHP's located message, the root
`#0 {main}` frame and final thrown line. Runtime constant-expression array-unpack
errors use the spread opcode's own location, and CLI runtime fatals begin on
PHP's fresh diagnostic line after any prior output.

Original clean-room regressions cover independent creation/throw locations,
constructor-time observation, later throws, empty-message rendering and the
runtime constant-unpack error. Source filenames share their compiled owner,
and Exception/Error file and line state uses declared slots rather than
allocating dynamic maps for every instance. An alternating release control of
100 million ordinary object creations measured 0.55 s for the preceding
candidate and 0.56 s for this candidate; a deliberately extreme 100,000-case
construct/throw/catch loop measured 0.03 s versus 0.05 s after removing
per-instance filename allocation. These local timings are a regression
control, not a general performance claim.

The exact additions are `Zend/tests/array_unpack/gh9769.phpt`,
`exception_001.phpt`, `exception_003.phpt`, `exception_handler_007.phpt`,
`flexible-heredoc-nowdoc-lineno.phpt`, `gh13097_b.phpt`,
`throw_reference.phpt` and `try/try_finally_recursive_previous.phpt`. The array
unpack directory now has eleven exact passes; only `undef_var.phpt` remains,
on the separate undefined-variable warning boundary. Complete nested call
traces, general call-site source locations and other warning/diagnostic
families remain explicit non-claims. The pinned Symfony FrameworkBundle 7.4.16
S3 gate remains green against PHP 8.2.33 across cold and cached loads, health
and missing-route requests, deleted and malformed caches, and concurrent
atomic publication. The broader PHP 8.2 corpus-convergence goal remains
active.

The preceding `e0d94aa` checkpoint, relative to the retained `c92a29e`
baseline, added one exact pass without losing a previous pass or adding a crash
or timeout. Array unpack
now preserves the source line of its `...` token and rejects a statically known
non-array operand during compilation with PHP's located fatal diagnostic.
Literals, constant scalar expressions, magic and built-in constants, and known
class constants participate in that compile-time decision. Ordinary user
constants, variables and constructed objects retain the distinct catchable
runtime `Error` contract. Original clean-room regressions cover token and AST
line retention, scalar expressions, magic and class constants, and both
runtime-only boundaries.

The exact addition is
`Zend/tests/array_unpack/unpack_invalid_type_compile_time.phpt`. The directory
now has ten exact passes; `gh9769.phpt` and `undef_var.phpt` remain failures for
general uncaught-Throwable formatting and undefined-variable warning behavior.
The pinned Symfony FrameworkBundle 7.4.16 S3 gate remains green against PHP
8.2.33 across cold and cached loads, health and missing-route requests, deleted
and malformed caches, and concurrent atomic publication. This checkpoint does
not claim complete array-unpack, diagnostic, warning or PHP 8.2 compatibility,
and the broader PHP 8.2 corpus-convergence goal remains active.

The preceding `c92a29e` checkpoint, relative to the retained `caf5590`
baseline, added two exact passes without losing a previous pass or adding a
crash or timeout. Unkeyed spread syntax in short and `list()` destructuring,
including nested and
`foreach` targets, is now preserved through parsing and rejected during
compilation with PHP's source line and `Spread operator is not supported in
assignments` diagnostic. The compiler error is staged before the right-hand
side can execute; keyed spread retains its distinct parse-error boundary.
Original clean-room regressions cover both destructuring spellings, nested and
`foreach` forms, the deferred compile stage, source location and absence of
right-hand-side execution.

The exact additions are `Zend/tests/array_unpack/in_destructuring.phpt` and
`Zend/tests/array_unpack/in_destructuring_2.phpt`. The remaining three exact
failures in that directory expose general diagnostic formatting rather than
the newly admitted destructuring-spread staging. The pinned Symfony
FrameworkBundle 7.4.16 S3 gate remains green against PHP 8.2.33 across cold and
cached loads, health and missing-route requests, deleted and malformed caches,
and concurrent atomic publication. That checkpoint did not claim complete
destructuring, diagnostic or PHP 8.2 compatibility, and the broader PHP 8.2
corpus-convergence goal remains active.

The preceding `caf5590` checkpoint, relative to the retained `f1990e4`
baseline, added one exact pass without losing a previous pass or adding a crash
or timeout. Constant array expressions now apply PHP unpack semantics while
folding: integer keys
are reindexed, string keys overwrite in insertion order and forward `self::`
class-constant dependencies resolve before property defaults. Class-constant
dependency cycles link successfully and raise a catchable `Error` only when an
affected value is read. When a permitted deferred `new` expression prevents
folding a constant, parameter default or static-local initializer, the runtime
materializer retains the constant-expression context and rejects Traversable
objects rather than applying ordinary runtime array-unpack rules. Original
clean-room regressions cover top-level and class constants, static properties,
forward dependencies, key behavior, delayed cycle errors and deferred defaults
and static locals.

The exact addition is `Zend/tests/array_unpack/classes.phpt`. The related
`array_unpack/gh9769.phpt` now reaches the correct constant-expression error
message but remains an exact failure because general uncaught stack-trace and
source-location formatting is still incomplete. The pinned Symfony
FrameworkBundle 7.4.16 S3 gate remains green against PHP 8.2.33 across cold and
cached loads, health and missing-route requests, deleted and malformed caches,
and concurrent atomic publication. That checkpoint did not claim complete
constant-expression, class-constant, diagnostic or PHP 8.2 compatibility.

The preceding `f1990e4` checkpoint, relative to the retained `a89e091`
baseline, added six exact passes without losing a previous pass or adding a
crash or timeout. A local
`unset($variable)` now replaces the compiled-variable binding itself rather
than assigning `undef` through a reference target. Other local names, array
elements and object properties that own the same reference cell retain both
the cell and its value; assigning the detached name later creates an
independent variable. Original clean-room regressions cover multiple local
aliases, array and property owners, rebinding after unset and the last local
alias disappearing while a container still owns the value.

The exact additions are `Zend/tests/array_unpack/ref1.phpt`,
`Zend/tests/bug68262.phpt` and `Zend/tests/bug72543_1.phpt` through
`bug72543_4.phpt`. The pinned Symfony FrameworkBundle 7.4.16 S3 gate remains
green against PHP 8.2.33 across cold and cached loads, health and missing-route
requests, deleted and malformed caches, and concurrent atomic publication.
This checkpoint does not claim complete reference, unset, copy-on-write or PHP
8.2 compatibility.

The preceding `a89e091` checkpoint, relative to the retained `8bcc548`
baseline, added 14 exact passes without losing a previous pass or adding a
crash or timeout. Array literal unpacking now consumes arrays and
`Traversable` objects through the shared iterator protocol while retaining its
own PHP value contract: integer
keys and canonical numeric-string iterator keys are reindexed, string keys
overwrite in insertion order, and source reference cells are copied by value.
Invalid sources and iterator keys raise catchable `Error` objects with the
array-unpack diagnostics. Exhausting the signed integer key space raises the
PHP error instead of wrapping from `PHP_INT_MAX` into negative keys. Original
clean-room regressions cover arrays, a user `IteratorAggregate`, key
normalization and overwrite order, references, catchable failures and the
integer boundary.

The exact additions are `Zend/tests/array_unpack/already_occupied.phpt`,
`array_unpack/basic.phpt`, `array_unpack/non_integer_keys.phpt`,
`array_unpack/string_keys.phpt`, `Zend/tests/bug60573.phpt`,
`bug60573_2.phpt`, `bug73987_1.phpt`, `bug73987_3.phpt`, `bug80126.phpt`,
`bug80126_2.phpt`, `generators/gh11028_1.phpt`,
`type_declarations/typed_properties_016.phpt`,
`type_declarations/variance/object_variance.phpt` and
`type_declarations/variance/parent_in_class_success.phpt`. The pinned Symfony
FrameworkBundle 7.4.16 S3 gate remains green against PHP 8.2.33 across cold and
cached loads, health and missing-route requests, deleted and malformed caches,
and concurrent atomic publication. Constant-expression unpacking,
destructuring-spread diagnostics, unsetting the last external reference and
compile-time rejection of statically invalid sources remain separate visible
boundaries; that checkpoint did not claim complete array, iterator or PHP 8.2
compatibility.

The preceding `8bcc548` checkpoint, relative to the retained `0afbae3`
baseline, added 21 exact passes without losing a previous pass or adding a
crash or timeout. PHP source argument unpacking now uses a dedicated compiler
and baseline-VM protocol
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
atomic publication. That checkpoint does not claim complete call, iterator,
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
[`f8fc462-arm64-manifest.jsonl`](../tests/php-src/results/php-8.2.33/f8fc462-arm64-manifest.jsonl),
with aggregate metadata in
[`f8fc462-arm64-summary.json`](../tests/php-src/results/php-8.2.33/f8fc462-arm64-summary.json).
The retained directory/status navigation map is
[`8bcc548-arm64-coverage-map.json`](../tests/php-src/results/php-8.2.33/8bcc548-arm64-coverage-map.json),
and the full reference aggregate is
[`reference-arm64-summary.json`](../tests/php-src/results/php-8.2.33/reference-arm64-summary.json),
with image and official-runner cross-checks in
[`reference-validation.json`](../tests/php-src/results/php-8.2.33/reference-validation.json).
Every upstream path remains visible; the rollup never replaces the manifest.

To reproduce the RPHP contract run from the exact external checkout:

```sh
cargo build --locked --release
RPHP_PHPT_PHP_SRC_COMMIT=651db3ebfa622cae0c4e6b39766812efbd274ced \
RPHP_PHPT_REFERENCE_PHP=/path/to/php-8.2.33 \
RPHP_PHPT_FEATURES=default \
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
RPHP_PHPT_FEATURES=all-features \
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
