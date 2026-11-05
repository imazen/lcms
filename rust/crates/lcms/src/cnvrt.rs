//! `cmscnvrt.c` — intent linking: `DefaultICCintents` plus the conversion
//! helpers (absolute-intent CHAD math, BPC, AddConversion stage plumbing).

use crate::context::Context;
use crate::io1;
use crate::lut::{Pipeline, StageLoc, pipeline_alloc, stage_alloc_matrix};
use crate::profile::{self, IccProfile};
use crate::signatures::colorspace as cs;
use crate::signatures::profileclass as pc;
use crate::types::{CieXyz, Mat3, Vec3};
use crate::wtpnt::{adaptation_matrix, d50_xyz, temp_from_white_point, white_point_from_temp};

pub const INTENT_PERCEPTUAL: u32 = 0;
pub const INTENT_RELATIVE_COLORIMETRIC: u32 = 1;
pub const INTENT_SATURATION: u32 = 2;
pub const INTENT_ABSOLUTE_COLORIMETRIC: u32 = 3;
pub const INTENT_PRESERVE_K_ONLY_PERCEPTUAL: u32 = 10;
pub const INTENT_PRESERVE_K_ONLY_RELATIVE_COLORIMETRIC: u32 = 11;
pub const INTENT_PRESERVE_K_ONLY_SATURATION: u32 = 12;
pub const INTENT_PRESERVE_K_PLANE_PERCEPTUAL: u32 = 13;
pub const INTENT_PRESERVE_K_PLANE_RELATIVE_COLORIMETRIC: u32 = 14;
pub const INTENT_PRESERVE_K_PLANE_SATURATION: u32 = 15;

const FLAGS_NONEGATIVES: u32 = 0x0040;
const MAX_ENC_XYZ: f64 = (2.0 * 65535.0) / 65536.0;

/// `ComputeBlackPointCompensation`
fn compute_black_point_compensation(bp_in: &CieXyz, bp_out: &CieXyz, m: &mut Mat3, off: &mut Vec3) {
    let d50 = d50_xyz();
    let tx = bp_in.X - d50.X;
    let ty = bp_in.Y - d50.Y;
    let tz = bp_in.Z - d50.Z;
    let ax = (bp_out.X - d50.X) / tx;
    let ay = (bp_out.Y - d50.Y) / ty;
    let az = (bp_out.Z - d50.Z) / tz;
    let bx = -d50.X * (bp_out.X - bp_in.X) / tx;
    let by = -d50.Y * (bp_out.Y - bp_in.Y) / ty;
    let bz = -d50.Z * (bp_out.Z - bp_in.Z) / tz;
    m.v[0] = Vec3 { n: [ax, 0.0, 0.0] };
    m.v[1] = Vec3 { n: [0.0, ay, 0.0] };
    m.v[2] = Vec3 { n: [0.0, 0.0, az] };
    *off = Vec3 { n: [bx, by, bz] };
}

/// `CHAD2Temp` — recover a correlated temperature from a CHAD matrix
fn chad2temp(chad: &Mat3) -> f64 {
    let mut m2 = Mat3::default();
    if !crate::mtrx::mat3_inverse(chad, &mut m2) {
        return -1.0;
    }
    let d50 = d50_xyz();
    let s = Vec3 {
        n: [d50.X, d50.Y, d50.Z],
    };
    let mut d = Vec3::default();
    crate::mtrx::mat3_eval(&mut d, &m2, &s);
    let dest = CieXyz {
        X: d.n[0],
        Y: d.n[1],
        Z: d.n[2],
    };
    let ch = crate::pcs::xyz2xyy(&dest);
    temp_from_white_point(&ch).unwrap_or(-1.0)
}

/// `Temp2CHAD`
fn temp2chad(chad: &mut Mat3, temp: f64) {
    let chroma = white_point_from_temp(temp).unwrap_or_default();
    let white = crate::pcs::xyy2xyz(&chroma);
    adaptation_matrix(chad, None, &white, &d50_xyz());
}

/// `ComputeAbsoluteIntent` — the absolute-colorimetric adaptation matrix
fn compute_absolute_intent(
    adaptation_state: f64,
    wp_in: &CieXyz,
    chad_in: &Mat3,
    wp_out: &CieXyz,
    chad_out: &Mat3,
    m: &mut Mat3,
) -> bool {
    let mut scale = Mat3::default();
    if adaptation_state == 1.0 {
        m.v[0] = Vec3 {
            n: [wp_in.X / wp_out.X, 0.0, 0.0],
        };
        m.v[1] = Vec3 {
            n: [0.0, wp_in.Y / wp_out.Y, 0.0],
        };
        m.v[2] = Vec3 {
            n: [0.0, 0.0, wp_in.Z / wp_out.Z],
        };
    } else {
        scale.v[0] = Vec3 {
            n: [wp_in.X / wp_out.X, 0.0, 0.0],
        };
        scale.v[1] = Vec3 {
            n: [0.0, wp_in.Y / wp_out.Y, 0.0],
        };
        scale.v[2] = Vec3 {
            n: [0.0, 0.0, wp_in.Z / wp_out.Z],
        };
        if adaptation_state == 0.0 {
            let mut m2 = Mat3::default();
            crate::mtrx::mat3_per(&mut m2, chad_out, &scale);
            let mut m3 = *chad_in;
            let mut m4 = Mat3::default();
            if !crate::mtrx::mat3_inverse(&m3, &mut m4) {
                return false;
            }
            let _ = m3;
            crate::mtrx::mat3_per(m, &m2, &m4);
        } else {
            let mut m1 = *chad_in;
            let mut m2 = Mat3::default();
            if !crate::mtrx::mat3_inverse(&m1, &mut m2) {
                return false;
            }
            let mut m3 = Mat3::default();
            crate::mtrx::mat3_per(&mut m3, &m2, &scale);
            let temp_src = chad2temp(chad_in);
            let temp_dest = chad2temp(chad_out);
            if temp_src < 0.0 || temp_dest < 0.0 {
                return false;
            }
            if crate::mtrx::mat3_is_identity(&scale) && (temp_src - temp_dest).abs() < 0.01 {
                crate::mtrx::mat3_identity(m);
                return true;
            }
            let temp = (1.0 - adaptation_state) * temp_dest + adaptation_state * temp_src;
            let mut mixed = Mat3::default();
            temp2chad(&mut mixed, temp);
            crate::mtrx::mat3_per(m, &m3, &mixed);
        }
    }
    true
}

/// `IsEmptyLayer` — identity matrix + zero offset within 0.002
fn is_empty_layer(m: &Mat3, off: &Vec3) -> bool {
    let mut diff = 0.0f64;
    let mut ident = Mat3::default();
    crate::mtrx::mat3_identity(&mut ident);
    for i in 0..3 {
        for j in 0..3 {
            diff += (m.v[i].n[j] - ident.v[i].n[j]).abs();
        }
        diff += off.n[i].abs();
    }
    diff < 0.002
}

/// `ComputeConversion` — the inter-profile conversion (abs intent or BPC)
fn compute_conversion(
    i: usize,
    profiles: &mut [Box<IccProfile>],
    intent: u32,
    bpc: bool,
    adaptation_state: f64,
    m: &mut Mat3,
    off: &mut Vec3,
) -> bool {
    crate::mtrx::mat3_identity(m);
    *off = Vec3 { n: [0.0, 0.0, 0.0] };
    if intent == INTENT_ABSOLUTE_COLORIMETRIC {
        let (before, after) = profiles.split_at_mut(i);
        let wp_in = io1::read_media_white_point(before[i - 1].as_mut());
        let chad_in = io1::read_chad(before[i - 1].as_mut());
        let wp_out = io1::read_media_white_point(after[0].as_mut());
        let chad_out = io1::read_chad(after[0].as_mut());
        if !compute_absolute_intent(adaptation_state, &wp_in, &chad_in, &wp_out, &chad_out, m) {
            return false;
        }
    } else if bpc {
        // black point detection lives in cmssamp — needs the transform engine
        let (before, after) = profiles.split_at_mut(i);
        let Some(bp_in) = crate::sm::detect_black_point(before[i - 1].as_mut(), intent, 0) else {
            return false;
        };
        let Some(bp_out) = crate::sm::detect_destination_black_point(after[0].as_mut(), intent, 0)
        else {
            return false;
        };
        if bp_in.X != bp_out.X || bp_in.Y != bp_out.Y || bp_in.Z != bp_out.Z {
            compute_black_point_compensation(&bp_in, &bp_out, m, off);
        }
    }
    for k in 0..3 {
        off.n[k] /= MAX_ENC_XYZ;
    }
    true
}

/// `AddConversion` — insert the conversion stages between two PCSs
fn add_conversion(
    ctx: Option<&Context>,
    result: &mut Pipeline,
    in_pcs: u32,
    out_pcs: u32,
    m: &Mat3,
    off: &Vec3,
) -> bool {
    let mut m_flat = [0.0f64; 9];
    let mut o_flat = [0.0f64; 3];
    for r in 0..3 {
        for c in 0..3 {
            m_flat[r * 3 + c] = m.v[r].n[c];
        }
        o_flat[r] = off.n[r];
    }
    let empty = is_empty_layer(m, off);
    match in_pcs {
        x if x == cs::SigXYZData => match out_pcs {
            y if y == cs::SigXYZData => {
                if !empty {
                    let Some(st) = stage_alloc_matrix(ctx, 3, 3, &m_flat, Some(&o_flat)) else {
                        return false;
                    };
                    if !result.insert_stage(StageLoc::AtEnd, st) {
                        return false;
                    }
                }
            }
            y if y == cs::SigLabData => {
                if !empty {
                    let Some(st) = stage_alloc_matrix(ctx, 3, 3, &m_flat, Some(&o_flat)) else {
                        return false;
                    };
                    if !result.insert_stage(StageLoc::AtEnd, st) {
                        return false;
                    }
                }
                let st = crate::lut::stage_alloc_xyz2lab(ctx);
                if !result.insert_stage(StageLoc::AtEnd, st) {
                    return false;
                }
            }
            _ => return false,
        },
        x if x == cs::SigLabData => match out_pcs {
            y if y == cs::SigXYZData => {
                let st = crate::lut::stage_alloc_lab2xyz(ctx);
                if !result.insert_stage(StageLoc::AtEnd, st) {
                    return false;
                }
                if !empty {
                    let Some(st) = stage_alloc_matrix(ctx, 3, 3, &m_flat, Some(&o_flat)) else {
                        return false;
                    };
                    if !result.insert_stage(StageLoc::AtEnd, st) {
                        return false;
                    }
                }
            }
            y if y == cs::SigLabData => {
                if !empty {
                    let s1 = crate::lut::stage_alloc_lab2xyz(ctx);
                    if !result.insert_stage(StageLoc::AtEnd, s1) {
                        return false;
                    }
                    let Some(s2) = stage_alloc_matrix(ctx, 3, 3, &m_flat, Some(&o_flat)) else {
                        return false;
                    };
                    if !result.insert_stage(StageLoc::AtEnd, s2) {
                        return false;
                    }
                    let s3 = crate::lut::stage_alloc_xyz2lab(ctx);
                    if !result.insert_stage(StageLoc::AtEnd, s3) {
                        return false;
                    }
                }
            }
            _ => return false,
        },
        _ => {
            if in_pcs != out_pcs {
                return false;
            }
        }
    }
    true
}

/// `ColorSpaceIsCompatible`
fn color_space_is_compatible(a: u32, b: u32) -> bool {
    a == b
        || (a == cs::Sig4colorData && b == cs::SigCmykData)
        || (a == cs::SigCmykData && b == cs::Sig4colorData)
        || (a == cs::SigXYZData && b == cs::SigLabData)
        || (a == cs::SigLabData && b == cs::SigXYZData)
}

/// `DefaultICCintents` — link n profiles into one pipeline
fn default_icc_intents(
    ctx: Option<&Context>,
    profiles: &mut [Box<IccProfile>],
    intents: &[u32],
    bpc: &mut [bool],
    adaptation_states: &[f64],
    dw_flags: u32,
) -> Option<Pipeline> {
    let n = profiles.len();
    if n == 0 {
        return None;
    }
    let mut result = pipeline_alloc(ctx, 0, 0)?;
    let mut current_color_space = profiles[0].color_space;
    let mut color_space_out = cs::SigLabData;

    for i in 0..n {
        let class_sig = profiles[i].device_class;
        let is_device_link = class_sig == pc::SigLinkClass || class_sig == pc::SigAbstractClass;
        let is_input = if i == 0 && !is_device_link {
            true
        } else {
            current_color_space != cs::SigXYZData && current_color_space != cs::SigLabData
        };
        let intent = intents[i];
        let (color_space_in, co) = if is_input || is_device_link {
            (profiles[i].color_space, profiles[i].pcs)
        } else {
            (profiles[i].pcs, profiles[i].color_space)
        };
        color_space_out = co;
        if !color_space_is_compatible(color_space_in, current_color_space) {
            crate::err::signal_error(
                ctx,
                crate::types::ERROR_COLORSPACE_CHECK,
                "ColorSpace mismatch",
            );
            return None;
        }

        let mut m = Mat3::default();
        let mut off = Vec3::default();
        let mut lut: Option<Pipeline>;
        if is_device_link || (class_sig == pc::SigNamedColorClass && n == 1) {
            lut = io1::read_devicelink_lut(&mut profiles[i], intent);
            if lut.is_none() {
                return None;
            }
            if class_sig == pc::SigAbstractClass && i > 0 {
                if !compute_conversion(
                    i,
                    profiles,
                    intent,
                    bpc[i],
                    adaptation_states[i],
                    &mut m,
                    &mut off,
                ) {
                    return None;
                }
            } else {
                crate::mtrx::mat3_identity(&mut m);
                off = Vec3 { n: [0.0, 0.0, 0.0] };
            }
            if !add_conversion(
                ctx,
                &mut result,
                current_color_space,
                color_space_in,
                &m,
                &off,
            ) {
                return None;
            }
        } else {
            if is_input {
                lut = io1::read_input_lut(&mut profiles[i], intent);
                if lut.is_none() {
                    return None;
                }
            } else {
                lut = io1::read_output_lut(&mut profiles[i], intent);
                if lut.is_none() {
                    return None;
                }
                if !compute_conversion(
                    i,
                    profiles,
                    intent,
                    bpc[i],
                    adaptation_states[i],
                    &mut m,
                    &mut off,
                ) {
                    return None;
                }
                if !add_conversion(
                    ctx,
                    &mut result,
                    current_color_space,
                    color_space_in,
                    &m,
                    &off,
                ) {
                    return None;
                }
            }
        }

        if !result.cat(&lut.unwrap()) {
            return None;
        }
        current_color_space = color_space_out;
    }

    if dw_flags & FLAGS_NONEGATIVES != 0
        && matches!(color_space_out, x if x == cs::SigGrayData || x == cs::SigRgbData || x == cs::SigCmykData)
    {
        let n_ch = crate::pcs::channels_of_color_space(color_space_out);
        let clip = crate::lut::stage_clip_negatives(ctx, n_ch as u32);
        if !result.insert_stage(StageLoc::AtEnd, clip) {
            return None;
        }
    }
    Some(result)
}

/// `_cmsLinkProfiles` — intent lookup then link
pub fn link_profiles(
    ctx: Option<Context>,
    profiles: &mut [Box<IccProfile>],
    intents: &[u32],
    bpc: &mut [bool],
    adaptation_states: &[f64],
    dw_flags: u32,
) -> Option<Pipeline> {
    let n = profiles.len();
    if n == 0 || n > 255 {
        crate::err::signal_error(
            ctx.as_ref(),
            crate::types::ERROR_RANGE,
            "Couldn't link profiles",
        );
        return None;
    }
    for i in 0..n {
        if intents[i] == INTENT_ABSOLUTE_COLORIMETRIC {
            bpc[i] = false;
        }
        if (intents[i] == INTENT_PERCEPTUAL || intents[i] == INTENT_SATURATION)
            && profile::get_encoded_icc_version(&profiles[i]) >= 0x4000000
        {
            bpc[i] = true;
        }
    }
    // default + black-preserving intents dispatch via the intent table
    match intents[0] {
        INTENT_PRESERVE_K_ONLY_PERCEPTUAL
        | INTENT_PRESERVE_K_ONLY_RELATIVE_COLORIMETRIC
        | INTENT_PRESERVE_K_ONLY_SATURATION => black_preserving_k_only_intents(
            ctx.as_ref(),
            profiles,
            intents,
            bpc,
            adaptation_states,
            dw_flags,
        ),
        INTENT_PRESERVE_K_PLANE_PERCEPTUAL
        | INTENT_PRESERVE_K_PLANE_RELATIVE_COLORIMETRIC
        | INTENT_PRESERVE_K_PLANE_SATURATION => black_preserving_k_plane_intents(
            ctx.as_ref(),
            profiles,
            intents,
            bpc,
            adaptation_states,
            dw_flags,
        ),
        _ => default_icc_intents(
            ctx.as_ref(),
            profiles,
            intents,
            bpc,
            adaptation_states,
            dw_flags,
        ),
    }
}

// --- Black-preserving intents: need cmssamp (TAC) + xform; ported later ---
fn black_preserving_k_only_intents(
    _ctx: Option<&Context>,
    _profiles: &mut [Box<IccProfile>],
    _intents: &[u32],
    _bpc: &mut [bool],
    _adaptation_states: &[f64],
    _dw_flags: u32,
) -> Option<Pipeline> {
    // TODO(cmscnvrt): BlackPreservingKOnlyIntents — needs cmsDetectTAC +
    // precalculated white/black curves over the transform engine
    crate::err::signal_error(
        _ctx,
        crate::types::ERROR_UNKNOWN_EXTENSION,
        "K-only intent not yet ported",
    );
    None
}

fn black_preserving_k_plane_intents(
    _ctx: Option<&Context>,
    _profiles: &mut [Box<IccProfile>],
    _intents: &[u32],
    _bpc: &mut [bool],
    _adaptation_states: &[f64],
    _dw_flags: u32,
) -> Option<Pipeline> {
    crate::err::signal_error(
        _ctx,
        crate::types::ERROR_UNKNOWN_EXTENSION,
        "K-plane intent not yet ported",
    );
    None
}
