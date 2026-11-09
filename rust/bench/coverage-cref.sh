#!/usr/bin/env bash
# C-oracle coverage THROUGH THE PARITY SUITE — the honest answer to
# "which lcms2 C paths do the c_parity_* gates exercise?"
#
# coverage.sh measures the testbed (testcms2), which is fixed. This script
# instead rebuilds lcms-cref's C oracle with clang source-based coverage
# (-fprofile-instr-generate -fcoverage-mapping), links the compiler-rt
# profile runtime into every c_parity_* test binary, runs the whole suite,
# and unions the per-binary reports into one per-file C coverage table.
#
# IMPORTANT: this is MEASUREMENT ONLY, not the parity contract. The oracle
# contract is `cc -O2 -fno-strict-aliasing` (gcc). Under the clang build a
# few gates diverge by last-ulp codegen differences (FMA contraction) and
# report failures; --no-fail-fast still runs every binary, and the counts
# under-report the tail of whichever tests panic mid-sweep. The gate itself
# is always the gcc build (`cargo test`).
#
# Needs: clang-14, llvm-cov-14, llvm-profdata-14 (coverage.sh's C-side deps).
set -euo pipefail

RUST=$(cd "$(dirname "$0")/.." && pwd)
OUT="$RUST/bench/out/coverage-cref"
PROFILE_RT=/usr/lib/llvm-14/lib/clang/14.0.0/lib/linux/libclang_rt.profile-x86_64.a
mkdir -p "$OUT/raw"
rm -f "$OUT/raw"/*.profraw "$OUT/all.profdata"

# Separate target dir: the normal (gcc, uninstrumented) test build is
# untouched, and lcms-cref's build.rs stamp forces a fresh C compile here.
export CARGO_TARGET_DIR="$RUST/target/covcref"
export CC=clang
export CFLAGS="-fprofile-instr-generate -fcoverage-mapping"
# -l pulls the profile runtime archive; -Wl,-u,__llvm_profile_runtime is the
# -u flag clang's driver adds at link time — without it the instrumented
# objects carry counters but nothing registers the profraw writer.
export RUSTFLAGS="-L $(dirname "$PROFILE_RT") -l clang_rt.profile-x86_64 -C link-args=-Wl,-u,__llvm_profile_runtime"
export LLVM_PROFILE_FILE="$OUT/raw/%p.profraw"

cd "$RUST"
cargo test -p lcms --features math-sys --no-fail-fast 2>&1 | grep -E 'test result: FAILED|^error' || true

cd "$OUT"
llvm-profdata-14 merge raw/*.profraw -o all.profdata

# Union across every c_parity_* binary: each links only the C objects it
# referenced, so the union is needed for whole-tree coverage. llvm-cov
# aggregates identical functions across -object arguments.
mapfile -t BINS < <(ls "$RUST/target/covcref/debug/deps/"c_parity_* 2>/dev/null | grep -v '\.d$')
ARGS=()
for b in "${BINS[@]:1}"; do ARGS+=(-object "$b"); done

echo
echo "=== C oracle coverage through the parity suite (lcms2 lines) ==="
llvm-cov-14 report "${BINS[0]}" "${ARGS[@]}" -instr-profile=all.profdata \
    | grep -E 'lcms2/src|lcms2_internal|TOTAL'
