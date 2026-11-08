//! cmstypes tag-level parity: every tag in the bundled corpus is raw-read
//! on both sides (payload byte-exact), raw-written into a copy and re-read,
//! and the full profile re-serializes identically. Exercises the per-tag
//! type readers/writers incl. MPE tag types (ICC.1:2022-05 mAB/mBA/mpet).

use lcms::profile;
use lcms_cref as c;
use std::ffi::c_void;

fn c_open(data: &[u8]) -> *mut c_void {
    unsafe { c::cmsOpenProfileFromMem(data.as_ptr() as _, data.len() as u32) }
}

fn c_read_raw(h: *mut c_void, sig: u32) -> Option<Vec<u8>> {
    unsafe {
        let n = c::cmsReadRawTag(h, sig, std::ptr::null_mut(), 0);
        if n == 0 {
            return None;
        }
        let mut b = vec![0u8; n as usize];
        let got = c::cmsReadRawTag(h, sig, b.as_mut_ptr() as *mut _, n);
        if got == 0 {
            return None;
        }
        b.truncate(got as usize);
        Some(b)
    }
}

fn r_read_raw(p: &mut lcms::profile::IccProfile, sig: u32) -> Option<Vec<u8>> {
    let n = profile::cms_read_raw_tag(p, sig, None);
    if n == 0 {
        return None;
    }
    let mut b = vec![0u8; n as usize];
    let got = profile::cms_read_raw_tag(p, sig, Some(&mut b));
    if got == 0 {
        return None;
    }
    b.truncate(got as usize);
    Some(b)
}

fn corpus() -> Vec<(String, Vec<u8>)> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "icc").unwrap_or(false))
        .collect();
    files.sort();
    files
        .into_iter()
        .map(|f| {
            (
                f.file_name().unwrap().to_string_lossy().into_owned(),
                std::fs::read(&f).unwrap(),
            )
        })
        .collect()
}

#[test]
fn raw_tag_reads() {
    for (name, data) in corpus() {
        let ch = c_open(&data);
        assert!(!ch.is_null(), "{name}: C open");
        let mut rp = profile::open_profile_from_mem(None, &data).expect("rust open");
        let n_tags = unsafe { c::cmsGetTagCount(ch) };
        assert_eq!(
            n_tags as u32,
            rp.tag_count,
            "{name}: tag count"
        );
        for i in 0..n_tags {
            let sig = unsafe { c::cmsGetTagSignature(ch, i as u32) };
            // is-tag parity
            unsafe {
                assert_eq!(
                    c::cmsIsTag(ch, sig),
                    profile::cms_is_tag(&rp, sig) as i32,
                    "{name}: is_tag {sig:#010x}"
                );
            }
            let cb = c_read_raw(ch, sig);
            let rb = r_read_raw(&mut rp, sig);
            assert_eq!(
                cb.is_some(),
                rb.is_some(),
                "{name}: read_raw presence {sig:#010x}"
            );
            if let (Some(cb), Some(rb)) = (cb, rb) {
                assert_eq!(cb, rb, "{name}: raw payload {sig:#010x}");
            }
        }
        unsafe { c::cmsCloseProfile(ch) };
    }
}

#[test]
fn raw_tag_write_roundtrip() {
    for (name, data) in corpus() {
        let ch = c_open(&data);
        let mut rp = profile::open_profile_from_mem(None, &data).expect("rust open");
        let n_tags = unsafe { c::cmsGetTagCount(ch) };
        for i in 0..n_tags {
            let sig = unsafe { c::cmsGetTagSignature(ch, i as u32) };
            let Some(cb) = c_read_raw(ch, sig) else { continue };
            // write raw into both, read back
            let cw = unsafe { c::cmsWriteRawTag(ch, sig, cb.as_ptr() as _, cb.len() as u32) };
            let rw = profile::cms_write_raw_tag(&mut rp, sig, &cb);
            assert_eq!(cw != 0, rw, "{name}: write_raw status {sig:#010x}");
            let cback = c_read_raw(ch, sig);
            let rback = r_read_raw(&mut rp, sig);
            assert_eq!(
                cback, rback,
                "{name}: raw roundtrip {sig:#010x}"
            );
        }
        unsafe { c::cmsCloseProfile(ch) };
    }
}

/// Parsed-tag read then full re-serialize — drives each tag-type's reader.
#[test]
fn parsed_tag_resave() {
    for (name, data) in corpus() {
        let ch = c_open(&data);
        let mut rp = profile::open_profile_from_mem(None, &data).expect("rust open");
        let n_tags = unsafe { c::cmsGetTagCount(ch) };
        for i in 0..n_tags {
            let sig = unsafe { c::cmsGetTagSignature(ch, i as u32) };
            // force the parsed reader on both sides
            let ct = unsafe { c::cmsReadTag(ch, sig) };
            let rt = profile::cms_read_tag(&mut rp, sig).map(|_| ());
            assert_eq!(
                ct.is_null(),
                rt.is_none(),
                "{name}: read_tag presence {sig:#010x}"
            );
        }
        // resave — byte-compare whole profile (header created field masked)
        let mut cn = 0u32;
        let c_out = unsafe {
            assert!(c::cmsSaveProfileToMem(ch, std::ptr::null_mut(), &mut cn) != 0);
            let mut b = vec![0u8; cn as usize];
            assert!(c::cmsSaveProfileToMem(ch, b.as_mut_ptr() as *mut _, &mut cn) != 0);
            c::cmsCloseProfile(ch);
            b
        };
        let r_out = profile::save_profile_to_mem(&mut rp).expect("rust save");
        let n = c_out.len().min(r_out.len());
        for off in 0..n {
            if (24..36).contains(&off) {
                continue; // created timestamp — nondeterministic
            }
            assert_eq!(
                c_out[off], r_out[off],
                "{name}: resave byte @{off:#x}"
            );
        }
        assert_eq!(c_out.len(), r_out.len(), "{name}: resave length");
    }
}
