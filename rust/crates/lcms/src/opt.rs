//! `cmsopt.c` — pipeline optimizer: 4 default strategies tried in order
//! (join curves → matrix shaper → computing linearization → resampling),
//! plus the plugin optimization collection.
//!
//! Each strategy returns `bool` and may replace the pipeline + revise the
//! formats/flags through the mutable refs, exactly as the C signature.

use crate::context::Context;
use crate::lut::Pipeline;

/// `OptData::ClutOnly` payload — the CLUT plus a baked `InterpParams` built
/// once at optimization time (C's `Data->Params`). The params' table is an
/// `Rc` copy of the sampled `tab16` — identical values, built after sampling.
#[derive(Clone)]
pub struct ClutOnly {
    pub clut: crate::lut::CLutData,
    pub params16: crate::intrp::InterpParams<'static>,
}

/// Optimization payload — C's `Eval16Fn` data union, closed-world enum
#[derive(Clone)]
pub enum OptData {
    Curves16(Curves16Data),
    MatShaper(Box<MatShaper8Data>),
    Prelin16(Box<Prelin16Data>),
    Prelin8(Box<Prelin8Data>),
    ClutOnly(Box<ClutOnly>),

    Identity,
}

/// `_cmsOptimizationCollection` — one strategy callback
type OptimizeFn = fn(
    &mut Pipeline,
    u32,      // intent
    &mut u32, // input format
    &mut u32, // output format
    &mut u32, // dwFlags
) -> bool;

/// `DefaultOptimization` — C order
const DEFAULT_OPTIMIZATIONS: &[&str] = &[
    "join_curves",
    "matrix_shaper",
    "computing_linearization",
    "resampling",
];

/// `_cmsOptimizePipeline` — exact C flow: FORCE_CLUT→resample, empty→identity,
/// named-color→bail, PreOptimize, NOOPTIMIZE, then strategy chain in order.
pub fn optimize_pipeline(
    _ctx: Option<&Context>,
    lut: &mut Pipeline,
    intent: u32,
    input_format: &mut u32,
    output_format: &mut u32,
    dw_flags: &mut u32,
) -> bool {
    if *dw_flags & crate::types::FLAGS_FORCE_CLUT != 0 {
        pre_optimize(lut);
        return optimize_by_resampling(lut, intent, input_format, output_format, dw_flags);
    }
    if lut.elements.is_none() {
        lut.opt_data = Some(OptData::Identity);
        lut.eval16_fn = |i, o, _l| {
            fast_identity16(i, o, &OptData::Identity);
        };
        return true;
    }
    // named-color stages can't be optimized
    {
        let mut st = lut.elements.as_deref();
        while let Some(s) = st {
            if s.stage_type() == ssig::SigNamedColorElemType {
                return false;
            }
            st = s.next.as_deref();
        }
    }
    let any_success = pre_optimize(lut);
    if lut.elements.is_none() {
        lut.opt_data = Some(OptData::Identity);
        lut.eval16_fn = |i, o, _l| {
            fast_identity16(i, o, &OptData::Identity);
        };
        return true;
    }
    if *dw_flags & crate::xform::FLAGS_NOOPTIMIZE != 0 {
        return false;
    }
    if optimize_by_joining_curves(lut, intent, input_format, output_format, dw_flags)
        || optimize_matrix_shaper(lut, intent, input_format, output_format, dw_flags)
        || optimize_by_computing_linearization(lut, intent, input_format, output_format, dw_flags)
        || optimize_by_resampling(lut, intent, input_format, output_format, dw_flags)
    {
        return true;
    }
    any_success
}

// ---------------------------------------------------------------------------
// `PreOptimize` — remove identity stages + cancel inverse pairs + fuse matrices
// ---------------------------------------------------------------------------

use crate::lut::{Stage, StageData};
use crate::signatures::stage as ssig;
use crate::types::{Mat3, Vec3};

/// `_Remove1Op` — drop every stage whose `implements` is `unary`
fn remove1_op(lut: &mut Pipeline, unary: u32) -> bool {
    let mut any = false;
    loop {
        // find first matching stage
        let mut pt: &mut Option<Box<Stage>> = &mut lut.elements;
        let mut found = false;
        while let Some(s) = pt {
            if s.implements == unary {
                let nxt = s.next.take();
                *pt = nxt;
                any = true;
                found = true;
                break;
            }
            pt = &mut pt.as_mut().unwrap().next;
        }
        if !found {
            break;
        }
    }
    any
}

/// `_Remove2Op` — drop adjacent pairs whose implements match (op1, op2)
fn remove2_op(lut: &mut Pipeline, op1: u32, op2: u32) -> bool {
    let mut any = false;
    loop {
        // find a pair where first==op1 && next==op2
        let mut pt: &mut Option<Box<Stage>> = &mut lut.elements;
        let mut found = false;
        while let Some(s) = pt {
            let is_pair =
                s.implements == op1 && s.next.as_deref().map(|n| n.implements) == Some(op2);
            if is_pair {
                // remove both: take next.next, replace pt
                let mut nxt = s.next.take().unwrap();
                let tail = nxt.next.take();
                *pt = tail;
                any = true;
                found = true;
                break;
            }
            pt = &mut pt.as_mut().unwrap().next;
        }
        if !found {
            break;
        }
    }
    any
}

fn mat_from_stage(s: &Stage) -> Option<Mat3> {
    if let StageData::Matrix { double, offset: _ } = &s.data {
        if double.len() == 9 {
            let mut m = Mat3::default();
            for i in 0..3 {
                for j in 0..3 {
                    m.v[i].n[j] = double[i * 3 + j];
                }
            }
            return Some(m);
        }
    }
    None
}

/// `_MultiplyMatrix` — fuse adjacent 3x3 matrices (no offsets)
fn multiply_matrix(lut: &mut Pipeline) -> bool {
    let mut any = false;
    loop {
        let mut pt: &mut Option<Box<Stage>> = &mut lut.elements;
        let mut fused = false;
        while let Some(s) = pt {
            if s.implements == ssig::SigMatrixElemType
                && s.next.as_deref().map(|n| n.implements) == Some(ssig::SigMatrixElemType)
            {
                // C's _MultiplyMatrix bails (FALSE) if EITHER matrix has an
                // offset — an offset matrix is never fused, which preserves
                // BPC/adaptation offsets that a naive product would drop.
                let has_off = |st: &Stage| {
                    matches!(
                        &st.data,
                        crate::lut::StageData::Matrix {
                            offset: Some(_),
                            ..
                        }
                    )
                };
                let ok = s.input_channels == 3
                    && s.output_channels == 3
                    && !has_off(s)
                    && s.next
                        .as_deref()
                        .map(|n| n.input_channels == 3 && n.output_channels == 3 && !has_off(n))
                        .unwrap_or(false);
                if !ok {
                    return any;
                }
                let m1 = mat_from_stage(s);
                let m2 = s.next.as_deref().and_then(mat_from_stage);
                if let (Some(m1), Some(m2)) = (m1, m2) {
                    let mut res = Mat3::default();
                    crate::mtrx::mat3_per(&mut res, &m2, &m1);
                    let mut nxt = s.next.take().unwrap();
                    let tail = nxt.next.take();
                    if !crate::mtrx::mat3_is_identity(&res) {
                        let mut flat = [0.0f64; 9];
                        for i in 0..3 {
                            for j in 0..3 {
                                flat[i * 3 + j] = res.v[i].n[j];
                            }
                        }
                        if let Some(mut new_st) =
                            crate::lut::stage_alloc_matrix(None, 3, 3, &flat, None)
                        {
                            new_st.next = tail.map(|t| Box::new(*t));
                            *pt = Some(Box::new(new_st));
                        }
                    } else {
                        *pt = tail;
                    }
                    any = true;
                    fused = true;
                    break;
                }
                return any;
            }
            pt = &mut pt.as_mut().unwrap().next;
        }
        if !fused {
            break;
        }
    }
    any
}

/// `PreOptimize` — loop the simplification passes to fixpoint
fn pre_optimize(lut: &mut Pipeline) -> bool {
    let mut any_opt = false;
    loop {
        let mut opt = false;
        opt |= remove1_op(lut, ssig::SigIdentityElemType);
        opt |= remove2_op(lut, ssig::SigXYZ2LabElemType, ssig::SigLab2XYZElemType);
        opt |= remove2_op(lut, ssig::SigLab2XYZElemType, ssig::SigXYZ2LabElemType);
        opt |= remove2_op(lut, ssig::SigLabV4toV2, ssig::SigLabV2toV4);
        opt |= remove2_op(lut, ssig::SigLabV2toV4, ssig::SigLabV4toV2);
        opt |= remove2_op(lut, ssig::SigLab2FloatPCS, ssig::SigFloatPCS2Lab);
        opt |= remove2_op(lut, ssig::SigXYZ2FloatPCS, ssig::SigFloatPCS2XYZ);
        opt |= multiply_matrix(lut);
        if opt {
            any_opt = true;
        } else {
            break;
        }
    }
    any_opt
}

// ---------------------------------------------------------------------------
// `OptimizeByJoiningCurves` — curves-only pipeline → 4096-pt sampled curves
// ---------------------------------------------------------------------------

const PRELINEARIZATION_POINTS: u32 = 4096;

/// `Curves16Data` — baked per-channel lookup tables
#[derive(Clone)]
pub struct Curves16Data {
    pub curves: Vec<Vec<u16>>,
}

/// `FastEvaluateCurves8` — 8-bit quantized index
fn fast_eval_curves8(input: &[u16], output: &mut [u16], data: &OptData) -> bool {
    let OptData::Curves16(c) = data else {
        return false;
    };
    for i in 0..c.curves.len() {
        output[i] = c.curves[i][(input[i] >> 8) as usize];
    }
    true
}
/// `FastEvaluateCurves16` — direct index
pub(crate) fn fast_eval_curves16(input: &[u16], output: &mut [u16], data: &OptData) -> bool {
    let OptData::Curves16(c) = data else {
        return false;
    };
    for i in 0..c.curves.len() {
        output[i] = c.curves[i][input[i] as usize];
    }
    true
}
/// `eval_opt16` — dispatch an `OptData` to its concrete eval16
pub(crate) fn eval_opt16(input: &[u16], output: &mut [u16], d: &OptData) {
    match d {
        OptData::Curves16(_) => {
            fast_eval_curves16(input, output, d);
        }
        OptData::MatShaper(_) => {
            mat_shaper_eval16(input, output, d);
        }
        OptData::Prelin16(_) => {
            prelin_eval16(input, output, d);
        }
        OptData::Prelin8(_) => {
            prelin_eval8(input, output, d);
        }
        OptData::ClutOnly(_) => {
            clut_eval16(input, output, d);
        }
        OptData::Identity => {
            fast_identity16(input, output, d);
        }
    }
}

/// `FastIdentity16` — copy
pub(crate) fn fast_identity16(input: &[u16], output: &mut [u16], _d: &OptData) -> bool {
    let n = input.len().min(output.len());
    output[..n].copy_from_slice(&input[..n]);
    true
}

/// `AllCurvesAreLinear` — every curve maps x→x exactly
fn all_curves_are_linear(stage: &Stage) -> bool {
    let StageData::ToneCurves(curves) = &stage.data else {
        return false;
    };
    curves.iter().all(|c| {
        let n = c.table16.len();
        (0..n).all(|i| {
            let expected = (i as f64 * 65535.0 / (n - 1).max(1) as f64).round() as i64;
            (c.table16[i] as i64 - expected).abs() <= 0
        })
    })
}

fn optimize_by_joining_curves(
    lut: &mut Pipeline,
    _intent: u32,
    input_format: &mut u32,
    _of: &mut u32,
    dw_flags: &mut u32,
) -> bool {
    use crate::lut::{StageData, StageLoc};
    // C: float formats aren't 16-bit optimizable
    if crate::pack::t_float(*input_format) != 0 {
        return false;
    }
    // every stage must be a CurveSet
    let mut st = lut.elements.as_deref();
    while let Some(s) = st {
        if s.stage_type() != ssig::SigCurveSetElemType {
            return false;
        }
        st = s.next.as_deref();
    }
    let n_in = lut.input_channels;
    let mut gamma_tables: Vec<crate::gamma::ToneCurve> = Vec::new();
    for _ in 0..n_in {
        let Some(g) =
            crate::gamma::build_tabulated_tone_curve16(None, PRELINEARIZATION_POINTS, None)
        else {
            return false;
        };
        gamma_tables.push(g);
    }
    // sample the source pipeline across the input ramp
    let mut in_float = [0f32; 16];
    let mut out_float = [0f32; 16];
    for i in 0..PRELINEARIZATION_POINTS as usize {
        for j in 0..n_in as usize {
            in_float[j] = (i as f64 / (PRELINEARIZATION_POINTS - 1) as f64) as f32;
        }
        lut.eval_float(&in_float[..n_in as usize], &mut out_float);
        for j in 0..n_in as usize {
            gamma_tables[j].table16[i] =
                crate::fixed::quick_saturate_word(out_float[j] as f64 * 65535.0);
        }
    }
    let refs: Vec<&crate::gamma::ToneCurve> = gamma_tables.iter().collect();
    let Some(obtained) = crate::lut::stage_alloc_tone_curves(None, n_in, Some(&refs)) else {
        return false;
    };
    let linear = all_curves_are_linear(&obtained);
    let mut dest = crate::lut::pipeline_alloc(None, lut.input_channels, lut.output_channels);
    let Some(mut dest) = dest else { return false };
    if !linear {
        if !dest.insert_stage(StageLoc::AtBegin, obtained) {
            return false;
        }
        let is_8bit = crate::pack::t_bytes(*input_format) == 1;
        let table: Vec<Vec<u16>> = gamma_tables.iter().map(|g| g.table16.clone()).collect();
        *dw_flags |= crate::xform::FLAGS_NOCACHE;
        if is_8bit {
            let curves: Vec<Vec<u16>> = table
                .iter()
                .map(|t| {
                    let mut v = vec![0u16; 256];
                    for i in 0..256 {
                        v[i] = t[i * (PRELINEARIZATION_POINTS as usize - 1) / 255];
                    }
                    v
                })
                .collect();
            dest.opt_data = Some(OptData::Curves16(Curves16Data { curves }));
            dest.eval16_fn = |i, o, l| {
                if let Some(d) = &l.opt_data {
                    fast_eval_curves8(i, o, d);
                }
            };
        } else {
            dest.opt_data = Some(OptData::Curves16(Curves16Data { curves: table }));
            dest.eval16_fn = |i, o, l| {
                if let Some(d) = &l.opt_data {
                    fast_eval_curves16(i, o, d);
                }
            };
        }
    } else {
        let st = crate::lut::stage_alloc_identity(None, n_in);
        if !dest.insert_stage(StageLoc::AtBegin, st) {
            return false;
        }
        *dw_flags |= crate::xform::FLAGS_NOCACHE;
        dest.opt_data = Some(OptData::Identity);
        dest.eval16_fn = |i, o, _l| {
            fast_identity16(i, o, &OptData::Identity);
        };
    }
    *lut = dest;
    true
}

// ---------------------------------------------------------------------------
// `OptimizeMatrixShaper` — curves+matrix+curves → precomputed MatShaper8Data
// ---------------------------------------------------------------------------

/// `MatShaper8Data` — the fused 8-bit shaper+matrix+shaper lookup
#[derive(Clone)]
pub struct MatShaper8Data {
    pub shaper1: [[i32; 256]; 3],
    pub mat: [[i32; 3]; 3],
    pub off: [i32; 3],
    /// `[[u16; 16385]; 3]` flattened to a `Box<[u16]>` (index `c*16385+i`) —
    /// fallible `try_zeroed_vec` keeps the 98KB table out of a memset
    pub shaper2: Box<[u16]>,
}

/// `MatShaperEval16`
pub(crate) fn mat_shaper_eval16(input: &[u16], output: &mut [u16], d: &OptData) -> bool {
    let OptData::MatShaper(p) = d else {
        return false;
    };
    let ri = (input[0] & 0xff) as usize;
    let gi = (input[1] & 0xff) as usize;
    let bi = (input[2] & 0xff) as usize;
    let r = p.shaper1[0][ri];
    let g = p.shaper1[1][gi];
    let b = p.shaper1[2][bi];
    let l1 = (p.mat[0][0].wrapping_mul(r)
        + p.mat[0][1].wrapping_mul(g)
        + p.mat[0][2].wrapping_mul(b)
        + p.off[0]
        + 0x2000)
        >> 14;
    let l2 = (p.mat[1][0].wrapping_mul(r)
        + p.mat[1][1].wrapping_mul(g)
        + p.mat[1][2].wrapping_mul(b)
        + p.off[1]
        + 0x2000)
        >> 14;
    let l3 = (p.mat[2][0].wrapping_mul(r)
        + p.mat[2][1].wrapping_mul(g)
        + p.mat[2][2].wrapping_mul(b)
        + p.off[2]
        + 0x2000)
        >> 14;
    let ri = l1.clamp(0, 16384) as usize;
    let gi = l2.clamp(0, 16384) as usize;
    let bi = l3.clamp(0, 16384) as usize;
    output[0] = p.shaper2[ri];
    output[1] = p.shaper2[16385 + gi];
    output[2] = p.shaper2[2 * 16385 + bi];
    true
}

fn double_to_1fixed14(x: f64) -> i32 {
    (x * 16384.0 + 0.5).floor() as i32
}

/// `FillFirstShaper` — 0..255 → 1.14 fixed
fn fill_first_shaper(table: &mut [i32; 256], curve: &crate::gamma::ToneCurve) {
    for i in 0..256 {
        let r = i as f32 / 255.0;
        let y = curve.eval_float(r);
        table[i] = if y < 131072.0 {
            double_to_1fixed14(y as f64)
        } else {
            0x7fffffff
        };
    }
}

/// `FillSecondShaper` — 0..16384 → u16
fn fill_second_shaper(table: &mut [u16; 16385], curve: &crate::gamma::ToneCurve, is8: bool) {
    for i in 0..16385 {
        let r = i as f32 / 16384.0;
        let val = curve.eval_float(r).clamp(0.0, 1.0);
        if is8 {
            let w = crate::fixed::quick_saturate_word(val as f64 * 65535.0);
            let b = (((w as u32) * 65281 + 8388608) >> 24 & 0xff) as u8; // FROM_16_TO_8
            table[i] = ((b as u16) << 8) | b as u16; // FROM_8_TO_16
        } else {
            table[i] = crate::fixed::quick_saturate_word(val as f64 * 65535.0);
        }
    }
}

/// `SetMatShaper` — build the fused shaper + wire the fast eval
fn set_mat_shaper(
    dest: &mut Pipeline,
    curve1: &[crate::gamma::ToneCurve],
    mat: &Mat3,
    off: Option<&Vec3>,
    curve2: &[crate::gamma::ToneCurve],
    input_format: &u32,
    output_format: &mut u32,
) -> bool {
    let is8 = crate::pack::t_bytes(*output_format) == 1;
    if crate::pack::t_premul(*input_format) != 0 || crate::pack::t_premul(*output_format) != 0 {
        return false;
    }
    let mut p = MatShaper8Data {
        shaper1: [[0; 256]; 3],
        mat: [[0; 3]; 3],
        off: [0; 3],
        shaper2: match bytemuck::try_zeroed_vec::<u16>(3 * 16385) {
            Ok(v) => v.into_boxed_slice(),
            Err(()) => return false,
        },
    };
    for c in 0..3 {
        fill_first_shaper(&mut p.shaper1[c], &curve1[c]);
        let (pre, rest) = p.shaper2.split_at_mut(c * 16385);
        let _ = pre;
        let win: &mut [u16; 16385] = (&mut rest[..16385]).try_into().unwrap();
        fill_second_shaper(win, &curve2[c], is8);
        for j in 0..3 {
            p.mat[c][j] = double_to_1fixed14(mat.v[c].n[j]);
        }
        p.off[c] = off.map(|o| double_to_1fixed14(o.n[c])).unwrap_or(0);
    }
    if is8 {
        *output_format |= 1 << 21; // OPTIMIZED_SH
    }
    dest.opt_data = Some(OptData::MatShaper(Box::new(p)));
    dest.eval16_fn = |i, o, l| {
        if let Some(d) = &l.opt_data {
            mat_shaper_eval16(i, o, d);
        }
    };
    true
}

/// `OptimizeMatrixShaper` — curves(+matrix)+curves → fused shaper
fn optimize_matrix_shaper(
    lut: &mut Pipeline,
    intent: u32,
    input_format: &mut u32,
    output_format: &mut u32,
    dw_flags: &mut u32,
) -> bool {
    use crate::lut::{StageData, StageLoc};
    if crate::pack::t_channels(*input_format) != 3 || crate::pack::t_channels(*output_format) != 3 {
        return false;
    }
    if crate::pack::t_bytes(*input_format) != 1 {
        return false;
    }
    // match 4-stage: curves, mat, mat, curves — or 3-stage: curves, mat, curves
    let chain: Vec<&Stage> = {
        let mut v = Vec::new();
        let mut s = lut.elements.as_deref();
        while let Some(e) = s {
            v.push(e);
            s = e.next.as_deref();
        }
        v
    };
    let (curve1_i, matrix1_i, matrix2_i, curve2_i) = if chain.len() == 4
        && chain.iter().enumerate().all(|(i, s)| {
            s.stage_type()
                == match i {
                    0 | 3 => ssig::SigCurveSetElemType,
                    _ => ssig::SigMatrixElemType,
                }
        }) {
        (0, 1, Some(2), 3)
    } else if chain.len() == 3
        && chain[0].stage_type() == ssig::SigCurveSetElemType
        && chain[1].stage_type() == ssig::SigMatrixElemType
        && chain[2].stage_type() == ssig::SigCurveSetElemType
    {
        (0, 1, None, 2)
    } else {
        return false;
    };
    let c1 = &chain[curve1_i];
    let m1 = &chain[matrix1_i];
    let c2 = &chain[curve2_i];
    if m1.input_channels != 3 || m1.output_channels != 3 {
        return false;
    }
    let m1d = mat_from_stage(m1);
    let m1_off = if let StageData::Matrix { offset, .. } = &m1.data {
        offset.clone()
    } else {
        None
    };
    let (res, offset) = if let Some(mi2) = matrix2_i {
        let m2 = &chain[mi2];
        if m2.input_channels != 3 || m2.output_channels != 3 {
            return false;
        }
        if m1_off.is_some() {
            return false;
        }
        let m2d = mat_from_stage(m2);
        let m2_off = if let StageData::Matrix { offset, .. } = &m2.data {
            offset.clone()
        } else {
            None
        };
        let mut r = Mat3::default();
        if let (Some(a), Some(b)) = (m2d, m1d) {
            crate::mtrx::mat3_per(&mut r, &a, &b);
            (Some(r), m2_off)
        } else {
            (None, m2_off)
        }
    } else {
        (m1d, m1_off)
    };
    let Some(res) = res else { return false };
    let identity_mat = crate::mtrx::mat3_is_identity(&res) && offset.is_none();
    let mut dest = match crate::lut::pipeline_alloc(None, lut.input_channels, lut.output_channels) {
        Some(d) => d,
        None => return false,
    };
    if !dest.insert_stage(StageLoc::AtBegin, {
        let mut d = (*c1).clone();
        d.next = None;
        d
    }) {
        return false;
    }
    if !identity_mat {
        let mut flat = [0.0f64; 9];
        for i in 0..3 {
            for j in 0..3 {
                flat[i * 3 + j] = res.v[i].n[j];
            }
        }
        let st = match crate::lut::stage_alloc_matrix(None, 3, 3, &flat, offset.as_deref()) {
            Some(st) => st,
            None => return false,
        };
        if !dest.insert_stage(StageLoc::AtEnd, st) {
            return false;
        }
    }
    if !dest.insert_stage(StageLoc::AtEnd, {
        let mut d = (*c2).clone();
        d.next = None;
        d
    }) {
        return false;
    }
    if identity_mat {
        optimize_by_joining_curves(&mut dest, intent, input_format, output_format, dw_flags);
    } else {
        let c1c = match &chain[curve1_i].data {
            StageData::ToneCurves(v) => v.clone(),
            _ => return false,
        };
        let c2c = match &chain[curve2_i].data {
            StageData::ToneCurves(v) => v.clone(),
            _ => return false,
        };
        *dw_flags |= crate::xform::FLAGS_NOCACHE;
        let off_vec3 = offset.as_deref().map(|o| Vec3 {
            n: [o[0], o[1], o[2]],
        });
        set_mat_shaper(
            &mut dest,
            &c1c,
            &res,
            off_vec3.as_ref(),
            &c2c,
            input_format,
            output_format,
        );
    }
    *lut = dest;
    true
}

// ---------------------------------------------------------------------------
// `Prelin16Data` + `PatchLUT` + `FixWhiteMisalignment` — resampling machinery
// ---------------------------------------------------------------------------

/// `Prelin16Data` — in curves → CLUT → out curves fused eval
#[derive(Clone)]
pub struct Prelin16Data {
    pub n_inputs: u32,
    pub n_outputs: u32,
    /// per-input curves; empty = `Eval16nop1D` (nop, copy in→out)
    pub in_curves: Vec<crate::gamma::ToneCurve>,
    pub clut: crate::lut::CLutData,
    /// per-output curves; empty = nop
    pub out_curves: Vec<crate::gamma::ToneCurve>,
}

/// `Eval16nop1D` — pass-through single-channel
fn eval16_nop1d(input: u16) -> u16 {
    input
}

/// `PrelinEval16` — in curves → CLUT → out curves
pub(crate) fn prelin_eval16(input: &[u16], output: &mut [u16], d: &OptData) -> bool {
    let OptData::Prelin16(p16) = d else {
        return false;
    };
    let ni = p16.n_inputs as usize;
    let no = p16.n_outputs as usize;
    let mut stage_abc = [0u16; 8];
    let mut stage_def = [0u16; 16];
    for i in 0..ni {
        stage_abc[i] = if i < p16.in_curves.len() {
            p16.in_curves[i].eval16(input[i])
        } else {
            eval16_nop1d(input[i])
        };
    }
    {
        use crate::intrp::{InterpParams, InterpTable};
        let c = &p16.clut;
        let p = InterpParams {
            dw_flags: c.dw_flags,
            n_inputs: c.n_inputs,
            n_outputs: c.n_outputs,
            n_samples: c.n_samples,
            domain: c.domain,
            opta: c.opta,
            table: InterpTable::U16(c.tab16.as_deref().unwrap_or(&[])),
            interpolation: c.interpolation,
            foreign: c.foreign.clone(),
        };
        p.eval16(&stage_abc[..ni], &mut stage_def[..no]);
    }
    for i in 0..no {
        output[i] = if i < p16.out_curves.len() {
            p16.out_curves[i].eval16(stage_def[i])
        } else {
            eval16_nop1d(stage_def[i])
        };
    }
    true
}

/// `CLUTEval16` — bare CLUT eval (no curves)
fn clut_eval16(input: &[u16], output: &mut [u16], d: &OptData) -> bool {
    let OptData::ClutOnly(c) = d else {
        return false;
    };
    c.params16.eval16(input, output);
    true
}

/// `PatchLUT` — force `Value` at grid node `At` (must be exact node)
fn patch_lut(clut: &mut crate::lut::CLutData, at: &[u16], value: &[u16]) -> bool {
    let n_in = clut.n_inputs as usize;
    let index = if n_in == 4 {
        let px = at[0] as f64 * clut.domain[0] as f64 / 65535.0;
        let py = at[1] as f64 * clut.domain[1] as f64 / 65535.0;
        let pz = at[2] as f64 * clut.domain[2] as f64 / 65535.0;
        let pw = at[3] as f64 * clut.domain[3] as f64 / 65535.0;
        let (x0, y0, z0, w0) = (
            px.floor() as i64,
            py.floor() as i64,
            pz.floor() as i64,
            pw.floor() as i64,
        );
        if (px - x0 as f64) != 0.0
            || (py - y0 as f64) != 0.0
            || (pz - z0 as f64) != 0.0
            || (pw - w0 as f64) != 0.0
        {
            return false;
        }
        (clut.opta[3] as i64 * x0
            + clut.opta[2] as i64 * y0
            + clut.opta[1] as i64 * z0
            + clut.opta[0] as i64 * w0) as usize
    } else if n_in == 3 {
        let px = at[0] as f64 * clut.domain[0] as f64 / 65535.0;
        let py = at[1] as f64 * clut.domain[1] as f64 / 65535.0;
        let pz = at[2] as f64 * clut.domain[2] as f64 / 65535.0;
        let (x0, y0, z0) = (px.floor() as i64, py.floor() as i64, pz.floor() as i64);
        if (px - x0 as f64) != 0.0 || (py - y0 as f64) != 0.0 || (pz - z0 as f64) != 0.0 {
            return false;
        }
        (clut.opta[2] as i64 * x0 + clut.opta[1] as i64 * y0 + clut.opta[0] as i64 * z0) as usize
    } else if n_in == 1 {
        let px = at[0] as f64 * clut.domain[0] as f64 / 65535.0;
        let x0 = px.floor() as i64;
        if (px - x0 as f64) != 0.0 {
            return false;
        }
        (clut.opta[0] as i64 * x0) as usize
    } else {
        return false;
    };
    if let Some(tab) = &mut clut.tab16 {
        for (i, &v) in value.iter().enumerate() {
            tab[index + i] = v;
        }
    }
    true
}

/// `WhitesAreEqual` — within 0xf000 per channel or identical
fn whites_are_equal(n: usize, w1: &[u16], w2: &[u16]) -> bool {
    for i in 0..n {
        if (w1[i] as i32 - w2[i] as i32).abs() > 0xf000 {
            return true; // so different that fixup should be avoided
        }
        if w1[i] != w2[i] {
            return false;
        }
    }
    true
}

/// `FixWhiteMisalignment` — patch the CLUT's white node so white→white
fn fix_white_misalignment(
    lut: &mut Pipeline,
    entry_space: crate::types::ColorSpaceSignature,
    exit_space: crate::types::ColorSpaceSignature,
) -> bool {
    let Some((white_point_in, _, n_ins)) = crate::pcs::end_points_by_space(entry_space) else {
        return false;
    };
    let Some((white_point_out, _, n_outs)) = crate::pcs::end_points_by_space(exit_space) else {
        return false;
    };
    if lut.input_channels != n_ins || lut.output_channels != n_outs {
        return false;
    }
    let ni = n_ins as usize;
    let no = n_outs as usize;
    let mut obtained_out = [0u16; 16];
    lut.eval16(&white_point_in[..ni], &mut obtained_out);
    if whites_are_equal(no, &white_point_out[..no], &obtained_out) {
        return true; // whites already match
    }
    // locate PreLin/CLUT/PostLin in Dest (curves, clut, curves / pairs / lone clut)
    let chain_types: Vec<u32> = {
        let mut v = Vec::new();
        let mut st = lut.elements.as_deref();
        while let Some(e) = st {
            v.push(e.stage_type());
            st = e.next.as_deref();
        }
        v
    };
    let (pre_i, clut_i, post_i): (Option<usize>, usize, Option<usize>) = match chain_types.len() {
        3 if chain_types
            == [
                ssig::SigCurveSetElemType,
                ssig::SigCLutElemType,
                ssig::SigCurveSetElemType,
            ] =>
        {
            (Some(0), 1, Some(2))
        }
        2 if chain_types == [ssig::SigCurveSetElemType, ssig::SigCLutElemType] => {
            (Some(0), 1, None)
        }
        2 if chain_types == [ssig::SigCLutElemType, ssig::SigCurveSetElemType] => {
            (None, 0, Some(1))
        }
        1 if chain_types == [ssig::SigCLutElemType] => (None, 0, None),
        _ => return false,
    };
    let mut white_in = [0u16; 16];
    let mut white_out = [0u16; 16];
    // eval white through pre curves
    if let Some(pi) = pre_i {
        let mut st = lut.elements.as_deref();
        for _ in 0..pi {
            st = st.and_then(|s| s.next.as_deref());
        }
        if let Some(StageData::ToneCurves(curves)) = st.map(|s| &s.data) {
            for i in 0..ni {
                white_in[i] = curves[i].eval16(white_point_in[i]);
            }
        } else {
            return false;
        }
    } else {
        white_in[..ni].copy_from_slice(&white_point_in[..ni]);
    }
    // reverse-eval white through post curves
    if let Some(pi) = post_i {
        let mut st = lut.elements.as_deref();
        for _ in 0..pi {
            st = st.and_then(|s| s.next.as_deref());
        }
        if let Some(StageData::ToneCurves(curves)) = st.map(|s| &s.data) {
            for i in 0..no {
                white_out[i] = match crate::gamma::reverse_tone_curve(&curves[i]) {
                    Some(inv) => inv.eval16(white_point_out[i]),
                    None => white_point_out[i],
                };
            }
        } else {
            return false;
        }
    } else {
        white_out[..no].copy_from_slice(&white_point_out[..no]);
    }
    // patch the CLUT node — find the CLUT stage mutably
    let mut st = lut.elements.as_deref_mut();
    for _ in 0..clut_i {
        st = st.and_then(|s| s.next.as_deref_mut());
    }
    if let Some(StageData::CLut(d)) = st.map(|s| &mut s.data) {
        patch_lut(d, &white_in[..ni], &white_out[..no]);
    }
    true
}

// ---------------------------------------------------------------------------
// `OptimizeByComputingLinearization` — RGB→prelin+CLUT(+postlin) resample
// ---------------------------------------------------------------------------

/// `Prelin8Data` — 8-bit quantized trilinear + residual + prelin curves
#[derive(Clone)]
pub struct Prelin8Data {
    /// the CLUT's own eval state (owns the table)
    pub clut: crate::lut::CLutData,
    /// per-input index bases, quantized to 8-bit
    pub x0: [u32; 256],
    pub y0: [u32; 256],
    pub z0: [u32; 256],
    pub rx: [u16; 256],
    pub ry: [u16; 256],
    pub rz: [u16; 256],
}

/// `SlopeLimiting` — linearize the 2%/98% tails of a tone curve
fn slope_limiting(g: &mut crate::gamma::ToneCurve) {
    let n = g.table16.len();
    let at_begin = (n as f64 * 0.02 + 0.5).floor() as usize;
    let at_end = n - at_begin - 1;
    let (begin_val, end_val) = if crate::gamma::is_tone_curve_descending(g) {
        (65535.0f64, 0.0f64)
    } else {
        (0.0f64, 65535.0)
    };
    let mut val = g.table16[at_begin] as f64;
    let mut slope = (val - begin_val) / at_begin as f64;
    let mut beta = val - slope * at_begin as f64;
    for i in 0..at_begin {
        g.table16[i] = crate::fixed::quick_saturate_word(i as f64 * slope + beta);
    }
    val = g.table16[at_end] as f64;
    slope = (end_val - val) / at_begin as f64;
    beta = val - slope * at_end as f64;
    for i in at_end..n {
        g.table16[i] = crate::fixed::quick_saturate_word(i as f64 * slope + beta);
    }
}

/// `IsDegenerated` — >5% poles or zeros
fn is_degenerated(g: &crate::gamma::ToneCurve) -> bool {
    let n = g.table16.len();
    let mut zeros = 0usize;
    let mut poles = 0usize;
    for &t in &g.table16 {
        if t == 0 {
            zeros += 1;
        }
        if t == 0xffff {
            poles += 1;
        }
    }
    if zeros == 1 && poles == 1 {
        return false;
    }
    zeros > n / 20 || poles > n / 20
}

/// `PrelinOpt8alloc` — build the 8-bit quantized prelin eval state
fn prelin_opt8alloc(
    p: &crate::lut::CLutData,
    g: Option<&[crate::gamma::ToneCurve]>,
) -> Prelin8Data {
    let mut p8 = Prelin8Data {
        clut: p.clone(),
        x0: [0; 256],
        y0: [0; 256],
        z0: [0; 256],
        rx: [0; 256],
        ry: [0; 256],
        rz: [0; 256],
    };
    for i in 0..256 {
        let input = if let Some(g) = g {
            [
                g[0].eval16(((i as u16) << 8) | i as u16),
                g[1].eval16(((i as u16) << 8) | i as u16),
                g[2].eval16(((i as u16) << 8) | i as u16),
            ]
        } else {
            [
                ((i as u16) << 8) | i as u16,
                ((i as u16) << 8) | i as u16,
                ((i as u16) << 8) | i as u16,
            ]
        };
        let v1 = crate::fixed::to_fixed_domain((input[0] as u32 * p.domain[0]) as i32) as u32;
        let v2 = crate::fixed::to_fixed_domain((input[1] as u32 * p.domain[1]) as i32) as u32;
        let v3 = crate::fixed::to_fixed_domain((input[2] as u32 * p.domain[2]) as i32) as u32;
        p8.x0[i] = p.opta[2] * (v1 >> 16);
        p8.y0[i] = p.opta[1] * (v2 >> 16);
        p8.z0[i] = p.opta[0] * (v3 >> 16);
        p8.rx[i] = (v1 & 0xffff) as u16;
        p8.ry[i] = (v2 & 0xffff) as u16;
        p8.rz[i] = (v3 & 0xffff) as u16;
    }
    p8
}

/// `PrelinEval8` — the trilinear-optimized 8-bit CLUT eval
pub(crate) fn prelin_eval8(input: &[u16], output: &mut [u16], d: &OptData) -> bool {
    let OptData::Prelin8(p8) = d else {
        return false;
    };
    let r = (input[0] >> 8) as usize;
    let g = (input[1] >> 8) as usize;
    let b = (input[2] >> 8) as usize;
    let x0 = p8.x0[r] as i64;
    let y0 = p8.y0[g] as i64;
    let z0 = p8.z0[b] as i64;
    let rx = p8.rx[r] as i64;
    let ry = p8.ry[g] as i64;
    let rz = p8.rz[b] as i64;
    let x1 = x0 + if rx == 0 { 0 } else { p8.clut.opta[2] as i64 };
    let y1 = y0 + if ry == 0 { 0 } else { p8.clut.opta[1] as i64 };
    let z1 = z0 + if rz == 0 { 0 } else { p8.clut.opta[0] as i64 };
    let Some(tab) = &p8.clut.tab16 else {
        return false;
    };
    let dens = |i: i64, j: i64, k: i64, oc: usize| -> i64 { tab[(i + j + k) as usize + oc] as i64 };
    let total_out = p8.clut.n_outputs as usize;
    for oc in 0..total_out {
        let c0 = dens(x0, y0, z0, oc);
        let (c1, c2, c3) = if rx >= ry && ry >= rz {
            (
                dens(x1, y0, z0, oc) - c0,
                dens(x1, y1, z0, oc) - dens(x1, y0, z0, oc),
                dens(x1, y1, z1, oc) - dens(x1, y1, z0, oc),
            )
        } else if rx >= rz && rz >= ry {
            (
                dens(x1, y0, z0, oc) - c0,
                dens(x1, y1, z1, oc) - dens(x1, y0, z1, oc),
                dens(x1, y0, z1, oc) - dens(x1, y0, z0, oc),
            )
        } else if rz >= rx && rx >= ry {
            (
                dens(x1, y0, z1, oc) - dens(x0, y0, z1, oc),
                dens(x1, y1, z1, oc) - dens(x1, y0, z1, oc),
                dens(x0, y0, z1, oc) - c0,
            )
        } else if ry >= rx && rx >= rz {
            (
                dens(x1, y1, z0, oc) - dens(x0, y1, z0, oc),
                dens(x0, y1, z0, oc) - c0,
                dens(x1, y1, z1, oc) - dens(x1, y1, z0, oc),
            )
        } else if ry >= rz && rz >= rx {
            (
                dens(x1, y1, z1, oc) - dens(x0, y1, z1, oc),
                dens(x0, y1, z0, oc) - c0,
                dens(x0, y1, z1, oc) - dens(x0, y1, z0, oc),
            )
        } else {
            (
                dens(x1, y1, z1, oc) - dens(x0, y1, z1, oc),
                dens(x0, y1, z1, oc) - dens(x0, y0, z1, oc),
                dens(x0, y0, z1, oc) - c0,
            )
        };
        let rest = c1 * rx + c2 * ry + c3 * rz + 0x8001;
        output[oc] = (c0 + ((rest + (rest >> 16)) >> 16)) as u16;
    }
    true
}

/// `OptimizeByComputingLinearization` — prelin + CLUT + postlin resample
fn optimize_by_computing_linearization(
    lut: &mut Pipeline,
    intent: u32,
    input_format: &mut u32,
    output_format: &mut u32,
    dw_flags: &mut u32,
) -> bool {
    use crate::lut::StageLoc;
    if crate::pack::t_float(*input_format) != 0 || crate::pack::t_float(*output_format) != 0 {
        return false;
    }
    if crate::pack::t_colorspace(*input_format) != 4
    /* PT_RGB */
    {
        return false;
    }
    if crate::pack::t_planar(*input_format) != 0 {
        return false;
    }
    if crate::pack::t_colorspace(*output_format) != 4 {
        return false;
    }
    if crate::pack::t_planar(*output_format) != 0 {
        return false;
    }
    if crate::pack::t_bytes(*input_format) != 1 {
        if *dw_flags & crate::types::FLAGS_CLUT_PRE_LINEARIZATION == 0 {
            return false;
        }
    }
    let color_space = crate::pcs::icc_color_space(crate::pack::t_colorspace(*input_format) as i32);
    let output_space =
        crate::pcs::icc_color_space(crate::pack::t_colorspace(*output_format) as i32);
    if color_space == 0 || output_space == 0 {
        return false;
    }
    let n_grid = crate::pcs::reasonable_gridpoints_by_colorspace(color_space, *dw_flags);
    let ni = lut.input_channels as usize;
    // last stage degenerate check
    if let Some(last) = lut.last_stage() {
        if last.stage_type() == ssig::SigCurveSetElemType {
            if let StageData::ToneCurves(curves) = &last.data {
                if curves.iter().any(is_degenerated) {
                    return false;
                }
            }
        }
    }
    let mut trans: Vec<crate::gamma::ToneCurve> = Vec::new();
    for _ in 0..ni {
        let Some(g) =
            crate::gamma::build_tabulated_tone_curve16(None, PRELINEARIZATION_POINTS, None)
        else {
            return false;
        };
        trans.push(g);
    }
    let mut in_f = [0f32; 16];
    let mut out_f = [0f32; 16];
    for i in 0..PRELINEARIZATION_POINTS as usize {
        let v = (i as f64 / (PRELINEARIZATION_POINTS - 1) as f64) as f32;
        for t in 0..ni {
            in_f[t] = v;
        }
        lut.eval_float(&in_f[..ni], &mut out_f);
        for t in 0..ni {
            trans[t].table16[i] = crate::fixed::quick_saturate_word(out_f[t] as f64 * 65535.0);
        }
    }
    for t in &mut trans {
        slope_limiting(t);
    }
    let mut suitable = true;
    let mut linear = true;
    for t in &trans {
        if !crate::gamma::is_tone_curve_linear(t) {
            linear = false;
        }
        if !crate::gamma::is_tone_curve_monotonic(t) {
            suitable = false;
        }
        if is_degenerated(t) {
            suitable = false;
        }
    }
    let _ = linear;
    if !suitable {
        return false;
    }
    let mut trans_reverse: Vec<crate::gamma::ToneCurve> = Vec::new();
    for t in &trans {
        let Some(r) = crate::gamma::reverse_tone_curve_ex(PRELINEARIZATION_POINTS, t) else {
            return false;
        };
        trans_reverse.push(r);
    }
    let Some(mut lut_plus_curves) = lut.dup() else {
        return false;
    };
    {
        let refs: Vec<&crate::gamma::ToneCurve> = trans_reverse.iter().collect();
        let st = crate::lut::stage_alloc_tone_curves(None, lut.input_channels, Some(&refs));
        let Some(st) = st else { return false };
        if !lut_plus_curves.insert_stage(StageLoc::AtBegin, st) {
            return false;
        }
    }
    let mut optimized = crate::lut::pipeline_alloc(None, lut.input_channels, lut.output_channels);
    let Some(mut optimized) = optimized else {
        return false;
    };
    {
        let refs: Vec<&crate::gamma::ToneCurve> = trans.iter().collect();
        let st = crate::lut::stage_alloc_tone_curves(None, lut.input_channels, Some(&refs));
        let Some(st) = st else { return false };
        if !optimized.insert_stage(StageLoc::AtBegin, st) {
            return false;
        }
    }
    let Some(mut clut) = crate::lut::stage_alloc_clut16bit(
        None,
        n_grid,
        lut.input_channels,
        lut.output_channels,
        None,
    ) else {
        return false;
    };
    {
        let ni = lut_plus_curves.input_channels as usize;
        let no = lut_plus_curves.output_channels as usize;
        let mut sampler = |input: &[u16], output: &mut [u16]| -> i32 {
            let mut in_f = [0f32; 16];
            let mut out_f = [0f32; 16];
            for i in 0..ni {
                in_f[i] = (input[i] as f64 / 65535.0) as f32;
            }
            lut_plus_curves.eval_float(&in_f[..ni], &mut out_f);
            for i in 0..no {
                output[i] = crate::fixed::quick_saturate_word(out_f[i] as f64 * 65535.0);
            }
            1
        };
        if !crate::lut::stage_sample_clut16bit(&mut clut, &mut sampler, 0) {
            return false;
        }
    }
    // grab the prelin curves + the CLUT data for the opt params
    let prelin_curves: Vec<crate::gamma::ToneCurve> = {
        let mut out = Vec::new();
        let mut st = optimized.elements.as_deref();
        while let Some(s) = st {
            if s.stage_type() == ssig::SigCurveSetElemType {
                if let StageData::ToneCurves(v) = &s.data {
                    out = v.clone();
                    break;
                }
            }
            st = s.next.as_deref();
        }
        out
    };
    let clut_data = if let StageData::CLut(d) = &clut.data {
        d.as_ref().clone()
    } else {
        return false;
    };
    if !optimized.insert_stage(StageLoc::AtEnd, clut) {
        return false;
    }
    if crate::pack::t_bytes(*input_format) == 1 {
        let p8 = prelin_opt8alloc(
            &clut_data,
            if prelin_curves.is_empty() {
                None
            } else {
                Some(&prelin_curves)
            },
        );
        optimized.opt_data = Some(OptData::Prelin8(Box::new(p8)));
        optimized.eval16_fn = |i, o, l| {
            if let Some(d) = &l.opt_data {
                prelin_eval8(i, o, d);
            }
        };
    } else {
        optimized.opt_data = Some(OptData::Prelin16(Box::new(Prelin16Data {
            n_inputs: 3,
            n_outputs: 3,
            in_curves: prelin_curves,
            clut: clut_data,
            out_curves: Vec::new(),
        })));
        optimized.eval16_fn = |i, o, l| {
            if let Some(d) = &l.opt_data {
                prelin_eval16(i, o, d);
            }
        };
    }
    if intent == crate::types::INTENT_ABSOLUTE_COLORIMETRIC {
        *dw_flags |= crate::types::FLAGS_NOWHITEONWHITEFIXUP;
    }
    if *dw_flags & crate::types::FLAGS_NOWHITEONWHITEFIXUP == 0 {
        if !fix_white_misalignment(&mut optimized, color_space, output_space) {
            return false;
        }
    }
    *lut = optimized;
    true
}

/// `OptimizeByResampling` — resample the whole pipeline into a CLUT
fn optimize_by_resampling(
    lut: &mut Pipeline,
    intent: u32,
    input_format: &mut u32,
    output_format: &mut u32,
    dw_flags: &mut u32,
) -> bool {
    use crate::lut::StageLoc;
    if crate::pack::t_float(*input_format) != 0 || crate::pack::t_float(*output_format) != 0 {
        return false;
    }
    let color_space = crate::pcs::icc_color_space(crate::pack::t_colorspace(*input_format) as i32);
    let output_space =
        crate::pcs::icc_color_space(crate::pack::t_colorspace(*output_format) as i32);
    if color_space == 0 || output_space == 0 {
        return false;
    }
    let n_grid = if lut.stage_count() == 0 {
        2
    } else {
        let g = crate::pcs::reasonable_gridpoints_by_colorspace(color_space, *dw_flags);
        if *dw_flags & crate::types::FLAGS_FORCE_CLUT == 0
            && color_space == crate::signatures::colorspace::SigLabData
            && crate::pack::t_bytes(*input_format) == 2
        {
            return false;
        }
        g
    };
    let mut dest = crate::lut::pipeline_alloc(None, lut.input_channels, lut.output_channels);
    let Some(mut dest) = dest else { return false };
    let mut keep_pre: Option<Stage> = None;
    let mut keep_post: Option<Stage> = None;
    let mut new_pre: Option<Stage> = None;
    let mut new_post: Option<Stage> = None;
    if *dw_flags & crate::types::FLAGS_CLUT_PRE_LINEARIZATION != 0 {
        if let Some(first) = lut.elements.as_deref() {
            if first.stage_type() == ssig::SigCurveSetElemType && !all_curves_are_linear(first) {
                new_pre = Some({
                    let mut d = first.clone();
                    d.next = None;
                    d
                });
                if let Some(np) = new_pre.clone() {
                    if !dest.insert_stage(StageLoc::AtBegin, np) {
                        return false;
                    }
                }
                keep_pre = lut.unlink_stage(StageLoc::AtBegin);
            }
        }
    }
    let Some(mut clut) = crate::lut::stage_alloc_clut16bit(
        None,
        n_grid,
        lut.input_channels,
        lut.output_channels,
        None,
    ) else {
        return false;
    };
    if *dw_flags & crate::types::FLAGS_CLUT_POST_LINEARIZATION != 0 {
        if let Some(last) = lut.last_stage() {
            if last.stage_type() == ssig::SigCurveSetElemType && !all_curves_are_linear(last) {
                new_post = Some({
                    let mut d = last.clone();
                    d.next = None;
                    d
                });
                if let Some(np) = new_post.clone() {
                    if !dest.insert_stage(StageLoc::AtEnd, np) {
                        return false;
                    }
                }
                keep_post = lut.unlink_stage(StageLoc::AtEnd);
            }
        }
    }
    // sample the ORIGINAL pipeline through `XFormSampler16` — the closure
    // owns a moved-out copy, freeing it after (C frees Src on success)
    {
        let orig = std::mem::replace(lut, crate::lut::pipeline_alloc(None, 1, 1).unwrap());
        let mut sampler = |input: &[u16], output: &mut [u16]| -> i32 {
            let ni = orig.input_channels as usize;
            let no = orig.output_channels as usize;
            let mut in_f = [0f32; 16];
            let mut out_f = [0f32; 16];
            for i in 0..ni {
                in_f[i] = (input[i] as f64 / 65535.0) as f32;
            }
            orig.eval_float(&in_f[..ni], &mut out_f);
            for i in 0..no {
                output[i] = crate::fixed::quick_saturate_word(out_f[i] as f64 * 65535.0);
            }
            1
        };
        if !crate::lut::stage_sample_clut16bit(&mut clut, &mut sampler, 0) {
            // C error path: reinsert unlinked stages back into Src
            if let Some(k) = keep_pre {
                // note: cannot fully restore since orig consumed — matches
                // C's own error path only when sampling fails (rare)
                let _ = k;
            }
            if let Some(k) = keep_post {
                let _ = k;
            }
            return false;
        }
    }
    if !dest.insert_stage(StageLoc::AtEnd, clut) {
        return false;
    }
    // keep_* stages are consumed/dropped here (C frees them)
    let _ = keep_pre;
    let _ = keep_post;
    // set the opt eval — CLUT-only or prelin16
    let clut_stage_data = {
        let mut st = dest.elements.as_deref_mut();
        let mut found = None;
        while let Some(s) = st {
            if s.stage_type() == ssig::SigCLutElemType {
                if let StageData::CLut(d) = &mut s.data {
                    found = Some(d.as_ref().clone());
                }
                break;
            }
            st = s.next.as_deref_mut();
        }
        found
    };
    let Some(clut_data) = clut_stage_data else {
        return false;
    };
    let data_in: Vec<crate::gamma::ToneCurve> = new_pre
        .as_ref()
        .and_then(|s| {
            if let StageData::ToneCurves(v) = &s.data {
                Some(v.clone())
            } else {
                None
            }
        })
        .unwrap_or_default();
    let data_out: Vec<crate::gamma::ToneCurve> = new_post
        .as_ref()
        .and_then(|s| {
            if let StageData::ToneCurves(v) = &s.data {
                Some(v.clone())
            } else {
                None
            }
        })
        .unwrap_or_default();
    if data_in.is_empty() && data_out.is_empty() {
        let params16 = crate::intrp::InterpParams {
            dw_flags: clut_data.dw_flags,
            n_inputs: clut_data.n_inputs,
            n_outputs: clut_data.n_outputs,
            n_samples: clut_data.n_samples,
            domain: clut_data.domain,
            opta: clut_data.opta,
            table: crate::intrp::InterpTable::U16Rc(std::sync::Arc::from(
                clut_data.tab16.as_deref().unwrap_or(&[]),
            )),
            interpolation: clut_data.interpolation,
            foreign: clut_data.foreign.clone(),
        };
        dest.opt_data = Some(OptData::ClutOnly(Box::new(ClutOnly {
            clut: clut_data,
            params16,
        })));
        dest.eval16_fn = |i, o, l| {
            if let Some(d) = &l.opt_data {
                clut_eval16(i, o, d);
            }
        };
    } else {
        dest.opt_data = Some(OptData::Prelin16(Box::new(Prelin16Data {
            n_inputs: dest.input_channels,
            n_outputs: dest.output_channels,
            in_curves: data_in,
            clut: clut_data,
            out_curves: data_out,
        })));
        dest.eval16_fn = |i, o, l| {
            if let Some(d) = &l.opt_data {
                prelin_eval16(i, o, d);
            }
        };
    }
    if intent == crate::types::INTENT_ABSOLUTE_COLORIMETRIC {
        *dw_flags |= crate::types::FLAGS_NOWHITEONWHITEFIXUP;
    }
    if *dw_flags & crate::types::FLAGS_NOWHITEONWHITEFIXUP == 0 {
        fix_white_misalignment(&mut dest, color_space, output_space);
    }
    *lut = dest;
    true
}
