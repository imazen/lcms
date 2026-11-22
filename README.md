# lcms

A Rust port of [Little CMS](https://github.com/mm2/Little-CMS) 2.20 for
reading ICC profiles and converting colors. The default Rust API tracks
the behavior exercised by the pinned C oracle. The opt-in ICC.1 modes
apply the implemented specification rules and report unsupported exports.

See the [Rust quickstart](rust/README.md#quickstart) for a runnable RGB
to Lab example and the [conformance audit](rust/docs/CONFORMANCE.md)
for the differences between modes. The core crate forbids unsafe Rust.
The C ABI and compiled differential oracle live in separate crates.

## Status

The Rust port covers profile I/O, curves and interpolation, pixel formatting,
linking, black-point and gamut operations, and colorimetry. The
[porting map](PORTING.md) shows the differential gates for each module.
The [conformance audit](rust/docs/CONFORMANCE.md) records coverage limits
and the opt-in ICC.1 behavior.

## Performance

Historical results from `just bench` (best of 10, `release` build).
Use the recorded benchmark commands below to measure your target.

| path | Rust | C oracle | rust/c |
|---|---|---|---|
| 16-bit CLUT | ~100 MPix/s | ~67 MPix/s | ~1.5× |
| 8-bit CMYK | ~45 MPix/s | ~35 MPix/s | ~1.3× |
| 8-bit SAME MatShaper | ~980 MPix/s | ~260 MPix/s | ~3.8× |
| 8-bit curves | ~950 MPix/s | ~230 MPix/s | ~4× |
| 16-bit curves | ~1.6 kMPix/s | ~250 MPix/s | ~6× |
| 32-bit float CLUT | ~6.7 MPix/s | ~6.4 MPix/s | ~1.05× |

The 32-bit float paths sit at ~0.94–1.05× of C — roughly parity.

`rust/bench/*.c` are the benchmark sources, linked once against
`liblcms2_ffi.so` (the Rust build) and once against `liblcms2_c.a` (the oracle
built from `reference/lcms2/src/*.c` with `cc -O2 -fno-strict-aliasing` — the
same flags the `lcms-cref` differential oracle uses). `just bench` builds
both, runs each benchmark in the upstream testbed dir, and prints the
side-by-side table plus the differential-dump counts.

## Testing on a fresh box

```bash
git clone --recurse-submodules https://github.com/imazen/lcms && cd lcms
cd rust && cargo test --workspace
```

The C oracle is built by cargo (`lcms-cref/build.rs` runs `cc` over
`reference/lcms2/src/*.c`) — needs a C compiler, nothing else.

The `just` recipes cover the rest:

```bash
just verify     # the full gate: cargo tests + testbed + api-doc check + diffs
just test       # the workspace cargo tests (the c_parity_* oracle gates)
just testbed    # build+run upstream testcms2 against both; pass/fail counts
just bench      # C-vs-Rust MPix/s table + differential-dump counts
just c-oracle   # just the oracle static lib (bench/out/liblcms2_c.a)
just coverage   # source-based coverage of the lcms crate (cargo-llvm-cov)
just coverage-testbed  # two-sided coverage through testcms2 (C vs Rust)
just api-doc    # regenerate docs/public-api/*.txt snapshots
```

Coverage is measured on **both** sides through the upstream testbed
(`just coverage-testbed` builds each lib instrumented and runs `testcms2`
against it):

| | lines | functions |
|---|---|---|
| C oracle (`liblcms2_c`) | ~74% | ~83% |
| Rust (`liblcms2_ffi`) | ~67% | ~68% |

The gap is the foreign-plugin checks `testplugin.c` exercises (the C
testbed runs 161 checks, the Rust build runs the 144 it supports). Neither
side approaches 100% — lcms2 carries error paths and uncommon formats the
suite doesn't reach. `just coverage` (`cargo llvm-cov` on the `cargo test`
parity gates) is the deeper kernel-level number: `intrp` 91%, `pcs` 95%,
`mtrx`/`half`/`cam02` ~100%.

## License

Dual-licensed **AGPL-3.0-only OR a commercial license** — the standard Imazen
"zen" model: [LICENSE-AGPL3](LICENSE-AGPL3) / [LICENSE-COMMERCIAL](LICENSE-COMMERCIAL).

The `reference/lcms2` submodule keeps its upstream MIT licensing; the Rust
port is a derivative work of that MIT-licensed C source, and upstream
attribution is preserved.

## Acknowledgments

- [Little CMS](https://github.com/mm2/Little-CMS) (Martí Maria Saguer) — the
  C library this port is built on
- The [zenav1-svt](https://github.com/imazen/zenav1-svt) port — the
  differential-oracle methodology this project follows
