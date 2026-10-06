# Canonical source-unpack argument entry

Accepted checkpoint under explicit instruction priority; strict global time gate fails.
The original simultaneous PHPStan instruction/time parity goal remains active.

## General change and semantic contract

Source `f(...$arguments)` previously prepared declared user parameters while
reading the argument array, then checked them again when constructing the
owned call frame. It also eagerly copied the function display name and the
entire parameter-name vector on successful calls. Reading live references
during the first coercion could expose a later argument to reentrant mutation.

Bind and take all arguments first, then validate the owned frame once. Borrow
parameter names, and create the display name only for a diagnostic. A constant
source-entry policy carries source strictness and the actual file/line;
ordinary engine callback entry retains its existing policy. This is general
call preparation with no PHPStan, source-name or argument-shape recognizer.

Reference controls establish the order: named binding holes precede type
validation; supplied fixed values precede missing trailing arity; trailing
arity precedes typed variadics. Named variadic extras do not count as supplied
positional arguments in a user arity error. Relocated closure captures leave
omitted parameter CVs Undef so the ordinary default-binding opcode can act.
Traversable reference warnings go through the existing diagnostic dispatcher,
including handler exceptions and the actual source origin.

Seven original regressions cover value snapshots, reentrant reference aliases,
strict versus engine-callback coercion, callable visibility, diagnostic origin,
warning handlers, arity ordering and omitted captured parameters. Eleven CLI
controls agree exactly with reference PHP; six of them also agree with the
accepted baseline. The five baseline mismatches remain explicit corrections,
not baseline passes. Existing source-unpack scope-introspection limitations
are outside this slice; this is not a claim of complete unpack compatibility.

## Selection evidence and rejected variants

The exact runtime source fingerprint is
`49305541ed9639c786d7d7852095b52da3db8de4e02a6916daf78206ec0d40ac`
on documentation baseline `02633afd`. Ordinary default release uses the same
pinned Rust 1.98.1/LLVM 22.1.8 and function-alignment flags as the accepted
ordinary binary, without PGO. The current three-pair actual analysis selector
reduces 73.7765G to 73.0048G instructions (-1.0460%). All outputs, exits,
five files and twenty findings match reference PHP; every hardware counter
runs at 100%, and all valid observations are retained.

The first prototype used the caller scope for a private callable type and
failed a PHP counterexample. The next checked missing arity before an invalid
supplied value; its PGO and waiting feature jobs were explicitly interrupted
as whole process groups. A third exposed raw closure captures in omitted
optional parameter slots and incorrect exact/at-least wording. These variants
are not accepted, and their instruction results are not pooled into the final
variant. An incomplete frozen snapshot initially omitted unchanged `build.rs`;
that build failed explicitly and was repaired before measurement.

## Remaining gates and lifecycle

All 491 focused executions across default/canonical/quick/all features,
all-target compilation, formatting and unsafe-policy checks pass. Eleven CLI
contracts match PHP in ordinary release and PGO: 66 observations, with five
baseline mismatches retained explicitly. Fresh PGO uses 18 independent public
training programs and 54 exact-output observations, excluding PHPStan and
acceptance controls. Instrumented/profile-use builds take 259.333/196.524
seconds; 27 build-script missing-profile warning lines remain visible, with
no runtime profile hash mismatch warning. Unsafe inventory remains 1,749 blocks / 321 functions;
Value, frame and instruction representations and optimized admission are
unchanged. This checkpoint changes no assembly or JIT backend.

Builds and expensive checks use verified 6 GiB aggregate memory boundaries,
no swap, process-group OOM/termination and the exclusive benchmark lock.
No OOM occurred in completed final checks. Superseded exact source
snapshots and binaries are retained as evidence; cleanup hooks run around
cycles. No private benchmark host is configured. Performance evidence is
x86-64 only, with no ARM64 speedup claim.

## Production measurements and integration tradeoff

| Analysis window | Baseline instructions | Candidate instructions | Baseline/candidate seconds |
| --- | ---: | ---: | ---: |
| PGO first, two interleaved pairs | 67.5218G | 66.8119G | 5.9768 / 5.8720 |
| PGO independent, two pairs | 67.5204G | 66.8068G | 5.9202 / 5.8162 |

Independent confirmation saves **1.057% instructions**
and **1.756% analysis time**. Reference PHP uses
10.3380G / 0.9114 seconds in that window.
The remaining **6.46x instruction gap** keeps simultaneous parity open.
All native counters run at 100%; every output, exit, five files and twenty
findings remain exact. Whole-command confirmation separately measures
85.1322G / 84.3793G instructions, 7.6352 / 7.5098 seconds wall time, and
566700 / 566686 KiB paired median RSS. Those instruction counts include
startup/shutdown and are not the analysis scorecard; RSS is effectively flat.

| Untrained control | Independent instructions | First time | Independent time |
| --- | ---: | ---: | ---: |
| `bench_shared_frame_release.php` | +0.000015% | +0.620% | -1.334% |
| `bench_trait_property_scope.php` | +0.000001% | +2.712% | +1.937% |
| `bench_regex_nested_continuation.php` | -0.000002% | +1.162% | +0.192% |
| `bench_inherited_method_metadata.php` | -0.037579% | -0.425% | +1.174% |
| `bench_shared_temp_read_results.php` | -0.000056% | +2.831% | +3.839% |
| `bench_class_constant_replay.php` | -0.000013% | +0.063% | +0.740% |
| `bench_return_scope_projection.php` | -0.000015% | +4.837% | +4.540% |
| `bench_borrowed_return_validation.php` | +0.019828% | +7.576% | +6.368% |
| `bench_type_return_fanout_string_typed.php` | -0.000003% | -1.168% | +0.405% |
| `bench_hash_dynamic_string_array_loop.php` | +0.000003% | +0.200% | +1.021% |

Two independent five-pair sets retain every valid observation for all ten
controls. Trait, shared-temporary, scope-projection and borrowed-return timing
regressions remain above one percent in both sets; inherited-method and hash
controls also exceed one percent in the independent set. Regex resolves to
+0.192% in the independent set. These are explicit failures of the strict
global time gate. The sole integrator accepts the bounded tradeoff under the
user's explicit instruction priority: target time and instructions improve,
semantic corrections pass, and no independent control instruction median
regresses one percent. Code placement can affect cycles at fixed instruction
counts, but the specific cause of these timing changes is not established.
No compensating runtime patch is added and no general speedup is claimed.

The PGO executable is 76,396,376 bytes, 31,864 bytes smaller than the baseline.
PGO build peak is 4,804,403,200 bytes; completed final services have zero OOM
events. One early confirmation launch failed because the exclusive benchmark
lock was still held; it performed no application measurement and remains
separate from the successful later confirmation. Cleanup completes in both
checkouts; superseded PGO/test targets are deleted while exact snapshots,
binaries, profiles and the active ordinary dependency cache remain.

[All observations and identities](performance-phpstan-source-unpack-entry-data.json)
contain no private machine or path data. Main integration and ARM64 performance
remain separate. Continue with quantified broad operand/call/ownership work;
another one-percent saving alone will not deliver the original parity goal.
