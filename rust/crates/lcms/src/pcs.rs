//! Port of `cmspcs.c` — PCS-side colour-space conversions and encodings.
//!
//! C source: `reference/lcms2/src/cmspcs.c`
//! Gate: `tests/c_parity_pcs.rs`

use crate::fixed::{quick_saturate_word, s15_fixed16_to_double};
use crate::signatures::colorspace as cs;
use crate::types::{*, CpowExt};
use crate::wtpnt::d50_xyz;
use std::f64::consts::PI;

const MAX_ENCODEABLE_XYZ: f64 = 1.0 + 32767.0 / 32768.0;
const MIN_ENCODEABLE_AB2: f64 = -128.0;
const MAX_ENCODEABLE_AB2: f64 = (65535.0 / 256.0) - 128.0;
const MIN_ENCODEABLE_AB4: f64 = -128.0;
const MAX_ENCODEABLE_AB4: f64 = 127.0;

/// `cmsXYZ2xyY`
pub fn xyz2xyy(source: &CieXyz) -> CiexyY {
    let i_sum = 1. / (source.X + source.Y + source.Z);

    CiexyY {
        x: source.X * i_sum,
        y: source.Y * i_sum,
        Y: source.Y,
    }
}

/// `cmsxyY2XYZ`
pub fn xyy2xyz(source: &CiexyY) -> CieXyz {
    CieXyz {
        X: (source.x / source.y) * source.Y,
        Y: source.Y,
        Z: ((1.0 - source.x - source.y) / source.y) * source.Y,
    }
}

/// `f` (static in C)
fn f(t: f64) -> f64 {
    const LIMIT: f64 = (24.0 / 116.0) * (24.0 / 116.0) * (24.0 / 116.0);

    if t <= LIMIT {
        (841.0 / 108.0) * t + (16.0 / 116.0)
    } else {
        t.cpow(1.0 / 3.0)
    }
}

/// `f_1` (static in C)
fn f_1(t: f64) -> f64 {
    const LIMIT: f64 = 24.0 / 116.0;

    if t <= LIMIT {
        (108.0 / 841.0) * (t - (16.0 / 116.0))
    } else {
        t * t * t
    }
}

/// `cmsXYZ2Lab` — standard XYZ to Lab. WhitePoint defaults to D50 (`None`).
pub fn xyz2lab(white_point: Option<&CieXyz>, xyz: &CieXyz) -> CieLab {
    let wp = white_point.copied().unwrap_or_else(d50_xyz);

    let fx = f(xyz.X / wp.X);
    let fy = f(xyz.Y / wp.Y);
    let fz = f(xyz.Z / wp.Z);

    CieLab {
        L: 116.0 * fy - 16.0,
        a: 500.0 * (fx - fy),
        b: 200.0 * (fy - fz),
    }
}

/// `cmsLab2XYZ` — it can return negative XYZ in some cases
pub fn lab2xyz(white_point: Option<&CieXyz>, lab: &CieLab) -> CieXyz {
    let wp = white_point.copied().unwrap_or_else(d50_xyz);

    let y = (lab.L + 16.0) / 116.0;
    let x = y + 0.002 * lab.a;
    let z = y - 0.005 * lab.b;

    CieXyz {
        X: f_1(x) * wp.X,
        Y: f_1(y) * wp.Y,
        Z: f_1(z) * wp.Z,
    }
}

fn l2float2(v: u16) -> f64 {
    v as f64 / 652.800
}

fn ab2float2(v: u16) -> f64 {
    (v as f64 / 256.0) - 128.0
}

fn l2fix2(l: f64) -> u16 {
    quick_saturate_word(l * 652.8)
}

fn ab2fix2(ab: f64) -> u16 {
    quick_saturate_word((ab + 128.0) * 256.0)
}

fn l2float4(v: u16) -> f64 {
    v as f64 / 655.35
}

fn ab2float4(v: u16) -> f64 {
    (v as f64 / 257.0) - 128.0
}

/// `cmsLabEncoded2FloatV2`
pub fn lab_encoded2float_v2(w_lab: [u16; 3]) -> CieLab {
    CieLab {
        L: l2float2(w_lab[0]),
        a: ab2float2(w_lab[1]),
        b: ab2float2(w_lab[2]),
    }
}

/// `cmsLabEncoded2Float` (V4 encoding)
pub fn lab_encoded2float(w_lab: [u16; 3]) -> CieLab {
    CieLab {
        L: l2float4(w_lab[0]),
        a: ab2float4(w_lab[1]),
        b: ab2float4(w_lab[2]),
    }
}

fn clamp_l_double_v2(mut l: f64) -> f64 {
    let l_max = (0xFFFF as f64 * 100.0) / 0xFF00 as f64;

    if l < 0.0 {
        l = 0.0;
    }
    if l > l_max {
        l = l_max;
    }
    l
}

fn clamp_ab_double_v2(mut ab: f64) -> f64 {
    if ab < MIN_ENCODEABLE_AB2 {
        ab = MIN_ENCODEABLE_AB2;
    }
    if ab > MAX_ENCODEABLE_AB2 {
        ab = MAX_ENCODEABLE_AB2;
    }
    ab
}

/// `cmsFloat2LabEncodedV2`
pub fn float2lab_encoded_v2(f_lab: &CieLab) -> [u16; 3] {
    let lab = CieLab {
        L: clamp_l_double_v2(f_lab.L),
        a: clamp_ab_double_v2(f_lab.a),
        b: clamp_ab_double_v2(f_lab.b),
    };

    [l2fix2(lab.L), ab2fix2(lab.a), ab2fix2(lab.b)]
}

fn clamp_l_double_v4(mut l: f64) -> f64 {
    if l < 0.0 {
        l = 0.0;
    }
    if l > 100.0 {
        l = 100.0;
    }
    l
}

fn clamp_ab_double_v4(mut ab: f64) -> f64 {
    if ab < MIN_ENCODEABLE_AB4 {
        ab = MIN_ENCODEABLE_AB4;
    }
    if ab > MAX_ENCODEABLE_AB4 {
        ab = MAX_ENCODEABLE_AB4;
    }
    ab
}

fn l2fix4(l: f64) -> u16 {
    quick_saturate_word(l * 655.35)
}

fn ab2fix4(ab: f64) -> u16 {
    quick_saturate_word((ab + 128.0) * 257.0)
}

/// `cmsFloat2LabEncoded` (V4 encoding)
pub fn float2lab_encoded(f_lab: &CieLab) -> [u16; 3] {
    let lab = CieLab {
        L: clamp_l_double_v4(f_lab.L),
        a: clamp_ab_double_v4(f_lab.a),
        b: clamp_ab_double_v4(f_lab.b),
    };

    [l2fix4(lab.L), ab2fix4(lab.a), ab2fix4(lab.b)]
}

/// `RADIANS`
#[inline]
fn radians(deg: f64) -> f64 {
    (deg * PI) / 180.
}

/// `atan2deg` — atan2 in degrees, returning 0 if a==b==0
fn atan2deg(a: f64, b: f64) -> f64 {
    let mut h = if a == 0.0 && b == 0.0 {
        0.0
    } else {
        a.atan2(b)
    };

    h *= 180. / PI;

    while h > 360. {
        h -= 360.;
    }
    while h < 0. {
        h += 360.;
    }
    h
}

/// `Sqr`
#[inline]
fn sqr(v: f64) -> f64 {
    v * v
}

/// `cmsLab2LCh` — from cylindrical coordinates; negative values allowed
pub fn lab2lch(lab: &CieLab) -> CieLCh {
    CieLCh {
        L: lab.L,
        C: (sqr(lab.a) + sqr(lab.b)).cpow(0.5),
        h: atan2deg(lab.b, lab.a),
    }
}

/// `cmsLCh2Lab` — to cylindrical coordinates; negative values allowed.
///
/// GCC -O2 merges `cos(h)`/`sin(h)` into a single libm `sincos`, which can
/// differ in the last ulp from two separate calls. `f64::sin_cos()` lowers to
/// the same `sincos` libcall on glibc, keeping bit parity.
pub fn lch2lab(lch: &CieLCh) -> CieLab {
    let h = (lch.h * PI) / 180.0;
    let (sh, ch) = h.sin_cos();

    CieLab {
        L: lch.L,
        a: lch.C * ch,
        b: lch.C * sh,
    }
}

/// `XYZ2Fix` — all 3 components encoded using 1.15 fixed point
fn xyz2fix(d: f64) -> u16 {
    quick_saturate_word(d * 32768.0)
}

/// `cmsFloat2XYZEncoded`
pub fn float2xyz_encoded(f_xyz: &CieXyz) -> [u16; 3] {
    let mut xyz = *f_xyz;

    // Clamp to encodeable values.
    if xyz.Y <= 0.0 {
        xyz.X = 0.0;
        xyz.Y = 0.0;
        xyz.Z = 0.0;
    }
    if xyz.X > MAX_ENCODEABLE_XYZ {
        xyz.X = MAX_ENCODEABLE_XYZ;
    }
    if xyz.X < 0.0 {
        xyz.X = 0.0;
    }
    if xyz.Y > MAX_ENCODEABLE_XYZ {
        xyz.Y = MAX_ENCODEABLE_XYZ;
    }
    if xyz.Y < 0.0 {
        xyz.Y = 0.0;
    }
    if xyz.Z > MAX_ENCODEABLE_XYZ {
        xyz.Z = MAX_ENCODEABLE_XYZ;
    }
    if xyz.Z < 0.0 {
        xyz.Z = 0.0;
    }

    [xyz2fix(xyz.X), xyz2fix(xyz.Y), xyz2fix(xyz.Z)]
}

/// `XYZ2float` — from fixed 1.15 point to double
fn xyz2float(v: u16) -> f64 {
    // From 1.15 to 15.16
    let fix32 = (v as i32) << 1;
    // From fixed 15.16 to double
    s15_fixed16_to_double(fix32)
}

/// `cmsXYZEncoded2Float`
pub fn xyz_encoded2float(xyz: [u16; 3]) -> CieXyz {
    CieXyz {
        X: xyz2float(xyz[0]),
        Y: xyz2float(xyz[1]),
        Z: xyz2float(xyz[2]),
    }
}

/// `cmsDeltaE`
pub fn delta_e(lab1: &CieLab, lab2: &CieLab) -> f64 {
    let dl = (lab1.L - lab2.L).abs();
    let da = (lab1.a - lab2.a).abs();
    let db = (lab1.b - lab2.b).abs();

    (sqr(dl) + sqr(da) + sqr(db)).cpow(0.5)
}

/// `cmsCIE94DeltaE`
pub fn cie94_delta_e(lab1: &CieLab, lab2: &CieLab) -> f64 {
    let dl = (lab1.L - lab2.L).abs();

    let lch1 = lab2lch(lab1);
    let lch2 = lab2lch(lab2);

    let dc = (lch1.C - lch2.C).abs();
    let de = delta_e(lab1, lab2);

    let dhsq = sqr(de) - sqr(dl) - sqr(dc);
    let dh = if dhsq < 0.0 { 0.0 } else { dhsq.cpow(0.5) };

    let c12 = (lch1.C * lch2.C).sqrt();

    let sc = 1.0 + (0.048 * c12);
    let sh = 1.0 + (0.014 * c12);

    (sqr(dl) + sqr(dc) / sqr(sc) + sqr(dh) / sqr(sh)).sqrt()
}

/// `ComputeLBFD` (static in C)
fn compute_lbfd(lab: &CieLab) -> f64 {
    let yt = if lab.L > 7.996969 {
        (sqr((lab.L + 16.0) / 116.0) * ((lab.L + 16.0) / 116.0)) * 100.0
    } else {
        100.0 * (lab.L / 903.3)
    };

    54.6 * (std::f64::consts::LOG10_E * (yt + 1.5).cln()) - 9.6
}

/// `cmsBFDdeltaE` — BFD(1:1) difference
pub fn bfd_delta_e(lab1: &CieLab, lab2: &CieLab) -> f64 {
    let lbfd1 = compute_lbfd(lab1);
    let lbfd2 = compute_lbfd(lab2);
    let delta_l = lbfd2 - lbfd1;

    let lch1 = lab2lch(lab1);
    let lch2 = lab2lch(lab2);

    let delta_c = lch2.C - lch1.C;
    let ave_c = (lch1.C + lch2.C) / 2.0;
    let ave_h = (lch1.h + lch2.h) / 2.0;

    let de = delta_e(lab1, lab2);

    let deltah = if sqr(de) > (sqr(lab2.L - lab1.L) + sqr(delta_c)) {
        (sqr(de) - sqr(lab2.L - lab1.L) - sqr(delta_c)).sqrt()
    } else {
        0.0
    };

    let dc = 0.035 * ave_c / (1.0 + 0.00365 * ave_c) + 0.521;
    let g = (sqr(sqr(ave_c)) / (sqr(sqr(ave_c)) + 14000.0)).sqrt();
    let t = 0.627
        + (0.055 * ((ave_h - 254.0) / (180.0 / PI)).cos()
            - 0.040 * ((2.0 * ave_h - 136.0) / (180.0 / PI)).cos()
            + 0.070 * ((3.0 * ave_h - 31.0) / (180.0 / PI)).cos()
            + 0.049 * ((4.0 * ave_h + 114.0) / (180.0 / PI)).cos()
            - 0.015 * ((5.0 * ave_h - 103.0) / (180.0 / PI)).cos());

    let dh = dc * (g * t + 1.0 - g);
    let rh = -0.260 * ((ave_h - 308.0) / (180.0 / PI)).cos()
        - 0.379 * ((2.0 * ave_h - 160.0) / (180.0 / PI)).cos()
        - 0.636 * ((3.0 * ave_h + 254.0) / (180.0 / PI)).cos()
        + 0.226 * ((4.0 * ave_h + 140.0) / (180.0 / PI)).cos()
        - 0.194 * ((5.0 * ave_h + 280.0) / (180.0 / PI)).cos();

    let rc = ((ave_c * ave_c * ave_c * ave_c * ave_c * ave_c)
        / ((ave_c * ave_c * ave_c * ave_c * ave_c * ave_c) + 70000000.0))
        .sqrt();
    let rt = rh * rc;

    (sqr(delta_l) + sqr(delta_c / dc) + sqr(deltah / dh) + (rt * (delta_c / dc) * (deltah / dh)))
        .sqrt()
}

/// `cmsCMCdeltaE` — CMC(l:c) difference
pub fn cmc_delta_e(lab1: &CieLab, lab2: &CieLab, l: f64, c: f64) -> f64 {
    if lab1.L == 0.0 && lab2.L == 0.0 {
        return 0.0;
    }

    let lch1 = lab2lch(lab1);
    let lch2 = lab2lch(lab2);

    let dl = lab2.L - lab1.L;
    let dc = lch2.C - lch1.C;

    let de = delta_e(lab1, lab2);

    let dh = if sqr(de) > (sqr(dl) + sqr(dc)) {
        (sqr(de) - sqr(dl) - sqr(dc)).sqrt()
    } else {
        0.0
    };

    let t = if lch1.h > 164.0 && lch1.h < 345.0 {
        0.56 + (0.2 * ((lch1.h + 168.0) / (180.0 / PI)).cos()).abs()
    } else {
        0.36 + (0.4 * ((lch1.h + 35.0) / (180.0 / PI)).cos()).abs()
    };

    let sc = 0.0638 * lch1.C / (1.0 + 0.0131 * lch1.C) + 0.638;
    let mut sl = 0.040975 * lab1.L / (1.0 + 0.01765 * lab1.L);

    if lab1.L < 16.0 {
        sl = 0.511;
    }

    let f = ((lch1.C * lch1.C * lch1.C * lch1.C) / ((lch1.C * lch1.C * lch1.C * lch1.C) + 1900.0))
        .sqrt();
    let sh = sc * (t * f + 1.0 - f);

    (sqr(dl / (l * sl)) + sqr(dc / (c * sc)) + sqr(dh / sh)).sqrt()
}

/// `cmsCIE2000DeltaE`
pub fn cie2000_delta_e(lab1: &CieLab, lab2: &CieLab, kl: f64, kc: f64, kh: f64) -> f64 {
    let l1 = lab1.L;
    let a1 = lab1.a;
    let b1 = lab1.b;
    let c = (sqr(a1) + sqr(b1)).sqrt();

    let ls = lab2.L;
    let a_s = lab2.a;
    let b_s = lab2.b;
    let c_s = (sqr(a_s) + sqr(b_s)).sqrt();

    let g = 0.5
        * (1.0
            - (((c + c_s) / 2.0).cpow(7.0) / (((c + c_s) / 2.0).cpow(7.0) + 25.0_f64.cpow(7.0)))
                .sqrt());

    let a_p = (1.0 + g) * a1;
    let b_p = b1;
    let c_p = (sqr(a_p) + sqr(b_p)).sqrt();
    let h_p = atan2deg(b_p, a_p);

    let a_ps = (1.0 + g) * a_s;
    let b_ps = b_s;
    let c_ps = (sqr(a_ps) + sqr(b_ps)).sqrt();
    let h_ps = atan2deg(b_ps, a_ps);

    let mean_c_p = (c_p + c_ps) / 2.0;

    let hps_plus_hp = h_ps + h_p;
    let hps_minus_hp = h_ps - h_p;

    let meanh_p = if hps_minus_hp.abs() <= 180.000001 {
        hps_plus_hp / 2.0
    } else if hps_plus_hp < 360.0 {
        (hps_plus_hp + 360.0) / 2.0
    } else {
        (hps_plus_hp - 360.0) / 2.0
    };

    let delta_h = if hps_minus_hp <= -180.000001 {
        hps_minus_hp + 360.0
    } else if hps_minus_hp > 180.0 {
        hps_minus_hp - 360.0
    } else {
        hps_minus_hp
    };
    let delta_l = ls - l1;
    let delta_c = c_ps - c_p;

    let delta_hh = 2.0 * (c_ps * c_p).sqrt() * (radians(delta_h) / 2.0).sin();

    let t = 1.0 - 0.17 * radians(meanh_p - 30.0).cos()
        + 0.24 * radians(2.0 * meanh_p).cos()
        + 0.32 * radians(3.0 * meanh_p + 6.0).cos()
        - 0.2 * radians(4.0 * meanh_p - 63.0).cos();

    let sl =
        1.0 + (0.015 * sqr((ls + l1) / 2.0 - 50.0)) / (20.0 + sqr((ls + l1) / 2.0 - 50.0)).sqrt();

    let sc = 1.0 + 0.045 * (c_p + c_ps) / 2.0;
    let sh = 1.0 + 0.015 * ((c_ps + c_p) / 2.0) * t;

    let delta_ro = 30.0 * (-sqr((meanh_p - 275.0) / 25.0)).exp();

    let rc = 2.0 * (mean_c_p.cpow(7.0) / (mean_c_p.cpow(7.0) + 25.0_f64.cpow(7.0))).sqrt();

    let rt = -(2.0 * radians(delta_ro)).sin() * rc;

    (sqr(delta_l / (sl * kl))
        + sqr(delta_c / (sc * kc))
        + sqr(delta_hh / (sh * kh))
        + rt * (delta_c / (sc * kc)) * (delta_hh / (sh * kh)))
        .sqrt()
}

/// `_cmsReasonableGridpointsByColorspace`
pub fn reasonable_gridpoints_by_colorspace(colorspace: ColorSpaceSignature, dw_flags: u32) -> u32 {
    // Already specified?
    if dw_flags & FLAGS_GRIDPOINTS_MASK != 0 {
        return (dw_flags >> 16) & 0xFF;
    }

    let n_channels = channels_of(colorspace);

    // HighResPrecalc is maximum resolution
    if dw_flags & FLAGS_HIGHRESPRECALC != 0 {
        if n_channels > 4 {
            return 7; // 7 for Hifi
        }
        if n_channels == 4 {
            return 23; // 23 for CMYK
        }
        return 49; // 49 for RGB and others
    }

    // LowResPrecal is lower resolution
    if dw_flags & FLAGS_LOWRESPRECALC != 0 {
        if n_channels > 4 {
            return 6; // 6 for more than 4 channels
        }
        if n_channels == 1 {
            return 33; // For monochrome
        }
        return 17; // 17 for remaining
    }

    // Default values
    if n_channels > 4 {
        return 7; // 7 for Hifi
    }
    if n_channels == 4 {
        return 17; // 17 for CMYK
    }
    33 // 33 for RGB
}

/// `_cmsEndPointsBySpace` — returns (white, black, n_outputs) for common spaces
pub fn end_points_by_space(
    space: ColorSpaceSignature,
) -> Option<(&'static [u16], &'static [u16], u32)> {
    const RGB_BLACK: [u16; 4] = [0, 0, 0, 0];
    const RGB_WHITE: [u16; 4] = [0xffff, 0xffff, 0xffff, 0];
    const CMYK_BLACK: [u16; 4] = [0xffff, 0xffff, 0xffff, 0xffff];
    const CMYK_WHITE: [u16; 4] = [0, 0, 0, 0];
    const LAB_BLACK: [u16; 4] = [0, 0x8080, 0x8080, 0];
    const LAB_WHITE: [u16; 4] = [0xffff, 0x8080, 0x8080, 0];
    const CMY_BLACK: [u16; 4] = [0xffff, 0xffff, 0xffff, 0];
    const CMY_WHITE: [u16; 4] = [0, 0, 0, 0];
    const GRAY_BLACK: [u16; 4] = [0, 0, 0, 0];
    const GRAY_WHITE: [u16; 4] = [0xffff, 0, 0, 0];

    match space {
        cs::SigGrayData => Some((&GRAY_WHITE, &GRAY_BLACK, 1)),
        cs::SigRgbData => Some((&RGB_WHITE, &RGB_BLACK, 3)),
        cs::SigLabData => Some((&LAB_WHITE, &LAB_BLACK, 3)),
        cs::SigCmykData => Some((&CMYK_WHITE, &CMYK_BLACK, 4)),
        cs::SigCmyData => Some((&CMY_WHITE, &CMY_BLACK, 3)),
        _ => None,
    }
}

/// `_cmsICCcolorSpace` — translate from our colorspace to ICC representation
pub fn icc_color_space(our_notation: i32) -> ColorSpaceSignature {
    match our_notation as u32 {
        1 => cs::SigGrayData,
        x if x == PT_GRAY => cs::SigGrayData,
        2 => cs::SigRgbData,
        x if x == PT_RGB => cs::SigRgbData,
        x if x == PT_CMY => cs::SigCmyData,
        x if x == PT_CMYK => cs::SigCmykData,
        x if x == PT_YCBCR => cs::SigYCbCrData,
        x if x == PT_YUV => cs::SigLuvData,
        x if x == PT_XYZ => cs::SigXYZData,
        x if x == PT_LABV2 => cs::SigLabData,
        x if x == PT_LAB => cs::SigLabData,
        x if x == PT_YUVK => cs::SigLuvKData,
        x if x == PT_HSV => cs::SigHsvData,
        x if x == PT_HLS => cs::SigHlsData,
        x if x == PT_YXY => cs::SigYxyData,
        x if x == PT_MCH1 => cs::SigMCH1Data,
        x if x == PT_MCH2 => cs::SigMCH2Data,
        x if x == PT_MCH3 => cs::SigMCH3Data,
        x if x == PT_MCH4 => cs::SigMCH4Data,
        x if x == PT_MCH5 => cs::SigMCH5Data,
        x if x == PT_MCH6 => cs::SigMCH6Data,
        x if x == PT_MCH7 => cs::SigMCH7Data,
        x if x == PT_MCH8 => cs::SigMCH8Data,
        x if x == PT_MCH9 => cs::SigMCH9Data,
        x if x == PT_MCH10 => cs::SigMCHAData,
        x if x == PT_MCH11 => cs::SigMCHBData,
        x if x == PT_MCH12 => cs::SigMCHCData,
        x if x == PT_MCH13 => cs::SigMCHDData,
        x if x == PT_MCH14 => cs::SigMCHEData,
        x if x == PT_MCH15 => cs::SigMCHFData,
        _ => 0,
    }
}

/// `_cmsLCMScolorSpace` — profile space to our colorspace notation
pub fn lcms_color_space(profile_space: ColorSpaceSignature) -> i32 {
    (match profile_space {
        cs::SigGrayData => PT_GRAY,
        cs::SigRgbData => PT_RGB,
        cs::SigCmyData => PT_CMY,
        cs::SigCmykData => PT_CMYK,
        cs::SigYCbCrData => PT_YCBCR,
        cs::SigLuvData => PT_YUV,
        cs::SigXYZData => PT_XYZ,
        cs::SigLabData => PT_LAB,
        cs::SigLuvKData => PT_YUVK,
        cs::SigHsvData => PT_HSV,
        cs::SigHlsData => PT_HLS,
        cs::SigYxyData => PT_YXY,
        cs::Sig1colorData | cs::SigMCH1Data => PT_MCH1,
        cs::Sig2colorData | cs::SigMCH2Data => PT_MCH2,
        cs::Sig3colorData | cs::SigMCH3Data => PT_MCH3,
        cs::Sig4colorData | cs::SigMCH4Data => PT_MCH4,
        cs::Sig5colorData | cs::SigMCH5Data => PT_MCH5,
        cs::Sig6colorData | cs::SigMCH6Data => PT_MCH6,
        cs::SigMCH7Data | cs::Sig7colorData => PT_MCH7,
        cs::SigMCH8Data | cs::Sig8colorData => PT_MCH8,
        cs::SigMCH9Data | cs::Sig9colorData => PT_MCH9,
        cs::SigMCHAData | cs::Sig10colorData => PT_MCH10,
        cs::SigMCHBData | cs::Sig11colorData => PT_MCH11,
        cs::SigMCHCData | cs::Sig12colorData => PT_MCH12,
        cs::SigMCHDData | cs::Sig13colorData => PT_MCH13,
        cs::SigMCHEData | cs::Sig14colorData => PT_MCH14,
        cs::SigMCHFData | cs::Sig15colorData => PT_MCH15,
        _ => return 0,
    }) as i32
}

/// `cmsChannelsOfColorSpace` — -1 on unknown
pub fn channels_of_color_space(color_space: ColorSpaceSignature) -> i32 {
    match color_space {
        cs::SigMCH1Data | cs::Sig1colorData | cs::SigGrayData => 1,
        cs::SigMCH2Data | cs::Sig2colorData => 2,
        cs::SigXYZData
        | cs::SigLabData
        | cs::SigLuvData
        | cs::SigYCbCrData
        | cs::SigYxyData
        | cs::SigRgbData
        | cs::SigHsvData
        | cs::SigHlsData
        | cs::SigCmyData
        | cs::SigMCH3Data
        | cs::Sig3colorData => 3,
        cs::SigLuvKData | cs::SigCmykData | cs::SigMCH4Data | cs::Sig4colorData => 4,
        cs::SigMCH5Data | cs::Sig5colorData => 5,
        cs::SigMCH6Data | cs::Sig6colorData => 6,
        cs::SigMCH7Data | cs::Sig7colorData => 7,
        cs::SigMCH8Data | cs::Sig8colorData => 8,
        cs::SigMCH9Data | cs::Sig9colorData => 9,
        cs::SigMCHAData | cs::Sig10colorData => 10,
        cs::SigMCHBData | cs::Sig11colorData => 11,
        cs::SigMCHCData | cs::Sig12colorData => 12,
        cs::SigMCHDData | cs::Sig13colorData => 13,
        cs::SigMCHEData | cs::Sig14colorData => 14,
        cs::SigMCHFData | cs::Sig15colorData => 15,
        _ => -1,
    }
}

/// `cmsChannelsOf` — DEPRECATED compat shim: unknown → 3
pub fn channels_of(color_space: ColorSpaceSignature) -> u32 {
    let n = channels_of_color_space(color_space);
    if n < 0 { 3 } else { n as u32 }
}
