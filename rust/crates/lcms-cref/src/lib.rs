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
pub type U64 = u64; // cmsUInt64Number
pub type I32 = i32; // cmsInt32Number
pub type S15Fixed16 = i32; // cmsS15Fixed16Number
pub type F64 = f64; // cmsFloat64Number
pub type F32 = f32; // cmsFloat32Number
pub type Context = *mut c_void; // cmsContext
pub type Handle = *mut c_void; // Handle
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

/// Opaque `cmsToneCurve`
#[repr(C)]
pub struct ToneCurve {
    _private: [u8; 0],
}

/// `cmsJCh`
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct JCh {
    pub j: F64,
    pub c_: F64,
    pub h: F64,
}

/// `cmsViewingConditions`
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ViewingConditions {
    pub white_point: CieXyz,
    pub yb: F64,
    pub la: F64,
    pub surround: U32,
    pub d_value: F64,
}

/// `cmsCurveSegment` (lcms2.h)
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CurveSegment {
    pub x0: F32,
    pub x1: F32,
    pub r#type: i32,
    pub params: [F64; 10],
    pub n_grid_points: U32,
    pub sampled_points: *const F32,
}

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

#[repr(C)]
pub struct CmsTagTypeHandler {
    pub signature: U32,
    pub read_ptr: unsafe extern "C" fn(*mut CmsTagTypeHandler, Handle, *mut U32, U32) -> Handle,
    pub write_ptr: unsafe extern "C" fn(*mut CmsTagTypeHandler, Handle, Handle, U32) -> i32,
    pub dup_ptr: unsafe extern "C" fn(*mut CmsTagTypeHandler, Handle, U32) -> Handle,
    pub free_ptr: unsafe extern "C" fn(*mut CmsTagTypeHandler, Handle),
    pub context_id: Context,
    pub icc_version: U32,
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
    pub fn dbg_lab2lch(lab: *const CieLab, out: *mut f64);
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

    // ---- cmsgamma.c ----
    pub fn cmsBuildParametricToneCurve(
        context_id: Context,
        r#type: i32,
        params: *const F64,
    ) -> *mut ToneCurve;
    pub fn cmsBuildGamma(context_id: Context, gamma: F64) -> *mut ToneCurve;
    pub fn cmsBuildTabulatedToneCurve16(
        context_id: Context,
        n_entries: U32,
        values: *const U16,
    ) -> *mut ToneCurve;
    pub fn cmsBuildTabulatedToneCurveFloat(
        context_id: Context,
        n_entries: U32,
        values: *const F32,
    ) -> *mut ToneCurve;
    pub fn cmsBuildSegmentedToneCurve(
        context_id: Context,
        n_segments: U32,
        segments: *const CurveSegment,
    ) -> *mut ToneCurve;
    pub fn cmsEvalToneCurveFloat(curve: *const ToneCurve, v: F32) -> F32;
    pub fn cmsEvalToneCurve16(curve: *const ToneCurve, v: U16) -> U16;
    pub fn cmsIsToneCurveLinear(curve: *const ToneCurve) -> Bool;
    pub fn cmsIsToneCurveMonotonic(curve: *const ToneCurve) -> Bool;
    pub fn cmsIsToneCurveDescending(curve: *const ToneCurve) -> Bool;
    pub fn cmsIsToneCurveMultisegment(curve: *const ToneCurve) -> Bool;
    pub fn cmsGetToneCurveParametricType(curve: *const ToneCurve) -> i32;
    pub fn cmsEstimateGamma(curve: *const ToneCurve, precision: F64) -> F64;
    pub fn cmsReverseToneCurveEx(n_result_samples: U32, curve: *const ToneCurve) -> *mut ToneCurve;
    pub fn cmsReverseToneCurve(curve: *const ToneCurve) -> *mut ToneCurve;
    pub fn cmsJoinToneCurve(
        context_id: Context,
        x: *const ToneCurve,
        y: *const ToneCurve,
        n_points: U32,
    ) -> *mut ToneCurve;
    pub fn cmsSmoothToneCurve(tab: *mut ToneCurve, lambda: F64) -> Bool;
    pub fn cmsGetToneCurveEstimatedTableEntries(curve: *const ToneCurve) -> U32;
    pub fn cmsGetToneCurveEstimatedTable(curve: *const ToneCurve) -> *const U16;
    pub fn cmsDupToneCurve(curve: *const ToneCurve) -> *mut ToneCurve;
    pub fn cmsFreeToneCurve(curve: *mut ToneCurve);

    // ---- cmserr.c / cmsplugin.c misc ----
    pub fn cmsGetHeaderRenderingIntent(h_profile: HProfile) -> U32;

    // ---- cmscam02.c ----
    pub fn cmsCIECAM02Init(context_id: Context, vc: *const ViewingConditions) -> Handle;
    pub fn cmsCIECAM02Done(model: Handle);
    pub fn cmsCIECAM02Forward(model: Handle, in_: *const CieXyz, out: *mut JCh);
    pub fn cmsCIECAM02Reverse(model: Handle, in_: *const JCh, out: *mut CieXyz);

    // ---- io layer (cmsio0.c + cmsplugin.c primitives) ----
    pub fn cmsOpenIOhandlerFromNULL(context_id: Context) -> Handle;
    pub fn cmsOpenIOhandlerFromMem(
        context_id: Context,
        buffer: *mut u8,
        size: U32,
        access_mode: *const u8,
    ) -> Handle;
    pub fn cmsCloseIOhandler(io: Handle) -> i32;
    pub fn cmsOpenIOhandlerFromFile(context_id: Context, name: *const i8, access: *const u8) -> Handle;
    // --- cmsio0 profile API ---
    pub fn cmsOpenProfileFromFile(name: *const i8, access: *const u8) -> *mut c_void;
    pub fn cmsOpenProfileFromMem(mem: *const c_void, size: u32) -> *mut c_void;
    pub fn cmsOpenProfileFromIOhandlerTHR(ctx: *mut c_void, io: Handle) -> *mut c_void;
    pub fn cmsSaveProfileToFile(p: *mut c_void, name: *const i8) -> i32;
    pub fn cmsSaveProfileToIOhandler(p: *mut c_void, io: Handle) -> i32;
    pub fn cmsCloseProfile(p: *mut c_void) -> i32;
    pub fn cmsGetTagCount(p: *const c_void) -> i32;
    pub fn cmsGetTagSignature(p: *const c_void, n: u32) -> u32;
    pub fn cmsLinkTag(p: *mut c_void, sig: u32, dest: u32) -> i32;
    pub fn cmsTagLinkedTo(p: *const c_void, sig: u32) -> u32;
    pub fn cmsGetTagOffsetAndSize(p: *const c_void, n: u32, off: *mut u32, sz: *mut u32) -> i32;
    pub fn cmsIsTag(p: *const c_void, sig: u32) -> i32;
    pub fn cmsReadTag(p: *mut c_void, sig: u32) -> *mut c_void;
    pub fn cmsWriteTag(p: *mut c_void, sig: u32, data: *const c_void) -> i32;
    pub fn cmsReadRawTag(p: *mut c_void, sig: u32, buf: *mut c_void, size: u32) -> u32;
    pub fn cmsWriteRawTag(p: *mut c_void, sig: u32, data: *const c_void, size: u32) -> i32;
    pub fn cmsSaveProfileToMem(p: *mut c_void, mem: *mut c_void, size: *mut u32) -> i32;
    pub fn cmsCreateProfilePlaceholder(ctx: *mut c_void) -> *mut c_void;
    // --- cmsvirt ---
    pub fn cmsCreate_sRGBProfile() -> *mut c_void;
    pub fn cmsCreateRGBProfileTHR(ctx: Context, white: *const c_void, primaries: *const c_void, transfer: *const *mut c_void) -> *mut c_void;
    pub fn cmsCreateLab2Profile(wp: *const c_void) -> *mut c_void;
    pub fn cmsCreateLab4Profile(wp: *const c_void) -> *mut c_void;
    pub fn cmsCreateXYZProfile() -> *mut c_void;
    pub fn _cmsReadUInt8Number(io: Handle, n: *mut U8) -> i32;
    pub fn _cmsReadUInt16Number(io: Handle, n: *mut U16) -> i32;
    pub fn _cmsReadUInt32Number(io: Handle, n: *mut U32) -> i32;
    pub fn _cmsReadUInt64Number(io: Handle, n: *mut U64) -> i32;
    pub fn _cmsReadFloat32Number(io: Handle, n: *mut F32) -> i32;
    pub fn _cmsRead15Fixed16Number(io: Handle, n: *mut F64) -> i32;
    pub fn _cmsReadXYZNumber(io: Handle, xyz: *mut CieXyz) -> i32;
    pub fn _cmsWriteUInt8Number(io: Handle, n: U8) -> i32;
    pub fn _cmsWriteUInt16Number(io: Handle, n: U16) -> i32;
    pub fn _cmsWriteUInt32Number(io: Handle, n: U32) -> i32;
    pub fn _cmsWriteUInt64Number(io: Handle, n: *const U64) -> i32;
    pub fn _cmsWriteFloat32Number(io: Handle, n: F32) -> i32;
    pub fn _cmsWrite15Fixed16Number(io: Handle, n: F64) -> i32;
    pub fn _cmsWriteXYZNumber(io: Handle, xyz: *const CieXyz) -> i32;
    pub fn _cmsReadTypeBase(io: Handle) -> U32;
    pub fn _cmsWriteTypeBase(io: Handle, sig: U32) -> i32;
    pub fn _cmsReadAlignment(io: Handle) -> i32;
    pub fn _cmsWriteAlignment(io: Handle) -> i32;
    pub fn _cmsAdjustEndianess16(w: U16) -> U16;
    pub fn _cmsAdjustEndianess32(v: U32) -> U32;
    pub fn _cmsAdjustEndianess64(result: *mut U64, qword: *const U64);

    // ---- cmstypes.c ----
    pub fn _cmsGetTagTypeHandler(context_id: Context, sig: U32) -> *mut CmsTagTypeHandler;

    // ---- cmsnamed.c ----
    pub fn cmsMLUalloc(context_id: Context, n_items: U32) -> Handle;
    pub fn cmsMLUfree(mlu: Handle);
    pub fn cmsMLUdup(mlu: Handle) -> Handle;
    pub fn cmsMLUsetASCII(mlu: Handle, lang: *const u8, cntry: *const u8, ascii: *const u8) -> i32;
    pub fn cmsMLUsetUTF8(mlu: Handle, lang: *const u8, cntry: *const u8, utf8: *const u8) -> i32;
    pub fn cmsMLUsetWide(mlu: Handle, lang: *const u8, cntry: *const u8, wide: *const i32) -> i32;
    pub fn cmsMLUgetASCII(
        mlu: Handle,
        lang: *const u8,
        cntry: *const u8,
        buffer: *mut u8,
        buffer_size: U32,
    ) -> U32;
    pub fn cmsMLUgetUTF8(
        mlu: Handle,
        lang: *const u8,
        cntry: *const u8,
        buffer: *mut u8,
        buffer_size: U32,
    ) -> U32;
    pub fn cmsMLUgetWide(
        mlu: Handle,
        lang: *const u8,
        cntry: *const u8,
        buffer: *mut i32,
        buffer_size: U32,
    ) -> U32;
    pub fn cmsMLUgetTranslation(
        mlu: Handle,
        lang: *const u8,
        cntry: *const u8,
        obt_lang: *mut u8,
        obt_cntry: *mut u8,
    ) -> i32;
    pub fn cmsMLUtranslationsCount(mlu: Handle) -> U32;
    pub fn cmsMLUtranslationsCodes(mlu: Handle, idx: U32, lang: *mut u8, cntry: *mut u8) -> i32;
    pub fn cmsAllocNamedColorList(
        context_id: Context,
        n: U32,
        colorant_count: U32,
        prefix: *const u8,
        suffix: *const u8,
    ) -> Handle;
    pub fn cmsFreeNamedColorList(list: Handle);
    pub fn cmsDupNamedColorList(list: Handle) -> Handle;
    pub fn cmsAppendNamedColor(
        list: Handle,
        name: *const u8,
        pcs: *const U16,
        colorant: *const U16,
    ) -> i32;
    pub fn cmsNamedColorCount(list: Handle) -> U32;
    pub fn cmsNamedColorInfo(
        list: Handle,
        n_color: U32,
        name: *mut u8,
        prefix: *mut u8,
        suffix: *mut u8,
        pcs: *mut U16,
        colorant: *mut U16,
    ) -> i32;
    pub fn cmsNamedColorIndex(list: Handle, name: *const u8) -> i32;
    pub fn cmsstrcasecmp(s1: *const u8, s2: *const u8) -> i32;
    pub fn _cmsStageAllocNamedColor(list: Handle, use_pcs: i32) -> Handle;
    pub fn cmsAllocProfileSequenceDescription(context_id: Context, n: U32) -> Handle;
    pub fn cmsFreeProfileSequenceDescription(pseq: Handle);
    pub fn cmsDupProfileSequenceDescription(pseq: Handle) -> Handle;
    pub fn cmsDictAlloc(context_id: Context) -> Handle;
    pub fn cmsDictFree(dict: Handle);
    pub fn cmsDictDup(dict: Handle) -> Handle;
    pub fn cmsDictGetEntryList(dict: Handle) -> *const c_void;
    pub fn cmsDictAddEntry(
        dict: Handle,
        name: *const i32,
        value: *const i32,
        display_name: Handle,
        display_value: Handle,
    ) -> i32;

    // ---- cmslut.c ----
    pub fn cmsStageAllocIdentity(context_id: Context, n_channels: U32) -> Handle;
    pub fn cmsStageAllocToneCurves(
        context_id: Context,
        n_channels: U32,
        curves: *const *mut ToneCurve,
    ) -> Handle;
    pub fn cmsStageAllocMatrix(
        context_id: Context,
        rows: U32,
        cols: U32,
        matrix: *const F64,
        offset: *const F64,
    ) -> Handle;
    pub fn cmsStageAllocCLut16bit(
        context_id: Context,
        n_grid_points: U32,
        input_chan: U32,
        output_chan: U32,
        table: *const U16,
    ) -> Handle;
    pub fn cmsStageAllocCLut16bitGranular(
        context_id: Context,
        clut_points: *const U32,
        input_chan: U32,
        output_chan: U32,
        table: *const U16,
    ) -> Handle;
    pub fn cmsStageAllocCLutFloat(
        context_id: Context,
        n_grid_points: U32,
        input_chan: U32,
        output_chan: U32,
        table: *const F32,
    ) -> Handle;
    pub fn cmsStageDup(mpe: Handle) -> Handle;
    pub fn cmsStageFree(mpe: Handle);
    pub fn cmsStageInputChannels(mpe: Handle) -> U32;
    pub fn cmsStageOutputChannels(mpe: Handle) -> U32;
    pub fn cmsStageType(mpe: Handle) -> U32;
    pub fn cmsStageNext(mpe: Handle) -> Handle;
    pub fn cmsStageSampleCLut16bit(
        mpe: Handle,
        sampler: unsafe extern "C" fn(*const U16, *mut U16, *mut c_void) -> i32,
        cargo: *mut c_void,
        dw_flags: U32,
    ) -> i32;
    pub fn cmsSliceSpace16(
        n_inputs: U32,
        clut_points: *const U32,
        sampler: unsafe extern "C" fn(*const U16, *mut U16, *mut c_void) -> i32,
        cargo: *mut c_void,
    ) -> i32;
    pub fn _cmsStageAllocLab2XYZ(context_id: Context) -> Handle;
    pub fn _cmsStageAllocXYZ2Lab(context_id: Context) -> Handle;
    pub fn _cmsStageAllocLabV2ToV4(context_id: Context) -> Handle;
    pub fn _cmsStageAllocLabV4ToV2(context_id: Context) -> Handle;
    pub fn _cmsStageAllocLabV2ToV4curves(context_id: Context) -> Handle;
    pub fn _cmsStageAllocLabPrelin(context_id: Context) -> Handle;
    pub fn _cmsStageAllocIdentityCLut(context_id: Context, n_chan: U32) -> Handle;
    pub fn _cmsStageClipNegatives(context_id: Context, n_channels: U32) -> Handle;
    pub fn _cmsStageNormalizeFromLabFloat(context_id: Context) -> Handle;
    pub fn _cmsStageNormalizeFromXyzFloat(context_id: Context) -> Handle;
    pub fn _cmsStageNormalizeToLabFloat(context_id: Context) -> Handle;
    pub fn _cmsStageNormalizeToXyzFloat(context_id: Context) -> Handle;
    pub fn cmsPipelineAlloc(context_id: Context, in_chans: U32, out_chans: U32) -> Handle;
    pub fn cmsPipelineFree(lut: Handle);
    pub fn cmsPipelineDup(lut: Handle) -> Handle;
    pub fn cmsPipelineEval16(in_: *const U16, out: *mut U16, lut: Handle);
    pub fn cmsPipelineEvalFloat(in_: *const F32, out: *mut F32, lut: Handle);
    pub fn cmsStageData(mpe: Handle) -> *mut c_void;
    pub fn cmsStageSampleCLutFloat(
        mpe: Handle,
        sampler: unsafe extern "C" fn(*const F32, *mut F32, *mut c_void) -> i32,
        cargo: *mut c_void,
        dw_flags: U32,
    ) -> i32;
    pub fn cmsPipelineEvalReverseFloat(
        target: *mut F32,
        result: *mut F32,
        hint: *mut F32,
        lut: Handle,
    ) -> i32;
    pub fn cmsPipelineInsertStage(lut: Handle, loc: i32, mpe: Handle) -> i32;
    pub fn cmsPipelineUnlinkStage(lut: Handle, loc: i32, mpe: *mut Handle);
    pub fn cmsPipelineCat(l1: Handle, l2: Handle) -> i32;
    pub fn cmsPipelineStageCount(lut: Handle) -> U32;
    pub fn cmsPipelineGetPtrToFirstStage(lut: Handle) -> Handle;
    pub fn cmsPipelineInputChannels(lut: Handle) -> U32;
    pub fn cmsPipelineOutputChannels(lut: Handle) -> U32;
    pub fn _cmsQuantizeVal(i: F64, max_samples: U32) -> U16;

    // ---- cmsmd5.c ----
    pub fn cmsMD5alloc(context_id: Context) -> Handle;
    pub fn cmsMD5add(handle: Handle, buf: *const U8, len: U32);
    pub fn cmsMD5finish(profile_id: *mut ProfileID, handle: Handle);

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
    // --- cmspack parity ---
    pub fn orc_call_fmt16(
        fmt: *mut c_void,
        info: *const c_void,
        w: *mut u16,
        buf: *mut u8,
        stride: u32,
    ) -> *mut u8;
    pub fn orc_call_fmtfl(
        fmt: *mut c_void,
        info: *const c_void,
        w: *mut f32,
        buf: *mut u8,
        stride: u32,
    ) -> *mut u8;
    pub fn orc_get_input_fmt16(ty: u32) -> *mut c_void;
    pub fn orc_get_input_fmtfl(ty: u32) -> *mut c_void;
    pub fn orc_get_output_fmt16(ty: u32) -> *mut c_void;
    pub fn orc_get_output_fmtfl(ty: u32) -> *mut c_void;
    // --- transform ---
    pub fn cmsCreateTransform(input: *mut c_void, input_format: u32, output: *mut c_void, output_format: u32, intent: u32, flags: u32) -> *mut c_void;
    pub fn cmsDoTransform(t: *mut c_void, input: *const c_void, output: *mut c_void, size: u32);
    pub fn cmsDeleteTransform(t: *mut c_void);
    pub fn cmsGetTransformInputColorants(t: *mut c_void) -> Handle;
    pub fn cmsGetTransformOutputColorants(t: *mut c_void) -> Handle;
    pub fn cmsGetNamedColorList(t: *mut c_void) -> Handle;
    // --- null-profile / header setters (named-color gate) ---
    pub fn cmsCreateNULLProfile() -> *mut c_void;
    pub fn cmsSetDeviceClass(p: *mut c_void, sig: u32);
    pub fn cmsSetColorSpace(p: *mut c_void, sig: u32);
    pub fn cmsSetPCS(p: *mut c_void, sig: u32);
    pub fn cmsSetProfileVersion(p: *mut c_void, version: f64);
    pub fn cmsSetHeaderRenderingIntent(p: *mut c_void, intent: u32);
    // --- PostScript CSA/CRD ---
    pub fn cmsGetPostScriptCSA(ctx: *mut c_void, p: *mut c_void, intent: u32, flags: u32, buf: *mut c_void, len: u32) -> u32;
    pub fn cmsGetPostScriptCRD(ctx: *mut c_void, p: *mut c_void, intent: u32, flags: u32, buf: *mut c_void, len: u32) -> u32;
    // --- cgats / IT8 ---
    pub fn cmsIT8Alloc(ctx: *mut c_void) -> *mut c_void;
    pub fn cmsIT8LoadFromMem(ctx: *mut c_void, ptr: *const c_void, len: u32) -> *mut c_void;
    pub fn cmsIT8SaveToMem(h: *mut c_void, mem: *mut c_void, bytes_needed: *mut u32) -> i32;
    pub fn cmsIT8GetSheetType(h: *mut c_void) -> *const i8;
    pub fn cmsIT8GetProperty(h: *mut c_void, key: *const i8) -> *const i8;
    pub fn cmsIT8GetData(h: *mut c_void, patch: *const i8, sample: *const i8) -> *const i8;
    pub fn cmsIT8Free(h: *mut c_void);
    pub fn cmsIT8SetSheetType(h: *mut c_void, ty: *const i8) -> i32;
    pub fn cmsIT8SetComment(h: *mut c_void, comment: *const i8) -> i32;
    pub fn cmsIT8SetPropertyStr(h: *mut c_void, prop: *const i8, val: *const i8) -> i32;
    pub fn cmsIT8SetPropertyDbl(h: *mut c_void, prop: *const i8, val: f64) -> i32;
    pub fn cmsIT8SetPropertyHex(h: *mut c_void, prop: *const i8, val: u32) -> i32;
    pub fn cmsIT8SetPropertyMulti(h: *mut c_void, key: *const i8, subkey: *const i8, val: *const i8) -> i32;
    pub fn cmsIT8SetPropertyUncooked(h: *mut c_void, key: *const i8, val: *const i8) -> i32;
    pub fn cmsIT8GetPropertyDbl(h: *mut c_void, prop: *const i8) -> f64;
    pub fn cmsIT8EnumProperties(h: *mut c_void, names: *mut *mut *mut i8) -> u32;
    pub fn cmsIT8EnumPropertyMulti(h: *mut c_void, prop: *const i8, names: *mut *mut *mut i8) -> u32;
    pub fn cmsIT8GetDataRowCol(h: *mut c_void, row: i32, col: i32) -> *const i8;
    pub fn cmsIT8GetDataRowColDbl(h: *mut c_void, row: i32, col: i32) -> f64;
    pub fn cmsIT8SetDataRowCol(h: *mut c_void, row: i32, col: i32, val: *const i8) -> i32;
    pub fn cmsIT8SetDataRowColDbl(h: *mut c_void, row: i32, col: i32, val: f64) -> i32;
    pub fn cmsIT8GetDataDbl(h: *mut c_void, patch: *const i8, sample: *const i8) -> f64;
    pub fn cmsIT8SetData(h: *mut c_void, patch: *const i8, sample: *const i8, val: *const i8) -> i32;
    pub fn cmsIT8SetDataDbl(h: *mut c_void, patch: *const i8, sample: *const i8, val: f64) -> i32;
    pub fn cmsIT8FindDataFormat(h: *mut c_void, sample: *const i8) -> i32;
    pub fn cmsIT8SetDataFormat(h: *mut c_void, n: i32, sample: *const i8) -> i32;
    pub fn cmsIT8EnumDataFormat(h: *mut c_void, names: *mut *mut *mut i8) -> i32;
    pub fn cmsIT8GetPatchName(h: *mut c_void, n_patch: i32, buffer: *mut i8) -> *const i8;
    pub fn cmsIT8GetPatchByName(h: *mut c_void, patch: *const i8) -> i32;
    pub fn cmsIT8SetTableByLabel(h: *mut c_void, set: *const i8, field: *const i8, expected: *const i8) -> i32;
    pub fn cmsIT8SetIndexColumn(h: *mut c_void, sample: *const i8) -> i32;
    pub fn cmsIT8DefineDblFormat(h: *mut c_void, formatter: *const i8);
    pub fn cmsIT8SetTable(h: *mut c_void, n_table: u32) -> i32;
    pub fn cmsIT8GetPropertyMulti(h: *mut c_void, key: *const i8, subkey: *const i8) -> *const i8;
    // --- context + error handling ---
    pub fn cmsCreateContext(plugins: *mut c_void, user_data: *mut c_void) -> *mut c_void;
    pub fn cmsDeleteContext(ctx: *mut c_void);
    pub fn cmsDupContext(ctx: *mut c_void, new_user_data: *mut c_void) -> *mut c_void;
    pub fn cmsGetContextUserData(ctx: *mut c_void) -> *mut c_void;
    pub fn cmsSetLogErrorHandler(fn_: *mut c_void);
    pub fn cmsSetLogErrorHandlerTHR(ctx: *mut c_void, fn_: *mut c_void);
    pub fn cmsLogError(ctx: *mut c_void, code: u32, text: *const i8);
    pub fn cmsGetLogErrorHandler(ctx: *mut c_void) -> *mut c_void;
    // --- gbd / gamut boundary descriptor ---
    pub fn cmsGBDAlloc(ctx: *mut c_void) -> *mut c_void;
    pub fn cmsGBDFree(gbd: *mut c_void);
    pub fn cmsGDBAddPoint(gbd: *mut c_void, lab: *const c_void) -> i32;
    pub fn cmsGDBCheckPoint(gbd: *mut c_void, lab: *const c_void) -> i32;
    pub fn cmsGDBCompute(gbd: *mut c_void, flags: u32) -> i32;
    // --- cube / devicelink ---
    pub fn cmsCreateDeviceLinkFromCubeFile(path: *const i8) -> *mut c_void;
    // --- black point / sampling ---
    pub fn cmsDetectBlackPoint(bp: *mut c_void, prof: *mut c_void, intent: u32, flags: u32) -> i32;
    pub fn cmsDetectDestinationBlackPoint(bp: *mut c_void, prof: *mut c_void, intent: u32, flags: u32) -> i32;
    pub fn cmsDetectTAC(prof: *mut c_void) -> f64;
    pub fn cmsDesaturateLab(lab: *mut c_void, amax: f64, amin: f64, bmax: f64, bmin: f64) -> i32;
    // --- profile info (cmsio1) ---
    pub fn cmsIsIntentSupported(prof: *mut c_void, intent: u32, direction: u32) -> i32;
    pub fn cmsGetSupportedIntents(n_max: u32, codes: *mut u32, descriptions: *mut *mut u8) -> u32;
    pub fn _cmsReadInputLUT(prof: *mut c_void, intent: u32) -> *mut c_void;
    pub fn _cmsReadOutputLUT(prof: *mut c_void, intent: u32) -> *mut c_void;
    pub fn _cmsReadDevicelinkLUT(prof: *mut c_void, intent: u32) -> *mut c_void;
    // --- virtual profile builders (cmsvirt) ---
    pub fn cmsCreateGrayProfile(white_point: *const c_void, transfer: *mut ToneCurve) -> *mut c_void;
    pub fn cmsCreateBCHSWabstractProfile(n_lut_points: u32, bright: f64, contrast: f64, hue: f64, saturation: f64, temp_src: u32, temp_dest: u32) -> *mut c_void;
    pub fn cmsCreateLinearizationDeviceLinkTHR(ctx: *mut c_void, color_space: u32, transfer: *const *mut ToneCurve) -> *mut c_void;
    pub fn cmsCreateInkLimitingDeviceLinkTHR(ctx: *mut c_void, color_space: u32, limit: f64) -> *mut c_void;
    pub fn cmsTransform2DeviceLink(t: *mut c_void, version: f64, flags: u32) -> *mut c_void;
    // --- transforms, extended surface ---
    pub fn cmsCreateMultiprofileTransform(profs: *mut *mut c_void, n_profiles: i32, in_fmt: u32, out_fmt: u32, intent: u32, flags: u32) -> *mut c_void;
    pub fn cmsCreateProofingTransform(input: *mut c_void, in_fmt: u32, output: *mut c_void, out_fmt: u32, proof: *mut c_void, intent: u32, proofing_intent: u32, flags: u32) -> *mut c_void;
    pub fn cmsDoTransformStride(t: *mut c_void, input: *const c_void, output: *mut c_void, size: u32, stride: u32);
    pub fn cmsDoTransformLineStride(t: *mut c_void, input: *const c_void, output: *mut c_void, ppl: u32, lines: u32, bpl_in: u32, bpl_out: u32, bpp_in: u32, bpp_out: u32);
    pub fn cmsChangeBuffersFormat(t: *mut c_void, in_fmt: u32, out_fmt: u32) -> i32;
    pub fn cmsGetTransformInputFormat(t: *mut c_void) -> u32;
    pub fn cmsGetTransformOutputFormat(t: *mut c_void) -> u32;
    pub fn cmsGetTransformGamutCheckPipeline(t: *mut c_void) -> *mut c_void;
    pub fn cmsSetAlarmCodesTHR(ctx: *mut c_void, codes: *const u16);
    pub fn cmsGetAlarmCodesTHR(ctx: *mut c_void, codes: *mut u16);
    pub fn cmsSetAdaptationStateTHR(ctx: *mut c_void, d: f64) -> f64;
    pub fn cmsGetPCS(prof: *mut c_void) -> u32;
    pub fn cmsGetColorSpace(prof: *mut c_void) -> u32;
    pub fn cmsGetDeviceClass(prof: *mut c_void) -> u32;
    pub fn cmsGetEncodedICCversion(prof: *mut c_void) -> u32;
    pub fn cmsGetHeaderFlags(p: *mut c_void, flags: *mut u32);
    pub fn cmsGetHeaderAttributes(p: *mut c_void, flags: *mut u64);
    pub fn cmsGetHeaderCreator(p: *mut c_void) -> u32;
    pub fn cmsGetHeaderModel(p: *mut c_void) -> u32;
    pub fn cmsGetHeaderProfileID(p: *mut c_void, id: *mut u8);
    pub fn cmsGetHeaderCreationDateTime(p: *mut c_void, dt: *mut c_void) -> i32;
    pub fn cmsIsCLUT(prof: *mut c_void, intent: u32, direction: u32) -> i32;
    pub fn cmsGetInfo(p: *mut c_void, info: u32) -> *const u16;
    pub fn cmsSetEncodedICCversion(prof: *mut c_void, version: u32);

    // transform-internals shims for the prelin8 divergence bisect
    pub fn cref_xform_lut(t: *mut c_void) -> *mut c_void;
    pub fn cref_pipeline_first_stage(lut: *mut c_void) -> *mut c_void;
    pub fn cref_stage_data(mpe: *mut c_void) -> *mut c_void;
    pub fn cref_clut_u16_table(clutdata: *mut c_void) -> *mut c_void;
    pub fn cref_clut_nodes(clutdata: *mut c_void) -> u32;
    pub fn cref_stage_next(mpe: *mut c_void) -> *mut c_void;
    pub fn cref_stage_type(mpe: *mut c_void) -> u32;
    pub fn cref_lut_private_data(lut: *mut c_void) -> *mut c_void;
    pub fn cref_p8_r(p8: *mut c_void, which: i32) -> *mut u16;
    pub fn cref_p8_x(p8: *mut c_void, which: i32) -> *mut u32;
}

/// cmsGetEncodedCMMversion sanity: returns LCMS_VERSION (2200).
pub fn encoded_version() -> U32 {
    unsafe { cmsGetEncodedCMMversion() }
}
unsafe extern "C" {
    pub fn cref_sigmoid_correction(k: f64) -> u64;
    pub fn cref_sigmoid_base_bits(k: f64, t: f64) -> u64;
    pub fn cref_inverse_sigmoid_x_bits(k: f64, t: f64) -> u64;
    pub fn cref_inverse_sigmoid_inner_bits(k: f64, t: f64) -> u64;
    pub fn cref_isb_bits(k: f64, t: f64) -> u64;
    pub fn cref_inverse_sigmoid_bits(k: f64, t: f64) -> u64;
}

unsafe extern "C" {
    pub fn cref_seg_param(curve: *mut ToneCurve, seg: i32, p: i32) -> f64;
    pub fn cref_seg_type(curve: *mut ToneCurve, seg: i32) -> i32;
    pub fn cref_seg_ngrid(curve: *mut ToneCurve, seg: i32) -> u32;
    pub fn cmsPlugin(plugin: *mut c_void) -> i32;
    pub fn cmsPluginTHR(ctx: *mut c_void, plugin: *mut c_void) -> i32;
}
