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
| 8 | `cmslut.c` | `lut` | done — all stage types + pipeline + Newton reverse |
| 9 | `cmsio0.c` | `io` | IO layer done (handlers + primitives); profile/tag machinery next |
| 10 | `cmstypes.c` | `tagtypes` | next — all tag type handlers (~6.3k C lines) |
| 11 | `cmsmd5.c` | `md5` | done (computeID waits on cmsio0) |
| 12 | `cmsnamed.c`, `cmsio1.c` | `named` | cmsnamed done; io1 pending |
| 13 | `cmspack.c` | `pack` | pixel formatters (~4k lines) |
| 14 | `cmsalpha.c` | `alpha` | alpha channel copy |
| 15 | `cmsxform.c` | `xform` | transforms, cache, proofing |
| 16 | `cmsopt.c` | `opt` | optimizer — reproduces C's *decisions* |
| 17 | `cmscnvrt.c` | `cnvrt` | profile linking, intents, BPC |
| 18 | `cmsvirt.c` | `virt` | sRGB/Lab/XYZ/abstract/BCHSW virtual profiles |
| 19 | `cmscam02.c` | `cam02` | done |
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

## State @ 7940ce6 (cmsvirt landed)

DONE (C-file → rust module, all differential-gated):
cmsmtrx.c→mtrx, cmspcs.c→pcs, cmswtpnt.c→wtpnt, cmshalf.c→half,
cmsintrp.c→intrp, cmsgamma.c→gamma, cmsmd5.c→md5 (+computeID), cmslut.c→lut,
cmscam02.c→cam02, cmsnamed.c→named, cmsio0.c→profile (full: header, tag dict,
links, raw tags, 2-pass save w/ blind-copy, file/mem open+save),
cmstypes.c→tagtypes (ALL 37 handlers + SupportedTags + MPE + plugin chunks),
cmsvirt.c→virt (all ctors incl OkLab/NULL/BCHSW), io/plugin primitives→io+context+err.

Gates: 74 tests across 12 files; byte-exact serialization verified on real
testbed .icc files + virtual profiles.

REMAINING C files (by priority):
cmscnvrt.c (1247) transform construction — NEXT
cmsxform.c (1513) transform engine
cmspack.c (4066) pixel formatters
cmsopt.c (2006) optimizer
cmsio1.c (1045) profile-info helpers
cmssm.c (736) gamut check
cmsalpha.c (650) alpha xform
cmssamp.c (602) sampling (partially deferred)
cmscgats.c (3339) CGATS
cmsps2.c (1635) PostScript
cmserr.c/cmsplugin.c (residual pieces)
cmstypes→decide/LUTtype helpers in cmsio1?

Key lessons (all in git history): jj drops gitlinks (land.sh grafts);
sincos merge; C wrapping int ops; wchar truncation (no WCHAR_MAX);
'curf' not 'segf'; Mat3.v row-major; text handlers share cmsMLU*;
header offsets version@8/magic@36/profileID@84; SaveTags blind-copies
unmodified tags + relinks + re-decides types; Keep-restore of dir offsets.

## State @ eede859 (cmspack landed)

DONE now also: cmspack.c→pack (ALL ~120 formatters + 4 stock tables +
_cmsGetFormatter; oracle-driven via fabricated info header). 77+ tests.

BUG TRAIL additions: gray unrolls broadcast wIn[0..3]; T_COLORSPACE&31;
FROM_16_TO_8=(v*65281+8388608)>>24; is_ink=CMY|CMYK|MCH5..15;
cref stamp keys shim contents now.

NEXT: cmsxform.c (engine: Transform struct, 16/float eval loops,
cmsCreateExtendedTransform → cnvrt link + pack formatters + opt),
then cmsopt.c (2k), cmssamp.c (black point/TAC via transforms),
finish cnvrt K-intents. cmsio1/cnvrt are IN and compile.
cmssamp/cmsps2/cmscgats/cmsalpha remain.

## State @ f26b62a (cmsxform skeleton landed)

DONE: +cmspack.c→pack (all ~120 formatters), cmsio1.c→io1 (LUT readers),
cmscnvrt.c→cnvrt (link machinery, K-intents stubbed), cmsxform.c→xform
(Transform struct + eval loops + create_extended_transform).

REMAINING: cmsopt.c (optimizer hooks into create_extended_transform —
NOOPTIMIZE path works, optimizer selection not wired), cmssamp.c
(black-point/TAC — needs working transforms), cnvrt K-intents (needs
cmssamp+precalc stages), cmsxform precalculated/cached variants +
cmsCreateProofingTransform gamut, cmsalpha.c, cmscgats.c, cmsps2.c,
cmssm.c GBD API, cmserr/cmsplugin residuals.

## State @ d4773f3 (END-TO-END TRANSFORM PARITY)

WORKING: link_profiles→optimize→pack formatters→eval = byte-identical
sRGB→sRGB 16-bit vs C. cmsxform engine live. cmsopt complete.
cmssamp black-point ported (darker-colorant + perceptual-black +
dest ramp/quadratic). cmsio1 LUT readers, cmscnvrt link machinery.

BUG FIXED (important): Pipeline::cat must clone stage WITHOUT its
next-tail (dup already did; cat didn't) — nested rest-of-chain under
every appended stage, the eval-order corruption behind the 0.66× drift.

REMAINING: cmsxform precalculated/cached xform variants + proofing
transform + cmsChangeBuffersFormat + alarm/gamut; cmscnvrt K-only/
K-plane intents (need cmsDetectTAC + precalc stages); cmsalpha.c;
cmscgats.c; cmsps2.c; cmssm.c GBD API; cmserr/cmsplugin residuals;
then: wider parity (formats x intents x profiles), perf parity.

## State @ 69e6540 (cmsxform complete — all workers)

cmsxform.c DONE: Worker dispatch (Precalculated/Cached/GamutCheck/Null
x 16|float), AllocEmptyTransform, HandleExtraChannels, all 3 stride
APIs, cmsChangeBuffersFormat, GetXFormColorSpaces/IsProperColorSpace,
gamma-detect NOOPTIMIZE gate, SetWhitePoint/NormalizeXYZ, alpha-copy
check, CAN_CHANGE_FORMATTER. Context: adaptation_state + alarm_codes.
sRGB->sRGB still byte-identical with the full rewrite.

NEXT: BlackPreservingKOnlyIntents (cmscnvrt.c:732) + KPlane (:965) —
the two remaining stubs in cnvrt.rs. Each ~230 lines; needs
cmsDetectTAC + precalculated K-curves + BuildGrayOutputPipeline/
BuildRGB2CMYK helpers (also in cmscnvrt). Then cmssamp TAC /
cmsalpha / cmscgats / cmsps2 / cmssm GBD for full coverage.

## State @ fe086fa (cmscnvrt complete — all intents)

BlackPreservingKOnlyIntents + BlackPreservingKPlaneIntents + full
support: TranslateNonICCIntents, is_cmyk_devicelink, GrayOnlySampler,
slice_space16, detect_tac (EstimateTAC over Lab->profile transform),
ComputeKToLstar/_cmsBuildKToneCurve, PreserveKPlaneParams sampler with
eval_reverse_float + TAC limiting + deltaE. dup_profile bridges the
transform's owned-vec signature.

Remaining modules: cmssamp.c residual (cmsDesaturateLab, gamut
checks), cmsalpha.c, cmscgats.c, cmsps2.c, cmssm.c GBD API,
cmserr/cmsplugin residuals. cmsio0/io1/lut/gamma/pcs/wtpnt/intrp/
md5/cam02/half/named/tagtypes/virt/pack/opt/xform/cnvrt/sm DONE.

## State @ 38165e2 (cmssm + cmsalpha done)

cmssm.c → gbd.rs: 16x16 spherical-sector gamut boundary descriptor
(ToSpherical/ToCartesian/QuantizeToSector/ClosestLineToLine/Spiral/
InterpolateMissingSector). cmsGBDAlloc/Free/AddPoint/CheckPoint/Compute.

cmsalpha.c → alpha.rs: the 6x6 FormattersAlpha matrix (8/16/16SE/HLF/
FLT/DBL conversions), ComputeIncrementsForChunky/Planar, the real
_cmsHandleExtraChannels (xform delegates to it; Transform gains
dw_flags for the COPY_ALPHA check).

DONE end-to-end: cmsio0 io1 lut gamma pcs wtpnt intrp md5 cam02 half
named tagtypes virt pack opt xform cnvrt sm gbd alpha. The full
profile->transform->eval path is byte-identical on sRGB->sRGB.

NEXT: _cmsCreateGamutCheckPipeline (cmsgmt.c:287 — needs GAMUTCHAIN
transforms + GamutSampler + cmsSliceSpace16; gates FLAGS_GAMUTCHECK in
xform), cmsDesaturateLab (cmsgmt.c:515), cmscgats.c (3339 lines —
IT8 file format, biggest remaining module), cmsps2.c (1635 lines —
PostScript CSA/CRD), then parity expansion + perf.

## State @ 5bbd0d8 (cmsgmt core + gamut-check wired)

cmsDesaturateLab, _cmsCreateGamutCheckPipeline (3-transform proof
chain -> 3->1 deltaE CLUT), GamutSampler. xform's FLAGS_GAMUTCHECK
now fully functional — gamut profile + PCS position produce a real
gamut-check pipeline feeding the Cached/PrecalculatedGamutCheck
workers.

COMPLETE modules: cmsio0 io1 lut gamma pcs wtpnt intrp md5 cam02
half named tagtypes virt pack opt xform cnvrt sm(sm/black-point)
gbd(sm) alpha gmt(desaturate/gamut/tac/Ktone) context err profile io.

REMAINING: cmscgats.c (3339 lines, 39 exports — IT8 file format, the
biggest unported piece), cmsps2.c (1635 lines, 3 exports — PostScript
CSA/CRD/ColorResource), cmserr residual plugin-register functions,
cmsplugin.c, then: parity expansion (formats x intents x profiles,
LUT-heavy real profiles) + perf parity vs C.

## State @ 0dd397d (ps2 scaffold)

ps2.rs: PsWriter + mechanical emitters (header, PS escaping, Lab2XYZ
decode dict, gamma emitters, intent/range/white-black). The heavy
emitters (EmitCIEBased*/WriteInputLUT/OutputLUT/NamedColorCSA+CRD/
EmitPQRStage/EmitXYZ2Lab/GenerateCSA+CRD/3 exports) are next.

REMAINING MODULES:
- cmscgats.c (3339 ln, 39 exports — IT8 file format; biggest piece)
- cmsps2.c rest (~1100 ln of emitters + GenerateCSA/GenerateCRD)
- cmserr/cmsplugin residuals (plugin-register fn table)
- Parity: expand xform gate beyond sRGB->sRGB (LUT profiles, all
  intents, all formats), perf parity vs C oracle

## State @ 85b83a2 (ps2 emitters — in progress)

ps2.rs done so far: PsWriter, Word2Byte/WriteByte/RemoveCR/
EmitPSEscaped/EmitHeader/EmitWhiteBlackD50/EmitRangeCheck/EmitIntent/
EmitLab2XYZ + real Emit1Gamma/EmitNGamma (parametric shortcut +
tabulated-table fn; TODO C's %g vs Rust {}).

cmsps2.c emitters remaining (line refs): OutputValueSampler:589,
WriteCLUT:668, EmitCIEBasedA:714, EmitCIEBasedABC:742,
EmitCIEBasedDEF:780, ExtractGray2Y:841, WriteInputLUT:871,
GetPtrToMatrix:953, WriteInputMatrixShaper:963,
WriteNamedColorCSA:1010, GenerateCSA:1063, EmitPQRStage:1193,
EmitXYZ2Lab:1266, WriteOutputLUT:1295, BuildColorantList:1418,
WriteNamedColorCRD:1443, GenerateCRD:1510, 3 exports:1557-1635.

## State @ f2f738c (cmsps2 complete — ps2.rs done)

All PostScript emitters ported: EmitPQRStage, EmitXYZ2Lab,
WriteInputLUT/OutputLUT, WriteInputMatrixShaper, WriteNamedColorCSA/
CRD, BuildColorantList, EmitCIEBasedA/ABC/DEF, ExtractGray2Y,
GenerateCSA (named/matrix-shaper/LUT dispatch), GenerateCRD, plus
the 3 exports (get_post_script_color_resource/crd/csa). Named-color
transform eval is a stub pending TYPE_NAMED_COLOR_INDEX formatter.

ONLY UNPORTED MODULE LEFT: cmscgats.c (3339 lines, 39 exports —
the IT8/CGATS file format: reader, writer, sampler, patch/label
machinery). cmserr/cmsplugin residuals remain small.

After cmscgats: 100% lcms2 C coverage. Then: parity expansion
(LUT profiles, all intents, all pixel formats, edge values),
perf parity vs C oracle.

## cmscgats.c resume note (the last module — 3339 lines, 39 exports)

Structure: chunk allocator (AllocChunk/AllocBigBlock/AllocString),
string type (StringAlloc/Clear/Append/Cat/Ptr), lexer (NextCh/
InSymbol/InStringSymbol/BinSrchKey/ReadReal/ParseFloatNumber),
property lists (IsAvailableOnList/AddToList/AddAvailableProperty/
AddAvailableSampleID/KEYVALUE), TABLE array (AllocTable/
cmsIT8SetTable/GetTable), the file parser (ParseIT8Header/
ParseDataFormat/ParseData/ParseFile → ReadReal/ReadChar/ReadField),
the writer (WriteHeader/WriteDataFormat/WriteData/SaveFile), and
the 39 exports: IT8Alloc/Free/SetTable/GetSheetType/SetSheetType/
SetComment/SetPropertyStr/Dbl/Hex/Uncooked/SetPropertyMulti/
GetPropertyStr/Dbl/Multi/GetPatchID/GetPatchName/FindDataFormat/
SetDataFormat/GetDataDbl/SetDataDbl/GetDataDblIndex/SetDataDblIndex/
GetDataSetID/SetDataSetID/GetDataFormat/GetDataFormatStr/
GetDataSetStr/Dbl/SetDataSetStr/Dbl/GetDataSetID/SetDataSetID/
GetDataSetId/SetDataSetId/GetDataSetID/SetDataSetID/LCL2PCS/
PCS2LCL/SetTableIndex/GetTableIndex/GetTableData/SaveToFile/
SaveToMem/SetSheetType/SetComment.

## State @ ad0484d — 100% lcms2 C module coverage

Every lcms2 .c file now has a bitexact Rust port:
cmsio0 io io1 lut gamma pcs wtpnt intrp md5 cam02 half named
tagtypes virt pack opt xform cnvrt sm(sm) gbd(sm) alpha gmt ps2
cgats context err profile fixed mtrx types signatures.
srgb_identity_transform is byte-exact end-to-end.

REMAINING toward "100% coverage + parity + perf":
- Parity expansion: transform gate across real LUT profiles (not
  just sRGB), all 4 intents, the pixel-format matrix, named-color,
  GBD CheckPoint/Compute oracle, cgats IT8/CUBE round-trip oracle
- Perf parity vs C oracle (formatter + CLUT eval hot paths)
- Named-color transform eval is a stub pending TYPE_NAMED_COLOR_INDEX
- %g PostScript formatting vs C's %g
- cmserr/cmsplugin residuals (plugin-register fn table)

## State @ ff3a5dd — 100% coverage + expanding parity

ALL lcms2 .c modules ported. Parity gates green:
xform (sRGB identity), xform_matrix (intents×4, RGB16/8, BGR8,
Lab4→sRGB, sRGB→XYZ, Lab2→sRGB), cgats (IT8 parse+data cells),
gbd (GBD alloc/add/check/compute containment), cam02, pack, +
~80 unit/parity tests. Full workspace green.

Real bugs caught by gates this session:
- compute_conversion propagated detect_black_point None; C ignores
  the return (point stays {0,0,0}) — broke Lab4(abstract)→sRGB
- cgats: is_separator had \n; in_string didn't consume close quote;
  get_val double-advanced; check_eoln didn't consume EOLNs;
  load_from_mem never primed ch; nosheet inverted

REMAINING: cmserr/cmsplugin residuals (plugin-register fn table),
named-color transform eval stub, %g PostScript formatting,
perf parity (2.4-3x on dense eval — structural: per-pixel indirect
calls + bounds checks vs C raw pointers; needs scanline-batch eval
or unsafe to close), more parity expansion.

## State @ ae17878 — 100% coverage + all gates green

Every lcms2 .c module has a bitexact Rust port. Parity gates:
xform (sRGB identity byte-exact), xform_matrix (6: intents×4,
RGB8/16, BGR8, Lab4→sRGB, sRGB→XYZ, Lab2→sRGB), cgats (IT8 parse +
data cells), gbd (GBD alloc/add/check/compute), cam02, pack.
Full workspace green (~92 tests).

REAL bugs caught by gates:
- compute_conversion propagated detect_black_point None; C ignores
  the return (point stays {0,0,0}) — broke Lab4(abstract)→sRGB
- cgats lexer: is_separator had \n; in_string didn't consume close
  quote; get_val double-advanced; check_eoln didn't consume EOLNs;
  load_from_mem never primed ch; nosheet inverted
- cat() cloned stage tails (duplicate chains)
- pipeline_stage_match inverted trailing check
- text_description_write family-accept; placeholder defaults;
  Mat3 row-major; gray broadcast; T_COLORSPACE mask; FROM_16_TO_8
  formula; is_ink_space

REMAINING:
- cmserr/cmsplugin residuals (done: Plugin/PluginType/create/dup_ctx)
- named-color transform eval stub (TYPE_NAMED_COLOR_INDEX)
- %g PostScript formatting vs C's %g
- perf parity (2.4-3x on dense eval — structural: per-pixel indirect
  calls + bounds checks vs C raw pointers; needs scanline-batch eval
  or unsafe to close)
- more parity expansion (CMYK profiles, gamut-check xform, .cube)

## State @ 14388bd — 100% coverage, real-profile parity started

ALL modules ported. Gates green: sRGB identity, intents×4, RGB8/16,
BGR8, Lab4→sRGB, sRGB→XYZ, Lab2→sRGB, FLAGS_FORCE_CLUT (resampling),
test1.icc CMYK-prtr→sRGB (557KB real LUT-heavy profile — byte-exact),
IT8, GBD, cam02, cube devicelink. ~93 tests green.

OPEN THREAD — ibm-t61.icc divergence: v3.4 RGB matrix-shaper monitor
→ sRGB gives C=[0,0,8] vs Rust=[2,4,12]. Likely a TRC tag-type or
colorant-matrix path that differs for v3.x profiles (test1.icc v2.1
works). Bisect read_input_lut's matrix-shaper builder on ibm-t61 —
compare the TRC eval + colorant matrix Rust-vs-C on that profile.

REMAINING: ibm-t61 root cause, named-color transform eval stub,
%g PostScript formatting, perf parity (2.4-3x structural gap),
more real-profile coverage (test2-5, crayons, v4 profiles).

## ibm-t61 divergence bisect (for next context)

ibm-t61.icc = v3.4 RGB matrix-shaper monitor, XYZ PCS.
TRCs = `curv` type, 256-entry tables (same tag family as sRGB —
NOT a parametric/para divergence). Colorants present (rXYZ/gXYZ/bXYZ).

At input 0u8: C=[0,0,8], Rust=[2,4,12] — consistent small offset,
Rust brighter. Bisect in read_input_lut's matrix-shaper path:
1. eval rTRC[0] Rust-vs-C on the parsed profile (curv table interp
   at the low end — LinearInterp16 edge handling?)
2. compare the read colorant matrix Rust-vs-C
3. v3.4 XYZ-PCS path vs v4's (xyz2lab adjacency differs by PCS type)
Likely a low-end curv-table interpolation or a PCS-domain handling
difference, not a structural bug (test1.icc CMYK/Lab-prtr is exact).

All prior state committed/pushed; full workspace green except the
single c_parity_realprofile::monitor_rgb_to_srgb assert.

## ibm-t61 divergence — narrowed to MatShaper eval (root cause TBD)

VERIFIED CORRECT (Rust == C): rTRC eval, colorant matrix, input-LUT
XYZ at 0 ([0x002b,0x002b,0x0058] matches manual trc×colorant math),
output-LUT structure. Rust's [2,4,12] is the mathematically CORRECT
answer (numpy inverse check). C gives [0,0,8].

Optimizer: Rust fires MatShaper for ibm-t61→sRGB (correct — PreOptimize
collapses curv,mat,mat,mat,curv → curv,mat,curv). mat_shaper_eval16
structure identical to C (In&0xFF, mat·shaper1, >>14+0x2000, shaper2).

Divergence is in the TABLE contents: shaper1 (fixed-14 trc), mat
(fixed-14 RGB2XYZ_ibm × inv_sRGB), or shaper2. Either:
(a) C's double_to_1fixed14 rounds differently,
(b) the combined `res` matrix product differs (mat_from_stage /
    mat3_per precision), or
(c) C picks a different strategy (resampling CLUT) for this pair.
NEXT: dump mat[][]+shaper2[0..k] Rust-vs-C on the same linked pipeline
(add an oracle hook that returns the C MatShaper tables), or eval C's
MatShaperEval16 directly. Files: rust/crates/lcms/src/opt.rs:400-560.

## K-preserving intents — last functional gap, porting now

black_preserving_k_only_intents / k_plane_intents are stubs
(signal_error + None). Structure captured from cmscnvrt.c:

K-only: translate_non_icc_intents → trim trailing CMYK devicelinks →
if !cmyk-in/cmyk-or-output → default_icc_intents → Result=Pipeline(4,4)
→ cmyk2cmyk = DefaultICCintents(preservationCount) → KTone =
_cmsBuildKToneCurve(4096) → CLUT=AllocCLut16(grid,4,4) sampled by
BlackPreservingGrayOnlySampler(cmyk2cmyk, KTone) → append trailing
devicelinks. KTone curve needs _cmsBuildKToneCurve (per-channel K→K
through the chain — port in sm.rs or gamma.rs).

K-plane: PreserveKPlaneParams needs hProofOutput (CMYK→Lab on last
profile), cmyk2Lab (input chain), LabK2cmyk (output profile), hRoundTrip,
cmsDetectTAC, eval_reverse_float. Much bigger — needs reverse-eval +
roundtrip transforms.

Helpers needed: _cmsBuildKToneCurve (NEW — builds K→K curve by
evaluating the chain on pure-K inputs), stage_sample_clut16bit (exists,
Sampler16 = FnMut(&[u16],&mut[u16])->i32), eval_reverse_float (exists),
detect_tac (exists in sm.rs).

## K-plane intent — in progress (detect_tac roundtrip produces 0)

K-only intent WORKS (byte-exact, cmyk_to_cmyk_konly passes).
K-plane intent compiles but transform returns None — bisected to
detect_tac returning 0. slice_space16 runs (returns true) but max_tac
stays 0 → the roundtrip Lab16→f32-CMYK do_transform produces no CMYK
output into outb. Likely the input format (type_lab_16, u16) unroll or
the f32 output pack path produces zeros — check do_transform's Fmt
dispatch for mixed u16-in/f32-out (Fmt::F16 vs Fmt::Flt mismatch).
Files: cnvrt.rs detect_tac (~line 782), make_last_proof, run_lab_transform*.

## State @ 0e04819 — 100% coverage, ALL intents + real profiles green

Every lcms2 .c module ported. ALL parity gates green:
- xform: sRGB identity, intents×4, RGB8/16, BGR8, Lab4→sRGB,
  sRGB→XYZ, Lab2→sRGB, FLAGS_FORCE_CLUT
- real-profile (6 gates): test1/2 CMYK-prtr↔sRGB, ibm-t61 v3.4 mntr,
  test3/4/5/crayons RGB profiles → sRGB, sRGB→test1 CMYK output,
  test1→test2 K-only + K-plane (both black-preserving intents)
- cgats IT8, GBD, cam02, pack, cube devicelink
~97 tests green.

REAL bugs caught by oracle gates:
- compute_conversion propagated detect_black_point None; C ignores
  the return (point stays {0,0,0}) — broke Lab4(abstract)→sRGB
- cgats lexer: is_separator had \n; in_string didn't consume close
  quote; get_val double-advanced; check_eoln didn't consume EOLNs;
  load_from_mem never primed ch; nosheet inverted
- cat() cloned stage tails (duplicate chains)
- pipeline_stage_match inverted trailing check
- _MultiplyMatrix fused offset-carrying matrices (ibm-t61 BPC drop)
- detect_tac: 'move' on Copy captured copies of accumulators → 0
- placeholder defaults, Mat3 row-major, gray broadcast, T_COLORSPACE
  mask, FROM_16_TO_8, is_ink_space

REMAINING:
- named-color transform eval stub (TYPE_NAMED_COLOR_INDEX)
- %g PostScript formatting vs C's %g (textual parity)
- perf parity: 2.4-3x on dense eval — structural (per-pixel indirect
  calls + bounds checks vs C raw pointers); needs scanline-batch or
  unsafe to close
- cmserr/cmsplugin residuals (Plugin/PluginType/create/dup_ctx done)
- more parity expansion (gamut-check xform, named-color, ps2 text)

## State @ f23e1b5 — 100% coverage, all intents + real profiles green

Every lcms2 .c module ported and parity-verified. ALL gates green:
xform (sRGB identity, intents×4, RGB8/16, BGR8, Lab4→sRGB, sRGB→XYZ,
Lab2→sRGB, FLAGS_FORCE_CLUT), real-profile×6 (CMYK-prtr↔sRGB, ibm-t61,
test3/4/5/crayons→sRGB, sRGB→CMYK, K-only + K-plane CMYK→CMYK), cgats,
GBD, cam02, pack, cube. ~97 tests.

REMAINING (last stub): named-color transform eval — Transform needs
InputColorant/OutputColorant (cmsDupNamedColorList of the input
profile's SigColorantTableTag + output's TableOut/Table tag), the
TYPE_NAMED_COLOR_INDEX input formatter (index → colorant Lab via the
list), and cmsGetTransformInput/OutputColorants accessors. C reads the
colorant lists in AllocEmptyTransform (cmsxform.c:1241-1264). The
NamedColorList type + alloc/dup/append already exist in named.rs.

REMAINING (other): %g PostScript formatting, perf parity (2.4-3x
structural). Named-color transform DONE @9498e9ab: clrt/ncl2 colorant
lists + TYPE_NAMED_COLOR_INDEX + get_xform_color_spaces fix
(*Input = profile[0] computed ColorSpaceIn).

## State — cmsps2.c complete, named-CRD upstream quirk found

ps2.rs now has the full module: ExtractGray2Y, WriteInputLUT,
WriteInputMatrixShaper, WriteNamedColorCSA, GenerateCSA, EmitPQRStage,
EmitXYZ2Lab, WriteOutputLUT, BuildColorantList, WriteNamedColorCRD,
GenerateCRD, get_post_script_color_resource/CRD/CSA + format_g (C %g)
+ ctime_now. Gates: srgb_csa, cmyk_csa_crd (CSA+CRD on test1.icc),
named_csa_crd — all byte-exact (Created line masked, ctime-dependent).

Real bugs fixed this session:
- sm::detect_black_point unconditionally used perceptual-black for the
  fall-through; C only does for Intent==REL && class==Output &&
  isInkColorspace — else BlackPointAsDarkerColorant(Intent). sRGB
  relative gave -1.27e-16 vs C's exact 0.0.
- sm::detect_destination_black_point fed a color-space SIGNATURE to
  pack::is_ink_space (a pixel-format helper) — new sm::is_ink_colorspace.
- cnvrt::link_profiles was missing C's final check
  channels_of(ColorSpaceOut) != Result.output_channels → Rust's n=1
  named→CMYK transform succeeded where C fails "Couldn't link".
  Upstream quirk: WriteNamedColorCRD unreachable for non-Lab named
  profiles (recorded in private ledger lcms-C-BUGS.md).
- ps2 WriteCLUT iterated tab16 directly w/o In[] grouping and would
  panic on inp[0]; now uses stage_sample_clut16bit like C. FixWhite
  assigned nothing to out + skipped row-grouping.
- _cmsIOPrintf ','→'.' replacement replicated in PsWriter::out.
- emit_range_check text was wrong ({dup 0 lt...} bind vs C's
  'dup 0.0 lt { pop 0.0 } if dup 1.0 gt { pop 1.0 } if ').

CLOSED: srgb_crd ±1-LSB — root cause was ps2::dup_profile serializing
+reopening the profile (float tags quantized to s15Fixed16). C borrows
the original handle. Fix: IccProfile/IoHandler/IoStream/FileMem now
Clone; ps2::dup_profile and sm::dup_profile do a field-exact clone.
srgb_crd byte-exact; ignore removed. Ledger quirk recorded in
rust/docs/C-BUGS.md.
REMAINING: perf parity (~2.4-3x structural).

## srgb_crd ±1-LSB bisect — root cause FOUND: dup_profile precision loss

PROOF CHAIN (all verified):
- C resampled tab16 (/tmp/c_resample2.c, _cmsOptimizePipeline FORCE_CLUT)
  vs Rust replication (t.lut.clone + optimize_pipeline) — 0 diffs.
- Emitted CRD stream: 175 byte diffs, each w2b(tab)±1 — C emits
  w2b(tab), Rust emits different.
- PS2DBG probe of output_value_sampler: the REAL write_output_lut
  path's CLUT has e.g. out[1]=0x29e2 at node (0,5,10) where C and my
  replication both have 0x29e1. So the real path's resampled TABLE
  differs ±1 — not the emission.
- ROOT CAUSE: ps2::dup_profile() = save_profile_to_mem +
  open_profile_from_mem. Round-tripping through ICC serialization
  quantizes float tags (XYZ colorants→s15Fixed16, etc). The transform
  then evals a slightly different pipeline → ±1 CLUT entries.
- C passes the SAME cmsHPROFILE (borrowed, full precision).
- FIX NEEDED: create_extended_transform/link_profiles must accept
  profile REFERENCES, not owned boxes. Plan: make
  xform::create_extended_transform generic over
  P: DerefMut<Target=IccProfile> (callers pass &mut [Box<IccProfile>]
  or &mut [&mut IccProfile]), cascade to cnvrt::link_profiles +
  get_xform_color_spaces + io1 readers. Then ps2 dup sites pass
  &mut icc directly — no serialization.
- Alternative (worse): IccProfile Clone impl — fails for lazily-loaded
  tags when io=None.
- Affected ps2 fns: write_input_lut, write_output_lut,
  write_named_color_csa, write_named_color_crd, extract_gray2y —
  ALL use dup_profile. Same pattern may exist in cnvrt K-intents +
  sm::detect_black_point roundtrips (which PASSED so far — check if
  they also need the reference fix for exactness).

DEBUG ARTIFACTS: /tmp/c_resample2.c (C side bisect program),
/tmp/psdiff (rust bisect), /tmp/c_tab16.bin /tmp/r_tab16.bin,
/tmp/c_crd.txt /tmp/r_crd.txt, /tmp/c_named*.c.
Also: ps2.rs has a TEMPORARY eprintln in output_value_sampler gated by
env PS2DBG — remove before landing (or keep? it's harmless).
Also: opt.rs got 4 f32-div fixes (input[i]/65535 now double-then-cast,
matching C's XFormSampler16 + prelin ramp) — correct but didn't fix
this diff.

PRIVATE LEDGER: ~/work/zen/lcms-C-BUGS.md (untracked).

## State — perf: fused fast paths + formatter bounds-check removal

- pack.rs: all Unpack16/Pack16/UnpackF32/PackF32 fn-pointer signatures
  now take `w: &[T; MAX_CHANNELS]`/`&mut [T; MAX_CHANNELS]` (callers
  always pass fixed arrays) — every w[i] bounds check eliminated.
  a/out sub-slice prefixes (let a = &a[..N]; let out = &mut out[..N])
  added to hot formatters + unroll macros (const-N computed from the
  $idx list) — lets LLVM elide the per-element checks.
- xform16: fused fast paths BEFORE the generic loop —
  (a) OptData::Identity + identical non-planar input/output formats
      → per-line copy_from_slice (byte-exact: unroll→copy→pack is a
      provable round trip). sRGB→sRGB 8-bit now ~0.10x of C, 16-bit
      ~0.17x (was 2.4-2.7x). Bug fixed mid-way: pixel byte count is
      (t_channels + t_extra) * t_bytes, not pixel_size (bytes/channel).
  (b) OptData::MatShaper + RGB8→RGB8 → inlined shaper1→mat→shaper2
      eval (same tables/ops, no fn-ptr dispatch).
- C reaches the same FastIdentity16 for sRGB→sRGB (identity mat removed
  by PreOptimize → [curv,curv] → join_curves → all-linear → idn stage +
  FLAGS_NOCACHE). Rust did the same already (OptData::Identity +
  fast_identity16) — the gap was purely per-pixel plumbing.
- perf_xform (1M px sRGB→sRGB): 8-bit 0.10x, 16-bit 0.17x of C.
  Generic non-identity paths still ~2x (fn-ptr chain + slice plumbing)
  — further fused paths or batching are future work.

REMAINING: generic-path perf (~2x), residual coverage edges.

## State — CMYK/CLUT generic-path perf + perf_xform CMYK bench

- perf_xform now benches test1.icc CMYK→sRGB (CLUT-heavy generic
  path, Prelin/CLUT eval + 4ch unroll/pack). Measured: 8-bit ~1.3x,
  16-bit ~1.8x of C (after formatter bounds-check removal). The fixed
  [u16;MAX_CHANNELS] w-param change paid off on the dense paths too.
- sRGB→sRGB identity is now 0.10x/0.17x of C via the fused line-copy.
- Real-profile gate still byte-exact incl. monitor_rgb_to_srgb
  (MatShaper fused path) and both K-intents.
REMAINING: 16-bit CLUT eval ~1.8x (interp16 fixed-point loops —
  candidate for per-node fused kernels), generic-path edge cases.

## NEXT PERF TARGET — interp16 table-read bounds checks (~1.8x on 16-bit CLUT)

16-bit CMYK→sRGB (CLUT path) sits at ~1.8x of C: `eval4_inputs` /
`tetrahedral_interp16` / `trilinear_interp16` each do ~4 `tbl_get16`
bounds-checked reads per output channel (C uses raw pointer arithmetic).

Fix recipe (SAFE, no unchecked): per pixel, after ordering the offsets
(cumulative: x1 ≤ y1 ≤ z1 in tetra16; eval4_inputs similar), slice once
`let cell = &lut[base .. base + n_out + z_max]` — then reads
`cell[ch + off]` for ch in 0..n_out, off ∈ {0,x1,y1,z1} are all
provably ≤ n_out-1+z1 < cell.len() → LLVM elides the ~16 checks/px.
Same shape for trilinear (2^3 nodes, offsets opta products) and
eval4_inputs (2^4). Verify byte-exact via c_parity_realprofile +
c_parity_intrp gates. Est. CMYK 16-bit ~1.8x → ~1.1-1.3x.

Also open: f32-path fused fast paths (xform_f32 equivalents) if a real
workload needs them; residual coverage edges per the C-file checklist.

## PERF STATE + NEXT — baked InterpParams for ClutOnly (memmove hotspot)

Host is lilith-wsl (Zen4 7950X, WSL2 — AVX-512 double-pumps; "not
measured on Zen 5"). perf counters OK (paranoid=-1). archmage latest =
0.9.31-beta.2; 0.9.31-beta.1 adds `attune`/`attuned!`/`reattune!`
(next-gen feature-body/dispatch macros — prefer over arcane/rite/incant
for new SIMD work). coz-rs 0.1.3 on crates.io for causal profiling.

Profile: `__memmove_avx512` ~11.5% — `clut_eval16` (opt.rs:743) rebuilds
a ~120B InterpParams on the stack EVERY pixel (C bakes Data->Params at
CLutData creation). `#![forbid(unsafe_code)]` — no raw-ptr params.

Fix plan (C-faithful, safe):
1. InterpTable += `U16Rc(Rc<[u16]>)`/`F32Rc(Rc<[f32]>)` variants;
   table_u16/f32 return `&[u16]`/`&[f32]` tied to &self.
2. CLutData.tab16: Option<Vec<u16>> -> Option<Rc<[u16]>> (Rc::from(Vec)
   reuses alloc — no copy); same for tab_float.
3. OptData::ClutOnly gains a baked `InterpParams<'static>` (Rc table ->
   'static lifetime) built once at opt.rs:1461 construction.
4. clut_eval16 -> `c.params.eval16(input, output)` — zero rebuild.
tab16 read sites to update: lut.rs:332,395,566,577; opt.rs:726,754,
805,1044. Est. -1-3ns/px on all CLUT paths. Gates: c_parity_realprofile,
c_parity_intrp, c_parity_xform + full workspace.

## PERF NEXT — monomorphized xform16 inner loop (kills the ~1.6x dispatch)

CMYK16→sRGB now ~1.5-1.9x. Profile shows the per-pixel dispatch chain:
unroll fnptr → l.eval16 → eval16_fn indirect → opt_data match →
params.eval16 → interpolation match → interp fnptr → tetra. ~5 indirect
+ 2-3 matches/px vs C's 1 indirect + params deref.

IMPLEMENT: extract the generic pixel loop (xform.rs ~445-485, the
for-lines/for-ppl body after the fused paths) into
`xform16_eval_loop<E: Fn(&[u16],&mut[u16])>(...)` parameterized on a
monomorphized eval closure. Borrow: can't take &mut self while
self.lut is borrowed → make it a free fn taking
(unroll, pack, info, gamut_check: &mut Option<Pipeline>,
alarm: &[u16;16], lut-independent fields) OR pass lut out separately.

Dispatch once in xform16 on self.lut.opt_data:
  ClutOnly(c)      → |i,o| c.params16.eval16(i,o)
  MatShaper(_)     → |i,o| { mat_shaper_eval16(i,o,od); }
  Curves16(_)      → |i,o| { fast_eval_curves16(i,o,od); }
  Prelin8/16,Identity similar; None → |i,o| l.eval16(i,o)
Each arm monomorphizes → eval inlined into the loop (mat/curves/identity
become one inlined block; clut_eval16 inlines params deref, the interp
fnptr remains).

FURTHER: Interpolation could store a kernel tag so the monomorphized
ClutOnly arm calls eval4_inputs/tetrahedral_interp16 DIRECTLY (concrete
fn → full inline → table/opta loads hoisted, all bounds elided). That
last step likely reaches ~1.0-1.1x. Est the tag: Kernel16 enum at
set_interpolation_routine.

REMAINING after that: 5-8ch interps, f32 fused paths, generic
stride/planar edges.

## PERF — monomorphized xform16 loop landed (dispatch removed)

xform16_eval_loop<E: Fn(&[u16],&mut[u16])> — generic !gamut pixel loop,
dispatched once on opt_data: ClutOnly → c.params16.eval16 inline; all
other opt variants → opt::eval_opt16 (match dispatcher added in opt.rs).
gamut/cached-gamut workers keep the generic loop. Parity green; CMYK16
~1.6x (unchanged — the tetra16 interp math dominates, not dispatch).
Remaining 16-bit CLUT gap is inside eval4_inputs/tetra16 itself (16
dependent node reads + lerp per channel — memory-latency bound like C).

## PERF — concrete-kernel dispatch landed (CMYK16 ~1.25-1.6x)

xform16 ClutOnly arm resolves params16.interpolation fn ptr once (==
against eval4_inputs/tetrahedral/trilinear/bilinear/lin_lerp_1d/
eval1_input — now pub(crate) + #[inline] in intrp.rs) → monomorphized
loop calls the concrete kernel. #[inline] isn't enough to fold the
~150-line kernels (14 calls remain in disasm); the residual gap is the
tetra math itself (16 dependent node reads + lerp per channel — same
latency-bound work as C, ~1.3-1.6x residual is codegen arithmetic not
dispatch). Options not pursued: inline(always), archmage attune batch
SIMD (interp is gather-bound, poor fit), Kernel16 tag.

## DIVERGENCE BISECT — float paths clip negatives (zenpixels handoff)

From ~/tmp/handoff/lcms-parity-20261105/brief.md — real parity divergences
on the FLOAT paths (C does not clip, port clips to 0.0):
- test1.icc CMYK_FLT->RGB_FLT intent0: 1024 diffs worst 208.95/255.
  Repro: input [85.88,0,14.5,29] -> port [0,.543,.647] C [-.819,.543,.647].
- test3.icc RGB spac->sRGB F32: 352 diffs worst 15.75; C R=-5.40 port 0.0.
- test1.icc U8 identical; test3.icc U8 differs 4960/12288 (eval-path bug
  too, not just float clip).

Clip hunt (in progress): unroll_fd_to_float divides ink by 100 correctly;
pack_fd_from_float esz=4/8 writes raw floats (no clamp); interp float
fclamps INPUT coords only (C same); ToneCurve::eval_float quick_saturates
negatives -> Table[0] — but C's cmsEvalToneCurveFloat clamps identically.
NEXT: dump the linked pipeline stage types for CMYK_FLT->RGB_FLT vs C's
cmsPipeline — the -0.819 must come from a non-clamping final stage
(CLUT direct output?) — if my port adds/evals a TRC stage that C doesn't
route through, or eval_clut_float clamps, that's the bug. Also U8 path
differs on test3.icc -> same stage-eval divergence, not just float clip.

Also pending from brief: Transform: Send (Rc->Arc, Box<dyn Any+Send+Sync>,
check perf), README status table (transforms ported).

## test3.icc U8-opt bisect UPDATE — eval_float feeds the resampler

FLAGS_NONEGATIVES (0x40->0x8000, dd8c01b) removed the clp stage and
cleared ALL F32 diffs to 0. U8-opt still 4960/12288, worst byte diff
254 (port 255 vs C 1). qs16/quick_saturate_word clamps correctly (no
wrap). port-noopt (lut_eval16, u16 path) is bit-exact vs C — so the
divergence is inside `eval_float` (the resampler eval's the linked
pipeline in float then saturates). At the bad node my float eval gives
~0.996 (->255) where C gives ~0.004 (->1) — a real eval_float error at
scattered nodes, not a clip. test3.icc = RGB spac, mft1, Lab PCS ->
Lab->XYZ->RGB matrix+curves chain. Check: eval4_inputs_float/
tetrahedral_interp_float for the mft1 CLUT, Lab2XYZ/XYZ2Lab float
stages, and the cvst curve eval (eval16->float conversion for table
curves).

## test3.icc U8-opt bisect — Prelin8, structure verified

Fired optimizer is `optimize_by_computing_linearization` (Prelin8), NOT
resampling. Rust structure matches C's `OptimizeByComputingLinearization`
exactly: Trans[i] gray-ramp eval_float -> slope_limiting -> monotonic/
degenerate gate -> TransReverse[i] = reverse_tone_curve_ex(Trans[i]) ->
LutPlusCurves = dup(Original) + TransReverse prepended -> OptimizedLUT =
Trans (prelin) + CLUT sampled over LutPlusCurves.

Bad node (32,31,28): opt R=0 vs true R~1.0. Structure identical, so the
divergence is inside `reverse_tone_curve_ex` or `build_tabulated_tone_curve16`:
test3.icc's gray-ramp Trans has flat/saturated regions (non-strictly-
monotonic) — C's cmsReverseToneCurveEx picks a specific inverse; if my
port's tie-breaking/interpolation differs the sampled CLUT inputs differ
-> wrong table entries at scattered nodes -> the 255-vs-1 wraparound.
NEXT: compare reverse_tone_curve_ex output vs C's cmsReverseToneCurveEx
on the actual Trans curves (non-monotonic sections), esp. endpoints and
flat regions. Also verify build_tabulated_tone_curve16 + eval_float of
the gray ramp match C's Trans (the eval_float divergence could ALSO feed
Trans wrong -> different reverse).

## test3.icc U8-opt — FULL bisect, residual localized to LutPlusCurves eval

FORCE_CLUT (optimize_by_resampling) = 0 diffs -> eval_float on the
ORIGINAL pipeline is bit-exact, and the sampler/indexing is correct.
The bug is strictly inside optimize_by_computing_linearization's
LutPlusCurves path. Verified bit-exact vs C:
- Trans[i] gray-ramp eval_float -> qs16 (0 diffs, F32 transform too)
- slope_limiting == C's SlopeLimiting
- reverse_tone_curve_ex bit-exact on the actual Trans (0/4096 diffs)
- get_interval == C's GetInterval
- is_tone_curve_monotonic == C's (all true -> both use Prelin8)
- is_degenerated == C's IsDegenerated
- prelin_opt8alloc / prelin_eval8 structure == C's
- Pipeline::dup == cmsPipelineDup (clones stages, order preserved)

Data: node (32,31,28) — my table [65535,63281,57337] stores my
LutPlusCurves.eval_float([1.0,.969,.875])=[1.0017,.9656,.8749]->qs16.
C's table there ~[256,...] -> C's LutPlusCurves eval ~0.004 -> C's
TransReverse(1.0) maps 1.0 to a different input than mine. So
TransReverse IS built from the same Trans but the sampled composite
eval differs -> residual inside the TransReverse TONE-CURVE STAGE
eval_float (eval_curve_set on the prepended reversed table) or a
boundary case in ToneCurve::eval_float at the 4095 table end. Check:
eval_curve_set/stage_alloc_tone_curves on the prepended TransReverse,
and whether eval16 of a 4096-entry table curve at index ~4095 matches
cmsEvalToneCurve16 (the X0[255] path gives opta*32 correctly, but the
float-path eval_float of the same curve may diverge at the top).

## test3.icc U8-opt — residual: TransReverse stage eval at table top

Narrowed further: my resampled table at (32,31,28)=[65535,63281,57337]
matches MY LutPlusCurves.eval_float exactly. C's table ~256 means C's
TransReverse(1.0) maps 1.0 to a different input. The TransReverse is a
4096-entry TABLE curve; TransReverse.eval_float(1.0) =
eval16(qs16(65535)) = eval16(65535) = table lookup at the LAST entry —
the boundary case. If my eval16(65535)/eval1_input at the top
(n_entries-1 index) or C's cmsEvalToneCurve16(65535) diverge by even 1
ulp, the inverse at the top shifts a whole CLUT edge row -> the 255-vs-1
wraparound. ALSO: Trans[4095] depends on eval_float(gray_ramp i=4095,
v=1.0) — if my eval_float gives exactly 1.0 (qs16->65535) but C gives
0.9998 (qs16->65422), the top interval of GetInterval differs -> whole
edge of the reverse shifts. Check eval1_input/Eval1Input top-of-table
handling and eval_float at v>=1.0 for the spac pipeline.

## NEXT-PHASE WORK PLAN (user, post-corpus)

Ordered asks: broaden coverage → drill down → perf fix → parallel API →
moxcms-competitive tetra → C-FFI dropin crate (threading) → pass C test suite.

### Real bugs to fix (correctness, blocking)
- scRGB-v2 u16 1-ulp: systematic ~1-ulp float diff in `eval_segmented_fn`
  (52867/65536 eval_float ulp diffs vs cmsPipelineEvalFloat). One boundary
  crosses qs16 -> 1-off output. Isolate vs C EvalSegmentedFn on the curve params.
- Slow link (BIGGEST real perf bug): `create_extended_transform` takes
  1-14s on CLUT profiles (Calibrated_A2B_XYZ_Mismatch 14s, Canon EOS 13s).
  C takes <100ms. Suspect O(n^2) or per-node heap alloc in
  `optimize_by_resampling`/`stage_sample_clut16bit`/`reverse_tone_curve_ex`
  or `OptimizeByComputingLinearization`. Profile a slow link first.
- Corpus test (`realworld_profile_corpus` in c_parity_virt) is u8-only for
  speed; u16 links are ~10s on CLUT profiles — needs the link-perf fix.

### Features (moxcms parity)
- Parallel transform API: Transform is Send — add a row-parallel
  `do_transform`/`xform` (rayon feature or std::thread row split) that
  keeps byte-exact output. moxcms is 2-3x faster via better tetra codegen
  + row parallelism + special-cased matrix paths.
- Tetra residual ~0.79-0.8x: tetra16 dependent table reads, same as C —
  the gap is LLVM-vs-GCC codegen, hard to close without unsafe/SIMD.
- C FFI dropin crate (`lcms2-ffi` or similar): separate crate exposing the
  cms* C API surface backed by the Rust impl; must check Send/threading of
  the FFI boundary (Transform is Send; C callers may share across threads).
- C test suite (testcms2.c in reference/lcms2/testbed): port or bind the
  oracle test suite against the Rust impl as the broadest parity gate.

## eval_float ~4e-6 divergence — NARROWED (calibrated_nonzero_black, u8)

Pipeline [cvst, matf, matf, cvst], intent0, flags=NOOPTIMIZE. Verified:
- rTRC/gTRC/bTRC all bit-exact vs cmsEvalToneCurveFloat (0/65536)
- evaluate_matrix == C EvaluateMatrix (same f64 accumulator)
- BUT pipeline eval_float: 179193 diffs, worst ~4.3e-6, producing NEGATIVE
  values (port -4.6e-6 vs C -2.9e-7). "nonzero black" profile — near-zero
  input -> negative matrix output -> 1-off u16/u8.
=> curves + matrix eval are exact; the divergence is the matf stage's
   `double`/`offset` COEFFICIENTS (from the A2B/B2A matrix tag read or the
   chromatic-adaptation composition) or an extra/missing conversion stage.
NEXT: dump my matf stage `double`/`offset` vs C's (xform->Lut stage Data
via cref shim) on calibrated_nonzero_black; compare mat3/adaptation
composition (compute_conversion off/MAX_ENC_XYZ scaling, mat3_per order).
Likely a matrix-tag-read rounding (s15Fixed16 -> f64) or a missing
chromatic-adaptation stage ordering difference.

## st3 matf+offset is the suspect (mAB 'M' element)
Pipeline [cvst, MAT(no off), MAT(+off -6.3e-4), MAT, cvst]. The offset is
the profile's mAB 'M' matrix-offset (real ICC element, bpc=false so not
from compute_conversion). ~4e-6 output divergence = the offset VALUES
differ from C -> the mAB 'M' offset read/scaling. C's stage is
_cmsStageMatrixData{Double[9], Offset[3]}; dump C's m+off via a new shim
and compare to the double/offset in st3.

## eval_float ~4e-6 divergence — ROOT CAUSE localized to mAB 'M' element read
C st3: MAT 1.0012986815038307 off [-0.0006261039,-0.0006493507,-0.0005356494]
R st3: MAT 1.001299017045028  off [-0.0006262657,-0.0006495184,-0.0005357878]
~3e-7 scale + ~2e-7 offset diff. Values are Float32 (not s15Fixed16 — the
×65536 products aren't integer). Suspect type_mab_read/elem_matrix reads
the 'M' element's matrix+offset at a slightly different byte position or
with different Float32->f64 conversion than C's _cmsReadFloat32Number.
NEXT: compare type_mab_read's element-offset/size handling + the 'M'
elem matrix/offset read loop vs C's Type_mAB_Read + ReadMatrix.

## eval_float divergence — ROOT: black-point detection values
compute_black_point_compensation formula == C exactly. st3 (matf+off,
scale+offset) is the BPC matrix. Offdiag ~3e-7 diff in scale, ~2e-7 in
offset => detect_black_point/detect_destination_black_point (sm.rs,
cmsDetectBlackPoint) returns ~3e-7 different CieXyz than C's
cmsDetectBlackPoint — a grid-search eval divergence (nearest-black LAB
point). NOTE: st3 exists even though bpc=false in the test -> check WHY
BPC path ran (is compute_conversion hit with bpc true, or is this the
'm'/absolute path?). Then diff sm::detect_black_point vs C
cmsDetectBlackPoint on the profile — the LAB-grid eval is the suspect.

## eval_float divergence — RESOLVED (3 real bugs, all landed)

1. **`optimize_pipeline` gated on `!NOOPTIMIZE`** (xform.rs) — C's
   `AllocEmptyTransform` calls `_cmsOptimizePipeline` UNCONDITIONALLY;
   `PreOptimize` strips identity/inverse-pair stages BEFORE the internal
   NOOPTIMIZE check. The Lab2 abstract profile's devicelink LUT is
   `[v4v2, identity-clut, v2v4]` — C collapses it to nothing (3-stage link);
   the port kept it, and `evaluate_clut_float_in16` u16-quantized the Lab
   output -> ~3e-4 Lab error -> wrong black points -> ~4e-6 BPC matrix error.
   FIX: call optimize_pipeline unconditionally (it handles NOOPTIMIZE
   internally, matching C's `_cmsOptimizePipeline`).

2. **`detect_destination_black_point` missing `cmsIsCLUT(OUTPUT)` check**
   (sm.rs) — C's condition `!cmsIsCLUT(OUTPUT) || (space != Gray/RGB/ink)`
   falls through to `cmsDetectBlackPoint` for non-CLUT-output profiles;
   the port only tested colorspace (`!ink && !Gray && !RGB`, wrong grouping
   and wrong operands) and always took the round-trip ramp for RGB/Gray/ink.
   FIX: added `io1::is_clut`/`is_intent_supported`/`LCMS_USED_AS_*` and the
   exact C condition. bp == dbp now (bit-exact, 0.0 diff on all failing
   profiles).

3. **Intent tag tables index-3 wrong** (io1.rs) — `DEVICE2PCS16[3]=AToB0`,
   `DEVICE2PCSFLOAT[3]=DToB0`, `PCS2DEVICE16[3]=BToA0`, `PCS2DEVICEFLOAT[3]=BToD0`;
   C uses `[3]=AToB1/DToB3/BToA1/BToD3` (absolute intent reads). Fixed all four.

VERIFY: detect_black_point + detect_destination_black_point bit-exact vs
cmsDetectBlackPoint/cmsDetectDestinationBlackPoint on
calibrated_nonzero_black, ibm-t61, new_c27b55f8 (0.0 diffs).
Corpus test (`realworld_profile_corpus`, 176 profiles) green — the u8
divergences are gone.
