# lcms — Little CMS in Rust

Open ICC profiles and build reusable color transforms with the Rust API.
The default mode tracks the pinned Little CMS 2.20 implementation.
`SpecExact` and `SpecDefault` apply the implemented ICC.1 transform and
export rules. The Rust core forbids unsafe code. The optional C ABI and
differential test oracle have separate crates.

## Quickstart

From a checkout, add `lcms` as a path dependency pointing to
`rust/crates/lcms`. Use Rust 1.89 or later. The Rust API needs no surface feature.

Pick the layer that matches what you're doing:

| You want… | Use | Feature |
|---|---|---|
| a Rust-native API | `lcms::api` | *(always on)* |
| to migrate from the **`lcms2` crate** | `lcms::lcms2` | `lcms2-api` |
| C-shaped calls (`cms*`, 1:1 parity) | `lcms::*` | `little-cms-api` *(opt-in)* |

### `lcms::api` — the idiomatic face (recommended)

```rust
use lcms::api::{BehaviorMode, Format, Plan, Profile};

fn main() -> Result<(), lcms::api::Error> {
    let profiles = Profile::with_mode(BehaviorMode::SpecDefault);
    let plan = Plan::new(
        &profiles.srgb(),
        &profiles.lab4(),
        Format::RGB_8,
        Format::LAB_DBL,
    )?;
    let mut out = [[0.0_f64; 3]];
    plan.transform_pixels(&[[255_u8, 0, 0]], &mut out);
    assert!(out[0][1] > 60.0); // Red has a positive a* value.
    Ok(())
}
```

Use `profiles.open(&icc_bytes)` for embedded profiles.
`SpecDefault` reports supported legacy recoveries through
`Profile::read_warnings`. Use `SpecExact` when checking the supported ICC rules
and `Lcms2Exact` when matching Little CMS 2.20 output. See
[the ICC conformance audit](docs/CONFORMANCE.md) for the covered rules.
The [quality review](docs/QUALITY.md) records packaging and toolchain limits.

Link once and reuse a plan across workers. `Plan::apply_strided` handles
caller-owned rows with padding. A buffer whose length disagrees with the
selected format is a programming error and can panic. Profile reads,
writes, and link construction return typed errors.

Read/write tags, named colors, header fields:

```rust
for tag in srgb.tags() { println!("{tag}: {:?}", srgb.get(tag)); }
let wp = srgb.white_point();                         // Option<&CieXyz>
let desc = srgb.info(InfoType::Description);         // localized text
let mut p = Profile::rgb(&D65, &REC709_PRIMARIES, [&trc, &trc, &trc])?;
p.write_tag(Tag::COPYRIGHT, "No rights reserved")?;  // &str, CieXyz, ToneCurve, ...
let bytes = p.save()?;
```

Tag payloads are ordinary Rust values. `TagValue` borrows straight into the
profile (`&ToneCurve`, `&Pipeline`, `&Mlu`, `&NamedColorList`, ...) at no
cost, and the same types are what you build and hand to `write_tag`:

```rust
let trc = srgb.get(Tag::RED_TRC).and_then(|v| v.as_curve()).unwrap();
let inv = trc.inverse()?;                              // an owned ToneCurve
let mut lut = Pipeline::new(3, 3)?;
lut.push(Stage::tone_curves(&[trc, trc, trc])?)?;      // a failed push changes nothing
let mut name = Mlu::from("Display");
name.set(Locale::new("de_DE"), "Bildschirm")?;
for color in srgb.named_colors().into_iter().flatten() { println!("{}", color.name); }
```

They are the parity layer's own types with the C bookkeeping hidden: their
fields and C-shaped methods are crate-private unless `little-cms-api` is on,
so borrowing costs nothing and no `lcms::api` signature exposes an internal
type (`docs/public-api/lcms.txt` is the checked snapshot).

### Select ICC behavior

Existing constructors, the `lcms::lcms2` migration facade, and the
`lcms2-ffi` C ABI use `BehaviorMode::Lcms2Exact` for Little CMS 2.20
compatibility. Rust-native callers can select `SpecExact` or `SpecDefault`
with a `ProfileFactory`. A plan can link profiles from either ICC.1 mode.
It retains the first profile's mode when it builds a DeviceLink. Reopen
the saved bytes with a factory to select the behavior for subsequent
operations.

```rust
use lcms::api::{BehaviorMode, Format, Plan, Profile, ToneCurve};

let icc = Profile::with_mode(BehaviorMode::SpecExact);
let source = icc.srgb();
let destination = icc.lab4();
let plan = Plan::new(&source, &destination, Format::RGB_8, Format::LAB_16)?;
let link = plan.to_device_link(4.4)?;
let bytes = link.save()?;
let reopened = icc.open(&bytes)?;
let inverse = ToneCurve::srgb().with_mode(BehaviorMode::SpecExact)?.inverse()?;
# Ok::<(), lcms::api::Error>(())
```

The factory provides the standard built-ins, custom profiles, placeholders
and file or memory open. `Profile::set_version` selects a v2 or v4 writer.
If the target version cannot represent a tag, `save` returns
`ErrorKind::NotSuitable`. Both ICC.1 modes calculate the profile ID on v4 save.
Use `write_cicp_with_chromaticities` and `cicp_chromaticities` for amended
CICP. The coordinates are the eight exact s15Fixed16 words.

ICC `curveType` and `parametricCurveType` data use bounded evaluation and
the Annex F.1 inverse. Explicit unbounded math and MPE formulas remain
available. The [conformance audit](docs/CONFORMANCE.md#specexact-disposition)
lists the supported ICC rules, refusals and validation scope.

For normalized packed RGB and RGBA float input, callers can opt into
sampled matrix-shaper curves with
`PlanOptions::new().approximate_float_curves(true)`. Use exact evaluation
for code-for-code compatibility, or the sampled option when the reported
error fits the application. [Supported paths and measurements](docs/TRANSFORM-THROUGHPUT.md#sampled-float-curves-and-packed-byte-output)

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
Gated by the `little-cms-api` feature (opt-in; without it the modules
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

## Cargo features

Ordinary Rust consumers can use the default features. Cargo selects
`math-libm` for the transcendental math backend. Builds that disable
default features select one provider explicitly. `math-rust` uses
`f64` methods, and `math-zen` currently uses the same implementation
while reserving the feature name for a future provider.

Use `little-cms-api` for the C-shaped Rust surface and `lcms2-api`
for the migration facade. ICC behavior modes are runtime API choices.
The integer transform loop selects its available SIMD tier automatically.

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
- `just bench-cms` compares packed transforms with stock Little CMS and
  moxcms. See the [throughput measurements](docs/TRANSFORM-THROUGHPUT.md).

## Layout

- `crates/lcms` — the port (`api` for apps, `lcms2` for migration,
  `little-cms-api` for C parity)
- `crates/lcms-cref` — the unmodified-lcms2 oracle FFI (dev-dep only)
- `crates/lcms2-ffi` — C-ABI wrapper (upstream testbed driver)
- `docs/` — `WORKING-ON-THIS` (port loop + traps), `COVERAGE`, `C-BUGS`
  (confirmed upstream quirks reproduced), `CONFORMANCE` (ICC specification
  audit: where lcms2.20, and so the port, departs from ICC.1),
  `PORTING.md` (C→Rust map)
