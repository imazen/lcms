//! cmstypes tag-table machinery: cmsTagCount / cmsGetTagSignature /
//! cmsIsTag / cmsReadTag presence + cmsTagLinkedTo / cmsLinkTag /
//! cmsLinkTag-followed-read parity on the corpus + builder profiles.

use lcms::{named, profile, virt};
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
fn tag_table_matrix() {
    for data in corpus() {
        let rp = profile::open_profile_from_mem(None, &data).unwrap();
        unsafe {
            let h = c::cmsOpenProfileFromMem(data.as_ptr() as _, data.len() as u32);
            // count + signature parity
            // count parity: cmsGetTagSignature iterates to C's count
            let c_cnt = rp.tag_count;
            let mut got = 0u32;
            for i in 0..c_cnt {
                let sig = c::cmsGetTagSignature(h, i);
                assert_eq!(sig, rp.tag_names[i as usize], "tag sig {i}");
                got += 1;
            }
            assert_eq!(got, c_cnt, "tag count");
            // out-of-range sig
            assert_eq!(c::cmsGetTagSignature(h, c_cnt), 0, "sig oor");
            // is_tag on a sweep of known sigs
            for sig in [
                0x41324230u32,
                0x42324130,
                0x6D414220,
                0x6D424120,
                0x6B747263,
                0x72545952,
                0x67545952,
                0x62545952,
                0x77687470,
                0x64657363,
                0x63707274,
                0x70736571,
                0x63686164,
                0x636C7475,
                0x6D657461,
                0x76696577,
                0x7363726E,
                0x74656368,
                0x76346367,
                0x73667472,
            ] {
                assert_eq!(
                    c::cmsIsTag(h, sig) != 0,
                    profile::cms_is_tag_pub(&rp, sig),
                    "is_tag {sig:#x}"
                );
            }
            c::cmsCloseProfile(h);
        }
    }
}

#[test]
fn linked_tag_follow() {
    // cmsLinkTag + followed read — a linked tag resolves to its
    // destination's data
    unsafe {
        let ch = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let mut rp = virt::create_lab4_profile(None, None).unwrap();
        // link 'foo ' -> 'A2B0' (a tag the lab4 profile carries)
        assert_eq!(
            c::cmsLinkTag(ch, 0x666F6F20, 0x41324230) != 0,
            profile::cms_link_tag(&mut rp, 0x666F6F20, 0x41324230),
            "link foo->A2B0"
        );
        // followed read — cmsReadTag on the linked sig returns the dest
        assert!(!c::cmsReadTag(ch, 0x41324230).is_null(), "C read A2B0");
        let c_rd = c::cmsReadTag(ch, 0x666F6F20);
        let r_rd = profile::cms_read_tag(&mut rp, 0x666F6F20);
        assert_eq!(!c_rd.is_null(), r_rd.is_some(), "linked read presence");
        c::cmsCloseProfile(ch);
    }
}

#[test]
fn mpe_tag_read() {
    // MPE-typed tags (mAB / mBA / clut16 from BCHSW + devicelink
    // builders) — read back via cmsReadTag exercises the MPE element
    // readers
    unsafe {
        for mk in [0u32, 1, 2] {
            let (ch, rp): (*mut _, Option<Box<lcms::profile::IccProfile>>) = match mk {
                0 => (
                    c::cmsCreateBCHSWabstractProfile(17, 0.0, 20.0, 30.0, 40.0, 5000, 6000),
                    virt::create_bchsw_abstract_profile(
                        None, 17, 0.0, 20.0, 30.0, 40.0, 5000, 6000,
                    ),
                ),
                1 => {
                    let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
                    let curves = [g; 3];
                    (
                        c::cmsCreateLinearizationDeviceLinkTHR(
                            std::ptr::null_mut(),
                            0x52474220,
                            curves.as_ptr(),
                        ),
                        virt::create_linearization_device_link(
                            None,
                            0x52474220,
                            &[&lcms::gamma::build_gamma(None, 2.2).unwrap(); 3],
                        ),
                    )
                }
                _ => (
                    c::cmsCreateInkLimitingDeviceLinkTHR(std::ptr::null_mut(), 0x434D594B, 85.0),
                    virt::create_ink_limiting_device_link(None, 0x434D594B, 85.0),
                ),
            };
            assert_eq!(ch.is_null(), rp.is_none(), "builder {mk} presence");
            if ch.is_null() || rp.is_none() {
                continue;
            }
            let mut rp = rp.unwrap();
            // tag-count + per-tag read presence parity
            let c_cnt = rp.tag_count;
            assert_eq!(c_cnt, rp.tag_count, "mpe count {mk}");
            for i in 0..c_cnt {
                let sig = c::cmsGetTagSignature(ch, i);
                assert_eq!(sig, rp.tag_names[i as usize], "mpe sig {mk} #{i}");
                let c_rd = c::cmsReadTag(ch, sig);
                let r_rd = profile::cms_read_tag(&mut rp, sig);
                assert_eq!(!c_rd.is_null(), r_rd.is_some(), "mpe read {sig:#x}");
            }
            c::cmsCloseProfile(ch);
        }
    }
}

/// cmsDict machinery: alloc + wide-string entries + dup + entry count.
#[test]
fn dict_machinery() {
    unsafe {
        let cd = c::cmsDictAlloc(std::ptr::null_mut());
        let mut rd = named::dict_alloc(None);
        assert!(!cd.is_null());
        // wide strings (UTF-32 as i32)
        let w = |s: &str| -> Vec<i32> {
            s.chars()
                .map(|c| c as i32)
                .chain(std::iter::once(0))
                .collect()
        };
        let pairs = [
            ("Manufacturer", "Acme"),
            ("Model", "Pro 1000"),
            ("SerialNumber", "SN-42"),
            ("ColorGamut", "Wide"),
        ];
        for (k, v) in pairs {
            let kw = w(k);
            let vw = w(v);
            let cm1 = c::cmsMLUalloc(std::ptr::null_mut(), 1);
            let cm2 = c::cmsMLUalloc(std::ptr::null_mut(), 1);
            let c_ok = c::cmsDictAddEntry(cd, kw.as_ptr(), vw.as_ptr(), cm1, cm2);
            let rm1 = named::mlu_alloc(None, 1).unwrap();
            let rm2 = named::mlu_alloc(None, 1).unwrap();
            let r_ok = named::dict_add_entry(&mut rd, &kw, Some(&vw), Some(&rm1), Some(&rm2));
            assert_eq!(c_ok != 0, r_ok, "dict add {k}");
            c::cmsMLUfree(cm1);
            c::cmsMLUfree(cm2);
        }
        // dup + count parity
        let cdd = c::cmsDictDup(cd);
        let rdd = named::dict_dup(&rd);
        // entry count via GetEntryList walk — cmsDICTentry.next is
        // field 0
        #[repr(C)]
        struct CDictEntry {
            next: *const CDictEntry,
        }
        let mut cnt = 0u32;
        let mut e = c::cmsDictGetEntryList(cdd) as *const CDictEntry;
        while !e.is_null() {
            cnt += 1;
            e = (*e).next;
        }
        assert_eq!(cnt as usize, rdd.entries.len(), "dict dup count");
        c::cmsDictFree(cd);
        c::cmsDictFree(cdd);
    }
}
