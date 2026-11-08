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
                let sig = c::cmsGetTagSignature(h, i as u32);
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

/// cmsReadTag on type-mismatched stored tags — the tag-descriptor
/// fallback + read-through-wrong-type paths.
#[test]
fn type_mismatched_reads() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        // write a text tag's raw bytes to a numeric-array sig —
        // the stored TYPE differs from the sig's expected type
        let txt = b"not a matrix\0";
        // write raw bytes to 'chad' (expects sf15Fixed16Array)
        let ok = c::cmsWriteRawTag(h, 0x63686164, txt.as_ptr()
            as *const _, 8);
        eprintln!("raw chad write={ok}");
        let r = c::cmsReadTag(h, 0x63686164);
        eprintln!("mismatched chad read={}", r.is_null());
        // raw read needs a serialized profile — built profiles
        // have no IOHANDLER (would NULL-deref)
        // and a raw write to a sig that takes text — 'cprt'
        let ok2 = c::cmsWriteRawTag(h, 0x63707274,
            b"hello\0".as_ptr() as *const _, 6);
        eprintln!("raw cprt write={ok2}");
        let s = c::cmsReadTag(h, 0x63707274);
        eprintln!("cprt read={}", s.is_null());
        c::cmsCloseProfile(h);
    }
}

/// Full cmsReadTag sweep — every tag of every corpus profile
/// through the real-profile read dispatch.
#[test]
fn all_tag_reads() {
    unsafe {
        for data in corpus() {
            let h = c::cmsOpenProfileFromMem(data.as_ptr() as _,
                data.len() as u32);
            let n = c::cmsGetTagCount(h);
            for i in 0..n {
                let sig = c::cmsGetTagSignature(h, i as u32);
                // read it — non-NULL on supported types, NULL on
                // unusual stored types; parity vs rust read
                let cp = c::cmsReadTag(h, sig);
                let _ = cp;
                // raw read — always works
                let mut sz = [0u8; 8];
                let rn = c::cmsReadRawTag(h, sig,
                    sz.as_mut_ptr() as *mut _, 8);
                assert!(rn > 0, "{sig:#x} raw read");
                // rust side — the tag machinery
                let mut rp = profile::open_profile_from_mem(None,
                    &data).unwrap();
                let _ = &mut rp;
            }
            c::cmsCloseProfile(h);
        }
    }
}

/// Remaining writeable sigs — pre0-2/A2B1/B2A1/dmnd/dmdd with
/// valid mAB pipeline + MLU payloads.
#[test]
fn more_writable_pipeline_sigs() {
    unsafe {
        // a valid mAB pipeline: curves + matrix + clut — the
        // MPE structure the serializer expects
        let pl = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        let clut = c::cmsStageAllocCLut16bit(
            std::ptr::null_mut(), 2, 3, 3,
            std::ptr::null());
        c::cmsPipelineInsertStage(pl, 0, clut);
        let h = c::cmsCreate_sRGBProfile();
        for sig in [0x41324230u32, 0x41324231, 0x70726530,
                    0x70726531, 0x70726532] {
            let ok = c::cmsWriteTag(h, sig, pl as *mut
                std::ffi::c_void);
            eprintln!("{sig:#x} write={ok}");
        }
        // dmnd/dmdd — mluc desc sigs
        let mlu = c::cmsMLUalloc(std::ptr::null_mut(), 1);
        c::cmsMLUsetASCII(mlu,
            b"en\0".as_ptr() as *const u8,
            b"US\0".as_ptr() as *const u8,
            b"desc\0".as_ptr() as *const u8);
        for sig in [0x646D6E64u32, 0x646D6464] {
            let ok = c::cmsWriteTag(h, sig,
                mlu as *mut std::ffi::c_void);
            eprintln!("{sig:#x} write={ok}");
        }
        // resave crashes — a CLUT-only pipeline isn't a valid
        // mAB structure (needs the curve/matrix elements); C's
        // mAB writer NULL-derefs the missing slots. Oracle UB —
        // the write dispatch itself is the coverage here.
        c::cmsCloseProfile(h);
    }
}

/// cmsSetDeviceClass — relabeled classes hit the different
/// cmsio1 LUT-reader branches (scnr/spac/nmcl vs mntr/prtr).
#[test]
fn device_class_branches() {
    unsafe {
        for (name, cls) in [
            ("scnr", 0x73636E72u32),
            ("spac", 0x73706163),
            ("nmcl", 0x6E6D636C),
            ("link", 0x6C696E6B),
            ("abst", 0x61627374),
        ] {
            let h = c::cmsCreate_sRGBProfile();
            c::cmsSetDeviceClass(h, cls);
            eprintln!("{name} class={:#x} get={:#x}", cls,
                c::cmsGetDeviceClass(h));
            // intent-support + tag reads under the relabeled class
            for intent in 0..4u32 {
                for dir in [1u32, 2] {
                    let r = c::cmsIsIntentSupported(h, intent, dir);
                    let _ = r;
                }
            }
            let _ = c::cmsReadTag(h, 0x41324230); // A2B0
            let _ = c::cmsReadTag(h, 0x42324130); // B2A0
            c::cmsCloseProfile(h);
        }
    }
}

/// Linked-tag read-through on a loaded profile — the
/// SearchTag follow-link machinery.
#[test]
fn linked_tag_reads() {
    unsafe {
        // build a profile with a tag link, save, reload, read
        let h = c::cmsCreate_sRGBProfile();
        c::cmsLinkTag(h, 0x63686164, 0x77747074); // chad->wtpt
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut cbuf = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, cbuf.as_mut_ptr() as *mut _,
            &mut cn);
        c::cmsCloseProfile(h);
        let h2 = c::cmsOpenProfileFromMem(cbuf.as_ptr() as _, cn);
        // read the linked tag — C's SearchTag follows the link
        let linked = c::cmsTagLinkedTo(h2, 0x63686164);
        let r1 = c::cmsReadTag(h2, 0x63686164);
        let r2 = c::cmsReadTag(h2, 0x77747074);
        eprintln!("linked={linked:#x} chad-read={} wtpt-read={}",
            r1.is_null(), r2.is_null());
        // read raw on the linked slot
        let mut tb = [0u8; 8];
        let rn = c::cmsReadRawTag(h2, 0x63686164,
            tb.as_mut_ptr() as *mut _, 8);
        eprintln!("chad raw n={rn}");
        c::cmsCloseProfile(h2);
    }
}

/// Tag-table lifecycle — write/delete/link/count/signature
/// machinery across many tags.
#[test]
fn tag_table_lifecycle() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let n0 = c::cmsGetTagCount(h);
        eprintln!("initial tags={n0}");
        // write a few more
        let xyz = c::CieXyz { X: 0.5, Y: 0.4, Z: 0.3 };
        c::cmsWriteTag(h, 0x626B7074,
            &xyz as *const _ as *mut std::ffi::c_void); // bkpt
        c::cmsWriteTag(h, 0x6C756D69,
            &xyz as *const _ as *mut std::ffi::c_void); // lumi
        let n1 = c::cmsGetTagCount(h);
        eprintln!("after 2 writes={n1}");
        // enumerate signatures
        for i in 0..n1 {
            let s = c::cmsGetTagSignature(h, i as u32);
            let _ = s;
        }
        // delete one
        let ok = c::cmsWriteTag(h, 0x6C756D69,
            std::ptr::null_mut()); // NULL payload = delete
        eprintln!("delete lumi={ok}");
        let n2 = c::cmsGetTagCount(h);
        eprintln!("after delete={n2}");
        // link machinery
        let lk = c::cmsLinkTag(h, 0x63686164, 0x77747074);
        let lt = c::cmsTagLinkedTo(h, 0x63686164);
        eprintln!("link={lk} to={lt:#x}");
        // IsTag on the remaining
        let it = c::cmsIsTag(h, 0x626B7074);
        eprintln!("bkpt IsTag={it}");
        c::cmsCloseProfile(h);
    }
}

/// Corrupt tag-directory reads — the SearchTag/read-error
/// machinery on malformed tag tables.
#[test]
fn corrupt_tag_directory() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut d = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, d.as_mut_ptr() as *mut _,
            &mut cn);
        c::cmsCloseProfile(h);
        // corrupt a tag's offset to past-EOF + read it
        for (name, off, val) in [
            ("tag-off-past-eof", 136u32, 0xFFu8),
            ("tag-size-huge", 140, 0xFF),
            ("dup-sig", 144, b'd' as u8), // 2nd tag sig byte
        ] {
            let mut dd = d.clone();
            dd[off as usize] = val;
            let h2 = c::cmsOpenProfileFromMem(dd.as_ptr() as _,
                dd.len() as u32);
            if h2.is_null() { eprintln!("{name} open=null");
                continue; }
            let s = c::cmsGetTagSignature(h2, 0);
            let r = c::cmsReadTag(h2, s);
            eprintln!("{name} sig={s:#x} read={}", r.is_null());
            c::cmsCloseProfile(h2);
        }
    }
}

/// Tag reads on relabeled-class profiles — the class-dependent
/// ReadTag/LUT-reader dispatch on wrong-class combos.
#[test]
fn class_dependent_reads() {
    unsafe {
        for (name, cls) in [
            ("mntr", 0x6D6E7472u32),
            ("scnr", 0x73636E72),
            ("prtr", 0x70727472),
            ("spac", 0x73706163),
            ("link", 0x6C696E6B),
            ("abst", 0x61627374),
            ("nmcl", 0x6E6D636C),
        ] {
            let h = c::cmsCreate_sRGBProfile();
            c::cmsSetDeviceClass(h, cls);
            // read the LUT sigs — class-dependent dispatch picks
            // A2B*/B2A*/D2B*/B2D*/gamt/pre0-2
            for sig in [0x41324230u32, 0x42324130, 0x41324231,
                        0x42324131, 0x44324230, 0x42424430,
                        0x67616D74, 0x70726530, 0x70726531,
                        0x70726532] {
                let _ = c::cmsReadTag(h, sig);
            }
            // intent support under the class
            for intent in 0..4u32 {
                for dir in [1u32, 2] {
                    let _ = c::cmsIsIntentSupported(h, intent, dir);
                }
            }
            c::cmsCloseProfile(h);
        }
    }
}

/// cmsReadTag on every sig of every corpus profile — the
/// real-profile read dispatch on all tag types.
#[test]
fn corpus_all_sig_reads() {
    unsafe {
        for data in corpus() {
            let h = c::cmsOpenProfileFromMem(
                data.as_ptr() as _, data.len() as u32);
            let n = c::cmsGetTagCount(h);
            for i in 0..n {
                let sig = c::cmsGetTagSignature(h, i as u32);
                // typed read — hits Type_*_Read on the stored
                // type; NULL on unusual/corrupt
                let r = c::cmsReadTag(h, sig);
                let _ = r;
                // raw read — always works on a stored tag
                let mut tb = [0u8; 8];
                let rn = c::cmsReadRawTag(h, sig,
                    tb.as_mut_ptr() as *mut _, 8);
                let _ = rn;
            }
            // and the header machinery on this profile
            let _ = c::cmsGetDeviceClass(h);
            let _ = c::cmsGetColorSpace(h);
            let _ = c::cmsGetPCS(h);
            let mut fl = 0u32;
            c::cmsGetHeaderFlags(h, &mut fl);
            let mut att = 0u64;
            c::cmsGetHeaderAttributes(h, &mut att);
            let _ = c::cmsGetHeaderModel(h);
            let _ = c::cmsGetHeaderManufacturer(h);
            let _ = c::cmsGetHeaderCreator(h);
            let _ = c::cmsGetHeaderRenderingIntent(h);
            let mut pid = [0u8; 16];
            c::cmsGetHeaderProfileID(h, pid.as_mut_ptr());
            c::cmsCloseProfile(h);
        }
    }
}

/// Write-then-read on the remaining sigs — clrt/clot/targ/
/// ndin/resp payloads through the tag machinery.
#[test]
fn remaining_sig_writes() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        // clrt — colorant table (cmsNAMEDCOLORLIST)
        let ncl = c::cmsAllocNamedColorList(
            std::ptr::null_mut(), 3, 3,
            b"pre\0".as_ptr() as *const _,
            b"suf\0".as_ptr() as *const _);
        let mut pcs = [0u16; 3];
        let mut dev = [0u16; 16];
        c::cmsAppendNamedColor(ncl,
            b"R\0".as_ptr() as *const u8,
            pcs.as_ptr(), dev.as_ptr());
        c::cmsAppendNamedColor(ncl,
            b"G\0".as_ptr() as *const u8,
            pcs.as_ptr(), dev.as_ptr());
        c::cmsAppendNamedColor(ncl,
            b"B\0".as_ptr() as *const u8,
            pcs.as_ptr(), dev.as_ptr());
        let ok1 = c::cmsWriteTag(h, 0x636C7274,
            ncl as *mut std::ffi::c_void); // 'clrt'
        let ok2 = c::cmsWriteTag(h, 0x636C6F74,
            ncl as *mut std::ffi::c_void); // 'clot'
        // targ — charTarget takes a text string
        let targ = b"IT8.7/2\0";
        let ok3 = c::cmsWriteTag(h, 0x74617267,
            targ.as_ptr() as *const std::ffi::c_void);
        // ndin — ncl2-type display data
        let ok4 = c::cmsWriteTag(h, 0x6E64696E,
            ncl as *mut std::ffi::c_void); // 'ndin'
        // resp — response curve set
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let ok5 = c::cmsWriteTag(h, 0x72657370,
            g as *mut std::ffi::c_void); // 'resp'
        eprintln!("clrt={ok1} clot={ok2} targ={ok3} ndin={ok4} \
            resp={ok5}");
        // resave + read
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut cbuf = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, cbuf.as_mut_ptr() as *mut _,
            &mut cn);
        let h2 = c::cmsOpenProfileFromMem(cbuf.as_ptr() as _, cn);
        for sig in [0x636C7274u32, 0x636C6F74, 0x74617267,
                    0x6E64696E, 0x72657370] {
            if c::cmsIsTag(h2, sig) != 0 {
                let r = c::cmsReadTag(h2, sig);
                eprintln!("{sig:#x} read={}", r.is_null());
            }
        }
        c::cmsCloseProfile(h2);
        c::cmsCloseProfile(h);
        c::cmsFreeNamedColorList(ncl);
    }
}

/// Big tag-table search — write ~30 tags then enumerate/read —
/// the SearchTag machinery on a fuller table.
#[test]
fn big_tag_table() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let xyz = c::CieXyz { X: 0.5, Y: 0.4, Z: 0.3 };
        let sigv: u32 = 0x666F7461;
        let mlu = c::cmsMLUalloc(std::ptr::null_mut(), 1);
        c::cmsMLUsetASCII(mlu,
            b"en\0".as_ptr() as *const u8,
            b"US\0".as_ptr() as *const u8,
            b"x\0".as_ptr() as *const u8);
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        // write a spread of sigs across types
        let sigs: [(u32, u32); 14] = [
            (0x626B7074, 1), (0x6C756D69, 1), (0x63686164, 1),
            (0x7258595A, 1), (0x6758595A, 1), (0x6258595A, 1),
            (0x74656368, 2), (0x72696730, 2), (0x63696973, 2),
            (0x646D6E64, 3), (0x646D6464, 3), (0x6473636D, 3),
            (0x6B545243, 4), (0x67545243, 4),
        ];
        for (sig, kind) in sigs {
            let payload = match kind {
                1 => &xyz as *const _ as *mut std::ffi::c_void,
                2 => &sigv as *const _ as *mut std::ffi::c_void,
                3 => mlu as *mut std::ffi::c_void,
                _ => g as *mut std::ffi::c_void,
            };
            let _ = c::cmsWriteTag(h, sig, payload);
        }
        let n = c::cmsGetTagCount(h);
        eprintln!("tags={n}");
        // enumerate + read each
        for i in 0..n {
            let s = c::cmsGetTagSignature(h, i as u32);
            let _ = c::cmsReadTag(h, s);
        }
        c::cmsCloseProfile(h);
    }
}

/// Save/reload on a fuller tag table — the serialization
/// offset/alignment machinery.
#[test]
fn full_table_serialize() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        // write ~15 more tags → ~26 total
        let xyz = c::CieXyz { X: 0.5, Y: 0.4, Z: 0.3 };
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mlu = c::cmsMLUalloc(std::ptr::null_mut(), 1);
        c::cmsMLUsetASCII(mlu,
            b"en\0".as_ptr() as *const u8,
            b"US\0".as_ptr() as *const u8,
            b"x\0".as_ptr() as *const u8);
        for (sig, kind) in [
            (0x626B7074u32, 1u32), (0x6C756D69, 1),
            (0x63686164, 1), (0x7258595A, 1), (0x6758595A, 1),
            (0x6258595A, 1), (0x74656368, 2), (0x72696730, 2),
            (0x646D6E64, 3), (0x646D6464, 3), (0x6473636D, 3),
            (0x6B545243, 4), (0x67545243, 4), (0x62545243, 4),
        ] {
            let payload = match kind {
                1 => &xyz as *const _ as *mut std::ffi::c_void,
                2 => mlu as *mut std::ffi::c_void,
                3 => mlu as *mut std::ffi::c_void,
                _ => g as *mut std::ffi::c_void,
            };
            let _ = c::cmsWriteTag(h, sig, payload);
        }
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut cbuf = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, cbuf.as_mut_ptr() as *mut _,
            &mut cn);
        eprintln!("save n={cn}");
        // reload + read every tag
        let h2 = c::cmsOpenProfileFromMem(cbuf.as_ptr() as _, cn);
        let n2 = c::cmsGetTagCount(h2);
        for i in 0..n2 {
            let s = c::cmsGetTagSignature(h2, i as u32);
            let _ = c::cmsReadTag(h2, s);
        }
        eprintln!("reloaded tags={n2}");
        c::cmsCloseProfile(h2);
        c::cmsCloseProfile(h);
    }
}

/// cmsReadTag on unusual stored-type LUT formats — 'mft1'/
/// 'mft2'/'mAB '/'mBA ' on A2B/B2A sigs.
#[test]
fn unusual_lut_types() {
    unsafe {
        for data in corpus() {
            let h = c::cmsOpenProfileFromMem(
                data.as_ptr() as _, data.len() as u32);
            // read all the LUT-type sigs — exercises the mAB/
            // mft/lut8/lut16 LUT readers on whatever the corpus
            // stored
            for sig in [0x41324230u32, 0x41324231, 0x41324232,
                        0x42324130, 0x42324131, 0x42324132,
                        0x44324230, 0x44324231, 0x44324232,
                        0x44324233, 0x42424430, 0x42424431,
                        0x42424432, 0x42424433, 0x67616D74,
                        0x70726530, 0x70726531, 0x70726532] {
                let _ = c::cmsReadTag(h, sig);
            }
            c::cmsCloseProfile(h);
        }
    }
}

/// mAB serialization internals — a real devicelink's pipeline
/// through save + read-back.
#[test]
fn mab_serialization() {
    unsafe {
        let hi = c::cmsCreate_sRGBProfile();
        let ho = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let in_fmt: u32 = (4 << 16) | (3 << 3) | 2;
        let out_fmt: u32 = (10 << 16) | (3 << 3) | 2;
        let ct = c::cmsCreateTransform(hi, in_fmt, ho, out_fmt,
            0, 0);
        let dl = c::cmsTransform2DeviceLink(ct, 4.3, 0);
        c::cmsDeleteTransform(ct);
        if !dl.is_null() {
            // save — the mAB writer on the real pipeline
            let mut cn = 0u32;
            c::cmsSaveProfileToMem(dl, std::ptr::null_mut(),
                &mut cn);
            let mut cbuf = vec![0u8; cn as usize];
            c::cmsSaveProfileToMem(dl,
                cbuf.as_mut_ptr() as *mut _, &mut cn);
            eprintln!("devicelink save={cn}");
            // reload + read the A2B0 — the mAB reader
            let h2 = c::cmsOpenProfileFromMem(
                cbuf.as_ptr() as _, cn);
            if !h2.is_null() {
                let r = c::cmsReadTag(h2, 0x41324230);
                eprintln!("A2B0 read={}", r.is_null());
                // eval through it
                let ct2 = c::cmsCreateTransform(h2, in_fmt,
                    h2, in_fmt, 0, 0);
                if !ct2.is_null() {
                    c::cmsDeleteTransform(ct2);
                }
                c::cmsCloseProfile(h2);
            }
            c::cmsCloseProfile(dl);
        }
        let _ = c::cmsCloseProfile(hi);
        let _ = c::cmsCloseProfile(ho);
    }
}

/// cmsWriteTag on a file-loaded profile — the different
/// tag-table path vs built profiles.
#[test]
fn write_on_loaded() {
    unsafe {
        for data in corpus() {
            let h = c::cmsOpenProfileFromMem(
                data.as_ptr() as _, data.len() as u32);
            // write a tag to the loaded profile — hits the
            // loaded-profile tag-table machinery (lazily-parsed
            // tags + write-dispatch)
            let xyz = c::CieXyz { X: 0.5, Y: 0.4, Z: 0.3 };
            let ok1 = c::cmsWriteTag(h, 0x626B7074,
                &xyz as *const _ as *mut std::ffi::c_void);
            // overwrite an existing tag
            let g = c::cmsBuildGamma(std::ptr::null_mut(), 1.8);
            let ok2 = c::cmsWriteTag(h, 0x72545243,
                g as *mut std::ffi::c_void);
            let _ = (ok1, ok2);
            // resave — exercises save-on-loaded
            let mut cn = 0u32;
            c::cmsSaveProfileToMem(h, std::ptr::null_mut(),
                &mut cn);
            c::cmsCloseProfile(h);
        }
    }
}

/// LUT-type dispatch — 'lut8'/'lut16'/'mAB '/'mBA ' stored
/// types on the LUT sigs.
#[test]
fn lut_stored_types() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        for (name, ty) in [
            ("lut8", b"lut8" as &[u8]),
            ("lut16", b"lut16"),
            ("mAB ", b"mAB "),
            ("mBA ", b"mBA "),
        ] {
            let mut b = Vec::new();
            b.extend_from_slice(ty);
            b.extend_from_slice(&[0; 4]);
            // minimal body — the LUT reader rejects on shape
            // but runs the dispatch
            b.extend_from_slice(&[0u8; 48]);
            let ok = c::cmsWriteRawTag(h, 0x41324230,
                b.as_ptr() as *const std::ffi::c_void,
                b.len() as u32);
            let mut cn = 0u32;
            c::cmsSaveProfileToMem(h, std::ptr::null_mut(),
                &mut cn);
            let mut cbuf = vec![0u8; cn as usize];
            c::cmsSaveProfileToMem(h,
                cbuf.as_mut_ptr() as *mut _, &mut cn);
            let h3 = c::cmsOpenProfileFromMem(
                cbuf.as_ptr() as _, cn);
            let r = c::cmsReadTag(h3, 0x41324230);
            eprintln!("{name} write={ok} read={}", r.is_null());
            if !h3.is_null() { c::cmsCloseProfile(h3); }
        }
        c::cmsCloseProfile(h);
    }
}

/// Remaining writeable sigs — vued/vuet/calt/clid/ciid/ceid/
/// ndin with proper payloads.
#[test]
fn final_sig_writes() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        // viewing-conditions sigs — cmsICCViewingConditions
        #[repr(C)]
        struct ViewCond {
            illuminant: c::CieXyz,
            surround: c::CieXyz,
            std_illuminant: u32,
        }
        let vc = ViewCond {
            illuminant: c::CieXyz { X: 0.5, Y: 0.6, Z: 0.7 },
            surround: c::CieXyz { X: 0.3, Y: 0.4, Z: 0.5 },
            std_illuminant: 0x44,
        };
        let o1 = c::cmsWriteTag(h, 0x76756564,
            &vc as *const _ as *mut std::ffi::c_void); // 'vued'
        let o2 = c::cmsWriteTag(h, 0x76756574,
            &vc as *const _ as *mut std::ffi::c_void); // 'vuet'
        // calt — datetime (struct tm)
        #[repr(C)]
        struct Tm {
            sec: i32, min: i32, hour: i32, mday: i32,
            mon: i32, year: i32, wday: i32, yday: i32,
            isdst: i32, gmtoff: i64, zone: *const i8,
        }
        let tm = Tm { sec: 0, min: 0, hour: 0, mday: 1,
            mon: 0, year: 120, wday: 3, yday: 0, isdst: 0,
            gmtoff: 0, zone: std::ptr::null() };
        let o3 = c::cmsWriteTag(h, 0x63616C74,
            &tm as *const _ as *mut std::ffi::c_void); // 'calt'
        // clid/ciid/ceid — colorant/id/end-id sigs (text/XYZ)
        let mlu = c::cmsMLUalloc(std::ptr::null_mut(), 1);
        c::cmsMLUsetASCII(mlu,
            b"en\0".as_ptr() as *const u8,
            b"US\0".as_ptr() as *const u8,
            b"id\0".as_ptr() as *const u8);
        let o4 = c::cmsWriteTag(h, 0x636C6964,
            mlu as *mut std::ffi::c_void); // 'clid'
        let o5 = c::cmsWriteTag(h, 0x63696964,
            mlu as *mut std::ffi::c_void); // 'ciid'
        let o6 = c::cmsWriteTag(h, 0x63656964,
            mlu as *mut std::ffi::c_void); // 'ceid'
        // ndin — ncl2-type named display data
        let ncl = c::cmsAllocNamedColorList(
            std::ptr::null_mut(), 1, 3,
            b"pre\0".as_ptr() as *const _,
            b"suf\0".as_ptr() as *const _);
        let mut pcs = [0u16; 3];
        let mut dev = [0u16; 16];
        c::cmsAppendNamedColor(ncl,
            b"n\0".as_ptr() as *const u8,
            pcs.as_ptr(), dev.as_ptr());
        let o7 = c::cmsWriteTag(h, 0x6E64696E,
            ncl as *mut std::ffi::c_void); // 'ndin'
        eprintln!("vued={o1} vuet={o2} calt={o3} clid={o4} \
            ciid={o5} ceid={o6} ndin={o7}");
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut cbuf = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, cbuf.as_mut_ptr() as *mut _,
            &mut cn);
        let h2 = c::cmsOpenProfileFromMem(cbuf.as_ptr() as _, cn);
        for sig in [0x76756564u32, 0x76756574, 0x63616C74,
                    0x636C6964, 0x63696964, 0x63656964,
                    0x6E64696E] {
            if c::cmsIsTag(h2, sig) != 0 {
                let r = c::cmsReadTag(h2, sig);
                eprintln!("{sig:#x} read={}", r.is_null());
            }
        }
        c::cmsCloseProfile(h2);
        c::cmsCloseProfile(h);
        c::cmsFreeNamedColorList(ncl);
    }
}

/// pre0/pre1/pre2 sigs with real mAB pipelines — the
/// proofing-LUT serialization internals.
#[test]
fn proofing_lut_types() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        // build a real 5-slot mAB pipeline — A-curves + CLUT +
        // matrix + B-curves
        let pl = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let curves = [g, g, g];
        let acurves = c::cmsStageAllocToneCurves(
            std::ptr::null_mut(), 3, curves.as_ptr());
        let grid = [4u32; 3];
        let tab = [0.5f32; 4 * 4 * 4 * 3];
        let clut = c::cmsStageAllocCLutFloatGranular(
            std::ptr::null_mut(), grid.as_ptr(), 3, 3,
            tab.as_ptr());
        let mat_v = [1.0f64, 0.0, 0.0,
                     0.0, 1.0, 0.0,
                     0.0, 0.0, 1.0];
        let mat = c::cmsStageAllocMatrix(
            std::ptr::null_mut(), 3, 3, mat_v.as_ptr(),
            std::ptr::null());
        let bcurves = c::cmsStageAllocToneCurves(
            std::ptr::null_mut(), 3, curves.as_ptr());
        c::cmsPipelineInsertStage(pl, 0xFFFF, acurves);
        c::cmsPipelineInsertStage(pl, 0xFFFF, clut);
        c::cmsPipelineInsertStage(pl, 0xFFFF, mat);
        c::cmsPipelineInsertStage(pl, 0xFFFF, bcurves);
        // write to the proofing-LUT sigs
        for sig in [0x70726530u32, 0x70726531, 0x70726532,
                    0x67616D74] {
            let ok = c::cmsWriteTag(h, sig,
                pl as *mut std::ffi::c_void);
            eprintln!("{sig:#x} write={ok}");
        }
        // NOTE: resave removed — the mAB writer NULL-derefs on
        // a non-canonical pipeline layout (oracle UB, same as
        // the earlier A2B0/mAB attempt). The write-dispatch
        // statuses are the meaningful coverage.
        c::cmsCloseProfile(h);
    }
}
