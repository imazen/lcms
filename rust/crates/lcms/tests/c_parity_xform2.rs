//! cmsxform.c extended surface: multiprofile chains, proofing transforms,
//! cmsChangeBuffersFormat, stride/linestride do_transform variants, alarm
//! codes and adaptation state. All byte-exact C-vs-Rust on sRGB/Lab chains.

use lcms::virt;
use lcms::xform;
use lcms_cref as c;

const PT_RGB: u32 = 4;
const PT_LAB: u32 = 10;
const RGB8: u32 = PT_RGB << 16 | 3 << 3 | 1;
const LAB8: u32 = PT_LAB << 16 | 3 << 3 | 1;
const RGB16: u32 = PT_RGB << 16 | 3 << 3 | 2;
const CMYK8: u32 = 5 << 16 | 4 << 3 | 1;

fn fill(n: usize) -> Vec<u8> {
    let mut v = vec![0u8; n];
    let mut s = 0x1234_5678u32;
    for (i, b) in v.iter_mut().enumerate() {
        s ^= s << 13;
        s ^= s >> 17;
        s ^= s << 5;
        *b = (s as u8).wrapping_add(i as u8);
    }
    v
}

fn r_xform(
    profs: &mut [Box<lcms::profile::IccProfile>],
    gamut: Option<&mut lcms::profile::IccProfile>,
    gpos: u32,
    fi: u32,
    fo: u32,
    flags: u32,
) -> Option<Box<xform::Transform>> {
    let n = profs.len();
    let mut bpc = vec![false; n];
    let intents = vec![0u32; n];
    let adapt = vec![1.0f64; n];
    xform::create_extended_transform(
        None, profs, &mut bpc, &intents, &adapt, gamut, gpos, fi, fo, flags,
    )
}

#[test]
fn multiprofile_chain() {
    // sRGB -> Lab4 -> sRGB, RGB8 in/out
    let n = 257;
    let inp = fill(n * 3);
    let (c_out,) = unsafe {
        let srgb1 = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null());
        let srgb2 = c::cmsCreate_sRGBProfile();
        let arr = [srgb1, lab, srgb2];
        let t = c::cmsCreateMultiprofileTransform(
            arr.as_ptr() as *mut _,
            3,
            RGB8,
            RGB8,
            0,
            0,
        );
        assert!(!t.is_null(), "C multiprofile");
        let mut out = vec![0u8; n * 3];
        c::cmsDoTransform(t, inp.as_ptr() as _, out.as_mut_ptr() as _, n as u32);
        c::cmsDeleteTransform(t);
        c::cmsCloseProfile(srgb1);
        c::cmsCloseProfile(lab);
        c::cmsCloseProfile(srgb2);
        (out,)
    };
    let mut profs = vec![
        virt::create_srgb_profile(None).unwrap(),
        virt::create_lab4_profile(None, None).unwrap(),
        virt::create_srgb_profile(None).unwrap(),
    ];
    let mut t = r_xform(&mut profs, None, 0, RGB8, RGB8, 0).expect("rust multiprofile");
    let mut r_out = vec![0u8; n * 3];
    t.do_transform(&inp, &mut r_out, n as u32);
    assert_eq!(c_out, r_out, "multiprofile sRGB->Lab->sRGB");
}

#[test]
fn proofing_transform() {
    // sRGB -> sRGB proofed through CMYK (lcms2cmyk.icc), GamutCheck flag
    let n = 137;
    let inp = fill(n * 3);
    let path = format!("{}/../../lcms2cmyk.icc\0", env!("CARGO_MANIFEST_DIR"));
    for flags in [0x2000u32, 0] {
        // cmsFLAGS_GAMUTCHECK / plain proof
        let c_out = unsafe {
            let i = c::cmsCreate_sRGBProfile();
            let o = c::cmsCreate_sRGBProfile();
            let prf =
                c::cmsOpenProfileFromFile(path.as_ptr() as *const i8, b"r\0".as_ptr());
            assert!(!prf.is_null());
            let t = c::cmsCreateProofingTransform(i, RGB8, o, RGB8, prf, 0, 0, flags);
            assert!(!t.is_null(), "C proofing flags={flags:#x}");
            let mut out = vec![0u8; n * 3];
            c::cmsDoTransform(t, inp.as_ptr() as _, out.as_mut_ptr() as _, n as u32);
            c::cmsDeleteTransform(t);
            c::cmsCloseProfile(i);
            c::cmsCloseProfile(o);
            c::cmsCloseProfile(prf);
            out
        };
        let mut profs = vec![
            virt::create_srgb_profile(None).unwrap(),
            virt::create_srgb_profile(None).unwrap(),
        ];
        let mut gp = lcms::profile::open_profile_from_file(
            None,
            &format!("{}/../../lcms2cmyk.icc", env!("CARGO_MANIFEST_DIR")),
            "r",
        )
        .expect("rust cmyk");
        let mut t = r_xform(&mut profs, Some(&mut gp), 1, RGB8, RGB8, flags)
            .expect("rust proofing");
        let mut r_out = vec![0u8; n * 3];
        t.do_transform(&inp, &mut r_out, n as u32);
        assert_eq!(c_out, r_out, "proofing flags={flags:#x}");
    }
}

#[test]
fn change_buffers_format() {
    // create RGB8->RGB8, then switch to RGB16->RGB16 and transform.
    let n = 101;
    let inp8 = fill(n * 3);
    let mut inp16 = vec![0u8; n * 3 * 2];
    for i in 0..n * 3 {
        let v = (inp8[i] as u16) * 257;
        inp16[i * 2] = (v & 0xff) as u8;
        inp16[i * 2 + 1] = (v >> 8) as u8;
    }
    let (c_status, c_out) = unsafe {
        let prof = c::cmsCreate_sRGBProfile();
        let t = c::cmsCreateTransform(prof, RGB8, prof, RGB8, 0, 0);
        assert!(!t.is_null());
        let st = c::cmsChangeBuffersFormat(t, RGB16, RGB16);
        let mut out = vec![0u8; n * 3 * 2];
        if st != 0 {
            c::cmsDoTransform(t, inp16.as_ptr() as _, out.as_mut_ptr() as _, n as u32);
        }
        c::cmsDeleteTransform(t);
        c::cmsCloseProfile(prof);
        (st, out)
    };
    let p1 = virt::create_srgb_profile(None).unwrap();
    let p2 = virt::create_srgb_profile(None).unwrap();
    let mut profs = vec![p1, p2];
    let mut t = r_xform(&mut profs, None, 0, RGB8, RGB8, 0).expect("rust xform");
    let r_status = t.change_buffers_format(RGB16, RGB16);
    assert_eq!(c_status != 0, r_status, "change_buffers status");
    let mut r_out = vec![0u8; n * 3 * 2];
    if r_status {
        t.do_transform(&inp16, &mut r_out, n as u32);
    }
    assert_eq!(c_out, r_out, "change_buffers output");

    // format getter parity — sRGB input profile, Lab4 output profile
    unsafe {
        let pi = c::cmsCreate_sRGBProfile();
        let po = c::cmsCreateLab4Profile(std::ptr::null());
        let t = c::cmsCreateTransform(pi, RGB8, po, LAB8, 0, 0);
        assert!(!t.is_null());
        let ci = c::cmsGetTransformInputFormat(t);
        let co = c::cmsGetTransformOutputFormat(t);
        c::cmsDeleteTransform(t);
        c::cmsCloseProfile(pi);
        c::cmsCloseProfile(po);
        let p1 = virt::create_srgb_profile(None).unwrap();
        let p2 = virt::create_lab4_profile(None, None).unwrap();
        let mut profs = vec![p1, p2];
        let rt = r_xform(&mut profs, None, 0, RGB8, LAB8, 0).expect("rust xform2");
        assert_eq!(ci, rt.input_format, "input format");
        assert_eq!(co, rt.output_format, "output format");
    }
}

#[test]
fn stride_variants() {
    // padded rows: transform n px spread across a larger row stride
    let px = 64;
    let stride_extra = 11; // bytes of padding after each "pixel row"
    let inp = fill(px * 3 + stride_extra);
    let c_out = unsafe {
        let prof = c::cmsCreate_sRGBProfile();
        let t = c::cmsCreateTransform(prof, RGB8, prof, RGB8, 0, 0);
        let mut out = vec![0u8; px * 3 + stride_extra];
        c::cmsDoTransformStride(
            t,
            inp.as_ptr() as _,
            out.as_mut_ptr() as _,
            px as u32,
            (px * 3 + stride_extra) as u32,
        );
        c::cmsDeleteTransform(t);
        c::cmsCloseProfile(prof);
        out
    };
    let p1 = virt::create_srgb_profile(None).unwrap();
    let p2 = virt::create_srgb_profile(None).unwrap();
    let mut profs = vec![p1, p2];
    let mut t = r_xform(&mut profs, None, 0, RGB8, RGB8, 0).expect("rust xform");
    let mut r_out = vec![0u8; px * 3 + stride_extra];
    t.do_transform_stride(
        &inp,
        &mut r_out,
        px as u32,
        (px * 3 + stride_extra) as u32,
    );
    assert_eq!(c_out, r_out, "do_transform_stride");

    // line stride: 4 lines of ppl=px pixels, padded input + output lines
    let ppl = 16u32;
    let lines = 4u32;
    let bpl_in = (ppl * 3 + stride_extra as u32 * 0) as usize + 5; // padded
    let bpl_out = (ppl * 3) as usize + 7;
    let inp2 = fill(bpl_in * lines as usize);
    let (c2,) = unsafe {
        let prof = c::cmsCreate_sRGBProfile();
        let t = c::cmsCreateTransform(prof, RGB8, prof, RGB8, 0, 0);
        let mut out = vec![0u8; bpl_out * lines as usize];
        c::cmsDoTransformLineStride(
            t,
            inp2.as_ptr() as _,
            out.as_mut_ptr() as _,
            ppl,
            lines,
            bpl_in as u32,
            bpl_out as u32,
            0,
            0,
        );
        c::cmsDeleteTransform(t);
        c::cmsCloseProfile(prof);
        (out,)
    };
    let p1 = virt::create_srgb_profile(None).unwrap();
    let p2 = virt::create_srgb_profile(None).unwrap();
    let mut profs = vec![p1, p2];
    let mut t = r_xform(&mut profs, None, 0, RGB8, RGB8, 0).expect("rust xform2");
    let mut r2 = vec![0u8; bpl_out * lines as usize];
    t.do_transform_line_stride(
        &inp2,
        &mut r2,
        ppl,
        lines,
        bpl_in as u32,
        bpl_out as u32,
        0,
        0,
    );
    assert_eq!(c2, r2, "do_transform_line_stride");
}

#[test]
fn alarm_and_adaptation() {
    let mut c_codes = [0u16; 16];
    unsafe {
        c::cmsSetAlarmCodesTHR(std::ptr::null_mut(), [0xDEADu16; 16].as_ptr());
        c::cmsGetAlarmCodesTHR(std::ptr::null_mut(), c_codes.as_mut_ptr());
    }
    xform::set_alarm_codes(None, &[0xDEAD; 16]);
    let r_codes = xform::get_alarm_codes(None);
    assert_eq!(c_codes[..], r_codes[..], "alarm codes");

    let c_prev = unsafe { c::cmsSetAdaptationStateTHR(std::ptr::null_mut(), 0.5) };
    let r_prev = xform::set_adaptation_state(None, 0.5);
    assert_eq!(c_prev, r_prev, "adaptation prev");
    unsafe {
        c::cmsSetAdaptationStateTHR(std::ptr::null_mut(), 1.0);
    }
    xform::set_adaptation_state(None, 1.0);

    // out-of-gamut pixels through a Lab->sRGB transform trip the alarm codes
    let n = 33;
    let inp = fill(n * 3);
    for flags in [0x2000u32, 0x0400u32] {
        // GAMUTCHECK / SOFTPROOFING
        let c_out = unsafe {
            let i = c::cmsCreateLab4Profile(std::ptr::null());
            let o = c::cmsCreate_sRGBProfile();
            let t = c::cmsCreateTransform(i, LAB8, o, RGB8, 0, flags);
            assert!(!t.is_null(), "C gamut xform {flags:#x}");
            let mut out = vec![0u8; n * 3];
            c::cmsDoTransform(t, inp.as_ptr() as _, out.as_mut_ptr() as _, n as u32);
            c::cmsDeleteTransform(t);
            c::cmsCloseProfile(i);
            c::cmsCloseProfile(o);
            out
        };
        let p1 = virt::create_lab4_profile(None, None).unwrap();
        let p2 = virt::create_srgb_profile(None).unwrap();
        let mut profs = vec![p1, p2];
        let mut t = r_xform(&mut profs, None, 0, LAB8, RGB8, flags).expect("rust gamut");
        let mut r_out = vec![0u8; n * 3];
        t.do_transform(&inp, &mut r_out, n as u32);
        assert_eq!(c_out, r_out, "gamut flags={flags:#x}");
    }
}

/// cmscnvrt intent-pipeline coverage: transforms at intents 1/2/3 use
/// different _cmsLinkProfiles construction paths than the intent-0 cases.
#[test]
fn intent_pipelines() {
    let profiles: Vec<String> = std::fs::read_dir(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../"),
    ).unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "icc").unwrap_or(false))
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    let n_px = 16;
    for intent in 1..4u32 {
        for f in &profiles {
            let data = std::fs::read(f).unwrap();
            let ch = unsafe { c::cmsOpenProfileFromMem(data.as_ptr() as _, data.len() as u32) };
            if ch.is_null() { continue; }
            let mut rp = lcms::profile::open_profile_from_mem(None, &data).unwrap();
            // input/output format from the profile's own colorspace
            let cs = rp.color_space;
            let (fi, fo) = (cs | 3 << 3 | 1, cs | 3 << 3 | 1); // u8 chunky
            let ct = unsafe { c::cmsCreateTransform(ch, fi, ch, fo, intent, 0) };
            let mut profs = vec![rp.clone()];
            let mut bpc = [false];
            let intents = [intent];
            let adapt = [1.0f64];
            let rt = lcms::xform::create_extended_transform(
                None, &mut profs, &mut bpc, &intents, &adapt, None, 0, fi, fo, 0,
            );
            assert_eq!(ct.is_null(), rt.is_none(),
                "{} intent {intent} transform presence", f);
            if ct.is_null() || rt.is_none() { continue; }
            let mut rt = rt.unwrap();
            let in_sz = n_px * ((fi >> 3) & 15) as usize;
            let out_sz = n_px * ((fo >> 3) & 15) as usize;
            let inp: Vec<u8> = (0..in_sz).map(|i| (i * 7 + intent as usize) as u8).collect();
            let mut co = vec![0u8; out_sz];
            let mut ro = vec![0u8; out_sz];
            unsafe { c::cmsDoTransform(ct, inp.as_ptr() as _, co.as_mut_ptr() as _, n_px as u32) };
            rt.do_transform(&inp, &mut ro, n_px as u32);
            assert_eq!(co, ro, "{} intent {intent}", f);
            unsafe { c::cmsDeleteTransform(ct) };
            unsafe { c::cmsCloseProfile(ch) };
        }
    }
}
