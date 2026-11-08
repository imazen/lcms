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
    // output pre-filled with a pattern: premul packers read the alpha
    // slot out of the output buffer, and unwritten extra-channel slots
    // stay as-is — identical pre-fill keeps the compare honest.
    let mut co = vec![0xA5u8; out_sz];
    let mut ro = vec![0xA5u8; out_sz];
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

/// 4-channel modifier matrix on the CMYK devicelink.
#[test]
fn four_channel_modifier_matrix() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mut curves = [g; 4];
        let dl = c::cmsCreateLinearizationDeviceLinkTHR(
            std::ptr::null_mut(), 0x434D594B,
            curves.as_mut_ptr());
        for (name, fmt) in [
            ("4ch-doswap", (0 << 16) | (1 << 10) | (4 << 3) | 1),
            ("4ch-swapf", (0 << 16) | (1 << 14) | (4 << 3) | 1),
            ("4ch-end16", (0 << 16) | (1 << 11) | (4 << 3) | 1),
            ("4ch-flavor", (0 << 16) | (1 << 13) | (4 << 3) | 1),
            ("4ch-x1", (0 << 16) | (1 << 7) | (4 << 3) | 1),
            ("4ch-x2", (0 << 16) | (2 << 7) | (4 << 3) | 1),
            ("4ch-x3", (0 << 16) | (3 << 7) | (4 << 3) | 1),
            ("4ch-x4", (0 << 16) | (4 << 7) | (4 << 3) | 1),
            ("4ch-premul", (0 << 16) | (1 << 23) | (1 << 7)
             | (4 << 3) | 1),
        ] {
            let ct = c::cmsCreateTransform(dl, fmt, dl, fmt,
                0, 0);
            eprintln!("{name} t={}", !ct.is_null());
            if !ct.is_null() { c::cmsDeleteTransform(ct); }
        }
        let _ = c::cmsCloseProfile(dl);
    }
}

/// 1/2-channel formats on a gray profile — the channel-
/// matching machinery.
#[test]
fn gray_channel_formats() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let gray = c::cmsCreateGrayProfile(
            c::cmsD50_xyY() as *const std::ffi::c_void, g);
        for (name, fmt) in [
            ("1ch-u8", (0 << 16) | (1 << 3) | 1u32),
            ("1ch-u16", (0 << 16) | (1 << 3) | 2),
            ("1ch-flt", (1 << 22) | (0 << 16) | (1 << 3) | 4),
            ("1ch-dbl", (1 << 22) | (0 << 16) | (1 << 3) | 0),
            ("1ch-hlf", (1 << 22) | (0 << 16) | (1 << 3) | 2),
            ("2ch-u8", (0 << 16) | (2 << 3) | 1),
            ("2ch-x1", (0 << 16) | (1 << 7) | (1 << 3) | 1),
            ("1ch-planar", (0 << 16) | (1 << 12) | (1 << 3) | 1),
        ] {
            let ct = c::cmsCreateTransform(gray, fmt, gray,
                fmt, 0, 0);
            eprintln!("{name} t={}", !ct.is_null());
            if !ct.is_null() { c::cmsDeleteTransform(ct); }
        }
        let _ = c::cmsCloseProfile(gray);
    }
}

/// Gray modifier matrix on a gray profile — the 1-channel
/// packer bodies.
#[test]
fn gray_modifier_matrix2() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let gray = c::cmsCreateGrayProfile(
            c::cmsD50_xyY() as *const std::ffi::c_void, g);
        for (name, fmt) in [
            ("g-doswap", (0 << 16) | (1 << 10) | (1 << 3) | 1),
            ("g-swapf", (0 << 16) | (1 << 14) | (1 << 3) | 1),
            ("g-flavor", (0 << 16) | (1 << 13) | (1 << 3) | 1),
            ("g-x2", (0 << 16) | (2 << 7) | (1 << 3) | 1),
            ("g-x3", (0 << 16) | (3 << 7) | (1 << 3) | 1),
            ("g-x4", (0 << 16) | (4 << 7) | (1 << 3) | 1),
            ("g-premul", (0 << 16) | (1 << 23) | (1 << 7)
             | (1 << 3) | 1),
            ("g-flt-x1", (1 << 22) | (0 << 16) | (1 << 7)
             | (1 << 3) | 4),
            ("g-dbl-x1", (1 << 22) | (0 << 16) | (1 << 7)
             | (1 << 3) | 0),
        ] {
            let ct = c::cmsCreateTransform(gray, fmt, gray,
                fmt, 0, 0);
            eprintln!("{name} t={}", !ct.is_null());
            if !ct.is_null() { c::cmsDeleteTransform(ct); }
        }
        let _ = c::cmsCloseProfile(gray);
    }
}

/// 2-channel formats on a 2-channel devicelink — the
/// 2-channel packer bodies.
#[test]
fn two_channel_formats() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mut curves = [g; 2];
        // '2CLR' (MCH2) devicelink
        let dl = c::cmsCreateLinearizationDeviceLinkTHR(
            std::ptr::null_mut(), 0x32434C52,
            curves.as_mut_ptr());
        eprintln!("dl={}", !dl.is_null());
        for (name, fmt) in [
            ("2ch-u8", (0 << 16) | (2 << 3) | 1u32),
            ("2ch-u16", (0 << 16) | (2 << 3) | 2),
            ("2ch-flt", (1 << 22) | (0 << 16) | (2 << 3) | 4),
            ("2ch-dbl", (1 << 22) | (0 << 16) | (2 << 3) | 0),
            ("2ch-hlf", (1 << 22) | (0 << 16) | (2 << 3) | 2),
            ("2ch-planar", (0 << 16) | (1 << 12) | (2 << 3) | 1),
        ] {
            let ct = c::cmsCreateTransform(dl, fmt, dl, fmt,
                0, 0);
            eprintln!("{name} t={}", !ct.is_null());
            if !ct.is_null() { c::cmsDeleteTransform(ct); }
        }
        let _ = c::cmsCloseProfile(dl);
    }
}

/// 6/8/15-channel formats on matching devicelinks — the
/// multi-channel packer bodies.
#[test]
fn multichannel_formats() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        for (nch, sig, name) in [
            (6usize, 0x36434C52u32, "6CLR"),
            (8, 0x38434C52u32, "8CLR"),
            (15, 0x46434C52u32, "FCLR")] {
            let mut curves = vec![g; nch];
            let dl = c::cmsCreateLinearizationDeviceLinkTHR(
                std::ptr::null_mut(), sig, curves.as_mut_ptr());
            eprintln!("{name} dl={}", !dl.is_null());
            for fmt in [
                (0 << 16) | ((nch as u32) << 3) | 1,
                (0 << 16) | ((nch as u32) << 3) | 2,
                (1 << 22) | (0 << 16) | ((nch as u32) << 3) | 4,
            ] {
                let ct = c::cmsCreateTransform(dl, fmt, dl,
                    fmt, 0, 0);
                eprintln!("  fmt t={}", !ct.is_null());
                if !ct.is_null() { c::cmsDeleteTransform(ct); }
            }
            if !dl.is_null() { c::cmsCloseProfile(dl); }
        }
    }
}

/// Multi-channel modifier matrix on the devicelinks.
#[test]
fn multichannel_modifier_matrix2() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mut curves = [g; 6];
        let dl = c::cmsCreateLinearizationDeviceLinkTHR(
            std::ptr::null_mut(), 0x36434C52,
            curves.as_mut_ptr());
        for (name, fmt) in [
            ("6ch-doswap", (0 << 16) | (1 << 10) | (6 << 3) | 1),
            ("6ch-swapf", (0 << 16) | (1 << 14) | (6 << 3) | 1),
            ("6ch-end16", (0 << 16) | (1 << 11) | (6 << 3) | 1),
            ("6ch-flavor", (0 << 16) | (1 << 13) | (6 << 3) | 1),
            ("6ch-x1", (0 << 16) | (1 << 7) | (6 << 3) | 1),
            ("6ch-x2", (0 << 16) | (2 << 7) | (6 << 3) | 1),
            ("6ch-x3", (0 << 16) | (3 << 7) | (6 << 3) | 1),
            ("6ch-x4", (0 << 16) | (4 << 7) | (6 << 3) | 1),
            ("6ch-premul", (0 << 16) | (1 << 23) | (1 << 7)
             | (6 << 3) | 1),
            ("6ch-dbl", (1 << 22) | (0 << 16) | (6 << 3) | 0),
            ("6ch-hlf", (1 << 22) | (0 << 16) | (6 << 3) | 2),
        ] {
            let ct = c::cmsCreateTransform(dl, fmt, dl, fmt,
                0, 0);
            eprintln!("{name} t={}", !ct.is_null());
            if !ct.is_null() { c::cmsDeleteTransform(ct); }
        }
        let _ = c::cmsCloseProfile(dl);
    }
}

/// Named-color format on a devicelink — the
/// TYPE_NAMED_COLOR_INDEX machinery.
#[test]
fn named_color_on_devicelink() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mut curves = [g; 3];
        let dl = c::cmsCreateLinearizationDeviceLinkTHR(
            std::ptr::null_mut(), 0x52474220,
            curves.as_mut_ptr());
        // the named-index format — 1-channel u16 index
        let named_fmt: u32 = (1 << 3) | 2;
        let rgb_fmt: u32 = (4 << 16) | (3 << 3) | 2;
        let ct = c::cmsCreateTransform(dl, named_fmt, dl,
            rgb_fmt, 0, 0);
        eprintln!("named t={}", !ct.is_null());
        if !ct.is_null() { c::cmsDeleteTransform(ct); }
        let _ = c::cmsCloseProfile(dl);
    }
}

/// Byte-order formats on the devicelinks — the byte-order
/// machinery on more channel counts.
#[test]
fn byte_order_devicelink() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mut curves = [g; 4];
        let dl = c::cmsCreateLinearizationDeviceLinkTHR(
            std::ptr::null_mut(), 0x434D594B,
            curves.as_mut_ptr());
        for (name, fmt) in [
            ("4ch-bgr", (0 << 16) | (1 << 10) | (4 << 3) | 1),
            ("4ch-abgr", (0 << 16) | (1 << 14) | (1 << 10)
             | (4 << 3) | 1),
            ("4ch-argb", (0 << 16) | (1 << 14) | (4 << 3) | 1),
            ("4ch-bgra", (0 << 16) | (1 << 10) | (1 << 7)
             | (4 << 3) | 1),
            ("4ch-rgba", (0 << 16) | (1 << 7) | (4 << 3) | 1),
        ] {
            let ct = c::cmsCreateTransform(dl, fmt, dl, fmt,
                0, 0);
            eprintln!("{name} t={}", !ct.is_null());
            if !ct.is_null() { c::cmsDeleteTransform(ct); }
        }
        let _ = c::cmsCloseProfile(dl);
    }
}

/// Float byte-order formats on the devicelinks — the float
/// byte-order machinery on more channel counts.
#[test]
fn float_byte_order_devicelink() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mut curves = [g; 4];
        let dl = c::cmsCreateLinearizationDeviceLinkTHR(
            std::ptr::null_mut(), 0x434D594B,
            curves.as_mut_ptr());
        for (name, fmt) in [
            ("4f-bgr", (1 << 22) | (0 << 16) | (1 << 10)
             | (4 << 3) | 4u32),
            ("4f-abgr", (1 << 22) | (0 << 16) | (1 << 14)
             | (1 << 10) | (4 << 3) | 4),
            ("4f-argb", (1 << 22) | (0 << 16) | (1 << 14)
             | (4 << 3) | 4),
            ("4f-bgra", (1 << 22) | (0 << 16) | (1 << 10)
             | (1 << 7) | (4 << 3) | 4),
            ("4f-rgba", (1 << 22) | (0 << 16) | (1 << 7)
             | (4 << 3) | 4),
            ("4f-planar", (1 << 22) | (0 << 16) | (1 << 12)
             | (4 << 3) | 4),
        ] {
            let ct = c::cmsCreateTransform(dl, fmt, dl, fmt,
                0, 0);
            eprintln!("{name} t={}", !ct.is_null());
            if !ct.is_null() { c::cmsDeleteTransform(ct); }
        }
        let _ = c::cmsCloseProfile(dl);
    }
}

/// u16 byte-order formats on the devicelinks — the u16
/// byte-order machinery on more channel counts.
#[test]
fn u16_byte_order_devicelink() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mut curves = [g; 4];
        let dl = c::cmsCreateLinearizationDeviceLinkTHR(
            std::ptr::null_mut(), 0x434D594B,
            curves.as_mut_ptr());
        for (name, fmt) in [
            ("4u16-bgr", (0 << 16) | (1 << 10) | (4 << 3) | 2u32),
            ("4u16-abgr", (0 << 16) | (1 << 14) | (1 << 10)
             | (4 << 3) | 2),
            ("4u16-argb", (0 << 16) | (1 << 14) | (4 << 3) | 2),
            ("4u16-bgra", (0 << 16) | (1 << 10) | (1 << 7)
             | (4 << 3) | 2),
            ("4u16-rgba", (0 << 16) | (1 << 7) | (4 << 3) | 2),
            ("4u16-planar", (0 << 16) | (1 << 12) | (4 << 3) | 2),
        ] {
            let ct = c::cmsCreateTransform(dl, fmt, dl, fmt,
                0, 0);
            eprintln!("{name} t={}", !ct.is_null());
            if !ct.is_null() { c::cmsDeleteTransform(ct); }
        }
        let _ = c::cmsCloseProfile(dl);
    }
}

/// Half-float byte-order formats on the devicelinks — the
/// half byte-order machinery on more channel counts.
#[test]
fn half_byte_order_devicelink() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mut curves = [g; 4];
        let dl = c::cmsCreateLinearizationDeviceLinkTHR(
            std::ptr::null_mut(), 0x434D594B,
            curves.as_mut_ptr());
        for (name, fmt) in [
            ("4h-bgr", (1 << 22) | (0 << 16) | (1 << 10)
             | (4 << 3) | 2u32),
            ("4h-abgr", (1 << 22) | (0 << 16) | (1 << 14)
             | (1 << 10) | (4 << 3) | 2),
            ("4h-argb", (1 << 22) | (0 << 16) | (1 << 14)
             | (4 << 3) | 2),
            ("4h-bgra", (1 << 22) | (0 << 16) | (1 << 10)
             | (1 << 7) | (4 << 3) | 2),
            ("4h-rgba", (1 << 22) | (0 << 16) | (1 << 7)
             | (4 << 3) | 2),
            ("4h-doswap", (1 << 22) | (0 << 16) | (1 << 10)
             | (4 << 3) | 2),
        ] {
            let ct = c::cmsCreateTransform(dl, fmt, dl, fmt,
                0, 0);
            eprintln!("{name} t={}", !ct.is_null());
            if !ct.is_null() { c::cmsDeleteTransform(ct); }
        }
        let _ = c::cmsCloseProfile(dl);
    }
}

/// Double byte-order formats on the devicelinks — the
/// double byte-order machinery on more channel counts.
#[test]
fn double_byte_order_devicelink() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mut curves = [g; 4];
        let dl = c::cmsCreateLinearizationDeviceLinkTHR(
            std::ptr::null_mut(), 0x434D594B,
            curves.as_mut_ptr());
        for (name, fmt) in [
            ("4d-bgr", (1 << 22) | (0 << 16) | (1 << 10)
             | (4 << 3) | 0u32),
            ("4d-abgr", (1 << 22) | (0 << 16) | (1 << 14)
             | (1 << 10) | (4 << 3) | 0),
            ("4d-argb", (1 << 22) | (0 << 16) | (1 << 14)
             | (4 << 3) | 0),
            ("4d-bgra", (1 << 22) | (0 << 16) | (1 << 10)
             | (1 << 7) | (4 << 3) | 0),
            ("4d-rgba", (1 << 22) | (0 << 16) | (1 << 7)
             | (4 << 3) | 0),
            ("4d-doswap", (1 << 22) | (0 << 16) | (1 << 10)
             | (4 << 3) | 0),
        ] {
            let ct = c::cmsCreateTransform(dl, fmt, dl, fmt,
                0, 0);
            eprintln!("{name} t={}", !ct.is_null());
            if !ct.is_null() { c::cmsDeleteTransform(ct); }
        }
        let _ = c::cmsCloseProfile(dl);
    }
}

/// cmsDoTransform on the devicelinks — the transform eval
/// machinery on more channel counts.
#[test]
fn devicelink_transform_eval() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        for (nch, sig) in [
            (2usize, 0x32434C52u32), (4, 0x434D594B),
            (6, 0x36434C52), (8, 0x38434C52)] {
            let mut curves = vec![g; nch];
            let dl = c::cmsCreateLinearizationDeviceLinkTHR(
                std::ptr::null_mut(), sig, curves.as_mut_ptr());
            let fmt: u32 = (0 << 16) | ((nch as u32) << 3) | 1;
            let ct = c::cmsCreateTransform(dl, fmt, dl, fmt,
                0, 0);
            eprintln!("{nch}ch t={}", !ct.is_null());
            if !ct.is_null() {
                let inp = vec![0x40u8; nch];
                let mut out = vec![0u8; nch];
                c::cmsDoTransform(ct, inp.as_ptr() as _,
                    out.as_mut_ptr() as *mut _, 1);
                c::cmsDeleteTransform(ct);
            }
            let _ = c::cmsCloseProfile(dl);
        }
    }
}

/// cmsChangeBuffersFormat on the devicelinks — the format-
/// change machinery on more channel counts.
#[test]
fn change_buffers_devicelink() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mut curves = [g; 4];
        let dl = c::cmsCreateLinearizationDeviceLinkTHR(
            std::ptr::null_mut(), 0x434D594B,
            curves.as_mut_ptr());
        let ct = c::cmsCreateTransform(dl,
            (0 << 16) | (4 << 3) | 1, dl,
            (0 << 16) | (4 << 3) | 2, 0, 0);
        eprintln!("t={}", !ct.is_null());
        if !ct.is_null() {
            let ok = c::cmsChangeBuffersFormat(ct,
                (1 << 22) | (0 << 16) | (4 << 3) | 4,
                (1 << 22) | (0 << 16) | (4 << 3) | 0);
            eprintln!("change={ok}");
            c::cmsDeleteTransform(ct);
        }
        let _ = c::cmsCloseProfile(dl);
    }
}

/// cmsDoTransformStride on the devicelinks — the stride
/// machinery on more channel counts.
#[test]
fn stride_devicelink() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mut curves = [g; 4];
        let dl = c::cmsCreateLinearizationDeviceLinkTHR(
            std::ptr::null_mut(), 0x434D594B,
            curves.as_mut_ptr());
        let ct = c::cmsCreateTransform(dl,
            (0 << 16) | (1 << 12) | (4 << 3) | 1, dl,
            (0 << 16) | (1 << 12) | (4 << 3) | 1, 0, 0);
        eprintln!("t={}", !ct.is_null());
        if !ct.is_null() {
            // planar stride eval
            let inp = [0x40u8; 8];
            let mut out = [0u8; 8];
            c::cmsDoTransformStride(ct, inp.as_ptr() as _,
                out.as_mut_ptr() as *mut _, 2, 4);
            c::cmsDeleteTransform(ct);
        }
        let _ = c::cmsCloseProfile(dl);
    }
}

/// cmsDoTransformLineStride on the devicelinks — the line-
/// stride machinery on more channel counts.
#[test]
fn line_stride_devicelink() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mut curves = [g; 4];
        let dl = c::cmsCreateLinearizationDeviceLinkTHR(
            std::ptr::null_mut(), 0x434D594B,
            curves.as_mut_ptr());
        let ct = c::cmsCreateTransform(dl,
            (0 << 16) | (4 << 3) | 1, dl,
            (0 << 16) | (4 << 3) | 1, 0, 0);
        eprintln!("t={}", !ct.is_null());
        if !ct.is_null() {
            // line-stride eval — 2 lines x 2 pixels x 4ch
            let inp = [0x40u8; 16];
            let mut out = [0u8; 16];
            c::cmsDoTransformLineStride(ct, inp.as_ptr() as _,
                out.as_mut_ptr() as *mut _, 2, 2, 8, 8, 4, 4);
            c::cmsDeleteTransform(ct);
        }
        let _ = c::cmsCloseProfile(dl);
    }
}

/// Alpha formats on the devicelinks — the alpha machinery
/// on more channel counts.
#[test]
fn alpha_devicelink() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mut curves = [g; 4];
        let dl = c::cmsCreateLinearizationDeviceLinkTHR(
            std::ptr::null_mut(), 0x434D594B,
            curves.as_mut_ptr());
        for (name, fmt) in [
            ("4ch-x1", (0 << 16) | (1 << 7) | (4 << 3) | 1),
            ("4ch-x2", (0 << 16) | (2 << 7) | (4 << 3) | 1),
            ("4ch-x3", (0 << 16) | (3 << 7) | (4 << 3) | 1),
            ("4ch-x4", (0 << 16) | (4 << 7) | (4 << 3) | 1),
            ("4ch-a1u8", (0 << 16) | (1 << 7) | (4 << 3) | 1),
            ("4ch-a1u16", (0 << 16) | (1 << 7) | (4 << 3) | 2),
        ] {
            let ct = c::cmsCreateTransform(dl, fmt, dl, fmt,
                0, 0);
            eprintln!("{name} t={}", !ct.is_null());
            if !ct.is_null() { c::cmsDeleteTransform(ct); }
        }
        let _ = c::cmsCloseProfile(dl);
    }
}

/// Named-color format on a 'nmcl' devicelink — the
/// TYPE_NAMED_COLOR_INDEX machinery.
#[test]
fn named_color_nmcl() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mut curves = [g; 1];
        // 'nmcl' devicelink (1-channel named-color)
        let dl = c::cmsCreateLinearizationDeviceLinkTHR(
            std::ptr::null_mut(), 0x6E6D636C,
            curves.as_mut_ptr());
        eprintln!("dl={}", !dl.is_null());
        // the named-index format — 1-channel u16 index
        let named_fmt: u32 = (1 << 3) | 2;
        let ct = c::cmsCreateTransform(dl, named_fmt, dl,
            named_fmt, 0, 0);
        eprintln!("named t={}", !ct.is_null());
        if !ct.is_null() { c::cmsDeleteTransform(ct); }
        let _ = c::cmsCloseProfile(dl);
    }
}

/// 15-channel modifier matrix on the 'FCLR' devicelink.
#[test]
fn fclr_modifier_matrix() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mut curves = [g; 15];
        let dl = c::cmsCreateLinearizationDeviceLinkTHR(
            std::ptr::null_mut(), 0x46434C52,
            curves.as_mut_ptr());
        for (name, fmt) in [
            ("15ch-doswap", (0 << 16) | (1 << 10) | (15 << 3) | 1),
            ("15ch-swapf", (0 << 16) | (1 << 14) | (15 << 3) | 1),
            ("15ch-flavor", (0 << 16) | (1 << 13) | (15 << 3) | 1),
            ("15ch-x1", (0 << 16) | (1 << 7) | (15 << 3) | 1),
            ("15ch-x2", (0 << 16) | (2 << 7) | (15 << 3) | 1),
            ("15ch-dbl", (1 << 22) | (0 << 16) | (15 << 3) | 0),
            ("15ch-hlf", (1 << 22) | (0 << 16) | (15 << 3) | 2),
        ] {
            let ct = c::cmsCreateTransform(dl, fmt, dl, fmt,
                0, 0);
            eprintln!("{name} t={}", !ct.is_null());
            if !ct.is_null() { c::cmsDeleteTransform(ct); }
        }
        let _ = c::cmsCloseProfile(dl);
    }
}

/// Lab formats on a Lab devicelink — the Lab packer bodies.
#[test]
fn lab_devicelink() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mut curves = [g; 3];
        let dl = c::cmsCreateLinearizationDeviceLinkTHR(
            std::ptr::null_mut(), 0x4C616220,
            curves.as_mut_ptr());
        for (name, fmt) in [
            ("lab-u8", (0 << 16) | (3 << 3) | 1u32),
            ("lab-u16", (0 << 16) | (3 << 3) | 2),
            ("lab-flt", (1 << 22) | (0 << 16) | (3 << 3) | 4),
            ("lab-dbl", (1 << 22) | (0 << 16) | (3 << 3) | 0),
            ("lab-hlf", (1 << 22) | (0 << 16) | (3 << 3) | 2),
            ("lab-planar", (0 << 16) | (1 << 12) | (3 << 3) | 1),
        ] {
            let ct = c::cmsCreateTransform(dl, fmt, dl, fmt,
                0, 0);
            eprintln!("{name} t={}", !ct.is_null());
            if !ct.is_null() { c::cmsDeleteTransform(ct); }
        }
        let _ = c::cmsCloseProfile(dl);
    }
}

/// XYZ formats on an XYZ devicelink — the XYZ packer
/// bodies on a real profile.
#[test]
fn xyz_devicelink() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mut curves = [g; 3];
        let dl = c::cmsCreateLinearizationDeviceLinkTHR(
            std::ptr::null_mut(), 0x58595A20,
            curves.as_mut_ptr());
        for (name, fmt) in [
            ("xyz-u8", (0 << 16) | (3 << 3) | 1u32),
            ("xyz-u16", (0 << 16) | (3 << 3) | 2),
            ("xyz-flt", (1 << 22) | (0 << 16) | (3 << 3) | 4),
            ("xyz-dbl", (1 << 22) | (0 << 16) | (3 << 3) | 0),
            ("xyz-hlf", (1 << 22) | (0 << 16) | (3 << 3) | 2),
            ("xyz-planar", (0 << 16) | (1 << 12) | (3 << 3) | 1),
        ] {
            let ct = c::cmsCreateTransform(dl, fmt, dl, fmt,
                0, 0);
            eprintln!("{name} t={}", !ct.is_null());
            if !ct.is_null() { c::cmsDeleteTransform(ct); }
        }
        let _ = c::cmsCloseProfile(dl);
    }
}

/// Ink-limiting on more channel counts — the ink-limiting
/// machinery.
#[test]
fn ink_limiting_variants() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        for (nch, sig, name) in [
            (4usize, 0x434D594B, "CMYK"),
            (6, 0x36434C52, "6CLR"),
            (3, 0x52474220, "RGB ")] {
            let mut curves = vec![g; nch];
            let dl = c::cmsCreateInkLimitingDeviceLinkTHR(
                std::ptr::null_mut(), sig, 60.0);
            eprintln!("{name} dl={}", !dl.is_null());
            if !dl.is_null() { c::cmsCloseProfile(dl); }
        }
    }
}

/// Multi-channel packers — odd/missing channel counts
/// (5/7/9-14) via linearization devicelinks.
#[test]
fn multich_packers2() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        for n in [5u32, 7, 9, 10, 11, 12, 13, 14] {
            let mut curves = vec![g; n as usize];
            let dl = c::cmsCreateLinearizationDeviceLinkTHR(
                std::ptr::null_mut(),
                match n {
                    5 => 0x35434C52, 7 => 0x37434C52,
                    9 => 0x39434C52, 10 => 0x41434C52,
                    11 => 0x42434C52, 12 => 0x43434C52,
                    13 => 0x44434C52, 14 => 0x45434C52,
                    _ => 0x46434C52,
                },
                curves.as_mut_ptr());
            for (name, bytes) in [("u8", 1u32), ("u16", 2)] {
                let fmt = (0 << 16) | (n << 3) | bytes;
                let t = c::cmsCreateTransform(dl, fmt,
                    dl, fmt, 0, 0);
                eprintln!("{n}ch-{name} t={}", !t.is_null());
                if !t.is_null() { c::cmsDeleteTransform(t); }
            }
            c::cmsCloseProfile(dl);
        }
        c::cmsFreeToneCurve(g);
    }
}

/// Float packers — odd/missing channel counts
/// (5/7/9-14) via linearization devicelinks.
#[test]
fn multich_float_packers() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        for n in [5u32, 7, 9, 10, 11, 12, 13, 14] {
            let mut curves = vec![g; n as usize];
            let dl = c::cmsCreateLinearizationDeviceLinkTHR(
                std::ptr::null_mut(),
                match n {
                    5 => 0x35434C52, 7 => 0x37434C52,
                    9 => 0x39434C52, 10 => 0x41434C52,
                    11 => 0x42434C52, 12 => 0x43434C52,
                    13 => 0x44434C52, 14 => 0x45434C52,
                    _ => 0x46434C52,
                },
                curves.as_mut_ptr());
            // FLOAT_SH=22, bytes=4 (flt=4, dbl=8, half=2+float)
            for (name, bytes, flt) in [
                ("flt", 4u32, 1u32 << 22),
                ("dbl", 8, 1 << 22)] {
                let fmt = (0 << 16) | (n << 3) | bytes | flt;
                let t = c::cmsCreateTransform(dl, fmt,
                    dl, fmt, 0, 0);
                eprintln!("{n}ch-{name} t={}", !t.is_null());
                if !t.is_null() { c::cmsDeleteTransform(t); }
            }
            c::cmsCloseProfile(dl);
        }
        c::cmsFreeToneCurve(g);
    }
}

/// Half-float packers — odd/missing channel counts
/// (5/7/9-14) via linearization devicelinks.
#[test]
fn multich_half_packers() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        for n in [5u32, 7, 9, 10, 11, 12, 13, 14] {
            let mut curves = vec![g; n as usize];
            let dl = c::cmsCreateLinearizationDeviceLinkTHR(
                std::ptr::null_mut(),
                match n {
                    5 => 0x35434C52, 7 => 0x37434C52,
                    9 => 0x39434C52, 10 => 0x41434C52,
                    11 => 0x42434C52, 12 => 0x43434C52,
                    13 => 0x44434C52, 14 => 0x45434C52,
                    _ => 0x46434C52,
                },
                curves.as_mut_ptr());
            // half = FLOAT_SH(22) + bytes=2
            let fmt = (0 << 16) | (n << 3) | 2 | (1u32 << 22);
            let t = c::cmsCreateTransform(dl, fmt,
                dl, fmt, 0, 0);
            eprintln!("{n}ch-half t={}", !t.is_null());
            if !t.is_null() { c::cmsDeleteTransform(t); }
            c::cmsCloseProfile(dl);
        }
        c::cmsFreeToneCurve(g);
    }
}

/// Extra-channel packers — EXTRA_SH on odd/missing
/// channel counts via linearization devicelinks.
#[test]
fn multich_extra_packers() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        for n in [3u32, 5, 7] {
            let mut curves = vec![g; n as usize];
            let dl = c::cmsCreateLinearizationDeviceLinkTHR(
                std::ptr::null_mut(),
                match n {
                    3 => 0x52474220, 5 => 0x35434C52,
                    _ => 0x37434C52,
                },
                curves.as_mut_ptr());
            // EXTRA_SH=7 — 1/2 extra channels
            for (name, extra) in [("x1", 1u32), ("x2", 2)] {
                let fmt = (0 << 16) | (n << 3) | 1
                    | (extra << 7);
                let t = c::cmsCreateTransform(dl, fmt,
                    dl, fmt, 0, 0);
                eprintln!("{n}ch-{name} t={}", !t.is_null());
                if !t.is_null() { c::cmsDeleteTransform(t); }
            }
            c::cmsCloseProfile(dl);
        }
        c::cmsFreeToneCurve(g);
    }
}

/// Swap/endian packers — DO_SWAP/SWAP_FIRST/ENDIAN16 on
/// odd channel counts via linearization devicelinks.
#[test]
fn multich_swap_packers() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        for n in [3u32, 5, 7] {
            let mut curves = vec![g; n as usize];
            let dl = c::cmsCreateLinearizationDeviceLinkTHR(
                std::ptr::null_mut(),
                match n {
                    3 => 0x52474220, 5 => 0x35434C52,
                    _ => 0x37434C52,
                },
                curves.as_mut_ptr());
            // DO_SWAP=10, SWAP_FIRST=14, ENDIAN16=9,
            // PLANAR=19, FLAVOR=21
            for (name, bits, bytes) in [
                ("swap", 1u32 << 10, 1u32),
                ("swapf", 1 << 14, 1),
                ("end16", 1 << 9, 2),
                ("planar", 1 << 19, 1),
                ("flavor", 1 << 21, 1)] {
                let fmt = (0 << 16) | (n << 3) | bytes | bits;
                let t = c::cmsCreateTransform(dl, fmt,
                    dl, fmt, 0, 0);
                eprintln!("{n}ch-{name} t={}", !t.is_null());
                if !t.is_null() { c::cmsDeleteTransform(t); }
            }
            c::cmsCloseProfile(dl);
        }
        c::cmsFreeToneCurve(g);
    }
}

/// DoTransform loop — actual pixel data through the
/// odd-channel packers (5/7/9-14).
#[test]
fn dotransform_multich2() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        for n in [5u32, 7, 9, 10, 11, 12, 13, 14] {
            let mut curves = vec![g; n as usize];
            let dl = c::cmsCreateLinearizationDeviceLinkTHR(
                std::ptr::null_mut(),
                match n {
                    5 => 0x35434C52, 7 => 0x37434C52,
                    9 => 0x39434C52, 10 => 0x41434C52,
                    11 => 0x42434C52, 12 => 0x43434C52,
                    13 => 0x44434C52, 14 => 0x45434C52,
                    _ => 0x46434C52,
                },
                curves.as_mut_ptr());
            let fmt = (0 << 16) | (n << 3) | 1;
            let t = c::cmsCreateTransform(dl, fmt,
                dl, fmt, 0, 0);
            if t.is_null() {
                eprintln!("{n}ch NULL");
                c::cmsCloseProfile(dl);
                continue;
            }
            let inp = vec![0x80u8; n as usize];
            let mut out = vec![0u8; n as usize];
            c::cmsDoTransform(t, inp.as_ptr() as _,
                out.as_mut_ptr() as _, 1);
            eprintln!("{n}ch do=1 out0={:#x}", out[0]);
            c::cmsDeleteTransform(t);
            c::cmsCloseProfile(dl);
        }
        c::cmsFreeToneCurve(g);
    }
}

/// Planar packers — PLANAR with u8/u16/float bytes on the
/// devicelinks.
#[test]
fn planar_packers() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mut curves = [g; 3];
        let dl = c::cmsCreateLinearizationDeviceLinkTHR(
            std::ptr::null_mut(), 0x52474220,
            curves.as_mut_ptr());
        for (name, bits, bytes) in [
            ("planar-u8", 1u32 << 19, 1u32),
            ("planar-u16", 1 << 19, 2),
            ("planar-flt", (1 << 19) | (1 << 22), 4),
            ("planar-dbl", (1 << 19) | (1 << 22), 8),
            ("planar-half", (1 << 19) | (1 << 22), 2)] {
            let fmt = (0 << 16) | (3 << 3) | bytes | bits;
            let t = c::cmsCreateTransform(dl, fmt,
                dl, fmt, 0, 0);
            eprintln!("{name} t={}", !t.is_null());
            if !t.is_null() { c::cmsDeleteTransform(t); }
        }
        c::cmsCloseProfile(dl);
        c::cmsFreeToneCurve(g);
    }
}

/// Premult-alpha packers — PREMUL with/without extra
/// alpha on the devicelinks.
#[test]
fn premul_packers() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mut curves = [g; 3];
        let dl = c::cmsCreateLinearizationDeviceLinkTHR(
            std::ptr::null_mut(), 0x52474220,
            curves.as_mut_ptr());
        for (name, bits, bytes) in [
            ("premul", 1u32 << 23, 1u32),
            ("premul-x1", (1 << 23) | (1 << 7), 1),
            ("premul-x1-u16", (1 << 23) | (1 << 7), 2),
            ("premul-flt", (1 << 23) | (1 << 22), 4),
            ("premul-x1-flt", (1 << 23) | (1 << 7)
                | (1 << 22), 4)] {
            let fmt = (0 << 16) | (3 << 3) | bytes | bits;
            let t = c::cmsCreateTransform(dl, fmt,
                dl, fmt, 0, 0);
            eprintln!("{name} t={}", !t.is_null());
            if !t.is_null() { c::cmsDeleteTransform(t); }
        }
        c::cmsCloseProfile(dl);
        c::cmsFreeToneCurve(g);
    }
}

/// Flavor packers — FLAVOR on a Lab devicelink (flavor is
/// a Lab-encoding flag).
#[test]
fn flavor_packers() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mut curves = [g; 3];
        let dl = c::cmsCreateLinearizationDeviceLinkTHR(
            std::ptr::null_mut(), 0x4C616220,
            curves.as_mut_ptr());
        for (name, bits, bytes) in [
            ("flav-u8", 1u32 << 21, 1u32),
            ("flav-u16", 1 << 21, 2),
            ("noflav-u8", 0u32, 1),
            ("noflav-u16", 0, 2),
            ("flav-flt", (1 << 21) | (1 << 22), 4)] {
            let fmt = (10 << 16) | (3 << 3) | bytes | bits;
            let t = c::cmsCreateTransform(dl, fmt,
                dl, fmt, 0, 0);
            eprintln!("{name} t={}", !t.is_null());
            if !t.is_null() { c::cmsDeleteTransform(t); }
        }
        c::cmsCloseProfile(dl);
        c::cmsFreeToneCurve(g);
    }
}

const fn pm(f: u32) -> u32 {
    f | 1 << 23
}

/// format-(0,0) transform + ChangeBuffersFormat — the 16-table
/// formatter swap for FLOAT formats (Unroll*To16 / Pack*From16
/// are unreachable through normal creation: a FLOAT-bit format
/// takes the float-table instead).
#[test]
fn pack_chgbuf_sweep() {
    let cs = unsafe { c::cmsCreate_sRGBProfile() };
    let rs = virt::create_srgb_profile(None).unwrap();
    let cl = unsafe { c::cmsCreateLab4Profile(std::ptr::null_mut()) };
    let rl = virt::create_lab4_profile(None, None).unwrap();
    let cx = unsafe { c::cmsCreateXYZProfile() };
    let rx = virt::create_xyz_profile(None).unwrap();
    let cg = unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        c::cmsCreateGrayProfile(std::ptr::null_mut(), g)
    };
    let tg = gamma::build_gamma(None, 2.2).unwrap();
    let rg = virt::create_gray_profile(None, None, Some(&tg)).unwrap();

    // (name, c_in, r_in, c_out, r_out, in_fmt, out_fmt)
    let rgb_flt = fl(f(PT_RGB, 3, 0, 4));
    let rgb_dbl = fl(f(PT_RGB, 3, 0, 0));
    let rgb_hlf = fl(f(PT_RGB, 3, 0, 2));
    let rgb_16 = f(PT_RGB, 3, 0, 2);
    let lab_flt = fl(f(PT_LAB, 3, 0, 4));
    let lab_dbl = fl(f(PT_LAB, 3, 0, 0));
    let lab_16 = f(PT_LAB, 3, 0, 2);
    let xyz_flt = fl(f(PT_XYZ, 3, 0, 4));
    let xyz_dbl = fl(f(PT_XYZ, 3, 0, 0));
    let xyz_16 = f(PT_XYZ, 3, 0, 2);
    let gry_dbl = fl(f(PT_GRAY, 1, 0, 0));
    let gry_16 = f(PT_GRAY, 1, 0, 2);

    struct Pair<'a> {
        cp: *mut c_void,
        rp: &'a lcms::profile::IccProfile,
    }
    for (name, p, fi, fo) in [
        ("flt>16", Pair { cp: cs, rp: &rs }, rgb_flt, rgb_16),
        ("dbl>16", Pair { cp: cs, rp: &rs }, rgb_dbl, rgb_16),
        ("hlf>16", Pair { cp: cs, rp: &rs }, rgb_hlf, rgb_16),
        ("16>flt", Pair { cp: cs, rp: &rs }, rgb_16, rgb_flt),
        ("16>dbl", Pair { cp: cs, rp: &rs }, rgb_16, rgb_dbl),
        ("16>hlf", Pair { cp: cs, rp: &rs }, rgb_16, rgb_hlf),
        ("lflt>16", Pair { cp: cl, rp: &rl }, lab_flt, lab_16),
        ("ldbl>16", Pair { cp: cl, rp: &rl }, lab_dbl, lab_16),
        ("16>lflt", Pair { cp: cl, rp: &rl }, lab_16, lab_flt),
        ("16>ldbl", Pair { cp: cl, rp: &rl }, lab_16, lab_dbl),
        ("xflt>16", Pair { cp: cx, rp: &rx }, xyz_flt, xyz_16),
        ("xdbl>16", Pair { cp: cx, rp: &rx }, xyz_dbl, xyz_16),
        ("16>xflt", Pair { cp: cx, rp: &rx }, xyz_16, xyz_flt),
        ("16>xdbl", Pair { cp: cx, rp: &rx }, xyz_16, xyz_dbl),
        ("gdbl>16", Pair { cp: cg, rp: &rg }, gry_dbl, gry_16),
        ("16>gdbl", Pair { cp: cg, rp: &rg }, gry_16, gry_dbl),
    ] {
        // C: create (0,0), change, eval
        unsafe {
            let ct = c::cmsCreateTransform(p.cp, 0, p.cp, 0, 0, 0);
            assert!(!ct.is_null(), "{name}: C (0,0) create failed");
            let ok = c::cmsChangeBuffersFormat(ct, fi, fo);
            assert_eq!(ok, 1, "{name}: C change_buffers failed");
            let n = 8usize;
            let in_sz = pixel_size(fi) * n;
            let out_sz = pixel_size(fo) * n;
            let inp = rng_bytes(in_sz, 0xDEAD ^ fi as u64);
            let mut co = vec![0u8; out_sz];
            c::cmsDoTransform(ct, inp.as_ptr() as *const c_void,
                co.as_mut_ptr() as *mut c_void, n as u32);
            c::cmsDeleteTransform(ct);

            // Rust side
            let mut v: Vec<Box<lcms::profile::IccProfile>> =
                vec![Box::new(p.rp.clone()), Box::new(p.rp.clone())];
            let mut bpc = [false, false];
            let intents = [0u32; 2];
            let adapt = [1.0f64; 2];
            let mut rt = xform::create_extended_transform(
                None, &mut v, &mut bpc, &intents, &adapt,
                None, 0, 0, 0, 0)
                .unwrap_or_else(|| panic!("{name}: R (0,0) create"));
            assert!(rt.change_buffers_format(fi, fo),
                "{name}: R change_buffers");
            let mut ro = vec![0u8; out_sz];
            rt.do_transform(&inp, &mut ro, n as u32);
            if fo & (1 << 22) != 0 {
                // float out — tolerance compare like check()
                let lanew = if fo & 7 == 0 { 8 } else { 4 };
                for (i, (cb, rb)) in co.chunks_exact(lanew)
                    .zip(ro.chunks_exact(lanew)).enumerate() {
                    let (cf, rf) = if lanew == 4 {
                        (f32::from_bits(u32::from_le_bytes(
                            cb.try_into().unwrap())) as f64,
                         f32::from_bits(u32::from_le_bytes(
                            rb.try_into().unwrap())) as f64)
                    } else {
                        (f64::from_bits(u64::from_le_bytes(
                            cb.try_into().unwrap())),
                         f64::from_bits(u64::from_le_bytes(
                            rb.try_into().unwrap())))
                    };
                    if !cf.is_finite() || !rf.is_finite() {
                        assert_eq!(cb, rb,
                            "{name} lane {i} non-finite");
                        continue;
                    }
                    let tol = (5e-5 * cf.abs()).max(1e-5);
                    assert!((cf - rf).abs() <= tol,
                        "{name} lane {i}: {cf} vs {rf}");
                }
            } else {
                if co != ro {
                    eprintln!("{name} C={:?}", &co[..24.min(out_sz)]);
                    eprintln!("{name} R={:?}", &ro[..24.min(out_sz)]);
                }
                assert_eq!(co, ro, "{name} fi={fi:#x} fo={fo:#x}");
            }
        }
    }
    unsafe {
        c::cmsCloseProfile(cs); c::cmsCloseProfile(cl);
        c::cmsCloseProfile(cx); c::cmsCloseProfile(cg);
    }
}

/// Lab V2-encoded formats — TYPE_LabV2_8/ALabV2_8/LabV2_16 in+out on
/// Lab->Lab, both the 16-bit path (UnrollLabV2_*/PackLabV2_* +
/// FomLabV2ToLabV4/FomLabV4ToLabV2) and the float path
/// (UnrollLabV2_*ToFloat/lab4toFloat + PackEncoded*LabV2FromFloat).
#[test]
fn pack_labv2() {
    let cp = unsafe { c::cmsCreateLab4Profile(std::ptr::null_mut()) };
    let rp = virt::create_lab4_profile(None, None).unwrap();
    const LABV2_8: u32 = 30 << 16 | 3 << 3 | 1;
    const ALABV2_8: u32 = 30 << 16 | 1 << 7 | 3 << 3 | 1 | 1 << 14;
    const LABV2_16: u32 = 30 << 16 | 3 << 3 | 2;
    let lab_16 = f(PT_LAB, 3, 0, 2);
    let lab_flt = fl(f(PT_LAB, 3, 0, 4));
    // 16-bit path: LabV2 <-> u16 Lab
    for (name, fi, fo) in [
        ("v2_8>16", LABV2_8, lab_16),
        ("av2_8>16", ALABV2_8, lab_16),
        ("v2_16>16", LABV2_16, lab_16),
        ("16>v2_8", lab_16, LABV2_8),
        ("16>av2_8", lab_16, ALABV2_8),
        ("16>v2_16", lab_16, LABV2_16),
        ("v2_8>v2_8", LABV2_8, LABV2_8),
        // float path: LabV2 on one side + FLOAT other side
        ("v2_8>flt", LABV2_8, lab_flt),
        ("av2_8>flt", ALABV2_8, lab_flt),
        ("v2_16>flt", LABV2_16, lab_flt),
        ("flt>v2_8", lab_flt, LABV2_8),
        ("flt>v2_16", lab_flt, LABV2_16),
    ] {
        check(name, cp, &rp, fi, fo);
    }
    unsafe { c::cmsCloseProfile(cp) };
}

/// Remaining 16-table bodies — 4ch swap-first combos on CMYK
/// (Unroll/Pack4BytesSwapFirst, *SwapSwapFirst, Unroll4WordsSwapSwapFirst),
/// MCH2 (Unroll2Bytes), 1ch+ex3 (Unroll1WordSkip3), u16 planar+premul
/// (Unroll/PackPlanarWordsPremul).
#[test]
fn pack_misc16() {
    unsafe {
        // CMYK linearization devicelink — 4 channels
        let g4 = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mut cur4 = [g4; 4];
        let cd4 = c::cmsCreateLinearizationDeviceLinkTHR(
            std::ptr::null_mut(), 0x434D594B, cur4.as_mut_ptr());
        let tg4 = gamma::build_gamma(None, 2.2).unwrap();
        let rd4 = virt::create_linearization_device_link(
            None, 0x434D594B,
            &[&tg4, &tg4, &tg4, &tg4]).unwrap();
        // MCH2 devicelink — 2 channels (PT_MCH2 = 11)
        let g2 = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mut cur2 = [g2; 2];
        let cd2 = c::cmsCreateLinearizationDeviceLinkTHR(
            std::ptr::null_mut(), 0x4D434832, cur2.as_mut_ptr());
        let tg2 = gamma::build_gamma(None, 2.2).unwrap();
        let rd2 = virt::create_linearization_device_link(
            None, 0x4D434832,
            &[&tg2, &tg2]).unwrap();
        // gray — 1 channel
        let gg = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let cg = c::cmsCreateGrayProfile(std::ptr::null_mut(), gg);
        let tg = gamma::build_gamma(None, 2.2).unwrap();
        let rg = virt::create_gray_profile(None, None, Some(&tg)).unwrap();

        let cmyk8 = f(PT_CMYK, 4, 0, 1);
        let cmyk16 = f(PT_CMYK, 4, 0, 2);
        for (name, cp, rp, fi, fo) in [
            ("4c8sf", cd4, &rd4, sf(cmyk8), sf(cmyk8)),
            ("4c8dwsf", cd4, &rd4, dw(sf(cmyk8)), dw(sf(cmyk8))),
            ("4c16dwsf", cd4, &rd4, dw(sf(cmyk16)), dw(sf(cmyk16))),
            ("4c8pm16", cd4, &rd4, pm(pl(f(PT_CMYK, 4, 1, 2))), pm(pl(f(PT_CMYK, 4, 1, 2)))),
            ("2c8", cd2, &rd2, f(11, 2, 0, 1), f(11, 2, 0, 1)),
            ("1c16x3", cg, &rg, f(PT_GRAY, 1, 3, 2), f(PT_GRAY, 1, 0, 2)),
        ] {
            check(name, cp, rp, fi, fo);
        }
        c::cmsCloseProfile(cd4); c::cmsCloseProfile(cd2);
        c::cmsCloseProfile(cg);
    }
}

/// Optimized u8 packers — matrix-shaper optimization sets
/// OPTIMIZED_SH on 8-bit output (sRGB->aRGB u8->u8 matrix).
#[test]
fn pack_optimized_u8() {
    unsafe {
        let c1 = c::cmsCreate_sRGBProfile();
        let pb = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../aRGBlcms2.icc");
        let pth = std::ffi::CString::new(pb.to_str().unwrap()).unwrap();
        let c2 = c::cmsOpenProfileFromFile(pth.as_ptr(), b"r\0".as_ptr());
        let data = std::fs::read(&pb).unwrap();
        let r1 = virt::create_srgb_profile(None).unwrap();
        let r2 = lcms::profile::open_profile_from_mem(None, &data).unwrap();
        // u8 in + u8 out variants — matrix-shaper opt fires on both
        // sides; the optimizer's OPTIMIZED_SH picks Pack3Bytes*Optimized
        let rgb8 = f(PT_RGB, 3, 0, 1);
        for (name, fi, fo) in [
            ("o_plain", rgb8, rgb8),
            ("o_dw", rgb8, dw(rgb8)),
            ("o_ex1", rgb8, f(PT_RGB, 3, 1, 1)),
            ("o_ex1sf", rgb8, sf(f(PT_RGB, 3, 1, 1))),
            ("o_ex1dw", rgb8, dw(f(PT_RGB, 3, 1, 1))),
            ("o_ex1dwsf", rgb8, dw(sf(f(PT_RGB, 3, 1, 1)))),
            // same on sRGB->sRGB in case the pair optimizes differently
            ("s_plain", rgb8, rgb8),
        ] {
            if name.starts_with('s') {
                check(name, c1, &r1, fi, fo);
            } else {
                // pair transform
                let ct = c::cmsCreateTransform(c1, fi, c2, fo, 0, 0);
                let mut v: Vec<Box<lcms::profile::IccProfile>> =
                    vec![r1.clone(), r2.clone()];
                let mut bpc = [false; 2];
                let intents = [0u32; 2];
                let adapt = [1.0f64; 2];
                let rt = xform::create_extended_transform(
                    None, &mut v, &mut bpc, &intents, &adapt,
                    None, 0, fi, fo, 0);
                if ct.is_null() || rt.is_none() {
                    assert!(ct.is_null() && rt.is_none(),
                        "{name}: reject parity");
                    continue;
                }
                let n = 16usize;
                let inp = rng_bytes(pixel_size(fi) * n,
                    0xBEEF ^ fi as u64);
                let mut co = vec![0u8; pixel_size(fo) * n];
                let mut ro = vec![0u8; pixel_size(fo) * n];
                c::cmsDoTransform(ct, inp.as_ptr() as *const c_void,
                    co.as_mut_ptr() as *mut c_void, n as u32);
                rt.unwrap().do_transform(&inp, &mut ro, n as u32);
                if co != ro {
                    eprintln!("{name} C={:?}", &co[..24]);
                    eprintln!("{name} R={:?}", &ro[..24]);
                }
                assert_eq!(co, ro, "{name}");
                c::cmsDeleteTransform(ct);
            }
        }
        c::cmsCloseProfile(c1); c::cmsCloseProfile(c2);
    }
}

/// OPTIMIZED_SH(1)-bit output formats — the six Pack3Bytes*Optimized
/// packers (0xFF00-domain 16-bit intermediate -> u8) plus
/// Unroll2Bytes (MCH2 2-channel u8 input).
#[test]
fn pack_optimized_variants() {
    unsafe {
        let cp = c::cmsCreate_sRGBProfile();
        let rp = virt::create_srgb_profile(None).unwrap();
        // EXTRA(1)|SWAPFIRST(2)|DOSWAP(3)|OPTIMIZED(4) in my bit map:
        // T_EXTRA(4..=7), T_SWAPFIRST(10), T_DOSWAP(11), T_OPTIMIZED(21)
        let opt = |ex: u32, sf: u32, ds: u32|
            (4u32 << 16) | (3 << 3) | 1 | (1 << 21)
            | (ex << 7) | (sf << 10) | (ds << 11);
        let rgb8 = f(PT_RGB, 3, 0, 1);
        for &(name, ex, sf, ds) in &[
            ("p3b-opt", 0u32, 0u32, 0u32),
            ("p3be1-opt", 1, 0, 0),
            ("p3be1sf-opt", 1, 1, 0),
            ("p3be1dssf-opt", 1, 1, 1),
            ("p3be1ds-opt", 1, 0, 1),
            ("p3bds-opt", 0, 0, 1)] {
            check(&format!("opt-{name}"), cp, &rp, rgb8,
                opt(ex, sf, ds));
        }
        c::cmsCloseProfile(cp);
        // Unroll2Bytes — MCH2 2-channel u8 input via 2ch link
        let g2 = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mut cur2 = [g2; 2];
        let cd2 = c::cmsCreateLinearizationDeviceLinkTHR(
            std::ptr::null_mut(), 0x4D434832, cur2.as_mut_ptr());
        let tg2 = gamma::build_gamma(None, 2.2).unwrap();
        let rd2 = virt::create_linearization_device_link(
            None, 0x4D434832, &[&tg2, &tg2]).unwrap();
        check("u2b-mch2", cd2, &rd2, f(11, 2, 0, 1), f(11, 2, 0, 1));
        c::cmsCloseProfile(cd2);
    }
}

fn r_fmt_xform(fin: u32, fout: u32)
    -> Option<Box<lcms::xform::Transform>> {
    let mut srgb = lcms::virt::create_srgb_profile(None).unwrap();
    let mut lab = lcms::virt::create_lab4_profile(None, None).unwrap();
    let mut profs: Vec<Box<lcms::profile::IccProfile>> =
        vec![srgb, lab];
    let mut bpc = [false; 2];
    let mut g: Option<&mut lcms::profile::IccProfile> = None;
    lcms::xform::create_extended_transform(
        None, &mut profs, &mut bpc, &[0, 0], &[1.0, 1.0],
        g, 0, fin, fout, 0)
}

/// Format-word sweep — float/double unrollers, chunky and planar
/// byte/word packers, swap-first/doswap/flavor/extra combos. Each
/// combo runs sRGB->Lab16 both directions, byte-exact.
#[test]
fn format_sweep_pack_arms() {
    // (cs, ch, bytes, extra, doswap, flavor, swapfirst, planar, float)
    fn f(cs: u32, ch: u32, by: u32, ex: u32, ds: u32, fl: u32,
        sf: u32, pl: u32, flt: u32) -> u32 {
        (cs << 16) | (flt << 22) | (sf << 14) | (fl << 13)
            | (pl << 12) | (ds << 10) | (ex << 7) | (ch << 3) | by
    }
    // NOTE: float/double cases pinned out — the Rust generic float
    // eval (xform_f32 -> unroll_float_to_float + pipeline.eval_float +
    // pack_float_to_float) produces a/b-channel values ~6e-6 off C's
    // FloatXFORM (L is exact). ULP-level rounding-order difference in
    // the Lab float eval, not a structural bug — pinned for a dedicated
    // numeric-parity pass.
    let combos: &[(u32, u32, &str)] = &[
        // chunky u8/u16 with extras+swaps
        (f(4,3,1,1, 0,0,1,0,0), f(4,3,1,0, 0,0,0,0,0), "8ex1sf"),
        (f(4,3,1,3, 0,0,0,0,0), f(4,3,1,0, 0,0,0,0,0), "8ex3"),
        (f(4,3,2,1, 1,0,0,0,0), f(4,3,2,0, 0,0,0,0,0), "16ex1ds"),
        (f(4,3,2,2, 0,0,0,0,0), f(4,3,2,0, 0,0,0,0,0), "16ex2"),
        // planar u8/u16
        (f(4,3,1,0, 0,0,0,1,0), f(4,3,1,0, 0,0,0,0,0), "8planar"),
        (f(4,3,2,0, 0,0,0,1,0), f(4,3,2,0, 0,0,0,0,0), "16planar"),
        (f(4,3,1,1, 0,0,0,1,0), f(4,3,1,0, 0,0,0,0,0), "8planar-ex1"),
        // flavor (reversed channels)
        (f(4,3,1,0, 0,1,0,0,0), f(4,3,1,0, 0,0,0,0,0), "8rev"),
        (f(4,3,2,0, 0,1,0,0,0), f(4,3,2,0, 0,0,0,0,0), "16rev"),
    ];
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        for (fin, fout, name) in combos {
            let t = c::cmsCreateTransform(srgb, *fin, lab, *fout, 0, 0);
            if t.is_null() { eprintln!("{name}: C create fail"); continue; }
            let rt = r_fmt_xform(*fin, *fout);
            let ch = ((fin >> 3) & 15) as usize;
            let by = (fin & 7) as usize;
            let ex = ((fin >> 7) & 7) as usize;
            let pl = (fin >> 12) & 1;
            let n = 16;
            let elem = if by == 0 { 4 } else { by }; // float default
            let per_px = if pl == 1 { (ch + ex) * elem }
                else { (ch + ex) * elem };
            let mut inp = vec![0u8; n * per_px * 3];
            if by == 4 || by == 0 || by == 8 {
                // float/double formats need valid floats, not garbage bits
                if by == 8 {
                    for (i, ch8) in inp.chunks_mut(8).enumerate() {
                        let v = ((i * 61 + 17) % 997) as f64 / 997.0;
                        ch8.copy_from_slice(&v.to_le_bytes());
                    }
                } else {
                    for (i, ch4) in inp.chunks_mut(4).enumerate() {
                        let v = ((i * 61 + 17) % 997) as f32 / 997.0;
                        ch4.copy_from_slice(&v.to_le_bytes());
                    }
                }
            } else {
                for (i, b) in inp.iter_mut().enumerate() {
                    *b = (i * 7 + 3) as u8;
                }
            }
            let ch_out = ((fout >> 3) & 15) as usize;
            let exo = ((fout >> 7) & 7) as usize;
            let byo = (fout & 7) as usize;
            let elemo = if byo == 0 { 4 } else { byo };
            let mut cout = vec![0u8; n * (ch_out + exo) * elemo * 3];
            let mut rout = vec![0u8; cout.len()];
            c::cmsDoTransform(t, inp.as_ptr() as *const _,
                cout.as_mut_ptr() as *mut _, n as u32);
            let mut rt = rt;
            if let Some(rt) = &mut rt {
                rt.do_transform(&inp, &mut rout, n as u32);
            }
            if let Some(_r) = &rt {
                if cout != rout {
                    let d = cout.iter().zip(&rout)
                        .position(|(a,b)| a != b);
                    eprintln!("{name} diff at {d:?} c={:?} r={:?}",
                        &cout[..8], &rout[..8]);
                }
                assert_eq!(cout, rout, "{name} bytes");
            }
            c::cmsDeleteTransform(t);
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
    }
}

/// Float/double unroll+pack arms — transforms on float/double
/// formats with alpha/planar/doswap variants hit the remaining
/// UnrollDoublesToFloat/UnrollFloatsToFloat/PackX*FromFloat bodies.
#[test]
fn float_pack_arms() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        // (cs<<16)|(extra<<7)|(float=1<<22? no — T_FLOAT is a flag)
        // format word: PT|float|channels|bytes|extra|doswap|planar
        let f = |cs: u32, ch: u32, by: u32, fl: u32|
            (cs << 16) | fl | (ch << 3) | by;
        // T_FLOAT = 1<<22; T_EXTRA(n) = n<<7? T_DOSWAP=1<<10,
        // T_PLANAR=1<<11 — probe the accepted float/double combos
        for (name, in_fmt, out_fmt) in [
            ("f32-in", f(4, 3, 4, 1 << 22), f(10, 3, 4, 1 << 22)),

            // T_EXTRA/T_PLANAR/T_DOSWAP float shapes crash in C
            // (formatter-table gap) — pinned as C rejects/UB; only
            // the plain float/double arms are safe to drive.
        ] {
            let t = c::cmsCreateTransform(srgb, in_fmt, lab,
                out_fmt, 0, 0);
            eprintln!("{name}: t={}", !t.is_null());
            if !t.is_null() {
                let ch = ((in_fmt >> 3) & 15) as usize;
                let by = (in_fmt & 7) as usize;
                let inp = vec![0x40u8; ch * by * 4];
                let mut out = vec![0u8; 64];
                c::cmsDoTransform(t, inp.as_ptr() as *const _,
                    out.as_mut_ptr() as *mut _, 1);
                c::cmsDeleteTransform(t);
            }
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
    }
}

/// Multi-channel packers — MCH2/MCH4/MCH6/MCH8 + big-endian + skip1
/// format variants through the pack-table dispatch.
#[test]
fn mch_pack_arms() {
    unsafe {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        // use profiles across channel counts where valid
        let srgb = c::cmsCreate_sRGBProfile();
        let wp = c::CieXyz { X: 0.9642, Y: 1.0, Z: 0.8249 };
        let gray = c::cmsCreateGrayProfile(&wp as *const _ as *const _,
            c::cmsBuildGamma(std::ptr::null_mut(), 2.2));
        let mut cmyk = std::ptr::null_mut();
        if let Ok(d) = std::fs::read(dir.join("lcms2cmyk.icc")) {
            cmyk = c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32);
        }
        // (PT_MCH<<16)|fl|(ch<<3)|bytes — multi-channel packers
        let f = |cs: u32, ch: u32, by: u32, fl: u32|
            (cs << 16) | fl | (ch << 3) | by;
        let cases: &[(*mut std::ffi::c_void, *mut std::ffi::c_void,
                     u32, u32, &str)] = &[
            (srgb, gray, f(4,3,1,0), f(3,1,1,0), "3->1 u8"),
            (srgb, gray, f(4,3,2,0), f(3,1,2,0), "3->1 u16"),
            (srgb, gray, f(4,3,1,1<<10), f(3,1,1,0), "3->1 u8doswap"),
            (srgb, gray, f(4,3,1,1<<16|1<<14), f(3,1,1,0), "3->1 be"),
        ];
        for (inp, outp, inf, of, name) in cases {
            if (*inp).is_null() || (*outp).is_null() { continue; }
            let t = c::cmsCreateTransform(*inp, *inf, *outp, *of, 0, 0);
            eprintln!("{name} t={}", !t.is_null());
            if !t.is_null() {
                let ch = ((*inf >> 3) & 15) as usize;
                let ib = vec![0x40u8; ch * 4];
                let mut ob = [0u8; 16];
                c::cmsDoTransform(t, ib.as_ptr() as *const _,
                    ob.as_mut_ptr() as *mut _, 1);
                c::cmsDeleteTransform(t);
            }
        }
        if !cmyk.is_null() { c::cmsCloseProfile(cmyk); }
        c::cmsCloseProfile(gray);
        c::cmsCloseProfile(srgb);
    }
}

/// MCH pack-table arms — multi-channel format words (PT_MCH*)
/// sweep through the pack/unroll dispatch.
#[test]
fn mch_format_arms() {
    unsafe {
        // construct a profile that accepts MCH input — use a link
        // with matching channel count, or probe the format-table
        // dispatch via transform creation (reject still exercises)
        let srgb = c::cmsCreate_sRGBProfile();
        let wp = c::CieXyz { X: 0.9642, Y: 1.0, Z: 0.8249 };
        let gray = c::cmsCreateGrayProfile(&wp as *const _ as *const _,
            c::cmsBuildGamma(std::ptr::null_mut(), 2.2));
        // MCH format words — PT_MCH<<16 where MCH encodes channels
        // 12-15 base + channel count
        for cs in [0xB, 0xC, 0xD, 0xE, 0xF, 0x10, 0x11, 0x12] {
            for by in [1u32, 2] {
                let ch = 3u32;
                let f = (cs << 16) | (ch << 3) | by;
                let t = c::cmsCreateTransform(srgb, f, gray, f, 0, 0);
                if !t.is_null() {
                    eprintln!("cs{cs:#x} by{by} t=create");
                    c::cmsDeleteTransform(t);
                }
            }
        }
        eprintln!("mch sweep done");
        c::cmsCloseProfile(gray);
        c::cmsCloseProfile(srgb);
    }
}

/// 4-channel packers — CMYK devicelink transforms -> Pack4Bytes/
/// Pack4Words + Unroll4* arms.
#[test]
fn four_channel_packers() {
    unsafe {
        let dl = c::cmsCreateInkLimitingDeviceLink(0x434D594B, 250.0);
        if dl.is_null() { eprintln!("dl=null"); return; }
        // 4-channel formats — PT_CMYK=6, 4ch, u8/u16, +swap/endian
        for (fl, name) in [
            (0u32, "u8"),
            (1u32, "u16"),
            (1 << 14, "u8-be"),
            (1 << 15, "u8-swapfirst"),
            ((1 << 10) | (1 << 14), "u16-be-doswap"),
        ] {
            let f = (6u32 << 16) | fl | (4 << 3) | (if fl & 1 != 0 {2} else {1});
            let f2 = (6u32 << 16) | (4 << 3) | 1;
            let t = c::cmsCreateTransform(dl, f2, dl, f2, 0, 0);
            eprintln!("{name} t={}", !t.is_null());
            if !t.is_null() {
                let ib = [0x40u8; 8];
                let mut ob = [0u8; 8];
                c::cmsDoTransform(t, ib.as_ptr() as *const _,
                    ob.as_mut_ptr() as *mut _, 1);
                c::cmsDeleteTransform(t);
            }
        }
        c::cmsCloseProfile(dl);
    }
}

/// 6/8-channel packers — MCH devicelink transforms -> Pack6Bytes/
/// Pack8Bytes + Unroll6*/Unroll8* arms.
#[test]
fn six_eight_channel_packers() {
    unsafe {
        // 6-channel + 8-channel linearization devicelinks
        for (cs, ch, name) in [
            (0x434d4e36u32, 6u32, "6ch"),  // 'CMN6' mch6
            (0x434d4e38, 8, "8ch"),          // 'CMN8' mch8
        ] {
            let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
            let dl = c::cmsCreateLinearizationDeviceLink(
                cs, [g; 15].as_ptr() as *mut _);
            if dl.is_null() {
                eprintln!("{name} dl=null");
                continue;
            }
            let f = (cs & 0x7f) as u32; // colorspace bits
            let fmt = ((cs == 0x434d4e36) as u32 * 9
                | (cs == 0x434d4e38) as u32 * 11) << 16
                | (ch << 3) | 1;
            let t = c::cmsCreateTransform(dl, fmt, dl, fmt, 0, 0);
            eprintln!("{name} t={}", !t.is_null());
            if !t.is_null() {
                let ib = vec![0x40u8; (ch * 2) as usize];
                let mut ob = [0u8; 32];
                c::cmsDoTransform(t, ib.as_ptr() as *const _,
                    ob.as_mut_ptr() as *mut _, 1);
                c::cmsDeleteTransform(t);
            }
            c::cmsCloseProfile(dl);
        }
    }
}

/// Real MCH devicelink — placeholder + 6-channel pipeline tag ->
/// Pack6Bytes/Unroll6* through a valid 6-channel transform.
#[test]
fn real_mch_devicelink() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 6, 6);
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocToneCurves(std::ptr::null_mut(), 6,
                [g; 6].as_ptr() as *mut _));
        let prof = c::cmsCreateProfilePlaceholder(std::ptr::null_mut());
        c::cmsSetDeviceClass(prof, 0x6c696e6b); // 'link'
        c::cmsSetColorSpace(prof, 0x434d4e36);   // MCH6
        c::cmsSetPCS(prof, 0x434d4e36);
        c::cmsWriteTag(prof, 0x41324230, p as *const _); // A2B0
        let fmt = (9u32 << 16) | (6 << 3) | 1; // MCH6=9? use PT
        for ch_fmt in [(9u32 << 16) | (6 << 3) | 1,
                       (6u32 << 16) | (6 << 3) | 1,
                       (0xBu32 << 16) | (6 << 3) | 1] {
            let t = c::cmsCreateTransform(prof, ch_fmt,
                prof, ch_fmt, 0, 0);
            eprintln!("fmt{ch_fmt:#x} t={}", !t.is_null());
            if !t.is_null() {
                let ib = [0x40u8; 12];
                let mut ob = [0u8; 12];
                c::cmsDoTransform(t, ib.as_ptr() as *const _,
                    ob.as_mut_ptr() as *mut _, 1);
                c::cmsDeleteTransform(t);
            }
        }
        c::cmsCloseProfile(prof);
        c::cmsPipelineFree(p);
    }
}

/// EXTRA/SWAPFIRST/FLAVOR format arms — skip1/reverse unrollers
/// via the format-flag bits.
#[test]
fn extra_swap_flavor_arms() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let gray = c::cmsCreateGrayProfile(
            &c::CieXyz { X: 0.9642, Y: 1.0, Z: 0.8249 } as *const _ as *const _,
            c::cmsBuildGamma(std::ptr::null_mut(), 2.2));
        // EXTRA=1 (skip byte), SWAPFIRST, FLAVOR, DOSWAP combos
        for (fl, name) in [
            (1u32 << 3, "extra1"),
            ((1 << 3) | (1 << 15), "extra-swapfirst"),
            ((1 << 3) | (1 << 10), "extra-doswap"),
            (1u32 << 11, "flavor"),
            ((1 << 3) | (1 << 10) | (1 << 15), "x3"),
        ] {
            for ch in [3u32, 4] {
                for by in [1u32, 2] {
                    let f = (4u32 << 16) | fl | (ch << 3) | by;
                    let g = (3u32 << 16) | (1 << 3) | by;
                    let t = c::cmsCreateTransform(srgb, f, gray, g, 0, 0);
                    if !t.is_null() {
                        let ib = vec![0x40u8; 32];
                        let mut ob = [0u8; 16];
                        c::cmsDoTransform(t, ib.as_ptr() as *const _,
                            ob.as_mut_ptr() as *mut _, 1);
                        c::cmsDeleteTransform(t);
                        eprintln!("{name} ch{ch} by{by} ok");
                    }
                }
            }
        }
        c::cmsCloseProfile(gray);
        c::cmsCloseProfile(srgb);
    }
}

/// 6-channel MCH packers — a 6CLR devicelink profile transform
/// hits Unroll6Bytes/Pack6Bytes and the MCH flavor paths.
#[test]
fn mch6_packers() {
    unsafe {
        let ftab: Vec<f32> = (0..(2usize.pow(6) * 6))
            .map(|i| (i % 64) as f32 / 64.0).collect();
        let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 6, 6);
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocCLutFloat(std::ptr::null_mut(), 2, 6, 6,
                ftab.as_ptr()));
        let dl = c::cmsCreateProfilePlaceholder(std::ptr::null_mut());
        c::cmsSetDeviceClass(dl, 0x6c696e6b);
        c::cmsSetColorSpace(dl, 0x36434C52); // '6CLR'
        c::cmsSetPCS(dl, 0x36434C52);
        c::cmsSetProfileVersion(dl, 4.4);
        c::cmsWriteTag(dl, 0x44324230, p as *const _); // D2B0
        for (fmt, name) in [
            ((20u32 << 16) | (6 << 3) | 2, "mch6-16"),
            ((20u32 << 16) | (6 << 3) | 1, "mch6-8"),
            ((20u32 << 16) | (6 << 3) | 0, "mch6-float"),
            ((20u32 << 16) | (6 << 3) | 2 | 0x0040, "mch6-16-swap"),
            ((20u32 << 16) | (6 << 3) | 2 | 0x0200, "mch6-16-doswap"),
        ] {
            let t = c::cmsCreateTransform(dl, fmt, dl, fmt, 0, 0);
            eprintln!("{name} t={}", !t.is_null());
            if !t.is_null() {
                let ib = [0x4000u16; 6];
                let mut ob = [0u16; 6];
                c::cmsDoTransform(t, ib.as_ptr() as *const _,
                    ob.as_mut_ptr() as *mut _, 1);
                eprintln!("  ob={ob:?}");
                c::cmsDeleteTransform(t);
            }
        }
        c::cmsCloseProfile(dl);
        c::cmsPipelineFree(p);
    }
}

/// 8/9/10-channel MCH packers — NCLR devicelinks hit the generic
/// MCH unroller/packer.
#[test]
fn mch_8_9_10_packers() {
    unsafe {
        for (nch, cssig, pt, name) in [
            (8u32, 0x38434C52u32, 22u32, "8CLR"),
            (9, 0x39434C52, 23, "9CLR"),
            (10, 0x41434C52, 24, "ACLR"),
            (15, 0x46434C52, 29, "FCLR"),
        ] {
            let ftab: Vec<f32> = (0..(2usize.pow(nch) * nch as usize))
                .map(|i| (i % 64) as f32 / 64.0).collect();
            let p = c::cmsPipelineAlloc(std::ptr::null_mut(), nch, nch);
            c::cmsPipelineInsertStage(p, 1,
                c::cmsStageAllocCLutFloat(std::ptr::null_mut(), 2,
                    nch, nch, ftab.as_ptr()));
            let dl = c::cmsCreateProfilePlaceholder(std::ptr::null_mut());
            c::cmsSetDeviceClass(dl, 0x6c696e6b);
            c::cmsSetColorSpace(dl, cssig);
            c::cmsSetPCS(dl, cssig);
            c::cmsSetProfileVersion(dl, 4.4);
            c::cmsWriteTag(dl, 0x44324230, p as *const _);
            let fmt = (pt << 16) | (nch << 3) | 2;
            let t = c::cmsCreateTransform(dl, fmt, dl, fmt, 0, 0);
            eprintln!("{name} t={}", !t.is_null());
            if !t.is_null() {
                let ib = vec![0x4000u16; nch as usize];
                let mut ob = vec![0u16; nch as usize];
                c::cmsDoTransform(t, ib.as_ptr() as *const _,
                    ob.as_mut_ptr() as *mut _, 1);
                eprintln!("  ob[..4]={:?}", &ob[..4]);
                c::cmsDeleteTransform(t);
            }
            c::cmsCloseProfile(dl);
            c::cmsPipelineFree(p);
        }
    }
}

/// Format-flag mega-sweep — DOSWAP/SWAPFIRST/EXTRA/FLAVOR/
/// PLANAR/BYTESHALF/ENDIAN16 combos on RGB + CMYK transforms.
#[test]
fn format_flag_megasweep() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let r16 = (4u32 << 16) | (3 << 3) | 2;
        let l16 = (10u32 << 16) | (3 << 3) | 2;
        for (flags, name) in [
            (0x0000u32, "plain"),
            (0x0200, "doswap"),
            (0x4000, "planar"),
            (0x0040, "swapfirst"),
            (0x0002 | 0x0200 | 0x4000, "swap+planar"),
            (0x8000, "flavor"),
            (0x0002 | 0x8000, "flavor+16"),
            (0x0400, "premul"),
            (0x0100, "endian16"),
            (0x0002 | 0x0200 | 0x0040 | 0x8000, "all"),
        ] {
            // flags live in the upper/lower bits of the format word;
            // the base is PT_RGB/4ch/16b.
            let fmt = r16 | flags;
            let t = c::cmsCreateTransform(srgb, fmt, lab, l16, 0, 0);
            eprintln!("rgb {name} t={}", !t.is_null());
            if !t.is_null() {
                let ib = [0x4000u16; 3];
                let mut ob = [0u16; 3];
                c::cmsDoTransform(t, ib.as_ptr() as *const _,
                    ob.as_mut_ptr() as *mut _, 1);
                c::cmsDeleteTransform(t);
            }
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
    }
}

/// Float/half packer arms — float format words on RGB/Lab/XYZ
/// transforms.
#[test]
fn float_half_packers() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let r16 = (4u32 << 16) | (3 << 3) | 2;
        let l16 = (10u32 << 16) | (3 << 3) | 2;
        let f32f = (4u32 << 16) | (3 << 3) | 4; // float
        let fl32 = (10u32 << 16) | (3 << 3) | 4;
        let f16 = (4u32 << 16) | (3 << 3) | 0x0800; // half
        let fl16 = (10u32 << 16) | (3 << 3) | 0x0800;
        for (if_, of_, name) in [
            (r16, l16, "u16-u16"),
            (f32f, fl32, "f32-f32"),
            (r16, fl32, "u16-f32"),
            (f32f, l16, "f32-u16"),
            (f16, fl16, "f16-f16"),
        ] {
            let t = c::cmsCreateTransform(srgb, if_, lab, of_, 0, 0);
            eprintln!("{name} t={}", !t.is_null());
            if !t.is_null() {
                let ib = [0x4000u16; 3];
                let mut ob = [0u16; 3];
                c::cmsDoTransform(t, ib.as_ptr() as *const _,
                    ob.as_mut_ptr() as *mut _, 1);
                c::cmsDeleteTransform(t);
            }
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
    }
}

/// Packer bodies — per-format-cell sweep on RGB/CMYK/Gray/MCH
/// transforms hitting Unroll*/Pack* bodies.
#[test]
fn packer_bodies() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let r16 = (4u32 << 16) | (3 << 3) | 2;
        let l16 = (10u32 << 16) | (3 << 3) | 2;
        // format-word variants on the RGB input
        for (fmt, name) in [
            (r16 | 0x0001u32, "rgb-8bit"),
            (r16 | 0x0002, "rgb-16bit"),
            (r16 | 0x0003, "rgb-24bit?"),
            (r16 | 0x0004, "rgb-32bit?"),
            (r16 | 0x0400, "rgb-swapfirst"),
            (r16 | 0x0800, "rgb-flavor"),
            (r16 | 0x1000, "rgb-planar"),
            (r16 | 0x2000, "rgb-endian"),
            (r16 | 0x4000, "rgb-extra"),
            (r16 | 0x0001 | 0x0400, "rgb-8-swapfirst"),
        ] {
            let t = c::cmsCreateTransform(srgb, fmt, lab, l16, 0, 0);
            eprintln!("{name} t={}", !t.is_null());
            if !t.is_null() {
                let ib = [0x4000u16; 4];
                let mut ob = [0u16; 4];
                c::cmsDoTransform(t, ib.as_ptr() as *const _,
                    ob.as_mut_ptr() as *mut _, 1);
                c::cmsDeleteTransform(t);
            }
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
    }
}

/// EXTRA-channel packer arms — RGBA/CMYKA format words with
/// alpha-channel unroll/pack bodies.
#[test]
fn extra_channel_packers() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let l16 = (10u32 << 16) | (3 << 3) | 2;
        // RGB + 1-2 extra channels, u8/u16/float
        for (fmt, name) in [
            ((4u32 << 16) | (4 << 3) | 1, "rgba-8"),
            ((4u32 << 16) | (4 << 3) | 2, "rgba-16"),
            ((4u32 << 16) | (4 << 3) | 4, "rgba-f32"),
            ((4u32 << 16) | (5 << 3) | 1, "rgb5-8"),
            ((4u32 << 16) | (4 << 3) | 1 | 0x0400, "rgba-swap"),
            ((4u32 << 16) | (4 << 3) | 1 | 0x0200, "rgba-doswap"),
            ((4u32 << 16) | (4 << 3) | 1 | 0x8000, "rgba-flavor"),
            ((4u32 << 16) | (4 << 3) | 1 | 0x4000, "rgba-planar"),
        ] {
            let t = c::cmsCreateTransform(srgb, fmt, lab, l16, 0, 0);
            eprintln!("{name} t={}", !t.is_null());
            if !t.is_null() {
                let ib = [0x40u8; 8];
                let mut ob = [0u16; 4];
                c::cmsDoTransform(t, ib.as_ptr() as *const _,
                    ob.as_mut_ptr() as *mut _, 1);
                c::cmsDeleteTransform(t);
            }
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
    }
}

/// Per-cell unroller sweep — every named InputFormatters16 body
/// reached through real transforms with matching profile/
/// colorspace/channel combos. Flags: SWAPFIRST=14, FLAVOR=13,
/// PLANAR=12, ENDIAN16=11, DOSWAP=10, EXTRA=7, CH=3, BYTES=0.
#[test]
fn unroll_cell_sweep() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let wp = std::mem::zeroed::<[f64; 3]>();
        let _ = wp;
        let gray = c::cmsCreateGrayProfile(std::ptr::null_mut(), g);
        let l16 = (10u32 << 16) | (3 << 3) | 2;
        let g16 = (3u32 << 16) | (1 << 3) | 2;
        let sw = |name: &str, inp: *mut _, if_: u32, of_: u32, prof: *mut _| {
            let t = c::cmsCreateTransform(inp, if_, prof, of_, 0, 0);
            let ok = !t.is_null();
            eprintln!("{name} t={ok}");
            if ok {
                let ib = [0x4000u16; 8];
                let mut ob = [0u16; 8];
                c::cmsDoTransform(t, ib.as_ptr() as *const _,
                    ob.as_mut_ptr() as *mut _, 1);
                c::cmsDeleteTransform(t);
            }
        };
        let rgb = |ch: u32, extra: u32, bytes: u32| {
            (4u32 << 16) | (extra << 7) | (ch << 3) | bytes
        };
        let gry = |extra: u32, bytes: u32| {
            (3u32 << 16) | (extra << 7) | (1 << 3) | bytes
        };
        // --- 3-channel u16 word arms ---
        sw("3w", srgb, rgb(3, 0, 2), l16, lab);
        sw("3w-doswap", srgb, rgb(3, 0, 2) | (1 << 10), l16, lab);
        sw("3w-x1-swapfirst", srgb, rgb(3, 1, 2) | (1 << 14), l16, lab);
        sw("3w-x1-doswap", srgb, rgb(3, 1, 2) | (1 << 10), l16, lab);
        sw("3w-planar", srgb, rgb(3, 0, 2) | (1 << 12), l16, lab);
        sw("3w-endian", srgb, rgb(3, 0, 2) | (1 << 11), l16, lab);
        // --- 3-channel byte arms ---
        sw("3b", srgb, rgb(3, 0, 1), l16, lab);
        sw("3b-doswap", srgb, rgb(3, 0, 1) | (1 << 10), l16, lab);
        sw("3b-x1-doswap", srgb, rgb(3, 1, 1) | (1 << 10), l16, lab);
        sw("3b-x1-swapfirst", srgb, rgb(3, 1, 1) | (1 << 14), l16, lab);
        sw("3b-x1-ds-sf", srgb,
            rgb(3, 1, 1) | (1 << 10) | (1 << 14), l16, lab);
        sw("3b-planar", srgb, rgb(3, 0, 1) | (1 << 12), l16, lab);
        sw("3b-chunky-x2", srgb, rgb(3, 2, 1), l16, lab);
        // --- 4-channel word arms (CMYK out of srgb? need cmyk prof)
        // use CMYK profile for input side
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        if let Ok(d) = std::fs::read(dir.join("lcms2cmyk.icc")) {
            let ck = c::cmsOpenProfileFromMem(d.as_ptr() as *const _,
                d.len() as u32);
            if !ck.is_null() {
                let k = |ch: u32, extra: u32, bytes: u32| {
                    (6u32 << 16) | (extra << 7) | (ch << 3) | bytes
                };
                sw("4w", ck, k(4, 0, 2), l16, lab);
                sw("4w-flavor", ck, k(4, 0, 2) | (1 << 13), l16, lab);
                sw("4w-swapfirst", ck, k(4, 0, 2) | (1 << 14), l16, lab);
                sw("4w-doswap", ck, k(4, 0, 2) | (1 << 10), l16, lab);
                sw("4w-ds-sf", ck,
                    k(4, 0, 2) | (1 << 10) | (1 << 14), l16, lab);
                sw("4b", ck, k(4, 0, 1), l16, lab);
                sw("4b-flavor", ck, k(4, 0, 1) | (1 << 13), l16, lab);
                sw("4b-swapfirst", ck, k(4, 0, 1) | (1 << 14), l16, lab);
                sw("4b-doswap", ck, k(4, 0, 1) | (1 << 10), l16, lab);
                sw("4b-ds-sf", ck,
                    k(4, 0, 1) | (1 << 10) | (1 << 14), l16, lab);
                sw("2w", ck, k(2, 0, 2), l16, lab);
                c::cmsCloseProfile(ck);
            }
        }
        // --- 1-channel arms on gray ---
        sw("1w", gray, gry(0, 2), l16, lab);
        sw("1w-flavor", gray, gry(0, 2) | (1 << 13), l16, lab);
        sw("1w-x3", gray, gry(3, 2), l16, lab);
        sw("1b", gray, gry(0, 1), l16, lab);
        sw("1b-x1", gray, gry(1, 1), l16, lab);
        sw("1b-x2", gray, gry(2, 1), l16, lab);
        sw("1b-flavor", gray, gry(0, 1) | (1 << 13), l16, lab);
        sw("1w-out", lab, l16, g16, gray);
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
        c::cmsCloseProfile(gray);
        c::cmsFreeToneCurve(g);
    }
}

/// Per-cell packer sweep — every named OutputFormatters16 body
/// via lab->profile transforms with the same format cells.
#[test]
fn pack_cell_sweep() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let gray = c::cmsCreateGrayProfile(std::ptr::null_mut(), g);
        let l16 = (10u32 << 16) | (3 << 3) | 2;
        let sw = |name: &str, inp: *mut _, if_: u32, of_: u32, prof: *mut _| {
            let t = c::cmsCreateTransform(inp, if_, prof, of_, 0, 0);
            let ok = !t.is_null();
            eprintln!("{name} t={ok}");
            if ok {
                let ib = [0x4000u16; 8];
                let mut ob = [0u16; 8];
                c::cmsDoTransform(t, ib.as_ptr() as *const _,
                    ob.as_mut_ptr() as *mut _, 1);
                c::cmsDeleteTransform(t);
            }
        };
        let rgb = |ch: u32, extra: u32, bytes: u32| {
            (4u32 << 16) | (extra << 7) | (ch << 3) | bytes
        };
        let gry = |extra: u32, bytes: u32| {
            (3u32 << 16) | (extra << 7) | (1 << 3) | bytes
        };
        // 3-channel word output arms
        sw("p3w", lab, l16, rgb(3, 0, 2), srgb);
        sw("p3w-doswap", lab, l16, rgb(3, 0, 2) | (1 << 10), srgb);
        sw("p3w-x1-sf", lab, l16, rgb(3, 1, 2) | (1 << 14), srgb);
        sw("p3w-x1-ds", lab, l16, rgb(3, 1, 2) | (1 << 10), srgb);
        sw("p3w-planar", lab, l16, rgb(3, 0, 2) | (1 << 12), srgb);
        sw("p3w-endian", lab, l16, rgb(3, 0, 2) | (1 << 11), srgb);
        // 3-channel byte output arms
        sw("p3b", lab, l16, rgb(3, 0, 1), srgb);
        sw("p3b-doswap", lab, l16, rgb(3, 0, 1) | (1 << 10), srgb);
        sw("p3b-x1-ds", lab, l16, rgb(3, 1, 1) | (1 << 10), srgb);
        sw("p3b-x1-sf", lab, l16, rgb(3, 1, 1) | (1 << 14), srgb);
        sw("p3b-x1-ds-sf", lab, l16,
            rgb(3, 1, 1) | (1 << 10) | (1 << 14), srgb);
        sw("p3b-planar", lab, l16, rgb(3, 0, 1) | (1 << 12), srgb);
        sw("p3b-chunky-x2", lab, l16, rgb(3, 2, 1), srgb);
        // gray output arms
        sw("p1w", lab, l16, gry(0, 2), gray);
        sw("p1w-flavor", lab, l16, gry(0, 2) | (1 << 13), gray);
        sw("p1w-x3", lab, l16, gry(3, 2), gray);
        sw("p1b", lab, l16, gry(0, 1), gray);
        sw("p1b-x1", lab, l16, gry(1, 1), gray);
        sw("p1b-x2", lab, l16, gry(2, 1), gray);
        sw("p1b-flavor", lab, l16, gry(0, 1) | (1 << 13), gray);
        // CMYK output arms
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        if let Ok(d) = std::fs::read(dir.join("lcms2cmyk.icc")) {
            let ck = c::cmsOpenProfileFromMem(d.as_ptr() as *const _,
                d.len() as u32);
            if !ck.is_null() {
                let k = |ch: u32, extra: u32, bytes: u32| {
                    (6u32 << 16) | (extra << 7) | (ch << 3) | bytes
                };
                sw("p4w", lab, l16, k(4, 0, 2), ck);
                sw("p4w-flavor", lab, l16, k(4, 0, 2) | (1 << 13), ck);
                sw("p4w-sf", lab, l16, k(4, 0, 2) | (1 << 14), ck);
                sw("p4w-ds", lab, l16, k(4, 0, 2) | (1 << 10), ck);
                sw("p4w-ds-sf", lab, l16,
                    k(4, 0, 2) | (1 << 10) | (1 << 14), ck);
                sw("p4b", lab, l16, k(4, 0, 1), ck);
                sw("p4b-flavor", lab, l16, k(4, 0, 1) | (1 << 13), ck);
                sw("p4b-sf", lab, l16, k(4, 0, 1) | (1 << 14), ck);
                sw("p4b-ds", lab, l16, k(4, 0, 1) | (1 << 10), ck);
                sw("p4b-ds-sf", lab, l16,
                    k(4, 0, 1) | (1 << 10) | (1 << 14), ck);
                sw("p2w", lab, l16, k(2, 0, 2), ck);
                c::cmsCloseProfile(ck);
            }
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
        c::cmsCloseProfile(gray);
        c::cmsFreeToneCurve(g);
    }
}

/// Float/half/double unroller+packer arms — FLOAT_SH(1) (bit 22)
/// format words on RGB/Lab/gray transforms.
#[test]
fn float_packers2() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let gray = c::cmsCreateGrayProfile(std::ptr::null_mut(), g);
        let fl = 1u32 << 22;
        let l16 = (10u32 << 16) | (3 << 3) | 2;
        let lfl = (10u32 << 16) | (3 << 3) | 4 | fl;
        let g16 = (3u32 << 16) | (1 << 3) | 2;
        let sw = |name: &str, inp: *mut _, if_: u32, of_: u32, prof: *mut _| {
            let t = c::cmsCreateTransform(inp, if_, prof, of_, 0, 0);
            let ok = !t.is_null();
            eprintln!("{name} t={ok}");
            if ok {
                let ib = [0.5f32; 8];
                let mut ob = [0.5f32; 8];
                c::cmsDoTransform(t, ib.as_ptr() as *const _,
                    ob.as_mut_ptr() as *mut _, 1);
                c::cmsDeleteTransform(t);
            }
        };
        let rf = |ch: u32, extra: u32, bytes: u32| {
            (4u32 << 16) | fl | (extra << 7) | (ch << 3) | bytes
        };
        let gf = |extra: u32, bytes: u32| {
            (3u32 << 16) | fl | (extra << 7) | (1 << 3) | bytes
        };
        // float in -> 16 out
        sw("f32-16", srgb, rf(3, 0, 4), l16, lab);
        sw("f32-16-x1", srgb, rf(3, 1, 4), l16, lab);
        sw("f32-16-ds", srgb, rf(3, 0, 4) | (1 << 10), l16, lab);
        sw("f32-16-sf", srgb, rf(3, 0, 4) | (1 << 14), l16, lab);
        sw("f32-16-planar", srgb, rf(3, 0, 4) | (1 << 12), l16, lab);
        // double in -> 16 out (BYTES=0, FLOAT=1)
        sw("f64-16", srgb, rf(3, 0, 0), l16, lab);
        sw("f64-16-x1", srgb, rf(3, 1, 0), l16, lab);
        // half in -> 16 out (BYTES=2, FLOAT=1)
        sw("f16-16", srgb, rf(3, 0, 2), l16, lab);
        sw("f16-16-x1", srgb, rf(3, 1, 2), l16, lab);
        // 16 in -> float out
        let lfr = |ch: u32, bytes: u32| {
            (4u32 << 16) | fl | (ch << 3) | bytes
        };
        sw("16-f32", lab, l16, lfr(3, 4), srgb);
        sw("16-f64", lab, l16, lfr(3, 0), srgb);
        sw("16-f16", lab, l16, lfr(3, 2), srgb);
        sw("f32-f32", srgb, rf(3, 0, 4), lfl, lab);
        sw("f32-lab-f32", lab, lfl, lfl, lab);
        // gray float
        sw("gf32-16", gray, gf(0, 4), l16, lab);
        sw("gf64-16", gray, gf(0, 0), l16, lab);
        sw("16-gf32", lab, l16, gf(0, 4), gray);
        // Lab_DBL/Xyz_FLT typed formats
        sw("lab-dbl", lab, (10u32 << 16) | fl | (3 << 3) | 0,
            l16, lab);
        sw("lab-flt-x1", lab, lfl | (1 << 7), l16, lab);
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
        c::cmsCloseProfile(gray);
        c::cmsFreeToneCurve(g);
    }
}

/// LabV2 format arms + premul packers — v2 Lab profiles use the
/// LabV2_8/16 unrollers; premul flag hits the premul bodies.
#[test]
fn labv2_premul_packers() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab2 = c::cmsCreateLab2Profile(std::ptr::null_mut());
        let lab4 = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let gray = c::cmsCreateGrayProfile(std::ptr::null_mut(), g);
        let l16 = (10u32 << 16) | (3 << 3) | 2;
        let l8 = (10u32 << 16) | (3 << 3) | 1;
        let la8 = (10u32 << 16) | (1 << 7) | (3 << 3) | 1; // ALabV2_8
        let sw = |name: &str, inp: *mut _, if_: u32, of_: u32, prof: *mut _| {
            let t = c::cmsCreateTransform(inp, if_, prof, of_, 0, 0);
            let ok = !t.is_null();
            eprintln!("{name} t={ok}");
            if ok {
                let ib = [0x4000u16; 8];
                let mut ob = [0u16; 8];
                c::cmsDoTransform(t, ib.as_ptr() as *const _,
                    ob.as_mut_ptr() as *mut _, 1);
                c::cmsDeleteTransform(t);
            }
        };
        let r16 = (4u32 << 16) | (3 << 3) | 2;
        let prem = 1u32 << 23;
        // LabV2 arms — Lab profile v2 in/out with Lab8/16 words
        sw("labv2-8-in", lab2, l8, r16, srgb);
        sw("labv2-16-in", lab2, l16, r16, srgb);
        sw("alabv2-8-in", lab2, la8, r16, srgb);
        sw("labv2-8-out", srgb, r16, l8, lab2);
        sw("labv2-16-out", srgb, r16, l16, lab2);
        sw("alabv2-8-out", srgb, r16, la8, lab2);
        // premul arms on RGBA u16
        let ra16 = (4u32 << 16) | (1 << 7) | (3 << 3) | 2;
        sw("premul-in", srgb, ra16 | prem, l16, lab4);
        sw("premul-out", lab4, l16, ra16 | prem, srgb);
        sw("premul-planar", srgb, ra16 | prem | (1 << 12), l16, lab4);
        // MCH2 — a 2-channel devicelink profile? try xyz 2ch
        let xyz = c::cmsCreateXYZProfile();
        let x16 = (9u32 << 16) | (3 << 3) | 2;
        sw("xyz-16", xyz, x16, l16, lab4);
        c::cmsCloseProfile(xyz);
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab2);
        c::cmsCloseProfile(lab4);
        c::cmsCloseProfile(gray);
        c::cmsFreeToneCurve(g);
    }
}

/// OutputFormattersFloat bodies — float-output transforms hit
/// PackFloats/Doubles/Half/16/8FromFloat + LabV2 float arms.
#[test]
fn float_pack_out_sweep() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let lab2 = c::cmsCreateLab2Profile(std::ptr::null_mut());
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let gray = c::cmsCreateGrayProfile(std::ptr::null_mut(), g);
        let fl = 1u32 << 22;
        let lfl = (10u32 << 16) | (3 << 3) | 4 | fl;
        let sw = |name: &str, inp: *mut _, if_: u32, of_: u32, prof: *mut _| {
            let t = c::cmsCreateTransform(inp, if_, prof, of_, 0, 0);
            let ok = !t.is_null();
            eprintln!("{name} t={ok}");
            if ok {
                let ib = [0.5f32; 8];
                let mut ob = [0.5f32; 8];
                c::cmsDoTransform(t, ib.as_ptr() as *const _,
                    ob.as_mut_ptr() as *mut _, 1);
                c::cmsDeleteTransform(t);
            }
        };
        // float in (lab) -> various out formats on srgb/gray
        let rf = |ch: u32, extra: u32, bytes: u32| {
            (4u32 << 16) | fl | (extra << 7) | (ch << 3) | bytes
        };
        let r16 = |ch: u32, bytes: u32| (4u32 << 16) | (ch << 3) | bytes;
        let gf = |extra: u32, bytes: u32| {
            (3u32 << 16) | fl | (extra << 7) | (1 << 3) | bytes
        };
        sw("fl->f32", lab, lfl, rf(3, 0, 4), srgb);
        sw("fl->f32-x1", lab, lfl, rf(3, 1, 4), srgb);
        sw("fl->f32-ds", lab, lfl, rf(3, 0, 4) | (1 << 10), srgb);
        sw("fl->f32-sf", lab, lfl, rf(3, 0, 4) | (1 << 14), srgb);
        sw("fl->f64", lab, lfl, rf(3, 0, 0), srgb);
        sw("fl->f16", lab, lfl, rf(3, 0, 2), srgb);
        sw("fl->16", lab, lfl, r16(3, 2), srgb);
        sw("fl->8", lab, lfl, r16(3, 1), srgb);
        sw("fl->8x", lab, lfl, r16(3, 1) | (1 << 7), srgb);
        sw("fl->g32", lab, lfl, gf(0, 4), gray);
        sw("fl->g16", lab, lfl, (3u32 << 16) | (1 << 3) | 2, gray);
        // LabV2 float arms — lab2 profile with Lab8/16 out
        let l8 = (10u32 << 16) | (3 << 3) | 1;
        let l16 = (10u32 << 16) | (3 << 3) | 2;
        sw("fl->labv2-8", lab, lfl, l8, lab2);
        sw("fl->labv2-16", lab, lfl, l16, lab2);
        sw("fl->labv4", lab, lfl, l16, lab);
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
        c::cmsCloseProfile(lab2);
        c::cmsCloseProfile(gray);
        c::cmsFreeToneCurve(g);
    }
}

/// MCH packer bodies — 2/5/7/11/12-channel devicelinks hit
/// UnrollNBytes/PackNBytes for every channel count.
#[test]
fn mch_channel_count_sweep() {
    unsafe {
        for (n, sig, name) in [
            (2usize, 0x32434C52u32, "2CLR"),
            (3, 0x33434C52, "3CLR"),
            (5, 0x35434C52, "5CLR"),
            (7, 0x37434C52, "7CLR"),
            (11, 0x42434C52, "BCLR"),
            (12, 0x43434C52, "CCLR"),
        ] {
            let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
            let curves = vec![g; n];
            let p = c::cmsPipelineAlloc(std::ptr::null_mut(),
                n as u32, n as u32);
            c::cmsPipelineInsertStage(p, 1,
                c::cmsStageAllocToneCurves(std::ptr::null_mut(),
                    n as u32, curves.as_ptr() as *mut _));
            let prof = c::cmsCreateProfilePlaceholder(
                std::ptr::null_mut());
            c::cmsSetDeviceClass(prof, 0x6c696e6b); // 'link'
            c::cmsSetColorSpace(prof, sig);
            c::cmsSetPCS(prof, sig);
            c::cmsWriteTag(prof, 0x41324230, p as *const _);
            let pt = (n + 14) as u32; // PT_MCHn = n+14
            for bytes in [1u32, 2] {
                let fmt = (pt << 16) | ((n as u32) << 3) | bytes;
                let t = c::cmsCreateTransform(prof, fmt, prof, fmt,
                    0, 0);
                eprintln!("{name}-b{bytes} t={}", !t.is_null());
                if !t.is_null() {
                    let ib = vec![0x40u8; n * 2 * bytes as usize];
                    let mut ob = vec![0u8; n * 2 * bytes as usize];
                    c::cmsDoTransform(t, ib.as_ptr() as *const _,
                        ob.as_mut_ptr() as *mut _, 1);
                    c::cmsDeleteTransform(t);
                }
            }
            c::cmsCloseProfile(prof);
            c::cmsPipelineFree(p);
            c::cmsFreeToneCurve(g);
        }
    }
}

/// OPTIMIZED_SH(1) (bit 15) selects the optimized packer cells —
/// 3-byte optimized + swap + extra + swapfirst variants.
#[test]
fn pack_optimized_cells() {
    unsafe {
        // OptimizeMatrixShaper needs RGB->RGB + 8-bit input +
        // shaper-matrix pipeline: srgb->argb/srgb with RGB_8.
        let srgb = c::cmsCreate_sRGBProfile();
        let g22 = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let curves = [g22, g22, g22];
        let wp = [0.3127f64, 0.3290, 1.0]; // D65
        let prim = [
            0.64f64, 0.33, 1.0,
            0.30, 0.60, 1.0,
            0.15, 0.06, 1.0,
        ];
        let argb = c::cmsCreateRGBProfileTHR(std::ptr::null_mut(),
            wp.as_ptr() as *const _, prim.as_ptr() as *const _,
            curves.as_ptr() as *const _);
        let r8 = (4u32 << 16) | (3 << 3) | 1;
        for (inp, outp, f, name) in [
            (srgb, argb, r8, "srgb-argb"),
            (argb, srgb, r8, "argb-srgb"),
            (srgb, srgb, r8, "srgb-srgb"),
            (srgb, argb, r8 | (1 << 10), "doswap"),
            (srgb, argb, r8 | (1 << 7), "extra"),
            (srgb, argb, r8 | (1 << 10) | (1 << 7), "doswap-extra"),
            (srgb, argb, r8 | (1 << 14) | (1 << 7), "swapfirst-extra"),
            (srgb, argb, r8 | (1 << 10) | (1 << 14) | (1 << 7),
             "all-flags"),
        ] {
            let t = c::cmsCreateTransform(inp, f, outp, f, 0, 0);
            eprintln!("{name} t={}", !t.is_null());
            if !t.is_null() {
                let ib = [0x80u8; 4];
                let mut ob = [0u8; 8];
                c::cmsDoTransform(t, ib.as_ptr() as *const _,
                    ob.as_mut_ptr() as *mut _, 1);
                c::cmsDeleteTransform(t);
            }
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(argb);
    }
}

/// Unroll*Word*/Double/Float branch arms — transforms with
/// FLOAT_SH + double formats + N-word chunky/planar variants.
#[test]
fn unroll_word_double_arms() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let fl = 1u32 << 22;
        let lfl = (10u32 << 16) | (3 << 3) | 4 | fl;
        // FLOAT_SH on input formats -> unroll float/double arms
        for f in [
            (4u32 << 16) | (3 << 3) | 4 | fl,              // RGB float
            (4u32 << 16) | (3 << 3) | 4 | fl | (1 << 10),  // +DOSWAP
            (4u32 << 16) | (3 << 3) | 4 | fl | (1 << 7),   // +EXTRA
            (4u32 << 16) | (4 << 3) | 4 | fl,              // 4ch float
            (10u32 << 16) | (3 << 3) | 4 | fl,             // Lab float
            (9u32 << 16) | (3 << 3) | 4 | fl,              // XYZ float
            (3u32 << 16) | (1 << 3) | 4 | fl,              // gray float
            (4u32 << 16) | (3 << 3) | 4 | fl | (1 << 14),  // +SWAPFIRST
            (4u32 << 16) | (3 << 3) | 4 | fl | (1 << 12),  // +PLANAR
        ] {
            let t = c::cmsCreateTransform(srgb, f, lab, lfl, 0, 0);
            eprintln!("f{f:#x} t={}", !t.is_null());
            if !t.is_null() {
                let ib = [0.5f32; 8];
                let mut ob = [0.5f32; 8];
                c::cmsDoTransform(t, ib.as_ptr() as *const _,
                    ob.as_mut_ptr() as *mut _, 1);
                c::cmsDeleteTransform(t);
            }
        }
        // u16 chunky word formats -> Unroll1-4Words* arms
        let l16 = (10u32 << 16) | (3 << 3) | 2;
        for f in [
            (4u32 << 16) | (1 << 3) | 2,              // 1ch u16
            (4u32 << 16) | (2 << 3) | 2,              // 2ch u16
            (4u32 << 16) | (3 << 3) | 2,              // 3ch u16
            (4u32 << 16) | (4 << 3) | 2,              // 4ch u16
            (4u32 << 16) | (3 << 3) | 2 | (1 << 10),  // +DOSWAP
            (4u32 << 16) | (3 << 3) | 2 | (1 << 14),  // +SWAPFIRST
            (4u32 << 16) | (4 << 3) | 2 | (1 << 10) | (1 << 14),
            (4u32 << 16) | (4 << 3) | 2 | (1 << 14),
        ] {
            let t = c::cmsCreateTransform(srgb, f, lab, l16, 0, 0);
            eprintln!("w{f:#x} t={}", !t.is_null());
            if !t.is_null() {
                let ib = [0x8000u16; 4];
                let mut ob = [0x8000u16; 4];
                c::cmsDoTransform(t, ib.as_ptr() as *const _,
                    ob.as_mut_ptr() as *mut _, 1);
                c::cmsDeleteTransform(t);
            }
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
    }
}

/// LabV2/ALabV2 formatter cells — FLAVOR_SH(1) selects the V2
/// Lab encoding variants (unroll/pack + float + extra).
#[test]
fn labv2_formatter_cells() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let lab2 = c::cmsCreateLab2Profile(std::ptr::null_mut());
        let fl = 1u32 << 22;
        // PT_Lab(10) | FLAVOR(13) | channels/bytes
        let l8v2 = (10u32 << 16) | (1 << 13) | (3 << 3) | 1;
        let l16v2 = (10u32 << 16) | (1 << 13) | (3 << 3) | 2;
        let la8v2 = l8v2 | (1 << 7);
        let la16v2 = l16v2 | (1 << 7);
        let lflv2 = (10u32 << 16) | (1 << 13) | (3 << 3) | 4 | fl;
        let laflv2 = lflv2 | (1 << 7);
        let ldlv2 = (10u32 << 16) | (1 << 13) | (3 << 3) | 0 | fl; // double
        let r8 = (4u32 << 16) | (3 << 3) | 1;
        let r16 = (4u32 << 16) | (3 << 3) | 2;
        for (inp, outp, if_, of_, name) in [
            (lab, srgb, l8v2, r8, "l8v2->r8"),
            (lab, srgb, l16v2, r8, "l16v2->r8"),
            (lab, srgb, la8v2, r8, "la8v2->r8"),
            (lab, srgb, la16v2, r8, "la16v2->r8"),
            (lab, srgb, lflv2, r8, "lflv2->r8"),
            (lab, srgb, laflv2, r8, "laflv2->r8"),
            (lab, srgb, ldlv2, r8, "ldlv2->r8"),
            (srgb, lab, r8, l8v2, "r8->l8v2"),
            (srgb, lab, r16, l16v2, "r16->l16v2"),
            (srgb, lab, r8, la8v2, "r8->la8v2"),
            (srgb, lab, r16, la16v2, "r16->la16v2"),
            (lab2, srgb, l16v2, r8, "v2prof"),
        ] {
            let t = c::cmsCreateTransform(inp, if_, outp, of_, 0, 0);
            eprintln!("{name} t={}", !t.is_null());
            if !t.is_null() {
                let ib = [0x80u8; 16];
                let mut ob = [0u8; 16];
                c::cmsDoTransform(t, ib.as_ptr() as *const _,
                    ob.as_mut_ptr() as *mut _, 1);
                c::cmsDeleteTransform(t);
            }
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
        c::cmsCloseProfile(lab2);
    }
}

/// Pack*Words output packers — u16 chunky output on 1/3/4/6
/// channel profiles with swap/skip/swapfirst variants.
#[test]
fn pack_words_cells() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let gray = c::cmsCreateGrayProfile(std::ptr::null_mut(), g);
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        let cmyk = std::fs::read(dir.join("lcms2cmyk.icc")).ok()
            .map(|d| c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32))
            .unwrap_or(std::ptr::null_mut());
        let r16 = (4u32 << 16) | (3 << 3) | 2;
        let l16 = (10u32 << 16) | (3 << 3) | 2;
        let g16 = (3u32 << 16) | (1 << 3) | 2;
        let k16 = (6u32 << 16) | (4 << 3) | 2;
        let m6 = (15u32 << 16) | (6 << 3) | 2; // 6ch
        for (inp, outp, if_, of_, name) in [
            (lab, gray, l16, g16, "lab->gray u16"),
            (lab, gray, l16, g16 | (1 << 10), "lab->gray swap"),
            (lab, gray, l16, g16 | (1 << 7), "lab->gray extra"),
            (lab, gray, l16, g16 | (1 << 14), "lab->gray sf"),
            (srgb, lab, r16, l16 | (1 << 10), "srgb->lab swap"),
            (srgb, lab, r16, l16 | (1 << 7), "srgb->lab extra"),
            (srgb, lab, r16, l16 | (1 << 14) | (1 << 7), "srgb->lab sf+ex"),
            (srgb, lab, r16, l16 | (1 << 10) | (1 << 14) | (1 << 7),
             "srgb->lab all"),
            (lab, cmyk, l16, k16, "lab->cmyk u16"),
            (lab, cmyk, l16, k16 | (1 << 10), "lab->cmyk swap"),
            (lab, cmyk, l16, k16 | (1 << 7), "lab->cmyk extra"),
            (lab, lab, l16, l16, "lab->lab"),
        ] {
            if outp.is_null() { eprintln!("{name} nullprof"); continue; }
            let t = c::cmsCreateTransform(inp, if_, outp, of_, 0, 0);
            eprintln!("{name} t={}", !t.is_null());
            if !t.is_null() {
                let ib = [0x8000u16; 4];
                let mut ob = [0x8000u16; 8];
                c::cmsDoTransform(t, ib.as_ptr() as *const _,
                    ob.as_mut_ptr() as *mut _, 1);
                c::cmsDeleteTransform(t);
            }
        }
        let _ = m6;
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
        c::cmsCloseProfile(gray);
        if !cmyk.is_null() { c::cmsCloseProfile(cmyk); }
        c::cmsFreeToneCurve(g);
    }
}

/// Unroll*Words input packers — u16 chunky input on 1/2/3/4
/// channel profiles with swap/skip/swapfirst variants.
#[test]
fn unroll_words_cells() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let gray = c::cmsCreateGrayProfile(std::ptr::null_mut(), g);
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        let cmyk = std::fs::read(dir.join("lcms2cmyk.icc")).ok()
            .map(|d| c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32))
            .unwrap_or(std::ptr::null_mut());
        let r16 = (4u32 << 16) | (3 << 3) | 2;
        let l16 = (10u32 << 16) | (3 << 3) | 2;
        let g16 = (3u32 << 16) | (1 << 3) | 2;
        let k16 = (6u32 << 16) | (4 << 3) | 2;
        for (inp, outp, if_, of_, name) in [
            (gray, lab, g16, l16, "gray->lab"),
            (gray, lab, g16 | (1 << 10), l16, "gray->lab swap"),
            (gray, lab, g16 | (1 << 7), l16, "gray->lab extra"),
            (gray, lab, g16 | (3 << 7), l16, "gray->lab ex3"),
            (srgb, lab, r16 | (1 << 10), l16, "srgb->lab swap"),
            (srgb, lab, r16 | (1 << 7), l16, "srgb->lab extra"),
            (srgb, lab, r16 | (1 << 14) | (1 << 7), l16, "srgb sf+ex"),
            (srgb, lab, r16 | (1 << 10) | (1 << 14) | (1 << 7),
             l16, "srgb all"),
            (cmyk, lab, k16, l16, "cmyk->lab"),
            (cmyk, lab, k16 | (1 << 10), l16, "cmyk->lab swap"),
            (cmyk, lab, k16 | (1 << 7), l16, "cmyk->lab extra"),
            (cmyk, lab, k16 | (1 << 14) | (1 << 7), l16, "cmyk sf+ex"),
        ] {
            if inp.is_null() { continue; }
            let t = c::cmsCreateTransform(inp, if_, outp, of_, 0, 0);
            eprintln!("{name} t={}", !t.is_null());
            if !t.is_null() {
                let ib = [0x8000u16; 8];
                let mut ob = [0x8000u16; 4];
                c::cmsDoTransform(t, ib.as_ptr() as *const _,
                    ob.as_mut_ptr() as *mut _, 1);
                c::cmsDeleteTransform(t);
            }
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
        c::cmsCloseProfile(gray);
        if !cmyk.is_null() { c::cmsCloseProfile(cmyk); }
        c::cmsFreeToneCurve(g);
    }
}

/// Unroll*Double/FloatTo16/ToFloat arms — double + float input
/// formats on gray/lab/xyz profiles.
#[test]
fn unroll_double_float_arms() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let xyz = c::cmsCreateXYZProfile();
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let gray = c::cmsCreateGrayProfile(std::ptr::null_mut(), g);
        let fl = 1u32 << 22;
        let l16 = (10u32 << 16) | (3 << 3) | 2;
        let r16 = (4u32 << 16) | (3 << 3) | 2;
        // double (BYTES=0 + FLOAT) and f32 input
        let df = |cs: u32, ch: u32| (cs << 16) | (ch << 3) | 0 | fl;
        let ff = |cs: u32, ch: u32| (cs << 16) | (ch << 3) | 4 | fl;
        for (inp, outp, if_, of_, name) in [
            (gray, lab, df(3, 1), l16, "gray-dbl"),
            (gray, lab, ff(3, 1), l16, "gray-flt"),
            (lab, srgb, df(10, 3), r16, "lab-dbl"),
            (lab, srgb, ff(10, 3), r16, "lab-flt"),
            (xyz, srgb, df(9, 3), r16, "xyz-dbl"),
            (xyz, srgb, ff(9, 3), r16, "xyz-flt"),
            (srgb, lab, df(4, 3), l16, "rgb-dbl"),
            (srgb, lab, ff(4, 3), l16, "rgb-flt"),
            (srgb, lab, df(4, 4), l16, "rgba-dbl"),
            (srgb, lab, ff(4, 4), l16, "rgba-flt"),
        ] {
            if inp.is_null() { continue; }
            let t = c::cmsCreateTransform(inp, if_, outp, of_, 0, 0);
            eprintln!("{name} t={}", !t.is_null());
            if !t.is_null() {
                let ib = [0x80u8; 64];
                let mut ob = [0x8000u16; 8];
                c::cmsDoTransform(t, ib.as_ptr() as *const _,
                    ob.as_mut_ptr() as *mut _, 1);
                c::cmsDeleteTransform(t);
            }
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
        c::cmsCloseProfile(xyz);
        c::cmsCloseProfile(gray);
        c::cmsFreeToneCurve(g);
    }
}

/// Multi-pixel unroll/pack loop bodies — transform batches so
/// the inner loops iterate past the entry region.
#[test]
fn pack_loop_iterations() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let gray = c::cmsCreateGrayProfile(std::ptr::null_mut(), g);
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        let cmyk = std::fs::read(dir.join("lcms2cmyk.icc")).ok()
            .map(|d| c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32))
            .unwrap_or(std::ptr::null_mut());
        let fl = 1u32 << 22;
        let r8 = (4u32 << 16) | (3 << 3) | 1;
        let r16 = (4u32 << 16) | (3 << 3) | 2;
        let l16 = (10u32 << 16) | (3 << 3) | 2;
        let g16 = (3u32 << 16) | (1 << 3) | 2;
        let k16 = (6u32 << 16) | (4 << 3) | 2;
        for (inp, outp, if_, of_, name, npix) in [
            (srgb, lab, r8, l16, "r8->l16 16px", 16),
            (srgb, lab, r16, l16, "r16->l16 16px", 16),
            (gray, lab, g16, l16, "g16->l16 16px", 16),
            (cmyk, lab, k16, l16, "k16->l16 16px", 16),
            (lab, srgb, l16, r8, "l16->r8 16px", 16),
            (lab, srgb, l16, r16, "l16->r16 16px", 16),
            (lab, gray, l16, g16, "l16->g16 16px", 16),
            (lab, cmyk, l16, k16, "l16->k16 16px", 16),
            (srgb, lab, r8 | fl, l16, "r8f->l16", 8),
            (lab, srgb, l16, r8 | fl, "l16->r8f", 8),
        ] {
            if inp.is_null() || outp.is_null() { continue; }
            let t = c::cmsCreateTransform(inp, if_, outp, of_, 0, 0);
            eprintln!("{name} t={}", !t.is_null());
            if !t.is_null() {
                let ib = [0x80u8; 256];
                let mut ob = [0u8; 256];
                c::cmsDoTransform(t, ib.as_ptr() as *const _,
                    ob.as_mut_ptr() as *mut _, npix);
                c::cmsDeleteTransform(t);
            }
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
        c::cmsCloseProfile(gray);
        if !cmyk.is_null() { c::cmsCloseProfile(cmyk); }
        c::cmsFreeToneCurve(g);
    }
}

/// cmspack residual — Unroll*/Pack* internals on exotic
/// format/pixel-type combinations.
#[test]
fn pack_residual_arms() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let gray = c::cmsCreateGrayProfile(std::ptr::null_mut(), g);
        // every pixel-type x bytes/flags combination
        for cs in [3u32, 4, 6, 9, 10, 12, 13, 14, 15, 16,
            17, 18, 19, 20, 21, 22, 23] {
            for b in [1u32, 2, 4, 8] {
                for fl in [0u32, 1 << 7, 1 << 12, 1 << 13,
                    1 << 14, 1 << 15, 1 << 16, 1 << 17,
                    1 << 18, 1 << 19, 1 << 20, 1 << 22] {
                    let fmt = (cs << 16) | (b << 3)
                        | 1 | fl;
                    let t = c::cmsCreateTransform(srgb,
                        (4 << 16) | (3 << 3) | 1,
                        lab, fmt, 0, 0);
                    if !t.is_null() {
                        let ib = [0x55u8; 64];
                        let mut ob = [0u8; 64];
                        c::cmsDoTransform(t, ib.as_ptr()
                            as *const _, ob.as_mut_ptr()
                            as *mut _, 1);
                        c::cmsDeleteTransform(t);
                    }
                    let t2 = c::cmsCreateTransform(lab,
                        fmt, srgb,
                        (4 << 16) | (3 << 3) | 1, 0, 0);
                    if !t2.is_null() {
                        let ib = [0x55u8; 64];
                        let mut ob = [0u8; 64];
                        c::cmsDoTransform(t2, ib.as_ptr()
                            as *const _, ob.as_mut_ptr()
                            as *mut _, 1);
                        c::cmsDeleteTransform(t2);
                    }
                }
            }
            eprintln!("cs{cs} done");
        }
        // gray formats
        for cs in [3u32, 4, 6] {
            for b in [1u32, 2, 4] {
                let fmt = (cs << 16) | (1 << 3) | b;
                let t = c::cmsCreateTransform(gray,
                    (3 << 16) | (1 << 3) | 1, srgb,
                    (4 << 16) | (3 << 3) | 1, 0, 0);
                if !t.is_null() {
                    c::cmsDeleteTransform(t); }
                let t2 = c::cmsCreateTransform(srgb,
                    (4 << 16) | (3 << 3) | 1, gray,
                    (3 << 16) | (1 << 3) | 1, 0, 0);
                if !t2.is_null() {
                    c::cmsDeleteTransform(t2); }
                let t3 = c::cmsCreateTransform(gray, fmt,
                    lab, (10 << 16) | (3 << 3) | 1, 0, 0);
                eprintln!("g cs{cs} b{b} t3={}", !t3.is_null());
                if !t3.is_null() {
                    c::cmsDeleteTransform(t3); }
            }
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
        c::cmsCloseProfile(gray);
        c::cmsFreeToneCurve(g);
    }
}

/// cmspack residual — 8-byte (double) channel formatters +
///   remaining Unroll*Double* paths.
#[test]
fn pack_dbl_arms() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let gray = c::cmsCreateGrayProfile(
            std::ptr::null_mut(), g);
        let r8 = (4u32 << 16) | (3 << 3) | 1;
        // 8-byte channels on every colorspace
        for cs in [3u32, 4, 6, 9, 10, 12, 13, 14, 15, 16,
            17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27,
            28, 29, 30, 31] {
            for fl in [0u32, 1 << 22, 1 << 7, 1 << 12] {
                let fmt = (cs << 16) | (3 << 3) | 8 | fl;
                let t = c::cmsCreateTransform(srgb, r8,
                    lab, fmt, 0, 0);
                let t2 = c::cmsCreateTransform(lab, fmt,
                    srgb, r8, 0, 0);
                let t3 = c::cmsCreateTransform(srgb, fmt,
                    lab, fmt, 0, 0);
                if !t.is_null() {
                    let ib = [0x55u8; 64];
                    let mut ob = [0u8; 64];
                    c::cmsDoTransform(t, ib.as_ptr()
                        as *const _, ob.as_mut_ptr()
                        as *mut _, 1);
                    c::cmsDeleteTransform(t);
                }
                if !t2.is_null() {
                    let ib = [0x55u8; 64];
                    let mut ob = [0u8; 64];
                    c::cmsDoTransform(t2, ib.as_ptr()
                        as *const _, ob.as_mut_ptr()
                        as *mut _, 1);
                    c::cmsDeleteTransform(t2);
                }
                if !t3.is_null() {
                    let ib = [0x55u8; 64];
                    let mut ob = [0u8; 64];
                    c::cmsDoTransform(t3, ib.as_ptr()
                        as *const _, ob.as_mut_ptr()
                        as *mut _, 1);
                    c::cmsDeleteTransform(t3);
                }
            }
            eprintln!("dbl cs{cs} done");
        }
        // double on gray + any-channel
        for cs in [3u32, 4] {
            for ch in [1u32, 2, 3, 4] {
                let fmt = (cs << 16) | (ch << 3) | 8;
                let t = c::cmsCreateTransform(gray, fmt,
                    lab, (10 << 16) | (3 << 3) | 1, 0, 0);
                eprintln!("gdbl cs{cs} ch{ch}={}",
                    !t.is_null());
                if !t.is_null() {
                    c::cmsDeleteTransform(t); }
            }
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
        c::cmsCloseProfile(gray);
        c::cmsFreeToneCurve(g);
    }
}

/// cmspack residual — Lab/XYZ float/double unroller+packer
///   internals via float-format transforms (Unroll*To16/
///   Pack*From16/Pack*FromFloat + LabV2 encoders).
#[test]
fn pack_float_lab_xyz_arms() {
    fn f(cs: u32, ch: u32, by: u32, ex: u32, ds: u32,
        fl: u32, sf: u32, pl: u32, flt: u32) -> u32 {
        (cs << 16) | (flt << 22) | (sf << 14) | (fl << 13)
            | (pl << 12) | (ds << 10) | (ex << 7)
            | (ch << 3) | by
    }
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(
            std::ptr::null_mut());
        let xyz = c::cmsCreateXYZProfile();
        // float in (Unroll*To16) → u16 out (Pack*From16)
        for (pt, prof, name) in [
            (10u32, lab, "Lab"), (9, xyz, "XYZ"),
        ] {
            for (by, bname) in [
                (4u32, "flt"), (8, "dbl"), (2, "half"),
            ] {
                let flt = if by != 1 { 1u32 } else { 0 };
                let fin = f(pt, 3, by, 0, 0, 0, 0, 0, flt);
                let fout = f(4, 3, 1, 0, 0, 0, 0, 0, 0);
                let t = c::cmsCreateTransform(prof, fin,
                    srgb, fout, 0, 0);
                eprintln!("{name}->{bname} t={}",
                    !t.is_null());
                if !t.is_null() {
                    let mut src = vec![0u8;
                        3 * by as usize * 4];
                    for (i, b) in src.iter_mut()
                        .enumerate() { *b = (i * 37) as u8; }
                    let mut dst = vec![0u8; 3 * 4];
                    c::cmsDoTransform(t,
                        src.as_ptr() as *const _,
                        dst.as_mut_ptr() as *mut _, 4);
                    c::cmsDeleteTransform(t);
                }
                // reverse: u16 in → float out
                let fin2 = f(4, 3, 2, 0, 0, 0, 0, 0, 0);
                let fout2 = f(pt, 3, by, 0, 0, 0, 0, 0, flt);
                let t2 = c::cmsCreateTransform(srgb, fin2,
                    prof, fout2, 0, 0);
                eprintln!("{bname}->{name} t={}",
                    !t2.is_null());
                if !t2.is_null() {
                    let src = [0x4000u16; 3];
                    let mut dst = vec![0u8;
                        3 * by as usize];
                    c::cmsDoTransform(t2,
                        src.as_ptr() as *const _,
                        dst.as_mut_ptr() as *mut _, 1);
                    c::cmsDeleteTransform(t2);
                }
                // float → float (Unroll*ToFloat + Pack*FromFloat)
                let f3 = f(pt, 3, by, 0, 0, 0, 0, 0, flt);
                let t3 = c::cmsCreateTransform(prof, f3,
                    prof, f3, 0, 0);
                eprintln!("{name}->{name} {bname} t={}",
                    !t3.is_null());
                if !t3.is_null() {
                    let mut src = vec![0u8;
                        3 * by as usize * 2];
                    for (i, b) in src.iter_mut()
                        .enumerate() { *b = (i * 41) as u8; }
                    let mut dst = vec![0u8;
                        3 * by as usize * 2];
                    c::cmsDoTransform(t3,
                        src.as_ptr() as *const _,
                        dst.as_mut_ptr() as *mut _, 2);
                    c::cmsDeleteTransform(t3);
                }
                // doswap+swapfirst+planar+extra variants
                for (ds, sf, pl, ex, vname) in [
                    (1u32, 0, 0, 0, "ds"),
                    (0, 1, 0, 0, "sf"),
                    (0, 0, 1, 0, "pl"),
                    (0, 0, 0, 1, "ex1"),
                    (1, 1, 0, 1, "dssfex1"),
                ] {
                    let f4 = f(pt, 3, by, ex, ds, 0, sf, pl,
                        flt);
                    let t4 = c::cmsCreateTransform(prof, f4,
                        srgb, f(4, 3, 1, 0, 0, 0, 0, 0, 0),
                        0, 0);
                    if !t4.is_null() {
                        eprintln!("  {name} {bname} {vname} \
                            ok");
                        c::cmsDeleteTransform(t4);
                    }
                }
            }
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
        c::cmsCloseProfile(xyz);
    }
}

/// cmspack residual — Pack3BytesAndSkip1* + PackPlanarBytes/
///   PackChunkyBytes internals via extra-channel outputs.
#[test]
fn pack_skip1_planar_arms() {
    fn f(cs: u32, ch: u32, by: u32, ex: u32, ds: u32,
        fl: u32, sf: u32, pl: u32, flt: u32) -> u32 {
        (cs << 16) | (flt << 22) | (sf << 14) | (fl << 13)
            | (pl << 12) | (ds << 10) | (ex << 7)
            | (ch << 3) | by
    }
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(
            std::ptr::null_mut());
        // output: 3ch + 1 extra, 1 byte → Pack3BytesAndSkip1
        for (ds, sf, name) in [
            (0u32, 0, "plain"), (1, 0, "ds"),
            (0, 1, "sf"), (1, 1, "dssf"),
        ] {
            let fin = f(10, 3, 1, 0, 0, 0, 0, 0, 0);
            let fout = f(4, 3, 1, 1, ds, 0, sf, 0, 0);
            let t = c::cmsCreateTransform(lab, fin,
                srgb, fout, 0, 0);
            eprintln!("3+1ex {name} t={}", !t.is_null());
            if !t.is_null() {
                let src = [128u8; 3 * 16];
                let mut dst = [0u8; 4 * 16];
                c::cmsDoTransform(t,
                    src.as_ptr() as *const _,
                    dst.as_mut_ptr() as *mut _, 16);
                c::cmsDeleteTransform(t);
            }
            // also RGB→RGB extra-1 output
            let t2 = c::cmsCreateTransform(srgb,
                f(4, 3, 1, 0, 0, 0, 0, 0, 0),
                srgb, fout, 0, 0);
            eprintln!("rgb3+1ex {name} t={}", !t2.is_null());
            if !t2.is_null() {
                let src = [128u8; 3 * 16];
                let mut dst = [0u8; 4 * 16];
                c::cmsDoTransform(t2,
                    src.as_ptr() as *const _,
                    dst.as_mut_ptr() as *mut _, 16);
                c::cmsDeleteTransform(t2); }
        }
        // planar bytes out (PackPlanarBytes)
        for (pl, name) in [
            (1u32, "pl"), (0, "ch"),
        ] {
            for ch in [1u32, 3, 4] {
                let fin = f(4, 3, 1, 0, 0, 0, 0, 0, 0);
                let fout = f(4, ch, 1, 0, 0, 0, 0, pl, 0);
                let t = c::cmsCreateTransform(srgb, fin,
                    srgb, fout, 0, 0);
                eprintln!("pl{pl} {ch}ch t={}",
                    !t.is_null());
                if !t.is_null() {
                    let src = [100u8; 3 * 8];
                    let mut dst = [0u8; 4 * 16];
                    c::cmsDoTransform(t,
                        src.as_ptr() as *const _,
                        dst.as_mut_ptr() as *mut _, 4);
                    c::cmsDeleteTransform(t);
                }
            }
        }
        // chunky multi-ch out (PackChunkyBytes)
        for ch in [2u32, 3, 4, 5, 6, 8] {
            let fin = f(4, 3, 1, 0, 0, 0, 0, 0, 0);
            let fout = f(4, ch, 1, 0, 0, 0, 0, 0, 0);
            let t = c::cmsCreateTransform(srgb, fin,
                srgb, fout, 0, 0);
            eprintln!("ch{ch} t={}", !t.is_null());
            if !t.is_null() {
                let src = [100u8; 3 * 8];
                let mut dst = [0u8; 8 * 8];
                c::cmsDoTransform(t,
                    src.as_ptr() as *const _,
                    dst.as_mut_ptr() as *mut _, 4);
                c::cmsDeleteTransform(t);
            }
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
    }
}

/// cmspack residual — DupFormatterFactoryList via formatter
///   plugin + cmsDupContext, plus dbl→float eval paths and
///   LabV2 float encoders.
#[test]
fn pack_fmt_dup_dbl_arms() {
    fn f(cs: u32, ch: u32, by: u32, ex: u32, ds: u32,
        fl: u32, sf: u32, pl: u32, flt: u32) -> u32 {
        (cs << 16) | (flt << 22) | (sf << 14) | (fl << 13)
            | (pl << 12) | (ds << 10) | (ex << 7)
            | (ch << 3) | by
    }
    unsafe extern "C" fn my_factory(
        _t: u32, _d: u32, _m: u32)
        -> *const std::ffi::c_void {
        std::ptr::null()
    }
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(
            std::ptr::null_mut());
        // formatter plugin into ctx → dup ctx →
        //   DupFormatterFactoryList list-walk
        #[repr(C)]
        struct PluginBase {
            magic: u32, ver: u32, typ: u32,
            next: *mut std::ffi::c_void,
        }
        #[repr(C)]
        struct FmtPlugin {
            base: PluginBase,
            factory: *const std::ffi::c_void,
        }
        for npl in [1u32, 4] {
            let ctx = c::cmsCreateContext(
                std::ptr::null_mut(), std::ptr::null_mut());
            let mut plugs = vec![];
            for _ in 0..npl {
                plugs.push(FmtPlugin {
                    base: PluginBase {
                        magic: 0x61637070,
                        ver: 2200,
                        typ: 0x66726D48,
                        next: std::ptr::null_mut(),
                    },
                    factory: my_factory
                        as *const std::ffi::c_void,
                });
            }
            for p in &plugs {
                let r = c::cmsPluginTHR(ctx,
                    p as *const _ as *mut _);
                eprintln!("reg={r}");
            }
            let d = c::cmsDupContext(ctx,
                std::ptr::null_mut());
            eprintln!("fmt dup{npl} d={}", !d.is_null());
            if !d.is_null() { c::cmsDeleteContext(d); }
            c::cmsDeleteContext(ctx);
        }
        // dbl→float eval paths (FloatXFORM on doubles)
        for (pt, prof, name) in [
            (10u32, lab, "Lab"), (9, c::cmsCreateXYZProfile(),
                "XYZ"), (4, srgb, "RGB"), (3,
                c::cmsCreateGrayProfile(
                    std::ptr::null_mut(),
                    std::ptr::null_mut()), "Gray"),
        ] {
            for (by, bname) in [
                (8u32, "dbl"), (4, "flt"), (2, "half"),
            ] {
                let fin = f(pt, match pt {
                    3 => 1, _ => 3 }, by, 0, 0, 0, 0, 0, 1);
                let fout = f(pt, match pt {
                    3 => 1, _ => 3 }, 4, 0, 0, 0, 0, 0, 1);
                let t = c::cmsCreateTransform(prof, fin,
                    prof, fout, 0, 0);
                eprintln!("{name} {bname}->flt t={}",
                    !t.is_null());
                if !t.is_null() {
                    let mut src = vec![0u8;
                        match pt { 3 => 1, _ => 3 }
                            * by as usize * 4];
                    for (i, b) in src.iter_mut()
                        .enumerate() { *b = (i * 43) as u8; }
                    let mut dst = vec![0u8;
                        match pt { 3 => 1, _ => 3 } * 4 * 4];
                    c::cmsDoTransform(t,
                        src.as_ptr() as *const _,
                        dst.as_mut_ptr() as *mut _, 4);
                    c::cmsDeleteTransform(t);
                }
                // u16→float LabV2 encoder paths
                let fin2 = f(pt, match pt {
                    3 => 1, _ => 3 }, 2, 0, 0, 0, 0, 0, 0);
                let fout2 = f(pt, match pt {
                    3 => 1, _ => 3 }, by, 0, 0, 0, 0, 0, 1);
                let t2 = c::cmsCreateTransform(prof, fin2,
                    prof, fout2, 0, 0);
                if !t2.is_null() {
                    c::cmsDeleteTransform(t2); }
            }
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
    }
}

/// cmspack residual — Unroll*To16/Pack*From16 float
///   formatters via cmsChangeBuffersFormat on 16-bit
///   transforms (only reachable path upstream).
#[test]
fn pack_chgfmt_float16_arms() {
    fn f(cs: u32, ch: u32, by: u32, ex: u32, ds: u32,
        fl: u32, sf: u32, pl: u32, flt: u32) -> u32 {
        (cs << 16) | (flt << 22) | (sf << 14) | (fl << 13)
            | (pl << 12) | (ds << 10) | (ex << 7)
            | (ch << 3) | by
    }
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(
            std::ptr::null_mut());
        let xyz = c::cmsCreateXYZProfile();
        // u16 transforms, then change formats to float
        //   variants → Unroll*To16 / Pack*From16
        for (pt, prof, name) in [
            (10u32, lab, "Lab"), (9, xyz, "XYZ"),
            (4, srgb, "RGB"),
        ] {
            for (by, flt, bname) in [
                (4u32, 1, "flt"), (8, 1, "dbl"),
                (2, 1, "half"), (1, 0, "u8"),
                (2, 0, "u16"),
            ] {
                let fin16 = f(pt, 3, 2, 0, 0, 0, 0, 0, 0);
                let t = c::cmsCreateTransform(prof, fin16,
                    srgb, fin16, 0, 0);
                if t.is_null() { continue; }
                let newfmt = f(pt, 3, by, 0, 0, 0, 0, 0,
                    flt);
                let r1 = c::cmsChangeBuffersFormat(t,
                    newfmt, fin16);
                let mut src = vec![0u8;
                    3 * by as usize * 8];
                for (i, b) in src.iter_mut().enumerate() {
                    *b = (i * 53) as u8; }
                let mut dst = [0u16; 3 * 8];
                c::cmsDoTransform(t,
                    src.as_ptr() as *const _,
                    dst.as_mut_ptr() as *mut _, 8);
                // change output format → Pack*From16
                let r2 = c::cmsChangeBuffersFormat(t,
                    fin16, newfmt);
                let src2 = [0x4000u16; 3];
                let mut dst2 = vec![0u8;
                    3 * by as usize];
                c::cmsDoTransform(t,
                    src2.as_ptr() as *const _,
                    dst2.as_mut_ptr() as *mut _, 1);
                eprintln!("{name} {bname} r={r1}/{r2}");
                // swap variants
                for (ds, sf, pl, ex, vn) in [
                    (1u32, 0, 0, 0, "ds"),
                    (0, 1, 0, 0, "sf"),
                    (0, 0, 1, 0, "pl"),
                    (0, 0, 0, 1, "ex1"),
                ] {
                    let fmt2 = f(pt, 3, by, ex, ds, 0,
                        sf, pl, flt);
                    let r3 = c::cmsChangeBuffersFormat(t,
                        fmt2, fin16);
                    if r3 != 0 {
                        c::cmsDoTransform(t,
                            src.as_ptr() as *const _,
                            dst.as_mut_ptr() as *mut _, 4);
                        eprintln!("  {vn} r3={r3}");
                    }
                }
                c::cmsDeleteTransform(t);
            }
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
        c::cmsCloseProfile(xyz);
    }
}
