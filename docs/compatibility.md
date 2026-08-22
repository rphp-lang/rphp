# Compatibility status

RPHP implements a growing, tested subset of PHP. Its public dependency-platform
identity is PHP 8.5.0; some experimental language behavior remains available as an
experimental RPHP extension, but it is outside that compatibility contract.
RPHP is not certified for a complete PHP version and must not be treated as a
drop-in PHP replacement. Passing a script is evidence only for the exercised
behavior.

The latest measured AMD64 PHP 8.5 contract checkpoint is pinned to php-src
8.5.6 commit `fcc29c8` and candidate commit `c8539999`. Across all 5,599
unmodified `Zend/tests` and `tests/lang` cases, 4,075 pass, 1,224 fail, 115
skip, none remain XFAIL, 185 are unsupported, and none time out or crash. The
headline pass rate is 76.901% and the whole-corpus rate is 72.781%; 4,805 of
5,299 attempted cases reach runtime (90.677%). Relative to exact integration
baseline `cb107ee1`, the pass-set delta is +1/-0.

The exact addition is `Zend/tests/bug45877.phpt`, which moves from a missing-
function runtime failure to pass. Every previous pass is preserved and there
is no other status or category movement. Two sequential final candidate runs
have byte-identical manifests and summaries. Their SHA-256 values are
`025e0d504cc024ce87172814a8b088eeaab362c61fee03e219e2536599d87df1`
and `70145291793e472b9e7874acd3b12f083dea3323b7d4a08b2f3bb05d85e8f4af`.

`array_fill_keys()` now snapshots an input array's values as output keys and
fills every unique key with the supplied mixed value. Integer and canonical
decimal-string keys retain integer identity across the full AMD64 range;
other scalar, resource, array and Stringable keys follow PHP's string
conversion, precision and diagnostic path. Duplicate keys replace the value
without moving their first position. Key references are dereferenced while
filled scalar/array values detach through ordinary COW and filled objects keep
identity. NAN and array-conversion warnings, throwing handlers, Stringable
side effects/exceptions and exact first-argument TypeErrors retain PHP 8.5
ordering.

One original E2E regression is byte-identical to PHP 8.5.9. The unmodified
Zend regression plus all five adjacent `ext/standard` `array_fill_keys` PHPTs
also pass, covering empty, keyed, mixed, reference, object, resource, null,
boolean and float cases. All five Cargo feature configurations,
all-feature/all-target, formatting, PHPT-runner and unsafe-policy self-tests,
the exact unsafe ratchet, Composer 2.8.12 S0, all four Symfony S1 gates and PHP
8.5.9 warmed-kernel S2 and cold-build S3 pass. The production inventory remains
1,619 unsafe blocks, 289 unsafe functions and 331 SAFETY annotations. No
opcode, executor/value/array layout, dependency, production-unsafe or hot
array-path change is made.

A local CPU-pinned 32-pair balanced alternating AMD64 release comparison on an
AMD Ryzen 9 7950X used performance-governor CPU 2, four warmups and no excluded
samples. Batches of 100 empty `-r` requests measured baseline/candidate p10/
median/p90 0.205837/0.207089/0.209550 and
0.206117/0.208036/0.209578 seconds, +0.457% independently and +0.382% paired.
One request executing 500,000 existing `array_fill()` calls measured
0.111736/0.112429/0.124068 and 0.107457/0.108540/0.112891 seconds, -3.459%
independently and -3.857% paired, with exact checksum `374999250000`. Both
controls remain below the +5% gate; paired p10/p90 changes are
-0.643%/+1.084% and -10.761%/-2.148%. As an absolute changed-path sanity
check, fifteen candidate runs of 500,000 `array_fill_keys()` calls measured
p10/median/p90 0.159549/0.164354/0.167286 seconds with the same checksum; no
A/B claim is possible because the baseline lacks the function. The baseline
and candidate binary SHA-256 values are
`0cd4895c48fda348b3312eb32525452747c8a144a318886d059d7ecbb320e72a`
and `748582e77b31863261a82aec96956a17bb78310b00f443b177174a74d22a713e`.

This checkpoint does not claim memory-exhaustion behavior for impractically
large key sets, the complete `ext/standard` array suite, companion array
functions or broader PHP compatibility.

The preceding `defined-function-inventory` checkpoint is pinned to php-src
8.5.6 commit `fcc29c8` and candidate commit `aa65a255`. Across all 5,599
unmodified `Zend/tests` and `tests/lang` cases, 4,074 pass, 1,225 fail, 115
skip, none remain XFAIL, 185 are unsupported, and none time out or crash. The
headline pass rate is 76.882% and the whole-corpus rate is 72.763%; 4,805 of
5,299 attempted cases reach runtime (90.677%). Relative to exact integration
baseline `73147de1`, the pass-set delta is +1/-0.

The exact addition is `Zend/tests/get_defined_functions_basic.phpt`, which
moves from a missing-function runtime failure to pass. Every previous pass is
preserved and there is no other status or category movement. The two arginfo
ZPP mismatch tests that first exposed the missing function now advance to an
independent `chunk_split()` argument failure while remaining in the same
runtime-failure category. Two sequential final candidate runs have
byte-identical manifests and summaries. Their SHA-256 values are
`2e85a774ab2194d1d9c298e93a58313fab3c941464fb519ed3bc9f2807889bfc`
and `2d3e9f3d4f32224b5ffbe6e6109eb57bb25f0094d52caf6a6cbc18410ac6d74c`.

`get_defined_functions()` now reports the live RPHP function table under the
`internal` and `user` keys. Class methods and runtime closure implementation
names are excluded, registered names retain their canonical lower-case form,
and RPHP sorts both lists to make its otherwise unspecified order
deterministic. The deprecated optional `bool $exclude_disabled` parameter has
PHP 8.5-compatible weak and strict scalar boundaries, null and NAN diagnostic
ordering, the parameter-has-no-effect deprecation and throwing-handler
behavior.

The original E2E regression is byte-identical to PHP 8.5.9 and covers real
internal/user inventory, method and closure exclusion, repeated-call
stability, weak/strict arguments and diagnostic-handler exceptions. All five
Cargo feature configurations, all-feature/all-target, formatting, PHPT-runner
and unsafe-policy self-tests, the exact unsafe ratchet, Composer 2.8.12 S0,
all four Symfony S1 gates and PHP 8.5.9 warmed-kernel S2 and cold-build S3
pass. The production inventory is 1,619 unsafe blocks, 289 unsafe functions
and 331 SAFETY annotations. The single new cold unsafe read validates a live
function-table entry before classification; there is no opcode, layout,
dependency or hot registration-path change.

A local CPU-pinned 32-pair balanced alternating AMD64 release comparison on an
AMD Ryzen 9 7950X used performance-governor CPU 2, four warmups and no excluded
samples. Batches of 100 empty `-r` requests measured baseline/candidate p10/
median/p90 0.200053/0.207986/0.217385 and
0.204203/0.210732/0.218119 seconds, +1.320% independently and +0.082% paired.
One request executing two million existing `is_float()` calls measured
0.191177/0.192250/0.194263 and 0.186290/0.187171/0.189396 seconds, -2.642%
independently and -2.618% paired, with exact checksum `2000000`. Both controls
remain below the +5% gate. As an absolute changed-path sanity check, fifteen
candidate runs of 20,000 `get_defined_functions()` calls measured p10/median/
p90 0.690816/0.699904/0.706702 seconds with checksum `7140000`; no A/B claim is
possible because the baseline lacks the function. The baseline and candidate
binary SHA-256 values are
`7dbcca75ca2577319bcdc70080b9f2da933b94687ad8d3cf2712904bdf6eabe2`
and `0cd4895c48fda348b3312eb32525452747c8a144a318886d059d7ecbb320e72a`.

This checkpoint reports RPHP's implemented inventory; it does not claim exact
PHP extension/function-set parity, CLI disabled-function support, PHP list
ordering, `chunk_split()` argument compatibility or broader PHP compatibility.

The preceding `base-convert-contract` checkpoint is pinned to php-src 8.5.6
commit `fcc29c8` and candidate commit `ce214f02`. Across all 5,599
unmodified `Zend/tests` and `tests/lang` cases, 4,073 pass, 1,226 fail, 115
skip, none remain XFAIL, 185 are unsupported, and none time out or crash. The
headline pass rate is 76.864% and the whole-corpus rate is 72.745%; 4,805 of
5,299 attempted cases reach runtime (90.677%). Relative to exact integration
baseline `cfe92d09`, the pass-set delta is +1/-0.

The exact addition is `Zend/tests/bug70124.phpt`, which moves from output
failure to pass. Adding the missing public function exposes the test's earlier
argument-evaluation errors; its lower-case static call now also renders the
canonical declared class name in the cold undefined-method diagnostic. Every
previous pass is preserved and there is no other non-pass status or category
movement. Two sequential final candidate runs have byte-identical manifests
and summaries. Their SHA-256 values are
`a0642dbef2e245812fcb015735518852d5759a68366e832f07159d5ae5710944`
and `1f5b4f27de0cc0d2bddb410d3443725d3ae15232d15aef7ceaf72499b35332bf`.

`base_convert()` implements PHP 8.5's case-insensitive bases 2 through 36,
matching `0b`, `0o` and `0x` prefixes, admitted leading/trailing whitespace
and the historical ignore-with-deprecation behavior for other invalid bytes.
Accumulation stays in a signed 64-bit integer until overflow and then follows
PHP's floating-point conversion path, including its observable rounding and
the `ValueError` for an infinite intermediate value. The weak and strict
`string`, `int`, `int` call boundary covers scalar conversions, null and lossy
float deprecations, stringable objects, concrete invalid types, base-range
errors and exceptions from conversion or diagnostic handlers.

Two original unit tests and one E2E regression cover the conversion grammar,
integer/double boundary, precision loss, invalid characters and bases,
weak/strict arguments, Stringable behavior, handler exceptions, infinity and
canonical static-call diagnostics. A deterministic clean-room sweep of
502,245 input/from-base/to-base combinations is byte-identical to PHP 8.5.9;
both outputs have SHA-256
`3df636915bdfcc5aa70b07d0486246b29ced84274a7eda3f349031bf5168e605`.
An independent scalar/container argument matrix is also byte-identical, with
SHA-256
`c8012f1260e534fe4527729b4aed7e4b31c75145359713681411f51dd6646b0c`.

All five Cargo feature configurations, all-feature/all-target, formatting,
PHPT-runner and unsafe-policy self-tests, the exact unsafe ratchet, Composer
2.8.12 S0, all four Symfony S1 gates and PHP 8.5.9 warmed-kernel S2 and
cold-build S3 pass. The production inventory remains 1,618 unsafe blocks, 289
unsafe functions and 330 SAFETY annotations. No opcode, frame, value/object,
executor-globals, dependency or production-unsafe change is made; ordinary
static-call dispatch and its hot cache are unchanged.

A local CPU-pinned 32-pair balanced alternating AMD64 release comparison on an
AMD Ryzen 9 7950X used performance-governor CPU 2, four warmups and no excluded
samples. Batches of 100 empty requests measured baseline/candidate p10/median/
p90 0.202734/0.205041/0.211595 and 0.200989/0.203758/0.208873 seconds,
-0.626% independently and -0.737% paired. One request executing two million
existing `is_float()` calls measured 0.181775/0.182297/0.185102 and
0.185114/0.185699/0.186195 seconds, +1.866% independently and +1.831% paired,
with exact checksum `2000000`. Both controls remain below the +5% gate. As an
absolute changed-path sanity check, fifteen candidate runs of 500,000 decimal-
to-hexadecimal conversions measured p10/median/p90
0.110259/0.110763/0.112369 seconds with checksum `2430096`; no A/B claim is
possible because the baseline lacks the function. The baseline and candidate
binary SHA-256 values are
`8df8e168b3aa69623c4612464b99276c3e6dc434be5ce63282baf7659b24cffd`
and `7dbcca75ca2577319bcdc70080b9f2da933b94687ad8d3cf2712904bdf6eabe2`.

This checkpoint does not claim `bindec()`, `octdec()`, `hexdec()`, arbitrary-
precision conversion or broader PHP compatibility.

The latest measured AMD64 PHP 8.5 contract checkpoint is pinned to php-src
8.5.6 commit `fcc29c8` and candidate commit `7a1e2af7`. Across all 5,599
unmodified `Zend/tests` and `tests/lang` cases, 4,072 pass, 1,227 fail, 115
skip, none remain XFAIL, 185 are unsupported, and none time out or crash. The
headline pass rate is 76.845% and the whole-corpus rate is 72.727%; 4,805 of
5,299 attempted cases reach runtime (90.677%). Relative to exact integration
baseline `a2491a71`, the pass-set delta is +2/-0.

The exact additions are `Zend/tests/zend_ini/gh16886.phpt` and
`Zend/tests/zend_ini/gh16892.phpt`, both previously runtime failures caused by
the missing public function. Every previous pass is preserved, there is no
other non-pass status or category movement, and two sequential final candidate
runs have byte-identical manifests and summaries. Their SHA-256 values are
`b36258fec1d72b0375de4271070c32ecad2458662c248c1f920e11d4921a4d96`
and `db41b2075330d9ad7fec1d1ba4d3a7ee132fa65615c6cc7995e40309cf4a9dbd`.

`ini_parse_quantity()` now parses signed decimal, legacy or explicit octal,
binary and hexadecimal quantities plus case-insensitive K/M/G powers of 1024.
It preserves PHP 8.5's signed-range boundary and historical wrapping result,
including unsigned-parser saturation, while emitting the distinct invalid-
prefix, missing-prefix-digit, missing-leading-digit, unknown-multiplier,
partial-parse and overflow warnings. Diagnostic rendering retains PHP's byte
escapes and interpreted-prefix text. Weak callers accept scalar string
coercions, deprecate null and invoke stringable objects; strict callers accept
only strings. Invalid arrays, resources, closures and non-stringable objects
produce the parameter-specific `TypeError`, and exceptions from error handlers
or `__toString()` stop the call before publishing a return value.

The complete pinned `Zend/tests/zend_ini` slice moves from one pass, two
failures, ten extension skips and one unsupported CLI-INI case to three passes,
zero failures, the same ten skips and the same unsupported case. Its final
manifest/summary hashes are
`b610908469c5215b3190e266397f79e3097564ea564ae359a99a4c42bfde273a`
and `2d795c835b255386aa8f30b7bd27ac0e9702af1c3b5ba7fa772cc2d0a484fe4d`.
Three original parser tests and one E2E regression cover bases, multipliers,
signed boundaries, saturation, warning escaping, weak/strict calls, null,
stringable objects and invalid types. An additional deterministic clean-room
sweep of 5,832 three-byte ASCII inputs is byte-identical to PHP 8.5.9; both
outputs have SHA-256
`7db5293ac71e062a7368c2e2cee0d8a6e6f8b081d93f6710cdea06eca6183165`.

All five Cargo feature configurations, all-feature/all-target, formatting,
PHPT-runner and unsafe-policy self-tests, the exact unsafe ratchet, Composer
2.8.12 S0, all four Symfony S1 gates and PHP 8.5.9 warmed-kernel S2 and
cold-build S3 pass. The production inventory remains 1,618 unsafe blocks, 289
unsafe functions and 330 SAFETY annotations. No opcode, frame, value/object,
executor-globals, dependency or production-unsafe change is made; one cold
stdlib registry entry is added.

A local CPU-pinned 32-pair balanced alternating AMD64 release comparison on an
AMD Ryzen 9 7950X used performance-governor CPU 2, four warmups and no excluded
samples. Batches of 100 empty requests measured baseline/candidate p10/median/
p90 0.156831/0.157646/0.158806 and 0.157320/0.157980/0.160204 seconds,
+0.212% independently and +0.239% paired. One request executing 200,000
existing `parse_ini_string()` calls measured 0.119520/0.122135/0.125626 and
0.122494/0.125020/0.127595 seconds, +2.362% independently and +2.462% paired,
with exact checksum `4200000`. Both controls remain below the +5% gate. As an
absolute changed-path sanity check, fifteen candidate runs of one million
mixed valid quantity calls had a 0.158522-second median and checksum
`1056768000000`; no A/B claim is possible because the baseline lacks the
function. The baseline and candidate binary SHA-256 values are
`a2b1a032f970a747ca4e7ad484546b7d5a6f29c93c4989b2026bc5704effc8fd`
and `8df8e168b3aa69623c4612464b99276c3e6dc434be5ce63282baf7659b24cffd`.

This checkpoint does not claim quantity parsing for arbitrary CLI/INI setting
storage, the unavailable `zend_test` helpers, the remaining unsupported CLI-
INI case, ordinary Reflection signature snapshots or broader PHP
compatibility.

The latest measured AMD64 PHP 8.5 contract checkpoint is pinned to php-src
8.5.6 commit `fcc29c8` and candidate commit `b1c3757b`. Across all 5,599
unmodified `Zend/tests` and `tests/lang` cases, 4,070 pass, 1,229 fail, 115
skip, none remain XFAIL, 185 are unsupported, and none time out or crash. The
headline pass rate is 76.807% and the whole-corpus rate is 72.692%; 4,805 of
5,299 attempted cases reach runtime (90.677%). Relative to exact integration
baseline `cb3941a3`, the pass-set delta is +1/-0.

The exact addition is `Zend/tests/string_to_number_comparison.phpt`, which
moves from output failure to pass. Every previous pass is preserved, there is
no other non-pass status or category movement, and two sequential final
candidate runs have byte-identical manifests and summaries. Their SHA-256
values are
`1248fb5b91d224ff0ad903f3f58301ffe5dac0869291c4e6f51dcc744b55af31`
and `7ba8788af97b62ce4905c9649a491dc5a8439fa4e92ba911f06ee8ef6d8bb999`.

Dynamic float-to-nonnumeric-string equality and ordering now render the number
with the request-local `precision` before PHP's lexical comparison. Reversed
operands and scalar leaves nested in arrays or same-class objects use the same
rule. Complete numeric strings retain numeric comparison, non-finite values
retain their separate PHP ordering, and ordinary number/number or string/string
fast paths remain unchanged. The compiler snapshots precision for scalar
constant comparisons: a startup `-d precision` setting controls the main unit,
runtime `ini_set()` affects dynamic expressions, and a later `include` or
`eval` snapshots the value active when that source unit is compiled. This also
preserves PHP's observable distinction between a previously compiled literal
comparison and an otherwise equivalent comparison through a variable.

The focused upstream case moves from zero passes to one. Its final
manifest/summary hashes are
`6110877824c85d83d9bd9cab47c0d12512f5c1611a603e8fb9622fe38f4c2410`
and `30b274ef5ffe1031805f1fea8f7d7cae255daae580d676f26ae3715c001bd18c`.
Five original CLI regressions cover all loose relational operators, reversed
operands, nested array/object leaves, numeric strings, infinity, startup
constant folding and runtime `ini_set()` across the main, include and eval
compiler boundaries.

All five Cargo feature configurations, all-feature/all-target, formatting,
PHPT-runner and unsafe-policy self-tests, the exact unsafe ratchet, Composer
2.8.12 S0, all four Symfony S1 gates and PHP 8.5.9 warmed-kernel S2 and
cold-build S3 pass. The production inventory remains 1,618 unsafe blocks, 289
unsafe functions and 330 SAFETY annotations. No opcode, frame, value/object,
executor-globals or dependency layout changes and no production-unsafe change
are made; the cold compiler gains one precision snapshot field.

A local CPU-pinned 32-pair balanced alternating AMD64 release comparison on an
AMD Ryzen 9 7950X used CPU 2 with the performance governor, four warmups and no
excluded samples. Batches of 100 empty requests measured baseline/candidate
p10/median/p90 0.150914/0.151812/0.152886 and
0.151215/0.152585/0.153744 seconds, +0.509% independently and +0.433% paired.
Five million existing integer comparisons measured
0.177918/0.179786/0.182280 and 0.174730/0.179206/0.182192 seconds, -0.323%
independently and -0.923% paired. One million changed-path float/string
comparisons measured 0.143272/0.144055/0.144983 and
0.142676/0.144088/0.145009 seconds, +0.022% independently and -0.033% paired.
Compiling and executing 5,000 dynamic float/string comparisons measured
0.289647/0.290564/0.292832 and 0.289360/0.291949/0.293786 seconds, +0.477%
independently and +0.192% paired. All remain below the +5% gate with exact
output and status. The baseline and candidate binary SHA-256 values are
`a21843f3af976472e435dbd0e20288fc6ce6b95c03371943a814346b9b5fb788`
and `a2b1a032f970a747ca4e7ad484546b7d5a6f29c93c4989b2026bc5704effc8fd`.

This checkpoint does not claim the independent string-offset output behavior,
`base_convert()`, deferred Reflection-attribute comparison snapshots or broader
PHP compatibility.

The latest measured AMD64 PHP 8.5 contract checkpoint is pinned to php-src
8.5.6 commit `fcc29c8` and candidate commit `8520c67e`. Across all 5,599
unmodified `Zend/tests` and `tests/lang` cases, 4,069 pass, 1,230 fail, 115
skip, none remain XFAIL, 185 are unsupported, and none time out or crash. The
headline pass rate is 76.788% and the whole-corpus rate is 72.674%; 4,805 of
5,299 attempted cases reach runtime (90.677%). Relative to exact integration
baseline `f80af9ae`, the pass-set delta is +2/-0.

The exact additions are `Zend/tests/bug42143.phpt` and
`Zend/tests/bug73954.phpt`, both previously runtime failures. Every previous
pass is preserved. `Zend/tests/empty_str_offset.phpt`,
`Zend/tests/isset/isset_str_offset.phpt` and
`Zend/tests/string_to_number_comparison.phpt` advance from missing-builtin or
missing-constant runtime failures to their independent string-offset and
precision-zero float/string comparison output boundaries. They remain visible
failures, and there are no other non-pass status or category movements. Two
sequential final candidate runs have byte-identical manifests and summaries.
Their SHA-256 values are
`446ac9922bb0b6060c979df31191270843da7724b2e5aea3156b6cc9d5a53ac6`
and `05b1ee6bb715d7f43ffd25e0698c67949cc9da614c26f8251b6ec45dfa63e4c5`.

`is_nan()`, `is_finite()` and `is_infinite()` now share PHP 8.5's internal
`float $num` boundary. Floats and widening integers work in strict and weak
callers; weak calls additionally accept booleans and complete PHP numeric
strings, including decimal overflow to infinity. Textual Rust spellings such
as `NAN` and `INF` remain invalid strings. Weak null conversion emits PHP's
parameter-specific deprecation before classifying zero, while strict scalar
mismatches and invalid arrays or objects produce the exact `TypeError`,
including concrete class and `true`/`false` diagnostics. Ordinary,
namespaced-fallback, dynamic and first-class calls use the same path. The
`M_PI` constant exposes the same double as `pi()`.

The focused three-case upstream slice moves from zero passes to two; its one
remaining case is the independent precision-zero comparison boundary above.
Its final manifest/summary hashes are
`501bd5638aabf3b108fce19547341ae444277a96123b2eabc616aad09820596a`
and `7a11cd3d956cc99c6752241f8ba56012cfd17bcdea9fe8c0d9c16e08e4e1a073`.
Five original CLI regressions cover finite/infinite/NaN classification,
`M_PI`, weak scalar and null behavior, numeric overflow, strict widening,
invalid-type diagnostics, namespaced fallback and first-class calls.

All five Cargo feature configurations, all-feature/all-target, formatting,
PHPT-runner and unsafe-policy self-tests, the exact unsafe ratchet, Composer
2.8.12 S0, all four Symfony S1 gates and PHP 8.5.9 warmed-kernel S2 and
cold-build S3 pass. The production inventory remains 1,618 unsafe blocks, 289
unsafe functions and 330 SAFETY annotations. No opcode, frame, value/object
layout, dependency or production-unsafe change is made.

A local CPU-pinned 32-pair balanced alternating AMD64 release comparison on an
AMD Ryzen 9 7950X used CPU 2 with the performance governor, four warmups and no
excluded samples. Batches of 100 empty requests measured baseline/candidate
p10/median/p90 0.152417/0.153287/0.156623 and
0.151788/0.152610/0.157642 seconds, -0.442% independently and -0.345% paired.
Two million existing `is_float()` classifications measured
0.191234/0.192143/0.192963 and 0.184926/0.185388/0.186633 seconds, -3.516%
independently and -3.440% paired. Five hundred thousand existing
`defined('PHP_FLOAT_NAN')` lookups measured 0.067530/0.068004/0.069110 and
0.066951/0.067389/0.068143 seconds, -0.905% independently and -0.821% paired.
All remain below the +5% gate with exact output and status. The baseline and
candidate binary SHA-256 values are
`2e050fbacd9afec5bc8e3ab9fa5618bde3ce006711898aa4c29dfe2eebc91b64`
and `a21843f3af976472e435dbd0e20288fc6ce6b95c03371943a814346b9b5fb788`.

This checkpoint does not claim `base_convert()`, the independent string-offset
or precision-zero comparison behavior above, or broader PHP compatibility.

The latest measured AMD64 PHP 8.5 contract checkpoint is pinned to php-src
8.5.6 commit `fcc29c8` and candidate commit `b6910daa`. Across all 5,599
unmodified `Zend/tests` and `tests/lang` cases, 4,067 pass, 1,232 fail, 115
skip, none remain XFAIL, 185 are unsupported, and none time out or crash. The
headline pass rate is 76.750% and the whole-corpus rate is 72.638%; 4,805 of
5,299 attempted cases reach runtime (90.677%). Relative to exact integration
baseline `ac0bdf91`, the pass-set delta is +14/-0.

The exact additions are `Zend/tests/exit/define_die_constant.phpt`,
`define_die_constant_namespace.phpt`, `define_die_function.phpt`,
`define_die_function_namespace.phpt`, `define_exit_constant.phpt`,
`define_exit_constant_namespace.phpt`, `define_exit_function.phpt`,
`define_exit_function_namespace.phpt`, all four `define_goto_label_*` cases,
`die_string_cast_exception.phpt` and `exit_as_function.phpt`. Eleven output and
three runtime failures become exact with every previous pass preserved.
`Zend/tests/throw/leaks.phpt` advances from an undefined-constant runtime error
to its independent `error_reporting()` output mismatch after its bare `exit`
now executes; it remains a visible failure. There are no other non-pass status
or category movements. Two sequential final candidate runs have byte-identical
manifests and summaries. Their SHA-256 values are
`de8b76b4d48e44fa189c4b8a699b4f696f06faf6c12667f0235fab01be058e22`
and `d05d347480235bbb5e89f91f006c7bda84d39092dbbd4d8d3dae4650059de672`.

Case-insensitive unqualified `exit` and `die` now share PHP 8.5's reserved
keyword and canonical direct-function identity. Bare, parenthesized, named-
argument and first-class forms use the same call path; global declarations,
imports and goto labels stop with the required parse diagnostics. Qualified
function names remain ordinary names, while class methods/constants, enum cases
and named-argument labels retain their relaxed keyword spelling. The internal
`string|int` contract handles strict calls, weak null/bool/float conversion,
precision-loss deprecations, `NAN`, concrete invalid types, stringable objects
and exceptions from diagnostics or `__toString()` before process exit.

The focused 27-case upstream `Zend/tests/exit` cluster moves from 5 passes to
19, with five failures and three explicit unsupported CLI-INI cases remaining;
the final focused manifest/summary hashes are
`3e24dc5064403ff564b351a2bfb7fe1455686d21498ca350d2977f4d12a613e1`
and `0a96fbefd14225ec98a5db686c8ab6c742fedf354832a464c0aa7cf9c97a064e`.
Five original CLI regressions cover declaration diagnostics, relaxed member
names, canonical callable identity, evaluation order, weak scalar boundaries,
strict and structural type errors, re-entrant diagnostics and stringable
objects. An adjacent 799-case grammar, namespace, callable, jump, enum,
closure, magic-method and coercion slice has 551 passes, 216 failures, 21 skips
and 11 unsupported cases, with the same exact +14/-0 pass-set delta.

All five Cargo feature configurations, all-feature/all-target, formatting,
PHPT-runner and unsafe-policy self-tests, the exact unsafe ratchet, Composer
2.8.12 S0, all four Symfony S1 gates and PHP 8.5.9 warmed-kernel S2 and
cold-build S3 pass. The production inventory remains 1,618 unsafe blocks, 289
unsafe functions and 330 SAFETY annotations. No opcode, frame, value/object
layout, dependency or production-unsafe change is made.

A local CPU-pinned 32-pair balanced alternating AMD64 release comparison on an
AMD Ryzen 9 7950X used CPU 2 with the performance governor, four warmups and no
excluded samples. Batches of 100 empty requests measured baseline/candidate
p10/median/p90 0.144919/0.145430/0.146189 and
0.145911/0.146761/0.147858 seconds, +0.916% independently and +1.017% paired.
Batches of 12 requests compiling 1,000 ordinary identifier writes measured
0.081616/0.081972/0.084298 and 0.082079/0.082470/0.084405 seconds, +0.608%
independently and +0.421% paired. Batches of five requests compiling and
executing 5,000 short-circuited exit expressions measured
0.671911/0.673822/0.679266 and 0.674386/0.676460/0.688085 seconds, +0.392%
independently and +0.451% paired. All remain below the +5% gate with exact
output and status. The baseline and candidate binary SHA-256 values are
`2a6e48b145cbc6cc64326ffefdb08c2a3724620e82cccf03c0c3c32b74a488b7`
and `2e050fbacd9afec5bc8e3ab9fa5618bde3ce006711898aa4c29dfe2eebc91b64`.

This checkpoint does not claim the missing process helpers `exec()` and
`escapeshellarg()`, `auto_prepend_file` or `disable_functions` CLI-INI support,
automatic output-buffer chunk callbacks, or broader PHP compatibility.

The latest measured AMD64 PHP 8.5 contract checkpoint is pinned to php-src
8.5.6 commit `fcc29c8` and candidate commit `185f06bd`. Across all 5,599
unmodified `Zend/tests` and `tests/lang` cases, 4,053 pass, 1,246 fail, 115
skip, none remain XFAIL, 185 are unsupported, and none time out or crash. The
headline pass rate is 76.486% and the whole-corpus rate is 72.388%; 4,817 of
5,299 attempted cases reach runtime (90.904%). Relative to exact integration
baseline `cee13fc9`, the pass-set delta is +12/-0.

The exact additions are `Zend/tests/constexpr/new_dynamic_class_name.phpt`,
all ten previously failing cases under `Zend/tests/new_without_parentheses`,
and `Zend/tests/varSyntax/newVariable.phpt`. Five parse, six output and one
runtime failure become exact, with every previous pass preserved and no other
non-pass status or category movement. The runtime-reach count falls by two
because four invalid forms now stop at PHP's required parse error instead of
executing before an output mismatch; valid cases advancing to runtime partly
offset that correction. Two sequential final candidate runs have
byte-identical manifests and summaries. Their SHA-256 values are
`181cbfec43b450df1770f20bbb2787652e219e8a18f2bcea6718a604ea315f05`
and `16f50a0c60f04a11bcbb630e30955506c812699758086939b5ed3214e6c3c496`.

Dynamic `new (expression)` and named or dynamic static-property class operands
now follow PHP 8.5 grammar. Constructor parentheses enable result postfixes;
without them, named and grouped forms produce the context-sensitive parse
diagnostic required by echo, calls, returns and standalone statements. Bare
new results also reject assignment and `unset()` with PHP's exact diagnostic.
The dynamic class operand is evaluated before constructor arguments and stays
live across argument unpacking and generator suspension, so side effects and
exceptions occur once and in source order.

The focused 12-case upstream cluster and four original CLI regressions pass at
12/12 and 4/4. The adjacent new/grammar/dynamic-call/class-name/object slice has
132 passes, 57 failures, four skips and one unsupported case, an exact +10/-0
delta from the retained baseline. All five Cargo feature configurations,
all-feature/all-target, formatting, PHPT-runner and unsafe-policy self-tests,
the exact unsafe ratchet, Composer 2.8.12 S0, all four Symfony S1 gates and PHP
8.5.9 warmed-kernel S2 and cold-build S3 pass. The production inventory remains
1,618 unsafe blocks, 289 unsafe functions and 330 SAFETY annotations. No
opcode, frame, value layout, dependency or production-unsafe change is made;
the parser gains one optional diagnostic-context field.

A local CPU-pinned 32-pair balanced alternating AMD64 release comparison on an
AMD Ryzen 9 7950X used CPU 2 with the performance governor, four warmups and no
excluded samples. Batches of 100 empty requests measured baseline/candidate
p10/median/p90 0.201642/0.204214/0.209993 and
0.202835/0.205406/0.209492 seconds, +0.584% independently and +0.655% paired.
One request compiling 25,000 named `new` expressions measured
0.204661/0.206236/0.208442 and 0.205893/0.208462/0.210263 seconds, +1.079%
independently and +0.778% paired. Batches of three requests executing 100,000
dynamic property class constructions each measured 0.115278/0.116888/0.118412
and 0.112185/0.112973/0.114151 seconds, -3.349% independently and -3.305%
paired. All remain below the +5% gate with exact output. The baseline and
candidate binary SHA-256 values are
`bc912b3e0bb41a12ccbb9496938f993804ac69d6f7eb9406a57a26e8c0758d7b`
and `2a6e48b145cbc6cc64326ffefdb08c2a3724620e82cccf03c0c3c32b74a488b7`.

This checkpoint does not claim broader SPL or `ArrayObject` behavior,
independent diagnostics outside the covered new-expression forms, or broader
PHP compatibility.

The latest measured AMD64 PHP 8.5 contract checkpoint is pinned to php-src
8.5.6 commit `fcc29c8` and candidate commit `4a6fb864`. Across all 5,599
unmodified `Zend/tests` and `tests/lang` cases, 4,041 pass, 1,258 fail, 115
skip, none remain XFAIL, 185 are unsupported, and none time out or crash. The
headline pass rate is 76.260% and the whole-corpus rate is 72.174%; 4,819 of
5,299 attempted cases reach runtime (90.942%). Relative to exact integration
baseline `ae447c20`, the pass-set delta is +18/-0.

The exact additions are `Zend/tests/constants/gh18850.phpt`, all thirteen
cases under `Zend/tests/constants/halt_compiler`,
`Zend/tests/namespaces/ns_080.phpt`, and the three adjacent undefined-constant
cases `Zend/tests/errmsg/bug43344_1.phpt`, `Zend/tests/match/045.phpt` and
`Zend/tests/namespaces/ns_076.phpt`. Seven compile, two parse, two output and
seven runtime failures become exact, with every previous pass preserved.
`Zend/tests/closures/bug79778.phpt` advances from its undefined-constant
runtime failure to an independent closure `print_r()` output-formatting gap;
no other non-pass status or category moves. Two sequential final candidate
runs have byte-identical manifests and summaries. Their SHA-256 values are
`8376d22753e9a26eabc49c5e8c574a470cc3e897de92e6ce2a08ae6dd68c9414`
and `397ae0ae18e4fa12d9aa877290250af7c2781d83b52280229afb78e46744ea62`.

Case-insensitive unqualified `__halt_compiler();` is now restricted to the
outermost source scope, records the exact byte immediately after its semicolon
and makes the remaining source opaque. Direct and absolute
`__COMPILER_HALT_OFFSET__` reads, constant declarations, functions and dynamic
`constant()` lookup observe PHP 8.5's source-unit behavior across main files,
includes and eval strings. Repeated eval units retain the first dynamically
registered offset for their shared Zend source name while direct reads remain
local to each compilation. The exact uppercase global name remains reserved,
namespaced names remain ordinary constants, nested directives produce the
compile fatal, and an undefined constant now throws a catchable, source-aware
`Error` through the normal VM trace path.

The focused 15-case upstream cluster and six original end-to-end regressions
pass at 15/15 and 6/6. The adjacent constants/namespaces/grammar slice has 151
passes, 56 failures, two skips and three unsupported cases, an exact +16/-0
delta from the retained baseline. All five Cargo feature configurations,
all-feature/all-target, formatting, PHPT-runner and unsafe-policy self-tests,
the exact unsafe ratchet, Composer 2.8.12 S0, all four Symfony S1 gates and PHP
8.5.9 warmed-kernel S2 and cold-build S3 pass. The production inventory remains
1,618 unsafe blocks, 289 unsafe functions and 330 SAFETY annotations. No
opcode, frame or value layout changes; `ExecutorGlobals` gains one lazy map
pointer.

A local CPU-pinned 32-pair balanced alternating AMD64 release comparison on an
AMD Ryzen 9 7950X used CPU 2 with the performance governor, four warmups and no
excluded samples. Batches of 100 empty requests measured baseline/candidate
medians 0.139821/0.137793 seconds, -1.450% independently and -1.462% paired.
Batches of 12 requests compiling 1,000 ordinary variable writes measured
0.052470/0.052080 seconds, -0.745% independently and -0.681% paired. Batches of
eight requests compiling 1,000 named constant identifiers measured
1.054920/1.059558 seconds, +0.440% independently and +0.565% paired. All remain
below the +5% gate with exact output. The baseline and candidate binary SHA-256
values are `2746ec72ac2ab87c642493cca2de82b0a3baa6ca0d1ade9e4ef809b4beb19884`
and `bc912b3e0bb41a12ccbb9496938f993804ac69d6f7eb9406a57a26e8c0758d7b`.

This checkpoint does not claim PHP's legacy invalid-byte offset behavior,
because the current string frontend remaps those bytes, the independent
closure formatting boundary above, or broader PHP compatibility.

The latest measured AMD64 PHP 8.5 contract checkpoint is pinned to php-src
8.5.6 commit `fcc29c8` and candidate commit `863e3234`. Across all 5,599
unmodified `Zend/tests` and `tests/lang` cases, 4,023 pass, 1,276 fail, 115
skip, none remain XFAIL, 185 are unsupported, and none time out or crash. The
headline pass rate is 75.920% and the whole-corpus rate is 71.852%; 4,811 of
5,299 attempted cases reach runtime (90.791%). Relative to exact integration
baseline `5b4142ac`, the pass-set delta is +20/-0.

The exact additions are `Zend/tests/bug72441.phpt`,
`foreach/foreach_list_004.phpt`, `list/bug73663.phpt`, `bug73663_2.phpt`,
all three `gh11320` cases, `list_008.phpt`, `list_010.phpt`, `list_011.phpt`,
`list_014.phpt`, `list_empty_error.phpt`, `list_empty_error_keyed.phpt`,
`list_keyed_conversions.phpt`, all four `list_keyed_evaluation_order` cases,
`list_keyed_leading_comma.phpt`, and `list_mixed_keyed_unkeyed.phpt`. Twelve
move from parse failure, six from output mismatch and two from runtime failure;
every previous pass remains a pass. `list_keyed_ArrayAccess.phpt` advances from
parse failure to the independent `ArrayObject` mutation runtime boundary, and
`list_keyed_non_literals.phpt` advances from parse failure to the independent
`data:` stream-key output boundary. No other status or category moves. Two
sequential final candidate runs have byte-identical manifests and summaries.
Their SHA-256 values are
`b920f1eb8c6c94c7b67f07bc7d26180b7f4f8ed2cc3351711b6f2efd923e7031`
and `2d86ed267ed0f445b6afa5916b4c9b6abe019b28b06dbbe42ce0aed31cb948ce`.

Legacy `list()` is now value-producing expression grammar, including uppercase
spelling and direct or named call arguments. Keyed destructuring accepts
compile-time, magic and arbitrary runtime key expressions plus reference,
nested, append, array-dimension, object-property and static-property writable
targets. The source is evaluated first; each key, source fetch and destination
then executes in left-to-right PHP 8.5 order. Integer and numeric-string keys
remain distinct for `ArrayAccess`, arrow functions capture free variables from
keys, sources and member targets, and only a pattern containing a reference can
be passed to a by-reference parameter. Empty patterns, empty keyed entries,
mixed keyed/unkeyed entries, mixed `[]`/`list()` nesting and invalid writable
targets produce PHP's compile-time diagnostics before the source can execute.

The 39-case original list/global/static suite and the focused 21-case upstream
cluster pass at 39/39 and 19/21; the latter has only the two independent
boundaries above. All five Cargo feature configurations, all-feature/all-target,
formatting, PHPT-runner and unsafe-policy self-tests, the exact unsafe ratchet,
Composer 2.8.12 S0, all four Symfony S1 gates and PHP 8.5.9 warmed-kernel S2
and cold-build S3 pass. The production inventory remains 1,618 unsafe blocks,
289 unsafe functions and 330 SAFETY annotations.

A local CPU-pinned 32-pair balanced alternating AMD64 release comparison on an
AMD Ryzen 9 7950X used CPU 0 with the performance governor, four warmups and no
excluded samples. Batches of 100 empty requests measured baseline
p10/median/p90 0.231270/0.233732/0.241504 seconds and candidate
0.230754/0.232903/0.238323 seconds, -0.355% by independent medians and -0.248%
by the paired median. One request parsing, compiling and executing two million
positional-plus-keyed literal destructuring iterations measured baseline
0.467177/0.477029/0.485620 seconds and candidate
0.462147/0.468815/0.473345 seconds, -1.722% independently and -1.857% paired.
Both remain below the +5% gate with exact output. The baseline and candidate
binary SHA-256 values are
`a696ea142435a42d35369f874ff28c78562ced8fc8cc9bdaf10cf91b6b5af212`
and `2746ec72ac2ab87c642493cca2de82b0a3baa6ca0d1ade9e4ef809b4beb19884`.

This checkpoint does not claim the remaining `ArrayObject` write or `data:`
stream-wrapper behavior above, other independent SPL/stream gaps, or broader
PHP compatibility.

The latest measured AMD64 PHP 8.5 contract checkpoint is pinned to php-src
8.5.6 commit `fcc29c8` and candidate commit `9450ecb8`. Across all 5,599
unmodified `Zend/tests` and `tests/lang` cases, 4,003 pass, 1,296 fail, 115
skip, none remain XFAIL, 185 are unsupported, and none time out or crash. The
headline pass rate is 75.543% and the whole-corpus rate is 71.495%; 4,798 of
5,299 attempted cases reach runtime (90.545%). Relative to exact integration
baseline `5da46076`, the pass-set delta is +11/-0.

The exact additions are `Zend/tests/binary.phpt`, `bug74947.phpt`,
`double_to_string_64bit.phpt`, `int_underflow_64bit.phpt`,
`oct_overflow.phpt`, `tests/lang/bug27354.phpt`, `bug73172.phpt`,
all three 64-bit cases under `tests/lang/integer_literals`, and
`tests/lang/invalid_octal.phpt`. Every previous pass remains a pass.
`Zend/tests/offsets/array_offset_002.phpt` advances from its literal parse
failure to an independent reentrant float-key conversion output mismatch;
`Zend/tests/bug39018.phpt` remains in the parse category but now reaches its
independent error-suppressed assignment-target rejection. No other status or
category moves. Two sequential final candidate runs have byte-identical
manifests and summaries. Their SHA-256 values are
`67c07e1eafda864aaddaf5363f620169ab0427c21f66c000fafb140870776b55`
and `953ee1a0d5ea534e5a61fa2fb7401d9acb9b8bd4513c41fca638069adff403ff`.

On AMD64, decimal, binary, hexadecimal, explicit octal and legacy leading-zero
octal literals now remain integers through `PHP_INT_MAX` and promote to double
after that boundary. The cold overflow conversion reaches finite doubles or
infinity and preserves PHP 8.5's observable binary/octal rounding order;
ordinary `i64` parsing remains the inline fast path. Numeric separators work in
all admitted bases. Legacy octal digits 8 and 9 produce PHP's source-located
`Invalid numeric literal` parse error, including after a compact unary minus,
while leading-zero decimal floats remain decimal.

Original lexer, CLI and end-to-end regressions cover exact double bit patterns,
positive and negative boundaries, separators, explicit and legacy octal,
infinity and invalid-digit diagnostics. A deterministic differential sample of
500 additional literals matched PHP 8.5.9 exactly. The adjacent 17-case numeric
separator/integer-literal slice has 14 passes and the expected three AMD64
32-bit skips. All five Cargo feature configurations, all-feature/all-target,
formatting, PHPT-runner and unsafe-policy self-tests, the exact unsafe ratchet,
Composer 2.8.12 S0, all four Symfony S1 gates and PHP 8.5.9 warmed-kernel S2
and cold-build S3 pass. The production inventory remains 1,618 unsafe blocks,
289 unsafe functions and 330 SAFETY annotations.

A local CPU-pinned 32-pair balanced alternating AMD64 release comparison on an
AMD Ryzen 9 7950X used CPU 0 with the performance governor, four warmups and no
excluded samples. Batches of 100 empty requests measured baseline
p10/median/p90 0.143564/0.144225/0.144779 seconds and candidate
0.143729/0.144236/0.144847 seconds, +0.008% by independent medians and +0.028%
by the paired median. One request lexing, compiling and executing 50,000
ordinary integer expressions measured baseline 0.044994/0.045422/0.046269
seconds and candidate 0.045144/0.045867/0.046712 seconds, +0.978%
independently and +0.801% paired. Both remain below the +5% gate with exact
output. The baseline and candidate binary SHA-256 values are
`928392628b498c69e7cb2378e844871d0ba89e3bdd7f0dcad33517be7aa7b56f`
and `a696ea142435a42d35369f874ff28c78562ced8fc8cc9bdaf10cf91b6b5af212`.

This checkpoint does not claim the remaining error-suppressed assignment or
reentrant float-key behavior above, PHP-compatible diagnostics for malformed
explicit base prefixes, 32-bit integer boundaries, or broader PHP
compatibility.

The latest measured AMD64 PHP 8.5 contract checkpoint is pinned to php-src
8.5.6 commit `fcc29c8` and candidate commit `618532c3`. Across all 5,599
unmodified `Zend/tests` and `tests/lang` cases, 3,992 pass, 1,307 fail, 115
skip, none remain XFAIL, 185 are unsupported, and none time out or crash. The
headline pass rate is 75.335% and the whole-corpus rate is 71.298%; 4,789 of
5,299 attempted cases reach runtime (90.376%). Relative to exact integration
baseline `67e45bcd`, whose executable state is candidate `4b3b69e4`, the
pass-set delta is +4/-0.

The exact additions are `Zend/tests/concat/bug32833.phpt`,
`Zend/tests/falsetoarray.phpt`, `Zend/tests/falsetoarray_002.phpt` and
`Zend/tests/fe_fetch_op2_live_range.phpt`. The first and fourth move from parse
failure to pass, the false-autovivification pair move from parse/runtime
failure to pass, and every previous pass remains a pass. Four remaining
failures advance past the newly admitted append syntax or false conversion:
`ArrayAccess/bug69955.phpt` reaches its independent ArrayAccess compound-append
output mismatch, `assign_dim_obj_null_return.phpt` reaches later max-key and
catchable-scalar assignment behavior, `bug63882_2.phpt` reaches an output
mismatch rooted in recursive serialization returning false, and
`bug70182.phpt` reaches the existing string-append diagnostic mismatch. No
other status, category or stage moves. Two sequential final candidate runs
have byte-identical manifests and summaries. Their SHA-256 values are
`92977b7f9ae8e3d51db645fdc1a8a7e2717cd9ec8879554186e201e1c4482068`
and `43d02056848ad7e6f5f638f3032659189215412d82f8c76957256a4a4bc03385`.

False used as an indexed, appended, nested, referenced, property, static-
property, destructuring, foreach or compound write destination now publishes
an empty array and emits PHP 8.5's
`Automatic conversion of false to array is deprecated` through the ordinary
error-handler pipeline. After a reentrant handler returns, the VM reacquires
the destination and compares array identity plus its pristine state; a handler
replacement wins and the compiler-provided statement boundary prevents later
dimension evaluation or stale synthetic writeback. Mutable nested paths now
interleave each key evaluation with its container fetch, while `unset()` keeps
PHP's deprecation without converting false. Terminal empty dimensions are
also valid compound-assignment and by-value foreach destinations. Null/undef
autovivification and true/integer scalar errors retain their existing paths.

One original four-test CLI suite covers direct, alias, nested, instance/static
property, destructuring, foreach, compound, append-reference and unset forms;
whole-target and child reentrant clobbering; unrelated handler mutation; key,
value and nested foreach append targets; and true/integer negative controls.
The five-case focused upstream slice, all five Cargo feature configurations,
all-features/all-targets, formatting, PHPT-runner and unsafe-policy self-tests,
the exact unsafe ratchet, Composer S0, all four Symfony S1 gates and PHP 8.5.9
warmed-kernel S2 and cold-build S3 pass. The production inventory is 1,618
unsafe blocks against the 1,623 ceiling, 289 unsafe functions and 330 SAFETY
annotations.

A local CPU-pinned 32-pair balanced alternating AMD64 release comparison with
the performance governor, four warmups and no excluded sample keeps all
relevant controls below the +5% gate. Batches of 100 empty requests measure
0.223659 seconds for the exact baseline and 0.223236 seconds for the candidate,
with candidate/baseline ratios of 0.998111 independently and 0.996750 by paired
medians. Three million ordinary indexed-plus-append writes retain output
`8787931422` and measure 0.220782 versus 0.218214 seconds, ratios 0.988370 and
0.983890. One and a half million nested writes retain output
`750000:1499999` and measure 0.569146 versus 0.547530 seconds, ratios 0.962020
and 0.961226. This is evidence of no regression, not an optimization claim.
The exact baseline and candidate binary SHA-256 values are
`732a53c419def118f661aa11cf9425c3aa1481dfe68069786d36fd23c8bf1f22`
and `928392628b498c69e7cb2378e844871d0ba89e3bdd7f0dcad33517be7aa7b56f`.

This checkpoint does not claim the pending terminal write when a conversion
handler throws, effectful terminal-key ordering on a direct false root, typed
boolean-property auto-initialization diagnostics, by-reference foreach append
teardown, ArrayAccess append read-modify-write, string append diagnostics,
recursive serialization fidelity or broader PHP compatibility.

The latest measured AMD64 PHP 8.5 contract checkpoint is pinned to php-src
8.5.6 commit `fcc29c8` and candidate commit `4b3b69e4`. Across all 5,599
unmodified `Zend/tests` and `tests/lang` cases, 3,988 pass, 1,311 fail, 115
skip, none remain XFAIL, 185 are unsupported, and none time out or crash. The
headline pass rate is 75.259% and the whole-corpus rate is 71.227%; 4,783 of
5,299 attempted cases reach runtime (90.262%). Relative to exact integration
baseline `b19f32fa`, the pass-set delta is +1/-0.

The exact addition is `Zend/tests/varSyntax/writeToTempExpr.phpt`, which moves
from compile failure to pass. Every previous pass remains a pass and no other
status or failure category moves. Two sequential candidate runs have
byte-identical merged manifests and summaries. Their manifest and summary
SHA-256 values are
`da35904d48ed169c4cfc62bbfbb74a20b4194abd32a1a6a590fa3969674c0283`
and `8a9e977463222769bfc7d929703d941efc8eb94213b07802abe0fa0183508214`.

The parser now walks an indexed or appended write target to its semantic root
before admitting it. Variables, mutable properties and ordinary call results
retain their established write behavior. Array/string literals, binary,
ternary, coalescing, match, assignment, `new`, pipe and constant results emit
PHP 8.5's compile-time `Cannot use temporary expression in write context`
fatal instead of leaking the compiler's internal unsupported-target message.
The classification applies to indexed assignment, nested append, compound and
coalescing assignment, prefix/postfix mutation, `unset()`, direct and element
reference binding. `clone` retains PHP's distinct
`Cannot use result of built-in function in write context` wording, including
the by-reference foreach source path, because Zend lowers it through a
dedicated result opcode.

One original four-test CLI suite covers ten mutating forms, ten temporary-root
families, the clone boundary, source-unit non-execution and ordinary call,
read, `isset()`, by-value argument and temporary-array foreach controls. All
five Cargo configurations, all-features/all-targets, formatting, PHPT runner
self-test, unsafe self-test and the exact unsafe ratchet pass. The production
inventory remains 1,612 unsafe blocks, 289 unsafe functions and 321 SAFETY
annotations. Composer 2.8.12 S0, all four Symfony S1 gates, and exact PHP 8.5.9
warmed-kernel S2 and cold-build S3 also pass.

A local CPU-pinned 32-pair balanced alternating AMD64 release comparison with
the performance governor, four warmups and no excluded sample keeps both
front-end controls below the +5% gate. One hundred empty cold requests per
observation measure candidate/baseline ratios of 0.994948 by independent
medians and 0.995201 by paired medians. One request compiling and executing
1,000 source units containing an ordinary user-call indexed write and internal
call append measures 0.999530 and 1.001989 respectively, with exact output.
The baseline and candidate binary SHA-256 values are
`6bd04f742da17ac725b052af40b34f9f1110658f01ba514ec44319b6904eb3b3`
and `732a53c419def118f661aa11cf9425c3aa1481dfe68069786d36fd23c8bf1f22`;
the exact TSV SHA-256 is
`e37e4cc1238042611ad8ba9b6da6e5dcbb2d25cd76275466451e2f63facf649d`.
Deferred/dynamic by-reference argument errors, destructuring's writable-value
diagnostic, nested temporary by-reference foreach sources, false-to-array
conversion and string append diagnostics remain separate surfaces.

The latest measured AMD64 PHP 8.5 contract checkpoint is pinned to php-src
8.5.6 commit `fcc29c8` and candidate commit `65008ae6`. Across all 5,599
unmodified `Zend/tests` and `tests/lang` cases, 3,987 pass, 1,312 fail, 115
skip, none remain XFAIL, 185 are unsupported, and none time out or crash. The
headline pass rate is 75.241% and the whole-corpus rate is 71.209%; 4,782 of
5,299 attempted cases reach runtime (90.243%). Relative to integration
baseline `ba435240`, whose executable state is candidate `bf9a63eb`, the exact
pass-set delta is +3/-0.

The additions are `Zend/tests/builtin_in_write_context_error1.phpt`,
`builtin_in_write_context_error2.phpt` and
`Zend/tests/coalesce/assign_coalesce_009.phpt`. The first and third move from
runtime failure to exact pass, and the reference case moves from output
failure to exact pass. Every previous pass remains a pass and no other status
or failure category moves. Two sequential candidate runs have byte-identical
merged manifests and summaries. Their manifest and summary SHA-256 values are
`58ec2db8100692f8670561a0e25f77d4d715c7ce7a9671572c8ce7c849549823`
and `45c4b516504aaca76958d80193b648aebadf131ddfa34ba09ffa7051af7e80b8`.

PHP 8.5 compiler-special global built-in call shapes now produce the
compile-time `Cannot use result of built-in function in write context` fatal
when their result is used as an indexed or appended array root, coalescing or
compound assignment root, increment/decrement or `unset()` root, direct
reference target, reference to an indexed special result, or by-reference
`foreach` source. The bounded
metadata covers the PHP-special arities and constant forms for scalar/type
built-ins, `count()`/`sizeof()`, class and argument introspection,
`array_key_exists()`, `defined()`, strict literal `in_array()` and the
`array_slice(func_get_args(), literal)` form. It remains deliberately separate
from RPHP's runtime direct-call lowering.

Only lexically unambiguous global calls, fully qualified calls and imported
global aliases receive the diagnostic. Namespaced user shadows, ordinary
internal and user calls, named or unpacked forms, non-special arities and
dynamic constant/array forms retain their writable discarded-temporary or
returned-reference behavior. One original CLI suite exercises all admitted
families, write forms, namespace boundaries and negative controls. All five
Cargo configurations, all-features/all-targets, formatting, PHPT runner
self-test, unsafe self-test and the exact unsafe ratchet pass. The production
inventory remains 1,612 unsafe blocks, 289 unsafe functions and 321 SAFETY
annotations. Composer 2.8.12 S0, all four Symfony S1 gates, and exact PHP 8.5.9
warmed-kernel S2 and cold-build S3 also pass.

A local CPU-pinned 32-pair balanced alternating AMD64 release comparison with
the performance governor, four warmups and no excluded sample keeps both
front-end controls below the +5% gate. One hundred empty cold requests per
observation measure candidate/baseline ratios of 0.998608 by independent
medians and 0.998261 by paired medians. One request compiling and executing
1,000 source units containing an ordinary user-call indexed write and internal
call append measures 1.008287 and 1.009096 respectively, with exact output.
The baseline and candidate binary SHA-256 values are
`f72a40d9a2b7d717e2ff6879fb05eb8356ac25965ced12fa8bc9452f35b214db`
and `6bd04f742da17ac725b052af40b34f9f1110658f01ba514ec44319b6904eb3b3`;
the exact TSV SHA-256 is
`542241df3473c0e55f084306a558ae6cdd39c27fbee6f19578c6ace4127cb82f`.
Temporary non-call expression diagnostics, automatic false-to-array
conversion, string append diagnostics and clone-result diagnostic spelling
remain separate surfaces.

The latest measured AMD64 PHP 8.5 contract checkpoint is pinned to php-src
8.5.6 commit `fcc29c8` and candidate commit `bf9a63eb`. Across all 5,599
unmodified `Zend/tests` and `tests/lang` cases, 3,984 pass, 1,315 fail, 115
skip, none remain XFAIL, 185 are unsupported, and none time out or crash. The
headline pass rate is 75.184% and the whole-corpus rate is 71.156%; 4,782 of
5,299 attempted cases reach runtime (90.243%). Relative to exact candidate
`9ddfc055`, the pass-set delta is +6/-0.

The exact additions are `Zend/tests/backtrace/bug79108.phpt`,
`Zend/tests/dereference/dereference_006.phpt`, `dereference_007.phpt`,
`dereference_012.phpt`, `dereference_013.phpt` and
`Zend/tests/modify_isref_value_return.phpt`. Every previous pass remains a
pass. `Zend/tests/bug52614.phpt` advances from compile to output failure;
`assert/gh11580.phpt`, `bug70089.phpt`,
`builtin_in_write_context_error1.phpt`, `coalesce/assign_coalesce_009.phpt`,
`oss_fuzz_60441.phpt` and `temporary_cleaning/temporary_cleaning_013.phpt`
advance from compile to runtime failure; and
`undefined_multidimensional_array.phpt` advances from parse to output failure.
All remain visible. Two sequential final runs have byte-identical merged
manifests and summaries. Their manifest and summary SHA-256 values are
`24b541422ab343589269d21fba6b44de7250aa84c1d5afd1d2ba881dfd90dce5` and
`714b42f95131ef5507d16599285f371936a18201ce63410f88b06967a451efbe`.

PHP function, method, static and dynamic call results can now be array-write
roots. A by-reference result retains its caller-visible alias, while a by-value
result is mutated only as a discarded temporary. Indexed writes use the same
rule, including internal calls that return arrays. Static-property expressions
returned from `function &` use the existing reference fetch instead of
triggering the non-variable-reference notice.

Nested anonymous dimensions reuse the existing internal append-reference cell:
each new null element is published to its parent before the following write or
property error. This append-only resolver deliberately does not widen the
stricter by-reference `foreach` contract. Array literals and nullsafe chains
retain the exercised PHP compile-fatal boundaries, while other non-call
temporaries remain rejected. One original CLI suite covers call kinds,
by-value/by-reference identity, Reflection by-value reads, evaluation order,
nested append publication and both negative diagnostics. All five Cargo
configurations,
all-features/all-targets, formatting, PHPT runner self-test, unsafe self-test
and the exact unsafe ratchet pass. The production inventory remains 1,612
unsafe blocks, 289 unsafe functions and 321 SAFETY annotations. Composer
2.8.12 S0, all four Symfony S1 gates, and exact PHP 8.5.9 warmed-kernel S2 and
cold-build S3 also pass.

A local 32-pair balanced alternating AMD Ryzen 9 7950X release comparison,
pinned to CPU 0 with the performance governor, four warmups and no excluded
sample, keeps both front-end controls below the +5% gate. One hundred empty
cold requests per observation measure candidate/baseline ratios of 0.968987 by
independent medians and 0.986140 by paired medians. One request independently
generating, compiling and executing 1,000 source units that combine a
user-call indexed write with an ordinary array append measures 0.997520 and
0.997903 respectively, with exact output. The candidate binary SHA-256 is
`f72a40d9a2b7d717e2ff6879fb05eb8356ac25965ced12fa8bc9452f35b214db` and the
exact TSV SHA-256 is
`9a66b18a6e9056818742654912a9928933512e4375331573faac28fb48edf0cd`.

This checkpoint claims the exercised call-result and nested anonymous-dimension
array writes. Specialized compile diagnostics for scalar built-in results such
as `strlen()`, automatic false-to-array conversion, string append diagnostics
and clone-result diagnostic spelling, plus silent warning behavior for nested
property writes, remain separate surfaces.

The latest measured AMD64 PHP 8.5 contract checkpoint is pinned to php-src
8.5.6 commit `fcc29c8` and candidate commit `a47f14fc`. Across all 5,599
unmodified `Zend/tests` and `tests/lang` cases, 3,959 pass, 1,340 fail, 115
skip, none remain XFAIL, 185 are unsupported, and none time out or crash. The
headline pass rate is 74.712% and the whole-corpus rate is 70.709%; 4,750 of
5,299 attempted cases reach runtime (89.640%). Relative to exact candidate
`b7b97752`, the pass-set delta is +2/-0 with no other status or failure-
category movement.

The exact additions are `Zend/tests/break_error_001.phpt` and
`break_error_002.phpt`. Every previous pass remains a pass. Together with the
preceding checkpoint, all four `break_error_001` through `break_error_004`
cases now pass. Two sequential final runs have byte-identical merged manifests
and summaries. Their manifest and summary SHA-256 values are
`b6000893425bc1342d688b0e031c83d33ed7f46224124681b1c8b7ae1ade4b2f` and
`dc202f085528f93b58c269ad639f016cb436159a2ab89d8bbc9f3c7e0d196e2a`.

The parser now consumes PHP's historical expression grammar after `break` and
`continue`, while admitting only a positive integer literal, optionally
parenthesized, as the compiled level. Zero and non-integer scalar literals use
PHP 8.5's positive-integer fatal; variables, unary and compound expressions
use its removed non-integer-operand fatal. Both are deferred source-unit
compile errors rather than parser errors, so dead code, functions and ordinary
source locations retain the required error stage and file/line.

The lexer preserves a leading minus only at this operand boundary, including
through nested parentheses. Negative integers, floats and signed zero can
therefore remain distinct from unsigned zero without changing the existing
compact negative-number tokens elsewhere. Positive explicit levels still
lower through the unchanged compiler and VM control-flow path.

The original CLI regression now covers both operators, zero, scalar strings,
negative integers, signed integer and float zero, a variable, a binary
expression, nested positive parentheses, exact diagnostics, exit status and
an unaffected valid multi-level jump. All five Cargo configurations,
all-features/all-targets, formatting, PHPT runner self-test, unsafe self-test
and the exact unsafe ratchet pass. The production inventory remains 1,612
unsafe blocks, 289 unsafe functions and 321 SAFETY annotations. Composer
2.8.12 S0, all four Symfony S1 gates, and exact PHP 8.5.9 warmed-kernel S2 and
cold-build S3 also pass.

A local 32-pair balanced alternating AMD Ryzen 9 7950X release comparison,
pinned to CPU 0 with the performance governor, four warmups, 100 cold requests
per observation and no excluded sample, keeps both parser controls below the
+5% gate. Empty requests measure candidate/baseline ratios of 0.987876 by
independent medians and 1.009541 by paired medians. A valid explicit
multi-level `break` measures 1.004006 and 0.965585 respectively, with exact
output. The candidate binary SHA-256 is
`4effee007d16cfa128225d4b5e2354489980242651abc5ab3f4c4896ed63148c` and the
exact TSV SHA-256 is
`3536b730825029e99fa7ca807ca98a3fa88cf072b5e0f233926337c3869eeb6a`.

This checkpoint claims the exercised optional operand grammar, operand-kind
diagnostics and source locations. Positive nesting levels above 4,294,967,295,
unrelated malformed statement syntax and other control-flow operators remain
separate surfaces.

The latest measured AMD64 PHP 8.5 contract checkpoint is pinned to php-src
8.5.6 commit `fcc29c8` and candidate commit `b7b97752`. Across all 5,599
unmodified `Zend/tests` and `tests/lang` cases, 3,957 pass, 1,342 fail, 115
skip, none remain XFAIL, 185 are unsupported, and none time out or crash. The
headline pass rate is 74.674% and the whole-corpus rate is 70.673%; 4,748 of
5,299 attempted cases reach runtime (89.602%). Relative to exact candidate
`2945750f`, the pass-set delta is +5/-0 with no other status or failure-
category movement.

The exact additions are `Zend/tests/break_error_003.phpt`,
`break_error_004.phpt`, `Zend/tests/bug77660.phpt`, `gh13931.phpt` and
`Zend/tests/try/try_finally_011.phpt`. Every previous pass remains a pass. Two
sequential final runs have byte-identical merged manifests and summaries.
Their manifest and summary SHA-256 values are
`7c98995002131faaea9b84a5213391838d02b963545b6e2a816eaede6a2591b6` and
`0d31a148cdd586284b79010beb6aa40e3649364faeed2fa44ac2bc621543051d`.

`break` and `continue` tokens now retain their source line through the AST.
When compilation finds no loop/switch context at level one, it emits PHP
8.5's context diagnostic; a request beyond the available nesting instead
names the requested level. Both forms use the ordinary source-aware compiler
fatal path, including inside functions, `try`/`finally` and `eval`. Valid
single- and multi-level control flow lowers to the same jumps and VM bytecode
as before.

One original CLI regression covers implicit and explicit level one, excessive
`break` and `continue` depths, the AMD64-sized 2,147,483,648 boundary, exact
file/line diagnostics, exit status and an unaffected valid multi-level jump.
All five Cargo configurations, all-features/all-targets, formatting, PHPT
runner self-test, unsafe self-test and the exact unsafe ratchet pass. The
production inventory remains 1,612 unsafe blocks, 289 unsafe functions and 321
SAFETY annotations. Composer 2.8.12 S0, all four Symfony S1 gates, and exact
PHP 8.5.9 warmed-kernel S2 and cold-build S3 also pass.

A local 32-pair balanced alternating AMD Ryzen 9 7950X release comparison,
pinned to CPU 0 with the performance governor, four warmups, 100 cold requests
per observation and no excluded sample, keeps both front-end controls below
the +5% gate. Empty requests measure candidate/baseline ratios of 0.989596 by
independent medians and 0.982716 by paired medians. A valid nested loop with a
multi-level `break` measures 1.005497 and 1.020989 respectively, with exact
output. The candidate binary SHA-256 is
`391439baa97f0c7df65324f6a1945ae568063d5e73db596aa4c67d749ec7345a` and the
exact TSV SHA-256 is
`326e11d6bc5e76b70f0595e9947de35704d4f24b927562545ac29d2a2700c3e6`.

This checkpoint claims only the exercised compile-time loop-control context,
depth and source-location contract. Non-positive and non-literal operand
grammar and diagnostics, unrelated control-flow diagnostics and general
compile-error trace presentation remain separate surfaces.

The latest measured AMD64 PHP 8.5 contract checkpoint is pinned to php-src
8.5.6 commit `fcc29c8` and candidate commit `2945750f`. Across all 5,599
unmodified `Zend/tests` and `tests/lang` cases, 3,952 pass, 1,347 fail, 115
skip, none remain XFAIL, 185 are unsupported, and none time out or crash. The
headline pass rate is 74.580% and the whole-corpus rate is 70.584%; 4,748 of
5,299 attempted cases reach runtime (89.602%). Relative to exact candidate
`a97e06ee`, the pass-set delta is +1/-0 with no other status or failure-
category movement.

The exact addition is `Zend/tests/bug78396.phpt`. Every previous pass remains
a pass. Two sequential final runs have byte-identical merged manifests and
summaries. Their manifest and summary SHA-256 values are
`383675b6bb942ca10f43281c4e93dc3c0f505763a056206093a37aa19f6cf33a` and
`6f356d1a9dbe9446d0175152c80d1ded3249b54c6bf19aa22fa4c53cec957321`.

The default build now publishes `FILE_APPEND=8` and accepts the optional flags
argument of `file_put_contents()`. Regular-file append opens with atomic append
semantics, while `LOCK_EX` acquires an operating-system exclusive lock for the
complete write. Replacement under that lock opens without truncating, locks
first, and only then truncates and writes. Repeated `FILE_APPEND | LOCK_EX`
writes from a shutdown callback therefore complete in order and release each
lock when its file handle closes.

This bounded path extends the existing small default filesystem fallback; it
does not enable or duplicate the full opt-in `file-write` handler. The ordinary
two-argument overwrite continues through `std::fs::write`, and builds with
`file-write` retain their stream-backed four-argument implementation. One
original E2E regression checks both public flag values, shutdown completion and
the exact appended bytes. Default, no-default, explicit `file-write` and all-
feature filesystem suites pass.

All five Cargo configurations, all-features/all-targets, formatting, PHPT
runner self-test and the exact unsafe ratchet pass. The production inventory
remains 1,612 unsafe blocks, 289 unsafe functions and 321 SAFETY annotations.
Composer 2.8.12 S0, all four Symfony S1 gates, and exact PHP 8.5.9 warmed-
kernel S2 and cold-build S3 also pass.

A local 32-pair balanced alternating AMD Ryzen 9 7950X release comparison,
pinned to CPU 0 with the performance governor, four warmups, 100 cold requests
per observation and no excluded sample, keeps both relevant controls below the
+5% gate. Empty requests measure candidate/baseline ratios of 0.983392 by
independent medians and 0.983178 by paired medians. Five ordinary two-argument
overwrite writes per request measure 0.978608 and 0.979724 respectively, with
exact output. The candidate binary SHA-256 is
`24f1afe0e670e722059bfa5e4207a1cc6f3fd677c62ff4ccda7a4d6c0c3de4be` and the
exact TSV SHA-256 is
`f9843f82eaf258a3f46a79ee0005130d45d8a09ab64e1984846b5bb499991a9e`.

This checkpoint claims only default-build string writes to ordinary files with
the exercised append and exclusive-lock flags. Array or stream payloads,
stream contexts, include-path resolution, wrapper-specific flag behavior and
the broader opt-in `file-write` contract remain separate surfaces.

The latest measured AMD64 PHP 8.5 contract checkpoint is pinned to php-src
8.5.6 commit `fcc29c8` and candidate commit `a97e06ee`. Across all 5,599
unmodified `Zend/tests` and `tests/lang` cases, 3,951 pass, 1,348 fail, 115
skip, none remain XFAIL, 185 are unsupported, and none time out or crash. The
headline pass rate is 74.561% and the whole-corpus rate is 70.566%; 4,748 of
5,299 attempted cases reach runtime (89.602%). Relative to exact candidate
`73364c32`, the pass-set delta is +2/-0 with no other status or failure-
category movement.

The exact additions are `Zend/tests/bug51827.phpt` and
`Zend/tests/bug71221.phpt`. Every previous pass remains a pass. Two sequential
final runs have byte-identical merged manifests and summaries. Their manifest
and summary SHA-256 values are
`f8eead8e4836e094789c85ff088781647f2e8d03db133b25bfd2b75717420878` and
`7e6b7d0a9bc387ded6777371f2ed5f6fb8b20540436049288a18b526aee2f90d`.

An engine-invoked shutdown callback that fails before entering its body now
retains PHP's inactive call origin. Required-arity failures such as an empty
`explode()` callback and dynamically forbidden scope-introspection callbacks
such as `get_defined_vars()` expose `[no active file]:0`, an immutable
`#0 [internal function]` callback frame and the terminating `{main}` frame.
The same metadata is visible to an installed exception handler before fatal
rendering.

The detached callback boundary reuses its existing pending-call frame and
Throwable trace snapshot instead of synthesizing a diagnostic string. A cold,
explicit flag enables pre-entry capture only for request-shutdown dispatch;
ordinary direct and dynamic calls, error handlers, exception handlers,
Reflection and successful shutdown callbacks retain their existing entry
semantics. The inactive origin remains distinct from the `Unknown:0` trace
sentinel so the stored Throwable renders `[no active file]:0` while its frame
continues to render as `[internal function]`.

One original E2E regression covers both pre-entry classes with exact stderr,
exit status, origin and trace. The adjacent callable, Reflection, standard-
library, error-handler, exception-presentation and request-shutdown suites
pass. All five Cargo configurations, all-features/all-targets, formatting,
PHPT runner self-test and the exact unsafe ratchet pass. The production
inventory remains 1,612 unsafe blocks, 289 unsafe functions and 321 SAFETY
annotations. Composer 2.8.12 S0, all four Symfony S1 gates, and exact PHP 8.5.9
warmed-kernel S2 and cold-build S3 also pass.

A local 32-pair balanced alternating AMD Ryzen 9 7950X release comparison,
pinned to CPU 0 with the performance governor, four warmups, 100 cold requests
per observation and no excluded sample, keeps both relevant controls below the
+5% gate. Empty requests measure candidate/baseline ratios of 1.008708 by
independent medians and 1.009192 by paired medians; one successful shutdown
callback measures 1.007522 and 1.007585 respectively. Outputs are exact. The
candidate binary SHA-256 is
`ec977a2dc64467499d3ba2ea3a7c012cf1f6d2e35c385c84ae8f1dbf53ae4475` and the
exact TSV SHA-256 is
`7fb9aa4a78340ae0e6bdb8fc35a831eecc090b2c24f329ea43e561d864245cba`.

This checkpoint claims only pre-entry Throwable metadata for the exercised
CLI request-shutdown callbacks. Direct-call trace completeness, other engine-
callback kinds, SAPI-specific inactive origins and unrelated shutdown failures
remain separate work.

The latest measured AMD64 PHP 8.5 contract checkpoint is pinned to php-src
8.5.6 commit `fcc29c8` and candidate commit `73364c32`. Across all 5,599
unmodified `Zend/tests` and `tests/lang` cases, 3,949 pass, 1,350 fail, 115
skip, none remain XFAIL, 185 are unsupported, and none time out or crash. The
headline pass rate is 74.523% and the whole-corpus rate is 70.530%; 4,748 of
5,299 attempted cases reach runtime (89.602%). Relative to exact candidate
`a0e309b0`, the pass-set delta is +5/-0 with no other status or failure-
category movement.

The exact additions are `Zend/tests/bug32322.phpt`, `bug62763.phpt`,
`Zend/tests/fibers/destructors_008.phpt`,
`Zend/tests/magic_methods/bug51822.phpt` and `bug74053.phpt`. Every previous
pass remains a pass. Two sequential final runs have byte-identical merged
manifests and summaries. Their manifest and summary SHA-256 values are
`444da2bb3bc0fbd638341772606d0f0029649ea3bd4be31d50aad20bad4454fd`
and `db9f3146da0201cc084bf213d04f0f5c2afdb514ac41648d06b55c639659fbad`.

Request shutdown now retains resolved callback receivers, closure captures and
arguments through the complete FIFO callback phase. Successful completion
releases those owners in registration order after every callback has run. An
`exit` or unhandled callback failure still stops the remaining callbacks, but
owners retained by both executed and pending callbacks receive their required
destructors before the original status or failure leaves the request.

After the callback and main-scope phases, class-static values and then named-
function-static values enter a fixed-point destructor pass. Canonical static
cells remain readable while their destructor runs, and an object published
into either storage family by another shutdown destructor is observed on the
next pass. A shutdown destructor exception reaches the active exception
handler, and an unhandled engine-invoked destructor appears through PHP's
`[internal function]` trace boundary. Objects shared by main and static storage
are destructed once.

The fixed-point planner reuses the existing iterative destructor tree and
grouped reference-count checks. A request-local boolean records only runtime
static storage that may retain an object; ordinary requests therefore skip the
cold storage snapshots and allocations. `Value`, `PhpObject` and
`ExecuteData` layouts are unchanged. The final root-frame teardown lives in a
non-inlined cold helper so the new shutdown code does not perturb the ordinary
executor entry layout.

Eight original regressions cover successful, exiting and throwing callback
queues; main/class/function storage order; static-cell visibility; cross-
storage fixed-point publication; handled and unhandled destructor exceptions;
the internal trace boundary; and a main/static shared owner. The adjacent
constructor, frame-cleanup, magic-method, weak-object, cycle-collection and
Fiber suites pass. All five Cargo configurations, all-features/all-targets,
formatting, PHPT runner self-test and the exact unsafe ratchet pass. The
production inventory remains 1,612 unsafe blocks, 289 unsafe functions and
321 SAFETY annotations. Composer S0, all four Symfony S1 gates, and exact PHP
8.5.9 warmed-kernel S2 and cold-build S3 also pass.

A local 32-pair balanced alternating AMD Ryzen 9 7950X release comparison,
pinned to CPU 0 with the performance governor, four warmups, 100 cold requests
per observation and no excluded sample, keeps all controls below the +5% gate.
The order-balanced candidate/baseline median ratios are 0.999570 for an empty
request, 1.002151 for one owner-free shutdown callback, 1.007585 for a callback
owner with an empty destructor and 1.009815 for a class-static object with an
empty destructor. The corresponding independent median ratios are 0.999219,
1.001302, 1.006802 and 1.009673. The exact candidate binary SHA-256 is
`dc9eee89587b680d9c20771b1bf291fa25aeee7fb7b4ce55a1d8ee6a4dda4d53`.

General cycle reclamation, forced Fiber/generator close combinations, ordering
within multiple unrelated named-function-static maps, and SAPI-specific
shutdown remain separate work. This checkpoint claims the exercised CLI
request-shutdown callback ownership, storage tiers, fixed point, exception and
trace contracts only.

The latest measured AMD64 PHP 8.5 contract checkpoint is pinned to php-src
8.5.6 commit `fcc29c8` and candidate commit `a0e309b0`. Across all 5,599
unmodified `Zend/tests` and `tests/lang` cases, 3,944 pass, 1,355 fail, 115
skip, none remain XFAIL, 185 are unsupported, and none time out or crash. The
headline pass rate is 74.429% and the whole-corpus rate is 70.441%; 4,748 of
5,299 attempted cases reach runtime (89.602%). Relative to exact candidate
`6009daff`, the pass-set delta is +8/-0 with no other status or failure-
category movement.

The exact additions are `Zend/tests/bug49893.phpt`,
`Zend/tests/exceptions/bug73338.phpt`,
`Zend/tests/magic_methods/bug29368.phpt`, `bug29368_3.phpt`,
`Zend/tests/try/bug73337.phpt`, `catch_002.phpt`, `catch_003.phpt` and
`catch_004.phpt`. Every previous pass remains a pass. Two sequential final
runs have byte-identical merged manifests and summaries. Their manifest and
summary SHA-256 values are
`f5626bf6b5d994cadc104ec32dc211ac3ffdc4419ca30a70508cd52c6cef5444`
and `d9c477f9c20f114fcbfa9fb8b74b6cced72dee42a1e2e570811a886313b26483`.

A fresh object whose resolved class has both a constructor and destructor now
starts with its own destructor ineligible. Only successful completion of the
exact constructor activation entered by `new` enables it, after frame-local
destructor cleanup has also completed without an exception. Constructor
argument validation, a thrown constructor, a destructor exception at the
constructor return boundary, an escaped `$this`, and a later manual successful
`->__construct()` call therefore cannot make a failed allocation eligible.
Constructorless allocation, successful property-initializer and unpacked
constructor paths, and Reflection construction without invoking the
constructor preserve their PHP 8.5 behavior.

The original-construction marker shares the existing per-frame call-kind byte,
so `ExecuteData` remains four value slots and Fiber suspension needs no sparse
sidecar. The object lifecycle marker suppresses only the failed owner's own
destructor; destructor-bearing properties still retire normally. When a local
handler abandons an interrupted expression, the baseline VM now releases that
statement's bounded TMP/VAR range before matching a catch. A throwing
temporary destructor replaces the pending constructor exception, chains it as
`previous`, and can select a different catch. Ranges with no remaining VM
release work take an allocation-free cleanup path.

Seven original constructor regressions cover escaped and retried `$this`,
completed and failed argument temporaries, constructor-frame cleanup failure,
successful/no-constructor/Reflection boundaries, property-tree release,
replacement-exception catch reselection, argument validation, inheritance and
unpacking. The adjacent constructor, exception, frame-cleanup, magic-method,
type, weak-object, cycle-collection, Fiber and hot-tier suites pass. All five
Cargo configurations, all-features/all-targets, formatting, PHPT runner
self-test and the exact unsafe ratchet pass. The production inventory is 1,612
unsafe blocks, 289 unsafe functions and 321 SAFETY annotations. Composer S0,
all four Symfony S1 gates and exact PHP 8.5.9 warmed-kernel S2 and cold-build
S3 also pass.

A local 32-pair balanced alternating AMD Ryzen 9 7950X release comparison,
pinned to CPU 0 with the performance governor, four warmups and no excluded
sample, keeps every affected and unaffected control below the +5% gate. The
order-balanced candidate/baseline median ratios are 0.981968 for the ordinary
constructor control, 0.986547 for successful constructors with an empty
destructor, 0.997011 for caught ordinary calls and 1.004452 for failed
constructors with an empty destructor. The corresponding independent median
ratios are 0.987017, 0.989664, 0.996239 and 1.003543. The exact candidate
binary SHA-256 is
`29547a62c0ab592fb73abda7e466ba2f6463f479355e65e44587843e5e909d1b`.

Request-shutdown fixed-point ordering, broader cycle-collector and Fiber
lifetime behavior, and the remaining object-lifecycle corpus stay separate
work. This checkpoint claims constructor-failure destructor eligibility and
the exercised interrupted-statement temporary cleanup only.

The latest measured AMD64 PHP 8.5 contract checkpoint is pinned to php-src
8.5.6 commit `fcc29c8` and candidate commit `6009daff`. Across all 5,599
unmodified `Zend/tests` and `tests/lang` cases, 3,936 pass, 1,363 fail, 115
skip, none remain XFAIL, 185 are unsupported, and none time out or crash. The
headline pass rate is 74.278% and the whole-corpus rate is 70.298%; 4,748 of
5,299 attempted cases reach runtime (89.602%). Relative to exact candidate
`82d5a4ea`, the pass-set delta is +3/-0 with no other status or failure-
category movement.

The exact additions are `Zend/tests/magic_methods/bug52361.phpt`,
`Zend/tests/oss_fuzz_456317305.phpt` and
`Zend/tests/weakrefs/weakmap_dtor_exception.phpt`. Every previous pass remains
a pass. Two sequential final runs have byte-identical merged manifests and
summaries. Their manifest and summary SHA-256 values are
`e904908786e466c6ee3a269d806c6b1d22fc29e2b976e8985848abec9392d677`
and `77602ab435ffdde063a21d663dae491e796ac734a3b272d6c6e88a0941cfb3b7`.

An exception that leaves a user activation now releases final objects owned by
that frame before selecting a caller handler. The release planner preserves
objects with another owner, follows nested object, array, reference and closure
containers without looping on cycles, and invokes `__destruct()` with the
retiring frame's caller as its logical trace site. A throwing destructor
replaces the pending exception, chains the displaced throwable through
`previous`, and can therefore reselect a different caller catch; later
throwing destructors repeat that rule. Re-entry after a completed `finally`
uses the same dispatch path, so it no longer bypasses frame lifetime work.

VM bailouts remain outside PHP catch dispatch. The shared throw boundary now
returns `Result`, and every cold caller propagates it, so `exit(7)` from a
destructor after `finally` bypasses `catch`, preserves output ordering and
returns status 7 exactly like PHP 8.5. Correct propagation exposed an older
S2-masked gap: `ArrayIterator` and `ArrayObject` declared `Countable` without
their `count()` method. Both now report the number of values in their internal
array, including inherited method dispatch, which restores the unmodified
Symfony environment-loader path.

Original regressions cover destructor order, retained owners, nested
containers, replacement-exception chains and catch reselection, plus the
non-catchable `exit()` status and the two SPL count methods. All 228 selected
PHPTs containing `__destruct` reach 124 passes with exactly the three gains
above. All five Cargo configurations, all-features/all-targets, formatting,
PHPT runner self-test and the exact unsafe ratchet pass. The production
inventory is 1,605 unsafe blocks, 289 unsafe functions and 320 SAFETY
annotations. Composer S0, all four Symfony S1 gates and exact PHP 8.5.9
warmed-kernel S2 and cold-build S3 also pass.

A local 21-pair balanced alternating release comparison, with three warmups,
exact-output validation and no excluded sample, keeps the unaffected 200,000-
exception control at a 1.014541 independent median ratio and the ordinary
500,000-frame destructor control at 1.013195, below the +5% gate. The
intentionally affected 100,000-exception path with an empty local destructor
moves from a 0.181232-second baseline median to 0.253415 seconds, a 1.398285
ratio. This is localized compatibility-first performance debt: the baseline
omitted the destructor that PHP requires. The exact candidate binary SHA-256
is `fd69d89d77389fae3efb6aac2e75fb59ed35af44e403da9d16530a1175d64d7d`.

Constructor-failure destructor eligibility, request-shutdown fixed-point
ordering, broader cycle-collector behavior and the rest of the SPL iterator
API remain separate work. This checkpoint claims ordinary exception-frame
unwind and only the exercised `count()` contract.

The latest measured AMD64 PHP 8.5 contract checkpoint is pinned to php-src
8.5.6 commit `fcc29c8` and candidate commit `82d5a4ea`. Across all 5,599
unmodified `Zend/tests` and `tests/lang` cases, 3,933 pass, 1,366 fail, 115
skip, none remain XFAIL, 185 are unsupported, and none time out or crash. The
headline pass rate is 74.222% and the whole-corpus rate is 70.245%; 4,748 of
5,299 attempted cases reach runtime (89.602%). Relative to exact candidate
`1cb5d3b0`, the pass-set delta is +6/-0 with no other status or failure-
category movement.

The exact additions are
`Zend/tests/magic_methods/magic_methods_inheritance_rules_non_trivial_01.phpt`,
`magic_methods_inheritance_rules_non_trivial_02.phpt`,
`Zend/tests/type_declarations/intersection_types/variance/invalid5.phpt`,
`Zend/tests/type_declarations/iterable/iterable_004.phpt`,
`iterable_005.phpt` and
`Zend/tests/type_declarations/mixed/inheritance/mixed_parameter_inheritance_error3.phpt`.
Every previous pass remains a pass. Two sequential final runs have byte-
identical merged manifests and summaries. Their manifest and summary SHA-256
values are
`850f2f25c6b2b3324986dcce5e2b7d7b19e0776540326706cdb55da80189f4a4`
and `3ab2f7c7cdecb947244a48d4c5df78d988bba4a035f17621f33633ce03485cff`.

Method-compatibility fatals now render both parameter and return declarations
through PHP 8.5's canonical diagnostic type spelling after resolving lexical
`self`, `parent` and late-static types. Class, intersection, callable and
late-static members remain ahead of the fixed scalar/container sequence;
members are then ordered as `object`, `array`, `string`, `int`, `float`,
`bool` and `null`. `iterable` expands to
`Traversable|array`, nullable iterable adds `null`, and intersections retain
the parentheses required inside DNF unions. The underlying arity, reference,
visibility, parameter-contravariance and return-covariance decisions are
unchanged.

An original E2E regression covers source-order-independent parameter and
return unions, complete built-in ordering, iterable and nullable-iterable
expansion, DNF parentheses and an existing variadic-union expectation corrected
against exact PHP 8.5.9. All 131 upstream cases containing the relevant
method-compatibility diagnostic reach 109 passes, with the remaining failures
unchanged. The 555 related inheritance, interface, autoload, property-hook,
type-hint, magic and Reflection E2Es pass. All five Cargo configurations,
all-features/all-targets, formatting, PHPT runner self-test, unsafe self-test
and the exact unsafe ratchet pass. The production inventory remains 1,613
unsafe blocks, 289 unsafe functions and 312 SAFETY annotations. Composer S0,
all four Symfony S1 gates and exact PHP 8.5.9 warmed-kernel S2 and cold-build
S3 also pass. The change adds no opcode, runtime layout field, dependency or
unsafe block and executes only after a method incompatibility is established.

A local 21-pair alternating release control on an AMD Ryzen 9 7950X pinned to
CPU 0 with the performance governor, exact-output validation and two
additional warmups compiled 2,000 valid parent/child pairs, 4,000 unique
classes and 4,000 compatible union-typed method declarations per observation
through `eval()`. The valid path never invokes the changed fatal renderer. No
sample was excluded and every run returned checksum `2000`. Baseline
median/mean was 1.316843/1.318944 seconds and candidate median/mean was
1.312740/1.314178 seconds. The independent median and mean ratios are 0.996884
and 0.996387; paired median and mean ratios are 0.995520 and 0.996494, below
the +5% gate. The exact candidate binary SHA-256 is
`850eadbae667c63043c6da8e1356a2c8cc14f0a2ebf573c88999fcf1395ee5c5`.

General method-variance semantics, Reflection type rendering and independent
property/class-constant diagnostics remain separate work. The remaining
method-compatibility PHPT failures retain their existing parser, runtime or
unrelated output boundaries.

The latest measured AMD64 PHP 8.5 contract checkpoint is pinned to php-src
8.5.6 commit `fcc29c8` and candidate commit `1cb5d3b0`. Across all 5,599
unmodified `Zend/tests` and `tests/lang` cases, 3,927 pass, 1,372 fail, 115
skip, none remain XFAIL, 185 are unsupported, and none time out or crash. The
headline pass rate is 74.108% and the whole-corpus rate is 70.138%; 4,748 of
5,299 attempted cases reach runtime (89.602%). Relative to exact candidate
`50e34e4c`, the pass-set delta is +2/-0 with no other status or failure-
category movement.

The exact additions are
`Zend/tests/magic_methods/interface_with_tostring.phpt` and
`Zend/tests/magic_methods/stringable_automatic_implementation.phpt`. Every
previous pass remains a pass. Two sequential final runs have byte-identical
merged manifests and summaries. Their manifest and summary SHA-256 values are
`30724b695e068a6e086dac06b09dff8d9332f9dfb7f4a5fdf33fc8aab0b1e048`
and `a9b13b05a4b284252e0a0a253d6c78f84dafa95d5cab12a95ac96323c9ed701a`.

Classes and interfaces with an effective `__toString()` now project PHP 8.5's
implicit `Stringable` relation through `ReflectionClass::getInterfaceNames()`,
`ReflectionClass::getInterfaces()` and `class_implements()`. Reflection keeps
canonical interface spelling and PHP's parent-first, source-order traversal;
implicit, explicit, case-insensitive and aliased `Stringable` paths deduplicate
to the canonical name. Own, abstract, inherited, interface-required and nested
trait-provided methods participate. A trait declaration itself remains outside
the interface relation, including `is_a($trait, Stringable::class, true)`.
Source declaration metadata remains unchanged.

An original E2E regression covers direct and inherited classes, abstract
methods, parent and interface hierarchies, trait composition, explicit,
lowercase and aliased declarations, Reflection list/map parity,
`class_implements()` membership/counts, canonical spelling, deduplication and
the negative trait boundary. The complete 157-case
`Zend/tests/magic_methods` directory reaches 115 passes with two gains and no
regression; the adjacent anonymous-class `class_implements()` control remains
a pass. The 574 related Reflection, magic, autoload, class, operator, OOP and
type-hint E2Es pass. All five Cargo configurations, all-features/all-targets,
formatting, PHPT runner self-test, unsafe self-test and the exact unsafe
ratchet pass. The production inventory remains 1,613 unsafe blocks, 289 unsafe
functions and 312 SAFETY annotations. Composer S0, all four Symfony S1 gates
and exact PHP 8.5.9 warmed-kernel S2 and cold-build S3 also pass. The change
adds no opcode, runtime layout field, dependency or unsafe block.

A local 21-pair alternating release control on an AMD Ryzen 9 7950X pinned to
CPU 0 with the performance governor, exact-output validation and two
additional warmups ran 100,000 unaffected interface-enumeration cycles per
observation. Each cycle constructed a reflection, queried its interface name
list and reflection map, called `class_implements()` and checked `is_a()` over
a parent plus two-interface hierarchy. No sample was excluded and every run
returned checksum `700000`. Baseline median/mean was 0.422863/0.423829 seconds
and candidate median/mean was 0.429664/0.437936 seconds. The independent median
and mean ratios are 1.016084 and 1.033286; paired median and mean ratios are
1.028383 and 1.033789, below the +5% gate. The exact candidate binary SHA-256
is `5478267b9266153818e8c6250b9a4e2e546e2b609453fdf87c97e21ae35afb09`.

General inherited magic-method variance, runtime string-conversion behavior
and the pre-existing key order of multiple non-`Stringable` entries returned
by `class_implements()` remain separate work. This checkpoint claims exact
Reflection ordering and canonical `Stringable` membership.

The latest measured AMD64 PHP 8.5 contract checkpoint is pinned to php-src
8.5.6 commit `fcc29c8` and candidate commit `50e34e4c`. Across all 5,599
unmodified `Zend/tests` and `tests/lang` cases, 3,925 pass, 1,374 fail, 115
skip, none remain XFAIL, 185 are unsupported, and none time out or crash. The
headline pass rate is 74.071% and the whole-corpus rate is 70.102%; 4,748 of
5,299 attempted cases reach runtime (89.602%). Relative to exact candidate
`7e59186c`, the pass-set delta is +10/-0 with no other status or failure-
category movement.

The exact additions are `Zend/tests/magic_methods/magic_methods_011.phpt`
through `magic_methods_020.phpt`. Every previous pass remains a pass. Two
sequential final runs have byte-identical merged manifests and summaries. Their
manifest and summary SHA-256 values are
`73a3f3090b427e94b0e8ea6c44cce85c7b872fc7641f8ee0c23438b0ef5671b3`
and `07744dfead845f54a391ec0dfecfabba8f4e2547b67a93cb3cbdc2e6203c4370`.

The centralized magic-method signature validator now enforces PHP 8.5's
parameter type contracts after arity, canonical-reference and staticness
validation. Property names on `__get`, `__isset`, `__unset` and `__set`, plus
method names on `__call` and `__callStatic`, must accept `string`. Argument
lists on `__call` and `__callStatic`, and payloads on `__unserialize` and
`__set_state`, must accept `array`. Untyped and `mixed` parameters are valid;
nullable, union and `iterable` declarations are accepted where they remain
contravariant supertypes. A narrow scalar, object or class type and pure
`null` are rejected. PHP's unrestricted second `__set` parameter is preserved.

The rule applies consistently to classes, abstract methods, interfaces,
traits and admitted enum methods. Diagnostics name the one-based parameter and
its declared variable. Their observable priority is arity, reference shape,
staticness, a non-public visibility warning, parameter type from left to right,
then return type. This reuses compiled parameter hints and adds no runtime
opcode, bytecode/layout field, dependency or unsafe block.

An original source-aware E2E regression covers namespace-qualified classes,
traits, interfaces, abstract declarations, enums, both typed parameter
positions, serialization/state methods, pure `null`, diagnostic precedence,
and accepted untyped, `mixed`, nullable, union and `iterable` supertypes. The
complete 157-case `Zend/tests/magic_methods` directory reaches 113 passes with
ten gains and no regression. All five Cargo configurations,
all-features/all-targets, formatting, PHPT runner self-test, unsafe self-test
and the exact unsafe ratchet pass. The production inventory remains 1,613
unsafe blocks, 289 unsafe functions and 312 SAFETY annotations. Composer S0,
all four Symfony S1 gates and exact PHP 8.5.9 warmed-kernel S2 and cold-build
S3 also pass.

A local 21-pair alternating release control on an AMD Ryzen 9 7950X pinned to
CPU 0 with the performance governor, exact-output validation, two additional
warmups and JIT/quick loops disabled compiled 10,000 unique classes and 40,000
successful typed magic-method declarations per observation through `eval()`.
No sample was excluded and every run returned checksum `198890`. Baseline
median/mean was 2.359255/2.362190 seconds and candidate median/mean was
2.348112/2.349366 seconds. The independent median and mean ratios are 0.995277
and 0.994571; paired median and mean ratios are 0.994383 and 0.994667, below
the +5% gate. The exact candidate binary SHA-256 is
`fe022d2ca094ad8489959de3762aac87417c2e4fc8ccc57e781ea5441da31ba6`.

General inherited-method variance and runtime magic dispatch remain separate
work; this checkpoint claims declaration validation and diagnostic ordering
only.

The latest measured AMD64 PHP 8.5 contract checkpoint is pinned to php-src
8.5.6 commit `fcc29c8` and candidate commit `7e59186c`. Across all 5,599
unmodified `Zend/tests` and `tests/lang` cases, 3,915 pass, 1,384 fail, 115
skip, none remain XFAIL, 185 are unsupported, and none time out or crash. The
headline pass rate is 73.882% and the whole-corpus rate is 69.923%; 4,748 of
5,299 attempted cases reach runtime (89.602%). Relative to exact candidate
`ed43af85`, the pass-set delta is +18/-0 with no other status or failure-
category movement.

The exact additions are `Zend/tests/errmsg/errmsg_015.phpt`, `016.phpt`,
`017.phpt`, `019.phpt`, `032.phpt`, `033.phpt`, `034.phpt`,
`Zend/tests/magic_methods/bug70215.phpt`, `magic_methods_003.phpt`,
`005.phpt`, `006.phpt`, `007.phpt`, `010.phpt`,
`magic_methods_sleep.phpt`, `magic_methods_wakeup.phpt`,
`magic_methods_serialize.phpt`, `magic_methods_unserialize.phpt` and
`magic_methods_set_state.phpt`. Every previous pass remains a pass. Two
sequential final runs have byte-identical merged manifests and summaries. Their
manifest and summary SHA-256 values are
`ede638fde978d880e0410c175f23fa87430a82005163a70b14179d168848be72`
and `5a393cb28e4481799e3bf8a98544b3c715a35bc2a590e3ec7c5e4899349b5c8b`.

The centralized magic-method signature validator now enforces PHP 8.5's
declared arity and staticness across classes, abstract methods, interfaces,
traits and admitted enum methods. Zero-argument lifecycle, serialization,
string and debug methods reject fixed parameters; property/call/state methods
require their exact one or two fixed parameters. A trailing variadic parameter
is allowed after the canonical fixed parameters, including by reference, and a
sole variadic does not satisfy a required fixed position. `__callStatic` and
`__set_state` must be static; other recognized magic methods cannot be static.
Diagnostics follow PHP's observable priority: arity, canonical by-reference
parameters, staticness, return contract, then a non-public visibility warning.
The existing return-type rules now share the same normalized method dispatch.

An original source-aware E2E regression covers singular/plural and zero-arity
wording, optional and trailing-variadic forms, class/trait/interface/abstract/
enum declarations, constructor/`__invoke`/call/state staticness and compound
arity/reference/static/return/visibility precedence. The combined 210-case
`Zend/tests/magic_methods` plus `Zend/tests/errmsg` slice reaches 132 passes
with 18 gains and no regression. All five Cargo configurations,
all-features/all-targets, formatting, PHPT runner self-test, unsafe self-test
and the exact unsafe ratchet pass. The production inventory remains 1,613
unsafe blocks, 289 unsafe functions and 312 SAFETY annotations. Composer S0,
all four Symfony S1 gates and exact PHP 8.5.9 warmed-kernel S2 and cold-build
S3 also pass. The change adds no runtime opcode, bytecode/layout field,
dependency or unsafe block.

A local 21-pair alternating release control on an AMD Ryzen 9 7950X pinned to
CPU 0 with the performance governor, exact-output validation, two additional
warmups and JIT/quick loops disabled compiled 10,000 unique classes and 40,000
successful method declarations per observation through `eval()`. No sample
was excluded and every run returned checksum `198890`. Baseline median/mean
was 2.235682/2.232795 seconds and candidate median/mean was
2.243343/2.248204 seconds. The independent median and mean ratios are 1.003427
and 1.006901; paired median and mean ratios are 1.006594 and 1.006937, below
the +5% gate. The exact candidate binary SHA-256 is
`dcf041d2af502666db5d257fcb97ff0deb7095fcdd99971d875e89bef88daa78`.

Magic-method parameter type variance and runtime dispatch/alias semantics
remain separate work; this checkpoint claims declaration shape and diagnostic
ordering only.

The latest measured AMD64 PHP 8.5 contract checkpoint is pinned to php-src
8.5.6 commit `fcc29c8` and candidate commit `ed43af85`. Across all 5,599
unmodified `Zend/tests` and `tests/lang` cases, 3,897 pass, 1,402 fail, 115
skip, none remain XFAIL, 185 are unsupported, and none time out or crash. The
headline pass rate is 73.542% and the whole-corpus rate is 69.602%; 4,748 of
5,299 attempted cases reach runtime (89.602%). Relative to exact candidate
`e3e87f4f`, the pass-set delta is +7/-0: all seven
`Zend/tests/magic_methods/magic_by_ref_001.phpt` through
`magic_by_ref_007.phpt` cases become exact passes. Every previous pass remains
a pass, and there are no other status or failure-category transitions.

PHP 8.5's canonical `__get`, `__set`, `__isset`, `__unset`, `__call`,
`__callStatic`, `__unserialize` and `__set_state` signatures now reject any
by-reference parameter during compilation with the declaration's resolved
class and original method spelling. The rule applies consistently to classes,
abstract methods, interfaces, traits and the magic methods admitted on enums;
it precedes magic return-type validation. It is restricted to the canonical
non-variadic parameter count so a malformed arity retains its separate PHP
diagnostic boundary. Constructors, `__invoke` and ordinary methods continue to
permit reference parameters. The validation reuses existing method metadata
and adds no runtime bytecode, opcode, layout field, dependency or unsafe block.

An original source-aware E2E regression covers namespace-qualified class
names, trait/interface/abstract/enum declarations, case-preserving
`__callStatic`, serialization methods, reference-before-return-error priority
and the allowed constructor/`__invoke`/ordinary-method boundary. The complete
157-case `Zend/tests/magic_methods` directory rises from 85 to 92 passes with
no other movement. All five Cargo configurations, all-features/all-targets,
formatting, PHPT runner self-test, unsafe self-test and the exact unsafe ratchet
pass. The production inventory remains 1,613 unsafe blocks, 289 unsafe
functions and 312 SAFETY annotations. Composer S0, all four Symfony S1 gates
and exact PHP 8.5.9 warmed-kernel S2 and cold-build S3 also pass. Two
sequential final runs have byte-identical merged manifests and summaries. Their
manifest and summary SHA-256 values are
`b68afb9a597ca2831a85e1f36f817730f4d0e88093c106e4fad914b7f2f62a83`
and `c458c26b0aee2aa52aeb006fc37784e0b1caedf3b5917f9d4bb945165776144f`.

A local 21-pair alternating release control on an AMD Ryzen 9 7950X pinned to
CPU 0 with the performance governor, exact-output validation, two additional
warmups and JIT/quick loops disabled compiled 10,000 unique classes and 40,000
successful method declarations per observation through `eval()`. The mix
includes ordinary methods, ordinary reference parameters and the allowed
constructor/`__invoke` reference forms, exercising the new validator's
worst-case successful path. No sample was excluded and every run returned
checksum `198890`. Baseline median/mean was 2.199734/2.201690 seconds and
candidate median/mean was 2.191118/2.193328 seconds. The independent median
and mean ratios are 0.996083 and 0.996202; paired median and mean ratios are
0.996351 and 0.996216, below the +5% gate. The exact candidate binary SHA-256
is `f41027dc022db2255cec1693a45a54e96aacb1a1a0e698d14800bf8466bdce99`.

General magic-method argument-count/type/staticness diagnostics remain
separate work. The runtime propagation of an alias through a valid
`__invoke(&$argument)` call is also not claimed by this compile-time
checkpoint.

The latest measured AMD64 PHP 8.5 contract checkpoint is pinned to php-src
8.5.6 commit `fcc29c8` and candidate commit `e3e87f4f`. Across all 5,599
unmodified `Zend/tests` and `tests/lang` cases, 3,890 pass, 1,409 fail, 115
skip, none remain XFAIL, 185 are unsupported, and none time out or crash. The
headline pass rate is 73.410% and the whole-corpus rate is 69.477%; 4,748 of
5,299 attempted cases reach runtime (89.602%). Relative to exact candidate
`e7c108fc`, the pass-set delta is +4/-0:
`Zend/tests/assign_ref_to_overloaded_prop.phpt`,
`Zend/tests/bug69732.phpt`, `Zend/tests/bug70083.phpt` and
`Zend/tests/exceptions/exception_during_by_reference_magic_get.phpt` become
exact passes. Every previous pass remains a pass.

Two expected non-pass cases advance from output mismatch to their later
runtime boundary. `Zend/tests/magic_methods/bug32660.phpt` now emits both
indirect-modification notices and the expected overloaded-object reference
error before its independent missing object-dump output. The lazy-proxy case
`Zend/tests/lazy_objects/gh20873.phpt` now reaches that same target error after
independent lazy initialization, undefined-static-property and source-
diagnostic differences. There are no other status or failure-category
transitions. Two sequential final runs have byte-identical merged manifests
and summaries. Their manifest and summary SHA-256 values are
`7f22c0ee77535a08b680f4bb24e3a3b9adc4c9a7d556d979216b65258542c8ba`
and `24224ec208f4e3ff7ac98ae97f748638c7cc0d114914a351ba1abc13e539d36a`.

Reference binding to a missing or ordinarily inaccessible object property now
resolves `__get()` before rejecting the overloaded target with PHP 8.5's
`Cannot assign by reference to overloaded object` error. A scalar result from
a by-value getter first emits the indirect-modification notice; object,
Closure and reference results do not. Getter exceptions propagate, while an
exception thrown by the target-side notice handler is replaced by the required
assignment error after retaining the handler's side effects. Asymmetric set
visibility and readonly restrictions retain precedence over magic access, and
an existing dynamic property still binds directly.

The compiler retains a raw call-result operand for an object-property target
until that target is resolved. An overloaded target therefore suppresses the
otherwise applicable by-value-call notice, while an ordinary direct property
emits it before commit and a reference-returning call preserves its alias.
Source-side acquisition such as `$alias =& $object->missing` now emits the
indirect-modification notice for a non-reference scalar `__get()` result and
propagates a throwing notice handler. The change reuses the existing
`BindObjPropRef` opcode and cold object-property machinery; it adds no opcode,
instruction-layout field, dependency or unsafe block.

An original E2E regression covers source/getter ordering, by-value and
reference-returning call sources, scalar/object/reference getter results,
throwing getters, notice-handler precedence, inaccessible and asymmetric
properties, direct dynamic-property binding and source-side acquisition. The
64-case adjacent property-reference slice has 41 passes, 17 failures and six
unsupported capability cases; the broader 154-case exact `=&` slice has 94
passes, 38 failures and 22 unsupported cases, with no lost pass in either.
All five Cargo configurations, all-features/all-targets, formatting, PHPT
runner self-test, unsafe self-test and the exact unsafe ratchet pass. The
production inventory remains 1,613 unsafe blocks, 289 unsafe functions and
312 SAFETY annotations. Composer S0, all four Symfony S1 gates and exact PHP
8.5.9 warmed-kernel S2 and cold-build S3 also pass.

A local 21-pair alternating release control on an AMD Ryzen 9 7950X pinned to
CPU 0 with the performance governor, exact-output validation, two additional
warmups and JIT/quick loops disabled executed five million ordinary declared-
property reference binds per observation. No sample was excluded and every
run returned checksum `7500000|1|2`. Baseline median/mean was
2.801567/2.798900 seconds and candidate median/mean was 2.759919/2.767732
seconds. The independent median and mean ratios are 0.985134 and 0.988864;
paired median and mean ratios are 0.995755 and 0.990039, below the +5% gate.
The host ran x86-64 Linux 7.0.0 with 31 GiB RAM and rustc 1.93.1. The exact
candidate binary SHA-256 is
`349cc3f7e6537c430d7767686a300f0f4a82c8daabd667b899259f28ff919f72`.

Nested property/array target paths, lazy-proxy completion and the independent
object-dump/GC gaps remain outside this checkpoint. Property-hook behavior is
not broadened beyond its previously tested contract.

The latest measured AMD64 PHP 8.5 contract checkpoint is pinned to php-src
8.5.6 commit `fcc29c8` and candidate commit `e7c108fc`. Across all 5,599
unmodified `Zend/tests` and `tests/lang` cases, 3,886 pass, 1,413 fail, 115
skip, none remain XFAIL, 185 are unsupported, and none time out or crash. The
headline pass rate is 73.335% and the whole-corpus rate is 69.405%; 4,748 of
5,299 attempted cases reach runtime (89.602%). Relative to exact base
`ef6f6385`, the pass-set delta is +3/-0:
`Zend/tests/assign_by_val_function_by_ref_return_value.phpt`,
`Zend/tests/assign_obj_ref_byval_function.phpt` and
`tests/lang/bug21600.phpt` become exact passes. Every previous pass remains a
pass.

Three expected non-pass cases advance from runtime failure to their independent
output boundaries. `Zend/tests/assign_ref_error_var_handling.phpt` reaches its
overflow and notice-ordering checks, `Zend/tests/bug69732.phpt` reaches the
existing overloaded-property behavior, and `Zend/tests/bug70083.phpt` reaches
the existing missing overloaded-property reference error. There are no other
status or failure-category transitions. Two sequential final runs have byte-
identical merged manifests and summaries. Their manifest and summary SHA-256
values are
`085da6b97776d815681100eca30c740e998e8c5acb630a3b74b68ed5fffa41b4`
and `ba66d0d3b02ff7112b778f1104d2afef20d8eef26833596c9ae6758cb1b7d2fd`.

Reference assignment to dynamic-variable, global, object-property, static-
property and one-dimensional array targets now accepts a function, method,
static-method or dynamic-call result. The compiler prepares the observable
target location before invoking the source, then materializes the call result
through the existing `BindCvRef` contract. A reference-returning call therefore
preserves its alias, while a by-value result emits PHP 8.5's notice before the
write commits. Existing mutable-l-value sources retain source-before-target
ordering, including rehash-sensitive array aliases. Direct variable-array
append from a call result uses the same internal reference materialization.
This adds no opcode, VM path, instruction-layout field, dependency or unsafe
block.

Original E2E regressions cover target/source order and cardinality, re-entrant
one-dimensional array mutation, returned-reference identity and copy-on-write,
direct append, dynamic calls, the by-value notice and a throwing notice handler
that prevents the write. The complete 154-case adjacent `=&` source slice
retains 91 passes, 41 failures and 22 unsupported capability cases with no
regression. All five Cargo configurations, all-features/all-targets,
formatting, PHPT runner self-test, unsafe self-test and the exact unsafe ratchet
pass. The production inventory remains 1,613 unsafe blocks, 289 unsafe
functions and 312 SAFETY annotations. Composer S0, all four Symfony S1 gates
and exact PHP 8.5.9 warmed-kernel S2 and cold-build S3 also pass.

A local 21-pair alternating release control on an AMD Ryzen 9 7950X pinned to
CPU 0 with the performance governor, three warmups and JIT/quick loops disabled
compiled and executed 50,000 existing array-target reference assignments per
observation through `eval()`. No sample was excluded and every run returned
checksum `1250025000`. Baseline median/mean was 0.813000/0.815333 seconds and
candidate median/mean was 0.804000/0.813429 seconds. The independent median and
mean ratios are 0.988930 and 0.997664; paired median and mean ratios are
0.986618 and 0.998675, below the +5% gate. The host ran x86-64 Linux 7.0.0 with
30 GiB RAM and rustc 1.93.1. The exact candidate binary SHA-256 is
`af20557d6d5d00996da6609654ee6439c8521700af597ae6ddd92aed0cb91f21`.

Nested array target paths and call-result appends through a complex mutable
root still require a deferred path representation that survives source-call
re-entry. They remain explicitly rejected instead of committing from a stale
container snapshot and are not claimed by this checkpoint.

The latest measured AMD64 PHP 8.5 contract checkpoint is pinned to php-src
8.5.6 commit `fcc29c8` and candidate commit `ef6f6385`. Across all 5,599
unmodified `Zend/tests` and `tests/lang` cases, 3,883 pass, 1,416 fail, 115
skip, none remain XFAIL, 185 are unsupported, and none time out or crash. The
headline pass rate is 73.278% and the whole-corpus rate is 69.352%; 4,748 of
5,299 attempted cases reach runtime (89.602%). Relative to exact base
`f186311e`, the pass-set delta is +5/-0: `Zend/tests/bug69376.phpt`,
`Zend/tests/bug69376_2.phpt`, `Zend/tests/bug71529.phpt`,
`Zend/tests/bug73916.phpt` and `Zend/tests/gc/gc_034.phpt` become exact
passes. Every previous pass remains a pass.

Three expected non-pass cases advance past their former parser boundary.
`Zend/tests/gc/bug70805.phpt` now reaches its final GC count and retains the
independent root-accounting divergence (`int(9999)` rather than `int(0)`).
`Zend/tests/gc/bug80072.phpt` reaches the existing chained reference-assignment
and comparison-precedence boundary. `Zend/tests/indexing_001.phpt` reaches the
earlier value-append section and stops at the existing uncatchable scalar-to-
array error before exercising its reference-append section. There are no other
status or failure-category transitions. Two sequential final runs have byte-
identical merged manifests and summaries. Their manifest and summary SHA-256
values are
`5e38bbd069c1ba0ce83a57c8fad8017657e3c644dab57c518268efbf76a873d5`
and `c916ff69892a0daa7f0b3cd00bf699cecd0b40eb56f85b0e1a5af5a9617eff9b`.

A simple-variable `$target[] =& $source` statement now bypasses only the
ordinary value-push parser fast path and enters the existing general
`ArrayAppendAssign` reference AST. Its established compiler and VM lowering
reuse the source reference cell, so source mutation, copy-on-write aliases,
self-cycles and array-index evaluation match PHP 8.5. Ordinary `$target[] =`
statements retain `Stmt::ArrayPush`, and the compile-time prohibition on
appending to `$GLOBALS` is unchanged. No compiler or VM path, opcode,
instruction layout, dependency or unsafe block changes.

Original parser and E2E regressions cover the reference AST selection without
displacing ordinary pushes, alias and copy-on-write identity, recursive
self-reference, exactly-once indexed-source evaluation and the `$GLOBALS`
diagnostic. The complete 20-case upstream source-form slice rises from two to
seven passes; its five remaining failures and eight CLI-INI capability cases
are explicit. All five Cargo configurations,
all-features/all-targets, formatting, PHPT runner self-test, unsafe self-test
and the exact unsafe ratchet pass. The production inventory remains 1,613
unsafe blocks, 289 unsafe functions and 312 SAFETY annotations. Composer S0,
all four Symfony S1 gates and exact PHP 8.5.9 warmed-kernel S2 and cold-build
S3 also pass.

A local 21-pair alternating release control on an AMD Ryzen 9 7950X pinned to
CPU 0 with the performance governor, default features, JIT and quick loops
disabled, and three warmups compiled and executed 9,000 ordinary append sites
per observation. No sample was excluded and every run returned checksum
`9000`. Baseline p10/median/p90/mean was
0.269393/0.273275/0.293300/0.278394 seconds and candidate
0.268445/0.271907/0.295673/0.277731 seconds. The independent median and mean
ratios are 0.994996 and 0.997620; paired p10/median/p90/mean ratios are
0.939792/0.997561/1.047827/0.998545, below the +5% gate. The host ran x86-64
Linux 7.0.0 with 31.9 GB RAM and rustc 1.93.1. The exact candidate binary
SHA-256 is
`51f0c6aa5df45bf87156d265be813682cb6f2bae2c718542c71e70b797e8e9ec`.

The latest measured AMD64 PHP 8.5 contract checkpoint is pinned to php-src
8.5.6 commit `fcc29c8` and candidate commit `f186311e`. Across all 5,599
unmodified `Zend/tests` and `tests/lang` cases, 3,878 pass, 1,421 fail, 115
skip, none remain XFAIL, 185 are unsupported, and none time out or crash. The
headline pass rate is 73.184% and the whole-corpus rate is 69.262%; 4,740 of
5,299 attempted cases reach runtime (89.451%). Relative to exact base
`074ae14d`, the pass-set delta is +8/-0:
`Zend/tests/closures/closure_call_bind.phpt`,
`Zend/tests/first_class_callable/constexpr/error_namespace_no_fallback_002.phpt`,
`Zend/tests/first_class_callable/constexpr/error_static_call_instance_method.phpt`,
`Zend/tests/first_class_callable/constexpr/error_unknown_class.phpt`,
`Zend/tests/first_class_callable/constexpr/error_unknown_method.phpt`,
`Zend/tests/first_class_callable/first_class_callable_008.phpt`,
`Zend/tests/first_class_callable/first_class_callable_012.phpt` and
`Zend/tests/gh20113.phpt` become exact passes. Every previous pass remains a
pass.

Two expected non-pass cases advance past their former parser boundary.
`first_class_callable_dynamic.phpt` now produces all six expected callable
results and remains an output failure only because RPHP diagnoses the final
unused undefined `$nam` expression that PHP elides. `property_hooks/bug003.phpt`
now reaches runtime and retains the separate missing compile-time prohibition
for a parent property-hook callable. There are no other status or failure-
category transitions. Two sequential final runs have byte-identical merged
manifests and summaries. Their manifest and summary SHA-256 values are
`a3ee585777436fd1cf5fd593955400e9122223b51714a383bc28561dbc2af0d4`
and `bdc62f05f68a9447ced644576db38f7bac5b081e9d63123407083f4477e62365`.

The postfix parser now recognizes first-class callable placeholders after
dynamic class and method expressions, including braced members, and accepts
the explicit `namespace\name(...)` function form. Owner and member expressions
reuse the ordinary two-element callable representation, preserving left-to-
right, exactly-once evaluation before deferred invocation. Named and dynamic
`new ...(...)` forms and nullsafe method callable syntax instead record PHP
8.5's deferred compile fatal, including in non-executed source. General first-
class callable AST nodes retain their creation line, and the compiler attaches
it to the existing `CreateFirstClassCallable` instruction through the sparse
source map so failed class and method resolution keeps the creation origin and
caller trace.

Original parser regressions cover namespace-relative, dynamic static and
dynamic instance forms plus named/dynamic `new` and named/braced nullsafe
rejections. Two original E2E regressions cover successful dispatch, exact
resolution origin and trace, and owner/member evaluation order and cardinality.
The change adds no opcode, instruction-layout field, runtime resolver special
case, dependency or unsafe block. The production unsafe inventory remains
1,613 blocks, 289 unsafe functions and 312 SAFETY annotations. All five Cargo
configurations, all-features/all-targets, formatting, PHPT runner self-test,
unsafe self-test and the exact unsafe ratchet pass, as do all 31 pipe PHPTs,
Composer S0, all four Symfony S1 gates and exact PHP 8.5.9 warmed-kernel S2 and
cold-build S3.

Twenty-one alternating CPU-pinned default-release pairs with JIT and quick
loops disabled exercised two million existing dynamic string callable
creations and calls per run. Baseline p10/median/p90/mean was
0.648512/0.665015/0.691699/0.670971 seconds and candidate
0.636141/0.656294/0.682324/0.659312 seconds. The independent median and mean
ratios are 0.986886 and 0.982624; paired p10/median/p90/mean ratios are
0.950911/0.988196/1.026965/0.983541. A second control compiled and evaluated
5,000 existing dynamic string callable sites per run. Baseline
p10/median/p90/mean was 0.298809/0.302361/0.315235/0.305532 seconds and
candidate 0.298343/0.301279/0.306368/0.302699 seconds. Its independent median
and mean ratios are 0.996420 and 0.990726; paired p10/median/p90/mean ratios
are 0.954550/0.992131/1.021049/0.991714. Both independent medians are below
the +5% gate and every run retained its exact `7000000` or `3` checksum. The
exact candidate binary SHA-256 is
`bf5fe8b6f4bb61f8b0cd7abc55f7cb5464360545d8f65fbba8ee23141a731e51`.

The latest measured AMD64 PHP 8.5 contract checkpoint is pinned to php-src
8.5.6 commit `fcc29c8` and candidate commit `074ae14d`. Across all 5,599
unmodified `Zend/tests` and `tests/lang` cases, 3,870 pass, 1,429 fail, 115
skip, none remain XFAIL, 185 are unsupported, and none time out or crash. The
headline pass rate is 73.033% and the whole-corpus rate is 69.119%; 4,733 of
5,299 attempted cases reach runtime (89.319%). Relative to exact base
`9e7dc700`, the pass-set delta is +2/-0:
`Zend/tests/first_class_callable/constexpr/error_namespace_no_fallback_001.phpt`
and
`Zend/tests/first_class_callable/constexpr/error_unknown_function.phpt`
become exact passes. Every previous pass remains a pass, and there are no
other status or failure-category transitions. Two sequential final runs have
byte-identical merged manifests and summaries. Their manifest and summary
SHA-256 values are
`54baa407f1b4ddd9d078265c793410c8dea2fffcfc9c8e23af2ef47df37cc543`
and `93413a32449ac0527b46b128a0409aef768411be287a9cc50efe44f53eaa046e`.

Named first-class function callables now retain their source line in the AST.
The compiler attaches that line to the existing `CreateFirstClassCallable`
instruction through the sparse source map, so its established resolution
error path records the creation file and line and preserves the PHP 8.5 caller
trace. Pipe lowering supplies its existing pipe source line and remains fully
passing. Dynamic, method and generic callable expressions retain their
separate existing representation; the unsupported `namespace\\name(...)`
parser form remains a separate compatibility gap.

One original E2E regression catches a missing namespaced callable and verifies
its exact message, creation origin and two-frame trace. The change adds no
opcode, instruction-layout field, runtime special case, dependency or unsafe
block. The production unsafe inventory remains 1,613 blocks, 289 unsafe
functions and 312 SAFETY annotations. All five Cargo configurations,
all-features/all-targets, formatting, PHPT runner self-test, unsafe self-test
and the exact unsafe ratchet pass, as do Composer S0, all four Symfony S1 gates
and exact PHP 8.5.9 warmed-kernel S2 and cold-build S3.

Twenty-one alternating CPU-pinned default-release pairs with JIT and quick
loops disabled exercised two million named callable creations and calls per
run. Baseline p10/median/p90/mean was
0.593169/0.617611/0.628399/0.611875 seconds and candidate
0.601513/0.619936/0.666258/0.624712 seconds. The independent median and mean
ratios are 1.003764 and 1.020980; paired p10/median/p90/mean ratios are
0.963352/1.024464/1.080136/1.021538. A second control compiled and evaluated
5,000 distinct named first-class callable sites per run. Baseline
p10/median/p90/mean was 0.308282/0.312811/0.324468/0.315199 seconds and
candidate 0.308133/0.311609/0.319210/0.314541 seconds. Its independent median
and mean ratios are 0.996158 and 0.997915; paired p10/median/p90/mean ratios
are 0.960039/1.002798/1.014563/0.998343. Both independent medians are below
the +5% gate and every run retained its exact `7000000` or `3` checksum. The
exact candidate binary SHA-256 is
`c9b6e9b5cf4e82abe19e0c8a6b7babfca929a2800444297f624574af56ecac64`.

The latest measured AMD64 PHP 8.5 contract checkpoint is pinned to php-src
8.5.6 commit `fcc29c8` and candidate commit `9e7dc700`. Across all 5,599
unmodified `Zend/tests` and `tests/lang` cases, 3,868 pass, 1,431 fail, 115
skip, none remain XFAIL, 185 are unsupported, and none time out or crash. The
headline pass rate is 72.995% and the whole-corpus rate is 69.084%; 4,733 of
5,299 attempted cases reach runtime (89.319%). Relative to exact base
`81b0aca3`, the pass-set delta is +5/-0: `Zend/tests/bug43651.phpt`,
`Zend/tests/dynamic_call/dynamic_fully_qualified_call.phpt`,
`Zend/tests/namespaces/ns_019.phpt`, `Zend/tests/namespaces/ns_032.phpt` and
`Zend/tests/varSyntax/indirectFcall.phpt` become exact passes. Every previous
pass remains a pass, and there are no other status or failure-category
transitions. Two sequential final runs have byte-identical merged manifests
and summaries. Their manifest and summary SHA-256 values are
`905fcdc6bf914d0508713da452e2fc4a5c6ed65e44a7697b419943527eb92171`
and `199ba39697985e3012ac443ace6faeb16a1f6177d251cc2dc1b24c0a5338c48a`.

String function callables now use one shared PHP 8.5 lookup view that removes
exactly one leading namespace separator when it precedes a name. Direct
dynamic calls, `is_callable()`, `call_user_func()` and
`Closure::fromCallable()` therefore resolve global built-ins, namespaced user
functions and static methods consistently. Empty function names and function
names beginning with multiple separators remain invalid. A constant literal
call reports the normalized missing-function name, while a runtime-built
invalid string keeps its original spelling, matching the oracle at both
diagnostic boundaries.

One original E2E regression covers all four callback consumers, built-in,
namespaced and static-method success, literal and runtime missing functions,
and the empty and double-separator boundaries. The lookup returns a borrowed
slice and adds no allocation, opcode, instruction-layout field, dependency or
unsafe block. The production unsafe inventory remains 1,613 blocks, 289 unsafe
functions and 312 SAFETY annotations. All five Cargo configurations,
all-features/all-targets, formatting, PHPT runner self-test, unsafe self-test
and the exact unsafe ratchet pass, as do Composer S0, all four Symfony S1 gates
and exact PHP 8.5.9 warmed-kernel S2 and cold-build S3.

Twenty-one alternating CPU-pinned default-release pairs with JIT and quick
loops disabled exercised five million ordinary dynamic string calls per run.
Baseline p10/median/p90/mean was
0.900758/0.910348/1.024937/0.931098 seconds and candidate
0.889305/0.899474/0.933861/0.905416 seconds. The independent median and mean
ratios are 0.9881 and 0.9724; paired p10/median/p90/mean ratios are
0.9185/0.9861/1.0214/0.9756. The median is below the +5% gate and every run
retained `17500000`. The exact candidate binary SHA-256 is
`9cec1e714bf5f030561dfa8c2bbd3d700ceac94b976a35af7ea05d077f470a6d`.

The latest measured AMD64 PHP 8.5 contract checkpoint is pinned to php-src
8.5.6 commit `fcc29c8` and candidate commit `81b0aca3`. Across all 5,599
unmodified `Zend/tests` and `tests/lang` cases, 3,863 pass, 1,436 fail, 115
skip, none remain XFAIL, 185 are unsupported, and none time out or crash. The
headline pass rate is 72.901% and the whole-corpus rate is 68.994%; 4,733 of
5,299 attempted cases reach runtime (89.319%). Relative to exact base
`639cf6e3`, the pass-set delta is +7/-0:
`Zend/tests/generators/bug65161.phpt`, `Zend/tests/gh8841.phpt`,
`Zend/tests/magic_methods/bug70967.phpt`,
`Zend/tests/namespaces/bug77376.phpt`,
`Zend/tests/namespaces/ns_092.phpt`,
`Zend/tests/use_function/no_global_fallback.phpt` and
`Zend/tests/use_function/no_global_fallback2.phpt` become exact passes. Every
previous pass remains a pass, and there are no other status or
failure-category transitions. Two sequential final runs have byte-identical
merged manifests and summaries. Their manifest and summary SHA-256 values are
`5f892873e1f16b9f80403f3d23bd6c18184698caff22fc9e7979e9774280fbab`
and `8bc3412bdae180ea75396bdfc4be0ada88c7f2c23f7c6754d92fda0d1850efad`.

Ordinary direct named calls now attach their source line to `InitFcall`, where
function resolution can fail before argument evaluation or `DoFcall`. The
existing exception path therefore records the undefined-function origin in
the actual function, generator, namespace, shutdown callback or magic-method
frame, while the established call-site metadata retains each caller in the
trace. This also completes the direct `echo` `__toString()` trace shape
exercised by `bug70967.phpt`. Indirect, dynamic and first-class callable gaps
remain separate contracts; this checkpoint does not claim them.

One original E2E regression covers both an ordinary caught undefined call and
an undefined call dispatched through `__toString()`, including exact files,
lines and stored traces. The compiler reuses the existing sparse source map
and adds no opcode, instruction-layout field, runtime special case,
dependency or unsafe block. The production unsafe inventory remains 1,613
blocks, 289 unsafe functions and 312 SAFETY annotations. All five Cargo
configurations, all-features/all-targets, formatting, PHPT runner self-test,
unsafe self-test and the exact unsafe ratchet pass, as do Composer S0, all four
Symfony S1 gates and exact PHP 8.5.9 warmed-kernel S2 and cold-build S3.

Twenty-one alternating CPU-pinned default-release pairs with JIT and quick
loops disabled exercised five million successful direct calls per run.
Baseline p10/median/p90/mean was
0.331634/0.334895/0.360490/0.339787 seconds and candidate
0.334319/0.336718/0.362870/0.343275 seconds. The independent median ratio is
1.0054 and paired p10/median/p90 ratios are 0.9661/1.0122/1.0691. A second
control compiled and evaluated 5,000 distinct direct call sites per run:
baseline p10/median/p90/mean was
0.227856/0.229909/0.263390/0.242841 seconds and candidate
0.227276/0.230299/0.245289/0.236651 seconds, with independent median ratio
1.0017 and paired p10/median/p90 ratios 0.9313/0.9979/1.0265. Both medians are
below the +5% gate and every run retained its exact checksum. The exact
candidate binary SHA-256 is
`46af8d44f79877f0590c86e68fbb93c236ebb31de6b98e1fc77e143b65e24d15`.

The latest measured AMD64 PHP 8.5 contract checkpoint is pinned to php-src
8.5.6 commit `fcc29c8` and candidate commit `639cf6e3`. Across all 5,599
unmodified `Zend/tests` and `tests/lang` cases, 3,856 pass, 1,443 fail, 115
skip, none remain XFAIL, 185 are unsupported, and none time out or crash. The
headline pass rate is 72.768% and the whole-corpus rate is 68.869%; 4,733 of
5,299 attempted cases reach runtime (89.319%). Relative to exact base
`c79a2045`, the pass-set delta is +4/-0:
`Zend/tests/bug60909_2.phpt`,
`Zend/tests/exception_in_rope_end.phpt`, `tests/lang/bison1.phpt` and
`tests/lang/bug25922.phpt` become exact passes. Every previous pass remains a
pass, and there are no other status or failure-category transitions. Two
sequential final runs have byte-identical status maps, merged manifests and
summaries. Their manifest and summary SHA-256 values are
`25acfdba5f87b4dbeda1aa3094483d092f1078e8710e627a4bb1562f0e3d6989`
and `4d03e17d7c88de6141ba300591086ff2e832075d4d4812312d9d577a04e56349`.

Simple variable interpolation now retains its source line through lexing and
expression lowering. The compiler attaches the active operand site to ordinary
concatenation, compact concatenation assignment and both statement and
value-producing `.=` forms. Object conversion then links the engine-dispatched
`__toString()` frame to that currently active instruction, so live
`debug_backtrace()` frames and stored Throwable origin/trace data expose PHP
8.5's exact file and line. The rule also lets undefined-variable diagnostics in
interpolation and exceptions raised while finalizing a rope unwind from the
correct source boundary.

One original E2E regression covers live traces for explicit concat,
interpolation, statement `.=` and value-producing `.=` plus the stored trace
of a throwing interpolated conversion. The change adds no opcode, call-frame
or instruction-layout field, dependency or unsafe block. Detached logical
callers use a capacity-reusing small collection instead of paying a hash-table
insert/remove cost for every successful conversion; nested and suspended
frames remain keyed by frame identity. The production unsafe inventory remains
1,613 blocks, 289 unsafe functions and 312 SAFETY annotations. All five Cargo
configurations, all-features/all-targets, formatting, PHPT runner self-test,
unsafe self-test and the exact unsafe ratchet pass, as do Composer S0, all four
Symfony S1 gates and exact PHP 8.5.9 warmed-kernel S2 and cold-build S3.

Twenty-one alternating CPU-pinned default-release pairs with JIT and quick
loops disabled exercised 1.5 million object `__toString()` concatenations per
run. Baseline p10/median/p90 was 0.582162/0.589987/0.608089 seconds and
candidate 0.585155/0.601723/0.624659 seconds. The independent median ratio is
1.0199; paired p10/median/p90 ratios are 0.9832/1.0218/1.0878, with the paired
median below the +5% gate, and every run retained `6000000|abcd`. The exact
candidate binary SHA-256 is
`c71850021ab1f847b2f90bb7506031f78e13203f04c09f9795b23a466cc7e29f`.
Direct `echo` failure-origin rendering in
`Zend/tests/magic_methods/bug70967.phpt` remains a separate trace-shape gap;
this checkpoint does not claim it.

The latest measured AMD64 PHP 8.5 contract checkpoint is pinned to php-src
8.5.6 commit `fcc29c8` and candidate commit `c79a2045`. Across all 5,599
unmodified `Zend/tests` and `tests/lang` cases, 3,852 pass, 1,447 fail, 115
skip, none remain XFAIL, 185 are unsupported, and none time out or crash. The
headline pass rate is 72.693% and the whole-corpus rate is 68.798%; 4,733 of
5,299 attempted cases reach runtime (89.319%). Relative to exact base
`4553f347`, the pass-set delta is +2/-0:
`Zend/tests/magic_methods/class_toString_concat_with_itself.phpt` and
`Zend/tests/operator_unsupported_types.phpt` become exact passes, and every
previous pass remains a pass.

Three already failing cases move from output to runtime failure because the
new conversion boundary now exposes their earlier independent gaps. Delayed
attribute validation still lacks `ReflectionClassConstant` string rendering;
`bug60909_2.phpt` now propagates its `__toString()` exception but retains a
separate trace-shape mismatch; and nested coalesce assignment still reaches an
extra `ArrayAccess::offsetSet()` whose object argument cannot be rendered.
Two sequential final runs have byte-identical status maps, merged manifests
and summaries. Their manifest and summary SHA-256 values are
`c7250d7884c0897b181bd2fb0dfe799b17d9d3bbd1b13cd3412ed867ecc76b3a`
and `d14ff2f770e44d7fb01280031ce02a30667990cc0df1a667a6fb78edc27b0c13`.

Binary concatenation and compound `.=` now share one PHP 8.5 object-string
conversion rule. Objects without `__toString()` and Closures raise the
catchable class-specific `Error`; valid methods supply the string, weak method
files coerce scalar returns, and strict files or non-scalar returns raise the
exact `TypeError` including the returned runtime type. Ordinary binary `.`
preflights array warnings before object conversion and re-reads a later
operand after a re-entrant warning handler. Compound assignment instead
converts in source order and commits only after both conversions succeed. The
direct-CV `.=` path snapshots both evaluated operands, preserving aliases and
overwriting re-entrant target mutations only on success; value-producing
compound forms retain their compiler-materialized operands. Failed conversion
performs no operator writeback.

Two original E2E regressions cover plain objects, Closures, arrays, valid and
invalid magic returns, weak and strict return coercion, warning order,
re-entrant handlers, statement and value-producing compound forms, unchanged
failure targets, aliases and successful re-entrant writeback. The change adds
no opcode, layout field or dependency. Tighter operand and commit boundaries
lower the production unsafe inventory from 1,615 to 1,613 blocks while
retaining 289 unsafe functions. All five Cargo configurations,
all-features/all-targets, formatting, PHPT runner self-test, unsafe self-test
and the exact unsafe ratchet pass, as do Composer S0, all four Symfony S1 gates
and exact PHP 8.5.9 warmed-kernel S2 and cold-build S3.

Twenty-one alternating CPU-pinned default-release pairs with JIT and quick
loops disabled exercised eight million dynamic string concatenations per run.
Baseline p10/median/p90 was 1.192663/1.217943/1.263336 seconds and candidate
1.196339/1.222113/1.244458 seconds. The independent median ratio is 1.0034;
paired p10/median/p90 ratios are 0.9666/0.9935/1.0231, below the +5% gate, and
every run retained `32000000|cdab`. The exact candidate binary SHA-256 is
`30b7cb7aa593e5e8b32dfdf621d43a3cf14345a74d9471e81531f0fdfd94c399`.

The latest measured AMD64 PHP 8.5 contract checkpoint is pinned to php-src
8.5.6 commit `fcc29c8` and candidate commit `4553f347`. Across all 5,599
unmodified `Zend/tests` and `tests/lang` cases, 3,850 pass, 1,449 fail, 115
skip, none remain XFAIL, 185 are unsupported, and none time out or crash. The
headline pass rate is 72.655% and the whole-corpus rate is 68.762%; 4,733 of
5,299 attempted cases reach runtime (89.319%). Relative to exact base
`8ec911bb`, the pass-set and failure-category delta is intentionally +0/-0.
Two sequential final runs have byte-identical status maps, merged manifests
and summaries. Their manifest and summary SHA-256 values are
`76db41be2f0596ea31dc3b185dfe6be30c1cdfdacd1a711b71cdc3c1c1d00afc`
and `9b281472d905591480dba693910580fb88d79a73ef0d8d5b8817b455338b295e`.

Live and closed resources are no longer implicitly converted to their request
ID by arithmetic or integer-only operators. Addition, subtraction,
multiplication, division, exponentiation, modulo, shifts, bitwise operators,
unary signs and bitwise not now use PHP 8.5's operator-specific `TypeError`
contract. Explicit integer casts, loose resource-ID comparisons and resource
array-key conversion retain their separate existing behavior. Resource
increment/decrement also retains its already passing dedicated error contract.

For commutative multiplication and bitwise operations, PHP canonicalizes an
unsupported object/Closure or resource before a lower-priority operand and
does not emit conversion diagnostics from that lower operand. Compound
assignment instead converts in source order, so a leading-numeric warning or
float-to-int deprecation can precede the resource `TypeError`; a failed write
leaves its target unchanged. A low opcode-local flag distinguishes that cold
compound boundary in both statement and value-producing compiler paths while
preserving the 16-byte instruction layout. One original E2E regression covers
ordinary and specialized opcode shapes, both operand directions, diagnostic
ordering and suppression, compound statements and expressions, unchanged
targets, unary forms, explicit casts and comparisons. The complete arithmetic
and bitwise sections of `Zend/tests/operator_unsupported_types.phpt` now match
its exact expectation; the case remains an output failure only at the separate
object-concatenation gap.

All five Cargo configurations, all-features/all-targets, formatting, unsafe
self-test and the exact unsafe ratchet pass, as do Composer S0, all four
Symfony S1 gates and exact PHP 8.5.9 warmed-kernel S2 and cold-build S3. The
production unsafe inventory remains 1,615 blocks and 289 functions.
Twenty-one alternating CPU-pinned default-release pairs with JIT and quick
loops disabled exercised twelve million ordinary double operations per run.
Baseline p10/median/p90 was 0.602771/0.610268/0.616555 seconds and candidate
0.600825/0.603922/0.611089 seconds. The independent median ratio is 0.9896;
paired p10/median/p90 ratios are 0.9770/0.9936/1.0023, below the +5% gate, and
every run retained `10875000|3000000`. The exact candidate binary SHA-256 is
`a9ef00c36fb3b717a7f5d7e029a5c36667a2ac2d95cbd143dbce39a060188e94`.

The latest measured AMD64 PHP 8.5 contract checkpoint is pinned to php-src
8.5.6 commit `fcc29c8` and candidate commit `8ec911bb`. Across all 5,599
unmodified `Zend/tests` and `tests/lang` cases, 3,850 pass, 1,449 fail, 115
skip, none remain XFAIL, 185 are unsupported, and none time out or crash. The
headline pass rate is 72.655% and the whole-corpus rate is 68.762%; 4,733 of
5,299 attempted cases reach runtime (89.319%). Relative to exact base
`4257b3e0`, the pass-set delta is exactly +10/-0: `Zend/tests/add_006.phpt`,
`constexpr/constant_expressions.phpt`,
`constexpr/constant_expressions_dynamic.phpt`, both
`numeric_strings/invalid_numeric_string*` cases, and the five
`tests/lang/operators/{add,subtract,multiply,divide,negate}_variationStr.phpt`
cases become passes. Every prior pass remains a pass, and there are no other
status or failure-category transitions.

Two sequential final full runs have byte-identical merged manifests and
summaries. Their SHA-256 values are
`76db41be2f0596ea31dc3b185dfe6be30c1cdfdacd1a711b71cdc3c1c1d00afc`
and `1c3d28a4e8404a3aee6d9df0f3324d7ba8be7fa0d8186b24a14fbd018e3887bf`.
The exact default-release candidate binary SHA-256 is
`c2d9d31261fb5236415c8f960b6077ad62779723e0bd575c48bfbf0f6c8855ff`.

Ordinary addition, subtraction, multiplication, division and exponentiation
now accept PHP leading-numeric strings while retaining whether each operand
requires `E_WARNING: A non-numeric value encountered`. The shared cold
conversion path reports warnings left-to-right before committing a result;
an invalid left operand therefore reports no right warning, while a valid
leading-numeric left operand reports its warning before an invalid right
operand throws. A throwing error handler interrupts conversion immediately
and leaves a compound-assignment target unchanged. Unary plus and minus share
the same rule through their existing multiplication lowering. Complete
numeric strings retain integer results where PHP does, including exact
division and power, and constant folding preserves the integer kind only when
no runtime diagnostic is required.

One original E2E regression covers all five operators, unary forms, all
specialized addition/subtraction opcode shapes, exact division and power,
invalid-operand warning order, successful compound assignment and a throwing
handler. The shared operand converter, warning boundary and cold result
helpers add no opcode, layout field or dependency. Consolidating the touched
result writes lowers the production unsafe inventory from 1,616 to 1,615
blocks while retaining 289 unsafe functions. Integer-only operators remain on
their existing checked conversion path; resource arithmetic and broader
operator-output gaps remain separate checkpoints.

All five Cargo configurations, all-feature/all-target, formatting, PHPT runner
self-test, unsafe self-test and the exact unsafe diff ratchet pass, as do
Composer S0, all four Symfony S1 gates and exact PHP 8.5.9 warmed-kernel S2 and
cold-build S3. Twenty-one alternating CPU-pinned default-release pairs of the
ten-million-iteration mixed arithmetic control measured baseline
p10/median/p90 1.216975/1.237354/1.268309 seconds and candidate
1.267856/1.290534/1.352766 seconds. The independent median ratio is 1.0430 and
the paired-median ratio is 1.0445, below the +5% gate; paired p10/p90 ratios
are 1.0172/1.0733, and every run retained checksum `419,729999927,1`. The
exact base and candidate binary SHA-256 values are
`e3b9e4152ec62ab37758db34b59674c7a2d06c251ef5bf2b4eccb550bd8af557`
and `c2d9d31261fb5236415c8f960b6077ad62779723e0bd575c48bfbf0f6c8855ff`.

The latest measured AMD64 PHP 8.5 contract checkpoint is pinned to php-src
8.5.6 commit `fcc29c8` and candidate commit `4257b3e0`. Across all 5,599
unmodified `Zend/tests` and `tests/lang` cases, 3,840 pass, 1,459 fail, 115
skip, none remain XFAIL, 185 are unsupported, and none time out or crash. The
headline pass rate is 72.467% and the whole-corpus rate is 68.584%; 4,733 of
5,299 attempted cases reach runtime (89.319%). Relative to exact base
`d7480f5f`, the pass-set delta is exactly +2/-0:
`Zend/tests/assign_op_type_error.phpt` and `pow_array_leak.phpt` become passes,
and every prior pass remains a pass.

One remaining failure advances past the old internal exponentiation fatal
without becoming a compatibility pass.
`Zend/tests/operator_unsupported_types.phpt` moves from compile to output at
independent numeric-string warning/coercion and broader operator-output
boundaries. There are no other status or
failure-category transitions. Two sequential final full runs have
byte-identical merged manifests and summaries. Their SHA-256 values are
`69c16bae8bb481ccdc9cb9450572b1317fa67ebb6a9c9410af7fada49eddbf0b`
and `2f9f5510f652e939bb5f0a521b978af2be8e610b1de9f3caaddb52997de1ec9e`.

Unsupported binary exponentiation now throws a catchable `TypeError` through
the ordinary PHP unwind path instead of terminating with an internal
compile-style fatal. The existing `Pow` opcode reports PHP's
`Unsupported operand types: <left> ** <right>` message with concrete diagnostic
types in operand order, attaches source file, line and live trace before
unwinding, and commits no compound-assignment write after failure. Successful
integer, negative-exponent and double power operations retain their existing
execution paths and result types.

The compiler records source provenance for exponentiation compound assignments
through the same existing mechanism as addition, subtraction, multiplication
and division. The cold error arm reuses the shared operator throw helper and
adds no opcode, layout field or unsafe block. One original E2E regression
covers array and object operands, exact type order, origin and trace, unchanged
`**=` state, successful numeric values and assignment, source lines and an
eval-time constant declaration; all 24 operator E2E tests pass.

All five Cargo configurations, all-feature/all-target, formatting, PHPT runner
self-test, unsafe self-test and the exact unsafe diff ratchet pass, as do
Composer S0, all four Symfony S1 gates and exact PHP 8.5.9 warmed-kernel S2 and
cold-build S3. The production inventory remains 1,616 unsafe blocks and 289
unsafe functions. Twenty-one alternating CPU-pinned default-release pairs of
the ten-million-iteration mixed arithmetic control measured baseline
p10/median/p90 1.203929/1.298987/1.334285 seconds and candidate
1.223908/1.233313/1.246742 seconds. The independent median ratio is 0.9494 and
the paired-median ratio is 0.9558, below the +5% gate; paired p10/p90 ratios are
0.9441/1.0225, and every run retained checksum `419,729999927,1`. This sample
does not establish a speedup because the successful exponentiation branch is
unchanged. The exact base and candidate binary SHA-256 values are
`253971ae8621b7329d249fc531e40aab21e4a66e60001b5ee198ded6544b1844`
and `e3b9e4152ec62ab37758db34b59674c7a2d06c251ef5bf2b4eccb550bd8af557`.
Partial numeric-string arithmetic and the broader operator audit remain
separate work.

The latest measured AMD64 PHP 8.5 contract checkpoint is pinned to php-src
8.5.6 commit `fcc29c8` and candidate commit `d7480f5f`. Across all 5,599
unmodified `Zend/tests` and `tests/lang` cases, 3,838 pass, 1,461 fail, 115
skip, none remain XFAIL, 185 are unsupported, and none time out or crash. The
headline pass rate is 72.429% and the whole-corpus rate is 68.548%; 4,730 of
5,299 attempted cases reach runtime (89.262%). Relative to exact base
`933cdc2d`, the pass-set delta is exactly +1/-0: `Zend/tests/div_002.phpt`
becomes a pass and every prior pass remains a pass.

One remaining failure advances past the old internal division fatal without
becoming a compatibility pass. `tests/lang/operators/divide_variationStr.phpt`
moves from compile to output at the independent numeric-string coercion and
integer-versus-float formatting boundary. There are no other status or
failure-category transitions. Two sequential final full runs have
byte-identical merged manifests and summaries. Their SHA-256 values are
`144a209b59d5727d9b96f840db09566f8534b8e64d56bd6e8acce67cea1a24c2`
and `1113fb9efc5d130854d5d0c7627f7d533d6fec940b125277dbe8bd3808504b50`.

Unsupported binary division now throws a catchable `TypeError` through the
ordinary PHP unwind path instead of terminating with an internal compile-style
fatal. The existing `Div` opcode reports PHP's
`Unsupported operand types: <left> / <right>` message with concrete diagnostic
types in operand order, attaches source file, line and live trace before
unwinding, and commits no compound-assignment write after failure. Numeric
division retains its existing path, including the distinct catchable
`DivisionByZeroError` for zero divisors and its no-write compound-assignment
semantics.

The compiler records source provenance for division compound assignments
through the same existing mechanism as addition, subtraction and
multiplication. The cold error arm reuses the shared operator throw helper and
adds no opcode, layout field or unsafe block. One original E2E regression
covers array and object operands, exact type order, origin and trace, unchanged
`/=` state, preserved division-by-zero type and message, source lines and an
eval-time constant declaration; all 23 operator E2E tests pass.

All five Cargo configurations, all-feature/all-target, formatting, PHPT runner
self-test, unsafe self-test and the exact unsafe diff ratchet pass, as do
Composer S0, all four Symfony S1 gates and exact PHP 8.5.9 warmed-kernel S2 and
cold-build S3. The production inventory remains 1,616 unsafe blocks and 289
unsafe functions. Twenty-one alternating CPU-pinned default-release pairs of
the ten-million-iteration mixed arithmetic control measured baseline
p10/median/p90 1.199973/1.212753/1.284650 seconds and candidate
1.199960/1.252983/1.307749 seconds. The independent median ratio is 1.0332 and
the paired-median ratio is 1.0168, below the +5% gate; paired p10/p90 ratios are
0.9864/1.0869, and every run retained checksum `419,729999927,1`. The exact
base and candidate binary SHA-256 values are
`5a91d68075d89ed91f1598c95ec606fc4501bc82bee6e406acdf5982cd6dd8bd`
and `253971ae8621b7329d249fc531e40aab21e4a66e60001b5ee198ded6544b1844`.
Partial numeric-string arithmetic and unsupported exponentiation remain
separate work.

The latest measured AMD64 PHP 8.5 contract checkpoint is pinned to php-src
8.5.6 commit `fcc29c8` and candidate commit `933cdc2d`. Across all 5,599
unmodified `Zend/tests` and `tests/lang` cases, 3,837 pass, 1,462 fail, 115
skip, none remain XFAIL, 185 are unsupported, and none time out or crash. The
headline pass rate is 72.410% and the whole-corpus rate is 68.530%; 4,728 of
5,299 attempted cases reach runtime (89.224%). Relative to exact base
`555a7f6a`, the pass-set delta is exactly +1/-0:
`Zend/tests/mul_001.phpt` becomes a pass and every prior pass remains a pass.

Three remaining failures advance past the old internal multiplication fatal.
`Zend/tests/constexpr/constant_expressions_dynamic.phpt` moves from compile to
runtime while reaching the independent numeric-string unary-plus warning and
coercion gap. `tests/lang/operators/multiply_variationStr.phpt` and
`negate_variationStr.phpt` move from compile to output mismatches at the same
numeric-string boundary. There are no other status or failure-category
transitions. Two sequential final full runs have byte-identical merged
manifests and summaries. Their SHA-256 values are
`118cea1ddf23f8619cf1b296219625103866ee45a56534cf431fbaefa4948f9f`
and `c23e7c8984bf142b9924846ae01e0c2d5d4eb4203e77b648090eb9b0ab626fee`.

Unsupported binary multiplication now throws a catchable `TypeError` through
the ordinary PHP unwind path instead of terminating with an internal
compile-style fatal. The existing general `Mul` opcode reports PHP's
`Unsupported operand types: <left> * <right>` message with concrete diagnostic
types in operand order, attaches source file, line and live trace before
unwinding, and commits no compound-assignment write after failure. Successful
integer, overflow-to-float and double multiplication retain their existing
execution path.

The compiler records source provenance for multiplication compound assignments
through the same existing mechanism as addition and subtraction. The cold
error arm reuses the shared operator throw helper and adds no opcode, layout
field or unsafe block. One original E2E regression covers array and object
operands, unary negation lowered through multiplication, exact type order,
origin and trace, unchanged `*=` state and source line, and an eval-time
constant declaration; all 22 operator E2E tests pass.

All five Cargo configurations, all-feature/all-target, formatting, PHPT runner
self-test, unsafe self-test and the exact unsafe diff ratchet pass, as do
Composer S0, all four Symfony S1 gates and exact PHP 8.5.9 warmed-kernel S2 and
cold-build S3. The production inventory remains 1,616 unsafe blocks and 289
unsafe functions. Twenty-one alternating CPU-pinned default-release pairs of
the ten-million-iteration mixed arithmetic control measured baseline
p10/median/p90 1.211561/1.229448/1.321120 seconds and candidate
1.202832/1.212095/1.252848 seconds. The independent median ratio is 0.9859 and
the paired-median ratio is 0.9880, below the +5% gate; paired p10/p90 ratios are
0.9245/1.0126, and every run retained checksum `419,729999927,1`. This sample
does not establish a speedup because the successful multiplication branch is
unchanged. The exact base and candidate binary SHA-256 values are
`aae1523214c7dc649c9c0a59a79e23573c2f4e191896b202a4ddc39ff5540b7f`
and `5a91d68075d89ed91f1598c95ec606fc4501bc82bee6e406acdf5982cd6dd8bd`.
Partial numeric-string arithmetic and unsupported division and exponentiation
remain separate work.

The latest measured AMD64 PHP 8.5 contract checkpoint is pinned to php-src
8.5.6 commit `fcc29c8` and candidate commit `555a7f6a`. Across all 5,599
unmodified `Zend/tests` and `tests/lang` cases, 3,836 pass, 1,463 fail, 115
skip, none remain XFAIL, 185 are unsupported, and none time out or crash. The
headline pass rate is 72.391% and the whole-corpus rate is 68.512%; 4,724 of
5,299 attempted cases reach runtime (89.149%). Relative to exact base
`b0682a74`, the pass-set delta is exactly +5/-0: `Zend/tests/add_002.phpt`,
`add_003.phpt`, `add_004.phpt`, `add_007.phpt` and `throw/001.phpt` become
passes. Every prior pass remains a pass.

Nine remaining failures advance past the old internal addition fatal without
becoming compatibility passes. `add_006.phpt`, `bug74084.phpt`, both
`numeric_strings/invalid_numeric_string*` cases,
`readonly_props/readonly_containing_object.phpt` and `try/bug73337.phpt` move
from compile to runtime failures; `constexpr/new.phpt`,
`tests/lang/bug28800.phpt` and `tests/lang/operators/add_variationStr.phpt`
move from compile to output mismatches. These transitions expose independent
numeric-string warning/coercion, dynamic-variable/reference, readonly,
constant-expression and later-operator gaps. Two sequential final full runs
have byte-identical merged manifests and summaries. Their SHA-256 values are
`e57cb25c742e301281a1f8e7570293594df6fe5fc6c5c71787764ed207860188`
and `0cbacd8d55471c36c4c8b48980bb819ecad0d2ba8bbddeeab6b016f9f25fdeda`.

Unsupported binary addition now throws a catchable `TypeError` through the
ordinary PHP unwind path in all three emitted addition opcode shapes. The
message is PHP's `Unsupported operand types: <left> + <right>` with concrete
diagnostic types in operand order. Source file, line and live trace are
attached before unwinding, and a failing compound assignment does not commit
its result. Successful numeric addition and array union retain their existing
paths and semantics; the latter remains a negative control for the error
branch. This also closes the throw-expression case whose operand expression is
`new Exception() + 1`.

The compiler records source provenance for addition compound assignments just
as it already does for subtraction. The baseline error arms reuse the existing
cold operator throw helper and add no opcode, layout field or unsafe block. One
original E2E regression covers CV/constant, CV/temporary and
temporary/temporary forms, concrete type order, origin and trace, legal array
union, unchanged compound state and an eval-time constant declaration; all 21
operator E2E tests pass. The generics/JIT replay regression now checks the same
canonical PHP diagnostic while retaining its pre-mutation side-exit invariant.

All five Cargo configurations, all-feature/all-target, formatting, PHPT runner
self-test, unsafe self-test and the exact unsafe diff ratchet pass, as do
Composer S0, all four Symfony S1 gates and exact PHP 8.5.9 warmed-kernel S2 and
cold-build S3. The production inventory remains 1,616 unsafe blocks and 289
unsafe functions. Twenty-one alternating CPU-pinned default-release pairs of
the ten-million-iteration mixed arithmetic control measured baseline
p10/median/p90 1.197174/1.206874/1.248260 seconds and candidate
1.205292/1.230594/1.298811 seconds. The independent median ratio is 1.0197 and
the paired-median ratio is 1.0203, below the +5% gate; paired p10/p90 ratios are
0.9833/1.0746, and every run retained checksum `419,729999927,1`. The exact
base and candidate binary SHA-256 values are
`3866194031f3afeeef53e8932b8b696b0e8e961fa56af6dd0ec402372373689a`
and `aae1523214c7dc649c9c0a59a79e23573c2f4e191896b202a4ddc39ff5540b7f`.
Partial numeric-string arithmetic and unsupported multiplication, division,
exponentiation and the broader operator audit remain separate work.

The latest measured AMD64 PHP 8.5 contract checkpoint is pinned to php-src
8.5.6 commit `fcc29c8` and candidate commit `b0682a74`. Across all 5,599
unmodified `Zend/tests` and `tests/lang` cases, 3,831 pass, 1,468 fail, 115
skip, none remain XFAIL, 185 are unsupported, and none time out or crash. The
headline pass rate is 72.297% and the whole-corpus rate is 68.423%; 4,710 of
5,299 attempted cases reach runtime (88.885%). Relative to exact base
`d85d0528`, the pass-set delta is exactly +4/-0:
`Zend/tests/constexpr/constant_expressions_exceptions_001.phpt`,
`constant_expressions_exceptions_002.phpt`, `Zend/tests/not_002.phpt` and
`Zend/tests/sub_001.phpt` become passes. Every prior pass remains a pass.
`tests/lang/operators/subtract_variationStr.phpt` advances from a compile
failure to a later output mismatch; there are no other status or failure-
category transitions. Two sequential final full runs have byte-identical
merged manifests and summaries. Their SHA-256 values are
`ad7a759279f4e5b038e855bb15bc8892edf754522834ae907b9494c9a66dc21a`
and `741f14fa80d8018ea2115ada9b3d7277b946069068be4124dc9f9cdc1e047545`.

Unsupported binary subtraction now throws a catchable `TypeError` through the
ordinary PHP unwind path instead of terminating with an internal compile-style
fatal. All three emitted subtraction opcode shapes report PHP's
`Unsupported operand types: <left> - <right>` message with concrete diagnostic
types. The operation attaches its source file, line and live trace before
unwinding; compound assignment commits no write when subtraction fails. A
failing global constant expression is consequently catchable across `require`
or `eval`, and the constant remains undefined for a later attempt.

The compiler adds source provenance only to subtraction compound assignments
and explicit returned subtraction expressions. The shared cold operator throw
helper otherwise reuses the nearest located consumer, preceding expression or
callable declaration without enlarging instructions or `OpArray`; this also
supplies the previously missing uncaught origin for bitwise-not's existing
`TypeError`, producing the adjacent `not_002.phpt` pass. One original E2E
regression covers CV/constant, constant/CV and temporary/temporary forms,
exact type order, origin and trace, unchanged compound state and an eval-time
constant declaration; all 20 operator E2E tests pass.

All five Cargo configurations, all-feature/all-target, formatting, PHPT runner
self-test, unsafe self-test and the exact unsafe diff ratchet pass, as do
Composer S0, all four Symfony S1 gates and exact PHP 8.5.9 warmed-kernel S2 and
cold-build S3. The production inventory remains 1,616 unsafe blocks and 289
unsafe functions. Twenty-one alternating CPU-pinned default-release pairs of
the ten-million-iteration mixed arithmetic control measured baseline
p10/median/p90 1.124888/1.158695/1.238128 seconds and candidate
1.192392/1.203181/1.224889 seconds. The independent median ratio is 1.0384 and
the paired-median ratio is 1.0377, both below the +5% gate, with identical
`419,729999927,1` checksums. The success branches and runtime layouts are
unchanged; new formatting, trace collection and source lookup occur only after
an unsupported operation. The exact base and candidate binary SHA-256 values
are `7ac4b4e070d721f12115d36473f806633de08a6bdef8b5d1dd9478556cb2746a`
and `3866194031f3afeeef53e8932b8b696b0e8e961fa56af6dd0ec402372373689a`.
Unsupported addition, multiplication, division, exponentiation and the
remaining numeric-string warning/coercion differences are separate work.

The latest measured AMD64 PHP 8.5 contract checkpoint is pinned to php-src
8.5.6 commit `fcc29c8` and candidate commit `3e787cfe`. Across all 5,599
unmodified `Zend/tests` and `tests/lang` cases, 3,827 pass, 1,472 fail, 115
skip, none remain XFAIL, 185 are unsupported, and none time out or crash. The
headline pass rate is 72.221% and the whole-corpus rate is 68.351%; 4,706 of
5,299 attempted cases reach runtime (88.809%). Relative to exact base
`a460345c`, the pass-set delta is exactly +2/-0:
`Zend/tests/constants/bug41633_3.phpt` and
`Zend/tests/constexpr/constant_expressions_self_referencing_array.phpt`
become passes. Every prior pass remains a pass, and there are no other status
or failure-category transitions. Two sequential final full runs have
byte-identical merged manifests and summaries. Their SHA-256 values are
`adc50d5e2238fc34e08530fc55f812771a80f0561dc09d064879195e73722ed0`
and `4605d408b9815cb25ceecbc458c39ea6bf05f7df693dff3d8cf09a234c9d6fa3`.

A lazily reported self-referencing class constant now exposes the source file
and exact AST line of the local cycle edge through `Error::getFile()` and
`Error::getLine()`. Its trace begins at the runtime use site with PHP 8.5's
synthetic `[constant expression]` frame, followed by the existing live frame.
The same rule covers direct reads and `ReflectionClass::getConstant()`, remains
stable across repeated failures and does not force constant evaluation merely
to link a class or invoke a static method.

The implementation recovers the local reference line from the already retained
constant-expression AST and the existing cycle diagnostic; it adds no field to
class-constant metadata. The recursive lookup covers supported nested
expressions and distinguishes an external class reference from the local cycle
edge. Runtime cycle traversal through `constant()` and through a dependent
constant can select a different edge under PHP 8.5.9 and remains a separate
compatibility boundary rather than being generalized by this checkpoint. One
original E2E regression exercises direct, nested-array, mixed-scope, repeated
and Reflection paths; all 98 class-constant E2E tests pass. All five focused
self-reference expectations pass, and the combined 133-case `constants` plus
`constexpr` audit has the same exact +2/-0 delta.

All five Cargo configurations, all-feature/all-target, formatting, PHPT runner
self-test, unsafe self-test and the exact unsafe diff ratchet pass, as do
Composer S0, all four Symfony S1 gates and exact PHP 8.5.9 warmed-kernel S2 and
cold-build S3. The production inventory remains 1,616 unsafe blocks and 289
unsafe functions. No runtime performance or layout gate applies: the new AST
walk and Throwable origin/trace work execute only after an already recorded
self-reference failure, while ordinary and successful constant paths and all
runtime layouts remain unchanged. The exact base and candidate binary SHA-256
values are `ff91b915572d99b4c2641dde593b86d0ce499070aaedecd05a0ae4a8eaa7b422`
and `7ac4b4e070d721f12115d36473f806633de08a6bdef8b5d1dd9478556cb2746a`.

The latest measured AMD64 PHP 8.5 contract checkpoint is pinned to php-src
8.5.6 commit `fcc29c8` and candidate commit `a460345c`. Across all 5,599
unmodified `Zend/tests` and `tests/lang` cases, 3,825 pass, 1,474 fail, 115
skip, none remain XFAIL, 185 are unsupported, and none time out or crash. The
headline pass rate is 72.183% and the whole-corpus rate is 68.316%; 4,706 of
5,299 attempted cases reach runtime (88.809%). Relative to exact base
`5254b59a`, the pass-set delta is exactly +3/-0:
`Zend/tests/constants/bug41633_2.phpt`,
`Zend/tests/constexpr/gh7771_1.phpt` and `gh7771_2.phpt` become passes. Every
prior pass remains a pass, and there are no other status or failure-category
transitions. Two sequential final full runs have byte-identical merged
manifests and summaries. Their SHA-256 values are
`ddde5b05c3958b85ca859fb7c3df90bb66e85b424c41be901f8146bd4ce1767c`
and `9f8aec906679e834f7eee1f792c24ef99c0b7c50696db66842f73bf6a1bb260a`.

A failed deferred class-constant expression now retains the exact source unit
and AST line of its innermost unresolved class-constant reference. The created
`Error` exposes that declaration location through `getFile()`/`getLine()`,
while its trace begins at the runtime use site with PHP 8.5's synthetic
`[constant expression]` frame. Existing live frames follow in order, including
the native `constant()` or Reflection method frame where applicable, and the
snapshot honors `zend.exception_ignore_args`. Nested deferred definitions keep
the innermost already-located failure rather than replacing it with an outer
dependency line.

The rule is shared by direct class-constant fetches, ordinary object
construction, `constant()`, `ReflectionClass::getConstant()` and
`ReflectionClass::newInstanceWithoutConstructor()`. It runs only after a
deferred evaluation has failed; unlocated and typed-constant errors retain
their established metadata path. One original E2E regression exercises all
five entry points, repeated retryable failures, declaration-versus-use lines
and native frame ordering; all 97 class-constant E2E tests pass. The complete
38-case `Zend/tests/constexpr` audit rises from 12 to 14 exact passes with no
other movement. An adjacent audit of every current upstream expectation that
prints `[constant expression]` finds the additional `bug41633_2.phpt` pass and
no regression.

All five Cargo configurations, all-feature/all-target, formatting, PHPT runner
self-test, unsafe self-test and the exact unsafe diff ratchet pass, as do
Composer S0, all four Symfony S1 gates and exact PHP 8.5 warmed-kernel S2 and
cold-build S3. Two new, documented unsafe blocks snapshot live call frames on
this cold diagnostic path; the production inventory is 1,616 unsafe blocks
and 289 unsafe functions, both within the repository ratchet. No runtime
performance or layout gate applies: ordinary and successful deferred reads do
not allocate diagnostic metadata, and Throwable origin/trace work begins only
after an evaluation failure. The exact base and candidate binary SHA-256 values
are `3dffa5f665c2f1ce9a87560505668950143ce370b411486c83d23010dd47bdb6`
and `ff91b915572d99b4c2641dde593b86d0ce499070aaedecd05a0ae4a8eaa7b422`.

The latest measured AMD64 PHP 8.5 contract checkpoint is pinned to php-src
8.5.6 commit `fcc29c8` and candidate commit `5254b59a`. Across all 5,599
unmodified `Zend/tests` and `tests/lang` cases, 3,822 pass, 1,477 fail, 115
skip, none remain XFAIL, 185 are unsupported, and none time out or crash. The
headline pass rate is 72.127% and the whole-corpus rate is 68.262%; 4,706 of
5,299 attempted cases reach runtime (88.809%). Relative to exact base
`51c111c4`, the pass-set delta is exactly +1/-0:
`Zend/tests/type_declarations/typed_class_constants_ast_print.phpt` becomes a
pass, every prior pass remains a pass, and there are no other status or
failure-category transitions. Two sequential final full runs have byte-
identical merged manifests and summaries. Their SHA-256 values are
`602606b103fec01c92108143dbcda749ab10b3a3d960625f4212474216fdd756`
and `15876d51a2cc16eb0fe7ae1a5f0fca2a76b920e3b5cbe9b0a9dd970f44622436`.

The existing compile-time source synthesizer for one-argument `assert()` now
renders separately declared class constants inside supported named and
anonymous class bodies. It emits PHP 8.5's canonical visibility, optional type,
name, renderable value, semicolon and indentation; implicit visibility becomes
`public`, DNF type parentheses are omitted in the synthesized AST text, and
PHP's assertion printer does not retain a class constant's `final` flag. This
text is only the default `AssertionError` description: the class body remains
short-circuited and is not evaluated to construct it.

The renderer deliberately declines synthesis when the current AST cannot
reconstruct exact source grouping or order: mixed property/constant bodies,
same-line comma-grouped constants and attributed constants remain unsupported,
as do methods, property hooks and attribute-expression AST families. One
original E2E regression covers anonymous and named classes, explicit and
implicit visibility, `final`, scalar and DNF types, literal values and
multiline formatting; all 132 standard-library E2E tests pass. The complete
496-case `Zend/tests/type_declarations` audit rises from 408 to 409 exact
passes with no other movement, and its 29 typed-class-constant cases are now
all exact.

All five Cargo configurations, all-feature/all-target, formatting, PHPT runner
self-test, unsafe self-test and the exact unsafe ratchet pass, as do Composer
S0, all four Symfony S1 gates and exact PHP 8.5 warmed-kernel S2 and cold-build
S3. The production inventory remains 1,614 unsafe blocks and 289 unsafe
functions. No runtime performance or layout gate applies: the change is
confined to compilation of a one-argument assertion whose supported expression
needs a synthesized default description; ordinary compilation, assertion
execution, class linking and object execution paths are unchanged. The exact
base and candidate binary SHA-256 values are
`3d250b83d567944b80ccbf282deb07f86ff0978756794dd804ddb0542f803540`
and `3dffa5f665c2f1ce9a87560505668950143ce370b411486c83d23010dd47bdb6`.

In the preceding deferred class-constant activation checkpoint, the measured
AMD64 PHP 8.5 contract was pinned to php-src
8.5.6 commit `fcc29c8` and candidate commit `51c111c4`. Across all 5,599
unmodified `Zend/tests` and `tests/lang` cases, 3,821 pass, 1,478 fail, 115
skip, none remain XFAIL, 185 are unsupported, and none time out or crash. The
headline pass rate is 72.108% and the whole-corpus rate is 68.244%; 4,706 of
5,299 attempted cases reach runtime (88.809%). Relative to exact base
`f8e4233d`, the pass-set delta is +2/-0: every prior pass remains a pass.
`Zend/tests/constants/gh10709.phpt` and
`Zend/tests/type_declarations/typed_class_constants_diamond_error1.phpt`
become exact. Two existing failures, `Zend/tests/constexpr/gh7771_1.phpt` and
`gh7771_2.phpt`, advance from silent output mismatches to runtime diagnostics;
they still fail only because their constant-expression source origin and stack
frame are not yet retained. There are no other status or failure-category
transitions. Two sequential final full runs have byte-identical merged
manifests and summaries. Their SHA-256 values are
`41d96379d8933289cca64b8549d9a7701b2e3bab9d74ea53548c0ab19113c44e`
and `4fcb5d20f2f42841b366c085ca20c20f9ad8321c3c41662eec130024a9f499c7`.

An ordinary `new` now activates every inherited or locally declared deferred
class constant before property defaults and object storage are materialized.
This gives a recursive `define('C', new A())` expression PHP's catchable
`Undefined constant "C"` ordering, lets autoload publish a dependency during
reentrant constant evaluation, and validates a newly available value against
its typed class-constant declaration. Failed activation stays pending, so a
later definition can make a repeated allocation succeed while an incompatible
typed value raises the same `TypeError` on every attempt. Interface, abstract
class and enum instantiability errors retain precedence. Class lookup and
static method calls remain non-activating, while
`ReflectionClass::newInstanceWithoutConstructor()` follows the same activation
rule as ordinary construction.

The implementation registers only classes with deferred constants in an
optional request-local class-ID sidecar after inheritance, interface and trait
constant linking. Successful activation changes that sidecar state without
enlarging class, class-constant or object layouts; requests containing only
ordinary constants never allocate it. Two original E2E regressions cover the
recursive `define()` ordering, missing and mistyped retry behavior, inherited
activation, non-eager boundaries, abstract precedence and Reflection. An older
class-constant covariance fixture was corrected to remove a recursive object
construction that PHP 8.5 also rejects under this rule. All 96 class-constant
E2E tests pass. The complete 496-case `Zend/tests/type_declarations` audit
rises from 407 to 408 exact passes with no other movement. The 29-case typed-
class-constant slice rises from 27 to 28 passes; only the separate assertion-
AST rendering case remains.

All five Cargo configurations, all-feature/all-target, formatting, PHPT runner
self-test, unsafe self-test and the exact unsafe ratchet pass, as do Composer
S0, all four Symfony S1 gates and exact PHP 8.5 warmed-kernel S2 and cold-build
S3. The production inventory remains 1,614 unsafe blocks and 289 unsafe
functions. Thirty-one CPU-pinned alternating AMD64 default-release pairs, with
no outlier removal, put five million ordinary declared-object allocations at a
1.0217 candidate/control independent-median ratio and 1.0234 paired-median
ratio. The same allocation count after successful deferred-constant activation
measures 1.0244 independently and 1.0245 paired. Paired p90 ratios are 1.0504
and 1.0513 respectively; both median regression gates remain below the five-
percent ceiling with exact checksums `85000000` and `85000000:23`. The exact
base and candidate binary SHA-256 values are
`012a8b4eb84ce1f06ab9e251a75764e3fe8316c5e8f189b74536279d3d5c8daf`
and `3d250b83d567944b80ccbf282deb07f86ff0978756794dd804ddb0542f803540`.
Other object-producing APIs, assertion AST rendering and exact deferred-
constant expression origin frames remain separate compatibility boundaries.

In the preceding typed class-constant value checkpoint, the measured AMD64 PHP
8.5 contract was pinned to php-src 8.5.6 commit `fcc29c8` and candidate commit
`f8e4233d`. Across all 5,599 unmodified `Zend/tests` and `tests/lang` cases,
3,819 pass, 1,480 fail, 115 skip, none remain XFAIL, 185 are unsupported, and
none time out or crash. The headline pass rate is 72.070% and the whole-corpus
rate is 68.209%; 4,706 of 5,299 attempted cases reach runtime (88.809%).
Relative to exact base `3f05d6a8`, the pass-set delta is +13/-0: every prior
pass remains a pass and there are no other status or failure-category
transitions. Two sequential final full runs have byte-identical merged
manifests and summaries. Their SHA-256 values are
`f3f8d521430862d54b4cd8b887f35e270ae0989ddfd7c2fe3b496d953ceb047f`
and `9ccd25053233fe0b455992e7f398c731ea88f37193fe924eda7ba9e2f23a7a8a`.

Typed class constants now validate eager scalar and enum-case values during
declaration compilation and runtime-published values when their dependency is
first read. The runtime rule is strict, with PHP's sole integer-to-float
widening, and a dependent constant propagates the originating declaration's
retryable `TypeError` instead of being mistaken for a self-reference cycle.
Declaration diagnostics identify the class, constant, declared type and source
line; forbidden `callable`, `void` and `never` types use their PHP 8.5 forms.
Parenthesized DNF declarations are accepted, `static` remains late-bound in
runtime diagnostics, and inherited constant types use real class/interface
subtyping across nullable, union and intersection forms.

Three original E2E regressions cover eager and forbidden-type diagnostics,
repeated deferred scalar/object failures, dependency-origin propagation,
integer-to-float widening, late-static enum validation and class/DNF
covariance; all 94 class-constant E2E tests pass. The complete 496-case
`Zend/tests/type_declarations` audit rises from 394 to 407 exact passes with no
loss or other movement. The 29-case typed-class-constant slice rises from 14 to
27 passes; only the separate assertion-AST rendering and diamond dependency-
ordering cases remain.

All five Cargo configurations, all-feature/all-target, formatting, PHPT runner
self-test, unsafe self-test and the exact unsafe ratchet pass, as do Composer
S0, all four Symfony S1 gates and exact PHP 8.5 warmed-kernel S2 and cold-build
S3. The production inventory remains 1,614 unsafe blocks and 289 unsafe
functions. Thirty-one CPU-pinned alternating AMD64 default-release pairs put
five million dynamic typed class-constant reads at a 0.9767 candidate/control
median ratio and 1.0114 p90 paired ratio with the identical `85000000`
checksum, below the five-percent ceiling. The ordinary cached read path and
runtime constant layout are unchanged; the added work is confined to parsing,
cold compilation/linking and explicitly deferred evaluation. The exact base
and candidate binary SHA-256 values are
`6b9247121c2fbc9eb04338c59a765ccc928f3eef3f672ffd688d7925104cb58a`
and `012a8b4eb84ce1f06ab9e251a75764e3fe8316c5e8f189b74536279d3d5c8daf`.

In the preceding enum Reflection-constant checkpoint, the measured AMD64 PHP
8.5 contract was pinned to php-src
8.5.6 commit `fcc29c8` and candidate commit `3f05d6a8`. Across all 5,599
unmodified `Zend/tests` and `tests/lang` cases, 3,806 pass, 1,493 fail, 115
skip, none remain XFAIL, 185 are unsupported, and none time out or crash. The
headline pass rate is 71.825% and the whole-corpus rate is 67.976%; 4,706 of
5,299 attempted cases reach runtime (88.809%). Relative to exact base
`def82dfa`, the pass-set delta is +2/-0: every prior pass remains a pass and
there are no other status or failure-category transitions. Two sequential
final full runs have byte-identical merged manifests and summaries. Their
SHA-256 values are
`8b23b13c1c0544052fe1c2a200ebeafdf7dcf7f63203f098b4ea1c73cc7ef233`
and `bab95126e974764355d53182cf058b4a2015a76dbbf202f60728ca16bdae0547`.

User-defined enum cases now participate in the existing class-constant
Reflection surface. `ReflectionClass::getConstants()` lists canonical case
singletons before ordinary constants, `getConstant()` preserves their object
identity, and modifier filtering treats cases as public but not final.
`getReflectionConstants()`, `getReflectionConstant()` and direct
`ReflectionClassConstant` construction expose the canonical declaring enum and
case attributes. Reflection reads each singleton from the existing enum static
storage slot rather than constructing a replacement. Cases remain absent from
`getDefaultProperties()`, matching their constant rather than property role.

One original E2E regression covers namespaced unbacked and string-backed enums,
case ordering, an ordinary enum constant, singleton identity, missing lookups,
public/final filters, reflected objects, case attributes and default-property
exclusion; all 84 Reflection E2E tests pass. Its PHP 8.5 and RPHP outputs are
byte-identical with SHA-256
`fe91346008e49bc476f1f0ac7f6126d974cee115f7ffe2b96bbf2d261e0d206a`.
The complete 356-case adjacent `Zend/tests/attributes` plus `Zend/tests/enum`
audit has only the two full-corpus additions:
`Zend/tests/enum/case-attributes.phpt` and
`Zend/tests/enum/reflectionclass.phpt`. `ReflectionEnum` and its specialized
case classes, the broader `ReflectionClassConstant` method inventory, enum
property-object presentation and refcount debug output remain separate visible
gaps.

All five Cargo configurations, all-feature/all-target, formatting, PHPT runner
self-test, unsafe self-test and the exact unsafe ratchet pass, as do Composer
S0, all four Symfony S1 gates and exact PHP 8.5 warmed-kernel S2 and cold-build
S3. The production inventory remains 1,614 unsafe blocks and 289 unsafe
functions. No separate performance or layout gate applies: the change is
confined to explicitly invoked cold Reflection handlers and reuses existing
enum storage; enum compilation, ordinary case access, dispatch, writes and
runtime layouts are unchanged. The exact base and candidate binary SHA-256
values are
`dd6643ff7c750594aba103d0f9b3c5caa407fec2f1cefb7b988574a51e0be17f`
and `6b9247121c2fbc9eb04338c59a765ccc928f3eef3f672ffd688d7925104cb58a`.

In the preceding user-enum Reflection-string checkpoint, the measured AMD64
PHP 8.5 contract was pinned to php-src
8.5.6 commit `fcc29c8` and candidate commit `def82dfa`. Across all 5,599
unmodified `Zend/tests` and `tests/lang` cases, 3,804 pass, 1,495 fail, 115
skip, none remain XFAIL, 185 are unsupported, and none time out or crash. The
headline pass rate is 71.787% and the whole-corpus rate is 67.941%; 4,706 of
5,299 attempted cases reach runtime (88.809%). Relative to exact base
`63f1cffe`, the pass-set delta is +1/-0: every prior pass remains a pass and
there are no other status or failure-category transitions. Two sequential
final full runs have byte-identical merged manifests and summaries. Their
SHA-256 values are
`186489973887cf5937a606c5c12968598e16de0d787a8eaa187363f2846219c4`
and `3bab12800e28bef38f2dbd8d74629c0c78f0d850216246fdb77dc778edeb87e6`.

`ReflectionClass::__toString()` now renders PHP 8.5's implicit metadata for
user-defined enums. Enum titles omit the implementation-only `final` modifier,
include backing types and declared plus implicit interfaces, and list cases in
their own section rather than as static properties. Implicit `cases()`,
`from()` and `tryFrom()` methods expose their internal `UnitEnum` or
`BackedEnum` prototypes, canonical parameter and return signatures, and PHP
ordering. The implicit `name` and backed `value` properties render as public
`protected(set)` readonly properties. Runtime write metadata is deliberately
unchanged so PHP's established enum-readonly diagnostics retain precedence.

One original E2E regression covers unbacked and string-backed enums, a custom
interface, cases, implicit methods, signatures, ordering and both implicit
properties; all 83 Reflection E2E tests pass. Minimal unbacked and backed
renderings match PHP 8.5 byte-for-byte. The complete 356-case adjacent
`Zend/tests/attributes` plus `Zend/tests/enum` audit has exactly one movement:
`Zend/tests/attributes/delayed_target_validation/validator_Deprecated.phpt`
becomes the full-corpus addition. In particular, all four enum property-write
controls remain passes and internal-enum rendering remains unchanged.
User-declared method source/signature formatting and PHP extension-qualified
internal-enum rendering remain separate visible gaps.

All five Cargo configurations, all-feature/all-target, formatting, PHPT runner
self-test, unsafe self-test and the exact unsafe ratchet pass, as do Composer
S0, all four Symfony S1 gates and exact PHP 8.5 warmed-kernel S2 and cold-build
S3. The production inventory remains 1,614 unsafe blocks and 289 unsafe
functions. No separate performance or layout gate applies: all added work is
confined to explicit cold Reflection stringification, while enum compilation,
storage, dispatch, writes and object layout are unchanged. The exact base and
candidate binary SHA-256 values are
`094a285dc58b6f7c5d698e6ac942393c2a6b429ab9353f50852dfb203a98c9fc`
and `dd6643ff7c750594aba103d0f9b3c5caa407fec2f1cefb7b988574a51e0be17f`.

In the preceding Deprecated-attribute checkpoint, the measured AMD64 PHP 8.5
contract was pinned to php-src
8.5.6 commit `fcc29c8` and candidate commit `63f1cffe`. Across all 5,599
unmodified `Zend/tests` and `tests/lang` cases, 3,803 pass, 1,496 fail, 115
skip, none remain XFAIL, 185 are unsupported, and none time out or crash. The
headline pass rate is 71.768% and the whole-corpus rate is 67.923%; 4,706 of
5,299 attempted cases reach runtime (88.809%). Relative to exact base
`4a1aaebe`, the pass-set delta is +3/-0: every prior pass remains a pass. The
only other failure-category transition is
`Zend/tests/attributes/delayed_target_validation/validator_Deprecated.phpt`,
which advances from the former runtime validation failure to its independent
Reflection-enum output mismatch and remains a visible failure. Two sequential
final full runs have byte-identical merged manifests and summaries. Their
SHA-256 values are
`50ad4604c8b9b010c44168b1c253618a251e2661f500a6bd802ca407283c3d04`
and `17bf0d9215c0b1511dea6ef47cbf18a41c551e2080d2a1514aa81f1b5cdbb9c0`.

The built-in `Deprecated` attribute now enforces its PHP 8.5 declaration
contract during compilation. It accepts functions, methods, constants, class
constants and traits, but rejects properties, parameters and enum cases with
the canonical allowed-target diagnostic. Ordinary and anonymous classes,
interfaces and enums retain their distinct `Cannot apply #[\Deprecated]`
diagnostics. Namespace and import resolution distinguish the built-in from a
same-named user attribute. `Deprecated` is non-repeatable;
`DelayedTargetValidation` defers target and class-form checks while still
rejecting repetition. Reflection instantiation performs those deferred
class-form checks against the declaration owner, rejecting delayed classes,
interfaces and enums while allowing traits.

One original E2E regression covers compile-time targets and repetition, valid
traits, all delayed forms, local names and delayed Reflection instantiation;
all 82 Reflection E2E tests pass. The complete 204-case adjacent
`Zend/tests/attributes` run loses no prior pass, and all 47 existing
`attributes/deprecated` controls remain green. The three exact full-corpus
additions are
`Zend/tests/attributes/constants/constant_listed_as_target-internal.phpt`,
`constants/not_repeatable-internal.phpt` and
`delayed_target_validation/with_Deprecated.phpt`. The delayed validator test
now emits the expected `Deprecated` errors for its class, interface and enum;
its remaining mismatch is the separately visible Reflection rendering of an
enum and internal enum cases.

All five Cargo configurations, all-feature/all-target, formatting, PHPT runner
self-test, unsafe self-test and the exact unsafe ratchet pass, as do Composer
S0, all four Symfony S1 gates and exact PHP 8.5 warmed-kernel S2 and cold-build
S3. The production inventory remains 1,614 unsafe blocks and 289 unsafe
functions. On CPU 2 with the performance governor, four warmups per binary and
32 balanced ABBA/BAAB groups with no removed observations, compilation of
1,000 ordinary typed function declarations retains output `ok`. The 64
observations per binary measure baseline p10/median/p90
0.183092/0.185286/0.187303 seconds and candidate
0.182780/0.185470/0.188614 seconds: +0.099% independently and +0.177% by the
paired-group median, whose p10/p90 is -0.863%/+1.785%. Both medians remain
below the five-percent regression ceiling; no speedup is claimed. The exact
base and candidate binary SHA-256 values are
`b29f58f9d5dbd3ddd86e9f35b74d4476462b559964e9113d1b4f07a0661cc814`
and `094a285dc58b6f7c5d698e6ac942393c2a6b429ab9353f50852dfb203a98c9fc`.

In the preceding Attribute-target checkpoint, the measured AMD64 PHP 8.5
contract was pinned to php-src
8.5.6 commit `fcc29c8` and candidate commit `4a1aaebe`. Across all 5,599
unmodified `Zend/tests` and `tests/lang` cases, 3,800 pass, 1,499 fail, 115
skip, none remain XFAIL, 185 are unsupported, and none time out or crash. The
headline pass rate is 71.712% and the whole-corpus rate is 67.869%; 4,706 of
5,299 attempted cases reach runtime (88.809%). Relative to exact base
`8b37e107`, the pass-set delta is +8/-0: every prior pass remains a pass and
there are no other status or failure-category transitions. Two sequential
final full runs have byte-identical merged manifests and summaries. Their
SHA-256 values are
`dcef4f1eb518f48dbf36e564817d574b9a09f404fbe6918d54283ee292b189b4`
and `06e1255b8b913f10a802b921e91061382a4df8dbacaf50d274a40d547899478c`.

The built-in `Attribute` meta-attribute now enforces its PHP 8.5 declaration
contract during compilation. It can target a concrete ordinary, final,
readonly or anonymous class, but rejects functions, constants, methods,
properties, parameters, class constants and enum cases. Abstract classes,
interfaces, traits and enums retain their distinct `Cannot apply
#[\Attribute]` diagnostics. Namespace and import resolution distinguish the
built-in from a same-named user attribute. `Attribute` is non-repeatable;
`DelayedTargetValidation` defers target and class-form validation while still
rejecting repetition, matching PHP's validation order.

One original E2E regression covers every declaration target, aliased and local
names, valid class forms, repetition and delayed validation. The complete
204-case adjacent `Zend/tests/attributes` run has no lost prior pass or
unrelated transition. The eight exact
full-corpus additions are `Zend/tests/attributes/008_wrong_attribution.phpt`,
`024_internal_target_validation.phpt`,
`025_internal_repeatable_validation.phpt`, the four
`attributes/Attribute/Attribute_on_*.phpt` cases, and
`attributes/constants/must_target_const-internal.phpt`. Reflection-time
validation of user-defined attributes and delayed validator instantiation is
unchanged and remains outside this compile-time checkpoint.

All five Cargo configurations, all-feature/all-target, formatting, PHPT runner
self-test, unsafe self-test and the exact unsafe ratchet pass, as do Composer
S0, all four Symfony S1 gates and exact PHP 8.5 warmed-kernel S2 and cold-build
S3. The production inventory remains 1,614 unsafe blocks and 289 unsafe
functions. On CPU 2 with the performance governor, four warmups per binary and
32 balanced ABBA/BAAB groups with no removed observations, compilation of
1,000 ordinary typed function declarations retains output `ok`. The 64
observations per binary measure baseline p10/median/p90
0.182858/0.184534/0.187294 seconds and candidate
0.183150/0.184973/0.186925 seconds: +0.238% independently and -0.063% by the
paired-group median, whose p10/p90 is -0.881%/+1.272%. Both medians remain
below the five-percent regression ceiling; no speedup is claimed. The exact
base and candidate binary SHA-256 values are
`8fba18dc89b1e5ba4b17ff13b79efbc7cf4692817ea71fbdf3b046516d81fdaa`
and `b29f58f9d5dbd3ddd86e9f35b74d4476462b559964e9113d1b4f07a0661cc814`.

In the preceding relative-type-declaration checkpoint, the measured AMD64 PHP
8.5 contract was pinned to php-src 8.5.6 commit `fcc29c8` and candidate commit
`8b37e107`. Across all 5,599
unmodified `Zend/tests` and `tests/lang` cases, 3,792 pass, 1,507 fail, 115
skip, none remain XFAIL, 185 are unsupported, and none time out or crash. The
headline pass rate is 71.561% and the whole-corpus rate is 67.726%; 4,706 of
5,299 attempted cases reach runtime (88.809%). Relative to exact base
`ef9b1f45`, the pass-set delta is +18/-0: every prior pass remains a pass and
there are no other status or failure-category transitions. Two sequential
final full runs have byte-identical merged manifests and summaries. Their
SHA-256 values are
`f756206beb91051bf9cf497396308ea151e3831779c2262ddbed6df3b8f4db69`
and `5f810dfaea28dd6f2924f5807c29d9955c39d65d0f69c33b3a0df6c18c500add`.

Declared relative types now follow PHP 8.5's compile-time scope and diagnostic
rules. Global `self` and `parent` declarations fail when no class scope is
active, and `parent` in a class or interface without a parent reports that
specific condition. Resolvable class/interface `self` and `parent` types remain
valid. Intersections reject those relative names only where a closure or trait
leaves their scope late-bound. Parameter `static` retains PHP's compile-fatal
modifier diagnostic, while nullable parameter/property `?static` remains a
located parse error. Namespace-relative and fully qualified built-in names,
qualified reserved names, and fully qualified `self`/`parent` now retain their
distinct PHP diagnostics and source case.

One parser regression and four original E2E regressions cover diagnostic stage
and location, resolvable versus late-bound relative scopes, qualified reserved
names, and parameter/property `static` forms. The 18 exact full-corpus additions
are `Zend/tests/ctor_promotion/ctor_promotion_additional_modifiers.phpt`,
`Zend/tests/return_types/024.phpt`, `return_types/026.phpt`, the four
`Zend/tests/type_declarations/intersection_types/relative_types/relative_*`
cases, `relative_types/invalid_types/parent_global_function.phpt`,
`parent_interface.phpt`, `self_global_function.phpt`,
`scalar_relative_typehint_disallowed.phpt`, `static_type_param.phpt`,
`static_type_property.phpt`, `variance/parent_in_class_failure1.phpt`, and
`Zend/tests/typehints/bug43332_2.phpt`, `bug76198.phpt`,
`fully_qualified_scalar.phpt` and `namespace_relative_scalar.phpt`. Runtime type
enforcement and generated bytecode are unchanged; unrelated variance, generic
and class-contract gaps remain visible rather than being claimed here.

All five Cargo configurations, all-feature/all-target, formatting, PHPT runner
self-test, unsafe self-test and the exact unsafe ratchet pass, as do Composer
S0, all four Symfony S1 gates and exact PHP 8.5 warmed-kernel S2 and cold-build
S3. The production inventory remains 1,614 unsafe blocks and 289 unsafe
functions. On CPU 2 with the performance governor, four warmups per binary and
32 balanced ABBA/BAAB groups with no removed observations, compilation of
1,000 ordinary typed function declarations retains output `ok`. The 64
observations per binary measure baseline p10/median/p90
0.183166/0.184660/0.188321 seconds and candidate
0.182849/0.184561/0.187283 seconds: -0.054% independently and +0.030% by the
paired-group median, whose p10/p90 is -1.377%/+1.469%. Both medians remain
below the five-percent regression ceiling; no speedup is claimed. The exact
base and candidate binary SHA-256 values are
`752aabc6caa6d10c2d23a31e96b2dc978fb21675307ae1b2b58100c7e0c323f8`
and `8fba18dc89b1e5ba4b17ff13b79efbc7cf4692817ea71fbdf3b046516d81fdaa`.

In the preceding relative-static-scope checkpoint, the measured AMD64 PHP 8.5
contract was pinned to php-src 8.5.6 commit `fcc29c8` and candidate commit
`ef9b1f45`. Across all 5,599
unmodified `Zend/tests` and `tests/lang` cases, 3,774 pass, 1,525 fail, 115
skip, none remain XFAIL, 185 are unsupported, and none time out or crash. The
headline pass rate is 71.221% and the whole-corpus rate is 67.405%; 4,716 of
5,299 attempted cases reach runtime (88.998%). Relative to exact base
`0ecb4823`, the pass-set delta is +16/-0: every prior pass remains a pass and
there are no other status or failure-category transitions. Two sequential
final full runs have byte-identical merged manifests and summaries. Their
SHA-256 values are
`4c28e3ed8473f41ba99d726988922e6c36f5718958784766546bfcb761efd89e`
and `6d0bac7bb9c2a17e0718bf14ce67568f162d2488d58936a04a9820486568dada`.

Relative `self`, `parent` and `static` operations now follow PHP 8.5's active
class-scope rules instead of being rejected uniformly by the parser. At top
level, member operations and `new static` raise catchable `Error` objects with
the operation-specific diagnostic and source origin; the same syntax inside an
unscoped named function remains a compile fatal. A closure declared globally
may acquire its relative scope through `Closure::bindTo()` or `call()`, while a
class-declared closure keeps lexical `self`/`parent` and forwards its late
`static` called class. Relative closure return types are admitted, invalid
global named-function return types retain the compile stage, and a failed late-
static return check names the resolved called class. Constant-expression forms
retain their distinct forbidden-`static` diagnostics.

One parser regression and eight original E2E regressions cover top-level and
named-function failures, bound global closures, lexical versus late-static
class closures, resolved return diagnostics, source location and three
constant-expression diagnostic families. The 16 exact full-corpus additions
are `Zend/tests/bug70918.phpt`, `Zend/tests/class_name/bug66811.phpt`,
`class_name_as_scalar_error_001.phpt`, `class_name_as_scalar_error_005.phpt`,
`class_name_as_scalar_error_006.phpt`, `class_name_as_scalar_error_007.phpt`,
`Zend/tests/closures/bug70987.phpt`,
`Zend/tests/constants/dynamic_class_const_fetch_cache_slot.phpt`,
`Zend/tests/constexpr/constant_expressions_static_class_name_error.phpt`,
`Zend/tests/first_class_callable/constexpr/error_dynamic_004.phpt`,
`Zend/tests/self_class_const_outside_class.phpt`,
`Zend/tests/type_declarations/relative_types/invalid_types/static_global_function.phpt`,
`relative_types/relative_type_in_closures.phpt`,
`static_type_outside_class.phpt`, `static_type_return.phpt` and
`static_type_trait.phpt`. Named functions do not become dynamically bindable,
lexical magic constants are unchanged, and unrelated callable, trait and
Reflection gaps remain visible rather than being claimed by this checkpoint.

All five Cargo configurations, all-feature/all-target, formatting, PHPT runner
self-test, unsafe self-test and the exact unsafe ratchet pass, as do Composer
S0, all four Symfony S1 gates and exact PHP 8.5 warmed-kernel S2 and cold-build
S3. The production inventory is 1,614 unsafe blocks and 289 unsafe functions.
On CPU 2 with the performance governor, four warmups per binary and 32 balanced
ABBA/BAAB groups with no removed observations, compilation of 10,000 ordinary
expressions in a dead branch retains output `ok`. The 64 observations per
binary measure baseline p10/median/p90
0.160889/0.161643/0.164149 seconds and candidate
0.160688/0.161697/0.164493 seconds: +0.034% independently and +0.091% by the
paired-ratio median, whose p10/p90 is -1.172%/+1.112%. Both medians remain
below the five-percent regression ceiling; no speedup is claimed. The exact
base and candidate binary SHA-256 values are
`f565434ee3a6528a50158819988015e0a361b95faca13e91720aa8f1e01a4847`
and `752aabc6caa6d10c2d23a31e96b2dc978fb21675307ae1b2b58100c7e0c323f8`.

In the preceding yield-from-Traversable checkpoint, the measured AMD64 PHP
8.5 contract was pinned to php-src 8.5.6 commit `fcc29c8` and candidate commit
`0ecb4823`. Across all 5,599
unmodified `Zend/tests` and `tests/lang` cases, 3,758 pass, 1,541 fail, 115
skip, none remain XFAIL, 185 are unsupported, and none time out or crash. The
headline pass rate is 70.919% and the whole-corpus rate is 67.119%; 4,709 of
5,299 attempted cases reach runtime (88.866%). Relative to exact base
`8e41909f`, the pass-set delta is +6/-0: every prior pass remains a pass. The
only other category transition is `Zend/tests/generators/bug71013.phpt`, which
advances from the former unsupported-delegate fatal to its independent
destructor-order output mismatch and remains a visible failure. Two sequential
final full runs have byte-identical merged manifests and summaries. Their
SHA-256 values are
`9705084126b027adf0ccc8ceaee95dbcfbde4039829c72094e34d0dadbf35c4e`
and `ce6b111d6cf07d10197fab995085b859e0bf8678f323bc72481f69bbe278f3b3`.

`yield from` now accepts the full core Traversable boundary rather than only
arrays and direct Generator objects. User Iterator delegates advance lazily in
PHP's `rewind`, `valid`, `current`, `key`, then `next` order, preserve yielded
keys, ignore sent values, and inject a supplied exception back into the parent
generator. IteratorAggregate is resolved with cycle and invalid-result checks.
When its iterator is a Generator, the existing iterative delegation engine is
reused while applying Iterator semantics: sent values are not forwarded,
throws re-enter the parent, the inner return value is discarded, and already
advanced or closed generators fail at the rewind boundary. Direct
`yield from Generator` keeps its existing send, throw and return-value
forwarding. Core ArrayIterator-family values, including subclasses, use their
registered iterable storage, and a retained user Iterator participates in the
request-local generator cycle graph.

Eight original E2E regressions cover lazy protocol order and keys, null
Traversable returns, Iterator and IteratorAggregate send/throw boundaries,
protocol exceptions, invalid aggregate results, Generator iterator lifecycle,
and an ArrayIterator subclass. The six exact full-corpus additions are
`Zend/tests/generators/gh15275-003.phpt`, `gh15275-004.phpt`,
`gh15275-005.phpt`, `yield_from_iterator.phpt`,
`yield_from_iterator_agregate.phpt` and `yield_from_valid_exception.phpt`.
The remaining generator/fiber cases that previously stopped at the same
missing-delegate fatal now expose their separate fiber, GC or destructor
lifecycle gaps; this checkpoint does not claim those contracts, live mutation
of the current built-in iterator snapshot, or by-reference delegation.

All five Cargo configurations, all-feature/all-target, formatting, PHPT runner
self-test, unsafe self-test and the exact unsafe ratchet pass, as do Composer
S0, all four Symfony S1 gates and exact PHP 8.5 warmed-kernel S2 and cold-build
S3. Refactoring the duplicated suspension path reduces the production
inventory from 1,618 to 1,613 unsafe blocks while retaining 289 unsafe
functions. Two independent CPU-2, four-warmup, 200-pair order-balanced direct
generator-resume gates retain checksum `19999900000` and measure +0.922% and
+0.720%, both below the +1% regression ceiling. No speedup is claimed. The
exact base and candidate binary SHA-256 values are
`3491dac6f4901f6d79794b677c8e34f55d371217ce7c63e6e0f9c29fbee37be5`
and `f565434ee3a6528a50158819988015e0a361b95faca13e91720aa8f1e01a4847`.

In the preceding empty-constant-name checkpoint, the measured AMD64 PHP 8.5
contract was pinned to php-src 8.5.6 commit `fcc29c8` and candidate commit
`8e41909f`. Across all 5,599
unmodified `Zend/tests` and `tests/lang` cases, 3,752 pass, 1,547 fail, 115
skip, none remain XFAIL, 185 are unsupported, and none time out or crash. The
headline pass rate is 70.806% and the whole-corpus rate is 67.012%; 4,709 of
5,299 attempted cases reach runtime (88.866%). Relative to exact base
`e9e20beb`, the pass-set delta is +1/-0: all 3,751 prior passes remain passes,
and no other status or failure category changes. Two final merged manifests
and summaries from sequential full runs are byte-for-byte identical. Their
SHA-256 values are
`727e79637bcaffa5e060c234c1ebc8c8e4e8fe74f0d5782f0590bb8d79d234ae`
and `0f839828f17fb9868a44c3afb1916dea74e8c01cc555126fd8c0c19f51a75c9c`.

An empty dynamic constant name is now valid: `define('', $value)` succeeds,
`defined('')` and `constant('')` observe it, and a repeated definition emits
the ordinary PHP 8.5 redefinition warning while preserving the first value.
`define()` now also uses the existing strict internal-string boundary before
weak coercion. In weak mode a null name emits PHP's null-to-string deprecation
and becomes the empty name; under `strict_types=1` the same value raises the
canonical `TypeError` without publishing a constant. Source `const` syntax and
class constants cannot express an empty identifier and are outside this
dynamic-name checkpoint.

Original E2E coverage exercises explicit empty-string creation, lookup,
redefinition and first-value preservation, weak null deprecation/coercion, and
strict null rejection without a side effect. The existing weak integer-name
coercion and invalid composite-name controls remain green. The exact
full-corpus addition is `Zend/tests/constants/constants_001.phpt`, which passes
in 32 consecutive focused runs; `Zend/tests/constants/constants_008.phpt` and
`Zend/tests/constants/008.phpt` remain exact retained controls.

All five Cargo configurations, all-feature/all-target, formatting, PHPT runner
self-test, unsafe self-test and the exact unsafe ratchet pass, as do Composer
S0, all four Symfony S1 gates and exact PHP 8.5 warmed-kernel S2 and cold-build
S3. The production inventory remains 1,618 unsafe blocks and 289 unsafe
functions. On CPU 2 with the performance governor, four warmups per binary and
32 order-balanced measured pairs with no removed observations, 1,000 ordinary
successful `define()` calls move from 0.023857 to 0.023754 seconds (-0.435%
independently, -0.378% paired), with paired p10/p90 of -2.152%/+5.243%.
Checksums match and both decision medians remain below the five-percent
regression ceiling. The exact base and candidate binary SHA-256 values are
`021f46fd72fe1df8f738381457c5b17769a80dddaa00fb6819229f6cef366ef4`
and `3491dac6f4901f6d79794b677c8e34f55d371217ce7c63e6e0f9c29fbee37be5`.

In the preceding constant-redefinition checkpoint, the measured AMD64 PHP 8.5
contract was pinned to php-src 8.5.6 commit `fcc29c8` and candidate commit
`e9e20beb`. Across all 5,599 unmodified `Zend/tests` and `tests/lang` cases,
3,751 pass, 1,548 fail, 115
skip, none remain XFAIL, 185 are unsupported, and none time out or crash. The
headline pass rate is 70.787% and the whole-corpus rate is 66.994%; 4,709 of
5,299 attempted cases reach runtime (88.866%). Relative to exact base
`3c5af617`, the pass-set delta is +7/-0: all 3,744 prior passes remain passes,
and no other status or failure category changes. Two final merged manifests
and summaries are byte-for-byte identical. Their SHA-256 values are
`357f2a087bb6003607236c021eb8c462142950c2804155add60f1778036e7687`
and `cbcf38eae735c99e25e415254023383837327b8c2e6272a4ae598b7cb1f4829b`.

Duplicate global constants declared by source `const` or `define()` now emit
PHP 8.5's `Constant NAME already defined, this will be an error in PHP 9`
warning at the colliding source line, continue execution and preserve the
first case-sensitive value. Within one compilation unit, repeated source
declarations retain the first declaration's attributes and deferred-expression
metadata. The two-pass constant pre-scan also reserves the first source
declaration even when its value must be resolved at runtime, so a statically
evaluable duplicate cannot seed later constant folding. Cold include/eval
metadata merges no longer overwrite metadata already resident in the executor.
Runtime `define()` preceding a later attributed source constant, especially
across source-unit execution order, still needs lazy metadata publication and
is explicitly outside this checkpoint's attribute-ownership claim.

Original E2E coverage exercises exact `define()` warning text, first source
value and attributes, constant-name case sensitivity, diagnostic source lines,
and the runtime-resolved-first/static-duplicate pre-scan boundary. The exact
full-corpus additions are
`Zend/tests/attributes/constants/constant_redefined_addition.phpt`,
`constant_redefined_change.phpt`, `constant_redefined_removal.phpt`,
`Zend/tests/constants/008.phpt`, `constants_004.phpt`, `constants_008.phpt`
and
`Zend/tests/type_declarations/typed_class_constants_inheritance_success4.phpt`;
all seven pass in 32 consecutive focused runs. `Zend/tests/bug29890.phpt`
remains blocked on `set_time_limit()`, `Zend/tests/constants/constants_001.phpt`
on the empty constant name, and `Zend/tests/constants/gh18850.phpt` on
`__COMPILER_HALT_OFFSET__`; all three retain their exact expected failures.

All five Cargo configurations, all-feature/all-target, formatting, PHPT runner
self-test, unsafe self-test and the exact unsafe ratchet pass, as do Composer
S0, all four Symfony S1 gates and exact PHP 8.5 warmed-kernel S2 and cold-build
S3. The production inventory is 1,618 unsafe blocks and 289 unsafe functions.
On CPU 2 with the performance governor, four warmups per binary and 32 order-
balanced measured pairs with no removed observations, 1,000 successful simple
constant declarations move from 0.179274 to 0.177592 seconds (-0.938%
independently, -0.457% paired), with paired p10/p90 of -2.986%/+2.528%.
Checksums match and both medians remain below the five-percent regression
ceiling. The exact base and candidate binary SHA-256 values are
`5c4d998f09a6a0a6f9eeef3ce48fdab25da745564a1f710a4cf93857935ab0ff`
and `021f46fd72fe1df8f738381457c5b17769a80dddaa00fb6819229f6cef366ef4`.

In the preceding function-redeclaration checkpoint, the measured AMD64 PHP 8.5
contract was pinned to php-src 8.5.6 commit `fcc29c8` and candidate commit
`3c5af617`. Across all 5,599 unmodified `Zend/tests` and `tests/lang` cases,
3,744 pass, 1,555 fail, 115
skip, none remain XFAIL, 185 are unsupported, and none time out or crash. The
headline pass rate is 70.655% and the whole-corpus rate is 66.869%; 4,709 of
5,299 attempted cases reach runtime (88.866%). Relative to exact base
`88b7abec`, the pass-set delta is +2/-0: all 3,742 prior passes remain passes,
and no other status or failure category changes. Two final merged manifests
and summaries are byte-for-byte identical. Their SHA-256 values are
`730443687b786f7b062b56cd593ff04eb961cee0e8b6587d7b82a9fabb0964de`
and `be24233922d94d8de61abb714ba0ce75ec330e34ccb5731f2eeaae62885fa4f9`.

User-function redeclarations now use the current declaration's qualified name
and spelling and report both source locations when the previous declaration is
also user-defined. A collision with an internal function omits the unavailable
previous-source clause. The cold formatter reads declaration lines and source
units already retained in each user `OpArray`, so successful registration and
runtime layouts gain no metadata. Include and eval compilation propagate a
duplicate for unconditional top-level, bare-block and namespace declarations;
the lexical scan excludes conditional and nested declarations, whose actual
runtime publication remains separate follow-up work.

Original E2E coverage exercises case-insensitive and namespaced collisions,
the internal `strlen()` boundary, repeated includes with matching first/current
locations, and an inactive conditional declaration across a recursive include.
The exact full-corpus additions are `Zend/tests/function_redecl.phpt` and
`Zend/tests/line_numbers/gh16509.phpt`; both pass in 32 consecutive focused
runs. `Zend/tests/autoload/bug63741.phpt` and `Zend/tests/bug35634.phpt` remain
exact retained controls. Runtime publication of conditional/nested functions,
including the separate lazy-object `gh20905.phpt` boundary, remains follow-up
work.

All five Cargo configurations, all-feature/all-target, formatting, PHPT runner
self-test, unsafe self-test and the exact unsafe ratchet pass, as do Composer
S0, all four Symfony S1 gates and exact PHP 8.5 warmed-kernel S2 and cold-build
S3. The production inventory is 1,618 unsafe blocks and 289 unsafe functions.
On CPU 2 with the performance governor, four warmups per binary and 32 order-
balanced measured pairs with no removed observations, 1,000 successful simple
function declarations move from 0.063433 to 0.063284 seconds (-0.234%
independently, -0.192% paired), below the five-percent regression ceiling.
Checksums match. The exact base and candidate binary SHA-256 values are
`751a58e2e023509401708992ed63c7c1f4c3c203ff7b77a40af2433b0a7f980e`
and `5c4d998f09a6a0a6f9eeef3ce48fdab25da745564a1f710a4cf93857935ab0ff`.

In the preceding import/declaration collision checkpoint, the measured AMD64
PHP 8.5 contract was pinned to php-src 8.5.6 commit `fcc29c8` and candidate
commit `88b7abec`. Across all 5,599 unmodified `Zend/tests` and `tests/lang`
cases, 3,742 pass, 1,557 fail, 115 skip, none remain XFAIL, 185 are unsupported,
and none time out or crash. The
headline pass rate is 70.617% and the whole-corpus rate is 66.833%; 4,709 of
5,299 attempted cases reach runtime (88.866%). Relative to exact base
`0e71faea`, the pass-set delta is +15/-0: all 3,727 prior passes remain passes,
and no other status or failure category changes. Two final merged manifests
and summaries are byte-for-byte identical. Their SHA-256 values are
`8f2e97d3c17474e58698db9c383035a5c7987f36d2129e835d109838bedb9a14`
and `efc19c4baecb04d0747283011c0d998753ea41334fc700b8c4bd51b4af48e3e4`.

Class-like, function and constant imports now use distinct collision tables
with PHP's case rules: class-like and function aliases are case-insensitive,
while constant aliases are case-sensitive. Declarations are qualified by their
lexical namespace without being rewritten through an import, and imports and
declarations reject a same-kind local alias in either order with the PHP 8.5
compile-time diagnostic. Compile-time-elided conditional declarations still
reserve their lexical name. A new namespace block resets the collision scope,
cross-kind aliases remain independent, and importing the same fully qualified
local class-like, function or constant symbol remains legal; a repeated import
still fails. The normalized class-import index keeps alias lookup constant-time
without adding metadata to ordinary declarations or changing runtime layouts.

Original E2E coverage exercises declaration-before-import and import-before-
declaration ordering, duplicate aliases, elided conditionals, multi-constant
statement source lines, namespace resets, cross-kind aliases, constant case
sensitivity and legal self-imports in both declaration orders. The exact
full-corpus additions are `Zend/tests/bug42859.phpt`,
`Zend/tests/name_collision/name_collision_07.phpt` through
`name_collision_09.phpt`, `Zend/tests/namespaces/ns_029.phpt` and
`ns_030.phpt`, `Zend/tests/use_const/conflicting_use.phpt`,
`define_imported.phpt`, `define_imported_before.phpt`,
`Zend/tests/use_function/case_insensivity.phpt`,
`conditional_function_declaration.phpt`, `conflicting_use.phpt`,
`define_imported.phpt`, `define_imported_before.phpt`, and
`Zend/tests/use_late_binding_conflict.phpt`; all 15 pass in 32 consecutive
focused runs. General function/constant redeclaration diagnostics and ordinary
cross-kind name resolution beyond these controls are not claimed here.

All five Cargo configurations, all-feature/all-target, formatting, PHPT runner
self-test, unsafe self-test and the exact unsafe ratchet pass, as do Composer
S0, all four Symfony S1 gates and exact PHP 8.5 warmed-kernel S2 and cold-build
S3. The production inventory remains 1,617 unsafe blocks and 289 unsafe
functions. On CPU 2 with the performance governor, four warmups per binary and
32 order-balanced measured pairs with no removed observations, 1,000 simple
class declarations move from 0.015140 to 0.015011 seconds (-0.853%
independently, -0.456% paired), while 1,000 valid class imports move from
0.023344 to 0.023533 seconds (+0.807% independently, +0.472% paired). Both are
below the five-percent regression ceiling and checksums match. The exact base
and candidate binary SHA-256 values are
`a6a581491457f8f288cfa7dab7a82d92f174f93af53a09972eeeb7404d1651c4`
and `751a58e2e023509401708992ed63c7c1f4c3c203ff7b77a40af2433b0a7f980e`.

In the preceding class-like redeclaration checkpoint, the measured AMD64 PHP
8.5 contract was pinned to php-src 8.5.6 commit `fcc29c8` and candidate commit
`0e71faea`. Across all 5,599 unmodified `Zend/tests` and `tests/lang` cases,
3,727 pass, 1,572 fail, 115
skip, none remain XFAIL, 185 are unsupported, and none time out or crash. The
headline pass rate is 70.334% and the whole-corpus rate is 66.565%; 4,710 of
5,299 attempted cases reach runtime (88.885%). Relative to exact base
`9dc13ff3`, the pass-set delta is +9/-0: all 3,718 prior passes remain passes,
and no other status or failure category changes. Two final merged manifests
and summaries are byte-for-byte identical. Their SHA-256 values are
`4af614cbdc6e9c7c689bf24b9d0be6a1d2390e63aff5f6d364124de32b9af325`
and `de913bcafc5dd6c4aad0c0bf5b1eff7ce01dc0f53bfd555c8bd6b967f31a804`.

Class-like name collisions now share one cold diagnostic formatter across
pending declarations, published classes and repeated runtime declaration
markers. A redeclaration reports PHP's original symbol kind and spelling, the
first user declaration's file and line, and the colliding declaration's file
and line. Internal classes and interfaces omit the unavailable previous-source
clause. Enum/non-enum collisions follow PHP 8.5's exceptional ownership rule:
the non-enum kind and spelling own the diagnostic regardless of declaration
order. Successful registration retains the pre-existing single
case-insensitive class-table scan and adds no metadata or object-layout state.

Original E2E coverage exercises differently cased class, interface, trait and
enum collisions, both enum/non-enum orders, internal `stdClass` and
`Stringable`, and a class declared on a repeated function call. The exact
full-corpus additions are `Zend/tests/errmsg/errmsg_026.phpt`,
`Zend/tests/inter_06.phpt`,
`Zend/tests/name_collision/declare_already_in_use.phpt`, and
`Zend/tests/name_collision/name_collision_01.phpt` through
`name_collision_06.phpt`; all nine pass in 32 consecutive focused runs. Import
name resolution, function and constant redeclarations, the active/reentrant
runtime-link collision, and the separate `class_alias()` warning contract are
not claimed by this checkpoint.

All five Cargo configurations, all-feature/all-target, formatting, PHPT runner
self-test, unsafe self-test and the exact unsafe ratchet pass, as do Composer
S0, all four Symfony S1 gates and exact PHP 8.5 warmed-kernel S2 and cold-build
S3. The production inventory remains 1,617 unsafe blocks and 289 unsafe
functions. On CPU 2 with the performance governor, four warmups per binary and
32 order-balanced measured pairs with no removed observations, 1,000
successful simple class declarations move from 0.019709 to 0.019742 seconds
(+0.166% independently, +0.023% paired), below the five-percent regression
ceiling. Checksums match. The exact base and candidate binary SHA-256 values
are `3c5a646294ba4cd45821f56686fc71762acfcfbe808c96d63c183f4f027e9e0e`
and `a6a581491457f8f288cfa7dab7a82d92f174f93af53a09972eeeb7404d1651c4`.

In the preceding direct-interface-relation checkpoint, the measured AMD64 PHP
8.5 contract was pinned to php-src 8.5.6 commit `fcc29c8` and candidate commit
`698349b7`. Across all 5,599 unmodified `Zend/tests` and `tests/lang` cases,
3,718 pass, 1,581 fail, 115
skip, none remain XFAIL, 185 are unsupported, and none time out or crash. The
headline pass rate is 70.164% and the whole-corpus rate is 66.405%; 4,710 of
5,299 attempted cases reach runtime (88.885%). Relative to exact base
`8e78657e`, the pass-set delta is +5/-0: all 3,713 prior passes remain passes,
and no other status or failure category changes. Two final merged manifests
and summaries are byte-for-byte identical. Their SHA-256 values are
`9404b1cd9b3ebe4eb61d67b065d83d752d867262e25fd7bd77eb4160e00eac53`
and `b5419da7910ae319507acfe668e1681f2cc6455bc286bb780f31f1bbb1aa72c7`.

Cold declaration linking now requires every resolved direct `implements` or
interface `extends` target to be an interface and rejects two direct spellings
that resolve to the same canonical interface identity. Diagnostics retain the
declaring class/interface kind, canonical target spelling and declaration
location. An explicit runtime alias rechecks this rule in stable declaration
order, while ordinary and alias-mediated diamonds that merely converge on one
inherited ancestor remain valid. Distinct direct type-argument bindings of one
interface retain the opt-in RPHP generics extension's plural Reflection
contract instead of being collapsed by their erased runtime identity.

Original E2E coverage exercises class and trait wrong-kind targets, class and
interface duplicate identities, case and alias canonicalization, declaration
locations, valid direct redundancy and ordinary/aliased diamonds. It also
corrects an earlier E2E oracle error that attributed an alias failure to the
`class_alias()` line and incorrectly rejected the aliased diamond. The exact
full-corpus additions are `Zend/tests/inter_05.phpt`,
`Zend/tests/objects/objects_012.phpt` through `objects_014.phpt`, and
`Zend/tests/traits/error_008.phpt`; the seven-case focused slice, including
retained alias and compatible-interface controls, passes in 32 consecutive
runs. Unresolved forward/self invalid relations, a lone invalid target reached
only through alternative casing, and broader inherited-interface signature
compatibility remain separate semantic slices.

All five Cargo configurations, all-feature/all-target, formatting, PHPT runner
self-test, unsafe self-test and the exact unsafe ratchet pass, as do Composer
S0, all four Symfony S1 gates and exact PHP 8.5 warmed-kernel S2 and cold-build
S3. The production inventory remains 1,617 unsafe blocks and 289 unsafe
functions. No runtime or object layout changes. On CPU 2 with the performance
governor, four warmup pairs and 32 order-balanced measured pairs with no
removed observations, a 1,000-pair resolved interface/class link control moves
from 0.109220 to 0.109122 seconds (-0.090% independently, -0.013% paired), and
the valid forward-link control moves from 0.133978 to 0.133709 seconds (-0.201%
independently, -0.103% paired). Checksums match and both medians remain below
the five-percent regression ceiling. The exact base and candidate binary
SHA-256 values are `43960b5ac7b788a819ce59c7a8869739feac96c5b1585cd6d988ea59e4e5cdda`
and `3c5a646294ba4cd45821f56686fc71762acfcfbe808c96d63c183f4f027e9e0e`.

In the preceding inherited-interface-staticness checkpoint, the measured
AMD64 PHP 8.5 contract was pinned to php-src 8.5.6 commit `fcc29c8` and
candidate commit `f6f94519`. Across all 5,599 unmodified `Zend/tests` and
`tests/lang` cases, 3,713 pass, 1,586 fail, 115
skip, none remain XFAIL, 185 are unsupported, and none time out or crash. The
headline pass rate is 70.070% and the whole-corpus rate is 66.315%; 4,710 of
5,299 attempted cases reach runtime (88.885%). Relative to exact base
`04124356`, the pass-set delta is +1/-0: all 3,712 prior passes remain passes,
and no other status or failure category changes. Two final merged manifests
and summaries are byte-for-byte identical. Their SHA-256 values are
`1f077f77aee77aa7e005836b9cbce7140e218e3ffb15d7fbe07c5b1f4028308d`
and `fd4f61da740b7424193617a8b7126a5e60a41bdc3e31d8e840ce8e0cce010a7d`.

Parent interfaces that contribute the same case-insensitive method name with
incompatible staticness now fail while the child interface is linked. The
first effective inherited declaration, or an explicit declaration on the
child, remains the implementation side of PHP's diagnostic. Method spelling
therefore follows that implementation while the conflicting requirement
retains its declaring interface as owner. Interface collection is alias-aware,
and an explicit `class_alias()` that publishes a previously unresolved edge
rechecks affected interface contracts in stable class-registration order after
preserving the existing duplicate-interface-identity diagnostic priority.

One original E2E regression covers the aliased and case-mismatched form, both
staticness directions, an explicit child declaration and a compatible repeated
non-static contract. The exact full-corpus addition is
`Zend/tests/inter_007.phpt`, which also passes in 32 consecutive focused runs.
Complete inherited-interface arity, reference and parameter/return-type
compatibility, including delayed variance dependencies, remains a separate
semantic slice and is not claimed by this checkpoint.

All five Cargo configurations, all-feature/all-target, formatting, PHPT runner
self-test, unsafe self-test and the exact unsafe ratchet pass, as do Composer
S0, all four Symfony S1 gates and exact PHP 8.5 warmed-kernel S2 and cold-build
S3. The production inventory remains 1,617 unsafe blocks and 289 unsafe
functions. No separate hot runtime performance gate applies: validation runs
only during cold interface declaration or an explicit `class_alias()`, scans
no successful method dispatch, adds no metadata or layout, and leaves the
successful dispatch path unchanged; S3 exercises the surrounding link path
end to end.

In the preceding inherited-trait diagnostic checkpoint, the measured AMD64
PHP 8.5 contract was pinned to php-src 8.5.6 commit `fcc29c8` and candidate
commit `71ff2ab6`. Across all 5,599 unmodified `Zend/tests` and `tests/lang`
cases, 3,712 pass, 1,587 fail, 115
skip, none remain XFAIL, 185 are unsupported, and none time out or crash. The
headline pass rate is 70.051% and the whole-corpus rate is 66.298%; 4,710 of
5,299 attempted cases reach runtime (88.885%). Relative to exact base
`a3446776`, the pass-set delta is +1/-0: all 3,711 prior passes remain passes,
and no other status or failure category changes. Two final merged manifests
and summaries are byte-for-byte identical. Their SHA-256 values are
`23a73a998c760a88fdac62d1cfa127990356df26bbf5a99e380de4eaff42feea`
and `96687be7416bc675b0ed4058bef8471a75b31f05b958930259c7abe52d54bf49`.

A concrete method compiled in a trait retains the trait as its declaration
owner after composition and inheritance. Cold incompatibility formatting now
walks the existing class/trait composition hierarchy and attributes that
method to the nearest class that actually consumed the trait. This covers a
method inherited from a parent, the same trait recomposed by a nearer parent,
a trait reached through another trait and the corresponding staticness fatal.
Direct consumers retain their existing class attribution, while abstract trait
requirements continue to name the trait that declared the requirement.

The existing trait-diagnostic E2E regression now covers those four additional
PHP 8.5.9 oracle boundaries. The exact full-corpus addition is
`Zend/tests/traits/bug62358.phpt`, which also passes in 32 consecutive focused
runs. Inherited narrowing of a concrete trait method's visibility remains an
earlier contract-enforcement gap, and inherited trait `self`/`parent`
pseudo-type binding in variance checks remains a separate semantic slice; this
checkpoint does not claim either behavior.

All five Cargo configurations, all-feature/all-target, formatting, PHPT runner
self-test, unsafe self-test and the exact unsafe ratchet pass, as do Composer
S0, all four Symfony S1 gates and exact PHP 8.5 warmed-kernel S2 and cold-build
S3. The production inventory remains 1,617 unsafe blocks and 289 unsafe
functions. No separate runtime performance gate applies: the owner traversal
runs only while formatting an already-failed class-link contract, adds no
metadata or layout, and leaves successful linking and method dispatch
unchanged; S3 exercises the surrounding class-link path end to end.

In the preceding reentrant array-key diagnostic checkpoint, the measured
AMD64 PHP 8.5 contract was pinned to php-src
8.5.6 commit `fcc29c8` and candidate commit `10586f82`. Across all 5,599
unmodified `Zend/tests` and `tests/lang` cases, 3,711 pass, 1,588 fail, 115
skip, none remain XFAIL, 185 are unsupported, and none time out or crash. The
headline pass rate is 70.032% and the whole-corpus rate is 66.280%; 4,710 of
5,299 attempted cases reach runtime (88.885%). Relative to exact base
`9fcac3f4`, the pass-set delta is +1/-0: all 3,710 prior passes remain passes,
and no other status or failure category changes. Two final merged manifests
and summaries are byte-for-byte identical. Their SHA-256 values are
`d457a819ba6a9a4719f97c0b249ae0d08f144743da1ade3805bfebdfc7b2a4a9`
and `a66767ccd0887e98a614273c0ff28f8a0a6251d097989c9a967623f4f1a74d26`.

An array-key conversion diagnostic may invoke user code before the dimension
read finishes. The baseline VM now snapshots the diagnostic source spelling
and retains the original array storage across that call. After the handler
returns, a replaced uniquely owned target yields `false` for `isset()` or
`null` for a read, while an existing copy-on-write owner preserves the read
from its original storage. A pristine empty array retains PHP's subsequent
undefined-key diagnostic even when the handler replaces the source variable,
and an exception thrown by the handler propagates without completing the
fetch. Rendering a non-representable float before the first warning also keeps
the second conversion diagnostic stable if the handler mutates the key source.

The empty-array lifetime marker occupies the otherwise unused high bit of the
existing internal cursor and is cleared before mutable `Value` access;
`PhpArray` remains 128 bytes. The cold diagnostic helper retains allocation
identity and owner count without adding a production unsafe function. This
checkpoint proves the exercised unique, shared COW, reference, pristine-empty
and throwing-handler boundaries. It does not claim complete provenance for
non-empty literals, every mutated-empty temporary-owner shape, or the separate
compiler snapshot-container read path.

One original E2E regression covers those lifetime and exception boundaries,
and an array-layout unit test covers marker clearing plus cursor behavior. Both
`Zend/tests/falsetoarray_003.phpt` and
`Zend/tests/type_coercion/float_to_int/non-rep-float-as-int-extra3.phpt` pass
in 32 consecutive runs. The latter is the exact full-corpus addition; the
former remains a retained pass.

All five Cargo configurations, all-feature/all-target, formatting, PHPT runner
self-test, unsafe self-test and the exact unsafe ratchet pass, as do Composer
S0, all four Symfony S1 gates and exact PHP 8.5 warmed-kernel S2 and cold-build
S3. The production inventory is 1,617 unsafe blocks and 289 unsafe functions,
within the established ceilings.

On an AMD Ryzen 9 7950X, the exact release binaries were pinned to CPU 2 with
the performance governor, four warmup pairs, 32 order-balanced measured pairs
and no removed observations. A five-million-iteration integer `isset()`
control moved from 0.215432 to 0.218616 seconds by independent medians
(+1.478%) and +1.654% by paired ratios. The standard indexed-array control
moved from 0.006784 to 0.006786 seconds (+0.038%) and +0.062% paired. Both
checksums are exact and every median remains below the five-percent regression
ceiling. The base and candidate binary SHA-256 values are respectively
`2a7d3573826eba3a1ee2b7e808cf156dda4e41cbb9f015b5551211c2d553b47b`
and `ecccf65168e45062cf40c67471297f9a97358484c8efd49ae48a625556bd187f`.

In the preceding method-staticness diagnostic checkpoint, the measured AMD64
PHP 8.5 contract was pinned to php-src
8.5.6 commit `fcc29c8` and candidate commit `77781600`. Across all 5,599
unmodified `Zend/tests` and `tests/lang` cases, 3,710 pass, 1,589 fail, 115
skip, none remain XFAIL, 185 are unsupported, and none time out or crash. The
headline pass rate is 70.013% and the whole-corpus rate is 66.262%; 4,710 of
5,299 attempted cases reach runtime (88.885%). Relative to exact base
`76a71d59`, the pass-set delta is +2/-0: all 3,708 prior passes remain passes,
both additions move from compile failure to exact pass, and no other status or
failure category changes. Two final merged manifests and summaries are
byte-for-byte identical. Their SHA-256 values are
`56c2256263454cf918b8458dfdc6902b4ed26f81e29f1c52d461781dbd3fed56`
and `a2f70b006b5d89cc38e80d627b2eaad351957f3e0958d3f642b6b1e5390d53f2`.

An inherited method that changes staticness now reports PHP 8.5's dedicated
`Cannot make static/non static method ...` fatal instead of the generic
signature-compatibility diagnostic. This diagnostic has priority over a
simultaneous visibility violation, names the original abstract parent or trait
requirement, names the class being linked, and preserves the implementation
source location. The established contract validation remains responsible for
detecting the mismatch; only its cold diagnostic selection changes.

One original E2E regression covers both staticness directions, priority over
narrowed visibility and abstract-trait ownership. The exact full-corpus
additions are `Zend/tests/traits/abstract_method_5.phpt` and
`Zend/tests/traits/bug78776.phpt`. The adjacent three-case focused gate retains
`Zend/tests/inter_007.phpt` as an explicit output failure because conflicting
staticness between two inherited interface requirements is not yet detected.

All five Cargo configurations, all-feature/all-target, formatting, PHPT runner
self-test and the exact unsafe ratchet pass, as do Composer S0, all four Symfony
S1 gates and exact PHP 8.5 warmed-kernel S2 and cold-build S3. The production
inventory remains 1,612 unsafe blocks and 289 unsafe functions. No separate hot
runtime benchmark applies: the change creates no metadata or layout change and
formats a different fatal only after cold contract validation has already
failed; S3 exercises the affected class-link path end to end.

In the preceding method-default diagnostic checkpoint, the measured AMD64 PHP
8.5 contract was pinned to php-src 8.5.6 commit `fcc29c8` and candidate commit
`9e9638f2`. Across all 5,599 unmodified `Zend/tests` and `tests/lang` cases,
3,708 pass, 1,591 fail, 115 skip, none remain XFAIL, 185 are unsupported, and
none time out or crash. The headline pass rate is 69.975% and the whole-corpus
rate is 66.226%; 4,708 of 5,299 attempted cases reach runtime (88.847%).
Relative to exact base `b4790c97`, the pass-set delta is +15/-0: all 3,693
prior passes remain passes, all additions move from compile failure to exact
pass, and no other status or failure category changes. Two final merged
manifests and summaries are byte-for-byte identical. Their SHA-256 values are
`b5fa4e7e93f4ebcaaf384260dcb125aa0076349151e796aa085ec7883920d024`
and `104c71a475e79071efd292a7987ef8e75263f17909b8b2393bed252a77c77c6c`.

Method-compatibility diagnostics now render PHP 8.5 parameter defaults for
userland methods. Scalar values, null, empty and non-empty arrays, truncated
strings, imported constants, class constants and dynamic class-constant
expressions use the reference diagnostic forms. A default before a later
required parameter remains part of the callable's nullable contract where
applicable but is omitted from the rendered signature, matching PHP's
effective required-argument boundary. The compiler retains these renderings in
an optional cold `UserFunction` sidecar only when a method has an effective
rendered default; ordinary functions and methods without one retain no sidecar
allocation. Internal-function defaults, ordinary Reflection display and
earlier parse failures remain separate compatibility slices.

Four original E2E regressions cover scalar/null/array/string rendering,
canonical imported constant and class names, dynamic class-constant expression
placeholders, and omission before a later required parameter. The exact
full-corpus additions are `Zend/tests/bug64988.phpt`,
`Zend/tests/inheritance/abstract_inheritance_003.phpt`,
`argument_restriction_001.phpt`, `bug71428.1.phpt`, `bug72119.phpt`,
`bug73987.phpt` and `bug73987_2.phpt`, plus
`Zend/tests/objects/objects_006.phpt`, `objects_007.phpt`,
`Zend/tests/oss-fuzz-465488618.phpt`,
`Zend/tests/parameter_default_values/userland_declaration_error_class_const.phpt`,
`userland_declaration_error_const.phpt`, `Zend/tests/traits/bug60217b.phpt`,
`bug60217c.phpt` and
`Zend/tests/variadic/adding_additional_optional_parameter_error.phpt`.

All five Cargo configurations, all-feature/all-target, formatting, PHPT runner
self-test and the exact unsafe ratchet pass, as do Composer S0, all four Symfony
S1 gates and exact PHP 8.5 warmed-kernel S2 and cold-build S3. The production
inventory remains 1,612 unsafe blocks and 289 unsafe functions. No separate hot
runtime benchmark applies: the new optional metadata follows all execution
fields, `FunctionCommon` remains at offset zero, rendering occurs only during
cold declaration diagnostics, and S3 exercises the affected compilation and
linking path end to end.

In the preceding method-visibility and magic-method diagnostic checkpoint, the
measured AMD64 PHP 8.5 contract was pinned to php-src 8.5.6 commit `fcc29c8`
and candidate commit `fb054c97`. Across all 5,599 unmodified `Zend/tests` and
`tests/lang` cases, 3,693 pass, 1,606 fail, 115 skip, none remain XFAIL, 185
are unsupported, and none time out or crash. The headline pass rate is 69.692%
and the whole-corpus rate is 65.958%; 4,708 of 5,299 attempted cases reach
runtime (88.847%). Relative to exact base `acad10dc`, the pass-set delta is
+13/-0: all 3,680 prior passes remain passes and no other status or failure
category changes. Two final merged manifests and summaries are byte-for-byte
identical. Their SHA-256 values are
`73ef76ffd950c144612f90bfa90eb34445c43a200cf9af17438e042588e553c3`
and `4c4c1f6850bba5c97a92b73bc3002f08d555d93103353588772ba71d1c5856f6`.

Method-contract validation now gives a visibility violation PHP's
higher-priority `Access level ...` diagnostic even when the same declaration
also has an incompatible signature. Public requirements omit a weakening
suffix, while protected requirements say `or weaker`. A concrete method
imported from a trait is attributed to the composing class; an abstract trait
requirement and an inherited parent implementation retain their declaring
owner. Non-public magic methods emit PHP's compile-time visibility warning
before later declaration failures. The warning applies to the public magic
method family while preserving the visibility exceptions for `__construct`,
`__destruct` and `__clone`, including declarations compiled through traits and
included source. Custom error-handler routing of compile warnings from `eval`
remains separate work.

Four original E2E regressions cover visibility priority, the protected
weakening boundary, concrete-versus-abstract trait attribution and magic
warning ordering/exceptions. The exact full-corpus additions are
`Zend/tests/bug67436/bug67436_nohandler.phpt`,
`Zend/tests/inheritance/bug62814.phpt`,
`Zend/tests/magic_methods/bug61970_1.phpt`, `bug61970_2.phpt`,
`magic_methods_002.phpt`, `magic_methods_004.phpt`, `magic_methods_008.phpt`
and `magic_methods_009.phpt`, plus `Zend/tests/traits/bug60153.phpt`,
`bug69467.phpt`, `bugs/abstract-methods05.phpt`,
`bugs/abstract-methods06.phpt` and `inheritance003.phpt`.

All five Cargo configurations, all-feature/all-target, formatting, PHPT runner
self-test and the exact unsafe ratchet pass, as do Composer S0, all four Symfony
S1 gates and exact PHP 8.5 warmed-kernel S2 and cold-build S3. The production
inventory remains 1,612 unsafe blocks and 289 unsafe functions. No separate hot
runtime benchmark applies: warning collection and method diagnostics run only
during compilation or cold declaration linking, method execution and value
layout are unchanged, and S3 exercises the affected cold path end to end.

In the preceding final-static return-variance checkpoint, the measured AMD64
PHP 8.5 contract was pinned to php-src 8.5.6 commit `fcc29c8` and candidate
commit `cce12eab`. Across all 5,599 unmodified `Zend/tests` and `tests/lang`
cases, 3,680 pass, 1,619 fail, 115 skip, none remain XFAIL, 185 are unsupported,
and none time out or crash. The previously known upstream XFAIL
`Zend/tests/inheritance/interface_constructor_prototype_002.phpt` is now an
exact pass. The headline pass rate is 69.447% and the whole-corpus rate is
65.726%; 4,703 of 5,299 attempted cases reach runtime (88.753%). Relative to
exact base `f2e3b6cc`, the pass-set delta is +18/-0: all 3,662 prior passes
remain passes. Five additional non-pass cases move from runtime to compile,
the correct declaration-validation stage, while retaining later diagnostic
gaps: `Zend/tests/inheritance/bug72119.phpt`,
`Zend/tests/traits/bug60153.phpt`, `bug69467.phpt`,
`Zend/tests/typehints/bug62441.phpt` and
`Zend/tests/variadic/adding_additional_optional_parameter_error.phpt`. Two
final merged manifests and summaries are byte-for-byte identical. Their
SHA-256 values are
`cf7ddc2f4621cf81b9d392af5a565be3cf9bd468273d3d5df6b3add0f3b3533e`
and `51d306343989615bb60c8046b29a64d74dddfc3f2172179d394a04d69248a1e8`.

A final implementation class may now close a required late-static return with
`self` or its exact class name because no descendant can widen that result.
The same rule applies through union branches, and source-equivalent `?T` and
`T|null` forms are normalized for covariance. Non-final implementations,
including final methods on an extensible class, still require `static`, and an
unrelated union branch remains incompatible. Interface requirements, including
those inherited through a parent, now join abstract class and trait
requirements in pre-publication validation so incompatible declarations fail
at PHP's declaration boundary.

Three original E2E regressions cover final implementations of interface,
abstract-parent and abstract-trait contracts plus union and nullable forms; the
non-final rejection; and rejection of an unrelated union branch. The nine-case
`Zend/tests/type_declarations/variance/override_static_with_self` cluster is
now exact: `static_to_self.phpt`, `static_to_self_in_non_final_class.phpt`,
`static_to_self_to_unions.phpt` and `union_to_union_1.phpt` through
`union_to_union_6.phpt`. The nine adjacent exact additions are
`Zend/tests/inheritance/interface_constructor_prototype_001.phpt`,
`interface_constructor_prototype_002.phpt`,
`Zend/tests/nullable_types/covariant_nullable_param_fails.phpt`,
`Zend/tests/return_types/009.phpt`, `inheritance003.phpt`, `rfc004.phpt`, and
`Zend/tests/variadic/non_variadic_implements_variadic_error.phpt`,
`variadic_changed_byref_error.phpt` and `variadic_changed_typehint_error.phpt`.

All five Cargo configurations, all-feature/all-target, formatting, PHPT runner
self-test and the exact unsafe ratchet pass, as do Composer S0, all four Symfony
S1 gates and exact PHP 8.5 warmed-kernel S2 and cold-build S3. The production
inventory remains 1,612 unsafe blocks and 289 unsafe functions. No separate hot
runtime benchmark applies: the work is confined to cold declaration and
class-link validation, successful method execution and value layout are
unchanged, and S3 exercises the affected cold path end to end.

In the preceding conditional-declaration and variance-loading-exception
checkpoint, the measured AMD64 PHP 8.5 contract was pinned to php-src 8.5.6
commit `fcc29c8`. Across all 5,599 unmodified `Zend/tests` and `tests/lang`
cases, 3,662 pass, 1,636 fail, 115 skip, one is an upstream XFAIL, 185 are
unsupported, and none time out or crash. The headline pass rate is 69.120% and
the whole-corpus rate is 65.405%; 4,712 of 5,298 attempted cases reach runtime
(88.939%). Relative to exact base `5e35c679`, the pass-set delta is +7/-0: all
3,655 prior passes remain passes. The only non-pass changes are
`Zend/tests/bug78406.phpt` and
`Zend/tests/inheritance/deprecation_to_exception_during_inheritance_can_be_caught.phpt`,
which advance from an early output mismatch to their later runtime gaps after
the containing conditional declaration begins at the correct stage. Two final
merged manifests and summaries are byte-for-byte identical. Their SHA-256
values are `6f20b8c3151b1bb66b87277dd28159f22792d7cb5883061bc580d7bf44f3e6c7`
and `6e8ccc7fc175a144792fbecd91796729fb2035975d67fcc4da91d7e250779cbf`.

Class, interface, trait and enum declarations nested in `if`, loop, `switch`
or `try` control flow now remain unpublished until execution reaches their
source marker. Unconditional top-level declarations and declarations inside a
bare top-level block retain eager publication. A caught exception while
autoloading a parent or interface restores the marker so a later loop
iteration can retry the declaration, while an active class relation records
when another class has already relied on it for method variance. At that point
PHP can no longer unlink the active declaration: a later dependency-loader
exception becomes the source-located `During inheritance ... with variance
dependencies` fatal and cannot be caught by the surrounding declaration.

Four original E2E regressions cover publication timing for every class-like
kind, retry after a caught parent load, active declarations remaining hidden
from reentrant class and interface autoload, and the fatal variance-dependent
boundary. The exact full-corpus additions are
`Zend/tests/autoload/bug63741.phpt`, `Zend/tests/bug35634.phpt`,
`Zend/tests/bug78926.phpt`,
`Zend/tests/type_declarations/variance/loading_exception1.phpt`,
`loading_exception2.phpt`, `unlinked_parent_1.phpt` and
`unlinked_parent_2.phpt`.

All five Cargo configurations, all-feature/all-target, formatting, PHPT runner
self-test and the exact unsafe ratchet pass, as do Composer S0, all four Symfony
S1 gates and exact PHP 8.5 warmed-kernel S2 and cold-build S3. The production
inventory remains 1,612 unsafe blocks and 289 unsafe functions. No separate hot
runtime benchmark applies: the compiler flag is consumed only while preparing
declarations, the relation state is read and written only during cold class
linking or its exceptional dependency path, and S3 exercises the affected
compile/autoload lifecycle end to end.

In the preceding unavailable-variance diagnostic checkpoint, the measured
AMD64 PHP 8.5 contract was pinned to php-src
8.5.6 commit `fcc29c8`. Across all 5,599 unmodified `Zend/tests` and
`tests/lang` cases, 3,655 pass, 1,643 fail, 115 skip, one is an upstream XFAIL,
185 are unsupported, and none time out or crash. The headline pass rate is
68.988% and the whole-corpus rate is 65.280%; 4,713 of 5,298 attempted cases
reach runtime (88.958%). Relative to exact base `572ed0d5`, the pass-set delta
is +2/-0: all 3,653 prior passes remain passes and no other status or failure
category changes. Two final merged manifests and summaries are byte-for-byte
identical. The manifest SHA-256 is
`f439d2d1dd0ffd08d1d4d01fbe37aa4579ace411967aa25d7e3a1c10a665c9ad`.

Method-variance dependency discovery now shares one ordered traversal of parent
and abstract contracts. If autoload returns normally without defining a
required class, runtime declarations, included/evaluated source units and
anonymous classes report PHP 8.5's `Could not check compatibility ... because
class ... is not available` diagnostic for the first affected contract. A
descendant participating in a suspended class-link transaction reconstructs
the diagnostic from its active provisional parent, so the message names the
parent obligation rather than the incidental descendant. Exceptions thrown by
the loader retain the existing catchable exception path.

The diagnostic is reconstructed only after the missing-symbol result; the
ordinary successful plan continues to retain just dependency names. Two
original E2E regressions cover a directly missing return class and the nested
active-parent obligation. The exact full-corpus additions are
`Zend/tests/type_declarations/variance/class_order_autoload_error6.phpt` and
`class_order_autoload_error7.phpt`. The separate `loading_exception1.phpt` and
`loading_exception2.phpt` contract/transaction boundaries remain follow-up
work.

All five Cargo configurations, all-feature/all-target, formatting, PHPT runner
self-test, unsafe self-test and the exact unsafe ratchet pass, as do Composer
S0, all four Symfony S1 gates and exact PHP 8.5 warmed-kernel S2 and cold-build
S3. The production inventory remains 1,612 unsafe blocks and 289 unsafe
functions. On one pinned AMD64 CPU, 32 order-balanced release pairs with four
warmups per binary and no removed observations measured 2,000 successful
variance-autoload links at 2.242807 seconds for exact base `572ed0d5` and
2.239782 seconds for the candidate: -0.135% independently and -0.094% by paired
ratios, with paired p10/p90 of -1.631%/+1.182% and checksum `2000`. An eager
diagnostic-string prototype was rejected at +7.965% independently and +7.700%
paired on the same gate before the lazy design was measured.

In the preceding suspended-class-link checkpoint, the measured AMD64 PHP 8.5
contract was pinned to php-src
8.5.6 commit `fcc29c8`. Across all 5,599 unmodified `Zend/tests` and
`tests/lang` cases, 3,653 pass, 1,645 fail, 115 skip, one is an upstream XFAIL,
185 are unsupported, and none time out or crash. The headline pass rate is
68.951% and the whole-corpus rate is 65.244%; 4,713 of 5,298 attempted cases
reach runtime (88.958%). Relative to exact base `3ae5a787`, the pass-set delta
is +3/-0: all 3,650 prior passes remain passes and no other status or failure
category changes. Three final merged manifests and summaries are byte-for-byte
identical. The manifest SHA-256 is
`502250688b3e0b6048000ba5d65e0a1ab1460ff2c92c593e032a89a7e38f6351`.

A provisionally composed class now retains its complete ordered set of
outstanding method- and property-variance dependencies. When autoload declares
a descendant of that active parent, the descendant drains the parent's still
missing dependencies depth-first, skips symbols that are already loaded or
currently linking, and revalidates the parent after every dependency returns.
This preserves PHP 8.5's nested loader order while reporting the first invalid
parent contract at the same point, before later dependency side effects can
run. The existing transaction continues to hide the provisional class from
ordinary userland lookup until final publication.

Two original E2E regressions cover deepest-dependency-first completion and the
diagnostic from an invalid nested descendant. The exact full-corpus additions
are `Zend/tests/type_declarations/variance/class_order_autoload6.phpt`,
`class_order_autoload_error8.phpt` and `class_order_autoload_error9.phpt`.
The distinct `class_order_autoload_error6.phpt` and
`class_order_autoload_error7.phpt` unlinked-parent boundaries remain follow-up
work.

All five Cargo configurations, all-feature/all-target, formatting, PHPT runner
self-test, unsafe self-test and the exact unsafe ratchet pass, as do Composer
S0, all four Symfony S1 gates and exact PHP 8.5 warmed-kernel S2 and cold-build
S3. The production inventory remains 1,612 unsafe blocks and 289 unsafe
functions. On one pinned AMD64 CPU, 32 order-balanced release pairs with four
warmups per binary and no removed observations measured 2,000 cold `eval`
class links at 0.161837 seconds for exact base `3ae5a787` and 0.161775 seconds
for the candidate: -0.038% independently and -0.210% by paired ratios, with
paired p10/p90 of -1.041%/+0.531% and checksum `1`.

In the preceding provisional-class-publication checkpoint, the measured AMD64
PHP 8.5 contract was pinned to php-src
8.5.6 commit `fcc29c8`. Across all 5,599 unmodified `Zend/tests` and
`tests/lang` cases, 3,650 pass, 1,648 fail, 115 skip, one is an upstream XFAIL,
185 are unsupported, and none time out or crash. The headline pass rate is
68.894% and the whole-corpus rate is 65.190%; 4,712 of 5,298 attempted cases
reach runtime (88.939%). Relative to exact base `3a2826f3`, the pass-set delta
is +2/-0: all 3,648 prior passes remain passes and no other status or failure
category changes. Two final merged manifests and summaries are byte-for-byte
identical. The manifest SHA-256 is
`79ed36b356819da335567d027e0409446cf96439afe525bb4222636ffad1b2e4`.

A runtime class with one outstanding method-variance dependency can now be
fully composed and provisionally published inside the active linking
transaction. A descendant loaded to satisfy that dependency therefore sees the
parent's inherited methods, properties and constructor, while ordinary
userland still observes the parent as absent through existence and declaration
inventory probes, explicit static calls, object construction, Reflection and
the class-relation introspection helpers. Relative `parent::` access from the
completed descendant retains the internal composed view. Final parent and
abstract-method contract validation runs only after the dependency finishes.

Dependency discovery rejects variance directions that cannot become compatible
through autoload, so invalid reverse relations retain their established
diagnostics and side effects. The provisional path is deliberately restricted
to one descendant dependency; multiple suspended descendants still need an
ordered link stack and remain separate work. Three original E2E regressions
cover public invisibility and internal relative scope, inherited layout and
constructor use, and an abstract trait `self` obligation. The exact full-corpus
additions are
`Zend/tests/type_declarations/variance/class_order_autoload2.phpt` and
`Zend/tests/traits/abstract_method_9.phpt`. `class_order_autoload6.phpt` remains
the next multiple-descendant transaction boundary; the related loading-error
cases retain their prior failure stage and loader side effects.

All five Cargo configurations, all-feature/all-target, formatting, PHPT runner
self-test, unsafe self-test and the exact unsafe ratchet pass, as do Composer
S0, all four Symfony S1 gates and PHP 8.5 warmed-kernel S2 and cold-build S3.
The production inventory remains 1,612 unsafe blocks and 289 unsafe functions.
On one pinned AMD64 CPU, 32 order-balanced release pairs with four warmups per
binary measured one million dynamic constructions at 0.334931 seconds for the
exact baseline and 0.343610 seconds for the candidate: +2.591% independently
and +2.396% by paired ratios, below the five-percent ceiling with checksum
`500000`. A 2,000-class cold `eval` linking control measured 0.182476 and
0.181352 seconds: -0.616% independently and -0.768% paired, with identical
results. The ordinary public-class guard bypasses name normalization whenever
no runtime link is active.

In the preceding nested-interface-publication checkpoint, the measured AMD64
PHP 8.5 contract was pinned to php-src
8.5.6 commit `fcc29c8`. Across all 5,599 unmodified `Zend/tests` and
`tests/lang` cases, 3,648 pass, 1,650 fail, 115 skip, one is an upstream XFAIL,
185 are unsupported, and none time out or crash. The headline pass rate is
68.856% and the whole-corpus rate is 65.154%; 4,710 of 5,298 attempted cases
reach runtime (88.901%). Relative to exact base `81a93455`, the pass-set delta
is +1/-0: all 3,647 prior passes remain passes and no other failure changes
category. Two final merged manifests and summaries are byte-for-byte identical.
The manifest SHA-256 is
`9e1982b0f8a7a386f8a677d2f1c7dbc0f5f3e3c9f6ab524bdedcdf18af697784`.

Named interfaces and traits compiled inside a function, method or closure now
use the same execution-time declaration marker as nested classes and enums.
Their definitions remain outside the public symbol table until the containing
child op-array reaches the declaration, then reuse the existing dependency
autoload, rollback, duplicate-name and registration path. Unconditional
top-level interfaces and traits retain eager registration; their source marker
is a cold no-op because no runtime definition is queued for it.

An original E2E regression proves both `interface_exists(..., false)` and
`trait_exists(..., false)` remain false before the containing function runs and
become true afterward. The exact full-corpus addition is
`Zend/tests/type_declarations/variance/class_order_autoload4.phpt`, whose nested
interface inheritance cycle now preserves PHP 8.5's load and output order.
`class_order_autoload2.phpt` and `class_order_autoload6.phpt` remain the next
transactional boundary because they require provisional publication of a new
child of the class currently linking.

All five Cargo configurations, all-feature/all-target, formatting, PHPT runner
self-test, unsafe self-test and the exact unsafe ratchet pass, as do Composer
S0, all four Symfony S1 gates and PHP 8.5 warmed-kernel S2 and cold-build S3.
The production inventory remains 1,612 unsafe blocks and 289 unsafe functions.
No runtime performance benchmark applies: the change adds only cold declaration
metadata and an execution-time marker for child interface/trait declarations;
ordinary method execution is unchanged, and the cold-build S3 gate exercises
the affected class-like loading path.

In the preceding transactional class-link checkpoint, the measured AMD64 PHP
8.5 contract was pinned to php-src
8.5.6 commit `fcc29c8`. Across all 5,599 unmodified `Zend/tests` and
`tests/lang` cases, 3,647 pass, 1,651 fail, 115 skip, one is an upstream XFAIL,
185 are unsupported, and none time out or crash. The headline pass rate is
68.837% and the whole-corpus rate is 65.137%; 4,710 of 5,298 attempted cases
reach runtime (88.901%). Relative to exact base `55c3ffbe`, the pass-set delta
is +7/-0: all 3,640 prior passes remain passes. Two final merged manifests and
summaries are byte-for-byte identical. The manifest SHA-256 is
`ac65ecb773ff70cb4799947bedb5ef56925b76c1288dae91e150d97820364978`.

Named classes and enums compiled inside a function, method or closure now stay
outside the public class table until their child op-array executes the source
declaration. Runtime linking retains only hierarchy metadata for declarations
active through reentrant autoload, so method variance can prove descendant
relationships without making half-composed classes visible to `class_exists()`
or ordinary lookup. A strict comparison used only for dependency discovery
also creates outstanding autoload obligations for otherwise optimistic
two-unknown class relations; the normal compatibility decision remains
unchanged until those types load. Failed dependency loads restore the runtime
declaration marker, permitting a later retry after a caught loader exception.

Original E2E regressions cover function- and closure-local publication timing,
an active descendant cycle whose root remains publicly invisible while
linking, the reverse invalid cycle's exact declaration fatal, and retry after
an autoloader exception. The exact full-corpus additions are
`Zend/tests/constants/gh10709_2.phpt`, `Zend/tests/enum/enum_exists.phpt`, and
`Zend/tests/type_declarations/variance/class_order_autoload1.phpt`,
`class_order_autoload3.phpt`, `class_order_autoload5.phpt`,
`class_order_autoload_error4.phpt` and `class_order_autoload_error5.phpt`.
`class_order_autoload2.phpt` and `class_order_autoload6.phpt` still need
provisional publication of a new child of the class currently linking;
`class_order_autoload4.phpt` needs runtime publication for nested interfaces.
The remaining loading-error, unlinked-parent and recursive abstract-trait cases
retain separate diagnostic or transactional-linking work.

All five Cargo configurations, all-feature/all-target, formatting, PHPT runner
self-test, unsafe self-test and the exact unsafe ratchet pass, as do Composer
S0, all four Symfony S1 gates and PHP 8.5 warmed-kernel S2 and cold-build S3.
The production inventory remains 1,612 unsafe blocks and 289 unsafe functions.
No runtime performance benchmark applies: the new work is confined to cold
compilation metadata and runtime declaration/link validation, ordinary method
execution is unchanged, and the cold-build S3 gate exercises the affected
autoload path.

In the preceding runtime method-variance autoload checkpoint, the measured
AMD64 PHP 8.5 contract was pinned to php-src
8.5.6 commit `fcc29c8`. Across all 5,599 unmodified `Zend/tests` and
`tests/lang` cases, 3,640 pass, 1,658 fail, 115 skip, one is an upstream XFAIL,
185 are unsupported, and none time out or crash. The headline pass rate is
68.705% and the whole-corpus rate is 65.012%; 4,708 of 5,298 attempted cases
reach runtime (88.864%). Relative to exact base `54ca1c6a`, the pass-set delta
is +1/-0: all 3,639 prior passes remain passes and no remaining failure changes
category. Two final merged manifests and summaries are byte-for-byte identical.
The manifest SHA-256 is
`848b1d8a5f7edcf39140802050cdd70c74d991e4f46ca59afb2681d8bf10cb47`.

Runtime class declarations now soft-autoload unknown class-like dependencies
from method contracts before final link validation, but only when resolving the
eventual hierarchy could make an otherwise valid signature compatible. The
same rule covers ordinary runtime declarations, included source units and
anonymous-class registration. Definite visibility, staticness, arity,
variadic, reference and scalar-type errors retain their existing diagnostics
without observable autoload side effects, and contextual or pseudo-types are
resolved or excluded rather than requested from a loader. Nullable covariant
return types now compare their inner class types instead of requiring literal
type-hint equality.

Original E2E regressions cover a known nullable hierarchy, a missing return
class supplied from an external file by a registered loader, and a fixed arity
error whose loader would throw if it were invoked. The exact full-corpus
addition is
`Zend/tests/type_declarations/variance/trait_success.phpt`. That upstream
fixture does not observe whether its inline loader ran; the external-file E2E
regression independently proves the new runtime autoload boundary. Recursive
`Zend/tests/traits/abstract_method_9.phpt` deliberately remains in the same
compile-failure category: loading `D extends C` while `C` itself is linking
requires provisional transactional class publication. The distinct
no-autoloader "class not available" diagnostic also remains follow-up work.

All five Cargo configurations, all-feature/all-target, formatting, PHPT runner
self-test, unsafe self-test and the exact unsafe ratchet pass, as do Composer
S0, all four Symfony S1 gates and PHP 8.5 warmed-kernel S2 and cold-build S3.
The production inventory remains 1,612 unsafe blocks and 289 unsafe functions.
No runtime performance benchmark applies: the added work is confined to cold
runtime declaration/link validation, ordinary method calls are unchanged, and
the cold-build S3 gate exercises the affected class-loading path.

In the preceding private-abstract-trait checkpoint, the measured AMD64 PHP 8.5
contract was pinned to php-src
8.5.6 commit `fcc29c8`. Across all 5,599 unmodified `Zend/tests` and
`tests/lang` cases, 3,639 pass, 1,659 fail, 115 skip, one is an upstream XFAIL,
185 are unsupported, and none time out or crash. The headline pass rate is
68.686% and the whole-corpus rate is 64.994%; 4,707 of 5,298 attempted cases
reach runtime (88.845%). Relative to exact base `9be14670`, the pass-set delta
is +8/-0: all 3,631 prior passes remain passes. Two final merged manifests and
summaries are byte-for-byte identical. The manifest SHA-256 is
`279dad512be1c49ea023bdca1ba1f356a49b34601fcf4e68c10d7d794f7fda0f`.

Private abstract method requirements are now valid inside traits while the
same modifier pair remains invalid in ordinary classes. At composition time,
`self` types bind to the consuming class, an explicit abstract method on that
class can forward the obligation with wider visibility, and a compatible
private implementation satisfies it. An unforwarded private requirement must
be implemented by the immediate consumer even when that consumer is abstract;
ordinary concrete consumers retain PHP's distinct "contains abstract method"
diagnostic. Signature mismatches now use the shared canonical renderer with
parameter names, resolved trait-relative types and declaration locations,
including when an inherited concrete method is selected as the implementation.

Original parser and E2E regressions cover trait-only admission, consuming-class
`self`, private implementations, protected abstract forwarding, concrete and
abstract missing-method wording, incompatible signatures and inherited
implementations. The exact full-corpus additions are
`Zend/tests/inheritance/constructor_abstract_grantparent.phpt` plus
`Zend/tests/traits/abstract_method_1.phpt`, `_3.phpt`, `_6.phpt` through
`_8.phpt`, `_10.phpt` and `gh14009_002.phpt`. The remaining
`abstract_method_9.phpt` advances from the former parser rejection to its
correct link boundary but remains a failure: RPHP still needs to defer the
`D`-is-a-`C` return-variance decision until the autoloader publishes `D`.

All five Cargo configurations, all-feature/all-target, formatting, PHPT runner
self-test, unsafe self-test and the exact unsafe ratchet pass, as do Composer
S0, all four Symfony S1 gates and PHP 8.5 warmed-kernel S2 and cold-build S3.
The production inventory remains 1,612 unsafe blocks and 289 unsafe functions.
No runtime performance gate applies: the change is confined to parser and cold
class-link validation, valid method execution is unchanged, and the cold-build
S3 gate exercises the affected linking path.

In the preceding duplicate-member-modifier checkpoint, the measured AMD64 PHP
8.5 contract was pinned to php-src 8.5.6 commit `fcc29c8`. Across all 5,599
unmodified `Zend/tests` and
`tests/lang` cases, 3,631 pass, 1,667 fail, 115 skip, one is an upstream XFAIL,
185 are unsupported, and none time out or crash. The headline pass rate is
68.535% and the whole-corpus rate is 64.851%; 4,704 of 5,298 attempted cases
reach runtime (88.788%). Relative to exact base `ad9daff5`, the pass-set delta
is +9/-0: all 3,622 prior passes remain passes and no other status or failure
category moves. Two final merged manifests and summaries are byte-for-byte
identical. The manifest SHA-256 is
`b66a7047ba786dfcf0d1534006bbdbaccd63c1ee4b60d30ab434a9fcbdc8b6ea`.

Repeated method and property modifiers now produce PHP 8.5 compile-time fatal
errors instead of accepting the declaration or reporting a parser failure.
One shared declaration reducer retains the first duplicate across ordinary
read visibility, asymmetric set visibility, `static`, `final`, `abstract` and
`readonly`, then reports it at the method or property declaration line. The
rule applies consistently to classes, traits, interfaces, enums and anonymous
classes. A duplicate abstract declaration can still be parsed far enough to
emit the earlier modifier error, and `final abstract` uses PHP's canonical
message without appending the method name. This also removes the former CLI
special case that promoted one parser-error string to a fatal diagnostic.

Original regressions cover same and mixed visibility, first-error precedence,
all modifier kinds, abstract bodies, `final abstract`, asymmetric visibility
and every class-like declaration shape. The exact full-corpus additions are
`Zend/tests/access_modifiers/access_modifiers_001.phpt`, `_002.phpt`,
`_004.phpt` through `_007.phpt`, `Zend/tests/errmsg/errmsg_009.phpt`,
`errmsg_010.phpt` and `Zend/tests/readonly_props/multiple_modifiers.phpt`.
Class-level duplicate modifiers remain separate front-end work;
`access_modifiers_003.phpt` remains visible as a failure.

All five Cargo configurations, all-feature/all-target, formatting, PHPT runner
self-test, unsafe self-test and the exact unsafe ratchet pass, as do Composer
S0, all four Symfony S1 gates and PHP 8.5 warmed-kernel S2 and cold-build S3.
The production inventory remains 1,612 unsafe blocks and 289 unsafe functions.
No runtime performance gate applies: invalid declarations stop at compilation,
valid declaration AST and compiler output are unchanged, and the cold-build S3
gate exercises the affected front-end path.

In the preceding serialize-precision checkpoint, the measured AMD64 PHP 8.5
contract was pinned to php-src 8.5.6 commit `fcc29c8`. Across all 5,599
unmodified `Zend/tests` and
`tests/lang` cases, 3,622 pass, 1,676 fail, 115 skip, one is an upstream XFAIL,
185 are unsupported, and none time out or crash. The headline pass rate is
68.365% and the whole-corpus rate is 64.690%; 4,702 of 5,298 attempted cases
reach runtime (88.750%). Relative to exact base `b753762b`, the pass-set delta
is +1/-0: all 3,621 prior passes remain passes. Admitting the new INI directive
also moves one AMD64-inapplicable case from unsupported to its upstream skip;
no other status or failure category moves. Two final merged manifests and
summaries are byte-for-byte identical. The manifest SHA-256 is
`18b99bc6bf823a0e5d672584ac761e4471396b07fb1be3b7a39c7bf821eab39b`.

PHP 8.5 `serialize_precision` is now request-local with its `-1` default and is
accepted through repeated CLI `-d` definitions plus `ini_get()`/`ini_set()`.
Valid leading integer text is preserved by the INI APIs while controlling the
parsed significant-digit value; values below `-1` retain or restore the
default. `var_export()`, `var_dump()`, `serialize()` and `json_encode()` share
the setting while ordinary echo/string conversion continues to use the
separate `precision` directive. The formatters preserve signed zero, float
identity where the API requires it, PHP's fixed/scientific boundaries and the
distinct zero-precision form. JSON additionally honors
`JSON_PRESERVE_ZERO_FRACTION`; compatible default values retain the original
compact-serde path instead of paying for the precision-aware formatter.

Original CLI coverage exercises startup and runtime mutation, invalid and
prefix-valued settings, precision zero, ±0, ordinary fractions, large and small
exponents, nested JSON and all four consumers. The exact full-corpus addition
is `tests/lang/bug24640.phpt`; `Zend/tests/hex_overflow_32bit.phpt` is now
correctly evaluated as an architecture skip instead of an unsupported INI
case. One existing callback-pipeline expectation was corrected from `6.0` to
the independently verified PHP 8.5 default JSON spelling `6`.

All five Cargo configurations, all-feature/all-target, formatting, PHPT runner
self-test, unsafe self-test and the exact unsafe ratchet pass, as do Composer
S0, all four Symfony S1 gates and PHP 8.5 warmed-kernel S2 and cold-build S3.
The production inventory remains 1,612 unsafe blocks and 289 unsafe functions.
On an AMD Ryzen 9 7950X pinned to one performance-governor CPU, 32 balanced
alternating release A/B pairs with two warmups per binary, JIT and quick loops
disabled execute five million ordinary `json_encode(1.25)` calls with the same
20,000,000 checksum. The candidate is +1.777% by independent medians and
+1.872% by the paired-delta median, within the +5% gate.

In the preceding canonical-special-float-export checkpoint, the measured AMD64
PHP 8.5 contract was pinned to php-src
8.5.6 commit `fcc29c8`. Across all 5,599 unmodified `Zend/tests` and
`tests/lang` cases, 3,621 pass, 1,676 fail, 114 skip, one is an upstream XFAIL,
187 are unsupported, and none time out or crash. The headline pass rate is
68.359% and the whole-corpus rate is 64.672%; 4,701 of 5,297 attempted cases
reach runtime (88.748%). Relative to exact base `56f5d908`, the pass-set delta
is +1/-0: all 3,620 prior passes remain passes and no other status or failure
category moves. Two final merged manifests and summaries are byte-for-byte
identical. The manifest SHA-256 is
`e124d838a3ef46742c130fe2ea09c8ae8bd495d616b0228a93646d2b3f3fa7a6`.

PHP 8.5 `var_export()` now renders non-finite floats with the canonical `NAN`,
`INF` and `-INF` spellings in both direct-output and returned-string modes,
including values nested in arrays and objects. Ordinary finite float rendering
was unchanged in that checkpoint; its wider `serialize_precision` contract was
addressed by the following checkpoint.
Original E2E coverage exercises all three special values, both API modes and a
nested array. The exact full-corpus addition is
`Zend/tests/type_coercion/nan_comp_op.phpt`; differential probing confirms its
comparison results were already correct and only the exported NAN label moved.

All five Cargo configurations, all-feature/all-target, formatting, unsafe
self-test and the exact unsafe ratchet pass, as do Composer S0, all four
Symfony S1 gates and PHP 8.5 warmed-kernel S2 and cold-build S3. The production
inventory remains 1,612 unsafe blocks and 289 unsafe functions. No runtime
performance gate applies: the change is confined to the cold, explicitly
invoked `var_export()` renderer and does not touch executor, compiler or value
hot paths.

In the preceding NAN-coercion checkpoint, the measured AMD64 PHP 8.5 contract
was pinned to php-src
8.5.6 commit `fcc29c8`. Across all 5,599 unmodified `Zend/tests` and
`tests/lang` cases, 3,620 pass, 1,677 fail, 114 skip, one is an upstream XFAIL,
187 are unsupported, and none time out or crash. The headline pass rate is
68.341% and the whole-corpus rate is 64.654%; 4,701 of 5,297 attempted cases
reach runtime (88.748%). Relative to exact base `e68b0b8f`, the pass-set delta
is +3/-0: all 3,617 prior passes remain passes and no other status or failure
category moves. Two final merged manifests and summaries are byte-for-byte
identical. The manifest SHA-256 is
`c5befe463edd76f7c47c4edeb81c038ff2767af83b81b5944e2fdd65b7b9b418`.

PHP 8.5 NAN conversion warnings now cover weak boolean and string arguments
plus explicit bool, string, array and object casts. `boolval()`, `strval()` and
the existing `settype()` conversions follow the same diagnostic contract.
References are transparent, union coercion retains PHP precedence, and an
exact float union member accepts NAN without a warning. Reentrant cast ordering
is conversion-specific: string captures the original NAN text, bool and array
read the live value after the handler, and object conversion allocates its
result before the handler but writes the live scalar property afterward. A
throwing handler interrupts a cast or call without exposing a partial result.

Weak NAN-to-integer calls and typed writes no longer silently saturate to zero.
The shared scalar guard rejects every non-finite or out-of-AMD64-range float
before an integer write, preserving the prior property value and PHP's
`TypeError`. Original E2E coverage exercises ordinary, union and reference
arguments, every explicit boundary, handler interruption and the exact-float
control. The full-corpus additions are
`Zend/tests/type_coercion/nan_to_other.phpt`,
`Zend/tests/type_declarations/typed_properties_038.phpt` and
`typed_properties_075.phpt`. The then-visible `nan_comp_op.phpt` failure was
outside this diagnostic checkpoint and was subsequently isolated to its
`var_export(NAN)` label rather than comparison ordering.

All five Cargo configurations, all-feature/all-target, formatting, unsafe
self-test and the exact unsafe ratchet pass, as do Composer S0, all four
Symfony S1 gates and PHP 8.5 warmed-kernel S2 and cold-build S3. The production
inventory remains 1,612 unsafe blocks and 289 unsafe functions. On an AMD Ryzen
9 7950X pinned to one performance-governor CPU, three 32-pair alternating
release A/B controls with two warmups per binary, JIT and quick loops disabled
execute five million ordinary operations with identical checksums. Dynamic
bool casts measure +2.227% independently and +1.883% paired; weak bool calls
measure -5.272% and -5.247%; weak typed-property writes measure -5.595% and
-5.612%. Every median remains within the +5% gate. Favorable medians are
control evidence, not an optimization claim.

In the preceding explicit-numeric-cast checkpoint, the measured AMD64 PHP 8.5
contract was pinned to php-src
8.5.6 commit `fcc29c8`. Across all 5,599 unmodified `Zend/tests` and
`tests/lang` cases, 3,617 pass, 1,680 fail, 114 skip, one is an upstream XFAIL,
187 are unsupported, and none time out or crash. The headline pass rate is
68.284% and the whole-corpus rate is 64.601%; 4,701 of 5,297 attempted cases
reach runtime (88.748%). Relative to exact base `5a0a52e1`, the pass-set delta
is +8/-0: all 3,609 prior passes remain passes and no other status or failure
category moves. Two final merged manifests and summaries are byte-for-byte
identical. The manifest SHA-256 is
`1897e8bd738e54867c8994d7aaec5daaa7cb6906b6edc97bb85ec5ea25048f7e`.

PHP 8.5 explicit float-to-integer conversion now shares one checked contract
across `(int)`, `intval()` and `settype(..., "int")`. Representable values
truncate normally; finite values outside the AMD64 signed range preserve their
low 64 bits, while INF and NAN become zero. Every non-representable float emits
the exact cast warning before its result. Numeric strings retain their
separate diagnostic-free explicit-conversion rules, including saturation for
out-of-range strings.

Explicit integer and float conversion of an empty or non-empty array now
produces zero or one. Objects produce one after the exact class-aware warning,
and resources retain their numeric ID. These rules apply consistently to cast
syntax, `intval()`/`floatval()` and `settype()`. A throwing warning handler
interrupts an ordinary cast or conversion call, while `settype()` still
commits its by-reference scalar write before propagating the handler exception.
PHP 8.5's adjacent `(string) NAN`, `strval(NAN)` and string `settype()` warning
is also preserved; the wider NAN-to-bool/array/object diagnostic family remains
separate work.

Original E2E coverage exercises ±INF, NAN, both signs of finite overflow, an
ordinary fractional value, arrays, objects, resources, all explicit APIs and
throwing handlers. The exact full-corpus additions are `Zend/tests/bug33999.phpt`,
`Zend/tests/int_overflow_64bit.phpt`,
`Zend/tests/type_coercion/float_to_int/dval_to_lval_64.phpt`,
`explicit_casts_should_not_warn.phpt`,
`warning_float_does_not_fit_zend_long_arrays.phpt`,
`Zend/tests/type_coercion/int_special_values.phpt`,
`Zend/tests/type_coercion/type_casts/cast_to_double.phpt` and
`cast_to_int.phpt`. The unplanned `bug33999`, integer-overflow and array-key
overflow gains follow from the same object and explicit-cast boundary; every
other selected control retains its prior outcome.

All five Cargo configurations, all-feature/all-target, formatting, unsafe
self-test and the exact unsafe ratchet pass, as do Composer S0, all four
Symfony S1 gates and PHP 8.5 warmed-kernel S2 and cold-build S3. The production
inventory remains 1,612 unsafe blocks and 289 unsafe functions. On an AMD Ryzen
9 7950X pinned to one performance-governor CPU, 32 alternating release A/B
pairs with two warmups per binary, JIT and quick loops disabled execute five
million ordinary dynamic double-to-integer casts with identical checksums.
Against the exact base binary, the candidate is +3.359% by independent medians
and +2.987% by the paired-ratio median, within the +5% gate.

In the preceding non-canonical-cast checkpoint, the measured AMD64 PHP 8.5
contract was pinned to php-src
8.5.6 commit `fcc29c8`. Across all 5,599 unmodified `Zend/tests` and
`tests/lang` cases, 3,609 pass, 1,688 fail, 114 skip, one is an upstream XFAIL,
187 are unsupported, and none time out or crash. The headline pass rate is
68.133% and the whole-corpus rate is 64.458%; 4,701 of 5,297 attempted cases
reach runtime (88.748%). Relative to exact base `b2b40557`, the pass-set delta
is +5/-0: all 3,604 prior passes remain passes and no other status or failure
category moves. Two final merged manifests and summaries are byte-for-byte
identical. The manifest SHA-256 is
`ced938e9733fe3996935387d05e38bb696a28948936c48cd99be6a02eba555cb`.

PHP 8.5 non-canonical `(binary)`, `(boolean)`, `(double)` and `(integer)`
casts now retain their established string, boolean, float and integer values
while emitting the exact compile-time deprecation and canonical replacement.
The diagnostics are collected across the complete source unit, survive dead-
branch elimination and execute in source order only after a successful parse.
Cast spellings remain case-insensitive. The removed `(real)` spelling instead
produces PHP 8.5's source-aware parser error directing callers to `(float)`.

Original parser and E2E coverage verifies the four deprecations, their lines
and ordering, a dead-code cast, unchanged values and the removed-cast error.
The exact full-corpus additions are
`Zend/tests/type_coercion/type_casts/non_canonical_binary_cast.phpt`,
`non_canonical_boolean_cast.phpt`, `non_canonical_double_cast.phpt`,
`non_canonical_integer_cast.phpt` and `real_cast.phpt`; all other selected
controls keep their prior outcome. Runtime cast semantics are unchanged.

All five Cargo configurations, all-feature/all-target, formatting, unsafe
self-test and the exact unsafe ratchet pass, as do Composer S0, all four
Symfony S1 gates and PHP 8.5 warmed-kernel S2 and cold-build S3. The production
inventory remains 1,612 unsafe blocks and 289 unsafe functions. No runtime
performance gate applies: the change is confined to cold parser diagnostics,
and every successfully compiled alias emits the same cast opcode as its
canonical spelling.

In the preceding unary-signed-zero checkpoint, the measured AMD64 PHP 8.5
contract was pinned to php-src
8.5.6 commit `fcc29c8`. Across all 5,599 unmodified `Zend/tests` and
`tests/lang` cases, 3,604 pass, 1,693 fail, 114 skip, one is an upstream XFAIL,
187 are unsupported, and none time out or crash. The headline pass rate is
68.039% and the whole-corpus rate is 64.369%; 4,701 of 5,297 attempted cases
reach runtime (88.748%). Relative to exact base `34745ac7`, the pass-set delta
is +3/-0: all 3,601 prior passes remain passes and no other status or failure
category moves. Two final merged manifests and summaries are byte-for-byte
identical. The manifest SHA-256 is
`888b346a7c22d3d057be4894bae20d3e30893a12872277354efe190f4f88ea83`.

Dynamic PHP 8.5 unary minus now lowers through the same operand-first numeric
multiplication contract as unary plus, using `value * -1` instead of
`0 - value`. This preserves IEEE negative zero for a dynamic positive-zero
operand and positive zero for a negative-zero operand, while retaining integer
results, `PHP_INT_MIN` overflow promotion, numeric-string conversion, INF/NAN,
evaluation order and single evaluation. Literal and constant-expression
folding remain unchanged; runtime `-ZERO` now agrees with the already-correct
constant `-ZERO` result.

Original E2E coverage exercises both signs over ±0, ordinary floats and
integers, `PHP_INT_MIN`, ±INF, NAN and a numeric string, then proves a dynamic
operand is evaluated once. The exact full-corpus additions are
`Zend/tests/bug52355.phpt`, `Zend/tests/bug70804.phpt` and
`Zend/tests/unary_minus_const_expr_consistency.phpt`. The established explicit
negative-zero cast control remains an exact pass. Trigonometric precision,
float-to-integer low-bit behavior and float formatting remain separate work.

All five Cargo configurations, all-feature/all-target, formatting, unsafe
self-test and the exact unsafe ratchet pass, as do Composer S0, all four
Symfony S1 gates and PHP 8.5 warmed-kernel S2 and cold-build S3. The production
inventory remains 1,612 unsafe blocks and 289 unsafe functions. On an AMD Ryzen
9 7950X pinned to one performance-governor CPU, 32 alternating release A/B
pairs with two warmups per binary, JIT and quick loops disabled execute five
million dynamic double negations with identical checksums. Against the exact
base binary, the candidate is -2.900% by independent medians and -2.857% by the
paired-ratio median. Favorable medians are control evidence, not a broader
optimization claim.

In the preceding string-float-prefix checkpoint, the measured AMD64 PHP 8.5
contract was pinned to php-src
8.5.6 commit `fcc29c8`. Across all 5,599 unmodified `Zend/tests` and
`tests/lang` cases, 3,601 pass, 1,696 fail, 114 skip, one is an upstream XFAIL,
187 are unsupported, and none time out or crash. The headline pass rate is
67.982% and the whole-corpus rate is 64.315%; 4,701 of 5,297 attempted cases
reach runtime (88.748%). Relative to exact base `fd0298d7`, the pass-set delta
is +3/-0: all 3,598 prior passes remain passes and no other status or failure
category moves. Two final merged manifests and summaries are byte-for-byte
identical. The manifest SHA-256 is
`70b49f99f2e53a7ad13c49e47ff7226d80c19d930db5a0f02f620cc560cbb361`.

Explicit PHP 8.5 string-to-float conversion now shares the complete numeric-
prefix grammar across `(float)`, `floatval()` and `settype(..., "float")`.
Decimal fractions, scientific notation, leading PHP ASCII whitespace and
leading-numeric trailing text convert without diagnostics; incomplete
exponents stop before `e`. Numeric exponent overflow produces signed infinity,
negative zero retains its sign, while Rust-only textual `NaN` and `inf`
spellings remain non-numeric and convert to zero. References are transparent
and non-string conversion behavior is unchanged.

Complete float strings containing an ASCII digit retain a direct parse fast
path. Prefix, trailing-text and invalid cases enter the checked parser already
shared by numeric coercion, so the optimization cannot admit textual special
values. Original E2E coverage exercises 13 boundary values through all three
explicit APIs plus a reference. The complete 19-case `(float)`/`floatval()`
audit moves from five to eight exact passes with two unchanged skips. The exact
full-corpus additions are
`Zend/tests/numeric_strings/explicit_cast_leading_numeric_must_work.phpt`,
`tests/lang/bug73329.phpt` and `tests/lang/string_decimals_001.phpt`.
Object/array/resource conversion diagnostics, unary signed-zero propagation,
non-canonical cast aliases, integer-literal parsing and constexpr object
support remain separate visible work.

All five Cargo configurations, all-feature/all-target, formatting, unsafe
self-test and the exact unsafe ratchet pass, as do Composer S0, all four
Symfony S1 gates and PHP 8.5 warmed-kernel S2 and cold-build S3. The production
inventory remains 1,612 unsafe blocks and 289 unsafe functions. On an AMD Ryzen
9 7950X pinned to one performance-governor CPU, 32 alternating release A/B
pairs with two warmups per binary, JIT and quick loops disabled execute five
million complete decimal-string casts with identical checksums. Against the
exact base binary, the candidate is +0.701% by independent medians and +0.888%
by the paired-ratio median, within the +5% gate. The initial parser-only variant
was rejected at +7.879% and +8.279%, respectively; the complete-string fast
path restored the common case without weakening the PHP contract.

In the preceding scientific-string integer-cast checkpoint, the measured
AMD64 PHP 8.5 contract was pinned to php-src
8.5.6 commit `fcc29c8`. Across all 5,599 unmodified `Zend/tests` and
`tests/lang` cases, 3,598 pass, 1,699 fail, 114 skip, one is an upstream XFAIL,
187 are unsupported, and none time out or crash. The headline pass rate is
67.925% and the whole-corpus rate is 64.261%; 4,701 of 5,297 attempted cases
reach runtime (88.748%). Relative to exact base `1d3b31af`, the pass-set delta
is +1/-0: all 3,597 prior passes remain passes and no other status or failure
category moves. Two final merged manifests and summaries are byte-for-byte
identical. The manifest SHA-256 is
`3eaa6c3c929b8843ed263d9349ada401259523c0c2d10df4962e1d7c4226bddf`.

Explicit PHP 8.5 string-to-integer conversion now uses the complete numeric-
prefix grammar at both `(int)` and `intval()` boundaries. Decimal fractions,
scientific notation, leading PHP ASCII whitespace and leading-numeric trailing
text convert without the warnings or precision deprecations required by
implicit arithmetic. Incomplete exponents stop before `e`; finite float syntax
saturates at the AMD64 integer bounds, non-finite syntax converts to zero, and
overflowing decimal integer strings clamp by sign. Non-numeric strings remain
zero. References are transparent and non-string conversion behavior is
unchanged.

The conversion reuses the checked integer-operator parser while discarding its
arithmetic-only diagnostic descriptor. A narrow fast path handles complete,
representable decimal integer strings, including surrounding ASCII whitespace;
fractional, exponent, leading-numeric and overflow cases enter the general
parser. Original E2E coverage exercises casts and `intval()` together across
both paths, references, incomplete exponents, finite and non-finite overflow,
integer overflow and non-numeric input. The 12-case adjacent cluster moves from
two to three exact passes. The sole full-corpus addition is
`Zend/tests/int_conversion_exponents.phpt`; all other selected controls keep
their prior outcome. At that checkpoint, prefix `(float)` conversion, float-to-
integer low-bit and diagnostic behavior, non-canonical cast deprecation,
object/array/resource cast details, optional `intval()` bases and constexpr
object support remained separate visible work.

All five Cargo configurations, all-feature/all-target, formatting, unsafe
self-test and the exact unsafe ratchet pass, as do Composer S0, all four
Symfony S1 gates and PHP 8.5 warmed-kernel S2 and cold-build S3. The production
inventory remains 1,612 unsafe blocks and 289 unsafe functions. On an AMD Ryzen
9 7950X pinned to one performance-governor CPU, 32 balanced release A/B pairs
with JIT and quick loops disabled execute five million ordinary numeric-string
casts with identical checksums. Against the exact base binary, the candidate is
-7.599% by independent medians, -7.579% by the paired-ratio median and -7.545%
after balancing order-specific median ratios, within the +5% gate. The initial
general-parser-only variant was rejected at a +19.187% balanced regression;
the complete-decimal fast path restored the common case without weakening the
PHP conversion contract. Favorable final medians are control evidence, not a
broader optimization claim.

In the preceding integer-operator checkpoint, the measured AMD64 PHP 8.5
contract was pinned to php-src
8.5.6 commit `fcc29c8`. Across all 5,599 unmodified `Zend/tests` and
`tests/lang` cases, 3,597 pass, 1,700 fail, 114 skip, one is an upstream XFAIL,
187 are unsupported, and none time out or crash. The headline pass rate is
67.906% and the whole-corpus rate is 64.244%; 4,701 of 5,297 attempted cases
reach runtime (88.748%). Relative to exact base `0c1c67e7`, the pass-set delta
is +28/-0: all 3,569 prior passes remain passes and no status regresses. Two
final merged manifests and summaries are byte-for-byte identical. The manifest
SHA-256 is
`1482a871c31cd810f7e14676be9a1e65ec2122f4f5f61134a28a1201eae36c41`.

PHP 8.5 integer-only operators now share a checked scalar-conversion boundary.
Modulo, bitwise AND/OR/XOR and shifts accept the same booleans, nulls,
resources, floats, numeric strings and leading-numeric strings, including their
compound-assignment forms. Bitwise NOT applies the same conversion outside its
separate byte-string operation. Fractional floats and float-strings emit the
exact implicit-conversion deprecation; non-representable floats emit PHP's
cast warning, and `NAN` preserves the warning-then-deprecation order. A
leading-numeric suffix warns before execution. Invalid arrays and objects
produce the operator-specific catchable `TypeError`, while modulo diagnostics
precede the zero-divisor exception. `PHP_INT_MIN % -1` remains defined.

The checked descriptor separates the converted `i64` from its observable
diagnostics. Constant evaluation therefore folds only diagnostic-free cases
and leaves warning or deprecation cases for runtime. Existing Long/Long modulo
and bitwise paths remain direct, and JIT side exits now resume the canonical
warning-plus-conversion contract instead of preserving the former fatal.
Original E2E coverage combines exact and fractional floats, float-strings,
leading-numeric text, non-representable values, `NAN`, compound assignment,
invalid operand types and modulo by zero. The focused 16-case upstream cluster
moves from 0 to 11 exact passes; the other five execute their integer-operator
sections correctly before reaching separately scoped behavior.

The exact Zend additions are `bitwise_not_precision_exception.phpt`,
`bug69957.phpt`, `mod_001.phpt`, `not_001.phpt`, `self_and.phpt`,
`self_mod.phpt`, `self_or.phpt`, `self_xor.phpt`, `xor_001.phpt`, the five
compatible float/float-string cases under `type_coercion/float_to_int`, both
assignment-operator warning cases there, and
`non-rep-float-as-int-extra1.phpt`. The eleven `tests/lang/operators`
additions cover the AMD64 long variants of AND, NOT, OR, XOR, left shift,
right shift and modulo plus three string-variation shift cases and the modulo
string variation.

At that checkpoint, seven remaining failures moved only beyond their former
operator rejection.
`int_conversion_exponents.phpt` reaches the separate scientific-string
`(int)`/`intval()` boundary; `not_002.phpt` reaches fatal-backtrace formatting;
the float-literal and float-variable warning cases reach independent string-
offset semantics; their two float-string counterparts reach weak call/property
conversion diagnostics; and `tests/lang/024.phpt` reaches an included-file parser
boundary. These remain visible failures and are not claimed here.

All five Cargo configurations, all-feature/all-target, formatting, unsafe
self-test and the exact unsafe ratchet pass, as do Composer S0, all four
Symfony S1 gates and PHP 8.5 warmed-kernel S2 and cold-build S3. The production
inventory is 1,612 unsafe blocks and 289 unsafe functions. On an AMD Ryzen 9
7950X pinned to one performance-governor CPU, 32 balanced release A/B pairs
with JIT and quick loops disabled retain the ten-million-iteration Long modulo
gate under its +1% ceiling: the candidate is -4.096% by independent medians,
-3.995% by the paired-ratio median and -3.810% after balancing order-specific
medians. Every sample has the identical checksum. Favorable medians are control
evidence, not a broader optimization claim.

In the preceding compound-comparison checkpoint, the measured AMD64 PHP 8.5
contract was pinned to php-src 8.5.6 commit `fcc29c8`. Across all 5,599
unmodified `Zend/tests` and `tests/lang` cases, 3,569 pass, 1,728 fail, 114
skip, one is an upstream XFAIL, 187 are unsupported, and none time out or
crash. The headline pass rate is 67.378% and the whole-corpus rate is 63.744%;
4,684 of 5,297 attempted cases reach runtime (88.427%). Relative to exact base
`1f6178d3`, the pass-set delta is +28/-0: all 3,541 prior passes remain passes
and no status regresses. Two final merged manifests and summaries are byte-for-
byte identical. The manifest SHA-256 is
`8b7fc631ad7bb9e76c26bbf68f03209413daea4bd474723d99aaa5f64c6a6176`.

PHP 8.5 loose equality, relational and spaceship operations now share a
canonical checked comparison fallback for compound and cross-type operands.
Boolean and null truthiness precedes numeric coercion; complete numeric strings
compare numerically while non-numeric number/string pairs use PHP's lexical
boundary. Arrays retain PHP's directional ordering against scalars, objects
and closures. Runtime object/scalar comparisons perform the required numeric
notice or checked `__toString()` conversion, including reentrant user code,
and propagate it through nested arrays and object properties, while recursive
comparison retains live operands and reports PHP's recursive-dependency error.
NaN retains PHP's unordered relational result while spaceship normalizes that
boundary to `1`. Constant evaluation uses the same pure comparison rules.
Existing integer, double and string opcode paths remain direct scalar fast
paths; only their non-scalar cases enter the cold fallback.

Original E2E regressions exercise the complete scalar/array/object matrix,
numeric-string boundaries, resources, enums, closures, recursive arrays,
NaN, invalid `__toString()` returns and a reentrant string cast that mutates the
compared object. The complete focused 22-case cluster is exact in 20 cases.

The exact full-corpus additions are `Zend/tests/bug69891.phpt`,
`Zend/tests/clone/bug24884.phpt`, comparison cases `compare_001_64bit.phpt`,
`003`, `004`, `005` and `006`, enum cases `comparison.phpt` and `gh16954.phpt`,
`Zend/tests/foreach/gh11222.phpt`, `gh19305-001.phpt`, `gh19305-002.phpt`,
`gh418106144.phpt`,
`match/gh11134.phpt`, `object-null.phpt`, object cases `objects_001.phpt` and
`objects_015.phpt`, `optimizer/nan_warning_switch.phpt`,
`oss_fuzz_434346548.phpt`, `switch/switch_on_numeric_strings.phpt`, temporary
cleaning cases `004` and `005`, and the six `tests/lang/operators` cases for
equals, greater-than, greater-than-or-equal, less-than, less-than-or-equal and
spaceship. `gh19305-001.phpt` specifically crosses the nested object-property
conversion boundary.

Seven remaining failures advance beyond their former compile rejection, with
no pass loss. `bug32322.phpt` reaches its separate by-reference/destructor
output boundary; `bug42143.phpt` reaches the missing `M_PI` constant;
`numeric_strings/trailling_whitespaces.phpt` reaches the vertical-tab/form-feed
lexer boundary; `type_coercion/nan_comp_op.phpt` now has exact comparison
results and differs only in `var_export(NAN)` spelling; `bug21961.phpt` reaches
the non-static-method static-call boundary; `foreachLoopObjects.004.phpt`
reaches missing-property warnings; and `string_decimals_001.phpt` reaches its
independent prefix float-cast behavior. These remain visible output or runtime
failures and are not claimed by this checkpoint.

All five Cargo configurations, all-feature/all-target, formatting, unsafe
self-test and the exact unsafe ratchet pass, as do Composer S0, all four
Symfony S1 gates and PHP 8.5 warmed-kernel S2 and cold-build S3. The production
inventory is 1,606 unsafe blocks and 289 unsafe functions. On an AMD Ryzen 9
7950X pinned to one performance-governor CPU, 32 balanced A/B pairs retain the
scalar gates under the +1% ceiling: the general CV/CV branch loop has a -6.75%
independent-median delta and -6.68% balanced paired delta; the quick-loop-
disabled CV/constant loop has -0.76% and -0.95%, respectively. Exact output
checksums match for every sample. Favorable medians are control evidence, not
a broader optimization claim.

In the preceding namespace-relative catch checkpoint, the measured AMD64 PHP
8.5 contract was pinned to php-src
8.5.6 commit `fcc29c8`. Across all 5,599 unmodified `Zend/tests` and
`tests/lang` cases, 3,541 pass, 1,756 fail, 114 skip, one is an upstream XFAIL,
187 are unsupported, and none time out or crash. The headline pass rate is
66.849% and the whole-corpus rate is 63.243%; 4,653 of 5,297 attempted cases
reach runtime (87.842%). Relative to exact base `3b0ad5ab`, the pass-set delta
is +1/-0: all 3,540 prior passes remain passes and no other status, failure
category or stage moves. Two final merged manifests and
summaries are byte-for-byte identical. The manifest SHA-256 is
`ac77eb9d1e0d657b4980e66327a770c3dd62ab9e993fb0c01971ef9c071342ca`.

PHP 8.5 catch type lists now admit explicit namespace-relative names such as
`namespace\Problem`. The same qualified-or-namespace-relative name parser is
shared with trait references, and is applied independently to every union
catch member. Ordinary relative and leading-backslash absolute types remain
unchanged, as do catch clauses with an exception variable and PHP 8's
variable-free form.

An original parser regression retains the exact name representations for a
mixed three-type catch union and a variable-free namespace-relative catch. A
namespace E2E regression throws into both forms and proves runtime class
resolution, union ordering and the optional-variable boundary. The complete
parser unit set, namespace E2E file and adjacent `traits` plus `exceptions`
PHPT directories pass with no unrelated transition.

The sole full-corpus addition is `Zend/tests/traits/bug55086.phpt`, which now
passes through its previously unreachable `catch (namespace\Foo $e)` tail
after preserving the already-supported trait import and adaptation behavior.

All five Cargo configurations, all-feature/all-target, formatting, unsafe
self-test and the exact unsafe ratchet pass, as do Composer S0, all four
Symfony S1 gates and PHP 8.5 warmed-kernel S2 and cold-build S3. The production
inventory remains at 1,623 unsafe blocks and 289 unsafe functions. No runtime
performance gate applies because the change only selects an existing parser
and name-resolution representation; successful bytecode paths are unchanged.

In the preceding semi-reserved member checkpoint, PHP 8.5 tokens share the
contextual-name path used by
method declarations, instance and static member access, trait adaptations and
named arguments. This admits logical `and`/`or`, `insteadof` and magic-constant
spellings without changing their expression meanings. Trait adaptations use
one reference parser across classes, anonymous classes, traits and enums: a
bare keyword is a method, while qualified and explicit `namespace\Trait`
forms identify the trait before `::`. Precedence exclusion lists accept the
same namespace-relative trait form. Legacy `var` is parsed as PHP's public,
untyped property modifier rather than as a class type.

Original parser coverage exercises ordinary, static and magic-constant method
names, instance/static calls, every trait consumer shape, bare keyword aliases,
precedence for a method named `insteadof`, namespace-relative exclusions and a
legacy `var` property. An E2E regression executes declaration, alias,
precedence, static-call, magic-name and property boundaries together. The
complete ten-case `Zend/tests/grammar/semi_reserved_00*.phpt` family is now
exact.

The exact full-corpus additions are `Zend/tests/bug18556.phpt`, `bug47165.phpt`,
`bug55305.phpt`, `Zend/tests/gc/bug72530.phpt`,
`Zend/tests/grammar/regression_001.phpt`, semi-reserved cases `001`, `002`,
`003`, `006`, `008`, `009` and `010`,
`Zend/tests/property_hooks/var_property.phpt`,
`Zend/tests/traits/bug77966.phpt`,
`Zend/tests/type_declarations/dnf_types/gh9500.phpt` and
`tests/lang/execution_order.phpt`.

Three non-pass outcomes move only after their previously misparsed `var`
property becomes valid. `bug75573.phpt` reaches its independent overloaded
`__get()`/`__set()` reference and output gap; `get_defined_vars.phpt` reaches
the missing CLI `argc` metadata boundary; and `magic_methods/bug39775.phpt`
reaches the missing indirect-overloaded-property modification notice. They
remain visible failures. `traits/bug55086.phpt` is unchanged at its separate
`catch (namespace\Type)` parsing boundary.

All five Cargo configurations, all-feature/all-target, formatting, unsafe
self-test and the exact unsafe ratchet pass, as do Composer S0, all four
Symfony S1 gates and PHP 8.5 warmed-kernel S2 and cold-build S3. The production
inventory remains at 1,623 unsafe blocks and 289 unsafe functions. No runtime
performance gate applies because the change is confined to contextual parsing
and declaration lowering; previously valid bytecode and execution paths are
unchanged.

In the preceding `strict_types` checkpoint, PHP 8.5 declarations retain
source-unit placement state.
Empty statements and earlier `declare` directives preserve eligibility, while
the first non-declare statement ends it for the complete source unit,
including namespace and nested parser paths. A later `strict_types` directive
produces PHP's compile-time
`strict_types declaration must be the very first statement in the script`
fatal at the directive's physical source line. Block mode produces
`strict_types declaration must not use block mode`; when both rules are
violated, the earlier placement diagnostic wins.

Original parser and source-aware E2E regressions cover valid first, weak,
duplicate and declare-prefixed directives, a leading empty statement,
namespace and function predecessors, both block-mode boundaries, exact source
locations and first-deferred-error priority. The parser consumes an invalid
block before replaying that first error, so a later nested compile error cannot
replace it.

The exact full-corpus additions are
`Zend/tests/type_declarations/scalar_strict_declaration_placement_001.phpt`,
`002.phpt`, `008.phpt` and
`Zend/tests/type_declarations/strict_nested.phpt`. The adjacent positive
`004.phpt` and `005.phpt` cases remain exact passes; `006.phpt` and `007.phpt`
remain unsupported for their existing CLI/INI requirements. Placement case
`003.phpt`, whose source starts with inline HTML before the opening PHP tag,
remains a separate lexer-ownership checkpoint. Line-exact diagnostics for the
currently line-less `static` and `unset` keyword tokens are likewise not
claimed here.

All five Cargo configurations, all-feature/all-target, formatting, unsafe
self-test and the exact unsafe ratchet pass, as do Composer S0, all four
Symfony S1 gates and PHP 8.5 warmed-kernel S2 and cold-build S3. The production
inventory remains at 1,623 unsafe blocks and 289 unsafe functions. No runtime
performance gate applies because the change only rejects PHP-invalid
declaration placement during parsing and leaves successful bytecode unchanged.

In the preceding call-result write-context checkpoint, exact call results used
as write targets produce PHP 8.5's compile-time
`Can't use function return value in write context` fatal, substituting
`method` where required and retaining the target's physical source line.

Direct and reference assignment, `??=`, compound assignment,
`unset()` and foreach targets share the classifier already used by inc/dec and
destructuring. The parser consumes the complete construct and retains the first
deferred error, so a later RHS error or dead-code elimination cannot replace
the write-context diagnostic.

Original parser and source-aware E2E regressions cover named, dynamic, instance
and static calls across every admitted write context, target-before-RHS
priority, exact source locations and both diagnostic kinds. The classifier
does not mistake contextual `list(...)` syntax for a function call and does
not recurse through an indexed dimension or property of a call result. The
pre-existing empty-append `call()[]` parser gap remains outside this checkpoint.

The exact full-corpus additions are
`Zend/tests/coalesce/assign_coalesce_004.phpt`,
`Zend/tests/errmsg/errmsg_004.phpt`,
`Zend/tests/errmsg/errmsg_005.phpt` and
`Zend/tests/unset/bug70240.phpt`. The adjacent inc/dec, pipe and property-hook
cases remain exact passes, and no other status, category or stage moves.

All five Cargo configurations, all-feature/all-target, formatting, unsafe
self-test and the exact unsafe ratchet pass, as do Composer S0, all four
Symfony S1 gates and PHP 8.5 warmed-kernel S2 and cold-build S3. The production
inventory remains at 1,623 unsafe blocks and 289 unsafe functions. No runtime
performance gate applies because the change only records PHP-invalid
compile-time write targets and leaves successful bytecode unchanged.

In the preceding literal-`$this` checkpoint, literal writes produce PHP 8.5's
compile-time fatal
`Cannot re-assign $this` with the target's physical source line. The parser
consumes the complete construct while retaining the first deferred error, so
the diagnostic survives dead-code elimination and takes priority over a later
compile error in the right-hand side. Direct and reference assignment, `??=`,
foreach keys, values, references and destructuring targets, and catch variables
share the same boundary. Property writes through `$this->property` remain
ordinary writable targets.

Original parser and source-aware E2E regressions cover every admitted grammar
shape, uncalled method bodies, RHS diagnostic priority and exact source
location. The exact full-corpus additions are
`Zend/tests/coalesce/assign_coalesce_005.phpt`,
`Zend/tests/errmsg/errmsg_003.phpt`, all four
`Zend/tests/foreach/this_in_foreach_00*.phpt` cases,
`Zend/tests/this-reserved/030.phpt`,
`Zend/tests/this-reserved/this_in_catch.phpt` and
`tests/lang/bug24573.phpt`. No other status, category or stage moves. Dynamic
`$$name` rebinding remains a separate runtime `Error` contract, while compound
operations and array dimensions on the receiver are not reclassified by this
checkpoint.

All five Cargo configurations, all-feature/all-target, formatting, unsafe
self-test and the exact unsafe ratchet pass, as do Composer S0, all four
Symfony S1 gates and PHP 8.5 warmed-kernel S2 and cold-build S3. The production
inventory remains at 1,623 unsafe blocks and 289 unsafe functions. No runtime
performance gate applies because the change is confined to PHP-invalid
compile-time write targets and leaves successful bytecode unchanged.

In the preceding positional-argument checkpoint, an argument following a named
argument records PHP 8.5's compile-time fatal error instead of terminating
parsing. The shared argument
parser consumes the remaining source and retains the first deferred error, so
dead branches, nested calls, attributes and `new` expressions in constant
initializers all fail at the same compile boundary. Valid positional, named and
unpacked argument lowering remains unchanged.

Original parser and source-aware E2E regressions cover dead code, later named
arguments, nested calls, attributes and constant initializers. The exact
full-corpus additions are
`Zend/tests/call_user_functions/call_user_func_array_array_slice_named_args.phpt`,
`Zend/tests/constexpr/new_positional_after_named.phpt` and both selected
`Zend/tests/named_params/*positional_after_named.phpt` cases. No other status,
category or stage moves. Runtime construction of named argument arrays and the
separate positional-after-unpack diagnostic remain outside this checkpoint.

All five Cargo configurations, all-feature/all-target, formatting, unsafe
self-test and the exact unsafe ratchet pass, as do Composer S0, all four
Symfony S1 gates and PHP 8.5 warmed-kernel S2 and cold-build S3. The production
inventory remains at 1,623 unsafe blocks and 289 unsafe functions. No runtime
performance gate applies because the change only defers an already-fatal
parser branch and leaves successful call bytecode unchanged.

In the preceding `isset()` checkpoint, applying `isset()` to a function result
or any other non-variable expression produces PHP 8.5's compile-time fatal
diagnostic, including its `null !== expression` alternative, source file and
physical line. The parser records the first invalid operand as a deferred
compile error so dead branches
cannot suppress it, while still consuming the complete source unit. Valid
single- and multi-target `isset()` execution is unchanged.

An original parser regression proves that a later invalid operand remains a
compile error inside dead code. Source-aware E2E cases cover arithmetic and
function-call results, multiline calls and multi-target lists. Both
`Zend/tests/isset/isset_expr_error.phpt` and `isset_func_error.phpt` are the
only full-corpus transitions, from parser failures to exact passes. This
checkpoint does not claim `unset()` write-context diagnostics or exact lines
for a line-less literal token placed below the opening parenthesis.

All five Cargo configurations, all-feature/all-target, formatting, unsafe
self-test and the exact unsafe ratchet pass, as do Composer S0, all four
Symfony S1 gates and PHP 8.5 warmed-kernel S2 and cold-build S3. The production
inventory remains at 1,623 unsafe blocks and 289 unsafe functions. No runtime
performance gate applies because the change only records an already-fatal
compile-time path and leaves valid `isset()` lowering and execution unchanged.

In the preceding call-list checkpoint, function, constructor, method,
invokable-object and closure calls share PHP 8.5's argument-list delimiter
state with `isset()` and `unset()`. One trailing comma is valid after a
positional, named or unpacked argument. A leading comma reports an unexpected
token, while a second comma after a
completed item additionally reports that the closing parenthesis was expected.

The positive call fixture also exposed that closure `__FUNCTION__` and
`__METHOD__` values must use the callable's public lexical identity. Ordinary,
arrow, method-scoped and nested closures now reuse the source-qualified name
already retained for traces instead of collapsing to `{closure}`. This changes
only the compile-time magic-constant literal; closure dispatch and storage are
unchanged.

Original parser and E2E regressions cover positional, named and unpacked call
boundaries, constructors, methods, invokable objects, closures, `isset()`,
`unset()`, exact leading/double-comma diagnostics and source-qualified magic
names. The five selected `Zend/tests/function_arguments/call_with_*comma*.phpt`
cases become exact, as do `Zend/tests/autoload/bug26697.phpt`,
`Zend/tests/exceptions/bug31102.phpt` and
`Zend/tests/nested_method_and_function.phpt`. These are the only full-corpus
transitions. This checkpoint does not claim exact punctuation lines when a
malformed comma is on a later physical line than the call opening, nor does it
claim unrelated closure visibility gaps.

All five Cargo configurations, all-feature/all-target, formatting, unsafe
self-test and the exact unsafe ratchet pass, as do Composer S0, all four
Symfony S1 gates and PHP 8.5 warmed-kernel S2 and cold-build S3. The production
inventory remains at 1,623 unsafe blocks and 289 unsafe functions. No runtime
performance gate applies: valid call bytecode is unchanged, rejected forms are
parser-only and closure magic names are constructed only while compiling an
explicit magic constant.

In the preceding group-use checkpoint, malformed group `use` declarations
report PHP 8.5's active parser state.

An empty group or leading comma expects an identifier or namespaced name and,
for a mixed group, also names the admissible `function` and `const` kinds. A
second comma after a completed item instead expects the closing brace. Typed
groups and explicit typed items use the narrower expectation, while a single
trailing comma remains valid.

An original parser regression covers mixed and typed empty groups, leading and
double commas, an explicit `function` item, source-unit diagnostic formatting
and the valid trailing-comma boundary. All eight
`Zend/tests/group_use/ns_trailing_comma_error_01.phpt` through `08.phpt` cases
are the exact full-corpus additions. This checkpoint does not claim exact token
line reporting when the malformed punctuation is placed on a later physical
line than the `use` keyword.

All five Cargo configurations, all-feature/all-target, formatting, unsafe
self-test and the exact unsafe ratchet pass, as do Composer S0, all four
Symfony S1 gates and PHP 8.5 warmed-kernel S2 and cold-build S3. No runtime
performance gate applies: the change is confined to already-rejected parser
states and does not alter generated bytecode or execution.

In the preceding generator checkpoint, when a generator is interrupted while
evaluating arguments for a function,
method, constructor or dynamic callable, operands evaluated before the
`yield` now keep PHP 8.5's value at that evaluation boundary. Late-bound calls
retain both a by-value snapshot and the original CV lvalue until signature
resolution selects the correct one; this applies to positional and named
arguments. Method receivers and dynamic callable operands are likewise
snapshotted before the suspension, while declared by-reference parameters and
lexical reference captures retain their original PHP cells through every
detach/materialize cycle.

Detached generator activations expose their saved CV, TMP, yielded key/value,
return, closure-static and array-delegate values to the request-local cycle
graph. Removing the last userland root therefore collects a generator that
references itself from an interrupted call and runs objects retained directly
or through a bound closure in PHP destructor order. Internal Generator chains
are flattened iteratively during destructor discovery, preserving the existing
10,000-level `yield from` release guarantee. `Closure::call()` also keeps its
temporary bound `$this` when the invoked closure returns a generator.

Four original E2E regressions cover rooted and unrooted collection, bound and
direct destructor ordering, positional and named runtime by-value/by-reference
selection after external mutation, constructor/reference behavior, callable
replacement, `Closure::call()` receiver lifetime and deep delegated release.
All eleven `Zend/tests/generators/gh9750-001.phpt` through `011.phpt` cases and
the adjacent `Zend/tests/closures/bug70397.phpt` are the exact full-corpus
additions.

All five Cargo configurations, all-feature/all-target, formatting, unsafe
self-test and the exact unsafe ratchet pass, as do Composer S0, all four
Symfony S1 gates and PHP 8.5 warmed-kernel S2 and cold-build S3. The production
inventory remains at its ceiling of 1,623 unsafe blocks and 289 unsafe
functions. On one pinned AMD64 CPU, the established 200-pair generator-resume
gate retains checksum `19999900000` and measures -1.532% by its balanced
order-specific median ratio, below the +1% regression ceiling. This is evidence
of no generator-resume regression, not an optimization claim.

This checkpoint does not claim that RPHP's public `gc_collect_cycles()` count
includes PHP's internal suspended-frame bookkeeping node; collection timing,
destructor effects and selected PHPT output are exact, but a diagnostic probe
can report one fewer collected element. Broader weak-reference temporary
retention, generator/fiber crossings, optional extension suites and general
PHP compatibility remain outside this checkpoint.

In the preceding relative-callable checkpoint, PHP 8.5's deprecated relative
callable spellings share one lexical, late-called and receiver-aware resolver.
String and two-element array forms
using `self`, `parent` or `static`, plus qualified arrays such as
`["Child", "parent::method"]`, retain forwarding scope, method visibility and
the live compatible receiver. Static targets keep late-static identity,
non-static targets bind `$this`, and missing methods select `__call` before
`__callStatic` when scoped static syntax still has an instance. Trait bytecode
uses the exact composition selected by dispatch, including one-step parent
forwarding rather than recursing through the dynamic receiver.

`call_user_func()`, `call_user_func_array()`, `array_map()`, `is_callable()`,
callable parameter validation, `Closure::fromCallable()` and
`register_shutdown_function()` deliver the PHP 8.5 deprecation at their
physical source boundary. A throwing diagnostic handler interrupts
consumption with PHP's outer callback `TypeError` and chained previous
exception where required, without leaving a pending callback frame. Detached
user callbacks retain surplus trace arguments, so error handlers and
Throwable snapshots report the same argument list and source line as PHP.
Ordinary function and static-method callbacks continue through their existing
monomorphic cache before any legacy-shape work.

Eight original E2E regressions cover lexical versus called scope, instance and
static targets, every admitted consumer, qualified visibility and error text,
trait parent composition, throwing handlers and cleanup, global invalid scope,
instance magic dispatch and shutdown lifetime. A syntax-selected 33-case
focused PHP 8.5 gate has 31 passes, one platform skip and only the pre-existing
`semi_reserved_003.phpt` parser failure. The 30 exact full-corpus additions are
`autoload/bug37138.phpt`, `bug41026.phpt`, both `bug48899` cases,
`call_user_functions/bug32290.phpt`, `bug66719.phpt`,
`callable_self_parent_static_deprecation.phpt`, `closures/closure_030.phpt`,
the five affected `dynamic_call` cases, `exceptions/bug51394.phpt`, all four
affected `gh16799`/`gh_21699` cases, `is_callable_trampoline_uaf-deprecated.phpt`,
five affected `lsb` cases, three affected magic-method cases, `match/029` and
`030`, and `traits/bug76773-deprecated.phpt`.

All five Cargo configurations, all-feature/all-target, formatting, unsafe
self-test and the exact unsafe ratchet pass, as do Composer S0, all four
Symfony S1 gates and PHP 8.5 warmed-kernel S2 and cold-build S3. The production
inventory remains at its ceiling of 1,623 unsafe blocks and 289 unsafe
functions. On one pinned AMD64 CPU, 32 order-balanced release pairs retain
checksum `12500002500000` and put five million ordinary `call_user_func()`
calls at +2.311% and five million scoped static callback calls at -0.415% by
the gate's balanced order-specific median ratio, both below the +5% regression
ceiling. This is evidence of no callback-dispatch regression, not an
optimization claim.

That checkpoint does not claim deprecated relative-scope behavior for every
unselected standard-library callback consumer, the remaining semi-reserved
grammar gap, optional extension suites outside the selected corpus or broader
PHP syntax and runtime compatibility.

In the preceding trait-property checkpoint, ordinary class property defaults
evaluate `__CLASS__` in their declaring class.

Trait instance and static property defaults containing `__CLASS__` or
`self::class` instead bind at every composition boundary, including nested
traits and expressions such as concatenation. A class that merely inherits a
composed instance property keeps the parent's value, while explicit trait
reuse creates the child's value. Compatible trait-property collisions compare
the values for the current composer and preserve the first declaration's
rebinding recipe, matching PHP's order-dependent nested-trait contract.

Inherited trait-composed static storage remains shared with its parent, so its
actual value does not change. PHP 8.5 nevertheless exposes a child-relative
static default through `ReflectionClass::getDefaultProperties()`; RPHP now
rebinds only the copied declaration metadata for that view. Explicit trait
reuse still creates independent static storage with the new consumer's value.

The compiler eagerly validates each default in its source trait and retains
only consumer-relative expressions in the existing cold deferred-default
sidecar. Class registration evaluates those recipes before property collision
checks and carries forward only the first accepted recipe. Final classes keep
static recipes solely for descendant Reflection metadata; instance object
templates are already final. This does not enlarge `ClassDef` or
`PropertyDefinition`, and classes with metadata-only recipes bypass runtime
default materialization during object construction.

Original E2E regressions cover direct class defaults, `__CLASS__`,
`self::class`, concatenation, instance and static properties, nested traits,
inheritance, explicit reuse, unrelated consumers, child Reflection metadata
and composition-order collisions. `bug55214.phpt` and `bug76539.phpt` are the
only full-corpus transitions, from compile/output failures to exact passes;
there is no lost pass or other category/stage movement. Two final manifests
and summaries are byte-for-byte identical. The manifest SHA-256 is
`5e5284349a91b036f40dba081c0442ebd20ccaae1ba4c285df783b02c7d26c5c`.

All five Cargo configurations, all-feature/all-target, formatting, unsafe
self-test and the exact unsafe ratchet pass, as do Composer S0, all four
Symfony S1 gates and PHP 8.5 warmed-kernel S2 and cold-build S3. The production
inventory remains at its ceiling of 1,623 unsafe blocks and 289 unsafe
functions. On one pinned AMD64 CPU, 32 order-balanced release pairs, each
measuring five requests of 1,000 ordinary trait-property compositions, retain
output `1000` and measure 0.403266 seconds for the exact baseline versus
0.399591 seconds for the candidate: -0.911% independently, -0.508% by paired
means and +0.324% by the paired median, below the +5% regression gate. This is
evidence of no link-path regression, not an optimization claim.

This checkpoint does not claim `parent::class` in trait property defaults,
other declaration magic constants, deprecated parent callables, missing
standard-library functions, optional extension suites outside the selected
corpus or broader PHP syntax and runtime compatibility.

In the preceding trait-method checkpoint, `__CLASS__` inside a trait method or
nested closure names the nearest final class that composed that trait body.
Inheriting a composed method keeps
the parent's composition, reusing the trait in a child creates the child's
composition, and a nested trait follows its final consumer. Static calls,
aliases, private dispatch and a reentrant `parent::method()` select the exact
body that was dispatched; a static closure can therefore retain its lexical
trait composition while `static::class` follows a different late-called
class. `__TRAIT__`, `__FUNCTION__` and `__METHOD__` retain their original trait
identity.

The compiler reserves a hidden function-local value only for trait-bound
`__CLASS__` reads. Call initialization fills it with the selected composition
for that activation, so recursive calls through another composition cannot
overwrite their caller. Closures carry that lexical composition separately
from late-static scope, and method caches retain the guarded composition ID.
Frame-free call plans are disabled only for affected bodies; the typed string
length path consumes the same guarded value.

An original E2E regression covers nested traits, inherited and repeated reuse,
alternating cache entries, instance and static calls, escaping and static
closures, aliases, private dispatch and reentrant parent calls. The six-case
focused slice makes `bug65419.phpt`, `bug76773.phpt` and `gh14009_005.phpt`
exact. The deprecated-callable peer, trait-property-default peer and missing
`get_defined_constants()` peer remain at their independent output, compile and
runtime boundaries.

Those same three tests make the only full-corpus fail/output-to-pass
transitions; no prior pass, remaining classification or execution stage moves.
Two final manifests and summaries are byte-for-byte identical. The manifest
SHA-256 is
`237c849d564ff8ae7c884d6afdd046d9de171841ce50ee1797941a135d030441`.

All five Cargo configurations, all-feature/all-target, formatting, unsafe
self-test and the exact unsafe ratchet pass, as do Composer S0, all four
Symfony S1 gates and PHP 8.5 warmed-kernel S2 and cold-build S3. The production
inventory remains within its committed ceiling at 1,623 unsafe blocks and 289
unsafe functions. On one pinned AMD64 CPU, 32 balanced alternating release
pairs measure the five-million-call trait `__CLASS__` path at 1.032805 seconds
for the exact baseline and 1.079833 seconds for the candidate: +4.553% by
independent means and +4.646% by paired ratios, below the +5% gate. A 1,000-
composition cold-link control measures -0.645% by independent means and
-0.298% paired; this is evidence of no regression, not an optimization claim.

That preceding checkpoint did not claim trait property-default magic constants,
deprecated `is_callable(['parent', ...])` behavior, missing standard-library
functions, optional extension suites outside the selected corpus or broader
PHP syntax and runtime compatibility.

The preceding get-parent-class-noarg-deprecation checkpoint was pinned to
php-src 8.5.6 commit `fcc29c8`. Across all 5,599 unmodified `Zend/tests` and
`tests/lang` cases, 3,438 pass, 1,859 fail, 114 skip, one is an upstream XFAIL,
187 are unsupported, and none time out or crash. The headline pass rate is
64.905% and the whole-corpus rate is 61.404%; 4,624 of 5,297 attempted cases
reach runtime (87.295%). Relative to exact base `a82bfeb5`, the pass-set delta
is +2/-0: all 3,436 prior passes remain passes.

Calling `get_parent_class()` without an argument now emits PHP 8.5's
deprecation before resolving the caller's lexical class or returning false in
global scope. The diagnostic uses the physical callsite and ordinary PHP
error-handler dispatch, so a handler that throws interrupts both the class
lookup and the false global return. A class without a parent still returns
false after ordinary reporting, an inherited method resolves the parent of its
lexical declaring class, and the explicit object-or-class argument path remains
unchanged.

An original E2E regression covers throwing handlers in global and class scope,
restored standard reporting, exact source lines, a parentless class, inherited
lexical scope and an explicit late-static class argument. The adjacent
inheritance regression suppresses `E_DEPRECATED` deliberately while retaining
its trait, runtime alias, object, string and invalid-input coverage. In the
four-case `get_parent_class()` cluster, both targeted no-argument cases pass;
the trait case and `bug21961.phpt` remain at their independent output and
compile boundaries.

`get_parent_class_001.phpt` and `get_parent_class_basic.phpt` make the same two
fail/output-to-pass transitions in the full corpus, with no other status or
category movement. Two final manifests are byte-for-byte identical with
SHA-256
`800dad7f6ac1aa1366f61e2688f27be5a16175162ab257ef0f39229602c19275`.

All five Cargo configurations, all-feature/all-target, formatting, unsafe
self-test and the exact unsafe ratchet pass, as do Composer S0, all four
Symfony S1 gates and exact PHP 8.5.6 warmed-kernel S2 and cold-build S3. The
production inventory remains 1,619 unsafe blocks and 289 unsafe functions. No
performance or layout gate applies: explicit `get_parent_class($value)` is
unchanged and the new work exists only on the already deprecated no-argument
branch.

This checkpoint does not claim the independent trait output and `bug21961.phpt`
compile failures, optional extension suites outside the selected corpus or
broader PHP syntax and runtime compatibility.

The preceding get-class-noarg-deprecation checkpoint was pinned to php-src
8.5.6 commit `fcc29c8`. Across all 5,599 unmodified `Zend/tests` and
`tests/lang` cases, 3,436 pass, 1,861 fail, 114 skip, one is an upstream XFAIL,
187 are unsupported, and none time out or crash. The headline pass rate is
64.867% and the whole-corpus rate is 61.368%; 4,624 of 5,297 attempted cases
reach runtime (87.295%). Relative to exact base `099be260`, the pass-set delta
is +3/-0: all 3,433 prior passes remain passes.

Calling `get_class()` without an argument inside class scope now emits PHP
8.5's deprecation before returning the lexical class name. The diagnostic uses
the physical callsite and ordinary PHP error-handler dispatch; a handler that
throws therefore interrupts the call before a name is returned. Calls outside
class scope retain their direct `Error` without an additional deprecation, and
the ordinary explicit-object form remains unchanged. Static late-called scope,
generators and runtime class aliases continue to return the lexical declaring
class required by PHP.

The original Reflection E2E regression covers handler interruption, restored
standard reporting, static and instance lexical scope, exact source lines and
the outside-scope error. The adjacent alias method-lookup regression now locks
the same PHP 8.5 diagnostics while preserving canonical alias identity. The
28-case focused gate contains the complete `class_alias` directory and the two
other no-argument `get_class()` peers: 27 pass and only the explicit
`memory_limit` capability case is unsupported. Thus every supported upstream
case in the `class_alias` directory is exact.

`class_alias_017.phpt`, `generator_static_method.phpt` and
`get_class_basic.phpt` make the same three fail/output-to-pass transitions in
the full corpus, with no other status or category movement. Two final
manifests are byte-for-byte identical with SHA-256
`f81e9d2cc26d8d9f97dc0ac5b93f3ad93d1c5b964ebd34ff5b083de7e73c5442`.

All five Cargo configurations, all-feature/all-target, formatting, unsafe
self-test and the exact unsafe ratchet pass, as do Composer S0, all four
Symfony S1 gates and exact PHP 8.5.6 warmed-kernel S2 and cold-build S3. The
production inventory remains 1,619 unsafe blocks and 289 unsafe functions. No
performance or layout gate applies: explicit `get_class($object)` is unchanged
and the new work exists only on the already deprecated no-argument branch.

This checkpoint does not claim the separate no-argument
`get_parent_class()` contract, the `memory_limit` CLI capability or broader PHP
syntax and runtime compatibility.

The preceding class-alias-collision-origin checkpoint was pinned to php-src
8.5.6 commit `fcc29c8`. Across all 5,599 unmodified `Zend/tests` and
`tests/lang` cases, 3,433 pass, 1,864 fail, 114 skip, one is an upstream XFAIL,
187 are unsupported, and none time out or crash. The headline pass rate is
64.810% and the whole-corpus rate is 61.315%; 4,624 of 5,297 attempted cases
reach runtime (87.295%). Relative to exact base `6592be1b`, the pass-set delta
is +4/-0: all 3,429 prior passes remain passes.

Name conflicts reached through `class_alias()` now emit PHP 8.5's canonical
`Cannot redeclare <kind> <alias>` warning. The kind comes from the symbol being
aliased and the alias preserves the caller's spelling. For a user-defined
source, the parenthetical origin is that source symbol's declaration—not the
conflicting target declaration. Internal sources have no userland declaration
origin, so the parenthetical is omitted even when their requested alias
collides with a user-defined symbol.

The implementation reads the existing `ClassDef` source file and declaration
line only after the alias registry reports `NameConflict`. Successful alias
publication, duplicate-interface validation and ordinary class lookup are
unchanged. An original E2E test covers class and interface source kinds, user
declaration origins, internal-origin omission, error-handler arguments and
false returns; the existing alias-identity regression now locks the canonical
uncaught warning too.

The complete 27-case `class_alias` directory plus
`gh15976/alias-names.phpt` rises from 21 to 25 passes. `class_alias_002.phpt`,
`class_alias_004.phpt`, `class_alias_010.phpt` and `class_alias_019.phpt` make
the same fail/output-to-pass transitions in the full corpus, with no other
status or category movement. Two final manifests are byte-for-byte identical
with SHA-256
`1f4a846f322d9f9216f38e860e34c214f3ee890c6a67f1ec7b7f256ea0f9c6d2`.

All five Cargo configurations, all-feature/all-target, formatting, unsafe
self-test and the exact unsafe ratchet pass, as do Composer S0, all four
Symfony S1 gates and exact PHP 8.5.6 warmed-kernel S2 and cold-build S3. The
production inventory remains 1,619 unsafe blocks and 289 unsafe functions. No
performance or layout gate applies: the successful path is instructionally
unchanged and the added diagnostic lookup runs only after the cold registry
conflict result.

This checkpoint does not claim the remaining `get_class()` deprecation output
in `class_alias_017.phpt`, the `memory_limit`-dependent unsupported alias case
or broader PHP syntax and runtime compatibility.

The preceding internal-class-alias checkpoint was pinned to php-src 8.5.6
commit `fcc29c8`. Across all 5,599 unmodified `Zend/tests` and `tests/lang`
cases, 3,429 pass, 1,868 fail, 114 skip, one is an upstream XFAIL, 187 are
unsupported, and none time out or crash. The headline pass rate is 64.735%
and the whole-corpus rate is 61.243%; 4,624 of 5,297 attempted cases reach
runtime (87.295%). Relative to exact base `459f8481`, the pass-set delta is
+4/-0: all 3,425 prior passes remain passes.

`class_alias()` now accepts internal classes and interfaces under the same
PHP 8.5 contract as user-defined class-like symbols. Original lookup and
optional autoload still happen first; a missing original reports its warning
and returns false before the alias spelling is considered. A resolved internal
symbol then reaches the shared reserved-name and unqualified-`_` validation,
so reserved aliases remain uncatchable fatals and `_` retains its deprecation
before publication. The existing alias registry publishes the same class
definition and numeric identity rather than copying internal metadata.

Constructing `ReflectionClass` from an alias now projects the resolved
definition's canonical public name. Internal/user classification, class-like
kind, object identity, `instanceof`, interface catches and canonical
`get_class()` behavior therefore remain attached to the original definition.
The constructor-only normalization also applies to user aliases and leaves
unresolved inputs on their existing path.

Original E2E coverage exercises a missing original under an error handler,
internal class construction and identity, internal interface `instanceof`,
canonical Reflection names and internal metadata, reserved aliases and the
qualified/unqualified `_` boundary. The complete 27-case `class_alias`
directory plus `gh15976/alias-names.phpt` rises from 17 to 21 passes:
`class_alias_006.phpt`, both `gh16665` cases and `alias-names.phpt` become
exact. The full-corpus delta contains those same four transitions from
fail/runtime to pass and no other status or category movement. Two final
manifests are byte-for-byte identical with SHA-256
`b8ed05a78c0ec1e69659762c5ce01bb038266c81cf22336e45a070b8cd732945`.

All five Cargo configurations, all-feature/all-target, formatting, unsafe
self-test and the exact unsafe ratchet pass, as do Composer S0, all four
Symfony S1 gates and exact PHP 8.5.6 warmed-kernel S2 and cold-build S3. The
production inventory remains 1,619 unsafe blocks and 289 unsafe functions. No
runtime performance or layout gate applies: both changes are confined to the
explicitly invoked cold `class_alias()` and `ReflectionClass` constructor
paths, reuse the existing alias registry and do not alter executor, value,
object or ordinary class-lookup paths.

This checkpoint does not claim the five remaining output-different
`class_alias` cases, the `memory_limit`-dependent unsupported case or broader
PHP syntax and runtime compatibility.

The preceding eval-compile-fatal checkpoint was pinned to php-src 8.5.6 commit
`fcc29c8`. Across all 5,599 unmodified `Zend/tests` and `tests/lang` cases,
3,425 pass, 1,872 fail, 114 skip, one is an upstream XFAIL, 187 are
unsupported, and none time out or crash. The headline pass rate is 64.659%
and the whole-corpus rate is 61.172%; 4,624 of 5,297 attempted cases reach
runtime (87.295%). Relative to exact base `0de2e3c4`, the pass-set delta is
+2/-0: all 3,423 prior passes remain passes.

Valid-syntax failures discovered while compiling an `eval()` or included
source unit now remain uncatchable PHP compile fatals. They retain the
compiler's canonical message, synthetic eval filename and source line instead
of being converted to a catchable `ParseError` with a duplicated
`Compile error in ...` prefix. Syntax failures remain catchable `ParseError`
objects. A dedicated `Eval` instruction flag also applies PHP's `@` reporting
mask to the synchronous eval unit: successful evaluation and a catchable parse
failure restore the caller mask, while a compile-fatal bailout keeps the
fatal-only mask active through shutdown callbacks.

Three original E2E tests cover successful suppressed eval, warning masking,
parse-error restoration, compile-fatal catch bypass and the matching included-
file boundary. A CLI regression additionally proves exit status 255, exact
fatal presentation, absent catch/continuation output and shutdown observation
of mask 4437. `Zend/tests/bug55007.phpt` and
`Zend/tests/restore_error_reporting.phpt` become exact. The only other status
category movements are expected later boundaries: `gh13931.phpt` and
`gh8841.phpt` move from the incorrect parse wrapper to runtime, while
`constexpr/gh7771_3.phpt` moves from runtime to compile. Two full-corpus runs
produce byte-for-byte identical manifests with SHA-256
`3b168d4e7af663b4bc9c6dbc23408df28e74ce706badaa7c98d85f5c215651c1`.

All five Cargo configurations, all-feature/all-target, formatting, unsafe
self-test and the exact unsafe ratchet pass, as do Composer S0, all four
Symfony S1 gates and exact PHP 8.5.6 warmed-kernel S2 and cold-build S3. The
production inventory remains 1,619 unsafe blocks and 289 unsafe functions. A
CPU-pinned release comparison of 20,000 successful eval compile/execute cycles
used two warm-ups per binary, 32 balanced order pairs and no outlier removal.
Against a fresh, identically built `0de2e3c4` baseline it retained output
`20000` and measured -0.320% independently and -0.726% paired, with paired
p10/p90 -2.860%/+2.551%. The ordinary successful eval path remains below the
five-percent regression ceiling; the newly correct `@eval` mask is not treated
as a like-for-like performance comparison against the prior missing behavior.

This checkpoint does not claim the remaining `break` diagnostic in
`gh13931.phpt`, shutdown-callback Throwable origin in `gh8841.phpt`, constant-
expression diagnostic in `gh7771_3.phpt`, general `@include` suppression or
the unsupported `fatal_error_backtraces` CLI-INI surface.

The preceding reserved-class-names checkpoint was pinned to php-src
8.5.6 commit `fcc29c8`. Across all 5,599 unmodified `Zend/tests` and
`tests/lang` cases, 3,423 pass, 1,874 fail, 114 skip, one is an upstream XFAIL,
187 are unsupported, and none time out or crash. The headline pass rate is
64.621% and the whole-corpus rate is 61.136%; 4,621 of 5,297 attempted cases
reach runtime (87.238%). Relative to exact base `91133161`, the pass-set delta
is +27/-0: all 3,396 prior passes remain passes.

A shared case-insensitive terminal-segment classifier now rejects PHP 8.5's
scalar, pseudo-class and special class names in class-like declarations, class
imports and runtime `class_alias()` strings. Declaration diagnostics preserve
the raw spelling, declaration kind, file and source line; `use` diagnostics
retain the start line of the complete import statement even for multiline
group imports. Runtime aliases normalize case and a leading namespace
separator only for validation and diagnostics, after resolving the original
class and enforcing the internal-class restriction. Function and constant
import namespaces remain independent of the class-name restriction.

Using `_` as a class, trait, interface or enum name, or as an unqualified
runtime class alias, emits PHP 8.5's deprecation introduced in 8.4. Qualified
runtime aliases ending in `\_` and class-import aliases named `_` remain
allowed without that deprecation. Legacy spellings such as `resource`,
`numeric`, `scalar`, `integer`, `double`, `boolean` and `real` remain legal
class names. Two classifier unit tests and six original E2E tests cover
declaration kinds, raw case, namespaces, multiline group imports,
function/constant import boundaries, runtime string-only keywords, `_`,
allowed legacy names and original/internal class lookup priority.

The exact 24-case reserved-name slice rises from 0 to 20 passes. The complete
corpus additionally makes all four `_` declaration cases, all four GH-15976
class-like cases and the lazy-object fatal-shutdown interaction exact. Two
full-corpus runs produce byte-for-byte identical manifests with SHA-256
`ba0b0ee188593b085ab2a6060c69451d443f5f5b0b16b9b901d785791546ad87`.
The only non-pass category movement is explained:
`restore_error_reporting.phpt` now reaches the correct reserved `self`
compile failure, but the independent eval boundary still wraps it as
`Parse error: Compile error` rather than PHP's fatal diagnostic.

All five Cargo configurations, all-feature/all-target, formatting, unsafe
self-test and the exact unsafe ratchet pass, as do Composer S0, all four
Symfony S1 gates and exact PHP 8.5.6 warmed-kernel S2 and cold-build S3. The
production inventory remains 1,619 unsafe blocks and 289 unsafe functions. A
CPU-pinned release comparison of 10,000 independently compiled class
declarations used two warm-ups per binary, 32 balanced order pairs and no
outlier removal. Against a fresh, identically built `91133161` baseline it
retained checksum `10000` and measured +0.187% independently and +0.119%
paired, with paired p10/p90 -1.549%/+1.837%. Both medians remain below the
five-percent regression ceiling; no executor path or runtime layout changed.

This checkpoint does not claim exact eval compile-error wrapping, aliasing of
internal classes, or broader PHP syntax and runtime compatibility.

The preceding magic-property-entry-frame checkpoint was pinned to php-src
8.5.6 commit `fcc29c8`. Across all 5,599 unmodified `Zend/tests` and
`tests/lang` cases, 3,396 pass, 1,901 fail, 114 skip, one is an upstream XFAIL,
187 are unsupported, and none time out or crash. The headline pass rate is
64.112% and the whole-corpus rate is 60.654%; 4,636 of 5,297 attempted cases
reach runtime (87.521%). Relative to exact base `3a2a2ae7`, the pass-set delta
is +1/-0: all 3,395 prior passes remain passes.

Engine-dispatched `__get`, `__set`, `__isset` and `__unset` methods retain their
detached return boundary while publishing the active property instruction as
their logical caller. Live backtraces can therefore cross the magic-property
entry frame and recover its exact source site without making `Return` resume
the suspended caller. The existing sparse detached-trace side state records
whether a logical caller is still at its current instruction, reuses its
allocation after the first dispatch and does not widen `ExecuteData`.
`IssetObj`, `UnsetObj` and silent intermediate property reads now retain their
source line in the compiler's existing sparse table, so all four magic entry
forms expose a line without runtime guessing. Stored Throwable traces keep
their existing post-execution reconnection behavior.

One original E2E regression covers all four property operations plus an
inherited nested getter and the adjacent stored-Throwable regression remains
exact. `Zend/tests/backtrace/bug69180-backtrace.phpt` becomes exact. Two
full-corpus runs produce byte-for-byte identical manifests with SHA-256
`63e298441b6559fdb083a9c10e4c4522cc91e58c743f91d7750709e872fa86c7` and
no other status or failure-stage movement.

All five Cargo configurations, all-feature/all-target, formatting, unsafe
self-test and the exact unsafe ratchet pass, as do Composer S0, all four
Symfony S1 gates and exact PHP 8.5.6 warmed-kernel S2 and cold-build S3. The
production inventory remains 1,619 unsafe blocks and 289 unsafe functions. A
CPU-pinned release comparison of one million repeated missing-property reads
used two warm-ups per binary, 32 balanced order pairs and no outlier removal.
It retained checksum `1000000` and measured -3.865% independently and -3.876%
paired, with paired p10/p90 -10.050%/+5.239%. Both medians remain below the
five-percent regression ceiling; the ordinary declared-property path is
unchanged.

This checkpoint does not generalize detached entry frames beyond magic
property dispatch or claim the remaining generator, eval, include and
Throwable-lifetime backtrace differences.

The preceding trait-method-identity checkpoint was pinned to php-src
8.5.6 commit `fcc29c8`. Across all 5,599 unmodified `Zend/tests` and
`tests/lang` cases, 3,395 pass, 1,902 fail, 114 skip, one is an upstream XFAIL,
187 are unsupported, and none time out or crash. The headline pass rate is
64.093% and the whole-corpus rate is 60.636%; 4,636 of 5,297 attempted cases
reach runtime (87.521%). Relative to exact base `e2d17bad`, the pass-set delta
is +5/-0: all 3,390 prior passes remain passes.

Shared trait bytecode now retains its trait lexical owner for private scope but
derives its public runtime identity from the paired live call initializer and
receiver or static target on diagnostic and trace paths. Original names,
aliases and inherited calls therefore report the concrete class that first
composed the trait, while aliases preserve the selected public method name.
Ordinary trait composition still shares one function/op-array allocation and
ordinary calls publish no new per-frame side state.

Two original E2E regressions cover instance and static original/alias calls,
inheritance, backtraces, argument type errors and return type errors. The three
`Zend/tests/backtrace/bug64239_*` cases plus
`Zend/tests/traits/bug70156.phpt` and
`Zend/tests/traits/trait_type_errors.phpt` become exact. A 236-case trait and
backtrace slice reaches 124 passes without a lost pass. Two full-corpus runs
produce byte-for-byte identical manifests with no other status or failure-stage
movement.

All five Cargo configurations, all-feature/all-target, formatting and the
exact unsafe ratchet pass, as do Composer S0, all four Symfony S1 gates and
exact PHP 8.5.6 warmed-kernel S2 and cold-build S3. The production inventory is
1,619 unsafe blocks and 289 unsafe functions. CPU-pinned 32-pair release A/B
measurements used two warm-ups per binary and removed no outliers. The
32-consumer/eight-method cold-link control measured -0.874% independently and
-0.249% paired, with paired p10/p90 -2.711%/+3.219%. A five-million-call trait
method control, sampled through 20 processes per pair member, measured -0.033%
independently and +0.009% paired, with p10/p90 -0.805%/+0.735%. Exact outputs
are retained and every median remains below the five-percent ceiling.

An op-array-per-composition prototype was rejected before integration: it had
the same exact +5/-0 semantics but regressed the cold-link control by +35.774%
paired. `bug69180-backtrace.phpt` now has the correct trait alias identity but
still lacks the outer magic-property `__get` entry frame; that independent
frame-lifecycle gap remains explicit follow-up work.

The preceding live-function-arguments checkpoint was pinned to php-src
8.5.6 commit `fcc29c8`. Across all 5,599 unmodified `Zend/tests` and
`tests/lang` cases, 3,390 pass, 1,907 fail, 114 skip, one is an upstream XFAIL,
187 are unsupported, and none time out or crash. The headline pass rate is
63.998% and the whole-corpus rate is 60.547%; 4,636 of 5,297 attempted cases
reach runtime (87.521%). Relative to exact base `87006c82`, the pass-set delta
is +2/-0: all 3,388 prior passes remain passes.

Function argument introspection now reads declared parameters from their live
CVs. Assigning through a reference or unsetting a fixed parameter is therefore
visible immediately to `func_get_arg()`, `func_get_args()`,
`debug_backtrace()` and `debug_print_backtrace()`, with an unset value exposed
as PHP `NULL`. True non-variadic extra arguments still use the stable cold
snapshot required after compiler temporaries reuse their original frame slots.

Methods containing `eval` now retain the same local-scope CV metadata already
used by method includes, so the eval scope can read and write method variables.
A backtrace taken before eval returns overlays the active eval-scope values on
the declared arguments of its logical caller. The ordinary function-call path,
call-frame layout and optional executor side-state layout are unchanged.

One original E2E regression covers fixed and by-reference mutation, unset-to-
null conversion, preservation of an extra argument, magic-call argument
packing, active eval traces and post-eval writeback. Both
`Zend/tests/backtrace/bug70547.phpt` and `Zend/tests/bug73156.phpt` become exact.
A 37-case backtrace and argument-introspection slice moves from 22 to 24 passes
without a lost pass. Two full-corpus runs reproduce the same counts,
classifications and pass set, with no other status or failure-stage movement.

All five Cargo configurations, all-feature/all-target, formatting, unsafe
self-test and exact unsafe ratchet pass, as do Composer S0, all four Symfony S1
component gates and warmed-kernel S2. The production inventory remains 1,618
unsafe blocks and 289 unsafe functions. A CPU-pinned release comparison of
2,000 eval compile/execute cycles inside a method used two warm-ups per binary
and 32 balanced order pairs without outlier removal. It retained checksum
`2000` and measured +4.979% independently and +4.468% paired, with paired
p10/p90 +2.396%/+7.076%; both medians remain below the five-percent regression
ceiling.

This checkpoint does not claim exact multiline eval-argument origins, broader
Throwable-trace lifetime behavior, or unrelated trait, generator and include
backtrace cases. Those remain separate compatibility work.

The preceding eval-backtrace checkpoint was pinned to php-src
8.5.6 commit `fcc29c8`. Across all 5,599 unmodified `Zend/tests` and
`tests/lang` cases, 3,388 pass, 1,909 fail, 114 skip, one is an upstream XFAIL,
187 are unsupported, and none time out or crash. The headline pass rate is
63.961% and the whole-corpus rate is 60.511%; 4,636 of 5,297 attempted cases
reach runtime (87.521%). Relative to exact base `293709cc`, the pass-set delta
is +1/-0: all 3,387 prior passes remain passes.

Successfully compiled `eval()` source units now retain a cold logical link to
their caller plus an exact synthetic `eval` frame name and origin. Live
`debug_backtrace()` and `debug_print_backtrace()` output and stored Throwable
traces therefore include inline and recursively nested eval frames without
turning the synchronous eval activation into a normal return-through call
frame. Synthetic frames omit an argument array, and their side metadata is
discarded before the eval activation is released. The hot call-frame layout
and the existing optional side-state pointer layout are unchanged.

One original E2E regression covers live and stored traces across inline and
nested eval. `Zend/tests/backtrace/bug_debug_backtrace.phpt` becomes exact; the
complete 21-case backtrace slice loses no pass. Two full-corpus runs reproduce
the same counts, classifications and pass set, with no other status or failure-
stage movement.

All five Cargo configurations, all-feature/all-target, formatting, unsafe
self-test and exact unsafe ratchet pass, as do Composer S0, all four Symfony S1
component gates and warmed-kernel S2. The production inventory remains 1,618
unsafe blocks and 289 unsafe functions. A CPU-pinned release comparison of
2,000 eval compile/execute cycles used two warm-ups per binary and 32 balanced
order pairs without outlier removal. It retained checksum `2000` and measured
+0.981% independently and +1.722% paired, with paired p10/p90
-0.438%/+3.373%; both medians remain below the five-percent regression ceiling.

This checkpoint does not claim exact source origins when the eval argument
begins on a later physical line: today's literal tokens do not yet retain that
argument line, so `sensitive_parameter_eval_call.phpt` now has the required
synthetic frame but remains one line early. `bug73156.phpt` separately still
needs an eval write to a magic-call argument array to update the trace snapshot.
Eval parse/compile failures and unrelated trait, generator or include backtrace
differences also remain separate compatibility work.

The preceding SensitiveParameter checkpoint was pinned to php-src
8.5.6 commit `fcc29c8`. Across all 5,599 unmodified `Zend/tests` and
`tests/lang` cases, 3,387 pass, 1,910 fail, 114 skip, one is an upstream XFAIL,
187 are unsupported, and none time out or crash. The headline pass rate is
63.942% and the whole-corpus rate is 60.493%; 4,636 of 5,297 attempted cases
reach runtime (87.521%). Relative to exact base `3283b463`, the pass-set delta
is +20/-0: all 3,367 prior passes remain passes.

RPHP now registers PHP 8.5's final internal `SensitiveParameter` attribute and
`SensitiveParameterValue` wrapper with their exercised Reflection metadata.
Live and stored user-function, method, closure, arrow-function and property-
setter-hook traces replace sensitive fixed, defaulted, named and variadic
arguments with opaque wrappers. Named variadic keys remain visible while their
values are redacted. Function bodies continue to receive the original value,
and each trace wrapper owns a dereferenced snapshot rather than an alias.

`SensitiveParameterValue::getValue()` returns the retained value, while
`__debugInfo()` returns an empty array. Ordinary dump, print, array-cast, JSON
and export surfaces keep the wrapper opaque; cloning retains the value, while
dynamic properties, serialization and string conversion remain forbidden. Six
original E2E regressions cover exact metadata, live and Throwable traces,
printed traces, fixed/default/variadic and named arguments, property-setter
hooks, opacity, clone/error surfaces and object retention.

Twenty full-corpus cases become exact passes: the delayed-target-validation
SensitiveParameter case; `gh20435.phpt`; the main, arrow, closure, original-
capture, eval-defined, extra-argument, multiple-argument, named-argument,
nested-call and variadic SensitiveParameter cases; all six exercised
SensitiveParameterValue metadata/clone/error cases; named-parameter backtrace
rendering; and property-hook parameter attributes. Two complete runs reproduce
the same counts, classifications and pass set, with no prior pass loss,
timeout, crash or unexplained stage regression.

All five Cargo configurations, all-feature/all-target, formatting, unsafe
self-test and exact unsafe ratchet pass, as do Composer S0, all four Symfony S1
component gates and warmed-kernel S2. The production inventory remains 1,618
unsafe blocks and 289 unsafe functions. CPU-pinned release comparisons against
the exact parent used two warm-up batches per binary and 32 balanced order
pairs without outlier removal. Batches of 150 empty requests measured -5.449%
independently and -5.534% paired. `bench_calls.php` retained checksum
`37500007500000` and measured -0.022% independently and -0.163% paired, with a
paired p10/p90 range of -1.454%/+0.830%. Exact outputs are retained and every
regression median remains below the five-percent ceiling; the favorable startup
control is not an optimization claim.

This checkpoint does not claim exact behavior for
`sensitive_parameter_eval_call.phpt`, which still reaches RPHP's pre-existing
eval-frame/line-mapping boundary, or
`sensitive_parameter_value_keeps_object_alive.phpt`, where the wrapper retains
the object but an additional Throwable-trace reference delays observable
destruction until request shutdown. General eval traces and Throwable trace
lifetime are separate compatibility work. Ordinary call, `Value` and object
layouts are unchanged.

The preceding weak-scalar-return checkpoint was pinned to php-src
8.5.6 commit `fcc29c8`. Across all 5,599 unmodified `Zend/tests` and
`tests/lang` cases, 3,367 pass, 1,930 fail, 114 skip, one is an upstream XFAIL,
187 are unsupported, and none time out or crash. The headline pass rate is
63.564% and the whole-corpus rate is 60.136%; 4,636 of 5,297 attempted cases
reach runtime (87.521%). Relative to exact base `24a592bc`, the pass-set delta
is +10/-0: all 3,357 prior passes remain passes.

User functions, methods, closures, arrow functions and synthesized property-
hook getters now apply PHP 8.5's weak scalar return contract at the common
return boundary. Exact union members win before coercion; otherwise canonical
`int`, `float`, `string` and `bool` preference is used, while literal `true`
and `false` arms are never treated as general boolean conversion targets.
Numeric strings use the complete PHP grammar, including sign, whitespace and
exponent forms, and integer-to-float widening remains valid in strict mode.
Weak object-to-string returns invoke `__toString()`, whereas strict returns do
not.

Lossy float and float-string conversion to `int` publishes PHP's deprecation,
and NAN-to-string or NAN-to-bool conversion publishes the corresponding
warning. Conversion and its diagnostic happen before an intervening `finally`.
If a deprecation handler throws, the required return `TypeError` wins and
retains the handler throwable as `previous`; an exception from the NAN warning
propagates directly. Ordinary value returns separate the converted result from
the source reference, while a declared by-reference return updates and
forwards the materialized shared cell. Hot and planned scalar return paths now
require the canonical runtime representation and side-exit to this baseline
boundary when conversion is needed.

Eight original E2E regressions cover scalar kinds and runtime representations,
union precedence and literal booleans, strict widening, object string
conversion, diagnostics and exception precedence, `finally`, value/reference
separation and warmed execution. Ten full-corpus cases become exact passes:
`arrow_functions/007.phpt`, `bug70117.phpt`, `bug72347.phpt`,
`property_hooks/get_type_check.phpt`, `property_hooks/recursion.phpt`,
`return_types/return_reference_separation.phpt`,
`literal_types/false_no_coercion_on_overload.phpt`,
`literal_types/true_no_coercion_on_overload.phpt`,
`type_declarations/return_separation.phpt` and
`type_declarations/scalar_return_basic_64bit.phpt`.

Two complete runs reproduce the same counts, classifications and pass set.
Comparison with the base reported several same-status hash changes; a six-case
rerun on both the unchanged base and candidate reproduced that pre-existing
nondeterminism from unordered property/SPL rendering and raw Reflection pointer
text. No stable output regression, pass loss, timeout, crash or other category
movement was found.

All five Cargo configurations, all-feature/all-target, formatting, unsafe
self-test and exact unsafe ratchet pass, as do Composer S0, all four Symfony S1
component gates and warmed-kernel S2. Consolidating return access reduces the
production inventory from 1,620 to 1,618 unsafe blocks while retaining 289
unsafe functions. CPU-pinned release comparisons against the exact parent used
four warmups and 32 balanced order pairs without outlier removal. Batches of
150 empty requests measured -4.232%, `bench_calls.php` measured -0.677% with
checksum `37500007500000`, and the exact typed-return control measured -5.566%
with checksum `900728`. Outputs were exact and every result remains below the
five-percent regression ceiling; negative controls are not optimization
claims.

This checkpoint does not claim broader loose-comparison or parameter/property-
assignment conversion coverage, unrelated trait diagnostics, optional native
extensions or non-CLI SAPIs. `Value` and object layouts are unchanged.

The preceding `cli-standard-streams-array-offsets` checkpoint was pinned to
php-src 8.5.6 commit `fcc29c8`. Across all 5,599 unmodified `Zend/tests` and
`tests/lang` cases, 3,357 pass, 1,940 fail, 114 skip, one is an upstream XFAIL,
187 are unsupported, and none time out or crash. The headline pass rate is
63.375% and the whole-corpus rate is 59.957%; 4,636 of 5,297 attempted cases
reach runtime (87.521%). Relative to exact base `7f3a309c`, the pass-set delta
is +23/-0: all 3,334 prior passes remain exact.

CLI requests now publish request-local `STDIN`, `STDOUT` and `STDERR` stream
resources before user code runs. Their stable first-request identities are 1,
2 and 3; metadata reports the PHP/STDIO backend, `rb`/`wb` direction and
`php://stdin`, `php://stdout` or `php://stderr` URI. Reads and writes use the
process standard channels, wrong-direction operations fail, and closing an
alias retires the resource from the request registry while the constant keeps
its closed identity. The same runtime resources resolve through global and
class constants, instance-property and parameter defaults, static locals and
attribute arguments instead of being rejected during constant-expression
compilation.

The shared ordinary-array offset boundary now matches PHP 8.5 diagnostics and
conversion for resource, null and float keys. Resources warn and use their
integer identity; null publishes the empty-string deprecation; lossy floats
publish the precision deprecation; and non-finite or out-of-range floats use
Zend-compatible modulo conversion after the non-representable warning, with
the additional NAN deprecation. Invalid array, object and Closure keys name
their concrete type and operation context. Compiler-generated compound and
nested writeback normalizes an already-read key without duplicating its
diagnostic, optimized array loops fall back to that baseline boundary, and
`$GLOBALS` keeps its distinct scalar-to-variable-name conversion, including
`Resource id #N`.

Five original subprocess tests cover resource identity and metadata, all three
process channels, direction and close state, runtime constant-expression
positions, resource array keys, `$GLOBALS` naming and destructuring warnings.
Original array E2E coverage adds null, finite-lossy, infinite and NAN key
diagnostics, exact concrete invalid-key errors and single-publication compound
writeback. Twenty-three full-corpus cases become exact passes:
`assign_dim_op_undef.phpt`, `bug72543_2.phpt`, `bug79790.phpt`,
`bug79947.phpt`, `closure_array_key_error.phpt`,
`closure_array_offset_error.phpt`, `const_array_with_resource_key.phpt`,
`falsetoarray_003.phpt`, `gc/bug67314.phpt`,
`illegal_offset_unset_isset_empty.phpt`, `init_array_illegal_offset_type.phpt`,
`isset/isset_array.phpt`, `list/destruct_resource.phpt`, `offset_array.phpt`,
`offsets/gh20194.phpt`, `offsets/null_offset_dep_promoted.phpt`,
`offsets/null_offset_no_uaf.phpt`,
`offsets/null_offset_unset_via_error_handler.phpt`,
`offsets/object_container_offset_behaviour.phpt`, `resource_key.phpt`,
`strict_001.phpt`, `type_coercion/float_to_int/non-rep-float-as-int-extra2.phpt`
and `type_declarations/scalar_strict_64bit.phpt`.

Two remaining failures advance to later, independent output mismatches:
`str_offset_006.phpt` still needs re-entrant string-offset writeback after its
error handler changes the container, and `scalar_return_basic_64bit.phpt` now
passes its former undefined-`STDERR` stop before exposing existing scalar
return-coercion differences. There are no other status/category changes and no
stable output-hash changes among remaining failures.

All five Cargo configurations, all-feature/all-target, formatting, unsafe
self-test and exact unsafe ratchet pass; the production inventory remains
1,620 unsafe blocks and 289 unsafe functions. Composer S0, all four Symfony S1
component gates and warmed-kernel S2 also pass. CPU-pinned release comparisons
against the exact parent used four warmups and 32 balanced order pairs without
outlier removal. Batches of 150 empty `-r` processes measured -0.780%, while
the established array workload measured +0.800%; outputs were exact and both
medians remain below the five-percent regression ceiling.

This checkpoint does not claim user/network stream wrappers, non-CLI SAPIs,
dynamic seekability when a standard descriptor is redirected to a regular
file, or actual closure of the process descriptor behind a retired RPHP
resource. Static-property defaults that depend on these runtime resources also
remain a separate deferred-storage contract. No `Value` or object layout and
no unsafe inventory changes are introduced.

The preceding `deferred-instance-property-defaults` checkpoint reached 3,334
passes with 1,963 failures, 114 skips, one XFAIL, 187 unsupported cases, zero
timeouts and zero crashes. Relative to exact base `c83ec45d`, its pass-set
delta was +11/-0 with all 3,323 prior passes retained.

An instance property default whose otherwise supported constant expression
depends on a global or class symbol unavailable while its source unit is
compiled now remains linked until the class is first instantiated. Runtime
materialization preserves namespace/import and `self`/`parent`/trait-consumer
scope, evaluates inherited defaults before child declarations, applies the
strict typed-property assignment contract and publishes one request-local
immutable template only after the complete class succeeds. A failure remains
catchable and retryable rather than poisoning or partially caching the class.

Ordinary classes retain their established property template and allocation
path behind one absent cold-sidecar branch; `PropertyDefinition`, object layout
and `Value` are unchanged. Deferred errors retain the property-expression file
and line plus PHP's synthetic `[constant expression]` trace frame. Trait
composition compares a direct global constant that an earlier include or
`define()` has published, while shadowed parent defaults disappear before
materialization. Boolean and null indexes in the shared deferred array
evaluator now use PHP's canonical integer/empty-string keys.

Four original E2E tests cover parent-before-child autoload order and repeated
object initialization, repeated typed failures, equal global and consumer-relative
trait defaults, a shadowed invalid parent default and an invalid expression
that must remain a compile error. Eleven full-corpus cases become exact passes:
`bug30702.phpt`, `bug69676_3.phpt`, `bug69832.phpt`,
`update_consts_shadowed_private_prop.phpt`, `oss-fuzz-474613951.phpt`,
`update_constants_virtual_prop.phpt`, `bug74922b.phpt`, `bug74922c.phpt`,
`typed_properties_021.phpt`, `typed_properties_022.phpt` and
`typed_properties_058.phpt`. There are no lost passes, timeout or crash.
`gh10709_2.phpt`, `gh10709_3.phpt`, `gh8176.phpt` and
`lazy_objects/oss_fuzz_71407.phpt` advance from compile rejection to their
independent output mismatches and remain explicit failures.

All five Cargo configurations, all-feature/all-target, formatting and the exact
unsafe ratchet pass; the production inventory remains 1,620 unsafe blocks and
289 unsafe functions. CPU-pinned release comparisons against the exact parent
used four warmups and 32 balanced order pairs without outlier removal. Batches
of 150 empty `-r` processes measured +0.634%, while the established declared-
object lifecycle control measured +0.729%; outputs were exact and both medians
remain below the five-percent regression ceiling. Deferred static-property
storage, closures/first-class callables, enum-property fetch, object concat
conversion and the independent nested-declaration/lazy-object/output gaps are
not claimed by this checkpoint.

The preceding `typed-property-default-diagnostics` checkpoint reached 3,323
passes with 1,974 failures, 114 skips, one XFAIL, 187 unsupported cases, zero
timeouts and zero crashes. Relative to exact base `89df791f`, its pass-set
delta was +7/-0 with all 3,316 prior passes retained.

An invalid literal default for a typed property now fails during compilation
with PHP 8.5's exact declaration diagnostic and property source line. Non-null
defaults name the actual value type, canonical `Class::$property` identity and
canonical declared type. A null default for a non-nullable simple or union type
uses PHP's dedicated `Default value for property of type ... may not be null`
message and suggests `?Type` or `Type|null`; a bare intersection instead uses
the ordinary `Cannot use null as default value` form.

The shared declaration path covers instance, static, trait and anonymous-class
properties, suppressing RPHP's private anonymous sequence in public errors.
The normalizer returns a rejected value only to the cold diagnostic builder;
accepted integer-to-float widening, nullable defaults, canonical union order,
parameter defaults and typed class constants preserve their existing behavior.

One original E2E matrix covers the diagnostic families, exact virtual filename
and line, static and trait declarations, anonymous classes and the accepted
defaults. Existing instance/static tests now assert the complete PHP message.
Seven full-corpus cases become exact passes:
`union_nullable_property_fails.phpt`, `bug81268.phpt`,
`typed_properties_013.phpt`, `typed_properties_014.phpt`,
`typed_properties_015.phpt`, `typed_properties_049.phpt` and
`union_types/illegal_default_value_property.phpt`. There are no lost passes,
other status/category changes or stable non-pass output hash changes.

All five Cargo configurations, all-feature/all-target, formatting and the exact
unsafe ratchet pass; the production inventory remains 1,620 unsafe blocks and
289 unsafe functions. CPU-pinned 32-pair release comparisons against the exact
parent used four warmups and batches of 150 processes. Balanced order-specific
median ratios were +0.055% for empty `-r` startup and +0.398% for compiling 32
valid typed defaults. Outputs were exact, no outliers were removed and both
decision medians remain below the five-percent regression ceiling. Deferred
global/class constants and broader constant-expression property initializers
remain separate contracts; this checkpoint does not claim their first-use
resolution.

The preceding `typed-property-incdec-overflow` checkpoint reached 3,316 passes
with 1,981 failures, 114 skips, one XFAIL, 187 unsupported cases, zero timeouts
and zero crashes. Relative to exact base `a23e9085`, its pass-set delta was
+4/-0 with all 3,312 prior passes retained.

Integer overflow from pre- or post-increment/decrement of an instance or static
typed property now follows the exact property contract. A declaration that
accepts the overflow `float`, such as `int|float`, stores it normally; a
declaration such as `int|bool` instead throws `Cannot increment property ...
past its maximal value` or the corresponding decrement/minimal diagnostic.
The guarded writeback recognizes only a boundary integer produced by the
specific inc/dec direction and uses exact type membership rather than ordinary
weak property-assignment coercion.

When the property cell is a reference, both direct property syntax and an
external alias enforce every retained property constraint and use PHP's
distinct `Cannot increment a reference held by property ...` diagnostic. The
special wording remains after the external alias is unset because the property
still owns the reference cell. Rejected operations preserve the original
boundary value and any outer assignment target; post-increment of an accepting
union still returns the old integer while storing a float. These rules cover
instance and static properties in all four pre/post directions.

An original E2E regression covers accepting and rejecting unions, maximal and
minimal boundaries, direct and aliased access, released aliases, static
storage, exact messages and state preservation. Four full-corpus cases become
exact passes: `typed_properties_019.phpt`, `typed_properties_044.phpt`,
`typed_properties_097.phpt` and `union_types/incdec_prop.phpt`. There are no
lost passes, other status/category changes or stable non-pass output hash
changes.

All five Cargo configurations, all-feature/all-target, formatting and the exact
unsafe ratchet pass; the production inventory remains 1,620 unsafe blocks and
289 unsafe functions. CPU-pinned 32-pair release comparisons against the exact
parent measured balanced order-specific median ratios of +1.185% for ordinary
property work and +0.787% for typed-property work. Outputs were exact, four
warmups preceded each workload, no outliers were removed and both decision
medians remain below the five-percent regression ceiling. Property hooks and
generic property contracts remain separate: hook dispatch is preserved and no
broader generic-property or float-to-int coercion behavior is claimed.

The preceding `incdec-call-write-context` checkpoint reached 3,312 passes with
1,985 failures, 114 skips, one XFAIL, 187 unsupported cases, zero timeouts and
zero crashes. Relative to exact base `764d8d62`, its pass-set delta was +1/-0
with all 3,311 prior passes retained.

Pre- and post-increment or decrement of a call result now produces PHP 8.5's
compile-time write-context diagnostic instead of a parser error or the wrong
call kind. Named, dynamic and immediately invoked callable expressions say
`Can't use function return value in write context`; instance, static and
dynamically named method syntax says `Can't use method return value in write
context`. The parser retains method syntax when dynamic dispatch is lowered
through a callable pair, without changing its runtime lowering. The first
error is deferred to source-unit scope, so dead branches cannot hide it, and
the call is never executed. All four inc/dec forms retain the call source line.

Original parser and E2E tests cover nine named, dynamic, immediate-callable,
instance and static shapes across prefix and postfix increment/decrement inside
dead code. `increment_function_return_error.phpt`, the final ordinary failure
in `Zend/tests/in-de-crement`, becomes an exact pass; the complete directory
has 34 passes and five capability or platform skips. There are no lost passes,
other status/category changes or stable non-pass output hash changes in the
full corpus.

All five Cargo configurations, all-feature/all-target, formatting and the exact
unsafe ratchet pass; the production inventory remains 1,620 unsafe blocks and
289 unsafe functions. No runtime or framework performance gate applies because
accepted programs retain their existing dispatch and bytecode; the new state
is consumed only while constructing compile-time diagnostics for rejected
sources. Built-in-function results in assignment/reference write contexts,
nullsafe write diagnostics and other temporary-expression restrictions remain
separate contracts. A function declared to return by reference is still not a
writable inc/dec result, matching PHP 8.5.

The preceding `incdec-target-semantics` checkpoint reached 3,311 passes with
1,986 failures, 114 skips, one XFAIL, 187 unsupported cases, zero timeouts and
zero crashes. Relative to exact base `c4cd2e15`, its pass-set delta was +9/-0
with all 3,302 prior passes retained.

Pre- and post-increment or decrement of property and dimension targets now
applies the PHP inc/dec value contract to the already-fetched snapshot before
canonical lvalue writeback. It therefore preserves pre/post results, checked
integer overflow, numeric and non-numeric string behavior, null/bool warnings
and invalid-type `TypeError` instead of approximating the operation as binary
addition or subtraction. The operation carries its source line. A checked
value-only `Long` fast path keeps ordinary target increments out of the cold
generic conversion and diagnostic branches.

An inc/dec property fetch now reports an unset declared property like PHP and
re-reads missing storage after a normally returning read-warning handler. A
handler that installs an object is therefore observed by the operation and the
exact class-named `TypeError` preserves that object. Conversely, mutation by
the operator's own warning or deprecation handler is followed by canonical
writeback from the original snapshot. An `ArrayAccess::offsetGet()` value that
is not a reference emits the indirect-modification notice; an invalid fetched
array then throws before `offsetSet()` can run. TMP/VAR inc/dec results use the
frame heap bitmap, preventing stale heap-slot drops across rebound closures.

Four original E2E tests cover scalar property values and source lines,
re-entrant unset/object replacement, all four invalid ArrayAccess forms without
`offsetSet()`, and heap tracking across a scope-rebound property-increment
closure. Nine full-corpus cases become exact passes:
`oss-fuzz-61469_postdec_dynamic_property_unset_error_handler.phpt`,
`oss-fuzz-61469_postinc_dynamic_property_unset_error_handler.phpt`,
`oss-fuzz-61469_predec_dynamic_property_unset_error_handler.phpt`,
`oss-fuzz-61865_postdec_declared_property_unset_error_handler.phpt`,
`oss-fuzz-61865_predec_declared_property_unset_error_handler.phpt`,
`overloaded_access.phpt`, `unset_object_property_in_error_handler.phpt`,
`unset_property_converted_to_obj_in_error_handler.phpt` and
`typed_properties_061.phpt`. There are no lost passes, other status changes or
stable non-pass output hash changes.

All five Cargo configurations, all-feature/all-target, formatting and the exact
unsafe ratchet pass; the production inventory remains 1,620 unsafe blocks and
289 unsafe functions. Composer S0, all four Symfony S1 gates, warmed-kernel S2
and cold-build S3 pass on AMD64. S3 used the available PHP 8.5.9 Phar-capable
oracle while the language corpus remained pinned to PHP 8.5.6. CPU-pinned
32-pair release measurements produced balanced order-specific median ratios of
-1.513% for the combined property/dimension target workload, -4.111% for
result-used direct inc/dec, +3.549% for property work, +1.042% for array work
and -0.468% for batches of 200 empty requests. Outputs are exact, no outliers
were removed and every decision median remains below the five-percent
regression ceiling. The first generic target candidate measured +5.419%; the
accepted checked-`Long` fast path recovered the gate without changing cold PHP
semantics.

At that checkpoint, typed-property overflow diagnostics in `incdec_prop.phpt`
and the distinct function-versus-method return-value write-context diagnostic
remained separate.
Dynamically named and static-property variants share the general target
lowering but are not separately promoted to compatibility claims here.
Extension-defined numeric object operators and broader compound-assignment
behavior also remain outside this checkpoint.

The preceding `incdec-invalid-operands` checkpoint reached 3,302 passes with
1,995 failures, 114 skips, one XFAIL, 187 unsupported cases, zero timeouts and
zero crashes. Relative to exact base `ce434e48`, its pass-set delta was +7/-0
with all 3,295 prior passes retained.

Direct-CV pre- and post-increment or decrement now rejects arrays, resources,
closures, enums and ordinary or internal objects with PHP 8.5's exact
`TypeError`: `Cannot increment TYPE` or `Cannot decrement TYPE`, where `TYPE`
is the diagnostic type or class name. The exception is raised before result or
operand writeback, so the invalid operand remains identical whether or not the
operator result is consumed. This error path does not invoke an error handler
and retains the operator's exact source origin, including uncaught traces.

Two original E2E tests cover all four operator forms across arrays,
`stdClass`, a named user object, a resource, `Closure`, an enum case and
`ReflectionMethod`; they also cover identity preservation, used and unused
results, exact messages, handler exclusion and source lines. Seven full-corpus
cases become exact passes: `bug54305.phpt`, `incdec_types.phpt`,
`object_cannot_incdec.phpt`, `object_cannot_incdec_use_result_op.phpt`,
`oss-fuzz-60734_predec-object.phpt`, `oss-fuzz-60734_preinc-object.phpt` and
`oss_fuzz_63802.phpt`. There are no other status changes and no stable non-pass
output hash changes.

All five Cargo configurations, all-feature/all-target, formatting and the exact
unsafe ratchet pass; the production inventory remains 1,620 unsafe blocks and
289 unsafe functions. Composer S0, all four Symfony S1 gates, warmed-kernel S2
and cold-build S3 pass on AMD64. S3 used the available PHP 8.5.9 Phar-capable
oracle while the language corpus remained pinned to PHP 8.5.6. CPU-pinned
32-pair release measurements produced balanced order-specific median ratios of
-1.305% for four million direct integer increments plus four million direct
integer decrements, -4.950% for property work, -1.639% for array work and
-0.680% for batches of 200 empty requests. Outputs are exact, no outliers were
removed and every decision median remains below the five-percent regression
ceiling.

At that checkpoint, property, dimension and dynamically named targets remained
separate contracts on their arithmetic lvalue paths. Extension-defined numeric
object operators were also outside that checkpoint because their `zend_test`
fixture was skipped; no broader binary-operator behavior was claimed.

The preceding `incdec-null-bool-warnings` checkpoint reached 3,295 passes with
2,002 failures, 114 skips, one XFAIL, 187 unsupported cases, zero timeouts and
zero crashes. Relative to exact base `e6e3fbb6`, its pass-set delta was +6/-0
with all 3,289 prior passes retained.

Direct pre- and post-increment of `bool` and direct pre- and post-decrement of
`bool` or `null` now emit PHP 8.5's exact `E_WARNING` diagnostics before
leaving the operand unchanged. Incrementing `null` remains the distinct
warning-free conversion to integer 1. Both operator forms retain their exact
source line, and decrementing an undefined CV preserves the observable order:
the undefined-variable warning first, then the null-decrement warning.

All four direct inc/dec opcodes consume their pre-handler snapshot. A normally
returning re-entrant handler may unset or replace the CV, but canonical
writeback restores the original unchanged `bool` or `null` through an ordinary
or referenced CV. If the handler throws after mutating the target, the opcode
restores the pre-handler value before propagating the exception. A shared cold
diagnostic kind selects warning or deprecation reporting; ordinary integer and
numeric-string paths do not enter that reporter.

Two original E2E tests cover pre/post values, warning levels and text, source
lines, null increment, undefined-CV ordering, references, handler replacement
and exception restoration. Existing null/bool and undefined-snapshot tests now
assert the PHP 8.5 diagnostics. Six full-corpus cases become exact passes:
`incdec_bool_exception.phpt`, `incdec_undef.phpt`,
`oss-fuzz-60709_globals_unset_after_undef_warning.phpt`,
`unset_globals_in_error_handler.phpt`, `remove_predecessor_of_pi_node.phpt`
and `unreachable_phi_cycle.phpt`. No other status or failure category changes.
`incdec_types.phpt` gains all scalar warnings but retains its independent
invalid-operand TypeErrors.

All five Cargo configurations, all-feature/all-target, formatting and the exact
unsafe ratchet pass; the production inventory remains 1,620 unsafe blocks and
289 unsafe functions. Composer S0, all four Symfony S1 gates, warmed-kernel S2
and cold-build S3 pass on AMD64. S3 used the available PHP 8.5.9 Phar-capable
oracle while the language corpus remained pinned to PHP 8.5.6. CPU-pinned
32-pair release measurements produced balanced order-specific median ratios of
+2.169% for four million direct integer increments plus four million direct
integer decrements, +3.988% for property work, +0.236% for array work and
-0.984% for batches of 200 empty requests. Outputs are exact, no outliers were
removed and every decision median remains below the five-percent regression
ceiling.

At that checkpoint, invalid-operand increment/decrement TypeErrors and property,
dimension or dynamically named targets remained separate contracts on their
existing arithmetic writeback paths.

The preceding `string-decrement-deprecations` checkpoint reached 3,289 passes
with 2,008 failures, 114 skips, one XFAIL, 187 unsupported cases, zero timeouts
and zero crashes. Relative to exact base `e3916d60`, its pass-set delta was
+6/-0 with all 3,283 prior passes retained.

Direct pre- and post-decrement of strings now follows the exercised PHP 8.5
contract. An empty string emits `E_DEPRECATED` with `Decrement on empty string
is deprecated as non-numeric` and becomes integer -1; post-decrement returns
the original empty string. A non-empty non-numeric string emits `Decrement on
non-numeric string has no effect and is deprecated` and remains unchanged.
Integer numeric strings stay integers after decrement, including checked
underflow to double, while floating numeric strings stay floating point. Both
direct decrement opcodes carry their exact PHP source line.

The decrement opcode retains the pre-handler snapshot. A re-entrant error
handler may replace the target, but a normally returning handler is followed by
canonical writeback from the original string; a handler-thrown exception stops
writeback and preserves that original value. The deprecation machinery remains
on the cold string branches, and ordinary integer decrement continues through
the shared result helper without entering it.

Two original E2E tests cover integer and floating numeric strings, empty and
non-numeric pre/post results, exact diagnostics and source lines, handler
replacement, normal handler return and exception interruption. Six full-corpus
cases become exact passes: `decrement_diagnostic_change_type.phpt`,
`incdec_strings.phpt`, `incdec_strings_exception.phpt`,
`oss-fuzz-62294_globals_unset_after_string_warning.phpt`,
`postdec_variationStr.phpt` and `predec_variationStr.phpt`. No other status or
failure category changes. `incdec_types.phpt` and
`unset_globals_in_error_handler.phpt` gain the string-decrement contract but
retain independent PHP 8.5 null/bool diagnostic differences.

All five Cargo configurations, all-feature/all-target, formatting and the exact
unsafe ratchet pass; the production inventory remains 1,620 unsafe blocks and
289 unsafe functions. Composer S0, all four Symfony S1 gates, warmed-kernel S2
and cold-build S3 pass on AMD64. S3 used the available PHP 8.5.9 Phar-capable
oracle while the language corpus remained pinned to PHP 8.5.6. CPU-pinned
32-pair release measurements produced balanced order-specific median ratios of
+4.486% for eight million direct integer post-decrements, -2.150% for property
work, -0.407% for array work and -1.227% for batches of 200 empty requests.
Outputs are exact, no outliers were removed and every decision median remains
below the five-percent regression ceiling.

At that checkpoint, null/bool decrement diagnostics remained separate.
Invalid-type errors are still a separate opcode contract. Complex property,
dimension and dynamically named decrement targets currently use the arithmetic
writeback path rather than this direct-CV decrement path.

The preceding `string-increment-deprecation` checkpoint reached 3,283 passes
with 2,014 failures, 114 skips, one XFAIL, 187 unsupported cases, zero timeouts
and zero crashes. Relative to exact base `6410e526`, its pass-set delta was
+7/-0 with all 3,276 prior passes retained.

Direct pre- and post-increment of a non-numeric string now emits PHP 8.5's
exact `E_DEPRECATED` diagnostic before applying the legacy alphanumeric
increment. Numeric strings still convert to `int` or `float` without a
diagnostic. Carry propagation keeps changes already made to an alphanumeric
suffix when it reaches punctuation, so values such as `.Z` and `a5.9` become
`.A` and `a5.0`, while unsupported trailing bytes and non-ASCII strings remain
unchanged.

The increment opcode retains the pre-handler value snapshot. A re-entrant
error handler may replace the target, but a normally returning handler is
followed by the increment result's canonical writeback; a handler-thrown
exception stops that writeback and leaves the original value intact. Compiler
source metadata now locates both direct increment opcodes at their exact PHP
line without widening the instruction. The diagnostic remains on the cold
non-numeric-string branch, while ordinary integer and numeric-string work does
not enter the error machinery.

Two original E2E tests cover numeric and non-numeric strings, pre/post results,
punctuation carry boundaries, exact source lines, handler replacement and
exception interruption. The existing nested dynamic-name test now asserts the
PHP 8.5 diagnostics that its direct selector increments produce. Seven
full-corpus cases become exact passes: `variable_variables_curly_syntax.phpt`,
both enum `unknown-hash` cases, `increment_diagnostic_change_type.phpt`,
`string_increment_various.phpt`, `postinc_variationStr.phpt` and
`preinc_variationStr.phpt`. No other status or failure category changes.
`bug71300.phpt` and the combined increment/decrement cases gain their correct
increment diagnostics but retain independent later output differences.

All five Cargo configurations, all-feature/all-target, formatting and the exact
unsafe ratchet pass; the production inventory remains 1,620 unsafe blocks and
289 unsafe functions. Composer S0, all four Symfony S1 gates, warmed-kernel S2
and cold-build S3 pass on AMD64. S3 used the available PHP 8.5.9 Phar-capable
oracle while the language corpus remained pinned to PHP 8.5.6. CPU-pinned
32-pair release controls measured balanced order-specific median ratios of
-4.955% for ten million ordinary scalar-loop increments, +1.155% for property
work, +1.048% for array work and -2.542% for 150 empty requests. Outputs are
exact, no outliers were removed and every decision median remains below the
five-percent regression ceiling.

At that checkpoint, decrement diagnostics remained separate. PHP 8.5's bool
warnings and invalid-type increment errors are still separate opcode contracts.
Complex property, dimension and dynamically named increment targets currently
use the arithmetic writeback path rather than this direct-CV increment path.
`bug71300.phpt` also retains its independent dynamic-variable divergence.

The preceding `enum-backing-value-contracts` checkpoint reached 3,276 passes
with 2,021 failures, 114 skips, one XFAIL, 187 unsupported cases, zero timeouts
and zero crashes. Relative to exact base `7fc23567`, its pass-set delta was
+4/-0 with all 3,272 prior passes retained.

Backed enums now retain a compiler-proven duplicate-value or backing-type
diagnostic without publishing it at declaration time. An ordinary read of any
declared enum constant and a correctly typed `from()` or `tryFrom()` call build
the logical backing table and throw the exact `Error` or `TypeError`. A failed
build is not cached, so later reads repeat the same first declaration-order
diagnostic. Invalidly typed lookup arguments still fail before the deferred
table contract and use the internal enum-method wording.

PHP's narrower lazy boundaries remain intact: `cases()`, declaration without
use, arbitrary enum methods, `Enum::class`, undefined constants and enum cases
materialized inside constant expressions do not build the backing table. A
dedicated class-constant instruction flag preserves that last distinction for
parameter defaults while ordinary case and declared-constant reads retain the
validator. Valid enums add no backing check to warmed constant-cache hits, and
the invalid metadata is boxed so every other class carries only one nullable
word.

Two original E2E tests cover int/string duplicates, mismatched values,
declaration and method laziness, `cases()` and constant-expression bypasses,
declared constants, repeatability, argument ordering and internal arity text.
The 152-case `Zend/tests/enum` slice rises from 106 to 110 exact passes. The
four full-corpus gains are `backed-duplicate-int.phpt`,
`backed-duplicate-string.phpt`, `backed-mismatch.phpt` and the adjacent
`backed-from-invalid-type.phpt`; no other status or failure category changes.

All five Cargo configurations, all-feature/all-target, formatting and the exact
unsafe ratchet pass; the production inventory remains 1,620 unsafe blocks and
289 unsafe functions. Composer S0, all four Symfony S1 gates, warmed-kernel S2
and cold-build S3 pass on AMD64. S3 used the available PHP 8.5.9 Phar-capable
oracle while the language corpus remained pinned to PHP 8.5.6. CPU-pinned
32-pair release controls measured balanced order-specific median ratios of
+0.810% for 150 empty requests, +1.087% for ordinary property work and -1.356%
for ordinary array work. Outputs are exact, no outliers were removed and every
decision median remains below the five-percent regression ceiling; the noisier
property paired p10/p90 range is -1.739%/+6.144%.

Broader `from()`/`tryFrom()` coercion edges, deliberate backing-hash collision
cases, Reflection and SplObjectStorage remain separate checkpoints.

The preceding `enum-property-contracts` checkpoint reached 3,272 passes with
2,025 failures, 114 skips, one XFAIL, 187 unsupported cases, zero timeouts and
zero crashes. Relative to exact base `86a51f12`, its pass-set delta was +12/-0
with all 3,260 prior passes retained.

Enum cases now enforce the complete exercised PHP 8.5 property-mutation
contract. Their declared `name` and backed `value` properties reject direct,
compound, increment/decrement, reference, by-reference call/return, foreach
reference and unset operations with the context-specific readonly diagnostic.
Unknown members reject both assignment and reference creation as dynamic
properties, while unsetting an unknown member remains a no-op. Unsetting a
property reached through an enum case constant is rejected during compilation
as a temporary-expression write.

An explicit by-reference `foreach` value may now target an ordinary or dynamic
object property or array dimension. The compiler binds each target to the
current element instead of copying its value, and array-dimension reference
assignment rebinds an existing slot rather than writing through its former
reference. Property arguments passed by reference retain PHP's temporary-cell
and writeback model, so `get_object_vars()` snapshots do not become live
aliases. Readonly scalar reference sources fail before alias publication, while
increment/decrement still uses the direct-modification diagnostic and the
one-write readonly `__clone()` window remains available.

Five original E2E tests cover enum direct/dynamic/reference/unset boundaries,
property and dimension foreach targets, the readonly clone increment window and
the compile-time temporary receiver error. The 152-case `Zend/tests/enum` slice
rises from 98 to 106 exact passes. The other four exact gains are
`Zend/tests/foreach/foreach_by_ref_to_property.phpt` and readonly-property cases
`array_append_initialization.phpt`, `gh7942.phpt` and
`readonly_modification.phpt`. `readonly_props/variation.phpt` advances from an
early runtime failure to a later output mismatch, and `cache_slot.phpt` gains
the required indirect-modification diagnostic before retaining its independent
later runtime failure. No previous pass is lost and no other stable output or
failure category changes.

All five Cargo configurations, all-feature/all-target, formatting and the exact
unsafe ratchet pass; the production inventory remains 1,620 unsafe blocks and
289 unsafe functions. Composer S0, all four Symfony S1 gates, warmed-kernel S2
and cold-build S3 pass on AMD64. S3 used the available PHP 8.5.9 Phar-capable
oracle while the language corpus remained pinned to PHP 8.5.6. CPU-pinned
32-pair release controls measured -0.470% for ordinary property work and
-0.254% for ordinary array work by balanced order-specific median ratios; both
retain exact checksums and stay below the five-percent regression ceiling.

Object-valued readonly reference detachment, the later closure-bound failure in
`readonly_props/cache_slot.phpt`, generic owned-reference JSON encoding, lazy
duplicate or mismatched enum backing values, Reflection and SplObjectStorage
remain separate checkpoints.

The preceding `enum-interface-contracts` checkpoint reached 3,260 passes with
2,037 failures, 114 skips, one XFAIL, 187 unsupported cases, zero timeouts and
zero crashes. Relative to exact base `7aea4a1f`, its pass-set delta was +10/-0
with all 3,250 prior passes retained.

Concrete class and enum declarations now validate PHP 8.5's reserved interface
contract across direct, inherited and separately included interface graphs.
Ordinary and abstract classes cannot implement `UnitEnum` or `BackedEnum`;
unit enums cannot acquire `BackedEnum`; and explicitly repeating an enum's
implicit interface reports the canonical declaration-stage fatal. User
interfaces may still extend either built-in and backed enums may implement an
interface derived from `BackedEnum`, including legal diamonds.

Enums reject `Throwable` and direct or indirect `Serializable` with the exact
class-like kind, source and line. `Serializable` also emits PHP's preceding
deprecation for the concrete enum. Concrete legacy classes receive the same
deprecation unless an effective `__serialize()` and `__unserialize()` pair is
declared, inherited or supplied by a trait; interfaces and abstract classes do
not emit it. The cold linker applies the rule after includes as well as within
one compiled source unit.

Four original E2E tests cover legal enum-interface diamonds, direct and
transitive rejection, the `BackedEnum`/`UnitEnum` diagnostic distinction,
inherited serialization hooks, diagnostic ordering and separately included
interface graphs. The 152-case `Zend/tests/enum` slice rises from 89 to 98
exact passes. Seven selected enum interface cases, adjacent `Throwable` and
duplicate-interface cases, and `serialize/serializable_deprecation.phpt`
become exact; no other full-corpus status or failure category moves.
Four remaining failures advance within the same output category by gaining the
required preceding `Serializable` deprecation; their later independent
serialization or signature mismatches remain visible.

All five Cargo configurations, all-feature/all-target, formatting and the exact
unsafe ratchet pass; the production inventory remains 1,620 unsafe blocks and
289 unsafe functions. Composer S0, all four Symfony S1 gates, warmed-kernel S2
and cold-build S3 pass on AMD64. S3 used the available PHP 8.5.9 Phar-capable
oracle while the language corpus remained pinned to PHP 8.5.6. The work is
confined to compiler validation, declaration diagnostics and cold class linking;
it changes no executor path or runtime/value/object layout, so no runtime
performance lane applies.

Lazy duplicate or mismatched backing-value validation, mutation/reference
diagnostics for built-in enum properties, Reflection and SplObjectStorage remain
separate checkpoints.

The preceding `enum-declaration-shape` checkpoint is documented below. It
reached 3,250 passes with 2,047 failures, 114 skips, one XFAIL, 187 unsupported
cases, zero timeouts and zero crashes. Relative to exact base `8bbd98b9`, its
pass-set delta was +11/-0 with all 3,239 prior passes retained.

Enum declarations now carry their complete backing type and any property syntax
from the parser into compiler validation. Backed enums accept exactly one
`int` or `string` type; class-like, scalar and union alternatives receive PHP's
canonical type spelling and declaration line. Every backed case must provide a
value, every unit-enum case must omit one, and instance, static, typed, untyped
or hooked property declarations all reach PHP's compiler-stage
`Enum ... cannot include properties` diagnostic instead of an earlier parser
rejection or a silently incomplete case.

Four original E2E tests cover valid unit, integer-backed and string-backed
declarations, invalid scalar/class/union backing types, namespaced case-value
diagnostics and all admitted property syntax shapes. The 152-case
`Zend/tests/enum` slice rises from 78 to 89 exact passes. The eleven gains are
the complete selected backing-shape, case-value-presence and property-declaration
cluster; no other full-corpus status or failure stage moves.

All five Cargo configurations, all-feature/all-target, formatting and the exact
unsafe ratchet pass; the production inventory remains 1,620 unsafe blocks and
289 unsafe functions. Composer S0, all four Symfony S1 gates, warmed-kernel S2
and cold-build S3 pass on AMD64. S3 used the available PHP 8.5.9 Phar-capable
oracle while the language corpus remained pinned to PHP 8.5.6. The change ends
after parser AST and compiler diagnostics, with no executor, runtime layout or
generated valid-enum bytecode change, so no runtime performance lane applies.

Lazy duplicate or mismatched backing-value validation, mutation/reference
diagnostics for built-in enum properties, Reflection and SplObjectStorage remain
separate checkpoints.

The preceding `enum-serialization-core` checkpoint is documented below. It
reached 3,239 passes with 2,058 failures, 114 skips, one XFAIL, 187 unsupported
cases, zero timeouts and zero crashes. Relative to exact base `0f84ee36`, its
pass-set delta was +7/-0 with all 3,232 prior passes retained.

Enum cases now use PHP 8.5's dedicated `E:<length>:"Class:Case";` serialization
form. Serialization proves the value against the request's registered enum-case
singletons, preserves repeated object identity through `r:N;`, and never treats
an ordinary object with a matching class or `name` property as a case.
Unserialization autoloads class names, resolves the canonical singleton even
when `allowed_classes` is false, keeps case names case-sensitive and preserves
identity through nested arrays and repeated references.

Malformed enum names, missing classes, non-enum classes, ordinary enum constants
and undefined cases now emit PHP's exercised warning text and byte offset through
the normal error-handler path before returning false. Four original E2E tests
cover unit and backed cases, repeated references, disabled class admission,
autoload, class/case casing, semantic diagnostics and malformed lengths. The
152-case `Zend/tests/enum` slice rises from 71 to 78 exact passes: the two
serialization and five unserialization wire/diagnostic cases become exact, with
no other status or failure-stage movement in the full corpus.

All five Cargo configurations, all-feature/all-target, formatting and the exact
unsafe ratchet pass; the production inventory remains 1,620 unsafe blocks and
289 unsafe functions. Composer S0, all four Symfony S1 gates, warmed-kernel S2
and cold-build S3 pass on AMD64. S3 used the available PHP 8.5.9 Phar-capable
oracle while the language corpus remained pinned to PHP 8.5.6. The change is
confined to explicitly invoked serialization, unserialization and their warning
path; no hot executor, value layout or object layout changed, so no runtime
performance regression lane applies.

Exact enum `debug_zval_dump()` formatting and refcounts, SplObjectStorage's
custom serialized representation, general trailing-data handling, Reflection
and the remaining enum declaration/readonly contracts stay outside this
checkpoint.

The preceding `cycle-collector-core` checkpoint is documented below. It reached
3,232 passes with 2,065 failures, 114 skips, one XFAIL, 187 unsupported cases,
zero timeouts and zero crashes. Relative to exact base `a022a0c5`, its pass-set
delta was +19/-0 with all 3,213 prior passes retained.

`gc_collect_cycles()` now performs explicit request-local collection for
unreachable cycles composed of ordinary objects, arrays, Closures and owned
references. A lazy possible-root registry feeds trial deletion; the cold graph
applies WeakMap ephemeron reachability to a fixpoint, reports the matching
strongly connected component count, runs all garbage destructors before
releasing graph edges, and rebuilds reachability after destructor resurrection.
WeakReference and WeakMap identities are invalidated only when their final
targets are released. Explicit collection remains available while automatic GC
is disabled.

The exact `Zend/tests/gc` and `Zend/tests/weakrefs` gain includes
`bug78999.phpt`, `gc_042.phpt`, `gc_048.phpt`, eleven `gh10043` cases and
`weakmap_weakness.phpt`. Four adjacent lifecycle cases also become exact:
Fiber destructor cases `destructors_002.phpt` and `destructors_003.phpt`,
generator regression `bug76427.phpt`, and magic-method regression
`bug29368_2.phpt`. Four further tests advance to a later known runtime boundary:
three still require suspending an internal Fiber callback, while `gc_049.phpt`
now reaches the missing `gc_status()` API. There are no unexplained category
moves and no lost pass.

Five original E2E tests cover self and two-object cycles, object/array/reference
cycles, destructor resurrection, WeakMap ephemerons and Closure capture graphs.
All five Cargo configurations, all-feature/all-target, formatting and the exact
unsafe ratchet pass; the production inventory is 1,620 unsafe blocks and 289
unsafe functions. Composer S0, all four Symfony S1 gates, warmed-kernel S2 and
cold-build S3 pass on AMD64. S3 used the available PHP 8.5.9 Phar-capable oracle
while the language corpus remained pinned to PHP 8.5.6.

Exact base `a022a0c5` and the final release candidate were compared on CPU 8 in
`performance` mode without removing outliers. Thirty-two balanced ABBA/BAAB
pairs of 150 empty-output requests measured baseline p10/median/p90
0.201480/0.202869/0.205669 seconds versus candidate
0.200215/0.201410/0.203880 seconds: -0.719% by independent medians and -0.704%
by the paired-ratio median. A separate million-object declared-lifecycle lane
retained output `17000000` and measured baseline 0.002652/0.002695/0.002824
seconds versus candidate 0.002634/0.002700/0.002790 seconds: +0.214%
independently and -0.945% paired. The existing ordered workload retained output
`9895778000,1327440292,11223218292,210000` and its balanced order-specific
median ratio was -0.133%. All relevant regression medians remain below the
five-percent ceiling.

There is no runnable cycle-collection baseline in RPHP at `a022a0c5`. A new
100-round lane collecting 1,000 self cycles per round retained the exact prefix
`100000`; PHP 8.5.6 measured median 0.006229 seconds and RPHP 0.086784 seconds,
approximately 13.9x the PHP median. This is an explicit optimization baseline,
not a regression or parity claim. Automatic threshold collection, complete
`gc_status()` telemetry, generator/Fiber/resource cycle breadth and WeakMap
indexing remain separate work. Exact PHP root-color ordering after transient
WeakReference upgrades and compound-Echo temporary lifetime are also outside
this checkpoint's claim.

The preceding `weak-objects-core` checkpoint is documented below.

RPHP now registers final internal `WeakReference`, `WeakMap` and
`InternalIterator` classes. `WeakReference::create()` accepts ordinary objects
and Closures, reuses one live wrapper per target and clears it at the final PHP
owner boundary while keeping the target live during its own destructor.
WeakMap admits object and Closure keys, retains insertion order, removes an
entry immediately after the key destructor, and releases its value afterward.
Reads, writes, null probes, explicit removal, aliases, cloning, iteration and
by-reference iteration follow PHP 8.5. The internal classes reject forbidden
construction, cloning, dynamic properties, append and serialization with the
exercised diagnostics; dumps expose PHP's weak projections and recursion or
reference markers.

The complete 35-case `Zend/tests/weakrefs` cluster rises from zero to 18 exact
passes. Six additional WeakReference/WeakMap users in enum, GC, lazy-object and
top-level regression tests become exact, as do three `ArrayAccess::empty()`
cases reached by the same protocol correction. Sixteen remaining failures
advance from an early missing-class or silent boundary to an executed output
comparison or final-class diagnostic. The 17 focused failures remain explicit:
11 require general cyclic collection, `weakmap_weakness.phpt` differs only in
its cycle-collection section, two `gh17442` cases need broader reference/header
destructor handling, `notify.phpt` needs the adjacent Reflection capability,
`weakmap_dtor_exception.phpt` needs an internal stack frame, and
`weakrefs_004.phpt` needs source-located final-class diagnostics. At that
preceding checkpoint no general cycle-GC claim was made; the explicit collector
above supersedes only the enumerated cycle cases.

Six original E2E tests cover wrapper caching, object and Closure targets,
destructor ordering, aliases and clone separation, null probes, key expiry,
iteration by value and reference, nested destructor notification, and the
construction/property/append/serialization restrictions. Explicit null keys
remain distinct from append and raise `WeakMap key must be an object`.

All five Cargo configurations, all-feature/all-target, formatting and the exact
unsafe ratchet pass; the production inventory remains 1,623 unsafe blocks and
289 unsafe functions. Composer S0, all four Symfony S1 gates, warmed-kernel S2
and cold-build S3 pass on AMD64. S3 used the available PHP 8.5.9 Phar-capable
oracle while the language corpus remained pinned to PHP 8.5.6.

Exact base `ee64027a` and the final release candidate were compared on CPU 8
in `performance` mode without removing outliers. Thirty-two balanced
ABBA/BAAB pairs of 150 empty-output requests measured baseline
p10/median/p90 0.198090/0.199350/0.202431 seconds versus candidate
0.200362/0.201114/0.203792 seconds: +0.885% by independent medians and +0.869%
by the paired-ratio median, whose p10/p90 is +0.434%/+1.453%. A separate
32-pair million-object declared-lifecycle control retained output `17000000`
and measured baseline 0.001081/0.001099/0.001130 seconds versus candidate
0.001092/0.001106/0.001125 seconds: +0.651% independently and +1.005% paired,
with paired p10/p90 -2.329%/+3.110%. Both existing regression medians remain
below the five-percent ceiling.

WeakMap has no runnable RPHP baseline at `ee64027a`. A new 2,000-key
insert/read plus 1,000-removal lane retained output `1999000|1000`; across 32
CPU-pinned pairs, PHP 8.5.6 measured p10/median/p90
0.000213/0.000219/0.000224 seconds and RPHP measured
0.004247/0.004352/0.004483 seconds, approximately 19.9x the independent PHP
median. This is an explicit optimization baseline, not a regression or parity
claim. Replacing the cold insertion-ordered sidecar's linear identity lookup
with a proven indexed representation is handed to the Performance Agent while
the observable weak-lifetime contract and focused tests remain fixed.

The preceding `fiber-bailout-shutdown` checkpoint was pinned to php-src
8.5.6 commit `fcc29c8`. Across all 5,599 unmodified `Zend/tests` and
`tests/lang` cases, 3,186 pass, 2,111 fail, 114 skip, one is an upstream XFAIL,
187 are unsupported, and none time out or crash. The headline pass rate is
60.147% and the whole-corpus rate is 56.903%; 4,610 of 5,297 attempted cases
reach runtime (87.030%). Relative to the preceding 3,170-pass checkpoint, the
exact pass-set delta is +16/-0.

`register_shutdown_function()` now validates and retains resolved callbacks,
supplied arguments, lexical scope and a live instance receiver in a lazily
allocated request-local FIFO. Shutdown callbacks may append more callbacks,
run before ordinary root-scope destruction on successful requests and still
run after `exit` or a displayed fatal diagnostic. An exception raised in the
queue is offered once to the active exception handler; a handled exception
continues the queue, while an unhandled replacement ends the phase.

A Fiber terminated by a VM bailout is now distinct from one that returned or
threw. Shutdown code observes PHP 8.5's exact `FiberError` from `getReturn()`,
including after a fallible `str_repeat()` allocation. Unhandled E_USER_ERROR
also emits its PHP 8.5 deprecation and retains physical file/line metadata for
the subsequent fatal. The three direct/nested/multiple-Fiber fatal cases,
`fiber-in-shutdown-function`, `get-return-after-bailout` and `gh10437` become
exact, taking the complete 110-case Fiber directory from 59 to 65 passes.

Ten adjacent lifecycle/diagnostic cases also become exact:
`anon/gh13097_a.phpt`, `bug20240.phpt`, three `gh10695` shutdown-exception
cases, `gh13446_3.phpt`, `gh13446_4.phpt`, `lsb/lsb_010.phpt`,
`object_gc_in_shutdown.phpt` and `register_shutdown_function_refcount.phpt`.
Four remaining failures advance from an inert output mismatch to their later
runtime boundary without becoming passes: `bug41026.phpt` still needs relative
`self` callable diagnostics, `bug51827.phpt` and `bug71221.phpt` need the exact
inactive-file synthetic trace, and `bug78396.phpt` needs filesystem flags.
General cycle collection, destructor Fibers, generator/internal crossings,
signals and ticks remain separate checkpoints.

All five Cargo configurations, all-feature/all-target, formatting and the exact
unsafe ratchet pass; the production inventory remains 1,623 unsafe blocks and
289 unsafe functions. Composer S0, all four Symfony S1 gates, warmed-kernel S2
and cold-build S3 pass on AMD64. S3 used the available PHP 8.5.9 Phar-capable
oracle while the language corpus remained pinned to PHP 8.5.6.

The exact base `118646fe` and final release candidate were compared on CPU 8
in `performance` mode without removing outliers. Thirty-two balanced
ABBA/BAAB pairs of 150 empty-output requests measured baseline
p10/median/p90 0.208827/0.209715/0.211578 seconds versus candidate
0.207066/0.208320/0.209572 seconds: -0.665% by independent medians and -0.800%
by the paired-ratio median, whose p10/p90 is -1.333%/-0.062%. A separate
32-pair 20,000-cycle Fiber switch control retained output
`199990000|200010000` and measured baseline 0.017052/0.017210/0.017659 seconds
versus candidate 0.017180/0.017350/0.017696 seconds: +0.815% independently and
+0.470% paired, with paired p10/p90 -1.441%/+1.966%. Both medians remain below
the five-percent regression ceiling.

The preceding `fiber-force-close` checkpoint was pinned to php-src
8.5.6 commit `fcc29c8`. Across all 5,599 unmodified `Zend/tests` and
`tests/lang` cases, 3,170 pass, 2,127 fail, 114 skip, one is an upstream XFAIL,
187 are unsupported, and none time out or crash. The headline pass rate is
59.845% and the whole-corpus rate is 56.617%; 4,610 of 5,297 attempted cases
reach runtime (87.030%). Relative to the preceding 3,152-pass checkpoint, the
exact pass-set delta is +18/-0 with no other status or failure-category
movement.

Releasing the last external handle of a suspended Fiber now resumes its pinned
stack with an internal exit value that ordinary `catch` clauses cannot observe.
The exit crosses nested catches, executes every `finally`, rejects another
`Fiber::suspend()` with PHP 8.5's exact `FiberError`, and is suppressed only
after the detached frame chain has been retired. A user exception raised by
cleanup replaces the internal exit; multiple cleanup exceptions retain PHP's
`previous` order and logical shutdown trace.

Detached frame retirement runs PHP object destructors before releasing each
activation. Suspended stacks cache direct handles to their owning Fiber, so a
last external assignment or unset can distinguish Fiber-owned self references
from unrelated aliases and close the cycle without placing PHP calls in
`Value::drop`. Eight `gh9735` stack-lifetime cases, the invocable callback case,
five direct/shutdown force-close diagnostics and four unfinished-Fiber
`finally` cases become exact. The complete 110-case `Zend/tests/fibers`
directory rises from 41 to 59 passes. General Zend cycle collection, destructor
Fibers that themselves suspend, generator/internal callback crossings, ticks,
signals and bailout/OOM cleanup remain separate boundaries.

All five Cargo configurations, all-feature/all-target, formatting and the exact
unsafe ratchet pass; the production inventory remains 1,623 unsafe blocks and
289 unsafe functions. Composer S0, all four Symfony S1 gates, warmed-kernel S2
and cold-build S3 pass on AMD64. S3 used the available PHP 8.5.9 Phar-capable
oracle while the language corpus remained pinned to PHP 8.5.6.

The exact base `272e41cc` and final release candidate were compared on one
pinned Ryzen 9 7950X CPU in `performance` mode without removing outliers.
Thirty-two balanced ABBA/BAAB pairs of the 20,000-cycle Fiber switch workload
retained output `199990000|200010000` and measured baseline p10/median/p90
0.015200/0.015318/0.015545 seconds versus candidate
0.015134/0.015300/0.015747 seconds: -0.114% by independent medians and +0.023%
by the paired-ratio median, whose p10/p90 is -1.916%/+2.475%. Both medians
remain below the five-percent regression ceiling.

The preceding `fiber-error-state` checkpoint was pinned to php-src
8.5.6 commit `fcc29c8`. Across all 5,599 unmodified `Zend/tests` and
`tests/lang` cases, 3,152 pass, 2,145 fail, 114 skip, one is an upstream XFAIL,
187 are unsupported, and none time out or crash. The headline pass rate is
59.505% and the whole-corpus rate is 56.296%; 4,610 of 5,297 attempted cases
reach runtime (87.030%). Relative to the preceding 3,149-pass checkpoint, the
exact pass-set delta is +3/-0. The initial PHP 8.5 corpus continues to have no
process hazard.

Fiber and coroutine stack exchange now includes the request error-reporting
mask and active `@` suppression frames. A newly started detached context
inherits the caller's unsuppressed mask; after suspension, later
`error_reporting()` changes and caller-side `@` frames remain local to their
own execution context. A Fiber's own suppression remains active across its
suspend/resume boundary and unwinds against the original Fiber frame.

The general `@` entry path now intersects the fatal-error mask with the current
reporting mask instead of replacing it with a fixed value. This also fixes
nested suppression outside Fibers. The two dedicated Fiber silence cases and
`Zend/tests/bug34786.phpt` become exact passes. The complete
`Zend/tests/fibers` directory rises from 39 to 41 passes; the full corpus delta
is +3/-0 with no lost pass, other status or failure-stage movement, timeout or
crash. Fiber destruction, GC, force-close and generator crossings remain
separate boundaries.

All five Cargo configurations, all-feature/all-target, formatting and the
exact unsafe ratchet pass; the production inventory remains 1,623 unsafe blocks
and 289 unsafe functions. Composer S0, all four Symfony S1 gates, warmed-kernel
S2 and cold-build S3 pass on AMD64. S3 used the available PHP 8.5.9
Phar-capable oracle while the language corpus remained pinned to PHP 8.5.6.

The exact base `aa7f7700` and final release candidate were compared on one
pinned Ryzen 9 7950X CPU in `performance` mode without removing outliers.
Thirty-two balanced ABBA/BAAB pairs of a 20,000-cycle Fiber switch workload
retained output `199990000|200010000` and measured baseline p10/median/p90
0.014710/0.014928/0.015835 seconds versus candidate
0.014549/0.014788/0.015074 seconds: -0.934% by independent medians and -1.267%
by the paired-ratio median, whose p10/p90 is -4.090%/+0.686%. A separate
32-pair five-million-call `@` control retained checksum `12500002500000` and
measured baseline 0.230367/0.232333/0.236449 seconds versus candidate
0.230317/0.232627/0.236591 seconds: +0.127% independently and +0.030% paired,
with paired p10/p90 -1.568%/+2.102%. Both medians remain below the five-percent
regression ceiling. The added state is allocated only with a detached Fiber or
coroutine context; ordinary executor and value layouts are unchanged.

The preceding Fiber-core checkpoint introduced the final internal `Fiber` and
`FiberError` classes and covers PHP 8.5's core lifecycle: construction, start
arguments, nested current identity, suspension,
resume values, injected exceptions, termination, status queries and return
values. Each Fiber owns a pinned alternate VM stack and suspended frame state;
nested execution swaps the request stack sidecars together and preserves a
logical caller for exact backtraces. Callback arguments use internal-dispatch
weak coercion, and failed initialization terminates the Fiber with PHP's
catchable `TypeError` or `ArgumentCountError` contract.

All 39 exact passes in the complete 110-case `Zend/tests/fibers` directory are
new, and no previous corpus pass is lost. Another 29 Fiber cases advance from
an early runtime failure to a later output comparison. GC cycles, destruction
and forced close during shutdown, generator/internal/magic callback roots,
generator crossings, ticks, signals and exact OOM cleanup remain explicit
boundaries. Generator-root suspension is rejected with a catchable
`FiberError` instead of retaining a popped frame. Fallible, exponentially
copied `str_repeat()` storage also prevents a Rust allocation abort reached by
Fiber bailout tests while preserving PHP 8.5's negative-count `ValueError`.

All five Cargo configurations, all-feature/all-target, formatting and the
exact unsafe ratchet pass; the production inventory is 1,623 unsafe blocks and
289 unsafe functions. Composer S0, all four Symfony S1 gates, warmed-kernel S2
and cold-build S3 pass on AMD64. S3 used the available PHP 8.5.9 Phar-capable
oracle while the language corpus remained pinned to PHP 8.5.6.

The exact base `d30234a5` and final release candidate were compared on one
pinned Ryzen 9 7950X CPU in `performance` mode without removing outliers.
Thirty-two balanced ABBA/BAAB pairs of 150 empty-output requests measured
baseline p10/median/p90 0.360475/0.365265/0.370457 seconds and candidate
0.366586/0.368921/0.377094 seconds: +1.001% by independent medians and +1.222%
by the paired-ratio median, whose p10/p90 is -0.889%/+2.965%. A separate
32-pair `bench_calls.php` control retained checksum `37500007500000` and
measured baseline 0.376982/0.378334/0.383635 seconds versus candidate
0.372740/0.375880/0.381867 seconds: -0.649% independently and -0.673% paired,
with paired p10/p90 -1.676%/+1.228%. A 32-pair 300-million-byte
`str_repeat()` control retained output `300000000` and measured baseline
0.006122/0.006192/0.006297 seconds versus candidate
0.006164/0.006259/0.006348 seconds: +1.090% independently and +1.079% paired,
with paired p10/p90 -0.428%/+1.781%. Every regression median remains below the
five-percent ceiling.

The new 20,000-cycle Fiber switch workload has no RPHP baseline opponent. Nine
paired release observations retained exact output `199990000|200010000` against
PHP 8.5.6 and measured candidate p10/median/p90
0.014328/0.014491/0.015791 seconds versus PHP
0.004783/0.005082/0.005189 seconds. This roughly 2.85x PHP median is recorded as
an optimization baseline for the newly admitted semantic path, not a general
PHP/RPHP performance claim or a regression against the Fiber-less base.

The preceding checkpoint made trait constants compose with PHP 8.5's exact
value, declared-type, visibility and finality compatibility rules. Compatible
definitions retain their attributes and values; Reflection reports a constant
declared directly on a trait as belonging to that trait and its composed
projection as belonging to the consuming class.
`ReflectionClass::getConstant()` and
`ReflectionClassConstant::getDeclaringClass()` expose those values and origins.
Direct trait-constant access remains illegal and now retains the fetch file and
line, while incompatible composition and inherited-final diagnostics name the
actual traits/classes and the class declaration location.

Named classes and enums that consume traits, plus their descendants, are
published by a cold declaration marker at their executable source position.
Earlier output therefore precedes a composition failure, forward-declared
traits work, dead function-local declarations remain unlinked, and dependencies
autoload only when the declaration executes. A missing dependency raises the
kind-specific catchable `Error`; a caught failure can be retried after the trait
is supplied. Ordinary classes remain eagerly linked and their marker is a cold
no-op. Trait declarations that themselves use traits remain on the existing
eager path, and doc comments remain unretained metadata; consequently
`Zend/tests/traits/constant_021.phpt` is not claimed by this checkpoint.

The complete `Zend/tests/traits` directory rises from 92 to 110 exact passes.
Every executable `constant_001.phpt` through `constant_020.phpt` case passes;
`constant_016.phpt` remains explicitly unsupported because it requires an
unimplemented CLI INI capability. The broader exact gains include typed/final
class constants, source diagnostics, enum mutation failures, forward/eval trait
composition and three missing-trait cases. The full 5,599-case delta is +40/-0
with no lost pass, timeout or crash. Five inspected remaining trait/backtrace
failures advance from an early missing-trait runtime error to their later output
comparison, and one advances to the expected declaration-check stage; no other
failure category changes.

All five Cargo configurations, all-feature/all-target, formatting and the exact
unsafe ratchet pass. Composer S0, all four Symfony S1 gates, warmed-kernel S2 and
cold-build S3 also pass on AMD64; S3 used the available PHP 8.5.9 Phar-capable
oracle while the language corpus remained pinned to PHP 8.5.6.

The exact base `568fc79d` and final release candidate were compared on one
pinned AMD64 CPU without removing outliers. Thirty-two balanced ABBA/BAAB pairs
of 150 empty-output requests measured baseline p10/median/p90
0.233997/0.235178/0.239952 seconds and candidate
0.223190/0.224238/0.226935 seconds: -4.652% by independent medians and -4.658%
by the paired-ratio median, whose p10/p90 is -6.338%/-3.989%. A separate
32-pair `bench_calls.php` control retained checksum `37500007500000` and
measured baseline 0.386443/0.389012/0.393184 seconds versus candidate
0.381720/0.383360/0.390291 seconds: -1.453% independently and -1.320% paired,
with paired p10/p90 -2.415%/-0.293%. A 32-pair cold-link control over 100
requests, each composing one trait into 32 classes and retaining output `32`,
measured baseline 0.195937/0.196757/0.197792 seconds versus candidate
0.192354/0.193225/0.194059 seconds: -1.795% independently and -1.791% paired,
with paired p10/p90 -2.400%/-1.376%. These measurements establish absence of a
regression rather than an optimization claim. Every median remains below the
five-percent regression ceiling; the new state is confined to cold compiler,
opcode and request-global declaration side tables and does not alter `Value`,
object, frame or `ClassDef` layout.

The preceding checkpoint implemented PHP 8.5's final internal `Override`
attribute.

PHP 8.5's final internal `Override` class is now registered as an
`#[Attribute(12)]` marker with its zero-argument constructor and Reflection
contract. Compile-time validation accepts it only on methods, property hooks,
properties and promoted properties, rejects repetition, and preserves
`DelayedTargetValidation` target suppression without suppressing the semantic
override check. A promoted marker is reflected on the property rather than the
constructor parameter.

Cold class linking requires each marked member to match an effective
non-private parent declaration, inherited interface contract or abstract trait
requirement. Concrete constructors and concrete members supplied only by a
trait are not override contracts; abstract constructors are. Trait markers are
validated when composed, honor `insteadof`, and propagate to aliases under the
alias name. Property matching remains case-sensitive, and hook matching uses
the exact inherited get/set capability, including implicit accessors of plain
or backed properties.

All 50 `Zend/tests/attributes/override` cases now pass, up from 25. The four
delayed-target-validation error cases and
`property_hooks/override_attribute_fail.phpt` also become exact, while all 59
corpus cases that use `Override` pass. The complete property-hooks directory is
182 of 211. The full 5,599-case delta is +30/-0 with no lost pass, other status
or failure-stage movement, timeout or crash. All five Cargo configurations,
all-feature/all-target, formatting and the exact unsafe ratchet pass. Composer
S0, all four Symfony S1 gates, warmed-kernel S2 and cold-build S3 also pass on
AMD64; S3 used the available PHP 8.5.9 Phar-capable oracle, while the language
corpus remained pinned to PHP 8.5.6.

The exact base `6923306b` and release candidate were compared on one pinned
Ryzen 9 7950X CPU without removing outliers. Sixty-three balanced ABBA/BAAB
pairs of 150 empty-output requests measured baseline p10/median/p90
0.204465/0.205459/0.207942 seconds and candidate
0.203213/0.204080/0.205632 seconds: -0.671% by independent medians and -0.681%
by the paired-ratio median, whose p10/p90 is -1.563%/+0.028%. A separate
31-pair `bench_calls.php` control retained checksum `37500007500000` and
measured baseline 0.375214/0.379965/0.385529 seconds versus candidate
0.377402/0.380948/0.385341 seconds: +0.259% independently and +0.277% paired,
with paired p10/p90 -1.334%/+1.601%. Both medians remain below the five-percent
regression ceiling. Override validation stays on compile/link and Reflection
paths, the added built-in fits the regression-tested registry reservation, and
no runtime value, object, frame or property layout changes.

The preceding checkpoint made reference-returning get hooks recognize a return
of their own property as backing-storage access, including the reference-
binding opcode used by PHP 8.5. Indirect dimension assignment, append,
append-by-reference and coalesce writes dereference temporary and variable
results before mutating the exposed cell. An alias obtained from a property
hook or magic getter carries access-local provenance, so the mutation writes
through that alias without a synthetic setter call or asymmetric-visibility
check, while ordinary reference-valued properties retain their write
visibility. Direct reference assignment to a hooked property invokes its
getter first and then raises PHP's overloaded-object error. That checkpoint
raised the complete property-hooks directory from 179 to 181 and the full
corpus by +3/-0.

Its exact base `664cd430` and release candidate were compared in 32 balanced
pairs on one pinned Ryzen 9 7950X CPU without removing outliers. Batches of 150
empty-output requests measured -1.229% independently and -1.185% paired.
Checksum-preserving controls measured -0.322%/-0.513% for indexed array append,
+0.425%/-0.233% for irregular integer-dimension assignment, -0.040%/-0.048%
for ordinary calls and +0.113%/+0.261% for append-by-reference, with each pair
reported as independent/paired median delta. Every independent and paired
median remained below the five-percent regression ceiling.

The preceding checkpoint scoped a protected instance property through the
oldest non-private declaration in its prototype family, used that prototype's
actual set capability for asymmetric visibility, and made plain child storage
inherit concrete parent hooks that it does not replace. It raised
`Zend/tests/property_hooks` from 171 to 179 exact passes and the complete
corpus by +10/-0. Its CPU-pinned release controls placed empty-request startup
at +0.837%, ordinary calls at -0.645%, instance-property reads at +0.351% and
writes at -0.297% by independent medians.

The preceding checkpoint made virtual get-only property types covariant and
virtual set-only property types contravariant, including children that add the
opposite hook. Backed storage remains invariant because it can be read and
written independently of the declared hook surface. The same compatibility
rule governs delayed linking for unresolved class-like types, so early and late
declarations cannot disagree about the accepted relation. That checkpoint
raised `Zend/tests/property_hooks` from 168 to 171 exact passes and the complete
corpus by +3/-0.

The preceding checkpoint made an explicit set-hook parameter preserve the
property's typed versus untyped declaration state and accept every value
admitted by the property type. Wider class types are accepted
contravariantly; narrower scalar, union or unrelated class types produce PHP's
property-qualified declaration error.
Named declarations wait for later class-like types only when those types can
change the variance result. Runtime includes invoke autoload in property-then-
parameter order for such unresolved relations, while an exact unresolved type
does not trigger unnecessary autoloading.

Set hooks also expose PHP's implicit `void` signature in inheritance
diagnostics. Synthetic accessors for plain properties project that public
contract without losing the value returned by `parent::$property::set()`, so a
plain property continues to satisfy abstract get/set hooks. The complete
`Zend/tests/property_hooks` directory now has 168 of 211 exact passes, up from
161. The six `set_value_parameter_type_variance` failures and the adjacent
write-only property inheritance case move to exact passes; the whole-corpus
delta is +7/-0 with no lost pass, timeout or crash. All five Cargo configurations,
all-feature/all-target, formatting and the exact unsafe ratchet pass. Composer
S0, all four Symfony S1 gates, warmed-kernel S2 and cold-build S3 also pass on
AMD64 against PHP 8.5.

The exact base `73543d13` and release candidate were compared without removing
outliers. A 63-pair ABBA/BAAB confirmation over 150 empty-output file requests
per executable measured baseline p10/median/p90
0.201904/0.205712/0.209709 seconds and candidate
0.203881/0.207462/0.211923 seconds: +0.850% by independent medians and +1.239%
by the paired-ratio median, whose p10/p90 is -1.595%/+3.344%. A separate
31-pair `bench_calls.php` control retained checksum `37500007500000` and
measured baseline 0.355409/0.359152/0.365715 seconds versus candidate
0.355532/0.359264/0.368188 seconds: +0.031% independently and -0.151% paired,
with paired p10/p90 -2.385%/+3.145%. The declaration and cold-link change
remains below the five-percent median regression ceiling; the measured fixed
startup cost is retained for the planned controlled refactoring pass.

The preceding checkpoint rejects hooked properties with PHP's declaration
diagnostics when they are readonly, when a virtual hook declaration specifies
a default, or when a class and trait (or two traits) compose the same hooked
property. A child hook declaration correctly retains backing storage inherited
from a visible, non-virtual parent property; its default presence, Reflection
`isVirtual()` result, implicit plain-property accessor and enumeration behavior
therefore match PHP. That checkpoint raised the property-hooks directory from
151 to 161 exact passes and the complete corpus by +10/-0.

The preceding checkpoint added PHP 8.5's `__PROPERTY__` magic constant in
property defaults and attributes, property-hook bodies and attributes, and
hook-parameter attributes. The same scope is preserved for interfaces and
traits, while ordinary functions and methods, promoted-parameter contexts and
nested functions or closures correctly observe the empty string. Deferred
attribute expressions retain this property scope until Reflection evaluates
their arguments.

All four PHP 8.5.6 corpus cases containing `__PROPERTY__` pass exactly:
`constants/gh17222.phpt` advances from a compile failure and the three
`property_hooks` cases advance from runtime failures. That full-corpus delta is
+4/-0 with no category regression, timeout or crash. All five Cargo
configurations, all-feature/all-target, formatting and the exact unsafe
ratchet pass. Composer S0, all four Symfony S1 gates, warmed-kernel S2 and
cold-build S3 also pass on AMD64.

The exact base `013b66ef` and that release candidate were compared without
removing outliers. A 63-pair ABBA/BAAB confirmation over 150 empty-output file
requests per executable measured baseline p10/median/p90
0.201749/0.205205/0.209230 seconds and candidate
0.201326/0.204412/0.209375 seconds: -0.386% by independent medians and +0.063%
by the paired-ratio median, whose p10/p90 is -2.731%/+2.159%. A separate
31-pair `bench_calls.php` control retained checksum `37500007500000` and
measured baseline 0.363736/0.369090/0.376324 seconds versus candidate
0.360420/0.365244/0.372666 seconds: -1.042% independently and -1.013% paired,
with paired p10/p90 -3.133%/+1.100%. The lexer and cold compiler-context
extension remain below the five-percent median regression ceiling.

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
output; no pass is lost and no timeout or crash appears. Exact internal-enum
object-handle numbering and broader Reflection string formatting remain
explicit follow-up work. All five Cargo configurations,
all-feature/all-target, formatting and the exact unsafe ratchet pass. Composer
S0, all four Symfony S1 gates,
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
