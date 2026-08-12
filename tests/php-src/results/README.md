# Published php-src PHPT results

Results are grouped by pinned upstream release and tested RPHP commit. Upstream
php-src checkouts, generated PHP files, shard manifests and build artifacts are
kept outside this repository.

Each schema-4 `*-summary.json` records the exact RPHP, runner and php-src
commits, feature set, architecture, timeout policy, aggregate statuses, failure
categories and suite breakdown. The corresponding `*-manifest.jsonl` has one
deterministic record per unmodified PHPT path with its status and category.
Skip, expected-failure, unsupported, timeout and crash records also retain
their public reason.

The headline rate is always `pass / (pass + fail)`. Skips, known upstream
`XFAIL` outcomes, unsupported cases, timeouts and crashes are published
separately and are never counted as passes.

Schema 4 also publishes an execution profile. `pre_execution_failed` separates
failed `SKIPIF` or runner setup from execution of the test's `FILE` section.
`front_end_rejected` includes observed parse/compile rejection even when that
diagnostic is the exact expected output of a passing negative test, and
`runtime_reached` is the remainder. It is evidence about how far the target
got, not a second compatibility score. A runtime-reaching case may still have
entirely incorrect behavior. Only an exact PHPT pass counts as compatible.

Use `scripts/run-php-src-phpt.sh` to reproduce or update a result.
