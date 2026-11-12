//! `lcms::api` — the idiomatic facade.
//!
//! The `cms*`/`pub mod` surface (`little-cms-api` feature) stays 1:1 with
//! the C source for conformance; this module wraps it in owned,
//! `Result`-returning, `Send`/`Sync`-friendly types for real applications.
//! It is always compiled — no feature required. Plans built here link
//! exactly as C's `cmsCreateTransform` / `cmsCreateMultiprofileTransform` /
//! `cmsCreateProofingTransform` do, and produce the same pixels
//! (gated by `tests/api_c_parity.rs` against the C library).
//!
//! ## The shared-plan model (for services)
//!
//! - **[`Profile`]** — an owned ICC profile. Every constructor decodes all
//!   tags up front, so a `Profile` is fully readable through `&self` and is
//!   `Send` + `Sync`: `Arc`-share it. [`Profile::get`], [`Profile::info`],
//!   [`Profile::named_colors`] are lock-free reads.
//! - **[`Plan`]** — the immutable linked, optimized transform. `Sync`:
//!   build once, `Arc`-share across workers, cache by
//!   `(src, dst, formats, PlanOptions)`.
//! - **[`Scratch`]** — the only per-run state (~64 B pixel cache): one per
//!   thread, or let [`Plan::transform`] make a fresh one per call.
//!
//! A proxy server keeps a profile cache (`Arc<Profile>` by content hash)
//! and a plan cache (`Arc<Plan>` by config key); each worker calls
//! [`Plan::apply`] with its own scratch. Nothing on the pixel path locks.
//!
//! ```
//! use lcms::api::*;
//!
//! // An embedded profile would be `Profile::open(&icc_bytes)?`.
//! let src = Profile::srgb();
//! let dst = Profile::lab4();
//!
//! // Link once — the expensive part. `&Profile` borrows are enough.
//! let plan = Plan::new(&src, &dst, Format::RGB_8, Format::LAB_DBL)?;
//!
//! // One-off call: the plan makes its own scratch.
//! let mut lab = [[0f64; 3]; 2];
//! plan.transform_pixels(&[[255u8, 0, 0], [0, 0, 255]], &mut lab);
//! assert!(lab[0][1] > 60.0);  // red: strongly positive a*
//! assert!(lab[1][2] < -90.0); // blue: strongly negative b*
//!
//! // Hot loop: keep a scratch per thread.
//! let mut sc = plan.scratch();
//! let mut out = [0u8; 2 * 24];
//! plan.apply(&mut sc, &[255, 255, 255, 0, 0, 0], &mut out);
//! # Ok::<(), lcms::api::Error>(())
//! ```
//!
//! Intent, black-point compensation and flags go through [`PlanOptions`];
//! rows with padding through [`Plan::apply_strided`]; big images through
//! [`Plan::apply_par`].
//!
//! Tags, info text, named colors, header fields:
//!
//! ```
//! use lcms::api::*;
//!
//! let mut p = Profile::srgb();
//! for tag in p.tags() { println!("{tag}: {:?}", p.get(tag)); }
//! assert_eq!(p.white_point().map(|wp| wp.Y), Some(1.0));
//! p.write_tag(Tag::COPYRIGHT, "No rights reserved")?;
//! assert_eq!(p.info(InfoType::Copyright).as_deref(), Some("No rights reserved"));
//! # Ok::<(), lcms::api::Error>(())
//! ```
//!
//! Buffer sizes that don't match a plan's formats are programming errors
//! and panic (like slice indexing); everything that depends on input data
//! (profile bytes, tag types, link feasibility) returns [`Result`].

use crate::tagtypes::{TagObject, Tm, VcgtData};
use crate::{named, profile, types, virt, xform};
use std::fmt;

// ---------------------------------------------------------------------------
// Re-exports — the api's owned/borrowed vocabulary types.
//
// These are the parity layer's own types, not wrappers, so a `TagValue`
// borrows straight into the profile (`&Pipeline`, `&ToneCurve`) at no cost.
// In the default build their fields and C-shaped methods are `pub(crate)`
// (`parity_struct!`/`parity_impl!` in lib.rs): what `lcms::api` shows is the
// methods defined in this file plus the few accessors the parity impls keep
// public (`eval16`, `eval_float`, `input_channels`, ...).
// ---------------------------------------------------------------------------

/// A tone curve — gamma, parametric, tabulated or segmented
/// (`cmsToneCurve`). Owned and borrowed like any Rust value: build one with
/// [`ToneCurve::gamma`] and friends, or borrow one out of a profile with
/// [`TagValue::as_curve`].
///
/// ```
/// use lcms::api::ToneCurve;
/// let srgb = ToneCurve::srgb();
/// assert_eq!(srgb.eval16(0), 0);
/// assert_eq!(srgb.eval16(0xffff), 0xffff);
/// assert!(srgb.is_monotonic());
///
/// let g = ToneCurve::gamma(2.2)?;
/// let inv = g.inverse()?;
/// assert!((inv.eval_float(g.eval_float(0.5)) - 0.5).abs() < 1e-3);
/// assert!(g.then_inverse_of(&g, 256)?.is_linear()); // g⁻¹(g(t)) = t
/// assert!((g.estimated_gamma(0.01).unwrap() - 2.2).abs() < 0.01);
/// # Ok::<(), lcms::api::Error>(())
/// ```
pub use crate::gamma::ToneCurve;
/// A processing pipeline — a chain of [`Stage`]s (`cmsPipeline`; the
/// payload of mft1/mft2/mAB/mBA/mpet LUT tags).
///
/// ```
/// use lcms::api::*;
/// let mut lut = Pipeline::new(3, 3)?;
/// lut.push(Stage::matrix(&[0., 0., 1., 0., 1., 0., 1., 0., 0.], 3, 3, None)?)?;
/// lut.push(Stage::identity(3))?;
/// assert_eq!(lut.stage_count(), 2);
/// let mut out = [0f32; 3];
/// lut.eval_float(&[0.1, 0.2, 0.3], &mut out);
/// assert_eq!(out, [0.3, 0.2, 0.1]); // the matrix swaps R and B
/// # Ok::<(), lcms::api::Error>(())
/// ```
pub use crate::lut::Pipeline;
/// One pipeline stage — curves, a matrix, a CLUT, ... (`cmsStage`).
pub use crate::lut::Stage;
/// A name/value dictionary — `cmsDICT` (`'dict'` tags).
///
/// ```
/// use lcms::api::Dict;
/// let mut d = Dict::new();
/// d.add("model", "X1")?;
/// assert_eq!(d.get("model").as_deref(), Some("X1"));
/// assert_eq!(d.iter().collect::<Vec<_>>(), [("model".to_string(), "X1".to_string())]);
/// # Ok::<(), lcms::api::Error>(())
/// ```
pub use crate::named::Dict;
/// Multi-language Unicode text — `cmsMLU`.
///
/// ```
/// use lcms::api::{Locale, Mlu};
/// let mut m = Mlu::from("Hello");
/// m.set(Locale::new("es_ES"), "Hola")?;
/// assert_eq!(m.get(Locale::new("es_ES")).as_deref(), Some("Hola"));
/// assert_eq!(m.get(Locale::NONE).as_deref(), Some("Hello"));
/// assert_eq!(m.len(), 2);
/// # Ok::<(), lcms::api::Error>(())
/// ```
pub use crate::named::Mlu;
/// A named-color palette — `cmsNAMEDCOLORLIST` (`'ncl2'`/`'clrt'` tags).
///
/// ```
/// use lcms::api::*;
/// let mut list = NamedColorList::new(3, "PANTONE ", " C")?;
/// list.push("Orange 021", [0xa000, 0xb000, 0x2000], &[0, 0x8000, 0xffff])?;
/// assert_eq!(list.position("orange 021"), Some(0)); // ASCII case-insensitive, like C
/// for c in &list { assert_eq!(c.colorant, [0, 0x8000, 0xffff]); }
/// # Ok::<(), lcms::api::Error>(())
/// ```
pub use crate::named::NamedColorList;
/// One entry of a [`ProfileSeqDesc`] — `cmsPSEQDESC`.
pub use crate::named::PSeqDesc as ProfileSeqEntry;
/// A profile sequence descriptor — `cmsSEQ` (`'pseq'`/`'psid'` tags).
pub use crate::named::ProfileSeqDesc;
/// An ICC `'data'` tag payload — `cmsICCData` (ASCII text or binary bytes).
pub use crate::tagtypes::IccData;
/// ICC `'view'` viewing conditions — `cmsICCViewingConditions`.
pub use crate::tagtypes::IccViewingConditions;
/// ICC `'MHC2'` display data — `cmsMHC2Type`. Plain data shared with the
/// `lcms2` shim; [`Profile::write_tag`] rejects it unless every curve holds
/// `curve_entries` values.
pub use crate::tagtypes::Mhc2;
/// ICC `'bfd '` undercolor removal / black generation — `cmsUcrBg`.
pub use crate::tagtypes::UcrBg;
/// ICC `'cicp'` video signal — `cmsVideoSignalType`.
pub use crate::tagtypes::VideoSignal;
/// ICC `'scrn'` halftone screening — `cmsScreening` + `cmsScreeningChannel`.
/// Plain data shared with the `lcms2` shim; the first `n_channels` (at most
/// 16) entries of `channels` are used, and [`Profile::write_tag`] rejects a
/// larger count.
pub use crate::tagtypes::{Screening, ScreeningChannel};
/// CIE XYZ / xyY / Lab / LCh vocabulary types (`cmsCIE*`).
pub use crate::types::{CieLCh, CieLab, CieXyz, CiexyY, CiexyYTriple};
/// ICC header types — `'dtim'` date, `cmsProfileID`, `'meas'` conditions.
pub use crate::types::{DateTimeNumber, IccMeasurementConditions, ProfileId};

// -- Error -----------------------------------------------------------------

/// The api result alias — `Result<T, api::Error>`.
pub type Result<T> = std::result::Result<T, Error>;

/// An lcms failure: the `cmsERROR_*` code the failing path signaled, and
/// its message.
///
/// The message is the real text lcms signaled (`cmsSignalError`), or a
/// short "`<operation>` failed" when the path failed without signaling.
///
/// ```
/// use lcms::api::{ErrorKind, Profile};
/// let e = Profile::open(b"not an ICC profile").unwrap_err();
/// assert_eq!(e.kind(), ErrorKind::Read); // too short for an ICC header
/// assert!(!e.message().is_empty());
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    code: i32,
    message: String,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({:?})", self.message, self.kind())
    }
}
impl std::error::Error for Error {}

/// What went wrong — the `cmsERROR_*` code of an [`Error`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ErrorKind {
    /// `cmsERROR_UNDEFINED` — the failing path didn't set a code.
    Undefined,
    /// `cmsERROR_FILE` — generic file failure.
    File,
    /// `cmsERROR_RANGE` — an argument was out of range.
    Range,
    /// `cmsERROR_INTERNAL` — internal consistency check.
    Internal,
    /// `cmsERROR_NULL` — a required argument was null/empty.
    Null,
    /// `cmsERROR_READ` — read failure (short or malformed input).
    Read,
    /// `cmsERROR_SEEK` — seek failure inside a stream.
    Seek,
    /// `cmsERROR_WRITE` — write/serialization failure.
    Write,
    /// `cmsERROR_UNKNOWN_EXTENSION`.
    UnknownExtension,
    /// `cmsERROR_COLORSPACE_CHECK` — format/profile color-space mismatch.
    ColorSpaceCheck,
    /// `cmsERROR_ALREADY_DEFINED`.
    AlreadyDefined,
    /// `cmsERROR_BAD_SIGNATURE`.
    BadSignature,
    /// `cmsERROR_CORRUPTION_DETECTED` — malformed profile data.
    CorruptionDetected,
    /// `cmsERROR_NOT_SUITABLE` — profile can't serve this purpose.
    NotSuitable,
    /// A code outside the `cmsERROR_*` set.
    Other(i32),
}

impl Error {
    pub(crate) fn new(code: i32, message: impl Into<String>) -> Self {
        Error {
            code,
            message: message.into(),
        }
    }
    /// Build from the last `signal_error` this thread produced, or the
    /// fallback if the failing path didn't signal one.
    pub(crate) fn from_last(fallback_code: i32, fallback: &str) -> Self {
        match crate::err::take_last_error() {
            Some((code, message)) => Error { code, message },
            None => Error::new(fallback_code, fallback),
        }
    }
    /// The signaled message (or the "`<operation>` failed" fallback).
    pub fn message(&self) -> &str {
        &self.message
    }
    /// The raw `cmsERROR_*` code; [`Error::kind`] is the typed form.
    pub fn code(&self) -> i32 {
        self.code
    }
    /// What went wrong.
    pub fn kind(&self) -> ErrorKind {
        use types as t;
        match self.code {
            t::ERROR_UNDEFINED => ErrorKind::Undefined,
            t::ERROR_FILE => ErrorKind::File,
            t::ERROR_RANGE => ErrorKind::Range,
            t::ERROR_INTERNAL => ErrorKind::Internal,
            t::ERROR_NULL => ErrorKind::Null,
            t::ERROR_READ => ErrorKind::Read,
            t::ERROR_SEEK => ErrorKind::Seek,
            t::ERROR_WRITE => ErrorKind::Write,
            t::ERROR_UNKNOWN_EXTENSION => ErrorKind::UnknownExtension,
            t::ERROR_COLORSPACE_CHECK => ErrorKind::ColorSpaceCheck,
            t::ERROR_ALREADY_DEFINED => ErrorKind::AlreadyDefined,
            t::ERROR_BAD_SIGNATURE => ErrorKind::BadSignature,
            t::ERROR_CORRUPTION_DETECTED => ErrorKind::CorruptionDetected,
            t::ERROR_NOT_SUITABLE => ErrorKind::NotSuitable,
            other => ErrorKind::Other(other),
        }
    }
}

/// Run a port call that reports failure as `None`, mapping it to an
/// [`Error`] carrying the message *that call* signalled. Any stale
/// message an earlier call left on this thread is discarded first, so a
/// failure is never blamed on someone else's error; messages a
/// successful call signalled (warnings) are discarded after it.
fn attempt<T>(fallback_id: i32, what: &str, f: impl FnOnce() -> Option<T>) -> Result<T> {
    let _ = crate::err::take_last_error();
    match f() {
        Some(v) => {
            let _ = crate::err::take_last_error();
            Ok(v)
        }
        None => Err(Error::from_last(fallback_id, &format!("{what} failed"))),
    }
}

/// `bool`-returning variant of [`attempt`].
fn attempt_bool(fallback_id: i32, what: &str, f: impl FnOnce() -> bool) -> Result<()> {
    attempt(fallback_id, what, || f().then_some(()))
}

// -- Format ---------------------------------------------------------------

// pixel-type codes (COLORSPACE_SH payload in the format u32)
const PT_GRAY: u32 = 3;
const PT_RGB: u32 = 4;
const PT_CMY: u32 = 5;
const PT_CMYK: u32 = 6;
const PT_YCBCR: u32 = 7;
const PT_YUV: u32 = 8;
const PT_XYZ: u32 = 9;
const PT_LAB: u32 = 10;
const PT_YUVK: u32 = 11;
const PT_HSV: u32 = 12;
const PT_HLS: u32 = 13;
const PT_YXY: u32 = 14;
const PT_LABV2: u32 = 30;

// `cmsFORMAT` field shifts (lcms2.h `*_SH` macros)
const fn colorspace_sh(pt: u32) -> u32 {
    pt << 16
}
const fn channels_sh(c: u32) -> u32 {
    c << 3
}
const fn extra_sh(e: u32) -> u32 {
    e << 7
}
const DOSWAP_SH: u32 = 1 << 10;
const PLANAR_SH: u32 = 1 << 12;
const SWAPFIRST_SH: u32 = 1 << 14;
const FLOAT_SH: u32 = 1 << 22;

/// Pixel format — the packed `cmsFORMAT` code, with constants for the
/// common `TYPE_*` layouts.
///
/// Any lcms format is representable ([`Format::from_u32`]); two `Format`s
/// are equal exactly when their codes are. Sample sizes: `_8` = `u8`,
/// `_16` = native-endian `u16`, `_HALF` = IEEE half floats in a `u16`,
/// `_FLT` = `f32`, `_DBL` = `f64`. Float formats use lcms's ranges: RGB and
/// gray `0.0..=1.0`, ink spaces such as CMYK in percent (`0.0..=100.0`),
/// Lab/XYZ in natural units (L* 0–100, Y = 1.0 for white).
///
/// ```
/// use lcms::api::Format;
/// let f = Format::RGBA_8;
/// assert_eq!((f.channels(), f.extra_channels(), f.pixel_size()), (3, 1, 4));
/// assert_eq!(Format::from_u32(f.code()), f);
/// assert_eq!(Format::CMYK_DBL.bytes_per_channel(), 8);
/// assert!(Format::RGB_8.planar().is_planar());
/// assert_eq!(format!("{:?}", Format::BGRA_8), "Format::BGRA_8");
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct Format(u32);

macro_rules! format_consts {
    ($( $(#[$m:meta])* $name:ident = $v:expr, )*) => {
        impl Format {
            $( $(#[$m])* pub const $name: Format = Format($v); )*
            /// The constant's name, for `Debug`.
            fn name(self) -> Option<&'static str> {
                $( if self.0 == $v { return Some(stringify!($name)); } )*
                None
            }
        }
    };
}

format_consts! {
    /// `TYPE_GRAY_8`
    GRAY_8 = colorspace_sh(PT_GRAY) | channels_sh(1) | 1,
    /// `TYPE_GRAY_16`
    GRAY_16 = colorspace_sh(PT_GRAY) | channels_sh(1) | 2,
    /// `TYPE_GRAY_HALF_FLT`
    GRAY_HALF = FLOAT_SH | colorspace_sh(PT_GRAY) | channels_sh(1) | 2,
    /// `TYPE_GRAY_FLT`
    GRAY_FLT = FLOAT_SH | colorspace_sh(PT_GRAY) | channels_sh(1) | 4,
    /// `TYPE_GRAY_DBL`
    GRAY_DBL = FLOAT_SH | colorspace_sh(PT_GRAY) | channels_sh(1),
    /// `TYPE_RGB_8`
    RGB_8 = colorspace_sh(PT_RGB) | channels_sh(3) | 1,
    /// `TYPE_RGB_16`
    RGB_16 = colorspace_sh(PT_RGB) | channels_sh(3) | 2,
    /// `TYPE_RGB_HALF_FLT`
    RGB_HALF = FLOAT_SH | colorspace_sh(PT_RGB) | channels_sh(3) | 2,
    /// `TYPE_RGB_FLT`
    RGB_FLT = FLOAT_SH | colorspace_sh(PT_RGB) | channels_sh(3) | 4,
    /// `TYPE_RGB_DBL`
    RGB_DBL = FLOAT_SH | colorspace_sh(PT_RGB) | channels_sh(3),
    /// `TYPE_BGR_8`
    BGR_8 = colorspace_sh(PT_RGB) | channels_sh(3) | 1 | DOSWAP_SH,
    /// `TYPE_BGR_16`
    BGR_16 = colorspace_sh(PT_RGB) | channels_sh(3) | 2 | DOSWAP_SH,
    /// `TYPE_BGR_FLT`
    BGR_FLT = FLOAT_SH | colorspace_sh(PT_RGB) | channels_sh(3) | 4 | DOSWAP_SH,
    /// `TYPE_RGBA_8` — alpha is an extra channel; add
    /// [`flags::COPY_ALPHA`] to carry it through.
    RGBA_8 = colorspace_sh(PT_RGB) | extra_sh(1) | channels_sh(3) | 1,
    /// `TYPE_RGBA_16`
    RGBA_16 = colorspace_sh(PT_RGB) | extra_sh(1) | channels_sh(3) | 2,
    /// `TYPE_RGBA_HALF_FLT`
    RGBA_HALF = FLOAT_SH | colorspace_sh(PT_RGB) | extra_sh(1) | channels_sh(3) | 2,
    /// `TYPE_RGBA_FLT`
    RGBA_FLT = FLOAT_SH | colorspace_sh(PT_RGB) | extra_sh(1) | channels_sh(3) | 4,
    /// `TYPE_BGRA_8`
    BGRA_8 = colorspace_sh(PT_RGB) | extra_sh(1) | channels_sh(3) | 1 | DOSWAP_SH | SWAPFIRST_SH,
    /// `TYPE_BGRA_16`
    BGRA_16 = colorspace_sh(PT_RGB) | extra_sh(1) | channels_sh(3) | 2 | DOSWAP_SH | SWAPFIRST_SH,
    /// `TYPE_BGRA_FLT`
    BGRA_FLT = FLOAT_SH | colorspace_sh(PT_RGB) | extra_sh(1) | channels_sh(3) | 4 | DOSWAP_SH | SWAPFIRST_SH,
    /// `TYPE_ARGB_8`
    ARGB_8 = colorspace_sh(PT_RGB) | extra_sh(1) | channels_sh(3) | 1 | SWAPFIRST_SH,
    /// `TYPE_ARGB_16`
    ARGB_16 = colorspace_sh(PT_RGB) | extra_sh(1) | channels_sh(3) | 2 | SWAPFIRST_SH,
    /// `TYPE_ARGB_FLT`
    ARGB_FLT = FLOAT_SH | colorspace_sh(PT_RGB) | extra_sh(1) | channels_sh(3) | 4 | SWAPFIRST_SH,
    /// `TYPE_ABGR_8`
    ABGR_8 = colorspace_sh(PT_RGB) | extra_sh(1) | channels_sh(3) | 1 | DOSWAP_SH,
    /// `TYPE_ABGR_16`
    ABGR_16 = colorspace_sh(PT_RGB) | extra_sh(1) | channels_sh(3) | 2 | DOSWAP_SH,
    /// `TYPE_ABGR_FLT`
    ABGR_FLT = FLOAT_SH | colorspace_sh(PT_RGB) | extra_sh(1) | channels_sh(3) | 4 | DOSWAP_SH,
    /// `TYPE_CMYK_8`
    CMYK_8 = colorspace_sh(PT_CMYK) | channels_sh(4) | 1,
    /// `TYPE_CMYK_16`
    CMYK_16 = colorspace_sh(PT_CMYK) | channels_sh(4) | 2,
    /// `TYPE_CMYK_HALF_FLT`
    CMYK_HALF = FLOAT_SH | colorspace_sh(PT_CMYK) | channels_sh(4) | 2,
    /// `TYPE_CMYK_FLT`
    CMYK_FLT = FLOAT_SH | colorspace_sh(PT_CMYK) | channels_sh(4) | 4,
    /// `TYPE_CMYK_DBL`
    CMYK_DBL = FLOAT_SH | colorspace_sh(PT_CMYK) | channels_sh(4),
    /// `TYPE_Lab_8`
    LAB_8 = colorspace_sh(PT_LAB) | channels_sh(3) | 1,
    /// `TYPE_Lab_16` — ICC v4 16-bit Lab encoding.
    LAB_16 = colorspace_sh(PT_LAB) | channels_sh(3) | 2,
    /// `TYPE_LabV2_8`
    LAB_V2_8 = colorspace_sh(PT_LABV2) | channels_sh(3) | 1,
    /// `TYPE_LabV2_16` — ICC v2 16-bit Lab encoding.
    LAB_V2_16 = colorspace_sh(PT_LABV2) | channels_sh(3) | 2,
    /// `TYPE_Lab_FLT`
    LAB_FLT = FLOAT_SH | colorspace_sh(PT_LAB) | channels_sh(3) | 4,
    /// `TYPE_Lab_DBL` — the layout of [`CieLab`].
    LAB_DBL = FLOAT_SH | colorspace_sh(PT_LAB) | channels_sh(3),
    /// `TYPE_XYZ_16`
    XYZ_16 = colorspace_sh(PT_XYZ) | channels_sh(3) | 2,
    /// `TYPE_XYZ_FLT`
    XYZ_FLT = FLOAT_SH | colorspace_sh(PT_XYZ) | channels_sh(3) | 4,
    /// `TYPE_XYZ_DBL` — the layout of [`CieXyz`].
    XYZ_DBL = FLOAT_SH | colorspace_sh(PT_XYZ) | channels_sh(3),
    /// `TYPE_NAMED_COLOR_INDEX` — one `u16` index into a named-color list.
    NAMED_COLOR_INDEX = channels_sh(1) | 2,
}

impl Format {
    /// Wrap any packed `cmsFORMAT` code (`TYPE_*`).
    pub const fn from_u32(code: u32) -> Format {
        Format(code)
    }
    /// The packed `cmsFORMAT` code (`TYPE_*`).
    pub const fn code(self) -> u32 {
        self.0
    }
    /// Bytes per pixel, counting color and extra channels:
    /// `total_channels × bytes_per_channel`. For a planar format, the
    /// summed size of one pixel across its planes.
    pub const fn pixel_size(self) -> usize {
        self.total_channels() * self.bytes_per_channel()
    }
    /// Bytes per sample: 1, 2, 4 or 8 (`T_BYTES`; lcms's `0` means 8-byte
    /// doubles).
    pub const fn bytes_per_channel(self) -> usize {
        match crate::pack::t_bytes(self.0) {
            0 => 8,
            b => b as usize,
        }
    }
    /// Color channels — not counting alpha/extra.
    pub const fn channels(self) -> usize {
        crate::pack::t_channels(self.0) as usize
    }
    /// Extra (alpha / spot) channels.
    pub const fn extra_channels(self) -> usize {
        crate::pack::t_extra(self.0) as usize
    }
    /// `channels + extra_channels` — total components per pixel.
    pub const fn total_channels(self) -> usize {
        self.channels() + self.extra_channels()
    }
    /// Floating-point samples (`T_FLOAT`), half floats included.
    pub const fn is_float(self) -> bool {
        crate::pack::t_float(self.0) != 0
    }
    /// One plane per channel (`T_PLANAR`) instead of interleaved pixels.
    pub const fn is_planar(self) -> bool {
        crate::pack::t_planar(self.0) != 0
    }
    /// The same layout, planar.
    pub const fn planar(self) -> Format {
        Format(self.0 | PLANAR_SH)
    }
    /// The [`ColorSpace`] this format's pixel-type encodes — `None` for
    /// pixel types without a named color space (YUV, exotic).
    pub fn color_space(self) -> Option<ColorSpace> {
        match crate::pack::t_colorspace(self.code()) {
            PT_GRAY => Some(ColorSpace::Gray),
            PT_RGB => Some(ColorSpace::Rgb),
            PT_CMY => Some(ColorSpace::Cmy),
            PT_CMYK => Some(ColorSpace::Cmyk),
            PT_XYZ => Some(ColorSpace::Xyz),
            PT_LAB | PT_LABV2 => Some(ColorSpace::Lab),
            PT_YCBCR => Some(ColorSpace::YCbCr),
            PT_HSV => Some(ColorSpace::Hsv),
            PT_HLS => Some(ColorSpace::Hls),
            PT_YXY => Some(ColorSpace::Yxy),
            PT_YUV | PT_YUVK => None,
            m @ 15..=29 => Some(ColorSpace::Mch((m - 14) as u8)),
            _ => None,
        }
    }
}
impl From<u32> for Format {
    fn from(code: u32) -> Format {
        Format::from_u32(code)
    }
}
impl From<Format> for u32 {
    fn from(f: Format) -> u32 {
        f.code()
    }
}
impl fmt::Debug for Format {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(n) = self.name() {
            return write!(f, "Format::{n}");
        }
        if self.is_planar() {
            if let Some(n) = Format(self.0 & !PLANAR_SH).name() {
                return write!(f, "Format::{n}.planar()");
            }
        }
        write!(f, "Format(0x{:x})", self.0)
    }
}
impl fmt::Display for Format {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

// -- signatures ---------------------------------------------------------------

/// Write an ICC 4-byte signature as `'xxxx'` (non-printable → `.`).
fn fmt_sig4(sig: u32, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    let b = sig.to_be_bytes();
    f.write_str("'")?;
    for c in b {
        write!(
            f,
            "{}",
            if c.is_ascii_graphic() || c == b' ' {
                c as char
            } else {
                '.'
            }
        )?;
    }
    f.write_str("'")
}

/// A 4-byte ICC tag signature — `Display`/`Debug` print the `'xxxx'` chars.
///
/// ```
/// use lcms::api::Tag;
/// let wtpt = Tag::new(*b"wtpt");
/// assert_eq!(wtpt, Tag::MEDIA_WHITE_POINT);
/// assert_eq!(format!("{wtpt}"), "'wtpt'");
/// ```
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Tag(pub u32);
impl Tag {
    /// Build a tag signature from its four chars: `Tag::new(*b"wtpt")`.
    pub const fn new(sig: [u8; 4]) -> Tag {
        Tag(u32::from_be_bytes(sig))
    }
    /// The raw signature value.
    pub const fn code(self) -> u32 {
        self.0
    }
    /// The four signature bytes (`'w','t','p','t'`).
    pub const fn chars(self) -> [u8; 4] {
        self.0.to_be_bytes()
    }

    // -- common tag signatures — the full set is `lcms::signatures::tag` --

    /// `'wtpt'` — media white point.
    pub const MEDIA_WHITE_POINT: Tag = Tag(crate::signatures::tag::SigMediaWhitePointTag);
    /// `'bkpt'` — media black point.
    pub const MEDIA_BLACK_POINT: Tag = Tag(crate::signatures::tag::SigMediaBlackPointTag);
    /// `'cprt'` — copyright.
    pub const COPYRIGHT: Tag = Tag(crate::signatures::tag::SigCopyrightTag);
    /// `'desc'` — profile description.
    pub const PROFILE_DESCRIPTION: Tag = Tag(crate::signatures::tag::SigProfileDescriptionTag);
    /// `'dscm'` — profile description (ML).
    pub const PROFILE_DESCRIPTION_ML: Tag = Tag(crate::signatures::tag::SigProfileDescriptionMLTag);
    /// `'dmnd'` — device manufacturer description.
    pub const DEVICE_MFG_DESC: Tag = Tag(crate::signatures::tag::SigDeviceMfgDescTag);
    /// `'dmdd'` — device model description.
    pub const DEVICE_MODEL_DESC: Tag = Tag(crate::signatures::tag::SigDeviceModelDescTag);
    /// `'chad'` — chromatic adaptation matrix.
    pub const CHROMATIC_ADAPTATION: Tag = Tag(crate::signatures::tag::SigChromaticAdaptationTag);
    /// `'rTRC'`/`'gTRC'`/`'bTRC'`/`'kTRC'` — channel tone curves.
    pub const RED_TRC: Tag = Tag(crate::signatures::tag::SigRedTRCTag);
    pub const GREEN_TRC: Tag = Tag(crate::signatures::tag::SigGreenTRCTag);
    pub const BLUE_TRC: Tag = Tag(crate::signatures::tag::SigBlueTRCTag);
    pub const GRAY_TRC: Tag = Tag(crate::signatures::tag::SigGrayTRCTag);
    /// `'rXYZ'`/`'gXYZ'`/`'bXYZ'` — matrix colorants.
    pub const RED_COLORANT: Tag = Tag(crate::signatures::tag::SigRedColorantTag);
    pub const GREEN_COLORANT: Tag = Tag(crate::signatures::tag::SigGreenColorantTag);
    pub const BLUE_COLORANT: Tag = Tag(crate::signatures::tag::SigBlueColorantTag);
    /// `'ncl2'` — named color palette.
    pub const NAMED_COLOR2: Tag = Tag(crate::signatures::tag::SigNamedColor2Tag);
    /// `'clrt'`/`'clot'` — colorant tables (named-color profiles).
    pub const COLORANT_TABLE: Tag = Tag(crate::signatures::tag::SigColorantTableTag);
    pub const COLORANT_TABLE_OUT: Tag = Tag(crate::signatures::tag::SigColorantTableOutTag);
    /// `'clro'` — colorant order.
    pub const COLORANT_ORDER: Tag = Tag(crate::signatures::tag::SigColorantOrderTag);
    /// `'chrm'` — chromaticity (xy primaries).
    pub const CHROMATICITY: Tag = Tag(crate::signatures::tag::SigChromaticityTag);
    /// `'A2B0'`/`'A2B1'`/`'A2B2'` — device → PCS pipelines per intent.
    pub const A_TO_B0: Tag = Tag(crate::signatures::tag::SigAToB0Tag);
    pub const A_TO_B1: Tag = Tag(crate::signatures::tag::SigAToB1Tag);
    pub const A_TO_B2: Tag = Tag(crate::signatures::tag::SigAToB2Tag);
    /// `'B2A0'`/`'B2A1'`/`'B2A2'` — PCS → device pipelines per intent.
    pub const B_TO_A0: Tag = Tag(crate::signatures::tag::SigBToA0Tag);
    pub const B_TO_A1: Tag = Tag(crate::signatures::tag::SigBToA1Tag);
    pub const B_TO_A2: Tag = Tag(crate::signatures::tag::SigBToA2Tag);
    /// `'D2B0'`..`'D2B3'`/`'B2D0'`..`'B2D3'` — iccMAX pipeline tags.
    pub const D_TO_B0: Tag = Tag(crate::signatures::tag::SigDToB0Tag);
    pub const D_TO_B1: Tag = Tag(crate::signatures::tag::SigDToB1Tag);
    pub const D_TO_B2: Tag = Tag(crate::signatures::tag::SigDToB2Tag);
    pub const D_TO_B3: Tag = Tag(crate::signatures::tag::SigDToB3Tag);
    pub const B_TO_D0: Tag = Tag(crate::signatures::tag::SigBToD0Tag);
    pub const B_TO_D1: Tag = Tag(crate::signatures::tag::SigBToD1Tag);
    pub const B_TO_D2: Tag = Tag(crate::signatures::tag::SigBToD2Tag);
    pub const B_TO_D3: Tag = Tag(crate::signatures::tag::SigBToD3Tag);
    /// `'pre0'`/`'pre1'`/`'pre2'` — preview pipelines.
    pub const PREVIEW0: Tag = Tag(crate::signatures::tag::SigPreview0Tag);
    pub const PREVIEW1: Tag = Tag(crate::signatures::tag::SigPreview1Tag);
    pub const PREVIEW2: Tag = Tag(crate::signatures::tag::SigPreview2Tag);
    /// `'gamt'` — gamut pipeline.
    pub const GAMUT: Tag = Tag(crate::signatures::tag::SigGamutTag);
    /// `'vcgt'` — video card gamma table.
    pub const VCGT: Tag = Tag(crate::signatures::tag::SigVcgtTag);
    /// `'targ'` — characterization target text.
    pub const CHARD: Tag = Tag(crate::signatures::tag::SigCharTargetTag);
    /// `'scrd'`/`'scrn'` — screening.
    pub const SCREENING_DESC: Tag = Tag(crate::signatures::tag::SigScreeningDescTag);
    pub const SCREENING: Tag = Tag(crate::signatures::tag::SigScreeningTag);
    /// `'bfd '` — UCR/black-generation.
    pub const UCRBG: Tag = Tag(crate::signatures::tag::SigUcrBgTag);
    /// `'meas'` — measurement conditions.
    pub const MEASUREMENT: Tag = Tag(crate::signatures::tag::SigMeasurementTag);
    /// `'view'`/`'vued'` — viewing conditions.
    pub const VIEWING_CONDITIONS: Tag = Tag(crate::signatures::tag::SigViewingConditionsTag);
    pub const VIEWING_COND_DESC: Tag = Tag(crate::signatures::tag::SigViewingCondDescTag);
    /// `'tech'` — technology signature.
    pub const TECHNOLOGY: Tag = Tag(crate::signatures::tag::SigTechnologyTag);
    /// `'calt'`/`'dtim'` — dates.
    pub const CALIBRATION_DATE_TIME: Tag = Tag(crate::signatures::tag::SigCalibrationDateTimeTag);
    /// `'lumi'` — viewing luminance.
    pub const LUMINANCE: Tag = Tag(crate::signatures::tag::SigLuminanceTag);
    /// `'pseq'`/`'psid'` — profile sequence descriptors.
    pub const PROFILE_SEQUENCE_DESC: Tag = Tag(crate::signatures::tag::SigProfileSequenceDescTag);
    pub const PROFILE_SEQUENCE_ID: Tag = Tag(crate::signatures::tag::SigProfileSequenceIdTag);
    /// `'resp'` — output response.
    pub const OUTPUT_RESPONSE: Tag = Tag(crate::signatures::tag::SigOutputResponseTag);
    /// `'ciis'` — colorimetric intent image state.
    pub const COLORIMETRIC_INTENT_IMAGE_STATE: Tag =
        Tag(crate::signatures::tag::SigColorimetricIntentImageStateTag);
    /// `'crdi'` — CRD info.
    pub const CRD_INFO: Tag = Tag(crate::signatures::tag::SigCrdInfoTag);
    /// `'data'` — generic data payload.
    pub const DATA: Tag = Tag(crate::signatures::tag::SigDataTag);
    /// `'cicp'` — video signal (BT.2100 etc.).
    pub const CICP: Tag = Tag(crate::signatures::tag::SigcicpTag);
    /// `'meta'` — iccMAX metadata.
    pub const META: Tag = Tag(crate::signatures::tag::SigMetaTag);
    /// `'ps2s'`/`'psd0'`..`'psd3'`/`'ps2i'` — PostScript tags.
    pub const PS2_CSA: Tag = Tag(crate::signatures::tag::SigPs2CSATag);
    pub const PS2_CRD0: Tag = Tag(crate::signatures::tag::SigPs2CRD0Tag);
    pub const PS2_CRD1: Tag = Tag(crate::signatures::tag::SigPs2CRD1Tag);
    pub const PS2_CRD2: Tag = Tag(crate::signatures::tag::SigPs2CRD2Tag);
    pub const PS2_CRD3: Tag = Tag(crate::signatures::tag::SigPs2CRD3Tag);
    pub const PS2_RENDERING_INTENT: Tag = Tag(crate::signatures::tag::SigPs2RenderingIntentTag);
}
impl fmt::Debug for Tag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Tag({self})")
    }
}
impl fmt::Display for Tag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt_sig4(self.0, f)
    }
}
impl From<Tag> for u32 {
    fn from(t: Tag) -> u32 {
        t.0
    }
}
impl From<u32> for Tag {
    fn from(u: u32) -> Tag {
        Tag(u)
    }
}

// -- Intent / InfoType / Locale -----------------------------------------------

/// Rendering intent — the four ICC intents plus lcms's black-preserving
/// intents for CMYK → CMYK links (`INTENT_PRESERVE_K_*`).
///
/// ```
/// use lcms::api::Intent;
/// assert_eq!(Intent::default(), Intent::Perceptual);
/// assert_eq!(Intent::try_from(13).unwrap(), Intent::PreserveKPlanePerceptual);
/// assert!(Intent::try_from(9).is_err());
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u32)]
#[non_exhaustive]
pub enum Intent {
    /// `INTENT_PERCEPTUAL` (0) — gamut compressed smoothly (the default).
    #[default]
    Perceptual = 0,
    /// `INTENT_RELATIVE_COLORIMETRIC` (1) — clip out-of-gamut, adapt white.
    RelativeColorimetric = 1,
    /// `INTENT_SATURATION` (2) — preserve saturation.
    Saturation = 2,
    /// `INTENT_ABSOLUTE_COLORIMETRIC` (3) — clip, keep paper white.
    AbsoluteColorimetric = 3,
    /// `INTENT_PRESERVE_K_ONLY_PERCEPTUAL` (10).
    PreserveKOnlyPerceptual = 10,
    /// `INTENT_PRESERVE_K_ONLY_RELATIVE_COLORIMETRIC` (11).
    PreserveKOnlyRelativeColorimetric = 11,
    /// `INTENT_PRESERVE_K_ONLY_SATURATION` (12).
    PreserveKOnlySaturation = 12,
    /// `INTENT_PRESERVE_K_PLANE_PERCEPTUAL` (13).
    PreserveKPlanePerceptual = 13,
    /// `INTENT_PRESERVE_K_PLANE_RELATIVE_COLORIMETRIC` (14).
    PreserveKPlaneRelativeColorimetric = 14,
    /// `INTENT_PRESERVE_K_PLANE_SATURATION` (15).
    PreserveKPlaneSaturation = 15,
}
impl Intent {
    /// The numeric intent code.
    pub const fn code(self) -> u32 {
        self as u32
    }
    /// `u32` -> `Intent`, `None` for a code that isn't an lcms intent.
    pub const fn from_u32(v: u32) -> Option<Intent> {
        Some(match v {
            0 => Intent::Perceptual,
            1 => Intent::RelativeColorimetric,
            2 => Intent::Saturation,
            3 => Intent::AbsoluteColorimetric,
            10 => Intent::PreserveKOnlyPerceptual,
            11 => Intent::PreserveKOnlyRelativeColorimetric,
            12 => Intent::PreserveKOnlySaturation,
            13 => Intent::PreserveKPlanePerceptual,
            14 => Intent::PreserveKPlaneRelativeColorimetric,
            15 => Intent::PreserveKPlaneSaturation,
            _ => return None,
        })
    }
    /// The four ICC intents, in code order.
    pub const ALL: [Intent; 4] = [
        Intent::Perceptual,
        Intent::RelativeColorimetric,
        Intent::Saturation,
        Intent::AbsoluteColorimetric,
    ];
}
impl From<Intent> for u32 {
    fn from(i: Intent) -> u32 {
        i.code()
    }
}
impl TryFrom<u32> for Intent {
    type Error = Error;
    fn try_from(v: u32) -> Result<Intent> {
        Intent::from_u32(v)
            .ok_or_else(|| Error::new(types::ERROR_RANGE, format!("unknown rendering intent {v}")))
    }
}

/// Profile info-text selector — which localized text tag to read.
/// (`cmsGetProfileInfo*` / `cmsGetProfileInfoUTF8`.)
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InfoType {
    /// `cmsInfoDescription` — `'desc'`.
    Description,
    /// `cmsInfoManufacturer` — `'dmnd'`.
    Manufacturer,
    /// `cmsInfoModel` — `'dmdd'`.
    Model,
    /// `cmsInfoCopyright` — `'cprt'`.
    Copyright,
}
impl InfoType {
    fn sig(self) -> u32 {
        match self {
            InfoType::Description => crate::signatures::tag::SigProfileDescriptionTag,
            InfoType::Manufacturer => crate::signatures::tag::SigDeviceMfgDescTag,
            InfoType::Model => crate::signatures::tag::SigDeviceModelDescTag,
            InfoType::Copyright => crate::signatures::tag::SigCopyrightTag,
        }
    }
}

/// An MLU language/country pair — `"en_US"`-shaped codes.
///
/// ICC stores each as a 2-char code (C's `cmsLanguageCode`/`cmsCountryCode`
/// are `char[3]` with a NUL). [`Locale::NONE`] selects the default/best
/// translation.
///
/// ```
/// use lcms::api::Locale;
/// let en = Locale::new("en_US");
/// assert_eq!(en.to_string(), "en_US");
/// ```
#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Locale {
    /// 2-char ISO-639 language + NUL (`b"en\0"`).
    pub language: [u8; 3],
    /// 2-char ISO-3166 country + NUL (`b"US\0"`).
    pub country: [u8; 3],
}
impl Locale {
    /// The null/default locale — picks the profile's best translation.
    pub const NONE: Locale = Locale {
        language: [0; 3],
        country: [0; 3],
    };
    /// `Locale::new("en_US")` — `lang` or `lang_COUNTRY`; extra chars are
    /// truncated to the 2-char ICC codes.
    pub fn new(code: &str) -> Locale {
        let (lang, country) = match code.split_once('_') {
            Some((l, c)) => (l, c),
            None => (code, ""),
        };
        let mut l = Locale::NONE;
        for (b, c) in l.language.iter_mut().zip(lang.bytes().take(2)) {
            *b = c;
        }
        for (b, c) in l.country.iter_mut().zip(country.bytes().take(2)) {
            *b = c;
        }
        l
    }
    /// Separate 2-char codes: `Locale::from_parts("en", "US")`.
    pub fn from_parts(lang: &str, country: &str) -> Locale {
        let mut l = Locale::NONE;
        for (b, c) in l.language.iter_mut().zip(lang.bytes().take(2)) {
            *b = c;
        }
        for (b, c) in l.country.iter_mut().zip(country.bytes().take(2)) {
            *b = c;
        }
        l
    }
    /// The `lang` code as a string (`"en"`).
    pub fn language(&self) -> String {
        String::from_utf8_lossy(&self.language[..2]).into_owned()
    }
    /// The `COUNTRY` code as a string (`"US"`).
    pub fn country(&self) -> String {
        String::from_utf8_lossy(&self.country[..2]).into_owned()
    }
    /// `true` for [`Locale::NONE`] (all-zero).
    pub fn is_none(&self) -> bool {
        *self == Locale::NONE
    }
}
impl fmt::Display for Locale {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (l, c) = (self.language(), self.country());
        if c.is_empty() {
            write!(f, "{l}")
        } else {
            write!(f, "{l}_{c}")
        }
    }
}
impl fmt::Debug for Locale {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Locale({self})")
    }
}
impl From<&str> for Locale {
    fn from(s: &str) -> Locale {
        Locale::new(s)
    }
}

// -- ColorSpace / ProfileClass -------------------------------------------------

/// An ICC color-space signature — the `color_space`/`pcs` header fields
/// and [`Format`] pixel-type mapping (`cmsSig*Data`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ColorSpace {
    /// `'XYZ '` — CIE XYZ.
    Xyz,
    /// `'Lab '` — CIE L*a*b*.
    Lab,
    /// `'Luv '` — CIE L*u*v*.
    Luv,
    /// `'YCbr'` — YCbCr.
    YCbCr,
    /// `'Yxy '` — CIE Yxy.
    Yxy,
    /// `'RGB '` — RGB.
    Rgb,
    /// `'GRAY'` — grayscale.
    Gray,
    /// `'HSV '` — HSV.
    Hsv,
    /// `'HLS '` — HLS.
    Hls,
    /// `'CMY '` — CMY.
    Cmy,
    /// `'CMYK'` — CMYK.
    Cmyk,
    /// `'MCHn'` — multichannel, `n` = channel count (1..=15; `'MCHA'`..`'MCHF'` for 10..15).
    Mch(u8),
    /// `'nCLR'` — n-channel spot data (`'1CLR'`..`'FCLR'`, n = 1..=15).
    NColor(u8),
    /// `'nmcl'` — named color.
    Named,
    /// Any other signature — the raw 4-char value.
    Other(u32),
}
impl ColorSpace {
    /// The 4-char signature.
    pub fn code(self) -> u32 {
        use crate::signatures::colorspace as cs;
        match self {
            ColorSpace::Xyz => cs::SigXYZData,
            ColorSpace::Lab => cs::SigLabData,
            ColorSpace::Luv => cs::SigLuvData,
            ColorSpace::YCbCr => cs::SigYCbCrData,
            ColorSpace::Yxy => cs::SigYxyData,
            ColorSpace::Rgb => cs::SigRgbData,
            ColorSpace::Gray => cs::SigGrayData,
            ColorSpace::Hsv => cs::SigHsvData,
            ColorSpace::Hls => cs::SigHlsData,
            ColorSpace::Cmy => cs::SigCmyData,
            ColorSpace::Cmyk => cs::SigCmykData,
            ColorSpace::Mch(n) => {
                let d = match n {
                    1..=9 => b'0' + n,
                    10..=15 => b'A' + n - 10,
                    _ => b'0',
                };
                0x4D43_4800 | d as u32 // 'MCH?'
            }
            ColorSpace::NColor(n) => {
                let d = match n {
                    1..=9 => b'0' + n,
                    10..=15 => b'A' + n - 10,
                    _ => b'0',
                };
                ((d as u32) << 24) | 0x0043_4C52 // '?CLR'
            }
            ColorSpace::Named => cs::SigNamedData,
            ColorSpace::Other(u) => u,
        }
    }
    /// `u32` signature -> `ColorSpace` (`Other` for unmapped values).
    pub fn from_u32(u: u32) -> ColorSpace {
        use crate::signatures::colorspace as cs;
        match u {
            cs::SigXYZData => ColorSpace::Xyz,
            cs::SigLabData => ColorSpace::Lab,
            cs::SigLuvData => ColorSpace::Luv,
            cs::SigYCbCrData => ColorSpace::YCbCr,
            cs::SigYxyData => ColorSpace::Yxy,
            cs::SigRgbData => ColorSpace::Rgb,
            cs::SigGrayData => ColorSpace::Gray,
            cs::SigHsvData => ColorSpace::Hsv,
            cs::SigHlsData => ColorSpace::Hls,
            cs::SigCmyData => ColorSpace::Cmy,
            cs::SigCmykData => ColorSpace::Cmyk,
            cs::SigNamedData => ColorSpace::Named,
            _ => {
                let (b3, b2, b1, b0) = ((u >> 24) as u8, (u >> 16) as u8, (u >> 8) as u8, u as u8);
                if (b3, b2, b1) == (b'M', b'C', b'H') {
                    match b0 {
                        d @ b'1'..=b'9' => return ColorSpace::Mch(d - b'0'),
                        d @ b'A'..=b'F' => return ColorSpace::Mch(d - b'A' + 10),
                        _ => {}
                    }
                }
                if (b2, b1, b0) == (b'C', b'L', b'R') {
                    match b3 {
                        d @ b'1'..=b'9' => return ColorSpace::NColor(d - b'0'),
                        d @ b'A'..=b'F' => return ColorSpace::NColor(d - b'A' + 10),
                        _ => {}
                    }
                }
                ColorSpace::Other(u)
            }
        }
    }
    /// Channel count of the color space (`cmsChannelsOf`).
    pub fn channels(self) -> u32 {
        crate::pcs::channels_of(self.code())
    }
}
impl From<ColorSpace> for u32 {
    fn from(c: ColorSpace) -> u32 {
        c.code()
    }
}
impl From<u32> for ColorSpace {
    fn from(u: u32) -> ColorSpace {
        ColorSpace::from_u32(u)
    }
}
impl fmt::Display for ColorSpace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt_sig4(self.code(), f)
    }
}

/// An ICC profile-class signature — the `device_class` header field
/// (`cmsSig*Class`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ProfileClass {
    /// `'scnr'` — input (scanner/camera).
    Input,
    /// `'mntr'` — display.
    Display,
    /// `'prtr'` — output (printer).
    Output,
    /// `'link'` — device link (device → device, no PCS hop).
    DeviceLink,
    /// `'abst'` — abstract transform.
    Abstract,
    /// `'spac'` — color-space conversion.
    ColorSpace,
    /// `'nmcl'` — named color.
    NamedColor,
    /// Any other signature (iccMAX classes, unknown) — the raw value.
    Other(u32),
}
impl ProfileClass {
    /// The 4-char signature.
    pub fn code(self) -> u32 {
        use crate::signatures::profileclass as pc;
        match self {
            ProfileClass::Input => pc::SigInputClass,
            ProfileClass::Display => pc::SigDisplayClass,
            ProfileClass::Output => pc::SigOutputClass,
            ProfileClass::DeviceLink => pc::SigLinkClass,
            ProfileClass::Abstract => pc::SigAbstractClass,
            ProfileClass::ColorSpace => pc::SigColorSpaceClass,
            ProfileClass::NamedColor => pc::SigNamedColorClass,
            ProfileClass::Other(u) => u,
        }
    }
    /// `u32` signature -> `ProfileClass` (`Other` for unmapped values).
    pub fn from_u32(u: u32) -> ProfileClass {
        use crate::signatures::profileclass as pc;
        match u {
            pc::SigInputClass => ProfileClass::Input,
            pc::SigDisplayClass => ProfileClass::Display,
            pc::SigOutputClass => ProfileClass::Output,
            pc::SigLinkClass => ProfileClass::DeviceLink,
            pc::SigAbstractClass => ProfileClass::Abstract,
            pc::SigColorSpaceClass => ProfileClass::ColorSpace,
            pc::SigNamedColorClass => ProfileClass::NamedColor,
            _ => ProfileClass::Other(u),
        }
    }
}
impl From<ProfileClass> for u32 {
    fn from(c: ProfileClass) -> u32 {
        c.code()
    }
}
impl From<u32> for ProfileClass {
    fn from(u: u32) -> ProfileClass {
        ProfileClass::from_u32(u)
    }
}
impl fmt::Display for ProfileClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt_sig4(self.code(), f)
    }
}

// -- flags ---------------------------------------------------------------------

/// Transform-creation flags — the `cmsFLAGS_*` constants.
///
/// ```
/// use lcms::api::{flags, Plan, Profile, Format, Intent};
/// # let srgb = Profile::srgb();
/// # let lab = Profile::lab4();
/// let plan = Plan::build(
///     &srgb, &lab, Format::RGB_8, Format::LAB_8,
///     Intent::Perceptual, flags::NO_OPTIMIZE | flags::BLACK_POINT_COMPENSATION,
/// )?;
/// # Ok::<(), lcms::api::Error>(())
/// ```
pub mod flags {
    /// No flags.
    pub const NONE: u32 = 0;
    /// `cmsFLAGS_NOCACHE` — skip the pixel-level cache.
    pub const NO_CACHE: u32 = crate::types::FLAGS_NOCACHE;
    /// `cmsFLAGS_NOOPTIMIZE` — don't flatten/optimize the pipeline.
    pub const NO_OPTIMIZE: u32 = crate::types::FLAGS_NOOPTIMIZE;
    /// `cmsFLAGS_NULLTRANSFORM` — copy pixels, no color math.
    pub const NULL_TRANSFORM: u32 = crate::types::FLAGS_NULLTRANSFORM;
    /// `cmsFLAGS_GAMUTCHECK` — enable out-of-gamut alarm (see [`Plan::new_proofing`]).
    ///
    /// [`Plan::new_proofing`]: crate::api::Plan::new_proofing
    pub const GAMUT_CHECK: u32 = crate::types::FLAGS_GAMUTCHECK;
    /// `cmsFLAGS_SOFTPROOFING`.
    pub const SOFT_PROOFING: u32 = crate::types::FLAGS_SOFTPROOFING;
    /// `cmsFLAGS_BLACKPOINTCOMPENSATION`.
    pub const BLACK_POINT_COMPENSATION: u32 = crate::types::FLAGS_BLACKPOINTCOMPENSATION;
    /// `cmsFLAGS_NOWHITEONWHITEFIXUP`.
    pub const NO_WHITE_ON_WHITE_FIXUP: u32 = crate::types::FLAGS_NOWHITEONWHITEFIXUP;
    /// `cmsFLAGS_HIGHRESPRECALC` — more CLUT grid points (slower link, tighter).
    pub const HIGH_RES_PRECALC: u32 = crate::types::FLAGS_HIGHRESPRECALC;
    /// `cmsFLAGS_LOWRESPRECALC` — fewer CLUT grid points.
    pub const LOW_RES_PRECALC: u32 = crate::types::FLAGS_LOWRESPRECALC;
    /// `cmsFLAGS_8BITS_DEVICELINK` — 8-bit CLUTs in devicelinks.
    pub const DEVICELINK_8BITS: u32 = crate::types::FLAGS_8BITS_DEVICELINK;
    /// `cmsFLAGS_GUESSDEVICECLASS`.
    pub const GUESS_DEVICE_CLASS: u32 = crate::types::FLAGS_GUESSDEVICECLASS;
    /// `cmsFLAGS_KEEP_SEQUENCE`.
    pub const KEEP_SEQUENCE: u32 = crate::types::FLAGS_KEEP_SEQUENCE;
    /// `cmsFLAGS_FORCE_CLUT` — always use the full-resolution CLUT path.
    pub const FORCE_CLUT: u32 = crate::types::FLAGS_FORCE_CLUT;
    /// `cmsFLAGS_CLUT_POST_LINEARIZATION`.
    pub const CLUT_POST_LINEARIZATION: u32 = crate::types::FLAGS_CLUT_POST_LINEARIZATION;
    /// `cmsFLAGS_CLUT_PRE_LINEARIZATION`.
    pub const CLUT_PRE_LINEARIZATION: u32 = crate::types::FLAGS_CLUT_PRE_LINEARIZATION;
    /// `cmsFLAGS_NONEGATIVES` — clamp negative float results.
    pub const NO_NEGATIVES: u32 = crate::types::FLAGS_NONEGATIVES;
    /// `cmsFLAGS_COPY_ALPHA` — copy extra channels through unchanged.
    pub const COPY_ALPHA: u32 = crate::types::FLAGS_COPY_ALPHA;
    /// `cmsFLAGS_NODEFAULTRESOURCEDEF`.
    pub const NO_DEFAULT_RESOURCE_DEF: u32 = crate::types::FLAGS_NODEFAULTRESOURCEDEF;
    /// `cmsFLAGS_GRIDPOINTS_MASK` — the bits [`gridpoints`] writes.
    pub const GRIDPOINTS_MASK: u32 = crate::types::FLAGS_GRIDPOINTS_MASK;
    /// `cmsFLAGS_GRIDPOINTS(n)` — override CLUT grid resolution (2..=255).
    pub fn gridpoints(n: u32) -> u32 {
        crate::types::flags_gridpoints(n)
    }
}

// -- Profile ------------------------------------------------------------------

/// An owned ICC profile — `Send` + `Sync`.
///
/// Every constructor **materializes all tags eagerly** (the C library's
/// lazy `cmsReadTag` materialization done up front — the same objects it
/// would produce). That makes `&Profile` reads — [`Profile::get`],
/// [`Profile::info`], [`Profile::named_colors`], [`Profile::tags`] —
/// lock-free and `Sync`-safe: `Arc<Profile>` is shareable as-is. The
/// trade-off: `open`/`open_file` decode every tag at construction, and
/// [`Profile::save`] re-encodes them rather than copying raw blobs.
///
/// [`Profile::freeze`] remains for explicitness — it is a no-op on a
/// profile built by this API.
///
/// ```
/// use lcms::api::*;
/// let p = Profile::srgb();
/// assert_eq!(p.device_class(), ProfileClass::Display);
/// assert_eq!(p.color_space(), ColorSpace::Rgb);
/// assert!(p.get(Tag::RED_TRC).is_some());
/// # Ok::<(), lcms::api::Error>(())
/// ```
pub struct Profile {
    pub(crate) icc: Box<crate::profile::IccProfile>,
}

impl Clone for Profile {
    fn clone(&self) -> Self {
        Profile {
            icc: self.icc.clone(),
        }
    }
}

impl fmt::Debug for Profile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Profile")
            .field("device_class", &self.device_class())
            .field("color_space", &self.color_space())
            .field("pcs", &self.pcs())
            .field("version", &format_args!("0x{:x}", self.icc.version))
            .field("description", &self.info(InfoType::Description))
            .finish()
    }
}

impl Profile {
    /// Freeze a freshly built/parsed profile — `&Profile` reads are then
    /// always valid.
    fn frozen(mut icc: Box<crate::profile::IccProfile>) -> Profile {
        profile::freeze(&mut icc);
        // a tag that fails to decode stays unmaterialized (reads `None`);
        // its signalled error isn't the constructor's failure.
        let _ = crate::err::take_last_error();
        Profile { icc }
    }
    fn wrap(
        what: &str,
        f: impl FnOnce() -> Option<Box<crate::profile::IccProfile>>,
    ) -> Result<Self> {
        attempt(types::ERROR_READ, what, f).map(Profile::frozen)
    }
    /// A built-in profile — its construction has no failure path for these
    /// fixed inputs, so a failure is a bug in the port.
    fn builtin(what: &str, f: impl FnOnce() -> Option<Box<crate::profile::IccProfile>>) -> Self {
        Self::wrap(what, f).unwrap_or_else(|e| panic!("built-in profile {what}: {e}"))
    }

    // -- constructors -----------------------------------------------------

    /// `cmsCreate_sRGBProfile` — IEC 61966-2.1 sRGB ([`D65`] white,
    /// [`REC709_PRIMARIES`], the sRGB curve), ICC v4.4.
    pub fn srgb() -> Self {
        Self::builtin("create_srgb_profile", || virt::create_srgb_profile(None))
    }
    /// `cmsCreateLab4Profile(NULL)` — Lab (D50) identity, ICC v4 encoding.
    pub fn lab4() -> Self {
        Self::builtin("create_lab4_profile", || {
            virt::create_lab4_profile(None, None)
        })
    }
    /// `cmsCreateLab2Profile(NULL)` — Lab (D50) identity, ICC v2 encoding.
    pub fn lab2() -> Self {
        Self::builtin("create_lab2_profile", || {
            virt::create_lab2_profile(None, None)
        })
    }
    /// `cmsCreateLab4Profile` with a custom white point.
    pub fn lab4_with_white_point(white_point: &CiexyY) -> Result<Self> {
        Self::wrap("create_lab4_profile", || {
            virt::create_lab4_profile(None, Some(white_point))
        })
    }
    /// `cmsCreateGrayProfile` with the [`D50`] white and a 2.2-gamma TRC —
    /// see [`Profile::gray_curve`] for a custom curve.
    pub fn gray() -> Self {
        let g = ToneCurve::gamma(2.2).expect("gamma 2.2 is valid");
        Self::builtin("create_gray_profile", || {
            virt::create_gray_profile(None, Some(&D50), Some(&g))
        })
    }
    /// `cmsCreateXYZProfile` — XYZ identity.
    pub fn xyz() -> Self {
        Self::builtin("create_xyz_profile", || virt::create_xyz_profile(None))
    }
    /// `cmsCreateNULLProfile` — an output profile mapping every color to
    /// gray 0 (black); useful as a gamut-check sink.
    pub fn null() -> Self {
        Self::builtin("create_null_profile", || virt::create_null_profile(None))
    }
    /// `cmsCreate_OkLabProfile` — OkLab color-space profile (MPE pipeline;
    /// cannot be saved to an ICC file, like C).
    pub fn oklab() -> Self {
        Self::builtin("create_oklab_profile", || virt::create_oklab_profile(None))
    }
    /// `cmsCreateProfilePlaceholder` — a bare profile shell for authoring
    /// (display class, `'XYZ '` PCS, no color space or tags yet; set them
    /// with [`Profile::set_color_space`], [`Profile::write_tag`], ...).
    pub fn placeholder() -> Self {
        Profile::frozen(crate::profile::create_profile_placeholder(None))
    }
    /// `cmsCreateRGBProfile` — white point, primaries, transfer curves.
    ///
    /// ```
    /// use lcms::api::*;
    /// // sRGB rebuilt from its parts matches the built-in profile exactly
    /// let trc = ToneCurve::srgb();
    /// let p = Profile::rgb(&D65, &REC709_PRIMARIES, [&trc, &trc, &trc])?;
    /// assert_eq!(p.get(Tag::RED_COLORANT).and_then(|v| v.as_xyz()),
    ///            Profile::srgb().get(Tag::RED_COLORANT).and_then(|v| v.as_xyz()));
    ///
    /// // Display P3: P3 primaries, D65 white, sRGB curve
    /// let p3 = CiexyYTriple {
    ///     Red:   CiexyY { x: 0.680, y: 0.320, Y: 1.0 },
    ///     Green: CiexyY { x: 0.265, y: 0.690, Y: 1.0 },
    ///     Blue:  CiexyY { x: 0.150, y: 0.060, Y: 1.0 },
    /// };
    /// let display_p3 = Profile::rgb(&D65, &p3, [&trc, &trc, &trc])?;
    /// # Ok::<(), lcms::api::Error>(())
    /// ```
    pub fn rgb(
        white_point: &CiexyY,
        primaries: &CiexyYTriple,
        curves: [&ToneCurve; 3],
    ) -> Result<Self> {
        Self::wrap("create_rgb_profile", || {
            virt::create_rgb_profile(None, Some(white_point), Some(primaries), Some(curves))
        })
    }
    /// `cmsCreateGrayProfile` — white point + a custom tone curve.
    pub fn gray_curve(white_point: &CiexyY, curve: &ToneCurve) -> Result<Self> {
        Self::wrap("create_gray_profile", || {
            virt::create_gray_profile(None, Some(white_point), Some(curve))
        })
    }
    /// `cmsCreateLinearizationDeviceLink` — a device-link profile whose
    /// `A2B0` pipeline is one tone-curve stage per channel.
    pub fn linearization_device_link(
        color_space: ColorSpace,
        curves: &[ToneCurve],
    ) -> Result<Self> {
        let refs: Vec<&ToneCurve> = curves.iter().collect();
        Self::wrap("create_linearization_device_link", || {
            virt::create_linearization_device_link(None, color_space.code(), &refs)
        })
    }
    /// `cmsCreateInkLimitingDeviceLink` — a CMYK→CMYK device link that
    /// caps total ink coverage (`limit` %, e.g. `250.0`).
    pub fn ink_limiting_device_link(color_space: ColorSpace, limit: f64) -> Result<Self> {
        Self::wrap("create_ink_limiting_device_link", || {
            virt::create_ink_limiting_device_link(None, color_space.code(), limit)
        })
    }
    /// `cmsCreateBCHSWabstractProfile` — a Lab→Lab abstract profile applying
    /// brightness/contrast/hue/saturation/temperature adjustments.
    ///
    /// `temp_src`/`temp_dest` are white-point temperatures in K; equal
    /// values skip the white-point move.
    #[allow(clippy::too_many_arguments)]
    pub fn bchsw_abstract(
        lut_points: u32,
        brightness: f64,
        contrast: f64,
        hue: f64,
        saturation: f64,
        temp_src: u32,
        temp_dest: u32,
    ) -> Result<Self> {
        Self::wrap("create_bchsw_abstract_profile", || {
            virt::create_bchsw_abstract_profile(
                None, lut_points, brightness, contrast, hue, saturation, temp_src, temp_dest,
            )
        })
    }
    /// `cmsOpenProfileFromMem` — parse an ICC profile from bytes.
    /// Materializes every tag eagerly (see the type docs).
    pub fn open(data: &[u8]) -> Result<Self> {
        Self::wrap("open_profile_from_mem", || {
            profile::open_profile_from_mem(None, data)
        })
    }
    /// `cmsOpenProfileFromFile` — read an ICC file off disk.
    pub fn open_file(path: impl AsRef<std::path::Path>) -> Result<Self> {
        let data = std::fs::read(path.as_ref())
            .map_err(|e| Error::new(types::ERROR_FILE, e.to_string()))?;
        Self::open(&data)
    }

    // -- serialization ------------------------------------------------------

    /// `cmsSaveProfileToMem` — the serialized ICC bytes. `&self`: the
    /// profile is saved unchanged (the encoder works on a clone).
    pub fn save(&self) -> Result<Vec<u8>> {
        let mut icc = self.icc.clone();
        attempt(types::ERROR_WRITE, "save_profile_to_mem", || {
            profile::save_profile_to_mem(&mut icc)
        })
    }
    /// `cmsSaveProfileToFile` — write the serialized ICC bytes to disk.
    pub fn save_file(&self, path: impl AsRef<std::path::Path>) -> Result<()> {
        let bytes = self.save()?;
        std::fs::write(path.as_ref(), bytes)
            .map_err(|e| Error::new(types::ERROR_WRITE, e.to_string()))
    }
    /// Eagerly materialize all tags — a no-op on profiles built by this
    /// API (constructors freeze already); kept for explicitness.
    pub fn freeze(&mut self) {
        profile::freeze(&mut self.icc);
    }

    // -- tag access -----------------------------------------------------------

    /// Read a tag — a borrowed [`TagValue`] view into the profile's
    /// stored object (`Some` only when the tag exists and decoded).
    ///
    /// Takes `Tag` or a raw `u32` signature: `p.get("wtpt"-ish
    /// Tag::MEDIA_WHITE_POINT)` or `p.get(0x77747074)`.
    pub fn get(&self, tag: impl Into<Tag>) -> Option<TagValue<'_>> {
        let obj = profile::read_tag_frozen(&self.icc, tag.into().0)?;
        Some(TagValue::of(obj))
    }
    /// The media white point, decoded — shorthand for
    /// `get(Tag::MEDIA_WHITE_POINT)` as XYZ.
    pub fn white_point(&self) -> Option<&CieXyz> {
        match self.get(Tag::MEDIA_WHITE_POINT) {
            Some(TagValue::Xyz(x)) => Some(x),
            _ => None,
        }
    }
    /// Iterate the tag signatures present in the directory.
    pub fn tags(&self) -> impl Iterator<Item = Tag> + '_ {
        self.icc.tag_names[..self.icc.tag_count as usize]
            .iter()
            .filter(|&&s| s != 0)
            .map(|&s| Tag(s))
    }
    /// Number of tags in the directory.
    pub fn tag_count(&self) -> usize {
        self.icc.tag_count as usize
    }
    /// `cmsWriteTag` — store `value` under `tag`, replacing any previous
    /// value. Takes a [`TagData`] or anything convertible into one
    /// (`&str`, [`ToneCurve`], [`CieXyz`], ...). Fails with
    /// [`ErrorKind::UnknownExtension`] when the tag doesn't accept that
    /// type (e.g. text for `'rTRC'`). As with C's `cmsWriteTag`, a failed
    /// write has already released any previous value: the tag is then
    /// absent.
    pub fn write_tag<'a>(
        &mut self,
        tag: impl Into<Tag>,
        value: impl Into<TagData<'a>>,
    ) -> Result<()> {
        let obj = match value.into() {
            TagData::Xyz(x) => TagObject::Xyz(x),
            TagData::Chromaticity(c) => TagObject::Chromaticity(c),
            TagData::ColorantOrder(o) => TagObject::ColorantOrder(o),
            TagData::Curve(c) => TagObject::Curve(c),
            TagData::ParametricCurve(c) => TagObject::ParametricCurve(c),
            TagData::Text(s) => {
                let mut m = Mlu::new();
                m.set(Locale::NONE, s)?;
                TagObject::Mlu(m)
            }
            TagData::Mlu(m) => TagObject::Mlu(m),
            TagData::NamedColors(l) => TagObject::NamedColor(l),
            TagData::Pipeline(p) => TagObject::Lut16(p),
            TagData::Data(d) => TagObject::Data(d),
            TagData::Signature(s) => TagObject::Signature(s),
            TagData::DateTime(d) => TagObject::DateTime(tm_from(d)),
            TagData::Measurement(m) => TagObject::Measurement(m),
            TagData::ViewingConditions(v) => TagObject::ViewingConditions(v),
            TagData::UcrBg(u) => TagObject::UcrBg(u),
            TagData::Screening(s) => {
                if s.n_channels as usize > s.channels.len() {
                    return Err(Error::new(
                        types::ERROR_RANGE,
                        format!(
                            "screening has {} channels; at most {} fit",
                            s.n_channels,
                            s.channels.len()
                        ),
                    ));
                }
                TagObject::Screening(s)
            }
            TagData::VideoSignal(v) => TagObject::VideoSignal(v),
            TagData::Mhc2(m) => {
                let n = m.curve_entries as usize;
                if [&m.red_curve, &m.green_curve, &m.blue_curve]
                    .iter()
                    .any(|c| c.len() != n)
                {
                    return Err(Error::new(
                        types::ERROR_RANGE,
                        format!(
                            "MHC2 curve_entries is {n} but the curves hold {}/{}/{} entries",
                            m.red_curve.len(),
                            m.green_curve.len(),
                            m.blue_curve.len()
                        ),
                    ));
                }
                TagObject::Mhc2(m)
            }
            TagData::VcgtCurves(c) => TagObject::Vcgt(VcgtData::Curves(Box::new(c))),
            TagData::VcgtTable {
                channels,
                elems,
                bytes,
                data,
            } => TagObject::Vcgt(VcgtData::Table {
                channels,
                elems,
                bytes,
                data,
            }),
            TagData::Seq(s) => TagObject::ProfileSeqDesc(s),
            TagData::Dict(d) => TagObject::Dictionary(d),
            TagData::UInt8(v) => TagObject::UInt8(v),
            TagData::UInt32(v) => TagObject::UInt32(v),
            TagData::UInt64(v) => TagObject::UInt64(v),
            TagData::S15Fixed16(v) => TagObject::S15Fixed16(v),
            TagData::U16Fixed16(v) => TagObject::U16Fixed16(v),
        };
        let sig = tag.into().0;
        let r = attempt_bool(types::ERROR_UNKNOWN_EXTENSION, "write_tag", || {
            crate::profile::cms_write_tag(&mut self.icc, sig, Some(obj))
        });
        if r.is_err() {
            // C leaves the directory slot named but empty; drop it so
            // `has_tag` and `get` agree.
            crate::profile::cms_write_tag(&mut self.icc, sig, None);
        }
        r
    }
    /// Remove a tag from the directory. Returns whether it was present.
    pub fn remove_tag(&mut self, tag: impl Into<Tag>) -> bool {
        crate::profile::cms_write_tag(&mut self.icc, tag.into().0, None)
    }
    /// `cmsIsTag` — a directory entry exists (whether or not it decoded).
    pub fn has_tag(&self, tag: impl Into<Tag>) -> bool {
        crate::profile::cms_is_tag(&self.icc, tag.into().0)
    }
    /// The named-color palette (`'ncl2'`/`'clrt'`), if present.
    pub fn named_colors(&self) -> Option<&NamedColorList> {
        for sig in [
            crate::signatures::tag::SigNamedColor2Tag,
            crate::signatures::tag::SigColorantTableTag,
        ] {
            match profile::read_tag_frozen(&self.icc, sig) {
                Some(TagObject::NamedColor(l)) | Some(TagObject::ColorantTable(l)) => {
                    return Some(l);
                }
                _ => {}
            }
        }
        None
    }

    // -- info text -------------------------------------------------------------

    /// Localized profile text — description/copyright/manufacturer/model,
    /// best available translation (`cmsGetProfileInfoUTF8` with the
    /// default locale). Use [`Profile::info_in`] for a specific locale.
    ///
    /// ```
    /// use lcms::api::*;
    /// let p = Profile::srgb();
    /// assert_eq!(p.info(InfoType::Description).as_deref(), Some("sRGB built-in"));
    /// # Ok::<(), lcms::api::Error>(())
    /// ```
    pub fn info(&self, kind: InfoType) -> Option<String> {
        self.info_in(kind, Locale::NONE)
    }
    /// [`Profile::info`] for a specific locale (`"en_US"`, `"de_DE"`, ...).
    /// Falls back per MLU rules when the locale has no translation.
    pub fn info_in(&self, kind: InfoType, locale: Locale) -> Option<String> {
        let obj = profile::read_tag_frozen(&self.icc, kind.sig())?;
        match obj {
            TagObject::Mlu(m)
            | TagObject::Text(m)
            | TagObject::TextDescription(m)
            | TagObject::CrdInfo(m) => m.get(locale),
            _ => None,
        }
    }
    /// Write an info-text tag (`TagData::Text` under the default locale).
    pub fn set_info(&mut self, kind: InfoType, text: &str) -> Result<()> {
        self.write_tag(kind.sig(), TagData::Text(text))
    }

    // -- header ----------------------------------------------------------------

    /// `cmsGetDeviceClass` — the profile class (`'mntr'`, `'prtr'`, ...).
    pub fn device_class(&self) -> ProfileClass {
        ProfileClass::from_u32(self.icc.device_class)
    }
    /// `cmsGetColorSpace` — the device color space (`'RGB '`, `'CMYK'`, ...).
    pub fn color_space(&self) -> ColorSpace {
        ColorSpace::from_u32(self.icc.color_space)
    }
    /// `cmsGetPCS` — the profile connection space (`'Lab '`/`'XYZ '`).
    pub fn pcs(&self) -> ColorSpace {
        ColorSpace::from_u32(self.icc.pcs)
    }
    /// `cmsGetEncodedICCversion` — e.g. `0x04300000` for v4.3.
    pub fn version(&self) -> u32 {
        self.icc.version
    }
    /// `cmsGetHeaderFlags`.
    pub fn header_flags(&self) -> u32 {
        self.icc.flags
    }
    /// `cmsGetHeaderAttributes`.
    pub fn header_attributes(&self) -> u64 {
        self.icc.attributes
    }
    /// `cmsGetHeaderCreator` — the CMM signature.
    pub fn header_creator(&self) -> u32 {
        self.icc.creator
    }
    /// `cmsGetHeaderManufacturer`/`cmsGetHeaderModel` — 4-char sigs.
    pub fn header_manufacturer(&self) -> u32 {
        self.icc.manufacturer
    }
    /// `cmsGetHeaderModel`.
    pub fn header_model(&self) -> u32 {
        self.icc.model
    }
    /// `cmsGetHeaderRenderingIntent` — `None` when the header holds a
    /// code that isn't an lcms intent.
    pub fn header_intent(&self) -> Option<Intent> {
        Intent::from_u32(self.icc.rendering_intent)
    }
    /// `cmsGetHeaderProfileID` — the 16-byte MD5 profile id.
    pub fn profile_id(&self) -> ProfileId {
        self.icc.profile_id
    }
    /// `cmsGetHeaderCreationDateTime` — the header creation date.
    pub fn creation_date(&self) -> DateTimeNumber {
        self.icc.created
    }
    /// `cmsGetHeaderPlatform` — the platform signature (`'APPL'`, `'MSFT'`, 0).
    pub fn platform(&self) -> u32 {
        self.icc.platform
    }

    // -- header setters (authoring) ----------------------------------------------

    /// `cmsSetColorSpace`.
    pub fn set_color_space(&mut self, cs: ColorSpace) {
        self.icc.color_space = cs.code();
    }
    /// `cmsSetDeviceClass`.
    pub fn set_device_class(&mut self, dc: ProfileClass) {
        self.icc.device_class = dc.code();
    }
    /// `cmsSetHeaderRenderingIntent`.
    pub fn set_header_intent(&mut self, intent: Intent) {
        self.icc.rendering_intent = intent.code();
    }
    /// `cmsSetHeaderFlags`.
    pub fn set_header_flags(&mut self, flags: u32) {
        self.icc.flags = flags;
    }
    /// `cmsSetHeaderManufacturer`/`cmsSetHeaderModel` — raw sigs.
    pub fn set_header_manufacturer(&mut self, mfg: u32) {
        self.icc.manufacturer = mfg;
    }
    /// `cmsSetHeaderModel`.
    pub fn set_header_model(&mut self, model: u32) {
        self.icc.model = model;
    }
    /// `cmsSetHeaderAttributes` — bit `i` of the u64 = attribute `i`.
    pub fn set_header_attributes(&mut self, attr: u64) {
        self.icc.attributes = attr;
    }
    /// `cmsSetPCS`.
    pub fn set_pcs(&mut self, pcs: ColorSpace) {
        self.icc.pcs = pcs.code();
    }

    /// Raw `&IccProfile` for the C-parity surface (`little-cms-api` only).
    #[cfg(feature = "little-cms-api")]
    pub fn icc(&self) -> &crate::profile::IccProfile {
        &self.icc
    }
    /// Raw `&mut IccProfile` — for plan building (lazy-materializes).
    #[cfg(feature = "little-cms-api")]
    pub fn icc_mut(&mut self) -> &mut crate::profile::IccProfile {
        &mut self.icc
    }
}

// -- Plan ----------------------------------------------------------------------

/// The immutable linked transform — `Sync`, `Arc`-shareable.
///
/// `Plan` is what `cmsCreateTransform` produces *after* linking and
/// optimizing: the flattened pipeline (CLUT + matrices + curves) plus the
/// format descriptors. Building it is the expensive step; running it is
/// cheap. The only mutable state during evaluation is [`Scratch`]
/// (~64 B of pixel cache), passed explicitly — so `&Plan` is `Sync` and a
/// shared `Arc<Plan>` serves every worker.
///
/// `&Profile` borrows are enough to build: the plan deep-clones the
/// profile headers/tags it needs, so profiles stay shareable across many
/// plans (a display→sRGB plan and a display→CMYK plan share the input
/// `Arc<Profile>`).
///
/// ```
/// use lcms::api::*;
/// let srgb = Profile::srgb();
/// let lab = Profile::lab4();
/// let plan = Plan::new(&srgb, &lab, Format::RGB_8, Format::LAB_8)?;
/// let mut sc = plan.scratch();
/// let mut out = [0u8; 3];
/// plan.apply(&mut sc, &[255, 0, 0], &mut out);
/// assert!(out[0] > 0); // red -> L* > 0
/// # Ok::<(), lcms::api::Error>(())
/// ```
pub struct Plan {
    pub(crate) inner: xform::TransformPlan,
}

impl fmt::Debug for Plan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Plan")
            .field("input_format", &Format::from_u32(self.inner.input_format))
            .field("output_format", &Format::from_u32(self.inner.output_format))
            .field("intent", &self.intent())
            .field("input_color_space", &self.input_color_space())
            .field("output_color_space", &self.output_color_space())
            .finish()
    }
}

/// Per-run pixel scratch (~64 B) — the warm transform cache.
///
/// `plan.scratch()` clones the seeded cache; pool it `thread_local` for
/// hot paths. `Send` + `Sync`, but it's the *mutable* state — one per
/// concurrent `apply`.
#[derive(Clone)]
pub struct Scratch(pub(crate) xform::TransformScratch);
impl fmt::Debug for Scratch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Scratch").finish_non_exhaustive()
    }
}

/// How [`Plan::with_options`] links profiles: rendering intent, `cmsFLAGS_*`
/// (black-point compensation is [`flags::BLACK_POINT_COMPENSATION`]) and
/// the chromatic adaptation state. The default is
/// `cmsCreateTransform(.., INTENT_PERCEPTUAL, 0)`.
///
/// ```
/// use lcms::api::*;
/// let opts = PlanOptions::new()
///     .intent(Intent::RelativeColorimetric)
///     .black_point_compensation(true);
/// let plan = Plan::with_options(
///     &Profile::srgb(), &Profile::lab4(), Format::RGB_8, Format::LAB_DBL, &opts,
/// )?;
/// assert_eq!(plan.intent(), Some(Intent::RelativeColorimetric));
/// assert_ne!(plan.flags() & flags::BLACK_POINT_COMPENSATION, 0);
/// # Ok::<(), lcms::api::Error>(())
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PlanOptions {
    intent: Intent,
    flags: u32,
    adaptation_state: Option<f64>,
}

impl PlanOptions {
    /// Perceptual intent, no flags, the global adaptation state.
    pub const fn new() -> PlanOptions {
        PlanOptions {
            intent: Intent::Perceptual,
            flags: 0,
            adaptation_state: None,
        }
    }
    /// Set the rendering intent.
    pub const fn intent(mut self, intent: Intent) -> PlanOptions {
        self.intent = intent;
        self
    }
    /// Replace the `cmsFLAGS_*` bits ([`flags`]).
    pub const fn flags(mut self, flags: u32) -> PlanOptions {
        self.flags = flags;
        self
    }
    /// Set or clear [`flags::BLACK_POINT_COMPENSATION`].
    pub const fn black_point_compensation(mut self, on: bool) -> PlanOptions {
        let bit = flags::BLACK_POINT_COMPENSATION;
        self.flags = if on {
            self.flags | bit
        } else {
            self.flags & !bit
        };
        self
    }
    /// Chromatic adaptation for absolute colorimetric: `1.0` = full
    /// adaptation, `0.0` = none. Unset (the default) uses the global
    /// [`adaptation_state`] (`1.0` unless changed), as C's
    /// `cmsCreateTransform` does.
    pub const fn adaptation_state(mut self, state: f64) -> PlanOptions {
        self.adaptation_state = Some(state);
        self
    }
    fn bpc(&self) -> bool {
        self.flags & flags::BLACK_POINT_COMPENSATION != 0
    }
    fn adaptation(&self) -> f64 {
        self.adaptation_state.unwrap_or_else(adaptation_state)
    }
}

impl From<Intent> for PlanOptions {
    fn from(intent: Intent) -> PlanOptions {
        PlanOptions::new().intent(intent)
    }
}

impl Plan {
    /// `cmsCreateTransform` — link `src -> dst`, perceptual intent, no
    /// flags. For intent/flags see [`Plan::build`] or [`Plan::with_options`].
    ///
    /// Fails with [`ErrorKind::ColorSpaceCheck`] if a format doesn't match
    /// its profile's color space.
    pub fn new(src: &Profile, dst: &Profile, fmt_in: Format, fmt_out: Format) -> Result<Self> {
        Self::with_options(src, dst, fmt_in, fmt_out, &PlanOptions::new())
    }

    /// `cmsCreateTransform` with an intent and `cmsFLAGS_*` ([`flags`]).
    pub fn build(
        src: &Profile,
        dst: &Profile,
        fmt_in: Format,
        fmt_out: Format,
        intent: Intent,
        flags: u32,
    ) -> Result<Self> {
        Self::with_options(
            src,
            dst,
            fmt_in,
            fmt_out,
            &PlanOptions::new().intent(intent).flags(flags),
        )
    }

    /// `cmsCreateTransform` with [`PlanOptions`] — the same link C builds:
    /// black-point compensation from the flags, the adaptation state from
    /// the options or the global setting.
    ///
    /// The profiles are borrowed: linking works on copies, so a shared
    /// `Arc<Profile>` can build any number of plans.
    pub fn with_options(
        src: &Profile,
        dst: &Profile,
        fmt_in: Format,
        fmt_out: Format,
        options: &PlanOptions,
    ) -> Result<Self> {
        Self::link(&[src, dst], None, fmt_in, fmt_out, options, None)
    }

    /// `cmsCreateMultiprofileTransform` — a chain of profiles (`input ->
    /// [abstract/intermediate] -> output`); a single device link or
    /// abstract profile is a valid one-element chain.
    pub fn new_multiprofile(
        profiles: &[&Profile],
        fmt_in: Format,
        fmt_out: Format,
        intent: Intent,
        flags: u32,
    ) -> Result<Self> {
        Self::multiprofile_with_options(
            profiles,
            fmt_in,
            fmt_out,
            &PlanOptions::new().intent(intent).flags(flags),
        )
    }

    /// [`Plan::new_multiprofile`] with [`PlanOptions`].
    pub fn multiprofile_with_options(
        profiles: &[&Profile],
        fmt_in: Format,
        fmt_out: Format,
        options: &PlanOptions,
    ) -> Result<Self> {
        if profiles.is_empty() || profiles.len() > 255 {
            return Err(Error::new(
                types::ERROR_RANGE,
                format!(
                    "a profile chain needs 1..=255 profiles, got {}",
                    profiles.len()
                ),
            ));
        }
        Self::link(profiles, None, fmt_in, fmt_out, options, None)
    }

    /// `cmsCreateProofingTransform` — `input -> output` while simulating
    /// the `proofing` device. With [`flags::SOFT_PROOFING`] the output
    /// shows what `proofing` would reproduce (via `proofing_intent`); with
    /// [`flags::GAMUT_CHECK`] colors it can't reproduce come out as the
    /// alarm codes ([`set_alarm_codes`]). Without either flag this is a
    /// plain [`Plan::build`], exactly as in C.
    #[allow(clippy::too_many_arguments)]
    pub fn new_proofing(
        input: &Profile,
        output: &Profile,
        proofing: &Profile,
        fmt_in: Format,
        fmt_out: Format,
        intent: Intent,
        proofing_intent: Intent,
        flags: u32,
    ) -> Result<Self> {
        if flags & (flags::SOFT_PROOFING | flags::GAMUT_CHECK) == 0 {
            return Self::build(input, output, fmt_in, fmt_out, intent, flags);
        }
        let opts = PlanOptions::new().intent(intent).flags(flags);
        // C: {Input, Proofing, Proofing, Output}, intents {nIntent, nIntent,
        // RELATIVE_COLORIMETRIC, ProofingIntent}, BPC {DoBPC, DoBPC, 0, 0},
        // gamut-check profile = Proofing at chain position 1.
        let bpc = opts.bpc();
        let per_profile = (
            vec![
                intent.code(),
                intent.code(),
                Intent::RelativeColorimetric.code(),
                proofing_intent.code(),
            ],
            vec![bpc, bpc, false, false],
        );
        Self::link(
            &[input, proofing, proofing, output],
            Some(proofing),
            fmt_in,
            fmt_out,
            &opts,
            Some(per_profile),
        )
    }

    /// The single linking path (`cmsCreateExtendedTransform`). Without
    /// `per_profile`, every profile gets the options' intent and BPC —
    /// `cmsCreateMultiprofileTransform`'s layout.
    fn link(
        profiles: &[&Profile],
        gamut: Option<&Profile>,
        fmt_in: Format,
        fmt_out: Format,
        options: &PlanOptions,
        per_profile: Option<(Vec<u32>, Vec<bool>)>,
    ) -> Result<Self> {
        // create_extended_transform takes &mut [Box<IccProfile>] — work on
        // clones so `&Profile` suffices.
        let mut profs: Vec<Box<crate::profile::IccProfile>> =
            profiles.iter().map(|p| p.icc.clone()).collect();
        let mut gamut = gamut.map(|g| g.icc.clone());
        let n = profs.len();
        let (intents, mut bpc) =
            per_profile.unwrap_or_else(|| (vec![options.intent.code(); n], vec![options.bpc(); n]));
        let states = vec![options.adaptation(); n];
        let gamut_pcs_position = if gamut.is_some() { 1 } else { 0 };
        let t = attempt(types::ERROR_NOT_SUITABLE, "create_transform", || {
            xform::create_extended_transform(
                None,
                &mut profs,
                &mut bpc,
                &intents,
                &states,
                gamut.as_deref_mut(),
                gamut_pcs_position,
                fmt_in.code(),
                fmt_out.code(),
                options.flags,
            )
        })?;
        let xform::Transform { plan, .. } = *t;
        Ok(Plan { inner: plan })
    }

    /// `cmsTransform2DeviceLink` — turn this plan back into a `link`-class
    /// ICC profile (consumes the plan). `version` is the target ICC
    /// version as a float (e.g. `4.3`).
    pub fn to_device_link(self, version: f64) -> Result<Profile> {
        self.to_device_link_flags(version, 0)
    }
    /// [`Plan::to_device_link`] with flags (`flags::DEVICELINK_8BITS`
    /// writes 8-bit CLUTs).
    pub fn to_device_link_flags(self, version: f64, flags: u32) -> Result<Profile> {
        let scratch = self.inner.scratch();
        let mut xf = xform::Transform {
            plan: self.inner,
            scratch,
        };
        attempt(types::ERROR_WRITE, "transform2_device_link", || {
            virt::transform2_device_link(&mut xf, version, flags)
        })
        .map(Profile::frozen)
    }

    // -- accessors --------------------------------------------------------------

    /// The input pixel format.
    pub fn input_format(&self) -> Format {
        Format::from_u32(self.inner.input_format)
    }
    /// The output pixel format.
    pub fn output_format(&self) -> Format {
        Format::from_u32(self.inner.output_format)
    }
    /// The rendering intent the plan was linked with (`None` for a
    /// plugin-defined intent code).
    pub fn intent(&self) -> Option<Intent> {
        Intent::from_u32(self.inner.rendering_intent)
    }
    /// The `cmsFLAGS_*` the plan was built with (including any lcms
    /// added, e.g. `NO_CACHE` for float formats).
    pub fn flags(&self) -> u32 {
        self.inner.dw_flags
    }
    /// The entry color space (`cmsGetTransformInputColorSpace`).
    pub fn input_color_space(&self) -> ColorSpace {
        ColorSpace::from_u32(self.inner.entry_color_space)
    }
    /// The exit color space (`cmsGetTransformOutputColorSpace`).
    pub fn output_color_space(&self) -> ColorSpace {
        ColorSpace::from_u32(self.inner.exit_color_space)
    }
    /// `cmsGetTransformInputColorantTable` — the input profile's named
    /// colorant table, if it had one.
    pub fn input_colorants(&self) -> Option<&NamedColorList> {
        self.inner.input_colorants.as_ref()
    }
    /// `cmsGetTransformOutputColorantTable`.
    pub fn output_colorants(&self) -> Option<&NamedColorList> {
        self.inner.output_colorants.as_ref()
    }

    // -- evaluation ---------------------------------------------------------------

    /// Fresh per-run scratch — clones the seeded cache (~64 B).
    pub fn scratch(&self) -> Scratch {
        Scratch(self.inner.scratch())
    }

    /// Pixel count for packed buffers — panics on a size mismatch.
    fn pixel_count(&self, in_len: usize, out_len: usize) -> u32 {
        let (fi, fo) = (self.input_format(), self.output_format());
        let (pi, po) = (fi.pixel_size(), fo.pixel_size());
        assert!(
            pi > 0 && po > 0,
            "plan formats {fi:?} -> {fo:?} have zero-size pixels"
        );
        assert!(
            in_len % pi == 0,
            "input is {in_len} bytes, not a whole number of {pi}-byte {fi:?} pixels"
        );
        let n = in_len / pi;
        assert!(
            out_len >= n * po,
            "output holds {out_len} bytes; {n} {fo:?} pixels need {}",
            n * po
        );
        u32::try_from(n).expect("more than u32::MAX pixels in one call")
    }

    /// `cmsDoTransform` on packed pixels: `input.len() / pixel_size` pixels
    /// from `input` into the front of `output`. `&self` — share one `Plan`
    /// across threads, with a [`Scratch`] per thread.
    ///
    /// Planar formats use C's layout: each plane `n` samples long, planes
    /// back to back.
    ///
    /// # Panics
    /// If `input` isn't a whole number of input pixels, or `output` is
    /// smaller than that many output pixels.
    pub fn apply(&self, scratch: &mut Scratch, input: &[u8], output: &mut [u8]) {
        let n = self.pixel_count(input.len(), output.len());
        if n > 0 {
            self.inner.apply(&mut scratch.0, input, output, n);
        }
    }
    /// Evaluate exactly `n` pixels (`cmsDoTransform`'s explicit count).
    ///
    /// # Panics
    /// If either buffer holds fewer than `n` pixels.
    pub fn apply_n(&self, scratch: &mut Scratch, input: &[u8], output: &mut [u8], n: u32) {
        let (pi, po) = (
            self.input_format().pixel_size(),
            self.output_format().pixel_size(),
        );
        assert!(
            input.len() >= n as usize * pi && output.len() >= n as usize * po,
            "{n} pixels need {} input / {} output bytes; got {} / {}",
            n as usize * pi,
            n as usize * po,
            input.len(),
            output.len()
        );
        self.inner.apply(&mut scratch.0, input, output, n);
    }
    /// [`Plan::apply`] on typed slices — any [`bytemuck::Pod`] element
    /// (`[u8; 3]`, `[u16; 4]`, `f32`, `[f64; 3]`, ...). The pixel count
    /// comes from the byte length, so per-pixel arrays and flat sample
    /// slices both work.
    ///
    /// ```
    /// use lcms::api::*;
    /// let plan = Plan::new(&Profile::srgb(), &Profile::lab4(), Format::RGB_8, Format::LAB_DBL)?;
    /// let mut lab = [[0f64; 3]; 2];
    /// plan.apply_pixels(&mut plan.scratch(), &[[255u8, 255, 255], [0, 0, 0]], &mut lab);
    /// assert!((lab[0][0] - 100.0).abs() < 0.01 && lab[1][0].abs() < 0.01);
    /// # Ok::<(), lcms::api::Error>(())
    /// ```
    ///
    /// # Panics
    /// As [`Plan::apply`].
    pub fn apply_pixels<I: bytemuck::Pod, O: bytemuck::Pod>(
        &self,
        scratch: &mut Scratch,
        input: &[I],
        output: &mut [O],
    ) {
        self.apply(
            scratch,
            bytemuck::cast_slice(input),
            bytemuck::cast_slice_mut(output),
        );
    }
    /// [`Plan::apply`] with a fresh internal [`Scratch`] — the one-call
    /// form when you don't manage scratch yourself. Output is identical.
    ///
    /// # Panics
    /// As [`Plan::apply`].
    pub fn transform(&self, input: &[u8], output: &mut [u8]) {
        self.apply(&mut self.scratch(), input, output);
    }
    /// [`Plan::apply_pixels`] with a fresh internal [`Scratch`].
    ///
    /// # Panics
    /// As [`Plan::apply`].
    pub fn transform_pixels<I: bytemuck::Pod, O: bytemuck::Pod>(
        &self,
        input: &[I],
        output: &mut [O],
    ) {
        self.apply_pixels(&mut self.scratch(), input, output);
    }
    /// In-place apply, for plans whose input and output pixels have the
    /// same size (e.g. `RGB_8 -> BGR_8`). Works through a 4 KiB stack
    /// buffer — no allocation.
    ///
    /// # Panics
    /// If the pixel sizes differ, either format is planar, or `buf` isn't
    /// a whole number of pixels.
    pub fn apply_in_place(&self, scratch: &mut Scratch, buf: &mut [u8]) {
        let (fi, fo) = (self.input_format(), self.output_format());
        let px = fi.pixel_size();
        assert!(
            px > 0 && px == fo.pixel_size() && !fi.is_planar() && !fo.is_planar(),
            "in-place needs equal interleaved pixel sizes, got {fi:?} -> {fo:?}"
        );
        assert!(
            buf.len() % px == 0,
            "buffer is not a whole number of {px}-byte pixels"
        );
        let mut tmp = [0u8; 4096];
        let per_chunk = tmp.len() / px * px;
        for chunk in buf.chunks_mut(per_chunk) {
            let src = &mut tmp[..chunk.len()];
            src.copy_from_slice(chunk);
            self.inner
                .apply(&mut scratch.0, src, chunk, (chunk.len() / px) as u32);
        }
    }
    /// A `width × height` image whose rows start `input_stride` /
    /// `output_stride` bytes apart (`cmsDoTransformLineStride`). Bytes
    /// between rows in `output` are left untouched.
    ///
    /// For planar formats each plane is `height` rows of the given stride,
    /// planes back to back (plane size = `stride × height`).
    ///
    /// ```
    /// use lcms::api::*;
    /// let srgb = Profile::srgb();
    /// let plan = Plan::new(&srgb, &srgb, Format::RGBA_8, Format::BGRA_8)?;
    /// let (width, height, stride) = (2, 2, 12); // 8 pixel bytes + 4 padding
    /// let src = vec![10u8; stride * height];
    /// let mut dst = vec![0u8; stride * height];
    /// plan.apply_strided(&mut plan.scratch(), &src, stride, &mut dst, stride, width, height);
    /// assert_eq!(&dst[8..12], &[0, 0, 0, 0]); // row padding untouched
    /// # Ok::<(), lcms::api::Error>(())
    /// ```
    ///
    /// # Panics
    /// If a stride is shorter than a row, or a buffer is too small for
    /// `height` rows.
    #[allow(clippy::too_many_arguments)]
    pub fn apply_strided(
        &self,
        scratch: &mut Scratch,
        input: &[u8],
        input_stride: usize,
        output: &mut [u8],
        output_stride: usize,
        width: usize,
        height: usize,
    ) {
        if width == 0 || height == 0 {
            return;
        }
        let (bpl_in, bpp_in) = stride_layout(
            self.input_format(),
            input.len(),
            input_stride,
            width,
            height,
            "input",
        );
        let (bpl_out, bpp_out) = stride_layout(
            self.output_format(),
            output.len(),
            output_stride,
            width,
            height,
            "output",
        );
        let w = u32::try_from(width).expect("width exceeds u32");
        let h = u32::try_from(height).expect("height exceeds u32");
        self.inner.apply_strided(
            &mut scratch.0,
            input,
            output,
            w,
            h,
            &xform::Stride {
                bytes_per_line_in: bpl_in,
                bytes_per_line_out: bpl_out,
                bytes_per_plane_in: bpp_in,
                bytes_per_plane_out: bpp_out,
            },
        );
    }
    /// Parallel apply — splits the pixels into `threads` chunks, each on a
    /// scoped thread with its own [`Scratch`]. `threads <= 1` runs on the
    /// calling thread. Output is identical to [`Plan::apply`]. One spawn
    /// per chunk per call; for hot loops keep worker threads and call
    /// [`Plan::apply`] per chunk instead.
    ///
    /// # Panics
    /// As [`Plan::apply`], or if a format is planar.
    pub fn apply_par(&self, input: &[u8], output: &mut [u8], threads: usize) {
        assert!(
            !self.input_format().is_planar() && !self.output_format().is_planar(),
            "apply_par splits interleaved pixels; use apply for planar formats"
        );
        let n = self.pixel_count(input.len(), output.len());
        if n > 0 {
            self.inner.apply_par(input, output, n, threads);
        }
    }
}

/// Validate one strided buffer; returns (bytes per line, bytes per plane).
fn stride_layout(
    f: Format,
    len: usize,
    stride: usize,
    width: usize,
    height: usize,
    what: &str,
) -> (u32, u32) {
    let (row, planes) = if f.is_planar() {
        (width * f.bytes_per_channel(), f.total_channels())
    } else {
        (width * f.pixel_size(), 1)
    };
    assert!(
        stride >= row,
        "{what} stride {stride} < row size {row} for {f:?}"
    );
    let plane = stride * height;
    let needed = plane * (planes - 1) + stride * (height - 1) + row;
    assert!(
        len >= needed,
        "{what} buffer is {len} bytes; {width}x{height} {f:?} needs {needed}"
    );
    let to32 = |v: usize| u32::try_from(v).expect("image too large for u32 strides");
    (to32(stride), to32(plane))
}

// -- ToneCurve ------------------------------------------------------------------

/// Construction, evaluation and inversion. `eval16`, `eval_float`,
/// `estimated_table` and `parametric_type` come from the parity impl.
impl ToneCurve {
    /// `cmsBuildGamma` — a plain `v^g` curve.
    pub fn gamma(g: f64) -> Result<Self> {
        attempt(types::ERROR_RANGE, "build_gamma", || {
            crate::gamma::build_gamma(None, g)
        })
    }
    /// A linear (identity) curve — `gamma(1.0)`.
    pub fn linear() -> Self {
        Self::gamma(1.0).expect("gamma 1.0 is valid")
    }
    /// The standard sRGB tone curve — `cmsBuildParametricToneCurve` type 4
    /// (`2.4` gamma + linear toe), the same curve `Profile::srgb` carries.
    pub fn srgb() -> Self {
        Self::parametric(4, &[2.4, 1.0 / 1.055, 0.055 / 1.055, 1.0 / 12.92, 0.04045])
            .expect("the sRGB parametric curve is valid")
    }
    /// `cmsBuildTabulatedToneCurve16` — a `u16` table, linearly interpolated.
    pub fn tabulated16(values: &[u16]) -> Result<Self> {
        attempt(types::ERROR_RANGE, "build_tabulated_tone_curve16", || {
            crate::gamma::build_tabulated_tone_curve16(None, values.len() as u32, Some(values))
        })
    }
    /// `cmsBuildTabulatedToneCurveFloat` — an `f32` table over `0.0..=1.0`.
    pub fn tabulated_float(values: &[f32]) -> Result<Self> {
        attempt(
            types::ERROR_RANGE,
            "build_tabulated_tone_curve_float",
            || crate::gamma::build_tabulated_tone_curve_float(None, values),
        )
    }
    /// `cmsBuildParametricToneCurve` — ICC parametric function `ty` with
    /// its `params` (type 1, pure gamma: `[g]`; type 4, sRGB-style:
    /// `[g, a, b, c, d]`; a negative type is the analytic inverse).
    pub fn parametric(ty: i32, params: &[f64]) -> Result<Self> {
        attempt(types::ERROR_RANGE, "build_parametric_tone_curve", || {
            crate::gamma::build_parametric_tone_curve(None, ty, params)
        })
    }
    /// `cmsGetToneCurveParams` — the parameters of a single-segment
    /// parametric curve, as many as its type takes (`None` for tabulated
    /// or segmented curves).
    pub fn parameters(&self) -> Option<&[f64]> {
        let ty = self.parametric_type();
        if ty == 0 {
            return None;
        }
        let seg = &self.segments[0];
        let n = crate::gamma::parametric_param_count(ty).unwrap_or(seg.params.len());
        Some(&seg.params[..n])
    }
    /// `cmsEstimateGamma` — the single gamma that best fits the curve
    /// within `precision` (e.g. `0.01`); `None` when none does.
    pub fn estimated_gamma(&self, precision: f64) -> Option<f64> {
        let g = crate::gamma::estimate_gamma(self, precision);
        if g < 0.0 { None } else { Some(g) }
    }
    /// `cmsIsToneCurveLinear`.
    pub fn is_linear(&self) -> bool {
        crate::gamma::is_tone_curve_linear(self)
    }
    /// `cmsIsToneCurveMonotonic`.
    pub fn is_monotonic(&self) -> bool {
        crate::gamma::is_tone_curve_monotonic(self)
    }
    /// `cmsIsToneCurveDescending`.
    pub fn is_descending(&self) -> bool {
        crate::gamma::is_tone_curve_descending(self)
    }
    /// `cmsIsToneCurveMultisegment`.
    pub fn is_multisegment(&self) -> bool {
        crate::gamma::is_tone_curve_multisegment(self)
    }
    /// `cmsReverseToneCurve` — the inverse curve (4096 samples).
    pub fn inverse(&self) -> Result<ToneCurve> {
        attempt(types::ERROR_RANGE, "reverse_tone_curve", || {
            crate::gamma::reverse_tone_curve(self)
        })
    }
    /// `cmsReverseToneCurveEx` — the inverse, sampled at `samples` points.
    pub fn inverse_sampled(&self, samples: usize) -> Result<ToneCurve> {
        attempt(types::ERROR_RANGE, "reverse_tone_curve_ex", || {
            crate::gamma::reverse_tone_curve_ex(samples as u32, self)
        })
    }
    /// `cmsJoinToneCurve(self, other)` — `other⁻¹(self(t))` sampled at
    /// `points` points: map through `self`, then back through `other`.
    pub fn then_inverse_of(&self, other: &ToneCurve, points: usize) -> Result<ToneCurve> {
        attempt(types::ERROR_RANGE, "join_tone_curve", || {
            crate::gamma::join_tone_curve(None, self, other, points as u32)
        })
    }
    /// `cmsSmoothToneCurve` — a copy smoothed with Whittaker factor
    /// `lambda`. Fails on curves too short or too irregular to smooth.
    pub fn smoothed(&self, lambda: f64) -> Result<ToneCurve> {
        let mut c = self.clone();
        attempt_bool(types::ERROR_RANGE, "smooth_tone_curve", || {
            crate::gamma::smooth_tone_curve(None, &mut c, lambda)
        })?;
        Ok(c)
    }
}
impl fmt::Debug for ToneCurve {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ToneCurve")
            .field("parametric_type", &self.parametric_type())
            .field("estimated_gamma", &self.estimated_gamma(0.001))
            .field("entries", &self.estimated_table().len())
            .finish()
    }
}

// -- tag reads / writes -----------------------------------------------------------

/// A borrowed view of a stored tag object — zero-cost: each variant
/// points at the decoded `TagObject` inside the [`Profile`]. `#[non_exhaustive]`
/// because ICC tag types grow; [`TagValue::Other`] is the fallback.
#[non_exhaustive]
pub enum TagValue<'a> {
    /// White point / colorant / luminance XYZ.
    Xyz(&'a CieXyz),
    /// Chromaticity (xyY primaries triple — `'chrm'`).
    Chromaticity(&'a CiexyYTriple),
    /// A tone curve (TRC, parametric curve, vcgt members).
    Curve(&'a ToneCurve),
    /// Multi-language text (description/copyright/text/desc/crdi).
    Text(&'a Mlu),
    /// A named-color palette (`'ncl2'`/`'clrt'`).
    NamedColors(&'a NamedColorList),
    /// A processing pipeline (mft1/mft2/mAB/mBA/mpet LUTs).
    Pipeline(&'a Pipeline),
    /// A lone processing stage (MPE element payload).
    Stage(&'a Stage),
    /// The 3-curve `'vcgt'` tag (video card gamma).
    VcgtCurves([&'a ToneCurve; 3]),
    /// The raw-table form of `'vcgt'`.
    VcgtTable {
        /// Channel count.
        channels: u16,
        /// Elements per channel.
        elems: u16,
        /// Bytes per element (1 or 2).
        bytes: u16,
        /// The raw table bytes.
        data: &'a [u8],
    },
    /// `'clro'` — channel order indices.
    ColorantOrder(&'a [u8]),
    /// `'scrn'` — halftone screening.
    Screening(&'a Screening),
    /// `'bfd '` — undercolor removal / black generation.
    UcrBg(&'a UcrBg),
    /// `'MHC2'` — Microsoft's display data tag.
    Mhc2(&'a Mhc2),
    /// `'cicp'` — video signal (BT.2100 etc.).
    VideoSignal(&'a VideoSignal),
    /// `'meas'` — measurement conditions.
    Measurement(&'a IccMeasurementConditions),
    /// `'view'` — viewing conditions.
    ViewingConditions(&'a IccViewingConditions),
    /// `'pseq'`/`'psid'` — profile sequence descriptor.
    Seq(&'a ProfileSeqDesc),
    /// `'dict'` — name/value dictionary.
    Dict(&'a Dict),
    /// `'sig '` — a bare signature value.
    Signature(u32),
    /// `'dtim'` — a calendar timestamp.
    DateTime(DateTimeNumber),
    /// `'ui08'`/`'ui32'`/`'ui64'` — integer arrays.
    UInt8(&'a [u8]),
    /// `'ui32'`.
    UInt32(&'a [u32]),
    /// `'ui64'`.
    UInt64(&'a [u64]),
    /// `'sf32'` — signed 15.16 fixed-point array.
    S15Fixed16(&'a [f64]),
    /// `'uf32'` — unsigned 16.16 fixed-point array.
    U16Fixed16(&'a [f64]),
    /// `'data'` — opaque payload (also raw/unparsed tag bytes).
    Data(&'a [u8]),
    /// Anything else — plugin-owned (`ForeignTag`) or unmapped objects.
    /// The parity layer (`little-cms-api`) sees the full `TagObject`.
    Other,
}

impl fmt::Debug for TagValue<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            TagValue::Xyz(_) => "Xyz(..)",
            TagValue::Chromaticity(_) => "Chromaticity(..)",
            TagValue::Curve(_) => "Curve(..)",
            TagValue::Text(_) => "Text(..)",
            TagValue::NamedColors(_) => "NamedColors(..)",
            TagValue::Pipeline(_) => "Pipeline(..)",
            TagValue::Stage(_) => "Stage(..)",
            TagValue::VcgtCurves(_) => "VcgtCurves(..)",
            TagValue::VcgtTable { .. } => "VcgtTable{..}",
            TagValue::ColorantOrder(_) => "ColorantOrder(..)",
            TagValue::Screening(_) => "Screening(..)",
            TagValue::UcrBg(_) => "UcrBg(..)",
            TagValue::Mhc2(_) => "Mhc2(..)",
            TagValue::VideoSignal(_) => "VideoSignal(..)",
            TagValue::Measurement(_) => "Measurement(..)",
            TagValue::ViewingConditions(_) => "ViewingConditions(..)",
            TagValue::Seq(_) => "Seq(..)",
            TagValue::Dict(_) => "Dict(..)",
            TagValue::Signature(s) => return write!(f, "TagValue::Signature({s:#x})"),
            TagValue::DateTime(_) => "DateTime(..)",
            TagValue::UInt8(_) => "UInt8(..)",
            TagValue::UInt32(_) => "UInt32(..)",
            TagValue::UInt64(_) => "UInt64(..)",
            TagValue::S15Fixed16(_) => "S15Fixed16(..)",
            TagValue::U16Fixed16(_) => "U16Fixed16(..)",
            TagValue::Data(_) => "Data(..)",
            TagValue::Other => "Other",
        };
        write!(f, "TagValue::{name}")
    }
}

impl<'a> TagValue<'a> {
    fn of(obj: &'a TagObject) -> Self {
        match obj {
            TagObject::Xyz(x) => TagValue::Xyz(x),
            TagObject::Chromaticity(c) => TagValue::Chromaticity(c),
            TagObject::ColorantOrder(o) => TagValue::ColorantOrder(o),
            TagObject::Curve(c) | TagObject::ParametricCurve(c) => TagValue::Curve(c),
            TagObject::Mlu(m)
            | TagObject::Text(m)
            | TagObject::TextDescription(m)
            | TagObject::CrdInfo(m) => TagValue::Text(m),
            TagObject::NamedColor(l) | TagObject::ColorantTable(l) => TagValue::NamedColors(l),
            TagObject::Lut8(l)
            | TagObject::Lut16(l)
            | TagObject::LutA2B(l)
            | TagObject::LutB2A(l)
            | TagObject::Mpe(l) => TagValue::Pipeline(l),
            TagObject::MpeElem(s) => TagValue::Stage(s),
            TagObject::Vcgt(VcgtData::Curves(c)) => TagValue::VcgtCurves([&c[0], &c[1], &c[2]]),
            TagObject::Vcgt(VcgtData::Table {
                channels,
                elems,
                bytes,
                data,
            }) => TagValue::VcgtTable {
                channels: *channels,
                elems: *elems,
                bytes: *bytes,
                data,
            },
            TagObject::Data(d) => TagValue::Data(&d.data),
            TagObject::Raw(v) => TagValue::Data(v),
            TagObject::Screening(s) => TagValue::Screening(s),
            TagObject::UcrBg(u) => TagValue::UcrBg(u),
            TagObject::Mhc2(m) => TagValue::Mhc2(m),
            TagObject::VideoSignal(v) => TagValue::VideoSignal(v),
            TagObject::Measurement(m) => TagValue::Measurement(m),
            TagObject::ViewingConditions(v) => TagValue::ViewingConditions(v),
            TagObject::ProfileSeqDesc(s) | TagObject::ProfileSeqId(s) => TagValue::Seq(s),
            TagObject::Dictionary(d) => TagValue::Dict(d),
            TagObject::Signature(s) => TagValue::Signature(*s),
            TagObject::DateTime(t) => TagValue::DateTime(date_time_from(t)),
            TagObject::UInt8(v) => TagValue::UInt8(v),
            TagObject::UInt32(v) => TagValue::UInt32(v),
            TagObject::UInt64(v) => TagValue::UInt64(v),
            TagObject::S15Fixed16(v) => TagValue::S15Fixed16(v),
            TagObject::U16Fixed16(v) => TagValue::U16Fixed16(v),
            _ => TagValue::Other,
        }
    }
    /// `Some(&xyz)` for [`TagValue::Xyz`].
    pub fn as_xyz(&self) -> Option<&'a CieXyz> {
        match self {
            TagValue::Xyz(x) => Some(x),
            _ => None,
        }
    }
    /// `Some(&curve)` for [`TagValue::Curve`].
    pub fn as_curve(&self) -> Option<&'a ToneCurve> {
        match self {
            TagValue::Curve(c) => Some(c),
            _ => None,
        }
    }
    /// `Some(&mlu)` for [`TagValue::Text`].
    pub fn as_text(&self) -> Option<&'a Mlu> {
        match self {
            TagValue::Text(m) => Some(m),
            _ => None,
        }
    }
    /// `Some(&list)` for [`TagValue::NamedColors`].
    pub fn as_named_colors(&self) -> Option<&'a NamedColorList> {
        match self {
            TagValue::NamedColors(l) => Some(l),
            _ => None,
        }
    }
    /// `Some(&pipeline)` for [`TagValue::Pipeline`].
    pub fn as_pipeline(&self) -> Option<&'a Pipeline> {
        match self {
            TagValue::Pipeline(p) => Some(p),
            _ => None,
        }
    }
    /// `Some(&stage)` for [`TagValue::Stage`].
    pub fn as_stage(&self) -> Option<&'a Stage> {
        match self {
            TagValue::Stage(s) => Some(s),
            _ => None,
        }
    }
    /// `Some(&dict)` for [`TagValue::Dict`].
    pub fn as_dict(&self) -> Option<&'a Dict> {
        match self {
            TagValue::Dict(d) => Some(d),
            _ => None,
        }
    }
    /// `Some(&seq)` for [`TagValue::Seq`].
    pub fn as_seq(&self) -> Option<&'a ProfileSeqDesc> {
        match self {
            TagValue::Seq(s) => Some(s),
            _ => None,
        }
    }
    /// `true` for [`TagValue::Other`] — an unmapped/plugin object.
    pub fn is_other(&self) -> bool {
        matches!(self, TagValue::Other)
    }
}

/// The owned write-side counterpart of [`TagValue`] —
/// [`Profile::write_tag`] stores it.
///
/// ```
/// use lcms::api::*;
/// let mut p = Profile::placeholder();
/// p.set_color_space(ColorSpace::Rgb);
/// p.set_info(InfoType::Description, "My display")?;
/// p.write_tag(Tag::MEDIA_WHITE_POINT, D50_XYZ)?;
/// assert!(p.has_tag(Tag::PROFILE_DESCRIPTION));
/// # Ok::<(), lcms::api::Error>(())
/// ```
#[non_exhaustive]
pub enum TagData<'a> {
    /// XYZ value (`'wtpt'`, colorants, `'lumi'`).
    Xyz(CieXyz),
    /// Chromaticity triple (`'chrm'`).
    Chromaticity(CiexyYTriple),
    /// `'clro'` — channel order indices (up to 16 channels).
    ColorantOrder([u8; 16]),
    /// A tone curve — writes as `'curv'`/`'para'` per its shape.
    Curve(ToneCurve),
    /// A tone curve forced to the `'para'` parametric encoding.
    ParametricCurve(ToneCurve),
    /// Text in the default locale — stored as an MLU (UTF-8 ok).
    Text(&'a str),
    /// A fully authored multi-language string.
    Mlu(Mlu),
    /// A named-color palette (`'ncl2'`/`'clrt'`).
    NamedColors(NamedColorList),
    /// A processing pipeline (writes as an `mft2` Lut16 object — for
    /// `A2B*`/`B2A*`/preview/gamut tags).
    Pipeline(Pipeline),
    /// A `'data'` payload.
    Data(IccData),
    /// A `'sig '` bare signature.
    Signature(u32),
    /// A `'dtim'` timestamp.
    DateTime(DateTimeNumber),
    /// A `'meas'` measurement-conditions record.
    Measurement(IccMeasurementConditions),
    /// A `'view'` viewing-conditions record.
    ViewingConditions(IccViewingConditions),
    /// `'bfd '` — UCR/black-generation.
    UcrBg(UcrBg),
    /// `'scrn'` — halftone screening.
    Screening(Screening),
    /// `'cicp'` — video signal.
    VideoSignal(VideoSignal),
    /// `'MHC2'` — Microsoft display data.
    Mhc2(Mhc2),
    /// `'vcgt'` — three gamma curves (R, G, B).
    VcgtCurves([ToneCurve; 3]),
    /// `'vcgt'` — a raw lookup table.
    VcgtTable {
        /// Channel count.
        channels: u16,
        /// Elements per channel.
        elems: u16,
        /// Bytes per element (1 or 2).
        bytes: u16,
        /// The raw table bytes.
        data: Vec<u8>,
    },
    /// `'pseq'`/`'psid'` — profile sequence descriptor.
    Seq(ProfileSeqDesc),
    /// `'dict'` — name/value dictionary.
    Dict(Dict),
    /// `'ui08'`/`'ui32'`/`'ui64'` — integer arrays.
    UInt8(Vec<u8>),
    /// `'ui32'`.
    UInt32(Vec<u32>),
    /// `'ui64'`.
    UInt64(Vec<u64>),
    /// `'sf32'` — signed 15.16 fixed-point array.
    S15Fixed16(Vec<f64>),
    /// `'uf32'` — unsigned 16.16 fixed-point array.
    U16Fixed16(Vec<f64>),
}
impl fmt::Debug for TagData<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            TagData::Xyz(_) => "TagData::Xyz(..)",
            TagData::Chromaticity(_) => "TagData::Chromaticity(..)",
            TagData::ColorantOrder(_) => "TagData::ColorantOrder(..)",
            TagData::Curve(_) => "TagData::Curve(..)",
            TagData::ParametricCurve(_) => "TagData::ParametricCurve(..)",
            TagData::Text(_) => "TagData::Text(..)",
            TagData::Mlu(_) => "TagData::Mlu(..)",
            TagData::NamedColors(_) => "TagData::NamedColors(..)",
            TagData::Pipeline(_) => "TagData::Pipeline(..)",
            TagData::Data(_) => "TagData::Data(..)",
            TagData::Signature(_) => "TagData::Signature(..)",
            TagData::DateTime(_) => "TagData::DateTime(..)",
            TagData::Measurement(_) => "TagData::Measurement(..)",
            TagData::ViewingConditions(_) => "TagData::ViewingConditions(..)",
            TagData::UcrBg(_) => "TagData::UcrBg(..)",
            TagData::Screening(_) => "TagData::Screening(..)",
            TagData::VideoSignal(_) => "TagData::VideoSignal(..)",
            TagData::Mhc2(_) => "TagData::Mhc2(..)",
            TagData::VcgtCurves(_) => "TagData::VcgtCurves(..)",
            TagData::VcgtTable { .. } => "TagData::VcgtTable{..}",
            TagData::Seq(_) => "TagData::Seq(..)",
            TagData::Dict(_) => "TagData::Dict(..)",
            TagData::UInt8(_) => "TagData::UInt8(..)",
            TagData::UInt32(_) => "TagData::UInt32(..)",
            TagData::UInt64(_) => "TagData::UInt64(..)",
            TagData::S15Fixed16(_) => "TagData::S15Fixed16(..)",
            TagData::U16Fixed16(_) => "TagData::U16Fixed16(..)",
        })
    }
}
impl<'a> From<&'a str> for TagData<'a> {
    fn from(s: &'a str) -> TagData<'a> {
        TagData::Text(s)
    }
}
impl From<ToneCurve> for TagData<'_> {
    fn from(c: ToneCurve) -> Self {
        TagData::Curve(c)
    }
}
impl From<CieXyz> for TagData<'_> {
    fn from(x: CieXyz) -> Self {
        TagData::Xyz(x)
    }
}
impl From<CiexyYTriple> for TagData<'_> {
    fn from(c: CiexyYTriple) -> Self {
        TagData::Chromaticity(c)
    }
}
impl From<Mlu> for TagData<'_> {
    fn from(m: Mlu) -> Self {
        TagData::Mlu(m)
    }
}
impl From<NamedColorList> for TagData<'_> {
    fn from(l: NamedColorList) -> Self {
        TagData::NamedColors(l)
    }
}
impl From<VideoSignal> for TagData<'_> {
    fn from(v: VideoSignal) -> Self {
        TagData::VideoSignal(v)
    }
}
impl From<[ToneCurve; 3]> for TagData<'_> {
    fn from(c: [ToneCurve; 3]) -> Self {
        TagData::VcgtCurves(c)
    }
}

// -- named colors --------------------------------------------------------------

/// Iterator over a [`NamedColorList`] — see [`NamedColorList::iter`].
#[derive(Clone, Debug)]
pub struct NamedColorIter<'a> {
    list: &'a NamedColorList,
    i: usize,
}
impl Iterator for NamedColorIter<'_> {
    type Item = NamedColor;
    fn next(&mut self) -> Option<NamedColor> {
        let c = self.list.entry(self.i)?;
        self.i += 1;
        Some(c)
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        let left = self.list.len().saturating_sub(self.i);
        (left, Some(left))
    }
}
impl ExactSizeIterator for NamedColorIter<'_> {}
impl std::iter::FusedIterator for NamedColorIter<'_> {}
impl<'a> IntoIterator for &'a NamedColorList {
    type Item = NamedColor;
    type IntoIter = NamedColorIter<'a>;
    fn into_iter(self) -> NamedColorIter<'a> {
        self.iter()
    }
}

/// `'dtim'` tags decode to a C `struct tm`; the api speaks the ICC
/// `dateTimeNumber` both ways (the writer converts back exactly).
fn date_time_from(t: &Tm) -> DateTimeNumber {
    DateTimeNumber {
        year: (t.tm_year + 1900) as u16,
        month: (t.tm_mon + 1) as u16,
        day: t.tm_mday as u16,
        hours: t.tm_hour as u16,
        minutes: t.tm_min as u16,
        seconds: t.tm_sec as u16,
    }
}
fn tm_from(d: DateTimeNumber) -> Tm {
    Tm {
        tm_sec: d.seconds as i32,
        tm_min: d.minutes as i32,
        tm_hour: d.hours as i32,
        tm_mday: d.day as i32,
        tm_mon: d.month as i32 - 1,
        tm_year: d.year as i32 - 1900,
        tm_wday: -1,
        tm_yday: -1,
        tm_isdst: 0,
    }
}

fn cstr(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).into_owned()
}

/// A single named-color entry.
#[derive(Clone, Debug, PartialEq)]
pub struct NamedColor {
    /// Spot-color name.
    pub name: String,
    /// PCS encoding (Lab or XYZ u16 triple, per the profile).
    pub pcs: [u16; 3],
    /// Device colorant values (len = the list's `colorant_count`).
    pub colorant: Vec<u16>,
}

// -- ergonomic methods on the re-exported parity types ------------------------------
//
// These are inherent impls on same-crate types — they exist in every
// feature configuration and never collide with the lcms2 shim (which
// deliberately picked different names).

/// Construction, editing and evaluation (`cmsPipeline*`). The accessors
/// `input_channels`, `output_channels`, `stage_count`, `first_stage`,
/// `last_stage`, `eval16`, `eval_float` and `set_save_as_8bits` come from
/// the parity impl.
impl Pipeline {
    /// `cmsPipelineAlloc` — an empty pipeline from `input_channels` to
    /// `output_channels` (each `1..=15`).
    pub fn new(input_channels: u32, output_channels: u32) -> Result<Pipeline> {
        attempt(types::ERROR_RANGE, "pipeline_alloc", || {
            crate::lut::pipeline_alloc(None, input_channels, output_channels)
        })
    }
    /// `cmsPipelineInsertStage(.., cmsAtEnd)` — append `stage`. Fails,
    /// leaving the pipeline as it was, when the stage's input channels
    /// don't match the current output.
    pub fn push(&mut self, stage: Stage) -> Result<()> {
        self.insert(crate::lut::StageLoc::AtEnd, stage)
    }
    /// `cmsPipelineInsertStage(.., cmsAtBegin)` — prepend `stage`; fails
    /// like [`Pipeline::push`].
    pub fn push_front(&mut self, stage: Stage) -> Result<()> {
        self.insert(crate::lut::StageLoc::AtBegin, stage)
    }
    fn insert(&mut self, loc: crate::lut::StageLoc, stage: Stage) -> Result<()> {
        let (stage_in, stage_out) = (stage.input_channels(), stage.output_channels());
        let _ = crate::err::take_last_error();
        if self.insert_stage(loc, stage) {
            return Ok(());
        }
        // C leaves the rejected stage linked into the chain; unlink it so a
        // failed push is a no-op.
        let _ = self.unlink_stage(loc);
        let _ = crate::err::take_last_error();
        Err(Error::new(
            types::ERROR_RANGE,
            format!(
                "a {stage_in}->{stage_out} channel stage doesn't chain onto this {}->{} pipeline",
                self.input_channels(),
                self.output_channels()
            ),
        ))
    }
    /// `cmsPipelineUnlinkStage(.., cmsAtEnd)` — remove and return the last
    /// stage.
    pub fn pop(&mut self) -> Option<Stage> {
        self.unlink_stage(crate::lut::StageLoc::AtEnd)
    }
    /// `cmsPipelineUnlinkStage(.., cmsAtBegin)` — remove and return the
    /// first stage.
    pub fn pop_front(&mut self) -> Option<Stage> {
        self.unlink_stage(crate::lut::StageLoc::AtBegin)
    }
    /// `cmsPipelineCat` — append copies of `other`'s stages. Fails, leaving
    /// the pipeline as it was, when the channel counts don't chain.
    pub fn append(&mut self, other: &Pipeline) -> Result<()> {
        let before = self.clone();
        attempt_bool(types::ERROR_RANGE, "pipeline_cat", || self.cat(other)).inspect_err(|_| {
            // C keeps whatever stages it linked before the mismatch
            *self = before;
        })
    }
    /// `cmsPipelineEvalReverseFloat` — Newton-solve for the input that
    /// produces `target` (3 output channels; 3- or 4-channel input),
    /// starting from `hint` (default `[0.3, 0.3, 0.3]`). Returns the best
    /// input found; fails for other channel layouts or a singular step.
    pub fn eval_reverse(&self, target: &[f32; 3], hint: Option<&[f32]>) -> Result<Vec<f32>> {
        let mut out = vec![0f32; self.input_channels() as usize];
        if let Some(h) = hint {
            assert!(h.len() >= 3, "hint needs 3 values, got {}", h.len());
        }
        attempt_bool(types::ERROR_RANGE, "pipeline_eval_reverse_float", || {
            self.eval_reverse_float(target, &mut out, hint)
        })?;
        Ok(out)
    }
    /// Iterate the stages, first to last.
    pub fn stages(&self) -> Stages<'_> {
        Stages(self.elements.as_deref())
    }
}
impl fmt::Debug for Pipeline {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Pipeline")
            .field("input_channels", &self.input_channels())
            .field("output_channels", &self.output_channels())
            .field("stages", &self.stages().collect::<Vec<_>>())
            .finish()
    }
}

/// Iterator over a [`Pipeline`]'s stages — see [`Pipeline::stages`].
#[derive(Clone, Debug)]
pub struct Stages<'a>(Option<&'a Stage>);
impl<'a> Iterator for Stages<'a> {
    type Item = &'a Stage;
    fn next(&mut self) -> Option<&'a Stage> {
        let s = self.0?;
        self.0 = s.next.as_deref();
        Some(s)
    }
}
impl std::iter::FusedIterator for Stages<'_> {}
impl<'a> IntoIterator for &'a Pipeline {
    type Item = &'a Stage;
    type IntoIter = Stages<'a>;
    fn into_iter(self) -> Stages<'a> {
        self.stages()
    }
}

/// The `cmsStageAlloc*` constructors. `input_channels`, `output_channels`,
/// `stage_type` (the stage's ICC signature, e.g. `'matf'`) and `eval` come
/// from the parity impl.
impl Stage {
    /// `cmsStageAllocIdentity` — pass `n_channels` channels through.
    pub fn identity(n_channels: u32) -> Stage {
        crate::lut::stage_alloc_identity(None, n_channels)
    }
    /// `cmsStageAllocToneCurves` — one curve per channel.
    pub fn tone_curves(curves: &[&ToneCurve]) -> Result<Stage> {
        attempt(types::ERROR_RANGE, "stage_alloc_tone_curves", || {
            crate::lut::stage_alloc_tone_curves(None, curves.len() as u32, Some(curves))
        })
    }
    /// `cmsStageAllocMatrix` — a `rows × cols` matrix (row-major, `cols`
    /// inputs → `rows` outputs) plus optional per-output offsets.
    pub fn matrix(matrix: &[f64], rows: u32, cols: u32, offsets: Option<&[f64]>) -> Result<Stage> {
        attempt(types::ERROR_RANGE, "stage_alloc_matrix", || {
            crate::lut::stage_alloc_matrix(None, rows, cols, matrix, offsets)
        })
    }
    /// `cmsStageAllocCLut16bit` — a CLUT with `points` grid points per
    /// input dimension. `table` holds `points^in_chan × out_chan` values
    /// (`None` = all zero).
    pub fn clut16(
        points: u32,
        in_chan: u32,
        out_chan: u32,
        table: Option<&[u16]>,
    ) -> Result<Stage> {
        attempt(types::ERROR_RANGE, "stage_alloc_clut16bit", || {
            crate::lut::stage_alloc_clut16bit(None, points, in_chan, out_chan, table)
        })
    }
    /// `cmsStageAllocCLut16bitGranular` — a CLUT with its own grid size per
    /// input dimension (`points.len()` inputs).
    pub fn clut16_granular(points: &[u32], out_chan: u32, table: Option<&[u16]>) -> Result<Stage> {
        attempt(types::ERROR_RANGE, "stage_alloc_clut16bit_granular", || {
            crate::lut::stage_alloc_clut16bit_granular(
                None,
                points,
                points.len() as u32,
                out_chan,
                table,
            )
        })
    }
    /// `cmsStageAllocCLutFloat` — [`Stage::clut16`] with `f32` entries.
    pub fn clut_float(
        points: u32,
        in_chan: u32,
        out_chan: u32,
        table: Option<&[f32]>,
    ) -> Result<Stage> {
        attempt(types::ERROR_RANGE, "stage_alloc_clut_float", || {
            crate::lut::stage_alloc_clut_float(None, points, in_chan, out_chan, table)
        })
    }
}
impl fmt::Debug for Stage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Stage")
            .field("type", &Tag(self.stage_type()))
            .field("input_channels", &self.input_channels())
            .field("output_channels", &self.output_channels())
            .finish()
    }
}

/// Build and read multi-language text (`cmsMLU*`).
impl Mlu {
    /// `cmsMLUalloc` — empty text with no translations.
    pub fn new() -> Mlu {
        named::mlu_alloc(None, 0).expect("mlu_alloc never fails")
    }
    /// `cmsMLUsetUTF8` — set `text` for `locale`, replacing any text the
    /// locale already had ([`Locale::NONE`] is the default entry).
    pub fn set(&mut self, locale: Locale, text: &str) -> Result<()> {
        attempt_bool(types::ERROR_RANGE, "mlu_set_utf8", || {
            named::mlu_set_utf8(self, &locale.language, &locale.country, text.as_bytes())
        })
    }
    /// `cmsMLUsetASCII` — [`Mlu::set`] for ASCII text; fails on non-ASCII.
    pub fn set_ascii(&mut self, locale: Locale, text: &str) -> Result<()> {
        if !text.is_ascii() {
            return Err(Error::new(
                types::ERROR_RANGE,
                "set_ascii: text is not ASCII",
            ));
        }
        attempt_bool(types::ERROR_RANGE, "mlu_set_ascii", || {
            named::mlu_set_ascii(self, &locale.language, &locale.country, text.as_bytes())
        })
    }
    /// `cmsMLUgetUTF8` — the text for `locale`, falling back like C: same
    /// language in another country, then the first entry.
    pub fn get(&self, locale: Locale) -> Option<String> {
        // size query first: C's `cmsMLUgetUTF8(.., NULL, 0)` returns the
        // byte count needed, NUL included — no fixed-buffer truncation.
        let (need, _) = named::mlu_get_utf8(self, &locale.language, &locale.country, None);
        if need == 0 {
            return None;
        }
        let (n, buf) = named::mlu_get_utf8(self, &locale.language, &locale.country, Some(need));
        trimmed(n, buf)
    }
    /// `cmsMLUgetASCII` — [`Mlu::get`] with non-ASCII characters replaced
    /// by `?`, as C does.
    pub fn get_ascii(&self, locale: Locale) -> Option<String> {
        let (need, _) = named::mlu_get_ascii(self, &locale.language, &locale.country, None);
        if need == 0 {
            return None;
        }
        let (n, buf) = named::mlu_get_ascii(self, &locale.language, &locale.country, Some(need));
        trimmed(n, buf)
    }
    /// `cmsMLUtranslationsCount` — how many locales have text.
    pub fn len(&self) -> usize {
        named::mlu_translations_count(self) as usize
    }
    /// `true` when no locale has text.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// `cmsMLUtranslationsCodes` — the locales that have text, in the
    /// order they were added.
    pub fn locales(&self) -> impl ExactSizeIterator<Item = Locale> + '_ {
        (0..self.len() as u32).map(|i| {
            let (language, country) =
                named::mlu_translations_codes(self, i).expect("index below translation count");
            Locale { language, country }
        })
    }
}
fn trimmed(n: u32, mut buf: Vec<u8>) -> Option<String> {
    if n == 0 {
        return None;
    }
    buf.truncate(n as usize);
    while buf.last() == Some(&0) {
        buf.pop();
    }
    String::from_utf8(buf).ok()
}
/// Text in the default locale ([`Locale::NONE`]).
impl From<&str> for Mlu {
    fn from(text: &str) -> Mlu {
        let mut m = Mlu::new();
        m.set(Locale::NONE, text)
            .expect("UTF-8 text always fits an MLU");
        m
    }
}
impl fmt::Debug for Mlu {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map()
            .entries(self.locales().map(|l| (l, self.get(l).unwrap_or_default())))
            .finish()
    }
}

/// Build and read palettes (`cmsAllocNamedColorList`, `cmsAppendNamedColor`,
/// `cmsNamedColorInfo`, ...).
impl NamedColorList {
    /// `cmsAllocNamedColorList` — an empty palette whose entries carry
    /// `colorant_count` device channels (`0..=15`), with list-wide `prefix`
    /// and `suffix` strings (each up to 32 bytes).
    pub fn new(colorant_count: usize, prefix: &str, suffix: &str) -> Result<NamedColorList> {
        attempt(types::ERROR_RANGE, "alloc_named_color_list", || {
            named::alloc_named_color_list(
                None,
                0,
                colorant_count as u32,
                prefix.as_bytes(),
                suffix.as_bytes(),
            )
        })
    }
    /// `cmsAppendNamedColor` — append an entry: `name` (up to 255 bytes),
    /// its 16-bit PCS encoding, and one value per device colorant (`&[]`
    /// = all zero).
    pub fn push(&mut self, name: &str, pcs: [u16; 3], colorant: &[u16]) -> Result<()> {
        let n = self.colorant_count as usize;
        if !colorant.is_empty() && colorant.len() != n {
            return Err(Error::new(
                types::ERROR_RANGE,
                format!("list has {n} colorants per color, got {}", colorant.len()),
            ));
        }
        let mut c = [0u16; 16];
        c[..colorant.len()].copy_from_slice(colorant);
        attempt_bool(types::ERROR_RANGE, "append_named_color", || {
            named::append_named_color(self, Some(name.as_bytes()), Some(&pcs), Some(&c))
        })
    }
    /// `cmsNamedColorCount`.
    pub fn len(&self) -> usize {
        self.n_colors as usize
    }
    /// `true` when the palette has no entries.
    pub fn is_empty(&self) -> bool {
        self.n_colors == 0
    }
    /// `cmsNamedColorInfo` — the `index`-th entry.
    pub fn entry(&self, index: usize) -> Option<NamedColor> {
        // `list` holds `allocated` slots; only the first `n_colors` are set
        if index >= self.len() {
            return None;
        }
        let c = &self.list[index];
        Some(NamedColor {
            name: cstr(&c.name),
            pcs: c.pcs,
            colorant: c.device_colorant[..self.colorant_count as usize].to_vec(),
        })
    }
    /// `cmsNamedColorIndex` — the index of the entry called `name` (ASCII
    /// case-insensitive, without the prefix/suffix).
    pub fn position(&self, name: &str) -> Option<usize> {
        usize::try_from(named::named_color_index(self, name.as_bytes())).ok()
    }
    /// Iterate the entries in order.
    pub fn iter(&self) -> NamedColorIter<'_> {
        NamedColorIter { list: self, i: 0 }
    }
    /// The list-wide prefix (e.g. `"PANTONE "`).
    pub fn prefix(&self) -> String {
        cstr(&self.prefix)
    }
    /// The list-wide suffix.
    pub fn suffix(&self) -> String {
        cstr(&self.suffix)
    }
    /// Device colorant channels per entry.
    pub fn colorant_count(&self) -> usize {
        self.colorant_count as usize
    }
}
impl fmt::Debug for NamedColorList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NamedColorList")
            .field("prefix", &self.prefix())
            .field("suffix", &self.suffix())
            .field("colors", &self.iter().collect::<Vec<_>>())
            .finish()
    }
}

/// Name/value string pairs (`cmsDict*`).
impl Dict {
    /// `cmsDictAlloc` — an empty dictionary.
    pub fn new() -> Dict {
        named::dict_alloc(None)
    }
    /// `cmsDictAddEntry` — add `name` = `value`, with matching display
    /// strings in the default locale. Names may repeat; [`Dict::get`]
    /// sees the newest.
    pub fn add(&mut self, name: &str, value: &str) -> Result<()> {
        let wide = |s: &str| -> Vec<named::Wchar> {
            s.chars().map(|c| c as named::Wchar).chain([0]).collect()
        };
        let display_name = Mlu::from(name);
        let display_value = Mlu::from(value);
        attempt_bool(types::ERROR_RANGE, "dict_add_entry", || {
            named::dict_add_entry(
                self,
                &wide(name),
                Some(&wide(value)),
                Some(&display_name),
                Some(&display_value),
            )
        })
    }
    /// The value stored under `name` (the newest, if it repeats).
    pub fn get(&self, name: &str) -> Option<String> {
        // `entries` is newest-first, like C's linked list
        self.entries
            .iter()
            .find(|e| narrow(&e.name) == name)
            .map(|e| e.value.as_deref().map(narrow).unwrap_or_default())
    }
    /// `(name, value)` pairs in the order they were added.
    pub fn iter(
        &self,
    ) -> impl DoubleEndedIterator<Item = (String, String)> + ExactSizeIterator + '_ {
        self.entries.iter().rev().map(|e| {
            (
                narrow(&e.name),
                e.value.as_deref().map(narrow).unwrap_or_default(),
            )
        })
    }
    /// Entry count.
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    /// `true` when empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
/// A NUL-terminated `wchar_t` string as UTF-8 (invalid code points → U+FFFD).
fn narrow(w: &[named::Wchar]) -> String {
    w.iter()
        .take_while(|&&c| c != 0)
        .map(|&c| {
            u32::try_from(c)
                .ok()
                .and_then(char::from_u32)
                .unwrap_or('\u{fffd}')
        })
        .collect()
}
impl fmt::Debug for Dict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}

/// The per-profile records of a `'pseq'`/`'psid'` tag.
impl ProfileSeqDesc {
    /// Number of profiles described.
    pub fn len(&self) -> usize {
        self.seq.len()
    }
    /// `true` when empty.
    pub fn is_empty(&self) -> bool {
        self.seq.is_empty()
    }
    /// The `index`-th profile's record.
    pub fn get(&self, index: usize) -> Option<&ProfileSeqEntry> {
        self.seq.get(index)
    }
    /// Iterate the records in chain order.
    pub fn iter(&self) -> std::slice::Iter<'_, ProfileSeqEntry> {
        self.seq.iter()
    }
}
impl<'a> IntoIterator for &'a ProfileSeqDesc {
    type Item = &'a ProfileSeqEntry;
    type IntoIter = std::slice::Iter<'a, ProfileSeqEntry>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}
impl fmt::Debug for ProfileSeqDesc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

/// One linked profile's header fields and description text.
impl ProfileSeqEntry {
    /// The profile's device manufacturer signature.
    pub fn device_manufacturer(&self) -> u32 {
        self.device_mfg
    }
    /// The profile's device model signature.
    pub fn device_model(&self) -> u32 {
        self.device_model
    }
    /// The profile's header attributes.
    pub fn attributes(&self) -> u64 {
        self.attributes
    }
    /// The `'tech'` technology signature.
    pub fn technology(&self) -> u32 {
        self.technology
    }
    /// The profile ID (all zero in `'pseq'`, set in `'psid'`).
    pub fn profile_id(&self) -> ProfileId {
        self.profile_id
    }
    /// The manufacturer description, if recorded.
    pub fn manufacturer(&self) -> Option<&Mlu> {
        self.manufacturer.as_ref()
    }
    /// The model description, if recorded.
    pub fn model(&self) -> Option<&Mlu> {
        self.model.as_ref()
    }
    /// The profile description, if recorded (`'psid'`).
    pub fn description(&self) -> Option<&Mlu> {
        self.description.as_ref()
    }
}
impl fmt::Debug for ProfileSeqEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProfileSeqEntry")
            .field("device_manufacturer", &Tag(self.device_mfg))
            .field("device_model", &Tag(self.device_model))
            .field("technology", &Tag(self.technology))
            .field("description", &self.description)
            .finish()
    }
}

/// An ICC `'data'` payload.
impl IccData {
    /// ASCII text (stored with `flag` 0).
    pub fn text(text: &str) -> IccData {
        IccData::with_flag(0, text.as_bytes().to_vec())
    }
    /// Binary bytes (`flag` 1).
    pub fn binary(bytes: Vec<u8>) -> IccData {
        IccData::with_flag(1, bytes)
    }
    fn with_flag(flag: u32, data: Vec<u8>) -> IccData {
        IccData {
            len: data.len() as u32,
            flag,
            data,
        }
    }
    /// `true` for binary data, `false` for ASCII text.
    pub fn is_binary(&self) -> bool {
        self.flag != 0
    }
    /// The payload bytes.
    pub fn bytes(&self) -> &[u8] {
        &self.data[..(self.len as usize).min(self.data.len())]
    }
}
impl fmt::Debug for IccData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IccData")
            .field("binary", &self.is_binary())
            .field("len", &self.bytes().len())
            .finish()
    }
}

// -- free functions -------------------------------------------------------------

/// `cmsGetEncodedCMMversion` — `2200` for lcms 2.20.
pub fn version() -> u32 {
    types::LCMS_VERSION
}
/// The D50 white point (the ICC PCS illuminant) in XYZ — `cmsD50_XYZ()`.
pub const D50_XYZ: CieXyz = CieXyz {
    X: types::D50X,
    Y: types::D50Y,
    Z: types::D50Z,
};
/// The D50 white point in xyY — bit-identical to `cmsD50_xyY()` (same
/// arithmetic as `cmsXYZ2xyY`).
pub const D50: CiexyY = {
    let i_sum = 1. / (types::D50X + types::D50Y + types::D50Z);
    CiexyY {
        x: types::D50X * i_sum,
        y: types::D50Y * i_sum,
        Y: types::D50Y,
    }
};
/// The D65 white point in xyY (CIE 15 / IEC 61966-2.1, the value
/// [`Profile::srgb`] is built with).
pub const D65: CiexyY = CiexyY {
    x: 0.3127,
    y: 0.3290,
    Y: 1.0,
};
/// Rec. 709 / sRGB primaries in xyY (the values [`Profile::srgb`] is
/// built with).
pub const REC709_PRIMARIES: CiexyYTriple = CiexyYTriple {
    Red: CiexyY {
        x: 0.6400,
        y: 0.3300,
        Y: 1.0,
    },
    Green: CiexyY {
        x: 0.3000,
        y: 0.6000,
        Y: 1.0,
    },
    Blue: CiexyY {
        x: 0.1500,
        y: 0.0600,
        Y: 1.0,
    },
};

/// `cmsWhitePointFromTemp` — white point (xyY) for a blackbody/`D`
/// illuminant temperature in K (`4000`..`25000`).
pub fn white_point_from_temp(temp_k: f64) -> Option<CiexyY> {
    crate::wtpnt::white_point_from_temp(temp_k)
}
/// `cmsTempFromWhitePoint` — correlated color temperature (K) of a
/// white point, `None` if it doesn't map.
pub fn temp_from_white_point(white_point: &CiexyY) -> Option<f64> {
    crate::wtpnt::temp_from_white_point(white_point)
}
/// `cmsSetAdaptationState` — the global white-point adaptation degree
/// (`0.0` = never adapt, `1.0` = always fully adapt, the default).
/// Returns the previous value — save/restore around a transform build.
pub fn set_adaptation_state(state: f64) -> f64 {
    xform::set_adaptation_state(None, state)
}
/// `cmsGetAdaptationState`.
pub fn adaptation_state() -> f64 {
    xform::get_adaptation_state(None)
}
/// `cmsSetAlarmCodes` — the out-of-gamut alarm pixel written by
/// `flags::GAMUT_CHECK` transforms (16 u16s, one per channel).
pub fn set_alarm_codes(codes: [u16; 16]) {
    xform::set_alarm_codes(None, &codes);
}
/// `cmsGetAlarmCodes`.
pub fn alarm_codes() -> [u16; 16] {
    xform::get_alarm_codes(None)
}
/// `cmsxyY2XYZ`.
pub fn xyy_to_xyz(xyy: &CiexyY) -> CieXyz {
    crate::pcs::xyy2xyz(xyy)
}
/// `cmsXYZ2xyY`.
pub fn xyz_to_xyy(xyz: &CieXyz) -> CiexyY {
    crate::pcs::xyz2xyy(xyz)
}
/// `cmsXYZ2Lab` — `None` white point = D50.
pub fn xyz_to_lab(white_point: Option<&CieXyz>, xyz: &CieXyz) -> CieLab {
    crate::pcs::xyz2lab(white_point, xyz)
}
/// `cmsLab2XYZ` — `None` white point = D50.
pub fn lab_to_xyz(white_point: Option<&CieXyz>, lab: &CieLab) -> CieXyz {
    crate::pcs::lab2xyz(white_point, lab)
}
/// `cmsLab2LCh`.
pub fn lab_to_lch(lab: &CieLab) -> CieLCh {
    crate::pcs::lab2lch(lab)
}
/// `cmsLCh2Lab`.
pub fn lch_to_lab(lch: &CieLCh) -> CieLab {
    crate::pcs::lch2lab(lch)
}
/// `cmsFloat2LabEncoded` — Lab -> ICC v4-encoded `u16` triple
/// (the `Format::LAB_16` wire form).
pub fn encode_lab(lab: &CieLab) -> [u16; 3] {
    crate::pcs::float2lab_encoded(lab)
}
/// `cmsLabEncoded2Float` — decode a v4-encoded Lab triple.
pub fn decode_lab(encoded: [u16; 3]) -> CieLab {
    crate::pcs::lab_encoded2float(encoded)
}
/// `cmsFloat2LabEncodedV2` — the ICC v2 encoding.
pub fn encode_lab_v2(lab: &CieLab) -> [u16; 3] {
    crate::pcs::float2lab_encoded_v2(lab)
}
/// `cmsLabEncoded2FloatV2`.
pub fn decode_lab_v2(encoded: [u16; 3]) -> CieLab {
    crate::pcs::lab_encoded2float_v2(encoded)
}
/// `cmsFloat2XYZEncoded` — XYZ -> ICC-encoded `u16` triple.
pub fn encode_xyz(xyz: &CieXyz) -> [u16; 3] {
    crate::pcs::float2xyz_encoded(xyz)
}
/// `cmsXYZEncoded2Float`.
pub fn decode_xyz(encoded: [u16; 3]) -> CieXyz {
    crate::pcs::xyz_encoded2float(encoded)
}
/// `cmsDeltaE` — the plain (CIE76) distance between two Lab colors.
pub fn delta_e(a: &CieLab, b: &CieLab) -> f64 {
    crate::pcs::delta_e(a, b)
}
/// `cmsCIE2000DeltaE` — the ΔE00 distance (`kl`/`kc`/`kh` weightings;
/// `1.0` each for the standard form).
pub fn cie2000_delta_e(a: &CieLab, b: &CieLab, kl: f64, kc: f64, kh: f64) -> f64 {
    crate::pcs::cie2000_delta_e(a, b, kl, kc, kh)
}
/// `cmsCIE94DeltaE` — the ΔE94 distance (graphic-arts weighting).
pub fn cie94_delta_e(a: &CieLab, b: &CieLab) -> f64 {
    crate::pcs::cie94_delta_e(a, b)
}
/// `cmsCMCdeltaE` — the CMC l:c distance (`l=2.0,c=1.0` for perceptibility).
pub fn cmc_delta_e(a: &CieLab, b: &CieLab, l: f64, c: f64) -> f64 {
    crate::pcs::cmc_delta_e(a, b, l, c)
}
/// `cmsBFDdeltaE` — the BFD(l:c) distance.
pub fn bfd_delta_e(a: &CieLab, b: &CieLab) -> f64 {
    crate::pcs::bfd_delta_e(a, b)
}
/// `cmsDesaturateLab` — clamp a*/b* to the `[amin,amax]×[bmin,bmax]` box,
/// preserving hue. Returns `true` if the value changed.
pub fn desaturate_lab(lab: &mut CieLab, amax: f64, amin: f64, bmax: f64, bmin: f64) -> bool {
    crate::sm::desaturate_lab(lab, amax, amin, bmax, bmin)
}
