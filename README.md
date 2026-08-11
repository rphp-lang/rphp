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
coroutines, experimental generic types, and the native JIT are opt-in Cargo
features.

RPHP is not yet a drop-in replacement for PHP. Major gaps include complete
language and standard-library compatibility, most extensions, Composer and
framework support, web SAPIs, databases, Redis, cURL, production-grade cycle
collection, and broad JIT coverage. See the [compatibility status](docs/compatibility.md)
for the current support contract.

CI tests the baseline runtime and native execution on these primary targets:

| Target | Baseline runtime | Experimental JIT |
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

## Experimental build modes

Features are selected at compile time. The resulting binary automatically uses
an enabled optimization when the current target and program region qualify.

```sh
# Native JIT for supported hot regions (macOS/AArch64 or Linux/x86-64)
cargo build --profile max-perf --features jit-prototype

# Structured, cooperative coroutines and their PHP API
cargo build --profile max-perf --features coroutines

# Bound-erased experimental generic types
cargo build --profile max-perf --features php-generics-erased

# Reified experimental generic types
cargo build --profile max-perf --features php-generics-reified
```

The generic syntax is an RPHP experiment and is disabled in a default build.
An all-features build contains both generic runtime capabilities for
differential testing. The coroutine API and its current limitations are
described in the [runtime roadmap](docs/roadmap-runtime-architecture.md).

## Design and roadmaps

- [Architecture](docs/architecture.md)
- [Compatibility status](docs/compatibility.md)
- [Runtime and coroutine roadmap](docs/roadmap-runtime-architecture.md)
- [Performance, JIT, and compatibility roadmap](docs/roadmap-performance-jit-compatibility.md)
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
