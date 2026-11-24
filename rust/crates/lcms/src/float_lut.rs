//! Sampled float curves for opt-in packed RGB matrix-shaper plans.

use crate::gamma::{ToneCurve, default_eval_parametric_fn};
use crate::lut::{Pipeline, StageData};
use crate::types::CpowExt;

// Place 1024 intervals per binary exponent. Near the dark end this is
// much denser than a uniform grid and accounts for the inverse curvature.
const SHIFT: u32 = 13;
const STEP: u32 = 1 << SHIFT;
const LOWEST: f32 = 1.0 / 16_777_216.0;

struct SampledCurve {
    start: u32,
    samples: Box<[[f32; 2]]>,
    kind: i32,
    params: [f64; 10],
    cutoff: f64,
}

impl SampledCurve {
    fn new(curves: &[ToneCurve; 3], inverse: bool) -> Option<Self> {
        let segment = curves[0].stock_parametric_segment()?;
        let kind = segment.r#type;
        let params = segment.params;
        let [gamma, a, b, c, d, ..] = params;
        if !(1.0..=3.0).contains(&gamma) {
            return None;
        }
        let cutoff = match (inverse, kind) {
            (false, 1) | (true, -1) => 0.0,
            (false, 4) | (true, -4)
                if (0.5..=2.0).contains(&a)
                    && (0.0..=0.5).contains(&b)
                    && (1.0 / 32.0..=4.0).contains(&c)
                    && (0.0..=0.25).contains(&d) =>
            {
                if inverse {
                    (a * d + b).cpow(gamma)
                } else {
                    d
                }
            }
            _ => return None,
        };
        for curve in &curves[1..] {
            let other = curve.stock_parametric_segment()?;
            if other.r#type != kind || other.params.map(f64::to_bits) != params.map(f64::to_bits) {
                return None;
            }
        }

        // Leave the interval crossing a linear/power join to the original
        // evaluator. An ICC curve's two branches need not meet exactly.
        let first = cutoff.max(f64::from(LOWEST));
        let mut start = (first as f32).to_bits().next_multiple_of(STEP);
        if f64::from(f32::from_bits(start)) < first {
            start += STEP;
        }
        let end = if inverse { 2.0f32 } else { 1.0f32 }.to_bits();
        let intervals = end.checked_sub(start)? / STEP;
        if intervals == 0 {
            return None;
        }
        let mut samples = bytemuck::try_zeroed_vec::<[f32; 2]>(intervals as usize).ok()?;
        let evaluate = |bits| curves[0].eval_float(f32::from_bits(bits));
        let mut lo = evaluate(start);
        let mut bits = start;
        for value in &mut samples {
            bits += STEP;
            let hi = evaluate(bits);
            *value = [lo, hi - lo];
            lo = hi;
        }
        Some(Self {
            start,
            samples: samples.into_boxed_slice(),
            kind,
            params,
            cutoff,
        })
    }

    #[inline]
    fn eval(&self, x: f32) -> f32 {
        let code = x.to_bits().wrapping_sub(self.start);
        if let Some(&[lo, delta]) = self.samples.get((code >> SHIFT) as usize) {
            let fraction = (code & (STEP - 1)) as f32 * (1.0 / STEP as f32);
            return lo + delta * fraction;
        }
        let x = f64::from(x);
        if x < self.cutoff {
            match self.kind {
                4 => return (x * self.params[3]) as f32,
                -4 => return (x / self.params[3]) as f32,
                _ => {}
            }
        }
        default_eval_parametric_fn(self.kind, &self.params, x) as f32
    }
}

pub(crate) struct FloatMatrixShaper {
    decode: SampledCurve,
    encode: SampledCurve,
    matrix: [[f64; 3]; 3],
    offset: Option<[f64; 3]>,
}

impl FloatMatrixShaper {
    pub(crate) fn new(pipeline: &Pipeline) -> Option<Box<Self>> {
        let (decode, matrix, encode) = pipeline.float_matrix_shaper()?;
        let StageData::Matrix { double, offset } = &matrix.data else {
            return None;
        };
        let (rows, remainder) = double.as_chunks::<3>();
        let matrix: [[f64; 3]; 3] = rows.try_into().ok()?;
        if !remainder.is_empty()
            || matrix
                .iter()
                .any(|row| row.iter().map(|x| x.abs()).sum::<f64>() > 8.0)
            || matrix.as_flattened().iter().any(|x| !x.is_finite())
        {
            return None;
        }
        let offset = if let Some(offset) = offset {
            if offset.iter().any(|x| !(-2.0..=2.0).contains(x)) {
                return None;
            }
            Some(offset.as_slice().try_into().ok()?)
        } else {
            None
        };
        Some(Box::new(Self {
            decode: SampledCurve::new(decode, false)?,
            encode: SampledCurve::new(encode, true)?,
            matrix,
            offset,
        }))
    }

    #[inline]
    pub(crate) fn eval(&self, pixel: [f32; 4], pipeline: &Pipeline) -> [f32; 4] {
        let [r, g, b, _] = pixel;
        if ![r, g, b].into_iter().all(|x| (0.0..=1.0).contains(&x)) {
            let mut output = [0.0; 4];
            pipeline.eval_float_compact(&pixel, &mut output);
            return output;
        }

        let [r, g, b] = [r, g, b].map(|x| f64::from(self.decode.eval(x)));
        let mut output = [0.0; 4];
        for (i, [a, c, d]) in self.matrix.into_iter().enumerate() {
            let mut out = ((0.0 + a * r) + c * g) + d * b;
            if let Some(offset) = self.offset {
                out += offset[i];
            }
            output[i] = self.encode.eval(out as f32);
        }
        output
    }
}
