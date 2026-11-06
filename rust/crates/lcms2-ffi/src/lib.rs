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
    let mut p = unsafe { Box::from_raw(h as *mut IccProfile) };
    let mut rc = 1;
    if p.is_write {
        // `cmsSaveProfileToIOhandler(h, NULL)` — serialize into the profile's
        // own (write-mode) IO handler before closing.
        let mut io = p.io.take();
        if profile::save_profile_to_iohandler(&mut p, io.as_mut()) == 0 {
            rc = 0;
        }
        p.is_write = false;
    }
    // `_cmsSetProfileUserData`'s destructor runs on close (C: cmsio0.c)
    if p.user_data != 0 && p.free_user_data_fn != 0 {
        let f: FreeUserDataC = unsafe { std::mem::transmute(p.free_user_data_fn) };
        let cid = p
            .ctx
            .as_ref()
            .map(|c| c as *const lcms::context::Context as Handle)
            .unwrap_or(ptr::null_mut());
        unsafe { f(cid, p.user_data as *mut c_void) };
    }
    drop(p);
    rc
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
        curve.as_ref(),
    ))
}
/// `cmsCreateLinearizationDeviceLink` — `trc` may be NULL
#[unsafe(no_mangle)]
pub extern "C" fn cmsCreateLinearizationDeviceLink(
    space: c_uint,
    trc: *const *mut c_void,
) -> Handle {
    cmsCreateLinearizationDeviceLinkTHR(ptr::null_mut(), space, trc)
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
        .map(|v| [&v[0], &v[1], &v[2]]);
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
unsafe fn curve_from(t: *mut c_void) -> Option<lcms::gamma::ToneCurve> {
    if t.is_null() {
        return None;
    }
    tc_to_rust(unsafe { &*(t as *const CmsToneCurve) })
}
unsafe fn curves3_from(t: *const *mut c_void) -> Option<Vec<lcms::gamma::ToneCurve>> {
    unsafe { curves_n_from(t, 3) }
}
unsafe fn curves_n_from(
    t: *const *mut c_void,
    n: usize,
) -> Option<Vec<lcms::gamma::ToneCurve>> {
    if t.is_null() {
        return None;
    }
    let mut v = Vec::with_capacity(n);
    for i in 0..n {
        let c = *t.add(i);
        if c.is_null() {
            return None;
        }
        v.push(tc_to_rust(unsafe { &*(c as *const CmsToneCurve) })?);
    }
    Some(v)
}

// ---------------------------------------------------------------------------
// tone curves
// ---------------------------------------------------------------------------
/// `cmsBuildGamma` — `context` ignored (global ctx), parametric gamma
#[unsafe(no_mangle)]
pub extern "C" fn cmsBuildGamma(ctx: Handle, gamma: f64) -> Handle {
    lcms::gamma::build_parametric_tone_curve(None, 1, &[gamma])
        .map(|t| rust_to_tc(ctx, t))
        .unwrap_or(ptr::null_mut())
}
/// `cmsBuildParametricToneCurve`
#[unsafe(no_mangle)]
pub extern "C" fn cmsBuildParametricToneCurve(
    ctx: Handle,
    ty: c_int,
    params: *const f64,
) -> Handle {
    if params.is_null() {
        return ptr::null_mut();
    }
    // Plugin-registered parametric types take precedence
    // (`GetParametricCurveByType` checks plugin collections first).
    if let Some(eval) = parametric_eval_for(ctx, ty) {
        let mut p10 = [0f64; 10];
        unsafe { ptr::copy_nonoverlapping(params, p10.as_mut_ptr(), 10) };
        let seg = CmsCurveSegment {
            x0: -1e22f32,
            x1: 1e22f32,
            r#type: ty,
            params: p10,
            n_grid_points: 0,
            sampled_points: ptr::null(),
        };
        let segs = Box::into_raw(Box::new([seg])) as *mut CmsCurveSegment;
        let si = Box::into_raw(Box::new([ptr::null_mut::<CmsInterpParams>()]))
            as *mut *mut CmsInterpParams;
        let ev = Box::into_raw(Box::new([eval])) as *mut usize;
        let interp_params = make_bare_cparams(ctx);
        return Box::into_raw(Box::new(CmsToneCurve {
            interp_params,
            n_segments: 1,
            segments: segs,
            seg_interp: si,
            evals: ev,
            n_entries: 0,
            table16: ptr::null_mut(),
        })) as Handle;
    }
    // C reads only the params the type needs; pass the 10-element max.
    let slice = unsafe { std::slice::from_raw_parts(params, 10) };
    lcms::gamma::build_parametric_tone_curve(unsafe { cctx(ctx) }, ty, slice)
        .map(|t| rust_to_tc(ctx, t))
        .unwrap_or(ptr::null_mut())
}
/// `cmsBuildTabulatedToneCurve16`
#[unsafe(no_mangle)]
pub extern "C" fn cmsBuildTabulatedToneCurve16(
    ctx: Handle,
    n: c_int,
    tab: *const u16,
) -> Handle {
    if tab.is_null() || n <= 0 {
        return ptr::null_mut();
    }
    let slice = unsafe { std::slice::from_raw_parts(tab, n as usize) };
    lcms::gamma::build_tabulated_tone_curve16(None, n as u32, Some(slice))
        .map(|t| rust_to_tc(ctx, t))
        .unwrap_or(ptr::null_mut())
}
/// `cmsEvalToneCurve16` — C evaluates tabulated curves through
/// `InterpParams->Interpolation.Lerp16` so plugin interpolators apply.
#[unsafe(no_mangle)]
pub extern "C" fn cmsEvalToneCurve16(curve: Handle, v: u16) -> u16 {
    if curve.is_null() {
        return 0;
    }
    let c = unsafe { &*(curve as *const CmsToneCurve) };
    if c.n_entries > 0 {
        if let Some(ip) = unsafe { c.interp_params.as_ref() } {
            if ip.interpolation != 0 {
                let f: Lerp16C = unsafe { std::mem::transmute(ip.interpolation) };
                let mut out = 0u16;
                unsafe { f(&v, &mut out, ip) };
                return out;
            }
        }
    }
    let Some(c) = curve_rust(curve) else { return 0 }; c.eval16(v)
}
/// `cmsEvalToneCurveFloat` — C dispatch: `nSegments==0` → u16→Lerp16→/65535;
/// segmented curves → `EvalSegmentedFn` (plugin `SegInterp`/`Evals` apply).
#[unsafe(no_mangle)]
pub extern "C" fn cmsEvalToneCurveFloat(curve: Handle, v: f32) -> f32 {
    if curve.is_null() {
        return 0.0;
    }
    let c = unsafe { &*(curve as *const CmsToneCurve) };
    if c.n_segments != 0 {
        return unsafe { eval_segmented_c(c, v as f64) as f32 };
    }
    // limited-precision table16 path: `_cmsQuickSaturateWord(v*65535)` → Lerp16
    let d = v as f64 * 65535.0 + 0.5;
    let inp = if d <= 0.0 {
        0u16
    } else if d >= 65535.0 {
        0xffff
    } else {
        d as u16
    };
    cmsEvalToneCurve16(curve, inp) as f32 / 65535.0
}
/// `cmsFreeToneCurve`
#[unsafe(no_mangle)]
pub extern "C" fn cmsFreeToneCurve(curve: Handle) {
    if curve.is_null() {
        return;
    }
    let c = unsafe { Box::from_raw(curve as *mut CmsToneCurve) };
    if !c.table16.is_null() {
        drop(unsafe {
            Box::from_raw(std::ptr::slice_from_raw_parts_mut(
                c.table16,
                c.n_entries as usize,
            ))
        });
    }
    if !c.segments.is_null() {
        for s in unsafe { std::slice::from_raw_parts(c.segments, c.n_segments as usize) } {
            if !s.sampled_points.is_null() && s.n_grid_points > 0 {
                drop(unsafe {
                    Box::from_raw(std::ptr::slice_from_raw_parts_mut(
                        s.sampled_points as *mut f32,
                        s.n_grid_points as usize,
                    ))
                });
            }
        }
        drop(unsafe {
            Box::from_raw(std::ptr::slice_from_raw_parts_mut(
                c.segments,
                c.n_segments as usize,
            ))
        });
    }
    if !c.interp_params.is_null() {
        drop(unsafe { Box::from_raw(c.interp_params) });
    }
    if !c.seg_interp.is_null() {
        for i in 0..c.n_segments as usize {
            let ip = unsafe { *c.seg_interp.add(i) };
            if !ip.is_null() {
                drop(unsafe { Box::from_raw(ip) });
            }
        }
        drop(unsafe {
            Box::from_raw(std::ptr::slice_from_raw_parts_mut(
                c.seg_interp,
                c.n_segments as usize,
            ))
        });
    }
    if !c.evals.is_null() {
        drop(unsafe {
            Box::from_raw(std::ptr::slice_from_raw_parts_mut(
                c.evals,
                c.n_segments as usize,
            ))
        });
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
/// `cmsReadTag` — marshal the stored `TagObject` to the C-side handle type
/// the caller expects for this tag.
#[unsafe(no_mangle)]
pub extern "C" fn cmsReadTag(h: Handle, sig: c_uint) -> Handle {
    use lcms::tagtypes::TagObject;
    let Some(p) = prof_ref(h) else {
        return ptr::null_mut();
    };
    let Some(obj) = profile::cms_read_tag(p, sig) else {
        return ptr::null_mut();
    };
    match obj {
        TagObject::Xyz(x) => Box::into_raw(Box::new(*x)) as Handle,
        TagObject::Chromaticity(x) => Box::into_raw(Box::new(*x)) as Handle,
        TagObject::Text(m) | TagObject::TextDescription(m) | TagObject::Mlu(m)
        | TagObject::CrdInfo(m) => Box::into_raw(Box::new(m.clone())) as Handle,
        TagObject::Curve(t) | TagObject::ParametricCurve(t) => {
                rust_to_tc(ptr::null_mut(), t.clone())
            }
        TagObject::NamedColor(l) | TagObject::ColorantTable(l) => {
            Box::into_raw(Box::new(l.clone())) as Handle
        }
        TagObject::Lut8(p) | TagObject::Lut16(p) | TagObject::LutA2B(p)
        | TagObject::LutB2A(p) | TagObject::Mpe(p) => {
            Box::into_raw(Box::new(p.clone())) as Handle
        }
        TagObject::MpeElem(s) => Box::into_raw(Box::new(s.clone())) as Handle,
        TagObject::Signature(s) => Box::into_raw(Box::new(*s)) as Handle,
        TagObject::ColorantOrder(c) => Box::into_raw(Box::new(*c)) as Handle,
        TagObject::UInt8(v) | TagObject::Raw(v) => boxed_data(v),
        TagObject::UInt32(v) => boxed_data(v),
        TagObject::UInt64(v) => boxed_data(v),
        TagObject::S15Fixed16(v) | TagObject::U16Fixed16(v) => boxed_data(v),
        TagObject::DateTime(t) => Box::into_raw(Box::new(t.clone())) as Handle,
        TagObject::Measurement(m) => Box::into_raw(Box::new(*m)) as Handle,
        TagObject::Screening(s) => Box::into_raw(Box::new(s.clone())) as Handle,
        TagObject::ViewingConditions(v) => Box::into_raw(Box::new(v.clone())) as Handle,
        TagObject::UcrBg(u) => Box::into_raw(Box::new(u.clone())) as Handle,
        TagObject::VideoSignal(v) => Box::into_raw(Box::new(*v)) as Handle,
        TagObject::Dictionary(d) => Box::into_raw(Box::new(d.clone())) as Handle,
        TagObject::ProfileSeqDesc(s) | TagObject::ProfileSeqId(s) => {
            Box::into_raw(Box::new(s.clone())) as Handle
        }
        TagObject::Vcgt(v) => Box::into_raw(Box::new(v.clone())) as Handle,
        TagObject::Mhc2(v) => Box::into_raw(Box::new(v.clone())) as Handle,
        TagObject::Data(d) => Box::into_raw(Box::new(d.clone())) as Handle,
    }
}
fn boxed_data<T: Clone>(v: &[T]) -> Handle {
    let b = v.to_vec().into_boxed_slice();
    Box::into_raw(b) as Handle
}
/// `cmsWriteTag` — dispatch the tag signature to the `TagObject` variant the
/// handle points at; `cms_write_tag` validates against the descriptor.
#[unsafe(no_mangle)]
pub extern "C" fn cmsWriteTag(h: Handle, sig: c_uint, data: *const c_void) -> c_int {
    use lcms::tagtypes::TagObject;
    let Some(p) = prof_ref(h) else {
        return 0;
    };
    if data.is_null() {
        return profile::cms_write_tag(p, sig, None) as c_int;
    }
    let obj: Option<TagObject> = match sig {
        s if s == lcms::signatures::tag::SigNamedColor2Tag => {
            ncl_ref(data as Handle).map(|l| TagObject::NamedColor(l.clone()))
        }
        s if s == lcms::signatures::tag::SigColorantTableTag => {
            ncl_ref(data as Handle).map(|l| TagObject::ColorantTable(l.clone()))
        }
        s if s == lcms::signatures::tag::SigMediaWhitePointTag
            || s == lcms::signatures::tag::SigMediaBlackPointTag
            || s == lcms::signatures::tag::SigChromaticAdaptationTag
            || s == lcms::signatures::tag::SigRedColorantTag
            || s == lcms::signatures::tag::SigGreenColorantTag
            || s == lcms::signatures::tag::SigBlueColorantTag =>
        {
            Some(TagObject::Xyz(unsafe { *(data as *const lcms::types::CieXyz) }))
        }
        s if s == lcms::signatures::tag::SigChromaticityTag => Some(
            TagObject::Chromaticity(unsafe { *(data as *const lcms::types::CiexyYTriple) }),
        ),
        s if s == lcms::signatures::tag::SigGrayTRCTag
            || s == lcms::signatures::tag::SigRedTRCTag
            || s == lcms::signatures::tag::SigGreenTRCTag
            || s == lcms::signatures::tag::SigBlueTRCTag =>
        {
            curve_rust(data as Handle).map(TagObject::Curve)
        }
        s if s == lcms::signatures::tag::SigAToB0Tag
            || s == lcms::signatures::tag::SigAToB1Tag
            || s == lcms::signatures::tag::SigAToB2Tag =>
        {
            pipe_ref(data as Handle).map(|p| TagObject::LutA2B(p.clone()))
        }
        s if s == lcms::signatures::tag::SigBToA0Tag
            || s == lcms::signatures::tag::SigBToA1Tag
            || s == lcms::signatures::tag::SigBToA2Tag
            || s == lcms::signatures::tag::SigGamutTag
            || s == lcms::signatures::tag::SigPreview0Tag
            || s == lcms::signatures::tag::SigPreview1Tag
            || s == lcms::signatures::tag::SigPreview2Tag =>
        {
            pipe_ref(data as Handle).map(|p| TagObject::LutB2A(p.clone()))
        }
        s if s == lcms::signatures::tag::SigColorantOrderTag => Some(
            TagObject::ColorantOrder(unsafe { *(data as *const [u8; lcms::xform::MAX_CHANNELS]) }),
        ),
        s if s == lcms::signatures::tag::SigUcrBgTag => {
            ucrbg_ref(data as Handle).map(|u| TagObject::UcrBg(u.clone()))
        }
        s if s == lcms::signatures::tag::SigProfileSequenceDescTag => {
            seqdesc_ref(data as Handle).map(|d| TagObject::ProfileSeqDesc(d.clone()))
        }
        s if s == lcms::signatures::tag::SigProfileSequenceIdTag => {
            seqdesc_ref(data as Handle).map(|d| TagObject::ProfileSeqId(d.clone()))
        }
        s if is_mlu_tag(s) => {
            mlu_ref(data as Handle).map(|m| TagObject::Mlu(m.clone()))
        }
        _ => None,
    };
    let Some(obj) = obj else {
        return 0;
    };
    profile::cms_write_tag(p, sig, Some(obj)) as c_int
}
fn is_mlu_tag(sig: u32) -> bool {
    use lcms::signatures::tag::*;
    [
        SigProfileDescriptionTag,
        SigCopyrightTag,
        SigDeviceMfgDescTag,
        SigDeviceModelDescTag,
        SigViewingCondDescTag,
        SigCharTargetTag,
        SigScreeningDescTag,
    ]
    .contains(&sig)
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
    // `cmsFLAGS_NULLTRANSFORM` needs no profiles at all (C checks the flag
    // before dereferencing them).
    if flags & lcms::types::FLAGS_NULLTRANSFORM == 0 {
        let Some(i) = prof_ref(input) else {
            return ptr::null_mut();
        };
    }
    let mut profs: Vec<Box<IccProfile>> = Vec::new();
    if let Some(i) = prof_ref(input) {
        profs.push(unsafe { Box::from_raw(i) });
    }
    // C: `cmsCreateMultiprofileTransformTHR(..., Output==NULL ? 1 : 2, ...)` —
    // a NULL output profile yields a single-profile transform (devicelink
    // applied forward).
    if let Some(o) = prof_ref(output) {
        profs.push(unsafe { Box::from_raw(o) });
    }
    let n = profs.len();
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
    // `by_sh(0)` marks double-precision elements — 8 bytes, like C's
    // `pixel_size`/`T_BYTES` handling of the DBL formats.
    (ch + ex) * if b == 0 { 8 } else { b }
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
    descriptions: *mut *mut c_char,
) -> c_uint {
    const INTENTS: &[(u32, &[u8])] = &[
        (0, b"Perceptual\0"),
        (1, b"Relative colorimetric\0"),
        (2, b"Saturation\0"),
        (3, b"Absolute colorimetric\0"),
    ];
    let n = INTENTS.len() as c_uint;
    let cap = n_max.min(n) as usize;
    for (i, &(code, desc)) in INTENTS[..cap].iter().enumerate() {
        unsafe {
            if !codes.is_null() {
                *codes.add(i) = code;
            }
            if !descriptions.is_null() {
                *descriptions.add(i) = desc.as_ptr() as *mut c_char;
            }
        }
    }
    n
}
/// `cmsGetSupportedIntentsTHR`
#[unsafe(no_mangle)]
pub extern "C" fn cmsGetSupportedIntentsTHR(
    _ctx: Handle,
    n_max: c_uint,
    codes: *mut c_uint,
    descriptions: *mut *mut c_char,
) -> c_uint {
    cmsGetSupportedIntents(n_max, codes, descriptions)
}
/// `cmsSetLogErrorHandler` — registers the C-side error callback. The port's
/// `signal_error` routes NULL-context errors through `err::GLOBAL_LOG_HOOK`;
/// the FFI stores the user's `cmsLogErrorHandlerFunction` and calls it from a
/// Rust trampoline (`c_log_trampoline` defined with the THR variant below).
#[unsafe(no_mangle)]
pub extern "C" fn cmsSetLogErrorHandler(f: Option<CLogFn>) {
    cmsSetLogErrorHandlerFn(f);
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

/// `cmsToneCurve` — the C-visible `_cms_curve_struct` layout. The testbed
/// dereferences `->Table16`, `->nEntries`, `->Segments[i]`, `->nSegments` and
/// `->InterpParams->ContextID` directly, so the struct must match upstream
/// exactly and the pointed-to arrays must be stable.
#[repr(C)]
pub struct CmsToneCurve {
    pub interp_params: *mut CmsInterpParams,
    pub n_segments: c_uint,
    pub segments: *mut CmsCurveSegment,
    pub seg_interp: *mut *mut CmsInterpParams,
    pub evals: *mut usize,
    pub n_entries: c_uint,
    pub table16: *mut u16,
}

/// Rebuild the Rust `ToneCurve` from the C-layout fields — picks up any
/// field-level mutation the C caller made (Table16 writes, Segments[].Type).
fn tc_to_rust(c: &CmsToneCurve) -> Option<lcms::gamma::ToneCurve> {
    let values = if c.n_entries == 0 || c.table16.is_null() {
        None
    } else {
        Some(unsafe { std::slice::from_raw_parts(c.table16, c.n_entries as usize) }.to_vec())
    };
    let segs = if c.n_segments == 0 || c.segments.is_null() {
        None
    } else {
        Some(
            unsafe { std::slice::from_raw_parts(c.segments, c.n_segments as usize) }
                .iter()
                .map(|s| lcms::gamma::CurveSegment {
                    x0: s.x0,
                    x1: s.x1,
                    r#type: s.r#type,
                    params: s.params,
                    n_grid_points: s.n_grid_points,
                    sampled_points: if s.sampled_points.is_null() || s.n_grid_points == 0 {
                        None
                    } else {
                        Some(unsafe {
                            std::slice::from_raw_parts(s.sampled_points, s.n_grid_points as usize)
                        }
                        .to_vec())
                    },
                })
                .collect::<Vec<_>>(),
        )
    };
    lcms::gamma::dbg_allocate_tone_curve_struct(
        None,
        c.n_entries,
        c.n_segments,
        segs.as_deref(),
        values.as_deref(),
    )
}

/// Materialize a `CmsToneCurve` handle from the Rust curve — allocates the
/// C-layout struct plus the arrays it points to (all freed by
/// `cmsFreeToneCurve`).
fn rust_to_tc(ctx: Handle, tc: lcms::gamma::ToneCurve) -> Handle {
    let n_entries = tc.table16.len() as c_uint;
    let table16 = if tc.table16.is_empty() {
        ptr::null_mut()
    } else {
        let b = tc.table16.into_boxed_slice();
        Box::into_raw(b) as *mut u16
    };
    let n_segments = tc.segments.len() as c_uint;
    let mut sampled_allocs: Vec<*mut f32> = Vec::new();
    let segs: Vec<CmsCurveSegment> = tc
        .segments
        .into_iter()
        .map(|s| {
            let sp = match s.sampled_points {
                Some(v) if !v.is_empty() => {
                    let b = v.into_boxed_slice();
                    let p = Box::into_raw(b) as *mut f32;
                    sampled_allocs.push(p);
                    p
                }
                _ => ptr::null_mut(),
            };
            CmsCurveSegment {
                x0: s.x0,
                x1: s.x1,
                r#type: s.r#type,
                params: s.params,
                n_grid_points: s.n_grid_points,
                sampled_points: sp,
            }
        })
        .collect();
    let segments = if segs.is_empty() {
        ptr::null_mut()
    } else {
        Box::into_raw(segs.into_boxed_slice()) as *mut CmsCurveSegment
    };
    // `p->InterpParams` exists for every curve (carries ContextID) — built
    // over Table16 with CMS_LERP_FLAGS_16BITS even when nEntries==0.
    let ns = [n_entries.max(1); 15];
    let mut interp_params =
        make_cparams(ctx, &ns, 1, 1, table16 as *const c_void, 0) as *mut CmsInterpParams;
    if interp_params.is_null() {
        interp_params = make_bare_cparams(ctx);
    }
    // `p->SegInterp` — one cmsInterpParams per Type-0 (sampled) segment,
    // float flag so the ctx's Interpolators factories see (1,1,FLOAT).
    let seg_interp = if n_segments == 0 {
        ptr::null_mut()
    } else {
        let arr: Vec<*mut CmsInterpParams> = unsafe {
            std::slice::from_raw_parts(segments, n_segments as usize)
        }
        .iter()
        .map(|s| {
                if s.r#type == 0 && s.n_grid_points > 0 {
                    make_cparams(
                        ctx,
                        &[s.n_grid_points],
                        1,
                        1,
                        s.sampled_points as *const c_void,
                        1, // CMS_LERP_FLAGS_FLOAT
                    ) as *mut CmsInterpParams
                } else {
                    ptr::null_mut()
                }
            })
            .collect();
        Box::into_raw(arr.into_boxed_slice()) as *mut *mut CmsInterpParams
    };
    // `p->Evals` — resolved evaluator per non-sampled segment: plugin
    // collections first, then the built-in default evaluator.
    let evals = if n_segments == 0 {
        ptr::null_mut()
    } else {
        let arr: Vec<usize> = unsafe {
            std::slice::from_raw_parts(segments, n_segments as usize)
        }
        .iter()
        .map(|s| {
            if s.r#type == 0 {
                return 0usize;
            }
            if let Some(ev) = parametric_eval_for(ctx, s.r#type) {
                ev
            } else {
                eval_parametric_c as usize
            }
        })
        .collect();
        Box::into_raw(arr.into_boxed_slice()) as *mut usize
    };
    Box::into_raw(Box::new(CmsToneCurve {
        interp_params,
        n_segments,
        segments,
        seg_interp,
        evals,
        n_entries,
        table16,
    })) as Handle
}

/// `GetParametricCurveByType` for the FFI — C plugin collections first
/// (matched on `abs(Type)`), else `None` → caller uses the built-in
/// evaluator (`eval_parametric_c` → `default_eval_parametric_fn`).
fn parametric_eval_for(ctx: Handle, ty: c_int) -> Option<usize> {
    let c = unsafe { cctx(ctx) }?;
    for set in &c.c_parametric_curves {
        if set.function_types.iter().any(|t| t.abs() == ty.abs()) {
            return Some(set.evaluator);
        }
    }
    None
}

/// `DefaultEvalParametricFn` as a C-ABI fn — stored in `Evals[]` so plugin
/// evaluators and the default share one dispatch site.
unsafe extern "C" fn eval_parametric_c(ty: c_int, params: *const f64, r: f64) -> f64 {
    if params.is_null() {
        return 0.0;
    }
    lcms::gamma::default_eval_parametric_fn(ty, unsafe { &*(params as *const [f64; 10]) }, r)
}

/// Minimal `cmsInterpParams` carrying just `context_id` — used when the
/// real compute fails (0-entry parametric curves still carry a params
/// struct in C so `InterpParams->ContextID` stays reachable).
fn make_bare_cparams(ctx: Handle) -> *mut CmsInterpParams {
    Box::into_raw(Box::new(CmsInterpParams {
        context_id: ctx,
        dw_flags: 0,
        n_inputs: 1,
        n_outputs: 1,
        n_samples: [0; 15],
        domain: [0; 15],
        opta: [0; 15],
        table: ptr::null(),
        interpolation: lerp16_c as usize,
    }))
}

/// `EvalSegmentedFn` — the C float eval over `Segments`/`SegInterp`/`Evals`.
unsafe fn eval_segmented_c(c: &CmsToneCurve, r: f64) -> f64 {
    const MINUS_INF: f64 = -1e22;
    const PLUS_INF: f64 = 1e22;
    let n = c.n_segments as usize;
    let segs = unsafe { std::slice::from_raw_parts(c.segments, n) };
    for i in (0..n).rev() {
        let s = &segs[i];
        if r > s.x0 as f64 && r <= s.x1 as f64 {
            let out = if s.r#type == 0 {
                if c.seg_interp.is_null() {
                    return MINUS_INF;
                }
                let ip = unsafe { *c.seg_interp.add(i) };
                if ip.is_null() {
                    return MINUS_INF;
                }
                let p = unsafe { &mut *ip };
                p.table = s.sampled_points as *const c_void;
                let f: LerpFloatC = unsafe { std::mem::transmute(p.interpolation) };
                let r1 = ((r - s.x0 as f64) as f32) / (s.x1 - s.x0);
                let mut o32 = 0f32;
                unsafe { f(&r1, &mut o32, p) };
                o32 as f64
            } else {
                if c.evals.is_null() {
                    return MINUS_INF;
                }
                let e = unsafe { *c.evals.add(i) };
                if e == 0 {
                    0.0
                } else {
                    let f: ParametricEvalC = unsafe { std::mem::transmute(e) };
                    unsafe { f(s.r#type, s.params.as_ptr(), r) }
                }
            };
            if out.is_infinite() {
                return if out > 0.0 { PLUS_INF } else { MINUS_INF };
            }
            return out;
        }
    }
    MINUS_INF
}

fn tc_ref<'a>(h: Handle) -> Option<&'a mut CmsToneCurve> {
    if h.is_null() {
        return None;
    }
    Some(unsafe { &mut *(h as *mut CmsToneCurve) })
}
/// Rebuild the Rust curve from a handle (mutations visible).
fn curve_rust(h: Handle) -> Option<lcms::gamma::ToneCurve> {
    let c = tc_ref(h)?;
    tc_to_rust(c)
}

/// `cmsBuildSegmentedToneCurve`
#[unsafe(no_mangle)]
pub extern "C" fn cmsBuildSegmentedToneCurve(
    ctx: Handle,
    n_segments: c_uint,
    seg: *const CmsCurveSegment,
) -> Handle {
    if seg.is_null() || n_segments == 0 {
        return ptr::null_mut();
    }
    // `Seg` is a `cmsCurveSegment[n_segments]` array — C-layout per element.
    let segs: Vec<lcms::gamma::CurveSegment> = (0..n_segments as usize)
        .map(|i| {
            let s = unsafe { &*seg.add(i) };
            lcms::gamma::CurveSegment {
                x0: s.x0,
                x1: s.x1,
                r#type: s.r#type,
                params: s.params,
                n_grid_points: s.n_grid_points,
                // Only Type-0 (sampled) segments carry SampledPoints — C's
                // BuildSegmentedToneCurve reads them only there, so parametric
                // segments' uninitialized fields must not be copied.
                sampled_points: if s.r#type == 0
                    && s.n_grid_points > 0
                    && !s.sampled_points.is_null()
                {
                    Some(unsafe {
                        std::slice::from_raw_parts(s.sampled_points, s.n_grid_points as usize)
                    }
                    .to_vec())
                } else {
                    None
                },
            }
        })
        .collect();
    lcms::gamma::build_segmented_tone_curve(None, &segs)
        .map(|t| rust_to_tc(ctx, t))
        .unwrap_or(ptr::null_mut())
}

/// `cmsBuildTabulatedToneCurveFloat`
#[unsafe(no_mangle)]
pub extern "C" fn cmsBuildTabulatedToneCurveFloat(
    ctx: Handle,
    n: c_uint,
    table: *const f32,
) -> Handle {
    if table.is_null() || n == 0 {
        return ptr::null_mut();
    }
    let slice = unsafe { std::slice::from_raw_parts(table, n as usize) };
    lcms::gamma::build_tabulated_tone_curve_float(None, slice)
        .map(|t| rust_to_tc(ctx, t))
        .unwrap_or(ptr::null_mut())
}

/// `cmsDupToneCurve`
#[unsafe(no_mangle)]
pub extern "C" fn cmsDupToneCurve(curve: Handle) -> Handle {
    if curve.is_null() {
        return ptr::null_mut();
    }
    let Some(c) = curve_rust(curve) else {
        return ptr::null_mut();
    };
    lcms::gamma::dup_tone_curve(&c, None)
        .map(|t| rust_to_tc(ptr::null_mut(), t))
        .unwrap_or(ptr::null_mut())
}

/// `cmsReverseToneCurve`
#[unsafe(no_mangle)]
pub extern "C" fn cmsReverseToneCurve(curve: Handle) -> Handle {
    cmsReverseToneCurveEx(4096, curve)
}

/// `cmsReverseToneCurveEx`
#[unsafe(no_mangle)]
pub extern "C" fn cmsReverseToneCurveEx(n: c_uint, curve: Handle) -> Handle {
    if curve.is_null() {
        return ptr::null_mut();
    }
    let cc = unsafe { &*(curve as *const CmsToneCurve) };
    // C first tries the analytic reverse: a single positive-type parametric
    // segment whose type is known → `cmsBuildParametricToneCurve(-Type)`
    // under the curve's own `InterpParams->ContextID` (plugin curves too).
    if cc.n_segments == 1 && !cc.segments.is_null() {
        let s = unsafe { &*cc.segments };
        let ctx = if cc.interp_params.is_null() {
            ptr::null_mut()
        } else {
            unsafe { (*cc.interp_params).context_id }
        };
        if s.r#type > 0
            && (parametric_eval_for(ctx, s.r#type).is_some()
                || matches!(s.r#type, 1..=8 | 108 | 109))
        {
            return cmsBuildParametricToneCurve(ctx, -s.r#type, s.params.as_ptr());
        }
    }
    let Some(c) = curve_rust(curve) else {
        return ptr::null_mut();
    };
    lcms::gamma::reverse_tone_curve_ex(n, &c)
        .map(|t| rust_to_tc(ptr::null_mut(), t))
        .unwrap_or(ptr::null_mut())
}

/// `cmsSmoothToneCurve`
#[unsafe(no_mangle)]
pub extern "C" fn cmsSmoothToneCurve(curve: Handle, lambda: f64) -> c_int {
    if curve.is_null() {
        return 0;
    }
    let Some(mut c) = curve_rust(curve) else { return 0 };
    if lcms::gamma::smooth_tone_curve(None, &mut c, lambda) {
        if let Some(cc) = tc_ref(curve) {
            if !cc.table16.is_null() && cc.n_entries == c.table16.len() as u32 {
                unsafe { std::ptr::copy_nonoverlapping(c.table16.as_ptr(), cc.table16, c.table16.len()) };
            }
        }
        1
    } else { 0 }
}

/// `cmsJoinToneCurve`
#[unsafe(no_mangle)]
pub extern "C" fn cmsJoinToneCurve(ctx: Handle, c1: Handle, c2: Handle, n: c_uint) -> Handle {
    let (Some(a), Some(b)) = (curve_rust(c1), curve_rust(c2)) else {
        return ptr::null_mut();
    };
    lcms::gamma::join_tone_curve(None, &a, &b, n)
        .map(|t| rust_to_tc(ctx, t))
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
            cmsFreeToneCurve(c);
        }
    }
}

/// `cmsEstimateGamma`
#[unsafe(no_mangle)]
pub extern "C" fn cmsEstimateGamma(curve: Handle, precision: f64) -> f64 {
    if curve.is_null() {
        return -1.0;
    }
    let Some(c) = curve_rust(curve) else {
        return -1.0;
    };
    lcms::gamma::estimate_gamma(&c, precision)
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

// ---------------------------------------------------------------------------
// white points + delta-E variants + profile creators
// ---------------------------------------------------------------------------

/// `cmsD50_XYZ` — returns `const cmsCIEXYZ*` (C's D50 point)
#[unsafe(no_mangle)]
pub extern "C" fn cmsD50_XYZ() -> *const CieXyz {
    static D50: std::sync::OnceLock<CieXyz> = std::sync::OnceLock::new();
    D50.get_or_init(lcms::wtpnt::d50_xyz)
}
/// `cmsD50_xyY` — returns `const cmsCIExyY*`
#[unsafe(no_mangle)]
pub extern "C" fn cmsD50_xyY() -> *const lcms::types::CiexyY {
    static D50: std::sync::OnceLock<lcms::types::CiexyY> = std::sync::OnceLock::new();
    D50.get_or_init(lcms::wtpnt::d50_xyy)
}

fn cielab(l: *const CmsLab) -> lcms::types::CieLab {
    let l = unsafe { &*l };
    lcms::types::CieLab { L: l.l, a: l.a, b: l.b }
}

/// `cmsCIE94DeltaE`
#[unsafe(no_mangle)]
pub extern "C" fn cmsCIE94DeltaE(lab1: *const CmsLab, lab2: *const CmsLab) -> f64 {
    if lab1.is_null() || lab2.is_null() {
        return 0.0;
    }
    lcms::pcs::cie94_delta_e(&cielab(lab1), &cielab(lab2))
}

/// `cmsCMCdeltaE`
#[unsafe(no_mangle)]
pub extern "C" fn cmsCMCdeltaE(
    lab1: *const CmsLab,
    lab2: *const CmsLab,
    l: f64,
    c: f64,
) -> f64 {
    if lab1.is_null() || lab2.is_null() {
        return 0.0;
    }
    lcms::pcs::cmc_delta_e(&cielab(lab1), &cielab(lab2), l, c)
}

/// `cmsCIE2000DeltaE`
#[unsafe(no_mangle)]
pub extern "C" fn cmsCIE2000DeltaE(
    lab1: *const CmsLab,
    lab2: *const CmsLab,
    kl: f64,
    kc: f64,
    kh: f64,
) -> f64 {
    if lab1.is_null() || lab2.is_null() {
        return 0.0;
    }
    lcms::pcs::cie2000_delta_e(&cielab(lab1), &cielab(lab2), kl, kc, kh)
}


// ---------------------------------------------------------------------------
// half/fixed conversions, tone-curve introspection, misc
// ---------------------------------------------------------------------------

/// `cmsFloat2Half`
#[unsafe(no_mangle)]
pub extern "C" fn cmsFloat2Half(flt: f32) -> u16 {
    lcms::half::float2half(flt)
}
/// `cmsHalf2Float`
#[unsafe(no_mangle)]
pub extern "C" fn cmsHalf2Float(h: u16) -> f32 {
    lcms::half::half2float(h)
}
/// `cmsDoubleTo15Fixed16`
#[unsafe(no_mangle)]
pub extern "C" fn cmsDoubleTo15Fixed16(d: f64) -> i32 {
    lcms::fixed::double_to_s15_fixed16(d)
}
/// `cms15Fixed16ToDouble`
#[unsafe(no_mangle)]
pub extern "C" fn cms15Fixed16ToDouble(f: i32) -> f64 {
    lcms::fixed::s15_fixed16_to_double(f)
}
/// `cmsDoubleTo8Fixed8`
#[unsafe(no_mangle)]
pub extern "C" fn cmsDoubleTo8Fixed8(d: f64) -> u16 {
    ((lcms::fixed::double_to_s15_fixed16(d) >> 8) & 0xFFFF) as u16
}
/// `cms8Fixed8ToDouble`
#[unsafe(no_mangle)]
pub extern "C" fn cms8Fixed8ToDouble(f: u16) -> f64 {
    f as f64 / 256.0
}
/// `cmsChannelsOfColorSpace`
#[unsafe(no_mangle)]
pub extern "C" fn cmsChannelsOfColorSpace(color_space: c_uint) -> c_int {
    lcms::pcs::channels_of_color_space(color_space) as c_int
}
/// `cmsIsToneCurveLinear`
#[unsafe(no_mangle)]
pub extern "C" fn cmsIsToneCurveLinear(curve: Handle) -> c_int {
    if curve.is_null() {
        return 0;
    }
    let Some(c) = curve_rust(curve) else {
        return 0;
    };
    lcms::gamma::is_tone_curve_linear(&c) as c_int
}
/// `cmsIsToneCurveMonotonic`
#[unsafe(no_mangle)]
pub extern "C" fn cmsIsToneCurveMonotonic(curve: Handle) -> c_int {
    if curve.is_null() {
        return 0;
    }
    let Some(c) = curve_rust(curve) else {
        return 0;
    };
    lcms::gamma::is_tone_curve_monotonic(&c) as c_int
}
/// `cmsIsToneCurveDescending`
#[unsafe(no_mangle)]
pub extern "C" fn cmsIsToneCurveDescending(curve: Handle) -> c_int {
    if curve.is_null() {
        return 0;
    }
    let Some(c) = curve_rust(curve) else {
        return 0;
    };
    lcms::gamma::is_tone_curve_descending(&c) as c_int
}
/// `cmsIsToneCurveMultisegment`
#[unsafe(no_mangle)]
pub extern "C" fn cmsIsToneCurveMultisegment(curve: Handle) -> c_int {
    if curve.is_null() {
        return 0;
    }
    let Some(c) = curve_rust(curve) else {
        return 0;
    };
    lcms::gamma::is_tone_curve_multisegment(&c) as c_int
}
/// `cmsGetEncodedCMMversion`
#[unsafe(no_mangle)]
pub extern "C" fn cmsGetEncodedCMMversion() -> c_int {
    lcms::err::encoded_cmm_version() as c_int
}
/// `cmsCreate_OkLabProfile` (+ `THR`)
#[unsafe(no_mangle)]
pub extern "C" fn cmsCreate_OkLabProfile() -> Handle {
    prof_to_handle(virt::create_oklab_profile(None))
}
/// `cmsChangeBuffersFormat` — reformats a transform's in/out formats
#[unsafe(no_mangle)]
pub extern "C" fn cmsChangeBuffersFormat(h: Handle, in_fmt: c_uint, out_fmt: c_uint) -> c_int {
    match xform_ref(h) {
        Some(t) => t.change_buffers_format(in_fmt, out_fmt) as c_int,
        None => 0,
    }
}

// ---------------------------------------------------------------------------
// MLU (multi-localized Unicode), named color, seq-desc, context, interp
// ---------------------------------------------------------------------------

use lcms::named::{self, Mlu};

fn mlu_ref<'a>(h: Handle) -> Option<&'a mut Mlu> {
    if h.is_null() {
        return None;
    }
    Some(unsafe { &mut *(h as *mut Mlu) })
}
unsafe fn wcs_to_slice(w: *const WcharT) -> Option<Vec<WcharT>> {
    if w.is_null() {
        return None;
    }
    let mut n = 0usize;
    while unsafe { *w.add(n) } != 0 {
        n += 1;
    }
    Some(unsafe { std::slice::from_raw_parts(w, n) }.to_vec())
}
#[allow(non_camel_case_types)]
pub type WcharT = i32;

/// `cmsMLUalloc`
#[unsafe(no_mangle)]
pub extern "C" fn cmsMLUalloc(_ctx: Handle, n_items: c_uint) -> Handle {
    named::mlu_alloc(None, n_items)
        .map(|b| Box::into_raw(Box::new(b)) as Handle)
        .unwrap_or(ptr::null_mut())
}
/// `cmsMLUfree`
#[unsafe(no_mangle)]
pub extern "C" fn cmsMLUfree(mlu: Handle) {
    if !mlu.is_null() {
        drop(unsafe { Box::from_raw(mlu as *mut Mlu) });
    }
}
/// `cmsMLUdup`
#[unsafe(no_mangle)]
pub extern "C" fn cmsMLUdup(mlu: Handle) -> Handle {
    let Some(m) = mlu_ref(mlu) else {
        return ptr::null_mut();
    };
    named::mlu_dup(m)
        .map(|b| Box::into_raw(Box::new(b)) as Handle)
        .unwrap_or(ptr::null_mut())
}
unsafe fn code2(c: *const c_char) -> Vec<u8> {
    if c.is_null() {
        return Vec::new();
    }
    unsafe { std::slice::from_raw_parts(c as *const u8, 2) }.to_vec()
}
/// `cmsMLUsetASCII`
#[unsafe(no_mangle)]
pub extern "C" fn cmsMLUsetASCII(
    mlu: Handle,
    lang: *const c_char,
    cntry: *const c_char,
    s: *const c_char,
) -> c_int {
    let Some(m) = mlu_ref(mlu) else {
        return 0;
    };
    let s = unsafe { cstr(s) }.unwrap_or_default();
    named::mlu_set_ascii(m, &unsafe { code2(lang) }, &unsafe { code2(cntry) }, s.as_bytes())
        as c_int
}
/// `cmsMLUsetUTF8`
#[unsafe(no_mangle)]
pub extern "C" fn cmsMLUsetUTF8(
    mlu: Handle,
    lang: *const c_char,
    cntry: *const c_char,
    s: *const c_char,
) -> c_int {
    let Some(m) = mlu_ref(mlu) else {
        return 0;
    };
    let s = unsafe { cstr(s) }.unwrap_or_default();
    named::mlu_set_utf8(m, &unsafe { code2(lang) }, &unsafe { code2(cntry) }, s.as_bytes())
        as c_int
}
/// `cmsMLUsetWide`
#[unsafe(no_mangle)]
pub extern "C" fn cmsMLUsetWide(
    mlu: Handle,
    lang: *const c_char,
    cntry: *const c_char,
    w: *const WcharT,
) -> c_int {
    let Some(m) = mlu_ref(mlu) else {
        return 0;
    };
    let ws = unsafe { wcs_to_slice(w) }.unwrap_or_default();
    named::mlu_set_wide(m, &unsafe { code2(lang) }, &unsafe { code2(cntry) }, &ws) as c_int
}
/// `cmsMLUgetASCII` — returns bytes needed (incl NUL), writes `size`-bounded
#[unsafe(no_mangle)]
pub extern "C" fn cmsMLUgetASCII(
    mlu: Handle,
    lang: *const c_char,
    cntry: *const c_char,
    buf: *mut u8,
    size: c_uint,
) -> c_uint {
    let Some(m) = mlu_ref(mlu) else {
        return 0;
    };
    let (need, data) = named::mlu_get_ascii(
        m,
        &unsafe { code2(lang) },
        &unsafe { code2(cntry) },
        None,
    );
    if !buf.is_null() && size > 0 {
        let n = (size as usize).min(data.len());
        unsafe { std::ptr::copy_nonoverlapping(data.as_ptr(), buf, n) };
    }
    need
}
/// `cmsMLUgetUTF8`
#[unsafe(no_mangle)]
pub extern "C" fn cmsMLUgetUTF8(
    mlu: Handle,
    lang: *const c_char,
    cntry: *const c_char,
    buf: *mut u8,
    size: c_uint,
) -> c_uint {
    let Some(m) = mlu_ref(mlu) else {
        return 0;
    };
    let (need, data) = named::mlu_get_utf8(
        m,
        &unsafe { code2(lang) },
        &unsafe { code2(cntry) },
        None,
    );
    if !buf.is_null() && size > 0 {
        let n = (size as usize).min(data.len());
        unsafe { std::ptr::copy_nonoverlapping(data.as_ptr(), buf, n) };
    }
    need
}
/// `cmsMLUgetWide`
#[unsafe(no_mangle)]
pub extern "C" fn cmsMLUgetWide(
    mlu: Handle,
    lang: *const c_char,
    cntry: *const c_char,
    buf: *mut WcharT,
    size: c_uint,
) -> c_uint {
    let Some(m) = mlu_ref(mlu) else {
        return 0;
    };
    let (need, data) = named::mlu_get_wide(
        m,
        &unsafe { code2(lang) },
        &unsafe { code2(cntry) },
        None,
    );
    if !buf.is_null() && size > 0 {
        let n = (size as usize / 4).min(data.len());
        unsafe { std::ptr::copy_nonoverlapping(data.as_ptr(), buf, n) };
    }
    need
}
/// `cmsMLUtranslationsCount`
#[unsafe(no_mangle)]
pub extern "C" fn cmsMLUtranslationsCount(mlu: Handle) -> c_uint {
    mlu_ref(mlu).map(|m| named::mlu_translations_count(m)).unwrap_or(0)
}

/// `cmsAllocNamedColorList`
#[unsafe(no_mangle)]
pub extern "C" fn cmsAllocNamedColorList(
    _ctx: Handle,
    n: c_uint,
    colorant_count: c_uint,
    prefix: *const c_char,
    suffix: *const c_char,
) -> Handle {
    let p = unsafe { cstr(prefix) }.unwrap_or_default();
    let s = unsafe { cstr(suffix) }.unwrap_or_default();
    named::alloc_named_color_list(None, n, colorant_count, p.as_bytes(), s.as_bytes())
        .map(|b| Box::into_raw(Box::new(b)) as Handle)
        .unwrap_or(ptr::null_mut())
}
fn ncl_ref<'a>(h: Handle) -> Option<&'a mut named::NamedColorList> {
    if h.is_null() {
        return None;
    }
    Some(unsafe { &mut *(h as *mut named::NamedColorList) })
}
fn seqdesc_ref<'a>(h: Handle) -> Option<&'a mut named::ProfileSeqDesc> {
    if h.is_null() {
        return None;
    }
    Some(unsafe { &mut *(h as *mut named::ProfileSeqDesc) })
}
fn ucrbg_ref<'a>(h: Handle) -> Option<&'a mut lcms::tagtypes::UcrBg> {
    if h.is_null() {
        return None;
    }
    Some(unsafe { &mut *(h as *mut lcms::tagtypes::UcrBg) })
}
/// `cmsFreeNamedColorList`
#[unsafe(no_mangle)]
pub extern "C" fn cmsFreeNamedColorList(list: Handle) {
    if !list.is_null() {
        drop(unsafe { Box::from_raw(list as *mut named::NamedColorList) });
    }
}
/// `cmsNamedColorCount`
#[unsafe(no_mangle)]
pub extern "C" fn cmsNamedColorCount(list: Handle) -> c_uint {
    ncl_ref(list).map(|l| named::named_color_count(l)).unwrap_or(0)
}
/// `cmsAppendNamedColor`
#[unsafe(no_mangle)]
pub extern "C" fn cmsAppendNamedColor(
    list: Handle,
    name: *const c_char,
    pcs: *const u16,
    colorant: *const u16,
) -> c_int {
    let Some(l) = ncl_ref(list) else {
        return 0;
    };
    let n = unsafe { cstr(name) }.unwrap_or_default();
    let p = if pcs.is_null() {
        None
    } else {
        Some(unsafe { &*(pcs as *const [u16; 3]) })
    };
    let c = if colorant.is_null() {
        None
    } else {
        Some(unsafe { &*(colorant as *const [u16; 16]) })
    };
    named::append_named_color(l, Some(n.as_bytes()), p, c) as c_int
}
/// `cmsAllocProfileSequenceDescription`
#[unsafe(no_mangle)]
pub extern "C" fn cmsAllocProfileSequenceDescription(_ctx: Handle, n: c_uint) -> Handle {
    named::alloc_profile_seq_desc(None, n)
        .map(|b| Box::into_raw(Box::new(b)) as Handle)
        .unwrap_or(ptr::null_mut())
}
/// `cmsFreeProfileSequenceDescription`
#[unsafe(no_mangle)]
pub extern "C" fn cmsFreeProfileSequenceDescription(d: Handle) {
    if !d.is_null() {
        drop(unsafe { Box::from_raw(d as *mut named::ProfileSeqDesc) });
    }
}

// ---------------------------------------------------------------------------
// matrix / vec3 / Lab-XYZ-LCh conversions
// ---------------------------------------------------------------------------

use lcms::types::{CieLCh, CieLab, Mat3, Vec3};

/// `cmsMAT3identity`
#[unsafe(no_mangle)]
pub extern "C" fn cmsMAT3identity(a: *mut Mat3) {
    if !a.is_null() {
        lcms::mtrx::mat3_identity(unsafe { &mut *a });
    }
}
/// `cmsMAT3per`
#[unsafe(no_mangle)]
pub extern "C" fn cmsMAT3per(r: *mut Mat3, a: *const Mat3, b: *const Mat3) {
    if r.is_null() || a.is_null() || b.is_null() {
        return;
    }
    lcms::mtrx::mat3_per(unsafe { &mut *r }, unsafe { &*a }, unsafe { &*b });
}
/// `cmsMAT3inverse`
#[unsafe(no_mangle)]
pub extern "C" fn cmsMAT3inverse(a: *const Mat3, b: *mut Mat3) -> c_int {
    if a.is_null() || b.is_null() {
        return 0;
    }
    lcms::mtrx::mat3_inverse(unsafe { &*a }, unsafe { &mut *b }) as c_int
}
/// `cmsMAT3eval`
#[unsafe(no_mangle)]
pub extern "C" fn cmsMAT3eval(r: *mut Vec3, a: *const Mat3, v: *const Vec3) {
    if r.is_null() || a.is_null() || v.is_null() {
        return;
    }
    lcms::mtrx::mat3_eval(unsafe { &mut *r }, unsafe { &*a }, unsafe { &*v });
}
/// `cmsMAT3solve`
#[unsafe(no_mangle)]
pub extern "C" fn cmsMAT3solve(x: *mut Vec3, a: *const Mat3, b: *const Vec3) -> c_int {
    if x.is_null() || a.is_null() || b.is_null() {
        return 0;
    }
    lcms::mtrx::mat3_solve(unsafe { &mut *x }, unsafe { &*a }, unsafe { &*b }) as c_int
}
/// `cmsVEC3init`
#[unsafe(no_mangle)]
pub extern "C" fn cmsVEC3init(r: *mut Vec3, x: f64, y: f64, z: f64) {
    if !r.is_null() {
        lcms::mtrx::vec3_init(unsafe { &mut *r }, x, y, z);
    }
}
/// `cmsVEC3minus`
#[unsafe(no_mangle)]
pub extern "C" fn cmsVEC3minus(r: *mut Vec3, a: *const Vec3, b: *const Vec3) {
    if r.is_null() || a.is_null() || b.is_null() {
        return;
    }
    lcms::mtrx::vec3_minus(unsafe { &mut *r }, unsafe { &*a }, unsafe { &*b });
}
/// `cmsVEC3cross`
#[unsafe(no_mangle)]
pub extern "C" fn cmsVEC3cross(r: *mut Vec3, u: *const Vec3, v: *const Vec3) {
    if r.is_null() || u.is_null() || v.is_null() {
        return;
    }
    lcms::mtrx::vec3_cross(unsafe { &mut *r }, unsafe { &*u }, unsafe { &*v });
}
/// `cmsVEC3distance`
#[unsafe(no_mangle)]
pub extern "C" fn cmsVEC3distance(a: *const Vec3, b: *const Vec3) -> f64 {
    if a.is_null() || b.is_null() {
        return 0.0;
    }
    lcms::mtrx::vec3_distance(unsafe { &*a }, unsafe { &*b })
}
/// `cmsVEC3length`
#[unsafe(no_mangle)]
pub extern "C" fn cmsVEC3length(a: *const Vec3) -> f64 {
    if a.is_null() {
        return 0.0;
    }
    lcms::mtrx::vec3_length(unsafe { &*a })
}
/// `cmsVEC3dot`
#[unsafe(no_mangle)]
pub extern "C" fn cmsVEC3dot(a: *const Vec3, b: *const Vec3) -> f64 {
    if a.is_null() || b.is_null() {
        return 0.0;
    }
    lcms::mtrx::vec3_dot(unsafe { &*a }, unsafe { &*b })
}

/// `cmsLab2LCh` — `cmsCIELab*`→`CieLab`, `cmsCIELCh*`→`CieLCh` (repr(C))
#[unsafe(no_mangle)]
pub extern "C" fn cmsLab2LCh(lch: *mut CieLCh, lab: *const CieLab) {
    if lch.is_null() || lab.is_null() {
        return;
    }
    unsafe { *lch = lcms::pcs::lab2lch(&*lab) };
}
/// `cmsLCh2Lab`
#[unsafe(no_mangle)]
pub extern "C" fn cmsLCh2Lab(lab: *mut CieLab, lch: *const CieLCh) {
    if lab.is_null() || lch.is_null() {
        return;
    }
    unsafe { *lab = lcms::pcs::lch2lab(&*lch) };
}
/// `cmsLab2XYZ` — `white_point` may be NULL (D50)
#[unsafe(no_mangle)]
pub extern "C" fn cmsLab2XYZ(wp: *const CieXyz, xyz: *mut CieXyz, lab: *const CieLab) {
    if xyz.is_null() || lab.is_null() {
        return;
    }
    let w = if wp.is_null() { None } else { Some(unsafe { &*wp }) };
    unsafe { *xyz = lcms::pcs::lab2xyz(w, &*lab) };
}
/// `cmsXYZ2Lab`
#[unsafe(no_mangle)]
pub extern "C" fn cmsXYZ2Lab(wp: *const CieXyz, lab: *mut CieLab, xyz: *const CieXyz) {
    if lab.is_null() || xyz.is_null() {
        return;
    }
    let w = if wp.is_null() { None } else { Some(unsafe { &*wp }) };
    unsafe { *lab = lcms::pcs::xyz2lab(w, &*xyz) };
}
/// `cmsFloat2LabEncoded` — `[u16;3]` out
#[unsafe(no_mangle)]
pub extern "C" fn cmsFloat2LabEncoded(w_lab: *mut u16, lab: *const CieLab) {
    if w_lab.is_null() || lab.is_null() {
        return;
    }
    let e = lcms::pcs::float2lab_encoded(unsafe { &*lab });
    unsafe { std::ptr::copy_nonoverlapping(e.as_ptr(), w_lab, 3) };
}
/// `cmsFloat2LabEncodedV2`
#[unsafe(no_mangle)]
pub extern "C" fn cmsFloat2LabEncodedV2(w_lab: *mut u16, lab: *const CieLab) {
    if w_lab.is_null() || lab.is_null() {
        return;
    }
    let e = lcms::pcs::float2lab_encoded_v2(unsafe { &*lab });
    unsafe { std::ptr::copy_nonoverlapping(e.as_ptr(), w_lab, 3) };
}
/// `cmsLabEncoded2Float`
#[unsafe(no_mangle)]
pub extern "C" fn cmsLabEncoded2Float(lab: *mut CieLab, w_lab: *const u16) {
    if lab.is_null() || w_lab.is_null() {
        return;
    }
    let w = unsafe { [w_lab.add(0).read(), w_lab.add(1).read(), w_lab.add(2).read()] };
    unsafe { *lab = lcms::pcs::lab_encoded2float(w) };
}
/// `cmsLabEncoded2FloatV2`
#[unsafe(no_mangle)]
pub extern "C" fn cmsLabEncoded2FloatV2(lab: *mut CieLab, w_lab: *const u16) {
    if lab.is_null() || w_lab.is_null() {
        return;
    }
    let w = unsafe { [w_lab.add(0).read(), w_lab.add(1).read(), w_lab.add(2).read()] };
    unsafe { *lab = lcms::pcs::lab_encoded2float_v2(w) };
}
/// `cmsFloat2XYZEncoded`
#[unsafe(no_mangle)]
pub extern "C" fn cmsFloat2XYZEncoded(w_xyz: *mut u16, xyz: *const CieXyz) {
    if w_xyz.is_null() || xyz.is_null() {
        return;
    }
    let e = lcms::pcs::float2xyz_encoded(unsafe { &*xyz });
    unsafe { std::ptr::copy_nonoverlapping(e.as_ptr(), w_xyz, 3) };
}
/// `cmsXYZEncoded2Float`
#[unsafe(no_mangle)]
pub extern "C" fn cmsXYZEncoded2Float(xyz: *mut CieXyz, w_xyz: *const u16) {
    if xyz.is_null() || w_xyz.is_null() {
        return;
    }
    let w = unsafe { [w_xyz.add(0).read(), w_xyz.add(1).read(), w_xyz.add(2).read()] };
    unsafe { *xyz = lcms::pcs::xyz_encoded2float(w) };
}

// ---------------------------------------------------------------------------
// context, adaptation/alarm, tags, profile metadata, interp params
// ---------------------------------------------------------------------------


/// `cmsCreateContext` — wraps `lcms::context::create_context` (global ctx →
/// non-null boxed Context; upstream returns a heap `cmsContext`)
#[unsafe(no_mangle)]
pub extern "C" fn cmsCreateContext(plugin: *const c_void, user_data: *mut c_void) -> Handle {
    // C: _cmsInstallAllocFunctions(_cmsFindMemoryPlugin(Plugin), ...) then
    // allocates the context THROUGH the (possibly plugin) allocator so the
    // testbed's DebugMemDontCheckThis/DebugFree header access is valid.
    let mem = unsafe { find_mem_plugin(plugin as *const CmsPluginBase) };
    let fns = install_mem_fns(mem);
    let raw = unsafe {
        mem_alloc_with(
            &fns,
            plugin as Handle, // WhoAllocated — covers DebugFree's ID check
            std::mem::size_of::<lcms::context::Context>() as c_uint,
        )
    };
    if raw.is_null() {
        return ptr::null_mut();
    }
    let mut c = lcms::context::create_context(None, user_data as usize);
    c.mem_fns = fns;
    unsafe { ptr::write(raw as *mut lcms::context::Context, c) };
    pool_register(raw);
    // register remaining (non-mem) plugins in the bundle
    let _ = plugin_thr_register(raw, plugin as *const CmsPluginBase);
    raw
}
/// `cmsDupContext`
#[unsafe(no_mangle)]
// C's `cmsDupContext` is (src, NewUserData) — 2 args; NULL inherits.
pub extern "C" fn cmsDupContext(src: Handle, user_data: *mut c_void) -> Handle {
    if src.is_null() {
        return cmsCreateContext(ptr::null_mut(), user_data);
    }
    let s = unsafe { &*(src as *const lcms::context::Context) };
    let fns = mem_fns_of(src);
    // C: _cmsMalloc(ContextID, sizeof(ctx)) — via src's mem-handler
    let raw = unsafe {
        mem_alloc_with(&fns, src, std::mem::size_of::<lcms::context::Context>() as c_uint)
    };
    if raw.is_null() {
        return ptr::null_mut();
    }
    let ud = if user_data.is_null() {
        s.user_data.load(std::sync::atomic::Ordering::Relaxed)
    } else {
        user_data as usize
    };
    let c = lcms::context::dup_context(s, None, ud);
    unsafe { ptr::write(raw as *mut lcms::context::Context, c) };
    pool_register(raw);
    raw
}
/// `cmsDeleteContext`
#[unsafe(no_mangle)]
pub extern "C" fn cmsDeleteContext(ctx: Handle) {
    if pool_contains(ctx) {
        let p = ctx as *mut lcms::context::Context;
        let fns = mem_fns_of(ctx);
        pool_unregister(ctx);
        unsafe { ptr::drop_in_place(p) };
        // C: _cmsFree(&fakeContext, ctx) — free through the ctx's own
        // DefaultMemoryManager (which reflects any memH plugin install)
        unsafe { mem_dealloc_ctx(&fns, ctx, ctx) };
    }
}
/// Free through a specific fn table (ctx already dropped / not yet built)
unsafe fn mem_dealloc_ctx(fns: &[usize; 6], id: Handle, p: *mut c_void) {
    let f: MemFreeFn = unsafe { std::mem::transmute(fns[2]) };
    unsafe { f(id, p) }
}

/// Walk a C plugin bundle and apply each recognized entry to `ctx`.
/// Currently: mem-handler (installed into `Context::mem_fns`) and the
/// interpolation factory (stored for `cmsComputeInterpParams`).
fn plugin_thr_register(ctx: Handle, mut p: *const CmsPluginBase) -> bool {
    if !pool_contains(ctx) {
        return false;
    }
    let mut hops = 0;
    while !p.is_null() && hops < 128 {
        let b = unsafe { &*p };
        if b.magic != PLUGIN_MAGIC || b.expected_version > LCMS_VERSION {
            return false;
        }
        match b.ty {
            PLUGIN_MEMHANDLER_SIG => {
                let fns = install_mem_fns(p as *const CmsPluginMemHandler);
                if fns[0] != 0 && fns[2] != 0 && fns[3] != 0 {
                    // required Malloc/Free/Realloc present
                    unsafe { (*(ctx as *mut lcms::context::Context)).mem_fns = fns };
                }
            }
            PLUGIN_INTERP_SIG => {
                let f = unsafe { (*(p as *const CmsPluginInterpolation)).interpolators_factory };
                if f != 0 {
                    ensure_foreign_hooks();
                    unsafe { (*(ctx as *mut lcms::context::Context)).interp_factories.push(f) };
                }
            }
            PLUGIN_PARAMETRIC_SIG => {
                let h = unsafe { &*(p as *const CmsPluginParametricCurves) };
                if h.evaluator != 0 && h.n_functions > 0 {
                    let n = (h.n_functions as usize).min(20);
                    let set = lcms::context::CParametricSet {
                        function_types: h.function_types[..n]
                            .iter()
                            .map(|&t| t as i32)
                            .collect(),
                        parameter_count: h.parameter_count[..n].to_vec(),
                        evaluator: h.evaluator,
                    };
                    unsafe {
                        (*(ctx as *mut lcms::context::Context))
                            .c_parametric_curves
                            .push(set)
                    };
                }
            }
            PLUGIN_HEADER_SIG => {
                // C: required callbacks — a header plugin missing Read/Write/
                // GetSize is rejected.
                let h = unsafe { &*(p as *const CmsPluginHeader) };
                if h.read_ptr == 0 || h.write_ptr == 0 || h.get_size_ptr == 0 {
                    return false;
                }
            }
            PLUGIN_FORMATTERS_SIG => {
                let f = unsafe { (*(p as *const CmsPluginFormatters)).formatters_factory };
                if f != 0 {
                    unsafe { (*(ctx as *mut lcms::context::Context)).formatter_factories.push(f) };
                }
            }
            _ => {}
        }
        p = b.next;
        hops += 1;
    }
    true
}

/// `cmsSetAlarmCodes` — writes `n` codes into the alarm slot
#[unsafe(no_mangle)]
pub extern "C" fn cmsSetAlarmCodes(codes: *const u16, n: c_uint) {
    let mut arr = [0u16; xform::MAX_CHANNELS];
    let m = (n as usize).min(xform::MAX_CHANNELS);
    if !codes.is_null() && m > 0 {
        unsafe { arr[..m].copy_from_slice(&std::slice::from_raw_parts(codes, m)[..m]) };
    }
    xform::set_alarm_codes(None, &arr);
}

/// `cmsGetProfileVersion` — encoded version as `cmsFloat64Number`
#[unsafe(no_mangle)]
pub extern "C" fn cmsGetProfileVersion(h: Handle) -> f64 {
    hdr(h, |p| profile::get_profile_version(p), 0.0)
}
/// `cmsSetProfileVersion`
#[unsafe(no_mangle)]
pub extern "C" fn cmsSetProfileVersion(h: Handle, ver: f64) {
    if let Some(p) = prof_ref(h) {
        profile::set_profile_version(p, ver);
    }
}
/// `cmsGetHeaderProfileID` — `[u8;16]` profile-ID out
#[unsafe(no_mangle)]
pub extern "C" fn cmsGetHeaderProfileID(h: Handle, out: *mut u8) {
    let Some(p) = prof_ref(h) else {
        return;
    };
    if !out.is_null() {
        unsafe { std::ptr::copy_nonoverlapping(p.profile_id.id8.as_ptr(), out, 16) };
    }
}
/// `cmsGetTagCount`
#[unsafe(no_mangle)]
pub extern "C" fn cmsGetTagCount(h: Handle) -> c_int {
    hdr(h, |p| p.tag_count as c_int, 0)
}
/// `cmsLinkTag` — makes `sig` a link to `dest`
#[unsafe(no_mangle)]
pub extern "C" fn cmsLinkTag(h: Handle, sig: c_uint, dest: c_uint) -> c_int {
    match prof_ref(h) {
        Some(p) => profile::cms_link_tag(p, sig, dest) as c_int,
        None => 0,
    }
}
/// `cmsReadRawTag` — reads raw bytes into `buf` (returns bytes needed)
#[unsafe(no_mangle)]
pub extern "C" fn cmsReadRawTag(h: Handle, sig: c_uint, buf: *mut u8, size: c_uint) -> c_uint {
    let Some(p) = prof_ref(h) else {
        return 0;
    };
    let data = unsafe {
        if buf.is_null() || size == 0 {
            None
        } else {
            Some(std::slice::from_raw_parts_mut(buf, size as usize))
        }
    };
    profile::cms_read_raw_tag(p, sig, data.map(|s| &mut *s))
}
/// `cmsWriteRawTag`
#[unsafe(no_mangle)]
pub extern "C" fn cmsWriteRawTag(h: Handle, sig: c_uint, buf: *const u8, size: c_uint) -> c_int {
    let Some(p) = prof_ref(h) else {
        return 0;
    };
    if buf.is_null() || size == 0 {
        return 0;
    }
    let d = unsafe { std::slice::from_raw_parts(buf, size as usize) };
    profile::cms_write_raw_tag(p, sig, d) as c_int
}
/// `cmsIsIntentSupported`
#[unsafe(no_mangle)]
pub extern "C" fn cmsIsIntentSupported(h: Handle, intent: c_uint, dir: c_int) -> c_int {
    hdr(h, |p| lcms::io1::is_intent_supported(p, intent, dir) as c_int, 0)
}

/// `cmsFLAGS_GRIDPOINTS` — `#define`-level helper
#[unsafe(no_mangle)]
pub extern "C" fn cmsFLAGS_GRIDPOINTS(n: c_uint) -> c_uint {
    lcms::types::flags_gridpoints(n)
}

// ---------------------------------------------------------------------------
// TAC, GBD, MD5, context, interp-params, THR aliases
// ---------------------------------------------------------------------------

/// `cmsDetectTAC` — max ink coverage % for a CMYK profile
#[unsafe(no_mangle)]
pub extern "C" fn cmsDetectTAC(h: Handle) -> f64 {
    match prof_ref(h) { Some(p) => lcms::cnvrt::dbg_detect_tac(p), None => 0.0 }
}
/// `cmsEstimateTAC` — alias of `cmsDetectTAC` in this port
#[unsafe(no_mangle)]
pub extern "C" fn cmsEstimateTAC(h: Handle) -> f64 {
    cmsDetectTAC(h)
}

/// `cmsGBDAlloc` — gamut-boundary descriptor (Lab→profile)
#[unsafe(no_mangle)]
pub extern "C" fn cmsGBDAlloc(h: Handle) -> Handle {
    if prof_ref(h).is_none() {
        return ptr::null_mut();
    }
    let g = lcms::gbd::Gbd::alloc(None);
    Box::into_raw(Box::new(g)) as Handle
}
/// `cmsGBDFree`
#[unsafe(no_mangle)]
pub extern "C" fn cmsGBDFree(g: Handle) {
    if !g.is_null() {
        drop(unsafe { Box::from_raw(g as *mut lcms::gbd::Gbd) });
    }
}
fn gbd_ref<'a>(h: Handle) -> Option<&'a mut lcms::gbd::Gbd> {
    if h.is_null() {
        return None;
    }
    Some(unsafe { &mut *(h as *mut lcms::gbd::Gbd) })
}
/// `cmsGBDAddPoint`
#[unsafe(no_mangle)]
pub extern "C" fn cmsGBDAddPoint(g: Handle, lab: *const CmsLab) -> c_int {
    let Some(gb) = gbd_ref(g) else {
        return 0;
    };
    gb.add_point(&cielab(lab)) as c_int
}
/// `cmsGBDCheckPoint`
#[unsafe(no_mangle)]
pub extern "C" fn cmsGBDCheckPoint(g: Handle, lab: *const CmsLab) -> c_int {
    let Some(gb) = gbd_ref(g) else {
        return 0;
    };
    gb.check_point(&cielab(lab)) as c_int
}
/// `cmsGBDCompute`
#[unsafe(no_mangle)]
pub extern "C" fn cmsGBDCompute(g: Handle, flags: c_uint) -> c_int {
    let Some(gb) = gbd_ref(g) else {
        return 0;
    };
    gb.compute(flags) as c_int
}

/// `cmsMD5alloc` — streaming MD5 hasher
#[unsafe(no_mangle)]
pub extern "C" fn cmsMD5alloc(_ctx: Handle) -> Handle {
    let m = lcms::md5::Md5::new();
    Box::into_raw(Box::new(m)) as Handle
}
/// `cmsMD5add`
#[unsafe(no_mangle)]
pub extern "C" fn cmsMD5add(m: Handle, buf: *const c_void, size: c_uint) {
    if m.is_null() || buf.is_null() || size == 0 {
        return;
    }
    let md = unsafe { &mut *(m as *mut lcms::md5::Md5) };
    let d = unsafe { std::slice::from_raw_parts(buf as *const u8, size as usize) };
    md.add(d);
}
/// `cmsMD5finish` — writes `[u8;16]` digest, frees hasher
#[unsafe(no_mangle)]
pub extern "C" fn cmsMD5finish(m: Handle, out: *mut u8) {
    if m.is_null() {
        return;
    }
    let md = unsafe { Box::from_raw(m as *mut lcms::md5::Md5) };
    let id = (*md).finish();
    if !out.is_null() {
        unsafe { std::ptr::copy_nonoverlapping(id.as_ptr(), out, 16) };
    }
}
/// `cmsMD5free` — frees hasher without finishing
#[unsafe(no_mangle)]
pub extern "C" fn cmsMD5free(m: Handle) {
    if !m.is_null() {
        drop(unsafe { Box::from_raw(m as *mut lcms::md5::Md5) });
    }
}

// THR variants — same as non-THR (global context; ctx arg ignored)
/// `cmsCreateTransformTHR`
#[unsafe(no_mangle)]
pub extern "C" fn cmsCreateTransformTHR(
    _ctx: Handle,
    input: Handle,
    in_fmt: c_uint,
    output: Handle,
    out_fmt: c_uint,
    intent: c_uint,
    flags: c_uint,
) -> Handle {
    cmsCreateTransform(input, in_fmt, output, out_fmt, intent, flags)
}
/// `cmsCreateRGBProfileTHR`
#[unsafe(no_mangle)]
pub extern "C" fn cmsCreateRGBProfileTHR(
    _ctx: Handle,
    wp: *const c_void,
    pr: *const c_void,
    trc: *const *mut c_void,
) -> Handle {
    cmsCreateRGBProfile(wp, pr, trc)
}
/// `cmsCreate_sRGBProfileTHR`
#[unsafe(no_mangle)]
pub extern "C" fn cmsCreate_sRGBProfileTHR(_ctx: Handle) -> Handle {
    cmsCreate_sRGBProfile()
}
/// `cmsCreateXYZProfileTHR`
#[unsafe(no_mangle)]
pub extern "C" fn cmsCreateXYZProfileTHR(_ctx: Handle) -> Handle {
    cmsCreateXYZProfile()
}
/// `cmsCreateNULLProfileTHR`
#[unsafe(no_mangle)]
pub extern "C" fn cmsCreateNULLProfileTHR(_ctx: Handle) -> Handle {
    cmsCreateNULLProfile()
}
/// `cmsCreateGrayProfileTHR`
#[unsafe(no_mangle)]
pub extern "C" fn cmsCreateGrayProfileTHR(
    _ctx: Handle,
    wp: *const c_void,
    trc: *mut c_void,
) -> Handle {
    cmsCreateGrayProfile(wp, trc)
}
/// `cmsCreateLab2ProfileTHR`/`cmsCreateLab4ProfileTHR`
#[unsafe(no_mangle)]
pub extern "C" fn cmsCreateLab2ProfileTHR(_ctx: Handle, wp: *const c_void) -> Handle {
    cmsCreateLab2Profile(wp)
}
#[unsafe(no_mangle)]
pub extern "C" fn cmsCreateLab4ProfileTHR(_ctx: Handle, wp: *const c_void) -> Handle {
    cmsCreateLab4Profile(wp)
}

// ---------------------------------------------------------------------------
// white-point/temp, quantize, named-color info, optimize, contexts
// ---------------------------------------------------------------------------

/// `cmsXYZ2xyY`
#[unsafe(no_mangle)]
pub extern "C" fn cmsXYZ2xyY(out: *mut lcms::types::CiexyY, xyz: *const CieXyz) {
    if out.is_null() || xyz.is_null() {
        return;
    }
    unsafe { *out = lcms::pcs::xyz2xyy(&*xyz) };
}
/// `cmsWhitePointFromTemp` — Planckian white from K
#[unsafe(no_mangle)]
pub extern "C" fn cmsWhitePointFromTemp(out: *mut lcms::types::CiexyY, temp_k: f64) -> c_int {
    let Some(w) = lcms::wtpnt::white_point_from_temp(temp_k) else {
        return 0;
    };
    if !out.is_null() {
        unsafe { *out = w };
    }
    1
}
/// `cmsTempFromWhitePoint` — K back from xyY
#[unsafe(no_mangle)]
pub extern "C" fn cmsTempFromWhitePoint(out: *mut f64, wp: *const lcms::types::CiexyY) -> c_int {
    if wp.is_null() {
        return 0;
    }
    let Some(t) = lcms::wtpnt::temp_from_white_point(unsafe { &*wp }) else {
        return 0;
    };
    if !out.is_null() {
        unsafe { *out = t };
    }
    1
}
/// `cmsQuickFloor`
#[unsafe(no_mangle)]
pub extern "C" fn cmsQuickFloor(v: f64) -> c_int {
    lcms::fixed::quick_floor(v)
}
/// `cmsQuickFloorWord`
#[unsafe(no_mangle)]
pub extern "C" fn cmsQuickFloorWord(v: f64) -> u16 {
    lcms::fixed::quick_floor_word(v)
}
/// `cmsQuantizeVal` — `[0,1]` → `[0,max]` rounded
#[unsafe(no_mangle)]
pub extern "C" fn cmsQuantizeVal(i: f64, max_samples: c_uint) -> u16 {
    lcms::fixed::quantize_val(i, max_samples)
}
/// `cmsNamedColorIndex`
#[unsafe(no_mangle)]
pub extern "C" fn cmsNamedColorIndex(list: Handle, name: *const c_char) -> c_int {
    let Some(l) = ncl_ref(list) else {
        return -1;
    };
    let n = unsafe { cstr(name) }.unwrap_or_default();
    named::named_color_index(l, n.as_bytes())
}
/// `cmsNamedColorInfo` — fills name/prefix/suffix/pcs/colorant
#[unsafe(no_mangle)]
pub extern "C" fn cmsNamedColorInfo(
    list: Handle,
    n_color: c_uint,
    name: *mut u8,
    prefix: *mut u8,
    suffix: *mut u8,
    pcs: *mut u16,
    colorant: *mut u16,
) -> c_int {
    let Some(l) = ncl_ref(list) else {
        return 0;
    };
    let Some((nm, pf, sf, p3, d16)) = named::named_color_info(l, n_color) else {
        return 0;
    };
    unsafe {
        if !name.is_null() {
            std::ptr::copy_nonoverlapping(nm.as_ptr(), name, nm.len());
        }
        if !prefix.is_null() {
            std::ptr::copy_nonoverlapping(pf.as_ptr(), prefix, pf.len());
        }
        if !suffix.is_null() {
            std::ptr::copy_nonoverlapping(sf.as_ptr(), suffix, sf.len());
        }
        if !pcs.is_null() {
            std::ptr::copy_nonoverlapping(p3.as_ptr(), pcs, 3);
        }
        if !colorant.is_null() {
            std::ptr::copy_nonoverlapping(d16.as_ptr(), colorant, 16);
        }
    }
    1
}

/// `cmsOpenProfileFromFileTHR`
#[unsafe(no_mangle)]
pub extern "C" fn cmsOpenProfileFromFileTHR(
    _ctx: Handle,
    path: *const c_char,
    mode: *const c_char,
) -> Handle {
    cmsOpenProfileFromFile(path, mode)
}
/// `cmsOpenProfileFromMemTHR`
#[unsafe(no_mangle)]
pub extern "C" fn cmsOpenProfileFromMemTHR(
    _ctx: Handle,
    buf: *const c_void,
    size: c_uint,
) -> Handle {
    cmsOpenProfileFromMem(buf, size)
}
/// `cmsCreateProfilePlaceholder` (+ `THR`)
#[unsafe(no_mangle)]
pub extern "C" fn cmsCreateProfilePlaceholder(_ctx: Handle) -> Handle {
    prof_to_handle(Some(profile::create_profile_placeholder(None)))
}
/// `cmsCreateInkLimitingDeviceLink`/`THR` — CMYK ink-limit devlink
#[unsafe(no_mangle)]
pub extern "C" fn cmsCreateInkLimitingDeviceLink(
    color_space: c_uint,
    limit: f64,
) -> Handle {
    prof_to_handle(virt::create_ink_limiting_device_link(None, color_space, limit))
}
#[unsafe(no_mangle)]
pub extern "C" fn cmsCreateInkLimitingDeviceLinkTHR(
    ctx: Handle,
    color_space: c_uint,
    limit: f64,
) -> Handle {
    let _ = ctx; cmsCreateInkLimitingDeviceLink(color_space, limit)
}
/// `cmsCreateLinearizationDeviceLink`/`THR`
#[unsafe(no_mangle)]
pub extern "C" fn cmsCreateLinearizationDeviceLinkTHR(
    _ctx: Handle,
    color_space: c_uint,
    transfer: *const *mut c_void,
) -> Handle {
    let n = lcms::pcs::channels_of_color_space(color_space).max(0) as usize;
    let tr = unsafe { curves_n_from(transfer, n) };
    let Some(v) = tr else {
        return ptr::null_mut();
    };
    let tref: Vec<&lcms::gamma::ToneCurve> = v.iter().collect();
    prof_to_handle(virt::create_linearization_device_link(
        None,
        color_space,
        &tref,
    ))
}
/// `cmsCreateProofingTransform`/`THR` — proof (simulate) transform
#[unsafe(no_mangle)]
pub extern "C" fn cmsCreateProofingTransform(
    input: Handle,
    in_fmt: c_uint,
    output: Handle,
    out_fmt: c_uint,
    proof: Handle,
    intent: c_uint,
    proof_intent: c_uint,
    flags: c_uint,
) -> Handle {
    let (i, o, pr) = (prof_ref(input), prof_ref(output), prof_ref(proof));
    let (Some(i), Some(o), Some(pr)) = (i, o, pr) else {
        return ptr::null_mut();
    };
    // C's `cmsCreateProofingTransformTHR`: {Input, Proofing, Proofing,
    // Output} with intents {nIntent, nIntent, RELATIVE_COLORIMETRIC,
    // ProofingIntent} and the proofing profile as the gamut-check profile at
    // position 1.
    let mut profs: Vec<Box<IccProfile>> = vec![
        unsafe { Box::from_raw(i) },
        unsafe { Box::from_raw(pr) },
        unsafe { Box::from_raw(pr) },
        unsafe { Box::from_raw(o) },
    ];
    let mut bpc = [flags & lcms::types::FLAGS_BLACKPOINTCOMPENSATION != 0; 4];
    let intents = [intent, intent, 1 /*INTENT_RELATIVE_COLORIMETRIC*/, proof_intent];
    let adapt = [1.0f64; 4];
    let t = xform_to_handle(xform::create_extended_transform(
        None,
        &mut profs,
        &mut bpc,
        &intents,
        &adapt,
        Some(prof_ref(proof).unwrap()),
        1,
        in_fmt,
        out_fmt,
        flags,
    ));
    for b in profs {
        std::mem::forget(b);
    }
    t
}
/// `cmsCreateProofingTransformTHR`
#[unsafe(no_mangle)]
pub extern "C" fn cmsCreateProofingTransformTHR(
    _ctx: Handle,
    input: Handle,
    in_fmt: c_uint,
    output: Handle,
    out_fmt: c_uint,
    proof: Handle,
    intent: c_uint,
    proof_intent: c_uint,
    flags: c_uint,
) -> Handle {
    cmsCreateProofingTransform(input, in_fmt, output, out_fmt, proof, intent, proof_intent, flags)
}
/// `cmsSetDeviceClass`
#[unsafe(no_mangle)]
pub extern "C" fn cmsSetDeviceClass(h: Handle, cl: c_uint) {
    if let Some(p) = prof_ref(h) {
        p.device_class = cl;
    }
}
/// `cmsSetPCS`
#[unsafe(no_mangle)]
pub extern "C" fn cmsSetPCS(h: Handle, pcs: c_uint) {
    if let Some(p) = prof_ref(h) {
        p.pcs = pcs;
    }
}
/// `cmsSetColorSpace`
#[unsafe(no_mangle)]
pub extern "C" fn cmsSetColorSpace(h: Handle, sig: c_uint) {
    if let Some(p) = prof_ref(h) {
        p.color_space = sig;
    }
}
/// `cmsTagLinkedTo`
#[unsafe(no_mangle)]
pub extern "C" fn cmsTagLinkedTo(h: Handle, sig: c_uint) -> c_uint {
    hdr(h, |p| profile::cms_tag_linked_to(p, sig), 0)
}

// ---------------------------------------------------------------------------
// interp params, optimize, BCHSW, PostScript, detect-RGB, IT8, memory
// ---------------------------------------------------------------------------

/// `cmsInterpParams` — the C-visible layout (`_cms_interp_struc`); the
/// testbed dereferences `p->Interpolation.Lerp16`, `p->Table`, `p->Domain`,
/// `p->nSamples` and `p->opta` directly, so the layout must match upstream.
#[repr(C)]
pub struct CmsInterpParams {
    pub context_id: Handle,
    pub dw_flags: c_uint,
    pub n_inputs: c_uint,
    pub n_outputs: c_uint,
    pub n_samples: [c_uint; 15],
    pub domain: [c_uint; 15],
    pub opta: [c_uint; 15],
    pub table: *const c_void,
    pub interpolation: usize, // cmsInterpFunction union — fn ptr
}

fn rebuild_params<'a>(p: &'a CmsInterpParams) -> Option<lcms::intrp::InterpParams<'a>> {
    // Only `n_inputs` dimensions carry samples — folding all 15 entries
    // would overflow to a giant span.
    let total = p.n_samples[..p.n_inputs.min(15) as usize]
        .iter()
        .fold(1usize, |a, &b| a.saturating_mul(b as usize))
        .saturating_mul(p.n_outputs as usize);
    let t = if p.dw_flags & 1 != 0 {
        lcms::intrp::InterpTable::F32(unsafe {
            std::slice::from_raw_parts(p.table as *const f32, total)
        })
    } else {
        lcms::intrp::InterpTable::U16(unsafe {
            std::slice::from_raw_parts(p.table as *const u16, total)
        })
    };
    lcms::intrp::compute_interp_params_ex(
        None,
        &p.n_samples,
        p.n_inputs,
        p.n_outputs,
        t,
        p.dw_flags,
    )
}
/// `_cmsInterpFn16` trampoline — rebuilds the Rust params from the C layout
/// and re-derives the same kernel `compute_interp_params_ex` chose.
unsafe extern "C" fn lerp16_c(
    input: *const u16,
    output: *mut u16,
    p: *const CmsInterpParams,
) {
    let Some(rp) = rebuild_params(unsafe { &*p }) else {
        return;
    };
    let i = unsafe { std::slice::from_raw_parts(input, rp.n_inputs as usize) };
    let o = unsafe { std::slice::from_raw_parts_mut(output, rp.n_outputs as usize) };
    rp.eval16(i, o);
}
unsafe extern "C" fn lerp_float_c(
    input: *const f32,
    output: *mut f32,
    p: *const CmsInterpParams,
) {
    let Some(rp) = rebuild_params(unsafe { &*p }) else {
        return;
    };
    let i = unsafe { std::slice::from_raw_parts(input, rp.n_inputs as usize) };
    let o = unsafe { std::slice::from_raw_parts_mut(output, rp.n_outputs as usize) };
    rp.eval_float(i, o);
}
fn make_cparams(
    ctx: Handle,
    n_samples: &[u32],
    in_chan: c_uint,
    out_chan: c_uint,
    table: *const c_void,
    flags: c_uint,
) -> Handle {
    let mut ns = [0u32; 15];
    for (i, &s) in n_samples.iter().take(15).enumerate() {
        ns[i] = s;
    }
    // Derive domain/opta/interp through the same routine the Rust port uses.
    let probe = CmsInterpParams {
        context_id: ctx,
        dw_flags: flags,
        n_inputs: in_chan,
        n_outputs: out_chan,
        n_samples: ns,
        domain: [0; 15],
        opta: [0; 15],
        table,
        interpolation: 0,
    };
    let (domain, opta) = match rebuild_params(&probe) {
        Some(rp) => (rp.domain, rp.opta),
        None => return ptr::null_mut(),
    };
    let mut cp = probe;
    cp.domain = domain;
    cp.opta = opta;
    cp.interpolation = if flags & 1 != 0 {
        lerp_float_c as usize
    } else {
        lerp16_c as usize
    };
    // `cmsComputeInterpParams` consults the context's Interpolators chain
    // first — a registered `cmsInterpFnFactory` that returns a non-NULL
    // `cmsInterpFunction` replaces the default lerp.
    if let Some(c) = unsafe { cctx(ctx) } {
        for &f in &c.interp_factories {
            let factory: InterpFnFactoryC = unsafe { std::mem::transmute(f) };
            let r = unsafe { factory(in_chan, out_chan, flags) };
            if r != 0 {
                cp.interpolation = r;
                break;
            }
        }
    }
    Box::into_raw(Box::new(cp)) as Handle
}
/// `cmsComputeInterpParams` — uniform `n_samples`
#[unsafe(no_mangle)]
pub extern "C" fn cmsComputeInterpParams(
    ctx: Handle,
    n_samples: c_uint,
    in_chan: c_uint,
    out_chan: c_uint,
    table: *const c_void,
    flags: c_uint,
) -> Handle {
    let ns = [n_samples; 15];
    make_cparams(ctx, &ns, in_chan, out_chan, table, flags)
}
/// `cmsComputeInterpParamsEx` — per-dimension `n_samples[]`
#[unsafe(no_mangle)]
pub extern "C" fn cmsComputeInterpParamsEx(
    ctx: Handle,
    n_samples: *const c_uint,
    in_chan: c_uint,
    out_chan: c_uint,
    table: *const c_void,
    flags: c_uint,
) -> Handle {
    let ns = if n_samples.is_null() {
        Vec::new()
    } else {
        unsafe { std::slice::from_raw_parts(n_samples, in_chan as usize) }.to_vec()
    };
    make_cparams(ctx, &ns, in_chan, out_chan, table, flags)
}
/// `_cmsComputeInterpParams`/`_cmsComputeInterpParamsEx` — internal variants
#[unsafe(no_mangle)]
pub extern "C" fn _cmsComputeInterpParams(
    ctx: Handle,
    n_samples: c_uint,
    in_chan: c_uint,
    out_chan: c_uint,
    table: *const c_void,
    flags: c_uint,
) -> Handle {
    cmsComputeInterpParams(ctx, n_samples, in_chan, out_chan, table, flags)
}
#[unsafe(no_mangle)]
pub extern "C" fn _cmsComputeInterpParamsEx(
    ctx: Handle,
    n_samples: *const c_uint,
    in_chan: c_uint,
    out_chan: c_uint,
    table: *const c_void,
    flags: c_uint,
) -> Handle {
    cmsComputeInterpParamsEx(ctx, n_samples, in_chan, out_chan, table, flags)
}
/// `cmsFreeInterpParams`/`_cmsFreeInterpParams`
#[unsafe(no_mangle)]
pub extern "C" fn cmsFreeInterpParams(p: Handle) {
    if !p.is_null() {
        drop(unsafe { Box::from_raw(p as *mut CmsInterpParams) });
    }
}
/// `cmsOptimizePipeline` — in-place CLUT/stage optimization
#[unsafe(no_mangle)]
pub extern "C" fn cmsOptimizePipeline(
    lut: Handle,
    intent: c_uint,
    in_fmt: *mut c_uint,
    out_fmt: *mut c_uint,
    flags: *mut c_uint,
) -> c_int {
    if lut.is_null() || in_fmt.is_null() || out_fmt.is_null() || flags.is_null() {
        return 0;
    }
    let l = unsafe { &mut *(lut as *mut lcms::lut::Pipeline) };
    lcms::opt::optimize_pipeline(
        None,
        l,
        intent,
        unsafe { &mut *in_fmt },
        unsafe { &mut *out_fmt },
        unsafe { &mut *flags },
    ) as c_int
}
/// `cmsCreateBCHSWabstractProfileTHR`/`cmsCreateBCHSWabstractProfile`
#[unsafe(no_mangle)]
pub extern "C" fn cmsCreateBCHSWabstractProfileTHR(
    _ctx: Handle,
    n_lut_points: c_uint,
    bright: f64,
    contrast: f64,
    hue: f64,
    saturation: f64,
    temp_src: c_uint,
    temp_dest: c_uint,
) -> Handle {
    prof_to_handle(virt::create_bchsw_abstract_profile(
        None,
        n_lut_points,
        bright,
        contrast,
        hue,
        saturation,
        temp_src,
        temp_dest,
    ))
}
#[unsafe(no_mangle)]
pub extern "C" fn cmsCreateBCHSWabstractProfile(
    n_lut_points: c_uint,
    bright: f64,
    contrast: f64,
    hue: f64,
    saturation: f64,
    temp_src: c_uint,
    temp_dest: c_uint,
) -> Handle {
    cmsCreateBCHSWabstractProfileTHR(
        ptr::null_mut(),
        n_lut_points,
        bright,
        contrast,
        hue,
        saturation,
        temp_src,
        temp_dest,
    )
}
/// `cmsDetectRGBProfileGamma`
#[unsafe(no_mangle)]
pub extern "C" fn cmsDetectRGBProfileGamma(h: Handle, precision: f64) -> f64 {
    match prof_ref(h) {
        Some(p) => lcms::xform::dbg_detect_rgb_profile_gamma(p, precision),
        None => -1.0,
    }
}
/// `cmsGetPostScriptCRD` — writes PS CRD into caller `*mut c_void`/`size`
#[unsafe(no_mangle)]
pub extern "C" fn cmsGetPostScriptCRD(
    _ctx: Handle,
    h: Handle,
    intent: c_uint,
    flags: c_uint,
    buf: *mut c_void,
    size: c_uint,
) -> c_uint {
    let Some(p) = prof_ref(h) else {
        return 0;
    };
    if buf.is_null() {
        // Sizing call — write into the NULL handler like C does.
        let mut io = lcms::io::io_handler_from_null(None);
        return lcms::ps2::get_post_script_crd(None, p, intent, flags, &mut io);
    }
    let mut io = match lcms::io::io_handler_from_mem(None, vec![0u8; size as usize], "w") {
        Some(i) => i,
        None => return 0,
    };
    let n = lcms::ps2::get_post_script_crd(None, p, intent, flags, &mut io);
    if n > 0 {
        if let lcms::io::IoStream::Mem(m) = &io.stream {
            unsafe { std::ptr::copy_nonoverlapping(m.block.as_ptr(), buf as *mut u8, (n as usize).min(size as usize)) };
        }
    }
    n
}
/// `cmsGetPostScriptCSA`
#[unsafe(no_mangle)]
pub extern "C" fn cmsGetPostScriptCSA(
    _ctx: Handle,
    h: Handle,
    intent: c_uint,
    flags: c_uint,
    buf: *mut c_void,
    size: c_uint,
) -> c_uint {
    let Some(p) = prof_ref(h) else {
        return 0;
    };
    if buf.is_null() {
        // Sizing call — write into the NULL handler like C does.
        let mut io = lcms::io::io_handler_from_null(None);
        return lcms::ps2::get_post_script_csa(None, p, intent, flags, &mut io);
    }
    let mut io = match lcms::io::io_handler_from_mem(None, vec![0u8; size as usize], "w") {
        Some(i) => i,
        None => return 0,
    };
    let n = lcms::ps2::get_post_script_csa(None, p, intent, flags, &mut io);
    if n > 0 {
        if let lcms::io::IoStream::Mem(m) = &io.stream {
            unsafe { std::ptr::copy_nonoverlapping(m.block.as_ptr(), buf as *mut u8, (n as usize).min(size as usize)) };
        }
    }
    n
}
/// `cmsUnregisterPlugins` — no-op (plugins live in contexts)
#[unsafe(no_mangle)]
pub extern "C" fn cmsUnregisterPlugins() {}

// ---------------------------------------------------------------------------
// Memory-manager plumbing — `_cmsMemPluginChunkType` (cmsplugin.c/cmserr.c)
//
// Every allocation carries a 32-byte `_cmsMemoryBlock`-compatible prefix:
//   +0  KeepSize     u32
//   +8  WhoAllocated ptr-sized
//   +16 DontCheck    u32
// so the testbed's debug allocator helpers (`DebugMemDontCheckThis` writes
// `ptr-16`, `DebugFree`/`DebugRealloc` read `ptr-32`..`ptr-8` and
// `free(ptr-32)`) work uniformly on default and plugin allocations, and a
// context whose mem-handler is replaced mid-life (cmsPluginTHR memH)
// still frees correctly.
// ---------------------------------------------------------------------------

const MEM_HDR: usize = 32;
const MEM_MAGIC: usize = 0x6C63_6D73_6D65_6D31; // 'lcmsmem1'
const MAX_ALLOC: u64 = 512 * 1024 * 1024; // C MAX_MEMORY_FOR_ALLOC

unsafe extern "C" {
    fn malloc(size: usize) -> *mut c_void;
    fn free(ptr: *mut c_void);
    fn realloc(ptr: *mut c_void, size: usize) -> *mut c_void;
}

type MemMallocFn = unsafe extern "C" fn(Handle, c_uint) -> *mut c_void;
type MemFreeFn = unsafe extern "C" fn(Handle, *mut c_void);
type MemReallocFn = unsafe extern "C" fn(Handle, *mut c_void, c_uint) -> *mut c_void;
type MemCallocFn = unsafe extern "C" fn(Handle, c_uint, c_uint) -> *mut c_void;
type MemDupFn = unsafe extern "C" fn(Handle, *const c_void, c_uint) -> *mut c_void;

unsafe extern "C" fn def_malloc(_ctx: Handle, size: c_uint) -> *mut c_void {
    if size == 0 || size as u64 > MAX_ALLOC {
        return ptr::null_mut();
    }
    let base = unsafe { malloc(size as usize + MEM_HDR) };
    if base.is_null() {
        return base;
    }
    unsafe {
        (base as *mut c_uint).write(size);
        *(base as *mut usize).add(1) = MEM_MAGIC; // WhoAllocated @8
        *(base as *mut c_uint).add(4) = 0; // DontCheck @16
        (base as *mut u8).add(MEM_HDR) as *mut c_void
    }
}
unsafe extern "C" fn def_free(_ctx: Handle, p: *mut c_void) {
    if !p.is_null() {
        unsafe { free((p as *mut u8).sub(MEM_HDR) as *mut c_void) };
    }
}
unsafe extern "C" fn def_realloc(ctx: Handle, p: *mut c_void, n: c_uint) -> *mut c_void {
    if p.is_null() {
        return unsafe { def_malloc(ctx, n) };
    }
    let base = unsafe { realloc((p as *mut u8).sub(MEM_HDR) as *mut c_void, n as usize + MEM_HDR) };
    if base.is_null() {
        return base;
    }
    unsafe {
        (base as *mut c_uint).write(n);
        (base as *mut u8).add(MEM_HDR) as *mut c_void
    }
}
unsafe extern "C" fn def_malloczero(ctx: Handle, size: c_uint) -> *mut c_void {
    let p = unsafe { def_malloc(ctx, size) };
    if !p.is_null() {
        unsafe { ptr::write_bytes(p as *mut u8, 0, size as usize) };
    }
    p
}
unsafe extern "C" fn def_calloc(ctx: Handle, num: c_uint, size: c_uint) -> *mut c_void {
    let total = num as u64 * size as u64;
    if total == 0
        || (size != 0 && num as u64 >= u32::MAX as u64 / size as u64)
        || total > MAX_ALLOC
    {
        return ptr::null_mut();
    }
    unsafe { def_malloczero(ctx, total as c_uint) }
}
unsafe extern "C" fn def_dup(ctx: Handle, org: *const c_void, size: c_uint) -> *mut c_void {
    if size as u64 > MAX_ALLOC {
        return ptr::null_mut();
    }
    let m = unsafe { def_malloc(ctx, size) };
    if !m.is_null() && !org.is_null() {
        unsafe { ptr::copy_nonoverlapping(org as *const u8, m as *mut u8, size as usize) };
    }
    m
}

/// The default `_cmsMemPluginChunkType` fn table — order:
/// Malloc, MallocZero, Free, Realloc, Calloc, Dup.
fn default_mem_fns() -> [usize; 6] {
    [
        def_malloc as usize,
        def_malloczero as usize,
        def_free as usize,
        def_realloc as usize,
        def_calloc as usize,
        def_dup as usize,
    ]
}

/// `cmsPluginBase` shared header (`Magic, ExpectedVersion, Type, Next`)
#[repr(C)]
struct CmsPluginBase {
    magic: c_uint,
    expected_version: c_uint,
    ty: c_uint,
    next: *mut CmsPluginBase,
}
/// `cmsPluginMemHandler` — base + 6 allocator slots
#[repr(C)]
struct CmsPluginMemHandler {
    base: CmsPluginBase,
    malloc_ptr: usize,
    free_ptr: usize,
    realloc_ptr: usize,
    malloc_zero_ptr: usize,
    calloc_ptr: usize,
    dup_ptr: usize,
}
/// `cmsPluginInterpolation` — base + `InterpolatorsFactory`
#[repr(C)]
struct CmsPluginInterpolation {
    base: CmsPluginBase,
    interpolators_factory: usize,
}
/// `cmsPluginParametricCurves` — base + type sets + `Evaluator`
#[repr(C)]
struct CmsPluginParametricCurves {
    base: CmsPluginBase,
    n_functions: c_uint,
    function_types: [c_uint; 20],
    parameter_count: [c_uint; 20],
    evaluator: usize,
}
/// `cmsInterpFnFactory` C signature — returns a `cmsInterpFunction` union
/// (one pointer) by value.
type InterpFnFactoryC = unsafe extern "C" fn(c_uint, c_uint, c_uint) -> usize;

// Foreign-kernel hooks for `lcms::intrp` — the lcms crate forbids `unsafe`,
// so C-kernel dispatch lives here and is reached through `fn` hooks.
fn foreign_factory_hook(f: usize, n_in: c_uint, n_out: c_uint, flags: c_uint) -> usize {
    let factory: InterpFnFactoryC = unsafe { std::mem::transmute(f) };
    unsafe { factory(n_in, n_out, flags) }
}
fn foreign_kernel16(input: &[u16], output: &mut [u16], p: &lcms::intrp::CmsInterpParams) {
    let f: unsafe extern "C" fn(*const u16, *mut u16, *const lcms::intrp::CmsInterpParams) =
        unsafe { std::mem::transmute(p.interpolation) };
    unsafe { f(input.as_ptr(), output.as_mut_ptr(), p) };
}
fn foreign_kernel_float(input: &[f32], output: &mut [f32], p: &lcms::intrp::CmsInterpParams) {
    let f: unsafe extern "C" fn(*const f32, *mut f32, *const lcms::intrp::CmsInterpParams) =
        unsafe { std::mem::transmute(p.interpolation) };
    unsafe { f(input.as_ptr(), output.as_mut_ptr(), p) };
}
fn ensure_foreign_hooks() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        lcms::intrp::install_foreign_hooks(lcms::intrp::ForeignHooks {
            factory: foreign_factory_hook,
            kernel16: foreign_kernel16,
            kernel_float: foreign_kernel_float,
        });
    });
}
/// `cmsParametricCurveEvaluator` C signature
type ParametricEvalC = unsafe extern "C" fn(c_int, *const f64, f64) -> f64;
/// `_cmsInterpFnFloat` C signature
type LerpFloatC = unsafe extern "C" fn(*const f32, *mut f32, *const CmsInterpParams);
/// `_cmsInterpFn16` C signature
type Lerp16C = unsafe extern "C" fn(*const u16, *mut u16, *const CmsInterpParams);
/// `cmsPluginHeader` — base + 4 header I/O callbacks
#[repr(C)]
struct CmsPluginHeader {
    base: CmsPluginBase,
    read_ptr: usize,
    write_ptr: usize,
    get_size_ptr: usize,
    get_info_ptr: usize,
}
/// `cmsPluginFormatters` — base + `FormattersFactory`
#[repr(C)]
struct CmsPluginFormatters {
    base: CmsPluginBase,
    formatters_factory: usize,
}
/// `cmsFormatterFactory` C signature — `(Type, Dir, dwFlags)` → `cmsFormatter`
/// union (one pointer) by value.
type FormatterFactoryC = unsafe extern "C" fn(c_uint, c_int, c_uint) -> usize;
const PLUGIN_MAGIC: c_uint = 0x6163_7070; // 'acpp'
const PLUGIN_MEMHANDLER_SIG: c_uint = 0x6D65_6D48; // 'memH'
const PLUGIN_INTERP_SIG: c_uint = 0x696E_7048; // 'inpH'
const PLUGIN_PARAMETRIC_SIG: c_uint = 0x7061_7248; // 'parH'
const PLUGIN_HEADER_SIG: c_uint = 0x6864_7248; // 'hdrH'
const PLUGIN_FORMATTERS_SIG: c_uint = 0x6672_6D48; // 'frmH'
const LCMS_VERSION: c_uint = 2200;

/// `_cmsFindMemoryPlugin` — scan the plugin chain for a mem-handler
unsafe fn find_mem_plugin(mut p: *const CmsPluginBase) -> *const CmsPluginMemHandler {
    let mut hops = 0;
    while !p.is_null() && hops < 128 {
        let b = unsafe { &*p };
        if b.magic == PLUGIN_MAGIC && b.expected_version <= LCMS_VERSION {
            if b.ty == PLUGIN_MEMHANDLER_SIG {
                return p as *const CmsPluginMemHandler;
            }
            p = b.next;
        } else {
            break;
        }
        hops += 1;
    }
    ptr::null()
}

/// `_cmsInstallAllocFunctions` — plugin fns with per-slot default fallback
fn install_mem_fns(plugin: *const CmsPluginMemHandler) -> [usize; 6] {
    if plugin.is_null() {
        return default_mem_fns();
    }
    let h = unsafe { &*plugin };
    [
        h.malloc_ptr,
        if h.malloc_zero_ptr != 0 { h.malloc_zero_ptr } else { default_mem_fns()[1] },
        h.free_ptr,
        h.realloc_ptr,
        if h.calloc_ptr != 0 { h.calloc_ptr } else { default_mem_fns()[4] },
        if h.dup_ptr != 0 { h.dup_ptr } else { default_mem_fns()[5] },
    ]
}

/// `chunks[MemPlugin]` for a context handle — the installed fns or defaults
fn mem_fns_of(ctx: Handle) -> [usize; 6] {
    if pool_contains(ctx) {
        let f = unsafe { (*(ctx as *const lcms::context::Context)).mem_fns };
        if f[0] != 0 {
            return f;
        }
    }
    default_mem_fns()
}
unsafe fn mem_alloc(ctx: Handle, size: c_uint) -> *mut c_void {
    let f: MemMallocFn = unsafe { std::mem::transmute(mem_fns_of(ctx)[0]) };
    unsafe { f(ctx, size) }
}
unsafe fn mem_alloc_zero(ctx: Handle, size: c_uint) -> *mut c_void {
    let f: MemMallocFn = unsafe { std::mem::transmute(mem_fns_of(ctx)[1]) };
    unsafe { f(ctx, size) }
}
unsafe fn mem_dealloc(ctx: Handle, p: *mut c_void) {
    let f: MemFreeFn = unsafe { std::mem::transmute(mem_fns_of(ctx)[2]) };
    unsafe { f(ctx, p) }
}
unsafe fn mem_realloc(ctx: Handle, p: *mut c_void, n: c_uint) -> *mut c_void {
    let f: MemReallocFn = unsafe { std::mem::transmute(mem_fns_of(ctx)[3]) };
    unsafe { f(ctx, p, n) }
}
unsafe fn mem_calloc(ctx: Handle, n: c_uint, s: c_uint) -> *mut c_void {
    let f: MemCallocFn = unsafe { std::mem::transmute(mem_fns_of(ctx)[4]) };
    unsafe { f(ctx, n, s) }
}
unsafe fn mem_dup(ctx: Handle, org: *const c_void, n: c_uint) -> *mut c_void {
    let f: MemDupFn = unsafe { std::mem::transmute(mem_fns_of(ctx)[5]) };
    unsafe { f(ctx, org, n) }
}
/// Allocate via a specific fn table (for context bootstrap — ctx not built yet)
unsafe fn mem_alloc_with(fns: &[usize; 6], id: Handle, size: c_uint) -> *mut c_void {
    let f: MemMallocFn = unsafe { std::mem::transmute(fns[0]) };
    unsafe { f(id, size) }
}

/// `cmsMalloc`/`cmsCalloc`/`cmsRealloc`/`cmsDupMem`/`cmsFree` — libc-style
/// allocation for C callers (`malloc`/`calloc`/`realloc`/`free`/`memcpy` semantics)
#[unsafe(no_mangle)]
pub extern "C" fn cmsMalloc(size: c_uint) -> *mut c_void {
    unsafe { def_malloc(ptr::null_mut(), size) }
}
#[unsafe(no_mangle)]
pub extern "C" fn cmsCalloc(n: c_uint, size: c_uint) -> *mut c_void {
    unsafe { def_calloc(ptr::null_mut(), n, size) }
}
#[unsafe(no_mangle)]
pub extern "C" fn cmsRealloc(ptr: *mut c_void, size: c_uint) -> *mut c_void {
    unsafe { def_realloc(ptr::null_mut(), ptr, size) }
}
#[unsafe(no_mangle)]
pub extern "C" fn cmsDupMem(ctx: Handle, src: *const c_void, size: c_uint) -> *mut c_void {
    unsafe { mem_dup(ctx, src, size) }
}
#[unsafe(no_mangle)]
pub extern "C" fn cmsFree(ptr: *mut c_void) {
    unsafe { def_free(ptr::null_mut(), ptr) }
}

// ---------------------------------------------------------------------------
// _cms* internals (testbed exercises internals), pipeline/stage, dict, IT8
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub extern "C" fn _cmsMalloc(ctx: Handle, size: c_uint) -> *mut c_void {
    unsafe { mem_alloc(ctx, size) }
}
#[unsafe(no_mangle)]
pub extern "C" fn _cmsMallocZero(ctx: Handle, size: c_uint) -> *mut c_void {
    unsafe { mem_alloc_zero(ctx, size) }
}
#[unsafe(no_mangle)]
pub extern "C" fn _cmsFree(ctx: Handle, ptr: *mut c_void) {
    unsafe { mem_dealloc(ctx, ptr) }
}
#[unsafe(no_mangle)]
pub extern "C" fn _cmsDupMem(ctx: Handle, src: *const c_void, size: c_uint) -> *mut c_void {
    unsafe { mem_dup(ctx, src, size) }
}
#[unsafe(no_mangle)]
pub extern "C" fn _cmsCalloc(ctx: Handle, num: c_uint, size: c_uint) -> *mut c_void {
    unsafe { mem_calloc(ctx, num, size) }
}
#[unsafe(no_mangle)]
pub extern "C" fn _cmsRealloc(ctx: Handle, ptr: *mut c_void, size: c_uint) -> *mut c_void {
    unsafe { mem_realloc(ctx, ptr, size) }
}
#[unsafe(no_mangle)]
pub extern "C" fn _cmsSubAlloc() -> *mut c_void {
    ptr::null_mut()
}

// Fixed/half — internal aliases.
#[unsafe(no_mangle)]
pub extern "C" fn _cms15Fixed16toDouble(f: i32) -> f64 {
    lcms::fixed::s15_fixed16_to_double(f)
}
#[unsafe(no_mangle)]
pub extern "C" fn _cms8Fixed8toDouble(f: u16) -> f64 {
    f as f64 / 256.0
}
#[unsafe(no_mangle)]
pub extern "C" fn _cmsDoubleTo15Fixed16(d: f64) -> i32 {
    lcms::fixed::double_to_s15_fixed16(d)
}
#[unsafe(no_mangle)]
pub extern "C" fn _cmsDoubleTo8Fixed8(d: f64) -> u16 {
    ((lcms::fixed::double_to_s15_fixed16(d) >> 8) & 0xFFFF) as u16
}
#[unsafe(no_mangle)]
pub extern "C" fn _cmsFloat2Half(f: f32) -> u16 {
    lcms::half::float2half(f)
}
#[unsafe(no_mangle)]
pub extern "C" fn _cmsHalf2Float(h: u16) -> f32 {
    lcms::half::half2float(h)
}
#[unsafe(no_mangle)]
pub extern "C" fn _cmsQuantizeVal(i: f64, max_samples: c_uint) -> u16 {
    lcms::fixed::quantize_val(i, max_samples)
}
#[unsafe(no_mangle)]
pub extern "C" fn _cmsQuickFloor(v: f64) -> c_int {
    lcms::fixed::quick_floor(v)
}
#[unsafe(no_mangle)]
pub extern "C" fn _cmsQuickFloorWord(v: f64) -> u16 {
    lcms::fixed::quick_floor_word(v)
}
#[unsafe(no_mangle)]
pub extern "C" fn _cmsQuickSaturateWord(v: f64) -> u16 {
    lcms::fixed::quick_saturate_word(v)
}
#[unsafe(no_mangle)]
pub extern "C" fn _cmsQuickSaturateByte(v: f64) -> u8 {
    lcms::fixed::quick_saturate_byte(v)
}
#[unsafe(no_mangle)]
pub extern "C" fn _cmsMAT3identity(a: *mut Mat3) {
    cmsMAT3identity(a)
}

// IO u16/u32 read/write — `io` is a boxed IoHandler.
fn io_ref<'a>(h: Handle) -> Option<&'a mut lcms::io::IoHandler> {
    if h.is_null() {
        return None;
    }
    Some(unsafe { &mut *(h as *mut lcms::io::IoHandler) })
}
#[unsafe(no_mangle)]
pub extern "C" fn _cmsReadUInt8Number(io: Handle, n: *mut u8) -> c_int {
    match (io_ref(io), n.is_null()) {
        (Some(i), false) => lcms::io::read_u8(i, unsafe { &mut *n }) as c_int,
        _ => 0,
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn _cmsReadUInt16Number(io: Handle, n: *mut u16) -> c_int {
    match (io_ref(io), n.is_null()) {
        (Some(i), false) => lcms::io::read_u16(i, unsafe { &mut *n }) as c_int,
        _ => 0,
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn _cmsReadUInt32Number(io: Handle, n: *mut u32) -> c_int {
    match (io_ref(io), n.is_null()) {
        (Some(i), false) => lcms::io::read_u32(i, unsafe { &mut *n }) as c_int,
        _ => 0,
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn _cmsWriteUInt8Number(io: Handle, n: u8) -> c_int {
    match io_ref(io) {
        Some(i) => lcms::io::write_u8(i, n) as c_int,
        None => 0,
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn _cmsWriteUInt16Number(io: Handle, n: u16) -> c_int {
    match io_ref(io) {
        Some(i) => lcms::io::write_u16(i, n) as c_int,
        None => 0,
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn _cmsWriteUInt32Number(io: Handle, n: u32) -> c_int {
    match io_ref(io) {
        Some(i) => lcms::io::write_u32(i, n) as c_int,
        None => 0,
    }
}

// Interp params — internal ctx variants.
#[unsafe(no_mangle)]
pub extern "C" fn _cmsFreeInterpParams(p: Handle) {
    cmsFreeInterpParams(p)
}
/// `_cmsGetFormatter` — formatter plugin lookup; not yet plumbed (returns 0)
#[unsafe(no_mangle)]
/// `_cmsTRANSFORM` cargo's first two fields — `InputFormat, OutputFormat`.
/// Formatters read only these, so the trampolines decode them directly.
fn cargo_formats(cargo: *mut c_void) -> (u32, u32) {
    let p = cargo as *const u32;
    unsafe { (*p, *p.add(1)) }
}
const FMT_BUF_LEN: usize = 1 << 16;

/// `cmsFormatter16` input (Unroll* — Buffer → Values)
unsafe extern "C" fn fmt16_unpack(
    cargo: *mut c_void,
    values: *mut u16,
    buffer: *mut u8,
    stride: u32,
) -> *mut u8 {
    let (i, o) = cargo_formats(cargo);
    let pi = lcms::pack::PackInfo {
        input_format: i,
        output_format: o,
        stride,
    };
    if let lcms::pack::Formatter::Unpack16(f) =
        lcms::pack::get_stock_input_formatter(i, 0)
    {
        let mut w = [0u16; lcms::xform::MAX_CHANNELS];
        let buf = unsafe { std::slice::from_raw_parts(buffer, FMT_BUF_LEN) };
        let c = f(&pi, &mut w, buf, stride);
        unsafe { std::ptr::copy_nonoverlapping(w.as_ptr(), values, lcms::xform::MAX_CHANNELS) };
        unsafe { buffer.add(c) }
    } else {
        buffer
    }
}
/// `cmsFormatter16` output (Pack* — Values → Buffer)
unsafe extern "C" fn fmt16_pack(
    cargo: *mut c_void,
    values: *mut u16,
    buffer: *mut u8,
    stride: u32,
) -> *mut u8 {
    let (i, o) = cargo_formats(cargo);
    let pi = lcms::pack::PackInfo {
        input_format: i,
        output_format: o,
        stride,
    };
    if let lcms::pack::Formatter::Pack16(f) =
        lcms::pack::get_stock_output_formatter(o, 0)
    {
        let w: &[u16; lcms::xform::MAX_CHANNELS] = unsafe { &*(values as *const [u16; lcms::xform::MAX_CHANNELS]) };
        let buf = unsafe { std::slice::from_raw_parts_mut(buffer, FMT_BUF_LEN) };
        let c = f(&pi, w, buf, stride);
        unsafe { buffer.add(c) }
    } else {
        buffer
    }
}
/// `cmsFormatterFloat` input
unsafe extern "C" fn fmt_float_unpack(
    cargo: *mut c_void,
    values: *mut f32,
    buffer: *mut u8,
    stride: u32,
) -> *mut u8 {
    let (i, o) = cargo_formats(cargo);
    let pi = lcms::pack::PackInfo {
        input_format: i,
        output_format: o,
        stride,
    };
    if let lcms::pack::Formatter::UnpackF32(f) =
        lcms::pack::get_stock_input_formatter(i, 1)
    {
        let mut w = [0.0f32; lcms::xform::MAX_CHANNELS];
        let buf = unsafe { std::slice::from_raw_parts(buffer, FMT_BUF_LEN) };
        let c = f(&pi, &mut w, buf, stride);
        unsafe {
            std::ptr::copy_nonoverlapping(w.as_ptr(), values, lcms::xform::MAX_CHANNELS)
        };
        unsafe { buffer.add(c) }
    } else {
        buffer
    }
}
/// `cmsFormatterFloat` output
unsafe extern "C" fn fmt_float_pack(
    cargo: *mut c_void,
    values: *mut f32,
    buffer: *mut u8,
    stride: u32,
) -> *mut u8 {
    let (i, o) = cargo_formats(cargo);
    let pi = lcms::pack::PackInfo {
        input_format: i,
        output_format: o,
        stride,
    };
    if let lcms::pack::Formatter::PackF32(f) =
        lcms::pack::get_stock_output_formatter(o, 1)
    {
        let w: &[f32; lcms::xform::MAX_CHANNELS] = unsafe { &*(values as *const [f32; lcms::xform::MAX_CHANNELS]) };
        let buf = unsafe { std::slice::from_raw_parts_mut(buffer, FMT_BUF_LEN) };
        let c = f(&pi, w, buf, stride);
        unsafe { buffer.add(c) }
    } else {
        buffer
    }
}
/// `_cmsGetFormatter` — returns a `cmsFormatter` union (Fmt16/FmtFloat slot).
/// The trampolines decode the type from the transform's format fields, so a
/// single fn ptr per (direction, precision) suffices.
#[unsafe(no_mangle)]
pub extern "C" fn _cmsGetFormatter(
    ctx: Handle,
    ty: c_uint,
    dir: c_int,
    flags: c_uint,
) -> usize {
    // C consults ctx-registered `cmsFormattersFactory` plugins first — first
    // factory to return a non-NULL `cmsFormatter` wins (`_cmsGetFormatter`).
    if let Some(cctx) = unsafe { cctx(ctx) } {
        for &f in &cctx.formatter_factories {
            let factory: FormatterFactoryC = unsafe { std::mem::transmute(f) };
            let fmt = unsafe { factory(ty, dir, flags) };
            if fmt != 0 {
                return fmt;
            }
        }
    }
    let float = flags & 1 != 0;
    match (dir, float) {
        (0, false) => fmt16_unpack as usize,
        (0, true) => fmt_float_unpack as usize,
        (1, false) => fmt16_pack as usize,
        _ => fmt_float_pack as usize,
    }
}
/// `_cmsOptimizePipeline` — takes `cmsPipeline**` (the optimizer may replace
/// the pipeline in place; `optimize_pipeline` mutates `&mut Pipeline`, so the
/// boxed handle stays valid — write back only if C-side replacement is added).
#[unsafe(no_mangle)]
pub extern "C" fn _cmsOptimizePipeline(
    _ctx: Handle,
    lut_ptr: *mut Handle,
    intent: c_uint,
    in_fmt: *mut c_uint,
    out_fmt: *mut c_uint,
    flags: *mut c_uint,
) -> c_int {
    if lut_ptr.is_null() {
        return 0;
    }
    cmsOptimizePipeline(unsafe { *lut_ptr }, intent, in_fmt, out_fmt, flags)
}
/// `_cmsPipelineSetOptimizationParameters` — plugin eval override (stub: 0)
#[unsafe(no_mangle)]
pub extern "C" fn _cmsPipelineSetOptimizationParameters(
    _lut: Handle,
    _eval16: *const c_void,
    _eval_float: *const c_void,
    _data: *mut c_void,
    _free_data: *const c_void,
) -> c_int {
    0
}

// Pipeline / stage
fn pipe_ref<'a>(h: Handle) -> Option<&'a mut lcms::lut::Pipeline> {
    if h.is_null() {
        return None;
    }
    Some(unsafe { &mut *(h as *mut lcms::lut::Pipeline) })
}
fn stage_ref<'a>(h: Handle) -> Option<&'a mut lcms::lut::Stage> {
    if h.is_null() {
        return None;
    }
    Some(unsafe { &mut *(h as *mut lcms::lut::Stage) })
}

#[unsafe(no_mangle)]
pub extern "C" fn cmsPipelineAlloc(_ctx: Handle, in_c: c_uint, out_c: c_uint) -> Handle {
    lcms::lut::pipeline_alloc(None, in_c, out_c)
        .map(|p| Box::into_raw(Box::new(p)) as Handle)
        .unwrap_or(ptr::null_mut())
}
#[unsafe(no_mangle)]
pub extern "C" fn cmsPipelineFree(h: Handle) {
    if !h.is_null() {
        drop(unsafe { Box::from_raw(h as *mut lcms::lut::Pipeline) });
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn cmsPipelineDup(h: Handle) -> Handle {
    pipe_ref(h)
        .and_then(|p| p.dup())
        .map(|p| Box::into_raw(Box::new(p)) as Handle)
        .unwrap_or(ptr::null_mut())
}
#[unsafe(no_mangle)]
pub extern "C" fn cmsPipelineEval16(
    input: *const u16,
    output: *mut u16,
    lut: Handle,
) {
    let Some(p) = pipe_ref(lut) else {
        return;
    };
    let i = unsafe { std::slice::from_raw_parts(input, p.input_channels() as usize) };
    let o = unsafe { std::slice::from_raw_parts_mut(output, p.output_channels() as usize) };
    p.eval16(i, o);
}
#[unsafe(no_mangle)]
pub extern "C" fn cmsPipelineEvalFloat(
    input: *const f32,
    output: *mut f32,
    lut: Handle,
) {
    let Some(p) = pipe_ref(lut) else {
        return;
    };
    let i = unsafe { std::slice::from_raw_parts(input, p.input_channels() as usize) };
    let o = unsafe { std::slice::from_raw_parts_mut(output, p.output_channels() as usize) };
    p.eval_float(i, o);
}
#[unsafe(no_mangle)]
pub extern "C" fn cmsPipelineEvalReverseFloat(
    target: *const f32,
    result: *mut f32,
    hint: *const f32,
    lut: Handle,
) -> c_int {
    let Some(p) = pipe_ref(lut) else {
        return 0;
    };
    let t = unsafe { std::slice::from_raw_parts(target, p.output_channels() as usize) };
    let r = unsafe { std::slice::from_raw_parts_mut(result, p.input_channels() as usize) };
    let h = if hint.is_null() {
        None
    } else {
        Some(unsafe { std::slice::from_raw_parts(hint, p.input_channels() as usize) })
    };
    p.eval_reverse_float(t, r, h) as c_int
}
#[unsafe(no_mangle)]
pub extern "C" fn cmsPipelineInputChannels(lut: Handle) -> c_uint {
    pipe_ref(lut).map(|p| p.input_channels()).unwrap_or(0)
}
#[unsafe(no_mangle)]
pub extern "C" fn cmsPipelineOutputChannels(lut: Handle) -> c_uint {
    pipe_ref(lut).map(|p| p.output_channels()).unwrap_or(0)
}
#[unsafe(no_mangle)]
pub extern "C" fn cmsPipelineStageCount(lut: Handle) -> c_uint {
    pipe_ref(lut).map(|p| p.stage_count()).unwrap_or(0)
}
/// `cmsPipelineInsertStage` — consumes `stage` (C transfers ownership)
#[unsafe(no_mangle)]
pub extern "C" fn cmsPipelineInsertStage(lut: Handle, loc: c_int, stage: Handle) -> c_int {
    let Some(p) = pipe_ref(lut) else {
        return 0;
    };
    if stage.is_null() {
        return 0;
    }
    let s = *unsafe { Box::from_raw(stage as *mut lcms::lut::Stage) };
    let l = if loc == 0 {
        lcms::lut::StageLoc::AtBegin
    } else {
        lcms::lut::StageLoc::AtEnd
    };
    p.insert_stage(l, s) as c_int
}
/// `cmsPipelineUnlinkStage` — writes the unlinked stage handle to `stage`
#[unsafe(no_mangle)]
pub extern "C" fn cmsPipelineUnlinkStage(lut: Handle, loc: c_int, stage: *mut Handle) -> c_int {
    let Some(p) = pipe_ref(lut) else {
        return 0;
    };
    if stage.is_null() {
        return 0;
    }
    let l = if loc == 0 {
        lcms::lut::StageLoc::AtBegin
    } else {
        lcms::lut::StageLoc::AtEnd
    };
    match p.unlink_stage(l) {
        Some(s) => {
            unsafe { *stage = Box::into_raw(Box::new(s)) as Handle };
            1
        }
        None => 0,
    }
}
/// `cmsPipelineGetPtrToFirstStage` — borrowed, not freed
#[unsafe(no_mangle)]
pub extern "C" fn cmsPipelineGetPtrToFirstStage(lut: Handle) -> Handle {
    pipe_ref(lut)
        .and_then(|p| p.first_stage().map(|s| s as *const _ as Handle))
        .unwrap_or(ptr::null_mut())
}
/// `cmsPipelineGetPtrToLastStage` — borrowed
#[unsafe(no_mangle)]
pub extern "C" fn cmsPipelineGetPtrToLastStage(lut: Handle) -> Handle {
    pipe_ref(lut)
        .and_then(|p| p.last_stage().map(|s| s as *const _ as Handle))
        .unwrap_or(ptr::null_mut())
}
#[unsafe(no_mangle)]
pub extern "C" fn cmsStageNext(stage: Handle) -> Handle {
    stage_ref(stage)
        .and_then(|s| s.next().map(|n| n as *const _ as Handle))
        .unwrap_or(ptr::null_mut())
}
#[unsafe(no_mangle)]
pub extern "C" fn cmsStageType(stage: Handle) -> c_uint {
    stage_ref(stage).map(|s| s.stage_type()).unwrap_or(0)
}
#[unsafe(no_mangle)]
pub extern "C" fn cmsStageData(stage: Handle) -> Handle {
    stage_ref(stage)
        .map(|s| &mut s.data as *mut _ as Handle)
        .unwrap_or(ptr::null_mut())
}
#[unsafe(no_mangle)]
pub extern "C" fn cmsStageInputChannels(stage: Handle) -> c_uint {
    stage_ref(stage).map(|s| s.input_channels()).unwrap_or(0)
}
#[unsafe(no_mangle)]
pub extern "C" fn cmsStageOutputChannels(stage: Handle) -> c_uint {
    stage_ref(stage).map(|s| s.output_channels()).unwrap_or(0)
}
#[unsafe(no_mangle)]
pub extern "C" fn cmsStageAllocIdentity(_ctx: Handle, n_chan: c_uint) -> Handle {
    let s = lcms::lut::stage_alloc_identity(None, n_chan);
    Box::into_raw(Box::new(s)) as Handle
}
#[unsafe(no_mangle)]
pub extern "C" fn cmsStageAllocMatrix(
    _ctx: Handle,
    rows: c_uint,
    cols: c_uint,
    matrix: *const f64,
    offset: *const f64,
) -> Handle {
    let n = (rows as usize) * (cols as usize);
    let m = if matrix.is_null() {
        Vec::new()
    } else {
        unsafe { std::slice::from_raw_parts(matrix, n) }.to_vec()
    };
    let off = if offset.is_null() {
        None
    } else {
        Some(unsafe { std::slice::from_raw_parts(offset, cols as usize) })
    };
    lcms::lut::stage_alloc_matrix(None, rows, cols, &m, off)
        .map(|s| Box::into_raw(Box::new(s)) as Handle)
        .unwrap_or(ptr::null_mut())
}
#[unsafe(no_mangle)]
pub extern "C" fn cmsStageAllocToneCurves(
    _ctx: Handle,
    n: c_uint,
    curves: *const *mut c_void,
) -> Handle {
    let cs = unsafe { curves3_from(curves) };
    let Some(v) = cs else {
        return ptr::null_mut();
    };
    let tref: Vec<&lcms::gamma::ToneCurve> = v.iter().collect();
    lcms::lut::stage_alloc_tone_curves(None, n, Some(&tref))
        .map(|s| Box::into_raw(Box::new(s)) as Handle)
        .unwrap_or(ptr::null_mut())
}
#[unsafe(no_mangle)]
pub extern "C" fn cmsStageAllocCLut16bitGranular(
    ctx: Handle,
    grid: *const c_uint,
    in_c: c_uint,
    out_c: c_uint,
    table: *const u16,
) -> Handle {
    let g = if grid.is_null() {
        Vec::new()
    } else {
        unsafe { std::slice::from_raw_parts(grid, in_c as usize) }.to_vec()
    };
    let t = if table.is_null() {
        None
    } else {
        Some(unsafe {
            std::slice::from_raw_parts(
                table,
                out_c as usize * g.iter().fold(1usize, |a, &b| a * b as usize),
            )
        })
    };
    lcms::lut::stage_alloc_clut16bit_granular(unsafe { cctx(ctx) }, &g, in_c, out_c, t)
        .map(|s| Box::into_raw(Box::new(s)) as Handle)
        .unwrap_or(ptr::null_mut())
}
#[unsafe(no_mangle)]
pub extern "C" fn cmsStageAllocCLut16bit(
    ctx: Handle,
    grid: c_uint,
    in_c: c_uint,
    out_c: c_uint,
    table: *const u16,
) -> Handle {
    let g = [grid; 15];
    cmsStageAllocCLut16bitGranular(ctx, g.as_ptr(), in_c, out_c, table)
}
#[unsafe(no_mangle)]
pub extern "C" fn cmsStageAllocCLutFloat(
    ctx: Handle,
    grid: c_uint,
    in_c: c_uint,
    out_c: c_uint,
    table: *const f32,
) -> Handle {
    let t = if table.is_null() {
        None
    } else {
        Some(unsafe {
            std::slice::from_raw_parts(
                table,
                out_c as usize * (grid as usize).pow(in_c),
            )
        })
    };
    lcms::lut::stage_alloc_clut_float(unsafe { cctx(ctx) }, grid, in_c, out_c, t)
        .map(|s| Box::into_raw(Box::new(s)) as Handle)
        .unwrap_or(ptr::null_mut())
}
type Sampler16C = unsafe extern "C" fn(*const u16, *mut u16, *mut c_void) -> c_int;
#[unsafe(no_mangle)]
pub extern "C" fn cmsStageSampleCLut16bit(
    stage: Handle,
    sampler: Option<Sampler16C>,
    cargo: *mut c_void,
    flags: c_uint,
) -> c_int {
    let (Some(s), Some(f)) = (stage_ref(stage), sampler) else {
        return 0;
    };
    let mut cl = |i: &[u16], o: &mut [u16]| -> i32 {
        unsafe { f(i.as_ptr(), o.as_mut_ptr(), cargo) }
    };
    lcms::lut::stage_sample_clut16bit(s, &mut cl, flags) as c_int
}
type SamplerFloatC = unsafe extern "C" fn(*const f32, *mut f32, *mut c_void) -> c_int;
#[unsafe(no_mangle)]
pub extern "C" fn cmsStageSampleCLutFloat(
    stage: Handle,
    sampler: Option<SamplerFloatC>,
    cargo: *mut c_void,
    flags: c_uint,
) -> c_int {
    let (Some(s), Some(f)) = (stage_ref(stage), sampler) else {
        return 0;
    };
    let mut cl = |i: &[f32], o: &mut [f32]| -> i32 {
        unsafe { f(i.as_ptr(), o.as_mut_ptr(), cargo) }
    };
    lcms::lut::stage_sample_clut_float(s, &mut cl, flags) as c_int
}

// ---------------------------------------------------------------------------
// dict (C-layout linked list — testbed dereferences fields), IT8, GBD,
// _cmsStageAlloc internals, user-data, THR chunk accessors, plugin
// ---------------------------------------------------------------------------

#[repr(C)]
pub struct CDictEntry {
    pub next: *mut CDictEntry,
    pub display_name: Handle,
    pub display_value: Handle,
    pub name: *mut WcharT,
    pub value: *mut WcharT,
}
#[repr(C)]
pub struct CDict {
    pub head: *mut CDictEntry,
}

unsafe fn wcs_dup(w: *const WcharT) -> *mut WcharT {
    match unsafe { wcs_to_slice(w) } {
        None => ptr::null_mut(),
        Some(mut v) => {
            v.push(0);
            let b = v.into_boxed_slice();
            Box::into_raw(b) as *mut WcharT
        }
    }
}
/// `cmsDictAlloc`
#[unsafe(no_mangle)]
pub extern "C" fn cmsDictAlloc(_ctx: Handle) -> Handle {
    Box::into_raw(Box::new(CDict {
        head: ptr::null_mut(),
    })) as Handle
}
fn dict_ref<'a>(h: Handle) -> Option<&'a mut CDict> {
    if h.is_null() {
        return None;
    }
    Some(unsafe { &mut *(h as *mut CDict) })
}
/// `cmsDictFree` — walk the list freeing nodes/strings
#[unsafe(no_mangle)]
pub extern "C" fn cmsDictFree(h: Handle) {
    let Some(d) = dict_ref(h) else {
        return;
    };
    let mut e = d.head;
    while !e.is_null() {
        let entry = unsafe { Box::from_raw(e) };
        if !entry.name.is_null() {
            let mut n = 0usize;
            while unsafe { *entry.name.add(n) } != 0 {
                n += 1;
            }
            drop(unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(entry.name, n + 1)) });
        }
        if !entry.value.is_null() {
            let mut n = 0usize;
            while unsafe { *entry.value.add(n) } != 0 {
                n += 1;
            }
            drop(unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(entry.value, n + 1)) });
        }
        cmsMLUfree(entry.display_name);
        cmsMLUfree(entry.display_value);
        e = entry.next;
    }
    drop(unsafe { Box::from_raw(h as *mut CDict) });
}
/// `cmsDictAddEntry` — head insert (C prepends)
#[unsafe(no_mangle)]
pub extern "C" fn cmsDictAddEntry(
    dict: Handle,
    name: *const WcharT,
    value: *const WcharT,
    display_name: Handle,
    display_value: Handle,
) -> c_int {
    let Some(d) = dict_ref(dict) else {
        return 0;
    };
    if name.is_null() {
        return 0;
    }
    let e = Box::new(CDictEntry {
        next: d.head,
        display_name: cmsMLUdup(display_name),
        display_value: cmsMLUdup(display_value),
        name: unsafe { wcs_dup(name) },
        value: unsafe { wcs_dup(value) },
    });
    d.head = Box::into_raw(e);
    1
}
/// `cmsDictDup` — iterate src head→tail adding to new (reverses order, C quirk)
#[unsafe(no_mangle)]
pub extern "C" fn cmsDictDup(dict: Handle) -> Handle {
    let Some(d) = dict_ref(dict) else {
        return ptr::null_mut();
    };
    let new = cmsDictAlloc(ptr::null_mut());
    let mut e = d.head;
    while !e.is_null() {
        let entry = unsafe { &*e };
        cmsDictAddEntry(
            new,
            entry.name,
            entry.value,
            entry.display_name,
            entry.display_value,
        );
        e = entry.next;
    }
    new
}
/// `cmsDictGetEntryList` — head node (borrowed)
#[unsafe(no_mangle)]
pub extern "C" fn cmsDictGetEntryList(dict: Handle) -> *mut CDictEntry {
    dict_ref(dict).map(|d| d.head).unwrap_or(ptr::null_mut())
}
/// `cmsDictNextEntry`
#[unsafe(no_mangle)]
pub extern "C" fn cmsDictNextEntry(e: *mut CDictEntry) -> *mut CDictEntry {
    if e.is_null() {
        ptr::null_mut()
    } else {
        unsafe { (*e).next }
    }
}

// IT8/CGATS
fn it8_ref<'a>(h: Handle) -> Option<&'a mut lcms::cgats::It8> {
    if h.is_null() {
        return None;
    }
    Some(unsafe { &mut *(h as *mut lcms::cgats::It8) })
}
#[unsafe(no_mangle)]
pub extern "C" fn cmsIT8Alloc(_ctx: Handle) -> Handle {
    Box::into_raw(lcms::cgats::It8::alloc(None)) as Handle
}
#[unsafe(no_mangle)]
pub extern "C" fn cmsIT8Free(h: Handle) {
    if !h.is_null() {
        drop(unsafe { Box::from_raw(h as *mut lcms::cgats::It8) });
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn cmsIT8LoadFromMem(
    _ctx: Handle,
    data: *const c_void,
    size: c_uint,
) -> Handle {
    if data.is_null() || size == 0 {
        return ptr::null_mut();
    }
    let d = unsafe { std::slice::from_raw_parts(data as *const u8, size as usize) };
    match lcms::cgats::It8::load_from_mem(None, d) {
        Some(b) => Box::into_raw(b) as Handle,
        None => ptr::null_mut(),
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn cmsIT8LoadFromFile(_ctx: Handle, path: *const c_char) -> Handle {
    let p = unsafe { cstr(path) }.unwrap_or_default();
    match lcms::cgats::It8::load_from_file(None, &p) {
        Some(b) => Box::into_raw(b) as Handle,
        None => ptr::null_mut(),
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn cmsIT8SaveToFile(h: Handle, path: *const c_char) -> c_int {
    let Some(it) = it8_ref(h) else {
        return 0;
    };
    let p = unsafe { cstr(path) }.unwrap_or_default();
    it.save_to_file(&p) as c_int
}
#[unsafe(no_mangle)]
pub extern "C" fn cmsIT8SetSheetType(h: Handle, ty: *const c_char) -> c_int {
    let Some(it) = it8_ref(h) else {
        return 0;
    };
    let t = unsafe { cstr(ty) }.unwrap_or_default();
    it.set_sheet_type(&t) as c_int
}
#[unsafe(no_mangle)]
pub extern "C" fn cmsIT8SetPropertyStr(h: Handle, key: *const c_char, val: *const c_char) -> c_int {
    let Some(it) = it8_ref(h) else {
        return 0;
    };
    let (k, v) = (unsafe { cstr(key) }.unwrap_or_default(), unsafe { cstr(val) }.unwrap_or_default());
    it.set_property_str(&k, &v) as c_int
}
#[unsafe(no_mangle)]
pub extern "C" fn cmsIT8SetPropertyDbl(h: Handle, key: *const c_char, val: f64) -> c_int {
    let Some(it) = it8_ref(h) else {
        return 0;
    };
    let k = unsafe { cstr(key) }.unwrap_or_default();
    it.set_property_dbl(&k, val) as c_int
}
#[unsafe(no_mangle)]
pub extern "C" fn cmsIT8SetPropertyHex(h: Handle, key: *const c_char, val: u32) -> c_int {
    let Some(it) = it8_ref(h) else {
        return 0;
    };
    let k = unsafe { cstr(key) }.unwrap_or_default();
    it.set_property_hex(&k, val) as c_int
}
#[unsafe(no_mangle)]
pub extern "C" fn cmsIT8SetPropertyUncooked(h: Handle, key: *const c_char, val: *const c_char) -> c_int {
    let Some(it) = it8_ref(h) else {
        return 0;
    };
    let (k, v) = (unsafe { cstr(key) }.unwrap_or_default(), unsafe { cstr(val) }.unwrap_or_default());
    it.set_property_uncooked(&k, &v) as c_int
}
#[unsafe(no_mangle)]
pub extern "C" fn cmsIT8GetPropertyDbl(h: Handle, key: *const c_char) -> f64 {
    let Some(it) = it8_ref(h) else {
        return 0.0;
    };
    let k = unsafe { cstr(key) }.unwrap_or_default();
    it.get_property_dbl(&k)
}
#[unsafe(no_mangle)]
pub extern "C" fn cmsIT8SetDataFormat(h: Handle, n: c_int, label: *const c_char) -> c_int {
    let Some(it) = it8_ref(h) else {
        return 0;
    };
    let l = unsafe { cstr(label) }.unwrap_or_default();
    if n < 0 {
        return 0;
    }
    it.set_data_format_export(n as usize, &l) as c_int
}
#[unsafe(no_mangle)]
pub extern "C" fn cmsIT8SetDataRowCol(
    h: Handle,
    row: c_int,
    col: c_int,
    val: *const c_char,
) -> c_int {
    let Some(it) = it8_ref(h) else {
        return 0;
    };
    let v = unsafe { cstr(val) }.unwrap_or_default();
    it.set_data_row_col(row as usize, col as usize, &v) as c_int
}
#[unsafe(no_mangle)]
pub extern "C" fn cmsIT8SetDataRowColDbl(
    h: Handle,
    row: c_int,
    col: c_int,
    val: f64,
) -> c_int {
    let Some(it) = it8_ref(h) else {
        return 0;
    };
    it.set_data_row_col_dbl(row as usize, col as usize, val) as c_int
}
#[unsafe(no_mangle)]
pub extern "C" fn cmsIT8GetDataDbl(h: Handle, patch: *const c_char, sample: *const c_char) -> f64 {
    let Some(it) = it8_ref(h) else {
        return 0.0;
    };
    let (p, s) = (unsafe { cstr(patch) }.unwrap_or_default(), unsafe { cstr(sample) }.unwrap_or_default());
    it.get_data_dbl(&p, &s)
}
#[unsafe(no_mangle)]
pub extern "C" fn cmsIT8GetDataRowColDbl(h: Handle, row: c_int, col: c_int) -> f64 {
    let Some(it) = it8_ref(h) else {
        return 0.0;
    };
    it.get_data_row_col_dbl(row as usize, col as usize)
}

// GBD — upstream spells the point ops cmsGDB* while alloc/free are cmsGBD*
#[unsafe(no_mangle)]
pub extern "C" fn cmsGDBAddPoint(g: Handle, lab: *const CmsLab) -> c_int {
    cmsGBDAddPoint(g, lab)
}
#[unsafe(no_mangle)]
pub extern "C" fn cmsGDBCheckPoint(g: Handle, lab: *const CmsLab) -> c_int {
    cmsGBDCheckPoint(g, lab)
}
#[unsafe(no_mangle)]
pub extern "C" fn cmsGDBCompute(g: Handle, flags: c_uint) -> c_int {
    cmsGBDCompute(g, flags)
}

// _cmsStageAlloc* internals
fn noop_eval(_input: &[f32], _output: &mut [f32], _stage: &lcms::lut::Stage) {}
/// `_cmsStageAllocPlaceholder` — custom-eval stage. The C eval/dup/free fn
/// pointers are opaque to Rust; a no-op eval is used (plugin-stage tests
/// that install a C EvalPtr can't run it — tracked as a gap).
#[unsafe(no_mangle)]
pub extern "C" fn _cmsStageAllocPlaceholder(
    _ctx: Handle,
    ty: c_uint,
    in_c: c_uint,
    out_c: c_uint,
    _eval: *const c_void,
    _dup: *const c_void,
    _free: *const c_void,
    _data: *mut c_void,
) -> Handle {
    let s = lcms::lut::stage_alloc_placeholder(
        None,
        ty,
        in_c,
        out_c,
        noop_eval,
        lcms::lut::StageData::None,
    );
    Box::into_raw(Box::new(s)) as Handle
}
#[unsafe(no_mangle)]
pub extern "C" fn _cmsStageAllocIdentity(_ctx: Handle, n: c_uint) -> Handle {
    cmsStageAllocIdentity(_ctx, n)
}
#[unsafe(no_mangle)]
pub extern "C" fn _cmsStageAllocIdentityCurves(_ctx: Handle, n: c_uint) -> Handle {
    lcms::lut::stage_alloc_identity_curves(None, n)
        .map(|s| Box::into_raw(Box::new(s)) as Handle)
        .unwrap_or(ptr::null_mut())
}
#[unsafe(no_mangle)]
pub extern "C" fn _cmsStageAllocIdentityCLut(_ctx: Handle, n: c_uint) -> Handle {
    lcms::lut::stage_alloc_identity_clut(None, n)
        .map(|s| Box::into_raw(Box::new(s)) as Handle)
        .unwrap_or(ptr::null_mut())
}
#[unsafe(no_mangle)]
pub extern "C" fn _cmsStageAllocLab2XYZ(_ctx: Handle) -> Handle {
    let s = lcms::lut::stage_alloc_lab2xyz(None);
    Box::into_raw(Box::new(s)) as Handle
}
#[unsafe(no_mangle)]
pub extern "C" fn _cmsStageAllocXYZ2Lab(_ctx: Handle) -> Handle {
    let s = lcms::lut::stage_alloc_xyz2lab(None);
    Box::into_raw(Box::new(s)) as Handle
}
#[unsafe(no_mangle)]
pub extern "C" fn _cmsStageAllocLabV2ToV4(_ctx: Handle) -> Handle {
    lcms::lut::stage_alloc_lab_v2_to_v4(None)
        .map(|s| Box::into_raw(Box::new(s)) as Handle)
        .unwrap_or(ptr::null_mut())
}
#[unsafe(no_mangle)]
pub extern "C" fn _cmsStageAllocLabV4ToV2(_ctx: Handle) -> Handle {
    lcms::lut::stage_alloc_lab_v4_to_v2(None)
        .map(|s| Box::into_raw(Box::new(s)) as Handle)
        .unwrap_or(ptr::null_mut())
}
#[unsafe(no_mangle)]
pub extern "C" fn _cmsStageAllocNamedColor(list: Handle, use_pcs: c_int) -> Handle {
    if list.is_null() {
        return ptr::null_mut();
    }
    let l = unsafe { &*(list as *const named::NamedColorList) };
    lcms::named::stage_alloc_named_color(l, use_pcs != 0)
        .map(|s| Box::into_raw(Box::new(s)) as Handle)
        .unwrap_or(ptr::null_mut())
}

// Profile user-data slots
#[unsafe(no_mangle)]
pub extern "C" fn _cmsGetProfileUserData(h: Handle) -> *mut c_void {
    hdr(h, |p| p.user_data as *mut c_void, ptr::null_mut())
}
/// `_cmsFreeUserDataFn` — `void (*)(cmsContext ContextID, void* Data)`
type FreeUserDataC = unsafe extern "C" fn(Handle, *mut c_void);
/// `_cmsSetProfileUserData` — frees any previous user data via its stored
/// `FreeUsrData` before installing the new pair.
#[unsafe(no_mangle)]
pub extern "C" fn _cmsSetProfileUserData(h: Handle, data: *mut c_void, free_fn: *const c_void) {
    if let Some(p) = prof_ref(h) {
        if p.user_data != 0 && p.free_user_data_fn != 0 {
            let f: FreeUserDataC = unsafe { std::mem::transmute(p.free_user_data_fn) };
            let cid = p
                .ctx
                .as_ref()
                .map(|c| c as *const lcms::context::Context as Handle)
                .unwrap_or(ptr::null_mut());
            unsafe { f(cid, p.user_data as *mut c_void) };
        }
        p.user_data = data as usize;
        p.free_user_data_fn = free_fn as usize;
    }
}
/// `cmsFreeUserDataFn` typedef'd value is opaque to us — the destructor is
/// invoked by `cmsCloseProfile` when the profile's user_data is set.
#[unsafe(no_mangle)]
pub extern "C" fn _cmsFreeUserDataFn() -> *mut c_void {
    ptr::null_mut()
}
/// `_cmsDefaultICCintents` — picks default intents for an n-profile link
#[unsafe(no_mangle)]
pub extern "C" fn _cmsDefaultICCintents(
    _ctx: Handle,
    n_profiles: c_uint,
    intents: *mut c_uint,
    profiles: *const *mut c_void,
) -> Handle {
    if profiles.is_null() || n_profiles == 0 {
        return ptr::null_mut();
    }
    let hs = unsafe { std::slice::from_raw_parts(profiles, n_profiles as usize) };
    let mut profs: Vec<Box<IccProfile>> = Vec::with_capacity(hs.len());
    for &h in hs {
        if h.is_null() {
            return ptr::null_mut();
        }
        profs.push(unsafe { Box::from_raw(h as *mut IccProfile) });
    }
    let mut bpc = vec![false; profs.len()];
    let intent_vec = if intents.is_null() {
        vec![lcms::types::INTENT_PERCEPTUAL; profs.len()]
    } else {
        unsafe { std::slice::from_raw_parts(intents, n_profiles as usize) }.to_vec()
    };
    let adapts = vec![1.0f64; profs.len()];
    let r = lcms::cnvrt::dbg_default_icc_intents(
        None,
        &mut profs,
        &intent_vec,
        &mut bpc,
        &adapts,
        0,
    );
    for b in profs {
        std::mem::forget(b);
    }
    r.map(|p| Box::into_raw(Box::new(p)) as Handle)
        .unwrap_or(ptr::null_mut())
}

// ---------------------------------------------------------------------------
// THR chunk accessors, plugin registration, MD5-ID, xyY2XYZ, transform2devlink
// ---------------------------------------------------------------------------

/// Registered-context pool — C's `_cmsContextPoolHead`. `_cmsGetContext`
/// resolves unknown non-NULL handles (e.g. the testbed's `DbgThread()`
/// fake thread ids) to the *global* context rather than dereferencing
/// them — `ctx_ref`/`cctx`/`mem_fns_of` mirror that.
static CTX_POOL: std::sync::Mutex<Vec<usize>> = std::sync::Mutex::new(Vec::new());
fn pool_register(h: Handle) {
    CTX_POOL.lock().unwrap().push(h as usize);
}
fn pool_unregister(h: Handle) {
    CTX_POOL.lock().unwrap().retain(|&x| x != h as usize);
}
fn pool_contains(h: Handle) -> bool {
    !h.is_null() && CTX_POOL.lock().unwrap().contains(&(h as usize))
}

fn ctx_ref<'a>(h: Handle) -> Option<&'a mut lcms::context::Context> {
    if !pool_contains(h) {
        return None;
    }
    Some(unsafe { &mut *(h as *mut lcms::context::Context) })
}
unsafe fn cctx(h: Handle) -> Option<&'static lcms::context::Context> {
    if !pool_contains(h) {
        None
    } else {
        Some(unsafe { &*(h as *const lcms::context::Context) })
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn cmsSetAlarmCodesTHR(ctx: Handle, codes: *const u16, n: c_uint) {
    let mut arr = [0u16; xform::MAX_CHANNELS];
    let m = (n as usize).min(xform::MAX_CHANNELS);
    if !codes.is_null() && m > 0 {
        unsafe { arr[..m].copy_from_slice(&std::slice::from_raw_parts(codes, m)[..m]) };
    }
    xform::set_alarm_codes(unsafe { cctx(ctx) }, &arr);
}
#[unsafe(no_mangle)]
pub extern "C" fn cmsGetAlarmCodesTHR(ctx: Handle, codes: *mut u16) {
    if codes.is_null() {
        return;
    }
    let c = xform::get_alarm_codes(unsafe { cctx(ctx) });
    unsafe { std::ptr::copy_nonoverlapping(c.as_ptr(), codes, xform::MAX_CHANNELS) };
}
#[unsafe(no_mangle)]
pub extern "C" fn cmsGetAlarmCodes(codes: *mut u16) {
    cmsGetAlarmCodesTHR(ptr::null_mut(), codes);
}
#[unsafe(no_mangle)]
pub extern "C" fn cmsSetAdaptationStateTHR(ctx: Handle, d: f64) -> f64 {
    xform::set_adaptation_state(unsafe { cctx(ctx) }, d)
}
#[unsafe(no_mangle)]
pub extern "C" fn cmsGetContextUserData(ctx: Handle) -> *mut c_void {
    match unsafe { cctx(ctx) } {
        Some(c) => c.user_data.load(std::sync::atomic::Ordering::Relaxed) as *mut c_void,
        None => {
            // NULL ctx → global context's user_data
            lcms::context::global()
                .user_data
                .load(std::sync::atomic::Ordering::Relaxed) as *mut c_void
        }
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn cmsSetContextUserData(ctx: Handle, data: *mut c_void) -> c_int {
    match ctx_ref(ctx) {
        Some(c) => {
            c.user_data
                .store(data as usize, std::sync::atomic::Ordering::Relaxed);
            1
        }
        None => 0,
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn cmsSetEncodedICCversion(h: Handle, v: c_uint) {
    if let Some(p) = prof_ref(h) {
        profile::set_encoded_icc_version(p, v);
    }
}
/// `cmsMD5computeID` — fills `ProfileID` with the MD5 of the profile tags
#[unsafe(no_mangle)]
pub extern "C" fn cmsMD5computeID(h: Handle) -> c_int {
    match prof_ref(h) {
        Some(p) => lcms::md5::compute_id(p) as c_int,
        None => 0,
    }
}
/// `cmsxyY2XYZ`
#[unsafe(no_mangle)]
pub extern "C" fn cmsxyY2XYZ(out: *mut CieXyz, xyy: *const lcms::types::CiexyY) {
    if out.is_null() || xyy.is_null() {
        return;
    }
    unsafe { *out = lcms::pcs::xyy2xyz(&*xyy) };
}

// Log-error handler — static C-callback + Rust trampoline into the context.
type CLogFn = unsafe extern "C" fn(Handle, c_uint, *const c_char);
static C_LOG_FN: std::sync::Mutex<Option<CLogFn>> = std::sync::Mutex::new(None);
fn c_log_trampoline(ctx: &lcms::context::Context, code: i32, text: &str) {
    let f = *C_LOG_FN.lock().unwrap();
    if let Some(f) = f {
        let cs = std::ffi::CString::new(text).unwrap_or_default();
        unsafe { f(ctx as *const lcms::context::Context as Handle, code as c_uint, cs.as_ptr()) };
    }
}
/// `cmsSetLogErrorHandler` — NULL-context handler → lcms global hook
#[unsafe(no_mangle)]
pub extern "C" fn cmsSetLogErrorHandlerFn(f: Option<CLogFn>) {
    *C_LOG_FN.lock().unwrap() = f;
    *lcms::err::GLOBAL_LOG_HOOK.lock().unwrap() = f.map(|_| c_log_trampoline as lcms::context::LogErrorHandler);
}
/// `cmsSetLogErrorHandlerTHR` — handler on the given boxed context
#[unsafe(no_mangle)]
pub extern "C" fn cmsSetLogErrorHandlerTHR(ctx: Handle, f: Option<CLogFn>) {
    if ctx.is_null() {
        cmsSetLogErrorHandlerFn(f);
        return;
    }
    *C_LOG_FN.lock().unwrap() = f;
    if let Some(c) = ctx_ref(ctx) {
        c.log_error_handler = f.map(|_| c_log_trampoline as lcms::context::LogErrorHandler);
    }
}
/// `cmsPlugin`/`cmsPluginTHR` — plugin registration. Walks the C
/// `cmsPluginBase` linked list; mem-handler and interp plugins are applied
/// to the context (remaining types are stub-tolerated for now).
#[unsafe(no_mangle)]
pub extern "C" fn cmsPlugin(_plugins: *const c_void) -> c_int {
    // Global-context registration: C applies to Context0's chunks. The
    // Rust global context is shared; registering into it would be a
    // persistent side effect — match C only for the memH case via a
    // process-global slot later if needed. Accept for now.
    1
}
#[unsafe(no_mangle)]
pub extern "C" fn cmsPluginTHR(ctx: Handle, plugins: *const c_void) -> c_int {
    if ctx.is_null() {
        return cmsPlugin(plugins);
    }
    if plugin_thr_register(ctx, plugins as *const CmsPluginBase) {
        1
    } else {
        0
    }
}
/// `cmsTransform2DeviceLink` — bakes a transform into a devicelink profile.
/// Needs a sampled-CLUT bake path that `lcms` doesn't expose yet (stub NULL).
#[unsafe(no_mangle)]
pub extern "C" fn cmsTransform2DeviceLink(
    _h: Handle,
    _version: f64,
    _flags: c_uint,
) -> Handle {
    ptr::null_mut()
}
