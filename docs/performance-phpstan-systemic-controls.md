# PHPStan: controls for broad executor hypotheses

Status: read-only diagnosis; production source is unchanged. Simultaneous
instruction/time parity remains active at the accepted PGO **68.6894G / 6.3610s**
versus PHP **10.3377G / 0.9294s**. The ordinary/diagnostic observations below do
not replace that scorecard. Exact identities and every valid observation are
in [the data](performance-phpstan-systemic-controls-data.json).

The [saved Zend/RPHP comparison](performance-phpstan-vm-contrast.md) separates
approximately 1.513x more bytecode steps from 4.508x more instructions per step;
the bytecodes are not equivalent semantic operations. Zend's
[generator](https://github.com/php/php-src/blob/PHP-8.5/Zend/zend_vm_gen.php)
substitutes operand storage variants before execution, including direct
literal/slot access and operand release. Its
[operand release](https://github.com/php/php-src/blob/PHP-8.5/Zend/zend_execute.c)
frees TMP/VAR operands inside handlers. These are concrete structural differences
to investigate in Rust. They are not evidence that C itself provides a several-fold
speed advantage, or that operand specialization alone closes the application gap.

## Wide ownership intervals

A private exact-source ordinary release build with VM statistics adds visit
counters only. Production stays byte-identical; no layout, writer, ownership or
unsafe change is admitted. Counters cover the whole command, including startup.
Their instruction counts include instrumentation overhead and are not speed claims.

| Whole-command counter | Count |
| --- | ---: |
| Wide release markers | 15,364,958 |
| Declared tail span | 27,169,875 |
| Actual short-circuit tail guard visits | 25,129,801 |
| Native cleanup tail visits | 57,223,808 |
| Committed-return tail visits | 57,188,292 |
| Statement predicate tail visits | 10,948,199 |
| Live statement predicate results | 7,637,354 |

The mean declared tail span is **1.768 slots per wide marker**. The marker count
is not a slot-visit count; overlapping scans are not additive removable costs.
This does not identify a quadratic long-range scan or justify a broad tail bitmap
migration as the next large parity change. Modest benefits remain unmeasured;
every added writer store, metadata word and callback invariant would also count.

The first observation attempt lacked the phase PHAR's required perf FIFO
endpoints and failed before analysis. It remains a failure in the packet.
Repairing only the observation environment, without rebuilding, produces known
exact reference/baseline/diagnostic outputs after the marked statistics suffix
and timer markers are isolated.

## Seven existing execution plans

With statistics disabled, wrappers run the **same exact diagnostic executable**
with seven existing plan switches enabled or disabled. Generic fast calls,
caches and other optimizations remain enabled; this is not a fully canonical
interpreter comparison.

Two alternating analysis-only instruction pairs yield **78.312749G enabled**
and **78.777028G disabled (+0.59285%)**. Analysis time medians are 7.49990/7.51807s.
Every output matches PHP. Dormant instrumentation and the ordinary vm-stats
configuration prevent comparison with the accepted PGO absolute scorecard.

Wholesale removal of those plans does not improve this application. Benefits
and failed-probe overhead can offset; this result does not prove every failed
guard is cheap or reject precomputed structural routing.

## LLVM machine tail merging

Exact accepted disassembly contains native-stack Value transport and jumps into
shared slot-write blocks. Saved main-executor stack traffic includes required
materialization as well as spills; neither source-PC samples nor the aggregate
stack category establishes a removable budget. One same-source ordinary release
build changes only LLVM `enable-tail-merge=false`, an option confirmed by the
pinned compiler's local help. Earlier LLVM IR sharing is still enabled.

Two alternating pairs record **74.637084G baseline** versus **74.579032G diagnostic
(-0.07778%)**. Time medians are **7.13254/7.36781s (+3.299%)**. The diagnostic fails
the declared one-percent instruction selector, so no PGO/full feature expansion
runs and no production flag changes. The time result is an observation, not an
independently confirmed corpus regression. The main executor grows from
352,495 to 409,865 bytes; executable size grows from 24,562,976 to 25,107,552 bytes.
Exact PHP output and 100% running instruction counters hold for every sample.

This rejects this particular compiler switch as the next parity improvement.
It does not reject every source partitioning or result-ownership design. A broad
handler rewrite still requires a measured operation boundary and real coverage;
reducing generated code size alone is not an application instruction gain.

All expensive work uses verified separate 6 GiB/no-swap/whole-group services,
CPU affinity, exclusive benchmark windows and automatic cleanup. No OOM occurs;
no private benchmark host is configured. Source, exact binaries and failed/valid
raw evidence remain retained locally. No production source, PGO scorecard, JIT
coverage or compatibility guarantee changes in this checkpoint.
