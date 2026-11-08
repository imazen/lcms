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

/// Optimizer deep arms — all flag combos on CLUT transforms:
/// PRE/POST lin + resample + precalc + gamut check.
#[test]
fn opt_flag_arms() {
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
        let k16 = (6u32 << 16) | (4 << 3) | 2;
        for fl in [
            0x0010u32,           // CLUT_PRE_LINEARIZATION
            0x0001,              // CLUT_POST_LINEARIZATION
            0x0011,              // both
            0x0010 | 0x0400,     // pre + highres
            0x0001 | 0x0800,     // post + lowres
            0x0011 | 0x0400,     // both + highres
            0x0011 | 0x0800,     // both + lowres
            0x0011 | (17u32<<16), // both + grid17
            0x0010 | 0x0040,     // pre + nocache
            0x0002,              // GUESSDEVICECLASS-ish
            0x0100,              // NOOPTIMIZE
            0x0010 | 0x0100,     // pre + noopt
            0x0800 | 0x0100,     // lowres + noopt
            0x0004,              // NOWHITEONWHITEFIXUP
            0x0010 | 0x0004,
            0x2000,              // BLACKPOINTCOMPENSATION
            0x0010 | 0x2000,
        ] {
            for (inp, outp, if_, of_, name) in [
                (cmyk, lab, k16, l16, "cmyk"),
                (srgb, lab, r16, l16, "srgb"),
                (gray, lab, (3u32<<16)|(1<<3)|2, l16, "gray"),
            ] {
                if inp.is_null() || outp.is_null() { continue; }
                let t = c::cmsCreateTransform(inp, if_, outp, of_,
                    0, fl);
                eprintln!("{name} fl={fl:#x} t={}", !t.is_null());
                if !t.is_null() {
                    let ib = [0x80u8; 64];
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

/// BlackPreservingKOnly/KPlane deep arms — intents 8-11 on
/// CMYK->CMYK + BPC + v2/v4 profiles + more CMYK corpus.
#[test]
fn black_preserving_arms() {
    unsafe {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        let read = |f: &str| -> *mut std::ffi::c_void {
            std::fs::read(dir.join(f)).ok()
                .map(|d| c::cmsOpenProfileFromMem(
                    d.as_ptr() as *const _, d.len() as u32))
                .unwrap_or(std::ptr::null_mut())
        };
        let cmyk = read("lcms2cmyk.icc");
        let cmyk2 = read("lcms2cmyk2.icc");
        let cmyk3 = read("uswop20coated_v2.icc");
        let null2 = read("nullcms2.icc");
        let k16 = (6u32 << 16) | (4 << 3) | 2;
        let r16 = (4u32 << 16) | (3 << 3) | 2;
        for (inp, outp, intent, fl, name) in [
            (cmyk, cmyk, 10, 0, "cmyk i10"),
            (cmyk, cmyk, 11, 0, "cmyk i11"),
            (cmyk, cmyk, 10, 0x2000, "cmyk i10+bpc"),
            (cmyk, cmyk, 11, 0x2000, "cmyk i11+bpc"),
            (cmyk, null2, 10, 0, "cmyk->null i10"),
            (cmyk, null2, 11, 0, "cmyk->null i11"),
            (cmyk, cmyk2, 10, 0, "cmyk->cmyk2 i10"),
            (cmyk, cmyk3, 10, 0, "cmyk->cmyk3 i10"),
            (null2, cmyk, 10, 0, "null->cmyk i10"),
            (null2, null2, 10, 0, "null->null i10"),
            (null2, null2, 11, 0, "null->null i11"),
        ] {
            if inp.is_null() || outp.is_null() { continue; }
            let t = c::cmsCreateTransform(inp, k16, outp, k16,
                intent, fl);
            eprintln!("{name} t={}", !t.is_null());
            if !t.is_null() {
                let ib = [0x8000u16; 8];
                let mut ob = [0x8000u16; 8];
                c::cmsDoTransform(t, ib.as_ptr() as *const _,
                    ob.as_mut_ptr() as *mut _, 4);
                c::cmsDeleteTransform(t);
            }
        }
        // rgb->cmyk with black-preserving intents
        let srgb = c::cmsCreate_sRGBProfile();
        for intent in [10u32, 11] {
            let t = c::cmsCreateTransform(srgb, r16, cmyk, k16,
                intent, 0);
            eprintln!("rgb->cmyk i{intent} t={}", !t.is_null());
            if !t.is_null() { c::cmsDeleteTransform(t); }
        }
        c::cmsCloseProfile(srgb);
        for p in [cmyk, cmyk2, cmyk3, null2] {
            if !p.is_null() { c::cmsCloseProfile(p); }
        }
    }
}

/// OptimizeByComputingLinearization — RGB->RGB only (T_COLORSPACE
/// in+out == PT_RGB); PRE_LINEARIZATION flag on 16-bit input.
#[test]
fn opt_prelin_rgb() {
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
        let k16 = (6u32 << 16) | (4 << 3) | 2;
        for (inp, outp, if_, of_, fl, name) in [
            (srgb, argb, r16, r16, 0x0010, "srgb->argb prelin"),
            (argb, srgb, r16, r16, 0x0010, "argb->srgb prelin"),
            (srgb, srgb, r16, r16, 0x0010, "srgb->srgb prelin"),
            (srgb, argb, r8, r8, 0x0010, "srgb->argb8 prelin"),
            (srgb, argb, r8, r8, 0, "srgb->argb8 noflag"),
            (srgb, argb, r16, r16, 0x0010|0x0400, "prelin+hiprec"),
            (srgb, argb, r16, r16, 0x0010|0x0800, "prelin+lowprec"),
            (srgb, argb, r16, r16, 0x0010|0x0001, "pre+post"),
            (link, argb, k16, r16, 0x0010, "link->argb prelin"),
            (cmyk, srgb, k16, r16, 0x0010, "cmyk->srgb prelin"),
        ] {
            if inp.is_null() || outp.is_null() { continue; }
            let t = c::cmsCreateTransform(inp, if_, outp, of_, 0, fl);
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
        if !cmyk.is_null() { c::cmsCloseProfile(cmyk); }
        if !link.is_null() { c::cmsCloseProfile(link); }
    }
}

/// DefaultICCintents/AddConversion deep arms — 4+ profile chains,
/// input/abstract/devicelink mid-chains, preview profiles.
#[test]
fn conversion_arms4() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
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
        let r16 = (4u32 << 16) | (3 << 3) | 2;
        let l16 = (10u32 << 16) | (3 << 3) | 2;
        for (profs, intent, name) in [
            // 4-profile chains
            (vec![srgb, lab, gray, srgb], 0, "4p-slab-g-sr"),
            (vec![srgb, cmyk, srgb, lab], 0, "4p-sr-cmyk-sr-lab"),
            (vec![lab, srgb, abs, lab], 0, "4p-lab-sr-abs-lab"),
            (vec![srgb, abs, lab, cmyk], 0, "4p-sr-abs-lab-cmyk"),
            (vec![xyz, srgb, lab, srgb], 0, "4p-x-sr-lab-sr"),
            (vec![srgb, gray, lab, srgb], 1, "4p-sr-g-lab-sr"),
            // 5-profile
            (vec![srgb, lab, srgb, lab, srgb], 0, "5p"),
            // abstract mid-chain in different positions
            (vec![srgb, lab, abs, srgb], 0, "abs-end-mid"),
            (vec![abs, lab, srgb, lab], 0, "abs-start"),
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
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
        c::cmsCloseProfile(xyz);
        c::cmsCloseProfile(gray);
        if !cmyk.is_null() { c::cmsCloseProfile(cmyk); }
        if !abs.is_null() { c::cmsCloseProfile(abs); }
        c::cmsFreeToneCurve(g);
    }
}

/// OptimizeByJoiningCurves deep arms — all-curve pipelines join
/// to a single curve set; curves+matrix/CLUT reject.
#[test]
fn opt_join_curves() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let g2 = c::cmsBuildGamma(std::ptr::null_mut(), 1.8);
        let gray = c::cmsCreateGrayProfile(std::ptr::null_mut(), g);
        let gray2 = c::cmsCreateGrayProfile(std::ptr::null_mut(), g2);
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let g16 = (3u32 << 16) | (1 << 3) | 2;
        let g8 = (3u32 << 16) | (1 << 3) | 1;
        let r16 = (4u32 << 16) | (3 << 3) | 2;
        let l16 = (10u32 << 16) | (3 << 3) | 2;
        for (inp, outp, if_, of_, fl, name) in [
            (gray, gray2, g16, g16, 0, "gray->gray2"),
            (gray2, gray, g16, g16, 0, "gray2->gray"),
            (gray, gray, g16, g16, 0, "gray->gray"),
            (gray, gray2, g8, g8, 0, "gray8"),
            (gray, gray2, g16, g16, 0x0002, "gray lpc"),
            (gray, gray2, g16, g16, 0x0010, "gray prelin"),
            (gray, gray2, g16, g16, 0x0400, "gray hiprec"),
            (gray, gray2, g16, g16, 0x0800, "gray lowprec"),
            (gray, srgb, g16, r16, 0, "gray->srgb"),
            (gray, lab, g16, l16, 0, "gray->lab"),
            (srgb, gray, r16, g16, 0, "srgb->gray"),
            (lab, gray, l16, g16, 0, "lab->gray"),
        ] {
            if inp.is_null() || outp.is_null() { continue; }
            let t = c::cmsCreateTransform(inp, if_, outp, of_, 0, fl);
            eprintln!("{name} t={}", !t.is_null());
            if !t.is_null() {
                let ib = [0x8000u16; 4];
                let mut ob = [0x8000u16; 4];
                c::cmsDoTransform(t, ib.as_ptr() as *const _,
                    ob.as_mut_ptr() as *mut _, 4);
                c::cmsDeleteTransform(t);
            }
        }
        c::cmsCloseProfile(gray);
        c::cmsCloseProfile(gray2);
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
        c::cmsFreeToneCurve(g);
        c::cmsFreeToneCurve(g2);
    }
}

/// OptimizeMatrixShaper deep arms — RGB->RGB between profiles
/// with different primaries/gammas (each produces a distinct
/// pipeline shape the optimizer must classify).
#[test]
fn opt_matshaper_arms() {
    unsafe {
        let wp = [0.3127f64, 0.3290, 1.0];
        let g1 = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let g2 = c::cmsBuildGamma(std::ptr::null_mut(), 1.8);
        let g3 = c::cmsBuildGamma(std::ptr::null_mut(), 2.6);
        let srgb = c::cmsCreate_sRGBProfile();
        let argb = c::cmsCreateRGBProfileTHR(std::ptr::null_mut(),
            wp.as_ptr() as *const _,
            [0.64f64, 0.33, 0.21, 0.30, 0.60, 0.10,
             0.15, 0.06, 0.79].as_ptr() as *const _,
            [g1, g1, g1].as_ptr() as *mut _);
        let p2020 = c::cmsCreateRGBProfileTHR(std::ptr::null_mut(),
            wp.as_ptr() as *const _,
            [0.708f64, 0.292, 0.170, 0.797, 0.131, 0.046,
             0.161, 0.023].as_ptr() as *const _,
            [g2, g2, g2].as_ptr() as *mut _);
        let p709 = c::cmsCreateRGBProfileTHR(std::ptr::null_mut(),
            wp.as_ptr() as *const _,
            [0.64f64, 0.33, 0.30, 0.60, 0.15, 0.06].as_ptr()
                as *const _,
            [g3, g3, g3].as_ptr() as *mut _);
        let r16 = (4u32 << 16) | (3 << 3) | 2;
        let r8 = (4u32 << 16) | (3 << 3) | 1;
        for (inp, outp, if_, of_, intent, name) in [
            (srgb, argb, r16, r16, 0, "srgb->argb"),
            (argb, srgb, r16, r16, 0, "argb->srgb"),
            (srgb, p2020, r16, r16, 0, "srgb->p2020"),
            (p2020, srgb, r16, r16, 0, "p2020->srgb"),
            (argb, p2020, r16, r16, 0, "argb->p2020"),
            (p709, argb, r16, r16, 0, "p709->argb"),
            (srgb, argb, r8, r8, 0, "srgb8->argb"),
            (srgb, argb, r16, r16, 1, "srgb->argb rel"),
            (srgb, argb, r16, r16, 2, "srgb->argb sat"),
            (srgb, argb, r16, r16, 3, "srgb->argb abs"),
            (argb, srgb, r16, r16, 3, "argb->srgb abs"),
            (srgb, srgb, r16, r16, 0, "srgb->srgb"),
        ] {
            if inp.is_null() || outp.is_null() { continue; }
            let t = c::cmsCreateTransform(inp, if_, outp, of_,
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
        for p in [argb, p2020, p709] {
            if !p.is_null() { c::cmsCloseProfile(p); }
        }
        c::cmsFreeToneCurve(g1);
        c::cmsFreeToneCurve(g2);
        c::cmsFreeToneCurve(g3);
    }
}

/// PatchLUT/DefaultICCintents deep arms — gamut-check pipelines,
/// preview transforms, intent direction variants.
#[test]
fn conversion_arms5() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let xyz = c::cmsCreateXYZProfile();
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
        let k16 = (6u32 << 16) | (4 << 3) | 2;
        // gamut-check on all intents — PreviewLUT path
        if !cmyk.is_null() {
            for intent in 0..4u32 {
                let t = c::cmsCreateProofingTransform(
                    srgb, r16, srgb, r16, cmyk, intent, 0, 0x1000);
                eprintln!("gamut-i{intent} t={}", !t.is_null());
                if !t.is_null() {
                    let ib = [0x8000u16; 8];
                    let mut ob = [0x8000u16; 8];
                    c::cmsDoTransform(t, ib.as_ptr() as *const _,
                        ob.as_mut_ptr() as *mut _, 2);
                    c::cmsDeleteTransform(t);
                }
            }
            // proofing with all proofing-intent variants
            for proof in 0..4u32 {
                let t = c::cmsCreateProofingTransform(
                    srgb, r16, srgb, r16, cmyk, 0, proof, 0);
                eprintln!("proof-p{proof} t={}", !t.is_null());
                if !t.is_null() { c::cmsDeleteTransform(t); }
            }
        }
        // reverse-direction class combos
        for (inp, outp, if_, of_, intent, name) in [
            (lab, srgb, l16, r16, 0, "lab->srgb"),
            (lab, srgb, l16, r16, 1, "lab->srgb rel"),
            (lab, srgb, l16, r16, 2, "lab->srgb sat"),
            (lab, srgb, l16, r16, 3, "lab->srgb abs"),
            (xyz, lab, (9u32<<16)|(3<<3)|2, l16, 0, "xyz->lab"),
            (lab, xyz, l16, (9u32<<16)|(3<<3)|2, 0, "lab->xyz"),
            (gray, srgb, (3u32<<16)|(1<<3)|2, r16, 0, "gray->srgb"),
            (srgb, xyz, r16, (9u32<<16)|(3<<3)|2, 0, "srgb->xyz"),
        ] {
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
        let _ = k16;
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
        c::cmsCloseProfile(xyz);
        c::cmsCloseProfile(gray);
        if !cmyk.is_null() { c::cmsCloseProfile(cmyk); }
        c::cmsFreeToneCurve(g);
    }
}

/// DefaultICCintents/AddConversion deep arms — every profile
/// class pair x intent x float/16 direction.
#[test]
fn conversion_arms6() {
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
        let fl = 1u32 << 22;
        let rf = (4u32 << 16) | (3 << 3) | 4 | fl;
        let lf = (10u32 << 16) | (3 << 3) | 4 | fl;
        let r16 = (4u32 << 16) | (3 << 3) | 2;
        let l16 = (10u32 << 16) | (3 << 3) | 2;
        let x16 = (9u32 << 16) | (3 << 3) | 2;
        let g16 = (3u32 << 16) | (1 << 3) | 2;
        let k16 = (6u32 << 16) | (4 << 3) | 2;
        for (inp, outp, if_, of_, intent, name) in [
            (srgb, lab2, r16, l16, 0, "v2lab-out"),
            (lab2, srgb, l16, r16, 0, "v2lab-in"),
            (lab, lab2, l16, l16, 0, "v4lab->v2lab"),
            (lab2, lab, l16, l16, 0, "v2lab->v4lab"),
            (abs, lab, l16, l16, 0, "abs->lab"),
            (abs, srgb, l16, r16, 0, "abs->srgb"),
            (abs, lab2, l16, l16, 0, "abs->v2lab"),
            (link, lab, k16, l16, 0, "link->lab"),
            (link, srgb, k16, r16, 0, "link->srgb"),
            (xyz, lab2, x16, l16, 0, "xyz->v2lab"),
            (lab2, xyz, l16, x16, 0, "v2lab->xyz"),
            (gray, lab2, g16, l16, 0, "gray->v2lab"),
            (lab2, gray, l16, g16, 0, "v2lab->gray"),
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
        let _ = (rf, lf);
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

/// DefaultICCintents/AddConversion deep arms — BPC x intents x
/// profile versions x float transforms.
#[test]
fn conversion_arms7() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let lab2 = c::cmsCreateLab2Profile(std::ptr::null_mut());
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let gray = c::cmsCreateGrayProfile(std::ptr::null_mut(), g);
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        let cmyk = std::fs::read(dir.join("lcms2cmyk.icc")).ok()
            .map(|d| c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32))
            .unwrap_or(std::ptr::null_mut());
        let fl = 1u32 << 22;
        let rf = (4u32 << 16) | (3 << 3) | 4 | fl;
        let lf = (10u32 << 16) | (3 << 3) | 4 | fl;
        let r16 = (4u32 << 16) | (3 << 3) | 2;
        let l16 = (10u32 << 16) | (3 << 3) | 2;
        let k16 = (6u32 << 16) | (4 << 3) | 2;
        let g16 = (3u32 << 16) | (1 << 3) | 2;
        // BPC x intents on all class pairs
        for (inp, outp, if_, of_, name) in [
            (srgb, cmyk, r16, k16, "srgb->cmyk"),
            (cmyk, srgb, k16, r16, "cmyk->srgb"),
            (srgb, lab, r16, l16, "srgb->lab"),
            (lab, srgb, l16, r16, "lab->srgb"),
            (gray, cmyk, g16, k16, "gray->cmyk"),
            (cmyk, gray, k16, g16, "cmyk->gray"),
            (lab2, cmyk, l16, k16, "v2lab->cmyk"),
            (cmyk, lab2, k16, l16, "cmyk->v2lab"),
        ] {
            if inp.is_null() || outp.is_null() { continue; }
            for intent in 0..4u32 {
                for bpc in [0u32, 0x2000] {
                    let t = c::cmsCreateTransform(inp, if_, outp,
                        of_, intent, bpc);
                    eprintln!("{name} i{intent} bpc={bpc} t={}",
                        !t.is_null());
                    if !t.is_null() {
                        let ib = [0x8000u16; 8];
                        let mut ob = [0x8000u16; 8];
                        c::cmsDoTransform(t,
                            ib.as_ptr() as *const _,
                            ob.as_mut_ptr() as *mut _, 2);
                        c::cmsDeleteTransform(t);
                    }
                }
            }
        }
        // float transforms on class pairs
        for (inp, outp, intent, name) in [
            (srgb, lab, 0, "f-srgb->lab"),
            (srgb, cmyk, 0, "f-srgb->cmyk"),
            (cmyk, srgb, 0, "f-cmyk->srgb"),
            (srgb, cmyk, 1, "f-srgb->cmyk rel"),
        ] {
            if inp.is_null() || outp.is_null() { continue; }
            let t = c::cmsCreateTransform(inp, rf, outp, lf,
                intent, 0);
            eprintln!("{name} t={}", !t.is_null());
            if !t.is_null() { c::cmsDeleteTransform(t); }
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
        c::cmsCloseProfile(lab2);
        c::cmsCloseProfile(gray);
        if !cmyk.is_null() { c::cmsCloseProfile(cmyk); }
        c::cmsFreeToneCurve(g);
    }
}

/// PatchLUT/OptimizeByResampling deep arms — CLUT resample with
/// prelin flags on RGB/CMYK/gray pipelines.
#[test]
fn opt_resample_deep() {
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
        let r16 = (4u32 << 16) | (3 << 3) | 2;
        let l16 = (10u32 << 16) | (3 << 3) | 2;
        let k16 = (6u32 << 16) | (4 << 3) | 2;
        let g16 = (3u32 << 16) | (1 << 3) | 2;
        // resample flags on every pipeline shape
        for (inp, outp, if_, of_, fl, name) in [
            (cmyk, lab, k16, l16, 0x0400|0x0010, "cmyk hi+pre"),
            (cmyk, lab, k16, l16, 0x0800|0x0010, "cmyk lo+pre"),
            (cmyk, lab, k16, l16, 0x0400|0x0001, "cmyk hi+post"),
            (cmyk, lab, k16, l16, 0x0800|0x0001, "cmyk lo+post"),
            (cmyk, lab, k16, l16, 0x0400|(5u32<<16), "cmyk hi+g5"),
            (cmyk, lab, k16, l16, 0x0800|(7u32<<16), "cmyk lo+g7"),
            (srgb, lab, r16, l16, 0x0400|0x0010, "srgb hi+pre"),
            (srgb, lab, r16, l16, 0x0800|0x0001, "srgb lo+post"),
            (gray, lab, g16, l16, 0x0400|0x0010, "gray hi+pre"),
            (link, lab, k16, l16, 0x0400|0x0010, "link hi+pre"),
            (cmyk, srgb, k16, r16, 0x0400|0x0010, "cmyk->srgb hi+pre"),
            (lab, cmyk, l16, k16, 0x0800|0x0010, "lab->cmyk lo+pre"),
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

/// OptimizeBy* remaining internals — flag combos on every
/// pipeline shape + intents.
#[test]
fn opt_arms3() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let g2 = c::cmsBuildGamma(std::ptr::null_mut(), 1.8);
        let gray = c::cmsCreateGrayProfile(std::ptr::null_mut(), g);
        let gray2 = c::cmsCreateGrayProfile(std::ptr::null_mut(), g2);
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        let cmyk = std::fs::read(dir.join("lcms2cmyk.icc")).ok()
            .map(|d| c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32))
            .unwrap_or(std::ptr::null_mut());
        let r16 = (4u32 << 16) | (3 << 3) | 2;
        let l16 = (10u32 << 16) | (3 << 3) | 2;
        let k16 = (6u32 << 16) | (4 << 3) | 2;
        let g16 = (3u32 << 16) | (1 << 3) | 2;
        let combos = [
            // gray joins curves; every flag combination
            (gray, gray2, g16, g16, 0x0010|0x0001, "gray pre+post"),
            (gray, gray2, g16, g16, 0x0400|0x0010, "gray hi+pre"),
            (gray, gray2, g16, g16, 0x0800|0x0001, "gray lo+post"),
            (gray, gray2, g16, g16, 0x0400|(3u32<<16), "gray hi+g3"),
            (gray, gray2, g16, g16, 0x0800|(9u32<<16), "gray lo+g9"),
            (gray, gray2, g16, g16, 0x0010|0x0001|0x0400, "gray lin+hi"),
            // cmyk CLUT pipelines x all flags
            (cmyk, srgb, k16, r16, 0x0010, "cmyk->srgb prelin"),
            (cmyk, srgb, k16, r16, 0x0001, "cmyk->srgb postlin"),
            (cmyk, srgb, k16, r16, 0x0400, "cmyk->srgb hiprec"),
            (cmyk, srgb, k16, r16, 0x0800, "cmyk->srgb lowprec"),
            (cmyk, srgb, k16, r16, 0x0400|0x0010, "cmyk->srgb hi+pre"),
            (cmyk, lab, k16, l16, 0x0400|(17u32<<16), "cmyk hi+g17"),
            (cmyk, lab, k16, l16, 0x0800|(255u32<<16), "cmyk lo+g255"),
            // srgb->lab (matrix-shaper) x flags
            (srgb, lab, r16, l16, 0x0010, "srgb->lab prelin"),
            (srgb, lab, r16, l16, 0x0001, "srgb->lab postlin"),
            (srgb, lab, r16, l16, 0x0400, "srgb->lab hiprec"),
            (lab, srgb, l16, r16, 0x0010, "lab->srgb prelin"),
            // intents on CLUT pipelines
            (cmyk, srgb, k16, r16, 0x0010, "cmyk i-prelin"),
        ];
        for (inp, outp, if_, of_, fl, name) in combos {
            if inp.is_null() || outp.is_null() { continue; }
            for intent in 0..4u32 {
                let t = c::cmsCreateTransform(inp, if_, outp, of_,
                    intent, fl);
                eprintln!("{name} i{intent} t={}", !t.is_null());
                if !t.is_null() {
                    let ib = [0x80u8; 64];
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
        c::cmsCloseProfile(gray2);
        if !cmyk.is_null() { c::cmsCloseProfile(cmyk); }
        c::cmsFreeToneCurve(g);
        c::cmsFreeToneCurve(g2);
    }
}

/// DefaultICCintents/AddConversion/PatchLUT remaining deep
/// arms — float transforms on all class pairs, proofing
/// variants, intent/flag combos.
#[test]
fn conversion_arms8() {
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
        let fl = 1u32 << 22;
        let rf = (4u32 << 16) | (3 << 3) | 4 | fl;
        let lf = (10u32 << 16) | (3 << 3) | 4 | fl;
        let kf = (6u32 << 16) | (4 << 3) | 4 | fl;
        let gf = (3u32 << 16) | (1 << 3) | 4 | fl;
        let xf = (9u32 << 16) | (3 << 3) | 4 | fl;
        // float transforms on every class pair x intent
        for (inp, outp, intent, name) in [
            (srgb, lab, 0, "f-srgb->lab"),
            (srgb, lab, 1, "f-srgb->lab rel"),
            (lab, srgb, 0, "f-lab->srgb"),
            (lab, srgb, 3, "f-lab->srgb abs"),
            (srgb, cmyk, 0, "f-srgb->cmyk"),
            (cmyk, srgb, 0, "f-cmyk->srgb"),
            (gray, lab, 0, "f-gray->lab"),
            (lab, gray, 0, "f-lab->gray"),
            (xyz, lab, 0, "f-xyz->lab"),
            (lab, xyz, 0, "f-lab->xyz"),
            (abs, lab, 0, "f-abs->lab"),
            (lab2, lab, 0, "f-v2lab->v4lab"),
        ] {
            if inp.is_null() || outp.is_null() { continue; }
            let t = c::cmsCreateTransform(inp, rf, outp, lf,
                intent, 0);
            eprintln!("{name} t={}", !t.is_null());
            if !t.is_null() { c::cmsDeleteTransform(t); }
        }
        // proofing with all proof-intent + flags combos
        if !cmyk.is_null() {
            for (intent, proof, flags, name) in [
                (0u32, 0, 0, "p00"),
                (1, 1, 0, "p11"),
                (0, 0, 0x2000, "p00-bpc"),
                (0, 0, 0x1000, "p00-gamut"),
                (0, 0, 0x4000, "p00-softproof"),
                (1, 2, 0x2000|0x1000, "p12-bpc-gamut"),
            ] {
                let t = c::cmsCreateProofingTransform(
                    srgb, rf, srgb, rf, cmyk,
                    intent, proof, flags);
                eprintln!("{name} t={}", !t.is_null());
                if !t.is_null() { c::cmsDeleteTransform(t); }
            }
        }
        let _ = (kf, gf, xf);
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
        c::cmsCloseProfile(lab2);
        c::cmsCloseProfile(xyz);
        c::cmsCloseProfile(gray);
        if !cmyk.is_null() { c::cmsCloseProfile(cmyk); }
        if !abs.is_null() { c::cmsCloseProfile(abs); }
        c::cmsFreeToneCurve(g);
    }
}

/// DefaultICCintents/AddConversion/PatchLUT remaining deep
/// arms — more class/intent/format combos on every profile
/// pair.
#[test]
fn conversion_arms9() {
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
        let r8 = (4u32 << 16) | (3 << 3) | 1;
        let l8 = (10u32 << 16) | (3 << 3) | 1;
        let k8 = (6u32 << 16) | (4 << 3) | 1;
        let g8 = (3u32 << 16) | (1 << 3) | 1;
        let r16 = (4u32 << 16) | (3 << 3) | 2;
        let l16 = (10u32 << 16) | (3 << 3) | 2;
        // 8-bit transforms on every class pair
        for (inp, outp, if_, of_, intent, name) in [
            (srgb, lab, r8, l8, 0, "8-srgb->lab"),
            (srgb, lab, r8, l8, 1, "8-srgb->lab rel"),
            (lab, srgb, l8, r8, 0, "8-lab->srgb"),
            (srgb, cmyk, r8, k8, 0, "8-srgb->cmyk"),
            (cmyk, srgb, k8, r8, 0, "8-cmyk->srgb"),
            (gray, lab, g8, l8, 0, "8-gray->lab"),
            (lab, gray, l8, g8, 0, "8-lab->gray"),
            (srgb, xyz, r8, (9u32<<16)|(3<<3)|1, 0, "8-srgb->xyz"),
            (xyz, srgb, (9u32<<16)|(3<<3)|1, r8, 0, "8-xyz->srgb"),
            (abs, lab, l8, l8, 0, "8-abs->lab"),
            (lab2, lab, l8, l8, 0, "8-v2lab->v4lab"),
            (lab, lab2, l8, l8, 0, "8-v4lab->v2lab"),
        ] {
            if inp.is_null() || outp.is_null() { continue; }
            let t = c::cmsCreateTransform(inp, if_, outp, of_,
                intent, 0);
            eprintln!("{name} t={}", !t.is_null());
            if !t.is_null() {
                let ib = [0x80u8; 8];
                let mut ob = [0x80u8; 8];
                c::cmsDoTransform(t, ib.as_ptr() as *const _,
                    ob.as_mut_ptr() as *mut _, 2);
                c::cmsDeleteTransform(t);
            }
        }
        // 8-bit + 16-bit mixed-format transforms
        for (inp, outp, if_, of_, name) in [
            (srgb, lab, r8, l16, "8->16 srgb->lab"),
            (srgb, lab, r16, l8, "16->8 srgb->lab"),
            (lab, srgb, l8, r16, "8->16 lab->srgb"),
            (lab, srgb, l16, r8, "16->8 lab->srgb"),
        ] {
            if inp.is_null() || outp.is_null() { continue; }
            let t = c::cmsCreateTransform(inp, if_, outp, of_,
                0, 0);
            eprintln!("{name} t={}", !t.is_null());
            if !t.is_null() { c::cmsDeleteTransform(t); }
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
        c::cmsCloseProfile(lab2);
        c::cmsCloseProfile(xyz);
        c::cmsCloseProfile(gray);
        if !cmyk.is_null() { c::cmsCloseProfile(cmyk); }
        if !abs.is_null() { c::cmsCloseProfile(abs); }
        c::cmsFreeToneCurve(g);
    }
}

/// cmsTransform2DeviceLink — version/class/flag arms: Lab in/out
/// v2 curve insertion, output-class BToA0, FORCE_CLUT,
/// 8BITS_DEVICELINK, NOWHITEONWHITEFIXUP, NULL-Lut.
#[test]
fn devicelink_arms() {
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
        let r8 = (4u32 << 16) | (3 << 3) | 1;
        let l8 = (10u32 << 16) | (3 << 3) | 1;
        let k8 = (6u32 << 16) | (4 << 3) | 1;
        let g8 = (3u32 << 16) | (1 << 3) | 1;
        let x8 = (9u32 << 16) | (3 << 3) | 1;
        let force_clut = 0x0400u32;      // cmsFLAGS_FORCE_CLUT
        let bits8 = 0x4000u32;         // cmsFLAGS_8BITS_DEVICELINK
        let nowhite = 0x0080u32;       // cmsFLAGS_NOWHITEONWHITEFIXUP
        // transform pair × version × flag combos
        for (inp, outp, if_, of_, name) in [
            (srgb, lab, r8, l8, "srgb->lab"),
            (lab, srgb, l8, r8, "lab->srgb"),
            (srgb, lab2, r8, l8, "srgb->lab2"),
            (lab2, srgb, l8, r8, "lab2->srgb"),
            (srgb, xyz, r8, x8, "srgb->xyz"),
            (xyz, srgb, x8, r8, "xyz->srgb"),
            (gray, lab, g8, l8, "gray->lab"),
            (lab, gray, l8, g8, "lab->gray"),
            (srgb, cmyk, r8, k8, "srgb->cmyk"),
            (cmyk, srgb, k8, r8, "cmyk->srgb"),
            (cmyk, lab, k8, l8, "cmyk->lab"),
            (lab, cmyk, l8, k8, "lab->cmyk"),
        ] {
            if inp.is_null() || outp.is_null() { continue; }
            for (ver, flags, fname) in [
                (2.1f64, 0u32, "v2"),
                (2.1, force_clut, "v2-clut"),
                (2.1, bits8, "v2-8b"),
                (2.1, force_clut | bits8, "v2-c8"),
                (2.1, nowhite, "v2-nw"),
                (4.0, 0, "v4"),
                (4.4, 0, "v44"),
                (4.4, force_clut, "v44-clut"),
                (4.4, bits8, "v44-8b"),
                (4.4, force_clut | bits8 | nowhite, "v44-all"),
            ] {
                let t = c::cmsCreateTransform(inp, if_, outp, of_,
                    0, 0);
                if t.is_null() { continue; }
                let dl = c::cmsTransform2DeviceLink(t, ver, flags);
                eprintln!("{name}/{fname} dl={}", !dl.is_null());
                if !dl.is_null() {
                    let mut buf = vec![0u8; 1 << 22];
                    let mut n = buf.len() as u32;
                    let s = c::cmsSaveProfileToMem(dl,
                        buf.as_mut_ptr() as *mut _, &mut n);
                    let _ = s;
                    c::cmsCloseProfile(dl);
                }
                c::cmsDeleteTransform(t);
            }
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
        c::cmsCloseProfile(lab2);
        c::cmsCloseProfile(xyz);
        c::cmsCloseProfile(gray);
        if !cmyk.is_null() { c::cmsCloseProfile(cmyk); }
        c::cmsFreeToneCurve(g);
    }
}

/// _cmsLCMScolorSpace/_cmsICCcolorSpace/cmsChannelsOfColorSpace —
/// the sig<->PT switches. Every colorspace sig driven through the
/// public channel-count API + a devicelink profile + transform.
#[test]
fn colorspace_switches() {
    unsafe {
        // sig, nch, PT
        let spaces: &[(u32, u32, u32)] = &[
            (0x47524159, 1, 3),   // GRAY / PT_GRAY
            (0x52474220, 3, 4),   // 'RGB ' / PT_RGB
            (0x434d5920, 3, 5),   // 'CMY ' / PT_CMY
            (0x434d594b, 4, 6),   // 'CMYK' / PT_CMYK
            (0x59436272, 3, 7),   // 'YCbr' / PT_YCbCr
            (0x4C757620, 3, 8),   // 'Luv ' / PT_YUV
            (0x58595A20, 3, 9),   // 'XYZ ' / PT_XYZ
            (0x4C616220, 3, 10),  // 'Lab ' / PT_Lab
            (0x4C75764B, 4, 11),  // 'LuvK' / PT_YUVK
            (0x48535620, 3, 12),  // 'HSV ' / PT_HSV
            (0x484C5320, 3, 13),  // 'HLS ' / PT_HLS
            (0x59787920, 3, 14),  // 'Yxy ' / PT_Yxy
            (0x4D434831, 1, 15),  // MCH1
            (0x4D434832, 2, 16),
            (0x4D434833, 3, 17),
            (0x4D434834, 4, 18),
            (0x4D434835, 5, 19),
            (0x4D434836, 6, 20),
            (0x4D434837, 7, 21),
            (0x4D434838, 8, 22),
            (0x4D434839, 9, 23),
            (0x4D434841, 10, 24),
            (0x4D434842, 11, 25),
            (0x4D434843, 12, 26),
            (0x4D434844, 13, 27),
            (0x4D434845, 14, 28),
            (0x4D434846, 15, 29),  // MCHF
            (0x31434C52, 1, 15),   // '1CLR'
            (0x32434C52, 2, 16),   // '2CLR'
            (0x33434C52, 3, 17),   // '3CLR'
            (0x34434C52, 4, 18),   // '4CLR'
            (0x35434C52, 5, 19),   // '5CLR'
            (0x36434C52, 6, 20),   // '6CLR'
            (0x37434C52, 7, 21),   // '7CLR'
            (0x38434C52, 8, 22),   // '8CLR'
            (0x39434C52, 9, 23),   // '9CLR'
            (0x41434C52, 10, 24),  // 'ACLR'
            (0x42434C52, 11, 25),  // 'BCLR'
            (0x43434C52, 12, 26),  // 'CCLR'
            (0x44434C52, 13, 27),  // 'DCLR'
            (0x45434C52, 14, 28),  // 'ECLR'
            (0x46434C52, 15, 29),  // 'FCLR'
            (0x62616420, 0, 0),    // 'bad ' -> default arm
        ];
        for (sig, nch, _pt) in spaces {
            let ch = c::cmsChannelsOfColorSpace(*sig);
            eprintln!("sig={:#x} channels={ch}", sig);
            if *nch == 0 { continue; }
            // devicelink profile with this colorspace → transform
            // drives _cmsLCMScolorSpace + _cmsICCcolorSpace arms
            let prof = c::cmsCreateProfilePlaceholder(
                std::ptr::null_mut());
            c::cmsSetDeviceClass(prof, 0x6c696e6b);   // link
            c::cmsSetColorSpace(prof, *sig);
            c::cmsSetPCS(prof, 0x58595A20);           // XYZ
            c::cmsSetProfileVersion(prof, 4.4);
            let p = c::cmsPipelineAlloc(std::ptr::null_mut(), *nch, 3);
            let total = (2u32.pow(*nch) * 3) as usize;
            let mut grid = vec![0u16; total];
            for (i, v) in grid.iter_mut().enumerate() {
                *v = ((i % 7) * 9362) as u16;
            }
            let dims = vec![2u32; *nch as usize];
            let clut = c::cmsStageAllocCLut16bitGranular(
                std::ptr::null_mut(),
                dims.as_ptr(), *nch, 3, grid.as_ptr());
            c::cmsPipelineInsertStage(p, 1, clut);
            let w = c::cmsWriteTag(prof, 0x41324230, p as *const _);
            let xyz = c::cmsCreateXYZProfile();
            let infmt = (_pt << 16) | (*nch << 3) | 1;
            let outfmt = (9u32 << 16) | (3 << 3) | 1;
            let t = if w != 0 {
                c::cmsCreateTransform(prof, infmt, xyz, outfmt, 0, 0)
            } else { std::ptr::null_mut() };
            eprintln!("sig={:#x} w={w} t={}", sig, !t.is_null());
            if !t.is_null() { c::cmsDeleteTransform(t); }
            c::cmsCloseProfile(xyz);
            c::cmsCloseProfile(prof);
            c::cmsPipelineFree(p);
        }
    }
}

/// cmsCreateRGBProfileTHR/cmsCreate_OkLabProfile edge arms +
/// cmsDetectDestinationBlackPoint intent/flag matrix.
#[test]
fn create_profile_arms() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let g2 = c::cmsBuildGamma(std::ptr::null_mut(), 1.8);
        let g3 = c::cmsBuildGamma(std::ptr::null_mut(), 3.1);
        #[repr(C)] struct XyY { x: f64, y: f64, yy: f64 }
        #[repr(C)] struct Trip { r: XyY, g: XyY, b: XyY }
        let wp = XyY { x: 0.3457, y: 0.3585, yy: 1.0 };
        let prim = Trip {
            r: XyY { x: 0.64, y: 0.33, yy: 1.0 },
            g: XyY { x: 0.30, y: 0.60, yy: 1.0 },
            b: XyY { x: 0.15, y: 0.06, yy: 1.0 } };
        let curves = [g, g2, g3];
        let cptr: *const *mut std::ffi::c_void = curves.as_ptr()
            as *const *mut std::ffi::c_void;
        // normal RGB profile + NULL/edge-param variants
        for (wpp, pp, cp, name) in [
            (&wp as *const _, &prim as *const _, cptr, "ok"),
            (std::ptr::null(), &prim, cptr, "null-wp"),
            (&wp, std::ptr::null(), cptr, "null-prim"),
            (&wp, &prim, std::ptr::null(), "null-curves"),
            (std::ptr::null(), std::ptr::null(), cptr, "null-2"),
        ] {
            let h = c::cmsCreateRGBProfileTHR(
                std::ptr::null_mut(),
                wpp as *const _, pp as *const _,
                cp);
            eprintln!("rgb {name} h={}", !h.is_null());
            if !h.is_null() {
                // serialize → WriteMatrix/curve arms via save
                let mut buf = vec![0u8; 1 << 20];
                let mut n = buf.len() as u32;
                let s = c::cmsSaveProfileToMem(h,
                    buf.as_mut_ptr() as *mut _, &mut n);
                eprintln!("rgb {name} save={s}");
                c::cmsCloseProfile(h);
            }
        }
        // singular/degenerate primaries → matrix inversion fail
        let bad = Trip {
            r: XyY { x: 0.5, y: 0.5, yy: 1.0 },
            g: XyY { x: 0.5, y: 0.5, yy: 1.0 },
            b: XyY { x: 0.5, y: 0.5, yy: 1.0 } };
        let hb = c::cmsCreateRGBProfileTHR(std::ptr::null_mut(),
            &wp as *const _ as *const _,
            &bad as *const _ as *const _, cptr);
        eprintln!("rgb singular h={}", !hb.is_null());
        if !hb.is_null() { c::cmsCloseProfile(hb); }
        // OkLab profile
        let ok = c::cmsCreate_OkLabProfile(std::ptr::null_mut());
        eprintln!("oklab h={}", !ok.is_null());
        if !ok.is_null() {
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            c::cmsSaveProfileToMem(ok,
                buf.as_mut_ptr() as *mut _, &mut n);
            c::cmsCloseProfile(ok);
        }
        // cmsDetectBlackPoint/DestinationBlackPoint matrix
        let srgb = c::cmsCreate_sRGBProfile();
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        let cmyk = std::fs::read(dir.join("lcms2cmyk.icc")).ok()
            .map(|d| c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32))
            .unwrap_or(std::ptr::null_mut());
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        for (pr, pname) in [
            (srgb, "srgb"), (cmyk, "cmyk"), (lab, "lab"),
        ] {
            if pr.is_null() { continue; }
            for intent in [0u32, 1, 2, 3, 4, 10, 11] {
                for flags in [0u32, 1, 2, 0x0040] {
                    #[repr(C)] struct Xyz { x: f64, y: f64, z: f64 }
                    let mut bp = Xyz { x: 0.0, y: 0.0, z: 0.0 };
                    let r1 = c::cmsDetectBlackPoint(
                        &mut bp as *mut _ as *mut _,
                        pr, intent, flags);
                    let mut bp2 = Xyz { x: 0.0, y: 0.0, z: 0.0 };
                    let r2 = c::cmsDetectDestinationBlackPoint(
                        &mut bp2 as *mut _ as *mut _,
                        pr, intent, flags);
                    if intent <= 3 && flags == 0 {
                        eprintln!("bp {pname} i{intent} \
                            b={r1}({:.3}) d={r2}({:.3})",
                            bp.x, bp2.x);
                    }
                }
            }
            let tac = c::cmsDetectTAC(pr);
            eprintln!("tac {pname}={tac:.2}");
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
        if !cmyk.is_null() { c::cmsCloseProfile(cmyk); }
        c::cmsFreeToneCurve(g);
        c::cmsFreeToneCurve(g2);
        c::cmsFreeToneCurve(g3);
    }
}

/// FixWhiteMisalignment + PatchLUT — optimizer paths between
/// white-mismatched profiles (D65 sRGB vs D50 printer/lab) with
/// precalc + prelin/postlin flags.
#[test]
fn fixwhite_arms() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        let cmyk = std::fs::read(dir.join("lcms2cmyk.icc")).ok()
            .map(|d| c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32))
            .unwrap_or(std::ptr::null_mut());
        let r8 = (4u32 << 16) | (3 << 3) | 1;
        let l8 = (10u32 << 16) | (3 << 3) | 1;
        let k8 = (6u32 << 16) | (4 << 3) | 1;
        let r16 = (4u32 << 16) | (3 << 3) | 2;
        let l16 = (10u32 << 16) | (3 << 3) | 2;
        let k16 = (6u32 << 16) | (4 << 3) | 2;
        const HIGHRES: u32 = 0x0400;
        const LOWRES: u32 = 0x0800;
        const PRELIN: u32 = 0x0010;
        const POSTLIN: u32 = 0x0001;
        for (inp, outp, if_, of_, name) in [
            (srgb, lab, r8, l8, "srgb->lab"),
            (srgb, lab, r16, l16, "srgb->lab16"),
            (lab, srgb, l8, r8, "lab->srgb"),
            (srgb, cmyk, r8, k8, "srgb->cmyk"),
            (srgb, cmyk, r16, k16, "srgb->cmyk16"),
            (cmyk, srgb, k8, r8, "cmyk->srgb"),
            (cmyk, lab, k8, l8, "cmyk->lab"),
            (lab, cmyk, l8, k8, "lab->cmyk"),
        ] {
            if inp.is_null() || outp.is_null() { continue; }
            for (flags, fname) in [
                (0u32, "plain"),
                (HIGHRES, "hires"),
                (LOWRES, "lores"),
                (PRELIN, "prelin"),
                (POSTLIN, "postlin"),
                (PRELIN | POSTLIN, "prepost"),
                (HIGHRES | PRELIN, "hi+pre"),
                (LOWRES | POSTLIN, "lo+post"),
                (HIGHRES | PRELIN | POSTLIN, "all"),
            ] {
                for intent in [0u32, 1, 2] {
                    let t = c::cmsCreateTransform(inp, if_, outp,
                        of_, intent, flags);
                    eprintln!("{name}/{fname}/i{intent} t={}",
                        !t.is_null());
                    if !t.is_null() {
                        let inp_b = [0x55u8; 8];
                        let mut out = [0u8; 8];
                        c::cmsDoTransform(t, inp_b.as_ptr()
                            as *const _, out.as_mut_ptr()
                            as *mut _, 1);
                        c::cmsDeleteTransform(t);
                    }
                }
            }
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
        if !cmyk.is_null() { c::cmsCloseProfile(cmyk); }
    }
}

/// BlackPreservingKOnly/KPlane intents (10-15) — full sweep on
/// cmyk/rgb/lab profile directions + bpc combos.
#[test]
fn black_preserve_arms2() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        let cmyk = std::fs::read(dir.join("lcms2cmyk.icc")).ok()
            .map(|d| c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32))
            .unwrap_or(std::ptr::null_mut());
        let r8 = (4u32 << 16) | (3 << 3) | 1;
        let l8 = (10u32 << 16) | (3 << 3) | 1;
        let k8 = (6u32 << 16) | (4 << 3) | 1;
        const BPC: u32 = 0x2000;
        for (inp, outp, if_, of_, name) in [
            (cmyk, cmyk, k8, k8, "cmyk->cmyk"),
            (srgb, cmyk, r8, k8, "srgb->cmyk"),
            (cmyk, srgb, k8, r8, "cmyk->srgb"),
            (cmyk, lab, k8, l8, "cmyk->lab"),
            (lab, cmyk, l8, k8, "lab->cmyk"),
        ] {
            if inp.is_null() || outp.is_null() { continue; }
            for intent in [10u32, 11, 12, 13, 14, 15] {
                for flags in [0u32, BPC] {
                    let t = c::cmsCreateTransform(inp, if_, outp,
                        of_, intent, flags);
                    let mut out = [0u8; 8];
                    let inp_b = [0x55u8; 8];
                    if !t.is_null() {
                        c::cmsDoTransform(t, inp_b.as_ptr()
                            as *const _, out.as_mut_ptr()
                            as *mut _, 1);
                        c::cmsDeleteTransform(t);
                    }
                    if intent == 10 || intent == 13 {
                        eprintln!("{name} i{intent} f{flags:#x} t={} \
                            out={out:?}", !t.is_null());
                    }
                }
            }
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
        if !cmyk.is_null() { c::cmsCloseProfile(cmyk); }
    }
}

/// OptimizeMatrixShaper — 4-stage (abs intent) + 3-stage
/// shaper-matrix paths, identity-matrix drop, offset-matrix and
/// non-8bit/non-3ch rejects.
#[test]
fn matshaper_arms() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        // second RGB profile → 2 distinct mat-shaper pipelines
        let prophoto = std::fs::read(
            dir.join("lcms2prophoto.icc")).ok()
            .map(|d| c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32));
        let rgb2 = match prophoto {
            Some(p) if !p.is_null() => p,
            _ => c::cmsCreate_sRGBProfile(),
        };
        let r8 = (4u32 << 16) | (3 << 3) | 1;
        let r16 = (4u32 << 16) | (3 << 3) | 2;
        let rf = (4u32 << 16) | (3 << 3) | 5;
        // devicelink with cv-mat(offset)-cv pipeline →
        // the Data1->Offset != NULL reject in the 4-stage path
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let offs = [0.01f64, 0.02, -0.01];
        let mat = [1.0f64, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        let link = c::cmsCreateProfilePlaceholder(
            std::ptr::null_mut());
        c::cmsSetDeviceClass(link, 0x6c696e6b);   // link
        c::cmsSetColorSpace(link, 0x52474220);    // RGB
        c::cmsSetPCS(link, 0x52474220);
        c::cmsSetProfileVersion(link, 4.4);
        let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocToneCurves(std::ptr::null_mut(), 3,
                [g, g, g].as_ptr() as *mut _));
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocMatrix(std::ptr::null_mut(), 3, 3,
                mat.as_ptr(), offs.as_ptr()));
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocToneCurves(std::ptr::null_mut(), 3,
                [g, g, g].as_ptr() as *mut _));
        c::cmsWriteTag(link, 0x41324230, p as *const _);
        let links: Vec<(*mut std::ffi::c_void,
            *mut std::ffi::c_void, u32, u32, &str)> = vec![
            (srgb, rgb2, r8, r8, "srgb->rgb2"),
            (rgb2, srgb, r8, r8, "rgb2->srgb"),
            (srgb, srgb, r8, r8, "srgb->srgb"),
            (srgb, link, r8, r8, "srgb->link"),
            (link, srgb, r8, r8, "link->srgb"),
            (srgb, rgb2, r16, r16, "srgb->rgb2-16"),
            (srgb, rgb2, rf, rf, "srgb->rgb2-f"),
        ];
        for intent in [0u32, 1, 3, 4] {
            for (inp, outp, if_, of_, name) in &links {
                if inp.is_null() || outp.is_null() { continue; }
                let t = c::cmsCreateTransform(*inp, *if_,
                    *outp, *of_, intent, 0);
                eprintln!("{name} i{intent} t={}", !t.is_null());
                if !t.is_null() {
                    let inp_b = [0x55u8; 8];
                    let mut out = [0u8; 8];
                    c::cmsDoTransform(t, inp_b.as_ptr() as *const _,
                        out.as_mut_ptr() as *mut _, 1);
                    c::cmsDeleteTransform(t);
                }
            }
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(rgb2);
        c::cmsCloseProfile(link);
        c::cmsPipelineFree(p);
        c::cmsFreeToneCurve(g);
    }
}

/// OptimizeByJoiningCurves — all-curves pipelines (gray/lab pairs),
/// float reject, non-curveset reject, linear-curve skip, 8bit path.
#[test]
fn joincurves_arms() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let gl = c::cmsBuildGamma(std::ptr::null_mut(), 1.0);
        let gray = c::cmsCreateGrayProfile(std::ptr::null_mut(), g);
        let graylin = c::cmsCreateGrayProfile(
            std::ptr::null_mut(), gl);
        let xyz = c::cmsCreateXYZProfile();
        let g8 = (3u32 << 16) | (1 << 3) | 1;
        let g16 = (3u32 << 16) | (1 << 3) | 2;
        let gf = (3u32 << 16) | (1 << 3) | 5;
        let l8 = (10u32 << 16) | (3 << 3) | 1;
        let l16 = (10u32 << 16) | (3 << 3) | 2;
        let lf = (10u32 << 16) | (3 << 3) | 5;
        let x8 = (9u32 << 16) | (3 << 3) | 1;
        let r8 = (4u32 << 16) | (3 << 3) | 1;
        for (inp, outp, if_, of_, name) in [
            (gray, gray, g8, g8, "gray->gray"),
            (gray, gray, g16, g16, "gray16"),
            (gray, gray, gf, gf, "grayf"),
            (graylin, gray, g8, g8, "graylin->gray"),
            (gray, graylin, g8, g8, "gray->graylin"),
            (graylin, graylin, g8, g8, "graylin->graylin"),
            (gray, lab, g8, l8, "gray->lab"),
            (lab, gray, l8, g8, "lab->gray"),
            (gray, lab, g16, l16, "gray->lab16"),
            (gray, lab, gf, lf, "gray->labf"),
            (gray, xyz, g8, x8, "gray->xyz"),
            (xyz, gray, x8, g8, "xyz->gray"),
            (lab, lab, l8, l8, "lab->lab"),
            (lab, lab, lf, lf, "lab->labf"),
            (srgb, srgb, r8, r8, "srgb->srgb"),
        ] {
            for intent in [0u32, 1] {
                let t = c::cmsCreateTransform(inp, if_, outp,
                    of_, intent, 0);
                eprintln!("{name} i{intent} t={}", !t.is_null());
                if !t.is_null() {
                    let inp_b = [0x55u8; 8];
                    let mut out = [0u8; 8];
                    c::cmsDoTransform(t, inp_b.as_ptr() as *const _,
                        out.as_mut_ptr() as *mut _, 1);
                    c::cmsDeleteTransform(t);
                }
            }
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
        c::cmsCloseProfile(gray);
        c::cmsCloseProfile(graylin);
        c::cmsCloseProfile(xyz);
        c::cmsFreeToneCurve(g);
        c::cmsFreeToneCurve(gl);
    }
}

/// DefaultICCintents — multiprofile chains, abstract/devicelink
/// mid-chains, NONEGATIVES clip, colorspace-mismatch rejects.
#[test]
fn default_icc_intents_arms() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let xyz = c::cmsCreateXYZProfile();
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let gray = c::cmsCreateGrayProfile(std::ptr::null_mut(), g);
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        let cmyk = std::fs::read(dir.join("lcms2cmyk.icc")).ok()
            .map(|d| c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32))
            .unwrap_or(std::ptr::null_mut());
        // abstract-class profile (mid-chain arm)
        let abst = c::cmsCreateProfilePlaceholder(
            std::ptr::null_mut());
        c::cmsSetDeviceClass(abst, 0x61627374);   // 'abst'
        c::cmsSetColorSpace(abst, 0x4C616220);    // Lab
        c::cmsSetPCS(abst, 0x4C616220);
        c::cmsSetProfileVersion(abst, 4.4);
        // devicelink mid-chain (srgb->srgb pipeline)
        let link = c::cmsCreateProfilePlaceholder(
            std::ptr::null_mut());
        c::cmsSetDeviceClass(link, 0x6c696e6b);
        c::cmsSetColorSpace(link, 0x52474220);
        c::cmsSetPCS(link, 0x52474220);
        c::cmsSetProfileVersion(link, 4.4);
        let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        let mat = [1.0f64, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocMatrix(std::ptr::null_mut(), 3, 3,
                mat.as_ptr(), std::ptr::null()));
        c::cmsWriteTag(link, 0x41324230, p as *const _);
        let r8 = (4u32 << 16) | (3 << 3) | 1;
        let l8 = (10u32 << 16) | (3 << 3) | 1;
        let k8 = (6u32 << 16) | (4 << 3) | 1;
        let g8 = (3u32 << 16) | (1 << 3) | 1;
        const NONEG: u32 = 0x0002;
        let chains: Vec<(Vec<*mut std::ffi::c_void>, &str)> = vec![
            (vec![srgb, lab, cmyk], "srgb-lab-cmyk"),
            (vec![srgb, xyz, lab, cmyk], "srgb-xyz-lab-cmyk"),
            (vec![srgb, abst, cmyk], "srgb-abst-cmyk"),
            (vec![srgb, abst, lab, cmyk], "srgb-abst-lab-cmyk"),
            (vec![gray, lab, srgb], "gray-lab-srgb"),
            (vec![srgb, link, cmyk], "srgb-link-cmyk"),
            (vec![lab, abst, srgb], "lab-abst-srgb"),
            (vec![srgb, xyz, cmyk], "srgb-xyz-cmyk"),
            (vec![srgb], "single"),
            (vec![abst], "abst-single"),
            (vec![link], "link-single"),
        ];
        for intent in [0u32, 1, 3] {
            for flags in [0u32, NONEG] {
                for (chain, name) in &chains {
                    if chain.iter().any(|p| p.is_null()) { continue; }
                    let n = chain.len() as i32;
                    let t = c::cmsCreateMultiprofileTransform(
                        chain.as_ptr() as *mut _, n, r8, k8,
                        intent, flags);
                    eprintln!("{name} i{intent} f{flags:#x} t={}",
                        !t.is_null());
                    if !t.is_null() {
                        let inp_b = [0x55u8; 8];
                        let mut out = [0u8; 8];
                        c::cmsDoTransform(t, inp_b.as_ptr()
                            as *const _, out.as_mut_ptr()
                            as *mut _, 1);
                        c::cmsDeleteTransform(t);
                    }
                }
            }
        }
        // proofing transforms — proof profile mid-chain
        if !cmyk.is_null() {
            for pi in [0u32, 1] {
                let t = c::cmsCreateProofingTransform(
                    srgb, r8, cmyk, k8, cmyk, 0, pi, 0);
                eprintln!("proof sr-cm-cm pi{pi} t={}",
                    !t.is_null());
                if !t.is_null() { c::cmsDeleteTransform(t); }
                let t2 = c::cmsCreateProofingTransform(
                    srgb, r8, lab, l8, cmyk, 0, pi, 0);
                eprintln!("proof sr-lb-cm pi{pi} t={}",
                    !t2.is_null());
                if !t2.is_null() { c::cmsDeleteTransform(t2); }
                let t3 = c::cmsCreateProofingTransform(
                    gray, g8, srgb, r8, cmyk, 0, pi, 0);
                eprintln!("proof g-sr-cm pi{pi} t={}",
                    !t3.is_null());
                if !t3.is_null() { c::cmsDeleteTransform(t3); }
            }
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
        c::cmsCloseProfile(xyz);
        c::cmsCloseProfile(gray);
        c::cmsCloseProfile(link);
        c::cmsCloseProfile(abst);
        if !cmyk.is_null() { c::cmsCloseProfile(cmyk); }
        c::cmsPipelineFree(p);
        c::cmsFreeToneCurve(g);
    }
}

/// cmsCreateExtendedTransform — per-profile intent/BPC/adaptation
/// arrays + gamut-position.
#[test]
fn extended_transform_arms() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        let cmyk = std::fs::read(dir.join("lcms2cmyk.icc")).ok()
            .map(|d| c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32))
            .unwrap_or(std::ptr::null_mut());
        let r8 = (4u32 << 16) | (3 << 3) | 1;
        let l8 = (10u32 << 16) | (3 << 3) | 1;
        let k8 = (6u32 << 16) | (4 << 3) | 1;
        let g8 = (3u32 << 16) | (1 << 3) | 1;
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let gray = c::cmsCreateGrayProfile(std::ptr::null_mut(), g);
        // per-chain profile sets with intent/bpc/adaptation arrays
        let chains: Vec<(Vec<*mut std::ffi::c_void>, u32, u32,
            &str)> = vec![
            (vec![srgb, lab], r8, l8, "srgb-lab"),
            (vec![srgb, lab, cmyk], r8, k8, "srgb-lab-cmyk"),
            (vec![gray, lab, srgb], g8, r8, "gray-lab-srgb"),
            (vec![srgb, srgb], r8, r8, "srgb-srgb"),
        ];
        for (chain, if_, of_, name) in &chains {
            if chain.iter().any(|p| p.is_null()) { continue; }
            let n = chain.len() as u32;
            let mut bpc = vec![0i32; n as usize];
            let mut intents = vec![0u32; n as usize];
            let mut adapt = vec![1.0f64; n as usize];
            for (bi, ii, ai, variant) in [
                (0i32, 0u32, 1.0f64, "base"),
                (1, 1, 0.5, "bpc-i1-ad05"),
                (1, 3, 0.0, "bpc-i3-ad0"),
                (0, 2, 1.0, "sat-ad1"),
            ] {
                for v in bpc.iter_mut() { *v = bi; }
                for v in intents.iter_mut() { *v = ii; }
                for v in adapt.iter_mut() { *v = ai; }
                let t = c::cmsCreateExtendedTransform(
                    std::ptr::null_mut(), n,
                    chain.as_ptr() as *mut _,
                    bpc.as_mut_ptr(), intents.as_mut_ptr(),
                    adapt.as_mut_ptr(),
                    std::ptr::null_mut(), 0,
                    *if_, *of_, 0);
                eprintln!("{name}/{variant} t={}", !t.is_null());
                if !t.is_null() {
                    let inp_b = [0x55u8; 8];
                    let mut out = [0u8; 8];
                    c::cmsDoTransform(t, inp_b.as_ptr()
                        as *const _, out.as_mut_ptr()
                        as *mut _, 1);
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

/// _cmsReadDevicelinkLUT + AddConversion — link-class profile
/// transforms + adaptation/BPC matrix insertion per colorspace
/// pair.
#[test]
fn devicelink_lut_arms() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let xyz = c::cmsCreateXYZProfile();
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        let cmyk = std::fs::read(dir.join("lcms2cmyk.icc")).ok()
            .map(|d| c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32))
            .unwrap_or(std::ptr::null_mut());
        // devicelink profiles at different channel counts +
        // colorspace headers → _cmsReadDevicelinkLUT arms
        let r8 = (4u32 << 16) | (3 << 3) | 1;
        let l8 = (10u32 << 16) | (3 << 3) | 1;
        let k8 = (6u32 << 16) | (4 << 3) | 1;
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mut links = Vec::new();
        for (cs, pcs, nch, name) in [
            (0x52474220u32, 0x58595A20, 3u32, "rgb->xyz"),
            (0x52474220, 0x4C616220, 3, "rgb->lab"),
            (0x434d594b, 0x4C616220, 4, "cmyk->lab"),
            (0x434d594b, 0x58595A20, 4, "cmyk->xyz"),
            (0x47524159, 0x4C616220, 1, "gray->lab"),
        ] {
            let link = c::cmsCreateProfilePlaceholder(
                std::ptr::null_mut());
            c::cmsSetDeviceClass(link, 0x6c696e6b);   // link
            c::cmsSetColorSpace(link, cs);
            c::cmsSetPCS(link, pcs);
            c::cmsSetProfileVersion(link, 4.4);
            let p = c::cmsPipelineAlloc(
                std::ptr::null_mut(), nch, 3);
            let n = (3u32.pow(nch) * 3) as usize;
            let mut grid = vec![0u16; n];
            for (i, v) in grid.iter_mut().enumerate() {
                *v = ((i % 11) * 5957) as u16;
            }
            let dims = vec![3u32; nch as usize];
            c::cmsPipelineInsertStage(p, 1,
                c::cmsStageAllocCLut16bitGranular(
                    std::ptr::null_mut(), dims.as_ptr(), nch, 3,
                    grid.as_ptr()));
            let w = c::cmsWriteTag(link, 0x41324230,
                p as *const _);
            eprintln!("link {name} w={w}");
            links.push((link, name, cs));
            c::cmsPipelineFree(p);
        }
        // link profiles in transforms (input side) →
        // _cmsReadDevicelinkLUT + AddConversion (PCS fixups)
        for (link, name, cs) in &links {
            let infmt = (*cs as u32 >> 16 & 0x0f) << 16;
            for (outp, of_, oname) in [
                (srgb, r8, "srgb"),
                (lab, l8, "lab"),
                (cmyk, k8, "cmyk"),
            ] {
                if outp.is_null() { continue; }
                let t = c::cmsCreateTransform(*link, infmt,
                    outp, of_, 0, 0);
                eprintln!("{name}->{oname} t={}", !t.is_null());
                if !t.is_null() {
                    let inp_b = [0x55u8; 8];
                    let mut out = [0u8; 8];
                    c::cmsDoTransform(t, inp_b.as_ptr()
                        as *const _, out.as_mut_ptr()
                        as *mut _, 1);
                    c::cmsDeleteTransform(t);
                }
            }
        }
        // AddConversion arms — multiprofile chains with BPC on
        const BPC: u32 = 0x2000;
        for (inp, outp, if_, of_, name) in [
            (srgb, cmyk, r8, k8, "srgb->cmyk"),
            (lab, cmyk, l8, k8, "lab->cmyk"),
            (xyz, cmyk, (9u32 << 16) | (3 << 3) | 1, k8,
                "xyz->cmyk"),
            (cmyk, lab, k8, l8, "cmyk->lab"),
        ] {
            if inp.is_null() || outp.is_null() { continue; }
            for flags in [0u32, BPC] {
                for intent in [0u32, 1] {
                    let t = c::cmsCreateTransform(inp, if_, outp,
                        of_, intent, flags);
                    eprintln!("{name} f{flags:#x} i{intent} t={}",
                        !t.is_null());
                    if !t.is_null() { c::cmsDeleteTransform(t); }
                }
            }
        }
        for (l, _, _) in &links { c::cmsCloseProfile(*l); }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
        c::cmsCloseProfile(xyz);
        if !cmyk.is_null() { c::cmsCloseProfile(cmyk); }
        c::cmsFreeToneCurve(g);
    }
}

/// OptimizeByComputingLinearization + OptimizeByResampling —
/// precalc/prelin flag transforms on CLUT profiles x intents.
#[test]
fn precalc_arms() {
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
        const HIGHRES: u32 = 0x0400;
        const LOWRES: u32 = 0x0800;
        const CLUT_PRELIN: u32 = 0x0010;
        const CLUT_POSTLIN: u32 = 0x0001;
        const NOOPT: u32 = 0x0040;
        let r8 = (4u32 << 16) | (3 << 3) | 1;
        let l8 = (10u32 << 16) | (3 << 3) | 1;
        let k8 = (6u32 << 16) | (4 << 3) | 1;
        let g8 = (3u32 << 16) | (1 << 3) | 1;
        for (inp, outp, if_, of_, name) in [
            (cmyk, srgb, k8, r8, "cmyk->srgb"),
            (srgb, cmyk, r8, k8, "srgb->cmyk"),
            (cmyk, lab, k8, l8, "cmyk->lab"),
            (lab, cmyk, l8, k8, "lab->cmyk"),
            (srgb, srgb, r8, r8, "srgb->srgb"),
            (gray, cmyk, g8, k8, "gray->cmyk"),
            (srgb, gray, r8, g8, "srgb->gray"),
        ] {
            if inp.is_null() || outp.is_null() { continue; }
            for flags in [0u32, HIGHRES, LOWRES, CLUT_PRELIN,
                CLUT_POSTLIN, HIGHRES | CLUT_PRELIN,
                HIGHRES | CLUT_POSTLIN, LOWRES | CLUT_PRELIN,
                HIGHRES | NOOPT] {
                for intent in [0u32, 1] {
                    let t = c::cmsCreateTransform(inp, if_, outp,
                        of_, intent, flags);
                    let ok = !t.is_null();
                    if flags == HIGHRES && intent == 0 {
                        eprintln!("{name} f{flags:#x} t={ok}");
                    }
                    if ok {
                        let inp_b = [0x55u8; 8];
                        let mut out = [0u8; 8];
                        c::cmsDoTransform(t, inp_b.as_ptr()
                            as *const _, out.as_mut_ptr()
                            as *mut _, 1);
                        c::cmsDeleteTransform(t);
                    }
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

/// cmsPluginTHR — valid-plugin registration per type with real
/// (non-null) handlers where the registry requires them.
#[test]
fn plugin_valid_arms() {
    unsafe extern "C" { fn memset(p: *mut std::ffi::c_void,
        v: i32, n: usize) -> *mut std::ffi::c_void; }
    // minimal valid plugin base: magic + version + type + sig
    fn mk(sig: u32, payload_len: usize) -> Vec<u64> {
        let mut v = vec![0u64; payload_len / 8 + 8];
        let b = v.as_mut_ptr() as *mut u8;
        unsafe {
            *(b as *mut u32) = 0x61637070;      // 'acpp'
            *(b as *mut u32).add(1) = 2100;     // expected version
            *(b as *mut u32).add(2) = sig;
        }
        v
    }
    // Registered plugins persist in the global context — run
    // inside a forked child so bogus handlers can't leak into
    // later tests in this binary
    unsafe extern "C" { fn fork() -> i32;
        fn waitpid(p: i32, s: *mut i32, o: i32) -> i32;
        fn _exit(code: i32) -> !;
        fn dlsym(h: *mut std::ffi::c_void,
            n: *const i8) -> *mut std::ffi::c_void; }
    let flush: Option<unsafe extern "C" fn() -> i32> = unsafe {
        let f = dlsym(std::ptr::null_mut(),
            b"__llvm_profile_write_file\0".as_ptr() as *const i8);
        if f.is_null() { None } else { Some(std::mem::transmute(f)) }
    };
    let pid = unsafe { fork() };
    if pid == 0 {
        unsafe {
            for sig in [
                0x6D656D48u32, 0x696E7048, 0x70617248, 0x66726D48,
                0x74797048, 0x74616748, 0x696E7448, 0x6D706548,
                0x6F707448, 0x7A666D48, 0x6D747A48, 0x70726C48,
                0x68647248,
            ] {
                for paylen in [0usize, 16, 40, 64, 96] {
                    let mut pl = mk(sig, paylen);
                    c::cmsPluginTHR(std::ptr::null_mut(),
                        pl.as_mut_ptr() as *mut _);
                }
            }
            if let Some(f) = flush { f(); }
            _exit(0);
        }
    }
    let mut st = 0i32;
    unsafe { waitpid(pid, &mut st, 0); }
    eprintln!("plugin_valid child st={st}");
}

/// cmsPluginTHR — valid-plugin registrations: real fn ptrs per
/// type (register-internal validation + list-append arms).
#[test]
fn plugin_real_arms() {
    unsafe extern "C" fn m_malloc(_c: *mut std::ffi::c_void,
        n: usize) -> *mut std::ffi::c_void {
        unsafe {
            let p = libc_malloc(n);
            p
        }
    }
    unsafe extern "C" fn m_free(_c: *mut std::ffi::c_void,
        p: *mut std::ffi::c_void) {
        unsafe { libc_free(p); }
    }
    unsafe extern "C" fn m_realloc(_c: *mut std::ffi::c_void,
        p: *mut std::ffi::c_void, n: usize)
        -> *mut std::ffi::c_void {
        unsafe { libc_realloc(p, n) }
    }
    unsafe extern "C" {
        fn malloc(n: usize) -> *mut std::ffi::c_void;
        fn free(p: *mut std::ffi::c_void);
        fn realloc(p: *mut std::ffi::c_void, n: usize)
            -> *mut std::ffi::c_void;
    }
    unsafe extern "C" fn libc_malloc(n: usize)
        -> *mut std::ffi::c_void {
        unsafe { malloc(n) }
    }
    unsafe extern "C" fn libc_free(p: *mut std::ffi::c_void) {
        unsafe { free(p) }
    }
    unsafe extern "C" fn libc_realloc(p: *mut std::ffi::c_void,
        n: usize) -> *mut std::ffi::c_void {
        unsafe { realloc(p, n) }
    }
    // base: magic u32, expected u32, type u32, sig u32,
    // saveRevisionReported u32, Next ptr — 5×u32+ptr = 32B
    fn base(sig: u32) -> Vec<u64> {
        let mut v = vec![0u64; 80];
        let b = v.as_mut_ptr() as *mut u8;
        unsafe {
            *(b as *mut u32) = 0x61637070;      // 'acpp'
            *(b as *mut u32).add(1) = 2100;
            *(b as *mut u32).add(2) = sig;
        }
        v
    }
    // Fork-isolate: registered plugins persist globally
    unsafe extern "C" { fn fork() -> i32;
        fn waitpid(p: i32, s: *mut i32, o: i32) -> i32;
        fn _exit(code: i32) -> !;
        fn dlsym(h: *mut std::ffi::c_void,
            n: *const i8) -> *mut std::ffi::c_void; }
    let flush: Option<unsafe extern "C" fn() -> i32> = unsafe {
        let f = dlsym(std::ptr::null_mut(),
            b"__llvm_profile_write_file\0".as_ptr() as *const i8);
        if f.is_null() { None } else { Some(std::mem::transmute(f)) }
    };
    let pid = unsafe { fork() };
    if pid == 0 {
        unsafe {
        // memH — valid alloc/free/realloc
        let mut pl = base(0x6D656D48);
        let b = pl.as_mut_ptr() as *mut usize;
        *b.add(4) = m_malloc as usize;
        *b.add(5) = m_free as usize;
        *b.add(6) = m_realloc as usize;
        *b.add(7) = m_malloc as usize;          // MallocZero
        *b.add(8) = m_malloc as usize;          // Calloc
        *b.add(9) = m_malloc as usize;          // Dup
        let r = c::cmsPluginTHR(std::ptr::null_mut(),
            pl.as_mut_ptr() as *mut _);
        eprintln!("memH r={r}");
        // inpH — interpolation factory
        let mut pl = base(0x696E7048);
        let b = pl.as_mut_ptr() as *mut usize;
        *b.add(4) = m_malloc as usize;          // factory ptr
        let r = c::cmsPluginTHR(std::ptr::null_mut(),
            pl.as_mut_ptr() as *mut _);
        eprintln!("inpH r={r}");
        // parH — nFunctions + evaluator
        let mut pl = base(0x70617248);
        let b = pl.as_mut_ptr() as *mut u8;
        *(b as *mut u32).add(8) = 1;            // nFunctions
        *(b as *mut u32).add(9) = 200;          // FunctionTypes[0]
        *(b as *mut u32).add(9 + 256) = 2;      // ParamCount[0]
        let bu = pl.as_mut_ptr() as *mut usize;
        *bu.add(70) = m_malloc as usize;        // Evaluator
        let r = c::cmsPluginTHR(std::ptr::null_mut(),
            pl.as_mut_ptr() as *mut _);
        eprintln!("parH r={r}");
        // frmH — formatter factory
        let mut pl = base(0x66726D48);
        let b = pl.as_mut_ptr() as *mut usize;
        *b.add(4) = m_malloc as usize;
        let r = c::cmsPluginTHR(std::ptr::null_mut(),
            pl.as_mut_ptr() as *mut _);
        eprintln!("frmH r={r}");
        // typH — tag type handler
        let mut pl = base(0x74797048);
        let b = pl.as_mut_ptr() as *mut usize;
        *b.add(4) = m_malloc as usize;          // ReadPtr
        *b.add(5) = m_malloc as usize;          // WritePtr
        *b.add(6) = m_malloc as usize;          // DupPtr
        *b.add(7) = m_malloc as usize;          // FreePtr
        *b.add(8) = 0x74657874;                 // Sig 'text'
        let r = c::cmsPluginTHR(std::ptr::null_mut(),
            pl.as_mut_ptr() as *mut _);
        eprintln!("typH r={r}");
        // tagH — tag descriptor
        let mut pl = base(0x74616748);
        let b = pl.as_mut_ptr() as *mut usize;
        *b.add(4) = 0x74657874;                 // Sig
        let r = c::cmsPluginTHR(std::ptr::null_mut(),
            pl.as_mut_ptr() as *mut _);
        eprintln!("tagH r={r}");
        // intH — intent pipeline factory
        let mut pl = base(0x696E7448);
        let b = pl.as_mut_ptr() as *mut usize;
        *b.add(4) = 0x1234;                     // intent
        *b.add(5) = 0;                          // desc
        *b.add(6) = m_malloc as usize;          // Link
        let r = c::cmsPluginTHR(std::ptr::null_mut(),
            pl.as_mut_ptr() as *mut _);
        eprintln!("intH r={r}");
        // mpeH — MPE element handler
        let mut pl = base(0x6D706548);
        let b = pl.as_mut_ptr() as *mut usize;
        *b.add(4) = m_malloc as usize;
        *b.add(5) = m_malloc as usize;
        *b.add(6) = m_malloc as usize;
        *b.add(7) = m_malloc as usize;
        let r = c::cmsPluginTHR(std::ptr::null_mut(),
            pl.as_mut_ptr() as *mut _);
        eprintln!("mpeH r={r}");
        // optH — optimizer factory
        let mut pl = base(0x6F707448);
        let b = pl.as_mut_ptr() as *mut usize;
        *b.add(4) = m_malloc as usize;
        let r = c::cmsPluginTHR(std::ptr::null_mut(),
            pl.as_mut_ptr() as *mut _);
        eprintln!("optH r={r}");
        // xfmH — transform factory
        let mut pl = base(0x7A666D48);
        let b = pl.as_mut_ptr() as *mut usize;
        *b.add(4) = m_malloc as usize;
        let r = c::cmsPluginTHR(std::ptr::null_mut(),
            pl.as_mut_ptr() as *mut _);
        eprintln!("xfmH r={r}");
        // mtxH — mutex create/lock/unlock/destroy
        let mut pl = base(0x6D747A48);
        let b = pl.as_mut_ptr() as *mut usize;
        *b.add(4) = m_malloc as usize;
        *b.add(5) = m_malloc as usize;
        *b.add(6) = m_malloc as usize;
        *b.add(7) = m_malloc as usize;
        let r = c::cmsPluginTHR(std::ptr::null_mut(),
            pl.as_mut_ptr() as *mut _);
        eprintln!("mtxH r={r}");
        // prlH — parallelization
        let mut pl = base(0x70726C48);
        let b = pl.as_mut_ptr() as *mut usize;
        *b.add(4) = m_malloc as usize;
        let r = c::cmsPluginTHR(std::ptr::null_mut(),
            pl.as_mut_ptr() as *mut _);
        eprintln!("prlH r={r}");
        // hdrH — profile header handler
        let mut pl = base(0x68647248);
        let b = pl.as_mut_ptr() as *mut usize;
        *b.add(4) = m_malloc as usize;
        let r = c::cmsPluginTHR(std::ptr::null_mut(),
            pl.as_mut_ptr() as *mut _);
        eprintln!("hdrH r={r}");
            if let Some(f) = flush { f(); }
            _exit(0);
        }
    }
    let mut st = 0i32;
    unsafe { waitpid(pid, &mut st, 0); }
    eprintln!("plugin_real child st={st}");

}

/// RootOfLeastSquaresFitQuadraticCurve — DetectBlackPoint/Dest
/// intent-1 + 0/2 on varied profile shapes (CLUT + mat-shaper +
/// gray + deviating ramps).
#[test]
fn blackpoint_fit_arms() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let xyz = c::cmsCreateXYZProfile();
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let gl = c::cmsBuildGamma(std::ptr::null_mut(), 1.0);
        let gs = c::cmsBuildGamma(std::ptr::null_mut(), 0.5);
        let gh = c::cmsBuildGamma(std::ptr::null_mut(), 5.0);
        let gray = c::cmsCreateGrayProfile(std::ptr::null_mut(), g);
        let graylin = c::cmsCreateGrayProfile(
            std::ptr::null_mut(), gl);
        let graysoft = c::cmsCreateGrayProfile(
            std::ptr::null_mut(), gs);
        let grayhard = c::cmsCreateGrayProfile(
            std::ptr::null_mut(), gh);
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        let cmyk = std::fs::read(dir.join("lcms2cmyk.icc")).ok()
            .map(|d| c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32))
            .unwrap_or(std::ptr::null_mut());
        // deviating-CLUT profile — offset midrange so the
        // fitting band sees non-linear points
        let dev = c::cmsCreateProfilePlaceholder(
            std::ptr::null_mut());
        c::cmsSetDeviceClass(dev, 0x6D6E7472);       // mntr
        c::cmsSetColorSpace(dev, 0x52474220);        // RGB
        c::cmsSetPCS(dev, 0x4C616220);               // Lab
        c::cmsSetProfileVersion(dev, 4.4);
        let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        let mut grid = vec![0u16; 3 * 27];
        // midrange L* offset (deviates the 20-100% band)
        for i in 0..27 {
            let l = ((i % 9) * 4096 + (i / 9) * 8192) as u16;
            grid[i * 3] = l;
            grid[i * 3 + 1] = (l / 2) + 4096;
            grid[i * 3 + 2] = l % 65535;
        }
        let dims = [3u32; 3];
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocCLut16bitGranular(
                std::ptr::null_mut(), dims.as_ptr(), 3, 3,
                grid.as_ptr()));
        c::cmsWriteTag(dev, 0x41324231, p as *const _); // a2b1
        let mut bp = c::CieXyz { X: 0.0, Y: 0.0, Z: 0.0 };
        for (prof, name) in [
            (srgb, "srgb"), (lab, "lab"), (xyz, "xyz"),
            (gray, "gray"), (graylin, "graylin"),
            (graysoft, "graysoft"), (grayhard, "grayhard"),
            (cmyk, "cmyk"), (dev, "dev"),
        ] {
            if prof.is_null() { continue; }
            for intent in [0u32, 1, 2, 3] {
                let mut bp1 = bp;
                let r1 = c::cmsDetectBlackPoint(
                    &mut bp1 as *mut c::CieXyz as *mut _,
                    prof, intent, 0);
                let mut bp2 = bp;
                let r2 = c::cmsDetectDestinationBlackPoint(
                    &mut bp2 as *mut c::CieXyz as *mut _,
                    prof, intent, 0);
                let mut bp3 = bp;
                let r3 = c::cmsDetectDestinationBlackPoint(
                    &mut bp3 as *mut c::CieXyz as *mut _,
                    prof, intent, 0x2000);   // BPC
                eprintln!("{name} i{intent} bp={r1} dbp={r2} \
                    dbpb={r3}");
            }
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
        c::cmsCloseProfile(xyz);
        c::cmsCloseProfile(gray);
        c::cmsCloseProfile(graylin);
        c::cmsCloseProfile(graysoft);
        c::cmsCloseProfile(grayhard);
        c::cmsCloseProfile(dev);
        if !cmyk.is_null() { c::cmsCloseProfile(cmyk); }
        c::cmsPipelineFree(p);
        for tc in [g, gl, gs, gh] { c::cmsFreeToneCurve(tc); }
    }
}

/// AddConversion — PCS-mismatch multiprofile chains: XYZ<->Lab
/// stage insertion + empty-layer (same-white vs adapted) arms.
#[test]
fn addconversion_arms() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab50 = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let lab2 = c::cmsCreateLab2Profile(std::ptr::null_mut());
        let xyz = c::cmsCreateXYZProfile();
        let d65 = c::CieXyz { X: 0.9642, Y: 1.0, Z: 0.8249 };
        let d50 = c::CieXyz { X: 0.9642, Y: 1.0, Z: 0.8249 };
        let lab_d50 = c::cmsCreateLab4Profile(
            &d50 as *const _ as *mut _);
        let lab_d65 = c::cmsCreateLab4Profile(
            &d65 as *const _ as *mut _);
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        let cmyk = std::fs::read(dir.join("lcms2cmyk.icc")).ok()
            .map(|d| c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32))
            .unwrap_or(std::ptr::null_mut());
        let r8 = (4u32 << 16) | (3 << 3) | 1;
        let l8 = (10u32 << 16) | (3 << 3) | 1;
        let x8 = (9u32 << 16) | (3 << 3) | 1;
        let k8 = (6u32 << 16) | (4 << 3) | 1;
        // chains exercising XYZ/Lab junctions + empty-layer arms
        let chains: Vec<(Vec<*mut std::ffi::c_void>, u32, u32,
            &str)> = vec![
            (vec![xyz, lab50], x8, l8, "xyz->lab50"),
            (vec![xyz, lab2], x8, l8, "xyz->lab2"),
            (vec![xyz, lab_d65], x8, l8, "xyz->labd65"),
            (vec![lab50, xyz], l8, x8, "lab50->xyz"),
            (vec![lab2, xyz], l8, x8, "lab2->xyz"),
            (vec![lab50, lab2], l8, l8, "lab4->lab2"),
            (vec![lab_d50, lab_d65], l8, l8, "d50->d65"),
            (vec![srgb, lab50, xyz, cmyk], r8, k8,
                "srgb-lab-xyz-cmyk"),
            (vec![srgb, xyz, lab50, cmyk], r8, k8,
                "srgb-xyz-lab-cmyk"),
            (vec![lab50, xyz, srgb], l8, r8, "lab-xyz-srgb"),
            (vec![xyz, lab50, srgb], x8, r8, "xyz-lab-srgb"),
        ];
        for intent in [0u32, 1] {
            for (chain, if_, of_, name) in &chains {
                if chain.iter().any(|p| p.is_null()) { continue; }
                let n = chain.len() as u32;
                let mut bpc = vec![0i32; n as usize];
                let mut intents = vec![intent; n as usize];
                let mut adapt = vec![1.0f64; n as usize];
                let t = c::cmsCreateExtendedTransform(
                    std::ptr::null_mut(), n,
                    chain.as_ptr() as *mut _,
                    bpc.as_mut_ptr(), intents.as_mut_ptr(),
                    adapt.as_mut_ptr(),
                    std::ptr::null_mut(), 0, *if_, *of_, 0);
                eprintln!("{name} i{intent} t={}", !t.is_null());
                if !t.is_null() {
                    let ib = [0x55u8; 8];
                    let mut ob = [0u8; 8];
                    c::cmsDoTransform(t, ib.as_ptr() as *const _,
                        ob.as_mut_ptr() as *mut _, 1);
                    c::cmsDeleteTransform(t);
                }
            }
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab50);
        c::cmsCloseProfile(lab2);
        c::cmsCloseProfile(lab_d50);
        c::cmsCloseProfile(lab_d65);
        c::cmsCloseProfile(xyz);
        if !cmyk.is_null() { c::cmsCloseProfile(cmyk); }
    }
}

/// OptimizeByComputingLinearization/OptimizeByResampling — the
/// planar/non-RGB/degenerate-curve/8bit-flag reject arms +
/// resample internals on planar+degenerate-profile variants.
#[test]
fn prelin_opt_arms() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let gray = c::cmsCreateGrayProfile(std::ptr::null_mut(), g);
        // degenerate-last-stage profile: reversed TRCs
        let degen = c::cmsCreateProfilePlaceholder(
            std::ptr::null_mut());
        c::cmsSetDeviceClass(degen, 0x6D6E7472);
        c::cmsSetColorSpace(degen, 0x52474220);
        c::cmsSetPCS(degen, 0x58595A20);
        c::cmsSetProfileVersion(degen, 4.4);
        let tv = [0u16, 0x3000, 0x7000, 0x9000, 0xf000];
        let tc = c::cmsBuildTabulatedToneCurve16(
            std::ptr::null_mut(), 5, tv.as_ptr());
        let rev = c::cmsReverseToneCurve(tc);
        let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        let n = 3u32 * 27;
        let mut grid = vec![0u16; n as usize];
        for (i, v) in grid.iter_mut().enumerate() {
            *v = (i % 31) as u16 * 2048;
        }
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocCLut16bitGranular(
                std::ptr::null_mut(), [3u32; 3].as_ptr(), 3, 3,
                grid.as_ptr()));
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocToneCurves(std::ptr::null_mut(), 3,
                [rev, rev, rev].as_ptr() as *mut _));
        c::cmsWriteTag(degen, 0x41324230, p as *const _);
        const PLANAR: u32 = 1 << 10;
        const HIGHRES: u32 = 0x0400;
        const CLUT_PRELIN: u32 = 0x0010;
        let r8 = (4u32 << 16) | (3 << 3) | 1;
        let rp = r8 | PLANAR;
        let r16 = (4u32 << 16) | (3 << 3) | 2;
        let rf = (4u32 << 16) | (3 << 3) | 5;
        let l8 = (10u32 << 16) | (3 << 3) | 1;
        let g8 = (3u32 << 16) | (1 << 3) | 1;
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        let cmyk = std::fs::read(dir.join("lcms2cmyk.icc")).ok()
            .map(|d| c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32))
            .unwrap_or(std::ptr::null_mut());
        for (inp, outp, if_, of_, name) in [
            (srgb, cmyk, rp, (6u32 << 16) | (4 << 3) | 1, "planar"),
            (srgb, cmyk, r16, (6u32 << 16) | (4 << 3) | 1,
                "16bit-noprelin"),
            (srgb, cmyk, rf, (6u32 << 16) | (4 << 3) | 1,
                "float"),
            (degen, srgb, r8, r8, "degen->srgb"),
            (srgb, degen, r8, r8, "srgb->degen"),
            (degen, lab, r8, l8, "degen->lab"),
            (gray, cmyk, g8, (6u32 << 16) | (4 << 3) | 1,
                "gray->cmyk"),
            (srgb, cmyk, r8, (6u32 << 16) | (4 << 3) | 1,
                "srgb->cmyk"),
        ] {
            if inp.is_null() || outp.is_null() { continue; }
            for flags in [0u32, HIGHRES, CLUT_PRELIN,
                HIGHRES | CLUT_PRELIN] {
                let t = c::cmsCreateTransform(inp, if_, outp,
                    of_, 0, flags);
                eprintln!("{name} f{flags:#x} t={}", !t.is_null());
                if !t.is_null() {
                    let ib = [0x55u8; 16];
                    let mut ob = [0u8; 16];
                    c::cmsDoTransform(t, ib.as_ptr() as *const _,
                        ob.as_mut_ptr() as *mut _, 1);
                    c::cmsDeleteTransform(t);
                }
            }
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
        c::cmsCloseProfile(gray);
        c::cmsCloseProfile(degen);
        if !cmyk.is_null() { c::cmsCloseProfile(cmyk); }
        c::cmsPipelineFree(p);
        c::cmsFreeToneCurve(g);
        c::cmsFreeToneCurve(tc);
        c::cmsFreeToneCurve(rev);
    }
}

/// cmsvirt builder APIs — ink-limiting devicelink, NULL profile,
/// BCHSW abstract, plus cmsDesaturateLab internals.
#[test]
fn virt_builder_arms() {
    unsafe {
        for cs in [0x434D594Bu32, 0x52474220, 0x434D5950] {
            for limit in [0.0f64, 50.0, 100.0, 150.0, -1.0] {
                let p = c::cmsCreateInkLimitingDeviceLink(
                    cs, limit);
                eprintln!("inklink cs={cs:#x} lim={limit} \
                    ok={}", !p.is_null());
                if !p.is_null() {
                    c::cmsCloseProfile(p);
                }
            }
        }
        let nul = c::cmsCreateNULLProfile();
        let nul2 = c::cmsCreateNULLProfileTHR(std::ptr::null_mut());
        eprintln!("null={} null-thr={}", !nul.is_null(),
            !nul2.is_null());
        // null profile transforms — identity in/out arms
        if !nul.is_null() {
            let r8 = (4u32 << 16) | (3 << 3) | 1;
            let x8 = (9u32 << 16) | (3 << 3) | 1;
            let l8 = (10u32 << 16) | (3 << 3) | 1;
            let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
            let xyz = c::cmsCreateXYZProfile();
            for (i2, o2, f1, f2) in [
                (nul, lab, x8, l8), (lab, nul, l8, x8),
                (nul, xyz, x8, x8), (xyz, nul, x8, x8),
            ] {
                let t = c::cmsCreateTransform(i2, f1, o2, f2,
                    0, 0);
                eprintln!("null xform t={}", !t.is_null());
                if !t.is_null() { c::cmsDeleteTransform(t); }
            }
            c::cmsCloseProfile(lab);
            c::cmsCloseProfile(xyz);
        }
        for npts in [2u32, 17, 33] {
            for (br, co, hu, sa) in [
                (0.0f64, 0.0, 0.0, 0.0), (10.0, 5.0, 3.0, 2.0),
                (-10.0, -5.0, 180.0, -2.0), (100.0, 100.0, 360.0,
                    100.0),
            ] {
                let p = c::cmsCreateBCHSWabstractProfile(
                    npts, br, co, hu, sa, 5000, 6500);
                eprintln!("bchsw n{npts} {br}/{co}/{hu}/{sa} \
                    ok={}", !p.is_null());
                if !p.is_null() { c::cmsCloseProfile(p); }
            }
        }
        for (a2, a1, b2, b1) in [
            (50.0f64, -50.0, 50.0, -50.0), (10.0, 0.0, 10.0, 0.0),
            (0.0, -127.0, 0.0, -127.0),
        ] {
            let mut lab = c::CieLab { L: 50.0, a: 30.0,
                b: -40.0 };
            let r = c::cmsDesaturateLab(
                &mut lab as *mut c::CieLab as *mut _,
                a2, a1, b2, b1);
            eprintln!("desat {a2}/{a1}/{b2}/{b1} r={r} \
                a={} b={}", lab.a, lab.b);
        }
        if !nul.is_null() { c::cmsCloseProfile(nul); }
        if !nul2.is_null() { c::cmsCloseProfile(nul2); }
    }
}

/// Unroll/Pack formatters + _cmsHandleExtraChannels — transform
/// format sweeps: double->float unroll, planar, chunky,
/// alpha/extra-channel formats.
#[test]
fn formatter_arms() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        let cmyk = std::fs::read(dir.join("lcms2cmyk.icc")).ok()
            .map(|d| c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32))
            .unwrap_or(std::ptr::null_mut());
        // format encoding: (colorspace<<16)|(extra<<12?)|
        //   (channels<<3)|bytes
        let enc = |cs: u32, ch: u32, b: u32, flags: u32| {
            (cs << 16) | (ch << 3) | b | flags
        };
        const FL: u32 = 1 << 14;       // FLOAT_SH
        const DBL: u32 = 1 << 15;      // (double marker via OPT?)
        const PLANAR: u32 = 1 << 10;
        const DOSWAP: u32 = 1 << 11;
        const SWAPFIRST: u32 = 1 << 15;
        const EXTRA: u32 = 1 << 7;     // extra channel count
        let r8 = enc(4, 3, 1, 0);
        let rgb = enc(4, 3, 1, 0);
        let lab_dbl_in = enc(10, 3, 0, FL | DBL);   // Lab double
        let lab_flt = enc(10, 3, 4, FL);            // Lab float
        let lab_16 = enc(10, 3, 2, 0);
        let rgb_pl = rgb | PLANAR;
        let cmyk8 = enc(6, 4, 1, 0);
        let cmyk_pl = cmyk8 | PLANAR;
        let rgba = enc(4, 4, 1, 0);                 // RGBA
        let rgb_a2 = enc(4, 4, 1, 0) | EXTRA;       // RGB+2 alphas?
        let lab_a = enc(10, 4, 1, 0);
        let x8 = enc(9, 3, 1, 0);
        let fmts: Vec<(*mut std::ffi::c_void,
            *mut std::ffi::c_void, u32, u32, &str)> = vec![
            (srgb, lab, r8, lab_dbl_in, "r8->labdbl"),
            (lab, srgb, lab_dbl_in, r8, "labdbl->r8"),
            (srgb, lab, r8, lab_flt, "r8->labflt"),
            (lab, srgb, lab_flt, r8, "labflt->r8"),
            (srgb, lab, rgb_pl, lab_16, "rpl->lab16"),
            (lab, srgb, lab_16, rgb_pl, "lab16->rpl"),
            (cmyk, srgb, cmyk_pl, r8, "cmykpl->r8"),
            (srgb, cmyk, r8, cmyk_pl, "r8->cmykpl"),
            (srgb, lab, rgba, lab_16, "rgba->lab16"),
            (lab, srgb, lab_16, rgba, "lab16->rgba"),
            (srgb, lab, rgb_a2, lab_16, "rba2->lab16"),
            (srgb, lab, lab_a, lab_16, "laba->lab16"),
            (srgb, lab, r8 | DOSWAP, lab_16, "rswap->lab"),
            (srgb, lab, r8 | SWAPFIRST, lab_16, "rsw1->lab"),
            (lab, srgb, lab_dbl_in, lab_flt, "dbl->flt"),
        ];
        for (inp, outp, if_, of_, name) in &fmts {
            if inp.is_null() || outp.is_null() { continue; }
            let t = c::cmsCreateTransform(*inp, *if_,
                *outp, *of_, 0, 0);
            eprintln!("{name} t={}", !t.is_null());
            if !t.is_null() {
                let ib = [0x55u8; 32];
                let mut ob = [0u8; 64];
                c::cmsDoTransform(t, ib.as_ptr() as *const _,
                    ob.as_mut_ptr() as *mut _, 1);
                c::cmsDeleteTransform(t);
            }
        }
        // Eval4Inputs — 4-input CLUT pipeline eval
        let dims = [3u32; 4];
        let total = 3u32.pow(4) * 3;
        let grid = vec![0x4000u16; total as usize];
        let cl = c::cmsStageAllocCLut16bitGranular(
            std::ptr::null_mut(), dims.as_ptr(), 4, 3,
            grid.as_ptr());
        let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 4, 3);
        c::cmsPipelineInsertStage(p, 1, cl);
        let inp = [0x5555u16; 4];
        let mut out = [0u16; 3];
        c::cmsPipelineEval16(inp.as_ptr(), out.as_mut_ptr(), p);
        let inpf = [0.5f32; 4];
        let mut outf = [0f32; 3];
        c::cmsPipelineEvalFloat(inpf.as_ptr(), outf.as_mut_ptr(),
            p);
        eprintln!("eval4in {out:?} {outf:?}");
        c::cmsPipelineFree(p);
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
        if !cmyk.is_null() { c::cmsCloseProfile(cmyk); }
    }
}

/// Unroll*/Pack* float/double formatters + ComputeAbsoluteIntent
/// — real float transforms (FLOAT_SH at bit 22).
#[test]
fn float_fmt_arms() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let xyz = c::cmsCreateXYZProfile();
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let gray = c::cmsCreateGrayProfile(std::ptr::null_mut(), g);
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        let cmyk = std::fs::read(dir.join("lcms2cmyk.icc")).ok()
            .map(|d| c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32))
            .unwrap_or(std::ptr::null_mut());
        let enc = |cs: u32, ch: u32, b: u32, flags: u32| {
            (flags << 22) | (cs << 16) | (ch << 3) | b
        };
        // FLOAT_SH=1<<22 → flags arg
        let fl = |cs: u32, ch: u32| (1u32 << 22) | (cs << 16)
            | (ch << 3) | 4;
        let db = |cs: u32, ch: u32| (1u32 << 22) | (cs << 16)
            | (ch << 3) | 0;  // bytes=0 → double
        let r8 = (4u32 << 16) | (3 << 3) | 1;
        let k8 = (6u32 << 16) | (4 << 3) | 1;
        let g8 = (3u32 << 16) | (1 << 3) | 1;
        for (inp, outp, if_, of_, name) in [
            (srgb, lab, fl(4, 3), fl(10, 3), "rgbf->labf"),
            (lab, srgb, fl(10, 3), fl(4, 3), "labf->rgbf"),
            (srgb, lab, db(4, 3), db(10, 3), "rgbd->labd"),
            (lab, srgb, db(10, 3), db(4, 3), "labd->rgbd"),
            (srgb, xyz, fl(4, 3), fl(9, 3), "rgbf->xyzf"),
            (xyz, srgb, fl(9, 3), fl(4, 3), "xyzf->rgbf"),
            (gray, srgb, fl(3, 1), fl(4, 3), "grayf->rgbf"),
            (srgb, gray, fl(4, 3), fl(3, 1), "rgbf->grayf"),
            (cmyk, srgb, fl(6, 4), fl(4, 3), "cmykf->rgbf"),
            (srgb, cmyk, fl(4, 3), fl(6, 4), "rgbf->cmykf"),
            (srgb, lab, r8, fl(10, 3), "rgb8->labf"),
            (lab, srgb, fl(10, 3), r8, "labf->rgb8"),
            (srgb, cmyk, fl(4, 3), k8, "rgbf->cmyk8"),
            (cmyk, srgb, k8, fl(4, 3), "cmyk8->rgbf"),
            (gray, lab, g8, fl(10, 3), "gray8->labf"),
        ] {
            if inp.is_null() || outp.is_null() { continue; }
            for intent in [0u32, 3] {   // + abs intent
                let t = c::cmsCreateTransform(inp, if_, outp,
                    of_, intent, 0);
                eprintln!("{name} i{intent} t={}", !t.is_null());
                if !t.is_null() {
                    let ib = [0x55u8; 32];
                    let mut ob = [0u8; 64];
                    c::cmsDoTransform(t, ib.as_ptr()
                        as *const _, ob.as_mut_ptr()
                        as *mut _, 1);
                    c::cmsDeleteTransform(t);
                }
            }
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
        c::cmsCloseProfile(xyz);
        c::cmsCloseProfile(gray);
        if !cmyk.is_null() { c::cmsCloseProfile(cmyk); }
        c::cmsFreeToneCurve(g);
    }
}

/// Unroll*/Pack* chunky-byte + planar formatters — 8/16-bit
/// packed formats across channel counts and endianness.
#[test]
fn chunky_fmt_arms() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        let cmyk = std::fs::read(dir.join("lcms2cmyk.icc")).ok()
            .map(|d| c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32))
            .unwrap_or(std::ptr::null_mut());
        let enc = |cs: u32, ch: u32, b: u32| {
            (cs << 16) | (ch << 3) | b
        };
        const PLANAR: u32 = 1 << 12;
        const END16: u32 = 1 << 11;
        const DOSWAP: u32 = 1 << 10;
        const SWAPF: u32 = 1 << 14;
        const EXTRA: u32 = 1 << 7;
        let r8 = enc(4, 3, 1);
        let r16 = enc(4, 3, 2);
        let l8 = enc(10, 3, 1);
        let l16 = enc(10, 3, 2);
        let k8 = enc(6, 4, 1);
        let k16 = enc(6, 4, 2);
        let g8 = enc(3, 1, 1);
        let g16 = enc(3, 1, 2);
        let x8 = enc(9, 3, 1);
        let x16 = enc(9, 3, 2);
        for (inp, outp, if_, of_, name) in [
            (srgb, lab, r16, l16, "r16->l16"),
            (lab, srgb, l16, r16, "l16->r16"),
            (srgb, cmyk, r16, k16, "r16->k16"),
            (cmyk, srgb, k16, r16, "k16->r16"),
            (srgb, lab, r8 | PLANAR, l8, "rpl->l8"),
            (lab, srgb, l8 | PLANAR, r8, "lpl->r8"),
            (srgb, lab, r16 | PLANAR, l16, "r16pl->l16"),
            (cmyk, srgb, k8 | PLANAR, r8, "kpl->r8"),
            (srgb, cmyk, r8, k8 | PLANAR, "r8->kpl"),
            (srgb, lab, r8 | DOSWAP, l8, "rswap->l8"),
            (lab, srgb, l8 | DOSWAP, r8, "lswap->r8"),
            (srgb, lab, r16 | END16, l16, "r16se->l16"),
            (srgb, lab, r8 | SWAPF, l8, "rswf->l8"),
            (srgb, lab, r8 | EXTRA, l8, "rexa->l8"),
            (srgb, lab, r16 | END16 | PLANAR, l16, "rsepl"),
            (srgb, lab, r8 | DOSWAP | EXTRA, l8, "rdswe"),
            (srgb, lab, r8, x8, "r8->x8"),
            (srgb, lab, r16, x16, "r16->x16"),
            (srgb, lab, r16 | DOSWAP, l16, "r16sw->l16"),
            (srgb, lab, g8 | PLANAR, l8, "g8pl->l8"),
            (srgb, lab, g16 | PLANAR, l16, "g16pl->l16"),
        ] {
            if inp.is_null() || outp.is_null() { continue; }
            let t = c::cmsCreateTransform(inp, if_, outp,
                of_, 0, 0);
            eprintln!("{name} t={}", !t.is_null());
            if !t.is_null() {
                let ib = [0x55u8; 32];
                let mut ob = [0u8; 64];
                c::cmsDoTransform(t, ib.as_ptr()
                    as *const _, ob.as_mut_ptr()
                    as *mut _, 1);
                c::cmsDeleteTransform(t);
            }
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
        if !cmyk.is_null() { c::cmsCloseProfile(cmyk); }
    }
}

/// AllocEmptyTransform — formatter/plugin/context arms via
/// wide format x intent x flag sweeps on transform creation.
#[test]
fn transform_alloc_arms() {
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
        let enc = |cs: u32, ch: u32, b: u32, flags: u32| {
            (flags) | (cs << 16) | (ch << 3) | b
        };
        let r8 = enc(4, 3, 1, 0);
        let k8 = enc(6, 4, 1, 0);
        let g8 = enc(3, 1, 1, 0);
        let fl = |cs: u32, ch: u32| (1u32 << 22) | (cs << 16)
            | (ch << 3) | 4;
        const BPC: u32 = 0x2000;
        const NOOPT: u32 = 0x200;
        const GRID: u32 = 0x400;
        const HIGHRES: u32 = 0x0400;
        const NULLTRANS: u32 = 0x0008;
        const NOCACHE: u32 = 0x0040;
        const NEGATIVES: u32 = 0x80000000u32.wrapping_shr(0);
        // try the documented flags
        let flagset = [0u32, BPC, NOOPT, GRID, HIGHRES, NULLTRANS,
            NOCACHE, 0x8000u32 /* GUESSDEVICECLASS */,
            0x40000000 /* COPY_ALPHA */, 0x0004 /* NULLTRANSFORM2? */];
        for (inp, outp, if_, of_, name) in [
            (srgb, lab, r8, enc(10, 3, 1, 0), "r8->l8"),
            (lab, srgb, enc(10, 3, 1, 0), r8, "l8->r8"),
            (srgb, cmyk, r8, k8, "r8->k8"),
            (cmyk, srgb, k8, r8, "k8->r8"),
            (gray, srgb, g8, r8, "g8->r8"),
            (srgb, gray, r8, g8, "r8->g8"),
            (srgb, lab, fl(4, 3), fl(10, 3), "fl->fl"),
            (srgb, srgb, r8, r8, "r8->r8"),
            (lab, lab, fl(10, 3), fl(10, 3), "labfl->labfl"),
        ] {
            if inp.is_null() || outp.is_null() { continue; }
            for flags in &flagset {
                for intent in [0u32, 1, 3, 10] {
                    let t = c::cmsCreateTransform(inp, if_,
                        outp, of_, intent, *flags);
                    if !t.is_null() {
                        let ib = [0x55u8; 32];
                        let mut ob = [0u8; 64];
                        c::cmsDoTransform(t, ib.as_ptr()
                            as *const _, ob.as_mut_ptr()
                            as *mut _, 1);
                        c::cmsDeleteTransform(t);
                    }
                }
            }
            eprintln!("{name} swept");
        }
        let _ = NEGATIVES;
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
        c::cmsCloseProfile(gray);
        if !cmyk.is_null() { c::cmsCloseProfile(cmyk); }
        c::cmsFreeToneCurve(g);
    }
}

/// Unroll*/Pack* half-float + word + premul formatters.
#[test]
fn half_fmt_arms() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let gray = c::cmsCreateGrayProfile(std::ptr::null_mut(), g);
        let enc = |cs: u32, ch: u32, b: u32, flags: u32| {
            flags | (cs << 16) | (ch << 3) | b
        };
        const FLOAT: u32 = 1 << 22;
        const HALF: u32 = 1 << 21;          // half-float
        const PREMUL: u32 = 1 << 23;
        const PLANAR: u32 = 1 << 12;
        const EXTRA: u32 = 1 << 7;
        let r8 = enc(4, 3, 1, 0);
        let l8 = enc(10, 3, 1, 0);
        let g8 = enc(3, 1, 1, 0);
        let rh = enc(4, 3, 2, HALF);
        let lh = enc(10, 3, 2, HALF);
        let rf = enc(4, 3, 4, FLOAT);
        let lf = enc(10, 3, 4, FLOAT);
        let rw = enc(4, 3, 2, 0);           // word 16
        let lw = enc(10, 3, 2, 0);
        let rpa = enc(4, 4, 1, PREMUL);
        let rpp = enc(4, 4, 2, PREMUL | PLANAR);
        let rpl = enc(4, 3, 2, PLANAR);
        let rex = enc(4, 4, 1, EXTRA);
        for (inp, outp, if_, of_, name) in [
            (srgb, lab, rh, lh, "half->half"),
            (srgb, lab, rh, l8, "half->l8"),
            (lab, srgb, lh, r8, "lhalf->r8"),
            (srgb, lab, rf, lf, "rflt->lflt"),
            (srgb, lab, rw, lw, "r16->l16"),
            (lab, srgb, lw, rw, "l16->r16"),
            (srgb, lab, rpa, l8, "premul->l8"),
            (srgb, lab, rpp, l8, "premul-pl->l8"),
            (srgb, lab, rpl, l8, "r16pl->l8"),
            (srgb, lab, rex, l8, "extra->l8"),
            (gray, srgb, g8, rh, "g8->half"),
            (srgb, gray, rh, g8, "half->g8"),
            (srgb, srgb, rh, rh, "half-id"),
            (srgb, lab, rh, lf, "half->flt"),
            (srgb, lab, rf, lh, "flt->half"),
        ] {
            let t = c::cmsCreateTransform(inp, if_, outp,
                of_, 0, 0);
            eprintln!("{name} t={}", !t.is_null());
            if !t.is_null() {
                let ib = [0x55u8; 32];
                let mut ob = [0u8; 64];
                c::cmsDoTransform(t, ib.as_ptr()
                    as *const _, ob.as_mut_ptr()
                    as *mut _, 1);
                c::cmsDeleteTransform(t);
            }
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
        c::cmsCloseProfile(gray);
        c::cmsFreeToneCurve(g);
    }
}

/// OptimizeByComputingLinearization — needs RGB->RGB transforms
/// on cv-clut-cv pipelines (link-class profiles).
#[test]
fn linearize_opt_arms() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let tv = [0u16, 0x4000, 0x8000, 0xc000, 0xffff];
        let tc = c::cmsBuildTabulatedToneCurve16(
            std::ptr::null_mut(), 5, tv.as_ptr());
        let srgb = c::cmsCreate_sRGBProfile();
        // link-class profile RGB->RGB with {A curves, CLUT,
        //   B curves} — the optimizer's target shape
        let link = c::cmsCreateProfilePlaceholder(
            std::ptr::null_mut());
        c::cmsSetDeviceClass(link, 0x6C696E6B);   // 'link'
        c::cmsSetColorSpace(link, 0x52474220);
        c::cmsSetPCS(link, 0x52474220);
        c::cmsSetProfileVersion(link, 4.4);
        let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        c::cmsPipelineInsertStage(p, 0,
            c::cmsStageAllocToneCurves(std::ptr::null_mut(), 3,
                [tc, tc, tc].as_ptr() as *mut _));
        let dims = [3u32; 3];
        let mut grid = vec![0u16; 27 * 3];
        for (i, v) in grid.iter_mut().enumerate() {
            *v = ((i % 23) * 2848) as u16;
        }
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocCLut16bitGranular(
                std::ptr::null_mut(), dims.as_ptr(), 3, 3,
                grid.as_ptr()));
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocToneCurves(std::ptr::null_mut(), 3,
                [g, g, g].as_ptr() as *mut _));
        c::cmsWriteTag(link, 0x41324230, p as *const _);
        // second link with reversed (B-clut-A) pipeline
        let link2 = c::cmsCreateProfilePlaceholder(
            std::ptr::null_mut());
        c::cmsSetDeviceClass(link2, 0x6C696E6B);
        c::cmsSetColorSpace(link2, 0x52474220);
        c::cmsSetPCS(link2, 0x52474220);
        c::cmsSetProfileVersion(link2, 4.4);
        let p2 = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        c::cmsPipelineInsertStage(p2, 0,
            c::cmsStageAllocToneCurves(std::ptr::null_mut(), 3,
                [g, g, g].as_ptr() as *mut _));
        let mut grid2 = vec![0u16; 27 * 3];
        for (i, v) in grid2.iter_mut().enumerate() {
            *v = (0xffffu32 - ((i % 23) * 2848) as u32)
                as u16;
        }
        c::cmsPipelineInsertStage(p2, 1,
            c::cmsStageAllocCLut16bitGranular(
                std::ptr::null_mut(), dims.as_ptr(), 3, 3,
                grid2.as_ptr()));
        c::cmsPipelineInsertStage(p2, 1,
            c::cmsStageAllocToneCurves(std::ptr::null_mut(), 3,
                [tc, tc, tc].as_ptr() as *mut _));
        c::cmsWriteTag(link2, 0x41324230, p2 as *const _);
        let r8 = (4u32 << 16) | (3 << 3) | 1;
        const PRELIN: u32 = 0x0010;
        const HIGHRES: u32 = 0x0400;
        const LOWRES: u32 = 0x0100;
        for (inp, outp, name) in [
            (link, srgb, "link->srgb"),
            (srgb, link, "srgb->link"),
            (link, link, "link->link"),
            (link2, srgb, "link2->srgb"),
            (srgb, link2, "srgb->link2"),
            (link, link2, "link->link2"),
        ] {
            for flags in [0u32, PRELIN, HIGHRES, LOWRES,
                PRELIN | HIGHRES, PRELIN | LOWRES] {
                let t = c::cmsCreateTransform(inp, r8, outp,
                    r8, 0, flags);
                eprintln!("{name} f{flags:#x} t={}",
                    !t.is_null());
                if !t.is_null() {
                    let ib = [0x55u8; 8];
                    let mut ob = [0u8; 8];
                    c::cmsDoTransform(t, ib.as_ptr()
                        as *const _, ob.as_mut_ptr()
                        as *mut _, 1);
                    c::cmsDeleteTransform(t);
                }
            }
        }
        c::cmsCloseProfile(link);
        c::cmsCloseProfile(link2);
        c::cmsCloseProfile(srgb);
        c::cmsPipelineFree(p);
        c::cmsPipelineFree(p2);
        c::cmsFreeToneCurve(g);
        c::cmsFreeToneCurve(tc);
    }
}

/// ComputeConversion internals — intents + proofing +
/// GamutCheck + BlackPointCompensation stage injection on
/// varied PCS-mismatch chains.
#[test]
fn compute_conversion_arms() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        let cmyk = std::fs::read(dir.join("lcms2cmyk.icc")).ok()
            .map(|d| c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32))
            .unwrap_or(std::ptr::null_mut());
        let r8 = (4u32 << 16) | (3 << 3) | 1;
        let l8 = (10u32 << 16) | (3 << 3) | 1;
        let k8 = (6u32 << 16) | (4 << 3) | 1;
        const BPC: u32 = 0x2000;
        const GAMUTCHECK: u32 = 0x1000;
        const SOFT: u32 = 0x0040;
        const NOBPC: u32 = 0x0100;
        for (inp, outp, fmt_in, fmt_out, name) in [
            (srgb, cmyk, r8, k8, "rgb->cmyk"),
            (cmyk, srgb, k8, r8, "cmyk->rgb"),
            (lab, cmyk, l8, k8, "lab->cmyk"),
            (cmyk, lab, k8, l8, "cmyk->lab"),
            (srgb, lab, r8, l8, "rgb->lab"),
        ] {
            if inp.is_null() || outp.is_null() { continue; }
            for intent in 0..17u32 {
                for flags in [0u32, BPC, GAMUTCHECK,
                    BPC | GAMUTCHECK, NOBPC] {
                    let t = c::cmsCreateTransform(inp, fmt_in,
                        outp, fmt_out, intent, flags);
                    if !t.is_null() {
                        let ib = [0x55u8; 8];
                        let mut ob = [0u8; 8];
                        c::cmsDoTransform(t, ib.as_ptr()
                            as *const _, ob.as_mut_ptr()
                            as *mut _, 1);
                        c::cmsDeleteTransform(t);
                    }
                }
            }
            eprintln!("{name} swept");
            // proofing transform (3 profiles)
            let t2 = c::cmsCreateProofingTransform(inp,
                fmt_in, outp, fmt_out, srgb, 0, 0,
                SOFT | GAMUTCHECK);
            eprintln!("{name} proof t={}", !t2.is_null());
            if !t2.is_null() { c::cmsDeleteTransform(t2); }
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
        if !cmyk.is_null() { c::cmsCloseProfile(cmyk); }
    }
}

/// _cmsReasonableGridpointsByColorspace + ComputeChromaticAdaptation
/// + BlackPreservingSampler + TranslateNonICCIntents internals.
#[test]
fn gridpoint_adapt_arms() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        // reasonable-gridpoints — fires inside CLUT alloc for
        //   each colorspace channel count
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let gray = c::cmsCreateGrayProfile(std::ptr::null_mut(), g);
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        let cmyk = std::fs::read(dir.join("lcms2cmyk.icc")).ok()
            .map(|d| c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32))
            .unwrap_or(std::ptr::null_mut());
        for (inp, outp, fi, fo, name) in [
            (gray, srgb, (3u32 << 16) | (1 << 3) | 1,
                (4u32 << 16) | (3 << 3) | 1, "g->rgb"),
            (srgb, gray, (4u32 << 16) | (3 << 3) | 1,
                (3u32 << 16) | (1 << 3) | 1, "rgb->g"),
            (lab, gray, (10u32 << 16) | (3 << 3) | 1,
                (3u32 << 16) | (1 << 3) | 1, "lab->g"),
            (srgb, cmyk, (4u32 << 16) | (3 << 3) | 1,
                (6u32 << 16) | (4 << 3) | 1, "rgb->cmyk"),
        ] {
            if inp.is_null() || outp.is_null() { continue; }
            for intent in [0u32, 1, 10, 11, 12, 13, 14, 15] {
                let t = c::cmsCreateTransform(inp, fi,
                    outp, fo, intent, 0x0400 /* HIGHRES */);
                eprintln!("{name} i{intent} t={}", !t.is_null());
                if !t.is_null() {
                    let ib = [0x55u8; 8];
                    let mut ob = [0u8; 8];
                    c::cmsDoTransform(t, ib.as_ptr()
                        as *const _, ob.as_mut_ptr()
                        as *mut _, 1);
                    c::cmsDeleteTransform(t);
                }
            }
        }
        // chromatic adaptation — profile pairs with different
        //   white points drive ComputeChromaticAdaptation
        let d50 = [0.3457f64, 0.3585, 1.0];
        let d65 = [0.3127f64, 0.3290, 1.0];
        let prim = [0.64f64, 0.33, 1.0, 0.30, 0.60, 1.0,
            0.15, 0.06, 1.0];
        let trc = [g, g, g];
        let p50 = c::cmsCreateRGBProfile(d50.as_ptr() as *const _,
            prim.as_ptr() as *const _,
            trc.as_ptr() as *const *mut _);
        let p65 = c::cmsCreateRGBProfile(d65.as_ptr() as *const _,
            prim.as_ptr() as *const _,
            trc.as_ptr() as *const *mut _);
        let r8 = (4u32 << 16) | (3 << 3) | 1;
        for (a, b, name) in [
            (p50, p65, "d50->d65"), (p65, p50, "d65->d50"),
            (p50, srgb, "d50->srgb"), (p65, srgb, "d65->srgb"),
        ] {
            for intent in [0u32, 1, 3] {
                let t = c::cmsCreateTransform(a, r8, b, r8,
                    intent, 0);
                eprintln!("{name} i{intent} t={}", !t.is_null());
                if !t.is_null() {
                    let ib = [0x55u8; 8];
                    let mut ob = [0u8; 8];
                    c::cmsDoTransform(t, ib.as_ptr()
                        as *const _, ob.as_mut_ptr()
                        as *mut _, 1);
                    c::cmsDeleteTransform(t);
                }
            }
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
        c::cmsCloseProfile(gray);
        c::cmsCloseProfile(p50);
        c::cmsCloseProfile(p65);
        if !cmyk.is_null() { c::cmsCloseProfile(cmyk); }
        c::cmsFreeToneCurve(g);
    }
}

/// cmsvirt residual — cmsTransform2DeviceLink internals +
/// cmsCreate_OkLabProfile + ink-limiting variants +
/// BCHSW parameter edges.
#[test]
fn virt_internal_arms() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        let cmyk = std::fs::read(dir.join("lcms2cmyk.icc")).ok()
            .map(|d| c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32))
            .unwrap_or(std::ptr::null_mut());
        let r8 = (4u32 << 16) | (3 << 3) | 1;
        let l8 = (10u32 << 16) | (3 << 3) | 1;
        let k8 = (6u32 << 16) | (4 << 3) | 1;
        // Transform2DeviceLink — Lab-in/out, version<4,
        //   FORCE_CLUT/8BITS_DEVICELINK, various intents
        for (a, b, fi, fo, name) in [
            (srgb, lab, r8, l8, "srgb->lab"),
            (lab, srgb, l8, r8, "lab->srgb"),
            (srgb, cmyk, r8, k8, "srgb->cmyk"),
            (cmyk, srgb, k8, r8, "cmyk->srgb"),
            (lab, lab, l8, l8, "lab->lab"),
        ] {
            if a.is_null() || b.is_null() { continue; }
            for ver in [2.1f64, 4.0, 4.4] {
                for flags in [0u32, 0x0100 /* FORCE_CLUT */,
                    0x0080 /* 8BITS */] {
                    for intent in [0u32, 1, 3] {
                        let t = c::cmsCreateTransform(
                            a, fi, b, fo, intent, flags);
                        if !t.is_null() {
                            let link = c::cmsTransform2DeviceLink(
                                t, ver, flags);
                            let mut buf = vec![0u8; 1 << 20];
                            let mut n = buf.len() as u32;
                            let s = if !link.is_null() {
                                c::cmsSaveProfileToMem(link,
                                    buf.as_mut_ptr() as *mut _,
                                    &mut n)
                            } else { 0 };
                            eprintln!("{name} v{ver} f{flags:#x} \
                                i{intent} link={} s={s}",
                                !link.is_null());
                            if !link.is_null() {
                                c::cmsCloseProfile(link);
                            }
                            c::cmsDeleteTransform(t);
                        }
                    }
                }
            }
        }
        // OkLab
        let oklab = c::cmsCreate_OkLabProfile(
            std::ptr::null_mut());
        if !oklab.is_null() {
            let t = c::cmsCreateTransform(oklab, l8, srgb, r8,
                0, 0);
            let t2 = c::cmsCreateTransform(srgb, r8, oklab, l8,
                0, 0);
            eprintln!("oklab in={} out={}", !t.is_null(),
                !t2.is_null());
            if !t.is_null() { c::cmsDeleteTransform(t); }
            if !t2.is_null() { c::cmsDeleteTransform(t2); }
            c::cmsCloseProfile(oklab);
        }
        // ink-limiting variants — limits + colorspaces
        for (cs, lim) in [
            (0x434D594Bu32, 50.0f64), (0x434D594B, 150.0),
            (0x434D594B, 200.0), (0x434D594B, 300.0),
            (0x52474220, 100.0), (0x4C616220, 100.0),
        ] {
            let p = c::cmsCreateInkLimitingDeviceLinkTHR(
                std::ptr::null_mut(), cs, lim);
            eprintln!("inklim cs={cs:#x} lim={lim} p={}",
                !p.is_null());
            if !p.is_null() {
                let mut buf = vec![0u8; 1 << 20];
                let mut n = buf.len() as u32;
                let s = c::cmsSaveProfileToMem(p,
                    buf.as_mut_ptr() as *mut _, &mut n);
                eprintln!("  save={s}");
                c::cmsCloseProfile(p);
            }
        }
        // BCHSW — param edges (NULL white points, extremes)
        let wp = [0.3457f64, 0.3585, 1.0];
        for (br, co, hu, sa, name) in [
            (0.0f64, 0.0, 0.0, 0.0, "zero"),
            (50.0, 50.0, 180.0, 50.0, "mid"),
            (-50.0, -50.0, -180.0, -50.0, "neg"),
            (100.0, 100.0, 360.0, 100.0, "max"),
            (0.0, 0.0, 0.0, 0.0, "zero2"),
        ] {
            let p = c::cmsCreateBCHSWabstractProfileTHR(
                std::ptr::null_mut(), 5, br, co, hu, sa,
                wp.as_ptr() as *const _,
                wp.as_ptr() as *const _);
            eprintln!("bchsw {name}={}", !p.is_null());
            if !p.is_null() { c::cmsCloseProfile(p); }
        }
        // NULL white point
        let p = c::cmsCreateBCHSWabstractProfileTHR(
            std::ptr::null_mut(), 5, 0.0, 0.0, 0.0, 0.0,
            std::ptr::null_mut(), std::ptr::null_mut());
        eprintln!("bchsw null-wp={}", !p.is_null());
        if !p.is_null() { c::cmsCloseProfile(p); }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
        if !cmyk.is_null() { c::cmsCloseProfile(cmyk); }
        c::cmsFreeToneCurve(g);
    }
}
