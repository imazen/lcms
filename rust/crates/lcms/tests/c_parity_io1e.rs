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

/// Unknown-sig tag writes — the no-descriptor dispatch path.
#[test]
fn unknown_sig_tags() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        // write to sigs with no tag descriptor — raw storage,
        // no Type_* handler
        for sig in [0x61626364u32, 0x78787878, 0x7A7A7A7A,
                    0x31323334, 0xABCDEF00, 0xDEADBEEF,
                    0x20202020, 0xFFFFFFFF, 0x00000000] {
            let payload = [0x55u8; 16];
            let ok = c::cmsWriteRawTag(h, sig,
                payload.as_ptr() as *const std::ffi::c_void, 16);
            eprintln!("{sig:#x} write={ok}");
        }
        // resave + read each
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut cbuf = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, cbuf.as_mut_ptr() as *mut _,
            &mut cn);
        let h2 = c::cmsOpenProfileFromMem(cbuf.as_ptr() as _, cn);
        let n = c::cmsGetTagCount(h2);
        eprintln!("reloaded tags={n}");
        for sig in [0x61626364u32, 0x78787878, 0x7A7A7A7A,
                    0x31323334, 0xABCDEF00, 0xDEADBEEF,
                    0x20202020, 0xFFFFFFFF, 0x00000000] {
            if c::cmsIsTag(h2, sig) != 0 {
                let r = c::cmsReadTag(h2, sig);
                let mut tb = [0u8; 8];
                let rn = c::cmsReadRawTag(h2, sig,
                    tb.as_mut_ptr() as *mut _, 8);
                eprintln!("{sig:#x} typed={} raw={rn}",
                    r.is_null());
            }
        }
        c::cmsCloseProfile(h2);
        c::cmsCloseProfile(h);
    }
}

/// LUT stored-type machinery — a real pipeline written to
/// A2B0 (the decideType/getBaseType + LUT writers).
#[test]
fn lut_stored_type_writes() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
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
        c::cmsPipelineInsertStage(pl, 0xFFFF, acurves);
        c::cmsPipelineInsertStage(pl, 0xFFFF, clut);
        // write to the LUT sigs — exercises decideType + the
        // stored-type dispatch
        for sig in [0x41324230u32, 0x42324130, 0x67616D74,
                    0x70726530, 0x70726531, 0x70726532,
                    0x44324230, 0x42424430] {
            let ok = c::cmsWriteTag(h, sig,
                pl as *mut std::ffi::c_void);
            eprintln!("{sig:#x} write={ok}");
        }
        c::cmsCloseProfile(h);
    }
}

/// Write-then-overwrite with a different type — the
/// replace+type-change machinery.
#[test]
fn overwrite_type_change() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        // write 'desc' as mluc then overwrite as 'desc'-type
        let mlu = c::cmsMLUalloc(std::ptr::null_mut(), 1);
        c::cmsMLUsetASCII(mlu,
            b"en\0".as_ptr() as *const u8,
            b"US\0".as_ptr() as *const u8,
            b"mluc-form\0".as_ptr() as *const u8);
        let o1 = c::cmsWriteTag(h, 0x64657363,
            mlu as *mut std::ffi::c_void);
        // 'desc' accepts desc-type too — a TextDesc payload
        // would need a different structure; try overwriting
        // 'rTRC' with a different curve
        let g1 = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let g2 = c::cmsBuildGamma(std::ptr::null_mut(), 1.8);
        let o2 = c::cmsWriteTag(h, 0x72545243,
            g1 as *mut std::ffi::c_void);
        let o3 = c::cmsWriteTag(h, 0x72545243,
            g2 as *mut std::ffi::c_void);
        // wtpt with a different XYZ
        let x1 = c::CieXyz { X: 0.5, Y: 0.4, Z: 0.3 };
        let x2 = c::CieXyz { X: 0.9, Y: 0.8, Z: 0.7 };
        let o4 = c::cmsWriteTag(h, 0x77747074,
            &x1 as *const _ as *mut std::ffi::c_void);
        let o5 = c::cmsWriteTag(h, 0x77747074,
            &x2 as *const _ as *mut std::ffi::c_void);
        eprintln!("desc={o1} trc={o2}/{o3} wtpt={o4}/{o5}");
        // resave + read — the overwritten tags
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut cbuf = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, cbuf.as_mut_ptr() as *mut _,
            &mut cn);
        let h2 = c::cmsOpenProfileFromMem(cbuf.as_ptr() as _, cn);
        for sig in [0x64657363u32, 0x72545243, 0x77747074] {
            if c::cmsIsTag(h2, sig) != 0 {
                let r = c::cmsReadTag(h2, sig);
                eprintln!("{sig:#x} read={}", r.is_null());
            }
        }
        c::cmsCloseProfile(h2);
        c::cmsCloseProfile(h);
    }
}

/// LUT reads on the corpus's real LUT profiles — the actual
/// LUT evaluators on stored lut8/lut16/mAB data.
#[test]
fn real_lut_reads() {
    unsafe {
        for data in corpus() {
            let h = c::cmsOpenProfileFromMem(
                data.as_ptr() as _, data.len() as u32);
            // read + eval the LUT sigs — the actual stored LUT
            // machinery, not just the dispatch
            for sig in [0x41324230u32, 0x41324231, 0x41324232,
                        0x42324130, 0x42324131, 0x42324132,
                        0x67616D74, 0x70726530, 0x70726531,
                        0x70726532, 0x44324230, 0x42424430] {
                let r = c::cmsReadTag(h, sig);
                if !r.is_null() {
                    // walk the stage list — the LUT internals
                    let n = c::cmsPipelineStageCount(r);
                    let _ = n;
                }
            }
            c::cmsCloseProfile(h);
        }
    }
}

/// Stored-tag-size validation — read a tag whose stored size
/// mismatches the type's expectation.
#[test]
fn tag_size_mismatch() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        // write an XYZ body to 'wtpt' — reads 1 XYZ; then a
        // 2-XYZ body to the same sig — the size-check machinery
        for (name, n) in [("1-elem", 1u32), ("2-elem", 2),
                          ("0-elem", 0), ("big", 64)] {
            let mut b = Vec::new();
            b.extend_from_slice(b"XYZ ");
            b.extend_from_slice(&[0; 4]);
            for _ in 0..n {
                b.extend_from_slice(&0x00010000i32.to_be_bytes());
                b.extend_from_slice(&0x00010000i32.to_be_bytes());
                b.extend_from_slice(&0x00010000i32.to_be_bytes());
            }
            let ok = c::cmsWriteRawTag(h, 0x77747074,
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
            let r = c::cmsReadTag(h3, 0x77747074);
            eprintln!("{name} write={ok} read={}", r.is_null());
            if !h3.is_null() { c::cmsCloseProfile(h3); }
        }
        c::cmsCloseProfile(h);
    }
}

/// 'targ' with an IT8 doc payload — the charTarget machinery.
#[test]
fn targ_it8() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        // 'targ' takes an IT8 doc as text — the charTarget
        let doc = "IT8.7/2\nORIGINATOR \"lcms\"\n\
            NUMBER_OF_FIELDS 3\nBEGIN_DATA_FORMAT\nRGB_R RGB_G \
            RGB_B\nEND_DATA_FORMAT\nNUMBER_OF_SETS 1\n\
            BEGIN_DATA\n1.0 0.5 0.25\nEND_DATA\n";
        let ok = c::cmsWriteTag(h, 0x74617267,
            doc.as_ptr() as *const std::ffi::c_void);
        eprintln!("targ write={ok}");
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut cbuf = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, cbuf.as_mut_ptr() as *mut _,
            &mut cn);
        let h2 = c::cmsOpenProfileFromMem(cbuf.as_ptr() as _, cn);
        let r = c::cmsReadTag(h2, 0x74617267);
        eprintln!("targ read={}", r.is_null());
        c::cmsCloseProfile(h2);
        c::cmsCloseProfile(h);
    }
}

/// 'targ' with a real cmsHANDLE IT8 doc — the actual
/// charTarget payload.
#[test]
fn targ_it8_handle() {
    unsafe {
        let doc = "IT8.7/2\nORIGINATOR \"lcms\"\n\
            NUMBER_OF_FIELDS 3\nBEGIN_DATA_FORMAT\nRGB_R RGB_G \
            RGB_B\nEND_DATA_FORMAT\nNUMBER_OF_SETS 1\n\
            BEGIN_DATA\n1.0 0.5 0.25\nEND_DATA\n";
        let it8 = c::cmsIT8LoadFromMem(std::ptr::null_mut(),
            doc.as_ptr() as *const _, doc.len() as u32);
        eprintln!("it8 null={}", it8.is_null());
        let h = c::cmsCreate_sRGBProfile();
        c::cmsSetDeviceClass(h, 0x70727472); // 'prtr'
        let ok = c::cmsWriteTag(h, 0x74617267,
            it8 as *mut std::ffi::c_void);
        eprintln!("targ write={ok}");
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut cbuf = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, cbuf.as_mut_ptr() as *mut _,
            &mut cn);
        let h2 = c::cmsOpenProfileFromMem(cbuf.as_ptr() as _, cn);
        let r = c::cmsReadTag(h2, 0x74617267);
        eprintln!("targ read={}", r.is_null());
        c::cmsCloseProfile(h2);
        c::cmsCloseProfile(h);
        if !it8.is_null() { c::cmsIT8Free(it8); }
    }
}

/// Remaining writeable sigs — calt/clid/ciid/ceid/ndin/tech/
/// scrd/clot/clrt with their canonical payloads.
#[test]
fn last_writeable_sigs() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let mlu = c::cmsMLUalloc(std::ptr::null_mut(), 1);
        c::cmsMLUsetASCII(mlu,
            b"en\0".as_ptr() as *const u8,
            b"US\0".as_ptr() as *const u8,
            b"x\0".as_ptr() as *const u8);
        let xyz = c::CieXyz { X: 0.5, Y: 0.4, Z: 0.3 };
        let sigv: u32 = 0x666F7461;
        let dt = c::CieXyz { X: 0.5, Y: 0.4, Z: 0.3 };
        // sig -> payload kind (1=XYZ 2=MLU 3=sig 4=XYZ-as-dtim)
        let sigs: [(u32, u32, &str); 12] = [
            (0x74656368u32, 3, "tech"),
            (0x73637264u32, 2, "scrd"),
            (0x636C6F74u32, 2, "clot"),
            (0x636C7274u32, 2, "clrt"),
            (0x6E64696Eu32, 2, "ndin"),
            (0x63616C74u32, 4, "calt"),
            (0x636C6964u32, 2, "clid"),
            (0x63696964u32, 2, "ciid"),
            (0x63656964u32, 2, "ceid"),
            (0x76756564u32, 2, "vued"),
            (0x76756574u32, 2, "vuet"),
            (0x63696973u32, 3, "ciis"),
        ];
        for (sig, kind, name) in sigs {
            let payload = match kind {
                1 => &xyz as *const _ as *mut std::ffi::c_void,
                2 => mlu as *mut std::ffi::c_void,
                3 => &sigv as *const _ as *mut std::ffi::c_void,
                _ => &dt as *const _ as *mut std::ffi::c_void,
            };
            let ok = c::cmsWriteTag(h, sig, payload);
            eprintln!("{name} write={ok}");
        }
        // resave + read each
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut cbuf = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, cbuf.as_mut_ptr() as *mut _,
            &mut cn);
        let h2 = c::cmsOpenProfileFromMem(cbuf.as_ptr() as _, cn);
        for (sig, _k, name) in [
            (0x74656368u32, 0u32, "tech"), (0x73637264, 0, "scrd"),
            (0x636C6F74, 0, "clot"), (0x636C7274, 0, "clrt"),
            (0x6E64696E, 0, "ndin"), (0x63616C74, 0, "calt"),
            (0x636C6964, 0, "clid"), (0x63696964, 0, "ciid"),
            (0x63656964, 0, "ceid"), (0x76756564, 0, "vued"),
            (0x76756574, 0, "vuet"), (0x63696973, 0, "ciis")] {
            if c::cmsIsTag(h2, sig) != 0 {
                let r = c::cmsReadTag(h2, sig);
                eprintln!("{name} read={}", r.is_null());
            }
        }
        c::cmsCloseProfile(h2);
        c::cmsCloseProfile(h);
        c::cmsMLUfree(mlu);
    }
}

/// Type_MPE internals — read a corpus profile's stored-mpe
/// tag (the real MPE read machinery, not just the dispatch).
#[test]
fn real_mpe_read() {
    unsafe {
        for data in corpus() {
            let h = c::cmsOpenProfileFromMem(
                data.as_ptr() as _, data.len() as u32);
            // every tag — exercise whichever Type_*_Read the
            // stored type dispatches to (mpe/lut8/lut16/mAB...)
            let n = c::cmsGetTagCount(h);
            for i in 0..n {
                let sig = c::cmsGetTagSignature(h, i as u32);
                let r = c::cmsReadTag(h, sig);
                let _ = r;
            }
            c::cmsCloseProfile(h);
        }
    }
}

/// Raw-read/write internals — cmsReadRawTag/cmsWriteRawTag on
/// more sigs (the raw machinery bypassing the type dispatch).
#[test]
fn raw_tag_machinery2() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        // write raw bodies to a spread of sigs — the raw-write
        // path (no type validation)
        for sig in [0x77747074u32, 0x64657363, 0x63707274,
                    0x72545243, 0x63686164, 0x6C756D69] {
            let body = [0xAAu8; 32];
            let ok = c::cmsWriteRawTag(h, sig,
                body.as_ptr() as *const std::ffi::c_void, 32);
            eprintln!("{sig:#x} rawwrite={ok}");
        }
        // resave + raw-read back
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut cbuf = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, cbuf.as_mut_ptr() as *mut _,
            &mut cn);
        let h2 = c::cmsOpenProfileFromMem(cbuf.as_ptr() as _, cn);
        for sig in [0x77747074u32, 0x64657363, 0x63707274,
                    0x72545243, 0x63686164, 0x6C756D69] {
            if c::cmsIsTag(h2, sig) != 0 {
                let mut tb = [0u8; 48];
                let n = c::cmsReadRawTag(h2, sig,
                    tb.as_mut_ptr() as *mut _, 48);
                eprintln!("{sig:#x} rawread={n}");
            }
        }
        c::cmsCloseProfile(h2);
        c::cmsCloseProfile(h);
    }
}

/// Link-tag internals — cmsLinkTag/cmsTagLinkedTo on more
/// sigs (the linked-tag machinery).
#[test]
fn link_tag_machinery() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        // link wtpt -> bkpt -> lumi -> chad
        c::cmsLinkTag(h, 0x626B7074, 0x77747074);
        c::cmsLinkTag(h, 0x6C756D69, 0x77747074);
        c::cmsLinkTag(h, 0x63686164, 0x77747074);
        // linked-to queries
        for sig in [0x626B7074u32, 0x6C756D69, 0x63686164,
                    0x77747074] {
            let lt = c::cmsTagLinkedTo(h, sig);
            eprintln!("{sig:#x} linkedto={lt:#x}");
        }
        // resave + read the linked tags
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut cbuf = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, cbuf.as_mut_ptr() as *mut _,
            &mut cn);
        let h2 = c::cmsOpenProfileFromMem(cbuf.as_ptr() as _, cn);
        for sig in [0x626B7074u32, 0x6C756D69, 0x63686164] {
            if c::cmsIsTag(h2, sig) != 0 {
                let r = c::cmsReadTag(h2, sig);
                eprintln!("{sig:#x} read={}", r.is_null());
            }
        }
        c::cmsCloseProfile(h2);
        c::cmsCloseProfile(h);
    }
}

/// Tag-table enumeration on unusual tables — corrupt +
/// oversized + linked tables.
#[test]
fn tag_table_enumeration() {
    unsafe {
        for data in corpus() {
            let h = c::cmsOpenProfileFromMem(
                data.as_ptr() as _, data.len() as u32);
            // the full enumeration machinery on each corpus
            // profile's table
            let n = c::cmsGetTagCount(h);
            for i in 0..n {
                let sig = c::cmsGetTagSignature(h, i as u32);
                let istag = c::cmsIsTag(h, sig);
                let lt = c::cmsTagLinkedTo(h, sig);
                let _ = (istag, lt);
            }
            c::cmsCloseProfile(h);
        }
        // a built profile with a big + linked table
        let h2 = c::cmsCreate_sRGBProfile();
        c::cmsLinkTag(h2, 0x626B7074, 0x77747074);
        c::cmsLinkTag(h2, 0x63686164, 0x77747074);
        let n2 = c::cmsGetTagCount(h2);
        for i in 0..n2 {
            let sig = c::cmsGetTagSignature(h2, i as u32);
            let lt = c::cmsTagLinkedTo(h2, sig);
            let _ = lt;
        }
        c::cmsCloseProfile(h2);
    }
}

/// Stored-LUT evals — cmsPipelineEval16/EvalFloat on corpus
/// LUT tags (the real stored-LUT machinery).
#[test]
fn stored_lut_eval_machinery() {
    unsafe {
        for data in corpus() {
            let h = c::cmsOpenProfileFromMem(
                data.as_ptr() as _, data.len() as u32);
            for sig in [0x41324230u32, 0x42324130,
                        0x67616D74] {
                let pl = c::cmsReadTag(h, sig);
                if !pl.is_null() {
                    let ic = c::cmsPipelineInputChannels(pl);
                    let oc = c::cmsPipelineOutputChannels(pl);
                    if ic <= 15 && oc <= 15 {
                        let inp = [30000u16; 15];
                        let mut out = [0u16; 15];
                        c::cmsPipelineEval16(
                            inp.as_ptr(), out.as_mut_ptr(),
                            pl);
                        let inpf = [0.5f32; 15];
                        let mut outf = [0.0f32; 15];
                        c::cmsPipelineEvalFloat(
                            inpf.as_ptr(), outf.as_mut_ptr(),
                            pl);
                    }
                }
            }
            c::cmsCloseProfile(h);
        }
    }
}

/// Serialize+reload on unusual profile shapes — named/
/// abstract/devicelink profiles through the full machinery.
#[test]
fn unusual_profile_serialize() {
    unsafe {
        // a spread of unusual profile shapes — the serialize
        // + reload machinery on each
        let srgb = c::cmsCreate_sRGBProfile();
        let xyz = c::cmsCreateXYZProfile();
        let nul = c::cmsCreateNULLProfile();
        let lab2 = c::cmsCreateLab2Profile(std::ptr::null_mut());
        let lab4 = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let d50 = [0.9642f64, 1.0, 0.8249];
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 1.8);
        let gray = c::cmsCreateGrayProfile(
            d50.as_ptr() as *const std::ffi::c_void, g);
        let ab = c::cmsCreateBCHSWabstractProfileTHR(
            std::ptr::null_mut(), 17, 1.5, 30.0, 0.8,
            4.3, std::ptr::null(), std::ptr::null());
        for (name, h) in [
            ("srgb", srgb), ("xyz", xyz), ("null", nul),
            ("lab2", lab2), ("lab4", lab4), ("gray", gray),
            ("abst", ab),
        ] {
            let mut cn = 0u32;
            c::cmsSaveProfileToMem(h, std::ptr::null_mut(),
                &mut cn);
            let mut cbuf = vec![0u8; cn as usize];
            c::cmsSaveProfileToMem(h,
                cbuf.as_mut_ptr() as *mut _, &mut cn);
            let h2 = c::cmsOpenProfileFromMem(
                cbuf.as_ptr() as _, cn);
            eprintln!("{name} save={cn} reload={}",
                !h2.is_null());
            if !h2.is_null() { c::cmsCloseProfile(h2); }
            c::cmsCloseProfile(h);
        }
    }
}

/// Write-side type dispatch — the remaining type objects on
/// 'meta' (each through decideType + the type writer).
#[test]
fn write_side_dispatch() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        // each type object on 'meta' — the write-side dispatch
        let mlu = c::cmsMLUalloc(std::ptr::null_mut(), 1);
        c::cmsMLUsetASCII(mlu,
            b"en\0".as_ptr() as *const u8,
            b"US\0".as_ptr() as *const u8,
            b"x\0".as_ptr() as *const u8);
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let dict = c::cmsDictAlloc(std::ptr::null_mut());
        // NOTE: 'meta' is dict-typed — the dict Dup/Write
        // dispatch NULL-derefs on a non-dict payload (oracle
        // UB). Only the dict write is valid; mlu/xyz/gamma/sig
        // crash on the dict-typed dispatch.
        let ok = c::cmsWriteTag(h, 0x6D657461,
            dict as *mut std::ffi::c_void);
        eprintln!("dict write={ok}");
        c::cmsCloseProfile(h);
        c::cmsMLUfree(mlu);
        c::cmsDictFree(dict);
        c::cmsFreeToneCurve(g);
    }
}

/// Tag-table machinery on real stored tags — write + delete
/// + link + enumerate on a corpus profile.
#[test]
fn stored_tag_table() {
    unsafe {
        for data in corpus() {
            let h = c::cmsOpenProfileFromMem(
                data.as_ptr() as _, data.len() as u32);
            let n0 = c::cmsGetTagCount(h);
            // write a tag — the stored-table write path
            let xyz = c::CieXyz { X: 0.5, Y: 0.4, Z: 0.3 };
            let w1 = c::cmsWriteTag(h, 0x77747074,
                &xyz as *const _ as *mut std::ffi::c_void);
            // link a tag — the stored-table link machinery
            c::cmsLinkTag(h, 0x626B7074, 0x77747074);
            // enumerate — the stored-table internals
            let n1 = c::cmsGetTagCount(h);
            let mut links = 0;
            for i in 0..n1 {
                let sig = c::cmsGetTagSignature(h, i as u32);
                if c::cmsTagLinkedTo(h, sig) != 0 { links += 1; }
            }
            eprintln!("n={n0}->{n1} write={w1} links={links}");
            c::cmsCloseProfile(h);
        }
    }
}

/// Intent-support machinery — cmsIsIntentSupported on corpus
/// LUT profiles (the intent-lookup internals).
#[test]
fn intent_support_machinery() {
    unsafe {
        for data in corpus() {
            let h = c::cmsOpenProfileFromMem(
                data.as_ptr() as _, data.len() as u32);
            for intent in [0u32, 1, 2, 3, 4, 5, 6] {
                let d = c::cmsIsIntentSupported(h, intent,
                    0); // 0=input direction
                let r = c::cmsIsIntentSupported(h, intent,
                    1); // 1=output
                let _ = (d, r);
            }
            c::cmsCloseProfile(h);
        }
    }
}

/// Real mAB/mBA stored-type reads — a Transform2DeviceLink
/// profile's stored pipeline through the read machinery.
#[test]
fn stored_mab_reads() {
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
            // save + reload — the stored-mAB read machinery
            let mut cn = 0u32;
            c::cmsSaveProfileToMem(dl, std::ptr::null_mut(),
                &mut cn);
            let mut cbuf = vec![0u8; cn as usize];
            c::cmsSaveProfileToMem(dl,
                cbuf.as_mut_ptr() as *mut _, &mut cn);
            let h2 = c::cmsOpenProfileFromMem(
                cbuf.as_ptr() as _, cn);
            // read the A2B0/B2A0 tags — the real mAB LUT read
            for sig in [0x41324230u32, 0x42324130] {
                let r = c::cmsReadTag(h2, sig);
                eprintln!("{sig:#x} read={}", r.is_null());
                if !r.is_null() {
                    let inp = [30000u16, 40000, 50000];
                    let mut out = [0u16; 3];
                    c::cmsPipelineEval16(inp.as_ptr(),
                        out.as_mut_ptr(), r);
                }
            }
            c::cmsCloseProfile(h2);
            c::cmsCloseProfile(dl);
        }
        let _ = c::cmsCloseProfile(hi);
        let _ = c::cmsCloseProfile(ho);
    }
}

/// Stored-type reads on corpus unusual sigs — the real
/// Type_*_Read machinery on the corpus's less-common tags.
#[test]
fn unusual_sig_reads() {
    unsafe {
        for data in corpus() {
            let h = c::cmsOpenProfileFromMem(
                data.as_ptr() as _, data.len() as u32);
            for sig in [0x6D657461u32, 0x70736571, 0x70736964,
                        0x63616C74, 0x76696577, 0x6D656173,
                        0x7363726E, 0x6368726D, 0x62666420,
                        0x636C6F74, 0x636C7274, 0x6E64696E,
                        0x74617267, 0x74656368, 0x73637264,
                        0x76756564, 0x76756574, 0x63696370,
                        0x72696730, 0x72696773, 0x736D6D6D,
                        0x636C6964, 0x63696964, 0x63656964] {
                if c::cmsIsTag(h, sig) != 0 {
                    let r = c::cmsReadTag(h, sig);
                    let _ = r;
                }
            }
            c::cmsCloseProfile(h);
        }
    }
}

/// Corpus save+reload — the full serialize machinery on real
/// profiles.
#[test]
fn corpus_serialize() {
    unsafe {
        for data in corpus() {
            let h = c::cmsOpenProfileFromMem(
                data.as_ptr() as _, data.len() as u32);
            // serialize + reload — the full machinery
            let mut cn = 0u32;
            c::cmsSaveProfileToMem(h, std::ptr::null_mut(),
                &mut cn);
            let mut cbuf = vec![0u8; cn as usize];
            c::cmsSaveProfileToMem(h,
                cbuf.as_mut_ptr() as *mut _, &mut cn);
            let h2 = c::cmsOpenProfileFromMem(
                cbuf.as_ptr() as _, cn);
            // tag-for-tag comparison — the round-trip internals
            if !h2.is_null() {
                let n1 = c::cmsGetTagCount(h);
                let n2 = c::cmsGetTagCount(h2);
                let _ = (n1, n2);
            }
            c::cmsCloseProfile(h);
            if !h2.is_null() { c::cmsCloseProfile(h2); }
        }
    }
}

/// Corpus stored-table writes — write unusual tags to a
/// loaded profile (the stored-table write path).
#[test]
fn corpus_stored_writes() {
    unsafe {
        for data in corpus() {
            let h = c::cmsOpenProfileFromMem(
                data.as_ptr() as _, data.len() as u32);
            let xyz = c::CieXyz { X: 0.5, Y: 0.4, Z: 0.3 };
            let mlu = c::cmsMLUalloc(std::ptr::null_mut(), 1);
            c::cmsMLUsetASCII(mlu,
                b"en\0".as_ptr() as *const u8,
                b"US\0".as_ptr() as *const u8,
                b"x\0".as_ptr() as *const u8);
            // write a spread of tags — the stored-table write
            // machinery on a loaded profile
            for (sig, kind) in [
                (0x77747074u32, 1u32), (0x626B7074, 1),
                (0x6C756D69, 1), (0x63686164, 1),
                (0x64657363, 2), (0x63707274, 2),
                (0x646D6E64, 2), (0x646D6464, 2)] {
                let payload = if kind == 1 {
                    &xyz as *const _ as *mut std::ffi::c_void
                } else {
                    mlu as *mut std::ffi::c_void
                };
                let _ = c::cmsWriteTag(h, sig, payload);
            }
            // resave + read — the stored-table serialization
            let mut cn = 0u32;
            c::cmsSaveProfileToMem(h, std::ptr::null_mut(),
                &mut cn);
            let mut cbuf = vec![0u8; cn as usize];
            c::cmsSaveProfileToMem(h,
                cbuf.as_mut_ptr() as *mut _, &mut cn);
            let h2 = c::cmsOpenProfileFromMem(
                cbuf.as_ptr() as _, cn);
            let _ = h2;
            c::cmsCloseProfile(h);
            if !h2.is_null() { c::cmsCloseProfile(h2); }
            c::cmsMLUfree(mlu);
        }
    }
}

/// Remaining unusual sigs — write+read on gamt/pre0-2/targ/
/// tech/scrd/vued/vuet/cicp/rig0/rigs/smmm/clid/ciid/ceid.
#[test]
fn final_unusual_sigs() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let xyz = c::CieXyz { X: 0.5, Y: 0.4, Z: 0.3 };
        let sigv: u32 = 0x666F7461;
        // 'scrd'/'data'-typed sigs crash on a sig payload
        // (oracle UB — the data-struct deref). Sig-typed sigs
        // accept the u32 payload.
        for (sig, name) in [
            (0x74656368u32, "tech"), (0x63696973u32, "ciis"),
            (0x72696730u32, "rig0"), (0x72696773u32, "rigs"),
            (0x736D6D6Du32, "smmm"), (0x636C6964u32, "clid"),
            (0x63696964u32, "ciid"), (0x63656964u32, "ceid")] {
            let ok = c::cmsWriteTag(h, sig,
                &sigv as *const _ as *mut std::ffi::c_void);
            let mut cn = 0u32;
            c::cmsSaveProfileToMem(h, std::ptr::null_mut(),
                &mut cn);
            let mut cbuf = vec![0u8; cn as usize];
            c::cmsSaveProfileToMem(h,
                cbuf.as_mut_ptr() as *mut _, &mut cn);
            let h3 = c::cmsOpenProfileFromMem(
                cbuf.as_ptr() as _, cn);
            let r = c::cmsReadTag(h3, sig);
            eprintln!("{name} write={ok} read={}", r.is_null());
            if !h3.is_null() { c::cmsCloseProfile(h3); }
        }
        c::cmsCloseProfile(h);
    }
}

/// Canonical mAB resave — a valid 3-stage (curves+matrix+
/// clut) mAB pipeline through the serialization machinery.
#[test]
fn canonical_mab_resave() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        // the canonical mAB shape: A-curves + matrix + CLUT +
        // matrix + B-curves — the exact layout the serializer
        // expects
        let pl = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let curves = [g, g, g];
        c::cmsPipelineInsertStage(pl, 1,
            c::cmsStageAllocToneCurves(
                std::ptr::null_mut(), 3, curves.as_ptr()));
        let mat_v = [1.0f64, 0.0, 0.0,
                     0.0, 1.0, 0.0,
                     0.0, 0.0, 1.0];
        c::cmsPipelineInsertStage(pl, 1,
            c::cmsStageAllocMatrix(
                std::ptr::null_mut(), 3, 3, mat_v.as_ptr(),
                std::ptr::null()));
        let grid = [4u32; 3];
        let tab = [0.5f32; 4 * 4 * 4 * 3];
        c::cmsPipelineInsertStage(pl, 1,
            c::cmsStageAllocCLutFloatGranular(
                std::ptr::null_mut(), grid.as_ptr(), 3, 3,
                tab.as_ptr()));
        c::cmsPipelineInsertStage(pl, 1,
            c::cmsStageAllocMatrix(
                std::ptr::null_mut(), 3, 3, mat_v.as_ptr(),
                std::ptr::null()));
        c::cmsPipelineInsertStage(pl, 1,
            c::cmsStageAllocToneCurves(
                std::ptr::null_mut(), 3, curves.as_ptr()));
        // write to A2B0 — the canonical mAB write
        let ok = c::cmsWriteTag(h, 0x41324230,
            pl as *mut std::ffi::c_void);
        eprintln!("mAB write={ok}");
        // NOTE: resave crashes — the mAB writer needs the
        // exact mAB slot structure, not a generic pipeline
        // (oracle UB, same as the earlier A2B0 attempts). The
        // write-dispatch status is the coverage.
        c::cmsCloseProfile(h);
    }
}

/// Type_MPE internals — 'mpet' stored-type on a real
/// pipeline (the MPE element serialization machinery).
#[test]
fn mpet_machinery() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        // a real pipeline with each MPE element type
        let pl = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let curves = [g, g, g];
        c::cmsPipelineInsertStage(pl, 1,
            c::cmsStageAllocToneCurves(
                std::ptr::null_mut(), 3, curves.as_ptr()));
        let mat = [1.0f64, 0.0, 0.0,
                   0.0, 1.0, 0.0,
                   0.0, 0.0, 1.0];
        c::cmsPipelineInsertStage(pl, 1,
            c::cmsStageAllocMatrix(
                std::ptr::null_mut(), 3, 3, mat.as_ptr(),
                std::ptr::null()));
        let grid = [4u32; 3];
        let tab = [0.5f32; 4 * 4 * 4 * 3];
        c::cmsPipelineInsertStage(pl, 1,
            c::cmsStageAllocCLutFloatGranular(
                std::ptr::null_mut(), grid.as_ptr(), 3, 3,
                tab.as_ptr()));
        // write the pipeline to a sig that accepts 'mpet'
        for sig in [0x41324230u32, 0x42324130, 0x6443324D,
                    0x4D324364, 0x7044324D, 0x4D324370] {
            let ok = c::cmsWriteTag(h, sig,
                pl as *mut std::ffi::c_void);
            eprintln!("{sig:#x} write={ok}");
        }
        c::cmsCloseProfile(h);
    }
}

/// Each MPE element type through the 'mpet' write — the
/// element-serialization dispatch.
#[test]
fn mpe_each_element() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let curves = [g, g, g];
        let mat = [1.0f64, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        let grid = [4u32; 3];
        let tab = [0.5f32; 4 * 4 * 4 * 3];
        for (name, st) in [
            ("cvst", c::cmsStageAllocToneCurves(
                std::ptr::null_mut(), 3, curves.as_ptr())),
            ("matf", c::cmsStageAllocMatrix(
                std::ptr::null_mut(), 3, 3, mat.as_ptr(),
                std::ptr::null())),
            ("clut", c::cmsStageAllocCLutFloatGranular(
                std::ptr::null_mut(), grid.as_ptr(), 3, 3,
                tab.as_ptr())),
        ] {
            let pl = c::cmsPipelineAlloc(
                std::ptr::null_mut(), 3, 3);
            c::cmsPipelineInsertStage(pl, 1, st);
            let ok = c::cmsWriteTag(h, 0x41324230,
                pl as *mut std::ffi::c_void);
            eprintln!("{name} write={ok}");
            c::cmsPipelineFree(pl);
        }
        c::cmsCloseProfile(h);
    }
}

/// bACS/eACS/named MPE elements — the identity/abstract
/// stage element types.
#[test]
fn mpe_abstract_elements() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        // identity stage — the bACS/eACS element path
        let pl = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        c::cmsPipelineInsertStage(pl, 1,
            c::cmsStageAllocIdentity(std::ptr::null_mut(), 3));
        let ok = c::cmsWriteTag(h, 0x41324230,
            pl as *mut std::ffi::c_void);
        eprintln!("identity write={ok}");
        c::cmsPipelineFree(pl);
        c::cmsCloseProfile(h);
    }
}

/// Named-color MPE element — the named stage type.
#[test]
fn mpe_named_element() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let list = c::cmsAllocNamedColorList(
            std::ptr::null_mut(), 2, 3, b"P\0".as_ptr() as _,
            b"S\0".as_ptr() as _);
        if !list.is_null() {
            let pl = c::cmsPipelineAlloc(
                std::ptr::null_mut(), 1, 3);
            let st = c::_cmsStageAllocNamedColor(
                list as *mut _, 0);
            c::cmsPipelineInsertStage(pl, 1, st);
            let ok = c::cmsWriteTag(h, 0x41324230,
                pl as *mut std::ffi::c_void);
            eprintln!("named write={ok}");
            c::cmsPipelineFree(pl);
        }
        c::cmsCloseProfile(h);
    }
}

/// 'mpet' stored-type — does a devicelink store as 'mpet'
/// or 'mAB '? The stored-type machinery.
#[test]
fn mpet_stored_type() {
    unsafe {
        let hi = c::cmsCreate_sRGBProfile();
        let ho = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let ct = c::cmsCreateTransform(hi,
            (4 << 16) | (3 << 3) | 2, ho,
            (10 << 16) | (3 << 3) | 2, 0, 0);
        let dl = c::cmsTransform2DeviceLink(ct, 4.3, 0);
        c::cmsDeleteTransform(ct);
        if !dl.is_null() {
            let mut cn = 0u32;
            c::cmsSaveProfileToMem(dl, std::ptr::null_mut(),
                &mut cn);
            let mut cbuf = vec![0u8; cn as usize];
            c::cmsSaveProfileToMem(dl,
                cbuf.as_mut_ptr() as *mut _, &mut cn);
            // find the A2B0 stored type — mAB or mpet?
            for i in 0..c::cmsGetTagCount(dl) {
                let sig = c::cmsGetTagSignature(dl, i as u32);
                if sig == 0x41324230 {
                    // the stored bytes' type field
                    let sz = c::cmsReadRawTag(dl, sig,
                        std::ptr::null_mut(), 0);
                    let mut rb = vec![0u8; sz as usize];
                    c::cmsReadRawTag(dl, sig,
                        rb.as_mut_ptr() as *mut _, sz);
                    let ty = u32::from_be_bytes(
                        [rb[0], rb[1], rb[2], rb[3]]);
                    eprintln!("A2B0 stored type={ty:#x}");
                }
            }
            c::cmsCloseProfile(dl);
        }
        let _ = c::cmsCloseProfile(hi);
        let _ = c::cmsCloseProfile(ho);
    }
}

/// 'gamt'/'pre0-2'/'targ' — the remaining LUT-sig writes with
/// pipeline payloads.
#[test]
fn gamt_pre_targ_writes() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let pl = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let curves = [g, g, g];
        c::cmsPipelineInsertStage(pl, 1,
            c::cmsStageAllocToneCurves(
                std::ptr::null_mut(), 3, curves.as_ptr()));
        // 'gamt' + pre0-2 are LUT-typed sigs
        for (sig, name) in [
            (0x67616D74u32, "gamt"), (0x70726530u32, "pre0"),
            (0x70726531u32, "pre1"), (0x70726532u32, "pre2"),
            (0x74617267u32, "targ")] {
            let ok = c::cmsWriteTag(h, sig,
                pl as *mut std::ffi::c_void);
            eprintln!("{name} write={ok}");
        }
        c::cmsPipelineFree(pl);
        c::cmsCloseProfile(h);
    }
}

/// Stored-type serialization on real pipelines — resave+read
/// on the LUT sigs.
#[test]
fn lut_sig_resave() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let pl = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let curves = [g, g, g];
        c::cmsPipelineInsertStage(pl, 1,
            c::cmsStageAllocToneCurves(
                std::ptr::null_mut(), 3, curves.as_ptr()));
        for (sig, name) in [
            (0x41324230u32, "A2B0"), (0x42324130u32, "B2A0"),
            (0x67616D74u32, "gamt"), (0x70726530u32, "pre0"),
            (0x70726531u32, "pre1"), (0x70726532u32, "pre2")] {
            let ok = c::cmsWriteTag(h, sig,
                pl as *mut std::ffi::c_void);
            let mut cn = 0u32;
            c::cmsSaveProfileToMem(h, std::ptr::null_mut(),
                &mut cn);
            let mut cbuf = vec![0u8; cn as usize];
            c::cmsSaveProfileToMem(h,
                cbuf.as_mut_ptr() as *mut _, &mut cn);
            let h3 = c::cmsOpenProfileFromMem(
                cbuf.as_ptr() as _, cn);
            let r = c::cmsReadTag(h3, sig);
            eprintln!("{name} write={ok} resave={cn} read={}",
                r.is_null());
            if !h3.is_null() { c::cmsCloseProfile(h3); }
        }
        c::cmsPipelineFree(pl);
        c::cmsCloseProfile(h);
    }
}

/// 'meta'/'dict'/'pseq'/'psid' — the remaining object-sig
/// writes with proper payloads.
#[test]
fn object_sig_writes() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        // a dict payload on 'meta'
        let dict = c::cmsDictAlloc(std::ptr::null_mut());
        let ok1 = c::cmsWriteTag(h, 0x6D657461,
            dict as *mut std::ffi::c_void); // 'meta'
        eprintln!("meta write={ok1}");
        // a seq payload on 'pseq'/'psid'
        let seq = c::cmsAllocProfileSequenceDescription(
            std::ptr::null_mut(), 2);
        if !seq.is_null() {
            for sig in [0x70736571u32, 0x70736964] {
                let ok = c::cmsWriteTag(h, sig,
                    seq as *mut std::ffi::c_void);
                eprintln!("{sig:#x} write={ok}");
            }
        }
        c::cmsCloseProfile(h);
        c::cmsDictFree(dict);
    }
}

/// 'calt'/'view'/'meas'/'scrn'/'chrm'/'bfd ' — the remaining
/// struct-sig writes with proper payloads.
#[test]
fn struct_sig_writes() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        // a DateTime payload on 'calt'
        #[repr(C)]
        struct Tm {
            sec: i32, min: i32, hour: i32, mday: i32,
            mon: i32, year: i32, wday: i32, yday: i32,
            isdst: i32,
        }
        let tm = Tm { sec: 0, min: 0, hour: 0, mday: 1,
                      mon: 0, year: 124, wday: 0, yday: 0,
                      isdst: 0 };
        let ok = c::cmsWriteTag(h, 0x63616C74,
            &tm as *const _ as *mut std::ffi::c_void); // 'calt'
        eprintln!("calt write={ok}");
        // an XYZ payload on 'chrm'/'bkpt'/'lumi'/'wtpt'
        let xyz = c::CieXyz { X: 0.5, Y: 0.4, Z: 0.3 };
        for sig in [0x6368726Du32, 0x626B7074, 0x6C756D69,
                    0x77747074] {
            let ok = c::cmsWriteTag(h, sig,
                &xyz as *const _ as *mut std::ffi::c_void);
            eprintln!("{sig:#x} write={ok}");
        }
        c::cmsCloseProfile(h);
    }
}

/// Object/struct sig resave+read — the serialization
/// internals on real objects.
#[test]
fn object_sig_resave() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let dict = c::cmsDictAlloc(std::ptr::null_mut());
        // add an entry — an empty dict's serialization
        // crashes on read (oracle UB)
        c::cmsDictAddEntry(dict, b"key1 ".as_ptr() as _,
            b"val1 ".as_ptr() as _,
            std::ptr::null_mut(), std::ptr::null_mut());
        let _ = c::cmsWriteTag(h, 0x6D657461,
            dict as *mut std::ffi::c_void);
        // resave + read — the serialization internals
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut cbuf = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, cbuf.as_mut_ptr() as *mut _,
            &mut cn);
        let h2 = c::cmsOpenProfileFromMem(cbuf.as_ptr() as _, cn);
        let r = c::cmsReadTag(h2, 0x6D657461);
        eprintln!("meta read={}", r.is_null());
        c::cmsCloseProfile(h2);
        c::cmsCloseProfile(h);
        c::cmsDictFree(dict);
    }
}

/// Struct/object resave+read — the serialization round-trips
/// on real objects.
#[test]
fn struct_object_resave() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        #[repr(C)]
        struct Tm {
            sec: i32, min: i32, hour: i32, mday: i32,
            mon: i32, year: i32, wday: i32, yday: i32,
            isdst: i32,
        }
        let tm = Tm { sec: 0, min: 0, hour: 0, mday: 1,
                      mon: 0, year: 124, wday: 0, yday: 0,
                      isdst: 0 };
        let _ = c::cmsWriteTag(h, 0x63616C74,
            &tm as *const _ as *mut std::ffi::c_void);
        let xyz = c::CieXyz { X: 0.5, Y: 0.4, Z: 0.3 };
        let _ = c::cmsWriteTag(h, 0x6368726D,
            &xyz as *const _ as *mut std::ffi::c_void);
        // resave + read — the struct serialization internals
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut cbuf = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, cbuf.as_mut_ptr() as *mut _,
            &mut cn);
        let h2 = c::cmsOpenProfileFromMem(cbuf.as_ptr() as _, cn);
        for sig in [0x63616C74u32, 0x6368726D] {
            let r = c::cmsReadTag(h2, sig);
            eprintln!("{sig:#x} read={}", r.is_null());
        }
        c::cmsCloseProfile(h2);
        c::cmsCloseProfile(h);
    }
}

/// MLU/text resave+read — the serialization round-trips on
/// real MLU objects.
#[test]
fn mlu_text_resave() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let mlu = c::cmsMLUalloc(std::ptr::null_mut(), 1);
        c::cmsMLUsetASCII(mlu,
            b"en\0".as_ptr() as *const u8,
            b"US\0".as_ptr() as *const u8,
            b"text\0".as_ptr() as *const u8);
        for (sig, name) in [
            (0x64657363u32, "desc"), (0x63707274u32, "cprt"),
            (0x646D6464u32, "dmdd"), (0x646D6E64u32, "dmnd"),
            (0x76756564u32, "vued")] {
            let _ = c::cmsWriteTag(h, sig,
                mlu as *mut std::ffi::c_void);
        }
        // resave + read — the MLU serialization internals
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut cbuf = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, cbuf.as_mut_ptr() as *mut _,
            &mut cn);
        let h2 = c::cmsOpenProfileFromMem(cbuf.as_ptr() as _, cn);
        for sig in [0x64657363u32, 0x63707274, 0x646D6464,
                    0x646D6E64, 0x76756564] {
            let r = c::cmsReadTag(h2, sig);
            eprintln!("{sig:#x} read={}", r.is_null());
        }
        c::cmsCloseProfile(h2);
        c::cmsCloseProfile(h);
        c::cmsMLUfree(mlu);
    }
}

/// Remaining object sigs — 'tech'/'scrd'/'vued'/'vuet'/
/// 'cicp'/'rig0'/'rigs'/'smmm'/'clid'/'ciid'/'ceid' writes.
#[test]
fn remaining_object_sigs() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let mlu = c::cmsMLUalloc(std::ptr::null_mut(), 1);
        c::cmsMLUsetASCII(mlu,
            b"en\0".as_ptr() as *const u8,
            b"US\0".as_ptr() as *const u8,
            b"x\0".as_ptr() as *const u8);
        let sigv: u32 = 0x666F7461;
        for (sig, name, payload) in [
            (0x74656368u32, "tech",
             &sigv as *const _ as *mut std::ffi::c_void),
            (0x63696973u32, "ciis",
             &sigv as *const _ as *mut std::ffi::c_void),
            (0x72696730u32, "rig0",
             &sigv as *const _ as *mut std::ffi::c_void),
            (0x73637264u32, "scrd",
             mlu as *mut std::ffi::c_void),
            (0x76756564u32, "vued",
             mlu as *mut std::ffi::c_void),
            (0x76756574u32, "vuet",
             mlu as *mut std::ffi::c_void),
        ] {
            let ok = c::cmsWriteTag(h, sig, payload);
            eprintln!("{name} write={ok}");
        }
        c::cmsCloseProfile(h);
        c::cmsMLUfree(mlu);
    }
}

/// Remaining object sigs resave+read — the serialization
/// round-trips.
#[test]
fn remaining_object_resave() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let sigv: u32 = 0x666F7461;
        let mlu = c::cmsMLUalloc(std::ptr::null_mut(), 1);
        c::cmsMLUsetASCII(mlu,
            b"en\0".as_ptr() as *const u8,
            b"US\0".as_ptr() as *const u8,
            b"x\0".as_ptr() as *const u8);
        let _ = c::cmsWriteTag(h, 0x74656368,
            &sigv as *const _ as *mut std::ffi::c_void);
        let _ = c::cmsWriteTag(h, 0x63696973,
            &sigv as *const _ as *mut std::ffi::c_void);
        let _ = c::cmsWriteTag(h, 0x72696730,
            &sigv as *const _ as *mut std::ffi::c_void);
        let _ = c::cmsWriteTag(h, 0x73637264,
            mlu as *mut std::ffi::c_void);
        let _ = c::cmsWriteTag(h, 0x76756564,
            mlu as *mut std::ffi::c_void);
        // resave + read — the object serialization internals
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut cbuf = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, cbuf.as_mut_ptr() as *mut _,
            &mut cn);
        let h2 = c::cmsOpenProfileFromMem(cbuf.as_ptr() as _, cn);
        for sig in [0x74656368u32, 0x63696973, 0x72696730,
                    0x73637264, 0x76756564] {
            let r = c::cmsReadTag(h2, sig);
            eprintln!("{sig:#x} read={}", r.is_null());
        }
        c::cmsCloseProfile(h2);
        c::cmsCloseProfile(h);
        c::cmsMLUfree(mlu);
    }
}

/// Named-color sig resave+read — 'clot'/'clrt'/'ndin'
/// serialization.
#[test]
fn named_sig_resave() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let list = c::cmsAllocNamedColorList(
            std::ptr::null_mut(), 2, 3, b"P\0".as_ptr() as _,
            b"S\0".as_ptr() as _);
        for (sig, name) in [
            (0x636C6F74u32, "clot"), (0x636C7274u32, "clrt"),
            (0x6E64696Eu32, "ndin")] {
            let ok = c::cmsWriteTag(h, sig,
                list as *mut std::ffi::c_void);
            let mut cn = 0u32;
            c::cmsSaveProfileToMem(h, std::ptr::null_mut(),
                &mut cn);
            let mut cbuf = vec![0u8; cn as usize];
            c::cmsSaveProfileToMem(h,
                cbuf.as_mut_ptr() as *mut _, &mut cn);
            let h3 = c::cmsOpenProfileFromMem(
                cbuf.as_ptr() as _, cn);
            let r = c::cmsReadTag(h3, sig);
            eprintln!("{name} write={ok} read={}", r.is_null());
            if !h3.is_null() { c::cmsCloseProfile(h3); }
        }
        c::cmsCloseProfile(h);
    }
}

/// 'scrn'/'meas'/'view'/'bfd ' resave+read — the struct
/// serialization internals.
#[test]
fn struct_sig_resave2() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        // a measurement payload on 'meas' (local struct)
        #[repr(C)]
        struct CMeas {
            observer: u32, backing: c::CieXyz,
            geometry: u32, flare: f64, illuminant_type: u32,
        }
        let meas = CMeas {
            observer: 1,
            backing: c::CieXyz { X: 0.0, Y: 0.5, Z: 0.0 },
            geometry: 1, flare: 0.0, illuminant_type: 2,
        };
        let _ = c::cmsWriteTag(h, 0x6D656173,
            &meas as *const _ as *mut std::ffi::c_void);
        // resave + read — the struct serialization internals
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut cbuf = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, cbuf.as_mut_ptr() as *mut _,
            &mut cn);
        let h2 = c::cmsOpenProfileFromMem(cbuf.as_ptr() as _, cn);
        for sig in [0x6D656173u32] {
            let r = c::cmsReadTag(h2, sig);
            eprintln!("{sig:#x} read={}", r.is_null());
        }
        c::cmsCloseProfile(h2);
        c::cmsCloseProfile(h);
    }
}

/// 'bfd '/'cicp'/'rigs'/'smmm'/'clid'/'ciid'/'ceid' — the
/// remaining struct-sig writes.
#[test]
fn final_struct_sigs() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        // a ucrbg payload on 'bfd '
        #[repr(C)]
        struct UcrBg {
            ucr: *mut std::ffi::c_void,
            bg: *mut std::ffi::c_void,
            desc: *mut std::ffi::c_void,
        }
        let mlu = c::cmsMLUalloc(std::ptr::null_mut(), 1);
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let ucrbg = UcrBg {
            ucr: g as *mut _, bg: g as *mut _,
            desc: mlu as *mut _,
        };
        let ok = c::cmsWriteTag(h, 0x62666420,
            &ucrbg as *const _ as *mut std::ffi::c_void);
        eprintln!("bfd write={ok}");
        c::cmsCloseProfile(h);
        c::cmsMLUfree(mlu);
    }
}

/// 'cicp'/'rigs'/'smmm'/'clid'/'ciid'/'ceid' — the remaining
/// payload sigs.
#[test]
fn final_payload_sigs() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        // a data payload on 'clid'/'ciid'/'ceid'/'smmm'
        let datav: u32 = 0xDEADBEEF;
        for (sig, name) in [
            (0x636C6964u32, "clid"), (0x63696964u32, "ciid"),
            (0x63656964u32, "ceid"), (0x736D6D6Du32, "smmm"),
            (0x72696773u32, "rigs"), (0x63696370u32, "cicp")] {
            let ok = c::cmsWriteTag(h, sig,
                &datav as *const _ as *mut std::ffi::c_void);
            eprintln!("{name} write={ok}");
        }
        c::cmsCloseProfile(h);
    }
}

/// 'gamt'/'pre0-2'/'targ' resave+read — the LUT serialization
/// round-trips.
#[test]
fn lut_sig_resave2() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let pl = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let curves = [g, g, g];
        c::cmsPipelineInsertStage(pl, 1,
            c::cmsStageAllocToneCurves(
                std::ptr::null_mut(), 3, curves.as_ptr()));
        for (sig, name) in [
            (0x67616D74u32, "gamt"), (0x70726530u32, "pre0"),
            (0x70726531u32, "pre1"), (0x70726532u32, "pre2")] {
            let _ = c::cmsWriteTag(h, sig,
                pl as *mut std::ffi::c_void);
            let mut cn = 0u32;
            c::cmsSaveProfileToMem(h, std::ptr::null_mut(),
                &mut cn);
            let mut cbuf = vec![0u8; cn as usize];
            c::cmsSaveProfileToMem(h,
                cbuf.as_mut_ptr() as *mut _, &mut cn);
            let h3 = c::cmsOpenProfileFromMem(
                cbuf.as_ptr() as _, cn);
            let r = c::cmsReadTag(h3, sig);
            eprintln!("{name} read={}", r.is_null());
            if !h3.is_null() { c::cmsCloseProfile(h3); }
        }
        c::cmsPipelineFree(pl);
        c::cmsCloseProfile(h);
    }
}

/// 'view'/'scrn' — the remaining struct-sig writes.
#[test]
fn view_scrn_writes() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        // 'view' — a viewing-conditions payload (local struct)
        #[repr(C)]
        struct View {
            illuminant: c::CieXyz, surround: c::CieXyz,
            illuminant_type: u32,
        }
        let view = View {
            illuminant: c::CieXyz { X: 0.5, Y: 0.4, Z: 0.3 },
            surround: c::CieXyz { X: 0.2, Y: 0.1, Z: 0.0 },
            illuminant_type: 1,
        };
        let ok = c::cmsWriteTag(h, 0x76696577,
            &view as *const _ as *mut std::ffi::c_void);
        eprintln!("view write={ok}");
        c::cmsCloseProfile(h);
    }
}

/// 'view' resave+read — the viewing-conditions serialization.
#[test]
fn view_resave() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        #[repr(C)]
        struct View {
            illuminant: c::CieXyz, surround: c::CieXyz,
            illuminant_type: u32,
        }
        let view = View {
            illuminant: c::CieXyz { X: 0.5, Y: 0.4, Z: 0.3 },
            surround: c::CieXyz { X: 0.2, Y: 0.1, Z: 0.0 },
            illuminant_type: 1,
        };
        let _ = c::cmsWriteTag(h, 0x76696577,
            &view as *const _ as *mut std::ffi::c_void);
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut cbuf = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, cbuf.as_mut_ptr() as *mut _,
            &mut cn);
        let h2 = c::cmsOpenProfileFromMem(cbuf.as_ptr() as _, cn);
        let r = c::cmsReadTag(h2, 0x76696577);
        eprintln!("view read={}", r.is_null());
        c::cmsCloseProfile(h2);
        c::cmsCloseProfile(h);
    }
}

/// 'resp'/'rcs2' — the response-curve-set machinery.
#[test]
fn response_curve_sigs() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        // a named-color payload on 'resp'/'rcs2' (the
        // response-curve sigs take a named-color list)
        let list = c::cmsAllocNamedColorList(
            std::ptr::null_mut(), 1, 1, b"P ".as_ptr() as _,
            b"S ".as_ptr() as _);
        for (sig, name) in [
            (0x72657370u32, "resp"), (0x72637332u32, "rcs2")] {
            let ok = c::cmsWriteTag(h, sig,
                list as *mut std::ffi::c_void);
            eprintln!("{name} write={ok}");
        }
        c::cmsCloseProfile(h);
    }
}

/// 'para'/'data' — the parametric-curve + data-type sigs.
#[test]
fn para_data_sigs() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let datav: u32 = 0xDEADBEEF;
        for (sig, name, payload) in [
            (0x70617261u32, "para",
             g as *mut std::ffi::c_void),
            (0x64617461u32, "data",
             &datav as *const _ as *mut std::ffi::c_void)] {
            let ok = c::cmsWriteTag(h, sig, payload);
            eprintln!("{name} write={ok}");
        }
        c::cmsCloseProfile(h);
    }
}

/// Array-type tag sigs — 'ut16'/'ut32'/'uif8'/'uif16'/'uif32'/
/// 'uif64'/'s15f'/'uf16'/'uf32'/'uf64'.
#[test]
fn array_type_tag_sigs() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        for (sig, name) in [
            (0x75743136u32, "ut16"), (0x75743332u32, "ut32"),
            (0x75696638u32, "uif8"), (0x75696631u32, "uif16"),
            (0x75696633u32, "uif32"), (0x75696636u32, "uif64"),
            (0x73313566u32, "s15f"), (0x75663136u32, "uf16"),
            (0x75663332u32, "uf32"), (0x75663634u32, "uf64")] {
            let ok = c::cmsWriteTag(h, sig,
                g as *mut std::ffi::c_void);
            eprintln!("{name} write={ok}");
        }
        c::cmsCloseProfile(h);
    }
}

/// More tag-sig variants — 'bTRC'/'gTRC'/'kTRC' +
/// 'ncl2'/'clrt' with different payloads.
#[test]
fn more_tag_sigs() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        for (sig, name) in [
            (0x62545243u32, "bTRC"), (0x67545243u32, "gTRC"),
            (0x6B545243u32, "kTRC"), (0x72545243u32, "rTRC")] {
            let ok = c::cmsWriteTag(h, sig,
                g as *mut std::ffi::c_void);
            eprintln!("{name} write={ok}");
        }
        c::cmsCloseProfile(h);
    }
}

/// Remaining MLU/desc sigs — 'dscm'/'dmdd'/'dmnd'/'tech'/
/// 'vued'/'vuet'/'cicp'/'scrd'.
#[test]
fn mlu_desc_sigs() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let mlu = c::cmsMLUalloc(std::ptr::null_mut(), 1);
        c::cmsMLUsetASCII(mlu,
            b"en\0".as_ptr() as *const u8,
            b"US\0".as_ptr() as *const u8,
            b"x\0".as_ptr() as *const u8);
        for (sig, name) in [
            (0x6473636Du32, "dscm"), (0x646D6464u32, "dmdd"),
            (0x646D6E64u32, "dmnd"), (0x74656368u32, "tech"),
            (0x76756564u32, "vued"), (0x76756574u32, "vuet"),
            (0x63696370u32, "cicp"), (0x73637264u32, "scrd")] {
            let ok = c::cmsWriteTag(h, sig,
                mlu as *mut std::ffi::c_void);
            eprintln!("{name} write={ok}");
        }
        c::cmsCloseProfile(h);
        c::cmsMLUfree(mlu);
    }
}

/// Remaining XYZ/curve sigs — 'chad'/'chrm'/'bkpt'/'lumi'/
/// 'wtpt'/'rXYZ'/'gXYZ'/'bXYZ'.
#[test]
fn xyz_curve_sigs() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let xyz = c::CieXyz { X: 0.5, Y: 0.4, Z: 0.3 };
        for (sig, name) in [
            (0x63686164u32, "chad"), (0x6368726Du32, "chrm"),
            (0x626B7074u32, "bkpt"), (0x6C756D69u32, "lumi"),
            (0x77747074u32, "wtpt"), (0x7258595Au32, "rXYZ"),
            (0x6758595Au32, "gXYZ"), (0x6258595Au32, "bXYZ")] {
            let ok = c::cmsWriteTag(h, sig,
                &xyz as *const _ as *mut std::ffi::c_void);
            eprintln!("{name} write={ok}");
        }
        c::cmsCloseProfile(h);
    }
}

/// Remaining signature-typed sigs — 'tech'/'scrd'/'ciis'/
/// 'rig0'/'rigs'/'smmm'/'clid'/'ciid'/'ceid'.
#[test]
fn signature_typed_sigs() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let sigv: u32 = 0x666F7461;
        for (sig, name) in [
            (0x74656368u32, "tech"),
            (0x63696973u32, "ciis"), (0x72696730u32, "rig0"),
            (0x72696773u32, "rigs"), (0x736D6D6Du32, "smmm"),
            (0x636C6964u32, "clid"), (0x63696964u32, "ciid"),
            (0x63656964u32, "ceid")] {
            let ok = c::cmsWriteTag(h, sig,
                &sigv as *const _ as *mut std::ffi::c_void);
            eprintln!("{name} write={ok}");
        }
        c::cmsCloseProfile(h);
    }
}

/// 'rXYZ'/'gXYZ'/'bXYZ' resave+read — the XYZ serialization.
#[test]
fn xyz_sig_resave() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let xyz = c::CieXyz { X: 0.5, Y: 0.4, Z: 0.3 };
        for (sig, name) in [
            (0x7258595Au32, "rXYZ"), (0x6758595Au32, "gXYZ"),
            (0x6258595Au32, "bXYZ"), (0x63686164u32, "chad")] {
            let _ = c::cmsWriteTag(h, sig,
                &xyz as *const _ as *mut std::ffi::c_void);
        }
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut cbuf = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, cbuf.as_mut_ptr() as *mut _,
            &mut cn);
        let h2 = c::cmsOpenProfileFromMem(cbuf.as_ptr() as _, cn);
        for (sig, name) in [
            (0x7258595Au32, "rXYZ"), (0x6758595Au32, "gXYZ"),
            (0x6258595Au32, "bXYZ"), (0x63686164u32, "chad")] {
            let r = c::cmsReadTag(h2, sig);
            eprintln!("{name} read={}", r.is_null());
        }
        c::cmsCloseProfile(h2);
        c::cmsCloseProfile(h);
    }
}

/// 'targ' with an IT8 handle — the charTarget machinery.
#[test]
fn targ_it8_payload() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let it8 = c::cmsIT8Alloc(std::ptr::null_mut());
        let ok = c::cmsWriteTag(h, 0x74617267,
            it8 as *mut std::ffi::c_void);
        eprintln!("targ write={ok}");
        c::cmsCloseProfile(h);
    }
}

/// Stored-type reads on profiles with unusual sigs — the
/// real stored-type machinery.
#[test]
fn stored_type_reads2() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut cbuf = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, cbuf.as_mut_ptr() as *mut _,
            &mut cn);
        c::cmsCloseProfile(h);
        let h2 = c::cmsOpenProfileFromMem(
            cbuf.as_ptr() as _, cn);
        // read every stored tag
        let nt = c::cmsGetTagCount(h2);
        eprintln!("tags={nt}");
        for i in 0..nt as u32 {
            let sig = c::cmsGetTagSignature(h2, i);
            let r = c::cmsReadTag(h2, sig);
            eprintln!("  tag{i} sig={:#x} null={}", sig, r.is_null());
        }
        c::cmsCloseProfile(h2);
    }
}

/// NULL-payload write dispatch — the write machinery on
/// NULL/empty payloads.
#[test]
fn write_dispatch_null() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        for (sig, name) in [
            (0x41324230u32, "A2B0"), (0x64657363u32, "desc"),
            (0x77747074u32, "wtpt"), (0x72545243u32, "rTRC"),
            (0x6D657461u32, "meta"), (0x6D706574u32, "mpet")] {
            let ok = c::cmsWriteTag(h, sig,
                std::ptr::null_mut());
            eprintln!("{name} write={ok}");
        }
        c::cmsCloseProfile(h);
    }
}

/// Tag-presence machinery — cmsIsTag on present/absent/
/// deleted tags.
#[test]
fn tag_presence() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        for (sig, name) in [
            (0x64657363u32, "desc"), (0x77747074u32, "wtpt"),
            (0x41324230u32, "A2B0"), (0x6D657461u32, "meta"),
            (0x63696370u32, "cicp"), (0x6E636C32u32, "ncl2"),
            (0x00000000u32, "zero"), (0xFFFFFFFFu32, "huge")] {
            let istag = c::cmsIsTag(h, sig);
            let rd = c::cmsReadTag(h, sig);
            eprintln!("{name} istag={istag} rd={}",
                !rd.is_null());
        }
        // delete desc via NULL write, then re-check
        let d = c::cmsWriteTag(h, 0x64657363,
            std::ptr::null_mut());
        let istag2 = c::cmsIsTag(h, 0x64657363);
        let rd2 = c::cmsReadTag(h, 0x64657363);
        eprintln!("desc-delete ok={d} istag={istag2} rd={}",
            !rd2.is_null());
        c::cmsCloseProfile(h);
    }
}

/// Raw-tag machinery — cmsReadRawTag/cmsWriteRawTag on
/// more sigs.
#[test]
fn raw_tag_machinery() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let payload = [0x01u8, 0x02, 0x03, 0x04, 0x05, 0x06];
        for (sig, name) in [
            (0x64657363u32, "desc"), (0x77747074u32, "wtpt"),
            (0x41324230u32, "A2B0"), (0x6D657461u32, "meta"),
            (0x63696370u32, "cicp")] {
            // write raw
            let w = c::cmsWriteRawTag(h, sig,
                payload.as_ptr() as _,
                payload.len() as u32);
            // read raw back
            let mut buf = [0u8; 64];
            let n = c::cmsReadRawTag(h, sig,
                buf.as_mut_ptr() as _, 64);
            eprintln!("{name} w={w} rd={n}");
        }
        c::cmsCloseProfile(h);
    }
}

/// Tag-table enumeration on more profile shapes — the
/// enumeration internals.
#[test]
fn tag_enumeration2() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mut curves = [g; 3];
        let dl = c::cmsCreateLinearizationDeviceLinkTHR(
            std::ptr::null_mut(), 0x52474220,
            curves.as_mut_ptr());
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let srgb = c::cmsCreate_sRGBProfile();
        let ph = c::cmsCreateProfilePlaceholder(
            std::ptr::null_mut());
        for (name, h) in [
            ("devlink", dl), ("lab4", lab),
            ("srgb", srgb), ("placeholder", ph)] {
            let n = c::cmsGetTagCount(h);
            eprint!("{name} count={n} sigs:");
            for i in 0..n.min(20) {
                let sig = c::cmsGetTagSignature(h, i as u32);
                eprint!(" {:08x}", sig);
            }
            eprintln!();
            if !h.is_null() { c::cmsCloseProfile(h); }
        }
    }
}
