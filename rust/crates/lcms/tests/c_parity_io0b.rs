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
const CREATED_OFF: usize = 24;

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

/// cmsio1 residual — _cmsReadCHAD v2-display-missing-
///   wtpt identity path, named-color Lab-PCS -> V2ToV4
///   stage, Lab-cs+Lab-PCS mft1 -> V4ToV2/V2ToV4.
#[test]
fn internal_chad_labv_arms() {
    unsafe {
        // v2 display-class profile with NO wtpt tag
        //   -> CHAD identity path
        let p1 = c::cmsCreateProfilePlaceholder(
            std::ptr::null_mut());
        c::cmsSetDeviceClass(p1, 0x6D6E7472);
        c::cmsSetColorSpace(p1, 0x52474220);
        c::cmsSetPCS(p1, 0x58595A20);
        c::cmsSetProfileVersion(p1, 2.4);
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let trcs = [g, g, g];
        for sig in [0x72545243u32, 0x67545243, 0x62545243] {
            c::cmsWriteTag(p1, sig,
                g as *const _);
        }
        let xyz = [0x0000F6D6u32, 0x00010000, 0x0000D32D];
        for sig in [0x7258595Au32, 0x6758595A, 0x6258595A] {
            c::cmsWriteTag(p1, sig,
                xyz.as_ptr() as *const _);
        }
        let mut buf = [0u8; 32768];
        let mut bn = 32768u32;
        c::cmsSaveProfileToMem(p1,
            buf.as_mut_ptr() as *mut _, &mut bn);
        let p1r = c::cmsOpenProfileFromMem(
            buf.as_ptr() as *const _, 32768);
        eprintln!("v2-disp-no-wtpt prof={}",
            !p1r.is_null());
        let fin = (0u32 << 16) | (3 << 3) | 2;
        let t = c::cmsCreateTransform(p1r, fin,
            p1r, fin, 1, 0);
        eprintln!("  intent1-xform t={}", !t.is_null());
        if !t.is_null() {
            c::cmsDeleteTransform(t); }
        if !p1r.is_null() {
            c::cmsCloseProfile(p1r); }
        c::cmsCloseProfile(p1);
        // named-color profile with Lab PCS -> LabV2ToV4
        let ncp = c::cmsCreateProfilePlaceholder(
            std::ptr::null_mut());
        c::cmsSetDeviceClass(ncp, 0x6E6D636C);
        c::cmsSetColorSpace(ncp, 0x46434C52);
        c::cmsSetPCS(ncp, 0x4C616220);
        let nl = c::cmsAllocNamedColorList(
            std::ptr::null_mut(), 2, 3,
            b"P\0".as_ptr() as *const _,
            b"S\0".as_ptr() as *const _);
        let pcs3 = [0x1000u16, 0x2000, 0x3000];
        let dev3 = [0x4000u16, 0x5000, 0x6000];
        c::cmsAppendNamedColor(nl,
            b"a\0".as_ptr() as *const _,
            pcs3.as_ptr(), dev3.as_ptr());
        c::cmsAppendNamedColor(nl,
            b"b\0".as_ptr() as *const _,
            pcs3.as_ptr(), dev3.as_ptr());
        c::cmsWriteTag(ncp, 0x6E636C32,
            nl as *const _);
        let tn = c::cmsCreateTransform(ncp,
            (3u32 << 16) | (1 << 3) | 2, // gray->lab-ish
            ncp, fin, 0, 0);
        eprintln!("ncl-lab t={}", !tn.is_null());
        if !tn.is_null() {
            c::cmsDeleteTransform(tn); }
        c::cmsCloseProfile(ncp);
        c::cmsFreeNamedColorList(nl);
        // Lab colorspace + Lab PCS + mft1 tag
        let lab2 = c::cmsCreateLab2Profile(
            std::ptr::null_mut());
        let lab4 = c::cmsCreateLab4Profile(
            std::ptr::null_mut());
        let t1 = c::cmsCreateTransform(lab2,
            (12u32 << 16) | (3 << 3) | 2,
            lab4, (12u32 << 16) | (3 << 3) | 2, 0, 0);
        eprintln!("lab2->lab4 t={}", !t1.is_null());
        if !t1.is_null() {
            c::cmsDeleteTransform(t1); }
        c::cmsCloseProfile(lab2);
        c::cmsCloseProfile(lab4);
        c::cmsFreeToneCurve(g);
    }
}
