#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
fixture_source="${repository_root}/tests/fixtures/symfony-warmed-kernel"
reference_php="${RPHP_REFERENCE_PHP:-php}"
composer_version="2.8.12"
composer_sha256="f446ea719708bb85fcbf4ef18def5d0515f1f9b4d703f6d820c9c1656e10a2f2"
composer_url="https://getcomposer.org/download/${composer_version}/composer.phar"
composer_phar="${RPHP_COMPOSER_PHAR:-${TMPDIR:-/tmp}/rphp-composer-${composer_version}.phar}"
workspace="$(mktemp -d "${TMPDIR:-/tmp}/rphp-symfony-cold-kernel-s3.XXXXXX")"
candidate_target="$(mktemp -d "${TMPDIR:-/tmp}/rphp-candidate-symfony-s3.XXXXXX")"
download_candidate="${composer_phar}.candidate.$$"

cleanup() {
    rm -rf -- "${workspace}" "${candidate_target}"
    rm -f -- "${download_candidate}"
}
trap cleanup EXIT

sha256_file() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    else
        shasum -a 256 "$1" | awk '{print $1}'
    fi
}

normalize_generated_names() {
    sed -E \
        -e 's/Container[A-Za-z0-9]+/Container@/g' \
        -e 's/Ghost[A-Fa-f0-9]+/Ghost@/g' \
        -e 's/_ServiceLocator_[A-Za-z0-9]+/_ServiceLocator_@/g'
}

cache_manifest() {
    local cache="$1"
    find "${cache}" -type f -print \
        | sed "s#^${cache}/##" \
        | normalize_generated_names \
        | LC_ALL=C sort
}

tree_digest() {
    local cache="$1"
    local digest_manifest="$2"
    : >"${digest_manifest}"
    while IFS= read -r file; do
        printf '%s  %s\n' "$(sha256_file "${file}")" "${file#"${cache}/"}" >>"${digest_manifest}"
    done < <(find "${cache}" -type f -print | LC_ALL=C sort)
    sha256_file "${digest_manifest}"
}

canonicalize_gate_output() {
    local input="$1"
    local output="$2"
    : >"${output}"
    while IFS= read -r line || [[ -n "${line}" ]]; do
        case "${line}" in
            included=*|symbols=*|container-meta=*|route-meta=*)
                local key="${line%%=*}"
                local values="${line#*=}"
                local canonical=""
                if [[ -n "${values}" ]]; then
                    canonical="$(printf '%s' "${values}" | tr ',' '\n' | LC_ALL=C sort | paste -sd, -)"
                fi
                printf '%s=%s\n' "${key}" "${canonical}" >>"${output}"
                ;;
            *)
                printf '%s\n' "${line}" >>"${output}"
                ;;
        esac
    done <"${input}"
}

run_capture() {
    local runtime="$1"
    local directory="$2"
    local script="$3"
    local prefix="$4"
    local status
    set +e
    (
        cd "${directory}"
        ulimit -s 65520 2>/dev/null || true
        "${runtime}" "${script}"
    ) >"${prefix}.stdout" 2>"${prefix}.stderr"
    status=$?
    set -e
    printf '%s\n' "${status}" >"${prefix}.status"
}

compare_capture() {
    local phase="$1"
    local reference_prefix="$2"
    local candidate_prefix="$3"
    local canonicalize="${4:-no}"
    local reference_stdout="${reference_prefix}.stdout"
    local candidate_stdout="${candidate_prefix}.stdout"

    if [[ "${canonicalize}" == "yes" ]]; then
        canonicalize_gate_output "${reference_stdout}" "${reference_prefix}.canonical"
        canonicalize_gate_output "${candidate_stdout}" "${candidate_prefix}.canonical"
        reference_stdout="${reference_prefix}.canonical"
        candidate_stdout="${candidate_prefix}.canonical"
    fi

    if ! cmp -s "${reference_prefix}.status" "${candidate_prefix}.status"; then
        echo "error: ${phase} exit status differs between PHP ($(cat "${reference_prefix}.status")) and RPHP ($(cat "${candidate_prefix}.status"))" >&2
        cat "${candidate_prefix}.stderr" >&2
        exit 1
    fi
    if [[ "$(cat "${candidate_prefix}.status")" != "0" ]]; then
        echo "error: ${phase} exited non-zero" >&2
        exit 1
    fi
    if ! cmp -s "${reference_prefix}.stderr" "${candidate_prefix}.stderr"; then
        echo "error: ${phase} diagnostics differ between PHP and RPHP" >&2
        diff -u "${reference_prefix}.stderr" "${candidate_prefix}.stderr" >&2 || true
        exit 1
    fi
    if ! cmp -s "${reference_stdout}" "${candidate_stdout}"; then
        echo "error: ${phase} output differs between PHP and RPHP" >&2
        diff -u "${reference_stdout}" "${candidate_stdout}" >&2 || true
        exit 1
    fi
}

assert_expected_requests() {
    local output="$1"
    grep -Fqx 'health=200|warmed|OK' "${output}"
    grep -Fqx 'missing=Symfony\Component\HttpKernel\Exception\NotFoundHttpException|404|No route found for "GET http://localhost/missing"' "${output}"
}

assert_cache_shape() {
    local cache="$1"
    [[ -s "${cache}/Rphp_SymfonyKernelFixture_KernelProdContainer.php" ]]
    [[ -f "${cache}/Rphp_SymfonyKernelFixture_KernelProdContainer.php.lock" ]]
    [[ -s "${cache}/Rphp_SymfonyKernelFixture_KernelProdContainer.php.meta.json" ]]
    [[ -s "${cache}/url_matching_routes.php" ]]
    [[ -s "${cache}/url_matching_routes.php.meta.json" ]]
    if find "${cache}" -type f \( -name '*.tmp' -o -name '*.temp' -o -name '*~' \) -print | grep -q .; then
        echo "error: cache publication left a temporary file" >&2
        exit 1
    fi
}

lint_cache() {
    local cache="$1"
    while IFS= read -r file; do
        "${reference_php}" -l "${file}" >/dev/null
    done < <(find "${cache}" -type f -name '*.php' -print | LC_ALL=C sort)
}

compare_cache_manifests() {
    local phase="$1"
    local reference_cache="$2"
    local candidate_cache="$3"
    local reference_manifest="${workspace}/${phase}-reference.manifest"
    local candidate_manifest="${workspace}/${phase}-candidate.manifest"
    cache_manifest "${reference_cache}" >"${reference_manifest}"
    cache_manifest "${candidate_cache}" >"${candidate_manifest}"
    if ! cmp -s "${reference_manifest}" "${candidate_manifest}"; then
        echo "error: ${phase} normalized cache manifests differ" >&2
        diff -u "${reference_manifest}" "${candidate_manifest}" >&2 || true
        exit 1
    fi
}

run_concurrent_scripts() {
    local runtime="$1"
    local directory="$2"
    local script="$3"
    local prefix="$4"
    local first_status second_status
    set +e
    (
        cd "${directory}"
        ulimit -s 65520 2>/dev/null || true
        "${runtime}" "${script}"
    ) >"${prefix}-1.stdout" 2>"${prefix}-1.stderr" &
    local first_pid=$!
    (
        cd "${directory}"
        ulimit -s 65520 2>/dev/null || true
        "${runtime}" "${script}"
    ) >"${prefix}-2.stdout" 2>"${prefix}-2.stderr" &
    local second_pid=$!
    wait "${first_pid}"
    first_status=$?
    wait "${second_pid}"
    second_status=$?
    set -e
    printf '%s\n' "${first_status}" >"${prefix}-1.status"
    printf '%s\n' "${second_status}" >"${prefix}-2.status"
}

compare_concurrent_captures() {
    local phase="$1"
    local reference_prefix="$2"
    local candidate_prefix="$3"
    local reference_manifest="${workspace}/${phase}-reference.captures"
    local candidate_manifest="${workspace}/${phase}-candidate.captures"

    : >"${reference_manifest}"
    : >"${candidate_manifest}"
    for index in 1 2; do
        canonicalize_gate_output \
            "${reference_prefix}-${index}.stdout" \
            "${reference_prefix}-${index}.canonical"
        canonicalize_gate_output \
            "${candidate_prefix}-${index}.stdout" \
            "${candidate_prefix}-${index}.canonical"

        if [[ "$(cat "${reference_prefix}-${index}.status")" != "0" ]]; then
            echo "error: ${phase} PHP process ${index} exited non-zero" >&2
            exit 1
        fi
        if [[ "$(cat "${candidate_prefix}-${index}.status")" != "0" ]]; then
            echo "error: ${phase} RPHP process ${index} exited non-zero" >&2
            cat "${candidate_prefix}-${index}.stderr" >&2
            exit 1
        fi

        printf '%s %s %s\n' \
            "$(sha256_file "${reference_prefix}-${index}.status")" \
            "$(sha256_file "${reference_prefix}-${index}.stderr")" \
            "$(sha256_file "${reference_prefix}-${index}.canonical")" \
            >>"${reference_manifest}"
        printf '%s %s %s\n' \
            "$(sha256_file "${candidate_prefix}-${index}.status")" \
            "$(sha256_file "${candidate_prefix}-${index}.stderr")" \
            "$(sha256_file "${candidate_prefix}-${index}.canonical")" \
            >>"${candidate_manifest}"
    done

    LC_ALL=C sort -o "${reference_manifest}" "${reference_manifest}"
    LC_ALL=C sort -o "${candidate_manifest}" "${candidate_manifest}"
    if ! cmp -s "${reference_manifest}" "${candidate_manifest}"; then
        echo "error: ${phase} concurrent capture multisets differ between PHP and RPHP" >&2
        for index in 1 2; do
            echo "--- PHP process ${index}" >&2
            cat "${reference_prefix}-${index}.canonical" >&2
            echo "--- RPHP process ${index}" >&2
            cat "${candidate_prefix}-${index}.canonical" >&2
        done
        exit 1
    fi
}

reference_version="$("${reference_php}" -r 'echo PHP_MAJOR_VERSION.".".PHP_MINOR_VERSION.".".PHP_RELEASE_VERSION;')"
if [[ "${reference_version}" != 8.5.* ]]; then
    echo "error: Symfony S3 requires a PHP 8.5 reference oracle, got ${reference_version}" >&2
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
    echo "error: Composer artifact checksum mismatch" >&2
    exit 1
fi

"${repository_root}/scripts/cleanup-builds.sh"

seed="${workspace}/seed"
reference_fixture="${workspace}/reference"
candidate_fixture="${workspace}/candidate"
mkdir -p "${seed}"
cp -R "${fixture_source}/." "${seed}/"
rm -rf -- "${seed}/vendor" "${seed}/var" "${seed}/.composer-home"
lock_before="$(sha256_file "${seed}/composer.lock")"
(
    cd "${seed}"
    COMPOSER_HOME="${seed}/.composer-home" "${reference_php}" "${composer_phar}" install \
        --no-ansi --no-dev --no-interaction --no-progress --no-scripts --prefer-dist --quiet
)
if [[ "$(sha256_file "${seed}/composer.lock")" != "${lock_before}" ]]; then
    echo "error: Composer changed the pinned lock" >&2
    exit 1
fi
cp -R "${seed}/." "${reference_fixture}/"
cp -R "${seed}/." "${candidate_fixture}/"

(
    cd "${repository_root}"
    cargo build --locked --release --quiet --target-dir "${candidate_target}"
)
rphp="${candidate_target}/release/rphp"

reference_cache="${reference_fixture}/var/cache/prod"
candidate_cache="${candidate_fixture}/var/cache/prod"

run_capture "${reference_php}" "${reference_fixture}" s3-gate.php "${workspace}/cold-reference"
run_capture "${rphp}" "${candidate_fixture}" s3-gate.php "${workspace}/cold-candidate"
compare_capture cold "${workspace}/cold-reference" "${workspace}/cold-candidate" yes
assert_expected_requests "${workspace}/cold-candidate.canonical"
assert_cache_shape "${reference_cache}"
assert_cache_shape "${candidate_cache}"
lint_cache "${reference_cache}"
lint_cache "${candidate_cache}"
compare_cache_manifests cold "${reference_cache}" "${candidate_cache}"

reference_before="$(tree_digest "${reference_cache}" "${workspace}/cached-reference-before.digest")"
candidate_before="$(tree_digest "${candidate_cache}" "${workspace}/cached-candidate-before.digest")"
run_capture "${reference_php}" "${reference_fixture}" s3-gate.php "${workspace}/cached-reference"
run_capture "${rphp}" "${candidate_fixture}" s3-gate.php "${workspace}/cached-candidate"
compare_capture cached "${workspace}/cached-reference" "${workspace}/cached-candidate" yes
assert_expected_requests "${workspace}/cached-candidate.canonical"
reference_after="$(tree_digest "${reference_cache}" "${workspace}/cached-reference-after.digest")"
candidate_after="$(tree_digest "${candidate_cache}" "${workspace}/cached-candidate-after.digest")"
[[ "${reference_before}" == "${reference_after}" ]]
[[ "${candidate_before}" == "${candidate_after}" ]]

rm -rf -- "${reference_cache}" "${candidate_cache}"
run_capture "${reference_php}" "${reference_fixture}" s3-gate.php "${workspace}/deleted-reference"
run_capture "${rphp}" "${candidate_fixture}" s3-gate.php "${workspace}/deleted-candidate"
compare_capture deleted-cache "${workspace}/deleted-reference" "${workspace}/deleted-candidate" yes
assert_expected_requests "${workspace}/deleted-candidate.canonical"
assert_cache_shape "${reference_cache}"
assert_cache_shape "${candidate_cache}"
lint_cache "${reference_cache}"
lint_cache "${candidate_cache}"
compare_cache_manifests deleted "${reference_cache}" "${candidate_cache}"

cp "${fixture_source}/malformed-route-cache.php" "${reference_cache}/url_matching_routes.php"
cp "${fixture_source}/malformed-route-cache.php" "${candidate_cache}/url_matching_routes.php"
reference_before="$(tree_digest "${reference_cache}" "${workspace}/malformed-reference-before.digest")"
candidate_before="$(tree_digest "${candidate_cache}" "${workspace}/malformed-candidate-before.digest")"
run_capture "${reference_php}" "${reference_fixture}" s3-gate.php "${workspace}/malformed-reference"
run_capture "${rphp}" "${candidate_fixture}" s3-gate.php "${workspace}/malformed-candidate"
compare_capture malformed-cache "${workspace}/malformed-reference" "${workspace}/malformed-candidate" yes
grep -Fqx 'health=RuntimeException|0|malformed-cache' "${workspace}/malformed-candidate.canonical"
grep -Fqx 'missing=RuntimeException|0|malformed-cache' "${workspace}/malformed-candidate.canonical"
reference_after="$(tree_digest "${reference_cache}" "${workspace}/malformed-reference-after.digest")"
candidate_after="$(tree_digest "${candidate_cache}" "${workspace}/malformed-candidate-after.digest")"
[[ "${reference_before}" == "${reference_after}" ]]
[[ "${candidate_before}" == "${candidate_after}" ]]

reference_atomic="${workspace}/reference-atomic"
candidate_atomic="${workspace}/candidate-atomic"
cp -R "${seed}/." "${reference_atomic}/"
cp -R "${seed}/." "${candidate_atomic}/"
run_concurrent_scripts "${reference_php}" "${reference_atomic}" s3-gate.php "${workspace}/atomic-reference"
run_concurrent_scripts "${rphp}" "${candidate_atomic}" s3-gate.php "${workspace}/atomic-candidate"
compare_concurrent_captures \
    atomic \
    "${workspace}/atomic-reference" \
    "${workspace}/atomic-candidate"
for index in 1 2; do
    assert_expected_requests "${workspace}/atomic-candidate-${index}.canonical"
done
assert_cache_shape "${reference_atomic}/var/cache/prod"
assert_cache_shape "${candidate_atomic}/var/cache/prod"
lint_cache "${reference_atomic}/var/cache/prod"
lint_cache "${candidate_atomic}/var/cache/prod"
compare_cache_manifests atomic "${reference_atomic}/var/cache/prod" "${candidate_atomic}/var/cache/prod"
run_capture "${reference_php}" "${reference_atomic}" s3-gate.php "${workspace}/atomic-gate-reference"
run_capture "${rphp}" "${candidate_atomic}" s3-gate.php "${workspace}/atomic-gate-candidate"
compare_capture atomic-cache-load "${workspace}/atomic-gate-reference" "${workspace}/atomic-gate-candidate" yes
assert_expected_requests "${workspace}/atomic-gate-candidate.canonical"

"${repository_root}/scripts/cleanup-builds.sh"

echo "Symfony FrameworkBundle v7.4.16 S3 cold-build gate passed against PHP ${reference_version}"
