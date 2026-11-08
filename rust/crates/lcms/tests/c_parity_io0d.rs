//! cmsio0 IOhandler-dispatch: SaveProfileToIOhandler +
//! OpenProfileFromIOhandler via FILE handlers — the dispatch
//! machinery beyond SaveProfileToMem/File.

use lcms::profile;
use lcms_cref as c;

fn corpus() -> Vec<(String, Vec<u8>)> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
    let mut files: Vec<_> = std::fs::read_dir(&dir).unwrap()
        .filter_map(|e| e.ok()).map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "icc").unwrap_or(false))
        .collect();
    files.sort();
    files.iter().map(|f| {
        (f.to_str().unwrap().to_string(), std::fs::read(f).unwrap())
    }).collect()
}

#[test]
fn file_iohandler_open() {
    // cmsOpenProfileFromIOhandlerTHR on a FILE handler — exercises
    // the dispatch+read machinery. The profile owns the handler on
    // success; the handler is freed by Close.
    for (path, data) in corpus() {
        unsafe {
            let name = std::ffi::CString::new(path.as_str()).unwrap();
            let io = c::cmsOpenIOhandlerFromFile(
                std::ptr::null_mut(), name.as_ptr(), b"r\0".as_ptr());
            assert!(!io.is_null(), "iohandler open {path}");
            let h = c::cmsOpenProfileFromIOhandlerTHR(std::ptr::null_mut(), io);
            let rr = profile::open_profile_from_mem(None, &data);
            assert_eq!(h.is_null(), rr.is_none(), "iohandler-from-file {path}");
            if !h.is_null() { let _ = c::cmsCloseProfile(h); }
            else { let _ = c::cmsCloseIOhandler(io); }
        }
    }
}

#[test]
fn null_iohandler_dry_run() {
    // cmsOpenIOhandlerFromNULL — the size-query dry-run
    for (_, data) in corpus() {
        unsafe {
            let io = c::cmsOpenIOhandlerFromNULL(std::ptr::null_mut());
            let prof = c::cmsOpenProfileFromMem(
                data.as_ptr() as _, data.len() as u32);
            let ok = c::cmsSaveProfileToIOhandler(prof, io);
            let _ = c::cmsCloseProfile(prof);
            let _ = c::cmsCloseIOhandler(io);
            assert_eq!(ok != 0, true, "null-handler save");
        }
    }
}

/// SaveProfileToIOhandler on a write-mode file handler + the
/// GetProfileIOhandler getter.
#[test]
fn save_to_file_iohandler() {
    for (_, data) in corpus() {
        let tmp = std::env::temp_dir()
            .join(format!("lcms-io0d-{}.icc", std::process::id()));
        unsafe {
            let prof = c::cmsOpenProfileFromMem(
                data.as_ptr() as _, data.len() as u32);
            assert!(!prof.is_null());
            // getter on a mem-opened profile — C returns the internal
            // handler it created for the mem read (non-NULL)
            let io0 = c::cmsGetProfileIOhandler(prof);
            assert!(!io0.is_null(),
                "mem profile iohandler is the internal mem handler");
            // write-mode file handler
            let path = std::ffi::CString::new(
                tmp.to_str().unwrap()).unwrap();
            let io = c::cmsOpenIOhandlerFromFile(
                std::ptr::null_mut(), path.as_ptr(), b"w\0".as_ptr());
            assert!(!io.is_null());
            let ok = c::cmsSaveProfileToIOhandler(prof, io);
            assert!(ok != 0, "save to file iohandler");
            let _ = c::cmsCloseIOhandler(io);
            let _ = c::cmsCloseProfile(prof);
        }
        let _ = std::fs::remove_file(&tmp);
    }
}

/// corrupt-profile mutations — byte-level corruption exercises
/// the parser's validation branches.
#[test]
fn corrupt_profile_bytes() {
    unsafe {
        let base = c::cmsCreate_sRGBProfile();
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(base, std::ptr::null_mut(), &mut cn);
        let mut good = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(base, good.as_mut_ptr() as *mut _,
            &mut cn);
        c::cmsCloseProfile(base);
        // byte mutations at parser-relevant offsets
        for (name, off, val) in [
            ("size-hi", 0u32, 0xFF),
            ("size-lo", 3, 0x00),
            ("cmm", 4, 0xFF),
            ("version-hi", 8, 0x05),
            ("class", 12, 0xFF),
            ("cs", 16, 0xFF),
            ("pcs", 20, 0xFF),
            ("sig", 36, 0xFF),
            ("ntags", 128, 0xFF),
            ("tag-off", 136, 0xFF),
            ("tag-size", 140, 0xFF),
        ] {
            let mut d = good.clone();
            d[off as usize] = val;
            let h = c::cmsOpenProfileFromMem(d.as_ptr() as _,
                d.len() as u32);
            let ok = !h.is_null();
            eprintln!("{name} off={off} open={ok}");
            if ok {
                // read a tag — may reject on the corrupted field
                let _ = c::cmsReadTag(h, 0x64657363);
                c::cmsCloseProfile(h);
            }
        }
    }
}

