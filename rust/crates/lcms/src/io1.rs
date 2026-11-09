//! `cmsio1.c` — profile-content readers: media white point, CHAD,
//! matrix-shaper/gray pipelines, and the input/output/devicelink LUT
//! readers that turn profile tags into runnable pipelines.

use crate::gamma::{ToneCurve, build_tabulated_tone_curve16};
use crate::lut::{Pipeline, StageLoc, pipeline_alloc, stage_alloc_matrix, stage_alloc_tone_curves};
use crate::profile::{self, IccProfile};
use crate::signatures::colorspace as cs;
use crate::signatures::profileclass as pc;
use crate::signatures::tag as tags;
use crate::tagtypes::TagObject;
use crate::types::{CieXyz, Mat3, Vec3};
use crate::wtpnt::{adaptation_matrix, d50_xyz};

// `Device2PCS16` / `Device2PCSFloat` — intent-indexed tag tables
const DEVICE2PCS16: [u32; 4] = [
    tags::SigAToB0Tag,
    tags::SigAToB1Tag,
    tags::SigAToB2Tag,
    tags::SigAToB1Tag,
];
const DEVICE2PCSFLOAT: [u32; 4] = [
    tags::SigDToB0Tag,
    tags::SigDToB1Tag,
    tags::SigDToB2Tag,
    tags::SigDToB3Tag,
];
const PCS2DEVICE16: [u32; 4] = [
    tags::SigBToA0Tag,
    tags::SigBToA1Tag,
    tags::SigBToA2Tag,
    tags::SigBToA1Tag,
];
const PCS2DEVICEFLOAT: [u32; 4] = [
    tags::SigBToD0Tag,
    tags::SigBToD1Tag,
    tags::SigBToD2Tag,
    tags::SigBToD3Tag,
];

const MAX_ENC_XYZ: f64 = (2.0 * 65535.0) / 65536.0;
const INP_ADJ: f64 = 1.0 / MAX_ENC_XYZ;
const OUTP_ADJ: f64 = MAX_ENC_XYZ;

/// `_cmsReadMediaWhitePoint` — D50 default; display-class v2→D50
pub fn read_media_white_point(icc: &mut IccProfile) -> CieXyz {
    let Some(TagObject::Xyz(tag)) = profile::cms_read_tag(icc, tags::SigMediaWhitePointTag) else {
        return d50_xyz();
    };
    let tag = *tag;
    if profile::get_encoded_icc_version(icc) < 0x4000000 && icc.device_class == pc::SigDisplayClass
    {
        return d50_xyz();
    }
    tag
}

/// `_cmsReadCHAD` — chromatic adaptation matrix; absent → identity;
/// display-class v2 → computed from wtpt→D50
pub fn read_chad(icc: &mut IccProfile) -> Mat3 {
    if let Some(TagObject::S15Fixed16(arr)) =
        profile::cms_read_tag(icc, tags::SigChromaticAdaptationTag)
    {
        if arr.len() >= 9 {
            let mut m = Mat3::default();
            for r in 0..3 {
                for c in 0..3 {
                    m.v[r].n[c] = arr[r * 3 + c];
                }
            }
            return m;
        }
    }
    let mut dest = Mat3::default();
    crate::mtrx::mat3_identity(&mut dest);
    if profile::get_encoded_icc_version(icc) < 0x4000000 && icc.device_class == pc::SigDisplayClass
    {
        let Some(TagObject::Xyz(white)) = profile::cms_read_tag(icc, tags::SigMediaWhitePointTag)
        else {
            return dest;
        };
        let white = *white;
        adaptation_matrix(&mut dest, None, &white, &d50_xyz());
        return dest;
    }
    dest
}

/// `ReadICCMatrixRGB2XYZ` — colorants → matrix
fn read_icc_matrix_rgb2xyz(icc: &mut IccProfile) -> Option<Mat3> {
    let r = match profile::cms_read_tag(icc, tags::SigRedColorantTag) {
        Some(TagObject::Xyz(x)) => *x,
        _ => return None,
    };
    let g = match profile::cms_read_tag(icc, tags::SigGreenColorantTag) {
        Some(TagObject::Xyz(x)) => *x,
        _ => return None,
    };
    let b = match profile::cms_read_tag(icc, tags::SigBlueColorantTag) {
        Some(TagObject::Xyz(x)) => *x,
        _ => return None,
    };
    let mut m = Mat3::default();
    m.v[0] = Vec3 { n: [r.X, g.X, b.X] };
    m.v[1] = Vec3 { n: [r.Y, g.Y, b.Y] };
    m.v[2] = Vec3 { n: [r.Z, g.Z, b.Z] };
    Some(m)
}

/// `BuildGrayInputMatrixPipeline` — gray TRC + Gray→XYZ matrix (+ Lab fixup)
fn build_gray_input_matrix_pipeline(icc: &mut IccProfile) -> Option<Pipeline> {
    let trc = match profile::cms_read_tag(icc, tags::SigGrayTRCTag) {
        Some(TagObject::Curve(t)) | Some(TagObject::ParametricCurve(t)) => t.clone(),
        _ => return None,
    };
    let ctx = icc.ctx.as_ref();
    let mut lut = pipeline_alloc(ctx, 1, 3)?;
    if icc.pcs == cs::SigLabData {
        const ZERO: [u16; 2] = [0x8080, 0x8080];
        let empty = build_tabulated_tone_curve16(ctx, 2, Some(&ZERO))?;
        let one_to_three: [f64; 3] = [1.0, 1.0, 1.0];
        let refs: [&ToneCurve; 3] = [&trc, &empty, &empty];
        let m = stage_alloc_matrix(ctx, 3, 1, &one_to_three, None)?;
        if !lut.insert_stage(StageLoc::AtEnd, m) {
            return None;
        }
        let c = stage_alloc_tone_curves(ctx, 3, Some(&refs))?;
        if !lut.insert_stage(StageLoc::AtEnd, c) {
            return None;
        }
    } else {
        let refs: [&ToneCurve; 1] = [&trc];
        let c = stage_alloc_tone_curves(ctx, 1, Some(&refs))?;
        if !lut.insert_stage(StageLoc::AtEnd, c) {
            return None;
        }
        let d50 = d50_xyz();
        let gray: [f64; 3] = [INP_ADJ * d50.X, INP_ADJ * d50.Y, INP_ADJ * d50.Z];
        let m = stage_alloc_matrix(ctx, 3, 1, &gray, None)?;
        if !lut.insert_stage(StageLoc::AtEnd, m) {
            return None;
        }
    }
    Some(lut)
}

/// `BuildRGBInputMatrixShaper` — TRCs + RGB→XYZ matrix (+ optional XYZ→Lab)
fn build_rgb_input_matrix_shaper(icc: &mut IccProfile) -> Option<Pipeline> {
    let mut mat = read_icc_matrix_rgb2xyz(icc)?;
    for i in 0..3 {
        for j in 0..3 {
            mat.v[i].n[j] *= INP_ADJ;
        }
    }
    let get_trc = |icc: &mut IccProfile, sig: u32| -> Option<ToneCurve> {
        match profile::cms_read_tag(icc, sig) {
            Some(TagObject::Curve(t)) | Some(TagObject::ParametricCurve(t)) => Some(t.clone()),
            _ => None,
        }
    };
    let s0 = get_trc(icc, tags::SigRedTRCTag)?;
    let s1 = get_trc(icc, tags::SigGreenTRCTag)?;
    let s2 = get_trc(icc, tags::SigBlueTRCTag)?;
    let ctx = icc.ctx.as_ref();
    let mut lut = pipeline_alloc(ctx, 3, 3)?;
    let refs: [&ToneCurve; 3] = [&s0, &s1, &s2];
    let c = stage_alloc_tone_curves(ctx, 3, Some(&refs))?;
    if !lut.insert_stage(StageLoc::AtEnd, c) {
        return None;
    }
    let mut flat = [0.0f64; 9];
    for r in 0..3 {
        for c in 0..3 {
            flat[r * 3 + c] = mat.v[r].n[c];
        }
    }
    let m = stage_alloc_matrix(ctx, 3, 3, &flat, None)?;
    if !lut.insert_stage(StageLoc::AtEnd, m) {
        return None;
    }
    if icc.pcs == cs::SigLabData {
        let st = crate::lut::stage_alloc_xyz2lab(ctx);
        if !lut.insert_stage(StageLoc::AtEnd, st) {
            return None;
        }
    }
    Some(lut)
}

/// `_cmsReadFloatInputTag` — dup the float tag + normalize stages
fn read_float_input_tag(icc: &mut IccProfile, tag_float: u32) -> Option<Pipeline> {
    let spc = icc.color_space;
    let pcs = icc.pcs;
    let lut = match profile::cms_read_tag(icc, tag_float) {
        Some(TagObject::Mpe(l))
        | Some(TagObject::LutA2B(l))
        | Some(TagObject::LutB2A(l))
        | Some(TagObject::Lut8(l))
        | Some(TagObject::Lut16(l)) => l.clone(),
        _ => return None,
    };
    let ctx = icc.ctx.as_ref();
    let mut lut = lut;
    if spc == cs::SigLabData {
        let st = crate::lut::stage_normalize_to_lab_float(ctx)?;
        if !lut.insert_stage(StageLoc::AtBegin, st) {
            return None;
        }
    } else if spc == cs::SigXYZData {
        let st = crate::lut::stage_normalize_to_xyz_float(ctx)?;
        if !lut.insert_stage(StageLoc::AtBegin, st) {
            return None;
        }
    }
    if pcs == cs::SigLabData {
        let st = crate::lut::stage_normalize_from_lab_float(ctx)?;
        if !lut.insert_stage(StageLoc::AtEnd, st) {
            return None;
        }
    } else if pcs == cs::SigXYZData {
        let st = crate::lut::stage_normalize_from_xyz_float(ctx)?;
        if !lut.insert_stage(StageLoc::AtEnd, st) {
            return None;
        }
    }
    Some(lut)
}

/// `_cmsGetTagTrueType` — the type signature used when the tag was cooked
fn get_tag_true_type(icc: &IccProfile, sig: u32) -> u32 {
    let n = profile::search_tag(icc, sig, true);
    if n < 0 {
        return 0;
    }
    icc.tag_type_sigs[n as usize]
}

/// `_cmsReadInputLUT` — named-color, float tag, LUT tag, or matrix shaper
pub fn read_input_lut(icc: &mut IccProfile, intent: u32) -> Option<Pipeline> {
    if icc.device_class == pc::SigNamedColorClass {
        let ncl = match profile::cms_read_tag(icc, tags::SigNamedColor2Tag) {
            Some(TagObject::NamedColor(n)) => n.clone(),
            _ => return None,
        };
        let ctx = icc.ctx.as_ref();
        let mut lut = pipeline_alloc(ctx, 0, 0)?;
        let Some(st) = crate::named::stage_alloc_named_color(&ncl, true) else {
            return None;
        };
        if !lut.insert_stage(StageLoc::AtBegin, st) {
            return None;
        }
        let st = crate::lut::stage_alloc_lab_v2_to_v4(ctx)?;
        if !lut.insert_stage(StageLoc::AtEnd, st) {
            return None;
        }
        return Some(lut);
    }
    if intent <= 3 {
        let mut tag16 = DEVICE2PCS16[intent as usize];
        let tag_float = DEVICE2PCSFLOAT[intent as usize];
        if profile::cms_is_tag(icc, tag_float) {
            return read_float_input_tag(icc, tag_float);
        }
        if !profile::cms_is_tag(icc, tag16) {
            tag16 = DEVICE2PCS16[0];
        }
        if profile::cms_is_tag(icc, tag16) {
            let lut = match profile::cms_read_tag(icc, tag16) {
                Some(TagObject::LutA2B(l))
                | Some(TagObject::LutB2A(l))
                | Some(TagObject::Lut8(l))
                | Some(TagObject::Lut16(l))
                | Some(TagObject::Mpe(l)) => l.clone(),
                _ => return None,
            };
            let original_type = get_tag_true_type(icc, tag16);
            if original_type != crate::signatures::tagtype::SigLut16Type
                || icc.pcs != cs::SigLabData
            {
                return Some(lut);
            }
            let ctx = icc.ctx.as_ref();
            let mut lut = lut;
            if icc.color_space == cs::SigLabData {
                let st = crate::lut::stage_alloc_lab_v4_to_v2(ctx)?;
                if !lut.insert_stage(StageLoc::AtBegin, st) {
                    return None;
                }
            }
            let st = crate::lut::stage_alloc_lab_v2_to_v4(ctx)?;
            if !lut.insert_stage(StageLoc::AtEnd, st) {
                return None;
            }
            return Some(lut);
        }
    }
    if icc.color_space == cs::SigGrayData {
        return build_gray_input_matrix_pipeline(icc);
    }
    build_rgb_input_matrix_shaper(icc)
}

/// `BuildGrayOutputPipeline` — PickY/L* matrix + reversed gray TRC
fn build_gray_output_pipeline(icc: &mut IccProfile) -> Option<Pipeline> {
    let trc = match profile::cms_read_tag(icc, tags::SigGrayTRCTag) {
        Some(TagObject::Curve(t)) | Some(TagObject::ParametricCurve(t)) => t.clone(),
        _ => return None,
    };
    let ctx = icc.ctx.as_ref();
    let rev = crate::gamma::reverse_tone_curve(&trc)?;
    let mut lut = pipeline_alloc(ctx, 3, 1)?;
    let mat: [f64; 3] = if icc.pcs == cs::SigLabData {
        [1.0, 0.0, 0.0] // PickLstarMatrix
    } else {
        [0.0, OUTP_ADJ * d50_xyz().Y, 0.0] // PickYMatrix
    };
    let m = stage_alloc_matrix(ctx, 1, 3, &mat, None)?;
    if !lut.insert_stage(StageLoc::AtEnd, m) {
        return None;
    }
    let refs: [&ToneCurve; 1] = [&rev];
    let c = stage_alloc_tone_curves(ctx, 1, Some(&refs))?;
    if !lut.insert_stage(StageLoc::AtEnd, c) {
        return None;
    }
    Some(lut)
}

/// `BuildRGBOutputMatrixShaper` — inverse matrix + reversed TRCs
fn build_rgb_output_matrix_shaper(icc: &mut IccProfile) -> Option<Pipeline> {
    let mut inv = Mat3::default();
    {
        let mat = read_icc_matrix_rgb2xyz(icc)?;
        if !crate::mtrx::mat3_inverse(&mat, &mut inv) {
            return None;
        }
    }
    for i in 0..3 {
        for j in 0..3 {
            inv.v[i].n[j] *= OUTP_ADJ;
        }
    }
    let get_trc = |icc: &mut IccProfile, sig: u32| -> Option<ToneCurve> {
        match profile::cms_read_tag(icc, sig) {
            Some(TagObject::Curve(t)) | Some(TagObject::ParametricCurve(t)) => Some(t.clone()),
            _ => None,
        }
    };
    let s0 = get_trc(icc, tags::SigRedTRCTag)?;
    let s1 = get_trc(icc, tags::SigGreenTRCTag)?;
    let s2 = get_trc(icc, tags::SigBlueTRCTag)?;
    let i0 = crate::gamma::reverse_tone_curve(&s0)?;
    let i1 = crate::gamma::reverse_tone_curve(&s1)?;
    let i2 = crate::gamma::reverse_tone_curve(&s2)?;
    let ctx = icc.ctx.as_ref();
    let mut lut = pipeline_alloc(ctx, 3, 3)?;
    if icc.pcs == cs::SigLabData {
        let st = crate::lut::stage_alloc_lab2xyz(ctx);
        if !lut.insert_stage(StageLoc::AtEnd, st) {
            return None;
        }
    }
    let mut flat = [0.0f64; 9];
    for r in 0..3 {
        for c in 0..3 {
            flat[r * 3 + c] = inv.v[r].n[c];
        }
    }
    let m = stage_alloc_matrix(ctx, 3, 3, &flat, None)?;
    if !lut.insert_stage(StageLoc::AtEnd, m) {
        return None;
    }
    let refs: [&ToneCurve; 3] = [&i0, &i1, &i2];
    let c = stage_alloc_tone_curves(ctx, 3, Some(&refs))?;
    if !lut.insert_stage(StageLoc::AtEnd, c) {
        return None;
    }
    Some(lut)
}

/// `ChangeInterpolationToTrilinear` — flip every CLUT stage to trilinear
fn change_interpolation_to_trilinear(lut: &mut Pipeline) {
    let mut st = lut.elements.as_deref_mut();
    while let Some(s) = st {
        if s.stage_type() == crate::signatures::stage::SigCLutElemType {
            if let crate::lut::StageData::CLut(c) = &mut s.data {
                c.dw_flags |= crate::intrp::LERP_FLAGS_TRILINEAR;
                let _ = crate::intrp::default_interpolators_factory(
                    c.n_inputs,
                    c.n_outputs,
                    c.dw_flags,
                )
                .map(|i| c.interpolation = i);
                // Keep the prebuilt eval params in sync with the flip.
                for p in [c.params16.as_deref_mut(), c.params_f.as_deref_mut()]
                    .into_iter()
                    .flatten()
                {
                    p.dw_flags = c.dw_flags;
                    p.interpolation = c.interpolation;
                    p.kernel = crate::intrp::kernel_kind(c.interpolation);
                }
            }
        }
        st = s.next.as_deref_mut();
    }
}

/// `_cmsReadFloatOutputTag`
fn read_float_output_tag(icc: &mut IccProfile, tag_float: u32) -> Option<Pipeline> {
    let pcs = icc.pcs;
    let data_space = icc.color_space;
    let lut = match profile::cms_read_tag(icc, tag_float) {
        Some(TagObject::Mpe(l))
        | Some(TagObject::LutA2B(l))
        | Some(TagObject::LutB2A(l))
        | Some(TagObject::Lut8(l))
        | Some(TagObject::Lut16(l)) => l.clone(),
        _ => return None,
    };
    let ctx = icc.ctx.as_ref();
    let mut lut = lut;
    if pcs == cs::SigLabData {
        let st = crate::lut::stage_normalize_to_lab_float(ctx)?;
        if !lut.insert_stage(StageLoc::AtBegin, st) {
            return None;
        }
    } else if pcs == cs::SigXYZData {
        let st = crate::lut::stage_normalize_to_xyz_float(ctx)?;
        if !lut.insert_stage(StageLoc::AtBegin, st) {
            return None;
        }
    }
    if data_space == cs::SigLabData {
        let st = crate::lut::stage_normalize_from_lab_float(ctx)?;
        if !lut.insert_stage(StageLoc::AtEnd, st) {
            return None;
        }
    } else if data_space == cs::SigXYZData {
        let st = crate::lut::stage_normalize_from_xyz_float(ctx)?;
        if !lut.insert_stage(StageLoc::AtEnd, st) {
            return None;
        }
    }
    Some(lut)
}

/// `_cmsReadOutputLUT`
pub fn read_output_lut(icc: &mut IccProfile, intent: u32) -> Option<Pipeline> {
    if intent <= 3 {
        let mut tag16 = PCS2DEVICE16[intent as usize];
        let tag_float = PCS2DEVICEFLOAT[intent as usize];
        if profile::cms_is_tag(icc, tag_float) {
            return read_float_output_tag(icc, tag_float);
        }
        if !profile::cms_is_tag(icc, tag16) {
            tag16 = PCS2DEVICE16[0];
        }
        if profile::cms_is_tag(icc, tag16) {
            let lut = match profile::cms_read_tag(icc, tag16) {
                Some(TagObject::LutA2B(l))
                | Some(TagObject::LutB2A(l))
                | Some(TagObject::Lut8(l))
                | Some(TagObject::Lut16(l))
                | Some(TagObject::Mpe(l)) => l.clone(),
                _ => return None,
            };
            let mut lut = lut;
            if icc.pcs == cs::SigLabData {
                change_interpolation_to_trilinear(&mut lut);
            }
            let original_type = get_tag_true_type(icc, tag16);
            if original_type != crate::signatures::tagtype::SigLut16Type
                || icc.pcs != cs::SigLabData
            {
                return Some(lut);
            }
            let ctx = icc.ctx.as_ref();
            let st = crate::lut::stage_alloc_lab_v4_to_v2(ctx)?;
            if !lut.insert_stage(StageLoc::AtBegin, st) {
                return None;
            }
            if icc.color_space == cs::SigLabData {
                let st = crate::lut::stage_alloc_lab_v2_to_v4(ctx)?;
                if !lut.insert_stage(StageLoc::AtEnd, st) {
                    return None;
                }
            }
            return Some(lut);
        }
    }
    if icc.color_space == cs::SigGrayData {
        return build_gray_output_pipeline(icc);
    }
    build_rgb_output_matrix_shaper(icc)
}

/// `_cmsReadFloatDevicelinkTag`
fn read_float_devicelink_tag(icc: &mut IccProfile, tag_float: u32) -> Option<Pipeline> {
    read_float_input_tag(icc, tag_float)
}

/// `_cmsReadDevicelinkLUT`
pub fn read_devicelink_lut(icc: &mut IccProfile, intent: u32) -> Option<Pipeline> {
    if intent > 3 {
        return None;
    }
    let mut tag16 = DEVICE2PCS16[intent as usize];
    let mut tag_float = DEVICE2PCSFLOAT[intent as usize];
    if icc.device_class == pc::SigNamedColorClass {
        let ncl = match profile::cms_read_tag(icc, tags::SigNamedColor2Tag) {
            Some(TagObject::NamedColor(n)) => n.clone(),
            _ => return None,
        };
        let ctx = icc.ctx.as_ref();
        let mut lut = pipeline_alloc(ctx, 0, 0)?;
        let Some(st) = crate::named::stage_alloc_named_color(&ncl, false) else {
            return None;
        };
        if !lut.insert_stage(StageLoc::AtBegin, st) {
            return None;
        }
        if icc.color_space == cs::SigLabData {
            let st = crate::lut::stage_alloc_lab_v2_to_v4(ctx)?;
            if !lut.insert_stage(StageLoc::AtEnd, st) {
                return None;
            }
        }
        return Some(lut);
    }
    if profile::cms_is_tag(icc, tag_float) {
        return read_float_devicelink_tag(icc, tag_float);
    }
    tag_float = DEVICE2PCSFLOAT[0];
    if profile::cms_is_tag(icc, tag_float) {
        return match profile::cms_read_tag(icc, tag_float) {
            Some(TagObject::Mpe(l))
            | Some(TagObject::LutA2B(l))
            | Some(TagObject::LutB2A(l))
            | Some(TagObject::Lut8(l))
            | Some(TagObject::Lut16(l)) => Some(l.clone()),
            _ => None,
        };
    }
    if !profile::cms_is_tag(icc, tag16) {
        tag16 = DEVICE2PCS16[0];
        if !profile::cms_is_tag(icc, tag16) {
            return None;
        }
    }
    let lut = match profile::cms_read_tag(icc, tag16) {
        Some(TagObject::LutA2B(l))
        | Some(TagObject::LutB2A(l))
        | Some(TagObject::Lut8(l))
        | Some(TagObject::Lut16(l))
        | Some(TagObject::Mpe(l)) => l.clone(),
        _ => return None,
    };
    let mut lut = lut;
    if icc.pcs == cs::SigLabData {
        change_interpolation_to_trilinear(&mut lut);
    }
    let original_type = get_tag_true_type(icc, tag16);
    if original_type != crate::signatures::tagtype::SigLut16Type {
        return Some(lut);
    }
    let ctx = icc.ctx.as_ref();
    if icc.color_space == cs::SigLabData {
        let st = crate::lut::stage_alloc_lab_v4_to_v2(ctx)?;
        if !lut.insert_stage(StageLoc::AtBegin, st) {
            return None;
        }
    }
    if icc.pcs == cs::SigLabData {
        let st = crate::lut::stage_alloc_lab_v2_to_v4(ctx)?;
        if !lut.insert_stage(StageLoc::AtEnd, st) {
            return None;
        }
    }
    Some(lut)
}

// `LCMS_USED_AS_INPUT` / `LCMS_USED_AS_OUTPUT` / `LCMS_USED_AS_PROOF` — cmsIsCLUT direction
pub const LCMS_USED_AS_INPUT: i32 = 0;
pub const LCMS_USED_AS_OUTPUT: i32 = 1;
pub const LCMS_USED_AS_PROOF: i32 = 2;

/// `cmsIsCLUT` — is there a LUT-based tag for this intent + direction?
pub fn is_clut(icc: &IccProfile, intent: u32, used_direction: i32) -> bool {
    use crate::signatures::profileclass as pc;
    if icc.device_class == pc::SigLinkClass {
        return icc.rendering_intent == intent;
    }
    let tag_table = match used_direction {
        LCMS_USED_AS_INPUT => DEVICE2PCS16,
        LCMS_USED_AS_OUTPUT => PCS2DEVICE16,
        LCMS_USED_AS_PROOF => {
            return is_intent_supported(icc, intent, LCMS_USED_AS_INPUT)
                && is_intent_supported(
                    icc,
                    crate::types::INTENT_RELATIVE_COLORIMETRIC,
                    LCMS_USED_AS_OUTPUT,
                );
        }
        _ => {
            crate::err::signal_error(
                icc.ctx.as_ref(),
                crate::types::ERROR_RANGE,
                "Unexpected direction",
            );
            return false;
        }
    };
    if intent > crate::types::INTENT_ABSOLUTE_COLORIMETRIC {
        return false;
    }
    profile::cms_is_tag(icc, tag_table[intent as usize])
}

/// `cmsIsIntentSupported` — CLUT or matrix-shaper
pub fn is_intent_supported(icc: &IccProfile, intent: u32, used_direction: i32) -> bool {
    is_clut(icc, intent, used_direction) || crate::sm::is_matrix_shaper(icc)
}
