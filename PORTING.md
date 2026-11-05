# PORTING.md — the C → Rust map

Which C file each Rust module ports, and how to run the gate that proves it.
`rust/docs/WORKING-ON-THIS.md` is the working-rules doc; read it before your
first change.

## Layout

```
reference/lcms2/   Little-CMS 2.20 (git SUBMODULE, upstream mm2/Little-CMS,
                   tag lcms2.20, commit 7717fce) — READ-ONLY reference + oracle
rust/
  crates/lcms/     the product crate — pure safe Rust, forbid(unsafe_code)
  crates/lcms-cref/test-only FFI oracle: build.rs cc-compiles the whole
                   C tree into liblcms2cref.a; shims/ wraps statics+inlines
  tools/           generators (gen_signatures.py, gen_half_tables.py)
  docs/            WORKING-ON-THIS.md and campaign docs
```

## The oracle

lcms2 is 26 flat C files — no cmake needed. `lcms-cref/build.rs` compiles
them all with `cc -O2 -fno-strict-aliasing`, so every non-`static` symbol
(`cmsXxx` and `_cmsXxx`) is directly callable from tests. `static` and
`cmsINLINE` code is reached through `shims/inline_oracle.c`, a TU that includes
`lcms2_internal.h` and wraps them — same source, same flags, one indirection.

**`-fno-strict-aliasing` is load-bearing**: `cmshalf.c`, `cmsio0.c` and the
`_cmsQuickFloor` union trick rely on type-punning. Keep the flag.

## Port order (dependency order)

Done: types/signatures → fixed-point helpers → `mtrx` → `pcs` → `wtpnt` → `half`.

| order | C file | Rust module | notes |
|---|---|---|---|
| 1 | `cmsmtrx.c` | `mtrx` | done |
| 2 | `cmspcs.c` | `pcs` | done |
| 3 | `cmswtpnt.c` | `wtpnt` | done |
| 4 | `cmshalf.c` | `half` + `half_tables` (generated) | done, exhaustive gate |
| 5 | `lcms2_internal.h` inlines + `cmsplugin.c` helpers | `fixed`, `context`, `err` | done (minimal Context; slots grow) |
| 6 | `cmsintrp.c` | `intrp` | done — all kernels, dims 1–15 |
| 7 | `cmsgamma.c` | `gamma` | done — parametric/segmented/tabulated + smooth/reverse/join |
| 8 | `cmslut.c` | `lut` | in progress — stage/pipeline machinery + MPE elements |
| 9 | `cmsio0.c` | `io` | profile read/write + tag dir + endian IO |
| 10 | `cmstypes.c` | `tagtypes` | all tag type handlers (~6.3k C lines) |
| 11 | `cmsmd5.c` | `md5` | done (computeID waits on cmsio0) |
| 12 | `cmsnamed.c`, `cmsio1.c` | `named`, `io1` | named colors, profile sequence |
| 13 | `cmspack.c` | `pack` | pixel formatters (~4k lines) |
| 14 | `cmsalpha.c` | `alpha` | alpha channel copy |
| 15 | `cmsxform.c` | `xform` | transforms, cache, proofing |
| 16 | `cmsopt.c` | `opt` | optimizer — reproduces C's *decisions* |
| 17 | `cmscnvrt.c` | `cnvrt` | profile linking, intents, BPC |
| 18 | `cmsvirt.c` | `virt` | sRGB/Lab/XYZ/abstract/BCHSW virtual profiles |
| 19 | `cmscam02.c` | `cam02` | CIECAM02 |
| 20 | `cmssm.c`, `cmsgmt.c` | `sm`, `gmt` | gamut boundary descriptor, gamut check |
| 21 | `cmscgats.c` | `cgats` | CGATS/IT8 parser (~3.3k) |
| 22 | `cmssamp.c`, `cmsps2.c` | `samp`, `ps2` | sampling, PostScript CSA/CRD |

Later/optional: `plugins/fast_float`, `plugins/threaded` (upstream plugin
packs — separate scope decision), `utils/*` CLIs.

## Bit-exactness notes (the rules that have already cost a bug)

- **GCC merges adjacent `cos(x)`/`sin(x)` into `sincos`**, which can differ in
  the last ulp from two calls. Rust `f64::sin_cos()` lowers to the same libm
  call — use it wherever C calls both on one argument (found in `cmsLCh2Lab`,
  2-ulp divergence caught by `c_parity_pcs`).
- `pow(x, 0.5)` → `sqrtsd`, `pow(x, 7.0)` stays a libm call, `pow(const, n)`
  constant-folds — Rust's `powf`/`sqrt` lowerings match; verify with the gate.
- `_cmsQuickFloor` is the double2fixmagic bit trick, not `floor()` — port the
  bits (`fixed::quick_floor`), valid for |v| < 32767 like C.
- C `int`/`unsigned` arithmetic: use `i32`/`u32` with `wrapping_*` where C
  relies on wraparound; `/` truncates toward zero in both languages.
- C `(int)double` casts are UB out of range; Rust `as` saturates — identical
  inside C's defined domain, and note any place C actually escapes.
- `double` = `f64` throughout; `cmsFloat32Number` = `f32` (formatters, MPE).
- Asserts (`_cmsAssert`) compile out in release C — port as debug_assert or
  faithful runtime checks depending on whether the C path can reach them.

## Evidence tiers (same as zenav1-svt)

1. Differential vs the real exported C symbol (`lcms-cref`) — the standard.
2. Byte-identity of whole outputs (profile writes, transform pixels) vs C.
3. Same-source shim around `static`/`cmsINLINE` code.
4. Hand-derived vectors — only where a symbol is genuinely unreachable.

Prefer exhaustive gates where the domain allows it (`half` checks all 65,536
inputs; `fixed` sweeps full integer ranges).
