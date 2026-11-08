//! cmserr/cmsplugin context machinery: cmsDupContext,
//! cmsGetContextUserData, cmsSetLogErrorHandler(THR).

use lcms::context;
use lcms_cref as c;
use std::ffi::c_void;

#[test]
fn context_dup_userdata() {
    unsafe {
        // NULL context dup — creates a fresh context
        let c_ctx = c::cmsDupContext(std::ptr::null_mut(),
            0xDEADBEEF as *mut c_void);
        assert!(!c_ctx.is_null());
        assert_eq!(c::cmsGetContextUserData(c_ctx) as usize,
            0xDEADBEEF);
        // a second dup with different user data
        let c_ctx2 = c::cmsDupContext(c_ctx,
            0xCAFEBABE as *mut c_void);
        assert_eq!(c::cmsGetContextUserData(c_ctx2) as usize,
            0xCAFEBABE);
        assert_eq!(c::cmsGetContextUserData(c_ctx) as usize,
            0xDEADBEEF); // original unchanged
        c::cmsDeleteContext(c_ctx);
        c::cmsDeleteContext(c_ctx2);
        // rust side
        let r_ctx = context::create_context(None, 0);
        let r_dup = context::dup_context(&r_ctx, None, 0xDEADBEEF);
        assert_eq!(r_dup.user_data.load(
            std::sync::atomic::Ordering::Relaxed), 0xDEADBEEF);
        let r_dup2 = context::dup_context(&r_dup, None, 0xCAFEBABE);
        assert_eq!(r_dup2.user_data.load(
            std::sync::atomic::Ordering::Relaxed), 0xCAFEBABE);
        assert_eq!(r_dup.user_data.load(
            std::sync::atomic::Ordering::Relaxed), 0xDEADBEEF);
    }
}

#[test]
fn log_error_handler() {
    unsafe {
        // register a handler on the NULL context — exercises the
        // global error-handler slot
        c::cmsSetLogErrorHandler(std::ptr::null_mut()); // NULL handler
        // set a real handler
        unsafe extern "C" fn handler(_ctx: *mut c_void,
            _code: u32, _text: *const i8) {}
        c::cmsSetLogErrorHandler(handler as *mut c_void);
        c::cmsSetLogErrorHandler(std::ptr::null_mut()); // restore
        // THR variant on a dup'd context
        let ctx = c::cmsDupContext(std::ptr::null_mut(),
            std::ptr::null_mut());
        c::cmsSetLogErrorHandlerTHR(ctx, handler as *mut c_void);
        c::cmsDeleteContext(ctx);
    }
}

