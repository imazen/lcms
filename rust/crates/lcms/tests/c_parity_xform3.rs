//! cmsxform remaining workers: DoTransformLineStride, ChangeBuffersFormat,
//! SetAdaptationState — byte-exact or status parity.

use lcms::{profile, types::*, virt, xform};
use lcms_cref as c;

#[test]
fn line_stride_transform() {
    unsafe {
        let h1 = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let h2 = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let in_fmt: u32 = (10 << 16) | (3 << 3) | 1; // TYPE_Lab_16
        let ct = c::cmsCreateTransform(h1, in_fmt, h2, in_fmt, 0, 0);
        let mut rp = vec![
            virt::create_lab4_profile(None, None).unwrap(),
            virt::create_lab4_profile(None, None).unwrap(),
        ];
        let mut bpc = [false; 2];
        let intents = [0u32; 2];
        let adapt = [1.0f64; 2];
        let mut rt = xform::create_extended_transform(
            None, &mut rp, &mut bpc, &intents, &adapt, None, 0,
            in_fmt, in_fmt, 0).unwrap();
        // 4 lines x 6 px Lab16 (3ch x 2B = 6B/px), strided
        let inp: Vec<u8> = (0..24 * 6).map(|i| (i * 37) as u8).collect();
        let mut co = vec![0u8; 24 * 6];
        let mut ro = vec![0u8; 24 * 6];
        c::cmsDoTransformLineStride(ct, inp.as_ptr() as _, co.as_mut_ptr() as _,
            6, 4, 36, 36, 6, 6);
        rt.do_transform_line_stride(&inp, &mut ro, 6, 4, 36, 36, 6, 6);
        assert_eq!(co, ro, "line-stride out");
        c::cmsDeleteTransform(ct);
        c::cmsCloseProfile(h1);
        c::cmsCloseProfile(h2);
    }
}

#[test]
fn change_buffers_format() {
    unsafe {
        let h1 = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let h2 = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let in16: u32 = (10 << 16) | (3 << 3) | 1;
        let out16: u32 = (10 << 16) | (3 << 3) | 1;
        let in8: u32 = (10 << 16) | (3 << 3) | 0;
        let ct = c::cmsCreateTransform(h1, in16, h2, out16, 0, 0);
        let mut rp = vec![
            virt::create_lab4_profile(None, None).unwrap(),
            virt::create_lab4_profile(None, None).unwrap(),
        ];
        let mut bpc = [false; 2];
        let intents = [0u32; 2];
        let adapt = [1.0f64; 2];
        let mut rt = xform::create_extended_transform(
            None, &mut rp, &mut bpc, &intents, &adapt, None, 0,
            in16, out16, 0).unwrap();
        // status parity — 16->8 change succeeds when CAN_CHANGE_FORMATTER
        assert_eq!(
            c::cmsChangeBuffersFormat(ct, in8, out16) != 0,
            rt.change_buffers_format(in8, out16), "change 16->8 in");
        assert_eq!(
            c::cmsChangeBuffersFormat(ct, in8, in8) != 0,
            rt.change_buffers_format(in8, in8), "change both");
        c::cmsDeleteTransform(ct);
        c::cmsCloseProfile(h1);
        c::cmsCloseProfile(h2);
    }
}

#[test]
fn adaptation_state() {
    unsafe {
        for d in [0.0f64, 0.5, 1.0, -0.5, 1.5] {
            let c_prev = c::cmsSetAdaptationStateTHR(std::ptr::null_mut(), d);
            let r_prev = xform::set_adaptation_state(None, d);
            assert_eq!(c_prev, r_prev, "adapt prev for d={d}");
        }
    }
}
