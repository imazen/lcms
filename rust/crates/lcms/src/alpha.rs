//! `cmsalpha.c` — the alpha/extra-channel copy machinery. The C version
//! converts each extra-channel value through a 6×6 type matrix keyed on
//! the pixel-format's byte width and float flag.

use crate::context::Context;
use crate::pack::{
    t_bytes, t_channels, t_doswap, t_endian16, t_extra, t_float, t_planar, t_swapfirst,
};
use crate::xform::Stride;

/// `trueBytesSize` — bytes per channel; float formats report raw bytes
fn true_bytes_size(fmt: u32) -> u32 {
    let b = t_bytes(fmt);
    if b == 0 { 8 } else { b }
}

/// `cmsFormatterAlphaFn` — a dst←src alpha-value converter
pub type FormatterAlphaFn = fn(&mut [u8], &[u8]);

// --- individual converters -----------------------------------------------
fn copy8(d: &mut [u8], s: &[u8]) {
    d[..1].copy_from_slice(&s[..1]);
}
fn from8to16(d: &mut [u8], s: &[u8]) {
    let v = (s[0] as u16) * 257;
    d[..2].copy_from_slice(&v.to_le_bytes());
}
fn from8to16se(d: &mut [u8], s: &[u8]) {
    let v = (s[0] as u16) * 257;
    d[..2].copy_from_slice(&v.to_be_bytes());
}
fn from8tohlf(d: &mut [u8], s: &[u8]) {
    let f = s[0] as f32 / 255.0;
    d[..2].copy_from_slice(&crate::half::float2half(f).to_le_bytes());
}
fn from8toflt(d: &mut [u8], s: &[u8]) {
    d[..4].copy_from_slice(&(s[0] as f32 / 255.0).to_le_bytes());
}
fn from8todbl(d: &mut [u8], s: &[u8]) {
    d[..8].copy_from_slice(&(s[0] as f64 / 255.0).to_le_bytes());
}
fn from16to8(d: &mut [u8], s: &[u8]) {
    let v = u16::from_le_bytes(s[..2].try_into().unwrap());
    d[0] = (((v as u32) * 65281 + 8388608) >> 24) as u8;
}
fn from16seto8(d: &mut [u8], s: &[u8]) {
    let v = u16::from_be_bytes(s[..2].try_into().unwrap());
    d[0] = (((v as u32) * 65281 + 8388608) >> 24) as u8;
}
fn copy16(d: &mut [u8], s: &[u8]) {
    d[..2].copy_from_slice(&s[..2]);
}
fn from16to16(d: &mut [u8], s: &[u8]) {
    // C: n = *(u16*)src; *(u16*)dst = CHANGE_ENDIAN(n) — a byte swap,
    // used for both 16→16SE and 16SE→16 (and it is the swap either way).
    d[..2].copy_from_slice(&[s[1], s[0]]);
}
fn from16toflt(d: &mut [u8], s: &[u8]) {
    let v = u16::from_le_bytes(s[..2].try_into().unwrap());
    d[..4].copy_from_slice(&(v as f32 / 65535.0).to_le_bytes());
}
fn from16setoflt(d: &mut [u8], s: &[u8]) {
    let v = u16::from_be_bytes(s[..2].try_into().unwrap());
    d[..4].copy_from_slice(&(v as f32 / 65535.0).to_le_bytes());
}
fn from16todbl(d: &mut [u8], s: &[u8]) {
    let v = u16::from_le_bytes(s[..2].try_into().unwrap());
    d[..8].copy_from_slice(&(v as f64 / 65535.0).to_le_bytes());
}
fn from16setodbl(d: &mut [u8], s: &[u8]) {
    let v = u16::from_be_bytes(s[..2].try_into().unwrap());
    d[..8].copy_from_slice(&(v as f64 / 65535.0).to_le_bytes());
}
fn from16tohlf(d: &mut [u8], s: &[u8]) {
    let v = u16::from_le_bytes(s[..2].try_into().unwrap());
    d[..2].copy_from_slice(&crate::half::float2half(v as f32 / 65535.0).to_le_bytes());
}
fn from16setohlf(d: &mut [u8], s: &[u8]) {
    let v = u16::from_be_bytes(s[..2].try_into().unwrap());
    d[..2].copy_from_slice(&crate::half::float2half(v as f32 / 65535.0).to_le_bytes());
}
fn fromfltto8(d: &mut [u8], s: &[u8]) {
    let n = f32::from_le_bytes(s[..4].try_into().unwrap());
    // C: n * 255.0F — f32 product, then widened to f64 for the saturate
    d[0] = crate::fixed::quick_saturate_byte((n * 255.0f32) as f64);
}
fn fromfltto16(d: &mut [u8], s: &[u8]) {
    let n = f32::from_le_bytes(s[..4].try_into().unwrap());
    d[..2].copy_from_slice(
        &crate::fixed::quick_saturate_word((n * 65535.0f32) as f64).to_le_bytes(),
    );
}
fn fromfltto16se(d: &mut [u8], s: &[u8]) {
    let n = f32::from_le_bytes(s[..4].try_into().unwrap());
    d[..2].copy_from_slice(
        &crate::fixed::quick_saturate_word((n * 65535.0f32) as f64).to_be_bytes(),
    );
}
fn fromflttohlf(d: &mut [u8], s: &[u8]) {
    let n = f32::from_le_bytes(s[..4].try_into().unwrap());
    d[..2].copy_from_slice(&crate::half::float2half(n).to_le_bytes());
}
fn fromflttodbl(d: &mut [u8], s: &[u8]) {
    let n = f32::from_le_bytes(s[..4].try_into().unwrap());
    d[..8].copy_from_slice(&(n as f64).to_le_bytes());
}
fn fromhlfto8(d: &mut [u8], s: &[u8]) {
    let n = crate::half::half2float(u16::from_le_bytes(s[..2].try_into().unwrap()));
    d[0] = crate::fixed::quick_saturate_byte(n as f64 * 255.0);
}
fn fromhlfto16(d: &mut [u8], s: &[u8]) {
    let n = crate::half::half2float(u16::from_le_bytes(s[..2].try_into().unwrap()));
    d[..2].copy_from_slice(&crate::fixed::quick_saturate_word(n as f64 * 65535.0).to_le_bytes());
}
fn fromhlfto16se(d: &mut [u8], s: &[u8]) {
    let n = crate::half::half2float(u16::from_le_bytes(s[..2].try_into().unwrap()));
    d[..2].copy_from_slice(&crate::fixed::quick_saturate_word(n as f64 * 65535.0).to_be_bytes());
}
fn fromhlftoflt(d: &mut [u8], s: &[u8]) {
    let n = crate::half::half2float(u16::from_le_bytes(s[..2].try_into().unwrap()));
    d[..4].copy_from_slice(&n.to_le_bytes());
}
fn fromhlftodbl(d: &mut [u8], s: &[u8]) {
    let n = crate::half::half2float(u16::from_le_bytes(s[..2].try_into().unwrap()));
    d[..8].copy_from_slice(&(n as f64).to_le_bytes());
}
fn copy32(d: &mut [u8], s: &[u8]) {
    d[..4].copy_from_slice(&s[..4]);
}
fn fromdblto8(d: &mut [u8], s: &[u8]) {
    let n = f64::from_le_bytes(s[..8].try_into().unwrap());
    d[0] = crate::fixed::quick_saturate_byte(n * 255.0);
}
fn fromdblto16(d: &mut [u8], s: &[u8]) {
    let n = f64::from_le_bytes(s[..8].try_into().unwrap());
    d[..2].copy_from_slice(&crate::fixed::quick_saturate_word(n * 65535.0).to_le_bytes());
}
fn fromdblto16se(d: &mut [u8], s: &[u8]) {
    let n = f64::from_le_bytes(s[..8].try_into().unwrap());
    d[..2].copy_from_slice(&crate::fixed::quick_saturate_word(n * 65535.0).to_be_bytes());
}
fn fromdbltoflt(d: &mut [u8], s: &[u8]) {
    let n = f64::from_le_bytes(s[..8].try_into().unwrap());
    d[..4].copy_from_slice(&(n as f32).to_le_bytes());
}
fn fromdbltohlf(d: &mut [u8], s: &[u8]) {
    let n = f64::from_le_bytes(s[..8].try_into().unwrap());
    d[..2].copy_from_slice(&crate::half::float2half(n as f32).to_le_bytes());
}
fn copy64(d: &mut [u8], s: &[u8]) {
    d[..8].copy_from_slice(&s[..8]);
}

/// `FormatterPos` — format → row/col index into `FormattersAlpha`
fn formatter_pos(frm: u32) -> i32 {
    let b = t_bytes(frm);
    if b == 0 && t_float(frm) != 0 {
        return 5; // DBL
    }
    if b == 2 && t_float(frm) != 0 {
        return 3; // HLF
    }
    if b == 4 && t_float(frm) != 0 {
        return 4; // FLT
    }
    if b == 2 && t_float(frm) == 0 {
        return if t_endian16(frm) != 0 { 2 } else { 1 };
    }
    if b == 1 && t_float(frm) == 0 {
        return 0; // 8
    }
    -1
}

static FORMATTERS_ALPHA: [[FormatterAlphaFn; 6]; 6] = [
    [
        copy8,
        from8to16,
        from8to16se,
        from8tohlf,
        from8toflt,
        from8todbl,
    ],
    [
        from16to8,
        copy16,
        from16to16,
        from16tohlf,
        from16toflt,
        from16todbl,
    ],
    [
        from16seto8,
        from16to16,
        copy16,
        from16setohlf,
        from16setoflt,
        from16setodbl,
    ],
    [
        fromhlfto8,
        fromhlfto16,
        fromhlfto16se,
        copy16,
        fromhlftoflt,
        fromhlftodbl,
    ],
    [
        fromfltto8,
        fromfltto16,
        fromfltto16se,
        fromflttohlf,
        copy32,
        fromflttodbl,
    ],
    [
        fromdblto8,
        fromdblto16,
        fromdblto16se,
        fromdbltohlf,
        fromdbltoflt,
        copy64,
    ],
];

/// `_cmsGetFormatterAlpha`
pub fn get_formatter_alpha(
    ctx: Option<&Context>,
    in_fmt: u32,
    out_fmt: u32,
) -> Option<FormatterAlphaFn> {
    let i = formatter_pos(in_fmt);
    let o = formatter_pos(out_fmt);
    if i < 0 || o < 0 || i > 5 || o > 5 {
        crate::err::signal_error(
            ctx,
            crate::types::ERROR_UNKNOWN_EXTENSION,
            "Unrecognized alpha channel width",
        );
        return None;
    }
    Some(FORMATTERS_ALPHA[i as usize][o as usize])
}

/// `ComputeIncrementsForChunky`
fn compute_increments_for_chunky(fmt: u32, start_order: &mut [u32], ptr_inc: &mut [u32]) -> bool {
    let mut channels = [0u32; 16];
    let extra = t_extra(fmt);
    let n_ch = t_channels(fmt);
    let total = n_ch + extra;
    let ch_size = true_bytes_size(fmt);
    let pixel_size = ch_size * total;
    if total == 0 || total >= 16 {
        return false;
    }
    for i in 0..extra as usize {
        ptr_inc[i] = pixel_size;
    }
    for i in 0..total as usize {
        channels[i] = if t_doswap(fmt) != 0 {
            total - i as u32 - 1
        } else {
            i as u32
        };
    }
    if t_swapfirst(fmt) != 0 && total > 1 {
        let tmp = channels[0];
        for i in 0..total as usize - 1 {
            channels[i] = channels[i + 1];
        }
        channels[total as usize - 1] = tmp;
    }
    if ch_size > 1 {
        for i in 0..total as usize {
            channels[i] *= ch_size;
        }
    }
    for i in 0..extra as usize {
        start_order[i] = channels[i + n_ch as usize];
    }
    true
}

/// `ComputeIncrementsForPlanar`
fn compute_increments_for_planar(
    fmt: u32,
    bytes_per_plane: u32,
    start_order: &mut [u32],
    ptr_inc: &mut [u32],
) -> bool {
    let mut channels = [0u32; 16];
    let extra = t_extra(fmt);
    let n_ch = t_channels(fmt);
    let total = n_ch + extra;
    let ch_size = true_bytes_size(fmt);
    if total == 0 || total >= 16 {
        return false;
    }
    for i in 0..extra as usize {
        ptr_inc[i] = ch_size;
    }
    for i in 0..total as usize {
        channels[i] = if t_doswap(fmt) != 0 {
            total - i as u32 - 1
        } else {
            i as u32
        };
    }
    if t_swapfirst(fmt) != 0 && total > 0 {
        let tmp = channels[0];
        for i in 0..total as usize - 1 {
            channels[i] = channels[i + 1];
        }
        channels[total as usize - 1] = tmp;
    }
    for i in 0..total as usize {
        channels[i] *= bytes_per_plane;
    }
    for i in 0..extra as usize {
        start_order[i] = channels[i + n_ch as usize];
    }
    true
}

/// `ComputeComponentIncrements`
fn compute_component_increments(
    fmt: u32,
    bytes_per_plane: u32,
    start_order: &mut [u32],
    ptr_inc: &mut [u32],
) -> bool {
    if t_planar(fmt) != 0 {
        compute_increments_for_planar(fmt, bytes_per_plane, start_order, ptr_inc)
    } else {
        compute_increments_for_chunky(fmt, start_order, ptr_inc)
    }
}

/// `_cmsHandleExtraChannels` — the real C version with formatter conversion
pub fn handle_extra_channels(
    ctx: Option<&Context>,
    input: &[u8],
    output: &mut [u8],
    ppl: u32,
    lines: u32,
    stride: &Stride,
    in_fmt: u32,
    out_fmt: u32,
    dw_flags: u32,
) {
    if dw_flags & crate::xform::FLAGS_COPY_ALPHA == 0 {
        return;
    }
    if in_fmt == out_fmt && input.as_ptr() == output.as_ptr() {
        return;
    }
    let n_extra = t_extra(in_fmt) as usize;
    if n_extra != t_extra(out_fmt) as usize || n_extra == 0 {
        return;
    }
    let mut src_order = [0u32; 16];
    let mut src_inc = [0u32; 16];
    let mut dst_order = [0u32; 16];
    let mut dst_inc = [0u32; 16];
    if !compute_component_increments(
        in_fmt,
        stride.bytes_per_plane_in,
        &mut src_order,
        &mut src_inc,
    ) {
        return;
    }
    if !compute_component_increments(
        out_fmt,
        stride.bytes_per_plane_out,
        &mut dst_order,
        &mut dst_inc,
    ) {
        return;
    }
    let Some(copy_value) = get_formatter_alpha(ctx, in_fmt, out_fmt) else {
        return;
    };
    if n_extra == 1 {
        let mut s_line = 0usize;
        let mut d_line = 0usize;
        for _ in 0..lines {
            let mut src = s_line + src_order[0] as usize;
            let mut dst = d_line + dst_order[0] as usize;
            for _ in 0..ppl {
                copy_value(&mut output[dst..], &input[src..]);
                src += src_inc[0] as usize;
                dst += dst_inc[0] as usize;
            }
            s_line += stride.bytes_per_line_in as usize;
            d_line += stride.bytes_per_line_out as usize;
        }
    } else {
        let mut s_line = [0usize; 16];
        let mut d_line = [0usize; 16];
        for _ in 0..lines {
            let mut src = [0usize; 16];
            let mut dst = [0usize; 16];
            for j in 0..n_extra {
                src[j] = input.as_ptr() as usize + src_order[j] as usize + s_line[j];
                dst[j] = output.as_ptr() as usize + dst_order[j] as usize + d_line[j];
            }
            for _ in 0..ppl {
                for k in 0..n_extra {
                    let so = src[k] - input.as_ptr() as usize;
                    let doo = dst[k] - output.as_ptr() as usize;
                    copy_value(&mut output[doo..], &input[so..]);
                    src[k] += src_inc[k] as usize;
                    dst[k] += dst_inc[k] as usize;
                }
            }
            for j in 0..n_extra {
                s_line[j] += stride.bytes_per_line_in as usize;
                d_line[j] += stride.bytes_per_line_out as usize;
            }
        }
    }
}
