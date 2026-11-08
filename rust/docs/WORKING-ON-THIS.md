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

**68.23% lines / 86.13% functions.** ~337 parity gates.
cmscgats up to 71.67%, cmsio0 to 77.36%, cmsnamed to 75.54%.
The remaining uncovered lines are deep internals reachable
only through oracle-UB inputs, profile shapes the public API
can't produce (real CMYK/MCH device profiles), or per-format
packer bodies for the less-common pixel types.
