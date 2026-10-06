# Rejected Rust opcode-function executor

Two Rust executor prototypes reject the idea that splitting the
canonical VM into one native function per opcode is sufficient to repair the
PHPStan instruction gap. The sibling-transfer version is rejected before PHP
bytecode execution because native calls grow the stack. A bounded ordinary-call
version preserves focused and actual application outputs but increases native
analysis instructions **73.7814 to 77.5971 billion (+5.1716%)** across four pairs
in two independent windows. Time medians are **6.6682/6.8574 seconds (+2.8368%)**.

Production runtime is unchanged from source
`76dd1a94701101cf8b425e4cd325ab6da07cb5bba313c71880562423bb2cbbe1`
on documentation head `a280c750`. Accepted PGO remains **67.5210 billion /
6.0081 seconds**, versus PHP **10.3380 billion / 0.9445 seconds**. Simultaneous
instruction/time parity is still open. The experiment uses ordinary release,
default features, without PGO or runtime statistics; it is not a comparison of
the candidate against the accepted PGO binary.

## General mechanism and semantic boundary

A private source transformation moves all 161 declared opcodes into one shared
macro source, comprising 131 source groups including complementary feature
branches. That source expands into the original loop for forced canonical
comparisons and smaller Rust opcode functions. Each operation keeps its original
operand, reference, COW, cache, frame, exception, diagnostic and cleanup work.
Raw context metadata is projected only within a live instruction handler;
references are not retained across frame retirement. The shared interrupt tick,
GC poll, previous instruction origin and activation changes follow the original
boundaries. No workload name, literal or hot-shape admission rule is introduced.

The first version asks LLVM to lower sibling continuations between these
functions. The next-step dispatcher has an indirect tail jump, but **52**
functions retain an ordinary call to that dispatcher, followed by their
individual epilogues and returns. That continuation can grow the native stack
with bytecode length. This binary is rejected by the assembly audit and never
executes PHP bytecode. Stable Rust source alone provides no mandatory tail-call
contract for this architecture.

The second independent version replaces that continuation with a fixed shared
loop. One indirect ordinary call runs the selected handler and receives a bool
before the backedge. A 256-entry static table selects handlers without an
allocation; each operation returns before the next begins. Native audit finds
one indirect call in the loop and no handler calls back into the dispatcher.
The stack is bounded across VM steps without requiring sibling-call lowering.

## Native result and attribution

All eight ordinary native rows have 100% counter running and the same expected
exit, stdout and ordinary stderr. Independent windows increase instructions by
**5.17237% and 5.17437%**. All valid rows are retained, with no outlier deletion.
Each window also validates the same PHP reference at approximately 10.338G /
0.94 seconds. CPU 2, fresh temporary state and the analysis FIFO define scope.

Separate instruction-sampling runs over the exact ordinary binaries lose zero
samples. Rounded full symbol reports put original main self at approximately
**24.5964G**. The candidate loop plus opcode functions total approximately
**27.6444G**; the shared next-step loop alone contributes **4.4392G**. Smaller
operation bodies therefore do not pay for the new transition and context costs.
These are approximate self-attribution samples, not exact per-opcode counts or
a removable budget; the shared loop also contains required interrupt/GC work.
The profile does not assign the complete regression to call/return alone.

A separate one-pair diagnostic forces the canonical expansion inside the same
candidate binary. It increases instructions **73.7746 to 74.1887G (+0.5613%)**.
Thus code-generation/source-layout changes also affect the comparison. This
short diagnostic is not an independent acceptance run, and subtracting its
cross-window result does not establish an exact transition budget.

## Validation, failures and rejection

Nine controls compare PHP, accepted ordinary release, new executor and
same-source forced canonical: **36 exact output observations**. They cover
arithmetic, values, references, COW, caches, callbacks, cleanup, exceptions,
wide frames and an automatic-GC model. Actual PHPStan output also matches all
three ordinary application runtimes. These focused results do not certify the
full coroutine, interrupt or feature envelope.

Formatting, the pinned release library check and source unsafe inventory pass:
**1,748 blocks / 321 functions**, within unchanged production ceilings of
1,749 / 321. Fourteen structural compiler warnings remain visible. Early
source-generation/compilation failures and one output-harness environment-key
failure are retained privately and are never counted as passes; the final
frozen source and binary pass the focused gates.

The hypothesis is rejected by its native instruction stop rule. No PGO build,
broad feature matrix or ARM64 acceptance gate is run for the slower design.
Both disposable build targets are removed, while exact sources, binaries,
assembly and measurement evidence remain preserved. Cleanup hooks complete in
the workspace and worktree; no private benchmark host is configured.

Exact source/binary identities, all native rows, audit summaries, profiles and
scope limitations are in
[`performance-phpstan-rust-dispatch-rejected-samples.json`](performance-phpstan-rust-dispatch-rejected-samples.json).
This diagnostic report does not publish or integrate either failed executor.
The result rejects these specific source/ABI designs, not every possible Rust
VM representation. Subsequent work needs a quantified shared semantic cost;
compensating opcode rules on this rejected dispatcher are not a checkpoint.
