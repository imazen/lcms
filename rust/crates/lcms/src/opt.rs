//! `cmsopt.c` — pipeline optimizer: 4 default strategies tried in order
//! (join curves → matrix shaper → computing linearization → resampling),
//! plus the plugin optimization collection.
//!
//! Each strategy returns `bool` and may replace the pipeline + revise the
//! formats/flags through the mutable refs, exactly as the C signature.

use crate::context::Context;
use crate::lut::Pipeline;

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

/// `OptimizeByJoiningCurves` — merge adjacent CurveSet stages
fn optimize_by_joining_curves(
    lut: &mut Pipeline,
    _intent: u32,
    _if: &mut u32,
    _of: &mut u32,
    _fl: &mut u32,
) -> bool {
    let _ = lut;
    // TODO(cmsopt): CollectCurves → join into one curve set when the
    // whole pipeline is curves-only
    false
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
