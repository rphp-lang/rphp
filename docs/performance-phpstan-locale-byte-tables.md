# Locale-aware byte classification tables

Ordinary byte regex matching repeatedly constructed the generic native-call
argument, entered its cold function and called libc for classification or
case folding. Fixed byte tables now provide those results through safe Rust
indexed reads. Independent PGO confirmation reduces the same PHPStan analysis
**72.1691 to 71.4742 billion instructions (-0.963%)**, with identical PHP output.
The original instruction and one-second parity targets remain open.

Each thread retains a fixed 776-byte projection of all twelve classifications
and lowercase conversion for all 256 byte values. The existing native boundary
fills it without callbacks and publishes its generation afterward. Successful
`LC_CTYPE` or `LC_ALL` mutation advances a process-global generation; tables
refresh lazily. Locale queries, failed changes, other categories and environment
variable changes preserve the table. This retains the existing module's single
VM thread contract for native state mutation. Unicode matching and non-Linux
ASCII behavior remain unchanged. No ordinary heap allocation, unsafe invariant,
VM layout, opcode or native lowering is added.

| Measurement | Baseline instructions | Candidate instructions | Change | Baseline analysis | Candidate analysis |
| --- | ---: | ---: | ---: | ---: | ---: |
| First two PGO pairs | 72.1745 G | 71.4530 G | -1.000% | 6.6532 s | 6.5974 s |
| Independent two PGO pairs | 72.1691 G | 71.4742 G | -0.963% | 6.8070 s | 6.6122 s |

The confirmation PHP reference executes 10.3377 billion instructions in
0.9502 seconds: RPHP still has a **6.91x instruction gap**. Whole-command
measurement separately records 90.6493 to 89.8096 billion instructions and
622,332 to 622,424 KiB RSS. Startup-inclusive and analysis-only counts are not
combined. Executable text shrinks 649 bytes and the full binary 10,376 bytes;
BSS grows 1,480 bytes. Native evidence is x86-64 only, with no architecture
specific source change.

The preceding exact profile attributed 12.268 million native entries to byte
classification/folding; its 504.099-million inclusive budget excludes caller
setup and is not a claim of current removable instructions. Current instruction
samples identified the same boundary before implementation. The predefined
exploratory gate uses identical default `test-fast` builds: 111.9590 to 111.0815
billion instructions (-0.784%). Only after this passes is PGO rebuilt using the
same eighteen independent training inputs. PHPStan and all six controls are
excluded. Compiler profile warnings refer only to the build script.

Three independently ordered pairs on each of six PHP-validated controls pass.
The untrained [caseless nested regex](../benches/bench_regex_locale_caseless.php)
improves 9.367% in instructions and 9.406% in time. All control instruction
medians improve; the largest timing regression is +0.663%, within the one
percent gate. The first byte-count regex control lasts under half a millisecond
and has +1.856% timing variation. Its count is enlarged from 5,000 to 500,000
before independently repeating the entire control suite: instructions improve
0.783% and time is +0.368%. Both complete sample sets remain visible and their
medians are not combined across input sizes.

The native unit contract verifies every byte and classification in the C locale,
case folding, successful invalidation and query/failure/other-category stability.
Default, no-default and all-feature configurations pass 693 focused executions;
preparation repeats add 93, for 786 actual passes. Formatting, unsafe inventory
and locked all-target/all-feature compilation pass. The
[PHP oracle fixture](../tests/fixtures/locale_byte_tables/differential.php)
compares all byte classifications, C and available UTF-8 locales, regex ranges,
POSIX classes, Unicode matching and a locale-changing warning callback against
PHP and both PGO builds. Its ten output lines and thirty supported continuation
lines match PHP. The previously documented nested PRUNE mismatch remains a
failed PHP line; baseline and candidate remain byte-equal there.

Reproduce the focused source checks with `cargo test --offline --locked
--profile test-fast --lib stdlib::native_process::tests` and the regex, PCRE,
ctype and setlocale integration packets in the
[complete evidence](performance-phpstan-locale-byte-tables-samples.json).
The same commands are repeated with `--no-default-features` and `--all-features`.
The PHP oracle is run directly with PHP and the two exact default-feature PGO
binaries. Analysis counters use `scripts/bench-instructions.py --phase` with
fresh cache directories, exclusive locking and full output comparison.

Every expensive job uses verified 6 GiB/no-swap aggregate limits. There is no
OOM or timeout. Cleanup removes superseded Cargo targets while retaining exact
sources and binaries. The next selection attributes current native samples to
source sites inside the main executor, which still owns about forty percent
of executed instructions. This bounded checkpoint does not complete parity or
integrate the performance branch into main.
