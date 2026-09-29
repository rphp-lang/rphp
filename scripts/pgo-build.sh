#!/usr/bin/env bash
# Maximum-performance PGO build pipeline for rphp.
#
# Usage:
#   ./scripts/pgo-build.sh
#
# Run inside the aggregate memory boundary described in AGENTS.md.
# Produces an optimized binary and retained profiles in a fresh temporary
# candidate directory. RPHP_PGO_DIR may name another empty artifact directory.
# CPU targeting stays at rustc's default; RPHP_PGO_TARGET_CPU is an explicit
# override applied equally to the instrumented and final builds.
set -euo pipefail

cd "$(dirname "$0")/.."

RUST_HOST="$(rustc -vV | awk '/^host:/{print $2}')"
RUST_LLVM_MAJOR="$(rustc -vV | awk -F'[:.]' '/^LLVM version:/{gsub(/ /, "", $2); print $2}')"
RUST_SYSROOT="$(rustc --print sysroot)"

# PGO data is compiler-version specific. Retain the exact inputs alongside the
# result, and never merge profiles emitted by Cargo's own build scripts.
PROFDATA_DIR="${RPHP_PGO_DIR:-$(mktemp -d "${TMPDIR:-/tmp}/rphp-candidate-pgo.XXXXXX")}"
mkdir -p -- "$PROFDATA_DIR"
PROFDATA_DIR="$(cd "$PROFDATA_DIR" && pwd)"
if [ -n "$(find "$PROFDATA_DIR" -mindepth 1 -maxdepth 1 -print -quit)" ]; then
    echo "ERROR: RPHP_PGO_DIR must be empty." >&2
    exit 1
fi
export CARGO_TARGET_DIR="$PROFDATA_DIR/target"
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-4}"
BUILD_PROFILES="$PROFDATA_DIR/build-profiles"
TRAINING_PROFILES="$PROFDATA_DIR/training-profiles"
mkdir -p "$BUILD_PROFILES" "$TRAINING_PROFILES" "$PROFDATA_DIR/training-output"
cleanup() {
    ./scripts/cleanup-builds.sh
}
trap cleanup EXIT
./scripts/cleanup-builds.sh

# Find llvm-profdata matching rustc's LLVM major version. A mismatched Apple
# LLVM tool can reject or misread profiles emitted by a Homebrew/rustup rustc.
LLVM_PROFDATA=""
find_llvm_profdata() {
    local candidate version
    for candidate in \
        "$RUST_SYSROOT/lib/rustlib/$RUST_HOST/bin/llvm-profdata" \
        "$(command -v llvm-profdata 2>/dev/null || true)" \
        "/opt/homebrew/opt/llvm/bin/llvm-profdata" \
        "/usr/local/opt/llvm/bin/llvm-profdata" \
        "$(xcrun --find llvm-profdata 2>/dev/null || true)"; do
        [ -n "$candidate" ] && [ -x "$candidate" ] || continue
        version="$("$candidate" --version 2>/dev/null || true)"
        if [[ "$version" == *"version $RUST_LLVM_MAJOR."* ]]; then
            LLVM_PROFDATA="$candidate"
            return
        fi
    done
}
find_llvm_profdata

if [ -z "$LLVM_PROFDATA" ]; then
    echo "ERROR: llvm-profdata matching rustc LLVM $RUST_LLVM_MAJOR was not found." >&2
    echo "Install Rust's llvm-tools component or a matching LLVM package." >&2
    exit 1
fi

GENERATE_FLAGS=(-C llvm-args=--align-all-functions=6)
USE_FLAGS=(-C "llvm-args=--align-all-functions=6 -pgo-warn-missing-function")
if [ -n "${RPHP_PGO_TARGET_CPU:-}" ]; then
    GENERATE_FLAGS+=(-C "target-cpu=$RPHP_PGO_TARGET_CPU")
    USE_FLAGS+=(-C "target-cpu=$RPHP_PGO_TARGET_CPU")
fi
build() {
    local encoded
    printf -v encoded '%s\x1f' "$@"
    # Encoded flags preserve spaces in profile paths and LLVM's option list.
    CARGO_ENCODED_RUSTFLAGS="${encoded%$'\x1f'}" \
        cargo build --locked --profile max-perf --bin rphp
}
RPHP="$CARGO_TARGET_DIR/max-perf/rphp"

echo "=== Maximum-performance RPHP build ==="
echo "rustc:          $(rustc --version)"
echo "llvm-profdata:  $LLVM_PROFDATA"
echo "CPU target:     ${RPHP_PGO_TARGET_CPU:-rustc default}"
echo "Cargo profile:  max-perf (fat LTO, one codegen unit)"
echo "Artifacts:      $PROFDATA_DIR"
echo ""

echo "=== Step 1/3: Instrumented max-perf build ==="
build "${GENERATE_FLAGS[@]}" -C "profile-generate=$BUILD_PROFILES"

echo "=== Step 2/3: Representative training ==="
shopt -s nullglob
WORKLOADS=()
while IFS= read -r workload || [ -n "$workload" ]; do
    [[ -z "$workload" || "$workload" == \#* ]] && continue
    [ -f "$workload" ] || { echo "Missing training program: $workload" >&2; exit 1; }
    WORKLOADS+=("$workload")
done < scripts/pgo-workloads.txt
if [ "${#WORKLOADS[@]}" -eq 0 ]; then
    echo "ERROR: no PGO training workloads found." >&2
    exit 1
fi

for index in "${!WORKLOADS[@]}"; do
    workload="${WORKLOADS[$index]}"
    echo "  $workload"
    LLVM_PROFILE_FILE="$TRAINING_PROFILES/$index-%m-%p.profraw" \
        "$RPHP" "$workload" > "$PROFDATA_DIR/training-output/$index.txt"
    CASE_PROFILES=("$TRAINING_PROFILES/$index-"*.profraw)
    [ "${#CASE_PROFILES[@]}" -gt 0 ] || { echo "Missing profile: $workload" >&2; exit 1; }
done

RAW_PROFILES=("$TRAINING_PROFILES"/*.profraw)
if [ "${#RAW_PROFILES[@]}" -eq 0 ]; then
    echo "ERROR: instrumented workloads produced no PGO profiles." >&2
    exit 1
fi
echo "Training complete (${#WORKLOADS[@]} workloads, ${#RAW_PROFILES[@]} raw profiles)."

echo "=== Step 3/3: Profile-use max-perf build ==="
"$LLVM_PROFDATA" merge \
    -o "$PROFDATA_DIR/merged.profdata" \
    "${RAW_PROFILES[@]}"
build "${USE_FLAGS[@]}" -C "profile-use=$PROFDATA_DIR/merged.profdata"

echo ""
echo "=== Done ==="
echo "PGO-optimized binary: $RPHP"
echo "Training manifest:    scripts/pgo-workloads.txt"
echo "Retained profiles:    $PROFDATA_DIR"
