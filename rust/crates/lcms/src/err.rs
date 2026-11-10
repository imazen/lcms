//! Port of the `cmserr.c` surface — error signalling and the version query.
//!
//! C source: `reference/lcms2/src/cmserr.c`
//! The context plumbing (`_cmsCreateContext` etc.) is in `context.rs`.

use crate::context::{Context, or_global};
use crate::types::LCMS_VERSION;

/// Default log handler — `cmsSignalError`'s built-in fallback. lcms2.20's
/// `DefaultLogErrorHandlerFunction` (cmserr.c:521) is silent — its
/// `fprintf(stderr, ...)` is commented out — so the port prints nothing
/// either. The message still reaches callers through `take_last_error`
/// (`api::Error`) and any handler registered with `cmsSetLogErrorHandler`.
fn default_log_error_handler(_ctx: &Context, _code: i32, text: &str) {
    // FFI hook: `cmsSetLogErrorHandler` on the NULL context registers here
    // (Context0's chunk holds the handler in C).
    if let Some(f) = *GLOBAL_LOG_HOOK.lock().unwrap() {
        f(_ctx, _code, text);
    }
}

/// `cmsSetLogErrorHandler` global handler slot — FFI-only bridge for the
/// NULL context (Context0). Mirrors C's LogErrorHandlerChunk in Context0.
#[doc(hidden)]
pub static GLOBAL_LOG_HOOK: std::sync::Mutex<Option<crate::context::LogErrorHandler>> =
    std::sync::Mutex::new(None);

thread_local! {
    /// The most recent `signal_error` (code, text) on this thread —
    /// lets `api::Error` carry the real message a failing call produced.
    static LAST_ERROR: std::cell::Cell<Option<(i32, &'static str)>> = const { std::cell::Cell::new(None) };
    static LAST_ERROR_TEXT: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

/// `cmsSignalError` — route an error through the context's log handler.
/// `ctx == None` is C's `NULL` (global context). The text is preformatted;
/// C takes a printf-style format, we take the finished message.
pub fn signal_error(ctx: Option<&Context>, code: i32, text: &str) {
    // record for `take_last_error` before dispatch
    LAST_ERROR.with(|le| le.set(Some((code, ""))));
    LAST_ERROR_TEXT.with(|lt| *lt.borrow_mut() = text.to_string());
    let c = or_global(ctx);
    match c.log_error_handler {
        Some(handler) => handler(c, code, text),
        None => default_log_error_handler(c, code, text),
    }
}

/// Read + clear the last `signal_error` this thread produced.
/// `Some((code, message))` if a call just failed through the error path.
pub fn take_last_error() -> Option<(i32, String)> {
    let got = LAST_ERROR.with(|le| le.take());
    got.map(|(code, _)| (code, LAST_ERROR_TEXT.with(|lt| std::mem::take(&mut *lt.borrow_mut()))))
}

/// `cmsGetEncodedCMMversion` — `LCMS_VERSION`
pub fn encoded_cmm_version() -> u32 {
    LCMS_VERSION
}
