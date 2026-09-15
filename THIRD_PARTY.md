# Third-party dependencies

RPHP's Rust dependencies are locked in `Cargo.lock`. This inventory records
all 69 resolved packages and their declared licenses as of 2026-09-15,
including build and target-specific dependencies. Full license texts are in
the corresponding Cargo registry/source distributions; no source is vendored.

| Package | Version | Declared license |
| --- | ---: | --- |
| `ar_archive_writer` | 0.5.3 | Apache-2.0 WITH LLVM-exception |
| `base64ct` | 1.8.3 | Apache-2.0 OR MIT |
| `bitflags` | 2.13.2 | MIT OR Apache-2.0 |
| `block-buffer` | 0.9.0 | MIT OR Apache-2.0 |
| `block-buffer` | 0.12.1 | MIT OR Apache-2.0 |
| `blowfish` | 0.7.0 | MIT OR Apache-2.0 |
| `byteorder` | 1.5.0 | Unlicense OR MIT |
| `cc` | 1.4.4 | MIT OR Apache-2.0 |
| `cfg-if` | 1.0.4 | MIT OR Apache-2.0 |
| `cipher` | 0.2.5 | MIT OR Apache-2.0 |
| `cpufeatures` | 0.2.17 | MIT OR Apache-2.0 |
| `cpufeatures` | 0.3.1 | MIT OR Apache-2.0 |
| `crypto-common` | 0.2.2 | MIT OR Apache-2.0 |
| `crypto-mac` | 0.10.1 | MIT OR Apache-2.0 |
| `digest` | 0.9.0 | MIT OR Apache-2.0 |
| `digest` | 0.11.3 | MIT OR Apache-2.0 |
| `errno` | 0.3.14 | MIT OR Apache-2.0 |
| `find-msvc-tools` | 0.1.11 | MIT OR Apache-2.0 |
| `fs2` | 0.4.3 | MIT/Apache-2.0 |
| `generic-array` | 0.14.9 | MIT |
| `getrandom` | 0.2.17 | MIT OR Apache-2.0 |
| `hmac` | 0.10.1 | MIT OR Apache-2.0 |
| `hybrid-array` | 0.4.15 | MIT OR Apache-2.0 |
| `itoa` | 1.0.17 | MIT OR Apache-2.0 |
| `libc` | 0.2.189 | MIT OR Apache-2.0 |
| `libloading` | 0.9.0 | ISC |
| `linux-raw-sys` | 0.12.1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| `md-5` | 0.9.1 | MIT OR Apache-2.0 |
| `md5` | 0.8.1 | Apache-2.0 OR MIT |
| `md5crypt` | 1.0.0 | MIT |
| `memchr` | 2.8.0 | Unlicense OR MIT |
| `object` | 0.39.1 | Apache-2.0 OR MIT |
| `opaque-debug` | 0.3.1 | MIT OR Apache-2.0 |
| `pkg-config` | 0.3.34 | MIT OR Apache-2.0 |
| `ppv-lite86` | 0.2.21 | MIT OR Apache-2.0 |
| `proc-macro2` | 1.0.106 | MIT OR Apache-2.0 |
| `psm` | 0.1.32 | MIT OR Apache-2.0 |
| `pwhash` | 1.0.0 | MIT |
| `quote` | 1.0.45 | MIT OR Apache-2.0 |
| `rand` | 0.8.8 | MIT OR Apache-2.0 |
| `rand_chacha` | 0.3.1 | MIT OR Apache-2.0 |
| `rand_core` | 0.6.4 | MIT OR Apache-2.0 |
| `rustix` | 1.1.4 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| `serde` | 1.0.228 | MIT OR Apache-2.0 |
| `serde_core` | 1.0.228 | MIT OR Apache-2.0 |
| `serde_derive` | 1.0.228 | MIT OR Apache-2.0 |
| `serde_json` | 1.0.149 | MIT OR Apache-2.0 |
| `serde_stacker` | 0.1.14 | MIT OR Apache-2.0 |
| `sha-1` | 0.9.8 | MIT OR Apache-2.0 |
| `sha-crypt` | 0.6.0 | MIT OR Apache-2.0 |
| `sha2` | 0.9.9 | MIT OR Apache-2.0 |
| `sha2` | 0.11.0 | MIT OR Apache-2.0 |
| `shlex` | 2.0.1 | MIT OR Apache-2.0 |
| `stacker` | 0.1.25 | MIT OR Apache-2.0 |
| `subtle` | 2.4.1 | BSD-3-Clause |
| `syn` | 2.0.117 | MIT OR Apache-2.0 |
| `typenum` | 1.20.1 | MIT OR Apache-2.0 |
| `unicode-ident` | 1.0.24 | (MIT OR Apache-2.0) AND Unicode-3.0 |
| `version_check` | 0.9.5 | MIT/Apache-2.0 |
| `wasi` | 0.11.1+wasi-snapshot-preview1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| `winapi` | 0.3.9 | MIT/Apache-2.0 |
| `winapi-i686-pc-windows-gnu` | 0.4.0 | MIT/Apache-2.0 |
| `winapi-x86_64-pc-windows-gnu` | 0.4.0 | MIT/Apache-2.0 |
| `windows-link` | 0.2.1 | MIT OR Apache-2.0 |
| `windows-sys` | 0.61.2 | MIT OR Apache-2.0 |
| `xxhash-rust` | 0.8.18 | BSL-1.0 |
| `zerocopy` | 0.8.57 | BSD-2-Clause OR Apache-2.0 OR MIT |
| `zerocopy-derive` | 0.8.57 | BSD-2-Clause OR Apache-2.0 OR MIT |
| `zmij` | 1.0.21 | MIT |

## Native crypt dependency

`libxcrypt` >= 4.4 is a separately installed, dynamically linked dependency
under LGPL-2.1-or-later. The checkpoint's Linux oracle used 4.4.36. Library
source and notices are distributed by the system package, not copied into
RPHP. Binary distributors must keep it replaceable and comply with its
license, corresponding-source and notice obligations. See
[the crypt boundary review](docs/crypt-boundary.md) for the API and rejected
alternatives. Static distribution is not configured or validated.

This inventory is informational, not legal advice. Regenerate and review it
when `Cargo.lock` changes. Preserve each package's license and applicable
NOTICE files when redistributing it; `unicode-ident` also carries Unicode
license terms, and some native/tooling dependencies contain additional
file-specific notices.
