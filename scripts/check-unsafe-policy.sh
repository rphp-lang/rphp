#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repository_root="$(cd "${script_dir}/.." && pwd)"
baseline_file="${script_dir}/unsafe-baseline.env"
diff_base=""

usage() {
    cat <<'USAGE'
Usage: scripts/check-unsafe-policy.sh [--diff-base <git-revision>]

Reports the canonical unsafe inventory under src/, enforces the committed
ceilings, and optionally checks newly added unsafe code against a Git base.
USAGE
}

while (($# > 0)); do
    case "$1" in
        --diff-base)
            if (($# < 2)); then
                echo "error: --diff-base requires a Git revision" >&2
                exit 2
            fi
            diff_base="$2"
            shift 2
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "error: unknown argument: $1" >&2
            usage >&2
            exit 2
            ;;
    esac
done

if [[ ! -f "${baseline_file}" ]]; then
    echo "error: missing unsafe baseline: ${baseline_file}" >&2
    exit 2
fi

baseline_keys=(
    RPHP_UNSAFE_BASELINE_BLOCKS
    RPHP_UNSAFE_BASELINE_FUNCTIONS
    RPHP_UNSAFE_BASELINE_SAFETY_COMMENTS
    RPHP_UNSAFE_BASELINE_SAFETY_SECTIONS
)

# The baseline is controlled by the checkout being reviewed, so never execute
# it as shell source. Accept exactly one decimal assignment for every known key
# and reject every other non-comment line.
declare -A baseline_values=()
while IFS= read -r baseline_line || [[ -n "${baseline_line}" ]]; do
    [[ "${baseline_line}" =~ ^[[:space:]]*$ ]] && continue
    [[ "${baseline_line}" =~ ^[[:space:]]*# ]] && continue
    if [[ ! "${baseline_line}" =~ ^([A-Z0-9_]+)=([0-9]+)$ ]]; then
        echo "error: invalid unsafe baseline line: ${baseline_line}" >&2
        exit 2
    fi
    baseline_key="${BASH_REMATCH[1]}"
    baseline_value="${BASH_REMATCH[2]}"
    known_key=false
    for expected_key in "${baseline_keys[@]}"; do
        if [[ "${baseline_key}" == "${expected_key}" ]]; then
            known_key=true
            break
        fi
    done
    if [[ "${known_key}" != true ]]; then
        echo "error: unknown unsafe baseline key: ${baseline_key}" >&2
        exit 2
    fi
    if [[ -n "${baseline_values[${baseline_key}]+set}" ]]; then
        echo "error: duplicate unsafe baseline key: ${baseline_key}" >&2
        exit 2
    fi
    baseline_values["${baseline_key}"]="${baseline_value}"
done <"${baseline_file}"

for baseline_key in "${baseline_keys[@]}"; do
    if [[ -z "${baseline_values[${baseline_key}]+set}" ]]; then
        echo "error: missing unsafe baseline key: ${baseline_key}" >&2
        exit 2
    fi
    printf -v "${baseline_key}" '%s' "${baseline_values[${baseline_key}]}"
done

cd "${repository_root}"

count_matches() {
    local search_root="$1"
    local pattern="$2"
    local matches

    matches="$(rg -n --glob '*.rs' "${pattern}" "${search_root}" 2>/dev/null || true)"
    if [[ -z "${matches}" ]]; then
        printf '0\n'
    else
        printf '%s\n' "${matches}" | awk 'END { print NR }'
    fi
}

unsafe_block_pattern='\bunsafe\s*\{'
unsafe_function_pattern='\bunsafe(?:\s+extern(?:\s+"[^"]+")?)?\s+fn\s+[A-Za-z_][A-Za-z0-9_]*\s*(?:<|\()'
safety_comment_pattern='^\s*//+\s*SAFETY:'
safety_section_pattern='^\s*///\s*#\s+Safety\b'

production_blocks="$(count_matches src "${unsafe_block_pattern}")"
production_functions="$(count_matches src "${unsafe_function_pattern}")"
production_comments="$(count_matches src "${safety_comment_pattern}")"
production_sections="$(count_matches src "${safety_section_pattern}")"
test_blocks="$(count_matches tests "${unsafe_block_pattern}")"
test_functions="$(count_matches tests "${unsafe_function_pattern}")"

cat <<REPORT
Unsafe policy inventory
  production unsafe blocks:     ${production_blocks} (ceiling ${RPHP_UNSAFE_BASELINE_BLOCKS})
  production unsafe functions:  ${production_functions} (ceiling ${RPHP_UNSAFE_BASELINE_FUNCTIONS})
  production SAFETY annotations: ${production_comments} (floor ${RPHP_UNSAFE_BASELINE_SAFETY_COMMENTS})
  production # Safety sections: ${production_sections} (floor ${RPHP_UNSAFE_BASELINE_SAFETY_SECTIONS})
  test-only unsafe blocks:      ${test_blocks}
  test-only unsafe functions:   ${test_functions}
REPORT

status=0

if ((production_blocks > RPHP_UNSAFE_BASELINE_BLOCKS)); then
    echo "error: production unsafe-block ceiling increased" >&2
    status=1
fi
if ((production_functions > RPHP_UNSAFE_BASELINE_FUNCTIONS)); then
    echo "error: production unsafe-function ceiling increased" >&2
    status=1
fi
if ((production_comments < RPHP_UNSAFE_BASELINE_SAFETY_COMMENTS)); then
    echo "error: production SAFETY-comment floor decreased" >&2
    status=1
fi
if ((production_sections < RPHP_UNSAFE_BASELINE_SAFETY_SECTIONS)); then
    echo "error: production # Safety-section floor decreased" >&2
    status=1
fi

if [[ -n "${diff_base}" ]]; then
    if ! git rev-parse --verify "${diff_base}^{commit}" >/dev/null 2>&1; then
        echo "error: diff base is not an available commit: ${diff_base}" >&2
        exit 2
    fi

    diff_file="$(mktemp "${TMPDIR:-/tmp}/rphp-unsafe-diff.XXXXXX")"
    trap 'rm -f "${diff_file}"' EXIT
    git diff --unified=4 "${diff_base}" -- src >"${diff_file}"

    # `git diff` omits untracked files. Append them as synthetic added-file
    # hunks so a local ratchet cannot hide a new unsafe block behind unused
    # aggregate budget from an unrelated deletion.
    while IFS= read -r -d '' untracked_source; do
        {
            printf 'diff --git a/%s b/%s\n' "${untracked_source}" "${untracked_source}"
            printf -- '--- /dev/null\n'
            printf '+++ b/%s\n' "${untracked_source}"
            printf '@@ -0,0 +1,%s @@\n' "$(awk 'END { print NR }' "${untracked_source}")"
            sed 's/^/+/' "${untracked_source}"
        } >>"${diff_file}"
    done < <(git ls-files --others --exclude-standard -z -- 'src/*.rs' 'src/**/*.rs')

    if ! awk '
        function flush_hunk() {
            if (added_blocks > 0 && added_comments == 0) {
                print "error: added unsafe block lacks an added SAFETY proof in " file " " hunk > "/dev/stderr"
                failed = 1
            }
            if (added_functions > added_sections) {
                print "error: added unsafe function lacks an added # Safety contract in " file " " hunk > "/dev/stderr"
                failed = 1
            }
            added_blocks = 0
            added_comments = 0
            added_functions = 0
            added_sections = 0
        }

        /^diff --git / {
            flush_hunk()
            file = $4
            sub(/^b\//, "", file)
            hunk = ""
            next
        }
        /^@@ / {
            flush_hunk()
            hunk = $0
            next
        }
        /^\+\+\+/ { next }
        /^\+/ {
            line = substr($0, 2)
            if (line ~ /unsafe[[:space:]]*\{/) added_blocks++
            if (line ~ /unsafe([[:space:]]+extern([[:space:]]+"[^"]+")?)?[[:space:]]+fn[[:space:]]+[A-Za-z_][A-Za-z0-9_]*/) added_functions++
            if (line ~ /\/\/[[:space:]]*SAFETY:/) added_comments++
            if (line ~ /\/\/\/[[:space:]]*#[[:space:]]+Safety/) added_sections++
        }
        END {
            flush_hunk()
            exit failed
        }
    ' "${diff_file}"; then
        status=1
    fi
fi

if ((status != 0)); then
    cat >&2 <<'ERROR'
Unsafe policy check failed. Document the concrete invariant and caller
obligations, reduce or encapsulate the unsafe operation, or submit a separately
reviewed baseline change with an explicit security rationale.
ERROR
    exit "${status}"
fi

echo "Unsafe policy check passed."
