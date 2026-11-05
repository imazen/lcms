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
    /// `_cmsIOPrintf`-style output + column tracking
    fn out(&mut self, s: &str) {
        for b in s.bytes() {
            if b == b'\n' {
                self.col = 0;
            } else {
                self.col += 1;
            }
        }
        self.io.write(s.len() as u32, s.as_bytes());
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
        "/BlackPoint  [{:.5} {:.5} {:.5}]\n",
        black_point.X, black_point.Y, black_point.Z
    ));
    w.out(&format!(
        "/WhitePoint  [{:.5} {:.5} {:.5}]\n",
        crate::types::D50X,
        crate::types::D50Y,
        crate::types::D50Z
    ));
}

/// `EmitRangeCheck`
fn emit_range_check(w: &mut PsWriter) {
    w.out("{dup 0 lt {pop 0} if dup 1 gt {pop 1} if} bind\n");
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
        // TODO(cmsps2): C's %g — Rust's {} gives shortest round-trip but a
        // different exponent style; needs a C-printf %g helper for byte parity
        w.out(&format!("{{ {gamma} exp }} bind "));
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

// ---------------------------------------------------------------------------
// TODO(cmsps2): the remaining emitters are larger — EmitCIEBasedA/ABC/DEF
// (sampled-function CLUT serialization), WriteInputLUT/OutputLUT/
// NamedColorCSA/CRD, EmitPQRStage, EmitXYZ2Lab, GenerateCSA/CRD
// (the pipeline-walk emitters), and the 3 public entry points
// (cmsGetPostScriptColorResource/CRD/CSA). These serialize the
// profile machinery into PS dictionary syntax and are ported next.
// ---------------------------------------------------------------------------

/// `cmsGetPostScriptCSA` — public entry point
pub fn get_post_script_csa(
    ctx: Option<&Context>,
    icc: &mut IccProfile,
    intent: u32,
    dw_flags: u32,
    io: &mut IoHandler,
) -> Option<u32> {
    let _ = (ctx, intent, dw_flags);
    let mut w = PsWriter::new(io);
    emit_header(&mut w, "Color Space Array", icc, "");
    // TODO: full CSA generation — matrix-shaper, LUT, or named-color path
    None
}
