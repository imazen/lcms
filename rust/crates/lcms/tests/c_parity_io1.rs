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
