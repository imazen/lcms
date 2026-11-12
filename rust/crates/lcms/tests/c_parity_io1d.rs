//! cmsio1 remaining getters: version/context/matrix-shaper parity
//! across the corpus + builder profiles.

use lcms::profile;
use lcms_cref as c;

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
fn version_getters() {
    for data in corpus() {
        let rp = profile::open_profile_from_mem(None, &data).unwrap();
        unsafe {
            let h = c::cmsOpenProfileFromMem(data.as_ptr() as _, data.len() as u32);
            assert_eq!(
                c::cmsGetProfileVersion(h),
                profile::get_profile_version(&rp),
                "version"
            );
            assert_eq!(
                c::cmsGetEncodedICCversion(h),
                profile::get_encoded_icc_version(&rp),
                "encoded version"
            );
            // set-version round-trip parity
            c::cmsSetProfileVersion(h, 2.3);
            let mut rp2 = rp.clone();
            profile::set_profile_version(&mut rp2, 2.3);
            assert_eq!(
                c::cmsGetProfileVersion(h),
                profile::get_profile_version(&rp2),
                "set version"
            );
            c::cmsCloseProfile(h);
        }
    }
}

#[test]
fn context_id() {
    // cmsGetProfileContextID returns the profile's context — NULL for
    // non-THR opens
    unsafe {
        let h = c::cmsCreateXYZProfile();
        assert!(c::cmsGetProfileContextID(h).is_null(), "NULL ctx id");
        c::cmsCloseProfile(h);
    }
}
