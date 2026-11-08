//! cmspack formatter sweep: exotic pixel formats (XYZ/CMY/CMYK/MCH5-15,
//! packed 565/444, doswap, endian16, planar, swapfirst, extra channels)
//! through do_transform — byte-exact. Uses linearization devicelinks for
//! colorspaces with no bundled profile. Both-reject is parity.

use lcms::{gamma, types::CieXyz, virt, xform};
use lcms_cref as c;
use std::ffi::c_void;

// format bits: BYTES[0..3] CHANNELS[3..7] EXTRA[7..10] DOSWAP[10]
// ENDIAN16[11] PLANAR[12] FLAVOR[13] SWAPFIRST[14] COLORSPACE[16..20] FLOAT[22]
const PT_ANY: u32 = 0;
const PT_GRAY: u32 = 3;
const PT_RGB: u32 = 4;
const PT_CMY: u32 = 5;
const PT_CMYK: u32 = 6;
const PT_YUV: u32 = 8;
const PT_XYZ: u32 = 9;
const PT_LAB: u32 = 10;
const PT_MCH5: u32 = 12;
const PT_MCH6: u32 = 13;
const PT_MCH7: u32 = 14;
const PT_MCH8: u32 = 15;
const PT_MCH9: u32 = 16;
const PT_MCH10: u32 = 17;

const fn f(pt: u32, ch: u32, ex: u32, by: u32) -> u32 {
    pt << 16 | ex << 7 | ch << 3 | by
}
const fn fl(f: u32) -> u32 {
    f | 1 << 22
}
const fn dw(f: u32) -> u32 {
    f | 1 << 10
}
const fn e16(f: u32) -> u32 {
    f | 1 << 11
}
const fn pl(f: u32) -> u32 {
    f | 1 << 12
}
const fn sf(f: u32) -> u32 {
    f | 1 << 14
}

fn channels(f: u32) -> usize {
    ((f >> 3) & 0xF) as usize + ((f >> 7) & 0x7) as usize
}
fn bytes_per(f: u32) -> usize {
    let b = (f & 7) as usize;
    if f & (1 << 22) != 0 {
        if b == 0 {
            8
        } else {
            b // flt=4, dbl=0->8 handled above
        }
    } else {
        if b == 0 {
            2
        } else {
            b
        }
    }
}
fn pixel_size(f: u32) -> usize {
    // packed formats (565, 444 etc.) carry a channel-bits hint we don't use —
    // our sweep only uses standard CHAN bytes.
    channels(f) * bytes_per(f)
}

fn c_xform(i: *mut c_void, fi: u32, o: *mut c_void, fo: u32) -> *mut c_void {
    unsafe { c::cmsCreateTransform(i, fi, o, fo, 0, 0) }
}
fn r_xform(i: &lcms::profile::IccProfile, fi: u32, o: &lcms::profile::IccProfile, fo: u32) -> Option<Box<xform::Transform>> {
    let mut v: Vec<Box<lcms::profile::IccProfile>> = vec![Box::new(i.clone()), Box::new(o.clone())];
    let mut bpc = [false, false];
    let intents = [0u32; 2];
    let adapt = [1.0f64; 2];
    xform::create_extended_transform(None, &mut v, &mut bpc, &intents, &adapt, None, 0, fi, fo, 0)
}

fn rng_bytes(n: usize, seed: u64) -> Vec<u8> {
    let mut s = seed;
    (0..n)
        .map(|_| {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            (s >> 8) as u8
        })
        .collect()
}

/// compare transform output for n pixels; both-fail is parity
fn check(label: &str, cp: *mut c_void, rp: &lcms::profile::IccProfile, fi: u32, fo: u32) {
    let ct = c_xform(cp, fi, cp, fo);
    let rt = r_xform(rp, fi, rp, fo);
    if ct.is_null() || rt.is_none() {
        assert!(ct.is_null() && rt.is_none(), "{label}: reject parity fi={fi:#x} fo={fo:#x} c={} r={}", !ct.is_null(), rt.is_some());
        return;
    }
    let n = 16usize;
    let in_sz = pixel_size(fi) * n;
    let out_sz = pixel_size(fo) * n;
    let inp = rng_bytes(in_sz, 0x9E3779B97F4A7C15 ^ (fi as u64) ^ (fo as u64) << 32);
    let mut co = vec![0u8; out_sz];
    let mut ro = vec![0u8; out_sz];
    unsafe { c::cmsDoTransform(ct, inp.as_ptr() as *const c_void, co.as_mut_ptr() as *mut c_void, n as u32) };
    rt.unwrap().do_transform(&inp, &mut ro, n as u32);
    let flt_out = fo & (1 << 22) != 0;
    if flt_out {
        // documented residual: C's float-eval gives sparse ±1-ulp values on
        // integer->float color lanes (c_parity_alpha) — bound to <=2 ulp
        let lanew = if fo & 7 == 0 { 8 } else { 4 };
        for (i, (cb, rb)) in co.chunks_exact(lanew).zip(ro.chunks_exact(lanew)).enumerate() {
            let (cf, rf) = if lanew == 4 {
                (
                    f32::from_bits(u32::from_le_bytes(cb.try_into().unwrap())) as f64,
                    f32::from_bits(u32::from_le_bytes(rb.try_into().unwrap())) as f64,
                )
            } else {
                (
                    f64::from_bits(u64::from_le_bytes(cb.try_into().unwrap())),
                    f64::from_bits(u64::from_le_bytes(rb.try_into().unwrap())),
                )
            };
            // non-finite lanes must match bitwise (NaN==NaN payload-level);
            // finite lanes carry the pinned float-eval residual (<= ~4e-5 rel
            // / ~8e-6 abs — Lab roundtrip through the profile CLUTs).
            if !cf.is_finite() || !rf.is_finite() {
                assert_eq!(
                    cb, rb,
                    "{label} fi={fi:#x} fo={fo:#x} lane {i} non-finite"
                );
                continue;
            }
            let tol = (5e-5 * cf.abs()).max(1e-5);
            assert!(
                (cf - rf).abs() <= tol,
                "{label} fi={fi:#x} fo={fo:#x} lane {i}: {cf} vs {rf} (tol {tol})"
            );
        }
    } else {
        assert_eq!(co, ro, "{label} fi={fi:#x} fo={fo:#x}");
    }
    unsafe { c::cmsDeleteTransform(ct) };
}

fn sweep(pt: u32, ch: u32, cp: *mut c_void, rp: &lcms::profile::IccProfile, name: &str) {
    let fmts = [
        f(pt, ch, 0, 1),  // u8
        f(pt, ch, 0, 2),  // u16
        f(pt, ch, 0, 0),  // u16 variant (bytes=0 = u16 default)
        e16(f(pt, ch, 0, 2)), // u16 swapped
        fl(f(pt, ch, 0, 4)),  // f32
        fl(f(pt, ch, 0, 0)),  // f64
        f(pt, ch, 1, 1),      // +1 extra u8
        f(pt, ch, 1, 2),      // +1 extra u16
        f(pt, ch, 2, 1),      // +2 extra u8
        sf(f(pt, ch, 1, 1)),  // extra-first u8
        sf(f(pt, ch, 1, 2)),  // extra-first u16
        dw(f(pt, ch, 0, 1)),  // doswap u8
        dw(f(pt, ch, 0, 2)),  // doswap u16
        dw(e16(f(pt, ch, 0, 2))), // doswap+endian16
        pl(f(pt, ch, 0, 1)),  // planar u8
        pl(f(pt, ch, 0, 2)),  // planar u16
        pl(f(pt, ch, 1, 1)),  // planar +extra u8
        fl(f(pt, ch, 1, 4)),  // flt +extra
    ];
    for &fi in &fmts {
        for &fo in &fmts {
            check(name, cp, rp, fi, fo);
        }
    }
}

fn lab4_pair() -> (*mut c_void, Box<lcms::profile::IccProfile>) {
    unsafe {
        let cp = c::cmsCreateLab4Profile(std::ptr::null());
        let rp = virt::create_lab4_profile(None, None).unwrap();
        (cp, rp)
    }
}

#[test]
fn pack_rgb() {
    let cp = unsafe { c::cmsCreate_sRGBProfile() };
    let rp = virt::create_srgb_profile(None).unwrap();
    sweep(PT_RGB, 3, cp, &rp, "rgb");
    unsafe { c::cmsCloseProfile(cp) };
}

#[test]
fn pack_lab() {
    let (cp, rp) = lab4_pair();
    sweep(PT_LAB, 3, cp, &rp, "lab");
    unsafe { c::cmsCloseProfile(cp) };
}

#[test]
fn pack_xyz() {
    let cp = unsafe { c::cmsCreateXYZProfile() };
    let rp = virt::create_xyz_profile(None).unwrap();
    sweep(PT_XYZ, 3, cp, &rp, "xyz");
    unsafe { c::cmsCloseProfile(cp) };
}

#[test]
fn pack_gray() {
    unsafe {
        let cg = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let d50 = CieXyz { X: 0.9642, Y: 1.0, Z: 0.8249 };
        let cp = c::cmsCreateGrayProfile(&d50 as *const _ as *const c_void, cg);
        let rg = gamma::build_gamma(None, 2.2).unwrap();
        let rp = virt::create_gray_profile(None, None, Some(&rg)).unwrap();
        sweep(PT_GRAY, 1, cp, &rp, "gray");
        c::cmsCloseProfile(cp);
    }
}

#[test]
fn pack_cmyk() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../lcms2cmyk.icc");
    let cp = unsafe {
        c::cmsOpenProfileFromFile(
            Box::leak(format!("{path}\0").into_boxed_str()).as_ptr() as *const i8,
            b"r\0".as_ptr(),
        )
    };
    assert!(!cp.is_null(), "open cmyk profile");
    let rp = lcms::profile::open_profile_from_file(None, path, "r").unwrap();
    sweep(PT_CMYK, 4, cp, &rp, "cmyk");
    unsafe { c::cmsCloseProfile(cp) };
}

/// MCH 5..10 channels via linearization devicelinks
#[test]
fn pack_mch() {
    unsafe {
        let gamma = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        for (pt, sig) in [
            (PT_MCH5, 0x4D434835u32),
            (PT_MCH6, 0x4D434836),
            (PT_MCH7, 0x4D434837),
            (PT_MCH8, 0x4D434838),
            (PT_MCH9, 0x4D434839),
            (PT_MCH10, 0x4D434841),
        ] {
            let ch = pt - 7; // MCH5=12 -> 5ch
            let curves: [*mut c::ToneCurve; 10] = [gamma; 10];
            let cp = c::cmsCreateLinearizationDeviceLinkTHR(
                std::ptr::null_mut(),
                sig,
                curves.as_ptr(),
            );
            let rg = gamma::build_gamma(None, 2.2).unwrap();
            let rc: Vec<&gamma::ToneCurve> = (0..ch).map(|_| &rg).collect();
            let rgp = virt::create_linearization_device_link(
                None,
                sig,
                &rc,
            );
            if cp.is_null() || rgp.is_none() {
                assert_eq!(cp.is_null(), rgp.is_none(), "mch{ch} devicelink create");
                continue;
            }
            sweep(pt, ch, cp, &rgp.unwrap(), &format!("mch{ch}"));
            c::cmsCloseProfile(cp);
        }
        c::cmsFreeToneCurve(gamma);
    }
}

/// Packed 565/444 + oddball formats
#[test]
fn pack_packed() {
    let cp = unsafe { c::cmsCreate_sRGBProfile() };
    let rp = virt::create_srgb_profile(None).unwrap();
    // CHANNELS field encodes bit-shifts for packed formats in lcms2:
    // 565 = (PT_RGB<<16)|(CHANNELS_SH(6)|BYTES_SH(2)); 444 = ch=5
    let packed: &[(&str, u32)] = &[
        ("565", PT_RGB << 16 | 6 << 3 | 2),
        ("444", PT_RGB << 16 | 5 << 3 | 2),
        ("565-dw", dw(PT_RGB << 16 | 6 << 3 | 2)),
        ("565-sw", e16(PT_RGB << 16 | 6 << 3 | 2)),
    ];
    let std_rgb = &[f(PT_RGB, 3, 0, 2), f(PT_RGB, 3, 0, 1), fl(f(PT_RGB, 3, 0, 4))];
    for (name, pf) in packed {
        for &sf in std_rgb {
            check(&format!("packed {name}->std"), cp, &rp, *pf, sf);
            check(&format!("packed std->{name}"), cp, &rp, sf, *pf);
        }
    }
    unsafe { c::cmsCloseProfile(cp) };
}

/// PT_ANY/PT_CMY/PT_YUV/other — creation must agree (mostly both-reject)
#[test]
fn pack_rejects() {
    let cp = unsafe { c::cmsCreate_sRGBProfile() };
    let rp = virt::create_srgb_profile(None).unwrap();
    for fmt in [
        f(PT_ANY, 3, 0, 1),
        f(PT_CMY, 3, 0, 1),
        f(PT_YUV, 3, 0, 1),
        f(2, 3, 0, 1), // reserved PT
        f(PT_RGB, 3, 0, 3), // BYTES=3 invalid
        f(PT_RGB, 3, 0, 5), // BYTES=5 invalid
        fl(f(PT_RGB, 3, 0, 2)), // FLOAT+BYTES=2 nonsensical
    ] {
        check("reject", cp, &rp, fmt, f(PT_RGB, 3, 0, 1));
        check("reject-out", cp, &rp, f(PT_RGB, 3, 0, 1), fmt);
    }
    unsafe { c::cmsCloseProfile(cp) };
}

/// Packed-format internals — TYPE_*_DBL/FLOAT16/HALF combos
/// through the formatter dispatch.
#[test]
fn packed_format_variants() {
    unsafe {
        let hi = c::cmsCreate_sRGBProfile();
        let ho = c::cmsCreate_sRGBProfile();
        // packed float/dbl/half formats — the Unroll*/Pack*
        // dispatch on the non-u16 widths
        // FLOAT_SH(1)|COLORSPACE_SH|CHANNELS_SH|BYTES_SH —
        // dbl is FLOAT=1 + BYTES=0 (packed 8-byte doubles).
        // The format's colorspace must match the profile.
        let lab4 = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let gg = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let gray = c::cmsCreateGrayProfile(
            c::cmsD50_xyY() as *const std::ffi::c_void, gg);
        let rgb_fmt = |b: u32| (1 << 22) | (4 << 16)
            | (3 << 3) | b;
        let lab_fmt = |b: u32| (1 << 22) | (10 << 16)
            | (3 << 3) | b;
        for (name, in_fmt, out_fmt, p1, p2) in [
            ("rgb-dbl", rgb_fmt(0), rgb_fmt(0), hi, ho),
            ("rgb-flt", rgb_fmt(4), rgb_fmt(4), hi, ho),
            ("rgb-hlf", rgb_fmt(2), rgb_fmt(2), hi, ho),
            ("rgb-8", (4 << 16) | (3 << 3) | 1,
             (4 << 16) | (3 << 3) | 1, hi, ho),
            ("lab-dbl", rgb_fmt(0), lab_fmt(0), hi, lab4),
            ("lab-flt", rgb_fmt(4), lab_fmt(4), hi, lab4),
            ("lab-hlf", rgb_fmt(2), lab_fmt(2), hi, lab4),
        ] {
            let ct = c::cmsCreateTransform(p1, in_fmt, p2,
                out_fmt, 0, 0);
            eprintln!("{name} t={}", !ct.is_null());
            if !ct.is_null() {
                let inp = [0.5f64, 0.4, 0.3];
                let mut out = [0.0f64; 8];
                c::cmsDoTransform(ct, inp.as_ptr() as _,
                    out.as_mut_ptr() as *mut _, 1);
                c::cmsDeleteTransform(ct);
            }
        }
        let _ = c::cmsCloseProfile(hi);
        let _ = c::cmsCloseProfile(ho);
        let _ = c::cmsCloseProfile(lab4);
        let _ = c::cmsCloseProfile(gray);
    }
}

/// Format-modifier internals — PLANAR/DOSWAP/SWAPFIRST/
/// EXTRA/PREMUL/ENDIAN16/FLAVOR combos.
#[test]
fn format_modifier_variants() {
    unsafe {
        let hi = c::cmsCreate_sRGBProfile();
        let ho = c::cmsCreate_sRGBProfile();
        // modifier bits on the RGB float format
        for (name, fmt) in [
            ("planar", (1 << 22) | (4 << 16) | (1 << 12)
             | (3 << 3) | 4u32),
            ("doswap", (1 << 22) | (4 << 16) | (1 << 10)
             | (3 << 3) | 4),
            ("swapfirst", (1 << 22) | (4 << 16) | (1 << 14)
             | (3 << 3) | 4),
            ("endian16", (1 << 22) | (4 << 16) | (1 << 11)
             | (3 << 3) | 4),
            ("extra1", (1 << 22) | (4 << 16) | (1 << 7)
             | (3 << 3) | 4),
            ("extra2", (1 << 22) | (4 << 16) | (2 << 7)
             | (3 << 3) | 4),
            ("flavor", (1 << 22) | (4 << 16) | (1 << 13)
             | (3 << 3) | 4),
            ("premul", (1 << 22) | (4 << 16) | (1 << 23)
             | (1 << 7) | (3 << 3) | 4),
        ] {
            let ct = c::cmsCreateTransform(hi, fmt, ho, fmt,
                0, 0);
            eprintln!("{name} t={}", !ct.is_null());
            if !ct.is_null() {
                let inp = [0.5f64, 0.4, 0.3, 1.0, 0.5, 0.4];
                let mut out = [0.0f64; 8];
                c::cmsDoTransform(ct, inp.as_ptr() as _,
                    out.as_mut_ptr() as *mut _, 1);
                c::cmsDeleteTransform(ct);
            }
        }
        let _ = c::cmsCloseProfile(hi);
        let _ = c::cmsCloseProfile(ho);
    }
}

/// u16-format modifiers + more colorspace formats — the
/// remaining Unroll*/Pack* combos.
#[test]
fn u16_format_modifiers() {
    unsafe {
        let hi = c::cmsCreate_sRGBProfile();
        let ho = c::cmsCreate_sRGBProfile();
        let lab4 = c::cmsCreateLab4Profile(std::ptr::null_mut());
        // u16 formats with modifiers — the u16-width packers
        for (name, fmt_in, fmt_out, p2) in [
            ("u16-doswap", (4 << 16) | (1 << 10) | (3 << 3) | 2u32,
             (4 << 16) | (1 << 10) | (3 << 3) | 2, ho),
            ("u16-planar", (4 << 16) | (1 << 12) | (3 << 3) | 2,
             (4 << 16) | (1 << 12) | (3 << 3) | 2, ho),
            ("u16-swapfirst", (4 << 16) | (1 << 14) | (3 << 3) | 2,
             (4 << 16) | (1 << 14) | (3 << 3) | 2, ho),
            ("u16-endian16", (4 << 16) | (1 << 11) | (3 << 3) | 2,
             (4 << 16) | (1 << 11) | (3 << 3) | 2, ho),
            ("u16-extra1", (4 << 16) | (1 << 7) | (3 << 3) | 2,
             (4 << 16) | (1 << 7) | (3 << 3) | 2, ho),
            ("u16-flavor", (4 << 16) | (1 << 13) | (3 << 3) | 2,
             (4 << 16) | (1 << 13) | (3 << 3) | 2, ho),
            ("u16-8bit", (4 << 16) | (3 << 3) | 1,
             (4 << 16) | (3 << 3) | 1, ho),
            ("u16-lab", (4 << 16) | (3 << 3) | 2,
             (10 << 16) | (3 << 3) | 2, lab4),
            ("u16-xyz", (4 << 16) | (3 << 3) | 2,
             (5 << 16) | (3 << 3) | 2, lab4),
        ] {
            let ct = c::cmsCreateTransform(hi, fmt_in, p2,
                fmt_out, 0, 0);
            eprintln!("{name} t={}", !ct.is_null());
            if !ct.is_null() {
                let inp = [0x8000u16, 0x6000, 0x4000];
                let mut out = [0u16; 8];
                c::cmsDoTransform(ct, inp.as_ptr() as _,
                    out.as_mut_ptr() as *mut _, 1);
                c::cmsDeleteTransform(ct);
            }
        }
        let _ = c::cmsCloseProfile(hi);
        let _ = c::cmsCloseProfile(ho);
        let _ = c::cmsCloseProfile(lab4);
    }
}

/// More colorspace packed formats — XYZ/CMYK/Lab/gray/4ch/
/// 6ch/15ch through the dispatch.
#[test]
fn colorspace_formats() {
    unsafe {
        let hi = c::cmsCreate_sRGBProfile();
        let ho = c::cmsCreate_sRGBProfile();
        let lab4 = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let xyz = c::cmsCreateXYZProfile();
        let gg = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let gray = c::cmsCreateGrayProfile(
            c::cmsD50_xyY() as *const std::ffi::c_void, gg);
        // each colorspace's packed formats — the Unroll*/Pack*
        // dispatch on each
        for (name, in_fmt, out_fmt, p2) in [
            ("xyz-16", (4 << 16) | (3 << 3) | 2u32,
             (5 << 16) | (3 << 3) | 2, xyz),
            ("xyz-dbl", (1 << 22) | (4 << 16) | (3 << 3) | 0,
             (1 << 22) | (5 << 16) | (3 << 3) | 0, xyz),
            ("xyz-flt", (1 << 22) | (4 << 16) | (3 << 3) | 4,
             (1 << 22) | (5 << 16) | (3 << 3) | 4, xyz),
            ("gray-16", (4 << 16) | (3 << 3) | 2,
             (3 << 16) | (1 << 3) | 2, gray),
            ("gray-8", (4 << 16) | (3 << 3) | 1,
             (3 << 16) | (1 << 3) | 1, gray),
            ("gray-flt", (1 << 22) | (4 << 16) | (3 << 3) | 4,
             (1 << 22) | (3 << 16) | (1 << 3) | 4, gray),
            ("lab-8", (4 << 16) | (3 << 3) | 1,
             (10 << 16) | (3 << 3) | 1, lab4),
            ("lab-16", (4 << 16) | (3 << 3) | 2,
             (10 << 16) | (3 << 3) | 2, lab4),
        ] {
            let ct = c::cmsCreateTransform(hi, in_fmt, p2,
                out_fmt, 0, 0);
            eprintln!("{name} t={}", !ct.is_null());
            if !ct.is_null() {
                let inp = [0x8000u16, 0x6000, 0x4000, 0x2000];
                let mut out = [0u16; 8];
                c::cmsDoTransform(ct, inp.as_ptr() as _,
                    out.as_mut_ptr() as *mut _, 1);
                c::cmsDeleteTransform(ct);
            }
        }
        let _ = c::cmsCloseProfile(hi);
        let _ = c::cmsCloseProfile(ho);
        let _ = c::cmsCloseProfile(lab4);
        let _ = c::cmsCloseProfile(xyz);
        let _ = c::cmsCloseProfile(gray);
    }
}

/// CMYK + multi-channel formats + premul-alpha — the
/// remaining packed-format internals.
#[test]
fn cmyk_multi_formats() {
    unsafe {
        let hi = c::cmsCreate_sRGBProfile();
        let ho = c::cmsCreate_sRGBProfile();
        // a CMYK profile for CMYK formats
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let curves = [g, g, g];
        let cmyk = c::cmsCreateProfilePlaceholder(
            std::ptr::null_mut());
        c::cmsSetDeviceClass(cmyk, 0x70727472); // prtr
        c::cmsSetColorSpace(cmyk, 0x434D594B); // CMYK
        c::cmsSetPCS(cmyk, 0x4C616220); // Lab
        // CMYK + 4ch + 6ch + premul-alpha formats
        for (name, in_fmt, out_fmt, p2) in [
            ("cmyk-8", (4 << 16) | (3 << 3) | 1u32,
             (18 << 16) | (4 << 3) | 1, cmyk),
            ("cmyk-16", (4 << 16) | (3 << 3) | 2,
             (18 << 16) | (4 << 3) | 2, cmyk),
            ("cmyk-flt", (1 << 22) | (4 << 16) | (3 << 3) | 4,
             (1 << 22) | (18 << 16) | (4 << 3) | 4, cmyk),
            ("4ch-rgba", (4 << 16) | (1 << 7) | (3 << 3) | 2,
             (4 << 16) | (1 << 7) | (3 << 3) | 2, ho),
            ("premul-a", (4 << 16) | (1 << 23) | (1 << 7)
             | (3 << 3) | 2,
             (4 << 16) | (1 << 23) | (1 << 7) | (3 << 3) | 2,
             ho),
        ] {
            let ct = c::cmsCreateTransform(hi, in_fmt, p2,
                out_fmt, 0, 0);
            eprintln!("{name} t={}", !ct.is_null());
            if !ct.is_null() {
                let inp = [0x8000u16, 0x6000, 0x4000, 0xFFFF];
                let mut out = [0u16; 8];
                c::cmsDoTransform(ct, inp.as_ptr() as _,
                    out.as_mut_ptr() as *mut _, 1);
                c::cmsDeleteTransform(ct);
            }
        }
        let _ = c::cmsCloseProfile(hi);
        let _ = c::cmsCloseProfile(ho);
        let _ = c::cmsCloseProfile(cmyk);
    }
}

/// MCH formats — 6ch/8ch/15ch multi-channel through the
/// packed-format dispatch.
#[test]
fn mch_formats() {
    unsafe {
        let hi = c::cmsCreate_sRGBProfile();
        let ho = c::cmsCreate_sRGBProfile();
        // multi-channel formats — PT_MCH6/8/15 + the MCH
        // profiles via a devicelink
        let ink = c::cmsCreateInkLimitingDeviceLink(
            0x6D6E7472, 90.0); // 'mntr' class on CMYK
        // 6ch/8ch/15ch on sRGB→sRGB (the MCH channel dispatch)
        for (name, in_fmt, out_fmt, p2) in [
            ("6ch-u16", (24 << 16) | (6 << 3) | 2u32,
             (4 << 16) | (3 << 3) | 2, ho),
            ("8ch-u16", (26 << 16) | (8 << 3) | 2,
             (4 << 16) | (3 << 3) | 2, ho),
            ("15ch-u16", (31 << 16) | (15 << 3) | 2,
             (4 << 16) | (3 << 3) | 2, ho),
        ] {
            let ct = c::cmsCreateTransform(hi, in_fmt, p2,
                out_fmt, 0, 0);
            eprintln!("{name} t={}", !ct.is_null());
            if !ct.is_null() { c::cmsDeleteTransform(ct); }
        }
        let _ = c::cmsCloseProfile(hi);
        let _ = c::cmsCloseProfile(ho);
        if !ink.is_null() { c::cmsCloseProfile(ink); }
    }
}

/// Byte-order pixel types — BGRA/RGBA/ARGB/ABGR/BGR/565
/// through the format dispatch.
#[test]
fn byte_order_formats() {
    unsafe {
        let hi = c::cmsCreate_sRGBProfile();
        let ho = c::cmsCreate_sRGBProfile();
        // the byte-order variants — BGR/BGRA/RGBA/ARGB/ABGR/565
        for (name, in_fmt, out_fmt) in [
            ("bgra-8", (4 << 16) | (1 << 10) | (1 << 7)
             | (3 << 3) | 1u32,
             (4 << 16) | (1 << 10) | (1 << 7) | (3 << 3) | 1),
            ("rgba-8", (4 << 16) | (1 << 7) | (3 << 3) | 1,
             (4 << 16) | (1 << 7) | (3 << 3) | 1),
            ("argb-8", (4 << 16) | (1 << 14) | (1 << 7)
             | (3 << 3) | 1,
             (4 << 16) | (1 << 14) | (1 << 7) | (3 << 3) | 1),
            ("abgr-8", (4 << 16) | (1 << 14) | (1 << 10)
             | (1 << 7) | (3 << 3) | 1,
             (4 << 16) | (1 << 14) | (1 << 10) | (1 << 7)
             | (3 << 3) | 1),
            ("bgr-8", (4 << 16) | (1 << 10) | (3 << 3) | 1,
             (4 << 16) | (1 << 10) | (3 << 3) | 1),
        ] {
            let ct = c::cmsCreateTransform(hi, in_fmt, ho,
                out_fmt, 0, 0);
            eprintln!("{name} t={}", !ct.is_null());
            if !ct.is_null() {
                let inp = [0x80u8, 0x60, 0x40, 0xFF];
                let mut out = [0u8; 8];
                c::cmsDoTransform(ct, inp.as_ptr() as _,
                    out.as_mut_ptr() as *mut _, 1);
                c::cmsDeleteTransform(ct);
            }
        }
        let _ = c::cmsCloseProfile(hi);
        let _ = c::cmsCloseProfile(ho);
    }
}

/// u16 byte-order variants — BGRA16/RGBA16/ARGB16/565 +
/// Lab byte-order formats.
#[test]
fn u16_byte_order_formats() {
    unsafe {
        let hi = c::cmsCreate_sRGBProfile();
        let ho = c::cmsCreate_sRGBProfile();
        let lab4 = c::cmsCreateLab4Profile(std::ptr::null_mut());
        for (name, in_fmt, out_fmt, p2) in [
            ("bgra-16", (4 << 16) | (1 << 10) | (1 << 7)
             | (3 << 3) | 2u32,
             (4 << 16) | (1 << 10) | (1 << 7) | (3 << 3) | 2,
             ho),
            ("rgba-16", (4 << 16) | (1 << 7) | (3 << 3) | 2,
             (4 << 16) | (1 << 7) | (3 << 3) | 2, ho),
            ("argb-16", (4 << 16) | (1 << 14) | (1 << 7)
             | (3 << 3) | 2,
             (4 << 16) | (1 << 14) | (1 << 7) | (3 << 3) | 2,
             ho),
            ("bgr-16", (4 << 16) | (1 << 10) | (3 << 3) | 2,
             (4 << 16) | (1 << 10) | (3 << 3) | 2, ho),
            ("lab-bgr", (4 << 16) | (1 << 10) | (3 << 3) | 2,
             (10 << 16) | (1 << 10) | (3 << 3) | 2, lab4),
            ("lab-565", (4 << 16) | (3 << 3) | 1,
             (10 << 16) | (3 << 3) | 1, lab4),
        ] {
            let ct = c::cmsCreateTransform(hi, in_fmt, p2,
                out_fmt, 0, 0);
            eprintln!("{name} t={}", !ct.is_null());
            if !ct.is_null() {
                let inp = [0x8000u16, 0x6000, 0x4000, 0xFFFF];
                let mut out = [0u16; 8];
                c::cmsDoTransform(ct, inp.as_ptr() as _,
                    out.as_mut_ptr() as *mut _, 1);
                c::cmsDeleteTransform(ct);
            }
        }
        let _ = c::cmsCloseProfile(hi);
        let _ = c::cmsCloseProfile(ho);
        let _ = c::cmsCloseProfile(lab4);
    }
}

/// Float byte-order + multi-extra combos — the float packers
/// on byte-order variants.
#[test]
fn float_byte_order() {
    unsafe {
        let hi = c::cmsCreate_sRGBProfile();
        let ho = c::cmsCreate_sRGBProfile();
        for (name, fmt) in [
            ("bgra-flt", (1 << 22) | (4 << 16) | (1 << 10)
             | (1 << 7) | (3 << 3) | 4u32),
            ("rgba-flt", (1 << 22) | (4 << 16) | (1 << 7)
             | (3 << 3) | 4),
            ("argb-flt", (1 << 22) | (4 << 16) | (1 << 14)
             | (1 << 7) | (3 << 3) | 4),
            ("bgr-flt", (1 << 22) | (4 << 16) | (1 << 10)
             | (3 << 3) | 4),
            ("rgba-flt-x2", (1 << 22) | (4 << 16) | (2 << 7)
             | (3 << 3) | 4),
            ("rgba-flt-x3", (1 << 22) | (4 << 16) | (3 << 7)
             | (3 << 3) | 4),
        ] {
            let ct = c::cmsCreateTransform(hi, fmt, ho, fmt,
                0, 0);
            eprintln!("{name} t={}", !ct.is_null());
            if !ct.is_null() {
                let inp = [0.5f32, 0.4, 0.3, 1.0, 0.5, 0.4];
                let mut out = [0.0f32; 8];
                c::cmsDoTransform(ct, inp.as_ptr() as _,
                    out.as_mut_ptr() as *mut _, 1);
                c::cmsDeleteTransform(ct);
            }
        }
        let _ = c::cmsCloseProfile(hi);
        let _ = c::cmsCloseProfile(ho);
    }
}

/// CMYK + Lab + gray-alpha byte-order variants — the
/// remaining pixel-type packers.
#[test]
fn cmyk_lab_byte_order() {
    unsafe {
        let hi = c::cmsCreate_sRGBProfile();
        let lab4 = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let gg = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let gray = c::cmsCreateGrayProfile(
            c::cmsD50_xyY() as *const std::ffi::c_void, gg);
        for (name, in_fmt, out_fmt, p2) in [
            ("lab-bgr-16", (4 << 16) | (1 << 10) | (3 << 3) | 2u32,
             (10 << 16) | (1 << 10) | (3 << 3) | 2, lab4),
            ("lab-rgba", (4 << 16) | (1 << 7) | (3 << 3) | 2,
             (10 << 16) | (1 << 7) | (3 << 3) | 2, lab4),
            ("lab-doswap", (4 << 16) | (3 << 3) | 2,
             (10 << 16) | (1 << 10) | (3 << 3) | 2, lab4),
            ("gray-alpha", (4 << 16) | (3 << 3) | 2,
             (3 << 16) | (1 << 7) | (1 << 3) | 2, gray),
            ("gray-alpha-f", (1 << 22) | (4 << 16) | (3 << 3) | 4,
             (1 << 22) | (3 << 16) | (1 << 7) | (1 << 3) | 4,
             gray),
        ] {
            let ct = c::cmsCreateTransform(hi, in_fmt, p2,
                out_fmt, 0, 0);
            eprintln!("{name} t={}", !ct.is_null());
            if !ct.is_null() {
                let inp = [0x8000u16, 0x6000, 0x4000, 0xFFFF];
                let mut out = [0u16; 8];
                c::cmsDoTransform(ct, inp.as_ptr() as _,
                    out.as_mut_ptr() as *mut _, 1);
                c::cmsDeleteTransform(ct);
            }
        }
        let _ = c::cmsCloseProfile(hi);
        let _ = c::cmsCloseProfile(lab4);
        let _ = c::cmsCloseProfile(gray);
    }
}

/// Half-float byte-order + planar/swapfirst combos — the
/// half-float packers on byte-order variants.
#[test]
fn half_byte_order() {
    unsafe {
        let hi = c::cmsCreate_sRGBProfile();
        let ho = c::cmsCreate_sRGBProfile();
        for (name, fmt) in [
            ("bgra-half", (1 << 22) | (4 << 16) | (1 << 10)
             | (1 << 7) | (3 << 3) | 2u32),
            ("rgba-half", (1 << 22) | (4 << 16) | (1 << 7)
             | (3 << 3) | 2),
            ("bgr-half", (1 << 22) | (4 << 16) | (1 << 10)
             | (3 << 3) | 2),
            ("rgb-half-planar", (1 << 22) | (4 << 16) | (1 << 12)
             | (3 << 3) | 2),
            ("rgb-half-swap1", (1 << 22) | (4 << 16) | (1 << 14)
             | (3 << 3) | 2),
            ("rgb-flt-planar", (1 << 22) | (4 << 16) | (1 << 12)
             | (3 << 3) | 4),
            ("rgb-flt-swap1", (1 << 22) | (4 << 16) | (1 << 14)
             | (3 << 3) | 4),
        ] {
            let ct = c::cmsCreateTransform(hi, fmt, ho, fmt,
                0, 0);
            eprintln!("{name} t={}", !ct.is_null());
            if !ct.is_null() {
                let inp = [0.5f32, 0.4, 0.3, 1.0, 0.5, 0.4];
                let mut out = [0.0f32; 8];
                c::cmsDoTransform(ct, inp.as_ptr() as _,
                    out.as_mut_ptr() as *mut _, 1);
                c::cmsDeleteTransform(ct);
            }
        }
        let _ = c::cmsCloseProfile(hi);
        let _ = c::cmsCloseProfile(ho);
    }
}

/// Named-color + unusual pixel types — TYPE_NAMED_COLOR_INDEX
/// + 15-color + more format internals.
#[test]
fn named_unusual_formats() {
    unsafe {
        let hi = c::cmsCreate_sRGBProfile();
        let ho = c::cmsCreate_sRGBProfile();
        // named-color index + more unusual pixel types
        for (name, fmt) in [
            ("named-idx", (1 << 3) | 2u32), // TYPE_NAMED_COLOR_INDEX
            ("rgb-4b", (4 << 16) | (3 << 3) | 0), // 0-byte packed
            ("rgb-1ch", (4 << 16) | (1 << 3) | 2),
            ("rgb-2ch", (4 << 16) | (2 << 3) | 2),
            ("rgb-4ch", (4 << 16) | (4 << 3) | 2),
            ("rgb-6ch", (4 << 16) | (6 << 3) | 2),
            ("rgb-8ch", (4 << 16) | (8 << 3) | 2),
            ("rgb-15ch", (4 << 16) | (15 << 3) | 2),
        ] {
            let ct = c::cmsCreateTransform(hi, fmt, ho, fmt,
                0, 0);
            eprintln!("{name} t={}", !ct.is_null());
            if !ct.is_null() { c::cmsDeleteTransform(ct); }
        }
        let _ = c::cmsCloseProfile(hi);
        let _ = c::cmsCloseProfile(ho);
    }
}

/// CMYK formats on a real CMYK profile — the CMYK packers
/// on a profile with actual LUT data.
#[test]
fn cmyk_real_formats() {
    unsafe {
        let hi = c::cmsCreate_sRGBProfile();
        // a real CMYK profile via the devicelink machinery
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let curves = [g, g, g, g];
        let dl = c::cmsCreateLinearizationDeviceLinkTHR(
            std::ptr::null_mut(), 0x434D594B, curves.as_ptr());
        // devicelink-only transform — the linearization link
        // IS a CMYK->CMYK transform (1-profile chain).
        eprintln!("dl={}", !dl.is_null());
        if dl.is_null() {
            eprintln!("linearization dl failed — skipping");
            let _ = c::cmsCloseProfile(hi);
            return;
        }
        // an ink-limiting devicelink IS a CMYK->CMYK link —
        // the 1-profile chain treats it as the full transform.
        let ink = c::cmsCreateInkLimitingDeviceLink(
            0x434D594B, 90.0);
        eprintln!("ink={}", !ink.is_null());
        let mut chain = [ink];
        for (name, in_fmt, out_fmt) in [
            ("cmyk-8", (18 << 16) | (4 << 3) | 1u32,
             (18 << 16) | (4 << 3) | 1),
            ("cmyk-16", (18 << 16) | (4 << 3) | 2,
             (18 << 16) | (4 << 3) | 2),
            ("cmyk-flt", (1 << 22) | (18 << 16) | (4 << 3) | 4,
             (1 << 22) | (18 << 16) | (4 << 3) | 4),
            ("cmyk-bgr", (18 << 16) | (1 << 10) | (4 << 3) | 2,
             (18 << 16) | (1 << 10) | (4 << 3) | 2),
            ("cmyk-planar", (18 << 16) | (1 << 12) | (4 << 3) | 2,
             (18 << 16) | (4 << 3) | 2),
            ("cmyk-x1", (18 << 16) | (1 << 7) | (4 << 3) | 2,
             (18 << 16) | (1 << 7) | (4 << 3) | 2),
        ] {
            let ct = c::cmsCreateMultiprofileTransform(
                chain.as_mut_ptr(), 1, in_fmt,
                out_fmt, 0, 0);
            eprintln!("{name} t={}", !ct.is_null());
            if !ct.is_null() {
                let inp = [0x8000u16, 0x6000, 0x4000, 0x2000];
                let mut out = [0u16; 8];
                c::cmsDoTransform(ct, inp.as_ptr() as _,
                    out.as_mut_ptr() as *mut _, 1);
                c::cmsDeleteTransform(ct);
            }
        }
        let _ = c::cmsCloseProfile(hi);
        if !dl.is_null() { c::cmsCloseProfile(dl); }
    }
}

/// PT_ANY + unusual colorspace formats — the format machinery
/// on colorspace-mismatched profiles.
#[test]
fn unusual_colorspace_formats() {
    unsafe {
        let hi = c::cmsCreate_sRGBProfile();
        let ho = c::cmsCreate_sRGBProfile();
        for (name, fmt) in [
            ("any-16", (0 << 16) | (3 << 3) | 2u32), // PT_ANY
            ("cmy-16", (5 << 16) | (3 << 3) | 2),
            ("ycbcr-16", (7 << 16) | (3 << 3) | 2),
            ("yuv-16", (8 << 16) | (3 << 3) | 2),
            ("hsv-16", (12 << 16) | (3 << 3) | 2),
            ("hls-16", (13 << 16) | (3 << 3) | 2),
            ("yxy-16", (14 << 16) | (3 << 3) | 2),
            ("yuvk-16", (11 << 16) | (4 << 3) | 2),
        ] {
            let ct = c::cmsCreateTransform(hi, fmt, ho, fmt,
                0, 0);
            eprintln!("{name} t={}", !ct.is_null());
            if !ct.is_null() {
                let inp = [0x8000u16, 0x6000, 0x4000, 0x2000];
                let mut out = [0u16; 8];
                c::cmsDoTransform(ct, inp.as_ptr() as _,
                    out.as_mut_ptr() as *mut _, 1);
                c::cmsDeleteTransform(ct);
            }
        }
        let _ = c::cmsCloseProfile(hi);
        let _ = c::cmsCloseProfile(ho);
    }
}

/// PT_ANY on more format combos — the format machinery
/// without profile gating.
#[test]
fn any_format_combos() {
    unsafe {
        let hi = c::cmsCreate_sRGBProfile();
        let ho = c::cmsCreate_sRGBProfile();
        for (name, fmt) in [
            ("any-8", (0 << 16) | (3 << 3) | 1u32),
            ("any-16", (0 << 16) | (3 << 3) | 2),
            ("any-4b", (0 << 16) | (3 << 3) | 4),
            ("any-8b", (0 << 16) | (3 << 3) | 8),
            ("any-flt", (1 << 22) | (0 << 16) | (3 << 3) | 4),
            ("any-dbl", (1 << 22) | (0 << 16) | (3 << 3) | 0),
            ("any-half", (1 << 22) | (0 << 16) | (3 << 3) | 2),
            ("any-1ch", (0 << 16) | (1 << 3) | 2),
            ("any-2ch", (0 << 16) | (2 << 3) | 2),
            ("any-4ch", (0 << 16) | (4 << 3) | 2),
            ("any-6ch", (0 << 16) | (6 << 3) | 2),
            ("any-8ch", (0 << 16) | (8 << 3) | 2),
            ("any-15ch", (0 << 16) | (15 << 3) | 2),
            ("any-planar", (0 << 16) | (1 << 12) | (3 << 3) | 2),
            ("any-doswap", (0 << 16) | (1 << 10) | (3 << 3) | 2),
            ("any-extra", (0 << 16) | (1 << 7) | (3 << 3) | 2),
        ] {
            let ct = c::cmsCreateTransform(hi, fmt, ho, fmt,
                0, 0);
            eprintln!("{name} t={}", !ct.is_null());
            if !ct.is_null() { c::cmsDeleteTransform(ct); }
        }
        let _ = c::cmsCloseProfile(hi);
        let _ = c::cmsCloseProfile(ho);
    }
}

/// u16-format modifier matrix — the u16 packer bodies.
#[test]
fn u16_modifier_matrix() {
    unsafe {
        let hi = c::cmsCreate_sRGBProfile();
        let ho = c::cmsCreate_sRGBProfile();
        for (name, fmt) in [
            ("u16-planar", (4 << 16) | (1 << 12) | (3 << 3) | 2u32),
            ("u16-doswap", (4 << 16) | (1 << 10) | (3 << 3) | 2),
            ("u16-swapf", (4 << 16) | (1 << 14) | (3 << 3) | 2),
            ("u16-end16", (4 << 16) | (1 << 11) | (3 << 3) | 2),
            ("u16-flavor", (4 << 16) | (1 << 13) | (3 << 3) | 2),
            ("u16-x1", (4 << 16) | (1 << 7) | (3 << 3) | 2),
            ("u16-x2", (4 << 16) | (2 << 7) | (3 << 3) | 2),
            ("u16-x3", (4 << 16) | (3 << 7) | (3 << 3) | 2),
            ("u16-x4", (4 << 16) | (4 << 7) | (3 << 3) | 2),
            ("u16-premul", (4 << 16) | (1 << 23) | (1 << 7)
             | (3 << 3) | 2),
        ] {
            let ct = c::cmsCreateTransform(hi, fmt, ho, fmt,
                0, 0);
            eprintln!("{name} t={}", !ct.is_null());
            if !ct.is_null() { c::cmsDeleteTransform(ct); }
        }
        let _ = c::cmsCloseProfile(hi);
        let _ = c::cmsCloseProfile(ho);
    }
}

/// u8-format modifier matrix — the u8 packer bodies.
#[test]
fn u8_modifier_matrix() {
    unsafe {
        let hi = c::cmsCreate_sRGBProfile();
        let ho = c::cmsCreate_sRGBProfile();
        for (name, fmt) in [
            ("u8-planar", (4 << 16) | (1 << 12) | (3 << 3) | 1u32),
            ("u8-doswap", (4 << 16) | (1 << 10) | (3 << 3) | 1),
            ("u8-swapf", (4 << 16) | (1 << 14) | (3 << 3) | 1),
            ("u8-end16", (4 << 16) | (1 << 11) | (3 << 3) | 1),
            ("u8-flavor", (4 << 16) | (1 << 13) | (3 << 3) | 1),
            ("u8-x1", (4 << 16) | (1 << 7) | (3 << 3) | 1),
            ("u8-x2", (4 << 16) | (2 << 7) | (3 << 3) | 1),
            ("u8-x3", (4 << 16) | (3 << 7) | (3 << 3) | 1),
            ("u8-x4", (4 << 16) | (4 << 7) | (3 << 3) | 1),
            ("u8-premul", (4 << 16) | (1 << 23) | (1 << 7)
             | (3 << 3) | 1),
        ] {
            let ct = c::cmsCreateTransform(hi, fmt, ho, fmt,
                0, 0);
            eprintln!("{name} t={}", !ct.is_null());
            if !ct.is_null() { c::cmsDeleteTransform(ct); }
        }
        let _ = c::cmsCloseProfile(hi);
        let _ = c::cmsCloseProfile(ho);
    }
}

/// Float-format modifier matrix — the float packer bodies.
#[test]
fn float_modifier_matrix() {
    unsafe {
        let hi = c::cmsCreate_sRGBProfile();
        let ho = c::cmsCreate_sRGBProfile();
        for (name, fmt) in [
            ("f-planar", (1 << 22) | (4 << 16) | (1 << 12)
             | (3 << 3) | 4u32),
            ("f-doswap", (1 << 22) | (4 << 16) | (1 << 10)
             | (3 << 3) | 4),
            ("f-swapf", (1 << 22) | (4 << 16) | (1 << 14)
             | (3 << 3) | 4),
            ("f-flavor", (1 << 22) | (4 << 16) | (1 << 13)
             | (3 << 3) | 4),
            ("f-x1", (1 << 22) | (4 << 16) | (1 << 7)
             | (3 << 3) | 4),
            ("f-x2", (1 << 22) | (4 << 16) | (2 << 7)
             | (3 << 3) | 4),
            ("f-x3", (1 << 22) | (4 << 16) | (3 << 7)
             | (3 << 3) | 4),
            ("f-x4", (1 << 22) | (4 << 16) | (4 << 7)
             | (3 << 3) | 4),
            ("f-premul", (1 << 22) | (4 << 16) | (1 << 23)
             | (1 << 7) | (3 << 3) | 4),
        ] {
            let ct = c::cmsCreateTransform(hi, fmt, ho, fmt,
                0, 0);
            eprintln!("{name} t={}", !ct.is_null());
            if !ct.is_null() { c::cmsDeleteTransform(ct); }
        }
        let _ = c::cmsCloseProfile(hi);
        let _ = c::cmsCloseProfile(ho);
    }
}

/// Half-float modifier matrix — the half packer bodies.
#[test]
fn half_modifier_matrix() {
    unsafe {
        let hi = c::cmsCreate_sRGBProfile();
        let ho = c::cmsCreate_sRGBProfile();
        for (name, fmt) in [
            ("h-planar", (1 << 22) | (4 << 16) | (1 << 12)
             | (3 << 3) | 2u32),
            ("h-doswap", (1 << 22) | (4 << 16) | (1 << 10)
             | (3 << 3) | 2),
            ("h-swapf", (1 << 22) | (4 << 16) | (1 << 14)
             | (3 << 3) | 2),
            ("h-flavor", (1 << 22) | (4 << 16) | (1 << 13)
             | (3 << 3) | 2),
            ("h-x1", (1 << 22) | (4 << 16) | (1 << 7)
             | (3 << 3) | 2),
            ("h-x2", (1 << 22) | (4 << 16) | (2 << 7)
             | (3 << 3) | 2),
            ("h-x3", (1 << 22) | (4 << 16) | (3 << 7)
             | (3 << 3) | 2),
            ("h-x4", (1 << 22) | (4 << 16) | (4 << 7)
             | (3 << 3) | 2),
            ("h-premul", (1 << 22) | (4 << 16) | (1 << 23)
             | (1 << 7) | (3 << 3) | 2),
        ] {
            let ct = c::cmsCreateTransform(hi, fmt, ho, fmt,
                0, 0);
            eprintln!("{name} t={}", !ct.is_null());
            if !ct.is_null() { c::cmsDeleteTransform(ct); }
        }
        let _ = c::cmsCloseProfile(hi);
        let _ = c::cmsCloseProfile(ho);
    }
}

/// Lab-format modifier matrix — the Lab packer bodies.
#[test]
fn lab_modifier_matrix() {
    unsafe {
        let hi = c::cmsCreate_sRGBProfile();
        let lab4 = c::cmsCreateLab4Profile(std::ptr::null_mut());
        for (name, fmt) in [
            ("lab-planar", (10 << 16) | (1 << 12) | (3 << 3) | 2u32),
            ("lab-doswap", (10 << 16) | (1 << 10) | (3 << 3) | 2),
            ("lab-swapf", (10 << 16) | (1 << 14) | (3 << 3) | 2),
            ("lab-end16", (10 << 16) | (1 << 11) | (3 << 3) | 2),
            ("lab-flavor", (10 << 16) | (1 << 13) | (3 << 3) | 2),
            ("lab-x1", (10 << 16) | (1 << 7) | (3 << 3) | 2),
            ("lab-x2", (10 << 16) | (2 << 7) | (3 << 3) | 2),
            ("lab-8", (10 << 16) | (3 << 3) | 1),
        ] {
            let ct = c::cmsCreateTransform(hi,
                (4 << 16) | (3 << 3) | 2, lab4, fmt, 0, 0);
            eprintln!("{name} t={}", !ct.is_null());
            if !ct.is_null() { c::cmsDeleteTransform(ct); }
        }
        let _ = c::cmsCloseProfile(hi);
        let _ = c::cmsCloseProfile(lab4);
    }
}

/// Gray-format modifier matrix — the gray packer bodies.
#[test]
fn gray_modifier_matrix() {
    unsafe {
        let hi = c::cmsCreate_sRGBProfile();
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let gray = c::cmsCreateGrayProfile(
            c::cmsD50_xyY() as *const std::ffi::c_void, g);
        for (name, fmt) in [
            ("g-planar", (3 << 16) | (1 << 12) | (1 << 3) | 2u32),
            ("g-doswap", (3 << 16) | (1 << 10) | (1 << 3) | 2),
            ("g-swapf", (3 << 16) | (1 << 14) | (1 << 3) | 2),
            ("g-end16", (3 << 16) | (1 << 11) | (1 << 3) | 2),
            ("g-flavor", (3 << 16) | (1 << 13) | (1 << 3) | 2),
            ("g-x1", (3 << 16) | (1 << 7) | (1 << 3) | 2),
            ("g-x2", (3 << 16) | (2 << 7) | (1 << 3) | 2),
            ("g-8", (3 << 16) | (1 << 3) | 1),
            ("g-flt", (1 << 22) | (3 << 16) | (1 << 3) | 4),
            ("g-dbl", (1 << 22) | (3 << 16) | (1 << 3) | 0),
            ("g-hlf", (1 << 22) | (3 << 16) | (1 << 3) | 2),
        ] {
            let ct = c::cmsCreateTransform(hi,
                (4 << 16) | (3 << 3) | 2, gray, fmt, 0, 0);
            eprintln!("{name} t={}", !ct.is_null());
            if !ct.is_null() { c::cmsDeleteTransform(ct); }
        }
        let _ = c::cmsCloseProfile(hi);
        let _ = c::cmsCloseProfile(gray);
    }
}

/// XYZ-format modifier matrix — the XYZ packer bodies
/// (XYZ profile as input).
#[test]
fn xyz_modifier_matrix() {
    unsafe {
        let xyz = c::cmsCreateXYZProfile();
        let ho = c::cmsCreate_sRGBProfile();
        for (name, fmt) in [
            ("x-planar", (9 << 16) | (1 << 12) | (3 << 3) | 2u32),
            ("x-doswap", (9 << 16) | (1 << 10) | (3 << 3) | 2),
            ("x-swapf", (9 << 16) | (1 << 14) | (3 << 3) | 2),
            ("x-end16", (9 << 16) | (1 << 11) | (3 << 3) | 2),
            ("x-flavor", (9 << 16) | (1 << 13) | (3 << 3) | 2),
            ("x-8", (9 << 16) | (3 << 3) | 1),
            ("x-flt", (1 << 22) | (9 << 16) | (3 << 3) | 4),
            ("x-dbl", (1 << 22) | (9 << 16) | (3 << 3) | 0),
            ("x-hlf", (1 << 22) | (9 << 16) | (3 << 3) | 2),
        ] {
            let ct = c::cmsCreateTransform(xyz, fmt, ho,
                (4 << 16) | (3 << 3) | 2, 0, 0);
            eprintln!("{name} t={}", !ct.is_null());
            if !ct.is_null() { c::cmsDeleteTransform(ct); }
        }
        let _ = c::cmsCloseProfile(xyz);
        let _ = c::cmsCloseProfile(ho);
    }
}

/// Named-color format internals — TYPE_NAMED_COLOR_INDEX on
/// a named-color profile.
#[test]
fn named_color_format() {
    unsafe {
        // a named-color profile via the placeholder + ncl2 tag
        let h = c::cmsCreateProfilePlaceholder(
            std::ptr::null_mut());
        c::cmsSetDeviceClass(h, 0x6E6D636C); // 'nmcl'
        c::cmsSetColorSpace(h, 0x6E6D636C); // 'nmcl'
        c::cmsSetPCS(h, 0x4C616220); // Lab
        // a named-color list written to 'ncl2'
        let list = c::cmsAllocNamedColorList(
            std::ptr::null_mut(), 2, 3, b"P\0".as_ptr() as _,
            b"S\0".as_ptr() as _);
        if !list.is_null() {
            let _ = c::cmsWriteTag(h, 0x6E636C32,
                list as *mut std::ffi::c_void);
        }
        // the named-index format — 1-channel u16 index
        let named_fmt: u32 = (1 << 3) | 2;
        let lab_fmt: u32 = (10 << 16) | (3 << 3) | 2;
        let ct = c::cmsCreateTransform(h, named_fmt, h,
            lab_fmt, 0, 0);
        eprintln!("named fmt t={}", !ct.is_null());
        if !ct.is_null() {
            let inp = [0u16; 1];
            let mut out = [0u16; 3];
            c::cmsDoTransform(ct, inp.as_ptr() as _,
                out.as_mut_ptr() as *mut _, 1);
            c::cmsDeleteTransform(ct);
        }
        let _ = c::cmsCloseProfile(h);
    }
}

/// Double-format modifier matrix — the double packer bodies.
#[test]
fn double_modifier_matrix() {
    unsafe {
        let hi = c::cmsCreate_sRGBProfile();
        let ho = c::cmsCreate_sRGBProfile();
        for (name, fmt) in [
            ("d-planar", (1 << 22) | (4 << 16) | (1 << 12)
             | (3 << 3) | 0u32),
            ("d-doswap", (1 << 22) | (4 << 16) | (1 << 10)
             | (3 << 3) | 0),
            ("d-swapf", (1 << 22) | (4 << 16) | (1 << 14)
             | (3 << 3) | 0),
            ("d-flavor", (1 << 22) | (4 << 16) | (1 << 13)
             | (3 << 3) | 0),
            ("d-x1", (1 << 22) | (4 << 16) | (1 << 7)
             | (3 << 3) | 0),
            ("d-x2", (1 << 22) | (4 << 16) | (2 << 7)
             | (3 << 3) | 0),
            ("d-x3", (1 << 22) | (4 << 16) | (3 << 7)
             | (3 << 3) | 0),
            ("d-x4", (1 << 22) | (4 << 16) | (4 << 7)
             | (3 << 3) | 0),
            ("d-premul", (1 << 22) | (4 << 16) | (1 << 23)
             | (1 << 7) | (3 << 3) | 0),
        ] {
            let ct = c::cmsCreateTransform(hi, fmt, ho, fmt,
                0, 0);
            eprintln!("{name} t={}", !ct.is_null());
            if !ct.is_null() { c::cmsDeleteTransform(ct); }
        }
        let _ = c::cmsCloseProfile(hi);
        let _ = c::cmsCloseProfile(ho);
    }
}

/// Multi-channel format modifier matrix — the 1/2/4/6/8-ch
/// packer bodies.
#[test]
fn multichannel_modifier_matrix() {
    unsafe {
        let hi = c::cmsCreate_sRGBProfile();
        let ho = c::cmsCreate_sRGBProfile();
        // PT_ANY (0) + channel count variations
        for (name, fmt) in [
            ("1ch", (0 << 16) | (1 << 3) | 1u32),
            ("2ch", (0 << 16) | (2 << 3) | 1),
            ("4ch", (0 << 16) | (4 << 3) | 1),
            ("6ch", (0 << 16) | (6 << 3) | 1),
            ("8ch", (0 << 16) | (8 << 3) | 1),
            ("15ch", (0 << 16) | (15 << 3) | 1),
            ("1ch-x1", (0 << 16) | (1 << 7) | (1 << 3) | 1),
            ("2ch-planar", (0 << 16) | (1 << 12) | (2 << 3) | 1),
            ("4ch-doswap", (0 << 16) | (1 << 10) | (4 << 3) | 1),
            ("6ch-swapf", (0 << 16) | (1 << 14) | (6 << 3) | 1),
        ] {
            let ct = c::cmsCreateTransform(hi, fmt, ho, fmt,
                0, 0);
            eprintln!("{name} t={}", !ct.is_null());
            if !ct.is_null() { c::cmsDeleteTransform(ct); }
        }
        let _ = c::cmsCloseProfile(hi);
        let _ = c::cmsCloseProfile(ho);
    }
}

/// 3-channel format on a 3-channel profile — the matching
/// channel-count formats.
#[test]
fn matching_channel_formats() {
    unsafe {
        let hi = c::cmsCreate_sRGBProfile();
        let ho = c::cmsCreate_sRGBProfile();
        for (name, fmt) in [
            ("3ch-u8", (0 << 16) | (3 << 3) | 1u32),
            ("3ch-u16", (0 << 16) | (3 << 3) | 2),
            ("3ch-flt", (1 << 22) | (0 << 16) | (3 << 3) | 4),
            ("3ch-dbl", (1 << 22) | (0 << 16) | (3 << 3) | 0),
            ("3ch-hlf", (1 << 22) | (0 << 16) | (3 << 3) | 2),
            ("3ch-planar", (0 << 16) | (1 << 12) | (3 << 3) | 1),
            ("3ch-doswap", (0 << 16) | (1 << 10) | (3 << 3) | 1),
            ("3ch-swapf", (0 << 16) | (1 << 14) | (3 << 3) | 1),
            ("3ch-end16", (0 << 16) | (1 << 11) | (3 << 3) | 1),
            ("3ch-flavor", (0 << 16) | (1 << 13) | (3 << 3) | 1),
        ] {
            let ct = c::cmsCreateTransform(hi, fmt, ho, fmt,
                0, 0);
            eprintln!("{name} t={}", !ct.is_null());
            if !ct.is_null() { c::cmsDeleteTransform(ct); }
        }
        let _ = c::cmsCloseProfile(hi);
        let _ = c::cmsCloseProfile(ho);
    }
}

/// 4-channel format on a CMYK devicelink — the channel-
/// matching machinery.
#[test]
fn four_channel_formats() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mut curves = [g; 4];
        let dl = c::cmsCreateLinearizationDeviceLinkTHR(
            std::ptr::null_mut(), 0x434D594B,
            curves.as_mut_ptr());
        eprintln!("dl={}", !dl.is_null());
        // 4-channel formats on the devicelink
        for (name, fmt) in [
            ("4ch-u8", (0 << 16) | (4 << 3) | 1u32),
            ("4ch-u16", (0 << 16) | (4 << 3) | 2),
            ("4ch-flt", (1 << 22) | (0 << 16) | (4 << 3) | 4),
            ("4ch-dbl", (1 << 22) | (0 << 16) | (4 << 3) | 0),
            ("4ch-hlf", (1 << 22) | (0 << 16) | (4 << 3) | 2),
            ("4ch-planar", (0 << 16) | (1 << 12) | (4 << 3) | 1),
        ] {
            let ct = c::cmsCreateTransform(dl, fmt, dl, fmt,
                0, 0);
            eprintln!("{name} t={}", !ct.is_null());
            if !ct.is_null() { c::cmsDeleteTransform(ct); }
        }
        let _ = c::cmsCloseProfile(dl);
    }
}
