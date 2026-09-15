# Crypt callable implementation boundary

This is an implementation and security review record, not a claim that RPHP
is hardened or suitable for password storage. Legacy crypt algorithms remain
insecure choices for new applications.

## Dependency decision

The callable uses independent packages, not transplanted PHP or library code:

- `pwhash` 1.0.0 (MIT), only its DES and extended-DES byte-input APIs.
- `md5crypt` 1.0.0 (MIT), accepting raw salt bytes.
- RustCrypto `sha-crypt` 0.6.0 (MIT OR Apache-2.0), with default features off,
  for raw SHA-256/SHA-512 digest computation.
- System `libxcrypt` >= 4.4 (LGPL-2.1-or-later), dynamically linked, only the
  reentrant bcrypt API and its read-only capability query. `pkg-config`
  discovers the target library; this does not vendor it or its headers.
- `libloading` 0.9.0 (ISC) retains that library from first use until process
  exit. Only a build-selected absolute library path is admitted; no PHP value,
  current directory or runtime environment chooses the module. No bcrypt
  library is mapped for requests using only the pure-Rust hash families.

The salt parser, output projection and sparse native SensitiveParameter
metadata are original RPHP code derived from PHP 8.5 black-box observations.
SHA digest encoding follows step 22 of the public
[SHA-crypt specification](https://www.akkadia.org/drepper/SHA-crypt.txt).
Its rounds-clamping description does not override the observed PHP error
contract. No RNG or password-verification API is introduced.

A single libxcrypt backend was rejected: it refuses raw MD5/SHA salt bytes
accepted by PHP and extended-DES passphrases of 512 bytes or more. A single
pwhash/crypt3_rs backend was rejected because its bcrypt API lacks the legacy
`2x` signed-byte behavior. `libcrypt-rs` was rejected because its safe wrapper
uses a shared global crypt result without synchronization or a null-result
check. The mixed backend is necessary to retain these public boundaries,
not an attempt to hide a native call inside a third-party safe wrapper.

## Intentional unsafe addition: three local blocks

The proposed ratchet change is **1623 -> 1626 blocks**, with **289 unsafe
functions unchanged**. This addition must be reviewed with this record and
the complete `src/stdlib/crypt.rs` diff; it is not generic ratchet maintenance.
No unrelated unsafe code is removed to manufacture budget.

1. `native_hash` calls public `crypt_ra` with live CStrings and an exclusively
   owned `(null, 0)` allocation request. No guessed C struct layout is used.
   Non-null output is documented to be a terminated string in that allocation.
   It is copied into Rust storage before the owner drops. No callback, shared
   buffer, pointer retention, aliasing or C-to-Rust unwind is permitted.
2. `NativeState::drop` passes the library's malloc/realloc allocation to C
   `free`, exactly once, including null/failure and Rust allocation-unwind
   paths. The owner is not Clone/Copy and does not expose its pointer.
3. `native_api` loads the build-validated libxcrypt path and resolves the exact
   public C function signatures. Initializers belong to that trusted system
   library, not user-selected code. A `OnceLock` retains the owning Library
   alongside its function pointer for all threads and in-flight calls; there
   is no dangling symbol or unload race. A static terminated capability
   setting reports actual availability. Load/symbol failure publishes no API
   and gives the ordinary unsupported-algorithm sentinel/zero flag.

All three operations expose safe local Rust interfaces. NUL truncation is PHP
semantics, and bcrypt's 72-byte truncation occurs before the native call. The
native password allocation limit cannot therefore truncate a PHP input.
Output and trace tests cover invalid salts, raw high bytes, failures followed
by success, nested conversion, parallel calls and sensitive argument retention.
These tests do not substitute for a cryptographic audit or allocator proof.
The loader extends the original capability-query unsafe boundary; it is not
an unreviewed safe-wrapper exemption. A Linux subprocess regression verifies
that pure-Rust calls leave libxcrypt unmapped and first bcrypt use loads it.

## Distribution and unsupported claims

libxcrypt is an explicit system build/runtime prerequisite on both supported
Unix targets. It must remain independently replaceable when binaries are
distributed; distributors must also satisfy its LGPL license/source/notice
obligations. Static redistribution is not configured or validated here.
The project does not claim FIPS equivalence, blanket platform parity, OOM
equivalence, secure erasure, constant-time PHP conversion, or a loaded PHP
extension. macOS builds require the target libxcrypt package, not macOS's
legacy libc `crypt` symbol. Native validation results must name the platform
actually run; availability of a package is not execution evidence.
The build-selected shared-library link must remain installed at runtime;
relocating a binary to a different native installation requires a rebuild.
Cross-toolchain/sysroot relocation is not validated by this checkpoint.
