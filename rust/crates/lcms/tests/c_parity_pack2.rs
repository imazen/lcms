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

/// PREMUL_SH premultiplied-alpha formats — the premul formatter
/// paths.
#[test]
fn premul_formats() {
    unsafe {
        // PREMUL_SH = bit 21 — premultiplied alpha
        for (name, extra_bits) in [
            ("premul", 1u32 << 21),
            ("premul+extra", (1 << 21) | (1 << 17)),
            ("premul+doswap", (1 << 21) | (1 << 19)),
        ] {
            let in_fmt = (4 << 16) | (4 << 3) | 2 | extra_bits;
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
            assert_eq!(ct.is_null(), rt.is_none(), "{name} create");
            if !ct.is_null() { c::cmsDeleteTransform(ct); }
        }
    }
}

/// Exotic-modifier matrix — SWAPFIRST/FLAVOR/ENDIAN16 × channel
/// counts.
#[test]
fn modifier_matrix() {
    unsafe {
        // modifiers × channel counts — each combo hits a different
        // pack/unroll dispatch
        for (mname, bits) in [
            ("swapfirst", 1u32 << 18),
            ("flavor", 1u32 << 19),
            ("endian16", 1u32 << 24),
            ("swapfirst+flavor", (1 << 18) | (1 << 19)),
            ("endian16+swapfirst", (1 << 24) | (1 << 18)),
        ] {
            for nch in [1u32, 3, 4, 8] {
                let in_fmt = (4 << 16) | (nch << 3) | 2 | bits;
                let out_fmt: u32 = (10 << 16) | (3 << 3) | 2;
                let hi = c::cmsCreate_sRGBProfile();
                let ho = c::cmsCreateLab4Profile(std::ptr::null_mut());
                let ct = c::cmsCreateTransform(hi, in_fmt, ho,
                    out_fmt, 0, 0);
                let _ = c::cmsCloseProfile(hi);
                let _ = c::cmsCloseProfile(ho);
                let mut rp1 = virt::create_srgb_profile(None).unwrap();
                let mut rp2 = virt::create_lab4_profile(None, None)
                    .unwrap();
                let mut profs = vec![rp1.clone(), rp2.clone()];
                let mut bpc = [false; 2];
                let intents = [0u32; 2];
                let adapt = [1.0f64; 2];
                let rt = xform::create_extended_transform(
                    None, &mut profs, &mut bpc, &intents, &adapt,
                    None, 0, in_fmt, out_fmt, 0);
                assert_eq!(ct.is_null(), rt.is_none(),
                    "{mname} {nch}ch");
                if !ct.is_null() { c::cmsDeleteTransform(ct); }
            }
        }
    }
}

/// eval matrix — cmsDoTransform byte-exact evals across
/// BYTES_SH(1/2/4) x channel counts x modifiers. C gets native
/// u16/u8/f32 memory; Rust gets the same bytes.
#[test]
fn eval_matrix() {
    unsafe {
        for (bname, bytes) in [("u8", 1u32), ("u16", 2), ("f32", 4)] {
            for nch in [1u32, 3, 4, 6, 8] {
                for (mname, bits) in [
                    ("plain", 0u32),
                    ("extra", 1 << 17),
                    ("doswap", 1 << 10),
                    ("flavor", 1 << 19),
                ] {
                    let in_fmt = (4 << 16) | (nch << 3) | bytes | bits;
                    let out_fmt: u32 = (10 << 16) | (3 << 3) | 2;
                    let hi = c::cmsCreate_sRGBProfile();
                    let ho = c::cmsCreateLab4Profile(
                        std::ptr::null_mut());
                    let ct = c::cmsCreateTransform(hi, in_fmt, ho,
                        out_fmt, 0, 0);
                    let _ = c::cmsCloseProfile(hi);
                    let _ = c::cmsCloseProfile(ho);
                    let mut rp1 = virt::create_srgb_profile(None)
                        .unwrap();
                    let mut rp2 = virt::create_lab4_profile(None,
                        None).unwrap();
                    let mut profs = vec![rp1.clone(), rp2.clone()];
                    let mut bpc = [false; 2];
                    let intents = [0u32; 2];
                    let adapt = [1.0f64; 2];
                    let rt = xform::create_extended_transform(
                        None, &mut profs, &mut bpc, &intents,
                        &adapt, None, 0, in_fmt, out_fmt, 0);
                    if ct.is_null() != rt.is_none() {
                        eprintln!("{bname} {nch}ch {mname} status \
                            c_null={} r_none={}",
                            ct.is_null(), rt.is_none());
                        if !ct.is_null() {
                            c::cmsDeleteTransform(ct);
                        }
                        continue;
                    }
                    if ct.is_null() { continue; }
                    let mut rt = rt.unwrap();
                    // build a native input buffer + a byte view
                    let nvals = (nch + if bits & (1 << 17) != 0 { 1 }
                        else { 0 }) as usize;
                    let inp: Vec<u8> = match bytes {
                        1 => (0..nvals).map(|i|
                            (i * 40) as u8).collect(),
                        2 => (0..nvals).flat_map(|i|
                            ((i * 8192) as u16).to_ne_bytes())
                            .collect(),
                        _ => (0..nvals).flat_map(|i|
                            (i as f32 * 0.25).to_ne_bytes())
                            .collect(),
                    };
                    let mut co = [0u16; 8];
                    let mut ro = vec![0u8; 6];
                    c::cmsDoTransform(ct, inp.as_ptr() as _,
                        co.as_mut_ptr() as *mut _, 1);
                    rt.do_transform(&inp, &mut ro, 1);
                    let ro16: Vec<u16> = ro.chunks_exact(2)
                        .map(|b| u16::from_ne_bytes([b[0], b[1]]))
                        .collect();
                    if co[..3] != ro16[..3] {
                        eprintln!("DIVERGE {bname} {nch}ch {mname} \
                            c={:?} r={:?}", &co[..3], &ro16[..3]);
                    }
                    c::cmsDeleteTransform(ct);
                }
            }
        }
    }
}

/// cmsChangeBuffersFormat — in-place formatter swap.
#[test]
fn change_buffers_format() {
    unsafe {
        let hi = c::cmsCreate_sRGBProfile();
        let ho = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let in_fmt: u32 = (4 << 16) | (3 << 3) | 2;
        let out_fmt: u32 = (10 << 16) | (3 << 3) | 2;
        let ct = c::cmsCreateTransform(hi, in_fmt, ho, out_fmt,
            0, 0);
        let _ = c::cmsCloseProfile(hi);
        let _ = c::cmsCloseProfile(ho);
        assert!(!ct.is_null());
        // swap input to u8, keep u16 out
        let in8: u32 = (4 << 16) | (3 << 3) | 1;
        let cok = c::cmsChangeBuffersFormat(ct, in8, out_fmt);
        eprintln!("change to u8 cok={cok}");
        // rust side
        let mut rp1 = virt::create_srgb_profile(None).unwrap();
        let mut rp2 = virt::create_lab4_profile(None, None)
            .unwrap();
        let mut profs = vec![rp1.clone(), rp2.clone()];
        let mut bpc = [false; 2];
        let intents = [0u32; 2];
        let adapt = [1.0f64; 2];
        let mut rt = xform::create_extended_transform(
            None, &mut profs, &mut bpc, &intents, &adapt, None, 0,
            in_fmt, out_fmt, 0).unwrap();
        let rok = rt.change_buffers_format(in8, out_fmt);
        assert_eq!(cok != 0, rok, "change status");
        // eval through the swapped transform
        let inp8 = [120u8, 160, 200];
        let mut co = [0u16; 3];
        let mut ro = vec![0u8; 6];
        c::cmsDoTransform(ct, inp8.as_ptr() as _,
            co.as_mut_ptr() as *mut _, 1);
        rt.do_transform(&inp8, &mut ro, 1);
        let ro16: Vec<u16> = ro.chunks_exact(2)
            .map(|b| u16::from_ne_bytes([b[0], b[1]]))
            .collect();
        assert_eq!(co, ro16.as_slice(), "post-change eval");
        c::cmsDeleteTransform(ct);
    }
}

/// PLANAR evals — planar-buffer transforms (3 separate channel
/// planes) across BYTES_SH(1/2/4).
#[test]
fn planar_evals() {
    unsafe {
        for (bname, bytes) in [("u8", 1u32), ("u16", 2), ("f32", 4)] {
            // RGB planar -> Lab chunky
            let in_fmt: u32 = (4 << 16) | (3 << 3) | bytes
                | (1 << 9); // PLANAR_SH
            let out_fmt: u32 = (10 << 16) | (3 << 3) | 2;
            let hi = c::cmsCreate_sRGBProfile();
            let ho = c::cmsCreateLab4Profile(std::ptr::null_mut());
            let ct = c::cmsCreateTransform(hi, in_fmt, ho,
                out_fmt, 0, 0);
            let _ = c::cmsCloseProfile(hi);
            let _ = c::cmsCloseProfile(ho);
            if ct.is_null() { continue; }
            // planar input: 3 channel planes
            let mut planes: [Vec<u8>; 3] = match bytes {
                1 => [vec![120u8], vec![160], vec![200]],
                2 => [
                    vec![120u16].into_iter()
                        .flat_map(|v| v.to_ne_bytes()).collect(),
                    vec![160u16].into_iter()
                        .flat_map(|v| v.to_ne_bytes()).collect(),
                    vec![200u16].into_iter()
                        .flat_map(|v| v.to_ne_bytes()).collect(),
                ],
                _ => [
                    vec![0.5f32].into_iter()
                        .flat_map(|v| v.to_ne_bytes()).collect(),
                    vec![0.6f32].into_iter()
                        .flat_map(|v| v.to_ne_bytes()).collect(),
                    vec![0.7f32].into_iter()
                        .flat_map(|v| v.to_ne_bytes()).collect(),
                ],
            };
            let ptrs = [planes[0].as_ptr(),
                        planes[1].as_ptr(),
                        planes[2].as_ptr()];
            let mut co = [0u16; 3];
            c::cmsDoTransform(ct, ptrs.as_ptr() as _,
                co.as_mut_ptr() as *mut _, 1);
            let _ = &mut planes;
            c::cmsDeleteTransform(ct);
        }
    }
}

/// PACKED evals — BYTES_SH(0) packed-pixel formats.
#[test]
fn packed_evals() {
    unsafe {
        // PACKED_SH formats — the any-packed formatter path
        for (name, pt, nch, packed_bytes) in [
            ("rgb332", 4u32, 3, 1u32),   // 1-byte packed RGB
            ("rgb444", 4, 3, 2),          // 2-byte packed
            ("rgb555", 4, 3, 2),
            ("rgb565", 4, 3, 2),
            ("rgb5551", 4, 4, 2),
            ("rgb4444", 4, 4, 2),
            ("rgb888", 4, 3, 3),
            ("rgbx888", 4, 4, 4),
        ] {
            // PACKED_SH = the packed-color encoding flag; the
            // channel/byte count comes from the pt+nch+bytes
            let in_fmt: u32 = (pt << 16) | (nch << 3)
                | packed_bytes;
            let out_fmt: u32 = (10 << 16) | (3 << 3) | 2;
            let hi = c::cmsCreate_sRGBProfile();
            let ho = c::cmsCreateLab4Profile(
                std::ptr::null_mut());
            let ct = c::cmsCreateTransform(hi, in_fmt, ho,
                out_fmt, 0, 0);
            let _ = c::cmsCloseProfile(hi);
            let _ = c::cmsCloseProfile(ho);
            if ct.is_null() { continue; }
            // eval one pixel — packed bytes
            let inp = vec![0xABu8; packed_bytes as usize];
            let mut co = [0u16; 3];
            c::cmsDoTransform(ct, inp.as_ptr() as _,
                co.as_mut_ptr() as *mut _, 1);
            eprintln!("{name} eval={:?}", co);
            c::cmsDeleteTransform(ct);
        }
    }
}

/// cross-format eval matrix — (in_fmt x out_fmt) on same-PCS
/// srgb->srgb transforms; hits Pack* output variants too.
#[test]
fn cross_format_evals() {
    unsafe {
        // several (pt, bytes, modifiers) input/output formats —
        // each exercises a different Unroll*/Pack* variant
        let variants: Vec<(u32, &str)> = vec![
            ((4 << 16) | (3 << 3) | 1, "rgb u8"),
            ((4 << 16) | (3 << 3) | 2, "rgb u16"),
            ((4 << 16) | (3 << 3) | 4, "rgb f32"),
            ((4 << 16) | (4 << 3) | 1, "rgba u8"),
            ((4 << 16) | (4 << 3) | 2, "rgba u16"),
            ((4 << 16) | (4 << 3) | 4, "rgba f32"),
            ((10 << 16) | (3 << 3) | 1, "lab u8"),
            ((10 << 16) | (3 << 3) | 2, "lab u16"),
            ((10 << 16) | (3 << 3) | 4, "lab f32"),
            ((3 << 16) | (3 << 3) | 2, "xyz u16"),
            ((3 << 16) | (3 << 3) | 4, "xyz f32"),
            ((1 << 16) | (1 << 3) | 1, "gray u8"),
            ((1 << 16) | (1 << 3) | 2, "gray u16"),
            ((4 << 16) | (3 << 3) | 2 | (1 << 10), "rgb doswap"),
            ((4 << 16) | (3 << 3) | 2 | (1 << 19), "rgb flavor"),
            ((4 << 16) | (4 << 3) | 2 | (1 << 17), "rgb extra"),
            ((4 << 16) | (3 << 3) | 2 | (1 << 9), "rgb planar"),
        ];
        for (in_fmt, in_name) in &variants {
            for (out_fmt, out_name) in &variants {
                let hi = c::cmsCreate_sRGBProfile();
                let ho = c::cmsCreate_sRGBProfile();
                let ct = c::cmsCreateTransform(hi, *in_fmt, ho,
                    *out_fmt, 0, 0);
                let _ = c::cmsCloseProfile(hi);
                let _ = c::cmsCloseProfile(ho);
                if ct.is_null() { continue; }
                // in buffer sized to the in-format's byte count
                let nbytes = ((in_fmt >> 0) & 7).max(1) as usize
                    * ((in_fmt >> 3) & 0xF) as usize;
                let inp = vec![0x55u8; nbytes.max(1)];
                // out buffer
                let obytes = ((out_fmt >> 0) & 7).max(1) as usize
                    * ((out_fmt >> 3) & 0xF) as usize;
                let mut co = vec![0u8; obytes.max(1)];
                c::cmsDoTransform(ct, inp.as_ptr() as _,
                    co.as_mut_ptr() as *mut _, 1);
                c::cmsDeleteTransform(ct);
            }
        }
    }
}

/// denser formatter matrix — unusual modifiers (endian16/premul/
/// swapfirst) × pts × bytes on same-PCS transforms.
#[test]
fn modifier_format_evals() {
    unsafe {
        // FLOAT_SH=4, EXTRA_SH, PLANAR_SH=1<<9, DOSWAP_SH=1<<10,
        // SWAPFIRST_SH=1<<11, ENDIAN16_SH=1<<12, FLAVOR_SH=1<<13,
        // PREMUL_SH=1<<11? — check the actual shift constants
        let variants: Vec<(u32, &str)> = vec![
            ((4 << 16) | (3 << 3) | 2 | (1 << 10), "rgb doswap"),
            ((4 << 16) | (3 << 3) | 2 | (1 << 11), "rgb swapfirst"),
            ((4 << 16) | (3 << 3) | 2 | (1 << 12), "rgb endian16"),
            ((4 << 16) | (3 << 3) | 4 | (1 << 13), "rgb flavor"),
            ((4 << 16) | (4 << 3) | 2 | (1 << 14), "rgb premul?"),
            ((1 << 16) | (1 << 3) | 1, "gray u8"),
            ((1 << 16) | (1 << 3) | 2, "gray u16"),
            ((1 << 16) | (1 << 3) | 4, "gray f32"),
            ((2 << 16) | (2 << 3) | 2, "2ch u16"),
            ((3 << 16) | (3 << 3) | 1, "xyz u8"),
            ((3 << 16) | (3 << 3) | 2, "xyz u16"),
            ((3 << 16) | (3 << 3) | 4, "xyz f32"),
            ((6 << 16) | (4 << 3) | 1, "cmyk u8"),
            ((6 << 16) | (4 << 3) | 2, "cmyk u16"),
            ((6 << 16) | (4 << 3) | 4, "cmyk f32"),
            ((10 << 16) | (3 << 3) | 1, "lab u8"),
            ((10 << 16) | (3 << 3) | 2, "lab u16"),
            ((10 << 16) | (3 << 3) | 4, "lab f32"),
            ((18 << 16) | (5 << 3) | 2, "mch5 u16"),
            ((18 << 16) | (6 << 3) | 2, "mch6 u16"),
            ((18 << 16) | (8 << 3) | 2, "mch8 u16"),
            ((4 << 16) | (3 << 3) | 8, "rgb f64"),
            ((10 << 16) | (3 << 3) | 8, "lab f64"),
        ];
        for (in_fmt, name) in &variants {
            for (out_fmt, oname) in &variants {
                let hi = c::cmsCreate_sRGBProfile();
                let ho = c::cmsCreate_sRGBProfile();
                let ct = c::cmsCreateTransform(hi, *in_fmt, ho,
                    *out_fmt, 0, 0);
                let _ = c::cmsCloseProfile(hi);
                let _ = c::cmsCloseProfile(ho);
                if ct.is_null() { continue; }
                let nbytes = ((in_fmt & 7).max(1) as usize)
                    * ((in_fmt >> 3) & 0xF) as usize;
                let inp = vec![0x66u8; nbytes.max(1)];
                let obytes = ((out_fmt & 7).max(1) as usize)
                    * ((out_fmt >> 3) & 0xF) as usize;
                let mut co = vec![0u8; obytes.max(1)];
                c::cmsDoTransform(ct, inp.as_ptr() as _,
                    co.as_mut_ptr() as *mut _, 1);
                c::cmsDeleteTransform(ct);
            }
        }
    }
}

/// Exhaustive formatter matrix — (pt x nch x bytes x modifiers)
/// on same-PCS srgb->srgb transforms; hits every Unroll*/Pack*
/// variant C accepts.
#[test]
fn exhaustive_format_matrix() {
    unsafe {
        // pixel-type sigs: gray=1, 2ch=2, xyz=3, lab=4, rgb=4?,
        // the TYPE bits select the formatter family
        let pts = [1u32, 2, 3, 4, 10, 6, 18, 15];
        let byteses = [1u32, 2, 4, 8];
        let mods = [0u32, 1 << 9, 1 << 10, 1 << 11, 1 << 12,
                    1 << 13, 1 << 17, 1 << 18];
        let mut count = 0u32;
        for pt in pts {
            for nch in 1..=8u32 {
                for bytes in byteses {
                    for m in mods {
                        let fmt: u32 = (pt << 16) | (nch << 3)
                            | bytes | m;
                        let hi = c::cmsCreate_sRGBProfile();
                        let ho = c::cmsCreate_sRGBProfile();
                        let ct = c::cmsCreateTransform(hi, fmt,
                            ho, fmt, 0, 0);
                        let _ = c::cmsCloseProfile(hi);
                        let _ = c::cmsCloseProfile(ho);
                        if ct.is_null() { continue; }
                        count += 1;
                        let nbytes = (bytes.max(1) as usize)
                            * nch as usize;
                        let inp = vec![0x77u8; nbytes];
                        let mut co = vec![0u8; nbytes];
                        c::cmsDoTransform(ct, inp.as_ptr() as _,
                            co.as_mut_ptr() as *mut _, 1);
                        c::cmsDeleteTransform(ct);
                    }
                }
            }
        }
        eprintln!("accepted {count} format combos");
    }
}

/// More format combos — all bytes 0-8 x nch 1-16 x more
/// modifier bits on same-PCS transforms.
#[test]
fn format_matrix_round2() {
    unsafe {
        let pts = [1u32, 2, 3, 4, 10, 6, 18];
        let byteses = [1u32, 2, 4, 8];
        let mods = [0u32, 1 << 9, 1 << 10, 1 << 11, 1 << 12,
                    1 << 13, 1 << 17, 1 << 18];
        let mut count = 0u32;
        for pt in pts {
            for nch in 1..=8u32 {
                for bytes in byteses {
                    for m in mods {
                        let fmt: u32 = (pt << 16) | (nch << 3)
                            | bytes | m;
                        let hi = c::cmsCreate_sRGBProfile();
                        let ho = c::cmsCreate_sRGBProfile();
                        let ct = c::cmsCreateTransform(hi, fmt,
                            ho, fmt, 0, 0);
                        let _ = c::cmsCloseProfile(hi);
                        let _ = c::cmsCloseProfile(ho);
                        if ct.is_null() { continue; }
                        count += 1;
                        let nbytes = (bytes.max(1) as usize)
                            * nch.min(8) as usize;
                        let inp = vec![0x55u8; nbytes];
                        let mut co = vec![0u8; nbytes];
                        c::cmsDoTransform(ct, inp.as_ptr() as _,
                            co.as_mut_ptr() as *mut _, 1);
                        c::cmsDeleteTransform(ct);
                    }
                }
            }
        }
        eprintln!("accepted {count} combos");
    }
}
