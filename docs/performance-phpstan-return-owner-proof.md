# Detached frame-owner admission

Status: rejected exploratory checkpoint; production source restored exactly.

## Outcome, baseline and evidence

Reduce the general cost of retiring detached frame owners before a function
return or another committed release boundary. Baseline is clean `9419ab7c`,
production source SHA-256
`9635606606194d209de413515955f5159194a1edbf5f456b3b96dff7a080c967`.
Its accepted analysis-only hardware count is 74.2814 billion instructions,
against reference PHP's 10.3374 billion.

The exact analysis profile attributes 3.522 billion inclusive instructions to
8.739 million `retire_owned_frame_value` entries. Array and direct release
preparation invoke final-tree inspection, whose inclusive costs overlap with
retirement and are not added to its total.

An archived diagnostic source copy preserves the complete PHPStan output under
reference PHP and the accepted binary. Whole-command counters record 13,325,685
preparation attempts, 2,380,710 shared arrays, 6,133,078 non-final direct owners,
2,165,386 Strings and 129,604 other values, with 3,729 actual preparations.
Aliased references overlap those value-kind counts. The diagnostic is a
separate optimization-level-one build; its time is not release evidence.

Classification runs before the planner retains its temporary plan reference.
The initial diagnostic classified after preparation and consequently counted
538 arrays and 3,191 direct final owners as shared; the corrected diagnostic
preserves the same 3,729 actual preparations and exact PHP output.

## Hypothesis and semantic envelope

Inline the existing alias and live strong-count rejection at the detached-owner
boundary. Strings and scalar values need ordinary drop only. An array with a
count other than one cannot retire its tree. A direct owner with a count other
than one plus its fiber-owned references cannot retire its PHP release work.
Use exactly the existing count APIs and fiber term; final owners continue into
the complete canonical planner.

Every detached owner is still consumed by ordinary Rust drop, preserving GC
candidate admission, reference-cell ownership and resource release. No new
layout, cache, unsafe invariant, class identity or workload recognizer is added.
Nested destruction, weak/lazy sidecars, generators, fibers, throwing callbacks,
pending exceptions and callback return policy retain the original cold body.
The count proof occurs before any callback; the existing retry loop rechecks
live state after re-entry.

## Ownership and gates

The sole performance task owns `src/vm/execute/call_frames.rs` in the isolated
`codex/perf-phpstan-cold` worktree. No other implementation is active.

First compare frozen, identical default-feature optimization-level-one builds
with FIFO-enabled analysis-only hardware instructions and exact PHP output.
Run formatting, unsafe inventory, focused frame/callable/finally lifetime
coverage and exact compact/wide CLI contracts. Reject before expensive PGO if
instructions fail to improve by one percent or if any owner/callback behavior
changes. Do not widen the proof to cached state.

After selection, run the relevant default/no-default/all-feature lifetime gates
and all-target checks. Build fresh identical-policy PGO from the established
training corpus excluding PHPStan and holdouts. Interleave baseline/candidate
analysis instructions, timing and RSS; retain all valid samples and confirm
independently. Measure shared/final frame owners and independent inherited-call
controls, force the complete canonical path in private coverage diagnostics,
and report code size. Architecture-neutral Rust evidence is available on
x86-64; ARM64 measurements remain unavailable and must stay explicit.

## Rejected result

The exact candidate source is
`78a82b9b0e5226f222aefb1095ebe2e46a0a231c829ec3b8f89e5ba57fcb6de0`.
Its optimization-level-one binary is
`0179699c0ecb620da0bec394a7cb042631c3eae280a4398f8dbfbcbc8e3ffe42`.
Analysis-only Callgrind instructions fall from **118,420,810,256 to
118,109,426,265 (-0.26295%)**, saving 311,383,991 instructions. Both complete
outputs match PHP. This fails the prespecified one-percent exploratory filter.
No PGO build, native release acceptance or canonical coverage claim follows.
Prepared coverage sources and PGO scripts are retained privately but unexecuted.

The initial hardware-counter run fails before running the baseline executable:
the system's `perf_event_paranoid` setting returned to 4 after reboot and
noninteractive sudo requires a password. This is an environment failure, not a
performance sample. Callgrind counts both exact binaries under the same FIFO
analysis boundaries and feature/profile policy; they are not pooled with the
accepted PGO hardware count of 74.2814 billion.

All **122 focused test executions** pass across default/no-default/all-feature
builds, as do formatting, unchanged unsafe inventory and all-target/all-feature
compilation. Eight compact/wide CLI contracts agree with PHP. The first private
counter refresh fails because its prior disposable diagnostic build is no
longer present; the subsequent bounded rebuild succeeds and retains its exact
binary/source. Both failures remain in the packet. Every expensive job uses
the verified 6 GiB/no-swap aggregate boundary and exclusive lock, with no OOM
or timeout.

The source edit is completely removed and restored to the accepted baseline
source identity above. The result shows why call frequency alone is a weak
budget: the existing profile's direct preparer spends 0.865 billion of its
cost in final-tree inspection, and the array preparer spends 1.348 billion in
that inspection. Those traversals remain after the inline count rejection.
Selection returns to those actual graph costs and common opcode bodies.
The original PHPStan instruction/time parity goal remains open.

Exact valid counts, source/binary identities, diagnostic corrections and gates
are in [the checkpoint packet](performance-phpstan-return-owner-proof-samples.json).
