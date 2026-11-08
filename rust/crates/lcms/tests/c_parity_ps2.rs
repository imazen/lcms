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

