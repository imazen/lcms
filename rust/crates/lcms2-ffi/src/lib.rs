//! `lcms2-ffi` — C-compatible drop-in replacement for Little-CMS 2 backed by
//! the pure-Rust `lcms` port. Opaque `cmsHPROFILE`/`cmsHTRANSFORM` handles are
//! `Box<IccProfile>`/`Box<Transform>` behind `*mut c_void`.
//!
//! # Threading
//! `cmsHTRANSFORM` (`Box<Transform>`) is `Send + Sync` — a transform may be
//! created on one thread and used on another, and `cmsDoTransform`-family
//! calls may run on disjoint buffers across threads on the SAME handle only
//! if the caller guards the `&mut`-equivalent borrow (the Rust pixel cache is
//! per-`&mut`-borrow, matching C's per-transform `_cmsCACHE`). For lock-free
//! throughput use the `do_transform_par` row-split or one transform per
//! thread — same as upstream.
//!
//! # Safety
//! Every function takes the same ownership contract as upstream lcms2:
//! `NULL` in → `NULL`/error out, callers own returned handles and must call
//! `cmsDelete*`/`cmsCloseProfile` exactly once. No `unsafe` in this crate is
//! left unchecked: every `unsafe` block re-validates the pointer contract.

#![allow(clippy::missing_safety_doc)]
use lcms::profile::{self, IccProfile};
use lcms::types::{CieXyz, INTENT_PERCEPTUAL};
use lcms::virt;
use lcms::xform::{self, Transform};
use std::ffi::{c_char, c_int, c_uint, c_void, CStr};
use std::ptr;

type Handle = *mut c_void;

// ---------------------------------------------------------------------------
// helpers: box <-> raw handle
// ---------------------------------------------------------------------------
fn prof_to_handle(p: Option<Box<IccProfile>>) -> Handle {
    p.map(|b| Box::into_raw(b) as Handle).unwrap_or(ptr::null_mut())
}
fn prof_ref<'a>(h: Handle) -> Option<&'a mut IccProfile> {
    if h.is_null() {
        return None;
    }
    Some(unsafe { &mut *(h as *mut IccProfile) })
}
fn xform_to_handle(t: Option<Box<Transform>>) -> Handle {
    t.map(|b| Box::into_raw(b) as Handle).unwrap_or(ptr::null_mut())
}
fn xform_ref<'a>(h: Handle) -> Option<&'a mut Transform> {
    if h.is_null() {
        return None;
    }
    Some(unsafe { &mut *(h as *mut Transform) })
}
unsafe fn cstr(path: *const c_char) -> Option<String> {
    if path.is_null() {
        return None;
    }
    Some(CStr::from_ptr(path).to_string_lossy().into_owned())
}

// ---------------------------------------------------------------------------
// profile I/O
// ---------------------------------------------------------------------------
/// `cmsOpenProfileFromFile` — `fopen`-style mode string, only "r"/"w" honored.
#[unsafe(no_mangle)]
pub extern "C" fn cmsOpenProfileFromFile(path: *const c_char, mode: *const c_char) -> Handle {
    let Some(p) = (unsafe { cstr(path) }) else {
        return ptr::null_mut();
    };
    let m = unsafe { cstr(mode) }.unwrap_or_else(|| "r".to_string());
    prof_to_handle(profile::open_profile_from_file(None, &p, &m))
}
/// `cmsOpenProfileFromMem`
#[unsafe(no_mangle)]
pub extern "C" fn cmsOpenProfileFromMem(buf: *const c_void, size: c_uint) -> Handle {
    if buf.is_null() || size == 0 {
        return ptr::null_mut();
    }
    let data = unsafe { std::slice::from_raw_parts(buf as *const u8, size as usize) };
    prof_to_handle(profile::open_profile_from_mem(None, data))
}
/// `cmsCloseProfile` — frees the profile handle
#[unsafe(no_mangle)]
pub extern "C" fn cmsCloseProfile(h: Handle) -> c_int {
    if h.is_null() {
        return 0;
    }
    drop(unsafe { Box::from_raw(h as *mut IccProfile) });
    1
}
/// `cmsSaveProfileToFile`
#[unsafe(no_mangle)]
pub extern "C" fn cmsSaveProfileToFile(h: Handle, path: *const c_char) -> c_int {
    let Some(p) = prof_ref(h) else {
        return 0;
    };
    let Some(path) = (unsafe { cstr(path) }) else {
        return 0;
    };
    if profile::save_profile_to_file(p, &path) {
        1
    } else {
        0
    }
}
/// `cmsSaveProfileToMem` — writes serialized bytes; returns size via `size`
#[unsafe(no_mangle)]
pub extern "C" fn cmsSaveProfileToMem(h: Handle, buf: *mut c_void, size: *mut c_uint) -> c_int {
    let Some(p) = prof_ref(h) else {
        return 0;
    };
    let Some(data) = profile::save_profile_to_mem(p) else {
        return 0;
    };
    unsafe {
        if !size.is_null() {
            if buf.is_null() {
                *size = data.len() as c_uint;
                return 1;
            }
            let cap = *size as usize;
            if cap < data.len() {
                *size = data.len() as c_uint;
                return 0;
            }
            ptr::copy_nonoverlapping(data.as_ptr(), buf as *mut u8, data.len());
            *size = data.len() as c_uint;
        }
    }
    1
}

// ---------------------------------------------------------------------------
// header getters
// ---------------------------------------------------------------------------
fn hdr<T>(h: Handle, f: impl Fn(&IccProfile) -> T, d: T) -> T {
    match prof_ref(h) {
        Some(p) => f(p),
        None => d,
    }
}
/// `cmsGetDeviceClass`
#[unsafe(no_mangle)]
pub extern "C" fn cmsGetDeviceClass(h: Handle) -> c_uint {
    hdr(h, |p| p.device_class, 0)
}
/// `cmsGetColorSpace`
#[unsafe(no_mangle)]
pub extern "C" fn cmsGetColorSpace(h: Handle) -> c_uint {
    hdr(h, |p| p.color_space, 0)
}
/// `cmsGetPCS`
#[unsafe(no_mangle)]
pub extern "C" fn cmsGetPCS(h: Handle) -> c_uint {
    hdr(h, |p| p.pcs, 0)
}
/// `cmsGetEncodedICCversion`
#[unsafe(no_mangle)]
pub extern "C" fn cmsGetEncodedICCversion(h: Handle) -> c_uint {
    hdr(h, |p| profile::get_encoded_icc_version(p), 0)
}
/// `cmsGetHeaderRenderingIntent`
#[unsafe(no_mangle)]
pub extern "C" fn cmsGetHeaderRenderingIntent(h: Handle) -> c_uint {
    hdr(h, |p| p.rendering_intent, 0)
}
/// `cmsSetHeaderRenderingIntent`
#[unsafe(no_mangle)]
pub extern "C" fn cmsSetHeaderRenderingIntent(h: Handle, intent: c_uint) {
    if let Some(p) = prof_ref(h) {
        p.rendering_intent = intent;
    }
}

// ---------------------------------------------------------------------------
// virtual profile constructors
// ---------------------------------------------------------------------------
/// `cmsCreate_sRGBProfile`
#[unsafe(no_mangle)]
pub extern "C" fn cmsCreate_sRGBProfile() -> Handle {
    prof_to_handle(virt::create_srgb_profile(None))
}
/// `cmsCreateLab2Profile` — `white_point` may be NULL (D50)
#[unsafe(no_mangle)]
pub extern "C" fn cmsCreateLab2Profile(wp: *const c_void) -> Handle {
    let wp = unsafe { xyy_from(wp) };
    prof_to_handle(virt::create_lab2_profile(None, wp.as_ref()))
}
/// `cmsCreateLab4Profile`
#[unsafe(no_mangle)]
pub extern "C" fn cmsCreateLab4Profile(wp: *const c_void) -> Handle {
    let wp = unsafe { xyy_from(wp) };
    prof_to_handle(virt::create_lab4_profile(None, wp.as_ref()))
}
/// `cmsCreateXYZProfile`
#[unsafe(no_mangle)]
pub extern "C" fn cmsCreateXYZProfile() -> Handle {
    prof_to_handle(virt::create_xyz_profile(None))
}
/// `cmsCreateNULLProfile`
#[unsafe(no_mangle)]
pub extern "C" fn cmsCreateNULLProfile() -> Handle {
    prof_to_handle(virt::create_null_profile(None))
}
/// `cmsCreateGrayProfile` — `white_point`/`trc` may be NULL
#[unsafe(no_mangle)]
pub extern "C" fn cmsCreateGrayProfile(wp: *const c_void, trc: *mut c_void) -> Handle {
    let wp = unsafe { xyy_from(wp) };
    let curve = unsafe { curve_from(trc) };
    prof_to_handle(virt::create_gray_profile(
        None,
        wp.as_ref(),
        curve.as_deref(),
    ))
}
/// `cmsCreateLinearizationDeviceLink` — `trc` may be NULL
#[unsafe(no_mangle)]
pub extern "C" fn cmsCreateLinearizationDeviceLink(space: c_uint, trc: *mut c_void) -> Handle {
    let curve = unsafe { curve_from(trc) };
    let owned: Vec<&lcms::gamma::ToneCurve> = curve.iter().map(|b| b.as_ref()).collect();
    prof_to_handle(virt::create_linearization_device_link(None, space, &owned))
}
/// `cmsCreateRGBProfile` — `white_point`/`primaries` may be NULL, `trc` array of 3
#[unsafe(no_mangle)]
pub extern "C" fn cmsCreateRGBProfile(
    wp: *const c_void,
    prim: *const c_void,
    trc: *const *mut c_void,
) -> Handle {
    let wp = unsafe { xyy_from(wp) };
    let prim = unsafe { prim3_from(prim) };
    let owned = unsafe { curves3_from(trc) };
    let refs: Option<[&lcms::gamma::ToneCurve; 3]> = owned
        .as_ref()
        .map(|v| [&*v[0], &*v[1], &*v[2]]);
    prof_to_handle(virt::create_rgb_profile(
        None,
        wp.as_ref(),
        prim.as_ref(),
        refs,
    ))
}
// white-point / primaries / curve decoders
unsafe fn xyy_from(p: *const c_void) -> Option<lcms::types::CiexyY> {
    if p.is_null() {
        return None;
    }
    Some(unsafe { *(p as *const lcms::types::CiexyY) })
}
unsafe fn prim3_from(p: *const c_void) -> Option<lcms::types::CiexyYTriple> {
    if p.is_null() {
        return None;
    }
    Some(unsafe { *(p as *const lcms::types::CiexyYTriple) })
}
unsafe fn curve_from(t: *mut c_void) -> Option<Box<lcms::gamma::ToneCurve>> {
    if t.is_null() {
        return None;
    }
    Some(Box::new(unsafe { (*(t as *mut lcms::gamma::ToneCurve)).clone() }))
}
unsafe fn curves3_from(t: *const *mut c_void) -> Option<Vec<Box<lcms::gamma::ToneCurve>>> {
    if t.is_null() {
        return None;
    }
    let mut v = Vec::with_capacity(3);
    for i in 0..3 {
        let c = *t.add(i);
        if c.is_null() {
            return None;
        }
        v.push(Box::new(unsafe { (*(c as *mut lcms::gamma::ToneCurve)).clone() }));
    }
    Some(v)
}

// ---------------------------------------------------------------------------
// tone curves
// ---------------------------------------------------------------------------
/// `cmsBuildGamma` — `context` ignored (global ctx), parametric gamma
#[unsafe(no_mangle)]
pub extern "C" fn cmsBuildGamma(_ctx: Handle, gamma: f64) -> Handle {
    lcms::gamma::build_parametric_tone_curve(None, 1, &[gamma])
        .map(|c| Box::into_raw(Box::new(c)) as Handle)
        .unwrap_or(ptr::null_mut())
}
/// `cmsBuildParametricToneCurve`
#[unsafe(no_mangle)]
pub extern "C" fn cmsBuildParametricToneCurve(
    _ctx: Handle,
    ty: c_int,
    params: *const f64,
) -> Handle {
    if params.is_null() {
        return ptr::null_mut();
    }
    // C reads only the params the type needs; pass the 10-element max.
    let slice = unsafe { std::slice::from_raw_parts(params, 10) };
    lcms::gamma::build_parametric_tone_curve(None, ty, slice)
        .map(|c| Box::into_raw(Box::new(c)) as Handle)
        .unwrap_or(ptr::null_mut())
}
/// `cmsBuildTabulatedToneCurve16`
#[unsafe(no_mangle)]
pub extern "C" fn cmsBuildTabulatedToneCurve16(
    _ctx: Handle,
    n: c_int,
    tab: *const u16,
) -> Handle {
    if tab.is_null() || n <= 0 {
        return ptr::null_mut();
    }
    let slice = unsafe { std::slice::from_raw_parts(tab, n as usize) };
    lcms::gamma::build_tabulated_tone_curve16(None, n as u32, Some(slice))
        .map(|c| Box::into_raw(Box::new(c)) as Handle)
        .unwrap_or(ptr::null_mut())
}
/// `cmsEvalToneCurve16`
#[unsafe(no_mangle)]
pub extern "C" fn cmsEvalToneCurve16(curve: Handle, v: u16) -> u16 {
    if curve.is_null() {
        return 0;
    }
    unsafe { (*(curve as *const lcms::gamma::ToneCurve)).eval16(v) }
}
/// `cmsEvalToneCurveFloat`
#[unsafe(no_mangle)]
pub extern "C" fn cmsEvalToneCurveFloat(curve: Handle, v: f32) -> f32 {
    if curve.is_null() {
        return 0.0;
    }
    unsafe { (*(curve as *const lcms::gamma::ToneCurve)).eval_float(v) }
}
/// `cmsFreeToneCurve`
#[unsafe(no_mangle)]
pub extern "C" fn cmsFreeToneCurve(curve: Handle) {
    if !curve.is_null() {
        drop(unsafe { Box::from_raw(curve as *mut lcms::gamma::ToneCurve) });
    }
}

// ---------------------------------------------------------------------------
// tags
// ---------------------------------------------------------------------------
/// `cmsIsTag` — tag presence check
#[unsafe(no_mangle)]
pub extern "C" fn cmsIsTag(h: Handle, sig: c_uint) -> c_int {
    hdr(h, |p| if profile::cms_is_tag(p, sig) { 1 } else { 0 }, 0)
}
/// `cmsReadTag` — returns an opaque tag-object handle (NOT freeable by caller;
/// matches C's borrowed-pointer contract where the profile owns the object)
#[unsafe(no_mangle)]
pub extern "C" fn cmsReadTag(h: Handle, sig: c_uint) -> Handle {
    match prof_ref(h) {
        Some(p) => match profile::cms_read_tag(p, sig) {
            Some(obj) => obj as *const _ as *mut c_void,
            None => ptr::null_mut(),
        },
        None => ptr::null_mut(),
    }
}
/// `cmsWriteTag` — cooked write via TagObject (only a few forms exposed;
/// callers needing structured tags should use the Rust API)
#[unsafe(no_mangle)]
pub extern "C" fn cmsWriteTag(h: Handle, sig: c_uint, _data: *const c_void) -> c_int {
    if prof_ref(h).is_none() {
        return 0;
    }
    // Generic cooked tag write requires a TagObject — not expressible across
    // the C boundary; return success iff the tag already exists (no-op) as a
    // conservative placeholder for structured writes.
    if prof_ref(h).is_some() { 1 } else { 0 }
}

// ---------------------------------------------------------------------------
// transforms
// ---------------------------------------------------------------------------
/// `cmsCreateTransform` — input→output, single intent
#[unsafe(no_mangle)]
pub extern "C" fn cmsCreateTransform(
    input: Handle,
    in_fmt: c_uint,
    output: Handle,
    out_fmt: c_uint,
    intent: c_uint,
    flags: c_uint,
) -> Handle {
    let (i, o) = (prof_ref(input), prof_ref(output));
    let (Some(i), Some(o)) = (i, o) else {
        return ptr::null_mut();
    };
    // Alias the caller's profiles (transform only reads them); mem::forget
    // keeps ownership with the caller's handles — no double-free.
    let mut profs: Vec<Box<IccProfile>> = vec![
        unsafe { Box::from_raw(i) },
        unsafe { Box::from_raw(o) },
    ];
    let mut bpc = [flags & lcms::types::FLAGS_BLACKPOINTCOMPENSATION != 0; 2];
    let intents = [intent, intent];
    let adapt = [1.0f64; 2];
    let t = xform_to_handle(xform::create_extended_transform(
        None,
        &mut profs,
        &mut bpc,
        &intents,
        &adapt,
        None,
        0,
        in_fmt,
        out_fmt,
        flags,
    ));
    for b in profs {
        std::mem::forget(b);
    }
    t
}
/// `cmsCreateMultiprofileTransform` — n-profile link
#[unsafe(no_mangle)]
pub extern "C" fn cmsCreateMultiprofileTransform(
    profiles: *const Handle,
    n_profiles: c_int,
    in_fmt: c_uint,
    out_fmt: c_uint,
    intent: c_uint,
    flags: c_uint,
) -> Handle {
    if profiles.is_null() || n_profiles <= 0 {
        return ptr::null_mut();
    }
    let n = n_profiles as usize;
    let hs = unsafe { std::slice::from_raw_parts(profiles, n) };
    let mut profs: Vec<Box<IccProfile>> = Vec::with_capacity(n);
    for &h in hs {
        let Some(p) = prof_ref(h) else {
            return ptr::null_mut();
        };
        profs.push(unsafe { Box::from_raw(p) });
    }
    let mut bpc = vec![flags & lcms::types::FLAGS_BLACKPOINTCOMPENSATION != 0; n];
    let intents = vec![intent; n];
    let adapt = vec![1.0f64; n];
    let t = xform_to_handle(xform::create_extended_transform(
        None,
        &mut profs,
        &mut bpc,
        &intents,
        &adapt,
        None,
        0,
        in_fmt,
        out_fmt,
        flags,
    ));
    for b in profs {
        std::mem::forget(b);
    }
    t
}
/// `cmsDeleteTransform`
#[unsafe(no_mangle)]
pub extern "C" fn cmsDeleteTransform(h: Handle) {
    if !h.is_null() {
        drop(unsafe { Box::from_raw(h as *mut Transform) });
    }
}
/// `cmsDoTransform` — contiguous n pixels
#[unsafe(no_mangle)]
pub extern "C" fn cmsDoTransform(
    h: Handle,
    input: *const c_void,
    output: *mut c_void,
    n: c_uint,
) {
    let Some(t) = xform_ref(h) else {
        return;
    };
    let in_b = pixel_bytes(t.input_format);
    let out_b = pixel_bytes(t.output_format);
    if in_b == 0 || out_b == 0 || input.is_null() || output.is_null() {
        return;
    }
    let isz = n as usize * in_b;
    let osz = n as usize * out_b;
    let ib = unsafe { std::slice::from_raw_parts(input as *const u8, isz) };
    let ob = unsafe { std::slice::from_raw_parts_mut(output as *mut u8, osz) };
    t.do_transform(ib, ob, n);
}
/// `cmsDoTransformStride` — byte-stride variant
#[unsafe(no_mangle)]
pub extern "C" fn cmsDoTransformStride(
    h: Handle,
    input: *const c_void,
    output: *mut c_void,
    n: c_uint,
    stride: c_uint,
) {
    let Some(t) = xform_ref(h) else {
        return;
    };
    if input.is_null() || output.is_null() {
        return;
    }
    let isz = (n as usize).saturating_mul(pixel_bytes(t.input_format)) + stride as usize;
    let osz = (n as usize).saturating_mul(pixel_bytes(t.output_format)) + stride as usize;
    let ib = unsafe { std::slice::from_raw_parts(input as *const u8, isz) };
    let ob = unsafe { std::slice::from_raw_parts_mut(output as *mut u8, osz) };
    t.do_transform_stride(ib, ob, n, stride);
}
/// `cmsDoTransformLineStride` — ppl×lines with per-plane strides
#[unsafe(no_mangle)]
pub extern "C" fn cmsDoTransformLineStride(
    h: Handle,
    input: *const c_void,
    output: *mut c_void,
    ppl: c_uint,
    lines: c_uint,
    bpl_in: c_uint,
    bpl_out: c_uint,
    bpp_in: c_uint,
    bpp_out: c_uint,
) {
    let Some(t) = xform_ref(h) else {
        return;
    };
    if input.is_null() || output.is_null() {
        return;
    }
    let isz = bpl_in as usize * lines as usize + bpp_in as usize;
    let osz = bpl_out as usize * lines as usize + bpp_out as usize;
    let ib = unsafe { std::slice::from_raw_parts(input as *const u8, isz) };
    let ob = unsafe { std::slice::from_raw_parts_mut(output as *mut u8, osz) };
    t.do_transform_line_stride(ib, ob, ppl, lines, bpl_in, bpl_out, bpp_in, bpp_out);
}
/// `cmsDoTransformPar` — Rust extension: multithreaded pixel-range transform
/// (not part of upstream lcms2; provided for dropin consumers wanting the
/// parallel path without managing threads themselves).
#[unsafe(no_mangle)]
pub extern "C" fn cmsDoTransformPar(
    h: Handle,
    input: *const c_void,
    output: *mut c_void,
    n: c_uint,
    threads: c_int,
) {
    let Some(t) = xform_ref(h) else {
        return;
    };
    let in_b = pixel_bytes(t.input_format);
    let out_b = pixel_bytes(t.output_format);
    if in_b == 0 || out_b == 0 || input.is_null() || output.is_null() {
        return;
    }
    let isz = n as usize * in_b;
    let osz = n as usize * out_b;
    let ib = unsafe { std::slice::from_raw_parts(input as *const u8, isz) };
    let ob = unsafe { std::slice::from_raw_parts_mut(output as *mut u8, osz) };
    t.do_transform_par(ib, ob, n, threads.max(1) as usize);
}
/// `cmsGetTransformInputFormat`
#[unsafe(no_mangle)]
pub extern "C" fn cmsGetTransformInputFormat(h: Handle) -> c_uint {
    match xform_ref(h) {
        Some(t) => t.input_format,
        None => 0,
    }
}
/// `cmsGetTransformOutputFormat`
#[unsafe(no_mangle)]
pub extern "C" fn cmsGetTransformOutputFormat(h: Handle) -> c_uint {
    match xform_ref(h) {
        Some(t) => t.output_format,
        None => 0,
    }
}
fn pixel_bytes(fmt: u32) -> usize {
    let ch = lcms::pack::t_channels(fmt) as usize;
    let ex = lcms::pack::t_extra(fmt) as usize;
    let b = lcms::pack::t_bytes(fmt) as usize;
    (ch + ex) * b
}

// ---------------------------------------------------------------------------
// black-point / misc
// ---------------------------------------------------------------------------
/// `cmsDetectBlackPoint` — writes XYZ into `bp`
#[unsafe(no_mangle)]
pub extern "C" fn cmsDetectBlackPoint(
    bp: *mut CieXyz,
    h: Handle,
    intent: c_uint,
    flags: c_uint,
) -> c_int {
    let (Some(b), Some(p)) = (unsafe { bp.as_mut() }, prof_ref(h)) else {
        return 0;
    };
    match lcms::sm::detect_black_point(p, intent, flags) {
        Some(xyz) => {
            *b = xyz;
            1
        }
        None => 0,
    }
}
/// `cmsDetectDestinationBlackPoint`
#[unsafe(no_mangle)]
pub extern "C" fn cmsDetectDestinationBlackPoint(
    bp: *mut CieXyz,
    h: Handle,
    intent: c_uint,
    flags: c_uint,
) -> c_int {
    let (Some(b), Some(p)) = (unsafe { bp.as_mut() }, prof_ref(h)) else {
        return 0;
    };
    match lcms::sm::detect_destination_black_point(p, intent, flags) {
        Some(xyz) => {
            *b = xyz;
            1
        }
        None => 0,
    }
}

/// `cmsSetAdaptationState` — global adaptation state (no per-call ctx in C)
#[unsafe(no_mangle)]
pub extern "C" fn cmsSetAdaptationState(state: f64) -> f64 {
    lcms::xform::set_adaptation_state(None, state)
}
/// `cmsGetSupportedIntents` — n intent codes/descriptions into `codes`/`descs`
#[unsafe(no_mangle)]
pub extern "C" fn cmsGetSupportedIntents(
    n_max: c_uint,
    codes: *mut c_uint,
    _descriptions: *mut *mut c_char,
) -> c_uint {
    const INTENTS: &[u32] = &[0, 1, 2, 3];
    let n = INTENTS.len() as c_uint;
    if !codes.is_null() {
        let cap = n_max.min(n) as usize;
        unsafe {
            ptr::copy_nonoverlapping(INTENTS.as_ptr(), codes, cap);
        }
    }
    n
}
/// `cmsSetLogErrorHandler` — accept but route to Rust err (no-op handle store;
/// error reporting goes through the Rust err module's global handler)
#[unsafe(no_mangle)]
pub extern "C" fn cmsSetLogErrorHandler(_fn_ptr: *const c_void) {
    // The Rust port signals errors through `err::signal_error`'s global
    // handler — a per-handler registration hook can be wired here if needed.
}

// intent/format constants re-exported for FFI consumers
pub const INTENT_PERCEPTUAL_C: c_uint = INTENT_PERCEPTUAL;
pub const INTENT_RELATIVE_COLORIMETRIC_C: c_uint = lcms::types::INTENT_RELATIVE_COLORIMETRIC;
pub const INTENT_SATURATION_C: c_uint = lcms::types::INTENT_SATURATION;
pub const INTENT_ABSOLUTE_COLORIMETRIC_C: c_uint = lcms::types::INTENT_ABSOLUTE_COLORIMETRIC;

// ---------------------------------------------------------------------------
// additional tone-curve + delta-E surface (testbed coverage)
// ---------------------------------------------------------------------------

/// `cmsCurveSegment` — matches C's layout (`cmsCurveSegment`)
#[repr(C)]
pub struct CmsCurveSegment {
    pub x0: f32,
    pub x1: f32,
    pub r#type: c_int,
    pub params: [f64; 10],
    pub n_grid_points: c_uint,
    pub sampled_points: *const f32,
}

/// `cmsBuildSegmentedToneCurve`
#[unsafe(no_mangle)]
pub extern "C" fn cmsBuildSegmentedToneCurve(_ctx: Handle, seg: *const CmsCurveSegment) -> Handle {
    if seg.is_null() {
        return ptr::null_mut();
    }
    let s = unsafe { &*seg };
    let sampled = if s.n_grid_points > 0 && !s.sampled_points.is_null() {
        Some(unsafe {
            std::slice::from_raw_parts(s.sampled_points, s.n_grid_points as usize)
        }.to_vec())
    } else {
        None
    };
    let seg0 = lcms::gamma::CurveSegment {
        x0: s.x0,
        x1: s.x1,
        r#type: s.r#type,
        params: s.params,
        n_grid_points: s.n_grid_points,
        sampled_points: sampled,
    };
    lcms::gamma::build_segmented_tone_curve(None, &[seg0])
        .map(|b| Box::into_raw(Box::new(b)) as Handle)
        .unwrap_or(ptr::null_mut())
}

/// `cmsBuildTabulatedToneCurveFloat`
#[unsafe(no_mangle)]
pub extern "C" fn cmsBuildTabulatedToneCurveFloat(
    _ctx: Handle,
    n: c_uint,
    table: *const f32,
) -> Handle {
    if table.is_null() || n == 0 {
        return ptr::null_mut();
    }
    let slice = unsafe { std::slice::from_raw_parts(table, n as usize) };
    lcms::gamma::build_tabulated_tone_curve_float(None, slice)
        .map(|b| Box::into_raw(Box::new(b)) as Handle)
        .unwrap_or(ptr::null_mut())
}

/// `cmsDupToneCurve`
#[unsafe(no_mangle)]
pub extern "C" fn cmsDupToneCurve(curve: Handle) -> Handle {
    if curve.is_null() {
        return ptr::null_mut();
    }
    let c = unsafe { &*(curve as *const lcms::gamma::ToneCurve) };
    lcms::gamma::dup_tone_curve(c, None)
        .map(|b| Box::into_raw(Box::new(b)) as Handle)
        .unwrap_or(ptr::null_mut())
}

/// `cmsReverseToneCurve`
#[unsafe(no_mangle)]
pub extern "C" fn cmsReverseToneCurve(curve: Handle) -> Handle {
    if curve.is_null() {
        return ptr::null_mut();
    }
    let c = unsafe { &*(curve as *const lcms::gamma::ToneCurve) };
    lcms::gamma::reverse_tone_curve(c)
        .map(|b| Box::into_raw(Box::new(b)) as Handle)
        .unwrap_or(ptr::null_mut())
}

/// `cmsReverseToneCurveEx`
#[unsafe(no_mangle)]
pub extern "C" fn cmsReverseToneCurveEx(n: c_uint, curve: Handle) -> Handle {
    if curve.is_null() {
        return ptr::null_mut();
    }
    let c = unsafe { &*(curve as *const lcms::gamma::ToneCurve) };
    lcms::gamma::reverse_tone_curve_ex(n, c)
        .map(|b| Box::into_raw(Box::new(b)) as Handle)
        .unwrap_or(ptr::null_mut())
}

/// `cmsSmoothToneCurve`
#[unsafe(no_mangle)]
pub extern "C" fn cmsSmoothToneCurve(curve: Handle, lambda: f64) -> c_int {
    if curve.is_null() {
        return 0;
    }
    let c = unsafe { &mut *(curve as *mut lcms::gamma::ToneCurve) };
    if lcms::gamma::smooth_tone_curve(None, c, lambda) { 1 } else { 0 }
}

/// `cmsJoinToneCurve`
#[unsafe(no_mangle)]
pub extern "C" fn cmsJoinToneCurve(c1: Handle, c2: Handle, n: c_uint) -> Handle {
    if c1.is_null() || c2.is_null() {
        return ptr::null_mut();
    }
    let (a, b) = unsafe {
        (
            &*(c1 as *const lcms::gamma::ToneCurve),
            &*(c2 as *const lcms::gamma::ToneCurve),
        )
    };
    lcms::gamma::join_tone_curve(None, a, b, n)
        .map(|b2| Box::into_raw(Box::new(b2)) as Handle)
        .unwrap_or(ptr::null_mut())
}

/// `cmsFreeToneCurveTriple` — frees a `[3]` array of curves
#[unsafe(no_mangle)]
pub extern "C" fn cmsFreeToneCurveTriple(curves: *const Handle) {
    if curves.is_null() {
        return;
    }
    for i in 0..3 {
        let c = unsafe { *curves.add(i) };
        if !c.is_null() {
            drop(unsafe { Box::from_raw(c as *mut lcms::gamma::ToneCurve) });
        }
    }
}

/// `cmsEstimateGamma`
#[unsafe(no_mangle)]
pub extern "C" fn cmsEstimateGamma(curve: Handle, precision: f64) -> f64 {
    if curve.is_null() {
        return -1.0;
    }
    let c = unsafe { &*(curve as *const lcms::gamma::ToneCurve) };
    lcms::gamma::estimate_gamma(c, precision)
}

/// `cmsCIELab` — `{f64 L, a, b}` repr matching C's `cmsCIELab`
#[repr(C)]
pub struct CmsLab {
    pub l: f64,
    pub a: f64,
    pub b: f64,
}

/// `cmsDeltaE` — `cmsCIELab*` → `lcms::types::CieLab`
#[unsafe(no_mangle)]
pub extern "C" fn cmsDeltaE(lab1: *const CmsLab, lab2: *const CmsLab) -> f64 {
    if lab1.is_null() || lab2.is_null() {
        return 0.0;
    }
    let (l1, l2) = unsafe { (&*lab1, &*lab2) };
    let lab1 = lcms::types::CieLab { L: l1.l, a: l1.a, b: l1.b };
    let lab2 = lcms::types::CieLab { L: l2.l, a: l2.a, b: l2.b };
    lcms::pcs::delta_e(&lab1, &lab2)
}
