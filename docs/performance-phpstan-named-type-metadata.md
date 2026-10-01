# Named type declaration metadata

Status: accepted bounded instruction checkpoint; full PHPStan parity remains open.
Baseline `929df6ed`, runtime source
`9635606606194d209de413515955f5159194a1edbf5f456b3b96dff7a080c967`.

## Outcome and contract

Classify immutable named type declarations once and share their positive
class-ID resolutions between cloned parameter, return and property hints.
The accepted profile assigns 2.1598 billion inclusive instructions to type
checking, including 0.7459 billion to repeated target-name lookup. The earlier
prototype was rejected under the former time-led criterion; the user now makes
application instructions decisive. This is a fresh implementation and PGO
comparison against the current baseline, rather than adoption of an old result.
The sole integrating task owns compiler/runtime/type/stdlib edits.

The early selection rejects increased instructions or changed output. Final
adoption requires a reproducible phase reduction of at least 0.5%, exact PHP
results, focused feature gates, independent controls and bounded resources.

## Representation and semantics

`TypeName` shares immutable spelling and a preclassified kind through `Rc`.
Its single-entry cache stores only a positive numeric class ID and a never-reused
executor identity. It retains no executor, class or borrowed pointer. Moving,
dropping or alternating executors cannot reuse another request's resolution.
Misses are looked up again, so later declarations and aliases remain visible.
`self`, `parent` and `static` always follow the current activation's lexical or
called scope and bypass this cache. Class-zero objects keep the canonical
name-based query. Original spelling, equality, reflection, errors, unions,
intersections and property constraints remain observable. The checker keeps
canonical ancestry membership; it adds no workload recognizer or unsafe block.
Compiler and stdlib constructors convert strings to the shared representation.
The three shared nullable Throwable constructor hints use thread-local storage
so their Rc metadata remains thread-confined; exact constructor contracts pass. `Value`, `ExecuteData` and the 32-byte hint enum keep their ABI.

## Instruction and time evidence

The final identical-policy PGO comparison uses native user instruction counters,
CPU 2, fresh analysis directories, the same five-file analysis and exact PHP
stdout/stderr/exit signatures. Each window contains two interleaved baseline and
candidate samples; every valid sample is retained. The independent confirmation
reduces **74.2806 to 73.4727 billion instructions (-1.088%)**, versus PHP
**10.3379 billion**. The first window is 74.2870 to 73.4769 billion (-1.091%).
The remaining **7.11x instruction gap** keeps the original goal active.

Confirmation analysis medians are 6.8726 to 6.8589 seconds (-0.200%), versus
PHP's 0.9418 seconds. First-window medians are 6.9629 to 6.7979 seconds.
The separate whole-command window reduces 92.8248 to 91.9744 billion instructions
and measures analysis at 6.9856 to 6.8020 seconds. Whole-command counts include
startup and are never pooled with the FIFO-controlled analysis counts. Median
RSS rises **2,400 KiB**, from 620,078 to 622,478 KiB. These results establish an
instruction saving, without a broad or one-second speed claim.

The initial test-fast, optimization-level-one selection is separate:
117.1460 to 115.3032 billion (-1.573%). It selected the implementation before the
fresh PGO build and cannot establish the release result. The checked-in runner
now provides an output-validated native selection/profile cycle in seconds;
this PGO first window takes 46.33 seconds including its separate sampled profile,
and independent confirmation takes 37.20 seconds without profiling.

## Correctness, controls and resource limits

All **654 focused feature executions** pass: cache identity/alias tests plus
closure scopes, instanceof, parameter/return hints and typed property constraints
in applicable default/no-default/all-feature configurations. All-target and
all-feature checks, formatting and unchanged unsafe inventory pass. Twelve exact
PGO CLI contracts match PHP and baseline RPHP. The existing callback-last-owner
and named-function self compile-time PHP gaps stay baseline-equal failures.
The baseline executable independently exercises the canonical name checker;
misses, repeated successful checks, aliases and rebound relative scopes are
covered on both sides.

Twelve independent controls retain exact PHP checksums. All first-window time
deltas outside one percent, including gains, receive a separate five-pair
confirmation. Instruction regressions are at most 0.060%; stable typed getters
improve 1.122%. Relative-self return time regresses 1.458% with instructions
-0.420%; typed getters regress 2.719% with instructions -0.472%. These are explicit
instruction-priority tradeoffs. Counts, cycles and branch misses are retained;
an exact microarchitectural cause for the timing changes is not isolated.

The checker shrinks 2,721 to 1,417 bytes. The main executor stays 390,795 bytes,
return-owner retirement 3,800 and statement release 17,290 bytes. The eighteen
independent PGO inputs exclude PHPStan and all controls. Instrumented/profile-use
builds take 256.27/196.21 seconds; missing-profile warnings are confined to the
untrained build script, without an executable mismatch.

Every expensive service verifies a 6 GiB aggregate limit, zero swap and an
exclusive lock. The largest successful boundary peaks at 4,769,574,912 bytes,
without OOM or timeout. Initial missing-script and out-of-boundary orchestration
failures remain recorded; neither ran or passed the intended tests. Automatic
cleanup runs in both checkouts, with exact source, binaries and profiles retained
before disposable Cargo targets are removed. Native evidence is x86-64 only;
ARM64 measurements are unavailable. No architecture-specific code is introduced.

The [sample packet](performance-phpstan-named-type-metadata-samples.json) contains
identities, all native samples, builds, feature summaries, controls and memory
boundaries. The integrator accepts this partial checkpoint and continues the
original parity goal with the remaining general runtime costs.
