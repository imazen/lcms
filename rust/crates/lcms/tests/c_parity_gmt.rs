//! cmsgmt parity: GBD compute on varied point clouds, checkpoint before
//! and after compute, cmsIsCLUT across corpus x intent x direction.

use lcms::{gbd::Gbd, profile, types::*};
use lcms_cref as c;
use std::ffi::c_void;

fn corpus() -> Vec<Vec<u8>> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
    let mut files: Vec<_> = std::fs::read_dir(&dir).unwrap()
        .filter_map(|e| e.ok()).map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "icc").unwrap_or(false))
        .collect();
    files.sort();
    files.iter().map(|f| std::fs::read(f).unwrap()).collect()
}

#[test]
fn gbd_compute_variants() {
    // point-cloud shapes: cube shell, sphere, tetrahedron, degenerate
    let mut seed = 0x9999u64;
    for shape in 0..4u32 {
        unsafe {
            let cg = c::cmsGBDAlloc(std::ptr::null_mut());
            let mut rg = Gbd::alloc(None);
            assert!(!cg.is_null());
            // add the same deterministic point set on both sides
            for i in 0..200 {
                seed = seed.wrapping_mul(0x9e3779b97f4a7c15) ^ (seed >> 33);
                let (l, a, b) = match shape {
                    0 => ( // cube shell — face centers + corners
                        if i % 3 == 0 { 20.0 } else { 80.0 },
                        (((seed >> 48) as u16 % 200) as f64) - 100.0,
                        (((seed >> 32) as u16 % 200) as f64) - 100.0,
                    ),
                    1 => { // sphere-ish
                        let t = (seed >> 48) as u16 as f64 / 65535.0 * std::f64::consts::TAU;
                        let r = 40.0 + (seed >> 32) as u16 as f64 / 65535.0 * 30.0;
                        (50.0 + t.sin() * 20.0, r * t.cos(), r * t.sin() * 0.8)
                    }
                    2 => { // degenerate — nearly-planar
                        (((seed >> 48) as u16 % 100) as f64,
                         ((seed >> 32) as u16 % 50) as f64 - 25.0,
                         ((seed >> 48) as u16 % 3) as f64 - 1.0)
                    }
                    _ => { // cluster
                        (40.0 + ((seed >> 48) as u16 % 10) as f64,
                         ((seed >> 32) as u16 % 5) as f64 - 2.0,
                         ((seed >> 16) as u16 % 5) as f64 - 2.0)
                    }
                };
                let lab = CieLab { L: l, a, b };
                let clab = c::CieLab { L: l, a, b };
                assert_eq!(
                    c::cmsGDBAddPoint(cg, &clab as *const _ as *const c_void) != 0,
                    rg.add_point(&lab),
                    "add shape{shape} pt{i}");
            }
            // compute both
            assert_eq!(
                c::cmsGDBCompute(cg, 0) != 0,
                rg.compute(0),
                "compute shape{shape}");
            // checkpoint parity post-compute
            for i in 0..100 {
                seed = seed.wrapping_mul(0x9e3779b97f4a7c15) ^ (seed >> 33);
                let lab = CieLab {
                    L: ((seed >> 48) as u16 % 100) as f64,
                    a: ((seed >> 32) as u16 % 200) as f64 - 100.0,
                    b: ((seed >> 16) as u16 % 200) as f64 - 100.0,
                };
                let clab = c::CieLab { L: lab.L, a: lab.a, b: lab.b };
                assert_eq!(
                    c::cmsGDBCheckPoint(cg, &clab as *const _ as *const c_void) != 0,
                    rg.check_point(&lab),
                    "check shape{shape} pt{i}");
            }
            c::cmsGBDFree(cg);
        }
    }
}

#[test]
fn is_clut_matrix() {
    for data in corpus() {
        let rp = profile::open_profile_from_mem(None, &data).unwrap();
        for intent in 0..4u32 {
            for dir in 0..3u32 {
                let c_ok = unsafe {
                    let h = c::cmsOpenProfileFromMem(data.as_ptr() as _, data.len() as u32);
                    let r = c::cmsIsCLUT(h, intent, dir);
                    c::cmsCloseProfile(h);
                    r
                };
                let r_ok = lcms::io1::is_clut(&rp, intent, dir as i32);
                assert_eq!(c_ok != 0, r_ok, "is_clut i{intent} d{dir}");
            }
        }
    }
}
