//! PostScript CSA/CRD parity — `cmsGetPostScriptCSA`/`cmsGetPostScriptCRD`
//! byte comparison between C and Rust emitters. The `% Created:` line is
//! masked (ctime is wall-clock dependent on both sides).

use lcms::io::{IoStream, io_handler_from_mem};
use lcms::ps2;
use lcms::virt;
use lcms_cref as c;
use std::ffi::CString;
use std::sync::Mutex;

/// C's `_cmsPSActualColumn` is a file-scope static shared by every
/// cmsGetPostScript* call in the process. Tests run in parallel threads, so
/// two tests' C emissions interleave and shift each other's hex-line wraps.
/// Serialize every C PS emission (Rust is per-call and doesn't need it, but
/// locking the whole test keeps the comparison pairs aligned).
static PS_LOCK: Mutex<()> = Mutex::new(());

const BUF: usize = 4 * 1024 * 1024;

fn c_ps(
    f: unsafe extern "C" fn(
        *mut std::ffi::c_void,
        *mut std::ffi::c_void,
        u32,
        u32,
        *mut std::ffi::c_void,
        u32,
    ) -> u32,
    prof: *mut std::ffi::c_void,
    intent: u32,
    flags: u32,
) -> Vec<u8> {
    unsafe {
        let mut buf = vec![0u8; BUF];
        let n = f(
            std::ptr::null_mut(),
            prof,
            intent,
            flags,
            buf.as_mut_ptr() as *mut _,
            BUF as u32,
        );
        assert!(n > 0, "C PS emission failed");
        buf.truncate(n as usize);
        buf
    }
}

fn r_ps(csa: bool, icc: &mut lcms::profile::IccProfile, intent: u32, flags: u32) -> Vec<u8> {
    let mut io = io_handler_from_mem(None, vec![0u8; BUF], "w").expect("io");
    let n = if csa {
        ps2::get_post_script_csa(None, icc, intent, flags, &mut io)
    } else {
        ps2::get_post_script_crd(None, icc, intent, flags, &mut io)
    };
    assert!(n > 0, "rust PS emission failed");
    match &io.stream {
        IoStream::Mem(fm) => fm.block[..n as usize].to_vec(),
        _ => panic!("expected mem io"),
    }
}

/// Mask `% Created: <ctime>` — C's ctime and our emulation can straddle a second.
fn mask_created(b: &[u8]) -> Vec<u8> {
    let s = String::from_utf8_lossy(b);
    s.lines()
        .map(|l| {
            if l.starts_with("% Created: ") {
                "% Created: <masked>"
            } else {
                l
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        .into_bytes()
}

fn cmp(name: &str, c_out: &[u8], r_out: &[u8]) {
    let cm = mask_created(c_out);
    let rm = mask_created(r_out);
    if cm != rm {
        // report the first differing line for debuggability
        let cl = String::from_utf8_lossy(&cm);
        let rl = String::from_utf8_lossy(&rm);
        for (i, (a, b)) in cl.lines().zip(rl.lines()).enumerate() {
            if a != b {
                panic!("{name}: first diff at line {i}:\nC:    {a:?}\nRUST: {b:?}");
            }
        }
        panic!("{name}: length differs ({} vs {})", cm.len(), rm.len());
    }
}

#[test]
fn srgb_csa() {
    let _g = PS_LOCK.lock().unwrap();
    unsafe {
        let prof = c::cmsCreate_sRGBProfile();
        let cout = c_ps(c::cmsGetPostScriptCSA, prof, 0, 0);
        c::cmsCloseProfile(prof);
        let mut p = virt::create_srgb_profile(None).unwrap();
        let rout = r_ps(true, &mut p, 0, 0);
        cmp("srgb CSA", &cout, &rout);
    }
}

#[test]
fn srgb_crd() {
    let _g = PS_LOCK.lock().unwrap();
    unsafe {
        let prof = c::cmsCreate_sRGBProfile();
        for intent in [0u32, 1, 3] {
            let cout = c_ps(c::cmsGetPostScriptCRD, prof, intent, 0);
            let mut p = virt::create_srgb_profile(None).unwrap();
            let rout = r_ps(false, &mut p, intent, 0);
            cmp(&format!("srgb CRD intent {intent}"), &cout, &rout);
        }
        // BPC + absolute arms of EmitPQRStage
        let cout = c_ps(c::cmsGetPostScriptCRD, prof, 1, 0x2000);
        let mut p = virt::create_srgb_profile(None).unwrap();
        let rout = r_ps(false, &mut p, 1, 0x2000);
        cmp("srgb CRD bpc", &cout, &rout);
        c::cmsCloseProfile(prof);
    }
}

#[test]
fn cmyk_csa_crd() {
    let _g = PS_LOCK.lock().unwrap();
    let bytes = std::fs::read("/home/lilith/work/zen/lcms/reference/lcms2/testbed/test1.icc")
        .expect("test1.icc");
    unsafe {
        let prof = c::cmsOpenProfileFromMem(bytes.as_ptr() as *const _, bytes.len() as u32);
        assert!(!prof.is_null());
        let cout = c_ps(c::cmsGetPostScriptCSA, prof, 0, 0);
        let mut p = lcms::profile::open_profile_from_mem(None, &bytes).unwrap();
        let rout = r_ps(true, &mut p, 0, 0);
        cmp("test1 CSA", &cout, &rout);

        let cout = c_ps(c::cmsGetPostScriptCRD, prof, 0, 0);
        let mut p = lcms::profile::open_profile_from_mem(None, &bytes).unwrap();
        let rout = r_ps(false, &mut p, 0, 0);
        cmp("test1 CRD", &cout, &rout);
        c::cmsCloseProfile(prof);
    }
}

// -- named-color profile (same builder as the named-color gate) --

const SIG_NAMED_COLOR_CLASS: u32 = 0x6e6d636c;
const SIG_CMYK: u32 = 0x434d594b;
const SIG_LAB: u32 = 0x4c616220;
const SIG_NCL2: u32 = 0x6e636c32;
const SIG_WTPT: u32 = 0x77747074;

fn c_named_profile_bytes() -> Vec<u8> {
    let names = ["Hazelnut 14-1315", "Kale 18-0107", "PANTONE 100 C"];
    let labs = [(50.0, 10.0, -10.0), (40.0, -5.0, 8.0), (88.0, -3.0, 90.0)];
    let cols: [[u16; 4]; 3] = [
        [10 * 257, 20 * 257, 30 * 257, 40 * 257],
        [10 * 257, 20 * 257, 30 * 257, 40 * 257],
        [0, 0, 60 * 257, 0],
    ];
    unsafe {
        let p = c::cmsCreateNULLProfile();
        c::cmsSetProfileVersion(p, 4.3);
        c::cmsSetDeviceClass(p, SIG_NAMED_COLOR_CLASS);
        c::cmsSetColorSpace(p, SIG_CMYK);
        c::cmsSetPCS(p, SIG_LAB);
        c::cmsSetHeaderRenderingIntent(p, 0);
        assert_eq!(c::cmsWriteTag(p, SIG_WTPT, c::cmsD50_XYZ() as *const _), 1);
        let list = c::cmsAllocNamedColorList(
            std::ptr::null_mut(),
            names.len() as u32,
            4,
            c"TEST".as_ptr() as *const _,
            c"SUF".as_ptr() as *const _,
        );
        for ((name, (l, a, b)), col) in names.iter().zip(labs.iter()).zip(cols.iter()) {
            let name_c = CString::new(*name).unwrap();
            let lab = c::CieLab {
                L: *l,
                a: *a,
                b: *b,
            };
            let mut pcs = [0u16; 3];
            c::cmsFloat2LabEncodedV2(pcs.as_mut_ptr(), &lab);
            let mut dev = [0u16; 16];
            dev[..4].copy_from_slice(col);
            assert_eq!(
                c::cmsAppendNamedColor(
                    list,
                    name_c.as_ptr() as *const _,
                    pcs.as_ptr(),
                    dev.as_ptr()
                ),
                1
            );
        }
        assert_eq!(c::cmsWriteTag(p, SIG_NCL2, list as *const _), 1);
        let mut size = 0u32;
        assert_eq!(
            c::cmsSaveProfileToMem(p, std::ptr::null_mut(), &mut size),
            1
        );
        let mut buf = vec![0u8; size as usize];
        assert_eq!(
            c::cmsSaveProfileToMem(p, buf.as_mut_ptr() as *mut _, &mut size),
            1
        );
        buf.truncate(size as usize);
        c::cmsFreeNamedColorList(list);
        c::cmsCloseProfile(p);
        buf
    }
}

#[test]
fn named_csa_crd() {
    let _g = PS_LOCK.lock().unwrap();
    let bytes = c_named_profile_bytes();
    unsafe {
        let prof = c::cmsOpenProfileFromMem(bytes.as_ptr() as *const _, bytes.len() as u32);
        assert!(!prof.is_null());
        let cout = c_ps(c::cmsGetPostScriptCSA, prof, 0, 0);
        let mut p = lcms::profile::open_profile_from_mem(None, &bytes).unwrap();
        let rout = r_ps(true, &mut p, 0, 0);
        cmp("named CSA", &cout, &rout);

        // Upstream: WriteNamedColorCRD on a CMYK named profile fails —
        // the n=1 link checks channels(PCS=Lab=3) vs pipeline out(4).
        // Both sides must return 0.
        let mut buf = vec![0u8; BUF];
        let c_n = c::cmsGetPostScriptCRD(
            std::ptr::null_mut(),
            prof,
            0,
            0,
            buf.as_mut_ptr() as *mut _,
            BUF as u32,
        );
        assert_eq!(c_n, 0, "C named CRD unexpectedly succeeded");
        let mut p = lcms::profile::open_profile_from_mem(None, &bytes).unwrap();
        let mut io = io_handler_from_mem(None, vec![0u8; BUF], "w").unwrap();
        let r_n = ps2::get_post_script_crd(None, &mut p, 0, 0, &mut io);
        assert_eq!(r_n, 0, "rust named CRD unexpectedly succeeded");
        c::cmsCloseProfile(prof);
    }
}

/// cmsGetPostScriptCSA on all intents — the WritePSA internals
/// vary by intent.
#[test]
fn ps2_all_intents() {
    unsafe {
        let prof = c::cmsCreate_sRGBProfile();
        let mut rp = virt::create_srgb_profile(None).unwrap();
        for intent in 0..4u32 {
            let cout = c_ps(c::cmsGetPostScriptCSA, prof,
                intent, 0);
            let rout = r_ps(true, &mut rp, intent, 0);
            assert_eq!(cout, rout, "intent {intent} CSA");
        }
        c::cmsCloseProfile(prof);
    }
}

/// PostScript machinery — cmsGetPostScriptCSA/CRD on more
/// shapes.
#[test]
fn postscript_shapes() {
    let _g = PS_LOCK.lock().unwrap();
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mut curves = [g; 3];
        let dl = c::cmsCreateLinearizationDeviceLinkTHR(
            std::ptr::null_mut(), 0x52474220,
            curves.as_mut_ptr());
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        for (pname, h) in [
            ("devlink", dl), ("srgb", srgb),
            ("lab4", lab)] {
            let n = c::cmsGetPostScriptCSA(
                std::ptr::null_mut(), h, 0, 0,
                std::ptr::null_mut(), 0);
            let mut buf = vec![0u8; n as usize];
            let n2 = c::cmsGetPostScriptCSA(
                std::ptr::null_mut(), h, 0, 0,
                buf.as_mut_ptr() as *mut _, n);
            eprintln!("{pname} csa={n} n2={n2}");
            let n3 = c::cmsGetPostScriptCRD(
                std::ptr::null_mut(), h, 0, 0,
                std::ptr::null_mut(), 0);
            eprintln!("{pname} crd={n3}");
        }
        let _ = (dl, srgb, lab);
        c::cmsFreeToneCurve(g);
    }
}

/// PostScript color-resource machinery — more shapes +
/// intents.
#[test]
fn postscript_cr() {
    let _g = PS_LOCK.lock().unwrap();
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mut curves = [g; 3];
        let dl = c::cmsCreateLinearizationDeviceLinkTHR(
            std::ptr::null_mut(), 0x52474220,
            curves.as_mut_ptr());
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        // type: 0=CSA, 1=CRD (probably)
        for (pname, h) in [
            ("srgb", srgb), ("lab4", lab)] {
            for ty in [0u32, 1, 2] {
                let io = c::cmsOpenIOhandlerFromNULL(
                    std::ptr::null_mut());
                let n = c::cmsGetPostScriptColorResource(
                    std::ptr::null_mut(), ty, h, 0, 0, io);
                eprintln!("{pname} ty{ty} n={n}");
                if !io.is_null() { c::cmsCloseIOhandler(io); }
            }
        }
        let _ = (dl, srgb, lab);
        c::cmsFreeToneCurve(g);
    }
}

/// PostScript intent/flag machinery — CSA/CRD on intents
/// 0-3 + flags.
#[test]
fn postscript_intents() {
    let _g = PS_LOCK.lock().unwrap();
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        for (pname, h) in [
            ("srgb", srgb), ("lab4", lab)] {
            for intent in [0u32, 1, 2, 3] {
                for flags in [0u32, 1 << 0, 1 << 5] {
                    let io = c::cmsOpenIOhandlerFromNULL(
                        std::ptr::null_mut());
                    let n = c::cmsGetPostScriptColorResource(
                        std::ptr::null_mut(), 0, h, intent,
                        flags, io);
                    if !io.is_null() { c::cmsCloseIOhandler(io); }
                    eprintln!("{pname} i{intent}f{flags} n={n}");
                }
            }
        }
        let _ = (srgb, lab);
    }
}

/// PostScript CRD intent/flag machinery — CRD on intents
/// 0-3 + flags.
#[test]
fn postscript_crd_intents() {
    let _g = PS_LOCK.lock().unwrap();
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        for (pname, h) in [
            ("srgb", srgb), ("lab4", lab)] {
            for intent in [0u32, 1, 2, 3] {
                for flags in [0u32, 1 << 0, 1 << 5] {
                    let io = c::cmsOpenIOhandlerFromNULL(
                        std::ptr::null_mut());
                    let n = c::cmsGetPostScriptColorResource(
                        std::ptr::null_mut(), 1, h, intent,
                        flags, io);
                    if !io.is_null() { c::cmsCloseIOhandler(io); }
                    eprintln!("{pname} i{intent}f{flags} n={n}");
                }
            }
        }
        let _ = (srgb, lab);
    }
}

/// PostScript on more profile shapes — xyz/gray/
/// devicelink-4ch.
#[test]
fn postscript_shapes2() {
    let _g = PS_LOCK.lock().unwrap();
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let xyz = c::cmsCreateXYZProfile();
        let gray = c::cmsCreateGrayProfile(
            std::ptr::null(), g);
        let dl4 = c::cmsCreateLinearizationDeviceLinkTHR(
            std::ptr::null_mut(), 0x434D594B,
            [g; 4].as_mut_ptr());
        for (pname, h) in [
            ("xyz", xyz), ("gray", gray), ("dl4", dl4)] {
            for ty in [0u32, 1] {
                let io = c::cmsOpenIOhandlerFromNULL(
                    std::ptr::null_mut());
                let n = c::cmsGetPostScriptColorResource(
                    std::ptr::null_mut(), ty, h, 0, 0, io);
                if !io.is_null() { c::cmsCloseIOhandler(io); }
                eprintln!("{pname} ty{ty} n={n}");
            }
        }
        let _ = (xyz, gray, dl4);
        c::cmsFreeToneCurve(g);
    }
}

/// CSA/CRD write bodies — real buffers through the
/// writers (not just size queries).
#[test]
fn postscript_write_bodies() {
    let _g = PS_LOCK.lock().unwrap();
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        for (pname, h) in [
            ("srgb", srgb), ("lab4", lab)] {
            let n = c::cmsGetPostScriptCSA(
                std::ptr::null_mut(), h, 0, 0,
                std::ptr::null_mut(), 0);
            let mut buf = vec![0u8; n as usize];
            let n2 = c::cmsGetPostScriptCSA(
                std::ptr::null_mut(), h, 0, 0,
                buf.as_mut_ptr() as *mut _, n);
            let head = String::from_utf8_lossy(&buf[..8.min(n2 as usize)]);
            eprintln!("{pname} csa n={n2} head={head:?}");
            let n3 = c::cmsGetPostScriptCRD(
                std::ptr::null_mut(), h, 0, 0,
                std::ptr::null_mut(), 0);
            let mut buf2 = vec![0u8; n3 as usize];
            let n4 = c::cmsGetPostScriptCRD(
                std::ptr::null_mut(), h, 0, 0,
                buf2.as_mut_ptr() as *mut _, n3);
            eprintln!("{pname} crd n={n4}");
        }
        let _ = (srgb, lab);
    }
}

/// 1-channel CSA — a Gray profile through cmsGetPostScriptCSA hits
/// the nChannels=1 arm: ExtractGray2Y (Gray->XYZ tabulation) +
/// EmitCIEBasedA. Named-color CRD — an 'nmcl' profile through
/// cmsGetPostScriptCRD hits WriteNamedColorCRD.
#[test]
fn gray_csa_named_crd() {
    let _g = PS_LOCK.lock().unwrap();
    unsafe {
        let cg = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mut wp = c::CiexyY { x: 0.3457, y: 0.3585, Y: 1.0 };
        let gp = c::cmsCreateGrayProfile(
            &mut wp as *mut _ as *const _, cg);
        let tg = lcms::gamma::build_gamma(None, 2.2).unwrap();
        let mut rg = lcms::virt::create_gray_profile(None, None, Some(&tg)).unwrap();
        let cout = c_ps(c::cmsGetPostScriptCSA, gp, 0, 0);
        let rout = r_ps(true, &mut rg, 0, 0);
        assert_eq!(cout, rout, "gray CSA");
        // gray CRD also exercises the CIEBasedA path in CRD form
        let cout2 = c_ps(c::cmsGetPostScriptCRD, gp, 0, 0);
        let rout2 = r_ps(false, &mut rg, 0, 0);
        // strip the "% Created:" timestamp line — same-second races
        let strip = |v: &[u8]| -> Vec<u8> {
            let s = String::from_utf8_lossy(v);
            s.lines().filter(|l| !l.starts_with("% Created:"))
                .collect::<Vec<_>>().join("
").into_bytes()
        };
        assert_eq!(strip(&cout2), strip(&rout2), "gray CRD");
        // named-color CRD — 'nmcl' placeholder + ncl2 tag
        let np = c::cmsCreateProfilePlaceholder(std::ptr::null_mut());
        c::cmsSetDeviceClass(np, 0x6e6d636c); // 'nmcl'
        c::cmsSetColorSpace(np, 0x6e636c32); // 'ncl2'
        c::cmsSetPCS(np, 0x4C616220); // 'Lab '
        let ncl = c::cmsAllocNamedColorList(
            std::ptr::null_mut(), 2, 4, b"\0".as_ptr(), b"\0".as_ptr());
        let pcs = [0x8000u16, 0x8000, 0x8000];
        let col = [0xFFFFu16, 0, 0, 0];
        c::cmsAppendNamedColor(ncl, b"Color1\0".as_ptr(),
            pcs.as_ptr(), col.as_ptr());
        c::cmsAppendNamedColor(ncl, b"Color2\0".as_ptr(),
            pcs.as_ptr(), col.as_ptr());
        c::cmsWriteTag(np, 0x6e636c32, ncl as *const _);
        let mut buf = vec![0u8; BUF];
        let cn = c::cmsGetPostScriptCRD(std::ptr::null_mut(), np, 0, 0,
            buf.as_mut_ptr() as *mut _, buf.len() as u32);
        eprintln!("named CRD n={cn} (0 = C rejects WriteNamedColorCRD)");
        // rust side — same nmcl profile
        let mut rp = lcms::profile::create_profile_placeholder(None);
        rp.device_class = lcms::signatures::profileclass::SigNamedColorClass;
        rp.color_space = 0x6e636c32;
        rp.pcs = 0x4C616220;
        let mut rncl = lcms::named::alloc_named_color_list(
            None, 2, 4, b"", b"").unwrap();
        let mut col16 = [0u16; 16];
        col16[..4].copy_from_slice(&col);
        lcms::named::append_named_color(&mut rncl,
            Some(b"Color1"), Some(&pcs), Some(&col16));
        lcms::named::append_named_color(&mut rncl,
            Some(b"Color2"), Some(&pcs), Some(&col16));
        let mut rp2 = *rp;
        lcms::profile::cms_write_tag(&mut rp2, 0x6e636c32,
            Some(lcms::tagtypes::TagObject::NamedColor(rncl)));
        c::cmsCloseProfile(gp);
        c::cmsCloseProfile(np);
        c::cmsFreeNamedColorList(ncl);
    }
}

/// WriteNamedColorCRD — named-color-class profile + ncl2 tag ->
/// the CR named-color write path.
#[test]
fn named_color_crd() {
    unsafe {
        let ncl = c::cmsAllocNamedColorList(std::ptr::null_mut(), 2,
            3, b"pre\0".as_ptr(), b"suf\0".as_ptr());
        let lab = [0xF000u16, 0x8000, 0x8000];
        c::cmsAppendNamedColor(ncl, b"c1\0".as_ptr(),
            lab.as_ptr(), lab.as_ptr());
        c::cmsAppendNamedColor(ncl, b"c2\0".as_ptr(),
            lab.as_ptr(), lab.as_ptr());
        let prof = c::cmsCreateProfilePlaceholder(std::ptr::null_mut());
        c::cmsSetDeviceClass(prof, 0x6e6d636c); // 'nmcl'
        c::cmsSetColorSpace(prof, 0x6e6d636c);   // 'nmcl' colorspace
        c::cmsSetPCS(prof, 0x4C616220);           // 'Lab ' PCS
        c::cmsWriteTag(prof, 0x6e636c32, ncl as *const _); // 'ncl2'
        let wp = c::CieXyz { X: 0.9642, Y: 1.0, Z: 0.8249 };
        c::cmsWriteTag(prof, 0x77747074, &wp as *const _ as *const _); // 'wtpt'
        let mut buf = vec![0u8; 1 << 20];
        let r = c::cmsGetPostScriptCRD(std::ptr::null_mut(), prof,
            0, 0, buf.as_mut_ptr() as *mut _, buf.len() as u32);
        eprintln!("nmcl-crd={r}");
        c::cmsCloseProfile(prof);
        c::cmsFreeNamedColorList(ncl);
    }
}

/// PostScript emitters — CSA + CRD generation on input/output
/// profiles and intents.
#[test]
fn ps2_emitters() {
    let _g = PS_LOCK.lock().unwrap();
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let mut buf = vec![0u8; 1 << 16];
        for (prof, intent, name) in [
            (srgb, 0u32, "srgb-i0"),
            (srgb, 1, "srgb-i1"),
            (srgb, 2, "srgb-i2"),
            (lab, 0, "lab-i0"),
        ] {
            let n = c::cmsGetPostScriptCSA(std::ptr::null_mut(),
                prof, intent, 0, buf.as_mut_ptr() as *mut _,
                buf.len() as u32);
            eprintln!("csa {name} n={n}");
            let n2 = c::cmsGetPostScriptCRD(std::ptr::null_mut(),
                prof, intent, 0, buf.as_mut_ptr() as *mut _,
                buf.len() as u32);
            eprintln!("crd {name} n={n2}");
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
    }
}

/// cmsps2 gray/named-color internals — ExtractGray2Y +
/// BuildColorantList via gray/mono + multi-channel profiles.
#[test]
fn ps2_gray_colorant() {
    let _g = PS_LOCK.lock().unwrap();
    unsafe {
        let g22 = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let gray = c::cmsCreateGrayProfile(std::ptr::null_mut(), g22);
        let srgb = c::cmsCreate_sRGBProfile();
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        for f in ["graylcms2.icc", "gray3lcms2.icc", "lcms2cmyk.icc",
                  "lcms2link.icc", "aRGBlcms2.icc", "limitlcms2.icc"] {
            let Ok(d) = std::fs::read(dir.join(f)) else { continue };
            let p = c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32);
            if p.is_null() { continue; }
            for intent in 0..4u32 {
                let mut buf = vec![0u8; 1 << 18];
                let n = c::cmsGetPostScriptCSA(std::ptr::null_mut(),
                    p, intent, 0, buf.as_mut_ptr() as *mut _,
                    buf.len() as u32);
                let n2 = c::cmsGetPostScriptCRD(std::ptr::null_mut(),
                    p, intent, 0, buf.as_mut_ptr() as *mut _,
                    buf.len() as u32);
                if intent == 0 && n + n2 > 0 {
                    eprintln!("{f} csa={n} crd={n2}");
                }
            }
            c::cmsCloseProfile(p);
        }
        for p in [gray, srgb] {
            for intent in 0..4u32 {
                let mut buf = vec![0u8; 1 << 18];
                let n = c::cmsGetPostScriptCSA(std::ptr::null_mut(),
                    p, intent, 0, buf.as_mut_ptr() as *mut _,
                    buf.len() as u32);
                let n2 = c::cmsGetPostScriptCRD(std::ptr::null_mut(),
                    p, intent, 0, buf.as_mut_ptr() as *mut _,
                    buf.len() as u32);
                if intent == 0 { eprintln!("virt csa={n} crd={n2}"); }
            }
        }
        c::cmsCloseProfile(gray);
        c::cmsCloseProfile(srgb);
        c::cmsFreeToneCurve(g22);
    }
}

/// ExtractGray2Y + BuildColorantList + EmitNamedColorCS — gray
/// and named-color-class profiles through the PostScript emitters.
#[test]
fn ps2_gray_named_emitters() {
    let _g = PS_LOCK.lock().unwrap();
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let gray = c::cmsCreateGrayProfile(std::ptr::null_mut(), g);
        // named-color profile: nmcl class + ncl2 tag
        let nmcl = c::cmsCreateProfilePlaceholder(std::ptr::null_mut());
        c::cmsSetDeviceClass(nmcl, 0x6e6d636c);
        c::cmsSetColorSpace(nmcl, 0x34e434e); // NCLR-ish
        c::cmsSetPCS(nmcl, 0x4c616220);
        let mut buf = vec![0u8; 1 << 18];
        let mut n = buf.len() as u32;
        for (prof, name) in [(gray, "gray"), (nmcl, "nmcl")] {
            let mut n2 = buf.len() as u32;
            let csa = c::cmsGetPostScriptCSA(std::ptr::null_mut(),
                prof, 0, 0, buf.as_mut_ptr() as *mut _, n2);
            eprintln!("{name}-csa={csa}");
            let mut n3 = buf.len() as u32;
            let crd = c::cmsGetPostScriptCRD(std::ptr::null_mut(),
                prof, 0, 0, buf.as_mut_ptr() as *mut _, n3);
            eprintln!("{name}-crd={crd}");
        }
        let _ = n;
        c::cmsCloseProfile(gray);
        c::cmsCloseProfile(nmcl);
        c::cmsFreeToneCurve(g);
    }
}

/// WriteNamedColorCRD + BuildColorantList — nmcl profile with a
/// real ncl2 tag + A2B0 through the CRD emitter.
#[test]
fn ps2_named_crd_internals() {
    let _g = PS_LOCK.lock().unwrap();
    unsafe {
        let nmcl = c::cmsCreateProfilePlaceholder(std::ptr::null_mut());
        c::cmsSetDeviceClass(nmcl, 0x6e6d636c);   // 'nmcl'
        c::cmsSetColorSpace(nmcl, 0x33434c52);    // '3CLR' device
        c::cmsSetPCS(nmcl, 0x4c616220);           // 'Lab '
        // named-color list -> ncl2 tag
        let list = c::cmsAllocNamedColorList(
            std::ptr::null_mut(), 4, 3,
            b"pre\0".as_ptr(), b"post\0".as_ptr());
        for i in 0..4u32 {
            let mut name = [0u8; 256];
            let nm = format!("color{}\0", i);
            name[..nm.len()].copy_from_slice(nm.as_bytes());
            let pcs = [100u16 * (i as u16 + 1), 200, 300];
            let dev = [0u16; 15];
            c::cmsAppendNamedColor(list,
                name.as_ptr() as *mut _,
                pcs.as_ptr(), dev.as_ptr());
        }
        let w = c::cmsWriteTag(nmcl, 0x6e636c32, list as *const _);
        eprintln!("ncl2 w={w}");
        // desc + wtpt for completeness
        let ml = c::cmsMLUalloc(std::ptr::null_mut(), 1);
        let mut wk = [0x74u32, 0x65, 0x73, 0x74, 0];
        c::cmsMLUsetWide(ml, b"en\0".as_ptr() as *const _,
            b"US\0".as_ptr() as *const _, wk.as_mut_ptr() as *const _);
        c::cmsWriteTag(nmcl, 0x64657363, ml as *const _);
        let wp = [0.9642f64, 1.0, 0.8249];
        c::cmsWriteTag(nmcl, 0x77747074, wp.as_ptr() as *const _);
        let mut buf = vec![0u8; 1 << 18];
        for (intent, name) in [(0u32, "i0"), (1, "i1"), (3, "i3")] {
            let mut n = buf.len() as u32;
            let crd = c::cmsGetPostScriptCRD(std::ptr::null_mut(),
                nmcl, intent, 0, buf.as_mut_ptr() as *mut _, n);
            eprintln!("{name} crd={crd}");
            let mut n2 = buf.len() as u32;
            let csa = c::cmsGetPostScriptCSA(std::ptr::null_mut(),
                nmcl, intent, 0, buf.as_mut_ptr() as *mut _, n2);
            eprintln!("{name} csa={csa}");
        }
        c::cmsCloseProfile(nmcl);
        c::cmsFreeNamedColorList(list);
        c::cmsMLUfree(ml);
    }
}

/// ExtractGray2Y — gray profile with an A2B0 CLUT routes
/// through WriteInputLUT case-1 (not the matrix-shaper path).
#[test]
fn ps2_gray_lut_emitters() {
    let _g = PS_LOCK.lock().unwrap();
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let gray = c::cmsCreateGrayProfile(std::ptr::null_mut(), g);
        // write an A2B0 CLUT so the LUT path fires
        let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 1, 3);
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocToneCurves(std::ptr::null_mut(), 1,
                [g].as_ptr() as *mut _));
        let mut grid = vec![0u16; 17 * 3];
        for (i, v) in grid.iter_mut().enumerate() {
            *v = ((i % 17) * 3855) as u16;
        }
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocCLut16bit(std::ptr::null_mut(), 17, 1, 3,
                grid.as_ptr()));
        let w = c::cmsWriteTag(gray, 0x41324230, p as *const _);
        eprintln!("a2b0 w={w}");
        let mut buf = vec![0u8; 1 << 18];
        for (intent, name) in [(0u32, "i0"), (1, "i1"), (3, "i3")] {
            let mut n = buf.len() as u32;
            let csa = c::cmsGetPostScriptCSA(std::ptr::null_mut(),
                gray, intent, 0, buf.as_mut_ptr() as *mut _, n);
            eprintln!("{name} csa={csa}");
            let mut n2 = buf.len() as u32;
            let crd = c::cmsGetPostScriptCRD(std::ptr::null_mut(),
                gray, intent, 0, buf.as_mut_ptr() as *mut _, n2);
            eprintln!("{name} crd={crd}");
        }
        c::cmsCloseProfile(gray);
        c::cmsPipelineFree(p);
        c::cmsFreeToneCurve(g);
    }
}

/// cmsps2 residual — named-color CSA/CRD emit +
///   intent/shape/flag variants through
///   cmsGetPostScriptCSA/CRD/ColorResource.
#[test]
fn ps2_named_intent_arms() {
    let _g = PS_LOCK.lock().unwrap();
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        // named-color profile (class 'nmcl')
        let ncp = c::cmsCreateProfilePlaceholder(
            std::ptr::null_mut());
        c::cmsSetDeviceClass(ncp, 0x6E6D636C);
        c::cmsSetColorSpace(ncp, 0x46434C52);
        c::cmsSetPCS(ncp, 0x58595A20);
        let nl = c::cmsAllocNamedColorList(
            std::ptr::null_mut(), 2, 3,
            b"P\0".as_ptr() as *const _,
            b"S\0".as_ptr() as *const _);
        let pcs = [0x1000u16, 0x2000, 0x3000];
        let dev = [0x4000u16, 0x5000, 0x6000];
        c::cmsAppendNamedColor(nl,
            b"red\0".as_ptr() as *const _,
            pcs.as_ptr(), dev.as_ptr());
        c::cmsAppendNamedColor(nl,
            b"blue\0".as_ptr() as *const _,
            pcs.as_ptr(), dev.as_ptr());
        c::cmsWriteTag(ncp, 0x6E636C32,
            nl as *const _);
        // matrix-shaper profile for WriteInputMatrixShaper
        #[repr(C)]
        struct CieXyy { x: f64, y: f64, yy: f64 }
        let wp = CieXyy { x: 0.3, y: 0.3, yy: 1.0 };
        let prim = [
            CieXyy { x: 0.6, y: 0.3, yy: 1.0 },
            CieXyy { x: 0.2, y: 0.6, yy: 1.0 },
            CieXyy { x: 0.15, y: 0.06, yy: 1.0 },
        ];
        let trcs = [g as *mut std::ffi::c_void,
                    g as *mut std::ffi::c_void,
                    g as *mut std::ffi::c_void];
        let msp = c::cmsCreateRGBProfileTHR(
            std::ptr::null_mut(),
            &wp as *const _ as *const _,
            prim.as_ptr() as *const _,
            trcs.as_ptr());
        // CSA/CRD on named-color + matrix-shaper + srgb,
        //   all intents + flag combos
        for (prof, pname) in [
            (ncp, "ncl"), (srgb, "srgb"),
        ] {
            // named-color CSA only valid at intent 0 —
            //   intents 1+ crash upstream on ncl2
            let ints: &[u32] = if prof == ncp {
                &[0] } else { &[0, 1, 2, 3] };
            for int_ in ints.iter().copied() {
                for fl in [0u32, 0x0040, 0x0100] {
                    let mut buf = vec![0u8; 1 << 18];
                    let csa = c::cmsGetPostScriptCSA(
                        std::ptr::null_mut(), prof, int_,
                        fl, buf.as_mut_ptr() as *mut _,
                        buf.len() as u32);
                    let mut buf2 = vec![0u8; 1 << 18];
                    let crd = c::cmsGetPostScriptCRD(
                        std::ptr::null_mut(), prof, int_,
                        fl, buf2.as_mut_ptr() as *mut _,
                        buf2.len() as u32);
                    if int_ == 0 && fl == 0 {
                        eprintln!("{pname} csa={csa} \
                            crd={crd}");
                    }
                }
            }
        }
        // ColorResource — valid types 0=CSA,1=CRD;
        //   needs an IOHANDLER arg, not a raw buffer
        for ty in [0u32, 1] {
            let mut buf = vec![0u8; 1 << 18];
            let mut nb = buf.len() as u32;
            let io = c::cmsOpenIOhandlerFromMem(
                std::ptr::null_mut(),
                buf.as_mut_ptr() as *mut _,
                nb, b"w\0".as_ptr() as *const _);
            if io.is_null() {
                eprintln!("cres ty={ty} io=0");
                continue;
            }
            let r = c::cmsGetPostScriptColorResource(
                std::ptr::null_mut(), ty, srgb, 0, 0,
                io as *mut _);
            eprintln!("cres ty={ty} r={r}");
            let _ = nb;
            c::cmsCloseIOhandler(io);
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(ncp);
        if !msp.is_null() {
            c::cmsCloseProfile(msp); }
        c::cmsFreeNamedColorList(nl);
        c::cmsFreeToneCurve(g);
    }
}

/// cmsps2/cmserr residual — CSA/CRD emit on matrix +
///   ncl2 + devicelink profiles, intent/flag variants,
///   error-handler get/set round-trips.
#[test]
fn ps2_tail_arms() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab4 = c::cmsCreateLab4Profile(
            std::ptr::null_mut());
        // CSA + CRD emit on srgb/lab4 at intents+flags
        let mut buf = vec![0u8; 1 << 19];
        for (prof, name) in [
            (srgb, "srgb"), (lab4, "lab4"),
        ] {
            for intent in [0u32, 1, 2] {
                for flags in [0u32, 0x10, 0x20] {
                    let n = c::cmsGetPostScriptCSA(
                        std::ptr::null_mut(), prof,
                        intent, flags,
                        buf.as_mut_ptr() as *mut _,
                        buf.len() as u32);
                    let n2 = c::cmsGetPostScriptCRD(
                        std::ptr::null_mut(), prof,
                        intent, flags,
                        buf.as_mut_ptr() as *mut _,
                        buf.len() as u32);
                    eprintln!("{name} i{intent} f{flags:#x}: \
                        csa={n} crd={n2}");
                }
            }
        }
        // error-handler set + fire via a bad read
        unsafe extern "C" fn myerr(
            _ctx: *mut std::ffi::c_void, code: u32,
            _msg: *const i8) {
            eprintln!("  myerr {code}");
        }
        let fp: unsafe extern "C" fn(
            *mut std::ffi::c_void, u32, *const i8) = myerr;
        c::cmsSetLogErrorHandlerTHR(
            std::ptr::null_mut(), fp as *mut _);
        // trigger an error so the handler fires
        let bad = c::cmsOpenProfileFromMem(
            b"\x00".as_ptr() as *const _, 1);
        eprintln!("bad-open={}", !bad.is_null());
        if !bad.is_null() {
            c::cmsCloseProfile(bad); }
        c::cmsSetLogErrorHandlerTHR(
            std::ptr::null_mut(),
            std::ptr::null_mut());
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab4);
    }
}

/// cmsps2 residual — CPD emit + CSA/CRD on more
///   profile shapes (devicelink, ncl2, abstract) +
///   output-flag internals.
#[test]
fn ps2_cpd_arms() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab4 = c::cmsCreateLab4Profile(
            std::ptr::null_mut());
        let mut buf = vec![0u8; 1 << 19];
        // devicelink profile for CSA/CRD emit
        let dl = {
            let p = c::cmsCreateProfilePlaceholder(
                std::ptr::null_mut());
            c::cmsSetDeviceClass(p, 0x6C696E6B);
            c::cmsSetColorSpace(p, 0x52474220);
            c::cmsSetPCS(p, 0x4C616220);
            c::cmsSetProfileVersion(p, 4.4);
            let g = c::cmsBuildGamma(
                std::ptr::null_mut(), 1.0);
            let pipe = c::cmsPipelineAlloc(
                std::ptr::null_mut(), 3, 3);
            let tcs = [g, g, g];
            c::cmsPipelineInsertStage(pipe, 0,
                c::cmsStageAllocToneCurves(
                    std::ptr::null_mut(), 3,
                    tcs.as_ptr() as *mut _));
            c::cmsWriteTag(p, 0x41324230,
                pipe as *const _);
            let mut bb = [0u8; 1 << 17];
            let mut n = bb.len() as u32;
            let sv = c::cmsSaveProfileToMem(p,
                bb.as_mut_ptr() as *mut _, &mut n);
            let pr = if sv != 0 {
                c::cmsOpenProfileFromMem(
                    bb.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            c::cmsCloseProfile(p);
            c::cmsPipelineFree(pipe);
            pr
        };
        for (prof, name) in [
            (srgb, "srgb"), (lab4, "lab4"), (dl, "dl"),
        ] {
            if prof.is_null() { continue; }
            for i in [0u32, 2] {
                let n = c::cmsGetPostScriptCSA(
                    std::ptr::null_mut(), prof, i, 0,
                    buf.as_mut_ptr() as *mut _,
                    buf.len() as u32);
                let n2 = c::cmsGetPostScriptCRD(
                    std::ptr::null_mut(), prof, i, 0,
                    buf.as_mut_ptr() as *mut _,
                    buf.len() as u32);
                eprintln!("{name} i{i} csa={n} crd={n2}");
            }
        }
        if !dl.is_null() {
            c::cmsCloseProfile(dl); }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab4);
    }
}
