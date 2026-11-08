//! cmsio0 file/IOhandler paths: SaveProfileToFile + OpenProfileFromFile
//! round-trip on the corpus, GetHeaderRenderingIntent parity.

use lcms::profile;
use lcms_cref as c;

fn corpus() -> Vec<Vec<u8>> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
    let mut files: Vec<_> = std::fs::read_dir(&dir).unwrap()
        .filter_map(|e| e.ok()).map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "icc").unwrap_or(false))
        .collect();
    files.sort();
    files.iter().map(|f| std::fs::read(f).unwrap()).collect()
}

#[test]
fn file_open_save_roundtrip() {
    for data in corpus() {
        let mut rp = profile::open_profile_from_mem(None, &data).unwrap();
        let tmp = std::env::temp_dir().join(format!("lcms_{}.icc", rp.profile_id.id8[0]));
        let c_ok = unsafe {
            let h = c::cmsOpenProfileFromMem(data.as_ptr() as _, data.len() as u32);
            let name = std::ffi::CString::new(tmp.to_str().unwrap()).unwrap();
            let ok = c::cmsSaveProfileToFile(h, name.as_ptr());
            c::cmsCloseProfile(h);
            ok
        };
        let r_bytes = profile::save_profile_to_mem(&mut rp).unwrap_or_default();
        assert_eq!(c_ok != 0, !r_bytes.is_empty(), "save status");
        if c_ok != 0 {
            // C's file save vs rust's mem save — the file content should
            // match the mem bytes (same serialization path)
            let c_bytes = std::fs::read(&tmp).unwrap_or_default();
            let mut c_out = c_bytes;
            let mut r_out = r_bytes;
            for i in 0..12 {
                c_out.get_mut(24 + i).map(|b| *b = 0);
                r_out.get_mut(24 + i).map(|b| *b = 0);
            }
            c_out.resize(r_out.len().max(c_out.len()), 0);
            r_out.resize(c_out.len(), 0);
            assert_eq!(c_out, r_out, "file-vs-mem bytes");
            // open the saved file on both sides — header parity
            let (c_ok2, c_flags) = unsafe {
                let name = std::ffi::CString::new(tmp.to_str().unwrap()).unwrap();
                let h = c::cmsOpenProfileFromFile(name.as_ptr(), b"r\0".as_ptr());
                if h.is_null() { (false, 0) } else {
                    let mut f = 0u32;
                    c::cmsGetHeaderFlags(h, &mut f);
                    c::cmsCloseProfile(h);
                    (true, f)
                }
            };
            let rrp = profile::open_profile_from_mem(None, &std::fs::read(&tmp).unwrap());
            assert_eq!(c_ok2, rrp.is_some(), "reopen presence");
            if let Some(rrp) = rrp {
                assert_eq!(c_flags, rrp.flags, "reopen flags");
            }
        }
        let _ = std::fs::remove_file(&tmp);
    }
}

#[test]
fn rendering_intent_getter() {
    for data in corpus() {
        let rp = profile::open_profile_from_mem(None, &data).unwrap();
        let c_intent = unsafe {
            let h = c::cmsOpenProfileFromMem(data.as_ptr() as _, data.len() as u32);
            let i = c::cmsGetHeaderRenderingIntent(h);
            c::cmsCloseProfile(h);
            i
        };
        assert_eq!(c_intent, rp.rendering_intent, "rendering_intent");
    }
}
