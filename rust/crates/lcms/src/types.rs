//! Scalar types and small structs mirroring `lcms2.h`.
//!
//! The C typedefs are spelled out in each Rust signature (there is no point
//! aliasing `u16`); the named structs keep their C field order.

/// cmsSignature — a 4-byte big-endian tag/code.
pub type Signature = u32;
/// cmsColorSpaceSignature.
pub type ColorSpaceSignature = u32;
/// cmsS15Fixed16Number.
pub type S15Fixed16 = i32;
/// cmsU8Fixed8Number.
pub type U8Fixed8 = u16;
/// cmsU16Fixed16Number.
pub type U16Fixed16 = u32;
/// cmsContext is opaque in C; here it is a `&crate::context::Context`
/// (or `None`, matching C's NULL → global context).
pub use crate::context::Context;

/// cmsFloat64Number and cmsFloat32Number stay `f64`/`f32` in signatures.

// --- small POD structs, C field order preserved ---

/// cmsCIEXYZ
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
pub struct CieXyz {
    #[allow(non_snake_case)]
    pub X: f64,
    #[allow(non_snake_case)]
    pub Y: f64,
    #[allow(non_snake_case)]
    pub Z: f64,
}

/// cmsCIExyY — capital `Y` is the luminance, matching C.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
pub struct CiexyY {
    pub x: f64,
    pub y: f64,
    #[allow(non_snake_case)]
    pub Y: f64,
}

/// cmsCIELab
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
pub struct CieLab {
    #[allow(non_snake_case)]
    pub L: f64,
    pub a: f64,
    pub b: f64,
}

/// cmsCIELCh
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
pub struct CieLCh {
    #[allow(non_snake_case)]
    pub L: f64,
    #[allow(non_snake_case)]
    pub C: f64,
    pub h: f64,
}

/// cmsCIExyYTRIPLE
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
pub struct CiexyYTriple {
    #[allow(non_snake_case)]
    pub Red: CiexyY,
    #[allow(non_snake_case)]
    pub Green: CiexyY,
    #[allow(non_snake_case)]
    pub Blue: CiexyY,
}

/// cmsVEC3 (internal; VX=0, VY=1, VZ=2)
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
pub struct Vec3 {
    pub n: [f64; 3],
}

/// cmsMAT3 (internal)
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
pub struct Mat3 {
    pub v: [Vec3; 3],
}

/// cmsProfileID
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
pub struct ProfileId {
    pub id8: [u8; 16],
}

/// cmsDateTimeNumber (stored big-endian on disk)
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
pub struct DateTimeNumber {
    pub year: u16,
    pub month: u16,
    pub day: u16,
    pub hours: u16,
    pub minutes: u16,
    pub seconds: u16,
}

/// cmsEncodedXYZNumber
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
pub struct EncodedXyzNumber {
    #[allow(non_snake_case)]
    pub X: S15Fixed16,
    #[allow(non_snake_case)]
    pub Y: S15Fixed16,
    #[allow(non_snake_case)]
    pub Z: S15Fixed16,
}

/// cmsICCMeasurementConditions
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
pub struct IccMeasurementConditions {
    pub observer: u32,
    pub backing: EncodedXyzNumber,
    pub geometry: u32,
    pub flare: f64,
    pub illuminant: u32,
}

// --- constants from lcms2.h ---

pub const D50X: f64 = 0.9642;
pub const D50Y: f64 = 1.0;
pub const D50Z: f64 = 0.8249;

/// cmsPERCEPTUAL_BLACK_XYZ
pub const PERCEPTUAL_BLACK_X: f64 = 0.00336;
pub const PERCEPTUAL_BLACK_Y: f64 = 0.0034731;
pub const PERCEPTUAL_BLACK_Z: f64 = 0.00287;

// Intents
pub const INTENT_PERCEPTUAL: u32 = 0;
pub const INTENT_RELATIVE_COLORIMETRIC: u32 = 1;
pub const INTENT_SATURATION: u32 = 2;
pub const INTENT_ABSOLUTE_COLORIMETRIC: u32 = 3;
pub const INTENT_PRESERVE_K_ONLY_PERCEPTUAL: u32 = 10;
pub const INTENT_PRESERVE_K_ONLY_RELATIVE_COLORIMETRIC: u32 = 11;
pub const INTENT_PRESERVE_K_ONLY_SATURATION: u32 = 12;
pub const INTENT_PRESERVE_K_PLANE_PERCEPTUAL: u32 = 13;
pub const INTENT_PRESERVE_K_PLANE_RELATIVE_COLORIMETRIC: u32 = 14;
pub const INTENT_PRESERVE_K_PLANE_SATURATION: u32 = 15;

// Pixel type codes (COLORSPACE_SH payload)
pub const PT_ANY: u32 = 0;
pub const PT_GRAY: u32 = 3;
pub const PT_RGB: u32 = 4;
pub const PT_CMY: u32 = 5;
pub const PT_CMYK: u32 = 6;
pub const PT_YCBCR: u32 = 7;
pub const PT_YUV: u32 = 8; // Lu'v'
pub const PT_XYZ: u32 = 9;
pub const PT_LAB: u32 = 10;
pub const PT_YUVK: u32 = 11; // Lu'v'K
pub const PT_HSV: u32 = 12;
pub const PT_HLS: u32 = 13;
pub const PT_YXY: u32 = 14;
pub const PT_MCH1: u32 = 15;
pub const PT_MCH2: u32 = 16;
pub const PT_MCH3: u32 = 17;
pub const PT_MCH4: u32 = 18;
pub const PT_MCH5: u32 = 19;
pub const PT_MCH6: u32 = 20;
pub const PT_MCH7: u32 = 21;
pub const PT_MCH8: u32 = 22;
pub const PT_MCH9: u32 = 23;
pub const PT_MCH10: u32 = 24;
pub const PT_MCH11: u32 = 25;
pub const PT_MCH12: u32 = 26;
pub const PT_MCH13: u32 = 27;
pub const PT_MCH14: u32 = 28;
pub const PT_MCH15: u32 = 29;
pub const PT_LABV2: u32 = 30; // identical to PT_LAB but using the V2 old encoding

// Transform flags (dwFlags)
pub const FLAGS_NOCACHE: u32 = 0x0040;
pub const FLAGS_NOOPTIMIZE: u32 = 0x0100;
pub const FLAGS_NULLTRANSFORM: u32 = 0x0200;
pub const FLAGS_GAMUTCHECK: u32 = 0x1000;
pub const FLAGS_SOFTPROOFING: u32 = 0x4000;
pub const FLAGS_BLACKPOINTCOMPENSATION: u32 = 0x2000;
pub const FLAGS_NOWHITEONWHITEFIXUP: u32 = 0x0004;
pub const FLAGS_HIGHRESPRECALC: u32 = 0x0400;
pub const FLAGS_LOWRESPRECALC: u32 = 0x0800;
pub const FLAGS_8BITS_DEVICELINK: u32 = 0x0008;
pub const FLAGS_GUESSDEVICECLASS: u32 = 0x0020;
pub const FLAGS_KEEP_SEQUENCE: u32 = 0x0080;
pub const FLAGS_FORCE_CLUT: u32 = 0x0002;
pub const FLAGS_CLUT_POST_LINEARIZATION: u32 = 0x0001;
pub const FLAGS_CLUT_PRE_LINEARIZATION: u32 = 0x0010;
pub const FLAGS_NONEGATIVES: u32 = 0x8000;
pub const FLAGS_COPY_ALPHA: u32 = 0x0400_0000;
pub const FLAGS_NODEFAULTRESOURCEDEF: u32 = 0x0100_0000;
pub const FLAGS_GRIDPOINTS_MASK: u32 = 0x00FF_0000;

/// cmsFLAGS_GRIDPOINTS(n): `((n) & 0xFF) << 16`
pub fn flags_gridpoints(n: u32) -> u32 {
    (n & 0xFF) << 16
}

// Error codes (cmsERROR_*)
pub const ERROR_UNDEFINED: i32 = 0;
pub const ERROR_FILE: i32 = 1;
pub const ERROR_RANGE: i32 = 2;
pub const ERROR_INTERNAL: i32 = 3;
pub const ERROR_NULL: i32 = 4;
pub const ERROR_READ: i32 = 5;
pub const ERROR_SEEK: i32 = 6;
pub const ERROR_WRITE: i32 = 7;
pub const ERROR_UNKNOWN_EXTENSION: i32 = 8;
pub const ERROR_COLORSPACE_CHECK: i32 = 9;
pub const ERROR_ALREADY_DEFINED: i32 = 10;
pub const ERROR_BAD_SIGNATURE: i32 = 11;
pub const ERROR_CORRUPTION_DETECTED: i32 = 12;
pub const ERROR_NOT_SUITABLE: i32 = 13;

/// LCMS_VERSION
pub const LCMS_VERSION: u32 = 2200;

/// `cmsJCh` — CIECAM02 JCh correlates.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
pub struct JCh {
    #[allow(non_snake_case)]
    pub J: f64,
    #[allow(non_snake_case)]
    pub C: f64,
    pub h: f64,
}

/// `AVG_SURROUND`
pub const AVG_SURROUND: u32 = 1;
/// `DIM_SURROUND`
pub const DIM_SURROUND: u32 = 2;
/// `DARK_SURROUND`
pub const DARK_SURROUND: u32 = 3;
/// `CUTSHEET_SURROUND`
pub const CUTSHEET_SURROUND: u32 = 4;
/// `D_CALCULATE`
pub const D_CALCULATE: f64 = -1.0;

/// `cmsViewingConditions`
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
pub struct ViewingConditions {
    #[allow(non_snake_case)]
    pub white_point: CieXyz,
    #[allow(non_snake_case)]
    pub Yb: f64,
    #[allow(non_snake_case)]
    pub La: f64,
    pub surround: u32,
    #[allow(non_snake_case)]
    pub D_value: f64,
}

/// `pow` with C/glibc NaN semantics: a NaN base or exponent propagates the
/// *input* NaN (with its sign), matching glibc `pow`. Rust's `f64::powf`
/// canonicalizes any NaN input to a positive quiet NaN, which diverges in
/// bit-exact tests whenever a NaN flows into `pow` (e.g. CIECAM02's
/// `pow(J/100, 0.5)` where J=-NaN). `pow(x, 0)` returns 1.0 for any x
/// including NaN, per IEEE/C. For non-NaN inputs this is identical to `powf`.
pub(crate) trait CpowExt {
    fn cpow(self, exp: Self) -> Self;
    /// `log`/`ln` with glibc domain-error NaN sign (negative → `-NaN`).
    fn cln(self) -> Self;
    /// `log10` with glibc domain-error NaN sign.
    fn clog10(self) -> Self;
    /// `acos` with glibc domain-error NaN sign (|x|>1 → `-NaN`).
    fn cacos(self) -> Self;
    /// `asin` with glibc domain-error NaN sign.
    fn casin(self) -> Self;
    /// `atanh` with glibc domain-error NaN sign.
    fn catanh(self) -> Self;
    /// `acosh` with glibc domain-error NaN sign.
    fn cacosh(self) -> Self;
}

/// glibc domain-error NaN — `-nan` (`0xfff8000000000000`). Rust's
/// `ln`/`log10`/`acos`/`asin`/`atanh`/`acosh`/`pow` return the positive
/// quiet NaN `0x7ff8000000000000` for out-of-domain inputs and NaN inputs,
/// while glibc returns a *negative* quiet NaN — a bit-visible difference
/// that propagates through formulas (CIECAM02, parametric curves).
#[inline]
fn domain_nan() -> f64 {
    f64::from_bits(0xfff8000000000000)
}

impl CpowExt for f64 {
    // `black_box(exp)` keeps the exponent an opaque runtime value: if a
    // literal `0.5` propagated into `powf`, LLVM would lower `pow(x, 0.5)`
    // to the exact `sqrt` instruction, which differs by 1 ULP from glibc
    // `pow(x, 0.5)` on some inputs (bit-exactness). Hiding the constant
    // forces a real `pow` while staying inlinable (no call overhead on the
    // hot curve-eval path).
    #[inline]
    fn cpow(self, exp: f64) -> f64 {
        let exp = std::hint::black_box(exp);
        if exp == 0.0 {
            1.0
        } else if self.is_nan() {
            self
        } else if exp.is_nan() {
            exp
        } else {
            lcms_sys::pow(self, exp)
        }
    }
    #[inline]
    fn cln(self) -> f64 {
        if self.is_nan() {
            self
        } else if self < 0.0 {
            domain_nan()
        } else {
            lcms_sys::ln(self)
        }
    }
    #[inline]
    fn clog10(self) -> f64 {
        if self.is_nan() {
            self
        } else if self < 0.0 {
            domain_nan()
        } else {
            lcms_sys::log10(self)
        }
    }
    #[inline]
    fn cacos(self) -> f64 {
        if self.is_nan() {
            self
        } else if self < -1.0 || self > 1.0 {
            domain_nan()
        } else {
            lcms_sys::acos(self)
        }
    }
    #[inline]
    fn casin(self) -> f64 {
        if self.is_nan() {
            self
        } else if self < -1.0 || self > 1.0 {
            domain_nan()
        } else {
            lcms_sys::asin(self)
        }
    }
    #[inline]
    fn catanh(self) -> f64 {
        if self.is_nan() {
            self
        } else if self <= -1.0 || self >= 1.0 {
            domain_nan()
        } else {
            lcms_sys::atanh(self)
        }
    }
    #[inline]
    fn cacosh(self) -> f64 {
        if self.is_nan() {
            self
        } else if self < 1.0 {
            domain_nan()
        } else {
            lcms_sys::acosh(self)
        }
    }
}
