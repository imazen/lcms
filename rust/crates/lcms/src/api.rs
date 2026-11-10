//! `lcms::api` — the idiomatic layer.
//!
//! The `cms*`/`pub mod` surface below stays 1:1 with the C source for
//! conformance; this module wraps it in owned, `Result`-returning,
//! `Send`/`Sync`-friendly types for real applications.
//!
//! Model for a service (e.g. an image proxy):
//!
//! - `Profile` — an owned ICC profile (`Send`); `freeze()` then `Arc` it
//!   for shared read access, or keep it per-worker.
//! - `Plan` (`xform::TransformPlan`) — the immutable linked transform
//!   (`Sync`); `Arc`-share across workers, cache by
//!   `(src, dst, fmt-in, fmt-out, intent, flags)`.
//! - `Scratch` — the only per-run state (~64 B pixel cache). Fresh per
//!   call via `plan.scratch()`, or pooled `thread_local`.
//!
//! ```no_run
//! use lcms::api::*;
//! let mut srgb = Profile::srgb().unwrap();
//! let mut p3 = Profile::open(&std::fs::read("Display-P3.icc").unwrap()).unwrap();
//! let plan = Plan::new(&mut p3, &mut srgb, Format::RgbU8, Format::RgbU8).unwrap();
//! let mut sc = plan.scratch();
//! let src = [255u8, 0, 0]; let mut dst = [0u8; 3];
//! plan.apply(&mut sc, &src, &mut dst);
//! ```

use crate::{profile, tagtypes::TagObject, types, virt, xform};
use std::fmt;

// -- Error ----------------------------------------------------------------

/// An lcms failure — the `cmsSignalError` id + message.
#[derive(Debug)]
pub struct Error {
    pub id: i32,
    pub message: String,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "lcms error 0x{:x}: {}", self.id, self.message)
    }
}
impl std::error::Error for Error {}

impl Error {
    /// Build from the last `signal_error` this thread produced, or the
    /// fallback if the failing path didn't signal one.
    pub fn from_last(fallback_id: i32, fallback: &str) -> Self {
        match crate::err::take_last_error() {
            Some((id, message)) => Error { id, message },
            None => Error { id: fallback_id, message: fallback.into() },
        }
    }
}

// -- Format ---------------------------------------------------------------

/// Pixel format — wraps the packed `cmsFORMAT` u32.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    RgbU8,
    RgbU16,
    RgbF32,
    RgbaU8,
    RgbaF32,
    LabU8,
    LabU16,
    LabF32,
    XyzU16,
    XyzF32,
    GrayU8,
    GrayU16,
    GrayF32,
    CmykU8,
    CmykU16,
    CmykF32,
    /// Any other packed format code — escape hatch for the full
    /// `cmsPACK` matrix (planes, doSwap, extra channels, ...).
    Raw(u32),
}

// pixel-type codes (COLORSPACE_SH payload in the format u32)
const PT_GRAY: u32 = 3;
const PT_RGB: u32 = 4;
const PT_CMYK: u32 = 6;
const PT_XYZ: u32 = 9;
const PT_LAB: u32 = 10;

fn f16(pt: u32, ch: u32) -> u32 {
    pt << 16 | ch << 3 | 2
}
fn f8(pt: u32, ch: u32) -> u32 {
    pt << 16 | ch << 3 | 1
}
fn ff32(pt: u32, ch: u32) -> u32 {
    1 << 22 | pt << 16 | ch << 3 | 4
}

impl Format {
    /// The packed `cmsFORMAT` code.
    pub fn code(self) -> u32 {
        match self {
            Format::RgbU8 => f8(PT_RGB, 3),
            Format::RgbU16 => f16(PT_RGB, 3),
            Format::RgbF32 => ff32(PT_RGB, 3),
            Format::RgbaU8 => f8(PT_RGB, 4),
            Format::RgbaF32 => ff32(PT_RGB, 4),
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
    /// Bytes per pixel for this format (packed, no stride).
    pub fn pixel_size(self) -> usize {
        crate::pack::t_bytes(self.code()) as usize
    }
}

// -- Tag ------------------------------------------------------------------

/// A 4-byte ICC tag signature — `Display` prints the `'xxxx'` chars.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Tag(pub u32);
impl fmt::Debug for Tag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "Tag({self})") }
}
impl fmt::Display for Tag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let b = self.0.to_be_bytes();
        write!(f, "'")?;
        for c in b {
            write!(f, "{}", if c.is_ascii_graphic() { c as char } else { '.' })?;
        }
        write!(f, "'")
    }
}
impl From<Tag> for u32 {
    fn from(t: Tag) -> u32 { t.0 }
}
impl From<u32> for Tag {
    fn from(u: u32) -> Tag { Tag(u) }
}
impl Tag {
    /// Common tag signatures — the full set is `lcms::signatures::tag`.
    pub const MEDIA_WHITE_POINT: Tag = Tag(crate::signatures::tag::SigMediaWhitePointTag);
    pub const COPYRIGHT: Tag = Tag(crate::signatures::tag::SigCopyrightTag);
    pub const PROFILE_DESCRIPTION: Tag = Tag(crate::signatures::tag::SigProfileDescriptionTag);
    pub const MEDIA_BLACK_POINT: Tag = Tag(crate::signatures::tag::SigMediaBlackPointTag);
    pub const CHROMATIC_ADAPTATION: Tag = Tag(crate::signatures::tag::SigChromaticAdaptationTag);
    pub const RED_TRC: Tag = Tag(crate::signatures::tag::SigRedTRCTag);
    pub const GREEN_TRC: Tag = Tag(crate::signatures::tag::SigGreenTRCTag);
    pub const BLUE_TRC: Tag = Tag(crate::signatures::tag::SigBlueTRCTag);
    pub const GRAY_TRC: Tag = Tag(crate::signatures::tag::SigGrayTRCTag);
    pub const RED_COLORANT: Tag = Tag(crate::signatures::tag::SigRedColorantTag);
    pub const GREEN_COLORANT: Tag = Tag(crate::signatures::tag::SigGreenColorantTag);
    pub const BLUE_COLORANT: Tag = Tag(crate::signatures::tag::SigBlueColorantTag);
    pub const NAMED_COLOR2: Tag = Tag(crate::signatures::tag::SigNamedColor2Tag);
    pub const A_TO_B0: Tag = Tag(crate::signatures::tag::SigAToB0Tag);
    pub const A_TO_B1: Tag = Tag(crate::signatures::tag::SigAToB1Tag);
    pub const A_TO_B2: Tag = Tag(crate::signatures::tag::SigAToB2Tag);
    pub const B_TO_A0: Tag = Tag(crate::signatures::tag::SigBToA0Tag);
    pub const B_TO_A1: Tag = Tag(crate::signatures::tag::SigBToA1Tag);
    pub const B_TO_A2: Tag = Tag(crate::signatures::tag::SigBToA2Tag);
    pub const VCGT: Tag = Tag(crate::signatures::tag::SigVcgtTag);
    pub const GAMUT: Tag = Tag(crate::signatures::tag::SigGamutTag);
    pub const CHARD: Tag = Tag(crate::signatures::tag::SigCharTargetTag);
    pub const SCREENING_DESC: Tag = Tag(crate::signatures::tag::SigScreeningDescTag);
    pub const UCRBG: Tag = Tag(crate::signatures::tag::SigUcrBgTag);
    pub const PS2_CSA: Tag = Tag(crate::signatures::tag::SigPs2CSATag);
    pub const PS2_CRD0: Tag = Tag(crate::signatures::tag::SigPs2CRD0Tag);
    pub const PS2_CRD1: Tag = Tag(crate::signatures::tag::SigPs2CRD1Tag);
    pub const PS2_CRD2: Tag = Tag(crate::signatures::tag::SigPs2CRD2Tag);
    pub const PS2_CRD3: Tag = Tag(crate::signatures::tag::SigPs2CRD3Tag);
    pub const PS2_RENDERING_INTENT: Tag = Tag(crate::signatures::tag::SigPs2RenderingIntentTag);
}

// -- Intent ---------------------------------------------------------------

/// Rendering intent (`cmsIntents.h`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Intent {
    Perceptual,
    RelativeColorimetric,
    Saturation,
    AbsoluteColorimetric,
}
impl Intent {
    pub fn code(self) -> u32 {
        self as u32
    }
    /// `u32` -> `Intent` — wraps out-of-range values to the nearest
    /// valid (matches C's intent clamping).
    pub fn from_u32(v: u32) -> Intent {
        match v {
            0 => Intent::Perceptual,
            1 => Intent::RelativeColorimetric,
            2 => Intent::Saturation,
            _ => Intent::AbsoluteColorimetric,
        }
    }
}

// -- Profile --------------------------------------------------------------

/// An owned ICC profile. `Send`; `freeze()` + `Arc` for shared reads.
pub struct Profile {
    pub(crate) icc: Box<crate::profile::IccProfile>,
}

impl fmt::Debug for Profile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Profile")
            .field("device_class", &format_args!("0x{:x}", self.icc.device_class))
            .field("color_space", &format_args!("0x{:x}", self.icc.color_space))
            .field("pcs", &format_args!("0x{:x}", self.icc.pcs))
            .field("version", &format_args!("0x{:x}", self.icc.version))
            .finish()
    }
}

impl Profile {
    fn wrap(icc: Option<Box<crate::profile::IccProfile>>, what: &str) -> Result<Self, Error> {
        icc.map(|icc| Profile { icc })
            .ok_or_else(|| Error::from_last(types::ERROR_READ, &format!("{what} failed")))
    }

    /// `cmsCreate_sRGBProfile`
    pub fn srgb() -> Result<Self, Error> {
        Self::wrap(virt::create_srgb_profile(None), "create_srgb_profile")
    }
    /// `cmsCreateLab4Profile`
    pub fn lab4() -> Result<Self, Error> {
        Self::wrap(virt::create_lab4_profile(None, None), "create_lab4_profile")
    }
    /// `cmsCreateGrayProfile` with D50 white and a 2.2 gamma TRC.
    pub fn gray() -> Result<Self, Error> {
        let wp = types::CiexyY { x: 0.3457, y: 0.3585, Y: 1.0 }; // D50
        let g = crate::gamma::build_gamma(None, 2.2);
        Self::wrap(virt::create_gray_profile(None, Some(&wp), g.as_ref()), "create_gray_profile")
    }
    /// `cmsCreateXYZProfile`
    pub fn xyz() -> Result<Self, Error> {
        Self::wrap(virt::create_xyz_profile(None), "create_xyz_profile")
    }
    /// `cmsCreateLab2Profile`
    pub fn lab2() -> Result<Self, Error> {
        Self::wrap(virt::create_lab2_profile(None, None), "create_lab2_profile")
    }
    /// `cmsCreateNULLProfile`
    pub fn null() -> Result<Self, Error> {
        Self::wrap(virt::create_null_profile(None), "create_null_profile")
    }
    /// `cmsCreateRGBProfile` — white point, primaries, transfer curves.
    pub fn rgb(
        white_point: &types::CiexyY,
        primaries: &types::CiexyYTriple,
        curves: [&ToneCurve; 3],
    ) -> Result<Self, Error> {
        Self::wrap(
            virt::create_rgb_profile(
                None, Some(white_point), Some(primaries),
                Some([&curves[0].0, &curves[1].0, &curves[2].0]),
            ),
            "create_rgb_profile",
        )
    }
    /// `cmsCreateGrayProfile` — white point + a custom tone curve.
    pub fn gray_curve(white_point: &types::CiexyY, curve: &ToneCurve) -> Result<Self, Error> {
        Self::wrap(
            virt::create_gray_profile(None, Some(white_point), Some(&curve.0)),
            "create_gray_profile",
        )
    }
    /// `cmsOpenProfileFromMem`
    pub fn open(data: &[u8]) -> Result<Self, Error> {
        Self::wrap(
            profile::open_profile_from_mem(None, data),
            "open_profile_from_mem",
        )
    }
    /// `cmsOpenProfileFromFile` — read an ICC file off disk.
    pub fn open_file(path: &std::path::Path) -> Result<Self, Error> {
        let data = std::fs::read(path).map_err(|e| Error {
            id: types::ERROR_READ,
            message: e.to_string(),
        })?;
        Self::open(&data)
    }
    /// `cmsSaveProfileToMem` — serialized ICC bytes.
    pub fn save(&mut self) -> Result<Vec<u8>, Error> {
        profile::save_profile_to_mem(&mut self.icc)
            .ok_or_else(|| Error::from_last(types::ERROR_WRITE, "save_profile_to_mem failed"))
    }
    /// Eagerly materialize all tags — after `freeze`, `Arc<Profile>`
    /// reads via [`Profile::tag`] are `Sync`-safe.
    pub fn freeze(&mut self) {
        profile::freeze(&mut self.icc);
    }
    /// Read a tag value (`None` if absent/not frozen) — a borrowed
    /// `TagValue` view into the profile's stored `TagObject`.
    pub fn tag(&self, sig: u32) -> Option<TagValue<'_>> {
        let obj = profile::read_tag_frozen(&self.icc, sig)?;
        Some(TagValue::of(obj))
    }
    /// Iterate the tag signatures present in the profile.
    pub fn tags(&self) -> impl Iterator<Item = Tag> + '_ {
        self.icc.tag_names[..self.icc.tag_count as usize]
            .iter()
            .filter(|&&s| s != 0)
            .map(|&s| Tag(s))
    }
    /// Read a tag by `Tag` — `prof.get(Tag::MEDIA_WHITE_POINT)`.
    pub fn get(&self, tag: Tag) -> Option<TagValue<'_>> {
        self.tag(tag.0)
    }
    /// Write a tag (`TagData` is converted to the stored `TagObject`).
    pub fn write_tag(&mut self, tag: Tag, value: TagData) -> bool {
        let obj = match value {
            TagData::Xyz(x) => TagObject::Xyz(x),
            TagData::Curve(c) => TagObject::Curve(c.0),
            TagData::Text(s) => {
                let mut m = crate::named::mlu_alloc(None, 1).unwrap_or_default();
                crate::named::mlu_set_ascii(&mut m, &[0;3], &[0;3], s.as_bytes());
                TagObject::Mlu(m)
            }
            TagData::Mlu(m) => TagObject::Mlu(m),
        };
        crate::profile::cms_write_tag(&mut self.icc, tag.0, Some(obj))
    }
    /// Remove a tag.
    pub fn remove_tag(&mut self, tag: Tag) {
        crate::profile::cms_write_tag(&mut self.icc, tag.0, None);
    }
    /// `Tag` present (directory entry, frozen or not).
    pub fn has_tag(&self, tag: Tag) -> bool {
        crate::profile::cms_is_tag(&self.icc, tag.0)
    }
    /// The named-color list, if the profile carries one.
    pub fn named_colors(&self) -> Option<NamedColors<'_>> {
        match profile::read_tag_frozen(&self.icc, crate::signatures::tag::SigNamedColor2Tag) {
            Some(TagObject::NamedColor(l)) | Some(TagObject::ColorantTable(l)) =>
                Some(NamedColors(l)),
            _ => None,
        }
    }
    /// Localized profile text — description/copyright/manufacturer/model.
    pub fn info(&self, kind: InfoType) -> Option<String> {
        let sig = match kind {
            InfoType::Description => crate::signatures::tag::SigProfileDescriptionTag,
            InfoType::Manufacturer => crate::signatures::tag::SigDeviceMfgDescTag,
            InfoType::Model => crate::signatures::tag::SigDeviceModelDescTag,
            InfoType::Copyright => crate::signatures::tag::SigCopyrightTag,
        };
        let obj = profile::read_tag_frozen(&self.icc, sig)?;
        if let TagObject::Mlu(m) = obj {
            let (n, mut buf) = crate::named::mlu_get_utf8(m, &[0;3], &[0;3], Some(256));
            if n == 0 { None } else {
                buf.truncate(n as usize);
                while buf.last() == Some(&0) { buf.pop(); }
                String::from_utf8(buf).ok()
            }
        } else { None }
    }
    /// Header accessors — the ICC header fields (raw u32 sig values; use
    /// `Tag(v)`/`u32` conversions to interpret).
    pub fn device_class(&self) -> u32 { self.icc.device_class }
    pub fn color_space(&self) -> u32 { self.icc.color_space }
    pub fn pcs(&self) -> u32 { self.icc.pcs }
    pub fn version(&self) -> u32 { self.icc.version }
    pub fn header_flags(&self) -> u32 { self.icc.flags }
    pub fn header_attributes(&self) -> u64 { self.icc.attributes }
    pub fn header_creator(&self) -> u32 { self.icc.creator }
    pub fn header_intent(&self) -> Intent { Intent::from_u32(self.icc.rendering_intent as u32) }
    /// `set_color_space`/`set_device_class`/`set_header_intent` —
    /// header field setters for profile authoring.
    pub fn set_color_space(&mut self, cs: u32) { self.icc.color_space = cs; }
    pub fn set_device_class(&mut self, dc: u32) { self.icc.device_class = dc; }
    pub fn set_header_intent(&mut self, intent: Intent) {
        self.icc.rendering_intent = intent.code();
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

// -- Plan -----------------------------------------------------------------

/// The immutable linked transform — `Sync`, `Arc`-shareable.
/// Cheap `Scratch` clones run the pixels; the plan is the expensive part.
pub struct Plan {
    pub(crate) inner: xform::TransformPlan,
}

impl fmt::Debug for Plan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Plan")
            .field("input_format", &format_args!("0x{:x}", self.inner.input_format))
            .field("output_format", &format_args!("0x{:x}", self.inner.output_format))
            .field("intent", &self.inner.rendering_intent)
            .finish()
    }
}

/// Per-run pixel scratch (~64 B) — `plan.scratch()`, or pooled
/// `thread_local`. `Send`; not `Sync` (it's the mutable eval cache).
pub struct Scratch(pub(crate) xform::TransformScratch);
impl fmt::Debug for Scratch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Scratch").finish_non_exhaustive()
    }
}

impl Plan {
    /// `cmsCreateTransform` — link `src -> dst` with the given formats,
    /// perceptual intent, no flags.
    pub fn new(
        src: &mut Profile,
        dst: &mut Profile,
        fmt_in: Format,
        fmt_out: Format,
    ) -> Result<Self, Error> {
        Self::build(src, dst, fmt_in, fmt_out, Intent::Perceptual, 0)
    }

    /// Full-parameter link — `cmsCreateExtendedTransform` with a single
    /// intent/flags pair. `&mut Profile` because the link path
    /// lazy-materializes tags (call `freeze` first to pre-resolve).
    pub fn build(
        src: &mut Profile,
        dst: &mut Profile,
        fmt_in: Format,
        fmt_out: Format,
        intent: Intent,
        flags: u32,
    ) -> Result<Self, Error> {
        // create_extended_transform takes &mut [Box<IccProfile>] — swap
        // the boxes out of the Profiles for the call, swap back after.
        let mut profs = [
            std::mem::take(&mut src.icc),
            std::mem::take(&mut dst.icc),
        ];
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
        let [sb, db] = profs;
        src.icc = sb;
        dst.icc = db;
        let t = t.ok_or_else(|| {
            Error::from_last(types::ERROR_UNDEFINED, "create_transform failed")
        })?;
        let xform::Transform { plan, .. } = *t;
        Ok(Plan { inner: plan })
    }

    /// Fresh per-run scratch — clones the seeded cache (~64 B).
    pub fn scratch(&self) -> Scratch {
        Scratch(self.inner.scratch())
    }
    /// Evaluate `input` -> `output` — `&self` is `Sync`. `n` = pixel
    /// count: `input.len() / Format::pixel_size`.
    pub fn apply(&self, scratch: &mut Scratch, input: &[u8], output: &mut [u8]) {
        let b = crate::pack::t_bytes(self.inner.input_format) as usize;
        self.inner.apply(&mut scratch.0, input, output,
            (input.len() / b.max(1)).max(1) as u32);
    }
    /// Evaluate `n` pixels.
    pub fn apply_n(&self, scratch: &mut Scratch, input: &[u8], output: &mut [u8], n: u32) {
        self.inner.apply(&mut scratch.0, input, output, n);
    }
    /// Typed-pixel apply — `&[I]` -> `&mut [O]` via `Pod` casts.
    /// `I`/`O` are any `bytemuck::Pod` pixel types ([u8;3], u16, f32, ...).
    pub fn apply_pixels<I: bytemuck::Pod, O: bytemuck::Pod>(
        &self, scratch: &mut Scratch, input: &[I], output: &mut [O],
    ) {
        let n = input.len().min(output.len()) as u32;
        self.inner.apply(&mut scratch.0,
            bytemuck::cast_slice(input),
            bytemuck::cast_slice_mut(output),
            n);
    }
    /// Parallel apply — scratch-per-chunk, pixel-independent.
    pub fn apply_par(&self, input: &[u8], output: &mut [u8], n: u32, threads: usize) {
        self.inner.apply_par(input, output, n, threads);
    }
}

// -- ToneCurve --------------------------------------------------------------

/// An owned ICC tone curve (TRC/gamma/parametric/tabulated).
/// Wraps the parity layer's `gamma::ToneCurve`; `Clone`.
#[derive(Clone)]
pub struct ToneCurve(pub(crate) crate::gamma::ToneCurve);
impl fmt::Debug for ToneCurve {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ToneCurve")
            .field("estimated_gamma", &self.0.eval16(0x8000))
            .field("entries", &self.0.estimated_table_entries())
            .finish()
    }
}
impl ToneCurve {
    /// `cmsBuildGamma` — a plain `v^g` curve.
    pub fn gamma(g: f64) -> Result<Self, Error> {
        crate::gamma::build_gamma(None, g)
            .map(ToneCurve)
            .ok_or_else(|| Error::from_last(types::ERROR_UNDEFINED, "build_gamma failed"))
    }
    /// `cmsBuildTabulatedToneCurve16` — a 16-entry interpolated table.
    pub fn tabulated16(values: &[u16]) -> Result<Self, Error> {
        crate::gamma::build_tabulated_tone_curve16(None, values.len() as u32, Some(values))
            .map(ToneCurve)
            .ok_or_else(|| Error::from_last(types::ERROR_UNDEFINED, "build_tabulated_tone_curve16 failed"))
    }
    /// `cmsBuildTabulatedToneCurveFloat` — a float interpolated table.
    pub fn tabulated_f32(values: &[f32]) -> Result<Self, Error> {
        crate::gamma::build_tabulated_tone_curve_float(None, values)
            .map(ToneCurve)
            .ok_or_else(|| Error::from_last(types::ERROR_UNDEFINED, "build_tabulated_tone_curve_float failed"))
    }
    /// `cmsBuildParametricToneCurve` — `type` + `params` (e.g. type 1
    /// sRGB: `[2.4, 1/1.055, 0.055/1.055, 1/12.92, 0.04045]`).
    pub fn parametric(ty: i16, params: &[f64]) -> Result<Self, Error> {
        crate::gamma::build_parametric_tone_curve(None, ty as i32, params)
            .map(ToneCurve)
            .ok_or_else(|| Error::from_last(types::ERROR_UNDEFINED, "build_parametric_tone_curve failed"))
    }
    /// `cmsEvalToneCurve16` / `cmsEvalToneCurveFloat`.
    pub fn eval16(&self, v: u16) -> u16 { self.0.eval16(v) }
    pub fn eval_f32(&self, v: f32) -> f32 { self.0.eval_float(v) }
    /// `cmsEstimateGamma` — the effective single-gamma fit (`None` if the
    /// curve can't be fit — e.g. multisegment).
    pub fn estimated_gamma(&self) -> Option<f64> {
        let g = crate::gamma::estimate_gamma(&self.0, 0.001);
        if g < 0.0 { None } else { Some(g) }
    }
    /// `cmsReverseToneCurve` — the inverse curve (identity-preserving
    /// for monotonic curves).
    pub fn reversed(&self) -> ToneCurve {
        ToneCurve(crate::gamma::reverse_tone_curve(&self.0)
            .unwrap_or_else(|| self.0.clone()))
    }
    /// `cmsIsToneCurveMonotonic` / `cmsIsToneCurveDescending`.
    pub fn is_monotonic(&self) -> bool { crate::gamma::is_tone_curve_monotonic(&self.0) }
    pub fn is_descending(&self) -> bool { crate::gamma::is_tone_curve_descending(&self.0) }
}

// -- info --------------------------------------------------------------------

/// Profile info-text selector — which localized text tag to read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InfoType {
    Description,
    Manufacturer,
    Model,
    Copyright,
}

// -- tag reads / writes -------------------------------------------------------

/// `TagValue<'a>` — a borrowed view of a stored `TagObject` (zero-cost;
/// points at the frozen profile's tag data).
#[non_exhaustive]
pub enum TagValue<'a> {
    /// White point / colorant XYZ.
    Xyz(&'a types::CieXyz),
    /// A tone curve (TRC, parametric, vcgt curves).
    Curve(&'a ToneCurveRef),
    /// Multi-language text (description/copyright/...).
    Text(&'a MluView<'a>),
    /// A named-color palette.
    NamedColors(&'a NamedColorListRef),
    /// A processing pipeline (mft/mAB/mBA LUTs).
    Pipeline(&'a PipelineRef),
    /// The 3-curve vcgt tag.
    VcgtCurves([&'a ToneCurveRef; 3]),
    /// Chromaticity (xyY primaries triple).
    Chromaticity(&'a types::CiexyYTriple),
    /// ICC data payload.
    Data(&'a [u8]),
    /// Screening.
    Screening(&'a crate::tagtypes::Screening),
    /// UCR/BG.
    UcrBg(&'a crate::tagtypes::UcrBg),
    /// MHC2.
    Mhc2(&'a crate::tagtypes::Mhc2),
    /// Video signal.
    VideoSignal(&'a crate::tagtypes::VideoSignal),
    /// Anything else — `TagObject` is `pub(crate)`, so this is a
    /// marker; the parity layer can read the full object.
    Other,
}

/// `ToneCurveRef` — a borrowed `&ToneCurve` (the parity layer's
/// `gamma::ToneCurve`, the same type the lcms2 shim exposes).
pub type ToneCurveRef = crate::gamma::ToneCurve;
/// `MluView<'a>` — a borrowed `&Mlu` (the parity MLU type).
pub type MluView<'a> = crate::named::Mlu;
/// `NamedColorListRef` — a borrowed `&NamedColorList`.
pub type NamedColorListRef = crate::named::NamedColorList;
/// `PipelineRef` — a borrowed `&Pipeline`.
pub type PipelineRef = crate::lut::Pipeline;
/// `StageRef` — a borrowed `&Stage`.
pub type StageRef = crate::lut::Stage;

impl<'a> TagValue<'a> {
    fn of(obj: &'a TagObject) -> Self {
        match obj {
            TagObject::Xyz(x) => TagValue::Xyz(x),
            TagObject::Curve(c) | TagObject::ParametricCurve(c) => TagValue::Curve(c),
            TagObject::Mlu(m) => TagValue::Text(m),
            TagObject::NamedColor(l) | TagObject::ColorantTable(l) => TagValue::NamedColors(l),
            TagObject::Lut8(l) | TagObject::Lut16(l)
            | TagObject::LutA2B(l) | TagObject::LutB2A(l)
            | TagObject::Mpe(l) => TagValue::Pipeline(l),
            TagObject::Vcgt(crate::tagtypes::VcgtData::Curves(c)) =>
                TagValue::VcgtCurves([&c[0], &c[1], &c[2]]),
            TagObject::Chromaticity(c) => TagValue::Chromaticity(c),
            TagObject::Data(d) => TagValue::Data(&d.data),
            TagObject::Screening(s) => TagValue::Screening(s),
            TagObject::UcrBg(u) => TagValue::UcrBg(u),
            TagObject::Mhc2(m) => TagValue::Mhc2(m),
            TagObject::VideoSignal(v) => TagValue::VideoSignal(v),
            _ => TagValue::Other,
        }
    }
}

/// `TagData` — the owned write-side counterpart of `TagValue`.
/// `Profile::write_tag(Tag, TagData)` stores it.
pub enum TagData {
    /// XYZ value.
    Xyz(types::CieXyz),
    /// A tone curve.
    Curve(ToneCurve),
    /// An ASCII text tag.
    Text(&'static str),
    /// An MLU tag (multi-language).
    Mlu(crate::named::Mlu),
}

// -- named colors --------------------------------------------------------------

/// `NamedColors` — a borrowed view of the profile's named-color list.
pub struct NamedColors<'a>(&'a NamedColorListRef);
impl<'a> NamedColors<'a> {
    /// Count of named-color entries.
    pub fn count(&self) -> usize { self.0.n_colors as usize }
    /// The `index`-th entry (`None` if out of bounds).
    pub fn get(&self, index: usize) -> Option<NamedColor> {
        let c = self.0.list.get(index)?;
        let name_end = c.name.iter().position(|&b| b == 0).unwrap_or(c.name.len());
        Some(NamedColor {
            name: String::from_utf8_lossy(&c.name[..name_end]).into_owned(),
            pcs: c.pcs,
            colorant: c.device_colorant[..self.0.colorant_count as usize].to_vec(),
        })
    }
    /// Iterate the entries.
    pub fn iter(&self) -> impl Iterator<Item = NamedColor> + '_ {
        (0..self.0.n_colors as usize).filter_map(move |i| self.get(i))
    }
}
/// A single named-color entry.
#[derive(Clone, Debug, PartialEq)]
pub struct NamedColor {
    pub name: String,
    pub pcs: [u16; 3],
    pub colorant: Vec<u16>,
}

// -- Plan extras ---------------------------------------------------------------

impl Plan {
    /// `cmsCreateMultiprofileTransform` — a chain of N profiles
    /// (input -> [intermediates] -> output).
    pub fn new_multiprofile(
        profiles: &mut [&mut Profile],
        fmt_in: Format,
        fmt_out: Format,
        intent: Intent,
        flags: u32,
    ) -> Result<Self, Error> {
        let mut profs: Vec<Box<crate::profile::IccProfile>> =
            profiles.iter_mut().map(|p| std::mem::take(&mut p.icc)).collect();
        let n = profs.len();
        let mut bpc = vec![false; n];
        let intents = vec![intent.code(); n];
        let states = vec![3.0f64; n];
        let t = xform::create_extended_transform(
            None, &mut profs, &mut bpc, &intents, &states, None, 0,
            fmt_in.code(), fmt_out.code(), flags,
        );
        for (p, prof) in profiles.iter_mut().zip(profs.into_iter()) { p.icc = prof; }
        let t = t.ok_or_else(|| {
            Error::from_last(types::ERROR_UNDEFINED, "create_extended_transform failed")
        })?;
        let xform::Transform { plan, .. } = *t;
        Ok(Plan { inner: plan })
    }
    /// `cmsTransform2DeviceLink` — turn this plan back into an ICC
    /// devicelink profile (consumes the plan).
    pub fn to_device_link(self, version: f64) -> Result<Profile, Error> {
        let scratch = self.inner.scratch();
        let mut xf = xform::Transform { plan: self.inner, scratch };
        virt::transform2_device_link(&mut xf, version, 0)
            .map(|icc| Profile { icc })
            .ok_or_else(|| Error::from_last(types::ERROR_WRITE, "transform2_device_link failed"))
    }
}
