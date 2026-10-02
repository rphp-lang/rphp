# Consumed argument owner transfer: rejected performance prototype

Status: runtime prototype removed; simultaneous PHPStan instruction/time parity
remains active. Accepted PGO analysis stays at 68.6894G and 6.3610 seconds.

The prototype transferred a proven single-definition, single-consumer heap TMP
into a pending user call instead of retaining a second caller owner. It did not
recognize source names, benchmark literals or PHPStan. CVs, references, internal
call snapshots and scalar backedge bytes retained their existing behavior.

Reference PHP exposed a baseline lifetime gap: a directly constructed or
function-returned temporary object should die when the callee unsets its only
argument. The accepted runtime keeps its caller TMP alive until the statement
ends. Abandoning a pending call after a later argument throws also requires
PHP-visible owner retirement before catch selection; destructor exceptions can
replace the original exception. The local prototype addressed both boundaries.

Four focused tests passed. Four supported differential programs matched PHP in
twelve observations across PHP, ordinary release and a separate vm-stats build
with quick loops and direct/composed/deferred/getter plans disabled. They cover
call forms, references, COW, wide frames, abandoned arguments, type errors,
replacement exceptions and suspension during an ordinary callee unset.
Suspension while retiring an abandoned pending argument still exits 255 in both
RPHP paths versus PHP's exit 0. That preexisting unsupported boundary remains a
failure, not a compatibility pass. These focused checks do not establish the
full feature or callback envelope.

Two ordinary-release analysis pairs, alternating baseline/candidate order,
match PHP's exit code and both output hashes. All observations are retained.

| Analysis, ordinary release | Baseline | Candidate |
| --- | ---: | ---: |
| Median instructions | 74.644573G | 74.197281G |
| Instruction change | baseline | -0.599228% |
| Median elapsed time | 7.147750s | 7.153620s |

PHP's reference observation is 10.337789G and 0.941099s. This exploratory selector
uses ordinary release, whereas the accepted scorecard uses PGO; its absolute
counts must not replace that scorecard. The selector has no warmup, uses fresh
temporary storage, pins one CPU, counts only the actual analysis through FIFO
controls, and runs under an aggregate 6 GiB/no-swap boundary. Peak memory is
666,963,968 bytes; no OOM events occur.

The small instruction improvement does not justify a broader performance
migration. No PGO cycle or full feature matrix ran. Exact candidate source,
test, patch, binaries, failed intermediate attempts and observations are kept
as local evidence; owned runtime files are restored to `d05dd37b`. The lifetime
counterexamples remain a separate compatibility finding, not an accepted fix.
See [the public packet](performance-phpstan-argument-transfer-rejected-data.json).
