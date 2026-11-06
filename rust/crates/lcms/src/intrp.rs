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
use std::ffi::c_void;
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
fn foreign_hooks() -> Option<ForeignHooks> {
    FOREIGN_HOOKS.get().copied()
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
        match self.interpolation {
            Interpolation::Lerp16(f) => f(input, output, self),
            Interpolation::LerpFloat(_) => unreachable!("Lerp16 on float params"),
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
        match self.interpolation {
            Interpolation::LerpFloat(f) => f(input, output, self),
            Interpolation::Lerp16(_) => unreachable!("LerpFloat on u16 params"),
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
fn tbl_get16(t: &[u16], i: usize) -> i32 {
    t.get(i).copied().unwrap_or(0) as i32
}

/// Same for float tables.
#[inline]
fn tbl_getf(t: &[f32], i: usize) -> f32 {
    t.get(i).copied().unwrap_or(0.0)
}

/// `LinearInterp` — inline fixed-point interpolation.
#[inline]
fn linear_interp(a: i32, l: i32, h: i32) -> u16 {
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

        let y0 = tbl_get16(lut_table, cell0 as usize) as u16;
        let y1 = tbl_get16(lut_table, cell0 as usize + 1) as u16;

        output[0] = linear_interp(rest, y0 as i32, y1 as i32);
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

        let y0 = tbl_getf(lut_table, cell0 as usize);
        let y1 = tbl_getf(lut_table, cell1 as usize);

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

        let k0i = (p.opta[0] as i32) * k0;
        let k1i = (p.opta[0] as i32) * k1;

        for out_chan in 0..p.n_outputs as usize {
            output[out_chan] = linear_interp(
                rk as i32,
                tbl_get16(lut_table, (k0i + out_chan as i32) as usize),
                tbl_get16(lut_table, (k1i + out_chan as i32) as usize),
            );
        }
    }
}

/// `Eval1InputFloat` — gray LUT, one input channel (float).
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

        for out_chan in 0..p.n_outputs as usize {
            let y0 = tbl_getf(lut_table, cell0 as usize + out_chan);
            let y1 = tbl_getf(lut_table, cell1 as usize + out_chan);

            output[out_chan] = y0 + (y1 - y0) * rest;
        }
    }
}

/// `BilinearInterpFloat` — bilinear interpolation, float version.
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

    for out_chan in 0..total_out {
        let dens = |i: i32, j: i32| tbl_getf(lut_table, (i + j) as usize + out_chan);

        let d00 = dens(x0i, y0i);
        let d01 = dens(x0i, y1);
        let d10 = dens(x1, y0i);
        let d11 = dens(x1, y1);

        let lerp = |a: f32, l: f32, h: f32| l + ((h - l) * a);
        let dx0 = lerp(fx, d00, d10);
        let dx1 = lerp(fx, d01, d11);

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

    for out_chan in 0..total_out {
        let dens = |i: i32, j: i32| tbl_get16(lut_table, (i + j) as usize + out_chan);

        let d00 = dens(x0i, y0i);
        let d01 = dens(x0i, y1);
        let d10 = dens(x1, y0i);
        let d11 = dens(x1, y1);

        let lerp =
            |a: i32, l: i32, h: i32| (l + round_fixed_to_int((h - l).wrapping_mul(a))) as u16;
        let dx0 = lerp(rx, d00, d10) as i32;
        let dx1 = lerp(rx, d01, d11) as i32;

        output[out_chan] = lerp(ry, dx0, dx1);
    }
}

/// `TrilinearInterpFloat` — trilinear, float.
fn trilinear_interp_float(input: &[f32], output: &mut [f32], p: &InterpParams) {
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

    for out_chan in 0..total_out {
        let dens = |i: i32, j: i32, k: i32| tbl_getf(lut_table, (i + j + k) as usize + out_chan);

        let d000 = dens(x0i, y0i, z0i);
        let d001 = dens(x0i, y0i, z1);
        let d010 = dens(x0i, y1, z0i);
        let d011 = dens(x0i, y1, z1);

        let d100 = dens(x1, y0i, z0i);
        let d101 = dens(x1, y0i, z1);
        let d110 = dens(x1, y1, z0i);
        let d111 = dens(x1, y1, z1);

        let lerp = |a: f32, l: f32, h: f32| l + ((h - l) * a);
        let dx00 = lerp(fx, d000, d100);
        let dx01 = lerp(fx, d001, d101);
        let dx10 = lerp(fx, d010, d110);
        let dx11 = lerp(fx, d011, d111);

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
    let last = cell.len() - 1;
    for out_chan in 0..total_out {
        let dens = |i: i32, j: i32, k: i32| {
            cell[(((i + j + k).min(max_off)) as usize + out_chan).min(last)] as i32
        };

        let d000 = dens(x0i, y0i, z0i);
        let d001 = dens(x0i, y0i, z1);
        let d010 = dens(x0i, y1, z0i);
        let d011 = dens(x0i, y1, z1);

        let d100 = dens(x1, y0i, z0i);
        let d101 = dens(x1, y0i, z1);
        let d110 = dens(x1, y1, z0i);
        let d111 = dens(x1, y1, z1);

        let lerp =
            |a: i32, l: i32, h: i32| (l + round_fixed_to_int((h - l).wrapping_mul(a))) as u16;
        let dx00 = lerp(rx, d000, d100) as i32;
        let dx01 = lerp(rx, d001, d101) as i32;
        let dx10 = lerp(rx, d010, d110) as i32;
        let dx11 = lerp(rx, d011, d111) as i32;

        let dxy0 = lerp(ry, dx00, dx10) as i32;
        let dxy1 = lerp(ry, dx01, dx11) as i32;

        output[out_chan] = lerp(rz, dxy0, dxy1);
    }
}

/// `TetrahedralInterpFloat` — Sakamoto tetrahedral interpolation, float.
#[inline]
fn tetrahedral_interp_float(input: &[f32], output: &mut [f32], p: &InterpParams) {
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

    for out_chan in 0..total_out {
        let dens = |i: i32, j: i32, k: i32| tbl_getf(lut_table, (i + j + k) as usize + out_chan);

        // These are the 6 Tetrahedral
        let c0 = dens(x0i, y0i, z0i);
        let (c1, c2, c3);

        if rx >= ry && ry >= rz {
            c1 = dens(x1, y0i, z0i) - c0;
            c2 = dens(x1, y1, z0i) - dens(x1, y0i, z0i);
            c3 = dens(x1, y1, z1) - dens(x1, y1, z0i);
        } else if rx >= rz && rz >= ry {
            c1 = dens(x1, y0i, z0i) - c0;
            c2 = dens(x1, y1, z1) - dens(x1, y0i, z1);
            c3 = dens(x1, y0i, z1) - dens(x1, y0i, z0i);
        } else if rz >= rx && rx >= ry {
            c1 = dens(x1, y0i, z1) - dens(x0i, y0i, z1);
            c2 = dens(x1, y1, z1) - dens(x1, y0i, z1);
            c3 = dens(x0i, y0i, z1) - c0;
        } else if ry >= rx && rx >= rz {
            c1 = dens(x1, y1, z0i) - dens(x0i, y1, z0i);
            c2 = dens(x0i, y1, z0i) - c0;
            c3 = dens(x1, y1, z1) - dens(x1, y1, z0i);
        } else if ry >= rz && rz >= rx {
            c1 = dens(x1, y1, z1) - dens(x0i, y1, z1);
            c2 = dens(x0i, y1, z0i) - c0;
            c3 = dens(x0i, y1, z1) - dens(x0i, y1, z0i);
        } else if rz >= ry && ry >= rx {
            c1 = dens(x1, y1, z1) - dens(x0i, y1, z1);
            c2 = dens(x0i, y1, z1) - dens(x0i, y0i, z1);
            c3 = dens(x0i, y0i, z1) - c0;
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

    let x0i = (p.opta[2] as u32).wrapping_mul(x0 as u32);
    let mut x1 = if input[0] == 0xFFFF { 0 } else { p.opta[2] };

    let y0i = (p.opta[1] as u32).wrapping_mul(y0 as u32);
    let mut y1 = if input[1] == 0xFFFF { 0 } else { p.opta[1] };

    let z0i = (p.opta[0] as u32).wrapping_mul(z0 as u32);
    let mut z1 = if input[2] == 0xFFFF { 0 } else { p.opta[0] };

    let base = (x0i + y0i + z0i) as usize;
    let mut li = base; // index into lut_table, incremented per output channel
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

/// `Eval4Inputs` — CMYK: two tetrahedral cells + linear blend, fixed point.
#[inline]
#[inline]
pub(crate) fn eval4_inputs(input: &[u16], output: &mut [u16], p: &InterpParams) {
    let lut_table = p.table_u16();
    let mut tmp1 = [0u16; MAX_STAGE_CHANNELS];
    let mut tmp2 = [0u16; MAX_STAGE_CHANNELS];

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
            diffs: F,
        ) {
            let o1 = o1.min(max_off);
            let o2 = o2.min(max_off);
            let o3 = o3.min(max_off);
            for oc in 0..n_outputs {
                let c0 = cell[oc] as i32;
                let t1 = cell[(o1 + oc as i32) as usize] as i32;
                let t2 = cell[(o2 + oc as i32) as usize] as i32;
                let t3 = cell[(o3 + oc as i32) as usize] as i32;
                let (c1, c2, c3) = diffs(c0, t1, t2, t3);
                let rest = c1
                    .wrapping_mul(rx)
                    .wrapping_add(c2.wrapping_mul(ry))
                    .wrapping_add(c3.wrapping_mul(rz))
                    .wrapping_add(0x8001);
                out[oc] = (c0 as u16).wrapping_add((rest.wrapping_add(rest >> 16) >> 16) as u16);
            }
        }

        if rx >= ry {
            if ry >= rz {
                arm(cell, d_x, d_x + d_y, max_off, max_off, rx, ry, rz, n_outputs, out, |c0, t1, t2, t3| {
                    (t1 - c0, t2 - t1, t3 - t2)
                });
            } else if rz >= rx {
                arm(cell, d_x + d_z, max_off, d_z, max_off, rx, ry, rz, n_outputs, out, |c0, t1, t2, t3| {
                    (t1 - t3, t2 - t1, t3 - c0)
                });
            } else {
                arm(cell, d_x, max_off, d_x + d_z, max_off, rx, ry, rz, n_outputs, out, |c0, t1, t2, t3| {
                    (t1 - c0, t2 - t3, t3 - t1)
                });
            }
        } else if rx >= rz {
            arm(cell, d_x + d_y, d_y, max_off, max_off, rx, ry, rz, n_outputs, out, |c0, t1, t2, t3| {
                (t1 - t2, t2 - c0, t3 - t1)
            });
        } else if ry >= rz {
            arm(cell, max_off, d_y, d_y + d_z, max_off, rx, ry, rz, n_outputs, out, |c0, t1, t2, t3| {
                (t1 - t3, t2 - c0, t3 - t2)
            });
        } else {
            arm(cell, max_off, d_y + d_z, d_z, max_off, rx, ry, rz, n_outputs, out, |c0, t1, t2, t3| {
                (t1 - t2, t2 - t3, t3 - c0)
            });
        }
    }

    let n_outputs = p.n_outputs as usize;
    tetra16(
        lut_table, k0i, x0i, x1, y0i, y1, z0i, z1, rx, ry, rz, n_outputs, &mut tmp1,
    );
    tetra16(
        lut_table, k1, x0i, x1, y0i, y1, z0i, z1, rx, ry, rz, n_outputs, &mut tmp2,
    );

    for i in 0..n_outputs {
        output[i] = linear_interp(rk, tmp1[i] as i32, tmp2[i] as i32);
    }
}

/// `Eval4InputsFloat` — two tetrahedral evals + lerp, float.
#[inline]
fn eval4_inputs_float(input: &[f32], output: &mut [f32], p: &InterpParams) {
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
        4 => eval4_inputs(input, output, p),
        _ => eval_chain16(input, output, p, n),
    }
}

fn eval_dispatch_float(input: &[f32], output: &mut [f32], p: &InterpParams, n: usize) {
    match n {
        4 => eval4_inputs_float(input, output, p),
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
