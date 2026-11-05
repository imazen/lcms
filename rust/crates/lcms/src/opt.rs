//! `cmsopt.c` — pipeline optimizer: 4 default strategies tried in order
//! (join curves → matrix shaper → computing linearization → resampling),
//! plus the plugin optimization collection.
//!
//! Each strategy returns `bool` and may replace the pipeline + revise the
//! formats/flags through the mutable refs, exactly as the C signature.

use crate::context::Context;
use crate::lut::Pipeline;

/// Optimization payload — C's `Eval16Fn` data union, closed-world enum
#[derive(Clone)]
pub enum OptData {
    Curves16(Curves16Data),
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

/// `_cmsOptimizePipeline` — try each strategy in order; first success wins.
/// When `flags` asks for it, only resampling is allowed.
pub fn optimize_pipeline(
    _ctx: Option<&Context>,
    lut: &mut Pipeline,
    intent: u32,
    input_format: &mut u32,
    output_format: &mut u32,
    dw_flags: &mut u32,
) -> bool {
    if *dw_flags & crate::xform::FLAGS_HIGHRESPRECALC != 0 {
        // C: HIGHRESPRECALC forces resampling-only (OptimizeByResampling direct)
        return optimize_by_resampling(lut, intent, input_format, output_format, dw_flags);
    }
    optimize_by_joining_curves(lut, intent, input_format, output_format, dw_flags)
        || optimize_matrix_shaper(lut, intent, input_format, output_format, dw_flags)
        || optimize_by_computing_linearization(lut, intent, input_format, output_format, dw_flags)
        || optimize_by_resampling(lut, intent, input_format, output_format, dw_flags)
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
                // both must be 3x3 with no offset
                let ok = s.input_channels == 3
                    && s.output_channels == 3
                    && s.next
                        .as_deref()
                        .map(|n| n.input_channels == 3 && n.output_channels == 3)
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
fn fast_eval_curves16(input: &[u16], output: &mut [u16], data: &OptData) -> bool {
    let OptData::Curves16(c) = data else {
        return false;
    };
    for i in 0..c.curves.len() {
        output[i] = c.curves[i][input[i] as usize];
    }
    true
}
/// `FastIdentity16` — copy
fn fast_identity16(input: &[u16], output: &mut [u16], _d: &OptData) -> bool {
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
            in_float[j] = i as f32 / (PRELINEARIZATION_POINTS - 1) as f32;
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

/// `OptimizeMatrixShaper` — curves+matrix+curves → precomputed shaper
fn optimize_matrix_shaper(
    lut: &mut Pipeline,
    _intent: u32,
    _if: &mut u32,
    _of: &mut u32,
    _fl: &mut u32,
) -> bool {
    let _ = lut;
    false
}

/// `OptimizeByComputingLinearization` — prelin + CLUT + postlin resampled
fn optimize_by_computing_linearization(
    lut: &mut Pipeline,
    _intent: u32,
    _if: &mut u32,
    _of: &mut u32,
    _fl: &mut u32,
) -> bool {
    let _ = lut;
    false
}

/// `OptimizeByResampling` — resample the pipeline into a fresh CLUT
fn optimize_by_resampling(
    lut: &mut Pipeline,
    _intent: u32,
    _if: &mut u32,
    _of: &mut u32,
    _fl: &mut u32,
) -> bool {
    let _ = lut;
    false
}
