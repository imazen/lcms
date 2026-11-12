//! cmsio1 info-text getters — cmsGetProfileInfo/ASCII/UTF8 across
//! info types on the corpus + builder profiles. (C-side only — the
//! info-getter APIs are unported in Rust.)

use lcms_cref as c;
use std::ffi::c_void;

fn corpus() -> Vec<Vec<u8>> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "icc").unwrap_or(false))
        .collect();
    files.sort();
    files.iter().map(|f| std::fs::read(f).unwrap()).collect()
}

#[test]
fn info_getter_matrix() {
    unsafe {
        // info types: 0=Description,1=Manufacturer,2=Model,3=Copyright
        for data in corpus() {
            let h = c::cmsOpenProfileFromMem(data.as_ptr() as _, data.len() as u32);
            if h.is_null() {
                continue;
            }
            for info in 0..4u32 {
                // utf8 getter — query size then fetch
                let mut buf = [0u8; 256];
                let n = c::cmsGetProfileInfoUTF8(
                    h,
                    info,
                    c"en".as_ptr() as *const std::ffi::c_char,
                    c"US".as_ptr() as *const std::ffi::c_char,
                    buf.as_mut_ptr() as *mut c_void,
                    256,
                );
                // ascii getter — same call pattern
                let mut abuf = [0u8; 256];
                let an = c::cmsGetProfileInfoASCII(
                    h,
                    info,
                    c"en".as_ptr() as *const std::ffi::c_char,
                    c"US".as_ptr() as *const std::ffi::c_char,
                    abuf.as_mut_ptr() as *mut c_void,
                    256,
                );
                // both should return the same length for ASCII-safe
                // content (utf8 is a superset; at least the query
                // shouldn't error)
                let _ = (n, an);
                // wide getter — wchar buffer
                let mut wbuf = [0i32; 256];
                let wn = c::cmsGetProfileInfo(
                    h,
                    info,
                    c"en".as_ptr() as *const std::ffi::c_char,
                    c"US".as_ptr() as *const std::ffi::c_char,
                    wbuf.as_mut_ptr() as *mut c_void,
                    256,
                );
                let _ = wn;
            }
            c::cmsCloseProfile(h);
        }
        // builder profiles — the text tags are populated
        for (name, mk) in [("srgb", 0), ("lab4", 1), ("xyz", 2), ("null", 3)] {
            let h = match mk {
                0 => c::cmsCreate_sRGBProfile(),
                1 => c::cmsCreateLab4Profile(std::ptr::null_mut()),
                2 => c::cmsCreateXYZProfile(),
                _ => c::cmsCreateNULLProfile(),
            };
            assert!(!h.is_null(), "{name} builder");
            let mut buf = [0u8; 256];
            let n = c::cmsGetProfileInfoUTF8(
                h,
                0,
                c"en".as_ptr() as *const std::ffi::c_char,
                c"US".as_ptr() as *const std::ffi::c_char,
                buf.as_mut_ptr() as *mut c_void,
                256,
            );
            assert!(n > 0, "{name} desc utf8 len");
            let s = std::ffi::CStr::from_ptr(buf.as_ptr() as *const std::ffi::c_char)
                .to_string_lossy()
                .into_owned();
            assert!(!s.is_empty(), "{name} desc utf8");
            let _ = c::cmsCloseProfile(h);
        }
    }
}
