//! `cmsvirt.c` — virtual profile constructors.
//!
//! Mirrors the C file: each constructor fills a profile placeholder with
//! text tags, colorants/TRCs, and optional AToB0/BToA0 pipelines.

use crate::context::Context;
use crate::fixed::quick_saturate_word;
use crate::gamma::{ToneCurve, build_parametric_tone_curve};
use crate::lut::{
    StageLoc, pipeline_alloc, stage_alloc_clut16bit, stage_alloc_identity_clut,
    stage_alloc_identity_curves, stage_alloc_tone_curves, stage_sample_clut16bit,
};
use crate::named::{Wchar, alloc_profile_seq_desc, mlu_alloc, mlu_set_ascii, mlu_set_wide};
use crate::pcs::{channels_of_color_space, float2lab_encoded, lab2lch, lab2xyz, lch2lab, xyz2lab};
use crate::profile::{self, IccProfile, cms_link_tag, cms_write_tag, create_profile_placeholder};
use crate::signatures::colorspace as cs;
use crate::signatures::profileclass as pc;
use crate::signatures::tag as tags;
use crate::tagtypes::TagObject;
use crate::types::{CieXyz, CiexyY, CiexyYTriple, Mat3};
use crate::wtpnt::{adaptation_matrix, build_rgb2xyz_transfer_matrix, d50_xyy, d50_xyz};

pub const INTENT_PERCEPTUAL: u32 = 0;
pub const INTENT_RELATIVE_COLORIMETRIC: u32 = 3;

/// `SetTextTags` — desc + copyright MLU tags
fn set_text_tags(icc: &mut IccProfile, desc: &[Wchar]) -> bool {
    let Some(mut d) = mlu_alloc(None, 1) else {
        return false;
    };
    let Some(mut c) = mlu_alloc(None, 1) else {
        return false;
    };
    if !mlu_set_wide(&mut d, b"en", b"US", desc) {
        return false;
    }
    if !mlu_set_wide(&mut c, b"en", b"US", &wide("No copyright, use freely")) {
        return false;
    }
    cms_write_tag(icc, tags::SigProfileDescriptionTag, Some(TagObject::Mlu(d)))
        && cms_write_tag(icc, tags::SigCopyrightTag, Some(TagObject::Mlu(c)))
}

/// UTF-16 encode a &str (the L"" literals in C)
fn wide(s: &str) -> Vec<Wchar> {
    s.chars().map(|c| c as Wchar).collect()
}

/// `SetSeqDescTag` — profile sequence with lcms manufacturer + model
fn set_seq_desc_tag(icc: &mut IccProfile, model: &str) -> bool {
    let Some(mut seq) = alloc_profile_seq_desc(None, 1) else {
        return false;
    };
    seq.seq[0].device_mfg = 0;
    seq.seq[0].device_model = 0;
    seq.seq[0].attributes = 0;
    seq.seq[0].technology = 0;
    let Some(mut mfg) = mlu_alloc(None, 1) else {
        return false;
    };
    let Some(mut mdl) = mlu_alloc(None, 1) else {
        return false;
    };
    mlu_set_ascii(&mut mfg, b"en", b"US", b"Little CMS");
    mlu_set_ascii(&mut mdl, b"en", b"US", model.as_bytes());
    seq.seq[0].manufacturer = Some(mfg);
    seq.seq[0].model = Some(mdl);
    profile::cms_write_tag(
        icc,
        tags::SigProfileSequenceDescTag,
        Some(TagObject::ProfileSeqDesc(seq)),
    )
}

/// `cmsCreateRGBProfileTHR` — `transfer` compares by pointer like C
pub fn create_rgb_profile(
    ctx: Option<Context>,
    white_point: Option<&CiexyY>,
    primaries: Option<&CiexyYTriple>,
    transfer: Option<[&ToneCurve; 3]>,
) -> Option<Box<IccProfile>> {
    let mut icc = create_profile_placeholder(ctx);
    icc.version = 0x04400000; // 4.4
    icc.device_class = pc::SigDisplayClass;
    icc.color_space = cs::SigRgbData;
    icc.pcs = cs::SigXYZData;
    icc.rendering_intent = INTENT_PERCEPTUAL;

    if !set_text_tags(&mut icc, &wide("RGB built-in")) {
        return None;
    }

    if white_point.is_some() {
        if !cms_write_tag(
            &mut icc,
            tags::SigMediaWhitePointTag,
            Some(TagObject::Xyz(d50_xyz())),
        ) {
            return None;
        }
        let wpx = crate::pcs::xyy2xyz(white_point.unwrap());
        let mut chad = Mat3::default();
        if !adaptation_matrix(&mut chad, None, &wpx, &d50_xyz()) {
            return None;
        }
        // CHAD serializes as a 9-element s15Fixed16 array (row-major)
        let mut flat = [0.0f64; 9];
        for r in 0..3 {
            for c in 0..3 {
                flat[r * 3 + c] = chad.v[r].n[c];
            }
        }
        if !cms_write_tag(
            &mut icc,
            tags::SigChromaticAdaptationTag,
            Some(TagObject::S15Fixed16(flat.to_vec())),
        ) {
            return None;
        }
    }

    if let (Some(wp), Some(pr)) = (white_point, primaries) {
        let max_white = CiexyY {
            x: wp.x,
            y: wp.y,
            Y: 1.0,
        };
        let mut m_colorants = Mat3::default();
        if !build_rgb2xyz_transfer_matrix(&mut m_colorants, &max_white, pr) {
            return None;
        }
        let (r, g, b) = (
            CieXyz {
                X: m_colorants.v[0].n[0],
                Y: m_colorants.v[1].n[0],
                Z: m_colorants.v[2].n[0],
            },
            CieXyz {
                X: m_colorants.v[0].n[1],
                Y: m_colorants.v[1].n[1],
                Z: m_colorants.v[2].n[1],
            },
            CieXyz {
                X: m_colorants.v[0].n[2],
                Y: m_colorants.v[1].n[2],
                Z: m_colorants.v[2].n[2],
            },
        );
        if !cms_write_tag(&mut icc, tags::SigRedColorantTag, Some(TagObject::Xyz(r)))
            || !cms_write_tag(&mut icc, tags::SigBlueColorantTag, Some(TagObject::Xyz(b)))
            || !cms_write_tag(&mut icc, tags::SigGreenColorantTag, Some(TagObject::Xyz(g)))
        {
            return None;
        }
    }

    if let Some(tr) = transfer {
        if !cms_write_tag(
            &mut icc,
            tags::SigRedTRCTag,
            Some(TagObject::Curve(tr[0].clone())),
        ) {
            return None;
        }
        if std::ptr::eq(tr[1], tr[0]) {
            if !cms_link_tag(&mut icc, tags::SigGreenTRCTag, tags::SigRedTRCTag) {
                return None;
            }
        } else if !cms_write_tag(
            &mut icc,
            tags::SigGreenTRCTag,
            Some(TagObject::Curve(tr[1].clone())),
        ) {
            return None;
        }
        if std::ptr::eq(tr[2], tr[0]) {
            if !cms_link_tag(&mut icc, tags::SigBlueTRCTag, tags::SigRedTRCTag) {
                return None;
            }
        } else if !cms_write_tag(
            &mut icc,
            tags::SigBlueTRCTag,
            Some(TagObject::Curve(tr[2].clone())),
        ) {
            return None;
        }
    }

    if let Some(pr) = primaries {
        if !cms_write_tag(
            &mut icc,
            tags::SigChromaticityTag,
            Some(TagObject::Chromaticity(pr.clone())),
        ) {
            return None;
        }
    }

    Some(icc)
}

/// `cmsCreateGrayProfileTHR`
pub fn create_gray_profile(
    ctx: Option<Context>,
    white_point: Option<&CiexyY>,
    transfer: Option<&ToneCurve>,
) -> Option<Box<IccProfile>> {
    let mut icc = create_profile_placeholder(ctx);
    icc.version = 0x04400000;
    icc.device_class = pc::SigDisplayClass;
    icc.color_space = cs::SigGrayData;
    icc.pcs = cs::SigXYZData;
    icc.rendering_intent = INTENT_PERCEPTUAL;
    if !set_text_tags(&mut icc, &wide("gray built-in")) {
        return None;
    }
    if let Some(wp) = white_point {
        let tmp = crate::pcs::xyy2xyz(wp);
        if !cms_write_tag(
            &mut icc,
            tags::SigMediaWhitePointTag,
            Some(TagObject::Xyz(tmp)),
        ) {
            return None;
        }
    }
    if let Some(tc) = transfer {
        if !cms_write_tag(
            &mut icc,
            tags::SigGrayTRCTag,
            Some(TagObject::Curve(tc.clone())),
        ) {
            return None;
        }
    }
    Some(icc)
}

/// `cmsCreateLinearizationDeviceLinkTHR`
pub fn create_linearization_device_link(
    ctx: Option<Context>,
    color_space: u32,
    transfers: &[&ToneCurve],
) -> Option<Box<IccProfile>> {
    let mut icc = create_profile_placeholder(ctx);
    icc.version = 0x04400000;
    icc.device_class = pc::SigLinkClass;
    icc.color_space = color_space;
    icc.pcs = color_space;
    icc.rendering_intent = INTENT_PERCEPTUAL;
    let n_ch = channels_of_color_space(color_space) as u32;
    let mut lut = pipeline_alloc(None, n_ch, n_ch)?;
    let st = stage_alloc_tone_curves(None, n_ch, Some(transfers))?;
    if !lut.insert_stage(StageLoc::AtBegin, st) {
        return None;
    }
    if !set_text_tags(&mut icc, &wide("Linearization built-in")) {
        return None;
    }
    if !cms_write_tag(&mut icc, tags::SigAToB0Tag, Some(TagObject::LutA2B(lut))) {
        return None;
    }
    if !set_seq_desc_tag(&mut icc, "Linearization built-in") {
        return None;
    }
    Some(icc)
}

/// `cmsCreateInkLimitingDeviceLinkTHR`
pub fn create_ink_limiting_device_link(
    ctx: Option<Context>,
    color_space: u32,
    mut limit: f64,
) -> Option<Box<IccProfile>> {
    if color_space != cs::SigCmykData {
        crate::err::signal_error(
            None,
            crate::types::ERROR_COLORSPACE_CHECK,
            "InkLimiting: Only CMYK currently supported",
        );
        return None;
    }
    if !(1.0..=400.0).contains(&limit) {
        crate::err::signal_error(
            None,
            crate::types::ERROR_RANGE,
            "InkLimiting: Limit should be between 1..400",
        );
        limit = limit.clamp(1.0, 400.0);
    }
    let mut icc = create_profile_placeholder(ctx);
    icc.version = 0x04400000;
    icc.device_class = pc::SigLinkClass;
    icc.color_space = color_space;
    icc.pcs = color_space;
    icc.rendering_intent = INTENT_PERCEPTUAL;
    let mut lut = pipeline_alloc(None, 4, 4)?;
    let n_ch = channels_of_color_space(color_space) as u32;
    let mut clut = stage_alloc_clut16bit(None, 17, n_ch, n_ch, None)?;

    // `InkLimitingSampler`
    let mut sampler = move |input: &[u16], output: &mut [u16]| -> i32 {
        let lim = limit * 655.35;
        let sum_cmy = input[0] as f64 + input[1] as f64 + input[2] as f64;
        let sum_cmyk = sum_cmy + input[3] as f64;
        let ratio = if sum_cmyk > lim && sum_cmy > 0.0 {
            (1.0 - (sum_cmyk - lim) / sum_cmy).max(0.0)
        } else {
            1.0
        };
        output[0] = quick_saturate_word(input[0] as f64 * ratio);
        output[1] = quick_saturate_word(input[1] as f64 * ratio);
        output[2] = quick_saturate_word(input[2] as f64 * ratio);
        output[3] = input[3];
        1
    };
    if !stage_sample_clut16bit(&mut clut, &mut sampler, 0) {
        return None;
    }
    if !lut.insert_stage(StageLoc::AtBegin, stage_alloc_identity_curves(None, n_ch)?)
        || !lut.insert_stage(StageLoc::AtEnd, clut)
        || !lut.insert_stage(StageLoc::AtEnd, stage_alloc_identity_curves(None, n_ch)?)
    {
        return None;
    }
    if !set_text_tags(&mut icc, &wide("ink-limiting built-in"))
        || !cms_write_tag(&mut icc, tags::SigAToB0Tag, Some(TagObject::LutA2B(lut)))
        || !set_seq_desc_tag(&mut icc, "ink-limiting built-in")
    {
        return None;
    }
    Some(icc)
}

/// `cmsCreateLab2ProfileTHR`
pub fn create_lab2_profile(
    ctx: Option<Context>,
    white_point: Option<&CiexyY>,
) -> Option<Box<IccProfile>> {
    let xyz = match white_point {
        None => d50_xyz(),
        Some(wp) => crate::pcs::xyy2xyz(wp),
    };
    let mut icc = create_rgb_profile(ctx, None, None, None)?;
    icc.version = 0x02100000; // 2.1
    icc.device_class = pc::SigAbstractClass;
    icc.color_space = cs::SigLabData;
    icc.pcs = cs::SigLabData;
    if !cms_write_tag(
        &mut icc,
        tags::SigMediaWhitePointTag,
        Some(TagObject::Xyz(xyz)),
    ) {
        return None;
    }
    if !set_text_tags(&mut icc, &wide("Lab identity built-in")) {
        return None;
    }
    let mut lut = pipeline_alloc(None, 3, 3)?;
    if !lut.insert_stage(StageLoc::AtBegin, stage_alloc_identity_clut(None, 3)?) {
        return None;
    }
    if !cms_write_tag(&mut icc, tags::SigAToB0Tag, Some(TagObject::LutA2B(lut))) {
        return None;
    }
    Some(icc)
}

/// `cmsCreateLab4ProfileTHR`
pub fn create_lab4_profile(
    ctx: Option<Context>,
    white_point: Option<&CiexyY>,
) -> Option<Box<IccProfile>> {
    let xyz = match white_point {
        None => d50_xyz(),
        Some(wp) => crate::pcs::xyy2xyz(wp),
    };
    let mut icc = create_rgb_profile(ctx, None, None, None)?;
    icc.version = 0x04400000; // 4.4
    icc.device_class = pc::SigAbstractClass;
    icc.color_space = cs::SigLabData;
    icc.pcs = cs::SigLabData;
    if !cms_write_tag(
        &mut icc,
        tags::SigMediaWhitePointTag,
        Some(TagObject::Xyz(xyz)),
    ) {
        return None;
    }
    if !set_text_tags(&mut icc, &wide("Lab identity built-in")) {
        return None;
    }
    let mut lut = pipeline_alloc(None, 3, 3)?;
    if !lut.insert_stage(StageLoc::AtBegin, stage_alloc_identity_curves(None, 3)?) {
        return None;
    }
    if !cms_write_tag(&mut icc, tags::SigAToB0Tag, Some(TagObject::LutA2B(lut))) {
        return None;
    }
    Some(icc)
}

/// `cmsCreateXYZProfileTHR`
pub fn create_xyz_profile(ctx: Option<Context>) -> Option<Box<IccProfile>> {
    let mut icc = create_rgb_profile(ctx, Some(&d50_xyy()), None, None)?;
    icc.version = 0x04400000;
    icc.device_class = pc::SigAbstractClass;
    icc.color_space = cs::SigXYZData;
    icc.pcs = cs::SigXYZData;
    if !set_text_tags(&mut icc, &wide("XYZ identity built-in")) {
        return None;
    }
    let mut lut = pipeline_alloc(None, 3, 3)?;
    if !lut.insert_stage(StageLoc::AtBegin, stage_alloc_identity_curves(None, 3)?) {
        return None;
    }
    if !cms_write_tag(&mut icc, tags::SigAToB0Tag, Some(TagObject::LutA2B(lut))) {
        return None;
    }
    Some(icc)
}

/// `Build_sRGBGamma` — parametric curve type 4
fn build_srgb_gamma(ctx: Option<&Context>) -> Option<ToneCurve> {
    build_parametric_tone_curve(
        ctx,
        4,
        &[2.4, 1.0 / 1.055, 0.055 / 1.055, 1.0 / 12.92, 0.04045],
    )
}

/// `cmsCreate_sRGBProfileTHR`
pub fn create_srgb_profile(ctx: Option<Context>) -> Option<Box<IccProfile>> {
    let d65 = CiexyY {
        x: 0.3127,
        y: 0.3290,
        Y: 1.0,
    };
    let rec709 = CiexyYTriple {
        Red: CiexyY {
            x: 0.6400,
            y: 0.3300,
            Y: 1.0,
        },
        Green: CiexyY {
            x: 0.3000,
            y: 0.6000,
            Y: 1.0,
        },
        Blue: CiexyY {
            x: 0.1500,
            y: 0.0600,
            Y: 1.0,
        },
    };
    let g = build_srgb_gamma(ctx.as_ref())?;
    let mut icc = create_rgb_profile(ctx, Some(&d65), Some(&rec709), Some([&g, &g, &g]))?;
    if !set_text_tags(&mut icc, &wide("sRGB built-in")) {
        return None;
    }
    Some(icc)
}

/// `cmsCreate_OkLabProfile` — MPE-pipeline OkLab colorspace profile.
/// (C notes this profile cannot be saved as an ICC file.)
pub fn create_oklab_profile(ctx: Option<Context>) -> Option<Box<IccProfile>> {
    let xyz_pcs = crate::lut::stage_normalize_from_xyz_float(ctx.as_ref())?;
    let pcs_xyz = crate::lut::stage_normalize_to_xyz_float(ctx.as_ref())?;

    const M_D65_D50: [f64; 9] = [
        1.047886, 0.022919, -0.050216, 0.029582, 0.990484, -0.017079, -0.009252, 0.015073, 0.751678,
    ];
    const M_D50_D65: [f64; 9] = [
        0.955512609517083,
        -0.023073214184645,
        0.063308961782107,
        -0.028324949364887,
        1.009942432477107,
        0.021054814890112,
        0.012328875695483,
        -0.020535835374141,
        1.330713916450354,
    ];
    let d65_to_d50 = crate::lut::stage_alloc_matrix(ctx.as_ref(), 3, 3, &M_D65_D50, None)?;
    let d50_to_d65 = crate::lut::stage_alloc_matrix(ctx.as_ref(), 3, 3, &M_D50_D65, None)?;

    const M_D65_LMS: [f64; 9] = [
        0.8189330101,
        0.3618667424,
        -0.1288597137,
        0.0329845436,
        0.9293118715,
        0.0361456387,
        0.0482003018,
        0.2643662691,
        0.6338517070,
    ];
    const M_LMS_D65: [f64; 9] = [
        1.227013851103521,
        -0.557799980651822,
        0.281256148966468,
        -0.040580178423281,
        1.112256869616830,
        -0.071676678665601,
        -0.076381284505707,
        -0.421481978418013,
        1.586163220440795,
    ];
    let d65_to_lms = crate::lut::stage_alloc_matrix(ctx.as_ref(), 3, 3, &M_D65_LMS, None)?;
    let lms_to_d65 = crate::lut::stage_alloc_matrix(ctx.as_ref(), 3, 3, &M_LMS_D65, None)?;

    let cube_root = crate::gamma::build_gamma(ctx.as_ref(), 1.0 / 3.0)?;
    let cube = crate::gamma::build_gamma(ctx.as_ref(), 3.0)?;
    let roots: [&ToneCurve; 3] = [&cube_root, &cube_root, &cube_root];
    let cubes: [&ToneCurve; 3] = [&cube, &cube, &cube];
    let nonlin_fw = crate::lut::stage_alloc_tone_curves(ctx.as_ref(), 3, Some(&roots))?;
    let nonlin_rv = crate::lut::stage_alloc_tone_curves(ctx.as_ref(), 3, Some(&cubes))?;

    const M_LMSP_OKLAB: [f64; 9] = [
        0.2104542553,
        0.7936177850,
        -0.0040720468,
        1.9779984951,
        -2.4285922050,
        0.4505937099,
        0.0259040371,
        0.7827717662,
        -0.8086757660,
    ];
    const M_OKLAB_LMSP: [f64; 9] = [
        0.999999998450520,
        0.396337792173768,
        0.215803758060759,
        1.000000008881761,
        -0.105561342323656,
        -0.063854174771706,
        1.000000054672411,
        -0.089484182094966,
        -1.291485537864092,
    ];
    let lmsp_oklab = crate::lut::stage_alloc_matrix(ctx.as_ref(), 3, 3, &M_LMSP_OKLAB, None)?;
    let oklab_lmsp = crate::lut::stage_alloc_matrix(ctx.as_ref(), 3, 3, &M_OKLAB_LMSP, None)?;

    let mut atob = pipeline_alloc(ctx.as_ref(), 3, 3)?;
    let mut btoa = pipeline_alloc(ctx.as_ref(), 3, 3)?;
    let mut icc = create_profile_placeholder(ctx);
    icc.version = 0x04400000;
    icc.device_class = pc::SigColorSpaceClass;
    icc.color_space = cs::Sig3colorData;
    icc.pcs = cs::SigXYZData;
    icc.rendering_intent = INTENT_RELATIVE_COLORIMETRIC;

    // PCS (XYZ/D50) → OkLab
    if !btoa.insert_stage(StageLoc::AtEnd, pcs_xyz)
        || !btoa.insert_stage(StageLoc::AtEnd, d50_to_d65)
        || !btoa.insert_stage(StageLoc::AtEnd, d65_to_lms)
        || !btoa.insert_stage(StageLoc::AtEnd, nonlin_fw)
        || !btoa.insert_stage(StageLoc::AtEnd, lmsp_oklab)
    {
        return None;
    }
    if !cms_write_tag(&mut icc, tags::SigBToA0Tag, Some(TagObject::LutB2A(btoa))) {
        return None;
    }

    // OkLab → PCS
    if !atob.insert_stage(StageLoc::AtEnd, oklab_lmsp)
        || !atob.insert_stage(StageLoc::AtEnd, nonlin_rv)
        || !atob.insert_stage(StageLoc::AtEnd, lms_to_d65)
        || !atob.insert_stage(StageLoc::AtEnd, d65_to_d50)
        || !atob.insert_stage(StageLoc::AtEnd, xyz_pcs)
    {
        return None;
    }
    if !cms_write_tag(&mut icc, tags::SigAToB0Tag, Some(TagObject::LutA2B(atob))) {
        return None;
    }
    Some(icc)
}

/// `cmsCreateNULLProfileTHR` — output profile with a black-only devicelink
pub fn create_null_profile(ctx: Option<Context>) -> Option<Box<IccProfile>> {
    let mut icc = create_profile_placeholder(ctx.clone());
    icc.version = 0x04400000;
    if !set_text_tags(&mut icc, &wide("NULL profile built-in")) {
        return None;
    }
    icc.device_class = pc::SigOutputClass;
    icc.color_space = cs::SigGrayData;
    icc.pcs = cs::SigLabData;

    let mut lut = pipeline_alloc(ctx.as_ref(), 3, 1)?;
    let zero = [0u16, 0];
    let empty = crate::gamma::build_tabulated_tone_curve16(ctx.as_ref(), 2, Some(&zero))?;
    let refs3: [&ToneCurve; 3] = [&empty, &empty, &empty];
    let refs1: [&ToneCurve; 1] = [&empty];
    let post_lin = crate::lut::stage_alloc_tone_curves(ctx.as_ref(), 3, Some(&refs3))?;
    let out_lin = crate::lut::stage_alloc_tone_curves(ctx.as_ref(), 1, Some(&refs1))?;
    const PICK_LSTAR: [f64; 3] = [1.0, 0.0, 0.0];
    let pick = crate::lut::stage_alloc_matrix(ctx.as_ref(), 1, 3, &PICK_LSTAR, None)?;

    if !lut.insert_stage(StageLoc::AtEnd, post_lin)
        || !lut.insert_stage(StageLoc::AtEnd, pick)
        || !lut.insert_stage(StageLoc::AtEnd, out_lin)
    {
        return None;
    }
    if !cms_write_tag(&mut icc, tags::SigBToA0Tag, Some(TagObject::LutB2A(lut))) {
        return None;
    }
    if !cms_write_tag(
        &mut icc,
        tags::SigMediaWhitePointTag,
        Some(TagObject::Xyz(d50_xyz())),
    ) {
        return None;
    }
    Some(icc)
}

/// `BCHSWADJUSTS`
struct BchswAdjusts {
    brightness: f64,
    contrast: f64,
    hue: f64,
    saturation: f64,
    wp_src: CieXyz,
    wp_dest: CieXyz,
    adjust_wp: bool,
}

/// `bchswSampler`
fn bchsw_sampler(b: &BchswAdjusts, input: &[u16], output: &mut [u16]) -> i32 {
    let lab_in = crate::pcs::lab_encoded2float([input[0], input[1], input[2]]);
    let lch_in = lab2lch(&lab_in);
    let lch_out = crate::types::CieLCh {
        L: lch_in.L * b.contrast + b.brightness,
        C: lch_in.C + b.saturation,
        h: lch_in.h + b.hue,
    };
    let mut lab_out = lch2lab(&lch_out);
    if b.adjust_wp {
        let xyz = lab2xyz(Some(&b.wp_src), &lab_out);
        lab_out = xyz2lab(Some(&b.wp_dest), &xyz);
    }
    let enc = float2lab_encoded(&lab_out);
    output[0] = enc[0];
    output[1] = enc[1];
    output[2] = enc[2];
    1
}

/// `cmsCreateBCHSWabstractProfileTHR`
pub fn create_bchsw_abstract_profile(
    ctx: Option<Context>,
    n_lut_points: u32,
    bright: f64,
    contrast: f64,
    hue: f64,
    saturation: f64,
    temp_src: u32,
    temp_dest: u32,
) -> Option<Box<IccProfile>> {
    let mut b = BchswAdjusts {
        brightness: bright,
        contrast,
        hue,
        saturation,
        wp_src: CieXyz::default(),
        wp_dest: CieXyz::default(),
        adjust_wp: temp_src != temp_dest,
    };
    if b.adjust_wp {
        let wp = crate::wtpnt::white_point_from_temp(temp_src as f64)?;
        b.wp_src = crate::pcs::xyy2xyz(&wp);
        let wp = crate::wtpnt::white_point_from_temp(temp_dest as f64)?;
        b.wp_dest = crate::pcs::xyy2xyz(&wp);
    }
    let mut icc = create_profile_placeholder(ctx.clone());
    icc.device_class = pc::SigAbstractClass;
    icc.color_space = cs::SigLabData;
    icc.pcs = cs::SigLabData;
    icc.rendering_intent = INTENT_PERCEPTUAL;

    let mut lut = pipeline_alloc(ctx.as_ref(), 3, 3)?;
    let dims = [n_lut_points; 15];
    let mut clut =
        crate::lut::stage_alloc_clut16bit_granular(ctx.as_ref(), &dims[..3], 3, 3, None)?;
    {
        let mut sampler =
            |input: &[u16], output: &mut [u16]| -> i32 { bchsw_sampler(&b, input, output) };
        if !stage_sample_clut16bit(&mut clut, &mut sampler, 0) {
            return None;
        }
    }
    if !lut.insert_stage(StageLoc::AtEnd, clut) {
        return None;
    }
    if !set_text_tags(&mut icc, &wide("BCHS built-in"))
        || !cms_write_tag(
            &mut icc,
            tags::SigMediaWhitePointTag,
            Some(TagObject::Xyz(d50_xyz())),
        )
        || !cms_write_tag(&mut icc, tags::SigAToB0Tag, Some(TagObject::LutA2B(lut)))
    {
        return None;
    }
    Some(icc)
}
