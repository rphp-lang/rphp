# PHPStan Value transport controls

Status: both prototypes rejected and completely removed. Simultaneous PHP
instruction/time parity remains open. Accepted runtime `724abb76` stays at
68.6894G / 6.3610s against PHP's 10.3377G / 0.9294s. The ordinary-release
selectors below are different builds and do not replace that PGO scorecard.

## Why this boundary was tested

The reconciled retained executor profile attributes 0.6719G main self
instructions to inlined Value cloning and 5.1292G to explicit stack-memory
instructions. Those are attribution totals, not an entirely removable budget:
stack traffic also carries live values, callback arguments and frame state.

An exact Rust 1.98.1 / LLVM 22.1.8 probe showed a sixteen-byte mixed numeric/
pointer union returned through a hidden memory buffer, while a transparent
pointer payload with the same metadata returned a scalar pair in registers.
Both keep size sixteen and payload/metadata offsets zero/eight. Production
behavior still required a full application measurement.

## Ordinary full-analysis results

Each candidate starts from clean `33bba254`, uses default features and ordinary
release without PGO, and is compared against the exact same retained executable.
Two interleaved pairs retain every valid sample. Analysis counters use the FIFO
boundary, user instructions, CPU 2 and fresh temporary/cache directories. Counters
run 100% of the measurement interval. Exact output remains five files/twenty
findings and exit one. All samples and executable/input hashes are in
[the data](performance-phpstan-value-transport-data.json).

| Prototype | Baseline median | Candidate median | Instruction change |
| --- | ---: | ---: | ---: |
| Clone normalization | 74.6252G | 74.8957G | +0.3625% |
| Pointer scalar-pair payload | 74.6414G | 75.0101G | +0.4939% |

The declared selector requires at least one percent fewer instructions before
expanding PGO, feature and independent-control gates. Both candidates fail it.
Two-sample timing observations do not establish a speedup. No new runtime
checkpoint is accepted.

## What the prototypes preserve and establish

Clone normalization retains the typed Rc operations, clears only the existing
argument-snapshot flag on ordinary owner copies, and outlines reference-following
copying. It introduces no new unsafe function or Rc layout assumption. Two
reference-PHP contracts cover reference/COW and callback re-entry exactly.

The second prototype starts from the original Clone implementation. On 64-bit
targets only the payload changes from a union to a transparent raw pointer.
Real owner pointers retain their provenance; Long/Double bits use address-only
pointers and are never dereferenced. Narrow-pointer targets retain the original
full-width union. Tags, flags, zero initialization, reference/COW/GC/Drop behavior,
physical offsets and native entry signatures remain unchanged. The API supports
address-only storage without exposing provenance:
[Rust without_provenance_mut](https://doc.rust-lang.org/std/ptr/fn.without_provenance_mut.html).

A final probe links the exact candidate library selected from Cargo's artifact
graph. Actual production Value constructors, move and Clone all return
`{ ptr, i32 }`; the move also receives that pair in registers. This proves the
intended ABI change took effect, rather than merely assuming it from the
synthetic layout. Initial diagnostic artifact/name selection failures are kept
separate from that successful proof. They are not runtime passes.

Three exact PHP CLI contracts pass for this candidate, including integer limits,
negative zero, infinities/NaN bit copying, array COW, object ownership and
references. Native helpers still receive scalar/context/storage pointers rather
than Value by value. The physical payload remains at offset zero for direct
native numeric accesses. A full correctness matrix is intentionally not expanded
after the instruction selector rejects the implementation.

All builds and measurements use verified aggregate 6-GiB/no-swap services with
exclusive benchmark windows; neither prototype causes an OOM. Both full patches
are removed, and the restored source fingerprint matches the accepted baseline.
No ARM64 native performance or comprehensive production soundness claim follows
from these rejected prototypes.

## Consequence for the parity investigation

Wide Value transport exists, but removing it at these general boundaries adds
application instructions. The evidence rejects this isolated representation as
a parity improvement; it does not identify a particular register-pressure or
machine-layout cause of the regression. No compensating patch is stacked onto
it. Continue with quantified operation/storage and lifetime work across the VM.
The previous [execution comparison](performance-phpstan-vm-contrast.md) and
[current body-entry control](performance-phpstan-application-work.md) remain the
basis for that investigation.
