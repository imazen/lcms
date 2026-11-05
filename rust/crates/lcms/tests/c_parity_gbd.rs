//! GBD (gamut boundary descriptor) parity: C `cmsGBD*` vs Rust `Gbd`.
//! Same points added, same containment checks, same Compute result.

use lcms::gbd::Gbd;
use lcms::types::CieLab;
use lcms_cref as c;

#[repr(C)]
struct CLab {
    l: f64,
    a: f64,
    b: f64,
}

fn inside(pt: &CieLab) -> (bool, bool) {
    // Rust
    let mut g = Gbd::alloc(None);
    let pts = [
        CieLab { L: 50.0, a: 40.0, b: 40.0 },
        CieLab { L: 60.0, a: -40.0, b: -40.0 },
        CieLab { L: 30.0, a: 80.0, b: -80.0 },
        CieLab { L: 90.0, a: -80.0, b: 80.0 },
    ];
    for p in &pts {
        assert!(g.add_point(p));
    }
    g.compute(0);
    let rust = g.check_point(pt);
    // C
    let c_pts: Vec<CLab> = pts.iter().map(|p| CLab { l: p.L, a: p.a, b: p.b }).collect();
    let cres = unsafe {
        let g = c::cmsGBDAlloc(std::ptr::null_mut());
        assert!(!g.is_null());
        for p in &c_pts {
            c::cmsGDBAddPoint(g, p as *const _ as *const _);
        }
        assert_eq!(c::cmsGDBCompute(g, 0), 1);
        let cl = CLab { l: pt.L, a: pt.a, b: pt.b };
        let r = c::cmsGDBCheckPoint(g, &cl as *const _ as *const _);
        c::cmsGBDFree(g);
        r
    };
    (cres != 0, rust)
}

#[test]
fn gbd_add_and_check() {
    // a point deep inside the hull
    let (c_in, r_in) = inside(&CieLab { L: 55.0, a: 20.0, b: 20.0 });
    // a point far outside
    let (c_out, r_out) = inside(&CieLab { L: 95.0, a: 127.0, b: 127.0 });
    assert_eq!(c_in, r_in, "inside-check diverged");
    assert_eq!(c_out, r_out, "outside-check diverged");
}
