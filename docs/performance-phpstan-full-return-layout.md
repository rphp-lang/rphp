# Full-return execution layout

Status: rejected at exploratory application instruction gate; original parity open.
Baseline: `84df0ff3`, source
`9635606606194d209de413515955f5159194a1edbf5f456b3b96dff7a080c967`,
PGO executable
`58d60aabc60fa4e8c8ec1accf3c1518a44b67484b8f20d84f7ec08ba11015b89`.

## Quantified selection and scope

The preceding exact analysis profile, before the accepted wide retirement
change, assigns 28.8627 billion exclusive instructions to execute_ex_inner.
Conservative unique-body accounting assigns 2.4390 billion to Return, excluding
called helpers. Return-owner retirement separately costs 4.1634 billion
inclusive, with frame pop at 2.2713 billion inclusive. These called budgets
include their descendants and are not added again to per-owner/helper costs.
The current main executable is 390,795 bytes; the unchanged return retirement
and frame-pop functions retain the same sizes as that profiled baseline.

Try a mechanical Rust layout change: keep both existing inline return branches,
move only the existing full-return body to one non-inlined canonical handler,
and return the existing dispatcher outcome. The same existing branch selects
it, with no new admission predicate, allocation, cache or semantic shortcut.
The hypothesis is reduced pressure on the main executor's locals and register
allocation. Recorded stack-address traffic also includes legitimate locals;
it is not claimed as entirely removable spill cost. The Return body is the
affected quantified area, not a promise to remove its whole budget.

The sole integrating task owns baseline_dispatch.rs. The wide retirement
checkpoint is accepted and closed. This is the only active implementation.

## Semantic envelope and gates

Preserve all return-value ownership and type/coercion work, diagnostics,
constructor completion, pending exception/finally state, global/static and
symbol synchronization, generator/fiber behavior, destructor order and exact
caller resume positions. Preserve the existing initial recursive boundary and
canonical exception dispatch. No PHP effect may be replayed or skipped; no
new raw-pointer assumption, frame layout, feature semantics or architecture
behavior is admitted.

First compare exact-source default-feature test-fast baseline/candidate
application phase instructions and full PHP output in an interleaved window,
with existing return/lifetime contracts. Require at least a one-percent exploratory application instruction reduction
before fresh PGO or broad matrices; reject a smaller effect at this filter. This exploratory build is separate
from release acceptance. Preserve every failed attempt and window.

A useful result then needs focused default/no-default/all-feature return/type,
reference/alias/exception/finally/global/generator/fiber gates, exact PHP CLI
checks and canonical outcomes, formatting, unchanged unsafe inventory and
all-target checks. Freeze sources for identical-policy untrained-holdout PGO;
confirm application instructions, controls, code sizes and all timing tradeoffs.
Run all expensive work in the verified 6 GiB/no-swap boundary under the exclusive
lock, with mandatory lifecycle cleanup. Native evidence is x86-64 only; ARM64
measurement is unavailable. No partial checkpoint completes overall parity.

## Rejection

The exact default-feature test-fast analysis window counts **117.1536 billion
baseline and 117.1803 billion candidate instructions (+0.0228%)**. Both RPHP
executables and reference PHP produce identical complete analysis output;
these optimization-level-one counts are not pooled with current PGO's 74.2814
billion instructions. The implementation fails the prespecified one-percent
reduction filter. No fresh PGO or broad feature matrix is started for it.

All 24 actual focused return/lifetime executions and eight existing CLI
contracts pass. A return_hints:: test-name filter selected zero tests; it is
retained explicitly and is not a completed type-hint gate. The initial build
fails because an unlabeled finally continue originally targeted the VM loop;
the corrected prototype carries that exact same-activation outcome explicitly.
Both the failed source/build and corrected source/binary/window remain evidence.
After inverse dispatcher-outcome substitution, canonical full-return operations
are byte-for-byte identical modulo whitespace. Formatting and unsafe checks
pass. Both bounded runs stay within 6 GiB/no swap with no OOM or timeout.

Restore baseline_dispatch.rs completely to accepted 84df0ff3 source. The current
accepted instruction baseline remains 74.2814 billion. This experiment disproves
the proposed layout improvement at the cheap gate; function size alone does not
establish an instruction saving. Exact samples, compiler failure and filtering
limitations are in [the rejected packet](performance-phpstan-full-return-layout-samples.json).
Continue read-only attribution and quantify removable canonical work before
selecting another production implementation.
