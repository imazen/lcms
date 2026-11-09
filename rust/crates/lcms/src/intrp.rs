//! Port of `cmsintrp.c` — interpolation kernels and `cmsInterpParams`.
//!
//! C source: `reference/lcms2/src/cmsintrp.c`
//! Gate: `tests/c_parity_intrp.rs`
//!
//! C's `cmsInterpParams.Table` is a borrowed `const void*`; here it is a
//! borrowed `InterpTable` slice — same lifetime semantics, bounds-checked.
//! The `Interpolation` union is an enum (16-bit vs float kernel); a plugin
//! factory returns `None` where C returns a zeroed union.

use crate::context::{Context, or_global};
use crate::err;
use crate::fixed::{fixed_rest_to_int, fixed_to_int, round_fixed_to_int, to_fixed_domain};
use crate::types::{ERROR_RANGE, ERROR_UNKNOWN_EXTENSION};

/// `MAX_INPUT_DIMENSIONS`
pub const MAX_INPUT_DIMENSIONS: usize = 15;
/// `MAX_STAGE_CHANNELS` (lcms2_internal.h)
pub const MAX_STAGE_CHANNELS: usize = 128;

/// `CMS_LERP_FLAGS_16BITS`
pub const LERP_FLAGS_16BITS: u32 = 0x0000;
/// `CMS_LERP_FLAGS_FLOAT`
pub const LERP_FLAGS_FLOAT: u32 = 0x0001;
/// `CMS_LERP_FLAGS_TRILINEAR`
pub const LERP_FLAGS_TRILINEAR: u32 = 0x0100;

/// `_cmsInterpFn16`
pub type InterpFn16 = fn(input: &[u16], output: &mut [u16], p: &InterpParams);
/// `_cmsInterpFnFloat`
pub type InterpFnFloat = fn(input: &[f32], output: &mut [f32], p: &InterpParams);

/// `cmsInterpParams` — the C-layout params block handed to foreign
/// (plugin-registered `_cmsInterpFn16`/`_cmsInterpFnFloat`) kernels.
/// Matches `lcms2_internal.h`'s `_cms_interp_struc` field order; pointers
/// are stored as `usize` so the struct stays Send+Sync — the FFI layer
/// casts it back to `*const CmsInterpParams` when invoking the kernel.
#[repr(C)]
#[derive(Clone)]
pub struct CmsInterpParams {
    pub context_id: usize,
    pub dw_flags: u32,
    pub n_inputs: u32,
    pub n_outputs: u32,
    pub n_samples: [u32; MAX_INPUT_DIMENSIONS],
    pub domain: [u32; MAX_INPUT_DIMENSIONS],
    pub opta: [u32; MAX_INPUT_DIMENSIONS],
    pub table: usize,
    /// `cmsInterpFunction` union — one fn pointer.
    pub interpolation: usize,
}

/// `cmsInterpFunction` union — one member is live at a time.
#[derive(Clone, Copy)]
pub enum Interpolation {
    Lerp16(InterpFn16),
    LerpFloat(InterpFnFloat),
}

/// Which built-in kernel `interpolation` holds — resolved ONCE at params
/// construction so `eval16`/`eval_float` match a tag instead of running a
/// `fn_addr_eq` chain per pixel. `Other` → indirect call like C.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum KernelKind {
    Eval4, Tetra3, Trilinear, Bilinear, Eval1, LinLerp1D, Other16,
    Eval4F, Tetra3F, TrilinearF, BilinearF, Eval1F, LinLerp1DF, OtherF,
}

pub(crate) fn kernel_kind(interp: Interpolation) -> KernelKind {
    use std::ptr::fn_addr_eq;
    match interp {
        Interpolation::Lerp16(f) => {
            if fn_addr_eq(f, eval4_inputs as InterpFn16) { KernelKind::Eval4 }
            else if fn_addr_eq(f, tetrahedral_interp16 as InterpFn16) { KernelKind::Tetra3 }
            else if fn_addr_eq(f, trilinear_interp16 as InterpFn16) { KernelKind::Trilinear }
            else if fn_addr_eq(f, bilinear_interp16 as InterpFn16) { KernelKind::Bilinear }
            else if fn_addr_eq(f, eval1_input as InterpFn16) { KernelKind::Eval1 }
            else if fn_addr_eq(f, lin_lerp_1d as InterpFn16) { KernelKind::LinLerp1D }
            else { KernelKind::Other16 }
        }
        Interpolation::LerpFloat(f) => {
            if fn_addr_eq(f, eval4_inputs_float as InterpFnFloat) { KernelKind::Eval4F }
            else if fn_addr_eq(f, tetrahedral_interp_float as InterpFnFloat) { KernelKind::Tetra3F }
            else if fn_addr_eq(f, trilinear_interp_float as InterpFnFloat) { KernelKind::TrilinearF }
            else if fn_addr_eq(f, bilinear_interp_float as InterpFnFloat) { KernelKind::BilinearF }
            else if fn_addr_eq(f, eval1_input_float as InterpFnFloat) { KernelKind::Eval1F }
            else if fn_addr_eq(f, lin_lerp_1d_float as InterpFnFloat) { KernelKind::LinLerp1DF }
            else { KernelKind::OtherF }
        }
    }
}

/// Foreign-kernel plumbing — registered by `lcms2-ffi` (which may use
/// `unsafe`; this crate forbids it). When `InterpParams::foreign` is set,
/// `eval16`/`eval_float` dispatch through these hooks instead of the
/// built-in kernel — mirroring C where `p->Interpolation` is simply the
/// plugin's fn pointer.
pub type ForeignKernel16 = fn(&[u16], &mut [u16], &CmsInterpParams);
pub type ForeignKernelFloat = fn(&[f32], &mut [f32], &CmsInterpParams);
/// `cmsInterpFnFactory` — returns the `cmsInterpFunction` union (usize)
/// or 0; the extra arg is the factory's own address.
pub type ForeignFactory = fn(usize, u32, u32, u32) -> usize;

#[derive(Clone, Copy)]
pub struct ForeignHooks {
    pub factory: ForeignFactory,
    pub kernel16: ForeignKernel16,
    pub kernel_float: ForeignKernelFloat,
}
static FOREIGN_HOOKS: std::sync::OnceLock<ForeignHooks> = std::sync::OnceLock::new();
pub fn install_foreign_hooks(h: ForeignHooks) {
    let _ = FOREIGN_HOOKS.set(h);
}
pub(crate) fn foreign_hooks() -> Option<ForeignHooks> {
    FOREIGN_HOOKS.get().copied()
}

/// `X64V3Token::summon()` behind a `OnceLock` — the cpuid check runs
/// once per process, so per-pixel kernel dispatch costs a load.
#[inline]
pub(crate) fn x64v3_cached() -> Option<archmage::X64V3Token> {
    if std::env::var_os("LCMS_DISABLE_V3").is_some() { return None; }
    use archmage::SimdToken;
    static TOK: std::sync::OnceLock<Option<archmage::X64V3Token>> =
        std::sync::OnceLock::new();
    *TOK.get_or_init(archmage::X64V3Token::summon)
}

/// The borrowed LUT a params struct points at.
#[derive(Clone)]
pub enum InterpTable<'a> {
    U16(&'a [u16]),
    F32(&'a [f32]),
    /// Owned `Rc` table — makes `InterpParams<'static>` storable (C bakes
    /// `cmsInterpParams` inside the stage data; this is the safe-Rust shape).
    U16Rc(std::sync::Arc<[u16]>),
    F32Rc(std::sync::Arc<[f32]>),
}

/// `cmsInterpParams` — `ContextID` lives on the constructor call in Rust;
/// the params keep no context (C stores it only for `_cmsFreeInterpParams`).
#[derive(Clone)]
pub struct InterpParams<'a> {
    pub dw_flags: u32,
    pub n_inputs: u32,
    pub n_outputs: u32,
    pub n_samples: [u32; MAX_INPUT_DIMENSIONS],
    pub domain: [u32; MAX_INPUT_DIMENSIONS],
    pub opta: [u32; MAX_INPUT_DIMENSIONS],
    pub table: InterpTable<'a>,
    pub interpolation: Interpolation,
    /// Foreign (C-plugin) kernel — `Some` when a `cmsPluginTHR`-registered
    /// `cmsInterpFnFactory` supplied the kernel; `interpolation` stays the
    /// built-in default for any consumer that doesn't consult `foreign`.
    pub foreign: Option<Box<CmsInterpParams>>,
    /// Resolved built-in kernel tag — see `kernel_kind`; `eval16`/`eval_float`
    /// match on this instead of sniffing `interpolation`'s address per call.
    pub kernel: KernelKind,
}

impl<'a> InterpParams<'a> {
    #[inline]
    fn table_u16(&self) -> &[u16] {
        match &self.table {
            InterpTable::U16(t) => t,
            InterpTable::U16Rc(t) => t,
            _ => unreachable!("u16 interp on float table"),
        }
    }
    #[inline]
    fn table_f32(&self) -> &[f32] {
        match &self.table {
            InterpTable::F32(t) => t,
            InterpTable::F32Rc(t) => t,
            _ => unreachable!("float interp on u16 table"),
        }
    }
    /// `p->Interpolation.Lerp16(input, output, p)` — panics if the bound
    /// kernel is the float one, like C would crash on a null call.
    #[inline]
    pub fn eval16(&self, input: &[u16], output: &mut [u16]) {
        if let Some(fb) = &self.foreign {
            if let Some(h) = foreign_hooks() {
                (h.kernel16)(input, output, fb);
                return;
            }
        }
        match self.kernel {
            KernelKind::Eval4 => {
                // 4-in/4-out eval4 → SIMD kernel (bit-exact lane math).
                if self.n_inputs == 4 && self.n_outputs == 4 && output.len() >= 4
                {
                    #[cfg(target_arch = "x86_64")]
                    if let Some(t) = x64v3_cached() {
                        return eval16_eval4_v3(t, input, output, self);
                    }
                }
                eval4_inputs(input, output, self)
            }
            KernelKind::Tetra3 => {
                // 3-in/3-out tetra → SIMD kernel (bit-exact lane math).
                if self.n_outputs == 3 && output.len() >= 3 {
                    #[cfg(target_arch = "x86_64")]
                    if let Some(t) = x64v3_cached() {
                        return eval16_tetra3_v3(t, input, output, self);
                    }
                }
                tetrahedral_interp16(input, output, self)
            }
            KernelKind::Trilinear => {
                let n_out = self.n_outputs as usize;
                if (1..=4).contains(&n_out) && output.len() >= n_out {
                    #[cfg(target_arch = "x86_64")]
                    if let Some(t) = x64v3_cached() {
                        return eval16_trilinear_v3(t, input, output, self);
                    }
                }
                trilinear_interp16(input, output, self)
            }
            KernelKind::Bilinear => {
                let n_out = self.n_outputs as usize;
                if (1..=4).contains(&n_out) && output.len() >= n_out {
                    #[cfg(target_arch = "x86_64")]
                    if let Some(t) = x64v3_cached() {
                        return eval16_bilinear_v3(t, input, output, self);
                    }
                }
                bilinear_interp16(input, output, self)
            }
            KernelKind::Eval1 => {
                let n_out = self.n_outputs as usize;
                if (1..=4).contains(&n_out) && output.len() >= n_out {
                    #[cfg(target_arch = "x86_64")]
                    if let Some(t) = x64v3_cached() {
                        return eval16_eval1_v3(t, input, output, self);
                    }
                }
                eval1_input(input, output, self)
            }
            KernelKind::LinLerp1D => lin_lerp_1d(input, output, self),
            _ => {
                let Interpolation::Lerp16(f) = self.interpolation else {
                    unreachable!("Lerp16 on float params")
                };
                f(input, output, self)
            }
        }
    }
    /// `p->Interpolation.LerpFloat(input, output, p)`
    #[inline]
    pub fn eval_float(&self, input: &[f32], output: &mut [f32]) {
        if let Some(fb) = &self.foreign {
            if let Some(h) = foreign_hooks() {
                (h.kernel_float)(input, output, fb);
                return;
            }
        }
        match self.kernel {
            KernelKind::Eval4F => {
                let n_out = self.n_outputs as usize;
                if (1..=4).contains(&n_out) && output.len() >= n_out {
                    #[cfg(target_arch = "x86_64")]
                    if let Some(t) = x64v3_cached() {
                        return evalflt_eval4_v3(t, input, output, n_out, self);
                    }
                }
                eval4_inputs_float(input, output, self)
            }
            KernelKind::Tetra3F => {
                let n_out = self.n_outputs as usize;
                if (1..=4).contains(&n_out) && output.len() >= n_out {
                    #[cfg(target_arch = "x86_64")]
                    if let Some(t) = x64v3_cached() {
                        return evalflt_tetra3_v3(t, input, output, n_out, self);
                    }
                }
                tetrahedral_interp_float(input, output, self)
            }
            KernelKind::TrilinearF => {
                let n_out = self.n_outputs as usize;
                if (1..=4).contains(&n_out) && output.len() >= n_out {
                    #[cfg(target_arch = "x86_64")]
                    if let Some(t) = x64v3_cached() {
                        return evalflt_trilinear_v3(t, input, output, n_out, self);
                    }
                }
                trilinear_interp_float(input, output, self)
            }
            KernelKind::BilinearF => {
                let n_out = self.n_outputs as usize;
                if (1..=4).contains(&n_out) && output.len() >= n_out {
                    #[cfg(target_arch = "x86_64")]
                    if let Some(t) = x64v3_cached() {
                        return evalflt_bilinear_v3(t, input, output, n_out, self);
                    }
                }
                bilinear_interp_float(input, output, self)
            }
            KernelKind::Eval1F => {
                let n_out = self.n_outputs as usize;
                if (1..=4).contains(&n_out) && output.len() >= n_out {
                    #[cfg(target_arch = "x86_64")]
                    if let Some(t) = x64v3_cached() {
                        return evalflt_eval1_v3(t, input, output, n_out, self);
                    }
                }
                eval1_input_float(input, output, self)
            }
            KernelKind::LinLerp1DF => lin_lerp_1d_float(input, output, self),
            _ => {
                let Interpolation::LerpFloat(f) = self.interpolation else {
                    unreachable!("LerpFloat on u16 params")
                };
                f(input, output, self)
            }
        }
    }
    /// Build the C-layout params mirror handed to a foreign kernel.
    fn to_c_params(&self) -> Box<CmsInterpParams> {
        Box::new(CmsInterpParams {
            context_id: 0,
            dw_flags: self.dw_flags,
            n_inputs: self.n_inputs,
            n_outputs: self.n_outputs,
            n_samples: self.n_samples,
            domain: self.domain,
            opta: self.opta,
            table: match &self.table {
                InterpTable::U16(t) => t.as_ptr() as usize,
                InterpTable::F32(t) => t.as_ptr() as usize,
                InterpTable::U16Rc(t) => t.as_ptr() as usize,
                InterpTable::F32Rc(t) => t.as_ptr() as usize,
            },
            interpolation: 0,
        })
    }
}

/// `_cmsSetInterpolationRoutine` — invoke the plugin factory if set, else the
/// default factory; error path matches C (`ERROR_UNKNOWN_EXTENSION` upstream).
pub fn set_interpolation_routine(ctx: Option<&Context>, p: &mut InterpParams) -> bool {
    let c = or_global(ctx);

    // FFI-registered C factories (`cmsPluginTHR` inpH) — consulted like C's
    // `Interpolators` chain; first factory returning non-NULL wins.
    let mut interp: Option<Interpolation> = None;
    if let Some(hooks) = foreign_hooks() {
        for &f in &c.interp_factories {
            let r = (hooks.factory)(f, p.n_inputs, p.n_outputs, p.dw_flags);
            if r != 0 {
                let mut cp = p.to_c_params();
                cp.interpolation = r;
                p.foreign = Some(cp);
                break;
            }
        }
    }
    if let Some(factory) = c.interpolators {
        interp = factory(p.n_inputs, p.n_outputs, p.dw_flags);
    }

    // If unsupported by the plug-in, go for the LittleCMS default
    if interp.is_none() {
        interp = default_interpolators_factory(p.n_inputs, p.n_outputs, p.dw_flags);
    }

    match interp {
        Some(i) => {
            p.interpolation = i;
            p.kernel = kernel_kind(i);
            true
        }
        None => false,
    }
}

/// `_cmsComputeInterpParamsEx` — per-dimension node counts.
pub fn compute_interp_params_ex<'a>(
    ctx: Option<&Context>,
    n_samples: &[u32],
    input_chan: u32,
    output_chan: u32,
    table: InterpTable<'a>,
    dw_flags: u32,
) -> Option<InterpParams<'a>> {
    // Check for maximum inputs
    if input_chan as usize > MAX_INPUT_DIMENSIONS {
        err::signal_error(ctx, ERROR_RANGE, "Too many input channels (max=15)");
        return None;
    }

    let mut p = InterpParams {
        dw_flags,
        n_inputs: input_chan,
        n_outputs: output_chan,
        n_samples: [0; MAX_INPUT_DIMENSIONS],
        domain: [0; MAX_INPUT_DIMENSIONS],
        opta: [0; MAX_INPUT_DIMENSIONS],
        table,
        interpolation: Interpolation::Lerp16(|_, _, _| {}),
            kernel: kernel_kind(Interpolation::Lerp16(|_, _, _| {})),
        foreign: None,
    };

    // Fill samples per input direction and domain (nodes minus one).
    // C computes these on u32 — wrap like C (parametric curves pass 0 nodes).
    for i in 0..input_chan as usize {
        p.n_samples[i] = n_samples[i];
        p.domain[i] = n_samples[i].wrapping_sub(1);
    }

    // Factors to apply to each component to index the grid array
    p.opta[0] = p.n_outputs;
    for i in 1..input_chan as usize {
        p.opta[i] = p.opta[i - 1].wrapping_mul(n_samples[input_chan as usize - i]);
    }

    if !set_interpolation_routine(ctx, &mut p) {
        err::signal_error(
            ctx,
            ERROR_UNKNOWN_EXTENSION,
            "Unsupported interpolation (channels)",
        );
        return None;
    }

    Some(p)
}

/// `_cmsComputeInterpParams` — same nodes in every direction.
pub fn compute_interp_params<'a>(
    ctx: Option<&Context>,
    n_samples: u32,
    input_chan: u32,
    output_chan: u32,
    table: InterpTable<'a>,
    dw_flags: u32,
) -> Option<InterpParams<'a>> {
    let samples = [n_samples; MAX_INPUT_DIMENSIONS];
    compute_interp_params_ex(ctx, &samples, input_chan, output_chan, table, dw_flags)
}

// ---------------------------------------------------------------------------
// Interpolation kernels
// ---------------------------------------------------------------------------

/// Bounds-safe table read. C dereferences `LutTable[cell + delta]` even when
/// the delta offset points past the table end (single-node dimensions, domain
/// 0). Every such read is multiplied by a zero rest component, so the value is
/// dead — substituting 0 preserves the output bit-for-bit while the port stays
/// in bounds.
#[inline]
pub(crate) fn tbl_get16(t: &[u16], i: usize) -> i32 {
    t.get(i).copied().unwrap_or(0) as i32
}

/// Same for float tables.
#[inline]
fn tbl_getf(t: &[f32], i: usize) -> f32 {
    t.get(i).copied().unwrap_or(0.0)
}

/// `LinearInterp` — inline fixed-point interpolation.
#[inline]
pub(crate) fn linear_interp(a: i32, l: i32, h: i32) -> u16 {
    let dif = ((h - l) as u32).wrapping_mul(a as u32).wrapping_add(0x8000);
    ((dif >> 16).wrapping_add(l as u32)) as u16
}

/// `LinLerp1D` — 1in→1out linear interpolation, fixed-point optimized.
pub(crate) fn lin_lerp_1d(input: &[u16], output: &mut [u16], p: &InterpParams) {
    let lut_table = p.table_u16();

    // if last value or just one point
    if input[0] == 0xffff || p.domain[0] == 0 {
        output[0] = lut_table[p.domain[0] as usize];
    } else {
        let mut val3 = p.domain[0].wrapping_mul(input[0] as u32) as i32;
        val3 = to_fixed_domain(val3); // To fixed 15.16

        let cell0 = fixed_to_int(val3); // Cell is 16 MSB bits
        let rest = fixed_rest_to_int(val3); // Rest is 16 LSB bits

        let c = cell0.min(lut_table.len().saturating_sub(2) as i32).max(0) as usize;
        let t = &lut_table[c..c + 2];
        output[0] = linear_interp(rest, t[0] as i32, t[1] as i32);
    }
}

/// `fclamp` — prevent out-of-bounds indexing.
#[inline]
fn fclamp(v: f32) -> f32 {
    if v < 1.0e-9 || v.is_nan() {
        0.0
    } else if v > 1.0 {
        1.0
    } else {
        v
    }
}

/// `LinLerp1Dfloat` — floating-point version of 1D interpolation.
#[inline]
fn lin_lerp_1d_float(input: &[f32], output: &mut [f32], p: &InterpParams) {
    let lut_table = p.table_f32();

    let mut val2 = fclamp(input[0]);

    // if last value...
    if val2 == 1.0 || p.domain[0] == 0 {
        output[0] = lut_table[p.domain[0] as usize];
    } else {
        val2 *= p.domain[0] as f32;

        let cell0 = val2.floor() as i32;
        let cell1 = val2.ceil() as i32;

        // Rest is 16 LSB bits
        let rest = val2 - cell0 as f32;

        let m = lut_table.len();
        let y0 = lut_table[(cell0 as usize).min(m - 1)];
        let y1 = lut_table[(cell1 as usize).min(m - 1)];

        output[0] = y0 + (y1 - y0) * rest;
    }
}

/// `Eval1Input` — gray LUT, one input channel (fixed point).
pub(crate) fn eval1_input(input: &[u16], output: &mut [u16], p: &InterpParams) {
    let lut_table = p.table_u16();

    // if last value...
    if input[0] == 0xffff || p.domain[0] == 0 {
        let y0 = p.domain[0] * p.opta[0];
        for out_chan in 0..p.n_outputs as usize {
            output[out_chan] = lut_table[y0 as usize + out_chan];
        }
    } else {
        let v = (input[0] as u32).wrapping_mul(p.domain[0]) as i32;
        let fk = to_fixed_domain(v);

        let k0 = fixed_to_int(fk);
        let rk = fixed_rest_to_int(fk) as u16;

        let k1 = k0 + if input[0] != 0xFFFF { 1 } else { 0 };

        let n = p.n_outputs as usize;
        let m = lut_table.len() - n;
        let k0i = ((p.opta[0] as i32) * k0).clamp(0, m as i32) as usize;
        let k1i = ((p.opta[0] as i32) * k1).clamp(0, m as i32) as usize;
        let d0 = &lut_table[k0i..k0i + n];
        let d1 = &lut_table[k1i..k1i + n];
        let output = &mut output[..n];

        for out_chan in 0..n {
            output[out_chan] = linear_interp(
                rk as i32,
                d0[out_chan] as i32,
                d1[out_chan] as i32,
            );
        }
    }
}

/// `Eval1InputFloat` — gray LUT, one input channel (float).
#[inline]
fn eval1_input_float(input: &[f32], output: &mut [f32], p: &InterpParams) {
    let lut_table = p.table_f32();

    let mut val2 = fclamp(input[0]);

    // if last value...
    if val2 == 1.0 || p.domain[0] == 0 {
        let start = p.domain[0] * p.opta[0];
        for out_chan in 0..p.n_outputs as usize {
            output[out_chan] = lut_table[start as usize + out_chan];
        }
    } else {
        val2 *= p.domain[0] as f32;

        let cell0 = val2.floor() as i32;
        let cell1 = val2.ceil() as i32;

        let rest = val2 - cell0 as f32;

        let cell0 = cell0 * p.opta[0] as i32;
        let cell1 = cell1 * p.opta[0] as i32;

        let n = p.n_outputs as usize;
        let m = lut_table.len() - n;
        let c0 = (cell0 as usize).min(m);
        let c1 = (cell1 as usize).min(m);
        let d0 = &lut_table[c0..c0 + n];
        let d1 = &lut_table[c1..c1 + n];
        let output = &mut output[..n];
        for out_chan in 0..n {
            let y0 = d0[out_chan];
            let y1 = d1[out_chan];

            output[out_chan] = y0 + (y1 - y0) * rest;
        }
    }
}

/// `BilinearInterpFloat` — bilinear interpolation, float version.
#[inline]
fn bilinear_interp_float(input: &[f32], output: &mut [f32], p: &InterpParams) {
    let lut_table = p.table_f32();
    let total_out = p.n_outputs as usize;

    let px = fclamp(input[0]) * p.domain[0] as f32;
    let py = fclamp(input[1]) * p.domain[1] as f32;

    let x0 = px.floor() as i32;
    let fx = px - x0 as f32;
    let y0 = py.floor() as i32;
    let fy = py - y0 as f32;

    let x0i = (p.opta[1] as i32).wrapping_mul(x0);
    let x1 = x0i
        + if fclamp(input[0]) >= 1.0 {
            0
        } else {
            p.opta[1] as i32
        };

    let y0i = (p.opta[0] as i32).wrapping_mul(y0);
    let y1 = y0i
        + if fclamp(input[1]) >= 1.0 {
            0
        } else {
            p.opta[0] as i32
        };

    let n = total_out;
    let m = lut_table.len() - n;
    let w = |off: i32| &lut_table[((off as usize).min(m))..][..n];
    let d00 = w(x0i + y0i);
    let d01 = w(x0i + y1);
    let d10 = w(x1 + y0i);
    let d11 = w(x1 + y1);
    let lerp = |a: f32, l: f32, h: f32| l + ((h - l) * a);
    for out_chan in 0..n {
        let dx0 = lerp(fx, d00[out_chan], d10[out_chan]);
        let dx1 = lerp(fx, d01[out_chan], d11[out_chan]);

        output[out_chan] = lerp(fy, dx0, dx1);
    }
}

/// `BilinearInterp16` — bilinear interpolation, fixed-point version.
pub(crate) fn bilinear_interp16(input: &[u16], output: &mut [u16], p: &InterpParams) {
    let lut_table = p.table_u16();
    let total_out = p.n_outputs as usize;

    let fx = to_fixed_domain((input[0] as i32).wrapping_mul(p.domain[0] as i32));
    let x0 = fixed_to_int(fx);
    let rx = fixed_rest_to_int(fx);

    let fy = to_fixed_domain((input[1] as i32).wrapping_mul(p.domain[1] as i32));
    let y0 = fixed_to_int(fy);
    let ry = fixed_rest_to_int(fy);

    let x0i = (p.opta[1] as i32).wrapping_mul(x0);
    let x1 = x0i
        + if input[0] == 0xFFFF {
            0
        } else {
            p.opta[1] as i32
        };

    let y0i = (p.opta[0] as i32).wrapping_mul(y0);
    let y1 = y0i
        + if input[1] == 0xFFFF {
            0
        } else {
            p.opta[0] as i32
        };

    let n = total_out;
    let m = lut_table.len() - n;
    let w = |off: i32| &lut_table[((off as usize).min(m))..][..n];
    let d00 = w(x0i + y0i);
    let d01 = w(x0i + y1);
    let d10 = w(x1 + y0i);
    let d11 = w(x1 + y1);
    let lerp =
        |a: i32, l: i32, h: i32| (l + round_fixed_to_int((h - l).wrapping_mul(a))) as u16;
    for out_chan in 0..n {
        let dx0 = lerp(rx, d00[out_chan] as i32, d10[out_chan] as i32) as i32;
        let dx1 = lerp(rx, d01[out_chan] as i32, d11[out_chan] as i32) as i32;

        output[out_chan] = lerp(ry, dx0, dx1);
    }
}

/// `TrilinearInterpFloat` — trilinear, float.
#[inline]
pub(crate) fn trilinear_interp_float(input: &[f32], output: &mut [f32], p: &InterpParams) {
    let lut_table = p.table_f32();
    let total_out = p.n_outputs as usize;

    // We need some clipping here
    let px = fclamp(input[0]) * p.domain[0] as f32;
    let py = fclamp(input[1]) * p.domain[1] as f32;
    let pz = fclamp(input[2]) * p.domain[2] as f32;

    let x0 = px.floor() as i32;
    let fx = px - x0 as f32;
    let y0 = py.floor() as i32;
    let fy = py - y0 as f32;
    let z0 = pz.floor() as i32;
    let fz = pz - z0 as f32;

    let x0i = (p.opta[2] as i32).wrapping_mul(x0);
    let x1 = x0i
        + if fclamp(input[0]) >= 1.0 {
            0
        } else {
            p.opta[2] as i32
        };

    let y0i = (p.opta[1] as i32).wrapping_mul(y0);
    let y1 = y0i
        + if fclamp(input[1]) >= 1.0 {
            0
        } else {
            p.opta[1] as i32
        };

    let z0i = (p.opta[0] as i32).wrapping_mul(z0);
    let z1 = z0i
        + if fclamp(input[2]) >= 1.0 {
            0
        } else {
            p.opta[0] as i32
        };

    let n = total_out;
    let m = lut_table.len() - n;
    let w = |off: i32| &lut_table[((off as usize).min(m))..][..n];
    let d000 = w(x0i + y0i + z0i);
    let d001 = w(x0i + y0i + z1);
    let d010 = w(x0i + y1 + z0i);
    let d011 = w(x0i + y1 + z1);
    let d100 = w(x1 + y0i + z0i);
    let d101 = w(x1 + y0i + z1);
    let d110 = w(x1 + y1 + z0i);
    let d111 = w(x1 + y1 + z1);
    let lerp = |a: f32, l: f32, h: f32| l + ((h - l) * a);
    for out_chan in 0..n {
        let dx00 = lerp(fx, d000[out_chan], d100[out_chan]);
        let dx01 = lerp(fx, d001[out_chan], d101[out_chan]);
        let dx10 = lerp(fx, d010[out_chan], d110[out_chan]);
        let dx11 = lerp(fx, d011[out_chan], d111[out_chan]);

        let dxy0 = lerp(fy, dx00, dx10);
        let dxy1 = lerp(fy, dx01, dx11);

        output[out_chan] = lerp(fz, dxy0, dxy1);
    }
}

/// `TrilinearInterp16` — trilinear, fixed point.
#[inline]
pub(crate) fn trilinear_interp16(input: &[u16], output: &mut [u16], p: &InterpParams) {
    let lut_table = p.table_u16();
    let total_out = p.n_outputs as usize;

    let fx = to_fixed_domain((input[0] as i32).wrapping_mul(p.domain[0] as i32));
    let x0 = fixed_to_int(fx);
    let rx = fixed_rest_to_int(fx);

    let fy = to_fixed_domain((input[1] as i32).wrapping_mul(p.domain[1] as i32));
    let y0 = fixed_to_int(fy);
    let ry = fixed_rest_to_int(fy);

    let fz = to_fixed_domain((input[2] as i32).wrapping_mul(p.domain[2] as i32));
    let z0 = fixed_to_int(fz);
    let rz = fixed_rest_to_int(fz);

    let x0i = (p.opta[2] as i32).wrapping_mul(x0);
    let x1 = x0i
        + if input[0] == 0xFFFF {
            0
        } else {
            p.opta[2] as i32
        };

    let y0i = (p.opta[1] as i32).wrapping_mul(y0);
    let y1 = y0i
        + if input[1] == 0xFFFF {
            0
        } else {
            p.opta[1] as i32
        };

    let z0i = (p.opta[0] as i32).wrapping_mul(z0);
    let z1 = z0i
        + if input[2] == 0xFFFF {
            0
        } else {
            p.opta[0] as i32
        };

    let max_off = x1.wrapping_add(y1).wrapping_add(z1);
    let cell = lut_table;
    // Re-slice each corner into a fixed-span window — one clamp per offset
    // instead of `.min(max_off)` + `.min(last)` on every `8×n` read.
    let n = total_out;
    let m = cell.len() - n;
    let w = |off: i32| &cell[(off.min(max_off) as usize).min(m)..][..n];
    let d000 = w(x0i + y0i + z0i);
    let d001 = w(x0i + y0i + z1);
    let d010 = w(x0i + y1 + z0i);
    let d011 = w(x0i + y1 + z1);
    let d100 = w(x1 + y0i + z0i);
    let d101 = w(x1 + y0i + z1);
    let d110 = w(x1 + y1 + z0i);
    let d111 = w(x1 + y1 + z1);
    let lerp = |a: i32, l: i32, h: i32| {
        (l + round_fixed_to_int((h - l).wrapping_mul(a))) as u16
    };
    for out_chan in 0..n {
        let dx00 = lerp(rx, d000[out_chan] as i32, d100[out_chan] as i32) as i32;
        let dx01 = lerp(rx, d001[out_chan] as i32, d101[out_chan] as i32) as i32;
        let dx10 = lerp(rx, d010[out_chan] as i32, d110[out_chan] as i32) as i32;
        let dx11 = lerp(rx, d011[out_chan] as i32, d111[out_chan] as i32) as i32;

        let dxy0 = lerp(ry, dx00, dx10) as i32;
        let dxy1 = lerp(ry, dx01, dx11) as i32;

        output[out_chan] = lerp(rz, dxy0, dxy1);
    }
}

/// `TetrahedralInterpFloat` — Sakamoto tetrahedral interpolation, float.
#[inline]
pub(crate) fn tetrahedral_interp_float(input: &[f32], output: &mut [f32], p: &InterpParams) {
    let lut_table = p.table_f32();
    let total_out = p.n_outputs as usize;

    // We need some clipping here
    let px = fclamp(input[0]) * p.domain[0] as f32;
    let py = fclamp(input[1]) * p.domain[1] as f32;
    let pz = fclamp(input[2]) * p.domain[2] as f32;

    let x0 = px.floor() as i32;
    let rx = px - x0 as f32;
    let y0 = py.floor() as i32;
    let ry = py - y0 as f32;
    let z0 = pz.floor() as i32;
    let rz = pz - z0 as f32;

    let x0i = (p.opta[2] as i32).wrapping_mul(x0);
    let x1 = x0i
        + if fclamp(input[0]) >= 1.0 {
            0
        } else {
            p.opta[2] as i32
        };

    let y0i = (p.opta[1] as i32).wrapping_mul(y0);
    let y1 = y0i
        + if fclamp(input[1]) >= 1.0 {
            0
        } else {
            p.opta[1] as i32
        };

    let z0i = (p.opta[0] as i32).wrapping_mul(z0);
    let z1 = z0i
        + if fclamp(input[2]) >= 1.0 {
            0
        } else {
            p.opta[0] as i32
        };

    let n = total_out;
    let m = lut_table.len() - n;
    let w = |off: i32| &lut_table[((off as usize).min(m))..][..n];
    let d000 = w(x0i + y0i + z0i);
    let d001 = w(x0i + y0i + z1);
    let d010 = w(x0i + y1 + z0i);
    let d011 = w(x0i + y1 + z1);
    let d100 = w(x1 + y0i + z0i);
    let d101 = w(x1 + y0i + z1);
    let d110 = w(x1 + y1 + z0i);
    let d111 = w(x1 + y1 + z1);
    let output = &mut output[..n];

    for out_chan in 0..n {
        let c0 = d000[out_chan];
        let (c1, c2, c3);

        if rx >= ry && ry >= rz {
            c1 = d100[out_chan] - c0;
            c2 = d110[out_chan] - d100[out_chan];
            c3 = d111[out_chan] - d110[out_chan];
        } else if rx >= rz && rz >= ry {
            c1 = d100[out_chan] - c0;
            c2 = d111[out_chan] - d101[out_chan];
            c3 = d101[out_chan] - d100[out_chan];
        } else if rz >= rx && rx >= ry {
            c1 = d101[out_chan] - d001[out_chan];
            c2 = d111[out_chan] - d101[out_chan];
            c3 = d001[out_chan] - c0;
        } else if ry >= rx && rx >= rz {
            c1 = d110[out_chan] - d010[out_chan];
            c2 = d010[out_chan] - c0;
            c3 = d111[out_chan] - d110[out_chan];
        } else if ry >= rz && rz >= rx {
            c1 = d111[out_chan] - d011[out_chan];
            c2 = d010[out_chan] - c0;
            c3 = d011[out_chan] - d010[out_chan];
        } else if rz >= ry && ry >= rx {
            c1 = d111[out_chan] - d011[out_chan];
            c2 = d011[out_chan] - d001[out_chan];
            c3 = d001[out_chan] - c0;
        } else {
            c1 = 0.0;
            c2 = 0.0;
            c3 = 0.0;
        }

        output[out_chan] = c0 + c1 * rx + c2 * ry + c3 * rz;
    }
}

/// `TetrahedralInterp16` — Sakamoto tetrahedral interpolation, fixed point.
///
/// The output formula is a documented shortcut:
/// `x = (Rest + ((Rest+0x7fff)/0xFFFF) + 0x8000) >> 16` replaced by
/// `t = Rest+0x8001; x = (t + (t>>16)) >> 16` — off by one at 0x7fff/0x17ffe,
/// which is C's actual behaviour and is preserved.
#[inline]
pub(crate) fn tetrahedral_interp16(input: &[u16], output: &mut [u16], p: &InterpParams) {
    let lut_table = p.table_u16();
    let mut total_out = p.n_outputs as usize;

    let fx = to_fixed_domain((input[0] as i32).wrapping_mul(p.domain[0] as i32));
    let fy = to_fixed_domain((input[1] as i32).wrapping_mul(p.domain[1] as i32));
    let fz = to_fixed_domain((input[2] as i32).wrapping_mul(p.domain[2] as i32));

    let x0 = fixed_to_int(fx);
    let y0 = fixed_to_int(fy);
    let z0 = fixed_to_int(fz);

    let rx = fixed_rest_to_int(fx);
    let ry = fixed_rest_to_int(fy);
    let rz = fixed_rest_to_int(fz);

    let x0i = p.opta[2].wrapping_mul(x0 as u32);
    let mut x1 = if input[0] == 0xFFFF { 0 } else { p.opta[2] };

    let y0i = p.opta[1].wrapping_mul(y0 as u32);
    let mut y1 = if input[1] == 0xFFFF { 0 } else { p.opta[1] };

    let z0i = p.opta[0].wrapping_mul(z0 as u32);
    let mut z1 = if input[2] == 0xFFFF { 0 } else { p.opta[0] };

    let base = (x0i + y0i + z0i) as usize;
    let _li = base; // index into lut_table, incremented per output channel
    let mut oi = 0usize;

    if rx >= ry {
        if ry >= rz {
            y1 += x1;
            z1 += y1;
            let cell = &lut_table[base..];
            let last = cell.len() - 1;
            while total_out > 0 {
                total_out -= 1;
                let c1 = cell[(oi + x1 as usize).min(last)] as i32;
                let mut c2 = cell[(oi + y1 as usize).min(last)] as i32;
                let mut c3 = cell[(oi + z1 as usize).min(last)] as i32;
                let c0 = cell[oi.min(last)] as i32;
                c3 -= c2;
                c2 -= c1;
                let c1 = c1 - c0;
                let rest = c1
                    .wrapping_mul(rx)
                    .wrapping_add(c2.wrapping_mul(ry))
                    .wrapping_add(c3.wrapping_mul(rz))
                    .wrapping_add(0x8001);
                output[oi] = (c0 as u16).wrapping_add((rest.wrapping_add(rest >> 16) >> 16) as u16);
                oi += 1;
            }
        } else if rz >= rx {
            x1 += z1;
            y1 += x1;
            let cell = &lut_table[base..];
            let last = cell.len() - 1;
            while total_out > 0 {
                total_out -= 1;
                let mut c1 = cell[(oi + x1 as usize).min(last)] as i32;
                let mut c2 = cell[(oi + y1 as usize).min(last)] as i32;
                let mut c3 = cell[(oi + z1 as usize).min(last)] as i32;
                let c0 = cell[oi.min(last)] as i32;
                c2 -= c1;
                c1 -= c3;
                c3 -= c0;
                let rest = c1
                    .wrapping_mul(rx)
                    .wrapping_add(c2.wrapping_mul(ry))
                    .wrapping_add(c3.wrapping_mul(rz))
                    .wrapping_add(0x8001);
                output[oi] = (c0 as u16).wrapping_add((rest.wrapping_add(rest >> 16) >> 16) as u16);
                oi += 1;
            }
        } else {
            z1 += x1;
            y1 += z1;
            let cell = &lut_table[base..];
            let last = cell.len() - 1;
            while total_out > 0 {
                total_out -= 1;
                let c1 = cell[(oi + x1 as usize).min(last)] as i32;
                let mut c2 = cell[(oi + y1 as usize).min(last)] as i32;
                let mut c3 = cell[(oi + z1 as usize).min(last)] as i32;
                let c0 = cell[oi.min(last)] as i32;
                c2 -= c3;
                c3 -= c1;
                let c1 = c1 - c0;
                let rest = c1
                    .wrapping_mul(rx)
                    .wrapping_add(c2.wrapping_mul(ry))
                    .wrapping_add(c3.wrapping_mul(rz))
                    .wrapping_add(0x8001);
                output[oi] = (c0 as u16).wrapping_add((rest.wrapping_add(rest >> 16) >> 16) as u16);
                oi += 1;
            }
        }
    } else {
        if rx >= rz {
            x1 += y1;
            z1 += x1;
            let cell = &lut_table[base..];
            let last = cell.len() - 1;
            while total_out > 0 {
                total_out -= 1;
                let mut c1 = cell[(oi + x1 as usize).min(last)] as i32;
                let mut c2 = cell[(oi + y1 as usize).min(last)] as i32;
                let mut c3 = cell[(oi + z1 as usize).min(last)] as i32;
                let c0 = cell[oi.min(last)] as i32;
                c3 -= c1;
                c1 -= c2;
                c2 -= c0;
                let rest = c1
                    .wrapping_mul(rx)
                    .wrapping_add(c2.wrapping_mul(ry))
                    .wrapping_add(c3.wrapping_mul(rz))
                    .wrapping_add(0x8001);
                output[oi] = (c0 as u16).wrapping_add((rest.wrapping_add(rest >> 16) >> 16) as u16);
                oi += 1;
            }
        } else if ry >= rz {
            z1 += y1;
            x1 += z1;
            let cell = &lut_table[base..];
            let last = cell.len() - 1;
            while total_out > 0 {
                total_out -= 1;
                let mut c1 = cell[(oi + x1 as usize).min(last)] as i32;
                let c2 = cell[(oi + y1 as usize).min(last)] as i32;
                let mut c3 = cell[(oi + z1 as usize).min(last)] as i32;
                let c0 = cell[oi.min(last)] as i32;
                c1 -= c3;
                c3 -= c2;
                let c2 = c2 - c0;
                let rest = c1
                    .wrapping_mul(rx)
                    .wrapping_add(c2.wrapping_mul(ry))
                    .wrapping_add(c3.wrapping_mul(rz))
                    .wrapping_add(0x8001);
                output[oi] = (c0 as u16).wrapping_add((rest.wrapping_add(rest >> 16) >> 16) as u16);
                oi += 1;
            }
        } else {
            y1 += z1;
            x1 += y1;
            let cell = &lut_table[base..];
            let last = cell.len() - 1;
            while total_out > 0 {
                total_out -= 1;
                let mut c1 = cell[(oi + x1 as usize).min(last)] as i32;
                let mut c2 = cell[(oi + y1 as usize).min(last)] as i32;
                let mut c3 = cell[(oi + z1 as usize).min(last)] as i32;
                let c0 = cell[oi.min(last)] as i32;
                c1 -= c2;
                c2 -= c3;
                c3 -= c0;
                let rest = c1
                    .wrapping_mul(rx)
                    .wrapping_add(c2.wrapping_mul(ry))
                    .wrapping_add(c3.wrapping_mul(rz))
                    .wrapping_add(0x8001);
                output[oi] = (c0 as u16).wrapping_add((rest.wrapping_add(rest >> 16) >> 16) as u16);
                oi += 1;
            }
        }
    }
}

/// Corner block for `tetra3_interp16`: four consecutive u16s as one u64.
/// Offsets beyond `cell.len()` clamp to the last element — matching the
/// scalar `.min(last)` reads; in-range blocks take the single-load path.
#[inline(always)]
pub(crate) fn corner_u64(cell: &[u16], off: usize, last: usize) -> u64 {
    match cell.get(off..off + 4) {
        Some(s) => {
            (s[0] as u64) | ((s[1] as u64) << 16) | ((s[2] as u64) << 32) | ((s[3] as u64) << 48)
        }
        None => {
            let l = |i: usize| cell[(off + i).min(last)] as u64;
            l(0) | (l(1) << 16) | (l(2) << 32) | (l(3) << 48)
        }
    }
}

/// `tetrahedral_interp16` specialized to 3 inputs / 3 outputs — the
/// fused-path kernel. Identical index math and 6-arm selection; the
/// per-corner blocks load as single u64s instead of per-channel u16s.
#[inline]
pub(crate) fn tetra3_interp16(vin: [u16; 3], p: &InterpParams) -> [u16; 3] {
    let lut_table = p.table_u16();

    let fx = to_fixed_domain((vin[0] as i32).wrapping_mul(p.domain[0] as i32));
    let fy = to_fixed_domain((vin[1] as i32).wrapping_mul(p.domain[1] as i32));
    let fz = to_fixed_domain((vin[2] as i32).wrapping_mul(p.domain[2] as i32));

    let x0 = fixed_to_int(fx);
    let y0 = fixed_to_int(fy);
    let z0 = fixed_to_int(fz);

    let rx = fixed_rest_to_int(fx);
    let ry = fixed_rest_to_int(fy);
    let rz = fixed_rest_to_int(fz);

    let x0i = p.opta[2].wrapping_mul(x0 as u32);
    let mut x1 = if vin[0] == 0xFFFF { 0 } else { p.opta[2] };
    let y0i = p.opta[1].wrapping_mul(y0 as u32);
    let mut y1 = if vin[1] == 0xFFFF { 0 } else { p.opta[1] };
    let z0i = p.opta[0].wrapping_mul(z0 as u32);
    let mut z1 = if vin[2] == 0xFFFF { 0 } else { p.opta[0] };

    let base = (x0i + y0i + z0i) as usize;
    let cell = &lut_table[base..];
    let last = cell.len() - 1;
    let mut out = [0u16; 3];

    // Per-arm: pick the 3 corner offsets, run the arm's diff chain on
    // [i32; 4] lane arrays (LLVM lowers element-wise array ops to a
    // single i32x4 op), finish with the shared lerp per lane.
    #[inline(always)]
    fn lanes(v: u64) -> [i32; 4] {
        [
            (v & 0xffff) as i32,
            ((v >> 16) & 0xffff) as i32,
            ((v >> 32) & 0xffff) as i32,
            (v >> 48) as i32,
        ]
    }
    #[inline(always)]
    fn sub(a: [i32; 4], b: [i32; 4]) -> [i32; 4] {
        [
            a[0].wrapping_sub(b[0]),
            a[1].wrapping_sub(b[1]),
            a[2].wrapping_sub(b[2]),
            a[3].wrapping_sub(b[3]),
        ]
    }
    #[inline(always)]
    fn mul_add(a: [i32; 4], k: i32, acc: [i32; 4]) -> [i32; 4] {
        [
            acc[0].wrapping_add(a[0].wrapping_mul(k)),
            acc[1].wrapping_add(a[1].wrapping_mul(k)),
            acc[2].wrapping_add(a[2].wrapping_mul(k)),
            acc[3].wrapping_add(a[3].wrapping_mul(k)),
        ]
    }

    macro_rules! arm {
        (|$c1:ident, $c2:ident, $c3:ident, $c0:ident| $chain:expr) => {{
            let $c0 = lanes(corner_u64(cell, 0, last));
            let $c1 = lanes(corner_u64(cell, x1 as usize, last));
            let $c2 = lanes(corner_u64(cell, y1 as usize, last));
            let $c3 = lanes(corner_u64(cell, z1 as usize, last));
            let ($c1, $c2, $c3) = $chain;
            let rest = mul_add($c3, rz, mul_add($c2, ry, mul_add($c1, rx, [0x8001; 4])));
            for oi in 0..3 {
                let r = rest[oi];
                out[oi] =
                    ($c0[oi] as u16).wrapping_add((r.wrapping_add(r >> 16) >> 16) as u16);
            }
        }};
    }

    if rx >= ry {
        if ry >= rz {
            y1 += x1;
            z1 += y1;
            arm!(|c1, c2, c3, c0| (sub(c1, c0), sub(c2, c1), sub(c3, c2)));
        } else if rz >= rx {
            x1 += z1;
            y1 += x1;
            arm!(|c1, c2, c3, c0| (sub(c1, c3), sub(c2, c1), sub(c3, c0)));
        } else {
            z1 += x1;
            y1 += z1;
            arm!(|c1, c2, c3, c0| (sub(c1, c0), sub(c2, c3), sub(c3, c1)));
        }
    } else {
        if rx >= rz {
            x1 += y1;
            z1 += x1;
            arm!(|c1, c2, c3, c0| (sub(c1, c2), sub(c2, c0), sub(c3, c1)));
        } else if ry >= rz {
            z1 += y1;
            x1 += z1;
            arm!(|c1, c2, c3, c0| (sub(c1, c3), sub(c2, c0), sub(c3, c2)));
        } else {
            y1 += z1;
            x1 += y1;
            arm!(|c1, c2, c3, c0| (sub(c1, c2), sub(c2, c3), sub(c3, c0)));
        }
    }
    out
}

/// `tetra3_interp16` over `magetypes::i32x4` — the four corner blocks as
/// vector lanes. Identical index math and per-arm diff chains; wrapping
/// i32 SIMD ops match the scalar `.wrapping_*` arithmetic exactly.
/// Called once per pixel inside `#[rite]`/`#[arcane]` loop fns.
#[inline(always)]
pub(crate) fn tetra3_i32x4<T: magetypes::simd::backends::I32x4Backend>(
    token: T,
    vin: [u16; 3],
    p: &InterpParams,
) -> [u16; 3] {
    let lut_table = p.table_u16();

    let fx = to_fixed_domain((vin[0] as i32).wrapping_mul(p.domain[0] as i32));
    let fy = to_fixed_domain((vin[1] as i32).wrapping_mul(p.domain[1] as i32));
    let fz = to_fixed_domain((vin[2] as i32).wrapping_mul(p.domain[2] as i32));

    let x0 = fixed_to_int(fx);
    let y0 = fixed_to_int(fy);
    let z0 = fixed_to_int(fz);

    let rx = fixed_rest_to_int(fx);
    let ry = fixed_rest_to_int(fy);
    let rz = fixed_rest_to_int(fz);

    let x0i = p.opta[2].wrapping_mul(x0 as u32);
    let mut x1 = if vin[0] == 0xFFFF { 0 } else { p.opta[2] };
    let y0i = p.opta[1].wrapping_mul(y0 as u32);
    let mut y1 = if vin[1] == 0xFFFF { 0 } else { p.opta[1] };
    let z0i = p.opta[0].wrapping_mul(z0 as u32);
    let mut z1 = if vin[2] == 0xFFFF { 0 } else { p.opta[0] };

    let base = (x0i + y0i + z0i) as usize;
    let cell = &lut_table[base..];
    let last = cell.len() - 1;
    let mut out = [0u16; 3];

    let rxv = magetypes::simd::generic::i32x4::<T>::splat_t(token, rx);
    let ryv = magetypes::simd::generic::i32x4::<T>::splat_t(token, ry);
    let rzv = magetypes::simd::generic::i32x4::<T>::splat_t(token, rz);
    let k = magetypes::simd::generic::i32x4::<T>::splat_t(token, 0x8001);

    macro_rules! arm {
        (|$c1:ident, $c2:ident, $c3:ident, $c0:ident| $chain:expr) => {{
            let $c0 = magetypes::simd::generic::i32x4::<T>::from_array_t(
                token,
                lanes_u64(corner_u64(cell, 0, last)),
            );
            let $c1 = magetypes::simd::generic::i32x4::<T>::from_array_t(
                token,
                lanes_u64(corner_u64(cell, x1 as usize, last)),
            );
            let $c2 = magetypes::simd::generic::i32x4::<T>::from_array_t(
                token,
                lanes_u64(corner_u64(cell, y1 as usize, last)),
            );
            let $c3 = magetypes::simd::generic::i32x4::<T>::from_array_t(
                token,
                lanes_u64(corner_u64(cell, z1 as usize, last)),
            );
            let ($c1, $c2, $c3) = $chain;
            let rest = $c1 * rxv + $c2 * ryv + $c3 * rzv + k;
            let rest = (rest + rest.shr_arithmetic::<16>()).shr_arithmetic::<16>();
            let ra = rest.to_array();
            let ca = $c0.to_array();
            for oi in 0..3 {
                out[oi] = (ca[oi] as u16).wrapping_add(ra[oi] as u16);
            }
        }};
    }

    if rx >= ry {
        if ry >= rz {
            y1 += x1;
            z1 += y1;
            arm!(|c1, c2, c3, c0| (c1 - c0, c2 - c1, c3 - c2));
        } else if rz >= rx {
            x1 += z1;
            y1 += x1;
            arm!(|c1, c2, c3, c0| (c1 - c3, c2 - c1, c3 - c0));
        } else {
            z1 += x1;
            y1 += z1;
            arm!(|c1, c2, c3, c0| (c1 - c0, c2 - c3, c3 - c1));
        }
    } else {
        if rx >= rz {
            x1 += y1;
            z1 += x1;
            arm!(|c1, c2, c3, c0| (c1 - c2, c2 - c0, c3 - c1));
        } else if ry >= rz {
            z1 += y1;
            x1 += z1;
            arm!(|c1, c2, c3, c0| (c1 - c3, c2 - c0, c3 - c2));
        } else {
            y1 += z1;
            x1 += y1;
            arm!(|c1, c2, c3, c0| (c1 - c2, c2 - c3, c3 - c0));
        }
    }
    out
}

/// `#[arcane]` wrappers so `eval16` can reach the SIMD kernels from
/// non-token contexts — the feature-gated body lets the i32x4 backend
/// ops inline properly (a plain call into `tetra3_i32x4` leaves them
/// as per-op calls).
#[archmage::arcane]
pub(crate) fn eval16_tetra3_v3(
    token: archmage::X64V3Token,
    input: &[u16],
    output: &mut [u16],
    p: &InterpParams,
) {
    let v = tetra3_i32x4(token, [input[0], input[1], input[2]], p);
    output[..3].copy_from_slice(&v);
}

#[archmage::arcane]
pub(crate) fn eval16_eval4_v3(
    token: archmage::X64V3Token,
    input: &[u16],
    output: &mut [u16],
    p: &InterpParams,
) {
    let v = eval4_i32x4(
        token,
        [input[0], input[1], input[2], input[3]],
        4,
        p,
    );
    output[..4].copy_from_slice(&v);
}

/// `tetrahedral_interp_float` over `magetypes::f32x4` — the ≤4 output
/// channels as vector lanes. Per-lane `mulps`/`addps` results are
/// IEEE-identical to the scalar ops (no FMA — the expression order is
/// preserved); lanes beyond n_outputs are don't-care.
#[inline(always)]
pub(crate) fn tetra3_f32x4<T: magetypes::simd::backends::F32x4Backend>(
    token: T,
    input: &[f32; 3],
    n_outputs: usize,
    p: &InterpParams,
) -> [f32; 4] {
    let lut_table = p.table_f32();

    let px = fclamp(input[0]) * p.domain[0] as f32;
    let py = fclamp(input[1]) * p.domain[1] as f32;
    let pz = fclamp(input[2]) * p.domain[2] as f32;

    let x0 = px.floor() as i32;
    let rx = px - x0 as f32;
    let y0 = py.floor() as i32;
    let ry = py - y0 as f32;
    let z0 = pz.floor() as i32;
    let rz = pz - z0 as f32;

    let x0i = (p.opta[2] as i32).wrapping_mul(x0);
    let x1 = x0i
        + if fclamp(input[0]) >= 1.0 {
            0
        } else {
            p.opta[2] as i32
        };
    let y0i = (p.opta[1] as i32).wrapping_mul(y0);
    let y1 = y0i
        + if fclamp(input[1]) >= 1.0 {
            0
        } else {
            p.opta[1] as i32
        };
    let z0i = (p.opta[0] as i32).wrapping_mul(z0);
    let z1 = z0i
        + if fclamp(input[2]) >= 1.0 {
            0
        } else {
            p.opta[0] as i32
        };

    let n = p.n_outputs as usize;
    let m = lut_table.len() - n;
    let last = lut_table.len() - 1;
    let c = |off: i32| corner_f32x4(lut_table, (off as usize).min(m), last);

    let d000 = c(x0i + y0i + z0i);
    let d001 = c(x0i + y0i + z1);
    let d010 = c(x0i + y1 + z0i);
    let d011 = c(x0i + y1 + z1);
    let d100 = c(x1 + y0i + z0i);
    let d101 = c(x1 + y0i + z1);
    let d110 = c(x1 + y1 + z0i);
    let d111 = c(x1 + y1 + z1);

    let rxv = magetypes::simd::generic::f32x4::<T>::splat_t(token, rx);
    let ryv = magetypes::simd::generic::f32x4::<T>::splat_t(token, ry);
    let rzv = magetypes::simd::generic::f32x4::<T>::splat_t(token, rz);
    let lv = |a: [f32; 4]| magetypes::simd::generic::f32x4::<T>::from_array_t(token, a);

    let (d000v, d001v, d010v, d011v) = (lv(d000), lv(d001), lv(d010), lv(d011));
    let (d100v, d101v, d110v, d111v) = (lv(d100), lv(d101), lv(d110), lv(d111));

    let c0 = d000v;
    let (c1, c2, c3);
    if rx >= ry && ry >= rz {
        c1 = d100v - c0;
        c2 = d110v - d100v;
        c3 = d111v - d110v;
    } else if rx >= rz && rz >= ry {
        c1 = d100v - c0;
        c2 = d111v - d101v;
        c3 = d101v - d100v;
    } else if rz >= rx && rx >= ry {
        c1 = d101v - d001v;
        c2 = d111v - d101v;
        c3 = d001v - c0;
    } else if ry >= rx && rx >= rz {
        c1 = d110v - d010v;
        c2 = d010v - c0;
        c3 = d111v - d110v;
    } else if ry >= rz && rz >= rx {
        c1 = d111v - d011v;
        c2 = d010v - c0;
        c3 = d011v - d010v;
    } else if rz >= ry && ry >= rx {
        c1 = d111v - d011v;
        c2 = d011v - d001v;
        c3 = d001v - c0;
    } else {
        let z = magetypes::simd::generic::f32x4::<T>::zero_t(token);
        c1 = z;
        c2 = z;
        c3 = z;
    }

    let mut out = [0f32; 4];
    let res = ((c0 + c1 * rxv) + c2 * ryv) + c3 * rzv;
    let ra = res.to_array();
    for oc in 0..n_outputs.min(4) {
        out[oc] = ra[oc];
    }
    out
}

/// f32 corner block → `[f32; 4]` lanes with the same tail clamp as the
/// scalar `w(off)` slices (lanes beyond the table end take `last`).
#[inline(always)]
fn corner_f32x4(tab: &[f32], off: usize, last: usize) -> [f32; 4] {
    match tab.get(off..off + 4) {
        Some(s) => s.try_into().unwrap(),
        None => {
            let l = tab[last];
            [
                *tab.get(off).unwrap_or(&l),
                *tab.get(off + 1).unwrap_or(&l),
                *tab.get(off + 2).unwrap_or(&l),
                *tab.get(off + 3).unwrap_or(&l),
            ]
        }
    }
}

/// `eval4_inputs_float` over `magetypes::f32x4` — two shifted tetra
/// evals + the rk blend, all lanes. Same bit-exactness argument as
/// `tetra3_f32x4`; `y0 + (y1-y0)*rest` stays `mul` then `add` (no FMA).
#[inline(always)]
pub(crate) fn eval4_f32x4<T: magetypes::simd::backends::F32x4Backend>(
    token: T,
    input: &[f32; 4],
    n_outputs: usize,
    p: &InterpParams,
) -> [f32; 4] {
    let lut_table = p.table_f32();

    let pk = fclamp(input[0]) * p.domain[0] as f32;
    let k0 = pk.floor() as i32;
    let rest = pk - k0 as f32;

    let k0i = (p.opta[3] as i32).wrapping_mul(k0);
    let k1 = k0i
        + if fclamp(input[0]) >= 1.0 {
            0
        } else {
            p.opta[3] as i32
        };

    let mut p1 = InterpParams {
        dw_flags: p.dw_flags,
        n_inputs: p.n_inputs,
        n_outputs: p.n_outputs,
        n_samples: p.n_samples,
        domain: p.domain,
        opta: p.opta,
        table: InterpTable::F32(&lut_table[k0i as usize..]),
        interpolation: p.interpolation,
            kernel: p.kernel,
        foreign: None,
    };
    p1.domain[..3].copy_from_slice(&p.domain[1..4]);

    let vin: [f32; 3] = [input[1], input[2], input[3]];
    let v0 = tetra3_f32x4(token, &vin, n_outputs, &p1);
    p1.table = InterpTable::F32(&lut_table[k1 as usize..]);
    let v1 = tetra3_f32x4(token, &vin, n_outputs, &p1);

    let v0v = magetypes::simd::generic::f32x4::<T>::from_array_t(token, v0);
    let v1v = magetypes::simd::generic::f32x4::<T>::from_array_t(token, v1);
    let restv = magetypes::simd::generic::f32x4::<T>::splat_t(token, rest);
    let res = (v1v - v0v) * restv + v0v;
    let ra = res.to_array();
    let mut out = [0f32; 4];
    for oc in 0..n_outputs.min(4) {
        out[oc] = ra[oc];
    }
    out
}

/// `#[arcane]` wrapper for `eval_float`'s eval4 arm.
#[archmage::arcane]
pub(crate) fn evalflt_eval4_v3(
    token: archmage::X64V3Token,
    input: &[f32],
    output: &mut [f32],
    n_outputs: usize,
    p: &InterpParams,
) {
    let v = eval4_f32x4(
        token,
        &[input[0], input[1], input[2], input[3]],
        n_outputs,
        p,
    );
    output[..n_outputs].copy_from_slice(&v[..n_outputs]);
}

/// `#[arcane]` wrapper for `eval_float`'s tetra arm.
#[archmage::arcane]
pub(crate) fn evalflt_tetra3_v3(
    token: archmage::X64V3Token,
    input: &[f32],
    output: &mut [f32],
    n_outputs: usize,
    p: &InterpParams,
) {
    let v = tetra3_f32x4(token, &[input[0], input[1], input[2]], n_outputs, p);
    output[..n_outputs].copy_from_slice(&v[..n_outputs]);
}

/// u64 corner block → four i32 lanes (u16 zero-extend).
#[inline(always)]
pub(crate) fn lanes_u64(v: u64) -> [i32; 4] {
    [
        (v & 0xffff) as i32,
        ((v >> 16) & 0xffff) as i32,
        ((v >> 32) & 0xffff) as i32,
        (v >> 48) as i32,
    ]
}

/// `eval4_inputs` over `magetypes::i32x4` — the two K-cell tetra evals
/// vectorized across the (≤4) output channels, rk blend on the arrays.
/// For `n_outputs < 4` lanes beyond the count are don't-care (corner
/// loads stay in-table; only the first n_outputs lanes are stored).
#[inline(always)]
pub(crate) fn eval4_i32x4<T: magetypes::simd::backends::I32x4Backend>(
    token: T,
    vin: [u16; 4],
    n_outputs: usize,
    p: &InterpParams,
) -> [u16; 4] {
    let lut_table = p.table_u16();

    let fk = to_fixed_domain((vin[0] as i32).wrapping_mul(p.domain[0] as i32));
    let fx = to_fixed_domain((vin[1] as i32).wrapping_mul(p.domain[1] as i32));
    let fy = to_fixed_domain((vin[2] as i32).wrapping_mul(p.domain[2] as i32));
    let fz = to_fixed_domain((vin[3] as i32).wrapping_mul(p.domain[3] as i32));

    let k0 = fixed_to_int(fk);
    let x0 = fixed_to_int(fx);
    let y0 = fixed_to_int(fy);
    let z0 = fixed_to_int(fz);

    let rk = fixed_rest_to_int(fk);
    let rx = fixed_rest_to_int(fx);
    let ry = fixed_rest_to_int(fy);
    let rz = fixed_rest_to_int(fz);

    let k0i = (p.opta[3] as i32).wrapping_mul(k0);
    let k1 =
        k0i + if vin[0] == 0xFFFF { 0 } else { p.opta[3] as i32 };
    let x0i = (p.opta[2] as i32).wrapping_mul(x0);
    let x1 =
        x0i + if vin[1] == 0xFFFF { 0 } else { p.opta[2] as i32 };
    let y0i = (p.opta[1] as i32).wrapping_mul(y0);
    let y1 =
        y0i + if vin[2] == 0xFFFF { 0 } else { p.opta[1] as i32 };
    let z0i = (p.opta[0] as i32).wrapping_mul(z0);
    let z1 =
        z0i + if vin[3] == 0xFFFF { 0 } else { p.opta[0] as i32 };

    let rxv = magetypes::simd::generic::i32x4::<T>::splat_t(token, rx);
    let ryv = magetypes::simd::generic::i32x4::<T>::splat_t(token, ry);
    let rzv = magetypes::simd::generic::i32x4::<T>::splat_t(token, rz);
    let rk4 = magetypes::simd::generic::i32x4::<T>::splat_t(token, 0x8001);

    // One K-cell tetra eval → the pre-round lanes for all 4 outputs.
    macro_rules! tetra_cell {
        ($base:expr) => {{
            let d_x = x1 - x0i;
            let d_y = y1 - y0i;
            let d_z = z1 - z0i;
            let max_off = d_x + d_y + d_z;
            let start = ($base + x0i + y0i + z0i) as usize;
            let cell = &lut_table[start..];
            let last = cell.len() - 1;
            let mo = max_off as usize;

            macro_rules! arm {
                ($o1:expr, $o2:expr, $o3:expr, |$t1:ident, $t2:ident, $t3:ident, $c0:ident| $chain:expr) => {{
                    let $c0 = magetypes::simd::generic::i32x4::<T>::from_array_t(
                        token,
                        lanes_u64(corner_u64(cell, 0, last)),
                    );
                    let $t1 = magetypes::simd::generic::i32x4::<T>::from_array_t(
                        token,
                        lanes_u64(corner_u64(cell, ($o1).min(max_off) as usize, last)),
                    );
                    let $t2 = magetypes::simd::generic::i32x4::<T>::from_array_t(
                        token,
                        lanes_u64(corner_u64(cell, ($o2).min(max_off) as usize, last)),
                    );
                    let $t3 = magetypes::simd::generic::i32x4::<T>::from_array_t(
                        token,
                        lanes_u64(corner_u64(cell, ($o3).min(max_off) as usize, last)),
                    );
                    let (c1, c2, c3) = $chain;
                    let rest = c1 * rxv + c2 * ryv + c3 * rzv + rk4;
                    let rest = (rest + rest.shr_arithmetic::<16>())
                        .shr_arithmetic::<16>();
                    let ra = rest.to_array();
                    let ca = $c0.to_array();
                    [
                        (ca[0] as u16).wrapping_add(ra[0] as u16),
                        (ca[1] as u16).wrapping_add(ra[1] as u16),
                        (ca[2] as u16).wrapping_add(ra[2] as u16),
                        (ca[3] as u16).wrapping_add(ra[3] as u16),
                    ]
                }};
            }

            if rx >= ry {
                if ry >= rz {
                    arm!(d_x, d_x + d_y, max_off, |t1, t2, t3, c0| {
                        (t1 - c0, t2 - t1, t3 - t2)
                    })
                } else if rz >= rx {
                    arm!(d_x + d_z, max_off, d_z, |t1, t2, t3, c0| {
                        (t1 - t3, t2 - t1, t3 - c0)
                    })
                } else {
                    arm!(d_x, max_off, d_x + d_z, |t1, t2, t3, c0| {
                        (t1 - c0, t2 - t3, t3 - t1)
                    })
                }
            } else {
                if rx >= rz {
                    arm!(d_x + d_y, d_y, max_off, |t1, t2, t3, c0| {
                        (t1 - t2, t2 - c0, t3 - t1)
                    })
                } else if ry >= rz {
                    arm!(max_off, d_y, d_y + d_z, |t1, t2, t3, c0| {
                        (t1 - t3, t2 - c0, t3 - t2)
                    })
                } else {
                    arm!(max_off, d_y + d_z, d_z, |t1, t2, t3, c0| {
                        (t1 - t2, t2 - t3, t3 - c0)
                    })
                }
            }
        }};
    }

    let v0 = tetra_cell!(k0i);
    let v1 = tetra_cell!(k1);

    let mut out = [0u16; 4];
    for oc in 0..n_outputs.min(4) {
        out[oc] = linear_interp(rk, v0[oc] as i32, v1[oc] as i32);
    }
    out
}

/// `trilinear_interp16` over `magetypes::i32x4` — the ≤4 output
/// channels as vector lanes; the 3-level lerp chain is wrapping i32
/// like the scalar (intermediate `as u16` truncation = `& 0xffff`).
#[inline(always)]
pub(crate) fn trilinear_i32x4<T: magetypes::simd::backends::I32x4Backend>(
    token: T,
    vin: [u16; 3],
    n_outputs: usize,
    p: &InterpParams,
) -> [u16; 4] {
    let lut_table = p.table_u16();

    let fx = to_fixed_domain((vin[0] as i32).wrapping_mul(p.domain[0] as i32));
    let x0 = fixed_to_int(fx);
    let rx = fixed_rest_to_int(fx);
    let fy = to_fixed_domain((vin[1] as i32).wrapping_mul(p.domain[1] as i32));
    let y0 = fixed_to_int(fy);
    let ry = fixed_rest_to_int(fy);
    let fz = to_fixed_domain((vin[2] as i32).wrapping_mul(p.domain[2] as i32));
    let z0 = fixed_to_int(fz);
    let rz = fixed_rest_to_int(fz);

    let x0i = (p.opta[2] as i32).wrapping_mul(x0);
    let x1 = x0i
        + if vin[0] == 0xFFFF { 0 } else { p.opta[2] as i32 };
    let y0i = (p.opta[1] as i32).wrapping_mul(y0);
    let y1 = y0i
        + if vin[1] == 0xFFFF { 0 } else { p.opta[1] as i32 };
    let z0i = (p.opta[0] as i32).wrapping_mul(z0);
    let z1 = z0i
        + if vin[2] == 0xFFFF { 0 } else { p.opta[0] as i32 };

    let max_off = x1.wrapping_add(y1).wrapping_add(z1);
    let cell = lut_table;
    let n = p.n_outputs as usize;
    let m = cell.len() - n;
    let last = cell.len() - 1;
    let w = |off: i32| {
        magetypes::simd::generic::i32x4::<T>::from_array_t(
            token,
            lanes_u64(corner_u64(
                cell,
                ((off.min(max_off)) as usize).min(m),
                last,
            )),
        )
    };

    let d000 = w(x0i + y0i + z0i);
    let d001 = w(x0i + y0i + z1);
    let d010 = w(x0i + y1 + z0i);
    let d011 = w(x0i + y1 + z1);
    let d100 = w(x1 + y0i + z0i);
    let d101 = w(x1 + y0i + z1);
    let d110 = w(x1 + y1 + z0i);
    let d111 = w(x1 + y1 + z1);

    type L<T> = magetypes::simd::generic::i32x4<T>;
    let half = L::<T>::splat_t(token, 0x8000);
    let mask = L::<T>::splat_t(token, 0xffff);
    let lerp =
        |a: L<T>, l: L<T>, h: L<T>| (l + ((h - l) * a + half).shr_arithmetic::<16>()) & mask;

    let dx00 = lerp(L::<T>::splat_t(token, rx), d000, d100);
    let dx01 = lerp(L::<T>::splat_t(token, rx), d001, d101);
    let dx10 = lerp(L::<T>::splat_t(token, rx), d010, d110);
    let dx11 = lerp(L::<T>::splat_t(token, rx), d011, d111);

    let dxy0 = lerp(L::<T>::splat_t(token, ry), dx00, dx10);
    let dxy1 = lerp(L::<T>::splat_t(token, ry), dx01, dx11);

    let res = lerp(L::<T>::splat_t(token, rz), dxy0, dxy1).to_array();
    let mut out = [0u16; 4];
    for oc in 0..n_outputs.min(4) {
        out[oc] = res[oc] as u16;
    }
    out
}

/// `eval1_input` over `magetypes::i32x4` — 1-in/≤4-out: two u64 corner
/// blocks + `linear_interp` per lane. `linear_interp` uses a *logical*
/// u32 shift — `shr_logical`, not the lerp's arithmetic one.
#[inline(always)]
pub(crate) fn eval1_i32x4<T: magetypes::simd::backends::I32x4Backend>(
    token: T,
    input: u16,
    n_outputs: usize,
    p: &InterpParams,
) -> [u16; 4] {
    let lut_table = p.table_u16();
    let n = p.n_outputs as usize;
    let mut out = [0u16; 4];
    if input == 0xffff || p.domain[0] == 0 {
        let y0 = (p.domain[0] * p.opta[0]) as usize;
        let last = lut_table.len() - 1;
        let lanes = lanes_u64(corner_u64(lut_table, y0.min(last), last));
        for oc in 0..n.min(4) {
            out[oc] = lut_table
                .get(y0 + oc)
                .copied()
                .unwrap_or(lanes[oc.min(3)] as u16);
        }
        return out;
    }
    let v = (input as u32).wrapping_mul(p.domain[0]) as i32;
    let fk = to_fixed_domain(v);
    let k0 = fixed_to_int(fk);
    let rk = fixed_rest_to_int(fk);
    let k1 = k0 + if input != 0xFFFF { 1 } else { 0 };
    let m = lut_table.len() - n;
    let k0i = ((p.opta[0] as i32) * k0).clamp(0, m as i32) as usize;
    let k1i = ((p.opta[0] as i32) * k1).clamp(0, m as i32) as usize;
    let last = lut_table.len() - 1;
    let d0 = magetypes::simd::generic::i32x4::<T>::from_array_t(
        token,
        lanes_u64(corner_u64(lut_table, k0i, last)),
    );
    let d1 = magetypes::simd::generic::i32x4::<T>::from_array_t(
        token,
        lanes_u64(corner_u64(lut_table, k1i, last)),
    );
    type L<T> = magetypes::simd::generic::i32x4<T>;
    let half = L::<T>::splat_t(token, 0x8000);
    let mask = L::<T>::splat_t(token, 0xffff);
    let dif = (d1 - d0) * L::<T>::splat_t(token, rk) + half;
    let res = (dif.shr_logical::<16>() + d0) & mask;
    let ra = res.to_array();
    for oc in 0..n_outputs.min(4) {
        out[oc] = ra[oc] as u16;
    }
    out
}

/// `eval1_input_float` over `magetypes::f32x4`.
#[inline(always)]
pub(crate) fn eval1_f32x4<T: magetypes::simd::backends::F32x4Backend>(
    token: T,
    input: f32,
    n_outputs: usize,
    p: &InterpParams,
) -> [f32; 4] {
    let lut_table = p.table_f32();
    let n = p.n_outputs as usize;
    let mut out = [0f32; 4];
    let val2 = fclamp(input);
    if val2 == 1.0 || p.domain[0] == 0 {
        let start = (p.domain[0] * p.opta[0]) as usize;
        let last = lut_table.len() - 1;
        let lanes = corner_f32x4(lut_table, start.min(last), last);
        for oc in 0..n.min(4) {
            out[oc] = lanes[oc];
        }
        return out;
    }
    let val2 = val2 * p.domain[0] as f32;
    let cell0 = val2.floor() as i32;
    let cell1 = val2.ceil() as i32;
    let rest = val2 - cell0 as f32;
    let cell0 = cell0 * p.opta[0] as i32;
    let cell1 = cell1 * p.opta[0] as i32;
    let m = lut_table.len() - n;
    let last = lut_table.len() - 1;
    let d0 = magetypes::simd::generic::f32x4::<T>::from_array_t(
        token,
        corner_f32x4(lut_table, (cell0 as usize).min(m), last),
    );
    let d1 = magetypes::simd::generic::f32x4::<T>::from_array_t(
        token,
        corner_f32x4(lut_table, (cell1 as usize).min(m), last),
    );
    let res = (d1 - d0) * magetypes::simd::generic::f32x4::<T>::splat_t(token, rest) + d0;
    let ra = res.to_array();
    for oc in 0..n_outputs.min(4) {
        out[oc] = ra[oc];
    }
    out
}

/// `#[arcane]` wrappers for the eval1 arms.
#[archmage::arcane]
pub(crate) fn eval16_eval1_v3(
    token: archmage::X64V3Token,
    input: &[u16],
    output: &mut [u16],
    p: &InterpParams,
) {
    let n_out = p.n_outputs as usize;
    let v = eval1_i32x4(token, input[0], n_out, p);
    output[..n_out].copy_from_slice(&v[..n_out]);
}

#[archmage::arcane]
pub(crate) fn evalflt_eval1_v3(
    token: archmage::X64V3Token,
    input: &[f32],
    output: &mut [f32],
    n_outputs: usize,
    p: &InterpParams,
) {
    let v = eval1_f32x4(token, input[0], n_outputs, p);
    output[..n_outputs].copy_from_slice(&v[..n_outputs]);
}

/// `bilinear_interp16` over `magetypes::i32x4` — the ≤4 output
/// channels as vector lanes (same wrapping-lerp semantics as the
/// scalar; intermediate `as u16` = `& 0xffff`).
#[inline(always)]
pub(crate) fn bilinear_i32x4<T: magetypes::simd::backends::I32x4Backend>(
    token: T,
    vin: [u16; 2],
    n_outputs: usize,
    p: &InterpParams,
) -> [u16; 4] {
    let lut_table = p.table_u16();

    let fx = to_fixed_domain((vin[0] as i32).wrapping_mul(p.domain[0] as i32));
    let x0 = fixed_to_int(fx);
    let rx = fixed_rest_to_int(fx);
    let fy = to_fixed_domain((vin[1] as i32).wrapping_mul(p.domain[1] as i32));
    let y0 = fixed_to_int(fy);
    let ry = fixed_rest_to_int(fy);

    let x0i = (p.opta[1] as i32).wrapping_mul(x0);
    let x1 = x0i
        + if vin[0] == 0xFFFF { 0 } else { p.opta[1] as i32 };
    let y0i = (p.opta[0] as i32).wrapping_mul(y0);
    let y1 = y0i
        + if vin[1] == 0xFFFF { 0 } else { p.opta[0] as i32 };

    let cell = lut_table;
    let n = p.n_outputs as usize;
    let m = cell.len() - n;
    let last = cell.len() - 1;
    let w = |off: i32| {
        magetypes::simd::generic::i32x4::<T>::from_array_t(
            token,
            lanes_u64(corner_u64(cell, (off as usize).min(m), last)),
        )
    };

    let d00 = w(x0i + y0i);
    let d01 = w(x0i + y1);
    let d10 = w(x1 + y0i);
    let d11 = w(x1 + y1);

    type L<T> = magetypes::simd::generic::i32x4<T>;
    let half = L::<T>::splat_t(token, 0x8000);
    let mask = L::<T>::splat_t(token, 0xffff);
    let lerp =
        |a: L<T>, l: L<T>, h: L<T>| (l + ((h - l) * a + half).shr_arithmetic::<16>()) & mask;

    let dx0 = lerp(L::<T>::splat_t(token, rx), d00, d10);
    let dx1 = lerp(L::<T>::splat_t(token, rx), d01, d11);
    let res = lerp(L::<T>::splat_t(token, ry), dx0, dx1).to_array();
    let mut out = [0u16; 4];
    for oc in 0..n_outputs.min(4) {
        out[oc] = res[oc] as u16;
    }
    out
}

/// `trilinear_interp_float` over `magetypes::f32x4` — ≤4 output
/// channels as lanes; `l + (h-l)*a` per lane is IEEE-identical.
#[inline(always)]
pub(crate) fn trilinear_f32x4<T: magetypes::simd::backends::F32x4Backend>(
    token: T,
    input: &[f32; 3],
    n_outputs: usize,
    p: &InterpParams,
) -> [f32; 4] {
    let lut_table = p.table_f32();

    let px = fclamp(input[0]) * p.domain[0] as f32;
    let py = fclamp(input[1]) * p.domain[1] as f32;
    let pz = fclamp(input[2]) * p.domain[2] as f32;
    let x0 = px.floor() as i32;
    let fx = px - x0 as f32;
    let y0 = py.floor() as i32;
    let fy = py - y0 as f32;
    let z0 = pz.floor() as i32;
    let fz = pz - z0 as f32;

    let x0i = (p.opta[2] as i32).wrapping_mul(x0);
    let x1 = x0i
        + if fclamp(input[0]) >= 1.0 { 0 } else { p.opta[2] as i32 };
    let y0i = (p.opta[1] as i32).wrapping_mul(y0);
    let y1 = y0i
        + if fclamp(input[1]) >= 1.0 { 0 } else { p.opta[1] as i32 };
    let z0i = (p.opta[0] as i32).wrapping_mul(z0);
    let z1 = z0i
        + if fclamp(input[2]) >= 1.0 { 0 } else { p.opta[0] as i32 };

    let n = p.n_outputs as usize;
    let m = lut_table.len() - n;
    let last = lut_table.len() - 1;
    let w = |off: i32| {
        magetypes::simd::generic::f32x4::<T>::from_array_t(
            token,
            corner_f32x4(lut_table, (off as usize).min(m), last),
        )
    };

    let d000 = w(x0i + y0i + z0i);
    let d001 = w(x0i + y0i + z1);
    let d010 = w(x0i + y1 + z0i);
    let d011 = w(x0i + y1 + z1);
    let d100 = w(x1 + y0i + z0i);
    let d101 = w(x1 + y0i + z1);
    let d110 = w(x1 + y1 + z0i);
    let d111 = w(x1 + y1 + z1);

    type V<T> = magetypes::simd::generic::f32x4<T>;
    let lerp = |a: V<T>, l: V<T>, h: V<T>| l + (h - l) * a;
    let dx00 = lerp(V::<T>::splat_t(token, fx), d000, d100);
    let dx01 = lerp(V::<T>::splat_t(token, fx), d001, d101);
    let dx10 = lerp(V::<T>::splat_t(token, fx), d010, d110);
    let dx11 = lerp(V::<T>::splat_t(token, fx), d011, d111);
    let dxy0 = lerp(V::<T>::splat_t(token, fy), dx00, dx10);
    let dxy1 = lerp(V::<T>::splat_t(token, fy), dx01, dx11);
    let res = lerp(V::<T>::splat_t(token, fz), dxy0, dxy1).to_array();
    let mut out = [0f32; 4];
    for oc in 0..n_outputs.min(4) {
        out[oc] = res[oc];
    }
    out
}

/// `bilinear_interp_float` over `magetypes::f32x4`.
#[inline(always)]
pub(crate) fn bilinear_f32x4<T: magetypes::simd::backends::F32x4Backend>(
    token: T,
    input: &[f32; 2],
    n_outputs: usize,
    p: &InterpParams,
) -> [f32; 4] {
    let lut_table = p.table_f32();

    let px = fclamp(input[0]) * p.domain[0] as f32;
    let py = fclamp(input[1]) * p.domain[1] as f32;
    let x0 = px.floor() as i32;
    let fx = px - x0 as f32;
    let y0 = py.floor() as i32;
    let fy = py - y0 as f32;

    let x0i = (p.opta[1] as i32).wrapping_mul(x0);
    let x1 = x0i
        + if fclamp(input[0]) >= 1.0 { 0 } else { p.opta[1] as i32 };
    let y0i = (p.opta[0] as i32).wrapping_mul(y0);
    let y1 = y0i
        + if fclamp(input[1]) >= 1.0 { 0 } else { p.opta[0] as i32 };

    let n = p.n_outputs as usize;
    let m = lut_table.len() - n;
    let last = lut_table.len() - 1;
    let w = |off: i32| {
        magetypes::simd::generic::f32x4::<T>::from_array_t(
            token,
            corner_f32x4(lut_table, (off as usize).min(m), last),
        )
    };

    let d00 = w(x0i + y0i);
    let d01 = w(x0i + y1);
    let d10 = w(x1 + y0i);
    let d11 = w(x1 + y1);

    type V<T> = magetypes::simd::generic::f32x4<T>;
    let lerp = |a: V<T>, l: V<T>, h: V<T>| l + (h - l) * a;
    let dx0 = lerp(V::<T>::splat_t(token, fx), d00, d10);
    let dx1 = lerp(V::<T>::splat_t(token, fx), d01, d11);
    let res = lerp(V::<T>::splat_t(token, fy), dx0, dx1).to_array();
    let mut out = [0f32; 4];
    for oc in 0..n_outputs.min(4) {
        out[oc] = res[oc];
    }
    out
}

/// `#[arcane]` wrappers for `eval_float`'s trilinear/bilinear arms.
#[archmage::arcane]
pub(crate) fn evalflt_trilinear_v3(
    token: archmage::X64V3Token,
    input: &[f32],
    output: &mut [f32],
    n_outputs: usize,
    p: &InterpParams,
) {
    let v = trilinear_f32x4(
        token,
        &[input[0], input[1], input[2]],
        n_outputs,
        p,
    );
    output[..n_outputs].copy_from_slice(&v[..n_outputs]);
}

#[archmage::arcane]
pub(crate) fn evalflt_bilinear_v3(
    token: archmage::X64V3Token,
    input: &[f32],
    output: &mut [f32],
    n_outputs: usize,
    p: &InterpParams,
) {
    let v = bilinear_f32x4(token, &[input[0], input[1]], n_outputs, p);
    output[..n_outputs].copy_from_slice(&v[..n_outputs]);
}

/// `#[arcane]` wrapper for `eval16`'s bilinear arm.
#[archmage::arcane]
pub(crate) fn eval16_bilinear_v3(
    token: archmage::X64V3Token,
    input: &[u16],
    output: &mut [u16],
    p: &InterpParams,
) {
    let n_out = p.n_outputs as usize;
    let v = bilinear_i32x4(token, [input[0], input[1]], n_out, p);
    output[..n_out].copy_from_slice(&v[..n_out]);
}

/// `#[arcane]` wrapper for `eval16`'s trilinear arm.
#[archmage::arcane]
pub(crate) fn eval16_trilinear_v3(
    token: archmage::X64V3Token,
    input: &[u16],
    output: &mut [u16],
    p: &InterpParams,
) {
    let n_out = p.n_outputs as usize;
    let v = trilinear_i32x4(
        token,
        [input[0], input[1], input[2]],
        n_out,
        p,
    );
    output[..n_out].copy_from_slice(&v[..n_out]);
}

/// `Eval4Inputs` — CMYK: two tetrahedral cells + linear blend, fixed point.
#[inline]
pub(crate) fn eval4_inputs(input: &[u16], output: &mut [u16], p: &InterpParams) {
    let lut_table = p.table_u16();
    let mut tmp1 = [0u16; MAX_STAGE_CHANNELS];

    let fk = to_fixed_domain((input[0] as i32).wrapping_mul(p.domain[0] as i32));
    let fx = to_fixed_domain((input[1] as i32).wrapping_mul(p.domain[1] as i32));
    let fy = to_fixed_domain((input[2] as i32).wrapping_mul(p.domain[2] as i32));
    let fz = to_fixed_domain((input[3] as i32).wrapping_mul(p.domain[3] as i32));

    let k0 = fixed_to_int(fk);
    let x0 = fixed_to_int(fx);
    let y0 = fixed_to_int(fy);
    let z0 = fixed_to_int(fz);

    let rk = fixed_rest_to_int(fk);
    let rx = fixed_rest_to_int(fx);
    let ry = fixed_rest_to_int(fy);
    let rz = fixed_rest_to_int(fz);

    let k0i = (p.opta[3] as i32).wrapping_mul(k0);
    let k1 = k0i
        + if input[0] == 0xFFFF {
            0
        } else {
            p.opta[3] as i32
        };

    let x0i = (p.opta[2] as i32).wrapping_mul(x0);
    let x1 = x0i
        + if input[1] == 0xFFFF {
            0
        } else {
            p.opta[2] as i32
        };

    let y0i = (p.opta[1] as i32).wrapping_mul(y0);
    let y1 = y0i
        + if input[2] == 0xFFFF {
            0
        } else {
            p.opta[1] as i32
        };

    let z0i = (p.opta[0] as i32).wrapping_mul(z0);
    let z1 = z0i
        + if input[3] == 0xFFFF {
            0
        } else {
            p.opta[0] as i32
        };

    // Inner tetrahedral eval shared by the K0 and K1 passes — C's layout:
    // the tetrahedron ladder is chosen once, then each arm runs the
    // output-channel loop with 4 table loads and 3 subs (offsets computed
    // once; `.min` keeps `o+oc < cell.len()` provable, eliding bounds
    // checks in the channel loop).
    #[inline(always)]
    fn tetra16(
        lut_table: &[u16],
        base: i32,
        x0i: i32,
        x1: i32,
        y0i: i32,
        y1: i32,
        z0i: i32,
        z1: i32,
        rx: i32,
        ry: i32,
        rz: i32,
        n_outputs: usize,
        out: &mut [u16],
        // K1 pass: lerp the just-computed tetra output against the K0 pass
        // result (`prev`) — folds `linear_interp` into the channel loop and
        // avoids a second temp array + separate interp tail.
        prev_rk: Option<(&[u16], i32)>,
    ) {
        let c0_off = x0i + y0i + z0i;
        let d_x = x1 - x0i;
        let d_y = y1 - y0i;
        let d_z = z1 - z0i;
        let max_off = d_x + d_y + d_z;
        let start = (base + c0_off) as usize;
        let cell = &lut_table[start..start + max_off as usize + n_outputs];

        #[inline(always)]
        fn arm<F: Fn(i32, i32, i32, i32) -> (i32, i32, i32)>(
            cell: &[u16],
            o1: i32,
            o2: i32,
            o3: i32,
            max_off: i32,
            rx: i32,
            ry: i32,
            rz: i32,
            n_outputs: usize,
            out: &mut [u16],
            prev_rk: Option<(&[u16], i32)>,
            diffs: F,
        ) {
            // `o+oc` stays in-bounds by `o.min(max_off)` + cell's
            // `max_off + n` span — re-slice so the channel loop's four
            // table reads carry one bounds check each, not `4×n`.
            let o1 = (o1.min(max_off)) as usize;
            let o2 = (o2.min(max_off)) as usize;
            let o3 = (o3.min(max_off)) as usize;
            let c0s = &cell[..n_outputs];
            let t1s = &cell[o1..o1 + n_outputs];
            let t2s = &cell[o2..o2 + n_outputs];
            let t3s = &cell[o3..o3 + n_outputs];
            let out = &mut out[..n_outputs];
            for oc in 0..n_outputs {
                let c0 = c0s[oc] as i32;
                let t1 = t1s[oc] as i32;
                let t2 = t2s[oc] as i32;
                let t3 = t3s[oc] as i32;
                let (c1, c2, c3) = diffs(c0, t1, t2, t3);
                let rest = c1
                    .wrapping_mul(rx)
                    .wrapping_add(c2.wrapping_mul(ry))
                    .wrapping_add(c3.wrapping_mul(rz))
                    .wrapping_add(0x8001);
                let v = (c0 as u16).wrapping_add((rest.wrapping_add(rest >> 16) >> 16) as u16);
                out[oc] = match prev_rk {
                    Some((prev, rk)) => linear_interp(rk, prev[oc] as i32, v as i32),
                    None => v,
                };
            }
        }

        if rx >= ry {
            if ry >= rz {
                arm(cell, d_x, d_x + d_y, max_off, max_off, rx, ry, rz, n_outputs, out, prev_rk, |c0, t1, t2, t3| {
                    (t1 - c0, t2 - t1, t3 - t2)
                });
            } else if rz >= rx {
                arm(cell, d_x + d_z, max_off, d_z, max_off, rx, ry, rz, n_outputs, out, prev_rk, |c0, t1, t2, t3| {
                    (t1 - t3, t2 - t1, t3 - c0)
                });
            } else {
                arm(cell, d_x, max_off, d_x + d_z, max_off, rx, ry, rz, n_outputs, out, prev_rk, |c0, t1, t2, t3| {
                    (t1 - c0, t2 - t3, t3 - t1)
                });
            }
        } else if rx >= rz {
            arm(cell, d_x + d_y, d_y, max_off, max_off, rx, ry, rz, n_outputs, out, prev_rk, |c0, t1, t2, t3| {
                (t1 - t2, t2 - c0, t3 - t1)
            });
        } else if ry >= rz {
            arm(cell, max_off, d_y, d_y + d_z, max_off, rx, ry, rz, n_outputs, out, prev_rk, |c0, t1, t2, t3| {
                (t1 - t3, t2 - c0, t3 - t2)
            });
        } else {
            arm(cell, max_off, d_y + d_z, d_z, max_off, rx, ry, rz, n_outputs, out, prev_rk, |c0, t1, t2, t3| {
                (t1 - t2, t2 - t3, t3 - c0)
            });
        }
    }

    let n_outputs = p.n_outputs as usize;
    tetra16(
        lut_table, k0i, x0i, x1, y0i, y1, z0i, z1, rx, ry, rz, n_outputs, &mut tmp1, None,
    );
    tetra16(
        lut_table, k1, x0i, x1, y0i, y1, z0i, z1, rx, ry, rz, n_outputs, output,
        Some((&tmp1, rk)),
    );
}

/// `Eval4InputsFloat` — two tetrahedral evals + lerp, float.
#[inline]
pub(crate) fn eval4_inputs_float(input: &[f32], output: &mut [f32], p: &InterpParams) {
    let lut_table = p.table_f32();
    let mut tmp1 = [0.0f32; MAX_STAGE_CHANNELS];
    let mut tmp2 = [0.0f32; MAX_STAGE_CHANNELS];

    let pk = fclamp(input[0]) * p.domain[0] as f32;
    let k0 = pk.floor() as i32;
    let rest = pk - k0 as f32;

    let k0i = (p.opta[3] as i32).wrapping_mul(k0);
    let k1 = k0i
        + if fclamp(input[0]) >= 1.0 {
            0
        } else {
            p.opta[3] as i32
        };

    // p1 = *p with Domain shifted left (dims 1..3 become dims 0..2).
    // `foreign` is consulted only at the outer kernel entry — the inner
    // recursion uses built-in kernels, so it is not copied (avoids a per-
    // pixel `Box` clone of `CmsInterpParams`).
    let mut p1 = InterpParams {
        dw_flags: p.dw_flags,
        n_inputs: p.n_inputs,
        n_outputs: p.n_outputs,
        n_samples: p.n_samples,
        domain: p.domain,
        opta: p.opta,
        table: InterpTable::F32(&lut_table[k0i as usize..]),
        interpolation: p.interpolation,
            kernel: p.kernel,
        foreign: None,
    };
    p1.domain[..3].copy_from_slice(&p.domain[1..4]);

    tetrahedral_interp_float(&input[1..], &mut tmp1, &p1);

    p1.table = InterpTable::F32(&lut_table[k1 as usize..]);
    tetrahedral_interp_float(&input[1..], &mut tmp2, &p1);

    for i in 0..p.n_outputs as usize {
        let y0 = tmp1[i];
        let y1 = tmp2[i];
        output[i] = y0 + (y1 - y0) * rest;
    }
}

/// The recursive `EVAL_FNS(N, NM)` expansion for 5..=15 inputs, fixed point.
/// `n` is the current input count; recurses to n-1, bottoming out at
/// `eval4_inputs`.
fn eval_chain16(input: &[u16], output: &mut [u16], p: &InterpParams, n: usize) {
    let lut_table = p.table_u16();
    let mut tmp1 = [0u16; MAX_STAGE_CHANNELS];
    let mut tmp2 = [0u16; MAX_STAGE_CHANNELS];

    let fk = to_fixed_domain((input[0] as i32).wrapping_mul(p.domain[0] as i32));
    let k0 = fixed_to_int(fk);
    let rk = fixed_rest_to_int(fk);

    let nm = n - 1;
    let k0i = (p.opta[nm] as i32).wrapping_mul(k0);
    let k1i = (p.opta[nm] as i32).wrapping_mul(k0 + if input[0] != 0xFFFF { 1 } else { 0 });

    // p1 = *p16 with Domain[0..NM] = Domain[1..=NM] — see the `foreign`
    // note in eval4_inputs_float
    let mut p1 = InterpParams {
        dw_flags: p.dw_flags,
        n_inputs: p.n_inputs,
        n_outputs: p.n_outputs,
        n_samples: p.n_samples,
        domain: p.domain,
        opta: p.opta,
        table: InterpTable::U16(&lut_table[k0i as usize..]),
        interpolation: p.interpolation,
            kernel: p.kernel,
        foreign: None,
    };
    p1.domain[..nm].copy_from_slice(&p.domain[1..=nm]);

    eval_dispatch16(&input[1..], &mut tmp1, &p1, nm);

    p1.table = InterpTable::U16(&lut_table[k1i as usize..]);
    eval_dispatch16(&input[1..], &mut tmp2, &p1, nm);

    for i in 0..p.n_outputs as usize {
        output[i] = linear_interp(rk, tmp1[i] as i32, tmp2[i] as i32);
    }
}

/// `EVAL_FNS` float twin.
fn eval_chain_float(input: &[f32], output: &mut [f32], p: &InterpParams, n: usize) {
    let lut_table = p.table_f32();
    let mut tmp1 = [0.0f32; MAX_STAGE_CHANNELS];
    let _tmp2 = [0.0f32; MAX_STAGE_CHANNELS];
    let mut tmp2 = [0.0f32; MAX_STAGE_CHANNELS];

    let pk = fclamp(input[0]) * p.domain[0] as f32;
    let k0 = pk.floor() as i32;
    let rest = pk - k0 as f32;

    let nm = n - 1;
    let k0i = (p.opta[nm] as i32).wrapping_mul(k0);
    let k1i = k0i
        + if fclamp(input[0]) >= 1.0 {
            0
        } else {
            p.opta[nm] as i32
        };

    let mut p1 = InterpParams {
        dw_flags: p.dw_flags,
        n_inputs: p.n_inputs,
        n_outputs: p.n_outputs,
        n_samples: p.n_samples,
        domain: p.domain,
        opta: p.opta,
        table: InterpTable::F32(&lut_table[k0i as usize..]),
        interpolation: p.interpolation,
            kernel: p.kernel,
        foreign: None,
    };
    p1.domain[..nm].copy_from_slice(&p.domain[1..=nm]);

    eval_dispatch_float(&input[1..], &mut tmp1, &p1, nm);

    p1.table = InterpTable::F32(&lut_table[k1i as usize..]);
    eval_dispatch_float(&input[1..], &mut tmp2, &p1, nm);

    for i in 0..p.n_outputs as usize {
        let y0 = tmp1[i];
        let y1 = tmp2[i];
        output[i] = y0 + (y1 - y0) * rest;
    }
}

/// Dispatch to the kernel for `n` inputs — used by the chain (the C recursion
/// lands on Eval4Inputs / Eval4InputsFloat at the bottom).
fn eval_dispatch16(input: &[u16], output: &mut [u16], p: &InterpParams, n: usize) {
    match n {
        4 => {
            // Same SIMD arm as `eval16` — multi-ink CLUTs get vectorized
            // eval4 on each of the two shifted cells.
            if p.n_outputs == 4 && output.len() >= 4 {
                #[cfg(target_arch = "x86_64")]
                if let Some(t) = x64v3_cached() {
                    return eval16_eval4_v3(t, input, output, p);
                }
            }
            eval4_inputs(input, output, p)
        }
        _ => eval_chain16(input, output, p, n),
    }
}

fn eval_dispatch_float(input: &[f32], output: &mut [f32], p: &InterpParams, n: usize) {
    match n {
        4 => {
            let n_out = p.n_outputs as usize;
            if (1..=4).contains(&n_out) && output.len() >= n_out {
                #[cfg(target_arch = "x86_64")]
                if let Some(t) = x64v3_cached() {
                    return evalflt_eval4_v3(t, input, output, n_out, p);
                }
            }
            eval4_inputs_float(input, output, p)
        }
        _ => eval_chain_float(input, output, p, n),
    }
}

// Named kernels for the factory table (the C macro expands each name).
macro_rules! eval_kernels {
    ($($name16:ident / $namef:ident => $n:expr),+) => {
        $(
        fn $name16(input: &[u16], output: &mut [u16], p: &InterpParams) {
            eval_chain16(input, output, p, $n)
        }
        fn $namef(input: &[f32], output: &mut [f32], p: &InterpParams) {
            eval_chain_float(input, output, p, $n)
        }
        )+
    };
}

eval_kernels! {
    eval5_inputs / eval5_inputs_float => 5,
    eval6_inputs / eval6_inputs_float => 6,
    eval7_inputs / eval7_inputs_float => 7,
    eval8_inputs / eval8_inputs_float => 8,
    eval9_inputs / eval9_inputs_float => 9,
    eval10_inputs / eval10_inputs_float => 10,
    eval11_inputs / eval11_inputs_float => 11,
    eval12_inputs / eval12_inputs_float => 12,
    eval13_inputs / eval13_inputs_float => 13,
    eval14_inputs / eval14_inputs_float => 14,
    eval15_inputs / eval15_inputs_float => 15
}

/// `DefaultInterpolatorsFactory` (static in C) — kernel selection.
pub fn default_interpolators_factory(
    n_input_channels: u32,
    n_output_channels: u32,
    dw_flags: u32,
) -> Option<Interpolation> {
    let is_float = dw_flags & LERP_FLAGS_FLOAT != 0;
    let is_trilinear = dw_flags & LERP_FLAGS_TRILINEAR != 0;

    // Safety check
    if n_input_channels >= 4 && n_output_channels as usize >= MAX_STAGE_CHANNELS {
        return None;
    }

    match n_input_channels {
        1 => {
            if n_output_channels == 1 {
                Some(if is_float {
                    Interpolation::LerpFloat(lin_lerp_1d_float)
                } else {
                    Interpolation::Lerp16(lin_lerp_1d)
                })
            } else {
                Some(if is_float {
                    Interpolation::LerpFloat(eval1_input_float)
                } else {
                    Interpolation::Lerp16(eval1_input)
                })
            }
        }
        2 => Some(if is_float {
            Interpolation::LerpFloat(bilinear_interp_float)
        } else {
            Interpolation::Lerp16(bilinear_interp16)
        }),
        3 => {
            if is_trilinear {
                Some(if is_float {
                    Interpolation::LerpFloat(trilinear_interp_float)
                } else {
                    Interpolation::Lerp16(trilinear_interp16)
                })
            } else {
                Some(if is_float {
                    Interpolation::LerpFloat(tetrahedral_interp_float)
                } else {
                    Interpolation::Lerp16(tetrahedral_interp16)
                })
            }
        }
        4 => Some(if is_float {
            Interpolation::LerpFloat(eval4_inputs_float)
        } else {
            Interpolation::Lerp16(eval4_inputs)
        }),
        5 => Some(if is_float {
            Interpolation::LerpFloat(eval5_inputs_float)
        } else {
            Interpolation::Lerp16(eval5_inputs)
        }),
        6 => Some(if is_float {
            Interpolation::LerpFloat(eval6_inputs_float)
        } else {
            Interpolation::Lerp16(eval6_inputs)
        }),
        7 => Some(if is_float {
            Interpolation::LerpFloat(eval7_inputs_float)
        } else {
            Interpolation::Lerp16(eval7_inputs)
        }),
        8 => Some(if is_float {
            Interpolation::LerpFloat(eval8_inputs_float)
        } else {
            Interpolation::Lerp16(eval8_inputs)
        }),
        9 => Some(if is_float {
            Interpolation::LerpFloat(eval9_inputs_float)
        } else {
            Interpolation::Lerp16(eval9_inputs)
        }),
        10 => Some(if is_float {
            Interpolation::LerpFloat(eval10_inputs_float)
        } else {
            Interpolation::Lerp16(eval10_inputs)
        }),
        11 => Some(if is_float {
            Interpolation::LerpFloat(eval11_inputs_float)
        } else {
            Interpolation::Lerp16(eval11_inputs)
        }),
        12 => Some(if is_float {
            Interpolation::LerpFloat(eval12_inputs_float)
        } else {
            Interpolation::Lerp16(eval12_inputs)
        }),
        13 => Some(if is_float {
            Interpolation::LerpFloat(eval13_inputs_float)
        } else {
            Interpolation::Lerp16(eval13_inputs)
        }),
        14 => Some(if is_float {
            Interpolation::LerpFloat(eval14_inputs_float)
        } else {
            Interpolation::Lerp16(eval14_inputs)
        }),
        15 => Some(if is_float {
            Interpolation::LerpFloat(eval15_inputs_float)
        } else {
            Interpolation::Lerp16(eval15_inputs)
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `tetra3_f32x4` must be bit-exact vs `tetrahedral_interp_float` —
    /// lane order and per-lane IEEE ops preserved on every arm.
    #[test]
    fn tetra3_f32x4_parity() {
        let n = 33u32;
        let mut tab = Vec::new();
        for z in 0..n {
            for y in 0..n {
                for x in 0..n {
                    for c in 0..3u32 {
                        tab.push(((x * 3 + y * 5 + z * 7 + c * 11) as f32) / 4096.0);
                    }
                }
            }
        }
        let p = InterpParams {
            dw_flags: 0,
            n_inputs: 3,
            n_outputs: 3,
            n_samples: [n; 15],
            domain: [n - 1; 15],
            opta: [3, 3 * n, 3 * n * n, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            table: InterpTable::F32(&tab),
            interpolation: Interpolation::LerpFloat(
                tetrahedral_interp_float as InterpFnFloat,
            ),
            foreign: None,
            kernel: KernelKind::Tetra3F,
        };
        let Some(tok) = x64v3_cached() else {
            return; // non-AVX2 host — nothing to compare against
        };
        let mut diffs = 0u64;
        for i in 0..200000u32 {
            let f = [
                ((i * 37) % 65536) as f32 / 65535.0,
                ((i * 71 + 13) % 65536) as f32 / 65535.0,
                ((i * 113 + 29) % 65536) as f32 / 65535.0,
            ];
            let mut o1 = [0f32; 4];
            tetrahedral_interp_float(&f, &mut o1, &p);
            let o2 = tetra3_f32x4(tok, &f, 3, &p);
            for c in 0..3 {
                if o1[c].to_bits() != o2[c].to_bits() {
                    diffs += 1;
                }
            }
        }
        assert_eq!(diffs, 0);
    }

    /// `eval4_f32x4` must be bit-exact vs `eval4_inputs_float` — two
    /// shifted tetra evals + the rk blend, lane order preserved.
    #[test]
    fn eval4_f32x4_parity() {
        let n = 17u32; // 17^4 grid, 4 outputs
        let mut tab = Vec::new();
        for k in 0..n {
            for z in 0..n {
                for y in 0..n {
                    for x in 0..n {
                        for c in 0..4u32 {
                            tab.push(
                                ((x * 3 + y * 5 + z * 7 + k * 11 + c * 13) as f32)
                                    / 8192.0,
                            );
                        }
                    }
                }
            }
        }
        let p = InterpParams {
            dw_flags: 0,
            n_inputs: 4,
            n_outputs: 4,
            n_samples: [n; 15],
            domain: [n - 1; 15],
            opta: [4, 4 * n, 4 * n * n, 4 * n * n * n, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            table: InterpTable::F32(&tab),
            interpolation: Interpolation::LerpFloat(
                eval4_inputs_float as InterpFnFloat,
            ),
            foreign: None,
            kernel: KernelKind::Eval4F,
        };
        let Some(tok) = x64v3_cached() else {
            return;
        };
        let mut diffs = 0u64;
        for i in 0..200000u32 {
            let f = [
                ((i * 37) % 65536) as f32 / 65535.0,
                ((i * 71 + 13) % 65536) as f32 / 65535.0,
                ((i * 113 + 29) % 65536) as f32 / 65535.0,
                ((i * 151 + 41) % 65536) as f32 / 65535.0,
            ];
            let mut o1 = [0f32; 4];
            eval4_inputs_float(&f, &mut o1, &p);
            let o2 = eval4_f32x4(tok, &f, 4, &p);
            for c in 0..4 {
                if o1[c].to_bits() != o2[c].to_bits() {
                    diffs += 1;
                }
            }
        }
        assert_eq!(diffs, 0);
    }

    /// `trilinear_i32x4` must be bit-exact vs `trilinear_interp16` —
    /// corner loads + the 3-level lerp chain (with u16 truncation)
    /// lane-exact.
    #[test]
    fn trilinear_i32x4_parity() {
        let n = 33u32;
        let mut tab = Vec::new();
        for z in 0..n {
            for y in 0..n {
                for x in 0..n {
                    for c in 0..3u32 {
                        tab.push((((x * 3 + y * 5 + z * 7 + c * 11) * 15) as u16)
                            .wrapping_add(c as u16));
                    }
                }
            }
        }
        let p = InterpParams {
            dw_flags: 0,
            n_inputs: 3,
            n_outputs: 3,
            n_samples: [n; 15],
            domain: [n - 1; 15],
            opta: [3, 3 * n, 3 * n * n, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            table: InterpTable::U16(&tab),
            interpolation: Interpolation::Lerp16(
                trilinear_interp16 as InterpFn16,
            ),
            foreign: None,
            kernel: KernelKind::Trilinear,
        };
        let Some(tok) = x64v3_cached() else {
            return;
        };
        let mut diffs = 0u64;
        for i in 0..200000u32 {
            let f = [
                ((i * 37) % 65536) as u16,
                ((i * 71 + 13) % 65536) as u16,
                ((i * 113 + 29) % 65536) as u16,
            ];
            let mut o1 = [0u16; 4];
            trilinear_interp16(&f, &mut o1, &p);
            let o2 = trilinear_i32x4(tok, f, 3, &p);
            for c in 0..3 {
                if o1[c] != o2[c] {
                    diffs += 1;
                }
            }
        }
        assert_eq!(diffs, 0);
    }

    /// `bilinear_i32x4` vs `bilinear_interp16` — lane-exact 2-D lerp.
    #[test]
    fn bilinear_i32x4_parity() {
        let n = 64u32;
        let mut tab = Vec::new();
        for y in 0..n {
            for x in 0..n {
                for c in 0..3u32 {
                    tab.push((((x * 5 + y * 7 + c * 11) * 23) as u16)
                        .wrapping_add(c as u16));
                }
            }
        }
        let p = InterpParams {
            dw_flags: 0,
            n_inputs: 2,
            n_outputs: 3,
            n_samples: [n; 15],
            domain: [n - 1; 15],
            opta: [3, 3 * n, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            table: InterpTable::U16(&tab),
            interpolation: Interpolation::Lerp16(
                bilinear_interp16 as InterpFn16,
            ),
            foreign: None,
            kernel: KernelKind::Bilinear,
        };
        let Some(tok) = x64v3_cached() else {
            return;
        };
        let mut diffs = 0u64;
        for i in 0..200000u32 {
            let f = [
                ((i * 37) % 65536) as u16,
                ((i * 71 + 13) % 65536) as u16,
            ];
            let mut o1 = [0u16; 4];
            bilinear_interp16(&f, &mut o1, &p);
            let o2 = bilinear_i32x4(tok, f, 3, &p);
            for c in 0..3 {
                if o1[c] != o2[c] {
                    diffs += 1;
                }
            }
        }
        assert_eq!(diffs, 0);
    }

    /// `trilinear_f32x4`/`bilinear_f32x4` vs their scalar kernels —
    /// lane-exact f32 lerp chains.
    #[test]
    fn lin_f32x4_parity() {
        let n = 33u32;
        let mut tab = Vec::new();
        for z in 0..n {
            for y in 0..n {
                for x in 0..n {
                    for c in 0..3u32 {
                        tab.push(((x * 3 + y * 5 + z * 7 + c * 11) as f32) / 4096.0);
                    }
                }
            }
        }
        let p3 = InterpParams {
            dw_flags: 0,
            n_inputs: 3,
            n_outputs: 3,
            n_samples: [n; 15],
            domain: [n - 1; 15],
            opta: [3, 3 * n, 3 * n * n, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            table: InterpTable::F32(&tab),
            interpolation: Interpolation::LerpFloat(
                trilinear_interp_float as InterpFnFloat,
            ),
            foreign: None,
            kernel: KernelKind::TrilinearF,
        };
        // 2-D table over the same data (y-major stride n)
        let p2 = InterpParams {
            n_inputs: 2,
            opta: [3, 3 * n, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            interpolation: Interpolation::LerpFloat(
                bilinear_interp_float as InterpFnFloat,
            ),
            kernel: KernelKind::BilinearF,
            ..p3.clone()
        };
        let Some(tok) = x64v3_cached() else {
            return;
        };
        let mut diffs = 0u64;
        for i in 0..200000u32 {
            let f3 = [
                ((i * 37) % 65536) as f32 / 65535.0,
                ((i * 71 + 13) % 65536) as f32 / 65535.0,
                ((i * 113 + 29) % 65536) as f32 / 65535.0,
            ];
            let mut o1 = [0f32; 4];
            trilinear_interp_float(&f3, &mut o1, &p3);
            let o2 = trilinear_f32x4(tok, &f3, 3, &p3);
            for c in 0..3 {
                if o1[c].to_bits() != o2[c].to_bits() {
                    diffs += 1;
                }
            }
            let f2 = [f3[0], f3[1]];
            let mut b1 = [0f32; 4];
            bilinear_interp_float(&f2, &mut b1, &p2);
            let b2 = bilinear_f32x4(tok, &f2, 3, &p2);
            for c in 0..3 {
                if b1[c].to_bits() != b2[c].to_bits() {
                    diffs += 1;
                }
            }
        }
        assert_eq!(diffs, 0);
    }

    /// `eval1_i32x4`/`eval1_f32x4` vs their scalar kernels — 1-in/3-out.
    #[test]
    fn eval1_x4_parity() {
        let n = 4096u32;
        let mut tab16 = Vec::new();
        let mut tabf = Vec::new();
        for i in 0..n {
            for c in 0..3u32 {
                tab16.push(((i * 7 + c * 1000) & 0xffff) as u16);
                tabf.push(((i * 7 + c * 1000) % 4096) as f32 / 4096.0);
            }
        }
        let p16 = InterpParams {
            dw_flags: 0,
            n_inputs: 1,
            n_outputs: 3,
            n_samples: [n; 15],
            domain: [n - 1; 15],
            opta: [3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            table: InterpTable::U16(&tab16),
            interpolation: Interpolation::Lerp16(eval1_input as InterpFn16),
            kernel: kernel_kind(Interpolation::Lerp16(eval1_input as InterpFn16)),
            foreign: None,
        };
        let pf = InterpParams {
            table: InterpTable::F32(&tabf),
            interpolation: Interpolation::LerpFloat(
                eval1_input_float as InterpFnFloat,
            ),
            ..p16.clone()
        };
        let Some(tok) = x64v3_cached() else {
            return;
        };
        let mut diffs = 0u64;
        for i in 0..65536u32 {
            let v = i as u16;
            let mut o1 = [0u16; 4];
            eval1_input(&[v], &mut o1, &p16);
            let o2 = eval1_i32x4(tok, v, 3, &p16);
            for c in 0..3 {
                if o1[c] != o2[c] {
                    diffs += 1;
                }
            }
            let f = i as f32 / 65535.0;
            let mut f1 = [0f32; 4];
            eval1_input_float(&[f], &mut f1, &pf);
            let f2 = eval1_f32x4(tok, f, 3, &pf);
            for c in 0..3 {
                if f1[c].to_bits() != f2[c].to_bits() {
                    diffs += 1;
                }
            }
        }
        assert_eq!(diffs, 0);
    }
}

#[cfg(test)]
mod vertex_probe {
    use super::*;
    #[test]
    fn tetra3_vs_tetrahedral_vertex0() {
        // 17^3 x 3 table; node(0,0,0) = [0,14607,49958]
        let n = 17usize;
        let mut tab = vec![0u16; n * n * n * 3];
        for i in 0..tab.len() {
            tab[i] = ((i * 97) % 65536) as u16;
        }
        tab[0] = 0; tab[1] = 14607; tab[2] = 49958;
        let mut ns = [1u32; MAX_INPUT_DIMENSIONS];
        let mut dom = [0u32; MAX_INPUT_DIMENSIONS];
        let mut opta = [0u32; MAX_INPUT_DIMENSIONS];
        ns[0] = n as u32; ns[1] = n as u32; ns[2] = n as u32;
        dom[0] = (n - 1) as u32; dom[1] = (n - 1) as u32; dom[2] = (n - 1) as u32;
        opta[0] = 3; opta[1] = (n * 3) as u32; opta[2] = (n * n * 3) as u32;
        let p = InterpParams {
            dw_flags: 0,
            n_inputs: 3,
            n_outputs: 3,
            n_samples: ns,
            domain: dom,
            opta,
            table: InterpTable::U16Rc(std::sync::Arc::from(tab)),
            interpolation: Interpolation::Lerp16(tetrahedral_interp16),
            kernel: kernel_kind(Interpolation::Lerp16(tetrahedral_interp16)),
            foreign: None,
        };
        let mut o_gen = [0u16; 3];
        tetrahedral_interp16(&[0, 0, 0], &mut o_gen, &p);
        let o_t3 = tetra3_interp16([0, 0, 0], &p);
        eprintln!("generic={o_gen:?} tetra3={o_t3:?}");
        assert_eq!(o_gen, [0, 14607, 49958], "generic eval at vertex0");
        assert_eq!(o_t3, o_gen, "tetra3 vs generic at vertex0");
    }
}
