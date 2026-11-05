//! Port of `cmshalf.c` — IEEE-754 half-precision conversion.
//!
//! Inspired by the paper "Fast Half Float Conversions" by Jeroen van der Zijp.
//! The tables in `half_tables` are extracted verbatim from the C source.
//!
//! C source: `reference/lcms2/src/cmshalf.c`
//! Gate: `tests/c_parity_half.rs` — exhaustive over all 2^16 half inputs.

use crate::half_tables::{BASE, EXPONENT, MANTISSA, OFFSET, SHIFT};

/// `_cmsHalf2Float` — half to single via the union-bit trick.
/// `out.num = Mantissa[(h & 0x3ff) + Offset[h >> 10]] + Exponent[h >> 10]`
pub fn half2float(h: u16) -> f32 {
    let n = (h >> 10) as usize;
    let num = MANTISSA[(h & 0x3ff) as usize + OFFSET[n] as usize].wrapping_add(EXPONENT[n]);
    f32::from_bits(num)
}

/// `_cmsFloat2Half` — single to half via the union-bit trick.
/// `Base[j] + ((n & 0x007fffff) >> Shift[j])` where `j = (n >> 23) & 0x1ff`
pub fn float2half(flt: f32) -> u16 {
    let n = flt.to_bits();
    let j = ((n >> 23) & 0x1ff) as usize;
    (BASE[j] as u32 + ((n & 0x007f_ffff) >> SHIFT[j])) as u16
}
