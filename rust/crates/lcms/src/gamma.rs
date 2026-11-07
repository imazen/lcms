//! Port of `cmsgamma.c` — tone curves (segmented, parametric, tabulated).
//!
//! C source: `reference/lcms2/src/cmsgamma.c`
//! Gate: `tests/c_parity_gamma.rs`
//!
//! Differences in representation, not semantics:
//! - `cmsToneCurve` owns its `Table16`, segments and per-segment evaluators as
//!   Rust collections; `cmsFreeToneCurve` is `Drop`.
//! - C keeps an `cmsInterpParams` pointing at `Table16` and per-segment
//!   `SegInterp` whose `Table` field it mutates at eval time (through a const
//!   pointer). Here the resolved kernels are stored and a params struct is
//!   assembled on the stack per call — identical arithmetic.
//! - The parametric-curves plugin list lives in `Context` as a Vec; index 0 is
//!   the list head (C prepends on registration).

use crate::context::{Context, ParametricCurveEvaluator, or_global};
use crate::err;
use crate::fixed::{quantize_val, quick_saturate_word};
use crate::intrp::{
    InterpFn16, InterpFnFloat, InterpParams, InterpTable, Interpolation, LERP_FLAGS_16BITS,
    LERP_FLAGS_FLOAT, compute_interp_params,
};
use crate::types::CpowExt;
use crate::types::{ERROR_RANGE, ERROR_UNKNOWN_EXTENSION};

/// `MAX_NODES_IN_CURVE`
pub const MAX_NODES_IN_CURVE: usize = 4097;
/// `MINUS_INF`
pub const MINUS_INF: f32 = -1.0e22;
/// `PLUS_INF`
pub const PLUS_INF: f32 = 1.0e22;
/// `MAX_TYPES_IN_LCMS_PLUGIN`
pub const MAX_TYPES_IN_LCMS_PLUGIN: usize = 20;
/// `MATRIX_DET_TOLERANCE` (lcms2_internal.h)
pub const MATRIX_DET_TOLERANCE: f64 = 0.0001;

/// `cmsCurveSegment` — `SampledPoints` is an owned copy (C duplicates it).
#[derive(Clone)]
pub struct CurveSegment {
    pub x0: f32,
    pub x1: f32,
    /// Parametric type; 0 means sampled segment; negative = analytic inverse.
    pub r#type: i32,
    pub params: [f64; 10],
    pub n_grid_points: u32,
    pub sampled_points: Option<Vec<f32>>,
}

impl Default for CurveSegment {
    fn default() -> Self {
        CurveSegment {
            x0: 0.0,
            x1: 0.0,
            r#type: 0,
            params: [0.0; 10],
            n_grid_points: 0,
            sampled_points: None,
        }
    }
}

/// `_cmsParametricCurvesCollection` — one registered evaluator's type set.
#[derive(Clone)]
pub struct ParametricCurves {
    pub function_types: Vec<i32>,
    pub parameter_count: Vec<u32>,
    pub evaluator: ParametricCurveEvaluator,
}

/// `cmsToneCurve`
pub struct ToneCurve {
    pub n_entries: u32,
    pub table16: Vec<u16>,
    pub segments: Vec<CurveSegment>,
    /// `Evals` — resolved evaluator per segment (`None` where C leaves the
    /// calloc'ed pointer NULL because no collection knew the type).
    pub evals: Vec<Option<ParametricCurveEvaluator>>,
    /// C-registered `cmsPluginParametricCurves` evaluator ptrs, parallel
    /// to `evals` — `Some(ptr)` when the segment's type matched a C set.
    pub(crate) c_evals: Vec<Option<usize>>,
    /// `SegInterp` — the resolved 1in/1out float kernel per type-0 segment.
    /// `None` mirrors C leaving the slot NULL for non-sampled segments.
    seg_interp: Vec<Option<InterpFnFloat>>,
    /// `InterpParams` kernel over `table16` (C stores full params; we store
    /// the resolved kernel — the params are fully derivable per call).
    interp16: InterpFn16,
    /// `InterpParams.Domain[0]` — kept for `GetInterval` which reads it.
    domain0: u32,
}

impl ToneCurve {
    /// `cmsGetToneCurveEstimatedTableEntries`
    pub fn estimated_table_entries(&self) -> u32 {
        self.n_entries
    }
    /// `cmsGetToneCurveEstimatedTable`
    pub fn estimated_table(&self) -> &[u16] {
        &self.table16
    }
    /// `cmsGetToneCurveSegment`
    pub fn segment(&self, n: i32) -> Option<&CurveSegment> {
        if n < 0 || n >= self.segments.len() as i32 {
            return None;
        }
        Some(&self.segments[n as usize])
    }
    /// `cmsGetToneCurveParametricType`
    pub fn parametric_type(&self) -> i32 {
        if self.segments.len() != 1 {
            return 0;
        }
        self.segments[0].r#type
    }

    /// `cmsEvalToneCurve16`
    pub fn eval16(&self, v: u16) -> u16 {
        // Fast path — `LinLerp1D` over `table16`/`domain0` without building
        // a full `InterpParams` (C caches the params inside cmsToneCurve).
        if self.interp16 == crate::intrp::lin_lerp_1d as InterpFn16 {
            let d = self.domain0;
            if v == 0xffff || d == 0 {
                return crate::intrp::tbl_get16(&self.table16, d as usize) as u16;
            }
            let val3 = crate::fixed::to_fixed_domain(d.wrapping_mul(v as u32) as i32);
            let cell0 = crate::fixed::fixed_to_int(val3) as usize;
            let rest = crate::fixed::fixed_rest_to_int(val3);
            let y0 = crate::intrp::tbl_get16(&self.table16, cell0);
            let y1 = crate::intrp::tbl_get16(&self.table16, cell0 + 1);
            return crate::intrp::linear_interp(rest, y0, y1);
        }
        let p = InterpParams {
            dw_flags: LERP_FLAGS_16BITS,
            n_inputs: 1,
            n_outputs: 1,
            n_samples: [self.domain0 + 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            domain: [self.domain0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            opta: [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            table: InterpTable::U16(&self.table16),
            interpolation: Interpolation::Lerp16(self.interp16),
            foreign: None,
        };
        let mut out = [0u16; 1];
        p.eval16(&[v], &mut out);
        out[0]
    }

    /// `cmsEvalToneCurveFloat` — uses the 16-bit table when there are no
    /// segments (limited-precision curve), else the segmented function.
    pub fn eval_float(&self, v: f32) -> f32 {
        if self.segments.is_empty() {
            let i = quick_saturate_word(v as f64 * 65535.0);
            let out = self.eval16(i);
            return (out as f64 / 65535.0) as f32;
        }
        eval_segmented_fn(self, v as f64) as f32
    }
}

/// `EvalSegmentedFn` — `-Inf` (`MINUS_INF`) when no segment covers `R`.
fn eval_segmented_fn(g: &ToneCurve, r: f64) -> f64 {
    let mut i = g.segments.len() as i64 - 1;
    while i >= 0 {
        let seg = &g.segments[i as usize];

        // Check for domain
        if r > seg.x0 as f64 && r <= seg.x1 as f64 {
            let out = if seg.r#type == 0 {
                // Type == 0 means segment is sampled
                let r1 = ((r - seg.x0 as f64) as f32) / (seg.x1 - seg.x0);
                let pts = seg.sampled_points.as_deref().unwrap_or(&[]);
                if let Some(kernel) = g.seg_interp[i as usize] {
                    let p = InterpParams {
                        dw_flags: LERP_FLAGS_FLOAT,
                        n_inputs: 1,
                        n_outputs: 1,
                        n_samples: [seg.n_grid_points, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
                        domain: [
                            seg.n_grid_points - 1,
                            0,
                            0,
                            0,
                            0,
                            0,
                            0,
                            0,
                            0,
                            0,
                            0,
                            0,
                            0,
                            0,
                            0,
                        ],
                        opta: [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
                        table: InterpTable::F32(pts),
                        interpolation: Interpolation::LerpFloat(kernel),
                        foreign: None,
                    };
                    let mut out32 = [0.0f32; 1];
                    kernel(&[r1], &mut out32, &p);
                    out32[0] as f64
                } else {
                    0.0
                }
            } else if let Some(eval) = g.evals[i as usize] {
                eval(seg.r#type, &seg.params, r)
            } else if let Some(ep) = g.c_evals.get(i as usize).copied().flatten() {
                match PARAMETRIC_EVAL_HOOK.get() {
                    Some(h) => h(ep, seg.r#type, &seg.params, r),
                    None => 0.0,
                }
            } else {
                0.0
            };

            if out.is_infinite() {
                return if out > 0.0 {
                    PLUS_INF as f64
                } else {
                    MINUS_INF as f64
                };
            }
            return out;
        }
        i -= 1;
    }
    MINUS_INF as f64
}

// ---------------------------------------------------------------------------
// Parametric collections + the default evaluator
// ---------------------------------------------------------------------------

/// `IsInSet` — index of `|Type|` in the collection, or None.
fn is_in_set(r#type: i32, c: &ParametricCurves) -> Option<usize> {
    c.function_types.iter().position(|&t| t == r#type.abs())
}

/// `GetParametricCurveByType` — plugins first, then the built-in defaults.
fn get_parametric_curve_by_type<'a>(
    ctx: Option<&'a Context>,
    r#type: i32,
) -> Option<(usize, &'a ParametricCurves)> {
    let c = or_global(ctx);
    for entry in &c.parametric_curves {
        if let Some(pos) = is_in_set(r#type, entry) {
            return Some((pos, entry));
        }
    }
    // If none found, revert for defaults
    if let Some(pos) = is_in_set(r#type, default_curves()) {
        return Some((pos, default_curves()));
    }
    None
}

/// `DefaultCurves` — the built-in collection.
fn default_curves() -> &'static ParametricCurves {
    static D: std::sync::OnceLock<ParametricCurves> = std::sync::OnceLock::new();
    D.get_or_init(|| ParametricCurves {
        function_types: vec![1, 2, 3, 4, 5, 6, 7, 8, 108, 109],
        parameter_count: vec![1, 3, 4, 5, 7, 4, 5, 5, 1, 1],
        evaluator: default_eval_parametric_fn,
    })
}

/// `_cmsRegisterParametricCurvesPlugin` — push a collection (head = index 0).
/// `n_functions` is clamped to `MAX_TYPES_IN_LCMS_PLUGIN` like C.
/// Foreign parametric-curve evaluator hook — the lcms crate forbids
/// `unsafe`, so calling a `cmsPluginParametricCurves`-registered C
/// evaluator fn ptr goes through a bridge installed by lcms2-ffi.
/// Signature: `(evaluator_ptr, type, params, r) -> f64`.
pub type ForeignParametricEval = fn(usize, i32, &[f64; 10], f64) -> f64;
static PARAMETRIC_EVAL_HOOK: std::sync::OnceLock<ForeignParametricEval> =
    std::sync::OnceLock::new();
pub fn set_parametric_eval_hook(h: ForeignParametricEval) {
    let _ = PARAMETRIC_EVAL_HOOK.set(h);
}

/// Walk the ctx's C-registered `cmsPluginParametricCurves` collections —
/// the `abs(Type)` match C applies in `_cmsGetParametricCurveByType`.
pub(crate) fn parametric_curve_in_c_sets<'a>(
    ctx: Option<&'a Context>,
    r#type: i32,
) -> Option<(usize, &'a crate::context::CParametricSet)> {
    let c = or_global(ctx);
    for (i, set) in c.c_parametric_curves.iter().enumerate() {
        if set.function_types.iter().any(|&t| t == r#type.abs()) {
            return Some((i, set));
        }
    }
    None
}

pub fn register_parametric_curves(ctx: &mut Context, collection: Option<ParametricCurves>) -> bool {
    match collection {
        None => {
            ctx.parametric_curves.clear();
            true
        }
        Some(mut c) => {
            c.function_types.truncate(MAX_TYPES_IN_LCMS_PLUGIN);
            c.parameter_count.truncate(MAX_TYPES_IN_LCMS_PLUGIN);
            ctx.parametric_curves.insert(0, c);
            true
        }
    }
}

// ---------------------------------------------------------------------------
// Default evaluator (DefaultEvalParametricFn) + sigmoid helpers
// ---------------------------------------------------------------------------

/// `sigmoid_base`
#[inline]
fn sigmoid_base(k: f64, t: f64) -> f64 {
    (1.0 / (1.0 + (-k * t).exp())) - 0.5
}

/// `inverted_sigmoid_base`
// `#[inline(never)]`: inlining `-inner.cln()/k` into `inverse_sigmoid_factory`
// produced the wrong NaN sign (`-NaN` where glibc `log(neg)` gives `-NaN`,
// negated to `+NaN`). Keeping the call out-of-line matches C bit-for-bit;
// this path is cold (type -109 curves only).
#[inline(never)]
fn inverted_sigmoid_base(k: f64, t: f64) -> f64 {
    -((1.0 / (t + 0.5)) - 1.0).cln() / k
}

/// `sigmoid_factory`
#[inline]
fn sigmoid_factory(k: f64, t: f64) -> f64 {
    let correction = 0.5 / sigmoid_base(k, 1.0);
    correction * sigmoid_base(k, 2.0 * t - 1.0) + 0.5
}

/// `inverse_sigmoid_factory`
#[inline]
fn inverse_sigmoid_factory(k: f64, t: f64) -> f64 {
    let correction = 0.5 / sigmoid_base(k, 1.0);
    let isb = inverted_sigmoid_base(k, (t - 0.5) / correction);
    (isb + 1.0) / 2.0
}

/// `DefaultEvalParametricFn` — types 1..8, 108, 109 and their negatives.
pub fn default_eval_parametric_fn(r#type: i32, params: &[f64; 10], r: f64) -> f64 {
    let (mut val, e, mut disc);
    match r#type {
        // X = Y ^ Gamma
        1 => {
            if r < 0.0 {
                val = if (params[0] - 1.0).abs() < MATRIX_DET_TOLERANCE {
                    r
                } else {
                    0.0
                };
            } else {
                val = r.cpow(params[0]);
            }
        }
        // Type 1 Reversed: X = Y ^1/gamma
        -1 => {
            if r < 0.0 {
                val = if (params[0] - 1.0).abs() < MATRIX_DET_TOLERANCE {
                    r
                } else {
                    0.0
                };
            } else if params[0].abs() < MATRIX_DET_TOLERANCE {
                val = PLUS_INF as f64;
            } else {
                val = r.cpow(1.0 / params[0]);
            }
        }
        // CIE 122-1966: Y = (aX + b)^Gamma | X >= -b/a, else 0
        2 => {
            if params[1].abs() < MATRIX_DET_TOLERANCE {
                val = 0.0;
            } else {
                disc = -params[2] / params[1];
                if r >= disc {
                    e = params[1] * r + params[2];
                    val = if e > 0.0 { e.cpow(params[0]) } else { 0.0 };
                } else {
                    val = 0.0;
                }
            }
        }
        // Type 2 Reversed: X = (Y ^1/g - b) / a
        -2 => {
            if params[0].abs() < MATRIX_DET_TOLERANCE || params[1].abs() < MATRIX_DET_TOLERANCE {
                val = 0.0;
            } else {
                val = if r < 0.0 {
                    0.0
                } else {
                    (r.cpow(1.0 / params[0]) - params[2]) / params[1]
                };
                if val < 0.0 {
                    val = 0.0;
                }
            }
        }
        // IEC 61966-3: Y = (aX + b)^Gamma + c | X <= -b/a, else c
        3 => {
            if params[1].abs() < MATRIX_DET_TOLERANCE {
                val = 0.0;
            } else {
                disc = -params[2] / params[1];
                if disc < 0.0 {
                    disc = 0.0;
                }
                if r >= disc {
                    e = params[1] * r + params[2];
                    val = if e > 0.0 {
                        e.cpow(params[0]) + params[3]
                    } else {
                        0.0
                    };
                } else {
                    val = params[3];
                }
            }
        }
        // Type 3 reversed: X=((Y-c)^1/g - b)/a | (Y>=c); X=-b/a | (Y<c)
        -3 => {
            if params[0].abs() < MATRIX_DET_TOLERANCE || params[1].abs() < MATRIX_DET_TOLERANCE {
                val = 0.0;
            } else if r >= params[3] {
                e = r - params[3];
                val = if e > 0.0 {
                    (e.cpow(1.0 / params[0]) - params[2]) / params[1]
                } else {
                    0.0
                };
            } else {
                val = -params[2] / params[1];
            }
        }
        // IEC 61966-2.1 (sRGB): Y = (aX + b)^Gamma | X >= d; Y = cX | X < d
        4 => {
            if r >= params[4] {
                e = params[1] * r + params[2];
                val = if e > 0.0 { e.cpow(params[0]) } else { 0.0 };
            } else {
                val = r * params[3];
            }
        }
        // Type 4 reversed
        -4 => {
            e = params[1] * params[4] + params[2];
            disc = if e < 0.0 { 0.0 } else { e.cpow(params[0]) };
            if r >= disc {
                val = if params[0].abs() < MATRIX_DET_TOLERANCE
                    || params[1].abs() < MATRIX_DET_TOLERANCE
                {
                    0.0
                } else {
                    (r.cpow(1.0 / params[0]) - params[2]) / params[1]
                };
            } else if params[3].abs() < MATRIX_DET_TOLERANCE {
                val = 0.0;
            } else {
                val = r / params[3];
            }
        }
        // Y = (aX + b)^Gamma + e | X >= d; Y = cX + f | X < d
        5 => {
            if r >= params[4] {
                e = params[1] * r + params[2];
                val = if e > 0.0 {
                    e.cpow(params[0]) + params[5]
                } else {
                    params[5]
                };
            } else {
                val = r * params[3] + params[6];
            }
        }
        // Reversed type 5
        -5 => {
            disc = params[3] * params[4] + params[6];
            if r >= disc {
                e = r - params[5];
                if e < 0.0 {
                    val = 0.0;
                } else if params[0].abs() < MATRIX_DET_TOLERANCE
                    || params[1].abs() < MATRIX_DET_TOLERANCE
                {
                    val = 0.0;
                } else {
                    val = (e.cpow(1.0 / params[0]) - params[2]) / params[1];
                }
            } else if params[3].abs() < MATRIX_DET_TOLERANCE {
                val = 0.0;
            } else {
                val = (r - params[6]) / params[3];
            }
        }
        // Type 6: Y = (a * X + b) ^ Gamma + c (type 5 without d)
        6 => {
            e = params[1] * r + params[2];
            // On gamma 1.0, don't clamp
            if params[0] == 1.0 {
                val = e + params[3];
            } else if e < 0.0 {
                val = params[3];
            } else {
                val = e.cpow(params[0]) + params[3];
            }
        }
        // ((Y - c) ^1/Gamma - b) / a
        -6 => {
            if params[0].abs() < MATRIX_DET_TOLERANCE || params[1].abs() < MATRIX_DET_TOLERANCE {
                val = 0.0;
            } else {
                e = r - params[3];
                val = if e < 0.0 {
                    0.0
                } else {
                    (e.cpow(1.0 / params[0]) - params[2]) / params[1]
                };
            }
        }
        // Y = a * log10 (b * X^Gamma + c) + d
        7 => {
            e = params[2] * r.cpow(params[0]) + params[3];
            val = if e <= 0.0 {
                params[4]
            } else {
                params[1] * e.clog10() + params[4]
            };
        }
        // (Y - d) / a = log(b * X ^Gamma + c)
        -7 => {
            if params[0].abs() < MATRIX_DET_TOLERANCE
                || params[1].abs() < MATRIX_DET_TOLERANCE
                || params[2].abs() < MATRIX_DET_TOLERANCE
            {
                val = 0.0;
            } else {
                val = ((10.0_f64.cpow((r - params[4]) / params[1]) - params[3]) / params[2])
                    .cpow(1.0 / params[0]);
            }
        }
        // Y = a * b^(c*X+d) + e
        8 => {
            val = params[0] * params[1].cpow(params[2] * r + params[3]) + params[4];
        }
        // Y = (log((y-e) / a) / log(b) - d ) / c
        -8 => {
            disc = r - params[4];
            if disc < 0.0 {
                val = 0.0;
            } else if params[0].abs() < MATRIX_DET_TOLERANCE
                || params[2].abs() < MATRIX_DET_TOLERANCE
            {
                val = 0.0;
            } else {
                val = ((disc / params[0]).cln() / params[1].cln() - params[3]) / params[2];
            }
        }
        // S-Shaped: (1 - (1-x)^1/g)^1/g
        108 => {
            val = if params[0].abs() < MATRIX_DET_TOLERANCE {
                0.0
            } else {
                (1.0 - (1.0 - r).cpow(1.0 / params[0])).cpow(1.0 / params[0])
            };
        }
        -108 => {
            val = 1.0 - (1.0 - r.cpow(params[0])).cpow(params[0]);
        }
        // Sigmoidals
        109 => {
            val = sigmoid_factory(params[0], r);
        }
        -109 => {
            val = inverse_sigmoid_factory(params[0], r);
        }
        _ => return 0.0,
    }
    val
}

// ---------------------------------------------------------------------------
// Construction
// ---------------------------------------------------------------------------

/// `AllocateToneCurveStruct` — FFI-visible rebuild from raw parts (the FFI
/// layer publishes `cmsToneCurve` fields directly; mutations are synced back
/// through this constructor on each call).
#[doc(hidden)]
pub fn dbg_allocate_tone_curve_struct(
    ctx: Option<&Context>,
    n_entries: u32,
    n_segments: u32,
    segments: Option<&[CurveSegment]>,
    values: Option<&[u16]>,
) -> Option<ToneCurve> {
    allocate_tone_curve_struct(ctx, n_entries, n_segments, segments, values)
}
/// `AllocateToneCurveStruct`
fn allocate_tone_curve_struct(
    ctx: Option<&Context>,
    n_entries: u32,
    n_segments: u32,
    segments: Option<&[CurveSegment]>,
    values: Option<&[u16]>,
) -> Option<ToneCurve> {
    // We allow huge tables, which are then restricted for smoothing operations
    if n_entries > 65530 {
        err::signal_error(
            ctx,
            ERROR_RANGE,
            "Couldn't create tone curve of more than 65530 entries",
        );
        return None;
    }
    if n_entries == 0 && n_segments == 0 {
        err::signal_error(
            ctx,
            ERROR_RANGE,
            "Couldn't create tone curve with zero segments and no table",
        );
        return None;
    }

    let mut table16 = bytemuck::try_zeroed_vec::<u16>(n_entries as usize).ok()?;
    if let Some(v) = values {
        if n_entries > 0 {
            table16.copy_from_slice(v);
        }
    }

    let mut segs: Vec<CurveSegment> = Vec::new();
    let mut evals: Vec<Option<ParametricCurveEvaluator>> = Vec::new();
    let mut c_evals: Vec<Option<usize>> = Vec::new();
    let mut seg_interp: Vec<Option<InterpFnFloat>> = Vec::new();

    if let Some(segments) = segments {
        if n_segments > 0 {
            for seg in segments {
                // Type 0 is a special marker for table-based curves — resolve
                // the 1in/1out float kernel (plugin-aware like C).
                let kernel = if seg.r#type == 0 {
                    compute_interp_params(
                        ctx,
                        seg.n_grid_points,
                        1,
                        1,
                        InterpTable::F32(&[]),
                        LERP_FLAGS_FLOAT,
                    )
                    .map(|p| match p.interpolation {
                        Interpolation::LerpFloat(f) => f,
                        _ => unreachable!(),
                    })
                } else {
                    None
                };

                // memmove(&p->Segments[i], &Segments[i], ...) + SampledPoints dup
                let st = seg.r#type;
                segs.push(seg.clone());
                seg_interp.push(kernel);

                let evaluator = get_parametric_curve_by_type(ctx, st).map(|(_, c)| c.evaluator);
                evals.push(evaluator);
                // C-evaluator arm — only when no native collection matched.
                c_evals.push(if evaluator.is_none() {
                    parametric_curve_in_c_sets(ctx, st).map(|(_, s)| s.evaluator)
                } else {
                    None
                });
            }
        }
    }

    // p->InterpParams — the 1in/1out 16-bit kernel over table16
    let params = compute_interp_params(
        ctx,
        n_entries,
        1,
        1,
        InterpTable::U16(&[]),
        LERP_FLAGS_16BITS,
    )?;
    let (interp16, domain0) = match params.interpolation {
        Interpolation::Lerp16(f) => (f, params.domain[0]),
        _ => unreachable!(),
    };

    Some(ToneCurve {
        n_entries,
        table16,
        segments: segs,
        evals,
        c_evals,
        seg_interp,
        interp16,
        domain0,
    })
}

/// `cmsBuildTabulatedToneCurve16`
pub fn build_tabulated_tone_curve16(
    ctx: Option<&Context>,
    n_entries: u32,
    values: Option<&[u16]>,
) -> Option<ToneCurve> {
    allocate_tone_curve_struct(ctx, n_entries, 0, None, values)
}

/// `EntriesByGamma`
fn entries_by_gamma(gamma: f64) -> u32 {
    if (gamma - 1.0).abs() < 0.001 { 2 } else { 4096 }
}

/// `cmsBuildSegmentedToneCurve`
pub fn build_segmented_tone_curve(
    ctx: Option<&Context>,
    segments: &[CurveSegment],
) -> Option<ToneCurve> {
    let mut n_grid_points: u32 = 4096;

    // Optimization for identity curves.
    if segments.len() == 1 && segments[0].r#type == 1 {
        n_grid_points = entries_by_gamma(segments[0].params[0]);
    }

    let mut g = allocate_tone_curve_struct(
        ctx,
        n_grid_points,
        segments.len() as u32,
        Some(segments),
        None,
    )?;

    // Once we have the floating point version, we can approximate a 16 bit
    // table of 4096 entries for performance reasons.
    for i in 0..n_grid_points as usize {
        let r = i as f64 / (n_grid_points - 1) as f64;
        let val = eval_segmented_fn(&g, r);
        g.table16[i] = quick_saturate_word(val * 65535.0);
    }

    Some(g)
}

/// `cmsBuildTabulatedToneCurveFloat`
pub fn build_tabulated_tone_curve_float(
    ctx: Option<&Context>,
    values: &[f32],
) -> Option<ToneCurve> {
    let n_entries = values.len() as u32;
    if n_entries == 0 {
        return None;
    }

    // First and last segments are constant-function type 6
    let mut seg0 = CurveSegment {
        x0: MINUS_INF,
        x1: 0.0,
        r#type: 6,
        params: [
            1.0,
            0.0,
            0.0,
            values[0] as f64,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
        ],
        ..Default::default()
    };
    let seg1 = CurveSegment {
        x0: 0.0,
        x1: 1.0,
        r#type: 0,
        params: [0.0; 10],
        n_grid_points: n_entries,
        sampled_points: Some(values.to_vec()),
    };
    let mut seg2 = CurveSegment {
        x0: 1.0,
        x1: PLUS_INF,
        r#type: 6,
        params: [
            1.0,
            0.0,
            0.0,
            values[n_entries as usize - 1] as f64,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
        ],
        ..Default::default()
    };
    seg0.n_grid_points = 0;
    seg2.n_grid_points = 0;

    build_segmented_tone_curve(ctx, &[seg0, seg1, seg2])
}

/// `cmsBuildParametricToneCurve` — `Type` is ICC type +1; negative = inverted.
pub fn build_parametric_tone_curve(
    ctx: Option<&Context>,
    r#type: i32,
    params: &[f64],
) -> Option<ToneCurve> {
    let (pos, n_params) = match get_parametric_curve_by_type(ctx, r#type) {
        Some((pos, c)) => (pos, c.parameter_count[pos] as usize),
        // C-registered `cmsPluginParametricCurves` collection arm.
        None => match parametric_curve_in_c_sets(ctx, r#type) {
            Some((pos, s)) => (pos, s.parameter_count[pos] as usize),
            None => {
                err::signal_error(
                    ctx,
                    ERROR_UNKNOWN_EXTENSION,
                    "Invalid parametric curve type",
                );
                return None;
            }
        },
    };

    let mut seg0 = CurveSegment {
        x0: MINUS_INF,
        x1: PLUS_INF,
        r#type,
        params: [0.0; 10],
        ..Default::default()
    };

    seg0.params[..n_params].copy_from_slice(&params[..n_params]);

    build_segmented_tone_curve(ctx, &[seg0])
}

/// `cmsBuildGamma`
pub fn build_gamma(ctx: Option<&Context>, gamma: f64) -> Option<ToneCurve> {
    build_parametric_tone_curve(ctx, 1, &[gamma])
}

/// `cmsDupToneCurve`
pub fn dup_tone_curve(c: &ToneCurve, ctx: Option<&Context>) -> Option<ToneCurve> {
    allocate_tone_curve_struct(
        ctx,
        c.n_entries,
        c.segments.len() as u32,
        if c.segments.is_empty() {
            None
        } else {
            Some(&c.segments)
        },
        Some(&c.table16),
    )
}

/// `cmsJoinToneCurve` — y = Y^-1(X(t)); curves should be monotonic.
pub fn join_tone_curve(
    ctx: Option<&Context>,
    x: &ToneCurve,
    y: &ToneCurve,
    n_resulting_points: u32,
) -> Option<ToneCurve> {
    let y_reversed = reverse_tone_curve_ex(n_resulting_points, y)?;

    let mut res = vec![0.0f32; n_resulting_points as usize];
    for i in 0..n_resulting_points as usize {
        let t = i as f32 / (n_resulting_points - 1) as f32;
        let xv = x.eval_float(t);
        res[i] = y_reversed.eval_float(xv);
    }

    build_tabulated_tone_curve_float(ctx, &res)
}

// ---------------------------------------------------------------------------
// Reversal
// ---------------------------------------------------------------------------

/// `GetInterval` — surrounding nodes; tricky on non-monotonic tables.
fn get_interval(i_n: f64, lut_table: &[u16], domain0: u32) -> i32 {
    let mut y0: i32;
    let mut y1: i32;

    // A 1 point table is not allowed
    if domain0 < 1 {
        return -1;
    }

    // Let's see if ascending or descending.
    if lut_table[0] < lut_table[domain0 as usize] {
        // Table is overall ascending
        let mut i = domain0 as i32 - 1;
        while i >= 0 {
            y0 = lut_table[i as usize] as i32;
            y1 = lut_table[i as usize + 1] as i32;

            if y0 <= y1 {
                if i_n >= y0 as f64 && i_n <= y1 as f64 {
                    return i;
                }
            } else if y1 < y0 && i_n >= y1 as f64 && i_n <= y0 as f64 {
                return i;
            }
            i -= 1;
        }
    } else {
        // Table is overall descending
        for i in 0..domain0 as i32 {
            y0 = lut_table[i as usize] as i32;
            y1 = lut_table[i as usize + 1] as i32;

            if y0 <= y1 {
                if i_n >= y0 as f64 && i_n <= y1 as f64 {
                    return i;
                }
            } else if y1 < y0 && i_n >= y1 as f64 && i_n <= y0 as f64 {
                return i;
            }
        }
    }
    -1
}

/// `cmsReverseToneCurveEx`
pub fn reverse_tone_curve_ex(n_result_samples: u32, in_curve: &ToneCurve) -> Option<ToneCurve> {
    let ctx = None; // C carries the curve's context; plugin lists are global here
    let _ = ctx;

    // Try to reverse it analytically whatever possible
    if in_curve.segments.len() == 1
        && in_curve.segments[0].r#type > 0
        && (get_parametric_curve_by_type(ctx, in_curve.segments[0].r#type).is_some()
            || parametric_curve_in_c_sets(ctx, in_curve.segments[0].r#type).is_some())
    {
        return build_parametric_tone_curve(
            ctx,
            -in_curve.segments[0].r#type,
            &in_curve.segments[0].params,
        );
    }

    // Nope, reverse the table.
    let mut out = build_tabulated_tone_curve16(ctx, n_result_samples, None)?;

    // We want to know if this is an ascending or descending table
    let ascending = !is_tone_curve_descending(in_curve);

    let mut a = 0.0f64;
    let mut b = 0.0f64;

    // Iterate across Y axis
    for i in 0..n_result_samples as usize {
        let y = i as f64 * 65535.0 / (n_result_samples - 1) as f64;

        // Find interval in which y is within.
        let j = get_interval(y, &in_curve.table16, in_curve.domain0);
        if j >= 0 {
            let x1 = in_curve.table16[j as usize] as f64;
            let x2 = in_curve.table16[j as usize + 1] as f64;

            let y1 = (j as f64 * 65535.0) / (in_curve.n_entries - 1) as f64;
            let y2 = ((j + 1) as f64 * 65535.0) / (in_curve.n_entries - 1) as f64;

            // If collapsed, then use any
            if x1 == x2 {
                out.table16[i] = quick_saturate_word(if ascending { y2 } else { y1 });
                continue;
            } else {
                // Interpolate
                a = (y2 - y1) / (x2 - x1);
                b = y2 - a * x2;
            }
        }

        out.table16[i] = quick_saturate_word(a * y + b);
    }

    Some(out)
}

/// `cmsReverseToneCurve`
pub fn reverse_tone_curve(in_curve: &ToneCurve) -> Option<ToneCurve> {
    reverse_tone_curve_ex(4096, in_curve)
}

// ---------------------------------------------------------------------------
// Smoothing (Eilers' second-difference method)
// ---------------------------------------------------------------------------

/// `smooth2`
fn smooth2(w: &mut [f32], y: &mut [f32], z: &mut [f32], lambda: f32, m: usize) -> bool {
    let mut c = [0.0f32; MAX_NODES_IN_CURVE];
    let mut d = [0.0f32; MAX_NODES_IN_CURVE];
    let mut e = [0.0f32; MAX_NODES_IN_CURVE];

    if m < 4 || lambda < MATRIX_DET_TOLERANCE as f32 {
        return false;
    }

    d[1] = w[1] + lambda;
    c[1] = -2.0 * lambda / d[1];
    e[1] = lambda / d[1];
    z[1] = w[1] * y[1];
    d[2] = w[2] + 5.0 * lambda - d[1] * c[1] * c[1];
    c[2] = (-4.0 * lambda - d[1] * c[1] * e[1]) / d[2];
    e[2] = lambda / d[2];
    z[2] = w[2] * y[2] - c[1] * z[1];

    for i in 3..m - 1 {
        let i1 = i - 1;
        let i2 = i - 2;
        d[i] = w[i] + 6.0 * lambda - c[i1] * c[i1] * d[i1] - e[i2] * e[i2] * d[i2];
        c[i] = (-4.0 * lambda - d[i1] * c[i1] * e[i1]) / d[i];
        e[i] = lambda / d[i];
        z[i] = w[i] * y[i] - c[i1] * z[i1] - e[i2] * z[i2];
    }

    let i1 = m - 2;
    let i2 = m - 3;

    d[m - 1] = w[m - 1] + 5.0 * lambda - c[i1] * c[i1] * d[i1] - e[i2] * e[i2] * d[i2];
    c[m - 1] = (-2.0 * lambda - d[i1] * c[i1] * e[i1]) / d[m - 1];
    z[m - 1] = w[m - 1] * y[m - 1] - c[i1] * z[i1] - e[i2] * z[i2];
    let i1 = m - 1;
    let i2 = m - 2;

    d[m] = w[m] + lambda - c[i1] * c[i1] * d[i1] - e[i2] * e[i2] * d[i2];
    z[m] = (w[m] * y[m] - c[i1] * z[i1] - e[i2] * z[i2]) / d[m];
    z[m - 1] = z[m - 1] / d[m - 1] - c[m - 1] * z[m];

    for i in (1..=m - 2).rev() {
        z[i] = z[i] / d[i] - c[i] * z[i + 1] - e[i] * z[i + 2];
    }

    true
}

/// `cmsSmoothToneCurve` — smooths a curve sampled at regular intervals.
/// `&mut ToneCurve` mirrors the in-place C update.
pub fn smooth_tone_curve(ctx: Option<&Context>, tab: &mut ToneCurve, mut lambda: f64) -> bool {
    let mut success = true;
    let mut not_check = false;

    if !is_tone_curve_linear(tab) {
        let n_items = tab.n_entries as usize;
        if n_items < MAX_NODES_IN_CURVE {
            let mut w = vec![0.0f32; n_items + 1];
            let mut y = vec![0.0f32; n_items + 1];
            let mut z = vec![0.0f32; n_items + 1];

            for i in 0..n_items {
                y[i + 1] = tab.table16[i] as f32;
                w[i + 1] = 1.0;
            }

            if lambda < 0.0 {
                not_check = true;
                lambda = -lambda;
            }

            if smooth2(&mut w, &mut y, &mut z, lambda as f32, n_items) {
                // Do some reality - checking...
                let mut zeros = 0u32;
                let mut poles = 0u32;
                for i in (2..=n_items).rev() {
                    if z[i] == 0.0 {
                        zeros += 1;
                    }
                    if z[i] >= 65535.0 {
                        poles += 1;
                    }
                    if z[i] < z[i - 1] {
                        err::signal_error(ctx, ERROR_RANGE, "cmsSmoothToneCurve: Non-Monotonic.");
                        success = not_check;
                        break;
                    }
                }

                if success && zeros > (n_items as u32 / 3) {
                    err::signal_error(
                        ctx,
                        ERROR_RANGE,
                        "cmsSmoothToneCurve: Degenerated, mostly zeros.",
                    );
                    success = not_check;
                }

                if success && poles > (n_items as u32 / 3) {
                    err::signal_error(
                        ctx,
                        ERROR_RANGE,
                        "cmsSmoothToneCurve: Degenerated, mostly poles.",
                    );
                    success = not_check;
                }

                if success {
                    for i in 0..n_items {
                        tab.table16[i] = quick_saturate_word(z[i + 1] as f64);
                    }
                }
            } else {
                err::signal_error(
                    ctx,
                    ERROR_RANGE,
                    "cmsSmoothToneCurve: Function smooth2 failed.",
                );
                success = false;
            }
        } else {
            err::signal_error(ctx, ERROR_RANGE, "cmsSmoothToneCurve: Too many points.");
            success = false;
        }
    }

    success
}

// ---------------------------------------------------------------------------
// Info functions
// ---------------------------------------------------------------------------

/// `cmsIsToneCurveLinear` — linear in 12 bits.
pub fn is_tone_curve_linear(curve: &ToneCurve) -> bool {
    for i in 0..curve.n_entries as i64 {
        let diff = (curve.table16[i as usize] as i32
            - quantize_val(i as f64, curve.n_entries) as i32)
            .abs();
        if diff > 0x0f {
            return false;
        }
    }
    true
}

/// `cmsIsToneCurveMonotonic` — allows a ripple of 2.
pub fn is_tone_curve_monotonic(t: &ToneCurve) -> bool {
    // Degenerated curves are monotonic? Ok, let's pass them
    let n = t.n_entries;
    if n < 2 {
        return true;
    }

    let l_descending = is_tone_curve_descending(t);
    if l_descending {
        let mut last = t.table16[0] as i32;
        for i in 1..n as usize {
            if t.table16[i] as i32 - last > 2 {
                return false;
            } else {
                last = t.table16[i] as i32;
            }
        }
    } else {
        let mut last = t.table16[n as usize - 1] as i32;
        for i in (0..n as usize - 1).rev() {
            if t.table16[i] as i32 - last > 2 {
                return false;
            } else {
                last = t.table16[i] as i32;
            }
        }
    }
    true
}

/// `cmsIsToneCurveDescending`
pub fn is_tone_curve_descending(t: &ToneCurve) -> bool {
    t.table16[0] > t.table16[t.n_entries as usize - 1]
}

/// `cmsIsToneCurveMultisegment`
pub fn is_tone_curve_multisegment(t: &ToneCurve) -> bool {
    t.segments.len() > 1
}

/// `cmsEstimateGamma` — least-squares gamma fit.
pub fn estimate_gamma(t: &ToneCurve, precision: f64) -> f64 {
    let mut sum = 0.0f64;
    let mut sum2 = 0.0f64;
    let mut n = 0.0f64;

    // Excluding endpoints
    for i in 1..MAX_NODES_IN_CURVE - 1 {
        let x = i as f64 / (MAX_NODES_IN_CURVE - 1) as f64;
        let y = t.eval_float(x as f32) as f64;

        // Avoid 7% on lower part to prevent artifacts due to linear ramps
        if y > 0.0 && y < 1.0 && x > 0.07 {
            let gamma = y.cln() / x.cln();
            sum += gamma;
            sum2 += gamma * gamma;
            n += 1.0;
        }
    }

    // We need enough valid samples
    if n <= 1.0 {
        return -1.0;
    }

    // Take a look on SD to see if gamma isn't exponential at all
    let std_dev = ((n * sum2 - sum * sum) / (n * (n - 1.0))).sqrt();
    if std_dev > precision {
        return -1.0;
    }

    sum / n
}

impl Clone for ToneCurve {
    fn clone(&self) -> Self {
        dup_tone_curve(self, None).expect("tone curve dup")
    }
}
