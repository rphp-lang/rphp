# Guarded direct-call regions

Status: accepted and integrated on `main`, 2026-08-15

## Checkpoint contract

- **Outcome:** execute a stable immutable closure call inside an existing typed
  region without constructing and cleaning a canonical PHP frame on every
  iteration, while retaining exact canonical fallback semantics.
- **Baseline:** clean `main` commit
  `385dc1c6f8a72da201675a32b31ba6b50b38c03c`.
- **Measured candidate:** clean `codex/perf-direct-call-regions` source commit
  `0f8b5b9cc66d8602d5d988abe3ea9040c7b22a1b`. Documentation-only commits
  after that source commit do not change the measured binary.
- **Integration:** the integrating agent accepted the measured checkpoint after
  the full joint gate. A separate follow-up commit, `4660315`, fixes a
  pre-existing wide-`__invoke` frame-initialization failure found by the gate;
  it does not widen or change the measured direct-call region.
- **Scope:** an exact positional by-value wrapper whose callable is either one
  immutable receiver property or a proven-dead local alias of the first public
  argument. The leaf closure has a stable user-function identity, no binding
  or called scope, a pure scalar Long body and at most eight public/capture
  inputs. Immutable by-value Long captures and String captures consumed by
  `strlen` are admitted.
- **Stop conditions:** return-by-reference, by-reference captures/arguments,
  `$this`, changed scope or binding, generators, globals, statics, try regions,
  unsupported types/operations, live aliases, changed callable identity or a
  failed guard. Rejection resumes the original canonical instruction before
  mutation.
- **Regression ceiling:** no slowdown above one percent on default-JIT
  order/ledger/routing and typed-only order/neutrality controls in the accepted
  distribution on either architecture.

These results cover only the supported workloads and guarded region described
here. They are not evidence that RPHP is faster for arbitrary PHP programs.

## Implementation and semantic boundary

The compiler records a cold capture-aware scalar plan on the closure's
`UserFunction`. It describes public arguments and immutable capture inputs but
does not retain request-owned capture values. At region entry the runtime
guards the actual closure identity, scope, binding, capture layout, reference
state and types. It then substitutes immutable capture values into an ordinary
ephemeral `ScalarLongFunctionPlan`.

Both closure-property calls and public-argument wrapper calls use the existing
scalar call ABI and the same typed/native lowering as named functions and
monomorphic methods. There is no second closure executor. Ephemeral plans are
boxed for pointer stability and live for exactly one region invocation.

The public-argument slice admits one assignment of the form
`$copy = $callback` only when whole-function liveness proves that `$copy` has
no other read or write and is used immediately as the first argument of the
named wrapper call. The optimized region does not perform the redundant Rc
copy. Every entry failure or native side exit resumes at the original
assignment, so the canonical call protocol and exception/error ordering remain
the source of truth.

Return mode is now published before any frame-free plan is derived. Every
scalar, captured, composed, object, recursion and indirect plan also rejects a
`returns_reference` signature as defense in depth. The existing public Rust
`make_user_function_typed(...)` signature remains source-compatible and
defaults to value return; parsed PHP declarations use an internal explicit
return-mode constructor.

The checkpoint deliberately does not implement general PHP return-by-reference
call semantics. A minimal alias-mutation reproducer produces `13:5` in the
baseline runtime versus PHP's `13:13`; ownership was handed to Compatibility.
Performance tests prove only that reference-returning leaves and wrappers stay
canonical and form no scalar, indirect, captured or native direct-call plan.
No test encodes the known-wrong alias result.

All direct-call planning and execution state is compiled only when both
`quick-loops` and `jit-prototype` are enabled. The return-mode planner guards
remain active in every configuration where a frame-free plan could otherwise
be constructed.

## Benchmark protocol

Every accepted A/B row uses the exact clean commits above, `cargo build
--locked --profile max-perf`, `RUSTFLAGS=-Ctarget-cpu=native`, three warmups per
binary and workload, then 101 measured rounds. Odd rounds execute baseline
first and even rounds candidate first. Each RPHP invocation validates the
payload before its in-workload elapsed value is retained. All 101 samples per
side are retained; the predeclared outlier rule is **remove none**. Tables show
`median [p10, p90]` in seconds.

No timeout or CPU affinity pinning was used. The ARM64 run had an exclusive
process window with Compatibility read-only; process inspection immediately
before timing found no competing `cargo`, `rustc` or `rphp` work. The x86-64
run held one exclusive benchmark lock across builds, tests, instrumentation
and timing. PHP was used for output reference, not as an A/B timing side.

Exact validated payloads were:

| Workload | Payload |
| --- | --- |
| Closure copy and invoke | `34616936` |
| Independent closure-property holdout | `750000` |
| Typed order control | `9895778000,1327440292,11223218292,210000` |
| Ledger control | `500000,7981250000,280500000,182500` |
| Routing control | `290394364,154183816,54660174,384960,192495,64134,108411` |

## Accepted ARM64 evidence

The accepted run started and ended on AC power while charging from 25% to 28%;
macOS low-power mode was disabled. All 1,616 timing records are structurally
valid: ten default-JIT and six typed-only groups of 101 samples.

| Lane and workload | Baseline median [p10, p90] (s) | Candidate median [p10, p90] (s) | Delta |
| --- | ---: | ---: | ---: |
| Default JIT — closure copy | 0.046259880 [0.045346022, 0.047966003] | 0.000317097 [0.000303984, 0.000338078] | -99.315% |
| Default JIT — closure-property holdout | 0.104685068 [0.095005989, 0.168244123] | 0.000917912 [0.000820160, 0.001524925] | -99.123% |
| Default JIT — order control | 0.005302906 [0.004848003, 0.005766153] | 0.005286932 [0.004939079, 0.005911112] | -0.301% |
| Default JIT — ledger control | 0.002954006 [0.002557993, 0.003444910] | 0.002928019 [0.002552032, 0.003583908] | -0.880% |
| Default JIT — routing control | 0.007164001 [0.006842852, 0.007555962] | 0.007097006 [0.006824970, 0.007431984] | -0.935% |
| Typed only — closure copy | 0.047511101 [0.046525002, 0.048400879] | 0.046864986 [0.045779228, 0.048475027] | -1.360% |
| Typed only — closure-property holdout | 0.107233047 [0.100442886, 0.125175953] | 0.106307030 [0.100244045, 0.126528978] | -0.864% |
| Typed only — order control | 0.115767002 [0.090636015, 0.144773006] | 0.116524935 [0.091524839, 0.146830082] | +0.655% |

The default-JIT max-perf binary grows from 4,805,040 to 4,823,328 bytes:
18,288 bytes or 0.381%. The typed-only binary grows from 4,574,032 to
4,574,128 bytes: 96 bytes or 0.002%.

### Rejected battery-powered ARM64 distributions

Three complete exploratory distributions ran while the host discharged from
32% to 15%, with low-power mode disabled. They are retained below because they
exposed power/frequency drift and motivated the AC rerun. They are **not** used
for acceptance: control medians crossed the one-percent ceiling and their
spreads were much wider. No samples were removed. An external temporary-file
cleanup during the AC wait removed their raw TSVs; the logged distribution
summaries are preserved here. `B5x101` values are normalized per invocation
from 101 alternating batches of five validated executions.

| Session and lane/workload | Baseline median [p10, p90] (s) | Candidate median [p10, p90] (s) | Delta |
| --- | ---: | ---: | ---: |
| B101 default — closure copy | 0.139761925 [0.131700039, 0.150630951] | 0.000895977 [0.000710011, 0.001332998] | -99.359% |
| B101 default — closure holdout | 0.227997065 [0.161684990, 0.270759106] | 0.002056122 [0.001379967, 0.003389835] | -99.098% |
| B101 default — order | 0.008851051 [0.007417917, 0.011053801] | 0.008650064 [0.007477999, 0.010675907] | -2.271% |
| B101 default — ledger | 0.003897905 [0.003491879, 0.005259991] | 0.004012823 [0.003507853, 0.005014181] | +2.948% |
| B101 default — routing | 0.010600090 [0.009535789, 0.012285948] | 0.010545015 [0.009644985, 0.011898994] | -0.520% |
| B101 typed — closure copy | 0.048858881 [0.046834946, 0.067121029] | 0.048547983 [0.046420813, 0.064549923] | -0.636% |
| B101 typed — closure holdout | 0.112550020 [0.105667114, 0.128016949] | 0.112241030 [0.104610205, 0.126385927] | -0.275% |
| B101 typed — order | 0.103219032 [0.097067118, 0.119771957] | 0.104715109 [0.098246098, 0.120203972] | +1.449% |
| B201 default — closure copy | 0.128810883 [0.114440918, 0.151376009] | 0.000844002 [0.000551939, 0.001176119] | -99.345% |
| B201 default — closure holdout | 0.254107952 [0.230871916, 0.305091858] | 0.002652884 [0.001512051, 0.003527880] | -98.956% |
| B201 default — order | 0.008214951 [0.006551981, 0.013419151] | 0.008644819 [0.006494999, 0.013917923] | +5.233% |
| B201 default — ledger | 0.003563881 [0.003150940, 0.004508972] | 0.003601789 [0.003166914, 0.004563093] | +1.064% |
| B201 default — routing | 0.013687134 [0.011142969, 0.021339178] | 0.013579845 [0.010951996, 0.020948887] | -0.784% |
| B201 typed — closure copy | 0.057195187 [0.047302961, 0.069884777] | 0.056588888 [0.046801090, 0.070054054] | -1.060% |
| B201 typed — closure holdout | 0.111253977 [0.104096174, 0.144654989] | 0.110642910 [0.102952957, 0.138193846] | -0.549% |
| B201 typed — order | 0.144321203 [0.102318048, 0.257923841] | 0.154214859 [0.104367018, 0.261514902] | +6.855% |
| B5x101 typed — closure copy | 0.129146624 [0.108252001, 0.148187780] | 0.128229570 [0.100046349, 0.147890806] | -0.710% |
| B5x101 typed — closure holdout | 0.241125822 [0.225685596, 0.268647194] | 0.245038176 [0.228746796, 0.278437996] | +1.623% |
| B5x101 typed — order | 0.130079842 [0.117616463, 0.167863178] | 0.133146238 [0.119466829, 0.163361311] | +2.357% |

## Accepted x86-64 evidence

Exact baseline and candidate Git archives were SHA-256 validated on the
physical x86-64 host. The archive hashes were respectively
`79f753eb0834b625a5c175aa6bcae49c77384f878de3a936bbec0492c91f3927`
and `d87e0c67fe88c360a5ed618dbe87930a3a043e8a4759c6a36396fa88d8910900`.
All 1,616 timing records are structurally valid and all samples are retained.

| Lane and workload | Baseline median [p10, p90] (s) | Candidate median [p10, p90] (s) | Delta |
| --- | ---: | ---: | ---: |
| Default JIT — closure copy | 0.057909727 [0.057054043, 0.060759544] | 0.000390768 [0.000381947, 0.000405788] | -99.325% |
| Default JIT — closure-property holdout | 0.109150887 [0.108261585, 0.112888336] | 0.000428438 [0.000410795, 0.000447512] | -99.607% |
| Default JIT — order control | 0.004162312 [0.004087925, 0.004238605] | 0.004154444 [0.004100084, 0.004241705] | -0.189% |
| Default JIT — ledger control | 0.002058506 [0.002002716, 0.002194643] | 0.002054691 [0.001999140, 0.002132416] | -0.185% |
| Default JIT — routing control | 0.007421732 [0.007272959, 0.007642508] | 0.007435799 [0.007275581, 0.007582664] | +0.190% |
| Typed only — closure copy | 0.056109905 [0.055531025, 0.057644129] | 0.055922747 [0.055178642, 0.057688475] | -0.334% |
| Typed only — closure-property holdout | 0.108449697 [0.105773926, 0.114284277] | 0.106345415 [0.105468988, 0.108884335] | -1.940% |
| Typed only — order control | 0.091857672 [0.090105295, 0.094349623] | 0.091923237 [0.089901209, 0.094057560] | +0.071% |

The default-JIT binary grows from 5,633,856 to 5,669,392 bytes: 35,536
bytes or 0.631%. The typed-only binary shrinks from 5,361,104 to 5,356,896
bytes: 4,208 bytes or 0.078%.

## Structural evidence

Fresh `vm-stats` builds on ARM64 and the retained x86-64 stats reproduce the
same target and holdout counts. One admitted region uses one bounded 16 KiB
native mapping and reports zero side exits.

| Closure-copy counter | Baseline | Candidate |
| --- | ---: | ---: |
| Frame pushes | 500,004 | 70 |
| Frame cleanups | 500,004 | 70 |
| Cleanup slots scanned | 1,500,013 | 211 |
| Frame-slot writes | 1,250,008 | 173 |
| Heap-value frame-slot writes | 500,002 | 68 |
| Quick-region entries/completions | 0 / 0 | 1 / 1 |
| Quick iterations | 0 | 249,967 |
| Native mappings / side exits | 0 / 0 | 1 / 0 |

The independent property holdout falls from 1,500,005 frame pushes, 750,013
cleanups, 3,750,024 scanned slots, 3,000,007 frame-slot writes and 1,500,001
heap-value frame writes to 71, 46, 189, 139 and 67. It completes 749,967 quick
iterations in one native execution with zero side exits.

## Reproducibility metadata

### ARM64 host

- Apple M4, AArch64, 10 physical/logical cores, 25,769,803,776 bytes memory;
  macOS 26.5.2 build 25F84.
- Accepted power mode: AC, charging 25% to 28%, low-power mode disabled.
  Affinity was not pinned; the process window was exclusive.
- Rust/Cargo 1.93.1 Homebrew, rustc commit
  `01f6ddf7588f42ae2d7eb0a2f21d44e8e96674cf`, LLVM 21.1.8.
- PHP 8.5.9 CLI, NTS, built 2026-07-28 13:06:52; configuration
  `/opt/homebrew/etc/php/8.5/php.ini`, no additional ini files. Zend OPcache is
  loaded, `opcache.enable_cli=0`, `opcache.jit=disable`, JIT buffer 64M.
- Loaded PHP extensions: Core, date, lexbor, openssl, pcre, sqlite3, zlib,
  bcmath, bz2, calendar, ctype, curl, dba, uri, json, mbstring, SPL, FFI,
  fileinfo, filter, ftp, gd, gettext, gmp, hash, iconv, intl, session, ldap,
  standard, libxml, mysqlnd, mysqli, odbc, Zend OPcache, pcntl, PDO,
  pdo_dblib, pdo_mysql, PDO_ODBC, pdo_pgsql, pdo_sqlite, pgsql, Phar, posix,
  random, readline, Reflection, exif, shmop, SimpleXML, snmp, soap, sockets,
  sodium, sysvmsg, sysvsem, sysvshm, tidy, tokenizer, dom, xml, xmlreader,
  xmlwriter, xsl and zip.

### x86-64 host

- AMD Ryzen 9 7950X 16-Core Processor, x86-64, 16 cores/32 threads,
  32,753,020,928 bytes memory; Ubuntu 24.04.4 LTS, kernel 7.0.0-28-generic.
- CPU scaling governor `performance`; affinity not pinned, allowed CPU list
  0-31. One exclusive lock prevented competing benchmark/build work.
- Rust/Cargo 1.93.1, rustc commit
  `01f6ddf7588f42ae2d7eb0a2f21d44e8e96674cf`, LLVM 21.1.8.
- PHP 8.4.24 CLI, NTS, built 2026-07-30 15:23:13 by Ubuntu;
  `/etc/php/8.4/cli/php.ini` plus the distribution CLI extension ini files.
  Zend OPcache 8.4.24 is loaded, `opcache.enable_cli=0`, JIT unset, JIT buffer
  64M.
- Loaded PHP extensions: Core, date, libxml, openssl, pcre, zlib, filter, hash,
  json, pcntl, random, Reflection, SPL, session, standard, sodium, PDO,
  calendar, ctype, exif, FFI, fileinfo, ftp, gettext, iconv, Phar, posix,
  readline, shmop, sockets, sysvmsg, sysvsem, sysvshm, tokenizer and Zend
  OPcache.

### RPHP build modes

- Default JIT: default features `quick-loops,jit-prototype`.
- Typed only: `--no-default-features --features quick-loops`.
- Structural counts: default features plus `vm-stats`, with
  `RPHP_VM_STATS=1` only for the counted invocation.
- All release variants: `max-perf` profile, fat LTO, one codegen unit,
  `-Ctarget-cpu=native`. PHP reference runs used CLI OPcache/JIT settings above.

## Correctness evidence

The focused suites cover empty and captured closure properties, immutable
Long and String captures, reference captures, public-argument wrappers, dead
and live aliases, changed callable identity, stale cache reuse and exact
overflow replay. Two runtime `Closure` instances created from the same compiled
closure declaration with different Long/String captures produce exactly
`300000:1100000`: the first configuration enters native code once and the
changed capture configuration uses the canonical lane, with no stale constant
reuse or side exit.

The return-by-reference regression proves that a wrapper, a captured closure
leaf and a no-capture scalar-shaped named leaf all retain
`returns_reference=true` and receive no scalar/captured/indirect plan. The
outer typed loop receives no `QuickLongOps` native region. A separate by-value
read through the reference-returning closure matches PHP output `5` without
asserting the known alias bug.

The final ARM64 source matrix passed after the rebase and after all code
changes:

```text
cargo test --locked --quiet
cargo test --locked --no-default-features --quiet
cargo test --locked --features php-generics-erased --quiet
cargo test --locked --features php-generics-reified --quiet
cargo test --locked --all-features --quiet
cargo check --locked --all-features --all-targets
scripts/check-unsafe-policy.sh --diff-base main
```

Focused ARM64 evidence also passed `e2e_direct_call_regions` 4/4,
`jit_direct_call_regions` 8/8, the no-default direct-call E2E 4/4,
`e2e_corpus` 4/4 and `jit_aarch64_prototype` 103/103. The locked x86-64 run
passed direct-call E2E 4/4, direct-call JIT 8/8, corpus 4/4 and focused native
routing/order/ledger tests 1/1 each.

The repository-wide unsafe-policy inventory passes at exactly 1,623 production
blocks and 289 unsafe functions, matching both ceilings. This checkpoint adds
no new `unsafe` block or function. Mandatory cleanup ran before and after the
full matrix and release benchmark lifecycle.

## Ownership and handoff

The accepted implementation touches shared compiler/runtime files, including
`src/compiler/mod.rs`, `src/vm/function.rs`, `src/vm/planner.rs`,
`src/vm/quick.rs`, `src/vm/quick_long_region_plan.rs` and the quick/native
execution adapters. Compatibility owns the separate return-by-reference alias
semantic gap. The integrating agent completed the joint gate and made these
files part of the shared `main` baseline; later work must rebase before taking
new ownership.

Public-data hygiene is clean: no private hostname, address, username,
credential, private filesystem path or raw remote log is stored in tracked
content.
