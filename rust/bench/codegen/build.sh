#!/usr/bin/env bash
set -euo pipefail

# Run under run-heavy and set TMPDIR to a disk-backed directory.
# Usage: build.sh <gcc|apple|wasi> <artifact-directory>
kind=$1
out=$2
here=$(cd -- "$(dirname -- "$0")" && pwd)
oracle=$(cd -- "$here/../../../reference/lcms2" && pwd)
mkdir -p -- "$out"
out=$(cd -- "$out" && pwd)

case "$kind" in
    gcc)
        cc=${CC:-gcc}
        flags=(-flto -Wl,--gc-sections -s)
        target=$(rustc -vV | sed -n 's/^host: //p')
        extension=
        ;;
    apple)
        cc=${CC:-clang}
        flags=(-flto=thin -Wl,-dead_strip -Wl,-x -Wl,-S)
        target=aarch64-apple-darwin
        extension=
        ;;
    wasi)
        cc="${WASI_SDK_PATH:?set WASI_SDK_PATH}/bin/clang"
        flags=(-flto=thin -Wl,--gc-sections -Wl,--strip-all)
        target=wasm32-wasip1
        extension=.wasm
        ;;
    *) printf 'unknown toolchain %s\n' "$kind" >&2; exit 2 ;;
esac

{
    rustc -vV
    "$cc" --version
    printf 'C flags: -O2 -fno-strict-aliasing -ffunction-sections -fdata-sections %s\n' "${flags[*]}"
    printf 'Rust target: %s\n' "$target"
} > "$out/toolchain.txt"

export CARGO_TARGET_DIR="$out/target"
for use in inspect convert; do
    defs=()
    features=()
    if [[ $use == inspect ]]; then
        defs=(-DINSPECT)
        features=(--features inspect)
    fi
    "$cc" -O2 -fno-strict-aliasing -ffunction-sections -fdata-sections \
        "${flags[@]}" "${defs[@]}" -I"$oracle/include" \
        "$here/probe.c" "$oracle"/src/*.c -lm -o "$out/c-$use$extension"

    for mode in compat spec-default; do
        enabled=("${features[@]}")
        if [[ $mode == spec-default ]]; then
            enabled+=(--features spec-default)
        fi
        cargo build --locked --release --target "$target" --manifest-path "$here/Cargo.toml" "${enabled[@]}"
        cp -- "$CARGO_TARGET_DIR/$target/release/lcms-codegen-consumer$extension" \
            "$out/rust-$mode-$use$extension"
    done
done
