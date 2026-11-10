# lcms — pure-Rust Little CMS

A bit-exact Rust port of Little CMS 2.20 (`reference/lcms2`, upstream
`lcms2.20` @ `7717fce`), with a differential test suite that proves every
exercised code path produces identical output to the C oracle — serialized
profiles, transformed pixels, scalar conversions, rejection behavior —
all driven through `cargo test` against the real compiled C library.

`#![forbid(unsafe_code)]` — the shipped dependency tree is 100% safe Rust.
The C oracle (`lcms-cref`, dev-only) never reaches the production deps.

## Quickstart

Pick the layer that matches what you're doing:

| You want… | Use | Feature |
|---|---|---|
| a Rust-native API | `lcms::api` | *(always on)* |
| to migrate from the **`lcms2` crate** | `lcms::lcms2` | `lcms2-api` |
| C-shaped calls (`cms*`, 1:1 parity) | `lcms::*` | `little-cms-api` *(opt-in)* |

### `lcms::api` — the idiomatic face (recommended)

```rust
use lcms::api::*;

// Profiles decode every tag at construction: `&Profile` reads are Sync.
let srgb = Profile::srgb();
let p3 = Profile::open_file("Display-P3.icc")?;

// Link once — the expensive part. Plan is Sync: Arc-share it.
let plan = Plan::new(&p3, &srgb, Format::RGB_8, Format::RGB_8)?;

// One-off: the plan makes its own scratch.
let mut out = [[0u8; 3]; 1];
plan.transform_pixels(&[[255u8, 0, 0]], &mut out);

// Hot loop: one ~64 B Scratch per thread.
let mut sc = plan.scratch();
plan.apply(&mut sc, &rgb_bytes, &mut out_bytes);             // packed
plan.apply_strided(&mut sc, &src, src_stride, &mut dst, dst_stride, width, height);
plan.apply_par(&rgb_bytes, &mut out_bytes, 8);               // scoped threads

// Intent, black-point compensation, adaptation:
let opts = PlanOptions::new().intent(Intent::RelativeColorimetric).black_point_compensation(true);
let plan = Plan::with_options(&p3, &srgb, Format::RGBA_8, Format::RGBA_8, &opts)?;
```

Plans link exactly as C's `cmsCreateTransform` / multiprofile / proofing
transforms do; `tests/api_c_parity.rs` checks the facade's output byte for
byte against the C library across intents, BPC, formats and real profiles.
Buffer sizes that don't match a plan's formats panic; anything that depends
on input data returns `Result`.

Read/write tags, named colors, header fields:

```rust
for tag in srgb.tags() { println!("{tag}: {:?}", srgb.get(tag)); }
let wp = srgb.white_point();                         // Option<&CieXyz>
let desc = srgb.info(InfoType::Description);         // localized text
let mut p = Profile::rgb(&D65, &REC709_PRIMARIES, [&trc, &trc, &trc])?;
p.write_tag(Tag::COPYRIGHT, "No rights reserved")?;  // &str, CieXyz, ToneCurve, ...
let bytes = p.save()?;
```

### `lcms::lcms2` — drop-in migration from the `lcms2` crate

```toml
[dependencies]
lcms = { path = "lcms", features = ["lcms2-api"] }
```

Change one line — `use lcms2::*` → `use lcms::lcms2::*` — and the same code
compiles:

```rust
use lcms::lcms2::*;

let srgb = Profile::new_srgb();
let t: Transform<[u8; 3], [u8; 3]> =
    Transform::new(&srgb, PixelFormat::RGB_8, &srgb, PixelFormat::RGB_8,
                   Intent::Perceptual)?;
let mut dst = [[0u8; 3]];
t.transform_pixels(&[[255, 0, 0]], &mut dst);

let mut mlu = MLU::new(1);
mlu.set_text("My profile", Locale::none());
```

The official `lcms2` crate's own test suite runs verbatim against this
shim (`tests/lcms2_crate/` — 15/15, MIT-credited extraction). Every public
type is `Send` + `Sync`.

**Differences to know when migrating:**

- `transform_pixels_uninit` doesn't exist — `forbid(unsafe_code)` can't
  return `&mut [O]` over `MaybeUninit`. Use `transform_pixels` or
  `transform_in_place`.
- FFI-shaped methods (`as_ptr`/`from_ptr`/`CType`/`Owned`/`Target`) don't
  exist — there's no raw-handle layer. `Deref` gives the same `Ref` model.
- `Transform` is `Sync` *stronger* than the C crate — `transform_pixels
  (&self)` locks the per-transform pixel cache (`Mutex` inside), so a
  shared `Arc<Transform>` is safe. For lock-free parallelism use
  `lcms::api`'s `Plan` + per-call `Scratch` instead.
- `read_tag` returns `Tag<'a>` borrowed from a frozen profile — `&self`,
  zero-cost, `Tag::None` for missing tags (same shape as the crate).

See `docs/public-api/lcms2-comparison.md` for the full surface diff vs
`lcms2` 6.2.0.

### `lcms::*` — the C-parity layer

`cmsCreateTransform`, `cmsDoTransform`, `IccProfile`, `Pipeline`, the
`cmsPACK`/`fmt` machinery — 1:1 with the C source, same operation order.
Gated by the `little-cms-api` feature (default on; without it the modules
are crate-private and consumers see only `lcms::api`). This is what the
~30 `c_parity_*` differential suites gate against the oracle — use it when
you need C-shaped behavior or are porting C callers.

## The shared-plan model (for servers)

The object model is split deliberately for concurrent services:

| | type | mutability | sharing |
|---|---|---|---|
| **profile** | `Profile` / `IccProfile` | lazy-materializes on read | `freeze()` → `Sync`, `Arc` for reads |
| **plan** | `Plan` / `TransformPlan` | **immutable after link** | `Sync` — `Arc` shared, cache by `(src, dst, fmt_in, fmt_out, intent, flags)` |
| **scratch** | `Scratch` / `TransformScratch` | the only per-run state (~64 B pixel cache) | per-call / `thread_local` |

A proxy server does: profile cache (`Arc` by content-hash) → plan cache
(`Arc<Plan>` by config key) → per-request `plan.scratch()` → `apply`. The
whole pixel path is `&self` on shared state — no locks anywhere.

## The math provider

`lcms::math` selects the transcendental backend at compile time (default
`math-libm` — the `libm` crate, deterministic on every platform).
`math-rust` uses `f64` intrinsics; `math-zen` is a reserved slot for a
future correctly-rounded implementation. The providers are mutually
exclusive.

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

- `crates/lcms` — the port (`api` for apps, `lcms2` for migration,
  `little-cms-api` for C parity)
- `crates/lcms-cref` — the unmodified-lcms2 oracle FFI (dev-dep only)
- `crates/lcms2-ffi` — C-ABI wrapper (upstream testbed driver)
- `docs/` — `WORKING-ON-THIS` (port loop + traps), `COVERAGE`, `C-BUGS`
  (confirmed upstream quirks reproduced), `PORTING.md` (C→Rust map)
