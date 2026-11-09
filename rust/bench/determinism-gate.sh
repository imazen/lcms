#!/usr/bin/env bash
# determinism-gate — CI parity+determinism gate.
#
# Model (post-interposition): the lcms-cref oracle links against the
# SAME libm-crate implementations the Rust port uses — the oracle's
# `pow`/`sin`/... resolve to Rust-exported `extern "C"` symbols (the
# executable's own symbols interpose the dynamic libm). So C==R is
# bit-exact BY CONSTRUCTION on every platform, all providers are pure
# Rust, and `forbid(unsafe_code)` is unconditional.
#
# Legs:
#   1. default (math-libm + interposed oracle): ALL cells must be C==R
#      — parity under canonical math.
#   2. LCMS_CREF_MATH=sys (system-libm oracle): divergence must stay
#      confined to the transcendental cells — documents the libm delta.
#
# Usage: bench/determinism-gate.sh [--goldens DIR]
set -euo pipefail
cd "$(dirname "$0")/.."

TRANSCENDENTAL="pcs deltaE cam02 adapt estg1 estg2 estg3 estg4 estg5 estg6 estg7 estg8 param"
GOLDENS=""
[ "${1:-}" = "--goldens" ] && GOLDENS="$2" && mkdir -p "$GOLDENS"
TRIPLE=$(rustc -vV | awk '/host:/{print $2}')

run_manifest () {
    cargo test -p lcms --test determinism "$@" -- --nocapture 2>&1 \
        | grep -E '^(CELL|DIGEST)'
}

check () { # $1 label $2 cells $3 mode (all|confined)
    local label="$1" cells="$2" mode="$3"
    local fails
    fails=$(echo "$cells" | awk -F'|' '/^CELL/{if($4==0)print substr($1,6)}')
    if [ "$mode" = "all" ]; then
        if [ -n "$fails" ]; then
            echo "FAIL $label: cells diverged:" >&2; echo "$fails" >&2
            return 1
        fi
        echo "PASS $label: all $(echo "$cells" | grep -c '^CELL') cells C==R bit-exact"
    else
        local bad=""
        for c in $fails; do
            case " $TRANSCENDENTAL " in *" $c "*) ;; *) bad="$bad $c" ;; esac
        done
        if [ -n "$bad" ]; then
            echo "FAIL $label: non-transcendental cells diverged:$bad" >&2
            return 1
        fi
        echo "PASS $label: divergence confined to $(echo "$fails" | grep -c . || true) transcendental cells"
        echo "$cells" | awk -F'|' '/^CELL/{if($4==0)print "   XX " substr($1,6) "  dmax=" $5}'
    fi
}

ABI=$(run_manifest)
SYS=$(LCMS_CREF_MATH=sys run_manifest)

check "abi(libm)" "$ABI" all
check "sys-libm" "$SYS" confined

echo "$ABI" | grep DIGEST | sed 's/^/abi /'
echo "$SYS" | grep DIGEST | sed 's/^/sys /'

if [ -n "$GOLDENS" ]; then
    echo "$ABI" | awk -F'|' '/^CELL/{print substr($1,6)"\t"$2"\t"$3}' \
        > "$GOLDENS/abi.$TRIPLE.cells"
    echo "$SYS" | awk -F'|' '/^CELL/{print substr($1,6)"\t"$2"\t"$3}' \
        > "$GOLDENS/sys.$TRIPLE.cells"
    echo "wrote $GOLDENS/{abi,sys}.$TRIPLE.cells"
fi
