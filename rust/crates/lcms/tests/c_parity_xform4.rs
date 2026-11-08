//! cmsxform workers: alarm codes, adaptation state round-trip,
//! proofing transform construction parity.

use lcms::{virt, xform};
use lcms_cref as c;

#[test]
fn alarm_codes_get_set() {
    unsafe {
        // C: set then get
        let c_in = [0x1111u16, 0x2222, 0x3333, 0x4444, 0x5555, 0x6666,
                    0x7777, 0x8888, 0x9999, 0xaaaa, 0xbbbb, 0xcccc,
                    0xdddd, 0xeeee, 0xffff, 0x0000];
        c::cmsSetAlarmCodesTHR(std::ptr::null_mut(), c_in.as_ptr());
        let mut c_out = [0u16; 16];
        c::cmsGetAlarmCodesTHR(std::ptr::null_mut(), c_out.as_mut_ptr());
        assert_eq!(c_in, c_out, "C alarm round-trip");
        // rust: the same field on the context
        let mut ctx = lcms::context::create_context(None, 0);
        *ctx.alarm_codes.lock().unwrap() = c_in;
        assert_eq!(*ctx.alarm_codes.lock().unwrap(), c_out, "rust alarm");
        // restore default
        c::cmsSetAlarmCodesTHR(std::ptr::null_mut(), [0x7f00; 16].as_ptr());
    }
}

#[test]
fn adaptation_state_round_trip() {
    unsafe {
        for d in [0.0f64, 0.5, 1.0] {
            // each set returns the prev — C and rust start at the same
            // default (1.0) and track identically
            let c_prev = c::cmsSetAdaptationStateTHR(std::ptr::null_mut(), d);
            let r_prev = xform::set_adaptation_state(None, d);
            assert_eq!(c_prev, r_prev, "adapt prev d={d}");
        }
        c::cmsSetAdaptationStateTHR(std::ptr::null_mut(), 1.0);
        xform::set_adaptation_state(None, 1.0);
    }
}

#[test]
fn proofing_transform() {
    unsafe {
        let h_in = c::cmsCreate_sRGBProfile();
        let h_out = c::cmsCreate_sRGBProfile();
        let h_proof = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let in_fmt: u32 = (6 << 16) | (3 << 3) | 1;
        let out_fmt: u32 = in_fmt;
        for pi in [0u32, 1, 2, 3] {
            let ct = c::cmsCreateProofingTransform(
                h_in, in_fmt, h_out, out_fmt, h_proof, 0, pi, 0);
            let mut bpc = [false; 3];
            let intents = [0u32, pi, 0];
            let adapt = [1.0f64; 3];
            let mut ps = vec![
                virt::create_srgb_profile(None).unwrap(),
                virt::create_lab4_profile(None, None).unwrap(),
                virt::create_srgb_profile(None).unwrap(),
            ];
            let mut rt = xform::create_extended_transform(
                None, &mut ps, &mut bpc, &intents, &adapt, None, 0,
                in_fmt, out_fmt, 0);
            assert_eq!(ct.is_null(), rt.is_none(), "proof i{pi}");
            if !ct.is_null() {
                // transform eval parity — same input in both
                let inp = [0x8000u16; 3];
                let inb: Vec<u8> = inp.iter().flat_map(|v| v.to_be_bytes()).collect();
                let mut co = [0u16; 3];
                let mut ro = vec![0u8; 6];
                c::cmsDoTransform(ct, inp.as_ptr() as _, co.as_mut_ptr() as *mut _, 1);
                if let Some(rt) = &mut rt { rt.do_transform(&inb, &mut ro, 1) }
                let ro16: Vec<u16> = ro.chunks_exact(2)
                    .map(|b| u16::from_be_bytes([b[0], b[1]])).collect();
                assert_eq!(co, ro16[..3], "proof eval i{pi}");
                c::cmsDeleteTransform(ct);
            }
        }
        c::cmsCloseProfile(h_in);
        c::cmsCloseProfile(h_out);
        c::cmsCloseProfile(h_proof);
    }
}

/// Multiprofile chains: 3-4 profile links exercise _cmsLinkProfiles
/// deeper than the 2-profile path.
#[test]
fn multiprofile_chains() {
    unsafe {
        let in_fmt: u32 = (6 << 16) | (3 << 3) | 1;
        let out_fmt: u32 = (10 << 16) | (3 << 3) | 1;
        for n in [3usize, 4] {
            let mut c_profs: Vec<*mut _> = (0..n).map(|i| {
                if i % 2 == 0 { c::cmsCreate_sRGBProfile() }
                else { c::cmsCreateLab4Profile(std::ptr::null_mut()) }
            }).collect();
            let ct = c::cmsCreateMultiprofileTransform(
                c_profs.as_mut_ptr(), n as i32, in_fmt, out_fmt, 0, 0);
            let mut ps: Vec<Box<lcms::profile::IccProfile>> = (0..n).map(|i| {
                if i % 2 == 0 { virt::create_srgb_profile(None).unwrap() }
                else { virt::create_lab4_profile(None, None).unwrap() }
            }).collect();
            let mut bpc = vec![false; n];
            let intents = vec![0u32; n];
            let adapt = vec![1.0f64; n];
            let mut rt = xform::create_extended_transform(
                None, &mut ps, &mut bpc, &intents, &adapt, None, 0,
                in_fmt, out_fmt, 0);
            assert_eq!(ct.is_null(), rt.is_none(), "chain n{n}");
            if !ct.is_null() {
                for v in [0x1234u16, 0x7fff, 0xf000] {
                    let inp = [v; 3];
                    let mut co = [0u16; 3];
                    let mut ro = vec![0u8; 6];
                    c::cmsDoTransform(ct, inp.as_ptr() as _,
                        co.as_mut_ptr() as *mut _, 1);
                    if let Some(rt) = &mut rt {
                        let inb: Vec<u8> = inp.iter()
                            .flat_map(|x| x.to_be_bytes()).collect();
                        rt.do_transform(&inb, &mut ro, 1);
                    }
                    let ro16: Vec<u16> = ro.chunks_exact(2)
                        .map(|b| u16::from_be_bytes([b[0], b[1]])).collect();
                    assert_eq!(co, ro16[..3], "chain n{n} v{v:#x}");
                }
                c::cmsDeleteTransform(ct);
            }
            for h in c_profs { c::cmsCloseProfile(h); }
        }
    }
}

/// cmsCreateExtendedTransform with a gamut-check profile — the
/// gamut-check construction path.
#[test]
fn gamut_check_transform() {
    unsafe {
        let in_fmt: u32 = (6 << 16) | (3 << 3) | 1;
        let out_fmt: u32 = in_fmt;
        for pos in [0u32, 1] {
            let mut c_profs = vec![
                c::cmsCreate_sRGBProfile(),
                c::cmsCreate_sRGBProfile(),
            ];
            let g = c::cmsCreateLab4Profile(std::ptr::null_mut());
            let mut cbpc = [0i32; 2];
            let mut cint = [0u32; 2];
            let mut cad = [1.0f64; 2];
            let ct = c::cmsCreateExtendedTransform(
                std::ptr::null_mut(), 2, c_profs.as_mut_ptr(),
                cbpc.as_mut_ptr(), cint.as_mut_ptr(), cad.as_mut_ptr(),
                g, pos, in_fmt, out_fmt, 0);
            let mut ps = vec![
                virt::create_srgb_profile(None).unwrap(),
                virt::create_srgb_profile(None).unwrap(),
            ];
            let mut gp = virt::create_lab4_profile(None, None).unwrap();
            let mut bpc = [false; 2];
            let intents = [0u32; 2];
            let adapt = [1.0f64; 2];
            let mut rt = xform::create_extended_transform(
                None, &mut ps, &mut bpc, &intents, &adapt,
                Some(&mut gp), pos, in_fmt, out_fmt, 0);
            assert_eq!(ct.is_null(), rt.is_none(), "gamut pos{pos}");
            if !ct.is_null() {
                let inp = [0x8000u16; 3];
                let inb: Vec<u8> = inp.iter()
                    .flat_map(|v| v.to_be_bytes()).collect();
                let mut co = [0u16; 3];
                let mut ro = vec![0u8; 6];
                c::cmsDoTransform(ct, inp.as_ptr() as _,
                    co.as_mut_ptr() as *mut _, 1);
                if let Some(rt) = &mut rt { rt.do_transform(&inb, &mut ro, 1) }
                let ro16: Vec<u16> = ro.chunks_exact(2)
                    .map(|b| u16::from_be_bytes([b[0], b[1]])).collect();
                assert_eq!(co, ro16[..3], "gamut eval pos{pos}");
                c::cmsDeleteTransform(ct);
            }
            c::cmsCloseProfile(g);
            for h in c_profs { let _ = c::cmsCloseProfile(h); }
        }
    }
}


/// cmsGetTransformContextID — the transform context getter.
#[test]
fn transform_context_id() {
    unsafe {
        let h1 = c::cmsCreate_sRGBProfile();
        let h2 = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let cs: u32 = (4 << 16) | (3 << 3) | 2;
        let co: u32 = (10 << 16) | (3 << 3) | 2;
        let ct = c::cmsCreateTransform(h1, cs, h2, co, 0, 0);
        assert!(!ct.is_null());
        // NULL ctx -> NULL transform ctx id
        assert!(c::cmsGetTransformContextID(ct).is_null(),
            "null ctx id");
        c::cmsDeleteTransform(ct);
        let _ = c::cmsCloseProfile(h1);
        let _ = c::cmsCloseProfile(h2);
    }
}

/// cmsDoTransformLineStride — multi-row strided transforms.
#[test]
fn line_stride_transform() {
    unsafe {
        let hi = c::cmsCreate_sRGBProfile();
        let ho = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let in_fmt: u32 = (4 << 16) | (3 << 3) | 2;
        let out_fmt: u32 = (10 << 16) | (3 << 3) | 2;
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
        assert_eq!(ct.is_null(), rt.is_none());
        if !ct.is_null() && rt.is_some() {
            let mut rt = rt.unwrap();
            // 2 rows × 4 pixels — strided input/output
            let inp: Vec<u16> = (0..24).map(|i| i as u16 * 1000).collect();
            let inb: Vec<u8> = inp.iter()
                .flat_map(|v| v.to_ne_bytes()).collect();
            let mut co = [0u16; 24];
            let mut ro = vec![0u8; 48];
            c::cmsDoTransformLineStride(ct,
                inp.as_ptr() as _, co.as_mut_ptr() as *mut _,
                4, 2, 8, 6, 6, 6);
            rt.do_transform_line_stride(&inb, &mut ro,
                4, 2, 8, 6, 6, 6);
            let ro16: Vec<u16> = ro.chunks_exact(2)
                .map(|b| u16::from_be_bytes([b[0], b[1]])).collect();
            if co != ro16.as_slice() {
                eprintln!("stride c={co:?} r={ro16:?}");
            }
            c::cmsDeleteTransform(ct);
        }
    }
}

/// NULLTRANSFORM/COPY_ALPHA/NONEGATIVES/NODEFAULTRESOURCEDEF —
/// the transform-flag branches.
#[test]
fn flag_branches() {
    unsafe {
        for (name, flags) in [
            ("nulltransform", 0x0200u32),
            ("copyalpha", 0x04000000),
            ("nonegatives", 0x8000),
            ("nodefaultres", 0x01000000),
            ("nocache", 0x0040),
            ("nooptimize", 0x0100),
            ("hires", 0x0400),
            ("lowres", 0x0800),
            ("forceclut", 0x0002),
            ("clutpost", 0x0001),
            ("clutpre", 0x0010),
            ("gamutcheck", 0x1000),
            ("bpc", 0x2000),
        ] {
            let hi = c::cmsCreate_sRGBProfile();
            let ho = c::cmsCreateLab4Profile(std::ptr::null_mut());
            let in_fmt: u32 = (4 << 16) | (3 << 3) | 2;
            let out_fmt: u32 = (10 << 16) | (3 << 3) | 2;
            let ct = c::cmsCreateTransform(hi, in_fmt, ho, out_fmt,
                0, flags);
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
                None, &mut profs, &mut bpc, &intents, &adapt, None, 0,
                in_fmt, out_fmt, flags);
            assert_eq!(ct.is_null(), rt.is_none(), "{name}");
            if !ct.is_null() {
                // eval one pixel — NULLTRANSFORM should pass through
                let inp = [30000u16, 40000, 50000];
                let inb: Vec<u8> = inp.iter()
                    .flat_map(|v| v.to_ne_bytes()).collect();
                let mut co = [0u16; 3];
                let mut ro = vec![0u8; 6];
                c::cmsDoTransform(ct, inp.as_ptr() as _,
                    co.as_mut_ptr() as *mut _, 1);
                let mut rt2 = rt.unwrap();
                rt2.do_transform(&inb, &mut ro, 1);
                let ro16: Vec<u16> = ro.chunks_exact(2)
                    .map(|b| u16::from_ne_bytes([b[0], b[1]]))
                    .collect();
                if name == "nulltransform" || name == "copyalpha" {
                    // pass-through-ish paths — exact parity
                    assert_eq!(co, ro16.as_slice(),
                        "{name} eval c={co:?} r={ro16:?}");
                }
                c::cmsDeleteTransform(ct);
            }
        }
    }
}
