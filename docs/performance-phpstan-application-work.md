# Current PHPStan application-work control

Status: verified diagnostic, with production unchanged. The accepted native
analysis result remains **68.6894G / 6.3610s**, versus PHP **10.3377G / 0.9294s**.
This control addresses whether RPHP executes substantially more application
work, rather than proving a runtime saving.
[Exact identities and observations](performance-phpstan-application-work-data.json)
identify the accepted PGO executable and retained counted archive.

One original reference run and one counted run per interpreter use the same
project, process-disabling INI settings and fresh cache directories. All three
analyse five files and produce the same twenty findings, ordinary stderr and
exit status. The retained probe counts 23,557 named-function/ordinary-closure
body positions across 3,014 archive files; no new instrumentation is generated.

| Whole-command body-entry diagnostic | PHP | Accepted RPHP |
| --- | ---: | ---: |
| Counted body entries | 9,605,729 | 9,537,284 |
| Entered body positions | 6,818 | 6,809 |
| ParserAbstract::parse entries | 331 | 331 |
| NodeTraverser::traverse entries | 1,685 | 1,685 |
| NodeTraverser::traverseNode entries | 63,069 | 63,069 |
| NodeTraverser::traverseArray entries | 27,948 | 27,948 |

RPHP has **0.7125% fewer** counted entries. Of the entered positions, 6,733
have exactly equal counts; 85 mapped positions differ. Equal-count positions
account for 7,967,099 entries on each side. Summing positive/negative differences
gives 1,849 extra and 70,294 fewer RPHP entries; their net is -68,445.

The largest differences still involve reflection-driven dependency handling
and native-versus-polyfill selection. Their existence prevents claiming that
every application branch is identical; this run does not establish every
cause. The retained earlier control had 773 differing positions and -0.160%
total entries. Neither complete counter vector repeats exactly, even on PHP;
the new observation stands separately and is not pooled with the old one.

Together with the [executor comparison](performance-phpstan-vm-contrast.md),
these counts rule out an order-of-magnitude increase in counted application
function entries. They support prioritizing the VM's operand, result, call and
ownership protocols over a hypothesis that PHPStan repeatedly invokes many
more PHP functions. The separate 36.411M ReleaseTemps dispatches and more
expensive executor bodies are runtime mechanisms, not evidence of that many
additional PHPStan functions. No measured removable-work budget follows merely
from identifying a representation difference.

## Limits and verification

Counters cover the whole command, including bootstrap and rendering, rather
than the native analysis-only counter boundary. They exclude arrow-expression
and native-function bodies, do not count inner iterations or work inside a
body, and added PHP statements can inhibit optimized paths. Thus the result
does not prove equal inner-loop algorithms, full PHP compatibility or a runtime
speedup. Instrumented elapsed times are diagnostic only and cannot replace the
accepted uninstrumented native scorecard.

All executable/archive/mapping identities are verified before and after.
Production source remains byte-identical; no Rust, unsafe or coverage change
requires another feature matrix. The aggregate service verifies six GiB,
no swap and group OOM handling, owns the exclusive lock, and completes in
13.5435 seconds with 719,994,880 bytes peak, no timeout and no OOM.
Cleanup runs in both local checkouts; no expensive descendants survive and
no private benchmark host is configured. Simultaneous parity remains open.
