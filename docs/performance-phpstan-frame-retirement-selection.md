# General frame retirement selection

Status: selected follow-up accepted; overall parity open.
Baseline: clean `4e79d905`, source
`9f4ebb69821b9505671cd633adba6c577427d31fe903966d6bffc293d6988664`,
executable
`70599ddfc0c916698ea94ae9b29e486b8f716db3dbdc41365fb8fac96bac4a5d`.

The fresh exact analysis profile counts 76.9799 billion Callgrind instructions
and matches PHP output. Native phase counters independently count 75.5715
billion; these different counter types are not pooled. Main execution owns
28.8627 billion, of which conservative unique-body accounting assigns 4.4735
billion to ReleaseTemps, 2.4729 to DoFcall and 2.4390 to Return. Shared tails and
called functions are excluded from those body figures. Committed frame-owner
retirement has 4.1634 billion inclusive instructions, including its per-owner
calls; it must not be added to those calls' 3.5224 billion inclusive total.

Selection investigates repeated range scans and PHP release planning for
already shared owners. The current single-owner proof is restricted to the
tracked prefix, even though initialized wide tails can establish the same
proof. A private diagnostic will count actual width, owner multiplicity and
eligible existing single-owner shapes before deciding on implementation.
Simulation counts are opportunities, not production optimized coverage.

The sole integrating task owns call_frames.rs and any affected baseline
retirement boundary. The resolved instanceof checkpoint is accepted and closed.
Preserve final-owner, pending-argument, callback, reference, cycle admission,
weak/lazy/generator/fiber and exception semantics, including canonical fallback.
Reject changes based only on a microbenchmark or an unquantified guessed budget.
Expensive discovery remains inside the verified 6 GiB/no-swap boundary and
exclusive lock. A production contract and useful instruction gate precede
implementation; focused matrices follow a successful exploratory A/B.

## Selection result

The first diagnostic classifies total frame width, an upper bound that also
includes wide frames whose current release interval fits the prefix. The
refined diagnostic instead classifies the release interval's end. Beyond the
prefix there are 5,320,516 single-owner and 124,626 multiple-owner releases over
the whole command. The existing direct-drop shape holds for 5,252,123 of those
single-owner intervals before pending-argument cleanup. All PHP/baseline/private
diagnostic outputs match. These are opportunities; runtime completion still
requires the same post-cleanup strong-count proof.

Select [width-independent sole-owner retirement](performance-phpstan-wide-temp-retirement.md)
for an exploratory application instruction gate before broad validation. The
4.4735-billion ReleaseTemps body is an affected budget, not a promise to remove
it all. The implementation retains multiple/final-owner canonical planning.

The selected implementation passes the exploratory application gate, focused
lifetime/feature gates and independent PGO comparison. Confirmed analysis
instructions fall 1.710% to 74.2814 billion. Actual post-cleanup tail completions
match the 5,252,123 predicted opportunities; the broader initial total-width
counts are not claimed as optimized coverage. The accepted implementation and
visible control tradeoffs are documented in the linked retirement contract.
