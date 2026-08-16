#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
fixture_source="${repository_root}/tests/fixtures/symfony-dependency-injection"
composer_version="2.8.12"
composer_sha256="f446ea719708bb85fcbf4ef18def5d0515f1f9b4d703f6d820c9c1656e10a2f2"
composer_url="https://getcomposer.org/download/${composer_version}/composer.phar"
composer_phar="${RPHP_COMPOSER_PHAR:-${TMPDIR:-/tmp}/rphp-composer-${composer_version}.phar}"
fixture="$(mktemp -d "${TMPDIR:-/tmp}/rphp-symfony-di-s1.XXXXXX")"
download_candidate="${composer_phar}.candidate.$$"
trap 'rm -rf -- "${fixture}"; rm -f -- "${download_candidate}"' EXIT

sha256_file() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    else
        shasum -a 256 "$1" | awk '{print $1}'
    fi
}

if [[ ! -f "${composer_phar}" ]]; then
    curl -fsSL --retry 3 --retry-all-errors --retry-delay 2 \
        "${composer_url}" -o "${download_candidate}"
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
rm -rf -- "${fixture}/vendor" "${fixture}/.composer-home"
rm -f -- "${fixture}/RphpPrebuiltContainer.php"
(
    cd "${fixture}"
    COMPOSER_HOME="${fixture}/.composer-home" php "${composer_phar}" install \
        --no-ansi --no-dev --no-interaction --no-progress --no-scripts --prefer-dist --quiet
    php build.php
)

if [[ ! -s "${fixture}/RphpPrebuiltContainer.php" ]]; then
    echo "error: reference PHP did not generate the prebuilt container" >&2
    exit 1
fi

expected='hello RPHP|same|hello|missing'
reference="$(php "${fixture}/run.php")"
if [[ "${reference}" != "${expected}" ]]; then
    printf 'error: reference PHP DependencyInjection output mismatch\nexpected: %s\nactual:   %s\n' \
        "${expected}" "${reference}" >&2
    exit 1
fi

cd "${repository_root}"
cargo build --locked --quiet
actual="$(target/debug/rphp "${fixture}/run.php")"
if [[ "${actual}" != "${reference}" ]]; then
    printf 'error: Symfony DependencyInjection S1 gate mismatch\nreference: %s\nRPHP:      %s\n' \
        "${reference}" "${actual}" >&2
    exit 1
fi

echo "Symfony DependencyInjection v7.4.16 S1 gate passed: ${actual}"
