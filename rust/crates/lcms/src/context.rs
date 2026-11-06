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
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

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
    /// `_cmsAlarmCodesChunkType` (cmsxform.c) — default {0x7F00,0x7F00,0x7F00,0,...}
    pub alarm_codes: Mutex<[u16; 16]>,
    /// `_cmsAdaptationStateChunkType` — default 1.0 (f64 bits)
    pub adaptation_state: AtomicU64,
    /// `UserData` — `cmsCreateContext`'s opaque slot (`cmsGetContextUserData`)
    pub user_data: AtomicUsize,
    /// `DefaultMemoryManager` — C stores the 6 `cmsPluginMemHandler` fn ptrs
    /// inside `_cmsContext_struct`. Stored as usize so the pure-Rust side
    /// stays Send+Sync; populated/read by the FFI layer only.
    /// Order: Malloc, MallocZero, Free, Realloc, Calloc, Dup. `[0;6]` =
    /// defaults (the FFI supplies compatible fns).
    pub mem_fns: [usize; 6],
    /// C-side `Interpolators` chain — raw `cmsInterpFnFactory` pointers
    /// registered via `cmsPluginTHR` (FFI only; the pure-Rust `interpolators`
    /// slot above serves native callers). Each factory is consulted in order
    /// until one returns a non-NULL `cmsInterpFunction`, like
    /// `_cmsRegisterInterpPlugin` chains them.
    pub interp_factories: Vec<usize>,
    /// C-side `ParametricCurves` chain — raw `cmsParametricCurveEvaluator`
    /// pointers registered via `cmsPluginTHR` (FFI only). Checked before the
    /// built-in defaults, mirroring `GetParametricCurveByType`.
    pub c_parametric_curves: Vec<CParametricSet>,
    /// C-side `Formatters` chain — raw `cmsFormattersFactory` pointers
    /// registered via `cmsPluginTHR` (FFI only). Consulted before the stock
    /// formatters in `_cmsGetFormatter`.
    pub formatter_factories: Vec<usize>,
}

/// A C-registered `_cmsParametricCurvesCollection` entry — FFI fn ptrs.
#[derive(Clone)]
pub struct CParametricSet {
    /// `FunctionTypes` — C matches by `abs(Type)`
    pub function_types: Vec<i32>,
    /// `ParameterCount`
    pub parameter_count: Vec<u32>,
    /// `Evaluator` — `f64 (*)(i32, *const f64 /*[10]*/, f64) -> f64`
    pub evaluator: usize,
}

/// C `DEFAULT_ALARM_CODES_VALUE`
pub const DEFAULT_ALARM_CODES: [u16; 16] =
    [0x7F00, 0x7F00, 0x7F00, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

impl Default for Context {
    fn default() -> Self {
        Context {
            log_error_handler: None,
            interpolators: None,
            parametric_curves: Vec::new(),
            mpe_handlers: Vec::new(),
            tag_type_handlers: Vec::new(),
            tag_descriptors: Vec::new(),
            alarm_codes: Mutex::new(DEFAULT_ALARM_CODES),
            adaptation_state: AtomicU64::new(1.0f64.to_bits()),
            user_data: AtomicUsize::new(0),
            mem_fns: [0; 6],
            interp_factories: Vec::new(),
            c_parametric_curves: Vec::new(),
            formatter_factories: Vec::new(),
        }
    }
}

impl Clone for Context {
    fn clone(&self) -> Self {
        Context {
            log_error_handler: self.log_error_handler,
            interpolators: self.interpolators,
            parametric_curves: self.parametric_curves.clone(),
            mpe_handlers: self.mpe_handlers.clone(),
            tag_type_handlers: self.tag_type_handlers.clone(),
            tag_descriptors: self.tag_descriptors.clone(),
            alarm_codes: Mutex::new(*self.alarm_codes.lock().unwrap()),
            adaptation_state: AtomicU64::new(self.adaptation_state.load(Ordering::Relaxed)),
            user_data: AtomicUsize::new(self.user_data.load(Ordering::Relaxed)),
            mem_fns: self.mem_fns,
            interp_factories: self.interp_factories.clone(),
            c_parametric_curves: self.c_parametric_curves.clone(),
            formatter_factories: self.formatter_factories.clone(),
        }
    }
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
pub fn create_context(plugins: Option<&[Plugin]>, user_data: usize) -> Context {
    let mut ctx = Context::default();
    ctx.user_data.store(user_data, Ordering::Relaxed);
    if let Some(p) = plugins {
        ctx.plugin(p);
    }
    ctx
}

/// `cmsDupContext` — clone a context, apply new plugins + user data
pub fn dup_context(src: &Context, plugins: Option<&[Plugin]>, new_user_data: usize) -> Context {
    let mut ctx = src.clone();
    ctx.user_data.store(new_user_data, Ordering::Relaxed);
    if let Some(p) = plugins {
        ctx.plugin(p);
    }
    ctx
}
