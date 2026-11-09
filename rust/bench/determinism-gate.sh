#!/usr/bin/env bash
# determinism-gate — CI parity+determinism gate for the math providers.
#
# Trust model:
#   1. math-sys: Rust's C shim and the lcms-cref oracle both call the
#      LOCAL system libm -> C==R bit-exact is expected on EVERY cell.
#      This proves the port's code paths are oracle-faithful.
#   2. math-libm: same code paths, but the 21 transcendental entry
#      points swap to the pure-Rust `libm` crate. Cells exercising them
#      diverge from the oracle by ulp amounts — that XX set must be
#      confined to TRANSCENDENTAL_CELLS. Any other cell going XX means
#      the provider swap leaked into a non-math path -> real bug.
#   3. R-hashes under math-libm are platform-deterministic -> CI matrix
#      compares DIGEST/R-hashes across runners for drift.
#
# Usage: bench/determinism-gate.sh [--goldens DIR]
#   --goldens DIR  write $DIR/<provider>.cells (name\trh\tch) for the
#                  cross-platform compare job.
set -euo pipefail
cd "$(dirname "$0")/.."

TRANSCENDENTAL="pcs deltaE cam02 adapt estg1 estg2 estg3 estg4 estg5 estg6 estg7 estg8 param"
GOLDENS=""
[ "${1:-}" = "--goldens" ] && GOLDENS="$2" && mkdir -p "$GOLDENS"

run_provider () {
    local name="$1"; shift
    cargo test -p lcms --test determinism "$@" -- --nocapture 2>&1 \
        | grep -E '^(CELL|DIGEST)'
}

check () { # $1 provider $2 cells-output
    local prov="$1" cells="$2"
    local fails
    fails=$(echo "$cells" | awk -F'|' '/^CELL/{if($4==0)print substr($1,6)}')
    if [ "$prov" = "math-sys" ]; then
        if [ -n "$fails" ]; then
            echo "FAIL math-sys: oracle-parity cells diverged:" >&2
            echo "$fails" >&2; return 1
        fi
        echo "PASS math-sys: all $(echo "$cells" | grep -c '^CELL') cells C==R bit-exact"
    else
        local bad=""
        for c in $fails; do
            case " $TRANSCENDENTAL " in *" $c "*) ;; *) bad="$bad $c" ;; esac
        done
        if [ -n "$bad" ]; then
            echo "FAIL $prov: non-transcendental cells diverged:$bad" >&2
            return 1
        fi
        local n=$(echo "$fails" | grep -c . || true)
        echo "PASS $prov: divergence confined to ${n:-0} transcendental cells"
        echo "$cells" | awk -F'|' '/^CELL/{if($4==0)print "   XX " substr($1,6) "  dmax=" $5}'
    fi
}

SYS=$(run_provider math-sys --no-default-features --features math-sys)
LIBM=$(run_provider math-libm --no-default-features --features math-libm)

check math-sys "$SYS"
check math-libm "$LIBM"

echo "$SYS"  | grep DIGEST | sed 's/^/sys  /'
echo "$LIBM" | grep DIGEST | sed 's/^/libm /'

if [ -n "$GOLDENS" ]; then
    echo "$SYS"  | awk -F'|' '/^CELL/{print substr($1,6)"\t"$2"\t"$3}' > "$GOLDENS/sys.cells"
    echo "$LIBM" | awk -F'|' '/^CELL/{print substr($1,6)"\t"$2"\t"$3}' > "$GOLDENS/libm.cells"
    echo "wrote $GOLDENS/{sys,libm}.cells"
fi
