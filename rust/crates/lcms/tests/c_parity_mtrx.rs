//! Differential test: `lcms::mtrx` vs `cmsmtrx.c`.

use lcms::mtrx::*;
use lcms::types::{Mat3, Vec3};
use lcms_cref as c;

fn rng(state: &mut u64) -> u64 {
    *state = state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    *state >> 11
}

fn f64_in(state: &mut u64, lo: f64, hi: f64) -> f64 {
    let u = rng(state) as f64 / (1u64 << 53) as f64;
    lo + u * (hi - lo)
}

fn rand_vec3(s: &mut u64) -> Vec3 {
    Vec3 {
        n: [
            f64_in(s, -5.0, 5.0),
            f64_in(s, -5.0, 5.0),
            f64_in(s, -5.0, 5.0),
        ],
    }
}

fn rand_mat3(s: &mut u64) -> Mat3 {
    Mat3 {
        v: [rand_vec3(s), rand_vec3(s), rand_vec3(s)],
    }
}

fn cv(v: &Vec3) -> c::Vec3 {
    c::Vec3 { n: v.n }
}
fn vb(n: &[f64; 3]) -> [u64; 3] {
    [n[0].to_bits(), n[1].to_bits(), n[2].to_bits()]
}
fn cm(m: &Mat3) -> c::Mat3 {
    c::Mat3 {
        v: [cv(&m.v[0]), cv(&m.v[1]), cv(&m.v[2])],
    }
}

#[test]
fn vec3_ops_match_c() {
    let mut s = 0x42u64;
    for _ in 0..10_000 {
        let a = rand_vec3(&mut s);
        let b = rand_vec3(&mut s);

        let mut r = Vec3::default();
        let mut cr = c::Vec3::default();

        vec3_minus(&mut r, &a, &b);
        unsafe { c::_cmsVEC3minus(&mut cr, &cv(&a), &cv(&b)) };
        assert_eq!(vb(&r.n), vb(&cr.n), "minus {a:?} {b:?}");

        vec3_cross(&mut r, &a, &b);
        unsafe { c::_cmsVEC3cross(&mut cr, &cv(&a), &cv(&b)) };
        assert_eq!(vb(&r.n), vb(&cr.n), "cross {a:?} {b:?}");

        let c_dot = unsafe { c::_cmsVEC3dot(&cv(&a), &cv(&b)) };
        assert_eq!(vec3_dot(&a, &b).to_bits(), c_dot.to_bits(), "dot");

        let c_len = unsafe { c::_cmsVEC3length(&cv(&a)) };
        assert_eq!(vec3_length(&a).to_bits(), c_len.to_bits(), "length");

        let c_dist = unsafe { c::_cmsVEC3distance(&cv(&a), &cv(&b)) };
        assert_eq!(vec3_distance(&a, &b).to_bits(), c_dist.to_bits(), "dist");
    }
}

#[test]
fn mat3_identity_matches_c() {
    let mut m = Mat3::default();
    let mut cm_ = c::Mat3::default();
    mat3_identity(&mut m);
    unsafe { c::_cmsMAT3identity(&mut cm_) };
    for i in 0..3 {
        assert_eq!(vb(&m.v[i].n), vb(&cm_.v[i].n));
    }
    assert!(mat3_is_identity(&m));
    assert_eq!(unsafe { c::_cmsMAT3isIdentity(&cm_) } != 0, true);
}

#[test]
fn mat3_per_eval_inverse_solve() {
    let mut s = 0x77u64;
    let mut disagreements = 0u32;
    for _ in 0..10_000 {
        let a = rand_mat3(&mut s);
        let b = rand_mat3(&mut s);

        // per
        let mut r = Mat3::default();
        let mut cr = c::Mat3::default();
        mat3_per(&mut r, &a, &b);
        unsafe { c::_cmsMAT3per(&mut cr, &cm(&a), &cm(&b)) };
        for i in 0..3 {
            assert_eq!(vb(&r.v[i].n), vb(&cr.v[i].n), "per {a:?} {b:?}");
        }

        // inverse (including the singular-det path)
        let mut inv = Mat3::default();
        let mut cinv = c::Mat3::default();
        let ok = mat3_inverse(&a, &mut inv);
        let cok = unsafe { c::_cmsMAT3inverse(&cm(&a), &mut cinv) } != 0;
        assert_eq!(ok, cok, "inverse bool {a:?}");
        if ok {
            for i in 0..3 {
                assert_eq!(vb(&inv.v[i].n), vb(&cinv.v[i].n), "inverse {a:?}");
            }
        } else {
            disagreements += 1;
        }

        // eval
        let v = rand_vec3(&mut s);
        let mut ev = Vec3::default();
        let mut cev = c::Vec3::default();
        mat3_eval(&mut ev, &a, &v);
        unsafe { c::_cmsMAT3eval(&mut cev, &cm(&a), &cv(&v)) };
        assert_eq!(vb(&ev.n), vb(&cev.n), "eval {a:?} {v:?}");

        // solve
        let mut xv = Vec3::default();
        let mut cxv = c::Vec3::default();
        let mut ca = cm(&a);
        let mut cb = cv(&b.v[0]);
        let ok = mat3_solve(&mut xv, &a, &b.v[0]);
        let cok = unsafe { c::_cmsMAT3solve(&mut cxv, &mut ca, &mut cb) } != 0;
        assert_eq!(ok, cok, "solve bool {a:?}");
        if ok {
            assert_eq!(vb(&xv.n), vb(&cxv.n), "solve {a:?}");
        }
    }
    // The sweep must have exercised the singular path at least once
    // (random near-singular matrices are rare — inject some deliberately).
    let mut singular_seen = disagreements;
    for k in 0..100 {
        let mut a = rand_mat3(&mut s);
        a.v[2] = a.v[0]; // duplicate row → singular
        let mut inv = Mat3::default();
        let mut cinv = c::Mat3::default();
        let ok = mat3_inverse(&a, &mut inv);
        let cok = unsafe { c::_cmsMAT3inverse(&cm(&a), &mut cinv) } != 0;
        assert_eq!(ok, cok, "singular inverse bool");
        if !ok {
            singular_seen += 1;
        }
        let _ = k;
    }
    assert!(singular_seen > 0, "no singular matrix exercised");
}
