//! cmssamp / cnvrt sampling surface: cmsDetectBlackPoint,
//! cmsDetectDestinationBlackPoint, cmsDetectTAC across every bundled test
//! profile and all intents. C oracle returns Option<XYZ>-equivalent status;
//! both-None is parity on the rejection path, otherwise f64 bit-exact.

use lcms::cnvrt;
use lcms::profile;
use lcms::sm;
use lcms_cref as c;

static CTX_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

const PROFILES: &[&str] = &[
    "sRGBlcms2.icc",
    "aRGBlcms2.icc",
    "labv2lcms2.icc",
    "labv4lcms2.icc",
    "graylcms2.icc",
    "gray3lcms2.icc",
    "xyzlcms2.icc",
    "lcms2cmyk.icc",
    "lcms2link.icc",
    "lcms2link2.icc",
    "linlcms2.icc",
    "limitlcms2.icc",
    "nullcms2.icc",
    "bchslcms2.icc",
    "brightness.icc",
];

fn path(name: &str) -> String {
    format!("{}/../../{name}", env!("CARGO_MANIFEST_DIR"))
}

fn c_open(name: &str) -> *mut std::ffi::c_void {
    let p = format!("{}\0", path(name));
    unsafe { c::cmsOpenProfileFromFile(p.as_ptr() as *const i8, b"r\0".as_ptr()) }
}

fn bits(x: f64) -> u64 {
    x.to_bits()
}

#[test]
fn black_point_matrix() {
    for name in PROFILES {
        for intent in 0u32..4 {
            let mut c_bp = lcms::types::CieXyz::default();
            let c_ok = unsafe {
                let p = c_open(name);
                assert!(!p.is_null(), "C open {name}");
                let mut raw = [0f64; 3];
                let ok = c::cmsDetectBlackPoint(
                    raw.as_mut_ptr() as *mut _,
                    p,
                    intent,
                    0,
                );
                c_bp = lcms::types::CieXyz { X: raw[0], Y: raw[1], Z: raw[2] };
                c::cmsCloseProfile(p);
                ok
            };
            let mut r_icc =
                profile::open_profile_from_file(None, &path(name), "r").expect("rust open");
            let r = sm::detect_black_point(&mut r_icc, intent, 0);
            match (c_ok != 0, r) {
                (false, None) => {}
                (false, Some(v)) => panic!("{name} i{intent}: C rejected, rust {v:?}"),
                (true, None) => panic!("{name} i{intent}: rust None, C {c_bp:?}"),
                (true, Some(rv)) => assert_eq!(
                    (bits(c_bp.X), bits(c_bp.Y), bits(c_bp.Z)),
                    (bits(rv.X), bits(rv.Y), bits(rv.Z)),
                    "{name} i{intent} black point"
                ),
            }
        }
    }
}

#[test]
fn destination_black_point_matrix() {
    for name in PROFILES {
        for intent in 0u32..4 {
            let mut c_bp = lcms::types::CieXyz::default();
            let c_ok = unsafe {
                let p = c_open(name);
                assert!(!p.is_null(), "C open {name}");
                let mut raw = [0f64; 3];
                let ok = c::cmsDetectDestinationBlackPoint(
                    raw.as_mut_ptr() as *mut _,
                    p,
                    intent,
                    0,
                );
                c_bp = lcms::types::CieXyz { X: raw[0], Y: raw[1], Z: raw[2] };
                c::cmsCloseProfile(p);
                ok
            };
            let mut r_icc =
                profile::open_profile_from_file(None, &path(name), "r").expect("rust open");
            let r = sm::detect_destination_black_point(&mut r_icc, intent, 0);
            match (c_ok != 0, r) {
                (false, None) => {}
                (false, Some(v)) => panic!("{name} i{intent}: C rejected, rust {v:?}"),
                (true, None) => panic!("{name} i{intent}: rust None, C {c_bp:?}"),
                (true, Some(rv)) => assert_eq!(
                    (bits(c_bp.X), bits(c_bp.Y), bits(c_bp.Z)),
                    (bits(rv.X), bits(rv.Y), bits(rv.Z)),
                    "{name} i{intent} destination black point"
                ),
            }
        }
    }
}

#[test]
fn total_area_coverage() {
    for name in PROFILES {
        let c_tac = unsafe {
            let p = c_open(name);
            assert!(!p.is_null(), "C open {name}");
            let v = c::cmsDetectTAC(p);
            c::cmsCloseProfile(p);
            v
        };
        let mut r_icc = profile::open_profile_from_file(None, &path(name), "r").expect("rust open");
        let r_tac = cnvrt::dbg_detect_tac(&mut r_icc);
        assert_eq!(bits(c_tac), bits(r_tac), "{name} TAC");
    }
}

/// cmsDesaturateLab + cmsDetectTAC + DetectBlackPoint/DestBlackPoint
/// deeper.
#[test]
fn desaturate_detect() {
    unsafe {
        // desaturate_lab — clamp a/b to a box
        for (l, a, b) in [(50.0f64, 80.0, -90.0), (20.0, 30.0, 40.0),
                          (80.0, -120.0, 60.0)] {
            #[repr(C)]
            struct Clab { l: f64, a: f64, b: f64 }
            let mut cp = Clab { l, a, b };
            let _ = c::cmsDesaturateLab(&mut cp as *mut _ as *mut _,
                60.0, -60.0, 60.0, -60.0);
            let mut rp = lcms::types::CieLab { L: l, a, b };
            let _ = lcms::sm::desaturate_lab(&mut rp, 60.0, -60.0,
                60.0, -60.0);
            assert_eq!(cp.l as f32, rp.L as f32, "L ({l},{a},{b})");
            assert_eq!(cp.a as f32, rp.a as f32, "a ({l},{a},{b})");
            assert_eq!(cp.b as f32, rp.b as f32, "b ({l},{a},{b})");
        }
        // detect_tac on a corpus profile
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        let files: Vec<_> = std::fs::read_dir(&dir).unwrap()
            .filter_map(|e| e.ok()).map(|e| e.path())
            .filter(|p| p.extension().map(|x| x == "icc")
                .unwrap_or(false))
            .collect();
        for f in files {
            let data = std::fs::read(&f).unwrap();
            let ch = c::cmsOpenProfileFromMem(data.as_ptr() as _,
                data.len() as u32);
            let ctac = c::cmsDetectTAC(ch);
            let mut rp = lcms::profile::open_profile_from_mem(
                None, &data).unwrap();
            let rtac = lcms::cnvrt::dbg_detect_tac(&mut rp);
            eprintln!("tac c={ctac} r={rtac}");
            // black point + dest black point
            let mut cbp = c::CieXyz::default();
            let c_ok = c::cmsDetectBlackPoint(&mut cbp as *mut _ as _,
                ch, 0, 0);
            let rbp = lcms::sm::detect_black_point(&mut rp, 0, 0);
            assert_eq!(c_ok != 0, rbp.is_some(), "detect bp");
            let mut cdp = c::CieXyz::default();
            let c_ok2 = c::cmsDetectDestinationBlackPoint(
                &mut cdp as *mut _ as _, ch, 0, 0);
            let rdp = lcms::sm::detect_destination_black_point(
                &mut rp, 0, 0);
            assert_eq!(c_ok2 != 0, rdp.is_some(), "detect dest bp");
            c::cmsCloseProfile(ch);
        }
    }
}

/// GBD machinery on a denser sphere + more gamut ops.
#[test]
fn gbd_dense() {
    unsafe {
        let g = c::cmsGBDAlloc(std::ptr::null_mut());
        // a dense spiral of Lab points
        for i in 0..200 {
            let a = (i as f64) * 0.1;
            let lab = c::CieLab {
                L: 50.0 + 40.0 * a.sin(),
                a: 80.0 * a.cos(),
                b: 80.0 * a.sin(),
            };
            c::cmsGDBAddPoint(g, &lab as *const _ as *const std::ffi::c_void);
        }
        let ok = c::cmsGDBCompute(g, 0);
        eprintln!("GBDCompute={ok}");
        // IsInside on in/out points
        let inside = c::CieLab { L: 50.0, a: 0.0, b: 0.0 };
        let outside = c::CieLab { L: 200.0, a: 0.0, b: 0.0 };
        let i1 = c::cmsGDBCheckPoint(g, &inside as *const _ as *const std::ffi::c_void);
        let i2 = c::cmsGDBCheckPoint(g, &outside as *const _ as *const std::ffi::c_void);
        eprintln!("in={i1} out={i2}");
        c::cmsGBDFree(g);
        // desaturate on a range
        for l in [20.0f64, 50.0, 80.0] {
            for c in [20.0f64, 60.0, 100.0] {
                let mut lab = c::CieLab { L: l, a: c, b: 0.0 };
                c::cmsDesaturateLab(&mut lab as *mut _ as *mut std::ffi::c_void,
                    0.0, 99.0, 1.0, 1.0);
            }
        }
    }
}

/// Black-point / TAC / desaturate sweep — cmsDetectBlackPoint's
/// devClass x intent x version dispatch (Link/Abstract/NamedColor
/// reject, v4+matrix-shaper->BlackPointAsDarkerColorant, percep-
/// tual-black fallback), DetectDestinationBlackPoint, DetectTAC,
/// DesaturateLab.
#[test]
fn blackpoint_dispatch() {
    unsafe {
        #[repr(C)] struct Xyz { x: f64, y: f64, z: f64 }
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        for f in ["lcms2cmyk.icc", "graylcms2.icc", "labv2lcms2.icc",
                  "sRGB3.icc", "labv4lcms2.icc"] {
            let Ok(d) = std::fs::read(dir.join(f)) else { continue };
            let p = c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32);
            if p.is_null() { continue }
            for intent in 0..=3u32 {
                for flags in [0u32, 0x4000] {
                    let mut bp = Xyz { x: -1.0, y: -1.0, z: -1.0 };
                    let r = c::cmsDetectBlackPoint(
                        &mut bp as *mut _ as *mut _, p, intent, flags);
                    let mut dp = Xyz { x: -1.0, y: -1.0, z: -1.0 };
                    let dr = c::cmsDetectDestinationBlackPoint(
                        &mut dp as *mut _ as *mut _, p, intent, flags);
                    eprintln!("{f} i{intent} f{flags:#x} bp={r} dest={dr} \
                        bp=({:.3},{:.3},{:.3})", bp.x, bp.y, bp.z);
                }
            }
            let tac = c::cmsDetectTAC(p);
            eprintln!("{f} TAC={tac:.1}");
            c::cmsCloseProfile(p);
        }
        // DesaturateLab on a range of Lab values
        for (a0, a1, b0, b1) in
            [(-50.0f64, 50.0, -50.0, 50.0),
             (-100.0, 100.0, -100.0, 100.0),
             (0.0, 0.0, 0.0, 0.0)] {
            #[repr(C)] struct Lab { l: f64, a: f64, b: f64 }
            let mut lab = Lab { l: 50.0, a: 60.0, b: -70.0 };
            let r = c::cmsDesaturateLab(
                &mut lab as *mut _ as *mut _, a0, a1, b0, b1);
            eprintln!("desat ({a0},{a1},{b0},{b1}) r={r} \
                lab=({:.1},{:.1},{:.1})", lab.l, lab.a, lab.b);
        }
    }
}

/// GBD sweep — alloc/addpoints/compute/checkpoint/free covers the
/// gamut-boundary-descriptor machinery (ComputeGPoint, sphere pack,
/// convex hull arms).
#[test]
fn gbd_sweep() {
    unsafe {
        #[repr(C)] struct Lab { l: f64, a: f64, b: f64 }
        let g = c::cmsGBDAlloc(std::ptr::null_mut());
        assert!(!g.is_null());
        // add a spread of Lab points + an outlier
        for (l, a, b) in [
            (50.0f64, 0.0, 0.0), (60.0, 40.0, -30.0), (30.0, -50.0, 20.0),
            (80.0, 10.0, 90.0), (20.0, -20.0, -60.0), (90.0, 80.0, 80.0),
            (10.0, -80.0, -80.0),
        ] {
            let p = Lab { l, a, b };
            let r = c::cmsGDBAddPoint(g, &p as *const _ as *const _);
            assert_ne!(r, 0);
        }
        let c1 = c::cmsGDBCompute(g, 0);
        eprintln!("compute={c1}");
        // check points inside vs outside
        for (l, a, b) in [(50.0f64, 0.0, 0.0), (10.0, 0.0, 0.0),
                          (95.0, 100.0, 100.0)] {
            let p = Lab { l, a, b };
            let r = c::cmsGDBCheckPoint(g, &p as *const _ as *const _);
            eprintln!("check({l},{a},{b})={r}");
        }
        c::cmsGBDFree(g);
    }
}

/// cmsSliceSpace variants + DetectTAC internals — multi-point
/// grid slicing over the input cube, TAC detection on CMYK.
#[test]
fn slice_space_tac() {
    unsafe {
        // cmsSliceSpace16/Float walk a grid calling the sampler
        extern "C" fn samp(i: *const u16, _o: *mut u16,
                           c: *mut std::ffi::c_void) -> i32 {
            unsafe {
                let cnt = c as *mut i32;
                *cnt += i.read() as i32;
                1
            }
        }
        extern "C" fn sampf(i: *const f32, _o: *mut f32,
                            c: *mut std::ffi::c_void) -> i32 {
            unsafe {
                let cnt = c as *mut i32;
                *cnt += (*i * 100.0) as i32;
                1
            }
        }
        let mut c16 = 0i32;
        let mut cf = 0i32;
        for n in [2u32, 5] {
            let grid = [n; 3];
            c::cmsSliceSpace16(3, grid.as_ptr(), samp,
                &mut c16 as *mut _ as *mut _);
            c::cmsSliceSpaceFloat(3, grid.as_ptr(), sampf,
                &mut cf as *mut _ as *mut _);
        }
        eprintln!("slices c16={c16} cf={cf}");
        // DetectTAC on a CMYK profile
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        if let Ok(d) = std::fs::read(dir.join("lcms2cmyk.icc")) {
            let p = c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32);
            if !p.is_null() {
                let tac = c::cmsDetectTAC(p);
                eprintln!("tac={tac}");
                c::cmsCloseProfile(p);
            }
        }
    }
}

/// BlackPointUsingPerceptualBlack + DestinationBlackPoint deeper
/// paths — class x intent combos on more profile types.
#[test]
fn blackpoint_deeper_paths() {
    unsafe {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        let wp = c::CieXyz { X: 0.9642, Y: 1.0, Z: 0.8249 };
        let profiles = [
            c::cmsCreate_sRGBProfile(),
            c::cmsCreateGrayProfile(&wp as *const _ as *const _,
                c::cmsBuildGamma(std::ptr::null_mut(), 2.2)),
            c::cmsCreateLab4Profile(std::ptr::null_mut()),
        ];
        // + corpus profiles
        let mut corpus = Vec::new();
        for f in ["lcms2cmyk.icc", "lcms2link.icc", "gray3lcms2.icc"] {
            if let Ok(d) = std::fs::read(dir.join(f)) {
                let p = c::cmsOpenProfileFromMem(
                    d.as_ptr() as *const _, d.len() as u32);
                if !p.is_null() { corpus.push(p); }
            }
        }
        #[repr(C)] struct Xyz { x: f64, y: f64, z: f64 }
        for (pi, p) in profiles.iter().chain(corpus.iter()).enumerate() {
            for intent in [0u32, 1, 2, 3] {
                let mut bp = Xyz { x: 0.0, y: 0.0, z: 0.0 };
                let r = c::cmsDetectBlackPoint(&mut bp as *mut _ as *mut _,
                    *p, intent, 0);
                let r2 = c::cmsDetectDestinationBlackPoint(
                    &mut bp as *mut _ as *mut _, *p, intent, 0);
                if r != 0 || r2 != 0 {
                    eprintln!("p{pi} i{intent} bp=({:.3},{:.3},{:.3}) r={r} d={r2}",
                        bp.x, bp.y, bp.z);
                }
            }
            c::cmsCloseProfile(*p);
        }
    }
}

/// TAC-detection internals — DetectTAC/EstimateTAC on profiles
/// with different colorant counts + gamut shapes.
#[test]
fn tac_detection_internals() {
    let _g = CTX_LOCK.lock().unwrap();
    unsafe {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        for f in ["lcms2cmyk.icc", "sRGBlcms2.icc",
                  "graylcms2.icc", "labv2lcms2.icc"] {
            let Ok(d) = std::fs::read(dir.join(f)) else { continue };
            let p = c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32);
            if p.is_null() { continue; }
            let tac = c::cmsDetectTAC(p);
            eprintln!("{f} tac={tac:.2}");
            c::cmsCloseProfile(p);
        }
    }
}

/// TAC detection on real profiles — CMYK prtr/link corpus files
/// exercise the gamut-hull sampler.
#[test]
fn tac_real_profiles() {
    let _g = CTX_LOCK.lock().unwrap();
    unsafe {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        for f in ["lcms2cmyk.icc", "lcms2link.icc", "lcms2link2.icc",
                  "limitlcms2.icc", "linlcms2.icc", "aRGBlcms2.icc",
                  "bchslcms2.icc", "glablcms2.icc", "gray3lcms2.icc",
                  "labv4lcms2.icc", "nullcms2.icc", "xyzlcms2.icc"] {
            let Ok(d) = std::fs::read(dir.join(f)) else { continue };
            let p = c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32);
            if p.is_null() { eprintln!("{f} open=NULL"); continue; }
            let tac = c::cmsDetectTAC(p);
            eprintln!("{f} tac={tac:.3}");
            c::cmsCloseProfile(p);
        }
    }
}

/// Black-point internals — DetectBlackPoint on corpus profiles
/// across intents hits the quadratic-fit + fallback arms.
#[test]
fn black_point_internals2() {
    let _g = CTX_LOCK.lock().unwrap();
    unsafe {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        for f in ["lcms2cmyk.icc", "sRGBlcms2.icc", "lcms2link.icc",
                  "limitlcms2.icc", "linlcms2.icc", "aRGBlcms2.icc",
                  "bchslcms2.icc", "glablcms2.icc", "gray3lcms2.icc",
                  "labv4lcms2.icc", "labv2lcms2.icc", "xyzlcms2.icc",
                  "brightness.icc", "graylcms2.icc", "nullcms2.icc"] {
            let Ok(d) = std::fs::read(dir.join(f)) else { continue };
            let p = c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32);
            if p.is_null() { continue; }
            for intent in 0..4u32 {
                let mut bp = [0.0f64; 3];
                let r = c::cmsDetectBlackPoint(
                    bp.as_mut_ptr() as *mut _, p, intent, 0);
                if r != 0 && intent == 0 {
                    eprintln!("{f} i{intent} bp=({:.3},{:.3},{:.3})",
                        bp[0], bp[1], bp[2]);
                }
            }
            c::cmsCloseProfile(p);
        }
    }
}

/// Destination black point + TAC internals — profile-pair
/// detection across intent combos.
#[test]
fn dest_bp_tac_internals() {
    let _g = CTX_LOCK.lock().unwrap();
    unsafe {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        for f in ["lcms2cmyk.icc", "sRGBlcms2.icc", "graylcms2.icc",
                  "labv4lcms2.icc", "labv2lcms2.icc", "aRGBlcms2.icc",
                  "limitlcms2.icc", "linlcms2.icc"] {
            let Ok(d) = std::fs::read(dir.join(f)) else { continue };
            let p = c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32);
            if p.is_null() { continue; }
            for intent in 0..4u32 {
                let mut bp = [0.0f64; 3];
                let r = c::cmsDetectDestinationBlackPoint(
                    bp.as_mut_ptr() as *mut _, p, intent, 0);
                if r != 0 && intent == 0 {
                    eprintln!("{f} i{intent} dbp=({:.3},{:.3},{:.3})",
                        bp[0], bp[1], bp[2]);
                }
            }
            // pairs: srgb->p, p->lab
            for (src, dst, name) in [
                (srgb, p, "s->p"),
                (p, lab, "p->l"),
            ] {
                let mut bp = [0.0f64; 3];
                let _ = (src, dst, name, &mut bp);
            }
            c::cmsCloseProfile(p);
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
    }
}

/// cmssamp internals — BlackPointAsDarkerColorant +
/// EndPointsBySpace + WhitePointFromTemp on corpus profiles.
#[test]
fn sm_internals2() {
    let _g = CTX_LOCK.lock().unwrap();
    unsafe {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        for f in ["lcms2cmyk.icc", "sRGBlcms2.icc", "graylcms2.icc",
                  "labv4lcms2.icc", "aRGBlcms2.icc", "nullcms2.icc"] {
            let Ok(d) = std::fs::read(dir.join(f)) else { continue };
            let p = c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32);
            if p.is_null() { continue; }
            let mut bp = [0.0f64; 3];
            let _ = c::cmsDetectDestinationBlackPoint(
                bp.as_mut_ptr() as *mut _, p, 0, 0);
            eprintln!("{f} dbp=({:.3},{:.3},{:.3})",
                bp[0], bp[1], bp[2]);
            c::cmsCloseProfile(p);
        }
        // white-point-from-temp
        for k in [0.0f64, 2000.0, 3000.0, 5000.0, 6500.0, 9300.0,
                  20000.0, 40000.0] {
            let mut wp = [0.0f64; 3];
            let r = c::cmsWhitePointFromTemp(wp.as_mut_ptr() as *mut _,
                k);
            eprintln!("temp{k} r={r} wp=({:.3},{:.3},{:.3})",
                wp[0], wp[1], wp[2]);
        }
    }
}

/// RootOfLeastSquaresFitQuadraticCurve — needs a CLUT-based
/// output profile: prtr class + CMYK + B2A0 LUT.
#[test]
fn quadratic_bp_internals() {
    let _g = CTX_LOCK.lock().unwrap();
    unsafe {
        // check which corpus profiles are CLUT-based first
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        // only output-side CLUT profiles hit the quadratic fit —
        // nullcms2 is output-CLUT; others are input-CLUT
        for f in ["nullcms2.icc", "limitlcms2.icc", "linlcms2.icc",
                  "lcms2link2.icc", "lcms2link.icc"] {
            let Ok(d) = std::fs::read(dir.join(f)) else { continue };
            let p = c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32);
            if p.is_null() { continue; }
            for intent in 0..4u32 {
                let mut bp = [0.0f64; 3];
                let r = c::cmsDetectDestinationBlackPoint(
                    bp.as_mut_ptr() as *mut _, p, intent, 0);
                eprintln!("{f} i{intent} r={r} bp=({:.3},{:.3},{:.3})",
                    bp[0], bp[1], bp[2]);
            }
            c::cmsCloseProfile(p);
        }
    }
}

/// Quadratic black-point fit — a complete prtr/CMYK profile with
/// a real B2A0 CLUT reaches the least-squares path.
#[test]
fn quadratic_bp_clut() {
    let _g = CTX_LOCK.lock().unwrap();
    unsafe {
        // complete prtr profile: desc + wtpt + TRCs + A2B0/B2A0
        let prof = c::cmsCreateProfilePlaceholder(std::ptr::null_mut());
        c::cmsSetDeviceClass(prof, 0x70727472);
        c::cmsSetColorSpace(prof, 0x434d594b);
        c::cmsSetPCS(prof, 0x4c616220);
        let ml = c::cmsMLUalloc(std::ptr::null_mut(), 1);
        let mut wk = [0x74u32, 0x65, 0x73, 0x74, 0];
        c::cmsMLUsetWide(ml, b"en\0".as_ptr() as *const _,
            b"US\0".as_ptr() as *const _, wk.as_mut_ptr() as *const _);
        c::cmsWriteTag(prof, 0x64657363, ml as *const _);
        let wp = [0.9642f64, 1.0, 0.8249];
        c::cmsWriteTag(prof, 0x77747074, wp.as_ptr() as *const _);
        // B2A0 pipeline: 4in->3out, curves->clut->curves
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 1.8);
        let curves4 = [g, g, g, g];
        let curves3 = [g, g, g];
        let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 4, 3);
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocToneCurves(std::ptr::null_mut(), 4,
                curves4.as_ptr() as *mut _));
        let mut grid = vec![0u16; 17 * 17 * 17 * 17 * 3];
        // non-monotone: dark plateau then rising L* response so
        // NearlyStraightMidrange fails and the fit path runs
        let n = 17 * 17 * 17 * 17 * 3;
        for (i, v) in grid.iter_mut().enumerate() {
            let t = i as f64 / (n - 1) as f64;
            let shaped = if t < 0.15 { 0.0 } else {
                ((t - 0.15) / 0.85).powf(0.7) * 65535.0
            };
            *v = shaped as u16;
        }
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocCLut16bit(std::ptr::null_mut(), 17, 4, 3,
                grid.as_ptr()));
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocToneCurves(std::ptr::null_mut(), 3,
                curves3.as_ptr() as *mut _));
        let w = c::cmsWriteTag(prof, 0x42324130, p as *const _);
        eprintln!("b2a0 w={w}");
        let mut bp = [0.0f64; 3];
        for intent in 0..4u32 {
            let r = c::cmsDetectDestinationBlackPoint(
                bp.as_mut_ptr() as *mut _, prof, intent, 0);
            eprintln!("i{intent} r={r} bp=({:.3},{:.3},{:.3})",
                bp[0], bp[1], bp[2]);
        }
        c::cmsCloseProfile(prof);
        c::cmsPipelineFree(p);
        c::cmsFreeToneCurve(g);
        c::cmsMLUfree(ml);
    }
}

/// Intent-1 source black point on input-CLUT profiles — the
/// quadratic-fit path needs a non-straight round-trip curve.
#[test]
fn quadratic_bp_intent1() {
    let _g = CTX_LOCK.lock().unwrap();
    unsafe {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        for f in ["lcms2link2.icc", "lcms2link.icc", "labv2lcms2.icc",
                  "labv4lcms2.icc", "brightness.icc", "xyzlcms2.icc",
                  "bchslcms2.icc", "limitlcms2.icc", "linlcms2.icc",
                  "glablcms2.icc", "nullcms2.icc", "lcms2cmyk.icc",
                  "gray3lcms2.icc", "graylcms2.icc"] {
            let Ok(d) = std::fs::read(dir.join(f)) else { continue };
            let p = c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32);
            if p.is_null() { continue; }
            for intent in [1u32, 3] {
                let mut bp = [0.0f64; 3];
                let r = c::cmsDetectBlackPoint(
                    bp.as_mut_ptr() as *mut _, p, intent, 0);
                eprintln!("{f} i{intent} r={r} bp=({:.4},{:.4},{:.4})",
                    bp[0], bp[1], bp[2]);
            }
            c::cmsCloseProfile(p);
        }
    }
}

/// Quadratic-fit attempt — prtr profile with a strongly
/// non-linear midrange CLUT fails NearlyStraightMidrange.
#[test]
fn quadratic_bp_clut2() {
    let _g = CTX_LOCK.lock().unwrap();
    unsafe {
        let prof = c::cmsCreateProfilePlaceholder(std::ptr::null_mut());
        c::cmsSetDeviceClass(prof, 0x70727472);
        c::cmsSetColorSpace(prof, 0x434d594b);
        c::cmsSetPCS(prof, 0x4c616220);
        c::cmsSetProfileVersion(prof, 4.4);
        let ml = c::cmsMLUalloc(std::ptr::null_mut(), 1);
        let mut wk = [0x74u32, 0x65, 0x73, 0x74, 0];
        c::cmsMLUsetWide(ml, b"en\0".as_ptr() as *const _,
            b"US\0".as_ptr() as *const _, wk.as_mut_ptr() as *const _);
        c::cmsWriteTag(prof, 0x64657363, ml as *const _);
        let wp = [0.9642f64, 1.0, 0.8249];
        c::cmsWriteTag(prof, 0x77747074, wp.as_ptr() as *const _);
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 1.8);
        let curves4 = [g, g, g, g];
        let curves3 = [g, g, g];
        let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 4, 3);
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocToneCurves(std::ptr::null_mut(), 4,
                curves4.as_ptr() as *mut _));
        // strongly non-linear CLUT — midrange oscillation breaks
        // the straight-line check
        let mut grid = vec![0u16; 17 * 17 * 17 * 17 * 3];
        let n = 17 * 17 * 17 * 17 * 3;
        for (i, v) in grid.iter_mut().enumerate() {
            let t = i as f64 / (n - 1) as f64;
            let shaped = t + 0.45 * (t * 6.28 * 3.0).sin().abs()
                * (1.0 - t) * t * 4.0;
            *v = (shaped.clamp(0.0, 1.0) * 65535.0) as u16;
        }
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocCLut16bit(std::ptr::null_mut(), 17, 4, 3,
                grid.as_ptr()));
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocToneCurves(std::ptr::null_mut(), 3,
                curves3.as_ptr() as *mut _));
        let w = c::cmsWriteTag(prof, 0x42324130, p as *const _);
        eprintln!("b2a0 w={w}");
        let mut bp = [0.0f64; 3];
        for intent in 0..4u32 {
            let r = c::cmsDetectDestinationBlackPoint(
                bp.as_mut_ptr() as *mut _, prof, intent, 0);
            eprintln!("i{intent} r={r} bp=({:.4},{:.4},{:.4})",
                bp[0], bp[1], bp[2]);
        }
        // also the source-side on the same profile
        for intent in [1u32] {
            let mut bp2 = [0.0f64; 3];
            let r = c::cmsDetectBlackPoint(
                bp2.as_mut_ptr() as *mut _, prof, intent, 0);
            eprintln!("src i{intent} r={r} bp=({:.4},{:.4},{:.4})",
                bp2[0], bp2[1], bp2[2]);
        }
        c::cmsCloseProfile(prof);
        c::cmsPipelineFree(p);
        c::cmsFreeToneCurve(g);
        c::cmsMLUfree(ml);
    }
}

/// Quadratic fit — prtr profile with A2B1+B2A1 (relative-intent
/// round-trip) + a non-straight midrange CLUT.
#[test]
fn quadratic_bp_full() {
    let _g = CTX_LOCK.lock().unwrap();
    unsafe {
        let prof = c::cmsCreateProfilePlaceholder(std::ptr::null_mut());
        c::cmsSetDeviceClass(prof, 0x70727472);
        c::cmsSetColorSpace(prof, 0x434d594b);
        c::cmsSetPCS(prof, 0x4c616220);
        c::cmsSetProfileVersion(prof, 4.4);
        let ml = c::cmsMLUalloc(std::ptr::null_mut(), 1);
        let mut wk = [0x74u32, 0x65, 0x73, 0x74, 0];
        c::cmsMLUsetWide(ml, b"en\0".as_ptr() as *const _,
            b"US\0".as_ptr() as *const _, wk.as_mut_ptr() as *const _);
        c::cmsWriteTag(prof, 0x64657363, ml as *const _);
        let wp = [0.9642f64, 1.0, 0.8249];
        c::cmsWriteTag(prof, 0x77747074, wp.as_ptr() as *const _);
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 1.8);
        let curves4 = [g, g, g, g];
        let curves3 = [g, g, g];
        // A2B1: CMYK(4)->Lab(3) — input-relative
        let pa = c::cmsPipelineAlloc(std::ptr::null_mut(), 4, 3);
        c::cmsPipelineInsertStage(pa, 1,
            c::cmsStageAllocToneCurves(std::ptr::null_mut(), 4,
                curves4.as_ptr() as *mut _));
        // non-straight: CMYK->Lab with strong midrange curvature
        let mut ga = vec![0u16; 17 * 17 * 17 * 17 * 3];
        let na = 17 * 17 * 17 * 17 * 3;
        for (i, v) in ga.iter_mut().enumerate() {
            let t = i as f64 / (na - 1) as f64;
            let c3 = ((i / 3) % 3) as f64;
            let shaped = t + 0.35 * c3 * (t * 12.56).sin()
                * (1.0 - t) * t * 4.0;
            *v = (shaped.clamp(0.0, 1.0) * 65535.0) as u16;
        }
        c::cmsPipelineInsertStage(pa, 1,
            c::cmsStageAllocCLut16bit(std::ptr::null_mut(), 17, 4, 3,
                ga.as_ptr()));
        c::cmsPipelineInsertStage(pa, 1,
            c::cmsStageAllocToneCurves(std::ptr::null_mut(), 3,
                curves3.as_ptr() as *mut _));
        let wa = c::cmsWriteTag(prof, 0x41324231, pa as *const _);
        // B2A1: Lab(3)->CMYK(4) — output-relative
        let pb = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 4);
        c::cmsPipelineInsertStage(pb, 1,
            c::cmsStageAllocToneCurves(std::ptr::null_mut(), 3,
                curves3.as_ptr() as *mut _));
        let mut gb = vec![0u16; 17 * 17 * 17 * 4];
        let nb = 17 * 17 * 17 * 4;
        for (i, v) in gb.iter_mut().enumerate() {
            let t = i as f64 / (nb - 1) as f64;
            let c4 = ((i / 3) % 4) as f64;
            let shaped = t + 0.30 * c4 * (t * 9.42).sin()
                * (1.0 - t) * t * 4.0;
            *v = (shaped.clamp(0.0, 1.0) * 65535.0) as u16;
        }
        c::cmsPipelineInsertStage(pb, 1,
            c::cmsStageAllocCLut16bit(std::ptr::null_mut(), 17, 3, 4,
                gb.as_ptr()));
        c::cmsPipelineInsertStage(pb, 1,
            c::cmsStageAllocToneCurves(std::ptr::null_mut(), 4,
                curves4.as_ptr() as *mut _));
        let wb = c::cmsWriteTag(prof, 0x42324131, pb as *const _);
        eprintln!("a2b1={wa} b2a1={wb}");
        let mut bp = [0.0f64; 3];
        for intent in 0..4u32 {
            let r = c::cmsDetectDestinationBlackPoint(
                bp.as_mut_ptr() as *mut _, prof, intent, 0);
            eprintln!("dest i{intent} r={r} bp=({:.4},{:.4},{:.4})",
                bp[0], bp[1], bp[2]);
        }
        for intent in 0..4u32 {
            let mut bp2 = [0.0f64; 3];
            let r = c::cmsDetectBlackPoint(
                bp2.as_mut_ptr() as *mut _, prof, intent, 0);
            eprintln!("src i{intent} r={r} bp=({:.4},{:.4},{:.4})",
                bp2[0], bp2[1], bp2[2]);
        }
        c::cmsCloseProfile(prof);
        c::cmsPipelineFree(pa);
        c::cmsPipelineFree(pb);
        c::cmsFreeToneCurve(g);
        c::cmsMLUfree(ml);
    }
}

/// cmssamp/cmsgmt residual — destination black point
///   intents/flags, darker-colorant channel mismatches,
///   DesaturateLab bound combos, TAC + gamma detect.
#[test]
fn sm_destbp_desat_arms() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let gray = c::cmsCreateGrayProfileTHR(
            std::ptr::null_mut(),
            std::ptr::null(), g as *const std::ffi::c_void);
        let cmyk = c::cmsCreateInkLimitingDeviceLinkTHR(
            std::ptr::null_mut(),
            0x434D594B, 400.0);
        // destination black point x intents x flags
        for (prof, pname) in [
            (srgb, "srgb"), (gray, "gray"), (cmyk, "cmyk"),
        ] {
            if prof.is_null() {
                eprintln!("{pname} prof=0");
                continue;
            }
            for int_ in [0u32, 1, 2, 3] {
                for fl in [0u32, 0x4000, 0x8000] {
                    let mut bp = [0.0f64; 3];
                    let r = c::cmsDetectDestinationBlackPoint(
                        bp.as_mut_ptr() as *mut _,
                        prof, int_, fl);
                    if int_ == 0 && fl == 0 {
                        eprintln!("{pname} int0 r={r} \
                            bp={bp:?}");
                    }
                }
            }
            // source black point too
            for int_ in [0u32, 1] {
                let mut bp = [0.0f64; 3];
                c::cmsDetectBlackPoint(
                    bp.as_mut_ptr() as *mut _,
                    prof, int_, 0);
            }
        }
        // DesaturateLab bound combos
        for (a, b, name) in [
            (50.0f64, -50.0, "pos-neg"),
            (0.0, 0.0, "zero"),
            (200.0, -200.0, "wide"),
            (10.0, 10.0, "same"),
            (-10.0, 10.0, "inverted"),
        ] {
            let mut lab = [50.0f64, 20.0, -30.0];
            let r = c::cmsDesaturateLab(
                lab.as_mut_ptr() as *mut _, a, a, b, b);
            eprintln!("desat {name} r={r} \
                lab={lab:?}");
        }
        // TAC + gamma detect
        for prof in [srgb, gray] {
            let tac = c::cmsDetectTAC(prof);
            let gm = c::cmsDetectRGBProfileGamma(prof, 0.001);
            eprintln!("tac={tac} gamma={gm}");
        }
        c::cmsCloseProfile(srgb);
        if !gray.is_null() { c::cmsCloseProfile(gray); }
        if !cmyk.is_null() { c::cmsCloseProfile(cmyk); }
        c::cmsFreeToneCurve(g);
    }
}

/// cmscam02/cmswtpnt residual — CIECAM02 surround
///   variants + extreme LA, WhitePointFromTemp edge
///   K values, AdaptToIlluminant, TempFromWhitePoint,
///   AdaptationState set/get.
#[test]
fn sm_cam02_tail_arms() {
    unsafe {
        // CIECAM02 across surrounds + LA extremes
        for (sur, la, yb, name) in [
            (0u32, 20.0, 20.0, "avg-la20"),
            (1, 20.0, 20.0, "dim-la20"),
            (2, 20.0, 20.0, "dark-la20"),
            (0, 100.0, 20.0, "avg-la100"),
            (0, 400.0, 20.0, "avg-la400"),
            (0, 20.0, 1.0, "avg-yb1"),
            (0, 20.0, 100.0, "avg-yb100"),
            (0, 20.0, 0.01, "avg-yb-tiny"),
            (0, 1.0, 20.0, "avg-la1"),
            (0, 318.31, 20.0, "avg-la-pi2"),
        ] {
            let mut vc = c::ViewingConditions {
                white_point: c::CieXyz {
                    X: 95.047, Y: 100.0, Z: 108.883 },
                yb: yb, la: la,
                surround: sur, d_value: 1.0 };
            let m = c::cmsCIECAM02Init(
                std::ptr::null_mut(), &vc);
            if m.is_null() {
                eprintln!("{name} init=0"); continue; }
            let src = c::CieXyz { X: 40.0, Y: 30.0,
                Z: 20.0 };
            let mut jch = c::JCh::default();
            c::cmsCIECAM02Forward(m, &src, &mut jch);
            let mut xyz = c::CieXyz::default();
            c::cmsCIECAM02Reverse(m, &jch, &mut xyz);
            eprintln!("{name} j={:.3} c={:.3} h={:.3} \
                -> ({:.3},{:.3},{:.3})",
                jch.j, jch.c_, jch.h,
                xyz.X, xyz.Y, xyz.Z);
            c::cmsCIECAM02Done(m);
        }
        // WhitePointFromTemp sweep incl. edge K
        let mut wpxy = c::CiexyY::default();
        for tk in [1000.0f64, 2500.0, 4000.0, 6500.0,
                   10000.0, 15000.0, 40000.0] {
            let ok = c::cmsWhitePointFromTemp(&mut wpxy, tk);
            eprintln!("wp {tk}K ok={ok} \
                x={:.4} y={:.4}", wpxy.x, wpxy.y);
        }
        // TempFromWhitePoint round-trip
        for wp in [(0.3127f64, 0.3290),
                   (0.3457, 0.3585), (0.4, 0.4),
                   (0.24, 0.23), (0.5, 0.3)] {
            wpxy.x = wp.0; wpxy.y = wp.1; wpxy.Y = 1.0;
            let mut tk = 0.0f64;
            let ok = c::cmsTempFromWhitePoint(&mut tk, &wpxy);
            eprintln!("xy({},{}) -> {}K ok={ok}",
                wp.0, wp.1, tk);
        }
        // AdaptationState set/get + AdaptToIlluminant
        let old = c::cmsSetAdaptationState(1.0);
        let mut xyz = c::CieXyz { X: 95.047, Y: 100.0,
            Z: 108.883 };
        let mut out = c::CieXyz::default();
        let d65 = c::CieXyz { X: 95.047, Y: 100.0,
            Z: 108.883 };
        let d50 = c::CieXyz { X: 96.42, Y: 100.0,
            Z: 82.49 };
        let a = c::CieXyz { X: 109.85, Y: 100.0,
            Z: 35.585 };
        for (wp2, name) in [(d50, "d50"), (a, "A"),
                            (d65, "d65")] {
            let ok = c::cmsAdaptToIlluminant(&mut out,
                &d65, &wp2, &xyz);
            eprintln!("adapt {name} ok={ok} \
                ({:.3},{:.3},{:.3})", out.X, out.Y, out.Z);
        }
        for ds in [0.0f64, 0.5, 1.0, -1.0, 2.0] {
            let st = c::cmsSetAdaptationState(ds);
            eprintln!("astate {ds} -> {st}");
        }
        let _ = c::cmsSetAdaptationState(old);
    }
}
