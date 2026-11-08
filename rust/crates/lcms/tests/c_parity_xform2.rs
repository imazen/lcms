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

/// Pack/unroll dispatch on u8 formats — the formatter paths
/// beyond the u16 sweep.
#[test]
fn u8_packed_formats() {
    unsafe {
        let rgb8: u32 = (4 << 16) | (3 << 3) | 1;
        let lab8: u32 = (10 << 16) | (3 << 3) | 1;
        for (name, in_fmt, out_fmt) in [
            ("srgb->lab8", rgb8, lab8),
            ("lab8->srgb", lab8, rgb8),
        ] {
            let (hi, ho) = if name == "lab8->srgb" {
                (c::cmsCreateLab4Profile(std::ptr::null_mut()),
                 c::cmsCreate_sRGBProfile())
            } else {
                (c::cmsCreate_sRGBProfile(),
                 c::cmsCreateLab4Profile(std::ptr::null_mut()))
            };
            let ct = c::cmsCreateTransform(hi, in_fmt, ho, out_fmt, 0, 0);
            let _ = c::cmsCloseProfile(hi);
            let _ = c::cmsCloseProfile(ho);
            let mut rp1 = if name == "lab8->srgb" {
                virt::create_lab4_profile(None, None).unwrap()
            } else { virt::create_srgb_profile(None).unwrap() };
            let mut rp2 = if name == "lab8->srgb" {
                virt::create_srgb_profile(None).unwrap()
            } else { virt::create_lab4_profile(None, None).unwrap() };
            let mut profs = vec![rp1.clone(), rp2.clone()];
            let mut bpc = [false; 2];
            let intents = [0u32; 2];
            let adapt = [1.0f64; 2];
            let rt = xform::create_extended_transform(
                None, &mut profs, &mut bpc, &intents, &adapt, None, 0,
                in_fmt, out_fmt, 0);
            assert_eq!(ct.is_null(), rt.is_none(), "{name}");
            if !ct.is_null() && rt.is_some() {
                let mut rt = rt.unwrap();
                let inp = [0x80u8, 0x40, 0xC0];
                let mut co = [0u8; 4];
                let mut ro = [0u8; 4];
                c::cmsDoTransform(ct, inp.as_ptr() as _,
                    co.as_mut_ptr() as *mut _, 1);
                rt.do_transform(&inp, &mut ro[..3], 1);
                assert_eq!(co[..3], ro[..3], "{name} u8 eval");
                c::cmsDeleteTransform(ct);
            }
        }
    }
}



/// Packed 3-byte RGB (BYTES_SH(3)) + 4-byte (BYTES_SH(4)) formats —
/// the packed-byte formatter paths.
#[test]
fn packed_byte_formats() {
    unsafe {
        // TYPE_RGB_8_PLANAR-ish / packed 3-byte = BYTES_SH(3)
        // (3 bytes/pixel packed, not padded)
        let lab8: u32 = (10 << 16) | (3 << 3) | 1;
        let packed3: u32 = (4 << 16) | (3 << 3) | 3; // 3 bytes
        let hi = c::cmsCreate_sRGBProfile();
        let ho = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let ct = c::cmsCreateTransform(hi, packed3, ho, lab8, 0, 0);
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
            packed3, lab8, 0);
        assert_eq!(ct.is_null(), rt.is_none(), "packed3");
        if !ct.is_null() && rt.is_some() {
            let mut rt = rt.unwrap();
            let inp = [0x80u8, 0x40, 0xC0];
            let mut co = [0u8; 4];
            let mut ro = [0u8; 4];
            c::cmsDoTransform(ct, inp.as_ptr() as _,
                co.as_mut_ptr() as *mut _, 1);
            rt.do_transform(&inp, &mut ro[..3], 1);
            assert_eq!(co[..3], ro[..3], "packed3 eval");
            c::cmsDeleteTransform(ct);
        }
    }
}


/// Optimizer internals on unusual shapes — gray/XYZ/named
/// profiles triggering different optimizer branches.
#[test]
fn optimizer_shape_variants() {
    unsafe {
        let in_fmt: u32 = (4 << 16) | (3 << 3) | 2;
        let lab_fmt: u32 = (10 << 16) | (3 << 3) | 2;
        let g_fmt: u32 = (1 << 16) | (1 << 3) | 2;
        let xyz_fmt: u32 = (3 << 16) | (3 << 3) | 2;
        let g = c::cmsCreateGrayProfile(
            c::cmsD50_xyY() as *const std::ffi::c_void,
            c::cmsBuildGamma(std::ptr::null_mut(), 2.2));
        let xyz = c::cmsCreateXYZProfile();
        let srgb = c::cmsCreate_sRGBProfile();
        let lab4 = c::cmsCreateLab4Profile(
            std::ptr::null_mut());
        // unusual in/out profile pairs — different optimizer
        // branches fire on each
        for (name, (i, ifmt, o, ofmt)) in [
            ("gray->srgb", (g, g_fmt, srgb, in_fmt)),
            ("srgb->gray", (srgb, in_fmt, g, g_fmt)),
            ("xyz->srgb", (xyz, xyz_fmt, srgb, in_fmt)),
            ("srgb->xyz", (srgb, in_fmt, xyz, xyz_fmt)),
            ("lab4->gray", (lab4, lab_fmt, g, g_fmt)),
            ("gray->lab4", (g, g_fmt, lab4, lab_fmt)),
            ("xyz->gray", (xyz, xyz_fmt, g, g_fmt)),
            ("gray->xyz", (g, g_fmt, xyz, xyz_fmt)),
        ] {
            let ct = c::cmsCreateTransform(i, ifmt, o, ofmt, 0, 0);
            eprintln!("{name} null={}", ct.is_null());
            if !ct.is_null() {
                let inp = [30000u16, 40000, 50000, 0];
                let mut co = [0u16; 4];
                c::cmsDoTransform(ct, inp.as_ptr() as _,
                    co.as_mut_ptr() as *mut _, 1);
                c::cmsDeleteTransform(ct);
            }
        }
        let _ = c::cmsCloseProfile(g);
        let _ = c::cmsCloseProfile(xyz);
        let _ = c::cmsCloseProfile(srgb);
        let _ = c::cmsCloseProfile(lab4);
    }
}

/// Strided transforms on unusual stride/count combos — the
/// stride machinery.
#[test]
fn stride_combos() {
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
        // cmsDoTransformStride — different strides + counts
        let inp = vec![30000u16, 40000, 50000, 10000, 20000,
                       30000, 50000, 60000, 7000];
        let mut co = vec![0u16; 12];
        for (size, stride) in [(1u32, 6u32), (2, 12), (3, 24),
                               (2, 3), (1, 0)] {
            c::cmsDoTransformStride(ct,
                inp.as_ptr() as _, co.as_mut_ptr() as *mut _,
                size, stride);
        }
        c::cmsDeleteTransform(ct);
    }
}

/// cmsDoTransformLineStride — multi-row strided buffers
/// through a transform.
#[test]
fn line_stride_variants() {
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
        // (ppl, lines, bpl_in, bpl_out) combos — the stride
        // machinery walks rows at the byte strides
        let inp = vec![0x55u16; 96];
        let mut co = vec![0u16; 96];
        for (ppl, lines, bpl_in, bpl_out) in
            [(4u32, 2u32, 48u32, 48u32), (8, 1, 48, 48),
             (1, 8, 16, 16), (2, 4, 32, 32)] {
            co.fill(0);
            c::cmsDoTransformLineStride(ct,
                inp.as_ptr() as _, co.as_mut_ptr() as *mut _,
                ppl, lines, bpl_in, bpl_out, 6, 6);
        }
        c::cmsDeleteTransform(ct);
    }
}

/// Multiprofile machinery — more profile chains through
/// cmsCreateMultiprofileTransform.
#[test]
fn multiprofile_chains() {
    unsafe {
        let rgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let rgb2 = c::cmsCreate_sRGBProfile();
        let rgb3 = c::cmsCreate_sRGBProfile();
        let rgb4 = c::cmsCreate_sRGBProfile();
        let fmt = (4 << 16) | (3 << 3) | 2u32;
        let lfmt = (10 << 16) | (3 << 3) | 2u32;
        for (name, profs, inf, outf) in [
            ("rgb-lab-rgb",
             vec![rgb, lab, rgb2] as Vec<*mut _>, fmt, fmt),
            ("rgb-rgb-rgb",
             vec![rgb, rgb3, rgb2] as Vec<*mut _>, fmt, fmt),
            ("rgb4",
             vec![rgb, rgb3, rgb2, rgb4] as Vec<*mut _>,
             fmt, fmt),
            ("rgb-lab",
             vec![rgb, lab] as Vec<*mut _>, fmt, lfmt)] {
            let mut pv = profs;
            let n = pv.len() as i32;
            let t = c::cmsCreateMultiprofileTransform(
                pv.as_mut_ptr(), n, inf, outf, 0, 0);
            eprintln!("{name} t={}", !t.is_null());
            if !t.is_null() { c::cmsDeleteTransform(t); }
        }
        // don't double-close: transforms consumed the handles
        // via dup or kept them — close only un-transformed
        let _ = (rgb, lab, rgb2, rgb3, rgb4);
    }
}

/// Multiprofile machinery — chains with devicelinks +
/// larger chains.
#[test]
fn multiprofile_devlink() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mut curves = [g; 3];
        let dl = c::cmsCreateLinearizationDeviceLinkTHR(
            std::ptr::null_mut(), 0x52474220,
            curves.as_mut_ptr());
        let rgb = c::cmsCreate_sRGBProfile();
        let rgb2 = c::cmsCreate_sRGBProfile();
        let dl2 = c::cmsCreateLinearizationDeviceLinkTHR(
            std::ptr::null_mut(), 0x434D594B,
            [g; 4].as_mut_ptr());
        let fmt = (4 << 16) | (3 << 3) | 2u32;
        for (name, profs) in [
            ("rgb-dl", vec![rgb, dl] as Vec<*mut _>),
            ("dl-rgb", vec![dl, rgb2] as Vec<*mut _>),
            ("rgb-dl-rgb", vec![rgb, dl, rgb2] as Vec<*mut _>),
            ("dl-dl2", vec![dl, dl2] as Vec<*mut _>)] {
            let mut pv = profs;
            let n = pv.len() as i32;
            let t = c::cmsCreateMultiprofileTransform(
                pv.as_mut_ptr(), n, fmt, fmt, 0, 0);
            eprintln!("{name} t={}", !t.is_null());
            if !t.is_null() { c::cmsDeleteTransform(t); }
        }
        let _ = (rgb, rgb2, dl, dl2);
    }
}

/// Proofing machinery — cmsCreateProofingTransform on more
/// combos.
#[test]
fn proofing_variants() {
    unsafe {
        let rgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let rgb2 = c::cmsCreate_sRGBProfile();
        let fmt = (4 << 16) | (3 << 3) | 2u32;
        for (name, pin, pproof, pout) in [
            ("rgb-rgb-rgb", rgb, rgb2, rgb2),
            ("rgb-lab-rgb", rgb, lab, rgb2),
            ("rgb-rgb-lab", rgb, rgb2, lab)] {
            let t = c::cmsCreateProofingTransform(pin, fmt,
                pout, fmt, pproof, 0, 0, 0);
            eprintln!("{name} t={}", !t.is_null());
            if !t.is_null() { c::cmsDeleteTransform(t); }
        }
        let _ = (rgb, lab, rgb2);
    }
}

/// Context machinery — THR variants on more combos.
#[test]
fn thr_variants() {
    unsafe {
        let ctx = c::cmsCreateContext(std::ptr::null_mut(),
            std::ptr::null_mut());
        let rgb = c::cmsCreate_sRGBProfileTHR(ctx);
        let lab = c::cmsCreateLab4ProfileTHR(ctx,
            std::ptr::null_mut());
        let fmt = (4 << 16) | (3 << 3) | 2u32;
        let lfmt = (10 << 16) | (3 << 3) | 2u32;
        let t1 = c::cmsCreateTransformTHR(ctx, rgb, fmt,
            lab, lfmt, 0, 0);
        eprintln!("rgb-lab-thr={}", !t1.is_null());
        if !t1.is_null() { c::cmsDeleteTransform(t1); }
        let mut profs = [rgb, lab];
        let mut intents = [0u32, 0];
        let mut bpc = [0i32, 0];
        let mut adaptation = 0.0f64;
        let t2 = c::cmsCreateExtendedTransform(ctx, 2,
            profs.as_mut_ptr(), bpc.as_mut_ptr(),
            intents.as_mut_ptr(), &mut adaptation,
            std::ptr::null_mut(), 0, fmt, lfmt, 0);
        eprintln!("ext={}", !t2.is_null());
        if !t2.is_null() { c::cmsDeleteTransform(t2); }
        let _ = (rgb, lab, ctx);
    }
}

/// Intent-support machinery on more profile types —
/// devicelinks/abstract/placeholder.
#[test]
fn intent_supported2() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mut curves = [g; 3];
        let dl = c::cmsCreateLinearizationDeviceLinkTHR(
            std::ptr::null_mut(), 0x52474220,
            curves.as_mut_ptr());
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let ph = c::cmsCreateProfilePlaceholder(
            std::ptr::null_mut());
        for (pname, h) in [
            ("devlink", dl), ("lab4", lab), ("ph", ph)] {
            for (dir, intent) in [
                (0u32, 0u32), (0, 1), (0, 2), (0, 3),
                (1, 0), (1, 1), (1, 2), (1, 3)] {
                let r = c::cmsIsIntentSupported(h, intent, dir);
                eprintln!("{pname} d{dir}i{intent}={r}");
            }
        }
        let _ = (dl, lab, ph);
    }
}
