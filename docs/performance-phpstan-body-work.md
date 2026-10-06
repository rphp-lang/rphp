# Canonical body work and parser iteration control

This diagnostic changes no production runtime and accepts no speedup. The
accepted scorecard remains 66.8068G native analysis instructions / 5.8162s,
versus PHP's 10.3380G / 0.9114s. Simultaneous parity remains open.
The exact [data packet](performance-phpstan-body-work-data.json) separates
instrumented whole-request events from that native scorecard.

The [native inline caller budget](performance-phpstan-main-inline-budget.md)
locates costs by VM source arm; it cannot identify the running PHP body. This
private default+vm-stats build starts from the frozen correct
[owned-frame repair](performance-phpstan-call-frame-ownership.md). Each actual
OpArray lazily retains its own record containing diagnostic strings and Cell
counters, never PHP values or bytecode. The existing valid instruction-decode
boundary records opcode, current PC and previous PC. Feature-off source gains
no field or observation, and unsafe counts remain 1,745 blocks / 319 functions.

All 10,686 executed OpArray records reconcile to 269,826,212 decoded canonical
steps and 5,380,850 backward/self PC transitions. Anonymous bodies are included:
their 12,262,463 steps are 4.545% of the total. Main/include bodies account for
85,105 steps. Instructions executed wholly in native/typed plans or a hot
executor and synthetic fused counts are excluded. Sequences can restart after
calls, catches or resumption; their 17,361,589 starts are not function entries.
Backward transitions are not complete semantic loop counts.

| Body | Decoded steps | Share of captured steps |
| --- | ---: | ---: |
| PhpParser ParserAbstract::doParse | 73,240,648 | 27.144% |
| PHPStan TypeCombinator::doIntersect | 9,902,652 | 3.670% |
| PhpParser NodeTraverser::traverseNode | 6,213,094 | 2.303% |
| PhpParser Lexer::postprocessTokens | 4,754,811 | 1.762% |
| PHPDoc TokenIterator::joinUntil | 4,752,763 | 1.761% |

The top ten bodies cover 44.197% of these steps, and the top fifty 65.896%.
These are VM operation frequencies, not native instruction-cost shares or a
proven removable budget. Full records and archive-origin strings remain private;
the packet publishes normalized leading bodies, source groups and all opcode
totals without personal paths.

Within doParse, ReleaseTemps accounts for 18,759,954 steps (25.614%); FetchObjR,
FetchDimR and AssignCv account for 11,492,944, 6,034,071 and 8,342,035 respectively.
The existing scalar/string specializations and canonical cleanup/exception
boundaries remain intact. Frequencies do not authorize dropping ownership,
pending-exception checks or PHP-visible destruction.

To distinguish extra parser work from expensive VM operations, a separate
private archive adds five source counters inside doParse and snapshots them
at the original analysis boundary. One fresh-cache counted request per
interpreter matches the original exit, stdout, ordinary stderr, five files and
twenty findings. All five events match exactly:

| Parser event during analysis | PHP | Correct RPHP |
| --- | ---: | ---: |
| doParse entries | 331 | 331 |
| Outer iterations | 333,081 | 333,081 |
| Token reads | 263,696 | 263,696 |
| Inner iterations | 435,666 | 435,666 |
| Reductions | 435,335 | 435,335 |

All pre-analysis counters are zero on both interpreters. This excludes extra
iterations at these dominant parser sites, including inner loops omitted by the
[older entry probe](performance-phpstan-application-work.md). It does not prove
equal work in every application body. Diagnostic statements can inhibit
optimized paths; their instruction counts and elapsed times are not acceptance
measurements. The remaining hypothesis is the cost of shared execution,
publication and retirement across these operations, rather than extra parser
iterations or optimizing each named PHP function separately.

Four focused counter tests and reader reconciliation checks pass. Sixteen
ownership controls give 48 equal PHP/repair/diagnostic observations; the actual
three-runtime request also matches. Every per-body opcode sum, full opcode
matrix, backward-transition total and sequence total reconciles. Dedicated
decoded opcode counts never exceed the global counters that also include fused
observations. No row is silently omitted or inferred from reused addresses.

The first loop reader rejected PHP's valid empty pre-analysis array. The failed
attempt remains visible; the repaired reader normalizes absent zero events,
rejects unknown events and reuses the intact completed PHP request after hash
checks. Only the previously unexecuted RPHP request follows. Build/run/control
services verify 6 GiB, no swap and group OOM handling, with zero OOM events;
final loop control peaks at 630,497,280 bytes. Cleanup completes in both local
checkouts and disposable diagnostic target storage is retired. Source snapshots,
exact binaries and the active comparison baselines remain available.

No new runtime migration is admitted from these counts alone. The next change
requires a measured shared protocol and its native instruction budget, plus
proof of reference/COW, callback, exception and lifetime behavior on the owned
baseline. These diagnostics provide a narrower target, not parity or a delivery
date.
