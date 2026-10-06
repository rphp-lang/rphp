# Extended frame ownership experiment

Status: rejected; all production source restored. PHPStan parity remains open.

The accepted PGO analysis profile assigns 3.9755 billion exclusive executor
instructions to ReleaseTemps, the largest separately attributed opcode body.
The experiment adds bounded ownership words after wide frames' Value storage,
while preserving the 64-byte header, initialized tail slots, slot numbers and
release plans. Slot publishers, transfers and retirement maintain those words;
wide interval queries skip empty words instead of inspecting Values. This is
a general ownership representation change, without workload recognition.

The predeclared exploratory gate requires at least one percent fewer application
instructions before a fresh PGO and broader feature cycle. Default test-fast
native analysis counts are **117.1511 billion baseline** and **118.5791 billion
candidate**, a **1.219% increase**. Analysis times are 10.5160 and 10.5236 seconds;
PHP measures 10.3378 billion instructions and 1.0119 seconds in that window.
These optimization-level-one measurements are separate from the accepted PGO
74.2814 billion result. No release improvement is claimed.

All **65 focused checks** pass, covering owner writes/transfers at 64/128-slot
boundaries, compact materialization, cross-page stacks, references, statement
and return retirement, and static bindings. Eight exact lifetime CLI controls
and the entire PHPStan exit/stdout/stderr match reference PHP. Formatting passes.
The initial unsafe-policy check fails visibly: the experiment adds six unsafe
helper functions beyond the unchanged ceiling, and initially lacks two function
safety sections. Those sections are repaired before the final preparation;
the ceiling is never weakened and the experiment is not committed. Its larger
allocation/publication bookkeeping has not earned adoption; the native sampled
profile is retained for diagnosis, without asserting an exact causal split.

The final preparation takes 119.918 seconds including rebuilding both the CLI
and unit-test executable. The single native pair, reference and separate
instruction profile take **42.224 seconds**. Peak preparation memory is
3,275,849,728 bytes under the verified 6 GiB/no-swap boundary, with zero OOMs.
The failed selection, exact sources and executable remain private evidence;
the disposable Cargo target is removed. No PGO or full feature matrix is run
for this rejected candidate. The accepted runtime source remains
`9635606606194d209de413515955f5159194a1edbf5f456b3b96dff7a080c967`.
