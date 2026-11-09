//! `lcms::lcms2` — a drop-in API shim shaped like the `lcms2` crate
//! (6.x) for easy migrations: `Profile`, `Transform<I, O>` with `Pod`
//! pixel types, `PixelFormat`, `Intent`, `Flags`, `LCMSResult`.
//!
//! Migrating users swap `use lcms2::...` for `use lcms::lcms2::...` —
//! same constructors, same typed `transform_pixels(&[I], &mut [O])`.
//! Underneath everything is the pure-Rust port — `forbid(unsafe_code)`,
//! no C library.
//!
//! ```no_run
//! use lcms::lcms2::*;
//! let srgb = Profile::new_srgb();
//! let p3 = Profile::new_icc(&std::fs::read("Display-P3.icc").unwrap()).unwrap();
//! let t: Transform<[u8; 3], [u8; 3]> =
//!     Transform::new(&p3, PixelFormat::RGB_8, &srgb, PixelFormat::RGB_8, Intent::Perceptual).unwrap();
//! let mut dst = [[0u8; 3]];
//! t.transform_pixels(&[[255, 0, 0]], &mut dst);
//! ```

use crate::{profile as prof, types, virt, xform};

/// lcms2-crate type aliases — `CIEXYZ`/`CIExyY`/`CIEXYZTRIPLE`/`CIELAB`.
pub type CIEXYZ = types::CieXyz;
pub type CIExyY = types::CiexyY;
pub type CIEXYZTRIPLE = types::CiexyYTriple;
pub type CIELAB = types::CieLab;
pub type CIELCh = types::CieLCh;
pub type ToneCurve = crate::gamma::ToneCurve;
pub type CIELABEncoded = types::CieLab;
pub type CIEChad = types::CieXyz;
pub use types::{ERROR_READ, ERROR_WRITE, ERROR_UNDEFINED};
use std::fmt;
use std::marker::PhantomData;
use std::sync::Mutex;

// -- errors ---------------------------------------------------------------

/// `LCMSResult` — the lcms2-crate result alias.
pub type LCMSResult<T> = Result<T, LcmsError>;

/// The lcms2-crate error — carries the lcms error id + message.
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

fn last_or(fallback_id: i32, fallback: &str) -> LcmsError {
    match crate::err::take_last_error() {
        Some((id, message)) => LcmsError { id, message },
        None => LcmsError { id: fallback_id, message: fallback.into() },
    }
}

// -- context markers -------------------------------------------------------

/// `GlobalContext` — the lcms2-crate global-context marker (our `None`).
#[derive(Clone, Copy, Debug, Default)]
pub struct GlobalContext;
/// `ThreadContext` — the lcms2-crate thread-context marker.
pub struct ThreadContext(pub crate::context::Context);

// -- flags / intents --------------------------------------------------------

/// `Flags` — the lcms2-crate cache/flags wrapper (`Flags(0)` = none).
/// The `F` parameter carries the cache marker (`AllowCache`/`DisallowCache`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Flags<F = ()>(pub u32, pub F);
/// `AllowCache` / `DisallowCache` — the lcms2-crate cache markers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AllowCache;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DisallowCache;

impl Flags<()> {
    pub const NONE: Flags = Flags(0, ());
    pub const NO_OPTIMIZE: Flags = Flags(crate::types::FLAGS_NOOPTIMIZE, ());
    pub const NULL_TRANSFORM: Flags = Flags(crate::types::FLAGS_NULLTRANSFORM, ());
    pub const GAMUT_CHECK: Flags = Flags(crate::types::FLAGS_GAMUTCHECK, ());
    pub const SOFT_PROOFING: Flags = Flags(crate::types::FLAGS_SOFTPROOFING, ());
    pub const BLACKPOINT_COMPENSATION: Flags =
        Flags(crate::types::FLAGS_BLACKPOINTCOMPENSATION, ());
    pub const HIGHRES_PRECALC: Flags = Flags(crate::types::FLAGS_HIGHRESPRECALC, ());
    pub const LOWRES_PRECALC: Flags = Flags(crate::types::FLAGS_LOWRESPRECALC, ());
    pub const COPY_ALPHA: Flags = Flags(crate::types::FLAGS_COPY_ALPHA, ());
}
impl Flags<DisallowCache> {
    pub const NO_CACHE: Flags<DisallowCache> =
        Flags(crate::types::FLAGS_NOCACHE, DisallowCache);
}
/// `Intent` — the lcms2-crate rendering intent.
pub type Intent = crate::api::Intent;

// -- pixel formats ----------------------------------------------------------

/// `PixelFormat` — the lcms2-crate packed-format descriptor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PixelFormat(pub u32);
impl PixelFormat {
    pub const GRAY_8: PixelFormat = PixelFormat(f8(PT_GRAY, 1));
    pub const GRAY_16: PixelFormat = PixelFormat(f16(PT_GRAY, 1));
    pub const GRAY_DBL: PixelFormat = PixelFormat(ff32(PT_GRAY, 1));
    pub const RGB_8: PixelFormat = PixelFormat(f8(PT_RGB, 3));
    pub const RGB_16: PixelFormat = PixelFormat(f16(PT_RGB, 3));
    pub const RGB_DBL: PixelFormat = PixelFormat(ff32(PT_RGB, 3));
    pub const RGBA_8: PixelFormat = PixelFormat(f8(PT_RGB, 4));
    pub const RGBA_16: PixelFormat = PixelFormat(f16(PT_RGB, 4));
    pub const RGBA_DBL: PixelFormat = PixelFormat(ff32(PT_RGB, 4));
    pub const LAB_8: PixelFormat = PixelFormat(f8(PT_LAB, 3));
    pub const LAB_16: PixelFormat = PixelFormat(f16(PT_LAB, 3));
    pub const LAB_DBL: PixelFormat = PixelFormat(ff32(PT_LAB, 3));
    pub const XYZ_16: PixelFormat = PixelFormat(f16(PT_XYZ, 3));
    pub const XYZ_DBL: PixelFormat = PixelFormat(ff32(PT_XYZ, 3));
    pub const CMYK_8: PixelFormat = PixelFormat(f8(PT_CMYK, 4));
    pub const CMYK_16: PixelFormat = PixelFormat(f16(PT_CMYK, 4));
    pub const CMYK_DBL: PixelFormat = PixelFormat(ff32(PT_CMYK, 4));
    /// Any packed `cmsFORMAT` — escape hatch.
    pub const fn raw(code: u32) -> PixelFormat { PixelFormat(code) }
    /// Bytes per pixel.
    pub fn bytes(self) -> usize { crate::pack::t_bytes(self.0) as usize }
}

const PT_GRAY: u32 = 3;
const PT_RGB: u32 = 4;
const PT_CMYK: u32 = 6;
const PT_XYZ: u32 = 9;
const PT_LAB: u32 = 10;
const fn f8(pt: u32, ch: u32) -> u32 { pt << 16 | ch << 3 | 1 }
const fn f16(pt: u32, ch: u32) -> u32 { pt << 16 | ch << 3 | 2 }
const fn ff32(pt: u32, ch: u32) -> u32 { 1 << 22 | pt << 16 | ch << 3 | 4 }

// -- profile ----------------------------------------------------------------

/// `Profile` — the lcms2-crate profile type.
/// `C` is the lcms2 context-generic (default `GlobalContext`).
pub struct Profile<C = GlobalContext> {
    pub(crate) icc: Box<crate::profile::IccProfile>,
    _ctx: PhantomData<C>,
}

impl Profile<GlobalContext> {
    /// `cmsOpenProfileFromMem`
    pub fn new_icc(data: &[u8]) -> LCMSResult<Self> {
        match prof::open_profile_from_mem(None, data) {
            Some(icc) => Ok(Profile { icc, _ctx: PhantomData }),
            None => Err(last_or(types::ERROR_READ, "open_profile_from_mem failed")),
        }
    }
    /// `cmsOpenProfileFromFile`
    pub fn new_file<P: AsRef<std::path::Path>>(path: P) -> std::io::Result<Self> {
        let data = std::fs::read(path)?;
        Self::new_icc(&data)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.message))
    }
    /// `cmsCreate_sRGBProfile`
    pub fn new_srgb() -> Self {
        Self::wrap(virt::create_srgb_profile(None))
    }
    /// `cmsCreateRGBProfileTHR`
    pub fn new_rgb(
        white_point: &CIExyY, primaries: &CIEXYZTRIPLE,
        red: &ToneCurve, green: &ToneCurve, blue: &ToneCurve,
    ) -> LCMSResult<Self> {
        match virt::create_rgb_profile(
            None, Some(white_point), Some(primaries), Some([red, green, blue]),
        ) {
            Some(icc) => Ok(Profile { icc, _ctx: PhantomData }),
            None => Err(last_or(types::ERROR_WRITE, "create_rgb_profile failed")),
        }
    }
    /// `cmsCreateGrayProfileTHR`
    pub fn new_gray(white_point: &CIExyY, curve: &ToneCurve) -> LCMSResult<Self> {
        match virt::create_gray_profile(None, Some(white_point), Some(curve)) {
            Some(icc) => Ok(Profile { icc, _ctx: PhantomData }),
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
    pub fn new_placeholder() -> Self {
        Profile { icc: Box::new(crate::profile::IccProfile::default()), _ctx: PhantomData }
    }

    fn wrap(icc: Option<Box<crate::profile::IccProfile>>) -> Self {
        Profile { icc: icc.expect("lcms profile create failed"), _ctx: PhantomData }
    }
}

impl<C> Profile<C> {
    /// `cmsSaveProfileToMem`
    pub fn icc(&mut self) -> LCMSResult<Vec<u8>> {
        match prof::save_profile_to_mem(&mut self.icc) {
            Some(b) => Ok(b),
            None => Err(last_or(types::ERROR_WRITE, "save_profile_to_mem failed")),
        }
    }
    /// `cmsGetDeviceClass` — raw signature u32.
    pub fn device_class(&self) -> u32 { self.icc.device_class }
    /// `cmsGetColorSpace` — raw signature u32.
    pub fn color_space(&self) -> u32 { self.icc.color_space }
    /// `cmsGetPCS` — raw signature u32.
    pub fn pcs(&self) -> u32 { self.icc.pcs }
    /// `cmsGetEncodedICCversion`
    pub fn encoded_icc_version(&self) -> u32 { self.icc.version }
    /// `cmsGetHeaderAttributes`
    pub fn header_attributes(&self) -> u64 { self.icc.attributes }
    /// `cmsGetHeaderFlags`
    pub fn header_flags(&self) -> u32 { self.icc.flags }
    /// `cmsGetHeaderCreator`
    pub fn header_creator(&self) -> u32 { self.icc.creator }
    /// `cmsGetHeaderProfileID`
    pub fn header_profile_id(&self) -> crate::types::ProfileId { self.icc.profile_id.clone() }
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
    pub fn tag_count(&self) -> u32 { self.icc.tag_count }
    /// `cmsGetTagSignature`
    pub fn tag_signature(&self, i: u32) -> u32 {
        self.icc.tag_names.get(i as usize).copied().unwrap_or(0)
    }
    /// `cmsIsTag`
    pub fn is_tag(&self, sig: u32) -> bool {
        crate::profile::cms_is_tag(&self.icc, sig)
    }
    /// The raw parity-layer profile.
    pub fn inner(&self) -> &crate::profile::IccProfile { &self.icc }
    /// `&mut` — for plan-building (lazy materialization).
    pub fn inner_mut(&mut self) -> &mut crate::profile::IccProfile { &mut self.icc }
}

// -- transform --------------------------------------------------------------

/// `Transform<I, O>` — the lcms2-crate typed transform.
/// `I`/`O` are `Pod` pixel types (`[u8;3]`, `(u8,u8,u8)`, `RGB<u8>`, ...).
/// The per-run pixel cache is a `Mutex<TransformScratch>` so
/// `transform_pixels` is `&self` — matching the lcms2 API shape and
/// keeping the warm `_cmsCACHE` semantics.
pub struct Transform<I, O, C = GlobalContext, F = AllowCache> {
    plan: xform::TransformPlan,
    scratch: Mutex<xform::TransformScratch>,
    _i: PhantomData<I>,
    _o: PhantomData<O>,
    _c: PhantomData<C>,
    _f: PhantomData<F>,
}

impl<I: Copy + bytemuck::Pod, O: Copy + bytemuck::Pod>
    Transform<I, O, GlobalContext, AllowCache>
{
    /// `cmsCreateTransform` — perceptual intent, no flags.
    pub fn new(
        input: &Profile, in_format: PixelFormat,
        output: &Profile, out_format: PixelFormat,
        intent: Intent,
    ) -> LCMSResult<Self> {
        Self::with_flags(input, in_format, output, out_format, intent, Flags::NONE)
    }

    /// `cmsCreateTransform` with flags.
    pub fn with_flags(
        input: &Profile, in_format: PixelFormat,
        output: &Profile, out_format: PixelFormat,
        intent: Intent, flags: Flags,
    ) -> LCMSResult<Self> {
        // create_extended_transform wants &mut [Box<IccProfile>] — clone
        // the profiles' boxes so &Profile stays borrowed-immutable.
        let mut profs = [input.icc.clone(), output.icc.clone()];
        let mut bpc = [false, false];
        let intents = [intent as u32; 2];
        let states = [3.0f64; 2];
        let t = xform::create_extended_transform(
            None, &mut profs, &mut bpc, &intents, &states, None, 0,
            in_format.0, out_format.0, flags.0,
        )
        .ok_or_else(|| last_or(types::ERROR_UNDEFINED, "create_transform failed"))?;
        let xform::Transform { plan, scratch } = *t;
        Ok(Transform {
            plan,
            scratch: Mutex::new(scratch),
            _i: PhantomData, _o: PhantomData, _c: PhantomData, _f: PhantomData,
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
        let mut sc = self.scratch.lock().unwrap_or_else(|e| e.into_inner());
        self.plan.apply(&mut sc, ib, ob, n);
    }


    /// `transform_in_place` — same buffer in/out (I == O layout).
    /// Implemented as a copy+writeback — pixel-independent, so the
    /// copy preserves the transform's semantics exactly.
    pub fn transform_in_place(&self, buf: &mut [I]) {
        let tmp: Vec<I> = buf.to_vec();
        let n = buf.len() as u32;
        let mut sc = self.scratch.lock().unwrap_or_else(|e| e.into_inner());
        self.plan.apply(&mut sc, bytemuck::cast_slice(&tmp), bytemuck::cast_slice_mut(buf), n);
    }
}
