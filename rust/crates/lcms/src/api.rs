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

fn _unused<T>(id: i32, msg: &str) -> Result<T, Error> {
    Err(Error { id, message: msg.into() })
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
}

// -- Profile --------------------------------------------------------------

/// An owned ICC profile. `Send`; `freeze()` + `Arc` for shared reads.
pub struct Profile {
    pub(crate) icc: Box<crate::profile::IccProfile>,
}

impl Profile {
    fn wrap(icc: Option<Box<crate::profile::IccProfile>>, what: &str) -> Result<Self, Error> {
        icc.map(|icc| Profile { icc })
            .ok_or_else(|| Error { id: types::ERROR_READ, message: format!("{what} failed") })
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
    /// `cmsOpenProfileFromMem`
    pub fn open(data: &[u8]) -> Result<Self, Error> {
        Self::wrap(
            profile::open_profile_from_mem(None, data),
            "open_profile_from_mem",
        )
    }
    /// `cmsSaveProfileToMem` — serialized ICC bytes.
    pub fn save(&mut self) -> Result<Vec<u8>, Error> {
        profile::save_profile_to_mem(&mut self.icc)
            .ok_or_else(|| Error { id: types::ERROR_WRITE, message: "save_profile_to_mem failed".into() })
    }
    /// Eagerly materialize all tags — after `freeze`, `Arc<Profile>`
    /// reads via [`Profile::tag`] are `Sync`-safe.
    pub fn freeze(&mut self) {
        profile::freeze(&mut self.icc);
    }
    /// Read a tag object (`None` if absent/not frozen).
    pub fn tag(&self, sig: u32) -> Option<&TagObject> {
        profile::read_tag_frozen(&self.icc, sig)
    }
    /// Raw `&IccProfile` for the C-parity surface.
    pub fn icc(&self) -> &crate::profile::IccProfile {
        &self.icc
    }
    /// Raw `&mut IccProfile` — for plan building (lazy-materializes).
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

/// Per-run pixel scratch (~64 B) — `plan.scratch()`, or pooled.
pub type Scratch = xform::TransformScratch;

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
            std::mem::replace(&mut src.icc, Box::new(Default::default())),
            std::mem::replace(&mut dst.icc, Box::new(Default::default())),
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
        let t = t.ok_or_else(|| Error {
            id: types::ERROR_UNDEFINED,
            message: "create_transform failed".into(),
        })?;
        let xform::Transform { plan, .. } = *t;
        Ok(Plan { inner: plan })
    }

    /// Fresh per-run scratch — clones the seeded cache (~64 B).
    pub fn scratch(&self) -> Scratch {
        self.inner.scratch()
    }
    /// Evaluate `input` -> `output` — `&self` is `Sync`. `n` = pixel
    /// count: `input.len() / Format::pixel_size`.
    pub fn apply(&self, scratch: &mut Scratch, input: &[u8], output: &mut [u8]) {
        let b = crate::pack::t_bytes(self.inner.input_format) as usize;
        self.inner.apply(scratch, input, output,
            (input.len() / b.max(1)).max(1) as u32);
    }
    /// Evaluate `n` pixels.
    pub fn apply_n(&self, scratch: &mut Scratch, input: &[u8], output: &mut [u8], n: u32) {
        self.inner.apply(scratch, input, output, n);
    }
    /// Parallel apply — scratch-per-chunk, pixel-independent.
    pub fn apply_par(&self, input: &[u8], output: &mut [u8], n: u32, threads: usize) {
        self.inner.apply_par(input, output, n, threads);
    }
}
