# Compatibility status

RPHP implements a growing, tested subset of PHP. It currently identifies PHP
compatibility around PHP 8.4 language behavior, but it is not certified for a
complete PHP version and must not be treated as a drop-in PHP replacement.
Passing a script is evidence only for the exercised behavior.

## Official php-src PHPT baseline

The latest reproducible upstream baseline runs the unmodified `Zend/tests` and
`tests/lang` suites from PHP 8.4.21 commit
`7a64ae0507799547fbbd39b067bd3dd2c35e8fec` against all-features RPHP commit
`f6a20c1049501343352d4bff35fd2291271033d5`. The recorded run used arm64 and a
three-second per-process timeout. It discovered 5,259 PHPT cases.

| Suite | Pass | Fail | Skip | Unsupported | Timeout | Crash | Headline pass rate |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `Zend/tests` | 351 | 4,264 | 85 | 258 | 0 | 7 | 7.606% |
| `tests/lang` | 36 | 232 | 10 | 16 | 0 | 0 | 13.433% |
| **Combined** | **387** | **4,496** | **95** | **274** | **0** | **7** | **7.925%** |

The headline follows the published gate definition exactly:
`pass / (pass + fail)`. It does not count skips, unsupported cases, timeouts or
crashes as passes. A stricter whole-corpus view is 387 / 5,259, or **7.359%**;
including crashes in the attempted denominator gives **7.914%**. These numbers
are intentionally pre-alpha and are not a claim of PHP 8.4 compatibility.

The largest failure groups are 2,580 parse failures, 1,156 runtime failures,
665 output mismatches, 83 compile failures, 11 failed `SKIPIF` evaluations and
one expected-failure mismatch. Seven cases terminate by signal. Of the 95
skips, 65 require unavailable extensions and 30 are selected by `SKIPIF`.
Unsupported cases remain in the total: 269 require per-process `INI` behavior
that the RPHP CLI does not expose, while five require PHPDBG or CGI/header
sections outside this CLI gate.

Relative to the retained `1a5a270` baseline, this run adds 37 passing cases
without losing a previous pass, reduces parser failures by 241 and reduces
signal-terminated cases from 14 to 7. The measured change covers standard
comma-separated `echo` syntax plus generator-safe call argument suspension;
it does not infer support for the remaining downstream behavior of every case
that now parses.

The dependency-free project runner supports `FILE`, `FILEEOF`,
`FILE_EXTERNAL`, `EXPECT`, `EXPECTF`, `EXPECTREGEX`, `SKIPIF`, `INI`, `ENV`,
`ARGS`, `STDIN`, `CLEAN` and extension declarations, with explicit capability
classification where the runtime cannot execute a section. Its expectation and
section handling was compared on the same pinned checkout with the official
`run-tests.php` under the official PHP 8.4.21 CLI image; both runners passed the
same five representative cases, 5/5.

The complete machine-readable result is committed as
[`f6a20c1-arm64-manifest.jsonl`](../tests/php-src/results/php-8.4.21/f6a20c1-arm64-manifest.jsonl),
with aggregate metadata in
[`f6a20c1-arm64-summary.json`](../tests/php-src/results/php-8.4.21/f6a20c1-arm64-summary.json).
Every upstream path remains visible with its pass/fail/skip/unsupported/
timeout/crash status and classification.

To reproduce the run from an external checkout pinned to the commit above:

```sh
cargo build --locked --release --all-features
RPHP_PHPT_TIMEOUT=3 scripts/run-php-src-phpt.sh \
  /path/to/php-src target/release/rphp /tmp/rphp-phpt-results 4
```

## Default build

End-to-end tests cover representative behavior in these areas:

- scalar values, arithmetic, comparisons, strings, arrays, references, and
  copy-on-write paths;
- conditionals, loops, `foreach`, `switch`, `match`, null coalescing, and
  nullsafe access;
- functions, closures, arrow functions, variadics, named arguments,
  generators, and selected callable forms;
- classes, constructors, inheritance, interfaces, traits, visibility, clone,
  magic methods, constants, namespaces, and `instanceof`;
- scalar/object type declarations, strict types, exceptions, `try`/`catch`/
  `finally`, and selected reflection behavior;
- selected standard-library functions, JSON, regular expressions, and core
  stream behavior.

This list describes tested areas, not complete coverage of each PHP feature.
Exact supported edge cases are defined by the source and tests on the commit
being run.

## Opt-in surfaces

| Cargo feature | Status |
| --- | --- |
| `jit-prototype` | Native execution for selected guarded regions on macOS/AArch64 and Linux/x86-64 |
| `coroutines` | Experimental structured cooperative tasks, channels, timers, and readiness APIs |
| `php-generics-erased` | Experimental generic syntax with bound-erased runtime behavior |
| `php-generics-reified` | Experimental generic syntax with reified runtime sidecars and checks |
| `file-contents`, `file-write`, `file-lines`, `include-path` | Incremental file/include functionality |
| `stream-*`, `csv-*`, `resource-lifetime`, `value-errors` | Incremental stream, resource, and error behavior |
| `vm-stats` | Internal execution and JIT diagnostics |

The generic syntax is an RPHP experiment, not part of the default PHP
compatibility claim. Feature names and APIs can change without deprecation
during pre-alpha development.

## Known major gaps

- complete PHP syntax, edge-case semantics, standard library, and extension
  coverage;
- Composer packages and framework applications, including Symfony;
- HTTP/FPM or another production web SAPI, WebSockets, and server lifecycle;
- databases, Redis, cURL, and most external integrations;
- production cycle collection, resource limits, sandboxing, and security
  hardening;
- BigInt/BigDecimal and broad specialist extension compatibility;
- complete JIT coverage and supported native backends beyond the two primary
  CI targets;
- stable embedding, extension, bytecode, CLI, or Rust APIs.

Missing behavior may fail at lexing, parsing, compilation, or execution. Error
messages and exit codes are not yet fully PHP-compatible.

## Reporting compatibility bugs

Include the exact RPHP commit, build command and feature flags, target and OS,
a minimal PHP program, actual output, expected output, and the reference PHP
version/configuration. Do not report security-sensitive cases publicly; use
the process in `SECURITY.md`.
