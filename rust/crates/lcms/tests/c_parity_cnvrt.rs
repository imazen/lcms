//! cmscnvrt: transform creation with format x colorspace mismatches —
//! the validation/color-space-check machinery returns the same
//! accept/reject on both sides.

use lcms::{profile, virt, xform};
use std::ffi::c_void;
use lcms_cref as c;

fn corpus() -> Vec<Vec<u8>> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
    let mut files: Vec<_> = std::fs::read_dir(&dir).unwrap()
        .filter_map(|e| e.ok()).map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "icc").unwrap_or(false))
        .collect();
    files.sort();
    files.iter().map(|f| std::fs::read(f).unwrap()).collect()
}

// pixel-format builder: colorspace-sh(16) | channels(3) | bytes(0)
fn fmt(cs_sh: u32, ch: u32, bytes: u32) -> u32 {
    (cs_sh << 16) | (ch << 3) | bytes
}

#[test]
fn format_colorspace_check() {
    // wrong-format-for-profile combos: an RGB profile with a CMYK
    // input format, a Lab profile with an RGB format, etc. Both sides
    // must accept or reject identically.
    let profiles: Vec<(Vec<u8>, &str)> = {
        let mut v = Vec::new();
        if let Ok(d) = std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../sRGB_v4_ICC_preference.icc")) {
            v.push((d, "srgb_v4"));
        }
        if let Ok(d) = std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../labv4lcms2.icc")) {
            v.push((d, "labv4"));
        }
        if let Ok(d) = std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../linlcms2.icc")) {
            v.push((d, "lin"));
        }
        v
    };
    // format sweeps: RGB(6,3ch), Lab(10,3ch), CMYK(4,4ch), Gray(1,1ch)
    let in_fmts: Vec<u32> = vec![
        fmt(6, 3, 1),  // RGB16-ish
        fmt(10, 3, 1), // Lab16
        fmt(4, 4, 1),  // CMYK16
        fmt(1, 1, 1),  // Gray16
        fmt(9, 3, 0),  // XYZ8
    ];
    for (data, name) in &profiles {
        for &in_fmt in &in_fmts {
            let c_h = unsafe {
                let h = c::cmsOpenProfileFromMem(data.as_ptr() as _, data.len() as u32);
                let t = c::cmsCreateTransform(h, in_fmt, h, in_fmt, 0, 0);
                c::cmsCloseProfile(h);
                t
            };
            let mut rp = profile::open_profile_from_mem(None, data).unwrap();
            let mut bpc = [false; 2];
            let intents = [0u32; 2];
            let adapt = [1.0f64; 2];
            let r_t = {
                let mut rp2 = profile::open_profile_from_mem(None, data).unwrap();
                let mut ps = vec![rp, rp2];
                xform::create_extended_transform(
                    None, &mut ps, &mut bpc, &intents, &adapt, None, 0,
                    in_fmt, in_fmt, 0)
            };
            assert_eq!(c_h.is_null(), r_t.is_none(),
                "{name} fmt{in_fmt:#x} presence");
            unsafe { if !c_h.is_null() { c::cmsDeleteTransform(c_h) } };
        }
    }
}

#[test]
fn virtual_transform_combos() {
    // virtual profiles with format sweeps — the link-construction
    // validation on builder output
    let mut rp = vec![
        virt::create_srgb_profile(None).unwrap(),
        virt::create_lab4_profile(None, None).unwrap(),
    ];
    for &(in_fmt, out_fmt) in &[
        (fmt(6, 3, 1), fmt(10, 3, 1)),
        (fmt(10, 3, 1), fmt(6, 3, 1)),
        (fmt(9, 3, 0), fmt(6, 3, 0)),
        (fmt(1, 1, 1), fmt(6, 3, 1)),
    ] {
        unsafe {
            let h1 = c::cmsCreate_sRGBProfile();
            let h2 = c::cmsCreateLab4Profile(std::ptr::null_mut());
            let ct = c::cmsCreateTransform(h1, in_fmt, h2, out_fmt, 0, 0);
            c::cmsCloseProfile(h1);
            c::cmsCloseProfile(h2);
            let mut bpc = [false; 2];
            let intents = [0u32; 2];
            let adapt = [1.0f64; 2];
            let r_t = xform::create_extended_transform(
                None, &mut rp, &mut bpc, &intents, &adapt, None, 0,
                in_fmt, out_fmt, 0);
            assert_eq!(ct.is_null(), r_t.is_none(),
                "virt {in_fmt:#x}->{out_fmt:#x}");
            if !ct.is_null() { c::cmsDeleteTransform(ct) };
        }
    }
}

/// Multi-colorspace transform chains: sRGB↔Lab4↔XYZ↔Gray — the
/// link-construction paths through different PCS conversions.
#[test]
fn colorspace_chains() {
    unsafe {
        let fmts = [
            ((6u32 << 16) | (3 << 3) | 1, "RGB16"),
            ((10u32 << 16) | (3 << 3) | 1, "Lab16"),
            ((9u32 << 16) | (3 << 3) | 0, "XYZ8"),
            ((1u32 << 16) | (1 << 3) | 1, "Gray16"),
        ];
        let profiles: Vec<(*mut c_void, Option<Box<lcms::profile::IccProfile>>)> = vec![
            (c::cmsCreate_sRGBProfile(), virt::create_srgb_profile(None)),
            (c::cmsCreateLab4Profile(std::ptr::null_mut()), virt::create_lab4_profile(None, None)),
            (c::cmsCreateXYZProfile(), virt::create_xyz_profile(None)),
        ];
        for (i, (ch1, _)) in profiles.iter().enumerate() {
            for (j, (ch2, _)) in profiles.iter().enumerate() {
                for &(in_fmt, _) in &fmts[..3] {
                    for &(out_fmt, _) in &fmts[..3] {
                        let ct = c::cmsCreateTransform(*ch1, in_fmt, *ch2, out_fmt, 0, 0);
                        let mut ps = vec![
                            match i { 0 => virt::create_srgb_profile(None).unwrap(),
                                      1 => virt::create_lab4_profile(None, None).unwrap(),
                                      _ => virt::create_xyz_profile(None).unwrap() },
                            match j { 0 => virt::create_srgb_profile(None).unwrap(),
                                      1 => virt::create_lab4_profile(None, None).unwrap(),
                                      _ => virt::create_xyz_profile(None).unwrap() },
                        ];
                        let mut bpc = [false; 2];
                        let intents = [0u32; 2];
                        let adapt = [1.0f64; 2];
                        let rt = xform::create_extended_transform(
                            None, &mut ps, &mut bpc, &intents, &adapt,
                            None, 0, in_fmt, out_fmt, 0);
                        assert_eq!(ct.is_null(), rt.is_none(),
                            "chain {i}->{j} {in_fmt:#x}->{out_fmt:#x}");
                        if !ct.is_null() { c::cmsDeleteTransform(ct) }
                    }
                }
            }
        }
        for (ch, _) in profiles { let _ = c::cmsCloseProfile(ch); }
    }
}

/// Intent-specific link construction: sRGB->sRGB at each intent —
/// the link shape differs per intent (matrix-join, curve-join,
/// CLUT-resample branches).
#[test]
fn intent_link_matrix() {
    unsafe {
        let in_fmt: u32 = (6 << 16) | (3 << 3) | 1;
        for intent in 0..4u32 {
            let h1 = c::cmsCreate_sRGBProfile();
            let h2 = c::cmsCreate_sRGBProfile();
            let cs = c::cmsGetColorSpace(h1) | 3 << 3 | 1;
            let ct = c::cmsCreateTransform(h1, cs, h2, cs, intent, 0);
            let _ = c::cmsCloseProfile(h1);
            let _ = c::cmsCloseProfile(h2);
            let mut rp1 = virt::create_srgb_profile(None).unwrap();
            let mut rp2 = virt::create_srgb_profile(None).unwrap();
            let fi = rp1.color_space | 3 << 3 | 1;
            let fo = rp2.color_space | 3 << 3 | 1;
            let mut profs = vec![rp1.clone(), rp2.clone()];
            let mut bpc = [false; 2];
            let intents = [intent; 2];
            let adapt = [1.0f64; 2];
            let rt = xform::create_extended_transform(
                None, &mut profs, &mut bpc, &intents, &adapt, None, 0,
                fi, fo, 0);
            assert_eq!(ct.is_null(), rt.is_none(), "intent{intent}");
            if !ct.is_null() && rt.is_some() {
                let mut rt = rt.unwrap();
                let inp = [0x1234u16, 0x5678, 0x9abc];
                let inb: Vec<u8> = inp.iter()
                    .flat_map(|v| v.to_be_bytes()).collect();
                let mut co = [0u16; 3];
                let mut ro = vec![0u8; 6];
                c::cmsDoTransform(ct, inp.as_ptr() as _,
                    co.as_mut_ptr() as *mut _, 1);
                rt.do_transform(&inb, &mut ro, 1);
                let ro16: Vec<u16> = ro.chunks_exact(2)
                    .map(|b| u16::from_be_bytes([b[0], b[1]])).collect();
                assert_eq!(co, ro16[..3], "intent{intent} eval");
                c::cmsDeleteTransform(ct);
            }
        }
    }
}

/// cmsCreateNULLProfile: the black-only devicelink output profile —
/// exercises the devicelink link path.
#[test]
fn null_profile_link() {
    unsafe {
        let h1 = c::cmsCreate_sRGBProfile();
        let h2 = c::cmsCreateNULLProfile();
        assert!(!h2.is_null());
        let cs = c::cmsGetColorSpace(h1) | 3 << 3 | 1;
        let ct = c::cmsCreateTransform(h1, cs, h2, cs, 0, 0);
        let _ = c::cmsCloseProfile(h1);
        let _ = c::cmsCloseProfile(h2);
        let mut rp1 = virt::create_srgb_profile(None).unwrap();
        let rp2 = virt::create_null_profile(None);
        assert!(rp2.is_some());
        let mut rp2 = rp2.unwrap();
        let fi = rp1.color_space | 3 << 3 | 1;
        let fo = rp2.color_space | 3 << 3 | 1;
        let mut profs = vec![rp1.clone(), rp2.clone()];
        let mut bpc = [false; 2];
        let intents = [0u32; 2];
        let adapt = [1.0f64; 2];
        let rt = xform::create_extended_transform(
            None, &mut profs, &mut bpc, &intents, &adapt, None, 0,
            fi, fo, 0);
        assert_eq!(ct.is_null(), rt.is_none(), "null profile xform");
        if !ct.is_null() && rt.is_some() {
            let mut rt = rt.unwrap();
            let inp = [0x1234u16, 0x5678, 0x9abc];
            let inb: Vec<u8> = inp.iter()
                .flat_map(|v| v.to_be_bytes()).collect();
            let mut co = [0u16; 4];
            let mut ro = vec![0u8; 8];
            c::cmsDoTransform(ct, inp.as_ptr() as _,
                co.as_mut_ptr() as *mut _, 1);
            rt.do_transform(&inb, &mut ro, 1);
            let ro16: Vec<u16> = ro.chunks_exact(2)
                .map(|b| u16::from_be_bytes([b[0], b[1]])).collect();
            assert_eq!(&co[..ro16.len()], &ro16[..], "null profile eval");
            c::cmsDeleteTransform(ct);
        }
    }
}

/// cmsCreateMultiprofileTransform — the simpler multiprofile entry
/// (vs create_extended_transform).
#[test]
fn multiprofile_transform() {
    unsafe {
        let in_fmt: u32 = (4 << 16) | (3 << 3) | 2;
        let out_fmt: u32 = (4 << 16) | (3 << 3) | 2;
        for n in [2i32, 3, 4] {
            let mut c_profs: Vec<*mut c_void> = (0..n)
                .map(|_| c::cmsCreate_sRGBProfile()).collect();
            let ct = c::cmsCreateMultiprofileTransform(
                c_profs.as_mut_ptr(), n, in_fmt, out_fmt, 0, 0);
            let mut ps: Vec<_> = (0..n)
                .map(|_| virt::create_srgb_profile(None).unwrap()).collect();
            let mut bpc = vec![false; n as usize];
            let intents = vec![0u32; n as usize];
            let adapt = vec![1.0f64; n as usize];
            let rt = xform::create_extended_transform(
                None, &mut ps, &mut bpc, &intents, &adapt, None, 0,
                in_fmt, out_fmt, 0);
            assert_eq!(ct.is_null(), rt.is_none(), "n{n}");
            if !ct.is_null() && rt.is_some() {
                let mut rt = rt.unwrap();
                let inp = [0x1234u16, 0x5678, 0x9abc];
                let inb: Vec<u8> = inp.iter()
                    .flat_map(|v| v.to_be_bytes()).collect();
                let mut co = [0u16; 3];
                let mut ro = vec![0u8; 6];
                c::cmsDoTransform(ct, inp.as_ptr() as _,
                    co.as_mut_ptr() as *mut _, 1);
                rt.do_transform(&inb, &mut ro, 1);
                let ro16: Vec<u16> = ro.chunks_exact(2)
                    .map(|b| u16::from_be_bytes([b[0], b[1]])).collect();
                assert_eq!(co, ro16[..3], "n{n} eval");
                c::cmsDeleteTransform(ct);
            }
            for h in c_profs { let _ = c::cmsCloseProfile(h); }
        }
    }
}

/// cmsCreateMultiprofileTransform on mixed-class chains — the
/// CheckOne/FindCombination/AllowedLUT link-validation machinery.
#[test]
fn mixed_class_chains() {
    unsafe {
        let in_fmt: u32 = (4 << 16) | (3 << 3) | 2;
        let out_fmt: u32 = (4 << 16) | (3 << 3) | 2;
        // chains: matrix-shaper->LUT, LUT->matrix, matrix->devicelink,
        // devicelink->matrix — exercises FindCombination's LUT table
        for (name, n, kinds) in [
            ("m->l", 2i32, [1u8, 2, 0, 0]),
            ("l->m", 2, [2u8, 1, 0, 0]),
            ("m->d", 2, [1u8, 3, 0, 0]),
            ("d->m", 2, [3u8, 1, 0, 0]),
            ("m->l->m", 3, [1u8, 2, 1, 0]),
            ("l->m->d", 3, [2u8, 1, 3, 0]),
            ("m->l->m->d", 4, [1u8, 2, 1, 3]),
        ] {
            let mut c_profs: Vec<*mut c_void> = Vec::new();
            for k in &kinds[..n as usize] {
                c_profs.push(match k {
                    1 => c::cmsCreate_sRGBProfile(), // matrix-shaper
                    2 => c::cmsCreateLab4Profile(std::ptr::null_mut()), // LUT
                    _ => c::cmsCreateNULLProfile(), // devicelink
                });
            }
            let ct = c::cmsCreateMultiprofileTransform(
                c_profs.as_mut_ptr(), n, in_fmt, out_fmt, 0, 0);
            let mut ps: Vec<_> = kinds[..n as usize].iter().map(|k| {
                match k {
                    1 => virt::create_srgb_profile(None).unwrap(),
                    2 => virt::create_lab4_profile(None, None).unwrap(),
                    _ => virt::create_null_profile(None).unwrap(),
                }
            }).collect();
            let mut bpc = vec![false; n as usize];
            let intents = vec![0u32; n as usize];
            let adapt = vec![1.0f64; n as usize];
            let rt = xform::create_extended_transform(
                None, &mut ps, &mut bpc, &intents, &adapt, None, 0,
                in_fmt, out_fmt, 0);
            assert_eq!(ct.is_null(), rt.is_none(), "{name}");
            if !ct.is_null() { c::cmsDeleteTransform(ct); }
            for h in c_profs { let _ = c::cmsCloseProfile(h); }
        }
    }
}

/// IsProperColorSpace — transform creation rejects formats whose
/// pixel-type colorspace mismatches the profile's colorspace.
#[test]
fn colorspace_mismatch_reject() {
    unsafe {
        // PT codes: 4=RGB, 10=Lab, 3=XYZ, 1=Gray, 6=CMYK
        for (name, pt, prof_colorspace_ok) in [
            ("lab-on-srgb", 10u32, false),   // Lab format on sRGB → reject
            ("xyz-on-srgb", 3, false),       // XYZ format on sRGB → reject
            ("gray-on-srgb", 1, false),
            ("cmyk-on-srgb", 6, false),
            ("rgb-on-srgb", 4, true),        // matches
        ] {
            let in_fmt = (pt << 16) | (3 << 3) | 2;
            let out_fmt: u32 = (10 << 16) | (3 << 3) | 2;
            let hi = c::cmsCreate_sRGBProfile();
            let ho = c::cmsCreateLab4Profile(std::ptr::null_mut());
            let ct = c::cmsCreateTransform(hi, in_fmt, ho, out_fmt,
                0, 0);
            let _ = c::cmsCloseProfile(hi);
            let _ = c::cmsCloseProfile(ho);
            assert_eq!(ct.is_null(), !prof_colorspace_ok,
                "{name} C reject");
            let mut rp1 = virt::create_srgb_profile(None).unwrap();
            let mut rp2 = virt::create_lab4_profile(None, None)
                .unwrap();
            let mut profs = vec![rp1.clone(), rp2.clone()];
            let mut bpc = [false; 2];
            let intents = [0u32; 2];
            let adapt = [1.0f64; 2];
            let rt = xform::create_extended_transform(
                None, &mut profs, &mut bpc, &intents, &adapt, None, 0,
                in_fmt, out_fmt, 0);
            assert_eq!(rt.is_none(), !prof_colorspace_ok,
                "{name} rust reject");
            if !ct.is_null() { c::cmsDeleteTransform(ct); }
        }
    }
}

/// Intent-link internals — absolute intent + devicelink-in-middle
/// + abstract-end chains.
#[test]
fn intent_link_internals() {
    unsafe {
        let hi = c::cmsCreate_sRGBProfile();
        let ho = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let in_fmt: u32 = (4 << 16) | (3 << 3) | 2;
        let out_fmt: u32 = (10 << 16) | (3 << 3) | 2;
        // absolute colorimetric — different link branch
        for intent in [0u32, 1, 2, 3] {
            let ct = c::cmsCreateTransform(hi, in_fmt, ho, out_fmt,
                intent, 0);
            let mut rp1 = virt::create_srgb_profile(None).unwrap();
            let mut rp2 = virt::create_lab4_profile(None, None)
                .unwrap();
            let mut profs = vec![rp1.clone(), rp2.clone()];
            let mut bpc = [false; 2];
            let intents = [intent; 2];
            let adapt = [1.0f64; 2];
            let rt = xform::create_extended_transform(
                None, &mut profs, &mut bpc, &intents, &adapt,
                None, 0, in_fmt, out_fmt, 0);
            assert_eq!(ct.is_null(), rt.is_none(), "intent {intent}");
            if !ct.is_null() {
                let inp = [30000u16, 40000, 50000];
                let inb: Vec<u8> = inp.iter()
                    .flat_map(|v| v.to_ne_bytes()).collect();
                let mut co = [0u16; 3];
                let mut ro = vec![0u8; 6];
                c::cmsDoTransform(ct, inp.as_ptr() as _,
                    co.as_mut_ptr() as *mut _, 1);
                let mut rt2 = rt.unwrap();
                rt2.do_transform(&inb, &mut ro, 1);
                let ro16: Vec<u16> = ro.chunks_exact(2)
                    .map(|b| u16::from_ne_bytes([b[0], b[1]]))
                    .collect();
                assert_eq!(co, ro16.as_slice(),
                    "intent {intent} eval");
                c::cmsDeleteTransform(ct);
            }
        }
        // devicelink in the middle of a chain
        let dl = c::cmsCreateNULLProfile();
        let mut profs = [hi, dl, ho];
        let ct2 = c::cmsCreateMultiprofileTransform(
            profs.as_mut_ptr(), 3, in_fmt, out_fmt, 0, 0);
        eprintln!("dl-mid chain null={}", ct2.is_null());
        if !ct2.is_null() { c::cmsDeleteTransform(ct2); }
        let _ = c::cmsCloseProfile(dl);
        let _ = c::cmsCloseProfile(hi);
        let _ = c::cmsCloseProfile(ho);
    }
}

/// Unusual class chains — abstract/devicelink/named-color in
/// multiprofile transforms, exercising the CheckOne/AllowedLUT/
/// ColorSpaceIsCompatible link-validation branches.
#[test]
fn unusual_class_chains() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab4 = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let nullp = c::cmsCreateNULLProfile();
        let bchsw = c::cmsCreateBCHSWabstractProfileTHR(
            std::ptr::null_mut(), 17, 0.0, 0.0, 0.0, 1.0,
            std::ptr::null(), std::ptr::null());
        let in_fmt: u32 = (4 << 16) | (3 << 3) | 2;
        let lab_fmt: u32 = (10 << 16) | (3 << 3) | 2;
        // 3-profile chains with unusual class in the middle/end
        for (name, chain) in [
            ("srgb+null+lab4", (srgb, nullp, lab4, in_fmt, lab_fmt)),
            ("srgb+bchsw+srgb", (srgb, bchsw, srgb, in_fmt, in_fmt)),
            ("lab4+srgb+srgb", (lab4, srgb, srgb, lab_fmt, in_fmt)),
            ("null+srgb+lab4", (nullp, srgb, lab4, lab_fmt, lab_fmt)),
        ] {
            let (p1, p2, p3, ifmt, ofmt) = chain;
            let mut profs = [p1, p2, p3];
            let ct = c::cmsCreateMultiprofileTransform(
                profs.as_mut_ptr(), 3, ifmt, ofmt, 0, 0);
            eprintln!("{name} null={}", ct.is_null());
            if !ct.is_null() {
                let inp = [30000u16, 40000, 50000];
                let mut co = [0u16; 3];
                c::cmsDoTransform(ct, inp.as_ptr() as _,
                    co.as_mut_ptr() as *mut _, 1);
                c::cmsDeleteTransform(ct);
            }
        }
        let _ = c::cmsCloseProfile(srgb);
        let _ = c::cmsCloseProfile(lab4);
        let _ = c::cmsCloseProfile(nullp);
        let _ = c::cmsCloseProfile(bchsw);
    }
}

/// Transform FROM a devicelink profile — the devicelink-transform
/// branch in the link machinery.
#[test]
fn devicelink_transform() {
    unsafe {
        // bake a devicelink, then transform through it
        let hi = c::cmsCreate_sRGBProfile();
        let ho = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let in_fmt: u32 = (4 << 16) | (3 << 3) | 2;
        let out_fmt: u32 = (10 << 16) | (3 << 3) | 2;
        let ct = c::cmsCreateTransform(hi, in_fmt, ho, out_fmt,
            0, 0);
        let dl = c::cmsTransform2DeviceLink(ct, 4.3, 0);
        c::cmsDeleteTransform(ct);
        if !dl.is_null() {
            let in_fmt2: u32 = (4 << 16) | (3 << 3) | 2;
            let ct2 = c::cmsCreateTransform(dl, in_fmt2, dl,
                in_fmt2, 0, 0);
            eprintln!("dl->dl transform null={}", ct2.is_null());
            if !ct2.is_null() {
                let inp = [30000u16, 40000, 50000];
                let mut co = [0u16; 3];
                c::cmsDoTransform(ct2, inp.as_ptr() as _,
                    co.as_mut_ptr() as *mut _, 1);
                c::cmsDeleteTransform(ct2);
            }
            c::cmsCloseProfile(dl);
        }
        let _ = c::cmsCloseProfile(hi);
        let _ = c::cmsCloseProfile(ho);
    }
}

/// 4-profile chains + proofing gamut — the multi-link and
/// gamut-check internals.
#[test]
fn four_profile_chains() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab4 = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let xyz = c::cmsCreateXYZProfile();
        let nullp = c::cmsCreateNULLProfile();
        let in_fmt: u32 = (4 << 16) | (3 << 3) | 2;
        let lab_fmt: u32 = (10 << 16) | (3 << 3) | 2;
        for (name, chain) in [
            ("srgb+lab4+xyz+srgb",
             (srgb, lab4, xyz, srgb, in_fmt, in_fmt)),
            ("srgb+null+lab4+srgb",
             (srgb, nullp, lab4, srgb, in_fmt, in_fmt)),
            ("lab4+srgb+xyz+lab4",
             (lab4, srgb, xyz, lab4, lab_fmt, lab_fmt)),
        ] {
            let (p1, p2, p3, p4, ifmt, ofmt) = chain;
            let mut profs = [p1, p2, p3, p4];
            let ct = c::cmsCreateMultiprofileTransform(
                profs.as_mut_ptr(), 4, ifmt, ofmt, 0, 0);
            eprintln!("{name} null={}", ct.is_null());
            if !ct.is_null() {
                let inp = [30000u16, 40000, 50000];
                let mut co = [0u16; 3];
                c::cmsDoTransform(ct, inp.as_ptr() as _,
                    co.as_mut_ptr() as *mut _, 1);
                c::cmsDeleteTransform(ct);
            }
        }
        let _ = c::cmsCloseProfile(srgb);
        let _ = c::cmsCloseProfile(lab4);
        let _ = c::cmsCloseProfile(xyz);
        let _ = c::cmsCloseProfile(nullp);
    }
}

/// Transform FROM a stored-mAB devicelink — the mAB LUT-reader
/// internals on a serialized pipeline.
#[test]
fn mab_lut_reader() {
    unsafe {
        let hi = c::cmsCreate_sRGBProfile();
        let ho = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let in_fmt: u32 = (4 << 16) | (3 << 3) | 2;
        let out_fmt: u32 = (10 << 16) | (3 << 3) | 2;
        let ct = c::cmsCreateTransform(hi, in_fmt, ho, out_fmt,
            0, 0);
        let dl = c::cmsTransform2DeviceLink(ct, 4.3, 0);
        c::cmsDeleteTransform(ct);
        if dl.is_null() { return; }
        // save + reload — the devicelink's A2B0 is now a stored
        // mAB tag; a transform reads it back through the LUT
        // reader
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(dl, std::ptr::null_mut(), &mut cn);
        let mut cbuf = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(dl, cbuf.as_mut_ptr() as *mut _,
            &mut cn);
        let dl2 = c::cmsOpenProfileFromMem(cbuf.as_ptr() as _, cn);
        // transform through the reloaded devicelink — reads the
        // mAB into a real pipeline
        let lab4 = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let ct2 = c::cmsCreateMultiprofileTransform(
            [dl2, lab4].as_mut_ptr(), 2, in_fmt, out_fmt, 0, 0);
        eprintln!("mab transform null={}", ct2.is_null());
        if !ct2.is_null() {
            let inp = [30000u16, 40000, 50000];
            let mut co = [0u16; 3];
            c::cmsDoTransform(ct2, inp.as_ptr() as _,
                co.as_mut_ptr() as *mut _, 1);
            eprintln!("eval={:?}", co);
            c::cmsDeleteTransform(ct2);
        }
        let _ = c::cmsCloseProfile(lab4);
        let _ = c::cmsCloseProfile(dl2);
        c::cmsCloseProfile(dl);
        let _ = c::cmsCloseProfile(hi);
        let _ = c::cmsCloseProfile(ho);
    }
}

/// Optimizer internals — transforms between profile pairs
/// producing different optimized pipelines.
#[test]
fn optimizer_pipelines() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab4 = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let lab2 = c::cmsCreateLab2Profile(std::ptr::null_mut());
        let xyz = c::cmsCreateXYZProfile();
        let nullp = c::cmsCreateNULLProfile();
        let in_fmt: u32 = (4 << 16) | (3 << 3) | 2;
        let lab_fmt: u32 = (10 << 16) | (3 << 3) | 2;
        let xyz_fmt: u32 = (3 << 16) | (3 << 3) | 2;
        // different profile pairs → different optimized shapes
        for (name, (i, ifmt, o, ofmt)) in [
            ("srgb->lab4", (srgb, in_fmt, lab4, lab_fmt)),
            ("srgb->lab2", (srgb, in_fmt, lab2, lab_fmt)),
            ("lab4->srgb", (lab4, lab_fmt, srgb, in_fmt)),
            ("lab2->srgb", (lab2, lab_fmt, srgb, in_fmt)),
            ("srgb->null", (srgb, in_fmt, nullp, in_fmt)),
            ("null->srgb", (nullp, in_fmt, srgb, in_fmt)),
            ("srgb->xyz", (srgb, in_fmt, xyz, xyz_fmt)),
            ("xyz->srgb", (xyz, xyz_fmt, srgb, in_fmt)),
            ("lab4->lab2", (lab4, lab_fmt, lab2, lab_fmt)),
            ("lab2->lab4", (lab2, lab_fmt, lab4, lab_fmt)),
        ] {
            let ct = c::cmsCreateTransform(i, ifmt, o, ofmt, 0, 0);
            if ct.is_null() { continue; }
            let inp = [30000u16, 40000, 50000];
            let mut co = [0u16; 3];
            c::cmsDoTransform(ct, inp.as_ptr() as _,
                co.as_mut_ptr() as *mut _, 1);
            c::cmsDeleteTransform(ct);
        }
        let _ = c::cmsCloseProfile(srgb);
        let _ = c::cmsCloseProfile(lab4);
        let _ = c::cmsCloseProfile(lab2);
        let _ = c::cmsCloseProfile(xyz);
        let _ = c::cmsCloseProfile(nullp);
    }
}

/// More intent-link internals — mixed chains (abstract-in-
/// middle, devicelink-at-end, proofing, unusual classes).
#[test]
fn mixed_chains() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab4 = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let in_fmt: u32 = (4 << 16) | (3 << 3) | 2;
        let lab_fmt: u32 = (10 << 16) | (3 << 3) | 2;
        let ab = c::cmsCreateBCHSWabstractProfileTHR(
            std::ptr::null_mut(), 17, 1.5, 30.0, 0.8,
            4.3, std::ptr::null(), std::ptr::null());
        // mixed chains — different profile combinations
        for (name, profs) in [
            ("srgb+ab+srgb",
             vec![srgb, ab, srgb]),
            ("srgb+lab4+ab+srgb",
             vec![srgb, lab4, ab, srgb]),
            ("ab+srgb+lab4",
             vec![ab, srgb, lab4]),
            ("srgb+ab+lab4",
             vec![srgb, ab, lab4]),
            ("lab4+ab+srgb",
             vec![lab4, ab, srgb]),
            ("srgb+srgb+ab",
             vec![srgb, srgb, ab]),
        ] {
            let mut p = profs;
            let ct = c::cmsCreateMultiprofileTransform(
                p.as_mut_ptr(), p.len() as i32,
                in_fmt, in_fmt, 0, 0);
            eprintln!("{name} null={}", ct.is_null());
            if !ct.is_null() {
                let inp = [30000u16, 40000, 50000];
                let mut co = [0u16; 3];
                c::cmsDoTransform(ct, inp.as_ptr() as _,
                    co.as_mut_ptr() as *mut _, 1);
                c::cmsDeleteTransform(ct);
            }
        }
        let _ = c::cmsCloseProfile(srgb);
        let _ = c::cmsCloseProfile(lab4);
        let _ = c::cmsCloseProfile(ab);
    }
}

/// Proofing-link machinery — cmsCreateProofingTransform on
/// more combos (gamut-check pipelines).
#[test]
fn proofing_links() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab4 = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let cmyk = c::cmsCreateInkLimitingDeviceLink(
            0x434D594B, 300.0);
        let in_fmt: u32 = (4 << 16) | (3 << 3) | 2;
        let lab_fmt: u32 = (10 << 16) | (3 << 3) | 2;
        let cmyk_fmt: u32 = (6 << 16) | (4 << 3) | 2;
        // proofing transforms — the gamut-check machinery
        for (name, (i, ifmt, o, ofmt, prf, intents)) in [
            ("srgb->lab4-cmyk",
             (srgb, in_fmt, lab4, lab_fmt, cmyk,
              [0u32, 0])),
            ("lab4->srgb-cmyk",
             (lab4, lab_fmt, srgb, in_fmt, cmyk,
              [0u32, 0])),
            ("srgb->srgb-cmyk",
             (srgb, in_fmt, srgb, in_fmt, cmyk,
              [0u32, 0])),
        ] {
            let (pi, pifmt, po, pofmt, proof, intents) =
                (i, ifmt, o, ofmt, prf, intents);
            let ct = c::cmsCreateProofingTransform(
                pi, pifmt, po, pofmt, proof,
                intents[0], intents[1], 0);
            eprintln!("{name} null={}", ct.is_null());
            if !ct.is_null() {
                let inp = [30000u16, 40000, 50000];
                let mut co = [0u16; 4];
                c::cmsDoTransform(ct, inp.as_ptr() as _,
                    co.as_mut_ptr() as *mut _, 1);
                c::cmsDeleteTransform(ct);
            }
        }
        let _ = c::cmsCloseProfile(srgb);
        let _ = c::cmsCloseProfile(lab4);
        let _ = c::cmsCloseProfile(cmyk);
    }
}

/// Real stored-LUT evals — transforms through corpus
/// profiles' actual LUT data.
#[test]
fn stored_lut_evals() {
    unsafe {
        for data in corpus() {
            let h = c::cmsOpenProfileFromMem(
                data.as_ptr() as _, data.len() as u32);
            // transform through the profile's stored LUT — the
            // actual LUT evaluator on real serialized data
            let in_fmt: u32 = (4 << 16) | (3 << 3) | 2;
            let lab_fmt: u32 = (10 << 16) | (3 << 3) | 2;
            let lab4 = c::cmsCreateLab4Profile(
                std::ptr::null_mut());
            let ct = c::cmsCreateTransform(h, in_fmt, lab4,
                lab_fmt, 0, 0);
            let _ = c::cmsCloseProfile(lab4);
            if !ct.is_null() {
                let inp = [30000u16, 40000, 50000];
                let mut co = [0u16; 3];
                c::cmsDoTransform(ct, inp.as_ptr() as _,
                    co.as_mut_ptr() as *mut _, 1);
                c::cmsDeleteTransform(ct);
            }
            c::cmsCloseProfile(h);
        }
    }
}

/// Stored-type LUT readers — cmsCreateTransform on profiles
/// with unusual stored types (the LUT-reader internals).
#[test]
fn stored_type_lut_readers() {
    unsafe {
        for data in corpus() {
            let h = c::cmsOpenProfileFromMem(
                data.as_ptr() as _, data.len() as u32);
            // transform through the profile's stored LUT — the
            // LUT-reader dispatch on whatever type was stored
            let in_fmt: u32 = (4 << 16) | (3 << 3) | 2;
            let lab4 = c::cmsCreateLab4Profile(
                std::ptr::null_mut());
            let lab_fmt: u32 = (10 << 16) | (3 << 3) | 2;
            let ct = c::cmsCreateTransform(h, in_fmt, lab4,
                lab_fmt, 0, 0);
            if !ct.is_null() {
                let inp = [30000u16, 40000, 50000];
                let mut out = [0u16; 3];
                c::cmsDoTransform(ct, inp.as_ptr() as _,
                    out.as_mut_ptr() as *mut _, 1);
                c::cmsDeleteTransform(ct);
            }
            let _ = c::cmsCloseProfile(lab4);
            c::cmsCloseProfile(h);
        }
    }
}
