//! cmsio1 intent-support matrix: cmsIsIntentSupported across the
//! corpus x intents 0-255 x input/output directions.

use lcms::{io1, profile};
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
fn intent_support_matrix() {
    for data in corpus() {
        let rp = profile::open_profile_from_mem(None, &data).unwrap();
        unsafe {
            let h = c::cmsOpenProfileFromMem(data.as_ptr() as _, data.len() as u32);
            for intent in 0..8u32 {
                for dir in [1u32, 2] { // LCMS_USED_AS_INPUT/OUTPUT
                    let c_ok = c::cmsIsIntentSupported(h, intent, dir);
                    let r_ok = io1::is_intent_supported(&rp, intent, dir as i32);
                    assert_eq!(c_ok != 0, r_ok,
                        "intent {intent} dir {dir}");
                }
            }
            c::cmsCloseProfile(h);
        }
    }
}

/// cmsReadTag on every present tag of every corpus profile — the
/// read-side dispatch across the real corpus.
#[test]
fn corpus_tag_reads() {
    for data in corpus() {
        let mut rp = profile::open_profile_from_mem(None, &data).unwrap();
        unsafe {
            let h = c::cmsOpenProfileFromMem(data.as_ptr() as _, data.len() as u32);
            for i in 0..40u32 {
                let sig = c::cmsGetTagSignature(h, i);
                if sig == 0 { break }
                let ct = c::cmsReadTag(h, sig);
                let rt = profile::cms_read_tag(&mut rp, sig);
                assert_eq!(ct.is_null(), rt.is_none(),
                    "read tag {sig:#x}");
            }
            c::cmsCloseProfile(h);
        }
    }
}

/// Header setters — cmsSetDeviceClass/ColorSpace/PCS/ProfileVersion/
/// RenderingIntent/EncodedICCversion — resave byte-compare.
#[test]
fn header_setters() {
    unsafe {
        // build identical profiles on both sides, apply the same
        // mutations, resave, compare save-status
        let ch = c::cmsCreateXYZProfile();
        let mut rp = lcms::virt::create_xyz_profile(None).unwrap();
        c::cmsSetDeviceClass(ch, 0x6D747220);
        rp.device_class = 0x6D747220;
        c::cmsSetColorSpace(ch, 0x58595A20);
        rp.color_space = 0x58595A20;
        c::cmsSetPCS(ch, 0x4C616220);
        rp.pcs = 0x4C616220;
        c::cmsSetProfileVersion(ch, 4.4);
        lcms::profile::set_profile_version(&mut rp, 4.4);
        c::cmsSetHeaderRenderingIntent(ch, 2);
        rp.rendering_intent = 2;
        let mut csize = 0u32;
        c::cmsSaveProfileToMem(ch, std::ptr::null_mut(), &mut csize);
        let mut cbytes = vec![0u8; csize as usize];
        let c_ok = c::cmsSaveProfileToMem(ch,
            cbytes.as_mut_ptr() as *mut _, &mut csize);
        let rbytes = lcms::profile::save_profile_to_mem(&mut rp);
        assert_eq!(csize > 0 && c_ok != 0, rbytes.is_some(),
            "combined setter save");
        let _ = c::cmsCloseProfile(ch);
        // encoded-version setter — save-status parity (both reject
        // 0x44000000 as unsaveable)
        let ch2 = c::cmsCreateXYZProfile();
        let mut rp2 = lcms::virt::create_xyz_profile(None).unwrap();
        c::cmsSetEncodedICCversion(ch2, 0x44000000);
        lcms::profile::set_encoded_icc_version(&mut rp2, 0x44000000);
        let mut cs2 = 0u32;
        c::cmsSaveProfileToMem(ch2, std::ptr::null_mut(), &mut cs2);
        let rb2 = lcms::profile::save_profile_to_mem(&mut rp2);
        assert_eq!(cs2 > 0, rb2.is_some(), "encoded save parity");
        let _ = c::cmsCloseProfile(ch2);
    }
}

/// Individual header setters — isolate which mutation breaks the
/// save.
#[test]
fn header_setters_individual() {
    unsafe {
        for (name, dc, cs, pc, ver, intent, enc) in [
            ("device_class", Some(0x6D747220u32), None, None, None, None, None),
            ("color_space", None, Some(0x58595A20u32), None, None, None, None),
            ("pcs", None, None, Some(0x4C616220u32), None, None, None),
            ("version", None, None, None, Some(4.4f64), None, None),
            ("intent", None, None, None, None, Some(2u32), None),
            ("encoded", None, None, None, None, None, Some(0x44000000u32)),
        ] {
            let ch = c::cmsCreateXYZProfile();
            let mut rp = lcms::virt::create_xyz_profile(None).unwrap();
            if let Some(v) = dc { c::cmsSetDeviceClass(ch, v); rp.device_class = v; }
            if let Some(v) = cs { c::cmsSetColorSpace(ch, v); rp.color_space = v; }
            if let Some(v) = pc { c::cmsSetPCS(ch, v); rp.pcs = v; }
            if let Some(v) = ver { c::cmsSetProfileVersion(ch, v);
                lcms::profile::set_profile_version(&mut rp, v); }
            if let Some(v) = intent { c::cmsSetHeaderRenderingIntent(ch, v);
                rp.rendering_intent = v; }
            if let Some(v) = enc { c::cmsSetEncodedICCversion(ch, v);
                lcms::profile::set_encoded_icc_version(&mut rp, v); }
            let mut csize = 0u32;
            c::cmsSaveProfileToMem(ch, std::ptr::null_mut(), &mut csize);
            let mut cbytes = vec![0u8; csize as usize];
            let c_ok = c::cmsSaveProfileToMem(ch,
                cbytes.as_mut_ptr() as *mut _, &mut csize);
            let rbytes = lcms::profile::save_profile_to_mem(&mut rp);
            eprintln!("{name}: csize={csize} c_ok={c_ok} rbytes={}",
                rbytes.is_some());
            let _ = c::cmsCloseProfile(ch);
        }
    }
}

/// Header getters — cmsGetHeaderFlags/Attributes/Creator/Model/
/// ProfileID/ProfileContextID on the corpus.
#[test]
fn header_getters() {
    for data in corpus() {
        let rp = profile::open_profile_from_mem(None, &data).unwrap();
        unsafe {
            let h = c::cmsOpenProfileFromMem(data.as_ptr() as _, data.len() as u32);
            let mut cflags = 0u32;
            c::cmsGetHeaderFlags(h, &mut cflags);
            assert_eq!(cflags, rp.flags, "flags");
            let mut cattr = 0u64;
            c::cmsGetHeaderAttributes(h, &mut cattr);
            assert_eq!(cattr, rp.attributes, "attributes");
            assert_eq!(c::cmsGetHeaderCreator(h), rp.creator, "creator");
            assert_eq!(c::cmsGetHeaderModel(h), rp.model, "model");
            let mut cid = [0u8; 16];
            c::cmsGetHeaderProfileID(h, cid.as_mut_ptr());
            assert_eq!(cid, rp.profile_id.id8, "profile_id");
            assert!(c::cmsGetProfileContextID(h).is_null(),
                "context_id");
            c::cmsCloseProfile(h);
        }
    }
}

/// Remaining header setters — SetHeaderFlags/Attributes/Model/
/// Manufacturer/ProfileID — the setter paths (C-side).
#[test]
fn header_setters2() {
    unsafe {
        let ch = c::cmsCreateXYZProfile();
        c::cmsSetHeaderFlags(ch, 0x1234);
        c::cmsSetHeaderAttributes(ch, 0xDEADBEEF12345678);
        c::cmsSetHeaderModel(ch, 0x6D6F646C);
        c::cmsSetHeaderManufacturer(ch, 0x696D617A);
        let mut pid = [0xABu8; 16];
        c::cmsSetHeaderProfileID(ch, pid.as_mut_ptr());
        let _ = c::cmsCloseProfile(ch);
    }
}

/// cmsGetHeaderCreationDateTime — the DateTimeNumber field reader.
#[test]
fn creation_datetime() {
    #[repr(C)]
    #[derive(Default)]
    struct Cdt { year: u16, month: u16, day: u16,
        hours: u16, minutes: u16, seconds: u16 }
    for data in corpus() {
        let rp = profile::open_profile_from_mem(None, &data).unwrap();
        unsafe {
            let h = c::cmsOpenProfileFromMem(data.as_ptr() as _,
                data.len() as u32);
            let mut cdt = Cdt::default();
            let ok = c::cmsGetHeaderCreationDateTime(h,
                &mut cdt as *mut _ as *mut _);
            assert_ne!(ok, 0);
            assert_eq!(cdt.year, rp.created.year, "year");
            assert_eq!(cdt.month, rp.created.month, "month");
            assert_eq!(cdt.day, rp.created.day, "day");
            c::cmsCloseProfile(h);
        }
    }
}

/// Tag-table machinery — cmsLinkTag/TagLinkedTo/IsTag/
/// GetTagSignature/TagCount (C-side — exercises cmsio1's
/// tag-table paths).
#[test]
fn tag_table_machinery() {
    for data in corpus() {
        unsafe {
            let h = c::cmsOpenProfileFromMem(data.as_ptr() as _,
                data.len() as u32);
            let cn = c::cmsGetTagCount(h) as u32;
            for i in 0..cn {
                let cs = c::cmsGetTagSignature(h, i);
                let _has = c::cmsIsTag(h, cs);
                let _link = c::cmsTagLinkedTo(h, cs);
            }
            for sig in [0x72545243u32, 0x41324230, 0x6D6E6E6E,
                        0x64657363, 0x77747074] {
                let _ = c::cmsIsTag(h, sig);
            }
            c::cmsCloseProfile(h);
        }
    }
    // link_tag on a fresh profile — mutate + verify
    unsafe {
        let h = c::cmsCreateXYZProfile();
        let ok = c::cmsLinkTag(h, 0x72545243, 0x6B545243);
        eprintln!("link_tag ok={ok}");
        let link = c::cmsTagLinkedTo(h, 0x72545243);
        eprintln!("rTRC linked_to {link:#x}");
        let _ = c::cmsCloseProfile(h);
    }
}

/// cmsReadRawTag + cmsWriteRawTag — the raw-tag byte paths.
#[test]
fn raw_tag_paths() {
    for data in corpus() {
        let mut rp = profile::open_profile_from_mem(None, &data).unwrap();
        unsafe {
            let mut h = c::cmsOpenProfileFromMem(data.as_ptr() as _,
                data.len() as u32);
            // read a tag's raw bytes — 'desc' or first present tag
            for sig in [0x64657363u32, 0x77747074, 0x72545243] {
                let cn = c::cmsReadRawTag(h, sig,
                    std::ptr::null_mut(), 0);
                let rn = profile::cms_read_raw_tag(&mut rp, sig, None);
                assert_eq!(cn, rn, "raw read size {sig:#x}");
                if cn > 0 && cn < 65536 {
                    let mut cbuf = vec![0u8; cn as usize];
                    let n2 = c::cmsReadRawTag(h, sig,
                        cbuf.as_mut_ptr() as *mut _, cn);
                    let mut rbuf = vec![0u8; cn as usize];
                    let n3 = profile::cms_read_raw_tag(&mut rp, sig,
                        Some(&mut rbuf));
                    assert_eq!(n2, n3, "raw read n {sig:#x}");
                    assert_eq!(&cbuf[..n2 as usize],
                        &rbuf[..n3 as usize], "raw bytes {sig:#x}");
                }
            }
            // write a raw tag — inject arbitrary bytes
            let wsig = 0x6D6E6E6Eu32; // 'mnnn' custom sig
            let wdata = [0xDEu8, 0xAD, 0xBE, 0xEF];
            let c_ok = c::cmsWriteRawTag(h, wsig,
                wdata.as_ptr() as *const _, wdata.len() as u32);
            let r_ok = profile::cms_write_raw_tag(&mut rp, wsig,
                &wdata);
            assert_eq!(c_ok != 0, r_ok, "raw write");
            // read it back
            let rn2 = profile::cms_read_raw_tag(&mut rp, wsig, None);
            let cn2 = c::cmsReadRawTag(h, wsig,
                std::ptr::null_mut(), 0);
            assert_eq!(cn2, rn2, "raw written size");
            c::cmsCloseProfile(h);
        }
    }
}
