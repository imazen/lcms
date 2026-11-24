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
runs. The production kernel reuses the original output table and
avoids another 48 KiB per eligible plan.
[Zenpixels row selection](https://github.com/imazen/zenpixels/blob/c4626b22eff6be78c30c4dfaeea082d709e68e96/zenpixels-convert/src/strip.rs#L711)
[Zenpixels exhaustive witness](https://github.com/imazen/zenpixels/blob/c4626b22eff6be78c30c4dfaeea082d709e68e96/zenpixels-convert/tests/audit_05_regressions.rs#L1455)
[MageMath profiles](https://docs.rs/magemath/0.1.1/magemath/powf/index.html)
