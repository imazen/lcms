//! io1 LUT-reader coverage — corpus x intent transform matrix.
use lcms_cref as c;
use std::sync::Mutex;
static CTX_LOCK: Mutex<()> = Mutex::new(());

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

/// Matrix-shaper reader arms — sRGB as output (BuildRGBOutput-
/// MatrixShaper: inverse matrix + inverse TRC) and as input
/// (BuildRGBInputMatrixShaper) at multiple intents.
#[test]
fn matrix_shaper_directions() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let rgb16 = (4u32 << 16) | (3 << 3) | 2;   // RGB u16
        let lab16 = (10u32 << 16) | (3 << 3) | 2;  // Lab u16
        for (name, inp, out, in_fmt, out_fmt) in [
            ("srgb-in", srgb, lab, rgb16, lab16),
            ("srgb-out", lab, srgb, lab16, rgb16),
        ] {
            for intent in 0..=3u32 {
                let t = c::cmsCreateTransform(inp, in_fmt, out,
                    out_fmt, intent, 0);
                eprintln!("{name} i{intent} t={}", !t.is_null());
                if !t.is_null() {
                    let mut buf = [0x80u8; 16];
                    let mut ob = [0u8; 16];
                    c::cmsDoTransform(t, buf.as_ptr() as *const _,
                        ob.as_mut_ptr() as *mut _, 1);
                    c::cmsDeleteTransform(t);
                }
            }
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
    }
}

/// _cmsReadInputLUT named-color arm — nmcl-class profile in input
/// position: ncl2 -> named-color stage + LabV2ToV4 + empty pipeline.
#[test]
fn named_color_input_lut() {
    unsafe {
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
        c::cmsSetDeviceClass(nmcl, 0x6e6d636c);
        c::cmsSetColorSpace(nmcl, 0x6e636c32);
        c::cmsSetPCS(nmcl, 0x4C616220);
        c::cmsWriteTag(nmcl, 0x6e636c32, ncl as *const _);
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let rgb16 = (4u32 << 16) | (3 << 3) | 2;
        let lab16 = (10u32 << 16) | (3 << 3) | 2;
        for (out, name) in [(lab, "lab"), (srgb, "srgb")] {
            let of = if out == srgb { rgb16 } else { lab16 };
            let t = c::cmsCreateTransform(nmcl,
                (1 << 3) | 2, out, of, 0, 0);
            eprintln!("nmcl-in->{name} t={}", !t.is_null());
            if !t.is_null() {
                let idx = [0u16, 1];
                let mut ob = [0u8; 8];
                c::cmsDoTransform(t, idx.as_ptr() as *const _,
                    ob.as_mut_ptr() as *mut _, 1);
                c::cmsDeleteTransform(t);
            }
        }
        c::cmsCloseProfile(nmcl);
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
        c::cmsFreeNamedColorList(ncl);
    }
}

/// Gray matrix pipelines — gray profile in input/output transform
/// position -> BuildGrayInputMatrixPipeline/BuildGrayOutput-
/// MatrixPipeline + the gray-PCS branches.
#[test]
fn gray_matrix_pipelines() {
    unsafe {
        let wp = c::CieXyz { X: 0.9642, Y: 1.0, Z: 0.8249 };
        let gray = c::cmsCreateGrayProfile(&wp as *const _ as *const _,
            c::cmsBuildGamma(std::ptr::null_mut(), 2.2));
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let g16 = (3u32 << 16) | (1 << 3) | 2;  // GRAY u16
        let r16 = (4u32 << 16) | (3 << 3) | 2;  // RGB u16
        let l16 = (10u32 << 16) | (3 << 3) | 2; // Lab u16
        // gray in -> gray out (input + output pipelines)
        for (inp, outp, if_, of, name) in [
            (gray, srgb, g16, r16, "gray->srgb"),
            (srgb, gray, r16, g16, "srgb->gray"),
            (gray, lab, g16, l16, "gray->lab"),
            (lab, gray, l16, g16, "lab->gray"),
        ] {
            let t = c::cmsCreateTransform(inp, if_, outp, of, 0, 0);
            eprintln!("{name} t={}", !t.is_null());
            if !t.is_null() {
                let ib = [0x40u8; 8];
                let mut ob = [0u8; 8];
                c::cmsDoTransform(t, ib.as_ptr() as *const _,
                    ob.as_mut_ptr() as *mut _, 1);
                c::cmsDeleteTransform(t);
            }
        }
        c::cmsCloseProfile(gray);
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
    }
}

/// _cmsReadMediaWhitePoint + _cmsReadCHAD — wtpt/chad tag reads on
/// profiles with/without those tags; header setter/getter arms.
#[test]
fn media_white_chad() {
    unsafe {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        #[repr(C)] struct Xyz { x: f64, y: f64, z: f64 }
        for f in ["lcms2cmyk.icc", "graylcms2.icc", "labv2lcms2.icc",
                  "sRGB3.icc", "linlcms2.icc"] {
            let Ok(d) = std::fs::read(dir.join(f)) else { continue };
            let p = c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32);
            if p.is_null() { continue; }
            let mut wp = Xyz { x: 0.0, y: 0.0, z: 0.0 };
            let r = c::_cmsReadMediaWhitePoint(
                &mut wp as *mut _ as *mut _, p);
            let mut chad = [0f64; 9];
            let r2 = c::_cmsReadCHAD(chad.as_mut_ptr() as *mut _, p);
            eprintln!("{f} wp={r} ({:.3},{:.3},{:.3}) chad={r2}",
                wp.x, wp.y, wp.z);
            c::cmsCloseProfile(p);
        }
    }
}

/// _cmsReadFloatInputTag normalization arms — Lab-colorspace
/// profile + mpet DToB tag -> NormalizeToLabFloat + FromLabFloat.
#[test]
fn float_lut_lab_normalization() {
    unsafe {
        let ftab: Vec<f32> = (0..8*3).map(|i| i as f32 / 24.0).collect();
        let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocCLutFloat(std::ptr::null_mut(), 2, 3, 3,
                ftab.as_ptr()));
        let prof = c::cmsCreateProfilePlaceholder(std::ptr::null_mut());
        c::cmsSetDeviceClass(prof, 0x6d6e7472); // 'mntr'
        c::cmsSetColorSpace(prof, 0x4C616220);  // 'Lab ' colorspace
        c::cmsSetPCS(prof, 0x4C616220);          // 'Lab ' PCS
        c::cmsWriteTag(prof, 0x44324230, p as *const _); // D2B0 mpet
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let l16 = (10u32 << 16) | (3 << 3) | 2;
        let lf = (10u32 << 16) | (1 << 22) | (3 << 3) | 4;
        let t = c::cmsCreateTransform(prof, l16, lab, l16, 0, 0);
        let tf = c::cmsCreateTransform(prof, lf, lab, lf, 0, 0);
        eprintln!("lab-mpet t={} tf={}", !t.is_null(), !tf.is_null());
        if !t.is_null() { c::cmsDeleteTransform(t); }
        if !tf.is_null() { c::cmsDeleteTransform(tf); }
        c::cmsCloseProfile(prof);
        c::cmsCloseProfile(lab);
        c::cmsPipelineFree(p);
    }
}

/// _cmsReadFloatDevicelinkTag — devicelink profile + DToB/BToD
/// mpet tag in transform position -> the float devicelink reader.
#[test]
fn float_devicelink_tag() {
    unsafe {
        let ftab: Vec<f32> = (0..8*3)
            .map(|i| i as f32 / 24.0).collect();
        let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocCLutFloat(std::ptr::null_mut(), 2, 3, 3,
                ftab.as_ptr()));
        let dl = c::cmsCreateProfilePlaceholder(std::ptr::null_mut());
        c::cmsSetDeviceClass(dl, 0x6c696e6b); // 'link'
        c::cmsSetColorSpace(dl, 0x52474220);   // 'RGB '
        c::cmsSetPCS(dl, 0x4C616220);           // 'Lab '
        c::cmsSetProfileVersion(dl, 4.4);
        c::cmsWriteTag(dl, 0x41324230, p as *const _); // A2B0 mpet
        let r16 = (4u32 << 16) | (3 << 3) | 2;
        let rf = (4u32 << 16) | (1 << 22) | (3 << 3) | 4;
        for (f, name) in [(r16, "u16"), (rf, "f32")] {
            let t = c::cmsCreateTransform(dl, f, dl, f, 0, 0);
            eprintln!("{name} t={}", !t.is_null());
            if !t.is_null() {
                let ib = [0x40u8; 16];
                let mut ob = [0u8; 16];
                c::cmsDoTransform(t, ib.as_ptr() as *const _,
                    ob.as_mut_ptr() as *mut _, 1);
                c::cmsDeleteTransform(t);
            }
        }
        c::cmsCloseProfile(dl);
        c::cmsPipelineFree(p);
    }
}

/// Profile-capability helpers — cmsIsCLUT/IsIntentSupported/
/// IsMatrixShaper across profile classes x intents.
#[test]
fn profile_capability_helpers() {
    unsafe {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        let mut profs = vec![
            ("srgb", c::cmsCreate_sRGBProfile()),
            ("lab", c::cmsCreateLab4Profile(std::ptr::null_mut())),
        ];
        for f in ["lcms2cmyk.icc", "lcms2link.icc", "graylcms2.icc"] {
            if let Ok(d) = std::fs::read(dir.join(f)) {
                let p = c::cmsOpenProfileFromMem(
                    d.as_ptr() as *const _, d.len() as u32);
                if !p.is_null() { profs.push((f, p)); }
            }
        }
        for (name, p) in &profs {
            let ms = c::cmsIsMatrixShaper(*p);
            let mut clut = String::new();
            for intent in [0u32, 1, 2] {
                for dir in [0u32, 1] {
                    if c::cmsIsCLUT(*p, intent, dir) != 0 {
                        clut.push_str(&format!("i{intent}d{dir} "));
                    }
                }
            }
            let sup = c::cmsIsIntentSupported(*p, 0, 0);
            eprintln!("{name} mxs={ms} clut=[{clut}] sup={sup}");
            c::cmsCloseProfile(*p);
        }
    }
}

/// Float-tag readers — DToB/BToD mpet tags on profiles in each
/// transform position -> _cmsReadFloatInputTag/OutputTag/
/// DevicelinkTag + the normalization insertions.
#[test]
fn float_tag_readers() {
    unsafe {
        let ftab: Vec<f32> = (0..8*3)
            .map(|i| i as f32 / 24.0).collect();
        let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocCLutFloat(std::ptr::null_mut(), 2, 3, 3,
                ftab.as_ptr()));
        // input-class profile with DToB tag -> _cmsReadFloatInputTag
        let inp = c::cmsCreateProfilePlaceholder(std::ptr::null_mut());
        c::cmsSetDeviceClass(inp, 0x6d6e7472);
        c::cmsSetColorSpace(inp, 0x52474220);
        c::cmsSetPCS(inp, 0x4C616220);
        c::cmsSetProfileVersion(inp, 4.4);
        c::cmsWriteTag(inp, 0x44324230, p as *const _); // 'D2B0'
        let r16 = (4u32 << 16) | (3 << 3) | 2;
        let l16 = (10u32 << 16) | (3 << 3) | 2;
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        for (a, b, af, bf, name) in [
            (inp, lab, r16, l16, "dtob-in"),
            (lab, inp, l16, r16, "dtob-out"),
        ] {
            let t = c::cmsCreateTransform(a, af, b, bf, 0, 0);
            eprintln!("{name} t={}", !t.is_null());
            if !t.is_null() { c::cmsDeleteTransform(t); }
        }
        c::cmsCloseProfile(inp);
        c::cmsCloseProfile(lab);
        c::cmsPipelineFree(p);
    }
}

/// _cmsReadFloatOutputTag — BToD mpet tag on an output-position
/// profile -> the float output reader + FromLab normalization.
#[test]
fn float_output_tag() {
    unsafe {
        let ftab: Vec<f32> = (0..8*3)
            .map(|i| i as f32 / 24.0).collect();
        let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocCLutFloat(std::ptr::null_mut(), 2, 3, 3,
                ftab.as_ptr()));
        // output profile (Lab PCS in, RGB device out) with BToD tag
        let outp = c::cmsCreateProfilePlaceholder(std::ptr::null_mut());
        c::cmsSetDeviceClass(outp, 0x6d6e7472);
        c::cmsSetColorSpace(outp, 0x52474220);
        c::cmsSetPCS(outp, 0x4C616220);
        c::cmsSetProfileVersion(outp, 4.4);
        c::cmsWriteTag(outp, 0x42324430, p as *const _); // 'B2D0'
        let r16 = (4u32 << 16) | (3 << 3) | 2;
        let l16 = (10u32 << 16) | (3 << 3) | 2;
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let t = c::cmsCreateTransform(lab, l16, outp, r16, 0, 0);
        eprintln!("b2d-out t={}", !t.is_null());
        if !t.is_null() { c::cmsDeleteTransform(t); }
        c::cmsCloseProfile(outp);
        c::cmsCloseProfile(lab);
        c::cmsPipelineFree(p);
    }
}

/// _cmsReadInputLUT per-intent dispatch — profiles with A2B0/1/2
/// tags selected by intent + the fallback chain.
#[test]
fn input_lut_intent_dispatch() {
    unsafe {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let l16 = (10u32 << 16) | (3 << 3) | 2;
        for f in ["lcms2cmyk.icc", "sRGBlcms2.icc", "aRGBlcms2.icc"] {
            let Ok(d) = std::fs::read(dir.join(f)) else { continue };
            let p = c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32);
            if p.is_null() { continue; }
            for intent in [0u32, 1, 2, 3] {
                let t = c::cmsCreateTransform(p,
                    (6u32 << 16) | (4 << 3) | 2, lab, l16, intent, 0);
                if !t.is_null() {
                    eprintln!("{f} i{intent} ok");
                    c::cmsDeleteTransform(t);
                }
            }
            c::cmsCloseProfile(p);
        }
        c::cmsCloseProfile(lab);
    }
}

/// _cmsReadOutputLUT per-intent dispatch — output profiles with
/// B2A0/1/2 tags selected by intent + gamut/preview fallbacks.
#[test]
fn output_lut_intent_dispatch() {
    unsafe {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let l16 = (10u32 << 16) | (3 << 3) | 2;
        for f in ["lcms2cmyk.icc", "lcms2link.icc", "labv2lcms2.icc"] {
            let Ok(d) = std::fs::read(dir.join(f)) else { continue };
            let p = c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32);
            if p.is_null() { continue; }
            for intent in [0u32, 1, 2, 3] {
                let t = c::cmsCreateTransform(lab, l16, p,
                    (6u32 << 16) | (4 << 3) | 2, intent, 0);
                if !t.is_null() {
                    eprintln!("{f} i{intent} ok");
                    c::cmsDeleteTransform(t);
                }
            }
            c::cmsCloseProfile(p);
        }
        c::cmsCloseProfile(lab);
    }
}

/// Formatter-for-profile helpers — FormatterForColorspaceOfProfile/
/// FormatterForPCSOfProfile across n_bytes x is_float.
#[test]
fn formatter_for_profile() {
    unsafe {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        let mut profs = vec![
            ("srgb", c::cmsCreate_sRGBProfile()),
            ("lab", c::cmsCreateLab4Profile(std::ptr::null_mut())),
        ];
        if let Ok(d) = std::fs::read(dir.join("lcms2cmyk.icc")) {
            let p = c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32);
            if !p.is_null() { profs.push(("cmyk", p)); }
        }
        for (name, p) in &profs {
            for nb in [1u32, 2, 4] {
                for fl in [0i32, 1] {
                    let fc = c::cmsFormatterForColorspaceOfProfile(
                        *p, nb, fl);
                    let fp = c::cmsFormatterForPCSOfProfile(*p, nb, fl);
                    if fc != 0 || fp != 0 {
                        eprintln!("{name} nb{nb} f{fl} cs={fc:#x} pcs={fp:#x}");
                    }
                }
            }
            c::cmsCloseProfile(*p);
        }
    }
}

/// _cmsReadOutputLUT gamut/preview fallback arms — profiles with
/// gamt/pre0-2 tags; intent selects them over B2A.
#[test]
fn output_lut_fallbacks() {
    unsafe {
        let ftab: Vec<f32> = (0..8*3)
            .map(|i| i as f32 / 24.0).collect();
        let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocCLutFloat(std::ptr::null_mut(), 2, 3, 3,
                ftab.as_ptr()));
        let prof = c::cmsCreate_sRGBProfile();
        c::cmsSetProfileVersion(prof, 4.4);
        // gamut + preview tags -> the fallback read arms
        for sig in [0x67616d74u32, 0x70726530, 0x70726531] {
            c::cmsWriteTag(prof, sig, p as *const _);
        }
        let r16 = (4u32 << 16) | (3 << 3) | 2;
        let l16 = (10u32 << 16) | (3 << 3) | 2;
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        for intent in [0u32, 1, 2, 3, 5, 6] {
            let t = c::cmsCreateTransform(lab, l16, prof, r16,
                intent, 0);
            eprintln!("i{intent} t={}", !t.is_null());
            if !t.is_null() { c::cmsDeleteTransform(t); }
        }
        c::cmsCloseProfile(prof);
        c::cmsCloseProfile(lab);
        c::cmsPipelineFree(p);
    }
}

/// _cmsReadDevicelinkLUT internals — link-class profiles with
/// A2B/AToB/DToB tags -> the devicelink tag dispatch.
#[test]
fn devicelink_lut_dispatch2() {
    unsafe {
        let ftab: Vec<f32> = (0..8*3)
            .map(|i| i as f32 / 24.0).collect();
        let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocCLutFloat(std::ptr::null_mut(), 2, 3, 3,
                ftab.as_ptr()));
        let r16 = (4u32 << 16) | (3 << 3) | 2;
        // link-class profiles with each tag sig
        for sig in [0x41324230u32, 0x44324230, 0x6B4C5546] {
            let dl = c::cmsCreateProfilePlaceholder(std::ptr::null_mut());
            c::cmsSetDeviceClass(dl, 0x6c696e6b);
            c::cmsSetColorSpace(dl, 0x52474220);
            c::cmsSetPCS(dl, 0x4C616220);
            c::cmsSetProfileVersion(dl, 4.4);
            c::cmsWriteTag(dl, sig, p as *const _);
            let t = c::cmsCreateTransform(dl, r16, dl, r16, 0, 0);
            eprintln!("sig{sig:#x} t={}", !t.is_null());
            if !t.is_null() { c::cmsDeleteTransform(t); }
            c::cmsCloseProfile(dl);
        }
        c::cmsPipelineFree(p);
    }
}

/// _cmsReadInputLUT matrix-shaper + gamut arms — matrix-shaper
/// profiles + gamut-check profiles -> the specific read arms.
#[test]
fn input_lut_matrix_shaper() {
    unsafe {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let l16 = (10u32 << 16) | (3 << 3) | 2;
        let r16 = (4u32 << 16) | (3 << 3) | 2;
        for f in ["sRGBlcms2.icc", "aRGBlcms2.icc", "graylcms2.icc",
                  "lcms2mtsh.icc", "lcms2matrix.icc"] {
            let Ok(d) = std::fs::read(dir.join(f)) else { continue };
            let p = c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32);
            if p.is_null() { continue; }
            for intent in [0u32, 1, 2, 3] {
                let t = c::cmsCreateTransform(p, r16, lab, l16,
                    intent, 0);
                if !t.is_null() { c::cmsDeleteTransform(t); }
            }
            eprintln!("{f} ok");
            c::cmsCloseProfile(p);
        }
        c::cmsCloseProfile(lab);
    }
}

/// _cmsReadGamutLUT/_cmsReadPreviewLUT — gamt/pre0-2 tagged
/// profiles via gamut-check + proofing transforms.
#[test]
fn gamut_preview_lut_readers2() {
    let _g = CTX_LOCK.lock().unwrap();
    unsafe {
        let ftab: Vec<f32> = (0..8*3)
            .map(|i| i as f32 / 24.0).collect();
        let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocCLutFloat(std::ptr::null_mut(), 2, 3, 3,
                ftab.as_ptr()));
        let prof = c::cmsCreate_sRGBProfile();
        c::cmsSetProfileVersion(prof, 4.4);
        c::cmsWriteTag(prof, 0x67616d74, p as *const _); // 'gamt'
        let r16 = (4u32 << 16) | (3 << 3) | 2;
        let l16 = (10u32 << 16) | (3 << 3) | 2;
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        // gamut-check transform (proof flag + gamut intent)
        let t = c::cmsCreateTransform(prof, r16, lab, l16, 0,
            0x00000400 | 0x00001000); // NOCACHE + GAMUTCHECK
        eprintln!("gamut t={}", !t.is_null());
        if !t.is_null() { c::cmsDeleteTransform(t); }
        c::cmsCloseProfile(prof);
        c::cmsCloseProfile(lab);
        c::cmsPipelineFree(p);
    }
}

/// Float-tag reader internals — DToB/BToD mpet tags trigger the
/// Lab/XYZ float-normalization stage insertion.
#[test]
fn float_reader_normalization() {
    unsafe {
        let ftab: Vec<f32> = (0..8*3)
            .map(|i| i as f32 / 24.0).collect();
        let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocCLutFloat(std::ptr::null_mut(), 2, 3, 3,
                ftab.as_ptr()));
        // input profile + DToB tag — triggers NormalizeToLabFloat
        let inp = c::cmsCreateProfilePlaceholder(std::ptr::null_mut());
        c::cmsSetDeviceClass(inp, 0x6d6e7472);
        c::cmsSetColorSpace(inp, 0x52474220);
        c::cmsSetPCS(inp, 0x4C616220);
        c::cmsSetProfileVersion(inp, 4.4);
        c::cmsWriteTag(inp, 0x44324230, p as *const _);
        let r16 = (4u32 << 16) | (3 << 3) | 2;
        let l16 = (10u32 << 16) | (3 << 3) | 2;
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let t = c::cmsCreateTransform(inp, r16, lab, l16, 0, 0);
        eprintln!("dtob-in t={}", !t.is_null());
        if !t.is_null() {
            let ib = [0x4000u16; 3];
            let mut ob = [0u16; 3];
            c::cmsDoTransform(t, ib.as_ptr() as *const _,
                ob.as_mut_ptr() as *mut _, 1);
            eprintln!("ob={ob:?}");
            c::cmsDeleteTransform(t);
        }
        c::cmsCloseProfile(inp);
        c::cmsCloseProfile(lab);
        c::cmsPipelineFree(p);
    }
}

/// _cmsReadFloatOutputTag normalization — BToD mpet output profile
/// transform + eval -> NormalizeFromLabFloat insertion.
#[test]
fn float_output_normalization() {
    unsafe {
        let ftab: Vec<f32> = (0..8*3)
            .map(|i| i as f32 / 24.0).collect();
        let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocCLutFloat(std::ptr::null_mut(), 2, 3, 3,
                ftab.as_ptr()));
        let outp = c::cmsCreateProfilePlaceholder(std::ptr::null_mut());
        c::cmsSetDeviceClass(outp, 0x6d6e7472);
        c::cmsSetColorSpace(outp, 0x52474220);
        c::cmsSetPCS(outp, 0x4C616220);
        c::cmsSetProfileVersion(outp, 4.4);
        c::cmsWriteTag(outp, 0x42324430, p as *const _); // 'B2D0'
        let r16 = (4u32 << 16) | (3 << 3) | 2;
        let l16 = (10u32 << 16) | (3 << 3) | 2;
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let t = c::cmsCreateTransform(lab, l16, outp, r16, 0, 0);
        eprintln!("b2d-out t={}", !t.is_null());
        if !t.is_null() {
            let ib = [0x8000u16; 3];
            let mut ob = [0u16; 3];
            c::cmsDoTransform(t, ib.as_ptr() as *const _,
                ob.as_mut_ptr() as *mut _, 1);
            eprintln!("ob={ob:?}");
            c::cmsDeleteTransform(t);
        }
        c::cmsCloseProfile(outp);
        c::cmsCloseProfile(lab);
        c::cmsPipelineFree(p);
    }
}

/// _cmsReadDevicelinkLUT/_cmsReadOutputLUT remaining arms —
/// devicelink + output profiles with varied tag sets.
#[test]
fn devicelink_output_remaining() {
    unsafe {
        let ftab: Vec<f32> = (0..8*3)
            .map(|i| i as f32 / 24.0).collect();
        let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocCLutFloat(std::ptr::null_mut(), 2, 3, 3,
                ftab.as_ptr()));
        // devicelink with AToB0 tag
        let dl = c::cmsCreateProfilePlaceholder(std::ptr::null_mut());
        c::cmsSetDeviceClass(dl, 0x6c696e6b);
        c::cmsSetColorSpace(dl, 0x52474220);
        c::cmsSetPCS(dl, 0x4C616220);
        c::cmsSetProfileVersion(dl, 4.4);
        c::cmsWriteTag(dl, 0x41324230, p as *const _);
        let r16 = (4u32 << 16) | (3 << 3) | 2;
        let t = c::cmsCreateTransform(dl, r16, dl, r16, 0, 0);
        eprintln!("dl t={}", !t.is_null());
        if !t.is_null() { c::cmsDeleteTransform(t); }
        c::cmsCloseProfile(dl);
        c::cmsPipelineFree(p);
    }
}

/// _cmsReadInputLUT/_cmsReadOutputLUT remaining arms — named-color
/// profile input + preview-profile + devicelink variations.
#[test]
fn lut_reader_remaining_arms() {
    unsafe {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let l16 = (10u32 << 16) | (3 << 3) | 2;
        let r16 = (4u32 << 16) | (3 << 3) | 2;
        // named-color input (nmcl profile with ncl2 tag)
        let ncl = c::cmsAllocNamedColorList(std::ptr::null_mut(), 1,
            3, b"p\0".as_ptr(), b"s\0".as_ptr());
        let lab16 = [0xF000u16; 3];
        c::cmsAppendNamedColor(ncl, b"c1\0".as_ptr(),
            lab16.as_ptr(), lab16.as_ptr());
        let nclp = c::cmsCreateProfilePlaceholder(std::ptr::null_mut());
        c::cmsSetDeviceClass(nclp, 0x6e6d636c);
        c::cmsSetColorSpace(nclp, 0x6e6d636c);
        c::cmsSetPCS(nclp, 0x4C616220);
        c::cmsWriteTag(nclp, 0x6e636c32, ncl as *const _);
        let ncf = (0xFFu32 << 16) | (1 << 3) | 1;
        let t = c::cmsCreateTransform(nclp, ncf, lab, l16, 0, 0);
        eprintln!("ncl t={}", !t.is_null());
        if !t.is_null() { c::cmsDeleteTransform(t); }
        c::cmsCloseProfile(nclp);
        c::cmsFreeNamedColorList(ncl);
        c::cmsCloseProfile(lab);
    }
}

/// _cmsReadOutputLUT internals — Lab-PCS output + Lut16-type
/// output tags -> the normalization read arms.
#[test]
fn output_lut_internals() {
    unsafe {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let l16 = (10u32 << 16) | (3 << 3) | 2;
        // output-position corpus profiles exercise the reader
        for f in ["lcms2cmyk.icc", "labv2lcms2.icc", "graylcms2.icc",
                  "lcms2link.icc", "lcms2mtsh.icc"] {
            let Ok(d) = std::fs::read(dir.join(f)) else { continue };
            let p = c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32);
            if p.is_null() { continue; }
            for intent in [0u32, 1, 2, 3] {
                let t = c::cmsCreateTransform(lab, l16, p,
                    (4u32 << 16) | (3 << 3) | 2, intent, 0);
                if !t.is_null() { c::cmsDeleteTransform(t); }
            }
            eprintln!("{f} ok");
            c::cmsCloseProfile(p);
        }
        c::cmsCloseProfile(lab);
    }
}

/// Float-tag reader normalization arms — Lab-PCS + XYZ-PCS float
/// tags -> NormalizeTo/From{Lab,Xyz}Float insertion.
#[test]
fn float_normalization_arms() {
    unsafe {
        let ftab: Vec<f32> = (0..8*3)
            .map(|i| i as f32 / 24.0).collect();
        let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocCLutFloat(std::ptr::null_mut(), 2, 3, 3,
                ftab.as_ptr()));
        let r16 = (4u32 << 16) | (3 << 3) | 2;
        let l16 = (10u32 << 16) | (3 << 3) | 2;
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        // Lab-PCS + XYZ-PCS input profiles
        for (pcs, name) in [(0x4C616220u32, "lab-pcs"),
                            (0x58595A20, "xyz-pcs")] {
            let inp = c::cmsCreateProfilePlaceholder(std::ptr::null_mut());
            c::cmsSetDeviceClass(inp, 0x6d6e7472);
            c::cmsSetColorSpace(inp, 0x52474220);
            c::cmsSetPCS(inp, pcs);
            c::cmsSetProfileVersion(inp, 4.4);
            c::cmsWriteTag(inp, 0x44324230, p as *const _);
            let t = c::cmsCreateTransform(inp, r16, lab, l16, 0, 0);
            eprintln!("{name} t={}", !t.is_null());
            if !t.is_null() { c::cmsDeleteTransform(t); }
            c::cmsCloseProfile(inp);
        }
        c::cmsCloseProfile(lab);
        c::cmsPipelineFree(p);
    }
}

/// _cmsReadFloatDevicelinkTag internals — a devicelink profile
/// with DToB/BToD tags + transform -> the float devicelink reader.
#[test]
fn float_devicelink_reader() {
    unsafe {
        let ftab: Vec<f32> = (0..8*3)
            .map(|i| i as f32 / 24.0).collect();
        let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocCLutFloat(std::ptr::null_mut(), 2, 3, 3,
                ftab.as_ptr()));
        for sig in [0x44324230u32, 0x42324430] { // 'D2B0','B2D0'
            let dl = c::cmsCreateProfilePlaceholder(std::ptr::null_mut());
            c::cmsSetDeviceClass(dl, 0x6c696e6b);
            c::cmsSetColorSpace(dl, 0x52474220);
            c::cmsSetPCS(dl, 0x4C616220);
            c::cmsSetProfileVersion(dl, 4.4);
            c::cmsWriteTag(dl, sig, p as *const _);
            let r16 = (4u32 << 16) | (3 << 3) | 2;
            let t = c::cmsCreateTransform(dl, r16, dl, r16, 0, 0);
            eprintln!("sig{sig:#x} t={}", !t.is_null());
            if !t.is_null() { c::cmsDeleteTransform(t); }
            c::cmsCloseProfile(dl);
        }
        c::cmsPipelineFree(p);
    }
}

/// _cmsReadPreviewLUT — pre0-2 tagged profile via proofing
/// transform.
#[test]
fn preview_lut_reader() {
    let _g = CTX_LOCK.lock().unwrap();
    unsafe {
        let ftab: Vec<f32> = (0..8*3)
            .map(|i| i as f32 / 24.0).collect();
        let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocCLutFloat(std::ptr::null_mut(), 2, 3, 3,
                ftab.as_ptr()));
        let prof = c::cmsCreate_sRGBProfile();
        c::cmsSetProfileVersion(prof, 4.4);
        c::cmsWriteTag(prof, 0x70726530, p as *const _); // 'pre0'
        let r16 = (4u32 << 16) | (3 << 3) | 2;
        let l16 = (10u32 << 16) | (3 << 3) | 2;
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        // proofing transform — the preview path
        let pt = c::cmsCreateProofingTransform(prof, r16,
            lab, l16, prof, 0, 0, 0);
        eprintln!("proof t={}", !pt.is_null());
        if !pt.is_null() { c::cmsDeleteTransform(pt); }
        c::cmsCloseProfile(prof);
        c::cmsCloseProfile(lab);
        c::cmsPipelineFree(p);
    }
}

/// _cmsReadInputLUT/_cmsReadOutputLUT internals — matrix-shaper,
/// gamut, preview, named-color, devicelink profile classes.
#[test]
fn lut_reader_dispatch_all() {
    unsafe {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let l16 = (10u32 << 16) | (3 << 3) | 2;
        let r16 = (4u32 << 16) | (3 << 3) | 2;
        // every corpus profile in input + output positions
        for f in ["lcms2cmyk.icc", "sRGBlcms2.icc", "aRGBlcms2.icc",
                  "lcms2link.icc", "labv2lcms2.icc", "graylcms2.icc",
                  "lcms2mtsh.icc", "lcms2matrix.icc"] {
            let Ok(d) = std::fs::read(dir.join(f)) else { continue };
            let p = c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32);
            if p.is_null() { continue; }
            for (inp, outp, if_, of) in [
                (p, lab, r16, l16),      // input
                (lab, p, l16, r16),      // output
            ] {
                let t = c::cmsCreateTransform(inp, if_, outp, of,
                    0, 0);
                if !t.is_null() { c::cmsDeleteTransform(t); }
            }
            eprintln!("{f} ok");
            c::cmsCloseProfile(p);
        }
        c::cmsCloseProfile(lab);
    }
}
