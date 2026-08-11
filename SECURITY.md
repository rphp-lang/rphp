# Security policy

## Pre-alpha warning

RPHP is experimental pre-alpha software. It has not been hardened as a
security boundary and must not execute untrusted PHP code. Run it only with
source, inputs, files, environment variables, and network peers you trust,
inside an appropriately isolated development environment.

Only the current `main` branch is eligible for security fixes. There are no
supported releases yet.

## Reporting a vulnerability

Use GitHub's private vulnerability reporting for this repository. Include a
minimal reproducer, affected commit, platform and architecture, build command
and feature flags, expected impact, and any suggested mitigation. Please do
not open a public issue or pull request containing exploit details.

If private vulnerability reporting is temporarily unavailable, do not publish
the report; contact a maintainer through their GitHub profile to arrange a
private channel.

The project will acknowledge a report when a maintainer is available, then
coordinate validation, a fix, and disclosure. Pre-alpha status means no
response or remediation deadline is promised.

## Scope

Memory-safety bugs, sandbox escapes, unsafe-code invariant violations, parser
or VM denial of service, unintended file or network access, and dependency
supply-chain issues are in scope. Missing PHP compatibility without a security
impact belongs in the public issue tracker.
