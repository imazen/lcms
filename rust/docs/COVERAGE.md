# Coverage — which lcms2 C surface the gates exercise

Three measurement axes, three different questions:

| command | what it measures |
|---|---|
| `just coverage-cref` | **C oracle coverage through the parity suite** — clang-instrumented lcms-cref + compiler-rt linked into every `c_parity_*` binary, profraws unioned across binaries. Answers: *which C paths have a gate.* |
| `just coverage-testbed` | two-sided (C + Rust `.so`) coverage driven by upstream `testcms2`. Fixed harness — the parity suite is the only way to move the needle. |
| `cargo llvm-cov -p lcms --summary-only` (just `coverage`) | Rust-side line coverage under `cargo test`. |

Notes:

- The `coverage-cref` build is **measurement only**. The parity contract is
  `cc -O2 -fno-strict-aliasing` (gcc). Under the clang-instrumented oracle a
  few gates report last-ulp codegen diffs (FMA contraction); `--no-fail-fast`
  still runs every binary, and a panicking test under-reports the tail of
  its own sweep — treat the numbers as a floor, not a ceiling.
- `coverage.sh`'s Rust-side tool pick is fixed to the *active* toolchain's
  `llvm-profdata`/`llvm-cov` via `rustc --print sysroot` (the previous
  `find|head -1` grabbed a stale toolchain and died on "raw profile version
  mismatch").

## Baselines

C oracle through **testcms2** (fixed; `just coverage-testbed`, llvm-cov-14):

    TOTAL lines 27534: 66.97%   fns: 83.47%

Notable testbed zeros/lows: `cmscam02.c` 0%, `cmspcs.c` 44%, `cmsalpha.c`
49% (only the alpha-matrix path — `testplugin.c` drives a fraction),
`cmscgats.c` 49%, `cmstypes.c` 57%, `cmsio1.c` 58%.

Rust `lcms` through **testcms2** (`liblcms2_ffi.so`): 66.5% lines / 67.9% fns.
Rust `lcms` through **`cargo test`**: 60.6% lines / 66.6% fns.

## C oracle through the parity suite — BEFORE this campaign

`just coverage-cref` at HEAD~ (the honest "what has a gate" number):

| file | lines | fns | | file | lines | fns |
|---|---|---|---|---|---|---|
| cmsalpha.c | 8.9% | 2.4% | | cmsopt.c | 63.6% | 61.4% |
| cmscam02.c | 98.1% | 100% | | cmspack.c | 29.2% | 37.0% |
| cmscgats.c | 35.6% | 50.5% | | cmspcs.c | 92.4% | 100% |
| cmscnvrt.c | 63.3% | 65.2% | | cmsplugin.c | 53.2% | 77.3% |
| cmserr.c | 37.7% | 58.1% | | cmsps2.c | 77.9% | 90.6% |
| cmsgamma.c | 81.7% | 89.5% | | cmssamp.c | 27.0% | 42.9% |
| cmsgmt.c | 45.3% | 66.7% | | cmssm.c | 91.7% | 100% |
| cmshalf.c | 100% | 100% | | cmstypes.c | 56.0% | 63.1% |
| cmsintrp.c | 96.2% | 91.3% | | cmsvirt.c | 27.1% | 40.6% |
| cmsio0.c | 60.6% | 67.4% | | cmswtpnt.c | 99.3% | 100% |
| cmsio1.c | 41.4% | 52.0% | | cmsxform.c | 42.0% | 33.9% |
| cmslut.c | 82.8% | 93.4% | | **TOTAL** | **52.2%** | **62.7%** |
| cmsmd5.c | 82.4% | 80.0% | | | | |
| cmsmtrx.c | 100% | 100% | | | | |
| cmsnamed.c | 81.7% | 91.3% | | | | |

## Gates added this campaign

| gate | C surface exercised | oracle |
|---|---|---|
| `c_parity_alpha` | `cmsalpha.c`: full `_cmsGetFormatterAlpha` 6x6 matrix (8/16/16SE/HLF/FLT/DBL), `ComputeIncrementsForChunky`/`Planar`, nExtra>1 loop, doswap/swapfirst order branches; also pulls pack alpha formatters + xform `HandleExtraChannels` | byte-exact `cmsDoTransform` C-vs-Rust on sRGB→sRGB `COPY_ALPHA`; both-fail is parity on rejected combos |
| `c_parity_ps2` mutex | `cmsps2.c` hex-wrap column | same byte-compare, now deterministic (see below) |

### Findings the new gate produced

1. **`alpha.rs::from16to16` missing `CHANGE_ENDIAN`** — C's 16↔16SE alpha
   copy byte-swaps; the port copied straight through. Caught on
   `alpha 16->16SE chunky px0`: C `[232,24]` vs Rust `[24,232]`. Fixed.
2. **`fromFLTto{8,16,16SE}` computed `n*255`/`n*65535` in f64** where C
   uses f32 (`n * 255.0F`) — a rounding-class divergence on the saturate
   path. Fixed (compute in f32, widen for qs).
3. **Flaky `c_parity_ps2`**: C's `_cmsPSActualColumn` is a file-scope
   static shared by all `cmsGetPostScript*` calls; the four tests ran in
   parallel threads and interleaved emissions shifted each other's hex
   line-wraps. Now serialized by a test-file `Mutex`.
4. **Pinned residual (int→float quadrant)**: on `u8→FLT` the C oracle's
   float-eval produces sparse ±1-ulp color values (smallest input: u8=15 →
   C `0x3d70f0f2` vs Rust `0x3d70f0f1`) — the known float-eval residual in
   cmsxform/cmsopt, distinct from the alpha machinery. Alpha lanes are
   byte-exact everywhere else; in that quadrant the test bounds them to
   ≤2 ulp. All other directions (FLT→FLT, FLT→u8, DBL→FLT, u16→FLT, …) are
   enforced byte-exact.
5. C's `cmsCreateTransform` rejects some format combos (e.g. planar
   half-float paths, 16SE+extra2+doswap→FLT); the gate treats C-NULL ↔
   Rust-None as parity on the error path.

## ICC spec references

Downloaded 2026-11-07 to `/mnt/v/icc-spec/` (not committed — binary docs):

- `ICC.1-2022-05.pdf` — **ICC.1:2022-05 (v4.4), the current v4 spec** —
  authority for tag types, encodings, and MPE semantics the gates check.
- `ICC.1-2001-04.pdf` — v2.1 spec (legacy v2 profiles).
- `ICC.2-2019.pdf` — iccMAX (v5); outside lcms2's scope.

## C oracle through the parity suite — AFTER (2026-11-07, `just coverage-cref`)

| file | lines before → after | | file | lines before → after |
|---|---|---|---|---|
| **cmsalpha.c** | **8.87% → 86.54%** | | cmsplugin.c | 53.15% → 53.15% |
| **cmspack.c** | **29.19% → 41.39%** | | cmsps2.c | 77.93% → 77.93% |
| **cmsxform.c** | **41.96% → 43.07%** | | cmssamp.c | 27.02% → 27.02% |
| cmsgamma.c | 81.74% → 81.96% | | cmssm.c | 91.70% → 91.70% |
| cmscnvrt.c | 63.29% → 63.29% | | cmstypes.c | 56.00% → 56.00% |
| cmscam02.c | 98.05% → 98.05% | | cmsvirt.c | 27.13% → 27.13% |
| cmscgats.c | 35.56% → 35.56% | | cmswtpnt.c | 99.33% → 99.33% |
| cmserr.c | 37.70% → 37.70% | | cmsio0.c | 60.63% → 60.63% |
| cmsgmt.c | 45.27% → 45.27% | | cmsio1.c | 41.39% → 41.39% |
| cmshalf.c | 100% → 100% | | cmslut.c | 82.80% → 82.80% |
| cmsintrp.c | 96.21% → 96.21% | | cmsmd5.c | 82.35% → 82.35% |
| cmsnamed.c | 81.67% → 81.67% | | cmsmtrx.c | 100% → 100% |
| cmsopt.c | 63.57% → 63.48% | | cmspcs.c | 92.41% → 92.41% |
| **TOTAL** | **57.02% → 59.60% lines, 62.71% → 66.48% fns** | | | |

(cmsopt 63.57→63.48 is within measurement noise from clang-vs-gcc
sweep-tail loss; cmspack +12.2 lines comes free with the alpha transforms
driving pack's alpha/float formatters.)

## Remaining ungated C surface (next gates, in order of missed lines)

- **cmstypes.c** 1506 missed lines — MPE tag-type handlers, error paths in
  tag readers (malformed payloads), uncommon types
- **cmspack.c** ~1400 missed — exotic pixel-format formatters (5–15ch, XYZ,
  premultiplied, swap-first combos beyond the alpha matrix)
- **cmscgats.c** 1160 missed — IT8 writer (`SaveToMem`), property/patch/
  data-format machinery, multi-table (`SetTable`/`GetTable`) paths
- **cmsvirt.c** 540 missed — profile builders beyond sRGB/Lab/XYZ:
  BCHSW abstract, linearization/ink-limiting devicelinks, OkLab
- **cmsio0.c** 535 missed — profile-open error paths (truncated, bad magic,
  corrupt tag table), save/serialize edge cases
- **cmsio1.c** 337 missed — LUT readers for profile classes/intents the
  testbed doesn't reach, `cmsIsIntentSupported`/`cmsGetSupportedIntents`
- **cmssamp.c** 235 missed — `cmsDetectBlackPoint`/`DestinationBlackPoint`/
  `DetectTAC` across profile classes and intents
- **cmserr.c / cmsplugin.c** ~500 missed — error-handler registration,
  plugin slot machinery, context dup
- **cmsxform.c** ~460 missed — multiprofile/proofing transforms, gamut-check
  workers, `cmsChangeBuffersFormat`, `cmsDoTransform*Stride`, alarm codes
