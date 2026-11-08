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

- [finding] EXTRA_SH(1) input transform eval diverges — C
  [23321,32940,21682] vs Rust [21116,23420,38229] on RGBA16
  input; neither channel order matches. Rust's extra-channel
  formatter maps the alpha into the transform differently.

- [finding] DOSWAP input format also diverges the same way as
  EXTRA_SH — the format-bit channel mapping in do_transform is
  incomplete; the u16 pack/unroll paths produce the same wrong
  output class regardless of the DOSWAP/EXTRA modifiers.

- [finding] u16 srgb→Lab4 transform eval diverges broadly — C
  [31826,31835,21909] vs Rust [21116,23420,38229] on
  RGB[0x1234,0x5678,0x9abc]; the same wrong output appears
  regardless of format modifier bits (extra/doswap/swapfirst/
  endian16/flavor/planar). The u8 formats work — the u16-format
  transform eval path is broken deeper than the channel mapping.
  Also found: rd16 in pack.rs panics out-of-bounds on premul
  formats with short inputs (bounds check missing).
