//! io1 LUT-reader coverage — corpus x intent transform matrix.
use lcms_cref as c;

// RGB_8 format for the sRGB side
const RGB8: u32 = 0x00ff04;

fn fmt_for(colorspace: u32, as_input: u32) -> u32 {
    // format matching the profile colorspace (u8 chunky)
    let ch = match colorspace {
        0x47524159 => 1u32, // 'GRAY'
        0x434D594B => 4,    // 'CMYK'
        0x4C616220 => 3,    // 'Lab '
        _ => 3,
    };
    let pt = match colorspace {
        0x47524159 => 3u32, // PT_GRAY
        0x434D594B => 6,    // PT_CMYK
        0x4C616220 => 10,   // PT_Lab
        _ => 4,             // PT_RGB
    };
    (pt << 16) | (ch << 3) | 1
}

fn make_r_xform(data: &[u8], dir: u32, intent: u32)
    -> Option<Box<lcms::xform::Transform>> {
    let mut prof = lcms::profile::open_profile_from_mem(None, data)?;
    let mut srgb = lcms::virt::create_srgb_profile(None).unwrap();
    let cs = prof.color_space;
    let (i, o) = if dir == 0 {
        (prof, srgb)
    } else {
        (srgb, prof)
    };
    let mut profs: Vec<Box<lcms::profile::IccProfile>> = vec![i, o];
    let mut bpc = [false; 2];
    let mut gamut: Option<&mut lcms::profile::IccProfile> = None;
    lcms::xform::create_extended_transform(
        None, &mut profs, &mut bpc, &[intent, intent],
        &[1.0, 1.0], gamut, 0, fmt_for(cs, dir), RGB8, 0)
}

/// Corpus x intent transform matrix — every bundled profile as
/// input AND output at intents 0/1/2 covers the tag-dispatch arms
/// of _cmsReadInputLUT/_cmsReadOutputLUT/_cmsReadDevicelinkLUT and
/// the matrix-shaper builders for matrix+curve profiles.
#[test]
fn corpus_intent_matrix() {
    unsafe {
        let files = [
            "lcms2link.icc", "graylcms2.icc", "labv2lcms2.icc",
            "linlcms2.icc", "brightness.icc", "gray3lcms2.icc",
            "sRGB3.icc", "labv4lcms2.icc",
        ];
        let dir = std::path::Path::new(
            env!("CARGO_MANIFEST_DIR")).join("../../");
        for f in files {
            let path = dir.join(f);
            let Ok(data) = std::fs::read(&path) else {
                eprintln!("{f}: missing"); continue;
            };
            let cp = c::cmsOpenProfileFromMem(
                data.as_ptr() as *const _, data.len() as u32);
            if cp.is_null() { eprintln!("{f}: C open fail"); continue; }
            let cs = c::cmsGetColorSpace(cp);
            // input->srgb and srgb->output at each intent
            for intent in [0u32, 1, 2] {
                let srgb = c::cmsCreate_sRGBProfile();
                for dir in [0u32, 1] {
                    let (pi, po) = if dir == 0 { (cp, srgb) } else { (srgb, cp) };
                    let t = c::cmsCreateTransform(pi,
                        fmt_for(cs, dir), po, 0x00ff04, intent, 0);
                    let rt = make_r_xform(&data, dir, intent);
                    assert_eq!(t.is_null(), rt.is_none(),
                        "{f} dir{dir} i{intent} create parity");
                    if !t.is_null() { c::cmsDeleteTransform(t); }
                }
                c::cmsCloseProfile(srgb);
            }
            c::cmsCloseProfile(cp);
        }
    }
}

/// BuildRGBInputMatrixShaper Lab-PCS arm — a matrix-shaper profile
/// with cmsSigLabData PCS appends the _cmsStageAllocXYZ2Lab stage.
/// Construct: placeholder + Lab PCS + rXYZ/gXYZ/bXYZ + rTRC..bTRC.
#[test]
fn matrix_shaper_lab_pcs() {
    unsafe {
        let p = c::cmsCreateProfilePlaceholder(std::ptr::null_mut());
        c::cmsSetDeviceClass(p, 0x6d6e7472); // 'mntr'
        c::cmsSetColorSpace(p, 0x52474220);  // 'RGB '
        c::cmsSetPCS(p, 0x4C616220);          // 'Lab '
        // media white point (required)
        #[repr(C)] struct Xyz0 { x: f64, y: f64, z: f64 }
        let wp = Xyz0 { x: 0.9642, y: 1.0, z: 0.8249 };
        c::cmsWriteTag(p, 0x77747074, &wp as *const _ as *const _);
        // XYZ colorant tags (sRGB primaries -> XYZ)
        for (sig, xyz) in [
            (0x7258595Au32, [0.436, 0.2225, 0.0139]),   // 'rXYZ'
            (0x6758595Au32, [0.385, 0.7169, 0.0971]),   // 'gXYZ'
            (0x6258595Au32, [0.143, 0.0606, 0.714]),    // 'bXYZ'
        ] {
            #[repr(C)] struct Xyz { x: f64, y: f64, z: f64 }
            let v = Xyz { x: xyz[0], y: xyz[1], z: xyz[2] };
            c::cmsWriteTag(p, sig, &v as *const _ as *const _);
        }
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        for sig in [0x72545243u32, 0x67545243, 0x62545243] { // rTRC/gTRC/bTRC
            c::cmsWriteTag(p, sig, g as *const _);
        }
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let t = c::cmsCreateTransform(p, 0x00ff04, lab,
            0x0a0702, 0, 0);
        eprintln!("labpcs t={}", !t.is_null());
        if !t.is_null() {
            let inp = [128u8, 200, 50];
            let mut out = [0u16; 3];
            c::cmsDoTransform(t, inp.as_ptr() as *const _,
                out.as_mut_ptr() as *mut _, 1);
            eprintln!("  out={out:?}");
            c::cmsDeleteTransform(t);
        }
        c::cmsCloseProfile(p);
        c::cmsCloseProfile(lab);
    }
}

/// _cmsReadOutputLUT Lab-PCS/Lut16 normalization arms — Lab-PCS
/// profiles with Lut16 B2A insert LabV4ToV2; Lab-colorspace output
/// appends LabV2ToV4. labv2/labv4 corpus profiles as OUTPUT side.
#[test]
fn output_lut_lab_normalization() {
    unsafe {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        for f in ["labv2lcms2.icc", "labv4lcms2.icc", "lcms2cmyk.icc",
                  "graylcms2.icc"] {
            let Ok(d) = std::fs::read(dir.join(f)) else { continue };
            let p = c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32);
            if p.is_null() { continue; }
            let cs = c::cmsGetColorSpace(p);
            let out_fmt = match cs {
                0x4C616220 => (10 << 16) | (3 << 3) | 2u32, // Lab16
                0x434D594B => (6 << 16) | (4 << 3) | 1,
                0x47524159 => (3 << 16) | (1 << 3) | 1,
                _ => (4 << 16) | (3 << 3) | 1,
            };
            let srgb = c::cmsCreate_sRGBProfile();
            // srgb -> profile: profile is OUTPUT -> ReadOutputLUT
            for intent in 0..=3u32 {
                let t = c::cmsCreateTransform(srgb, 0x00ff04,
                    p, out_fmt, intent, 0);
                eprintln!("{f} out i{intent} t={}", !t.is_null());
                if !t.is_null() {
                    let inp = [128u8, 64, 220];
                    let mut out = [0u8; 8];
                    c::cmsDoTransform(t, inp.as_ptr() as *const _,
                        out.as_mut_ptr() as *mut _, 1);
                    c::cmsDeleteTransform(t);
                }
            }
            c::cmsCloseProfile(srgb);
            c::cmsCloseProfile(p);
        }
    }
}

/// _cmsReadDevicelinkLUT dispatch — named-color-class profile with
/// ncl2 tag -> named-color stage + LabV2ToV4; devicelink with
/// AToB*/DToB* tags -> float/16 LUT arms.
#[test]
fn devicelink_lut_dispatch() {
    unsafe {
        // named-color class + ncl2 tag -> the named-color arm
        let ncl = c::cmsAllocNamedColorList(
            std::ptr::null_mut(), 2, 4, b"pre\0".as_ptr(),
            b"suf\0".as_ptr());
        for (name, pcs) in [("S1\0", [0x9000u16, 0x5500, 0x5500]),
                            ("S2\0", [0x3000, 0x2000, 0x9000])] {
            let col = [0xFFFFu16; 4];
            c::cmsAppendNamedColor(ncl, name.as_ptr(),
                pcs.as_ptr(), col.as_ptr());
        }
        let nmcl = c::cmsCreateProfilePlaceholder(std::ptr::null_mut());
        c::cmsSetDeviceClass(nmcl, 0x6e6d636c);      // 'nmcl'
        c::cmsSetColorSpace(nmcl, 0x6e636c32);        // 'ncl2'
        c::cmsSetPCS(nmcl, 0x4C616220);               // 'Lab '
        c::cmsWriteTag(nmcl, 0x6e636c32, ncl as *const _);
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        for intent in 0..=4u32 {
            let t = c::cmsCreateTransform(nmcl,
                (1 << 3) | 2, lab, 0x0a0702, intent, 0);
            eprintln!("nmcl i{intent} t={}", !t.is_null());
            if !t.is_null() {
                let idx = [0u16, 1];
                let mut out = [0u16; 3];
                c::cmsDoTransform(t, idx.as_ptr() as *const _,
                    out.as_mut_ptr() as *mut _, 1);
                eprintln!("  out={out:?}");
                c::cmsDeleteTransform(t);
            }
        }
        c::cmsCloseProfile(nmcl);
        c::cmsCloseProfile(lab);
        c::cmsFreeNamedColorList(ncl);
    }
}
