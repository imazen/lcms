//! Port of the `cmserr.c` surface — error signalling and the version query.
//!
//! C source: `reference/lcms2/src/cmserr.c`
//! The context plumbing (`_cmsCreateContext` etc.) is in `context.rs`.

use crate::context::{Context, or_global};
use crate::types::LCMS_VERSION;

/// Default log handler — `cmsSignalError`'s built-in fallback:
/// `fprintf(stderr, "lcms2: %s\n", Text)` is the C behaviour; the message is
/// already formatted, so the port emits the same text on stderr.
fn default_log_error_handler(_ctx: &Context, _code: i32, text: &str) {
    eprintln!("lcms2: {text}");
}

/// `cmsSignalError` — route an error through the context's log handler.
/// `ctx == None` is C's `NULL` (global context). The text is preformatted;
/// C takes a printf-style format, we take the finished message.
pub fn signal_error(ctx: Option<&Context>, code: i32, text: &str) {
    let c = or_global(ctx);
    match c.log_error_handler {
        Some(handler) => handler(c, code, text),
        None => default_log_error_handler(c, code, text),
    }
}

/// `cmsGetEncodedCMMversion` — `LCMS_VERSION`
pub fn encoded_cmm_version() -> u32 {
    LCMS_VERSION
}
