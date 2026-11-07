#!/usr/bin/env bash
# Testbed-driven coverage on BOTH sides — the honest port-coverage comparison.
# `cargo test` coverage underweights the formatter/profile-builder paths, which
# are exercised by testcms2 (linked against the .so, not the rlib). This builds
#   1. the C oracle + testcms2 with -fprofile-instr-generate -fcoverage-mapping
#   2. liblcms2_ffi.so with -C instrument-coverage + testcms2 linked to it
# and reports the llvm-cov line coverage each testbed run reaches.
#
# Needs: clang + llvm-cov-14/llvm-profdata-14 for the C side; the rustc llvm
# tools (rust-toolchain component llvm-tools) for the .so side.
set -euo pipefail

RUST=$(cd "$(dirname "$0")/.." && pwd)
LCMS2=$(cd "$RUST/../reference/lcms2" && pwd)
OUT="$RUST/bench/out/coverage"
CFLAGS="-O2 -fno-strict-aliasing -I$LCMS2/include"
mkdir -p "$OUT"
cd "$OUT"
cp -n "$LCMS2"/testbed/*.icc . 2>/dev/null || true

# --- C oracle + testcms2, instrumented ---
for f in "$LCMS2"/src/*.c; do
    clang -fprofile-instr-generate -fcoverage-mapping $CFLAGS -c "$f" \
        -o "c_$(basename "$f" .c).o"
done
clang -fprofile-instr-generate -fcoverage-mapping $CFLAGS -I"$LCMS2/src" -c \
    "$LCMS2"/testbed/{testcms2.c,testplugin.c,zoo_icc.c}
clang -fprofile-instr-generate -fcoverage-mapping c_*.o testcms2.o testplugin.o zoo_icc.o -lm -o testcms2_cov
LLVM_PROFILE_FILE=c.profraw ./testcms2_cov >/dev/null 2>&1 || true
llvm-profdata-14 merge c.profraw -o c.profdata
echo "=== C oracle coverage through testcms2 (lcms2 lines) ==="
llvm-cov-14 report testcms2_cov -instr-profile=c.profdata \
    | grep -E "src/cms|TOTAL" | awk '{print $1, "lines:"$8, " fns:"$4}' | tail -30

# --- Rust liblcms2_ffi.so, instrumented ---
cd "$RUST"
RUSTFLAGS="-C instrument-coverage" cargo build --release -p lcms2-ffi 2>&1 | tail -1
cc $CFLAGS -I"$LCMS2/src" "$LCMS2"/testbed/{testcms2.c,testplugin.c,zoo_icc.c} \
    -L"$RUST/target/release" -Wl,-rpath,"$RUST/target/release" -llcms2_ffi -lm \
    -o "$OUT/testcms2_covrust"
cd "$OUT"
LLVM_PROFILE_FILE=rust.profraw ./testcms2_covrust >/dev/null 2>&1 || true
PD=$(find ~/.rustup/toolchains -name llvm-profdata | head -1)
CV=$(find ~/.rustup/toolchains -name llvm-cov | head -1)
"$PD" merge rust.profraw -o rust.profdata
echo
echo "=== Rust lcms coverage through testcms2 (lcms lines) ==="
"$CV" report "$RUST/target/release/liblcms2_ffi.so" -instr-profile=rust.profdata \
    | grep -E "lcms/src|lcms2-ffi|TOTAL" | awk '{print $1, $NF}' | tail -35
