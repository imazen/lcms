//! Port of `cmscam02.c` — CIECAM02 appearance model.
//!
//! C source: `reference/lcms2/src/cmscam02.c`
//! Gate: `tests/c_parity_cam02.rs`
//!
//! `cmsCIECAM02Init`/`Done`/`Forward`/`Reverse` → `CieCam02::new`,
//! `forward`, `reverse` (the object is a `Box`-owned value; "Done" is `Drop`).

use crate::types::{
    CUTSHEET_SURROUND, CieXyz, CpowExt, D_CALCULATE, DARK_SURROUND, DIM_SURROUND, JCh,
    ViewingConditions,
};

/// `CAM02COLOR` — the working color record.
#[derive(Clone, Copy, Debug, Default)]
struct Cam02Color {
    xyz: [f64; 3],
    rgb: [f64; 3],
    rgbc: [f64; 3],
    rgbp: [f64; 3],
    rgbpa: [f64; 3],
    a: f64,
    b: f64,
    h: f64,
    #[allow(non_snake_case)]
    h_upper: f64, // H
    #[allow(non_snake_case)]
    a_upper: f64, // A
    #[allow(non_snake_case)]
    j_upper: f64, // J
    #[allow(non_snake_case)]
    q_upper: f64, // Q
    s: f64,
    #[allow(dead_code)]
    t: f64,
    #[allow(non_snake_case)]
    c_upper: f64, // C
    #[allow(non_snake_case)]
    m_upper: f64, // M
}

/// `cmsCIECAM02` — the model state.
pub struct CieCam02 {
    adopted_white: Cam02Color,
    la: f64,
    yb: f64,
    f: f64,
    c: f64,
    nc: f64,
    surround: u32,
    n: f64,
    nbb: f64,
    ncb: f64,
    z: f64,
    fl: f64,
    d: f64,
}

fn xyz_to_cat02(mut clr: Cam02Color) -> Cam02Color {
    clr.rgb[0] = (clr.xyz[0] * 0.7328) + (clr.xyz[1] * 0.4296) + (clr.xyz[2] * -0.1624);
    clr.rgb[1] = (clr.xyz[0] * -0.7036) + (clr.xyz[1] * 1.6975) + (clr.xyz[2] * 0.0061);
    clr.rgb[2] = (clr.xyz[0] * 0.0030) + (clr.xyz[1] * 0.0136) + (clr.xyz[2] * 0.9834);
    clr
}

fn chromatic_adaptation(mut clr: Cam02Color, m: &CieCam02) -> Cam02Color {
    for i in 0..3 {
        clr.rgbc[i] =
            ((m.adopted_white.xyz[1] * (m.d / m.adopted_white.rgb[i])) + (1.0 - m.d)) * clr.rgb[i];
    }
    clr
}

fn cat02_to_hpe(mut clr: Cam02Color) -> Cam02Color {
    let m = [
        (0.38971 * 1.096124) + (0.68898 * 0.454369) + (-0.07868 * -0.009628),
        (0.38971 * -0.278869) + (0.68898 * 0.473533) + (-0.07868 * -0.005698),
        (0.38971 * 0.182745) + (0.68898 * 0.072098) + (-0.07868 * 1.015326),
        (-0.22981 * 1.096124) + (1.18340 * 0.454369) + (0.04641 * -0.009628),
        (-0.22981 * -0.278869) + (1.18340 * 0.473533) + (0.04641 * -0.005698),
        (-0.22981 * 0.182745) + (1.18340 * 0.072098) + (0.04641 * 1.015326),
        -0.009628,
        -0.005698,
        1.015326,
    ];
    clr.rgbp[0] = (clr.rgbc[0] * m[0]) + (clr.rgbc[1] * m[1]) + (clr.rgbc[2] * m[2]);
    clr.rgbp[1] = (clr.rgbc[0] * m[3]) + (clr.rgbc[1] * m[4]) + (clr.rgbc[2] * m[5]);
    clr.rgbp[2] = (clr.rgbc[0] * m[6]) + (clr.rgbc[1] * m[7]) + (clr.rgbc[2] * m[8]);
    clr
}

fn nonlinear_compression(mut clr: Cam02Color, m: &CieCam02) -> Cam02Color {
    for i in 0..3 {
        if clr.rgbp[i] < 0.0 {
            let temp = (-1.0 * m.fl * clr.rgbp[i] / 100.0).cpow(0.42);
            clr.rgbpa[i] = (-1.0 * 400.0 * temp) / (temp + 27.13) + 0.1;
        } else {
            let temp = (m.fl * clr.rgbp[i] / 100.0).cpow(0.42);
            clr.rgbpa[i] = (400.0 * temp) / (temp + 27.13) + 0.1;
        }
    }
    clr.a_upper = ((2.0 * clr.rgbpa[0] + clr.rgbpa[1] + clr.rgbpa[2] / 20.0) - 0.305) * m.nbb;
    clr
}

fn compute_correlates(mut clr: Cam02Color, m: &CieCam02) -> Cam02Color {
    let a = clr.rgbpa[0] - (12.0 * clr.rgbpa[1] / 11.0) + (clr.rgbpa[2] / 11.0);
    let b = (clr.rgbpa[0] + clr.rgbpa[1] - (2.0 * clr.rgbpa[2])) / 9.0;

    let r2d = 180.0 / 3.141592654;
    if a == 0.0 {
        if b == 0.0 {
            clr.h = 0.0;
        } else if b > 0.0 {
            clr.h = 90.0;
        } else {
            clr.h = 270.0;
        }
    } else if a > 0.0 {
        let temp = b / a;
        if b > 0.0 {
            clr.h = r2d * temp.atan();
        } else if b == 0.0 {
            clr.h = 0.0;
        } else {
            clr.h = (r2d * temp.atan()) + 360.0;
        }
    } else {
        let temp = b / a;
        clr.h = (r2d * temp.atan()) + 180.0;
    }

    let d2r = 3.141592654 / 180.0;
    let e = ((12500.0 / 13.0) * m.nc * m.ncb) * ((clr.h * d2r + 2.0).cos() + 3.8);

    let temp;
    if clr.h < 20.14 {
        temp = ((clr.h + 122.47) / 1.2) + ((20.14 - clr.h) / 0.8);
        clr.h_upper = 300.0 + (100.0 * ((clr.h + 122.47) / 1.2)) / temp;
    } else if clr.h < 90.0 {
        temp = ((clr.h - 20.14) / 0.8) + ((90.00 - clr.h) / 0.7);
        clr.h_upper = (100.0 * ((clr.h - 20.14) / 0.8)) / temp;
    } else if clr.h < 164.25 {
        temp = ((clr.h - 90.00) / 0.7) + ((164.25 - clr.h) / 1.0);
        clr.h_upper = 100.0 + ((100.0 * ((clr.h - 90.00) / 0.7)) / temp);
    } else if clr.h < 237.53 {
        temp = ((clr.h - 164.25) / 1.0) + ((237.53 - clr.h) / 1.2);
        clr.h_upper = 200.0 + ((100.0 * ((clr.h - 164.25) / 1.0)) / temp);
    } else {
        temp = ((clr.h - 237.53) / 1.2) + ((360.0 - clr.h + 20.14) / 0.8);
        clr.h_upper = 300.0 + ((100.0 * ((clr.h - 237.53) / 1.2)) / temp);
    }

    clr.j_upper = 100.0 * (clr.a_upper / m.adopted_white.a_upper).cpow(m.c * m.z);

    clr.q_upper = (4.0 / m.c)
        * (clr.j_upper / 100.0).cpow(0.5)
        * (m.adopted_white.a_upper + 4.0)
        * m.fl.cpow(0.25);

    let t = (e * (a * a + b * b).cpow(0.5))
        / (clr.rgbpa[0] + clr.rgbpa[1] + (21.0 / 20.0) * clr.rgbpa[2]);

    clr.c_upper =
        t.cpow(0.9) * (clr.j_upper / 100.0).cpow(0.5) * (1.64 - 0.29_f64.cpow(m.n)).cpow(0.73);
    clr.m_upper = clr.c_upper * m.fl.cpow(0.25);
    clr.s = 100.0 * (clr.m_upper / clr.q_upper).cpow(0.5);
    clr.a = a;
    clr.b = b;
    clr
}

fn inverse_correlates(mut clr: Cam02Color, m: &CieCam02) -> Cam02Color {
    let d2r = 3.141592654 / 180.0;

    let t = (clr.c_upper
        / ((clr.j_upper / 100.0).cpow(0.5) * (1.64 - 0.29_f64.cpow(m.n)).cpow(0.73)))
    .cpow(1.0 / 0.9);
    let e = ((12500.0 / 13.0) * m.nc * m.ncb) * ((clr.h * d2r + 2.0).cos() + 3.8);

    clr.a_upper = m.adopted_white.a_upper * (clr.j_upper / 100.0).cpow(1.0 / (m.c * m.z));

    let p2 = (clr.a_upper / m.nbb) + 0.305;

    if t <= 0.0 {
        // special case from spec notes, avoid divide by zero
        clr.a = 0.0;
        clr.b = 0.0;
    } else {
        let hr = clr.h * d2r;
        // C merges sin(hr)/cos(hr) into one sincos libcall at -O2
        let (sh, ch) = hr.sin_cos();
        let p1 = e / t;
        let p3 = 21.0 / 20.0;

        if sh.abs() >= ch.abs() {
            let p4 = p1 / sh;
            clr.b = (p2 * (2.0 + p3) * (460.0 / 1403.0))
                / (p4 + (2.0 + p3) * (220.0 / 1403.0) * (ch / sh) - (27.0 / 1403.0)
                    + p3 * (6300.0 / 1403.0));
            clr.a = clr.b * (ch / sh);
        } else {
            let p5 = p1 / ch;
            clr.a = (p2 * (2.0 + p3) * (460.0 / 1403.0))
                / (p5 + (2.0 + p3) * (220.0 / 1403.0)
                    - ((27.0 / 1403.0) - p3 * (6300.0 / 1403.0)) * (sh / ch));
            clr.b = clr.a * (sh / ch);
        }
    }

    clr.rgbpa[0] =
        ((460.0 / 1403.0) * p2) + ((451.0 / 1403.0) * clr.a) + ((288.0 / 1403.0) * clr.b);
    clr.rgbpa[1] =
        ((460.0 / 1403.0) * p2) - ((891.0 / 1403.0) * clr.a) - ((261.0 / 1403.0) * clr.b);
    clr.rgbpa[2] =
        ((460.0 / 1403.0) * p2) - ((220.0 / 1403.0) * clr.a) - ((6300.0 / 1403.0) * clr.b);
    clr
}

fn inverse_nonlinearity(mut clr: Cam02Color, m: &CieCam02) -> Cam02Color {
    for i in 0..3 {
        let c1 = if clr.rgbpa[i] - 0.1 < 0.0 { -1.0 } else { 1.0 };
        clr.rgbp[i] = c1
            * (100.0 / m.fl)
            * ((27.13 * (clr.rgbpa[i] - 0.1).abs()) / (400.0 - (clr.rgbpa[i] - 0.1).abs()))
                .cpow(1.0 / 0.42);
    }
    clr
}

fn hpe_to_cat02(mut clr: Cam02Color) -> Cam02Color {
    let m = [
        (0.7328 * 1.910197) + (0.4296 * 0.370950),
        (0.7328 * -1.112124) + (0.4296 * 0.629054),
        (0.7328 * 0.201908) + (0.4296 * 0.000008) - 0.1624,
        (-0.7036 * 1.910197) + (1.6975 * 0.370950),
        (-0.7036 * -1.112124) + (1.6975 * 0.629054),
        (-0.7036 * 0.201908) + (1.6975 * 0.000008) + 0.0061,
        (0.0030 * 1.910197) + (0.0136 * 0.370950),
        (0.0030 * -1.112124) + (0.0136 * 0.629054),
        (0.0030 * 0.201908) + (0.0136 * 0.000008) + 0.9834,
    ];
    clr.rgbc[0] = (clr.rgbp[0] * m[0]) + (clr.rgbp[1] * m[1]) + (clr.rgbp[2] * m[2]);
    clr.rgbc[1] = (clr.rgbp[0] * m[3]) + (clr.rgbp[1] * m[4]) + (clr.rgbp[2] * m[5]);
    clr.rgbc[2] = (clr.rgbp[0] * m[6]) + (clr.rgbp[1] * m[7]) + (clr.rgbp[2] * m[8]);
    clr
}

fn inverse_chromatic_adaptation(mut clr: Cam02Color, m: &CieCam02) -> Cam02Color {
    for i in 0..3 {
        clr.rgb[i] =
            clr.rgbc[i] / ((m.adopted_white.xyz[1] * m.d / m.adopted_white.rgb[i]) + 1.0 - m.d);
    }
    clr
}

fn cat02_to_xyz(mut clr: Cam02Color) -> Cam02Color {
    clr.xyz[0] = (clr.rgb[0] * 1.096124) + (clr.rgb[1] * -0.278869) + (clr.rgb[2] * 0.182745);
    clr.xyz[1] = (clr.rgb[0] * 0.454369) + (clr.rgb[1] * 0.473533) + (clr.rgb[2] * 0.072098);
    clr.xyz[2] = (clr.rgb[0] * -0.009628) + (clr.rgb[1] * -0.005698) + (clr.rgb[2] * 1.015326);
    clr
}

impl CieCam02 {
    /// `cmsCIECAM02Init`
    pub fn new(_ctx: Option<&crate::context::Context>, vc: &ViewingConditions) -> Box<CieCam02> {
        let mut m = Box::new(CieCam02 {
            adopted_white: Cam02Color::default(),
            la: vc.La,
            yb: vc.Yb,
            f: 0.0,
            c: 0.0,
            nc: 0.0,
            surround: vc.surround,
            n: 0.0,
            nbb: 0.0,
            ncb: 0.0,
            z: 0.0,
            fl: 0.0,
            d: vc.D_value,
        });

        m.adopted_white.xyz[0] = vc.white_point.X;
        m.adopted_white.xyz[1] = vc.white_point.Y;
        m.adopted_white.xyz[2] = vc.white_point.Z;

        match m.surround {
            CUTSHEET_SURROUND => {
                m.f = 0.8;
                m.c = 0.41;
                m.nc = 0.8;
            }
            DARK_SURROUND => {
                m.f = 0.8;
                m.c = 0.525;
                m.nc = 0.8;
            }
            DIM_SURROUND => {
                m.f = 0.9;
                m.c = 0.59;
                m.nc = 0.95;
            }
            _ => {
                // Average surround
                m.f = 1.0;
                m.c = 0.69;
                m.nc = 1.0;
            }
        }

        // compute_n
        m.n = m.yb / m.adopted_white.xyz[1];
        // compute_z
        m.z = 1.48 + m.n.cpow(0.5);
        // computeNbb
        m.nbb = 0.725 * (1.0 / m.n).cpow(0.2);
        // computeFL
        let k = 1.0 / (5.0 * m.la + 1.0);
        m.fl = 0.2 * k.cpow(4.0) * (5.0 * m.la)
            + 0.1 * (1.0 - k.cpow(4.0)).cpow(2.0) * (5.0 * m.la).cpow(1.0 / 3.0);

        // computeD
        if m.d == D_CALCULATE {
            let temp = 1.0 - (1.0 / 3.6) * ((-m.la - 42.0) / 92.0).exp();
            m.d = m.f * temp;
        }

        m.ncb = m.nbb;

        m.adopted_white = xyz_to_cat02(m.adopted_white);
        m.adopted_white = chromatic_adaptation(m.adopted_white, &m);
        m.adopted_white = cat02_to_hpe(m.adopted_white);
        m.adopted_white = nonlinear_compression(m.adopted_white, &m);

        m
    }

    /// `cmsCIECAM02Forward`
    pub fn forward(&self, p_in: &CieXyz, p_out: &mut JCh) {
        let mut clr = Cam02Color::default();
        clr.xyz[0] = p_in.X;
        clr.xyz[1] = p_in.Y;
        clr.xyz[2] = p_in.Z;

        clr = xyz_to_cat02(clr);
        clr = chromatic_adaptation(clr, self);
        clr = cat02_to_hpe(clr);
        clr = nonlinear_compression(clr, self);
        clr = compute_correlates(clr, self);

        p_out.J = clr.j_upper;
        p_out.C = clr.c_upper;
        p_out.h = clr.h;
    }

    /// `cmsCIECAM02Reverse`
    pub fn reverse(&self, p_in: &JCh, p_out: &mut CieXyz) {
        let mut clr = Cam02Color::default();
        clr.j_upper = p_in.J;
        clr.c_upper = p_in.C;
        clr.h = p_in.h;

        clr = inverse_correlates(clr, self);
        clr = inverse_nonlinearity(clr, self);
        clr = hpe_to_cat02(clr);
        clr = inverse_chromatic_adaptation(clr, self);
        clr = cat02_to_xyz(clr);

        p_out.X = clr.xyz[0];
        p_out.Y = clr.xyz[1];
        p_out.Z = clr.xyz[2];
    }
}
