# Reconciled executor instruction positions

Status: verified profiling-reader repair; production is unchanged. Simultaneous
PHPStan parity remains open at the accepted **68.6894G / 6.3610s**, versus
PHP **10.3377G / 0.9294s**. No new runtime speedup or analysis run is claimed.
[Exact identities and results](performance-phpstan-pc-reconciliation-data.json)
cover reanalysis of the retained older profiles.

## Cause of the unexplained addresses

The earlier [traffic diagnostic](performance-phpstan-executor-traffic.md) left
1.317066G RPHP instructions unmatched. All 193 affected addresses occupy one
short region and are exactly one byte after a real instruction boundary.
One indirect-call association has a source position different from the
preceding ordinary cost position. Updating the reader's compression base from
that association shifts subsequent ordinary positions until the next absolute
address.

The exact official [Valgrind 3.22.0 source archive](https://sourceware.org/pub/valgrind/valgrind-3.22.0.tar.bz2)
confirms that fprint_fcost updates the compression base; fprint_jcc prints
association source/target positions without updating it. The
[format manual](https://valgrind.org/docs/manual/cl-format.html) does not
explicitly distinguish that writer behavior from ordinary cost records.
The earlier missing-zero-event fix was insufficient because it still advanced
the base for association records.

The reusable [reader](../scripts/callgrind_reader.py) follows the actual writer.
It retains zero costs, source and target positions, event order, compressed
names and distinct objects; inclusive call costs never enter self totals.
Five focused tests cover the shifted call source, conditional/unconditional
jumps, ordinary zero-cost positions, sparse events, object identities, and
invalid totals/unfinished associations.

## Independent checks on retained evidence

Both exact executables and profiles are hash-verified. Every RPHP function's
self total agrees with the preceding budget. Full analysis totals remain
71.045942G RPHP and 10.413595G PHP; main totals remain 27.916272G and 3.140080G.
RPHP's 19,619 and PHP's 4,001 executed main positions now all match actual
instruction boundaries. PHP includes cold addresses outside its named hot
symbol, so its boundary check uses the whole executable.

The previous 322 recognized Zend function/context rows cover **318 distinct
physical functions**. Summing duplicate context rows preserves the original
3.377659G handler total. Main plus these handlers still costs 6.517739G.
Other PHP functions remain separate.

| Exact self-cost machine category | RPHP main G | PHP main plus recognized handlers G |
| --- | ---: | ---: |
| Explicit RSP memory | 5.129215 | 0 |
| Push/pop | 0.016887 | 0.346229 |
| Control flow | 6.379919 | 1.469745 |
| Other explicit memory | 5.833895 | 2.621337 |
| Other | 10.556356 | 2.080430 |
| Unmatched boundary | 0 | 0 |

These are executed machine categories, not a semantic or removable-cost
partition. RSP traffic includes required Value materialization and callback
inputs as well as spills. The table excludes RPHP's other helpers and PHP's
other functions. It does not prove all stack traffic is avoidable, or that
outlining handlers would produce the difference as a saving.

## Corrected operation priorities

Rebuilding the conservative direct/executed machine CFG preserves all
247,231,188 RPHP dispatches. Unique bodies account for 22.968700G, shared
tails/setup for 4.934526G, and semantic attribution remains unknown for
0.013046G. Those disjoint buckets sum to the unchanged main total.

| RPHP operation body | Corrected lower bound G | Previously provisional G |
| --- | ---: | ---: |
| ReleaseTemps | 3.675814 | 3.675814 |
| FetchObjR | 2.832544 | 1.893567 |
| DoFcall | 2.481050 | 2.478827 |
| Return | 2.319707 | 2.300853 |
| AssignCv / BindCvRef | 2.147168 | 2.109068 |
| FetchDimR | 1.939543 | 1.620764 |

Helpers and shared code are excluded; these remain lower bounds rather than
equivalent PHP-operation costs. The newly assigned property/dimension bodies
change the next investigation: quantify the common operand/result and slot
ownership protocol across reads/writes before choosing another implementation.
The additional 0.939G property and 0.319G dimension costs were already in the
total, not newly discovered execution or a measured saving. A broad rewrite
still requires identified removable work and an application A/B.

Reproduction of the reader gate:

~~~sh
python3 -m unittest discover -s scripts/tests -p test_callgrind_reader.py -v
~~~

All diagnostics run under verified six-GiB/no-swap aggregate services and the
exclusive lock, with cleanup before/after. Largest peak is 844,877,824 bytes;
no OOM or timeout occurs. Initial import, hot-symbol-only, and handler-context
assertion failures remain failures in the packet. Exact profiles/binaries and
raw source diagnostics stay local; no private benchmark host is configured.
No production Rust source, unsafe inventory, PHP semantics or native scorecard
changes, and the original parity goal is not complete.
