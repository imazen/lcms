//! FFI bindings to the reference lcms2 C library (built by build.rs).
//!
//! Test-only oracle crate. Every `c_parity_*` test in `lcms` calls the real
//! exported C symbols — or, for `static`/`cmsINLINE` functions, the shim
//! wrappers compiled from `shims/` — and compares against the Rust port.
//!
//! Naming convention: `c_<name>` = direct link to the C symbol;
//! `cref_*` in shims = a same-source wrapper around `static`/`cmsINLINE` code.

#![allow(non_snake_case)]
#![allow(non_camel_case_types)]
#![allow(dead_code)]

use std::ffi::c_void;

pub type Bool = i32; // cmsBool
pub type U8 = u8; // cmsUInt8Number
pub type U16 = u16; // cmsUInt16Number
pub type U32 = u32; // cmsUInt32Number
pub type I32 = i32; // cmsInt32Number
pub type S15Fixed16 = i32; // cmsS15Fixed16Number
pub type F64 = f64; // cmsFloat64Number
pub type F32 = f32; // cmsFloat32Number
pub type Context = *mut c_void; // cmsContext
pub type Handle = *mut c_void; // cmsHANDLE
pub type HProfile = *mut c_void; // cmsHPROFILE
pub type ColorSpaceSignature = u32;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Vec3 {
    pub n: [F64; 3],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Mat3 {
    pub v: [Vec3; 3],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct CieXyz {
    pub X: F64,
    pub Y: F64,
    pub Z: F64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct CiexyY {
    pub x: F64,
    pub y: F64,
    pub Y: F64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct CieLab {
    pub L: F64,
    pub a: F64,
    pub b: F64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct CieLCh {
    pub L: F64,
    pub C: F64,
    pub h: F64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct CiexyYTriple {
    pub Red: CiexyY,
    pub Green: CiexyY,
    pub Blue: CiexyY,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ProfileID {
    pub id8: [U8; 16],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ToneCurve; // opaque cmsToneCurve

// ---- cmsInterpParams (lcms2_plugin.h) ----

pub type InterpFn16 = unsafe extern "C" fn(*const U16, *mut U16, *const InterpParams);
pub type InterpFnFloat = unsafe extern "C" fn(*const F32, *mut F32, *const InterpParams);

#[repr(C)]
#[derive(Clone, Copy)]
pub union InterpFunction {
    pub lerp16: Option<InterpFn16>,
    pub lerp_float: Option<InterpFnFloat>,
}

#[repr(C)]
pub struct InterpParams {
    pub context_id: Context,
    pub dw_flags: U32,
    pub n_inputs: U32,
    pub n_outputs: U32,
    pub n_samples: [U32; 15],
    pub domain: [U32; 15],
    pub opta: [U32; 15],
    pub table: *const c_void,
    pub interpolation: InterpFunction,
}

unsafe extern "C" {
    // ---- cmsmtrx.c ----
    pub fn _cmsVEC3init(r: *mut Vec3, x: F64, y: F64, z: F64);
    pub fn _cmsVEC3minus(r: *mut Vec3, a: *const Vec3, b: *const Vec3);
    pub fn _cmsVEC3cross(r: *mut Vec3, u: *const Vec3, v: *const Vec3);
    pub fn _cmsVEC3dot(u: *const Vec3, v: *const Vec3) -> F64;
    pub fn _cmsVEC3length(a: *const Vec3) -> F64;
    pub fn _cmsVEC3distance(a: *const Vec3, b: *const Vec3) -> F64;
    pub fn _cmsMAT3identity(a: *mut Mat3);
    pub fn _cmsMAT3isIdentity(a: *const Mat3) -> Bool;
    pub fn _cmsMAT3per(r: *mut Mat3, a: *const Mat3, b: *const Mat3);
    pub fn _cmsMAT3inverse(a: *const Mat3, b: *mut Mat3) -> Bool;
    pub fn _cmsMAT3solve(x: *mut Vec3, a: *mut Mat3, b: *mut Vec3) -> Bool;
    pub fn _cmsMAT3eval(r: *mut Vec3, a: *const Mat3, v: *const Vec3);

    // ---- cmshalf.c ----
    pub fn _cmsHalf2Float(h: U16) -> F32;
    pub fn _cmsFloat2Half(flt: F32) -> U16;

    // ---- cmspcs.c ----
    pub fn cmsXYZ2xyY(dest: *mut CiexyY, source: *const CieXyz);
    pub fn cmsxyY2XYZ(dest: *mut CieXyz, source: *const CiexyY);
    pub fn cmsXYZ2Lab(white_point: *const CieXyz, lab: *mut CieLab, xyz: *const CieXyz);
    pub fn cmsLab2XYZ(white_point: *const CieXyz, xyz: *mut CieXyz, lab: *const CieLab);
    pub fn cmsLabEncoded2FloatV2(lab: *mut CieLab, w_lab: *const U16);
    pub fn cmsLabEncoded2Float(lab: *mut CieLab, w_lab: *const U16);
    pub fn cmsFloat2LabEncodedV2(w_lab: *mut U16, f_lab: *const CieLab);
    pub fn cmsFloat2LabEncoded(w_lab: *mut U16, f_lab: *const CieLab);
    pub fn cmsLab2LCh(lch: *mut CieLCh, lab: *const CieLab);
    pub fn cmsLCh2Lab(lab: *mut CieLab, lch: *const CieLCh);
    pub fn cmsFloat2XYZEncoded(xyz: *mut U16, f_xyz: *const CieXyz);
    pub fn cmsXYZEncoded2Float(f_xyz: *mut CieXyz, xyz: *const U16);
    pub fn cmsDeltaE(lab1: *const CieLab, lab2: *const CieLab) -> F64;
    pub fn cmsCIE94DeltaE(lab1: *const CieLab, lab2: *const CieLab) -> F64;
    pub fn cmsBFDdeltaE(lab1: *const CieLab, lab2: *const CieLab) -> F64;
    pub fn cmsCMCdeltaE(lab1: *const CieLab, lab2: *const CieLab, l: F64, c: F64) -> F64;
    pub fn cmsCIE2000DeltaE(
        lab1: *const CieLab,
        lab2: *const CieLab,
        kl: F64,
        kc: F64,
        kh: F64,
    ) -> F64;
    pub fn _cmsReasonableGridpointsByColorspace(colorspace: ColorSpaceSignature, flags: U32)
    -> U32;
    pub fn _cmsEndPointsBySpace(
        space: ColorSpaceSignature,
        white: *mut *const U16,
        black: *mut *const U16,
        n_outputs: *mut U32,
    ) -> Bool;
    pub fn cmsChannelsOfColorSpace(colorspace: ColorSpaceSignature) -> I32;
    pub fn cmsChannelsOf(colorspace: ColorSpaceSignature) -> U32;
    pub fn _cmsICCcolorSpace(our_notation: i32) -> ColorSpaceSignature;
    pub fn _cmsLCMScolorSpace(profile_space: ColorSpaceSignature) -> i32;

    // ---- cmsplugin.c ----
    pub fn _cms15Fixed16toDouble(fix32: S15Fixed16) -> F64;
    pub fn _cmsDoubleTo15Fixed16(v: F64) -> S15Fixed16;
    pub fn _cms8Fixed8toDouble(fix8: U16) -> F64;
    pub fn _cmsDoubleTo8Fixed8(val: F64) -> U16;
    pub fn cmsGetEncodedCMMversion() -> U32;

    // ---- cmswtpnt.c ----
    pub fn cmsD50_XYZ() -> *const CieXyz;
    pub fn cmsD50_xyY() -> *const CiexyY;
    pub fn cmsWhitePointFromTemp(white_point: *mut CiexyY, temp_k: F64) -> Bool;
    pub fn cmsTempFromWhitePoint(temp_k: *mut F64, white_point: *const CiexyY) -> Bool;
    pub fn cmsAdaptToIlluminant(
        result: *mut CieXyz,
        world_white: *const CieXyz,
        illuminant: *const CieXyz,
        value: *const CieXyz,
    ) -> Bool;
    pub fn _cmsAdaptationMatrix(
        r: *mut Mat3,
        cone_matrix: *const Mat3,
        from_ill: *const CieXyz,
        to_ill: *const CieXyz,
    ) -> Bool;
    pub fn _cmsBuildRGB2XYZtransferMatrix(
        r: *mut Mat3,
        white_pt: *const CiexyY,
        primrs: *const CiexyYTriple,
    ) -> Bool;

    // ---- cmserr.c / cmsplugin.c misc ----
    pub fn cmsGetHeaderRenderingIntent(h_profile: HProfile) -> U32;

    // ---- cmsintrp.c ----
    pub fn _cmsComputeInterpParamsEx(
        context_id: Context,
        n_samples: *const U32,
        input_chan: U32,
        output_chan: U32,
        table: *const c_void,
        dw_flags: U32,
    ) -> *mut InterpParams;
    pub fn _cmsComputeInterpParams(
        context_id: Context,
        n_samples: U32,
        input_chan: U32,
        output_chan: U32,
        table: *const c_void,
        dw_flags: U32,
    ) -> *mut InterpParams;
    pub fn _cmsFreeInterpParams(p: *mut InterpParams);

    // ---- shims (wrappers around cmsINLINE / static code) ----
    pub fn cref_quick_floor(val: F64) -> i32;
    pub fn cref_quick_floor_word(d: F64) -> U16;
    pub fn cref_quick_saturate_word(d: F64) -> U16;
    pub fn cref_to_fixed_domain(a: i32) -> S15Fixed16;
    pub fn cref_from_fixed_domain(a: S15Fixed16) -> i32;
}

/// cmsGetEncodedCMMversion sanity: returns LCMS_VERSION (2200).
pub fn encoded_version() -> U32 {
    unsafe { cmsGetEncodedCMMversion() }
}
