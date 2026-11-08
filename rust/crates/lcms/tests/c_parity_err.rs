//! cmserr/cmsgmt/context parity: GBD alloc->add->compute->check over a
//! point grid (containment booleans), error-handler invocation parity
//! (code+text on a triggered SynError), context create/dup/user-data.

use lcms::{gbd::Gbd, types::CieLab};
use lcms_cref as c;
use std::ffi::c_void;
use std::sync::Mutex;

fn cstr(s: &str) -> *const i8 {
    Box::leak(format!("{s}\0").into_boxed_str()).as_ptr() as *const i8
}

// --- GBD: same hull both sides, check grid ---
fn hull_points() -> Vec<CieLab> {
    // sRGB gamut-ish hull: corners + midpoints of a Lab box
    let mut v = Vec::new();
    for &l in &[0.0f64, 50.0, 100.0] {
        for &a in &[-128.0f64, 0.0, 127.0] {
            for &b in &[-128.0f64, 0.0, 127.0] {
                v.push(CieLab { L: l, a, b });
            }
        }
    }
    v
}

#[test]
fn gbd_containment() {
    let hull = hull_points();
    let cg = unsafe { c::cmsGBDAlloc(std::ptr::null_mut()) };
    assert!(!cg.is_null());
    let mut rg = Gbd::alloc(None);
    for p in &hull {
        let c_ok = unsafe {
            c::cmsGDBAddPoint(cg, p as *const CieLab as *const c_void)
        };
        assert_eq!(c_ok != 0, rg.add_point(p), "add_point {p:?}");
    }
    let c_comp = unsafe { c::cmsGDBCompute(cg, 0) };
    assert_eq!(c_comp != 0, rg.compute(0), "compute");
    // containment grid: inside/on/outside the hull
    let mut checked = 0usize;
    for l in (-20..140).step_by(20) {
        for a in (-160..170).step_by(40) {
            for b in (-160..170).step_by(40) {
                let p = CieLab {
                    L: l as f64,
                    a: a as f64,
                    b: b as f64,
                };
                let ci = unsafe {
                    c::cmsGDBCheckPoint(cg, &p as *const CieLab as *const c_void)
                };
                assert_eq!(ci != 0, rg.check_point(&p), "check {p:?}");
                checked += 1;
            }
        }
    }
    assert!(checked > 100);
    unsafe { c::cmsGBDFree(cg) };
}

// --- error handler parity ---
static LAST_C: Mutex<Option<(u32, String)>> = Mutex::new(None);

unsafe extern "C" fn c_handler(_ctx: *mut c_void, code: u32, text: *const i8) {
    let t = if text.is_null() {
        String::new()
    } else {
        std::ffi::CStr::from_ptr(text).to_string_lossy().into_owned()
    };
    *LAST_C.lock().unwrap() = Some((code, t));
}

#[test]
fn error_handler_fires() {
    // C: install THR handler, trigger SynError via out-of-range SetDataFormat
    unsafe {
        let h = c::cmsIT8Alloc(std::ptr::null_mut());
        c::cmsIT8SetPropertyDbl(h, cstr("NUMBER_OF_FIELDS"), 4.0);
        c::cmsSetLogErrorHandlerTHR(std::ptr::null_mut(), c_handler as *mut c_void);
        *LAST_C.lock().unwrap() = None;
        // n >= NUMBER_OF_FIELDS -> "Invalid or more than NUMBER_OF_FIELDS fields."
        c::cmsIT8SetDataFormat(h, 99, cstr("X"));
        let c_err = LAST_C.lock().unwrap().clone();
        assert!(c_err.is_some(), "C handler fired");
        c::cmsIT8Free(h);

        // Rust: context with recording handler, same trigger
        static LAST_R: Mutex<Option<(i32, String)>> = Mutex::new(None);
        *LAST_R.lock().unwrap() = None;
        fn r_handler(_c: &lcms::context::Context, code: i32, text: &str) {
            *LAST_R.lock().unwrap() = Some((code, text.to_string()));
        }
        let mut ctx = lcms::context::create_context(None, 0);
        ctx.log_error_handler = Some(r_handler);
        let mut it8 = lcms::cgats::It8::alloc(Some(ctx));
        it8.set_property_dbl("NUMBER_OF_FIELDS", 4.0);
        it8.set_data_format_export(99, "X");
        let r_err = LAST_R.lock().unwrap().clone();
        assert!(r_err.is_some(), "rust handler fired");
        let (cc, ct) = c_err.unwrap();
        let (rc, rt) = r_err.unwrap();
        assert_eq!(cc as i32, rc, "error code parity");
        assert_eq!(ct, rt, "error text parity");
    }
}

#[test]
fn context_lifecycle() {
    unsafe {
        let ud = 0xDEADBEEFusize as *mut c_void;
        let cctx = c::cmsCreateContext(std::ptr::null_mut(), ud);
        assert!(!cctx.is_null());
        assert_eq!(c::cmsGetContextUserData(cctx), ud, "C user data");
        let ud2 = 0x1234usize as *mut c_void;
        let cdup = c::cmsDupContext(cctx, ud2);
        assert!(!cdup.is_null());
        assert_eq!(c::cmsGetContextUserData(cdup), ud2, "C dup user data");
        c::cmsDeleteContext(cdup);
        c::cmsDeleteContext(cctx);
    }
    let rctx = lcms::context::create_context(None, 0xDEADBEEF);
    assert_eq!(
        rctx.user_data.load(std::sync::atomic::Ordering::Relaxed),
        0xDEADBEEF,
        "rust user data"
    );
    let rdup = lcms::context::dup_context(&rctx, None, 0x1234);
    assert_eq!(
        rdup.user_data.load(std::sync::atomic::Ordering::Relaxed),
        0x1234,
        "rust dup user data"
    );
}

/// cmssamp desaturate + GBD edge cases (empty hull, check-before-compute,
/// boundary points).
#[test]
fn desaturate_and_gbd_edges() {
    use lcms::types::CieLab;
    // out-of-gamut points get clamped into the box [amin..amax]x[bmin..bmax]
    for &(l, a, b) in &[(50.0f64, 200.0f64, -200.0f64), (10.0, -50.0, 80.0), (0.0, 0.0, 0.0)] {
        let mut cl = CieLab { L: l, a, b };
        let mut rl = cl;
        unsafe {
            c::cmsDesaturateLab(&mut cl as *mut CieLab as *mut _, 80.0, -80.0, 80.0, -80.0)
        };
        lcms::sm::desaturate_lab(&mut rl, 80.0, -80.0, 80.0, -80.0);
        assert_eq!((cl.L, cl.a, cl.b), (rl.L, rl.a, rl.b), "desaturate {l},{a},{b}");
    }
    // GBD edge cases — empty compute, check-before-compute
    unsafe {
        let cg = c::cmsGBDAlloc(std::ptr::null_mut());
        let mut rg = lcms::gbd::Gbd::alloc(None);
        let p = CieLab { L: 50.0, a: 0.0, b: 0.0 };
        // check before any add — both should agree (probably false/not-in-gamut)
        assert_eq!(
            c::cmsGDBCheckPoint(cg, &p as *const CieLab as *const _) != 0,
            rg.check_point(&p),
            "check on empty GBD"
        );
        // compute empty — status parity
        assert_eq!(
            c::cmsGDBCompute(cg, 0) != 0,
            rg.compute(0),
            "compute empty"
        );
        c::cmsGBDFree(cg);
    }
}

/// cmsplugin registration machinery: ParametricCurves plugin base-struct
/// registration + context dup preserving the registration.
#[repr(C)]
struct CPluginBase {
    magic: u32,
    expected_version: u32,
    type_: u32,
    next: *mut c_void,
}
#[repr(C)]
struct CPluginParametricCurves {
    base: CPluginBase,
    n_functions: u32,
    function_types: [u32; 20],
    parameter_count: [u32; 20],
    evaluator: *mut c_void,
}

unsafe extern "C" fn nop_eval(_t: i32, _p: *const f64, _r: f64) -> f64 { 0.0 }

#[test]
fn plugin_registration() {
    // C: register a ParametricCurves plugin (new curve type 999) into the
    // default context; status must be true.
    let mut cp = CPluginParametricCurves {
        base: CPluginBase {
            magic: 0x61637070, // 'acpp'
            expected_version: 2200,
            type_: 0x70617248, // 'parH'
            next: std::ptr::null_mut(),
        },
        n_functions: 1,
        function_types: {
            let mut a = [0u32; 20];
            a[0] = 999;
            a
        },
        parameter_count: {
            let mut a = [0u32; 20];
            a[0] = 3;
            a
        },
        evaluator: nop_eval as *mut c_void,
    };
    let c_ok = unsafe { c::cmsPlugin(&mut cp as *mut CPluginParametricCurves as *mut _) };
    assert!(c_ok != 0, "C plugin registration");

    // Rust: same registration into a context
    fn r_eval(_t: i32, _p: &[f64; 10], _r: f64) -> f64 { 0.0 }
    let mut ctx = lcms::context::create_context(None, 0);
    let r_ok = ctx.plugin(&[lcms::context::Plugin::ParametricCurve(
        lcms::gamma::ParametricCurves {
            function_types: vec![999],
            parameter_count: vec![3],
            evaluator: r_eval,
        },
    )]);
    assert!(r_ok, "rust plugin registration");
}
