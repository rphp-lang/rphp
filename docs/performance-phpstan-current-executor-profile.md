# Current accepted executor instruction profile

Status: read-only diagnosis; the original simultaneous PHPStan parity goal
remains active. Runtime source is unchanged and exactly restored after the
[rejected array-retirement slice](performance-phpstan-array-retirement-rejected.md).

A fresh single hardware-count observation of the accepted PGO executable
records **68.682525G / 6.411633s**, against PHP **10.337848G / 0.929031s**.
These observations reproduce the existing gap and do not replace the accepted
paired scorecard or claim a new speedup. Exit and both output hashes match the
same actual analysis exactly. Counting is limited to the analysis FIFO boundary,
with 100% counter running time, CPU affinity and a fresh temporary directory.

A separate instruction-sampling run uses the identical executable and input.
There are no lost samples. Main executor self accounts for approximately
**40.42%** of the sampled instruction events. Other observed self shares include
memmove 2.62%, regex sequence matching 2.05%, return-owner retirement 1.96%,
instanceof 1.88%, graph release inspection 1.65%, memcmp 1.10% and class lookup
0.97%. These are disjoint flat symbol shares, not inclusive subtree costs.
The raw 0.5%-threshold listing covers 65.78%; smaller/unlisted symbols retain
34.22% and must not be silently discarded.

Native PCs from the main symbol were decoded using the exact retained
executable. Candidate source locations include dispatch, slot/result writes,
assignment and per-instruction polling. Instruction-event IPs can skid and
optimized/shared code can carry misleading source lines: a dispatch PC even
maps to an unrelated opcode body line. This is not a replacement exact
per-opcode budget and does not reconcile the earlier Callgrind PC discrepancy.
A hot sampled instruction or source line alone does not admit an implementation.

The result keeps broad execution/representation work as the priority. It does
not establish that eliminating frame cleanup, assembly conversion, outlining
the whole executor or recycling more TMPs would remove the remaining gap.
The rejected array slice already shows that a locally simpler ownership walk
can leave full application instructions unchanged. The next runtime contract
must identify repeated work across operations and quantify its actual coverage,
rather than add another narrow lifetime guard.

All profiling/decoding runs use the verified 6 GiB/no-swap aggregate boundary
and exclusive lock. Peak profile memory is 654,589,952 bytes, with no OOM.
Automatic build cleanup runs before/after the checkpoint; no private benchmark
host is configured. Exact binary identities, every native observation and
sampled symbol shares are in
[the data](performance-phpstan-current-executor-profile-data.json).
