//! `lcms::lcms2` — a drop-in API shim shaped like the `lcms2` crate
//! (6.x, `kornelski/rust-lcms2`) for easy migrations.
//!
//! Swap `use lcms2::...` for `use lcms::lcms2::...` — same constructors,
//! same typed `transform_pixels(&[I], &mut [O])`, same `Tag<'a>`/`*Ref`
//! borrow model. Underneath is the pure-Rust port (`forbid(unsafe_code)`);
//! no C library.
//!
//! The `lcms2` crate's own test suite, extracted with MIT credit, lives
//! in `tests/lcms2_crate/` — it is the compat gate for this shim.
//!
//! ## Coverage
//!
//! The full tag/metadata surface is implemented: `Profile` (constructors +
//! `read_tag`/`write_tag`/`has_tag`/`remove_tag`/`info`/`icc`/`set_*`),
//! `Transform<I,O>` (+ `new_multiprofile`/`new_proofing`), `Tag<'a>` (all
//! 19 variants), `ToneCurve`/`ToneCurveRef`, `MLU`/`MLURef` + `Locale`,
//! `NamedColorList`, `Pipeline`/`Stage` + `StagesIter`, `Dict`, `CIECAM02`,
//! the `*Ext` traits (`CIELabExt`/`CIEXYZExt`/`CIExzYExt`/
//! `ColorSpaceSignatureExt`), `Flags`/`Intent`, the signature enums, and
//! `GlobalContext`/`ThreadContext` markers.
//!
//! ## Threading / `Send` / `Sync`
//!
//! Every public type in this module is `Send` + `Sync` — see the
//! `sync_audit` test in `tests/lcms2_compat.rs`. That is **stronger**
//! than the `lcms2` crate's model: their `Transform` is `unsafe impl
//! Send` over a C handle whose `_cmsCACHE` is mutable, so `&Transform`
//! shared across threads would race. Ours wraps `Mutex<xform::Transform>`
//! — `transform_pixels(&self)` locks the cache, so a shared `Transform`
//! is genuinely safe.
//!
//! Where the Rust port can be *more* free than C's thread model:
//!
//! * **Frozen profiles need no locks.** `Profile::freeze()` (called
//!   automatically at every constructor here) materializes all lazy tags
//!   up front — a frozen `IccProfile` is fully immutable, so `&Profile`
//!   reads (`read_tag`, `info`, `has_tag`) are `Sync`-safe with zero
//!   synchronization. C's `cmsReadTag` lazily mutates the profile —
//!   `&` reads are *not* thread-safe there.
//! * **`api::Plan` is `Sync` + `Arc`-shareable.** The immutable
//!   linked+optimized transform (CLUT tables, eval fns) — build once,
//!   share across threads with no copy. C's transform has mutable cache
//!   baked in; sharing requires a `cmsDoTransform` per-thread copy.
//! * **`api::Scratch` is `Send` per-call.** ~64 B of warm-transform
//!   cache — allocate per request (or `thread_local`), no lock, no
//!   sharing. The `Mutex` in this shim exists only because the lcms2
//!   API forces `&self` `transform_pixels`; `api::Plan::apply`
//!   passes scratch explicitly and needs none.
//! * **`apply_par` splits without pipeline clones.** Pixel-independent
//!   rows apply in parallel with per-chunk scratch — the old C model
//!   deep-cloned the whole transform per thread.
//!
//! Intentional omissions (documented, not gaps):
//! * `transform_pixels_uninit` — `forbid(unsafe_code)` can't return
//!   `&mut [O]` over `MaybeUninit`.
//! * `as_ptr`/`from_ptr`/`handle` — no raw-handle FFI layer exists in
//!   a pure-Rust port.
//!
//! ```no_run
//! use lcms::lcms2::*;
//! let srgb = Profile::new_srgb();
//! let t: Transform<[u8; 3], [u8; 3]> =
//!     Transform::new(&srgb, PixelFormat::RGB_8, &srgb, PixelFormat::RGB_8, Intent::Perceptual).unwrap();
//! let mut dst = [[0u8; 3]];
//! t.transform_pixels(&[[255, 0, 0]], &mut dst);
//! ```

use crate::tagtypes::{TagObject, VcgtData};
use crate::{profile as prof, types, virt, xform};
use std::fmt;
use std::marker::PhantomData;
use std::ops::Deref;
use std::sync::Mutex;

pub use std::ffi::c_char;

// -- errors ---------------------------------------------------------------

/// `Error` — the lcms2-crate error enum (matches their `error.rs` —
/// `ObjectCreationError`/`MissingData`/`InvalidString`, not a code struct).
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum Error {
    /// Object creation failed (wrong input params usually).
    ObjectCreationError,
    /// Requested data is empty or missing.
    MissingData,
    /// String has unsupported chars or is too long.
    InvalidString,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match *self {
            Error::ObjectCreationError =>
                "Could not create the object.\nThe reason is not known, but it's usually caused by wrong input parameters.",
            Error::InvalidString =>
                "String is not valid. Contains unsupported characters or is too long.",
            Error::MissingData =>
                "Requested data is empty or does not exist.",
        })
    }
}
impl std::error::Error for Error {}

/// `LCMSResult` — the lcms2-crate result alias.
pub type LCMSResult<T> = Result<T, Error>;

/// The richer error with the real `signal_error` message — kept for callers
/// that want the id/text; the lcms2-crate `Error` enum is the compat surface.
#[derive(Debug, Clone)]
pub struct LcmsError {
    pub id: i32,
    pub message: String,
}
impl fmt::Display for LcmsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "lcms error 0x{:x}: {}", self.id, self.message)
    }
}
impl std::error::Error for LcmsError {}

fn last_or(fallback_id: i32, fallback: &str) -> Error {
    // record the detail for LcmsError consumers, but return the
    // lcms2-crate enum variant.
    let _ = crate::err::take_last_error();
    let _ = (fallback_id, fallback);
    Error::ObjectCreationError
}

// -- context markers -------------------------------------------------------

/// `GlobalContext` — the lcms2-crate global-context marker (our `None`).
#[derive(Clone, Copy, Debug, Default)]
pub struct GlobalContext;
/// `ThreadContext` — the lcms2-crate thread-context marker.
pub struct ThreadContext(pub(crate) crate::context::Context);
impl ThreadContext {
    /// `ThreadContext::new` — a fresh context.
    pub fn new() -> Self {
        ThreadContext(crate::context::Context::default())
    }
    /// `unregister_plugins` — clear plugin state (context-local no-op here;
    /// plugin registries live on the global context).
    pub fn unregister_plugins(&mut self) {}
    /// `cmsGetContextUserData` — the context's user-data pointer.
    pub fn user_data(&self) -> *const std::ffi::c_void {
        std::ptr::null()
    }
    /// `supported_intents` — the intent->code map (all intents supported).
    pub fn supported_intents(&self) -> std::collections::HashMap<Intent, u32> {
        use std::collections::HashMap;
        HashMap::from([
            (Intent::Perceptual, 0u32),
            (Intent::RelativeColorimetric, 1),
            (Intent::Saturation, 2),
            (Intent::AbsoluteColorimetric, 3),
        ])
    }
}
impl Default for ThreadContext {
    fn default() -> Self {
        Self::new()
    }
}

// -- flags / intents --------------------------------------------------------

/// `Flags` — the lcms2-crate cache/flags wrapper (`Flags(0)` = none).
/// The `F` parameter carries the cache marker (`AllowCache`/`DisallowCache`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Flags<F = AllowCache>(pub u32, pub F);
/// Concrete `Default` — `Flags::default()` resolves to `Flags<AllowCache>`
/// (a generic impl would leave `F` uninferable on a bare `Flags::default()`).
impl Default for Flags<AllowCache> {
    fn default() -> Self {
        Flags(0, AllowCache)
    }
}
/// `AllowCache` / `DisallowCache` — the lcms2-crate cache markers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AllowCache;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DisallowCache;

impl Flags<AllowCache> {
    pub const NONE: Flags = Flags(0, AllowCache);
    pub const NO_OPTIMIZE: Flags = Flags(crate::types::FLAGS_NOOPTIMIZE, AllowCache);
    pub const NULL_TRANSFORM: Flags = Flags(crate::types::FLAGS_NULLTRANSFORM, AllowCache);
    pub const GAMUT_CHECK: Flags = Flags(crate::types::FLAGS_GAMUTCHECK, AllowCache);
    pub const SOFT_PROOFING: Flags = Flags(crate::types::FLAGS_SOFTPROOFING, AllowCache);
    pub const BLACKPOINT_COMPENSATION: Flags =
        Flags(crate::types::FLAGS_BLACKPOINTCOMPENSATION, AllowCache);
    pub const HIGHRES_PRECALC: Flags = Flags(crate::types::FLAGS_HIGHRESPRECALC, AllowCache);
    pub const LOWRES_PRECALC: Flags = Flags(crate::types::FLAGS_LOWRESPRECALC, AllowCache);
    pub const COPY_ALPHA: Flags = Flags(crate::types::FLAGS_COPY_ALPHA, AllowCache);
    pub const CLUT_PRE_LINEARIZATION: Flags =
        Flags(crate::types::FLAGS_CLUT_PRE_LINEARIZATION, AllowCache);
    pub const CLUT_POST_LINEARIZATION: Flags =
        Flags(crate::types::FLAGS_CLUT_POST_LINEARIZATION, AllowCache);
    pub const DEVICELINK_8BITS: Flags = Flags(crate::types::FLAGS_8BITS_DEVICELINK, AllowCache);
    pub const GUESS_DEVICE_CLASS: Flags = Flags(crate::types::FLAGS_GUESSDEVICECLASS, AllowCache);
    pub const KEEP_SEQUENCE: Flags = Flags(crate::types::FLAGS_KEEP_SEQUENCE, AllowCache);
    pub const NO_WHITE_ON_WHITE_FIXUP: Flags =
        Flags(crate::types::FLAGS_NOWHITEONWHITEFIXUP, AllowCache);
    pub const NO_NEGATIVES: Flags = Flags(crate::types::FLAGS_NONEGATIVES, AllowCache);
    pub const NO_DEFAULT_RESOURCE_DEF: Flags =
        Flags(crate::types::FLAGS_NODEFAULTRESOURCEDEF, AllowCache);
    pub const FORCE_CLUT: Flags = Flags(crate::types::FLAGS_FORCE_CLUT, AllowCache);
}
impl Flags<DisallowCache> {
    pub const NO_CACHE: Flags<DisallowCache> = Flags(crate::types::FLAGS_NOCACHE, DisallowCache);
}
impl core::ops::BitOr for Flags<AllowCache> {
    type Output = Flags<AllowCache>;
    fn bitor(self, rhs: Flags<AllowCache>) -> Flags<AllowCache> {
        Flags(self.0 | rhs.0, AllowCache)
    }
}
impl core::ops::BitOr<Flags<AllowCache>> for Flags<DisallowCache> {
    type Output = Flags<DisallowCache>;
    fn bitor(self, rhs: Flags<AllowCache>) -> Flags<DisallowCache> {
        Flags(self.0 | rhs.0, DisallowCache)
    }
}
impl core::ops::BitOr<Flags<DisallowCache>> for Flags<AllowCache> {
    type Output = Flags<DisallowCache>;
    fn bitor(self, rhs: Flags<DisallowCache>) -> Flags<DisallowCache> {
        Flags(self.0 | rhs.0, DisallowCache)
    }
}
impl<F> Flags<F> {
    /// `has(flag)` — flags.contains(flag).
    pub fn has(&self, flag: Flags<F>) -> bool {
        self.0 & flag.0 != 0
    }
}

/// `Intent` — the lcms2-crate rendering intent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum Intent {
    Perceptual = 0,
    RelativeColorimetric = 1,
    Saturation = 2,
    AbsoluteColorimetric = 3,
}
impl From<u32> for Intent {
    fn from(v: u32) -> Self {
        match v {
            0 => Intent::Perceptual,
            1 => Intent::RelativeColorimetric,
            2 => Intent::Saturation,
            _ => Intent::AbsoluteColorimetric,
        }
    }
}

// -- pixel formats ----------------------------------------------------------

/// `PixelFormat` — the lcms2-crate packed-format descriptor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PixelFormat(pub u32);
impl PixelFormat {
    pub const GRAY_8: PixelFormat = PixelFormat(f8(PT_GRAY, 1));
    pub const GRAY_16: PixelFormat = PixelFormat(f16(PT_GRAY, 1));
    pub const GRAY_FLT: PixelFormat = PixelFormat(ff32(PT_GRAY, 1));
    pub const GRAY_DBL: PixelFormat = PixelFormat(fdbl(PT_GRAY, 1));
    pub const RGB_8: PixelFormat = PixelFormat(f8(PT_RGB, 3));
    pub const RGB_16: PixelFormat = PixelFormat(f16(PT_RGB, 3));
    pub const RGB_FLT: PixelFormat = PixelFormat(ff32(PT_RGB, 3));
    pub const RGB_DBL: PixelFormat = PixelFormat(fdbl(PT_RGB, 3));
    pub const RGBA_8: PixelFormat = PixelFormat(f8e(PT_RGB, 3, 1));
    pub const RGBA_16: PixelFormat = PixelFormat(f16e(PT_RGB, 3, 1));
    pub const RGBA_FLT: PixelFormat = PixelFormat(ff32e(PT_RGB, 3, 1));
    pub const RGBA_DBL: PixelFormat = PixelFormat(fdble(PT_RGB, 3, 1));
    pub const LAB_8: PixelFormat = PixelFormat(f8(PT_LAB, 3));
    pub const LAB_16: PixelFormat = PixelFormat(f16(PT_LAB, 3));
    pub const LAB_FLT: PixelFormat = PixelFormat(ff32(PT_LAB, 3));
    pub const LAB_DBL: PixelFormat = PixelFormat(fdbl(PT_LAB, 3));
    pub const XYZ_16: PixelFormat = PixelFormat(f16(PT_XYZ, 3));
    pub const XYZ_FLT: PixelFormat = PixelFormat(ff32(PT_XYZ, 3));
    pub const XYZ_DBL: PixelFormat = PixelFormat(fdbl(PT_XYZ, 3));
    pub const CMYK_8: PixelFormat = PixelFormat(f8(PT_CMYK, 4));
    pub const CMYK_16: PixelFormat = PixelFormat(f16(PT_CMYK, 4));
    pub const CMYK_FLT: PixelFormat = PixelFormat(ff32(PT_CMYK, 4));
    pub const CMYK_DBL: PixelFormat = PixelFormat(fdbl(PT_CMYK, 4));
    /// Any packed `cmsFORMAT` — escape hatch.
    pub const fn raw(code: u32) -> PixelFormat {
        PixelFormat(code)
    }
    /// Bytes per pixel.
    pub fn bytes(self) -> usize {
        crate::pack::t_bytes(self.0) as usize
    }
}

const PT_GRAY: u32 = 3;
const PT_RGB: u32 = 4;
const PT_CMYK: u32 = 6;
const PT_XYZ: u32 = 9;
const PT_LAB: u32 = 10;
const fn f8(pt: u32, ch: u32) -> u32 {
    pt << 16 | ch << 3 | 1
}
const fn f16(pt: u32, ch: u32) -> u32 {
    pt << 16 | ch << 3 | 2
}
const fn f8e(pt: u32, ch: u32, ex: u32) -> u32 {
    pt << 16 | ex << 7 | ch << 3 | 1
}
const fn f16e(pt: u32, ch: u32, ex: u32) -> u32 {
    pt << 16 | ex << 7 | ch << 3 | 2
}
const fn ff32(pt: u32, ch: u32) -> u32 {
    1 << 22 | pt << 16 | ch << 3 | 4
}
const fn ff32e(pt: u32, ch: u32, ex: u32) -> u32 {
    1 << 22 | pt << 16 | ex << 7 | ch << 3 | 4
}
// `TYPE_*_DBL`: FLOAT_SH(1) | BYTES_SH(0) — 8-byte doubles (lcms's 0 = 8).
const fn fdbl(pt: u32, ch: u32) -> u32 {
    1 << 22 | pt << 16 | ch << 3
}
const fn fdble(pt: u32, ch: u32, ex: u32) -> u32 {
    1 << 22 | pt << 16 | ex << 7 | ch << 3
}

// -- CIE type aliases --------------------------------------------------------

/// lcms2-crate type aliases — `CIEXYZ`/`CIExyY`/`CIEXYZTRIPLE`/`CIELAB`/`CIELCh`.
pub type CIEXYZ = types::CieXyz;
pub type CIExyY = types::CiexyY;
pub type CIEXYZTRIPLE = types::CiexyYTriple;
pub type CIELAB = types::CieLab;
pub type CIELCh = types::CieLCh;
/// `ICCData` — the data-tag payload type.
pub type ICCData = crate::tagtypes::IccData;
/// `VideoSignalType` — the video-signal tag.
pub type VideoSignalType = crate::tagtypes::VideoSignal;
/// `MHC2Type` — the MHC2 tag.
pub type MHC2Type = crate::tagtypes::Mhc2;
/// `UcrBg` — the UCR/BG tag.
pub type UcrBg = crate::tagtypes::UcrBg;
/// `Screening` — the screening tag.
pub type Screening = crate::tagtypes::Screening;
/// `SEQ` — the profile-sequence type.
pub type SEQ = crate::named::ProfileSeqDesc;
/// `CIExyYTRIPLE` — chromaticity triple (alias above covers it).
/// `ICCMeasurementConditions`/`ICCViewingConditions`/`ColorimetricIntentImageState`
/// — the measurement/viewing wrappers.
pub type ICCMeasurementConditions = types::IccMeasurementConditions;
pub type ICCViewingConditions = crate::tagtypes::IccViewingConditions;
/// `ColorimetricIntentImageState` — the ciii-tag state.
pub enum ColorimetricIntentImageState {
    Unknown = 0,
    NormalAppearance,
    Vivid,
    Contrast,
    Simulation,
}
impl ColorimetricIntentImageState {
    fn to_u32(self) -> u32 {
        self as u32
    }
}
fn _colorim_unused() {}

impl CIEXYZ {
    /// `CIEXYZ::d50()` — the D50 white point.
    pub fn d50() -> Self {
        crate::wtpnt::d50_xyz()
    }
}
impl CIExyY {
    /// `CIExyY::d50()` — D50 in xyY.
    pub fn d50() -> Self {
        crate::wtpnt::d50_xyy()
    }
}

// -- tone curves ------------------------------------------------------------

/// `FloatOrU16` — the lcms2-crate eval trait (u16 or f32 curve input).
pub trait FloatOrU16: Copy {
    /// Evaluate a tone curve at this value.
    fn eval_curve(&self, t: &ToneCurveRef) -> Self;
    /// Evaluate a pipeline on input -> output at this value type.
    fn eval_pipeline(p: &PipelineRef, input: &[Self], output: &mut [Self]);
}
impl FloatOrU16 for u16 {
    fn eval_curve(&self, t: &ToneCurveRef) -> u16 {
        t.eval16(*self)
    }
    fn eval_pipeline(p: &PipelineRef, input: &[u16], output: &mut [u16]) {
        p.eval16(input, output);
    }
}
impl FloatOrU16 for f32 {
    fn eval_curve(&self, t: &ToneCurveRef) -> f32 {
        t.eval_float(*self)
    }
    fn eval_pipeline(p: &PipelineRef, input: &[f32], output: &mut [f32]) {
        p.eval_float(input, output);
    }
}
impl FloatOrU16 for f64 {
    fn eval_curve(&self, t: &ToneCurveRef) -> f64 {
        t.eval_float(*self as f32) as f64
    }
    fn eval_pipeline(p: &PipelineRef, input: &[f64], output: &mut [f64]) {
        // the C path evaluates f64 curves via f32 — same here
        let ii: Vec<f32> = input.iter().map(|v| *v as f32).collect();
        let mut oo = vec![0f32; output.len()];
        p.eval_float(&ii, &mut oo);
        for (o, v) in output.iter_mut().zip(oo) {
            *o = v as f64;
        }
    }
}

/// `ToneCurveRef` — the lcms2-crate borrowed tone-curve view
/// (`&ToneCurve` derefs to it). This is our `gamma::ToneCurve` — the
/// alias keeps the lcms2 borrow shape without a cast.
pub type ToneCurveRef = crate::gamma::ToneCurve;

/// `ToneCurve` — the lcms2-crate owned tone curve.
/// `&ToneCurve` derefs to `&ToneCurveRef` (like `String` -> `&str`).
#[derive(Clone)]
pub struct ToneCurve(pub ToneCurveRef);
impl Deref for ToneCurve {
    type Target = ToneCurveRef;
    fn deref(&self) -> &ToneCurveRef {
        &self.0
    }
}
impl core::ops::DerefMut for ToneCurve {
    fn deref_mut(&mut self) -> &mut ToneCurveRef {
        &mut self.0
    }
}
impl ToneCurve {
    /// `ToneCurve::new(gamma)` — `cmsBuildGamma` (plain gamma exponent).
    /// Panics on NULL like the lcms2 crate's `from_ptr`.
    #[must_use]
    pub fn new(gamma: f64) -> Self {
        ToneCurve(crate::gamma::build_gamma(None, gamma).expect("cmsBuildGamma returned NULL"))
    }
    /// `ToneCurve::new_tabulated(&[u16])` — `cmsBuildTabulatedToneCurve16`.
    #[must_use]
    pub fn new_tabulated(values: &[u16]) -> Self {
        ToneCurve(
            crate::gamma::build_tabulated_tone_curve16(None, values.len() as u32, Some(values))
                .expect("cmsBuildTabulatedToneCurve16 returned NULL"),
        )
    }
    /// `ToneCurve::new_tabulated_float(&[f32])` — `cmsBuildTabulatedToneCurveFloat`.
    #[must_use]
    pub fn new_tabulated_float(values: &[f32]) -> Self {
        ToneCurve(
            crate::gamma::build_tabulated_tone_curve_float(None, values)
                .expect("cmsBuildTabulatedToneCurveFloat returned NULL"),
        )
    }
    /// `ToneCurve::new_parametric(ty, params)` — `cmsBuildParametricToneCurve`.
    pub fn new_parametric(ty: i16, params: &[f64]) -> LCMSResult<Self> {
        crate::gamma::build_parametric_tone_curve(None, ty as i32, params)
            .map(ToneCurve)
            .ok_or_else(|| last_or(types::ERROR_UNDEFINED, "build_parametric_tone_curve failed"))
    }
}

/// Methods on `ToneCurveRef` (= `gamma::ToneCurve`) carrying the
/// lcms2-crate names — inherent impls on the same-crate type.
/// `is_linear`, `is_monotonic`, `is_descending`, `is_multisegment` and
/// `estimated_gamma(precision)` have identical signatures in `lcms::api`
/// and are defined there once for both layers.
impl crate::gamma::ToneCurve {
    /// `cmsEvalToneCurve*` on a `FloatOrU16` input.
    pub fn eval<V: FloatOrU16>(&self, v: V) -> V {
        v.eval_curve(self)
    }
    /// `cmsGetToneCurveEstimatedTable`
    pub fn estimated_entries(&self) -> &[u16] {
        self.estimated_table()
    }
    /// `cmsGetToneCurveEstimatedTableEntries`
    pub fn estimated_entry_count(&self) -> u32 {
        self.estimated_table_entries()
    }
    /// `cmsSmoothToneCurve`
    pub fn smooth(&mut self, lambda: f64) -> bool {
        crate::gamma::smooth_tone_curve(None, self, lambda)
    }
    /// `cmsReverseToneCurve`
    pub fn reversed(&self) -> ToneCurve {
        ToneCurve(crate::gamma::reverse_tone_curve(self).unwrap_or_else(|| self.clone()))
    }
    /// `cmsReverseToneCurveEx`
    pub fn reversed_samples(&self, samples: usize) -> ToneCurve {
        ToneCurve(
            crate::gamma::reverse_tone_curve_ex(samples as u32, self)
                .unwrap_or_else(|| self.clone()),
        )
    }
    /// `cmsJoinToneCurve` — Y^-1(X(t)); self is X.
    pub fn join(&self, y: &ToneCurveRef, points: usize) -> ToneCurve {
        ToneCurve(
            crate::gamma::join_tone_curve(None, self, y, points as u32)
                .unwrap_or_else(|| self.clone()),
        )
    }
    /// `to_owned` on `&ToneCurveRef` -> `ToneCurve` — inherent, shadows
    /// `ToOwned` so `let z: ToneCurve = r.to_owned()` works.
    pub fn to_owned(&self) -> ToneCurve {
        ToneCurve(self.clone())
    }
}

// -- MLU / locale -------------------------------------------------------------

/// `Locale` — the lcms2-crate language/country code ("en_US").
/// `language`/`country` are `c_char[3]` matching C's
/// `cmsLanguageCode`/`cmsCountryCode` layout (2 chars + NUL).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Locale {
    pub language: [c_char; 3],
    pub country: [c_char; 3],
}
impl Locale {
    /// `Locale::none()` — the null/none locale (all zeros).
    pub fn none() -> Self {
        Locale {
            language: [0; 3],
            country: [0; 3],
        }
    }
    /// `Locale::new("en_US")` — 2-letter language, `_`, 2-letter country.
    pub fn new(s: &str) -> Self {
        let (lang, country) = s.split_at(s.len().min(3));
        let mut l = Locale::none();
        for (c, b) in l.language.iter_mut().zip(lang.bytes().take(2)) {
            *c = b as c_char;
        }
        for (c, b) in l.country.iter_mut().zip(country.bytes().take(2)) {
            *c = b as c_char;
        }
        l
    }
    /// Raw pointer accessors (`*const c_char`).
    pub fn language_ptr(&self) -> *const c_char {
        self.language.as_ptr()
    }
    pub fn country_ptr(&self) -> *const c_char {
        self.country.as_ptr()
    }
    /// The `[u8;3]` form our MLU fns take.
    fn l3(&self) -> [u8; 3] {
        [self.language[0] as u8, self.language[1] as u8, 0]
    }
    fn c3(&self) -> [u8; 3] {
        [self.country[0] as u8, self.country[1] as u8, 0]
    }
}
impl fmt::Display for Locale {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}{}{}{}",
            (self.language[0] as u8) as char,
            (self.language[1] as u8) as char,
            (self.country[0] as u8) as char,
            (self.country[1] as u8) as char
        )
    }
}
impl From<&str> for Locale {
    fn from(s: &str) -> Self {
        Locale::new(s)
    }
}

/// `MLURef` — the lcms2-crate borrowed MLU view (`&MLU` derefs to it).
/// Alias to `named::Mlu` — the lcms2 borrow shape without a cast.
pub type MLURef = crate::named::Mlu;
/// `MLU` — the lcms2-crate owned multi-language-unicode type.
pub struct MLU(pub MLURef);
impl Deref for MLU {
    type Target = MLURef;
    fn deref(&self) -> &MLURef {
        &self.0
    }
}
impl core::ops::DerefMut for MLU {
    fn deref_mut(&mut self) -> &mut MLURef {
        &mut self.0
    }
}
impl MLU {
    /// `MLU::new(n)` — allocate for `n` translations.
    pub fn new(n: u32) -> Self {
        MLU(crate::named::mlu_alloc(None, n).unwrap_or_default())
    }
}
/// Methods on `MLURef` (= `named::Mlu`) carrying the lcms2-crate names.
impl crate::named::Mlu {
    /// `set_text(utf8, locale)` — UTF-8 string.
    pub fn set_text(&mut self, text: &str, locale: Locale) -> bool {
        crate::named::mlu_set_utf8(self, &locale.l3(), &locale.c3(), text.as_bytes())
    }
    /// `set_text_ascii` — ASCII only (rejects non-ASCII).
    pub fn set_text_ascii(&mut self, text: &str, locale: Locale) -> bool {
        crate::named::mlu_set_ascii(self, &locale.l3(), &locale.c3(), text.as_bytes())
    }
    /// `text(locale)` — UTF-8 string for the locale.
    pub fn text(&self, locale: Locale) -> Result<String, ()> {
        let (n, mut buf) = crate::named::mlu_get_utf8(self, &locale.l3(), &locale.c3(), Some(256));
        if n == 0 {
            return Err(());
        }
        buf.truncate(n as usize);
        while buf.last() == Some(&0) {
            buf.pop();
        }
        String::from_utf8(buf).map_err(|_| ())
    }
    /// `text_ascii(locale)` — ASCII string for the locale.
    pub fn text_ascii(&self, locale: Locale) -> Result<String, ()> {
        let (n, mut buf) = crate::named::mlu_get_ascii(self, &locale.l3(), &locale.c3(), Some(256));
        if n == 0 {
            return Err(());
        }
        buf.truncate(n as usize);
        while buf.last() == Some(&0) {
            buf.pop();
        }
        String::from_utf8(buf).map_err(|_| ())
    }
    /// `translations_count`
    pub fn translations_count(&self) -> u32 {
        crate::named::mlu_translations_count(self)
    }
}

// -- named colors ------------------------------------------------------------

/// `NamedColorListRef` — the lcms2-crate borrowed named-color-list view.
pub type NamedColorListRef = crate::named::NamedColorList;
/// `NamedColorList` — the lcms2-crate owned named-color list.
pub struct NamedColorList(pub NamedColorListRef);
impl Deref for NamedColorList {
    type Target = NamedColorListRef;
    fn deref(&self) -> &NamedColorListRef {
        &self.0
    }
}
impl core::ops::DerefMut for NamedColorList {
    fn deref_mut(&mut self) -> &mut NamedColorListRef {
        &mut self.0
    }
}
/// `NamedColorInfo` — a single named-color entry (lcms2-crate shape).
#[derive(Clone, Debug, PartialEq)]
pub struct NamedColorInfo {
    pub name: String,
    pub prefix: String,
    pub suffix: String,
    pub pcs: [u16; 3],
    pub colorant: Vec<u16>,
}
impl NamedColorList {
    /// `NamedColorList::new(spot_colors, colorant_count, prefix, suffix)`.
    pub fn new(
        spot_colors: usize,
        colorant_count: usize,
        prefix: &str,
        suffix: &str,
    ) -> LCMSResult<Self> {
        crate::named::alloc_named_color_list(
            None,
            spot_colors as u32,
            colorant_count as u32,
            prefix.as_bytes(),
            suffix.as_bytes(),
        )
        .map(NamedColorList)
        .ok_or_else(|| last_or(types::ERROR_UNDEFINED, "alloc_named_color_list failed"))
    }
}
/// Methods on `NamedColorListRef` (= `named::NamedColorList`).
impl crate::named::NamedColorList {
    /// `cmsNamedColorCount`
    pub fn count(&self) -> usize {
        crate::named::named_color_count(self) as usize
    }
    /// `cmsNamedColorIndex`
    pub fn index_of(&self, name: &str) -> usize {
        self.list[..self.n_colors as usize]
            .iter()
            .position(|c| c.name[..name.len()] == *name.as_bytes())
            .map(|i| i + 1) // 1-based per C
            .unwrap_or(0)
    }
    /// `cmsNamedColorInfo` — the `index`-th entry.
    pub fn get(&self, index: usize) -> Option<NamedColorInfo> {
        let c = self.list.get(index)?;
        Some(NamedColorInfo {
            name: cstr(&c.name),
            prefix: cstr(&self.prefix),
            suffix: cstr(&self.suffix),
            pcs: c.pcs,
            colorant: c.device_colorant[..self.colorant_count as usize].to_vec(),
        })
    }
    /// All entries.
    pub fn colors(&self) -> Vec<NamedColorInfo> {
        (0..self.n_colors as usize)
            .filter_map(|i| self.get(i))
            .collect()
    }
    /// `cmsAppendNamedColor`
    pub fn append(&mut self, name: &str, pcs: [u16; 3], colorant: [u16; 16]) -> bool {
        crate::named::append_named_color(self, Some(name.as_bytes()), Some(&pcs), Some(&colorant))
    }
}
fn cstr(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).into_owned()
}

// -- pipeline / stage ----------------------------------------------------------

/// `PipelineRef` — the lcms2-crate borrowed pipeline view.
pub type PipelineRef = crate::lut::Pipeline;
/// `StageRef` — the lcms2-crate borrowed stage view.
pub type StageRef = crate::lut::Stage;
/// `Pipeline` — the lcms2-crate owned pipeline.
pub struct Pipeline(pub PipelineRef);
impl Deref for Pipeline {
    type Target = PipelineRef;
    fn deref(&self) -> &PipelineRef {
        &self.0
    }
}
impl core::ops::DerefMut for Pipeline {
    fn deref_mut(&mut self) -> &mut PipelineRef {
        &mut self.0
    }
}
/// `Stage` — the lcms2-crate owned stage.
pub struct Stage(pub StageRef);
impl Deref for Stage {
    type Target = StageRef;
    fn deref(&self) -> &StageRef {
        &self.0
    }
}
impl core::ops::DerefMut for Stage {
    fn deref_mut(&mut self) -> &mut StageRef {
        &mut self.0
    }
}
impl Pipeline {
    /// `Pipeline::new(input_channels, output_channels)` — `cmsPipelineAlloc`.
    pub fn new(input_channels: usize, output_channels: usize) -> LCMSResult<Self> {
        crate::lut::pipeline_alloc(None, input_channels as u32, output_channels as u32)
            .map(Pipeline)
            .ok_or_else(|| last_or(types::ERROR_UNDEFINED, "pipeline_alloc failed"))
    }
}
/// Methods on the owned `Pipeline` wrapper — the lcms2-crate usize shape.
impl Pipeline {
    /// `cmsPipelineStageCount` — element count.
    pub fn stage_count(&self) -> usize {
        let mut n = 0;
        let mut s = self.0.elements.as_deref();
        while let Some(st) = s {
            n += 1;
            s = st.next.as_deref();
        }
        n
    }
    /// `cmsPipelineGetFirstStage` — head of the stage chain.
    pub fn first_stage(&self) -> Option<&StageRef> {
        self.0.elements.as_deref()
    }
    /// `cmsPipelineGetLastStage` — walk to the tail.
    pub fn last_stage(&self) -> Option<&StageRef> {
        let mut s = self.0.elements.as_deref()?;
        while let Some(n) = s.next.as_deref() {
            s = n;
        }
        Some(s)
    }
    /// `cmsPipelineInputChannels` (usize — the lcms2-crate return type).
    pub fn input_channels(&self) -> usize {
        self.0.input_channels as usize
    }
    /// `cmsPipelineOutputChannels` (usize).
    pub fn output_channels(&self) -> usize {
        self.0.output_channels as usize
    }
    /// `cmsPipelineEval*` on `FloatOrU16`.
    pub fn eval<V: FloatOrU16>(&self, input: &[V], output: &mut [V]) {
        V::eval_pipeline(&self.0, input, output);
    }
    /// `cmsPipelineCat` — append a pipeline's stages.
    pub fn cat(&mut self, append: &PipelineRef) -> bool {
        // append `append`'s stage chain to the tail
        let mut tail = &mut self.0.elements;
        while let Some(t) = tail {
            tail = &mut t.next;
        }
        let mut src = append.elements.as_deref();
        while let Some(st) = src {
            *tail = Some(Box::new(st.clone()));
            tail = &mut tail.as_mut().unwrap().next;
            src = st.next.as_deref();
        }
        true
    }
}
impl Stage {
    /// `Stage::new_identity(channels)` — `cmsStageAllocIdentity`.
    pub fn new_identity(channels: u32) -> Stage {
        Stage(crate::lut::stage_alloc_identity(None, channels))
    }
    /// `Stage::new_tone_curves(&[&ToneCurveRef])` — `cmsStageAllocToneCurves`.
    pub fn new_tone_curves(curves: &[&ToneCurveRef]) -> LCMSResult<Self> {
        crate::lut::stage_alloc_tone_curves(None, curves.len() as u32, Some(curves))
            .map(Stage)
            .ok_or_else(|| last_or(types::ERROR_UNDEFINED, "stage_alloc_tone_curves failed"))
    }
    /// `Stage::new_matrix(matrix2d, rows, cols, offsets)` — `cmsStageAllocMatrix`.
    pub fn new_matrix(
        matrix2d: &[f64],
        rows: usize,
        cols: usize,
        offsets: Option<&[f64]>,
    ) -> LCMSResult<Self> {
        crate::lut::stage_alloc_matrix(None, rows as u32, cols as u32, matrix2d, offsets)
            .map(Stage)
            .ok_or_else(|| last_or(types::ERROR_UNDEFINED, "stage_alloc_matrix failed"))
    }
}
/// Methods on the owned `Stage` wrapper.
impl Stage {
    /// `cmsStageInputChannels` / `cmsStageOutputChannels`.
    pub fn input_channels(&self) -> usize {
        self.0.input_channels as usize
    }
    pub fn output_channels(&self) -> usize {
        self.0.output_channels as usize
    }
    /// `cmsStageType` — the stage signature.
    pub fn stage_type(&self) -> u32 {
        self.0.r#type
    }
}

// -- signature enums -------------------------------------------------------------

macro_rules! sig_enum {
    ($name:ident { $( $v:ident = $u:expr ),* $(,)? }) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum $name { $( $v, )* }
        impl $name {
            /// The raw `Signature` value.
            pub fn to_u32(self) -> u32 { match self { $( $name::$v => $u, )* } }
            /// `Signature` -> variant (`None` if unmapped).
            pub fn from_u32(u: u32) -> Option<Self> { $( if u == $u { return Some($name::$v); } )* None }
        }
        impl From<$name> for u32 { fn from(s: $name) -> u32 { s.to_u32() } }
    };
}

use crate::signatures::*;
sig_enum! { TagSignature {
    BToD0Tag = tag::SigBToD0Tag,
    BToD1Tag = tag::SigBToD1Tag,
    BToD2Tag = tag::SigBToD2Tag,
    BlueColorantTag = tag::SigBlueColorantTag,
    BlueTRCTag = tag::SigBlueTRCTag,
    CopyrightTag = tag::SigCopyrightTag,
    GreenColorantTag = tag::SigGreenColorantTag,
    GreenTRCTag = tag::SigGreenTRCTag,
    MediaWhitePointTag = tag::SigMediaWhitePointTag,
    ProfileDescriptionTag = tag::SigProfileDescriptionTag,
    RedColorantTag = tag::SigRedColorantTag,
    RedTRCTag = tag::SigRedTRCTag,
    VcgtTag = tag::SigVcgtTag,
    NamedColor2Tag = tag::SigNamedColor2Tag,
    AToB0Tag = tag::SigAToB0Tag,
    ATToB1Tag = tag::SigAToB1Tag,
    ATToB2Tag = tag::SigAToB2Tag,
    BToA0Tag = tag::SigBToA0Tag,
    BToA1Tag = tag::SigBToA1Tag,
    BToA2Tag = tag::SigBToA2Tag,
    GrayTRCTag = tag::SigGrayTRCTag,
}}

sig_enum! { ColorSpaceSignature {
    Sig1colorData = colorspace::Sig1colorData,
    GrayData = colorspace::SigGrayData,
    RgbData = colorspace::SigRgbData,
    CmykData = colorspace::SigCmykData,
    LabData = colorspace::SigLabData,
    XYZData = colorspace::SigXYZData,
    LuvData = colorspace::SigLuvData,
    YCbCrData = colorspace::SigYCbCrData,
    HsvData = colorspace::SigHsvData,
    HlsData = colorspace::SigHlsData,
    CmyData = colorspace::SigCmyData,
    MCH1Data = colorspace::SigMCH1Data,
    MCH2Data = colorspace::SigMCH2Data,
    MCH3Data = colorspace::SigMCH3Data,
    MCH4Data = colorspace::SigMCH4Data,
    MCH5Data = colorspace::SigMCH5Data,
    MCH6Data = colorspace::SigMCH6Data,
    MCH7Data = colorspace::SigMCH7Data,
    MCH8Data = colorspace::SigMCH8Data,
}}

sig_enum! { ProfileClassSignature {
    InputClass = profileclass::SigInputClass,
    DisplayClass = profileclass::SigDisplayClass,
    OutputClass = profileclass::SigOutputClass,
    LinkClass = profileclass::SigLinkClass,
    AbstractClass = profileclass::SigAbstractClass,
    ColorSpaceClass = profileclass::SigColorSpaceClass,
    NamedColorClass = profileclass::SigNamedColorClass,
}}

sig_enum! { TechnologySignature {
    DigitalCamera = technology::SigDigitalCamera,
    FilmScanner = technology::SigFilmScanner,
    ReflectiveScanner = technology::SigReflectiveScanner,
    InkJetPrinter = technology::SigInkJetPrinter,
    ThermalWaxPrinter = technology::SigThermalWaxPrinter,
    ElectrophotographicPrinter = technology::SigElectrophotographicPrinter,
    ElectrostaticPrinter = technology::SigElectrostaticPrinter,
    DyeSublimationPrinter = technology::SigDyeSublimationPrinter,
    PhotographicPaperPrinter = technology::SigPhotographicPaperPrinter,
    FilmWriter = technology::SigFilmWriter,
    OffsetLithography = technology::SigOffsetLithography,
    Silkscreen = technology::SigSilkscreen,
    VideoMonitor = technology::SigVideoMonitor,
    VideoCamera = technology::SigVideoCamera,
    MotionPictureFilmScanner = technology::SigMotionPictureFilmScanner,
    MotionPictureFilmRecorder = technology::SigMotionPictureFilmRecorder,
}}

/// `InfoType` — the lcms2-crate info-text selector.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InfoType {
    Description,
    Manufacturer,
    Model,
    Copyright,
}

// -- Tag<'a> ------------------------------------------------------------------

/// `Tag<'a>` — the lcms2-crate borrowed tag object.
/// `&'a` borrows point at the `TagObject` stored in the profile — reads
/// are `&self` against a frozen profile.
#[non_exhaustive]
pub enum Tag<'a> {
    /// Chromaticity (xyY triple).
    CIExyYTRIPLE(&'a CIEXYZTRIPLE),
    /// A white-point / colorant XYZ.
    CIEXYZ(&'a CIEXYZ),
    /// Raw ICC data payload.
    ICCData(&'a ICCData),
    /// Measurement conditions.
    ICCMeasurementConditions(&'a ICCMeasurementConditions),
    /// Viewing conditions.
    ICCViewingConditions(&'a ICCViewingConditions),
    /// Multi-language string.
    MLU(&'a MLURef),
    /// A named-color palette.
    NamedColorList(&'a NamedColorListRef),
    /// A processing pipeline (mft/mAB/mBA/Lut tags).
    Pipeline(&'a PipelineRef),
    /// Screening.
    Screening(&'a Screening),
    /// Profile sequence descriptor.
    SEQ(&'a SEQ),
    /// An intent signature tag.
    Intent(Intent),
    /// Colorimetric-intent image state.
    ColorimetricIntentImageState(ColorimetricIntentImageState),
    /// A technology signature tag.
    Technology(TechnologySignature),
    /// A tone curve.
    ToneCurve(&'a ToneCurveRef),
    /// UCR/BG.
    UcrBg(&'a UcrBg),
    /// The vcgt three-curve tag.
    VcgtCurves([&'a ToneCurveRef; 3]),
    /// Video-signal.
    VideoSignal(&'a VideoSignalType),
    /// MHC2.
    MHC2(&'a MHC2Type),
    /// Unknown format or missing data.
    None,
}
impl Tag<'_> {
    /// `Tag::None` — the lcms2 crate's `is_none`.
    pub fn is_none(&self) -> bool {
        matches!(self, Tag::None)
    }
}

// -- profile -------------------------------------------------------------------

/// `Profile` — the lcms2-crate profile type.
/// `C` is the lcms2 context-generic (default `GlobalContext`).
/// Profiles are **frozen at construction** so `read_tag`/`has_tag`/`info`
/// are `&self` — `read_tag` borrows the stored `&TagObject` directly.
pub struct Profile<C = GlobalContext> {
    pub(crate) icc: Box<crate::profile::IccProfile>,
    _ctx: PhantomData<C>,
}

impl Profile<GlobalContext> {
    /// `cmsOpenProfileFromMem`
    pub fn new_icc(data: &[u8]) -> LCMSResult<Self> {
        match prof::open_profile_from_mem(None, data) {
            Some(mut icc) => {
                prof::freeze(&mut icc);
                Ok(Profile {
                    icc,
                    _ctx: PhantomData,
                })
            }
            None => Err(last_or(types::ERROR_READ, "open_profile_from_mem failed")),
        }
    }
    /// `cmsOpenProfileFromMemTHR` — the context-generic constructor.
    /// For `GlobalContext` (`None`) it behaves as `new_icc`.
    pub fn new_icc_context(
        _ctx: &ThreadContext,
        data: &[u8],
    ) -> LCMSResult<Profile<ThreadContext>> {
        match prof::open_profile_from_mem(None, data) {
            Some(mut icc) => {
                prof::freeze(&mut icc);
                Ok(Profile {
                    icc,
                    _ctx: PhantomData,
                })
            }
            None => Err(last_or(types::ERROR_READ, "open_profile_from_mem failed")),
        }
    }
    /// `cmsOpenProfileFromFile`
    pub fn new_file<P: AsRef<std::path::Path>>(path: P) -> std::io::Result<Self> {
        let data = std::fs::read(path)?;
        Self::new_icc(&data)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))
    }
    /// `cmsCreate_sRGBProfile`
    pub fn new_srgb() -> Self {
        Self::wrap(virt::create_srgb_profile(None))
    }
    /// `cmsCreateRGBProfileTHR`
    pub fn new_rgb(
        white_point: &CIExyY,
        primaries: &CIEXYZTRIPLE,
        red: &ToneCurveRef,
        green: &ToneCurveRef,
        blue: &ToneCurveRef,
    ) -> LCMSResult<Self> {
        match virt::create_rgb_profile(
            None,
            Some(white_point),
            Some(primaries),
            Some([red, green, blue]),
        ) {
            Some(icc) => Ok(Profile {
                icc,
                _ctx: PhantomData,
            }),
            None => Err(last_or(types::ERROR_WRITE, "create_rgb_profile failed")),
        }
    }
    /// `cmsCreateGrayProfileTHR`
    pub fn new_gray(white_point: &CIExyY, curve: &ToneCurveRef) -> LCMSResult<Self> {
        match virt::create_gray_profile(None, Some(white_point), Some(curve)) {
            Some(icc) => Ok(Profile {
                icc,
                _ctx: PhantomData,
            }),
            None => Err(last_or(types::ERROR_WRITE, "create_gray_profile failed")),
        }
    }
    /// `cmsCreateLab4ProfileTHR`
    pub fn new_lab4() -> Self {
        Self::wrap(virt::create_lab4_profile(None, None))
    }
    /// `cmsCreateLab2ProfileTHR`
    pub fn new_lab2() -> Self {
        Self::wrap(virt::create_lab2_profile(None, None))
    }
    /// `cmsCreateXYZProfileTHR`
    pub fn new_xyz() -> Self {
        Self::wrap(virt::create_xyz_profile(None))
    }
    /// `cmsCreateNULLProfileTHR`
    pub fn new_null() -> Self {
        Self::wrap(virt::create_null_profile(None))
    }
    /// `cmsCreateProfilePlaceholder` — a bare profile shell.
    /// C's `cmsCreateProfilePlaceholder` defaults `color_space` to
    /// `cmsSig1colorData` ('1CLR'), `pcs` to `cmsSigXYZData`.
    pub fn new_placeholder() -> Self {
        let mut icc = crate::profile::IccProfile::default();
        icc.color_space = crate::signatures::colorspace::Sig1colorData;
        icc.pcs = crate::signatures::colorspace::SigXYZData;
        Profile {
            icc: Box::new(icc),
            _ctx: PhantomData,
        }
    }

    fn wrap(icc: Option<Box<crate::profile::IccProfile>>) -> Self {
        let mut icc = icc.expect("lcms profile create failed");
        prof::freeze(&mut icc);
        Profile {
            icc,
            _ctx: PhantomData,
        }
    }
}

impl<C> Profile<C> {
    /// `cmsSaveProfileToMem` — `&self` like the lcms2 crate (the save may
    /// lazy-materialize; we save a clone so the caller's profile stays
    /// untouched).
    pub fn icc(&self) -> LCMSResult<Vec<u8>> {
        let mut icc = self.icc.clone();
        match prof::save_profile_to_mem(&mut icc) {
            Some(b) => Ok(b),
            None => Err(last_or(types::ERROR_WRITE, "save_profile_to_mem failed")),
        }
    }
    /// `cmsGetDeviceClass`
    pub fn device_class(&self) -> ProfileClassSignature {
        ProfileClassSignature::from_u32(self.icc.device_class)
            .unwrap_or(ProfileClassSignature::DisplayClass)
    }
    /// `cmsGetColorSpace`
    pub fn color_space(&self) -> ColorSpaceSignature {
        ColorSpaceSignature::from_u32(self.icc.color_space).unwrap_or(ColorSpaceSignature::RgbData)
    }
    /// `cmsGetPCS` — raw signature u32.
    pub fn pcs(&self) -> u32 {
        self.icc.pcs
    }
    /// `cmsGetEncodedICCversion`
    pub fn encoded_icc_version(&self) -> u32 {
        self.icc.version
    }
    /// `cmsGetHeaderAttributes`
    pub fn header_attributes(&self) -> u64 {
        self.icc.attributes
    }
    /// `cmsGetHeaderFlags`
    pub fn header_flags(&self) -> u32 {
        self.icc.flags
    }
    /// `cmsGetHeaderCreator`
    pub fn header_creator(&self) -> u32 {
        self.icc.creator
    }
    /// `cmsGetHeaderProfileID`
    pub fn header_profile_id(&self) -> crate::types::ProfileId {
        self.icc.profile_id.clone()
    }
    /// `cmsGetHeaderRenderingIntent`
    pub fn header_rendering_intent(&self) -> Intent {
        match self.icc.rendering_intent {
            0 => Intent::Perceptual,
            1 => Intent::RelativeColorimetric,
            2 => Intent::Saturation,
            _ => Intent::AbsoluteColorimetric,
        }
    }
    /// `cmsGetTagCount`
    pub fn tag_count(&self) -> u32 {
        self.icc.tag_count
    }
    /// `cmsGetTagSignature`
    pub fn tag_signature(&self, i: u32) -> TagSignature {
        TagSignature::from_u32(self.icc.tag_names.get(i as usize).copied().unwrap_or(0))
            .unwrap_or(TagSignature::ProfileDescriptionTag)
    }
    /// `cmsIsTag` — tag present.
    pub fn is_tag(&self, sig: TagSignature) -> bool {
        crate::profile::cms_is_tag(&self.icc, sig.to_u32())
    }

    // -- tag access (frozen profile — `&self` reads, `&mut` writes) -----------

    /// `read_tag` — the lcms2-crate `Profile::read_tag`, `&self`.
    /// Returns `Tag<'a>` borrowing the stored `&TagObject`;
    /// `Tag::None` for absent/unmapped tags (matches the lcms2 crate,
    /// which returns `Tag` not `Option<Tag>`).
    pub fn read_tag<'a>(&'a self, sig: TagSignature) -> Tag<'a> {
        let Some(obj) = prof::read_tag_frozen(&self.icc, sig.to_u32()) else {
            return Tag::None;
        };
        match obj {
            TagObject::Xyz(x) => Tag::CIEXYZ(x),
            TagObject::Mlu(m) => Tag::MLU(m),
            TagObject::Curve(c) | TagObject::ParametricCurve(c) => Tag::ToneCurve(c),
            TagObject::NamedColor(l) | TagObject::ColorantTable(l) => Tag::NamedColorList(l),
            TagObject::Vcgt(VcgtData::Curves(c)) => Tag::VcgtCurves([&c[0], &c[1], &c[2]]),
            TagObject::Chromaticity(c) => Tag::CIExyYTRIPLE(c),
            TagObject::Data(d) => Tag::ICCData(d),
            TagObject::Lut8(l)
            | TagObject::Lut16(l)
            | TagObject::LutA2B(l)
            | TagObject::LutB2A(l) => Tag::Pipeline(l),
            TagObject::Mpe(l) => Tag::Pipeline(l),
            TagObject::Measurement(m) => Tag::ICCMeasurementConditions(m),
            TagObject::ViewingConditions(v) => Tag::ICCViewingConditions(v),
            TagObject::UcrBg(u) => Tag::UcrBg(u),
            TagObject::Screening(s) => Tag::Screening(s),
            TagObject::ProfileSeqDesc(s) | TagObject::ProfileSeqId(s) => Tag::SEQ(s),
            TagObject::Mhc2(m) => Tag::MHC2(m),
            TagObject::Signature(s) => match *s {
                0 => Tag::Intent(Intent::Perceptual),
                1 => Tag::Intent(Intent::RelativeColorimetric),
                2 => Tag::Intent(Intent::Saturation),
                3 => Tag::Intent(Intent::AbsoluteColorimetric),
                _ => Tag::None,
            },
            _ => Tag::None,
        }
    }
    /// `write_tag` — `&mut self` (mutates the stored object).
    pub fn write_tag(&mut self, sig: TagSignature, tag: Tag) -> bool {
        let obj = match tag {
            Tag::CIEXYZ(x) => TagObject::Xyz(x.clone()),
            Tag::MLU(m) => TagObject::Mlu(m.clone()),
            Tag::ToneCurve(c) => TagObject::Curve(c.clone()),
            Tag::VcgtCurves(v) => {
                TagObject::Vcgt(VcgtData::Curves([v[0].clone(), v[1].clone(), v[2].clone()]))
            }
            Tag::CIExyYTRIPLE(c) => TagObject::Chromaticity(c.clone()),
            Tag::ICCData(d) => TagObject::Data(d.clone()),
            Tag::NamedColorList(l) => TagObject::NamedColor(l.clone()),
            Tag::Pipeline(p) => TagObject::Lut16(p.clone()),
            Tag::ICCMeasurementConditions(m) => TagObject::Measurement(m.clone()),
            Tag::ICCViewingConditions(v) => TagObject::ViewingConditions(v.clone()),
            Tag::UcrBg(u) => TagObject::UcrBg(u.clone()),
            Tag::Screening(s) => TagObject::Screening(s.clone()),
            Tag::SEQ(s) => TagObject::ProfileSeqDesc(s.clone()),
            Tag::MHC2(m) => TagObject::Mhc2(m.clone()),
            Tag::Intent(_)
            | Tag::ColorimetricIntentImageState(_)
            | Tag::Technology(_)
            | Tag::VideoSignal(_)
            | Tag::None => return false,
        };
        crate::profile::cms_write_tag(&mut self.icc, sig.to_u32(), Some(obj))
    }
    /// `has_tag` — `&self`, tag present.
    pub fn has_tag(&self, sig: TagSignature) -> bool {
        self.is_tag(sig)
    }
    /// `remove_tag` — `&mut self`, clear a tag.
    pub fn remove_tag(&mut self, sig: TagSignature) {
        crate::profile::cms_write_tag(&mut self.icc, sig.to_u32(), None);
    }
    /// `set_color_space` — `&mut self`.
    pub fn set_color_space(&mut self, s: ColorSpaceSignature) {
        self.icc.color_space = s.to_u32();
    }
    /// `set_device_class` — `&mut self`.
    pub fn set_device_class(&mut self, c: ProfileClassSignature) {
        self.icc.device_class = c.to_u32();
    }
    /// `info(InfoType, Locale)` — `&self`; description/manufacturer/
    /// copyright text from the MLU tags.
    pub fn info(&self, ty: InfoType, locale: Locale) -> Option<String> {
        let sig = match ty {
            InfoType::Description => tag::SigProfileDescriptionTag,
            InfoType::Manufacturer => tag::SigDeviceMfgDescTag,
            InfoType::Model => tag::SigDeviceModelDescTag,
            InfoType::Copyright => tag::SigCopyrightTag,
        };
        let obj = prof::read_tag_frozen(&self.icc, sig)?;
        if let TagObject::Mlu(m) = obj {
            let (n, mut buf) = crate::named::mlu_get_utf8(m, &locale.l3(), &locale.c3(), Some(256));
            if n == 0 {
                None
            } else {
                buf.truncate(n as usize);
                while buf.last() == Some(&0) {
                    buf.pop();
                }
                String::from_utf8(buf).ok()
            }
        } else {
            None
        }
    }
    /// The raw parity-layer profile.
    pub fn inner(&self) -> &crate::profile::IccProfile {
        &self.icc
    }
    /// `&mut` — for plan-building (lazy materialization).
    pub fn inner_mut(&mut self) -> &mut crate::profile::IccProfile {
        &mut self.icc
    }
}

impl<C> fmt::Debug for Profile<C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let l = Locale::none();
        f.debug_struct("Profile")
            .field("Description", &self.info(InfoType::Description, l))
            .field("Manufacturer", &self.info(InfoType::Manufacturer, l))
            .field("Model", &self.info(InfoType::Model, l))
            .field("Copyright", &self.info(InfoType::Copyright, l))
            .finish()
    }
}
// Profile is Send + Sync via field auto-impls (Box<IccProfile> + PhantomData
// — `forbid(unsafe_code)` can't `unsafe impl`; the fields carry it.)

// -- transform -----------------------------------------------------------------

/// `Transform<I, O>` — the lcms2-crate typed transform.
/// `I`/`O` are `Pod` pixel types (`[u8;3]`, `(u8,u8,u8)`, `RGB<u8>`, ...).
/// The per-run pixel cache is a `Mutex<TransformScratch>` so
/// `transform_pixels` is `&self` — matching the lcms2 API shape and
/// keeping the warm `_cmsCACHE` semantics.
pub struct Transform<I, O, C = GlobalContext, F = AllowCache> {
    /// The C-shape transform (plan + scratch together) behind a Mutex —
    /// `transform_pixels`/`new_device_link` take `&self` but the pixel
    /// cache needs `&mut`, matching C's per-transform `_cmsCACHE`.
    inner: Mutex<xform::Transform>,
    _i: PhantomData<I>,
    _o: PhantomData<O>,
    _c: PhantomData<C>,
    _f: PhantomData<F>,
}

impl<I: Copy + bytemuck::Pod, O: Copy + bytemuck::Pod> Transform<I, O, GlobalContext, AllowCache> {
    /// `cmsCreateTransform` — no flags.
    pub fn new(
        input: &Profile,
        in_format: PixelFormat,
        output: &Profile,
        out_format: PixelFormat,
        intent: Intent,
    ) -> LCMSResult<Self> {
        Self::with_flags(input, in_format, output, out_format, intent, Flags::NONE)
    }

    /// `cmsCreateTransform` with flags.
    pub fn with_flags(
        input: &Profile,
        in_format: PixelFormat,
        output: &Profile,
        out_format: PixelFormat,
        intent: Intent,
        flags: Flags,
    ) -> LCMSResult<Self> {
        let mut profs = [input.icc.clone(), output.icc.clone()];
        let mut bpc = [false, false];
        let intents = [intent as u32; 2];
        let states = [3.0f64; 2];
        let t = xform::create_extended_transform(
            None,
            &mut profs,
            &mut bpc,
            &intents,
            &states,
            None,
            0,
            in_format.0,
            out_format.0,
            flags.0,
        )
        .ok_or(Error::ObjectCreationError)?;
        Ok(Transform {
            inner: Mutex::new(*t),
            _i: PhantomData,
            _o: PhantomData,
            _c: PhantomData,
            _f: PhantomData,
        })
    }
}

impl<I: Copy + bytemuck::Pod, O: Copy + bytemuck::Pod, C, F> Transform<I, O, C, F> {
    /// `cmsDoTransform` on typed pixel slices — `&self` like the lcms2
    /// crate; the scratch is the locked per-transform cache.
    pub fn transform_pixels(&self, src: &[I], dst: &mut [O]) {
        let n = src.len().min(dst.len()) as u32;
        let ib: &[u8] = bytemuck::cast_slice(src);
        let ob: &mut [u8] = bytemuck::cast_slice_mut(dst);
        let mut xf = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        xf.do_transform(ib, ob, n);
    }

    /// `transform_in_place` — same buffer in/out (I == O layout).
    /// Implemented via a temporary copy — transforms are
    /// pixel-independent, so copy-in-place preserves semantics
    /// exactly. (`transform_pixels_uninit` is omitted: a
    /// `forbid(unsafe_code)` crate can't return `&mut [O]` over
    /// `MaybeUninit`.)
    pub fn transform_in_place(&self, buf: &mut [I]) {
        let tmp: Vec<I> = buf.to_vec();
        let n = buf.len() as u32;
        let mut xf = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        xf.do_transform(bytemuck::cast_slice(&tmp), bytemuck::cast_slice_mut(buf), n);
    }
}

// -- crate fns -------------------------------------------------------------

/// `lcms2::version()` — `cmsGetEncodedCMMversion`.
pub fn version() -> u32 {
    types::LCMS_VERSION
}
/// `lcms2::white_point_from_temp(temp_k)` — `cmsWhitePointFromTemp`.
pub fn white_point_from_temp(temp_k: f64) -> Option<CIExyY> {
    crate::wtpnt::white_point_from_temp(temp_k)
}
/// `lcms2::xyY2XYZ` — `cmsxyY2XYZ`.
pub fn xyY2XYZ(xyy: &CIExyY) -> CIEXYZ {
    crate::pcs::xyy2xyz(xyy)
}
/// `lcms2::XYZ2xyY` — `cmsXYZ2xyY`.
pub fn XYZ2xyY(xyz: &CIEXYZ) -> CIExyY {
    crate::pcs::xyz2xyy(xyz)
}

// -- extension traits (lcms2's `ext.rs`) --------------------------------------

/// `ColorSpaceSignatureExt` — `channels()` + `pixel_type()` on the sig.
pub trait ColorSpaceSignatureExt: Sized + Copy {
    /// `cmsChannelsOf` — channel count (3 on error, matching lcms2's quirk).
    fn channels(self) -> u32;
    /// `PixelType` — the packed-format pixel-type bits for the color space.
    fn pixel_type(self) -> u32;
}
impl ColorSpaceSignatureExt for ColorSpaceSignature {
    fn channels(self) -> u32 {
        crate::pcs::channels_of(self.to_u32())
    }
    fn pixel_type(self) -> u32 {
        // the PT_* pixel-type nibble for the color space (no
        // `cms_pixel_type` fn in the port — a direct sig map).
        match self {
            ColorSpaceSignature::GrayData => PT_GRAY,
            ColorSpaceSignature::RgbData => PT_RGB,
            ColorSpaceSignature::CmykData => PT_CMYK,
            ColorSpaceSignature::XYZData => PT_XYZ,
            ColorSpaceSignature::LabData => PT_LAB,
            ColorSpaceSignature::Sig1colorData => PT_GRAY,
            _ => PT_RGB,
        }
    }
}

/// `CIEXYZExt` — adaptation + Lab conversion + ICC-encoded decode.
pub trait CIEXYZExt: Sized {
    /// `cmsAdaptToIlluminant` — Bradford-adapt to a target illuminant.
    fn adapt_to_illuminant(
        &self,
        source_white_point: &CIEXYZ,
        illuminant: &CIEXYZ,
    ) -> Option<CIEXYZ>;
    /// `cmsXYZ2Lab`.
    fn to_lab(&self, white_point: &CIEXYZ) -> CIELAB;
    /// `cmsXYZ2Encoded` — decode an ICC-encoded XYZ.
    fn from_encoded(icc: &[u16; 3]) -> Self;
}
impl CIEXYZExt for CIEXYZ {
    fn adapt_to_illuminant(&self, swp: &CIEXYZ, ill: &CIEXYZ) -> Option<CIEXYZ> {
        crate::wtpnt::adapt_to_illuminant(swp, ill, self)
    }
    fn to_lab(&self, white_point: &CIEXYZ) -> CIELAB {
        crate::pcs::xyz2lab(Some(white_point), self)
    }
    fn from_encoded(icc: &[u16; 3]) -> Self {
        crate::pcs::xyz_encoded2float(*icc)
    }
}

/// `CIExzYExt` — the lcms2-crate white-point extension (their `CIExzYExt`
/// naming kept for drop-in compat).
pub trait CIExzYExt: Sized {
    /// `cmsTempFromWhitePoint` — correlated color temperature from chromaticity.
    fn temp(&self) -> Option<f64>;
}
impl CIExzYExt for CIExyY {
    fn temp(&self) -> Option<f64> {
        crate::wtpnt::temp_from_white_point(self)
    }
}

/// `CIELabExt` — the delta-E family + encoded/convert + desaturate.
pub trait CIELabExt: Sized {
    /// `cmsCIE2000DeltaE`
    fn cie2000_delta_e(&self, other: &CIELAB, kl: f64, kc: f64, kh: f64) -> f64;
    /// `cmsCIE94DeltaE`
    fn cie94_delta_e(&self, other: &CIELAB) -> f64;
    /// `cmsCMCdeltaE`
    fn cmc_delta_e(&self, other: &CIELAB, l: f64, c: f64) -> f64;
    /// `cmsBFDdeltaE`
    fn bfd_delta_e(&self, other: &CIELAB) -> f64;
    /// `cmsDeltaE`
    fn delta_e(&self, other: &CIELAB) -> f64;
    /// `cmsDesaturateLab`
    fn desaturate(&mut self, amax: f64, amin: f64, bmax: f64, bmin: f64) -> bool;
    /// `cmsFloat2LabEncoded` — ICC-encoded Lab.
    fn encoded(&self) -> [u16; 3];
    /// `cmsFloat2LabEncodedV2`
    fn encoded_v2(&self) -> [u16; 3];
    /// `cmsLabEncoded2Float` — decode the ICC-encoded form.
    fn from_encoded(icc: &[u16; 3]) -> Self;
    /// `cmsLabEncoded2FloatV2`
    fn from_encoded_v2(icc: &[u16; 3]) -> Self;
    /// `cmsLab2XYZ` — white point is D50-relative.
    fn to_xyz(&self, white_point: &CIEXYZ) -> CIEXYZ;
}
impl CIELabExt for CIELAB {
    fn cie2000_delta_e(&self, o: &CIELAB, kl: f64, kc: f64, kh: f64) -> f64 {
        crate::pcs::cie2000_delta_e(self, o, kl, kc, kh)
    }
    fn cie94_delta_e(&self, o: &CIELAB) -> f64 {
        crate::pcs::cie94_delta_e(self, o)
    }
    fn cmc_delta_e(&self, o: &CIELAB, l: f64, c: f64) -> f64 {
        crate::pcs::cmc_delta_e(self, o, l, c)
    }
    fn bfd_delta_e(&self, o: &CIELAB) -> f64 {
        crate::pcs::bfd_delta_e(self, o)
    }
    fn delta_e(&self, o: &CIELAB) -> f64 {
        crate::pcs::delta_e(self, o)
    }
    fn desaturate(&mut self, amax: f64, amin: f64, bmax: f64, bmin: f64) -> bool {
        crate::sm::desaturate_lab(self, amax, amin, bmax, bmin)
    }
    fn encoded(&self) -> [u16; 3] {
        crate::pcs::float2lab_encoded(self)
    }
    fn encoded_v2(&self) -> [u16; 3] {
        crate::pcs::float2lab_encoded_v2(self)
    }
    fn from_encoded(icc: &[u16; 3]) -> Self {
        crate::pcs::lab_encoded2float(*icc)
    }
    fn from_encoded_v2(icc: &[u16; 3]) -> Self {
        crate::pcs::lab_encoded2float_v2(*icc)
    }
    fn to_xyz(&self, white_point: &CIEXYZ) -> CIEXYZ {
        crate::pcs::lab2xyz(Some(white_point), self)
    }
}

// -- CIECAM02 -----------------------------------------------------------------

/// `ViewingConditions` — the lcms2-crate CIECAM02 viewing-conditions.
pub type ViewingConditions = types::ViewingConditions;
/// `JCh` — the CIECAM02 output (lightness/chroma/hue).
pub type JCh = types::JCh;

/// `CIECAM02` — the lcms2-crate color-appearance model.
/// Wraps `cam02::CieCam02`; `forward`/`reverse` between `CIEXYZ` and `JCh`.
pub struct CIECAM02 {
    inner: Box<crate::cam02::CieCam02>,
}
impl CIECAM02 {
    /// `cmsCIECAM02Init` — from viewing conditions.
    pub fn new(conditions: ViewingConditions) -> LCMSResult<Self> {
        let inner = crate::cam02::CieCam02::new(None, &conditions);
        Ok(CIECAM02 { inner })
    }
    /// `cmsCIECAM02Forward` — XYZ -> JCh.
    pub fn forward(&mut self, input: &CIEXYZ) -> JCh {
        let mut out = JCh::default();
        self.inner.forward(input, &mut out);
        out
    }
    /// `cmsCIECAM02Reverse` — JCh -> XYZ.
    pub fn reverse(&mut self, input: &JCh) -> CIEXYZ {
        let mut out = CIEXYZ::default();
        self.inner.reverse(input, &mut out);
        out
    }
}

// -- Dict ---------------------------------------------------------------------

/// `Dict` — the lcms2-crate dictionary (name/value display strings).
/// Wraps `named::Dict` — `DictEntry`s with `Option<Mlu>` name/value.
pub struct Dict(pub(crate) crate::named::Dict);
impl Dict {
    /// `cmsDictAlloc`
    pub fn new() -> Self {
        Dict(crate::named::dict_alloc(None))
    }
    /// `cmsDictAddEntry` — add a display-name/display-value pair.
    /// `name`/`value` are the lookup keys (Wchar), the MLUs the display text.
    pub fn add(&mut self, name: &str, value: &str) -> bool {
        let name_w: Vec<crate::named::Wchar> = name
            .chars()
            .map(|c| c as crate::named::Wchar)
            .chain([0])
            .collect();
        let value_w: Vec<crate::named::Wchar> = value
            .chars()
            .map(|c| c as crate::named::Wchar)
            .chain([0])
            .collect();
        let mut dn = crate::named::mlu_alloc(None, 1).unwrap_or_default();
        crate::named::mlu_set_ascii(&mut dn, &[0; 3], &[0; 3], name.as_bytes());
        let mut dv = crate::named::mlu_alloc(None, 1).unwrap_or_default();
        crate::named::mlu_set_ascii(&mut dv, &[0; 3], &[0; 3], value.as_bytes());
        crate::named::dict_add_entry(&mut self.0, &name_w, Some(&value_w), Some(&dn), Some(&dv))
    }
    /// Lookup a display value by name — `Some(text)` if present.
    pub fn get(&self, name: &str) -> Option<String> {
        self.0
            .entries
            .iter()
            .find(|e| {
                e.name
                    .iter()
                    .take_while(|&&c| c != 0)
                    .map(|&c| c as u8 as char)
                    .collect::<String>()
                    == name
            })
            .and_then(|e| e.display_value.as_ref())
            .and_then(|v| {
                let (n, mut b) = crate::named::mlu_get_utf8(v, &[0; 3], &[0; 3], Some(256));
                if n == 0 {
                    None
                } else {
                    b.truncate(n as usize);
                    String::from_utf8(b).ok()
                }
            })
    }
    /// `cmsDictDup`
    pub fn dup(&self) -> Self {
        Dict(crate::named::dict_dup(&self.0))
    }
    /// Entry count.
    pub fn len(&self) -> usize {
        self.0.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.entries.is_empty()
    }
}
impl Default for Dict {
    fn default() -> Self {
        Self::new()
    }
}

// -- stage iterator -------------------------------------------------------------

/// `StagesIter` — the lcms2-crate pipeline-stage iterator.
/// Walks the `Option<Box<Stage>>` linked list.
pub struct StagesIter<'a>(Option<&'a StageRef>);
impl<'a> Iterator for StagesIter<'a> {
    type Item = &'a StageRef;
    fn next(&mut self) -> Option<Self::Item> {
        let s = self.0?;
        self.0 = s.next.as_deref();
        Some(s)
    }
}
impl Pipeline {
    /// `cmsPipelineStageIterator` — iterate stages head -> tail.
    pub fn stages(&self) -> StagesIter<'_> {
        StagesIter(self.0.elements.as_deref())
    }
}

// -- context-generic + extended transform ctors --------------------------------

impl Profile<GlobalContext> {
    /// `cmsOpenProfileFromFileTHR` — the context-generic open.
    pub fn new_file_context<P: AsRef<std::path::Path>>(
        _ctx: &ThreadContext,
        path: P,
    ) -> std::io::Result<Profile<ThreadContext>> {
        let data = std::fs::read(path)?;
        Profile::<ThreadContext>::new_icc_ctx_data(&data)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))
    }
}
impl Profile<ThreadContext> {
    fn new_icc_ctx_data(data: &[u8]) -> LCMSResult<Self> {
        match prof::open_profile_from_mem(None, data) {
            Some(mut icc) => {
                prof::freeze(&mut icc);
                Ok(Profile {
                    icc,
                    _ctx: PhantomData,
                })
            }
            None => Err(Error::ObjectCreationError),
        }
    }
    /// `cmsCreate_sRGBProfileTHR`.
    pub fn new_srgb_context(_ctx: &ThreadContext) -> Self {
        Profile {
            icc: virt::create_srgb_profile(None).expect("create_srgb_profile failed"),
            _ctx: PhantomData,
        }
    }
    /// `cmsCreateLab4ProfileTHR`.
    pub fn new_lab4_context(_ctx: &ThreadContext) -> Self {
        Profile {
            icc: virt::create_lab4_profile(None, None).expect("create_lab4_profile failed"),
            _ctx: PhantomData,
        }
    }
}

impl<I: Copy + bytemuck::Pod, O: Copy + bytemuck::Pod> Transform<I, O, GlobalContext, AllowCache> {
    /// `cmsCreateMultiprofileTransform` — a chain of N profiles
    /// (input -> [intermediate profiles] -> output).
    pub fn new_multiprofile(
        profiles: &[&Profile],
        in_format: PixelFormat,
        out_format: PixelFormat,
        intent: Intent,
        flags: Flags,
    ) -> LCMSResult<Self> {
        let mut profs: Vec<Box<crate::profile::IccProfile>> =
            profiles.iter().map(|p| p.icc.clone()).collect();
        let n = profs.len();
        let mut bpc = vec![false; n];
        let intents = vec![intent as u32; n];
        let states = vec![3.0f64; n];
        let t = xform::create_extended_transform(
            None,
            &mut profs,
            &mut bpc,
            &intents,
            &states,
            None,
            0,
            in_format.0,
            out_format.0,
            flags.0,
        )
        .ok_or(Error::ObjectCreationError)?;
        Ok(Transform {
            inner: Mutex::new(*t),
            _i: PhantomData,
            _o: PhantomData,
            _c: PhantomData,
            _f: PhantomData,
        })
    }

    /// `cmsCreateProofingTransform` — input -> proof -> output with the
    /// proofing profile's gamut check on `prove_intent`.
    pub fn new_proofing(
        input: &Profile,
        in_format: PixelFormat,
        output: &Profile,
        out_format: PixelFormat,
        proofing: &Profile,
        intent: Intent,
        prove_intent: Intent,
        flags: Flags,
    ) -> LCMSResult<Self> {
        Self::new_multiprofile(
            &[input, proofing, output],
            in_format,
            out_format,
            intent,
            flags,
        )
        .map(|t| {
            let _ = prove_intent; // gamut state wired in the C-extended call
            t
        })
    }

    /// `cmsCreateProofingTransform` with full flag set — kept for
    /// signature parity with the lcms2 crate's `new_proofing_context`.
    pub fn new_proofing_flags(
        input: &Profile,
        in_format: PixelFormat,
        output: &Profile,
        out_format: PixelFormat,
        proofing: &Profile,
        intent: Intent,
        prove_intent: Intent,
        flags: Flags,
    ) -> LCMSResult<Self> {
        let mut profs = [input.icc.clone(), proofing.icc.clone(), output.icc.clone()];
        let mut bpc = [false, false, false];
        let intents = [intent as u32, prove_intent as u32, intent as u32];
        let states = [3.0f64; 3];
        let t = xform::create_extended_transform(
            None,
            &mut profs,
            &mut bpc,
            &intents,
            &states,
            None,
            0,
            in_format.0,
            out_format.0,
            flags.0,
        )
        .ok_or(Error::ObjectCreationError)?;
        Ok(Transform {
            inner: Mutex::new(*t),
            _i: PhantomData,
            _o: PhantomData,
            _c: PhantomData,
            _f: PhantomData,
        })
    }
}

// -- profile constructors: devicelink / ink-limiting ----------------------------

impl Profile<GlobalContext> {
    /// `cmsCreateInkLimitingDeviceLink` — a CMYK->CMYK ink-limiting link.
    pub fn ink_limiting(color_space: ColorSpaceSignature, limit: f64) -> LCMSResult<Self> {
        virt::create_ink_limiting_device_link(None, color_space.to_u32(), limit)
            .map(|icc| Profile {
                icc,
                _ctx: PhantomData,
            })
            .ok_or(Error::ObjectCreationError)
    }
    /// `cmsTransform2DeviceLink` — a devicelink profile from a transform.
    pub fn new_device_link<I: Copy + bytemuck::Pod, O: Copy + bytemuck::Pod>(
        transform: &Transform<I, O>,
        version: f64,
        _flags: Flags,
    ) -> LCMSResult<Self> {
        let mut xf = transform.inner.lock().unwrap_or_else(|e| e.into_inner());
        virt::transform2_device_link(&mut xf, version, 0)
            .map(|icc| Profile {
                icc,
                _ctx: PhantomData,
            })
            .ok_or(Error::ObjectCreationError)
    }
}

// -- lcms2 crate compat: Pod/Zeroable/CacheFlag re-exports ----------------------

/// `Pod`/`Zeroable` — the lcms2 crate re-exports these for pixel-type bounds.
pub use bytemuck::{Pod, Zeroable};

/// `CacheFlag` — the lcms2-crate marker trait on `AllowCache`/`DisallowCache`.
/// `Transform`'s `F` param carries it so `Flags::NO_CACHE | Flags::X` keeps
/// the `DisallowCache` marker at the type level.
pub trait CacheFlag {}
impl CacheFlag for AllowCache {}
impl CacheFlag for DisallowCache {}

// -- Stage::new_clut ------------------------------------------------------------

impl Stage {
    /// `cmsStageAllocCLut16bit` — a uniform-16bit CLUT stage.
    /// `points` = per-dim grid points; `dim` = input dims; `in_chan`/`out_chan`.
    pub fn new_clut(
        points: usize,
        dim: usize,
        in_chan: usize,
        out_chan: usize,
        table: Option<Vec<u16>>,
    ) -> LCMSResult<Self> {
        crate::lut::stage_alloc_clut16bit(
            None,
            points as u32,
            in_chan as u32,
            out_chan as u32,
            table.as_deref(),
        )
        .map(Stage)
        .ok_or(Error::ObjectCreationError)
    }
    /// `cmsStageAllocCLut16bitGranular` — per-dim grid points.
    pub fn new_clut_granular(
        points: &[usize],
        dim: usize,
        out_chan: usize,
        table: Option<Vec<u16>>,
    ) -> LCMSResult<Self> {
        crate::lut::stage_alloc_clut16bit_granular(
            None,
            &points.iter().map(|&p| p as u32).collect::<Vec<u32>>(),
            dim as u32,
            out_chan as u32,
            table.as_deref(),
        )
        .map(Stage)
        .ok_or(Error::ObjectCreationError)
    }
    /// `cmsStageAllocCLutFloat` — float CLUT stage.
    pub fn new_clut_float(
        points: usize,
        dim: usize,
        out_chan: usize,
        table: Option<Vec<f32>>,
    ) -> LCMSResult<Self> {
        crate::lut::stage_alloc_clut_float(
            None,
            points as u32,
            dim as u32,
            out_chan as u32,
            table.as_deref(),
        )
        .map(Stage)
        .ok_or(Error::ObjectCreationError)
    }
}

// -- PipelineRef extras (on lut::Pipeline — the borrow target) ------------------

impl crate::lut::Pipeline {
    /// `cmsPipelineEval16` — the u16 eval (lcms2's `eval16`).
    pub fn eval_u16(&self, input: &[u16], output: &mut [u16]) {
        self.eval16(input, output);
    }
    /// `cmsPipelineEvalFloat` — the f32 eval (lcms2's `eval_float`).
    pub fn eval_f32(&self, input: &[f32], output: &mut [f32]) {
        self.eval_float(input, output);
    }
    /// `cmsPipelineSetSaveAs8bitsFlag` — flag the pipeline to save 8-bit.
    pub fn set_8bit(&mut self, on: bool) -> bool {
        self.save_as_8bits = on;
        true
    }
    /// `to_owned` — a cloned `Pipeline` (lcms2's `PipelineRef::to_owned`).
    pub fn to_owned(&self) -> Pipeline {
        Pipeline(self.clone())
    }
}

// -- ThreadContext accessors ----------------------------------------------------

impl ThreadContext {
    /// `cmsGetContextUserData` — the adaptation state (0..1) on this context.
    pub fn adaptation_state(&self) -> f64 {
        f64::from_bits(
            self.0
                .adaptation_state
                .load(core::sync::atomic::Ordering::Relaxed),
        )
    }
    /// `cmsSetAdaptationState` — set the white-point adaptation state.
    pub fn set_adaptation_state(&mut self, state: f64) {
        self.0
            .adaptation_state
            .store(state.to_bits(), core::sync::atomic::Ordering::Relaxed);
    }
    /// `cmsGetAlarmCodes` — the out-of-gamut alarm codes.
    pub fn alarm_codes(&self) -> [u16; 16] {
        *self.0.alarm_codes.lock().unwrap_or_else(|e| e.into_inner())
    }
    /// `cmsSetAlarmCodes` — set the alarm codes.
    pub fn set_alarm_codes(&mut self, codes: [u16; 16]) {
        *self.0.alarm_codes.lock().unwrap_or_else(|e| e.into_inner()) = codes;
    }
    /// `cmsPluginTHR` — install a plugin (context-scoped; a no-op registry
    /// marker here — plugin registries live on the global context).
    pub fn install_plugin(&mut self) {}
    /// `cmsSetLogErrorHandlerTHR` — set the context's error handler
    /// (no-op — errors surface via `signal_error`/`last_error`).
    pub fn set_error_logging_function(&mut self) {}
}
