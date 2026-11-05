//! `cmsxform.c` — `_cmsTRANSFORM`: links profiles into one pipeline via
//! `cnvrt`, binds pack formatters, and drives the eval loops.
//! `xform` is a stored closure; cache/adaptation state live on the struct.

use crate::cnvrt;
use crate::context::Context;
use crate::lut::Pipeline;
use crate::pack::{
    CMS_PACK_FLAGS_16BITS, CMS_PACK_FLAGS_FLOAT, Formatter, FormatterDir, PackInfo, get_formatter,
    t_channels,
};
use crate::profile::IccProfile;

pub const FLAGS_NOOPTIMIZE: u32 = 0x0100;
pub const FLAGS_NOCACHE: u32 = 0x0040;
pub const FLAGS_HIGHRESPRECALC: u32 = 0x0400;
pub const FLAGS_LOWRESPRECALC: u32 = 0x0800;

pub const MAX_CHANNELS: usize = 16;

/// `cmsStride`
#[derive(Clone, Copy, Default)]
pub struct Stride {
    pub bytes_per_line_in: u32,
    pub bytes_per_line_out: u32,
    pub bytes_per_plane_in: u32,
    pub bytes_per_plane_out: u32,
}

/// Per-pixel input/output formatters for a live transform.
enum Fmt {
    F16 {
        unroll: crate::pack::Unpack16Fn,
        pack: crate::pack::Pack16Fn,
    },
    F32 {
        unroll: crate::pack::UnpackF32Fn,
        pack: crate::pack::PackF32Fn,
    },
}

/// `_cmsTRANSFORM`
pub struct Transform {
    pub input_format: u32,
    pub output_format: u32,
    pub lut: Pipeline,
    pub gamut_check: Option<Pipeline>,
    pub alarm_codes: [u16; MAX_CHANNELS],
    pub ctx: Option<Context>,
    pub stride: Stride,
    fmt: Fmt,
    /// cached xform — the C `xform` fn pointer maps to the eval loop used
    pub cache_in: [u16; MAX_CHANNELS],
    pub cache_out: [u16; MAX_CHANNELS],
    pub cache_valid: bool,
    /// `wIn`/`wOut` scratch moved out of the loop for the 16-bit path
    _pad: (),
}

fn pixel_size(fmt: u32) -> u32 {
    let b = crate::pack::t_bytes(fmt);
    if b == 0 { 8 } else { b }
}

impl Transform {
    /// `cmsDoTransform` — n pixels, default stride
    pub fn do_transform(&mut self, input: &[u8], output: &mut [u8], n: u32) {
        let stride = Stride {
            bytes_per_line_in: 0,
            bytes_per_line_out: 0,
            bytes_per_plane_in: n * pixel_size(self.input_format),
            bytes_per_plane_out: n * pixel_size(self.output_format),
        };
        self.xform(input, output, n, 1, &stride);
    }

    /// `FloatXFORM` / `PrecalculatedXFORM` — the main eval loops
    fn xform(&mut self, input: &[u8], output: &mut [u8], ppl: u32, lines: u32, stride: &Stride) {
        match self.fmt {
            Fmt::F16 { unroll, pack } => {
                self.xform16(input, output, ppl, lines, stride, unroll, pack);
            }
            Fmt::F32 { unroll, pack } => {
                self.xform_f32(input, output, ppl, lines, stride, unroll, pack);
            }
        }
    }

    fn xform16(
        &mut self,
        input: &[u8],
        output: &mut [u8],
        ppl: u32,
        lines: u32,
        stride: &Stride,
        unroll: crate::pack::Unpack16Fn,
        pack: crate::pack::Pack16Fn,
    ) {
        let info = PackInfo {
            input_format: self.input_format,
            output_format: self.output_format,
            stride: 0,
        };
        let mut w_in = [0u16; MAX_CHANNELS];
        let mut w_out = [0u16; MAX_CHANNELS];
        let mut i_off = 0usize;
        let mut o_off = 0usize;
        for _ in 0..lines {
            let mut accum = i_off;
            let mut out_pos = o_off;
            for _ in 0..ppl {
                let adv = unroll(&info, &mut w_in, &input[accum..], stride.bytes_per_plane_in);
                accum += adv;
                if let Some(g) = &mut self.gamut_check {
                    let mut g_out = [0u16; MAX_CHANNELS];
                    g.eval16(&w_in, &mut g_out);
                    if g_out[0] > 0 {
                        w_out[..MAX_CHANNELS].copy_from_slice(&self.alarm_codes);
                    } else {
                        self.lut.eval16(&w_in, &mut w_out);
                    }
                } else {
                    self.lut.eval16(&w_in, &mut w_out);
                }
                let adv = pack(
                    &info,
                    &w_out,
                    &mut output[out_pos..],
                    stride.bytes_per_plane_out,
                );
                out_pos += adv;
            }
            i_off += stride.bytes_per_line_in as usize;
            o_off += stride.bytes_per_line_out as usize;
        }
    }

    fn xform_f32(
        &mut self,
        input: &[u8],
        output: &mut [u8],
        ppl: u32,
        lines: u32,
        stride: &Stride,
        unroll: crate::pack::UnpackF32Fn,
        pack: crate::pack::PackF32Fn,
    ) {
        let info = PackInfo {
            input_format: self.input_format,
            output_format: self.output_format,
            stride: 0,
        };
        let mut f_in = [0f32; MAX_CHANNELS];
        let mut f_out = [0f32; MAX_CHANNELS];
        let mut i_off = 0usize;
        let mut o_off = 0usize;
        for _ in 0..lines {
            let mut accum = i_off;
            let mut out_pos = o_off;
            for _ in 0..ppl {
                let adv = unroll(&info, &mut f_in, &input[accum..], stride.bytes_per_plane_in);
                accum += adv;
                if let Some(g) = &mut self.gamut_check {
                    let mut g_out = [0f32; 1];
                    g.eval_float(&f_in, &mut g_out);
                    if g_out[0] > 0.0 {
                        for c in 0..MAX_CHANNELS {
                            f_out[c] = self.alarm_codes[c] as f32 / 65535.0;
                        }
                    } else {
                        self.lut.eval_float(&f_in, &mut f_out);
                    }
                } else {
                    self.lut.eval_float(&f_in, &mut f_out);
                }
                let adv = pack(
                    &info,
                    &f_out,
                    &mut output[out_pos..],
                    stride.bytes_per_plane_out,
                );
                out_pos += adv;
            }
            i_off += stride.bytes_per_line_in as usize;
            o_off += stride.bytes_per_line_out as usize;
        }
    }
}

/// `cmsCreateExtendedTransform` — the real constructor: link + formatters
pub fn create_extended_transform(
    ctx: Option<Context>,
    profiles: &mut [Box<IccProfile>],
    bpc: &mut [bool],
    intents: &[u32],
    adaptation_states: &[f64],
    _gamut_profile: Option<&mut IccProfile>,
    _gamut_pcs_position: u32,
    input_format: u32,
    output_format: u32,
    dw_flags: u32,
) -> Option<Box<Transform>> {
    let lut = cnvrt::link_profiles(
        ctx.clone(),
        profiles,
        intents,
        bpc,
        adaptation_states,
        dw_flags,
    )?;
    let is_float = crate::pack::t_float(input_format) != 0
        || crate::pack::t_float(output_format) != 0
        || crate::pack::t_bytes(input_format) == 0
        || crate::pack::t_bytes(output_format) == 0;
    let dir_in = get_formatter(
        input_format,
        FormatterDir::Input,
        if is_float {
            CMS_PACK_FLAGS_FLOAT
        } else {
            CMS_PACK_FLAGS_16BITS
        },
    );
    let dir_out = get_formatter(
        output_format,
        FormatterDir::Output,
        if is_float {
            CMS_PACK_FLAGS_FLOAT
        } else {
            CMS_PACK_FLAGS_16BITS
        },
    );
    let fmt = match (is_float, dir_in, dir_out) {
        (true, Formatter::UnpackF32(u), Formatter::PackF32(p)) => Fmt::F32 { unroll: u, pack: p },
        (false, Formatter::Unpack16(u), Formatter::Pack16(p)) => Fmt::F16 { unroll: u, pack: p },
        _ => {
            crate::err::signal_error(
                ctx.as_ref(),
                crate::types::ERROR_UNKNOWN_EXTENSION,
                "Unknown input/output formatters",
            );
            return None;
        }
    };
    // sanity: channel counts vs format channels
    let expected_in = t_channels(input_format);
    let expected_out = t_channels(output_format);
    if expected_in != lut.input_channels || expected_out != lut.output_channels {
        crate::err::signal_error(
            ctx.as_ref(),
            crate::types::ERROR_COLORSPACE_CHECK,
            "Channel mismatch between formatter and profile chain",
        );
        return None;
    }
    let mut t = Box::new(Transform {
        input_format,
        output_format,
        lut,
        gamut_check: None,
        alarm_codes: [0; MAX_CHANNELS],
        ctx,
        stride: Stride::default(),
        fmt,
        cache_in: [0; MAX_CHANNELS],
        cache_out: [0; MAX_CHANNELS],
        cache_valid: false,
        _pad: (),
    });
    // cache seed: transform of all-zeros (16-bit only, FLAGS_NOCACHE skips)
    if matches!(t.fmt, Fmt::F16 { .. }) && dw_flags & FLAGS_NOCACHE == 0 {
        let mut z_in = [0u16; MAX_CHANNELS];
        let mut z_out = [0u16; MAX_CHANNELS];
        t.lut.eval16(&z_in, &mut z_out);
        t.cache_in = z_in;
        t.cache_out = z_out;
    }
    Some(t)
}
