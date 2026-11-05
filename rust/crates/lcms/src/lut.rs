//! Port of `cmslut.c` — pipeline stages (`cmsStage`) and pipelines (`cmsPipeline`).
//!
//! C source: `reference/lcms2/src/cmslut.c`
//! Gate: `tests/c_parity_lut.rs`
//!
//! Representation changes (semantics preserved):
//! - `Stage.Next` → `Option<Box<Stage>>` (same singly-linked chain).
//! - `Data`/`EvalPtr`/`DupElemPtr`/`FreePtr` → an `eval_ptr` fn pointer (same
//!   signature as C) plus a `StageData` enum for owned data; dup is `Clone`,
//!   free is `Drop`.
//! - `_cmsStageCLutData` stores the owned table plus the resolved `InterpParams`
//!   fields (kernel + nSamples/Domain/opta); the params struct is assembled
//!   per eval as in `gamma` — identical arithmetic, no self-referential borrow.

use crate::context::Context;
use crate::err;
use crate::fixed::{quantize_val, quick_saturate_word};
use crate::gamma::{ToneCurve, build_gamma, build_parametric_tone_curve, dup_tone_curve};
use crate::intrp::{
    InterpParams, InterpTable, Interpolation, LERP_FLAGS_16BITS, LERP_FLAGS_FLOAT,
    MAX_INPUT_DIMENSIONS, MAX_STAGE_CHANNELS, compute_interp_params_ex,
};
use crate::mtrx::mat3_solve;
use crate::pcs::{lab2xyz, xyz2lab};
use crate::signatures::stage as sig;
use crate::types::{CieLab, CieXyz, ERROR_RANGE, Mat3, Vec3};

/// `cmsMAXCHANNELS`
pub const MAX_CHANNELS: usize = 16;
/// `MAX_ENCODEABLE_XYZ`
pub const MAX_ENCODEABLE_XYZ: f64 = 1.0 + 32767.0 / 32768.0;
/// `SAMPLER_INSPECT`
pub const SAMPLER_INSPECT: u32 = 0x0100_0000;
/// `JACOBIAN_EPSILON`
const JACOBIAN_EPSILON: f32 = 0.001;
/// `INVERSION_MAX_ITERATIONS`
const INVERSION_MAX_ITERATIONS: usize = 30;

/// `cmsStageLoc`
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StageLoc {
    AtBegin,
    AtEnd,
}

/// `_cmsStageEvalFn` — evaluates `In` → `Out` on the stage.
pub type StageEvalFn = fn(input: &[f32], output: &mut [f32], stage: &Stage);

/// `_cmsStageCLutData`
#[derive(Clone)]
pub struct CLutData {
    /// `Tab.T` / `Tab.TFloat` union → owned vec; `None` mirrors NULL table
    /// (only possible transiently, same as C).
    pub tab16: Option<Vec<u16>>,
    pub tab_float: Option<Vec<f32>>,
    pub n_entries: u32,
    pub has_float_values: bool,
    // Params (resolved kernel + grid geometry — the table pointer is not stored)
    pub n_samples: [u32; MAX_INPUT_DIMENSIONS],
    pub domain: [u32; MAX_INPUT_DIMENSIONS],
    pub opta: [u32; MAX_INPUT_DIMENSIONS],
    pub n_inputs: u32,
    pub n_outputs: u32,
    pub dw_flags: u32,
    pub interpolation: Interpolation,
}

/// `_cmsStageToneCurvesData` — `Vec<ToneCurve>` (TheCurves = Vec elements).
/// `_cmsStageMatrixData` — `Matrix { double, offset }`.
#[derive(Clone)]
pub enum StageData {
    None,
    ToneCurves(Vec<ToneCurve>),
    Matrix {
        double: Vec<f64>,
        offset: Option<Vec<f64>>,
    },
    CLut(Box<CLutData>),
    NamedColor(crate::named::NamedColorList),
}

/// `cmsStage`
#[derive(Clone)]
pub struct Stage {
    pub r#type: u32,
    pub implements: u32,
    pub input_channels: u32,
    pub output_channels: u32,
    pub eval_ptr: StageEvalFn,
    pub data: StageData,
    pub next: Option<Box<Stage>>,
}

impl Stage {
    /// `cmsStageInputChannels`
    pub fn input_channels(&self) -> u32 {
        self.input_channels
    }
    /// `cmsStageOutputChannels`
    pub fn output_channels(&self) -> u32 {
        self.output_channels
    }
    /// `cmsStageType`
    pub fn stage_type(&self) -> u32 {
        self.r#type
    }
    /// `cmsStageNext`
    pub fn next(&self) -> Option<&Stage> {
        self.next.as_deref()
    }
    /// `mpe->EvalPtr(in, out, mpe)`
    #[inline]
    pub fn eval(&self, input: &[f32], output: &mut [f32]) {
        (self.eval_ptr)(input, output, self)
    }
}

/// `_cmsStageAllocPlaceholder` — eval fn + data; dup/free come free with
/// Clone/Drop on `StageData`.
pub fn stage_alloc_placeholder(
    _ctx: Option<&Context>,
    r#type: u32,
    input_channels: u32,
    output_channels: u32,
    eval_ptr: StageEvalFn,
    data: StageData,
) -> Stage {
    Stage {
        r#type,
        implements: r#type, // By default, no clue on what is implementing
        input_channels,
        output_channels,
        eval_ptr,
        data,
        next: None,
    }
}

/// `EvaluateIdentity`
fn evaluate_identity(input: &[f32], output: &mut [f32], mpe: &Stage) {
    output[..mpe.input_channels as usize].copy_from_slice(&input[..mpe.input_channels as usize]);
}

/// `cmsStageAllocIdentity`
pub fn stage_alloc_identity(ctx: Option<&Context>, n_channels: u32) -> Stage {
    stage_alloc_placeholder(
        ctx,
        sig::SigIdentityElemType,
        n_channels,
        n_channels,
        evaluate_identity,
        StageData::None,
    )
}

/// `FromFloatTo16`
fn from_float_to16(input: &[f32], output: &mut [u16], n: usize) {
    for i in 0..n {
        output[i] = quick_saturate_word(input[i] as f64 * 65535.0);
    }
}

/// `From16ToFloat`
fn from_16_to_float(input: &[u16], output: &mut [f32], n: usize) {
    for i in 0..n {
        output[i] = input[i] as f32 / 65535.0;
    }
}

/// `cmsPipelineCheckAndRetreiveStages` — slice instead of varargs; returns the
/// matching stage references (C hands out `cmsStage*` pointers).
pub fn pipeline_check_and_retrieve_stages<'a>(
    lut: &'a Pipeline,
    types: &[u32],
) -> Option<Vec<&'a Stage>> {
    if lut.stage_count() as usize != types.len() {
        return None;
    }
    let mut stages = Vec::with_capacity(types.len());
    let mut mpe = lut.elements.as_deref();
    for &t in types {
        let s = mpe?;
        if s.r#type != t {
            return None;
        }
        stages.push(s);
        mpe = s.next.as_deref();
    }
    Some(stages)
}

// ---------------------------------------------------------------------------
// CurveSet (cmsSigCurveSetElemType)
// ---------------------------------------------------------------------------

/// `_cmsStageGetPtrToCurveSet`
pub fn stage_get_curve_set(mpe: &Stage) -> Option<&Vec<ToneCurve>> {
    match &mpe.data {
        StageData::ToneCurves(c) => Some(c),
        _ => None,
    }
}

/// `EvaluateCurves`
fn evaluate_curves(input: &[f32], output: &mut [f32], mpe: &Stage) {
    if let StageData::ToneCurves(curves) = &mpe.data {
        for (i, c) in curves.iter().enumerate() {
            output[i] = c.eval_float(input[i]);
        }
    }
}

/// `cmsStageAllocToneCurves` — `None` curves means identity (gamma 1.0).
pub fn stage_alloc_tone_curves(
    ctx: Option<&Context>,
    n_channels: u32,
    curves: Option<&[&ToneCurve]>,
) -> Option<Stage> {
    let mut the_curves = Vec::with_capacity(n_channels as usize);
    for i in 0..n_channels as usize {
        let c = match curves {
            None => build_gamma(ctx, 1.0),
            Some(cs) => dup_tone_curve(cs[i], ctx),
        }?;
        the_curves.push(c);
    }
    Some(stage_alloc_placeholder(
        ctx,
        sig::SigCurveSetElemType,
        n_channels,
        n_channels,
        evaluate_curves,
        StageData::ToneCurves(the_curves),
    ))
}

/// `_cmsStageAllocIdentityCurves`
pub fn stage_alloc_identity_curves(ctx: Option<&Context>, n_channels: u32) -> Option<Stage> {
    let mut mpe = stage_alloc_tone_curves(ctx, n_channels, None)?;
    mpe.implements = sig::SigIdentityElemType;
    Some(mpe)
}

// ---------------------------------------------------------------------------
// Matrix (cmsSigMatrixElemType)
// ---------------------------------------------------------------------------

/// `EvaluateMatrix` — uses a f64 accumulator, as C does.
fn evaluate_matrix(input: &[f32], output: &mut [f32], mpe: &Stage) {
    if let StageData::Matrix { double, offset } = &mpe.data {
        for i in 0..mpe.output_channels as usize {
            let mut tmp = 0.0f64;
            for j in 0..mpe.input_channels as usize {
                tmp += input[j] as f64 * double[i * mpe.input_channels as usize + j];
            }
            if let Some(off) = offset {
                tmp += off[i];
            }
            output[i] = tmp as f32;
        }
    }
}

/// `cmsStageAllocMatrix` — `Rows × Cols` matrix + optional offset.
pub fn stage_alloc_matrix(
    ctx: Option<&Context>,
    rows: u32,
    cols: u32,
    matrix: &[f64],
    offset: Option<&[f64]>,
) -> Option<Stage> {
    let n = rows as usize * cols as usize;

    // Check for overflow
    if n == 0
        || rows as u64 * cols as u64 >= u32::MAX as u64 / cols.max(1) as u64
        || rows as u64 * cols as u64 >= u32::MAX as u64 / rows.max(1) as u64
        || n < rows as usize
        || n < cols as usize
    {
        return None;
    }

    let double = matrix[..n].to_vec();
    let offset = offset.map(|o| o[..rows as usize].to_vec());

    Some(stage_alloc_placeholder(
        ctx,
        sig::SigMatrixElemType,
        cols,
        rows,
        evaluate_matrix,
        StageData::Matrix { double, offset },
    ))
}

// ---------------------------------------------------------------------------
// CLUT (cmsSigCLutElemType)
// ---------------------------------------------------------------------------

/// `EvaluateCLUTfloat`
fn evaluate_clut_float(input: &[f32], output: &mut [f32], mpe: &Stage) {
    if let StageData::CLut(d) = &mpe.data {
        let p = InterpParams {
            dw_flags: d.dw_flags,
            n_inputs: d.n_inputs,
            n_outputs: d.n_outputs,
            n_samples: d.n_samples,
            domain: d.domain,
            opta: d.opta,
            table: InterpTable::F32(d.tab_float.as_deref().unwrap_or(&[])),
            interpolation: d.interpolation,
        };
        p.eval_float(input, output);
    }
}

/// `EvaluateCLUTfloatIn16` — convert → 16-bit interp → back to float.
fn evaluate_clut_float_in16(input: &[f32], output: &mut [f32], mpe: &Stage) {
    if let StageData::CLut(d) = &mpe.data {
        let mut in16 = [0u16; MAX_STAGE_CHANNELS];
        let mut out16 = [0u16; MAX_STAGE_CHANNELS];

        from_float_to16(input, &mut in16, mpe.input_channels as usize);
        let p = InterpParams {
            dw_flags: d.dw_flags,
            n_inputs: d.n_inputs,
            n_outputs: d.n_outputs,
            n_samples: d.n_samples,
            domain: d.domain,
            opta: d.opta,
            table: InterpTable::U16(d.tab16.as_deref().unwrap_or(&[])),
            interpolation: d.interpolation,
        };
        p.eval16(&in16, &mut out16[..mpe.output_channels as usize]);
        from_16_to_float(&out16, output, mpe.output_channels as usize);
    }
}

/// `CubeSize` — node count with overflow checks; 0 on failure.
fn cube_size(dims: &[u32], b: usize) -> u32 {
    let mut rv: u64 = 1;
    for bi in (1..=b).rev() {
        let dim = dims[bi - 1] as u64;
        if dim <= 1 {
            return 0;
        }
        if rv > u32::MAX as u64 / dim {
            return 0;
        }
        rv *= dim;
    }
    if rv > u32::MAX as u64 / 15 {
        return 0;
    }
    rv as u32
}

/// `cmsStageAllocCLut16bitGranular`
pub fn stage_alloc_clut16bit_granular(
    ctx: Option<&Context>,
    clut_points: &[u32],
    input_chan: u32,
    output_chan: u32,
    table: Option<&[u16]>,
) -> Option<Stage> {
    if input_chan as usize > MAX_INPUT_DIMENSIONS {
        err::signal_error(ctx, ERROR_RANGE, "Too many input channels (max=15)");
        return None;
    }

    let n_entries = output_chan.wrapping_mul(cube_size(clut_points, input_chan as usize));
    if n_entries == 0 {
        return None;
    }

    let mut tab = vec![0u16; n_entries as usize];
    if let Some(t) = table {
        tab.copy_from_slice(t);
    }

    let data = {
        let p = compute_interp_params_ex(
            ctx,
            clut_points,
            input_chan,
            output_chan,
            InterpTable::U16(&tab),
            LERP_FLAGS_16BITS,
        )?;
        let (n_samples, domain, opta) = (p.n_samples, p.domain, p.opta);
        let (n_inputs, n_outputs, dw_flags, interpolation) =
            (p.n_inputs, p.n_outputs, p.dw_flags, p.interpolation);
        CLutData {
            tab16: Some(tab),
            tab_float: None,
            n_entries,
            has_float_values: false,
            n_samples,
            domain,
            opta,
            n_inputs,
            n_outputs,
            dw_flags,
            interpolation,
        }
    };

    Some(stage_alloc_placeholder(
        ctx,
        sig::SigCLutElemType,
        input_chan,
        output_chan,
        evaluate_clut_float_in16,
        StageData::CLut(Box::new(data)),
    ))
}

/// `cmsStageAllocCLut16bit` — same nodes on every dimension.
pub fn stage_alloc_clut16bit(
    ctx: Option<&Context>,
    n_grid_points: u32,
    input_chan: u32,
    output_chan: u32,
    table: Option<&[u16]>,
) -> Option<Stage> {
    let dims = [n_grid_points; MAX_INPUT_DIMENSIONS];
    stage_alloc_clut16bit_granular(ctx, &dims, input_chan, output_chan, table)
}

/// `cmsStageAllocCLutFloatGranular`
pub fn stage_alloc_clut_float_granular(
    ctx: Option<&Context>,
    clut_points: &[u32],
    input_chan: u32,
    output_chan: u32,
    table: Option<&[f32]>,
) -> Option<Stage> {
    if input_chan as usize > MAX_INPUT_DIMENSIONS {
        err::signal_error(ctx, ERROR_RANGE, "Too many input channels (max=15)");
        return None;
    }

    let n_entries = output_chan.wrapping_mul(cube_size(clut_points, input_chan as usize));
    if n_entries == 0 {
        return None;
    }

    let mut tab = vec![0.0f32; n_entries as usize];
    if let Some(t) = table {
        tab.copy_from_slice(t);
    }

    let data = {
        let p = compute_interp_params_ex(
            ctx,
            clut_points,
            input_chan,
            output_chan,
            InterpTable::F32(&tab),
            LERP_FLAGS_FLOAT,
        )?;
        let (n_samples, domain, opta) = (p.n_samples, p.domain, p.opta);
        let (n_inputs, n_outputs, dw_flags, interpolation) =
            (p.n_inputs, p.n_outputs, p.dw_flags, p.interpolation);
        CLutData {
            tab16: None,
            tab_float: Some(tab),
            n_entries,
            has_float_values: true,
            n_samples,
            domain,
            opta,
            n_inputs,
            n_outputs,
            dw_flags,
            interpolation,
        }
    };

    Some(stage_alloc_placeholder(
        ctx,
        sig::SigCLutElemType,
        input_chan,
        output_chan,
        evaluate_clut_float,
        StageData::CLut(Box::new(data)),
    ))
}

/// `cmsStageAllocCLutFloat`
pub fn stage_alloc_clut_float(
    ctx: Option<&Context>,
    n_grid_points: u32,
    input_chan: u32,
    output_chan: u32,
    table: Option<&[f32]>,
) -> Option<Stage> {
    let dims = [n_grid_points; MAX_INPUT_DIMENSIONS];
    stage_alloc_clut_float_granular(ctx, &dims, input_chan, output_chan, table)
}

/// `IdentitySampler`
fn identity_sampler(input: &[u16], output: &mut [u16], n_chan: usize) -> i32 {
    output[..n_chan].copy_from_slice(&input[..n_chan]);
    1
}

/// `_cmsStageAllocIdentityCLut`
pub fn stage_alloc_identity_clut(ctx: Option<&Context>, n_chan: u32) -> Option<Stage> {
    let dims = [2u32; MAX_INPUT_DIMENSIONS];
    let mut mpe = stage_alloc_clut16bit_granular(ctx, &dims, n_chan, n_chan, None)?;

    if !stage_sample_clut16bit(
        &mut mpe,
        &mut |i_n, out| identity_sampler(i_n, out, n_chan as usize),
        0,
    ) {
        return None;
    }
    mpe.implements = sig::SigIdentityElemType;
    Some(mpe)
}

/// `cmsSAMPLER16` — returns nonzero on success.
pub type Sampler16<'a> = dyn FnMut(&[u16], &mut [u16]) -> i32 + 'a;
/// `cmsSAMPLERFLOAT`
pub type SamplerFloat<'a> = dyn FnMut(&[f32], &mut [f32]) -> i32 + 'a;

/// `cmsStageSampleCLut16bit` — sweep the whole input space on grid knots.
pub fn stage_sample_clut16bit(mpe: &mut Stage, sampler: &mut Sampler16, dw_flags: u32) -> bool {
    let clut = match &mut mpe.data {
        StageData::CLut(c) => c,
        _ => return false,
    };

    let n_samples = clut.n_samples;
    let n_inputs = clut.n_inputs as usize;
    let n_outputs = clut.n_outputs as usize;

    if n_inputs == 0
        || n_outputs == 0
        || n_inputs > MAX_INPUT_DIMENSIONS
        || n_outputs >= MAX_STAGE_CHANNELS
    {
        return false;
    }

    let mut i_n = [0u16; MAX_INPUT_DIMENSIONS + 1];
    let mut out = [0u16; MAX_STAGE_CHANNELS];

    let n_total = cube_size(&n_samples, n_inputs);
    if n_total == 0 {
        return false;
    }

    let mut index = 0usize;
    for i in 0..n_total as i32 {
        let mut rest = i;
        for t in (0..n_inputs).rev() {
            let colorant = rest as u32 % n_samples[t];
            rest /= n_samples[t] as i32;
            i_n[t] = quantize_val(colorant as f64, n_samples[t]);
        }

        if let Some(tab) = &clut.tab16 {
            for t in 0..n_outputs {
                out[t] = tab[index + t];
            }
        }

        if sampler(&i_n, &mut out) == 0 {
            return false;
        }

        if dw_flags & SAMPLER_INSPECT == 0 {
            if let Some(tab) = &mut clut.tab16 {
                for t in 0..n_outputs {
                    tab[index + t] = out[t];
                }
            }
        }

        index += n_outputs;
    }
    true
}

/// `cmsStageSampleCLutFloat`
pub fn stage_sample_clut_float(mpe: &mut Stage, sampler: &mut SamplerFloat, dw_flags: u32) -> bool {
    let clut = match &mut mpe.data {
        StageData::CLut(c) => c,
        _ => return false,
    };

    let n_samples = clut.n_samples;
    let n_inputs = clut.n_inputs as usize;
    let n_outputs = clut.n_outputs as usize;

    if n_inputs == 0
        || n_outputs == 0
        || n_inputs > MAX_INPUT_DIMENSIONS
        || n_outputs >= MAX_STAGE_CHANNELS
    {
        return false;
    }

    let mut i_n = [0.0f32; MAX_INPUT_DIMENSIONS + 1];
    let mut out = [0.0f32; MAX_STAGE_CHANNELS];

    let n_total = cube_size(&n_samples, n_inputs);
    if n_total == 0 {
        return false;
    }

    let mut index = 0usize;
    for i in 0..n_total as i32 {
        let mut rest = i;
        for t in (0..n_inputs).rev() {
            let colorant = rest as u32 % n_samples[t];
            rest /= n_samples[t] as i32;
            i_n[t] = quantize_val(colorant as f64, n_samples[t]) as f32 / 65535.0;
        }

        if let Some(tab) = &clut.tab_float {
            for t in 0..n_outputs {
                out[t] = tab[index + t];
            }
        }

        if sampler(&i_n, &mut out) == 0 {
            return false;
        }

        if dw_flags & SAMPLER_INSPECT == 0 {
            if let Some(tab) = &mut clut.tab_float {
                for t in 0..n_outputs {
                    tab[index + t] = out[t];
                }
            }
        }

        index += n_outputs;
    }
    true
}

/// `cmsSliceSpace16`
pub fn slice_space16(n_inputs: u32, clut_points: &[u32], sampler: &mut Sampler16) -> bool {
    if n_inputs as usize >= MAX_CHANNELS {
        return false;
    }
    let n_total = cube_size(clut_points, n_inputs as usize);
    if n_total == 0 {
        return false;
    }
    let mut i_n = [0u16; MAX_CHANNELS];
    for i in 0..n_total as i32 {
        let mut rest = i;
        for t in (0..n_inputs as usize).rev() {
            let colorant = rest as u32 % clut_points[t];
            rest /= clut_points[t] as i32;
            i_n[t] = quantize_val(colorant as f64, clut_points[t]);
        }
        if sampler(&i_n, &mut []) == 0 {
            return false;
        }
    }
    true
}

/// `cmsSliceSpaceFloat`
pub fn slice_space_float(n_inputs: u32, clut_points: &[u32], sampler: &mut SamplerFloat) -> bool {
    if n_inputs as usize >= MAX_CHANNELS {
        return false;
    }
    let n_total = cube_size(clut_points, n_inputs as usize);
    if n_total == 0 {
        return false;
    }
    let mut i_n = [0.0f32; MAX_CHANNELS];
    for i in 0..n_total as i32 {
        let mut rest = i;
        for t in (0..n_inputs as usize).rev() {
            let colorant = rest as u32 % clut_points[t];
            rest /= clut_points[t] as i32;
            i_n[t] = quantize_val(colorant as f64, clut_points[t]) as f32 / 65535.0;
        }
        if sampler(&i_n, &mut []) == 0 {
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------------------
// Lab/XYZ stages
// ---------------------------------------------------------------------------

/// `EvaluateLab2XYZ` — V4 rules.
fn evaluate_lab2xyz(input: &[f32], output: &mut [f32], _mpe: &Stage) {
    let lab = CieLab {
        L: input[0] as f64 * 100.0,
        a: input[1] as f64 * 255.0 - 128.0,
        b: input[2] as f64 * 255.0 - 128.0,
    };
    let xyz = lab2xyz(None, &lab);
    let adj = MAX_ENCODEABLE_XYZ;
    output[0] = (xyz.X / adj) as f32;
    output[1] = (xyz.Y / adj) as f32;
    output[2] = (xyz.Z / adj) as f32;
}

/// `_cmsStageAllocLab2XYZ`
pub fn stage_alloc_lab2xyz(ctx: Option<&Context>) -> Stage {
    stage_alloc_placeholder(
        ctx,
        sig::SigLab2XYZElemType,
        3,
        3,
        evaluate_lab2xyz,
        StageData::None,
    )
}

/// `_cmsStageAllocLabV2ToV4curves` — 258-entry tables mapping 0xFF00 exactly.
pub fn stage_alloc_lab_v2_to_v4_curves(ctx: Option<&Context>) -> Option<Stage> {
    let mut tables = Vec::new();
    for _ in 0..3 {
        let mut t = crate::gamma::build_tabulated_tone_curve16(ctx, 258, None)?;
        for i in 0..257usize {
            t.table16[i] = ((i * 0xffff + 0x80) >> 8) as u16;
        }
        t.table16[257] = 0xffff;
        tables.push(t);
    }
    let refs: Vec<&ToneCurve> = tables.iter().collect();
    let mut mpe = stage_alloc_tone_curves(ctx, 3, Some(&refs))?;
    mpe.implements = sig::SigLabV2toV4;
    Some(mpe)
}

/// `_cmsStageAllocLabV2ToV4` — matrix form.
pub fn stage_alloc_lab_v2_to_v4(ctx: Option<&Context>) -> Option<Stage> {
    const V2_TO_V4: [f64; 9] = [
        65535.0 / 65280.0,
        0.0,
        0.0,
        0.0,
        65535.0 / 65280.0,
        0.0,
        0.0,
        0.0,
        65535.0 / 65280.0,
    ];
    let mut mpe = stage_alloc_matrix(ctx, 3, 3, &V2_TO_V4, None)?;
    mpe.implements = sig::SigLabV2toV4;
    Some(mpe)
}

/// `_cmsStageAllocLabV4ToV2`
pub fn stage_alloc_lab_v4_to_v2(ctx: Option<&Context>) -> Option<Stage> {
    const V4_TO_V2: [f64; 9] = [
        65280.0 / 65535.0,
        0.0,
        0.0,
        0.0,
        65280.0 / 65535.0,
        0.0,
        0.0,
        0.0,
        65280.0 / 65535.0,
    ];
    let mut mpe = stage_alloc_matrix(ctx, 3, 3, &V4_TO_V2, None)?;
    mpe.implements = sig::SigLabV4toV2;
    Some(mpe)
}

/// `_cmsStageNormalizeFromLabFloat`
pub fn stage_normalize_from_lab_float(ctx: Option<&Context>) -> Option<Stage> {
    const A1: [f64; 9] = [
        1.0 / 100.0,
        0.0,
        0.0,
        0.0,
        1.0 / 255.0,
        0.0,
        0.0,
        0.0,
        1.0 / 255.0,
    ];
    const O1: [f64; 3] = [0.0, 128.0 / 255.0, 128.0 / 255.0];
    let mut mpe = stage_alloc_matrix(ctx, 3, 3, &A1, Some(&O1))?;
    mpe.implements = sig::SigLab2FloatPCS;
    Some(mpe)
}

/// `_cmsStageNormalizeFromXyzFloat`
pub fn stage_normalize_from_xyz_float(ctx: Option<&Context>) -> Option<Stage> {
    const N: f64 = 32768.0 / 65535.0;
    const A1: [f64; 9] = [N, 0.0, 0.0, 0.0, N, 0.0, 0.0, 0.0, N];
    let mut mpe = stage_alloc_matrix(ctx, 3, 3, &A1, None)?;
    mpe.implements = sig::SigXYZ2FloatPCS;
    Some(mpe)
}

/// `_cmsStageNormalizeToLabFloat`
pub fn stage_normalize_to_lab_float(ctx: Option<&Context>) -> Option<Stage> {
    const A1: [f64; 9] = [100.0, 0.0, 0.0, 0.0, 255.0, 0.0, 0.0, 0.0, 255.0];
    const O1: [f64; 3] = [0.0, -128.0, -128.0];
    let mut mpe = stage_alloc_matrix(ctx, 3, 3, &A1, Some(&O1))?;
    mpe.implements = sig::SigFloatPCS2Lab;
    Some(mpe)
}

/// `_cmsStageNormalizeToXyzFloat`
pub fn stage_normalize_to_xyz_float(ctx: Option<&Context>) -> Option<Stage> {
    const N: f64 = 65535.0 / 32768.0;
    const A1: [f64; 9] = [N, 0.0, 0.0, 0.0, N, 0.0, 0.0, 0.0, N];
    let mut mpe = stage_alloc_matrix(ctx, 3, 3, &A1, None)?;
    mpe.implements = sig::SigFloatPCS2XYZ;
    Some(mpe)
}

/// `Clipper` — clips values smaller than zero.
fn clipper(input: &[f32], output: &mut [f32], mpe: &Stage) {
    for i in 0..mpe.input_channels as usize {
        let n = input[i];
        output[i] = if n < 0.0 { 0.0 } else { n };
    }
}

/// `_cmsStageClipNegatives`
pub fn stage_clip_negatives(ctx: Option<&Context>, n_channels: u32) -> Stage {
    stage_alloc_placeholder(
        ctx,
        sig::SigClipNegativesElemType,
        n_channels,
        n_channels,
        clipper,
        StageData::None,
    )
}

/// `EvaluateXYZ2Lab`
fn evaluate_xyz2lab(input: &[f32], output: &mut [f32], _mpe: &Stage) {
    let adj = MAX_ENCODEABLE_XYZ;
    let xyz = CieXyz {
        X: input[0] as f64 * adj,
        Y: input[1] as f64 * adj,
        Z: input[2] as f64 * adj,
    };
    let lab = xyz2lab(None, &xyz);
    output[0] = (lab.L / 100.0) as f32;
    output[1] = ((lab.a + 128.0) / 255.0) as f32;
    output[2] = ((lab.b + 128.0) / 255.0) as f32;
}

/// `_cmsStageAllocXYZ2Lab`
pub fn stage_alloc_xyz2lab(ctx: Option<&Context>) -> Stage {
    stage_alloc_placeholder(
        ctx,
        sig::SigXYZ2LabElemType,
        3,
        3,
        evaluate_xyz2lab,
        StageData::None,
    )
}

/// `_cmsStageAllocLabPrelin` — gamma 1.0 + type-108 curves on a/b.
pub fn stage_alloc_lab_prelin(ctx: Option<&Context>) -> Option<Stage> {
    let params = [2.4f64];
    let l = build_gamma(ctx, 1.0)?;
    let a = build_parametric_tone_curve(ctx, 108, &params)?;
    let b = build_parametric_tone_curve(ctx, 108, &params)?;
    stage_alloc_tone_curves(ctx, 3, Some(&[&l, &a, &b]))
}

// ---------------------------------------------------------------------------
// cmsPipeline
// ---------------------------------------------------------------------------

/// `_cmsPipelineEval16Fn` — `lut->Data` in C is the pipeline itself by default;
/// the optimizer replaces it with private data. We keep `&Pipeline` and let
/// custom evals carry their own state (optimizer stage adds this).
pub type PipelineEval16Fn = fn(input: &[u16], output: &mut [u16], lut: &Pipeline);
/// `_cmsPipelineEvalFloatFn`
pub type PipelineEvalFloatFn = fn(input: &[f32], output: &mut [f32], lut: &Pipeline);

/// `cmsPipeline`
#[derive(Clone)]
pub struct Pipeline {
    pub input_channels: u32,
    pub output_channels: u32,
    /// `Elements` — head of the singly-linked stage chain.
    pub elements: Option<Box<Stage>>,
    pub eval16_fn: PipelineEval16Fn,
    pub eval_float_fn: PipelineEvalFloatFn,
    pub save_as_8bits: bool,
    /// `_cmsPipelineSetOptimizationParameters` — optional baked fast-eval data
    pub opt_data: Option<crate::opt::OptData>,
}

/// `BlessLUT` — set channel counts from first/last stage, check chain.
fn bless_lut(lut: &mut Pipeline) -> bool {
    if let Some(first) = lut.elements.as_deref() {
        let mut last = first;
        // Check chain consistency while finding the last stage
        let mut prev = first;
        while let Some(next) = prev.next.as_deref() {
            if next.input_channels != prev.output_channels {
                return false;
            }
            last = next;
            prev = next;
        }
        lut.input_channels = first.input_channels;
        lut.output_channels = last.output_channels;
    }
    true
}

/// `_LUTeval16` — 16-bit eval through float stages.
fn lut_eval16(input: &[u16], output: &mut [u16], lut: &Pipeline) {
    let mut storage = [[0.0f32; MAX_STAGE_CHANNELS]; 2];
    let mut phase = 0usize;

    from_16_to_float(input, &mut storage[phase], lut.input_channels as usize);

    let mut mpe = lut.elements.as_deref();
    while let Some(s) = mpe {
        let next_phase = phase ^ 1;
        let (i_buf, o_buf) = if phase == 0 {
            let (a, b) = storage.split_at_mut(1);
            (&a[0][..], &mut b[0][..])
        } else {
            let (a, b) = storage.split_at_mut(1);
            (&b[0][..], &mut a[0][..])
        };
        s.eval(i_buf, o_buf);
        phase = next_phase;
        mpe = s.next.as_deref();
    }

    from_float_to16(&storage[phase], output, lut.output_channels as usize);
}

/// `_LUTevalFloat`
fn lut_eval_float(input: &[f32], output: &mut [f32], lut: &Pipeline) {
    let mut storage = [[0.0f32; MAX_STAGE_CHANNELS]; 2];
    let mut phase = 0usize;

    storage[phase][..lut.input_channels as usize]
        .copy_from_slice(&input[..lut.input_channels as usize]);

    let mut mpe = lut.elements.as_deref();
    while let Some(s) = mpe {
        let next_phase = phase ^ 1;
        let (i_buf, o_buf) = if phase == 0 {
            let (a, b) = storage.split_at_mut(1);
            (&a[0][..], &mut b[0][..])
        } else {
            let (a, b) = storage.split_at_mut(1);
            (&b[0][..], &mut a[0][..])
        };
        s.eval(i_buf, o_buf);
        phase = next_phase;
        mpe = s.next.as_deref();
    }

    output[..lut.output_channels as usize]
        .copy_from_slice(&storage[phase][..lut.output_channels as usize]);
}

/// `cmsPipelineAlloc` — zero channels allowed as placeholder.
pub fn pipeline_alloc(
    _ctx: Option<&Context>,
    input_channels: u32,
    output_channels: u32,
) -> Option<Pipeline> {
    if input_channels as usize >= MAX_CHANNELS || output_channels as usize >= MAX_CHANNELS {
        return None;
    }
    let mut lut = Pipeline {
        input_channels,
        output_channels,
        elements: None,
        eval16_fn: lut_eval16,
        eval_float_fn: lut_eval_float,
        save_as_8bits: false,
        opt_data: None,
    };
    if !bless_lut(&mut lut) {
        return None;
    }
    Some(lut)
}

impl Pipeline {
    /// `cmsPipelineInputChannels`
    pub fn input_channels(&self) -> u32 {
        self.input_channels
    }
    /// `cmsPipelineOutputChannels`
    pub fn output_channels(&self) -> u32 {
        self.output_channels
    }
    /// `cmsPipelineEval16`
    pub fn eval16(&self, input: &[u16], output: &mut [u16]) {
        (self.eval16_fn)(input, output, self);
    }
    /// `cmsPipelineEvalFloat`
    pub fn eval_float(&self, input: &[f32], output: &mut [f32]) {
        (self.eval_float_fn)(input, output, self);
    }
    /// `cmsPipelineGetPtrToFirstStage`
    pub fn first_stage(&self) -> Option<&Stage> {
        self.elements.as_deref()
    }
    /// `cmsPipelineGetPtrToLastStage`
    pub fn last_stage(&self) -> Option<&Stage> {
        let mut last = None;
        let mut mpe = self.elements.as_deref();
        while let Some(s) = mpe {
            last = Some(s);
            mpe = s.next.as_deref();
        }
        last
    }
    /// `cmsPipelineStageCount`
    pub fn stage_count(&self) -> u32 {
        let mut n = 0;
        let mut mpe = self.elements.as_deref();
        while let Some(s) = mpe {
            n += 1;
            mpe = s.next.as_deref();
        }
        n
    }
    /// `cmsPipelineSetSaveAs8bitsFlag` — returns previous value.
    pub fn set_save_as_8bits(&mut self, on: bool) -> bool {
        let prev = self.save_as_8bits;
        self.save_as_8bits = on;
        prev
    }

    /// `cmsPipelineInsertStage` — `mpe` is consumed into the chain.
    /// Returns false (and drops mpe) on inconsistent channel chain, like C.
    pub fn insert_stage(&mut self, loc: StageLoc, mut mpe: Stage) -> bool {
        match loc {
            StageLoc::AtBegin => {
                mpe.next = self.elements.take();
                self.elements = Some(Box::new(mpe));
            }
            StageLoc::AtEnd => {
                if self.elements.is_none() {
                    self.elements = Some(Box::new(mpe));
                } else {
                    let mut last = self.elements.as_mut().unwrap();
                    while let Some(ref mut n) = last.next {
                        last = n;
                    }
                    mpe.next = None;
                    last.next = Some(Box::new(mpe));
                }
            }
        }
        bless_lut(self)
    }

    /// `cmsPipelineUnlinkStage` — returns the unlinked stage (C writes it to an
    /// out-param or frees it when the out-param is NULL; we always return it).
    pub fn unlink_stage(&mut self, loc: StageLoc) -> Option<Stage> {
        if self.elements.is_none() {
            return None;
        }
        let unlinked = match loc {
            StageLoc::AtBegin => {
                let mut elem = self.elements.take().unwrap();
                self.elements = elem.next.take();
                Some(*elem)
            }
            StageLoc::AtEnd => {
                // Walk to second-to-last
                if self.elements.as_ref().unwrap().next.is_none() {
                    Some(*self.elements.take().unwrap())
                } else {
                    let mut ant = self.elements.as_mut().unwrap();
                    while ant.next.as_ref().unwrap().next.is_some() {
                        ant = ant.next.as_mut().unwrap();
                    }
                    ant.next.take().map(|b| *b)
                }
            }
        };
        let _ = bless_lut(self);
        unlinked
    }

    /// `cmsPipelineCat` — append dups of l2's stages.
    pub fn cat(&mut self, l2: &Pipeline) -> bool {
        // If both LUTs are empty, inherit channel counts
        if self.elements.is_none() && l2.elements.is_none() {
            self.input_channels = l2.input_channels;
            self.output_channels = l2.output_channels;
        }
        let mut mpe = l2.elements.as_deref();
        while let Some(s) = mpe {
            if !self.insert_stage(StageLoc::AtEnd, (*s).clone()) {
                return false;
            }
            mpe = s.next.as_deref();
        }
        bless_lut(self)
    }

    /// `cmsPipelineDup`
    pub fn dup(&self) -> Option<Pipeline> {
        let mut new_lut = pipeline_alloc(None, self.input_channels, self.output_channels)?;
        let mut mpe = self.elements.as_deref();
        while let Some(s) = mpe {
            let mut dup_stage = (*s).clone();
            dup_stage.next = None;
            if !new_lut.insert_stage(StageLoc::AtEnd, dup_stage) {
                return None;
            }
            mpe = s.next.as_deref();
        }
        new_lut.save_as_8bits = self.save_as_8bits;
        Some(new_lut)
    }

    /// `cmsPipelineEvalReverseFloat` — Newton inversion on 3->3 / 4->3 LUTs.
    pub fn eval_reverse_float(
        &self,
        target: &[f32],
        result: &mut [f32],
        hint: Option<&[f32]>,
    ) -> bool {
        // Only 3->3 and 4->3 are supported
        if self.input_channels != 3 && self.input_channels != 4 {
            return false;
        }
        if self.output_channels != 3 {
            return false;
        }

        let mut x = [0.0f32; 4];
        let mut last_error = 1e20f64;

        // Take the hint as starting point if specified
        match hint {
            None => {
                x[0] = 0.3;
                x[1] = 0.3;
                x[2] = 0.3;
            }
            Some(h) => {
                x[..3].copy_from_slice(&h[..3]);
            }
        }

        if self.input_channels == 4 {
            x[3] = target[3];
        }

        let mut fx = [0.0f32; 4];
        let mut xd: [f32; 4];
        let mut fxd = [0.0f32; 4];

        for _ in 0..INVERSION_MAX_ITERATIONS {
            self.eval_float(&x, &mut fx);

            let error = euclidean_distance(&fx, target, 3);
            if error >= last_error {
                break;
            }
            last_error = error;
            for j in 0..self.input_channels as usize {
                result[j] = x[j];
            }
            if error <= 0.0 {
                break;
            }

            // Jacobian
            let mut jacobian = Mat3::default();
            for j in 0..3 {
                xd = x;
                inc_delta(&mut xd[j]);
                self.eval_float(&xd, &mut fxd);
                jacobian.v[0].n[j] = ((fxd[0] - fx[0]) / JACOBIAN_EPSILON) as f64;
                jacobian.v[1].n[j] = ((fxd[1] - fx[1]) / JACOBIAN_EPSILON) as f64;
                jacobian.v[2].n[j] = ((fxd[2] - fx[2]) / JACOBIAN_EPSILON) as f64;
            }

            let mut tmp2 = Vec3::default();
            tmp2.n[0] = (fx[0] - target[0]) as f64;
            tmp2.n[1] = (fx[1] - target[1]) as f64;
            tmp2.n[2] = (fx[2] - target[2]) as f64;

            let mut tmp = Vec3::default();
            if !mat3_solve(&mut tmp, &jacobian, &tmp2) {
                return false;
            }

            x[0] -= tmp.n[0] as f32;
            x[1] -= tmp.n[1] as f32;
            x[2] -= tmp.n[2] as f32;

            for j in 0..3 {
                if x[j] < 0.0 {
                    x[j] = 0.0;
                } else if x[j] > 1.0 {
                    x[j] = 1.0;
                }
            }
        }
        true
    }
}

/// `IncDelta` — increment with reflexion on boundary.
fn inc_delta(val: &mut f32) {
    if *val < 1.0 - JACOBIAN_EPSILON {
        *val += JACOBIAN_EPSILON;
    } else {
        *val -= JACOBIAN_EPSILON;
    }
}

/// `EuclideanDistance`
fn euclidean_distance(a: &[f32], b: &[f32], n: usize) -> f64 {
    let mut sum = 0.0f32;
    for i in 0..n {
        let dif = b[i] - a[i];
        sum += dif * dif;
    }
    (sum as f32).sqrt() as f64
}
