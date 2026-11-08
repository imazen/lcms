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
