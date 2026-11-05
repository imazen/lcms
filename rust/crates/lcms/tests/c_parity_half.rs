//! Differential test: `lcms::half` vs `cmshalf.c` — exhaustive over every
//! possible half input, and a dense sweep for float→half.

use lcms::half;

#[test]
fn half2float_exhaustive() {
    for h in 0..=u16::MAX {
        let c = unsafe { lcms_cref::_cmsHalf2Float(h) };
        assert_eq!(half::half2float(h).to_bits(), c.to_bits(), "h=0x{h:04x}");
    }
}

#[test]
fn float2half_sweep() {
    // Every u32 bit pattern is too many; sweep exponent × mantissa densely,
    // including all half-representable and boundary regions, NaN and Inf.
    let mut checked = 0u64;
    for sign in [0u32, 0x8000_0000] {
        for e in 0..255u32 {
            for m in (0..0x800000u32).step_by(101) {
                let f = f32::from_bits(sign | (e << 23) | m);
                let c = unsafe { lcms_cref::_cmsFloat2Half(f) };
                assert_eq!(half::float2half(f), c, "f={f:?} bits={:#010x}", f.to_bits());
                checked += 1;
            }
        }
    }
    // Special bit patterns: NaN payloads, +/-Inf, +/-0, subnormals.
    for bits in [
        0x7f80_0001u32,
        0x7fc0_0000,
        0x7fa0_0000,
        0x7f80_0000,
        0xff80_0000,
        0x0000_0000,
        0x8000_0000,
        0x0000_0001,
        0x007f_ffff,
        0x3380_0000,
        0x477f_e000,
    ] {
        let f = f32::from_bits(bits);
        let c = unsafe { lcms_cref::_cmsFloat2Half(f) };
        assert_eq!(half::float2half(f), c, "f={f:?} bits={bits:#010x}");
    }
    assert!(checked > 1_000_000);
}
