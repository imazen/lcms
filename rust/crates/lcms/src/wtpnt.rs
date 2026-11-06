//! Port of `cmswtpnt.c` — white points, correlated temperature, chromatic
//! adaptation.
//!
//! C source: `reference/lcms2/src/cmswtpnt.c`
//! Gate: `tests/c_parity_wtpnt.rs`

use crate::err;
use crate::mtrx::{MATRIX_DET_TOLERANCE, mat3_eval, mat3_inverse, mat3_per, vec3_init};
use crate::pcs::xyy2xyz;
use crate::types::*;

/// `cmsD50_XYZ` — the D50 white point as XYZ
pub fn d50_xyz() -> CieXyz {
    CieXyz {
        X: D50X,
        Y: D50Y,
        Z: D50Z,
    }
}

/// `cmsD50_xyY` — D50 in xyY (computed each call, as C does)
pub fn d50_xyy() -> CiexyY {
    crate::pcs::xyz2xyy(&d50_xyz())
}

/// `cmsWhitePointFromTemp` — obtains WhitePoint from Temperature.
/// (C takes no context; it signals on the global one.)
pub fn white_point_from_temp(temp_k: f64) -> Option<CiexyY> {
    let t = temp_k;
    let t2 = t * t; // Square
    let t3 = t2 * t; // Cube

    // For correlated color temperature (T) between 4000K and 7000K:
    let x = if (4000. ..=7000.).contains(&t) {
        -4.6070 * (1e9 / t3) + 2.9678 * (1e6 / t2) + 0.09911 * (1e3 / t) + 0.244063
    } else if t > 7000.0 && t <= 25000.0 {
        // or for correlated color temperature (T) between 7000K and 25000K:
        -2.0064 * (1e9 / t3) + 1.9018 * (1e6 / t2) + 0.24748 * (1e3 / t) + 0.237040
    } else {
        err::signal_error(None, ERROR_RANGE, "cmsWhitePointFromTemp: invalid temp");
        return None;
    };

    // Obtain y(x)
    let y = -3.000 * (x * x) + 2.870 * x - 0.275;

    Some(CiexyY { x, y, Y: 1.0 })
}

/// `ISOTEMPERATURE` — mirek, ut, vt, tt
struct IsoTemperature {
    mirek: f64,
    ut: f64,
    vt: f64,
    tt: f64,
}

#[rustfmt::skip]
const ISOTEMPDATA: [IsoTemperature; 31] = [
//    mirek   ut        vt        tt
    IsoTemperature { mirek: 0.,   ut: 0.18006, vt: 0.26352, tt: -0.24341 },
    IsoTemperature { mirek: 10.,  ut: 0.18066, vt: 0.26589, tt: -0.25479 },
    IsoTemperature { mirek: 20.,  ut: 0.18133, vt: 0.26846, tt: -0.26876 },
    IsoTemperature { mirek: 30.,  ut: 0.18208, vt: 0.27119, tt: -0.28539 },
    IsoTemperature { mirek: 40.,  ut: 0.18293, vt: 0.27407, tt: -0.30470 },
    IsoTemperature { mirek: 50.,  ut: 0.18388, vt: 0.27709, tt: -0.32675 },
    IsoTemperature { mirek: 60.,  ut: 0.18494, vt: 0.28021, tt: -0.35156 },
    IsoTemperature { mirek: 70.,  ut: 0.18611, vt: 0.28342, tt: -0.37915 },
    IsoTemperature { mirek: 80.,  ut: 0.18740, vt: 0.28668, tt: -0.40955 },
    IsoTemperature { mirek: 90.,  ut: 0.18880, vt: 0.28997, tt: -0.44278 },
    IsoTemperature { mirek: 100., ut: 0.19032, vt: 0.29326, tt: -0.47888 },
    IsoTemperature { mirek: 125., ut: 0.19462, vt: 0.30141, tt: -0.58204 },
    IsoTemperature { mirek: 150., ut: 0.19962, vt: 0.30921, tt: -0.70471 },
    IsoTemperature { mirek: 175., ut: 0.20525, vt: 0.31647, tt: -0.84901 },
    IsoTemperature { mirek: 200., ut: 0.21142, vt: 0.32312, tt: -1.0182  },
    IsoTemperature { mirek: 225., ut: 0.21807, vt: 0.32909, tt: -1.2168  },
    IsoTemperature { mirek: 250., ut: 0.22511, vt: 0.33439, tt: -1.4512  },
    IsoTemperature { mirek: 275., ut: 0.23247, vt: 0.33904, tt: -1.7298  },
    IsoTemperature { mirek: 300., ut: 0.24010, vt: 0.34308, tt: -2.0637  },
    IsoTemperature { mirek: 325., ut: 0.24702, vt: 0.34655, tt: -2.4681  },
    IsoTemperature { mirek: 350., ut: 0.25591, vt: 0.34951, tt: -2.9641  },
    IsoTemperature { mirek: 375., ut: 0.26400, vt: 0.35200, tt: -3.5814  },
    IsoTemperature { mirek: 400., ut: 0.27218, vt: 0.35407, tt: -4.3633  },
    IsoTemperature { mirek: 425., ut: 0.28039, vt: 0.35577, tt: -5.3762  },
    IsoTemperature { mirek: 450., ut: 0.28863, vt: 0.35714, tt: -6.7262  },
    IsoTemperature { mirek: 475., ut: 0.29685, vt: 0.35823, tt: -8.5955  },
    IsoTemperature { mirek: 500., ut: 0.30505, vt: 0.35907, tt: -11.324  },
    IsoTemperature { mirek: 525., ut: 0.31320, vt: 0.35968, tt: -15.628  },
    IsoTemperature { mirek: 550., ut: 0.32129, vt: 0.36011, tt: -23.325  },
    IsoTemperature { mirek: 575., ut: 0.32931, vt: 0.36038, tt: -40.770  },
    IsoTemperature { mirek: 600., ut: 0.33724, vt: 0.36051, tt: -116.45  },
];

/// `cmsTempFromWhitePoint` — Robertson's method
pub fn temp_from_white_point(white_point: &CiexyY) -> Option<f64> {
    let mut di = 0.0;
    let mut mi = 0.0;
    let xs = white_point.x;
    let ys = white_point.y;

    // convert (x,y) to CIE 1960 (u,v)
    let us = (2.0 * xs) / (-xs + 6.0 * ys + 1.5);
    let vs = (3.0 * ys) / (-xs + 6.0 * ys + 1.5);

    for (j, iso) in ISOTEMPDATA.iter().enumerate() {
        let uj = iso.ut;
        let vj = iso.vt;
        let tj = iso.tt;
        let mj = iso.mirek;

        let dj = ((vs - vj) - tj * (us - uj)) / (1.0 + tj * tj).sqrt();

        if j != 0 && (di / dj) < 0.0 {
            // Found a match
            return Some(1000000.0 / (mi + (di / (di - dj)) * (mj - mi)));
        }

        di = dj;
        mi = mj;
    }

    // Not found
    None
}

/// `ComputeChromaticAdaptation` (static in C) — using Chad as cone matrix
fn compute_chromatic_adaptation(
    conversion: &mut Mat3,
    source_white_point: &CieXyz,
    dest_white_point: &CieXyz,
    chad: &Mat3,
) -> bool {
    let tmp = *chad;
    let mut chad_inv = Mat3::default();
    if !mat3_inverse(&tmp, &mut chad_inv) {
        return false;
    }

    let mut cone_source_xyz = Vec3::default();
    let mut cone_source_rgb = Vec3::default();
    let mut cone_dest_xyz = Vec3::default();
    let mut cone_dest_rgb = Vec3::default();
    let mut cone = Mat3::default();

    vec3_init(
        &mut cone_source_xyz,
        source_white_point.X,
        source_white_point.Y,
        source_white_point.Z,
    );
    vec3_init(
        &mut cone_dest_xyz,
        dest_white_point.X,
        dest_white_point.Y,
        dest_white_point.Z,
    );

    mat3_eval(&mut cone_source_rgb, chad, &cone_source_xyz);
    mat3_eval(&mut cone_dest_rgb, chad, &cone_dest_xyz);

    if cone_source_rgb.n[0].abs() < MATRIX_DET_TOLERANCE
        || cone_source_rgb.n[1].abs() < MATRIX_DET_TOLERANCE
        || cone_source_rgb.n[2].abs() < MATRIX_DET_TOLERANCE
    {
        return false;
    }

    // Build matrix
    vec3_init(
        &mut cone.v[0],
        cone_dest_rgb.n[0] / cone_source_rgb.n[0],
        0.0,
        0.0,
    );
    vec3_init(
        &mut cone.v[1],
        0.0,
        cone_dest_rgb.n[1] / cone_source_rgb.n[1],
        0.0,
    );
    vec3_init(
        &mut cone.v[2],
        0.0,
        0.0,
        cone_dest_rgb.n[2] / cone_source_rgb.n[2],
    );

    // Normalize
    let mut tmp2 = Mat3::default();
    mat3_per(&mut tmp2, &cone, chad);
    mat3_per(conversion, &chad_inv, &tmp2);

    true
}

/// `_cmsAdaptationMatrix` — ConeMatrix `None` → Bradford
pub fn adaptation_matrix(
    r: &mut Mat3,
    cone_matrix: Option<&Mat3>,
    from_ill: &CieXyz,
    to_ill: &CieXyz,
) -> bool {
    const LAM_RIGG: Mat3 = Mat3 {
        // Bradford matrix
        v: [
            Vec3 {
                n: [0.8951, 0.2664, -0.1614],
            },
            Vec3 {
                n: [-0.7502, 1.7135, 0.0367],
            },
            Vec3 {
                n: [0.0389, -0.0685, 1.0296],
            },
        ],
    };

    let chad = cone_matrix.copied().unwrap_or(LAM_RIGG);
    compute_chromatic_adaptation(r, from_ill, to_ill, &chad)
}

/// `_cmsAdaptMatrixToD50` (static in C) — assuming D50 destination
pub(crate) fn adapt_matrix_to_d50(r: &mut Mat3, source_white_pt: &CiexyY) -> bool {
    let dn = xyy2xyz(source_white_pt);
    let mut bradford = Mat3::default();

    if !adaptation_matrix(&mut bradford, None, &dn, &d50_xyz()) {
        return false;
    }

    let tmp = *r;
    mat3_per(r, &bradford, &tmp);

    true
}

/// `_cmsBuildRGB2XYZtransferMatrix`
///
/// Build a white point + primary chromas transfer matrix from RGB to CIE XYZ.
/// This is just an approximation: non-linear aspects are not handled, gamma
/// correction is assumed transitive across the transformation chain.
pub fn build_rgb2xyz_transfer_matrix(
    r: &mut Mat3,
    white_pt: &CiexyY,
    primrs: &CiexyYTriple,
) -> bool {
    let xn = white_pt.x;
    let yn = white_pt.y;
    let xr = primrs.Red.x;
    let yr = primrs.Red.y;
    let xg = primrs.Green.x;
    let yg = primrs.Green.y;
    let xb = primrs.Blue.x;
    let yb = primrs.Blue.y;

    // Build Primaries matrix
    let mut primaries = Mat3::default();
    vec3_init(&mut primaries.v[0], xr, xg, xb);
    vec3_init(&mut primaries.v[1], yr, yg, yb);
    vec3_init(
        &mut primaries.v[2],
        1.0 - xr - yr,
        1.0 - xg - yg,
        1.0 - xb - yb,
    );

    // Result = Primaries ^ (-1) inverse matrix
    let mut result = Mat3::default();
    if !mat3_inverse(&primaries, &mut result) {
        return false;
    }

    let mut white_point = Vec3::default();
    vec3_init(&mut white_point, xn / yn, 1.0, (1.0 - xn - yn) / yn);

    // Across inverse primaries ...
    let mut coef = Vec3::default();
    mat3_eval(&mut coef, &result, &white_point);

    // Give us the Coefs, then I build transformation matrix
    vec3_init(&mut r.v[0], coef.n[0] * xr, coef.n[1] * xg, coef.n[2] * xb);
    vec3_init(&mut r.v[1], coef.n[0] * yr, coef.n[1] * yg, coef.n[2] * yb);
    vec3_init(
        &mut r.v[2],
        coef.n[0] * (1.0 - xr - yr),
        coef.n[1] * (1.0 - xg - yg),
        coef.n[2] * (1.0 - xb - yb),
    );

    adapt_matrix_to_d50(r, white_pt)
}

/// `cmsAdaptToIlluminant`
///
/// Adapts a color to a given illuminant. Original color is expected to have
/// a `source_white_pt` white point.
pub fn adapt_to_illuminant(
    source_white_pt: &CieXyz,
    illuminant: &CieXyz,
    value: &CieXyz,
) -> Option<CieXyz> {
    let mut bradford = Mat3::default();

    if !adaptation_matrix(&mut bradford, None, source_white_pt, illuminant) {
        return None;
    }

    let mut in_v = Vec3::default();
    vec3_init(&mut in_v, value.X, value.Y, value.Z);
    let mut out_v = Vec3::default();
    mat3_eval(&mut out_v, &bradford, &in_v);

    Some(CieXyz {
        X: out_v.n[0],
        Y: out_v.n[1],
        Z: out_v.n[2],
    })
}
