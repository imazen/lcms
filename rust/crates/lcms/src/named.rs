//! Port of `cmsnamed.c` — MLU strings, named colors, profile sequences, dicts.
//!
//! C source: `reference/lcms2/src/cmsnamed.c`
//! Gate: `tests/c_parity_named.rs`
//!
//! `wchar_t` is a signed 32-bit type on the oracle's platform, modelled as
//! `Wchar = i32` — the `MemPool` stores its raw bytes like C (LE units), so
//! every edge case (negative wchars, surrogates skipped) is preserved.

use crate::context::Context;
use crate::err;
use crate::fixed::quick_saturate_word;
use crate::lut::{Stage, StageData, stage_alloc_placeholder};
use crate::signatures::stage as sig;
use crate::types::ERROR_RANGE;

/// `wchar_t` on the reference platform (LP64).
pub type Wchar = i32;

/// `cmsMAX_PATH`
pub const CMS_MAX_PATH: usize = 256;

/// `_cmsMLUentry`
#[derive(Clone, Copy, Debug, Default)]
pub struct MluEntry {
    pub language: u16,
    pub country: u16,
    pub str_w: u32, // Offset to current unicode string
    pub len: u32,   // Length in bytes
}

/// `cmsMLU`
#[derive(Clone)]
pub struct Mlu {
    pub allocated_entries: u32,
    pub used_entries: u32,
    pub entries: Vec<MluEntry>, // len == allocated_entries
    pub pool: Vec<u8>,          // MemPool; len == pool_size
    pub pool_used: u32,
}

/// `cmsMLUalloc`
pub fn mlu_alloc(_ctx: Option<&Context>, n_items: u32) -> Option<Mlu> {
    // nItems should be positive if given
    let n_items = if n_items == 0 { 2 } else { n_items };
    Some(Mlu {
        allocated_entries: n_items,
        used_entries: 0,
        entries: vec![MluEntry::default(); n_items as usize],
        pool: Vec::new(),
        pool_used: 0,
    })
}

/// `GrowMLUpool` — doubles pool size (starts at 256).
fn grow_mlu_pool(mlu: &mut Mlu) -> bool {
    let size = if mlu.pool.is_empty() {
        256usize
    } else {
        mlu.pool.len() * 2
    };
    if size < mlu.pool.len() {
        return false;
    }
    mlu.pool.resize(size, 0);
    true
}

/// `GrowMLUtable` — doubles entry table size.
fn grow_mlu_table(mlu: &mut Mlu) -> bool {
    let allocated = mlu.allocated_entries.wrapping_mul(2);
    if allocated < mlu.allocated_entries {
        return false;
    }
    if allocated.wrapping_mul(16) / 16 != allocated {
        return false;
    }
    mlu.entries.resize(allocated as usize, MluEntry::default());
    mlu.allocated_entries = allocated;
    true
}

/// `SearchMLUEntry`
fn search_mlu_entry(mlu: &Mlu, language: u16, country: u16) -> i32 {
    for i in 0..mlu.used_entries as usize {
        if mlu.entries[i].country == country && mlu.entries[i].language == language {
            return i as i32;
        }
    }
    -1
}

/// `AddMLUBlock`
fn add_mlu_block(mlu: &mut Mlu, size: u32, block: &[u8], language: u16, country: u16) -> bool {
    // Is there any room available?
    if mlu.used_entries >= mlu.allocated_entries && !grow_mlu_table(mlu) {
        return false;
    }

    // Only one entry per language/country pair is allowed
    if search_mlu_entry(mlu, language, country) >= 0 {
        return false;
    }

    // Check for size
    while (mlu.pool.len() as u32 - mlu.pool_used) < size {
        if !grow_mlu_pool(mlu) {
            return false;
        }
    }

    let offset = mlu.pool_used as usize;
    mlu.pool[offset..offset + size as usize].copy_from_slice(block);
    mlu.pool_used += size;

    let e = &mut mlu.entries[mlu.used_entries as usize];
    e.str_w = offset as u32;
    e.len = size;
    e.country = country;
    e.language = language;
    mlu.used_entries += 1;
    true
}

/// `strTo16` — a 3-char code → u16 (first 2 bytes, big-endian-ish).
fn str_to16(s: &[u8]) -> u16 {
    ((s[0] as u16) << 8) | s[1] as u16
}

/// `strFrom16` — u16 → "XY\0"
fn str_from16(n: u16) -> [u8; 3] {
    [(n >> 8) as u8, n as u8, 0]
}

/// `decodeUTF8` — UTF8 → wchar count (out=None) or chars (out=Some).
/// sizeof(wchar_t) > 2 on the reference platform → BMP chars direct.
fn decode_utf8(mut out: Option<&mut Vec<Wchar>>, input: &[u8]) -> u32 {
    let mut codepoint: u32 = 0;
    let mut size = 0u32;
    let mut i = 0usize;

    while i < input.len() && input[i] != 0 {
        let ch = input[i];

        if ch <= 0x7f {
            codepoint = ch as u32;
        } else if ch <= 0xbf {
            codepoint = (codepoint << 6) | (ch as u32 & 0x3f);
        } else if ch <= 0xdf {
            codepoint = ch as u32 & 0x1f;
        } else if ch <= 0xef {
            codepoint = ch as u32 & 0x0f;
        } else {
            codepoint = ch as u32 & 0x07;
        }

        i += 1;

        if (i >= input.len() || (input[i] & 0xc0) != 0x80) && codepoint <= 0x10ffff {
            // sizeof(wchar_t) > 2 on the reference platform
            if let Some(o) = out.as_deref_mut() {
                o.push(codepoint as Wchar);
            }
            size += 1;
        }
    }
    size
}

/// `encodeUTF8` — wchar → UTF8. Returns length; writes up to `max_chars`
/// into `out` when present (C writes into caller buffer, clipping by size).
fn encode_utf8(out: Option<&mut Vec<u8>>, input: &[Wchar], max_wchars: u32, max_chars: u32) -> u32 {
    let mut codepoint: u32 = 0;
    let mut size = 0u32;
    let mut len_w = 0u32;
    let mut i = 0usize;

    let has_out = out.is_some();
    let mut buf: Vec<u8> = Vec::new();
    let push = |buf: &mut Vec<u8>, b: u8| buf.push(b);

    while len_w < max_wchars && i < input.len() && input[i] != 0 {
        let wc = input[i];
        if (0xd800..=0xdbff).contains(&wc) {
            codepoint = ((wc - 0xd800) as u32) << 10 | 0x10000;
        } else {
            if (0xdc00..=0xdfff).contains(&wc) {
                codepoint |= (wc - 0xdc00) as u32;
            } else {
                codepoint = wc as u32; // wraps like C u32 assign of negative wchar
            }

            if codepoint <= 0x7f {
                if has_out && size + 1 < max_chars {
                    push(&mut buf, codepoint as u8);
                }
                size += 1;
            } else if codepoint <= 0x7ff {
                if has_out && max_chars > 0 && size + 2 < max_chars {
                    push(&mut buf, (0xc0 | ((codepoint >> 6) & 0x1f)) as u8);
                    push(&mut buf, (0x80 | (codepoint & 0x3f)) as u8);
                }
                size += 2;
            } else if codepoint <= 0xffff {
                if has_out && max_chars > 0 && size + 3 < max_chars {
                    push(&mut buf, (0xe0 | ((codepoint >> 12) & 0x0f)) as u8);
                    push(&mut buf, (0x80 | ((codepoint >> 6) & 0x3f)) as u8);
                    push(&mut buf, (0x80 | (codepoint & 0x3f)) as u8);
                }
                size += 3;
            } else {
                if has_out && max_chars > 0 && size + 4 < max_chars {
                    push(&mut buf, (0xf0 | ((codepoint >> 18) & 0x07)) as u8);
                    push(&mut buf, (0x80 | ((codepoint >> 12) & 0x3f)) as u8);
                    push(&mut buf, (0x80 | ((codepoint >> 6) & 0x3f)) as u8);
                    push(&mut buf, (0x80 | (codepoint & 0x3f)) as u8);
                }
                size += 4;
            }
            codepoint = 0;
        }
        i += 1;
        len_w += 1;
    }

    if let Some(o) = out {
        *o = buf;
    }
    size
}

/// `cmsMLUsetASCII` — `ascii` bytes are the string (no NUL added).
pub fn mlu_set_ascii(mlu: &mut Mlu, lang: &[u8], cntry: &[u8], ascii: &[u8]) -> bool {
    let lang16 = str_to16(lang);
    let cntry16 = str_to16(cntry);

    if ascii.is_empty() {
        // force a \0
        return add_mlu_block(mlu, 4, &0i32.to_ne_bytes(), lang16, cntry16);
    }

    let mut wstr = Vec::with_capacity(ascii.len());
    for &b in ascii {
        wstr.extend_from_slice(&(b as i8 as Wchar).to_ne_bytes()); // (wchar_t)char: sign-extends
    }
    add_mlu_block(mlu, (ascii.len() * 4) as u32, &wstr, lang16, cntry16)
}

/// `cmsMLUsetUTF8`
pub fn mlu_set_utf8(mlu: &mut Mlu, lang: &[u8], cntry: &[u8], utf8: &[u8]) -> bool {
    let lang16 = str_to16(lang);
    let cntry16 = str_to16(cntry);

    if utf8.is_empty() || utf8[0] == 0 {
        return add_mlu_block(mlu, 4, &0i32.to_ne_bytes(), lang16, cntry16);
    }

    let utf8len = decode_utf8(None, utf8);
    let mut wchars = Vec::with_capacity(utf8len as usize);
    decode_utf8(Some(&mut wchars), utf8);

    let mut block = Vec::with_capacity(utf8len as usize * 4);
    for &w in &wchars {
        block.extend_from_slice(&w.to_ne_bytes());
    }
    add_mlu_block(mlu, utf8len * 4, &block, lang16, cntry16)
}

/// `mywcslen` — NUL-terminated length of a wchar slice (returns slice len if
/// unterminated — same bound as C reading to a NUL).
fn mywcslen(s: &[Wchar]) -> usize {
    s.iter().position(|&c| c == 0).unwrap_or(s.len())
}

/// `cmsMLUsetWide` — `wide` is a NUL-terminated wchar slice.
pub fn mlu_set_wide(mlu: &mut Mlu, lang: &[u8], cntry: &[u8], wide: &[Wchar]) -> bool {
    let lang16 = str_to16(lang);
    let cntry16 = str_to16(cntry);

    let mut len = (mywcslen(wide) * 4) as u32;
    if len == 0 {
        len = 4;
    }
    let mut block = Vec::with_capacity(len as usize);
    for &w in &wide[..len as usize / 4] {
        block.extend_from_slice(&w.to_ne_bytes());
    }
    add_mlu_block(mlu, len, &block, lang16, cntry16)
}

/// `cmsMLUdup`
pub fn mlu_dup(mlu: &Mlu) -> Option<Mlu> {
    Some(Mlu {
        allocated_entries: mlu.used_entries,
        used_entries: mlu.used_entries,
        entries: mlu.entries[..mlu.used_entries as usize].to_vec(),
        pool: if mlu.pool_used == 0 {
            Vec::new()
        } else {
            mlu.pool[..mlu.pool_used as usize].to_vec()
        },
        pool_used: mlu.pool_used,
    })
}

/// `_cmsMLUgetWide` — find best match; returns (wchar slice, len, used lang, used country).
fn mlu_get_wide_internal(mlu: &Mlu, language: u16, country: u16) -> Option<(&[u8], u32, u16, u16)> {
    if mlu.allocated_entries == 0 {
        return None;
    }

    let mut best: i32 = -1;
    for i in 0..mlu.used_entries as usize {
        let v = &mlu.entries[i];
        if v.language == language {
            if best == -1 {
                best = i as i32;
            }
            if v.country == country {
                let s = v.str_w as usize;
                return Some((
                    &mlu.pool[s..s + v.len as usize],
                    v.len,
                    v.language,
                    v.country,
                ));
            }
        }
    }

    let best = if best == -1 { 0usize } else { best as usize };
    let v = &mlu.entries[best];
    if v.str_w + v.len > mlu.pool.len() as u32 {
        return None;
    }
    let s = v.str_w as usize;
    Some((
        &mlu.pool[s..s + v.len as usize],
        v.len,
        v.language,
        v.country,
    ))
}

/// Read a wchar (i32, native-endian like C) from the byte pool.
#[inline]
fn wchar_at(pool: &[u8], idx: usize) -> Wchar {
    i32::from_ne_bytes(pool[idx * 4..idx * 4 + 4].try_into().unwrap())
}

/// `cmsMLUgetASCII` — returns (bytes incl. NUL like C's return count semantics,
/// needed length). `None` means get-length mode (returns needed size).
pub fn mlu_get_ascii(
    mlu: &Mlu,
    lang: &[u8],
    cntry: &[u8],
    buffer_size: Option<u32>,
) -> (u32, Vec<u8>) {
    let lang16 = str_to16(lang);
    let cntry16 = str_to16(cntry);

    let Some((pool_slice, str_len, _, _)) = mlu_get_wide_internal(mlu, lang16, cntry16) else {
        return (0, Vec::new());
    };

    let mut ascii_len = str_len / 4;
    match buffer_size {
        None => return (ascii_len + 1, Vec::new()), // note the zero at the end
        Some(0) => return (0, Vec::new()),
        Some(bs) => {
            if bs < ascii_len + 1 {
                ascii_len = bs - 1;
            }
        }
    }

    let mut buf = Vec::with_capacity(ascii_len as usize + 1);
    for i in 0..ascii_len as usize {
        let wc = wchar_at(pool_slice, i);
        buf.push(if wc < 0xff { wc as u8 } else { b'?' });
    }
    buf.push(0);
    (ascii_len + 1, buf)
}

/// `cmsMLUgetUTF8`
pub fn mlu_get_utf8(
    mlu: &Mlu,
    lang: &[u8],
    cntry: &[u8],
    buffer_size: Option<u32>,
) -> (u32, Vec<u8>) {
    let lang16 = str_to16(lang);
    let cntry16 = str_to16(cntry);

    let Some((pool_slice, str_len, _, _)) = mlu_get_wide_internal(mlu, lang16, cntry16) else {
        return (0, Vec::new());
    };

    // Reconstruct the wchar array
    let wide: Vec<Wchar> = (0..str_len / 4)
        .map(|i| wchar_at(pool_slice, i as usize))
        .collect();
    let max_wchars = str_len / 4;

    let utf8len = encode_utf8(None, &wide, max_wchars, buffer_size.unwrap_or(0));

    let Some(bs) = buffer_size else {
        return (utf8len + 1, Vec::new());
    };
    if bs == 0 {
        return (0, Vec::new());
    }

    let mut utf8len = utf8len;
    if bs < utf8len + 1 {
        utf8len = bs - 1;
    }

    let mut buf = Vec::new();
    encode_utf8(Some(&mut buf), &wide, max_wchars, bs);
    buf.truncate(utf8len as usize);
    buf.push(0);
    (utf8len + 1, buf)
}

/// `cmsMLUgetWide`
pub fn mlu_get_wide(
    mlu: &Mlu,
    lang: &[u8],
    cntry: &[u8],
    buffer_size_wchars: Option<u32>,
) -> (u32, Vec<Wchar>) {
    let lang16 = str_to16(lang);
    let cntry16 = str_to16(cntry);

    let Some((pool_slice, str_len, _, _)) = mlu_get_wide_internal(mlu, lang16, cntry16) else {
        return (0, Vec::new());
    };

    match buffer_size_wchars {
        None => return (str_len + 4, Vec::new()),
        Some(bs) if bs < 4 => return (0, Vec::new()),
        Some(bs) => {
            let mut sl = str_len;
            if bs < sl + 4 {
                sl = bs - 4;
            }
            let mut out = Vec::with_capacity(sl as usize / 4 + 1);
            for i in 0..sl / 4 {
                out.push(wchar_at(pool_slice, i as usize));
            }
            out.push(0);
            (sl + 4, out)
        }
    }
}

/// `cmsMLUgetTranslation`
pub fn mlu_get_translation(mlu: &Mlu, lang: &[u8], cntry: &[u8]) -> Option<([u8; 3], [u8; 3])> {
    let lang16 = str_to16(lang);
    let cntry16 = str_to16(cntry);
    let (_, _, ol, oc) = mlu_get_wide_internal(mlu, lang16, cntry16)?;
    Some((str_from16(ol), str_from16(oc)))
}

/// `cmsMLUtranslationsCount`
pub fn mlu_translations_count(mlu: &Mlu) -> u32 {
    mlu.used_entries
}

/// `cmsMLUtranslationsCodes`
pub fn mlu_translations_codes(mlu: &Mlu, idx: u32) -> Option<([u8; 3], [u8; 3])> {
    if idx >= mlu.used_entries {
        return None;
    }
    let e = &mlu.entries[idx as usize];
    Some((str_from16(e.language), str_from16(e.country)))
}

// ---------------------------------------------------------------------------
// Named color lists
// ---------------------------------------------------------------------------

/// `_cmsNAMEDCOLOR`
#[derive(Clone)]
pub struct NamedColor {
    pub name: [u8; CMS_MAX_PATH],
    pub pcs: [u16; 3],
    pub device_colorant: [u16; 16],
}

/// `cmsNAMEDCOLORLIST`
#[derive(Clone)]
pub struct NamedColorList {
    pub list: Vec<NamedColor>, // len tracks Allocated in C; nColors is a separate count
    pub n_colors: u32,
    pub allocated: u32,
    pub colorant_count: u32,
    pub prefix: [u8; 33],
    pub suffix: [u8; 33],
}

/// `GrowNamedColorList`
fn grow_named_color_list(v: &mut NamedColorList) -> bool {
    let size = if v.allocated == 0 {
        64
    } else {
        v.allocated * 2
    };
    if size > 1024 * 100 {
        v.list.clear();
        return false;
    }
    v.list.resize(
        size as usize,
        NamedColor {
            name: [0; CMS_MAX_PATH],
            pcs: [0; 3],
            device_colorant: [0; 16],
        },
    );
    v.allocated = size;
    true
}

fn c_str_to_fixed33(src: &[u8], dest: &mut [u8; 33]) {
    *dest = [0; 33];
    let n = src.len().min(32);
    dest[..n].copy_from_slice(&src[..n]);
}

/// `cmsAllocNamedColorList`
pub fn alloc_named_color_list(
    _ctx: Option<&Context>,
    n: u32,
    colorant_count: u32,
    prefix: &[u8],
    suffix: &[u8],
) -> Option<NamedColorList> {
    if colorant_count > 16 {
        return None;
    }
    let mut v = NamedColorList {
        list: Vec::new(),
        n_colors: 0,
        allocated: 0,
        colorant_count,
        prefix: [0; 33],
        suffix: [0; 33],
    };
    while v.allocated < n {
        if !grow_named_color_list(&mut v) {
            return None;
        }
    }
    c_str_to_fixed33(prefix, &mut v.prefix);
    c_str_to_fixed33(suffix, &mut v.suffix);
    Some(v)
}

/// `cmsDupNamedColorList`
pub fn dup_named_color_list(v: &NamedColorList) -> Option<NamedColorList> {
    let mut new_nc =
        alloc_named_color_list(None, v.n_colors, v.colorant_count, &v.prefix, &v.suffix)?;
    while new_nc.allocated < v.allocated {
        if !grow_named_color_list(&mut new_nc) {
            return None;
        }
    }
    new_nc.prefix = v.prefix;
    new_nc.suffix = v.suffix;
    new_nc.colorant_count = v.colorant_count;
    new_nc.list[..v.n_colors as usize].clone_from_slice(&v.list[..v.n_colors as usize]);
    new_nc.n_colors = v.n_colors;
    Some(new_nc)
}

/// `cmsAppendNamedColor`
pub fn append_named_color(
    list: &mut NamedColorList,
    name: Option<&[u8]>,
    pcs: Option<&[u16; 3]>,
    colorant: Option<&[u16; 16]>,
) -> bool {
    if list.n_colors + 1 > list.allocated && !grow_named_color_list(list) {
        return false;
    }
    let e = &mut list.list[list.n_colors as usize];
    for i in 0..list.colorant_count as usize {
        e.device_colorant[i] = colorant.map_or(0, |c| c[i]);
    }
    for i in 0..3 {
        e.pcs[i] = pcs.map_or(0, |p| p[i]);
    }
    match name {
        Some(n) => {
            e.name = [0; CMS_MAX_PATH];
            let len = n.len().min(CMS_MAX_PATH - 1);
            e.name[..len].copy_from_slice(&n[..len]);
        }
        None => e.name[0] = 0,
    }
    list.n_colors += 1;
    true
}

/// `cmsNamedColorCount`
pub fn named_color_count(list: &NamedColorList) -> u32 {
    list.n_colors
}

/// `cmsNamedColorInfo`
pub fn named_color_info(
    list: &NamedColorList,
    n_color: u32,
) -> Option<([u8; CMS_MAX_PATH], [u8; 33], [u8; 33], [u16; 3], [u16; 16])> {
    if n_color >= list.n_colors {
        return None;
    }
    let e = &list.list[n_color as usize];
    Some((e.name, list.prefix, list.suffix, e.pcs, e.device_colorant))
}

/// `cmsstrcasecmp` (from cmserr.c — included here since NamedColorIndex is its
/// only consumer so far).
pub fn cms_strcasecmp(s1: &[u8], s2: &[u8]) -> i32 {
    fn toupper(b: u8) -> u8 {
        if b.is_ascii_lowercase() { b - 32 } else { b }
    }
    let mut i = 0usize;
    loop {
        let a = if i < s1.len() { s1[i] } else { 0 };
        let b = if i < s2.len() { s2[i] } else { 0 };
        if toupper(a) != toupper(b) {
            return toupper(a) as i32 - toupper(b) as i32;
        }
        if a == 0 {
            return 0;
        }
        i += 1;
    }
}

/// `cmsNamedColorIndex`
pub fn named_color_index(list: &NamedColorList, name: &[u8]) -> i32 {
    for i in 0..list.n_colors as usize {
        let n = &list.list[i].name;
        let n_len = n.iter().position(|&b| b == 0).unwrap_or(n.len());
        if cms_strcasecmp(name, &n[..n_len]) == 0 {
            return i as i32;
        }
    }
    -1
}

// ---------------------------------------------------------------------------
// MPE support — `_cmsStageAllocNamedColor`
// ---------------------------------------------------------------------------

/// `EvalNamedColorPCS`
fn eval_named_color_pcs(input: &[f32], output: &mut [f32], mpe: &Stage) {
    if let StageData::NamedColor(list) = &mpe.data {
        let index = quick_saturate_word(input[0] as f64 * 65535.0);
        if index as u32 >= list.n_colors {
            err::signal_error(None, ERROR_RANGE, "Color out of range");
            output[0] = 0.0;
            output[1] = 0.0;
            output[2] = 0.0;
        } else {
            output[0] = (list.list[index as usize].pcs[0] as f64 / 65535.0) as f32;
            output[1] = (list.list[index as usize].pcs[1] as f64 / 65535.0) as f32;
            output[2] = (list.list[index as usize].pcs[2] as f64 / 65535.0) as f32;
        }
    }
}

/// `EvalNamedColor`
fn eval_named_color(input: &[f32], output: &mut [f32], mpe: &Stage) {
    if let StageData::NamedColor(list) = &mpe.data {
        let index = quick_saturate_word(input[0] as f64 * 65535.0);
        if index as u32 >= list.n_colors {
            err::signal_error(None, ERROR_RANGE, "Color out of range");
            for j in 0..list.colorant_count as usize {
                output[j] = 0.0;
            }
        } else {
            for j in 0..list.colorant_count as usize {
                output[j] = (list.list[index as usize].device_colorant[j] as f64 / 65535.0) as f32;
            }
        }
    }
}

/// `_cmsStageAllocNamedColor` — the stage owns a DUP of the list.
pub fn stage_alloc_named_color(list: &NamedColorList, use_pcs: bool) -> Option<Stage> {
    let dup = dup_named_color_list(list)?;
    Some(stage_alloc_placeholder(
        None,
        sig::SigNamedColorElemType,
        1,
        if use_pcs { 3 } else { list.colorant_count },
        if use_pcs {
            eval_named_color_pcs
        } else {
            eval_named_color
        },
        StageData::NamedColor(dup),
    ))
}

// ---------------------------------------------------------------------------
// Profile sequence descriptions
// ---------------------------------------------------------------------------

/// `cmsPSEQDESC`
#[derive(Clone)]
pub struct PSeqDesc {
    pub device_mfg: u32,
    pub device_model: u32,
    pub attributes: u64,
    pub technology: u32,
    pub profile_id: crate::types::ProfileId,
    pub manufacturer: Option<Mlu>,
    pub model: Option<Mlu>,
    pub description: Option<Mlu>,
}

/// `cmsSEQ`
#[derive(Clone)]
pub struct ProfileSeqDesc {
    pub seq: Vec<PSeqDesc>,
}

/// `cmsAllocProfileSequenceDescription` — max 255 profiles linked.
pub fn alloc_profile_seq_desc(_ctx: Option<&Context>, n: u32) -> Option<ProfileSeqDesc> {
    if n == 0 || n > 255 {
        return None;
    }
    Some(ProfileSeqDesc {
        seq: vec![
            PSeqDesc {
                device_mfg: 0,
                device_model: 0,
                attributes: 0,
                technology: 0,
                profile_id: crate::types::ProfileId::default(),
                manufacturer: None,
                model: None,
                description: None,
            };
            n as usize
        ],
    })
}

// ---------------------------------------------------------------------------
// Dictionaries
// ---------------------------------------------------------------------------

/// `cmsDICTentry`
#[derive(Clone)]
pub struct DictEntry {
    pub display_name: Option<Mlu>,
    pub display_value: Option<Mlu>,
    pub name: Vec<Wchar>,
    pub value: Vec<Wchar>,
}

/// `cmsDICT` — entries stored newest-first (C prepends to a linked list).
#[derive(Clone, Default)]
pub struct Dict {
    pub entries: Vec<DictEntry>,
}

/// `DupWcs` — duplicate a NUL-terminated wide string (keeps the NUL).
fn dup_wcs(src: &[Wchar]) -> Vec<Wchar> {
    let n = mywcslen(src);
    src[..n + 1].to_vec()
}

/// `cmsDictAlloc`
pub fn dict_alloc(_ctx: Option<&Context>) -> Dict {
    Dict::default()
}

/// `cmsDictAddEntry` — prepends (C linked-list head insert).
pub fn dict_add_entry(
    dict: &mut Dict,
    name: &[Wchar],
    value: &[Wchar],
    display_name: Option<&Mlu>,
    display_value: Option<&Mlu>,
) -> bool {
    dict.entries.insert(
        0,
        DictEntry {
            display_name: display_name.and_then(mlu_dup),
            display_value: display_value.and_then(mlu_dup),
            name: dup_wcs(name),
            value: dup_wcs(value),
        },
    );
    true
}

/// `cmsDictDup` — iterates the old list head→tail adding to new, which
/// reverses order (C quirk preserved exactly).
pub fn dict_dup(dict: &Dict) -> Dict {
    let mut new = Dict::default();
    for e in &dict.entries {
        dict_add_entry(
            &mut new,
            &e.name,
            &e.value,
            e.display_name.as_ref(),
            e.display_value.as_ref(),
        );
    }
    new
}
