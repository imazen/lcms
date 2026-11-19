# lcms Rust Port Rules

## START HERE

New to this port, or resuming after a gap? Read
**`docs/WORKING-ON-THIS.md`** before your first change — the port loop,
discipline, and numeric traps. `PORTING.md` (repo root) maps C files to Rust
modules and the gate for each.

## Conformance mandate

Lcms2Exact is the compatibility default. Keep each differential oracle
gate bit-exact across its declared sweep. Investigate divergences at the
first failing input and preserve established expectations.

SpecExact is the opt-in ICC.1 behavior. Test it against the normative
clauses and the independent checker in `tests/support/icc_spec.rs`. Record
writable cases, typed refusals and unimplemented semantics in
[CONFORMANCE.md](docs/CONFORMANCE.md). Keep work bounded to the requested gate.

## Envelope guards (settled facts — don't re-derive)

1. **The oracle is lcms2.20 upstream, unmodified**, at `reference/lcms2`
   (submodule → mm2/Little-CMS, tag `lcms2.20`, commit `7717fce`). If it ever
   needs a patch, that moves to an imazen fork; say so loudly.
2. **The oracle build flags are the contract**: `cc -O2
   -fno-strict-aliasing`. Changing them changes what "bit-exact" means
   (`sincos` merging, `pow` lowering). Bump `STAMP`/comment loudly if flags
   ever change.
3. **bit-exact is libm-exact on this platform.** lcms2's transcendentals go
   through the system libm; Rust's f64 methods call the same libm on
   Linux/glibc. Same-box parity is the gate; cross-platform parity is a
   separate, unproven axis — don't claim it.
4. **`cmsContext` = `Option<&Context>`, `None` = NULL.** The global context
   exists once. Error signalling goes through `err::signal_error` with C's
   exact conditions.
5. **lcms2.20 has no `_cmsLstar2Y`/`_cmsY2Lstar`** — older versions did; the
   functions were removed. Check the header before declaring a symbol.

## Habits

- Report what you ran, not what you believe.
- A gate that never saw a divergence is worth less than one that went red once.
- When wrong, correct it in place — doc, comment, commit message.


## Oracle harness bugs fixed after the SpecExact audit

- `c_parity_io1e` passed a 12-byte ICC date record to a C API that copies a
  native `struct tm`. The test shim now keeps native storage on the C side.
- `c_parity_pack` passed an 8-byte buffer to a four-plane transform with
  two pixels and a four-byte plane stride. The fixture now allocates every
  plane and compares transformed bytes and padding.
