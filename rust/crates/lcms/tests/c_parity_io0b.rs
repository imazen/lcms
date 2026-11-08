//! cmsio0 remaining save/serialize paths: save-to-mem byte-compare
//! across the corpus, GetHeaderCreationDateTime field parity.

use lcms::profile;
use lcms_cref as c;
use std::ffi::c_void;

/// C `struct tm` — fields used by cmsGetHeaderCreationDateTime.
#[repr(C)]
#[derive(Default, Debug, PartialEq)]
struct CTm {
    sec: i32, min: i32, hour: i32, mday: i32, mon: i32,
    year: i32, wday: i32, yday: i32, isdst: i32,
    gmtoff: i64, zone: *const i8,
}

fn corpus() -> Vec<Vec<u8>> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
    let mut files: Vec<_> = std::fs::read_dir(&dir).unwrap()
        .filter_map(|e| e.ok()).map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "icc").unwrap_or(false))
        .collect();
    files.sort();
    files.iter().map(|f| std::fs::read(f).unwrap()).collect()
}

/// ICC header `created` field offset (year..sec, 12 bytes at 36).
const CREATED_OFF: usize = 36;

#[test]
fn save_to_mem_reserialize() {
    for data in corpus() {
        let mut c_out = unsafe {
            let h = c::cmsOpenProfileFromMem(data.as_ptr() as _, data.len() as u32);
            if h.is_null() { continue; }
            let mut buf = vec![0u8; 1 << 20];
            let mut size: u32 = buf.len() as u32;
            let ok = c::cmsSaveProfileToMem(h, buf.as_mut_ptr() as *mut c_void, &mut size);
            c::cmsCloseProfile(h);
            if ok == 0 { continue; }
            buf.truncate(size as usize);
            buf
        };
        let mut rp = match profile::open_profile_from_mem(None, &data) {
            Some(p) => p,
            None => { assert!(c_out.is_empty(), "C saved but rust can't open"); continue; }
        };
        let mut r_out = profile::save_profile_to_mem(&mut rp).unwrap_or_default();
        // mask the volatile `created` timestamp — C rewrites it on save
        for i in 0..12 {
            c_out.get_mut(CREATED_OFF + i).map(|b| *b = 0);
            r_out.get_mut(CREATED_OFF + i).map(|b| *b = 0);
        }
        let mut c_out = c_out;
        c_out.resize(r_out.len().max(c_out.len()), 0);
        r_out.resize(c_out.len(), 0);
        assert_eq!(c_out, r_out, "save_to_mem reserialize");
    }
}

#[test]
fn header_creation_datetime() {
    for data in corpus() {
        let mut ct = CTm::default();
        let ok = unsafe {
            let h = c::cmsOpenProfileFromMem(data.as_ptr() as _, data.len() as u32);
            if h.is_null() { continue; }
            let ok = c::cmsGetHeaderCreationDateTime(h, &mut ct as *mut _ as *mut c_void);
            c::cmsCloseProfile(h);
            ok
        };
        let rp = profile::open_profile_from_mem(None, &data).unwrap();
        // rust side: read the raw `created` header bytes directly (the
        // profile struct keeps them verbatim) and compare to C's tm
        assert_eq!(ok, 1, "C get_header_creation_date_time");
        let d = &rp.created;
        assert_eq!(ct.year + 1900, d.year as i32, "tm year");
        assert_eq!(ct.mon, d.month as i32 - 1, "tm mon");
        assert_eq!(ct.mday, d.day as i32, "tm mday");
        assert_eq!(ct.hour, d.hours as i32, "tm hour");
        assert_eq!(ct.min, d.minutes as i32, "tm min");
        assert_eq!(ct.sec, d.seconds as i32, "tm sec");
    }
}
