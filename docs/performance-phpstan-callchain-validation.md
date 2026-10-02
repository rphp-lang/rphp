# Current native callchain validation

The current saved PGO profile's `perf` callchain report does not correctly
attribute descendant work. Reconstructing its saved registers and stack with
GDB recovers the missing runtime callers. This changes cost attribution, not
runtime behavior or performance. The accepted scorecard remains **67.5210G /
6.0081 seconds**, versus PHP **10.3380G / 0.9445 seconds**; simultaneous parity
is open. Production source remains `76dd1a94701101cf8b425e4cd325ab6da07cb5bba313c71880562423bb2cbbe1`
on clean documentation baseline `2938b2fd`, with runtime `b2205e89`.

## Contract and proof

Validate causal costs before selecting another shared runtime change. Reuse the
exact current analysis-only instruction profile, with period 1,000,003,
8,192-byte user stacks, 67,497 samples and zero lost samples. No PHPStan run or
runtime correctness matrix is repeated. The sole integrating task owns this
read-only checkpoint. Stop before using overlapping inclusive costs or missing
callers as evidence for an implementation or speedup.

The old self and inclusive reports are nearly identical despite nested calls.
For one saved SHA-256 sample, `perf` produces a corrupt return address. GDB
uses the same register/stack bytes and native CFI to recover `sha256_digest`,
`hash_algorithm_digest`, `fn_hash_file` and the main VM callers. The native
unwind sections contain the required rules. An isolated symbol/cache lookup
and removing the small `.debug_frame` section from a separate copy do not fix
`perf`. This establishes incorrect offline unwinding, without identifying the
precise library defect. The flat sampled instruction IPs remain usable.

A deterministic set of 600 samples first decodes into separate ELF core files.
Streaming reconstruction of the full profile matches every one of those 600
physical-frame vectors exactly. Every raw sample offset, IP and record mode
matches one output row; all 67,497 samples remain in the final partitions.
Physical parents are recovered for 67,477 samples. A nearest main-VM context
is recovered for 65,932 (97.681%).

Eleven GDB inline-frame reconstruction errors remain undecoded (0.0163%).
Eighteen records have a kernel sample IP and saved user-register IP; they remain
in a separate residual and their user stack supplies context only. Unknown
symbols and missing main ancestors also remain explicit. Most stacks stop at
unavailable outer memory in the bounded capture, so this is not a complete
root-to-leaf application call graph.

## Recovered cost boundaries

Each sample belongs once to the closest physical child below its nearest main
VM ancestor, or to main self/residual. Descendant work contributes to that one
boundary. The following are approximate period-scaled costs, not removable
instruction budgets:

| Boundary | Sampled instructions | Share of all samples |
| --- | ---: | ---: |
| Main VM own body | 27.519G | 40.771% |
| `execute_full_call`, including descendants | 5.781G | 8.565% |
| `retire_return_frame_owners`, including descendants | 3.789G | 5.614% |
| `call_resolved_with_source_unpack`, including descendants | 2.271G | 3.365% |
| `op_instanceof`, including descendants | 1.623G | 2.405% |

The full-call boundary contains approximately **3.035G** in three `preg_*`
builtin subtrees. That is actual library work, not pure call protocol overhead.
The boundary's own body is about 0.571G; exact argument matching contributes
another 0.402G. Return-frame retirement contains about 2.350G under owner
retirement and 1.157G in its own body. These subdivisions each sum to their
parent; they must not be added to that parent again.

Optimized nearest-main source locations descriptively group approximately
9.540G under `DoFcall`, 8.383G under `Return`, and 5.312G under `ReleaseTemps`.
This source grouping includes called helpers and actual builtin computation;
it is a separate partition, not additional cost. Optimized line tables and
source motion prevent it from proving exact opcode ownership without a
machine-CFG audit. It does not establish all of those costs as avoidable or
admit a collector-only parity rewrite. General value, call and lifetime work
remains distributed across operations.

## Failures, artifact restoration and lifecycle

An initial quoted GDB core path produced 600 no-frame results and was rejected.
The first streaming decoder stopped on a kernel-IP record. The corrected
streaming decoder emitted all samples and matched all 600 reference vectors,
but its strict zero-error gate still exited 1 for the eleven inline-frame
errors. That gate remains a failure. A separate explicit-residual reconciliation
passes all raw-record, vector and additive accounting checks; no failed sample
is discarded or presented as a reconstructed stack.

A diagnostic `objcopy --dump-section` command without a separate output ELF
unintentionally rewrote the retained input artifact. No application measurement
used that modified artifact. The original source, build path, flags and saved
training profile rebuilt the accepted binary byte for byte: SHA-256
`37138b687fd2430229c60a5c677047a281630ae1277a8d465226f135529a2bbd`,
76,428,240 bytes. It replaced the altered artifact only after exact identity
verification. The altered copy and failure evidence are retained separately.
Future section inspection must be read-only or name a separate output ELF.

Expensive decoding and restoration run inside the verified 6 GiB/no-swap/OOM
process-group boundary with the exclusive benchmark lock. The full decode takes
42.05 seconds and peaks at about 1.06 GB, with zero OOM events. Source-line
lookup and reconciliation reuse the capture; there is no application rerun.
Cleanup completes in both checkouts; the restoration build target is removed,
while exact baselines, profiles and source snapshots remain. No private benchmark
host is configured. This is x86-64 evidence, with no ARM64 speedup claim.

Exact identities, additive tables, failure outcomes and scope limitations are
in [the data](performance-phpstan-callchain-validation-data.json). The next
implementation still requires a general reducible-cost hypothesis and a real
native A/B saving; fixing the profiler alone does not improve the scorecard.
