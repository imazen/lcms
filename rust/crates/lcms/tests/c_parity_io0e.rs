//! cmsio0 disk-I/O path: cmsOpenProfileFromFile on the corpus files +
//! cmsGetProfileIOhandler — the file→IOhandler→profile chain.

use lcms::profile;
use lcms_cref as c;

fn corpus_paths() -> Vec<std::path::PathBuf> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
    let mut files: Vec<_> = std::fs::read_dir(&dir).unwrap()
        .filter_map(|e| e.ok()).map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "icc").unwrap_or(false))
        .collect();
    files.sort();
    files
}

#[test]
fn file_open_direct() {
    for f in corpus_paths() {
        let name = std::ffi::CString::new(f.to_str().unwrap()).unwrap();
        unsafe {
            let h = c::cmsOpenProfileFromFile(name.as_ptr(), b"r\0".as_ptr());
            let data = std::fs::read(&f).unwrap();
            let rr = profile::open_profile_from_mem(None, &data);
            assert_eq!(h.is_null(), rr.is_none(), "file open {}",
                f.display());
            if !h.is_null() {
                // header parity between file-open and mem-open
                let mut flags = 0u32;
                c::cmsGetHeaderFlags(h, &mut flags);
                assert_eq!(flags, rr.unwrap().flags, "file-open flags {}",
                    f.display());
                // IOhandler accessor presence
                let io = c::cmsGetProfileIOhandler(h);
                assert!(!io.is_null(), "iohandler accessor");
                c::cmsCloseProfile(h);
            }
        }
    }
}
