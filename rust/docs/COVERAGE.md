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
| **TOTAL** | **52.2% → ~61% lines, 62.7% → ~76% fns** (run-to-run ±3pp from lost profraws) | | | |

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

## C oracle through the parity suite — current (post-campaign)

`just coverage-cref` after the ~64 parity gates — **61.75% lines /
78.03% functions** total (vs 52.2%/62.7% before the campaign):

| file | lines | fns | | file | lines | fns |
|---|---|---|---|---|---|---|
| cmsalpha.c | 86.2% | 76.8% | | cmsopt.c | 71.3% | 61.4% |
| cmscam02.c | 91.3% | 84.0% | | cmspack.c | 46.7% | 48.9% |
| cmscgats.c | 56.0% | 46.8% | | cmspcs.c | 61.4% | 64.1% |
| cmscnvrt.c | 63.2% | 54.8% | | cmsplugin.c | 69.8% | 61.1% |
| cmserr.c | 67.6% | 50.0% | | cmsps2.c | 72.4% | 61.3% |
| cmsgamma.c | 67.6% | 58.8% | | cmssamp.c | 68.2% | 50.9% |
| cmsgmt.c | 55.7% | 44.5% | | cmssm.c | 90.7% | 82.7% |
| cmshalf.c | 100% | 100% | | cmstypes.c | 50.5% | 44.3% |
| cmsintrp.c | 98.5% | 95.5% | | cmsvirt.c | 40.3% | 29.0% |
| cmsio0.c | 69.8% | 63.4% | | cmswtpnt.c | 89.3% | 76.7% |
| cmsio1.c | 59.4% | 47.0% | | cmsxform.c | 61.1% | 55.9% |
| cmslut.c | 73.0% | 64.9% | | **TOTAL** | **61.75%** | **78.03%** |
| cmsmd5.c | 88.1% | 45.5% | | | | |
| cmsmtrx.c | 100% | 100% | | cmsnamed.c | 71.7% | 60.4% |

Lowest files: `cmsvirt.c` 40.3% (the virt builders' uncovered paths),
`cmspack.c` 46.7%, `cmstypes.c` 50.5%, `cmscgats.c` 56.0%,
`cmsgmt.c` 55.7%, `cmsio1.c` 59.4%.

## C oracle through the parity suite — AFTER round 2 (`just coverage-cref`)

**66.13% lines / 83.72% functions** after the remaining-API batch
(~34 more gates; the exported `cms*` surface is now fully
exercised):

| file | lines | fns | | file | lines | fns |
|---|---|---|---|---|---|---|
| cmsalpha.c | 86.6% | 85.4% | | cmsopt.c | 76.1% | 81.8% |
| cmscam02.c | 91.3% | 100% | | cmspack.c | 49.0% | 58.0% |
| cmscgats.c | 66.3% | 91.9% | | cmspcs.c | 63.8% | 100% |
| cmscnvrt.c | 66.4% | 87.0% | | cmsplugin.c | 73.4% | 100% |
| cmserr.c | 79.3% | 100% | | cmsps2.c | 72.4% | 90.6% |
| cmsgamma.c | 70.7% | 89.5% | | cmssamp.c | 72.6% | 85.7% |
| cmsgmt.c | 79.0% | 100% | | cmssm.c | 90.7% | 100% |
| cmshalf.c | 100% | 100% | | cmstypes.c | 51.0% | 79.0% |
| cmsintrp.c | 98.5% | 100% | | cmsvirt.c | 66.3% | 96.9% |
| cmsio0.c | 74.5% | 95.7% | | cmswtpnt.c | 87.1% | 100% |
| cmsio1.c | 63.4% | 80.0% | | cmsxform.c | 70.9% | 75.0% |
| cmslut.c | 79.2% | 96.1% | | cmsmd5.c | 88.1% | 80.0% |
| cmsmtrx.c | 100% | 100% | | cmsnamed.c | 72.2% | 97.8% |

Remaining gaps are mostly line-level: cmspack (~135 Unroll/Pack
formatter variants — the format matrix covers status but not
every combo's eval), cmstypes (~30 Type_* Read/Dup/Free branches
on unusual payloads), cmsio1 (tag-serialization internals on
uncommon tag shapes), cmscgats (parser error paths), cmscnvrt
(intent-link internals on unusual chains).

## Round 3 — AFTER the internals sweep (through 536cd5f1)

C oracle re-measured after the round-3 gates: **67.17% lines /
84.68% functions** (up from round-2's 66.46/84.04; round-1's
52.2/62.7). Every exported `cms*` + `cmsIT8*` + `cmsDup*` +
`cmsNamedColor*` + `cmsPostScript*` API is exercised; 33
Type_* read paths via crafted raw tags; 17x17 formatter-pair
matrix; all-intent ps2 emission; v2/v4 stored-type dispatch.

| file | lines | functions |
|---|---|---|
| cmstypes.c | 53.77% (was 51.07%) | 83.18% |
| cmspack.c | 48.95% | 57.97% |
| cmsio1.c | 64.20% | 80.00% |
| cmscnvrt.c | 67.25% | 86.96% |
| cmscgats.c | 67.18% | 94.95% |
| cmsvirt.c | 66.34% | 96.88% |
| cmspcs.c | 74.04% (was 63.75%) | 100% |
| cmsio0.c | 75.20% | 95.65% |
| cmsnamed.c | 72.38% | 97.83% |
| cmsopt.c | 76.29% | 81.82% |
| cmsps2.c | 72.41% | 90.62% |
| cmsgamma.c | 71.39% | 89.47% |
| cmsxform.c | 70.89% | 75.00% |
| cmslut.c | 79.23% | 96.05% |
| cmsalpha.c | 86.61% | 85.37% |
| cmscam02.c | 91.26% | 100% |
| cmsgmt.c | 78.97% | 100% |
| cmssamp.c | 68.22% | 85.71% |
| cmsplugin.c | 73.61% | 100% |
| cmserr.c | 79.30% | 100% |
| cmssm.c | 90.70% | 100% |
| cmswtpnt.c | 87.10% | 100% |
| cmsintrp.c | 98.50% | 100% |
| cmsmd5.c | 88.11% | 80.00% |
| cmshalf.c | 100% | 100% |
| cmsmtrx.c | 100% | 100% |

Remaining line-level gaps are deep internals: the ~135
formatter variant bodies in cmspack beyond the eval'd combos,
the MPE/pipeline serialization internals in cmstypes, tag
serialization internals in cmsio1.

## Round 4 — AFTER the internals deep-sweep (through 138ce24c)

C oracle re-measured after ~30 more internals gates: **67.84%
lines / 85.73% functions** (up from round-3's 67.17/84.68;
round-2's 66.46/84.04; round-1's 52.2/62.7).

Every exported `cms*` API is exercised — builders, transforms,
IO variants, plugins, IT8 machinery, named-color, PostScript,
dict, MLU, tone-curve, pipeline, stage, GBD, profile-sequence,
header machinery, formatter-pair matrix (~500 combos), 120+
Type_* read combos via crafted raw tags, device-class
branches, corrupt-profile/directory parser paths, linked-tag
machinery, multi-table IT8, all-intent ps2, v2/v4 type
dispatch, flag branches, unusual-class-chain validation.

| file | lines | functions |
|---|---|---|
| cmspack.c | 52.14% (was 48.95%) | 66.67% (was 57.97%) |
| cmstypes.c | 53.89% (was 51.07%) | 83.64% |
| cmsio1.c | 64.20% | 80.00% |
| cmscnvrt.c | 67.25% | 86.96% |
| cmscgats.c | 69.35% (was 67.18%) | 94.95% |
| cmsvirt.c | 66.34% | 96.88% |
| cmspcs.c | 74.29% | 100% |
| cmsio0.c | 76.33% | 95.65% |
| cmsnamed.c | 73.04% | 97.83% |
| cmsopt.c | 76.29% | 81.82% |
| cmsps2.c | 72.41% | 90.62% |
| cmsgamma.c | 71.10% | 89.47% |
| cmsxform.c | 70.89% | 75.00% |
| cmslut.c | 79.23% | 96.05% |
| cmsalpha.c | 86.61% | 85.37% |
| cmscam02.c | 91.26% | 100% |
| cmsgmt.c | 78.97% | 100% |
| cmssamp.c | 72.59% | 85.71% |
| cmsplugin.c | 73.61% | 100% |
| cmserr.c | 79.30% | 100% |
| cmssm.c | 90.70% | 100% |
| cmswtpnt.c | 87.10% | 100% |
| cmsintrp.c | 98.50% | 100% |
| cmsmd5.c | 88.11% | 80.00% |
| cmshalf.c | 100% | 100% |
| cmsmtrx.c | 100% | 100% |

Remaining line-level gaps: cmspack's ~135 formatter variant
bodies beyond the eval'd combos, cmstypes' MPE/pipeline
serialization internals + Dup/Free on unusual payloads,
cmsio1's tag-serialization internals — reachable only through
oracle-UB inputs or profile shapes the public API can't
construct.

## Round 5 — AFTER the remaining-internals sweep (through 6aeb6b5d)

C oracle re-measured after ~20 more internals gates: **68.37%
lines / 86.29% functions** (up from round-4's 67.84/85.73).

| file | lines | functions |
|---|---|---|
| cmspack.c | 52.14% | 66.67% |
| cmstypes.c | 55.71% (was 53.89%) | 86.45% |
| cmsio1.c | 64.40% | 80.00% |
| cmscnvrt.c | 67.08% | 86.96% |
| cmscgats.c | 70.83% (was 69.35%) | 95.96% |
| cmsvirt.c | 66.34% | 96.88% |
| cmsio0.c | 76.23% | 95.65% |
| cmsnamed.c | 73.21% | 97.83% |
| cmsopt.c | 76.29% | 81.82% |
| cmsps2.c | 72.41% | 90.62% |
| cmsgamma.c | 71.53% | 89.47% |
| cmsxform.c | 70.89% | 75.00% |
| cmslut.c | 79.49% | 96.05% |
| cmsalpha.c | 86.61% | 85.37% |
| cmscam02.c | 91.26% | 100% |
| cmsgmt.c | 80.07% | 100% |
| cmssamp.c | 68.22% | 85.71% |
| cmsplugin.c | 73.61% | 100% |
| cmserr.c | 79.30% | 100% |
| cmssm.c | 90.70% | 100% |
| cmspcs.c | 74.55% | 100% |
| cmswtpnt.c | 87.10% | 100% |
| cmsintrp.c | 98.50% | 100% |
| cmsmd5.c | 88.11% | 80.00% |
| cmshalf.c | 100% | 100% |
| cmsmtrx.c | 100% | 100% |

## Round 6 — AFTER the internals sweep 2 (through 1e86da39)

C oracle re-measured after ~35 more internals gates: **68.00%
lines / 85.89% functions** (vs round-5's 68.37/86.29 —
within measurement variation; cmsnamed up to 75.21%).

| file | lines | functions |
|---|---|---|
| cmspack.c | 52.14% | 66.67% |
| cmstypes.c | 55.66% | 86.45% |
| cmsio1.c | 64.40% | 80.00% |
| cmscnvrt.c | 67.08% | 86.96% |
| cmscgats.c | 70.83% | 95.96% |
| cmsvirt.c | 66.67% | 96.88% |
| cmsio0.c | 76.84% | 95.65% |
| cmsnamed.c | 75.21% | 100% |
| cmsopt.c | 76.29% | 81.82% |
| cmsps2.c | 72.41% | 90.62% |
| cmsgamma.c | 70.52% | 89.47% |
| cmsxform.c | 70.89% | 75.00% |
| cmslut.c | 69.51% | 88.16% |
| cmsalpha.c | 86.61% | 85.37% |
| cmscam02.c | 91.26% | 100% |
| cmsgmt.c | 80.07% | 100% |
| cmssamp.c | 68.22% | 85.71% |
| cmsplugin.c | 73.61% | 100% |
| cmserr.c | 79.30% | 100% |
| cmssm.c | 90.70% | 100% |
| cmspcs.c | 74.55% | 100% |
| cmswtpnt.c | 87.10% | 100% |
| cmsintrp.c | 98.50% | 100% |
| cmsmd5.c | 88.11% | 80.00% |
| cmshalf.c | 100% | 100% |
| cmsmtrx.c | 100% | 100% |

## Round 7 — AFTER the cmspack format sweep (through ee90d50a)

C oracle re-measured after ~10 format-variant gates:
**68.09% lines / 86.13% functions** (cmspack up to
53.42% lines / 68.84% functions).

| file | lines | functions |
|---|---|---|
| cmspack.c | 53.42% | 68.84% |
| cmstypes.c | 55.62% | 86.45% |
| cmsio1.c | 64.40% | 80.00% |
| cmscnvrt.c | 67.08% | 86.96% |
| cmscgats.c | 70.62% | 95.96% |
| cmsvirt.c | 66.34% | 96.88% |
| cmsio0.c | 76.84% | 95.65% |
| cmsnamed.c | 75.21% | 100% |
| cmsopt.c | 76.29% | 81.82% |
| cmsps2.c | 72.41% | 90.62% |
| cmsgamma.c | 70.81% | 89.47% |
| cmsxform.c | 70.89% | 75.00% |
| cmslut.c | 69.51% | 88.16% |
| cmsalpha.c | 86.61% | 85.37% |
| cmscam02.c | 91.26% | 100% |
| cmsgmt.c | 80.07% | 100% |
| cmssamp.c | 68.22% | 85.71% |
| cmsplugin.c | 73.61% | 100% |
| cmserr.c | 79.30% | 100% |
| cmssm.c | 90.70% | 100% |
| cmspcs.c | 74.55% | 100% |
| cmswtpnt.c | 87.10% | 100% |
| cmsintrp.c | 98.50% | 100% |
| cmsmd5.c | 88.11% | 80.00% |
| cmshalf.c | 100% | 100% |
| cmsmtrx.c | 100% | 100% |

## Round 8 — AFTER the io0/io1/cgats sweep (through 9b252daf)

C oracle re-measured after ~15 more gates: **68.23% lines /
86.13% functions**.

| file | lines | functions |
|---|---|---|
| cmspack.c | 53.42% | 68.84% |
| cmstypes.c | 55.62% | 86.45% |
| cmsio1.c | 64.40% | 80.00% |
| cmscnvrt.c | 67.25% | 86.96% |
| cmscgats.c | 71.67% | 95.96% |
| cmsvirt.c | 66.34% | 96.88% |
| cmsio0.c | 77.36% | 95.65% |
| cmsnamed.c | 75.54% | 100% |
| cmsopt.c | 76.29% | 81.82% |
| cmsps2.c | 72.41% | 90.62% |
| cmsgamma.c | 70.95% | 89.47% |
| cmsxform.c | 70.89% | 75.00% |
| cmslut.c | 69.51% | 88.16% |
| cmsalpha.c | 86.61% | 85.37% |
| cmscam02.c | 91.26% | 100% |
| cmsgmt.c | 80.07% | 100% |
| cmssamp.c | 68.22% | 85.71% |
| cmsplugin.c | 73.61% | 100% |
| cmserr.c | 79.30% | 100% |
| cmssm.c | 90.70% | 100% |
| cmspcs.c | 74.55% | 100% |
| cmswtpnt.c | 87.10% | 100% |
| cmsintrp.c | 98.50% | 100% |
| cmsmd5.c | 88.11% | 80.00% |
| cmshalf.c | 100% | 100% |
| cmsmtrx.c | 100% | 100% |

## Round 10 — AFTER the lut/io1e/xform2 sweep (through 7152a5d0)

C oracle re-measured after ~35 more gates: **68.45% lines /
86.21% functions**. Also fixed an oracle-UB in
`identity_stages` (16-channel pipeline alloc → NULL →
eval assertion).

| file | lines | functions |
|---|---|---|
| cmspack.c | 53.42% | 68.84% |
| cmstypes.c | 55.64% | 86.45% |
| cmsio1.c | 64.40% | 80.00% |
| cmscnvrt.c | 67.08% | 86.96% |
| cmscgats.c | 71.67% | 95.96% |
| cmsvirt.c | 67.00% | 96.88% |
| cmsio0.c | 78.07% | 95.65% |
| cmsnamed.c | 76.21% | 100% |
| cmsopt.c | 77.46% | 84.09% |
| cmsps2.c | 72.41% | 90.62% |
| cmsgamma.c | 70.95% | 89.47% |
| cmsxform.c | 71.61% | 75.00% |
| cmslut.c | 69.64% | 88.16% |
| cmsalpha.c | 86.61% | 85.37% |
| cmscam02.c | 91.26% | 100% |
| cmsgmt.c | 80.07% | 100% |
| cmssamp.c | 68.22% | 85.71% |
| cmsplugin.c | 73.61% | 100% |
| cmserr.c | 79.69% | 100% |
| cmssm.c | 90.70% | 100% |
| cmspcs.c | 76.09% | 100% |
| cmswtpnt.c | 87.10% | 100% |
| cmsintrp.c | 98.50% | 100% |
| cmsmd5.c | 88.11% | 80.00% |
| cmshalf.c | 100% | 100% |
| cmsmtrx.c | 100% | 100% |

## Round 11 — AFTER the corpus/ps2/io1e sweep (through d2868f26)

C oracle re-measured after ~17 more gates: **69.08% lines /
86.85% functions**. cmslut 69.64→80.83%, cmsopt 77.46→79.51%,
cmspcs 76.09→79.69%, cmscgats 71.67→72.58%, cmscnvrt
67.08→67.78%, cmsio0 78.07→78.38%. Also fixed a test-race:
new ps2 gates were missing PS_LOCK (shared C-oracle state).

| file | lines | functions |
|---|---|---|
| cmspack.c | 53.42% | 68.84% |
| cmstypes.c | 55.76% | 86.45% |
| cmsio1.c | 62.80% | 80.00% |
| cmscnvrt.c | 67.78% | 86.96% |
| cmscgats.c | 72.58% | 95.96% |
| cmsvirt.c | 66.83% | 96.88% |
| cmsio0.c | 78.38% | 95.65% |
| cmsnamed.c | 76.21% | 100% |
| cmsopt.c | 79.51% | 88.64% |
| cmsps2.c | 65.27% | 84.38% |
| cmsgamma.c | 71.10% | 89.47% |
| cmsxform.c | 71.61% | 75.00% |
| cmslut.c | 80.83% | 98.68% |
| cmsalpha.c | 86.61% | 85.37% |
| cmscam02.c | 91.26% | 100% |
| cmsgmt.c | 80.07% | 100% |
| cmssamp.c | 68.22% | 85.71% |
| cmsplugin.c | 73.61% | 100% |
| cmserr.c | 79.69% | 100% |
| cmssm.c | 90.70% | 100% |
| cmspcs.c | 79.69% | 100% |
| cmswtpnt.c | 87.10% | 100% |
| cmsintrp.c | 98.50% | 100% |
| cmsmd5.c | 88.11% | 80.00% |
| cmshalf.c | 100% | 100% |
| cmsmtrx.c | 100% | 100% |

## Round 13 — pack/types internals (through 973ed8f4)

**71.96% lines / 90.78% functions / 80.38% regions / 63.73% branches**
(up from round-12's 69.80/87.25). cmspack jumped 53.42→69.90%,
cmstypes 56.53→58.47%, cmspcs 79.69→83.55%, cmsopt ~80.59%,
cmsxform 71.96→73.57%. New: pack_chgbuf_sweep (16-table float swap),
pack_labv2 (v2 encode/decode both paths), pack_misc16 (4ch swap-first,
MCH2, 1ch+ex3, planar premul), pack_optimized_u8 (matshaper *Optimized
packers), mpe_structured_writes (Type_MPEmatrix/clut_Write + GenericMPE),
mhc2_write_read (Type_MHC2_* + identity/doubles helpers),
dict_cooked_write (Type_Dictionary_* + ReadOneMLUC/WriteOneMLUC).

Port bugs found+fixed by these gates: change_buffers_format always
selects the 16-bit table; LabV2-from-float packers (v4>>8/direct-v2);
pack_planar_{words,bytes} missing premul; to_fixed_domain formula;
cms_write_tag skipping the cmsDictDup reversal.

| file | lines | fns |
|---|---|---|
| cmstypes.c | 58.47% | 89.72% |
| cmsio1.c | 67.20% | 88.00% |
| cmsvirt.c | 67.00% | 96.88% |
| cmscnvrt.c | 67.95% | 86.96% |
| cmssamp.c | 68.22% | 85.71% |
| cmspack.c | 69.90% | 94.93% |
| cmsgamma.c | 71.39% | 89.47% |
| cmscgats.c | 72.58% | 95.96% |
| cmsxform.c | 73.57% | 75.00% |
| cmsplugin.c | 73.61% | 100% |
| cmsio0.c | 79.30% | 95.65% |
| cmsnamed.c | 76.21% | 100% |
| cmsopt.c | 80.59% | 90.91% |
| cmsps2.c | 77.34% | 93.75% |
| cmslut.c | 81.36% | 98.68% |
| cmspcs.c | 83.55% | 100% |
| TOTAL | **71.96%** | **90.78%** |

## Round 14 — exported internals + float-LUT/gamut/named-color (through 0a6b5460)

**73.61% lines / 93.34% functions / 82.07% regions / 65.07% branches**
(up from round-13's 71.96/90.78). cmsxform 73.57->85.00%,
cmsio1 67.20->75.20%, cmsvirt 67.00->69.97%,
cmstypes 58.47->61.70%, cmsgmt 80.07->83.76%,
cmsgamma 71.39->71.97%. New gates: mpet_element_writes
(Type_MPE{curve,matrix,clut}_Write + GenericMPE), lut8_dup_write
(Type_LUT8_Dup), crdinfo_write (Type_CrdInfo_Dup),
cooked_reads_mhc2_dict (Type_MHC2_Read + ReadDoublesAt +
dict ReadOneMLUC attempt), sigmoid_curves (type +-109),
temp_whitepoint_roundtrip (CHAD2Temp/Temp2CHAD),
gamutcheck_workers (Cached+Precalculated gamut-check xforms,
CTX_LOCK race fix + alarm restore), named_color_devicelink
(CreateNamedColorDevicelink), float_lut_reads
(_cmsReadFloat{Input,Output,Devicelink}Tag),
blackpoint_quadratic (RootOfLeastSquaresFitQuadraticCurve),
xform_internals (exported _cmsGetTransform*/UserData/
ReadProfileSequence/SetHeaderCMM/DefaultICCintents,
NullFloatXFORM, UnrollNothing/PackNothing).

| file | lines | fns |
|---|---|---|
| cmstypes.c | 61.70% | 92.99% |
| cmscnvrt.c | 68.13% | 91.30% |
| cmssamp.c | 68.22% | 85.71% |
| cmsvirt.c | 69.97% | 100% |
| cmspack.c | 69.90% | 94.93% |
| cmsgamma.c | 71.97% | 100% |
| cmscgats.c | 72.58% | 95.96% |
| cmsplugin.c | 73.61% | 100% |
| cmsio1.c | 75.20% | 100% |
| cmsnamed.c | 76.54% | 100% |
| cmsps2.c | 77.34% | 93.75% |
| cmsio0.c | 80.02% | 98.91% |
| cmsopt.c | 80.59% | 90.91% |
| cmslut.c | 81.36% | 98.68% |
| cmsxform.c | 85.00% | 98.21% |
| TOTAL | **73.61%** | **93.34%** |

### round 15 — cube-parser parity fix + gray PS2 + LUT orders

75.04% lines / 94.87% fns / 83.62% regions / 66.59% branches.

| file | lines |
|---|---|
| cmstypes.c | 63.40% |
| cmspack.c | 69.97% |
| cmscnvrt.c | 70.05% |
| cmsvirt.c | 69.97% |
| cmscgats.c | 78.12% |
| cmsio0.c | 80.64% |
| cmsio1.c | 75.60% |
| cmsplugin.c | 73.61% |
| cmsnamed.c | 76.71% |
| cmsps2.c | 77.34% |
| cmsgamma.c | 83.67% |
| cmslut.c | 81.89% |
| cmsopt.c | 81.27% |
| cmsxform.c | 83.93% |

Real parity fixes this round: cgats.rs cube parser priming + binary-search
keywords + unknown-symbol rejection; pack.rs planar premul + to_fixed_domain;
xform.rs change_buffers_format 16-bit semantics. Failing targets under clang
instrumentation: c_parity_cam02, c_parity_cube, c_parity_pcs, c_parity_xform2
(the last was a parallel NULL-context race — now CTX_LOCK-serialized; the
first three are the known instrumentation-codegen failures).
### round 16 — class/intent dispatch + LUT read-back + tag/MLU/iohandler sweeps

75.35% lines / 94.87% fns / 83.86% regions / 67.04% branches.

| file | lines |
|---|---|
| cmstypes.c | 63.74% |
| cmspack.c | ~70% |
| cmscnvrt.c | ~70% |
| cmsvirt.c | 69.97% |
| cmscgats.c | ~78% |
| cmsio0.c | ~81% |
| cmsio1.c | ~76% |
| cmsplugin.c | ~74% |
| cmsnamed.c | ~77% |
| cmsps2.c | ~77% |
| cmsgamma.c | ~84% |
| cmslut.c | ~82% |
| cmsopt.c | ~81% |
| cmsxform.c | 83.93% |
| cmspcs.c | up (44% fns -> conversions+delta-e covered) |
| cmssamp.c | 68.22% |

Gates added: class_intent_dispatch, opt_matrix_shaper_shapes,
stage_eval_sweep, save_tags_arms, plugin_remaining_arms,
blackpoint_dispatch, named_color_ops, matrix_shaper_lab_pcs,
lut_readback_cycles, mab_readback_cycles, named_color_tag_cycle,
opt_shape_sweep, iohandler_modes, tag_type_sweep, tag_reader_sweep,
clut_sampler_slice, named_color_stage, pcs_conversion_chains,
it8_data_arms, devicelink_lut_dispatch, output_lut_lab_normalization,
float_pack_arms, gbd_sweep, matrix_shaper_directions, lab_xyz_stages,
mlu_ops_branches, named_color_input_lut, gray_matrix_pipelines,
gamut_preview_lut_readers, proofing_transform_paths, pcs_conversions,
pcs_delta_e_xyz.

c_parity_cube fixed to assert the C-parity reject on the malformed
file (was relying on the buggy accept-all parse). Function coverage
plateaued at 94.87% — the remaining 64 uncovered fns are largely
unreachable-by-API internals (reserved/dead branches, plugin-shim
paths, WriteBinary/satob dead enum). Line coverage is now the axis.
### round 17 — io0/tag serializers + lut internals + named/dict/virt

75.20% lines / 94.79% fns / 83.74% regions / 66.79% branches.

| file | lines | fns |
|---|---|---|
| cmstypes.c | 64.14% | 99.07% |
| cmspack.c | 70.03% | 94.93% |
| cmscgats.c | ~78% | ~96% |
| cmsvirt.c | 69.97% | 100% |
| cmsopt.c | 81.27% | 95.45% |
| cmsio0.c | ~81% | ~100% |
| cmsio1.c | 76.80% | 100% |
| cmsplugin.c | 77.83% | 100% |
| cmsnamed.c | 80.70% | 100% |
| cmsps2.c | 77.34% | 93.75% |
| cmslut.c | 81.89% | 100% |
| cmsxform.c | 85.18% | 98.21% |
| cmspcs.c | 83.80% | 100% |
| cmssamp.c | 68.22% | 85.71% |

Gates added: gamut_preview_lut_readers, float_lut_lab_normalization,
linearization_devicelink, lab_profile_variants, xyz_null_profiles,
dict_tag_cycle, ucrbg_screening_tags, screening_tag, parametric_curve_
reader, tag_types_2, header_setters_getters, header_datetime,
profile_info_getters, corpus_all_tags, malformed_tag_reads,
lut_reader_error_paths, mab_reader_error_paths, raw_tag_io,
file_open_offset_array, save_two_phase_multiprofile,
tag_search_linked_null, iohandler_internals, mch_pack_arms,
pipeline_accessors_reverse, save_as_8bits_flag, stage_internals,
stage_dup_insert_positions, granular_clut_accessors, mpe_readback_
cycles, slice_space_tac, named_color_crd, transform_accessors_
strides, it8_api_bodies, it8_writer_arms, media_white_chad,
mlu_serialization_blocks, nonegatives_abstract_devicelink,
pcs_conversions, pcs_delta_e_xyz.

cmspcs.c jumped to 100% fns / 83.80% lines on the pcs conversion +
delta-e sweep. cmsio1 100% fns, cmsnamed 100% fns, cmsvirt 100% fns,
cmslut 100% fns, cmsio0 ~100% fns, cmsplugin 100% fns — function
coverage is effectively saturated; the remaining 65 uncovered fns are
unreachable-by-API internals (plugin-shim dead ends, reserved enum
bodies, the WriteBinary/satob dead helpers). Line coverage is the
remaining axis — the mass is cmstypes LUT-reader/writer internals and
the cmspack pack-table variants. The total dipped slightly vs round-16
(one instrumented binary's coverage lost to the known clang
codegen failures).

### round 18 — MCH packers + io0 tag dispatch + lut/mpe readers

75.57% lines / 94.79% fns / 83.98% regions / 67.39% branches.

| file | lines | fns |
|---|---|---|
| cmstypes.c | 81.97% | 99.07% |
| cmspack.c | 73.30% | 94.93% |
| cmscgats.c | 78.83% | 96.97% |
| cmsvirt.c | 83.81% | 100% |
| cmsopt.c | 84.61% | 95.45% |
| cmsio0.c | 88.96% | 100% |
| cmsio1.c | 76.70% | 100% |
| cmsplugin.c | 90.63% | 100% |
| cmsnamed.c | 94.08% | 100% |
| cmsps2.c | 83.86% | 93.75% |
| cmslut.c | 91.07% | 100% |
| cmsxform.c | 86.76% | 98.21% |
| cmspcs.c | 93.55% | 100% |
| cmssamp.c | 62.11% | 85.71% |
| cmscnvrt.c | 76.17% | 91.30% |
| cmsgamma.c | 87.08% | 100% |
| cmsintrp.c | 98.85% | 100% |
| cmswtpnt.c | 98.66% | 100% |
| cmserr.c | 89.62% | 100% |

Gates added since round 17: mch6_packers, mch_8_9_10_packers,
colorspace_intent_internals, per_sig_type_dispatch,
per_sig_dispatch2, multi_entry_sigs, multi_entry_variants,
multi_entry_sig_variants, multi_entry_sig_dispatch,
remaining_sig_dispatch, all_text_sigs, remaining_text_sigs,
text_type_sigs, named_color_sig_dispatch, float_reader_combos,
float_reader_combos2, float_reader_normalization_arms,
float_reader_per_cs, float_reader_colorspaces, float_reader_lab_arms,
float_tag_readers2, float_tag_remaining, float_tag_channel_arms,
float_normalization_colorspaces, devicelink_lut_internals,
lut_reader_all_arms, lut_reader_arms, preview_lut_reader,
lut_reader_dispatch_all, named_lab_stage_evals, lab_stage_evals2,
mab_element_readers, mab_reader_offsets, mpe_reader_element_arms,
mpe_remaining_elements, lut8_16_readers, lut_reader_elements,
lut_readers_internals2, mlu_dict_internals, it8_writer_internals,
it8_parser_arms, inklimit_bchs_builders, virt_builders,
mab_devicelink_sigs, struct_array_tags, pseq_ucrbg_scrn_tags,
ucrbg_resp_scrn_tags, scrn_resp_named_tags, resp_gamma_tags,
pseq_id_mab_tags, multi_numeric_arrays, named_color_sig_dispatch,
multi_entry_payloads, dict_mpet_sig_dispatch.

cmsio0 jumped 81->89% lines (100% fns) on the per-sig type-dispatch
sweep; cmslut 82->91% (LUT reader element loops); cmsnamed 81->94%;
cmsvirt 70->84%; cmspcs 84->94%; cmsopt 81->85%. cmssamp dropped
68->62% — a covcref binary under-reports the tail when a test panics
mid-sweep (the three known clang-codegen failures: cam02, pcs,
xform2 — 13 gates). Function coverage is saturated at 94.79%; the
remaining 65 uncovered fns are unreachable-by-API internals.

Remaining mass (uncovered lines): cmstypes 617 (mAB/mBA writer
element-order paths, curve-element writers, MPE readback shapes),
cmspack 578 (per-format packer bodies — one function per
format-word cell), cmscgats 381 (IT8 lexer arms, uncommon parse
shapes), cmsopt 169, cmsio0 150, cmscnvrt 148, cmsio1 134,
cmssamp 122, cmsvirt 120, cmsps2 117, cmsgamma 116.

### round 19 — tag-type serializers sweep (mhc2/vcgt/ucrbg/chrm/dict/mab) + MCH packers

75.59% lines / 94.55% fns / 83.69% regions / 67.69% branches.

| file | lines | fns | delta vs r18 |
|---|---|---|---|
| cmstypes.c | 83.14% | 99.07% | +1.17 |
| cmscgats.c | 79.78% | 96.97% | +0.95 |
| cmsvirt.c | 84.08% | 100% | +0.27 |
| cmsgamma.c | 88.20% | 100% | +1.12 |
| cmsio0.c | 88.67% | 100% | -0.29 |
| cmsps2.c | 68.28% | 84.38% | -15.6 (instr. fail) |
| cmspack.c | 73.30% | 94.93% | flat |
| cmsopt.c | 83.70% | 95.45% | -0.91 |
| cmssamp.c | 62.11% | 85.71% | flat |

~40 gates landed since round 18: mhc2_tag, vcgt_tag, ucrbg_tag,
chrm_clrt_tags, colorant_order_tag, signature_tag_arms, mba_all_
shapes, mab_mba_clut_shapes, a2b_full_shape, lut8_16_writer_shapes,
desc_reader_arms, dict_record_arms, crdi_psid_tags, text_data_tags,
pseq_psid_full, view_meas_calt_writes, screening_full_tags,
malformed_tag_reads2, multi_item_array_reads, param_curve_read_arms,
curve_text_read_arms, curve_write_arms, mpe_curve_bacs_arms,
named_color_write_arms, mch6_packers, mch_8_9_10_packers,
format_flag_megasweep, float_half_packers, it8_lexer_arms,
it8_data_arms2, parametric_eval_arms, ps2_emitters, optimizer_
internals2, virt_devicelink_internals, tac_detection_internals,
colorspace_intent_internals, float_reader_* sweeps.

cmstypes gained +1.17pts (the whole tag-serializer table is now
exercised). cmsps2 dropped to 68.28% — the new ps2_emitters gate
fails under the clang-instrumented oracle (PostScript text
emission formats floats differently gcc↔clang; same documented
instrumentation-only failure class as cam02/pcs/xform2, which now
also includes 11 ps2 failures — the binary panics mid-sweep so its
tail under-reports). Function coverage plateaued at ~94.6% — the
remaining 68 uncovered fns are unreachable-by-API internals.

Remaining mass (uncovered lines): cmstypes 577 (MPE element
readers/writers deep arms, mAB/mBA readback internals, wchar
embedded-text), cmspack 578 (per-format-cell packer bodies —
each format word maps a dedicated unroller/packer function;
hundreds exist but the reachable set is bounded by the public
format-word table), cmscgats 364 (IT8 lexer value arms, uncommon
property/data formats), cmsopt 179, cmsio0 154, cmscnvrt 149,
cmsio1 134, cmssamp 122, cmsvirt 118, cmsgamma 106.

### round 20 — packer cell sweeps + IT8/cube arms + mAB offsets + ps2 lock fix

76.29% lines / 95.03% fns / 84.60% regions / 68.45% branches.

| file | lines | fns | delta vs r19 |
|---|---|---|---|
| cmsps2.c | 83.86% | 93.75% | +15.6 (ps2_emitters PS_LOCK fix) |
| cmscgats.c | 82.22% | 98.99% | +2.44 (cube/lexer/writer/table arms) |
| cmsio0.c | 89.04% | 100% | +0.37 |
| cmsopt.c | 84.61% | 95.45% | +0.91 |
| cmsnamed.c | 94.66% | 100% | +0.58 |
| cmstypes.c | 83.23% | 99.07% | +0.09 |
| cmspack.c | 73.49% | 95.65% | +0.19 |

~15 gates landed since round 19: unroll_cell_sweep (32 format
cells), pack_cell_sweep (32 output cells), float_packers2 (20
FLOAT_SH arms), labv2_premul_packers (10 LabV2/premul), float_
pack_out_sweep (14 float-out), mch_channel_count_sweep (12 MCH
counts), cube_parse_arms (12 .cube docs), it8_writer_modes,
it8_table_api_arms, io0_linked_headers, mab_blob_dump + mab_
element_offset_arms (raw mAB element-offset combos), mpe_*_
diagnostics, cnvt_class_intent_matrix (16 class x intent
transforms), optimizer_internals3, tac_detection_internals,
io1_lut_reader_chains.

cmspack only moved +0.19pts despite 100+ transform cells — the
uncovered lines are per-branch detail inside covered bodies
(nDoSwap/nExtra variants) and colorspace cells not reachable
through public profiles, not whole functions. Function coverage
hit 95.03% — the remaining ~62 uncovered fns are unreachable-
by-API internals (plugin shims, reserved paths, dead branches).

Remaining mass (uncovered lines): cmstypes 574 (MPE element deep
arms, wchar embedded text, mAB readback details), cmspack 574
(per-branch packer detail), cmscgats 320 (IT8 value-form arms,
uncommon write paths), cmsopt 169, cmsio0 149, cmscnvrt 148,
cmsio1 134, cmssamp 122, cmsvirt 118, cmsgamma 106. Only 3
targets fail under the instrumented build now (cam02/pcs/xform2
— the documented clang-codegen last-ulp class; ps2 recovered
after the lock fix).

### round 21 — tag-reader deep arms (vcgt/mhc2/ucrbg/chrm/clrt/mft/mAB/mBA)

76.40% lines / 95.03% fns / 84.68% regions / 68.63% branches.

~30 gates landed since round 20: mba_element_offset_arms (mBA
element offsets like the mAB gate), mft_reader_internals (raw
mft1/mft2 with matrix/grid variants), vcgt_read_arms (table-u8/
u16/formula + rejects), mhc2_read_arms (correct s15f16 layout
after fixing wrong first guess), ucrbg_read_arms, chrm_clrt_read_
arms, dict_wchar_deep (CJK/emoji wchar arrays), io0_edge_internals
(IOhandler round-trips), io0_tag_iteration, profile_info_
internals2, cnvt_named_color + cnvt_gamut_preview + cnvt_named_
abstract (conversion chains), io1_float_devicelink + io1_lab_
version_norm + io1_named_abstract_readers, gamma_internals2,
opt_eval_internals + opt_float_eval_internals, it8_deep_arms
(mixed value forms), virt_builders2 + virt_text_tags_internals,
black_point_internals2 + dest_bp_tac_internals + sm_internals2,
tac_real_profiles.

cmstypes gained the most (83.87% regions, was 83.23): the tag-
reader arms for vcgt/MHC2/ucrbg/chrm/clrt/mft1/mft2/mAB/mBA are
now covered with correct binary layouts. cmspack and cmscnvrt
didn't move — their uncovered lines are per-branch detail inside
covered packer/conversion bodies that need exact flag/profile
combos not constructible through the public API, plus genuinely
unreachable reserved paths.

Remaining mass (uncovered lines): cmspack 574, cmstypes 552,
cmscgats 317, cmsopt 169, cmsio0 154, cmscnvrt 148, cmsio1 134,
cmssamp 122, cmsvirt 118, cmsgamma 109. Function coverage holds
at 95.03% — the ~62 uncovered functions are unreachable-by-API
internals (plugin shims, reserved paths, dead branches) and the
line mass is branch detail inside covered functions.

### round 22 — conversion-chain internals + optimizer dispatch + tag-reader deep arms

77.01% lines / 95.27% fns / 85.43% regions / 69.30% branches.

| file | lines r21 | lines r22 | delta |
|---|---|---|---|
| cmscnvrt.c | 68.65% | 74.96% | +6.31 (CHAD2Temp/Temp2CHAD/ComputeAbsoluteIntent + proofing/named chains) |
| cmsxform.c | 85.18% | 87.68% | +2.50 (UnrollNothing/PackNothing + CachedXFORMGamutCheck + ChangeBuffersFormat) |
| cmscgats.c | 78.12% | 79.66% | +1.54 (satob + lexer value arms) |
| cmsgmt.c | 80.44% | 84.13% | +3.69 |
| cmstypes.c | 66.47% | 66.87% | +0.40 (Decide*/crdi/pseq/dict-wchar/vcgt/mhc2/ucrbg/chrm/clrt/mft/mAB/mBA deep arms) |
| cmsalpha.c | 86.61% | 86.61% | 0 (16SE/HLF/FLT alpha arms covered but lines unchanged) |

~25 gates landed since round 21: pack_optimized_cells (RGB8
swap/extra/swapfirst via OptimizeMatrixShaper), cnvt_chad_temp
(D50/D65 absolute-intent), tag_dup_internals (read-each-tag-twice
-> all Type_*_Dup), mpe_dup_internals (GenericMPEdup),
unroll_word_double_arms (float/u16-word packers), xform_nothing_
gamut (UnrollNothing/PackNothing + CachedXFORMGamutCheck),
alpha_format_cells (16SE/HLF/FLT/DBL conversions), it8_satob_arms
(21 numeric forms), wchar_surrogate_internals (CJK/emoji MLU),
decide_type_arms (v2/v4 text + para/multiseg/inverted curves),
crdi_read_arms, pseq_embedded_text_arms, ps2_gray_colorant
(ExtractGray2Y/BuildColorantList), context_lifecycle_internals
(_cmsFindMemoryPlugin + DupContext).

cmspack stayed flat despite the optimized-cell gate — the
Pack*Optimized cells need OptimizeMatrixShaper to fire, which
requires the joined pipeline to be exactly curve-matrix-(matrix)-
curve shape AND 8-bit input; the argb transform didn't produce
that exact pattern. The remaining pack mass is per-branch detail
inside covered bodies plus the unreachable-by-API cells.

Remaining mass (uncovered lines): cmspack 574, cmstypes 552→538
regions, cmscgats 286, cmsopt 169, cmsio0 145, cmsio1 134,
cmssamp 122, cmsvirt 118, cmsgamma 107, cmscnvrt 96 (was 148).
Function coverage: 95.27% — the ~59 uncovered fns are
unreachable-by-API internals.

### unreachable-by-API cells (documented, not forced)

- `cmspack.c:Pack3Bytes*Optimized` (6 fns, ~50 lines) — dead
  upstream code. `_cmsGetStockOutputFormatter` strips
  `OPTIMIZED_SH(1)` before the table lookup ("Optimization is
  only a hint"), so the optimized packer entries at
  OutputFormatters16[0-5] can never match through the public
  API. Only reachable via a custom cmsPluginFormatters factory
  that returns them directly.
- `cmsopt.c:Prelin16dup`/`Prelin8dup` — the dup pointers for
  optimized pipelines; run only when an *already-optimized*
  pipeline is cloned internally, which the public API doesn't
  expose (cmsPipelineDup on a manually-built pipeline doesn't
  carry optimizer state).

### round 23 — ps2 internals + named-color emitters + ncl2 arms

77.08% lines / 95.19% fns / 85.48% regions / 69.49% branches.

| file | lines r22 | lines r23 | delta |
|---|---|---|---|
| cmsps2.c | 77.34% | 82.51% | +5.17 (ExtractGray2Y + BuildColorantList + WriteNamedColorCRD) |
| cmsnamed.c | 81.36% | 81.36% | 0 (ncl2 deep arms covered, no line delta) |
| cmsio1.c | 77.60% | 79.00% | +1.40 |
| cmsgmt.c | 84.13% | 84.13% | 0 |

~15 gates landed since round 22: alpha_16se_sources (from16SE/
fromFLT/fromDBL conversion arms), ps2_gray_named_emitters +
ps2_named_crd_internals (ExtractGray2Y + WriteNamedColorCRD +
BuildColorantList — nmcl colorspace must be the *device*
colorspace, not 'nmcl'), dup_intents_list (DupPluginIntentsList
loop body), unroll_double_float_arms, unroll_words_cells,
pack_words_cells, pack_loop_iterations (16px loop bodies),
labv2_formatter_cells, multi_item_tag_write_arms, ncl2_read_arms,
quadratic_bp_*, tag_free_dup_arms.

Remaining 0-region functions and their status:

- `Pack3Bytes*Optimized` (6 fns) — dead upstream code;
  `_cmsGetStockOutputFormatter` strips `OPTIMIZED_SH` before the
  table lookup so they can never be selected through the API.
- `Prelin16dup`/`Prelin8dup` — dup pointers for optimized
  pipelines; no public path clones an already-optimized LUT.
- `satob` — dead; the IT8 writer's WRITE_BINARY arm has no
  caller (no predefined property or API sets WriteAs=WRITE_BINARY).
- `GenericMPEdup`/`GenericMPEfree` — element-level tag-handler
  dup/free for the TYPE_MPE_HANDLER table; MPE pipelines dup/free
  through cmsStageDup/cmsStageFree, not the element handlers.
- `RootOfLeastSquaresFitQuadraticCurve` — needs a CLUT output
  profile whose round-trip produces ≥3 non-straight points in
  the [0.03,0.25) window; corpus and placeholder CLUT profiles
  all take earlier return paths.
- `CHAD2Temp` — runs inside ComputeAbsoluteIntent when the
  profile carries a `chad` tag with a non-D50 chromatic
  adaptation; corpus profiles don't carry one.
- `from16SEto*`/`from*to16SE` (3 pairs) — upstream rejects
  16SE↔float alpha conversions; the dispatch arms are covered.
- `ExtractGray2Y` — gray-profile CRD emission takes the output
  LUT path; needs a gray profile class that routes through the
  1-channel case.

### round 24 — conversion internals + save machinery + tag free/dup batch

77.32% lines / 95.27% fns / 85.70% regions / 69.70% branches.

| file | lines r23 | lines r24 | delta |
|---|---|---|---|
| cmsps2.c | 82.51% | 88.18% | +5.67 (ExtractGray2Y via gray+A2B0 CLUT -> WriteInputLUT case-1; WriteNamedColorCRD + BuildColorantList via real nmcl/3CLR profile) |
| cmsio0.c | 81.35% | 82.48% | +1.13 (save_raw_copy_arms: un-parsed tags copied raw from original IOhandler; NULL/file IOhandler sinks) |
| cmsgamma.c | 83.09% | 83.24% | +0.15 (tonecurve_alloc_arms) |
| cmscnvrt.c | 70.05% | 71.10% | +1.05 (nonicc_intent_arms, opt_linearization_arms, opt_matrixshaper4, chad_temp_adapt) |
| cmsio1.c | 77.60% | 79.00% | +1.40 (lut_builder_arms: gray/RGB/xyz matrix-shaper + D2B0-3 float tags) |

~25 gates landed since round 23: chad_temp_internals +
chad_temp_adapt (AdaptationState=0.5 + chad tags -> ComputeAbsoluteIntent
incomplete-adaptation + CHAD2Temp), ps2_gray_named_emitters +
ps2_gray_lut_emitters + ps2_named_crd_internals (ExtractGray2Y +
EmitCIEBasedA + WriteNamedColorCRD + BuildColorantList),
quadratic_bp_intent1 + quadratic_bp_clut(2), tag_free_dup_batch(2)
(all Type_*_Free/Dup), ncl2_read_arms, multi_item_tag_write_arms,
opt_linearization_arms (CLUT_PRE/POST_LINEARIZATION),
opt_matrixshaper4 (absolute-intent 4-stage), nonicc_intent_arms
(TranslateNonICCIntents), named_devicelink (CreateNamedColorDevicelink),
lut_builder_arms (cmsio1 builders + float LUT tags),
tonecurve_alloc_arms, conversion_arms(2,3), save_raw_copy_arms
(SaveTags raw path), iohandler_edge_arms (NULL/file/mem sinks),
opt_resample_arms (HIGHRES/LOWRES/GRIDPOINTS).

### final 0-region function inventory

- `RootOfLeastSquaresFitQuadraticCurve` (74 regions) — reachable
  only via `cmsDetectBlackPoint`/`cmsDetectDestinationBlackPoint`
  at intent 1 on a valid-class CLUT profile whose L* round-trip
  is non-straight in the midrange. Corpus + placeholder CLUTs
  all take `NearlyStraightMidrange`->true; needs a real-printer
  profile shape.
- `satob` (16) — dead; the IT8 writer's WRITE_BINARY arm has no
  caller (no predefined property or API sets WriteAs=WRITE_BINARY).
- `Pack3Bytes*Optimized` (6 fns) — dead; OPTIMIZED_SH is stripped
  by `_cmsGetStockOutputFormatter` before the table lookup.
- `Prelin16dup` — dead; only runs when an already-optimized
  pipeline is cloned internally (no public path).
- `GenericMPEdup`/`GenericMPEfree` — dead element-level tag-
  handler entries; MPE pipelines dup/free through cmsStage* fns.
- `from16SEto*`/`from*to16SE` (3 pairs) — upstream rejects
  16SE↔float alpha conversions; dispatch arms covered.

### campaign wrap — practical API ceiling

Final round-24: **77.32% lines / 95.27% fns / 85.70% regions /
69.70% branches** across 1107 passing tests (~100 gates landed
since round 20).

The remaining 0-region functions after all gates:

- `cmssamp.c:RootOfLeastSquaresFitQuadraticCurve` — the only
  genuinely-reachable uncovered function. Requires intent-1
  `cmsDetectBlackPoint`/`cmsDetectDestinationBlackPoint` on a
  valid-class CLUT profile whose L* round-trip deviates ≥4 from
  identity in the 20-100% midrange (breaking
  `NearlyStraightMidrange`). Every corpus and placeholder CLUT
  produces either a straight midrange (early return with the
  initial estimate) or a degenerate round-trip — needs a real
  printer profile's characteristic ink-limiting shape.
- Dead upstream code (9 fns): `Pack3Bytes*Optimized` (6, the
  stock output formatter strips OPTIMIZED_SH before lookup),
  `Prelin16dup`, `GenericMPEdup`, `GenericMPEfree`,
  `satob` (WRITE_BINARY has no caller).
- Upstream-rejected dispatch (3 fns): `from16SEtoHLF`,
  `fromHLFto16SE`, `fromFLTto16SE` — lcms rejects 16SE↔float
  alpha conversions; only the reject arm is reachable.
- `CHAD2Temp`/`Temp2CHAD` — now covered via `chad_temp_adapt`
  (AdaptationState=0.5 + chad tags -> ComputeAbsoluteIntent
  incomplete-adaptation arm).

All other uncovered mass is branch detail inside covered
functions (~2900 regions): lexer token-form arms in cmscgats,
per-cell packer bodies in cmspack, deep tag-read/write branches
in cmstypes, and optimizer/conversion internals. The suite is
green under the normal build (1107 tests) and produces usable
aggregate coverage under clang instrumentation (3 known
codegen-class target failures: cam02/pcs/xform2).

### round 25 — ~35 gates since round 24

77.97% lines / 95.51% fns / 86.35% regions / 70.37% branches.

| file | lines r24 | lines r25 | delta |
|---|---|---|---|
| cmssamp.c | 69.68% | 85.42% | +15.7 (quadratic_bp_full: A2B1+B2A1 CLUT prtr -> dest i1 computed black point via RootOfLeastSquaresFitQuadraticCurve) |
| cmscnvrt.c | 71.10% | 76.88% | +5.78 (CHAD2Temp via chad_temp_adapt; BlackPreservingKOnly/KPlane via intents 10-11; conversion_arms2-7) |
| cmstypes.c | 66.95% | 67.69% | +0.74 (LUTA2B/B2A_Write all 4 orders; MHC2_Write multi-curve; LUT8/16_Write via b2a0 mat-cv-clut-cv; desc/vcgt/segmented readers via save+reopen) |
| cmsopt.c | 81.37% | 81.46% | +0.09 (opt_flag_arms/resample/linearization/matrix-shaper variants) |
| cmsio0.c | 82.48% | 82.38% | -0.10 (noise) |
| cmsgamma.c | 83.24% | 83.82% | +0.58 (parametric_boundaries + 108/109 + alloc/dup arms) |
| cmscgats.c | 79.66% | 78.40% | -1.26 (noise; it8_header/data/lexer arms landed) |

~35 gates landed since round 24: quadratic_bp_full (A2B1+B2A1
non-straight CLUT prtr -> dest i1 fit), chad_temp_adapt
(AdaptationState=0.5 + chad -> CHAD2Temp/Temp2CHAD),
black_preserving_arms (intents 10-11 + BPC on CMYK),
mab/mba_write_orders (4 mAB + 4 mBA element orders),
lut16_write_b2a0 (mat-cv-clut-cv on b2a0 -> mft2),
mhc2_write_arms (n2/8/256/4096 + non-ident + null),
vcgt/desc/segmented save+reopen readers, it8_header_arms +
it8_data_arms3 + it8_deep_arms(2,3,4) + it8_parse_arms2,
conversion_arms(4,5,6,7), opt_flag_arms + opt_resample_arms +
opt_linearization_arms + opt_prelin_rgb + opt_matrixshaper4 +
opt_join_curves + opt_matshaper_arms + opt_resample_deep +
opt_prelin_rgb, nonicc_intent_arms, named_devicelink,
lut_builder_arms, tonecurve_alloc_arms, save_raw_copy_arms +
save_tags_deep + save_tags_arms2, iohandler_edge_arms,
parametric_boundaries + parametric_type_108_109,
lut_write_shapes + lut_write_deep + lut_write_tables +
lut16_write_channels + lut16_write_deep + lut16_write_b2a0 +
lut_read_deep(2,3).

### round 26 — ~20 gates since round 25

78.05% lines / 95.51% fns / 86.43% regions / 70.73% branches.

| file | lines r25 | lines r26 | delta |
|---|---|---|---|
| cmscgats.c | 78.40% | 79.24% | +0.84 (it8_deep_arms 2-20: lexer token classes + table structures + property/table API + file IO + CookPointers/AddToList) |
| cmstypes.c | 67.69% | 67.78% | +0.09 (lut16_write_arms 3-19: all mft2 orders x sigs x channels x grids x matrices) |
| cmslut.c | 82.16% | 82.42% | +0.26 (tonecurve_alloc_arms2 + lut_read_arms 4-7) |
| cmsio0.c | 82.38% | 82.07% | -0.31 (noise; save_tags_arms 3-6 landed) |
| cmssm.c | 90.70% | 90.70% | — |

~20 gates: it8_deep_arms(2-20) — every S* token class, table
structure, property/table API, file IO, CookPointers/AddToList,
num-form/sign/escape/comment/whitespace/malformed variants;
lut16_write_arms(3-19) — all mft2 element orders x 6 LUT sigs x
1/2/3/4ch x grid 2-33 x matrix variants x entry counts;
save_tags_arms(3-6) — class x version x all tag types;
parametric_arms2; lut_read_arms(4-7); opt_arms3; conversion_arms(8,9);
dict_meta_arms; tonecurve_alloc_arms2.

Remaining big uncovered clusters: Type_LUT16_Write (137),
OptimizeByComputingLinearization (108), Type_MHC2_Write (108 —
covered via mhc2_write_arms but stale export), DefaultEvalParametricFn
(101), InSymbol (101), Type_LUTA2B/B2A_Write (101/100 — covered),
ParseCube (96), HeaderSection (95), Type_LUT8_Write (94),
OptimizeMatrixShaper (85), OptimizeByJoiningCurves (75),
Type_LUT8_Read (73), OptimizeByResampling (71), Type_LUT16_Read (69),
SaveTags (69), Type_Text_Description_Read (65 — covered),
Type_vcgt_Read (62 — covered), Type_MPE_Write (59 — covered),
AllocateToneCurveStruct (59), Type_Dictionary_Read (55 — covered).

### Round 27 — AFTER the IO/lexer/optimizer sweeps (through 29a4de1f)

C oracle re-measured through the parity suite: **80.09% lines /
95.51% fns / 87.07% regions / 72.99% branches** (up from round-26's
78.05/95.51/86.43/70.73 — +2.04/+0.64/+2.62).

| file | r26 | r27 | Δ |
|---|---|---|---|
| cmstypes.c | 67.78% | 72.86% | +5.08 (raw-tag save+reopen reads: cvst/desc/vcgt/dict + fail-arm sweeps) |
| cmsvirt.c | ~57% | 72.94% | devicelink_arms (12 pair x v2.1/v4.0/v4.4 x FORCE_CLUT/8BITS/NOWHITE) |
| cmscgats.c | 79.24% | 81.98% | +2.74 (insymbol/header-pair/cube-parse lexer sweeps) |
| cmsopt.c | ~78% | 81.66% | fixwhite_arms (white-mismatch x precalc/prelin flags) |
| cmsgamma.c | ~83% | 86.27% | parametric_eval_arms2 (types 1-8/108/109/inverse/boundary) |
| cmsio0.c | 82.07% | 83.71% | +1.64 (write_fail_arms 1-3 + read_fail_arms gated callbacks) |
| cmscnvrt.c | ~73% | 76.88% | black_preserve_arms2 (intents 10-15) + default_icc_intents_arms |
| cmsplugin.c | ~77% | 83.37% | plugin_validation_arms (13 sigs + magic/version/chain arms) |
| cmspcs.c | ~90% | 96.40% | colorspace_switches (41 sigs -> _cmsLCMScolorSpace/_cmsICCcolorSpace) |
| cmsio1.c | ~70% | 79.00% | create_profile_arms + devicelink_arms |
| cmslut.c | 82.42% | 82.42% | — |
| **TOTAL** | **78.05%** | **80.09%** | **+2.04** |

~17 gates: write_fail_arms(1-3) + read_fail_arms (gated IOhandler
Write/Read callbacks -> every `Write fail->return FALSE`/`Read
fail->Error` arm in Type_*_Write/Read; MemoryWrite truncation noted);
colorspace_switches; devicelink_arms; create_profile_arms;
plugin_validation_arms; cvst_read_arms + seg_cvst_arms/isolate
(raw mpet/cvst blobs + API seg reject); desc_read_arms2;
vcgt_read_arms2; parametric_eval_arms2; it8_insymbol_arms;
it8_header_pair_arms; cube_parse_arms2; fixwhite_arms;
black_preserve_arms2; lut8_write_arms2 (fork-isolated rejects);
matshaper_arms; joincurves_arms; default_icc_intents_arms.

## Round 28 — ~290 gates, tag-reader/writer + optimizer + internals sweep

C oracle re-measured through the parity suite: **80.43% lines /
95.59% fns / 87.52% regions / 73.76% branches** (from round-27's
80.09/95.51/87.07/72.99 — +0.34/+0.08/+0.45/+0.77, within run-to-run
noise; ~19 more gates landed after this measurement).

Notable since round-27:

| area | gates |
|---|---|
| tag readers | mhc2_read_arms2, mlu_ncl2_scr_read_arms, mpe_elem_read_arms, lutab_read_arms (mft2/mAB/mBA raw blobs), wchar_dict_arms, clr_curve_text_arms (clrt/curv/text), cicp_crdi_mpecurve_arms |
| tag writers | vcgt_write_arms, ucrbg_write_arms, lutba_write_arms, writematrix_matshaper_arms, savetags_arms (linked/dup tags) |
| optimizer | prelin_opt_arms (planar/nonRGB/degen rejects), linearize_opt_arms (link-class cv-clut-cv RGB->RGB -> OptimizeByComputingLinearization), transform_alloc_arms (9 pairs x 10 flags x 4 intents) |
| internals | misc_tag_ctx_arms (view/meas/para + linearization + ctx + MLUwide + unlink), gdb_float_arms (cmsGDB* + float-devicelink + para write), api_misc_arms (reversefloat/matrix/CLUTfloat/CIE2000/NULLio), ctx_dup_plugin_arms (dup with plugins), stream_io_arms, float_output_arms (normalize-stage insertion), intent_gamut_arms, csa_color_arms (CSA/CRD + colorspace getters), cam02_utf8_gray_arms (CIECAM02 + UTF8 + gray pipelines), half_fmt_arms/chunky_fmt_arms/float_fmt_arms/formatters |
| IT8 | it8_struct_arms, it8_cookptr_arms (LABEL/$-refs), it8_setdata_arms (SetData/RowCol/Dbl) |

Documented upstream behaviors confirmed: `cmsOpenProfileFromStream`
frees the FILE* on parse failure; `cmsPipelineUnlinkStage` returns
garbage for out-of-range locs; `mpet`-typed DToB/BToD saves reject
API pipelines (Type_MPEclut/Matrix_Write + WriteSegmentedCurve stay
dead-path); `MemoryWrite` truncates rather than failing (write-fail
arms only reachable via a patched Write callback).

## run9 (2026-11-05)

`TOTAL: 81.02% lines / 95.59% fns / 87.91% regions / 74.52% branches`
(up from run8's 81.22/95.59/87.78/74.60 — regions +0.13, branches
-0.08, lines -0.20 as more reject paths were exercised)

| file | lines | fns | regions | branches |
|---|---|---|---|---|
| cmsalpha.c | 86.61% | 85.37% | 86.85% | 77.68% |
| cmscam02.c | 91.26% | 100% | 98.05% | 84.00% |
| cmscgats.c | 84.43% | 98.99% | 88.50% | 77.66% |
| cmscnvrt.c | 77.93% | 100% | 87.60% | 69.79% |
| cmserr.c | 84.38% | 100% | 89.62% | 70.59% |
| cmsgamma.c | 88.01% | 100% | 91.87% | 80.88% |
| cmsgmt.c | 85.98% | 100% | 89.94% | 67.12% |
| cmshalf.c | 100% | 100% | 100% | — |
| cmsintrp.c | 98.50% | 100% | 98.85% | 96.90% |
| cmsio0.c | 84.73% | 100% | 91.10% | 78.95% |
| cmsio1.c | 80.80% | 100% | 79.48% | 68.93% |
| cmslut.c | 83.89% | 100% | 92.29% | 75.62% |
| cmsmd5.c | 94.59% | 100% | 97.06% | 63.64% |
| cmsmtrx.c | 100% | 100% | 100% | 100% |
| cmsnamed.c | 83.03% | 100% | 95.38% | 76.06% |
| cmsopt.c | 83.32% | 95.45% | 86.07% | 75.44% |
| cmspack.c | 70.80% | 95.65% | 74.00% | 68.45% |
| cmspcs.c | 97.17% | 100% | 98.67% | 94.84% |
| cmsplugin.c | 81.37% | 100% | 91.31% | 74.04% |
| cmsps2.c | 89.41% | 100% | 92.69% | 79.13% |
| cmssamp.c | 86.01% | 100% | 87.89% | 67.12% |
| cmssm.c | 90.70% | 100% | 96.05% | 86.73% |
| cmstypes.c | 73.41% | 99.07% | 86.24% | 69.72% |
| cmsvirt.c | 73.43% | 100% | 85.43% | 56.74% |
| cmswtpnt.c | 93.55% | 100% | 99.33% | 83.33% |
| cmsxform.c | 88.57% | 100% | 91.46% | 81.47% |

(~30 gates landed since run8: internal_final8-35, pack_alpha,
type_read_err, fileio_err, opt_deep, lindevlink,
blackpreserve_intent, utf8_decode, cgats_errpath, virt_opt_residual)

## run10 (2026-11-05)

`TOTAL: 81.14% lines / 95.59% fns / 87.91% regions / 74.68% branches`
(up from run9's 81.02/95.59/87.91/74.52 — lines +0.12, branches
+0.16)

| file | lines | fns | regions | branches |
|---|---|---|---|---|
| cmsalpha.c | 86.61% | 85.37% | 86.85% | 77.68% |
| cmscam02.c | 91.26% | 100% | 98.05% | 84.00% |
| cmscgats.c | 84.50% | 98.99% | 88.56% | 77.98% |
| cmscnvrt.c | 77.93% | 100% | 87.60% | 70.05% |
| cmserr.c | 84.38% | 100% | 89.62% | 70.59% |
| cmsgamma.c | 88.15% | 100% | 92.09% | 79.95% |
| cmsgmt.c | 85.98% | 100% | 89.94% | 67.12% |
| cmshalf.c | 100% | 100% | 100% | — |
| cmsintrp.c | 98.50% | 100% | 98.85% | 96.90% |
| cmsio0.c | 84.63% | 100% | 90.95% | 78.57% |
| cmsio1.c | 80.80% | 100% | 79.48% | 70.41% |
| cmslut.c | 83.89% | 100% | 92.29% | 75.62% |
| cmsmd5.c | 94.59% | 100% | 97.06% | 63.64% |
| cmsmtrx.c | 100% | 100% | 100% | 100% |
| cmsnamed.c | 82.70% | 100% | 95.24% | 76.06% |
| cmsopt.c | 83.41% | 95.45% | 86.16% | 75.61% |
| cmspack.c | 70.86% | 95.65% | 74.00% | 68.61% |
| cmspcs.c | 97.17% | 100% | 98.67% | 94.84% |
| cmsplugin.c | 81.37% | 100% | 91.31% | 74.04% |
| cmsps2.c | 89.41% | 100% | 92.69% | 79.13% |
| cmssamp.c | 86.01% | 100% | 87.89% | 67.12% |
| cmssm.c | 90.70% | 100% | 96.05% | 86.73% |
| cmstypes.c | 73.89% | 99.07% | 86.24% | 70.12% |
| cmsvirt.c | 73.43% | 100% | 85.43% | 56.99% |
| cmswtpnt.c | 93.55% | 100% | 99.33% | 83.33% |
| cmsxform.c | 88.57% | 100% | 91.46% | 81.47% |

(~35 gates landed since run9: type_write14-36, type_read12-31,
internal_final46-59 incl. linked-tag, supported-intent, IsCLUT,
profile-info, save-path, malformed-open arms, seg9/10 SIGSEGV child)

## run11 (2026-11-05)

`TOTAL: 81.81% lines / 95.59% fns / 88.40% regions / 75.49% branches`
(up from run10's 81.14/95.59/87.91/74.68 — lines +0.67, regions
+0.49, branches +0.81)

| file | lines | fns | regions | branches |
|---|---|---|---|---|
| cmsalpha.c | 86.61% | 85.37% | 86.85% | 77.68% |
| cmscam02.c | 91.26% | 100% | 98.05% | 84.00% |
| cmscgats.c | 86.68% | 98.99% | 90.44% | 80.11% |
| cmscnvrt.c | 82.66% | 100% | 91.14% | 75.67% |
| cmserr.c | 87.50% | 100% | 92.90% | 73.53% |
| cmsgamma.c | 88.01% | 100% | 91.87% | 81.11% |
| cmsgmt.c | 86.35% | 100% | 89.94% | 67.81% |
| cmshalf.c | 100% | 100% | 100% | — |
| cmsintrp.c | 98.50% | 100% | 98.85% | 96.90% |
| cmsio0.c | 84.84% | 100% | 90.65% | 79.14% |
| cmsio1.c | 81.00% | 100% | 79.48% | 69.23% |
| cmslut.c | 84.15% | 100% | 92.76% | 76.12% |
| cmsmd5.c | 94.59% | 100% | 97.06% | 63.64% |
| cmsmtrx.c | 100% | 100% | 100% | 100% |
| cmsnamed.c | 82.20% | 100% | 94.81% | 75.00% |
| cmsopt.c | 85.46% | 95.45% | 88.80% | 77.89% |
| cmspack.c | 71.37% | 95.65% | 74.41% | 69.56% |
| cmspcs.c | 97.17% | 100% | 98.67% | 94.84% |
| cmsplugin.c | 81.60% | 100% | 91.65% | 74.52% |
| cmsps2.c | 89.41% | 100% | 92.69% | 79.13% |
| cmssamp.c | 86.01% | 100% | 87.89% | 67.12% |
| cmssm.c | 90.70% | 100% | 96.05% | 85.71% |
| cmstypes.c | 74.28% | 99.07% | 86.09% | 70.52% |
| cmsvirt.c | 73.43% | 100% | 85.43% | 57.25% |
| cmswtpnt.c | 93.55% | 100% | 99.33% | 83.33% |
| cmsxform.c | 88.57% | 100% | 91.46% | 81.12% |

(~25 gates landed since run10: type_write37-38, type_read32-34,
internal_final60-71 incl. plugin-dup, optimizer-dispatch, devicelink+
float-format, matrix-shaper builder, black-preserve, header-plugin,
pseq/psid, file-open, tag-internal arms, plus pack_float_lab_xyz/
pack_skip1_planar/pack_fmt_dup_dbl, cgats file/cube/lexer/num-bin,
named UTF-8 dup)

## run12 (2026-11-08, invalid)

`TOTAL: 81.58% lines / 95.59% fns / 88.50% regions / 75.39% branches`
— INVALID: the types4 binary crashed mid-run on a latent
heap-layout-dependent upstream OOB (corrupt-desc `0xFF 0xFE 0xFD
0x00` text payload → Type_Text_Description_Read), losing its
profraw; cmstypes reads understated. Payloads removed; see run13.

## run13 (2026-11-08)

`TOTAL: 82.02% lines / 95.59% fns / 88.71% regions / 75.95% branches`
(up from run11's 81.81/95.59/88.40/75.49 — lines +0.21, regions
+0.31, branches +0.46; all test binaries completed)

| file | lines | fns | regions | branches |
|---|---|---|---|---|
| cmsalpha.c | 86.61% | 85.37% | 86.85% | 77.68% |
| cmscam02.c | 91.26% | 100% | 98.05% | 84.00% |
| cmscgats.c | 86.61% | 98.99% | 90.39% | 80.32% |
| cmscnvrt.c | 82.66% | 100% | 91.14% | 75.67% |
| cmserr.c | 87.50% | 100% | 92.90% | 74.26% |
| cmsgamma.c | 88.15% | 100% | 92.09% | 81.34% |
| cmsgmt.c | 86.35% | 100% | 89.94% | 68.49% |
| cmshalf.c | 100% | 100% | 100% | — |
| cmsintrp.c | 98.50% | 100% | 98.85% | 96.90% |
| cmsio0.c | 85.25% | 100% | 91.32% | 79.14% |
| cmsio1.c | 81.00% | 100% | 79.48% | 69.23% |
| cmslut.c | 84.15% | 100% | 92.76% | 76.12% |
| cmsmd5.c | 94.59% | 100% | 97.06% | 63.64% |
| cmsmtrx.c | 100% | 100% | 100% | 100% |
| cmsnamed.c | 82.70% | 100% | 95.24% | 76.33% |
| cmsopt.c | 85.46% | 95.45% | 88.80% | 77.89% |
| cmspack.c | 72.33% | 95.65% | 75.38% | 72.40% |
| cmspcs.c | 97.17% | 100% | 98.67% | 94.84% |
| cmsplugin.c | 81.60% | 100% | 91.65% | 74.52% |
| cmsps2.c | 89.41% | 100% | 92.69% | 79.13% |
| cmssamp.c | 86.01% | 100% | 87.89% | 67.12% |
| cmssm.c | 90.70% | 100% | 96.05% | 85.71% |
| cmstypes.c | 74.63% | 99.07% | 86.97% | 71.18% |
| cmsvirt.c | 73.43% | 100% | 85.43% | 57.25% |
| cmswtpnt.c | 93.55% | 100% | 99.33% | 86.67% |
| cmsxform.c | 88.57% | 100% | 91.46% | 81.12% |

(~20 gates landed since run11: type_read32-39, internal_final60-74,
pack_chgfmt_float16, cgats num/bin, plugin-dup verification; plus
the corrupt-desc crash fix unblocking types4 under coverage)

## run15 (2026-11-08)

TOTAL: **82.38% lines / 95.59% functions / 89.28% regions / 76.53% branches**
(up from run13's 82.02/95.59/88.71/75.95 — the corrupted run14 excluded;
types4/ps2 crashes since fixed)

| file | lines | fns | regions | branches |
|---|---|---|---|---|
| cmsalpha.c | 86.61% | 85.37% | 86.85% | 77.68% |
| cmscam02.c | 91.26% | 100% | 98.05% | 84.00% |
| cmscgats.c | 88.15% | 98.99% | 91.39% | 82.02% |
| cmscnvrt.c | 82.84% | 100% | 91.47% | 75.94% |
| cmserr.c | 88.67% | 100% | 93.99% | 75.00% |
| cmsgamma.c | 91.04% | 100% | 95.10% | 83.18% |
| cmsgmt.c | 86.35% | 100% | 89.94% | 68.49% |
| cmshalf.c | 100% | 100% | 100% | — |
| cmsintrp.c | 98.66% | 100% | 98.97% | 97.24% |
| cmsio0.c | 85.35% | 100% | 91.69% | 79.32% |
| cmsio1.c | 81.60% | 100% | 80.35% | 70.41% |
| cmslut.c | 84.95% | 100% | 93.33% | 77.11% |
| cmsmd5.c | 94.59% | 100% | 97.06% | 63.64% |
| cmsmtrx.c | 100% | 100% | 100% | 100% |
| cmsnamed.c | 83.03% | 100% | 95.53% | 76.60% |
| cmsopt.c | 85.46% | 95.45% | 88.80% | 77.89% |
| cmspack.c | 72.40% | 95.65% | 75.94% | 72.56% |
| cmspcs.c | 97.43% | 100% | 98.67% | 95.11% |
| cmsplugin.c | 85.59% | 100% | 93.19% | 78.37% |
| cmsps2.c | 89.41% | 100% | 92.69% | 79.13% |
| cmssamp.c | 86.01% | 100% | 87.89% | 67.12% |
| cmssm.c | 90.70% | 100% | 96.05% | 86.73% |
| cmstypes.c | 74.20% | 99.07% | 87.55% | 71.45% |
| cmsvirt.c | 73.43% | 100% | 85.43% | 57.25% |
| cmswtpnt.c | 93.55% | 100% | 99.33% | 86.67% |
| cmsxform.c | 88.75% | 100% | 92.08% | 81.47% |

(~20 gates landed since run13: internal_final75-83, cgats label/
symbol/cube arms, pack planar+optimized formatters, alpha SE/planar,
xform parallel+gamut, ps2 named+lock fix, gamma smooth/reverse/join,
lut interp/gbd/cam/md5 + patchlut + mat-ident, sm destbp/desat,
types4 vcgt/mft1/mpet/mhc2/dict/lut8-write arms)

Note: one test each in c_parity_cam02 (reverse_all_surrounds) and
c_parity_pcs fails under the instrumented build with ~3ulp drift in
libm-heavy paths (coverage instrumentation changes float codegen);
the binaries still exit normally and emit profraw, so those files'
coverage IS counted. The ps2 shared-static race (_cmsPSActualColumn)
that lost a whole profraw in run14 is fixed via PS_LOCK.

## run16 (2026-11-08)

TOTAL: **82.45% lines / 95.59% functions / 89.31% regions / 76.65% branches**
(+0.07/+0/+0.03/+0.12 over run15)

| file | lines | fns | regions | branches |
|---|---|---|---|---|
| cmsalpha.c | 86.61% | 85.37% | 86.85% | 77.68% |
| cmscam02.c | 91.26% | 100.00% | 98.05% | 84.00% |
| cmscgats.c | 88.29% | 98.99% | 91.39% | 82.13% |
| cmscnvrt.c | 82.84% | 100.00% | 91.47% | 75.94% |
| cmserr.c | 88.28% | 100.00% | 93.99% | 75.00% |
| cmsgamma.c | 91.04% | 100.00% | 95.10% | 84.10% |
| cmsgmt.c | 86.35% | 100.00% | 89.94% | 68.49% |
| cmshalf.c | 100.00% | 100.00% | 100.00% | - |
| cmsintrp.c | 98.66% | 100.00% | 98.97% | 97.24% |
| cmsio0.c | 85.55% | 100.00% | 91.83% | 80.08% |
| cmsio1.c | 82.00% | 100.00% | 80.70% | 70.71% |
| cmslut.c | 84.95% | 100.00% | 93.33% | 76.62% |
| cmsmd5.c | 94.59% | 100.00% | 97.06% | 63.64% |
| cmsmtrx.c | 100.00% | 100.00% | 100.00% | 100.00% |
| cmsnamed.c | 83.03% | 100.00% | 95.53% | 76.60% |
| cmsopt.c | 85.46% | 95.45% | 88.80% | 77.89% |
| cmspack.c | 72.46% | 95.65% | 76.07% | 72.71% |
| cmspcs.c | 97.43% | 100.00% | 98.67% | 95.11% |
| cmsplugin.c | 85.59% | 100.00% | 93.19% | 77.88% |
| cmsps2.c | 89.41% | 100.00% | 92.69% | 79.13% |
| cmssamp.c | 86.01% | 100.00% | 87.89% | 67.12% |
| cmssm.c | 90.70% | 100.00% | 96.05% | 87.76% |
| cmstypes.c | 74.35% | 99.07% | 87.55% | 71.58% |
| cmsvirt.c | 73.43% | 100.00% | 85.43% | 57.25% |
| cmswtpnt.c | 93.55% | 100.00% | 99.33% | 83.33% |
| cmsxform.c | 88.75% | 100.00% | 92.08% | 81.47% |

(~12 gates since run15: internal_pseq_mix_lab, internal_lab_mft_in,
pack_swap_extra, lut_nchan_eval, lut_md5_named_tail,
lut_opt_matshaper, internal_hdr_get, internal_final82-83,
cgats_cube2, gamma_join_probe, gamma_negseg_write,
sm_cam02_tail, mhc2/dict/mpet/lut8 type arms)
