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
