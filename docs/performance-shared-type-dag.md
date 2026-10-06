# Shared immutable PHP declaration types

Accepted declaration/type graph foundation on `codex/perf-phpstan-runtime-costs`, based
on recovered runtime `d7723d96` and the reviewed general core design in
`73d87cbf`. This begins the declaration/type graph implementation; the original
PHPStan instruction/time parity goal and the three 100x targets remain open.
The exact observations and gate results are in
[the data packet](performance-shared-type-dag-data.json).

## Representation and semantic boundary

`Nullable`, `Union` and `Intersection` now retain immutable `Rc` children.
Cloning a declaration retains one root instead of recursively copying its
children, allocations and named-node owners. Children keep their original
order. No global interning cache or PHP object memoization is introduced.
On the measured 64-bit build, one `ParamTypeHint` occupies 24 bytes rather
than 32 bytes. First construction, matching, coercion, variance and ancestry
queries still pay their actual work; root sharing does not make those queries
constant-time.

The compiler, built-in signatures, properties, traits, type checks and
Reflection consume the same shared declaration nodes. Diagnostic formatting
that resolves `self`, `parent` or `static` first detaches the nodes it changes,
so another declaration or scope cannot observe that rewrite. Tests cover
ordered descendant sharing, clone/drop retention, cross-executor resolution,
late aliases and private diagnostic rewriting.

PHP argument/receiver owners, references, COW, warning and exception ordering,
autoload, coercion and fresh Reflection objects remain on their canonical
paths. No new unsafe contract, opcode, native emitter or workload recognizer
is added. Native measurements are x86-64 only; the change is portable Rust
metadata, with no ARM64 speed claim.

## Cost and observations

For N clones of an existing compound declaration, the old copy work grows
with the recursively copied descendant nodes and edges. The new work is N
root retains plus the required first construction, ordered queries and final
node release. Creating shared slices from temporary vectors is included in
the cold cost, not counted as free. This representation does not natively
implement a PHP program's own type algebra or parser.

Four alternating, fresh-process measurements use the same unpacked PHPStan,
five input files and twenty findings, with acknowledged counters around the
same `analyse()` body and GC disabled in both runtimes. Every output matches;
all observations are retained. Startup and teardown are outside this analysis
interval. Whole-process corpus instruction counts separately include source
translation and startup.

| Runtime | Analysis instructions, median | Analysis time, median |
| --- | ---: | ---: |
| Reference PHP | 10.400959 G | 1.034406 s |
| Recovered baseline | 74.688840 G | 7.073207 s |
| Shared type graph | 74.570410 G | 7.102214 s |

The instruction reduction is 0.159%. The time observations do not establish a
wall-time improvement. This is a representation foundation rather than a
solution to the remaining application cost.

Eight alternating corpus/holdout rounds retain all instruction, embedded-time
and RSS observations. Whole-process instruction changes range from -0.4243%
to +0.0213%. An initial +2.846% array timing observation triggered an independent
24-round confirmation; it measures -0.380%. The typed-order control measures
-0.051% in that confirmation. No new timing regression is confirmed.
The eight-round results remain in the packet rather than being replaced by
confirmation results.

The first analysis driver completed all twelve measurements but failed when
aggregating a stale baseline label. The frozen rows were validated and
reaggregated separately; the driver failure is retained explicitly.

## Verification and limits

Four focused tests and 35 PHP fixtures in default and forced canonical modes
pass (70 differential observations). The five feature configurations pass 36,290 test executions; the all-feature
all-target check also passes. Unsafe ceilings
remain 1,747 blocks / 321 functions. Build, matrix and performance work use the
verified aggregate 6 GiB/zero-swap boundary; cleanup runs before and after the
matrix and release cycles and between feature configurations when space
requires it.

The next execution layer must consume shared metadata through the common
proof/ownership IR and make general PHP property, array, branch and call work
cheaper. Current matching and userland parser/intersection algorithms still
execute their required paths. This checkpoint establishes neither full PHP
compatibility nor a 100x result.
