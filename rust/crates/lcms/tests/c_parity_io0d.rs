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
