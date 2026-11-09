# lcms — pure-Rust Little CMS

A bit-exact Rust port of Little CMS 2.20 (`reference/lcms2`, upstream
`lcms2.20` @ `7717fce`), with a differential test suite that proves every
exercised code path produces identical output to the C oracle — serialized
profiles, transformed pixels, scalar conversions, rejection behavior —
all driven through `cargo test` against the real compiled C library.

`#![forbid(unsafe_code)]` — the shipped dependency tree is 100% safe Rust.
The C oracle (`lcms-cref`, dev-only) never reaches the production deps.

## Two API surfaces

### `lcms::*` — the C-parity layer

`cmsCreateTransform`, `cmsDoTransform`, `IccProfile`, `Pipeline`, the
`cmsPACK`/`fmt` machinery — 1:1 with the C source, same operation order.
This is what the ~30 `c_parity_*` differential suites gate against the
oracle. Use it when you need C-shaped behavior or are porting C callers.

### `lcms::api` — the idiomatic layer

Owned, `Result`-returning, `Send`/`Sync`-friendly wrappers — what a real
Rust app uses:

```rust
use lcms::api::*;

// Load / build profiles
let mut srgb = Profile::srgb()?;
let mut p3 = Profile::open(&std::fs::read("Display-P3.icc")?)?;

// Link once — this is the expensive part (pipeline + optimizer +
// CLUT flattening). The plan is Sync: Arc-share it across workers.
let plan = Plan::new(&mut p3, &mut srgb, Format::RgbU8, Format::RgbU8)?;

// Per-request scratch — ~64 B, or pooled thread_local.
let mut sc = plan.scratch();
let src = [255u8, 0, 0];
let mut dst = [0u8; 3];
plan.apply(&mut sc, &src, &mut dst);

// Large images: pixel-independent split, scratch-per-chunk.
// plan.apply_par(&pixels_in, &mut pixels_out, n_pixels, n_threads);
```

## The shared-plan model (for servers)

The object model is split deliberately for concurrent services:

| | type | mutability | sharing |
|---|---|---|---|
| **profile** | `Profile` / `IccProfile` | lazy-materializes on read | `freeze()` → `Sync`, `Arc` for reads |
| **plan** | `Plan` / `TransformPlan` | **immutable after link** | `Sync` — `Arc` shared, cache by `(src, dst, fmt_in, fmt_out, intent, flags)` |
| **scratch** | `Scratch` / `TransformScratch` | the only per-run state (~64 B pixel cache) | per-call / `thread_local` |

So a proxy server does: profile cache (`Arc` by content-hash) → plan cache
(`Arc<Plan>` by config key) → per-request `plan.scratch()` → `apply`. The
whole pixel path is `&self` on shared state; nothing blocks on a lock.

## The math provider

`lcms::math` selects the transcendental backend at compile time
(default `math-libm` — the `libm` crate, deterministic on every platform).
`math-rust` uses `f64` intrinsics; `math-zen` is a reserved slot for a
future correctly-rounded implementation.

## Parity & determinism (development)

- **51/51 bit-exact** on the determinism manifest (`cargo test --test
  determinism`): 12 serialization resaves, u16+f32 transforms across
  profile pairs/intents/devicelinks/mAB/mPET/CICP, 13 scalar cells —
  every cell `C == R` byte-for-byte.
- **Canonical-math oracle**: the `lcms-cref` test crate's `extern "C"`
  `pow`/`sin`/… exports interpose the C oracle's own libm calls, so the
  oracle runs the *same* `libm` code the port uses — parity is exact on
  every platform by construction. `LCMS_CREF_MATH=sys` restores the
  system-libm oracle for math-delta documentation.
- **Cross-platform**: `Rust == C` verified on glibc 2.35/2.39, aarch64,
  macOS arm64, musl, and Windows MSVC (wine).
- `just determinism-gate` runs the CI parity+confinement gate;
  `just determinism-goldens <dir>` writes per-platform hash baselines.

## Layout

- `crates/lcms` — the port (`api` for apps, `pub mod cms*` for parity)
- `crates/lcms-cref` — the unmodified-lcms2 oracle FFI (dev-dep only)
- `crates/lcms2-ffi` — C-ABI wrapper (upstream testbed driver)
- `docs/` — `WORKING-ON-THIS` (port loop + traps), `COVERAGE`, `C-BUGS`
  (confirmed upstream quirks reproduced), `PORTING.md` (C→Rust map)
