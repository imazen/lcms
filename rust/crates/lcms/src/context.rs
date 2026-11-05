//! The `cmsContext` analogue — `cmsplugin.c`'s per-context plugin slots.
//!
//! C carries a `cmsContext` (NULL = the global context) holding plugin
//! pointers: memory allocator, error logger, interpolation factories,
//! formatters, tag handlers, optimizers, threading. The port keeps the same
//! shape: every function that takes `cmsContext` in C takes
//! `Option<&Context>` here, `None` being the global context.
//!
//! Slots are added as the modules that consume them are ported.

use std::sync::OnceLock;

/// `cmsLogErrorHandlerFunction` — receives the error code and the formatted text.
pub type LogErrorHandler = fn(&Context, code: i32, text: &str);

/// `cmsInterpFnFactory` — returns the interpolation for (n_in, n_out, flags),
/// or `None` where C returns a zeroed `cmsInterpFunction` union.
pub type InterpFnFactory =
    fn(n_input: u32, n_output: u32, flags: u32) -> Option<crate::intrp::Interpolation>;

/// `cmsParametricCurveEvaluator` — evaluates a parametric curve type at R.
pub type ParametricCurveEvaluator = fn(r#type: i32, params: &[f64; 10], r: f64) -> f64;

/// Plugin slots owned by a context. Mirrors `_cmsContext_struct`'s
/// plugin-pointer fields (memory management excluded — the port allocates
/// with Rust allocators, matching C semantics where the allocator is
/// unobservable in outputs).
#[derive(Default)]
pub struct Context {
    /// `LogErrorHandler` plugin slot (cmserr.c)
    pub log_error_handler: Option<LogErrorHandler>,
    /// `_cmsInterpPluginChunkType.Interpolators` (cmsintrp.c)
    pub interpolators: Option<InterpFnFactory>,
    /// `_cmsCurvesPluginChunkType.ParametricCurves` (cmsgamma.c) — head first.
    pub parametric_curves: Vec<crate::gamma::ParametricCurves>,
    /// `_cmsMPETypePluginChunk` (cmstypes.c) — MPE element handlers, checked
    /// before the built-in `SupportedMPEtypes` table
    pub mpe_handlers: Vec<crate::tagtypes::MpeElemHandler>,
    /// `_cmsTagTypePluginChunkType.TagTypes` — tag type handlers added by
    /// plugins, checked before `SupportedTagTypes`
    pub tag_type_handlers: Vec<crate::tagtypes::TagTypeHandler>,
    /// `_cmsTagPluginChunkType.Tag` — plugin-registered tag descriptors
    /// (signature, descriptor), checked before `SupportedTags`
    pub tag_descriptors: Vec<(u32, crate::tagtypes::TagDescriptor)>,
}

/// The global context, used wherever C passes `NULL`.
pub fn global() -> &'static Context {
    static GLOBAL: OnceLock<Context> = OnceLock::new();
    GLOBAL.get_or_init(Context::default)
}

/// Resolve a `Option<&Context>` to a concrete context (C's NULL → global).
#[inline]
pub fn or_global<'a>(ctx: Option<&'a Context>) -> &'a Context {
    ctx.unwrap_or_else(|| global())
}
