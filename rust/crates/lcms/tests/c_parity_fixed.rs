//! Differential test: `lcms::fixed` vs the reference C inlines
//! (`lcms-cref` shims + exported `_cms*To*` functions).

use lcms::fixed;

fn rng(state: &mut u64) -> u64 {
    *state = state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    *state >> 11
}

fn f64_in(state: &mut u64, lo: f64, hi: f64) -> f64 {
    let u = rng(state) as f64 / (1u64 << 53) as f64;
    lo + u * (hi - lo)
}

#[test]
fn quick_floor_matches_c_over_domain() {
    // The magic trick is defined for |v| < ~32767; sweep it plus edges.
    let mut s = 0x12345u64;
    for _ in 0..200_000 {
        let v = f64_in(&mut s, -32766.999, 32766.999);
        let c = unsafe { lcms_cref::cref_quick_floor(v) };
        assert_eq!(fixed::quick_floor(v), c, "v={v:?}");
    }
    // Exact integers and half steps — boundary precision.
    for k in -4000..4000 {
        for frac in [0.0, 0.25, 0.5, 0.75, 0.999999] {
            let v = k as f64 + frac;
            let c = unsafe { lcms_cref::cref_quick_floor(v) };
            assert_eq!(fixed::quick_floor(v), c, "v={v:?}");
        }
    }
}

#[test]
fn quick_floor_word_matches_c() {
    let mut s = 0x77u64;
    for _ in 0..200_000 {
        let v = f64_in(&mut s, -1.0, 65536.999);
        let c = unsafe { lcms_cref::cref_quick_floor_word(v) };
        assert_eq!(fixed::quick_floor_word(v), c, "v={v:?}");
    }
}

#[test]
fn quick_saturate_word_matches_c() {
    let mut s = 0x55u64;
    for _ in 0..200_000 {
        let v = f64_in(&mut s, -1000.0, 67000.0);
        let c = unsafe { lcms_cref::cref_quick_saturate_word(v) };
        assert_eq!(fixed::quick_saturate_word(v), c, "v={v:?}");
    }
    // Saturation rails and the d += 0.5 rounding edge.
    for v in [
        -1e-9, 0.0, 0.49999, 0.5, 0.500001, 1.0, 65534.4999, 65534.5, 65535.0, 65536.0,
    ] {
        let c = unsafe { lcms_cref::cref_quick_saturate_word(v) };
        assert_eq!(fixed::quick_saturate_word(v), c, "v={v:?}");
    }
}

#[test]
fn fixed_domain_round_trips() {
    for v in -65536..=65536 {
        let c = unsafe { lcms_cref::cref_to_fixed_domain(v) };
        assert_eq!(fixed::to_fixed_domain(v), c, "to_fixed_domain v={v}");
        let c = unsafe { lcms_cref::cref_from_fixed_domain(v) };
        assert_eq!(fixed::from_fixed_domain(v), c, "from_fixed_domain v={v}");
    }
}

#[test]
fn fixed16_conversions_match_c() {
    let mut s = 0x9u64;
    for _ in 0..200_000 {
        let v = f64_in(&mut s, -400.0, 400.0);
        let c = unsafe { lcms_cref::_cmsDoubleTo15Fixed16(v) };
        assert_eq!(fixed::double_to_s15_fixed16(v), c, "v={v:?}");
    }
    for v in (i32::MIN + 65536..i32::MAX - 65536).step_by(997) {
        let c = unsafe { lcms_cref::_cms15Fixed16toDouble(v) };
        assert_eq!(
            fixed::s15_fixed16_to_double(v).to_bits(),
            c.to_bits(),
            "v={v}"
        );
    }
}

#[test]
fn fixed8_conversions_match_c() {
    for v in 0..=u16::MAX {
        let c = unsafe { lcms_cref::_cms8Fixed8toDouble(v) };
        assert_eq!(
            fixed::u8_fixed8_to_double(v).to_bits(),
            c.to_bits(),
            "v={v}"
        );
    }
    let mut s = 0xabcu64;
    for _ in 0..100_000 {
        let v = f64_in(&mut s, -10.0, 300.0);
        let c = unsafe { lcms_cref::_cmsDoubleTo8Fixed8(v) };
        assert_eq!(fixed::double_to_u8_fixed8(v), c, "v={v:?}");
    }
}
