# Borrowed exact return validation checkpoint

Status: rejected; production source restored to the exact baseline. The overarching PHP instruction/time parity goal remains open.
Baseline: clean `9489c41f`, source
`f24637cce89489cd2a8c323d6dd2b1db4ce2af85fb7cdf0af5dc43d8cee260d7`,
executable `26410682002e68c683bd673d477d5a79c93920d2ea3b5ef883dff692889b2f6e`.
Confirmed phase instructions are 79.8590 billion versus PHP's 10.3365 billion;
analysis time is 7.2594 seconds versus 0.9113 seconds.

## Evidence and outcome

The preceding exact analysis profile records 3,907,236 full return-preparation
calls, with 1.2381 billion inclusive instructions in that helper. Each call is
preceded by an owned clone of the return operand and construction of an owned
callee class name even when the canonical read-only type check succeeds. The
inspection copy then runs ordinary ownership retirement without having become
a PHP variable. These nested costs are not additive to the complete profile.

Reference executor accounting now includes all indirect dispatch shapes: PHP
has 176,880,452 dispatches and RPHP has 251,165,578 (1.42x). Own executor bodies
use 3.1401 versus 28.5581 billion instructions. This checkpoint addresses one
unnecessary ownership boundary; it cannot by itself explain or close parity.

## Hypothesis and scope

Validate exact return storage by borrowing the live operand for the existing
pure canonical checker. Resolve declaration fallback through a borrowed name
from the existing function metadata. Only a failed exact check acquires the
stable owned snapshot required for coercion, diagnostics or PHP re-entry.
Keep the checker, precedence, relative scope and failure behavior identical.
Use the same boundary for ordinary return strategies and post-finally reference
validation. Do not change argument validation, call admission, return transfer,
release graphs, Value/frame/opcode/JIT layout or caches in this checkpoint.

The sole integrating task owns `execute.rs`, `baseline_dispatch.rs`, focused
return tests, this contract and evidence in the isolated performance checkout.
There is no other editing agent and no concurrent implementation checkpoint.

## Semantic envelope and proof

The exact checker must not run PHP, write/retire the operand or suspend its
frame. Class/alias/ancestry and callable shape inspection retain their canonical
rules. End the borrow before any fallible conversion, warning handler, exception
unwind, `__toString`, destructor or finally callback. On failure keep the same
owned snapshot before conversion so mutation through aliases cannot change the
validated source or diagnostic. Preserve weak scalar conversion, exact union
preference, self/parent/static and trait/closure scopes, reference-return cells,
finally revalidation, generator return separation and exception chaining.

Run focused default/no-default/all-feature return, scope, reference, GC and
callback gates, baseline/PHP differentials and formatting/unsafe/all-target
checks within the required aggregate memory boundary. Keep known baseline PHP
gaps distinct from passing cases. Build fresh independent PGO from the unchanged
18 inputs, excluding PHPStan and controls. Compare phase counters, application
output/time/RSS and established ownership/call/property/regex controls plus an
independent typed-object/array return holdout. Retain every valid measurement.

Reject if any successful-check borrow can survive PHP re-entry, callbacks see a
different source snapshot, lifecycle/error results regress, instruction counts
do not improve or graph/control costs outweigh the measured application benefit.
Native evidence is x86-64 only; ARM64 evidence remains unavailable.

## Review correction before acceptance

The initial experiment was preserved as preliminary evidence and not accepted.
Its fallback skipped the original exact check after cloning. Internal reference
constructors can retain nested reference cells, while Value cloning recursively
unwraps them. A borrowed single-level probe may therefore fail for a snapshot
that is an exact type or exact union member. Keep the canonical owned-snapshot
preparation unchanged on probe failure, including its scope fallback and exact
precedence. A focused nested-reference test covers this boundary. All final
builds and measurements use the corrected source and fresh PGO; preliminary
samples are retained separately and cannot be pooled with final samples.

## Final result and rejection

Corrected-source checks pass across the focused feature configurations,
including nested internal reference chains, union preference, callback mutation,
relative scopes and finally. The exact PGO binary reproduces PHP in four
supported compact/wide lifecycle cases. The existing last-owner callback gap
remains unchanged and is not a PHP pass. The first source variant and every
valid sample remain separate in the evidence packet.

Analysis-only instructions decrease from 79.8961 to 79.1002 billion (-1.00%).
Confirmation analysis time decreases from 7.3357 to 7.2634 seconds (-0.99%);
PHP takes 0.9179 seconds and 10.3382 billion phase instructions. The independent
return holdout uses 4.19% fewer instructions, but its confirmed time grows 2.52%.
The shared-frame and shared-temporary controls take 13.29% and 14.39% more time,
with nearly unchanged instruction counts. This is outside the accepted control
envelope, so the candidate is rejected and no production code is retained.

A short hardware diagnostic reproduces the regression. Shared-frame op-cache
misses increase from 4.61 to 93.80 million and frontend starvation from 0.486 to
2.421 billion event counts. Shared-temporary instruction-cache misses increase
from 3.63 to 23.62 million and frontend starvation from 0.804 to 3.258 billion.
The evidence supports an instruction-supply/code-layout regression; it does not
identify one particular address alignment as the complete cause. This is why
lower instruction count alone is insufficient to accept this native variant.
These diagnostic samples are not pooled with ordinary timing windows.

The last accepted source remains `f24637cce89489cd2a8c323d6dd2b1db4ce2af85fb7cdf0af5dc43d8cee260d7`.
All exact binaries, profiles, source snapshots, rejected diffs and fixture
sources are preserved in private evidence. The public review packet contains
commands, hashes, every valid measurement and the rejection decision:
[measurements](performance-phpstan-borrowed-return-validation-samples.json).
The overall PHPStan parity goal remains open.

Cleanup hooks ran in both local checkouts; superseded Cargo targets were removed
after evidence preservation. No private benchmark host is configured.
