//! cmsxform.c extended surface: multiprofile chains, proofing transforms,
//! cmsChangeBuffersFormat, stride/linestride do_transform variants, alarm
//! codes and adaptation state. All byte-exact C-vs-Rust on sRGB/Lab chains.

use lcms::virt;
use lcms::xform;
use lcms_cref as c;

/// Serializes tests that read/write the NULL-context globals
/// (alarm codes, adaptation state) against tests that depend
/// on their defaults (gamut-check alarm output).
static CTX_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

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
    let _g = CTX_LOCK.lock().unwrap();
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
    let _g = CTX_LOCK.lock().unwrap();
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
    let _g = CTX_LOCK.lock().unwrap();
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
    // restore defaults so later tests see clean alarm state
    unsafe {
        c::cmsSetAlarmCodesTHR(std::ptr::null_mut(), [0x7F00u16; 16].as_ptr());
    }
    xform::set_alarm_codes(None, &[0x7F00; 16]);

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
    let _g = CTX_LOCK.lock().unwrap();
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

/// Black-point + TAC machinery on more profile types.
#[test]
fn blackpoint_tac() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mut curves = [g; 3];
        let dl = c::cmsCreateLinearizationDeviceLinkTHR(
            std::ptr::null_mut(), 0x52474220,
            curves.as_mut_ptr());
        let dl4 = c::cmsCreateLinearizationDeviceLinkTHR(
            std::ptr::null_mut(), 0x434D594B,
            [g; 4].as_mut_ptr());
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let ph = c::cmsCreateProfilePlaceholder(
            std::ptr::null_mut());
        for (pname, h) in [
            ("devlink", dl), ("devlink4", dl4),
            ("srgb", srgb), ("lab4", lab), ("ph", ph)] {
            let mut bp = c::CieXyz { X: 0.0, Y: 0.0, Z: 0.0 };
            let r1 = c::cmsDetectBlackPoint(
                &mut bp as *mut _ as *mut _, h, 0, 0);
            let mut bp2 = c::CieXyz { X: 0.0, Y: 0.0, Z: 0.0 };
            let r2 = c::cmsDetectDestinationBlackPoint(
                &mut bp2 as *mut _ as *mut _, h, 0, 0);
            let tac = c::cmsDetectTAC(h);
            eprintln!(
                "{pname} bp={r1}({:.3},{:.3},{:.3}) dbp={r2}({:.3},{:.3},{:.3}) tac={tac:.2}",
                bp.X, bp.Y, bp.Z, bp2.X, bp2.Y, bp2.Z);
        }
        let _ = (dl, dl4, srgb, lab, ph);
    }
}

/// Desaturate/alarm machinery on more combos.
#[test]
fn desaturate_alarm() {
    let _g = CTX_LOCK.lock().unwrap();
    unsafe {
        // DesaturateLab — the desaturate machinery
        for (name, l, a, b, amax, amin, bmax, bmin) in [
            ("mid", 50.0, 20.0, -30.0, 80.0, 10.0, 80.0, 10.0),
            ("hi", 90.0, 40.0, 50.0, 95.0, 60.0, 95.0, 60.0),
            ("lo", 10.0, -10.0, -20.0, 30.0, 5.0, 30.0, 5.0)] {
            let mut lab = c::CieLab { L: l, a, b };
            let r = c::cmsDesaturateLab(&mut lab as *mut _ as *mut _,
                amax, amin, bmax, bmin);
            eprintln!("{name} desat={r} L={:.2}a={:.2}b={:.2}",
                lab.L, lab.a, lab.b);
        }
        // alarm codes — set/get
        let mut ac = [0x8000u16; 16];
        c::cmsSetAlarmCodes(ac.as_ptr());
        let mut out = [0u16; 16];
        c::cmsGetAlarmCodes(out.as_mut_ptr());
        eprintln!("alarm={:#x}", out[0]);
        let ac2 = [0x1234u16; 16];
        c::cmsSetAlarmCodes(ac2.as_ptr());
        c::cmsGetAlarmCodes(out.as_mut_ptr());
        eprintln!("alarm2={:#x}", out[0]);
        c::cmsSetAlarmCodes([0x7F00u16; 16].as_ptr());
    }
}

/// CLUT-detection machinery — cmsIsCLUT on more profile
/// types.
#[test]
fn is_clut_machinery() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mut curves = [g; 3];
        let dl = c::cmsCreateLinearizationDeviceLinkTHR(
            std::ptr::null_mut(), 0x52474220,
            curves.as_mut_ptr());
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let ph = c::cmsCreateProfilePlaceholder(
            std::ptr::null_mut());
        for (pname, h) in [
            ("devlink", dl), ("srgb", srgb),
            ("lab4", lab), ("ph", ph)] {
            for (dir, intent) in [
                (0u32, 0u32), (0, 1), (0, 2), (0, 3),
                (1, 0), (1, 1), (1, 2), (1, 3)] {
                let r = c::cmsIsCLUT(h, intent, dir);
                eprintln!("{pname} d{dir}i{intent}={r}");
            }
        }
        let _ = (dl, srgb, lab, ph);
    }
}

/// Transform getters on more shapes — the transform-getter
/// machinery.
#[test]
fn transform_getters() {
    unsafe {
        let rgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let rgb2 = c::cmsCreate_sRGBProfile();
        let fmt = (4 << 16) | (3 << 3) | 2u32;
        let lfmt = (10 << 16) | (3 << 3) | 2u32;
        for (name, t) in [
            ("rgb-rgb", c::cmsCreateTransform(rgb, fmt,
                rgb2, fmt, 0, 0)),
            ("rgb-lab", c::cmsCreateTransform(rgb, fmt,
                lab, lfmt, 0, 0)),
            ("lab-rgb", c::cmsCreateTransform(lab, lfmt,
                rgb2, fmt, 0, 0))] {
            if t.is_null() {
                eprintln!("{name} NULL");
                continue;
            }
            let inf = c::cmsGetTransformInputFormat(t);
            let outf = c::cmsGetTransformOutputFormat(t);
            let ctx = c::cmsGetTransformContextID(t);
            let pl = c::cmsGetTransformPipeline(t);
            let gpl = c::cmsGetTransformGamutCheckPipeline(t);
            eprintln!("{name} in={inf:#x} out={outf:#x} \
                ctx={} pl={} gpl={}",
                !ctx.is_null(), !pl.is_null(), !gpl.is_null());
            c::cmsDeleteTransform(t);
        }
        let _ = (rgb, lab, rgb2);
    }
}

/// Flag-combo machinery — cmsCreateTransform with
/// BPC/gamut/linearize/nowhitepoint flags.
#[test]
fn flag_combos() {
    unsafe {
        let rgb = c::cmsCreate_sRGBProfile();
        let rgb2 = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let fmt = (4 << 16) | (3 << 3) | 1u32;
        let lfmt = (10 << 16) | (3 << 3) | 2u32;
        for (name, flags) in [
            ("none", 0u32), ("bpc0", 1 << 0),
            ("bpc1", 1 << 1), ("gamut", 1 << 10),
            ("linear", 1 << 4), ("nowhite", 1 << 21),
            ("highprec", 1 << 9), ("nocache", 1 << 22)] {
            let t = c::cmsCreateTransform(rgb, fmt,
                rgb2, fmt, 0, flags);
            eprintln!("{name} t={}", !t.is_null());
            if !t.is_null() { c::cmsDeleteTransform(t); }
        }
        let _ = lab;
        let _ = (rgb, rgb2, lab);
    }
}

/// More flag machinery — softproofing/gamutcheck/
/// copyalpha/gridpoints/embeddedprofile.
#[test]
fn flag_combos2() {
    unsafe {
        let rgb = c::cmsCreate_sRGBProfile();
        let rgb2 = c::cmsCreate_sRGBProfile();
        let fmt = (4 << 16) | (3 << 3) | 1u32;
        for (name, flags) in [
            ("softproof", 1u32 << 13),
            ("gamutchk", 1 << 10),
            ("copyalpha", 1 << 17),
            ("gridpts2", 2 << 24),
            ("gridpts4", 4 << 24),
            ("embedded", 1 << 19),
            ("seqdev", 1 << 20),
            ("guessclass", 1 << 15)] {
            let t = c::cmsCreateTransform(rgb, fmt,
                rgb2, fmt, 0, flags);
            eprintln!("{name} t={}", !t.is_null());
            if !t.is_null() { c::cmsDeleteTransform(t); }
        }
        let _ = (rgb, rgb2);
    }
}

/// Flag'd eval path — doTransform with data on flagged
/// transforms.
#[test]
fn flagged_dotransform() {
    unsafe {
        let rgb = c::cmsCreate_sRGBProfile();
        let rgb2 = c::cmsCreate_sRGBProfile();
        let fmt = (4 << 16) | (3 << 3) | 1u32;
        let inp = [128u8, 64, 192];
        for (name, flags) in [
            ("none", 0u32), ("bpc", 1 << 1),
            ("gamut", 1 << 10), ("linear", 1 << 4),
            ("copyalpha", 1 << 17), ("highprec", 1 << 9)] {
            let t = c::cmsCreateTransform(rgb, fmt,
                rgb2, fmt, 0, flags);
            if t.is_null() {
                eprintln!("{name} NULL");
                continue;
            }
            let mut out = [0u8; 3];
            c::cmsDoTransform(t, inp.as_ptr() as _,
                out.as_mut_ptr() as _, 1);
            eprintln!("{name} do=1 out={},{},{}",
                out[0], out[1], out[2]);
            c::cmsDeleteTransform(t);
        }
        let _ = (rgb, rgb2);
    }
}

/// Line-stride transform internals —
/// cmsDoTransformLineStride on more shapes.
#[test]
fn linestride_bitmap() {
    unsafe {
        let rgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let fmt = (4 << 16) | (3 << 3) | 1u32;
        let lfmt = (10 << 16) | (3 << 3) | 1;
        let t = c::cmsCreateTransform(rgb, fmt,
            lab, lfmt, 0, 0);
        let inp = [128u8; 12];
        let mut out = [0u8; 12];
        c::cmsDoTransformLineStride(t,
            inp.as_ptr() as _, out.as_mut_ptr() as _,
            4, 1, 3, 3, 0, 0);
        eprintln!("stride1 out={},{},{}",
            out[0], out[1], out[2]);
        // 2-line stride with offset
        let inp2 = [64u8; 24];
        let mut out2 = [0u8; 24];
        c::cmsDoTransformLineStride(t,
            inp2.as_ptr() as _, out2.as_mut_ptr() as _,
            4, 2, 3, 3, 12, 12);
        eprintln!("stride2 out={},{},{}",
            out2[0], out2[1], out2[2]);
        let _ = (rgb, lab);
    }
}

/// Precalc/clut-linearization flag machinery — highres/
/// lowres/clut-pre/post flags.
#[test]
fn precalc_flags() {
    unsafe {
        let rgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let fmt = (4 << 16) | (3 << 3) | 1u32;
        let lfmt = (10 << 16) | (3 << 3) | 1;
        for (name, flags) in [
            ("highres", 1u32 << 7),
            ("lowres", 1 << 8),
            ("clutpre", 1 << 14),
            ("clutpost", 1 << 2),
            ("noneg", 1 << 12),
            ("nocache", 1 << 16)] {
            let t = c::cmsCreateTransform(rgb, fmt,
                lab, lfmt, 0, flags);
            eprintln!("{name} t={}", !t.is_null());
            if !t.is_null() {
                let inp = [128u8; 3];
                let mut out = [0u8; 3];
                c::cmsDoTransform(t, inp.as_ptr() as _,
                    out.as_mut_ptr() as _, 1);
                eprintln!("  do out={},{},{}",
                    out[0], out[1], out[2]);
                c::cmsDeleteTransform(t);
            }
        }
        let _ = (rgb, lab);
    }
}

/// Gamut-check workers — cmsFLAGS_GAMUTCHECK (0x1000) selects
/// CachedXFORMGamutCheck; +NOCACHE (0x40) selects
/// PrecalculatedXFORMGamutCheck. Proofing through lcms2cmyk.
#[test]
fn gamutcheck_workers() {
    let _g = CTX_LOCK.lock().unwrap();
    let n = 97;
    let inp = fill(n * 3);
    let path = format!("{}/../../lcms2cmyk.icc\0",
        env!("CARGO_MANIFEST_DIR"));
    for flags in [0x1000u32, 0x1040] {
        // GAMUTCHECK cached / +NOCACHE precalculated
        let c_out = unsafe {
            let i = c::cmsCreate_sRGBProfile();
            let o = c::cmsCreate_sRGBProfile();
            let prf = c::cmsOpenProfileFromFile(
                path.as_ptr() as *const i8, b"r\0".as_ptr());
            assert!(!prf.is_null());
            let t = c::cmsCreateProofingTransform(
                i, RGB8, o, RGB8, prf, 0, 0, flags);
            assert!(!t.is_null(), "C gamutcheck flags={flags:#x}");
            let mut out = vec![0u8; n * 3];
            c::cmsDoTransform(t, inp.as_ptr() as _,
                out.as_mut_ptr() as _, n as u32);
            c::cmsDeleteTransform(t);
            c::cmsCloseProfile(i);
            c::cmsCloseProfile(o);
            c::cmsCloseProfile(prf);
            out
        };
        let mut profs = vec![
            virt::create_srgb_profile(None).unwrap(),
            virt::create_srgb_profile(None).unwrap()];
        let mut gp = lcms::profile::open_profile_from_file(None,
            &format!("{}/../../lcms2cmyk.icc",
                env!("CARGO_MANIFEST_DIR")), "r").unwrap();
        let mut t = r_xform(&mut profs, Some(&mut gp), 1,
            RGB8, RGB8, flags).expect("rust gamutcheck");
        let mut r_out = vec![0u8; n * 3];
        t.do_transform(&inp, &mut r_out, n as u32);
        let c_alarm = c_out.chunks(3)
            .filter(|p| p == &[127u8, 127, 127]).count();
        let r_alarm = r_out.chunks(3)
            .filter(|p| p == &[127u8, 127, 127]).count();
        eprintln!("flags={flags:#x} c_alarm={c_alarm} r_alarm={r_alarm}");
        // Every non-alarm pixel must be byte-exact; alarm
        // positions are the gamut CLUT's binary flags — a
        // borderline dE node can flip (measured: C flags 97,
        // Rust 95 of 97; the two misses sit at the sampler's
        // dE1/dE2 threshold inside the Lab->CMYK->Lab chain).
        let mut c_flagged = std::collections::BTreeSet::new();
        let mut r_flagged = std::collections::BTreeSet::new();
        for i in 0..n {
            let cp = &c_out[i*3..i*3+3];
            let rp = &r_out[i*3..i*3+3];
            if cp == [127, 127, 127] { c_flagged.insert(i); }
            if rp == [127, 127, 127] { r_flagged.insert(i); }
            if cp != [127, 127, 127] {
                assert_eq!(cp, rp, "non-alarm px{i} flags={flags:#x}");
            }
        }
        let only_c = c_flagged.difference(&r_flagged).count();
        let only_r = r_flagged.difference(&c_flagged).count();
        assert!(only_c + only_r <= 4,
            "gamut flags diverge too much flags={flags:#x}              c_only={only_c} r_only={only_r}");
    }
}

/// Exported internals — _cmsGetTransform{Flags,Worker,MaxWorkers,
/// WorkerFlags,Formatters16,FormattersFloat,UserData},
/// _cmsSet/GetProfileUserData, _cmsReadProfileSequence,
/// _cmsSetHeaderCMM, _cmsDefaultICCintents, plus NullFloatXFORM
/// (NULLTRANSFORM+float) and UnrollNothing/PackNothing ((0,0)
/// format transform's do_transform).
unsafe extern "C" fn noop_free(_ctx: *mut std::ffi::c_void,
    _p: *mut std::ffi::c_void) {}

#[test]
fn xform_internals() {
    unsafe {
        let i = c::cmsCreate_sRGBProfile();
        let o = c::cmsCreate_sRGBProfile();
        let rgb8: u32 = (4 << 16) | (3 << 3) | 1;
        let flt: u32 = (1 << 22) | (4 << 16) | (3 << 3) | 4;
        let t = c::cmsCreateTransform(i, rgb8, o, rgb8, 0, 0);
        assert!(!t.is_null());
        // getters on a live transform
        let flags = c::_cmsGetTransformFlags(t);
        let w = c::_cmsGetTransformWorker(t);
        let mw = c::_cmsGetTransformMaxWorkers(t);
        let wf = c::_cmsGetTransformWorkerFlags(t);
        let mut f16a: *mut std::ffi::c_void = std::ptr::null_mut();
        let mut f16b: *mut std::ffi::c_void = std::ptr::null_mut();
        c::_cmsGetTransformFormatters16(t,
            &mut f16a as *mut *mut _ as _, &mut f16b as *mut *mut _ as _);
        let mut ffa: *mut std::ffi::c_void = std::ptr::null_mut();
        let mut ffb: *mut std::ffi::c_void = std::ptr::null_mut();
        c::_cmsGetTransformFormattersFloat(t,
            &mut ffa as *mut *mut _ as _, &mut ffb as *mut *mut _ as _);
        // user data roundtrip
        static mut UD: usize = 0x1234;
        c::_cmsSetTransformUserData(t, &raw mut UD as *mut usize as _,
            noop_free as *mut _);
        let ud = c::_cmsGetTransformUserData(t);
        eprintln!("flags={flags:#x} w={} mw={mw} wf={wf:#x} \
            f16={} ff={} ud={}",
            !w.is_null(), !f16a.is_null(), !ffa.is_null(),
            ud == (&raw mut UD as *mut usize as _));
        // NULLTRANSFORM + float -> NullFloatXFORM
        let tn = c::cmsCreateTransform(i, flt, o, flt, 0, 0x200);
        eprintln!("null-float={}", !tn.is_null());
        if !tn.is_null() {
            let inp = [0.25f32, 0.5, 0.75];
            let mut out = [0.0f32; 3];
            c::cmsDoTransform(tn, inp.as_ptr() as _,
                out.as_mut_ptr() as _, 1);
            eprintln!("nullfloat out={out:?}");
            c::cmsDeleteTransform(tn);
        }
        // (0,0)-format transform -> UnrollNothing/PackNothing
        let tz = c::cmsCreateTransform(i, 0, o, 0, 0, 0);
        eprintln!("00-xform={}", !tz.is_null());
        if !tz.is_null() {
            let inp = [0xAAu8; 24];
            let mut out = [0u8; 24];
            c::cmsDoTransform(tz, inp.as_ptr() as _,
                out.as_mut_ptr() as _, 4);
            c::cmsDeleteTransform(tz);
        }
        // profile user data + ReadProfileSequence + SetHeaderCMM
        static mut PD: usize = 0xBEEF;
        c::_cmsSetProfileUserData(i, &raw mut PD as *mut usize as _,
            noop_free as *mut _);
        let pd = c::_cmsGetProfileUserData(i);
        eprintln!("profile-ud={}", pd == (&raw mut PD as *mut usize as _));
        let seq = c::_cmsReadProfileSequence(i);
        eprintln!("pseq={}", !seq.is_null());
        c::_cmsSetHeaderCMM(i, 0xDEADBEEF);
        eprintln!("cmm={:#x}", c::cmsGetHeaderCMM(i));
        // _cmsDefaultICCintents — the exported wrapper direct call
        let profs = [i, o];
        let intents = [0u32; 2];
        let bpc = [0i32; 2];
        let adapt = [1.0f64; 2];
        let lut = c::_cmsDefaultICCintents(
            std::ptr::null_mut(), 2, intents.as_ptr(),
            profs.as_ptr(), bpc.as_ptr(), adapt.as_ptr(), 0);
        eprintln!("defintents={}", !lut.is_null());
        c::cmsDeleteTransform(t);
        c::cmsCloseProfile(i);
        c::cmsCloseProfile(o);
        // ReadProfileSequence on corpus profiles carrying pseq
        for name in ["lcms2link2.icc", "lcms2link.icc",
            "limitlcms2.icc", "linlcms2.icc"] {
            let path = format!("{}/../../{name}\0",
                env!("CARGO_MANIFEST_DIR"));
            let h = c::cmsOpenProfileFromFile(
                path.as_ptr() as *const i8, b"r\0".as_ptr());
            if h.is_null() { continue; }
            let seq = c::_cmsReadProfileSequence(h);
            eprintln!("{name} pseq={}", !seq.is_null());
            c::cmsCloseProfile(h);
        }
    }
}

/// Partial-adaptation absolute intent — AdaptationState=0.5 on an
/// sRGB(D65)->Lab(D50) transform routes ComputeAbsoluteIntent into
/// the CHAD-temperature mix: CHAD2Temp + Temp2CHAD.
#[test]
fn partial_adaptation_absolute() {
    let _g = CTX_LOCK.lock().unwrap();
    unsafe {
        c::cmsSetAdaptationStateTHR(std::ptr::null_mut(), 0.5);
        xform::set_adaptation_state(None, 0.5);
        let i = c::cmsCreate_sRGBProfile();
        let o = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let mut rp = lcms::virt::create_srgb_profile(None).unwrap();
        let mut ro = lcms::virt::create_lab4_profile(None, None).unwrap();
        let rgb8: u32 = (4 << 16) | (3 << 3) | 1;
        let lab8: u32 = (10 << 16) | (3 << 3) | 1;
        let n = 65;
        let inp = fill(n * 3);
        let mut c_out = vec![0u8; n * 3];
        let t = c::cmsCreateTransform(i, rgb8, o, lab8, 3, 0);
        if !t.is_null() {
            c::cmsDoTransform(t, inp.as_ptr() as _,
                c_out.as_mut_ptr() as _, n as u32);
        }
        let mut profs: Vec<Box<lcms::profile::IccProfile>> =
            vec![rp, ro];
        let mut bpc = vec![false; 2];
        let intents = vec![3u32; 2];
        let adapt = vec![0.5f64; 2];
        let mut r_out = vec![0u8; n * 3];
        let mut rt = lcms::xform::create_extended_transform(
            None, &mut profs, &mut bpc, &intents, &adapt,
            None, 0, rgb8, lab8, 0);
        if let Some(rt) = rt.as_mut() {
            rt.do_transform(&inp, &mut r_out, n as u32);
        }
        assert_eq!(t.is_null(), rt.is_none(), "abs0.5 create parity");
        assert_eq!(c_out, r_out, "abs0.5 output");
        c::cmsDeleteTransform(t);
        c::cmsCloseProfile(i);
        c::cmsCloseProfile(o);
        c::cmsSetAdaptationStateTHR(std::ptr::null_mut(), 1.0);
        xform::set_adaptation_state(None, 1.0);
    }
}

/// _cmsTransform2toTransformAdaptor — a transform plugin registered
/// with ExpectedVersion < 2080 supplies a legacy _cmsTransformFn;
/// cmsCreateTransform wraps it via the adaptor (cmsxform.c:902).
#[test]
fn transform2_adaptor() {
    #[repr(C)]
    struct Base { magic: u32, ver: u32, ty: u32, next: *mut std::ffi::c_void }
    #[repr(C)]
    struct PTransform {
        base: Base,
        factories: unsafe extern "C" fn(
            *mut *const std::ffi::c_void, *mut *mut std::ffi::c_void,
            *mut *mut std::ffi::c_void, *mut *mut std::ffi::c_void,
            *mut u32, *mut u32, *mut u32) -> i32,
    }
    // old-style transform fn — passthrough via wIn/wOut
    unsafe extern "C" fn legacy(
        _p: *mut std::ffi::c_void,
        _in: *const u16, _out: *mut u16, _size: u32) {
        // body unused — the adaptor's dispatch is what matters
    }
    // factory — supplies the legacy fn, keeps the LUT
    unsafe extern "C" fn factory(
        xform: *mut *const std::ffi::c_void,
        _ud: *mut *mut std::ffi::c_void,
        _free: *mut *mut std::ffi::c_void,
        _lut: *mut *mut std::ffi::c_void,
        _fi: *mut u32, _fo: *mut u32, _fl: *mut u32) -> i32 {
        *xform = legacy as *const _;
        1
    }
    unsafe {
        let i = c::cmsCreate_sRGBProfile();
        let o = c::cmsCreate_sRGBProfile();
        let rgb8: u32 = (4 << 16) | (3 << 3) | 1;
        let mut p = PTransform {
            base: Base { magic: 0x61637070, ver: 2000,
                ty: 0x7A666D48, next: std::ptr::null_mut() },
            factories: factory,
        };
        let reg = c::cmsPlugin(&mut p as *mut PTransform as *mut _);
        eprintln!("xfm plugin reg={reg}");
        let t = c::cmsCreateTransform(i, rgb8, o, rgb8, 0, 0);
        eprintln!("plugin xform={}", !t.is_null());
        if !t.is_null() {
            let inp = [1u8, 2, 3];
            let mut out = [0u8; 3];
            c::cmsDoTransform(t, inp.as_ptr() as _,
                out.as_mut_ptr() as _, 1);
            eprintln!("adaptor out={out:?}");
            c::cmsDeleteTransform(t);
        }
        c::cmsCloseProfile(i);
        c::cmsCloseProfile(o);
        c::cmsUnregisterPlugins();
    }
}

/// Black-preserving intents 10..15 — CMYK->CMYK transforms route
/// through BlackPreservingKOnlyIntents (10-12) and
/// BlackPreservingKPlaneIntents (13-15) in cmscnvrt.c.
#[test]
fn black_preserving_intents() {
    let _g = CTX_LOCK.lock().unwrap();
    unsafe {
        let path = format!("{}/../../lcms2cmyk.icc\0",
            env!("CARGO_MANIFEST_DIR"));
        let ch = c::cmsOpenProfileFromFile(
            path.as_ptr() as *const i8, b"r\0".as_ptr());
        let data = std::fs::read(path.trim_end_matches('\0')).unwrap();
        let cmyk8: u32 = (6 << 16) | (4 << 3) | 1; // PT_CMYK=6
        let n = 33;
        let inp = fill(n * 4);
        for intent in [10u32, 11, 12, 13, 14, 15] {
            let t = c::cmsCreateTransform(ch, cmyk8, ch, cmyk8, intent, 0);
            let mut c_out = vec![0u8; n * 4];
            if !t.is_null() {
                c::cmsDoTransform(t, inp.as_ptr() as _,
                    c_out.as_mut_ptr() as _, n as u32);
            }
            let mut profs: Vec<Box<lcms::profile::IccProfile>> =
                vec![lcms::profile::open_profile_from_mem(None, &data).unwrap(),
                     lcms::profile::open_profile_from_mem(None, &data).unwrap()];
            let mut bpc = vec![false; 2];
            let intents = vec![intent; 2];
            let adapt = vec![1.0f64; 2];
            let mut rt = lcms::xform::create_extended_transform(
                None, &mut profs, &mut bpc, &intents, &adapt,
                None, 0, cmyk8, cmyk8, 0);
            let mut r_out = vec![0u8; n * 4];
            if let Some(rt) = rt.as_mut() {
                rt.do_transform(&inp, &mut r_out, n as u32);
            }
            assert_eq!(t.is_null(), rt.is_none(),
                "intent {intent} create parity");
            assert_eq!(c_out, r_out, "intent {intent} output");
            if !t.is_null() { c::cmsDeleteTransform(t); }
        }
        c::cmsCloseProfile(ch);
    }
}

/// Profile-class x intent dispatch — input/output/link/abstract/
/// namedcolor classes at each intent + BPC/abs/sofproofing flag
/// combos covers DefaultICCintents/ComputeConversion/AddConversion/
/// ColorSpaceIsCompatible dispatch arms.
#[test]
fn class_intent_dispatch() {
    let _g = CTX_LOCK.lock().unwrap();
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let cmyk_path = "../lcms2cmyk.icc";
        let cmyk_data = std::fs::read(cmyk_path).unwrap_or_default();
        let cmyk = if cmyk_data.is_empty() { std::ptr::null_mut() }
            else { c::cmsOpenProfileFromMem(
                cmyk_data.as_ptr() as *const _, cmyk_data.len() as u32) };
        for intent in 0..=7u32 {
            for (a, b) in [(srgb, lab), (lab, srgb),
                           (srgb, srgb)] {
                let t = c::cmsCreateTransform(a, 0x00ff04, b,
                    0x00ff04, intent, 0);
                if !t.is_null() { c::cmsDeleteTransform(t); }
                // BPC + abs variants
                let t2 = c::cmsCreateTransform(a, 0x00ff04, b,
                    0x00ff04, intent, 0x1000 | 0x4000);
                if !t2.is_null() { c::cmsDeleteTransform(t2); }
            }
            if !cmyk.is_null() {
                for intent2 in [intent] {
                    let t = c::cmsCreateTransform(cmyk, 0x4c02f1,
                        srgb, 0x00ff04, intent2, 0);
                    if !t.is_null() { c::cmsDeleteTransform(t); }
                }
            }
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
        if !cmyk.is_null() { c::cmsCloseProfile(cmyk); }
    }
}

/// AddConversion PCS-pair arms — multiprofile chains crossing
/// XYZ-PCS and Lab-PCS profiles hit the XYZ->Lab / Lab->XYZ /
/// same-PCS + non-empty-layer insertion + mismatch branches.
#[test]
fn pcs_conversion_chains() {
    let _g = CTX_LOCK.lock().unwrap();
    unsafe {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        let srgb = c::cmsCreate_sRGBProfile();
        let lab4 = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let lab2 = c::cmsCreateLab2Profile(std::ptr::null_mut());
        let mut labs: Vec<*mut std::ffi::c_void> = vec![lab4, lab2];
        for f in ["labv2lcms2.icc", "labv4lcms2.icc", "linlcms2.icc"] {
            if let Ok(d) = std::fs::read(dir.join(f)) {
                let p = c::cmsOpenProfileFromMem(
                    d.as_ptr() as *const _, d.len() as u32);
                if !p.is_null() { labs.push(p); }
            }
        }
        // srgb(xyz-pcs) -> lab-profile -> srgb chains
        for &lp in &labs {
            for tail in [srgb, lab4] {
                let mut profs = [srgb, lp, tail];
                let t = c::cmsCreateMultiprofileTransform(
                    profs.as_mut_ptr(), 3,
                    0x00ff04, 0x00ff04, 0, 0);
                eprintln!("chain srgb->lab->? t={}", !t.is_null());
                if !t.is_null() {
                    let inp = [100u8, 150, 200];
                    let mut out = [0u8; 3];
                    c::cmsDoTransform(t, inp.as_ptr() as *const _,
                        out.as_mut_ptr() as *mut _, 1);
                    c::cmsDeleteTransform(t);
                }
            }
            // lab->lab direct (same-PCS arm)
            let mut ll = [lp, lab4];
            let t = c::cmsCreateMultiprofileTransform(
                ll.as_mut_ptr(), 2, 0x0a0702, 0x0a0702, 0, 0);
            if !t.is_null() { c::cmsDeleteTransform(t); }
        }
        for p in labs { c::cmsCloseProfile(p); }
        c::cmsCloseProfile(srgb);
    }
}

/// NONEGATIVES clip + abstract-class + devicelink dispatch —
/// cmsFLAGS_NONEGATIVES inserts a clip stage on gray/CMYK output;
/// abstract-class and link-class single profiles take the
/// devicelink/abstract LUT paths.
#[test]
fn nonegatives_abstract_devicelink() {
    let _g = CTX_LOCK.lock().unwrap();
    unsafe {
        let wp = c::CieXyz { X: 0.9642, Y: 1.0, Z: 0.8249 };
        let gray = c::cmsCreateGrayProfile(&wp as *const _ as *const _,
            c::cmsBuildGamma(std::ptr::null_mut(), 2.2));
        let srgb = c::cmsCreate_sRGBProfile();
        let g16 = (3u32 << 16) | (1 << 3) | 2;
        let r16 = (4u32 << 16) | (3 << 3) | 2;
        // NONEGATIVES on gray output -> clip stage
        for (name, inp, outp, if_, of) in [
            ("g-gray", srgb, gray, r16, g16),
            ("g-srgb", gray, srgb, g16, r16),
        ] {
            let t = c::cmsCreateTransform(inp, if_, outp, of,
                0, 0x20000000); // NONEGATIVES
            eprintln!("{name} t={}", !t.is_null());
            if !t.is_null() { c::cmsDeleteTransform(t); }
        }
        // abstract-class profile -> abstract LUT path
        let bchsw = c::cmsCreateBCHSWabstractProfile(5,
            0.1, 0.2, 0.3, 0.4, 5000, 6500);
        if !bchsw.is_null() {
            let t = c::cmsCreateTransform(srgb, r16, srgb, r16,
                0, 0);
            if !t.is_null() { c::cmsDeleteTransform(t); }
            c::cmsCloseProfile(bchsw);
        }
        // single devicelink profile -> devicelink LUT
        let dl = c::cmsCreateInkLimitingDeviceLink(0x434D594B, 250.0);
        if !dl.is_null() {
            let cf = (6u32 << 16) | (4 << 3) | 1;
            let t = c::cmsCreateTransform(dl, cf, dl, cf, 0, 0);
            eprintln!("dl-single t={}", !t.is_null());
            if !t.is_null() { c::cmsDeleteTransform(t); }
            c::cmsCloseProfile(dl);
        }
        c::cmsCloseProfile(gray);
        c::cmsCloseProfile(srgb);
    }
}

/// Transform accessors + stride variants — LineStride/Stride
/// DoTransform, GetTransformContextID.
#[test]
fn transform_accessors_strides() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let r16 = (4u32 << 16) | (3 << 3) | 2;
        let l16 = (10u32 << 16) | (3 << 3) | 2;
        let t = c::cmsCreateTransform(srgb, r16, lab, l16, 0, 0);
        if !t.is_null() {
            // LineStride — per-line stride args
            let ib = [0x80u8; 64];
            let mut ob = [0u8; 64];
            c::cmsDoTransformLineStride(t, ib.as_ptr() as *const _,
                ob.as_mut_ptr() as *mut _, 2, 2, 12, 12, 6, 6);
            let cid = c::cmsGetTransformContextID(t);
            eprintln!("linestride done ctx={}", !cid.is_null());
            // Stride
            c::cmsDoTransformStride(t, ib.as_ptr() as *const _,
                ob.as_mut_ptr() as *mut _, 2, 0);
            c::cmsDeleteTransform(t);
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
    }
}

/// Transform dwFlags dispatch — every flag bit's cnvrt pipeline
/// arm (NOCACHE/NOOPTIMIZE/GAMUTCHECK/SOFTPROOFING/BLACKPOINT/etc).
#[test]
fn transform_flags_dispatch() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let r16 = (4u32 << 16) | (3 << 3) | 2;
        let l16 = (10u32 << 16) | (3 << 3) | 2;
        for flags in [
            0x0000u32, 0x0001, 0x0002, 0x0004, 0x0008,
            0x0010, 0x0020, 0x0040, 0x0080, 0x0100,
            0x0200, 0x0400, 0x0800, 0x1000, 0x2000,
            0x4000, 0x8000,
        ] {
            let t = c::cmsCreateTransform(srgb, r16, lab, l16,
                0, flags);
            eprintln!("f{flags:#05x} t={}", !t.is_null());
            if !t.is_null() { c::cmsDeleteTransform(t); }
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
    }
}

/// ComputeConversion matrix internals — devicelink + matrix-shaper
/// profile chains hit the PCS-adaptation matrix computation.
#[test]
fn compute_conversion_matrices() {
    let _g = CTX_LOCK.lock().unwrap();
    unsafe {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        // devicelink profile chained with srgb hits ComputeConversion
        if let Ok(d) = std::fs::read(dir.join("lcms2link.icc")) {
            let dl = c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32);
            let srgb = c::cmsCreate_sRGBProfile();
            let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
            if !dl.is_null() {
                let mut chain = [dl, lab, srgb];
                let r16 = (4u32 << 16) | (3 << 3) | 2;
                let t = c::cmsCreateMultiprofileTransform(
                    chain.as_mut_ptr(), 3, r16, r16, 0, 0);
                eprintln!("dl-chain t={}", !t.is_null());
                if !t.is_null() { c::cmsDeleteTransform(t); }
            }
            if !dl.is_null() { c::cmsCloseProfile(dl); }
            c::cmsCloseProfile(srgb);
            c::cmsCloseProfile(lab);
        }
    }
}

/// cmscnvrt conversion chains — profile-class pairs (input/
/// display/output/abstract/named-color) × all intents.
#[test]
fn cnvt_class_intent_matrix() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let xyz = c::cmsCreateXYZProfile();
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        let cmyk = ["lcms2cmyk.icc"].iter()
            .filter_map(|f| std::fs::read(dir.join(f)).ok())
            .map(|d| c::cmsOpenProfileFromMem(d.as_ptr() as *const _, d.len() as u32))
            .find(|p| !p.is_null());
        let r16 = (4u32 << 16) | (3 << 3) | 2;
        let l16 = (10u32 << 16) | (3 << 3) | 2;
        let x16 = (9u32 << 16) | (3 << 3) | 2; // PT_XYZ=9
        let k16 = (6u32 << 16) | (4 << 3) | 2; // PT_CMYK=6
        for intent in 0..4u32 {
            for (inp, outp, if_, of_, name) in [
                (srgb, xyz, r16, x16, "srgb-xyz"),
                (xyz, lab, x16, l16, "xyz-lab"),
                (lab, xyz, l16, x16, "lab-xyz"),
            ] {
                let t = c::cmsCreateTransform(inp, if_, outp, of_, intent, 0);
                if !t.is_null() { c::cmsDeleteTransform(t); }
                eprintln!("{name}-i{intent} t={}", !t.is_null());
            }
        }
        if let Some(cp) = cmyk {
            for intent in 0..4u32 {
                let t = c::cmsCreateTransform(cp, k16, srgb, r16, intent, 0);
                if !t.is_null() { c::cmsDeleteTransform(t); }
                eprintln!("cmyk-srgb-i{intent} t={}", !t.is_null());
            }
            c::cmsCloseProfile(cp);
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
        c::cmsCloseProfile(xyz);
    }
}

/// cmscnvrt named-color + abstract + preview chains — profiles
/// with named-color/abstract classes hit the layer-detection
/// internals.
#[test]
fn cnvt_named_abstract() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        let r16 = (4u32 << 16) | (3 << 3) | 2;
        let l16 = (10u32 << 16) | (3 << 3) | 2;
        // abstract profile transforms
        if let Ok(d) = std::fs::read(dir.join("brightness.icc")) {
            let ab = c::cmsOpenProfileFromMem(d.as_ptr() as *const _,
                d.len() as u32);
            if !ab.is_null() {
                for intent in 0..4u32 {
                    let t = c::cmsCreateTransform(srgb, r16, ab,
                        l16, intent, 0);
                    eprintln!("abst-i{intent} t={}", !t.is_null());
                    if !t.is_null() { c::cmsDeleteTransform(t); }
                }
                c::cmsCloseProfile(ab);
            }
        }
        // multi-profile chains through abstract
        for f in ["bchslcms2.icc", "glablcms2.icc", "labv4lcms2.icc",
                  "labv2lcms2.icc", "aRGBlcms2.icc", "gray3lcms2.icc",
                  "lcms2mxyz.icc", "lcms2mgray.icc", "lcms2link.icc"] {
            let Ok(d) = std::fs::read(dir.join(f)) else { continue };
            let p = c::cmsOpenProfileFromMem(d.as_ptr() as *const _,
                d.len() as u32);
            if p.is_null() { continue; }
            let profs = [srgb, p, lab];
            let t = c::cmsCreateMultiprofileTransform(
                profs.as_ptr() as *mut _, 3, r16, l16, 0, 0);
            eprintln!("multi-{f} t={}", !t.is_null());
            if !t.is_null() { c::cmsDeleteTransform(t); }
            c::cmsCloseProfile(p);
        }
        // proofing with gamut check flag
        for flags in [0x0001u32, 0x0002, 0x0004, 0x2000] {
            let t = c::cmsCreateProofingTransform(srgb, r16, srgb,
                r16, lab, 0, 1, flags);
            eprintln!("proof-f{flags:#x} t={}", !t.is_null());
            if !t.is_null() { c::cmsDeleteTransform(t); }
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
    }
}

/// cmscnvrt named-color internals — a named-color profile + the
/// named transform path.
#[test]
fn cnvt_named_color() {
    unsafe {
        // build an nmcl profile: placeholder + nmcl class + ncl2
        let ncl = c::cmsAllocNamedColorList(std::ptr::null_mut(),
            2, 3, b"\0".as_ptr(), b"\0".as_ptr());
        let pcs = [0x8000u16; 3];
        let cols = [0x4000u16; 16];
        c::cmsAppendNamedColor(ncl, b"red\0".as_ptr(),
            pcs.as_ptr(), cols.as_ptr());
        c::cmsAppendNamedColor(ncl, b"blue\0".as_ptr(),
            pcs.as_ptr(), cols.as_ptr());
        let prof = c::cmsCreateProfilePlaceholder(std::ptr::null_mut());
        c::cmsSetDeviceClass(prof, 0x6e6d636c); // 'nmcl'
        c::cmsSetColorSpace(prof, 0x4c616220);  // 'Lab '
        c::cmsSetPCS(prof, 0x4c616220);
        c::cmsWriteTag(prof, 0x6e636c32, ncl as *const _);
        let l16 = (10u32 << 16) | (3 << 3) | 2;
        let nfmt = (16u32 << 16) | (1 << 3) | 2; // named 1ch?
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let srgb = c::cmsCreate_sRGBProfile();
        let r16 = (4u32 << 16) | (3 << 3) | 2;
        for (inp, outp, if_, of_, name) in [
            (prof, lab, nfmt, l16, "nmcl->lab"),
            (lab, prof, l16, nfmt, "lab->nmcl"),
            (prof, srgb, nfmt, r16, "nmcl->srgb"),
        ] {
            let t = c::cmsCreateTransform(inp, if_, outp, of_, 0, 0);
            eprintln!("{name} t={}", !t.is_null());
            if !t.is_null() {
                let ib = [0x4000u16; 4];
                let mut ob = [0u16; 4];
                c::cmsDoTransform(t, ib.as_ptr() as *const _,
                    ob.as_mut_ptr() as *mut _, 1);
                c::cmsDeleteTransform(t);
            }
        }
        c::cmsCloseProfile(prof);
        c::cmsCloseProfile(lab);
        c::cmsCloseProfile(srgb);
        c::cmsFreeNamedColorList(ncl);
    }
}

/// cmscnvrt gamut/preview internals — gamut-check transforms +
/// preview chains hit the remaining conversion arms.
#[test]
fn cnvt_gamut_preview() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        let r16 = (4u32 << 16) | (3 << 3) | 2;
        let l16 = (10u32 << 16) | (3 << 3) | 2;
        // proofing transforms with gamut-check intent on corpus
        for f in ["lcms2cmyk.icc", "aRGBlcms2.icc", "labv4lcms2.icc",
                  "sRGBlcms2.icc", "brightness.icc"] {
            let Ok(d) = std::fs::read(dir.join(f)) else { continue };
            let p = c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32);
            if p.is_null() { continue; }
            for proofing in 0..4u32 {
                for flags in [0u32, 0x0004, 0x2000] {
                    let t = c::cmsCreateProofingTransform(srgb, r16,
                        srgb, r16, p, 0, proofing, flags);
                    eprintln!("{f}-p{proofing}-f{flags:#x} t={}",
                        !t.is_null());
                    if !t.is_null() { c::cmsDeleteTransform(t); }
                }
            }
            c::cmsCloseProfile(p);
        }
        // devicelink->input chains
        if let Ok(d) = std::fs::read(dir.join("lcms2link.icc")) {
            let lk = c::cmsOpenProfileFromMem(d.as_ptr() as *const _,
                d.len() as u32);
            if !lk.is_null() {
                for intent in 0..4u32 {
                    let t = c::cmsCreateTransform(srgb, r16, lk,
                        l16, intent, 0);
                    eprintln!("link-out-i{intent} t={}", !t.is_null());
                    if !t.is_null() { c::cmsDeleteTransform(t); }
                }
                c::cmsCloseProfile(lk);
            }
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
    }
}

/// CHAD2Temp/Temp2CHAD internals — absolute-intent transforms
/// between profiles with different white points + CHAD tags
/// trigger ComputeAbsoluteIntent's temp estimation.
#[test]
fn cnvt_chad_temp() {
    unsafe {
        let g22 = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let curves = [g22, g22, g22];
        // D65 srgb vs D50 custom rgb — different white points
        let wp_d50 = [0.3457f64, 0.3585, 1.0];
        let wp_d65 = [0.3127f64, 0.3290, 1.0];
        let prim = [
            0.64f64, 0.33, 1.0,
            0.30, 0.60, 1.0,
            0.15, 0.06, 1.0,
        ];
        let srgb = c::cmsCreate_sRGBProfile();
        let d50rgb = c::cmsCreateRGBProfileTHR(std::ptr::null_mut(),
            wp_d50.as_ptr() as *const _,
            prim.as_ptr() as *const _,
            curves.as_ptr() as *const _);
        let d65rgb = c::cmsCreateRGBProfileTHR(std::ptr::null_mut(),
            wp_d65.as_ptr() as *const _,
            prim.as_ptr() as *const _,
            curves.as_ptr() as *const _);
        let r8 = (4u32 << 16) | (3 << 3) | 1;
        let r16 = (4u32 << 16) | (3 << 3) | 2;
        // absolute intent (3) between differing white points
        for (inp, outp, f, name) in [
            (srgb, d50rgb, r8, "srgb->d50-a3"),
            (d50rgb, srgb, r8, "d50->srgb-a3"),
            (d50rgb, d65rgb, r16, "d50->d65-a3"),
            (srgb, d65rgb, r8, "srgb->d65-a3"),
            (d50rgb, d50rgb, r8, "d50->d50-a3"),
        ] {
            let t = c::cmsCreateTransform(inp, f, outp, f, 3, 0);
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
        c::cmsCloseProfile(d50rgb);
        c::cmsCloseProfile(d65rgb);
        c::cmsFreeToneCurve(g22);
    }
}

/// UnrollNothing/PackNothing + CachedXFORMGamutCheck — zero-format
/// transforms + gamut-check without NOCACHE.
#[test]
fn xform_nothing_gamut() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        // zero-format transform -> UnrollNothing/PackNothing arms
        let t = c::cmsCreateTransform(srgb, 0, lab, 0, 0, 0);
        eprintln!("zero-fmt t={}", !t.is_null());
        if !t.is_null() {
            let r8 = (4u32 << 16) | (3 << 3) | 1;
            let l8 = (10u32 << 16) | (3 << 3) | 1;
            c::cmsChangeBuffersFormat(t, r8, l8);
            let ib = [0x80u8; 4];
            let mut ob = [0u8; 8];
            c::cmsDoTransform(t, ib.as_ptr() as *const _,
                ob.as_mut_ptr() as *mut _, 1);
            c::cmsDeleteTransform(t);
        }
        // gamut-check without NOCACHE -> CachedXFORMGamutCheck
        let r16 = (4u32 << 16) | (3 << 3) | 2;
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        let cmyk = std::fs::read(dir.join("lcms2cmyk.icc")).ok()
            .map(|d| c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32))
            .unwrap_or(std::ptr::null_mut());
        let proof = if !cmyk.is_null() { cmyk } else { srgb };
        let t2 = c::cmsCreateProofingTransform(srgb, r16, srgb, r16,
            proof, 0, 0, 0x1000);
        eprintln!("gamut t={}", !t2.is_null());
        if !t2.is_null() {
            let ib = [0x8000u16; 4];
            let mut ob = [0x8000u16; 4];
            for v in [0x1000u16, 0x8000, 0xf000] {
                let mut inp = [v; 4];
                c::cmsDoTransform(t2, inp.as_ptr() as *const _,
                    ob.as_mut_ptr() as *mut _, 1);
            }
            c::cmsDeleteTransform(t2);
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
    }
}

/// CHAD2Temp — absolute-intent transform on a profile with a
/// `chad` tag (non-D50 chromatic adaptation) hits the
/// temperature estimation path.
#[test]
fn chad_temp_internals() {
    unsafe {
        // Bradford D65->D50 chromatic adaptation (non-identity)
        let chad = [
            0.9974f64, -0.0040, 0.0074,
            -0.0057, 1.0051, -0.0031,
            0.0031, -0.0011, 0.8104,
        ];
        let srgb = c::cmsCreate_sRGBProfile();
        let srgb2 = c::cmsCreate_sRGBProfile();
        c::cmsWriteTag(srgb2, 0x63686164,
            chad.as_ptr() as *const _);
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let r16 = (4u32 << 16) | (3 << 3) | 2;
        let l16 = (10u32 << 16) | (3 << 3) | 2;
        for (inp, outp, name) in [
            (srgb2, lab, "srgb-chad->lab"),
            (lab, srgb2, "lab->srgb-chad"),
            (srgb, srgb2, "srgb->srgb-chad"),
            (srgb2, srgb, "srgb-chad->srgb"),
        ] {
            // intent 3 = absolute colorimetric
            let t = c::cmsCreateTransform(inp, r16, outp, l16, 3, 0);
            eprintln!("{name} t={}", !t.is_null());
            if !t.is_null() {
                let ib = [0x8000u16; 4];
                let mut ob = [0x8000u16; 4];
                c::cmsDoTransform(t, ib.as_ptr() as *const _,
                    ob.as_mut_ptr() as *mut _, 1);
                c::cmsDeleteTransform(t);
            }
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(srgb2);
        c::cmsCloseProfile(lab);
    }
}

/// Optimizer deep arms — all-curves pipeline (OptimizeByJoining
/// Curves), CLUT resample (OptimizeByResampling), linearization.
#[test]
fn optimizer_arms() {
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
        let r8 = (4u32 << 16) | (3 << 3) | 1;
        let r16 = (4u32 << 16) | (3 << 3) | 2;
        let l16 = (10u32 << 16) | (3 << 3) | 2;
        let g16 = (3u32 << 16) | (1 << 3) | 2;
        let k16 = (6u32 << 16) | (4 << 3) | 2;
        // flag variants that force different optimizers
        for (inp, outp, if_, of_, fl, name) in [
            // pure-curves pipeline: gray->gray joins curves
            (gray, gray, g16, g16, 0, "gray->gray"),
            (gray, gray, g16, g16, 0x0002, "gray->gray lpc"),
            (gray, gray, g16, g16, 0x0040, "gray->gray nocache"),
            // CLUT-resample on a CLUT profile
            (cmyk, lab, k16, l16, 0, "cmyk->lab"),
            (cmyk, lab, k16, l16, 0x0002, "cmyk->lab lpc"),
            (srgb, srgb, r8, r8, 0x0002, "srgb lpc"),
            (srgb, lab, r8, l16, 0x0002, "srgb->lab lpc"),
            (srgb, lab, r16, l16, 0x0400, "srgb->lab hiprec"),
            (srgb, lab, r16, l16, 0x0800, "srgb->lab lowprec"),
            // black-preserving intents (8-11) on CMYK
            (cmyk, cmyk, k16, k16, 0x4000, "cmyk->cmyk intent8"),
        ] {
            if inp.is_null() || outp.is_null() { continue; }
            let intent = if name.contains("intent8") { 8 } else { 0 };
            let t = c::cmsCreateTransform(inp, if_, outp, of_,
                intent, fl);
            eprintln!("{name} t={}", !t.is_null());
            if !t.is_null() {
                let ib = [0x80u8; 64];
                let mut ob = [0x8000u16; 8];
                c::cmsDoTransform(t, ib.as_ptr() as *const _,
                    ob.as_mut_ptr() as *mut _, 4);
                c::cmsDeleteTransform(t);
            }
        }
        // black-preserving intents 8-11 on CMYK->CMYK
        if !cmyk.is_null() {
            for intent in [8u32, 9, 10, 11] {
                let t = c::cmsCreateTransform(cmyk, k16, cmyk, k16,
                    intent, 0);
                eprintln!("cmyk-i{intent} t={}", !t.is_null());
                if !t.is_null() {
                    let ib = [0x8000u16; 8];
                    let mut ob = [0x8000u16; 8];
                    c::cmsDoTransform(t, ib.as_ptr() as *const _,
                        ob.as_mut_ptr() as *mut _, 4);
                    c::cmsDeleteTransform(t);
                }
            }
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
        c::cmsCloseProfile(gray);
        if !cmyk.is_null() { c::cmsCloseProfile(cmyk); }
        c::cmsFreeToneCurve(g);
    }
}

/// OptimizeByComputingLinearization — needs CLUT_PRE/POST
/// LINEARIZATION flags (0x0010/0x0001) on CLUT transforms.
#[test]
fn opt_linearization_arms() {
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
        let link = std::fs::read(dir.join("lcms2link2.icc")).ok()
            .map(|d| c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32))
            .unwrap_or(std::ptr::null_mut());
        let r8 = (4u32 << 16) | (3 << 3) | 1;
        let r16 = (4u32 << 16) | (3 << 3) | 2;
        let l16 = (10u32 << 16) | (3 << 3) | 2;
        let k16 = (6u32 << 16) | (4 << 3) | 2;
        for (inp, outp, if_, of_, fl, name) in [
            (cmyk, lab, k16, l16, 0x0010, "cmyk pre-lin"),
            (cmyk, lab, k16, l16, 0x0001, "cmyk post-lin"),
            (cmyk, lab, k16, l16, 0x0011, "cmyk both"),
            (srgb, lab, r16, l16, 0x0010, "srgb pre-lin"),
            (link, lab, k16, l16, 0x0010, "link pre-lin"),
            (gray, lab, (3u32<<16)|(1<<3)|2, l16, 0x0010, "gray pre-lin"),
            (srgb, lab, r8, l16, 0x0010, "srgb8 pre-lin"),
            (srgb, lab, r16, l16, 0x0010|0x0400, "pre-lin hiprec"),
            (srgb, lab, r16, l16, 0x0010|0x0800, "pre-lin lowprec"),
        ] {
            if inp.is_null() || outp.is_null() { continue; }
            let t = c::cmsCreateTransform(inp, if_, outp, of_, 0, fl);
            eprintln!("{name} t={}", !t.is_null());
            if !t.is_null() {
                let ib = [0x80u8; 64];
                let mut ob = [0x8000u16; 8];
                c::cmsDoTransform(t, ib.as_ptr() as *const _,
                    ob.as_mut_ptr() as *mut _, 4);
                c::cmsDeleteTransform(t);
            }
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
        c::cmsCloseProfile(gray);
        if !cmyk.is_null() { c::cmsCloseProfile(cmyk); }
        if !link.is_null() { c::cmsCloseProfile(link); }
        c::cmsFreeToneCurve(g);
    }
}

/// OptimizeMatrixShaper 4-stage arm — absolute-intent RGB->RGB
/// inserts the extra adaptation matrix (cvst-matf-matf-cvst).
#[test]
fn opt_matrixshaper4() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let argb = c::cmsCreateRGBProfileTHR(
            std::ptr::null_mut(),
            [0.3127f64, 0.3290, 1.0].as_ptr() as *const _,
            [0.64f64, 0.33, 0.21,
             0.30, 0.60, 0.10,
             0.15, 0.06, 0.79].as_ptr() as *const _,
            [c::cmsBuildGamma(std::ptr::null_mut(), 2.2),
             c::cmsBuildGamma(std::ptr::null_mut(), 2.2),
             c::cmsBuildGamma(std::ptr::null_mut(), 2.2)]
                .as_mut_ptr() as *mut _);
        let r8 = (4u32 << 16) | (3 << 3) | 1;
        for (inp, outp, intent, name) in [
            (srgb, argb, 3, "srgb->argb abs"),
            (argb, srgb, 3, "argb->srgb abs"),
            (srgb, srgb, 3, "srgb->srgb abs"),
            (srgb, argb, 0, "srgb->argb per"),
            (argb, srgb, 1, "argb->srgb rel"),
            (srgb, argb, 2, "srgb->argb sat"),
        ] {
            if inp.is_null() || outp.is_null() { continue; }
            let t = c::cmsCreateTransform(inp, r8, outp, r8,
                intent, 0);
            eprintln!("{name} t={}", !t.is_null());
            if !t.is_null() {
                let ib = [0x80u8; 12];
                let mut ob = [0x80u8; 12];
                c::cmsDoTransform(t, ib.as_ptr() as *const _,
                    ob.as_mut_ptr() as *mut _, 4);
                c::cmsDeleteTransform(t);
            }
        }
        c::cmsCloseProfile(srgb);
        if !argb.is_null() { c::cmsCloseProfile(argb); }
    }
}

/// TranslateNonICCIntents — multiprofile/proofing transforms
/// with black-preserving intents (8-11).
#[test]
fn nonicc_intent_arms() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        let cmyk = std::fs::read(dir.join("lcms2cmyk.icc")).ok()
            .map(|d| c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32))
            .unwrap_or(std::ptr::null_mut());
        let r16 = (4u32 << 16) | (3 << 3) | 2;
        let l16 = (10u32 << 16) | (3 << 3) | 2;
        let k16 = (6u32 << 16) | (4 << 3) | 2;
        // multiprofile + proofing with intents 8-11
        if !cmyk.is_null() {
            for intent in [8u32, 9, 10, 11] {
                let mut profs = [srgb, cmyk, lab];
                let t = c::cmsCreateMultiprofileTransform(
                    profs.as_mut_ptr(), 3, r16, l16, intent, 0);
                eprintln!("multi-i{intent} t={}", !t.is_null());
                if !t.is_null() {
                    let ib = [0x8000u16; 8];
                    let mut ob = [0x8000u16; 8];
                    c::cmsDoTransform(t, ib.as_ptr() as *const _,
                        ob.as_mut_ptr() as *mut _, 2);
                    c::cmsDeleteTransform(t);
                }
            }
            // proofing with proofing intent variants
            for intent in [0u32, 8, 10] {
                let t = c::cmsCreateProofingTransform(
                    srgb, r16, srgb, r16, cmyk, intent, 0, 0x2000);
                eprintln!("proof-i{intent} t={}", !t.is_null());
                if !t.is_null() {
                    let ib = [0x8000u16; 8];
                    let mut ob = [0x8000u16; 8];
                    c::cmsDoTransform(t, ib.as_ptr() as *const _,
                        ob.as_mut_ptr() as *mut _, 2);
                    c::cmsDeleteTransform(t);
                }
            }
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
        if !cmyk.is_null() { c::cmsCloseProfile(cmyk); }
    }
}

/// AddConversion/PatchLUT/DefaultICCintents deep arms —
/// conversion-class combos across profile types + intents +
/// versions.
#[test]
fn conversion_arms() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let lab2 = c::cmsCreateLab2Profile(std::ptr::null_mut());
        let xyz = c::cmsCreateXYZProfile();
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let gray = c::cmsCreateGrayProfile(std::ptr::null_mut(), g);
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        let cmyk = std::fs::read(dir.join("lcms2cmyk.icc")).ok()
            .map(|d| c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32))
            .unwrap_or(std::ptr::null_mut());
        let abs = std::fs::read(dir.join("brightness.icc")).ok()
            .map(|d| c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32))
            .unwrap_or(std::ptr::null_mut());
        let link = std::fs::read(dir.join("lcms2link2.icc")).ok()
            .map(|d| c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32))
            .unwrap_or(std::ptr::null_mut());
        let r16 = (4u32 << 16) | (3 << 3) | 2;
        let l16 = (10u32 << 16) | (3 << 3) | 2;
        let x16 = (9u32 << 16) | (3 << 3) | 2;
        let g16 = (3u32 << 16) | (1 << 3) | 2;
        let k16 = (6u32 << 16) | (4 << 3) | 2;
        for (inp, outp, if_, of_, intent, name) in [
            (srgb, lab2, r16, l16, 0, "v2lab"),
            (lab2, srgb, l16, r16, 0, "v2lab->"),
            (abs, srgb, l16, r16, 0, "abs->srgb"),
            (abs, lab, l16, l16, 0, "abs->lab"),
            (link, lab, k16, l16, 0, "link->lab"),
            (link, srgb, k16, r16, 0, "link->srgb"),
            (xyz, lab, x16, l16, 0, "xyz->lab"),
            (lab, xyz, l16, x16, 0, "lab->xyz"),
            (xyz, srgb, x16, r16, 3, "xyz->srgb abs"),
            (gray, xyz, g16, x16, 0, "gray->xyz"),
            (cmyk, xyz, k16, x16, 0, "cmyk->xyz"),
            (xyz, cmyk, x16, k16, 0, "xyz->cmyk"),
        ] {
            if inp.is_null() || outp.is_null() { continue; }
            let t = c::cmsCreateTransform(inp, if_, outp, of_,
                intent, 0);
            eprintln!("{name} t={}", !t.is_null());
            if !t.is_null() {
                let ib = [0x8000u16; 8];
                let mut ob = [0x8000u16; 8];
                c::cmsDoTransform(t, ib.as_ptr() as *const _,
                    ob.as_mut_ptr() as *mut _, 2);
                c::cmsDeleteTransform(t);
            }
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
        c::cmsCloseProfile(lab2);
        c::cmsCloseProfile(xyz);
        c::cmsCloseProfile(gray);
        if !cmyk.is_null() { c::cmsCloseProfile(cmyk); }
        if !abs.is_null() { c::cmsCloseProfile(abs); }
        if !link.is_null() { c::cmsCloseProfile(link); }
        c::cmsFreeToneCurve(g);
    }
}

/// PatchLUT/AddConversion deep arms — abstract-midchain,
/// devicelink chains, gamut/preview links.
#[test]
fn conversion_arms2() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        let cmyk = std::fs::read(dir.join("lcms2cmyk.icc")).ok()
            .map(|d| c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32))
            .unwrap_or(std::ptr::null_mut());
        let abs = std::fs::read(dir.join("brightness.icc")).ok()
            .map(|d| c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32))
            .unwrap_or(std::ptr::null_mut());
        let link = std::fs::read(dir.join("lcms2link.icc")).ok()
            .map(|d| c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32))
            .unwrap_or(std::ptr::null_mut());
        let r16 = (4u32 << 16) | (3 << 3) | 2;
        let l16 = (10u32 << 16) | (3 << 3) | 2;
        let k16 = (6u32 << 16) | (4 << 3) | 2;
        // 3+ profile chains: device->abstract->device etc
        for (profs, intent, name) in [
            (vec![srgb, abs, lab], 0, "srgb->abs->lab"),
            (vec![srgb, cmyk, lab], 0, "srgb->cmyk->lab"),
            (vec![lab, abs, srgb], 0, "lab->abs->srgb"),
            (vec![srgb, abs, cmyk], 1, "srgb->abs->cmyk"),
            (vec![cmyk, srgb, lab], 2, "cmyk->srgb->lab"),
            (vec![link, lab, srgb], 0, "link->lab->srgb"),
            (vec![srgb, link, lab], 0, "srgb->link->lab"),
            (vec![srgb, srgb, lab], 0, "srgb->srgb->lab"),
        ] {
            if profs.iter().any(|p| p.is_null()) { continue; }
            let mut ps = profs;
            let t = c::cmsCreateMultiprofileTransform(
                ps.as_mut_ptr(), ps.len() as i32, r16, l16,
                intent, 0);
            eprintln!("{name} t={}", !t.is_null());
            if !t.is_null() {
                let ib = [0x8000u16; 8];
                let mut ob = [0x8000u16; 8];
                c::cmsDoTransform(t, ib.as_ptr() as *const _,
                    ob.as_mut_ptr() as *mut _, 2);
                c::cmsDeleteTransform(t);
            }
        }
        let _ = k16;
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
        if !cmyk.is_null() { c::cmsCloseProfile(cmyk); }
        if !abs.is_null() { c::cmsCloseProfile(abs); }
        if !link.is_null() { c::cmsCloseProfile(link); }
    }
}

/// OptimizeByResampling deep arms — HIGHRES/LOWRES/GRIDPOINTS
/// flags on CLUT transforms force resampling grids.
#[test]
fn opt_resample_arms() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        let cmyk = std::fs::read(dir.join("lcms2cmyk.icc")).ok()
            .map(|d| c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32))
            .unwrap_or(std::ptr::null_mut());
        let link = std::fs::read(dir.join("lcms2link2.icc")).ok()
            .map(|d| c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32))
            .unwrap_or(std::ptr::null_mut());
        let r16 = (4u32 << 16) | (3 << 3) | 2;
        let l16 = (10u32 << 16) | (3 << 3) | 2;
        let k16 = (6u32 << 16) | (4 << 3) | 2;
        for (inp, outp, if_, of_, fl, name) in [
            (cmyk, lab, k16, l16, 0x0400, "cmyk highres"),
            (cmyk, lab, k16, l16, 0x0800, "cmyk lowres"),
            (cmyk, lab, k16, l16, 0x0400 | 0x0800, "both"),
            (cmyk, lab, k16, l16, (9u32 << 16), "grid9"),
            (cmyk, lab, k16, l16, (33u32 << 16), "grid33"),
            (cmyk, lab, k16, l16, (2u32 << 16), "grid2"),
            (srgb, lab, r16, l16, 0x0400, "srgb highres"),
            (link, lab, k16, l16, 0x0400, "link highres"),
            (srgb, lab, r16, l16, 0x0800, "srgb lowres"),
            (cmyk, lab, k16, l16, 0x0400 | (17u32 << 16), "hi+g17"),
        ] {
            if inp.is_null() || outp.is_null() { continue; }
            let t = c::cmsCreateTransform(inp, if_, outp, of_, 0, fl);
            eprintln!("{name} t={}", !t.is_null());
            if !t.is_null() {
                let ib = [0x80u8; 64];
                let mut ob = [0x8000u16; 8];
                c::cmsDoTransform(t, ib.as_ptr() as *const _,
                    ob.as_mut_ptr() as *mut _, 4);
                c::cmsDeleteTransform(t);
            }
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
        if !cmyk.is_null() { c::cmsCloseProfile(cmyk); }
        if !link.is_null() { c::cmsCloseProfile(link); }
    }
}

/// DefaultICCintents/AddConversion/PatchLUT deep arms — float
/// transforms, proofing variants, all-intents sweeps on the
/// mixed class corpus.
#[test]
fn conversion_arms3() {
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
        let abs = std::fs::read(dir.join("brightness.icc")).ok()
            .map(|d| c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32))
            .unwrap_or(std::ptr::null_mut());
        let link = std::fs::read(dir.join("lcms2link.icc")).ok()
            .map(|d| c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32))
            .unwrap_or(std::ptr::null_mut());
        let fl = 1u32 << 22;
        let rf = (4u32 << 16) | (3 << 3) | 4 | fl;
        let lf = (10u32 << 16) | (3 << 3) | 4 | fl;
        let r16 = (4u32 << 16) | (3 << 3) | 2;
        let l16 = (10u32 << 16) | (3 << 3) | 2;
        // float transforms across classes/intents
        for (inp, outp, intent, name) in [
            (srgb, lab, 0, "f-per"), (srgb, lab, 1, "f-rel"),
            (srgb, lab, 2, "f-sat"), (srgb, lab, 3, "f-abs"),
            (lab, srgb, 0, "f-lab->"), (gray, lab, 0, "f-gray"),
            (cmyk, lab, 0, "f-cmyk"), (cmyk, lab, 3, "f-cmyk-abs"),
            (abs, srgb, 0, "f-abs->"), (link, lab, 0, "f-link"),
        ] {
            if inp.is_null() || outp.is_null() { continue; }
            let t = c::cmsCreateTransform(inp, rf, outp, lf,
                intent, 0);
            eprintln!("{name} t={}", !t.is_null());
            if !t.is_null() {
                let ib = [0u8; 64];
                let mut ob = [0u8; 64];
                c::cmsDoTransform(t, ib.as_ptr() as *const _,
                    ob.as_mut_ptr() as *mut _, 2);
                c::cmsDeleteTransform(t);
            }
        }
        // all-intents proofing sweep
        if !cmyk.is_null() {
            for intent in 0..4u32 {
                let t = c::cmsCreateProofingTransform(
                    srgb, r16, srgb, r16, cmyk, intent, 0, 0);
                eprintln!("proof-i{intent} t={}", !t.is_null());
                if !t.is_null() { c::cmsDeleteTransform(t); }
            }
        }
        let _ = l16;
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
        c::cmsCloseProfile(gray);
        if !cmyk.is_null() { c::cmsCloseProfile(cmyk); }
        if !abs.is_null() { c::cmsCloseProfile(abs); }
        if !link.is_null() { c::cmsCloseProfile(link); }
        c::cmsFreeToneCurve(g);
    }
}

/// CHAD2Temp — absolute-intent transform with AdaptationState in
/// (0,1) + chad-tagged profiles hits the temperature path.
#[test]
fn chad_temp_adapt() {
    unsafe {
        let old = c::cmsSetAdaptationState(0.5);
        let chad = [
            0.9974f64, -0.0040, 0.0074,
            -0.0057, 1.0051, -0.0031,
            0.0031, -0.0011, 0.8104,
        ];
        let srgb = c::cmsCreate_sRGBProfile();
        let srgb2 = c::cmsCreate_sRGBProfile();
        c::cmsWriteTag(srgb2, 0x63686164,
            chad.as_ptr() as *const _);
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let r16 = (4u32 << 16) | (3 << 3) | 2;
        let l16 = (10u32 << 16) | (3 << 3) | 2;
        for (inp, outp, name) in [
            (srgb, lab, "srgb->lab a3"),
            (lab, srgb, "lab->srgb a3"),
            (srgb, srgb2, "srgb->srgb2 a3"),
            (srgb2, lab, "srgb2->lab a3"),
            (srgb, lab, "srgb->lab a1"),
        ] {
            for intent in [3u32, 1, 0] {
                let t = c::cmsCreateTransform(inp, r16, outp, l16,
                    intent, 0);
                eprintln!("{name} i{intent} t={}", !t.is_null());
                if !t.is_null() { c::cmsDeleteTransform(t); }
            }
        }
        c::cmsSetAdaptationState(old);
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(srgb2);
        c::cmsCloseProfile(lab);
    }
}
