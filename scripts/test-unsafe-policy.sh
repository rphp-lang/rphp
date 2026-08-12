#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
fixture="$(mktemp -d "${TMPDIR:-/tmp}/rphp-unsafe-policy-test.XXXXXX")"
trap 'rm -rf -- "${fixture}"' EXIT

mkdir -p "${fixture}/scripts" "${fixture}/src" "${fixture}/tests"
cp "${script_dir}/check-unsafe-policy.sh" "${fixture}/scripts/"
chmod +x "${fixture}/scripts/check-unsafe-policy.sh"

write_baseline() {
    local blocks="$1"
    local functions="$2"
    local comments="$3"
    local sections="$4"
    printf '%s\n' \
        "RPHP_UNSAFE_BASELINE_BLOCKS=${blocks}" \
        "RPHP_UNSAFE_BASELINE_FUNCTIONS=${functions}" \
        "RPHP_UNSAFE_BASELINE_SAFETY_COMMENTS=${comments}" \
        "RPHP_UNSAFE_BASELINE_SAFETY_SECTIONS=${sections}" \
        >"${fixture}/scripts/unsafe-baseline.env"
}

write_baseline 2 1 0 0
printf '%s\n' 'pub fn baseline() {}' >"${fixture}/src/lib.rs"
git -C "${fixture}" init -q
git -C "${fixture}" config user.email unsafe-policy@example.invalid
git -C "${fixture}" config user.name 'Unsafe policy test'
git -C "${fixture}" add .
git -C "${fixture}" commit -qm baseline

"${fixture}/scripts/check-unsafe-policy.sh" --diff-base HEAD >/dev/null

printf '%s\n' 'pub fn added() { unsafe { core::hint::unreachable_unchecked() } }' \
    >"${fixture}/src/added.rs"
if "${fixture}/scripts/check-unsafe-policy.sh" --diff-base HEAD >/dev/null 2>&1; then
    echo 'error: untracked unsafe block without proof was accepted' >&2
    exit 1
fi

printf '%s\n' \
    'pub fn added() {' \
    '    // SAFETY: this fixture checks recognition; the function is never called.' \
    '    unsafe { core::hint::unreachable_unchecked() }' \
    '}' >"${fixture}/src/added.rs"
"${fixture}/scripts/check-unsafe-policy.sh" --diff-base HEAD >/dev/null
git -C "${fixture}" add src/added.rs
git -C "${fixture}" commit -qm 'add proved block'

printf '%s\n' 'pub unsafe fn added_function() {}' >>"${fixture}/src/lib.rs"
if "${fixture}/scripts/check-unsafe-policy.sh" --diff-base HEAD >/dev/null 2>&1; then
    echo 'error: unsafe function without a Safety section was accepted' >&2
    exit 1
fi

printf '%s\n' \
    '/// # Safety' \
    '/// The caller must uphold the fixture invariant.' \
    'pub unsafe fn added_function() {}' >>"${fixture}/src/documented.rs"
rm "${fixture}/src/lib.rs"
printf '%s\n' 'pub fn baseline() {}' >"${fixture}/src/lib.rs"
"${fixture}/scripts/check-unsafe-policy.sh" --diff-base HEAD >/dev/null

marker="${fixture}/baseline-was-executed"
printf '%s\n' "touch ${marker}" >>"${fixture}/scripts/unsafe-baseline.env"
if "${fixture}/scripts/check-unsafe-policy.sh" >/dev/null 2>&1; then
    echo 'error: executable baseline content was accepted' >&2
    exit 1
fi
if [[ -e "${marker}" ]]; then
    echo 'error: baseline content was executed' >&2
    exit 1
fi

echo 'Unsafe policy self-test passed.'
