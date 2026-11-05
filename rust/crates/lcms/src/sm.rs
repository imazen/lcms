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

/// `cmsDesaturateLab` — clip Lab a/b into the [amin,amax]×[bmin,bmax] box
pub fn desaturate_lab(lab: &mut CieLab, amax: f64, amin: f64, bmax: f64, bmin: f64) -> bool {
    if lab.L < 0.0 {
        lab.L = 0.0;
        lab.a = 0.0;
        lab.b = 0.0;
        return false;
    }
    if lab.L > 100.0 {
        lab.L = 100.0;
    }
    if lab.a < amin || lab.a > amax || lab.b < bmin || lab.b > bmax {
        if lab.a == 0.0 {
            lab.b = if lab.b < 0.0 { bmin } else { bmax };
            return true;
        }
        let lch = crate::pcs::lab2lch(lab);
        let slope = lab.b / lab.a;
        let h = lch.h;
        if (h >= 0.0 && h < 45.0) || (h >= 315.0 && h <= 360.0) {
            lab.a = amax;
            lab.b = amax * slope;
        } else if h >= 45.0 && h < 135.0 {
            lab.b = bmax;
            lab.a = bmax / slope;
        } else if h >= 135.0 && h < 225.0 {
            lab.a = amin;
            lab.b = amin * slope;
        } else if h >= 225.0 && h < 315.0 {
            lab.b = bmin;
            lab.a = bmin / slope;
        } else {
            return false;
        }
    }
    true
}

/// `GAMUTCHAIN` — the three gamut-check transforms + threshold
struct GamutChain {
    h_input: crate::xform::Transform,
    h_forward: crate::xform::Transform,
    h_reverse: crate::xform::Transform,
    threshold: f64,
}

/// `GamutSampler` — round-trip each Lab point and score the drift
fn gamut_sampler(inp: &[u16], out: &mut [u16], chain: &mut GamutChain) -> i32 {
    use crate::types::CieLab;
    let mut lab_in1 = [0u8; 24];
    let mut proof = [0u8; 32];
    let mut lab_out1 = [0u8; 24];
    let mut proof2 = [0u8; 32];
    let mut lab_out2 = [0u8; 24];
    let err_threshold = 5.0;
    let _ = err_threshold;
    let mut buf = Vec::new();
    for &b in inp.iter().take(3) {
        buf.extend_from_slice(&b.to_le_bytes());
    }
    chain.h_input.do_transform(&buf, &mut lab_in1, 1);
    chain.h_forward.do_transform(&lab_in1, &mut proof, 1);
    chain.h_reverse.do_transform(&proof, &mut lab_out1, 1);
    let lab_in2 = lab_out1;
    chain.h_forward.do_transform(&lab_out1, &mut proof2, 1);
    chain.h_reverse.do_transform(&proof2, &mut lab_out2, 1);
    let d = |a: &[u8], b: &[u8]| {
        crate::pcs::delta_e(
            &CieLab {
                L: f64::from_le_bytes(a[0..8].try_into().unwrap()),
                a: f64::from_le_bytes(a[8..16].try_into().unwrap()),
                b: f64::from_le_bytes(a[16..24].try_into().unwrap()),
            },
            &CieLab {
                L: f64::from_le_bytes(b[0..8].try_into().unwrap()),
                a: f64::from_le_bytes(b[8..16].try_into().unwrap()),
                b: f64::from_le_bytes(b[16..24].try_into().unwrap()),
            },
        )
    };
    let d_e1 = d(&lab_in1, &lab_out1);
    let d_e2 = d(&lab_in2, &lab_out2);
    let t = chain.threshold;
    let mut error_ratio = 1.0f64;
    if d_e1 < t && d_e2 < t {
        out[0] = 0;
    } else if d_e1 < t && d_e2 > t {
        out[0] = 0;
    } else if d_e1 > t && d_e2 < t {
        out[0] = (d_e1 - t + 0.5).floor() as u16;
    } else {
        if d_e2 == 0.0 {
            error_ratio = d_e1;
        } else {
            error_ratio = d_e1 / d_e2;
        }
        if error_ratio > t {
            out[0] = (error_ratio - t + 0.5).floor() as u16;
        } else {
            out[0] = 0;
        }
    }
    1
}

/// serialize+reopen a profile (for `create_extended_transform`'s owned vec)
fn dup_profile(icc: &mut IccProfile) -> Option<IccProfile> {
    let bytes = crate::profile::save_profile_to_mem(icc)?;
    crate::profile::open_profile_from_mem(None, &bytes).map(|p| *p)
}

/// `_cmsCreateGamutCheckPipeline` — the proof chain → a 3→1 CLUT
pub fn create_gamut_check_pipeline(
    ctx: Option<&crate::context::Context>,
    profiles: &mut [Box<IccProfile>],
    bpc: &mut [bool],
    intents: &[u32],
    adaptation_states: &[f64],
    n_gamut_pcs_position: u32,
    h_gamut: &mut IccProfile,
) -> Option<crate::lut::Pipeline> {
    use crate::pack;
    use crate::signatures::colorspace as cs;
    let n = profiles.len();
    if n_gamut_pcs_position == 0 || n_gamut_pcs_position > 255 {
        crate::err::signal_error(ctx, crate::types::ERROR_RANGE, "Wrong position of PCS");
        return None;
    }
    let pos = n_gamut_pcs_position as usize;
    let mut hlab = crate::virt::create_lab4_profile(ctx.cloned(), None)?;
    let threshold = if is_matrix_shaper(h_gamut) { 1.0 } else { 5.0 };
    // input chain: profiles[0..pos] + Lab4 → Lab_DBL
    let mut chain: Vec<Box<IccProfile>> = Vec::new();
    for i in 0..pos.min(n) {
        chain.push(Box::new(dup_profile(&mut profiles[i])?));
    }
    chain.push(hlab);
    let mut bpc_l = vec![false; pos + 1];
    bpc_l[..pos].copy_from_slice(&bpc[..pos]);
    let mut intents_l = intents[..pos].to_vec();
    intents_l.push(INTENT_RELATIVE_COLORIMETRIC);
    let mut states_l = adaptation_states[..pos].to_vec();
    states_l.push(1.0);
    let input_cs = chain[0].color_space;
    let n_in = pack::t_channels(formatter_for_colorspace(&chain[0], 2, false)) as usize;
    let type_lab_dbl: u32 = 1 << 22 | 10 << 16 | 3 << 3 | 0;
    let dw_fmt = ((n_in as u32) << 3) | 2;
    let mut h_input = crate::xform::create_extended_transform(
        ctx.cloned(),
        &mut chain,
        &mut bpc_l,
        &intents_l,
        &states_l,
        None,
        0,
        dw_fmt,
        type_lab_dbl,
        crate::xform::FLAGS_NOCACHE,
    )?;
    // forward: Lab → gamut-profile output
    let gamut_cs = h_gamut.color_space;
    let n_ch = crate::pcs::channels_of_color_space(gamut_cs) as usize;
    let dw_fmt2 = ((n_ch as u32) << 3) | 2;
    let mut hlab2 = crate::virt::create_lab4_profile(ctx.cloned(), None)?;
    let mut pf = vec![hlab2, Box::new(dup_profile(h_gamut)?)];
    let mut bpc2 = [false; 2];
    let intents2 = [INTENT_RELATIVE_COLORIMETRIC; 2];
    let states2 = [1.0f64; 2];
    let mut h_forward = crate::xform::create_extended_transform(
        ctx.cloned(),
        &mut pf,
        &mut bpc2,
        &intents2,
        &states2,
        None,
        0,
        type_lab_dbl,
        dw_fmt2,
        crate::xform::FLAGS_NOCACHE,
    )?;
    // reverse: gamut → Lab
    let mut hlab3 = crate::virt::create_lab4_profile(ctx.cloned(), None)?;
    let mut pr = vec![Box::new(dup_profile(h_gamut)?), hlab3];
    let mut h_reverse = crate::xform::create_extended_transform(
        ctx.cloned(),
        &mut pr,
        &mut bpc2,
        &intents2,
        &states2,
        None,
        0,
        dw_fmt2,
        type_lab_dbl,
        crate::xform::FLAGS_NOCACHE,
    )?;
    let mut chain_obj = GamutChain {
        h_input: *h_input,
        h_forward: *h_forward,
        h_reverse: *h_reverse,
        threshold,
    };
    let mut gamut = crate::lut::pipeline_alloc(ctx, 3, 1)?;
    let n_grid = crate::pcs::reasonable_gridpoints_by_colorspace(
        gamut_cs,
        crate::xform::FLAGS_HIGHRESPRECALC,
    );
    let mut clut = crate::lut::stage_alloc_clut16bit(ctx, n_grid, n_ch as u32, 1, None)?;
    if !gamut.insert_stage(crate::lut::StageLoc::AtBegin, clut) {
        return None;
    }
    let grid = vec![n_grid; n_ch];
    let ok = crate::lut::slice_space16(n_ch as u32, &grid, &mut |inp: &[u16], _o: &mut [u16]| {
        let mut out = [0u16; 1];
        gamut_sampler(inp, &mut out, &mut chain_obj);
        let mut node = 0usize;
        for t in 0..n_ch {
            node = node * n_grid as usize + (inp[t] as u32 * (n_grid - 1) / 0xffff) as usize;
        }
        if let Some(s) = gamut.elements.as_deref_mut() {
            if let crate::lut::StageData::CLut(d) = &mut s.data {
                if let Some(tab) = d.tab16.as_deref_mut() {
                    tab[node] = out[0];
                }
            }
        }
        1
    });
    if !ok {
        return None;
    }
    Some(gamut)
}
