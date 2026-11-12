//! `cmspack.c` — pixel formatters: Unroll* (input decode), Pack* (output
//! encode), stock formatter tables and `_cmsGetFormatter` dispatch.
//!
//! C signature: `fn(info, out[], accum, Stride) -> accum'` — the returned
//! pointer is a byte advance here (`usize`), keeping the crate unsafe-free.
//! `Stride` is used by planar formatters as the plane stride.

// ---- T_* format bitfield decoders (lcms2.h) --------------------------------

pub const fn t_bytes(f: u32) -> u32 {
    f & 0x7
}
pub const fn t_channels(f: u32) -> u32 {
    (f >> 3) & 15
}
pub const fn t_extra(f: u32) -> u32 {
    (f >> 7) & 7
}
pub const fn t_doswap(f: u32) -> u32 {
    (f >> 10) & 1
}
pub const fn t_endian16(f: u32) -> u32 {
    (f >> 11) & 1
}
pub const fn t_planar(f: u32) -> u32 {
    (f >> 12) & 1
}
pub const fn t_flavor(f: u32) -> u32 {
    (f >> 13) & 1
}
pub const fn t_swapfirst(f: u32) -> u32 {
    (f >> 14) & 1
}
pub const fn t_half(f: u32) -> u32 {
    (f >> 16) & 1
}
pub const fn t_float(f: u32) -> u32 {
    (f >> 22) & 1
}
pub const fn t_premul(f: u32) -> u32 {
    (f >> 23) & 1
}
pub const fn t_dither(f: u32) -> u32 {
    (f >> 24) & 1
}
pub const fn t_colorspace(f: u32) -> u32 {
    (f >> 16) & 31
}

/// `PackFactor` — bytes consumed per input pixel (channels+extra) * bytes
pub fn pack_factor(fmt: u32) -> u32 {
    (t_channels(fmt) + t_extra(fmt)) * t_bytes(fmt)
}

pub(crate) fn f8to16(v: u8) -> u16 {
    ((v as u16) << 8) | v as u16
}
pub(crate) fn f16to8(v: u16) -> u8 {
    (((v as u32) * 65281 + 8388608) >> 24 & 0xff) as u8
}
/// `_cmsToFixedDomain` — `a + (a + 0x7fff)/0xffff` (lcms2_internal.h:151)
fn to_fixed_domain(v: u16) -> u32 {
    let a = v as u32;
    a + (a + 0x7fff) / 0xffff
}
// `REVERSE_FLAVOR_16/8/Float`

pub struct PackInfo {
    pub input_format: u32,
    pub output_format: u32,
    /// plane stride for planar formats (`Stride` param of the C fn)
    pub stride: u32,
}

pub type Unpack16Fn = fn(&PackInfo, &mut [u16; crate::xform::MAX_CHANNELS], &[u8], u32) -> usize;
pub type Pack16Fn = fn(&PackInfo, &[u16; crate::xform::MAX_CHANNELS], &mut [u8], u32) -> usize;
pub type UnpackF32Fn = fn(&PackInfo, &mut [f32; crate::xform::MAX_CHANNELS], &[u8], u32) -> usize;
pub type PackF32Fn = fn(&PackInfo, &[f32; crate::xform::MAX_CHANNELS], &mut [u8], u32) -> usize;

/// Foreign (C-registered) formatter dispatch — `cmsPluginFormatters` plugins
/// return `cmsFormatter` unions pointing at C `cmsFormatter16`/`Float` fns.
/// `lcms` forbids `unsafe`, so `lcms2-ffi` installs these hooks and the eval
/// loops reach the C fns through `Unpack16Fn`/`Pack16Fn`-compatible trampolines.
/// Formatters read `InputFormat`/`OutputFormat` off `PackInfo`, never a live
/// `_cmsTRANSFORM*` — the hook's first arg is a null info ptr.
pub struct ForeignFmtHooks {
    /// Invoke a `cmsFormatterFactory(Type, Dir, dwFlags)` → `cmsFormatter`
    /// union (one fn ptr) or 0.
    pub factory: fn(usize, u32, u32, u32) -> usize,
    /// Invoke a `cmsFormatter16` — returns the accum offset consumed/produced.
    pub kernel16: fn(usize, &mut [u16], &[u8], u32) -> usize,
    /// Invoke a `cmsFormatterFloat` — returns the accum offset.
    pub kernelf32: fn(usize, &mut [f32], &[u8], u32) -> usize,
}
static FMT_HOOKS: std::sync::OnceLock<ForeignFmtHooks> = std::sync::OnceLock::new();
/// `(dir as u8, format) → C formatter fn ptr` — populated when a transform
/// consults a ctx formatter factory (mirrors the C `Formatters` plugin chain).
static FOREIGN_FMT: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<(u8, u32), usize>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

pub fn set_foreign_fmt_hooks(h: ForeignFmtHooks) {
    let _ = FMT_HOOKS.set(h);
}

/// First non-NULL `cmsFormatter` union from the ctx's formatter factories
/// (`_cmsGetFormatter`'s plugin consult) — `dir` 0=Input 1=Output, `flags`
/// `CMS_PACK_FLAGS_*`.
pub fn foreign_formatter_fn(
    ctx: Option<&crate::context::Context>,
    ty: u32,
    dir: u32,
    flags: u32,
) -> Option<usize> {
    let cctx = ctx?;
    let hooks = FMT_HOOKS.get()?;
    for &f in &cctx.formatter_factories {
        let r = (hooks.factory)(f, ty, dir, flags);
        if r != 0 {
            return Some(r);
        }
    }
    None
}

/// Bind a foreign formatter ptr under `(dir, ty)` — the trampolines resolve it.
pub fn register_foreign_format(dir: u32, ty: u32, cfn: usize) {
    FOREIGN_FMT.lock().unwrap().insert((dir as u8, ty), cfn);
}

fn foreign16(dir: u8, fmt: u32) -> Option<(usize, &'static ForeignFmtHooks)> {
    let cf = *FOREIGN_FMT.lock().unwrap().get(&(dir, fmt))?;
    Some((cf, FMT_HOOKS.get()?))
}

/// `Unpack16Fn` trampoline for a C `cmsFormatter16` input formatter.
pub fn foreign_unpack16(
    i: &PackInfo,
    w: &mut [u16; crate::xform::MAX_CHANNELS],
    a: &[u8],
    stride: u32,
) -> usize {
    let Some((cf, h)) = foreign16(0, i.input_format) else {
        return 0;
    };
    (h.kernel16)(cf, w, a, stride)
}
/// `Pack16Fn` trampoline for a C `cmsFormatter16` output formatter.
pub fn foreign_pack16(
    i: &PackInfo,
    w: &[u16; crate::xform::MAX_CHANNELS],
    out: &mut [u8],
    stride: u32,
) -> usize {
    let Some((cf, h)) = foreign16(1, i.output_format) else {
        return 0;
    };
    let mut w_tmp = *w;
    (h.kernel16)(cf, &mut w_tmp, out, stride)
}
/// `UnpackF32Fn` trampoline for a C `cmsFormatterFloat` input formatter.
pub fn foreign_unpack_f32(
    i: &PackInfo,
    w: &mut [f32; crate::xform::MAX_CHANNELS],
    a: &[u8],
    stride: u32,
) -> usize {
    let Some((cf, h)) = foreign16(0, i.input_format) else {
        return 0;
    };
    (h.kernelf32)(cf, w, a, stride)
}
/// `PackF32Fn` trampoline for a C `cmsFormatterFloat` output formatter.
pub fn foreign_pack_f32(
    i: &PackInfo,
    w: &[f32; crate::xform::MAX_CHANNELS],
    out: &mut [u8],
    stride: u32,
) -> usize {
    let Some((cf, h)) = foreign16(1, i.output_format) else {
        return 0;
    };
    let mut w_tmp = *w;
    (h.kernelf32)(cf, &mut w_tmp, out, stride)
}
pub type UnpackDblFn = fn(&PackInfo, &mut [f64], &[u8], u32) -> usize;

// ---------------------------------------------------------------------------
// Unroll 8-bit → u16
// ---------------------------------------------------------------------------

/// `UnrollChunkyBytes`
pub fn unroll_chunky_bytes(
    info: &PackInfo,
    w_in: &mut [u16; crate::xform::MAX_CHANNELS],
    accum: &[u8],
    _stride: u32,
) -> usize {
    let n_chan = t_channels(info.input_format) as usize;
    let do_swap = t_doswap(info.input_format) != 0;
    let reverse = t_flavor(info.input_format) != 0;
    let swap_first = t_swapfirst(info.input_format) != 0;
    let extra = t_extra(info.input_format) as usize;
    let premul = t_premul(info.input_format) != 0;
    let extra_first = do_swap ^ swap_first;
    let mut alpha_factor: u32 = 1;
    let mut off = 0usize;
    if extra_first {
        if premul && extra > 0 {
            alpha_factor = to_fixed_domain(f8to16(accum[0]));
        }
        off += extra;
    } else if premul && extra > 0 {
        alpha_factor = to_fixed_domain(f8to16(accum[n_chan]));
    }
    for i in 0..n_chan {
        let index = if do_swap { n_chan - i - 1 } else { i };
        let mut v = f8to16(accum[off]) as u32;
        off += 1;
        if reverse {
            v = 0xffff - v;
        }
        if premul && alpha_factor > 0 {
            v = (v << 16) / alpha_factor;
            if v > 0xffff {
                v = 0xffff;
            }
        }
        w_in[index] = v as u16;
    }
    if !extra_first {
        off += extra;
    }
    if extra == 0 && swap_first {
        let tmp = w_in[0];
        w_in.copy_within(1..n_chan, 0);
        w_in[n_chan - 1] = tmp;
    }
    off
}

/// `UnrollPlanarBytes`
pub fn unroll_planar_bytes(
    info: &PackInfo,
    w_in: &mut [u16; crate::xform::MAX_CHANNELS],
    accum: &[u8],
    stride: u32,
) -> usize {
    let n_chan = t_channels(info.input_format) as usize;
    let do_swap = t_doswap(info.input_format) != 0;
    let swap_first = t_swapfirst(info.input_format) != 0;
    let reverse = t_flavor(info.input_format) != 0;
    let extra = t_extra(info.input_format) as usize;
    let premul = t_premul(info.input_format) != 0;
    let extra_first = do_swap ^ swap_first;
    let mut alpha_factor: u32 = 1;
    let init = 0usize;
    if extra_first && premul && extra > 0 {
        alpha_factor = to_fixed_domain(f8to16(accum[init]));
    }
    let stride = stride as usize;
    let mut pos = if extra_first { extra * stride } else { init };
    let mut plane = 0;
    for i in 0..n_chan {
        let index = if do_swap { n_chan - i - 1 } else { i };
        let mut v = f8to16(accum[pos]) as u32;
        plane += 1;
        // `extra_first` planes are offset by `extra` extra-channel planes:
        // color plane `i` lives at `(extra + i)*stride`, not `i*stride`.
        pos = if extra_first {
            (extra + plane) * stride
        } else {
            plane * stride
        };
        if reverse {
            v = 0xffff - v;
        }
        if premul && alpha_factor > 0 {
            v = (v << 16) / alpha_factor;
            if v > 0xffff {
                v = 0xffff;
            }
        }
        w_in[index] = v as u16;
    }
    if !extra_first {
        let _ = pos; // extras already walked via plane indexing
    }
    // C: return accum + 1 — each plane advances one byte per pixel
    1
}

fn rd16(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes(b[o..o + 2].try_into().unwrap())
}
fn rd32f(b: &[u8], o: usize) -> f32 {
    f32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}
fn rd64f(b: &[u8], o: usize) -> f64 {
    f64::from_le_bytes([
        b[o],
        b[o + 1],
        b[o + 2],
        b[o + 3],
        b[o + 4],
        b[o + 5],
        b[o + 6],
        b[o + 7],
    ])
}
fn fom_lab_v2_to_v4(x: u16) -> u16 {
    let a = ((x as u32) << 8 | x as u32) >> 8;
    if a > 0xffff { 0xffff } else { a as u16 }
}
fn fom_lab_v4_to_v2(x: u16) -> u16 {
    ((((x as u32) << 8) + 0x80) / 257) as u16
}
fn qs16(v: f64) -> u16 {
    crate::fixed::quick_saturate_word(v)
}

macro_rules! unroll8 {
    ($name:ident, [$($idx:expr),+], $skip:expr) => {
        pub fn $name(_i: &PackInfo, w: &mut [u16; crate::xform::MAX_CHANNELS], a: &[u8], _s: u32) -> usize {
            const N: usize = 0usize $(+ {let _ = $idx; 1})+ + $skip;
            let a = &a[..N];
            let mut o = 0;
            $(
                // 255 marks a skipped channel. Real indices are checked at
                // compile time; `get_mut` keeps clippy from flagging the dead
                // `w[255]` arm.
                const { assert!($idx == 255 || $idx < crate::xform::MAX_CHANNELS) };
                if $idx != 255 {
                    if let Some(slot) = w.get_mut($idx) { *slot = f8to16(a[o]); }
                }
                o += 1;
            )+
            o + $skip
        }
    };
    ($name:ident, [$($idx:expr),+], $skip:expr, rev) => {
        pub fn $name(_i: &PackInfo, w: &mut [u16; crate::xform::MAX_CHANNELS], a: &[u8], _s: u32) -> usize {
            const N: usize = 0usize $(+ {let _ = $idx; 1})+ + $skip;
            let a = &a[..N];
            let mut o = 0;
            $(
                if $idx == 255 { o += 1; } else {
                    w[$idx] = f8to16(0xff - a[o]); o += 1;
                }
            )+
            o + $skip
        }
    };
}

unroll8!(unroll4_bytes, [0, 1, 2, 3], 0);
unroll8!(unroll4_bytes_reverse, [0, 1, 2, 3], 0, rev);
// `UnrollChunkyBytes` specialized to chunky 3ch+ex1 (TYPE_RGBA_8 & kin —
// C's table falls through to the generic for this shape).
unroll8!(unroll3_bytes_skip1, [0, 1, 2], 1);
unroll8!(unroll4_bytes_swap_first, [3, 0, 1, 2], 0);
unroll8!(unroll4_bytes_swap, [3, 2, 1, 0], 0);
unroll8!(unroll4_bytes_swap_swap_first, [2, 1, 0, 3], 0);
unroll8!(unroll3_bytes, [0, 1, 2], 0);
// A B G R → skip A first, then B,G,R into w[2],w[1],w[0]
// (`Unroll3BytesSkip1Swap` — the skip is leading, not trailing)
pub fn unroll3_bytes_skip1_swap(
    i: &PackInfo,
    w: &mut [u16; crate::xform::MAX_CHANNELS],
    a: &[u8],
    s: u32,
) -> usize {
    let mut o = 1;
    w[2] = f8to16(a[o]);
    o += 1;
    w[1] = f8to16(a[o]);
    o += 1;
    w[0] = f8to16(a[o]);
    o += 1;
    let _ = i;
    let _ = s;
    o
}
unroll8!(unroll3_bytes_skip1_swap_swap_first, [2, 1, 0, 255], 0);
// B G R A — w[2]w[1]w[0] then skip A
pub fn unroll3_bytes_skip1_swap_swap_first_real(
    i: &PackInfo,
    w: &mut [u16; crate::xform::MAX_CHANNELS],
    a: &[u8],
    s: u32,
) -> usize {
    let _ = i;
    let _ = s;
    w[2] = f8to16(a[0]);
    w[1] = f8to16(a[1]);
    w[0] = f8to16(a[2]);
    4
}
pub fn unroll3_bytes_skip1_swap_first(
    i: &PackInfo,
    w: &mut [u16; crate::xform::MAX_CHANNELS],
    a: &[u8],
    s: u32,
) -> usize {
    let _ = i;
    let _ = s;
    w[0] = f8to16(a[1]);
    w[1] = f8to16(a[2]);
    w[2] = f8to16(a[3]);
    4
}
unroll8!(unroll3_bytes_swap, [2, 1, 0], 0);
pub fn unroll_lab_v2_8(
    i: &PackInfo,
    w: &mut [u16; crate::xform::MAX_CHANNELS],
    a: &[u8],
    s: u32,
) -> usize {
    let _ = i;
    let _ = s;
    w[0] = fom_lab_v2_to_v4(f8to16(a[0]));
    w[1] = fom_lab_v2_to_v4(f8to16(a[1]));
    w[2] = fom_lab_v2_to_v4(f8to16(a[2]));
    3
}
pub fn unroll_alab_v2_8(
    i: &PackInfo,
    w: &mut [u16; crate::xform::MAX_CHANNELS],
    a: &[u8],
    s: u32,
) -> usize {
    let _ = i;
    let _ = s;
    w[0] = fom_lab_v2_to_v4(f8to16(a[1]));
    w[1] = fom_lab_v2_to_v4(f8to16(a[2]));
    w[2] = fom_lab_v2_to_v4(f8to16(a[3]));
    4
}
pub fn unroll_lab_v2_16(
    i: &PackInfo,
    w: &mut [u16; crate::xform::MAX_CHANNELS],
    a: &[u8],
    s: u32,
) -> usize {
    let _ = i;
    let _ = s;
    w[0] = fom_lab_v2_to_v4(rd16(a, 0));
    w[1] = fom_lab_v2_to_v4(rd16(a, 2));
    w[2] = fom_lab_v2_to_v4(rd16(a, 4));
    6
}
pub fn unroll2_bytes(
    i: &PackInfo,
    w: &mut [u16; crate::xform::MAX_CHANNELS],
    a: &[u8],
    s: u32,
) -> usize {
    let _ = i;
    let _ = s;
    w[0] = f8to16(a[0]);
    w[1] = f8to16(a[1]);
    2
}
pub fn unroll1_byte(
    i: &PackInfo,
    w: &mut [u16; crate::xform::MAX_CHANNELS],
    a: &[u8],
    s: u32,
) -> usize {
    let _ = i;
    let _ = s;
    w[0] = f8to16(a[0]);
    w[1] = w[0];
    w[2] = w[0];
    1
}
pub fn unroll1_byte_skip1(
    i: &PackInfo,
    w: &mut [u16; crate::xform::MAX_CHANNELS],
    a: &[u8],
    s: u32,
) -> usize {
    let _ = i;
    let _ = s;
    w[0] = f8to16(a[0]);
    w[1] = w[0];
    w[2] = w[0];
    2
}
pub fn unroll1_byte_skip2(
    i: &PackInfo,
    w: &mut [u16; crate::xform::MAX_CHANNELS],
    a: &[u8],
    s: u32,
) -> usize {
    let _ = i;
    let _ = s;
    w[0] = f8to16(a[0]);
    w[1] = w[0];
    w[2] = w[0];
    3
}
pub fn unroll1_byte_reversed(
    i: &PackInfo,
    w: &mut [u16; crate::xform::MAX_CHANNELS],
    a: &[u8],
    s: u32,
) -> usize {
    let _ = i;
    let _ = s;
    w[0] = f8to16(0xff - a[0]);
    w[1] = w[0];
    w[2] = w[0];
    1
}

/// `UnrollAnyWords` — generic 16-bit chunky decoder
pub fn unroll_any_words(
    i: &PackInfo,
    w: &mut [u16; crate::xform::MAX_CHANNELS],
    a: &[u8],
    _s: u32,
) -> usize {
    let n_chan = t_channels(i.input_format) as usize;
    let swap_endian = t_endian16(i.input_format) != 0;
    let do_swap = t_doswap(i.input_format) != 0;
    let reverse = t_flavor(i.input_format) != 0;
    let swap_first = t_swapfirst(i.input_format) != 0;
    let extra = t_extra(i.input_format) as usize;
    let extra_first = do_swap ^ swap_first;
    let mut o = if extra_first { extra * 2 } else { 0 };
    for c in 0..n_chan {
        let index = if do_swap { n_chan - c - 1 } else { c };
        let mut v = rd16(a, o);
        if swap_endian {
            v = v.swap_bytes();
        }
        w[index] = if reverse { 0xffff - v } else { v };
        o += 2;
    }
    if !extra_first {
        o += extra * 2;
    }
    if extra == 0 && swap_first {
        let tmp = w[0];
        w.copy_within(1..n_chan, 0);
        w[n_chan - 1] = tmp;
    }
    o
}

/// `UnrollAnyWordsPremul`
pub fn unroll_any_words_premul(
    i: &PackInfo,
    w: &mut [u16; crate::xform::MAX_CHANNELS],
    a: &[u8],
    _s: u32,
) -> usize {
    let n_chan = t_channels(i.input_format) as usize;
    let swap_endian = t_endian16(i.input_format) != 0;
    let do_swap = t_doswap(i.input_format) != 0;
    let reverse = t_flavor(i.input_format) != 0;
    let swap_first = t_swapfirst(i.input_format) != 0;
    let extra = t_extra(i.input_format) as usize;
    let extra_first = do_swap ^ swap_first;
    let alpha = rd16(a, if extra_first { 0 } else { n_chan * 2 });
    let alpha_factor = to_fixed_domain(alpha);
    let mut o = if extra_first { extra * 2 } else { 0 };
    for c in 0..n_chan {
        let index = if do_swap { n_chan - c - 1 } else { c };
        let mut v = rd16(a, o) as u32;
        if swap_endian {
            v = (v as u16).swap_bytes() as u32;
        }
        if let Some(q) = (v << 16).checked_div(alpha_factor) {
            v = q;
            if v > 0xffff {
                v = 0xffff;
            }
        }
        w[index] = if reverse { 0xffff - v as u16 } else { v as u16 };
        o += 2;
    }
    if !extra_first {
        o += extra * 2;
    }
    o
}

/// `UnrollPlanarWords` — C: channels at Stride/word boundaries, returns accum+2
pub fn unroll_planar_words(
    i: &PackInfo,
    w: &mut [u16; crate::xform::MAX_CHANNELS],
    a: &[u8],
    stride: u32,
) -> usize {
    let n_chan = t_channels(i.input_format) as usize;
    let swap_endian = t_endian16(i.input_format) != 0;
    let do_swap = t_doswap(i.input_format) != 0;
    let swap_first = t_swapfirst(i.input_format) != 0;
    let reverse = t_flavor(i.input_format) != 0;
    let extra = t_extra(i.input_format) as usize;
    let extra_first = do_swap ^ swap_first;
    let stride = stride as usize;
    let init = if extra_first { extra * stride } else { 0 };
    for c in 0..n_chan {
        let index = if do_swap { n_chan - c - 1 } else { c };
        let o = init + c * stride;
        let mut v = rd16(a, o);
        if swap_endian {
            v = v.swap_bytes();
        }
        w[index] = if reverse { 0xffff - v } else { v };
    }
    2
}

/// `UnrollPlanarWordsPremul`
pub fn unroll_planar_words_premul(
    i: &PackInfo,
    w: &mut [u16; crate::xform::MAX_CHANNELS],
    a: &[u8],
    stride: u32,
) -> usize {
    let n_chan = t_channels(i.input_format) as usize;
    let swap_endian = t_endian16(i.input_format) != 0;
    let do_swap = t_doswap(i.input_format) != 0;
    let swap_first = t_swapfirst(i.input_format) != 0;
    let reverse = t_flavor(i.input_format) != 0;
    let extra = t_extra(i.input_format) as usize;
    let extra_first = do_swap ^ swap_first;
    let stride = stride as usize;
    let init = if extra_first { extra * stride } else { 0 };
    let alpha = rd16(a, if extra_first { 0 } else { n_chan * stride });
    let alpha_factor = to_fixed_domain(alpha);
    for c in 0..n_chan {
        let index = if do_swap { n_chan - c - 1 } else { c };
        let o = init + c * stride;
        let mut v = rd16(a, o) as u32;
        if swap_endian {
            v = (v as u16).swap_bytes() as u32;
        }
        if let Some(q) = (v << 16).checked_div(alpha_factor) {
            v = q;
            if v > 0xffff {
                v = 0xffff;
            }
        }
        w[index] = if reverse { 0xffff - v as u16 } else { v as u16 };
    }
    2
}

macro_rules! unrollw {
    ($name:ident, [$($idx:expr),+], $adv:expr) => {
        pub fn $name(_i: &PackInfo, w: &mut [u16; crate::xform::MAX_CHANNELS], a: &[u8], _s: u32) -> usize {
            const N: usize = 0usize $(+ {let _ = $idx; 2})+ + $adv;
            let a = &a[..N];
            let mut o = 0;
            $(
                const { assert!($idx == 255 || $idx < crate::xform::MAX_CHANNELS) };
                if $idx != 255 {
                    if let Some(slot) = w.get_mut($idx) { *slot = rd16(a, o); }
                }
                o += 2;
            )+
            o + $adv
        }
    };
}

unrollw!(unroll4_words, [0, 1, 2, 3], 0);
// `UnrollAnyWords` specialized to chunky 3ch+ex1 (TYPE_RGBA_16 & kin —
// C's table has no plain `ch3|by2|ex1` entry, so the generic runs).
unrollw!(unroll3_words_skip1, [0, 1, 2], 2);
// `Unroll4WordsReverse` — REVERSE_FLAVOR_16 on each channel
pub fn unroll4_words_reverse(
    _i: &PackInfo,
    w: &mut [u16; crate::xform::MAX_CHANNELS],
    a: &[u8],
    _s: u32,
) -> usize {
    let mut o = 0;
    for c in 0..4 {
        w[c] = 0xffff - rd16(a, o);
        o += 2;
    }
    o
}
unrollw!(unroll4_words_swap_first, [3, 0, 1, 2], 0);
unrollw!(unroll4_words_swap, [3, 2, 1, 0], 0);
unrollw!(unroll4_words_swap_swap_first, [2, 1, 0, 3], 0);
unrollw!(unroll3_words, [0, 1, 2], 0);
unrollw!(unroll3_words_swap, [2, 1, 0], 0);
// A B G R words → skip A first, then B,G,R into w[2],w[1],w[0]
// (`Unroll3WordsSkip1Swap` — leading skip, not trailing)
pub fn unroll3_words_skip1_swap(
    _i: &PackInfo,
    w: &mut [u16; crate::xform::MAX_CHANNELS],
    a: &[u8],
    _s: u32,
) -> usize {
    let mut o = 2;
    w[2] = rd16(a, o);
    o += 2;
    w[1] = rd16(a, o);
    o += 2;
    w[0] = rd16(a, o);
    o + 2
}
unrollw!(unroll3_words_skip1_swap_first, [255, 0, 1, 2], 0);
pub fn unroll1_word(
    _i: &PackInfo,
    w: &mut [u16; crate::xform::MAX_CHANNELS],
    a: &[u8],
    _s: u32,
) -> usize {
    w[0] = rd16(a, 0);
    w[1] = w[0];
    w[2] = w[0];
    2
}
pub fn unroll1_word_reversed(
    _i: &PackInfo,
    w: &mut [u16; crate::xform::MAX_CHANNELS],
    a: &[u8],
    _s: u32,
) -> usize {
    w[0] = 0xffff - rd16(a, 0);
    w[1] = w[0];
    w[2] = w[0];
    2
}
pub fn unroll1_word_skip3(
    _i: &PackInfo,
    w: &mut [u16; crate::xform::MAX_CHANNELS],
    a: &[u8],
    _s: u32,
) -> usize {
    w[0] = rd16(a, 0);
    w[1] = w[0];
    w[2] = w[0];
    8
}
unrollw!(unroll2_words, [0, 1], 0);

// ---------------------------------------------------------------------------
// Lab / XYZ encoded input → u16
// ---------------------------------------------------------------------------

pub fn unroll_lab_double_to16(
    i: &PackInfo,
    w: &mut [u16; crate::xform::MAX_CHANNELS],
    a: &[u8],
    stride: u32,
) -> usize {
    use crate::pcs::float2lab_encoded;
    if t_planar(i.input_format) != 0 {
        let s = stride as usize;
        let lab = crate::types::CieLab {
            L: rd64f(a, 0),
            a: rd64f(a, s),
            b: rd64f(a, 2 * s),
        };
        let e = float2lab_encoded(&lab);
        w[0] = e[0];
        w[1] = e[1];
        w[2] = e[2];
        8
    } else {
        let lab = crate::types::CieLab {
            L: rd64f(a, 0),
            a: rd64f(a, 8),
            b: rd64f(a, 16),
        };
        let e = float2lab_encoded(&lab);
        w[0] = e[0];
        w[1] = e[1];
        w[2] = e[2];
        24 + t_extra(i.input_format) as usize * 8
    }
}

pub fn unroll_lab_float_to16(
    i: &PackInfo,
    w: &mut [u16; crate::xform::MAX_CHANNELS],
    a: &[u8],
    stride: u32,
) -> usize {
    use crate::pcs::float2lab_encoded;
    if t_planar(i.input_format) != 0 {
        let s = stride as usize;
        let lab = crate::types::CieLab {
            L: rd32f(a, 0) as f64,
            a: rd32f(a, s) as f64,
            b: rd32f(a, 2 * s) as f64,
        };
        let e = float2lab_encoded(&lab);
        w[0] = e[0];
        w[1] = e[1];
        w[2] = e[2];
        4
    } else {
        let lab = crate::types::CieLab {
            L: rd32f(a, 0) as f64,
            a: rd32f(a, 4) as f64,
            b: rd32f(a, 8) as f64,
        };
        let e = float2lab_encoded(&lab);
        w[0] = e[0];
        w[1] = e[1];
        w[2] = e[2];
        12 + t_extra(i.input_format) as usize * 4
    }
}

pub fn unroll_xyz_double_to16(
    i: &PackInfo,
    w: &mut [u16; crate::xform::MAX_CHANNELS],
    a: &[u8],
    stride: u32,
) -> usize {
    use crate::pcs::float2xyz_encoded;
    if t_planar(i.input_format) != 0 {
        let s = stride as usize;
        let xyz = crate::types::CieXyz {
            X: rd64f(a, 0),
            Y: rd64f(a, s),
            Z: rd64f(a, 2 * s),
        };
        let e = float2xyz_encoded(&xyz);
        w[0] = e[0];
        w[1] = e[1];
        w[2] = e[2];
        8
    } else {
        let xyz = crate::types::CieXyz {
            X: rd64f(a, 0),
            Y: rd64f(a, 8),
            Z: rd64f(a, 16),
        };
        let e = float2xyz_encoded(&xyz);
        w[0] = e[0];
        w[1] = e[1];
        w[2] = e[2];
        24 + t_extra(i.input_format) as usize * 8
    }
}

pub fn unroll_xyz_float_to16(
    i: &PackInfo,
    w: &mut [u16; crate::xform::MAX_CHANNELS],
    a: &[u8],
    stride: u32,
) -> usize {
    use crate::pcs::float2xyz_encoded;
    if t_planar(i.input_format) != 0 {
        let s = stride as usize;
        let xyz = crate::types::CieXyz {
            X: rd32f(a, 0) as f64,
            Y: rd32f(a, s) as f64,
            Z: rd32f(a, 2 * s) as f64,
        };
        let e = float2xyz_encoded(&xyz);
        w[0] = e[0];
        w[1] = e[1];
        w[2] = e[2];
        4
    } else {
        let xyz = crate::types::CieXyz {
            X: rd32f(a, 0) as f64,
            Y: rd32f(a, 4) as f64,
            Z: rd32f(a, 8) as f64,
        };
        let e = float2xyz_encoded(&xyz);
        w[0] = e[0];
        w[1] = e[1];
        w[2] = e[2];
        12 + t_extra(i.input_format) as usize * 4
    }
}

// ---------------------------------------------------------------------------
// Float/double input → u16
// ---------------------------------------------------------------------------

pub fn is_ink_space(fmt: u32) -> bool {
    matches!(t_colorspace(fmt), 5 | 6 | 19..=29)
}
fn pixel_size(fmt: u32) -> u32 {
    let b = t_bytes(fmt);
    if b == 0 { 8 } else { b }
}

fn unroll_fd_to16(
    i: &PackInfo,
    w: &mut [u16; crate::xform::MAX_CHANNELS],
    a: &[u8],
    stride: u32,
    is_double: bool,
) -> usize {
    let n_chan = t_channels(i.input_format) as usize;
    let do_swap = t_doswap(i.input_format) != 0;
    let reverse = t_flavor(i.input_format) != 0;
    let swap_first = t_swapfirst(i.input_format) != 0;
    let extra = t_extra(i.input_format) as usize;
    let extra_first = do_swap ^ swap_first;
    let planar = t_planar(i.input_format) != 0;
    let maximum = if is_ink_space(i.input_format) {
        655.35
    } else {
        65535.0
    };
    let esz = if is_double { 8usize } else { 4usize };
    let stride_elems = stride as usize / pixel_size(i.input_format) as usize;
    let start = if extra_first { extra } else { 0 };
    for c in 0..n_chan {
        let index = if do_swap { n_chan - c - 1 } else { c };
        let v = if planar {
            let o = (c + start) * stride_elems * esz;
            if is_double {
                rd64f(a, o)
            } else {
                rd32f(a, o) as f64
            }
        } else {
            let o = (c + start) * esz;
            if is_double {
                rd64f(a, o)
            } else {
                rd32f(a, o) as f64
            }
        };
        let mut vi = qs16(v * maximum);
        if reverse {
            vi = 0xffff - vi;
        }
        w[index] = vi;
    }
    if extra == 0 && swap_first {
        let tmp = w[0];
        w.copy_within(1..n_chan, 0);
        w[n_chan - 1] = tmp;
    }
    if planar { esz } else { (n_chan + extra) * esz }
}

pub fn unroll_double_to16(
    i: &PackInfo,
    w: &mut [u16; crate::xform::MAX_CHANNELS],
    a: &[u8],
    s: u32,
) -> usize {
    unroll_fd_to16(i, w, a, s, true)
}
pub fn unroll_float_to16(
    i: &PackInfo,
    w: &mut [u16; crate::xform::MAX_CHANNELS],
    a: &[u8],
    s: u32,
) -> usize {
    unroll_fd_to16(i, w, a, s, false)
}
/// `UnrollDouble1Chan`
pub fn unroll_double_1chan(
    i: &PackInfo,
    w: &mut [u16; crate::xform::MAX_CHANNELS],
    a: &[u8],
    _s: u32,
) -> usize {
    let extra = t_extra(i.input_format) as usize;
    w[0] = qs16(rd64f(a, 0) * 65535.0);
    (1 + extra) * 8
}

// ---------------------------------------------------------------------------
// Byte/word/float/double input → f32
// ---------------------------------------------------------------------------

fn unroll_fd_to_float(
    i: &PackInfo,
    w: &mut [f32; crate::xform::MAX_CHANNELS],
    a: &[u8],
    stride: u32,
    esz: usize,
) -> usize {
    let n_chan = t_channels(i.input_format) as usize;
    let do_swap = t_doswap(i.input_format) != 0;
    let reverse = t_flavor(i.input_format) != 0;
    let swap_first = t_swapfirst(i.input_format) != 0;
    let extra = t_extra(i.input_format) as usize;
    let extra_first = do_swap ^ swap_first;
    let planar = t_planar(i.input_format) != 0;
    let premul = t_premul(i.input_format) != 0;
    let maximum: f64 = if is_ink_space(i.input_format) {
        100.0
    } else {
        1.0
    };
    let div = match esz {
        1 => 255.0,
        2 => 65535.0,
        _ => maximum,
    };
    let stride_elems = stride as usize / pixel_size(i.input_format) as usize;
    let start = if extra_first { extra } else { 0 };
    // Interleaved f32 → direct loads (`v / 1.0` and `f32→f64→f32` are
    // exact) — the dominant float-transform format.
    if esz == 4 && !do_swap && !reverse && !planar && !swap_first && !premul && maximum == 1.0 {
        for c in 0..n_chan {
            w[c] = rd32f(a, (c + start) * 4);
        }
        return (n_chan + extra) * 4;
    }
    // Interleaved u8/u16 → f64 div then f32 truncate — same op chain as
    // the generic loop, minus the per-channel matches.
    if esz == 1 && !do_swap && !reverse && !planar && !swap_first && !premul {
        for c in 0..n_chan {
            w[c] = (a[c + start] as f64 / 255.0) as f32;
        }
        return n_chan + extra;
    }
    if esz == 2 && !do_swap && !reverse && !planar && !swap_first && !premul {
        for c in 0..n_chan {
            w[c] = (rd16(a, (c + start) * 2) as f64 / 65535.0) as f32;
        }
        return (n_chan + extra) * 2;
    }
    let mut alpha_factor = 1.0f64;
    if premul && extra > 0 {
        let o = if planar {
            if extra_first {
                0
            } else {
                n_chan * stride_elems * esz
            }
        } else if extra_first {
            0
        } else {
            n_chan * esz
        };
        alpha_factor = match esz {
            1 => a[o] as f64 / div,
            2 => rd16(a, o) as f64 / div,
            4 => rd32f(a, o) as f64 / div,
            _ => rd64f(a, o) / div,
        };
    }
    for c in 0..n_chan {
        let index = if do_swap { n_chan - c - 1 } else { c };
        let mut v = if planar {
            let o = (c + start) * stride_elems * esz;
            match esz {
                1 => a[o] as f64,
                2 => rd16(a, o) as f64,
                4 => rd32f(a, o) as f64,
                _ => rd64f(a, o),
            }
        } else {
            let o = (c + start) * esz;
            match esz {
                1 => a[o] as f64,
                2 => rd16(a, o) as f64,
                4 => rd32f(a, o) as f64,
                _ => rd64f(a, o),
            }
        };
        if premul && alpha_factor > 0.0 {
            v /= alpha_factor;
        }
        v /= div;
        w[index] = if reverse { 1.0 - v } else { v } as f32;
    }
    if extra == 0 && swap_first {
        let tmp = w[0];
        w.copy_within(1..n_chan, 0);
        w[n_chan - 1] = tmp;
    }
    if planar { esz } else { (n_chan + extra) * esz }
}

/// `Unroll8ToFloat` (premul via scale, not alpha div — C has no premul here)
pub fn unroll8_to_float(
    i: &PackInfo,
    w: &mut [f32; crate::xform::MAX_CHANNELS],
    a: &[u8],
    s: u32,
) -> usize {
    unroll_fd_to_float(i, w, a, s, 1)
}
pub fn unroll16_to_float(
    i: &PackInfo,
    w: &mut [f32; crate::xform::MAX_CHANNELS],
    a: &[u8],
    s: u32,
) -> usize {
    unroll_fd_to_float(i, w, a, s, 2)
}
pub fn unroll_floats_to_float(
    i: &PackInfo,
    w: &mut [f32; crate::xform::MAX_CHANNELS],
    a: &[u8],
    s: u32,
) -> usize {
    unroll_fd_to_float(i, w, a, s, 4)
}
pub fn unroll_doubles_to_float(
    i: &PackInfo,
    w: &mut [f32; crate::xform::MAX_CHANNELS],
    a: &[u8],
    s: u32,
) -> usize {
    unroll_fd_to_float(i, w, a, s, 8)
}

const MAX_ENC_XYZ: f64 = (2.0 * 65535.0) / 65536.0;

/// `UnrollLabDoubleToFloat`
pub fn unroll_lab_double_to_float(
    i: &PackInfo,
    w: &mut [f32; crate::xform::MAX_CHANNELS],
    a: &[u8],
    stride: u32,
) -> usize {
    if t_planar(i.input_format) != 0 {
        let s = stride as usize / pixel_size(i.input_format) as usize;
        w[0] = (rd64f(a, 0) / 100.0) as f32;
        w[1] = ((rd64f(a, s * 8) + 128.0) / 255.0) as f32;
        w[2] = ((rd64f(a, s * 16) + 128.0) / 255.0) as f32;
        8
    } else {
        w[0] = (rd64f(a, 0) / 100.0) as f32;
        w[1] = ((rd64f(a, 8) + 128.0) / 255.0) as f32;
        w[2] = ((rd64f(a, 16) + 128.0) / 255.0) as f32;
        (3 + t_extra(i.input_format) as usize) * 8
    }
}
/// `UnrollLabFloatToFloat`
pub fn unroll_lab_float_to_float(
    i: &PackInfo,
    w: &mut [f32; crate::xform::MAX_CHANNELS],
    a: &[u8],
    stride: u32,
) -> usize {
    if t_planar(i.input_format) != 0 {
        let s = stride as usize / pixel_size(i.input_format) as usize;
        w[0] = rd32f(a, 0) / 100.0;
        w[1] = (rd32f(a, s * 4) + 128.0) / 255.0;
        w[2] = (rd32f(a, s * 8) + 128.0) / 255.0;
        4
    } else {
        w[0] = rd32f(a, 0) / 100.0;
        w[1] = (rd32f(a, 4) + 128.0) / 255.0;
        w[2] = (rd32f(a, 8) + 128.0) / 255.0;
        (3 + t_extra(i.input_format) as usize) * 4
    }
}
/// `UnrollXYZDoubleToFloat`
pub fn unroll_xyz_double_to_float(
    i: &PackInfo,
    w: &mut [f32; crate::xform::MAX_CHANNELS],
    a: &[u8],
    stride: u32,
) -> usize {
    if t_planar(i.input_format) != 0 {
        let s = stride as usize / pixel_size(i.input_format) as usize;
        w[0] = (rd64f(a, 0) / MAX_ENC_XYZ) as f32;
        w[1] = (rd64f(a, s * 8) / MAX_ENC_XYZ) as f32;
        w[2] = (rd64f(a, s * 16) / MAX_ENC_XYZ) as f32;
        8
    } else {
        w[0] = (rd64f(a, 0) / MAX_ENC_XYZ) as f32;
        w[1] = (rd64f(a, 8) / MAX_ENC_XYZ) as f32;
        w[2] = (rd64f(a, 16) / MAX_ENC_XYZ) as f32;
        (3 + t_extra(i.input_format) as usize) * 8
    }
}
/// `UnrollXYZFloatToFloat`
pub fn unroll_xyz_float_to_float(
    i: &PackInfo,
    w: &mut [f32; crate::xform::MAX_CHANNELS],
    a: &[u8],
    stride: u32,
) -> usize {
    if t_planar(i.input_format) != 0 {
        let s = stride as usize / pixel_size(i.input_format) as usize;
        w[0] = rd32f(a, 0) / MAX_ENC_XYZ as f32;
        w[1] = rd32f(a, s * 4) / MAX_ENC_XYZ as f32;
        w[2] = rd32f(a, s * 8) / MAX_ENC_XYZ as f32;
        4
    } else {
        w[0] = rd32f(a, 0) / MAX_ENC_XYZ as f32;
        w[1] = rd32f(a, 4) / MAX_ENC_XYZ as f32;
        w[2] = rd32f(a, 8) / MAX_ENC_XYZ as f32;
        (3 + t_extra(i.input_format) as usize) * 4
    }
}

fn lab4_to_float(w: &mut [f32; crate::xform::MAX_CHANNELS], lab4: [u16; 3]) {
    let l = lab4[0] as f32 / 655.35;
    let a = (lab4[1] as f32 / 257.0) - 128.0;
    let b = (lab4[2] as f32 / 257.0) - 128.0;
    w[0] = l / 100.0;
    w[1] = (a + 128.0) / 255.0;
    w[2] = (b + 128.0) / 255.0;
}

pub fn unroll_lab_v2_8_to_float(
    _i: &PackInfo,
    w: &mut [f32; crate::xform::MAX_CHANNELS],
    a: &[u8],
    _s: u32,
) -> usize {
    lab4_to_float(
        w,
        [
            fom_lab_v2_to_v4(f8to16(a[0])),
            fom_lab_v2_to_v4(f8to16(a[1])),
            fom_lab_v2_to_v4(f8to16(a[2])),
        ],
    );
    3
}
pub fn unroll_alab_v2_8_to_float(
    _i: &PackInfo,
    w: &mut [f32; crate::xform::MAX_CHANNELS],
    a: &[u8],
    _s: u32,
) -> usize {
    lab4_to_float(
        w,
        [
            fom_lab_v2_to_v4(f8to16(a[1])),
            fom_lab_v2_to_v4(f8to16(a[2])),
            fom_lab_v2_to_v4(f8to16(a[3])),
        ],
    );
    4
}
pub fn unroll_lab_v2_16_to_float(
    _i: &PackInfo,
    w: &mut [f32; crate::xform::MAX_CHANNELS],
    a: &[u8],
    _s: u32,
) -> usize {
    lab4_to_float(
        w,
        [
            fom_lab_v2_to_v4(rd16(a, 0)),
            fom_lab_v2_to_v4(rd16(a, 2)),
            fom_lab_v2_to_v4(rd16(a, 4)),
        ],
    );
    6
}

// ---------------------------------------------------------------------------
// Pack u16 → bytes / words (chunky + planar generics)
// ---------------------------------------------------------------------------

/// `PackChunkyBytes`
pub fn pack_chunky_bytes(
    i: &PackInfo,
    w: &[u16; crate::xform::MAX_CHANNELS],
    out: &mut [u8],
    _s: u32,
) -> usize {
    let n_chan = t_channels(i.output_format) as usize;
    let do_swap = t_doswap(i.output_format) != 0;
    let reverse = t_flavor(i.output_format) != 0;
    let extra = t_extra(i.output_format) as usize;
    let swap_first = t_swapfirst(i.output_format) != 0;
    let premul = t_premul(i.output_format) != 0;
    let extra_first = do_swap ^ swap_first;
    let mut o = 0usize;
    let mut alpha_factor = 0u32;
    if extra_first {
        if premul && extra > 0 {
            alpha_factor = to_fixed_domain(f8to16(out[0]));
        }
        o += extra;
    } else if premul && extra > 0 {
        alpha_factor = to_fixed_domain(f8to16(out[n_chan]));
    }
    let mut v: u16 = 0;
    for c in 0..n_chan {
        let index = if do_swap { n_chan - c - 1 } else { c };
        v = w[index];
        if reverse {
            v = 0xffff - v;
        }
        if premul {
            v = (((v as u32) * alpha_factor + 0x8000) >> 16) as u16;
        }
        out[o] = f16to8(v);
        o += 1;
    }
    if !extra_first {
        o += extra;
    }
    if extra == 0 && swap_first {
        out.copy_within(0..n_chan - 1, 1);
        out[0] = f16to8(v);
    }
    o
}

/// `PackChunkyWords`
pub fn pack_chunky_words(
    i: &PackInfo,
    w: &[u16; crate::xform::MAX_CHANNELS],
    out: &mut [u8],
    _s: u32,
) -> usize {
    let n_chan = t_channels(i.output_format) as usize;
    let swap_endian = t_endian16(i.output_format) != 0;
    let do_swap = t_doswap(i.output_format) != 0;
    let reverse = t_flavor(i.output_format) != 0;
    let extra = t_extra(i.output_format) as usize;
    let swap_first = t_swapfirst(i.output_format) != 0;
    let premul = t_premul(i.output_format) != 0;
    let extra_first = do_swap ^ swap_first;
    let mut o = 0usize;
    let mut alpha_factor = 0u32;
    if extra_first {
        if premul && extra > 0 {
            alpha_factor = to_fixed_domain(rd16(out, 0));
        }
        o += extra * 2;
    } else if premul && extra > 0 {
        alpha_factor = to_fixed_domain(rd16(out, n_chan * 2));
    }
    let mut v: u16 = 0;
    for c in 0..n_chan {
        let index = if do_swap { n_chan - c - 1 } else { c };
        v = w[index];
        if swap_endian {
            v = v.swap_bytes();
        }
        if reverse {
            v = 0xffff - v;
        }
        if premul {
            v = (((v as u32) * alpha_factor + 0x8000) >> 16) as u16;
        }
        out[o] = (v & 0xff) as u8;
        out[o + 1] = (v >> 8) as u8;
        o += 2;
    }
    if !extra_first {
        o += extra * 2;
    }
    if extra == 0 && swap_first {
        // shift bytes right by 2 then place last-written v at front
        out.copy_within(0..2 * (n_chan - 1), 2);
        out[0] = (v & 0xff) as u8;
        out[1] = (v >> 8) as u8;
    }
    o
}

/// `PackPlanarBytes` — C returns output+1
pub fn pack_planar_bytes(
    i: &PackInfo,
    w: &[u16; crate::xform::MAX_CHANNELS],
    out: &mut [u8],
    stride: u32,
) -> usize {
    let n_chan = t_channels(i.output_format) as usize;
    let do_swap = t_doswap(i.output_format) != 0;
    let reverse = t_flavor(i.output_format) != 0;
    let extra = t_extra(i.output_format) as usize;
    let swap_first = t_swapfirst(i.output_format) != 0;
    let premul = t_premul(i.output_format) != 0;
    let extra_first = do_swap ^ swap_first;
    let stride = stride as usize;
    let init = if extra_first { extra * stride } else { 0 };
    // C PackPlanarBytes — premul alpha comes from the output buffer's
    // u8 alpha plane (extra-first: plane 0; else the plane after the
    // color planes), widened via FROM_8_TO_16.
    let mut alpha_factor = 0u32;
    if premul && extra > 0 {
        let a8 = if extra_first {
            out[0]
        } else {
            out[n_chan * stride]
        };
        alpha_factor = to_fixed_domain(f8to16(a8));
    }
    let mut last_v = 0u16;
    for c in 0..n_chan {
        let index = if do_swap { n_chan - c - 1 } else { c };
        let mut v = w[index];
        if reverse {
            v = 0xffff - v;
        }
        if premul {
            v = (((v as u32) * alpha_factor + 0x8000) >> 16) as u16;
        }
        out[init + c * stride] = f16to8(v);
        last_v = v;
    }
    if extra == 0 && swap_first {
        // memmove planes right one stride? C: memmove(swap1+Stride, swap1, (nChan-1)*Stride); *swap1 = FROM_16_TO_8(v)
        let mut buf = vec![0u8; (n_chan - 1) * stride];
        buf.copy_from_slice(&out[0..(n_chan - 1) * stride]);
        out[stride..stride + (n_chan - 1) * stride].copy_from_slice(&buf);
        out[0] = f16to8(last_v);
    }
    1
}

/// `PackPlanarWords` — C returns output+sizeof(u16)
pub fn pack_planar_words(
    i: &PackInfo,
    w: &[u16; crate::xform::MAX_CHANNELS],
    out: &mut [u8],
    stride: u32,
) -> usize {
    let n_chan = t_channels(i.output_format) as usize;
    let swap_endian = t_endian16(i.output_format) != 0;
    let do_swap = t_doswap(i.output_format) != 0;
    let reverse = t_flavor(i.output_format) != 0;
    let extra = t_extra(i.output_format) as usize;
    let swap_first = t_swapfirst(i.output_format) != 0;
    let premul = t_premul(i.output_format) != 0;
    let extra_first = do_swap ^ swap_first;
    let stride = stride as usize;
    let init = if extra_first { extra * stride } else { 0 };
    // C cmspack.c PackPlanarWords — the premul alpha comes out of the
    // output buffer's alpha plane (extra-first: plane 0; otherwise the
    // plane right after the color planes).
    let mut alpha_factor = 0u32;
    if premul && extra > 0 {
        alpha_factor = if extra_first {
            to_fixed_domain(rd16(out, 0))
        } else {
            to_fixed_domain(rd16(out, n_chan * stride))
        };
    }
    let mut last_v = 0u16;
    for c in 0..n_chan {
        let index = if do_swap { n_chan - c - 1 } else { c };
        let mut v = w[index];
        if swap_endian {
            v = v.swap_bytes();
        }
        if reverse {
            v = 0xffff - v;
        }
        if premul {
            v = (((v as u32) * alpha_factor + 0x8000) >> 16) as u16;
        }
        let o = init + c * stride;
        out[o] = (v & 0xff) as u8;
        out[o + 1] = (v >> 8) as u8;
        last_v = v;
    }
    if extra == 0 && swap_first {
        let mut buf = vec![0u8; (n_chan - 1) * stride];
        buf.copy_from_slice(&out[0..(n_chan - 1) * stride]);
        out[stride..stride + (n_chan - 1) * stride].copy_from_slice(&buf);
        out[0] = (last_v & 0xff) as u8;
        out[1] = (last_v >> 8) as u8;
    }
    2
}

// ---------------------------------------------------------------------------
// Specialized chunky packers
// ---------------------------------------------------------------------------

// specialized chunky packers (Pack* family)
pub fn pack6_bytes(
    _i: &PackInfo,
    w: &[u16; crate::xform::MAX_CHANNELS],
    out: &mut [u8],
    _s: u32,
) -> usize {
    let mut o = 0usize;
    out[o] = f16to8(w[0]);
    o += 1;
    out[o] = f16to8(w[1]);
    o += 1;
    out[o] = f16to8(w[2]);
    o += 1;
    out[o] = f16to8(w[3]);
    o += 1;
    out[o] = f16to8(w[4]);
    o += 1;
    out[o] = f16to8(w[5]);
    o += 1;
    o
}

pub fn pack6_bytes_swap(
    _i: &PackInfo,
    w: &[u16; crate::xform::MAX_CHANNELS],
    out: &mut [u8],
    _s: u32,
) -> usize {
    let mut o = 0usize;
    out[o] = f16to8(w[5]);
    o += 1;
    out[o] = f16to8(w[4]);
    o += 1;
    out[o] = f16to8(w[3]);
    o += 1;
    out[o] = f16to8(w[2]);
    o += 1;
    out[o] = f16to8(w[1]);
    o += 1;
    out[o] = f16to8(w[0]);
    o += 1;
    o
}

pub fn pack6_words(
    _i: &PackInfo,
    w: &[u16; crate::xform::MAX_CHANNELS],
    out: &mut [u8],
    _s: u32,
) -> usize {
    let mut o = 0usize;
    out[o] = (w[0] & 0xff) as u8;
    out[o + 1] = (w[0] >> 8) as u8;
    o += 2;
    out[o] = (w[1] & 0xff) as u8;
    out[o + 1] = (w[1] >> 8) as u8;
    o += 2;
    out[o] = (w[2] & 0xff) as u8;
    out[o + 1] = (w[2] >> 8) as u8;
    o += 2;
    out[o] = (w[3] & 0xff) as u8;
    out[o + 1] = (w[3] >> 8) as u8;
    o += 2;
    out[o] = (w[4] & 0xff) as u8;
    out[o + 1] = (w[4] >> 8) as u8;
    o += 2;
    out[o] = (w[5] & 0xff) as u8;
    out[o + 1] = (w[5] >> 8) as u8;
    o += 2;
    o
}

pub fn pack6_words_swap(
    _i: &PackInfo,
    w: &[u16; crate::xform::MAX_CHANNELS],
    out: &mut [u8],
    _s: u32,
) -> usize {
    let mut o = 0usize;
    out[o] = (w[5] & 0xff) as u8;
    out[o + 1] = (w[5] >> 8) as u8;
    o += 2;
    out[o] = (w[4] & 0xff) as u8;
    out[o + 1] = (w[4] >> 8) as u8;
    o += 2;
    out[o] = (w[3] & 0xff) as u8;
    out[o + 1] = (w[3] >> 8) as u8;
    o += 2;
    out[o] = (w[2] & 0xff) as u8;
    out[o + 1] = (w[2] >> 8) as u8;
    o += 2;
    out[o] = (w[1] & 0xff) as u8;
    out[o + 1] = (w[1] >> 8) as u8;
    o += 2;
    out[o] = (w[0] & 0xff) as u8;
    out[o + 1] = (w[0] >> 8) as u8;
    o += 2;
    o
}

/// `UnrollAnyWords` specialized to chunky 4ch+ex1 (TYPE_RGBA_16 & kin):
/// C reaches the generic AnyWords for this shape — the specialized fn
/// skips the per-pixel bitfield decode.
#[inline]
pub fn unroll4_words_skip1(
    _i: &PackInfo,
    w: &mut [u16; crate::xform::MAX_CHANNELS],
    a: &[u8],
    _s: u32,
) -> usize {
    w[0] = rd16(a, 0);
    w[1] = rd16(a, 2);
    w[2] = rd16(a, 4);
    w[3] = rd16(a, 6);
    10
}

/// `PackAnyWords` specialized to chunky 4ch+ex1 (extra bytes untouched —
/// `_cmsHandleExtraChannels` owns them).
#[inline]
pub fn pack4_words_skip1(
    _i: &PackInfo,
    w: &[u16; crate::xform::MAX_CHANNELS],
    out: &mut [u8],
    _s: u32,
) -> usize {
    out[0] = (w[0] & 0xff) as u8;
    out[1] = (w[0] >> 8) as u8;
    out[2] = (w[1] & 0xff) as u8;
    out[3] = (w[1] >> 8) as u8;
    out[4] = (w[2] & 0xff) as u8;
    out[5] = (w[2] >> 8) as u8;
    out[6] = (w[3] & 0xff) as u8;
    out[7] = (w[3] >> 8) as u8;
    10
}

/// `UnrollChunkyBytes` specialized to chunky 4ch+ex1 (TYPE_RGBA_8 & kin).
#[inline]
pub fn unroll4_bytes_skip1(
    _i: &PackInfo,
    w: &mut [u16; crate::xform::MAX_CHANNELS],
    a: &[u8],
    _s: u32,
) -> usize {
    w[0] = f8to16(a[0]);
    w[1] = f8to16(a[1]);
    w[2] = f8to16(a[2]);
    w[3] = f8to16(a[3]);
    5
}

/// `PackChunkyBytes` specialized to chunky 4ch+ex1.
#[inline]
pub fn pack4_bytes_skip1(
    _i: &PackInfo,
    w: &[u16; crate::xform::MAX_CHANNELS],
    out: &mut [u8],
    _s: u32,
) -> usize {
    out[0] = f16to8(w[0]);
    out[1] = f16to8(w[1]);
    out[2] = f16to8(w[2]);
    out[3] = f16to8(w[3]);
    5
}

pub fn pack4_bytes(
    _i: &PackInfo,
    w: &[u16; crate::xform::MAX_CHANNELS],
    out: &mut [u8],
    _s: u32,
) -> usize {
    let mut o = 0usize;
    out[o] = f16to8(w[0]);
    o += 1;
    out[o] = f16to8(w[1]);
    o += 1;
    out[o] = f16to8(w[2]);
    o += 1;
    out[o] = f16to8(w[3]);
    o += 1;
    o
}

pub fn pack4_bytes_reverse(
    _i: &PackInfo,
    w: &[u16; crate::xform::MAX_CHANNELS],
    out: &mut [u8],
    _s: u32,
) -> usize {
    let mut o = 0usize;
    out[o] = 0xff - f16to8(w[0]);
    o += 1;
    out[o] = 0xff - f16to8(w[1]);
    o += 1;
    out[o] = 0xff - f16to8(w[2]);
    o += 1;
    out[o] = 0xff - f16to8(w[3]);
    o += 1;
    o
}

pub fn pack4_bytes_swap_first(
    _i: &PackInfo,
    w: &[u16; crate::xform::MAX_CHANNELS],
    out: &mut [u8],
    _s: u32,
) -> usize {
    let mut o = 0usize;
    out[o] = f16to8(w[3]);
    o += 1;
    out[o] = f16to8(w[0]);
    o += 1;
    out[o] = f16to8(w[1]);
    o += 1;
    out[o] = f16to8(w[2]);
    o += 1;
    o
}

pub fn pack4_bytes_swap(
    _i: &PackInfo,
    w: &[u16; crate::xform::MAX_CHANNELS],
    out: &mut [u8],
    _s: u32,
) -> usize {
    let mut o = 0usize;
    out[o] = f16to8(w[3]);
    o += 1;
    out[o] = f16to8(w[2]);
    o += 1;
    out[o] = f16to8(w[1]);
    o += 1;
    out[o] = f16to8(w[0]);
    o += 1;
    o
}

pub fn pack4_bytes_swap_swap_first(
    _i: &PackInfo,
    w: &[u16; crate::xform::MAX_CHANNELS],
    out: &mut [u8],
    _s: u32,
) -> usize {
    let mut o = 0usize;
    out[o] = f16to8(w[2]);
    o += 1;
    out[o] = f16to8(w[1]);
    o += 1;
    out[o] = f16to8(w[0]);
    o += 1;
    out[o] = f16to8(w[3]);
    o += 1;
    o
}

pub fn pack4_words(
    _i: &PackInfo,
    w: &[u16; crate::xform::MAX_CHANNELS],
    out: &mut [u8],
    _s: u32,
) -> usize {
    let mut o = 0usize;
    out[o] = (w[0] & 0xff) as u8;
    out[o + 1] = (w[0] >> 8) as u8;
    o += 2;
    out[o] = (w[1] & 0xff) as u8;
    out[o + 1] = (w[1] >> 8) as u8;
    o += 2;
    out[o] = (w[2] & 0xff) as u8;
    out[o + 1] = (w[2] >> 8) as u8;
    o += 2;
    out[o] = (w[3] & 0xff) as u8;
    out[o + 1] = (w[3] >> 8) as u8;
    o += 2;
    o
}

pub fn pack4_words_reverse(
    _i: &PackInfo,
    w: &[u16; crate::xform::MAX_CHANNELS],
    out: &mut [u8],
    _s: u32,
) -> usize {
    let mut o = 0usize;
    let v = 0xffffu16 - w[0];
    out[o] = (v & 0xff) as u8;
    out[o + 1] = (v >> 8) as u8;
    o += 2;
    let v = 0xffffu16 - w[1];
    out[o] = (v & 0xff) as u8;
    out[o + 1] = (v >> 8) as u8;
    o += 2;
    let v = 0xffffu16 - w[2];
    out[o] = (v & 0xff) as u8;
    out[o + 1] = (v >> 8) as u8;
    o += 2;
    let v = 0xffffu16 - w[3];
    out[o] = (v & 0xff) as u8;
    out[o + 1] = (v >> 8) as u8;
    o += 2;
    o
}

pub fn pack4_words_swap(
    _i: &PackInfo,
    w: &[u16; crate::xform::MAX_CHANNELS],
    out: &mut [u8],
    _s: u32,
) -> usize {
    let mut o = 0usize;
    out[o] = (w[3] & 0xff) as u8;
    out[o + 1] = (w[3] >> 8) as u8;
    o += 2;
    out[o] = (w[2] & 0xff) as u8;
    out[o + 1] = (w[2] >> 8) as u8;
    o += 2;
    out[o] = (w[1] & 0xff) as u8;
    out[o + 1] = (w[1] >> 8) as u8;
    o += 2;
    out[o] = (w[0] & 0xff) as u8;
    out[o + 1] = (w[0] >> 8) as u8;
    o += 2;
    o
}

pub fn pack4_words_big_endian(
    _i: &PackInfo,
    w: &[u16; crate::xform::MAX_CHANNELS],
    out: &mut [u8],
    _s: u32,
) -> usize {
    let mut o = 0usize;
    out[o] = (w[0] >> 8) as u8;
    out[o + 1] = (w[0] & 0xff) as u8;
    o += 2;
    out[o] = (w[1] >> 8) as u8;
    out[o + 1] = (w[1] & 0xff) as u8;
    o += 2;
    out[o] = (w[2] >> 8) as u8;
    out[o + 1] = (w[2] & 0xff) as u8;
    o += 2;
    out[o] = (w[3] >> 8) as u8;
    out[o + 1] = (w[3] & 0xff) as u8;
    o += 2;
    o
}

pub fn pack3_bytes(
    _i: &PackInfo,
    w: &[u16; crate::xform::MAX_CHANNELS],
    out: &mut [u8],
    _s: u32,
) -> usize {
    let out = &mut out[..3];
    out[0] = f16to8(w[0]);
    out[1] = f16to8(w[1]);
    out[2] = f16to8(w[2]);
    3
}

pub fn pack3_bytes_swap(
    _i: &PackInfo,
    w: &[u16; crate::xform::MAX_CHANNELS],
    out: &mut [u8],
    _s: u32,
) -> usize {
    let out = &mut out[..3];
    let mut o = 0usize;
    out[o] = f16to8(w[2]);
    o += 1;
    out[o] = f16to8(w[1]);
    o += 1;
    out[o] = f16to8(w[0]);
    o += 1;
    o
}

pub fn pack3_words(
    _i: &PackInfo,
    w: &[u16; crate::xform::MAX_CHANNELS],
    out: &mut [u8],
    _s: u32,
) -> usize {
    let out = &mut out[..6];
    let mut o = 0usize;
    out[o] = (w[0] & 0xff) as u8;
    out[o + 1] = (w[0] >> 8) as u8;
    o += 2;
    out[o] = (w[1] & 0xff) as u8;
    out[o + 1] = (w[1] >> 8) as u8;
    o += 2;
    out[o] = (w[2] & 0xff) as u8;
    out[o + 1] = (w[2] >> 8) as u8;
    o += 2;
    o
}

pub fn pack3_words_swap(
    _i: &PackInfo,
    w: &[u16; crate::xform::MAX_CHANNELS],
    out: &mut [u8],
    _s: u32,
) -> usize {
    let out = &mut out[..6];
    let mut o = 0usize;
    out[o] = (w[2] & 0xff) as u8;
    out[o + 1] = (w[2] >> 8) as u8;
    o += 2;
    out[o] = (w[1] & 0xff) as u8;
    out[o + 1] = (w[1] >> 8) as u8;
    o += 2;
    out[o] = (w[0] & 0xff) as u8;
    out[o + 1] = (w[0] >> 8) as u8;
    o += 2;
    o
}

pub fn pack3_words_big_endian(
    _i: &PackInfo,
    w: &[u16; crate::xform::MAX_CHANNELS],
    out: &mut [u8],
    _s: u32,
) -> usize {
    let out = &mut out[..6];
    let mut o = 0usize;
    out[o] = (w[0] >> 8) as u8;
    out[o + 1] = (w[0] & 0xff) as u8;
    o += 2;
    out[o] = (w[1] >> 8) as u8;
    out[o + 1] = (w[1] & 0xff) as u8;
    o += 2;
    out[o] = (w[2] >> 8) as u8;
    out[o + 1] = (w[2] & 0xff) as u8;
    o += 2;
    o
}

pub fn pack3_bytes_and_skip1(
    _i: &PackInfo,
    w: &[u16; crate::xform::MAX_CHANNELS],
    out: &mut [u8],
    _s: u32,
) -> usize {
    let mut o = 0usize;
    out[o] = f16to8(w[0]);
    o += 1;
    out[o] = f16to8(w[1]);
    o += 1;
    out[o] = f16to8(w[2]);
    o += 1;
    o += 1;
    o
}

pub fn pack3_bytes_and_skip1_swap_first(
    _i: &PackInfo,
    w: &[u16; crate::xform::MAX_CHANNELS],
    out: &mut [u8],
    _s: u32,
) -> usize {
    let mut o = 0usize;
    o += 1;
    out[o] = f16to8(w[0]);
    o += 1;
    out[o] = f16to8(w[1]);
    o += 1;
    out[o] = f16to8(w[2]);
    o += 1;
    o
}

pub fn pack3_bytes_and_skip1_swap(
    _i: &PackInfo,
    w: &[u16; crate::xform::MAX_CHANNELS],
    out: &mut [u8],
    _s: u32,
) -> usize {
    let mut o = 0usize;
    o += 1;
    out[o] = f16to8(w[2]);
    o += 1;
    out[o] = f16to8(w[1]);
    o += 1;
    out[o] = f16to8(w[0]);
    o += 1;
    o
}

pub fn pack3_bytes_and_skip1_swap_swap_first(
    _i: &PackInfo,
    w: &[u16; crate::xform::MAX_CHANNELS],
    out: &mut [u8],
    _s: u32,
) -> usize {
    let mut o = 0usize;
    out[o] = f16to8(w[2]);
    o += 1;
    out[o] = f16to8(w[1]);
    o += 1;
    out[o] = f16to8(w[0]);
    o += 1;
    o += 1;
    o
}

pub fn pack3_words_and_skip1(
    _i: &PackInfo,
    w: &[u16; crate::xform::MAX_CHANNELS],
    out: &mut [u8],
    _s: u32,
) -> usize {
    let mut o = 0usize;
    out[o] = (w[0] & 0xff) as u8;
    out[o + 1] = (w[0] >> 8) as u8;
    o += 2;
    out[o] = (w[1] & 0xff) as u8;
    out[o + 1] = (w[1] >> 8) as u8;
    o += 2;
    out[o] = (w[2] & 0xff) as u8;
    out[o + 1] = (w[2] >> 8) as u8;
    o += 2;
    o += 2;
    o
}

pub fn pack3_words_and_skip1_swap(
    _i: &PackInfo,
    w: &[u16; crate::xform::MAX_CHANNELS],
    out: &mut [u8],
    _s: u32,
) -> usize {
    let mut o = 0usize;
    o += 2;
    out[o] = (w[2] & 0xff) as u8;
    out[o + 1] = (w[2] >> 8) as u8;
    o += 2;
    out[o] = (w[1] & 0xff) as u8;
    out[o + 1] = (w[1] >> 8) as u8;
    o += 2;
    out[o] = (w[0] & 0xff) as u8;
    out[o + 1] = (w[0] >> 8) as u8;
    o += 2;
    o
}

pub fn pack3_words_and_skip1_swap_first(
    _i: &PackInfo,
    w: &[u16; crate::xform::MAX_CHANNELS],
    out: &mut [u8],
    _s: u32,
) -> usize {
    let mut o = 0usize;
    o += 2;
    out[o] = (w[0] & 0xff) as u8;
    out[o + 1] = (w[0] >> 8) as u8;
    o += 2;
    out[o] = (w[1] & 0xff) as u8;
    out[o + 1] = (w[1] >> 8) as u8;
    o += 2;
    out[o] = (w[2] & 0xff) as u8;
    out[o + 1] = (w[2] >> 8) as u8;
    o += 2;
    o
}

pub fn pack3_words_and_skip1_swap_swap_first(
    _i: &PackInfo,
    w: &[u16; crate::xform::MAX_CHANNELS],
    out: &mut [u8],
    _s: u32,
) -> usize {
    let mut o = 0usize;
    out[o] = (w[2] & 0xff) as u8;
    out[o + 1] = (w[2] >> 8) as u8;
    o += 2;
    out[o] = (w[1] & 0xff) as u8;
    out[o + 1] = (w[1] >> 8) as u8;
    o += 2;
    out[o] = (w[0] & 0xff) as u8;
    out[o + 1] = (w[0] >> 8) as u8;
    o += 2;
    o += 2;
    o
}

pub fn pack1_byte(
    _i: &PackInfo,
    w: &[u16; crate::xform::MAX_CHANNELS],
    out: &mut [u8],
    _s: u32,
) -> usize {
    let mut o = 0usize;
    out[o] = f16to8(w[0]);
    o += 1;
    o
}

pub fn pack1_byte_reversed(
    _i: &PackInfo,
    w: &[u16; crate::xform::MAX_CHANNELS],
    out: &mut [u8],
    _s: u32,
) -> usize {
    let mut o = 0usize;
    out[o] = 0xff - f16to8(w[0]);
    o += 1;
    o
}

pub fn pack1_byte_skip1(
    _i: &PackInfo,
    w: &[u16; crate::xform::MAX_CHANNELS],
    out: &mut [u8],
    _s: u32,
) -> usize {
    let mut o = 0usize;
    out[o] = f16to8(w[0]);
    o += 1;
    o += 1;
    o
}

pub fn pack1_byte_skip1_swap_first(
    _i: &PackInfo,
    w: &[u16; crate::xform::MAX_CHANNELS],
    out: &mut [u8],
    _s: u32,
) -> usize {
    let mut o = 0usize;
    o += 1;
    out[o] = f16to8(w[0]);
    o += 1;
    o
}

pub fn pack1_word(
    _i: &PackInfo,
    w: &[u16; crate::xform::MAX_CHANNELS],
    out: &mut [u8],
    _s: u32,
) -> usize {
    let mut o = 0usize;
    out[o] = (w[0] & 0xff) as u8;
    out[o + 1] = (w[0] >> 8) as u8;
    o += 2;
    o
}

pub fn pack1_word_reversed(
    _i: &PackInfo,
    w: &[u16; crate::xform::MAX_CHANNELS],
    out: &mut [u8],
    _s: u32,
) -> usize {
    let mut o = 0usize;
    let v = 0xffffu16 - w[0];
    out[o] = (v & 0xff) as u8;
    out[o + 1] = (v >> 8) as u8;
    o += 2;
    o
}

pub fn pack1_word_big_endian(
    _i: &PackInfo,
    w: &[u16; crate::xform::MAX_CHANNELS],
    out: &mut [u8],
    _s: u32,
) -> usize {
    let mut o = 0usize;
    out[o] = (w[0] >> 8) as u8;
    out[o + 1] = (w[0] & 0xff) as u8;
    o += 2;
    o
}

pub fn pack1_word_skip1(
    _i: &PackInfo,
    w: &[u16; crate::xform::MAX_CHANNELS],
    out: &mut [u8],
    _s: u32,
) -> usize {
    let mut o = 0usize;
    out[o] = (w[0] & 0xff) as u8;
    out[o + 1] = (w[0] >> 8) as u8;
    o += 2;
    o += 2;
    o
}

pub fn pack1_word_skip1_swap_first(
    _i: &PackInfo,
    w: &[u16; crate::xform::MAX_CHANNELS],
    out: &mut [u8],
    _s: u32,
) -> usize {
    let mut o = 0usize;
    o += 2;
    out[o] = (w[0] & 0xff) as u8;
    out[o + 1] = (w[0] >> 8) as u8;
    o += 2;
    o
}

// ---------------------------------------------------------------------------
// Lab v2 encode packers + Lab/XYZ/float/double from-16
// ---------------------------------------------------------------------------

pub fn pack_lab_v2_8(
    _i: &PackInfo,
    w: &[u16; crate::xform::MAX_CHANNELS],
    out: &mut [u8],
    _s: u32,
) -> usize {
    out[0] = f16to8(fom_lab_v4_to_v2(w[0]));
    out[1] = f16to8(fom_lab_v4_to_v2(w[1]));
    out[2] = f16to8(fom_lab_v4_to_v2(w[2]));
    3
}
pub fn pack_alab_v2_8(
    _i: &PackInfo,
    w: &[u16; crate::xform::MAX_CHANNELS],
    out: &mut [u8],
    _s: u32,
) -> usize {
    out[1] = f16to8(fom_lab_v4_to_v2(w[0]));
    out[2] = f16to8(fom_lab_v4_to_v2(w[1]));
    out[3] = f16to8(fom_lab_v4_to_v2(w[2]));
    4
}
pub fn pack_lab_v2_16(
    _i: &PackInfo,
    w: &[u16; crate::xform::MAX_CHANNELS],
    out: &mut [u8],
    _s: u32,
) -> usize {
    for (c, &v) in w[..3].iter().enumerate() {
        let x = fom_lab_v4_to_v2(v);
        out[c * 2] = (x & 0xff) as u8;
        out[c * 2 + 1] = (x >> 8) as u8;
    }
    6
}
pub fn pack_lab_double_from16(
    i: &PackInfo,
    w: &[u16; crate::xform::MAX_CHANNELS],
    out: &mut [u8],
    stride: u32,
) -> usize {
    let lab = crate::pcs::lab_encoded2float([w[0], w[1], w[2]]);
    if t_planar(i.output_format) != 0 {
        let s = stride as usize / pixel_size(i.output_format) as usize;
        out[0..8].copy_from_slice(&lab.L.to_le_bytes());
        out[s * 8..s * 8 + 8].copy_from_slice(&lab.a.to_le_bytes());
        out[s * 16..s * 16 + 8].copy_from_slice(&lab.b.to_le_bytes());
        8
    } else {
        out[0..8].copy_from_slice(&lab.L.to_le_bytes());
        out[8..16].copy_from_slice(&lab.a.to_le_bytes());
        out[16..24].copy_from_slice(&lab.b.to_le_bytes());
        24 + t_extra(i.output_format) as usize * 8
    }
}
pub fn pack_lab_float_from16(
    i: &PackInfo,
    w: &[u16; crate::xform::MAX_CHANNELS],
    out: &mut [u8],
    stride: u32,
) -> usize {
    let lab = crate::pcs::lab_encoded2float([w[0], w[1], w[2]]);
    if t_planar(i.output_format) != 0 {
        let s = stride as usize / pixel_size(i.output_format) as usize;
        out[0..4].copy_from_slice(&(lab.L as f32).to_le_bytes());
        out[s * 4..s * 4 + 4].copy_from_slice(&(lab.a as f32).to_le_bytes());
        out[s * 8..s * 8 + 4].copy_from_slice(&(lab.b as f32).to_le_bytes());
        4
    } else {
        out[0..4].copy_from_slice(&(lab.L as f32).to_le_bytes());
        out[4..8].copy_from_slice(&(lab.a as f32).to_le_bytes());
        out[8..12].copy_from_slice(&(lab.b as f32).to_le_bytes());
        (3 + t_extra(i.output_format) as usize) * 4
    }
}
pub fn pack_xyz_double_from16(
    i: &PackInfo,
    w: &[u16; crate::xform::MAX_CHANNELS],
    out: &mut [u8],
    stride: u32,
) -> usize {
    let xyz = crate::pcs::xyz_encoded2float([w[0], w[1], w[2]]);
    if t_planar(i.output_format) != 0 {
        let s = stride as usize / pixel_size(i.output_format) as usize;
        out[0..8].copy_from_slice(&xyz.X.to_le_bytes());
        out[s * 8..s * 8 + 8].copy_from_slice(&xyz.Y.to_le_bytes());
        out[s * 16..s * 16 + 8].copy_from_slice(&xyz.Z.to_le_bytes());
        8
    } else {
        out[0..8].copy_from_slice(&xyz.X.to_le_bytes());
        out[8..16].copy_from_slice(&xyz.Y.to_le_bytes());
        out[16..24].copy_from_slice(&xyz.Z.to_le_bytes());
        24 + t_extra(i.output_format) as usize * 8
    }
}
pub fn pack_xyz_float_from16(
    i: &PackInfo,
    w: &[u16; crate::xform::MAX_CHANNELS],
    out: &mut [u8],
    stride: u32,
) -> usize {
    let xyz = crate::pcs::xyz_encoded2float([w[0], w[1], w[2]]);
    if t_planar(i.output_format) != 0 {
        let s = stride as usize / pixel_size(i.output_format) as usize;
        out[0..4].copy_from_slice(&(xyz.X as f32).to_le_bytes());
        out[s * 4..s * 4 + 4].copy_from_slice(&(xyz.Y as f32).to_le_bytes());
        out[s * 8..s * 8 + 4].copy_from_slice(&(xyz.Z as f32).to_le_bytes());
        4
    } else {
        out[0..4].copy_from_slice(&(xyz.X as f32).to_le_bytes());
        out[4..8].copy_from_slice(&(xyz.Y as f32).to_le_bytes());
        out[8..12].copy_from_slice(&(xyz.Z as f32).to_le_bytes());
        12 + t_extra(i.output_format) as usize * 4
    }
}

/// `PackDoubleFrom16` / `PackFloatFrom16` — generic numeric write
fn pack_fd_from16(
    i: &PackInfo,
    w: &[u16; crate::xform::MAX_CHANNELS],
    out: &mut [u8],
    stride: u32,
    is_double: bool,
) -> usize {
    let n_chan = t_channels(i.output_format) as usize;
    let do_swap = t_doswap(i.output_format) != 0;
    let reverse = t_flavor(i.output_format) != 0;
    let extra = t_extra(i.output_format) as usize;
    let swap_first = t_swapfirst(i.output_format) != 0;
    let extra_first = do_swap ^ swap_first;
    let planar = t_planar(i.output_format) != 0;
    let premul = t_premul(i.output_format) != 0;
    let esz = if is_double { 8usize } else { 4usize };
    let maximum = if is_ink_space(i.output_format) {
        100.0
    } else {
        1.0
    };
    let stride_elems = stride as usize / pixel_size(i.output_format) as usize;
    let init = if extra_first { extra } else { 0 };
    // alpha written back into out if premul
    let mut alpha_factor = 1.0f64;
    if premul && extra > 0 {
        let o = if planar {
            if extra_first {
                0
            } else {
                n_chan * stride_elems * esz
            }
        } else if extra_first {
            0
        } else {
            n_chan * esz
        };
        alpha_factor = if is_double {
            rd64f(out, o)
        } else {
            rd32f(out, o) as f64
        };
    }
    for c in 0..n_chan {
        let index = if do_swap { n_chan - c - 1 } else { c };
        let mut v = w[index] as f64 * maximum / 65535.0;
        if premul {
            v = v * alpha_factor / maximum;
        }
        if reverse {
            v = maximum - v;
        }
        let o = if planar {
            (c + init) * stride_elems * esz
        } else {
            (c + init) * esz
        };
        if is_double {
            out[o..o + 8].copy_from_slice(&v.to_le_bytes());
        } else {
            out[o..o + 4].copy_from_slice(&(v as f32).to_le_bytes());
        }
    }
    if planar { esz } else { (n_chan + extra) * esz }
}
pub fn pack_double_from16(
    i: &PackInfo,
    w: &[u16; crate::xform::MAX_CHANNELS],
    out: &mut [u8],
    s: u32,
) -> usize {
    pack_fd_from16(i, w, out, s, true)
}
pub fn pack_float_from16(
    i: &PackInfo,
    w: &[u16; crate::xform::MAX_CHANNELS],
    out: &mut [u8],
    s: u32,
) -> usize {
    pack_fd_from16(i, w, out, s, false)
}

// ---------------------------------------------------------------------------
// f32 → bytes/words/floats/doubles output
// ---------------------------------------------------------------------------

fn pack_fd_from_float(
    i: &PackInfo,
    w: &[f32; crate::xform::MAX_CHANNELS],
    out: &mut [u8],
    stride: u32,
    esz: usize,
) -> usize {
    let n_chan = t_channels(i.output_format) as usize;
    let do_swap = t_doswap(i.output_format) != 0;
    let reverse = t_flavor(i.output_format) != 0;
    let extra = t_extra(i.output_format) as usize;
    let swap_first = t_swapfirst(i.output_format) != 0;
    let planar = t_planar(i.output_format) != 0;
    let extra_first = do_swap ^ swap_first;
    let maximum = if is_ink_space(i.output_format) {
        100.0
    } else {
        1.0
    };
    let stride_elems = stride as usize / pixel_size(i.output_format) as usize;
    let start = if extra_first { extra } else { 0 };
    // Interleaved f32 → direct stores — the dominant float-transform
    // output format.
    if esz == 4 && !do_swap && !reverse && !planar && !swap_first && maximum == 1.0 {
        for c in 0..n_chan {
            out[(c + start) * 4..(c + start) * 4 + 4].copy_from_slice(&w[c].to_le_bytes());
        }
        return (n_chan + extra) * 4;
    }
    // u8/u16 float output — same `qs16`/`f16to8` op chain, no decode.
    if esz == 1 && !do_swap && !reverse && !planar && !swap_first && maximum == 1.0 {
        for c in 0..n_chan {
            out[c + start] = f16to8(qs16(w[c] as f64 * 65535.0));
        }
        return n_chan + extra;
    }
    if esz == 2 && !do_swap && !reverse && !planar && !swap_first && maximum == 1.0 {
        for c in 0..n_chan {
            let v = qs16(w[c] as f64 * 65535.0);
            let o = (c + start) * 2;
            out[o] = (v & 0xff) as u8;
            out[o + 1] = (v >> 8) as u8;
        }
        return (n_chan + extra) * 2;
    }
    let mut last_v: f64 = 0.0;
    for c in 0..n_chan {
        let index = if do_swap { n_chan - c - 1 } else { c };
        let mut v = w[index] as f64 * maximum;
        if reverse {
            v = maximum - v;
        }
        let o = if planar {
            (c + start) * stride_elems * esz
        } else {
            (c + start) * esz
        };
        match esz {
            1 => {
                let vv = f16to8(qs16(
                    w[index] as f64 * 65535.0 * if reverse { -1.0 } else { 1.0 }
                        + if reverse { 65535.0 } else { 0.0 },
                ));
                out[o] = vv;
                last_v = vv as f64;
            }
            2 => {
                let vv = qs16(
                    w[index] as f64 * 65535.0 * if reverse { -1.0 } else { 1.0 }
                        + if reverse { 65535.0 } else { 0.0 },
                );
                out[o] = (vv & 0xff) as u8;
                out[o + 1] = (vv >> 8) as u8;
                last_v = vv as f64;
            }
            4 => {
                out[o..o + 4].copy_from_slice(&(v as f32).to_le_bytes());
                last_v = v;
            }
            _ => {
                out[o..o + 8].copy_from_slice(&v.to_le_bytes());
                last_v = v;
            }
        }
    }
    if extra == 0 && swap_first {
        // C: memmove(swap1+esz, swap1, (nChan-1)*esz); *swap1 = v
        // (n_chan-1)*esz <= 14*8 — stack buffer, no per-pixel alloc.
        let mut buf = [0u8; 14 * 8];
        let n = (n_chan - 1) * esz;
        buf[..n].copy_from_slice(&out[0..n]);
        out[esz..esz + n].copy_from_slice(&buf[..n]);
        match esz {
            1 => out[0] = last_v as u8,
            2 => {
                let v = last_v as u16;
                out[0] = (v & 0xff) as u8;
                out[1] = (v >> 8) as u8;
            }
            4 => out[0..4].copy_from_slice(&(last_v as f32).to_le_bytes()),
            _ => out[0..8].copy_from_slice(&last_v.to_le_bytes()),
        }
    }
    if planar { esz } else { (n_chan + extra) * esz }
}
pub fn pack_bytes_from_float(
    i: &PackInfo,
    w: &[f32; crate::xform::MAX_CHANNELS],
    out: &mut [u8],
    s: u32,
) -> usize {
    pack_fd_from_float(i, w, out, s, 1)
}
pub fn pack_words_from_float(
    i: &PackInfo,
    w: &[f32; crate::xform::MAX_CHANNELS],
    out: &mut [u8],
    s: u32,
) -> usize {
    pack_fd_from_float(i, w, out, s, 2)
}
pub fn pack_floats_from_float(
    i: &PackInfo,
    w: &[f32; crate::xform::MAX_CHANNELS],
    out: &mut [u8],
    s: u32,
) -> usize {
    pack_fd_from_float(i, w, out, s, 4)
}
pub fn pack_doubles_from_float(
    i: &PackInfo,
    w: &[f32; crate::xform::MAX_CHANNELS],
    out: &mut [u8],
    s: u32,
) -> usize {
    pack_fd_from_float(i, w, out, s, 8)
}

// ---------------------------------------------------------------------------
// Lab/XYZ float + LabV2 encode from f32
// ---------------------------------------------------------------------------

pub fn pack_lab_float_from_float(
    i: &PackInfo,
    w: &[f32; crate::xform::MAX_CHANNELS],
    out: &mut [u8],
    stride: u32,
) -> usize {
    let l = (w[0] as f64 * 100.0) as f32;
    let a = (w[1] as f64 * 255.0 - 128.0) as f32;
    let b = (w[2] as f64 * 255.0 - 128.0) as f32;
    if t_planar(i.output_format) != 0 {
        let s = stride as usize / pixel_size(i.output_format) as usize;
        out[0..4].copy_from_slice(&l.to_le_bytes());
        out[s * 4..s * 4 + 4].copy_from_slice(&a.to_le_bytes());
        out[s * 8..s * 8 + 4].copy_from_slice(&b.to_le_bytes());
        4
    } else {
        out[0..4].copy_from_slice(&l.to_le_bytes());
        out[4..8].copy_from_slice(&a.to_le_bytes());
        out[8..12].copy_from_slice(&b.to_le_bytes());
        12 + t_extra(i.output_format) as usize * 4
    }
}
pub fn pack_lab_double_from_float(
    i: &PackInfo,
    w: &[f32; crate::xform::MAX_CHANNELS],
    out: &mut [u8],
    stride: u32,
) -> usize {
    let l = w[0] as f64 * 100.0;
    let a = w[1] as f64 * 255.0 - 128.0;
    let b = w[2] as f64 * 255.0 - 128.0;
    if t_planar(i.output_format) != 0 {
        let s = stride as usize / pixel_size(i.output_format) as usize;
        out[0..8].copy_from_slice(&l.to_le_bytes());
        out[s * 8..s * 8 + 8].copy_from_slice(&a.to_le_bytes());
        out[s * 16..s * 16 + 8].copy_from_slice(&b.to_le_bytes());
        8
    } else {
        out[0..8].copy_from_slice(&l.to_le_bytes());
        out[8..16].copy_from_slice(&a.to_le_bytes());
        out[16..24].copy_from_slice(&b.to_le_bytes());
        24 + t_extra(i.output_format) as usize * 8
    }
}
pub fn pack_xyz_float_from_float(
    i: &PackInfo,
    w: &[f32; crate::xform::MAX_CHANNELS],
    out: &mut [u8],
    stride: u32,
) -> usize {
    let (x, y, z) = (
        w[0] * MAX_ENC_XYZ as f32,
        w[1] * MAX_ENC_XYZ as f32,
        w[2] * MAX_ENC_XYZ as f32,
    );
    if t_planar(i.output_format) != 0 {
        let s = stride as usize / pixel_size(i.output_format) as usize;
        out[0..4].copy_from_slice(&x.to_le_bytes());
        out[s * 4..s * 4 + 4].copy_from_slice(&y.to_le_bytes());
        out[s * 8..s * 8 + 4].copy_from_slice(&z.to_le_bytes());
        4
    } else {
        out[0..4].copy_from_slice(&x.to_le_bytes());
        out[4..8].copy_from_slice(&y.to_le_bytes());
        out[8..12].copy_from_slice(&z.to_le_bytes());
        12 + t_extra(i.output_format) as usize * 4
    }
}
pub fn pack_xyz_double_from_float(
    i: &PackInfo,
    w: &[f32; crate::xform::MAX_CHANNELS],
    out: &mut [u8],
    stride: u32,
) -> usize {
    let (x, y, z) = (
        w[0] as f64 * MAX_ENC_XYZ,
        w[1] as f64 * MAX_ENC_XYZ,
        w[2] as f64 * MAX_ENC_XYZ,
    );
    if t_planar(i.output_format) != 0 {
        let s = stride as usize / pixel_size(i.output_format) as usize;
        out[0..8].copy_from_slice(&x.to_le_bytes());
        out[s * 8..s * 8 + 8].copy_from_slice(&y.to_le_bytes());
        out[s * 16..s * 16 + 8].copy_from_slice(&z.to_le_bytes());
        8
    } else {
        out[0..8].copy_from_slice(&x.to_le_bytes());
        out[8..16].copy_from_slice(&y.to_le_bytes());
        out[16..24].copy_from_slice(&z.to_le_bytes());
        24 + t_extra(i.output_format) as usize * 8
    }
}

/// `PackEncodedBytesLabV2FromFloat` — f32 Lab → v2 encoded bytes
pub fn pack_encoded_bytes_lab_v2_from_float(
    i: &PackInfo,
    w: &[f32; crate::xform::MAX_CHANNELS],
    out: &mut [u8],
    stride: u32,
) -> usize {
    // C cmspack.c:3224 — v4-encode then TRUNCATE to the high byte
    // (no v4→v2 conversion, no rounding).
    let wlab = crate::pcs::float2lab_encoded(&crate::types::CieLab {
        L: w[0] as f64 * 100.0,
        a: w[1] as f64 * 255.0 - 128.0,
        b: w[2] as f64 * 255.0 - 128.0,
    });
    if t_planar(i.output_format) != 0 {
        let s = (stride / pixel_size(i.output_format)) as usize;
        out[0] = (wlab[0] >> 8) as u8;
        out[s] = (wlab[1] >> 8) as u8;
        out[2 * s] = (wlab[2] >> 8) as u8;
        1
    } else {
        out[0] = (wlab[0] >> 8) as u8;
        out[1] = (wlab[1] >> 8) as u8;
        out[2] = (wlab[2] >> 8) as u8;
        3 + t_extra(i.output_format) as usize
    }
}
/// `PackEncodedWordsLabV2FromFloat`
pub fn pack_encoded_words_lab_v2_from_float(
    i: &PackInfo,
    w: &[f32; crate::xform::MAX_CHANNELS],
    out: &mut [u8],
    stride: u32,
) -> usize {
    // C cmspack.c:3259 — direct v2 encoding (not v4→v2), LE words.
    let wlab = crate::pcs::float2lab_encoded_v2(&crate::types::CieLab {
        L: w[0] as f64 * 100.0,
        a: w[1] as f64 * 255.0 - 128.0,
        b: w[2] as f64 * 255.0 - 128.0,
    });
    if t_planar(i.output_format) != 0 {
        let s = (stride / pixel_size(i.output_format)) as usize;
        out[0..2].copy_from_slice(&wlab[0].to_le_bytes());
        out[s * 2..s * 2 + 2].copy_from_slice(&wlab[1].to_le_bytes());
        out[s * 4..s * 4 + 2].copy_from_slice(&wlab[2].to_le_bytes());
        2
    } else {
        for c in 0..3 {
            out[c * 2..c * 2 + 2].copy_from_slice(&wlab[c].to_le_bytes());
        }
        (3 + t_extra(i.output_format) as usize) * 2
    }
}

// ---------------------------------------------------------------------------
// Half-float
// ---------------------------------------------------------------------------

/// `UnrollHalfTo16` — generic half → u16 decoder
pub fn unroll_half_to16(
    i: &PackInfo,
    w: &mut [u16; crate::xform::MAX_CHANNELS],
    a: &[u8],
    stride: u32,
) -> usize {
    let n_chan = t_channels(i.input_format) as usize;
    let do_swap = t_doswap(i.input_format) != 0;
    let reverse = t_flavor(i.input_format) != 0;
    let swap_first = t_swapfirst(i.input_format) != 0;
    let extra = t_extra(i.input_format) as usize;
    let extra_first = do_swap ^ swap_first;
    let maximum = if is_ink_space(i.input_format) {
        655.35
    } else {
        65535.0
    };
    // C's `UnrollHalfTo16` — PLANAR puts each channel plane `stride` bytes
    // apart; otherwise channels are contiguous (2 bytes each).
    let planar = t_planar(i.input_format) != 0;
    let mut o = if extra_first {
        if planar {
            extra * stride as usize
        } else {
            extra * 2
        }
    } else {
        0
    };
    for c in 0..n_chan {
        let index = if do_swap { n_chan - c - 1 } else { c };
        let off = if planar { o + c * stride as usize } else { o };
        let v = qs16(crate::half::half2float(rd16(a, off)) as f64 * maximum);
        w[index] = if reverse { 0xffff - v } else { v };
        if !planar {
            o += 2;
        }
    }
    if extra == 0 && swap_first {
        let tmp = w[0];
        w.copy_within(1..n_chan, 0);
        w[n_chan - 1] = tmp;
    }
    if planar {
        return 2; // advance one plane element (like `UnrollPlanarWords`)
    }
    if !extra_first {
        o += extra * 2;
    }
    o
}

/// `UnrollHalfToFloat`
pub fn unroll_half_to_float(
    i: &PackInfo,
    w: &mut [f32; crate::xform::MAX_CHANNELS],
    a: &[u8],
    stride: u32,
) -> usize {
    let n_chan = t_channels(i.input_format) as usize;
    let do_swap = t_doswap(i.input_format) != 0;
    let reverse = t_flavor(i.input_format) != 0;
    let swap_first = t_swapfirst(i.input_format) != 0;
    let extra = t_extra(i.input_format) as usize;
    let extra_first = do_swap ^ swap_first;
    let maximum = if is_ink_space(i.input_format) {
        100.0
    } else {
        1.0
    };
    // PLANAR puts each channel plane `stride` bytes apart (see `unroll_half_to16`)
    let planar = t_planar(i.input_format) != 0;
    let mut o = if extra_first {
        if planar {
            extra * stride as usize
        } else {
            extra * 2
        }
    } else {
        0
    };
    for c in 0..n_chan {
        let index = if do_swap { n_chan - c - 1 } else { c };
        let off = if planar { o + c * stride as usize } else { o };
        let v = crate::half::half2float(rd16(a, off)) / maximum as f32;
        w[index] = if reverse { 1.0 - v } else { v };
        if !planar {
            o += 2;
        }
    }
    if extra == 0 && swap_first {
        let tmp = w[0];
        w.copy_within(1..n_chan, 0);
        w[n_chan - 1] = tmp;
    }
    if planar {
        return 2;
    }
    if !extra_first {
        o += extra * 2;
    }
    o
}

/// `PackHalfFrom16`
pub fn pack_half_from16(
    i: &PackInfo,
    w: &[u16; crate::xform::MAX_CHANNELS],
    out: &mut [u8],
    _s: u32,
) -> usize {
    let n_chan = t_channels(i.output_format) as usize;
    let do_swap = t_doswap(i.output_format) != 0;
    let reverse = t_flavor(i.output_format) != 0;
    let swap_first = t_swapfirst(i.output_format) != 0;
    let extra = t_extra(i.output_format) as usize;
    let extra_first = do_swap ^ swap_first;
    let maximum = if is_ink_space(i.output_format) {
        655.35f32
    } else {
        65535.0
    };
    let mut o = if extra_first { extra * 2 } else { 0 };
    let mut last = 0f32;
    for c in 0..n_chan {
        let index = if do_swap { n_chan - c - 1 } else { c };
        // C: v = wOut/maximum; Reverse → v = maximum - v (post-division)
        let v = if reverse {
            maximum - w[index] as f32 / maximum
        } else {
            w[index] as f32 / maximum
        };
        let h = crate::half::float2half(v);
        out[o] = (h & 0xff) as u8;
        out[o + 1] = (h >> 8) as u8;
        o += 2;
        last = v;
    }
    if !extra_first {
        o += extra * 2;
    }
    if extra == 0 && swap_first {
        let mut buf = vec![0u8; (n_chan - 1) * 2];
        buf.copy_from_slice(&out[0..(n_chan - 1) * 2]);
        out[2..2 + (n_chan - 1) * 2].copy_from_slice(&buf);
        let h = crate::half::float2half(last);
        out[0] = (h & 0xff) as u8;
        out[1] = (h >> 8) as u8;
    }
    o
}

/// `PackHalfFromFloat`
pub fn pack_half_from_float(
    i: &PackInfo,
    w: &[f32; crate::xform::MAX_CHANNELS],
    out: &mut [u8],
    _s: u32,
) -> usize {
    let n_chan = t_channels(i.output_format) as usize;
    let do_swap = t_doswap(i.output_format) != 0;
    let reverse = t_flavor(i.output_format) != 0;
    let swap_first = t_swapfirst(i.output_format) != 0;
    let extra = t_extra(i.output_format) as usize;
    let extra_first = do_swap ^ swap_first;
    let maximum = if is_ink_space(i.output_format) {
        100.0f32
    } else {
        1.0
    };
    let mut o = if extra_first { extra * 2 } else { 0 };
    let mut last = 0.0f32;
    for c in 0..n_chan {
        let index = if do_swap { n_chan - c - 1 } else { c };
        let v = if reverse {
            maximum - w[index] * maximum
        } else {
            w[index] * maximum
        };
        let h = crate::half::float2half(v);
        out[o] = (h & 0xff) as u8;
        out[o + 1] = (h >> 8) as u8;
        o += 2;
        last = v;
    }
    if !extra_first {
        o += extra * 2;
    }
    if extra == 0 && swap_first {
        let mut buf = vec![0u8; (n_chan - 1) * 2];
        buf.copy_from_slice(&out[0..(n_chan - 1) * 2]);
        out[2..2 + (n_chan - 1) * 2].copy_from_slice(&buf);
        let h = crate::half::float2half(last);
        out[0] = (h & 0xff) as u8;
        out[1] = (h >> 8) as u8;
    }
    o
}

// ---------------------------------------------------------------------------
// Formatter tables + `_cmsGetFormatter` dispatch
// ---------------------------------------------------------------------------

const fn cs_sh(c: u32) -> u32 {
    c << 16
}
const fn ch_sh(c: u32) -> u32 {
    c << 3
}
const fn by_sh(b: u32) -> u32 {
    b
}
const fn fl_sh(b: u32) -> u32 {
    b << 22
}
const fn sf_sh(b: u32) -> u32 {
    b << 14
}
const fn fv_sh(b: u32) -> u32 {
    b << 13
}
const fn pl_sh(b: u32) -> u32 {
    b << 12
}
const fn e16_sh(b: u32) -> u32 {
    b << 11
}
const fn ds_sh(b: u32) -> u32 {
    b << 10
}
const fn ex_sh(b: u32) -> u32 {
    b << 7
}
const fn pm_sh(b: u32) -> u32 {
    b << 23
}
const fn opt_sh(b: u32) -> u32 {
    b << 21
}

const PT_LABV2: u32 = 30;

const ANYSPACE: u32 = cs_sh(31);
const ANYCHANNELS: u32 = ch_sh(15);
const ANYEXTRA: u32 = ex_sh(7);
const ANYPLANAR: u32 = pl_sh(1);
const ANYENDIAN: u32 = e16_sh(1);
const ANYSWAP: u32 = ds_sh(1);
const ANYSWAPFIRST: u32 = sf_sh(1);
const ANYFLAVOR: u32 = fv_sh(1);
const ANYPREMUL: u32 = pm_sh(1);

// Concrete format signatures used by the tables
const TYPE_LAB_DBL: u32 = fl_sh(1) | cs_sh(10) | ch_sh(3) | by_sh(0);
const TYPE_XYZ_DBL: u32 = fl_sh(1) | cs_sh(9) | ch_sh(3) | by_sh(0);
const TYPE_LAB_FLT: u32 = fl_sh(1) | cs_sh(10) | ch_sh(3) | by_sh(4);
const TYPE_XYZ_FLT: u32 = fl_sh(1) | cs_sh(9) | ch_sh(3) | by_sh(4);
const TYPE_GRAY_DBL: u32 = fl_sh(1) | cs_sh(3) | ch_sh(1) | by_sh(0);
const TYPE_LABV2_8: u32 = cs_sh(PT_LABV2) | ch_sh(3) | by_sh(1);
const TYPE_ALABV2_8: u32 = cs_sh(PT_LABV2) | ch_sh(3) | by_sh(1) | ex_sh(1) | sf_sh(1);
const TYPE_LABV2_16: u32 = cs_sh(PT_LABV2) | ch_sh(3) | by_sh(2);

/// `cmsFormatter` — direction-resolved formatter choice
#[derive(Clone, Copy)]
pub enum Formatter {
    None,
    Unpack16(Unpack16Fn),
    Pack16(Pack16Fn),
    UnpackF32(UnpackF32Fn),
    PackF32(PackF32Fn),
}

type F16 = Unpack16Fn;
type P16 = Pack16Fn;
type UF32 = UnpackF32Fn;
type PF32 = PackF32Fn;

/// (Type, Mask, unpack16) — `InputFormatters16`
const INPUT_FORMATTERS16: &[(u32, u32, F16)] = &[
    (
        TYPE_LAB_DBL,
        ANYPLANAR | ANYEXTRA,
        unroll_lab_double_to16 as F16,
    ),
    (
        TYPE_XYZ_DBL,
        ANYPLANAR | ANYEXTRA,
        unroll_xyz_double_to16 as F16,
    ),
    (
        TYPE_LAB_FLT,
        ANYPLANAR | ANYEXTRA,
        unroll_lab_float_to16 as F16,
    ),
    (
        TYPE_XYZ_FLT,
        ANYPLANAR | ANYEXTRA,
        unroll_xyz_float_to16 as F16,
    ),
    (TYPE_GRAY_DBL, 0, unroll_double_1chan as F16),
    (
        fl_sh(1) | by_sh(0),
        ANYCHANNELS | ANYPLANAR | ANYSWAPFIRST | ANYFLAVOR | ANYSWAP | ANYEXTRA | ANYSPACE,
        unroll_double_to16 as F16,
    ),
    (
        fl_sh(1) | by_sh(4),
        ANYCHANNELS | ANYPLANAR | ANYSWAPFIRST | ANYFLAVOR | ANYSWAP | ANYEXTRA | ANYSPACE,
        unroll_float_to16 as F16,
    ),
    (
        fl_sh(1) | by_sh(2),
        ANYCHANNELS | ANYPLANAR | ANYSWAPFIRST | ANYFLAVOR | ANYEXTRA | ANYSWAP | ANYSPACE,
        unroll_half_to16 as F16,
    ),
    (ch_sh(1) | by_sh(1), ANYSPACE, unroll1_byte as F16),
    (
        ch_sh(1) | by_sh(1) | ex_sh(1),
        ANYSPACE,
        unroll1_byte_skip1 as F16,
    ),
    (
        ch_sh(1) | by_sh(1) | ex_sh(2),
        ANYSPACE,
        unroll1_byte_skip2 as F16,
    ),
    (
        ch_sh(1) | by_sh(1) | fv_sh(1),
        ANYSPACE,
        unroll1_byte_reversed as F16,
    ),
    (cs_sh(16) | ch_sh(2) | by_sh(1), 0, unroll2_bytes as F16),
    (TYPE_LABV2_8, 0, unroll_lab_v2_8 as F16),
    (TYPE_ALABV2_8, 0, unroll_alab_v2_8 as F16),
    (TYPE_LABV2_16, 0, unroll_lab_v2_16 as F16),
    (ch_sh(3) | by_sh(1), ANYSPACE, unroll3_bytes as F16),
    (
        ch_sh(3) | by_sh(1) | ds_sh(1),
        ANYSPACE,
        unroll3_bytes_swap as F16,
    ),
    (
        ch_sh(3) | ex_sh(1) | by_sh(1) | ds_sh(1),
        ANYSPACE,
        unroll3_bytes_skip1_swap as F16,
    ),
    (
        ch_sh(3) | ex_sh(1) | by_sh(1) | sf_sh(1),
        ANYSPACE,
        unroll3_bytes_skip1_swap_first as F16,
    ),
    (
        ch_sh(3) | ex_sh(1) | by_sh(1) | ds_sh(1) | sf_sh(1),
        ANYSPACE,
        unroll3_bytes_skip1_swap_swap_first as F16,
    ),
    (ch_sh(4) | by_sh(1), ANYSPACE, unroll4_bytes as F16),
    (
        ch_sh(4) | by_sh(1) | fv_sh(1),
        ANYSPACE,
        unroll4_bytes_reverse as F16,
    ),
    (
        ch_sh(4) | by_sh(1) | sf_sh(1),
        ANYSPACE,
        unroll4_bytes_swap_first as F16,
    ),
    (
        ch_sh(4) | by_sh(1) | ds_sh(1),
        ANYSPACE,
        unroll4_bytes_swap as F16,
    ),
    (
        ch_sh(4) | by_sh(1) | ds_sh(1) | sf_sh(1),
        ANYSPACE,
        unroll4_bytes_swap_swap_first as F16,
    ),
    (
        by_sh(1) | pl_sh(1),
        ANYFLAVOR | ANYSWAPFIRST | ANYPREMUL | ANYSWAP | ANYEXTRA | ANYCHANNELS | ANYSPACE,
        unroll_planar_bytes as F16,
    ),
    (
        by_sh(1),
        ANYFLAVOR | ANYSWAPFIRST | ANYSWAP | ANYPREMUL | ANYEXTRA | ANYCHANNELS | ANYSPACE,
        unroll_chunky_bytes as F16,
    ),
    (ch_sh(1) | by_sh(2), ANYSPACE, unroll1_word as F16),
    (
        ch_sh(1) | by_sh(2) | fv_sh(1),
        ANYSPACE,
        unroll1_word_reversed as F16,
    ),
    (
        ch_sh(1) | by_sh(2) | ex_sh(3),
        ANYSPACE,
        unroll1_word_skip3 as F16,
    ),
    (ch_sh(2) | by_sh(2), ANYSPACE, unroll2_words as F16),
    (ch_sh(3) | by_sh(2), ANYSPACE, unroll3_words as F16),
    (ch_sh(4) | by_sh(2), ANYSPACE, unroll4_words as F16),
    (
        ch_sh(3) | by_sh(2) | ds_sh(1),
        ANYSPACE,
        unroll3_words_swap as F16,
    ),
    (
        ch_sh(3) | by_sh(2) | ex_sh(1) | sf_sh(1),
        ANYSPACE,
        unroll3_words_skip1_swap_first as F16,
    ),
    (
        ch_sh(3) | by_sh(2) | ex_sh(1) | ds_sh(1),
        ANYSPACE,
        unroll3_words_skip1_swap as F16,
    ),
    (
        ch_sh(4) | by_sh(2) | fv_sh(1),
        ANYSPACE,
        unroll4_words_reverse as F16,
    ),
    (
        ch_sh(4) | by_sh(2) | sf_sh(1),
        ANYSPACE,
        unroll4_words_swap_first as F16,
    ),
    (
        ch_sh(4) | by_sh(2) | ds_sh(1),
        ANYSPACE,
        unroll4_words_swap as F16,
    ),
    (
        ch_sh(4) | by_sh(2) | ds_sh(1) | sf_sh(1),
        ANYSPACE,
        unroll4_words_swap_swap_first as F16,
    ),
    (
        by_sh(2) | pl_sh(1),
        ANYFLAVOR | ANYSWAP | ANYENDIAN | ANYEXTRA | ANYCHANNELS | ANYSPACE,
        unroll_planar_words as F16,
    ),
    (
        by_sh(2),
        ANYFLAVOR | ANYSWAPFIRST | ANYSWAP | ANYENDIAN | ANYEXTRA | ANYCHANNELS | ANYSPACE,
        unroll_any_words as F16,
    ),
    (
        by_sh(2) | pl_sh(1),
        ANYFLAVOR | ANYSWAP | ANYENDIAN | ANYEXTRA | ANYCHANNELS | ANYSPACE | pm_sh(1),
        unroll_planar_words_premul as F16,
    ),
    (
        by_sh(2),
        ANYFLAVOR
            | ANYSWAPFIRST
            | ANYSWAP
            | ANYENDIAN
            | ANYEXTRA
            | ANYCHANNELS
            | ANYSPACE
            | pm_sh(1),
        unroll_any_words_premul as F16,
    ),
];

/// `InputFormattersFloat`
const INPUT_FORMATTERS_FLOAT: &[(u32, u32, UF32)] = &[
    (
        TYPE_LAB_DBL,
        ANYPLANAR | ANYEXTRA,
        unroll_lab_double_to_float as UF32,
    ),
    (
        TYPE_LAB_FLT,
        ANYPLANAR | ANYEXTRA,
        unroll_lab_float_to_float as UF32,
    ),
    (
        TYPE_XYZ_DBL,
        ANYPLANAR | ANYEXTRA,
        unroll_xyz_double_to_float as UF32,
    ),
    (
        TYPE_XYZ_FLT,
        ANYPLANAR | ANYEXTRA,
        unroll_xyz_float_to_float as UF32,
    ),
    (
        fl_sh(1) | by_sh(4),
        ANYPLANAR | ANYSWAPFIRST | ANYSWAP | ANYEXTRA | ANYPREMUL | ANYCHANNELS | ANYSPACE,
        unroll_floats_to_float as UF32,
    ),
    (
        fl_sh(1) | by_sh(0),
        ANYPLANAR | ANYSWAPFIRST | ANYSWAP | ANYEXTRA | ANYCHANNELS | ANYSPACE | ANYPREMUL,
        unroll_doubles_to_float as UF32,
    ),
    (TYPE_LABV2_8, 0, unroll_lab_v2_8_to_float as UF32),
    (TYPE_ALABV2_8, 0, unroll_alab_v2_8_to_float as UF32),
    (TYPE_LABV2_16, 0, unroll_lab_v2_16_to_float as UF32),
    (
        by_sh(1),
        ANYPLANAR | ANYSWAPFIRST | ANYSWAP | ANYEXTRA | ANYCHANNELS | ANYSPACE,
        unroll8_to_float as UF32,
    ),
    (
        by_sh(2),
        ANYPLANAR | ANYSWAPFIRST | ANYSWAP | ANYEXTRA | ANYCHANNELS | ANYSPACE,
        unroll16_to_float as UF32,
    ),
    (
        fl_sh(1) | by_sh(2),
        ANYPLANAR | ANYSWAPFIRST | ANYSWAP | ANYEXTRA | ANYCHANNELS | ANYSPACE,
        unroll_half_to_float as UF32,
    ),
];

/// `OutputFormatters16`
const OUTPUT_FORMATTERS16: &[(u32, u32, P16)] = &[
    (
        TYPE_LAB_DBL,
        ANYPLANAR | ANYEXTRA,
        pack_lab_double_from16 as P16,
    ),
    (
        TYPE_XYZ_DBL,
        ANYPLANAR | ANYEXTRA,
        pack_xyz_double_from16 as P16,
    ),
    (
        TYPE_LAB_FLT,
        ANYPLANAR | ANYEXTRA,
        pack_lab_float_from16 as P16,
    ),
    (
        TYPE_XYZ_FLT,
        ANYPLANAR | ANYEXTRA,
        pack_xyz_float_from16 as P16,
    ),
    (
        fl_sh(1) | by_sh(0),
        ANYFLAVOR | ANYSWAPFIRST | ANYSWAP | ANYCHANNELS | ANYPLANAR | ANYEXTRA | ANYSPACE,
        pack_double_from16 as P16,
    ),
    (
        fl_sh(1) | by_sh(4),
        ANYFLAVOR | ANYSWAPFIRST | ANYSWAP | ANYCHANNELS | ANYPLANAR | ANYEXTRA | ANYSPACE,
        pack_float_from16 as P16,
    ),
    (
        fl_sh(1) | by_sh(2),
        ANYFLAVOR | ANYSWAPFIRST | ANYSWAP | ANYCHANNELS | ANYPLANAR | ANYEXTRA | ANYSPACE,
        pack_half_from16 as P16,
    ),
    (ch_sh(1) | by_sh(1), ANYSPACE, pack1_byte as P16),
    (
        ch_sh(1) | by_sh(1) | ex_sh(1),
        ANYSPACE,
        pack1_byte_skip1 as P16,
    ),
    (
        ch_sh(1) | by_sh(1) | ex_sh(1) | sf_sh(1),
        ANYSPACE,
        pack1_byte_skip1_swap_first as P16,
    ),
    (
        ch_sh(1) | by_sh(1) | fv_sh(1),
        ANYSPACE,
        pack1_byte_reversed as P16,
    ),
    (TYPE_LABV2_8, 0, pack_lab_v2_8 as P16),
    (TYPE_ALABV2_8, 0, pack_alab_v2_8 as P16),
    (TYPE_LABV2_16, 0, pack_lab_v2_16 as P16),
    (
        ch_sh(3) | by_sh(1) | opt_sh(1),
        ANYSPACE,
        pack3_bytes as P16,
    ),
    (
        ch_sh(3) | by_sh(1) | ex_sh(1) | opt_sh(1),
        ANYSPACE,
        pack3_bytes_and_skip1 as P16,
    ),
    (
        ch_sh(3) | by_sh(1) | ex_sh(1) | sf_sh(1) | opt_sh(1),
        ANYSPACE,
        pack3_bytes_and_skip1_swap_first as P16,
    ),
    (
        ch_sh(3) | by_sh(1) | ex_sh(1) | ds_sh(1) | sf_sh(1) | opt_sh(1),
        ANYSPACE,
        pack3_bytes_and_skip1_swap_swap_first as P16,
    ),
    (
        ch_sh(3) | by_sh(1) | ds_sh(1) | ex_sh(1) | opt_sh(1),
        ANYSPACE,
        pack3_bytes_and_skip1_swap as P16,
    ),
    (
        ch_sh(3) | by_sh(1) | ds_sh(1) | opt_sh(1),
        ANYSPACE,
        pack3_bytes_swap as P16,
    ),
    (ch_sh(3) | by_sh(1), ANYSPACE, pack3_bytes as P16),
    (
        ch_sh(3) | by_sh(1) | ex_sh(1),
        ANYSPACE,
        pack3_bytes_and_skip1 as P16,
    ),
    (
        ch_sh(3) | by_sh(1) | ex_sh(1) | sf_sh(1),
        ANYSPACE,
        pack3_bytes_and_skip1_swap_first as P16,
    ),
    (
        ch_sh(3) | by_sh(1) | ex_sh(1) | ds_sh(1) | sf_sh(1),
        ANYSPACE,
        pack3_bytes_and_skip1_swap_swap_first as P16,
    ),
    (
        ch_sh(3) | by_sh(1) | ds_sh(1) | ex_sh(1),
        ANYSPACE,
        pack3_bytes_and_skip1_swap as P16,
    ),
    (
        ch_sh(3) | by_sh(1) | ds_sh(1),
        ANYSPACE,
        pack3_bytes_swap as P16,
    ),
    (ch_sh(4) | by_sh(1), ANYSPACE, pack4_bytes as P16),
    (
        ch_sh(4) | by_sh(1) | fv_sh(1),
        ANYSPACE,
        pack4_bytes_reverse as P16,
    ),
    (
        ch_sh(4) | by_sh(1) | sf_sh(1),
        ANYSPACE,
        pack4_bytes_swap_first as P16,
    ),
    (
        ch_sh(4) | by_sh(1) | ds_sh(1),
        ANYSPACE,
        pack4_bytes_swap as P16,
    ),
    (
        ch_sh(4) | by_sh(1) | ds_sh(1) | sf_sh(1),
        ANYSPACE,
        pack4_bytes_swap_swap_first as P16,
    ),
    (ch_sh(6) | by_sh(1), ANYSPACE, pack6_bytes as P16),
    (
        ch_sh(6) | by_sh(1) | ds_sh(1),
        ANYSPACE,
        pack6_bytes_swap as P16,
    ),
    (
        by_sh(1),
        ANYFLAVOR | ANYSWAPFIRST | ANYSWAP | ANYEXTRA | ANYCHANNELS | ANYSPACE | ANYPREMUL,
        pack_chunky_bytes as P16,
    ),
    (
        by_sh(1) | pl_sh(1),
        ANYFLAVOR | ANYSWAPFIRST | ANYSWAP | ANYEXTRA | ANYCHANNELS | ANYSPACE | ANYPREMUL,
        pack_planar_bytes as P16,
    ),
    (ch_sh(1) | by_sh(2), ANYSPACE, pack1_word as P16),
    (
        ch_sh(1) | by_sh(2) | ex_sh(1),
        ANYSPACE,
        pack1_word_skip1 as P16,
    ),
    (
        ch_sh(1) | by_sh(2) | ex_sh(1) | sf_sh(1),
        ANYSPACE,
        pack1_word_skip1_swap_first as P16,
    ),
    (
        ch_sh(1) | by_sh(2) | fv_sh(1),
        ANYSPACE,
        pack1_word_reversed as P16,
    ),
    (
        ch_sh(1) | by_sh(2) | e16_sh(1),
        ANYSPACE,
        pack1_word_big_endian as P16,
    ),
    (ch_sh(3) | by_sh(2), ANYSPACE, pack3_words as P16),
    (
        ch_sh(3) | by_sh(2) | ds_sh(1),
        ANYSPACE,
        pack3_words_swap as P16,
    ),
    (
        ch_sh(3) | by_sh(2) | e16_sh(1),
        ANYSPACE,
        pack3_words_big_endian as P16,
    ),
    (
        ch_sh(3) | by_sh(2) | ex_sh(1),
        ANYSPACE,
        pack3_words_and_skip1 as P16,
    ),
    (
        ch_sh(3) | by_sh(2) | ex_sh(1) | ds_sh(1),
        ANYSPACE,
        pack3_words_and_skip1_swap as P16,
    ),
    (
        ch_sh(3) | by_sh(2) | ex_sh(1) | sf_sh(1),
        ANYSPACE,
        pack3_words_and_skip1_swap_first as P16,
    ),
    (
        ch_sh(3) | by_sh(2) | ex_sh(1) | ds_sh(1) | sf_sh(1),
        ANYSPACE,
        pack3_words_and_skip1_swap_swap_first as P16,
    ),
    (ch_sh(4) | by_sh(2), ANYSPACE, pack4_words as P16),
    (
        ch_sh(4) | by_sh(2) | fv_sh(1),
        ANYSPACE,
        pack4_words_reverse as P16,
    ),
    (
        ch_sh(4) | by_sh(2) | ds_sh(1),
        ANYSPACE,
        pack4_words_swap as P16,
    ),
    (
        ch_sh(4) | by_sh(2) | e16_sh(1),
        ANYSPACE,
        pack4_words_big_endian as P16,
    ),
    (ch_sh(6) | by_sh(2), ANYSPACE, pack6_words as P16),
    (
        ch_sh(6) | by_sh(2) | ds_sh(1),
        ANYSPACE,
        pack6_words_swap as P16,
    ),
    (
        by_sh(2),
        ANYFLAVOR
            | ANYSWAPFIRST
            | ANYSWAP
            | ANYENDIAN
            | ANYEXTRA
            | ANYCHANNELS
            | ANYSPACE
            | ANYPREMUL,
        pack_chunky_words as P16,
    ),
    (
        by_sh(2) | pl_sh(1),
        ANYFLAVOR | ANYENDIAN | ANYSWAP | ANYEXTRA | ANYCHANNELS | ANYSPACE | ANYPREMUL,
        pack_planar_words as P16,
    ),
];

/// `OutputFormattersFloat`
const OUTPUT_FORMATTERS_FLOAT: &[(u32, u32, PF32)] = &[
    (
        TYPE_LAB_FLT,
        ANYPLANAR | ANYEXTRA,
        pack_lab_float_from_float as PF32,
    ),
    (
        TYPE_XYZ_FLT,
        ANYPLANAR | ANYEXTRA,
        pack_xyz_float_from_float as PF32,
    ),
    (
        TYPE_LAB_DBL,
        ANYPLANAR | ANYEXTRA,
        pack_lab_double_from_float as PF32,
    ),
    (
        TYPE_XYZ_DBL,
        ANYPLANAR | ANYEXTRA,
        pack_xyz_double_from_float as PF32,
    ),
    (
        TYPE_LABV2_8,
        ANYPLANAR | ANYEXTRA,
        pack_encoded_bytes_lab_v2_from_float as PF32,
    ),
    (
        TYPE_LABV2_16,
        ANYPLANAR | ANYEXTRA,
        pack_encoded_words_lab_v2_from_float as PF32,
    ),
    (
        fl_sh(1) | by_sh(4),
        ANYPLANAR | ANYFLAVOR | ANYSWAPFIRST | ANYSWAP | ANYEXTRA | ANYCHANNELS | ANYSPACE,
        pack_floats_from_float as PF32,
    ),
    (
        fl_sh(1) | by_sh(0),
        ANYPLANAR | ANYFLAVOR | ANYSWAPFIRST | ANYSWAP | ANYEXTRA | ANYCHANNELS | ANYSPACE,
        pack_doubles_from_float as PF32,
    ),
    (
        by_sh(2),
        ANYPLANAR | ANYFLAVOR | ANYSWAPFIRST | ANYSWAP | ANYEXTRA | ANYCHANNELS | ANYSPACE,
        pack_words_from_float as PF32,
    ),
    (
        by_sh(1),
        ANYPLANAR | ANYFLAVOR | ANYSWAPFIRST | ANYSWAP | ANYEXTRA | ANYCHANNELS | ANYSPACE,
        pack_bytes_from_float as PF32,
    ),
    (
        fl_sh(1) | by_sh(2),
        ANYFLAVOR | ANYSWAPFIRST | ANYSWAP | ANYEXTRA | ANYCHANNELS | ANYSPACE,
        pack_half_from_float as PF32,
    ),
];

pub const CMS_PACK_FLAGS_16BITS: u32 = 0x0000;
pub const CMS_PACK_FLAGS_FLOAT: u32 = 0x0001;

/// `_cmsGetStockInputFormatter`
pub fn get_stock_input_formatter(dw_input: u32, dw_flags: u32) -> Formatter {
    match dw_flags {
        CMS_PACK_FLAGS_16BITS => {
            for &(ty, mask, f) in INPUT_FORMATTERS16 {
                if dw_input & !mask == ty {
                    return Formatter::Unpack16(f);
                }
            }
        }
        CMS_PACK_FLAGS_FLOAT => {
            for &(ty, mask, f) in INPUT_FORMATTERS_FLOAT {
                if dw_input & !mask == ty {
                    return Formatter::UnpackF32(f);
                }
            }
        }
        _ => {}
    }
    Formatter::None
}

/// `_cmsGetStockOutputFormatter`
pub fn get_stock_output_formatter(dw_input: u32, dw_flags: u32) -> Formatter {
    let dw_input = dw_input & !opt_sh(1);
    match dw_flags {
        CMS_PACK_FLAGS_16BITS => {
            for &(ty, mask, f) in OUTPUT_FORMATTERS16 {
                if dw_input & !mask == ty {
                    return Formatter::Pack16(f);
                }
            }
        }
        CMS_PACK_FLAGS_FLOAT => {
            for &(ty, mask, f) in OUTPUT_FORMATTERS_FLOAT {
                if dw_input & !mask == ty {
                    return Formatter::PackF32(f);
                }
            }
        }
        _ => {}
    }
    Formatter::None
}

/// `cmsFormatterDirection`
#[derive(Clone, Copy, PartialEq)]
pub enum FormatterDir {
    Input,
    Output,
}

/// `_cmsGetFormatter` — plugins first (unimplemented), then stock tables
pub fn get_formatter(ty: u32, dir: FormatterDir, dw_flags: u32) -> Formatter {
    if t_channels(ty) == 0 {
        return Formatter::None;
    }
    match dir {
        FormatterDir::Input => get_stock_input_formatter(ty, dw_flags),
        FormatterDir::Output => get_stock_output_formatter(ty, dw_flags),
    }
}

/// `_cmsGetFormatter` — C consults ctx-registered `cmsFormattersFactory`
/// plugins before the stock tables. `ctx` supplies the factories; on a
/// non-NULL `cmsFormatter` result the C fn is registered and a `Formatter`
/// wrapping the trampoline is returned.
pub fn get_formatter_ctx(
    ctx: Option<&crate::context::Context>,
    ty: u32,
    dir: FormatterDir,
    dw_flags: u32,
) -> Formatter {
    if t_channels(ty) == 0 {
        return Formatter::None;
    }
    let d = match dir {
        FormatterDir::Input => 0u32,
        FormatterDir::Output => 1u32,
    };
    // plugin factories first — first non-NULL `cmsFormatter` wins
    if let Some(cfn) = foreign_formatter_fn(ctx, ty, d, dw_flags) {
        register_foreign_format(d, ty, cfn);
        let float = dw_flags & 1 != 0;
        return match (d, float) {
            (0, false) => Formatter::Unpack16(foreign_unpack16),
            (0, true) => Formatter::UnpackF32(foreign_unpack_f32),
            (1, false) => Formatter::Pack16(foreign_pack16),
            _ => Formatter::PackF32(foreign_pack_f32),
        };
    }
    match dir {
        FormatterDir::Input => get_stock_input_formatter(ty, dw_flags),
        FormatterDir::Output => get_stock_output_formatter(ty, dw_flags),
    }
}
