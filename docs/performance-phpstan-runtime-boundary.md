# Rejected runtime boundary experiment

Combining shared call admission with scalar destination vacancy checks removes
0.77% of PHPStan analysis instructions but confirms substantial timing
regressions. The entire implementation is removed; accepted source remains
`2488921c`. The instruction/time parity goal remains active at about 69.68
billion instructions against PHP's 10.34 billion.

The call change queried existing pending side state once across mutually
exclusive user-call protocols, after attributes, generic contracts and Closure
argument normalization. Scalar writes applied the generic writer's existing
initialized-tail vacancy proof before outlined retirement. Neither component
added an eligibility fact, unsafe block, VM layout or native lowering. Their
standalone ordinary-release savings, 0.225% and 0.372%, each failed the declared
0.5% selection threshold and were restored before the combined experiment.
The combined ordinary release saved 0.583%, selecting it for final validation.

Fresh independent PGO used the unchanged eighteen-program manifest, excluding
PHPStan and every acceptance control. Two alternating two-pair native windows
saved 0.787% and 0.766% analysis instructions, with exact PHP output. The second
window gives 69.6744/69.1408 billion instructions but 6.4067/6.5243 seconds
(+1.84% time). All valid samples are retained.

Two complete randomized three-pair windows cover all seven retained controls.
Shared-frame retirement regresses 10.79%/9.57% in time despite instructions
falling 0.285%; shared temporary reads regress 5.12%/4.13% despite instructions
falling 0.623%. These confirmed regressions reject the candidate. Other controls
have mixed timing results; no selective rerun or new exception is used.
Instruction savings alone do not establish faster execution. No exact frontend
or code-placement cause is proven by these measurements.

All 870 focused default/no-default/all-feature executions pass, including the
existing scalar writer ownership test and deprecation/NoDiscard callback gates.
Seventeen final-PGO programs match PHP and forced canonical execution. Formatting,
unsafe inventory and all-target checks pass. Native validation is x86-64 only;
no ARM64 result or JIT coverage change is claimed.

The PGO executable grows 23,888 bytes (0.031%). Whole-command confirmation
retains identical five-file diagnostics and uses 620,412/620,588 KiB RSS. The
feature/PGO service reaches its six-GiB ceiling and records 112 limit-reclaim
events, but zero OOM and zero swap. Twenty-six profile-data warnings concern
build-script functions only; a summary line accounts for the twenty-seventh
warning line. The initial warning classifier mistakenly treated that summary
as a function warning; its failure and correction remain in private evidence.

Both production files are byte-for-byte restored to the accepted source
fingerprint. Exact rejected sources, patches, binaries, profiles and raw
streams remain private; task-scoped PGO and feature build directories are
removed and the mandatory cleanup hook passes. No private benchmark host is
configured. [The portable evidence](performance-phpstan-runtime-boundary-samples.json)
contains every valid native window, all seven controls and validation hashes.
The next diagnostic isolates the real parser loop for faster instruction-site
feedback, with full PHPStan retained as the acceptance workload.
