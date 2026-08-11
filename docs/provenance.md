# Code provenance policy

RPHP is independently implemented. Compatibility goals may be derived from
public PHP documentation, specifications, and black-box behavior, but RPHP
does not use the Zend Engine architecture or source as its implementation
base.

## Contribution rules

- Write original source, tests, tables, comments, and documentation.
- Do not copy or mechanically translate implementation material from PHP/Zend,
  TA-Lib, another runtime, or another library.
- A behavior described in a public manual may be reimplemented independently;
  cite the behavior source in the pull request when it is non-obvious.
- If material is intentionally adapted from another project, disclose the
  exact source, version, files, license, and required attribution before the
  change is accepted. Compatibility with RPHP's repository license must be
  reviewed first.
- Generated code or AI-assisted code must follow the same rules. Prompts and
  model output do not establish provenance or licensing rights.
- Do not remove an existing attribution or origin note without establishing
  why it is no longer required.

## Current-tree audit boundary

The publication audit searched the tracked source tree for license headers,
copyright notices, source-host URLs, and references to PHP/Zend or TA-Lib. It
found architectural comparison terminology in comments and explicitly marked
behavioral tests described as inspired by selected `php-src` test cases; it
did not find vendored third-party source or a foreign license header. This is
an automated review, not proof of authorship.

## Maintainer attestation

On 2026-08-12, the project maintainer attested that the current RPHP codebase
was developed specifically for RPHP by its maintainers with AI assistance,
under human direction and review. To the maintainer's knowledge, no
third-party implementation source was copied or mechanically translated into
the project. Any exception discovered later must be documented and confirmed
as license-compatible before the affected code is distributed.

Dependency provenance is tracked separately in `THIRD_PARTY.md` and
`Cargo.lock`.
