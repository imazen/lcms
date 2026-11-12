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

pub use crate::types::{
    INTENT_ABSOLUTE_COLORIMETRIC, INTENT_PERCEPTUAL, INTENT_PRESERVE_K_ONLY_PERCEPTUAL,
    INTENT_PRESERVE_K_ONLY_RELATIVE_COLORIMETRIC, INTENT_PRESERVE_K_ONLY_SATURATION,
    INTENT_PRESERVE_K_PLANE_PERCEPTUAL, INTENT_PRESERVE_K_PLANE_RELATIVE_COLORIMETRIC,
    INTENT_PRESERVE_K_PLANE_SATURATION, INTENT_RELATIVE_COLORIMETRIC, INTENT_SATURATION,
};

const FLAGS_NONEGATIVES: u32 = 0x8000;
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
            let m3 = *chad_in;
            let mut m4 = Mat3::default();
            if !crate::mtrx::mat3_inverse(&m3, &mut m4) {
                return false;
            }
            let _ = m3;
            crate::mtrx::mat3_per(m, &m2, &m4);
        } else {
            let m1 = *chad_in;
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
        // C ignores the detectors' return values — a failure leaves the
        // point at {0,0,0} and only the XYZ comparison runs
        let (before, after) = profiles.split_at_mut(i);
        let bp_in =
            crate::sm::detect_black_point(before[i - 1].as_mut(), intent, 0).unwrap_or(CieXyz {
                X: 0.0,
                Y: 0.0,
                Z: 0.0,
            });
        let bp_out = crate::sm::detect_destination_black_point(after[0].as_mut(), intent, 0)
            .unwrap_or(CieXyz {
                X: 0.0,
                Y: 0.0,
                Z: 0.0,
            });
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
/// `_cmsDefaultICCintents` — FFI-visible wrapper
#[doc(hidden)]
pub fn dbg_default_icc_intents(
    ctx: Option<&Context>,
    profiles: &mut [Box<IccProfile>],
    intents: &[u32],
    bpc: &mut [bool],
    adaptation_states: &[f64],
    dw_flags: u32,
) -> Option<Pipeline> {
    default_icc_intents(ctx, profiles, intents, bpc, adaptation_states, dw_flags)
}
fn default_icc_intents(
    /* traced */
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
        let lut: Option<Pipeline>;
        if is_device_link || (class_sig == pc::SigNamedColorClass && n == 1) {
            lut = io1::read_devicelink_lut(&mut profiles[i], intent);
            lut.as_ref()?;
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
                lut.as_ref()?;
            } else {
                lut = io1::read_output_lut(&mut profiles[i], intent);
                lut.as_ref()?;
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
    if crate::pcs::channels_of_color_space(color_space_out) != result.output_channels as i32 {
        return None;
    }
    Some(result)
}

/// FFI-installed hook for C-registered `cmsIntentFn` plugins — invokes the
/// plugin's `Link` over the raw profile/context handles and returns the
/// `cmsPipeline*` it built as a `Pipeline`. `lcms` forbids `unsafe`; the FFI
/// performs the transmute/`from_raw`.
pub type IntentLinkHook = fn(
    Option<&Context>,
    usize, // `cmsIntentFn` ptr
    &mut [Box<IccProfile>],
    &[u32],  // intents
    &[bool], // bpc
    &[f64],  // adaptation states
    u32,     // dwFlags
) -> Option<Box<Pipeline>>;
static INTENT_LINK_HOOK: std::sync::OnceLock<IntentLinkHook> = std::sync::OnceLock::new();
pub fn set_intent_link_hook(h: IntentLinkHook) {
    let _ = INTENT_LINK_HOOK.set(h);
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
    // C consults ctx-registered `cmsPluginRenderingIntent` `Link` fns for
    // non-standard intents before the built-in intent table
    // (`_cmsLinkProfiles`/`GetIntentPlugin`).
    if let Some(h) = INTENT_LINK_HOOK.get() {
        if let Some(cctx) = ctx.as_ref() {
            for ci in &cctx.c_intents {
                if ci.intent as u32 == intents[0] {
                    // C's `Intent->Link` returning NULL fails the transform —
                    // the result is not retried against the default table.
                    return h(
                        ctx.as_ref(),
                        ci.link,
                        profiles,
                        intents,
                        bpc,
                        adaptation_states,
                        dw_flags,
                    )
                    .map(|p| *p);
                }
            }
        }
    }
    link_profiles_inner(ctx, profiles, intents, bpc, adaptation_states, dw_flags)
}

fn link_profiles_inner(
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
    // `SearchIntent` — intents[0] must be a registered intent (default table
    // or ctx-registered), else "Unsupported intent" and no link at all.
    let registered = matches!(
        intents[0],
        INTENT_PERCEPTUAL
            | INTENT_RELATIVE_COLORIMETRIC
            | INTENT_SATURATION
            | INTENT_ABSOLUTE_COLORIMETRIC
            | INTENT_PRESERVE_K_ONLY_PERCEPTUAL
            | INTENT_PRESERVE_K_ONLY_RELATIVE_COLORIMETRIC
            | INTENT_PRESERVE_K_ONLY_SATURATION
            | INTENT_PRESERVE_K_PLANE_PERCEPTUAL
            | INTENT_PRESERVE_K_PLANE_RELATIVE_COLORIMETRIC
            | INTENT_PRESERVE_K_PLANE_SATURATION
    ) || ctx
        .as_ref()
        .is_some_and(|c| c.c_intents.iter().any(|ci| ci.intent as u32 == intents[0]));
    if !registered {
        crate::err::signal_error(
            ctx.as_ref(),
            crate::types::ERROR_UNKNOWN_EXTENSION,
            &format!("Unsupported intent '{}'", intents[0]),
        );
        return None;
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

/// `BlackPreservingKOnlyIntents` — CMYK→CMYK preserving pure-K
fn black_preserving_k_only_intents(
    ctx: Option<&Context>,
    profiles: &mut [Box<IccProfile>],
    intents: &[u32],
    bpc: &mut [bool],
    adaptation_states: &[f64],
    dw_flags: u32,
) -> Option<Pipeline> {
    use crate::signatures::colorspace as cs;
    use crate::signatures::profileclass as pc;
    let n = profiles.len();
    if n == 0 || n > 255 {
        return None;
    }
    let icc_intents: Vec<u32> = intents
        .iter()
        .map(|&i| translate_non_icc_intents(i))
        .collect();
    // Trim trailing CMYK→CMYK devicelinks
    let mut last_pos = n - 1;
    let mut last = last_pos;
    while is_cmyk_devicelink(&profiles[last]) {
        if last < 2 {
            break;
        }
        last -= 1;
    }
    last_pos = last;
    let preservation_count = last_pos + 1;
    // Check for non-CMYK input or non-CMYK/non-output last profile
    if profiles[0].color_space != cs::SigCmykData
        || !(profiles[last_pos].color_space == cs::SigCmykData
            || profiles[last_pos].device_class == pc::SigOutputClass)
    {
        return default_icc_intents(
            ctx,
            profiles,
            &icc_intents,
            bpc,
            adaptation_states,
            dw_flags,
        );
    }
    let mut result = crate::lut::pipeline_alloc(ctx, 4, 4)?;
    // normal ICC transform over the preservation profiles
    let cmyk2cmyk = default_icc_intents(
        ctx,
        &mut profiles[..preservation_count],
        &icc_intents[..preservation_count],
        &mut bpc[..preservation_count],
        &adaptation_states[..preservation_count],
        dw_flags,
    )?;
    let k_tone = build_k_tone_curve(
        ctx,
        4096,
        preservation_count,
        &icc_intents,
        profiles,
        bpc,
        adaptation_states,
        dw_flags,
    )?;
    let n_grid = crate::pcs::reasonable_gridpoints_by_colorspace(cs::SigCmykData, dw_flags);
    let mut clut = crate::lut::stage_alloc_clut16bit(ctx, n_grid, 4, 4, None)?;
    // GrayOnlySampler: pure-K → KTone eval; else the full cmyk2cmyk transform
    let mut sampler = move |inp: &[u16], out: &mut [u16]| -> i32 {
        if inp[0] == 0 && inp[1] == 0 && inp[2] == 0 {
            out[0] = 0;
            out[1] = 0;
            out[2] = 0;
            out[3] = k_tone.eval16(inp[3]);
            return 1;
        }
        cmyk2cmyk.eval16(inp, out);
        1
    };
    if !crate::lut::stage_sample_clut16bit(&mut clut, &mut sampler, 0) {
        return None;
    }
    if !result.insert_stage(crate::lut::StageLoc::AtBegin, clut) {
        return None;
    }
    // append the trailing devicelinks
    for i in last_pos + 1..n {
        let devlink = crate::io1::read_devicelink_lut(&mut profiles[i], icc_intents[i])?;
        if !result.cat(&devlink) {
            return None;
        }
    }
    Some(result)
}

/// `BlackPreservingKPlaneIntents` — CMYK→CMYK preserving the K plane
fn black_preserving_k_plane_intents(
    ctx: Option<&Context>,
    profiles: &mut [Box<IccProfile>],
    intents: &[u32],
    bpc: &mut [bool],
    adaptation_states: &[f64],
    dw_flags: u32,
) -> Option<Pipeline> {
    use crate::signatures::colorspace as cs;
    use crate::signatures::profileclass as pc;
    let n = profiles.len();
    if n == 0 || n > 255 {
        return None;
    }
    let icc_intents: Vec<u32> = intents
        .iter()
        .map(|&i| translate_non_icc_intents(i))
        .collect();
    let mut last_pos = n - 1;
    let mut last = last_pos;
    while is_cmyk_devicelink(&profiles[last]) {
        if last < 2 {
            break;
        }
        last -= 1;
    }
    last_pos = last;
    let preservation_count = last_pos + 1;
    if profiles[0].color_space != cs::SigCmykData
        || !(profiles[last_pos].color_space == cs::SigCmykData
            || profiles[last_pos].device_class == pc::SigOutputClass)
    {
        return default_icc_intents(
            ctx,
            profiles,
            &icc_intents,
            bpc,
            adaptation_states,
            dw_flags,
        );
    }
    // LabK→cmyk: the last profile's input LUT (searched inverse)
    let labk2cmyk = crate::io1::read_input_lut(
        &mut profiles[last_pos],
        crate::types::INTENT_RELATIVE_COLORIMETRIC,
    )?;
    // total area coverage in 0..1
    let max_tac = detect_tac(&mut profiles[last_pos]) / 100.0;
    if max_tac <= 0.0 {
        return None;
    }
    let cmyk2cmyk = default_icc_intents(
        ctx,
        &mut profiles[..preservation_count],
        &icc_intents[..preservation_count],
        &mut bpc[..preservation_count],
        &adaptation_states[..preservation_count],
        dw_flags,
    )?;
    let k_tone = build_k_tone_curve(
        ctx,
        4096,
        preservation_count,
        &icc_intents,
        profiles,
        bpc,
        adaptation_states,
        dw_flags,
    )?;
    // proof transforms on the last profile: CMYK→Lab (u16 and f32)
    let mut proof_u16 = make_last_proof(
        &mut profiles[last_pos],
        (6 << 16) | (4 << 3) | 2,
        TYPE_LAB_DBL,
    )?;
    let mut cmyk2lab = make_last_proof(
        &mut profiles[last_pos],
        (1 << 22) | (6 << 16) | (4 << 3) | 4,
        (1 << 22) | (10 << 16) | (3 << 3) | 4,
    )?;
    let n_grid = crate::pcs::reasonable_gridpoints_by_colorspace(cs::SigCmykData, dw_flags);
    let mut clut = crate::lut::stage_alloc_clut16bit(ctx, n_grid, 4, 4, None)?;
    let max_error = std::cell::Cell::new(0f64);
    // BlackPreservingSampler
    let mut sampler = move |inp: &[u16], out: &mut [u16]| -> i32 {
        let mut inf = [0f32; 4];
        for i in 0..4 {
            inf[i] = inp[i] as f32 / 65535.0;
        }
        let labk3 = k_tone.eval_float(inf[3]);
        if inp[0] == 0 && inp[1] == 0 && inp[2] == 0 {
            out[0] = 0;
            out[1] = 0;
            out[2] = 0;
            out[3] = crate::fixed::quick_saturate_word((labk3 * 65535.0) as f64);
            return 1;
        }
        let mut outf = [0f32; 4];
        cmyk2cmyk.eval_float(&inf, &mut outf);
        for i in 0..4 {
            out[i] = crate::fixed::quick_saturate_word((outf[i] * 65535.0) as f64);
        }
        if (outf[3] - labk3).abs() < 3.0 / 65535.0 {
            return 1;
        }
        // measure the colorimetric Lab of the transform output
        let o4: [u16; 4] = out[..4].try_into().unwrap();
        let colorimetric = run_lab_transform(&mut proof_u16, &o4);
        // Lab of output cmyk via the float transform
        let mut labk = [0f32; 4];
        labk[..3].copy_from_slice(&run_lab_transform_f(&mut cmyk2lab, &outf));
        labk[3] = labk3;
        // reverse-interpolate Lab+K → CMY(K fixed)
        let mut outf2 = [0f32; 4];
        if !labk2cmyk.eval_reverse_float(&labk, &mut outf2, Some(&outf)) {
            return 1; // fall back to colorimetric
        }
        outf2[3] = labk[3];
        // TAC clamp
        let sum_cmy = outf2[0] as f64 + outf2[1] as f64 + outf2[2] as f64;
        let sum_cmyk = sum_cmy + outf2[3] as f64;
        let ratio = if sum_cmyk > max_tac {
            let mut r = 1.0 - ((sum_cmyk - max_tac) / sum_cmy);
            if r < 0.0 {
                r = 0.0;
            }
            r
        } else {
            1.0
        };
        out[0] = crate::fixed::quick_saturate_word((outf2[0] * ratio as f32 * 65535.0) as f64);
        out[1] = crate::fixed::quick_saturate_word((outf2[1] * ratio as f32 * 65535.0) as f64);
        out[2] = crate::fixed::quick_saturate_word((outf2[2] * ratio as f32 * 65535.0) as f64);
        out[3] = crate::fixed::quick_saturate_word((outf2[3] * 65535.0) as f64);
        // error estimate
        let bp_lab = run_lab_transform(&mut proof_u16, &o4);
        let err = crate::pcs::delta_e(&colorimetric, &bp_lab);
        if err > max_error.get() {
            max_error.set(err);
        }
        1
    };
    if !crate::lut::stage_sample_clut16bit(&mut clut, &mut sampler, 0) {
        return None;
    }
    let mut result = crate::lut::pipeline_alloc(ctx, 4, 4)?;
    if !result.insert_stage(crate::lut::StageLoc::AtBegin, clut) {
        return None;
    }
    for i in last_pos + 1..n {
        let devlink = crate::io1::read_devicelink_lut(&mut profiles[i], icc_intents[i])?;
        if !result.cat(&devlink) {
            return None;
        }
    }
    Some(result)
}

/// a CMYK→Lab transform on the last profile for proofing
fn make_last_proof(
    p: &mut IccProfile,
    in_fmt: u32,
    out_fmt: u32,
) -> Option<Box<crate::xform::Transform>> {
    let lab = crate::virt::create_lab4_profile(None, None)?;
    let pd = dup_box(p)?;
    let mut profs = vec![pd, lab];
    let mut bpc = [false, false];
    let intents = [crate::types::INTENT_RELATIVE_COLORIMETRIC; 2];
    let adapt = [1.0f64, 1.0];
    crate::xform::create_extended_transform(
        None,
        &mut profs,
        &mut bpc,
        &intents,
        &adapt,
        None,
        0,
        in_fmt,
        out_fmt,
        crate::types::FLAGS_NOOPTIMIZE | crate::types::FLAGS_NOCACHE,
    )
}

/// run a u16-CMYK → Lab(dbl) transform, return CieLab
fn run_lab_transform(t: &mut crate::xform::Transform, cmyk16: &[u16; 4]) -> crate::types::CieLab {
    let mut inb = Vec::with_capacity(8);
    for &v in cmyk16.iter().take(4) {
        inb.extend_from_slice(&v.to_ne_bytes());
    }
    let mut outb = vec![0u8; 24];
    t.do_transform(&inb, &mut outb, 1);
    crate::types::CieLab {
        L: f64::from_ne_bytes(outb[0..8].try_into().unwrap()),
        a: f64::from_ne_bytes(outb[8..16].try_into().unwrap()),
        b: f64::from_ne_bytes(outb[16..24].try_into().unwrap()),
    }
}

/// run an f32-CMYK → Lab(f32) transform, return [L,a,b]
fn run_lab_transform_f(t: &mut crate::xform::Transform, cmyk: &[f32; 4]) -> [f32; 3] {
    let mut inb = Vec::with_capacity(16);
    for &v in cmyk.iter() {
        inb.extend_from_slice(&v.to_ne_bytes());
    }
    let mut outb = vec![0u8; 12];
    t.do_transform(&inb, &mut outb, 1);
    [
        f32::from_ne_bytes(outb[0..4].try_into().unwrap()),
        f32::from_ne_bytes(outb[4..8].try_into().unwrap()),
        f32::from_ne_bytes(outb[8..12].try_into().unwrap()),
    ]
}

// ---------------------------------------------------------------------------
// `cmsgmt.c`/`cmscnvrt.c` black-preserving intents — CMYK→CMYK K-only.
// ---------------------------------------------------------------------------

const TYPE_CMYK_FLT: u32 = (1 << 22) | (6 << 16) | (4 << 3) | 4;
const TYPE_LAB_DBL: u32 = (1 << 22) | (10 << 16) | (3 << 3);

/// `_cmsChain2Lab` — profiles + a trailing Lab4 profile → transform to Lab.
fn chain2lab(
    ctx: Option<&Context>,
    n_profiles: usize,
    input_format: u32,
    output_format: u32,
    intents: &[u32],
    profiles: &mut [Box<IccProfile>],
    bpc: &mut [bool],
    adaptation_states: &[f64],
    dw_flags: u32,
) -> Option<Box<crate::xform::Transform>> {
    if n_profiles > 254 || n_profiles > profiles.len() {
        return None;
    }
    let lab = crate::virt::create_lab4_profile(ctx.cloned(), None)?;
    // clone the slice of profiles + append Lab
    let mut plist: Vec<Box<IccProfile>> = Vec::new();
    // C reuses the caller's handles; clone them (a serialize+reopen copy
    // would re-encode already-decoded tags).
    for p in profiles.iter().take(n_profiles) {
        plist.push(p.clone());
    }
    plist.push(Box::new(*lab));
    let mut bpc_list = vec![false; n_profiles + 1];
    bpc_list[..n_profiles].copy_from_slice(&bpc[..n_profiles]);
    let mut intent_list = vec![0u32; n_profiles + 1];
    intent_list[..n_profiles].copy_from_slice(&intents[..n_profiles]);
    intent_list[n_profiles] = crate::types::INTENT_RELATIVE_COLORIMETRIC;
    let mut adapt_list = vec![1.0f64; n_profiles + 1];
    adapt_list[..n_profiles].copy_from_slice(&adaptation_states[..n_profiles]);
    crate::xform::create_extended_transform(
        None,
        &mut plist,
        &mut bpc_list,
        &intent_list,
        &adapt_list,
        None,
        0,
        input_format,
        output_format,
        dw_flags,
    )
}

/// `ComputeKToLstar` — K→L* curve via a CMYK→Lab transform on pure-K inputs
fn compute_k_to_lstar(
    ctx: Option<&Context>,
    n_points: usize,
    n_profiles: usize,
    intents: &[u32],
    profiles: &mut [Box<IccProfile>],
    bpc: &mut [bool],
    adaptation_states: &[f64],
    dw_flags: u32,
) -> Option<crate::gamma::ToneCurve> {
    let mut xform = chain2lab(
        ctx,
        n_profiles,
        TYPE_CMYK_FLT,
        TYPE_LAB_DBL,
        intents,
        profiles,
        bpc,
        adaptation_states,
        dw_flags,
    )?;
    let mut sampled = Vec::with_capacity(n_points);
    for i in 0..n_points {
        let cmyk: [f32; 4] = [0.0, 0.0, 0.0, (i as f32 * 100.0) / (n_points - 1) as f32];
        // feed the f32 cmyk as bytes into do_transform; read f64 Lab out
        let mut inb = Vec::with_capacity(16);
        for v in cmyk {
            inb.extend_from_slice(&v.to_ne_bytes());
        }
        let mut outb = vec![0u8; 24];
        xform.do_transform(&inb, &mut outb, 1);
        let l = f64::from_ne_bytes(outb[0..8].try_into().unwrap());
        sampled.push(1.0 - (l / 100.0) as f32); // Negate K for easier operation
    }
    crate::gamma::build_tabulated_tone_curve_float(ctx, &sampled)
}

/// `_cmsBuildKToneCurve` — K→K curve: input K→L* joined to output L*→K
fn build_k_tone_curve(
    ctx: Option<&Context>,
    n_points: usize,
    n_profiles: usize,
    intents: &[u32],
    profiles: &mut [Box<IccProfile>],
    bpc: &mut [bool],
    adaptation_states: &[f64],
    dw_flags: u32,
) -> Option<crate::gamma::ToneCurve> {
    use crate::signatures::colorspace as cs;
    use crate::signatures::profileclass as pc;
    if profiles[0].color_space != cs::SigCmykData
        || profiles[n_profiles - 1].color_space != cs::SigCmykData
    {
        return None;
    }
    if profiles[n_profiles - 1].device_class != pc::SigOutputClass {
        return None;
    }
    let in_ = compute_k_to_lstar(
        ctx,
        n_points,
        n_profiles - 1,
        intents,
        &mut profiles[..n_profiles],
        &mut bpc[..n_profiles],
        &adaptation_states[..n_profiles],
        dw_flags,
    )?;
    let out = compute_k_to_lstar(
        ctx,
        n_points,
        1,
        &intents[n_profiles - 1..],
        &mut profiles[n_profiles - 1..],
        &mut bpc[n_profiles - 1..],
        &adaptation_states[n_profiles - 1..],
        dw_flags,
    )?;
    let k_tone = crate::gamma::join_tone_curve(ctx, &in_, &out, n_points as u32)?;
    if !crate::gamma::is_tone_curve_monotonic(&k_tone) {
        return None;
    }
    Some(k_tone)
}

/// `is_cmyk_devicelink`
fn is_cmyk_devicelink(p: &IccProfile) -> bool {
    use crate::signatures::colorspace as cs;
    use crate::signatures::profileclass as pc;
    p.device_class == pc::SigLinkClass && p.color_space == cs::SigCmykData
}

/// `TranslateNonICCIntents` — map K-preserving intents to their ICC base
fn translate_non_icc_intents(intent: u32) -> u32 {
    use crate::types::*;
    match intent {
        INTENT_PRESERVE_K_ONLY_PERCEPTUAL | INTENT_PRESERVE_K_PLANE_PERCEPTUAL => INTENT_PERCEPTUAL,
        INTENT_PRESERVE_K_ONLY_RELATIVE_COLORIMETRIC
        | INTENT_PRESERVE_K_PLANE_RELATIVE_COLORIMETRIC => INTENT_RELATIVE_COLORIMETRIC,
        INTENT_PRESERVE_K_ONLY_SATURATION | INTENT_PRESERVE_K_PLANE_SATURATION => INTENT_SATURATION,
        _ => intent,
    }
}

/// `cmsFormatterForColorspaceOfProfile`
fn formatter_for_colorspace_of_profile(p: &IccProfile, n_bytes: u32, is_float: bool) -> u32 {
    let color_space_bits = crate::pcs::lcms_color_space(p.color_space) as u32;
    let n_out = crate::pcs::channels_of_color_space(p.color_space);
    if n_out < 0 {
        return 0;
    }
    let float = if is_float { 1u32 } else { 0 };
    (float << 22) | (color_space_bits << 16) | (n_bytes & 7) | ((n_out as u32) << 3)
}

/// `cmsDetectTAC` — total area coverage via a Lab→profile roundtrip sampled
/// over Lab space; returns the max ink sum in %
#[doc(hidden)]
pub fn dbg_detect_tac(p: &mut IccProfile) -> f64 {
    detect_tac(p)
}
fn detect_tac(p: &mut IccProfile) -> f64 {
    use crate::signatures::profileclass as pc;
    if p.device_class != pc::SigOutputClass {
        return 0.0;
    }
    let formatter = formatter_for_colorspace_of_profile(p, 4, true);
    if formatter == 0 {
        return 0.0;
    }
    let n_out = crate::pcs::channels_of_color_space(p.color_space);
    if n_out < 0 || n_out as usize >= crate::lut::MAX_CHANNELS {
        return 0.0;
    }
    let n_out = n_out as usize;
    // Lab → profile transform (perceptual, no opt/cache)
    let lab = match crate::virt::create_lab4_profile(None, None) {
        Some(l) => l,
        None => return 0.0,
    };
    let pdup = match dup_box(&mut *p) {
        Some(b) => b,
        None => return 0.0,
    };
    let mut profs = vec![lab, pdup];
    let mut bpc = [false, false];
    let intents = [
        crate::types::INTENT_PERCEPTUAL,
        crate::types::INTENT_PERCEPTUAL,
    ];
    let adapt = [1.0f64, 1.0];
    let type_lab_16: u32 = (10 << 16) | (3 << 3) | 2;
    let mut rt = match crate::xform::create_extended_transform(
        None,
        &mut profs,
        &mut bpc,
        &intents,
        &adapt,
        None,
        0,
        type_lab_16,
        formatter,
        crate::types::FLAGS_NOOPTIMIZE | crate::types::FLAGS_NOCACHE,
    ) {
        Some(t) => t,
        None => {
            eprintln!("[tac] transform failed");
            return 0.0;
        }
    };
    // sample Lab space [6,74,74] — track max total ink
    let grid = [6u32, 74, 74];
    let max_tac = std::rc::Rc::new(std::cell::Cell::new(0f32));
    let max_tac2 = max_tac.clone();
    let mut sampler = move |inp: &[u16], _out: &mut [u16]| -> i32 {
        // In is Lab16 (3ch); run the roundtrip, sum the f32 colorant
        let mut inb = Vec::with_capacity(6);
        for &v in inp.iter().take(3) {
            inb.extend_from_slice(&v.to_ne_bytes());
        }
        let n_b = n_out * 4; // f32 per channel
        let mut outb = vec![0u8; n_b];
        rt.do_transform(&inb, &mut outb, 1);
        let mut sum = 0f32;
        for i in 0..n_out {
            sum += f32::from_ne_bytes(outb[i * 4..i * 4 + 4].try_into().unwrap());
        }
        if sum > max_tac2.get() {
            max_tac2.set(sum);
        }
        1
    };
    if !crate::lut::slice_space16(3, &grid, &mut sampler) {
        return 0.0;
    }
    max_tac.get() as f64
}

/// Duplicate a profile handle — C reuses the same handle; a clone keeps
/// decoded tags decoded (serialize+reopen would re-encode them).
fn dup_box(p: &mut IccProfile) -> Option<Box<IccProfile>> {
    Some(Box::new(p.clone()))
}
