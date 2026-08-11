# Contributing to RPHP

RPHP is an experimental runtime with a correctness-first baseline executor and
performance-sensitive optional tiers. Small, reviewable changes with focused
tests are preferred.

## Before opening a change

- Discuss large language, VM, ABI, dependency, or architecture changes in an
  issue first.
- Keep the baseline bytecode semantics authoritative. An optimization must
  guard before observable mutation and resume at the exact baseline position.
- Add differential or end-to-end coverage for externally visible PHP behavior.
- Do not include build artifacts, benchmark candidates, local settings,
  prompts, private reviews, credentials, hostnames, or internal infrastructure.
- Follow the [unsafe-code policy](docs/unsafe-policy.md) for every new or
  changed unsafe block.

## Code provenance

Contributions must be original work, or derived from a source whose compatible
license and attribution are disclosed in the pull request and repository.
Public specifications and manuals may be used to understand behavior. Do not
copy or mechanically translate source code, tests, tables, comments, or
architecture from PHP/Zend, TA-Lib, or any other runtime or library. Do not
submit generated output that embeds such material. See the full
[provenance policy](docs/provenance.md).

By submitting a contribution, you represent that you have the right to submit
it under the project's MIT OR Apache-2.0 dual license.

## Format and test

The pinned toolchain is installed automatically by Rustup. Before requesting a
review, run:

```sh
cargo fmt --all -- --check
cargo test --locked
cargo test --locked --no-default-features
cargo test --locked --features php-generics-erased
cargo test --locked --features php-generics-reified
cargo test --locked --all-features
cargo check --locked --all-features --all-targets
```

JIT changes must also pass `cargo test --locked --features jit-prototype` on
macOS/AArch64 and Linux/x86-64. Feature-specific changes should add a focused
test invocation to the pull-request description.

Run `scripts/cleanup-builds.sh` before and after a complete local feature
matrix. It removes only regeneratable build output when the configured size or
free-space threshold is reached.

## Performance changes

Do not optimize only for a single microbenchmark. Preserve output and
correctness first, compare exact baseline and candidate commits, randomize
interleaved runs where practical, and report medians plus distribution data.
Never discard an unexpected regression as noise without a larger independent
rerun. Follow [docs/benchmarking.md](docs/benchmarking.md) for publishable
results.

## Pull requests

Explain the behavior changed, risks and invariants, tests run, feature flags,
supported targets, and any performance evidence. Keep refactors separate from
semantic or performance changes when that makes the result easier to audit.
