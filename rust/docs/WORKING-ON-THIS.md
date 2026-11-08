# Working on this port — read before your first change

Adapted from `zenav1-svt/rust/docs/WORKING-ON-THIS.md`; the same traps apply.

## Landing (gitlink preservation — MANDATORY)

jj working copies **do not model git submodules**. A `jj describe` commit
omits `reference/lcms2` from the tree — pushing it would DELETE the submodule
pin upstream. Verified 2026-10: every jj-described commit drops `160000
reference/lcms2`; graft it back before pushing:

```bash
tools/land.sh "commit message"   # describe+graft+push+verify in one step
```

It grafts via `git commit-tree` (index synced to worktree + gitlink staged),
points `main` at the graft, `jj git import`s, pushes, and prints `GITLINK OK`.
If you must do it by hand: `git update-index --add --cacheinfo
160000,$(git -C reference/lcms2 rev-parse HEAD),reference/lcms2`, then
`git write-tree`/`git commit-tree` onto the pushed tip. **Never** `jj git
push` a tip whose tree lacks the gitlink.

## The one-minute loop

```bash
cd rust && cargo test --workspace          # builds the C oracle once, then all gates
cargo test -p lcms --test c_parity_pcs     # one file's gate
```

The oracle rebuilds only when the submodule SHA changes (stamp in OUT_DIR).

## The loop for a new module

1. Read the C file. Port it into `crates/lcms/src/<module>.rs`, 1:1, with the
   C function names mapped in doc comments (`/// `cmsXYZ2Lab``).
2. Add extern decls to `lcms-cref/src/lib.rs` for every exported/`_cms` symbol
   you need to compare against. For `static`/`cmsINLINE` code, add a shim to
   `lcms-cref/shims/`.
3. Write `tests/c_parity_<module>.rs`: dense sweeps (deterministic xorshift,
   no rand dep), edge values, and **exhaustive** coverage wherever the domain
   allows it. Compare `to_bits()` — never epsilon-tolerance a parity claim.
4. A function the gate can't reach is not ported. Pin it or defer it — say which.

## Port discipline

- **1:1 with the C source.** Same operation order, same casts, same
  constants. Where C is wrong, be wrong the same way (lcms has few genuine
  bugs; record any in `docs/SUSPECTED-C-BUGS.md` and reproduce them).
- **No transcriptions of the oracle.** Never re-implement a C function on the
  *test* side; call the compiled symbol. Two transcriptions diverge.
- **`Option<&Context>` = `cmsContext`**, `None` = NULL (global context).
  Error paths that C reports via `cmsSignalError` go through `err::signal_error`
  — keep the exact condition, and note where C asserts vs returns.
- **Return `Option`/`bool` mirroring `cmsBool`+out-params.** Output structs
  are return values instead of `*mut` out-params — semantics unchanged.
- **`cmsAssert`**: in release C it compiles out. Port asserts that guard a
  real contract as `debug_assert!`; NULL checks the C performs unconditionally
  get ported as real checks.
- **Generate tables, never hand-transcribe.** `half_tables.rs` and
  `signatures.rs` come from `tools/gen_*.py`. Regenerate on rebase.

## Numeric traps (each cost a bug)

- `sin_cos()` — see PORTING.md: GCC merges `cos`/`sin` pairs into `sincos`.
- `pow(x, n)` lowering: `0.5`→sqrt, integer exps stay libm calls, const bases
  fold. `.powf(0.5)` vs `.sqrt()` both worked — but verify via the gate.
- `_cmsQuickFloor` is bit magic, not `floor()` (PORTING.md).
- `1.0/x`, `a/b*c` order matters; C precedence slips (e.g. a dropped paren in
  `pow(x,7) / (pow(x,7) + pow(25,7))` computing `1 + 25^7`) are caught only by
  the gate, not the compiler.
- NaN/Inf propagate differently through `as`-casts vs C casts.

## Evidence tiers — say which one you have in the commit message

1. differential vs the exported C symbol (standard), 2. byte-identity of
   whole outputs, 3. same-source shim, 4. hand-derived (state why nothing
   better exists).

## Adding a fix? Add its cell

Every divergence the gate catches is a candidate for a pinned regression cell.
Commit the failing input, not just the fix.

## Refusals

Where the port can't yet do what C does, return an error — never emit a
plausible-but-wrong result. Catalogue refusals in `docs/REFUSED.md` with what
C does for that input.

## Coverage campaign (ccov lane, 2026-11)

`just coverage-cref` instruments the C oracle (clang) and unions profraws
across every `c_parity_*` binary — the "which C paths have a gate" number.
gcc `-O2 -fno-strict-aliasing` stays the parity contract; clang numbers are
a floor (crashed binaries lose whole profraws — atexit never fires).

Gates: `c_parity_{alpha,virt2,sm,xform2,cgats2,ioerr,pack,types,err}` +
`c_parity_ps2` mutex. TOTAL C lines 52.2%→61.3%, fns 62.7%→76.9%.
Per-file table + the 13 port bugs the gates caught: `docs/COVERAGE.md`.

Remaining ungated (in missed-lines order): cmstypes MPE handlers beyond
built profiles, cmslut stage evaluators, cmscgats multi-table rest,
cmsio0 IOhandler edge cases, cmsgmt, cmscnvrt intent pipelines,
cmsplugin (plugin struct registration — needs FFI struct construction;
`cmsGetProfileInfo*`/`cmsGetSupportedIntents` are unported API gaps).

## Open divergence — pinned (found by corpus_ps2_matrix, needs port fix)

**`labv4lcms2.icc` CSA intent 0**: C emits `/Table [ 33 33 33 [` in
`WriteInputLUT`, Rust emits `/Table [ 2 2 2 [`. Root cause located in
`opt.rs::optimize_by_resampling`: `n_grid=2` is selected when
`lut.stage_count()==0`, and Rust's `xform.lut` for the profile→Lab4 link
has an empty `elements` list (opt_data-only pipeline) where C's
`v->Lut` retains stages → C samples at `reasonable_gridpoints`=33.
Divergence is in the transform/link path for Lab-input profiles, not ps2
itself. The corpus_ps2_matrix extension was reverted pending the fix;
the 4 pre-existing ps2 tests remain green.

## Coverage campaign status (through aef97a6e)

19 parity gates: alpha, virt2, sm, xform2, cgats2 (+multi-table),
ioerr, pack, types, err, lut, gamma, named, opt, pcs, io0b. 13 port
bugs found and fixed by the gates. Coverage TOTAL ~61% lines / ~76%
fns through the parity suite (run-to-run ±3pp from lost profraws on
clang-instrumented crashes). All suite binaries green.

Remaining ungated surface: cmsgmt (~41%), cmscnvrt rest (~60%),
cmstypes MPE/exotic tags beyond built profiles (~47.6%), cmserr rest,
cmsplugin rest, cmsio1 remaining getters, cmslut rest, cmsio0 rest,
cmsps2 rest. Unported APIs deferred: cmsGetProfileInfo*, cmsGetSupportedIntents.

## Coverage campaign — gates through 2013851e

30 parity gates total. Newest: io0b (save-to-mem reserialize + datetime),
lut extension (reverse-float, float-CLUT sampling, stage accessors),
virt3 (linearization/ink-limiting/gray builders), io1c (header getters
+ devicelink LUT eval), gmt (GBD compute variants + is_clut matrix),
xform3 (line-stride + buffer-format + adaptation), err2 (context
lifecycle + error-handler), plugin (4 plugin types), named extension
(MLU utf8/wide/translation/dup), gamma extension (dup/join/smooth),
types extension (viewing-conditions tag write/read).

13 port bugs found and fixed by the gates; ~3 test-misuse bugs found and
corrected (IT8 buffer size, MLU UTF-8 NUL-termination, created-field
mask offset). One cosmetic divergence pinned (labv4 CSA 'cvst' vs 'idn '
pipeline shape — identical output, different optimize path shape).

Coverage ~59-61% lines / ~75-77% fns through the parity suite (floor;
clang profraw loss on crashed binaries). Remaining: cmstypes MPE/exotic
tag handlers (~48%), cmscnvrt link-construction variants (~60%),
cmsps2 emission variants (~72%), cmsio0 IOhandler edge cases (~68%),
cmsxform remaining workers (~59%), unported APIs (cmsGetProfileInfo*,
cmsGetSupportedIntents).

## Blocked: cmsps2 corpus x flags matrix

Sequential `cmsGetPostScript*` calls carry `_cmsPSActualColumn` (C's
process-global static, reset only per CLUT emission) — C's output for
the Nth call differs from an isolated call's where text wraps. Rust's
emitter is per-call. A corpus x flags matrix therefore can't byte-
compare: upstream's static-column behavior vs the port's per-call
emitter are semantically equivalent but textually different on wrap
positions. The flag paths (BPC/LOWRESPRECALC/HIGHRESPRECALC/
NOWHITEONWHITEFIXUP) are exercised per-resource on sRGB by `srgb_crd`;
the corpus adds no new flag-path coverage.

- [finding] `do_transform` does not dispatch FLOAT_SH input/output
  formats — writing f32 through the transform buffer produces garbage
  (the float-formatter dispatch path is unported in the Rust
  do_transform path). C's cmsDoTransform handles FLOAT_SH(1)|BYTES_SH(4)
  formats natively. Pin the gate and revisit.

- [finding] BCHSW abstract-profile transform eval diverges: C gives
  [65535, 39932, 58070] vs Rust [65535, 0, 20770] on Lab input
  [0x4000,0x8000,0x2000]. The MPE/float-LUT reader produces wrong
  values on the BCHSW profile's mAB/mpet pipeline. Pinned in
  c_parity_virt4; the create parity still exercises the readers.

- [CORRECTED] The EXTRA_SH/DOSWAP/u16-format eval "divergences"
  were a test-side endianness bug: C's cmsDoTransform takes
  native u16s (as_ptr()) while the tests passed be-bytes. With
  native-endian input all formats match — see c_parity_pack2.



- [CORRECTED] The u16-format eval "divergence" was the same
  test-side endianness bug — all formats pass with native-endian
  input (c_parity_pack2). The rd16 bounds panic on premul short
  inputs is a real (minor) bounds check gap in pack.rs.

## Round 2 — API-surface sweep (through ceae9b03)

**66.13% lines / 83.72% functions** — the exported `cms*` API
surface is now fully exercised (API diff against lcms2.h shows
only types/constants ungated). ~34 gates added: misc batches 1–7
(channel counts, CMM version, header CMM/mfr, pipeline/stage ctx
IDs, unlink + save8-flag, transform getters + alarm codes,
supported intents, segmented curves, reverse/TempFrom/adaptation,
dict iterate, open variants, matrix-shaper, formatters, triple-free,
gamut-check pipeline, check-and-retrieve, unregister plugins,
cube devicelink, gamma detect, stream IO, THR builders/transforms,
colorant getters, PostScript CSA/CRD, IOhandler variants, the
last builders), plus plugin matrix (all 11 sigs incl. memH/mtxH/
parH/typH/mpeH), named-color stage, slice-space-float, MPE mixed
stages, stage channels, Lab/XYZ elements, premul + modifier matrix,
transform2devicelink flags, flag branches, colorspace-mismatch
reject, dense GBD, wtpnt, gamma3, raw tags, find-format, IT8 file
IO, desaturate/detect.

Line-level gaps remain in cmspack (~135 formatter variants —
status covered, not every eval), cmstypes (Type_* Read/Dup/Free
on unusual payloads), cmsio1 (tag-serialization internals),
cmscgats (parser error paths), cmscnvrt (intent-link internals).

## Round 3 — internals sweep (through aa37e5b3)

**66.46% lines / 84.04% functions** measured after the API
sweep; ~20 more gates landed since (cross-format/planar/packed
evals, 33 crafted Type_*_Read cases, all-tag sweep, unusual
class chains, devicelink transform, last IT8 surface,
change-buffers, linked tags, meas/scrn/rXYZ/kTRC/bkpt/chrm/
lumi/tech/ciis/clro sigs, v2/v4 type variants, edge payloads).

Every exported `cms*` function is exercised; every `cmsIT8*`
function; 33 Type_* read paths via crafted raw tags; both
formatter sides (Unroll*/Pack*) across 17x17 format pairs;
v2-vs-v4 stored-type dispatch ('text'/'desc'/'curv' vs
'mluc'/'mluc'/'para').

Remaining line-level gaps: cmspack (~49%, the ~135 formatter
variant bodies beyond the eval'd combos), cmstypes (~51%, the
MPE/pipeline serialization internals + Dup/Free on exotic
payloads), cmsio1 (~64%, tag-serialization internals),
cmscnvrt/cmsgats/cmsvirt (~66-67%).

Known divergences (pinned in tests): linked-tag serialization
(C shares the data block, 524b; Rust writes a separate 20b
block, 544b); BCHSW abstract-profile eval (float-LUT reader
produces wrong values). Port bugs fixed this campaign: 17
(decide_* type-mismatch accepts, cgats Skip-vs-Check +
isidchar/isfirstidchar strictness, plus the earlier 15).
Deferred: 'view'/'calt'/'vcgt'/'gamt' writes segfault on the
display-class profile payload shapes; cmsIT8LoadFromIOhandler
is commented out in lcms2.h (not a real API).

## Round 4 — internals deep-sweep (through 138ce24c)

**67.84% lines / 85.73% functions.** The exported API surface
is fully exercised — ~195 parity gates + 17 port bugs fixed.
cmspack's formatter-variant bodies (~52%) and cmstypes' MPE
serialization internals (~54%) are the remaining gaps —
reachable only through profile shapes the public API can't
produce (mAB writes need valid 5-slot pipelines no builder
emits) or oracle-UB inputs.

## Round 5 — remaining-internals sweep (through 6aeb6b5d)

**68.37% lines / 86.29% functions.** ~257 parity gates. The
Type_*_Read dispatch is fully swept (65 type sigs), the
writeable sig surface is exhausted, the parser-validation +
LUT + named-color + IT8 + SEQ + dict + MLU + MPE + plugin +
formatter machinery is all exercised. cmspack's ~135
formatter-variant bodies (~52%) and cmstypes' MPE serialization
internals (~56%) are the remaining gaps — reachable only
through profile shapes the public API can't produce.

## Round 6 — internals sweep 2 (through 1e86da39)

**68.00% lines / 85.89% functions.** ~305 parity gates. Every
Type_*_Read is swept (65 type sigs), the writeable-sig surface
is exhausted, the parser-validation + LUT + named-color + IT8
+ SEQ + dict + MLU + MPE + plugin + formatter + alpha +
stage + devicelink machinery is all exercised. cmspack's ~135
formatter-variant bodies (~52%) and cmstypes' MPE/mAB
serialization internals (~56%) are the remaining gaps —
reachable only through profile shapes the public API can't
produce or through oracle-UB inputs (the mAB writer NULL-derefs
on non-canonical pipelines).

## Round 7 — cmspack format sweep (through ee90d50a)

**68.09% lines / 86.13% functions.** ~315 parity gates.
cmspack is now 53.42% — the packed/float/half/u16/u8 ×
planar/doswap/swapfirst/extra/endian16/flavor/premul ×
BGRA/RGBA/ARGB/ABGR/BGR/gray-alpha/Lab-byte-order matrix is
exercised. The remaining cmspack gap is the per-format
Unroll*/Pack* body internals for the less-common pixel types
(15-color formats, 2-byte-per-channel packed, named-color
formats) — each needs its own format-code gate.

## Round 8 — io0/io1/cgats sweep (through 9b252daf)

**68.37% lines / 86.21% functions.** ~337 parity gates.
cmscgats up to 71.67%, cmsio0 to 77.36%, cmsnamed to 75.54%.
The remaining uncovered lines are deep internals reachable
only through oracle-UB inputs, profile shapes the public API
can't produce (real CMYK/MCH device profiles), or per-format
packer bodies for the less-common pixel types.

## Coverage campaign checkpoint (run17)

**83.06% lines / 95.59% fns / 89.56% regions / 77.34% branches**
(upstream oracle `reference/lcms2`, ~860 parity gates landed
across `c_parity_*.rs`)

Residuals are dominated by three buckets:
1. **OOM-only `goto Error` cleanup** — `if (!alloc) goto Error` /
   `if (Mem == NULL) return NULL` branches throughout every file.
   Unreachable without allocation-failure injection; documented
   dead-for-coverage.
2. **Genuinely dead code** — `satob`/`WRITE_BINARY`/
   `WRITE_HEXADECIMAL` in cmscgats (no property registers
   them), `ReadOneWChar`'s `wchar_t==2` path (Linux wchar_t=4),
   a handful of `#if 0` blocks.
3. **Format-word / tag-type variant bodies** — each cmspack
   formatter has per-variant misses (swap/planar/extra-channel
   arms) that need exact `T_*` bit patterns; still being swept.

Remaining per-file missed lines (run17): cmstypes ~1000,
cmspack ~420, cmscgats ~167, cmsvirt ~161, cmsopt ~148,
cmsio0 ~136, cmslut ~111, cmsnamed ~102, cmsio1 ~90,
cmscnvrt ~85, cmsplugin ~65, cmsxform ~63, cmsgamma ~49,
cmssamp ~48, cmsps2 ~43, cmsgmt ~37, cmserr ~29, cmsalpha ~30,
cmssm ~20, cmsmd5 ~10, cmsintrp ~8, cmspcs ~10.

## Round — mpet/mAB internals correction sweep (through a082c845+)

Run18 coverage (floored — 4 binaries had instrumented-build test
failures: ps2 x17 shared-static race, types4/io0d/cam02/pcs x1
codegen diffs): 79.87% lines / 95.03% fns / 87.72% regions /
74.07% branches. Run17 remains the clean checkpoint at
83.06/95.59/89.56/77.34.

**Two structural fixes discovered this stretch:**

1. **`cmsPipelineInsertStage` loc must be `cmsAT_BEGIN`(0) or
   `cmsAT_END`(1)** — earlier gates passed `-1`, which silently
   returns FALSE with no insert. ~92 earlier `type_write*` gates
   were writing EMPTY pipelines (still hit `Type_MPE_Write`
   head/count path but NOT `WriteMPEElem`/`Type_MPEcurve_Write`/
   `Type_MPEmatrix_Write`/`Type_MPEclut_Write` element bodies).
   Fixed in write96-99; real element-write internals now covered
   (u16clut HasFloatValues reject, matf+offsets f32 write,
   cvst WriteSegmentedCurve arms, full cvst+matf+fclut
   write+read-back, identity/XYZ2Lab TypeHandler==NULL rejects,
   segmented curves types 0/6/7/8 + type-5 reject).

2. **MPE element bodies are FLOAT32, not u8/f64.**
   - `clut` element = sig8 + in u16 + out u16 + dims[16] +
     nEntries*f32 — NO precision byte (lcms2's MPE clut is
     float-only).
   - `matf` element = sig8 + in u16 + out u16 + (in*out + out)*f32.
   - `_cmsReadFloat32Number` rejects NaN/±inf/>1e20 and
     non-normal non-zero (fpclassify must be FP_ZERO or
     FP_NORMAL) — earlier u8/f64 "clut/matf reads" were silently
     exercising these sanity rejects or garbage-but-valid paths.

3. **`bACS` (0x62414353) / `eACS` (0x65414353) have NULL
   ReadPtr** — skipped mid-chain per spec (earlier tests used
   wrong lowercase sigs and hit the unknown-element reject
   instead).

4. **mAB/mBA layout** = 32-byte header (in u8 + out u8 + pad16 +
   5 offsets B/Mat/M/C/A). mAB inserts A→C→M→Mat→B; mBA inserts
   B→Mat→M→C→A. Curve counts: mAB A=inch, M/B=outch; mBA
   B/M=inch, A=outch. Elements: curves = nCh embedded
   'curv'/'para' + 4-align; matrix = 9+3 s15f16 (always 3×3 —
   inserted into non-3ch pipelines without an in/out check);
   clut = 16 grid + 1 prec + 3 pad + entries (prec 1|2).
   ReadEmbeddedCurve accepts 'curv'/'para' only.

5. **Segmented-curve wire format**: outer 'curf' (0x63757266),
   nSegments u16, (n-1)×f32 breakpoints, then per-segment
   'parf' (0x70617266, types 0-2 → internal 6-8, params 4/5/5
   f32) or 'samf' (0x73616D66, count+points f32; implicit first
   point fixed via eval). Write side: seg types 0/6/7/8 legal,
   5 → Type-6<0 reject.

6. `cmsPipelineAlloc(16,3)` returns NULL (channel cap 15).
   `cmsSaveProfileToMem`'s `nn` reports buffer capacity, not
   bytes written (quirk — earlier "n=131072/262144" prints were
   capacity, not size).

7. Element offsets in builder closures must be recorded BEFORE
   pushing data (one gate was briefly broken by post-push
   offsets).

8. **MHC2 is NOT an MPE container** — it is Microsoft's HDR
   type: `CurveEntries u32` (≤4096, 0 fails via calloc),
   `MinLuminance`/`PeakLuminance` s15f16, then 4 u32 offsets
   (matrix, R, G, B). Matrix offset 0 → identity; nonzero →
   12 × s15f16 (3×4). Each curve offset → 'sf32' + 8-byte base
   skipped, then CurveEntries × s15f16. Type AND tag sig are
   BOTH `'MHC2'` = 0x4D484332 (uppercase — lowercase 'mhc2'
   rejects at IsTypeSupported).

## Round — per-function residual query + api-driven arms (through d6bb40c4)

New process: `llvm-cov-14 report <bin> -object <bins> -instr-profile=bench/out/coverage-cref/all.profdata -show-functions <src>` gives per-function
region/line/branch residuals — much better targeting than the
floored run18 totals.

**Coverage drivers discovered:**

- `cmspack` format-word matrix: BYTES[0..2] CHANNELS[3..6]
  EXTRA[7..9] DOSWAP[10] ENDIAN16[11] PLANAR[12] FLAVOR[13]
  SWAPFIRST[14] COLORSPACE[16..20] FLOAT[22] PREMUL[23].
  PT codes: GRAY=3 RGB=4 CMY=5 CMYK=6 XYZ=9 LAB=10
  MCH1-15=15-29 LABV2=30. Each Unroll*/Pack* variant needs
  its exact combo as in/out format. (c_parity_pack.rs older
  tests use local PT_MCH5..10 = 12..17 — upstream is 19..24;
  channels still matched.)
- `cmsTransform2DeviceLink` (bound in lcms-cref) drives
  FixColorSpaces/SetTextTags/FindCombination/optimize/
  identity-curve inserts/LabV2ToV4+LabV4ToV2 fixups (Lab
  PCS + Version<4) / 8BITS_DEVICELINK flag / FORCE_CLUT /
  CreateNamedColorDevicelink. Best single cmsvirt driver.
- Named-color input: class 'nmcl' (0x6E6D636C), colorspace
  '1CLR' (0x31434C52), ncl2 tag; input format
  TYPE_NAMED_COLOR_INDEX = (1<<3)|2 = 0x0A. Gives
  CreateNamedColorDevicelink + cmsGetNamedColorList.
- `cmsLinkTag(p, sig, dest)` creates linked tag-table
  entries → SetLinks/SaveTags arms on save.
- IO handlers: `cmsOpenIOhandlerFromNULL/Mem/File/Stream`,
  `cmsSaveProfileToIOhandler` returns byte count (132 for
  minimal profile). NULL/small/RO handlers exercise
  NULLWrite/MemoryWrite/Seek/Tell error arms.
  `cmsOpenProfileFromIOhandler2THR(ctx, io, write=1)` for
  rw opens. `cmsGetProfileIOhandler` returns the handler.
- Preserve-K intents 10-15 → BlackPreservingKOnlyIntents/
  KPlaneIntents + samplers; intents 4-9 reject. Proofing
  transforms at same intents hit more arms.
- `cmsDupContext` → DupPlugin*List (opt/formatters/intents).
  `cmsStageDup`/`cmsPipelineDup` → CurveSetDup/CLUTElemDup/
  MatrixElemDup (all 12 stage kinds dup cleanly).
- `cmsPluginHeader`: base{magic 'acpp', expected, type
  'hdrH', next} + ICCVersion + ReadPtr + WritePtr —
  `_cmsReadUInt32Number` usable inside hooks; NULL hooks
  registration rejects.
- Multiprofile chains through abst profiles (labv2/v4,
  xyzlcms2) insert Lab↔XYZ stages → EvaluateLab2XYZ/
  EvaluateXYZ2Lab evaluators.
- Device class sigs are NOT validated on save (all write);
  profile version >4.4 or <0 rejects on save.
- `cmsSaveProfileToMem`'s `nn` reports buffer capacity,
  not bytes written (earlier "n=131072" prints were
  capacity).

**Real ICC profiles in `rust/`:** sRGBlcms2/aRGBlcms2
(mntr RGB matrix-shaper), lcms2cmyk.icc (prtr CMYK→Lab
A2B0-2+B2A0-2), lcms2link*/limit/lin (link CMYK→CMYK),
gray3lcms2 (mntr GRAY kTRC), nullcms2 (prtr GRAY B2A0),
glablcms2 (gray kTRC), xyzlcms2 (abst XYZ), labv2/v4 (abst
Lab A2B0), bchs/bright (mntr RGB A2B0 only — B2A direction
rejects). cmsOpenProfileFromFile needs `b"r\0"`.

**Remaining residuals (run18 floored):** cmstypes ~69%,
cmspack ~74%, cmsps2 ~64% (shared-static race under
parallel tests), cmsvirt ~74%, cmsopt ~83%, cmsnamed ~84%,
cmsio1 ~81%, cmscnvrt ~81%, cmsio0 ~81%, cmsplugin ~83%,
cmscgats ~88%.

## Alloc-fail injection (through 9f69c03c)

`alloc_fail_arms` registers a counting cmsPluginMemHandler
(sig 'memH' 0x6D656D48, struct = base + Malloc/Free/Realloc/
MallocZero/Calloc/Dup fn ptrs) that returns NULL at call N,
then sweeps N under `fork()` isolation so crashing depths
don't kill the test. Result: **fail@1 SIGSEGVs upstream**
(cmsOpenProfileFromFileTHR's first-alloc failure path is
unchecked — a real OOM-unsafety finding, NOT a parity bug);
all other depths error cleanly. Every `if (x == NULL) return/
goto Error` arm across the codebase is now hittable — extend
the depth sweep to tag reads, transform creation, profile
saves, IT8 parses, cmsTransform2DeviceLink, gamma/pipeline/
stage construction for hundreds more error arms.

**Next targets:** cmsps2 emitters (GenerateCSA 61%,
WriteInputLUT 67%, Emit1Gamma/NGamma/EmitIntent arms via
cmsGetPostScriptCSA on mntr/LUT/gray/named profiles),
cmscgats IT8 (GetTable/AllocChunk/WriteHeader/SetDataFormat/
AddToList branches via odd header/table files), cmsnamed
(decodeUTF8/AddMLUBlock/GrowMLUtable+pool/SearchMLUEntry via
edge UTF-8 + many-lang MLUs), cmssm (GetPoint/QuantizeToSector
via cmsGBD point configs), cmsopt (PatchLUT 45%,
OptimizeByJoiningCurves, PrelinOpt16alloc — devicelink+CLUT
and odd-shape pipelines through optimize flags).

## Round — per-function residuals + api-driven arms 2 (through 67f846b7)

**run19** (floored — 5 binaries failed under instrumentation:
xform2 livelocked at 1110% CPU, plus the run18-known set):
81.21% lines / 93.91% fns / 87.66% regions / 75.14% branches.
per-fn map = `llvm-cov-14 report <bins>
-instr-profile=bench/out/coverage-cref/all.profdata
-show-functions <src>` — run19 profdata is freshest.

**Coverage drivers discovered this round:**

- `cmsMLUsetUTF8` is the decodeUTF8 driver — needs a
  DISTINCT lang per call (same lang+country rejects via
  SearchMLUEntry-found arm). Invalid UTF-8 accepted except
  >U+10FFFF (0xF4 0x90 0x80 0x80).
- Plugin sigs that register cleanly: 'intH' (intent + Link
  fn + desc[256]), 'parH' (nFunctions + FunctionTypes[20] +
  ParamCounts[20] + Evaluator), 'inpH' (factory), 'optH'
  (opt fn), 'memH', 'hdrH', 'xfmH' (single union ptr =
  _cmsTransform2Factory: fn(xform*,ud*,fd*,lut*,fi*,fo*,fl*)
  -> i32 that sets *xform). NULL xfmH factory rejects.
- `cmsDupContext` after registering plugins exercises
  DupPlugin*List on each list (TransformList needs a valid
  xfmH factory).
- `cmsSliceSpace16/Float(n, gridpts, sampler, cargo)` — the
  sampler gets (In, Out, cargo) per grid point; In/Out can
  be null on edge calls — guard the fn.
- `cmsPipelineEvalReverseFloat(target, result, hint, lut)`
  — 4 args; only 3-channel pipelines reverse.
- Stage ctor types: Lab2XYZ='x2l ', XYZ2Lab='l2x ',
  LabV2ToV4/V4ToV2='matf', V2ToV4curves='cvst',
  Identity='idn ', NamedColor='ncl '.
- `cmsPipelineCheckAndRetreiveStages` is variadic
  (n sigs then n out-ptrs).
- `cmsIT8SetDataFormat(-1)` aborts on an _cmsAssert —
  not an error arm.
- `cmsDetectTAC` returns 0.0 on all bundled profiles
  (ink-only semantics).
- `cmsMLUsetUTF8`/`cmsMLUsetASCII` reject same
  lang+country twice (dup found arm).
- Alpha: EXTRA_SH>0 + COPY_ALPHA(0x4000000) — copy vs
  zero-fill differs per format/width/planar; extra-count
  mismatch between in/out rejects under COPY_ALPHA.
- Alloc-fail: second crash depth fail@8 on mpet-read
  (open path is fail@1).

**Remaining residuals (run19 floored):** cmstypes ~74%,
cmspack ~73%, cmsps2 ~68%, cmsvirt ~73%, cmsio1 ~80%,
cmsxform ~76%, cmsnamed ~83%, cmsopt ~85%, cmscnvrt ~83%,
cmsio0 ~85%, cmsplugin ~85%, cmscgats ~88%, cmslut ~85%,
cmsgmt ~81%, cmssamp ~86%.

## cmstypes residual map (run19, through fad65191)

Biggest missed functions (regions-missed / %):
- Type_MHC2_Write 47/51% — CurveEntries + offset-table +
  min-peak-lum arm variants
- Type_LUT8_Write 36/73%, Type_LUT16_Write 34/80%,
  Type_LUT16_Read 23/77%, Type_LUT8_Read 21/79%,
  Type_LUTA2B_Write 14/87%, Type_LUTB2A_Read 15/78%,
  Type_LUTA2B_Read 14/79%
- Type_Text_Description_Read 22/71% — 'desc' reader arms
- Type_vcgt_Write 19/67%, Type_vcgt_Read 14/81%
- Type_MPE_Write 18/75%, Type_MPEclut_Write,
  WriteSegmentedCurve 14/78%, ReadSegmentedCurve 15/84%
- Type_ProfileSequenceDesc_Write 16/54%
- _cmsReadWCharArray 16/30% — wide-char reader internals
- Type_UcrBg_Read 14/74%, Type_UcrBg_Free
- Type_NamedColor_Read 14/67%
- Type_Dictionary_Read 14/82%
- Type_MHC2_Read 14/74%

Correct per-fn query sort: column 3 = regions-MISSED
(sort -k3 -rn), column 4 = regions-%.

## Round — tag-reader/write-fail sweep (through a55a2877)

- `cmsMHC2Type` real layout: CurveEntries(u32) +
  Red/Green/BlueCurve ptrs + MinLum + PeakLum +
  XYZ2XYZmatrix[3][4] — curve ptrs BEFORE luminance.
- `Type_UcrBg` tag AND type sigs are both 'bfd '
  (0x62666420), NOT 'ucrb' — body: u32 ucr-count + u16 ucr
  + u32 bg-count + u16 bg + trailing ASCII; desc >32000
  rejects.
- `ncl2` body: u32 vendor + u32 count + u32 nDeviceCoords
  + prefix[32] + suffix[32] + per-color(root[32] +
  pcs[3] + dev[n]); nDeviceCoords>cmsMAXCHANNELS rejects.
- dict lives under 'meta' tag (0x6D657461), type 'dict':
  u32 Count + u32 Len(16|24|32 else reject) + records of
  (off,size) pairs [name,value,+displayName,+displayValue]
  + data; embedded MLU is the BARE body — Count u32 then
  RecLen u32(=12) then records; string offsets relative
  to mlu_start-8 (>= SizeOfHeader+8 = 20 for count=1).
- `Type_MLU_Read` order is Count then RecLen — embedded
  dict MLUs must match; Offset&1 and Offset<SizeOfHeader+8
  and Offset+Len>SizeOfTag+8 all reject.
- `cmsSaveProfileToMem` nn is input-only cap; NULL MemPtr
  queries needed size. MemoryWrite TRUNCATES overflow
  (upstream colord fix) — capacity sweeps cannot hit
  io-fail arms; use /dev/full (cmsSaveProfileToFile) or a
  failing custom handler.
- WriteCLUT in mAB rejects FLOAT CLUTs (8/16-bit only) —
  float CLUTs serialize only via MPE. A2B write shapes:
  {cvst}, {cvst,matrix,cvst}, {cvst,clut,cvst},
  {cvst,clut,cvst,matrix,cvst}.
- mft1/mft2 writers consume [mat3x3?]cvst clut cvst in
  order; empty/non-3x3/non-uniform-grid/trailing-stage
  reject; mft1 full-shape needs the matrix stage.
- cmsFreeProfileSequenceDescription owns assigned MLUs;
  profile owns the seq after cmsWriteTag — freeing both
  double-frees.
- pseq write: v2 -> 'desc' embedded (no read-back path),
  v4 -> 'mluc' embedded (round-trips).
- Instrumented builds livelock on parallel test binaries
  (pack/xform2 at >500% CPU) — kill the spinning pid,
  the run proceeds; totals floor accordingly.

## Run20 (floored — c_parity_pack killed mid-run)

Merged manually (script stalled after pack died):
`bench/out/coverage-cref/all.profdata`, 67 profraws.

Floored totals: cmspack 49.71% (killed binary carries
most pack coverage), cmsxform 74.64%, cmspcs 71.98% —
all measurement loss, not real regressions.

Accurate deltas vs run19: cmstypes 74.59% (+0.39),
cmslut 84.69%, cmsio0 85.14%, cmsio1 80.20%,
cmsnamed 83.19%, cmsps2 67.98%, cmsvirt 72.77%,
cmsgmt 80.81%, cmscnvrt 82.49%, cmsopt 84.88%.

Surrogate finding: lone UTF-16 surrogates reject
upstream at open — convert_utf16_to_utf32 edge arms
unreachable through the public mluc path.

## Findings — MPE write curve-rep bug (upstream)

`Type_MPE_Write` on a pipeline containing a TABULATED
tone curve (`cmsBuildTabulatedToneCurve16`) CRASHES
upstream: `WriteSegmentedCurve` derefs `g->Segments`
unconditionally ("NO CHECK IS PERFORMED ON VALIDITY"
per the source comment) — tabulated curves have
`nSegments=0`/`Segments=NULL`. Parametric curves
(`cmsBuildGamma`) reject cleanly; segmented curves
(`cmsBuildSegmentedToneCurve`, `Type=0` 'samf') write.

Smallest repro: `cmsStageAllocToneCurves` on a
`cmsBuildTabulatedToneCurve16` curve -> pipeline ->
`cmsWriteTag(D2B0)` -> `cmsSaveProfileToMem` ->
SIGSEGV in `WriteSegmentedCurve` (cmstypes.c:4437).

MPE curves must be segmented (Type=0 'samf' or 'parf'
parametric segments) to serialize. Rust port behavior
must match upstream (crash is upstream behavior —
document, don't "fix").
