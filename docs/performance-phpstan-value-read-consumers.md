# Ordinary value reads and their consumers on PHPStan

Status: verified descriptive checkpoint; production runtime unchanged.
The accepted native scorecard remains **67.5210 billion / 6.0081 seconds** versus
PHP **10.3380 billion / 0.9445 seconds**. Simultaneous parity is still active.

The [existing-IR probes](performance-phpstan-existing-ir-feasibility.md) show that
individual admission-filter changes do not cover enough actual execution.
This probe instead records the raw types and following operations of completed
ordinary reads. It excludes the existing cached property/String-length fusion
and records already frame-free getters separately.

| Observed successful operation | Whole-request count | Raw Long results | Relevant boundary |
| --- | ---: | ---: | --- |
| Non-fused cached property read | 23,056,206 | 7,926,380 (34.379%) | 17,595,676 reads have scoped property metadata |
| Present array element read | 12,017,577 | 5,804,117 | 8,502,862 are followed immediately by `ReleaseTemps` |
| Existing frame-free property getter | 767,068 | 74,007 | Already optimized; includes unused return results |

Scoped metadata accounts for **76.316%** of these cached property reads. A new
public-property-only executor would therefore omit most of them. Raw Reference
results remain a separate category; their target types are not inferred.

In **4,715,883** cases, an Array-valued property result supplies the array operand
of the immediately following `FetchDimR`. Of these, 4,366,312 have zero producer
flags and a TMP/VAR result, including 4,334,028 scoped reads. These facts identify
a common value flow, not a complete no-effects/alias/guard proof.

## Cleanup is an execution boundary

The first diagnostic treated matching `ReleaseTemps` interval endpoints as
ordinary operand fields. That relation is invalid for value-consumer inference.
The refined probe assigns cleanup markers no value-use relation and separately
witnesses actual same-frame dispatch after up to four markers, only when the
produced slot is outside every release interval. Any unexpected frame, callback
or control edge discards the witness.

All **8,502,862** present-array reads immediately followed by cleanup have such a
witness in this request. Their next actual operations include:

| Operation after cleanup | Observations |
| --- | ---: |
| `AssignCv` | 3,558,203 |
| `IsIdentical` | 1,207,921 |
| `SendVal` | 876,009 |
| `FetchClassConst` | 737,863 |
| `FetchDimR` | 487,376 |
| `Add` | 464,447 |
| `JmpNZ` | 392,978 |
| `AssignObjProp` | 187,296 |
| `Return` | 177,494 |

This is an observed dispatch relationship. A following opcode need not consume
the result, and matching op1/op2 fields is not a complete operand-role decoder.
Callbacks, conversions, references, COW and lifetime effects still require exact
publication and resume rules before optimization.

## Verification and decision

The [basic control](performance-phpstan-value-read-consumers-control.php) matches
PHP, the unmodified diagnostic baseline and forced canonical execution. The
[callback control](performance-phpstan-value-read-consumers-callback-control.php)
contains two array reads followed by cleanup. Only the plain read reaches the
same-frame Return witness; cleanup of the other source array runs a destructor
callback and correctly cancels it. All outputs match PHP exactly.

The actual five-file/twenty-findings analysis retains exact stdout, ordinary
stderr and expected exit one. Builds and descendants have verified 6 GiB
aggregate limits, no swap and no OOM. Unsafe inventory remains exactly
1,749 blocks, 321 functions, 740 comments and 45 Safety sections. The complete
rows, source/binary fingerprints, boundaries and observer limits are in
[the data](performance-phpstan-value-read-consumers-data.json).

These counts justify implementing a common value/ownership graph around
ordinary scoped property and array reads, their opaque/scalar consumers, and
precise lifetime barriers. It must address bounded region-local storage within
wide frames and preserve canonical cache/scope guards. The first no-JIT proof
must demonstrate actual optimized execution and native instruction savings;
potential value flows are insufficient. No new runtime behavior, region
execution, native speedup or ARM64 performance is admitted by this packet.

To reconstruct, start from the stated runtime, apply the existing
[dispatch-context overlay](performance-phpstan-execution-coverage-probe.patch)
and then [this probe](performance-phpstan-value-read-consumers-probe.patch), each
with `git apply --unidiff-zero`. The source round trip is byte-exact. Build the
pinned release with default features plus `vm-stats` and run inside the recorded
resource boundary. These whole-request instrumented counters are separate from
the accepted analysis-only native performance scorecard.
