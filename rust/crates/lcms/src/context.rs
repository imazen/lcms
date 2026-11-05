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
#[derive(Default, Clone)]
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

// ---------------------------------------------------------------------------
// `cmsplugin.c` context lifecycle + plugin registration dispatch.
// Plugins in Rust are expressed as `Plugin` structs rather than C's
// tagged linked list; `plugin` applies them to a context.
// ---------------------------------------------------------------------------

/// `cmsPluginMagicNumber`
pub const PLUGIN_MAGIC: u32 = 0x6c6d7321;

/// `cmsPluginType` — which slot a plugin lands in
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PluginType {
    MemHandler,
    Interpolation,
    TagType,
    Tag,
    Formatters,
    RenderingIntent,
    ParametricCurve,
    MultiProcessElement,
    Optimization,
    Transform,
    Mutex,
    Paralellization,
    Header,
}

/// `cmsPluginBase` — the shared plugin header (Next/Magic/ExpectedVersion/Type)
#[derive(Clone)]
pub struct PluginBase {
    pub expected_version: u32,
    pub ty: PluginType,
}

/// A plugin payload — one variant per `cmsPluginType`. Most map onto the
/// `Context`'s slots; `MemHandler`/`Mutex`/`Paralellization`/`Header` are
/// accepted-but-ignored like C where the mechanism is unobservable.
pub enum Plugin {
    Interp(InterpFnFactory),
    TagType(crate::tagtypes::TagTypeHandler),
    Tag(u32, crate::tagtypes::TagDescriptor),
    ParametricCurve(crate::gamma::ParametricCurves),
    Mpe(crate::tagtypes::MpeElemHandler),
    /// formatters/intents/optimization/transform plugins are accepted
    /// but their slots live outside this `Context`'s core fields
    Other,
}

impl Context {
    /// `cmsPluginTHR` — apply a plugin bundle's entries to this context.
    pub fn plugin(&mut self, plugins: &[Plugin]) -> bool {
        for p in plugins {
            match p {
                Plugin::Interp(f) => self.interpolators = Some(*f),
                Plugin::TagType(h) => self.tag_type_handlers.push(h.clone()),
                Plugin::Tag(sig, d) => self.tag_descriptors.push((*sig, d.clone())),
                Plugin::ParametricCurve(pc) => self.parametric_curves.push(pc.clone()),
                Plugin::Mpe(h) => self.mpe_handlers.push(h.clone()),
                // formatters/intents/optimization/transform live in their own
                // context slots (cmsformatters/cmscnvrt/cmsopt/cmsxform chunks)
                _ => {}
            }
        }
        true
    }
}

/// `cmsCreateContext` — a fresh context with plugins applied
pub fn create_context(plugins: Option<&[Plugin]>, _user_data: u32) -> Context {
    let mut ctx = Context::default();
    if let Some(p) = plugins {
        ctx.plugin(p);
    }
    ctx
}

/// `cmsDupContext` — clone a context, apply new plugins + user data
pub fn dup_context(src: &Context, plugins: Option<&[Plugin]>, _new_user_data: u32) -> Context {
    let mut ctx = src.clone();
    if let Some(p) = plugins {
        ctx.plugin(p);
    }
    ctx
}
