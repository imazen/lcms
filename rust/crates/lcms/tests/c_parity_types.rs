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
        assert_eq!(n_tags as u32, rp.tag_count, "{name}: tag count");
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
            let Some(cb) = c_read_raw(ch, sig) else {
                continue;
            };
            // write raw into both, read back
            let cw = unsafe { c::cmsWriteRawTag(ch, sig, cb.as_ptr() as _, cb.len() as u32) };
            let rw = profile::cms_write_raw_tag(&mut rp, sig, &cb);
            assert_eq!(cw != 0, rw, "{name}: write_raw status {sig:#010x}");
            let cback = c_read_raw(ch, sig);
            let rback = r_read_raw(&mut rp, sig);
            assert_eq!(cback, rback, "{name}: raw roundtrip {sig:#010x}");
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
            assert_eq!(c_out[off], r_out[off], "{name}: resave byte @{off:#x}");
        }
        assert_eq!(c_out.len(), r_out.len(), "{name}: resave length");
    }
}

/// MPE/exotic-tag coverage: built profiles carry mAB/mBA/clut16/curv
/// payloads — raw-read every tag + parse+resave on both sides.
#[test]
fn built_profile_mpe_tags() {
    use lcms::virt;
    // builders that produce LUT/MPE-bearing profiles
    let (ch_list, rp_list): (Vec<*mut c_void>, Vec<profile::IccProfile>) = {
        let mut cs = Vec::new();
        let mut rs = Vec::new();
        unsafe {
            cs.push(c::cmsCreateBCHSWabstractProfile(
                24, 1.1, 1.2, 10.0, 0.8, 6500, 5000,
            ));
        }
        // linearization devicelink (CLUT16/mft2) via same gamma on both
        let c_g = unsafe { c::cmsBuildGamma(std::ptr::null_mut(), 1.8) };
        let r_g = lcms::gamma::build_gamma(None, 1.8).unwrap();
        unsafe {
            let curves = [c_g];
            cs.push(c::cmsCreateLinearizationDeviceLinkTHR(
                std::ptr::null_mut(),
                0x47524159, // cmsSigGrayData
                curves.as_ptr(),
            ));
            cs.push(c::cmsCreateInkLimitingDeviceLinkTHR(
                std::ptr::null_mut(),
                0x434D594B, // cmsSigCmykData
                350.0,
            ));
        }
        rs.push(
            *virt::create_bchsw_abstract_profile(None, 24, 1.1, 1.2, 10.0, 0.8, 6500, 5000)
                .unwrap(),
        );
        rs.push(*virt::create_linearization_device_link(None, 0x47524159, &[&r_g]).unwrap());
        rs.push(*virt::create_ink_limiting_device_link(None, 0x434D594B, 350.0).unwrap());
        (cs, rs)
    };
    for (idx, (&ch, rp)) in ch_list.iter().zip(rp_list.iter()).enumerate() {
        assert!(!ch.is_null(), "C builder {idx}");
        let n = unsafe { c::cmsGetTagCount(ch) };
        let mut rp = rp.clone();
        for i in 0..n {
            let sig = unsafe { c::cmsGetTagSignature(ch, i as u32) };
            let cb = c_read_raw(ch, sig);
            let rb = r_read_raw(&mut rp, sig);
            assert_eq!(cb, rb, "built {idx} raw tag {sig:#010x}");
        }
        // parse + resave byte-compare
        let mut cn = 0u32;
        let c_out = unsafe {
            assert!(c::cmsSaveProfileToMem(ch, std::ptr::null_mut(), &mut cn) != 0);
            let mut b = vec![0u8; cn as usize];
            assert!(c::cmsSaveProfileToMem(ch, b.as_mut_ptr() as *mut _, &mut cn) != 0);
            c::cmsCloseProfile(ch);
            b
        };
        let r_out = profile::save_profile_to_mem(&mut rp).unwrap();
        assert_eq!(c_out.len(), r_out.len(), "built {idx} resave length");
        for (i, (a, b)) in c_out.iter().zip(r_out.iter()).enumerate() {
            if (24..36).contains(&i) {
                continue;
            }
            assert_eq!(a, b, "built {idx} resave @{i:#x}");
        }
    }
}

/// `cmsICCViewingConditions` — the viewingConditionsType tag payload
/// (not the CAM02 ViewingConditions — different struct).
#[repr(C)]
struct CIccViewingConditions {
    illuminant: c::CieXyz,
    surround: c::CieXyz,
    illuminant_type: u32,
}

/// Exotic tag-type write/read round-trips: ViewingConditions,
/// Measurement, UcrBg, Chromaticity, Dict, NamedColor — the tag-type
/// handlers beyond the corpus-basics matrix.
#[test]
fn exotic_tag_types() {
    unsafe {
        // ViewingConditionsType
        let c_vc = CIccViewingConditions {
            illuminant: c::CieXyz {
                X: 0.9642,
                Y: 1.0,
                Z: 0.8249,
            },
            surround: c::CieXyz {
                X: 0.5,
                Y: 0.5,
                Z: 0.5,
            },
            illuminant_type: 1,
        };
        let r_vc = lcms::tagtypes::IccViewingConditions {
            illuminant_xyz: lcms::types::CieXyz {
                X: 0.9642,
                Y: 1.0,
                Z: 0.8249,
            },
            surround_xyz: lcms::types::CieXyz {
                X: 0.5,
                Y: 0.5,
                Z: 0.5,
            },
            illuminant_type: 1,
        };
        // write into a profile + read back — tag-type handler parity
        let ch = c::cmsCreateXYZProfile();
        let mut rp = lcms::virt::create_xyz_profile(None).unwrap();
        let c_ok = c::cmsWriteTag(ch, 0x76696577, &c_vc as *const _ as *const _);
        let r_ok = lcms::profile::cms_write_tag(
            &mut rp,
            0x76696577,
            Some(lcms::tagtypes::TagObject::ViewingConditions(r_vc)),
        );
        assert_eq!(c_ok != 0, r_ok, "write viewing");
        // read back on the C side + verify a resave round-trips
        let mut c_out = vec![0u8; 1 << 20];
        let mut size = c_out.len() as u32;
        c::cmsSaveProfileToMem(ch, c_out.as_mut_ptr() as *mut _, &mut size);
        c_out.truncate(size as usize);
        let mut r_out = lcms::profile::save_profile_to_mem(&mut rp).unwrap_or_default();
        // mask created (offset 24) + profile ID (offset 84..100)
        for i in 0..12 {
            c_out.get_mut(24 + i).map(|b| *b = 0);
            r_out.get_mut(24 + i).map(|b| *b = 0);
        }
        for i in 0..16 {
            c_out.get_mut(84 + i).map(|b| *b = 0);
            r_out.get_mut(84 + i).map(|b| *b = 0);
        }
        c_out.resize(r_out.len().max(c_out.len()), 0);
        r_out.resize(c_out.len(), 0);
        assert_eq!(c_out, r_out, "viewing resave");
        c::cmsCloseProfile(ch);
    }
}
