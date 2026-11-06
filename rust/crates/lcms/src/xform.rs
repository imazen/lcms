//! `cmsxform.c` — `_cmsTRANSFORM`: links profiles into one pipeline via
//! `cnvrt`, binds pack formatters, and drives the eval loops.
//! `xform` selects the eval loop; cache/adaptation state live on the struct.

use crate::cnvrt;
use crate::context::Context;
use crate::lut::Pipeline;
use crate::pack::{
    CMS_PACK_FLAGS_16BITS, CMS_PACK_FLAGS_FLOAT, Formatter, FormatterDir, PackInfo,
    get_formatter_ctx,
    t_channels,
};
use crate::profile::IccProfile;
use crate::types::{CieXyz, INTENT_PERCEPTUAL};

pub const FLAGS_NOOPTIMIZE: u32 = 0x0100;
pub const FLAGS_NOCACHE: u32 = 0x0040;
pub const FLAGS_HIGHRESPRECALC: u32 = 0x0400;
pub const FLAGS_LOWRESPRECALC: u32 = 0x0800;
pub const FLAGS_NULLTRANSFORM: u32 = 0x0200;
pub const FLAGS_GAMUTCHECK: u32 = 0x1000;
pub const FLAGS_COPY_ALPHA: u32 = 0x04000000;
pub const FLAGS_KEEP_SEQUENCE: u32 = 0x10000000;
pub const FLAGS_CAN_CHANGE_FORMATTER: u32 = 0x02000000;

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
#[derive(Clone, Copy)]
enum Fmt {
    F16 {
        unroll: crate::pack::Unpack16Fn,
        pack: crate::pack::Pack16Fn,
    },
    F32 {
        unroll: crate::pack::UnpackF32Fn,
        pack: crate::pack::PackF32Fn,
    },
    /// `UnrollNothing`/`PackNothing` — both formats 0, used by linkicc
    Noop,
}

/// Which eval loop `xform` dispatches to — the C `xform` fn pointer.
#[derive(Clone, Copy, PartialEq)]
pub enum Worker {
    Precalculated,
    PrecalculatedGamutCheck,
    Cached,
    CachedGamutCheck,
    Null,
    Float,
    NullFloat,
    Noop,
    /// A C `cmsPluginTransform`-supplied `_cmsTransformFn` — dispatched
    /// through `XFORM_HOOKS` with a C-ABI `_cmstransform_struct` view.
    Foreign,
}

/// `_cmsTRANSFORM::OldXform` + plugin user data — raw C fn ptrs/addresses.
#[derive(Clone, Copy)]
pub struct ForeignXform {
    /// The C `_cmsTransformFn` returned by the factory.
    pub old_xform: usize,
    /// `UserData` out-param from the factory.
    pub user_data: usize,
    /// `FreePrivateDataFn` out-param (0 = none).
    pub free_fn: usize,
}

/// `_cmsTRANSFORM`
pub struct Transform {
    pub input_format: u32,
    pub output_format: u32,
    pub lut: Option<Pipeline>,
    pub gamut_check: Option<Pipeline>,
    pub alarm_codes: [u16; MAX_CHANNELS],
    pub ctx: Option<Context>,
    pub stride: Stride,
    fmt: Fmt,
    pub worker: Worker,
    /// `_cmsCACHE` — the per-pixel in/out cache (16-bit path)
    pub cache_in: [u16; MAX_CHANNELS],
    pub cache_out: [u16; MAX_CHANNELS],
    /// entry/exit color-space signatures
    pub entry_color_space: u32,
    pub exit_color_space: u32,
    pub rendering_intent: u32,
    pub dw_flags: u32,
    pub entry_white_point: CieXyz,
    pub exit_white_point: CieXyz,
    /// transform-plugin user data
    pub user_data: Option<Box<dyn std::any::Any + Send + Sync>>,
    /// C plugin transform (`cmsPluginTransform` factory result)
    pub foreign: Option<ForeignXform>,
    /// `_cmsTRANSFORM::InputColorant` — dup of the input profile's colorant table
    pub input_colorants: Option<crate::named::NamedColorList>,
    /// `_cmsTRANSFORM::OutputColorant` — dup of the output profile's colorant table
    pub output_colorants: Option<crate::named::NamedColorList>,
    _pad: (),
}

fn pixel_size(fmt: u32) -> u32 {
    let b = crate::pack::t_bytes(fmt);
    if b == 0 { 8 } else { b }
}

/// Generic `Precalculated`/`Cached` (!gamut) pixel loop with the eval
/// monomorphized — each `OptData` arm instantiates this with its eval
/// inlined, so no per-pixel `opt_data` match or `eval16_fn` indirection.
#[allow(clippy::too_many_arguments)]
fn xform16_eval_loop<E: Fn(&[u16], &mut [u16])>(
    unroll: crate::pack::Unpack16Fn,
    pack: crate::pack::Pack16Fn,
    info: &PackInfo,
    input: &[u8],
    output: &mut [u8],
    ppl: u32,
    lines: u32,
    stride: &Stride,
    cache: bool,
    cache_in: &mut [u16; MAX_CHANNELS],
    cache_out: &mut [u16; MAX_CHANNELS],
    eval: E,
) {
    let mut w_in = [0u16; MAX_CHANNELS];
    let mut w_out = [0u16; MAX_CHANNELS];
    let mut i_off = 0usize;
    let mut o_off = 0usize;
    for _ in 0..lines {
        let mut accum = i_off;
        let mut out_pos = o_off;
        for _ in 0..ppl {
            accum += unroll(info, &mut w_in, &input[accum..], stride.bytes_per_plane_in);
            if cache && w_in[..MAX_CHANNELS] == cache_in[..MAX_CHANNELS] {
                w_out[..MAX_CHANNELS].copy_from_slice(&cache_out[..MAX_CHANNELS]);
            } else {
                eval(&w_in, &mut w_out);
                if cache {
                    cache_in[..MAX_CHANNELS].copy_from_slice(&w_in[..MAX_CHANNELS]);
                    cache_out[..MAX_CHANNELS].copy_from_slice(&w_out[..MAX_CHANNELS]);
                }
            }
            out_pos += pack(
                info,
                &w_out,
                &mut output[out_pos..],
                stride.bytes_per_plane_out,
            );
        }
        i_off += stride.bytes_per_line_in as usize;
        o_off += stride.bytes_per_line_out as usize;
    }
}

/// `IsProperColorSpace` — format's PT_* vs the profile-chain space
fn is_proper_color_space(check: u32, dw_format: u32) -> bool {
    let space1 = crate::pack::t_colorspace(dw_format) as i32;
    let space2 = crate::pcs::lcms_color_space(check);
    if dw_format == 0 {
        return true;
    }
    if space1 == crate::types::PT_ANY as i32 {
        return t_channels(dw_format) as i32 == crate::pcs::channels_of_color_space(check);
    }
    if space1 == space2 {
        return true;
    }
    if space1 == crate::types::PT_LABV2 as i32 && space2 == crate::types::PT_LAB as i32 {
        return true;
    }
    if space1 == crate::types::PT_LAB as i32 && space2 == crate::types::PT_LABV2 as i32 {
        return true;
    }
    false
}

/// `GetXFormColorSpaces` — chain entry/exit spaces, named-color special-cased
fn get_xform_color_spaces(profiles: &[Box<IccProfile>]) -> Option<(u32, u32)> {
    use crate::signatures::{colorspace as cs, profileclass as pc};
    let n = profiles.len();
    if n == 0 {
        return None;
    }
    let mut post_color_space = profiles[0].color_space;
    let mut input = post_color_space;
    for (i, prof) in profiles.iter().enumerate() {
        let is_input = post_color_space != cs::SigXYZData && post_color_space != cs::SigLabData;
        let cls = prof.device_class;
        let (cs_in, cs_out) = if cls == pc::SigNamedColorClass {
            (
                cs::Sig1colorData,
                if n > 1 { prof.pcs } else { prof.color_space },
            )
        } else if is_input || cls == pc::SigLinkClass {
            (prof.color_space, prof.pcs)
        } else {
            (prof.pcs, prof.color_space)
        };
        if i == 0 {
            input = cs_in;
        }
        post_color_space = cs_out;
    }
    Some((input, post_color_space))
}

/// `NormalizeXYZ` — bring white points below 2.0
fn normalize_xyz(d: &mut CieXyz) {
    while d.X > 2.0 && d.Y > 2.0 && d.Z > 2.0 {
        d.X /= 10.0;
        d.Y /= 10.0;
        d.Z /= 10.0;
    }
}

/// `SetWhitePoint` — D50 fallback then normalize
fn set_white_point(src: Option<&CieXyz>) -> CieXyz {
    match src {
        None => CieXyz {
            X: crate::types::D50X,
            Y: crate::types::D50Y,
            Z: crate::types::D50Z,
        },
        Some(s) => {
            let mut w = *s;
            normalize_xyz(&mut w);
            w
        }
    }
}

/// `UnrollNothing` — format 0 input
fn unroll_nothing(_i: &PackInfo, _w: &mut [u16], _a: &[u8], _s: u32) -> usize {
    0
}
/// `PackNothing` — format 0 output
fn pack_nothing(_i: &PackInfo, _w: &[u16], _o: &mut [u8], _s: u32) -> usize {
    0
}

/// `_cmsHandleExtraChannels` — delegates to alpha.rs's converter-matrix impl
fn handle_extra_channels(
    input: &[u8],
    output: &mut [u8],
    ppl: u32,
    lines: u32,
    stride: &Stride,
    in_fmt: u32,
    out_fmt: u32,
    dw_flags: u32,
    ctx: Option<&Context>,
) {
    crate::alpha::handle_extra_channels(
        ctx, input, output, ppl, lines, stride, in_fmt, out_fmt, dw_flags,
    );
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

    /// `cmsDoTransformStride` — n pixels, byte-stride
    pub fn do_transform_stride(&mut self, input: &[u8], output: &mut [u8], n: u32, stride_b: u32) {
        let stride = Stride {
            bytes_per_line_in: 0,
            bytes_per_line_out: 0,
            bytes_per_plane_in: stride_b,
            bytes_per_plane_out: stride_b,
        };
        self.xform(input, output, n, 1, &stride);
    }

    /// `cmsDoTransformLineStride` — ppl × lines with per-plane line strides
    pub fn do_transform_line_stride(
        &mut self,
        input: &[u8],
        output: &mut [u8],
        ppl: u32,
        lines: u32,
        bpl_in: u32,
        bpl_out: u32,
        bpp_in: u32,
        bpp_out: u32,
    ) {
        let stride = Stride {
            bytes_per_line_in: bpl_in,
            bytes_per_line_out: bpl_out,
            bytes_per_plane_in: bpp_in,
            bytes_per_plane_out: bpp_out,
        };
        self.xform(input, output, ppl, lines, &stride);
    }

    /// Clone the eval state for a parallel worker — everything the eval
    /// loop reads (`lut`/`opt_data`/`fmt`/`worker`/colorants) minus the
    /// non-Cloneable `user_data` and a fresh per-thread pixel cache.
    fn clone_for_parallel(&self) -> Transform {
        Transform {
            input_format: self.input_format,
            output_format: self.output_format,
            lut: self.lut.clone(),
            gamut_check: self.gamut_check.clone(),
            alarm_codes: self.alarm_codes,
            ctx: self.ctx.clone(),
            stride: self.stride,
            fmt: self.fmt,
            worker: self.worker,
            cache_in: [0u16; MAX_CHANNELS],
            cache_out: [0u16; MAX_CHANNELS],
            entry_color_space: self.entry_color_space,
            exit_color_space: self.exit_color_space,
            rendering_intent: self.rendering_intent,
            dw_flags: self.dw_flags,
            entry_white_point: self.entry_white_point,
            exit_white_point: self.exit_white_point,
            user_data: None,
            foreign: self.foreign,
            input_colorants: self.input_colorants.clone(),
            output_colorants: self.output_colorants.clone(),
            _pad: (),
        }
    }

    /// Multithreaded `cmsDoTransform` — splits the contiguous pixel range
    /// into `threads` chunks, each processed on its own thread with a fresh
    /// pixel cache. The eval state is `Sync` (read-only `lut`/`opt_data`/
    /// `fmt`/`worker`), so this is safe and byte-exact vs `do_transform`.
    /// `threads <= 1` or `n < threads` falls back to the serial path.
    pub fn do_transform_par(&mut self, input: &[u8], output: &mut [u8], n: u32, threads: usize) {
        let px_in = (crate::pack::t_channels(self.input_format)
            + crate::pack::t_extra(self.input_format)) as usize
            * pixel_size(self.input_format) as usize;
        let px_out = (crate::pack::t_channels(self.output_format)
            + crate::pack::t_extra(self.output_format)) as usize
            * pixel_size(self.output_format) as usize;
        if px_in == 0 || px_out == 0 {
            return;
        }
        let total = n as usize;
        let threads = threads.clamp(1, total.max(1));
        if threads <= 1 || total < 2 {
            self.do_transform(input, output, n);
            return;
        }
        let chunk = total.div_ceil(threads);
        let mut rest_in = input;
        let mut rest_out = output;
        std::thread::scope(|s| {
            let mut off = 0usize;
            while off < total {
                let take = chunk.min(total - off);
                let (ic, ri) = rest_in.split_at((take * px_in).min(rest_in.len()));
                let (oc, ro) = rest_out.split_at_mut((take * px_out).min(rest_out.len()));
                rest_in = ri;
                rest_out = ro;
                let npix = (ic.len() / px_in).min(oc.len() / px_out) as u32;
                let mut t = self.clone_for_parallel();
                s.spawn(move || t.do_transform(ic, oc, npix));
                off += take;
            }
        });
    }

    /// `xform` dispatch — worker chosen at create time
    fn xform(&mut self, input: &[u8], output: &mut [u8], ppl: u32, lines: u32, stride: &Stride) {
        handle_extra_channels(
            input,
            output,
            ppl,
            lines,
            stride,
            self.input_format,
            self.output_format,
            self.dw_flags,
            self.ctx.as_ref(),
        );
        match self.worker {
            Worker::Null => self.null_xform(input, output, ppl, lines, stride),
            Worker::NullFloat => self.null_float_xform(input, output, ppl, lines, stride),
            Worker::Precalculated => self.xform16(input, output, ppl, lines, stride, false, false),
            Worker::PrecalculatedGamutCheck => {
                self.xform16(input, output, ppl, lines, stride, true, false)
            }
            Worker::Cached => self.xform16(input, output, ppl, lines, stride, false, true),
            Worker::CachedGamutCheck => self.xform16(input, output, ppl, lines, stride, true, true),
            Worker::Float => self.xform_f32(input, output, ppl, lines, stride, false),
            Worker::Noop => {}
            Worker::Foreign => {
                if let Some(invoke) = *crate::context::XFORM_HOOKS.read().unwrap() {
                    invoke(
                        self as *const Transform as usize,
                        input.as_ptr(),
                        output.as_mut_ptr(),
                        ppl,
                        lines,
                        stride.bytes_per_line_in,
                        stride.bytes_per_line_out,
                        stride.bytes_per_plane_in,
                        stride.bytes_per_plane_out,
                    );
                }
            }
        }
    }

    fn null_xform(
        &mut self,
        input: &[u8],
        output: &mut [u8],
        ppl: u32,
        lines: u32,
        stride: &Stride,
    ) {
        let info = PackInfo {
            input_format: self.input_format,
            output_format: self.output_format,
            stride: 0,
        };
        let (unroll, pack) = match self.fmt {
            Fmt::F16 { unroll, pack } => (unroll, pack),
            _ => return,
        };
        let mut w_in = [0u16; MAX_CHANNELS];
        let mut i_off = 0usize;
        let mut o_off = 0usize;
        for _ in 0..lines {
            let mut accum = i_off;
            let mut out_pos = o_off;
            for _ in 0..ppl {
                accum += unroll(&info, &mut w_in, &input[accum..], stride.bytes_per_plane_in);
                out_pos += pack(
                    &info,
                    &w_in,
                    &mut output[out_pos..],
                    stride.bytes_per_plane_out,
                );
            }
            i_off += stride.bytes_per_line_in as usize;
            o_off += stride.bytes_per_line_out as usize;
        }
    }

    fn null_float_xform(
        &mut self,
        input: &[u8],
        output: &mut [u8],
        ppl: u32,
        lines: u32,
        stride: &Stride,
    ) {
        let info = PackInfo {
            input_format: self.input_format,
            output_format: self.output_format,
            stride: 0,
        };
        let (unroll, pack) = match self.fmt {
            Fmt::F32 { unroll, pack } => (unroll, pack),
            _ => return,
        };
        let mut f_in = [0f32; MAX_CHANNELS];
        let mut i_off = 0usize;
        let mut o_off = 0usize;
        for _ in 0..lines {
            let mut accum = i_off;
            let mut out_pos = o_off;
            for _ in 0..ppl {
                accum += unroll(&info, &mut f_in, &input[accum..], stride.bytes_per_plane_in);
                out_pos += pack(
                    &info,
                    &f_in,
                    &mut output[out_pos..],
                    stride.bytes_per_plane_out,
                );
            }
            i_off += stride.bytes_per_line_in as usize;
            o_off += stride.bytes_per_line_out as usize;
        }
    }

    /// `PrecalculatedXFORM`/`CachedXFORM` (+ gamut variants)
    fn xform16(
        &mut self,
        input: &[u8],
        output: &mut [u8],
        ppl: u32,
        lines: u32,
        stride: &Stride,
        gamut: bool,
        cache: bool,
    ) {
        // Fused fast paths — same math as the generic unroll→eval→pack
        // chain but with the whole pixel transform inlined.
        const RGB8: u32 = 4 << 16 | 3 << 3 | 1;
        if !cache && !gamut {
            // Identity + identical non-planar formats: unroll→copy→pack is a
            // byte-exact round trip, so the whole transform is a line copy.
            if self.input_format == self.output_format
                && crate::pack::t_planar(self.input_format) == 0
            {
                if let Some(l) = self.lut.as_mut() {
                    if let Some(crate::opt::OptData::Identity) = l.opt_data.as_ref() {
                        let pplb = ppl as usize
                            * (crate::pack::t_channels(self.input_format)
                                + crate::pack::t_extra(self.input_format))
                                as usize
                            * pixel_size(self.input_format) as usize;
                        let mut i_off = 0usize;
                        let mut o_off = 0usize;
                        for _ in 0..lines {
                            output[o_off..o_off + pplb]
                                .copy_from_slice(&input[i_off..i_off + pplb]);
                            i_off += stride.bytes_per_line_in as usize;
                            o_off += stride.bytes_per_line_out as usize;
                        }
                        return;
                    }
                }
            }
            if self.input_format == RGB8 && self.output_format == RGB8 {
                if let Some(l) = self.lut.as_mut() {
                    if let Some(crate::opt::OptData::MatShaper(p)) = l.opt_data.as_ref() {
                        let ppl3 = ppl as usize * 3;
                        let mut i_off = 0usize;
                        let mut o_off = 0usize;
                        for _ in 0..lines {
                            let iline = &input[i_off..i_off + ppl3];
                            let oline = &mut output[o_off..o_off + ppl3];
                            for i in 0..ppl as usize {
                                let px = i * 3;
                                let r = p.shaper1[0][iline[px] as usize];
                                let g = p.shaper1[1][iline[px + 1] as usize];
                                let b = p.shaper1[2][iline[px + 2] as usize];
                                let l1 = (p.mat[0][0].wrapping_mul(r)
                                    + p.mat[0][1].wrapping_mul(g)
                                    + p.mat[0][2].wrapping_mul(b)
                                    + p.off[0]
                                    + 0x2000)
                                    >> 14;
                                let l2 = (p.mat[1][0].wrapping_mul(r)
                                    + p.mat[1][1].wrapping_mul(g)
                                    + p.mat[1][2].wrapping_mul(b)
                                    + p.off[1]
                                    + 0x2000)
                                    >> 14;
                                let l3 = (p.mat[2][0].wrapping_mul(r)
                                    + p.mat[2][1].wrapping_mul(g)
                                    + p.mat[2][2].wrapping_mul(b)
                                    + p.off[2]
                                    + 0x2000)
                                    >> 14;
                                oline[px] =
                                    crate::pack::f16to8(p.shaper2[l1.clamp(0, 16384) as usize]);
                                oline[px + 1] = crate::pack::f16to8(
                                    p.shaper2[16385 + l2.clamp(0, 16384) as usize],
                                );
                                oline[px + 2] = crate::pack::f16to8(
                                    p.shaper2[2 * 16385 + l3.clamp(0, 16384) as usize],
                                );
                            }
                            i_off += stride.bytes_per_line_in as usize;
                            o_off += stride.bytes_per_line_out as usize;
                        }
                        return;
                    }
                }
            }
        }
        let info = PackInfo {
            input_format: self.input_format,
            output_format: self.output_format,
            stride: 0,
        };
        let (unroll, pack) = match self.fmt {
            Fmt::F16 { unroll, pack } => (unroll, pack),
            _ => return,
        };
        if !gamut {
            if let Some(l) = self.lut.as_mut() {
                if let Some(od) = l.opt_data.as_ref() {
                    let ci = &mut self.cache_in;
                    let co = &mut self.cache_out;
                    match od {
                        crate::opt::OptData::ClutOnly(c) => {
                            use crate::intrp::{self as ip, InterpFn16};
                            let f = match c.params16.interpolation {
                                ip::Interpolation::Lerp16(f) => f,
                                _ => unreachable!("clut16 on float params"),
                            };
                            macro_rules! run {
                                ($k:expr) => {
                                    return xform16_eval_loop(
                                        unroll,
                                        pack,
                                        &info,
                                        input,
                                        output,
                                        ppl,
                                        lines,
                                        stride,
                                        cache,
                                        ci,
                                        co,
                                        |i, o| $k(i, o, &c.params16),
                                    )
                                };
                            }
                            // Concrete-kernel dispatch — the fn ptr resolves
                            // once so the per-pixel call inlines fully.
                            if f == ip::eval4_inputs as InterpFn16 {
                                run!(ip::eval4_inputs)
                            } else if f == ip::tetrahedral_interp16 as InterpFn16 {
                                run!(ip::tetrahedral_interp16)
                            } else if f == ip::trilinear_interp16 as InterpFn16 {
                                run!(ip::trilinear_interp16)
                            } else if f == ip::bilinear_interp16 as InterpFn16 {
                                run!(ip::bilinear_interp16)
                            } else if f == ip::lin_lerp_1d as InterpFn16 {
                                run!(ip::lin_lerp_1d)
                            } else if f == ip::eval1_input as InterpFn16 {
                                run!(ip::eval1_input)
                            } else {
                                return xform16_eval_loop(
                                    unroll,
                                    pack,
                                    &info,
                                    input,
                                    output,
                                    ppl,
                                    lines,
                                    stride,
                                    cache,
                                    ci,
                                    co,
                                    |i, o| f(i, o, &c.params16),
                                );
                            }
                        }
                        crate::opt::OptData::MatShaper(_) => {
                            return xform16_eval_loop(
                                unroll,
                                pack,
                                &info,
                                input,
                                output,
                                ppl,
                                lines,
                                stride,
                                cache,
                                ci,
                                co,
                                |i, o| {
                                    crate::opt::mat_shaper_eval16(i, o, od);
                                },
                            );
                        }
                        crate::opt::OptData::Curves16(_) => {
                            return xform16_eval_loop(
                                unroll,
                                pack,
                                &info,
                                input,
                                output,
                                ppl,
                                lines,
                                stride,
                                cache,
                                ci,
                                co,
                                |i, o| {
                                    crate::opt::fast_eval_curves16(i, o, od);
                                },
                            );
                        }
                        crate::opt::OptData::Prelin16(_) => {
                            return xform16_eval_loop(
                                unroll,
                                pack,
                                &info,
                                input,
                                output,
                                ppl,
                                lines,
                                stride,
                                cache,
                                ci,
                                co,
                                |i, o| {
                                    crate::opt::prelin_eval16(i, o, od);
                                },
                            );
                        }
                        crate::opt::OptData::Prelin8(_) => {
                            return xform16_eval_loop(
                                unroll,
                                pack,
                                &info,
                                input,
                                output,
                                ppl,
                                lines,
                                stride,
                                cache,
                                ci,
                                co,
                                |i, o| {
                                    crate::opt::prelin_eval8(i, o, od);
                                },
                            );
                        }
                        crate::opt::OptData::Identity => {
                            return xform16_eval_loop(
                                unroll,
                                pack,
                                &info,
                                input,
                                output,
                                ppl,
                                lines,
                                stride,
                                cache,
                                ci,
                                co,
                                |i, o| {
                                    crate::opt::fast_identity16(i, o, od);
                                },
                            );
                        }
                    }
                }
            }
        }
        let mut w_in = [0u16; MAX_CHANNELS];
        let mut w_out = [0u16; MAX_CHANNELS];
        let mut cache_in = self.cache_in;
        let mut cache_out = self.cache_out;
        let mut i_off = 0usize;
        let mut o_off = 0usize;
        for _ in 0..lines {
            let mut accum = i_off;
            let mut out_pos = o_off;
            for _ in 0..ppl {
                accum += unroll(&info, &mut w_in, &input[accum..], stride.bytes_per_plane_in);
                if cache && w_in[..MAX_CHANNELS] == cache_in[..MAX_CHANNELS] {
                    w_out[..MAX_CHANNELS].copy_from_slice(&cache_out[..MAX_CHANNELS]);
                } else {
                    if gamut {
                        if let Some(g) = &mut self.gamut_check {
                            let mut g_out = [0u16; MAX_CHANNELS];
                            g.eval16(&w_in, &mut g_out);
                            if g_out[0] > 0 {
                                w_out[..MAX_CHANNELS].copy_from_slice(&self.alarm_codes);
                            } else if let Some(l) = &mut self.lut {
                                l.eval16(&w_in, &mut w_out);
                            }
                        }
                    } else if let Some(l) = &mut self.lut {
                        l.eval16(&w_in, &mut w_out);
                    }
                    if cache {
                        cache_in[..MAX_CHANNELS].copy_from_slice(&w_in[..MAX_CHANNELS]);
                        cache_out[..MAX_CHANNELS].copy_from_slice(&w_out[..MAX_CHANNELS]);
                    }
                }
                out_pos += pack(
                    &info,
                    &w_out,
                    &mut output[out_pos..],
                    stride.bytes_per_plane_out,
                );
            }
            i_off += stride.bytes_per_line_in as usize;
            o_off += stride.bytes_per_line_out as usize;
        }
    }

    /// `FloatXFORM` — the 32-bit float eval loop (+ gamut)
    fn xform_f32(
        &mut self,
        input: &[u8],
        output: &mut [u8],
        ppl: u32,
        lines: u32,
        stride: &Stride,
        gamut: bool,
    ) {
        let info = PackInfo {
            input_format: self.input_format,
            output_format: self.output_format,
            stride: 0,
        };
        let (unroll, pack) = match self.fmt {
            Fmt::F32 { unroll, pack } => (unroll, pack),
            _ => return,
        };
        let mut f_in = [0f32; MAX_CHANNELS];
        let mut f_out = [0f32; MAX_CHANNELS];
        let mut i_off = 0usize;
        let mut o_off = 0usize;
        for _ in 0..lines {
            let mut accum = i_off;
            let mut out_pos = o_off;
            for _ in 0..ppl {
                accum += unroll(&info, &mut f_in, &input[accum..], stride.bytes_per_plane_in);
                if gamut {
                    if let Some(g) = &mut self.gamut_check {
                        let mut g_out = [0f32; 1];
                        g.eval_float(&f_in, &mut g_out);
                        if g_out[0] > 0.0 {
                            for c in 0..MAX_CHANNELS {
                                f_out[c] = self.alarm_codes[c] as f32 / 65535.0;
                            }
                        } else if let Some(l) = &mut self.lut {
                            l.eval_float(&f_in, &mut f_out);
                        }
                    }
                } else if let Some(l) = &mut self.lut {
                    l.eval_float(&f_in, &mut f_out);
                }
                out_pos += pack(
                    &info,
                    &f_out,
                    &mut output[out_pos..],
                    stride.bytes_per_plane_out,
                );
            }
            i_off += stride.bytes_per_line_in as usize;
            o_off += stride.bytes_per_line_out as usize;
        }
    }

    /// `cmsChangeBuffersFormat` — swap input/output formatters in place
    pub fn change_buffers_format(&mut self, input_format: u32, output_format: u32) -> bool {
        if self.ctx.is_none() && self.user_data.is_none() {
            // dwFlags CAN_CHANGE_FORMATTER check happens at create; here just swap
        }
        // C's `IsFloat` = `T_FLOAT(in) || T_FLOAT(out)` — bit 22 only. DBL
        // formats set FLOAT_SH, so `t_bytes==0` is redundant and wrongly
        // routes BYTES_SH(0) 16-bit formats (e.g. TYPE_RGB_565) to float.
        let is_float = crate::pack::t_float(input_format) != 0
            || crate::pack::t_float(output_format) != 0;
        let flags = if is_float {
            CMS_PACK_FLAGS_FLOAT
        } else {
            CMS_PACK_FLAGS_16BITS
        };
        let dir_in = get_formatter_ctx(self.ctx.as_ref(), input_format, FormatterDir::Input, flags);
        let dir_out = get_formatter_ctx(self.ctx.as_ref(), output_format, FormatterDir::Output, flags);
        let fmt = match (is_float, dir_in, dir_out) {
            (true, Formatter::UnpackF32(u), Formatter::PackF32(p)) => {
                Fmt::F32 { unroll: u, pack: p }
            }
            (false, Formatter::Unpack16(u), Formatter::Pack16(p)) => {
                Fmt::F16 { unroll: u, pack: p }
            }
            _ => return false,
        };
        self.input_format = input_format;
        self.output_format = output_format;
        self.fmt = fmt;
        // rebind worker for float/16-bit switch
        self.worker = if is_float {
            if self.worker == Worker::NullFloat {
                Worker::NullFloat
            } else {
                Worker::Float
            }
        } else {
            match self.worker {
                Worker::Null => Worker::Null,
                _ => Worker::Precalculated,
            }
        };
        true
    }
}

/// `cmsSetAdaptationState` — stored per-context; affects absolute-intent math.
/// C treats `d < 0` as a read (`cmsplugin.c`: `if (d >= 0) *ptr = d`).
pub fn set_adaptation_state(ctx: Option<&Context>, d: f64) -> f64 {
    let c = crate::context::or_global(ctx);
    if d >= 0.0 {
        f64::from_bits(
            c.adaptation_state
                .swap(d.to_bits(), std::sync::atomic::Ordering::Relaxed),
        )
    } else {
        get_adaptation_state(ctx)
    }
}

/// `cmsGetAdaptationState`
pub fn get_adaptation_state(ctx: Option<&Context>) -> f64 {
    f64::from_bits(
        crate::context::or_global(ctx)
            .adaptation_state
            .load(std::sync::atomic::Ordering::Relaxed),
    )
}

/// `cmsSetAlarmCodes`/`cmsGetAlarmCodes` — per-context alarm values
pub fn set_alarm_codes(ctx: Option<&Context>, codes: &[u16; MAX_CHANNELS]) {
    *crate::context::or_global(ctx).alarm_codes.lock().unwrap() = *codes;
}

/// `cmsGetAlarmCodes`
pub fn get_alarm_codes(ctx: Option<&Context>) -> [u16; MAX_CHANNELS] {
    *crate::context::or_global(ctx).alarm_codes.lock().unwrap()
}

/// `AllocEmptyTransform` — bind lut + formatters, pick the worker
fn alloc_empty_transform(
    ctx: Option<Context>,
    lut: Option<Pipeline>,
    intent: u32,
    input_format: u32,
    output_format: u32,
    dw_flags: u32,
) -> Option<Box<Transform>> {
    let mut flags = dw_flags;
    // transform-plugin hook — no plugin chunk yet, so stock only
    // C's `IsFloat` = `T_FLOAT(in) || T_FLOAT(out)` — bit 22 only (see
    // `change_buffers_format`); `t_bytes==0` wrongly routes BYTES_SH(0)
    // 16-bit formats (TYPE_RGB_565) to the float formatter path.
    let is_float = crate::pack::t_float(input_format) != 0
        || crate::pack::t_float(output_format) != 0;
    let fmt: Fmt;
    let worker: Worker;
    if is_float {
        let dir_in = get_formatter_ctx(ctx.as_ref(), input_format, FormatterDir::Input, CMS_PACK_FLAGS_FLOAT);
        let dir_out = get_formatter_ctx(ctx.as_ref(), output_format, FormatterDir::Output, CMS_PACK_FLAGS_FLOAT);
        fmt = match (dir_in, dir_out) {
            (Formatter::UnpackF32(u), Formatter::PackF32(p)) => Fmt::F32 { unroll: u, pack: p },
            _ => {
                crate::err::signal_error(
                    ctx.as_ref(),
                    crate::types::ERROR_UNKNOWN_EXTENSION,
                    "Unsupported raster format",
                );
                return None;
            }
        };
        worker = if flags & FLAGS_NULLTRANSFORM != 0 {
            Worker::NullFloat
        } else {
            Worker::Float
        };
    } else {
        if input_format == 0 && output_format == 0 {
            fmt = Fmt::Noop;
            flags |= FLAGS_CAN_CHANGE_FORMATTER;
        } else {
            let dir_in = get_formatter_ctx(ctx.as_ref(), input_format, FormatterDir::Input, CMS_PACK_FLAGS_16BITS);
            let dir_out = get_formatter_ctx(ctx.as_ref(), output_format, FormatterDir::Output, CMS_PACK_FLAGS_16BITS);
            fmt = match (dir_in, dir_out) {
                (Formatter::Unpack16(u), Formatter::Pack16(p)) => Fmt::F16 { unroll: u, pack: p },
                _ => {
                    crate::err::signal_error(
                        ctx.as_ref(),
                        crate::types::ERROR_UNKNOWN_EXTENSION,
                        "Unsupported raster format",
                    );
                    return None;
                }
            };
            if crate::pack::t_bytes(input_format) == 0 || crate::pack::t_bytes(input_format) >= 2 {
                flags |= FLAGS_CAN_CHANGE_FORMATTER;
            }
        }
        worker = if flags & FLAGS_NULLTRANSFORM != 0 {
            Worker::Null
        } else if flags & FLAGS_NOCACHE != 0 {
            if flags & FLAGS_GAMUTCHECK != 0 {
                Worker::PrecalculatedGamutCheck
            } else {
                Worker::Precalculated
            }
        } else if flags & FLAGS_GAMUTCHECK != 0 {
            Worker::CachedGamutCheck
        } else {
            Worker::Cached
        };
    }
    // alpha-channel consistency
    if flags & FLAGS_COPY_ALPHA != 0
        && crate::pack::t_extra(input_format) != crate::pack::t_extra(output_format)
    {
        crate::err::signal_error(
            ctx.as_ref(),
            crate::types::ERROR_NOT_SUITABLE,
            "Mismatched alpha channels",
        );
        return None;
    }
    let mut t = Box::new(Transform {
        input_format,
        output_format,
        lut,
        gamut_check: None,
        alarm_codes: get_alarm_codes(ctx.as_ref()),
        ctx,
        stride: Stride::default(),
        fmt,
        worker,
        cache_in: [0; MAX_CHANNELS],
        cache_out: [0; MAX_CHANNELS],
        entry_color_space: 0,
        exit_color_space: 0,
        rendering_intent: intent,
        dw_flags: flags,
        entry_white_point: CieXyz {
            X: 0.0,
            Y: 0.0,
            Z: 0.0,
        },
        exit_white_point: CieXyz {
            X: 0.0,
            Y: 0.0,
            Z: 0.0,
        },
        user_data: None,
        foreign: None,
        input_colorants: None,
        output_colorants: None,
        _pad: (),
    });
    // seed the 16-bit cache (transform of all-zeros)
    if flags & FLAGS_NOCACHE == 0 {
        if let Some(l) = &mut t.lut {
            if t.gamut_check.is_some() {
                let mut g_out = [0u16; MAX_CHANNELS];
                let mut z_in = [0u16; MAX_CHANNELS];
                let mut z_out = [0u16; MAX_CHANNELS];
                if let Some(g) = &mut t.gamut_check {
                    g.eval16(&z_in, &mut g_out);
                    if g_out[0] > 0 {
                        z_out.copy_from_slice(&t.alarm_codes);
                    } else {
                        l.eval16(&z_in, &mut z_out);
                    }
                }
                t.cache_in = z_in;
                t.cache_out = z_out;
            } else {
                let mut z_out = [0u16; MAX_CHANNELS];
                l.eval16(&t.cache_in, &mut z_out);
                t.cache_out = z_out;
            }
        }
    }
    Some(t)
}

/// `cmsDetectRGBProfileGamma` — estimate display gamma from the TRCs.
/// C creates an sRGB→Lab transform and fits; we use the curve's own fit.
#[doc(hidden)]
pub fn dbg_detect_rgb_profile_gamma(icc: &mut IccProfile, threshold: f64) -> f64 {
    detect_rgb_profile_gamma(icc, threshold)
}
fn detect_rgb_profile_gamma(icc: &mut IccProfile, threshold: f64) -> f64 {
    use crate::signatures::tag as tg;
    use crate::tagtypes::TagObject;
    let mut sum = 0.0;
    let mut n = 0usize;
    for sig in [tg::SigRedTRCTag, tg::SigGreenTRCTag, tg::SigBlueTRCTag] {
        if let Some(TagObject::Curve(c)) | Some(TagObject::ParametricCurve(c)) =
            crate::profile::cms_read_tag(icc, sig)
        {
            let e = crate::gamma::estimate_gamma(&c, threshold);
            if e > 0.0 {
                sum += e;
                n += 1;
            }
        }
    }
    if n == 0 { 0.0 } else { sum / n as f64 }
}

/// `cmsCreateExtendedTransform` — link + colorspace checks + alloc
pub fn create_extended_transform(
    ctx: Option<Context>,
    profiles: &mut [Box<IccProfile>],
    bpc: &mut [bool],
    intents: &[u32],
    adaptation_states: &[f64],
    mut gamut_profile: Option<&mut IccProfile>,
    gamut_pcs_position: u32,
    input_format: u32,
    output_format: u32,
    dw_flags: u32,
) -> Option<Box<Transform>> {
    let mut flags = dw_flags;
    // C checks `cmsFLAGS_NULLTRANSFORM` before validating the profile array —
    // a null transform needs no profiles at all.
    if flags & FLAGS_NULLTRANSFORM != 0 {
        return alloc_empty_transform(
            ctx,
            None,
            INTENT_PERCEPTUAL,
            input_format,
            output_format,
            flags,
        );
    }
    let n = profiles.len();
    if n == 0 || n > 255 {
        crate::err::signal_error(
            ctx.as_ref(),
            crate::types::ERROR_RANGE,
            "Wrong number of profiles",
        );
        return None;
    }
    let last_intent = *intents.last()?;
    let has_gamut = flags & FLAGS_GAMUTCHECK != 0 && gamut_profile.is_some();
    if !has_gamut {
        flags &= !FLAGS_GAMUTCHECK;
    }
    if flags & FLAGS_GAMUTCHECK != 0
        && (gamut_pcs_position == 0 || gamut_pcs_position as usize > n - 1)
    {
        crate::err::signal_error(
            ctx.as_ref(),
            crate::types::ERROR_RANGE,
            "Wrong gamut PCS position",
        );
        return None;
    }
    if crate::pack::t_float(input_format) != 0 || crate::pack::t_float(output_format) != 0 {
        flags |= FLAGS_NOCACHE;
    }
    let (entry, exit) = get_xform_color_spaces(profiles)?;
    if !is_proper_color_space(entry, input_format) {
        crate::err::signal_error(
            ctx.as_ref(),
            crate::types::ERROR_COLORSPACE_CHECK,
            "Wrong input color space",
        );
        return None;
    }
    if !is_proper_color_space(exit, output_format) {
        crate::err::signal_error(
            ctx.as_ref(),
            crate::types::ERROR_COLORSPACE_CHECK,
            "Wrong output color space",
        );
        return None;
    }
    // gamma-detect: low-gamma 16-bit RGB input disables optimizer
    if entry == crate::signatures::colorspace::SigRgbData
        && crate::pack::t_bytes(input_format) == 2
        && flags & FLAGS_NOOPTIMIZE == 0
    {
        let g = detect_rgb_profile_gamma(&mut profiles[0], 0.1);
        if g > 0.0 && g < 1.6 {
            flags |= FLAGS_NOOPTIMIZE;
        }
    }
    let mut lut = cnvrt::link_profiles(
        ctx.clone(),
        profiles,
        intents,
        bpc,
        adaptation_states,
        flags,
    )?;
    // channel-consistency check
    if crate::pcs::channels_of_color_space(entry) != lut.input_channels as i32
        || crate::pcs::channels_of_color_space(exit) != lut.output_channels as i32
    {
        crate::err::signal_error(
            ctx.as_ref(),
            crate::types::ERROR_NOT_SUITABLE,
            "Channel count doesn't match",
        );
        return None;
    }
    // `_cmsOptimizePipeline` runs unconditionally in C's `AllocEmptyTransform`:
    // `PreOptimize` strips identity/inverse pairs even under NOOPTIMIZE.
    {
        let mut infmt = input_format;
        let mut outfmt = output_format;
        crate::opt::optimize_pipeline(
            ctx.as_ref(),
            &mut lut,
            last_intent,
            &mut infmt,
            &mut outfmt,
            &mut flags,
        );
        // optimizer may rewrite the effective formats — keep ours in sync
        let _ = (infmt, outfmt);
    }
    let mut t = alloc_empty_transform(
        ctx,
        Some(lut),
        last_intent,
        input_format,
        output_format,
        flags,
    )?;
    t.entry_color_space = entry;
    t.exit_color_space = exit;
    t.rendering_intent = last_intent;
    // `_cmsCreateGamutCheckPipeline` — the proof chain → a 3→1 CLUT, built from
    // the profile array + the gamut (proofing) profile (cmsxform.c:1227).
    if flags & FLAGS_GAMUTCHECK != 0 {
        if let Some(gp) = gamut_profile {
            t.gamut_check = crate::sm::create_gamut_check_pipeline(
                t.ctx.as_ref(),
                profiles,
                bpc,
                intents,
                adaptation_states,
                gamut_pcs_position,
                gp,
            );
        }
    }
    // white points — media-white tag else D50
    let wp_in = crate::io1::read_media_white_point(&mut profiles[0]);
    let wp_out = crate::io1::read_media_white_point(&mut profiles[n - 1]);
    t.entry_white_point = set_white_point(Some(&wp_in));
    t.exit_white_point = set_white_point(Some(&wp_out));
    // Try to read input and output colorant table (cmsxform.c:1241-1264)
    if crate::profile::cms_is_tag(&profiles[0], crate::signatures::tag::SigColorantTableTag) {
        if let Some(crate::tagtypes::TagObject::ColorantTable(list)) = crate::profile::cms_read_tag(
            profiles[0].as_mut(),
            crate::signatures::tag::SigColorantTableTag,
        ) {
            t.input_colorants = crate::named::dup_named_color_list(list);
        }
    }
    // Output is a little bit more complex.
    if profiles[n - 1].device_class == crate::signatures::profileclass::SigLinkClass {
        // This tag may exist only on devicelink profiles.
        if crate::profile::cms_is_tag(
            &profiles[n - 1],
            crate::signatures::tag::SigColorantTableOutTag,
        ) {
            if let Some(crate::tagtypes::TagObject::ColorantTable(list)) =
                crate::profile::cms_read_tag(
                    profiles[n - 1].as_mut(),
                    crate::signatures::tag::SigColorantTableOutTag,
                )
            {
                t.output_colorants = crate::named::dup_named_color_list(list);
            }
        }
    } else if crate::profile::cms_is_tag(
        &profiles[n - 1],
        crate::signatures::tag::SigColorantTableTag,
    ) {
        if let Some(crate::tagtypes::TagObject::ColorantTable(list)) = crate::profile::cms_read_tag(
            profiles[n - 1].as_mut(),
            crate::signatures::tag::SigColorantTableTag,
        ) {
            t.output_colorants = crate::named::dup_named_color_list(list);
        }
    }
    Some(t)
}

/// `cmsGetTransformInputColorants` — input colorant table (borrowed)
pub fn get_transform_input_colorants(xform: &Transform) -> Option<&crate::named::NamedColorList> {
    xform.input_colorants.as_ref()
}

/// `cmsGetTransformOutputColorants` — output colorant table (borrowed)
pub fn get_transform_output_colorants(xform: &Transform) -> Option<&crate::named::NamedColorList> {
    xform.output_colorants.as_ref()
}

/// `cmsGetNamedColorList` — the named-color list of a named-color transform.
/// Should be the first element in the LUT.
pub fn get_named_color_list(xform: &Transform) -> Option<&crate::named::NamedColorList> {
    let mpe = xform.lut.as_ref()?.elements.as_ref()?;
    if let crate::lut::StageData::NamedColor(list) = &mpe.data {
        Some(list)
    } else {
        None
    }
}
