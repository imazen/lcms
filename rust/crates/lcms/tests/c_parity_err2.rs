//! cmserr/cmsplugin lifecycle: context create/dup/delete + user-data +
//! log-error-handler get/set parity.

use lcms::context::{self, Context};
use lcms_cref as c;
use std::ffi::c_void;
use std::sync::atomic::Ordering;

extern "C" fn c_log(_ctx: *mut c_void, _code: i32, _text: *const std::ffi::c_char) {}

fn r_log(_ctx: &Context, _code: i32, _text: &str) {}

#[test]
fn context_lifecycle() {
    unsafe {
        let ud = 0x1234usize as *mut c_void;
        let c_ctx = c::cmsDupContext(std::ptr::null_mut(), ud);
        assert!(!c_ctx.is_null());
        assert_eq!(c::cmsGetContextUserData(c_ctx), ud, "C user data");
        c::cmsDeleteContext(c_ctx);
        let rctx = context::create_context(None, 0x1234);
        assert_eq!(
            rctx.user_data.load(Ordering::Relaxed),
            0x1234,
            "rust user data"
        );
        let rdup = context::dup_context(&rctx, None, 0x5678);
        assert_eq!(rdup.user_data.load(Ordering::Relaxed), 0x5678, "dup data");
    }
}

#[test]
fn error_handler_slot() {
    unsafe {
        // set-only public API — the get is internal; verify set works
        c::cmsSetLogErrorHandler(c_log as *mut c_void);
        c::cmsSetLogErrorHandlerTHR(std::ptr::null_mut(), c_log as *mut c_void);
        let mut rctx = context::create_context(None, 0);
        rctx.log_error_handler = Some(r_log);
        assert!(rctx.log_error_handler.is_some(), "rust handler set");
        let rdup = context::dup_context(&rctx, None, 0);
        assert!(rdup.log_error_handler.is_some(), "dup handler carried");
    }
}

/// Remaining plugin-type dispatch + reject arms — cmsPluginTHR on
/// Mutex/Paralellization/Header plugin sigs, bad-Magic and
/// high-ExpectedVersion rejects, cmsDupContext on the result.
#[test]
fn plugin_remaining_arms() {
    #[repr(C)]
    struct Base {
        magic: u32,
        ver: u32,
        ty: u32,
        next: *mut std::ffi::c_void,
    }
    #[repr(C)]
    struct Plugin {
        base: Base,
        payload: [*mut std::ffi::c_void; 8],
    }
    unsafe {
        let mk = |ty: u32, magic: u32, ver: u32| Plugin {
            base: Base {
                magic,
                ver,
                ty,
                next: std::ptr::null_mut(),
            },
            payload: [std::ptr::null_mut(); 8],
        };
        for (name, ty) in [
            ("mutex", 0x6D747A48u32),
            ("parl", 0x70726C48u32),
            ("hdr", 0x68647248u32),
        ] {
            let mut p = mk(ty, 0x61637070, 2200);
            let r = c::cmsPluginTHR(std::ptr::null_mut(), &mut p as *mut _ as *mut _);
            eprintln!("{name}: plugin={r}");
        }
        // rejects
        let mut bad = mk(0x6D747A48, 0xDEADBEEF, 2200);
        eprintln!(
            "badmagic={}",
            c::cmsPluginTHR(std::ptr::null_mut(), &mut bad as *mut _ as *mut _)
        );
        let mut hi = mk(0x6D747A48, 0x61637070, 9999);
        eprintln!(
            "hivers={}",
            c::cmsPluginTHR(std::ptr::null_mut(), &mut hi as *mut _ as *mut _)
        );
        // dup context on NULL + on a registered-context
        let d = c::cmsDupContext(std::ptr::null_mut(), std::ptr::null_mut());
        eprintln!("dupctx={}", !d.is_null());
        if !d.is_null() {
            c::cmsDeleteContext(d);
        }
    }
}

/// Plugin lookup internals — _cmsGetTagTypeHandler per sig +
/// cmsDupContext.
#[test]
fn plugin_lookup_internals() {
    unsafe {
        // tag-type-handler lookup across the tag-type sigs
        for sig in [
            0x63757276u32,
            0x70617261,
            0x6d667432,
            0x6d414220,
            0x6c617469,
            0xdeadbeef,
        ] {
            let th = c::_cmsGetTagTypeHandler(std::ptr::null_mut(), sig);
            eprintln!("sig{sig:#x} th={}", !th.is_null());
        }
        // DupContext — clones the context chunk
        let ctx = c::cmsCreateContext(std::ptr::null_mut(), std::ptr::null_mut());
        let dc = c::cmsDupContext(ctx, std::ptr::null_mut());
        eprintln!("ctx={} dup={}", !ctx.is_null(), !dc.is_null());
        if !dc.is_null() {
            c::cmsDeleteContext(dc);
        }
        if !ctx.is_null() {
            c::cmsDeleteContext(ctx);
        }
    }
}

/// DupPluginIntentsList loop body — register an intent plugin on
/// a context, then DupContext walks the list.
#[test]
fn dup_intents_list() {
    #[repr(C)]
    struct Base {
        magic: u32,
        ver: u32,
        ty: u32,
        next: *mut std::ffi::c_void,
    }
    #[repr(C)]
    struct IntentPlugin {
        base: Base,
        intent: u32,
        evaluator: *mut std::ffi::c_void,
        user: *mut std::ffi::c_void,
        next: *mut std::ffi::c_void,
    }
    unsafe {
        extern "C" fn ev(
            _a: *mut std::ffi::c_void,
            _b: *mut std::ffi::c_void,
            _c: *mut std::ffi::c_void,
            _d: *mut std::ffi::c_void,
            _e: u32,
            _f: u32,
        ) -> *mut std::ffi::c_void {
            std::ptr::null_mut()
        }
        let ctx = c::cmsCreateContext(std::ptr::null_mut(), std::ptr::null_mut());
        let mut p = IntentPlugin {
            base: Base {
                magic: 0x61637070,
                ver: 2200,
                ty: 0x696E7448,
                next: std::ptr::null_mut(),
            },
            intent: 42,
            evaluator: ev as *mut _,
            user: std::ptr::null_mut(),
            next: std::ptr::null_mut(),
        };
        let r = c::cmsPluginTHR(ctx, &mut p as *mut _ as *mut _);
        eprintln!("intent-plugin={r}");
        let d = c::cmsDupContext(ctx, std::ptr::null_mut());
        eprintln!("dup-intents={}", !d.is_null());
        if !d.is_null() {
            c::cmsDeleteContext(d);
        }
        if !ctx.is_null() {
            c::cmsDeleteContext(ctx);
        }
    }
}

/// cmsplugin residual — cmsCreateContext (plugins+
///   userdata), DupContext propagation, DeleteContext
///   lifecycle, SetAlarmCodes clamping, GetClientChunk
///   via context-scoped profile ops.
#[test]
fn err_ctx_lifecycle_arms() {
    unsafe {
        // create ctx with userdata, dup it, use both
        let mut ud = 0xABCDu32;
        let ctx = c::cmsCreateContext(std::ptr::null_mut(), &mut ud as *mut _ as *mut _);
        eprintln!("ctx={}", !ctx.is_null());
        let srgb_ctx = if !ctx.is_null() {
            c::cmsCreate_sRGBProfileTHR(ctx)
        } else {
            std::ptr::null_mut()
        };
        eprintln!("  srgb-on-ctx={}", !srgb_ctx.is_null());
        let dctx = if !ctx.is_null() {
            let mut ud2 = 0x1234u32;
            c::cmsDupContext(ctx, &mut ud2 as *mut _ as *mut _)
        } else {
            std::ptr::null_mut()
        };
        eprintln!("  dctx={}", !dctx.is_null());
        let srgb_d = if !dctx.is_null() {
            c::cmsCreate_sRGBProfileTHR(dctx)
        } else {
            std::ptr::null_mut()
        };
        eprintln!("  srgb-on-dctx={}", !srgb_d.is_null());
        // alarm codes: set/get on ctx + NULL (global)
        let codes = [1u16, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
        c::cmsSetAlarmCodes(codes.as_ptr());
        let mut got = [0u16; 16];
        c::cmsGetAlarmCodes(got.as_mut_ptr());
        eprintln!("  alarms={:?}", &got[..4]);
        if !ctx.is_null() {
            let codes2 = [100u16; 16];
            c::cmsSetAlarmCodesTHR(ctx, codes2.as_ptr());
            let mut g2 = [0u16; 16];
            c::cmsGetAlarmCodesTHR(ctx, g2.as_mut_ptr());
            eprintln!("  ctx-alarms={:?}", &g2[..4]);
        }
        // NULL-context alarm ops (global)
        for v in [0u16, 1, 0xFFFF] {
            let mut a = [v; 16];
            c::cmsGetAlarmCodes(a.as_mut_ptr());
            eprintln!("  null-ctx alarms[0]={}", a[0]);
        }
        // dup chain: dup-of-dup, then delete all
        let ddctx = if !dctx.is_null() {
            c::cmsDupContext(dctx, std::ptr::null_mut())
        } else {
            std::ptr::null_mut()
        };
        eprintln!("  ddctx={}", !ddctx.is_null());
        if !ddctx.is_null() {
            c::cmsDeleteContext(ddctx);
        }
        if !dctx.is_null() {
            c::cmsDeleteContext(dctx);
        }
        if !srgb_d.is_null() {
            c::cmsCloseProfile(srgb_d);
        }
        if !srgb_ctx.is_null() {
            c::cmsCloseProfile(srgb_ctx);
        }
        if !ctx.is_null() {
            c::cmsDeleteContext(ctx);
        }
        // create + immediately delete empty ctx
        for _ in 0..3 {
            let e = c::cmsCreateContext(std::ptr::null_mut(), std::ptr::null_mut());
            c::cmsDeleteContext(e);
        }
        eprintln!("lifecycle ok");
    }
}

/// cmsplugin residual — IOhandler plugin registration
///   + _cmsIOPrintf internals via plugin-registered
///     memory IO used for profile open.
#[test]
fn err_io_plugin_arms() {
    unsafe {
        // register a no-op mem-handler plugin bundle,
        //   then exercise PluginIO paths on a context
        #[repr(C)]
        struct PlugBase {
            magic: u32,
            expected_version: u32,
            ty: u32,
            next: *mut std::ffi::c_void,
        }
        #[repr(C)]
        struct MemPlug {
            base: PlugBase,
            alloc: *mut std::ffi::c_void,
            free: *mut std::ffi::c_void,
            realloc: *mut std::ffi::c_void,
            calloc: *mut std::ffi::c_void,
            dup: *mut std::ffi::c_void,
            malloc_zero: *mut std::ffi::c_void,
        }
        unsafe extern "C" {
            fn malloc(n: usize) -> *mut std::ffi::c_void;
            fn free(p: *mut std::ffi::c_void);
            fn realloc(p: *mut std::ffi::c_void, n: usize) -> *mut std::ffi::c_void;
        }
        unsafe extern "C" fn my_alloc(
            _ctx: *mut std::ffi::c_void,
            n: u32,
        ) -> *mut std::ffi::c_void {
            unsafe { malloc(n as usize) }
        }
        unsafe extern "C" fn my_free(_ctx: *mut std::ffi::c_void, p: *mut std::ffi::c_void) {
            unsafe { free(p) }
        }
        unsafe extern "C" fn my_realloc(
            _ctx: *mut std::ffi::c_void,
            p: *mut std::ffi::c_void,
            n: u32,
        ) -> *mut std::ffi::c_void {
            unsafe { realloc(p, n as usize) }
        }
        let mut plug = MemPlug {
            base: PlugBase {
                magic: 0x636D7350,
                expected_version: 2200,
                ty: 0x6D656D48,
                next: std::ptr::null_mut(),
            },
            alloc: my_alloc as *mut _,
            free: my_free as *mut _,
            realloc: my_realloc as *mut _,
            calloc: std::ptr::null_mut(),
            dup: std::ptr::null_mut(),
            malloc_zero: std::ptr::null_mut(),
        };
        // register on a fresh ctx
        let ctx = c::cmsCreateContext(&mut plug as *mut _ as *mut _, std::ptr::null_mut());
        eprintln!("ctx-with-mem={}", !ctx.is_null());
        // dup it -> DupPluginMemHandler
        let dctx = if !ctx.is_null() {
            c::cmsDupContext(ctx, std::ptr::null_mut())
        } else {
            std::ptr::null_mut()
        };
        eprintln!("dctx={}", !dctx.is_null());
        // profile ops on both contexts
        for cx in [ctx, dctx] {
            if cx.is_null() {
                continue;
            }
            let p = c::cmsCreate_sRGBProfileTHR(cx);
            eprintln!("srgb-on-ctx={}", !p.is_null());
            if !p.is_null() {
                c::cmsCloseProfile(p);
            }
        }
        if !dctx.is_null() {
            c::cmsDeleteContext(dctx);
        }
        if !ctx.is_null() {
            c::cmsDeleteContext(ctx);
        }
    }
}

/// cmserr/cmsio0 residual — cmsfilelength via file
///   ops on edge sizes + signal-error THR variants +
///   formatter lookup internals.
#[test]
fn err_filelen_arms() {
    unsafe {
        // signal errors via invalid opens at sizes
        for n in [0u32, 1, 4, 64, 127, 128, 129, 255] {
            let b = vec![0x61u8; n as usize];
            let p = c::cmsOpenProfileFromMem(b.as_ptr() as *const _, n);
            let _ = p;
            if !p.is_null() {
                c::cmsCloseProfile(p);
            }
        }
        // error handler capture at each error code
        static mut SEEN: [u32; 16] = [0; 16];
        unsafe extern "C" fn errh(
            _ctx: *mut std::ffi::c_void,
            code: u32,
            _msg: *const std::ffi::c_char,
        ) {
            unsafe {
                if (code as usize) < 16 {
                    SEEN[code as usize] += 1;
                }
            }
        }
        c::cmsSetLogErrorHandlerTHR(std::ptr::null_mut(), errh as *mut _);
        // trigger errors: bad profile, bad tag read,
        //   bad transform, bad it8
        let bad = c::cmsOpenProfileFromMem(b"x".as_ptr() as *const _, 1);
        if !bad.is_null() {
            c::cmsCloseProfile(bad);
        }
        let it8 = c::cmsIT8LoadFromMem(std::ptr::null_mut(), b"BAD".as_ptr() as *const _, 3);
        if !it8.is_null() {
            c::cmsIT8Free(it8);
        }
        eprintln!("errs={:?}", { &SEEN[..8] });
        c::cmsSetLogErrorHandlerTHR(std::ptr::null_mut(), std::ptr::null_mut());
    }
}
