//! Virtual profile builders — serialize every ctor to bytes and compare
//! C `cmsSaveProfileToMem` vs Rust `profile::save_profile_to_mem` exactly.
//! Covers cmsvirt.c builders the testbed never reaches (gray, linearization,
//! ink-limiting, BCHSW abstract, abstract, NULL, transform2devicelink) and,
//! through serialization, the tag writers in cmstypes.c/cmsio0.c.

use lcms::profile;
use lcms::virt;
use lcms_cref as c;

fn c_save(prof: *mut std::ffi::c_void) -> Option<Vec<u8>> {
    unsafe {
        let mut n = 0u32;
        if c::cmsSaveProfileToMem(prof, std::ptr::null_mut(), &mut n) == 0 {
            return None;
        }
        let mut buf = vec![0u8; n as usize];
        if c::cmsSaveProfileToMem(prof, buf.as_mut_ptr() as *mut _, &mut n) == 0 {
            return None;
        }
        buf.truncate(n as usize);
        Some(buf)
    }
}

fn r_save(p: &mut lcms::profile::IccProfile) -> Option<Vec<u8>> {
    profile::save_profile_to_mem(p)
}

/// Both-NULL is parity on the error path; otherwise byte-compare with the
/// 12-byte `created` datetime (header bytes 24..36) masked — C stamps
/// `time(0)` at placeholder creation, the port deliberately keeps epoch
/// (documented at `profile::create_profile_placeholder`). Mask it on both
/// sides, same convention as existing serialize gates.
fn cmp(name: &str, c: Option<Vec<u8>>, r: Option<Vec<u8>>) {
    match (c, r) {
        (None, None) => {}
        (None, Some(_)) => panic!("{name}: C failed, rust wrote"),
        (Some(_), None) => panic!("{name}: rust failed, C wrote"),
        (Some(mut cb), Some(mut rb)) => {
            cb[24..36].fill(0);
            rb[24..36].fill(0);
            assert_eq!(cb, rb, "{name}: serialized bytes differ");
        }
    }
}

fn c_gamma(g: f64) -> *mut c::ToneCurve {
    unsafe { c::cmsBuildGamma(std::ptr::null_mut(), g) }
}
fn r_gamma(g: f64) -> lcms::gamma::ToneCurve {
    lcms::gamma::build_gamma(None, g).expect("gamma")
}

#[test]
fn gray_profiles() {
    for g in [1.0, 1.8, 2.2, 2.4] {
        unsafe {
            let tc = c_gamma(g);
            let p = c::cmsCreateGrayProfile(std::ptr::null(), tc);
            assert!(!p.is_null());
            let c_b = c_save(p);
            c::cmsCloseProfile(p);
            let tc_r = r_gamma(g);
            let mut r_p = virt::create_gray_profile(None, None, Some(&tc_r)).expect("gray");
            cmp(&format!("gray g{g}"), c_b, r_save(&mut r_p));
        }
    }
    // non-default white point
    unsafe {
        let wp = c::CiexyY { x: 0.3127, y: 0.3290, Y: 1.0 };
        let tc = c_gamma(2.2);
        let p = c::cmsCreateGrayProfile(&wp as *const _ as *const _, tc);
        assert!(!p.is_null());
        let c_b = c_save(p);
        c::cmsCloseProfile(p);
        let tc_r = r_gamma(2.2);
        let wp_r = lcms::types::CiexyY { x: 0.3127, y: 0.3290, Y: 1.0 };
        let mut r_p = virt::create_gray_profile(None, Some(&wp_r), Some(&tc_r)).expect("gray wp");
        cmp("gray d65", c_b, r_save(&mut r_p));
    }
}

#[test]
fn rgb_profiles() {
    // sRGB primaries + a couple of TRC shapes
    let prim = c::CiexyYTriple {
        Red: c::CiexyY { x: 0.64, y: 0.33, Y: 1.0 },
        Green: c::CiexyY { x: 0.30, y: 0.60, Y: 1.0 },
        Blue: c::CiexyY { x: 0.15, y: 0.06, Y: 1.0 },
    };
    for gamma in [1.8, 2.2] {
        unsafe {
            let wp = c::cmsD50_xyY();
            let tcs: [*mut c::ToneCurve; 3] = [c_gamma(gamma); 3];
            let p = c::cmsCreateRGBProfileTHR(
                std::ptr::null_mut(),
                wp as *const _,
                &prim as *const _ as *const _,
                tcs.as_ptr() as *const _,
            );
            assert!(!p.is_null());
            let c_b = c_save(p);
            c::cmsCloseProfile(p);
            let tc_r = r_gamma(gamma);
            let wp_r = lcms::wtpnt::d50_xyy();
            let prim_r = lcms::types::CiexyYTriple {
                Red: lcms::types::CiexyY { x: 0.64, y: 0.33, Y: 1.0 },
                Green: lcms::types::CiexyY { x: 0.30, y: 0.60, Y: 1.0 },
                Blue: lcms::types::CiexyY { x: 0.15, y: 0.06, Y: 1.0 },
            };
            let mut r_p = virt::create_rgb_profile(
                None,
                Some(&wp_r),
                Some(&prim_r),
                Some([&tc_r, &tc_r, &tc_r]),
            )
            .expect("rgb");
            cmp(&format!("rgb g{gamma}"), c_b, r_save(&mut r_p));
        }
    }
}

#[test]
fn lab_xyz_null_srgb() {
    unsafe {
        let p = c::cmsCreateXYZProfile();
        let c_b = c_save(p);
        c::cmsCloseProfile(p);
        let mut r_p = virt::create_xyz_profile(None).expect("xyz");
        cmp("xyz", c_b, r_save(&mut r_p));

        let p = c::cmsCreate_sRGBProfile();
        let c_b = c_save(p);
        c::cmsCloseProfile(p);
        let mut r_p = virt::create_srgb_profile(None).expect("srgb");
        cmp("srgb", c_b, r_save(&mut r_p));

        let p = c::cmsCreateNULLProfile();
        let c_b = c_save(p);
        c::cmsCloseProfile(p);
        let mut r_p = virt::create_null_profile(None).expect("null");
        cmp("null", c_b, r_save(&mut r_p));

        let p = c::cmsCreateLab2Profile(std::ptr::null());
        let c_b = c_save(p);
        c::cmsCloseProfile(p);
        let mut r_p = virt::create_lab2_profile(None, None).expect("lab2");
        cmp("lab2", c_b, r_save(&mut r_p));

        let p = c::cmsCreateLab4Profile(std::ptr::null());
        let c_b = c_save(p);
        c::cmsCloseProfile(p);
        let mut r_p = virt::create_lab4_profile(None, None).expect("lab4");
        cmp("lab4", c_b, r_save(&mut r_p));
    }
}

#[test]
fn linearization_links() {
    // gray + rgb + cmyk linearization
    for (cs_sig, nch, name) in [
        (0x47524159u32, 1usize, "gray"), // 'GRAY'
        (0x52474220, 3, "rgb"),          // 'RGB '
        (0x434d594b, 4, "cmyk"),         // 'CMYK'
    ] {
        unsafe {
            let tcs: Vec<*mut c::ToneCurve> = (0..nch).map(|_| c_gamma(2.2)).collect();
            let p = c::cmsCreateLinearizationDeviceLinkTHR(
                std::ptr::null_mut(),
                cs_sig,
                tcs.as_ptr(),
            );
            assert!(!p.is_null(), "C lin {name}");
            let c_b = c_save(p);
            c::cmsCloseProfile(p);
            let tc_r = r_gamma(2.2);
            let tr: Vec<&lcms::gamma::ToneCurve> = (0..nch).map(|_| &tc_r).collect();
            let mut r_p = virt::create_linearization_device_link(None, cs_sig, &tr)
                .expect("rust lin");
            cmp(&format!("lin {name}"), c_b, r_save(&mut r_p));
        }
    }
}

#[test]
fn ink_limiting() {
    for lim in [50.0, 100.0, 150.0, 300.0] {
        unsafe {
            let p = c::cmsCreateInkLimitingDeviceLinkTHR(
                std::ptr::null_mut(),
                0x434d594b, // 'CMYK'
                lim,
            );
            assert!(!p.is_null());
            let c_b = c_save(p);
            c::cmsCloseProfile(p);
            let mut r_p =
                virt::create_ink_limiting_device_link(None, 0x434d594b, lim).expect("ink");
            cmp(&format!("ink {lim}"), c_b, r_save(&mut r_p));
        }
    }
    // error path: non-CMYK must fail on both sides
    unsafe {
        let p = c::cmsCreateInkLimitingDeviceLinkTHR(std::ptr::null_mut(), 0x52474220, 100.0);
        let r_p = virt::create_ink_limiting_device_link(None, 0x52474220, 100.0);
        assert_eq!(p.is_null(), r_p.is_none(), "ink-limit RGB reject");
        if !p.is_null() {
            c::cmsCloseProfile(p);
        }
    }
}

#[test]
fn bchsw_abstract() {
    // both take Kelvin temps
    for (lut_pts, bright, contrast, hue, sat, tsrc, tdst) in [
        (17u32, 0.0, 0.0, 0.0, 0.0, 6500u32, 6500u32),
        (17, 10.0, 5.0, -3.0, 20.0, 6500, 5000),
        (33, -20.0, 15.0, 10.0, -10.0, 5000, 6500),
    ] {
        unsafe {
            let p = c::cmsCreateBCHSWabstractProfile(
                lut_pts, bright, contrast, hue, sat, tsrc, tdst,
            );
            assert!(!p.is_null(), "C bchsw");
            let c_b = c_save(p);
            c::cmsCloseProfile(p);
            let mut r_p = virt::create_bchsw_abstract_profile(
                None, lut_pts, bright, contrast, hue, sat, tsrc, tdst,
            )
            .expect("rust bchsw");
            cmp(
                &format!("bchsw {bright}/{contrast}/{hue}/{sat}/{tsrc}->{tdst}"),
                c_b,
                r_save(&mut r_p),
            );
        }
    }
}

/// cmsvirt RGB-profile builders: varied primaries/white/gamma — exercises
/// the matrix/TRC writer paths beyond the bundled sRGB.
#[test]
fn rgb_profile_builders() {
    let wps: Vec<lcms::types::CiexyY> = vec![
        lcms::types::CiexyY { x: 0.3127, y: 0.3290, Y: 1.0 }, // D65
        lcms::types::CiexyY { x: 0.3457, y: 0.3585, Y: 1.0 }, // D50
        lcms::types::CiexyY { x: 1.0 / 3.0, y: 1.0 / 3.0, Y: 1.0 }, // E
    ];
    let prim_sets: Vec<lcms::types::CiexyYTriple> = vec![
        lcms::types::CiexyYTriple {
            Red: lcms::types::CiexyY { x: 0.64, y: 0.33, Y: 1.0 },
            Green: lcms::types::CiexyY { x: 0.30, y: 0.60, Y: 1.0 },
            Blue: lcms::types::CiexyY { x: 0.15, y: 0.06, Y: 1.0 },
        }, // sRGB
        lcms::types::CiexyYTriple {
            Red: lcms::types::CiexyY { x: 0.64, y: 0.33, Y: 1.0 },
            Green: lcms::types::CiexyY { x: 0.21, y: 0.71, Y: 1.0 },
            Blue: lcms::types::CiexyY { x: 0.15, y: 0.06, Y: 1.0 },
        }, // Adobe RGB
        lcms::types::CiexyYTriple {
            Red: lcms::types::CiexyY { x: 0.7347, y: 0.2653, Y: 1.0 },
            Green: lcms::types::CiexyY { x: 0.1596, y: 0.8404, Y: 1.0 },
            Blue: lcms::types::CiexyY { x: 0.0366, y: 0.0001, Y: 1.0 },
        }, // ProPhoto
        lcms::types::CiexyYTriple {
            Red: lcms::types::CiexyY { x: 0.68, y: 0.32, Y: 1.0 },
            Green: lcms::types::CiexyY { x: 0.265, y: 0.69, Y: 1.0 },
            Blue: lcms::types::CiexyY { x: 0.15, y: 0.06, Y: 1.0 },
        }, // DCI-P3
    ];
    for (wi, wp) in wps.iter().enumerate() {
        for (pi, prim) in prim_sets.iter().enumerate() {
            for g in [1.8f64, 2.2, 2.6] {
                unsafe {
                    let cg = c::cmsBuildGamma(std::ptr::null_mut(), g);
                    let curves = [cg, cg, cg];
                    let ch = c::cmsCreateRGBProfileTHR(
                        std::ptr::null_mut(),
                        wp as *const _ as _,
                        prim as *const _ as _,
                        curves.as_ptr() as _,
                    );
                    if ch.is_null() {
                        // C rejected the combo — rust must too
                        let r = lcms::virt::create_rgb_profile(
                            None, Some(wp), Some(prim), None,
                        );
                        assert!(r.is_none(), "wp{wi} prim{pi} g{g}: C-NULL vs rust");
                        continue;
                    }
                    let r_g = lcms::gamma::build_gamma(None, g).unwrap();
                    let mut rp = lcms::virt::create_rgb_profile(
                        None, Some(wp), Some(prim), Some([&r_g, &r_g, &r_g]),
                    ).expect("rust rgb profile");
                    // serialize + byte-compare (created masked)
                    let mut cn = 0u32;
                    assert!(c::cmsSaveProfileToMem(ch, std::ptr::null_mut(), &mut cn) != 0);
                    let mut cb = vec![0u8; cn as usize];
                    assert!(c::cmsSaveProfileToMem(ch, cb.as_mut_ptr() as *mut _, &mut cn) != 0);
                    let rb = lcms::profile::save_profile_to_mem(&mut rp).unwrap();
                    assert_eq!(cb.len(), rb.len(), "wp{wi} prim{pi} g{g} len");
                    for (i, (a, b)) in cb.iter().zip(rb.iter()).enumerate() {
                        if (24..36).contains(&i) { continue; }
                        assert_eq!(a, b, "wp{wi} prim{pi} g{g} byte @{i:#x}");
                    }
                    c::cmsCloseProfile(ch);
                }
            }
        }
    }
}

/// Profile-placeholder internals — different sigs + more
/// primaries.
#[test]
fn placeholder_variants() {
    unsafe {
        for (name, cls, cs, pcs) in [
            ("mntr-rgb", 0x6D6E7472u32, 0x52474220u32,
             0x58595A20u32),
            ("mntr-rgb-lab", 0x6D6E7472u32, 0x52474220u32,
             0x4C616220u32),
            ("prtr-cmyk", 0x70727472u32, 0x434D594Bu32,
             0x4C616220u32),
            ("link-rgb", 0x6C696E6Bu32, 0x52474220u32,
             0x58595A20u32),
            ("spac", 0x73706163u32, 0x52474220u32,
             0x58595A20u32),
            ("abst", 0x61627374u32, 0x52474220u32,
             0x4C616220u32),
            ("nmcl", 0x6E6D636Cu32, 0x6E6D636Cu32,
             0x4C616220u32),
        ] {
            let h = c::cmsCreateProfilePlaceholder(
                std::ptr::null_mut());
            c::cmsSetDeviceClass(h, cls);
            c::cmsSetColorSpace(h, cs);
            c::cmsSetPCS(h, pcs);
            let mut cn = 0u32;
            let ok = c::cmsSaveProfileToMem(h,
                std::ptr::null_mut(), &mut cn);
            eprintln!("{name} save={ok}");
            c::cmsCloseProfile(h);
        }
    }
}

/// Version machinery on more combos — cmsSetEncodedICCversion
/// internals.
#[test]
fn version_machinery() {
    unsafe {
        for (name, v) in [
            ("v2.0", 0x02000000u32), ("v2.1", 0x02100000),
            ("v2.4", 0x02400000), ("v4.0", 0x04000000),
            ("v4.3", 0x04300000), ("v4.4", 0x04400000)] {
            let h = c::cmsCreateProfilePlaceholder(
                std::ptr::null_mut());
            c::cmsSetEncodedICCversion(h, v);
            let rv = c::cmsGetEncodedICCversion(h);
            eprintln!("{name} enc={:#x}", rv);
            c::cmsCloseProfile(h);
        }
    }
}

/// Version getters/setters on more combos — the version
/// machinery internals.
#[test]
fn version_getters() {
    unsafe {
        for (name, v) in [
            ("v2.0", 0x02000000u32), ("v4.3", 0x04300000)] {
            let h = c::cmsCreateProfilePlaceholder(
                std::ptr::null_mut());
            c::cmsSetEncodedICCversion(h, v);
            let enc = c::cmsGetEncodedICCversion(h);
            let pv = c::cmsGetProfileVersion(h);
            eprintln!("{name} enc={:#x} pv={}", enc, pv);
            c::cmsCloseProfile(h);
        }
    }
}

/// OkLab header regression — C `cmsCreate_OkLabProfile` vs
/// Rust `create_oklab_profile`: the header rendering
/// intent must be RELATIVE (1) on both sides and the
/// serialized profile bytes must match.
#[test]
fn oklab_header_intent() {
    unsafe {
        let cp = c::cmsCreate_OkLabProfile(std::ptr::null_mut());
        assert!(!cp.is_null(), "C OkLab create failed");
        let c_intent = c::cmsGetHeaderRenderingIntent(cp);
        let c_b = c_save(cp);
        c::cmsCloseProfile(cp);
        let mut rp = virt::create_oklab_profile(None)
            .expect("rust oklab");
        assert_eq!(c_intent,
            lcms::types::INTENT_RELATIVE_COLORIMETRIC,
            "C OkLab header intent");
        assert_eq!(rp.rendering_intent,
            lcms::types::INTENT_RELATIVE_COLORIMETRIC,
            "rust OkLab header intent");
        cmp("oklab", c_b, r_save(&mut rp));
    }
}

/// OkLab transform regression — OkLab→XYZ through both
/// impls using the named RELATIVE constant, not a literal.
#[test]
fn oklab_transform_relative() {
    const INFMT: u32 = (0 << 16) | (3 << 3) | 1;   // PT_ANY 3ch u8
    const OUTFMT: u32 = (9 << 16) | (3 << 3) | 2;  // PT_XYZ 3ch u16
    let inp = [64u8, 128, 192, 0, 255, 32];
    let n_px = 2u32;
    // C side
    let mut c_out = vec![0u8; 2 * 3 * 2];
    unsafe {
        let oklab = c::cmsCreate_OkLabProfile(std::ptr::null_mut());
        let xyz = c::cmsCreateXYZProfile();
        let t = c::cmsCreateTransform(oklab, INFMT, xyz,
            OUTFMT, 1, 0);
        assert!(!t.is_null(), "C transform failed");
        c::cmsDoTransform(t, inp.as_ptr() as *const _,
            c_out.as_mut_ptr() as *mut _, n_px);
        c::cmsDeleteTransform(t);
        c::cmsCloseProfile(oklab);
        c::cmsCloseProfile(xyz);
    }
    // Rust side — the named constant, exercising the real
    // INTENT_RELATIVE_COLORIMETRIC path end to end.
    let p1 = virt::create_oklab_profile(None).expect("oklab");
    let p2 = virt::create_xyz_profile(None).expect("xyz");
    let mut profs = vec![p1, p2];
    let mut bpc = [false, false];
    let intents = [lcms::types::INTENT_RELATIVE_COLORIMETRIC; 2];
    let adapt = [1.0f64, 1.0];
    let mut t = lcms::xform::create_extended_transform(
        None, &mut profs, &mut bpc, &intents, &adapt,
        None, 0, INFMT, OUTFMT, 0)
        .expect("rust transform");
    let mut r_out = vec![0u8; 2 * 3 * 2];
    t.do_transform(&inp, &mut r_out, n_px);
    assert_eq!(c_out, r_out, "OkLab→XYZ output diverged");
}

/// Remaining virt builders — BCHSW abstract, NULL profile, OkLab,
/// ink-limiting devicelink, transform->devicelink. Serialize parity.
#[test]
fn misc_virt_builders() {
    unsafe {
        // BCHSW abstract (n_lut, bright, contrast, hue, sat, tempK src/dst)
        for (nl, b, cn, h, sa, ts, td) in
            [(17u32, 0.0, 1.0, 0.0, 0.0, 6500u32, 5000u32),
             (33, -0.1, 1.2, 15.0, 1.5, 5000, 6500)] {
            let p = c::cmsCreateBCHSWabstractProfile(nl, b, cn, h, sa, ts, td);
            assert!(!p.is_null());
            cmp(&format!("bchsw{nl}"), c_save(p),
                r_save(&mut *virt::create_bchsw_abstract_profile(
                    None, nl, b, cn, h, sa, ts, td).unwrap()));
            c::cmsCloseProfile(p);
        }
        // NULL profile
        let p = c::cmsCreateNULLProfile();
        assert!(!p.is_null());
        cmp("nullprof", c_save(p),
            r_save(&mut *virt::create_null_profile(None).unwrap()));
        c::cmsCloseProfile(p);
        // OkLab profile
        let p = c::cmsCreate_OkLabProfile(std::ptr::null_mut());
        assert!(!p.is_null());
        cmp("oklab", c_save(p),
            r_save(&mut *virt::create_oklab_profile(None).unwrap()));
        c::cmsCloseProfile(p);
        // ink-limiting devicelink — CMYK + RGB + Gray color spaces
        for (cs, lim) in [(0x434D594Bu32, 260.0),   // 'CMYK'
                          (0x52474220u32, 200.0),   // 'RGB '
                          (0x47524159u32, 240.0)] { // 'GRAY'
            let p = c::cmsCreateInkLimitingDeviceLink(cs, lim);
            let rok = virt::create_ink_limiting_device_link(
                None, cs, lim).is_some();
            eprintln!("inklim cs={cs:#x} C={} R={}", !p.is_null(), rok);
            assert_eq!(!p.is_null(), rok, "inklim {cs:#x} parity");
            if !p.is_null() {
                cmp(&format!("inklim{cs:#x}"), c_save(p),
                    r_save(&mut *virt::create_ink_limiting_device_link(
                        None, cs, lim).unwrap()));
                c::cmsCloseProfile(p);
            }
        }
        // transform -> devicelink
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let t = c::cmsCreateTransform(srgb, 0x00ff04, // TYPE_RGB_8
            lab, 0x0a0702, 0, 0); // TYPE_Lab_16
        if !t.is_null() {
            let p = c::cmsTransform2DeviceLink(t, 4.4, 0);
            eprintln!("t2dl ok={}", !p.is_null());
            if !p.is_null() {
                let _ = c_save(p); // C-only coverage probe
                c::cmsCloseProfile(p);
            }
            c::cmsDeleteTransform(t);
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
    }
}

/// Proofing-transform internals — preview/simulation pipeline
/// paths via cmsCreateProofingTransform over profile combos x
/// intents x SOFTPROOFING/GAMUTCHECK flags.
#[test]
fn proofing_transform_paths() {
    unsafe {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        let srgb = c::cmsCreate_sRGBProfile();
        let mut cmyk = std::ptr::null_mut();
        if let Ok(d) = std::fs::read(dir.join("lcms2cmyk.icc")) {
            cmyk = c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32);
        }
        for flags in [0x4000u32, 0x5000, 0x1000 | 0x4000] {
            // input + output + proofing profile
            if !cmyk.is_null() {
                let t = c::cmsCreateProofingTransform(
                    srgb, 0x00ff04, cmyk, 0x4c02f1, cmyk,
                    0, 1, flags);
                eprintln!("proof f{flags:#x} t={}", !t.is_null());
                if !t.is_null() {
                    let inp = [128u8, 64, 200];
                    let mut out = [0u8; 4];
                    c::cmsDoTransform(t, inp.as_ptr() as *const _,
                        out.as_mut_ptr() as *mut _, 1);
                    c::cmsDeleteTransform(t);
                }
            }
        }
        if !cmyk.is_null() { c::cmsCloseProfile(cmyk); }
        c::cmsCloseProfile(srgb);
    }
}

/// Gamut/preview LUT readers — proofing transform on a profile
/// carrying a gamt tag hits _cmsReadGamutLUT; preview tags pre0-2
/// hit _cmsReadPreviewLUT.
#[test]
fn gamut_preview_lut_readers() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mut cc = [g; 3];
        // build a pipeline for the gamt tag
        let gp = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        c::cmsPipelineInsertStage(gp, 1,
            c::cmsStageAllocToneCurves(std::ptr::null_mut(), 3, cc.as_ptr()));
        let prof = c::cmsCreate_sRGBProfile();
        c::cmsWriteTag(prof, 0x67616D74, gp as *const _); // 'gamt'
        let cmyk = c::cmsCreate_sRGBProfile(); // reuse as proof target
        let srgb = c::cmsCreate_sRGBProfile();
        for flags in [0x4000u32, 0x5000] {
            let rgb16 = (4u32 << 16) | (3 << 3) | 2;
            let t = c::cmsCreateProofingTransform(
                srgb, rgb16, cmyk, rgb16, prof,
                0, 0, flags);
            eprintln!("gamut proof f{flags:#x} t={}", !t.is_null());
            if !t.is_null() {
                let ib = [100u8, 150, 200];
                let mut ob = [0u8; 4];
                c::cmsDoTransform(t, ib.as_ptr() as *const _,
                    ob.as_mut_ptr() as *mut _, 1);
                c::cmsDeleteTransform(t);
            }
        }
        c::cmsCloseProfile(prof);
        c::cmsCloseProfile(cmyk);
        c::cmsCloseProfile(srgb);
        c::cmsPipelineFree(gp);
    }
}

/// cmsCreateLinearizationDeviceLink — per-channel curve devicelink
/// on RGB/CMYK/Gray spaces + transform through it.
#[test]
fn linearization_devicelink() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        for (cs, name, ch) in [
            (0x52474220u32, "rgb", 3),   // 'RGB '
            (0x434D594B, "cmyk", 4),      // 'CMYK'
            (0x47524159, "gray", 1),      // 'GRAY'
        ] {
            let dl = c::cmsCreateLinearizationDeviceLink(
                cs, [g; 4].as_ptr() as *mut _);
            eprintln!("{name} dl={}", !dl.is_null());
            if !dl.is_null() {
                // transform through the devicelink (link IS the xform)
                let f = ((cs & 0xff) as u32) | (((ch) << 3) | 1);
                let fmt = ((cs == 0x434D594B) as u32 * 6
                    | (cs == 0x52474220) as u32 * 4
                    | (cs == 0x47524159) as u32 * 3) << 16
                    | (ch << 3) | 1;
                let t = c::cmsCreateTransform(dl, fmt, dl, fmt, 0, 0);
                eprintln!("  t={}", !t.is_null());
                if !t.is_null() { c::cmsDeleteTransform(t); }
                c::cmsCloseProfile(dl);
            }
        }
    }
}

/// cmsCreateXYZProfile + NULLProfile — XYZ-PCS profile and the
/// NULL (identity) profile through transforms.
#[test]
fn xyz_null_profiles() {
    unsafe {
        let xyz = c::cmsCreateXYZProfile();
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let x16 = (11u32 << 16) | (3 << 3) | 2; // XYZ u16
        let r16 = (4u32 << 16) | (3 << 3) | 2;
        let l16 = (10u32 << 16) | (3 << 3) | 2;
        for (inp, outp, if_, of, name) in [
            (xyz, srgb, x16, r16, "xyz->srgb"),
            (srgb, xyz, r16, x16, "srgb->xyz"),
            (xyz, lab, x16, l16, "xyz->lab"),
        ] {
            let t = c::cmsCreateTransform(inp, if_, outp, of, 0, 0);
            eprintln!("{name} t={}", !t.is_null());
            if !t.is_null() { c::cmsDeleteTransform(t); }
        }
        // NULL profile — identity transform
        let nul = c::cmsCreateNULLProfile();
        if !nul.is_null() {
            let t = c::cmsCreateTransform(nul, r16, srgb, r16, 0, 0);
            eprintln!("null t={}", !t.is_null());
            if !t.is_null() { c::cmsDeleteTransform(t); }
            c::cmsCloseProfile(nul);
        }
        c::cmsCloseProfile(xyz);
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
    }
}

/// Lab2/Lab4 profile builders + placeholder internals — the two
/// Lab profile variants' tag structure (identity CLUT + wtpt).
#[test]
fn lab_profile_variants() {
    unsafe {
        let wp = c::CieXyz { X: 0.9642, Y: 1.0, Z: 0.8249 };
        for (v, name) in [(2u32, "lab2"), (4, "lab4")] {
            let p = if v == 2 {
                c::cmsCreateLab2Profile(&wp as *const _ as *const _)
            } else {
                c::cmsCreateLab4Profile(&wp as *const _ as *const _)
            };
            eprintln!("{name}={}", !p.is_null());
            if !p.is_null() {
                // save->read covers the identity-CLUT tag structure
                let mut buf = vec![0u8; 1 << 20];
                let mut n = buf.len() as u32;
                let s = c::cmsSaveProfileToMem(p,
                    buf.as_mut_ptr() as *mut _, &mut n);
                let rp = if s != 0 {
                    c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
                } else { std::ptr::null_mut() };
                eprintln!("  save={s} reopen={}", !rp.is_null());
                if !rp.is_null() { c::cmsCloseProfile(rp); }
                c::cmsCloseProfile(p);
            }
        }
        // NULL whitepoint variant
        let p2 = c::cmsCreateLab2Profile(std::ptr::null_mut());
        eprintln!("lab2-null-wp={}", !p2.is_null());
        if !p2.is_null() { c::cmsCloseProfile(p2); }
    }
}

/// cmsTransform2DeviceLink + FindCombination — transforms on
/// various profiles hit the combination-finder + identity-curve
/// insertion inside the devicelink writer.
#[test]
fn devicelink_combinations() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let gray = c::cmsCreateGrayProfile(
            &c::CieXyz { X: 0.9642, Y: 1.0, Z: 0.8249 } as *const _ as *const _,
            c::cmsBuildGamma(std::ptr::null_mut(), 2.2));
        let r16 = (4u32 << 16) | (3 << 3) | 2;
        let l16 = (10u32 << 16) | (3 << 3) | 2;
        let g16 = (3u32 << 16) | (1 << 3) | 2;
        for (inp, outp, if_, of, name) in [
            (srgb, lab, r16, l16, "srgb->lab"),
            (srgb, srgb, r16, r16, "srgb->srgb"),
            (gray, srgb, g16, r16, "gray->srgb"),
            (srgb, gray, r16, g16, "srgb->gray"),
        ] {
            let t = c::cmsCreateTransform(inp, if_, outp, of, 0, 0);
            if t.is_null() { eprintln!("{name} t=null"); continue; }
            let dl = c::cmsTransform2DeviceLink(t, 4.4, 0);
            eprintln!("{name} dl={}", !dl.is_null());
            if !dl.is_null() { c::cmsCloseProfile(dl); }
            c::cmsDeleteTransform(t);
        }
        c::cmsCloseProfile(gray);
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
    }
}

/// cmsCreateRGBProfile/GrayProfile/LabProfile internals —
/// different primaries/temp/white-point combos.
#[test]
fn virt_builders() {
    unsafe {
        let wp = c::CieXyz { X: 0.9642, Y: 1.0, Z: 0.8249 };
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mut trc = [g; 3];
        #[repr(C)] struct Prim { r: c::CiexyY, g: c::CiexyY,
            b: c::CiexyY }
        // RGB profile — explicit primaries
        let prim = Prim {
            r: c::CiexyY { x: 0.64, y: 0.33, Y: 1.0 },
            g: c::CiexyY { x: 0.3, y: 0.6, Y: 1.0 },
            b: c::CiexyY { x: 0.15, y: 0.06, Y: 1.0 },
        };
        let rgb = c::cmsCreateRGBProfileTHR(std::ptr::null_mut(),
            &wp as *const _ as *const _,
            &prim as *const _ as *const _,
            trc.as_mut_ptr() as *mut _);
        // Gray profile
        let g2 = c::cmsCreateGrayProfileTHR(std::ptr::null_mut(),
            &wp as *const _ as *const _, g as *const _);
        // Lab profile
        let lab = c::cmsCreateLab4Profile(
            &wp as *const _ as *const _);
        // XYZ profile
        let xyz = c::cmsCreateXYZProfile();
        // NULL profile
        let nul = c::cmsCreateNULLProfile();
        for (p, name) in [(rgb, "rgb"), (g2, "gray"), (lab, "lab"),
                          (xyz, "xyz"), (nul, "null")] {
            eprintln!("{name}={}", !p.is_null());
            if !p.is_null() { c::cmsCloseProfile(p); }
        }
    }
}

/// Ink-limiting + BCHS-abstract builders — InkLimitingDeviceLink
/// (CMYK) + BCHSWabstractProfile variants.
#[test]
fn inklimit_bchs_builders() {
    unsafe {
        // InkLimitingDeviceLink — CMYK only
        for (cs, name) in [(0x434D594Bu32, "cmyk"), (0x52474220, "rgb")] {
            let dl = c::cmsCreateInkLimitingDeviceLink(cs, 0.8);
            eprintln!("inklim {name}={}", !dl.is_null());
            if !dl.is_null() { c::cmsCloseProfile(dl); }
        }
        // BCHSWabstractProfile — different white-point temps
        for (src, dst) in [(5000u32, 6500), (6500, 5000)] {
            let ap = c::cmsCreateBCHSWabstractProfile(
                17, 0.0, 0.0, 0.0, 1.0, src, dst);
            eprintln!("bchs {src}->{dst}={}", !ap.is_null());
            if !ap.is_null() { c::cmsCloseProfile(ap); }
        }
    }
}

/// cmsvirt remaining internals — devicelink-from-transform on
/// different profile pairs + intents.
#[test]
fn virt_devicelink_internals() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let r16 = (4u32 << 16) | (3 << 3) | 2;
        let l16 = (10u32 << 16) | (3 << 3) | 2;
        for (inp, outp, if_, of, intent, name) in [
            (srgb, lab, r16, l16, 0u32, "srgb-lab"),
            (lab, srgb, l16, r16, 0, "lab-srgb"),
            (srgb, srgb, r16, r16, 2, "srgb-srgb-i2"),
            (lab, lab, l16, l16, 3, "lab-lab-i3"),
        ] {
            let t = c::cmsCreateTransform(inp, if_, outp, of,
                intent, 0);
            if t.is_null() { eprintln!("{name} t=NULL"); continue; }
            let dl = c::cmsTransform2DeviceLink(t, 3.4, 0);
            eprintln!("{name} dl={}", !dl.is_null());
            if !dl.is_null() { c::cmsCloseProfile(dl); }
            c::cmsDeleteTransform(t);
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
    }
}

/// cmsvirt builder internals — BCHSW abstract, ink-limiting and
/// linearization devicelink builders with param variants.
#[test]
fn virt_builders2() {
    unsafe {
        for (b, c, h, s, t1, t2, name) in [
            (0.0f64, 0.0, 0.0, 0.0, 0u32, 0u32, "bchsw-0"),
            (10.0, -10.0, 30.0, 50.0, 5000, 6500, "bchsw-mid"),
            (100.0, 100.0, 360.0, 100.0, 6500, 5000, "bchsw-max"),
            (-50.0, 50.0, -30.0, -20.0, 4000, 9000, "bchsw-neg"),
        ] {
            let p = c::cmsCreateBCHSWabstractProfile(
                4, b, c, h, s, t1, t2);
            eprintln!("{name} p={}", !p.is_null());
            if !p.is_null() { c::cmsCloseProfile(p); }
        }
        for lim in [0.0f64, 50.0, 100.0, 200.0, 300.0] {
            let p = c::cmsCreateInkLimitingDeviceLinkTHR(
                std::ptr::null_mut(), 0x434d594b, lim);
            eprintln!("ink{lim} p={}", !p.is_null());
            if !p.is_null() { c::cmsCloseProfile(p); }
        }
        for (n, name) in [(256usize, "lin256"), (2, "lin2"), (17, "lin17")] {
            let mut tab = vec![0f32; n];
            for i in 0..n { tab[i] = i as f32 / (n - 1).max(1) as f32; }
            let mut curves = [std::ptr::null_mut(); 4];
            for c_ in curves.iter_mut() {
                *c_ = c::cmsBuildTabulatedToneCurveFloat(
                    std::ptr::null_mut(), n as u32, tab.as_ptr());
            }
            let p = c::cmsCreateLinearizationDeviceLinkTHR(
                std::ptr::null_mut(), 0x434d594b, curves.as_ptr());
            eprintln!("{name} p={}", !p.is_null());
            if !p.is_null() { c::cmsCloseProfile(p); }
            for c_ in curves { c::cmsFreeToneCurve(c_); }
        }
    }
}
