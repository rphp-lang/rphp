# Actual-owner array retirement rejected

Status: removed local prototype; simultaneous PHPStan instruction/time parity
remains active. The accepted PGO scorecard is unchanged at 68.6894G/6.3610s.

The prototype consumed unique array owners inside the common committed-owner
retirement boundary. It moved existing child Values in insertion order through
an explicit stack while retaining each parent payload, keys, buffers and PHP
allocation until its children finished. Shared and referenced roots retained
canonical planning. Value/frame/instruction layouts and unsafe invariants did
not change.

Four focused tests pass. Eight small differential cases match PHP in baseline,
ordinary candidate and a separately compiled canonical variant with existing
plans disabled. They cover storage order, repeated/outside aliases, references,
cycles, resurrection, replacing destructor exceptions, deep arrays, callback
changes to later owners and generator retirement. The ninth case, suspension
inside array retirement, remains a preexisting unsupported boundary: PHP exits
0 and all three RPHP variants exit 255 with identical output. It is not a PHP
pass. These observations are a screening gate, not a full compatibility proof.

Two alternating ordinary-release pairs retain every output-checked sample:
**74.651815G baseline versus 74.670099G candidate (+0.02449%)**. Time medians
are 7.152949/7.173168 seconds (+0.28268%). PHP uses 10.337750G and 0.932916s.
Both Rust binaries use the same toolchain/profile; these ordinary results must
not replace the accepted PGO scorecard. The actual analysis findings, exit and
stdout/stderr hashes match PHP in every observation. Peak aggregate memory is
651,137,024 bytes under the verified 6 GiB/no-swap boundary, with no OOM.

The measured slice yields no material instruction reduction. Do not expand it
with compensating guards or claim a win from saved graph-helper self costs:
those costs do not establish how much this slice removes. Full feature/PGO
cycles were not warranted. Both changed runtime files and the new focused test
are completely restored/removed; exact source, binaries, fixtures and failed
hypothesis evidence remain in local diagnostic storage. Build cleanup ran
before/after the release comparison; no private benchmark host is configured.

Exact identities and all native observations are in
[the data](performance-phpstan-array-retirement-rejected-data.json).
