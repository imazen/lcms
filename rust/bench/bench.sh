#!/usr/bin/env bash
# Reproducible bench driver: builds the C oracle + the Rust FFI lib, links each
# benchmark .c against both, runs them in the upstream testbed dir (which holds
# the test*.icc profiles), and prints a side-by-side MPix/s table.
#
#   rust/bench/bench.sh            # all benchmarks
#   rust/bench/bench.sh b16 b32    # a subset
#
# The C oracle is the unmodified lcms2.20 sources under reference/lcms2 built
# with the contract flags (cc -O2 -fno-strict-aliasing) — the same flags the
# lcms-cref differential oracle uses.
set -euo pipefail

RUST=$(cd "$(dirname "$0")/.." && pwd)                 # .../rust
LCMS2=$(cd "$RUST/../reference/lcms2" && pwd)          # .../reference/lcms2
OUT="$RUST/bench/out"
RUSTLIB="$RUST/target/release"
CFLAGS="-O2 -fno-strict-aliasing -I$LCMS2/include"
mkdir -p "$OUT"

cd "$RUST"
cargo build --release -p lcms2-ffi

# C oracle static lib — rebuild only when a source changed.
ORACLE="$OUT/liblcms2_c.a"
newest_src=""
if [ -f "$ORACLE" ]; then
    newest_src=$(find "$LCMS2/src" -name '*.c' -newer "$ORACLE" 2>/dev/null | head -1 || true)
fi
if [ ! -f "$ORACLE" ] || [ -n "$newest_src" ]; then
    objs=()
    for f in "$LCMS2"/src/*.c; do
        o="$OUT/$(basename "$f" .c).o"
        cc $CFLAGS -c "$f" -o "$o"
        objs+=("$o")
    done
    ar rcs "$ORACLE" "${objs[@]}"
fi

# Link one bench twice: `_rust` (liblcms2_ffi.so) and `_c` (the oracle).
link() {
    local b=$1
    cc $CFLAGS "$RUST/bench/$b.c" \
        -L"$RUSTLIB" -Wl,-rpath,"$RUSTLIB" -llcms2_ffi -lm \
        -o "$OUT/${b}_rust"
    cc $CFLAGS "$RUST/bench/$b.c" "$ORACLE" -lm -o "$OUT/${b}_c"
}

run() {
    (cd "$LCMS2/testbed" && "$1")
}

BENCHES=("$@")
if [ ${#BENCHES[@]} -eq 0 ]; then
    # curv_bench is excluded: it takes profile paths (argv[1], argv[2]) rather
    # than self-contained profiles.
    BENCHES=(b16 b32 curv2 curv3)
fi

printf '%-8s  %12s  %12s  %8s\n' bench rust c 'rust/c'
for b in "${BENCHES[@]}"; do
    link "$b"
    r=$(run "$OUT/${b}_rust")
    c=$(run "$OUT/${b}_c")
    # each line prints "<label>: <MPix/s>" — pull the numbers
    rn=$(echo "$r" | grep -oE '[0-9.]+(e\+?[0-9]+)? MPix/s' | grep -oE '[0-9.]+(e\+?[0-9]+)?')
    cn=$(echo "$c" | grep -oE '[0-9.]+(e\+?[0-9]+)? MPix/s' | grep -oE '[0-9.]+(e\+?[0-9]+)?')
    if [ -n "$rn" ] && [ -n "$cn" ]; then
        ratio=$(awk -v r="$rn" -v c="$cn" 'BEGIN{printf "%.2f", r/c}')
    else
        ratio="?"
    fi
    printf '%-8s  %12s  %12s  %8s\n' "$b" "${rn:-ERR}" "${cn:-ERR}" "$ratio"
done

# Differential benches write a dump file per side — the dump must match
# bit-for-bit; report diff counts.
DIFFS=(diff_c8 diff_clut diff_curv diff_cache)
printf '\n%s\n' "differentials (run both, diff dumps):"
for d in "${DIFFS[@]}"; do
    link "$d"
    ro=$(run "$OUT/${d}_rust" > "$OUT/${d}_rust.txt")
    co=$(run "$OUT/${d}_c" > "$OUT/${d}_c.txt")
    nd=$(diff "$OUT/${d}_c.txt" "$OUT/${d}_rust.txt" | grep -c '^<' || true)
    printf '  %-12s diffs: %s\n' "$d" "$nd"
done
