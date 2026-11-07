# Brief: meaningful coverage of the unexercised lcms2 C surface + trustworthy oracles

**Repo:** `/home/lilith/work/zen/lcms` — a pure-Rust, bit-exact port of Little CMS 2.20.
The C reference is the unmodified upstream tree at `reference/lcms2` (submodule, commit
`7717fce`). **Do not modify anything under `reference/lcms2`.**

## Oracle and gate (mechanical)

The oracle is the **C library itself** — the port's definition of correct is "matches the C
binary's output byte-for-byte." The C oracle is compiled by `lcms-cref` (`cc` crate,
`-O2 -fno-strict-aliasing`) into a static lib the differential tests link against. Your gate
for every claim: a `c_parity_*.rs` test (or a bench differential) that compares the Rust
result against the real C symbol (or a same-source shim for `static`/`cmsINLINE` code) over
a dense or edge-case sweep, and reports **0 diffs**.

The existing suites all pass today: `cargo test --workspace` green, upstream `testcms2`
144/0, bench differentials 0 diffs. Keep them green.

## The coverage gap

Two-sided coverage through `testcms2` (run `just coverage-testbed` from `rust/`, or
`bench/coverage.sh`):

- C oracle: ~74% lines / ~83% functions
- Rust port: ~67% lines / ~68% functions (gap = `testplugin.c` foreign-plugin checks)

The ~26% of C the testbed never reaches is mostly: error/failure paths, uncommon pixel
formats, MPE (multi-process-element) types, `cam02` (0% under the testbed — the Rust side
is covered only by `c_parity_cam02`), uncommon tag handlers, profile builders. To see
exactly what, run `just coverage-testbed` then inspect the html report
(`llvm-cov-14 show bench/out/coverage/testcms2_cov -instr-profile=<profdata>`).

## Task

Raise the exercised surface meaningfully, with a trustworthy oracle for each path:

1. **Prioritize** the zero/low-coverage C functions: `cmscam02.c` (0% under testbed),
   `cmsalpha.c`, `cmsio1.c`, `cmscgats.c`, `cmspcs.c`, `cmstypes.c` uncovered regions —
   pull the per-line `llvm-cov show` output to find them.
2. For each function/region, build a gate where **C is the oracle**: extend or add a
   `c_parity_*.rs` test that drives the C symbol and the Rust port over the same inputs and
   asserts equality. For C `static` fns, add a shim in `lcms-cref/shims/` following the
   existing pattern (e.g. `inline_oracle.c`).
3. For error/failure paths, the oracle is "returns the same error / NULL / status": feed
   malformed inputs and compare both sides' result, not just the success value.
4. Where the C side of a path needs a driver the testbed lacks (e.g. a specific tag type or
   pixel format), drive it through the existing `lcms-cref` FFI in the parity test — not
   through a new C binary.
5. Re-measure: `just coverage-testbed` and `cargo llvm-cov -p lcms --summary-only` — report
   the before/after per-file numbers.

Report format in `PORTING.md` (or a new `docs/COVERAGE.md`): which C functions are now
gated, the gate for each, and the before/after coverage numbers per file.

## Constraints

- Rust product crate is `#![forbid(unsafe_code)]` — unsafe goes only in the FFI/cref crates.
- The bit-exact contract is `cc -O2 -fno-strict-aliasing` — don't change oracle build flags.
- Coverage instrumentation uses clang for the C side (that's fine — it's measurement, not
  the parity binary). Don't confuse "coverage builds" with the parity contract.
- Do not weaken any existing gate to gain coverage.
