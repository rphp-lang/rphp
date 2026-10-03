# Argument publication coverage on the owned-frame baseline

This is a read-only diagnostic checkpoint. Production remains unchanged and the
PHPStan instruction/time parity goal is open. A new isolated argument-transfer
implementation is not admitted by this census.

The frozen private ownership repair is the baseline: ordinary calls must own
argument/receiver values to preserve COW, references and resources. Its native
PGO performance cost and known production lifetime failures are recorded in the
[ownership review](performance-phpstan-call-frame-ownership.md). The old
argument-transfer selector predates that repair and cannot quantify its current
copying cost. The subsequent [code-generation control](performance-phpstan-release-boundary-control.md)
saves only 0.064612% ordinary instructions, so further boundary variants stopped.

A default plus `vm-stats` release records actual canonical `SendVal` and by-value
`SendVarEx` publications by operand kind, payload type, callee kind, flags and
static temporary-mention shape. A feature-gated `OnceCell` attaches the lazy
syntax sidecar to its owning `OpArray`; a recycled instruction/address cannot
reuse another function's facts. All execution, ownership and callback order
remain unchanged. The diagnostic snapshot also contains incidental compiler
formatting from the pinned rustfmt; it is not a production patch.

The following are **whole-request operation counts**, including initialization
and rendering. They are not analysis-only native instruction budgets. A heap
`Value::clone` generally copies a handle and increments a reference count; it
does not imply copying the full PHP array/string/object payload.

| Observed category | Count |
| --- | ---: |
| Canonical by-value sends at the two instrumented sites | 16,811,533 |
| Heap clone events at those send sites | 13,157,715 |
| All heap `Value::clone` events | 66,976,919 |
| All heap `Value::drop` events | 76,687,443 |
| TMP heap sends | 3,584,316 |
| User-callee TMP heap sends with one syntax consumer | 1,813,548 |
| Same syntax category with no send flags | 703,414 |

The 1.814 million syntax candidates cover **2.708% of all heap clone events**.
Even the two entire heap-send categories cover only **19.645%**. These shares
count events, not weighted instruction cost or removable work. The sidecar
counts an earlier TMP definition plus one explicit consuming mention, excludes
known implicit iterator/diagnostic/trait slots and ignores release interval
endpoints/array-builder writes. It does **not** establish dominance, control-flow
liveness, aliases, destructor timing or safe transfer. No MOVE flag is emitted.
References are classified separately; only four reference-valued sends reached
these by-value sites. By-reference sends, named/callback handlers, fused sends
and frame-free plans are outside this census. Every observed `SendVal` reconciles
with its opcode count; observed by-value `SendVarEx` stays within its total. Each
heap-send type stays within all recorded clone events of that type. All 131
nonzero counter rows are retained in the [data packet](performance-phpstan-owned-argument-census-data.json).

Five pure classifier tests pass. Sixteen ownership/diagnostic controls yield 48
exact PHP/ordinary-repair/diagnostic observations. The actual PHPStan request
also matches reference PHP in exit status, stdout and ordinary stderr: five
files, twenty findings. Only the separately identified statistics block and
existing phase/perf markers are stripped. Instrumented wall time and hardware
counts do not update the native scorecard. No PGO, architecture or production
performance gate is claimed for this diagnostic.

The build and requests run under an exclusive lock in separate verified 6 GiB,
zero-swap, group-OOM services. The build peaks at 4,389,941,248 bytes, and the final
diagnostic service at 633,802,752 bytes; all OOM counters remain zero. Three
harness failures remain visible in the data: missing phase FIFOs, an archival
path failure that left the harness unrepaired for one invocation, and rejection
of an existing compound JIT statistics line. The final reader supports that
format and saves process exit metadata before parsing. Successful controls and
reference/repair requests are reused; only the incomplete diagnostic request
is repeated. Failed diagnostics are not counted as passes.

The next measured boundary must span wider operand/result/assignment publication
and ownership rather than add another narrow send guard. This census neither
proves that all copies are redundant nor promises a date for PHP parity.
