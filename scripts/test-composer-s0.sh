#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
fixture_source="${repository_root}/tests/fixtures/composer-s0"
failure_fixture_source="${repository_root}/tests/fixtures/composer-platform-fail"
composer_version="2.8.12"
composer_sha256="f446ea719708bb85fcbf4ef18def5d0515f1f9b4d703f6d820c9c1656e10a2f2"
composer_url="https://getcomposer.org/download/${composer_version}/composer.phar"
composer_phar="${RPHP_COMPOSER_PHAR:-${TMPDIR:-/tmp}/rphp-composer-${composer_version}.phar}"
fixture="$(mktemp -d "${TMPDIR:-/tmp}/rphp-composer-s0.XXXXXX")"
failure_fixture="$(mktemp -d "${TMPDIR:-/tmp}/rphp-composer-platform-fail.XXXXXX")"
download_candidate="${composer_phar}.candidate.$$"
trap 'rm -rf -- "${fixture}" "${failure_fixture}"; rm -f -- "${download_candidate}"' EXIT

sha256_file() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    else
        shasum -a 256 "$1" | awk '{print $1}'
    fi
}

if [[ ! -f "${composer_phar}" ]]; then
    curl -fsSL "${composer_url}" -o "${download_candidate}"
    if [[ "$(sha256_file "${download_candidate}")" != "${composer_sha256}" ]]; then
        echo "error: downloaded Composer ${composer_version} checksum mismatch" >&2
        exit 1
    fi
    mv "${download_candidate}" "${composer_phar}"
fi

if [[ "$(sha256_file "${composer_phar}")" != "${composer_sha256}" ]]; then
    echo "error: Composer artifact checksum mismatch: ${composer_phar}" >&2
    exit 1
fi

composer_banner="$(php "${composer_phar}" --version --no-ansi)"
if [[ "${composer_banner}" != "Composer version ${composer_version} "* ]]; then
    echo "error: expected Composer ${composer_version}, got: ${composer_banner}" >&2
    exit 1
fi

cp -R "${fixture_source}/." "${fixture}/"
cp -R "${failure_fixture_source}/." "${failure_fixture}/"
(
    cd "${fixture}"
    COMPOSER_HOME="${fixture}/.composer-home" php "${composer_phar}" dump-autoload \
        --no-ansi --no-dev --no-interaction --no-scripts --quiet
)
(
    cd "${failure_fixture}"
    COMPOSER_HOME="${failure_fixture}/.composer-home" php "${composer_phar}" dump-autoload \
        --no-ansi --no-dev --no-interaction --no-scripts --quiet
)

cd "${repository_root}"
cargo build --locked --quiet
actual="$(target/debug/rphp "${fixture}/run.php")"
expected='80200|8.2.0|8.2.0|8|cli|loader|hello|composer'
if [[ "${actual}" != "${expected}" ]]; then
    printf 'error: Composer S0 gate mismatch\nexpected: %s\nactual:   %s\n' \
        "${expected}" "${actual}" >&2
    exit 1
fi

set +e
platform_failure_output="$(target/debug/rphp "${failure_fixture}/run.php" 2>&1)"
platform_failure_status=$?
set -e
if ((platform_failure_status == 0)); then
    echo "error: Composer negative platform check unexpectedly passed" >&2
    exit 1
fi
if [[ "${platform_failure_output}" != *'RuntimeException'* \
    || "${platform_failure_output}" != *'require a PHP version ">= 8.2.1"'* \
    || "${platform_failure_output}" != *'You are running 8.2.0'* ]]; then
    printf 'error: Composer negative platform check mismatch\n%s\n' \
        "${platform_failure_output}" >&2
    exit 1
fi

echo "Composer ${composer_version} S0 gate passed: ${actual}"
