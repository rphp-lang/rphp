# Executor traffic diagnostic

Status: read-only follow-up to the [Zend/RPHP contrast](performance-phpstan-vm-contrast.md).
No runtime change or performance gain is accepted here.

The [position reconciliation](performance-phpstan-pc-reconciliation.md)
supersedes this report's unmatched-boundary and stack-category numbers. An
association compression-base error caused the RPHP discrepancy; PHP's remaining
addresses were in cold code outside its named hot symbol. Both main PC maps
now match every executed instruction boundary, with totals unchanged.

The same retained RPHP profile has 27.916272G own instructions in its main
executor and another 17.478290G in functions whose symbols explicitly begin
with the VM module or a VM type. Together those disjoint buckets are 45.394562G,
63.89% of the full 71.045942G profile. Generic descendants remain in other
buckets. This confirms that the larger opportunity is repeated execution work;
it does not identify all that work as removable.

Mapping saved main-executor PCs to the exact executable finds 4.994722G
instructions with explicit native `%rsp` memory operands, excluding address
calculations. These include Value materialization and callback inputs as well
as register spills. Control-flow instructions account for 6.086933G; other
explicit memory instructions account for 5.620838G. These are machine categories,
not PHP semantic categories or predicted optimization gains.

Another 1.317066G cannot be matched to an instruction boundary by this static
disassembly and remains unclassified. PHP's main-executor check similarly leaves
0.087830G unclassified. The cause is not established. Existing per-opcode body
figures are provisional until this PC discrepancy is reconciled; whole-profile
and per-function self totals, hardware A/B counts and the dispatch comparison
do not depend on this particular static-PC annotation.

The initial decoder skipped cost records without event fields. The
[Callgrind specification](https://valgrind.org/docs/manual/cl-format.html)
requires absent events to be zero and relative positions to use the preceding
cost record. Decoding all 44,103 zero-event records is necessary for a general
reader, but changes **none** of the selected RPHP PC costs in this profile.
It therefore does not explain the unmatched boundaries. Initial failed
all-address assertions remain failures; the final report includes an explicit
unmatched bucket instead of treating it as decoded stack traffic.

This diagnostic does not compare all PHP handler stack traffic: PHP's separate
handlers are outside its main-executor bucket. It also does not justify a blind
executor split, error ABI migration or assembly rewrite. The next prototype
needs a general representation or control-flow change, a measured instruction
budget, preserved canonical behavior and a material ordinary native selector
before a broader PGO/feature cycle. The
[argument-only transfer attempt](performance-phpstan-argument-transfer-rejected.md)
was removed after only a 0.60% instruction improvement.

All categories, exact binaries, mapped sites and unresolved addresses are in
[the data](performance-phpstan-executor-traffic-data.json). Saved older Callgrind
counts are not current native timing results. Simultaneous parity remains open.
