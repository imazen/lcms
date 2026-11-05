//! Fixed-point helpers — `lcms2_internal.h` inlines + `cmsplugin.c` converters.
//!
//! `_cmsQuickFloor` is the double2fixmagic trick, NOT `floor()`: adding
//! 2^36*1.5 and reading the low mantissa word is only defined behaviour for
//! |val| < 32767, and that is exactly the domain C relies on. Port the trick,
//! not its mathematical meaning.

/// 2^36 * 1.5 — the double2fixmagic constant (lcms2_internal.h).
const DOUBLE2FIXMAGIC: f64 = 68719476736.0 * 1.5;

/// `_cmsQuickFloor` — Sree Kotay / Stuart Nixon fast floor.
///
/// C: `temp.val = val + magic; return temp.halves[lo_word] >> 16;`
/// `halves[]` is `int`, so the shift is arithmetic. On little-endian the low
/// word is bits 0..32 of the f64; on big-endian it is bits 32..64.
#[inline]
pub fn quick_floor(val: f64) -> i32 {
    let bits = (val + DOUBLE2FIXMAGIC).to_bits();
    #[cfg(target_endian = "little")]
    let word = bits as u32 as i32;
    #[cfg(target_endian = "big")]
    let word = (bits >> 32) as u32 as i32;
    word >> 16
}

/// `_cmsQuickFloorWord` — fast floor restricted to 0..65535.0
#[inline]
pub fn quick_floor_word(d: f64) -> u16 {
    (quick_floor(d - 32767.0) as u16).wrapping_add(32767)
}

/// `_cmsQuickSaturateWord` — floor to word with saturation.
#[inline]
pub fn quick_saturate_word(d: f64) -> u16 {
    let d = d + 0.5;
    if d <= 0.0 {
        return 0;
    }
    if d >= 65535.0 {
        return 0xffff;
    }
    quick_floor_word(d)
}

/// `FIXED_TO_INT`
#[inline]
pub fn fixed_to_int(x: i32) -> i32 {
    x >> 16
}

/// `FIXED_REST_TO_INT`
#[inline]
pub fn fixed_rest_to_int(x: i32) -> i32 {
    x & 0xFFFF
}

/// `ROUND_FIXED_TO_INT`
#[inline]
pub fn round_fixed_to_int(x: i32) -> i32 {
    x.wrapping_add(0x8000) >> 16
}

/// `_cmsToFixedDomain` — `a + ((a + 0x7fff) / 0xffff)`; C `/` truncates
/// toward zero, as does Rust's `i32` division.
#[inline]
pub fn to_fixed_domain(a: i32) -> i32 {
    a.wrapping_add((a.wrapping_add(0x7fff)) / 0xffff)
}

/// `_cmsFromFixedDomain` — `a - ((a + 0x7fff) >> 16)`
#[inline]
pub fn from_fixed_domain(a: i32) -> i32 {
    a.wrapping_sub((a.wrapping_add(0x7fff)) >> 16)
}

/// `_cms15Fixed16toDouble`
#[inline]
pub fn s15_fixed16_to_double(fix32: i32) -> f64 {
    fix32 as f64 / 65536.0
}

/// `_cmsDoubleTo15Fixed16` — `floor(v * 65536 + 0.5)` truncated to i32.
/// C's double→int cast is UB outside i32 range; Rust `as` saturates —
/// identical inside the domain C stays in.
#[inline]
pub fn double_to_s15_fixed16(v: f64) -> i32 {
    (v * 65536.0 + 0.5).floor() as i32
}

/// `_cmsQuantizeVal` — quantize 0 <= i < MaxSamples to 0..0xffff.
#[inline]
pub fn quantize_val(i: f64, max_samples: u32) -> u16 {
    let x = i * 65535.0 / (max_samples - 1) as f64;
    quick_saturate_word(x)
}

/// `_cms8Fixed8toDouble`
#[inline]
pub fn u8_fixed8_to_double(fixed8: u16) -> f64 {
    fixed8 as f64 / 256.0
}

/// `_cmsDoubleTo8Fixed8`
#[inline]
pub fn double_to_u8_fixed8(val: f64) -> u16 {
    let gamma_fixed32 = double_to_s15_fixed16(val);
    ((gamma_fixed32 >> 8) & 0xFFFF) as u16
}
