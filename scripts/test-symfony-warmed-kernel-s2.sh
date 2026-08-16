#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
fixture_source="${repository_root}/tests/fixtures/symfony-warmed-kernel"
composer_version="2.8.12"
composer_sha256="f446ea719708bb85fcbf4ef18def5d0515f1f9b4d703f6d820c9c1656e10a2f2"
composer_url="https://getcomposer.org/download/${composer_version}/composer.phar"
composer_phar="${RPHP_COMPOSER_PHAR:-${TMPDIR:-/tmp}/rphp-composer-${composer_version}.phar}"
fixture="$(mktemp -d "${TMPDIR:-/tmp}/rphp-symfony-warmed-kernel-s2.XXXXXX")"
download_candidate="${composer_phar}.candidate.$$"
trap 'rm -rf -- "${fixture}"; rm -f -- "${download_candidate}"' EXIT

sha256_file() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    else
        shasum -a 256 "$1" | awk '{print $1}'
    fi
}

tree_digest() {
    local root="$1"
    local manifest="${fixture}/cache-digest-manifest"
    : >"${manifest}"
    while IFS= read -r file; do
        printf '%s  %s\n' "$(sha256_file "${file}")" "${file#"${root}/"}" >>"${manifest}"
    done < <(find "${root}" -type f | LC_ALL=C sort)
    sha256_file "${manifest}"
}

reference_version="$(php -r 'echo PHP_MAJOR_VERSION.".".PHP_MINOR_VERSION.".".PHP_RELEASE_VERSION;')"
if [[ "${reference_version}" != 8.5.* ]]; then
    echo "error: Symfony S2 requires a PHP 8.5 reference oracle, got ${reference_version}" >&2
    exit 1
fi

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
rm -rf -- "${fixture}/vendor" "${fixture}/var" "${fixture}/.composer-home"
(
    cd "${fixture}"
    COMPOSER_HOME="${fixture}/.composer-home" php "${composer_phar}" install \
        --no-ansi --no-dev --no-interaction --no-progress --no-scripts --prefer-dist --quiet
    php warm.php
)

cache="${fixture}/var/cache/prod"
if [[ ! -s "${cache}/Rphp_SymfonyKernelFixture_KernelProdContainer.php" ]]; then
    echo "error: reference PHP did not generate the warmed container" >&2
    exit 1
fi

expected='200|warmed|OK'
reference="$(php "${fixture}/run.php")"
if [[ "${reference}" != "${expected}" ]]; then
    printf 'error: reference PHP warmed-kernel output mismatch\nexpected: %s\nactual:   %s\n' \
        "${expected}" "${reference}" >&2
    exit 1
fi
if [[ ! -s "${cache}/url_matching_routes.php" ]]; then
    echo "error: reference PHP did not generate the compiled route cache" >&2
    exit 1
fi
cache_before="$(tree_digest "${cache}")"

cd "${repository_root}"
cargo build --locked --quiet
actual="$(target/debug/rphp "${fixture}/run.php")"
if [[ "${actual}" != "${reference}" ]]; then
    printf 'error: Symfony warmed-kernel S2 gate mismatch\nreference: %s\nRPHP:      %s\n' \
        "${reference}" "${actual}" >&2
    exit 1
fi
cache_after="$(tree_digest "${cache}")"
if [[ "${cache_after}" != "${cache_before}" ]]; then
    echo "error: RPHP modified the reference-PHP-built warmed cache" >&2
    exit 1
fi

echo "Symfony FrameworkBundle v7.4.16 warmed-kernel S2 gate passed: ${actual}"
