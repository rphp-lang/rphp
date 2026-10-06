# PHPStan: typed pending-call state

Accepted portable Rust checkpoint on `codex/perf-phpstan-cold`, baseline
`82b60bc3` (production source `2488921c`). Independent PGO confirmation lowers
analysis **69.6932 to 69.0416 billion instructions
(-0.935%)** and records **6.5422/6.3595 seconds**.
PHP records 10.3378 billion instructions.
The remaining **6.68x instruction gap** keeps instruction/time parity active.
[All retained observations and identities](performance-phpstan-typed-call-state-samples.json)
cover the identical five-file serial analysis; this is a narrow application result.

The exact preceding full Callgrind profile records 11,858,531 calls to dynamic
scope discard (0.62991 billion inclusive instructions) and 6,686,859 calls to
late-static discard (0.51185 billion inclusive). Those helper edges overlap
frame-pop costs; they are not an additive saving forecast. Their trusted frame
keys used general keyed hashing, while pending call metadata used PHP arrays,
Value tags and COW machinery.

Pending receivers, magic names and late-static IDs now live in a typed Rust
stack. Actual Values remain owned once and drop in insertion order. Top-key
mismatches do not consume another activation; magic lookup stops at the newest
matching record. Suspended roots expose those actual owners. The old engine-only
array wrapper was never cloned or admitted as a possible cycle root, so removing
it does not bypass a PHP callback boundary. Legacy embedding Values retain their
original packed-array behavior through `From<Value>` and a read-only accessor.
Embeddings assigning the public executor field now use `.into()`.

Dynamic-variable tables reuse the existing request-seeded numeric identity
hasher only for engine-generated frame addresses. PHP string/symbol hashing and
ordered per-frame variables remain unchanged. Static assertions preserve the
optional pending-field and hash-map geometry. Value/frame/cache/instruction
sizes stay 16/64/16/16 bytes; no ASM, new VM admission guard, allocation on an
ordinary state-free call, unsafe inventory change or native lowering is added.
Native evidence is x86-64 only; no ARM64 speed claim is made.

Final ordinary release selection saves **0.541%**; it is feedback before fresh
PGO. PGO first/independent windows save **0.925%/0.935%**, with two alternating
CPU-2 pairs each, no warmup, fresh TMPDIR, complete `instructions:u` counter
running time and exact PHP stdout/stderr/exit. Rust 1.98.1/LLVM 22.1.8, default
features, fat LTO, one codegen unit, line tables and function alignment 6 match.
The fixed eighteen-program training excludes this application and controls.
All 54 training observations agree with PHP. Final instrumented/profile-use
builds take 244.30/195.67 seconds; 27 build-script profile warning lines remain
visible and are not runtime-output failures.

**284 successful focused tests** pass under default, no-default and all
features, including original opaque-state assertions. Two previously ignored
prototype performance tests remain ignored and are not counted. All-target/
all-feature checking, formatting and unchanged unsafe gates pass. Twelve CLI
programs match PHP on baseline, final candidate and forced-canonical candidate:
48 observations cover nested calls, magic names, late-static scope, dynamic
variables, aliases, exception cleanup, Fiber suspension and sole pending-owner
reachability. A review catches empty embedding state being cleared by an
unmatched discard; the original behavior is restored and a regression test added
before the final source repeats gates, selection and PGO.

Seven independent controls use three randomized pairs. Two timing excursions
trigger the fixed, unselective five-pair review of all seven; every one of the
112 valid observations is retained. Review instructions range from essentially
unchanged to +0.131%. **Inherited-method time +1.653% and return-scope time
+2.881% do not pass the one-percent timing ceiling.** The integrating task retains
these explicit tradeoffs under the user's instruction priority and the confirmed
application instruction/time gains. No particular latency or layout cause is
proved. Shared-frame and trait controls improve 1.333% and 3.308% in that review.

Executable size is 76,514,248/76,543,752 bytes; executor code is
391,171/391,786 bytes. All expensive jobs use a verified separate 6 GiB boundary,
zero swap, whole-group termination and the exclusive benchmark lock. The largest
final-job peak is 4,865,085,440 bytes, without OOM or timeout. Interrupted earlier
jobs and one unavailable-lock attempt are retained as non-passes. Source snapshots
and exact binaries remain private evidence; cleanup runs locally, and no private
benchmark host is configured. The original parity goal continues with the full
executor/call/retirement budget rather than another allocator replacement.
