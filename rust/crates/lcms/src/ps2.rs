//! `cmsps2.c` — PostScript color-resource generation: emits CSA /
//! CRD / ColorResource PostScript text from a profile's machinery.
//! The emitters write formatted PostScript to an `IoHandler`.

use crate::context::Context;
use crate::gamma::ToneCurve;
use crate::io::IoHandler;
use crate::profile::IccProfile;
use crate::types::CieXyz;

const MAXPSCOLS: usize = 60;

/// `Word2Byte` — 16→8 rounding
fn word2byte(w: u16) -> u8 {
    ((w as f64) / 257.0 + 0.5).floor() as u8
}

/// column-tracking writer — `_cmsPSActualColumn`
struct PsWriter<'a> {
    io: &'a mut IoHandler,
    col: usize,
}

impl<'a> PsWriter<'a> {
    fn new(io: &'a mut IoHandler) -> Self {
        PsWriter { io, col: 0 }
    }
    /// `_cmsIOPrintf`-style output. C's printf does NOT track the hex
    /// column (`_cmsPSActualColumn` is only bumped by WriteByte/reset by
    /// the row sampler). It does replace every ',' with '.' (locale guard).
    fn out(&mut self, s: &str) {
        let s2 = s.replace(',', ".");
        self.io.write(s2.len() as u32, s2.as_bytes());
    }
    /// `WriteByte` — hex-emit one byte with column wrap
    fn write_byte(&mut self, b: u8) {
        let s = format!("{b:02x}");
        self.col += 2;
        self.io.write(s.len() as u32, s.as_bytes());
        if self.col > MAXPSCOLS {
            self.io.write(1, b"\n");
            self.col = 0;
        }
    }
    /// `EmitPSEscaped` — PostScript string escaping
    fn emit_ps_escaped(&mut self, txt: &str) {
        for c in txt.bytes() {
            if c == b'\\' || c == b'(' || c == b')' {
                self.out(&format!("\\{}", c as char));
            } else if c < 0x20 || c >= 0x7f {
                self.out(&format!("\\{:03o}", c));
            } else {
                self.out(&(c as char).to_string());
            }
        }
    }
}

/// `RemoveCR` — replace \n/\r with spaces
fn remove_cr(txt: &str) -> String {
    let mut b = String::new();
    for (i, c) in txt.bytes().take(2047).enumerate() {
        if i == 2047 {
            break;
        }
        b.push(if c == b'\n' || c == b'\r' {
            ' '
        } else {
            c as char
        });
    }
    b
}

/// `EmitHeader` — the PS header block + description/copyright
fn emit_header(w: &mut PsWriter, title: &str, icc: &mut IccProfile, created: &str) {
    use crate::tagtypes::TagObject;
    let mut desc = String::new();
    let mut copyr = String::new();
    if let Some(TagObject::Mlu(m))
    | Some(TagObject::TextDescription(m))
    | Some(TagObject::Text(m)) =
        crate::profile::cms_read_tag(icc, crate::signatures::tag::SigProfileDescriptionTag)
    {
        let (_n, buf) = crate::named::mlu_get_ascii(m, &[0; 2], &[0; 2], Some(256));
        desc = String::from_utf8_lossy(&buf).trim_matches('\0').to_string();
    }
    if let Some(TagObject::Mlu(m))
    | Some(TagObject::TextDescription(m))
    | Some(TagObject::Text(m)) =
        crate::profile::cms_read_tag(icc, crate::signatures::tag::SigCopyrightTag)
    {
        let (_n, buf) = crate::named::mlu_get_ascii(m, &[0; 2], &[0; 2], Some(256));
        copyr = String::from_utf8_lossy(&buf).trim_matches('\0').to_string();
    }
    w.out("%!PS-Adobe-3.0\n");
    w.out("%\n");
    w.out(&format!("% {title}\n"));
    w.out(&format!("% Source: {}\n", remove_cr(&desc)));
    w.out(&format!("%         {}\n", remove_cr(&copyr)));
    w.out(&format!("% Created: {created}")); // ctime appends \n
    w.out("%\n");
    w.out("%%BeginResource\n");
}

/// `EmitWhiteBlackD50`
fn emit_white_black_d50(w: &mut PsWriter, black_point: &CieXyz) {
    w.out(&format!(
        "/BlackPoint [{:.6} {:.6} {:.6}]\n",
        black_point.X, black_point.Y, black_point.Z
    ));
    w.out(&format!(
        "/WhitePoint [{:.6} {:.6} {:.6}]\n",
        crate::types::D50X,
        crate::types::D50Y,
        crate::types::D50Z
    ));
}

/// `EmitRangeCheck`
fn emit_range_check(w: &mut PsWriter) {
    w.out("dup 0.0 lt { pop 0.0 } if dup 1.0 gt { pop 1.0 } if ");
}

/// `EmitIntent`
fn emit_intent(w: &mut PsWriter, rendering_intent: u32) {
    use crate::types::*;
    let intent = match rendering_intent {
        INTENT_PERCEPTUAL => "Perceptual",
        INTENT_RELATIVE_COLORIMETRIC => "RelativeColorimetric",
        INTENT_ABSOLUTE_COLORIMETRIC => "AbsoluteColorimetric",
        INTENT_SATURATION => "Saturation",
        _ => "Undefined",
    };
    w.out(&format!("/RenderingIntent ({intent})\n"));
}

/// `EmitLab2XYZ` — the Lab→XYZ decode dictionary
fn emit_lab2xyz(w: &mut PsWriter) {
    w.out("/RangeABC [ 0 1 0 1 0 1]\n");
    w.out("/DecodeABC [\n");
    w.out("{100 mul  16 add 116 div } bind\n");
    w.out("{255 mul 128 sub 500 div } bind\n");
    w.out("{255 mul 128 sub 200 div } bind\n");
    w.out("]\n");
    w.out("/MatrixABC [ 1 1 1 1 0 0 0 0 -1]\n");
    w.out("/RangeLMN [ -0.236 1.254 0 1 -0.635 1.640 ]\n");
    w.out("/DecodeLMN [\n");
    w.out("{dup 6 29 div ge {dup dup mul mul} {4 29 div sub 108 841 div mul} ifelse 0.964200 mul} bind\n");
    w.out("{dup 6 29 div ge {dup dup mul mul} {4 29 div sub 108 841 div mul} ifelse } bind\n");
    w.out("{dup 6 29 div ge {dup dup mul mul} {4 29 div sub 108 841 div mul} ifelse 0.824900 mul} bind\n");
    w.out("]\n");
}

/// `Emit1Gamma` — a single tone curve as a PS function
fn emit_1_gamma(w: &mut PsWriter, table: &ToneCurve) {
    // C: empty/linear → `{ 1 } bind `; a positive gamma estimate →
    // `{ %g exp } bind `; else the tabulated-table interpolating fn.
    if table.table16.is_empty() || crate::gamma::is_tone_curve_linear(table) {
        w.out("{ 1 } bind ");
        return;
    }
    let gamma = crate::gamma::estimate_gamma(table, 0.001);
    if gamma > 0.0 {
        w.out(&format!("{{ {} exp }} bind ", format_g(gamma)));
        return;
    }
    w.out("{ ");
    emit_range_check(w);
    w.out(" [");
    for (i, &v) in table.table16.iter().enumerate() {
        if i % 10 == 0 {
            w.out("\n  ");
        }
        w.out(&format!("{v} "));
    }
    w.out("] ");
    w.out("dup ");
    w.out("length 1 sub ");
    w.out("3 -1 roll ");
    w.out("mul ");
    w.out("dup ");
    w.out("dup ");
    w.out("floor cvi ");
    w.out("exch ");
    w.out("ceiling cvi ");
    w.out("3 index ");
    w.out("exch ");
    w.out("get\n  ");
    w.out("4 -1 roll ");
    w.out("3 -1 roll ");
    w.out("get ");
    w.out("dup ");
    w.out("3 1 roll ");
    w.out("sub ");
    w.out("3 -1 roll ");
    w.out("dup ");
    w.out("floor cvi ");
    w.out("sub ");
    w.out("mul ");
    w.out("add ");
    w.out("65535 div\n");
    w.out(" } bind ");
}

/// `EmitNGamma` — N tone curves, collapsing runs of equal tables
fn emit_n_gamma(w: &mut PsWriter, curves: &[&ToneCurve]) {
    for (i, c) in curves.iter().enumerate() {
        if i > 0 && curves[i - 1].table16 == c.table16 {
            w.out("dup ");
        } else {
            emit_1_gamma(w, c);
        }
    }
}

/// `cmsPsSamplerCargo` — WriteCLUT's streaming state
struct PsSamplerCargo<'a> {
    first_component: i32,
    second_component: i32,
    n_outputs: u32,
    pre_maj: &'a str,
    post_maj: &'a str,
    pre_min: &'a str,
    post_min: &'a str,
    fix_white: bool,
    color_space: u32,
}

/// `OutputValueSampler` — emit each CLUT node, grouping with PreMaj/PostMin
fn output_value_sampler(
    w: &mut PsWriter,
    inp: &[u16],
    out: &mut [u16],
    sc: &mut PsSamplerCargo,
) -> i32 {
    if sc.fix_white
        && inp[0] == 0xffff
        && inp[1] >= 0x7800
        && inp[1] <= 0x8800
        && inp[2] >= 0x7800
        && inp[2] <= 0x8800
    {
        let Some((white, _black, n_outs)) = crate::pcs::end_points_by_space(sc.color_space) else {
            return 0;
        };
        for (i, &wp) in white.iter().enumerate().take(n_outs as usize) {
            out[i] = wp;
        }
    }
    if inp[0] as i32 != sc.first_component {
        if sc.first_component != -1 {
            w.out(sc.post_min);
            sc.second_component = -1;
            w.out(sc.post_maj);
        }
        w.col = 0;
        w.out(sc.pre_maj);
        sc.first_component = inp[0] as i32;
    }
    if inp[1] as i32 != sc.second_component {
        if sc.second_component != -1 {
            w.out(sc.post_min);
        }
        w.out(sc.pre_min);
        sc.second_component = inp[1] as i32;
    }
    let n_outs = sc.n_outputs;
    for i in 0..n_outs as usize {
        w.write_byte(word2byte(out[i]));
    }
    1
}

/// `WriteCLUT` — `cmsStageSampleCLut16bit` driving the PS byte stream
fn write_clut(
    w: &mut PsWriter,
    mpe: &mut crate::lut::Stage,
    pre_maj: &'static str,
    post_maj: &'static str,
    pre_min: &'static str,
    post_min: &'static str,
    fix_white: bool,
    color_space: u32,
) {
    let (n_ins, n_outs, grid) = match &mpe.data {
        crate::lut::StageData::CLut(c) => (
            c.n_inputs,
            c.n_outputs,
            c.n_samples[..c.n_inputs as usize].to_vec(),
        ),
        _ => return,
    };
    let mut sc = PsSamplerCargo {
        first_component: -1,
        second_component: -1,
        n_outputs: n_outs,
        pre_maj,
        post_maj,
        pre_min,
        post_min,
        fix_white,
        color_space,
    };
    w.out("[");
    for i in 0..n_ins as usize {
        if i < crate::intrp::MAX_INPUT_DIMENSIONS as usize {
            w.out(&format!(" {} ", grid[i]));
        }
    }
    w.out(" [\n");
    {
        let w2 = &mut *w;
        let mut sampler = move |inp: &[u16], out: &mut [u16]| -> i32 {
            output_value_sampler(w2, inp, out, &mut sc)
        };
        crate::lut::stage_sample_clut16bit(mpe, &mut sampler, crate::lut::SAMPLER_INSPECT);
    }
    w.out(post_min);
    w.out(post_maj);
    w.out("] ");
}

/// `EmitCIEBasedA` — the gray CSA
fn emit_cie_based_a(w: &mut PsWriter, curve: &ToneCurve, black_point: &CieXyz) -> i32 {
    w.out("[ /CIEBasedA\n");
    w.out("  <<\n");
    w.out("/DecodeA ");
    emit_1_gamma(w, curve);
    w.out(" \n");
    w.out("/MatrixA [ 0.9642 1.0000 0.8249 ]\n");
    w.out("/RangeLMN [ 0.0 0.9642 0.0 1.0000 0.0 0.8249 ]\n");
    emit_white_black_d50(w, black_point);
    emit_intent(w, 0); // INTENT_PERCEPTUAL
    w.out(">>\n");
    w.out("]\n");
    1
}

/// `EmitCIEBasedABC` — RGB matrix-shaper CSA
fn emit_cie_based_abc(
    w: &mut PsWriter,
    matrix: &[f64; 9],
    curve_set: &[&ToneCurve],
    black_point: &CieXyz,
) -> i32 {
    w.out("[ /CIEBasedABC\n");
    w.out("<<\n");
    w.out("/DecodeABC [ ");
    emit_n_gamma(w, curve_set);
    w.out("]\n");
    w.out("/MatrixABC [ ");
    for i in 0..3 {
        w.out(&format!(
            "{:.6} {:.6} {:.6} ",
            matrix[i],
            matrix[i + 3],
            matrix[i + 6]
        ));
    }
    w.out("]\n");
    w.out("/RangeLMN [ 0.0 0.9642 0.0 1.0000 0.0 0.8249 ]\n");
    emit_white_black_d50(w, black_point);
    emit_intent(w, 0);
    w.out(">>\n");
    w.out("]\n");
    1
}

/// `EmitCIEBasedDEF` — LUT-based CSA (3 or 4 channel)
fn emit_cie_based_def(
    w: &mut PsWriter,
    pipeline: &mut crate::lut::Pipeline,
    intent: u32,
    black_point: &CieXyz,
) -> i32 {
    use crate::signatures::stage as sig;
    let Some(mpe) = pipeline.elements.as_deref_mut() else {
        return 0;
    };
    let (pre_maj, post_maj, pre_min, post_min) = match mpe.input_channels {
        3 => {
            w.out("[ /CIEBasedDEF\n");
            ("<", ">\n", "", "")
        }
        4 => {
            w.out("[ /CIEBasedDEFG\n");
            ("[", "]\n", "<", ">\n")
        }
        _ => return 0,
    };
    w.out("<<\n");
    let mut stage = mpe;
    if stage.stage_type() == sig::SigCurveSetElemType {
        w.out("/DecodeDEF [ ");
        if let crate::lut::StageData::ToneCurves(curves) = &stage.data {
            let refs: Vec<&ToneCurve> = curves.iter().collect();
            emit_n_gamma(w, &refs);
        }
        w.out("]\n");
        if stage.next.is_some() {
            stage = stage.next.as_deref_mut().unwrap();
        }
    }
    if stage.stage_type() == sig::SigCLutElemType {
        w.out("/Table ");
        write_clut(w, stage, pre_maj, post_maj, pre_min, post_min, false, 0);
        w.out("]\n");
    }
    emit_lab2xyz(w);
    emit_white_black_d50(w, black_point);
    emit_intent(w, intent);
    w.out("   >>\n");
    w.out("]\n");
    1
}

// ---------------------------------------------------------------------------
// Float formatting helpers — C printf semantics for byte-exact PS output
// ---------------------------------------------------------------------------

/// C `printf("%g", v)` — default precision 6: `%f` if the `%e` exponent is
/// in [-4, 6) else `%e`, trailing zeros stripped, >=2-digit exponent.
fn format_g(v: f64) -> String {
    if v.is_nan() {
        return "nan".to_string();
    }
    if v.is_infinite() {
        return if v < 0.0 { "-inf".into() } else { "inf".into() };
    }
    // Decimal exponent after %e rounding at precision P-1 = 5
    let s = format!("{v:.5e}");
    let epos = s.rfind('e').unwrap();
    let x: i32 = s[epos + 1..].parse().unwrap();
    if (-4..6).contains(&x) {
        // %f branch: precision P-1-X = 5-X, then strip zeros
        let prec = (5 - x).max(0) as usize;
        let mut f = format!("{v:.prec$}");
        if f.contains('.') {
            while f.ends_with('0') {
                f.pop();
            }
            if f.ends_with('.') {
                f.pop();
            }
        }
        f
    } else {
        let mut mant = s[..epos].to_string();
        if mant.contains('.') {
            while mant.ends_with('0') {
                mant.pop();
            }
            if mant.ends_with('.') {
                mant.pop();
            }
        }
        let sign = if x < 0 { '-' } else { '+' };
        format!("{mant}e{sign}{:02}", x.abs())
    }
}

/// `ctime()` — "Www Mmm %2d hh:mm:ss yyyy\n" of the current time (UTC; the
/// box runs TZ-naive for C either way — this matches glibc ctime's shape).
fn ctime_now() -> String {
    const DAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0) as i64;
    let days = secs.div_euclid(86400);
    let tod = secs.rem_euclid(86400);
    let (hh, mm, ss) = (tod / 3600, (tod % 3600) / 60, tod % 60);
    // civil-from-days (Hinnant)
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    let wday = DAYS[days.rem_euclid(7) as usize];
    format!(
        "{wday} {} {d:2} {hh:02}:{mm:02}:{ss:02} {year}\n",
        MONTHS[(m - 1) as usize]
    )
}

/// Serialize + reopen — the transform API needs owned profile boxes.
fn dup_profile(icc: &mut IccProfile) -> Option<Box<IccProfile>> {
    Some(Box::new(icc.clone()))
}

const TYPE_GRAY_8: u32 = crate::types::PT_GRAY << 16 | 1 << 3 | 1;
const TYPE_XYZ_DBL: u32 = 1 << 22 | crate::types::PT_XYZ << 16 | 3 << 3 | 0;
const TYPE_LAB_DBL: u32 = 1 << 22 | crate::types::PT_LAB << 16 | 3 << 3 | 0;
const TYPE_LAB_16: u32 = crate::types::PT_LAB << 16 | 3 << 3 | 2;
const TYPE_NAMED_COLOR_INDEX: u32 = 1 << 3 | 2;

/// `ExtractGray2Y` — gray profile → Y, as a 256-entry tone curve
fn extract_gray2y(icc: &mut IccProfile, intent: u32) -> Option<ToneCurve> {
    let mut out = crate::gamma::build_tabulated_tone_curve16(None, 256, None)?;
    let h_xyz = crate::virt::create_xyz_profile(None)?;
    let p_dup = dup_profile(icc)?;
    let mut profs = vec![p_dup, h_xyz];
    let mut bpc = [false, false];
    let intents = [intent, intent];
    let adapt = [1.0f64, 1.0];
    let mut xform = crate::xform::create_extended_transform(
        None,
        &mut profs,
        &mut bpc,
        &intents,
        &adapt,
        None,
        0,
        TYPE_GRAY_8,
        TYPE_XYZ_DBL,
        crate::types::FLAGS_NOOPTIMIZE,
    )?;
    for i in 0..256usize {
        let inp = [i as u8];
        let mut outb = [0u8; 24];
        xform.do_transform(&inp, &mut outb, 1);
        let y = f64::from_le_bytes(outb[8..16].try_into().unwrap());
        out.table16[i] = crate::fixed::quick_saturate_word(y * 65535.0);
    }
    Some(out)
}

/// `WriteInputLUT` — device-link → Lab → CIEBased* CSA
fn write_input_lut(w: &mut PsWriter, icc: &mut IccProfile, intent: u32, mut dw_flags: u32) -> bool {
    let input_format = crate::sm::formatter_for_colorspace(icc, 2, false);
    let n_channels = crate::pack::t_channels(input_format);
    let black_point = crate::sm::detect_black_point(icc, intent, 0).unwrap_or_default();

    let Some(hlab) = crate::virt::create_lab4_profile(None, None) else {
        return false;
    };
    let Some(p_dup) = dup_profile(icc) else {
        return false;
    };
    let mut profs = vec![p_dup, hlab];
    let mut bpc = [false, false];
    let intents = [intent, intent];
    let adapt = [1.0f64, 1.0];
    let Some(mut xform) = crate::xform::create_extended_transform(
        None,
        &mut profs,
        &mut bpc,
        &intents,
        &adapt,
        None,
        0,
        input_format,
        TYPE_LAB_DBL,
        0,
    ) else {
        crate::err::signal_error(
            None,
            crate::types::ERROR_COLORSPACE_CHECK,
            "Cannot create transform Profile -> Lab",
        );
        return false;
    };

    match n_channels {
        1 => {
            let Some(gray2y) = extract_gray2y(icc, intent) else {
                return false;
            };
            emit_cie_based_a(w, &gray2y, &black_point);
        }
        3 | 4 => {
            let mut out_frm = TYPE_LAB_16;
            let Some(mut link) = xform.lut.clone() else {
                return false;
            };
            dw_flags |= crate::types::FLAGS_FORCE_CLUT;
            let mut infmt = input_format;
            // C ignores the optimizer's return value here
            let _ = crate::opt::optimize_pipeline(
                None,
                &mut link,
                intent,
                &mut infmt,
                &mut out_frm,
                &mut dw_flags,
            );
            if emit_cie_based_def(w, &mut link, intent, &black_point) == 0 {
                return false;
            }
        }
        _ => {
            crate::err::signal_error(
                None,
                crate::types::ERROR_COLORSPACE_CHECK,
                "Only 3, 4 channels are supported for CSA. This profile has unsupported channels.",
            );
            return false;
        }
    }
    true
}

/// `WriteInputMatrixShaper` — gray/RGB matrix-shaper → CIEBasedA/ABC
fn write_input_matrix_shaper(
    w: &mut PsWriter,
    icc: &mut IccProfile,
    matrix: &crate::lut::Stage,
    shaper: &crate::lut::Stage,
) -> i32 {
    let color_space = icc.color_space;
    let black_point =
        crate::sm::detect_black_point(icc, crate::types::INTENT_RELATIVE_COLORIMETRIC, 0)
            .unwrap_or_default();
    if color_space == crate::signatures::colorspace::SigGrayData {
        let Some(curves) = crate::lut::stage_get_curve_set(shaper) else {
            return 0;
        };
        return emit_cie_based_a(w, &curves[0], &black_point);
    }
    if color_space == crate::signatures::colorspace::SigRgbData {
        let crate::lut::StageData::Matrix { double, .. } = &matrix.data else {
            return 0;
        };
        let mut mat: [f64; 9] = double[..9].try_into().unwrap();
        for i in 0..3 {
            for j in 0..3 {
                mat[i * 3 + j] *= crate::lut::MAX_ENCODEABLE_XYZ;
            }
        }
        let Some(curves) = crate::lut::stage_get_curve_set(shaper) else {
            return 0;
        };
        let refs: Vec<&ToneCurve> = curves.iter().collect();
        return emit_cie_based_abc(w, &mat, &refs, &black_point);
    }
    crate::err::signal_error(
        None,
        crate::types::ERROR_COLORSPACE_CHECK,
        "Profile is not suitable for CSA. Unsupported colorspace.",
    );
    0
}

/// `WriteNamedColorCSA` — HP-extension named-color CSA (Lab values)
fn write_named_color_csa(w: &mut PsWriter, icc: &mut IccProfile, intent: u32) -> i32 {
    let Some(hlab) = crate::virt::create_lab4_profile(None, None) else {
        return 0;
    };
    let Some(p_dup) = dup_profile(icc) else {
        return 0;
    };
    let mut profs = vec![p_dup, hlab];
    let mut bpc = [false, false];
    let intents = [intent, intent];
    let adapt = [1.0f64, 1.0];
    let Some(mut xform) = crate::xform::create_extended_transform(
        None,
        &mut profs,
        &mut bpc,
        &intents,
        &adapt,
        None,
        0,
        TYPE_NAMED_COLOR_INDEX,
        TYPE_LAB_DBL,
        0,
    ) else {
        return 0;
    };
    let Some(list) = crate::xform::get_named_color_list(&xform) else {
        return 0;
    };
    let list = list.clone();

    w.out("<<\n");
    w.out("(colorlistcomment) (Named color CSA)\n");
    w.out("(Prefix) [ (Pantone ) (PANTONE ) ]\n");
    w.out("(Suffix) [ ( CV) ( CVC) ( C) ]\n");

    let n_colors = crate::named::named_color_count(&list);
    for i in 0..n_colors {
        let Some((name, _pfx, _sfx, _pcs, _col)) = crate::named::named_color_info(&list, i) else {
            continue;
        };
        let inp = (i as u16).to_le_bytes();
        let mut outb = [0u8; 24];
        xform.do_transform(&inp, &mut outb, 1);
        let l = f64::from_le_bytes(outb[0..8].try_into().unwrap());
        let a = f64::from_le_bytes(outb[8..16].try_into().unwrap());
        let b = f64::from_le_bytes(outb[16..24].try_into().unwrap());
        let nend = name.iter().position(|&b| b == 0).unwrap_or(name.len());
        w.out("  (");
        w.emit_ps_escaped(&String::from_utf8_lossy(&name[..nend]));
        w.out(&format!(") [ {l:.3} {a:.3} {b:.3} ]\n"));
    }
    w.out(">>\n");
    1
}

/// `GenerateCSA` — CSA on XYZ colorspace
fn generate_csa(
    icc: &mut IccProfile,
    intent: u32,
    dw_flags: u32,
    io: &mut IoHandler,
) -> Option<u32> {
    let mut w = PsWriter::new(io);
    if icc.device_class == crate::signatures::profileclass::SigNamedColorClass {
        if write_named_color_csa(&mut w, icc, intent) == 0 {
            return None;
        }
    } else {
        use crate::signatures::colorspace as cs;
        let pcs = icc.pcs;
        if pcs != cs::SigXYZData && pcs != cs::SigLabData {
            crate::err::signal_error(
                None,
                crate::types::ERROR_COLORSPACE_CHECK,
                "Invalid output color space",
            );
            return None;
        }
        let lut = crate::io1::read_input_lut(icc, intent)?;
        if let Some(stages) = crate::lut::pipeline_check_and_retrieve_stages(
            &lut,
            &[
                crate::signatures::stage::SigCurveSetElemType,
                crate::signatures::stage::SigMatrixElemType,
            ],
        ) {
            let shaper = stages[0].clone();
            let matrix = stages[1].clone();
            if write_input_matrix_shaper(&mut w, icc, &matrix, &shaper) == 0 {
                return None;
            }
        } else if !write_input_lut(&mut w, icc, intent, dw_flags) {
            return None;
        }
    }
    Some(io_used_space(&mut w))
}

fn io_used_space(w: &mut PsWriter) -> u32 {
    w.io.used_space
}

/// `EmitPQRStage` — Bradford cone-space transform + optional BPC
fn emit_pqr_stage(w: &mut PsWriter, icc: &mut IccProfile, do_bpc: bool, is_absolute: bool) {
    if is_absolute {
        // encode back to relative: XYZpcs * (D50/WhitePoint)
        let white = crate::io1::read_media_white_point(icc);
        w.out("/MatrixPQR [1 0 0 0 1 0 0 0 1 ]\n");
        w.out("/RangePQR [ -0.5 2 -0.5 2 -0.5 2 ]\n");
        w.out("% Absolute colorimetric -- encode to relative to maximize LUT usage\n");
        w.out("/TransformPQR [\n");
        w.out(&format!(
            "{{0.9642 mul {} div exch pop exch pop exch pop exch pop}} bind\n",
            format_g(white.X)
        ));
        w.out(&format!(
            "{{1.0000 mul {} div exch pop exch pop exch pop exch pop}} bind\n",
            format_g(white.Y)
        ));
        w.out(&format!(
            "{{0.8249 mul {} div exch pop exch pop exch pop exch pop}} bind\n]\n",
            format_g(white.Z)
        ));
        return;
    }
    w.out("% Bradford Cone Space\n");
    w.out("/MatrixPQR [0.8951 -0.7502 0.0389 0.2664 1.7135 -0.0685 -0.1614 0.0367 1.0296 ] \n");
    w.out("/RangePQR [ -0.5 2 -0.5 2 -0.5 2 ]\n");
    if !do_bpc {
        w.out("% VonKries-like transform in Bradford Cone Space\n");
        w.out("/TransformPQR [\n");
        w.out("{exch pop exch 3 get mul exch pop exch 3 get div} bind\n");
        w.out("{exch pop exch 4 get mul exch pop exch 4 get div} bind\n");
        w.out("{exch pop exch 5 get mul exch pop exch 5 get div} bind\n]\n");
    } else {
        w.out("% VonKries-like transform in Bradford Cone Space plus BPC\n");
        w.out("/TransformPQR [\n");
        w.out("{4 index 3 get div 2 index 3 get mul 2 index 3 get 2 index 3 get sub mul 2 index 3 get 4 index 3 get 3 index 3 get sub mul sub 3 index 3 get 3 index 3 get exch sub div exch pop exch pop exch pop exch pop } bind\n");
        w.out("{4 index 4 get div 2 index 4 get mul 2 index 4 get 2 index 4 get sub mul 2 index 4 get 4 index 4 get 3 index 4 get sub mul sub 3 index 4 get 3 index 4 get exch sub div exch pop exch pop exch pop exch pop } bind\n");
        w.out("{4 index 5 get div 2 index 5 get mul 2 index 5 get 2 index 5 get sub mul 2 index 5 get 4 index 5 get 3 index 5 get sub mul sub 3 index 5 get 3 index 5 get exch sub div exch pop exch pop exch pop exch pop } bind\n]\n");
    }
}

/// `EmitXYZ2Lab` — the fixed LMN encode block
fn emit_xyz2lab(w: &mut PsWriter) {
    w.out("/RangeLMN [ -0.635 2.0 0 2 -0.635 2.0 ]\n");
    w.out("/EncodeLMN [\n");
    w.out(
        "{ 0.964200  div dup 0.008856 le {7.787 mul 16 116 div add}{1 3 div exp} ifelse } bind\n",
    );
    w.out(
        "{ 1.000000  div dup 0.008856 le {7.787 mul 16 116 div add}{1 3 div exp} ifelse } bind\n",
    );
    w.out(
        "{ 0.824900  div dup 0.008856 le {7.787 mul 16 116 div add}{1 3 div exp} ifelse } bind\n",
    );
    w.out("]\n");
    w.out("/MatrixABC [ 0 1 0 1 -1 1 0 0 -1 ]\n");
    w.out("/EncodeABC [\n");
    w.out("{ 116 mul  16 sub 100 div  } bind\n");
    w.out("{ 500 mul 128 add 256 div  } bind\n");
    w.out("{ 200 mul 128 add 256 div  } bind\n");
    w.out("]\n");
}

/// `WriteOutputLUT` — Lab → profile CLUT as a CRD RenderTable
fn write_output_lut(
    w: &mut PsWriter,
    icc: &mut IccProfile,
    intent: u32,
    mut dw_flags: u32,
) -> bool {
    let output_format = crate::sm::formatter_for_colorspace(icc, 2, false);
    let n_channels = crate::pack::t_channels(output_format);
    let color_space = icc.color_space;
    let do_bpc = dw_flags & crate::types::FLAGS_BLACKPOINTCOMPENSATION != 0;
    let mut fix_white = dw_flags & crate::types::FLAGS_NOWHITEONWHITEFIXUP == 0;

    let Some(hlab) = crate::virt::create_lab4_profile(None, None) else {
        return false;
    };
    // absolute colorimetric encodes the LUT as relative
    let rel_intent = if intent == crate::types::INTENT_ABSOLUTE_COLORIMETRIC {
        crate::types::INTENT_RELATIVE_COLORIMETRIC
    } else {
        intent
    };
    let Some(p_dup) = dup_profile(icc) else {
        return false;
    };
    let mut profs = vec![hlab, p_dup];
    let mut bpc = [false, false];
    let intents = [rel_intent, rel_intent];
    let adapt = [1.0f64, 1.0];
    let Some(mut xform) = crate::xform::create_extended_transform(
        None,
        &mut profs,
        &mut bpc,
        &intents,
        &adapt,
        None,
        0,
        TYPE_LAB_DBL,
        output_format,
        0,
    ) else {
        crate::err::signal_error(
            None,
            crate::types::ERROR_COLORSPACE_CHECK,
            "Cannot create transform Lab -> Profile in CRD creation",
        );
        return false;
    };
    let Some(mut link) = xform.lut.clone() else {
        crate::err::signal_error(
            None,
            crate::types::ERROR_CORRUPTION_DETECTED,
            "Cannot access link for CRD",
        );
        return false;
    };

    dw_flags |= crate::types::FLAGS_FORCE_CLUT;
    let mut in_frm = TYPE_LAB_16;
    let mut outfmt = output_format;
    if !crate::opt::optimize_pipeline(
        None,
        &mut link,
        rel_intent,
        &mut in_frm,
        &mut outfmt,
        &mut dw_flags,
    ) {
        crate::err::signal_error(
            None,
            crate::types::ERROR_CORRUPTION_DETECTED,
            "Cannot create CLUT table for CRD",
        );
        return false;
    }

    w.out("<<\n");
    w.out("/ColorRenderingType 1\n");

    let black_point = crate::sm::detect_black_point(icc, intent, 0).unwrap_or_default();
    emit_white_black_d50(w, &black_point);
    emit_pqr_stage(
        w,
        icc,
        do_bpc,
        intent == crate::types::INTENT_ABSOLUTE_COLORIMETRIC,
    );
    emit_xyz2lab(w);

    // FIXUP: map Lab(100,0,0) to perfect white
    if intent == crate::types::INTENT_ABSOLUTE_COLORIMETRIC {
        fix_white = false;
    }

    w.out("/RenderTable ");
    if let Some(first) = link.elements.as_deref_mut() {
        if first.r#type != crate::signatures::stage::SigCLutElemType {
            crate::err::signal_error(
                None,
                crate::types::ERROR_CORRUPTION_DETECTED,
                "Cannot create CLUT, revise your flags!",
            );
            return false;
        }
        write_clut(w, first, "<", ">\n", "", "", fix_white, color_space);
    }

    w.out(&format!(" {} {{}} bind ", n_channels));
    for _ in 1..n_channels {
        w.out("dup ");
    }
    w.out("]\n");
    emit_intent(w, intent);
    w.out(">>\n");
    if dw_flags & crate::types::FLAGS_NODEFAULTRESOURCEDEF == 0 {
        w.out("/Current exch /ColorRendering defineresource pop\n");
    }
    true
}

/// `BuildColorantList` — "0.000 1.000 ..." string of normalized colorants
fn build_colorant_list(n_colorant: usize, out: &[u16]) -> String {
    let n = n_colorant.min(crate::lut::MAX_CHANNELS as usize);
    (0..n)
        .map(|j| format!("{:.3}", out[j] as f64 / 65535.0))
        .collect::<Vec<_>>()
        .join(" ")
}

/// `WriteNamedColorCRD` — HP-extension named-color CRD (device colorants)
fn write_named_color_crd(
    w: &mut PsWriter,
    icc: &mut IccProfile,
    intent: u32,
    dw_flags: u32,
) -> i32 {
    let output_format = crate::sm::formatter_for_colorspace(icc, 2, false);
    let n_colorant = crate::pack::t_channels(output_format) as usize;
    let Some(p_dup) = dup_profile(icc) else {
        return 0;
    };
    let mut profs = vec![p_dup];
    let mut bpc = [false];
    let intents = [intent];
    let adapt = [1.0f64];
    let Some(mut xform) = crate::xform::create_extended_transform(
        None,
        &mut profs,
        &mut bpc,
        &intents,
        &adapt,
        None,
        0,
        TYPE_NAMED_COLOR_INDEX,
        output_format,
        dw_flags,
    ) else {
        return 0;
    };
    let Some(list) = crate::xform::get_named_color_list(&xform) else {
        return 0;
    };
    let list = list.clone();

    w.out("<<\n");
    w.out("(colorlistcomment) (Named profile) \n");
    w.out("(Prefix) [ (Pantone ) (PANTONE ) ]\n");
    w.out("(Suffix) [ ( CV) ( CVC) ( C) ]\n");

    let n_colors = crate::named::named_color_count(&list);
    for i in 0..n_colors {
        let Some((name, _pfx, _sfx, _pcs, _col)) = crate::named::named_color_info(&list, i) else {
            continue;
        };
        let inp = (i as u16).to_le_bytes();
        let mut outb = [0u8; 2 * crate::lut::MAX_CHANNELS as usize];
        xform.do_transform(&inp, &mut outb, 1);
        let mut out = [0u16; crate::lut::MAX_CHANNELS as usize];
        for (j, o) in out.iter_mut().enumerate() {
            *o = u16::from_le_bytes([outb[2 * j], outb[2 * j + 1]]);
        }
        let colorant = build_colorant_list(n_colorant, &out);
        let nend = name.iter().position(|&b| b == 0).unwrap_or(name.len());
        w.out("  (");
        w.emit_ps_escaped(&String::from_utf8_lossy(&name[..nend]));
        w.out(&format!(") [ {colorant} ]\n"));
    }
    w.out("   >>");
    if dw_flags & crate::types::FLAGS_NODEFAULTRESOURCEDEF == 0 {
        w.out(" /Current exch /HPSpotTable defineresource pop\n");
    }
    1
}

/// `GenerateCRD` — always LUT-based
fn generate_crd(
    icc: &mut IccProfile,
    intent: u32,
    dw_flags: u32,
    io: &mut IoHandler,
) -> Option<u32> {
    let mut w = PsWriter::new(io);
    if dw_flags & crate::types::FLAGS_NODEFAULTRESOURCEDEF == 0 {
        emit_header(
            &mut w,
            "Color Rendering Dictionary (CRD)",
            icc,
            &ctime_now(),
        );
    }
    if icc.device_class == crate::signatures::profileclass::SigNamedColorClass {
        if write_named_color_crd(&mut w, icc, intent, dw_flags) == 0 {
            return None;
        }
    } else if !write_output_lut(&mut w, icc, intent, dw_flags) {
        return None;
    }
    if dw_flags & crate::types::FLAGS_NODEFAULTRESOURCEDEF == 0 {
        w.out("%%EndResource\n");
        w.out("\n% CRD End\n");
    }
    Some(io_used_space(&mut w))
}

/// `cmsPSResourceType`
pub const PS_RESOURCE_CSA: u32 = 0;
pub const PS_RESOURCE_CRD: u32 = 1;

/// `cmsGetPostScriptColorResource` — dispatch on resource type
pub fn get_post_script_color_resource(
    _ctx: Option<&Context>,
    resource_type: u32,
    icc: &mut IccProfile,
    intent: u32,
    dw_flags: u32,
    io: &mut IoHandler,
) -> u32 {
    match resource_type {
        PS_RESOURCE_CSA => generate_csa(icc, intent, dw_flags, io).unwrap_or(0),
        _ => generate_crd(icc, intent, dw_flags, io).unwrap_or(0),
    }
}

/// `cmsGetPostScriptCRD` — public entry point
pub fn get_post_script_crd(
    _ctx: Option<&Context>,
    icc: &mut IccProfile,
    intent: u32,
    dw_flags: u32,
    io: &mut IoHandler,
) -> u32 {
    get_post_script_color_resource(None, PS_RESOURCE_CRD, icc, intent, dw_flags, io)
}

/// `cmsGetPostScriptCSA` — public entry point
pub fn get_post_script_csa(
    _ctx: Option<&Context>,
    icc: &mut IccProfile,
    intent: u32,
    dw_flags: u32,
    io: &mut IoHandler,
) -> u32 {
    get_post_script_color_resource(None, PS_RESOURCE_CSA, icc, intent, dw_flags, io)
}
