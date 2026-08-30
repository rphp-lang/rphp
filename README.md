# RPHP

[![CI](https://github.com/rphp-lang/rphp/actions/workflows/ci.yml/badge.svg)](https://github.com/rphp-lang/rphp/actions/workflows/ci.yml)

RPHP is an independent, experimental PHP-compatible runtime written in Rust.
It has its own lexer, parser, bytecode compiler, virtual machine, and optional
native execution tiers. RPHP does not embed PHP, preserve the Zend C ABI, or
copy the Zend Engine architecture.

> [!WARNING]
> RPHP is **pre-alpha**. It implements a tested subset of PHP, contains
> substantial unsafe VM/JIT code, and has not been security-hardened. Do not
> run untrusted PHP code or use RPHP in production.

## Status

The tested subset currently covers scalar expressions and control flow,
functions and closures, arrays and copy-on-write behavior, classes,
inheritance, interfaces, traits, exceptions, generators, type declarations,
namespaces, selected reflection behavior, JSON, regular expressions, and a
growing standard-library surface. Some streams, file operations, structured
coroutines and experimental generic types are opt-in Cargo features. The
default build includes native execution for proven hot regions on the two
primary targets below.

RPHP is not yet a drop-in replacement for PHP. Major gaps include complete
language and standard-library compatibility, most extensions, broad Composer
and framework support beyond the pinned compatibility gates, web SAPIs,
databases, Redis, cURL, production-grade cycle
collection, and broad JIT coverage. See the [compatibility status](docs/compatibility.md)
for the current support contract.

CI tests the baseline runtime and native execution on these primary targets:

| Target | Baseline runtime | Default native JIT |
| --- | --- | --- |
| macOS / AArch64 | Yes | Yes |
| Linux / x86-64 | Yes | Yes |

Other Rust targets may compile, but are not supported by the pre-alpha CI
contract. The project currently makes no minimum operating-system version
guarantee.

## Build and run

Install [Rustup](https://rustup.rs/) and clone the repository. Rustup will use
the toolchain pinned in `rust-toolchain.toml`.

```sh
cargo build --profile max-perf
./target/max-perf/rphp -r 'echo "Hello from RPHP\n";'
./target/max-perf/rphp example.php
```

A source file or standard input must contain a PHP opening tag. Code passed to
`-r` does not require one.

```sh
printf '<?php echo 6 * 7;' | ./target/max-perf/rphp
./target/max-perf/rphp --help
./target/max-perf/rphp --version
```

## Builtin compatibility audit

After a debug build, the on-demand audit compares every builtin exposed by the
selected reference PHP with RPHP, including argument names, arity,
required/optional and variadic parameters, by-reference mode, defaults and
Reflection type metadata:

```sh
cargo build
python3 scripts/audit-php-builtins.py
```

Add one or more `--vendor /path/to/vendor` arguments to prioritize builtin
function calls found statically in Composer packages. Results are written to
`target/builtin-audit/`: `report.md` summarizes the gaps, while `functions.csv`,
`types.csv`, `methods.csv` and `vendor-functions.csv` contain the complete
machine-readable mappings. `--reference-php`, `--rphp`, `--output-dir` and
`--fail-on` make the same audit reusable with another PHP build or in CI. The
inventory proves name and Reflection-visible contract coverage, not complete
runtime semantics.

## Experimental build modes

Features are selected at compile time. The resulting binary automatically uses
an enabled optimization when the current target and program region qualify.

```sh
# Structured, cooperative coroutines and their PHP API
cargo build --profile max-perf --features coroutines

# Bound-erased experimental generic types
cargo build --profile max-perf --features php-generics-erased

# Reified experimental generic types
cargo build --profile max-perf --features php-generics-reified
```

The default binary automatically lowers qualifying hot regions to native code
on macOS/AArch64 and Linux/x86-64. Set `RPHP_DISABLE_JIT=1` to retain the same
binary but force the typed executor, or build an explicit typed-only binary
with `--no-default-features --features quick-loops`. Live executable mappings
are capped at 16 MiB by default; `RPHP_JIT_CODE_LIMIT_BYTES` may lower that
budget or raise it up to the hard 1 GiB ceiling. The historical
`jit-prototype` Cargo feature remains available for explicit no-default build
matrices.

The generic syntax is an RPHP experiment and is disabled in a default build.
An all-features build contains both generic runtime capabilities for
differential testing. The coroutine API and its current limitations are
described in the [runtime roadmap](docs/roadmap-runtime-architecture.md).

## Design and roadmaps

- [Architecture](docs/architecture.md)
- [Active engineering roadmap](docs/roadmap.md)
- [Compatibility status](docs/compatibility.md)
- [Compatibility roadmap](docs/roadmap-compatibility.md)
- [Execution and performance roadmap](docs/roadmap-execution-performance.md)
- [Specialized agent goal contract](docs/agent-goal-contract.md)
- [Runtime architecture engineering log](docs/roadmap-runtime-architecture.md)
- [Combined performance/JIT/compatibility engineering
  log](docs/roadmap-performance-jit-compatibility.md)
- [Benchmark methodology](docs/benchmarking.md)
- [Unsafe-code policy](docs/unsafe-policy.md)
- [Code provenance policy](docs/provenance.md)

## Benchmarks

Repository benchmarks measure specific program shapes inside RPHP's currently
supported region. They do **not** represent the performance of all PHP
applications. Every publishable result must identify the exact commit,
hardware and operating system, PHP version and JIT configuration, RPHP feature
flags, warm-up, repetitions, aggregation method, and benchmark source. See
[the benchmark methodology](docs/benchmarking.md).

## Contributing and security

Read [CONTRIBUTING.md](CONTRIBUTING.md) before sending a change. In particular,
all contributions must be original or carry a compatible, disclosed license;
do not copy source from PHP/Zend, TA-Lib, or another implementation.

Security issues must not be filed as public bug reports. Follow
[SECURITY.md](SECURITY.md). RPHP's dependencies and their declared licenses are
listed in [THIRD_PARTY.md](THIRD_PARTY.md).

## License

RPHP is dual-licensed under the [MIT License](LICENSE-MIT) or the
[Apache License, Version 2.0](LICENSE-APACHE), at your option.
