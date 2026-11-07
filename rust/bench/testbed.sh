#!/usr/bin/env bash
# Build + run the upstream testbed (testcms2) against both the C oracle and the
# Rust lib in a scratch dir (the testbed writes *.icc profiles as check
# side-effects — never run it inside reference/lcms2/testbed where the
# committed profiles live). Reports both pass counts.
set -euo pipefail

RUST=$(cd "$(dirname "$0")/.." && pwd)
LCMS2=$(cd "$RUST/../reference/lcms2" && pwd)
OUT="$RUST/bench/out"
RUN="$OUT/testbed-run"
RUSTLIB="$RUST/target/release"
CFLAGS="-O2 -fno-strict-aliasing -I$LCMS2/include -I$LCMS2/src"
SRCS="$LCMS2/testbed/testcms2.c $LCMS2/testbed/testplugin.c $LCMS2/testbed/zoo_icc.c"
mkdir -p "$OUT" "$RUN"

cd "$RUST"
cargo build --release -p lcms2-ffi

if [ ! -f "$OUT/liblcms2_c.a" ]; then
    echo "testbed.sh: $OUT/liblcms2_c.a missing — run bench.sh first (it builds the oracle)" >&2
    exit 1
fi
ORACLE="$OUT/liblcms2_c.a"

cc $CFLAGS $SRCS "$ORACLE" -lm -o "$OUT/testcms2_c"
cc $CFLAGS $SRCS -L"$RUSTLIB" -Wl,-rpath,"$RUSTLIB" -llcms2_ffi -lm -o "$OUT/testcms2_rust"

# the testbed opens *.icc relative to cwd — copy the committed corpus in.
cp -n "$LCMS2"/testbed/*.icc "$RUN/" 2>/dev/null || true

cd "$RUN"
C=$("$OUT/testcms2_c" 2>/dev/null || true)
R=$("$OUT/testcms2_rust" 2>/dev/null || true)
echo "C testbed:    $(echo "$C" | grep -c 'Ok\.') checks passed, $(echo "$C" | grep -c 'FAIL') failures"
echo "Rust testbed: $(echo "$R" | grep -c 'Ok\.') checks passed, $(echo "$R" | grep -c 'FAIL') failures"
