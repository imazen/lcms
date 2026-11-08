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
