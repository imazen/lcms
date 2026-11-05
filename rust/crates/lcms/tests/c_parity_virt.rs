//! Differential gate for cmsvirt.c — C and Rust build the same virtual
//! profiles; byte-compare the serialized output.

use lcms::profile;
use lcms::virt;
use lcms_cref as c;
use std::ptr;

fn c_save(p: *mut std::ffi::c_void) -> Vec<u8> {
    unsafe {
        let mut needed = 0u32;
        assert!(c::cmsSaveProfileToMem(p, ptr::null_mut(), &mut needed) != 0);
        let mut buf = vec![0u8; needed as usize];
        assert!(c::cmsSaveProfileToMem(p, buf.as_mut_ptr() as *mut _, &mut needed) != 0);
        buf.truncate(needed as usize);
        buf
    }
}

fn r_save(icc: &mut lcms::profile::IccProfile) -> Vec<u8> {
    profile::save_profile_to_mem(icc).expect("rust save")
}

#[test]
fn srgb_profile_bytes() {
    let c_prof = unsafe { c::cmsCreate_sRGBProfile() };
    assert!(!c_prof.is_null());
    let c_bytes = c_save(c_prof);
    unsafe { c::cmsCloseProfile(c_prof) };

    let mut r_prof = virt::create_srgb_profile(None).expect("rust srgb");
    let r_bytes = r_save(&mut r_prof);

    assert_eq!(r_bytes.len(), c_bytes.len(), "size srgb");
    let (mut rb, mut cb) = (r_bytes.clone(), c_bytes.clone());
    rb[24..36].fill(0); // Created = time() in C, epoch in port
    cb[24..36].fill(0);
    assert_eq!(rb, cb, "sRGB profile bytes differ");
}

#[test]
fn lab2_lab4_xyz_profiles() {
    let c_lab2 = unsafe { c::cmsCreateLab2Profile(ptr::null()) };
    let c_lab4 = unsafe { c::cmsCreateLab4Profile(ptr::null()) };
    let c_xyz = unsafe { c::cmsCreateXYZProfile() };
    assert!(!c_lab2.is_null() && !c_lab4.is_null() && !c_xyz.is_null());
    let b_lab2 = c_save(c_lab2);
    let b_lab4 = c_save(c_lab4);
    let b_xyz = c_save(c_xyz);
    unsafe {
        c::cmsCloseProfile(c_lab2);
        c::cmsCloseProfile(c_lab4);
        c::cmsCloseProfile(c_xyz);
    }

    let mut r_lab2 = virt::create_lab2_profile(None, None).unwrap();
    let mut r_lab4 = virt::create_lab4_profile(None, None).unwrap();
    let mut r_xyz = virt::create_xyz_profile(None).unwrap();
    for (name, mut rb, mut cb) in [
        ("lab2", r_save(&mut r_lab2), b_lab2),
        ("lab4", r_save(&mut r_lab4), b_lab4),
        ("xyz", r_save(&mut r_xyz), b_xyz),
    ] {
        rb[24..36].fill(0);
        cb[24..36].fill(0);
        assert_eq!(rb, cb, "{name} bytes");
    }
}
