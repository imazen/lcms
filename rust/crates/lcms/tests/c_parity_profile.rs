//! Differential gate for cmsio0: open real testbed profiles, compare the
//! tag inventory and per-tag read (cook) success between the C oracle and
//! the Rust port.

use lcms::profile;
use lcms::signatures;
use lcms_cref as c;
use std::ffi::CStr;
use std::ptr;

const REF: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../reference/lcms2/testbed/"
);

#[test]
fn open_profiles_inventory() {
    for name in [
        "test1.icc",
        "test2.icc",
        "test3.icc",
        "test5.icc",
        "crayons.icc",
        "ibm-t61.icc",
    ] {
        let path = format!("{REF}{name}");
        let data = std::fs::read(&path).expect(&path);

        // C side: open + count tags
        let c_path = std::ffi::CString::new(path.clone()).unwrap();
        let c_prof = unsafe { c::cmsOpenProfileFromFile(c_path.as_ptr(), b"r\0".as_ptr()) };
        assert!(!c_prof.is_null(), "C failed to open {name}");
        let c_n = unsafe { c::cmsGetTagCount(c_prof) };

        // Rust side
        let mut icc = profile::open_profile_from_mem(None, &data)
            .unwrap_or_else(|| panic!("rust failed to open {name}"));
        assert_eq!(icc.tag_count as i32, c_n, "tag count {name}");

        // Tag names + sizes must match exactly
        for i in 0..c_n {
            let c_sig = unsafe { c::cmsGetTagSignature(c_prof, i as u32) };
            let (mut off, mut sz) = (0u32, 0u32);
            assert!(unsafe { c::cmsGetTagOffsetAndSize(c_prof, i as u32, &mut off, &mut sz) } != 0);
            assert_eq!(icc.tag_names[i as usize], c_sig, "tag sig {name}[{i}]");
            assert_eq!(icc.tag_offsets[i as usize], off, "tag off {name}[{i}]");
            assert_eq!(icc.tag_sizes[i as usize], sz, "tag sz {name}[{i}]");
        }
        unsafe { c::cmsCloseProfile(c_prof) };
    }
}

#[test]
fn read_all_tags_both_sides() {
    for name in ["test5.icc", "crayons.icc", "test1.icc"] {
        let path = format!("{REF}{name}");
        let data = std::fs::read(&path).unwrap();
        let c_path = std::ffi::CString::new(path.clone()).unwrap();
        let c_prof = unsafe { c::cmsOpenProfileFromFile(c_path.as_ptr(), b"r\0".as_ptr()) };
        let mut icc = profile::open_profile_from_mem(None, &data).unwrap();
        let c_n = unsafe { c::cmsGetTagCount(c_prof) };

        for i in 0..c_n {
            let c_sig = unsafe { c::cmsGetTagSignature(c_prof, i as u32) };
            let c_obj = unsafe { c::cmsReadTag(c_prof, c_sig) };
            let c_ok = !c_obj.is_null();
            let r_ok = profile::cms_read_tag(&mut icc, c_sig).is_some();
            let sig_str = c_sig.to_be_bytes();
            let sig_str = unsafe { CStr::from_bytes_until_nul(&sig_str) }
                .ok()
                .map(|s| s.to_str().unwrap_or("?").to_string())
                .unwrap_or_default();
            assert_eq!(r_ok, c_ok, "read tag {sig_str} ({c_sig:#x}) in {name}");
        }
        unsafe { c::cmsCloseProfile(c_prof) };
    }
}

#[test]
fn write_read_roundtrip_mem() {
    // Build a profile in Rust, save it, reopen it, read the tag back.
    let mut icc = profile::create_profile_placeholder(None);
    // write a text description tag — copyright 'cprt'
    let mut mlu = lcms::named::mlu_alloc(None, 1).unwrap();
    lcms::named::mlu_set_ascii(&mut mlu, b"en", b"US", b"Test Copyright");
    let ok = profile::cms_write_tag(
        &mut icc,
        signatures::tag::SigCopyrightTag,
        Some(lcms::tagtypes::TagObject::Mlu(mlu)),
    );
    assert!(ok, "write copyright tag");

    // media white point XYZ
    let xyz = lcms::types::CieXyz {
        X: 0.9642,
        Y: 1.0,
        Z: 0.8249,
    };
    let ok = profile::cms_write_tag(
        &mut icc,
        signatures::tag::SigMediaWhitePointTag,
        Some(lcms::tagtypes::TagObject::Xyz(xyz)),
    );
    assert!(ok, "write wtpt tag");

    let bytes = profile::save_profile_to_mem(&mut icc).expect("save profile");
    // Header sanity
    assert_eq!(&bytes[36..40], b"acsp");

    // Reopen in Rust
    let mut icc2 = profile::open_profile_from_mem(None, &bytes).expect("reopen");
    assert!(profile::cms_read_tag(&mut icc2, signatures::tag::SigCopyrightTag).is_some());
    assert!(profile::cms_read_tag(&mut icc2, signatures::tag::SigMediaWhitePointTag).is_some());

    // Reopen in C — must parse + cook identically
    let mut b2 = bytes.clone();
    let cio = unsafe {
        c::cmsOpenIOhandlerFromMem(
            ptr::null_mut(),
            b2.as_mut_ptr(),
            b2.len() as u32,
            b"r\0".as_ptr(),
        )
    };
    let c_prof = unsafe { c::cmsOpenProfileFromIOhandlerTHR(ptr::null_mut(), cio) };
    assert!(!c_prof.is_null(), "C failed to open Rust-written profile");
    let c_n = unsafe { c::cmsGetTagCount(c_prof) };
    assert_eq!(c_n, 2);
    for i in 0..c_n {
        let sig = unsafe { c::cmsGetTagSignature(c_prof, i as u32) };
        let obj = unsafe { c::cmsReadTag(c_prof, sig) };
        assert!(
            !obj.is_null(),
            "C failed to read tag {sig:#x} from Rust profile"
        );
    }
    unsafe { c::cmsCloseProfile(c_prof) };
}
