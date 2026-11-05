# lcms

A pure-Rust port of [Little CMS](https://github.com/mm2/Little-CMS) **2.20** —
algorithm-for-algorithm, verified **bit-exact** against the C library on the
ported surface, `#![forbid(unsafe_code)]`, no C in the product path.

The C tree lives in the `reference/lcms2` submodule (upstream, unmodified) and
is compiled by the test-only `lcms-cref` crate as the **differential oracle**:
every ported function is compared against the real exported C symbol (or a
same-source shim for `static`/`cmsINLINE` code) over dense and edge-case sweeps.

## Status

Early port. Ported and byte-gated today:

| C file | Rust module | gate |
|---|---|---|
| `cmsmtrx.c` | `lcms::mtrx` | `tests/c_parity_mtrx.rs` |
| `cmspcs.c` | `lcms::pcs` | `tests/c_parity_pcs.rs` |
| `cmswtpnt.c` | `lcms::wtpnt` | `tests/c_parity_wtpnt.rs` |
| `cmshalf.c` | `lcms::half` | `tests/c_parity_half.rs` (exhaustive) |
| `lcms2_internal.h` + `cmsplugin.c` fixed-point helpers | `lcms::fixed` | `tests/c_parity_fixed.rs` |
| signature enums (`lcms2.h`) | `lcms::signatures` | generated, `tools/gen_signatures.py` |

Still unported: `cmsio0` (profile I/O), `cmstypes` (tag handlers),
`cmslut`/`cmsintrp`/`cmsgamma` (pipelines, interpolation, tone curves),
`cmsopt`/`cmsxform`/`cmspack`/`cmsalpha` (transforms), `cmscnvrt`/`cmssm`/
`cmsgmt` (linking, gamut), `cmsnamed`/`cmscgats`/`cmsio1`/`cmsmd5`/`cmsps2`/
`cmssamp`/`cmscam02`/`cmsvirt` — see `PORTING.md` for the order and why.

## Testing on a fresh box

```bash
git clone --recurse-submodules https://github.com/imazen/lcms && cd lcms
cd rust && cargo test --workspace
```

The C oracle is built by cargo (`lcms-cref/build.rs` runs `cc` over
`reference/lcms2/src/*.c`) — needs a C compiler, nothing else.

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
