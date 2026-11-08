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
