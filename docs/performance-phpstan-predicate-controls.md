# PHPStan: predicate and code-generation controls

Status: rejected bounded implementations and an ineligible diagnostic. Production
source is fully restored to accepted runtime `724abb76`; simultaneous parity
remains open at **68.6894G / 6.3610s** versus PHP **10.3377G / 0.9294s**.
The ordinary-release observations below do not replace that PGO scorecard.
[All observations and failures](performance-phpstan-predicate-controls-data.json)
retain exact binary, source, input and output identities.

## Why these controls matter

The [reconciled executor comparison](performance-phpstan-vm-contrast.md) separates
1.513x more bytecode steps from 4.508x more total instructions per step; the two
bytecodes are not equivalent semantic operations. Current accepted instruction
sampling puts 40.42% inside the main executor. Saved explicit native-stack
traffic includes required materialization as well as spills. C versus Rust,
assembly size and a large match alone do not establish the cause of the gap.

Three bounded changes test separate consequences of that diagnosis. Each uses
two alternating analysis-only native pairs, CPU 2, no warmup, fresh TMPDIR,
identical serial five-file analysis, exact PHP output and 100% running
`instructions:u`. Every valid row is retained without outlier removal.

| Ordinary release control | Baseline G | Control G | Instructions | Baseline/control seconds |
| --- | ---: | ---: | ---: | ---: |
| Outline cached property read | 74.645530 | 75.609496 | +1.291% | 7.19246 / 7.18795 |
| Same source, panic=abort | 74.639764 | 73.877395 | -1.021% | 7.20804 / 7.13256 |
| Pure predicate plus adjacent branch | 74.641972 | 74.308916 | -0.446% | 7.16713 / 7.11671 |

These are exploratory ordinary-release observations, not independently confirmed
PGO speedups or corpus acceptance results. PHP reference rows are retained for
each window; all four RPHP observations in each window match the same stdout,
stderr and exit contract.

## Cached property boundary

Changing only `try_cached_fetch_obj_r` from always-inline to never-inline adds
1.291% analysis instructions. Its existing unit-result enum, cache guards,
ownership and ABI remain unchanged. Format, unsafe inventory and release build
pass; a reversible attribute-only selector does not require new mirror tests.
The native selector rejects the implementation before PGO or matrix expansion.
The whole source change is removed. This does not prove every executor split
must fail; it disproves this particular boundary as an application improvement.

## Unwinding control

An exact-source build changes only the Cargo release panic strategy to abort.
It reduces the ordinary main executor from 352,495 to 285,414 bytes and the
executable from 24,562,976 to 20,933,184 bytes, but removes just 1.021% of analysis
instructions. The declared two-percent diagnostic filter therefore does not
admit a broad unwind/error-API migration.

Regardless of speed, this executable cannot enter production: request-memory
exhaustion is caught by the existing executor contract, which abort changes.
No memory-exhaustion correctness pass, PGO result or supported-runtime gain is
claimed. The diagnostic does not remove explicit PHP checks or ownership work;
its result excludes unwinding code generation as the sole explanation for the
several-fold application difference.

## Pure predicates

Compiler finalization extends the existing strict-identity proof to `BoolNot`,
`Isset` and non-relative `Instanceof`. An unmarked adjacent branch must be the
sole consumer of a uniquely defined physical scalar TMP. Hidden foreach,
trait and diagnostic definitions, independently reachable jumps, exception
entries, marked releases, out-of-bounds targets and dynamic finally control
veto the lowering. There is no new runtime eligibility guard or workload key.

Six portable bytecodes omit the scalar TMP store/read and one dispatch. Original
instruction addresses and the adjacent canonical jump remain; metadata retains
the original result for typed planner projection. A const helper selects an
ordinary instanceof result store or direct continuation without changing name,
class-ID, alias or cache semantics. Existing cleanup classification is preserved,
including the previously ineligible ordinary isset case. No native lowering,
Value/frame/cache/instruction layout, allocation or unsafe ceiling changes.

The final default proof and focused packet passes **36 tests**: compiler
projection/idempotence/vetoes, existing identity and instanceof behavior, mixed
values and NaN, references, dynamic targets, aliases, missing classes,
diagnostics, explicit-owner destructor order, generators, finally and scalar
Fiber suspension. Six CLI observations independently match PHP. Initial unsafe
proof and lifetime-fixture failures remain failures in the packet.

The initial combined effects fixture also fails the unchanged accepted binary:
temporary operands survive their predicate until function return, and the last
Fiber-owned destructor is absent. Exact original baseline/candidate CLI streams
are equal to each other and different from PHP. They are not passing coverage.
The supported focused fixture uses explicit CV owners and scalar Fiber values;
no runtime guard encodes the old bugs. The original source/output evidence is
retained for compatibility repair, and no repair is claimed here.

For example, the existing temporary-operand gap can be reproduced with:

```php
class PredicateProbe {
    public function __destruct() { echo "drop\n"; }
}
function probe() {
    if (new PredicateProbe() instanceof PredicateProbe) { echo "body\n"; }
}
probe();
```

PHP prints drop before body; accepted RPHP and the removed candidate print body
before drop. This is evidence of a semantic lifetime difference, not a measured
PHPStan instruction-saving estimate. Baseline lifetime behavior must be repaired
through the general compiler/runtime contract, not a benchmark-specific guard.

The ordinary selector saves only 0.446%, below its predeclared 0.5% filter.
The entire implementation and added test files are removed. No fresh PGO,
feature matrix, independent control cycle or ARM64 performance claim follows.
A handful of additional pure predicate fusions is not a sufficient parity
strategy; the next slice must quantify work shared across operation/ownership
boundaries before implementation.

All expensive jobs use verified separate six-GiB/no-swap services, exclusive
benchmark windows and automatic cleanup. Largest build peak is 3,135,696,896
bytes; no OOM or timeout occurs. Source fingerprints, original failures and exact
executables remain private. Local cleanup runs on both checkouts, and the
superseded abort build target is removed after retaining its exact diagnostic.
No private benchmark host is configured. No runtime improvement is accepted.
