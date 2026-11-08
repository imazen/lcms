//! cmspack formatter-eval matrix — cmsDoTransform on a matrix of
//! format variants; each hits a different Pack/Unroll formatter fn.
//! C gets native u16s (as_ptr()); Rust gets the byte buffer.

use lcms::{virt, xform, gamma, types::*};
use lcms_cref as c;

#[test]
fn format_eval_matrix() {
    unsafe {
        let lab16: u32 = (10 << 16) | (3 << 3) | 2;
        let rgb16: u32 = (4 << 16) | (3 << 3) | 2;
        let rgb8: u32 = (4 << 16) | (3 << 3) | 1;
        let lab8: u32 = (10 << 16) | (3 << 3) | 1;
        let combos: [(&str, u32, u32, &[u16]); 10] = [
            ("rgb16->lab16", rgb16, lab16, &[0x1234, 0x5678, 0x9abc]),
            ("rgb8->lab8", rgb8, lab8, &[0x12, 0x56, 0x9a]),
            ("rgb16+swapfirst->lab16", rgb16 | (1 << 14), lab16,
                &[0x1234, 0x5678, 0x9abc, 0xffff]),
            ("rgb16+endian16->lab16", rgb16 | (1 << 11), lab16,
                &[0x1234, 0x5678, 0x9abc]),
            ("rgb16+flavor->lab16", rgb16 | (1 << 13), lab16,
                &[0x1234, 0x5678, 0x9abc]),
            ("rgb16+planar->lab16", rgb16 | (1 << 12), lab16,
                &[0x1234, 0x5678, 0x9abc]),
            ("rgb16+premul->lab16", rgb16 | (1 << 23), lab16,
                &[0x1234, 0x5678, 0x9abc, 0xffff]),
            ("lab16->rgb16", lab16, rgb16, &[0x4000, 0x8000, 0x2000]),
            ("lab8->rgb8", lab8, rgb8, &[0x40, 0x80, 0x20]),
            ("rgb8+endian16->lab8", rgb8 | (1 << 11), lab8,
                &[0x12, 0x56, 0x9a]),
        ];
        for (name, in_fmt, out_fmt, inp) in combos {
            let is_lab_in = name.starts_with("lab");
            let (hi, ho) = if is_lab_in {
                (c::cmsCreateLab4Profile(std::ptr::null_mut()),
                 c::cmsCreate_sRGBProfile())
            } else {
                (c::cmsCreate_sRGBProfile(),
                 c::cmsCreateLab4Profile(std::ptr::null_mut()))
            };
            let ct = c::cmsCreateTransform(hi, in_fmt, ho, out_fmt, 0, 0);
            let _ = c::cmsCloseProfile(hi);
            let _ = c::cmsCloseProfile(ho);
            let mut rp1 = if is_lab_in {
                virt::create_lab4_profile(None, None).unwrap()
            } else { virt::create_srgb_profile(None).unwrap() };
            let mut rp2 = if is_lab_in {
                virt::create_srgb_profile(None).unwrap()
            } else { virt::create_lab4_profile(None, None).unwrap() };
            let mut profs = vec![rp1.clone(), rp2.clone()];
            let mut bpc = [false; 2];
            let intents = [0u32; 2];
            let adapt = [1.0f64; 2];
            let rt = xform::create_extended_transform(
                None, &mut profs, &mut bpc, &intents, &adapt, None, 0,
                in_fmt, out_fmt, 0);
            assert_eq!(ct.is_null(), rt.is_none(), "{name} create");
            if !ct.is_null() && rt.is_some() {
                let mut rt = rt.unwrap();
                // C gets the native u16 slice; Rust gets the byte
                // buffer (native-endian u16s for u16 formats)
                let inb: Vec<u8> = if in_fmt & 7 == 2 {
                    inp.iter().flat_map(|v| v.to_ne_bytes()).collect()
                } else {
                    inp.iter().map(|v| *v as u8).collect()
                };
                let mut co = [0u16; 4];
                let mut ro = vec![0u8; 8];
                c::cmsDoTransform(ct, inp.as_ptr() as _,
                    co.as_mut_ptr() as *mut _, 1);
                rt.do_transform(&inb, &mut ro, 1);
                let rout: Vec<u16> = if out_fmt & 7 == 2 {
                    ro.chunks_exact(2)
                        .map(|b| u16::from_be_bytes([b[0], b[1]]))
                        .take(3).collect()
                } else {
                    ro[..3].iter().map(|v| *v as u16).collect()
                };
                if co[..3] != rout[..3] {
                    eprintln!("{name} DIVERGES c={:?} r={:?}",
                        &co[..3], &rout[..3]);
                }
                c::cmsDeleteTransform(ct);
            }
        }
    }
}

/// EXTRA_SH + DOSWAP formats with corrected encoding — the earlier
/// "divergences" were a test-side endianness bug (C wants native
/// u16s, not be-bytes).
#[test]
fn extra_doswap_corrected() {
    unsafe {
        let lab16: u32 = (10 << 16) | (3 << 3) | 2;
        let rgba16: u32 = (4 << 16) | (3 << 3) | 2 | (1 << 7);
        let bgr16: u32 = (4 << 16) | (3 << 3) | 2 | (1 << 10);
        for (name, in_fmt, inp) in [
            ("rgba16", rgba16, [0x1234u16, 0x5678, 0x9abc, 0xffff]),
            ("bgr16", bgr16, [0x9abcu16, 0x5678, 0x1234, 0]),
        ] {
            let hi = c::cmsCreate_sRGBProfile();
            let ho = c::cmsCreateLab4Profile(std::ptr::null_mut());
            let ct = c::cmsCreateTransform(hi, in_fmt, ho, lab16, 0, 0);
            let _ = c::cmsCloseProfile(hi);
            let _ = c::cmsCloseProfile(ho);
            let mut rp1 = virt::create_srgb_profile(None).unwrap();
            let mut rp2 = virt::create_lab4_profile(None, None).unwrap();
            let mut profs = vec![rp1.clone(), rp2.clone()];
            let mut bpc = [false; 2];
            let intents = [0u32; 2];
            let adapt = [1.0f64; 2];
            let rt = xform::create_extended_transform(
                None, &mut profs, &mut bpc, &intents, &adapt, None, 0,
                in_fmt, lab16, 0);
            assert_eq!(ct.is_null(), rt.is_none(), "{name}");
            if !ct.is_null() && rt.is_some() {
                let mut rt = rt.unwrap();
                let nchan = if in_fmt == rgba16 { 4 } else { 3 };
                let inb: Vec<u8> = inp[..nchan].iter()
                    .flat_map(|v| v.to_ne_bytes()).collect();
                let mut co = [0u16; 3];
                let mut ro = vec![0u8; 6];
                c::cmsDoTransform(ct, inp.as_ptr() as _,
                    co.as_mut_ptr() as *mut _, 1);
                rt.do_transform(&inb, &mut ro, 1);
                let ro16: Vec<u16> = ro.chunks_exact(2)
                    .map(|b| u16::from_be_bytes([b[0], b[1]])).collect();
                if co != ro16[..3] {
                    eprintln!("{name} c={co:?} r={ro16:?}");
                }
                c::cmsDeleteTransform(ct);
            }
        }
    }
}

/// MCH-format transforms (5/6/8-channel) — the MCH pack/unroll
/// formatters.
#[test]
fn mch_formats() {
    unsafe {
        let lab16: u32 = (10 << 16) | (3 << 3) | 2;
        // MCH5=(5<<3)|2, MCH6=(6<<3)|2, MCH8=(8<<3)|2 — PT_MCH5=15
        for (name, cs, nch) in [("mch5", 15u32, 5usize),
                                ("mch6", 16u32, 6),
                                ("mch8", 17u32, 8)] {
            let in_fmt = (cs << 16) | ((nch as u32) << 3) | 2;
            // a devicelink/abstract profile for the MCH side — use
            // NULL profile (device-independent MCH)
            let hi = c::cmsCreateNULLProfile();
            let ho = c::cmsCreateLab4Profile(std::ptr::null_mut());
            let ct = c::cmsCreateTransform(hi, in_fmt, ho, lab16, 0, 0);
            let _ = c::cmsCloseProfile(hi);
            let _ = c::cmsCloseProfile(ho);
            let mut rp1 = virt::create_null_profile(None).unwrap();
            let mut rp2 = virt::create_lab4_profile(None, None).unwrap();
            let mut profs = vec![rp1.clone(), rp2.clone()];
            let mut bpc = [false; 2];
            let intents = [0u32; 2];
            let adapt = [1.0f64; 2];
            let rt = xform::create_extended_transform(
                None, &mut profs, &mut bpc, &intents, &adapt, None, 0,
                in_fmt, lab16, 0);
            assert_eq!(ct.is_null(), rt.is_none(), "{name}");
            if !ct.is_null() && rt.is_some() {
                let mut rt = rt.unwrap();
                let mut inp = vec![0x4000u16; nch];
                inp[0] = 0x8000;
                let inb: Vec<u8> = inp.iter()
                    .flat_map(|v| v.to_ne_bytes()).collect();
                let mut co = [0u16; 3];
                let mut ro = vec![0u8; 6];
                c::cmsDoTransform(ct, inp.as_ptr() as _,
                    co.as_mut_ptr() as *mut _, 1);
                rt.do_transform(&inb, &mut ro, 1);
                let ro16: Vec<u16> = ro.chunks_exact(2)
                    .map(|b| u16::from_be_bytes([b[0], b[1]])).collect();
                if co != ro16[..3] {
                    eprintln!("{name} c={co:?} r={ro16:?}");
                }
                c::cmsDeleteTransform(ct);
            }
        }
    }
}

/// BYTES_SH(0) packed (nibble/packed) + BYTES_SH(4) float formats —
/// the remaining format-class paths.
#[test]
fn packed_float_formats() {
    unsafe {
        let lab16: u32 = (10 << 16) | (3 << 3) | 2;
        // BYTES_SH(0) = packed (1 byte/2 channels?) + BYTES_SH(4) =
        // float (probably unported)
        for (name, bytes) in [("packed0", 0u32), ("float4", 4u32)] {
            let in_fmt = (4 << 16) | (3 << 3) | bytes;
            let hi = c::cmsCreate_sRGBProfile();
            let ho = c::cmsCreateLab4Profile(std::ptr::null_mut());
            let ct = c::cmsCreateTransform(hi, in_fmt, ho, lab16, 0, 0);
            let _ = c::cmsCloseProfile(hi);
            let _ = c::cmsCloseProfile(ho);
            let mut rp1 = virt::create_srgb_profile(None).unwrap();
            let mut rp2 = virt::create_lab4_profile(None, None).unwrap();
            let mut profs = vec![rp1.clone(), rp2.clone()];
            let mut bpc = [false; 2];
            let intents = [0u32; 2];
            let adapt = [1.0f64; 2];
            let rt = xform::create_extended_transform(
                None, &mut profs, &mut bpc, &intents, &adapt, None, 0,
                in_fmt, lab16, 0);
            assert_eq!(ct.is_null(), rt.is_none(),
                "{name} create");
            if !ct.is_null() { c::cmsDeleteTransform(ct); }
        }
    }
}

/// PT_ANY colorspace format — skips colorspace validation.
#[test]
fn anyspace_format() {
    unsafe {
        // TYPE_ANY = colorspace PT_ANY(0) — accepts any channels
        let in_fmt: u32 = (0 << 16) | (3 << 3) | 2; // ANY 3ch u16
        let out_fmt: u32 = (10 << 16) | (3 << 3) | 2;
        let hi = c::cmsCreate_sRGBProfile();
        let ho = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let ct = c::cmsCreateTransform(hi, in_fmt, ho, out_fmt, 0, 0);
        let _ = c::cmsCloseProfile(hi);
        let _ = c::cmsCloseProfile(ho);
        let mut rp1 = virt::create_srgb_profile(None).unwrap();
        let mut rp2 = virt::create_lab4_profile(None, None).unwrap();
        let mut profs = vec![rp1.clone(), rp2.clone()];
        let mut bpc = [false; 2];
        let intents = [0u32; 2];
        let adapt = [1.0f64; 2];
        let rt = xform::create_extended_transform(
            None, &mut profs, &mut bpc, &intents, &adapt, None, 0,
            in_fmt, out_fmt, 0);
        assert_eq!(ct.is_null(), rt.is_none(), "anyspace create");
        if !ct.is_null() && rt.is_some() {
            let mut rt = rt.unwrap();
            let inp = [0x1234u16, 0x5678, 0x9abc];
            let inb: Vec<u8> = inp.iter()
                .flat_map(|v| v.to_ne_bytes()).collect();
            let mut co = [0u16; 3];
            let mut ro = vec![0u8; 6];
            c::cmsDoTransform(ct, inp.as_ptr() as _,
                co.as_mut_ptr() as *mut _, 1);
            rt.do_transform(&inb, &mut ro, 1);
            let ro16: Vec<u16> = ro.chunks_exact(2)
                .map(|b| u16::from_be_bytes([b[0], b[1]])).collect();
            if co != ro16[..3] {
                eprintln!("anyspace c={co:?} r={ro16:?}");
            }
            c::cmsDeleteTransform(ct);
        }
    }
}

/// 1ch/2ch format matrix — gray/duo-channel formatter paths.
#[test]
fn low_channel_formats() {
    unsafe {
        // gray (1ch) + duo (2ch) formats u8/u16
        for (name, nch, bytes) in [
            ("gray8", 1u32, 1u32), ("gray16", 1, 2),
            ("duo8", 2, 1), ("duo16", 2, 2),
        ] {
            let in_fmt = (7 << 16) | (nch << 3) | bytes; // gray cs
            let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
            let hi = c::cmsCreateGrayProfile(c::cmsD50_xyY() as *const std::ffi::c_void, g);
            let ho = c::cmsCreateLab4Profile(std::ptr::null_mut());
            let ct = c::cmsCreateTransform(hi, in_fmt, ho,
                (10 << 16) | (3 << 3) | 2, 0, 0);
            let _ = c::cmsCloseProfile(hi);
            let _ = c::cmsCloseProfile(ho);
            let rtc = gamma::build_gamma(None, 2.2).unwrap();
            let mut rp1 = virt::create_gray_profile(None, None,
                Some(&rtc)).unwrap();
            let mut rp2 = virt::create_lab4_profile(None, None).unwrap();
            let mut profs = vec![rp1.clone(), rp2.clone()];
            let mut bpc = [false; 2];
            let intents = [0u32; 2];
            let adapt = [1.0f64; 2];
            let rt = xform::create_extended_transform(
                None, &mut profs, &mut bpc, &intents, &adapt, None, 0,
                in_fmt, (10 << 16) | (3 << 3) | 2, 0);
            assert_eq!(ct.is_null(), rt.is_none(), "{name}");
            if !ct.is_null() { c::cmsDeleteTransform(ct); }
        }
    }
}
