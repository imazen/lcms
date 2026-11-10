//! `lcms::api` — the idiomatic facade.
//!
//! The `cms*`/`pub mod` surface below (`little-cms-api` feature) stays 1:1
//! with the C source for conformance; this module wraps it in owned,
//! `Result`-returning, `Send`/`Sync`-friendly types for real applications.
//! It is always compiled — no feature required.
//!
//! ## The shared-plan model (for services)
//!
//! The object model is split deliberately for concurrent workloads
//! (image proxies, render pipelines):
//!
//! - **[`Profile`]** — an owned ICC profile. All constructors materialize
//!   every tag up front (`cmsReadTag` on each), so a `Profile` is always
//!   fully readable through `&self` and is `Send` + `Sync`. `Arc`-share
//!   it; [`Profile::tag`], [`Profile::info`], [`Profile::named_colors`]
//!   are lock-free reads.
//! - **[`Plan`]** — the immutable linked transform (`cmsCreateTransform`
//!   plus pipeline optimization + CLUT flattening). `Sync`: build once,
//!   `Arc`-share across workers, cache by
//!   `(src, dst, fmt_in, fmt_out, intent, flags)`.
//! - **[`Scratch`]** — the only per-run state (~64 B pixel cache). Fresh
//!   per call via [`Plan::scratch`], or pooled `thread_local`.
//!
//! A proxy server does: profile cache (`Arc` by content hash) → plan
//! cache (`Arc<Plan>` by config key) → per-request `plan.scratch()` →
//! [`Plan::apply`]. The whole pixel path is `&self` on shared state — no
//! locks anywhere.
//!
//! ```no_run
//! use lcms::api::*;
//!
//! // Open or build profiles — always ready to read (`&self` is Sync).
//! let srgb = Profile::srgb()?;
//! let p3 = Profile::open_file("Display-P3.icc")?;
//!
//! // Link once — the expensive part. `&Profile` borrows are enough;
//! // the plan clones what it needs.
//! let plan = Plan::new(&p3, &srgb, Format::RgbU8, Format::RgbU8)?;
//!
//! // Per-request scratch — ~64 B, or pooled `thread_local`.
//! let mut sc = plan.scratch();
//! let src = [255u8, 0, 0]; let mut dst = [0u8; 3];
//! plan.apply(&mut sc, &src, &mut dst);
//!
//! // Typed pixels work too:
//! plan.apply_pixels(&mut sc, &[[255u8; 3]], &mut [[0u8; 3]]);
//!
//! // Large images: pixel-independent parallel split, scratch-per-chunk.
//! // plan.apply_par(&pixels_in, &mut pixels_out, n_pixels, n_threads);
//! # Ok::<(), lcms::api::Error>(())
//! ```
//!
//! Tags, info text, named colors, header fields:
//!
//! ```
//! use lcms::api::*;
//!
//! let srgb = Profile::srgb()?;
//! for tag in srgb.tags() { println!("{tag}"); }
//! if let Some(TagValue::Xyz(wp)) = srgb.get(Tag::MEDIA_WHITE_POINT) {
//!     println!("white point: {wp:?}");
//! }
//! # Ok::<(), lcms::api::Error>(())
//! ```

use crate::tagtypes::TagObject;
use crate::{named, profile, types, virt, xform};
use std::fmt;
use std::ops::Deref;

// ---------------------------------------------------------------------------
// Re-exports — the api's owned/borrowed vocabulary types.
//
// These live in the parity modules (`pub(crate)` under the default
// surface); re-exporting them here keeps `TagValue`/`TagData` payloads
// nameable (`lcms::api::Pipeline`, `lcms::api::Mlu`, ...) and zero-cost —
// they are the same types, not wrappers.
// ---------------------------------------------------------------------------

/// Borrowed tone curve — the parity layer's `gamma::ToneCurve`.
/// `&api::ToneCurve` derefs to it (the `String` → `str` model).
pub use crate::gamma::ToneCurve as ToneCurveRef;
/// A processing pipeline — `cmsPipeline` (mft/mAB/mBA LUTs, stage chains).
pub use crate::lut::Pipeline;
/// One pipeline stage — `cmsStage` (curves, matrix, CLUT, ...).
pub use crate::lut::Stage;
/// A name/value dictionary — `cmsDICT` (`'dict'` tags).
pub use crate::named::Dict;
/// Multi-language Unicode text — `cmsMLU`.
pub use crate::named::Mlu;
/// A named-color palette — `cmsNAMEDCOLORLIST`.
pub use crate::named::NamedColorList;
/// A profile sequence descriptor — `cmsSEQ` (`'pseq'`/`'psid'` tags).
pub use crate::named::ProfileSeqDesc;
/// An ICC `'data'` tag payload — `cmsICCData`.
pub use crate::tagtypes::IccData;
/// ICC `'view'` viewing conditions — `cmsICCViewingConditions`.
pub use crate::tagtypes::IccViewingConditions;
/// ICC `'MHC2'` display data — `cmsMHC2Type`.
pub use crate::tagtypes::Mhc2;
/// A `struct tm`-shaped calendar time — `'dtim'` tag payload.
pub use crate::tagtypes::Tm;
/// ICC `'bfd '` undercolor-removal/black-generation — `cmsUcrBg`.
pub use crate::tagtypes::UcrBg;
/// The `'vcgt'` payload — `Curves` or a raw `Table`.
pub use crate::tagtypes::VcgtData;
/// ICC `'cicp'` video signal — `cmsVideoSignalType`.
pub use crate::tagtypes::VideoSignal;
/// ICC `'scrn'` halftone screening — `cmsScreening` + `cmsScreeningChannel`.
pub use crate::tagtypes::{Screening, ScreeningChannel};
/// CIE XYZ / xyY / Lab / LCh vocabulary types (`cmsCIE*`).
pub use crate::types::{CieLCh, CieLab, CieXyz, CiexyY, CiexyYTriple};
/// ICC header types — `'dtim'` header date, `cmsProfileID`, `'meas'` conditions.
pub use crate::types::{DateTimeNumber, IccMeasurementConditions, ProfileId};
/// Transform row/plane strides — `cmsStride` ([`Plan::apply_strided`]).
pub use crate::xform::Stride;

// -- Error -----------------------------------------------------------------

/// The api result alias — `Result<T, api::Error>`.
pub type Result<T> = std::result::Result<T, Error>;

/// An lcms failure — the `cmsSignalError` error id plus its message.
///
/// `id` is one of the `cmsERROR_*` codes; [`Error::kind`] maps it to an
/// [`ErrorKind`]. `message` is the real text the failing path signaled
/// (a generic fallback when it didn't signal).
#[derive(Debug, Clone, PartialEq)]
pub struct Error {
    /// `cmsERROR_*` code (`types::ERROR_*`).
    pub id: i32,
    /// The signaled message, or a fallback.
    pub message: String,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "lcms error 0x{:x}: {}", self.id, self.message)
    }
}
impl std::error::Error for Error {}

/// A coarse classification of an [`Error::id`] — the `cmsERROR_*` codes.
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
    /// Build from the last `signal_error` this thread produced, or the
    /// fallback if the failing path didn't signal one.
    pub fn from_last(fallback_id: i32, fallback: &str) -> Self {
        match crate::err::take_last_error() {
            Some((id, message)) => Error { id, message },
            None => Error {
                id: fallback_id,
                message: fallback.into(),
            },
        }
    }
    /// The [`ErrorKind`] for `id`.
    pub fn kind(&self) -> ErrorKind {
        use types as t;
        match self.id {
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

const fn f16(pt: u32, ch: u32) -> u32 {
    pt << 16 | ch << 3 | 2
}
const fn f8(pt: u32, ch: u32) -> u32 {
    pt << 16 | ch << 3 | 1
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

/// Pixel format — the packed `cmsFORMAT` u32 made readable.
///
/// The variants cover the common packed formats; [`Format::Raw`] is the
/// escape hatch for the full `cmsPACK` matrix (planar, swapped,
/// alpha/extra channels, 1-channel Lab, ...) — construct it from any
/// `cmsTYPE_*` code and [`Format::from_u32`] round-trips it.
///
/// ```
/// use lcms::api::Format;
/// let f = Format::RgbU8;
/// assert_eq!(f.channels(), 3);
/// assert_eq!(f.pixel_size(), 3);
/// assert_eq!(Format::from_u32(f.code()), f);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Format {
    /// `cmsTYPE_RGB_8` — 3 × u8.
    RgbU8,
    /// `cmsTYPE_RGB_16` — 3 × u16.
    RgbU16,
    /// `cmsTYPE_RGB_DBL` — 3 × f32.
    RgbF32,
    /// `cmsTYPE_RGBA_8` — 4 × u8 (alpha passthrough on `flags::COPY_ALPHA`).
    RgbaU8,
    /// `cmsTYPE_RGBA_16` — 4 × u16.
    RgbaU16,
    /// `cmsTYPE_RGBA_DBL` — 4 × f32.
    RgbaF32,
    /// `cmsTYPE_Lab_8` — 3 × u8 (v4-encoded Lab).
    LabU8,
    /// `cmsTYPE_Lab_16` — 3 × u16.
    LabU16,
    /// `cmsTYPE_Lab_DBL` — 3 × f32.
    LabF32,
    /// `cmsTYPE_XYZ_16` — 3 × u16.
    XyzU16,
    /// `cmsTYPE_XYZ_DBL` — 3 × f32.
    XyzF32,
    /// `cmsTYPE_GRAY_8` — 1 × u8.
    GrayU8,
    /// `cmsTYPE_GRAY_16` — 1 × u16.
    GrayU16,
    /// `cmsTYPE_GRAY_DBL` — 1 × f32.
    GrayF32,
    /// `cmsTYPE_CMYK_8` — 4 × u8.
    CmykU8,
    /// `cmsTYPE_CMYK_16` — 4 × u16.
    CmykU16,
    /// `cmsTYPE_CMYK_DBL` — 4 × f32.
    CmykF32,
    /// Any other packed format code — the full `cmsPACK` matrix.
    Raw(u32),
}

impl Format {
    /// The packed `cmsFORMAT` code (`cmsTYPE_*`).
    pub fn code(self) -> u32 {
        match self {
            Format::RgbU8 => f8(PT_RGB, 3),
            Format::RgbU16 => f16(PT_RGB, 3),
            Format::RgbF32 => ff32(PT_RGB, 3),
            Format::RgbaU8 => f8e(PT_RGB, 3, 1),
            Format::RgbaU16 => f16e(PT_RGB, 3, 1),
            Format::RgbaF32 => ff32e(PT_RGB, 3, 1),
            Format::LabU8 => f8(PT_LAB, 3),
            Format::LabU16 => f16(PT_LAB, 3),
            Format::LabF32 => ff32(PT_LAB, 3),
            Format::XyzU16 => f16(PT_XYZ, 3),
            Format::XyzF32 => ff32(PT_XYZ, 3),
            Format::GrayU8 => f8(PT_GRAY, 1),
            Format::GrayU16 => f16(PT_GRAY, 1),
            Format::GrayF32 => ff32(PT_GRAY, 1),
            Format::CmykU8 => f8(PT_CMYK, 4),
            Format::CmykU16 => f16(PT_CMYK, 4),
            Format::CmykF32 => ff32(PT_CMYK, 4),
            Format::Raw(c) => c,
        }
    }
    /// Map a packed `cmsFORMAT` code back to a [`Format`] — named variant
    /// if it matches one, `Raw` otherwise.
    pub fn from_u32(code: u32) -> Format {
        const ALL: &[Format] = &[
            Format::RgbU8,
            Format::RgbU16,
            Format::RgbF32,
            Format::RgbaU8,
            Format::RgbaU16,
            Format::RgbaF32,
            Format::LabU8,
            Format::LabU16,
            Format::LabF32,
            Format::XyzU16,
            Format::XyzF32,
            Format::GrayU8,
            Format::GrayU16,
            Format::GrayF32,
            Format::CmykU8,
            Format::CmykU16,
            Format::CmykF32,
        ];
        ALL.iter()
            .copied()
            .find(|f| f.code() == code)
            .unwrap_or(Format::Raw(code))
    }
    /// Bytes per pixel (packed, no stride) — `(channels + extra) ×
    /// bytes-per-channel`. 0-byte formats (e.g. floating "any") count
    /// as 8/channel like lcms does.
    pub fn pixel_size(self) -> usize {
        let c = self.code();
        let b = crate::pack::t_bytes(c);
        let b = if b == 0 { 8 } else { b };
        (crate::pack::t_channels(c) + crate::pack::t_extra(c)) as usize * b as usize
    }
    /// Bytes per channel — 1, 2 or 4 (0 for exotic "any" formats).
    pub fn bytes_per_channel(self) -> usize {
        crate::pack::t_bytes(self.code()) as usize
    }
    /// Color channels — not counting alpha/extra.
    pub fn channels(self) -> usize {
        crate::pack::t_channels(self.code()) as usize
    }
    /// Extra (alpha / spot) channels.
    pub fn extra_channels(self) -> usize {
        crate::pack::t_extra(self.code()) as usize
    }
    /// `channels + extra_channels` — total components per pixel.
    pub fn total_channels(self) -> usize {
        self.channels() + self.extra_channels()
    }
    /// Floating-point format (`T_FLOAT`).
    pub fn is_float(self) -> bool {
        crate::pack::t_float(self.code()) != 0
    }
    /// Planar (separate-per-channel planes) format.
    pub fn is_planar(self) -> bool {
        crate::pack::t_planar(self.code()) != 0
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
impl fmt::Display for Format {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Format::Raw(c) => write!(f, "Raw(0x{c:x})"),
            other => fmt::Debug::fmt(other, f),
        }
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

/// Rendering intent (`cmsIntents.h` — `cmsINTENT_*`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Intent {
    /// `cmsINTENT_PERCEPTUAL` — gamut compressed smoothly (the default).
    #[default]
    Perceptual,
    /// `cmsINTENT_RELATIVE_COLORIMETRIC` — clip out-of-gamut, adapt white.
    RelativeColorimetric,
    /// `cmsINTENT_SATURATION` — preserve saturation.
    Saturation,
    /// `cmsINTENT_ABSOLUTE_COLORIMETRIC` — clip, keep paper white.
    AbsoluteColorimetric,
}
impl Intent {
    /// The `cmsINTENT_*` code (0–3).
    pub fn code(self) -> u32 {
        self as u32
    }
    /// `u32` -> `Intent` — clamps out-of-range values like C does.
    pub fn from_u32(v: u32) -> Intent {
        match v {
            0 => Intent::Perceptual,
            1 => Intent::RelativeColorimetric,
            2 => Intent::Saturation,
            _ => Intent::AbsoluteColorimetric,
        }
    }
    /// All four intents, in code order.
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
impl From<u32> for Intent {
    fn from(v: u32) -> Intent {
        Intent::from_u32(v)
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
/// # let srgb = Profile::srgb()?;
/// # let lab = Profile::lab4()?;
/// let plan = Plan::build(
///     &srgb, &lab, Format::RgbU8, Format::LabU8,
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
/// let p = Profile::srgb()?;
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
    fn wrap(icc: Option<Box<crate::profile::IccProfile>>, what: &str) -> Result<Self> {
        let mut icc =
            icc.ok_or_else(|| Error::from_last(types::ERROR_READ, &format!("{what} failed")))?;
        // Eager materialization — `&Profile` reads are always valid.
        profile::freeze(&mut icc);
        Ok(Profile { icc })
    }

    // -- constructors -----------------------------------------------------

    /// `cmsCreate_sRGBProfile` — the standard sRGB (D65, Rec.709 primaries).
    pub fn srgb() -> Result<Self> {
        Self::wrap(virt::create_srgb_profile(None), "create_srgb_profile")
    }
    /// `cmsCreateLab4Profile` — Lab PCS profile (ICC v4 encoding).
    pub fn lab4() -> Result<Self> {
        Self::wrap(virt::create_lab4_profile(None, None), "create_lab4_profile")
    }
    /// `cmsCreateLab2Profile` — Lab PCS profile (ICC v2 encoding).
    pub fn lab2() -> Result<Self> {
        Self::wrap(virt::create_lab2_profile(None, None), "create_lab2_profile")
    }
    /// `cmsCreateGrayProfile` with D50 white and a 2.2-gamma TRC —
    /// see [`Profile::gray_curve`] for a custom curve.
    pub fn gray() -> Result<Self> {
        let wp = CiexyY {
            x: 0.3457,
            y: 0.3585,
            Y: 1.0,
        }; // D50
        let g = crate::gamma::build_gamma(None, 2.2);
        Self::wrap(
            virt::create_gray_profile(None, Some(&wp), g.as_ref()),
            "create_gray_profile",
        )
    }
    /// `cmsCreateXYZProfile` — XYZ PCS profile.
    pub fn xyz() -> Result<Self> {
        Self::wrap(virt::create_xyz_profile(None), "create_xyz_profile")
    }
    /// `cmsCreateNULLProfile` — the identity passthrough.
    pub fn null() -> Result<Self> {
        Self::wrap(virt::create_null_profile(None), "create_null_profile")
    }
    /// `cmsCreate_OkLabProfile` — OkLab color-space profile (MPE pipeline;
    /// cannot be saved to an ICC file, like C).
    pub fn oklab() -> Result<Self> {
        Self::wrap(virt::create_oklab_profile(None), "create_oklab_profile")
    }
    /// `cmsCreateProfilePlaceholder` — a bare profile shell for authoring
    /// (`'1CLR'` color space, `'XYZ '` PCS; set them with
    /// [`Profile::set_color_space`]/[`Profile::set_device_class`]).
    pub fn placeholder() -> Self {
        let mut icc = crate::profile::IccProfile::default();
        icc.color_space = crate::signatures::colorspace::Sig1colorData;
        icc.pcs = crate::signatures::colorspace::SigXYZData;
        Profile { icc: Box::new(icc) }
    }
    /// `cmsCreateRGBProfile` — white point, primaries, transfer curves.
    ///
    /// ```
    /// use lcms::api::*;
    /// let c = ToneCurve::gamma(2.2)?;
    /// let p = Profile::rgb(
    ///     &CiexyY { x: 0.3127, y: 0.3290, Y: 1.0 },           // D65
    ///     &CiexyYTriple {
    ///         Red:   CiexyY { x: 0.6400, y: 0.3300, Y: 1.0 },
    ///         Green: CiexyY { x: 0.3000, y: 0.6000, Y: 1.0 },
    ///         Blue:  CiexyY { x: 0.1500, y: 0.0600, Y: 1.0 },
    ///     },
    ///     [&c, &c, &c],
    /// )?;
    /// # Ok::<(), lcms::api::Error>(())
    /// ```
    pub fn rgb(
        white_point: &CiexyY,
        primaries: &CiexyYTriple,
        curves: [&ToneCurve; 3],
    ) -> Result<Self> {
        Self::wrap(
            virt::create_rgb_profile(
                None,
                Some(white_point),
                Some(primaries),
                Some([&curves[0].0, &curves[1].0, &curves[2].0]),
            ),
            "create_rgb_profile",
        )
    }
    /// `cmsCreateGrayProfile` — white point + a custom tone curve.
    pub fn gray_curve(white_point: &CiexyY, curve: &ToneCurve) -> Result<Self> {
        Self::wrap(
            virt::create_gray_profile(None, Some(white_point), Some(&curve.0)),
            "create_gray_profile",
        )
    }
    /// `cmsCreateLinearizationDeviceLink` — a device-link profile whose
    /// `A2B0` pipeline is one tone-curve stage per channel.
    pub fn linearization_device_link(
        color_space: ColorSpace,
        curves: &[ToneCurve],
    ) -> Result<Self> {
        let refs: Vec<&ToneCurveRef> = curves.iter().map(|c| &c.0).collect();
        Self::wrap(
            virt::create_linearization_device_link(None, color_space.code(), &refs),
            "create_linearization_device_link",
        )
    }
    /// `cmsCreateInkLimitingDeviceLink` — a CMYK→CMYK device link that
    /// caps total ink coverage (`limit` %, e.g. `250.0`).
    pub fn ink_limiting_device_link(color_space: ColorSpace, limit: f64) -> Result<Self> {
        Self::wrap(
            virt::create_ink_limiting_device_link(None, color_space.code(), limit),
            "create_ink_limiting_device_link",
        )
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
        Self::wrap(
            virt::create_bchsw_abstract_profile(
                None, lut_points, brightness, contrast, hue, saturation, temp_src, temp_dest,
            ),
            "create_bchsw_abstract_profile",
        )
    }
    /// `cmsOpenProfileFromMem` — parse an ICC profile from bytes.
    /// Materializes every tag eagerly (see the type docs).
    pub fn open(data: &[u8]) -> Result<Self> {
        Self::wrap(
            profile::open_profile_from_mem(None, data),
            "open_profile_from_mem",
        )
    }
    /// `cmsOpenProfileFromFile` — read an ICC file off disk.
    pub fn open_file(path: impl AsRef<std::path::Path>) -> Result<Self> {
        let data = std::fs::read(path.as_ref()).map_err(|e| Error {
            id: types::ERROR_FILE,
            message: e.to_string(),
        })?;
        Self::open(&data)
    }

    // -- serialization ------------------------------------------------------

    /// `cmsSaveProfileToMem` — the serialized ICC bytes. `&self`: the
    /// profile is saved unchanged (the encoder works on a clone).
    pub fn save(&self) -> Result<Vec<u8>> {
        let mut icc = self.icc.clone();
        profile::save_profile_to_mem(&mut icc)
            .ok_or_else(|| Error::from_last(types::ERROR_WRITE, "save_profile_to_mem failed"))
    }
    /// `cmsSaveProfileToFile` — write the serialized ICC bytes to disk.
    pub fn save_file(&self, path: impl AsRef<std::path::Path>) -> Result<()> {
        let bytes = self.save()?;
        std::fs::write(path.as_ref(), bytes).map_err(|e| Error {
            id: types::ERROR_WRITE,
            message: e.to_string(),
        })
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
    /// Write a tag — stores the [`TagData`] (converted to the cooked
    /// `TagObject`). `false` if the tag can't be written (e.g. the
    /// descriptor table rejects the pairing).
    pub fn write_tag(&mut self, tag: impl Into<Tag>, value: TagData<'_>) -> bool {
        let obj = match value {
            TagData::Xyz(x) => TagObject::Xyz(x),
            TagData::Chromaticity(c) => TagObject::Chromaticity(c),
            TagData::ColorantOrder(o) => TagObject::ColorantOrder(o),
            TagData::Curve(c) => TagObject::Curve(c.0),
            TagData::ParametricCurve(c) => TagObject::ParametricCurve(c.0),
            TagData::Text(s) => {
                let mut m = named::mlu_alloc(None, 1).unwrap_or_default();
                named::mlu_set_utf8(&mut m, &[0; 3], &[0; 3], s.as_bytes());
                TagObject::Mlu(m)
            }
            TagData::Mlu(m) => TagObject::Mlu(m),
            TagData::NamedColors(l) => TagObject::NamedColor(l),
            TagData::Pipeline(p) => TagObject::Lut16(p),
            TagData::Data(d) => TagObject::Data(d),
            TagData::Signature(s) => TagObject::Signature(s),
            TagData::DateTime(t) => TagObject::DateTime(t),
            TagData::Measurement(m) => TagObject::Measurement(m),
            TagData::ViewingConditions(v) => TagObject::ViewingConditions(v),
            TagData::UcrBg(u) => TagObject::UcrBg(u),
            TagData::Screening(s) => TagObject::Screening(s),
            TagData::VideoSignal(v) => TagObject::VideoSignal(v),
            TagData::Mhc2(m) => TagObject::Mhc2(m),
            TagData::VcgtCurves([r, g, b]) => TagObject::Vcgt(VcgtData::Curves([r.0, g.0, b.0])),
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
        crate::profile::cms_write_tag(&mut self.icc, tag.into().0, Some(obj))
    }
    /// Remove a tag from the directory.
    pub fn remove_tag(&mut self, tag: impl Into<Tag>) {
        crate::profile::cms_write_tag(&mut self.icc, tag.into().0, None);
    }
    /// `cmsIsTag` — a directory entry exists (whether or not it decoded).
    pub fn has_tag(&self, tag: impl Into<Tag>) -> bool {
        crate::profile::cms_is_tag(&self.icc, tag.into().0)
    }
    /// The named-color palette (`'ncl2'`/`'clrt'`), if present.
    pub fn named_colors(&self) -> Option<NamedColors<'_>> {
        for sig in [
            crate::signatures::tag::SigNamedColor2Tag,
            crate::signatures::tag::SigColorantTableTag,
        ] {
            match profile::read_tag_frozen(&self.icc, sig) {
                Some(TagObject::NamedColor(l)) | Some(TagObject::ColorantTable(l)) => {
                    return Some(NamedColors(l));
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
    /// let p = Profile::srgb()?;
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
    pub fn set_info(&mut self, kind: InfoType, text: &str) -> bool {
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
    /// `cmsGetHeaderRenderingIntent`.
    pub fn header_intent(&self) -> Intent {
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
/// let srgb = Profile::srgb()?;
/// let lab = Profile::lab4()?;
/// let plan = Plan::new(&srgb, &lab, Format::RgbU8, Format::LabU8)?;
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

impl Plan {
    /// `cmsCreateTransform` — link `src -> dst`, perceptual intent, no
    /// flags. For intent/flags see [`Plan::build`].
    pub fn new(src: &Profile, dst: &Profile, fmt_in: Format, fmt_out: Format) -> Result<Self> {
        Self::build(src, dst, fmt_in, fmt_out, Intent::Perceptual, 0)
    }

    /// `cmsCreateExtendedTransform` — link `src -> dst` with intent + flags.
    pub fn build(
        src: &Profile,
        dst: &Profile,
        fmt_in: Format,
        fmt_out: Format,
        intent: Intent,
        flags: u32,
    ) -> Result<Self> {
        // create_extended_transform takes &mut [Box<IccProfile>] — work on
        // clones so `&Profile` suffices (lazy tag reads materialize on the
        // clone; the caller's profile is never touched).
        let mut profs = [src.icc.clone(), dst.icc.clone()];
        let mut bpc = [false, false];
        let intents = [intent.code(); 2];
        let states = [3.0f64; 2];
        let t = xform::create_extended_transform(
            None,
            &mut profs,
            &mut bpc,
            &intents,
            &states,
            None,
            0,
            fmt_in.code(),
            fmt_out.code(),
            flags,
        );
        let t =
            t.ok_or_else(|| Error::from_last(types::ERROR_UNDEFINED, "create_transform failed"))?;
        let xform::Transform { plan, .. } = *t;
        Ok(Plan { inner: plan })
    }

    /// `cmsCreateProofingTransform` — `input -> proofing -> output`.
    ///
    /// `proofing_intent` drives the middle hop; `flags::GAMUT_CHECK`
    /// additionally enables the gamut-alarm pipeline (out-of-gamut pixels
    /// come out as the alarm codes — see [`set_alarm_codes`]).
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
        let mut gamut = proofing.icc.clone();
        let mut profs = [input.icc.clone(), proofing.icc.clone(), output.icc.clone()];
        let mut bpc = [false; 3];
        let intents = [intent.code(), proofing_intent.code(), intent.code()];
        let states = [3.0f64; 3];
        let t = xform::create_extended_transform(
            None,
            &mut profs,
            &mut bpc,
            &intents,
            &states,
            Some(&mut gamut),
            1, // the proofing profile's position in the chain
            fmt_in.code(),
            fmt_out.code(),
            flags,
        );
        let t = t.ok_or_else(|| {
            Error::from_last(types::ERROR_UNDEFINED, "create_proofing_transform failed")
        })?;
        let xform::Transform { plan, .. } = *t;
        Ok(Plan { inner: plan })
    }

    /// `cmsCreateMultiprofileTransform` — a chain of N profiles
    /// (`input -> [intermediates] -> output`). At least two profiles.
    pub fn new_multiprofile(
        profiles: &[&Profile],
        fmt_in: Format,
        fmt_out: Format,
        intent: Intent,
        flags: u32,
    ) -> Result<Self> {
        let mut profs: Vec<Box<crate::profile::IccProfile>> =
            profiles.iter().map(|p| p.icc.clone()).collect();
        let n = profs.len();
        let mut bpc = vec![false; n];
        let intents = vec![intent.code(); n];
        let states = vec![3.0f64; n];
        let t = xform::create_extended_transform(
            None,
            &mut profs,
            &mut bpc,
            &intents,
            &states,
            None,
            0,
            fmt_in.code(),
            fmt_out.code(),
            flags,
        );
        let t = t.ok_or_else(|| {
            Error::from_last(
                types::ERROR_UNDEFINED,
                "create_multiprofile_transform failed",
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
        let mut icc = virt::transform2_device_link(&mut xf, version, flags)
            .ok_or_else(|| Error::from_last(types::ERROR_WRITE, "transform2_device_link failed"))?;
        profile::freeze(&mut icc);
        Ok(Profile { icc })
    }

    // -- accessors --------------------------------------------------------------

    /// The input pixel format (as linked — the optimizer may have
    /// rewritten it; see `Raw` for exotic codes).
    pub fn input_format(&self) -> Format {
        Format::from_u32(self.inner.input_format)
    }
    /// The output pixel format.
    pub fn output_format(&self) -> Format {
        Format::from_u32(self.inner.output_format)
    }
    /// The rendering intent the plan was linked with.
    pub fn intent(&self) -> Intent {
        Intent::from_u32(self.inner.rendering_intent)
    }
    /// The `flags` the plan was built with.
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
    pub fn input_colorants(&self) -> Option<NamedColors<'_>> {
        self.inner.input_colorants.as_ref().map(NamedColors)
    }
    /// `cmsGetTransformOutputColorantTable`.
    pub fn output_colorants(&self) -> Option<NamedColors<'_>> {
        self.inner.output_colorants.as_ref().map(NamedColors)
    }

    // -- evaluation ---------------------------------------------------------------

    /// Fresh per-run scratch — clones the seeded cache (~64 B).
    pub fn scratch(&self) -> Scratch {
        Scratch(self.inner.scratch())
    }

    /// Evaluate the whole `input` -> `output` as pixels — `cmsDoTransform`.
    /// Pixel count is derived: `min(input.len() / in_px, output.len() /
    /// out_px)`. `&self` — share one `Plan` across threads, with a
    /// [`Scratch`] per call.
    pub fn apply(&self, scratch: &mut Scratch, input: &[u8], output: &mut [u8]) {
        let pi = Format::from_u32(self.inner.input_format)
            .pixel_size()
            .max(1);
        let po = Format::from_u32(self.inner.output_format)
            .pixel_size()
            .max(1);
        let n = (input.len() / pi).min(output.len() / po) as u32;
        self.inner.apply(&mut scratch.0, input, output, n);
    }
    /// Evaluate exactly `n` pixels — the caller guarantees both buffers
    /// hold `n × pixel_size` bytes.
    pub fn apply_n(&self, scratch: &mut Scratch, input: &[u8], output: &mut [u8], n: u32) {
        self.inner.apply(&mut scratch.0, input, output, n);
    }
    /// Typed-pixel apply — `&[I]` -> `&mut [O]` via `bytemuck` casts.
    /// `I`/`O` are any `Pod` pixel types (`[u8; 3]`, `[u16; 4]`, `f32`,
    /// `RGB<u8>`, ...); each element is one pixel. Applies
    /// `min(input.len(), output.len())` pixels.
    ///
    /// ```text
    /// plan.apply_pixels(&mut sc, &[[255u8; 3]; N], &mut [[0u8; 3]; N]);
    /// ```
    pub fn apply_pixels<I: bytemuck::Pod, O: bytemuck::Pod>(
        &self,
        scratch: &mut Scratch,
        input: &[I],
        output: &mut [O],
    ) {
        debug_assert_eq!(
            std::mem::size_of::<I>(),
            Format::from_u32(self.inner.input_format).pixel_size(),
            "input pixel type doesn't match the plan's input format",
        );
        debug_assert_eq!(
            std::mem::size_of::<O>(),
            Format::from_u32(self.inner.output_format).pixel_size(),
            "output pixel type doesn't match the plan's output format",
        );
        let n = input.len().min(output.len()) as u32;
        self.inner.apply(
            &mut scratch.0,
            bytemuck::cast_slice(input),
            bytemuck::cast_slice_mut(output),
            n,
        );
    }
    /// In-place apply — `input_format` and `output_format` must carry the
    /// same pixel size. Implemented via a temporary copy (transforms are
    /// pixel-independent), so `buf` may alias either side. Returns `false`
    /// without touching `buf` when the formats can't overlap.
    pub fn apply_in_place(&self, scratch: &mut Scratch, buf: &mut [u8]) -> bool {
        let pi = Format::from_u32(self.inner.input_format).pixel_size();
        let po = Format::from_u32(self.inner.output_format).pixel_size();
        if pi == 0 || pi != po {
            return false;
        }
        let tmp = buf.to_vec();
        let n = buf.len() / pi;
        self.inner.apply(&mut scratch.0, &tmp, buf, n as u32);
        true
    }
    /// `cmsDoTransformLineStride` — `pixels_per_line × lines` with
    /// per-plane line strides ([`Stride`]). For chunky formats
    /// `bytes_per_line_*` is the row advance; `bytes_per_plane_*` is the
    /// plane advance used by planar formats.
    pub fn apply_strided(
        &self,
        scratch: &mut Scratch,
        input: &[u8],
        output: &mut [u8],
        pixels_per_line: u32,
        lines: u32,
        stride: &Stride,
    ) {
        self.inner.apply_strided(
            &mut scratch.0,
            input,
            output,
            pixels_per_line,
            lines,
            stride,
        );
    }
    /// Parallel apply — splits the contiguous pixel range into `threads`
    /// chunks, each evaluated on its own fresh [`Scratch`]
    /// (`&self` is `Sync`). `threads <= 1` or tiny `n` falls back to
    /// serial. Uses `std::thread` — one spawn per call; for hot loops
    /// keep worker threads and call [`Plan::apply`] per chunk instead.
    pub fn apply_par(&self, input: &[u8], output: &mut [u8], n: u32, threads: usize) {
        self.inner.apply_par(input, output, n, threads);
    }
}

/// `Stride` helpers — `Stride { bytes_per_line_in, .. }` has pub fields;
/// this adds constructors.
impl Stride {
    /// A fully explicit stride (`bytes_per_line_in`, `bytes_per_line_out`,
    /// `bytes_per_plane_in`, `bytes_per_plane_out` — C's
    /// `cmsDoTransformLineStride` argument order).
    pub const fn new(
        bytes_per_line_in: u32,
        bytes_per_line_out: u32,
        bytes_per_plane_in: u32,
        bytes_per_plane_out: u32,
    ) -> Stride {
        Stride {
            bytes_per_line_in,
            bytes_per_line_out,
            bytes_per_plane_in,
            bytes_per_plane_out,
        }
    }
    /// Chunky rows with a byte stride — `bytes_per_line` for both
    /// directions; plane strides set to `0` (contiguous-per-line like
    /// `cmsDoTransformStride`).
    pub const fn chunky(bytes_per_line: u32) -> Stride {
        Stride {
            bytes_per_line_in: bytes_per_line,
            bytes_per_line_out: bytes_per_line,
            bytes_per_plane_in: 0,
            bytes_per_plane_out: 0,
        }
    }
}

// -- ToneCurve ------------------------------------------------------------------

/// An owned ICC tone curve (gamma/parametric/tabulated) — `Clone`.
///
/// `&ToneCurve` derefs to [`ToneCurveRef`] (the parity `gamma::ToneCurve`),
/// so the full read API — `eval16`, `eval_float`, `estimated_table`,
/// `parametric_type`, `segment(n)` — is available directly; the methods
/// here are the idiomatic front.
///
/// ```
/// use lcms::api::ToneCurve;
/// let srgb = ToneCurve::srgb()?;
/// assert_eq!(srgb.eval16(0), 0);
/// assert_eq!(srgb.eval16(0xffff), 0xffff);
/// assert!(srgb.is_monotonic());
/// # Ok::<(), lcms::api::Error>(())
/// ```
#[derive(Clone)]
pub struct ToneCurve(pub(crate) crate::gamma::ToneCurve);
impl Deref for ToneCurve {
    type Target = ToneCurveRef;
    fn deref(&self) -> &ToneCurveRef {
        &self.0
    }
}
impl fmt::Debug for ToneCurve {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ToneCurve")
            .field("parametric_type", &self.0.parametric_type())
            .field("estimated_gamma", &self.estimated_gamma())
            .field("entries", &self.0.estimated_table_entries())
            .finish()
    }
}
impl ToneCurve {
    /// `cmsBuildGamma` — a plain `v^g` curve.
    pub fn gamma(g: f64) -> Result<Self> {
        crate::gamma::build_gamma(None, g)
            .map(ToneCurve)
            .ok_or_else(|| Error::from_last(types::ERROR_UNDEFINED, "build_gamma failed"))
    }
    /// A linear (identity) curve — `gamma(1.0)`.
    pub fn linear() -> Result<Self> {
        Self::gamma(1.0)
    }
    /// The standard sRGB tone curve — `cmsBuildParametricToneCurve` type 4
    /// (`2.4` gamma + linear toe), the same curve `Profile::srgb` carries.
    pub fn srgb() -> Result<Self> {
        Self::parametric(4, &[2.4, 1.0 / 1.055, 0.055 / 1.055, 1.0 / 12.92, 0.04045])
    }
    /// `cmsBuildTabulatedToneCurve16` — a `u16` interpolated table.
    pub fn tabulated16(values: &[u16]) -> Result<Self> {
        crate::gamma::build_tabulated_tone_curve16(None, values.len() as u32, Some(values))
            .map(ToneCurve)
            .ok_or_else(|| {
                Error::from_last(
                    types::ERROR_UNDEFINED,
                    "build_tabulated_tone_curve16 failed",
                )
            })
    }
    /// `cmsBuildTabulatedToneCurveFloat` — an `f32` interpolated table.
    pub fn tabulated_f32(values: &[f32]) -> Result<Self> {
        crate::gamma::build_tabulated_tone_curve_float(None, values)
            .map(ToneCurve)
            .ok_or_else(|| {
                Error::from_last(
                    types::ERROR_UNDEFINED,
                    "build_tabulated_tone_curve_float failed",
                )
            })
    }
    /// `cmsBuildParametricToneCurve` — `type` + `params` (e.g. type 1
    /// pure gamma: `[g]`; type 4 sRGB: `[g, a, b, c, d]`).
    pub fn parametric(ty: i16, params: &[f64]) -> Result<Self> {
        crate::gamma::build_parametric_tone_curve(None, ty as i32, params)
            .map(ToneCurve)
            .ok_or_else(|| {
                Error::from_last(types::ERROR_UNDEFINED, "build_parametric_tone_curve failed")
            })
    }
    /// `cmsEvalToneCurve16`.
    pub fn eval16(&self, v: u16) -> u16 {
        self.0.eval16(v)
    }
    /// `cmsEvalToneCurveFloat`.
    pub fn eval_f32(&self, v: f32) -> f32 {
        self.0.eval_float(v)
    }
    /// `cmsEstimateGamma` — the effective single-gamma fit at 0.1%
    /// precision (`None` when the curve can't be fit — e.g. multisegment).
    pub fn estimated_gamma(&self) -> Option<f64> {
        self.estimated_gamma_p(0.001)
    }
    /// [`ToneCurve::estimated_gamma`] with a precision parameter
    /// (`cmsEstimateGamma(curve, precision)`).
    pub fn estimated_gamma_p(&self, precision: f64) -> Option<f64> {
        let g = crate::gamma::estimate_gamma(&self.0, precision);
        if g < 0.0 { None } else { Some(g) }
    }
    /// `cmsIsToneCurveLinear`.
    pub fn is_linear(&self) -> bool {
        crate::gamma::is_tone_curve_linear(&self.0)
    }
    /// `cmsIsToneCurveMonotonic`.
    pub fn is_monotonic(&self) -> bool {
        crate::gamma::is_tone_curve_monotonic(&self.0)
    }
    /// `cmsIsToneCurveDescending`.
    pub fn is_descending(&self) -> bool {
        crate::gamma::is_tone_curve_descending(&self.0)
    }
    /// `cmsIsToneCurveMultisegment`.
    pub fn is_multisegment(&self) -> bool {
        crate::gamma::is_tone_curve_multisegment(&self.0)
    }
    /// `cmsGetToneCurveEstimatedTable` — the precomputed eval table.
    pub fn estimated_table(&self) -> &[u16] {
        self.0.estimated_table()
    }
    /// `cmsGetToneCurveParametricType` — the parametric function number
    /// (`<= 0` for tabulated/segmented curves).
    pub fn parametric_type(&self) -> i32 {
        self.0.parametric_type()
    }
    /// `cmsGetToneCurveParametricParams` — the segment params.
    pub fn segments(&self) -> &[crate::gamma::CurveSegment] {
        &self.0.segments
    }
    /// `cmsReverseToneCurve` — the inverse curve (round-trips monotonic
    /// curves; falls back to a copy on failure, like C).
    pub fn reversed(&self) -> ToneCurve {
        ToneCurve(crate::gamma::reverse_tone_curve(&self.0).unwrap_or_else(|| self.0.clone()))
    }
    /// `cmsReverseToneCurveEx` — reversed, sampled at `samples` points.
    pub fn reversed_samples(&self, samples: usize) -> ToneCurve {
        ToneCurve(
            crate::gamma::reverse_tone_curve_ex(samples as u32, &self.0)
                .unwrap_or_else(|| self.0.clone()),
        )
    }
    /// `cmsJoinToneCurve` — `self⁻¹(other(t))` composed, `points` samples.
    pub fn join(&self, other: &ToneCurve, points: usize) -> ToneCurve {
        ToneCurve(
            crate::gamma::join_tone_curve(None, &self.0, &other.0, points as u32)
                .unwrap_or_else(|| self.0.clone()),
        )
    }
    /// `cmsSmoothToneCurve` — smooth in place (`lambda` = smoothing
    /// factor; returns `false` if smoothing isn't applicable).
    pub fn smooth(&mut self, lambda: f64) -> bool {
        crate::gamma::smooth_tone_curve(None, &mut self.0, lambda)
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
    Curve(&'a ToneCurveRef),
    /// Multi-language text (description/copyright/text/desc/crdi).
    Text(&'a Mlu),
    /// A named-color palette (`'ncl2'`/`'clrt'`).
    NamedColors(&'a NamedColorList),
    /// A processing pipeline (mft1/mft2/mAB/mBA/mpet LUTs).
    Pipeline(&'a Pipeline),
    /// A lone processing stage (MPE element payload).
    Stage(&'a Stage),
    /// The 3-curve `'vcgt'` tag (video card gamma).
    VcgtCurves([&'a ToneCurveRef; 3]),
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
    DateTime(&'a Tm),
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
            TagObject::DateTime(t) => TagValue::DateTime(t),
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
    pub fn as_curve(&self) -> Option<&'a ToneCurveRef> {
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
/// p.set_info(InfoType::Description, "My display");
/// p.write_tag(Tag::MEDIA_WHITE_POINT, TagData::Xyz(d50_xyz()));
/// assert!(p.has_tag(Tag::PROFILE_DESCRIPTION));
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
    DateTime(Tm),
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

// -- named colors --------------------------------------------------------------

/// A borrowed view of a profile's named-color palette — zero-cost.
///
/// ```
/// use lcms::api::*;
/// # let mut p = Profile::placeholder();
/// # let mut list = NamedColorList::new(1, 3, "", "")?;
/// # list.push("spot", [0xffff, 0x8000, 0x2000], &[0xffff, 0, 0, 0,0,0,0,0,0,0,0,0,0,0,0,0]);
/// # p.write_tag(Tag::NAMED_COLOR2, TagData::NamedColors(list));
/// let ncl = p.named_colors().unwrap();
/// for c in &ncl { println!("{} -> {:?}", c.name, c.pcs); }
/// assert_eq!(ncl.position("spot"), Some(0));
/// # Ok::<(), lcms::api::Error>(())
/// ```
pub struct NamedColors<'a>(&'a NamedColorList);
impl<'a> NamedColors<'a> {
    /// Number of entries.
    pub fn len(&self) -> usize {
        self.0.n_colors as usize
    }
    /// `true` when the palette is empty.
    pub fn is_empty(&self) -> bool {
        self.0.n_colors == 0
    }
    /// The `index`-th entry (`None` if out of bounds).
    pub fn get(&self, index: usize) -> Option<NamedColor> {
        // bound by n_colors — `list` holds `allocated` blank entries beyond it
        if index >= self.0.n_colors as usize {
            return None;
        }
        let c = self.0.list.get(index)?;
        let name_end = c.name.iter().position(|&b| b == 0).unwrap_or(c.name.len());
        Some(NamedColor {
            name: String::from_utf8_lossy(&c.name[..name_end]).into_owned(),
            pcs: c.pcs,
            colorant: c.device_colorant[..self.0.colorant_count as usize].to_vec(),
        })
    }
    /// `cmsNamedColorIndex` — first index whose name matches (0-based).
    pub fn position(&self, name: &str) -> Option<usize> {
        (0..self.len()).find(|&i| self.get(i).is_some_and(|c| c.name == name))
    }
    /// The list-wide prefix string (`cmsNAMEDCOLORLIST` `Prefix`).
    pub fn prefix(&self) -> String {
        cstr(&self.0.prefix)
    }
    /// The list-wide suffix string.
    pub fn suffix(&self) -> String {
        cstr(&self.0.suffix)
    }
    /// Device colorant channels per entry.
    pub fn colorant_count(&self) -> usize {
        self.0.colorant_count as usize
    }
    /// Iterate the entries.
    pub fn iter(&self) -> NamedColorsIter<'_> {
        NamedColorsIter { list: self, i: 0 }
    }
}
impl<'a> IntoIterator for &'a NamedColors<'a> {
    type Item = NamedColor;
    type IntoIter = NamedColorsIter<'a>;
    fn into_iter(self) -> NamedColorsIter<'a> {
        self.iter()
    }
}
/// Iterator over a [`NamedColors`] palette — see [`NamedColors::iter`].
pub struct NamedColorsIter<'a> {
    list: &'a NamedColors<'a>,
    i: usize,
}
impl Iterator for NamedColorsIter<'_> {
    type Item = NamedColor;
    fn next(&mut self) -> Option<NamedColor> {
        let c = self.list.get(self.i)?;
        self.i += 1;
        Some(c)
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        let left = self.list.len().saturating_sub(self.i);
        (left, Some(left))
    }
}
impl ExactSizeIterator for NamedColorsIter<'_> {}
impl std::iter::FusedIterator for NamedColorsIter<'_> {}

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

/// `Pipeline` — construct + author + iterate (`cmsPipeline*`).
impl Pipeline {
    /// `cmsPipelineAlloc` — an empty pipeline (`in` → `out` channels).
    pub fn new(input_channels: u32, output_channels: u32) -> Result<Pipeline> {
        crate::lut::pipeline_alloc(None, input_channels, output_channels)
            .ok_or_else(|| Error::from_last(types::ERROR_UNDEFINED, "pipeline_alloc failed"))
    }
    /// `cmsPipelineInsertStage(.., cmsAtEnd)` — append a stage.
    pub fn push(&mut self, stage: Stage) -> bool {
        self.insert_stage(crate::lut::StageLoc::AtEnd, stage)
    }
    /// `cmsPipelineInsertStage(.., cmsAtBegin)` — prepend a stage.
    pub fn push_front(&mut self, stage: Stage) -> bool {
        self.insert_stage(crate::lut::StageLoc::AtBegin, stage)
    }
    /// Iterate the stage chain head -> tail.
    pub fn stages(&self) -> StagesIter<'_> {
        StagesIter(self.elements.as_deref())
    }
}

/// Stage-chain iterator — see [`Pipeline::stages`].
pub struct StagesIter<'a>(Option<&'a Stage>);
impl<'a> Iterator for StagesIter<'a> {
    type Item = &'a Stage;
    fn next(&mut self) -> Option<&'a Stage> {
        let s = self.0?;
        self.0 = s.next.as_deref();
        Some(s)
    }
}
impl std::iter::FusedIterator for StagesIter<'_> {}

/// `Stage` — the `cmsStageAlloc*` constructors.
impl Stage {
    /// `cmsStageAllocIdentity` — pass-through.
    pub fn identity(n_channels: u32) -> Stage {
        crate::lut::stage_alloc_identity(None, n_channels)
    }
    /// `cmsStageAllocToneCurves` — one curve per channel.
    pub fn tone_curves(curves: &[&ToneCurveRef]) -> Result<Stage> {
        crate::lut::stage_alloc_tone_curves(None, curves.len() as u32, Some(curves)).ok_or_else(
            || Error::from_last(types::ERROR_UNDEFINED, "stage_alloc_tone_curves failed"),
        )
    }
    /// `cmsStageAllocMatrix` — `rows × cols` matrix + optional offsets.
    pub fn matrix(
        matrix2d: &[f64],
        rows: u32,
        cols: u32,
        offsets: Option<&[f64]>,
    ) -> Result<Stage> {
        crate::lut::stage_alloc_matrix(None, rows, cols, matrix2d, offsets)
            .ok_or_else(|| Error::from_last(types::ERROR_UNDEFINED, "stage_alloc_matrix failed"))
    }
    /// `cmsStageAllocCLut16bit` — a uniform `points`-per-dim u16 CLUT.
    /// `table` len = `points^in_chan × out_chan` (`None` = zeroed).
    pub fn clut16(
        points: u32,
        in_chan: u32,
        out_chan: u32,
        table: Option<&[u16]>,
    ) -> Result<Stage> {
        crate::lut::stage_alloc_clut16bit(None, points, in_chan, out_chan, table)
            .ok_or_else(|| Error::from_last(types::ERROR_UNDEFINED, "stage_alloc_clut16bit failed"))
    }
    /// `cmsStageAllocCLut16bitGranular` — per-dimension grid points.
    pub fn clut16_granular(points: &[u32], out_chan: u32, table: Option<&[u16]>) -> Result<Stage> {
        crate::lut::stage_alloc_clut16bit_granular(
            None,
            points,
            points.len() as u32,
            out_chan,
            table,
        )
        .ok_or_else(|| {
            Error::from_last(
                types::ERROR_UNDEFINED,
                "stage_alloc_clut16bit_granular failed",
            )
        })
    }
    /// `cmsStageAllocCLutFloat` — a uniform `points`-per-dim f32 CLUT.
    pub fn clut_float(
        points: u32,
        in_chan: u32,
        out_chan: u32,
        table: Option<&[f32]>,
    ) -> Result<Stage> {
        crate::lut::stage_alloc_clut_float(None, points, in_chan, out_chan, table).ok_or_else(
            || Error::from_last(types::ERROR_UNDEFINED, "stage_alloc_clut_float failed"),
        )
    }
}

/// `Mlu` — build/read multi-language text (`cmsMLU*`).
impl Mlu {
    /// `cmsMLUalloc` — a new empty MLU with `capacity` translation slots.
    pub fn new(capacity: u32) -> Mlu {
        named::mlu_alloc(None, capacity).unwrap_or_default()
    }
    /// `cmsMLUsetUTF8` — set `text` under `locale`.
    pub fn set(&mut self, locale: Locale, text: &str) -> bool {
        named::mlu_set_utf8(self, &locale.language, &locale.country, text.as_bytes())
    }
    /// `cmsMLUsetASCII` — set `text` under `locale` (ASCII only —
    /// rejects non-ASCII input).
    pub fn set_ascii(&mut self, locale: Locale, text: &str) -> bool {
        named::mlu_set_ascii(self, &locale.language, &locale.country, text.as_bytes())
    }
    /// `cmsMLUgetUTF8` — the text for `locale` (`Locale::NONE` = best
    /// available translation).
    pub fn get(&self, locale: Locale) -> Option<String> {
        let (n, mut buf) = named::mlu_get_utf8(self, &locale.language, &locale.country, Some(256));
        if n == 0 {
            return None;
        }
        buf.truncate(n as usize);
        while buf.last() == Some(&0) {
            buf.pop();
        }
        String::from_utf8(buf).ok()
    }
    /// `cmsMLUgetASCII` — ASCII text for `locale`.
    pub fn get_ascii(&self, locale: Locale) -> Option<String> {
        let (n, mut buf) = named::mlu_get_ascii(self, &locale.language, &locale.country, Some(256));
        if n == 0 {
            return None;
        }
        buf.truncate(n as usize);
        while buf.last() == Some(&0) {
            buf.pop();
        }
        String::from_utf8(buf).ok()
    }
    /// `cmsMLUtranslationsCount`.
    pub fn translations(&self) -> u32 {
        named::mlu_translations_count(self)
    }
    /// `cmsMLUtranslationsCodes` — the `idx`-th translation's locale.
    pub fn translation_codes(&self, idx: u32) -> Option<Locale> {
        named::mlu_translations_codes(self, idx).map(|(l, c)| Locale {
            language: l,
            country: c,
        })
    }
}

/// `NamedColorList` — authored palettes (`cmsAllocNamedColorList`/`cmsAppendNamedColor`).
impl NamedColorList {
    /// `cmsAllocNamedColorList` — `spots` initial entries, `colorant_count`
    /// device channels per entry, list-wide `prefix`/`suffix` strings.
    pub fn new(
        spots: u32,
        colorant_count: u32,
        prefix: &str,
        suffix: &str,
    ) -> Result<NamedColorList> {
        named::alloc_named_color_list(
            None,
            spots,
            colorant_count,
            prefix.as_bytes(),
            suffix.as_bytes(),
        )
        .ok_or_else(|| Error::from_last(types::ERROR_UNDEFINED, "alloc_named_color_list failed"))
    }
    /// `cmsAppendNamedColor` — grow by one entry (`name`, PCS triple,
    /// 16-channel device colorant).
    pub fn push(&mut self, name: &str, pcs: [u16; 3], colorant: &[u16; 16]) -> bool {
        named::append_named_color(self, Some(name.as_bytes()), Some(&pcs), Some(colorant))
    }
    /// `cmsNamedColorIndex` — first index whose name matches (0-based).
    pub fn position(&self, name: &str) -> Option<usize> {
        let i = named::named_color_index(self, name.as_bytes());
        if i < 0 { None } else { Some(i as usize) }
    }
    /// Entry count (`cmsNamedColorCount`).
    pub fn len(&self) -> usize {
        self.n_colors as usize
    }
    /// `true` when empty.
    pub fn is_empty(&self) -> bool {
        self.n_colors == 0
    }
}

/// `Dict` — name/value string dictionary (`cmsDict*`).
impl Dict {
    /// `cmsDictAlloc`.
    pub fn new() -> Dict {
        named::dict_alloc(None)
    }
    /// `cmsDictAddEntry` — add a `name`/`value` pair (display strings are
    /// built for both, in the default locale).
    pub fn add(&mut self, name: &str, value: &str) -> bool {
        let name_w: Vec<named::Wchar> =
            name.chars().map(|c| c as named::Wchar).chain([0]).collect();
        let value_w: Vec<named::Wchar> = value
            .chars()
            .map(|c| c as named::Wchar)
            .chain([0])
            .collect();
        let mut dn = named::mlu_alloc(None, 1).unwrap_or_default();
        named::mlu_set_ascii(&mut dn, &[0; 3], &[0; 3], name.as_bytes());
        let mut dv = named::mlu_alloc(None, 1).unwrap_or_default();
        named::mlu_set_ascii(&mut dv, &[0; 3], &[0; 3], value.as_bytes());
        named::dict_add_entry(&mut *self, &name_w, Some(&value_w), Some(&dn), Some(&dv))
    }
    /// The display value stored under `name`.
    pub fn get(&self, name: &str) -> Option<String> {
        self.entries
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
            .and_then(|v| Mlu::get(v, Locale::NONE))
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

// -- free functions -------------------------------------------------------------

/// `cmsGetEncodedCMMversion` — `2200` for lcms 2.20.
pub fn version() -> u32 {
    types::LCMS_VERSION
}
/// `cmsD50_XYZ` — the D50 white point in XYZ.
pub fn d50_xyz() -> CieXyz {
    crate::wtpnt::d50_xyz()
}
/// `cmsD50_xyY` — D50 in xyY.
pub fn d50_xyy() -> CiexyY {
    crate::wtpnt::d50_xyy()
}
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
/// (the `Format::LabU16` wire form).
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
