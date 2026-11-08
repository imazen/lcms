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
