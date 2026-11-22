# Rust quality review

This review covers the Rust API, the C migration surfaces, and the workspace
package at the SpecDefault change. The prior behavior baseline is
`e8a67f81`. The clean consumer used Rust 1.99.1 on x86_64 Linux and the
default features. Packaging and minimum-toolchain checks use that same
consumer surface.

| Practice | Status | Finding |
|---|---|---|
| Public correctness | Verified for the tested surface | The public conformance tests exercise the ICC modes and the C-parity tests exercise the default. The corpus adds read, link, float samples and save/reopen coverage for real profiles. Rust tests cover packed, strided and planar pixel layouts. [Conformance and limits](CONFORMANCE.md) |
| Safe operation and actionable failures | Partial | Profile and plan operations return typed errors. Packed and strided pixel methods document size panics. The Rust core forbids unsafe code. The C ABI has a separate raw-pointer boundary and a different audit scope. The API reads entire files into memory, some internal table allocations are fallible, and there is no caller-supplied profile size or allocation budget. Long linking and parallel application lack caller-controlled cancellation. [Rust API](../crates/lcms/src/api.rs) · [C ABI](../crates/lcms2-ffi/src/lib.rs) |
| Compatible upgrades | Partial | The committed API snapshots pass. The workspace declares Rust 1.89, which passed the all-targets check on x86_64 Linux. The previous Rust 1.85 declaration failed because the resolved archmage family requires Rust 1.89 and `safe_unaligned_simd` requires 1.88. This review has no previous packaged release comparison or semver-checks result. [Workspace manifest](../Cargo.toml) · [API snapshot](public-api/lcms.txt) |
| First useful result | Verified for a configured local checkout | The README's RGB-to-Lab example compiled and ran in a separate consumer using the local crate as a path dependency. The example selects an ICC.1 mode explicitly and has a tested output assertion. [Quickstart](../README.md#quickstart) |
| Registry and API reference | Partial | Source doctests and public API snapshots pass. `cargo package --list` lists source without a package README. Actual packaging stops at the path-only `archmage` dependency, which lacks a version requirement. A staged package and docs.rs build remain unverified. [Package manifest](../crates/lcms/Cargo.toml) |
| Cross-API guide | Partial | The Rust README routes callers to the native, migration and C-shaped APIs. The porting and conformance docs explain the implementation contract. The local quickstart and API guide are the current entry points. [Rust README](../README.md) · [Porting map](../../PORTING.md) |
| Browser payload | Inapplicable to this review | The audited entry points are native Rust and C. No browser build or download claim is made here. |
| Reproducible measurements and limits | Partial | The conformance audit identifies the oracle, fixtures, inputs and comparison method. The root README's throughput table needs a recorded benchmark revision and environment before it can serve as release performance evidence. [ICC measurements](CONFORMANCE.md) · [Benchmark entry points](../../README.md#performance) |

## Shared interfaces and release priorities

- For future cancellable operations, start at the plan-link and multi-row
  execution boundaries. Preserve the existing methods and document the
  completed output if a caller cancels. The current API has no borrowed
  `how-far` pulse.
- The current `api::Error` stores a Little CMS code and a message.
  `open_file` and `save_file` stringify the underlying I/O error.
  Add a typed source and a `whereat` trace through an additive API review.
- The library processes profiles and transforms pixels. The shared image
  codec contract applies to callers that encode and decode image files.
  The existing native pixel APIs already accept caller strides.
  An additive `zenpixels` adapter would help callers that use shared views.
- Resolve package dependencies and the supported toolchain floor before
  making registry-install claims. Then verify the staged package's README,
  compile a clean consumer from the package, and check native target coverage.
  The audited revision has no checked-in release workflow under
  `.github/workflows`.
- The default normal/build dependency tree was resolved from the workspace
  lockfile and the configured archmage checkout at
  `c8c7f5e223fa96187988e09ba4168fb24318105f`. A local
  `cargo audit --no-fetch --file Cargo.lock` with cargo-audit 0.22.1 and
  RustSec database `3279c99911354c400e1d41e47c7d60e39258b147` reported
  no advisories in the workspace lockfile. This check covers matching
  database entries for those resolved dependency versions.
