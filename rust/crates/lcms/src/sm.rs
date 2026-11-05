//! `cmssamp.c` — black-point / TAC detection via round-trip transforms.
//! The transforms recurse back through `xform` — safe because the inner
//! transforms use RELATIVE intent (BPC off) or the perceptual-black path.

use crate::profile::IccProfile;
use crate::types::{
    CieLab, CieXyz, INTENT_PERCEPTUAL, INTENT_RELATIVE_COLORIMETRIC, INTENT_SATURATION,
};
use crate::virt;

pub const PERCEPTUAL_BLACK: CieXyz = CieXyz {
    X: 0.00336,
    Y: 0.0034731,
    Z: 0.00287,
};

/// `cmsIsMatrixShaper` — TRCs + colorants present for gray/RGB
fn is_matrix_shaper(icc: &IccProfile) -> bool {
    use crate::signatures::colorspace as cs;
    match icc.color_space {
        x if x == cs::SigGrayData => {
            crate::profile::cms_is_tag(icc, crate::signatures::tag::SigGrayTRCTag)
        }
        x if x == cs::SigRgbData => {
            crate::profile::cms_is_tag(icc, crate::signatures::tag::SigRedColorantTag)
                && crate::profile::cms_is_tag(icc, crate::signatures::tag::SigGreenColorantTag)
                && crate::profile::cms_is_tag(icc, crate::signatures::tag::SigBlueColorantTag)
                && crate::profile::cms_is_tag(icc, crate::signatures::tag::SigRedTRCTag)
                && crate::profile::cms_is_tag(icc, crate::signatures::tag::SigGreenTRCTag)
                && crate::profile::cms_is_tag(icc, crate::signatures::tag::SigBlueTRCTag)
        }
        _ => false,
    }
}

/// `cmsFormatterForColorspaceOfProfile` — bytes=2 nonfloat
fn formatter_for_colorspace(icc: &IccProfile, n_bytes: u32, is_float: bool) -> u32 {
    let color_space = icc.color_space;
    let color_bits = crate::pcs::lcms_color_space(color_space);
    if color_bits < 0 {
        return 0;
    }
    let color_bits = color_bits as u32;
    let n_ch = crate::pcs::channels_of_color_space(color_space);
    if n_ch < 0 {
        return 0;
    }
    let nb = n_bytes & 7;
    (is_float as u32) << 22 | color_bits << 16 | ((n_ch as u32) << 3) | nb
}

/// `_cmsLCMScolorSpace` inverse — colorspace signature → PT_* enum
/// `CreateRoundtripXForm` — Lab4→prof→prof→Lab4 (rel, intent, rel, rel)
fn create_roundtrip_xform(icc: &mut IccProfile, intent: u32) -> Option<crate::xform::Transform> {
    let mut hlab = virt::create_lab4_profile(None, None)?;
    // TYPE_Lab_DBL = float|colorspace(Lab)|3chan|bytes0
    let type_lab_dbl: u32 = 1 << 22 | 10 << 16 | 3 << 3 | 0;
    let mut hlab2 = virt::create_lab4_profile(None, None)?;
    // the round-trip uses the same profile twice — duplicate via serialize+reopen
    let bytes = crate::profile::save_profile_to_mem(icc)?;
    let prof1 = crate::profile::open_profile_from_mem(None, &bytes)?;
    let prof2 = crate::profile::open_profile_from_mem(None, &bytes)?;
    let mut profs: Vec<Box<IccProfile>> = Vec::new();
    profs.push(hlab);
    profs.push(prof1);
    profs.push(prof2);
    profs.push(hlab2);
    let mut bpc = [false; 4];
    let intents = [
        INTENT_RELATIVE_COLORIMETRIC,
        intent,
        INTENT_RELATIVE_COLORIMETRIC,
        INTENT_RELATIVE_COLORIMETRIC,
    ];
    let states = [1.0f64; 4];
    crate::xform::create_extended_transform(
        None,
        &mut profs,
        &mut bpc,
        &intents,
        &states,
        None,
        0,
        type_lab_dbl,
        type_lab_dbl,
        crate::xform::FLAGS_NOCACHE | crate::xform::FLAGS_NOOPTIMIZE,
    )
    .map(|t| *t)
}

/// `BlackPointUsingPerceptualBlack` — eval Lab(0,0,0) through the round-trip
fn black_point_using_perceptual_black(icc: &mut IccProfile) -> Option<CieXyz> {
    let mut t = create_roundtrip_xform(icc, INTENT_PERCEPTUAL)?;
    let lab_in = CieLab {
        L: 0.0,
        a: 0.0,
        b: 0.0,
    };
    let mut lab_out = CieLab {
        L: 0.0,
        a: 0.0,
        b: 0.0,
    };
    // do_transform on a Lab_DBL buffer
    let mut buf = [0u8; 24];
    buf[0..8].copy_from_slice(&lab_in.L.to_le_bytes());
    buf[8..16].copy_from_slice(&lab_in.a.to_le_bytes());
    buf[16..24].copy_from_slice(&lab_in.b.to_le_bytes());
    let mut out = [0u8; 24];
    t.do_transform(&buf, &mut out, 1);
    lab_out.L = f64::from_le_bytes(out[0..8].try_into().unwrap());
    lab_out.a = f64::from_le_bytes(out[8..16].try_into().unwrap());
    lab_out.b = f64::from_le_bytes(out[16..24].try_into().unwrap());
    if lab_out.L > 50.0 {
        lab_out.L = 50.0;
    }
    lab_out.a = 0.0;
    lab_out.b = 0.0;
    Some(crate::pcs::lab2xyz(None, &lab_out))
}

/// `BlackPointAsDarkerColorant` — the ink-space black endpoint through the transform
fn black_point_as_darker_colorant(icc: &mut IccProfile, intent: u32) -> Option<CieXyz> {
    let fmt = formatter_for_colorspace(icc, 2, false);
    let space = icc.color_space;
    let Some((_, black, n_ch)) = crate::pcs::end_points_by_space(space) else {
        return Some(CieXyz {
            X: 0.0,
            Y: 0.0,
            Z: 0.0,
        })
        .filter(|_| false);
    };
    if n_ch != crate::pack::t_channels(fmt) {
        return None;
    }
    let mut hlab = virt::create_lab2_profile(None, None)?;
    let type_lab_dbl: u32 = 1 << 22 | 10 << 16 | 3 << 3 | 0;
    let bytes = crate::profile::save_profile_to_mem(icc)?;
    let prof = crate::profile::open_profile_from_mem(None, &bytes)?;
    let mut profs = vec![prof, hlab];
    let mut bpc = [false; 2];
    let intents = [intent, INTENT_RELATIVE_COLORIMETRIC];
    let states = [1.0f64; 2];
    let mut t = crate::xform::create_extended_transform(
        None,
        &mut profs,
        &mut bpc,
        &intents,
        &states,
        None,
        0,
        fmt,
        type_lab_dbl,
        crate::xform::FLAGS_NOCACHE | crate::xform::FLAGS_NOOPTIMIZE,
    )?;
    // eval the black endpoint through the transform (16-bit in, Lab double out)
    let mut inbuf = Vec::new();
    for &b in black.iter().take(n_ch as usize) {
        inbuf.extend_from_slice(&b.to_le_bytes());
    }
    let mut out = [0u8; 24];
    t.do_transform(&inbuf, &mut out, 1);
    let mut lab = CieLab {
        L: f64::from_le_bytes(out[0..8].try_into().unwrap()),
        a: f64::from_le_bytes(out[8..16].try_into().unwrap()),
        b: f64::from_le_bytes(out[16..24].try_into().unwrap()),
    };
    lab.a = 0.0;
    lab.b = 0.0;
    if lab.L > 95.0 {
        lab.L = 0.0;
    } else if lab.L < 0.0 {
        lab.L = 0.0;
    } else if lab.L > 50.0 {
        lab.L = 50.0;
    }
    Some(crate::pcs::lab2xyz(None, &lab))
}

/// `cmsDetectBlackPoint`
pub fn detect_black_point(icc: &mut IccProfile, intent: u32, dw_flags: u32) -> Option<CieXyz> {
    use crate::signatures::profileclass as pc;
    let dc = icc.device_class;
    if dc == pc::SigLinkClass || dc == pc::SigAbstractClass || dc == pc::SigNamedColorClass {
        return None;
    }
    if intent != INTENT_PERCEPTUAL
        && intent != INTENT_RELATIVE_COLORIMETRIC
        && intent != INTENT_SATURATION
    {
        return None;
    }
    if crate::profile::get_encoded_icc_version(icc) >= 0x4000000
        && (intent == INTENT_PERCEPTUAL || intent == INTENT_SATURATION)
    {
        if is_matrix_shaper(icc) {
            return black_point_as_darker_colorant(icc, INTENT_RELATIVE_COLORIMETRIC);
        }
        return Some(PERCEPTUAL_BLACK);
    }
    // fall through: perceptual-black round-trip or the tag path
    black_point_using_perceptual_black(icc)
}

/// `cmsDetectDestinationBlackPoint` — the ink-space round-trip ramp
pub fn detect_destination_black_point(
    icc: &mut IccProfile,
    intent: u32,
    dw_flags: u32,
) -> Option<CieXyz> {
    use crate::signatures::profileclass as pc;
    let dc = icc.device_class;
    if dc == pc::SigLinkClass || dc == pc::SigAbstractClass || dc == pc::SigNamedColorClass {
        return None;
    }
    if intent != INTENT_PERCEPTUAL
        && intent != INTENT_RELATIVE_COLORIMETRIC
        && intent != INTENT_SATURATION
    {
        return None;
    }
    if crate::profile::get_encoded_icc_version(icc) >= 0x4000000
        && (intent == INTENT_PERCEPTUAL || intent == INTENT_SATURATION)
    {
        if is_matrix_shaper(icc) {
            return black_point_as_darker_colorant(icc, INTENT_RELATIVE_COLORIMETRIC);
        }
        return Some(PERCEPTUAL_BLACK);
    }
    let space = icc.color_space;
    let ink = crate::pack::is_ink_space(space);
    if !ink
        && space != crate::signatures::colorspace::SigGrayData
        && space != crate::signatures::colorspace::SigRgbData
    {
        return detect_black_point(icc, intent, dw_flags);
    }
    let initial_lab = if intent == INTENT_RELATIVE_COLORIMETRIC {
        let ini = detect_black_point(icc, intent, dw_flags)?;
        crate::pcs::xyz2lab(None, &ini)
    } else {
        CieLab {
            L: 0.0,
            a: 0.0,
            b: 0.0,
        }
    };
    let mut t = create_roundtrip_xform(icc, intent)?;
    let mut in_ramp = [0f64; 256];
    let mut out_ramp = [0f64; 256];
    for l in 0..256 {
        let lab = CieLab {
            L: l as f64 * 100.0 / 255.0,
            a: initial_lab.a.clamp(-50.0, 50.0),
            b: initial_lab.b.clamp(-50.0, 50.0),
        };
        let mut buf = [0u8; 24];
        buf[0..8].copy_from_slice(&lab.L.to_le_bytes());
        buf[8..16].copy_from_slice(&lab.a.to_le_bytes());
        buf[16..24].copy_from_slice(&lab.b.to_le_bytes());
        let mut out = [0u8; 24];
        t.do_transform(&buf, &mut out, 1);
        let dl = f64::from_le_bytes(out[0..8].try_into().unwrap());
        in_ramp[l] = lab.L;
        out_ramp[l] = dl;
    }
    for l in (0..255).rev() {
        out_ramp[l] = out_ramp[l].min(out_ramp[l + 1]);
    }
    if !(out_ramp[0] < out_ramp[255]) {
        return None;
    }
    let min_l = out_ramp[0];
    let max_l = out_ramp[255];
    if intent == INTENT_RELATIVE_COLORIMETRIC {
        let mut straight = true;
        for l in 0..256 {
            if !((in_ramp[l] <= min_l + 0.2 * (max_l - min_l))
                || (in_ramp[l] - out_ramp[l]).abs() < 4.0)
            {
                straight = false;
            }
        }
        if straight {
            return Some(crate::pcs::lab2xyz(None, &initial_lab));
        }
    }
    let mut y_ramp = [0f64; 256];
    for l in 0..256 {
        y_ramp[l] = (out_ramp[l] - min_l) / (max_l - min_l);
    }
    let (lo, hi) = if intent == INTENT_RELATIVE_COLORIMETRIC {
        (0.1, 0.5)
    } else {
        (0.03, 0.25)
    };
    let mut x = [0f64; 256];
    let mut y = [0f64; 256];
    let mut n = 0usize;
    for l in 0..256 {
        if y_ramp[l] >= lo && y_ramp[l] < hi {
            x[n] = in_ramp[l];
            y[n] = y_ramp[l];
            n += 1;
        }
    }
    if n < 3 {
        return None;
    }
    let l_root = root_of_least_squares_fit_quadratic(n, &x, &y);
    let lab = CieLab {
        L: if l_root < 0.0 { 0.0 } else { l_root },
        a: initial_lab.a,
        b: initial_lab.b,
    };
    Some(crate::pcs::lab2xyz(None, &lab))
}

/// `RootOfLeastSquaresFitQuadraticCurve` — quadratic fit root
fn root_of_least_squares_fit_quadratic(n: usize, x: &[f64], y: &[f64]) -> f64 {
    if n < 4 {
        return 0.0;
    }
    let (mut sx, mut sx2, mut sx3, mut sx4, mut sy, mut syx, mut syx2) =
        (0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    for i in 0..n {
        let xn = x[i];
        let yn = y[i];
        sx += xn;
        sx2 += xn * xn;
        sx3 += xn * xn * xn;
        sx4 += xn * xn * xn * xn;
        sy += yn;
        syx += yn * xn;
        syx2 += yn * xn * xn;
    }
    let mut m = crate::types::Mat3::default();
    m.v[0].n = [n as f64, sx, sx2];
    m.v[1].n = [sx, sx2, sx3];
    m.v[2].n = [sx2, sx3, sx4];
    let v = crate::types::Vec3 { n: [sy, syx, syx2] };
    let mut res = crate::types::Vec3::default();
    if !crate::mtrx::mat3_solve(&mut res, &m, &v) {
        return 0.0;
    }
    let (a, b, c) = (res.n[2], res.n[1], res.n[0]);
    if a.abs() < 1.0e-10 {
        if b.abs() < 1.0e-10 {
            return 0.0;
        }
        (-c / b).clamp(0.0, 50.0)
    } else {
        let d = b * b - 4.0 * a * c;
        if d <= 0.0 {
            0.0
        } else {
            ((-b + d.sqrt()) / (2.0 * a)).clamp(0.0, 50.0)
        }
    }
}
