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
