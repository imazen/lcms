# Linked consumer code size

These measurements link complete, stripped programs against the default
Rust crate or the pinned Little CMS 2.20 C source. The programs open a
supplied ICC file. The inspector prints its version, color space, PCS,
and description. The converter reads packed RGB8, builds a relative
colorimetric transform to sRGB, renders the input, and prints a pixel
checksum. The workloads are in [the consumer sources](../bench/codegen/).

The input was DisplayP3-v4 from the
[Compact ICC Profiles collection](https://github.com/saucecontrol/Compact-ICC-Profiles).
Its SHA-256 was
`cb51de38e482ee974c0c76b9689e16aad04bad16e226fed2f30c842d15ff3a3d`.
The converter input was a 16 × 16 × 16 RGB lattice with all channels
stepping by 17, red outermost and blue innermost. Its SHA-256 was
`2163a4097f6fb9c65610491bbbc71b12e579df1572e686a0e1e2513efaeeb980`.

## Results

Executable bytes and code-section bytes below include the retained runtime.
Native builds dynamically link system libraries. The WASI modules include
the code retained from their respective C or Rust runtimes. These figures
describe the two linked applications.

| Target | Consumer | Implementation | File bytes | Code bytes | gzip bytes |
|---|---|---|---:|---:|---:|
| x86-64 Linux | Inspect | C | 153,960 | 99,055 |  |
| x86-64 Linux | Inspect | Rust Lcms2Exact | 707,240 | 547,337 |  |
| x86-64 Linux | Inspect | Rust SpecDefault | 707,240 | 547,337 |  |
| x86-64 Linux | Convert | C | 283,848 | 190,767 |  |
| x86-64 Linux | Convert | Rust Lcms2Exact | 4,255,792 | 3,634,793 |  |
| x86-64 Linux | Convert | Rust SpecDefault | 4,255,808 | 3,634,809 |  |
| Apple ARM64 | Inspect | C | 205,496 | 156,032 |  |
| Apple ARM64 | Inspect | Rust Lcms2Exact | 624,928 | 430,124 |  |
| Apple ARM64 | Inspect | Rust SpecDefault | 624,928 | 430,124 |  |
| Apple ARM64 | Convert | C | 357,768 | 273,748 |  |
| Apple ARM64 | Convert | Rust Lcms2Exact | 1,702,464 | 1,330,476 |  |
| Apple ARM64 | Convert | Rust SpecDefault | 1,702,464 | 1,330,476 |  |
| WASI Preview 1 | Inspect | C | 219,351 | 192,517 | 63,451 |
| WASI Preview 1 | Inspect | Rust Lcms2Exact | 324,710 | 294,731 | 116,355 |
| WASI Preview 1 | Inspect | Rust SpecDefault | 324,710 | 294,731 | 116,356 |
| WASI Preview 1 | Convert | C | 348,294 | 301,787 | 103,170 |
| WASI Preview 1 | Convert | Rust Lcms2Exact | 1,346,759 | 1,270,454 | 288,360 |
| WASI Preview 1 | Convert | Rust SpecDefault | 1,346,761 | 1,270,454 | 288,361 |

Adding conversion brings in transform selection, interpolation, and pixel
execution. The x86-64 symbol-attribution build precedes the last diagnostic change
and has a 3,634,553-byte `.text` section, 240 bytes smaller than the
final stripped build. Its symbol table attributes 840,124 bytes to the scalar `xform16_eval_loop` family
and 761,277 bytes each to its v3 and v4 families. Together these are
2,362,678 bytes of retained code, or 65.0% of `.text`. Runtime format and
CPU dispatch retain those implementations even in the RGB8-only consumer.

The inspectors agreed in all nine executions. All three platforms produced
checksum `ff03e6a7` with C and Rust Lcms2Exact, and `b8dece2c` with Rust
SpecDefault. These are same-input execution checks. The C differential
gates separately compare the pixels under the compatibility contract.

## Reproduce

Use `just codegen gcc <output-dir>` on x86-64 Linux, `just codegen apple
<output-dir>` on an ARM Mac, and `just codegen wasi <output-dir>` with
`WASI_SDK_PATH` pointing to a WASI SDK installation. Use an absolute output
directory outside the source checkout. The script records compiler and target
information and creates a separate executable for each API path and mode.

Rust 1.99.1 used optimization level 2, ThinLTO, one codegen unit, the default
`math-libm` feature, and stripped symbols. The resolved archmage checkout
was `c8c7f5e223fa96187988e09ba4168fb24318105f`. C used the oracle source
`7717fce0d85447ef3b2df0cca2e99207c15eeecd`, optimization level 2,
`-fno-strict-aliasing`, section-level dead stripping, and LTO. The compilers
were GCC 11.4.0 with full LTO, Apple Clang 21.0.0 with ThinLTO, and WASI SDK
34 Clang 23.1.0 with ThinLTO. The ARM Mac build adapted the absolute paths
for local Rust dependencies in an isolated source copy.

Use `size -A` for ELF `.text`, `size -m` for Mach-O `__text`,
`wasm-tools objdump` for the WebAssembly code section, and
`gzip -n -9 -c` for compressed WASI size. For symbol attribution, build
the x86-64 Rust compatibility converter with `cargo rustc` and the
build script's release options, appending `-- -C strip=none`. Aggregate
unique executable symbol address/size pairs from `nm -S --demangle`.
The WASI programs ran under Wasmtime 40.0.1 with the input
fixtures supplied through the runtime's directory mapping.
[WASI SDK](https://github.com/WebAssembly/wasi-sdk)

## Magetypes and Archmage 0.9.33

The released dependencies replaced the local 0.9.31-beta.2 checkout.
These Lcms2Exact builds use the same Display P3 input, API calls, and
release options as above. For each Linux and WASI pair, both builds
used the same Linux host and Rust 1.100.0 with LLVM 23.1.5. The Apple
ARM64 comparison uses the recorded baseline above and Rust 1.99.1
with LLVM 23.1.1 on both sides.

| Target | Consumer | File bytes, beta → 0.9.33 | Code bytes, beta → 0.9.33 |
|---|---|---:|---:|
| x86-64 Linux | Inspect | 707,120 → 707,136 | 546,646 → 546,630 |
| x86-64 Linux | Convert | 4,251,160 → 4,251,192 | 3,628,310 → 3,628,310 |
| Apple ARM64 | Inspect | 624,928 → 624,928 | 430,124 → 430,124 |
| Apple ARM64 | Convert | 1,702,464 → 1,702,464 | 1,330,476 → 1,330,540 |
| WASI Preview 1 | Inspect | 333,200 → 333,232 | 302,131 → 302,131 |
| WASI Preview 1 | Convert | 1,350,181 → 1,350,229 | 1,272,842 → 1,272,842 |

Gzipped WASI changed from 119,309 to 119,338 bytes for inspection
and from 292,037 to 292,029 bytes for conversion. The inspectors agreed
across all six Rust builds. All six Rust converters returned
`ff03e6a7`. The Apple ARM64 rerun also matched the original C and
SpecDefault checksums. The Rust 1.89 workspace check passed with all
targets and both compatibility API features enabled.

### Where to reduce linked size

1. Start with formatter specialization in the
   [integer transform loop](../crates/lcms/src/xform.rs). Keep direct
   calls for format pairs that show a throughput benefit and share a
   loop with formatter callbacks for the rest. The measurements below
   show the effect of sharing the full loop on all three targets.
2. Evaluate CPU tiers for the resulting loop. In an earlier trial that
   changed only its attribute from `#[archmage::autoversion]` to
   `#[archmage::attune(dispatch)]`, the converter fell from
   4,251,192 to 3,259,976 bytes (21.0%). Its `.text` fell from
   3,628,310 to 2,754,646 bytes. The trial reproduced the fixture
   checksum and passed 14 C API and transform-matrix parity tests.
   Measure packed RGB8, strided and planar conversions, and a CLUT
   workload before changing the production loop. Keep explicit wider
   tiers for vector kernels where they improve throughput. Evaluate the
   floating-point loop separately.
   [Archmage 0.9.33 dispatch guide](https://github.com/imazen/archmage/blob/3143371be7eea1f00cac047470cef4d27333d49a/docs/ATTUNE.md)
3. Use token-first Magetypes operations where a kernel does meaningful
   vector work. Keep scalar tails equivalent in rounding, endpoint,
   and NaN behavior. Evaluate each Magemath approximation against its
   domain and precision contract, including C parity for Lcms2Exact.
   Dispatch and formatter changes can be measured without changing
   those arithmetic contracts.
   [Magemath migration guide](https://github.com/imazen/archmage/blob/3143371be7eea1f00cac047470cef4d27333d49a/docs/MAGEMATH-MIGRATION.md)

## Formatter specialization and CPU tiers

These scratch builds use source revision `d8cee076`, the same inputs
and optimization flags as above, and function names for attribution.
Linux and WASI used Rust 1.100.0. Apple ARM64 used Rust 1.99.1.
The stock C comparators used GCC 15.2 on Linux, WASI SDK 34 on WASI,
and the recorded Apple Clang 21 result above.

| Converter code bytes | x86-64 Linux | Apple ARM64 | WASI Preview 1 |
|---|---:|---:|---:|
| Stock Little CMS 2.20 | 194,079 | 273,748 | 301,787 |
| Rust baseline | 3,628,310 | 1,330,476 | 1,272,842 |
| Remove `autoversion` from integer loop | 1,868,038 | 1,330,540 | 1,274,791 |
| Share integer loop across formatter pairs | 1,038,022 | 662,764 | 564,195 |
| Apply both changes | 1,020,070 | 662,764 | 564,160 |

The shared-loop trial replaces the body of `xform16_run` with a single
call to `xform16_eval_loop` using its existing unpack and pack function
pointers. Its evaluator remains generic. The other trial removes only
the integer loop's `#[archmage::autoversion]` attribute. Both leave
the floating-point loop and separately dispatched SIMD kernels in place.
Every Rust variant reproduced the stock C fixture checksum `ff03e6a7`.
The shared-loop variant also passed all 14 x86-64 tests in
`api_c_parity` and `c_parity_xform_matrix`. Throughput and the full
parity suite remain to be measured for these changes.

On x86-64, GNU `nm` attributes 840,124 bytes to the baseline
integer loop's scalar family, and 761,277 bytes each to its v3 and v4
families. After sharing formatters, there are 12 evaluator instances
per tier, with 25,772 bytes of loop bodies across all three tiers.
The full executable's code benefit from removing integer-loop CPU
dispatch falls from 1,760,272 to 17,952 bytes after sharing formatters.
On Apple ARM64, `nm` finds 792 integer loop bodies before sharing and
12 after. Removing `autoversion` alone saves no bytes on Apple ARM64
or this WASI target.

On WASI, Twiggy 0.8.0 counts 792 `xform16_eval_loop` bodies
(553,938 bytes) and 12 `xform16_run` bodies (162,102 bytes). Together
they account for 56.3% of the code section. These are shallow sizes
from `twiggy top`. The dominator tree attributes 727,925 retained
bytes to `TransformPlan::xform16`. Retaining paths run from
`Plan::apply` and internal gamut-check and black-point transforms
through the common transform entry point. An RGB8 call therefore still
links the runtime format selection and the evaluator specializations.
After sharing formatters, Twiggy still attributes 105,099 retained bytes
to `Plan::build` and 88,198 to `cms_read_tag`. The named `core`,
`std`, and `alloc` bodies total 104,946 bytes before and after the
change. Retained sizes can overlap.

For attribution, build with `cargo rustc --locked --release --target
<target> --manifest-path bench/codegen/Cargo.toml -- -C strip=none`
from the Rust workspace. Keep the Wasm name section with
`wasm-tools strip`, then use `twiggy top -f csv`, `twiggy monos -g`,
`twiggy dominators`, and `twiggy paths --regex` on the named copy.
Measure code bytes with `wasm-tools objdump`, `size -A`, or
`size -m`. Use `nm -S --defined-only --demangle` on ELF and
count unique address and size pairs to avoid aliases. Wasmtime 40.0.1
ran all WASI variants. The Rust inspector and converter modules both
require SIMD instructions under this toolchain, including the variant
with integer-loop `autoversion` removed. WASI validates SIMD support
when it loads the module.

### Little CMS 2.20 dispatch

Stock C chooses the unpack, evaluation, and pack functions at runtime.
It reuses the transform loop across formatter pairs by calling those
functions through pointers. The x86-64 stock executable contained no
`cpuid` instructions. The separately registered `fast_float` plugin
does have runtime SIMD selection. Its 8-bit matrix-shaper optimization
checks SSE2 through CPUID before choosing the SSE2 kernel.
An otherwise matched C build with the plugin registered at startup
contained that CPUID check and the SSE2 kernel. Its `.text` was 244,511
bytes, 50,432 bytes larger than stock C, and its fixture checksum was
`f1d714d3`.
[Stock C transform loop](https://github.com/mm2/Little-CMS/blob/7717fce0d85447ef3b2df0cca2e99207c15eeecd/src/cmsxform.c#L518-L554)
[C plugin SSE2 check](https://github.com/mm2/Little-CMS/blob/7717fce0d85447ef3b2df0cca2e99207c15eeecd/plugins/fast_float/src/fast_8_matsh_sse.c#L302-L345)

## x86-64 formatter throughput

The scratch builds use source revision `d2fc005e`, Rust 1.100.0,
LLVM/LLD 23.1.5, and the converter's release settings above. A Ryzen 9
9950X3D ran the benchmark inside a verified one-CPU Linux cpuset on
the 96 MiB L3 cache die. The workload uses the public `Plan::build`,
`apply`, and `apply_strided` APIs in Lcms2Exact mode, with relative
colorimetric intent. Each run reuses the plan, scratch, and resident
buffers for a 512 × 256 input. Its samples are the top byte of
xorshift32 with shifts 13, 17, and 5 and seed `0x63a5d5df`.
The strided and planar cases pad each row by 32 bytes. The RGB cases
use the Display P3 profile above, and CMYK uses the pinned C testbed's
`test1.icc` (SHA-256
`1a996ebe6b5d1e7a21620993ce1903df16e4e8fa6cca312bedfb5ae133e62b4d`).

Each invocation ran four warmup applies followed by 192 applies
with `perf stat` task-clock, cycles, and instructions enabled.
Zenbench 0.1.11 interleaved five blocks per case and compared the
output buffers, including padding, with each block's baseline.
Latency deltas below are geometric means of within-block ratios.
Positive numbers mean slower. A/A compares identical baseline builds
and indicates timing variation. These are exploratory measurements on
one CPU and image size.

| Converter variant | Linked `.text` bytes |
|---|---:|
| Full formatter grid, scalar/v3/v4 | 3,628,310 |
| Shared loop, scalar/v3/v4 | 1,038,022 |
| Shared loop, scalar/v3 | 1,029,142 |
| RGB8 direct pair, scalar/v3/v4 | 1,069,430 |
| RGB8 direct pair, scalar/v3 | 1,049,046 |
| Three direct pairs, scalar/v3/v4 | 1,135,734 |
| Three direct pairs, scalar/v3 | 1,091,958 |

The initial comparison completed all 180 invocations with matching
output buffers. Sharing all formatters raised latency by 7.5% on
forced-CLUT RGB8, 53.0% on strided RGBA8, and 3.2% on CMYK8 to RGB8.
Keeping direct calls for the RGB8 formatter pair recovered the
forced-CLUT case. The strided RGBA8 and CMYK cases still executed
73.2% and 3.7% more instructions, respectively. Those cases motivated
the three-pair experiment.

The second comparison kept direct calls for packed RGB8 to RGB8,
packed RGBA8 to RGBA8, and packed CMYK8 to RGB8. The RGBA8 path
preserves the original formatter and layout checks before using
`unroll3_bytes_skip1` and `pack3_bytes_and_skip1`. The remaining
formats use their original unpack and pack callbacks. The v3-only
specialization uses `#[archmage::autoversion(v3, scalar)]` on
`xform16_eval_loop` in the x86-64 scratch build. Other SIMD dispatch
stays in place.

| Workload | Baseline ns/pixel | Three pairs, scalar/v3/v4 | Three pairs, scalar/v3 | A/A |
|---|---:|---:|---:|---:|
| Display P3, packed RGB8 | 6.101 | −0.72% | −1.02% | −0.15% |
| Display P3, forced-CLUT RGB8 | 17.327 | −0.10% | +0.41% | +0.02% |
| Display P3, strided RGBA8 with alpha copy | 7.120 | +0.81% | −0.72% | −0.28% |
| Display P3, packed RGB16 | 14.471 | +2.07% | −0.17% | −0.02% |
| Display P3, planar RGB8 | 12.818 | −0.07% | +0.42% | +0.05% |
| CMYK8 to sRGB RGB8 | 26.152 | +3.19% | +1.05% | +0.19% |

All 120 invocations in the second comparison matched the baseline
output. The three-pair build with v3 and scalar removes 2,536,352
linked code bytes (69.9%). Removing v4 from that build accounts
for 43,776 bytes. Both three-pair builds restore the baseline
instruction counts on the six workloads. The v3 and scalar build's
latency deltas range from −1.02% to +1.05%.
That build also passed 148 x86-64 tests across `api_c_parity`,
`c_parity_pack`, and `c_parity_xform_matrix`.
These data support testing the three-pair build on a wider set of
profiles, formats, and CPU generations.

[Initial formatter comparison CSV](../../benchmarks/x64-formatter-screen_2026-11-22.csv)
· [Three-pair comparison CSV](../../benchmarks/x64-three-pair_2026-11-22.csv)
