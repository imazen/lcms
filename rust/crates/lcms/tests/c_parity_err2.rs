//! cmserr/cmsplugin lifecycle: context create/dup/delete + user-data +
//! log-error-handler get/set parity.

use lcms::context::{self, Context};
use lcms_cref as c;
use std::ffi::c_void;
use std::sync::atomic::Ordering;

extern "C" fn c_log(_ctx: *mut c_void, _code: i32, _text: *const i8) {}

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
        assert_eq!(rctx.user_data.load(Ordering::Relaxed), 0x1234,
            "rust user data");
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
    struct Base { magic: u32, ver: u32, ty: u32, next: *mut std::ffi::c_void }
    #[repr(C)]
    struct Plugin { base: Base, payload: [*mut std::ffi::c_void; 8] }
    unsafe {
        let mut mk = |ty: u32, magic: u32, ver: u32| Plugin {
            base: Base { magic, ver, ty, next: std::ptr::null_mut() },
            payload: [std::ptr::null_mut(); 8],
        };
        for (name, ty) in [("mutex", 0x6D747A48u32),
                           ("parl", 0x70726C48u32),
                           ("hdr", 0x68647248u32)] {
            let mut p = mk(ty, 0x61637070, 2200);
            let r = c::cmsPluginTHR(std::ptr::null_mut(),
                &mut p as *mut _ as *mut _);
            eprintln!("{name}: plugin={r}");
        }
        // rejects
        let mut bad = mk(0x6D747A48, 0xDEADBEEF, 2200);
        eprintln!("badmagic={}", c::cmsPluginTHR(
            std::ptr::null_mut(), &mut bad as *mut _ as *mut _));
        let mut hi = mk(0x6D747A48, 0x61637070, 9999);
        eprintln!("hivers={}", c::cmsPluginTHR(
            std::ptr::null_mut(), &mut hi as *mut _ as *mut _));
        // dup context on NULL + on a registered-context
        let d = c::cmsDupContext(std::ptr::null_mut(),
            std::ptr::null_mut());
        eprintln!("dupctx={}", !d.is_null());
        if !d.is_null() { c::cmsDeleteContext(d); }
    }
}

/// Plugin lookup internals — _cmsGetTagTypeHandler per sig +
/// cmsDupContext.
#[test]
fn plugin_lookup_internals() {
    unsafe {
        // tag-type-handler lookup across the tag-type sigs
        for sig in [0x63757276u32, 0x70617261, 0x6d667432,
                    0x6d414220, 0x6c617469, 0xdeadbeef] {
            let th = c::_cmsGetTagTypeHandler(std::ptr::null_mut(), sig);
            eprintln!("sig{sig:#x} th={}", !th.is_null());
        }
        // DupContext — clones the context chunk
        let ctx = c::cmsCreateContext(std::ptr::null_mut(),
            std::ptr::null_mut());
        let dc = c::cmsDupContext(ctx, std::ptr::null_mut());
        eprintln!("ctx={} dup={}", !ctx.is_null(), !dc.is_null());
        if !dc.is_null() { c::cmsDeleteContext(dc); }
        if !ctx.is_null() { c::cmsDeleteContext(ctx); }
    }
}
