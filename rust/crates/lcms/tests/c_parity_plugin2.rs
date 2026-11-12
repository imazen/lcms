//! cmsPluginTHR validation arms — magic/version/type checks,
//! Next chains, per-sig register paths.
#![allow(clippy::missing_safety_doc)]
#[path = "support/solo.rs"]
mod solo;
use lcms_cref as c;
use std::ffi::c_void;

#[repr(C)]
struct PluginBase {
    magic: u32,
    expected_version: u32,
    ty: u32,
    _pad: u32,
    next: *mut PluginBase,
}

const MAGIC: u32 = 0x61637070;
const SIGS: &[(u32, &str)] = &[
    (0x6D656D48, "memH"),
    (0x696E7048, "inpH"),
    (0x70617248, "parH"),
    (0x66726D48, "frmH"),
    (0x74797048, "typH"),
    (0x74616748, "tagH"),
    (0x696E7448, "intH"),
    (0x6D706548, "mpeH"),
    (0x6F707448, "optH"),
    (0x7A666D48, "xfmH"),
    (0x6D747A48, "mtxH"),
    (0x70726C48, "prlH"),
    (0x68647248, "hdrH"),
];

#[test]
fn plugin_validation_arms() {
    if solo::delegated("plugin_validation_arms") {
        return;
    }
    // Registered plugins persist in the global context —
    // fork-isolate so handlers can't poison later tests
    unsafe extern "C" {
        fn fork() -> i32;
        fn waitpid(p: i32, s: *mut i32, o: i32) -> i32;
        fn _exit(code: i32) -> !;
        fn dlsym(h: *mut std::ffi::c_void, n: *const i8) -> *mut std::ffi::c_void;
    }
    let flush: Option<unsafe extern "C" fn() -> i32> = unsafe {
        let f = dlsym(
            std::ptr::null_mut(),
            b"__llvm_profile_write_file\0".as_ptr() as *const i8,
        );
        if f.is_null() {
            None
        } else {
            Some(std::mem::transmute(f))
        }
    };
    let pid = unsafe { fork() };
    if pid == 0 {
        unsafe {
            // bad magic → reject
            let mut p = PluginBase {
                magic: 0xDEADBEEF,
                expected_version: 2200,
                ty: 0x6D656D48,
                _pad: 0,
                next: std::ptr::null_mut(),
            };
            let r = c::cmsPluginTHR(std::ptr::null_mut(), &mut p as *mut _ as *mut c_void);
            eprintln!("badmagic={r}");
            // version from the future → reject
            let mut p2 = PluginBase {
                magic: MAGIC,
                expected_version: 99999,
                ty: 0x6D656D48,
                _pad: 0,
                next: std::ptr::null_mut(),
            };
            let r2 = c::cmsPluginTHR(std::ptr::null_mut(), &mut p2 as *mut _ as *mut c_void);
            eprintln!("futurever={r2}");
            // bogus type → unsupported arm
            let mut p3 = PluginBase {
                magic: MAGIC,
                expected_version: 2200,
                ty: 0x62616420,
                _pad: 0,
                next: std::ptr::null_mut(),
            };
            let r3 = c::cmsPluginTHR(std::ptr::null_mut(), &mut p3 as *mut _ as *mut c_void);
            eprintln!("bogus={r3}");
            // every plugin sig — truncated payload drives each
            // _cmsRegister*Plugin's validation arm
            for (sig, name) in SIGS {
                let mut raw = [0u64; 32];
                let h = raw.as_mut_ptr() as *mut PluginBase;
                (*h).magic = MAGIC;
                (*h).expected_version = 2200;
                (*h).ty = *sig;
                (*h).next = std::ptr::null_mut();
                let rr = c::cmsPluginTHR(std::ptr::null_mut(), raw.as_mut_ptr() as *mut c_void);
                eprintln!("sig {name}={rr}");
            }
            // chained plugin list — Next->Next iteration arm
            let mut a = PluginBase {
                magic: MAGIC,
                expected_version: 2200,
                ty: 0x62616420,
                _pad: 0,
                next: std::ptr::null_mut(),
            };
            let mut b = PluginBase {
                magic: MAGIC,
                expected_version: 2200,
                ty: 0x62616421,
                _pad: 0,
                next: std::ptr::null_mut(),
            };
            a.next = &mut b;
            let r4 = c::cmsPluginTHR(std::ptr::null_mut(), &mut a as *mut _ as *mut c_void);
            eprintln!("chain={r4}");
            // NULL plugin → loop skipped, returns TRUE
            let r5 = c::cmsPluginTHR(std::ptr::null_mut(), std::ptr::null_mut());
            eprintln!("null={r5}");
            // valid parametric-curve plugin — register then eval
            let mut mem = [0u64; 64];
            let h = mem.as_mut_ptr() as *mut PluginBase;
            (*h).magic = MAGIC;
            (*h).expected_version = 2200;
            (*h).ty = 0x70617248; // parH
            (*h).next = std::ptr::null_mut();
            let r6 = c::cmsPluginTHR(std::ptr::null_mut(), mem.as_mut_ptr() as *mut c_void);
            eprintln!("parH-zero={r6}");
            // mutex plugin sig
            let mut mem2 = [0u64; 64];
            let h2 = mem2.as_mut_ptr() as *mut PluginBase;
            (*h2).magic = MAGIC;
            (*h2).expected_version = 2200;
            (*h2).ty = 0x6D747A48; // mtxH
            (*h2).next = std::ptr::null_mut();
            let r7 = c::cmsPluginTHR(std::ptr::null_mut(), mem2.as_mut_ptr() as *mut c_void);
            eprintln!("mtxH-zero={r7}");
            // hdrH — header/parallelization sigs
            for sig in [0x70726C48u32, 0x68647248] {
                let mut m3 = [0u64; 64];
                let h3 = m3.as_mut_ptr() as *mut PluginBase;
                (*h3).magic = MAGIC;
                (*h3).expected_version = 2200;
                (*h3).ty = sig;
                (*h3).next = std::ptr::null_mut();
                let rr = c::cmsPluginTHR(std::ptr::null_mut(), m3.as_mut_ptr() as *mut c_void);
                eprintln!("sig {sig:#x}={rr}");
            }
            if let Some(f) = flush {
                f();
            }
            _exit(0);
        }
    }
    let mut st = 0i32;
    unsafe {
        waitpid(pid, &mut st, 0);
    }
    eprintln!("plugin_validation child st={st}");
}
