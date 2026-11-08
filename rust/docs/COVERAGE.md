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
| `c_parity_virt2` | `cmsvirt.c`: Gray/XYZ/sRGB/NULL/Lab2/Lab4/OkLab/BCHSW builders, linearization + ink-limiting devicelinks, `SetSeqDescTag`/`WriteSeqID` (`pseq`+`psid` serialization per ICC.1:2022-05 v4 rules) | serialized profile bytes C-vs-Rust (header `created` field masked — C stamps `time(0)`) |
| `c_parity_sm` | `cmssamp.c`: `cmsDetectBlackPoint`, `cmsDetectDestinationBlackPoint`, `cmsDetectTAC` over 15 bundled profiles x intents 0-3 | f64 XYZ result-exact + both-reject parity |
| `c_parity_xform2` | `cmsxform.c`: multiprofile chain, proofing (+GAMUTCHECK), `cmsDoTransformStride`/`LineStride`, `cmsChangeBuffersFormat`, format getters, `GetTransformGamutCheckPipeline`, alarm codes, adaptation state | byte-exact buffers + status/result parity |
| `c_parity_cgats2` | `cmscgats.c`: IT8 writer (`SaveToMem`), `SetSheetType`/`Comment`, property str/dbl/hex/multi/uncooked, `SetDataFormat`/`SetData`/`SetDataDbl`/`SetDataRowCol`, `GetPatchName`/`ByName`, `FindDataFormat`, parse->save roundtrip, malformed rejects | byte-exact writer output + getter/result parity |
| `c_parity_ioerr` | `cmsio0.c` open paths: 25+ corrupted profile streams (bad magic, tag-table corruption, truncations, header flips, appended garbage); `cmsio1.c` getters + `IsIntentSupported` matrix | accept/reject + getter parity |
| `c_parity_pack` | `cmspack.c`: 18-format sweep x all pairs over RGB/Lab/XYZ/Gray/CMYK/MCH5-10 (devicelink profiles), packed 565/444, doswap/endian16/planar/swapfirst, invalid-format rejects | byte-exact `cmsDoTransform`; float outputs pinned to ~4e-5 rel residual |
| `c_parity_types` | `cmstypes.c` tag-level: per-tag raw read/write/reserialize across the bundled corpus | raw payload + resave byte-exact (created masked) |
| `c_parity_err` | `cmserr.c`/`cmsgmt.c`/context: GBD alloc->add->compute->check grid, error-handler code+text parity on SynError, context lifecycle | bool/result + error-code/text parity |

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
6. **`virt.rs::set_seq_desc_tag` wrote real MLU strings** where C leaves
   them empty — `cmsAllocProfileSequenceDescription` NULLs the mlus and
   `cmsMLUsetASCII(NULL,…)` silently no-ops, so C serializes empty seq
   mlus. Rust now matches; also writes `psid` (v4) alongside `pseq` like C.
7. **`xform.rs::change_buffers_format` skipped the
   `dwOriginalFlags & CAN_CHANGE_FORMATTER` check** — C auto-sets the flag
   only for ≥2-byte inputs (or 0/0 placeholder); u8→u16 changes must
   return FALSE + `cmsERROR_NOT_SUITABLE`. Fixed.
8. **IT8 builder bugs in `cgats.rs`** (3): `set_data_by_name` re-ran
   `allocate_data_format`, wiping labels; `set_property_str` only quoted
   whitespace-containing values (C: WRITE_STRINGIFY always);
   `set_property_hex` stored `0x{x}` but C stores `"%u"` decimal — the
   writer emits `0x%X` of `satoi`, producing `0x0`.
9. **`xform.rs` identity memcpy fast path copied the extra channel** —
   C's unroll/pack leaves extra output bytes untouched without
   `cmsFLAGS_COPY_ALPHA` (RGBA8→RGBA8: C writes 0, Rust wrote input
   alpha). Fast path now gated on `t_extra == 0`.
10. Upstream-oracle quirks pinned (not parity paths): `cmsIT8LoadFromMem`
    asserts `len != 0`; C's IT8 parse is lossy (drops comments, collapses
    multi-props, normalizes hex) so roundtrip parity compares
    resave-of-same-input; `pseq`/`psid` header `created` field is
    `time(0)`-nondeterministic — masked in compares.
    (Retracted: an earlier version of this list claimed `cmsIT8Free` /
    `cmsIT8GetPatchName` corrupt the oracle heap on setter-built docs —
    that was TEST misuse: `cmsIT8GetPatchName` unconditionally writes
    `MAXSTR` (1024) bytes into the caller's buffer via `strncpy`, and the
    gate passed 64. With a 1024-byte buffer both calls are clean.)
11. **`cgats.rs::syn_error` wrong code + format** — signaled `ERROR_READ`
    (5) instead of `cmsERROR_CORRUPTION_DETECTED` (12), formatted
    `"msg (line N)"` instead of C's `": Line N, msg"` (FileName empty for
    mem/alloc docs), and dropped the trailing period on
    `"Invalid or more than NUMBER_OF_FIELDS fields."`. Caught by
    error-handler parity.
12. **`cgats.rs::set_property_dbl` ignored `define_dbl_format`** — C does
    `snprintf(buf, DoubleFormatter, val)`; Rust used Display. `%.2f`→
    `1.50` vs `1.5`. Added `format_dbl`, a printf-spec applier covering
    `%f`/`%e`/`%E`/`%g`/`%G` including `%g`'s exponential-vs-fixed
    selection and trailing-zero stripping.

## ICC spec references

Downloaded 2026-11-07 to `/mnt/v/icc-spec/` (not committed — binary docs):

- `ICC.1-2022-05.pdf` — **ICC.1:2022-05 (v4.4), the current v4 spec** —
  authority for tag types, encodings, and MPE semantics the gates check.
- `ICC.1-2001-04.pdf` — v2.1 spec (legacy v2 profiles).
- `ICC.2-2019.pdf` — iccMAX (v5); outside lcms2's scope.

## C oracle through the parity suite — AFTER (2026-11-08, `just coverage-cref`)

| file | lines before → after | | file | lines before → after |
|---|---|---|---|---|
| **cmsalpha.c** | **8.9% → 86.2%** | | cmsopt.c | 63.6% → 65.9% |
| **cmscgats.c** | **35.6% → 52.3%** | | **cmspack.c** | **29.2% → 45.5%** |
| **cmsio1.c** | **41.4% → 55.4%** | | cmspcs.c | 92.4% → 88.4% * |
| **cmssamp.c** | **27.0% → 68.2%** | | cmsplugin.c | 53.2% → 46.8% * |
| **cmsvirt.c** | **27.1% → 40.3%** | | cmsps2.c | 77.9% → 72.4% * |
| **cmsxform.c** | **42.0% → 55.0%** | | cmssm.c | 91.7% → 86.1% * |
| cmsio0.c | 60.6% → 59.0% * | | cmstypes.c | 56.0% → 47.6% * |
| cmscam02.c | 98.1% → 91.3% * | | cmswtpnt.c | 99.3% → 89.3% * |
| cmscnvrt.c | 63.3% → 60.4% * | | cmsintrp.c | 96.2% → 96.2% |
| cmserr.c | 37.7% → 34.8% * | | cmslut.c | 82.8% → 72.3% * |
| cmsgamma.c | 81.7% → 77.8% * | | cmsmd5.c | 82.4% → 88.1% |
| cmsgmt.c | 45.3% → 41.3% * | | cmsnamed.c | 81.7% → 72.1% * |
| cmshalf.c | 100% → 100% | | cmsmtrx.c | 100% → 100% |
| **TOTAL** | **52.2% → 61.3% lines, 62.7% → 76.9% fns** | | | |

`*` = the clang-instrumented run loses entire binaries' profraws to
SIGSEGV/SIGABRT (LLVM's atexit never fires) — apparent dips are lost
profraws, not regressions; the same binaries pass under the gcc oracle.
These numbers are a floor.

## Remaining ungated C surface (next gates, in order of missed lines)

- **cmstypes.c** ~47.6% lines — MPE tag-type handlers (BaAs/clut/vcgt/etc.),
  error paths in tag readers (malformed payloads), uncommon types
- **cmslut.c** ~72.3% — MPE stage evaluators beyond what transforms reach,
  stage alloc/read/write paths for exotic stage types
- **cmscgats.c** ~52.3% — multi-table paths (`SetTable`/`SetTableByLabel`),
  index-column, `EnumProperties`/`EnumPropertyMulti`, `DefineDblFormat`,
  `GetDataFormat`/`EnumDataFormat` getters
- **cmserr.c / cmsplugin.c** ~35–47% — error-handler registration
  (`cmsSetLogErrorHandler`), plugin slot machinery, context dup
- **cmsio0.c** ~59% — save/serialize edge cases, tag read/write paths for
  uncommon tags, IO-handler variants
- **cmsgmt.c** ~41% — gamut boundary descriptor paths beyond the proofing
  transform
- **cmscnvrt.c** ~60% — intent-specific pipeline construction beyond the
  transform combos exercised
- **cmsvirt.c** ~40% — remaining builders (OkLab variants, additional
  devicelink types)
