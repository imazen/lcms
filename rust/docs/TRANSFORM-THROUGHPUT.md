# Packed transform throughput

Run from `rust/`. The comparison uses the public `lcms::api::Plan` with
reused scratch, stock C Little CMS 2.20, and moxcms 0.9.2 with its default
SIMD features and CICP transfer substitution disabled.

```sh
just bench-cms
just bench-cms --group=lut-cmyk --format=csv
LCMS_BENCH_COMPARE_OUTPUTS=1 just bench-cms --group=p3-rgbf32
```

The recipe builds C with `LCMS_CREF_MATH=sys`, using system libm. The Rust
engine keeps its default pure-Rust math provider. Differential tests
use the repository's shared-math oracle for bit-for-bit checks.

The profiles and transform plans are created outside the timed callbacks.
The callbacks reuse input and output buffers and apply a transform one row
at a time. Each RGB arm processes a 3840 × 2160 frame. Each CMYK arm
processes a 4096-pixel row. The primary comparison uses relative
colorimetric intent. RGBA routes copy alpha. Integer cases include cached
and uncached measurements, and CMYK includes a separate black-point
compensation measurement for both Little CMS engines. moxcms 0.9.2 has
no corresponding black-point compensation option. Float CMYK inputs
represent the same ink amounts as percentages in Little CMS and normalized
fractions in moxcms. The optional output check compares the first row
against the stock C engine outside timing.

The default fixtures generate Display P3 and Adobe RGB matrix profiles and
the 17³ RGB and 9⁴ CMYK Lab PCS LUTs. The repeating RGB sample sequences
reproduce the input structure in [issue 3](https://github.com/imazen/lcms/issues/3).
The P3 noise case uses an independent deterministic sequence of 24-bit
float samples. Set `LCMS_BENCH_PROFILE_DIR` to use the exact matrix
profile files from that report. The profile filenames and SHA-256 values
for the run below are:

| File | SHA-256 |
|---|---|
| `DisplayP3Compat-v4.icc` | `231752984cd4a5278e1b8d2390fe496767d4511fc81f54e1a5c69ae9ab4c42b5` |
| `AdobeCompat-v2.icc` | `60fb2adecacf82132db0b1c09b303316f3bbd9e2823e7ba096d01627d12d57c9` |

## Measured routes

The November 23, 2026 measurements use an AMD Ryzen 9 7900X pinned to
logical CPU 6, Rust 1.99.1 and GCC 15.2.0. The Rust release profile uses
thin LTO and one codegen unit, with CPU features selected at runtime.
C uses `-O2 -fno-strict-aliasing` at the pinned 2.20 revision
`7717fce0d85447ef3b2df0cca2e99207c15eeecd`. The baseline is `c85eb7cd`
and the measured implementation is `09094e56`.
The arms are interleaved within each group by zenbench 0.1.11. RGB groups
use six rounds and CMYK groups use twenty. The timings are exploratory
mean latencies from separate builds and processes. The baseline and
candidate use isolated Cargo target directories. Both benchmark binaries
import `pow@GLIBC_2.29`.

| Source → sRGB | Format | Rust before | Rust after | C Little CMS 2.20 | moxcms 0.9.2 |
|---|---|---:|---:|---:|---:|
| Display P3 | RGB8 | 36.7 ms | 28.7 ms | 52.4 ms | 8.8 ms |
| Display P3 | RGBA8 | 64.3 ms | 30.9 ms | 84.2 ms | 9.5 ms |
| Display P3 | RGB16 | 87.7 ms | 86.7 ms | 107.2 ms | 31.5 ms |
| Display P3 | RGBF32 | 2,887.3 ms | 2,419.0 ms | 1,046.1 ms | 31.0 ms |
| Display P3 | RGBAF32 | 2,903.2 ms | 2,422.3 ms | 1,055.7 ms | 31.5 ms |
| Display P3, 24-bit noise | RGBF32 | 3,140.0 ms | 2,656.8 ms | 1,061.7 ms | 24.4 ms |
| Adobe RGB | RGB8 | 36.6 ms | 27.2 ms | 51.9 ms | 8.8 ms |
| Adobe RGB | RGBF32 | 2,898.0 ms | 2,374.6 ms | 1,030.2 ms | 28.3 ms |
| RGB 17³ CLUT | RGB8 | 132.5 ms | 72.3 ms | 119.6 ms | 22.0 ms |
| RGB 17³ CLUT | RGB16 | 86.7 ms | 86.9 ms | 107.3 ms | 24.8 ms |
| RGB 17³ CLUT | RGBF32 | 3,127.6 ms | 2,311.5 ms | 1,409.0 ms | 48.5 ms |
| CMYK 9⁴ CLUT | CMYK8 → RGB8 | 98.4 µs | 92.0 µs | 109.2 µs | 19.6 µs |
| CMYK 9⁴ CLUT | CMYK16 → RGB16 | 101.0 µs | 85.6 µs | 98.0 µs | 19.2 µs |
| CMYK 9⁴ CLUT | CMYKF32 → RGBF32 | 1,501.5 µs | 1,162.7 µs | 769.4 µs | 34.5 µs |

The table uses the default cache setting and no black-point compensation.
[The timing data](../../benchmarks/cms-throughput_2026-11-23.csv) also records
the explicit cache and black-point compensation arms.

## Exact packed RGB8 and RGB16

The November 25, 2026 benchmark interleaves `15e9947d`, `3dc4a616`,
stock C Little CMS 2.20 and moxcms 0.9.2 for 12 rounds per case on a
Ryzen 9 7900X pinned to logical CPU 6. It uses Rust 1.99.1 and
zenbench 0.1.11 with thread CPU time enabled in the paired harness.
All arms process a 3840 × 2160 frame through packed row APIs with
reusable output. The noise cases use the upper 8 or 16 bits of the same
deterministic mixer used for the float noise fixture. The Display P3
profile is the issue 3 fixture identified above.

The table reports mean thread CPU milliseconds per frame with default
caching. Wall time was affected by scheduling on the shared host. The
[timing data](../../benchmarks/packed-rgb-throughput_2026-11-25.csv)
records both clocks and the no-cache arms. The benchmark processes
peaked at 1.34 GiB RSS under `run-heavy`, including input and output
buffers for the paired engines.

| Input profile | Format and samples | Rust before | Rust after | C Little CMS 2.20 | moxcms 0.9.2 |
|---|---|---:|---:|---:|---:|
| Display P3 | RGB8, repeating | 17.9 | 10.9 | 52.1 | 8.7 |
| Display P3 | RGB8, noise | 41.6 | 12.1 | 55.7 | 8.7 |
| Display P3 | RGB16, repeating | 86.6 | 67.4 | 107.0 | 31.7 |
| Display P3 | RGB16, noise | 133.0 | 71.3 | 137.3 | 33.5 |
| RGB 17³ CLUT | RGB8, repeating | 73.5 | 61.7 | 120.3 | 21.9 |
| RGB 17³ CLUT | RGB8, noise | 109.1 | 69.9 | 161.8 | 26.6 |
| RGB 17³ CLUT | RGB16, repeating | 86.5 | 67.0 | 107.0 | 24.8 |
| RGB 17³ CLUT | RGB16, noise | 133.1 | 71.4 | 136.9 | 30.6 |

- The packed RGB8 matrix shaper adds its existing integer contribution
  tables in SIMD lanes and applies the original clamp and encode tables.
- The prepared RGB CLUTs load complete nodes and order the tetrahedron
  corners by carrying each grid step with its fraction. They preserve
  the wrapping fixed-point sum and rounding. RGB8 prepares the input
  coordinates and quantizes output in SIMD. Its cache compares packed
  byte keys and keeps the already encoded result for repeated pixels.
- RGB16 uses exact shifts and adds for the default 33³ grid. Other
  RGB16 grid sizes use the general evaluator. A 33³ direct CLUT adds
  71,874 bytes of table storage after sharing the original table with
  the fallback. RGB8 prelinearization can add up to 293,640 bytes of
  prepared tables. The stripped x86-64 conversion consumer grew from
  1,421,528 to 1,430,392 bytes.

C differential checks matched all 16,777,216 RGB8 triplets for each of
the Display P3 and RGB LUT fixtures with default caching and with
caching disabled. The strided differential tests cover mixed RGB16
endpoints, tied fractions, three grid resolutions, and scalar fallback.
The shared-math determinism manifest matched all 51 C cells with
digest `a0e0bf7ad99ca73d`. With system libm the differing cells were
PCS, Delta E, and CIECAM02. All transform cells matched in both runs.

## Implementation

- Use the packed RGB8, BGR8, RGBA8 and BGRA8 matrix-shaper loop when the
  stock optimizer adds its output-format hint. The loop uses the same
  fixed-point shaper tables and channel order as the normal formatters.
- Copy trailing 8-bit and float alpha with packed row loops. The color
  kernels write only the color samples.
- Use the packed RGB8 tetrahedral and prelinearized CLUT loops with both
  cache modes. The final corner load clamps to the end of the whole
  table, including when a 3-channel node has no fourth sample to load.
- Fuse stock packed CMYK8 and CMYK16 input with the existing 4D kernel
  and RGB, RGBA, or CMYK output. Preserve the last-pixel cache and
  destination alpha where applicable.
- Move CPU detection and its scalar-test override out of interpolation's
  per-pixel work. The prelinearized RGB8 SIMD loop orders the corners
  before applying its wrapping fixed-point sum.
- Evaluate packed RGB and CMYK float pixels using the original float
  pipeline, with small buffers and one packing decision per call. The
  3×3 matrix evaluator preserves the double-precision operation order.
  Profile curves and CMYK ink scaling retain their original precision.

Two experiments tested sharing corner-weight calculations more broadly.
Using one geometry calculation across both 4D tetrahedra raised CMYK8
and CMYK16 row times past 120 µs. Sharing the RGB8 calculation with the
general 3D kernel slowed uncached RGB16 CLUT by about 1.4% in repeated
runs. The final loops use the original 3D and 4D SIMD kernels for those
cases and retain the new packed dispatch.

The existing stripped x86-64 linked conversion example decreased from
1,395,360 to 1,390,048 bytes using its `opt-level=2`, thin-LTO profile.
The moxcms benchmark dependency is development-only.

## Float math provider

A separate Ryzen 9 9950X3D run pinned to logical CPU 4 measured the P3
noise frame at 2.25 s with Rust's default `math-libm` feature and
813.9 ms with the existing `math-rust` feature. Stock C on the latter
run took 924.8 ms and moxcms took 19.3 ms. A cycle profile of the
shared-math benchmark process attributed 88% of sampled user cycles to
`libm::pow`.

```sh
LCMS_CREF_MATH=sys cargo bench --locked -p lcms --no-default-features \
  --features little-cms-api,math-rust --bench cms_shootout -- --group=p3-noise-rgbf32
```

On that Linux host, `math-rust` matched C with system libm across all
51 determinism cells. PCS, Delta E and CIECAM02 differed from the
portable shared-math baseline. All transform cells matched both references
in that manifest.

The timing comparison also measures different numerical contracts.
moxcms uses table-driven curves in its default float matrix-shaper path.
Stock Little CMS evaluates the linked parametric curves directly, including
out-of-gamut values. On the first P3 RGBF32 row, all 11,520 samples from
moxcms differed from stock C, with a maximum absolute delta of 1.19326.
The high-entropy P3 row differed at 11,518 of 11,520 samples. Little CMS
also documents a separate Fast Float plug-in that uses specialized
interpolation and SIMD. This benchmark uses the stock engine.
[moxcms transform options](https://docs.rs/crate/moxcms/0.9.2/source/src/transform.rs)
[Little CMS Fast Float documentation](https://littlecms.com/plugin/)

## Exact curve and matrix-shaper optimizations

The Little CMS inverse type 4 curve computes a fixed cutoff from its
parameters. The Rust port now prepares the cutoff and reciprocal gamma
when it builds the curve. The evaluator checks the live parameters and
callback so direct edits through the Little CMS API remain effective.
The shared-math determinism manifest still matches all 51 C cells with
digest `a0e0bf7ad99ca73d`.

For frozen packed RGB8, BGR8, RGBA8 and BGRA8 matrix-shaper plans, Little
CMS's wrapping Q14 products can be computed for every input code at
link time. This adds 12 KiB of contribution tables per eligible plan.
The row kernel adds the existing integer products, applies the same
bias, shift and clamp, and reads the byte already stored in each
quantized output entry. The mutable Little CMS API evaluates the live
tables directly. The public `Plan` matched that path on all 16,777,216
RGB8 triplets for the Display P3 ICC fixture and the same serialized
sRGB destination. Focused C differential tests cover the four layouts,
strided rows and alpha copying with P3 and Adobe RGB input.

Zenbench 0.1.11 measurements below use a Ryzen 9 7900X pinned to
logical CPU 6 and Rust 1.99.1, with the standalone Cargo release
profile and runtime SIMD selection. Setup runs outside timing.
The float measurements are mean ranges from two eight-round processes
per revision in base, candidate, candidate, base order, with isolated
build outputs. The RGB8 arms are interleaved for twelve rounds in one
process. These are exploratory results. RGB8 coefficients of variation
were 29% and 34%, and part of the float measurement was noisy.
Peak RSS was 0.14 GiB for the float runs and 0.17 GiB for the RGB8 run.

| Display P3 ICC → sRGB | Work per round | Previous mean | Exact optimized mean |
|---|---:|---:|---:|
| RGBF32, deterministic 24-bit samples | 3,840 × 270 | 472.7–475.5 ms | 351.4–365.1 ms |
| RGB8, repeating issue 3 samples | 3,840 × 2,160 | 48.2 ms | 40.7 ms |

## MageMath and Zenpixels 0.5 experiments

The float prototype runs the two linked ICC powers on contiguous row
buffers with MageMath 0.1.1's unit-domain `Powf`. It uses the actual
profile TRC coefficients and retains the double-precision matrix
accumulation and operation order. Inputs outside [0, 1] use the scalar
pipeline for the whole pixel. An encode argument above 1 uses the
original scalar curve with the prototype's intermediate value.
Both profiles are the same serialized ICCs as in the exact float run.

| RGBF32 at 3,840 × 270 | Mean | Different float samples | Maximum absolute delta | Different after u8 rounding | Different after u16 rounding |
|---|---:|---:|---:|---:|---:|
| Exact with cutoff cache | 351.4–365.1 ms | reference | reference | reference | reference |
| MageMath Highp | 34.4–39.7 ms | 1,506,542 / 3,110,400 | 5.96 × 10⁻⁷ | 16 | 3,406 |
| MageMath Midp | 25.8–32.6 ms | 1,823,580 / 3,110,400 | 8.94 × 10⁻⁷ | 19 | 4,624 |

The rounding columns clamp and quantize the float results for
comparison. A second input mixed in 64,800 negative or above-one
pixels, all of which took the exact fallback and matched bit-for-bit.
The changed float and rounded samples keep MageMath an experimental
approximation for this transform.

Zenpixels 0.5 selects its native u8 kernel at row-plan construction.
Its exact kernel fuses the sRGB decode lookup, float matrix and encode
with byte packing. Its optional Q14 kernel stores the three
premultiplied matrix columns and an encode lookup. These paths cover
full-range sRGB-transfer RGB spaces with matching white and clipping
semantics. The exact reference is Zenpixels' scalar conversion.
Re-running the release test at `c4626b22` verified scalar/SIMD agreement
for each kernel over the full RGB8 cube on all four tested routes.
For P3 → sRGB with rounding, Q14 differed from the exact result by one
code in 410,084 of 50,331,648 channels (0.81%).

The contribution-table idea transfers to Little CMS while retaining
its profile-dependent shapers, fixed-point arithmetic and quantization.
An experimental byte-packed copy of the three output tables brought
the standalone RGB8 row kernel to 21.4–21.9 ms in two exploratory 4K
runs. The initial production kernel reused the original output table.
[Zenpixels row selection](https://github.com/imazen/zenpixels/blob/c4626b22eff6be78c30c4dfaeea082d709e68e96/zenpixels-convert/src/strip.rs#L711)
[Zenpixels exhaustive witness](https://github.com/imazen/zenpixels/blob/c4626b22eff6be78c30c4dfaeea082d709e68e96/zenpixels-convert/tests/audit_05_regressions.rs#L1455)
[MageMath profiles](https://docs.rs/magemath/0.1.1/magemath/powf/index.html)

## Sampled float curves and packed byte output

`PlanOptions::approximate_float_curves(true)` prepares sampled input and
output curves for eligible `RGB_FLT` and `RGBA_FLT` matrix-shaper plans.
The grid uses 1,024 intervals per binary exponent. Each table contains
samples of the original curve, and the evaluator interpolates within
the same analytic branch. The matrix still uses the original f64
arithmetic. The existing pipeline handles any pixel with a color
component outside [0, 1], including NaN and infinity. For a normalized
input the linked matrix can still yield extended-range output. The
sampled encoder preserves it and evaluates its original curve outside
the table. Both ICC specification modes, explicitly unoptimized
plans, and CLUT pipelines retain their corresponding evaluation paths.

For exact 8-bit output, the prepared matrix-shaper plan stores the
original quantized output codes in byte tables. Identical output
curves share a 16 KiB table. Three distinct curves use 48 KiB.
RGBA and BGRA copy alpha in the same row loop when requested.
Before and after matched on the full 16,777,216-triplet RGB8 cube.
Stock C differential tests also passed for all four packed layouts,
strided rows, alpha copying and separate destination curves.

The November 24 measurements use the same serialized Display P3 and
Adobe RGB input fixtures, serialized sRGB output, 4K frame size,
Ryzen 9 7900X pinned to logical CPU 6 and Rust 1.99.1. Float timings
use six interleaved rounds on `61129f85` with the default pure Rust
math provider. The RGB8 and RGBA8 comparison links `f6d7fe63` and
`61129f85` into the same benchmark and interleaves 24 rounds per
process, repeated twice. These are exploratory means with substantial
variance in the fastest arms. Peak RSS was 1.46 GiB for the five-arm
float benchmark and 0.30 GiB for the paired integer benchmark.
[Timing data](../../benchmarks/cms-matrix_2026-11-24.csv)

| ICC → sRGB at 4K | Exact Rust lcms | Sampled Rust lcms | C Little CMS 2.20 | moxcms default | moxcms extended range |
|---|---:|---:|---:|---:|---:|
| Display P3 RGBF32, 24-bit noise | 2,831 ms | 165 ms | 1,548 ms | 33 ms | 541 ms |
| Adobe RGB RGBF32 | 2,591 ms | 146 ms | 1,505 ms | 46 ms | 486 ms |
| Display P3 RGBAF32, alpha copied | 2,655 ms | 115 ms | 1,542 ms | 40 ms | 500 ms |

| Exact Display P3 ICC → sRGB at 4K | Before | Packed byte output |
|---|---:|---:|
| RGB8 | 29.83–30.93 ms | 24.87–25.07 ms |
| RGBA8, alpha copied | 34.03–34.24 ms | 28.26–28.31 ms |

The first row of each float route matched stock C in exact mode. The
sampled path changed zero codes after clipping and rounding each of
those rows to u8. For P3 noise, 2,207 of the 11,520 C reference samples
were outside [0, 1]. The maximum absolute error in that row was
`4.77e-7` with sampled curves. moxcms 0.9.2 had maximum absolute
error `2.81` with defaults and `2.56e-4` with extended range enabled.
Over the reference samples within [0, 1], the corresponding maxima
were `2.31e-7`, `1.67e-3` and `9.78e-5`.

| First-row output vs stock C | P3 RGBF32 noise | Adobe RGBF32 | P3 RGBAF32 |
|---|---:|---:|---:|
| Sampled Rust lcms, differing u16 codes | 21 / 11,520 | 15 / 11,520 | 16 / 15,360 |
| moxcms extended range, differing u16 codes | 3,721 / 11,520 | 621 / 11,520 | 3,725 / 15,360 |

The rounding comparison describes the float result. Integer LCMS
transforms use their own exact shapers and packing. The test suite
also checks exact extended-input fallback and alpha preservation on
strided rows with a non-vector-width tail.

```sh
LCMS_BENCH_COMPARE_OUTPUTS=1 just bench-cms --group=p3-noise-rgbf32
LCMS_BENCH_COMPARE_OUTPUTS=1 just bench-cms --group=adobe-rgbf32
```

[moxcms 0.9.2 transform options](https://docs.rs/moxcms/0.9.2/moxcms/struct.TransformOptions.html)

## Exact packed float device-link CLUTs

The packed float worker handles a stock pipeline whose only stage is
a u16 CLUT, as well as plans with precomputed CLUT optimization data.
It selects the tetrahedral or four-dimensional kernel once per call.
CMYK input still uses ink percentages, and the RGB output stays
normalized. RGBA alpha is copied by the existing extra-channel path.
Foreign interpolation and formatters keep their normal dispatch.

On a Ryzen 9 7950X pinned to logical CPU 6, zenbench 0.1.11 interleaved
18 rounds per arm using deterministic random samples in 64 rows of
4,096 pixels. Both arms used the same 9-node-per-axis device link, Rust
1.99.1, thin LTO and runtime SIMD selection. These exploratory timings
had substantial variance. Peak benchmark RSS was 0.26 GiB.

| 262,144 pixels | Before | Fused |
|---|---:|---:|
| RGBF32 → RGBF32 | 17.86 ms | 8.79 ms |
| CMYKF32 → RGBF32 | 17.32 ms | 13.46 ms |

The C parity tests cover both stock and preoptimized tables, strided
rows and packed alpha, with AVX2 enabled and disabled. The multi-stage
ICC routes in issue 3 continue to use their linked PCS and curve
operations. [Timing data](../../benchmarks/float-device-link_2026-11-24.csv)
