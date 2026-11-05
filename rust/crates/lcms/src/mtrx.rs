//! Port of `cmsmtrx.c` — 3x3 matrices and 3-vectors.
//!
//! C source: `reference/lcms2/src/cmsmtrx.c`
//! Gate: `tests/c_parity_mtrx.rs`

use crate::types::{Mat3, Vec3};

const VX: usize = 0;
const VY: usize = 1;
const VZ: usize = 2;

/// Determinant lower than this is assumed zero (used on matrix invert).
/// `MATRIX_DET_TOLERANCE`
pub const MATRIX_DET_TOLERANCE: f64 = 0.0001;

/// `_cmsVEC3init`
pub fn vec3_init(r: &mut Vec3, x: f64, y: f64, z: f64) {
    r.n[VX] = x;
    r.n[VY] = y;
    r.n[VZ] = z;
}

/// `_cmsVEC3minus` — vector subtraction
pub fn vec3_minus(r: &mut Vec3, a: &Vec3, b: &Vec3) {
    r.n[VX] = a.n[VX] - b.n[VX];
    r.n[VY] = a.n[VY] - b.n[VY];
    r.n[VZ] = a.n[VZ] - b.n[VZ];
}

/// `_cmsVEC3cross` — vector cross product
pub fn vec3_cross(r: &mut Vec3, u: &Vec3, v: &Vec3) {
    r.n[VX] = u.n[VY] * v.n[VZ] - v.n[VY] * u.n[VZ];
    r.n[VY] = u.n[VZ] * v.n[VX] - v.n[VZ] * u.n[VX];
    r.n[VZ] = u.n[VX] * v.n[VY] - v.n[VX] * u.n[VY];
}

/// `_cmsVEC3dot` — vector dot product
pub fn vec3_dot(u: &Vec3, v: &Vec3) -> f64 {
    u.n[VX] * v.n[VX] + u.n[VY] * v.n[VY] + u.n[VZ] * v.n[VZ]
}

/// `_cmsVEC3length` — Euclidean length
pub fn vec3_length(a: &Vec3) -> f64 {
    (a.n[VX] * a.n[VX] + a.n[VY] * a.n[VY] + a.n[VZ] * a.n[VZ]).sqrt()
}

/// `_cmsVEC3distance` — Euclidean distance
pub fn vec3_distance(a: &Vec3, b: &Vec3) -> f64 {
    let d1 = a.n[VX] - b.n[VX];
    let d2 = a.n[VY] - b.n[VY];
    let d3 = a.n[VZ] - b.n[VZ];

    (d1 * d1 + d2 * d2 + d3 * d3).sqrt()
}

/// `_cmsMAT3identity` — 3x3 identity
pub fn mat3_identity(a: &mut Mat3) {
    vec3_init(&mut a.v[0], 1.0, 0.0, 0.0);
    vec3_init(&mut a.v[1], 0.0, 1.0, 0.0);
    vec3_init(&mut a.v[2], 0.0, 0.0, 1.0);
}

/// `CloseEnough` (static in C)
fn close_enough(a: f64, b: f64) -> bool {
    (b - a).abs() < (1.0 / 65535.0)
}

/// `_cmsMAT3isIdentity`
pub fn mat3_is_identity(a: &Mat3) -> bool {
    let mut identity = Mat3::default();
    mat3_identity(&mut identity);

    for i in 0..3 {
        for j in 0..3 {
            if !close_enough(a.v[i].n[j], identity.v[i].n[j]) {
                return false;
            }
        }
    }
    true
}

/// `_cmsMAT3per` — multiply two matrices
pub fn mat3_per(r: &mut Mat3, a: &Mat3, b: &Mat3) {
    macro_rules! rowcol {
        ($i:expr, $j:expr) => {
            a.v[$i].n[0] * b.v[0].n[$j] + a.v[$i].n[1] * b.v[1].n[$j] + a.v[$i].n[2] * b.v[2].n[$j]
        };
    }

    vec3_init(&mut r.v[0], rowcol!(0, 0), rowcol!(0, 1), rowcol!(0, 2));
    vec3_init(&mut r.v[1], rowcol!(1, 0), rowcol!(1, 1), rowcol!(1, 2));
    vec3_init(&mut r.v[2], rowcol!(2, 0), rowcol!(2, 1), rowcol!(2, 2));
}

/// `_cmsMAT3inverse` — inverse of a matrix b = a^(-1)
pub fn mat3_inverse(a: &Mat3, b: &mut Mat3) -> bool {
    let c0 = a.v[1].n[1] * a.v[2].n[2] - a.v[1].n[2] * a.v[2].n[1];
    let c1 = -a.v[1].n[0] * a.v[2].n[2] + a.v[1].n[2] * a.v[2].n[0];
    let c2 = a.v[1].n[0] * a.v[2].n[1] - a.v[1].n[1] * a.v[2].n[0];

    let det = a.v[0].n[0] * c0 + a.v[0].n[1] * c1 + a.v[0].n[2] * c2;

    if det.abs() < MATRIX_DET_TOLERANCE {
        return false; // singular matrix; can't invert
    }

    b.v[0].n[0] = c0 / det;
    b.v[0].n[1] = (a.v[0].n[2] * a.v[2].n[1] - a.v[0].n[1] * a.v[2].n[2]) / det;
    b.v[0].n[2] = (a.v[0].n[1] * a.v[1].n[2] - a.v[0].n[2] * a.v[1].n[1]) / det;
    b.v[1].n[0] = c1 / det;
    b.v[1].n[1] = (a.v[0].n[0] * a.v[2].n[2] - a.v[0].n[2] * a.v[2].n[0]) / det;
    b.v[1].n[2] = (a.v[0].n[2] * a.v[1].n[0] - a.v[0].n[0] * a.v[1].n[2]) / det;
    b.v[2].n[0] = c2 / det;
    b.v[2].n[1] = (a.v[0].n[1] * a.v[2].n[0] - a.v[0].n[0] * a.v[2].n[1]) / det;
    b.v[2].n[2] = (a.v[0].n[0] * a.v[1].n[1] - a.v[0].n[1] * a.v[1].n[0]) / det;

    true
}

/// `_cmsMAT3solve` — solve a system in the form Ax = b
pub fn mat3_solve(x: &mut Vec3, a: &Mat3, b: &Vec3) -> bool {
    let mut a_1 = Mat3::default();

    if !mat3_inverse(a, &mut a_1) {
        return false; // Singular matrix
    }

    mat3_eval(x, &a_1, b);
    true
}

/// `_cmsMAT3eval` — evaluate a vector across a matrix
pub fn mat3_eval(r: &mut Vec3, a: &Mat3, v: &Vec3) {
    r.n[VX] = a.v[0].n[VX] * v.n[VX] + a.v[0].n[VY] * v.n[VY] + a.v[0].n[VZ] * v.n[VZ];
    r.n[VY] = a.v[1].n[VX] * v.n[VX] + a.v[1].n[VY] * v.n[VY] + a.v[1].n[VZ] * v.n[VZ];
    r.n[VZ] = a.v[2].n[VX] * v.n[VX] + a.v[2].n[VY] * v.n[VY] + a.v[2].n[VZ] * v.n[VZ];
}
