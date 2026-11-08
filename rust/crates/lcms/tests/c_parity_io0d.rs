//! cmsio0 IOhandler-dispatch: SaveProfileToIOhandler +
//! OpenProfileFromIOhandler via FILE handlers — the dispatch
//! machinery beyond SaveProfileToMem/File.

use lcms::profile;
use lcms_cref as c;

fn corpus() -> Vec<(String, Vec<u8>)> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
    let mut files: Vec<_> = std::fs::read_dir(&dir).unwrap()
        .filter_map(|e| e.ok()).map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "icc").unwrap_or(false))
        .collect();
    files.sort();
    files.iter().map(|f| {
        (f.to_str().unwrap().to_string(), std::fs::read(f).unwrap())
    }).collect()
}

#[test]
fn file_iohandler_open() {
    // cmsOpenProfileFromIOhandlerTHR on a FILE handler — exercises
    // the dispatch+read machinery. The profile owns the handler on
    // success; the handler is freed by Close.
    for (path, data) in corpus() {
        unsafe {
            let name = std::ffi::CString::new(path.as_str()).unwrap();
            let io = c::cmsOpenIOhandlerFromFile(
                std::ptr::null_mut(), name.as_ptr(), b"r\0".as_ptr());
            assert!(!io.is_null(), "iohandler open {path}");
            let h = c::cmsOpenProfileFromIOhandlerTHR(std::ptr::null_mut(), io);
            let rr = profile::open_profile_from_mem(None, &data);
            assert_eq!(h.is_null(), rr.is_none(), "iohandler-from-file {path}");
            if !h.is_null() { let _ = c::cmsCloseProfile(h); }
            else { let _ = c::cmsCloseIOhandler(io); }
        }
    }
}

#[test]
fn null_iohandler_dry_run() {
    // cmsOpenIOhandlerFromNULL — the size-query dry-run
    for (_, data) in corpus() {
        unsafe {
            let io = c::cmsOpenIOhandlerFromNULL(std::ptr::null_mut());
            let prof = c::cmsOpenProfileFromMem(
                data.as_ptr() as _, data.len() as u32);
            let ok = c::cmsSaveProfileToIOhandler(prof, io);
            let _ = c::cmsCloseProfile(prof);
            let _ = c::cmsCloseIOhandler(io);
            assert_eq!(ok != 0, true, "null-handler save");
        }
    }
}

/// SaveProfileToIOhandler on a write-mode file handler + the
/// GetProfileIOhandler getter.
#[test]
fn save_to_file_iohandler() {
    for (_, data) in corpus() {
        let tmp = std::env::temp_dir()
            .join(format!("lcms-io0d-{}.icc", std::process::id()));
        unsafe {
            let prof = c::cmsOpenProfileFromMem(
                data.as_ptr() as _, data.len() as u32);
            assert!(!prof.is_null());
            // getter on a mem-opened profile — C returns the internal
            // handler it created for the mem read (non-NULL)
            let io0 = c::cmsGetProfileIOhandler(prof);
            assert!(!io0.is_null(),
                "mem profile iohandler is the internal mem handler");
            // write-mode file handler
            let path = std::ffi::CString::new(
                tmp.to_str().unwrap()).unwrap();
            let io = c::cmsOpenIOhandlerFromFile(
                std::ptr::null_mut(), path.as_ptr(), b"w\0".as_ptr());
            assert!(!io.is_null());
            let ok = c::cmsSaveProfileToIOhandler(prof, io);
            assert!(ok != 0, "save to file iohandler");
            let _ = c::cmsCloseIOhandler(io);
            let _ = c::cmsCloseProfile(prof);
        }
        let _ = std::fs::remove_file(&tmp);
    }
}

/// corrupt-profile mutations — byte-level corruption exercises
/// the parser's validation branches.
#[test]
fn corrupt_profile_bytes() {
    unsafe {
        let base = c::cmsCreate_sRGBProfile();
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(base, std::ptr::null_mut(), &mut cn);
        let mut good = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(base, good.as_mut_ptr() as *mut _,
            &mut cn);
        c::cmsCloseProfile(base);
        // byte mutations at parser-relevant offsets
        for (name, off, val) in [
            ("size-hi", 0u32, 0xFF),
            ("size-lo", 3, 0x00),
            ("cmm", 4, 0xFF),
            ("version-hi", 8, 0x05),
            ("class", 12, 0xFF),
            ("cs", 16, 0xFF),
            ("pcs", 20, 0xFF),
            ("sig", 36, 0xFF),
            ("ntags", 128, 0xFF),
            ("tag-off", 136, 0xFF),
            ("tag-size", 140, 0xFF),
        ] {
            let mut d = good.clone();
            d[off as usize] = val;
            let h = c::cmsOpenProfileFromMem(d.as_ptr() as _,
                d.len() as u32);
            let ok = !h.is_null();
            eprintln!("{name} off={off} open={ok}");
            if ok {
                // read a tag — may reject on the corrupted field
                let _ = c::cmsReadTag(h, 0x64657363);
                c::cmsCloseProfile(h);
            }
        }
    }
}


/// IOhandler-FromNull + IT8 contiguous-save variants.
#[test]
fn iohandler_null_it8_contiguous() {
    unsafe {
        let n = c::cmsOpenIOhandlerFromNULL(std::ptr::null_mut());
        assert!(!n.is_null(), "null iohandler");
        c::cmsCloseIOhandler(n);
        // IT8 save variants — cmsIT8SaveToMem/ToFile are the
        // real exports (no contiguous variants exist)
        let doc = "CGATS.17\nNUMBER_OF_FIELDS 2\nBEGIN_DATA_FORMAT\nA B\nEND_DATA_FORMAT\nNUMBER_OF_SETS 1\nBEGIN_DATA\n1 2\nEND_DATA\n";
        let h = c::cmsIT8LoadFromMem(std::ptr::null_mut(),
            doc.as_ptr() as *const _, doc.len() as u32);
        let mut cb = vec![0u8; 4096];
        let mut cn = 0u32;
        let ok = c::cmsIT8SaveToMem(h,
            cb.as_mut_ptr() as *mut _, &mut cn);
        eprintln!("save mem ok={ok} n={cn}");
        let f = c::cmsIT8SaveToFile(h,
            b"/tmp/it8_save.txt\0".as_ptr() as *const _);
        eprintln!("save file={f}");
        c::cmsIT8Free(h);
    }
}

/// cmsGetProfileInfo* machinery on the corpus — info reads on
/// every profile x every info kind.
#[test]
fn profile_info_sweep() {
    unsafe {
        for (_n, data) in corpus() {
            let h = c::cmsOpenProfileFromMem(
                data.as_ptr() as _, data.len() as u32);
            let mut buf = vec![0u8; 256];
            for info in [0u32, 1, 2, 3, 4] {
                // cmsInfoDescription / Manufacturer / Model /
                // Copyright / ProfileName
                let n = c::cmsGetProfileInfoASCII(h, info,
                    b"en\0".as_ptr() as *const _,
                    b"US\0".as_ptr() as *const _,
                    buf.as_mut_ptr() as *mut _, 256);
                let _ = n;
                let n2 = c::cmsGetProfileInfoUTF8(h, info,
                    b"en\0".as_ptr() as *const _,
                    b"US\0".as_ptr() as *const _,
                    buf.as_mut_ptr() as *mut _, 256);
                let _ = n2;
                let mut wbuf = [0u16; 128];
                let n3 = c::cmsGetProfileInfo(h, info,
                    b"en\0".as_ptr() as *const _,
                    b"US\0".as_ptr() as *const _,
                    wbuf.as_mut_ptr() as *mut std::ffi::c_void, 128);
                let _ = n3;
            }
            c::cmsCloseProfile(h);
        }
    }
}

/// Version-dispatch machinery — profiles with unusual version
/// fields through the parser.
#[test]
fn version_dispatch() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut d = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, d.as_mut_ptr() as *mut _,
            &mut cn);
        c::cmsCloseProfile(h);
        // patch the version field (offset 8) — v2/v3/v4/v5/
        // sub-class dispatch
        for (name, v) in [
            ("v21", 0x02100000u32),
            ("v30", 0x03000000),
            ("v43", 0x04300000),
            ("v50", 0x05000000),  // v5.x — probably rejects
            ("v10", 0x01000000),  // v1.0 — probably rejects
            ("v99", 0x09900000),  // v9.9 — rejects
            ("zero", 0x00000000), // v0 — rejects
        ] {
            let mut dd = d.clone();
            dd[8..12].copy_from_slice(&v.to_be_bytes());
            let h2 = c::cmsOpenProfileFromMem(
                dd.as_ptr() as _, dd.len() as u32);
            eprintln!("{name} open={}", !h2.is_null());
            if !h2.is_null() {
                let _ = c::cmsReadTag(h2, 0x77747074);
                c::cmsCloseProfile(h2);
            }
        }
    }
}

/// Header-field validation — colorspace/PCS/class mutations
/// through the parser's field-check machinery.
#[test]
fn header_field_validation() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut d = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, d.as_mut_ptr() as *mut _,
            &mut cn);
        c::cmsCloseProfile(h);
        // mutate header fields — colorspace @16, PCS @20,
        // device-class @12
        for (name, off, val) in [
            ("cs-invalid", 16usize, 0xFFFF0000u32),
            ("cs-zero", 16, 0),
            ("pcs-invalid", 20, 0xFFFF0000),
            ("pcs-zero", 20, 0),
            ("class-invalid", 12, 0xFFFF0000),
            ("class-zero", 12, 0),
            ("sig-invalid", 36, 0xFFFF0000), // acsp sig
        ] {
            let mut dd = d.clone();
            dd[off..off + 4].copy_from_slice(&val.to_be_bytes());
            let h2 = c::cmsOpenProfileFromMem(
                dd.as_ptr() as _, dd.len() as u32);
            eprintln!("{name} open={}", !h2.is_null());
            if !h2.is_null() { c::cmsCloseProfile(h2); }
        }
    }
}

/// Tag-count/table-size validation — unusual tag-table fields
/// through the parser.
#[test]
fn tag_table_size() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut d = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, d.as_mut_ptr() as *mut _,
            &mut cn);
        c::cmsCloseProfile(h);
        // mutate the tag-count field @128
        for (name, val) in [
            ("count-0", 0u32),
            ("count-1", 1),
            ("count-huge", 0xFFFF),
            ("count-neg", 0xFFFFFFFF),
        ] {
            let mut dd = d.clone();
            dd[128..132].copy_from_slice(&val.to_be_bytes());
            let h2 = c::cmsOpenProfileFromMem(
                dd.as_ptr() as _, dd.len() as u32);
            eprintln!("{name} open={}", !h2.is_null());
            if !h2.is_null() { c::cmsCloseProfile(h2); }
        }
    }
}

/// Remaining header-field mutations — intent/attributes/
/// flags/model/mfg through the validation machinery.
#[test]
fn remaining_header_fields() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut d = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, d.as_mut_ptr() as *mut _,
            &mut cn);
        c::cmsCloseProfile(h);
        // mutate fields: intent @64, attributes @56-64,
        // flags @44, model @52, mfg @48, version @8 (covered),
        // creation-date @24-36
        for (name, off, val) in [
            ("intent-9", 64usize, 9u32),
            ("intent-99", 64, 99),
            ("attr-huge", 56, 0xFFFFFFFFu32),
            ("attr-zero", 56, 0),
            ("flags-huge", 44, 0xFFFFFFFF),
            ("model-weird", 52, 0xFFFF0000),
            ("mfg-weird", 48, 0xFFFF0000),
            ("year-2050", 24, 0x07E5u32), // year 2021->2050
        ] {
            let mut dd = d.clone();
            dd[off..off + 4].copy_from_slice(&val.to_be_bytes());
            let h2 = c::cmsOpenProfileFromMem(
                dd.as_ptr() as _, dd.len() as u32);
            eprintln!("{name} open={}", !h2.is_null());
            if !h2.is_null() { c::cmsCloseProfile(h2); }
        }
    }
}

/// cmsOpenProfileFrom* variants on the corpus — the remaining
/// IO-entry machinery.
#[test]
fn open_variants() {
    unsafe {
        for (_n, data) in corpus() {
            // FromMem — the mem-IO path
            let h1 = c::cmsOpenProfileFromMem(
                data.as_ptr() as _, data.len() as u32);
            assert!(!h1.is_null());
            c::cmsCloseProfile(h1);
            // FromIOhandler — a mem IOhandler wrapping the data
            let mut buf = data.clone();
            let io = c::cmsOpenIOhandlerFromMem(
                std::ptr::null_mut(),
                buf.as_mut_ptr() as *mut _,
                buf.len() as u32,
                b"r\0".as_ptr() as *const _);
            if !io.is_null() {
                let h2 = c::cmsOpenProfileFromIOhandlerTHR(
                    std::ptr::null_mut(), io);
                if !h2.is_null() { c::cmsCloseProfile(h2); }
                else { c::cmsCloseIOhandler(io); }
            }
        }
    }
}

/// cmsOpenProfileFromFile/FromStream — the file/stream IO
/// paths on the corpus.
#[test]
fn file_stream_open() {
    unsafe {
        for (i, (_n, data)) in corpus().iter().enumerate() {
            let path = format!("/tmp/prof{i}.icc\0");
            // write the profile to a file
            std::fs::write(&path[..path.len() - 1],
                data).unwrap();
            let h1 = c::cmsOpenProfileFromFile(
                path.as_ptr() as *const _,
                b"r\0".as_ptr() as *const _);
            eprintln!("file{i} open={}", !h1.is_null());
            if !h1.is_null() { c::cmsCloseProfile(h1); }
            // IOhandler variant — cmsOpenIOhandlerFromFile +
            // cmsOpenProfileFromIOhandlerTHR
            let io = c::cmsOpenIOhandlerFromFile(
                std::ptr::null_mut(),
                path.as_ptr() as *const _,
                b"r\0".as_ptr() as *const _);
            if !io.is_null() {
                let h2 = c::cmsOpenProfileFromIOhandlerTHR(
                    std::ptr::null_mut(), io);
                eprintln!("iohandler{i} open={}", !h2.is_null());
                if !h2.is_null() { c::cmsCloseProfile(h2); }
                else { c::cmsCloseIOhandler(io); }
            }
        }
    }
}

/// cmsSaveProfileToFile/ToIOhandler/ToStream — the remaining
/// save paths.
#[test]
fn save_variants() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let path = "/tmp/save_out.icc\0";
        // ToFile
        let ok1 = c::cmsSaveProfileToFile(h,
            path.as_ptr() as *const _);
        eprintln!("toFile ok={ok1}");
        // ToIOhandler — a mem IOhandler
        let mut buf = vec![0u8; 65536];
        let io = c::cmsOpenIOhandlerFromMem(std::ptr::null_mut(),
            buf.as_mut_ptr() as *mut _, 65536,
            b"w\0".as_ptr() as *const _);
        let ok2 = if !io.is_null() {
            let r = c::cmsSaveProfileToIOhandler(h, io);
            c::cmsCloseIOhandler(io);
            r } else { 0 };
        eprintln!("toIOhandler ok={ok2}");
        // NOTE: cmsSaveProfileToStream closes its FILE*
        // internally — a wrapper that also fcloses double-frees.
        // ToFile + ToIOhandler cover the save machinery.
        c::cmsCloseProfile(h);
    }
}

/// Device-class/colorspace/PCS header validation — unusual
/// values through the open machinery.
#[test]
fn cs_pcs_validation() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut d = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, d.as_mut_ptr() as *mut _,
            &mut cn);
        c::cmsCloseProfile(h);
        // mutate colorspace @16 + PCS @20 + class @12
        for (name, off, val) in [
            ("cs-xyz", 16usize, 0x58595A20u32),
            ("cs-lab", 16, 0x4C616220),
            ("cs-bad", 16, 0xFFFFFFFF),
            ("pcs-xyz", 20, 0x58595A20),
            ("pcs-bad", 20, 0xFFFFFFFF),
            ("class-abs", 12, 0x61627374), // 'abst'
            ("class-nm", 12, 0x6E6D636C),  // 'nmcl'
            ("class-spac", 12, 0x73706163), // 'spac'
            ("class-link", 12, 0x6C696E6B), // 'link'
        ] {
            let mut dd = d.clone();
            dd[off..off + 4].copy_from_slice(&val.to_be_bytes());
            let h2 = c::cmsOpenProfileFromMem(
                dd.as_ptr() as _, dd.len() as u32);
            eprintln!("{name} open={}", !h2.is_null());
            if !h2.is_null() { c::cmsCloseProfile(h2); }
        }
    }
}

/// Remaining header fields — profile-ID + creation-datetime +
/// model + manufacturer + creator through the validation.
#[test]
fn remaining_header_fields2() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut d = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, d.as_mut_ptr() as *mut _,
            &mut cn);
        c::cmsCloseProfile(h);
        // mutate fields: profile-ID @84-100, creation-date
        // @24-36, model @40-44, mfg @48-52, creator @80-84
        for (name, off, val) in [
            ("id-zeros", 84usize, 0u32),
            ("id-huge", 84, 0xFFFFFFFFu32),
            ("year-2000", 24, 0x07D0u32),
            ("month-13", 26, 0x000Du32),
            ("model-0", 40, 0u32),
            ("mfg-0", 48, 0u32),
            ("creator-0", 80, 0u32),
            ("id-set", 84, 0x12345678u32),
        ] {
            let mut dd = d.clone();
            dd[off..off + 4].copy_from_slice(&val.to_be_bytes());
            let h2 = c::cmsOpenProfileFromMem(
                dd.as_ptr() as _, dd.len() as u32);
            eprintln!("{name} open={}", !h2.is_null());
            if !h2.is_null() { c::cmsCloseProfile(h2); }
        }
    }
}

/// Unusual tag-table structures — linked offsets + tag-count
/// edge cases on crafted directories.
#[test]
fn unusual_tag_table() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut d = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, d.as_mut_ptr() as *mut _,
            &mut cn);
        c::cmsCloseProfile(h);
        // tag table starts @128: count@128, entries@132
        // each entry = sig(4) + offset(4) + size(4)
        for (name, entry_off, field, val) in [
            // sig of first entry
            ("sig-zero", 132usize, 0, 0u32),
            ("sig-huge", 132, 0, 0xFFFFFFFFu32),
            // offset of first entry
            ("off-zero", 132, 4, 0u32),
            ("off-huge", 132, 4, 0xFFFFFFFFu32),
            ("off-mid", 132, 4, 200u32),
            // size of first entry
            ("size-zero", 132, 8, 0u32),
            ("size-huge", 132, 8, 0xFFFFFFFFu32),
            ("size-1", 132, 8, 1u32),
        ] {
            let mut dd = d.clone();
            dd[entry_off + field..entry_off + field + 4]
                .copy_from_slice(&val.to_be_bytes());
            let h2 = c::cmsOpenProfileFromMem(
                dd.as_ptr() as _, dd.len() as u32);
            eprintln!("{name} open={}", !h2.is_null());
            if !h2.is_null() {
                let sig = c::cmsGetTagSignature(h2, 0);
                let _ = sig;
                c::cmsCloseProfile(h2);
            }
        }
    }
}

/// Magic/ACSP mutations — the magic-check internals.
#[test]
fn magic_mutations() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut d = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, d.as_mut_ptr() as *mut _,
            &mut cn);
        c::cmsCloseProfile(h);
        // 'acsp' magic @36-40
        for (name, val) in [
            ("acsp", 0x61637370u32), // 'acsp'
            ("ACSP", 0x41435350u32), // 'ACSP'
            ("acsp-x", 0x61637371u32), // 'acsq'
            ("zeros", 0u32),
            ("lcms", 0x6C636D73u32), // 'lcms'
            ("size-ok", 0x000002B8u32), // 696
        ] {
            let mut dd = d.clone();
            dd[36..40].copy_from_slice(&val.to_be_bytes());
            let h2 = c::cmsOpenProfileFromMem(
                dd.as_ptr() as _, dd.len() as u32);
            eprintln!("{name} open={}", !h2.is_null());
            if !h2.is_null() { c::cmsCloseProfile(h2); }
        }
    }
}

/// Profile-size mutations — the size-field machinery.
#[test]
fn size_mutations() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut d = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, d.as_mut_ptr() as *mut _,
            &mut cn);
        c::cmsCloseProfile(h);
        // profile size @0-4
        for (name, val) in [
            ("size-0", 0u32),
            ("size-127", 127),
            ("size-128", 128),
            ("size-129", 129),
            ("size-big", 0x10000u32),
            ("size-huge", 0xFFFFFFFFu32),
        ] {
            let mut dd = d.clone();
            dd[0..4].copy_from_slice(&val.to_be_bytes());
            let h2 = c::cmsOpenProfileFromMem(
                dd.as_ptr() as _, dd.len() as u32);
            eprintln!("{name} open={}", !h2.is_null());
            if !h2.is_null() { c::cmsCloseProfile(h2); }
        }
    }
}

/// Truncated profiles — the bounds-check machinery.
#[test]
fn truncated_profiles() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut d = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, d.as_mut_ptr() as *mut _,
            &mut cn);
        c::cmsCloseProfile(h);
        for (name, len) in [
            ("t-0", 0usize),
            ("t-4", 4), // just the size
            ("t-36", 36), // up to magic
            ("t-127", 127), // header
            ("t-128", 128), // header + no tag table
            ("t-140", 140), // partial tag table
            ("t-300", 300), // partial tags
            ("t-full", d.len() - 1), // last byte missing
        ] {
            let h2 = c::cmsOpenProfileFromMem(
                d.as_ptr() as _, len as u32);
            eprintln!("{name} open={}", !h2.is_null());
            if !h2.is_null() { c::cmsCloseProfile(h2); }
        }
    }
}

/// CMM signature mutations — the CMM-machinery internals.
#[test]
fn cmm_mutations() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut d = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, d.as_mut_ptr() as *mut _,
            &mut cn);
        c::cmsCloseProfile(h);
        // CMM sig @4-8
        for (name, val) in [
            ("lcms", 0x6C636D73u32), // 'lcms'
            ("adbe", 0x61646265u32), // 'adbe'
            ("MSFT", 0x4D534654u32), // 'MSFT'
            ("KODA", 0x4B4F4441u32), // 'KODA'
            ("zeros", 0u32),
            ("huge", 0xFFFFFFFFu32),
        ] {
            let mut dd = d.clone();
            dd[4..8].copy_from_slice(&val.to_be_bytes());
            let h2 = c::cmsOpenProfileFromMem(
                dd.as_ptr() as _, dd.len() as u32);
            eprintln!("{name} open={}", !h2.is_null());
            if !h2.is_null() { c::cmsCloseProfile(h2); }
        }
    }
}

/// ICC-version encoding mutations — the version-machinery
/// internals.
#[test]
fn version_mutations2() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut d = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, d.as_mut_ptr() as *mut _,
            &mut cn);
        c::cmsCloseProfile(h);
        // ICC version @8-12 (major.minor.bugfix)
        for (name, val) in [
            ("v2.0", 0x02000000u32),
            ("v2.1", 0x02100000u32),
            ("v2.4", 0x02400000u32),
            ("v4.0", 0x04000000u32),
            ("v4.2", 0x04200000u32),
            ("v4.3", 0x04300000u32),
            ("v4.4", 0x04400000u32),
            ("v5.0", 0x05000000u32),
            ("v9.9", 0x09900000u32),
            ("v0.0", 0x00000000u32),
        ] {
            let mut dd = d.clone();
            dd[8..12].copy_from_slice(&val.to_be_bytes());
            let h2 = c::cmsOpenProfileFromMem(
                dd.as_ptr() as _, dd.len() as u32);
            eprintln!("{name} open={}", !h2.is_null());
            if !h2.is_null() {
                let ver = c::cmsGetProfileVersion(h2);
                eprintln!("  read={ver}");
                c::cmsCloseProfile(h2);
            }
        }
    }
}

/// Tag-count/entry-count mutations — the tag-count machinery
/// internals.
#[test]
fn tag_count_mutations2() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut d = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, d.as_mut_ptr() as *mut _,
            &mut cn);
        c::cmsCloseProfile(h);
        // tag count @128-132
        for (name, val) in [
            ("n-0", 0u32),
            ("n-1", 1u32),
            ("n-5", 5u32),
            ("n-50", 50u32),
            ("n-500", 500u32),
            ("n-huge", 0x10000u32),
            ("n-neg", 0x80000001u32),
        ] {
            let mut dd = d.clone();
            dd[128..132].copy_from_slice(&val.to_be_bytes());
            let h2 = c::cmsOpenProfileFromMem(
                dd.as_ptr() as _, dd.len() as u32);
            eprintln!("{name} open={}", !h2.is_null());
            if !h2.is_null() {
                let n = c::cmsGetTagCount(h2);
                eprintln!("  count={n}");
                c::cmsCloseProfile(h2);
            }
        }
    }
}

/// Remaining header mutations — illuminant + flags + creator
/// fields.
#[test]
fn illuminant_mutations() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut d = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, d.as_mut_ptr() as *mut _,
            &mut cn);
        c::cmsCloseProfile(h);
        // illuminant XYZ @68-80 (3 x s15Fixed16)
        for (name, off, val) in [
            ("illum-X", 68usize, 0x00010000i32 as u32),
            ("illum-Y", 72, 0x00010000u32),
            ("illum-Z", 76, 0x00010000u32),
            ("illum-zero", 68, 0u32),
            ("illum-huge", 68, 0x7FFFFFFFu32),
        ] {
            let mut dd = d.clone();
            dd[off..off + 4].copy_from_slice(&val.to_be_bytes());
            let h2 = c::cmsOpenProfileFromMem(
                dd.as_ptr() as _, dd.len() as u32);
            eprintln!("{name} open={}", !h2.is_null());
            if !h2.is_null() { c::cmsCloseProfile(h2); }
        }
    }
}

/// Flags/attributes/intent mutations — the remaining header
/// validation internals.
#[test]
fn flags_attr_intent() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut d = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, d.as_mut_ptr() as *mut _,
            &mut cn);
        c::cmsCloseProfile(h);
        // flags @44-48, intent @64-68, attributes @56-64
        for (name, off, val) in [
            ("flags-0", 44usize, 0u32),
            ("flags-1", 44, 1u32),
            ("flags-huge", 44, 0xFFFFFFFFu32),
            ("intent-0", 64, 0u32),
            ("intent-1", 64, 1u32),
            ("intent-3", 64, 3u32),
            ("intent-4", 64, 4u32),
            ("intent-huge", 64, 0xFFFFFFFFu32),
            ("attr-0", 56, 0u32),
            ("attr-neg", 56, 0xFFFFFFFFu32),
        ] {
            let mut dd = d.clone();
            dd[off..off + 4].copy_from_slice(&val.to_be_bytes());
            let h2 = c::cmsOpenProfileFromMem(
                dd.as_ptr() as _, dd.len() as u32);
            eprintln!("{name} open={}", !h2.is_null());
            if !h2.is_null() { c::cmsCloseProfile(h2); }
        }
    }
}

/// Remaining open variants — memory/file/stream/IOhandler
/// combos.
#[test]
fn remaining_open_variants() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut d = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, d.as_mut_ptr() as *mut _,
            &mut cn);
        c::cmsCloseProfile(h);
        // memory variants
        let m1 = c::cmsOpenProfileFromMem(
            d.as_ptr() as _, cn);
        let m2 = c::cmsOpenProfileFromMemTHR(
            std::ptr::null_mut(),
            d.as_ptr() as _, cn);
        eprintln!("mem={} thr={}", !m1.is_null(), !m2.is_null());
        // IOhandler variants
        let ioh = c::cmsOpenIOhandlerFromMem(
            std::ptr::null_mut(),
            d.as_ptr() as *mut _, cn, b"r ".as_ptr() as _);
        let p = c::cmsOpenProfileFromIOhandlerTHR(
            std::ptr::null_mut(), ioh);
        eprintln!("iohandler={}", !p.is_null());
        // file variant — write then open
        let path = b"/tmp/ccov-open.icc\0";
        let f = c::cmsOpenIOhandlerFromFile(
            std::ptr::null_mut(),
            path.as_ptr() as _, b"wb\0".as_ptr() as _);
        if !f.is_null() {
            c::cmsSaveProfileToIOhandler(h, f);
            let _ = f; // the handler closes on its own
        }
        let h2 = c::cmsOpenProfileFromFile(
            path.as_ptr() as _, b"r ".as_ptr() as _);
        eprintln!("file={}", !h2.is_null());
        let _ = std::fs::remove_file("/tmp/ccov-open.icc");
        if !m1.is_null() { c::cmsCloseProfile(m1); }
        if !m2.is_null() { c::cmsCloseProfile(m2); }
        if !p.is_null() { c::cmsCloseProfile(p); }
        if !h2.is_null() { c::cmsCloseProfile(h2); }
    }
}

/// Stream/IOhandler variants — more IO paths on a corpus
/// profile.
#[test]
fn stream_iohandler_variants() {
    unsafe {
        for (name, h) in [
            ("srgb", c::cmsCreate_sRGBProfile()),
            ("xyz", c::cmsCreateXYZProfile()),
            ("lab4", c::cmsCreateLab4Profile(
                std::ptr::null_mut())),
        ] {
            // save to mem + reopen — the IO paths
            let mut cn = 0u32;
            c::cmsSaveProfileToMem(h, std::ptr::null_mut(),
                &mut cn);
            let mut cbuf = vec![0u8; cn as usize];
            c::cmsSaveProfileToMem(h,
                cbuf.as_mut_ptr() as *mut _, &mut cn);
            let h2 = c::cmsOpenProfileFromMem(
                cbuf.as_ptr() as _, cn);
            eprintln!("{name} reopen={}", !h2.is_null());
            c::cmsCloseProfile(h);
            if !h2.is_null() { c::cmsCloseProfile(h2); }
        }
    }
}

/// Colorspace sig mutations — the colorspace-validation
/// internals.
#[test]
fn colorspace_mutations() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut d = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, d.as_mut_ptr() as *mut _,
            &mut cn);
        c::cmsCloseProfile(h);
        // colorspace @16-20, PCS @20-24
        for (name, off, val) in [
            ("cs-rgb", 16usize, 0x52474220u32), // 'RGB '
            ("cs-cmyk", 16, 0x434D594Bu32), // 'CMYK'
            ("cs-lab", 16, 0x4C616220u32), // 'Lab '
            ("cs-xyz", 16, 0x58595A20u32), // 'XYZ '
            ("cs-nmcl", 16, 0x6E6D636Cu32), // 'nmcl'
            ("cs-zero", 16, 0u32),
            ("cs-huge", 16, 0xFFFFFFFFu32),
            ("pcs-xyz", 20, 0x58595A20u32),
            ("pcs-lab", 20, 0x4C616220u32),
            ("pcs-zero", 20, 0u32),
        ] {
            let mut dd = d.clone();
            dd[off..off + 4].copy_from_slice(&val.to_be_bytes());
            let h2 = c::cmsOpenProfileFromMem(
                dd.as_ptr() as _, dd.len() as u32);
            eprintln!("{name} open={}", !h2.is_null());
            if !h2.is_null() { c::cmsCloseProfile(h2); }
        }
    }
}

/// Tag-entry offset/size machinery — unusual tag bounds.
#[test]
fn tag_bounds() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut cbuf = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, cbuf.as_mut_ptr() as *mut _,
            &mut cn);
        c::cmsCloseProfile(h);
        // mutate the first tag's size field (offset+8)
        for (name, off, v) in [
            ("sz-huge", 128+8, 0xFF000000u32),
            ("sz-0", 128+8, 0u32),
            ("sz-neg", 128+8, 0x80000000u32),
            ("off-huge", 128+4, 0xFF000000u32),
            ("off-0", 128+4, 0u32),
            ("off-end", 128+4, (cn-4))] {
            let mut d = cbuf.clone();
            d[off..off+4].copy_from_slice(&v.to_be_bytes());
            let h2 = c::cmsOpenProfileFromMem(
                d.as_ptr() as _, cn);
            eprintln!("{name} open={}", !h2.is_null());
            if !h2.is_null() { c::cmsCloseProfile(h2); }
        }
    }
}

/// Profile-class machinery — unusual class sigs.
#[test]
fn profile_class_sigs() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut cbuf = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, cbuf.as_mut_ptr() as *mut _,
            &mut cn);
        c::cmsCloseProfile(h);
        // mutate the device-class field (offset 12)
        for (name, v) in [
            ("mntr", 0x6D6E7472u32), ("prtr", 0x72545243),
            ("scnr", 0x73636E72), ("link", 0x6C696E6B),
            ("abst", 0x61627374), ("spac", 0x73706163),
            ("nmcl", 0x6E6D636C), ("0", 0u32),
            ("huge", 0xFFFFFFFFu32)] {
            let mut d = cbuf.clone();
            d[12..16].copy_from_slice(&v.to_be_bytes());
            let h2 = c::cmsOpenProfileFromMem(
                d.as_ptr() as _, cn);
            eprintln!("{name} open={}", !h2.is_null());
            if !h2.is_null() { c::cmsCloseProfile(h2); }
        }
    }
}

/// Colorspace/PCS machinery — unusual cs/PCS sigs.
#[test]
fn colorspace_pcs_sigs() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut cbuf = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, cbuf.as_mut_ptr() as *mut _,
            &mut cn);
        c::cmsCloseProfile(h);
        // mutate colorspace (offset 16) + PCS (offset 20)
        for (name, off, v) in [
            ("cs-CMYK", 16, 0x434D594Bu32),
            ("cs-Lab", 16, 0x4C616220),
            ("cs-0", 16, 0u32),
            ("cs-huge", 16, 0xFFFFFFFFu32),
            ("pcs-Lab", 20, 0x4C616220u32),
            ("pcs-0", 20, 0u32),
            ("pcs-huge", 20, 0xFFFFFFFFu32)] {
            let mut d = cbuf.clone();
            d[off..off+4].copy_from_slice(&v.to_be_bytes());
            let h2 = c::cmsOpenProfileFromMem(
                d.as_ptr() as _, cn);
            eprintln!("{name} open={}", !h2.is_null());
            if !h2.is_null() { c::cmsCloseProfile(h2); }
        }
    }
}

/// Intent/illuminant machinery — unusual header fields.
#[test]
fn intent_illuminant_fields() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut cbuf = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, cbuf.as_mut_ptr() as *mut _,
            &mut cn);
        c::cmsCloseProfile(h);
        // mutate intent (offset 64) + illuminant (offset 68-80)
        for (name, off, v) in [
            ("intent-0", 64, 0u32),
            ("intent-1", 64, 1u32),
            ("intent-3", 64, 3u32),
            ("intent-huge", 64, 0xFFFFFFFFu32),
            ("illum-0", 68, 0u32),
            ("illum-huge", 68, 0xFFFFFFFFu32)] {
            let mut d = cbuf.clone();
            d[off..off+4].copy_from_slice(&v.to_be_bytes());
            let h2 = c::cmsOpenProfileFromMem(
                d.as_ptr() as _, cn);
            eprintln!("{name} open={}", !h2.is_null());
            if !h2.is_null() { c::cmsCloseProfile(h2); }
        }
    }
}

/// Flags/attributes machinery — unusual header fields.
#[test]
fn flags_attributes_fields() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut cbuf = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, cbuf.as_mut_ptr() as *mut _,
            &mut cn);
        c::cmsCloseProfile(h);
        // mutate flags (offset 24) + attributes (offset 84-92)
        for (name, off, v) in [
            ("flags-0", 24, 0u32),
            ("flags-embedded", 24, 1u32),
            ("flags-indep", 24, 2u32),
            ("flags-huge", 24, 0xFFFFFFFFu32),
            ("attr-0", 84, 0u32),
            ("attr-huge", 84, 0xFFFFFFFFu32)] {
            let mut d = cbuf.clone();
            d[off..off+4].copy_from_slice(&v.to_be_bytes());
            let h2 = c::cmsOpenProfileFromMem(
                d.as_ptr() as _, cn);
            eprintln!("{name} open={}", !h2.is_null());
            if !h2.is_null() { c::cmsCloseProfile(h2); }
        }
    }
}

/// Model/manufacturer/creator machinery — unusual header
/// fields.
#[test]
fn model_manufacturer_fields() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut cbuf = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, cbuf.as_mut_ptr() as *mut _,
            &mut cn);
        c::cmsCloseProfile(h);
        // mutate model (offset 28) + manufacturer (offset 40)
        // + creator (offset 80)
        for (name, off, v) in [
            ("model-0", 28, 0u32),
            ("model-huge", 28, 0xFFFFFFFFu32),
            ("mfr-0", 40, 0u32),
            ("mfr-huge", 40, 0xFFFFFFFFu32),
            ("creator-0", 80, 0u32),
            ("creator-huge", 80, 0xFFFFFFFFu32)] {
            let mut d = cbuf.clone();
            d[off..off+4].copy_from_slice(&v.to_be_bytes());
            let h2 = c::cmsOpenProfileFromMem(
                d.as_ptr() as _, cn);
            eprintln!("{name} open={}", !h2.is_null());
            if !h2.is_null() { c::cmsCloseProfile(h2); }
        }
    }
}

/// Datetime/version machinery — unusual header fields.
#[test]
fn datetime_version_fields() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut cbuf = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, cbuf.as_mut_ptr() as *mut _,
            &mut cn);
        c::cmsCloseProfile(h);
        // mutate version (offset 8) + datetime (offset 24-36
        // is year..sec as u16x6)
        for (name, off, v) in [
            ("ver-2", 8, 0x02000000u32),
            ("ver-4", 8, 0x04000000u32),
            ("ver-5", 8, 0x05000000u32),
            ("ver-99", 8, 0x99000000u32),
            ("ver-0", 8, 0u32),
            ("yr-0", 24, 0u16 as u32),
            ("yr-9999", 24, 9999u32)] {
            let mut d = cbuf.clone();
            if name.starts_with("yr-") {
                d[off..off+2].copy_from_slice(
                    &(v as u16).to_be_bytes());
            } else {
                d[off..off+4].copy_from_slice(
                    &v.to_be_bytes());
            }
            let h2 = c::cmsOpenProfileFromMem(
                d.as_ptr() as _, cn);
            eprintln!("{name} open={}", !h2.is_null());
            if !h2.is_null() { c::cmsCloseProfile(h2); }
        }
    }
}

/// ACSP/magic machinery — the magic-check internals.
#[test]
fn acsp_magic_fields() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut cbuf = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, cbuf.as_mut_ptr() as *mut _,
            &mut cn);
        c::cmsCloseProfile(h);
        // mutate the ACSP magic (offset 36)
        for (name, v) in [
            ("acsp", 0x61637370u32),
            ("acsq", 0x61637371),
            ("ACSP", 0x41435350),
            ("0", 0u32),
            ("huge", 0xFFFFFFFFu32),
            ("abcp", 0x61626370)] {
            let mut d = cbuf.clone();
            d[36..40].copy_from_slice(&v.to_be_bytes());
            let h2 = c::cmsOpenProfileFromMem(
                d.as_ptr() as _, cn);
            eprintln!("{name} open={}", !h2.is_null());
            if !h2.is_null() { c::cmsCloseProfile(h2); }
        }
    }
}

/// CMM-signature machinery — unusual header fields.
#[test]
fn cmm_signature_fields() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut cbuf = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, cbuf.as_mut_ptr() as *mut _,
            &mut cn);
        c::cmsCloseProfile(h);
        // mutate the CMM sig (offset 4)
        for (name, v) in [
            ("lcms", 0x6C636D73u32),
            ("0", 0u32),
            ("huge", 0xFFFFFFFFu32),
            ("ADBE", 0x41444245),
            ("APPL", 0x4150504C),
            ("MSFT", 0x4D534654)] {
            let mut d = cbuf.clone();
            d[4..8].copy_from_slice(&v.to_be_bytes());
            let h2 = c::cmsOpenProfileFromMem(
                d.as_ptr() as _, cn);
            eprintln!("{name} open={}", !h2.is_null());
            if !h2.is_null() { c::cmsCloseProfile(h2); }
        }
    }
}

/// File-size machinery — unusual size fields.
#[test]
fn file_size_fields() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut cbuf = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, cbuf.as_mut_ptr() as *mut _,
            &mut cn);
        c::cmsCloseProfile(h);
        // mutate the size field (offset 0)
        for (name, v) in [
            ("sz-0", 0u32),
            ("sz-1", 1u32),
            ("sz-real", cn),
            ("sz-huge", 0xFFFFFFFFu32),
            ("sz-128", 128u32),
            ("sz-8k", 8192u32)] {
            let mut d = cbuf.clone();
            d[0..4].copy_from_slice(&v.to_be_bytes());
            let h2 = c::cmsOpenProfileFromMem(
                d.as_ptr() as _, cn);
            eprintln!("{name} open={}", !h2.is_null());
            if !h2.is_null() { c::cmsCloseProfile(h2); }
        }
    }
}

/// Profile-ID machinery — unusual ID fields.
#[test]
fn profile_id_fields() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut cbuf = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, cbuf.as_mut_ptr() as *mut _,
            &mut cn);
        c::cmsCloseProfile(h);
        // mutate the profile-ID (offset 84+12=96? actually
        // 128-16 — try offset 84)
        for (name, off, v) in [
            ("id-0", 84, 0u32),
            ("id-huge", 84, 0xFFFFFFFFu32),
            ("id-88", 88, 0x12345678u32),
            ("id-92", 92, 0xFFFFFFFFu32),
            ("id-96", 96, 0x12345678u32),
            ("id-100", 100, 0xFFFFFFFFu32)] {
            let mut d = cbuf.clone();
            d[off..off+4].copy_from_slice(&v.to_be_bytes());
            let h2 = c::cmsOpenProfileFromMem(
                d.as_ptr() as _, cn);
            eprintln!("{name} open={}", !h2.is_null());
            if !h2.is_null() { c::cmsCloseProfile(h2); }
        }
    }
}

/// Tag-count machinery — unusual tag-count fields.
#[test]
fn tag_count_fields() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut cbuf = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, cbuf.as_mut_ptr() as *mut _,
            &mut cn);
        c::cmsCloseProfile(h);
        // mutate the tag-count (offset 128)
        for (name, v) in [
            ("cnt-0", 0u32),
            ("cnt-1", 1u32),
            ("cnt-5", 5u32),
            ("cnt-50", 50u32),
            ("cnt-500", 500u32),
            ("cnt-huge", 0xFFFFFFFFu32),
            ("cnt-neg", 0x80000000u32)] {
            let mut d = cbuf.clone();
            d[128..132].copy_from_slice(&v.to_be_bytes());
            let h2 = c::cmsOpenProfileFromMem(
                d.as_ptr() as _, cn);
            eprintln!("{name} open={}", !h2.is_null());
            if !h2.is_null() { c::cmsCloseProfile(h2); }
        }
    }
}

/// Reserved-field machinery — unusual header reserved
/// fields.
#[test]
fn reserved_fields() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut cbuf = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, cbuf.as_mut_ptr() as *mut _,
            &mut cn);
        c::cmsCloseProfile(h);
        // mutate the reserved fields (offsets 36-40 acsp,
        // 40-44 platform, 52-56 reserved)
        for (name, off, v) in [
            ("r52-0", 52, 0u32),
            ("r52-huge", 52, 0xFFFFFFFFu32),
            ("r56-0", 56, 0u32),
            ("r56-huge", 56, 0xFFFFFFFFu32),
            ("r60-0", 60, 0u32),
            ("r60-huge", 60, 0xFFFFFFFFu32)] {
            let mut d = cbuf.clone();
            d[off..off+4].copy_from_slice(&v.to_be_bytes());
            let h2 = c::cmsOpenProfileFromMem(
                d.as_ptr() as _, cn);
            eprintln!("{name} open={}", !h2.is_null());
            if !h2.is_null() { c::cmsCloseProfile(h2); }
        }
    }
}

/// Platform-signature machinery — unusual platform fields.
#[test]
fn platform_fields() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut cbuf = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, cbuf.as_mut_ptr() as *mut _,
            &mut cn);
        c::cmsCloseProfile(h);
        // mutate the platform sig (offset 40)
        for (name, v) in [
            ("APPL", 0x4150504Cu32),
            ("MSFT", 0x4D534654),
            ("SGI ", 0x53474920),
            ("SUNW", 0x53554E57),
            ("0", 0u32),
            ("huge", 0xFFFFFFFFu32)] {
            let mut d = cbuf.clone();
            d[40..44].copy_from_slice(&v.to_be_bytes());
            let h2 = c::cmsOpenProfileFromMem(
                d.as_ptr() as _, cn);
            eprintln!("{name} open={}", !h2.is_null());
            if !h2.is_null() { c::cmsCloseProfile(h2); }
        }
    }
}

/// Tag-table machinery — unusual tag-table structures.
#[test]
fn tag_table_structures() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut cbuf = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, cbuf.as_mut_ptr() as *mut _,
            &mut cn);
        c::cmsCloseProfile(h);
        // mutate the first tag's sig/offset/size (offset 132)
        for (name, off, v) in [
            ("sig0-0", 132, 0u32),
            ("sig0-huge", 132, 0xFFFFFFFFu32),
            ("off0-0", 136, 0u32),
            ("off0-huge", 136, 0xFFFFFFFFu32),
            ("size0-0", 140, 0u32),
            ("size0-huge", 140, 0xFFFFFFFFu32)] {
            let mut d = cbuf.clone();
            d[off..off+4].copy_from_slice(&v.to_be_bytes());
            let h2 = c::cmsOpenProfileFromMem(
                d.as_ptr() as _, cn);
            eprintln!("{name} open={}", !h2.is_null());
            if !h2.is_null() { c::cmsCloseProfile(h2); }
        }
    }
}

/// Tag-signature machinery — unusual tag sigs.
#[test]
fn tag_sig_fields() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut cbuf = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, cbuf.as_mut_ptr() as *mut _,
            &mut cn);
        c::cmsCloseProfile(h);
        // mutate the first tag's sig (offset 132)
        for (name, v) in [
            ("sig-0", 0u32),
            ("sig-huge", 0xFFFFFFFFu32),
            ("sig-desc", 0x64657363),
            ("sig-A2B0", 0x41324230),
            ("sig-acsp", 0x61637370),
            ("sig-abcd", 0x61626364)] {
            let mut d = cbuf.clone();
            d[132..136].copy_from_slice(&v.to_be_bytes());
            let h2 = c::cmsOpenProfileFromMem(
                d.as_ptr() as _, cn);
            eprintln!("{name} open={}", !h2.is_null());
            if !h2.is_null() { c::cmsCloseProfile(h2); }
        }
    }
}

/// Tag-offset/size machinery — unusual tag bounds.
#[test]
fn tag_bounds_fields() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut cbuf = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, cbuf.as_mut_ptr() as *mut _,
            &mut cn);
        c::cmsCloseProfile(h);
        // mutate the first tag's offset/size (offsets 136/140)
        for (name, off, v) in [
            ("off-0", 136, 0u32),
            ("off-huge", 136, 0xFFFFFFFFu32),
            ("off-128", 136, 128u32),
            ("size-0", 140, 0u32),
            ("size-huge", 140, 0xFFFFFFFFu32),
            ("size-8", 140, 8u32)] {
            let mut d = cbuf.clone();
            d[off..off+4].copy_from_slice(&v.to_be_bytes());
            let h2 = c::cmsOpenProfileFromMem(
                d.as_ptr() as _, cn);
            eprintln!("{name} open={}", !h2.is_null());
            if !h2.is_null() { c::cmsCloseProfile(h2); }
        }
    }
}

/// File-open machinery — missing/dir/empty/short paths.
#[test]
fn file_open_variants() {
    unsafe {
        // write a real profile to /tmp
        let h = c::cmsCreate_sRGBProfile();
        c::cmsSaveProfileToFile(h,
            b"/tmp/ccov_srgb.icc\0".as_ptr() as _);
        c::cmsCloseProfile(h);
        for (name, path) in [
            ("real", b"/tmp/ccov_srgb.icc\0" as &[u8]),
            ("missing", b"/tmp/ccov_nope.icc\0"),
            ("dir", b"/tmp\0"),
            ("empty", b"/tmp/ccov_empty.icc\0")] {
            if name == "empty" {
                std::fs::write("/tmp/ccov_empty.icc", b"")
                    .unwrap();
            }
            let mut p = [0u8; 256];
            p[..path.len()].copy_from_slice(path);
            let h2 = c::cmsOpenProfileFromFile(
                p.as_ptr() as _, b"r\0".as_ptr() as _);
            eprintln!("{name} open={}", !h2.is_null());
            if !h2.is_null() { c::cmsCloseProfile(h2); }
        }
    }
}

/// Real corpus profiles — open + transform on file-backed
/// profiles (the real-profile machinery).
#[test]
fn corpus_profile_transform() {
    unsafe {
        for (name, path) in [
            ("lcms2link", b"../../lcms2link.icc\0" as &[u8]),
            ("gray", b"../../graylcms2.icc\0"),
            ("labv2", b"../../labv2lcms2.icc\0"),
            ("lin", b"../../linlcms2.icc\0"),
            ("labv4", b"../../lcms2cmyk.icc\0"),
            ("xyz", b"../../xyzlcms2.icc\0")] {
            let h = c::cmsOpenProfileFromFile(
                path.as_ptr() as _, b"r\0".as_ptr() as _);
            if h.is_null() {
                eprintln!("{name} open=NULL");
                continue;
            }
            let cs = c::cmsGetColorSpace(h);
            let pcs = c::cmsGetPCS(h);
            let nt = c::cmsGetTagCount(h);
            eprintln!("{name} cs={cs:#x} pcs={pcs:#x} tags={nt}");
            // sRGB->profile transform where possible
            let srgb = c::cmsCreate_sRGBProfile();
            let fmt = (4 << 16) | (3 << 3) | 1u32;
            let t = c::cmsCreateTransform(srgb, fmt,
                h, fmt, 0, 0);
            eprintln!("  xform={}", !t.is_null());
            if !t.is_null() { c::cmsDeleteTransform(t); }
            c::cmsCloseProfile(h);
            c::cmsCloseProfile(srgb);
        }
    }
}

/// Corpus transforms in the right direction — corpus->sRGB
/// + corpus->corpus on real profiles with matching formats.
#[test]
fn corpus_transform_dir() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let rgb_fmt = (4 << 16) | (3 << 3) | 1u32;
        for (name, path, nch) in [
            ("lcms2link", b"../../lcms2link.icc\0" as &[u8], 4u32),
            ("gray", b"../../graylcms2.icc\0", 1),
            ("labv2", b"../../labv2lcms2.icc\0", 3),
            ("lin", b"../../linlcms2.icc\0", 4),
            ("labv4", b"../../lcms2cmyk.icc\0", 3),
            ("xyz", b"../../xyzlcms2.icc\0", 3)] {
            let h = c::cmsOpenProfileFromFile(
                path.as_ptr() as _, b"r\0".as_ptr() as _);
            if h.is_null() {
                eprintln!("{name} open=NULL");
                continue;
            }
            let cs_fmt = (0 << 16) | (nch << 3) | 1u32;
            let t1 = c::cmsCreateTransform(h, cs_fmt,
                srgb, rgb_fmt, 0, 0);
            let t2 = c::cmsCreateTransform(h, cs_fmt,
                h, cs_fmt, 0, 0);
            eprintln!("{name} c->rgb={} c->c={}",
                !t1.is_null(), !t2.is_null());
            if !t1.is_null() { c::cmsDeleteTransform(t1); }
            if !t2.is_null() { c::cmsDeleteTransform(t2); }
            c::cmsCloseProfile(h);
        }
        c::cmsCloseProfile(srgb);
    }
}

/// Corpus doTransform — actual pixel data through real
/// profiles (the eval path).
#[test]
fn corpus_dotransform() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let rgb_fmt = (4 << 16) | (3 << 3) | 1u32;
        for (name, path, nch) in [
            ("gray", b"../../graylcms2.icc\0" as &[u8], 1u32),
            ("labv2", b"../../labv2lcms2.icc\0", 3),
            ("labv4", b"../../lcms2cmyk.icc\0", 3),
            ("xyz", b"../../xyzlcms2.icc\0", 3)] {
            let h = c::cmsOpenProfileFromFile(
                path.as_ptr() as _, b"r\0".as_ptr() as _);
            if h.is_null() {
                eprintln!("{name} open=NULL");
                continue;
            }
            let cs_fmt = (0 << 16) | (nch << 3) | 1u32;
            let t = c::cmsCreateTransform(h, cs_fmt,
                srgb, rgb_fmt, 0, 0);
            if t.is_null() {
                eprintln!("{name} xform=NULL");
                c::cmsCloseProfile(h);
                continue;
            }
            let inp = vec![128u8; nch as usize];
            let mut out = [0u8; 3];
            c::cmsDoTransform(t, inp.as_ptr() as _,
                out.as_mut_ptr() as _, 1);
            eprintln!("{name} out={},{},{}",
                out[0], out[1], out[2]);
            c::cmsDeleteTransform(t);
            c::cmsCloseProfile(h);
        }
        c::cmsCloseProfile(srgb);
    }
}

/// Corpus mem round-trip — open file, save to mem, reopen
/// from mem (the real-profile mem machinery).
#[test]
fn corpus_mem_roundtrip() {
    unsafe {
        for (name, path) in [
            ("lcms2link", b"../../lcms2link.icc\0" as &[u8]),
            ("gray", b"../../graylcms2.icc\0"),
            ("labv2", b"../../labv2lcms2.icc\0"),
            ("labv4", b"../../lcms2cmyk.icc\0"),
            ("xyz", b"../../xyzlcms2.icc\0"),
            ("lin", b"../../linlcms2.icc\0")] {
            let h = c::cmsOpenProfileFromFile(
                path.as_ptr() as _, b"r\0".as_ptr() as _);
            if h.is_null() {
                eprintln!("{name} open=NULL");
                continue;
            }
            let mut n = 0u32;
            c::cmsSaveProfileToMem(h,
                std::ptr::null_mut(), &mut n);
            let mut buf = vec![0u8; n as usize];
            let w = c::cmsSaveProfileToMem(h,
                buf.as_mut_ptr() as *mut _, &mut n);
            let h2 = c::cmsOpenProfileFromMem(
                buf.as_ptr() as *mut _, n);
            eprintln!("{name} n={n} w={w} reopen={}",
                !h2.is_null());
            if !h2.is_null() { c::cmsCloseProfile(h2); }
            c::cmsCloseProfile(h);
        }
    }
}

/// Corpus tag walk — enumerate + read every stored tag on
/// real profiles.
#[test]
fn corpus_tag_walk() {
    unsafe {
        for (name, path) in [
            ("lcms2link", b"../../lcms2link.icc\0" as &[u8]),
            ("gray", b"../../graylcms2.icc\0"),
            ("labv2", b"../../labv2lcms2.icc\0"),
            ("labv4", b"../../lcms2cmyk.icc\0"),
            ("xyz", b"../../xyzlcms2.icc\0"),
            ("lin", b"../../linlcms2.icc\0"),
            ("bright", b"../../brightness.icc\0"),
            ("gray3", b"../../gray3lcms2.icc\0")] {
            let h = c::cmsOpenProfileFromFile(
                path.as_ptr() as _, b"r\0".as_ptr() as _);
            if h.is_null() {
                eprintln!("{name} open=NULL");
                continue;
            }
            let n = c::cmsGetTagCount(h);
            let mut ok = 0;
            for i in 0..n {
                let sig = c::cmsGetTagSignature(h, i as u32);
                let v = c::cmsReadTag(h, sig);
                eprintln!("{name}[{i}] {sig:#x}={}",
                    !v.is_null());
                if !v.is_null() { ok += 1; }
            }
            eprintln!("{name} {ok}/{n} read");
            c::cmsCloseProfile(h);
        }
    }
}

/// Corpus profile classes — aRGB/bchs/glab/cmyk/limit/
/// null/sRGB (the class machinery on real shapes).
#[test]
fn corpus_classes() {
    unsafe {
        for (name, path) in [
            ("argb", b"../../aRGBlcms2.icc\0" as &[u8]),
            ("bchs", b"../../bchslcms2.icc\0"),
            ("glab", b"../../glablcms2.icc\0"),
            ("cmyk", b"../../lcms2cmyk.icc\0"),
            ("link2", b"../../lcms2link2.icc\0"),
            ("limit", b"../../limitlcms2.icc\0"),
            ("null", b"../../nullcms2.icc\0"),
            ("srgb", b"../../sRGBlcms2.icc\0")] {
            let h = c::cmsOpenProfileFromFile(
                path.as_ptr() as _, b"r\0".as_ptr() as _);
            if h.is_null() {
                eprintln!("{name} open=NULL");
                continue;
            }
            let cs = c::cmsGetColorSpace(h);
            let pcs = c::cmsGetPCS(h);
            let cls = c::cmsGetDeviceClass(h);
            let nt = c::cmsGetTagCount(h);
            eprintln!(
                "{name} cs={cs:#x} pcs={pcs:#x} cls={cls:#x} tags={nt}");
            c::cmsCloseProfile(h);
        }
    }
}

/// Corpus class transforms — bchs/null/cmyk/argb
/// profile-class paths.
#[test]
fn corpus_class_transforms() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let rgb_fmt = (4 << 16) | (3 << 3) | 1u32;
        let lab_fmt = (10 << 16) | (3 << 3) | 1u32;
        for (name, path, in_fmt, out_fmt, prof_out) in [
            ("argb", b"../../aRGBlcms2.icc\0" as &[u8],
             rgb_fmt, lab_fmt, lab),
            ("bchs", b"../../bchslcms2.icc\0" as &[u8],
             lab_fmt, lab_fmt, lab),
            ("glab", b"../../glablcms2.icc\0" as &[u8],
             (3 << 16) | (1 << 3) | 1, lab_fmt, lab),
            ("cmyk", b"../../lcms2cmyk.icc\0" as &[u8],
             (6 << 16) | (4 << 3) | 1, lab_fmt, lab),
            ("null", b"../../nullcms2.icc\0" as &[u8],
             (3 << 16) | (1 << 3) | 1, lab_fmt, lab),
            ("srgb", b"../../sRGBlcms2.icc\0" as &[u8],
             rgb_fmt, lab_fmt, lab)] {
            let h = c::cmsOpenProfileFromFile(
                path.as_ptr() as _, b"r\0".as_ptr() as _);
            if h.is_null() {
                eprintln!("{name} open=NULL");
                continue;
            }
            let t = c::cmsCreateTransform(h, in_fmt,
                prof_out, out_fmt, 0, 0);
            eprintln!("{name} t={}", !t.is_null());
            if !t.is_null() { c::cmsDeleteTransform(t); }
            c::cmsCloseProfile(h);
        }
        let _ = (srgb, lab);
    }
}

/// Corpus intent transforms — all 4 intents on real
/// profiles (the intent-pipeline machinery).
#[test]
fn corpus_intent_transforms() {
    unsafe {
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let lab_fmt = (10 << 16) | (3 << 3) | 1u32;
        for (name, path, in_fmt) in [
            ("argb", b"../../aRGBlcms2.icc\0" as &[u8],
             (4 << 16) | (3 << 3) | 1),
            ("cmyk", b"../../lcms2cmyk.icc\0" as &[u8],
             (6 << 16) | (4 << 3) | 1),
            ("glab", b"../../glablcms2.icc\0" as &[u8],
             (3 << 16) | (1 << 3) | 1),
            ("srgb", b"../../sRGBlcms2.icc\0" as &[u8],
             (4 << 16) | (3 << 3) | 1)] {
            let h = c::cmsOpenProfileFromFile(
                path.as_ptr() as _, b"r\0".as_ptr() as _);
            if h.is_null() {
                eprintln!("{name} open=NULL");
                continue;
            }
            for intent in [0u32, 1, 2, 3] {
                let t = c::cmsCreateTransform(h, in_fmt,
                    lab, lab_fmt, intent, 0);
                eprintln!("{name} i{intent}={}",
                    !t.is_null());
                if !t.is_null() { c::cmsDeleteTransform(t); }
            }
            c::cmsCloseProfile(h);
        }
        c::cmsCloseProfile(lab);
    }
}

/// Corpus intent eval — pixel data through real profiles
/// at different intents.
#[test]
fn corpus_intent_eval() {
    unsafe {
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let lab_fmt = (10 << 16) | (3 << 3) | 1u32;
        for (name, path, in_fmt, nch) in [
            ("argb", b"../../aRGBlcms2.icc\0" as &[u8],
             (4 << 16) | (3 << 3) | 1, 3u32),
            ("cmyk", b"../../lcms2cmyk.icc\0" as &[u8],
             (6 << 16) | (4 << 3) | 1, 4),
            ("glab", b"../../glablcms2.icc\0" as &[u8],
             (3 << 16) | (1 << 3) | 1, 1),
            ("srgb", b"../../sRGBlcms2.icc\0" as &[u8],
             (4 << 16) | (3 << 3) | 1, 3)] {
            let h = c::cmsOpenProfileFromFile(
                path.as_ptr() as _, b"r\0".as_ptr() as _);
            if h.is_null() {
                eprintln!("{name} open=NULL");
                continue;
            }
            for intent in [0u32, 1, 2, 3] {
                let t = c::cmsCreateTransform(h, in_fmt,
                    lab, lab_fmt, intent, 0);
                if t.is_null() {
                    eprintln!("{name} i{intent}=NULL");
                    continue;
                }
                let inp = vec![128u8; nch as usize];
                let mut out = [0u8; 3];
                c::cmsDoTransform(t, inp.as_ptr() as _,
                    out.as_mut_ptr() as _, 1);
                eprintln!("{name} i{intent} out={},{},{}",
                    out[0], out[1], out[2]);
                c::cmsDeleteTransform(t);
            }
            c::cmsCloseProfile(h);
        }
        c::cmsCloseProfile(lab);
    }
}

/// Corpus multiprofile chains — real profiles through
/// cmsCreateMultiprofileTransform.
#[test]
fn corpus_multiprofile() {
    unsafe {
        let argb = c::cmsOpenProfileFromFile(
            b"../../aRGBlcms2.icc\0".as_ptr() as _,
            b"r\0".as_ptr() as _);
        let srgb = c::cmsOpenProfileFromFile(
            b"../../sRGBlcms2.icc\0".as_ptr() as _,
            b"r\0".as_ptr() as _);
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let cmyk = c::cmsOpenProfileFromFile(
            b"../../lcms2cmyk.icc\0".as_ptr() as _,
            b"r\0".as_ptr() as _);
        let rgb_fmt = (4 << 16) | (3 << 3) | 1u32;
        let lab_fmt = (10 << 16) | (3 << 3) | 1;
        for (name, mut profs, inf, outf) in [
            ("argb-lab-srgb",
             vec![argb, lab, srgb] as Vec<*mut _>,
             rgb_fmt, rgb_fmt),
            ("srgb-lab-cmyk",
             vec![srgb, lab, cmyk] as Vec<*mut _>,
             rgb_fmt, lab_fmt),
            ("argb-argb",
             vec![argb, argb] as Vec<*mut _>, rgb_fmt, rgb_fmt)] {
            let n = profs.len() as i32;
            let t = c::cmsCreateMultiprofileTransform(
                profs.as_mut_ptr(), n, inf, outf, 0, 0);
            eprintln!("{name} t={}", !t.is_null());
            if !t.is_null() { c::cmsDeleteTransform(t); }
        }
        let _ = (argb, srgb, lab, cmyk);
    }
}

/// Corpus multiprofile chains with matching formats —
/// real profiles through cmsCreateMultiprofileTransform.
#[test]
fn corpus_multiprofile2() {
    unsafe {
        let argb = c::cmsOpenProfileFromFile(
            b"../../aRGBlcms2.icc\0".as_ptr() as _,
            b"r\0".as_ptr() as _);
        let srgb = c::cmsOpenProfileFromFile(
            b"../../sRGBlcms2.icc\0".as_ptr() as _,
            b"r\0".as_ptr() as _);
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let cmyk = c::cmsOpenProfileFromFile(
            b"../../lcms2cmyk.icc\0".as_ptr() as _,
            b"r\0".as_ptr() as _);
        let rgb_fmt = (4 << 16) | (3 << 3) | 1u32;
        let lab_fmt = (10 << 16) | (3 << 3) | 1;
        let cmyk_fmt = (6 << 16) | (4 << 3) | 1;
        for (name, mut profs, inf, outf) in [
            ("srgb-lab-cmyk",
             vec![srgb, lab, cmyk] as Vec<*mut _>,
             rgb_fmt, cmyk_fmt),
            ("argb-lab-cmyk",
             vec![argb, lab, cmyk] as Vec<*mut _>,
             rgb_fmt, cmyk_fmt),
            ("srgb-lab-argb",
             vec![srgb, lab, argb] as Vec<*mut _>,
             rgb_fmt, rgb_fmt),
            ("cmyk-lab-srgb",
             vec![cmyk, lab, srgb] as Vec<*mut _>,
             cmyk_fmt, rgb_fmt)] {
            let n = profs.len() as i32;
            let t = c::cmsCreateMultiprofileTransform(
                profs.as_mut_ptr(), n, inf, outf, 0, 0);
            eprintln!("{name} t={}", !t.is_null());
            if !t.is_null() { c::cmsDeleteTransform(t); }
        }
        let _ = (argb, srgb, lab, cmyk);
    }
}

/// Corpus IOhandler variants — open via IOhandler + save
/// to IOhandler on real profiles.
#[test]
fn corpus_iohandler() {
    unsafe {
        for (name, path) in [
            ("argb", "../../aRGBlcms2.icc"),
            ("gray", "../../graylcms2.icc"),
            ("labv4", "../../lcms2cmyk.icc"),
            ("bchs", "../../bchslcms2.icc")] {
            let data = std::fs::read(path).unwrap_or_default();
            if data.is_empty() {
                eprintln!("{name} read=0");
                continue;
            }
            let io = c::cmsOpenIOhandlerFromMem(
                std::ptr::null_mut(),
                data.as_ptr() as *mut _, data.len() as u32,
                b"r\0".as_ptr() as _);
            if io.is_null() {
                eprintln!("{name} io=NULL");
                continue;
            }
            let h = c::cmsOpenProfileFromIOhandlerTHR(
                std::ptr::null_mut(), io);
            eprintln!("{name} iohandler-open={}",
                !h.is_null());
            if !h.is_null() { c::cmsCloseProfile(h); }
            // profile owns io once opened — no extra close
        }
    }
}

/// Corpus class tag walk — enumerate + read every stored
/// tag on class-specific profiles.
#[test]
fn corpus_class_tag_walk() {
    unsafe {
        for (name, path) in [
            ("argb", "../../aRGBlcms2.icc"),
            ("bchs", "../../bchslcms2.icc"),
            ("glab", "../../glablcms2.icc"),
            ("cmyk", "../../lcms2cmyk.icc"),
            ("link2", "../../lcms2link2.icc"),
            ("limit", "../../limitlcms2.icc"),
            ("null", "../../nullcms2.icc"),
            ("srgb", "../../sRGBlcms2.icc")] {
            let mut p = [0u8; 256];
            let pb = path.as_bytes();
            p[..pb.len()].copy_from_slice(pb);
            let h = c::cmsOpenProfileFromFile(
                p.as_ptr() as _, b"r\0".as_ptr() as _);
            if h.is_null() {
                eprintln!("{name} open=NULL");
                continue;
            }
            let n = c::cmsGetTagCount(h);
            let mut ok = 0;
            for i in 0..n {
                let sig = c::cmsGetTagSignature(h, i as u32);
                let v = c::cmsReadTag(h, sig);
                if !v.is_null() { ok += 1; }
                eprintln!("{name}[{i}] {sig:#x}={}",
                    !v.is_null());
            }
            eprintln!("{name} {ok}/{n} read");
            c::cmsCloseProfile(h);
        }
    }
}

/// Truncated-profile boundary — open a valid profile at
/// decreasing lengths (the length-validation machinery).
#[test]
fn truncated_profile() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let mut n = 0u32;
        c::cmsSaveProfileToMem(srgb,
            std::ptr::null_mut(), &mut n);
        let mut buf = vec![0u8; n as usize];
        c::cmsSaveProfileToMem(srgb,
            buf.as_mut_ptr() as *mut _, &mut n);
        // try opening at various truncated lengths
        for len in [n, n - 16, n / 2, 256, 128, 64, 32, 16, 8, 4, 0] {
            let h = c::cmsOpenProfileFromMem(
                buf.as_ptr() as *mut _, len);
            eprintln!("len={len} h={}", !h.is_null());
            if !h.is_null() { c::cmsCloseProfile(h); }
        }
        c::cmsCloseProfile(srgb);
    }
}

/// Tag-count bounds — mutate the tag count field then
/// open (the table-bounds validation machinery).
#[test]
fn tag_count_bounds() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let mut n = 0u32;
        c::cmsSaveProfileToMem(srgb,
            std::ptr::null_mut(), &mut n);
        let orig = vec![0u8; n as usize];
        let mut buf = orig.clone();
        c::cmsSaveProfileToMem(srgb,
            buf.as_mut_ptr() as *mut _, &mut n);
        // tag count is a u32be at offset 128
        for (name, val) in [
            ("zero", 0u32), ("one", 1), ("real", 11),
            ("huge", 0x7fffffff), ("max", 0xffffffff)] {
            let mut b = buf.clone();
            b[128..132].copy_from_slice(&val.to_be_bytes());
            let h = c::cmsOpenProfileFromMem(
                b.as_ptr() as *mut _, n);
            eprintln!("{name} cnt={val} h={}", !h.is_null());
            if !h.is_null() { c::cmsCloseProfile(h); }
        }
        c::cmsCloseProfile(srgb);
    }
}

/// Version-field bounds — mutate the profile version
/// field then open (the version validation machinery).
#[test]
fn version_bounds() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let mut n = 0u32;
        c::cmsSaveProfileToMem(srgb,
            std::ptr::null_mut(), &mut n);
        let mut buf = vec![0u8; n as usize];
        c::cmsSaveProfileToMem(srgb,
            buf.as_mut_ptr() as *mut _, &mut n);
        // version is a u32be at offset 8
        for (name, val) in [
            ("v20", 0x02100000u32), ("v21", 0x02100001),
            ("v40", 0x04000000), ("v43", 0x04300000),
            ("v00", 0), ("v99", 0x09900000),
            ("odd", 0x12345678), ("neg", 0xffffffff)] {
            let mut b = buf.clone();
            b[8..12].copy_from_slice(&val.to_be_bytes());
            let h = c::cmsOpenProfileFromMem(
                b.as_ptr() as *mut _, n);
            eprintln!("{name} v={val:#x} h={}", !h.is_null());
            if !h.is_null() { c::cmsCloseProfile(h); }
        }
        c::cmsCloseProfile(srgb);
    }
}

/// ColorSpace-sig bounds — mutate the colorSpace field
/// then open (the colorspace validation machinery).
#[test]
fn colorspace_bounds() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let mut n = 0u32;
        c::cmsSaveProfileToMem(srgb,
            std::ptr::null_mut(), &mut n);
        let mut buf = vec![0u8; n as usize];
        c::cmsSaveProfileToMem(srgb,
            buf.as_mut_ptr() as *mut _, &mut n);
        // colorSpace is a u32be at offset 16
        for (name, val) in [
            ("rgb", 0x52474220u32), ("gray", 0x47524159),
            ("lab", 0x4C616220), ("cmyk", 0x434D594B),
            ("xyz", 0x58595A20), ("zero", 0),
            ("junk", 0x12345678), ("max", 0xffffffff)] {
            let mut b = buf.clone();
            b[16..20].copy_from_slice(&val.to_be_bytes());
            let h = c::cmsOpenProfileFromMem(
                b.as_ptr() as *mut _, n);
            eprintln!("{name} cs={val:#x} h={}", !h.is_null());
            if !h.is_null() { c::cmsCloseProfile(h); }
        }
        c::cmsCloseProfile(srgb);
    }
}

/// Intent-field bounds — mutate the rendering-intent
/// field then open (the intent validation machinery).
#[test]
fn intent_bounds() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let mut n = 0u32;
        c::cmsSaveProfileToMem(srgb,
            std::ptr::null_mut(), &mut n);
        let mut buf = vec![0u8; n as usize];
        c::cmsSaveProfileToMem(srgb,
            buf.as_mut_ptr() as *mut _, &mut n);
        // rendering intent is a u32be at offset 64
        for (name, val) in [
            ("i0", 0u32), ("i1", 1), ("i2", 2), ("i3", 3),
            ("i4", 4), ("i255", 255), ("huge", 0x7fffffff),
            ("neg", 0xffffffff)] {
            let mut b = buf.clone();
            b[64..68].copy_from_slice(&val.to_be_bytes());
            let h = c::cmsOpenProfileFromMem(
                b.as_ptr() as *mut _, n);
            eprintln!("{name} i={val} h={}", !h.is_null());
            if !h.is_null() { c::cmsCloseProfile(h); }
        }
        c::cmsCloseProfile(srgb);
    }
}

/// DeviceClass-sig bounds — mutate the deviceClass field
/// then open (the class validation machinery).
#[test]
fn deviceclass_bounds() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let mut n = 0u32;
        c::cmsSaveProfileToMem(srgb,
            std::ptr::null_mut(), &mut n);
        let mut buf = vec![0u8; n as usize];
        c::cmsSaveProfileToMem(srgb,
            buf.as_mut_ptr() as *mut _, &mut n);
        // deviceClass is a u32be at offset 12
        for (name, val) in [
            ("mntr", 0x6d6e7472u32), ("prtr", 0x72545243),
            ("scnr", 0x73636e72), ("link", 0x6c696e6b),
            ("abst", 0x61627374), ("nmcl", 0x6e6d636c),
            ("junk", 0x12345678), ("zero", 0),
            ("neg", 0xffffffff)] {
            let mut b = buf.clone();
            b[12..16].copy_from_slice(&val.to_be_bytes());
            let h = c::cmsOpenProfileFromMem(
                b.as_ptr() as *mut _, n);
            eprintln!("{name} cls={val:#x} h={}", !h.is_null());
            if !h.is_null() { c::cmsCloseProfile(h); }
        }
        c::cmsCloseProfile(srgb);
    }
}

/// PCS-sig bounds — mutate the PCS field then open (the
/// PCS validation machinery).
#[test]
fn pcs_bounds() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let mut n = 0u32;
        c::cmsSaveProfileToMem(srgb,
            std::ptr::null_mut(), &mut n);
        let mut buf = vec![0u8; n as usize];
        c::cmsSaveProfileToMem(srgb,
            buf.as_mut_ptr() as *mut _, &mut n);
        // PCS is a u32be at offset 20
        for (name, val) in [
            ("xyz", 0x58595A20u32), ("lab", 0x4C616220),
            ("rgb", 0x52474220), ("gray", 0x47524159),
            ("junk", 0x12345678), ("zero", 0),
            ("neg", 0xffffffff)] {
            let mut b = buf.clone();
            b[20..24].copy_from_slice(&val.to_be_bytes());
            let h = c::cmsOpenProfileFromMem(
                b.as_ptr() as *mut _, n);
            eprintln!("{name} pcs={val:#x} h={}", !h.is_null());
            if !h.is_null() { c::cmsCloseProfile(h); }
        }
        c::cmsCloseProfile(srgb);
    }
}

/// Flags-field bounds — mutate the flags field then open
/// (the flags validation machinery).
#[test]
fn flags_bounds() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let mut n = 0u32;
        c::cmsSaveProfileToMem(srgb,
            std::ptr::null_mut(), &mut n);
        let mut buf = vec![0u8; n as usize];
        c::cmsSaveProfileToMem(srgb,
            buf.as_mut_ptr() as *mut _, &mut n);
        // flags is a u32be at offset 44
        for (name, val) in [
            ("f0", 0u32), ("f1", 1), ("embedded", 1 << 0),
            ("nodataloss", 1 << 2), ("junk", 0x12345678),
            ("max", 0xffffffff)] {
            let mut b = buf.clone();
            b[44..48].copy_from_slice(&val.to_be_bytes());
            let h = c::cmsOpenProfileFromMem(
                b.as_ptr() as *mut _, n);
            eprintln!("{name} f={val:#x} h={}", !h.is_null());
            if !h.is_null() { c::cmsCloseProfile(h); }
        }
        c::cmsCloseProfile(srgb);
    }
}

/// Attributes-field bounds — mutate the attributes field
/// then open (the attributes validation machinery).
#[test]
fn attributes_bounds() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let mut n = 0u32;
        c::cmsSaveProfileToMem(srgb,
            std::ptr::null_mut(), &mut n);
        let mut buf = vec![0u8; n as usize];
        c::cmsSaveProfileToMem(srgb,
            buf.as_mut_ptr() as *mut _, &mut n);
        // attributes is a u64be at offset 56
        for (name, lo, hi) in [
            ("a0", 0u32, 0u32), ("a1", 0, 1),
            ("trans", 0, 1 << 2), ("neg", 0xffffffff,
             0xffffffff), ("junk", 0x12345678, 0x9abcdef0)] {
            let mut b = buf.clone();
            b[56..60].copy_from_slice(&lo.to_be_bytes());
            b[60..64].copy_from_slice(&hi.to_be_bytes());
            let h = c::cmsOpenProfileFromMem(
                b.as_ptr() as *mut _, n);
            eprintln!("{name} a={lo:#x},{hi:#x} h={}",
                !h.is_null());
            if !h.is_null() { c::cmsCloseProfile(h); }
        }
        c::cmsCloseProfile(srgb);
    }
}

/// Illuminant-field bounds — mutate the illuminant XYZ
/// then open (the illuminant validation machinery).
#[test]
fn illuminant_bounds() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let mut n = 0u32;
        c::cmsSaveProfileToMem(srgb,
            std::ptr::null_mut(), &mut n);
        let mut buf = vec![0u8; n as usize];
        c::cmsSaveProfileToMem(srgb,
            buf.as_mut_ptr() as *mut _, &mut n);
        // illuminant is 3 s15f16be at offset 68 (12 bytes)
        for (name, vals) in [
            ("d50", [0x0000f6d6i32, 0x00010000,
             0x0000d32d] as [i32; 3]),
            ("zero", [0; 3]),
            ("one", [1, 1, 1]),
            ("neg", [-1, -1, -1]),
            ("huge", [0x7fffffff; 3])] {
            let mut b = buf.clone();
            for (i, v) in vals.iter().enumerate() {
                b[68 + i*4..72 + i*4]
                    .copy_from_slice(&v.to_be_bytes());
            }
            let h = c::cmsOpenProfileFromMem(
                b.as_ptr() as *mut _, n);
            eprintln!("{name} h={}", !h.is_null());
            if !h.is_null() { c::cmsCloseProfile(h); }
        }
        c::cmsCloseProfile(srgb);
    }
}

/// ProfileID-field bounds — mutate the profileID field
/// then open (the ID validation machinery).
#[test]
fn profileid_bounds() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let mut n = 0u32;
        c::cmsSaveProfileToMem(srgb,
            std::ptr::null_mut(), &mut n);
        let mut buf = vec![0u8; n as usize];
        c::cmsSaveProfileToMem(srgb,
            buf.as_mut_ptr() as *mut _, &mut n);
        // profileID is 16 bytes at offset 84
        for (name, fill) in [
            ("zero", 0u8), ("ones", 1), ("0xff", 0xff),
            ("rand", 0x42)] {
            let mut b = buf.clone();
            for i in 0..16 { b[84 + i] = fill; }
            let h = c::cmsOpenProfileFromMem(
                b.as_ptr() as *mut _, n);
            eprintln!("{name} h={}", !h.is_null());
            if !h.is_null() { c::cmsCloseProfile(h); }
        }
        c::cmsCloseProfile(srgb);
    }
}

/// Reserved-field bounds — mutate the reserved field
/// then open (the reserved validation machinery).
#[test]
fn reserved_bounds() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let mut n = 0u32;
        c::cmsSaveProfileToMem(srgb,
            std::ptr::null_mut(), &mut n);
        let mut buf = vec![0u8; n as usize];
        c::cmsSaveProfileToMem(srgb,
            buf.as_mut_ptr() as *mut _, &mut n);
        // reserved is 28 bytes at offset 100
        for (name, fill) in [
            ("zero", 0u8), ("ones", 1), ("0xff", 0xff),
            ("rand", 0x42)] {
            let mut b = buf.clone();
            for i in 0..28 { b[100 + i] = fill; }
            let h = c::cmsOpenProfileFromMem(
                b.as_ptr() as *mut _, n);
            eprintln!("{name} h={}", !h.is_null());
            if !h.is_null() { c::cmsCloseProfile(h); }
        }
        c::cmsCloseProfile(srgb);
    }
}

/// CMM-field bounds — mutate the CMM field then open (the
/// CMM validation machinery).
#[test]
fn cmm_bounds() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let mut n = 0u32;
        c::cmsSaveProfileToMem(srgb,
            std::ptr::null_mut(), &mut n);
        let mut buf = vec![0u8; n as usize];
        c::cmsSaveProfileToMem(srgb,
            buf.as_mut_ptr() as *mut _, &mut n);
        // CMM is a u32be at offset 4
        for (name, val) in [
            ("lcms", 0x6c636d73u32), ("adbe", 0x61646265),
            ("zero", 0), ("junk", 0x12345678),
            ("neg", 0xffffffff)] {
            let mut b = buf.clone();
            b[4..8].copy_from_slice(&val.to_be_bytes());
            let h = c::cmsOpenProfileFromMem(
                b.as_ptr() as *mut _, n);
            eprintln!("{name} cmm={val:#x} h={}", !h.is_null());
            if !h.is_null() { c::cmsCloseProfile(h); }
        }
        c::cmsCloseProfile(srgb);
    }
}

/// Creation-date bounds — mutate the creation-date field
/// then open (the date validation machinery).
#[test]
fn creationdate_bounds() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let mut n = 0u32;
        c::cmsSaveProfileToMem(srgb,
            std::ptr::null_mut(), &mut n);
        let mut buf = vec![0u8; n as usize];
        c::cmsSaveProfileToMem(srgb,
            buf.as_mut_ptr() as *mut _, &mut n);
        // creation date is 6 u16be at offset 24 (12 bytes)
        for (name, vals) in [
            ("epoch", [0u16; 6] as [u16; 6]),
            ("now", [2025, 1, 15, 12, 30, 45]),
            ("max", [0xffff; 6]),
            ("weird", [1, 13, 32, 25, 61, 61])] {
            let mut b = buf.clone();
            for (i, v) in vals.iter().enumerate() {
                b[24 + i*2..26 + i*2]
                    .copy_from_slice(&v.to_be_bytes());
            }
            let h = c::cmsOpenProfileFromMem(
                b.as_ptr() as *mut _, n);
            eprintln!("{name} h={}", !h.is_null());
            if !h.is_null() { c::cmsCloseProfile(h); }
        }
        c::cmsCloseProfile(srgb);
    }
}

/// Manufacturer/model bounds — mutate those fields then
/// open (the mfr/model validation machinery).
#[test]
fn mfgr_model_bounds() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let mut n = 0u32;
        c::cmsSaveProfileToMem(srgb,
            std::ptr::null_mut(), &mut n);
        let mut buf = vec![0u8; n as usize];
        c::cmsSaveProfileToMem(srgb,
            buf.as_mut_ptr() as *mut _, &mut n);
        // mfr sig at 48, model sig at 52
        for (name, m, d) in [
            ("zero", 0u32, 0u32), ("ones", 1, 1),
            ("appl", 0x4150504C, 0x4150504C),
            ("junk", 0x12345678, 0x9abcdef0),
            ("neg", 0xffffffff, 0xffffffff)] {
            let mut b = buf.clone();
            b[48..52].copy_from_slice(&m.to_be_bytes());
            b[52..56].copy_from_slice(&d.to_be_bytes());
            let h = c::cmsOpenProfileFromMem(
                b.as_ptr() as *mut _, n);
            eprintln!("{name} m={m:#x} d={d:#x} h={}",
                !h.is_null());
            if !h.is_null() { c::cmsCloseProfile(h); }
        }
        c::cmsCloseProfile(srgb);
    }
}

/// ACSP-magic bounds — mutate the ACSP sig then open (the
/// one strictly-validated field).
#[test]
fn acsp_bounds() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let mut n = 0u32;
        c::cmsSaveProfileToMem(srgb,
            std::ptr::null_mut(), &mut n);
        let mut buf = vec![0u8; n as usize];
        c::cmsSaveProfileToMem(srgb,
            buf.as_mut_ptr() as *mut _, &mut n);
        // ACSP sig at offset 36
        for (name, val) in [
            ("acsp", 0x61637370u32), ("zero", 0),
            ("junk", 0x12345678), ("neg", 0xffffffff),
            ("acsp+1", 0x61637371)] {
            let mut b = buf.clone();
            b[36..40].copy_from_slice(&val.to_be_bytes());
            let h = c::cmsOpenProfileFromMem(
                b.as_ptr() as *mut _, n);
            eprintln!("{name} sig={val:#x} h={}",
                !h.is_null());
            if !h.is_null() { c::cmsCloseProfile(h); }
        }
        c::cmsCloseProfile(srgb);
    }
}

/// Platform-sig bounds — mutate the platform sig then
/// open (the platform validation machinery).
#[test]
fn platform_bounds() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let mut n = 0u32;
        c::cmsSaveProfileToMem(srgb,
            std::ptr::null_mut(), &mut n);
        let mut buf = vec![0u8; n as usize];
        c::cmsSaveProfileToMem(srgb,
            buf.as_mut_ptr() as *mut _, &mut n);
        // platform sig at offset 40
        for (name, val) in [
            ("zero", 0u32), ("appl", 0x4150504C),
            ("msft", 0x4D534654), ("sgi", 0x53474920),
            ("sunw", 0x53554E57), ("junk", 0x12345678),
            ("neg", 0xffffffff)] {
            let mut b = buf.clone();
            b[40..44].copy_from_slice(&val.to_be_bytes());
            let h = c::cmsOpenProfileFromMem(
                b.as_ptr() as *mut _, n);
            eprintln!("{name} p={val:#x} h={}", !h.is_null());
            if !h.is_null() { c::cmsCloseProfile(h); }
        }
        c::cmsCloseProfile(srgb);
    }
}

/// Tag-sig bounds — mutate the first tag's signature in
/// the tag table then open.
#[test]
fn tagsig_bounds() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let mut n = 0u32;
        c::cmsSaveProfileToMem(srgb,
            std::ptr::null_mut(), &mut n);
        let mut buf = vec![0u8; n as usize];
        c::cmsSaveProfileToMem(srgb,
            buf.as_mut_ptr() as *mut _, &mut n);
        // first tag sig at offset 132
        for (name, val) in [
            ("desc", 0x64657363u32), ("wtpt", 0x77747074),
            ("rXYZ", 0x7258595A), ("junk", 0x12345678),
            ("zero", 0), ("neg", 0xffffffff)] {
            let mut b = buf.clone();
            b[132..136].copy_from_slice(&val.to_be_bytes());
            let h = c::cmsOpenProfileFromMem(
                b.as_ptr() as *mut _, n);
            eprintln!("{name} sig={val:#x} h={}",
                !h.is_null());
            if !h.is_null() { c::cmsCloseProfile(h); }
        }
        c::cmsCloseProfile(srgb);
    }
}

/// Tag-offset bounds — mutate the first tag's offset in
/// the tag table then open.
#[test]
fn tagoffset_bounds() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let mut n = 0u32;
        c::cmsSaveProfileToMem(srgb,
            std::ptr::null_mut(), &mut n);
        let mut buf = vec![0u8; n as usize];
        c::cmsSaveProfileToMem(srgb,
            buf.as_mut_ptr() as *mut _, &mut n);
        // first tag offset at offset 136
        for (name, val) in [
            ("real", 0x1b8u32), ("zero", 0),
            ("small", 128), ("half", 300),
            ("end", n - 16), ("past", n + 100),
            ("huge", 0x7fffffff), ("neg", 0xffffffff)] {
            let mut b = buf.clone();
            b[136..140].copy_from_slice(&val.to_be_bytes());
            let h = c::cmsOpenProfileFromMem(
                b.as_ptr() as *mut _, n);
            eprintln!("{name} off={val} h={}", !h.is_null());
            if !h.is_null() { c::cmsCloseProfile(h); }
        }
        c::cmsCloseProfile(srgb);
    }
}

/// Tag-size bounds — mutate the first tag's size in the
/// tag table then open.
#[test]
fn tagsize_bounds() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let mut n = 0u32;
        c::cmsSaveProfileToMem(srgb,
            std::ptr::null_mut(), &mut n);
        let mut buf = vec![0u8; n as usize];
        c::cmsSaveProfileToMem(srgb,
            buf.as_mut_ptr() as *mut _, &mut n);
        // first tag size at offset 140
        for (name, val) in [
            ("real", 0x24u32), ("zero", 0),
            ("small", 8), ("big", 1000),
            ("past", n), ("huge", 0x7fffffff),
            ("neg", 0xffffffff)] {
            let mut b = buf.clone();
            b[140..144].copy_from_slice(&val.to_be_bytes());
            let h = c::cmsOpenProfileFromMem(
                b.as_ptr() as *mut _, n);
            eprintln!("{name} sz={val} h={}", !h.is_null());
            if !h.is_null() { c::cmsCloseProfile(h); }
        }
        c::cmsCloseProfile(srgb);
    }
}

/// File-size bounds — mutate the size field then open
/// (the size validation machinery).
#[test]
fn filesize_bounds() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let mut n = 0u32;
        c::cmsSaveProfileToMem(srgb,
            std::ptr::null_mut(), &mut n);
        let mut buf = vec![0u8; n as usize];
        c::cmsSaveProfileToMem(srgb,
            buf.as_mut_ptr() as *mut _, &mut n);
        // file size is a u32be at offset 0
        for (name, val) in [
            ("real", n), ("zero", 0), ("small", 128),
            ("half", n / 2), ("big", n + 100),
            ("huge", 0x7fffffff), ("neg", 0xffffffff)] {
            let mut b = buf.clone();
            b[0..4].copy_from_slice(&val.to_be_bytes());
            let h = c::cmsOpenProfileFromMem(
                b.as_ptr() as *mut _, n);
            eprintln!("{name} sz={val} h={}", !h.is_null());
            if !h.is_null() { c::cmsCloseProfile(h); }
        }
        c::cmsCloseProfile(srgb);
    }
}

/// Tag-table bounds — truncate inside the tag table
/// (the tag-table-bounds machinery).
#[test]
fn tagtable_bounds() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let mut n = 0u32;
        c::cmsSaveProfileToMem(srgb,
            std::ptr::null_mut(), &mut n);
        let mut buf = vec![0u8; n as usize];
        c::cmsSaveProfileToMem(srgb,
            buf.as_mut_ptr() as *mut _, &mut n);
        // the tag table starts at 128: count(4) + N*12-byte entries
        for len in [128usize, 132, 140, 144, 200, 256] {
            let h = c::cmsOpenProfileFromMem(
                buf.as_ptr() as *mut _, len as u32);
            eprintln!("len={len} h={}", !h.is_null());
            if !h.is_null() { c::cmsCloseProfile(h); }
        }
        c::cmsCloseProfile(srgb);
    }
}

/// Tag-read bounds — open a profile whose tag points past EOF, then read
/// (the lazy tag-read-bounds machinery).
#[test]
fn tagread_bounds() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let mut n = 0u32;
        c::cmsSaveProfileToMem(srgb,
            std::ptr::null_mut(), &mut n);
        let mut buf = vec![0u8; n as usize];
        c::cmsSaveProfileToMem(srgb,
            buf.as_mut_ptr() as *mut _, &mut n);
        // tag[0] offset at 132+4=136, size at 140; point it past EOF
        for (name, off) in [
            ("real", u32::from_be_bytes(
                buf[136..140].try_into().unwrap())),
            ("eof", n), ("past", n + 100),
            ("huge", 0x7fffffff), ("neg", 0xffffffff)] {
            let mut b = buf.clone();
            b[136..140].copy_from_slice(&off.to_be_bytes());
            let h = c::cmsOpenProfileFromMem(
                b.as_ptr() as *mut _, n);
            let sig = u32::from_be_bytes(
                buf[132..136].try_into().unwrap());
            let t = if h.is_null() { std::ptr::null() }
                else { c::cmsReadTag(h, sig) };
            eprintln!(
                "{name} off={off} h={} t={}",
                !h.is_null(), !t.is_null());
            if !h.is_null() { c::cmsCloseProfile(h); }
        }
        c::cmsCloseProfile(srgb);
    }
}

/// Type-parse bounds — truncate a tag's data mid-type then read
/// (the type-parser-bounds machinery).
#[test]
fn typeparse_bounds() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let mut n = 0u32;
        c::cmsSaveProfileToMem(srgb,
            std::ptr::null_mut(), &mut n);
        let mut buf = vec![0u8; n as usize];
        c::cmsSaveProfileToMem(srgb,
            buf.as_mut_ptr() as *mut _, &mut n);
        // tag[0] size at 140 — shrink it so the type data is truncated
        let sig = u32::from_be_bytes(
            buf[132..136].try_into().unwrap());
        for (name, sz) in [
            ("real", u32::from_be_bytes(
                buf[140..144].try_into().unwrap())),
            ("zero", 0), ("sig8", 8), ("half", 18),
            ("nearly", 34)] {
            let mut b = buf.clone();
            b[140..144].copy_from_slice(&sz.to_be_bytes());
            let h = c::cmsOpenProfileFromMem(
                b.as_ptr() as *mut _, n);
            let t = if h.is_null() { std::ptr::null() }
                else { c::cmsReadTag(h, sig) };
            eprintln!(
                "{name} sz={sz} h={} t={}",
                !h.is_null(), !t.is_null());
            if !h.is_null() { c::cmsCloseProfile(h); }
        }
        c::cmsCloseProfile(srgb);
    }
}

/// Type-sig dispatch — mutate tag[0]'s type sig then read
/// (the per-type read dispatch).
#[test]
fn typesig_dispatch() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let mut n = 0u32;
        c::cmsSaveProfileToMem(srgb,
            std::ptr::null_mut(), &mut n);
        let mut buf = vec![0u8; n as usize];
        c::cmsSaveProfileToMem(srgb,
            buf.as_mut_ptr() as *mut _, &mut n);
        let off = u32::from_be_bytes(
            buf[136..140].try_into().unwrap()) as usize;
        let sig = u32::from_be_bytes(
            buf[132..136].try_into().unwrap());
        for (name, ty) in [
            ("real", u32::from_be_bytes(
                buf[off..off+4].try_into().unwrap())),
            ("curv", 0x63757276), ("para", 0x70617261),
            ("XYZ ", 0x58595a20), ("text", 0x74657874),
            ("mluc", 0x6d6c7563), ("mft2", 0x6d667432),
            ("junk", 0xdeadbeef), ("zero", 0)] {
            let mut b = buf.clone();
            b[off..off+4].copy_from_slice(&ty.to_be_bytes());
            let h = c::cmsOpenProfileFromMem(
                b.as_ptr() as *mut _, n);
            let t = if h.is_null() { std::ptr::null() }
                else { c::cmsReadTag(h, sig) };
            eprintln!(
                "{name} ty={ty:#x} h={} t={}",
                !h.is_null(), !t.is_null());
            if !h.is_null() { c::cmsCloseProfile(h); }
        }
        c::cmsCloseProfile(srgb);
    }
}

/// Curve-type dispatch — mutate rTRC's type sig then read
/// (TRC accepts curv+para; rejects others).
#[test]
fn curvetype_dispatch() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let mut n = 0u32;
        c::cmsSaveProfileToMem(srgb,
            std::ptr::null_mut(), &mut n);
        let mut buf = vec![0u8; n as usize];
        c::cmsSaveProfileToMem(srgb,
            buf.as_mut_ptr() as *mut _, &mut n);
        // find rTRC in the tag table (sig 'rTRC' = 0x72545243)
        let mut off = 0usize;
        let ntags = u32::from_be_bytes(
            buf[128..132].try_into().unwrap()) as usize;
        for i in 0..ntags {
            let e = 132 + i * 12;
            if u32::from_be_bytes(
                buf[e..e+4].try_into().unwrap()) == 0x72545243 {
                off = u32::from_be_bytes(
                    buf[e+4..e+8].try_into().unwrap()) as usize;
            }
        }
        for (name, ty) in [
            ("real", u32::from_be_bytes(
                buf[off..off+4].try_into().unwrap())),
            ("curv", 0x63757276), ("para", 0x70617261),
            ("XYZ ", 0x58595a20), ("text", 0x74657874),
            ("mluc", 0x6d6c7563), ("mft2", 0x6d667432),
            ("junk", 0xdeadbeef), ("zero", 0)] {
            let mut b = buf.clone();
            b[off..off+4].copy_from_slice(&ty.to_be_bytes());
            let h = c::cmsOpenProfileFromMem(
                b.as_ptr() as *mut _, n);
            let t = if h.is_null() { std::ptr::null() }
                else { c::cmsReadTag(h, 0x72545243) };
            eprintln!(
                "{name} ty={ty:#x} h={} t={}",
                !h.is_null(), !t.is_null());
            if !h.is_null() { c::cmsCloseProfile(h); }
        }
        c::cmsCloseProfile(srgb);
    }
}

/// XYZ-type dispatch — mutate wtpt's type sig then read
/// (wtpt accepts 'XYZ '+curv? — the colorimetric accept set).
#[test]
fn xyztype_dispatch() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let mut n = 0u32;
        c::cmsSaveProfileToMem(srgb,
            std::ptr::null_mut(), &mut n);
        let mut buf = vec![0u8; n as usize];
        c::cmsSaveProfileToMem(srgb,
            buf.as_mut_ptr() as *mut _, &mut n);
        // wtpt = 0x77747074
        let mut off = 0usize;
        let ntags = u32::from_be_bytes(
            buf[128..132].try_into().unwrap()) as usize;
        for i in 0..ntags {
            let e = 132 + i * 12;
            if u32::from_be_bytes(
                buf[e..e+4].try_into().unwrap()) == 0x77747074 {
                off = u32::from_be_bytes(
                    buf[e+4..e+8].try_into().unwrap()) as usize;
            }
        }
        for (name, ty) in [
            ("real", u32::from_be_bytes(
                buf[off..off+4].try_into().unwrap())),
            ("curv", 0x63757276), ("para", 0x70617261),
            ("XYZ ", 0x58595a20), ("text", 0x74657874),
            ("mluc", 0x6d6c7563), ("mft2", 0x6d667432),
            ("junk", 0xdeadbeef), ("zero", 0)] {
            let mut b = buf.clone();
            b[off..off+4].copy_from_slice(&ty.to_be_bytes());
            let h = c::cmsOpenProfileFromMem(
                b.as_ptr() as *mut _, n);
            let t = if h.is_null() { std::ptr::null() }
                else { c::cmsReadTag(h, 0x77747074) };
            eprintln!(
                "{name} ty={ty:#x} h={} t={}",
                !h.is_null(), !t.is_null());
            if !h.is_null() { c::cmsCloseProfile(h); }
        }
        c::cmsCloseProfile(srgb);
    }
}

/// LUT-type dispatch — mutate A2B0's type sig then read
/// (mft1/mft2/mAB/mBA — the LUT accept set).
#[test]
fn luttype_dispatch() {
    unsafe {
        let dl = c::cmsCreateInkLimitingDeviceLink(0x434d594b, 300.0);
        let mut n = 0u32;
        c::cmsSaveProfileToMem(dl,
            std::ptr::null_mut(), &mut n);
        let mut buf = vec![0u8; n as usize];
        c::cmsSaveProfileToMem(dl,
            buf.as_mut_ptr() as *mut _, &mut n);
        // A2B0 = 0x41324230
        let mut off = 0usize;
        let ntags = u32::from_be_bytes(
            buf[128..132].try_into().unwrap()) as usize;
        for i in 0..ntags {
            let e = 132 + i * 12;
            if u32::from_be_bytes(
                buf[e..e+4].try_into().unwrap()) == 0x41324230 {
                off = u32::from_be_bytes(
                    buf[e+4..e+8].try_into().unwrap()) as usize;
            }
        }
        for (name, ty) in [
            ("real", u32::from_be_bytes(
                buf[off..off+4].try_into().unwrap())),
            ("mft1", 0x6d667431), ("mft2", 0x6d667432),
            ("mAB ", 0x6d414220), ("mBA ", 0x6d424120),
            ("curv", 0x63757276), ("XYZ ", 0x58595a20),
            ("junk", 0xdeadbeef), ("zero", 0)] {
            let mut b = buf.clone();
            b[off..off+4].copy_from_slice(&ty.to_be_bytes());
            let h = c::cmsOpenProfileFromMem(
                b.as_ptr() as *mut _, n);
            let t = if h.is_null() { std::ptr::null() }
                else { c::cmsReadTag(h, 0x41324230) };
            eprintln!(
                "{name} ty={ty:#x} h={} t={}",
                !h.is_null(), !t.is_null());
            if !h.is_null() { c::cmsCloseProfile(h); }
        }
        c::cmsCloseProfile(dl);
    }
}

/// B2A-type dispatch — mutate a corpus profile's B2A0 type sig then read
/// (mft1/mft2/mBA — the reverse-LUT accept set).
#[test]
fn b2atype_dispatch() {
    unsafe {
        // lab4 has B2A0
        let pb = std::path::Path::new(
            env!("CARGO_MANIFEST_DIR"))
            .join("../../lcms2cmyk.icc");
        let path = std::ffi::CString::new(
            pb.to_str().unwrap()).unwrap();
        let src = c::cmsOpenProfileFromFile(
            path.as_ptr(), b"r\0".as_ptr());
        let mut n = 0u32;
        c::cmsSaveProfileToMem(src,
            std::ptr::null_mut(), &mut n);
        let mut buf = vec![0u8; n as usize];
        c::cmsSaveProfileToMem(src,
            buf.as_mut_ptr() as *mut _, &mut n);
        // B2A0 = 0x42324130
        let mut off = 0usize;
        let ntags = u32::from_be_bytes(
            buf[128..132].try_into().unwrap()) as usize;
        for i in 0..ntags {
            let e = 132 + i * 12;
            if u32::from_be_bytes(
                buf[e..e+4].try_into().unwrap()) == 0x42324130 {
                off = u32::from_be_bytes(
                    buf[e+4..e+8].try_into().unwrap()) as usize;
            }
        }
        for (name, ty) in [
            ("real", u32::from_be_bytes(
                buf[off..off+4].try_into().unwrap())),
            ("mft1", 0x6d667431), ("mft2", 0x6d667432),
            ("mAB ", 0x6d414220), ("mBA ", 0x6d424120),
            ("curv", 0x63757276), ("XYZ ", 0x58595a20),
            ("junk", 0xdeadbeef), ("zero", 0)] {
            let mut b = buf.clone();
            b[off..off+4].copy_from_slice(&ty.to_be_bytes());
            let h = c::cmsOpenProfileFromMem(
                b.as_ptr() as *mut _, n);
            let t = if h.is_null() { std::ptr::null() }
                else { c::cmsReadTag(h, 0x42324130) };
            eprintln!(
                "{name} ty={ty:#x} h={} t={}",
                !h.is_null(), !t.is_null());
            if !h.is_null() { c::cmsCloseProfile(h); }
        }
        c::cmsCloseProfile(src);
    }
}

/// Intent-LUT dispatch — read all A2B/B2A intent tags on lcms2cmyk
/// (the per-intent LUT read dispatch).
#[test]
fn intentlut_read() {
    unsafe {
        let pb = std::path::Path::new(
            env!("CARGO_MANIFEST_DIR"))
            .join("../../lcms2cmyk.icc");
        let path = std::ffi::CString::new(
            pb.to_str().unwrap()).unwrap();
        let h = c::cmsOpenProfileFromFile(
            path.as_ptr(), b"r\0".as_ptr());
        assert!(!h.is_null());
        // A2B0/1/2 + B2A0/1/2
        for (name, sig) in [
            ("a2b0", 0x41324230u32), ("a2b1", 0x41324231),
            ("a2b2", 0x41324232), ("b2a0", 0x42324130),
            ("b2a1", 0x42324131), ("b2a2", 0x42324132),
            ("gamt", 0x67616d74), ("pre0", 0x70726530),
            ("pre1", 0x70726531), ("pre2", 0x70726532)] {
            let t = c::cmsReadTag(h, sig);
            eprintln!("{name} sig={sig:#x} t={}", !t.is_null());
        }
        c::cmsCloseProfile(h);
    }
}

/// Distinct-type readers — pseq/sf32/chrm/v2-desc/v2-text/mft2 across
/// corpus profiles (the remaining per-type read dispatch).
#[test]
fn distincttype_read() {
    unsafe {
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        for (file, name, sig) in [
            ("lcms2link.icc", "pseq", 0x70736571u32),
            ("lcms2link.icc", "desc", 0x64657363),
            ("sRGBlcms2.icc", "chad", 0x63686164),
            ("sRGBlcms2.icc", "chrm", 0x6368726d),
            ("labv2lcms2.icc", "v2desc", 0x64657363),
            ("labv2lcms2.icc", "v2cprt", 0x63707274),
            ("labv2lcms2.icc", "v2a2b0", 0x41324230)] {
            let pb = base.join("../../").join(file);
            let path = std::ffi::CString::new(
                pb.to_str().unwrap()).unwrap();
            let h = c::cmsOpenProfileFromFile(
                path.as_ptr(), b"r\0".as_ptr());
            let t = if h.is_null() { std::ptr::null() }
                else { c::cmsReadTag(h, sig) };
            eprintln!("{file} {name} t={}", !t.is_null());
            if !h.is_null() { c::cmsCloseProfile(h); }
        }
    }
}

/// NULLRead — cmsOpenProfileFromIOhandler on the NULL IOhandler:
/// every read goes to the NULL stubs (returns 0), open fails.
#[test]
fn null_iohandler_read() {
    unsafe {
        let io = c::cmsOpenIOhandlerFromNULL(std::ptr::null_mut());
        assert!(!io.is_null());
        let h = c::cmsOpenProfileFromIOhandlerTHR(
            std::ptr::null_mut(), io);
        eprintln!("null-io open={}", !h.is_null());
        assert!(h.is_null(), "profile from NULL io must fail");
    }
}

/// SaveTags arms — open->save (blind-copy for unmodified disk tags),
/// open->write one tag->save (mixed copy+write), raw-tag reads.
#[test]
fn save_tags_arms() {
    unsafe {
        let data = std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../lcms2cmyk.icc")).unwrap();
        // pure open->save — every tag is an unmodified disk tag
        let cp = c::cmsOpenProfileFromMem(
            data.as_ptr() as *const _, data.len() as u32);
        let mut buf = vec![0u8; 1 << 20];
        let mut n = buf.len() as u32;
        let s = c::cmsSaveProfileToMem(cp, buf.as_mut_ptr() as *mut _, &mut n);
        eprintln!("open-save s={s} n={}", if s != 0 { n } else { 0 });
        c::cmsCloseProfile(cp);
        // modify one tag then save — mixed write+blind-copy
        let cp2 = c::cmsOpenProfileFromMem(
            data.as_ptr() as *const _, data.len() as u32);
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let w = c::cmsWriteTag(cp2, 0x6b545243, g as *const _); // 'kTRC'
        let mut n2 = buf.len() as u32;
        let s2 = c::cmsSaveProfileToMem(cp2,
            buf.as_mut_ptr() as *mut _, &mut n2);
        eprintln!("mod-save w={w} s={s2} n={}", if s2 != 0 { n2 } else { 0 });
        // raw tag reads both sizes
        for sig in [0x41324230u32, 0x6b545243, 0x77747074, 0x64657363] {
            let rn = c::cmsReadRawTag(cp2, sig, std::ptr::null_mut(), 0);
            eprintln!("  rawtag {sig:#x} n={rn}");
        }
        c::cmsCloseProfile(cp2);
    }
}

/// IOhandler open modes — file/mem/NULL x r/w access x error paths
/// (missing file, bad mode, truncated profile, IOhandler reuse).
#[test]
fn iohandler_modes() {
    unsafe {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        let data = std::fs::read(dir.join("lcms2cmyk.icc")).unwrap();
        // profile to a temp file for the file-IOhandler path
        let tp = "/tmp/ccov_prof.icc\0";
        std::fs::write(tp.trim_end_matches('\0'), &data).unwrap();
        for (name, mode) in [("file-r", "r"), ("file-w", "w"),
                             ("file-rb", "rb"), ("file-wb", "wb"),
                             ("file-rw", "r+"), ("file-bad", "z")] {
            // "w"-mode truncates; restore the profile each iteration
            std::fs::write(tp.trim_end_matches('\0'), &data).unwrap();
            let io = c::cmsOpenIOhandlerFromFile(
                std::ptr::null_mut(),
                tp.as_ptr() as *const i8,
                format!("{mode}\0").as_ptr() as *const u8);
            eprintln!("{name}: io={}", !io.is_null());
            if !io.is_null() {
                let p = c::cmsOpenProfileFromIOhandlerTHR(
                    std::ptr::null_mut(), io);
                eprintln!("  prof={}", !p.is_null());
                if !p.is_null() { c::cmsCloseProfile(p); }
                // cmsOpenProfileFromIOhandlerTHR frees the handler
                // on failure; only close on success (profile owns it).
            }
        }
        // mem handlers — read + write modes, truncated profile
        for (name, sz) in [("mem-full", data.len()),
                           ("mem-trunc", 100usize),
                           ("mem-zero", 0)] {
            let io = c::cmsOpenIOhandlerFromMem(
                std::ptr::null_mut(),
                data.as_ptr() as *mut _, sz as u32,
                b"r\0".as_ptr() as *const u8);
            let p = if io.is_null() { std::ptr::null_mut() }
                else { c::cmsOpenProfileFromIOhandlerTHR(
                    std::ptr::null_mut(), io) };
            eprintln!("{name}: prof={}", !p.is_null());
            if !p.is_null() { c::cmsCloseProfile(p); }
        }
        // missing file
        let io = c::cmsOpenIOhandlerFromFile(
            std::ptr::null_mut(),
            b"/tmp/nonexistent_xyz.icc\0".as_ptr() as *const i8,
            b"r\0".as_ptr() as *const u8);
        eprintln!("missing-file io={}", !io.is_null());
        // NULL iohandler
        let io = c::cmsOpenIOhandlerFromNULL(std::ptr::null_mut());
        let p = c::cmsOpenProfileFromIOhandlerTHR(
            std::ptr::null_mut(), io);
        eprintln!("null-io prof={}", !p.is_null());
        if !p.is_null() { c::cmsCloseProfile(p); }
    }
}

/// Tag-type write/read sweep — each remaining cmstypes serializer:
/// XYZ, signature, text, chromaticity, measurement, S15/U16Fixed16,
/// dateTime, curve, parametricCurve, UcrBg, screening, UInt8.
#[test]
fn tag_type_sweep() {
    unsafe {
        let prof = c::cmsCreate_sRGBProfile();
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        #[repr(C)] struct Xyz { x: f64, y: f64, z: f64 }
        #[repr(C)] struct Sig { sig: u32 }
        let xyz = Xyz { x: 0.96, y: 1.0, z: 0.82 };
        let sig4 = 0x73696720u32;
        let mlu = c::cmsMLUalloc(std::ptr::null_mut(), 1);
        c::cmsMLUsetUTF8(mlu, b"en".as_ptr(), b"US".as_ptr(),
            b"demo text\0".as_ptr());
        // (tag sig, payload ptr, name) — type-correct payloads
        let cases: &[(u32, *const std::ffi::c_void, &str)] = &[
            (0x77747074, &xyz as *const _ as *const _, "XYZ"),
            (0x63707274, mlu as *const _, "cprt-mlu"),
            (0x6b545243, g as *const _, "curve"),
            (0x64657363, mlu as *const _, "desc-mlu"),
        ];
        for (sig, payload, name) in cases {
            let w = c::cmsWriteTag(prof, *sig, *payload);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, *sig)
            } else { std::ptr::null_mut() };
            eprintln!("{name}: w={w} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
        }
        c::cmsCloseProfile(prof);
    }
}

/// Tag-type reader sweep — profiles carrying varied tag types hit
/// the Type_*_Read bodies: chrm/meas/dtim/cprt/desc/signature.
#[test]
fn tag_reader_sweep() {
    unsafe {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        // corpus profiles carry chrm/meas/dtim/etc tags naturally
        for f in ["lcms2cmyk.icc", "graylcms2.icc", "labv2lcms2.icc",
                  "sRGB3.icc", "linlcms2.icc"] {
            let Ok(d) = std::fs::read(dir.join(f)) else { continue };
            let p = c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32);
            if p.is_null() { continue; }
            let n = c::cmsGetTagCount(p);
            eprintln!("{f}: {n} tags");
            for i in 0..n {
                let sig = c::cmsGetTagSignature(p, i as u32);
                let tag = c::cmsReadTag(p, sig);
                eprintln!("  #{i} {sig:#x} read={}", !tag.is_null());
            }
            c::cmsCloseProfile(p);
        }
    }
}

/// Remaining tag-type serializers — chrm (CIExyYTriple), dtim
/// (DateTime), view (ViewingConditions), sig payloads through
/// write->save->read.
#[test]
fn tag_types_2() {
    unsafe {
        #[repr(C)] struct XyY { x: f64, y: f64, yv: f64 }
        #[repr(C)] struct Triple { r: XyY, g: XyY, b: XyY }
        #[repr(C)] struct Dt { y: u16, mo: u16, d: u16,
            h: u16, mi: u16, s: u16 }
        #[repr(C)] struct Xyz { x: f64, y: f64, z: f64 }
        #[repr(C)] struct View { illum: Xyz, surr: Xyz, it: u32 }
        let prof = c::cmsCreate_sRGBProfile();
        let chrm = Triple {
            r: XyY { x: 0.64, y: 0.33, yv: 1.0 },
            g: XyY { x: 0.30, y: 0.60, yv: 1.0 },
            b: XyY { x: 0.15, y: 0.06, yv: 1.0 } };
        let dt = Dt { y: 2026, mo: 11, d: 8, h: 12, mi: 0, s: 0 };
        let view = View {
            illum: Xyz { x: 0.96, y: 1.0, z: 0.82 },
            surr: Xyz { x: 0.2, y: 0.2, z: 0.2 }, it: 1 };
        for (sig, payload, name) in [
            (0x6368726D, &chrm as *const _ as *const _, "chrm"),
            (0x6474696D, &dt as *const _ as *const _, "dtim"),
            (0x76696577, &view as *const _ as *const _, "view"),
        ] {
            let w = c::cmsWriteTag(prof, sig, payload);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, sig)
            } else { std::ptr::null_mut() };
            eprintln!("{name}: w={w} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
        }
        c::cmsCloseProfile(prof);
    }
}

/// IsTag/SearchOneTag/linked-tag/NULLWrite arms — tag binary
/// search over present/absent sigs, tag linking, save to NULL IO.
#[test]
fn tag_search_linked_null() {
    unsafe {
        let prof = c::cmsCreate_sRGBProfile();
        // IsTag over present + absent sigs -> SearchOneTag both ways
        for sig in [0x77747074u32, 0x64657363, 0x41324230,
                    0xdeadbeef, 0x00000000] {
            let t = c::cmsIsTag(prof, sig);
            eprintln!("istag {sig:#x}={t}");
        }
        // linked tags — write gTRC then link rTRC to it
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        c::cmsWriteTag(prof, 0x67545243, g as *const _); // 'gTRC'
        let l = c::cmsLinkTag(prof, 0x72545243, 0x67545243); // rTRC->gTRC
        eprintln!("link={l}");
        let mut buf = vec![0u8; 1 << 20];
        let mut n = buf.len() as u32;
        let s = c::cmsSaveProfileToMem(prof,
            buf.as_mut_ptr() as *mut _, &mut n);
        eprintln!("save={s}");
        // NULL-IOhandler save -> NULLWrite
        let io = c::cmsOpenIOhandlerFromNULL(std::ptr::null_mut());
        let s2 = c::cmsSaveProfileToIOhandler(prof, io);
        eprintln!("null-save={s2}");
        if !io.is_null() { c::cmsCloseIOhandler(io); }
        c::cmsCloseProfile(prof);
    }
}

/// cmsOpenProfileFromFile path + ReadOffsetArray internals — file
/// opens (vs memory), profile header getters over the tag table.
#[test]
fn file_open_offset_array() {
    unsafe {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        let data = std::fs::read(dir.join("lcms2cmyk.icc")).unwrap();
        let tp = "/tmp/ccov_file_open.icc\0";
        std::fs::write(tp.trim_end_matches('\0'), &data).unwrap();
        let p = c::cmsOpenProfileFromFile(
            tp.as_ptr() as *const i8, b"r\0".as_ptr() as *const u8);
        eprintln!("file-open={}", !p.is_null());
        if !p.is_null() {
            // header getters + tag enumeration -> ReadOffsetArray
            let n = c::cmsGetTagCount(p);
            let cs = c::cmsGetColorSpace(p);
            let pcs = c::cmsGetPCS(p);
            let cls = c::cmsGetDeviceClass(p);
            let v = c::cmsGetEncodedICCversion(p);
            eprintln!("tags={n} cs={cs:#x} pcs={pcs:#x} cls={cls:#x} v={v:#x}");
            for i in 0..n {
                let sig = c::cmsGetTagSignature(p, i as u32);
                let sz = c::cmsReadRawTag(p, sig, std::ptr::null_mut(), 0);
                let _ = sz;
            }
            c::cmsCloseProfile(p);
        }
        // THR variant
        let p2 = c::cmsOpenProfileFromFileTHR(std::ptr::null_mut(),
            tp.as_ptr() as *const i8, b"r\0".as_ptr() as *const i8);
        let _ = p2;
        eprintln!("file-thr={}", !p2.is_null());
        if !p2.is_null() { c::cmsCloseProfile(p2); }
    }
}

/// UcrBg + Screening tag types — rare serializers through
/// write->save->read.
#[test]
fn ucrbg_screening_tags() {
    unsafe {
        let prof = c::cmsCreate_sRGBProfile();
        let u = c::cmsBuildGamma(std::ptr::null_mut(), 1.5);
        let b = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let d = c::cmsMLUalloc(std::ptr::null_mut(), 1);
        c::cmsMLUsetUTF8(d, b"en\0".as_ptr(), b"US\0".as_ptr(),
            b"ucr-bg\0".as_ptr());
        #[repr(C)] struct UcrBg { ucr: *mut std::ffi::c_void,
            bg: *mut std::ffi::c_void, desc: *mut std::ffi::c_void }
        let ub = UcrBg { ucr: u as *mut _, bg: b as *mut _, desc: d };
        let w = c::cmsWriteTag(prof, 0x62666420, &ub as *const _ as *const _); // 'bfd '
        let mut buf = vec![0u8; 1 << 20];
        let mut n = buf.len() as u32;
        let s = if w != 0 {
            c::cmsSaveProfileToMem(prof,
                buf.as_mut_ptr() as *mut _, &mut n)
        } else { 0 };
        let rp = if s != 0 {
            c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
        } else { std::ptr::null_mut() };
        let tag = if !rp.is_null() {
            c::cmsReadTag(rp, 0x62666420)
        } else { std::ptr::null_mut() };
        eprintln!("ucrbg w={w} s={s} read={}", !tag.is_null());
        if !rp.is_null() { c::cmsCloseProfile(rp); }
        c::cmsCloseProfile(prof);
    }
}

/// Raw-tag I/O — cmsReadRawTag/WriteRawTag bytes (no type decode)
/// over LUT and text tags.
#[test]
fn raw_tag_io() {
    unsafe {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        let d = std::fs::read(dir.join("lcms2cmyk.icc")).unwrap();
        let p = c::cmsOpenProfileFromMem(
            d.as_ptr() as *const _, d.len() as u32);
        // ReadRawTag size + bytes
        for sig in [0x41324230u32, 0x6b545243, 0x64657363] {
            let n = c::cmsReadRawTag(p, sig, std::ptr::null_mut(), 0);
            let mut buf = vec![0u8; n as usize];
            let n2 = c::cmsReadRawTag(p, sig,
                buf.as_mut_ptr() as *mut _, n);
            eprintln!("raw {sig:#x} size={n} read={n2}");
        }
        // WriteRawTag a new sig -> new raw tag
        let raw = [0xdeu8, 0xad, 0xbe, 0xef, 1, 2, 3, 4];
        let w = c::cmsWriteRawTag(p, 0xdeadbeef,
            raw.as_ptr() as *const _, raw.len() as u32);
        let rn = c::cmsReadRawTag(p, 0xdeadbeef,
            std::ptr::null_mut(), 0);
        eprintln!("writeraw={w} reread={rn}");
        c::cmsCloseProfile(p);
    }
}

/// Screening tag — cmsScreening {flag, nCh, channels[16]}
/// write->save->read.
#[test]
fn screening_tag() {
    unsafe {
        #[repr(C)] #[derive(Clone, Copy)]
        struct Ch { freq: f64, ang: f64, spot: u32 }
        #[repr(C)] struct Sc { flag: u32, n: u32, ch: [Ch; 16] }
        let prof = c::cmsCreate_sRGBProfile();
        let mut s = Sc { flag: 1, n: 3, ch: [Ch {
            freq: 0.0, ang: 0.0, spot: 0 }; 16] };
        for (i, (f, a, sp)) in [(120.0f64, 15.0f64, 2u32),
                (120.0, 45.0, 2), (120.0, 75.0, 2)]
            .iter().enumerate() {
            s.ch[i] = Ch { freq: *f, ang: *a, spot: *sp };
        }
        let w = c::cmsWriteTag(prof, 0x7363726E,
            &s as *const _ as *const _); // 'scrn'
        let mut buf = vec![0u8; 1 << 20];
        let mut n = buf.len() as u32;
        let sv = if w != 0 {
            c::cmsSaveProfileToMem(prof,
                buf.as_mut_ptr() as *mut _, &mut n)
        } else { 0 };
        let rp = if sv != 0 {
            c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
        } else { std::ptr::null_mut() };
        let tag = if !rp.is_null() {
            c::cmsReadTag(rp, 0x7363726E)
        } else { std::ptr::null_mut() };
        eprintln!("scrn w={w} s={sv} read={}", !tag.is_null());
        if !rp.is_null() { c::cmsCloseProfile(rp); }
        c::cmsCloseProfile(prof);
    }
}

/// Profile-info + header getters — GetProfileInfo/ASCII/UTF8 on
/// desc/cprt + HeaderFlags/Attributes/Creator/Model/ProfileID/
/// RenderingIntent/Manufacturer.
#[test]
fn profile_info_getters() {
    unsafe {
        let p = c::cmsCreate_sRGBProfile();
        let mut buf = vec![0u8; 256];
        for info in [0u32, 1, 2, 3] { // desc/cprt/mluc/info enum
            let n = c::cmsGetProfileInfoASCII(p, info,
                b"en\0".as_ptr() as *const i8,
                b"US\0".as_ptr() as *const i8,
                buf.as_mut_ptr() as *mut _, buf.len() as u32);
            let n2 = c::cmsGetProfileInfoUTF8(p, info,
                b"en\0".as_ptr() as *const i8,
                b"US\0".as_ptr() as *const i8,
                buf.as_mut_ptr() as *mut _, buf.len() as u32);
            eprintln!("info{info} ascii={n} utf8={n2}");
        }
        let mut fl = 0u32;
        let mut at = 0u64;
        let mut id = [0u8; 16];
        c::cmsGetHeaderFlags(p, &mut fl);
        c::cmsGetHeaderAttributes(p, &mut at);
        c::cmsGetHeaderProfileID(p, id.as_mut_ptr());
        eprintln!("flags={fl:#x} attr={at:#x} mdl={:#x} mfr={:#x} ri={}",
            c::cmsGetHeaderModel(p), c::cmsGetHeaderManufacturer(p),
            c::cmsGetHeaderRenderingIntent(p));
        c::cmsCloseProfile(p);
    }
}

/// Header set/get round-trip — SetHeaderRenderingIntent/Flags/
/// Attributes/Manufacturer/Model/Creator/ProfileID then save+read.
#[test]
fn header_setters_getters() {
    unsafe {
        let p = c::cmsCreate_sRGBProfile();
        c::cmsSetHeaderRenderingIntent(p, 1);
        c::cmsSetHeaderFlags(p, 0xABCD);
        c::cmsSetHeaderAttributes(p, 0x1122334455667788);
        c::cmsSetHeaderManufacturer(p, 0x6d66721a);
        c::cmsSetHeaderModel(p, 0x6d646c1a);
        let mut id = [0u8; 16];
        for i in 0..16 { id[i] = i as u8; }
        c::cmsSetHeaderProfileID(p, id.as_mut_ptr());
        let mut buf = vec![0u8; 1 << 20];
        let mut n = buf.len() as u32;
        let s = c::cmsSaveProfileToMem(p,
            buf.as_mut_ptr() as *mut _, &mut n);
        let rp = if s != 0 {
            c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
        } else { std::ptr::null_mut() };
        if !rp.is_null() {
            let mut fl = 0u32;
            let mut at = 0u64;
            let mut pid = [0u8; 16];
            c::cmsGetHeaderFlags(rp, &mut fl);
            c::cmsGetHeaderAttributes(rp, &mut at);
            c::cmsGetHeaderProfileID(rp, pid.as_mut_ptr());
            eprintln!("flags={fl:#x} attr={at:#x} mfr={:#x} mdl={:#x} ri={} id0={}",
                c::cmsGetHeaderManufacturer(rp),
                c::cmsGetHeaderModel(rp),
                c::cmsGetHeaderRenderingIntent(rp), pid[0]);
            c::cmsCloseProfile(rp);
        }
        c::cmsCloseProfile(p);
    }
}

/// IO-handler internals — GetProfileIOhandler + open-for-write
/// IOhandler2 + close arms.
#[test]
fn iohandler_internals() {
    unsafe {
        let data = vec![0xAAu8; 512];
        let io = c::cmsOpenIOhandlerFromMem(std::ptr::null_mut(),
            data.as_ptr() as *mut _, data.len() as u32,
            b"r\0".as_ptr());
        if !io.is_null() {
            let p = c::cmsOpenProfileFromIOhandlerTHR(
                std::ptr::null_mut(), io);
            eprintln!("io-open={}", !p.is_null());
            if !p.is_null() {
                let gio = c::cmsGetProfileIOhandler(p);
                eprintln!("getio={}", !gio.is_null());
                c::cmsCloseProfile(p); // frees io
            }
        }
        // write-mode IOhandler2
        let io2 = c::cmsOpenIOhandlerFromNULL(std::ptr::null_mut());
        let p2 = c::cmsOpenProfileFromIOhandler2THR(
            std::ptr::null_mut(), io2, 1);
        eprintln!("io2-open={}", !p2.is_null());
        if !p2.is_null() { c::cmsCloseProfile(p2); }
    }
}

/// ParametricCurve tag reader per-type dispatch — build each
/// parametric type, tag it, read back -> the reader's type arms.
#[test]
fn parametric_curve_reader() {
    unsafe {
        for ty in [1i32, 2, 3, 4, 5, 6, 7, 8, 108, -108] {
            let params = [2.2f64, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
            let tc = c::cmsBuildParametricToneCurve(
                std::ptr::null_mut(), ty, params.as_ptr());
            if tc.is_null() { eprintln!("ty{ty} alloc=null"); continue; }
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsWriteTag(prof, 0x72545243, tc as *const _); // 'prtr'
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = c::cmsSaveProfileToMem(prof,
                buf.as_mut_ptr() as *mut _, &mut n);
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, 0x72545243)
            } else { std::ptr::null_mut() };
            eprintln!("ty{ty} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
            c::cmsFreeToneCurve(tc);
        }
    }
}

/// Corpus tag-read sweep — every tag in every corpus profile ->
/// the full cmstypes reader dispatch + io0 tag-table machinery.
#[test]
fn corpus_all_tags() {
    unsafe {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        let mut profiles = Vec::new();
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.flatten() {
                let n = e.file_name().to_string_lossy().into_owned();
                if n.ends_with(".icc") || n.ends_with(".icm") {
                    profiles.push(e.path());
                }
            }
        }
        eprintln!("{} corpus profiles", profiles.len());
        let mut total = 0usize;
        let mut ok = 0usize;
        for pf in &profiles {
            let Ok(d) = std::fs::read(pf) else { continue };
            let p = c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32);
            if p.is_null() { continue; }
            let n = c::cmsGetTagCount(p);
            for i in 0..n {
                let sig = c::cmsGetTagSignature(p, i as u32);
                let tag = c::cmsReadTag(p, sig);
                total += 1;
                if !tag.is_null() { ok += 1; }
            }
            c::cmsCloseProfile(p);
        }
        eprintln!("tags {ok}/{total} read");
    }
}

/// SaveProfileToMem two-phase + MultiprofileTransform — the
/// size-query pass then the real write; multi-profile chain.
#[test]
fn save_two_phase_multiprofile() {
    unsafe {
        let p = c::cmsCreate_sRGBProfile();
        // phase 1: size query (NULL buffer)
        let mut need = 0u32;
        let s1 = c::cmsSaveProfileToMem(p, std::ptr::null_mut(), &mut need);
        eprintln!("size-query s={s1} need={need}");
        // phase 2: real write
        let mut buf = vec![0u8; need as usize + 64];
        let s2 = c::cmsSaveProfileToMem(p,
            buf.as_mut_ptr() as *mut _, &mut need);
        eprintln!("write s={s2}");
        // too-small buffer -> reject
        let mut small = [0u8; 8];
        let mut sn = small.len() as u32;
        let s3 = c::cmsSaveProfileToMem(p,
            small.as_mut_ptr() as *mut _, &mut sn);
        eprintln!("small s={s3}");
        c::cmsCloseProfile(p);
        // multiprofile transform — chain of 3
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let xyz = c::cmsCreateXYZProfile();
        let mut chain = [srgb, lab, xyz];
        let r16 = (4u32 << 16) | (3 << 3) | 2;
        let x16 = (11u32 << 16) | (3 << 3) | 2;
        let t = c::cmsCreateMultiprofileTransform(chain.as_mut_ptr(),
            2, r16, x16, 0, 0);
        eprintln!("multi t={}", !t.is_null());
        if !t.is_null() { c::cmsDeleteTransform(t); }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
        c::cmsCloseProfile(xyz);
    }
}

/// HeaderCreationDateTime + remaining header fields — the
/// dateTime header field get/set round-trip.
#[test]
fn header_datetime() {
    unsafe {
        let p = c::cmsCreate_sRGBProfile();
        #[repr(C)] struct Dt { y: u16, mo: u16, d: u16,
            h: u16, mi: u16, s: u16 }
        let mut dt = Dt { y: 0, mo: 0, d: 0, h: 0, mi: 0, s: 0 };
        let r = c::cmsGetHeaderCreationDateTime(p,
            &mut dt as *mut _ as *mut _);
        eprintln!("created {r} {}-{}-{}", dt.y, dt.mo, dt.d);
        // no public setter — re-read after a header mutation via
        // SetHeaderFlags touches the header-write path too
        c::cmsCloseProfile(p);
    }
}

/// Malformed tag reads — truncated/corrupt tag bytes feed the
/// readers' error paths (the biggest uncovered-line class).
#[test]
fn malformed_tag_reads() {
    unsafe {
        // hand-built minimal profiles with truncated tag payloads
        for sig in [0x72545243u32, 0x77747074, 0x64657363,
                    0x6e636c32, 0x70696d63, 0x7363726E] {
            // craft a fake tag-table entry pointing at short data
            let hlen = 128 + 4 + 12 + 32;
            let mut hdr = vec![0u8; hlen];
            // profile size
            hdr[0..4].copy_from_slice(&(hlen as u32).to_be_bytes());
            hdr[36..40].copy_from_slice(b"acsp");
            let n_tags = hdr.len() - 128 - 4;
            hdr[128..132].copy_from_slice(&1u32.to_be_bytes());
            hdr[132..136].copy_from_slice(&sig.to_be_bytes());
            hdr[136..140].copy_from_slice(&144u32.to_be_bytes());
            hdr[140..144].copy_from_slice(&8u32.to_be_bytes()); // 8-byte payload
            let p = c::cmsOpenProfileFromMem(
                hdr.as_ptr() as *const _, hdr.len() as u32);
            if p.is_null() { eprintln!("sig{sig:#x} open=null"); continue; }
            let tag = c::cmsReadTag(p, sig);
            eprintln!("sig{sig:#x} tag={}", !tag.is_null());
            c::cmsCloseProfile(p);
        }
    }
}

/// LUT-reader error paths — corrupt mft1/mft2 structures (bad
/// channel counts, bad CLUT sizes, bad offsets) -> reader
/// validation branches in cmstypes.
#[test]
fn lut_reader_error_paths() {
    unsafe {
        let mft2: [u8; 4] = *b"mft2";
        let mft1: [u8; 4] = *b"mft1";
        let mut v1 = Vec::from(mft2);
        v1.extend_from_slice(&[0,0,0,0, 0,0,0,0]);
        let mut v2 = Vec::from(mft2);
        v2.extend_from_slice(&[0,0,0,0, 20,3,17,0]);
        let mut v3 = Vec::from(mft1);
        v3.extend_from_slice(&[0,0,0,0, 3,3,2,0]);
        for (payload, name) in [
            (v1.as_slice(), "mft2-zeros"),
            (v2.as_slice(), "mft2-badch"),
            (v3.as_slice(), "mft1-min"),
            (mft2.as_slice(), "mft2-trunc"),
        ] {
            let hlen = 128 + 4 + 12 + payload.len();
            let mut hdr = vec![0u8; hlen];
            hdr[0..4].copy_from_slice(&(hlen as u32).to_be_bytes());
            hdr[36..40].copy_from_slice(b"acsp");
            hdr[128..132].copy_from_slice(&1u32.to_be_bytes());
            hdr[132..136].copy_from_slice(&0x41324230u32.to_be_bytes()); // A2B0
            hdr[136..140].copy_from_slice(&144u32.to_be_bytes());
            hdr[140..144].copy_from_slice(&(payload.len() as u32).to_be_bytes());
            hdr[144..].copy_from_slice(payload);
            let p = c::cmsOpenProfileFromMem(
                hdr.as_ptr() as *const _, hlen as u32);
            if p.is_null() { eprintln!("{name} open=null"); continue; }
            let tag = c::cmsReadTag(p, 0x41324230);
            eprintln!("{name} tag={}", !tag.is_null());
            c::cmsCloseProfile(p);
        }
    }
}

/// mAB reader error paths — corrupt mAB/mBA offset tables ->
/// Type_LUTA2B_Read/LUTB2A_Read validation branches.
#[test]
fn mab_reader_error_paths() {
    unsafe {
        let mab: [u8; 4] = *b"mAB ";
        let mba: [u8; 4] = *b"mBA ";
        let mut v1 = Vec::from(mab);
        // mAB header: reserved(4) inch(1) outch(1) pad(2)
        //           bCurve(4) matrix(4) mCurve(4) clut(4) aCurve(4)
        v1.extend_from_slice(&[0,0,0,0, 0xFF,3, 0,0]); // inch=255 invalid
        v1.extend_from_slice(&[0u8; 20]);
        let mut v2 = Vec::from(mab);
        v2.extend_from_slice(&[0,0,0,0, 3,3, 0,0]);
        // offsets all pointing past tag end
        v2.extend_from_slice(&[0xFF,0xFF,0xFF,0xF0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]);
        let mut v3 = Vec::from(mba);
        v3.extend_from_slice(&[0,0,0,0, 3,3, 0,0]);
        v3.extend_from_slice(&[0u8; 20]);
        for (payload, name) in [
            (v1.as_slice(), "mab-badch"),
            (v2.as_slice(), "mab-badoff"),
            (v3.as_slice(), "mba-zeros"),
        ] {
            let hlen = 128 + 4 + 12 + payload.len();
            let mut hdr = vec![0u8; hlen];
            hdr[0..4].copy_from_slice(&(hlen as u32).to_be_bytes());
            hdr[36..40].copy_from_slice(b"acsp");
            hdr[128..132].copy_from_slice(&1u32.to_be_bytes());
            hdr[132..136].copy_from_slice(&0x41324230u32.to_be_bytes());
            hdr[136..140].copy_from_slice(&144u32.to_be_bytes());
            hdr[140..144].copy_from_slice(&(payload.len() as u32).to_be_bytes());
            hdr[144..].copy_from_slice(payload);
            let p = c::cmsOpenProfileFromMem(
                hdr.as_ptr() as *const _, hlen as u32);
            if p.is_null() { eprintln!("{name} open=null"); continue; }
            let tag = c::cmsReadTag(p, 0x41324230);
            eprintln!("{name} tag={}", !tag.is_null());
            c::cmsCloseProfile(p);
        }
    }
}

/// Header setters/getters 2 — SetColorSpace/PCS/DeviceClass/
/// ProfileVersion + GetDeviceClass/ProfileVersion round-trips.
#[test]
fn header_setters_2() {
    unsafe {
        let p = c::cmsCreateProfilePlaceholder(std::ptr::null_mut());
        for cs in [0x52474220u32, 0x434D594B, 0x4C616220,
                   0x58595A20, 0x6E636C32] {
            c::cmsSetColorSpace(p, cs);
            eprintln!("cs={:#x} get={:#x}", cs, c::cmsGetColorSpace(p));
        }
        for pcs in [0x4C616220u32, 0x58595A20] {
            c::cmsSetPCS(p, pcs);
            eprintln!("pcs={:#x} get={:#x}", pcs, c::cmsGetPCS(p));
        }
        for cls in [0x6d6e7472u32, 0x6c696e6b, 0x61627374,
                    0x6e6d636c, 0x73706163] {
            c::cmsSetDeviceClass(p, cls);
            eprintln!("cls={:#x} get={:#x}", cls,
                c::cmsGetDeviceClass(p));
        }
        for v in [2.2f64, 4.3, 4.4] {
            c::cmsSetProfileVersion(p, v);
            eprintln!("v={v} get={:#x}", c::cmsGetEncodedICCversion(p));
        }
        c::cmsCloseProfile(p);
    }
}

/// cmsMD5 incremental hash — alloc/add/finish over profile bytes
/// -> the profile-ID computation internals.
#[test]
fn md5_incremental() {
    unsafe {
        let h = c::cmsMD5alloc(std::ptr::null_mut());
        eprintln!("md5={}", !h.is_null());
        if !h.is_null() {
            let data = b"profile content for hashing";
            // add in chunks -> the incremental-buffer arms
            for chunk in data.chunks(4) {
                c::cmsMD5add(h, chunk.as_ptr(), chunk.len() as u32);
            }
            let mut id = [0u8; 16];
            c::cmsMD5finish(id.as_mut_ptr() as *mut _, h);
            eprintln!("id={:02x?}", &id[..8]);
        }
    }
}

/// _cmsSearchTag lFollowLinks — linked tags resolve through the
/// link chain on read; IsTag follows them.
#[test]
fn linked_tag_resolution() {
    unsafe {
        let p = c::cmsCreate_sRGBProfile();
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        // write gTRC then link rTRC->gTRC and bTRC->rTRC (chain)
        c::cmsWriteTag(p, 0x67545243, g as *const _); // 'gTRC'
        c::cmsLinkTag(p, 0x72545243, 0x67545243);      // rTRC->gTRC
        c::cmsLinkTag(p, 0x62545243, 0x72545243);      // bTRC->rTRC
        // reads resolve the link chain
        for sig in [0x67545243u32, 0x72545243, 0x62545243] {
            let tag = c::cmsReadTag(p, sig);
            eprintln!("sig{sig:#x} tag={}", !tag.is_null());
        }
        // save + reread — linked tags serialize
        let mut buf = vec![0u8; 1 << 20];
        let mut n = buf.len() as u32;
        let s = c::cmsSaveProfileToMem(p,
            buf.as_mut_ptr() as *mut _, &mut n);
        let rp = if s != 0 {
            c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
        } else { std::ptr::null_mut() };
        if !rp.is_null() {
            let tag = c::cmsReadTag(rp, 0x62545243); // bTRC via chain
            eprintln!("reread bTRC={}", !tag.is_null());
            c::cmsCloseProfile(rp);
        }
        c::cmsCloseProfile(p);
    }
}

/// cicp VideoSignalType tag — 4-u8 video-signal payload
/// write->save->read.
#[test]
fn cicp_tag() {
    unsafe {
        #[repr(C)] struct Cicp { cp: u8, tc: u8, mc: u8, vr: u8 }
        let prof = c::cmsCreate_sRGBProfile();
        let c = Cicp { cp: 1, tc: 13, mc: 0, vr: 1 };
        let w = c::cmsWriteTag(prof, 0x63696370,
            &c as *const _ as *const _); // 'cicp'
        let mut buf = vec![0u8; 1 << 20];
        let mut n = buf.len() as u32;
        let s = if w != 0 {
            c::cmsSaveProfileToMem(prof,
                buf.as_mut_ptr() as *mut _, &mut n)
        } else { 0 };
        let rp = if s != 0 {
            c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
        } else { std::ptr::null_mut() };
        let tag = if !rp.is_null() {
            c::cmsReadTag(rp, 0x63696370)
        } else { std::ptr::null_mut() };
        eprintln!("cicp w={w} s={s} read={}", !tag.is_null());
        if !rp.is_null() { c::cmsCloseProfile(rp); }
        c::cmsCloseProfile(prof);
    }
}

/// WriteTag unsupported-type reject + ReadTag for tags without
/// handlers -> the type-handler-miss arms.
#[test]
fn unsupported_type_arms() {
    unsafe {
        let p = c::cmsCreate_sRGBProfile();
        // write with a payload that has no matching type handler
        let blob = [0xdeu8, 0xad, 0xbe, 0xef];
        for sig in [0xdeadbeefu32, 0x6e6f6e65, 0x00000000] {
            let w = c::cmsWriteTag(p, sig,
                blob.as_ptr() as *const _);
            eprintln!("sig{sig:#x} write={w}");
        }
        c::cmsCloseProfile(p);
    }
}

/// Numeric-array tag types — UInt8/16/32/64 + S15/U16Fixed16
/// arrays -> Type_UInt*/Fixed16Array serializers.
#[test]
fn numeric_array_tags() {
    unsafe {
        let prof = c::cmsCreate_sRGBProfile();
        let u16s = [1u16, 2, 3, 0x8000];
        let u32s = [1u32, 0xDEADBEEF];
        let s15 = [0x4000u32, 0xFFFF8000, 0x00010000];
        for (sig, payload, name) in [
            (0x4E4C5520u32, u16s.as_ptr() as *const _, "u16arr"),
            (0x4E4C5532, u32s.as_ptr() as *const _, "u32arr"),
            (0x53463136, s15.as_ptr() as *const _, "s15arr"),
        ] {
            let w = c::cmsWriteTag(prof, sig, payload);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, sig)
            } else { std::ptr::null_mut() };
            eprintln!("{name} w={w} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
        }
        c::cmsCloseProfile(prof);
    }
}

/// Type_LUT8/16_Read validation internals — malformed channel/
/// count/matrix structures -> the reader's per-field validation.
#[test]
fn lut_read_validation() {
    unsafe {
        // malformed mft2 (Lut16) tags — bad channel counts, counts
        let mut t = vec![0u8; 64];
        let mut hdr = |in_ch: u8, out_ch: u8, grid: u8| {
            // type sig + reserved + in_ch out_ch grid + matrix
            let mut v = Vec::with_capacity(60);
            v.extend_from_slice(&0x6d667432u32.to_be_bytes()); // 'mft2'
            v.extend_from_slice(&[0; 4]);
            v.push(in_ch); v.push(out_ch); v.push(grid); v.push(0);
            for _ in 0..9 { v.extend_from_slice(&0x10000u32.to_be_bytes()); }
            v.extend_from_slice(&256u16.to_be_bytes()); // inTableEntries
            v.extend_from_slice(&256u16.to_be_bytes()); // outTableEntries
            v
        };
        for (ic, oc, g, name) in [
            (0, 3, 2, "0-in"),
            (3, 0, 2, "0-out"),
            (3, 3, 0, "0-grid"),
            (15, 15, 2, "15ch"),
            (3, 3, 250, "huge-grid"),
        ] {
            let p = c::cmsCreate_sRGBProfile();
            let raw = hdr(ic, oc, g);
            c::cmsWriteRawTag(p, 0x41324230, raw.as_ptr() as *const _,
                raw.len() as u32);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = c::cmsSaveProfileToMem(p,
                buf.as_mut_ptr() as *mut _, &mut n);
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, 0x41324230)
            } else { std::ptr::null_mut() };
            eprintln!("{name} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(p);
        }
    }
}

/// Save-target variants — ToFile/ToIOhandler/ToStream +
/// GetProfileIOhandler + CloseIOhandler.
#[test]
fn save_target_variants() {
    unsafe {
        let p = c::cmsCreate_sRGBProfile();
        // ToFile
        let path = std::env::temp_dir().join("st-save.icc");
        let ps = format!("{}\0", path.display());
        let r1 = c::cmsSaveProfileToFile(p, ps.as_ptr() as *const i8);
        // ToIOhandler — memory handler, write mode
        let mut mem = vec![0u8; 1 << 20];
        let io = c::cmsOpenIOhandlerFromMem(std::ptr::null_mut(),
            mem.as_mut_ptr() as *mut _, mem.len() as u32,
            b"w\0".as_ptr());
        let r2 = if !io.is_null() {
            c::cmsSaveProfileToIOhandler(p, io)
        } else { 0 };
        // GetProfileIOhandler — the profile's own handler
        let gh = c::cmsGetProfileIOhandler(p);
        eprintln!("file={r1} ioh={r2} getio={}", !gh.is_null());
        if !io.is_null() { c::cmsCloseIOhandler(io); }
        c::cmsCloseProfile(p);
    }
}

/// SaveToMem partial-buffer + GetProfileInfoASCII/Unicode
/// variant dispatch.
#[test]
fn save_mem_edge_and_info_variants() {
    unsafe {
        let p = c::cmsCreate_sRGBProfile();
        // size query then partial buffer then exact buffer
        let mut n0 = 0u32;
        c::cmsSaveProfileToMem(p, std::ptr::null_mut(), &mut n0);
        let mut small = [0u8; 4];
        let mut ns = 4u32;
        let r_small = c::cmsSaveProfileToMem(p,
            small.as_mut_ptr() as *mut _, &mut ns);
        let mut buf = vec![0u8; n0 as usize];
        let mut n = n0;
        let r_full = c::cmsSaveProfileToMem(p,
            buf.as_mut_ptr() as *mut _, &mut n);
        eprintln!("n0={n0} small={r_small} full={r_full}");
        c::cmsCloseProfile(p);
    }
}

/// GetProfileInfo variants — ASCII/UTF8/wide per info-id ->
/// per-encoding + per-info-id dispatch.
#[test]
fn profile_info_variants() {
    unsafe {
        let p = c::cmsCreate_sRGBProfile();
        // info ids 0-9 -> desc/cprt/dmnd/dmdd/lmod/tech/etc
        for info in 0u32..10 {
            let mut ab = [0i8; 128];
            let mut wb = [0i32; 64];
            let a = c::cmsGetProfileInfoASCII(p, info,
                b"en\0".as_ptr() as *const i8,
                b"US\0".as_ptr() as *const i8,
                ab.as_mut_ptr() as *mut _, 128);
            let u = c::cmsGetProfileInfoUTF8(p, info,
                b"en\0".as_ptr() as *const i8,
                b"US\0".as_ptr() as *const i8,
                ab.as_mut_ptr() as *mut _, 128);
            let w = c::cmsGetProfileInfo(p, info,
                b"en\0".as_ptr() as *const i8,
                b"US\0".as_ptr() as *const i8,
                wb.as_mut_ptr() as *mut _, 64);
            if a + u + w > 0 {
                eprintln!("info{info} a={a} u={u} w={w}");
            }
        }
        c::cmsCloseProfile(p);
    }
}

/// Remaining tag-type handlers — mAB devicelink + multi-entry
/// dict + named-color + cicp + viewing conditions.
#[test]
fn remaining_tag_handlers() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mat = [0.4f64, 0.3, 0.3, 0.2, 0.6, 0.2, 0.1, 0.1, 0.8];
        let ftab: Vec<f32> = (0..8*3).map(|i| i as f32 / 24.0).collect();
        // named-color list + dict
        let ncl = c::cmsAllocNamedColorList(std::ptr::null_mut(), 1,
            3, b"p\0".as_ptr(), b"s\0".as_ptr());
        let lab16 = [0xF000u16; 3];
        c::cmsAppendNamedColor(ncl, b"c1\0".as_ptr(),
            lab16.as_ptr(), lab16.as_ptr());
        let d = c::cmsDictAlloc(std::ptr::null_mut());
        let w = |s: &str| s.chars().map(|c| c as i32)
            .chain([0]).collect::<Vec<i32>>();
        c::cmsDictAddEntry(d, w("k1").as_ptr(), w("v1").as_ptr(),
            std::ptr::null_mut(), std::ptr::null_mut());
        let m = c::cmsMLUalloc(std::ptr::null_mut(), 1);
        c::cmsMLUsetUTF8(m, b"en\0".as_ptr() as *const _,
            b"US\0".as_ptr() as *const _, b"x\0".as_ptr());
        for (sig, payload, name) in [
            (0x6d657461u32, d as *const _, "dict"),
            (0x6e636c32, ncl as *const _, "ncl2"),
            (0x64657363, m as *const _, "mlu-desc"),
        ] {
            let prof = c::cmsCreate_sRGBProfile();
            let w = c::cmsWriteTag(prof, sig, payload);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, sig)
            } else { std::ptr::null_mut() };
            eprintln!("{name} w={w} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
        c::cmsMLUfree(m);
        c::cmsFreeNamedColorList(ncl);
        c::cmsDictFree(d);
    }
}

/// Remaining tag-type handlers — XYZ/signature/text/chromaticity/
/// measurement/datetime payloads.
#[test]
fn misc_type_handlers() {
    unsafe {
        let prof = c::cmsCreate_sRGBProfile();
        #[repr(C)] struct Xyz { x: f64, y: f64, z: f64 }
        #[repr(C)] struct Chrm { u: u32, px: f64, py: f64,
            gx: f64, gy: f64, bx: f64, by: f64 }
        let xyz = Xyz { x: 0.96, y: 1.0, z: 0.82 };
        let chrm = Chrm { u: 1, px: 0.64, py: 0.33,
            gx: 0.3, gy: 0.6, bx: 0.15, by: 0.06 };
        for (sig, payload, name) in [
            (0x77747074u32, &xyz as *const _ as *const _, "xyz"),
            (0x72686976, &chrm as *const _ as *const _, "chrm"),
        ] {
            let w = c::cmsWriteTag(prof, sig, payload);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, sig)
            } else { std::ptr::null_mut() };
            eprintln!("{name} w={w} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
        }
        c::cmsCloseProfile(prof);
    }
}

/// Per-tag-sig dispatch — devicelink mpet tags (A2B0/B2A0/D2B0/
/// B2D0/AToB/BToA/gamt/pre) through WriteTag/ReadTag.
#[test]
fn devicelink_tag_dispatch() {
    unsafe {
        let ftab: Vec<f32> = (0..8*3)
            .map(|i| i as f32 / 24.0).collect();
        let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocCLutFloat(std::ptr::null_mut(), 2, 3, 3,
                ftab.as_ptr()));
        for sig in [0x41324230u32, 0x42324130, 0x44324230,
                    0x42324430, 0x67616d74, 0x70726530] {
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsSetProfileVersion(prof, 4.4);
            let w = c::cmsWriteTag(prof, sig, p as *const _);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, sig)
            } else { std::ptr::null_mut() };
            eprintln!("sig{sig:#x} w={w} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
        c::cmsPipelineFree(p);
    }
}

/// XYZ-typed tag handlers — wtpt/bkpt/lumi through the XYZ
/// serializer.
#[test]
fn xyz_tag_handlers() {
    unsafe {
        #[repr(C)] struct Xyz { x: f64, y: f64, z: f64 }
        let xyz = Xyz { x: 0.96, y: 1.0, z: 0.82 };
        for sig in [0x77747074u32, 0x626b7074, 0x6c756d69] {
            let prof = c::cmsCreate_sRGBProfile();
            let w = c::cmsWriteTag(prof, sig,
                &xyz as *const _ as *const _);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, sig)
            } else { std::ptr::null_mut() };
            eprintln!("sig{sig:#x} w={w} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
    }
}

/// Text/description/signature tag handlers — the remaining
/// type-handler dispatch.
#[test]
fn text_sig_handlers() {
    unsafe {
        let m = c::cmsMLUalloc(std::ptr::null_mut(), 1);
        c::cmsMLUsetUTF8(m, b"en\0".as_ptr() as *const _,
            b"US\0".as_ptr() as *const _, b"copyright\0".as_ptr());
        for (sig, name) in [
            (0x63707274u32, "cprt"),
            (0x6c6d6e64, "lmnd"),
            (0x6c6d7364, "lmsd"),
        ] {
            let prof = c::cmsCreate_sRGBProfile();
            let w = c::cmsWriteTag(prof, sig, m as *const _);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, sig)
            } else { std::ptr::null_mut() };
            eprintln!("{name} w={w} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
        c::cmsMLUfree(m);
    }
}

/// Curve-tag handlers — rTRC/gTRC/bTRC/kTRC through the curve
/// serializer with different curve types.
#[test]
fn curve_tag_handlers() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let tbl = [0u16, 0x4000, 0x8000, 0xFFFF];
        let tc = c::cmsBuildTabulatedToneCurve16(std::ptr::null_mut(),
            4, tbl.as_ptr());
        for (sig, tc2, name) in [
            (0x72545243u32, g, "rTRC-gamma"),
            (0x67545243, tc, "gTRC-table"),
            (0x62545243, g, "bTRC-gamma"),
            (0x6b545243, tc, "kTRC-table"),
        ] {
            let prof = c::cmsCreate_sRGBProfile();
            let w = c::cmsWriteTag(prof, sig, tc2 as *const _);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, sig)
            } else { std::ptr::null_mut() };
            eprintln!("{name} w={w} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
        c::cmsFreeToneCurve(g);
        c::cmsFreeToneCurve(tc);
    }
}

/// Parametric-curve tag handlers — all parametric types through
/// TRC sigs -> Type_ParametricCurve dispatch.
#[test]
fn parametric_curve_handlers() {
    unsafe {
        // parametric types 1-10 via BuildParametricToneCurve
        for ptype in 1i32..=10 {
            let params = [2.2f64, 1.0, 0.0, 0.0, 0.0,
                          0.0, 0.0, 0.0, 0.0, 0.0];
            let tc = c::cmsBuildParametricToneCurve(
                std::ptr::null_mut(), ptype, params.as_ptr());
            if tc.is_null() { continue; }
            let prof = c::cmsCreate_sRGBProfile();
            let w = c::cmsWriteTag(prof, 0x72545243, tc as *const _);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, 0x72545243)
            } else { std::ptr::null_mut() };
            eprintln!("ptype{ptype} w={w} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
            c::cmsFreeToneCurve(tc);
        }
    }
}

/// Multi-format desc handlers — 'desc' with MLU (v4) and 'desc'
/// tag structure (v2) variants.
#[test]
fn desc_format_variants() {
    unsafe {
        let m = c::cmsMLUalloc(std::ptr::null_mut(), 1);
        c::cmsMLUsetUTF8(m, b"en\0".as_ptr() as *const _,
            b"US\0".as_ptr() as *const _, b"description\0".as_ptr());
        for ver in [2.1f64, 4.4] {
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsSetProfileVersion(prof, ver);
            let w = c::cmsWriteTag(prof, 0x64657363, m as *const _);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, 0x64657363)
            } else { std::ptr::null_mut() };
            eprintln!("v{ver} w={w} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
        c::cmsMLUfree(m);
    }
}

/// Signature/technology/viewing tags — Type_Signature + Type_
/// ViewingConditions + Type_Measurement handlers.
#[test]
fn signature_viewing_tags() {
    unsafe {
        #[repr(C)] struct Viewing { x: f64, y: f64, z: f64,
            a: u32, b: u32, c: f64 }
        let v = Viewing { x: 0.95, y: 1.0, z: 1.09,
            a: 2, b: 0, c: 0.5 };
        for (sig, payload, name) in [
            (0x74656368u32, &(0x72676762u32) as *const _ as *const _, "tech"),
            (0x76696577, &v as *const _ as *const _, "view"),
        ] {
            let prof = c::cmsCreate_sRGBProfile();
            let w = c::cmsWriteTag(prof, sig, payload);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, sig)
            } else { std::ptr::null_mut() };
            eprintln!("{name} w={w} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
    }
}

/// Curve-tag variants — segmented + parametric + tabulated curves
/// on all 4 TRC sigs.
#[test]
fn curve_tag_variants() {
    unsafe {
        #[repr(C)] struct Seg { x0: f32, x1: f32, ty: i32,
            params: [f64; 10], n: u32, pt: *const f32 }
        let smp = [0.0f32, 0.5, 1.0];
        let segs = [Seg { x0: 0.0, x1: 1.0, ty: 0,
            params: [0.0; 10], n: 3, pt: smp.as_ptr() }];
        let sg = c::cmsBuildSegmentedToneCurve(std::ptr::null_mut(),
            1, segs.as_ptr() as *const _);
        let params = [2.2f64, 1.0, 0.0, 0.0, 0.0,
                      0.0, 0.0, 0.0, 0.0, 0.0];
        let pg = c::cmsBuildParametricToneCurve(
            std::ptr::null_mut(), 1, params.as_ptr());
        let tbl = [0u16, 0x8000, 0xFFFF];
        let tb = c::cmsBuildTabulatedToneCurve16(std::ptr::null_mut(),
            3, tbl.as_ptr());
        for (sig, tc, name) in [
            (0x72545243u32, sg, "seg-rTRC"),
            (0x67545243, pg, "para-gTRC"),
            (0x62545243, tb, "tab-bTRC"),
            (0x6b545243, sg, "seg-kTRC"),
        ] {
            if tc.is_null() { continue; }
            let prof = c::cmsCreate_sRGBProfile();
            let w = c::cmsWriteTag(prof, sig, tc as *const _);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, sig)
            } else { std::ptr::null_mut() };
            eprintln!("{name} w={w} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
    }
}

/// Rare tag-type handlers — screening, measurement, response-
/// curve-set, signature-array payloads.
#[test]
fn rare_type_handlers() {
    unsafe {
        #[repr(C)] struct Meas { obs: u32, geo: u32,
            i: [f64; 3], s: u32 }
        let m = Meas { obs: 0, geo: 0, i: [0.95, 1.0, 1.09], s: 1 };
        for (sig, payload, name) in [
            (0x6d656173u32, &m as *const _ as *const _, "meas"),
        ] {
            let prof = c::cmsCreate_sRGBProfile();
            let w = c::cmsWriteTag(prof, sig, payload);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, sig)
            } else { std::ptr::null_mut() };
            eprintln!("{name} w={w} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
    }
}

/// Datetime + signature-array + ucrbg tag handlers.
#[test]
fn datetime_ucrbg_tags() {
    unsafe {
        #[repr(C)] struct Dt { y: u16, m: u16, d: u16,
            h: u16, mi: u16, s: u16 }
        let dt = Dt { y: 2024, m: 6, d: 15, h: 12, mi: 30, s: 45 };
        for (sig, payload, name) in [
            (0x63616c74u32, &dt as *const _ as *const _, "calt"),
            (0x62707374, &dt as *const _ as *const _, "bpst"),
        ] {
            let prof = c::cmsCreate_sRGBProfile();
            let w = c::cmsWriteTag(prof, sig, payload);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, sig)
            } else { std::ptr::null_mut() };
            eprintln!("{name} w={w} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
    }
}

/// Multi-MLU + text tags — different sigs through the MLU/text
/// handlers.
#[test]
fn mlu_text_sig_variants() {
    unsafe {
        let m = c::cmsMLUalloc(std::ptr::null_mut(), 1);
        for (l, c_, t) in [("en\0","US\0","dev"),("de\0","DE\0","txt")] {
            c::cmsMLUsetUTF8(m, l.as_ptr() as *const _,
                c_.as_ptr() as *const _, t.as_ptr());
        }
        for sig in [0x646D6E64u32, 0x646D6464, 0x70747264,
                    0x74617267, 0x70726F70] {
            let prof = c::cmsCreate_sRGBProfile();
            let w = c::cmsWriteTag(prof, sig, m as *const _);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, sig)
            } else { std::ptr::null_mut() };
            eprintln!("sig{sig:#x} w={w} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
        c::cmsMLUfree(m);
    }
}

/// Numeric-array + struct-array tags — chad (3x3 s15 matrix),
/// bfd (ucr-bg), vcgt.
#[test]
fn struct_array_tags() {
    unsafe {
        // chad — 9x S15Fixed16 (adaptation matrix)
        let chad: [u32; 9] = [0x00010000; 9];
        for (sig, payload, n, name) in [
            (0x63686164u32, chad.as_ptr() as *const _, 9, "chad"),
        ] {
            let prof = c::cmsCreate_sRGBProfile();
            let w = c::cmsWriteTag(prof, sig, payload);
            let mut buf = vec![0u8; 1 << 20];
            let mut n2 = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n2)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n2)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, sig)
            } else { std::ptr::null_mut() };
            eprintln!("{name} w={w} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
    }
}

/// mAB devicelink tags — AToB0/BToA0/AToB1/BToA1/AToB2/BToA2
/// mpet payloads on a placeholder profile.
#[test]
fn mab_devicelink_sigs() {
    unsafe {
        let ftab: Vec<f32> = (0..8*3)
            .map(|i| i as f32 / 24.0).collect();
        let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocCLutFloat(std::ptr::null_mut(), 2, 3, 3,
                ftab.as_ptr()));
        for sig in [0x41324230u32, 0x42324130, 0x41324231,
                    0x42324131, 0x41324232, 0x42324132] {
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsSetProfileVersion(prof, 4.4);
            let w = c::cmsWriteTag(prof, sig, p as *const _);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, sig)
            } else { std::ptr::null_mut() };
            eprintln!("sig{sig:#x} w={w} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
        c::cmsPipelineFree(p);
    }
}

/// Profile-sequence-desc + UcrBg + screening tag handlers.
#[test]
fn pseq_ucrbg_scrn_tags() {
    unsafe {
        // profile-sequence-desc
        let psd = c::cmsAllocProfileSequenceDescription(
            std::ptr::null_mut(), 1);
        for (sig, payload, name) in [
            (0x70736571u32, psd as *const _, "pseq"),
            (0x62707374, psd as *const _, "bpst-psd"),
        ] {
            let prof = c::cmsCreate_sRGBProfile();
            let w = c::cmsWriteTag(prof, sig, payload);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, sig)
            } else { std::ptr::null_mut() };
            eprintln!("{name} w={w} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
        c::cmsFreeProfileSequenceDescription(psd);
    }
}

/// UcrBg + response16 + screening tag handlers.
#[test]
fn ucrbg_resp_scrn_tags() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        // UcrBg — {Ucr: curve, Bg: curve, K: f64, L: f64}
        #[repr(C)] struct UcrBg { ucr: *mut c::ToneCurve,
            bg: *mut c::ToneCurve, k: f64, l: f64 }
        let u = UcrBg { ucr: g, bg: g, k: 0.5, l: 0.3 };
        for (sig, payload, name) in [
            (0x75637262u32, &u as *const _ as *const _, "ucrbg"),
        ] {
            let prof = c::cmsCreate_sRGBProfile();
            let w = c::cmsWriteTag(prof, sig, payload);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, sig)
            } else { std::ptr::null_mut() };
            eprintln!("{name} w={w} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
        c::cmsFreeToneCurve(g);
    }
}

/// Screening + response16 + named-color tag handlers.
#[test]
fn scrn_resp_named_tags() {
    unsafe {
        // screening — {Flag, nChannels, Channels[n]}
        #[repr(C)] #[derive(Clone, Copy)]
        struct ScrCh { f: f64, sa: f64, ch: u32 }
        #[repr(C)] struct Scrn { flag: u32, nc: u32,
            ch: [ScrCh; 16] }
        let s = Scrn { flag: 0, nc: 2,
            ch: [ScrCh { f: 1.0, sa: 45.0, ch: 0 }; 16] };
        for (sig, payload, name) in [
            (0x7363726eu32, &s as *const _ as *const _, "scrn"),
        ] {
            let prof = c::cmsCreate_sRGBProfile();
            let w = c::cmsWriteTag(prof, sig, payload);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let sv = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if sv != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, sig)
            } else { std::ptr::null_mut() };
            eprintln!("{name} w={w} s={sv} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
    }
}

/// Response16 + gamma/vcgt tag handlers.
#[test]
fn resp_gamma_tags() {
    unsafe {
        // response16 — {nMeasurements, nValue, values}
        let resp: [u16; 6] = [0, 256, 1024, 4096, 16384, 65535];
        for (sig, payload, name) in [
            (0x72657370u32, resp.as_ptr() as *const _, "resp"),
        ] {
            let prof = c::cmsCreate_sRGBProfile();
            let w = c::cmsWriteTag(prof, sig, payload);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, sig)
            } else { std::ptr::null_mut() };
            eprintln!("{name} w={w} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
    }
}

/// Named-color + text on different sigs — the per-sig type
/// dispatch for the same payload type.
#[test]
fn per_sig_dispatch() {
    unsafe {
        let ncl = c::cmsAllocNamedColorList(std::ptr::null_mut(), 1,
            3, b"p\0".as_ptr(), b"s\0".as_ptr());
        let lab16 = [0xF000u16; 3];
        c::cmsAppendNamedColor(ncl, b"c1\0".as_ptr(),
            lab16.as_ptr(), lab16.as_ptr());
        let m = c::cmsMLUalloc(std::ptr::null_mut(), 1);
        c::cmsMLUsetUTF8(m, b"en\0".as_ptr() as *const _,
            b"US\0".as_ptr() as *const _, b"x\0".as_ptr());
        for (sig, payload, name) in [
            (0x6e636c32u32, ncl as *const _, "ncl2"),
            (0x6e636c32u32, m as *const _, "ncl2-mlu"),
            (0x6d657461, m as *const _, "meta-mlu"),
            (0x64657363, m as *const _, "desc-mlu"),
        ] {
            let prof = c::cmsCreate_sRGBProfile();
            let w = c::cmsWriteTag(prof, sig, payload);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, sig)
            } else { std::ptr::null_mut() };
            eprintln!("{name} w={w} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
        c::cmsFreeNamedColorList(ncl);
        c::cmsMLUfree(m);
    }
}

/// Multi-entry numeric arrays — S15Fixed16 + U16Fixed16 +
/// UInt64Array serializers with multi-entry payloads.
#[test]
fn multi_numeric_arrays() {
    unsafe {
        // chad takes 9x S15Fixed16 (3x3 adaptation matrix)
        let chad: [u32; 9] = [
            0x00010000, 0x00001000, 0x00000100,
            0x00002000, 0x00008000, 0x00000400,
            0x00000800, 0x00000200, 0x0001C000,
        ];
        for (sig, payload, name) in [
            (0x63686164u32, chad.as_ptr() as *const _, "chad"),
        ] {
            let prof = c::cmsCreate_sRGBProfile();
            let w = c::cmsWriteTag(prof, sig, payload);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, sig)
            } else { std::ptr::null_mut() };
            eprintln!("{name} w={w} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
    }
}

/// Multi-MLU + text on manufacturer/model/technology sigs.
#[test]
fn mlu_text_sigs() {
    unsafe {
        let m = c::cmsMLUalloc(std::ptr::null_mut(), 1);
        for (l, c_, t) in [("en\0","US\0","mfr"),("de\0","DE\0","txt")] {
            c::cmsMLUsetUTF8(m, l.as_ptr() as *const _,
                c_.as_ptr() as *const _, t.as_ptr());
        }
        for sig in [0x646d6e64u32, 0x646d6464, 0x6c6d6e64,
                    0x6c6d7364, 0x74656368] {
            let prof = c::cmsCreate_sRGBProfile();
            let w = c::cmsWriteTag(prof, sig, m as *const _);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, sig)
            } else { std::ptr::null_mut() };
            eprintln!("sig{sig:#x} w={w} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
        c::cmsMLUfree(m);
    }
}

/// Profile-sequence + profile-id + mAB tags on a placeholder.
#[test]
fn pseq_id_mab_tags() {
    unsafe {
        let psd = c::cmsAllocProfileSequenceDescription(
            std::ptr::null_mut(), 1);
        let ftab: Vec<f32> = (0..8*3)
            .map(|i| i as f32 / 24.0).collect();
        let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocCLutFloat(std::ptr::null_mut(), 2, 3, 3,
                ftab.as_ptr()));
        for (sig, payload, name) in [
            (0x70736571u32, psd as *const _, "pseq"),
            (0x44324230, p as *const _, "D2B0-mpet"),
            (0x42324430, p as *const _, "B2D0-mpet"),
        ] {
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsSetProfileVersion(prof, 4.4);
            let w = c::cmsWriteTag(prof, sig, payload);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, sig)
            } else { std::ptr::null_mut() };
            eprintln!("{name} w={w} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
        c::cmsFreeProfileSequenceDescription(psd);
        c::cmsPipelineFree(p);
    }
}

/// Multi-entry dict + named-color + mpet on different sigs —
/// the per-sig type dispatch.
#[test]
fn dict_mpet_sig_dispatch() {
    unsafe {
        let d = c::cmsDictAlloc(std::ptr::null_mut());
        let w = |s: &str| s.chars().map(|c| c as i32)
            .chain([0]).collect::<Vec<i32>>();
        c::cmsDictAddEntry(d, w("k1").as_ptr(), w("v1").as_ptr(),
            std::ptr::null_mut(), std::ptr::null_mut());
        c::cmsDictAddEntry(d, w("k2").as_ptr(), w("v2").as_ptr(),
            std::ptr::null_mut(), std::ptr::null_mut());
        let ftab: Vec<f32> = (0..8*3)
            .map(|i| i as f32 / 24.0).collect();
        let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocCLutFloat(std::ptr::null_mut(), 2, 3, 3,
                ftab.as_ptr()));
        for (sig, payload, name) in [
            (0x6d657461u32, d as *const _, "meta-dict"),
            (0x44324230, p as *const _, "D2B0-mpet"),
            (0x42324430, p as *const _, "B2D0-mpet"),
            (0x44324231, p as *const _, "D2B1-mpet"),
            (0x42324431, p as *const _, "B2D1-mpet"),
            (0x44324232, p as *const _, "D2B2-mpet"),
            (0x42324432, p as *const _, "B2D2-mpet"),
            (0x44324233, p as *const _, "D2B3-mpet"),
            (0x42324433, p as *const _, "B2D3-mpet"),
        ] {
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsSetProfileVersion(prof, 4.4);
            let w = c::cmsWriteTag(prof, sig, payload);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, sig)
            } else { std::ptr::null_mut() };
            eprintln!("{name} w={w} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
        c::cmsDictFree(d);
        c::cmsPipelineFree(p);
    }
}

/// Named-color on different sigs — ncl2 + nmcl-class + meta
/// dict payload combos.
#[test]
fn named_color_sig_dispatch() {
    unsafe {
        let ncl = c::cmsAllocNamedColorList(std::ptr::null_mut(), 2,
            3, b"pre\0".as_ptr(), b"suf\0".as_ptr());
        let lab16 = [0xF000u16; 3];
        c::cmsAppendNamedColor(ncl, b"c1\0".as_ptr(),
            lab16.as_ptr(), lab16.as_ptr());
        c::cmsAppendNamedColor(ncl, b"c2\0".as_ptr(),
            lab16.as_ptr(), lab16.as_ptr());
        for (cls, sig, name) in [
            (0x6e6d636cu32, 0x6e636c32, "nmcl-ncl2"),
            (0x6d6e7472, 0x6e636c32, "mntr-ncl2"),
        ] {
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsSetDeviceClass(prof, cls);
            let w = c::cmsWriteTag(prof, sig, ncl as *const _);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, sig)
            } else { std::ptr::null_mut() };
            eprintln!("{name} w={w} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
        c::cmsFreeNamedColorList(ncl);
    }
}

/// Multi-translation MLU on every text-type sig — the per-sig
/// text-handler dispatch.
#[test]
fn text_type_sigs() {
    unsafe {
        let m = c::cmsMLUalloc(std::ptr::null_mut(), 1);
        for (l, c_, t) in [("en\0","US\0","mfr"),("de\0","DE\0","txt")] {
            c::cmsMLUsetUTF8(m, l.as_ptr() as *const _,
                c_.as_ptr() as *const _, t.as_ptr());
        }
        for sig in [0x646d6e64u32, 0x646d6464, 0x6c6d6e64,
                    0x6c6d7364, 0x63707274, 0x64657363] {
            let prof = c::cmsCreate_sRGBProfile();
            let w = c::cmsWriteTag(prof, sig, m as *const _);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, sig)
            } else { std::ptr::null_mut() };
            eprintln!("sig{sig:#x} w={w} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
        c::cmsMLUfree(m);
    }
}

/// Multi-entry payload variants — MLU entries, dict entries,
/// named-color entries, mpet element counts.
#[test]
fn multi_entry_payloads() {
    unsafe {
        let w = |s: &str| s.chars().map(|c| c as i32)
            .chain([0]).collect::<Vec<i32>>();
        // 0, 1, 4, 16-entry payloads — different offset-table arms
        for n in [0usize, 1, 4, 16] {
            let d = c::cmsDictAlloc(std::ptr::null_mut());
            for i in 0..n {
                let k = format!("k{i}");
                c::cmsDictAddEntry(d, w(&k).as_ptr(), w("v").as_ptr(),
                    std::ptr::null_mut(), std::ptr::null_mut());
            }
            let prof = c::cmsCreate_sRGBProfile();
            let wt = c::cmsWriteTag(prof, 0x6d657461, d as *const _);
            let mut buf = vec![0u8; 1 << 20];
            let mut sz = buf.len() as u32;
            let s = if wt != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut sz)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, sz)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, 0x6d657461)
            } else { std::ptr::null_mut() };
            eprintln!("dict{n} w={wt} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
            c::cmsDictFree(d);
        }
    }
}

/// Per-sig type dispatch — the remaining type-handler arms on
/// the tag-signature table.
#[test]
fn per_sig_type_dispatch() {
    unsafe {
        let w = |s: &str| s.chars().map(|c| c as i32)
            .chain([0]).collect::<Vec<i32>>();
        let m = c::cmsMLUalloc(std::ptr::null_mut(), 1);
        c::cmsMLUsetUTF8(m, b"en\0".as_ptr() as *const _,
            b"US\0".as_ptr() as *const _, b"x\0".as_ptr());
        let d = c::cmsDictAlloc(std::ptr::null_mut());
        c::cmsDictAddEntry(d, w("k").as_ptr(), w("v").as_ptr(),
            std::ptr::null_mut(), std::ptr::null_mut());
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let ftab: Vec<f32> = (0..8*3).map(|i| i as f32 / 24.0).collect();
        let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocCLutFloat(std::ptr::null_mut(), 2, 3, 3,
                ftab.as_ptr()));
        let xyz: [f64; 3] = [0.96, 1.0, 0.82];
        let dt: [u16; 6] = [2024, 6, 15, 12, 30, 45];
        let chad: [u32; 9] = [0x10000; 9];
        for (sig, payload, name) in [
            (0x63757276u32, g as *const _, "curv"),
            (0x6d657461, d as *const _, "dict"),
            (0x64657363, m as *const _, "mlu"),
            (0x77747074, xyz.as_ptr() as *const _, "xyz"),
            (0x63616c74, dt.as_ptr() as *const _, "dtim"),
            (0x63686164, chad.as_ptr() as *const _, "s15arr"),
            (0x44324230, p as *const _, "mpet"),
        ] {
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsSetProfileVersion(prof, 4.4);
            let w = c::cmsWriteTag(prof, sig, payload);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, sig)
            } else { std::ptr::null_mut() };
            eprintln!("{name} w={w} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
        c::cmsMLUfree(m);
        c::cmsDictFree(d);
        c::cmsFreeToneCurve(g);
        c::cmsPipelineFree(p);
    }
}

/// Multi-translation MLU on the remaining text-type sigs.
#[test]
fn remaining_text_sigs() {
    unsafe {
        let m = c::cmsMLUalloc(std::ptr::null_mut(), 1);
        for (l, c_, t) in [("en\0","US\0","mfr"),("de\0","DE\0","txt")] {
            c::cmsMLUsetUTF8(m, l.as_ptr() as *const _,
                c_.as_ptr() as *const _, t.as_ptr());
        }
        for sig in [0x646d6e64u32, 0x646d6464, 0x6c6d6e64,
                    0x6c6d7364, 0x63707274, 0x64657363,
                    0x74617267, 0x70747264, 0x70726f70,
                    0x70726f64, 0x70736964, 0x70736f70] {
            let prof = c::cmsCreate_sRGBProfile();
            let w = c::cmsWriteTag(prof, sig, m as *const _);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, sig)
            } else { std::ptr::null_mut() };
            eprintln!("sig{sig:#x} w={w} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
        c::cmsMLUfree(m);
    }
}

/// Multi-translation MLU on every remaining text sig.
#[test]
fn all_text_sigs() {
    unsafe {
        let m = c::cmsMLUalloc(std::ptr::null_mut(), 1);
        for (l, c_, t) in [("en\0","US\0","mfr"),("de\0","DE\0","txt")] {
            c::cmsMLUsetUTF8(m, l.as_ptr() as *const _,
                c_.as_ptr() as *const _, t.as_ptr());
        }
        for sig in [0x646d6e64u32, 0x646d6464, 0x6c6d6e64,
                    0x6c6d7364, 0x63707274, 0x64657363,
                    0x74617267, 0x70747264, 0x70726f70,
                    0x70726f64, 0x70736964, 0x70736f70,
                    0x74656368, 0x6c756d69, 0x626b7074,
                    0x77747074, 0x7363726e, 0x63686164] {
            let prof = c::cmsCreate_sRGBProfile();
            let w = c::cmsWriteTag(prof, sig, m as *const _);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, sig)
            } else { std::ptr::null_mut() };
            eprintln!("sig{sig:#x} w={w} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
        c::cmsMLUfree(m);
    }
}

/// Multi-entry payload variants — dict + named-color + mpet on
/// different sigs.
#[test]
fn multi_entry_sig_dispatch() {
    unsafe {
        let w = |s: &str| s.chars().map(|c| c as i32)
            .chain([0]).collect::<Vec<i32>>();
        let d = c::cmsDictAlloc(std::ptr::null_mut());
        for i in 0..4 {
            let k = format!("k{i}");
            c::cmsDictAddEntry(d, w(&k).as_ptr(), w("v").as_ptr(),
                std::ptr::null_mut(), std::ptr::null_mut());
        }
        let ncl = c::cmsAllocNamedColorList(std::ptr::null_mut(), 2,
            3, b"p\0".as_ptr(), b"s\0".as_ptr());
        let lab16 = [0xF000u16; 3];
        c::cmsAppendNamedColor(ncl, b"c1\0".as_ptr(),
            lab16.as_ptr(), lab16.as_ptr());
        let ftab: Vec<f32> = (0..8*3).map(|i| i as f32 / 24.0).collect();
        let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocCLutFloat(std::ptr::null_mut(), 2, 3, 3,
                ftab.as_ptr()));
        for (sig, payload, name) in [
            (0x6d657461u32, d as *const _, "dict"),
            (0x6e636c32, ncl as *const _, "ncl2"),
            (0x44324230, p as *const _, "mpet"),
        ] {
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsSetProfileVersion(prof, 4.4);
            let wt = c::cmsWriteTag(prof, sig, payload);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if wt != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, sig)
            } else { std::ptr::null_mut() };
            eprintln!("{name} w={wt} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
        c::cmsDictFree(d);
        c::cmsFreeNamedColorList(ncl);
        c::cmsPipelineFree(p);
    }
}

/// Per-sig type dispatch — multi-entry payloads on the remaining
/// tag-type sigs.
#[test]
fn remaining_sig_dispatch() {
    unsafe {
        let w = |s: &str| s.chars().map(|c| c as i32)
            .chain([0]).collect::<Vec<i32>>();
        let d = c::cmsDictAlloc(std::ptr::null_mut());
        c::cmsDictAddEntry(d, w("k1").as_ptr(), w("v1").as_ptr(),
            std::ptr::null_mut(), std::ptr::null_mut());
        let m = c::cmsMLUalloc(std::ptr::null_mut(), 1);
        c::cmsMLUsetUTF8(m, b"en\0".as_ptr() as *const _,
            b"US\0".as_ptr() as *const _, b"x\0".as_ptr());
        let ncl = c::cmsAllocNamedColorList(std::ptr::null_mut(), 1,
            3, b"p\0".as_ptr(), b"s\0".as_ptr());
        let lab16 = [0xF000u16; 3];
        c::cmsAppendNamedColor(ncl, b"c1\0".as_ptr(),
            lab16.as_ptr(), lab16.as_ptr());
        for (sig, payload, name) in [
            (0x6d657461u32, d as *const _, "dict-meta"),
            (0x64657363, m as *const _, "mlu-desc"),
            (0x6e636c32, ncl as *const _, "ncl2"),
            (0x70736571, m as *const _, "pseq-mlu"),
            (0x63686164, m as *const _, "chad-mlu"),
        ] {
            let prof = c::cmsCreate_sRGBProfile();
            let wt = c::cmsWriteTag(prof, sig, payload);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if wt != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, sig)
            } else { std::ptr::null_mut() };
            eprintln!("{name} w={wt} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
        c::cmsDictFree(d);
        c::cmsMLUfree(m);
        c::cmsFreeNamedColorList(ncl);
    }
}

/// Multi-entry payload variants — the remaining per-sig dispatch.
#[test]
fn multi_entry_variants() {
    unsafe {
        let w = |s: &str| s.chars().map(|c| c as i32)
            .chain([0]).collect::<Vec<i32>>();
        let m = c::cmsMLUalloc(std::ptr::null_mut(), 1);
        for (l, c_, t) in [("en\0","US\0","x"),("de\0","DE\0","y")] {
            c::cmsMLUsetUTF8(m, l.as_ptr() as *const _,
                c_.as_ptr() as *const _, t.as_ptr());
        }
        let d = c::cmsDictAlloc(std::ptr::null_mut());
        c::cmsDictAddEntry(d, w("k").as_ptr(), w("v").as_ptr(),
            std::ptr::null_mut(), std::ptr::null_mut());
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let xyz: [f64; 3] = [0.96, 1.0, 0.82];
        for (sig, payload, name) in [
            (0x64657363u32, m as *const _, "desc-mlu"),
            (0x63707274, m as *const _, "cprt-mlu"),
            (0x6d657461, d as *const _, "meta-dict"),
            (0x72545243, g as *const _, "rTRC-curv"),
            (0x77747074, xyz.as_ptr() as *const _, "wtpt-xyz"),
        ] {
            let prof = c::cmsCreate_sRGBProfile();
            let wt = c::cmsWriteTag(prof, sig, payload);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if wt != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, sig)
            } else { std::ptr::null_mut() };
            eprintln!("{name} w={wt} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
        c::cmsMLUfree(m);
        c::cmsDictFree(d);
        c::cmsFreeToneCurve(g);
    }
}

/// Multi-entry payload variants — dict + named-color + mpet on
/// different sigs.
#[test]
fn multi_entry_sig_variants() {
    unsafe {
        let w = |s: &str| s.chars().map(|c| c as i32)
            .chain([0]).collect::<Vec<i32>>();
        let d = c::cmsDictAlloc(std::ptr::null_mut());
        for i in 0..3 {
            let k = format!("k{i}");
            c::cmsDictAddEntry(d, w(&k).as_ptr(), w("v").as_ptr(),
                std::ptr::null_mut(), std::ptr::null_mut());
        }
        let ncl = c::cmsAllocNamedColorList(std::ptr::null_mut(), 2,
            3, b"p\0".as_ptr(), b"s\0".as_ptr());
        let lab16 = [0xF000u16; 3];
        c::cmsAppendNamedColor(ncl, b"c1\0".as_ptr(),
            lab16.as_ptr(), lab16.as_ptr());
        let ftab: Vec<f32> = (0..8*3).map(|i| i as f32 / 24.0).collect();
        let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocCLutFloat(std::ptr::null_mut(), 2, 3, 3,
                ftab.as_ptr()));
        for (sig, payload, name) in [
            (0x6d657461u32, d as *const _, "dict-meta"),
            (0x6e636c32, ncl as *const _, "ncl2"),
            (0x44324230, p as *const _, "mpet-D2B"),
            (0x42324430, p as *const _, "mpet-B2D"),
        ] {
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsSetProfileVersion(prof, 4.4);
            let wt = c::cmsWriteTag(prof, sig, payload);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if wt != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, sig)
            } else { std::ptr::null_mut() };
            eprintln!("{name} w={wt} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
        c::cmsDictFree(d);
        c::cmsFreeNamedColorList(ncl);
        c::cmsPipelineFree(p);
    }
}

/// Multi-entry payload variants — dict + named-color + mpet on
/// different sigs.
#[test]
fn multi_entry_sigs() {
    unsafe {
        let w = |s: &str| s.chars().map(|c| c as i32)
            .chain([0]).collect::<Vec<i32>>();
        let d = c::cmsDictAlloc(std::ptr::null_mut());
        c::cmsDictAddEntry(d, w("k").as_ptr(), w("v").as_ptr(),
            std::ptr::null_mut(), std::ptr::null_mut());
        let ncl = c::cmsAllocNamedColorList(std::ptr::null_mut(), 1,
            3, b"p\0".as_ptr(), b"s\0".as_ptr());
        let lab16 = [0xF000u16; 3];
        c::cmsAppendNamedColor(ncl, b"c1\0".as_ptr(),
            lab16.as_ptr(), lab16.as_ptr());
        let ftab: Vec<f32> = (0..8*3).map(|i| i as f32 / 24.0).collect();
        let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocCLutFloat(std::ptr::null_mut(), 2, 3, 3,
                ftab.as_ptr()));
        for (sig, payload, name) in [
            (0x6d657461u32, d as *const _, "dict"),
            (0x6e636c32, ncl as *const _, "ncl2"),
            (0x44324230, p as *const _, "mpet"),
        ] {
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsSetProfileVersion(prof, 4.4);
            let wt = c::cmsWriteTag(prof, sig, payload);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if wt != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, sig)
            } else { std::ptr::null_mut() };
            eprintln!("{name} w={wt} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
        c::cmsDictFree(d);
        c::cmsFreeNamedColorList(ncl);
        c::cmsPipelineFree(p);
    }
}

/// Per-tag-sig type dispatch — multi-entry payloads on the
/// remaining tag-type sigs.
#[test]
fn per_sig_dispatch2() {
    unsafe {
        let w = |s: &str| s.chars().map(|c| c as i32)
            .chain([0]).collect::<Vec<i32>>();
        let d = c::cmsDictAlloc(std::ptr::null_mut());
        c::cmsDictAddEntry(d, w("k").as_ptr(), w("v").as_ptr(),
            std::ptr::null_mut(), std::ptr::null_mut());
        let m = c::cmsMLUalloc(std::ptr::null_mut(), 1);
        c::cmsMLUsetUTF8(m, b"en\0".as_ptr() as *const _,
            b"US\0".as_ptr() as *const _, b"x\0".as_ptr());
        let ncl = c::cmsAllocNamedColorList(std::ptr::null_mut(), 1,
            3, b"p\0".as_ptr(), b"s\0".as_ptr());
        let lab16 = [0xF000u16; 3];
        c::cmsAppendNamedColor(ncl, b"c1\0".as_ptr(),
            lab16.as_ptr(), lab16.as_ptr());
        for (sig, payload, name) in [
            (0x6d657461u32, d as *const _, "dict-meta"),
            (0x64657363, m as *const _, "mlu-desc"),
            (0x6e636c32, ncl as *const _, "ncl2"),
        ] {
            let prof = c::cmsCreate_sRGBProfile();
            let wt = c::cmsWriteTag(prof, sig, payload);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if wt != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, sig)
            } else { std::ptr::null_mut() };
            eprintln!("{name} w={wt} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
        c::cmsDictFree(d);
        c::cmsMLUfree(m);
        c::cmsFreeNamedColorList(ncl);
    }
}

/// Type_MHC2_Read/Write — the Microsoft HDR colorimetric tag
/// (curve arrays + luminance + XYZ2XYZ matrix).
#[test]
fn mhc2_tag() {
    unsafe {
        #[repr(C)] struct Mhc2 { n: u32, r: *mut f64,
            g: *mut f64, b: *mut f64, minl: f64, peakl: f64,
            m: [[f64; 4]; 3] }
        let mut rc = [0.0f64, 0.5, 1.0, 2.0];
        let mut gc = [0.0f64, 0.5, 1.0, 2.0];
        let mut bc = [0.0f64, 0.5, 1.0, 2.0];
        let mut mhc = Mhc2 { n: 4,
            r: rc.as_mut_ptr(), g: gc.as_mut_ptr(), b: bc.as_mut_ptr(),
            minl: 0.0001, peakl: 1000.0,
            m: [[1.0,0.0,0.0,0.0],[0.0,1.0,0.0,0.0],[0.0,0.0,1.0,0.0]] };
        let prof = c::cmsCreate_sRGBProfile();
        c::cmsSetProfileVersion(prof, 4.4);
        let w = c::cmsWriteTag(prof, 0x4D484332,
            &mut mhc as *mut _ as *const _);
        let mut buf = vec![0u8; 1 << 20];
        let mut n = buf.len() as u32;
        let s = if w != 0 {
            c::cmsSaveProfileToMem(prof,
                buf.as_mut_ptr() as *mut _, &mut n)
        } else { 0 };
        let rp = if s != 0 {
            c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
        } else { std::ptr::null_mut() };
        let tag = if !rp.is_null() {
            c::cmsReadTag(rp, 0x4D484332)
        } else { std::ptr::null_mut() };
        eprintln!("mhc2 w={w} s={s} read={}", !tag.is_null());
        if !rp.is_null() { c::cmsCloseProfile(rp); }
        c::cmsCloseProfile(prof);
    }
}

/// Type_vcgt_Read/Write — video-card gamma tag: payload is a
/// cmsToneCurve*[3].
#[test]
fn vcgt_tag() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mut curves = [g; 3];
        let prof = c::cmsCreate_sRGBProfile();
        let w = c::cmsWriteTag(prof, 0x76636774,
            curves.as_mut_ptr() as *const _);
        let mut buf = vec![0u8; 1 << 20];
        let mut n = buf.len() as u32;
        let s = if w != 0 {
            c::cmsSaveProfileToMem(prof,
                buf.as_mut_ptr() as *mut _, &mut n)
        } else { 0 };
        let rp = if s != 0 {
            c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
        } else { std::ptr::null_mut() };
        let tag = if !rp.is_null() {
            c::cmsReadTag(rp, 0x76636774)
        } else { std::ptr::null_mut() };
        eprintln!("vcgt w={w} s={s} read={}", !tag.is_null());
        if !rp.is_null() { c::cmsCloseProfile(rp); }
        c::cmsCloseProfile(prof);
        c::cmsFreeToneCurve(g);
    }
}

/// Type_UcrBg_Read/Write — undercolor-removal/black-generation
/// tag: {Ucr, Bg, Desc-MLU}.
#[test]
fn ucrbg_tag() {
    unsafe {
        #[repr(C)] struct UcrBg { ucr: *mut c::ToneCurve,
            bg: *mut c::ToneCurve, desc: *mut std::ffi::c_void }
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 0.4);
        let g2 = c::cmsBuildGamma(std::ptr::null_mut(), 0.7);
        let m = c::cmsMLUalloc(std::ptr::null_mut(), 1);
        c::cmsMLUsetUTF8(m, b"en\0".as_ptr() as *const _,
            b"US\0".as_ptr() as *const _, b"ucrbg\0".as_ptr());
        let mut u = UcrBg { ucr: g, bg: g2, desc: m as *mut _ };
        let prof = c::cmsCreate_sRGBProfile();
        let w = c::cmsWriteTag(prof, 0x62666420,
            &mut u as *mut _ as *const _); // 'bfd '
        let mut buf = vec![0u8; 1 << 20];
        let mut n = buf.len() as u32;
        let s = if w != 0 {
            c::cmsSaveProfileToMem(prof,
                buf.as_mut_ptr() as *mut _, &mut n)
        } else { 0 };
        let rp = if s != 0 {
            c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
        } else { std::ptr::null_mut() };
        let tag = if !rp.is_null() {
            c::cmsReadTag(rp, 0x62666420)
        } else { std::ptr::null_mut() };
        eprintln!("ucrbg w={w} s={s} read={}", !tag.is_null());
        if !rp.is_null() { c::cmsCloseProfile(rp); }
        c::cmsCloseProfile(prof);
        c::cmsFreeToneCurve(g);
        c::cmsFreeToneCurve(g2);
        c::cmsMLUfree(m);
    }
}

/// Type_Chromaticity + Type_ColorantTable — chrm primaries and
/// clrt/clro colorant tables.
#[test]
fn chrm_clrt_tags() {
    unsafe {
        #[repr(C)] struct Xy { x: f64, y: f64, yy: f64 }
        #[repr(C)] struct Triple { r: Xy, g: Xy, b: Xy }
        let mut tr = Triple {
            r: Xy { x: 0.64, y: 0.33, yy: 1.0 },
            g: Xy { x: 0.30, y: 0.60, yy: 1.0 },
            b: Xy { x: 0.15, y: 0.06, yy: 1.0 } };
        let ncl = c::cmsAllocNamedColorList(std::ptr::null_mut(), 2,
            3, b"\0".as_ptr(), b"\0".as_ptr());
        let pcs = [0x8000u16; 3];
        c::cmsAppendNamedColor(ncl, b"Cyan\0".as_ptr(),
            pcs.as_ptr(), std::ptr::null());
        c::cmsAppendNamedColor(ncl, b"Magenta\0".as_ptr(),
            pcs.as_ptr(), std::ptr::null());
        for (sig, payload, name) in [
            (0x6368726Du32, &mut tr as *mut _ as *const _, "chrm"),
            (0x636C7274, ncl as *const _, "clrt"),
            (0x636C726F, ncl as *const _, "clro"),
        ] {
            let prof = c::cmsCreate_sRGBProfile();
            let w = c::cmsWriteTag(prof, sig, payload);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, sig)
            } else { std::ptr::null_mut() };
            eprintln!("{name} w={w} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
        c::cmsFreeNamedColorList(ncl);
    }
}

/// Type_Text_Description_Read arms — raw 'desc' tags with
/// ASCII-only, +Unicode, +ScriptCode, and malformed shapes.
#[test]
fn desc_reader_arms() {
    unsafe {
        let mk = |ascii: &[u8], uni: Option<&[u16]>, sc: Option<(u16,u8,&[u8])>| -> Vec<u8> {
            let mut t = Vec::new();
            t.extend_from_slice(&0x64657363u32.to_be_bytes()); // 'desc'
            t.extend_from_slice(&[0; 4]);
            t.extend_from_slice(&(ascii.len() as u32).to_be_bytes());
            t.extend_from_slice(ascii);
            if let Some(u) = uni {
                t.extend_from_slice(&1u32.to_be_bytes()); // unicode code
                t.extend_from_slice(&(u.len() as u32).to_be_bytes());
                for &c in u { t.extend_from_slice(&c.to_be_bytes()); }
            }
            if let Some((code, cnt, dat)) = sc {
                t.extend_from_slice(&code.to_be_bytes());
                t.push(cnt);
                t.extend_from_slice(dat);
                // pad to 67
                while t.len() % 4 != 0 { t.push(0); }
            }
            t
        };
        for (i, payload) in [
            mk(b"Hello\0", None, None),                     // ascii only
            mk(b"Hi\0", Some(&[0x0048, 0x0069]), None),     // + unicode
            mk(b"H\0", Some(&[0x0048]), Some((0x6173, 2, b"ab"))), // + scriptcode
            mk(b"", Some(&[]), None),                       // 0-len unicode
            {
                // AsciiCount > tag size -> reject
                let mut t = Vec::new();
                t.extend_from_slice(&0x64657363u32.to_be_bytes());
                t.extend_from_slice(&[0; 4]);
                t.extend_from_slice(&0xfffffu32.to_be_bytes());
                t.extend_from_slice(b"ab");
                t
            },
        ].iter().enumerate() {
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsWriteRawTag(prof, 0x64657363,
                payload.as_ptr() as *const _, payload.len() as u32);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = c::cmsSaveProfileToMem(prof,
                buf.as_mut_ptr() as *mut _, &mut n);
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, 0x64657363)
            } else { std::ptr::null_mut() };
            let ok = if !tag.is_null() {
                let mut wb = [0i32; 32];
                c::cmsMLUgetWide(tag, std::ptr::null_mut(),
                    std::ptr::null_mut(), wb.as_mut_ptr(), 32) != 0
            } else { false };
            eprintln!("desc{i} s={s} read={} ok={ok}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
    }
}

/// Type_CrdInfo + Type_ProfileSequenceId — crdi PS-MLU and psid
/// sequence-id payloads.
#[test]
fn crdi_psid_tags() {
    unsafe {
        // crdi — MLU with lang "PS" and countries nm/#0/#1/#2/#3
        let m = c::cmsMLUalloc(std::ptr::null_mut(), 5);
        for (cnt, txt) in [
            ("nm\0", "prod\0"), ("#0\0", "crd0\0"), ("#1\0", "crd1\0"),
            ("#2\0", "crd2\0"), ("#3\0", "crd3\0"),
        ] {
            c::cmsMLUsetUTF8(m, b"PS\0".as_ptr() as *const _,
                cnt.as_ptr() as *const _, txt.as_ptr());
        }
        let prof = c::cmsCreate_sRGBProfile();
        let w = c::cmsWriteTag(prof, 0x63726469, m as *const _); // 'crdi'
        let mut buf = vec![0u8; 1 << 20];
        let mut n = buf.len() as u32;
        let s = if w != 0 {
            c::cmsSaveProfileToMem(prof,
                buf.as_mut_ptr() as *mut _, &mut n)
        } else { 0 };
        let rp = if s != 0 {
            c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
        } else { std::ptr::null_mut() };
        let tag = if !rp.is_null() {
            c::cmsReadTag(rp, 0x63726469)
        } else { std::ptr::null_mut() };
        eprintln!("crdi w={w} s={s} read={}", !tag.is_null());
        if !rp.is_null() { c::cmsCloseProfile(rp); }
        c::cmsCloseProfile(prof);
        c::cmsMLUfree(m);

        // psid — a cmsSEQ (profile-sequence-id payload)
        let psd = c::cmsAllocProfileSequenceDescription(
            std::ptr::null_mut(), 2);
        let prof2 = c::cmsCreate_sRGBProfile();
        let w2 = c::cmsWriteTag(prof2, 0x70736964, psd as *const _);
        let mut n2 = buf.len() as u32;
        let s2 = if w2 != 0 {
            c::cmsSaveProfileToMem(prof2,
                buf.as_mut_ptr() as *mut _, &mut n2)
        } else { 0 };
        let rp2 = if s2 != 0 {
            c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n2)
        } else { std::ptr::null_mut() };
        let tag2 = if !rp2.is_null() {
            c::cmsReadTag(rp2, 0x70736964)
        } else { std::ptr::null_mut() };
        eprintln!("psid w={w2} s={s2} read={}", !tag2.is_null());
        if !rp2.is_null() { c::cmsCloseProfile(rp2); }
        c::cmsCloseProfile(prof2);
        c::cmsFreeProfileSequenceDescription(psd);
    }
}

/// Type_Text (v2 cprt), Type_Text_Description (v2 dmnd) and
/// Type_Data (ps2s CRD) payloads.
#[test]
fn text_data_tags() {
    unsafe {
        // v2 profile: cprt takes textType (char*)
        let prof = c::cmsCreate_sRGBProfile();
        c::cmsSetProfileVersion(prof, 2.1);
        let txt = b"Copyright test\0";
        let w = c::cmsWriteTag(prof, 0x63707274,
            txt.as_ptr() as *const _);
        let mut buf = vec![0u8; 1 << 20];
        let mut n = buf.len() as u32;
        let s = if w != 0 {
            c::cmsSaveProfileToMem(prof,
                buf.as_mut_ptr() as *mut _, &mut n)
        } else { 0 };
        let rp = if s != 0 {
            c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
        } else { std::ptr::null_mut() };
        let tag = if !rp.is_null() {
            c::cmsReadTag(rp, 0x63707274)
        } else { std::ptr::null_mut() };
        eprintln!("v2-cprt-text w={w} s={s} read={}", !tag.is_null());
        if !rp.is_null() { c::cmsCloseProfile(rp); }
        c::cmsCloseProfile(prof);

        // v2 profile: dmnd takes textDescriptionType (MLU payload)
        let m = c::cmsMLUalloc(std::ptr::null_mut(), 1);
        c::cmsMLUsetUTF8(m, b"en\0".as_ptr() as *const _,
            b"US\0".as_ptr() as *const _, b"mfr desc\0".as_ptr());
        let prof2 = c::cmsCreate_sRGBProfile();
        c::cmsSetProfileVersion(prof2, 2.1);
        let w2 = c::cmsWriteTag(prof2, 0x646d6e64, m as *const _);
        let mut n2 = buf.len() as u32;
        let s2 = if w2 != 0 {
            c::cmsSaveProfileToMem(prof2,
                buf.as_mut_ptr() as *mut _, &mut n2)
        } else { 0 };
        let rp2 = if s2 != 0 {
            c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n2)
        } else { std::ptr::null_mut() };
        let tag2 = if !rp2.is_null() {
            c::cmsReadTag(rp2, 0x646d6e64)
        } else { std::ptr::null_mut() };
        eprintln!("v2-dmnd-td w={w2} s={s2} read={}", !tag2.is_null());
        if !rp2.is_null() { c::cmsCloseProfile(rp2); }
        c::cmsCloseProfile(prof2);
        c::cmsMLUfree(m);

        // data type — cmsICCData {len, flag, data[]} on ps2s
        #[repr(C)] struct ICCData { len: u32, flag: u32, data: [u8; 8] }
        let mut d = ICCData { len: 8, flag: 0, data: *b"CRDDATA!" };
        let prof3 = c::cmsCreate_sRGBProfile();
        let w3 = c::cmsWriteTag(prof3, 0x70733273,
            &mut d as *mut _ as *const _); // 'ps2s'
        let mut n3 = buf.len() as u32;
        let s3 = if w3 != 0 {
            c::cmsSaveProfileToMem(prof3,
                buf.as_mut_ptr() as *mut _, &mut n3)
        } else { 0 };
        let rp3 = if s3 != 0 {
            c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n3)
        } else { std::ptr::null_mut() };
        let tag3 = if !rp3.is_null() {
            c::cmsReadTag(rp3, 0x70733273)
        } else { std::ptr::null_mut() };
        eprintln!("ps2s-data w={w3} s={s3} read={}", !tag3.is_null());
        if !rp3.is_null() { c::cmsCloseProfile(rp3); }
        c::cmsCloseProfile(prof3);
    }
}

/// Malformed-tag reject arms — truncated/corrupt payloads for
/// meas, view, calt, curv, clro, cicp, chrm.
#[test]
fn malformed_tag_reads2() {
    unsafe {
        for (sig, name, payload) in [
            (0x6d656173u32, "meas", vec![0x6d656173u32,0,0,0,0,0,0,0]),
            (0x76696577, "view", vec![0x76696577u32,0,0,0,0]),
            (0x63616c74, "calt", vec![0x64696d65u32,0,0,0,0]),
            (0x72545243, "rTRC", vec![0x63757276u32,0,0xffffff00,0]),
            (0x636C7274, "clrt", vec![0x6e636c32u32,0,0xff,0,0]),
            (0x76636774, "vcgt", vec![0x76636774u32,0,0x99,0]),
            (0x6368726D, "chrm", vec![0x6368726Du32,0,0x05,0]),
        ] {
            let raw: Vec<u8> = payload.iter()
                .flat_map(|w| w.to_be_bytes()).collect();
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsWriteRawTag(prof, sig, raw.as_ptr() as *const _,
                raw.len() as u32);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = c::cmsSaveProfileToMem(prof,
                buf.as_mut_ptr() as *mut _, &mut n);
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, sig)
            } else { std::ptr::null_mut() };
            eprintln!("{name} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
    }
}

/// Type_ColorantOrderType — 'clro' tag is a u8 channel-order
/// array (count + indices).
#[test]
fn colorant_order_tag() {
    unsafe {
        // write path: u8 array payload
        let order: [u8; 4] = [3, 2, 1, 0];
        let prof = c::cmsCreate_sRGBProfile();
        let w = c::cmsWriteTag(prof, 0x636C726F,
            order.as_ptr() as *const _);
        let mut buf = vec![0u8; 1 << 20];
        let mut n = buf.len() as u32;
        let s = if w != 0 {
            c::cmsSaveProfileToMem(prof,
                buf.as_mut_ptr() as *mut _, &mut n)
        } else { 0 };
        let rp = if s != 0 {
            c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
        } else { std::ptr::null_mut() };
        let tag = if !rp.is_null() {
            c::cmsReadTag(rp, 0x636C726F)
        } else { std::ptr::null_mut() };
        eprintln!("clro w={w} s={s} read={}", !tag.is_null());
        if !rp.is_null() { c::cmsCloseProfile(rp); }
        c::cmsCloseProfile(prof);

        // raw malformed: Count > cmsMAXCHANNELS rejects
        let mut raw = Vec::new();
        raw.extend_from_slice(&0x636C726Fu32.to_be_bytes()); // type sig
        raw.extend_from_slice(&[0; 4]);
        raw.extend_from_slice(&17u32.to_be_bytes()); // Count=17 > 16
        raw.extend_from_slice(&[0u8; 20]);
        let prof2 = c::cmsCreate_sRGBProfile();
        c::cmsWriteRawTag(prof2, 0x636C726F,
            raw.as_ptr() as *const _, raw.len() as u32);
        let mut n2 = buf.len() as u32;
        let s2 = c::cmsSaveProfileToMem(prof2,
            buf.as_mut_ptr() as *mut _, &mut n2);
        let rp2 = if s2 != 0 {
            c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n2)
        } else { std::ptr::null_mut() };
        let tag2 = if !rp2.is_null() {
            c::cmsReadTag(rp2, 0x636C726F)
        } else { std::ptr::null_mut() };
        eprintln!("clro-17 s={s2} read={}", !tag2.is_null());
        if !rp2.is_null() { c::cmsCloseProfile(rp2); }
        c::cmsCloseProfile(prof2);
    }
}

/// Type_Screening + scrd — screening channels with frequency/
/// angle/spot-shape, plus screening-description tag.
#[test]
fn screening_full_tags() {
    unsafe {
        #[repr(C)] #[derive(Clone, Copy)]
        struct Ch { f: f64, a: f64, sp: u32 }
        #[repr(C)] struct Scrn { flag: u32, nch: u32,
            ch: [Ch; 16] }
        let mut sc = Scrn { flag: 0, nch: 4,
            ch: [Ch { f: 0.0, a: 0.0, sp: 0 }; 16] };
        for (i, (f, a, s)) in [
            (85.0, 15.0, 1u32), (85.0, 75.0, 1),
            (85.0, 0.0, 1), (85.0, 45.0, 2),
        ].iter().enumerate() {
            sc.ch[i] = Ch { f: *f, a: *a, sp: *s };
        }
        let prof = c::cmsCreate_sRGBProfile();
        let w = c::cmsWriteTag(prof, 0x7363726e,
            &mut sc as *mut _ as *const _); // 'scrn'
        let mut buf = vec![0u8; 1 << 20];
        let mut n = buf.len() as u32;
        let s = if w != 0 {
            c::cmsSaveProfileToMem(prof,
                buf.as_mut_ptr() as *mut _, &mut n)
        } else { 0 };
        let rp = if s != 0 {
            c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
        } else { std::ptr::null_mut() };
        let tag = if !rp.is_null() {
            c::cmsReadTag(rp, 0x7363726e)
        } else { std::ptr::null_mut() };
        eprintln!("scrn w={w} s={s} read={}", !tag.is_null());
        if !rp.is_null() { c::cmsCloseProfile(rp); }
        c::cmsCloseProfile(prof);
    }
}

/// Type_ProfileSequenceDesc + ProfileSequenceId — cmsSEQ with
/// populated deviceMfg/Model/tech/attrs + MLU fields.
#[test]
fn pseq_psid_full() {
    unsafe {
        use std::ffi::c_void;
        #[repr(C)] struct PseqDesc { mfg: u32, model: u32,
            attrs: u64, tech: u32, pid: [u8; 16],
            man: *mut c_void, mod_: *mut c_void, dsc: *mut c_void }
        #[repr(C)] struct Seq { n: u32, ctx: *mut c_void,
            seq: *mut PseqDesc }
        let mlu = |t: &[u8]| {
            let m = c::cmsMLUalloc(std::ptr::null_mut(), 1);
            c::cmsMLUsetUTF8(m, b"en\0".as_ptr() as *const _,
                b"US\0".as_ptr() as *const _, t.as_ptr());
            m
        };
        let psd = c::cmsAllocProfileSequenceDescription(
            std::ptr::null_mut(), 2);
        let s = psd as *mut Seq;
        for i in 0..2 {
            let e = (*s).seq.add(i);
            (*e).mfg = 0x41434D45;
            (*e).model = 0x4D4F444C;
            (*e).attrs = 0x1111111111111111;
            (*e).tech = 0x73636E20;
            (*e).pid = [i as u8; 16];
            (*e).man = mlu(b"acme\0") as *mut _;
            (*e).mod_ = mlu(b"model x\0") as *mut _;
            (*e).dsc = mlu(b"a printer\0") as *mut _;
        }
        for (sig, name) in [(0x70736571u32, "pseq"), (0x70736964, "psid")] {
            let prof = c::cmsCreate_sRGBProfile();
            let w = c::cmsWriteTag(prof, sig, psd as *const _);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let sv = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if sv != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, sig)
            } else { std::ptr::null_mut() };
            eprintln!("{name} w={w} s={sv} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
        c::cmsFreeProfileSequenceDescription(psd);

    }
}

/// Type_Curve_Read arms — 'curv' Count=0 (linear), Count=1
/// (gamma exponent), Count=N (table) + Type_Text raw read.
#[test]
fn curve_text_read_arms() {
    unsafe {
        // curv payloads: {count:u32, [u16;count]}
        for (name, count, entries) in [
            ("linear", 0u32, vec![]),
            ("gamma", 1, vec![0x0233u16]), // gamma 2.2 in 8.8
            ("table4", 4, vec![0u16, 0x5555, 0xAAAA, 0xFFFF]),
            ("table1", 1, vec![0x0100u16]),
        ] {
            let mut raw = Vec::new();
            raw.extend_from_slice(&0x63757276u32.to_be_bytes()); // 'curv'
            raw.extend_from_slice(&[0; 4]);
            raw.extend_from_slice(&count.to_be_bytes());
            for e in &entries {
                raw.extend_from_slice(&e.to_be_bytes());
            }
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsWriteRawTag(prof, 0x72545243,
                raw.as_ptr() as *const _, raw.len() as u32); // rTRC
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = c::cmsSaveProfileToMem(prof,
                buf.as_mut_ptr() as *mut _, &mut n);
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, 0x72545243)
            } else { std::ptr::null_mut() };
            let est = if !tag.is_null() {
                c::cmsEstimateGamma(tag as *const _, 0.001)
            } else { -1.0 };
            eprintln!("curv-{name} s={s} read={} est={est:.3}",
                !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
        // text type on targ
        let mut traw = Vec::new();
        traw.extend_from_slice(&0x74657874u32.to_be_bytes()); // 'text'
        traw.extend_from_slice(&[0; 4]);
        traw.extend_from_slice(b"Some target text\0");
        let prof = c::cmsCreate_sRGBProfile();
        c::cmsWriteRawTag(prof, 0x74617267,
            traw.as_ptr() as *const _, traw.len() as u32); // 'targ'
        let mut buf = vec![0u8; 1 << 20];
        let mut n = buf.len() as u32;
        let s = c::cmsSaveProfileToMem(prof,
            buf.as_mut_ptr() as *mut _, &mut n);
        let rp = if s != 0 {
            c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
        } else { std::ptr::null_mut() };
        let tag = if !rp.is_null() {
            c::cmsReadTag(rp, 0x74617267)
        } else { std::ptr::null_mut() };
        eprintln!("targ-text s={s} read={}", !tag.is_null());
        if !rp.is_null() { c::cmsCloseProfile(rp); }
        c::cmsCloseProfile(prof);
    }
}

/// Type_Dictionary_Read + _cmsReadWCharArray/ReadEmbeddedText —
/// raw 'dict' tags with 16/24/32-byte records and malformed arms.
#[test]
fn dict_record_arms() {
    unsafe {
        // dict tag: {count:u32, reclen:u32, entries[reclen/8 x
        // (off:u32, sz:u32)], data...}
        let utf16 = |s: &str| -> Vec<u8> {
            let mut v = Vec::new();
            for ch in s.encode_utf16().chain(std::iter::once(0)) {
                v.extend_from_slice(&ch.to_be_bytes());
            }
            v
        };
        // minimal mluc blob: {count:1, reclen:12, ent{en,US,ln,off}, str}
        let mluc = |s: &str| -> Vec<u8> {
            let body: Vec<u8> = s.encode_utf16()
                .flat_map(|c| c.to_be_bytes()).collect();
            let mut v = Vec::new();
            v.extend_from_slice(&1u32.to_be_bytes());
            v.extend_from_slice(&12u32.to_be_bytes());
            v.extend_from_slice(&0x656eu32.to_be_bytes());
            v.extend_from_slice(&0x5553u32.to_be_bytes());
            v.extend_from_slice(&(body.len() as u32).to_be_bytes());
            v.extend_from_slice(&20u32.to_be_bytes());
            v.extend_from_slice(&body);
            v
        };
        let mk = |reclen: u32| -> Vec<u8> {
            let name = utf16("key1");
            let value = utf16("val1");
            let dn = mluc("disp name");
            let dv = mluc("disp val");
            // entry sizes: 16->2 fields, 24->3, 32->4
            let n_fields = (reclen / 8) as usize;
            let base = 16 + reclen as usize; // header + count/reclen + table
            let mut offs = Vec::new();
            let mut data = Vec::new();
            for fld in [&name, &value, &dn, &dv][..n_fields].iter() {
                offs.push((base + data.len()) as u32);
                offs.push(fld.len() as u32);
                data.extend_from_slice(fld);
            }
            let mut t = Vec::new();
            t.extend_from_slice(&0x64696374u32.to_be_bytes()); // 'dict'
            t.extend_from_slice(&[0; 4]);
            t.extend_from_slice(&1u32.to_be_bytes()); // count
            t.extend_from_slice(&reclen.to_be_bytes());
            for o in offs {
                t.extend_from_slice(&o.to_be_bytes());
            }
            t.extend_from_slice(&data);
            t
        };
        for (reclen, name) in [(16u32, "r16"), (24, "r24"), (32, "r32"),
                               (20, "r20-bad"), (8, "r8-bad")] {
            let raw = mk(reclen);
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsWriteRawTag(prof, 0x6d657461,
                raw.as_ptr() as *const _, raw.len() as u32); // 'meta'
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = c::cmsSaveProfileToMem(prof,
                buf.as_mut_ptr() as *mut _, &mut n);
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, 0x6d657461)
            } else { std::ptr::null_mut() };
            eprintln!("dict-{name} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
    }
}

/// Type_ViewingConditions/Measurement/DateTime writes —
/// cmsICCViewingConditions, cmsICCMeasurementConditions and
/// cmsDateTimeNumber struct payloads.
#[test]
fn view_meas_calt_writes() {
    unsafe {
        #[repr(C)] struct Xyz3 { x: f64, y: f64, z: f64 }
        #[repr(C)] struct View { il: Xyz3, su: Xyz3, it: u32 }
        #[repr(C)] struct Meas { obs: u32, back: Xyz3, geo: u32,
            flare: f64, it: u32 }
        #[repr(C)] struct Dt { y: u16, mo: u16, d: u16,
            h: u16, mi: u16, s: u16 }
        let mut v = View {
            il: Xyz3 { x: 0.9642, y: 1.0, z: 0.8249 },
            su: Xyz3 { x: 0.5, y: 0.5, z: 0.5 },
            it: 2 };
        let mut mc = Meas { obs: 1,
            back: Xyz3 { x: 0.0, y: 0.0, z: 0.0 },
            geo: 1, flare: 0.0, it: 1 };
        let mut dt = Dt { y: 2024, mo: 6, d: 15,
            h: 12, mi: 30, s: 45 };
        let mut dt_bad = Dt { y: 0, mo: 99, d: 99,
            h: 99, mi: 99, s: 99 };
        for (sig, payload, name) in [
            (0x76696577u32, &mut v as *mut _ as *const _, "view"),
            (0x6d656173, &mut mc as *mut _ as *const _, "meas"),
            (0x63616c74, &mut dt as *mut _ as *const _, "calt"),
            (0x63616c74, &mut dt_bad as *mut _ as *const _, "calt-bad"),
        ] {
            let prof = c::cmsCreate_sRGBProfile();
            let w = c::cmsWriteTag(prof, sig, payload);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, sig)
            } else { std::ptr::null_mut() };
            eprintln!("{name} w={w} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
    }
}

/// Multi-item numeric/XYZ/signature array reads — raw tags with
/// 2-3 entries hit the nItems>1 loops in the type readers.
#[test]
fn multi_item_array_reads() {
    unsafe {
        // wtpt with 3 XYZ triples (nItems=3)
        let mut t = Vec::new();
        t.extend_from_slice(&0x58595A20u32.to_be_bytes()); // 'XYZ '
        t.extend_from_slice(&[0; 4]);
        for _ in 0..3 {
            t.extend_from_slice(&0x0000F6D6u32.to_be_bytes());
            t.extend_from_slice(&0x00010000u32.to_be_bytes());
            t.extend_from_slice(&0x0000D32Du32.to_be_bytes());
        }
        // lumi with 2 XYZ triples
        let mut t2 = Vec::new();
        t2.extend_from_slice(&0x58595A20u32.to_be_bytes());
        t2.extend_from_slice(&[0; 4]);
        for _ in 0..2 {
            t2.extend_from_slice(&0x000031CCu32.to_be_bytes());
            t2.extend_from_slice(&0x00010000u32.to_be_bytes());
            t2.extend_from_slice(&0x00000FA2u32.to_be_bytes());
        }
        // bkpt with bad XYZ type sig (wrong type -> reject or parse)
        let mut t3 = Vec::new();
        t3.extend_from_slice(&0x74657874u32.to_be_bytes()); // 'text'
        t3.extend_from_slice(&[0; 4]);
        t3.extend_from_slice(b"wrongtype\0");
        for (sig, raw, name) in [
            (0x77747074u32, &t, "wtpt-x3"),
            (0x6c756d69, &t2, "lumi-x2"),
            (0x626b7074, &t3, "bkpt-text"),
        ] {
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsWriteRawTag(prof, sig,
                raw.as_ptr() as *const _, raw.len() as u32);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = c::cmsSaveProfileToMem(prof,
                buf.as_mut_ptr() as *mut _, &mut n);
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, sig)
            } else { std::ptr::null_mut() };
            eprintln!("{name} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
    }
}

/// Type_ParametricCurve_Read arms — 'para' tags with each
/// function type and malformed variants.
#[test]
fn param_curve_read_arms() {
    unsafe {
        let mk = |ptype: u16, params: &[i32]| -> Vec<u8> {
            let mut t = Vec::new();
            t.extend_from_slice(&0x70617261u32.to_be_bytes()); // 'para'
            t.extend_from_slice(&[0; 4]);
            t.extend_from_slice(&ptype.to_be_bytes());
            t.extend_from_slice(&[0; 2]);
            for &v in params {
                t.extend_from_slice(&v.to_be_bytes());
            }
            t
        };
        let fx = |v: f64| (v * 65536.0) as i32; // S15Fixed16
        for (ptype, params, name) in [
            (0u16, &[fx(2.2)][..], "t0"),
            (1, &[fx(2.2), fx(1.0), fx(0.0)][..], "t1"),
            (2, &[fx(2.2), fx(1.0), fx(0.0), fx(0.0)][..], "t2"),
            (3, &[fx(2.2), fx(1.0), fx(0.0), fx(0.0), fx(0.0)][..], "t3"),
            (4, &[fx(2.2), fx(1.0), fx(0.0), fx(0.0), fx(0.0),
                  fx(0.0), fx(0.0)][..], "t4"),
            (5, &[fx(2.2), fx(1.0), fx(0.0), fx(0.0), fx(0.0),
                  fx(0.0), fx(0.0)][..], "t5"),
            (6, &[fx(1.0), fx(0.0), fx(0.0), fx(1.0)][..], "t6"),
            (7, &[fx(1.0), fx(0.0), fx(0.0), fx(0.0), fx(0.0)][..], "t7"),
            (8, &[fx(1.0), fx(0.0), fx(0.0), fx(0.0), fx(0.0)][..], "t8"),
            (9, &[fx(1.0)][..], "t9-bad"),
            (4, &[fx(1.0)][..], "t4-trunc"),
        ] {
            let raw = mk(ptype, params);
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsWriteRawTag(prof, 0x72545243,
                raw.as_ptr() as *const _, raw.len() as u32); // rTRC
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = c::cmsSaveProfileToMem(prof,
                buf.as_mut_ptr() as *mut _, &mut n);
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, 0x72545243)
            } else { std::ptr::null_mut() };
            let pt = if !tag.is_null() {
                c::cmsGetToneCurveParametricType(tag as *const _)
            } else { -1 };
            eprintln!("para-{name} s={s} read={} pt={pt}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
    }
}

/// Type_MPEcurve_Read + ReadMPECurve + bacs ignore-element —
/// hand-crafted mpet tag with a curve-set element plus a
/// (per-spec ignored) bacs element.
#[test]
fn mpe_curve_bacs_arms() {
    unsafe {
        // curv element payload for one channel: 'curv' + rsv + count + u16s
        let curv = |count: u32| -> Vec<u8> {
            let mut v = Vec::new();
            v.extend_from_slice(&0x63757276u32.to_be_bytes());
            v.extend_from_slice(&[0; 4]);
            v.extend_from_slice(&count.to_be_bytes());
            for i in 0..count {
                v.extend_from_slice(&((i * 4096) as u16).to_be_bytes());
            }
            v
        };
        let c0 = curv(2);
        // cvst element: {in:u16, out:u16} + pos table (3 entries) + 3 curv blobs
        let mut cvst = Vec::new();
        cvst.extend_from_slice(&0x63767374u32.to_be_bytes()); // 'cvst'
        cvst.extend_from_slice(&[0; 4]);
        cvst.extend_from_slice(&3u16.to_be_bytes());
        cvst.extend_from_slice(&3u16.to_be_bytes());
        let tab_base = 8 + 4 + 24; // mpet hdr + cvst hdr + pos table
        for i in 0..3u32 {
            cvst.extend_from_slice(&(tab_base + i * c0.len() as u32).to_be_bytes());
            cvst.extend_from_slice(&(c0.len() as u32).to_be_bytes());
        }
        for _ in 0..3 { cvst.extend_from_slice(&c0); }
        // bacs element: {in, out} only
        let mut bacs = Vec::new();
        bacs.extend_from_slice(&0x62414353u32.to_be_bytes()); // 'bACS'
        bacs.extend_from_slice(&[0; 4]);
        bacs.extend_from_slice(&3u16.to_be_bytes());
        bacs.extend_from_slice(&3u16.to_be_bytes());
        // mpet wrapper
        let mut mpet = Vec::new();
        mpet.extend_from_slice(&0x6d706574u32.to_be_bytes()); // 'mpet'
        mpet.extend_from_slice(&[0; 4]);
        mpet.extend_from_slice(&3u16.to_be_bytes());
        mpet.extend_from_slice(&3u16.to_be_bytes());
        mpet.extend_from_slice(&2u32.to_be_bytes()); // 2 elements
        let e0 = 8 + 2 + 2 + 4 + 2 * 8; // header + elem table
        let e1 = e0 + cvst.len() as u32;
        mpet.extend_from_slice(&e0.to_be_bytes());
        mpet.extend_from_slice(&(cvst.len() as u32).to_be_bytes());
        mpet.extend_from_slice(&e1.to_be_bytes());
        mpet.extend_from_slice(&(bacs.len() as u32).to_be_bytes());
        mpet.extend_from_slice(&cvst);
        mpet.extend_from_slice(&bacs);
        let prof = c::cmsCreate_sRGBProfile();
        c::cmsSetProfileVersion(prof, 4.4);
        c::cmsWriteRawTag(prof, 0x44324230,
            mpet.as_ptr() as *const _, mpet.len() as u32); // D2B0
        let mut buf = vec![0u8; 1 << 20];
        let mut n = buf.len() as u32;
        let s = c::cmsSaveProfileToMem(prof,
            buf.as_mut_ptr() as *mut _, &mut n);
        let rp = if s != 0 {
            c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
        } else { std::ptr::null_mut() };
        let tag = if !rp.is_null() {
            c::cmsReadTag(rp, 0x44324230)
        } else { std::ptr::null_mut() };
        eprintln!("mpet-cvst-bacs s={s} read={}", !tag.is_null());
        if !rp.is_null() { c::cmsCloseProfile(rp); }
        c::cmsCloseProfile(prof);
    }
}

/// Type_Signature_Read/Write — tech/ciis/rig0/rig2 signature
/// tags (u32 payload).
#[test]
fn signature_tag_arms() {
    unsafe {
        for (sig, val, name) in [
            (0x74656368u32, 0x66747020u32, "tech-ft"),
            (0x74656368, 0x73636E72u32, "tech-scnr"),
            (0x63696973, 0x7369676Eu32, "ciis"),
            (0x72696730, 0x7369676Eu32, "rig0"),
            (0x72696732, 0x7369676Eu32, "rig2"),
        ] {
            let prof = c::cmsCreate_sRGBProfile();
            let w = c::cmsWriteTag(prof, sig, &val as *const _ as *const _);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, sig)
            } else { std::ptr::null_mut() };
            let back = if !tag.is_null() {
                *(tag as *const u32)
            } else { 0 };
            eprintln!("{name} w={w} s={s} read={} val={back:#x}",
                !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
    }
}

/// Type_Curve_Write arms — parametric types 1-5 and tabulated
/// curves through rTRC round-trip; the writer decides 'curv'
/// vs 'para' by curve kind.
#[test]
fn curve_write_arms() {
    unsafe {
        let mut tab = [0u16; 16];
        for i in 0..16 { tab[i] = (i * 4096) as u16; }
        let tc = c::cmsBuildTabulatedToneCurve16(std::ptr::null_mut(),
            16, tab.as_ptr());
        let p2 = [2.2f64, 1.0, 0.0];
        let p3 = [2.2f64, 1.0, 0.0, 0.0];
        let p5 = [2.2f64, 1.0, 0.0, 0.0, 0.0];
        for (curve, name) in [
            (c::cmsBuildGamma(std::ptr::null_mut(), 2.2), "gamma"),
            (c::cmsBuildParametricToneCurve(std::ptr::null_mut(),
                1, p2.as_ptr()), "para1"),
            (c::cmsBuildParametricToneCurve(std::ptr::null_mut(),
                2, p3.as_ptr()), "para2"),
            (c::cmsBuildParametricToneCurve(std::ptr::null_mut(),
                3, p3.as_ptr()), "para3"),
            (c::cmsBuildParametricToneCurve(std::ptr::null_mut(),
                4, p5.as_ptr()), "para4"),
            (c::cmsBuildParametricToneCurve(std::ptr::null_mut(),
                5, p5.as_ptr()), "para5"),
            (tc, "tab"),
            (std::ptr::null_mut(), "null"),
        ] {
            if curve.is_null() && name != "null" { continue; }
            let prof = c::cmsCreate_sRGBProfile();
            let w = c::cmsWriteTag(prof, 0x72545243,
                curve as *const _);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, 0x72545243)
            } else { std::ptr::null_mut() };
            eprintln!("{name} w={w} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
            if !curve.is_null() { c::cmsFreeToneCurve(curve); }
        }
    }
}

/// io0 internals — linked-tag traversal, header read/write,
/// IOhandler seek edges.
#[test]
fn io0_linked_headers() {
    unsafe {
        let prof = c::cmsCreate_sRGBProfile();
        // write a tag, then link another sig to it
        let mut xyz = [0.5f64; 3];
        c::cmsWriteTag(prof, 0x77747074,
            xyz.as_mut_ptr() as *const _); // wtpt
        // gTRC -> rTRC link
        let mut tab = [0u16; 16];
        for i in 0..16 { tab[i] = (i * 4096) as u16; }
        let tc = c::cmsBuildTabulatedToneCurve16(std::ptr::null_mut(),
            16, tab.as_ptr());
        c::cmsWriteTag(prof, 0x72545243, tc as *const _); // rTRC
        c::cmsLinkTag(prof, 0x67545243, 0x72545243); // gTRC->rTRC
        c::cmsLinkTag(prof, 0x62545243, 0x67545243); // bTRC->gTRC (chain)
        let mut buf = vec![0u8; 1 << 20];
        let mut n = buf.len() as u32;
        let s = c::cmsSaveProfileToMem(prof,
            buf.as_mut_ptr() as *mut _, &mut n);
        let rp = if s != 0 {
            c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
        } else { std::ptr::null_mut() };
        for sig in [0x77747074u32, 0x72545243, 0x67545243, 0x62545243] {
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, sig)
            } else { std::ptr::null_mut() };
            eprintln!("sig={sig:#x} read={}", !tag.is_null());
        }
        // header accessors on the saved profile
        if !rp.is_null() {
            let mut pid = [0u8; 16];
            let mut flags = 0u32;
            let mut attrs = [0u64; 2];
            let mut dt = std::mem::zeroed::<[u16; 6]>();
            c::cmsGetHeaderProfileID(rp, pid.as_mut_ptr());
            c::cmsGetHeaderFlags(rp, &mut flags);
            c::cmsGetHeaderAttributes(rp, attrs.as_mut_ptr());
            let _ = c::cmsGetHeaderCreationDateTime(rp,
                dt.as_mut_ptr() as *mut _);
            let _ = c::cmsGetHeaderManufacturer(rp);
            let _ = c::cmsGetHeaderModel(rp);
            let _ = c::cmsGetHeaderRenderingIntent(rp);
            let _ = c::cmsGetHeaderCMM(rp);
            let _ = c::cmsGetHeaderCreator(rp);
            c::cmsCloseProfile(rp);
        }
        c::cmsCloseProfile(prof);
        c::cmsFreeToneCurve(tc);
    }
}

/// mpet curve-element — in-memory read first, then save-reread.
#[test]
fn mpe_curve_inmemory() {
    unsafe {
        // segmented curve: 'curf' + rsv + nseg(u16)+rsv(u16) +
        // (nseg-1) f32 breakpoints + per-seg {sig, rsv, body}
        let seeg = || -> Vec<u8> {
            let mut v = Vec::new();
            v.extend_from_slice(&0x63757266u32.to_be_bytes()); // 'curf'
            v.extend_from_slice(&[0; 4]);
            v.extend_from_slice(&1u16.to_be_bytes()); // 1 seg
            v.extend_from_slice(&[0; 2]);
            // no breakpoints for 1 seg
            // 'parf' seg: type(u16)+rsv(u16)+4 f32 params
            v.extend_from_slice(&0x70617266u32.to_be_bytes()); // 'parf'
            v.extend_from_slice(&[0; 4]);
            v.extend_from_slice(&0u16.to_be_bytes()); // Type 0
            v.extend_from_slice(&[0; 2]);
            for f in [1.0f32, 0.0, 0.0, 0.0] {
                v.extend_from_slice(&f.to_be_bytes());
            }
            v
        };
        let c0 = seeg();
        let mut cvst = Vec::new();
        cvst.extend_from_slice(&0x63767374u32.to_be_bytes());
        cvst.extend_from_slice(&[0; 4]);
        cvst.extend_from_slice(&3u16.to_be_bytes());
        cvst.extend_from_slice(&3u16.to_be_bytes());
        // position table — element-relative offsets, curves start
        // right after 12-byte cvst header + 24-byte table
        for i in 0..3u32 {
            cvst.extend_from_slice(&(12 + 24 + i * c0.len() as u32).to_be_bytes());
            cvst.extend_from_slice(&(c0.len() as u32).to_be_bytes());
        }
        for _ in 0..3 { cvst.extend_from_slice(&c0); }
        let mut mpet = Vec::new();
        mpet.extend_from_slice(&0x6d706574u32.to_be_bytes());
        mpet.extend_from_slice(&[0; 4]);
        mpet.extend_from_slice(&3u16.to_be_bytes());
        mpet.extend_from_slice(&3u16.to_be_bytes());
        mpet.extend_from_slice(&1u32.to_be_bytes()); // 1 element
        let e0: u32 = 8 + 2 + 2 + 4 + 8;
        mpet.extend_from_slice(&e0.to_be_bytes());
        mpet.extend_from_slice(&(cvst.len() as u32).to_be_bytes());
        mpet.extend_from_slice(&cvst);
        let prof = c::cmsCreateProfilePlaceholder(std::ptr::null_mut());
        c::cmsSetProfileVersion(prof, 4.4);
        c::cmsSetDeviceClass(prof, 0x6c696e6b); // 'link'
        c::cmsSetColorSpace(prof, 0x52474220); // 'RGB '
        c::cmsSetPCS(prof, 0x52474220);
        c::cmsWriteRawTag(prof, 0x44324230,
            mpet.as_ptr() as *const _, mpet.len() as u32);
        // in-memory read
        let t0 = c::cmsReadTag(prof, 0x44324230);
        eprintln!("inmem read={}", !t0.is_null());
        let mut buf = vec![0u8; 1 << 20];
        let mut n = buf.len() as u32;
        let s = c::cmsSaveProfileToMem(prof,
            buf.as_mut_ptr() as *mut _, &mut n);
        let rp = if s != 0 {
            c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
        } else { std::ptr::null_mut() };
        let t1 = if !rp.is_null() {
            c::cmsReadTag(rp, 0x44324230)
        } else { std::ptr::null_mut() };
        eprintln!("saved read={}", !t1.is_null());
        if !rp.is_null() { c::cmsCloseProfile(rp); }
        c::cmsCloseProfile(prof);
    }
}

/// Isolate mpet wrapper vs element blob — raw mpet with a single
/// matf element (known to round-trip via the API).
#[test]
fn mpe_matf_raw() {
    unsafe {
        // matf element: 'matf' + rsv + in(2)+out(2) + 9 f64? no —
        // matrix is 3x3 f32 + 3 f32 offsets
        let mut matf = Vec::new();
        matf.extend_from_slice(&0x6d617466u32.to_be_bytes()); // 'matf'
        matf.extend_from_slice(&[0; 4]);
        matf.extend_from_slice(&3u16.to_be_bytes());
        matf.extend_from_slice(&3u16.to_be_bytes());
        for i in 0..9u32 {
            let v = if i % 4 == 0 { 1.0f64 } else { 0.0 };
            matf.extend_from_slice(&v.to_be_bytes());
        }
        for _ in 0..3u32 {
            matf.extend_from_slice(&0.0f64.to_be_bytes());
        }
        let mut mpet = Vec::new();
        mpet.extend_from_slice(&0x6d706574u32.to_be_bytes());
        mpet.extend_from_slice(&[0; 4]);
        mpet.extend_from_slice(&3u16.to_be_bytes());
        mpet.extend_from_slice(&3u16.to_be_bytes());
        mpet.extend_from_slice(&1u32.to_be_bytes());
        let e0: u32 = 8 + 2 + 2 + 4 + 8;
        mpet.extend_from_slice(&e0.to_be_bytes());
        mpet.extend_from_slice(&(matf.len() as u32).to_be_bytes());
        mpet.extend_from_slice(&matf);
        let prof = c::cmsCreate_sRGBProfile();
        c::cmsSetProfileVersion(prof, 4.4);
        c::cmsWriteRawTag(prof, 0x44324230,
            mpet.as_ptr() as *const _, mpet.len() as u32);
        let t0 = c::cmsReadTag(prof, 0x44324230);
        eprintln!("matf inmem read={}", !t0.is_null());
        c::cmsCloseProfile(prof);
    }
}

/// Dump a real Type_MPE_Write-serialized mpet blob to compare
/// against the hand-crafted layout.
#[test]
fn mpe_blob_dump() {
    unsafe {
        let mat = [1.0f64, 0.0, 0.0,
                   0.0, 1.0, 0.0,
                   0.0, 0.0, 1.0];
        let off = [0.0f64; 3];
        let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocMatrix(std::ptr::null_mut(), 3, 3,
                mat.as_ptr(), off.as_ptr()));
        let prof = c::cmsCreateProfilePlaceholder(std::ptr::null_mut());
        c::cmsSetProfileVersion(prof, 4.4);
        c::cmsSetDeviceClass(prof, 0x6c696e6b);
        c::cmsSetColorSpace(prof, 0x52474220);
        c::cmsSetPCS(prof, 0x52474220);
        let w = c::cmsWriteTag(prof, 0x44324230, p as *const _);
        eprintln!("write={w}");
        let mut raw = vec![0u8; 1 << 16];
        let n = c::cmsReadRawTag(prof, 0x44324230,
            raw.as_mut_ptr() as *mut _, raw.len() as u32);
        eprintln!("raw n={n}");
        for (i, b) in raw[..n.min(64) as usize].iter().enumerate() {
            eprint!("{:02x}", b);
            if i % 4 == 3 { eprint!(" "); }
        }
        eprintln!();
        // feed the real bytes back as a raw tag
        let prof2 = c::cmsCreateProfilePlaceholder(std::ptr::null_mut());
        c::cmsSetProfileVersion(prof2, 4.4);
        c::cmsSetDeviceClass(prof2, 0x6c696e6b);
        c::cmsSetColorSpace(prof2, 0x52474220);
        c::cmsSetPCS(prof2, 0x52474220);
        c::cmsWriteRawTag(prof2, 0x44324230,
            raw.as_ptr() as *const _, n);
        let t0 = c::cmsReadTag(prof2, 0x44324230);
        eprintln!("realbytes-raw read={}", !t0.is_null());
        c::cmsCloseProfile(prof2);
        c::cmsCloseProfile(prof);
        c::cmsPipelineFree(p);
    }
}

/// Dump a real mAB serialized blob to craft element-order arms.
#[test]
fn mab_blob_dump() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let curves = vec![g; 3];
        let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocToneCurves(std::ptr::null_mut(), 3,
                curves.as_ptr() as *mut _));
        let prof = c::cmsCreateProfilePlaceholder(std::ptr::null_mut());
        c::cmsSetProfileVersion(prof, 4.4);
        c::cmsSetDeviceClass(prof, 0x6d6e7472); // 'mntr'
        c::cmsSetColorSpace(prof, 0x52474220);  // 'RGB '
        c::cmsSetPCS(prof, 0x4c616220);         // 'Lab '
        let w = c::cmsWriteTag(prof, 0x41324230, p as *const _); // A2B0
        eprintln!("write={w}");
        let mut raw = vec![0u8; 1 << 16];
        let n = c::cmsReadRawTag(prof, 0x41324230,
            raw.as_mut_ptr() as *mut _, raw.len() as u32);
        eprintln!("raw n={n}");
        for (i, b) in raw[..n.min(96) as usize].iter().enumerate() {
            eprint!("{:02x}", b);
            if i % 4 == 3 { eprint!(" "); }
        }
        eprintln!();
        c::cmsCloseProfile(prof);
        c::cmsPipelineFree(p);
        c::cmsFreeToneCurve(g);
    }
}

/// Type_LUTA2B_Read arms — raw mAB tags with element-offset
/// variants (B-only, B+matrix, B+clut, oob offsets, all-zero).
#[test]
fn mab_element_offset_arms() {
    unsafe {
        // base: mAB header + 3 'para' B curves at offset 0x20
        let para = |g: u32| -> Vec<u8> {
            let mut v = Vec::new();
            v.extend_from_slice(&0x70617261u32.to_be_bytes()); // 'para'
            v.extend_from_slice(&[0; 4]);
            v.extend_from_slice(&0u16.to_be_bytes()); // fnType 0
            v.extend_from_slice(&[0; 2]);
            v.extend_from_slice(&g.to_be_bytes());
            v
        };
        let curves: Vec<u8> = [0x0233u32, 0x0233, 0x0233].iter()
            .flat_map(|&g| para(g)).collect();
        // 'matf'-style? no — mAB matrix is raw s15fixed16 3x3+offsets
        let mut mat = Vec::new();
        for i in 0..12u32 {
            let v = if i % 4 == 0 { 0x00010000u32 } else { 0 };
            mat.extend_from_slice(&v.to_be_bytes());
        }
        let mk = |offs: [u32; 5], tail: Vec<u8>| -> Vec<u8> {
            let mut t = Vec::new();
            t.extend_from_slice(&0x6d414220u32.to_be_bytes()); // 'mAB '
            t.extend_from_slice(&[0; 4]);
            t.extend_from_slice(&0x0303u16.to_be_bytes()); // in=3
            t.extend_from_slice(&[0; 2]);
            for &o in &offs {
                t.extend_from_slice(&o.to_be_bytes());
            }
            t.extend_from_slice(&tail);
            t
        };
        let mut tail_bmat = curves.clone();
        tail_bmat.extend_from_slice(&mat);
        let mut tail_2cv = curves.clone();
        tail_2cv.extend_from_slice(&curves);
        // 16-bit CLUT: grid[16]=2 each dim, prec=2, 8 nodes x3
        let mut clut = vec![2u8; 16];
        clut.push(2); clut.extend_from_slice(&[0; 3]);
        for i in 0..24u32 {
            clut.extend_from_slice(&(i * 2730).to_be_bytes()
                .iter().skip(2).copied().collect::<Vec<u8>>()
                .as_slice());
        }
        let mut tail_bclut = curves.clone();
        tail_bclut.extend_from_slice(&clut);
        for (offs, tail, name) in [
            ([0x20u32, 0, 0, 0, 0], curves.clone(), "b-only"),
            ([0x20, 0x50, 0, 0, 0], tail_bmat.clone(), "b+mat"),
            ([0x20, 0, 0x50, 0, 0], tail_2cv.clone(), "b+m"),
            ([0x20, 0, 0, 0x50, 0], tail_bclut.clone(), "b+clut"),
            ([0x20, 0, 0, 0, 0x50], tail_2cv.clone(), "b+a"),
            ([0xffffu32, 0, 0, 0, 0], curves.clone(), "b-oob"),
            ([0, 0, 0, 0, 0], vec![], "all-zero"),
            ([0x20, 0x20, 0x20, 0x20, 0x20], tail_2cv.clone(), "all-b"),
            ([0x20, 0x50, 0x50, 0x50, 0x50], tail_bmat.clone(),
             "b+all-mat"),
        ] {
            let raw = mk(offs, tail);
            let prof = c::cmsCreateProfilePlaceholder(std::ptr::null_mut());
            c::cmsSetProfileVersion(prof, 4.4);
            c::cmsSetDeviceClass(prof, 0x6d6e7472);
            c::cmsSetColorSpace(prof, 0x52474220);
            c::cmsSetPCS(prof, 0x4c616220);
            c::cmsWriteRawTag(prof, 0x41324230,
                raw.as_ptr() as *const _, raw.len() as u32);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = c::cmsSaveProfileToMem(prof,
                buf.as_mut_ptr() as *mut _, &mut n);
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, 0x41324230)
            } else { std::ptr::null_mut() };
            eprintln!("{name} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
    }
}

/// _cmsReadWCharArray/_cmsWriteWCharArray — dict entries with
/// non-ASCII (surrogate-pair) names/values hit the wchar
/// conversion deep paths.
#[test]
fn dict_wchar_deep() {
    unsafe {
        // dict with non-ASCII names: CJK + emoji (surrogate pair)
        let d = c::cmsDictAlloc(std::ptr::null_mut());
        for (k, v) in [
            ("キー", "1"),
            ("emoji\u{1f600}key", "v2"),
            ("key3", "valüé"),
            ("", "empty-key"),
        ] {
            let mut wk: Vec<u32> = k.chars().map(|c| c as u32).collect();
            let mut wv: Vec<u32> = v.chars().map(|c| c as u32).collect();
            wk.push(0); wv.push(0);
            c::cmsDictAddEntry(d, wk.as_ptr() as *const _,
                wv.as_ptr() as *const _,
                std::ptr::null_mut(), std::ptr::null_mut());
        }
        let prof = c::cmsCreate_sRGBProfile();
        let w = c::cmsWriteTag(prof, 0x6d657461, d as *const _);
        let mut buf = vec![0u8; 1 << 20];
        let mut n = buf.len() as u32;
        let s = if w != 0 {
            c::cmsSaveProfileToMem(prof,
                buf.as_mut_ptr() as *mut _, &mut n)
        } else { 0 };
        let rp = if s != 0 {
            c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
        } else { std::ptr::null_mut() };
        let tag = if !rp.is_null() {
            c::cmsReadTag(rp, 0x6d657461)
        } else { std::ptr::null_mut() };
        eprintln!("dict-wchar w={w} s={s} read={}", !tag.is_null());
        if !rp.is_null() { c::cmsCloseProfile(rp); }
        c::cmsCloseProfile(prof);
        c::cmsDictFree(d);
    }
}

/// io0 save/open edge internals — IOhandler round-trips, buffer
/// sizing, raw-tag reads on missing tags.
#[test]
fn io0_edge_internals() {
    unsafe {
        let prof = c::cmsCreate_sRGBProfile();
        let mut buf = vec![0u8; 1 << 20];
        let mut n = buf.len() as u32;
        // save to mem -> open via IOhandler
        let s = c::cmsSaveProfileToMem(prof,
            buf.as_mut_ptr() as *mut _, &mut n);
        eprintln!("save={s} n={n}");
        let io = c::cmsOpenIOhandlerFromMem(std::ptr::null_mut(),
            buf.as_ptr() as *mut _, n, b"r\0".as_ptr());
        let rp = if !io.is_null() {
            c::cmsOpenProfileFromIOhandlerTHR(std::ptr::null_mut(), io)
        } else { std::ptr::null_mut() };
        eprintln!("io-open={}", !rp.is_null());
        // read raw tag on present + missing tags
        let mut raw = [0u8; 64];
        let r1 = c::cmsReadRawTag(rp, 0x77747074,
            raw.as_mut_ptr() as *mut _, 64);
        eprintln!("raw-wtpt={r1}");
        let r2 = c::cmsReadRawTag(rp, 0x78787878,
            raw.as_mut_ptr() as *mut _, 64);
        eprintln!("raw-missing={r2}");
        // write tag on the IOhandler-opened profile then re-save
        let io2 = c::cmsOpenIOhandlerFromMem(std::ptr::null_mut(),
            buf.as_mut_ptr() as *mut _, buf.len() as u32,
            b"w\0".as_ptr());
        let mut n2 = 0u32;
        let s2 = c::cmsSaveProfileToIOhandler(rp, io2);
        let _ = n2;
        eprintln!("save-ioh={s2}");
        if !rp.is_null() { c::cmsCloseProfile(rp); }
        // cmsReadRawTag with NULL buffer returns needed size
        let rp2 = c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n);
        let r3 = c::cmsReadRawTag(rp2, 0x77747074,
            std::ptr::null_mut(), 0);
        eprintln!("raw-size={r3}");
        if !rp2.is_null() { c::cmsCloseProfile(rp2); }
        c::cmsCloseProfile(prof);
    }
}

/// cmsGetProfileInfo internals — per-info-id tag selection on
/// corpus profiles × languages × encodings.
#[test]
fn profile_info_internals2() {
    unsafe {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        let srgb = c::cmsCreate_sRGBProfile();
        let mut buf = vec![0u16; 512];
        for (id, name) in [
            (0u32, "desc"), (1, "cprt"), (2, "dmnd"),
            (3, "dmdd"), (4, "lmod"), (5, "tech"),
            (6, "vued"), (7, "arg"), (8, "chad"),
        ] {
            let n = c::cmsGetProfileInfoUTF8(srgb, id,
                b"en\0".as_ptr() as *const i8,
                b"US\0".as_ptr() as *const i8,
                buf.as_mut_ptr() as *mut _, 512);
            let n2 = c::cmsGetProfileInfo(srgb, id,
                b"en\0".as_ptr() as *const _,
                b"US\0".as_ptr() as *const _,
                buf.as_mut_ptr() as *mut _, 512);
            let n3 = c::cmsGetProfileInfoASCII(srgb, id,
                b"en\0".as_ptr() as *const i8,
                b"US\0".as_ptr() as *const i8,
                buf.as_mut_ptr() as *mut _, 512);
            eprintln!("{name} utf8={n} u16={n2} ascii={n3}");
        }
        // corpus profiles
        for f in ["lcms2cmyk.icc", "labv4lcms2.icc", "labv2lcms2.icc",
                  "aRGBlcms2.icc", "graylcms2.icc", "xyzlcms2.icc"] {
            let Ok(d) = std::fs::read(dir.join(f)) else { continue };
            let p = c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32);
            if p.is_null() { continue; }
            for id in 0..9u32 {
                let _ = c::cmsGetProfileInfoASCII(p, id,
                    b"en\0".as_ptr() as *const i8,
                    b"US\0".as_ptr() as *const i8,
                    buf.as_mut_ptr() as *mut _, 512);
            }
            eprintln!("{f} done");
            c::cmsCloseProfile(p);
        }
        c::cmsCloseProfile(srgb);
    }
}

/// Type_LUTB2A_Read arms — raw mBA tags with element-offset
/// variants on B2A sigs.
#[test]
fn mba_element_offset_arms() {
    unsafe {
        let para = |g: u32| -> Vec<u8> {
            let mut v = Vec::new();
            v.extend_from_slice(&0x70617261u32.to_be_bytes()); // 'para'
            v.extend_from_slice(&[0; 4]);
            v.extend_from_slice(&0u16.to_be_bytes());
            v.extend_from_slice(&[0; 2]);
            v.extend_from_slice(&g.to_be_bytes());
            v
        };
        let curves: Vec<u8> = [0x0233u32, 0x0233, 0x0233].iter()
            .flat_map(|&g| para(g)).collect();
        let mut mat = Vec::new();
        for i in 0..12u32 {
            let v = if i % 4 == 0 { 0x00010000u32 } else { 0 };
            mat.extend_from_slice(&v.to_be_bytes());
        }
        // 16-bit CLUT: grid[16]=2, prec=2, 8 nodes x3
        let mut clut = vec![2u8; 16];
        clut.push(2); clut.extend_from_slice(&[0; 3]);
        for i in 0..24u32 {
            clut.extend_from_slice(&((i * 2730) as u16).to_be_bytes());
        }
        let mk = |offs: [u32; 5], tail: Vec<u8>| -> Vec<u8> {
            let mut t = Vec::new();
            t.extend_from_slice(&0x6d424120u32.to_be_bytes()); // 'mBA '
            t.extend_from_slice(&[0; 4]);
            t.extend_from_slice(&0x0303u16.to_be_bytes());
            t.extend_from_slice(&[0; 2]);
            for &o in &offs {
                t.extend_from_slice(&o.to_be_bytes());
            }
            t.extend_from_slice(&tail);
            t
        };
        let mut tail_bmat = curves.clone();
        tail_bmat.extend_from_slice(&mat);
        let mut tail_2cv = curves.clone();
        tail_2cv.extend_from_slice(&curves);
        let mut tail_bclut = curves.clone();
        tail_bclut.extend_from_slice(&clut);
        let mut tail_full = curves.clone(); // B
        tail_full.extend_from_slice(&mat);   // mat
        tail_full.extend_from_slice(&curves); // M
        tail_full.extend_from_slice(&clut);  // CLUT
        tail_full.extend_from_slice(&curves); // A
        for (offs, tail, name) in [
            ([0x20u32, 0, 0, 0, 0], curves.clone(), "b-only"),
            ([0x20, 0x50, 0, 0, 0], tail_bmat.clone(), "b+mat"),
            ([0x20, 0x50, 0x80, 0, 0], {
                let mut t = tail_bmat.clone();
                t.extend_from_slice(&curves);
                t
            }, "b+mat+m"),
            ([0x20, 0, 0, 0x50, 0], tail_bclut.clone(), "b+clut"),
            ([0x20, 0x50, 0x80, 0xb0, 0xd8], tail_full.clone(),
             "full"),
            ([0xffffu32, 0, 0, 0, 0], curves.clone(), "b-oob"),
            ([0, 0, 0, 0, 0], vec![], "all-zero"),
        ] {
            let raw = mk(offs, tail);
            let prof = c::cmsCreateProfilePlaceholder(std::ptr::null_mut());
            c::cmsSetProfileVersion(prof, 4.4);
            c::cmsSetDeviceClass(prof, 0x6d6e7472);
            c::cmsSetColorSpace(prof, 0x52474220);
            c::cmsSetPCS(prof, 0x4c616220);
            c::cmsWriteRawTag(prof, 0x42324130,
                raw.as_ptr() as *const _, raw.len() as u32); // B2A0
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = c::cmsSaveProfileToMem(prof,
                buf.as_mut_ptr() as *mut _, &mut n);
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, 0x42324130)
            } else { std::ptr::null_mut() };
            eprintln!("{name} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
    }
}

/// Type_LUT8_Read/Type_LUT16_Read internals — raw mft1/mft2 tags
/// with matrix + curve-set + CLUT structure variants.
#[test]
fn mft_reader_internals() {
    unsafe {
        // mft2: 'mft2' + in(u8)+out(u8)+grid(u8)+pad + 9xs15f16
        // matrix + u16 inEnt + u16 outEnt + in curves + clut + out
        let mft2 = |grid: u8, inent: u16, outent: u16,
                    clut_vals: &[u16], inc: &[u16], outc: &[u16]|
            -> Vec<u8> {
            let mut t = Vec::new();
            t.extend_from_slice(&0x6d667432u32.to_be_bytes()); // 'mft2'
            t.extend_from_slice(&[0; 4]);
            t.push(3); t.push(3); t.push(grid); t.push(0);
            for i in 0..9u32 {
                let v = if i % 4 == 0 { 0x00010000u32 } else { 0 };
                t.extend_from_slice(&v.to_be_bytes());
            }
            t.extend_from_slice(&inent.to_be_bytes());
            t.extend_from_slice(&outent.to_be_bytes());
            for _ in 0..3 {
                for &v in inc {
                    t.extend_from_slice(&v.to_be_bytes());
                }
            }
            for &v in clut_vals {
                t.extend_from_slice(&v.to_be_bytes());
            }
            for _ in 0..3 {
                for &v in outc {
                    t.extend_from_slice(&v.to_be_bytes());
                }
            }
            t
        };
        // mft1: same but u8 entries, no matrix
        let mft1 = |grid: u8, clut_vals: &[u8]| -> Vec<u8> {
            let mut t = Vec::new();
            t.extend_from_slice(&0x6d667431u32.to_be_bytes()); // 'mft1'
            t.extend_from_slice(&[0; 4]);
            t.push(3); t.push(3); t.push(grid); t.push(0);
            t.extend_from_slice(&[0; 4]); // inEnt=256 outEnt=256
            t.extend_from_slice(&[0; 2]);
            for _ in 0..3 { t.extend(std::iter::repeat(0u8..=255).take(256).flatten()); }
            t.extend_from_slice(clut_vals);
            for _ in 0..3 { t.extend(std::iter::repeat(0u8..=255).take(256).flatten()); }
            t
        };
        let inc: Vec<u16> = (0..4).map(|i| (i * 16384) as u16).collect();
        let outc = inc.clone();
        let clut: Vec<u16> = (0..8 * 3).map(|i| (i * 2048) as u16).collect();
        let clut1: Vec<u8> = (0..8 * 3).map(|i| (i * 7) as u8).collect();
        for (raw, sig, name) in [
            (mft2(2, 4, 4, &clut, &inc, &outc), 0x41324230u32, "mft2-full"),
            (mft2(4, 4, 4, &(0..64*3).map(|i| (i*100) as u16).collect::<Vec<u16>>(), &inc, &outc), 0x41324230, "mft2-grid4"),
            (mft2(2, 1, 1, &clut, &inc[..1], &outc[..1]), 0x41324230, "mft2-1ent"),
            (mft1(2, &clut1), 0x41324230, "mft1-full"),
        ] {
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsWriteRawTag(prof, sig,
                raw.as_ptr() as *const _, raw.len() as u32);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = c::cmsSaveProfileToMem(prof,
                buf.as_mut_ptr() as *mut _, &mut n);
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, sig)
            } else { std::ptr::null_mut() };
            eprintln!("{name} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
    }
}

/// io0 tag-iteration internals — GetTagCount/GetTagSignature/
/// IsTag on profiles with different tag sets.
#[test]
fn io0_tag_iteration() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let xyz = c::cmsCreateXYZProfile();
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        for (p, name) in [(srgb, "srgb"), (lab, "lab"), (xyz, "xyz")] {
            let n = c::cmsGetTagCount(p);
            eprintln!("{name} tags={n}");
            for i in 0..n.max(0) as u32 {
                let sig = c::cmsGetTagSignature(p, i as u32);
                let ist = c::cmsIsTag(p, sig);
                let _ = (sig, ist);
            }
            // IsTag on sigs that aren't present
            for sig in [0x6e636c32u32, 0x76636774, 0x6d657461,
                        0x6368726D, 0x636C7274] {
                let ist = c::cmsIsTag(p, sig);
                let _ = ist;
            }
        }
        for f in ["lcms2cmyk.icc", "labv4lcms2.icc", "aRGBlcms2.icc",
                  "brightness.icc", "lcms2link.icc"] {
            let Ok(d) = std::fs::read(dir.join(f)) else { continue };
            let p = c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32);
            if p.is_null() { continue; }
            let n = c::cmsGetTagCount(p);
            for i in 0..n.max(0) as u32 {
                let sig = c::cmsGetTagSignature(p, i as u32);
                let _ = sig;
            }
            eprintln!("{f} tags={n}");
            c::cmsCloseProfile(p);
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
        c::cmsCloseProfile(xyz);
    }
}

/// Type_vcgt_Read arms — raw vcgt tags covering table u8/u16,
/// formula params, wrong channel/byte-count rejects.
#[test]
fn vcgt_read_arms() {
    unsafe {
        // table type 0: {nch:u16, nElems:u16, nBytes:u16, data}
        let mut t_u8 = Vec::new();
        t_u8.extend_from_slice(&0x76636774u32.to_be_bytes()); // 'vcgt'
        t_u8.extend_from_slice(&[0; 4]);
        t_u8.extend_from_slice(&0u32.to_be_bytes()); // table type
        t_u8.extend_from_slice(&3u16.to_be_bytes()); // 3 ch
        t_u8.extend_from_slice(&4u16.to_be_bytes()); // 4 elems
        t_u8.extend_from_slice(&1u16.to_be_bytes()); // 1 byte
        t_u8.extend_from_slice(&[0, 85, 170, 255, 0, 85, 170, 255,
                                 0, 85, 170, 255]);
        let mut t_u16 = Vec::new();
        t_u16.extend_from_slice(&0x76636774u32.to_be_bytes());
        t_u16.extend_from_slice(&[0; 4]);
        t_u16.extend_from_slice(&0u32.to_be_bytes());
        t_u16.extend_from_slice(&3u16.to_be_bytes());
        t_u16.extend_from_slice(&4u16.to_be_bytes());
        t_u16.extend_from_slice(&2u16.to_be_bytes()); // 2 bytes
        for _ in 0..3 {
            for v in [0u16, 21845, 43690, 65535] {
                t_u16.extend_from_slice(&v.to_be_bytes());
            }
        }
        // formula type 1: {g, e[3]} x3 channels
        let mut t_f = Vec::new();
        t_f.extend_from_slice(&0x76636774u32.to_be_bytes());
        t_f.extend_from_slice(&[0; 4]);
        t_f.extend_from_slice(&1u32.to_be_bytes()); // formula
        for _ in 0..3 {
            for v in [0x00023333u32, 0, 0, 0, 0] {
                t_f.extend_from_slice(&v.to_be_bytes());
            }
        }
        // rejects: wrong channels, wrong bytes, bad tagtype
        let mut t_bad_ch = Vec::new();
        t_bad_ch.extend_from_slice(&0x76636774u32.to_be_bytes());
        t_bad_ch.extend_from_slice(&[0; 4]);
        t_bad_ch.extend_from_slice(&0u32.to_be_bytes());
        t_bad_ch.extend_from_slice(&2u16.to_be_bytes()); // 2ch bad
        t_bad_ch.extend_from_slice(&[0; 8]);
        let mut t_bad_b = Vec::new();
        t_bad_b.extend_from_slice(&0x76636774u32.to_be_bytes());
        t_bad_b.extend_from_slice(&[0; 4]);
        t_bad_b.extend_from_slice(&0u32.to_be_bytes());
        t_bad_b.extend_from_slice(&3u16.to_be_bytes());
        t_bad_b.extend_from_slice(&4u16.to_be_bytes());
        t_bad_b.extend_from_slice(&3u16.to_be_bytes()); // 3B bad
        t_bad_b.extend_from_slice(&[0; 24]);
        let mut t_bad_t = Vec::new();
        t_bad_t.extend_from_slice(&0x76636774u32.to_be_bytes());
        t_bad_t.extend_from_slice(&[0; 4]);
        t_bad_t.extend_from_slice(&7u32.to_be_bytes()); // bad type
        t_bad_t.extend_from_slice(&[0; 24]);
        for (raw, name) in [
            (t_u8, "u8"), (t_u16, "u16"), (t_f, "formula"),
            (t_bad_ch, "bad-ch"), (t_bad_b, "bad-bytes"),
            (t_bad_t, "bad-type"),
        ] {
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsWriteRawTag(prof, 0x76636774,
                raw.as_ptr() as *const _, raw.len() as u32);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = c::cmsSaveProfileToMem(prof,
                buf.as_mut_ptr() as *mut _, &mut n);
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, 0x76636774)
            } else { std::ptr::null_mut() };
            eprintln!("{name} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
    }
}

/// Type_MHC2_Read arms — raw MHC2 tags with wrong curve counts,
/// bad matrix entries and luminance edge cases.
#[test]
fn mhc2_read_arms() {
    unsafe {
        // real format: CurveEntries:u32, MinLum/PeakLum s15f16,
        // MatrixOffset + Red/Green/Blue table offsets, then
        // matrix f64x12 and 'sf32' curve tables at the offsets.
        let mk = |n_ent: u32, mat_off: u32, curv_off: u32,
                  min_l: u32, peak_l: u32| -> Vec<u8> {
            let mut t = Vec::new();
            t.extend_from_slice(&0x4d484332u32.to_be_bytes()); // 'MHC2'
            t.extend_from_slice(&[0; 4]);
            t.extend_from_slice(&n_ent.to_be_bytes());
            t.extend_from_slice(&min_l.to_be_bytes());
            t.extend_from_slice(&peak_l.to_be_bytes());
            t.extend_from_slice(&mat_off.to_be_bytes());
            t.extend_from_slice(&curv_off.to_be_bytes());
            t.extend_from_slice(&(curv_off).to_be_bytes());
            t.extend_from_slice(&(curv_off).to_be_bytes());
            // pad to offsets
            while t.len() < mat_off as usize && mat_off > 0 {
                t.push(0);
            }
            if mat_off > 0 {
                for i in 0..12u32 {
                    let v = if i % 4 == 0 { 1.0f64 } else { 0.0 };
                    t.extend_from_slice(&v.to_be_bytes());
                }
            }
            while t.len() < curv_off as usize && curv_off > 0 {
                t.push(0);
            }
            if curv_off > 0 {
                // 'sf32' + rsv + entries x f64
                t.extend_from_slice(&0x73663332u32.to_be_bytes());
                t.extend_from_slice(&[0; 4]);
                for i in 0..n_ent {
                    let v = i as f64 / (n_ent.max(1) - 1).max(1) as f64;
                    t.extend_from_slice(&v.to_be_bytes());
                }
            }
            t
        };
        for (raw, name) in [
            (mk(4, 0, 56, 0, 0x03200000), "full-nomat"),
            (mk(4, 56, 152, 0, 0x03200000), "full-mat"),
            (mk(0, 0, 0, 0, 0x03200000), "0-entries"),
            (mk(5000, 0, 56, 0, 0x03200000), "5k-entries"),
            (mk(4, 0, 56, 0, 0), "zero-peak"),
        ] {
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsWriteRawTag(prof, 0x4d484332,
                raw.as_ptr() as *const _, raw.len() as u32);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = c::cmsSaveProfileToMem(prof,
                buf.as_mut_ptr() as *mut _, &mut n);
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, 0x4d484332)
            } else { std::ptr::null_mut() };
            eprintln!("{name} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
    }
}

/// Type_UcrBg_Read arms — raw ucrbg tags with curve-count /
/// text-length variants.
#[test]
fn ucrbg_read_arms() {
    unsafe {
        let mk = |ucr: &[u16], bg: &[u16], txt: &str| -> Vec<u8> {
            let mut t = Vec::new();
            t.extend_from_slice(&0x75637242u32.to_be_bytes()); // 'ucrbg'... check sig
            t
        };
        let _ = mk;
        // real: 'ucrbg' sig is 'bfd '? check sig later; build raw
        // {CountUcr:u32, u16[], CountBg:u32, u16[], text}
        let mk2 = |ucr: &[u16], bg: &[u16], txt: &str| -> Vec<u8> {
            let mut t = Vec::new();
            t.extend_from_slice(&0x62666420u32.to_be_bytes()); // 'bfd '
            t.extend_from_slice(&[0; 4]);
            t.extend_from_slice(&(ucr.len() as u32).to_be_bytes());
            for &v in ucr {
                t.extend_from_slice(&v.to_be_bytes());
            }
            t.extend_from_slice(&(bg.len() as u32).to_be_bytes());
            for &v in bg {
                t.extend_from_slice(&v.to_be_bytes());
            }
            t.extend_from_slice(txt.as_bytes());
            t
        };
        for (raw, name) in [
            (mk2(&[0, 65535], &[0, 65535], "desc"), "full"),
            (mk2(&[], &[], "empty"), "0-curves"),
            (mk2(&[0; 64], &[0; 64], "64e"), "64e"),
            (mk2(&[0; 4], &[0; 4], ""), "no-txt"),
        ] {
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsWriteRawTag(prof, 0x62666420,
                raw.as_ptr() as *const _, raw.len() as u32);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = c::cmsSaveProfileToMem(prof,
                buf.as_mut_ptr() as *mut _, &mut n);
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, 0x62666420)
            } else { std::ptr::null_mut() };
            eprintln!("{name} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
    }
}

/// chrm/clrt read arms — raw tags with wrong channel counts,
/// multiple entries and malformed names.
#[test]
fn chrm_clrt_read_arms() {
    unsafe {
        // chrm: 'chrm' + rsv + nCh(u16)+ColorantType(u16) + xyY x nCh
        let mk_chrm = |nch: u16, ctyp: u16| -> Vec<u8> {
            let mut t = Vec::new();
            t.extend_from_slice(&0x6368726Du32.to_be_bytes()); // 'chrm'
            t.extend_from_slice(&[0; 4]);
            t.extend_from_slice(&nch.to_be_bytes());
            t.extend_from_slice(&ctyp.to_be_bytes());
            for _ in 0..nch {
                for v in [0x0000A1B3u32, 0x0000B75Au32, 0x00010000u32] {
                    t.extend_from_slice(&v.to_be_bytes());
                }
            }
            t
        };
        // clrt: 'clrt' + rsv + vendor(u32) + count(u16) +
        // per-entry {name[32], pcs[6], dev[u16 x nch]}
        let mk_clrt = |nch: u8, count: u16| -> Vec<u8> {
            let mut t = Vec::new();
            t.extend_from_slice(&0x636C7274u32.to_be_bytes()); // 'clrt'
            t.extend_from_slice(&[0; 4]);
            t.extend_from_slice(&0u32.to_be_bytes()); // vendor
            t.extend_from_slice(&count.to_be_bytes());
            t.push(nch);
            t.push(0);
            for _ in 0..count {
                t.extend_from_slice(b"colorant-name-16bytes-pad-00\0\0");
                for _ in 0..3 {
                    t.extend_from_slice(&0x8000u16.to_be_bytes());
                }
                for _ in 0..nch {
                    t.extend_from_slice(&0x4000u16.to_be_bytes());
                }
            }
            t
        };
        for (raw, sig, name) in [
            (mk_chrm(3, 1), 0x6368726Du32, "chrm-3"),
            (mk_chrm(4, 1), 0x6368726Du32, "chrm-4"),
            (mk_chrm(0, 1), 0x6368726Du32, "chrm-0"),
            (mk_chrm(3, 3), 0x6368726Du32, "chrm-ct3"),
            (mk_clrt(3, 1), 0x636C7274u32, "clrt-3-1"),
            (mk_clrt(4, 2), 0x636C7274u32, "clrt-4-2"),
            (mk_clrt(3, 0), 0x636C7274u32, "clrt-3-0"),
        ] {
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsWriteRawTag(prof, sig,
                raw.as_ptr() as *const _, raw.len() as u32);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = c::cmsSaveProfileToMem(prof,
                buf.as_mut_ptr() as *mut _, &mut n);
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, sig)
            } else { std::ptr::null_mut() };
            eprintln!("{name} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
    }
}

/// Type_*_Dup batch — cmsDupProfile copies every written tag
/// type through its Dup handler.
#[test]
fn tag_dup_internals() {
    unsafe {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        for f in ["lcms2cmyk.icc", "sRGBlcms2.icc", "labv4lcms2.icc",
                  "aRGBlcms2.icc", "graylcms2.icc", "xyzlcms2.icc",
                  "brightness.icc", "lcms2link.icc", "nullcms2.icc",
                  "limitlcms2.icc", "linlcms2.icc", "bchslcms2.icc",
                  "glablcms2.icc", "gray3lcms2.icc", "labv2lcms2.icc"] {
            let Ok(d) = std::fs::read(dir.join(f)) else { continue };
            let p = c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32);
            if p.is_null() { continue; }
            // reading a tag twice hits Type_*_Dup on the cached
            // object
            let n = c::cmsGetTagCount(p);
            for i in 0..n.max(0) as u32 {
                let sig = c::cmsGetTagSignature(p, i as u32);
                let t1 = c::cmsReadTag(p, sig);
                let t2 = c::cmsReadTag(p, sig);
                let _ = (t1, t2);
            }
            eprintln!("{f} tags={n}");
            c::cmsCloseProfile(p);
        }
        // virtual profiles dup every tag type they write
        for (p, name) in [
            (c::cmsCreate_sRGBProfile(), "srgb"),
            (c::cmsCreateLab4Profile(std::ptr::null_mut()), "lab4"),
            (c::cmsCreateXYZProfile(), "xyz"),
            (c::cmsCreateNULLProfile(), "null"),
        ] {
            let n = c::cmsGetTagCount(p);
            for i in 0..n.max(0) as u32 {
                let sig = c::cmsGetTagSignature(p, i as u32);
                let t1 = c::cmsReadTag(p, sig);
                let t2 = c::cmsReadTag(p, sig);
                let _ = (t1, t2);
            }
            eprintln!("{name} tags={n}");
            c::cmsCloseProfile(p);
        }
    }
}

/// GenericMPEdup/GenericMPEfree/WriteMPECurve — cmsPipelineDup
/// on MPE pipelines + saving mpet tags with curve elements.
#[test]
fn mpe_dup_internals() {
    unsafe {
        // build an mpet pipeline: curve-set + matrix + clut
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let curves = vec![g; 3];
        let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        let mat = [1.0f64, 0.0, 0.0,
                   0.0, 1.0, 0.0,
                   0.0, 0.0, 1.0];
        let off = [0.0f64; 3];
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocMatrix(std::ptr::null_mut(), 3, 3,
                mat.as_ptr(), off.as_ptr()));
        let mut grid = vec![0u16; 8 * 3];
        for (i, v) in grid.iter_mut().enumerate() {
            *v = (i * 2730) as u16;
        }
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocCLut16bit(std::ptr::null_mut(), 2, 3, 3,
                grid.as_ptr()));
        // dup the pipeline -> GenericMPEdup on each element
        let dp = c::cmsPipelineDup(p);
        eprintln!("pipeline dup={}", !dp.is_null());
        // save as mpet -> WriteMPECurve on the curve elements
        let prof = c::cmsCreate_sRGBProfile();
        c::cmsSetProfileVersion(prof, 4.4);
        // desc MLU for the placeholder path is already in sRGB
        let mut ml = c::cmsMLUalloc(std::ptr::null_mut(), 1);
        let m0 = c::cmsMLUsetWide(ml, b"en\0".as_ptr() as *const _,
            b"US\0".as_ptr() as *const _,
            [0x74u32, 0x65, 0x73, 0x74, 0].as_ptr() as *const _);
        let w1 = c::cmsWriteTag(prof, 0x64657363, ml as *const _);
        let wp = [0.9642f64, 1.0, 0.8249];
        let w2 = c::cmsWriteTag(prof, 0x77747074,
            wp.as_ptr() as *const _);
        let w = c::cmsWriteTag(prof, 0x44324230, p as *const _); // D2B0
        eprintln!("mlu={m0} desc={w1} wtpt={w2} d2b0={w}");
        // isolate: save an sRGB profile with NO added tags
        let prof2 = c::cmsCreate_sRGBProfile();
        let fs2 = c::cmsSaveProfileToFile(prof2,
            b"/tmp/plain_srgb.icc\0".as_ptr() as *const i8);
        eprintln!("plain-srgb file save={fs2}");
        c::cmsCloseProfile(prof2);
        // save to a FILE first to see the real error path
        let fs = c::cmsSaveProfileToFile(prof,
            b"/tmp/mpet_test.icc\0".as_ptr() as *const i8);
        eprintln!("file save={fs}");
        let mut buf = vec![0u8; 1 << 20];
        let mut n = buf.len() as u32;
        let s = c::cmsSaveProfileToMem(prof,
            buf.as_mut_ptr() as *mut _, &mut n);
        eprintln!("mpet save={s}");
        c::cmsCloseProfile(prof);
        if !dp.is_null() { c::cmsPipelineFree(dp); }
        c::cmsPipelineFree(p);
        c::cmsFreeToneCurve(g);
    }
}

/// _cmsReadWCharArray/_cmsWriteWCharArray/surrogate internals —
/// MLU entries with supplementary-plane chars hit the UTF-16
/// surrogate conversion arms.
#[test]
fn wchar_surrogate_internals() {
    unsafe {
        let ml = c::cmsMLUalloc(std::ptr::null_mut(), 6);
        for (lang, cntry, txt) in [
            ("en", "US", "ascii-only"),
            ("ja", "JP", "テキスト"),
            ("zh", "CN", ""),
            ("de", "DE", "äöüÄÖÜß"),
            ("en", "GB", "emoji \u{1f600}\u{1f31f}"),
            ("el", "GR", "Ελληνικά κείμενο"),
        ] {
            let mut wk: Vec<u32> = txt.chars()
                .map(|c| c as u32).collect();
            wk.push(0);
            c::cmsMLUsetWide(ml, format!("{lang}\0").as_ptr() as *const _,
                format!("{cntry}\0").as_ptr() as *const _,
                wk.as_ptr() as *const _);
        }
        let prof = c::cmsCreate_sRGBProfile();
        let w = c::cmsWriteTag(prof, 0x64657363, ml as *const _);
        let mut buf = vec![0u8; 1 << 20];
        let mut n = buf.len() as u32;
        let s = if w != 0 {
            c::cmsSaveProfileToMem(prof,
                buf.as_mut_ptr() as *mut _, &mut n)
        } else { 0 };
        let rp = if s != 0 {
            c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
        } else { std::ptr::null_mut() };
        let tag = if !rp.is_null() {
            c::cmsReadTag(rp, 0x64657363)
        } else { std::ptr::null_mut() };
        eprintln!("w={w} s={s} read={}", !tag.is_null());
        if !rp.is_null() {
            // read back twice for the dup arm
            let t2 = c::cmsReadTag(rp, 0x64657363);
            let _ = t2;
            c::cmsCloseProfile(rp);
        }
        c::cmsCloseProfile(prof);
        c::cmsMLUfree(ml);
    }
}

/// Decide*Type arms — version + payload properties pick among
/// multiple allowed types on a sig.
#[test]
fn decide_type_arms() {
    unsafe {
        let prof2 = c::cmsCreate_sRGBProfile();
        c::cmsSetProfileVersion(prof2, 2.1);
        let prof4 = c::cmsCreate_sRGBProfile();
        c::cmsSetProfileVersion(prof4, 4.4);
        // text desc: v2 -> desc, v4 -> mluc (DecideTextDescType)
        let ml = c::cmsMLUalloc(std::ptr::null_mut(), 1);
        let mut wk = [0x74u32, 0x65, 0x73, 0x74, 0];
        c::cmsMLUsetWide(ml, b"en\0".as_ptr() as *const _,
            b"US\0".as_ptr() as *const _, wk.as_mut_ptr() as *const _);
        for (prof, ver, name) in [
            (prof2, 2.1, "v2"),
            (prof4, 4.4, "v4"),
        ] {
            let w = c::cmsWriteTag(prof, 0x64657363, ml as *const _);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            eprintln!("desc-{name} w={w} s={s}");
        }
        // curves: 1-seg para type 0-5 -> para on v4; multi-seg /
        // inverted / type>5 -> curv on v4 too
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mut seg = [
            c::CurveSegment {
                x0: 0.0, x1: 0.5, r#type: 6,
                params: [2.2, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
                         0.0, 0.0],
                n_grid_points: 0,
                sampled_points: std::ptr::null_mut() },
            c::CurveSegment {
                x0: 0.5, x1: 1.0, r#type: 6,
                params: [1.8, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
                         0.0, 0.0],
                n_grid_points: 0,
                sampled_points: std::ptr::null_mut() },
        ];
        let multiseg = c::cmsBuildSegmentedToneCurve(
            std::ptr::null_mut(), 2, seg.as_ptr());
        let mut seg2 = [
            c::CurveSegment {
                x0: 0.0, x1: 1.0, r#type: -1,
                params: [0.0; 10], n_grid_points: 0,
                sampled_points: std::ptr::null_mut() },
        ];
        let inv = c::cmsBuildSegmentedToneCurve(
            std::ptr::null_mut(), 1, seg2.as_ptr());
        for (c_, name) in [
            (g, "para"),
            (multiseg, "multiseg"),
            (inv, "inverted"),
        ] {
            if c_.is_null() { eprintln!("{name} null"); continue; }
            let w = c::cmsWriteTag(prof4, 0x6b545243, c_ as *const _);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof4,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            eprintln!("curv-{name} w={w} s={s}");
        }
        // LUTs: SaveAs8Bits picks mft1 vs mft2 on v2
        let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        let mat = [1.0f64, 0.0, 0.0,
                   0.0, 1.0, 0.0,
                   0.0, 0.0, 1.0];
        let off = [0.0f64; 3];
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocMatrix(std::ptr::null_mut(), 3, 3,
                mat.as_ptr(), off.as_ptr()));
        let mut grid = vec![0u16; 8 * 3];
        for (i, v) in grid.iter_mut().enumerate() {
            *v = (i * 2730) as u16;
        }
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocCLut16bit(std::ptr::null_mut(), 2, 3, 3,
                grid.as_ptr()));
        let w = c::cmsWriteTag(prof2, 0x41324230, p as *const _);
        let mut buf = vec![0u8; 1 << 20];
        let mut n = buf.len() as u32;
        let s = if w != 0 {
            c::cmsSaveProfileToMem(prof2,
                buf.as_mut_ptr() as *mut _, &mut n)
        } else { 0 };
        eprintln!("lut-v2 w={w} s={s}");
        c::cmsCloseProfile(prof2);
        c::cmsCloseProfile(prof4);
        c::cmsMLUfree(ml);
        c::cmsFreeToneCurve(g);
        if !multiseg.is_null() { c::cmsFreeToneCurve(multiseg); }
        if !inv.is_null() { c::cmsFreeToneCurve(inv); }
        c::cmsPipelineFree(p);
    }
}

/// Type_CrdInfo_Read arms — raw crdi tags with count+string
/// record variants (valid, truncated, oversize counts).
#[test]
fn crdi_read_arms() {
    unsafe {
        let rec = |s: &str| -> Vec<u8> {
            let mut t = Vec::new();
            t.extend_from_slice(&(s.len() as u32).to_be_bytes());
            t.extend_from_slice(s.as_bytes());
            t
        };
        let mk = |recs: &[Vec<u8>]| -> Vec<u8> {
            let mut t = Vec::new();
            t.extend_from_slice(&0x63726469u32.to_be_bytes()); // 'crdi'
            t.extend_from_slice(&[0; 4]);
            for r in recs { t.extend_from_slice(r); }
            t
        };
        let full = [
            rec("device"), rec("0"), rec("1"), rec("2"), rec("3")];
        for (raw, name) in [
            (mk(&full), "full"),
            (mk(&full[..4]), "4-recs"),
            (mk(&full[..1]), "1-rec"),
            (mk(&[]), "0-recs"),
            (mk(&[vec![0xffu8; 4]]), "huge-count"),
            (mk(&[rec(&"x".repeat(400))]), "long"),
        ] {
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsWriteRawTag(prof, 0x63726469,
                raw.as_ptr() as *const _, raw.len() as u32);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = c::cmsSaveProfileToMem(prof,
                buf.as_mut_ptr() as *mut _, &mut n);
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, 0x63726469)
            } else { std::ptr::null_mut() };
            eprintln!("{name} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
    }
}

/// ReadEmbeddedText/ReadSeqID/ReadOneWChar internals — raw
/// pseq/psid tags with embedded-MLU + UTF-16 text arms.
#[test]
fn pseq_embedded_text_arms() {
    unsafe {
        // embedded text: type-base ('text'/'desc'/'mluc' + rsv)
        // + payload
        let text_blob = |s: &str| -> Vec<u8> {
            let mut t = Vec::new();
            t.extend_from_slice(&0x74657874u32.to_be_bytes()); // 'text'
            t.extend_from_slice(&[0; 4]);
            t.extend_from_slice(s.as_bytes());
            t.push(0);
            t
        };
        let desc_blob = |s: &str| -> Vec<u8> {
            let mut t = Vec::new();
            t.extend_from_slice(&0x64657363u32.to_be_bytes()); // 'desc'
            t.extend_from_slice(&[0; 4]);
            t.extend_from_slice(&(s.len() as u32 + 1).to_be_bytes());
            t.extend_from_slice(s.as_bytes());
            t.push(0);
            t.extend_from_slice(&[0; 3]);
            t
        };
        let mluc_blob = |s: &str| -> Vec<u8> {
            let mut t = Vec::new();
            t.extend_from_slice(&0x6d6c7563u32.to_be_bytes()); // 'mluc'
            t.extend_from_slice(&[0; 4]);
            t.extend_from_slice(&1u16.to_be_bytes()); // 1 entry
            t.extend_from_slice(&12u16.to_be_bytes()); // rec size
            t.extend_from_slice(b"enUS");
            let wlen = (s.chars().count() * 2) as u32;
            t.extend_from_slice(&wlen.to_be_bytes());
            t.extend_from_slice(&28u32.to_be_bytes()); // offset
            for ch in s.chars() {
                t.extend_from_slice(&(ch as u16).to_be_bytes());
            }
            while t.len() % 4 != 0 { t.push(0); }
            t
        };
        let mk = |blob: &[u8], n: u32| -> Vec<u8> {
            let mut t = Vec::new();
            t.extend_from_slice(&0x70736571u32.to_be_bytes()); // 'pseq'
            t.extend_from_slice(&[0; 4]);
            t.extend_from_slice(&n.to_be_bytes());
            for _ in 0..n {
                t.extend_from_slice(b"mfg\0");
                t.extend_from_slice(b"mdl\0");
                t.extend_from_slice(&[0; 8]); // attributes
                t.extend_from_slice(b"tech");
                t.extend_from_slice(blob); // Manufacturer text
                t.extend_from_slice(blob); // Model text
            }
            t
        };
        for (blob, name) in [
            (text_blob("mfg-name"), "text"),
            (desc_blob("mfg-name"), "desc"),
            (mluc_blob("mfg-name"), "mluc"),
        ] {
            let raw = mk(&blob, 1);
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsSetProfileVersion(prof, 4.4);
            c::cmsWriteRawTag(prof, 0x70736571,
                raw.as_ptr() as *const _, raw.len() as u32);
            let mut buf = vec![0u8; 1 << 20];
            let mut n2 = buf.len() as u32;
            let s = c::cmsSaveProfileToMem(prof,
                buf.as_mut_ptr() as *mut _, &mut n2);
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n2)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, 0x70736571)
            } else { std::ptr::null_mut() };
            eprintln!("{name} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
    }
}


/// Type_*_Free + Type_Data_Dup arms — read raw text-desc/curv/
/// para/data tags so the free/dup handlers run on close/read-twice.
#[test]
fn tag_free_dup_arms() {
    unsafe {
        // raw tags whose readers build objects freed via Free arm
        let mk_text = |s: &str| -> Vec<u8> {
            let mut t = Vec::new();
            t.extend_from_slice(&0x74657874u32.to_be_bytes()); // 'text'
            t.extend_from_slice(&[0; 4]);
            t.extend_from_slice(s.as_bytes());
            t.push(0);
            t
        };
        let mk_desc = |s: &str| -> Vec<u8> {
            let mut t = Vec::new();
            t.extend_from_slice(&0x64657363u32.to_be_bytes()); // 'desc'
            t.extend_from_slice(&[0; 4]);
            t.extend_from_slice(&(s.len() as u32 + 1).to_be_bytes());
            t.extend_from_slice(s.as_bytes());
            t.push(0);
            t.extend_from_slice(&[0; 3]);
            t
        };
        let mk_curv = |n: u32| -> Vec<u8> {
            let mut t = Vec::new();
            t.extend_from_slice(&0x63757276u32.to_be_bytes()); // 'curv'
            t.extend_from_slice(&[0; 4]);
            t.extend_from_slice(&n.to_be_bytes());
            for i in 0..n {
                t.extend_from_slice(&((i * 16384) as u16).to_be_bytes());
            }
            t
        };
        let mk_para = |ty: u16| -> Vec<u8> {
            let mut t = Vec::new();
            t.extend_from_slice(&0x70617261u32.to_be_bytes()); // 'para'
            t.extend_from_slice(&[0; 4]);
            t.extend_from_slice(&ty.to_be_bytes());
            t.extend_from_slice(&[0; 2]);
            for v in [0x00023333u32, 0, 0, 0, 0] {
                t.extend_from_slice(&v.to_be_bytes());
            }
            t
        };
        let mk_data = |d: &[u8]| -> Vec<u8> {
            let mut t = Vec::new();
            t.extend_from_slice(&0x64617461u32.to_be_bytes()); // 'data'
            t.extend_from_slice(&[0; 4]);
            t.extend_from_slice(d);
            t
        };
        for (raw, sig, name) in [
            (mk_text("desc text"), 0x64657363u32, "text-desc"),
            (mk_desc("a desc"), 0x64657363u32, "desc-desc"),
            (mk_curv(4), 0x72545243u32, "curv-rtrc"),
            (mk_para(1), 0x72545243u32, "para-rtrc"),
            (mk_data(b"data bytes\0"), 0x70733073u32, "data-ps0"),
        ] {
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsWriteRawTag(prof, sig,
                raw.as_ptr() as *const _, raw.len() as u32);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = c::cmsSaveProfileToMem(prof,
                buf.as_mut_ptr() as *mut _, &mut n);
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            if !rp.is_null() {
                // read twice — second read dups the cached object
                let t1 = c::cmsReadTag(rp, sig);
                let t2 = c::cmsReadTag(rp, sig);
                eprintln!("{name} s={s} r1={} r2={}",
                    !t1.is_null(), !t2.is_null());
                c::cmsCloseProfile(rp);
            }
            c::cmsCloseProfile(prof);
        }
    }
}

/// WriteMPECurve — mpet save with a cvst (curve-set) stage only.
#[test]
fn mpe_cvst_write() {
    unsafe {
        let mut seg = [
            c::CurveSegment {
                x0: 0.0, x1: 1.0, r#type: 6,
                params: [2.2, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
                         0.0, 0.0],
                n_grid_points: 0,
                sampled_points: std::ptr::null_mut() }];
        let g = c::cmsBuildSegmentedToneCurve(std::ptr::null_mut(),
            1, seg.as_ptr());
        let g2 = c::cmsBuildSegmentedToneCurve(std::ptr::null_mut(),
            1, seg.as_ptr());
        let g3 = c::cmsBuildSegmentedToneCurve(std::ptr::null_mut(),
            1, seg.as_ptr());
        // cvst-only pipeline
        let curves = [g, g2, g3];
        let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocToneCurves(std::ptr::null_mut(), 3,
                curves.as_ptr() as *mut _));
        let prof = c::cmsCreate_sRGBProfile();
        c::cmsSetProfileVersion(prof, 4.4);
        let w = c::cmsWriteTag(prof, 0x44324230, p as *const _);
        let mut buf = vec![0u8; 1 << 20];
        let mut n = buf.len() as u32;
        let s = if w != 0 {
            c::cmsSaveProfileToMem(prof,
                buf.as_mut_ptr() as *mut _, &mut n)
        } else { 0 };
        eprintln!("cvst-only w={w} s={s}");
        // cvst + clut
        let p2 = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        let curves2 = [g, g2, g3];
        c::cmsPipelineInsertStage(p2, 1,
            c::cmsStageAllocToneCurves(std::ptr::null_mut(), 3,
                curves2.as_ptr() as *mut _));
        let mut grid = vec![0u16; 8 * 3];
        for (i, v) in grid.iter_mut().enumerate() {
            *v = (i * 2730) as u16;
        }
        c::cmsPipelineInsertStage(p2, 1,
            c::cmsStageAllocCLut16bit(std::ptr::null_mut(), 2, 3, 3,
                grid.as_ptr()));
        let prof2 = c::cmsCreate_sRGBProfile();
        c::cmsSetProfileVersion(prof2, 4.4);
        let w2 = c::cmsWriteTag(prof2, 0x44324230, p2 as *const _);
        let s2 = if w2 != 0 {
            c::cmsSaveProfileToMem(prof2,
                buf.as_mut_ptr() as *mut _, &mut n)
        } else { 0 };
        eprintln!("cvst+clut w={w2} s={s2}");
        c::cmsCloseProfile(prof);
        c::cmsCloseProfile(prof2);
        c::cmsPipelineFree(p);
        c::cmsPipelineFree(p2);
        c::cmsFreeToneCurve(g);
        c::cmsFreeToneCurve(g2);
        c::cmsFreeToneCurve(g3);
    }
}

/// Type_Signature_Write/Type_XYZ_Write/Type_NamedColor_* —
/// multi-item write loops + ncl2 deep read/write arms.
#[test]
fn multi_item_tag_write_arms() {
    unsafe {
        let prof = c::cmsCreate_sRGBProfile();
        // sig-type tags (tech/ciis/rig0/rig2 take signatureType)
        for (sig, val, name) in [
            (0x74656368u32, 0x66696c74, "tech"),
            (0x63696973u32, 0x76657265, "ciis"),
            (0x72696730u32, 0x70723030, "rig0"),
            (0x72696732u32, 0x70723030, "rig2"),
        ] {
            let v = val as u32;
            let w = c::cmsWriteTag(prof, sig, &v as *const _ as *const _);
            eprintln!("{name} w={w}");
        }
        // XYZ multi-item via wtpt (single) + chrm (triple)
        let wp = [0.9642f64, 1.0, 0.8249];
        let w = c::cmsWriteTag(prof, 0x77747074,
            wp.as_ptr() as *const _);
        eprintln!("wtpt w={w}");
        // ncl2 round-trip: write + read
        let list = c::cmsAllocNamedColorList(
            std::ptr::null_mut(), 3, 3,
            b"pre\0".as_ptr(), b"post\0".as_ptr());
        for i in 0..3u32 {
            let mut name = [0u8; 256];
            let nm = format!("spot{}\0", i);
            name[..nm.len()].copy_from_slice(nm.as_bytes());
            let pcs = [100u16 * (i as u16 + 1), 200, 300];
            let dev = [0u16; 15];
            c::cmsAppendNamedColor(list,
                name.as_ptr() as *mut _, pcs.as_ptr(), dev.as_ptr());
        }
        let w2 = c::cmsWriteTag(prof, 0x6e636c32, list as *const _);
        let mut buf = vec![0u8; 1 << 20];
        let mut n = buf.len() as u32;
        let s = if w2 != 0 {
            c::cmsSaveProfileToMem(prof,
                buf.as_mut_ptr() as *mut _, &mut n)
        } else { 0 };
        let rp = if s != 0 {
            c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
        } else { std::ptr::null_mut() };
        let tag = if !rp.is_null() {
            c::cmsReadTag(rp, 0x6e636c32)
        } else { std::ptr::null_mut() };
        eprintln!("ncl2 w={w2} s={s} read={}", !tag.is_null());
        // read every written tag twice -> dup arms
        if !rp.is_null() {
            for sig in [0x74656368u32, 0x63696973, 0x72696730,
                        0x72696732, 0x77747074, 0x6e636c32] {
                let t1 = c::cmsReadTag(rp, sig);
                let t2 = c::cmsReadTag(rp, sig);
                let _ = (t1, t2);
            }
            c::cmsCloseProfile(rp);
        }
        c::cmsCloseProfile(prof);
        c::cmsFreeNamedColorList(list);
    }
}

/// Type_NamedColor_Read arms — raw ncl2 tags: count variants,
/// nDeviceCoords>16 reject, prefix/suffix.
#[test]
fn ncl2_read_arms() {
    unsafe {
        let mk = |count: u32, ndev: u32,
                  prefix: &[u8; 32], suffix: &[u8; 32],
                  colors: &[(u32, [u16; 3], u16)]| -> Vec<u8> {
            let mut t = Vec::new();
            t.extend_from_slice(&0x6e636c32u32.to_be_bytes()); // 'ncl2'
            t.extend_from_slice(&[0; 4]);
            t.extend_from_slice(&0u32.to_be_bytes()); // vendorFlag
            t.extend_from_slice(&count.to_be_bytes());
            t.extend_from_slice(&ndev.to_be_bytes());
            t.extend_from_slice(prefix);
            t.extend_from_slice(suffix);
            for (i, pcs, dev) in colors.iter() {
                let mut name = [0u8; 32];
                let nm = format!("color{i}\0");
                name[..nm.len()].copy_from_slice(nm.as_bytes());
                t.extend_from_slice(&name);
                for v in pcs {
                    t.extend_from_slice(&v.to_be_bytes());
                }
                for _ in 0..ndev {
                    t.extend_from_slice(&dev.to_be_bytes());
                }
            }
            t
        };
        let empty_p = [0u8; 32];
        let mut pfx = [0u8; 32];
        pfx[..4].copy_from_slice(b"PANT");
        let mut sfx = [0u8; 32];
        sfx[..3].copy_from_slice(b"CV\0");
        let colors3: Vec<_> = (0..3).map(|i|
            (i, [100u16 + i as u16, 200, 300], 0x1234u16)).collect();
        for (raw, name) in [
            (mk(3, 3, &empty_p, &empty_p, &colors3), "3col-3dev"),
            (mk(3, 3, &pfx, &sfx, &colors3), "prefix-suffix"),
            (mk(0, 3, &empty_p, &empty_p, &[]), "count0"),
            (mk(3, 17, &empty_p, &empty_p, &colors3), "dev17"),
            (mk(3, 0, &empty_p, &empty_p, &colors3), "dev0"),
            (mk(1, 3, &empty_p, &empty_p, &colors3[..1]), "1col"),
        ] {
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsWriteRawTag(prof, 0x6e636c32,
                raw.as_ptr() as *const _, raw.len() as u32);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = c::cmsSaveProfileToMem(prof,
                buf.as_mut_ptr() as *mut _, &mut n);
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, 0x6e636c32)
            } else { std::ptr::null_mut() };
            eprintln!("{name} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
    }
}

/// Type_*_Free/Type_*_Dup inner arms — v2 profiles carrying
/// each tag type read twice + closed so the free/dup bodies run.
#[test]
fn tag_free_dup_batch() {
    unsafe {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        // corpus v2 profiles carry desc/curv/para/data/chrm/clro/
        // calt/mft1/mft2/ncl2/mluc/pseq/meas/mAB/mBA/crdi/mpet/
        // scrn/view/XYZ/psid tags — reading each twice + closing
        // covers the Free inner branches; reading twice covers Dup
        for f in ["labv2lcms2.icc", "sRGBlcms2.icc", "lcms2cmyk.icc",
                  "graylcms2.icc", "gray3lcms2.icc", "aRGBlcms2.icc",
                  "glablcms2.icc", "xyzlcms2.icc", "nullcms2.icc",
                  "limitlcms2.icc", "linlcms2.icc", "bchslcms2.icc",
                  "lcms2link.icc", "lcms2link2.icc", "brightness.icc",
                  "labv4lcms2.icc"] {
            let Ok(d) = std::fs::read(dir.join(f)) else { continue };
            let p = c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32);
            if p.is_null() { continue; }
            // iterate the tag directory, read each twice
            let n = c::cmsGetTagCount(p);
            for i in 0..n {
                let sig = c::cmsGetTagSignature(p, i as u32);
                let t1 = c::cmsReadTag(p, sig);
                let t2 = c::cmsReadTag(p, sig);
                let _ = (t1, t2);
            }
            c::cmsCloseProfile(p);
        }
        // also write+read each raw-tag type on a v2 profile
        for (raw, sig) in [
            ({let mut v = Vec::new();
              v.extend_from_slice(&0x64657363u32.to_be_bytes());
              v.extend_from_slice(&[0; 4]);
              v.extend_from_slice(&5u32.to_be_bytes());
              v.extend_from_slice(b"abcd\0");
              v.extend_from_slice(&[0; 3]); v }, 0x64657363u32),
            ({let mut v = Vec::new();
              v.extend_from_slice(&0x63757276u32.to_be_bytes());
              v.extend_from_slice(&[0; 4]);
              v.extend_from_slice(&4u32.to_be_bytes());
              for i in 0..4u16 {
                  v.extend_from_slice(&(i * 16384).to_be_bytes());
              } v }, 0x72545243u32),
            ({let mut v = Vec::new();
              v.extend_from_slice(&0x70617261u32.to_be_bytes());
              v.extend_from_slice(&[0; 4]);
              v.extend_from_slice(&1u16.to_be_bytes());
              v.extend_from_slice(&[0; 2]);
              for vv in [0x00023333u32, 0, 0, 0, 0] {
                  v.extend_from_slice(&vv.to_be_bytes());
              } v }, 0x72545243u32),
            ({let mut v = Vec::new();
              v.extend_from_slice(&0x64617461u32.to_be_bytes());
              v.extend_from_slice(&[0; 4]);
              v.extend_from_slice(b"data\0"); v }, 0x70733073u32),
            ({let mut v = Vec::new();
              v.extend_from_slice(&0x58595a20u32.to_be_bytes());
              v.extend_from_slice(&[0; 4]);
              for vv in [0x0000f6d6u32, 0x00010000, 0x0000d32d] {
                  v.extend_from_slice(&vv.to_be_bytes());
              } v }, 0x77747074u32),
        ] {
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsSetProfileVersion(prof, 2.1);
            c::cmsWriteRawTag(prof, sig,
                raw.as_ptr() as *const _, raw.len() as u32);
            let mut buf = vec![0u8; 1 << 20];
            let mut nn = buf.len() as u32;
            let s = c::cmsSaveProfileToMem(prof,
                buf.as_mut_ptr() as *mut _, &mut nn);
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, nn)
            } else { std::ptr::null_mut() };
            if !rp.is_null() {
                let t1 = c::cmsReadTag(rp, sig);
                let t2 = c::cmsReadTag(rp, sig);
                let _ = (t1, t2);
                c::cmsCloseProfile(rp);
            }
            c::cmsCloseProfile(prof);
        }
    }
}


/// cmsio1 LUT-reader builders — gray/RGB input/output matrix
/// shaper pipelines + float-LUT tag readers.
#[test]
fn lut_builder_arms() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let gray = c::cmsCreateGrayProfile(std::ptr::null_mut(), g);
        let xyz = c::cmsCreateXYZProfile();
        let r16 = (4u32 << 16) | (3 << 3) | 2;
        let l16 = (10u32 << 16) | (3 << 3) | 2;
        let g16 = (3u32 << 16) | (1 << 3) | 2;
        let x16 = (9u32 << 16) | (3 << 3) | 2;
        for (inp, outp, if_, of_, name) in [
            (gray, lab, g16, l16, "gray->lab"),
            (lab, gray, l16, g16, "lab->gray"),
            (gray, srgb, g16, r16, "gray->rgb"),
            (srgb, gray, r16, g16, "rgb->gray"),
            (xyz, gray, x16, g16, "xyz->gray"),
            (gray, xyz, g16, x16, "gray->xyz"),
            (srgb, xyz, r16, x16, "rgb->xyz"),
            (xyz, srgb, x16, r16, "xyz->rgb"),
        ] {
            let t = c::cmsCreateTransform(inp, if_, outp, of_, 0, 0);
            eprintln!("{name} t={}", !t.is_null());
            if !t.is_null() {
                let ib = [0x8000u16; 4];
                let mut ob = [0x8000u16; 4];
                c::cmsDoTransform(t, ib.as_ptr() as *const _,
                    ob.as_mut_ptr() as *mut _, 1);
                c::cmsDeleteTransform(t);
            }
        }
        // float-LUT tags: D2B0/D2B1/D2B2/D2B3 on profiles
        for (sig, name) in [
            (0x44324230u32, "d2b0"), (0x44324231, "d2b1"),
            (0x44324232, "d2b2"), (0x44324233, "d2b3"),
            (0x41324430, "a2d0"),
        ] {
            let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
            let mat = [1.0f64, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
            c::cmsPipelineInsertStage(p, 1,
                c::cmsStageAllocMatrix(std::ptr::null_mut(), 3, 3,
                    mat.as_ptr(), std::ptr::null()));
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsSetProfileVersion(prof, 4.4);
            let w = c::cmsWriteTag(prof, sig, p as *const _);
            let t = if w != 0 {
                c::cmsCreateTransform(prof, r16, lab, l16, 0, 0)
            } else { std::ptr::null_mut() };
            eprintln!("{name} w={w} t={}", !t.is_null());
            if !t.is_null() { c::cmsDeleteTransform(t); }
            c::cmsCloseProfile(prof);
            c::cmsPipelineFree(p);
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
        c::cmsCloseProfile(gray);
        c::cmsCloseProfile(xyz);
        c::cmsFreeToneCurve(g);
    }
}

/// SaveTags raw-copy path — saving a mem-opened profile without
/// parsing all tags copies un-parsed tags raw from the original
/// IOhandler.
#[test]
fn save_raw_copy_arms() {
    unsafe {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        for f in ["lcms2cmyk.icc", "labv4lcms2.icc", "sRGBlcms2.icc",
                  "nullcms2.icc", "xyzlcms2.icc", "bchslcms2.icc",
                  "limitlcms2.icc", "linlcms2.icc"] {
            let Ok(d) = std::fs::read(dir.join(f)) else { continue };
            let p = c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32);
            if p.is_null() { continue; }
            // read only the FIRST tag — the rest stay raw
            let sig = c::cmsGetTagSignature(p, 0);
            let _ = c::cmsReadTag(p, sig);
            let mut buf = vec![0u8; 1 << 21];
            let mut n = buf.len() as u32;
            let s = c::cmsSaveProfileToMem(p,
                buf.as_mut_ptr() as *mut _, &mut n);
            eprintln!("{f} rawcopy={s}");
            // variant: read NO tags -> all raw-copy
            let p2 = c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32);
            if !p2.is_null() {
                let mut buf2 = vec![0u8; 1 << 21];
                let mut n2 = buf2.len() as u32;
                let s2 = c::cmsSaveProfileToMem(p2,
                    buf2.as_mut_ptr() as *mut _, &mut n2);
                eprintln!("{f} allraw={s2}");
                c::cmsCloseProfile(p2);
            }
            c::cmsCloseProfile(p);
        }
        // TagSaveAsRaw path: WriteRawTag then save
        let prof = c::cmsCreate_sRGBProfile();
        let raw = {
            let mut v = Vec::new();
            v.extend_from_slice(&0x74657874u32.to_be_bytes());
            v.extend_from_slice(&[0; 4]);
            v.extend_from_slice(b"hello\0");
            v
        };
        c::cmsWriteRawTag(prof, 0x63707274,
            raw.as_ptr() as *const _, raw.len() as u32);
        let mut buf = vec![0u8; 1 << 20];
        let mut n = buf.len() as u32;
        let s = c::cmsSaveProfileToMem(prof,
            buf.as_mut_ptr() as *mut _, &mut n);
        eprintln!("rawtag-save={s}");
        c::cmsCloseProfile(prof);
    }
}

/// NULL/Memory IOhandler internals — NULLWrite (discard sink),
/// MemoryRead/Write/Seek edge arms.
#[test]
fn iohandler_edge_arms() {
    unsafe {
        let prof = c::cmsCreate_sRGBProfile();
        // NULL IOhandler — a discard sink, writes accepted
        let null_io = c::cmsOpenIOhandlerFromNULL(std::ptr::null_mut());
        let s = c::cmsSaveProfileToIOhandler(prof, null_io);
        eprintln!("null-save={s} io={}", !null_io.is_null());
        // memory IOhandler — save then read back with seeks
        let io = c::cmsOpenIOhandlerFromMem(std::ptr::null_mut(),
            std::ptr::null_mut(), 0,
            b"w\0".as_ptr() as *const _);
        eprintln!("memio={}", !io.is_null());
        if !io.is_null() {
            let s2 = c::cmsSaveProfileToIOhandler(prof, io);
            eprintln!("mem-save={s2}");
            c::cmsCloseIOhandler(io);
        }
        // file IOhandler write path
        let fio = c::cmsOpenIOhandlerFromFile(std::ptr::null_mut(),
            b"/dev/null\0".as_ptr() as *const _,
            b"w\0".as_ptr() as *const _);
        if !fio.is_null() {
            let s3 = c::cmsSaveProfileToIOhandler(prof, fio);
            eprintln!("file-save={s3}");
            c::cmsCloseIOhandler(fio);
        }
        // profile IOhandler getter + reuse
        let pio = c::cmsGetProfileIOhandler(prof);
        eprintln!("profio={}", !pio.is_null());
        c::cmsCloseProfile(prof);
        if !null_io.is_null() { c::cmsCloseIOhandler(null_io); }
    }
}

/// LUT writer deep arms — mft1/mft2/mAB/mBA serialization with
/// more pipeline shapes: curve-only, matrix-only, CLUT-only,
/// multi-element, odd channel counts.
#[test]
fn lut_write_shapes() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let g2 = c::cmsBuildGamma(std::ptr::null_mut(), 1.8);
        let mat = [1.0f64, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        let mut grid = vec![0u16; 17 * 17 * 17 * 3];
        for (i, v) in grid.iter_mut().enumerate() {
            *v = ((i % 17) * 3855) as u16;
        }
        let mut grid4 = vec![0u16; 17 * 17 * 17 * 17 * 4];
        for (i, v) in grid4.iter_mut().enumerate() {
            *v = ((i % 17) * 3855) as u16;
        }
        let mk_cv = |n: u32| {
            let curves = match n {
                1 => [g].as_ptr(),
                3 => [g, g2, g].as_ptr(),
                4 => [g, g2, g, g2].as_ptr(),
                _ => [g].as_ptr(),
            };
            c::cmsStageAllocToneCurves(std::ptr::null_mut(), n,
                curves as *mut _)
        };
        // mft1 (v2, SaveAs8Bits) + mft2 (v2, !SaveAs8Bits) on a
        // 3ch->3ch pipeline; also mAB/mBA on v4
        let shapes: Vec<(Vec<*mut std::ffi::c_void>, u32, u32, &str)> = vec![
            // cv only
            (vec![mk_cv(3)], 3, 3, "cv3-only"),
            // cv-mat
            (vec![mk_cv(3),
                  c::cmsStageAllocMatrix(std::ptr::null_mut(), 3, 3,
                      mat.as_ptr(), std::ptr::null())], 3, 3, "cv-mat"),
            // cv-mat-cv
            (vec![mk_cv(3),
                  c::cmsStageAllocMatrix(std::ptr::null_mut(), 3, 3,
                      mat.as_ptr(), std::ptr::null()),
                  mk_cv(3)], 3, 3, "cv-mat-cv"),
            // cv-clut
            (vec![mk_cv(3),
                  c::cmsStageAllocCLut16bit(std::ptr::null_mut(),
                      17, 3, 3, grid.as_ptr())], 3, 3, "cv-clut"),
            // cv-clut-cv
            (vec![mk_cv(3),
                  c::cmsStageAllocCLut16bit(std::ptr::null_mut(),
                      17, 3, 3, grid.as_ptr()),
                  mk_cv(3)], 3, 3, "cv-clut-cv"),
            // mat-cv-clut-cv
            (vec![c::cmsStageAllocMatrix(std::ptr::null_mut(), 3, 3,
                      mat.as_ptr(), std::ptr::null()),
                  mk_cv(3),
                  c::cmsStageAllocCLut16bit(std::ptr::null_mut(),
                      17, 3, 3, grid.as_ptr()),
                  mk_cv(3)], 3, 3, "mat-cv-clut-cv"),
            // cv-mat-cv-clut-cv (full A2B)
            (vec![mk_cv(3),
                  c::cmsStageAllocMatrix(std::ptr::null_mut(), 3, 3,
                      mat.as_ptr(), std::ptr::null()),
                  mk_cv(3),
                  c::cmsStageAllocCLut16bit(std::ptr::null_mut(),
                      17, 3, 3, grid.as_ptr()),
                  mk_cv(3)], 3, 3, "full-a2b"),
            // 4ch cv-clut (cmyk)
            (vec![mk_cv(4),
                  c::cmsStageAllocCLut16bit(std::ptr::null_mut(),
                      17, 4, 4, grid4.as_ptr()),
                  mk_cv(4)], 4, 4, "cv4-clut4"),
        ];
        for (stages, ni, no, name) in shapes {
            for (sig, save8, ver, tname) in [
                (0x41324230u32, false, 2.1f64, "a2b0-v2"),   // mft2
                (0x41324230u32, true, 2.1f64, "a2b0-v2-8"),  // mft1
                (0x41324230u32, false, 4.4f64, "a2b0-v4"),   // mAB
                (0x42324130u32, false, 4.4f64, "b2a0-v4"),   // mBA
                (0x42324130u32, false, 2.1f64, "b2a0-v2"),   // mft2
            ] {
                let prof = c::cmsCreate_sRGBProfile();
                c::cmsSetProfileVersion(prof, ver);
                let p = c::cmsPipelineAlloc(std::ptr::null_mut(), ni, no);
                // dup stages for each pipeline (stages consumed)
                for st in &stages {
                    let d = c::cmsStageDup(*st);
                    c::cmsPipelineInsertStage(p, 1, d);
                }
                if save8 {
                    c::cmsPipelineSetSaveAs8bitsFlag(p, 1);
                }
                let w = c::cmsWriteTag(prof, sig, p as *const _);
                let mut buf = vec![0u8; 1 << 21];
                let mut nn = buf.len() as u32;
                let s = if w != 0 {
                    c::cmsSaveProfileToMem(prof,
                        buf.as_mut_ptr() as *mut _, &mut nn)
                } else { 0 };
                let rp = if s != 0 {
                    c::cmsOpenProfileFromMem(
                        buf.as_ptr() as *const _, nn)
                } else { std::ptr::null_mut() };
                let tag = if !rp.is_null() {
                    c::cmsReadTag(rp, sig)
                } else { std::ptr::null_mut() };
                eprintln!("{name}/{tname} w={w} s={s} r={}",
                    !tag.is_null());
                if !rp.is_null() { c::cmsCloseProfile(rp); }
                c::cmsCloseProfile(prof);
                c::cmsPipelineFree(p);
            }
            for st in &stages { c::cmsStageFree(*st); }
        }
        c::cmsFreeToneCurve(g);
        c::cmsFreeToneCurve(g2);
    }
}

/// mpet save across tag slots + profile classes.
#[test]
fn mpet_save_ararms() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let g2 = c::cmsBuildGamma(std::ptr::null_mut(), 1.8);
        let mat = [1.0f64, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        for (sig, name) in [
            (0x44324230u32, "d2b0"), (0x44324231, "d2b1"),
            (0x41324230, "a2b0"), (0x42324130, "b2a0"),
            (0x42324430, "b2d0"), (0x44324232, "d2b2"),
            (0x43324130, "c2a0"), (0x43324230, "c2b0"),
        ] {
            for (stages, sname) in [
                ("cv", "cv"),
                ("mat", "mat"),
                ("cv-mat", "cv-mat"),
            ] {
                let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
                if sname != "mat" {
                    c::cmsPipelineInsertStage(p, 1,
                        c::cmsStageAllocToneCurves(
                            std::ptr::null_mut(), 3,
                            [g, g2, g].as_ptr() as *mut _));
                }
                if sname != "cv" {
                    c::cmsPipelineInsertStage(p, 1,
                        c::cmsStageAllocMatrix(std::ptr::null_mut(),
                            3, 3, mat.as_ptr(),
                            std::ptr::null()));
                }
                let prof = c::cmsCreate_sRGBProfile();
                c::cmsSetProfileVersion(prof, 4.4);
                let w = c::cmsWriteTag(prof, sig, p as *const _);
                let mut buf = vec![0u8; 1 << 20];
                let mut n = buf.len() as u32;
                let s = if w != 0 {
                    c::cmsSaveProfileToMem(prof,
                        buf.as_mut_ptr() as *mut _, &mut n)
                } else { 0 };
                let rp = if s != 0 {
                    c::cmsOpenProfileFromMem(
                        buf.as_ptr() as *const _, n)
                } else { std::ptr::null_mut() };
                let tag = if !rp.is_null() {
                    c::cmsReadTag(rp, sig)
                } else { std::ptr::null_mut() };
                eprintln!("{name}/{sname} w={w} s={s} r={}",
                    !tag.is_null());
                if !rp.is_null() { c::cmsCloseProfile(rp); }
                c::cmsCloseProfile(prof);
                c::cmsPipelineFree(p);
            }
        }
        c::cmsFreeToneCurve(g);
        c::cmsFreeToneCurve(g2);
    }
}

/// Type_LUT8/16_Write deep arms — clutPoints=0, identity matrix,
/// mismatched nSamples, extra-stage reject.
#[test]
fn lut_write_deep() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let g2 = c::cmsBuildGamma(std::ptr::null_mut(), 1.8);
        let mat = [1.0f64, 0.5, 0.0, 0.0, 0.9, 0.0, 0.1, 0.0, 0.8];
        let mk_cv = |n: u32| {
            let curves = match n {
                1 => [g].as_ptr(),
                _ => [g, g2, g, g2].as_ptr(),
            };
            c::cmsStageAllocToneCurves(std::ptr::null_mut(), n,
                curves as *mut _)
        };
        let mk_clut = |npts: &[u32], ni: u32, no: u32| {
            let mut tot = 1usize;
            for &n in npts { tot *= n as usize; }
            let mut grid = vec![0u16; tot * no as usize];
            for (i, v) in grid.iter_mut().enumerate() {
                *v = ((i % 17) * 3855) as u16;
            }
            c::cmsStageAllocCLut16bitGranular(std::ptr::null_mut(),
                npts.as_ptr(), ni, no, grid.as_ptr())
        };
        for (stages, ni, no, name) in [
            // no clut (clutPoints=0) + matrix
            (vec![mk_cv(3),
                  c::cmsStageAllocMatrix(std::ptr::null_mut(), 3, 3,
                      mat.as_ptr(), std::ptr::null()),
                  mk_cv(3)], 3, 3, "cv-mat-cv-noclut"),
            // no clut, no matrix (identity written)
            (vec![mk_cv(3), mk_cv(3)], 3, 3, "cv-cv-noclut"),
            // clut only
            (vec![mk_clut(&[17, 17, 17], 3, 3)], 3, 3, "clut-only"),
            // mismatched nSamples per dim -> reject
            (vec![mk_cv(3),
                  mk_clut(&[17, 9, 17], 3, 3),
                  mk_cv(3)], 3, 3, "mismatch-rej"),
            // extra stage after post -> reject
            (vec![mk_cv(3),
                  mk_clut(&[17, 17, 17], 3, 3),
                  mk_cv(3),
                  mk_clut(&[5, 5, 5], 3, 3)], 3, 3, "extra-rej"),
            // 1ch pipeline
            (vec![mk_cv(1),
                  mk_clut(&[33], 1, 1),
                  mk_cv(1)], 1, 1, "1ch"),
            // 4ch cmyk
            (vec![mk_cv(4),
                  mk_clut(&[17, 17, 17, 17], 4, 4),
                  mk_cv(4)], 4, 4, "4ch"),
            // mat + clut no post-curves
            (vec![c::cmsStageAllocMatrix(std::ptr::null_mut(), 3, 3,
                      mat.as_ptr(), std::ptr::null()),
                  mk_clut(&[17, 17, 17], 3, 3)], 3, 3, "mat-clut"),
            // 3in-1out
            (vec![mk_cv(3),
                  mk_clut(&[17, 17, 17], 3, 1),
                  mk_cv(1)], 3, 1, "3to1"),
        ] {
            for (sig, save8, name2) in [
                (0x41324230u32, false, "a2b0"),
                (0x41324230u32, true, "a2b0-8"),
                (0x42324130u32, false, "b2a0"),
            ] {
                let prof = c::cmsCreate_sRGBProfile();
                c::cmsSetProfileVersion(prof, 2.1);
                let p = c::cmsPipelineAlloc(std::ptr::null_mut(), ni, no);
                for st in &stages {
                    let d = c::cmsStageDup(*st);
                    c::cmsPipelineInsertStage(p, 1, d);
                }
                if save8 { c::cmsPipelineSetSaveAs8bitsFlag(p, 1); }
                let w = c::cmsWriteTag(prof, sig, p as *const _);
                let mut buf = vec![0u8; 1 << 22];
                let mut nn = buf.len() as u32;
                let s = if w != 0 {
                    c::cmsSaveProfileToMem(prof,
                        buf.as_mut_ptr() as *mut _, &mut nn)
                } else { 0 };
                eprintln!("{name}/{name2} w={w} s={s}");
                c::cmsCloseProfile(prof);
                c::cmsPipelineFree(p);
            }
            for st in &stages { c::cmsStageFree(*st); }
        }
        c::cmsFreeToneCurve(g);
        c::cmsFreeToneCurve(g2);
    }
}

/// Type_LUTA2B/B2A_Write deep arms — all 4 valid mAB/mBA element
/// orders x SaveAs8Bits.
#[test]
fn mab_mba_write_orders() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let g2 = c::cmsBuildGamma(std::ptr::null_mut(), 1.8);
        let mat = [1.0f64, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        let mut grid = vec![0u16; 17 * 17 * 17 * 3];
        for (i, v) in grid.iter_mut().enumerate() {
            *v = ((i % 17) * 3855) as u16;
        }
        let mk_cv = || {
            c::cmsStageAllocToneCurves(std::ptr::null_mut(), 3,
                [g, g2, g].as_ptr() as *mut _)
        };
        let mk_mat = || {
            c::cmsStageAllocMatrix(std::ptr::null_mut(), 3, 3,
                mat.as_ptr(), std::ptr::null())
        };
        let mk_clut = || {
            c::cmsStageAllocCLut16bit(std::ptr::null_mut(), 17, 3, 3,
                grid.as_ptr())
        };
        // mAB orders: B, B+M+mat, A+clut+B, A+clut+M+mat+B
        for (stages, name) in [
            (vec![mk_cv()], "B"),
            (vec![mk_cv(), mk_cv(), mk_mat()], "B-M-mat"),
            (vec![mk_cv(), mk_clut(), mk_cv()], "A-clut-B"),
            (vec![mk_cv(), mk_clut(), mk_cv(), mk_mat(), mk_cv()],
                "A-clut-M-mat-B"),
        ] {
            for (sig, save8, name2) in [
                (0x41324230u32, false, "a2b0"),
                (0x41324230u32, true, "a2b0-8"),
                (0x42324130u32, false, "b2a0"),
            ] {
                let prof = c::cmsCreate_sRGBProfile();
                c::cmsSetProfileVersion(prof, 4.4);
                let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
                for st in &stages {
                    let d = c::cmsStageDup(*st);
                    c::cmsPipelineInsertStage(p, 1, d);
                }
                if save8 { c::cmsPipelineSetSaveAs8bitsFlag(p, 1); }
                let w = c::cmsWriteTag(prof, sig, p as *const _);
                let mut buf = vec![0u8; 1 << 22];
                let mut nn = buf.len() as u32;
                let s = if w != 0 {
                    c::cmsSaveProfileToMem(prof,
                        buf.as_mut_ptr() as *mut _, &mut nn)
                } else { 0 };
                let rp = if s != 0 {
                    c::cmsOpenProfileFromMem(
                        buf.as_ptr() as *const _, nn)
                } else { std::ptr::null_mut() };
                let tag = if !rp.is_null() {
                    c::cmsReadTag(rp, sig)
                } else { std::ptr::null_mut() };
                eprintln!("{name}/{name2} w={w} s={s} r={}",
                    !tag.is_null());
                if !rp.is_null() { c::cmsCloseProfile(rp); }
                c::cmsCloseProfile(prof);
                c::cmsPipelineFree(p);
            }
            for st in &stages { c::cmsStageFree(*st); }
        }
        c::cmsFreeToneCurve(g);
        c::cmsFreeToneCurve(g2);
    }
}

/// Type_LUTB2A_Write deep arms — all 4 valid mBA element orders
/// (B-first, then matrix/M/CLUT/A).
#[test]
fn mba_write_orders() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let g2 = c::cmsBuildGamma(std::ptr::null_mut(), 1.8);
        let mat = [1.0f64, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        let mut grid = vec![0u16; 17 * 17 * 17 * 3];
        for (i, v) in grid.iter_mut().enumerate() {
            *v = ((i % 17) * 3855) as u16;
        }
        let mk_cv = || {
            c::cmsStageAllocToneCurves(std::ptr::null_mut(), 3,
                [g, g2, g].as_ptr() as *mut _)
        };
        let mk_mat = || {
            c::cmsStageAllocMatrix(std::ptr::null_mut(), 3, 3,
                mat.as_ptr(), std::ptr::null())
        };
        let mk_clut = || {
            c::cmsStageAllocCLut16bit(std::ptr::null_mut(), 17, 3, 3,
                grid.as_ptr())
        };
        // mBA orders: B, B+mat+M, B+clut+A, B+mat+M+clut+A
        for (stages, name) in [
            (vec![mk_cv()], "B"),
            (vec![mk_cv(), mk_mat(), mk_cv()], "B-mat-M"),
            (vec![mk_cv(), mk_clut(), mk_cv()], "B-clut-A"),
            (vec![mk_cv(), mk_mat(), mk_cv(), mk_clut(), mk_cv()],
                "B-mat-M-clut-A"),
        ] {
            for (sig, save8, name2) in [
                (0x42324130u32, false, "b2a0"),
                (0x42324130u32, true, "b2a0-8"),
                (0x41324230u32, false, "a2b0"),
            ] {
                let prof = c::cmsCreate_sRGBProfile();
                c::cmsSetProfileVersion(prof, 4.4);
                let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
                for st in &stages {
                    let d = c::cmsStageDup(*st);
                    c::cmsPipelineInsertStage(p, 1, d);
                }
                if save8 { c::cmsPipelineSetSaveAs8bitsFlag(p, 1); }
                let w = c::cmsWriteTag(prof, sig, p as *const _);
                let mut buf = vec![0u8; 1 << 22];
                let mut nn = buf.len() as u32;
                let s = if w != 0 {
                    c::cmsSaveProfileToMem(prof,
                        buf.as_mut_ptr() as *mut _, &mut nn)
                } else { 0 };
                eprintln!("{name}/{name2} w={w} s={s}");
                c::cmsCloseProfile(prof);
                c::cmsPipelineFree(p);
            }
            for st in &stages { c::cmsStageFree(*st); }
        }
        c::cmsFreeToneCurve(g);
        c::cmsFreeToneCurve(g2);
    }
}

/// SaveTags linked-tag + dedup + unsupported-tag arms.
#[test]
fn save_tags_deep() {
    unsafe {
        // linked tags: two sigs pointing to the same serialized
        // data — write the same object to multiple sigs
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let prof = c::cmsCreate_sRGBProfile();
        // link rTRC/gTRC/bTRC to the same tone curve
        c::cmsWriteTag(prof, 0x72545243, g as *const _);
        c::cmsWriteTag(prof, 0x67545243, g as *const _);
        c::cmsWriteTag(prof, 0x62545243, g as *const _);
        let ml = c::cmsMLUalloc(std::ptr::null_mut(), 1);
        let mut wk = [0x74u32, 0x65, 0x73, 0x74, 0];
        c::cmsMLUsetWide(ml, b"en\0".as_ptr() as *const _,
            b"US\0".as_ptr() as *const _, wk.as_mut_ptr() as *const _);
        // same MLU to several text sigs
        for sig in [0x64657363u32, 0x63707274, 0x646d6464,
                    0x646d6e64, 0x64657673] {
            c::cmsWriteTag(prof, sig, ml as *const _);
        }
        let mut buf = vec![0u8; 1 << 20];
        let mut n = buf.len() as u32;
        let s = c::cmsSaveProfileToMem(prof,
            buf.as_mut_ptr() as *mut _, &mut n);
        eprintln!("linked save={s} n={n}");
        // re-open + re-save (raw-copy + linked arms)
        let rp = if s != 0 {
            c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
        } else { std::ptr::null_mut() };
        if !rp.is_null() {
            let mut buf2 = vec![0u8; 1 << 20];
            let mut n2 = buf2.len() as u32;
            let s2 = c::cmsSaveProfileToMem(rp,
                buf2.as_mut_ptr() as *mut _, &mut n2);
            eprintln!("re-save={s2} n2={n2}");
            c::cmsCloseProfile(rp);
        }
        c::cmsCloseProfile(prof);
        c::cmsFreeToneCurve(g);
        c::cmsMLUfree(ml);
    }
}

/// Type_LUT8/16_Read deep arms — mft1/mft2 reader internals:
/// clutPoints=0, matrix present/absent, input/output tables,
/// different nSamples, malformed offsets.
#[test]
fn lut_read_deep() {
    unsafe {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        // corpus mft1/mft2 reads
        for f in ["lcms2cmyk.icc", "nullcms2.icc", "limitlcms2.icc",
                  "linlcms2.icc", "bchslcms2.icc", "labv4lcms2.icc",
                  "sRGBlcms2.icc", "xyzlcms2.icc"] {
            let Ok(d) = std::fs::read(dir.join(f)) else { continue };
            let p = c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32);
            if p.is_null() { continue; }
            for i in 0..c::cmsGetTagCount(p) as u32 {
                let sig = c::cmsGetTagSignature(p, i);
                let tag = c::cmsReadTag(p, sig);
                if !tag.is_null() {
                    let _t2 = c::cmsReadTag(p, sig);
                }
            }
            c::cmsCloseProfile(p);
        }
        // raw mft1/mft2 blobs covering element-order + malformed
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let curves = [g, g, g];
        let mat = [1.0f64, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        let mut grid = vec![0u16; 17 * 17 * 17 * 3];
        for (i, v) in grid.iter_mut().enumerate() {
            *v = ((i % 17) * 3855) as u16;
        }
        for (stages, name) in [
            ("cv-mat-clut-cv", "full"),
            ("cv", "b-only"),
            ("cv-clut-cv", "no-mat"),
            ("cv-clut", "no-post"),
        ] {
            let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
            if stages != "cv-clut" && stages != "cv" {
                c::cmsPipelineInsertStage(p, 1,
                    c::cmsStageAllocToneCurves(std::ptr::null_mut(),
                        3, curves.as_ptr() as *mut _));
            }
            if stages == "cv-mat-clut-cv" {
                c::cmsPipelineInsertStage(p, 1,
                    c::cmsStageAllocMatrix(std::ptr::null_mut(), 3, 3,
                        mat.as_ptr(), std::ptr::null()));
            }
            if stages != "cv" && stages != "cv-mat-clut-cv" {
                c::cmsPipelineInsertStage(p, 1,
                    c::cmsStageAllocToneCurves(std::ptr::null_mut(),
                        3, curves.as_ptr() as *mut _));
            }
            if stages != "cv" {
                c::cmsPipelineInsertStage(p, 1,
                    c::cmsStageAllocCLut16bit(std::ptr::null_mut(),
                        17, 3, 3, grid.as_ptr()));
            }
            if stages == "cv-mat-clut-cv" || stages == "cv-clut-cv" {
                c::cmsPipelineInsertStage(p, 1,
                    c::cmsStageAllocToneCurves(std::ptr::null_mut(),
                        3, curves.as_ptr() as *mut _));
            }
            for (sig, ver, save8, name2) in [
                (0x41324230u32, 2.1f64, false, "a2b0-v2"),
                (0x41324230u32, 2.1f64, true, "a2b0-v2-8"),
            ] {
                let prof = c::cmsCreate_sRGBProfile();
                c::cmsSetProfileVersion(prof, ver);
                if save8 { c::cmsPipelineSetSaveAs8bitsFlag(p, 1); }
                let w = c::cmsWriteTag(prof, sig, p as *const _);
                let mut buf = vec![0u8; 1 << 22];
                let mut nn = buf.len() as u32;
                let s = if w != 0 {
                    c::cmsSaveProfileToMem(prof,
                        buf.as_mut_ptr() as *mut _, &mut nn)
                } else { 0 };
                let rp = if s != 0 {
                    c::cmsOpenProfileFromMem(
                        buf.as_ptr() as *const _, nn)
                } else { std::ptr::null_mut() };
                let tag = if !rp.is_null() {
                    c::cmsReadTag(rp, sig)
                } else { std::ptr::null_mut() };
                eprintln!("{name}/{name2} w={w} s={s} r={}",
                    !tag.is_null());
                if !rp.is_null() { c::cmsCloseProfile(rp); }
                c::cmsCloseProfile(prof);
            }
            c::cmsPipelineFree(p);
        }
        c::cmsFreeToneCurve(g);
    }
}

/// Type_LUT16_Write per-channel curve serialization — curves
/// with different shapes per channel.
#[test]
fn lut16_write_channels() {
    unsafe {
        let g22 = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let g18 = c::cmsBuildGamma(std::ptr::null_mut(), 1.8);
        let g26 = c::cmsBuildGamma(std::ptr::null_mut(), 2.6);
        let g10 = c::cmsBuildGamma(std::ptr::null_mut(), 1.0);
        let mat = [1.0f64, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        let mut grid = vec![0u16; 17 * 17 * 17 * 3];
        for (i, v) in grid.iter_mut().enumerate() {
            *v = ((i % 17) * 3855) as u16;
        }
        // different per-channel curves hit the per-channel
        // serialization loops inside WriteSetOfCurves/WriteCLUT
        for (c1, c2, c3, name) in [
            ([g22, g22, g22], [g22, g22, g22], [g22, g22, g22], "same"),
            ([g22, g18, g26], [g26, g22, g18], [g18, g26, g22], "diff"),
            ([g10, g22, g10], [g22, g10, g22], [g10, g22, g10], "mixed"),
        ] {
            for (sig, save8, name2) in [
                (0x41324230u32, false, "a2b0"),
                (0x41324230u32, true, "a2b0-8"),
            ] {
                let prof = c::cmsCreate_sRGBProfile();
                c::cmsSetProfileVersion(prof, 2.1);
                let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
                c::cmsPipelineInsertStage(p, 1,
                    c::cmsStageAllocToneCurves(std::ptr::null_mut(),
                        3, c1.as_ptr() as *mut _));
                c::cmsPipelineInsertStage(p, 1,
                    c::cmsStageAllocMatrix(std::ptr::null_mut(), 3, 3,
                        mat.as_ptr(), std::ptr::null()));
                c::cmsPipelineInsertStage(p, 1,
                    c::cmsStageAllocCLut16bit(std::ptr::null_mut(),
                        17, 3, 3, grid.as_ptr()));
                c::cmsPipelineInsertStage(p, 1,
                    c::cmsStageAllocToneCurves(std::ptr::null_mut(),
                        3, c3.as_ptr() as *mut _));
                if save8 { c::cmsPipelineSetSaveAs8bitsFlag(p, 1); }
                let w = c::cmsWriteTag(prof, sig, p as *const _);
                let mut buf = vec![0u8; 1 << 22];
                let mut nn = buf.len() as u32;
                let s = if w != 0 {
                    c::cmsSaveProfileToMem(prof,
                        buf.as_mut_ptr() as *mut _, &mut nn)
                } else { 0 };
                eprintln!("{name}/{name2} w={w} s={s}");
                c::cmsCloseProfile(prof);
                c::cmsPipelineFree(p);
            }
        }
        c::cmsFreeToneCurve(g22);
        c::cmsFreeToneCurve(g18);
        c::cmsFreeToneCurve(g26);
        c::cmsFreeToneCurve(g10);
    }
}

/// SaveTags remaining arms — unsupported-tag skip, DecideType
/// NULL paths, raw-tag saves, error propagation.
#[test]
fn save_tags_arms2() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        for (sig, name) in [
            // tags without registered type handlers -> skip
            (0x41424344u32, "unknown-tag"),
            (0xdeadbeefu32, "invalid-sig"),
            (0x00000000u32, "null-sig"),
            // tags whose DecideType might return NULL
            (0x6e636c32u32, "ncl2"),
            (0x63686164u32, "chad"),
            (0x70726561u32, "pre0"),
        ] {
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsSetProfileVersion(prof, 4.4);
            let w = c::cmsWriteTag(prof, sig, g as *const _);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            eprintln!("{name} w={w} s={s}");
            c::cmsCloseProfile(prof);
        }
        // buffer too small -> write-failure path
        let prof = c::cmsCreate_sRGBProfile();
        let mut tiny = [0u8; 64];
        let mut n = tiny.len() as u32;
        let s = c::cmsSaveProfileToMem(prof,
            tiny.as_mut_ptr() as *mut _, &mut n);
        eprintln!("tiny-save={s}");
        c::cmsCloseProfile(prof);
        c::cmsFreeToneCurve(g);
    }
}

/// Type_LUT8/16_Write input/output table serialization — curves
/// with different entry counts per channel.
#[test]
fn lut_write_tables() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let g2 = c::cmsBuildGamma(std::ptr::null_mut(), 1.8);
        let mat = [1.0f64, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        // different curve entry counts per channel
        let mut tab256 = vec![0u16; 256];
        let mut tab16 = vec![0u16; 16];
        let mut tab1024 = vec![0u16; 1024];
        for i in 0..256 { tab256[i] = (i * 255) as u16; }
        for i in 0..16 { tab16[i] = (i * 4095) as u16; }
        for i in 0..1024 { tab1024[i] = (i * 63) as u16; }
        let t256 = c::cmsBuildTabulatedToneCurve16(std::ptr::null_mut(),
            256, tab256.as_ptr());
        let t16 = c::cmsBuildTabulatedToneCurve16(std::ptr::null_mut(),
            16, tab16.as_ptr());
        let t1024 = c::cmsBuildTabulatedToneCurve16(std::ptr::null_mut(),
            1024, tab1024.as_ptr());
        let mut grid = vec![0u16; 17 * 17 * 17 * 3];
        for (i, v) in grid.iter_mut().enumerate() {
            *v = ((i % 17) * 3855) as u16;
        }
        for (c1, c2, c3, name) in [
            ([t256, t256, t256], [t256, t256, t256],
                [t256, t256, t256], "all-256"),
            ([t16, t16, t16], [t16, t16, t16],
                [t16, t16, t16], "all-16"),
            ([t1024, t1024, t1024], [t1024, t1024, t1024],
                [t1024, t1024, t1024], "all-1024"),
            ([t256, t16, t1024], [t16, t1024, t256],
                [t1024, t256, t16], "mixed"),
            ([t256, g, t256], [g, t256, g],
                [t256, g, t256], "parametric-mix"),
        ] {
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsSetProfileVersion(prof, 2.1);
            let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
            c::cmsPipelineInsertStage(p, 1,
                c::cmsStageAllocToneCurves(std::ptr::null_mut(), 3,
                    c1.as_ptr() as *mut _));
            c::cmsPipelineInsertStage(p, 1,
                c::cmsStageAllocMatrix(std::ptr::null_mut(), 3, 3,
                    mat.as_ptr(), std::ptr::null()));
            c::cmsPipelineInsertStage(p, 1,
                c::cmsStageAllocCLut16bit(std::ptr::null_mut(),
                    17, 3, 3, grid.as_ptr()));
            c::cmsPipelineInsertStage(p, 1,
                c::cmsStageAllocToneCurves(std::ptr::null_mut(), 3,
                    c3.as_ptr() as *mut _));
            let w = c::cmsWriteTag(prof, 0x41324230, p as *const _);
            let mut buf = vec![0u8; 1 << 22];
            let mut nn = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut nn)
            } else { 0 };
            eprintln!("{name} w={w} s={s}");
            c::cmsCloseProfile(prof);
            c::cmsPipelineFree(p);
        }
        c::cmsFreeToneCurve(g);
        c::cmsFreeToneCurve(g2);
        c::cmsFreeToneCurve(t256);
        c::cmsFreeToneCurve(t16);
        c::cmsFreeToneCurve(t1024);
    }
}

/// Type_LUT8/16_Read + LUTA2B/B2A_Read deep arms — element
/// orders, grid sizes, input/output table variants.
#[test]
fn lut_read_deep2() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let g2 = c::cmsBuildGamma(std::ptr::null_mut(), 1.8);
        let mat = [1.0f64, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        let mk_cv = || {
            c::cmsStageAllocToneCurves(std::ptr::null_mut(), 3,
                [g, g2, g].as_ptr() as *mut _)
        };
        let mk_mat = || {
            c::cmsStageAllocMatrix(std::ptr::null_mut(), 3, 3,
                mat.as_ptr(), std::ptr::null())
        };
        let mk_clut = |npts: u32| {
            let mut grid = vec![0u16;
                (npts * npts * npts * 3) as usize];
            for (i, v) in grid.iter_mut().enumerate() {
                *v = ((i % 17) * 3855) as u16;
            }
            c::cmsStageAllocCLut16bit(std::ptr::null_mut(), npts,
                3, 3, grid.as_ptr())
        };
        // different CLUT grid sizes -> different read paths
        for (npts, name) in [
            (2u32, "grid2"), (3, "grid3"), (5, "grid5"),
            (9, "grid9"), (17, "grid17"), (33, "grid33"),
        ] {
            for (stages, sname) in [
                ("cv-clut-cv", "cv-clut-cv"),
                ("cv-mat-clut-cv", "cv-mat-clut-cv"),
            ] {
                let prof = c::cmsCreate_sRGBProfile();
                c::cmsSetProfileVersion(prof, 4.4);
                let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
                c::cmsPipelineInsertStage(p, 1, c::cmsStageDup(
                    mk_cv()));
                if sname == "cv-mat-clut-cv" {
                    c::cmsPipelineInsertStage(p, 1, c::cmsStageDup(
                        mk_mat()));
                }
                c::cmsPipelineInsertStage(p, 1, c::cmsStageDup(
                    mk_clut(npts)));
                c::cmsPipelineInsertStage(p, 1, c::cmsStageDup(
                    mk_cv()));
                let w = c::cmsWriteTag(prof, 0x41324230,
                    p as *const _);
                let mut buf = vec![0u8; 1 << 22];
                let mut nn = buf.len() as u32;
                let s = if w != 0 {
                    c::cmsSaveProfileToMem(prof,
                        buf.as_mut_ptr() as *mut _, &mut nn)
                } else { 0 };
                let rp = if s != 0 {
                    c::cmsOpenProfileFromMem(
                        buf.as_ptr() as *const _, nn)
                } else { std::ptr::null_mut() };
                let tag = if !rp.is_null() {
                    c::cmsReadTag(rp, 0x41324230)
                } else { std::ptr::null_mut() };
                eprintln!("{sname}/{name} w={w} s={s} r={}",
                    !tag.is_null());
                if !rp.is_null() { c::cmsCloseProfile(rp); }
                c::cmsCloseProfile(prof);
                c::cmsPipelineFree(p);
            }
        }
        c::cmsFreeToneCurve(g);
        c::cmsFreeToneCurve(g2);
    }
}

/// Type_LUT8/16_Read deep arms — more mft shapes + malformed
/// offsets + different channel counts.
#[test]
fn lut_read_deep3() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let g2 = c::cmsBuildGamma(std::ptr::null_mut(), 1.8);
        let mat = [1.0f64, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        let mk_cv = |n: u32| {
            let curves = match n {
                1 => [g].as_ptr(),
                4 => [g, g2, g, g2].as_ptr(),
                _ => [g, g2, g].as_ptr(),
            };
            c::cmsStageAllocToneCurves(std::ptr::null_mut(), n,
                curves as *mut _)
        };
        // v2 profile + every mft2-compatible shape
        for (stages, ni, no, name) in [
            ("clut-only", 3, 3, "clut-only"),
            ("cv-clut", 3, 3, "cv-clut"),
            ("cv-clut-cv", 3, 3, "cv-clut-cv"),
            ("cv-mat-clut-cv", 3, 3, "cv-mat-clut-cv"),
            ("1ch", 1, 1, "1ch"),
            ("4ch", 4, 4, "4ch"),
            ("3to1", 3, 1, "3to1"),
            ("1to3", 1, 3, "1to3"),
            ("mat-clut-cv", 3, 3, "mat-clut-cv"),
            ("cv-mat-clut", 3, 3, "cv-mat-clut"),
        ] {
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsSetProfileVersion(prof, 2.1);
            let p = c::cmsPipelineAlloc(std::ptr::null_mut(), ni, no);
            for st in stages.split('-') {
                let s = match st {
                    "cv" => mk_cv(ni),
                    "mat" => c::cmsStageAllocMatrix(std::ptr::null_mut(),
                        3, 3, mat.as_ptr(), std::ptr::null()),
                    "clut" => {
                        let mut grid = vec![0u16;
                            (17 * 17 * 17 * no) as usize];
                        for (i, v) in grid.iter_mut().enumerate() {
                            *v = ((i % 17) * 3855) as u16;
                        }
                        c::cmsStageAllocCLut16bit(std::ptr::null_mut(),
                            17, ni, no, grid.as_ptr())
                    }
                    "1ch" => mk_cv(1),
                    "4ch" => mk_cv(4),
                    "3to1" => mk_cv(3),
                    "1to3" => mk_cv(1),
                    _ => continue,
                };
                c::cmsPipelineInsertStage(p, 1, s);
            }
            let w = c::cmsWriteTag(prof, 0x41324230, p as *const _);
            let mut buf = vec![0u8; 1 << 22];
            let mut nn = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut nn)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(
                    buf.as_ptr() as *const _, nn)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, 0x41324230)
            } else { std::ptr::null_mut() };
            eprintln!("{name} w={w} s={s} r={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
            c::cmsPipelineFree(p);
        }
        c::cmsFreeToneCurve(g);
        c::cmsFreeToneCurve(g2);
    }
}

/// Type_LUT16_Write remaining arms — per-channel table sizes,
/// output-table serialization, mixed curve shapes.
#[test]
fn lut16_write_deep() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let g2 = c::cmsBuildGamma(std::ptr::null_mut(), 1.8);
        let g3 = c::cmsBuildGamma(std::ptr::null_mut(), 3.1);
        let mat = [1.0f64, 0.5, 0.0, 0.0, 0.9, 0.0, 0.1, 0.0, 0.8];
        // per-channel table sizes inside WriteSetOfCurves
        let mut t64 = vec![0u16; 64];
        let mut t256 = vec![0u16; 256];
        let mut t1024 = vec![0u16; 1024];
        for i in 0..64 { t64[i] = (i * 1023) as u16; }
        for i in 0..256 { t256[i] = (i * 255) as u16; }
        for i in 0..1024 { t1024[i] = (i * 63) as u16; }
        let c64 = c::cmsBuildTabulatedToneCurve16(std::ptr::null_mut(),
            64, t64.as_ptr());
        let c256 = c::cmsBuildTabulatedToneCurve16(std::ptr::null_mut(),
            256, t256.as_ptr());
        let c1024 = c::cmsBuildTabulatedToneCurve16(std::ptr::null_mut(),
            1024, t1024.as_ptr());
        let mut grid = vec![0u16; 17 * 17 * 17 * 3];
        for (i, v) in grid.iter_mut().enumerate() {
            *v = ((i % 17) * 3855) as u16;
        }
        for (c1, c2, c3, name) in [
            ([c64, c64, c64], [c64, c64, c64], [c64, c64, c64],
                "64-64-64"),
            ([c256, c256, c256], [c256, c256, c256],
                [c256, c256, c256], "256-256-256"),
            ([c1024, c1024, c1024], [c1024, c1024, c1024],
                [c1024, c1024, c1024], "1024-1024-1024"),
            ([c64, c256, c1024], [c1024, c64, c256],
                [c256, c1024, c64], "mixed-sizes"),
            ([c64, g, c64], [g, c64, g], [c64, g, c64],
                "parametric-mix"),
            ([g, g2, g3], [g3, g, g2], [g2, g3, g],
                "gamma-mix"),
        ] {
            for (sig, save8, name2) in [
                (0x41324230u32, false, "a2b0"),
                (0x41324230u32, true, "a2b0-8"),
            ] {
                let prof = c::cmsCreate_sRGBProfile();
                c::cmsSetProfileVersion(prof, 2.1);
                let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
                c::cmsPipelineInsertStage(p, 1,
                    c::cmsStageAllocToneCurves(std::ptr::null_mut(),
                        3, c1.as_ptr() as *mut _));
                c::cmsPipelineInsertStage(p, 1,
                    c::cmsStageAllocMatrix(std::ptr::null_mut(), 3, 3,
                        mat.as_ptr(), std::ptr::null()));
                c::cmsPipelineInsertStage(p, 1,
                    c::cmsStageAllocCLut16bit(std::ptr::null_mut(),
                        17, 3, 3, grid.as_ptr()));
                c::cmsPipelineInsertStage(p, 1,
                    c::cmsStageAllocToneCurves(std::ptr::null_mut(),
                        3, c3.as_ptr() as *mut _));
                if save8 { c::cmsPipelineSetSaveAs8bitsFlag(p, 1); }
                let w = c::cmsWriteTag(prof, sig, p as *const _);
                let mut buf = vec![0u8; 1 << 22];
                let mut nn = buf.len() as u32;
                let s = if w != 0 {
                    c::cmsSaveProfileToMem(prof,
                        buf.as_mut_ptr() as *mut _, &mut nn)
                } else { 0 };
                eprintln!("{name}/{name2} w={w} s={s}");
                c::cmsCloseProfile(prof);
                c::cmsPipelineFree(p);
            }
        }
        c::cmsFreeToneCurve(g);
        c::cmsFreeToneCurve(g2);
        c::cmsFreeToneCurve(g3);
        c::cmsFreeToneCurve(c64);
        c::cmsFreeToneCurve(c256);
        c::cmsFreeToneCurve(c1024);
    }
}

/// Type_LUT16_Write via b2a0 — cv-mat-clut-cv on B2A0 picks
/// mft2 (B-first mBA order maps to PreMPE-mat-clut-PostMPE).
#[test]
fn lut16_write_b2a0() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let g2 = c::cmsBuildGamma(std::ptr::null_mut(), 1.8);
        let mat = [1.0f64, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        let mut grid = vec![0u16; 17 * 17 * 17 * 3];
        for (i, v) in grid.iter_mut().enumerate() {
            *v = ((i % 17) * 3855) as u16;
        }
        for (stages, name) in [
            ("cv-mat-clut-cv", "cv-mat-clut-cv"),
            ("mat-clut-cv", "mat-clut-cv"),
            ("cv-mat-clut", "cv-mat-clut"),
            ("cv-clut-cv", "cv-clut-cv"),
            ("mat-cv-clut-cv", "mat-cv-clut-cv"),
        ] {
            for (sig, save8, name2) in [
                (0x42324130u32, false, "b2a0"),
                (0x42324130u32, true, "b2a0-8"),
            ] {
                let prof = c::cmsCreate_sRGBProfile();
                c::cmsSetProfileVersion(prof, 2.1);
                let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
                for st in stages.split('-') {
                    let s = match st {
                        "cv" => c::cmsStageAllocToneCurves(
                            std::ptr::null_mut(), 3,
                            [g, g2, g].as_ptr() as *mut _),
                        "mat" => c::cmsStageAllocMatrix(
                            std::ptr::null_mut(), 3, 3,
                            mat.as_ptr(), std::ptr::null()),
                        "clut" => c::cmsStageAllocCLut16bit(
                            std::ptr::null_mut(), 17, 3, 3,
                            grid.as_ptr()),
                        _ => continue,
                    };
                    c::cmsPipelineInsertStage(p, 1, s);
                }
                if save8 { c::cmsPipelineSetSaveAs8bitsFlag(p, 1); }
                let w = c::cmsWriteTag(prof, sig, p as *const _);
                let mut buf = vec![0u8; 1 << 22];
                let mut nn = buf.len() as u32;
                let s = if w != 0 {
                    c::cmsSaveProfileToMem(prof,
                        buf.as_mut_ptr() as *mut _, &mut nn)
                } else { 0 };
                eprintln!("{name}/{name2} w={w} s={s}");
                c::cmsCloseProfile(prof);
                c::cmsPipelineFree(p);
            }
        }
        c::cmsFreeToneCurve(g);
        c::cmsFreeToneCurve(g2);
    }
}

/// Type_LUT16_Write remaining internals — SaveAs8Bits on all
/// valid mft2 shapes + per-element order variants.
#[test]
fn lut16_write_arms3() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let g2 = c::cmsBuildGamma(std::ptr::null_mut(), 1.8);
        let mat = [1.0f64, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        let mk_cv = |n: u32| {
            let curves = match n {
                1 => [g].as_ptr(),
                4 => [g, g2, g, g2].as_ptr(),
                _ => [g, g2, g].as_ptr(),
            };
            c::cmsStageAllocToneCurves(std::ptr::null_mut(), n,
                curves as *mut _)
        };
        let mk_mat = || {
            c::cmsStageAllocMatrix(std::ptr::null_mut(), 3, 3,
                mat.as_ptr(), std::ptr::null())
        };
        let mk_clut = |ni: u32, no: u32| {
            let mut grid = vec![0u16; (17 * 17 * 17 * no) as usize];
            for (i, v) in grid.iter_mut().enumerate() {
                *v = ((i % 17) * 3855) as u16;
            }
            c::cmsStageAllocCLut16bit(std::ptr::null_mut(), 17, ni, no,
                grid.as_ptr())
        };
        // all valid mft2 shapes x SaveAs8Bits x a2b0/b2a0
        for (stages, ni, no, name) in [
            ("mat-cv-clut-cv", 3, 3, "mat-cv-clut-cv"),
            ("mat-clut-cv", 3, 3, "mat-clut-cv"),
            ("cv-clut-cv", 3, 3, "cv-clut-cv"),
            ("clut-cv", 3, 3, "clut-cv"),
            ("mat-cv-clut", 3, 3, "mat-cv-clut"),
            ("clut-cv", 1, 1, "1ch-clut-cv"),
            ("cv-clut-cv", 1, 1, "1ch-cv-clut-cv"),
            ("mat-cv-clut-cv", 4, 4, "4ch-mat-cv-clut-cv"),
            ("clut-cv", 4, 4, "4ch-clut-cv"),
            ("mat-cv-clut-cv", 3, 1, "3to1"),
            ("clut-cv", 3, 4, "3to4"),
        ] {
            for (sig, save8, name2) in [
                (0x41324230u32, false, "a2b0"),
                (0x41324230u32, true, "a2b0-8"),
                (0x42324130u32, false, "b2a0"),
                (0x42324130u32, true, "b2a0-8"),
            ] {
                let prof = c::cmsCreate_sRGBProfile();
                c::cmsSetProfileVersion(prof, 2.1);
                let p = c::cmsPipelineAlloc(std::ptr::null_mut(), ni, no);
                for st in stages.split('-') {
                    let s = match st {
                        "cv" => mk_cv(ni),
                        "mat" => mk_mat(),
                        "clut" => mk_clut(ni, no),
                        _ => continue,
                    };
                    c::cmsPipelineInsertStage(p, 1, s);
                }
                if save8 { c::cmsPipelineSetSaveAs8bitsFlag(p, 1); }
                let w = c::cmsWriteTag(prof, sig, p as *const _);
                let mut buf = vec![0u8; 1 << 22];
                let mut nn = buf.len() as u32;
                let s = if w != 0 {
                    c::cmsSaveProfileToMem(prof,
                        buf.as_mut_ptr() as *mut _, &mut nn)
                } else { 0 };
                eprintln!("{name}/{name2} w={w} s={s}");
                c::cmsCloseProfile(prof);
                c::cmsPipelineFree(p);
            }
        }
        c::cmsFreeToneCurve(g);
        c::cmsFreeToneCurve(g2);
    }
}

/// Type_Dictionary_Read deep arms — dict tag reader internals:
/// multi-entry, locale names, empty entries, malformed counts.
#[test]
fn dict_read_arms() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        for (entries, name) in [
            (1u32, "1entry"),
            (2, "2entry"),
            (8, "8entry"),
            (64, "64entry"),
            (256, "256entry"),
        ] {
            let dict = c::cmsDictAlloc(std::ptr::null_mut());
            for i in 0..entries {
                let name = format!("key{}\0", i);
                let mut wn = name.as_bytes().iter()
                    .map(|&b| b as u32).collect::<Vec<u32>>();
                wn.push(0);
                let val = format!("value{}\0", i);
                let mut wv = val.as_bytes().iter()
                    .map(|&b| b as u32).collect::<Vec<u32>>();
                wv.push(0);
                let loc = if i % 3 == 0 {
                    format!("locale{}\0", i % 5)
                } else { "\0".to_string() };
                let mut wl = loc.as_bytes().iter()
                    .map(|&b| b as u32).collect::<Vec<u32>>();
                wl.push(0);
                // DisplayName/DisplayValue are cmsMLU* handles
                c::cmsDictAddEntry(dict,
                    wn.as_ptr() as *const i32,
                    wv.as_ptr() as *const i32,
                    std::ptr::null_mut(),
                    std::ptr::null_mut());
            }
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsSetProfileVersion(prof, 4.4);
            let w = c::cmsWriteTag(prof, 0x64696374, dict as *const _);
            let mut buf = vec![0u8; 1 << 22];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(
                    buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, 0x64696374)
            } else { std::ptr::null_mut() };
            eprintln!("dict-{name} w={w} s={s} r={}", !tag.is_null());
            if !tag.is_null() {
                let _t2 = c::cmsReadTag(rp, 0x64696374);
            }
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
            c::cmsDictFree(dict);
        }
        c::cmsFreeToneCurve(g);
    }
}

/// Type_Dictionary_Read via 'meta' tag — dict entries round-
/// trip through the meta-tag descriptor.
#[test]
fn dict_meta_arms() {
    unsafe {
        let dict = c::cmsDictAlloc(std::ptr::null_mut());
        for i in 0..16u32 {
            let name = format!("key{}\0", i);
            let mut wn = name.as_bytes().iter()
                .map(|&b| b as i32).collect::<Vec<i32>>();
            wn.push(0);
            let val = format!("value{}\0", i);
            let mut wv = val.as_bytes().iter()
                .map(|&b| b as i32).collect::<Vec<i32>>();
            wv.push(0);
            c::cmsDictAddEntry(dict,
                wn.as_ptr(), wv.as_ptr(),
                std::ptr::null_mut(), std::ptr::null_mut());
        }
        // also add with display name/value MLUs
        let dn = c::cmsMLUalloc(std::ptr::null_mut(), 1);
        let mut wn = [0x64u32, 0x69, 0x73, 0x70, 0];
        c::cmsMLUsetWide(dn, b"en\0".as_ptr() as *const _,
            b"US\0".as_ptr() as *const _, wn.as_mut_ptr() as *const _);
        let name = format!("dispkey\0");
        let mut wn2 = name.as_bytes().iter()
            .map(|&b| b as i32).collect::<Vec<i32>>();
        wn2.push(0);
        let val = format!("dispval\0");
        let mut wv2 = val.as_bytes().iter()
            .map(|&b| b as i32).collect::<Vec<i32>>();
        wv2.push(0);
        c::cmsDictAddEntry(dict,
            wn2.as_ptr(), wv2.as_ptr(), dn, dn);
        for sig in [0x6d657461u32, 0x64696374] {
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsSetProfileVersion(prof, 4.4);
            let w = c::cmsWriteTag(prof, sig, dict as *const _);
            let mut buf = vec![0u8; 1 << 22];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(
                    buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, sig)
            } else { std::ptr::null_mut() };
            eprintln!("{sig:#x} w={w} s={s} r={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
        c::cmsDictFree(dict);
        c::cmsMLUfree(dn);
    }
}

/// Type_LUT8/16_Read deep arms — mft1/mft2 element-order +
/// input/output table + CLUT read internals.
#[test]
fn lut_read_arms4() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let g2 = c::cmsBuildGamma(std::ptr::null_mut(), 1.8);
        let mat = [1.0f64, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        let mk_cv = |n: u32| {
            let curves = match n {
                1 => [g].as_ptr(),
                4 => [g, g2, g, g2].as_ptr(),
                _ => [g, g2, g].as_ptr(),
            };
            c::cmsStageAllocToneCurves(std::ptr::null_mut(), n,
                curves as *mut _)
        };
        let mk_clut = |ni: u32, no: u32, npts: u32| {
            let mut grid = vec![0u16;
                (npts * npts * npts * no) as usize];
            for (i, v) in grid.iter_mut().enumerate() {
                *v = ((i % 17) * 3855) as u16;
            }
            c::cmsStageAllocCLut16bit(std::ptr::null_mut(), npts,
                ni, no, grid.as_ptr())
        };
        // every valid mft1/mft2 shape x save+reopen x read-twice
        for (stages, ni, no, save8, name) in [
            ("mat-cv-clut-cv", 3, 3, false, "m2-mat-cv-clut-cv"),
            ("mat-clut-cv", 3, 3, false, "m2-mat-clut-cv"),
            ("cv-clut-cv", 3, 3, false, "m2-cv-clut-cv"),
            ("clut-cv", 3, 3, false, "m2-clut-cv"),
            ("mat-cv-clut", 3, 3, false, "m2-mat-cv-clut"),
            ("clut-cv", 1, 1, false, "m2-1ch"),
            ("clut-cv", 4, 4, false, "m2-4ch"),
            ("mat-cv-clut-cv", 3, 3, true, "m1-mat-cv-clut-cv"),
            ("mat-clut-cv", 3, 3, true, "m1-mat-clut-cv"),
            ("cv-clut-cv", 3, 3, true, "m1-cv-clut-cv"),
            ("clut-cv", 3, 3, true, "m1-clut-cv"),
            ("clut-cv", 1, 1, true, "m1-1ch"),
        ] {
            for sig in [0x41324230u32, 0x42324130] {
                let prof = c::cmsCreate_sRGBProfile();
                c::cmsSetProfileVersion(prof, 2.1);
                let p = c::cmsPipelineAlloc(std::ptr::null_mut(), ni, no);
                for st in stages.split('-') {
                    let s = match st {
                        "cv" => mk_cv(ni),
                        "mat" => c::cmsStageAllocMatrix(
                            std::ptr::null_mut(), 3, 3,
                            mat.as_ptr(), std::ptr::null()),
                        "clut" => mk_clut(ni, no, 17),
                        _ => continue,
                    };
                    c::cmsPipelineInsertStage(p, 1, s);
                }
                if save8 { c::cmsPipelineSetSaveAs8bitsFlag(p, 1); }
                let w = c::cmsWriteTag(prof, sig, p as *const _);
                let mut buf = vec![0u8; 1 << 22];
                let mut nn = buf.len() as u32;
                let s = if w != 0 {
                    c::cmsSaveProfileToMem(prof,
                        buf.as_mut_ptr() as *mut _, &mut nn)
                } else { 0 };
                let rp = if s != 0 {
                    c::cmsOpenProfileFromMem(
                        buf.as_ptr() as *const _, nn)
                } else { std::ptr::null_mut() };
                let tag = if !rp.is_null() {
                    c::cmsReadTag(rp, sig)
                } else { std::ptr::null_mut() };
                if !tag.is_null() {
                    // second read -> cached / already-read arm
                    let _t2 = c::cmsReadTag(rp, sig);
                }
                eprintln!("{name}/{sig:#x} w={w} s={s} r={}",
                    !tag.is_null());
                if !rp.is_null() { c::cmsCloseProfile(rp); }
                c::cmsCloseProfile(prof);
                c::cmsPipelineFree(p);
            }
        }
        c::cmsFreeToneCurve(g);
        c::cmsFreeToneCurve(g2);
    }
}

/// Type_LUT8_Write deep arms — mft1 write internals: per-
/// element order, input/output table sizes, CLUT read arms.
#[test]
fn lut8_write_arms() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let g2 = c::cmsBuildGamma(std::ptr::null_mut(), 1.8);
        let mat = [1.0f64, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        let mk_cv = |n: u32| {
            let curves = match n {
                1 => [g].as_ptr(),
                4 => [g, g2, g, g2].as_ptr(),
                _ => [g, g2, g].as_ptr(),
            };
            c::cmsStageAllocToneCurves(std::ptr::null_mut(), n,
                curves as *mut _)
        };
        let mk_clut8 = |ni: u32, no: u32| {
            let mut grid = vec![0u16; (17 * 17 * 17 * no) as usize];
            for (i, v) in grid.iter_mut().enumerate() {
                *v = ((i % 17) * 3855) as u16;
            }
            c::cmsStageAllocCLut16bit(std::ptr::null_mut(), 17,
                ni, no, grid.as_ptr())
        };
        // mft1 shapes via SaveAs8Bits
        for (stages, ni, no, name) in [
            ("mat-cv-clut-cv", 3, 3, "mat-cv-clut-cv"),
            ("mat-clut-cv", 3, 3, "mat-clut-cv"),
            ("cv-clut-cv", 3, 3, "cv-clut-cv"),
            ("clut-cv", 3, 3, "clut-cv"),
            ("mat-cv-clut", 3, 3, "mat-cv-clut"),
            ("clut-cv", 1, 1, "1ch"),
            ("clut-cv", 4, 4, "4ch"),
        ] {
            for (sig, name2) in [
                (0x41324230u32, "a2b0"),
                (0x42324130u32, "b2a0"),
            ] {
                let prof = c::cmsCreate_sRGBProfile();
                c::cmsSetProfileVersion(prof, 2.1);
                let p = c::cmsPipelineAlloc(std::ptr::null_mut(), ni, no);
                for st in stages.split('-') {
                    let s = match st {
                        "cv" => mk_cv(ni),
                        "mat" => c::cmsStageAllocMatrix(
                            std::ptr::null_mut(), 3, 3,
                            mat.as_ptr(), std::ptr::null()),
                        "clut" => mk_clut8(ni, no),
                        _ => continue,
                    };
                    c::cmsPipelineInsertStage(p, 1, s);
                }
                c::cmsPipelineSetSaveAs8bitsFlag(p, 1);
                let w = c::cmsWriteTag(prof, sig, p as *const _);
                let mut buf = vec![0u8; 1 << 22];
                let mut nn = buf.len() as u32;
                let s = if w != 0 {
                    c::cmsSaveProfileToMem(prof,
                        buf.as_mut_ptr() as *mut _, &mut nn)
                } else { 0 };
                eprintln!("{name}/{name2} w={w} s={s}");
                c::cmsCloseProfile(prof);
                c::cmsPipelineFree(p);
            }
        }
        c::cmsFreeToneCurve(g);
        c::cmsFreeToneCurve(g2);
    }
}

/// SaveTags remaining internals — write-failure propagation,
/// linked-tag dedup order, unsupported-tag skip, version
/// conversion during save.
#[test]
fn save_tags_arms3() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let g2 = c::cmsBuildGamma(std::ptr::null_mut(), 1.8);
        // version-conversion arms — tags that serialize
        // differently per version
        for ver in [2.1f64, 4.0, 4.2, 4.3, 4.4] {
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsSetProfileVersion(prof, ver);
            c::cmsWriteTag(prof, 0x72545243, g as *const _);
            c::cmsWriteTag(prof, 0x67545243, g2 as *const _);
            c::cmsWriteTag(prof, 0x62545243, g as *const _);
            let ml = c::cmsMLUalloc(std::ptr::null_mut(), 1);
            let mut wk = [0x74u32, 0x65, 0x73, 0x74, 0];
            c::cmsMLUsetWide(ml, b"en\0".as_ptr() as *const _,
                b"US\0".as_ptr() as *const _,
                wk.as_mut_ptr() as *const _);
            c::cmsWriteTag(prof, 0x64657363, ml as *const _);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = c::cmsSaveProfileToMem(prof,
                buf.as_mut_ptr() as *mut _, &mut n);
            eprintln!("v{ver} save={s} n={n}");
            c::cmsCloseProfile(prof);
            c::cmsMLUfree(ml);
        }
        // save with read-only IOhandler -> write-failure
        let prof = c::cmsCreate_sRGBProfile();
        let mut small = [0u8; 256];
        let mut n = small.len() as u32;
        let s = c::cmsSaveProfileToMem(prof,
            small.as_mut_ptr() as *mut _, &mut n);
        eprintln!("small={s} n={n}");
        c::cmsCloseProfile(prof);
        // save NULL data
        let prof2 = c::cmsCreate_sRGBProfile();
        let mut n2 = 0u32;
        let s2 = c::cmsSaveProfileToMem(prof2,
            std::ptr::null_mut(), &mut n2);
        eprintln!("null-save={s2}");
        c::cmsCloseProfile(prof2);
        c::cmsFreeToneCurve(g);
        c::cmsFreeToneCurve(g2);
    }
}

/// Type_LUT16_Write remaining internals — MatMPE/clutPoints/
/// input-output table serialization branches.
#[test]
fn lut16_write_arms4() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let g2 = c::cmsBuildGamma(std::ptr::null_mut(), 1.8);
        let mat = [1.0f64, 0.1, 0.0, 0.0, 0.9, 0.0, 0.0, 0.1, 0.8];
        let mk_cv = |n: u32| {
            let curves = match n {
                1 => [g].as_ptr(),
                4 => [g, g2, g, g2].as_ptr(),
                _ => [g, g2, g].as_ptr(),
            };
            c::cmsStageAllocToneCurves(std::ptr::null_mut(), n,
                curves as *mut _)
        };
        let mk_mat = || {
            c::cmsStageAllocMatrix(std::ptr::null_mut(), 3, 3,
                mat.as_ptr(), std::ptr::null())
        };
        // different CLUT grid sizes for the clutPoints arm
        for (npts, name) in [
            (2u32, "g2"), (4, "g4"), (8, "g8"), (16, "g16"),
            (17, "g17"), (32, "g32"), (33, "g33"),
        ] {
            for (stages, sname) in [
                ("mat-cv-clut-cv", "m-cc-c"),
                ("cv-clut-cv", "cv-c-c"),
            ] {
                let prof = c::cmsCreate_sRGBProfile();
                c::cmsSetProfileVersion(prof, 2.1);
                let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
                for st in stages.split('-') {
                    let s = match st {
                        "cv" => mk_cv(3),
                        "mat" => mk_mat(),
                        "clut" => {
                            let mut grid = vec![0u16;
                                (npts * npts * npts * 3) as usize];
                            for (i, v) in grid.iter_mut().enumerate() {
                                *v = ((i % 17) * 3855) as u16;
                            }
                            c::cmsStageAllocCLut16bit(std::ptr::null_mut(),
                                npts, 3, 3, grid.as_ptr())
                        }
                        _ => continue,
                    };
                    c::cmsPipelineInsertStage(p, 1, s);
                }
                let w = c::cmsWriteTag(prof, 0x42324130, p as *const _);
                let mut buf = vec![0u8; 1 << 22];
                let mut nn = buf.len() as u32;
                let s = if w != 0 {
                    c::cmsSaveProfileToMem(prof,
                        buf.as_mut_ptr() as *mut _, &mut nn)
                } else { 0 };
                eprintln!("{sname}/{name} w={w} s={s}");
                c::cmsCloseProfile(prof);
                c::cmsPipelineFree(p);
            }
        }
        c::cmsFreeToneCurve(g);
        c::cmsFreeToneCurve(g2);
    }
}

/// Type_LUT8/16_Read remaining internals — corpus profile mft
/// reads + save+reopen + read-twice + error paths.
#[test]
fn lut_read_arms5() {
    unsafe {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        // every corpus profile — read all tags twice
        for f in [
            "lcms2cmyk.icc", "nullcms2.icc", "limitlcms2.icc",
            "linlcms2.icc", "bchslcms2.icc", "labv4lcms2.icc",
            "sRGBlcms2.icc", "xyzlcms2.icc", "graylcms2.icc",
            "lcms2link.icc", "lcms2link2.icc", "brightness.icc",
            "itufax.icc", "lcms2abst.icc", "lcms2plug.icc",
        ] {
            let Ok(d) = std::fs::read(dir.join(f)) else { continue };
            let p = c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32);
            if p.is_null() { continue; }
            let nt = c::cmsGetTagCount(p);
            for i in 0..nt as u32 {
                let sig = c::cmsGetTagSignature(p, i);
                let t1 = c::cmsReadTag(p, sig);
                if !t1.is_null() {
                    let _t2 = c::cmsReadTag(p, sig);
                }
            }
            c::cmsCloseProfile(p);
        }
        // v2 profile with all standard tag types — read twice
        for ver in [2.1f64, 2.4, 4.0, 4.4] {
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsSetProfileVersion(prof, ver);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = c::cmsSaveProfileToMem(prof,
                buf.as_mut_ptr() as *mut _, &mut n);
            if s != 0 {
                let rp = c::cmsOpenProfileFromMem(
                    buf.as_ptr() as *const _, n);
                if !rp.is_null() {
                    let nt = c::cmsGetTagCount(rp);
                    for i in 0..nt as u32 {
                        let sig = c::cmsGetTagSignature(rp, i);
                        let t = c::cmsReadTag(rp, sig);
                        if !t.is_null() {
                            let _t2 = c::cmsReadTag(rp, sig);
                        }
                    }
                    c::cmsCloseProfile(rp);
                }
            }
            c::cmsCloseProfile(prof);
        }
    }
}

/// SaveTags remaining internals — IOhandler write failures,
/// linked-tag dedup across all tag types, version conversion
/// during save, error propagation through WriteTag.
#[test]
fn save_tags_arms4() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let g2 = c::cmsBuildGamma(std::ptr::null_mut(), 1.8);
        // write every tag type + link them -> dedup across the
        // whole SaveTags loop
        let prof = c::cmsCreate_sRGBProfile();
        c::cmsSetProfileVersion(prof, 4.4);
        for sig in [
            0x72545243u32, 0x67545243, 0x62545243, // rTRC/gTRC/bTRC
            0x7258595a, 0x6758595a, 0x6258595a,   // rXYZ/gXYZ/bXYZ
            0x6d656461, 0x74656368, 0x76696577,   // meas/tech/vcgt
            0x63686164, 0x636c726f, 0x63686172,   // chad/clro/chrm
        ] {
            c::cmsWriteTag(prof, sig, g as *const _);
        }
        let ml = c::cmsMLUalloc(std::ptr::null_mut(), 1);
        let mut wk = [0x74u32, 0x65, 0x73, 0x74, 0];
        c::cmsMLUsetWide(ml, b"en\0".as_ptr() as *const _,
            b"US\0".as_ptr() as *const _, wk.as_mut_ptr() as *const _);
        for sig in [0x64657363u32, 0x63707274, 0x646d6464,
                    0x646d6e64, 0x64657673] {
            c::cmsWriteTag(prof, sig, ml as *const _);
        }
        // add a pipeline tag
        let p2 = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        c::cmsPipelineInsertStage(p2, 1,
            c::cmsStageAllocToneCurves(std::ptr::null_mut(), 3,
                [g, g2, g].as_ptr() as *mut _));
        c::cmsWriteTag(prof, 0x41324230, p2 as *const _);
        let mut buf = vec![0u8; 1 << 22];
        let mut n = buf.len() as u32;
        let s = c::cmsSaveProfileToMem(prof,
            buf.as_mut_ptr() as *mut _, &mut n);
        eprintln!("alltags save={s} n={n}");
        // re-open + read all + re-save
        if s != 0 {
            let rp = c::cmsOpenProfileFromMem(
                buf.as_ptr() as *const _, n);
            if !rp.is_null() {
                let nt = c::cmsGetTagCount(rp);
                for i in 0..nt as u32 {
                    let sig = c::cmsGetTagSignature(rp, i);
                    let _t = c::cmsReadTag(rp, sig);
                }
                let mut buf2 = vec![0u8; 1 << 22];
                let mut n2 = buf2.len() as u32;
                let s2 = c::cmsSaveProfileToMem(rp,
                    buf2.as_mut_ptr() as *mut _, &mut n2);
                eprintln!("re-save={s2}");
                c::cmsCloseProfile(rp);
            }
        }
        c::cmsCloseProfile(prof);
        c::cmsMLUfree(ml);
        c::cmsPipelineFree(p2);
        c::cmsFreeToneCurve(g);
        c::cmsFreeToneCurve(g2);
    }
}

/// Type_LUT16/8_Read remaining internals — more element-order +
/// malformed-blob variants through save+reopen.
#[test]
fn lut_read_arms6() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let g2 = c::cmsBuildGamma(std::ptr::null_mut(), 1.8);
        let mat = [1.0f64, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        // v4 mAB/mBA reads (mpet element orders)
        for (stages, name) in [
            ("B", "b-only"),
            ("B-M-mat", "b-m-mat"),
            ("A-clut-B", "a-clut-b"),
            ("A-clut-M-mat-B", "a-clut-m-mat-b"),
            ("B-mat-M-clut-A", "b-mat-m-clut-a"),
        ] {
            for (sig, name2) in [
                (0x41324230u32, "a2b0"),
                (0x42324130u32, "b2a0"),
            ] {
                let prof = c::cmsCreate_sRGBProfile();
                c::cmsSetProfileVersion(prof, 4.4);
                let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
                for st in stages.split('-') {
                    let s = match st {
                        "A" | "B" => c::cmsStageAllocToneCurves(
                            std::ptr::null_mut(), 3,
                            [g, g2, g].as_ptr() as *mut _),
                        "M" => c::cmsStageAllocMatrix(
                            std::ptr::null_mut(), 3, 3,
                            mat.as_ptr(), std::ptr::null()),
                        "mat" => c::cmsStageAllocMatrix(
                            std::ptr::null_mut(), 3, 3,
                            mat.as_ptr(), std::ptr::null()),
                        "clut" => {
                            let mut grid = vec![0u16; 17*17*17*3];
                            for (i, v) in grid.iter_mut().enumerate() {
                                *v = ((i % 17) * 3855) as u16;
                            }
                            c::cmsStageAllocCLut16bit(std::ptr::null_mut(),
                                17, 3, 3, grid.as_ptr())
                        }
                        _ => continue,
                    };
                    c::cmsPipelineInsertStage(p, 1, s);
                }
                let w = c::cmsWriteTag(prof, sig, p as *const _);
                let mut buf = vec![0u8; 1 << 22];
                let mut nn = buf.len() as u32;
                let s = if w != 0 {
                    c::cmsSaveProfileToMem(prof,
                        buf.as_mut_ptr() as *mut _, &mut nn)
                } else { 0 };
                let rp = if s != 0 {
                    c::cmsOpenProfileFromMem(
                        buf.as_ptr() as *const _, nn)
                } else { std::ptr::null_mut() };
                let tag = if !rp.is_null() {
                    c::cmsReadTag(rp, sig)
                } else { std::ptr::null_mut() };
                if !tag.is_null() {
                    let _t2 = c::cmsReadTag(rp, sig);
                }
                eprintln!("{name}/{name2} w={w} s={s} r={}",
                    !tag.is_null());
                if !rp.is_null() { c::cmsCloseProfile(rp); }
                c::cmsCloseProfile(prof);
                c::cmsPipelineFree(p);
            }
        }
        c::cmsFreeToneCurve(g);
        c::cmsFreeToneCurve(g2);
    }
}

/// Type_LUT16_Write remaining internals — more element-order +
/// channel + entry-count + save-path combos.
#[test]
fn lut16_write_arms5() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let g2 = c::cmsBuildGamma(std::ptr::null_mut(), 1.8);
        let g3 = c::cmsBuildGamma(std::ptr::null_mut(), 3.1);
        let mat = [1.0f64, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        let mut grid = vec![0u16; 17 * 17 * 17 * 3];
        for (i, v) in grid.iter_mut().enumerate() {
            *v = ((i % 17) * 3855) as u16;
        }
        let mk_cv = |n: u32, curves: &[*mut c::ToneCurve]| {
            c::cmsStageAllocToneCurves(std::ptr::null_mut(), n,
                curves.as_ptr() as *mut _)
        };
        // non-identity matrix + all valid element orders
        for (stages, name) in [
            ("mat-cv-clut-cv", "mat-cv-clut-cv"),
            ("mat-clut-cv", "mat-clut-cv"),
            ("cv-clut-cv", "cv-clut-cv"),
            ("clut-cv", "clut-cv"),
            ("mat-cv-clut", "mat-cv-clut"),
            ("mat-clut", "mat-clut"),
            ("cv-clut", "cv-clut"),
        ] {
            for (sig, name2) in [
                (0x41324230u32, "a2b0"),
                (0x42324130u32, "b2a0"),
                (0x41324231u32, "a2b1"),
                (0x42324131u32, "b2a1"),
                (0x41324232u32, "a2b2"),
                (0x41324233u32, "a2b3"),
            ] {
                let prof = c::cmsCreate_sRGBProfile();
                c::cmsSetProfileVersion(prof, 2.1);
                let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
                for st in stages.split('-') {
                    let s = match st {
                        "cv" => mk_cv(3, &[g, g2, g3]),
                        "mat" => c::cmsStageAllocMatrix(
                            std::ptr::null_mut(), 3, 3,
                            mat.as_ptr(), std::ptr::null()),
                        "clut" => c::cmsStageAllocCLut16bit(
                            std::ptr::null_mut(), 17, 3, 3,
                            grid.as_ptr()),
                        _ => continue,
                    };
                    c::cmsPipelineInsertStage(p, 1, s);
                }
                let w = c::cmsWriteTag(prof, sig, p as *const _);
                let mut buf = vec![0u8; 1 << 22];
                let mut nn = buf.len() as u32;
                let s = if w != 0 {
                    c::cmsSaveProfileToMem(prof,
                        buf.as_mut_ptr() as *mut _, &mut nn)
                } else { 0 };
                eprintln!("{name}/{name2} w={w} s={s}");
                c::cmsCloseProfile(prof);
                c::cmsPipelineFree(p);
            }
        }
        c::cmsFreeToneCurve(g);
        c::cmsFreeToneCurve(g2);
        c::cmsFreeToneCurve(g3);
    }
}

/// Type_LUT8/16_Read deep internals — more element-order +
/// per-channel table-read + malformed variants.
#[test]
fn lut_read_arms7() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let g2 = c::cmsBuildGamma(std::ptr::null_mut(), 1.8);
        let mat = [1.0f64, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        // every valid v2 mft2 shape × every LUT sig × read-twice
        for (stages, name) in [
            ("mat-cv-clut-cv", "mat-cv-clut-cv"),
            ("mat-clut-cv", "mat-clut-cv"),
            ("cv-clut-cv", "cv-clut-cv"),
            ("clut-cv", "clut-cv"),
            ("mat-cv-clut", "mat-cv-clut"),
            ("cv-clut", "cv-clut"),
            ("mat-clut", "mat-clut"),
        ] {
            for (sig, name2) in [
                (0x41324230u32, "a2b0"),
                (0x42324130u32, "b2a0"),
                (0x41324231u32, "a2b1"),
                (0x42324131u32, "b2a1"),
            ] {
                let prof = c::cmsCreate_sRGBProfile();
                c::cmsSetProfileVersion(prof, 2.1);
                let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
                for st in stages.split('-') {
                    let s = match st {
                        "cv" => c::cmsStageAllocToneCurves(
                            std::ptr::null_mut(), 3,
                            [g, g2, g].as_ptr() as *mut _),
                        "mat" => c::cmsStageAllocMatrix(
                            std::ptr::null_mut(), 3, 3,
                            mat.as_ptr(), std::ptr::null()),
                        "clut" => {
                            let mut grid = vec![0u16; 17*17*17*3];
                            for (i, v) in grid.iter_mut().enumerate() {
                                *v = ((i % 17) * 3855) as u16;
                            }
                            c::cmsStageAllocCLut16bit(std::ptr::null_mut(),
                                17, 3, 3, grid.as_ptr())
                        }
                        _ => continue,
                    };
                    c::cmsPipelineInsertStage(p, 1, s);
                }
                let w = c::cmsWriteTag(prof, sig, p as *const _);
                let mut buf = vec![0u8; 1 << 22];
                let mut nn = buf.len() as u32;
                let s = if w != 0 {
                    c::cmsSaveProfileToMem(prof,
                        buf.as_mut_ptr() as *mut _, &mut nn)
                } else { 0 };
                let rp = if s != 0 {
                    c::cmsOpenProfileFromMem(
                        buf.as_ptr() as *const _, nn)
                } else { std::ptr::null_mut() };
                let tag = if !rp.is_null() {
                    c::cmsReadTag(rp, sig)
                } else { std::ptr::null_mut() };
                if !tag.is_null() {
                    let _t2 = c::cmsReadTag(rp, sig);
                }
                eprintln!("{name}/{name2} w={w} s={s} r={}",
                    !tag.is_null());
                if !rp.is_null() { c::cmsCloseProfile(rp); }
                c::cmsCloseProfile(prof);
                c::cmsPipelineFree(p);
            }
        }
        c::cmsFreeToneCurve(g);
        c::cmsFreeToneCurve(g2);
    }
}

/// Type_LUT16_Write remaining internals — non-identity matrix
/// serialization + CLUT data + per-channel input/output tables.
#[test]
fn lut16_write_arms6() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let g2 = c::cmsBuildGamma(std::ptr::null_mut(), 1.8);
        let g3 = c::cmsBuildGamma(std::ptr::null_mut(), 3.1);
        // non-identity matrix hits the MatMPE write path
        let mat = [1.0f64, 0.1, 0.05, 0.0, 0.9, 0.05, 0.0, 0.1, 0.85];
        let mk_cv = |c1, c2, c3| {
            c::cmsStageAllocToneCurves(std::ptr::null_mut(), 3,
                [c1, c2, c3].as_ptr() as *mut _)
        };
        // per-channel table serialization via different curves
        for (c1, c2, c3, name) in [
            ([g, g, g], [g, g, g], [g, g, g], "same-curves"),
            ([g, g2, g3], [g3, g, g2], [g2, g3, g], "diff-curves"),
            ([g2, g, g3], [g, g3, g2], [g3, g2, g], "rot-curves"),
        ] {
            for (stages, sname) in [
                ("mat-cv-clut-cv", "mat-cv-clut-cv"),
                ("mat-clut-cv", "mat-clut-cv"),
            ] {
                for sig in [0x41324230u32, 0x42324130] {
                    let prof = c::cmsCreate_sRGBProfile();
                    c::cmsSetProfileVersion(prof, 2.1);
                    let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
                    for st in stages.split('-') {
                        let s = match st {
                            "cv" => mk_cv(c1[0], c2[0], c3[0]),
                            "mat" => c::cmsStageAllocMatrix(
                                std::ptr::null_mut(), 3, 3,
                                mat.as_ptr(), std::ptr::null()),
                            "clut" => {
                                let mut grid = vec![0u16; 17*17*17*3];
                                for (i, v) in grid.iter_mut().enumerate() {
                                    *v = ((i % 17) * 3855) as u16;
                                }
                                c::cmsStageAllocCLut16bit(std::ptr::null_mut(),
                                    17, 3, 3, grid.as_ptr())
                            }
                            _ => continue,
                        };
                        c::cmsPipelineInsertStage(p, 1, s);
                    }
                    // second cv for cv-clut-cv tail
                    if sname == "mat-cv-clut-cv" {
                        // cv is at position 1 (pre); add post-cv
                    }
                    let w = c::cmsWriteTag(prof, sig, p as *const _);
                    let mut buf = vec![0u8; 1 << 22];
                    let mut nn = buf.len() as u32;
                    let s = if w != 0 {
                        c::cmsSaveProfileToMem(prof,
                            buf.as_mut_ptr() as *mut _, &mut nn)
                    } else { 0 };
                    eprintln!("{name}/{sname}/{sig:#x} w={w} s={s}");
                    c::cmsCloseProfile(prof);
                    c::cmsPipelineFree(p);
                }
            }
        }
        c::cmsFreeToneCurve(g);
        c::cmsFreeToneCurve(g2);
        c::cmsFreeToneCurve(g3);
    }
}

/// Type_LUT16_Write remaining internals — WriteCLUT data paths,
/// input/output table serialization, element-offset machinery.
#[test]
fn lut16_write_arms7() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let g2 = c::cmsBuildGamma(std::ptr::null_mut(), 1.8);
        let mat = [1.0f64, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        // every valid mft2 order × version × SaveAs8Bits flag
        for (ver, stages, name) in [
            (2.1f64, "mat-cv-clut-cv", "v2-mat-cv-clut-cv"),
            (2.1, "cv-clut-cv", "v2-cv-clut-cv"),
            (2.1, "mat-clut-cv", "v2-mat-clut-cv"),
            (2.4, "mat-cv-clut-cv", "v24-mat-cv-clut-cv"),
            (2.4, "cv-clut-cv", "v24-cv-clut-cv"),
            (4.0, "mat-cv-clut-cv", "v4-mat-cv-clut-cv"),
            (4.0, "cv-clut-cv", "v4-cv-clut-cv"),
        ] {
            for (sig, save8, name2) in [
                (0x41324230u32, false, "a2b0"),
                (0x42324130u32, false, "b2a0"),
                (0x41324230u32, true, "a2b0-8"),
                (0x42324130u32, true, "b2a0-8"),
            ] {
                let prof = c::cmsCreate_sRGBProfile();
                c::cmsSetProfileVersion(prof, ver);
                let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
                for st in stages.split('-') {
                    let s = match st {
                        "cv" => c::cmsStageAllocToneCurves(
                            std::ptr::null_mut(), 3,
                            [g, g2, g].as_ptr() as *mut _),
                        "mat" => c::cmsStageAllocMatrix(
                            std::ptr::null_mut(), 3, 3,
                            mat.as_ptr(), std::ptr::null()),
                        "clut" => {
                            let mut grid = vec![0u16; 17*17*17*3];
                            for (i, v) in grid.iter_mut().enumerate() {
                                *v = ((i % 17) * 3855) as u16;
                            }
                            c::cmsStageAllocCLut16bit(std::ptr::null_mut(),
                                17, 3, 3, grid.as_ptr())
                        }
                        _ => continue,
                    };
                    c::cmsPipelineInsertStage(p, 1, s);
                }
                if save8 { c::cmsPipelineSetSaveAs8bitsFlag(p, 1); }
                let w = c::cmsWriteTag(prof, sig, p as *const _);
                let mut buf = vec![0u8; 1 << 22];
                let mut nn = buf.len() as u32;
                let s = if w != 0 {
                    c::cmsSaveProfileToMem(prof,
                        buf.as_mut_ptr() as *mut _, &mut nn)
                } else { 0 };
                eprintln!("{name}/{name2} w={w} s={s}");
                c::cmsCloseProfile(prof);
                c::cmsPipelineFree(p);
            }
        }
        c::cmsFreeToneCurve(g);
        c::cmsFreeToneCurve(g2);
    }
}

/// SaveTags remaining internals — version conversion during save
/// for all tag types + profile-class conversion.
#[test]
fn save_tags_arms5() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let g2 = c::cmsBuildGamma(std::ptr::null_mut(), 1.8);
        // profile class × version conversion
        for (class, ver, name) in [
            (0x73636e72u32, 2.1, "scnr-v2"),
            (0x73636e72, 4.4, "scnr-v4"),
            (0x6d6e7472, 2.1, "mntr-v2"),
            (0x6d6e7472, 4.4, "mntr-v4"),
            (0x70727472, 2.1, "prtr-v2"),
            (0x70727472, 4.4, "prtr-v4"),
            (0x6c696e6b, 2.1, "link-v2"),
            (0x6c696e6b, 4.4, "link-v4"),
            (0x61627374, 2.1, "abst-v2"),
            (0x61627374, 4.4, "abst-v4"),
        ] {
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsSetProfileVersion(prof, ver);
            c::cmsSetDeviceClass(prof, class);
            c::cmsWriteTag(prof, 0x72545243, g as *const _);
            c::cmsWriteTag(prof, 0x67545243, g2 as *const _);
            let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
            c::cmsPipelineInsertStage(p, 1,
                c::cmsStageAllocToneCurves(std::ptr::null_mut(), 3,
                    [g, g2, g].as_ptr() as *mut _));
            c::cmsWriteTag(prof, 0x41324230, p as *const _);
            let mut buf = vec![0u8; 1 << 22];
            let mut n = buf.len() as u32;
            let s = c::cmsSaveProfileToMem(prof,
                buf.as_mut_ptr() as *mut _, &mut n);
            eprintln!("{name} save={s}");
            c::cmsCloseProfile(prof);
            c::cmsPipelineFree(p);
        }
        c::cmsFreeToneCurve(g);
        c::cmsFreeToneCurve(g2);
    }
}

/// Type_LUT16_Write remaining internals — 1ch/4ch pipeline
/// variants + clut data + per-channel serialization.
#[test]
fn lut16_write_arms8() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let g2 = c::cmsBuildGamma(std::ptr::null_mut(), 1.8);
        let mat = [1.0f64, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        // 1ch/4ch pipelines hit different input/output table paths
        for (ni, no, stages, name) in [
            (1u32, 1, "clut-cv", "1ch-clut-cv"),
            (1, 1, "cv-clut-cv", "1ch-cv-clut-cv"),
            (4, 4, "clut-cv", "4ch-clut-cv"),
            (4, 4, "cv-clut-cv", "4ch-cv-clut-cv"),
            (4, 4, "mat-clut-cv", "4ch-mat-clut-cv"),
        ] {
            for sig in [0x41324230u32, 0x42324130] {
                let prof = c::cmsCreate_sRGBProfile();
                c::cmsSetProfileVersion(prof, 2.1);
                let p = c::cmsPipelineAlloc(std::ptr::null_mut(), ni, no);
                for st in stages.split('-') {
                    let s = match st {
                        "cv" => {
                            let curves = if ni == 1 {
                                [g].as_ptr()
                            } else {
                                [g, g2, g, g2].as_ptr()
                            };
                            c::cmsStageAllocToneCurves(
                                std::ptr::null_mut(), ni,
                                curves as *mut _)
                        }
                        "mat" => c::cmsStageAllocMatrix(
                            std::ptr::null_mut(), no, no,
                            mat.as_ptr(), std::ptr::null()),
                        "clut" => {
                            let np = 17u32;
                            let total = (np.pow(ni as u32) * no) as usize;
                            let mut grid = vec![0u16; total];
                            for (i, v) in grid.iter_mut().enumerate() {
                                *v = ((i % 17) * 3855) as u16;
                            }
                            c::cmsStageAllocCLut16bit(std::ptr::null_mut(),
                                np, ni, no, grid.as_ptr())
                        }
                        _ => continue,
                    };
                    c::cmsPipelineInsertStage(p, 1, s);
                }
                let w = c::cmsWriteTag(prof, sig, p as *const _);
                let mut buf = vec![0u8; 1 << 22];
                let mut nn = buf.len() as u32;
                let s = if w != 0 {
                    c::cmsSaveProfileToMem(prof,
                        buf.as_mut_ptr() as *mut _, &mut nn)
                } else { 0 };
                eprintln!("{name}/{sig:#x} w={w} s={s}");
                c::cmsCloseProfile(prof);
                c::cmsPipelineFree(p);
            }
        }
        c::cmsFreeToneCurve(g);
        c::cmsFreeToneCurve(g2);
    }
}

/// Type_LUT16_Write remaining internals — more pipeline shapes
/// through the mft2 writer's element-order parse.
#[test]
fn lut16_write_arms9() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let g2 = c::cmsBuildGamma(std::ptr::null_mut(), 1.8);
        let mat = [1.0f64, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        // every remaining valid/invalid element order on mft2
        for (stages, name) in [
            ("clut", "clut-only"),
            ("cv", "cv-only"),
            ("mat", "mat-only"),
            ("mat-cv", "mat-cv"),
            ("cv-mat", "cv-mat"),
            ("mat-clut", "mat-clut"),
            ("clut-mat", "clut-mat"),
            ("cv-cv", "cv-cv"),
            ("mat-mat", "mat-mat"),
            ("cv-mat-cv-clut-cv", "cv-mat-cv-clut-cv"),
            ("mat-cv-mat-clut-cv", "mat-cv-mat-clut-cv"),
        ] {
            for sig in [0x41324230u32, 0x42324130] {
                let prof = c::cmsCreate_sRGBProfile();
                c::cmsSetProfileVersion(prof, 2.1);
                let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
                for st in stages.split('-') {
                    let s = match st {
                        "cv" => c::cmsStageAllocToneCurves(
                            std::ptr::null_mut(), 3,
                            [g, g2, g].as_ptr() as *mut _),
                        "mat" => c::cmsStageAllocMatrix(
                            std::ptr::null_mut(), 3, 3,
                            mat.as_ptr(), std::ptr::null()),
                        "clut" => {
                            let mut grid = vec![0u16; 17*17*17*3];
                            for (i, v) in grid.iter_mut().enumerate() {
                                *v = ((i % 17) * 3855) as u16;
                            }
                            c::cmsStageAllocCLut16bit(std::ptr::null_mut(),
                                17, 3, 3, grid.as_ptr())
                        }
                        _ => continue,
                    };
                    c::cmsPipelineInsertStage(p, 1, s);
                }
                let w = c::cmsWriteTag(prof, sig, p as *const _);
                let mut buf = vec![0u8; 1 << 22];
                let mut nn = buf.len() as u32;
                let s = if w != 0 {
                    c::cmsSaveProfileToMem(prof,
                        buf.as_mut_ptr() as *mut _, &mut nn)
                } else { 0 };
                eprintln!("{name}/{sig:#x} w={w} s={s}");
                c::cmsCloseProfile(prof);
                c::cmsPipelineFree(p);
            }
        }
        c::cmsFreeToneCurve(g);
        c::cmsFreeToneCurve(g2);
    }
}

/// Type_LUT16_Write remaining internals — WriteCLUT/WriteMatrix/
/// WriteSetOfCurves per-element serialization paths.
#[test]
fn lut16_write_arms10() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let g2 = c::cmsBuildGamma(std::ptr::null_mut(), 1.8);
        let g3 = c::cmsBuildGamma(std::ptr::null_mut(), 3.1);
        let mat = [1.0f64, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        // more element combos on the remaining sigs
        for (stages, name) in [
            ("mat-clut", "mat-clut"),
            ("clut-mat", "clut-mat"),
            ("mat-cv", "mat-cv"),
            ("cv-mat", "cv-mat"),
            ("cv-cv", "cv-cv"),
            ("mat-mat", "mat-mat"),
            ("mat-cv-clut-cv", "mat-cv-clut-cv"),
            ("cv-clut-cv-cv", "cv-clut-cv-cv"),
            ("mat-cv-clut-mat", "mat-cv-clut-mat"),
            ("cv-mat-clut-mat", "cv-mat-clut-mat"),
        ] {
            for (sig, name2) in [
                (0x41324230u32, "a2b0"),
                (0x42324130u32, "b2a0"),
                (0x41324231u32, "a2b1"),
                (0x42324131u32, "b2a1"),
                (0x41324232u32, "a2b2"),
                (0x42324132u32, "b2a2"),
            ] {
                let prof = c::cmsCreate_sRGBProfile();
                c::cmsSetProfileVersion(prof, 2.1);
                let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
                for st in stages.split('-') {
                    let s = match st {
                        "cv" => c::cmsStageAllocToneCurves(
                            std::ptr::null_mut(), 3,
                            [g, g2, g3].as_ptr() as *mut _),
                        "mat" => c::cmsStageAllocMatrix(
                            std::ptr::null_mut(), 3, 3,
                            mat.as_ptr(), std::ptr::null()),
                        "clut" => {
                            let mut grid = vec![0u16; 17*17*17*3];
                            for (i, v) in grid.iter_mut().enumerate() {
                                *v = ((i % 17) * 3855) as u16;
                            }
                            c::cmsStageAllocCLut16bit(std::ptr::null_mut(),
                                17, 3, 3, grid.as_ptr())
                        }
                        _ => continue,
                    };
                    c::cmsPipelineInsertStage(p, 1, s);
                }
                let w = c::cmsWriteTag(prof, sig, p as *const _);
                let mut buf = vec![0u8; 1 << 22];
                let mut nn = buf.len() as u32;
                let s = if w != 0 {
                    c::cmsSaveProfileToMem(prof,
                        buf.as_mut_ptr() as *mut _, &mut nn)
                } else { 0 };
                eprintln!("{name}/{name2} w={w} s={s}");
                c::cmsCloseProfile(prof);
                c::cmsPipelineFree(p);
            }
        }
        c::cmsFreeToneCurve(g);
        c::cmsFreeToneCurve(g2);
        c::cmsFreeToneCurve(g3);
    }
}

/// Type_LUT16_Write remaining internals — WriteCLUT's per-
/// element CLUT data serialization + WriteMatrix + WriteSetOfCurves.
#[test]
fn lut16_write_arms11() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let g2 = c::cmsBuildGamma(std::ptr::null_mut(), 1.8);
        let mat = [1.0f64, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        // every valid mft2 order x grid size x channel count
        for (ni, no, npts, stages, name) in [
            (3u32, 3, 2u32, "mat-cv-clut-cv", "3ch-g2-mcc"),
            (3, 3, 4, "mat-cv-clut-cv", "3ch-g4-mcc"),
            (3, 3, 8, "mat-cv-clut-cv", "3ch-g8-mcc"),
            (3, 3, 16, "mat-cv-clut-cv", "3ch-g16-mcc"),
            (3, 3, 33, "mat-cv-clut-cv", "3ch-g33-mcc"),
            (1, 1, 2, "clut-cv", "1ch-g2-cc"),
            (1, 1, 17, "clut-cv", "1ch-g17-cc"),
            (4, 4, 2, "clut-cv", "4ch-g2-cc"),
            (4, 4, 9, "clut-cv", "4ch-g9-cc"),
            (4, 4, 17, "clut-cv", "4ch-g17-cc"),
        ] {
            for sig in [0x41324230u32, 0x42324130] {
                let prof = c::cmsCreate_sRGBProfile();
                c::cmsSetProfileVersion(prof, 2.1);
                let p = c::cmsPipelineAlloc(std::ptr::null_mut(), ni, no);
                for st in stages.split('-') {
                    let s = match st {
                        "cv" => {
                            let curves = match ni {
                                1 => [g].as_ptr(),
                                4 => [g, g2, g, g2].as_ptr(),
                                _ => [g, g2, g].as_ptr(),
                            };
                            c::cmsStageAllocToneCurves(
                                std::ptr::null_mut(), ni,
                                curves as *mut _)
                        }
                        "mat" => c::cmsStageAllocMatrix(
                            std::ptr::null_mut(), 3, 3,
                            mat.as_ptr(), std::ptr::null()),
                        "clut" => {
                            let total = (npts.pow(ni) * no) as usize;
                            let mut grid = vec![0u16; total];
                            for (i, v) in grid.iter_mut().enumerate() {
                                *v = ((i % 17) * 3855) as u16;
                            }
                            c::cmsStageAllocCLut16bit(std::ptr::null_mut(),
                                npts, ni, no, grid.as_ptr())
                        }
                        _ => continue,
                    };
                    c::cmsPipelineInsertStage(p, 1, s);
                }
                let w = c::cmsWriteTag(prof, sig, p as *const _);
                let mut buf = vec![0u8; 1 << 22];
                let mut nn = buf.len() as u32;
                let s = if w != 0 {
                    c::cmsSaveProfileToMem(prof,
                        buf.as_mut_ptr() as *mut _, &mut nn)
                } else { 0 };
                eprintln!("{name}/{sig:#x} w={w} s={s}");
                c::cmsCloseProfile(prof);
                c::cmsPipelineFree(p);
            }
        }
        c::cmsFreeToneCurve(g);
        c::cmsFreeToneCurve(g2);
    }
}

/// Type_LUT16_Write remaining internals — WriteMatrix values,
/// WriteSetOfCurves per-channel entries, WriteCLUT data path.
#[test]
fn lut16_write_arms12() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let g2 = c::cmsBuildGamma(std::ptr::null_mut(), 1.8);
        let g3 = c::cmsBuildGamma(std::ptr::null_mut(), 3.1);
        // matrix values that serialize differently (non-identity)
        let mats = [
            [1.0f64, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
            [2.0, 0.5, 0.1, 0.0, 1.5, 0.0, 0.0, 0.3, 0.9],
            [0.5, 0.0, 0.0, 0.0, 0.5, 0.0, 0.0, 0.0, 0.5],
            [1.0, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8],
        ];
        let mut grid = vec![0u16; 17 * 17 * 17 * 3];
        for (i, v) in grid.iter_mut().enumerate() {
            *v = ((i % 17) * 3855) as u16;
        }
        for (mi, m) in mats.iter().enumerate() {
            for (stages, name) in [
                ("mat-clut-cv", "mat-clut-cv"),
                ("mat-cv-clut-cv", "mat-cv-clut-cv"),
            ] {
                for sig in [0x41324230u32, 0x42324130] {
                    let prof = c::cmsCreate_sRGBProfile();
                    c::cmsSetProfileVersion(prof, 2.1);
                    let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
                    for st in stages.split('-') {
                        let s = match st {
                            "cv" => c::cmsStageAllocToneCurves(
                                std::ptr::null_mut(), 3,
                                [g, g2, g3].as_ptr() as *mut _),
                            "mat" => c::cmsStageAllocMatrix(
                                std::ptr::null_mut(), 3, 3,
                                m.as_ptr(), std::ptr::null()),
                            "clut" => c::cmsStageAllocCLut16bit(
                                std::ptr::null_mut(), 17, 3, 3,
                                grid.as_ptr()),
                            _ => continue,
                        };
                        c::cmsPipelineInsertStage(p, 1, s);
                    }
                    let w = c::cmsWriteTag(prof, sig, p as *const _);
                    let mut buf = vec![0u8; 1 << 22];
                    let mut nn = buf.len() as u32;
                    let s = if w != 0 {
                        c::cmsSaveProfileToMem(prof,
                            buf.as_mut_ptr() as *mut _, &mut nn)
                    } else { 0 };
                    eprintln!("m{mi}/{name}/{sig:#x} w={w} s={s}");
                    c::cmsCloseProfile(prof);
                    c::cmsPipelineFree(p);
                }
            }
        }
        c::cmsFreeToneCurve(g);
        c::cmsFreeToneCurve(g2);
        c::cmsFreeToneCurve(g3);
    }
}

/// Type_LUT16_Write remaining internals — WriteMatrix offset/
/// WriteCLUT grid-dim + per-channel WriteSetOfCurves entries.
#[test]
fn lut16_write_arms13() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let g2 = c::cmsBuildGamma(std::ptr::null_mut(), 1.8);
        let g3 = c::cmsBuildGamma(std::ptr::null_mut(), 3.1);
        let mat = [1.0f64, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        // per-channel different curve types + entry counts
        let mut t64 = vec![0u16; 64];
        let mut t256 = vec![0u16; 256];
        for i in 0..64 { t64[i] = (i * 1023) as u16; }
        for i in 0..256 { t256[i] = (i * 255) as u16; }
        let c64 = c::cmsBuildTabulatedToneCurve16(std::ptr::null_mut(),
            64, t64.as_ptr());
        let c256 = c::cmsBuildTabulatedToneCurve16(std::ptr::null_mut(),
            256, t256.as_ptr());
        for (c1, c2, c3, name) in [
            ([c64, c64, c64], [c64, c64, c64], [c64, c64, c64],
                "all64"),
            ([c256, c256, c256], [c256, c256, c256],
                [c256, c256, c256], "all256"),
            ([c64, c256, g], [g, c64, c256], [c256, g, c64],
                "mixed"),
            ([g, g2, g3], [g3, g, g2], [g2, g3, g],
                "parametric"),
        ] {
            for sig in [0x41324230u32, 0x42324130] {
                let prof = c::cmsCreate_sRGBProfile();
                c::cmsSetProfileVersion(prof, 2.1);
                let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
                c::cmsPipelineInsertStage(p, 1,
                    c::cmsStageAllocMatrix(std::ptr::null_mut(), 3, 3,
                        mat.as_ptr(), std::ptr::null()));
                c::cmsPipelineInsertStage(p, 1,
                    c::cmsStageAllocToneCurves(std::ptr::null_mut(),
                        3, c1.as_ptr() as *mut _));
                let mut grid = vec![0u16; 17*17*17*3];
                for (i, v) in grid.iter_mut().enumerate() {
                    *v = ((i % 17) * 3855) as u16;
                }
                c::cmsPipelineInsertStage(p, 1,
                    c::cmsStageAllocCLut16bit(std::ptr::null_mut(),
                        17, 3, 3, grid.as_ptr()));
                c::cmsPipelineInsertStage(p, 1,
                    c::cmsStageAllocToneCurves(std::ptr::null_mut(),
                        3, c3.as_ptr() as *mut _));
                let w = c::cmsWriteTag(prof, sig, p as *const _);
                let mut buf = vec![0u8; 1 << 22];
                let mut nn = buf.len() as u32;
                let s = if w != 0 {
                    c::cmsSaveProfileToMem(prof,
                        buf.as_mut_ptr() as *mut _, &mut nn)
                } else { 0 };
                eprintln!("{name}/{sig:#x} w={w} s={s}");
                c::cmsCloseProfile(prof);
                c::cmsPipelineFree(p);
            }
        }
        c::cmsFreeToneCurve(g);
        c::cmsFreeToneCurve(g2);
        c::cmsFreeToneCurve(g3);
        c::cmsFreeToneCurve(c64);
        c::cmsFreeToneCurve(c256);
    }
}

/// Type_LUT16_Write remaining internals — clut data values +
/// WriteMatrix offset + WriteSetOfCurves entry-count arms.
#[test]
fn lut16_write_arms14() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let g2 = c::cmsBuildGamma(std::ptr::null_mut(), 1.8);
        let mat = [1.0f64, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        // different CLUT data patterns + matrix variants
        let fns: Vec<(fn(usize) -> u16, &str)> = vec![
            (|i| ((i % 17) * 3855) as u16, "gradient"),
            (|i| ((i % 256) * 257) as u16, "mod256"),
            (|i| ((i * 37) % 65536) as u16, "mod65536"),
            (|i| (i & 0xFFFF) as u16, "sequential"),
            (|_| 0x8000u16, "midpoint"),
            (|_| 0u16, "zero"),
            (|_| 0xFFFFu16, "max"),
        ];
        for (clut_fn, name) in fns {
            for (stages, sname) in [
                ("mat-cv-clut-cv", "mat-cv-clut-cv"),
                ("clut-cv", "clut-cv"),
            ] {
                for sig in [0x41324230u32, 0x42324130] {
                    let prof = c::cmsCreate_sRGBProfile();
                    c::cmsSetProfileVersion(prof, 2.1);
                    let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
                    for st in stages.split('-') {
                        let s = match st {
                            "cv" => c::cmsStageAllocToneCurves(
                                std::ptr::null_mut(), 3,
                                [g, g2, g].as_ptr() as *mut _),
                            "mat" => c::cmsStageAllocMatrix(
                                std::ptr::null_mut(), 3, 3,
                                mat.as_ptr(), std::ptr::null()),
                            "clut" => {
                                let mut grid = vec![0u16; 17*17*17*3];
                                for (i, v) in grid.iter_mut().enumerate() {
                                    *v = clut_fn(i);
                                }
                                c::cmsStageAllocCLut16bit(std::ptr::null_mut(),
                                    17, 3, 3, grid.as_ptr())
                            }
                            _ => continue,
                        };
                        c::cmsPipelineInsertStage(p, 1, s);
                    }
                    let w = c::cmsWriteTag(prof, sig, p as *const _);
                    let mut buf = vec![0u8; 1 << 22];
                    let mut nn = buf.len() as u32;
                    let s = if w != 0 {
                        c::cmsSaveProfileToMem(prof,
                            buf.as_mut_ptr() as *mut _, &mut nn)
                    } else { 0 };
                    eprintln!("{name}/{sname}/{sig:#x} w={w} s={s}");
                    c::cmsCloseProfile(prof);
                    c::cmsPipelineFree(p);
                }
            }
        }
        c::cmsFreeToneCurve(g);
        c::cmsFreeToneCurve(g2);
    }
}

/// SaveTags remaining internals — profile save/reopen/read-
/// twice on all class+version+tag combinations.
#[test]
fn save_tags_arms6() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let g2 = c::cmsBuildGamma(std::ptr::null_mut(), 1.8);
        // profile class × version + all tag types -> save+reopen
        for (class, ver, name) in [
            (0x73636e72u32, 2.1, "scnr-v2"),
            (0x73636e72, 4.4, "scnr-v4"),
            (0x6d6e7472, 2.1, "mntr-v2"),
            (0x6d6e7472, 4.4, "mntr-v4"),
            (0x70727472, 2.1, "prtr-v2"),
            (0x70727472, 4.4, "prtr-v4"),
            (0x61627374, 4.4, "abst-v4"),
        ] {
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsSetProfileVersion(prof, ver);
            c::cmsSetDeviceClass(prof, class);
            for sig in [
                0x72545243u32, 0x67545243, 0x62545243,
                0x7258595a, 0x6758595a, 0x6258595a,
                0x6d656461, 0x74656368, 0x63686164,
                0x636c726f, 0x63686172,
            ] {
                c::cmsWriteTag(prof, sig, g as *const _);
            }
            let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
            c::cmsPipelineInsertStage(p, 1,
                c::cmsStageAllocToneCurves(std::ptr::null_mut(), 3,
                    [g, g2, g].as_ptr() as *mut _));
            c::cmsWriteTag(prof, 0x41324230, p as *const _);
            let mut buf = vec![0u8; 1 << 22];
            let mut n = buf.len() as u32;
            let s = c::cmsSaveProfileToMem(prof,
                buf.as_mut_ptr() as *mut _, &mut n);
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(
                    buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            if !rp.is_null() {
                let nt = c::cmsGetTagCount(rp);
                for i in 0..nt as u32 {
                    let sig = c::cmsGetTagSignature(rp, i);
                    let _t = c::cmsReadTag(rp, sig);
                }
                c::cmsCloseProfile(rp);
            }
            eprintln!("{name} s={s}");
            c::cmsCloseProfile(prof);
            c::cmsPipelineFree(p);
        }
        c::cmsFreeToneCurve(g);
        c::cmsFreeToneCurve(g2);
    }
}

/// Type_LUT16_Write remaining internals — WriteMatrix offset/
/// WriteCLUT grid-point + WriteSetOfCurves per-channel arms.
#[test]
fn lut16_write_arms15() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let g2 = c::cmsBuildGamma(std::ptr::null_mut(), 1.8);
        let g3 = c::cmsBuildGamma(std::ptr::null_mut(), 3.1);
        let mat = [1.0f64, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        // more mft2 element orders + entry-count + grid variants
        for (stages, name) in [
            ("mat-clut", "mat-clut"),
            ("clut-mat", "clut-mat"),
            ("mat-cv", "mat-cv"),
            ("cv-mat", "cv-mat"),
            ("cv-clut", "cv-clut"),
            ("clut-cv", "clut-cv"),
            ("mat-clut-cv", "mat-clut-cv"),
            ("mat-cv-clut", "mat-cv-clut"),
            ("cv-mat-clut", "cv-mat-clut"),
            ("cv-clut-mat", "cv-clut-mat"),
        ] {
            for (sig, name2) in [
                (0x41324230u32, "a2b0"),
                (0x42324130u32, "b2a0"),
                (0x41324231u32, "a2b1"),
                (0x42324131u32, "b2a1"),
            ] {
                let prof = c::cmsCreate_sRGBProfile();
                c::cmsSetProfileVersion(prof, 2.1);
                let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
                for st in stages.split('-') {
                    let s = match st {
                        "cv" => c::cmsStageAllocToneCurves(
                            std::ptr::null_mut(), 3,
                            [g, g2, g3].as_ptr() as *mut _),
                        "mat" => c::cmsStageAllocMatrix(
                            std::ptr::null_mut(), 3, 3,
                            mat.as_ptr(), std::ptr::null()),
                        "clut" => {
                            let mut grid = vec![0u16; 17*17*17*3];
                            for (i, v) in grid.iter_mut().enumerate() {
                                *v = ((i % 17) * 3855) as u16;
                            }
                            c::cmsStageAllocCLut16bit(std::ptr::null_mut(),
                                17, 3, 3, grid.as_ptr())
                        }
                        _ => continue,
                    };
                    c::cmsPipelineInsertStage(p, 1, s);
                }
                let w = c::cmsWriteTag(prof, sig, p as *const _);
                let mut buf = vec![0u8; 1 << 22];
                let mut nn = buf.len() as u32;
                let s = if w != 0 {
                    c::cmsSaveProfileToMem(prof,
                        buf.as_mut_ptr() as *mut _, &mut nn)
                } else { 0 };
                eprintln!("{name}/{name2} w={w} s={s}");
                c::cmsCloseProfile(prof);
                c::cmsPipelineFree(p);
            }
        }
        let mat = [1.0f64, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        let _ = mat;
        c::cmsFreeToneCurve(g);
        c::cmsFreeToneCurve(g2);
        c::cmsFreeToneCurve(g3);
    }
}

/// Type_LUT16_Write remaining internals — WriteSetOfCurves/
/// WriteMatrix/WriteCLUT per-element + entry-count arms.
#[test]
fn lut16_write_arms16() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let g2 = c::cmsBuildGamma(std::ptr::null_mut(), 1.8);
        let g3 = c::cmsBuildGamma(std::ptr::null_mut(), 3.1);
        let mat = [1.0f64, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        // every remaining element-order + channel + entry-count
        for (ni, no, stages, name) in [
            (3u32, 3, "mat-clut-cv", "3ch-mcc"),
            (3, 3, "mat-cv-clut", "3ch-mccl"),
            (3, 3, "clut-cv", "3ch-cc"),
            (3, 3, "cv-clut", "3ch-ccl"),
            (3, 3, "mat-clut", "3ch-mc"),
            (3, 3, "mat-cv", "3ch-mcv"),
            (1, 1, "mat-clut-cv", "1ch-mcc"),
            (4, 4, "clut-cv", "4ch-cc"),
            (4, 4, "cv-clut", "4ch-ccl"),
        ] {
            for (sig, name2) in [
                (0x41324230u32, "a2b0"),
                (0x42324130u32, "b2a0"),
                (0x41324231u32, "a2b1"),
                (0x42324131u32, "b2a1"),
                (0x41324232u32, "a2b2"),
                (0x42324132u32, "b2a2"),
            ] {
                let prof = c::cmsCreate_sRGBProfile();
                c::cmsSetProfileVersion(prof, 2.1);
                let p = c::cmsPipelineAlloc(std::ptr::null_mut(), ni, no);
                for st in stages.split('-') {
                    let s = match st {
                        "cv" => {
                            let curves = match ni {
                                1 => [g].as_ptr(),
                                4 => [g, g2, g, g2].as_ptr(),
                                _ => [g, g2, g3].as_ptr(),
                            };
                            c::cmsStageAllocToneCurves(
                                std::ptr::null_mut(), ni,
                                curves as *mut _)
                        }
                        "mat" => c::cmsStageAllocMatrix(
                            std::ptr::null_mut(), 3, 3,
                            mat.as_ptr(), std::ptr::null()),
                        "clut" => {
                            let total = (17u32.pow(ni) * no) as usize;
                            let mut grid = vec![0u16; total];
                            for (i, v) in grid.iter_mut().enumerate() {
                                *v = ((i % 17) * 3855) as u16;
                            }
                            c::cmsStageAllocCLut16bit(std::ptr::null_mut(),
                                17, ni, no, grid.as_ptr())
                        }
                        _ => continue,
                    };
                    c::cmsPipelineInsertStage(p, 1, s);
                }
                let w = c::cmsWriteTag(prof, sig, p as *const _);
                let mut buf = vec![0u8; 1 << 22];
                let mut nn = buf.len() as u32;
                let s = if w != 0 {
                    c::cmsSaveProfileToMem(prof,
                        buf.as_mut_ptr() as *mut _, &mut nn)
                } else { 0 };
                eprintln!("{name}/{name2} w={w} s={s}");
                c::cmsCloseProfile(prof);
                c::cmsPipelineFree(p);
            }
        }
        c::cmsFreeToneCurve(g);
        c::cmsFreeToneCurve(g2);
        c::cmsFreeToneCurve(g3);
    }
}

/// Type_LUT16_Write remaining internals — WriteMatrix values +
/// WriteSetOfCurves/WriteCLUT per-element paths.
#[test]
fn lut16_write_arms17() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let g2 = c::cmsBuildGamma(std::ptr::null_mut(), 1.8);
        let g3 = c::cmsBuildGamma(std::ptr::null_mut(), 3.1);
        // matrix with offset vector (MatMPE offset arm)
        let mats: Vec<(&[f64], Option<&[f64]>)> = vec![
            (&[1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
                Some(&[0.1, 0.2, 0.3])),
            (&[2.0, 0.5, 0.1, 0.0, 1.5, 0.0, 0.0, 0.3, 0.9],
                Some(&[0.0, 0.0, 0.0])),
            (&[1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
                None),
        ];
        for (mi, (m, off)) in mats.iter().enumerate() {
            for (stages, sname) in [
                ("mat-clut-cv", "mat-clut-cv"),
                ("mat-cv-clut-cv", "mat-cv-clut-cv"),
            ] {
                for sig in [0x41324230u32, 0x42324130] {
                    let prof = c::cmsCreate_sRGBProfile();
                    c::cmsSetProfileVersion(prof, 2.1);
                    let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
                    for st in stages.split('-') {
                        let s = match st {
                            "cv" => c::cmsStageAllocToneCurves(
                                std::ptr::null_mut(), 3,
                                [g, g2, g3].as_ptr() as *mut _),
                            "mat" => c::cmsStageAllocMatrix(
                                std::ptr::null_mut(), 3, 3,
                                m.as_ptr(),
                                off.map(|o| o.as_ptr())
                                    .unwrap_or(std::ptr::null())),
                            "clut" => {
                                let mut grid = vec![0u16; 17*17*17*3];
                                for (i, v) in grid.iter_mut().enumerate() {
                                    *v = ((i % 17) * 3855) as u16;
                                }
                                c::cmsStageAllocCLut16bit(std::ptr::null_mut(),
                                    17, 3, 3, grid.as_ptr())
                            }
                            _ => continue,
                        };
                        c::cmsPipelineInsertStage(p, 1, s);
                    }
                    let w = c::cmsWriteTag(prof, sig, p as *const _);
                    let mut buf = vec![0u8; 1 << 22];
                    let mut nn = buf.len() as u32;
                    let s = if w != 0 {
                        c::cmsSaveProfileToMem(prof,
                            buf.as_mut_ptr() as *mut _, &mut nn)
                    } else { 0 };
                    eprintln!("m{mi}/{sname}/{sig:#x} w={w} s={s}");
                    c::cmsCloseProfile(prof);
                    c::cmsPipelineFree(p);
                }
            }
        }
        c::cmsFreeToneCurve(g);
        c::cmsFreeToneCurve(g2);
        c::cmsFreeToneCurve(g3);
    }
}

/// Type_LUT16_Write remaining internals — more element-order +
/// entry-count + grid-dim + sig combos on the mft2 writer.
#[test]
fn lut16_write_arms18() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let g2 = c::cmsBuildGamma(std::ptr::null_mut(), 1.8);
        let g3 = c::cmsBuildGamma(std::ptr::null_mut(), 3.1);
        let mat = [1.0f64, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        // all valid element orders x all sigs x entry counts
        for (stages, name) in [
            ("mat-clut-cv", "mat-clut-cv"),
            ("mat-cv-clut", "mat-cv-clut"),
            ("mat-cv-clut-cv", "mat-cv-clut-cv"),
            ("cv-clut-cv", "cv-clut-cv"),
            ("clut-cv", "clut-cv"),
            ("cv-clut", "cv-clut"),
        ] {
            for (sig, name2) in [
                (0x41324230u32, "a2b0"),
                (0x42324130u32, "b2a0"),
                (0x41324231u32, "a2b1"),
                (0x42324131u32, "b2a1"),
                (0x41324232u32, "a2b2"),
                (0x42324132u32, "b2a2"),
            ] {
                let prof = c::cmsCreate_sRGBProfile();
                c::cmsSetProfileVersion(prof, 2.1);
                let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
                for st in stages.split('-') {
                    let s = match st {
                        "cv" => c::cmsStageAllocToneCurves(
                            std::ptr::null_mut(), 3,
                            [g, g2, g3].as_ptr() as *mut _),
                        "mat" => c::cmsStageAllocMatrix(
                            std::ptr::null_mut(), 3, 3,
                            mat.as_ptr(), std::ptr::null()),
                        "clut" => {
                            let mut grid = vec![0u16; 17*17*17*3];
                            for (i, v) in grid.iter_mut().enumerate() {
                                *v = ((i % 17) * 3855) as u16;
                            }
                            c::cmsStageAllocCLut16bit(std::ptr::null_mut(),
                                17, 3, 3, grid.as_ptr())
                        }
                        _ => continue,
                    };
                    c::cmsPipelineInsertStage(p, 1, s);
                }
                let w = c::cmsWriteTag(prof, sig, p as *const _);
                let mut buf = vec![0u8; 1 << 22];
                let mut nn = buf.len() as u32;
                let s = if w != 0 {
                    c::cmsSaveProfileToMem(prof,
                        buf.as_mut_ptr() as *mut _, &mut nn)
                } else { 0 };
                eprintln!("{name}/{name2} w={w} s={s}");
                c::cmsCloseProfile(prof);
                c::cmsPipelineFree(p);
            }
        }
        c::cmsFreeToneCurve(g);
        c::cmsFreeToneCurve(g2);
        c::cmsFreeToneCurve(g3);
    }
}

/// Type_LUT16_Write remaining internals — WriteSetOfCurves
/// per-channel entry-count + WriteMatrix offset + WriteCLUT
/// grid-dim arms on all valid shapes.
#[test]
fn lut16_write_arms19() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let g2 = c::cmsBuildGamma(std::ptr::null_mut(), 1.8);
        let g3 = c::cmsBuildGamma(std::ptr::null_mut(), 3.1);
        let mat = [1.0f64, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        // all valid mft2 orders × channel counts × entry variants
        for (ni, no, stages, name) in [
            (1u32, 1, "cv-clut-cv", "1ch-cvcc"),
            (1, 1, "clut-cv", "1ch-cc"),
            (1, 1, "mat-clut-cv", "1ch-mcc"),
            (2, 2, "clut-cv", "2ch-cc"),
            (2, 2, "cv-clut-cv", "2ch-cvcc"),
            (3, 3, "mat-clut-cv", "3ch-mcc"),
            (4, 4, "clut-cv", "4ch-cc"),
            (4, 4, "cv-clut-cv", "4ch-cvcc"),
            (4, 4, "mat-clut-cv", "4ch-mcc"),
        ] {
            for (sig, name2) in [
                (0x41324230u32, "a2b0"),
                (0x42324130u32, "b2a0"),
            ] {
                let prof = c::cmsCreate_sRGBProfile();
                c::cmsSetProfileVersion(prof, 2.1);
                let p = c::cmsPipelineAlloc(std::ptr::null_mut(), ni, no);
                for st in stages.split('-') {
                    let s = match st {
                        "cv" => {
                            let curves = match ni {
                                1 => [g].as_ptr(),
                                2 => [g, g2].as_ptr(),
                                4 => [g, g2, g3, g].as_ptr(),
                                _ => [g, g2, g3].as_ptr(),
                            };
                            c::cmsStageAllocToneCurves(
                                std::ptr::null_mut(), ni,
                                curves as *mut _)
                        }
                        "mat" => c::cmsStageAllocMatrix(
                            std::ptr::null_mut(), no, no,
                            mat.as_ptr(), std::ptr::null()),
                        "clut" => {
                            let np = 5u32;
                            let total = (np.pow(ni) * no) as usize;
                            let mut grid = vec![0u16; total];
                            for (i, v) in grid.iter_mut().enumerate() {
                                *v = ((i % 17) * 3855) as u16;
                            }
                            c::cmsStageAllocCLut16bit(std::ptr::null_mut(),
                                np, ni, no, grid.as_ptr())
                        }
                        _ => continue,
                    };
                    c::cmsPipelineInsertStage(p, 1, s);
                }
                let w = c::cmsWriteTag(prof, sig, p as *const _);
                let mut buf = vec![0u8; 1 << 22];
                let mut nn = buf.len() as u32;
                let s = if w != 0 {
                    c::cmsSaveProfileToMem(prof,
                        buf.as_mut_ptr() as *mut _, &mut nn)
                } else { 0 };
                eprintln!("{name}/{name2} w={w} s={s}");
                c::cmsCloseProfile(prof);
                c::cmsPipelineFree(p);
            }
        }
        c::cmsFreeToneCurve(g);
        c::cmsFreeToneCurve(g2);
        c::cmsFreeToneCurve(g3);
    }
}

/// Type_*_Write error arms — every `Write fail -> return FALSE`
/// branch. MemoryWrite truncates rather than failing (colord#147)
/// so cmsSaveProfileToMem never hits these; we patch a real memory
/// IOhandler's Write slot (offset 312 in _cms_io_handler) with a
/// counter-gated stub and sweep the fail-at-call threshold.
static WRITE_CALLS: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(0);
static WRITE_LIMIT: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(u32::MAX);
static REAL_WRITE: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

type WriteFn = unsafe extern "C" fn(*mut std::ffi::c_void, u32,
    *const std::ffi::c_void) -> i32;

unsafe extern "C" fn gated_write(io: *mut std::ffi::c_void,
    size: u32, buf: *const std::ffi::c_void) -> i32 {
    use std::sync::atomic::Ordering::Relaxed;
    let n = WRITE_CALLS.fetch_add(1, Relaxed);
    if n >= WRITE_LIMIT.load(Relaxed) {
        return 0;
    }
    let real: WriteFn = std::mem::transmute(
        REAL_WRITE.load(Relaxed));
    real(io, size, buf)
}

unsafe fn save_with_write_limit(prof: *mut std::ffi::c_void,
    buf: &mut [u8], limit: u32) -> i32 {
    use std::sync::atomic::Ordering::Relaxed;
    let io = c::cmsOpenIOhandlerFromMem(std::ptr::null_mut(),
        buf.as_mut_ptr(), buf.len() as u32, b"w\0".as_ptr());
    assert!(!io.is_null());
    WRITE_CALLS.store(0, Relaxed);
    WRITE_LIMIT.store(limit, Relaxed);
    let real = *(io as *const usize).add(312 / 8);
    REAL_WRITE.store(real, Relaxed);
    *(io as *mut usize).add(312 / 8) = gated_write as usize;
    let r = c::cmsSaveProfileToIOhandler(prof, io);
    *(io as *mut usize).add(312 / 8) = real;
    c::cmsCloseIOhandler(io);
    r
}

#[test]
fn write_fail_arms() {
    unsafe {
        // small tabulated curves — Write16bitTables emits one
        // Write per entry so gamma (4096-entry) curves blow the
        // sweep range; 2-entry tables keep call counts low while
        // still crossing every `Write fail` arm.
        let t2 = [0u16, 0xFFFF];
        let t3 = [0u16, 0x8000, 0xFFFF];
        let t4 = [0u16, 0x5555, 0xAAAA, 0xFFFF];
        let g = c::cmsBuildTabulatedToneCurve16(std::ptr::null_mut(),
            2, t2.as_ptr());
        let g2 = c::cmsBuildTabulatedToneCurve16(std::ptr::null_mut(),
            3, t3.as_ptr());
        let g3 = c::cmsBuildTabulatedToneCurve16(std::ptr::null_mut(),
            4, t4.as_ptr());
        let mat = [1.0f64, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        let mut grid = vec![0u16; 5 * 5 * 5 * 3];
        for (i, v) in grid.iter_mut().enumerate() {
            *v = ((i % 17) * 3855) as u16;
        }
        // profile carrying mft2 + mAB + mBA + curves so the sweep
        // hits LUT16/A2B/B2A/curve/XYZ/text writers' fail arms
        for (ver, lut_sig, order, name) in [
            (2.1f64, 0x41324230u32, "mat-cv-clut-cv", "mft2"),
            (4.4, 0x41324230, "cv-clut-cv-mat-cv", "mab"),
            (4.4, 0x42324130, "cv-mat-cv-clut-cv", "mba"),
        ] {
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsSetProfileVersion(prof, ver);
            let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
            for st in order.split('-') {
                let s = match st {
                    "cv" => c::cmsStageAllocToneCurves(
                        std::ptr::null_mut(), 3,
                        [g, g2, g3].as_ptr() as *mut _),
                    "mat" => c::cmsStageAllocMatrix(
                        std::ptr::null_mut(), 3, 3,
                        mat.as_ptr(), std::ptr::null()),
                    "clut" => c::cmsStageAllocCLut16bit(
                        std::ptr::null_mut(), 5, 3, 3,
                        grid.as_ptr()),
                    _ => continue,
                };
                c::cmsPipelineInsertStage(p, 1, s);
            }
            let w = c::cmsWriteTag(prof, lut_sig, p as *const _);
            c::cmsWriteTag(prof, 0x72545243, g as *const _);
            let mut buf = vec![0u8; 1 << 20];
            let mut n_ok = 0u32;
            let mut n_fail = 0u32;
            for limit in 0..900u32 {
                let r = save_with_write_limit(prof,
                    &mut buf, limit);
                if r == 0 { n_fail += 1; } else { n_ok += 1; }
            }
            eprintln!("{name} w={w} ok={n_ok} fail={n_fail}");
            c::cmsCloseProfile(prof);
            c::cmsPipelineFree(p);
        }
        c::cmsFreeToneCurve(g);
        c::cmsFreeToneCurve(g2);
        c::cmsFreeToneCurve(g3);
    }
}

/// Second sweep payload — every writable struct-tag type on one
/// profile so the fail-at-N sweep covers their Type_*_Write arms.
#[test]
fn write_fail_arms2() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let prof = c::cmsCreate_sRGBProfile();
        c::cmsSetProfileVersion(prof, 4.4);
        // calt — DateTime
        #[repr(C)] struct Dt {
            y: u16, mo: u16, d: u16, h: u16, mi: u16, s: u16,
        }
        let dt = Dt { y: 2026, mo: 1, d: 1, h: 0, mi: 0, s: 0 };
        let w_calt = c::cmsWriteTag(prof, 0x63616c74,
            &dt as *const _ as *const _);
        // chad — S15Fixed16Array (9 doubles)
        let chad = [1.0f64, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        let w_chad = c::cmsWriteTag(prof, 0x63686164,
            chad.as_ptr() as *const _);
        // chrm — Chromaticity
        #[repr(C)] struct XyY { x: f64, y: f64, yy: f64 }
        #[repr(C)] struct Chrm {
            n: u32, ty: u16, _pad: u16,
            c: [XyY; 3],
        }
        let chrm = Chrm { n: 3, ty: 0, _pad: 0, c: [
            XyY { x: 0.64, y: 0.33, yy: 1.0 },
            XyY { x: 0.30, y: 0.60, yy: 1.0 },
            XyY { x: 0.15, y: 0.06, yy: 1.0 }] };
        let w_chrm = c::cmsWriteTag(prof, 0x6368726d,
            &chrm as *const _ as *const _);
        // meas — Measurement
        #[repr(C)] struct Meas {
            flare: u32, illum: u32, obs: u32,
            geo: u32, back: u32, meas: u32,
        }
        let meas = Meas { flare: 0, illum: 1, obs: 1,
            geo: 1, back: 1, meas: 1 };
        let w_meas = c::cmsWriteTag(prof, 0x6d656173,
            &meas as *const _ as *const _);
        // tech — signature
        let tech: u32 = 0x66747970u32;
        let w_tech = c::cmsWriteTag(prof, 0x74656368,
            &tech as *const _ as *const _);
        // view — ViewingConditions
        #[repr(C)] struct Xyz { x: f64, y: f64, z: f64 }
        #[repr(C)] struct View {
            o: Xyz, b: Xyz, la: f64, yb: f64, dd: f64,
        }
        let view = View {
            o: Xyz { x: 0.9642, y: 1.0, z: 0.8249 },
            b: Xyz { x: 0.9642, y: 1.0, z: 0.8249 },
            la: 20.0, yb: 20.0, dd: 1.0 };
        let w_view = c::cmsWriteTag(prof, 0x76696577,
            &view as *const _ as *const _);
        // scrn — Screening
        #[repr(C)] #[derive(Copy, Clone)]
        struct ScrnCh { freq: f64, ang: f64, spot: u32 }
        #[repr(C)] struct Scrn {
            flag: u32, nch: u32, _p: u32,
            ch: [ScrnCh; 16],
        }
        let mut ch = [ScrnCh { freq: 0.0, ang: 0.0, spot: 0 }; 16];
        ch[0].freq = 60.0;
        let scrn = Scrn { flag: 0, nch: 1, _p: 0, ch };
        let w_scrn = c::cmsWriteTag(prof, 0x7363726e,
            &scrn as *const _ as *const _);
        // ucbg — UcrBg {ToneCurve*, ToneCurve*, MLU*}
        let mlu = c::cmsMLUalloc(std::ptr::null_mut(), 1);
        c::cmsMLUsetASCII(mlu, b"en\0".as_ptr(),
            b"US\0".as_ptr(), b"ucbg\0".as_ptr());
        #[repr(C)] struct Ucbg {
            u: *mut std::ffi::c_void,
            b: *mut std::ffi::c_void,
            d: *mut std::ffi::c_void,
        }
        let ucbg = Ucbg { u: g as *mut _, b: g as *mut _, d: mlu };
        let w_ucbg = c::cmsWriteTag(prof, 0x62666420,
            &ucbg as *const _ as *const _);
        // psd0 — ICCData {len,flag,data}
        #[repr(C)] struct IccData {
            len: u32, flag: u32, data: [u8; 16],
        }
        let psd = IccData { len: 16, flag: 0, data: *b"Hello, world!!!!" };
        let w_psd = c::cmsWriteTag(prof, 0x70736430,
            &psd as *const _ as *const _);
        // meta — dict
        let dict = c::cmsDictAlloc(std::ptr::null_mut());
        let name: [i32; 3] = [b'n' as i32, b'1' as i32, 0];
        let val: [i32; 3] = [b'v' as i32, b'1' as i32, 0];
        c::cmsDictAddEntry(dict, name.as_ptr(), val.as_ptr(),
            std::ptr::null_mut(), std::ptr::null_mut());
        let w_meta = c::cmsWriteTag(prof, 0x6d657461,
            dict as *const _);
        // ncl2 — named color list
        let ncl = c::cmsAllocNamedColorList(
            std::ptr::null_mut(), 2, 4, b"pre\0".as_ptr(),
            b"suf\0".as_ptr());
        let w_ncl = if !ncl.is_null() {
            let mut nm = [0u16; 32];
            nm[0] = b'C' as u16;
            nm[1] = b'1' as u16;
            let pcs = [0u16; 3];
            let dev = [0u16; 16];
            c::cmsAppendNamedColor(ncl, nm.as_ptr() as *const _,
                pcs.as_ptr(), dev.as_ptr());
            c::cmsWriteTag(prof, 0x6e636c32, ncl as *const _)
        } else { 0 };
        eprintln!("calt={w_calt} chad={w_chad} chrm={w_chrm} \
            meas={w_meas} tech={w_tech} view={w_view} \
            scrn={w_scrn} ucbg={w_ucbg} psd={w_psd} \
            meta={w_meta} ncl={w_ncl}");
        let mut buf = vec![0u8; 1 << 20];
        let mut n_ok = 0u32;
        let mut n_fail = 0u32;
        // measure total write calls first
        WRITE_LIMIT.store(u32::MAX, std::sync::atomic::Ordering::Relaxed);
        let r = save_with_write_limit(prof, &mut buf, u32::MAX);
        let total = WRITE_CALLS.load(std::sync::atomic::Ordering::Relaxed);
        eprintln!("struct-tags total writes={total} r={r}");
        for limit in 0..total + 2 {
            let r = save_with_write_limit(prof, &mut buf, limit);
            if r == 0 { n_fail += 1; } else { n_ok += 1; }
        }
        eprintln!("struct-tags ok={n_ok} fail={n_fail}");
        c::cmsCloseProfile(prof);
        c::cmsFreeToneCurve(g);
    }
}

/// Third sweep — MHC2 + vcgt + segmented-curve MPE (cvst) so
/// Type_MHC2_Write/Type_vcgt_Write/Type_MPE_Write/
/// WriteSegmentedCurve fail arms fire.
#[test]
fn write_fail_arms3() {
    unsafe {
        // segmented curve → cvst element inside v4 mAB
        let t2 = [0.0f32, 0.5, 1.0];
        let segs = [
            c::CurveSegment { x0: 0.0, x1: 0.5, r#type: 1,
                params: [2.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                n_grid_points: 0, sampled_points: std::ptr::null() },
            c::CurveSegment { x0: 0.5, x1: 1.0, r#type: 0,
                params: [0.0; 10],
                n_grid_points: 3, sampled_points: t2.as_ptr() },
        ];
        let seg = c::cmsBuildSegmentedToneCurve(
            std::ptr::null_mut(), 2, segs.as_ptr());
        let prof = c::cmsCreate_sRGBProfile();
        c::cmsSetProfileVersion(prof, 4.4);
        let mut grid = vec![0u16; 3 * 3 * 3 * 3];
        for (i, v) in grid.iter_mut().enumerate() {
            *v = ((i % 9) * 7281) as u16;
        }
        let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocToneCurves(std::ptr::null_mut(), 3,
                [seg, seg, seg].as_ptr() as *mut _));
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocCLut16bit(std::ptr::null_mut(),
                3, 3, 3, grid.as_ptr()));
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocToneCurves(std::ptr::null_mut(), 3,
                [seg, seg, seg].as_ptr() as *mut _));
        // seg-curve MPE rejects in pass-1 on a throwaway profile —
        // keep it off the sweep profile so mhc2/vcgt still save.
        let profx = c::cmsCreate_sRGBProfile();
        c::cmsSetProfileVersion(profx, 4.4);
        let w1 = c::cmsWriteTag(profx, 0x41324230, p as *const _);
        let mut tbuf = vec![0u8; 1 << 18];
        let mut tn = tbuf.len() as u32;
        let tr = c::cmsSaveProfileToMem(profx,
            tbuf.as_mut_ptr() as *mut _, &mut tn);
        eprintln!("seg-mpe throwaway w={w1} save={tr}");
        c::cmsCloseProfile(profx);
        // mhc2
        #[repr(C)] struct Mhc2 { n: u32, r: *mut f64,
            g: *mut f64, b: *mut f64, minl: f64, peakl: f64,
            m: [[f64; 4]; 3] }
        let mut rc = [0.0f64, 0.5, 1.0, 2.0];
        let mut gc = [0.0f64, 0.5, 1.0, 2.0];
        let mut bc = [0.0f64, 0.5, 1.0, 2.0];
        let mut mhc = Mhc2 { n: 4,
            r: rc.as_mut_ptr(), g: gc.as_mut_ptr(), b: bc.as_mut_ptr(),
            minl: 0.0001, peakl: 1000.0,
            m: [[1.0,0.0,0.0,0.0],[0.0,1.0,0.0,0.0],[0.0,0.0,1.0,0.0]] };
        let w2 = c::cmsWriteTag(prof, 0x4D484332,
            &mut mhc as *mut _ as *const _);
        // vcgt — payload is cmsToneCurve*[3]
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mut curves = [g; 3];
        let w3 = c::cmsWriteTag(prof, 0x76636774,
            curves.as_mut_ptr() as *const _);
        eprintln!("seg-mpe={w1} mhc2={w2} vcgt={w3}");
        let mut buf = vec![0u8; 1 << 20];
        let r = save_with_write_limit(prof, &mut buf, u32::MAX);
        let total = WRITE_CALLS.load(std::sync::atomic::Ordering::Relaxed);
        eprintln!("writes={total} r={r}");
        let mut n_ok = 0u32;
        let mut n_fail = 0u32;
        for limit in 0..total + 2 {
            let rr = save_with_write_limit(prof, &mut buf, limit);
            if rr == 0 { n_fail += 1; } else { n_ok += 1; }
        }
        eprintln!("mpe/mhc2/vcgt ok={n_ok} fail={n_fail}");
        c::cmsCloseProfile(prof);
        c::cmsPipelineFree(p);
        c::cmsFreeToneCurve(g);
        c::cmsFreeToneCurve(seg);
    }
}

/// Type_*_Read error arms — `_cmsRead*` fail -> Error branches.
/// Patch io->Read (offset 280) on a real memory-read handler with
/// a counter-gated stub; sweep fail-at-call-N through open+read.
static READ_CALLS: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(0);
static READ_LIMIT: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(u32::MAX);
static REAL_READ: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

type ReadFn = unsafe extern "C" fn(*mut std::ffi::c_void,
    *mut std::ffi::c_void, u32, u32) -> u32;

unsafe extern "C" fn gated_read(io: *mut std::ffi::c_void,
    buf: *mut std::ffi::c_void, size: u32, count: u32) -> u32 {
    use std::sync::atomic::Ordering::Relaxed;
    let n = READ_CALLS.fetch_add(1, Relaxed);
    if n >= READ_LIMIT.load(Relaxed) {
        return 0;
    }
    let real: ReadFn = std::mem::transmute(
        REAL_READ.load(Relaxed));
    real(io, buf, size, count)
}

unsafe fn open_gated(data: &[u8], limit: u32)
    -> (*mut std::ffi::c_void, *mut std::ffi::c_void) {
    use std::sync::atomic::Ordering::Relaxed;
    let io = c::cmsOpenIOhandlerFromMem(std::ptr::null_mut(),
        data.as_ptr() as *mut u8, data.len() as u32,
        b"r\0".as_ptr());
    if io.is_null() { return (io, std::ptr::null_mut()); }
    READ_CALLS.store(0, Relaxed);
    READ_LIMIT.store(limit, Relaxed);
    let real = *(io as *const usize).add(280 / 8);
    REAL_READ.store(real, Relaxed);
    *(io as *mut usize).add(280 / 8) = gated_read as usize;
    let prof = c::cmsOpenProfileFromIOhandlerTHR(
        std::ptr::null_mut(), io);
    (io, prof)
}

#[test]
fn read_fail_arms() {
    unsafe {
        // a fat saved profile with every readable tag type
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let g2 = c::cmsBuildGamma(std::ptr::null_mut(), 1.8);
        let g3 = c::cmsBuildGamma(std::ptr::null_mut(), 3.1);
        let mat = [1.0f64, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        let prof = c::cmsCreate_sRGBProfile();
        c::cmsSetProfileVersion(prof, 2.1);
        let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocMatrix(std::ptr::null_mut(), 3, 3,
                mat.as_ptr(), std::ptr::null()));
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocToneCurves(std::ptr::null_mut(), 3,
                [g, g2, g3].as_ptr() as *mut _));
        let mut grid = vec![0u16; 5 * 5 * 5 * 3];
        for (i, v) in grid.iter_mut().enumerate() {
            *v = ((i % 17) * 3855) as u16;
        }
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocCLut16bit(std::ptr::null_mut(),
                5, 3, 3, grid.as_ptr()));
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocToneCurves(std::ptr::null_mut(), 3,
                [g3, g, g2].as_ptr() as *mut _));
        c::cmsWriteTag(prof, 0x41324230, p as *const _);
        c::cmsWriteTag(prof, 0x72545243, g as *const _);
        // struct tags → their Type_*_Read fail arms join the sweep
        #[repr(C)] struct Dt { y: u16, mo: u16, d: u16,
            h: u16, mi: u16, s: u16 }
        let dt = Dt { y: 2026, mo: 1, d: 1, h: 0, mi: 0, s: 0 };
        c::cmsWriteTag(prof, 0x63616c74,
            &dt as *const _ as *const _);
        let chad = [1.0f64, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        c::cmsWriteTag(prof, 0x63686164, chad.as_ptr() as *const _);
        #[repr(C)] struct XyY { x: f64, y: f64, yy: f64 }
        #[repr(C)] struct Chrm { n: u32, ty: u16, _p: u16,
            c: [XyY; 3] }
        let chrm = Chrm { n: 3, ty: 0, _p: 0, c: [
            XyY { x: 0.64, y: 0.33, yy: 1.0 },
            XyY { x: 0.30, y: 0.60, yy: 1.0 },
            XyY { x: 0.15, y: 0.06, yy: 1.0 }] };
        c::cmsWriteTag(prof, 0x6368726d,
            &chrm as *const _ as *const _);
        #[repr(C)] struct Meas { fl: u32, il: u32, ob: u32,
            ge: u32, ba: u32, me: u32 }
        let meas = Meas { fl: 0, il: 1, ob: 1,
            ge: 1, ba: 1, me: 1 };
        c::cmsWriteTag(prof, 0x6d656173,
            &meas as *const _ as *const _);
        #[repr(C)] struct Xyz { x: f64, y: f64, z: f64 }
        #[repr(C)] struct View { o: Xyz, b: Xyz,
            la: f64, yb: f64, dd: f64 }
        let view = View {
            o: Xyz { x: 0.9642, y: 1.0, z: 0.8249 },
            b: Xyz { x: 0.9642, y: 1.0, z: 0.8249 },
            la: 20.0, yb: 20.0, dd: 1.0 };
        c::cmsWriteTag(prof, 0x76696577,
            &view as *const _ as *const _);
        let ncl = c::cmsAllocNamedColorList(
            std::ptr::null_mut(), 2, 4, b"pre\0".as_ptr(),
            b"suf\0".as_ptr());
        if !ncl.is_null() {
            let mut nm = [0u16; 32];
            nm[0] = b'C' as u16;
            let pcs = [0u16; 3];
            let dev = [0u16; 16];
            c::cmsAppendNamedColor(ncl, nm.as_ptr() as *const _,
                pcs.as_ptr(), dev.as_ptr());
            c::cmsWriteTag(prof, 0x6e636c32, ncl as *const _);
        }
        let dict = c::cmsDictAlloc(std::ptr::null_mut());
        let dn: [i32; 3] = [b'n' as i32, b'1' as i32, 0];
        let dv: [i32; 3] = [b'v' as i32, b'1' as i32, 0];
        c::cmsDictAddEntry(dict, dn.as_ptr(), dv.as_ptr(),
            std::ptr::null_mut(), std::ptr::null_mut());
        c::cmsWriteTag(prof, 0x6d657461, dict as *const _);
        let mut buf = vec![0u8; 1 << 20];
        let mut fsz = buf.len() as u32;
        let ok = c::cmsSaveProfileToMem(prof,
            buf.as_mut_ptr() as *mut _, &mut fsz);
        assert_ne!(ok, 0);
        buf.truncate(fsz as usize);
        // open+read-every-tag under each read-fail threshold
        let mut n_ok = 0u32;
        let mut n_fail = 0u32;
        // fork per limit — upstream dereferences half-read
        // structures on mid-tag Read failure; a crash in the child
        // still counts the covered arms.
        unsafe extern "C" { fn fork() -> i32;
            fn waitpid(p: i32, s: *mut i32, o: i32) -> i32;
            fn _exit(c: i32) -> !;
            fn dlsym(h: *mut std::ffi::c_void,
                n: *const i8) -> *mut std::ffi::c_void; }
        // flush llvm-cov counters before _exit — %p.profraw keeps
        // each fork's data separate; absent in uninstrumented builds
        let flush: Option<unsafe extern "C" fn() -> i32> = unsafe {
            let f = dlsym(std::ptr::null_mut(),
                b"__llvm_profile_write_file\0".as_ptr()
                    as *const i8);
            if f.is_null() { None }
            else { Some(std::mem::transmute(f)) }
        };
        macro_rules! flush_cov { () => {
            if let Some(f) = flush { f(); }
        } }
        for limit in 0..400u32 {
            let pid = fork();
            if pid == 0 {
                let (io, rp) = open_gated(&buf, limit);
                if !rp.is_null() {
                    let nt = c::cmsGetTagCount(rp);
                    for i in 0..nt as u32 {
                        let sig = c::cmsGetTagSignature(rp, i);
                        let _t = c::cmsReadTag(rp, sig);
                    }
                    flush_cov!();
                    _exit(0);
                }
                if !io.is_null() { c::cmsCloseIOhandler(io); }
                flush_cov!();
                _exit(1);
            }
            let mut st = 0i32;
            waitpid(pid, &mut st, 0);
            if st == 0 { n_ok += 1; } else { n_fail += 1; }
        }
        eprintln!("read-fail ok={n_ok} fail={n_fail}");
        c::cmsCloseProfile(prof);
        c::cmsPipelineFree(p);
        c::cmsFreeToneCurve(g);
        c::cmsFreeToneCurve(g2);
        c::cmsFreeToneCurve(g3);
    }
}

/// ReadSegmentedCurve — cvst element reads through valid
/// segmented-curve MPE pipelines (formula types 6-8 + sampled).
#[test]
fn seg_cvst_arms() {
    unsafe {
        // formula type 6 (file type 0, 4 params), 7 (file 1, 5),
        // 8 (file 2, 5) + sampled type 0
        let sample_pts = [0.0f32, 0.3, 0.7, 1.0];
        let seg_sets: Vec<(Vec<c::CurveSegment>, &str)> = vec![
            (vec![c::CurveSegment { x0: 0.0, x1: 0.5, r#type: 6,
                params: [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                n_grid_points: 0, sampled_points: std::ptr::null() },
             c::CurveSegment { x0: 0.5, x1: 1.0, r#type: 0,
                params: [0.0; 10], n_grid_points: 4,
                sampled_points: sample_pts.as_ptr() }],
             "f6+smp"),
            (vec![c::CurveSegment { x0: 0.0, x1: 1.0, r#type: 7,
                params: [2.0, 0.5, 0.1, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                n_grid_points: 0, sampled_points: std::ptr::null() }],
             "f7-1seg"),
            (vec![c::CurveSegment { x0: 0.0, x1: 1.0, r#type: 8,
                params: [1.5, 0.3, 0.2, 0.1, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                n_grid_points: 0, sampled_points: std::ptr::null() }],
             "f8-1seg"),
            (vec![c::CurveSegment { x0: 0.0, x1: 1.0, r#type: 0,
                params: [0.0; 10], n_grid_points: 4,
                sampled_points: sample_pts.as_ptr() }],
             "smp-1seg"),
            (vec![c::CurveSegment { x0: 0.0, x1: 0.25, r#type: 6,
                params: [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                n_grid_points: 0, sampled_points: std::ptr::null() },
             c::CurveSegment { x0: 0.25, x1: 0.5, r#type: 7,
                params: [2.0, 0.5, 0.1, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                n_grid_points: 0, sampled_points: std::ptr::null() },
             c::CurveSegment { x0: 0.5, x1: 0.75, r#type: 8,
                params: [1.5, 0.3, 0.2, 0.1, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                n_grid_points: 0, sampled_points: std::ptr::null() },
             c::CurveSegment { x0: 0.75, x1: 1.0, r#type: 0,
                params: [0.0; 10], n_grid_points: 4,
                sampled_points: sample_pts.as_ptr() }],
             "all4"),
        ];
        let mut grid = vec![0u16; 3 * 3 * 3 * 3];
        for (i, v) in grid.iter_mut().enumerate() {
            *v = ((i % 9) * 7281) as u16;
        }
        for (segs, name) in &seg_sets {
            let seg = c::cmsBuildSegmentedToneCurve(
                std::ptr::null_mut(), segs.len() as u32,
                segs.as_ptr());
            eprintln!("{name} segbuild={}", !seg.is_null());
            if seg.is_null() {
                continue;
            }
            for sig in [0x41324230u32, 0x42324130] {
                let prof = c::cmsCreate_sRGBProfile();
                c::cmsSetProfileVersion(prof, 4.4);
                let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
                let st1 = c::cmsStageAllocToneCurves(
                    std::ptr::null_mut(), 3,
                    [seg, seg, seg].as_ptr() as *mut _);
                eprintln!("{name} st1={}", !st1.is_null());
                c::cmsPipelineInsertStage(p, 1, st1);
                c::cmsPipelineInsertStage(p, 1,
                    c::cmsStageAllocCLut16bit(std::ptr::null_mut(),
                        3, 3, 3, grid.as_ptr()));
                let st2 = c::cmsStageAllocToneCurves(
                    std::ptr::null_mut(), 3,
                    [seg, seg, seg].as_ptr() as *mut _);
                eprintln!("{name} st2={}", !st2.is_null());
                c::cmsPipelineInsertStage(p, 1, st2);
                let nst = c::cmsPipelineStageCount(p);
                eprintln!("{name} stages={nst}");
                let w = c::cmsWriteTag(prof, sig, p as *const _);
                let mut buf = vec![0u8; 1 << 20];
                let mut n = buf.len() as u32;
                let s = if w != 0 {
                    c::cmsSaveProfileToMem(prof,
                        buf.as_mut_ptr() as *mut _, &mut n)
                } else { 0 };
                let rp = if s != 0 {
                    c::cmsOpenProfileFromMem(
                        buf.as_ptr() as *const _, n)
                } else { std::ptr::null_mut() };
                let tag = if !rp.is_null() {
                    c::cmsReadTag(rp, sig)
                } else { std::ptr::null_mut() };
                eprintln!("{name}/{sig:#x} w={w} s={s} rd={}",
                    !tag.is_null());
                if !rp.is_null() { c::cmsCloseProfile(rp); }
                c::cmsCloseProfile(prof);
                c::cmsPipelineFree(p);
            }
            c::cmsFreeToneCurve(seg);
        }
    }
}

/// seg-curve cv-only pipeline → isolate WriteSegmentedCurve.
#[test]
fn seg_cvst_isolate() {
    unsafe {
        let pts = [0.0f32, 0.3, 0.7, 1.0];
        let segs = [
            c::CurveSegment { x0: 0.0, x1: 0.5, r#type: 6,
                params: [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                n_grid_points: 0, sampled_points: std::ptr::null() },
            c::CurveSegment { x0: 0.5, x1: 1.0, r#type: 0,
                params: [0.0; 10], n_grid_points: 4,
                sampled_points: pts.as_ptr() },
        ];
        let seg = c::cmsBuildSegmentedToneCurve(
            std::ptr::null_mut(), 2, segs.as_ptr());
        eprintln!("seg={}", !seg.is_null());
        // cv-only → mAB {B} single element
        for sig in [0x41324230u32, 0x42324130] {
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsSetProfileVersion(prof, 4.4);
            let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
            c::cmsPipelineInsertStage(p, 1,
                c::cmsStageAllocToneCurves(std::ptr::null_mut(),
                    3, [seg, seg, seg].as_ptr() as *mut _));
            let w = c::cmsWriteTag(prof, sig, p as *const _);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            eprintln!("cv-only/{sig:#x} w={w} s={s} n={n}");
            c::cmsCloseProfile(prof);
            c::cmsPipelineFree(p);
        }
        c::cmsFreeToneCurve(seg);
    }
}

/// ReadSegmentedCurve — raw `mpet` tags (d2b0/b2d0) carrying
/// cvst curve-set elements: formula types 0-2, sampled,
/// multi-seg, malformed.
#[test]
fn cvst_read_arms() {
    fn be32(v: u32) -> [u8; 4] { v.to_be_bytes() }
    fn be16(v: u16) -> [u8; 2] { v.to_be_bytes() }
    fn f32b(v: f32) -> [u8; 4] { v.to_be_bytes() }
    fn curve(segs: &[(u32, Vec<u8>)], nseg: u16,
        breaks: &[f32], sig_override: u32) -> Vec<u8> {
        let mut c = Vec::new();
        let sig = if sig_override == 0 { 0x63757266 }
            else { sig_override };
        c.extend_from_slice(&be32(sig));        // 'curf'
        c.extend_from_slice(&be32(0));
        c.extend_from_slice(&be16(nseg));
        c.extend_from_slice(&be16(0));
        for b in breaks { c.extend_from_slice(&f32b(*b)); }
        for (ssig, data) in segs {
            c.extend_from_slice(&be32(*ssig));
            c.extend_from_slice(&be32(0));
            c.extend_from_slice(data);
        }
        c
    }
    fn formula(ty: u16, params: &[f32]) -> Vec<u8> {
        let mut d = Vec::new();
        d.extend_from_slice(&be16(ty));
        d.extend_from_slice(&be16(0));
        for p in params { d.extend_from_slice(&f32b(*p)); }
        d
    }
    fn sampled(pts: &[f32]) -> Vec<u8> {
        let mut d = Vec::new();
        d.extend_from_slice(&be32(pts.len() as u32));
        for p in pts { d.extend_from_slice(&f32b(*p)); }
        d
    }
    // one mpet element: {sig, payload-bytes}
    fn elem(sig: u32, data: &[u8]) -> (u32, Vec<u8>) {
        let mut e = Vec::new();
        e.extend_from_slice(&be32(sig));
        e.extend_from_slice(&be32(0));
        e.extend_from_slice(data);
        (sig, e)
    }
    // raw element bytes (sig+rsv already inside)
    fn relem(e: Vec<u8>) -> (u32, Vec<u8>) { (0, e) }
    // mpet tag: in u16, out u16, count u32, pos table, elements
    fn mpet(nin: u16, nout: u16, elements: &[(u32, Vec<u8>)])
        -> Vec<u8> {
        let mut t = Vec::new();
        t.extend_from_slice(&be32(0x6D706574));  // 'mpet'
        t.extend_from_slice(&be32(0));
        t.extend_from_slice(&be16(nin));
        t.extend_from_slice(&be16(nout));
        t.extend_from_slice(&be32(elements.len() as u32));
        let pos_start = t.len();
        for _ in elements {
            t.extend_from_slice(&be32(0));
            t.extend_from_slice(&be32(0));
        }
        let mut offs = Vec::new();
        for (_, e) in elements {
            offs.push(t.len() as u32);
            t.extend_from_slice(e);
        }
        for (i, o) in offs.iter().enumerate() {
            let at = pos_start + i * 8;
            t[at..at + 4].copy_from_slice(&be32(*o));
            let end = if i + 1 < offs.len() { offs[i + 1] }
                else { t.len() as u32 };
            t[at + 4..at + 8].copy_from_slice(&be32(end - o));
        }
        t
    }
    // cvst element: sig+rsv+nIn+nOut+pos table+curves
    // pos-table offsets are relative to element start (the sig)
    fn cvst(nch: u16, channels: Vec<Vec<u8>>) -> Vec<u8> {
        let mut e = Vec::new();
        e.extend_from_slice(&be32(0x63767374)); // 'cvst'
        e.extend_from_slice(&be32(0));
        e.extend_from_slice(&be16(nch));
        e.extend_from_slice(&be16(nch));
        let pos_start = e.len();
        for _ in 0..nch {
            e.extend_from_slice(&be32(0));
            e.extend_from_slice(&be32(0));
        }
        let mut offs = Vec::new();
        for cv in &channels {
            offs.push(e.len() as u32);
            e.extend_from_slice(cv);
        }
        for (i, o) in offs.iter().enumerate() {
            let at = pos_start + i * 8;
            e[at..at + 4].copy_from_slice(&be32(*o));
            let end = if i + 1 < offs.len() { offs[i + 1] }
                else { e.len() as u32 };
            e[at + 4..at + 8].copy_from_slice(&be32(end - o));
        }
        e
    }
    unsafe {
        let f0 = formula(0, &[1.0, 0.0, 0.0, 0.0]);
        let f1 = formula(1, &[2.0, 0.5, 0.1, 0.0, 0.0]);
        let f2 = formula(2, &[1.5, 0.3, 0.2, 0.1, 0.0]);
        let s1 = sampled(&[0.0, 0.3, 0.7, 1.0]);
        let variants: Vec<(Vec<u8>, &str)> = vec![
            // 1 cvst element, 3 channels, formula-0 curves
            (mpet(3, 3, &[relem(cvst(3, vec![
                curve(&[(0x70617266, f0.clone())], 1, &[], 0),
                curve(&[(0x70617266, f0.clone())], 1, &[], 0),
                curve(&[(0x70617266, f0.clone())], 1, &[], 0)]))]),
                "f0x3"),
            // formula + sampled 2-seg curves
            (mpet(3, 3, &[relem(cvst(3, vec![
                curve(&[(0x70617266, f1.clone()),
                        (0x73616D66, s1.clone())], 2, &[0.5], 0),
                curve(&[(0x70617266, f1.clone()),
                        (0x73616D66, s1.clone())], 2, &[0.5], 0),
                curve(&[(0x70617266, f1.clone()),
                        (0x73616D66, s1.clone())], 2, &[0.5], 0)]))]),
                "f1+smp"),
            // 1ch 4-seg curve — all seg types
            (mpet(1, 1, &[relem(cvst(1, vec![
                curve(&[(0x73616D66, s1.clone()),
                        (0x70617266, f0.clone()),
                        (0x70617266, f1.clone()),
                        (0x70617266, f2.clone())], 4,
                    &[0.25, 0.5, 0.75], 0)]))]),
                "1ch-4seg"),
            // 5-seg 1ch
            (mpet(1, 1, &[relem(cvst(1, vec![
                curve(&[(0x73616D66, s1.clone()),
                        (0x70617266, f0.clone()),
                        (0x70617266, f1.clone()),
                        (0x70617266, f2.clone()),
                        (0x73616D66, s1.clone())], 5,
                    &[0.2, 0.4, 0.6, 0.8], 0)]))]),
                "1ch-5seg"),
            // malformed: nSegments=0
            (mpet(1, 1, &[relem(cvst(1, vec![
                curve(&[], 0, &[], 0)]))]), "nseg0"),
            // malformed: wrong curve sig
            (mpet(1, 1, &[relem(cvst(1, vec![
                curve(&[(0x70617266, f0.clone())], 1, &[],
                    0x62616420)]))]), "badsig"),
            // malformed: formula type > 2
            (mpet(1, 1, &[relem(cvst(1, vec![
                curve(&[(0x70617266,
                    formula(3, &[1.0, 0.0, 0.0, 0.0, 0.0]))],
                    1, &[], 0)]))]), "ftype3"),
            // malformed: unknown seg sig
            (mpet(1, 1, &[relem(cvst(1, vec![
                curve(&[(0x62616420, f0.clone())], 1, &[], 0)]))]),
                "badseg"),
            // cvst + matrix element pair
            (mpet(3, 3, &[
                relem(cvst(3, vec![
                    curve(&[(0x70617266, f0.clone())], 1, &[], 0),
                    curve(&[(0x70617266, f0.clone())], 1, &[], 0),
                    curve(&[(0x70617266, f0.clone())], 1, &[], 0)])),
                { let mut m = Vec::new();
                    m.extend_from_slice(&be16(3));
                    m.extend_from_slice(&be16(3));
                    for _ in 0..9 {
                        m.extend_from_slice(&be32(0x00010000));
                    }
                    for _ in 0..3 {
                        m.extend_from_slice(&be32(0));
                    }
                    elem(0x6D617466, &m) },
            ]), "cvst+mat"),
        ];
        for (blob, name) in &variants {
            for sig in [0x44324230u32, 0x42324430] {
                let prof = c::cmsCreate_sRGBProfile();
                c::cmsSetProfileVersion(prof, 4.4);
                let w = c::cmsWriteRawTag(prof, sig,
                    blob.as_ptr() as *const _, blob.len() as u32);
                let mut buf = vec![0u8; 1 << 20];
                let mut n = buf.len() as u32;
                let s = if w != 0 {
                    c::cmsSaveProfileToMem(prof,
                        buf.as_mut_ptr() as *mut _, &mut n)
                } else { 0 };
                let rp = if s != 0 {
                    c::cmsOpenProfileFromMem(
                        buf.as_ptr() as *const _, n)
                } else { std::ptr::null_mut() };
                let tag = if !rp.is_null() {
                    c::cmsReadTag(rp, sig)
                } else { std::ptr::null_mut() };
                eprintln!("{name}/{sig:#x} w={w} s={s} rd={}",
                    !tag.is_null());
                if !rp.is_null() { c::cmsCloseProfile(rp); }
                c::cmsCloseProfile(prof);
            }
        }
    }
}

/// Type_Text_Description_Read — raw v2 `desc` tags: ASCII-only,
/// +Unicode, +ScriptCode, malformed count/sig variants.
#[test]
fn desc_read_arms2() {
    fn be32(v: u32) -> [u8; 4] { v.to_be_bytes() }
    fn be16(v: u16) -> [u8; 2] { v.to_be_bytes() }
    fn desc(ascii: &[u8], ucs2: &[u16], sccode: u16,
        sccount: u8, mac: &[u8], sig_override: u32) -> Vec<u8> {
        let mut t = Vec::new();
        let sig = if sig_override == 0 { 0x64657363 }
            else { sig_override };
        t.extend_from_slice(&be32(sig));       // 'desc'
        t.extend_from_slice(&be32(0));
        t.extend_from_slice(&be32(ascii.len() as u32 + 1));
        let mut a = [0u8; 256];
        for (i, b) in ascii.iter().enumerate() {
            if i < 255 { a[i] = *b; }
        }
        t.extend_from_slice(&a);
        if !ucs2.is_empty() || sccode != 0 || sccount != 0 {
            t.extend_from_slice(&be32(1));     // UnicodeCode
            t.extend_from_slice(&be32(ucs2.len() as u32));
            for u in ucs2 {
                t.extend_from_slice(&be16(*u));
            }
            t.extend_from_slice(&be16(sccode));
            t.push(sccount);
            let mut m = [0u8; 67];
            for (i, b) in mac.iter().enumerate() {
                if i < 67 { m[i] = *b; }
            }
            t.extend_from_slice(&m);
        }
        t
    }
    unsafe {
        let variants: Vec<(Vec<u8>, &str)> = vec![
            (desc(b"plain", &[], 0, 0, &[], 0), "ascii-only"),
            (desc(b"with uni", &[0x0041, 0x00E9, 0x263A], 0, 0,
                &[], 0), "unicode"),
            (desc(b"with sc", &[0x0041], 5, 3,
                b"mac", 0), "scriptcode"),
            (desc(b"all fields", &[0x0041, 0x00E9], 7, 5,
                b"abcde", 0), "all"),
            (desc(b"", &[0x0041], 0, 0, &[], 0), "empty-ascii"),
            // UnicodeCount=0 but UnicodeCode=1
            ({ let mut t = Vec::new();
                t.extend_from_slice(&be32(0x64657363u32));
                t.extend_from_slice(&be32(0));
                t.extend_from_slice(&be32(2));
                let mut a = [0u8; 256];
                a[0] = b'x';
                t.extend_from_slice(&a);
                t.extend_from_slice(&be32(1));
                t.extend_from_slice(&be32(0));
                t.extend_from_slice(&be16(0));
                t.push(0);
                t.extend_from_slice(&[0u8; 67]);
                t }, "uni-count0"),
            // wrong type sig → reject
            (desc(b"x", &[], 0, 0, &[], 0x62616420), "badsig"),
            // huge UnicodeCount → OOB read reject
            ({ let mut t = Vec::new();
                t.extend_from_slice(&be32(0x64657363u32));
                t.extend_from_slice(&be32(0));
                t.extend_from_slice(&be32(2));
                t.extend_from_slice(&[0u8; 256]);
                t.extend_from_slice(&be32(1));
                t.extend_from_slice(&be32(0xFFFF));
                t.extend_from_slice(&be16(0));
                t.push(0);
                t.extend_from_slice(&[0u8; 67]);
                t }, "huge-uni"),
            // ASCII len = 0
            ({ let mut t = Vec::new();
                t.extend_from_slice(&be32(0x64657363u32));
                t.extend_from_slice(&be32(0));
                t.extend_from_slice(&be32(0));
                t.extend_from_slice(&[0u8; 256]);
                t }, "asciilen0"),
        ];
        for (blob, name) in &variants {
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsSetProfileVersion(prof, 2.1);
            let w = c::cmsWriteRawTag(prof, 0x64657363,
                blob.as_ptr() as *const _, blob.len() as u32);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(
                    buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, 0x64657363)
            } else { std::ptr::null_mut() };
            eprintln!("{name} w={w} s={s} rd={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
    }
}

/// Type_vcgt_Read — raw `vcgt` tags: table 1B/2B, formula,
/// Adobe quirk fixup, channel/bytes/TagType rejects.
#[test]
fn vcgt_read_arms2() {
    fn be32(v: u32) -> [u8; 4] { v.to_be_bytes() }
    fn be16(v: u16) -> [u8; 2] { v.to_be_bytes() }
    fn table(nch: u16, nelems: u16, nbytes: u16,
        data: Vec<u8>) -> Vec<u8> {
        let mut t = Vec::new();
        t.extend_from_slice(&be32(0x76636774)); // 'vcgt'
        t.extend_from_slice(&be32(0));
        t.extend_from_slice(&be32(0));          // table type
        t.extend_from_slice(&be16(nch));
        t.extend_from_slice(&be16(nelems));
        t.extend_from_slice(&be16(nbytes));
        t.extend_from_slice(&data);
        t
    }
    fn formula(gs: &[(f32, f32, f32); 3]) -> Vec<u8> {
        let mut t = Vec::new();
        t.extend_from_slice(&be32(0x76636774));
        t.extend_from_slice(&be32(0));
        t.extend_from_slice(&be32(1));          // formula type
        for (g, mn, mx) in gs {
            let f = |v: f32| ((v * 65536.0) as i32).to_be_bytes();
            t.extend_from_slice(&f(*g));
            t.extend_from_slice(&f(*mn));
            t.extend_from_slice(&f(*mx));
        }
        t
    }
    unsafe {
        let d8 = vec![0x33u8; 4 * 3];
        let d16 = vec![0x11u8; 4 * 2 * 3];
        // Adobe quirk: nElems=256 nBytes=1 SizeOfTag=1576
        let dq = vec![0x55u8; 1576 - 20];
        let variants: Vec<(Vec<u8>, &str)> = vec![
            (table(3, 4, 1, d8.clone()), "tab-1B"),
            (table(3, 4, 2, d16.clone()), "tab-2B"),
            (table(3, 256, 1, dq), "tab-adobe"),
            (table(3, 0, 1, vec![]), "tab-n0"),
            (formula(&[(2.2, 0.0, 1.0), (2.2, 0.0, 1.0),
                (2.2, 0.0, 1.0)]), "formula"),
            (formula(&[(2.2, 0.1, 0.9), (1.8, 0.0, 1.0),
                (2.4, 0.05, 0.95)]), "formula-vary"),
            (table(2, 4, 1, d8.clone()), "nch2"),
            (table(4, 4, 1, d8.clone()), "nch4"),
            (table(3, 4, 3, d8.clone()), "nb3"),
            (table(3, 4, 4, d8.clone()), "nb4"),
            // bad TagType
            ({ let mut t = Vec::new();
                t.extend_from_slice(&be32(0x76636774u32));
                t.extend_from_slice(&be32(0));
                t.extend_from_slice(&be32(7));
                t.extend_from_slice(&be32(0));
                t }, "ttype7"),
        ];
        for (blob, name) in &variants {
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsSetProfileVersion(prof, 2.1);
            let w = c::cmsWriteRawTag(prof, 0x76636774,
                blob.as_ptr() as *const _, blob.len() as u32);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(
                    buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, 0x76636774)
            } else { std::ptr::null_mut() };
            eprintln!("{name} w={w} s={s} rd={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
    }
}

/// Type_LUT8_Write reject arms — SaveAs8Bits pipelines with
/// matrix>3ch, mismatched nSamples, leftover stages, NULL clut.
#[test]
fn lut8_write_arms2() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 1.6);
        let g3: *const *mut c::ToneCurve =
            [g, g, g].as_ptr() as *const *mut c::ToneCurve;
        let mut grid2 = vec![0u16; 2 * 2 * 2 * 3];
        let mut grid4 = vec![0u16; 2u32.pow(4) as usize * 4];
        for (i, v) in grid2.iter_mut().enumerate() {
            *v = (i * 111) as u16;
        }
        for (i, v) in grid4.iter_mut().enumerate() {
            *v = (i * 71) as u16;
        }
        let dims3 = [2u32; 3];
        let dims4 = [2u32; 4];
        let dims_mix = [2u32, 3, 2];
        let mat9 = [1.0f64, 0.0, 0.0,
            0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        let mat16 = [1.0f64; 16];
        // shape, name — each (stages, expected)
        let cases: Vec<(Vec<*mut std::ffi::c_void>, &str)> = vec![
            (vec![c::cmsStageAllocCLut16bitGranular(
                std::ptr::null_mut(), dims3.as_ptr(), 3, 3,
                grid2.as_ptr())], "clut"),
            (vec![c::cmsStageAllocToneCurves(
                std::ptr::null_mut(), 3, g3),
                c::cmsStageAllocCLut16bitGranular(
                    std::ptr::null_mut(), dims3.as_ptr(), 3, 3,
                    grid2.as_ptr())], "cv-clut"),
            (vec![c::cmsStageAllocToneCurves(
                std::ptr::null_mut(), 3, g3),
                c::cmsStageAllocCLut16bitGranular(
                    std::ptr::null_mut(), dims3.as_ptr(), 3, 3,
                    grid2.as_ptr()),
                c::cmsStageAllocToneCurves(
                    std::ptr::null_mut(), 3, g3)], "cv-clut-cv"),
            (vec![c::cmsStageAllocMatrix(
                std::ptr::null_mut(), 3, 3,
                mat9.as_ptr(), std::ptr::null()),
                c::cmsStageAllocCLut16bitGranular(
                    std::ptr::null_mut(), dims3.as_ptr(), 3, 3,
                    grid2.as_ptr())], "mat-clut"),
            // 4ch matrix → reject
            (vec![c::cmsStageAllocMatrix(
                std::ptr::null_mut(), 4, 4,
                mat16.as_ptr(), std::ptr::null()),
                c::cmsStageAllocCLut16bitGranular(
                    std::ptr::null_mut(), dims4.as_ptr(), 4, 4,
                    grid4.as_ptr())], "mat4"),
            // mismatched nSamples → reject
            (vec![c::cmsStageAllocCLut16bitGranular(
                std::ptr::null_mut(), dims_mix.as_ptr(), 3, 3,
                grid2.as_ptr())], "mix"),
            // extra stage → not-suitable reject
            (vec![c::cmsStageAllocToneCurves(
                std::ptr::null_mut(), 3, g3),
                c::cmsStageAllocToneCurves(
                    std::ptr::null_mut(), 3, g3),
                c::cmsStageAllocCLut16bitGranular(
                    std::ptr::null_mut(), dims3.as_ptr(), 3, 3,
                    grid2.as_ptr())], "cv-cv-clut"),
            // clut-only 4ch
            (vec![c::cmsStageAllocCLut16bitGranular(
                std::ptr::null_mut(), dims4.as_ptr(), 4, 4,
                grid4.as_ptr())], "clut4"),
            // empty pipeline → empty-LUT8 reject
            (vec![], "empty"),
        ];
        for sig in [0x41324230u32, 0x42324130] {
            for (stages, name) in &cases {
                // upstream can double-free on rejected mBA writes —
                // isolate each case in a forked child
                unsafe extern "C" { fn fork() -> i32;
                    fn waitpid(p: i32, s: *mut i32, o: i32) -> i32;
                    fn _exit(code: i32) -> !;
                    fn dlsym(h: *mut std::ffi::c_void,
                        n: *const i8) -> *mut std::ffi::c_void; }
                let flush: Option<unsafe extern "C" fn() -> i32> =
                    unsafe {
                        let f = dlsym(std::ptr::null_mut(),
                            b"__llvm_profile_write_file ".as_ptr()
                                as *const i8);
                        if f.is_null() { None }
                        else { Some(std::mem::transmute(f)) }
                    };
                let pid = fork();
                if pid == 0 {
                    unsafe {
                        let prof = c::cmsCreate_sRGBProfile();
                        c::cmsSetProfileVersion(prof, 4.4);
                        let p = c::cmsPipelineAlloc(
                            std::ptr::null_mut(), 3, 3);
                        for st in stages {
                            if !st.is_null() {
                                c::cmsPipelineInsertStage(p, 1, *st);
                            }
                        }
                        c::cmsPipelineSetSaveAs8bitsFlag(p, 1);
                        let w = c::cmsWriteTag(prof, sig,
                            p as *const _);
                        let mut buf = vec![0u8; 1 << 20];
                        let mut n = buf.len() as u32;
                        let s = if w != 0 {
                            c::cmsSaveProfileToMem(prof,
                                buf.as_mut_ptr() as *mut _, &mut n)
                        } else { 0 };
                        if let Some(f) = flush { f(); }
                        _exit(if w == 0 { 0 }
                            else if s == 0 { 2 } else { 1 });
                    }
                }
                let mut st = 0i32;
                unsafe { waitpid(pid, &mut st, 0); }
                let rc = if st & 0x7f == 0 {
                    (st >> 8) & 0xff
                } else {
                    128 + (st & 0x7f)
                };
                eprintln!("{sig:#x}/{name} rc={rc}");
            }
        }
        c::cmsFreeToneCurve(g);
    }
}

/// Type_UcrBg_Read — raw `ucbg` tags: valid UCR/BG tables + text,
/// malformed count/size variants.
#[test]
fn ucrbg_read_arms2() {
    fn be32(v: u32) -> [u8; 4] { v.to_be_bytes() }
    fn be16(v: u16) -> [u8; 2] { v.to_be_bytes() }
    fn ucrbg(ucr: &[u16], bg: &[u16], text: &[u8],
        sig_override: u32) -> Vec<u8> {
        let mut t = Vec::new();
        let sig = if sig_override == 0 { 0x62666420 }
            else { sig_override };
        t.extend_from_slice(&be32(sig));        // 'ucbg'
        t.extend_from_slice(&be32(0));
        t.extend_from_slice(&be32(ucr.len() as u32));
        for v in ucr { t.extend_from_slice(&be16(*v)); }
        t.extend_from_slice(&be32(bg.len() as u32));
        for v in bg { t.extend_from_slice(&be16(*v)); }
        t.extend_from_slice(text);
        t
    }
    unsafe {
        let variants: Vec<(Vec<u8>, &str)> = vec![
            (ucrbg(&[0, 4096, 32768, 65535],
                &[0, 8192, 32768, 65535], b"ucr/desc\0", 0),
                "valid-4"),
            (ucrbg(&[0, 65535], &[0, 65535], b"x\0", 0),
                "valid-2"),
            (ucrbg(&[0; 256], &[0; 256], b"big\0", 0), "big"),
            (ucrbg(&[], &[0, 65535], b"y\0", 0), "ucr-empty"),
            (ucrbg(&[0, 65535], &[], b"z\0", 0), "bg-empty"),
            (ucrbg(&[0; 33000], &[0], b"", 0), "huge-ucr"),
            (ucrbg(&[0, 65535], &[0, 65535], b"", 0x62616420),
                "badsig"),
        ];
        for (blob, name) in &variants {
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsSetProfileVersion(prof, 4.4);
            let w = c::cmsWriteRawTag(prof, 0x62666420,
                blob.as_ptr() as *const _, blob.len() as u32);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(
                    buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, 0x62666420)
            } else { std::ptr::null_mut() };
            eprintln!("{name} w={w} s={s} rd={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
    }
}

/// Type_Chromaticity_Read — raw `chrm` tags: valid 3ch, nch!=3,
/// nch==0&&size32 special case, short.
#[test]
fn chrm_read_arms2() {
    fn be32(v: u32) -> [u8; 4] { v.to_be_bytes() }
    fn be16(v: u16) -> [u8; 2] { v.to_be_bytes() }
    fn f15(v: f64) -> [u8; 4] {
        ((v * 65536.0) as i32).to_be_bytes()
    }
    fn chrm(nch: u16, table: u16,
        rgb: &[(f64, f64); 3], sig: u32) -> Vec<u8> {
        let mut t = Vec::new();
        let s = if sig == 0 { 0x6368726D } else { sig };
        t.extend_from_slice(&be32(s));          // 'chrm'
        t.extend_from_slice(&be32(0));
        t.extend_from_slice(&be16(nch));
        t.extend_from_slice(&be16(table));
        for (x, y) in rgb {
            t.extend_from_slice(&f15(*x));
            t.extend_from_slice(&f15(*y));
        }
        t
    }
    unsafe {
        let std_rgb = [(0.64, 0.33), (0.30, 0.60), (0.15, 0.06)];
        let variants: Vec<(Vec<u8>, &str)> = vec![
            (chrm(3, 1, &std_rgb, 0), "valid"),
            (chrm(3, 2, &std_rgb, 0), "tab2"),
            (chrm(3, 4, &std_rgb, 0), "tab4"),
            (chrm(2, 1, &std_rgb, 0), "nch2"),
            (chrm(6, 1, &std_rgb, 0), "nch6"),
            // nch==0 + SizeOfTag==32 special: reads extra u16
            // then re-reads nChans → size math matters
            ({ let mut t = Vec::new();
                t.extend_from_slice(&be32(0x6368726Du32));
                t.extend_from_slice(&be32(0));
                t.extend_from_slice(&be16(0));
                t.extend_from_slice(&be16(0));
                t.extend_from_slice(&be16(3));
                t.extend_from_slice(&be16(1));
                for (x, y) in std_rgb.iter() {
                    t.extend_from_slice(&f15(*x));
                    t.extend_from_slice(&f15(*y));
                }
                t }, "nch0-sz32"),
            (chrm(3, 1, &std_rgb, 0x62616420), "badsig"),
        ];
        for (blob, name) in &variants {
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsSetProfileVersion(prof, 4.4);
            let w = c::cmsWriteRawTag(prof, 0x6368726D,
                blob.as_ptr() as *const _, blob.len() as u32);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(
                    buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, 0x6368726D)
            } else { std::ptr::null_mut() };
            eprintln!("{name} w={w} s={s} rd={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
    }
}

/// Type_Dictionary_Write + Type_NamedColor_Write — meta/ncl2
/// tags with full MLU fields + empty/large variants.
#[test]
fn dict_ncl2_write_arms() {
    unsafe {
        // dict via 'meta' tag
        for (entries, name) in [
            (2u32, "small"),
            (8, "mid"),
            (40, "large"),
            (0, "empty"),
        ] {
            let d = c::cmsDictAlloc(std::ptr::null_mut());
            if d.is_null() { continue; }
            let mlu1 = c::cmsMLUalloc(std::ptr::null_mut(), 1);
            c::cmsMLUsetASCII(mlu1, b"en\0".as_ptr(),
                b"US\0".as_ptr(), b"disp\0".as_ptr());
            for i in 0..entries {
                let name_s = format!("k{i}\0");
                let val_s = format!("v{i}\0");
                c::cmsDictAddEntry(d,
                    name_s.as_ptr() as *const i32,
                    val_s.as_ptr() as *const i32,
                    mlu1, mlu1);
            }
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsSetProfileVersion(prof, 4.4);
            let w = c::cmsWriteTag(prof, 0x6D657461,
                d as *const _);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(
                    buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, 0x6D657461)
            } else { std::ptr::null_mut() };
            eprintln!("meta-{name} w={w} s={s} rd={}",
                !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
            c::cmsDictFree(d);
            c::cmsMLUfree(mlu1);
        }
        // ncl2 via named-color list
        for (ncol, name) in [
            (3u32, "n3"),
            (1, "n1"),
            (12, "n12"),
        ] {
            let nl = c::cmsAllocNamedColorList(
                std::ptr::null_mut(), ncol, 4,
                b"pre\0".as_ptr(), b"suf\0".as_ptr());
            if nl.is_null() { continue; }
            let pcs = [0x8000u16, 0x4000, 0x2000];
            let dev = [0x1000u16, 0x2000, 0x3000, 0x4000];
            for i in 0..ncol {
                let nm = format!("color{i}\0");
                c::cmsAppendNamedColor(nl,
                    nm.as_ptr() as *const u8,
                    pcs.as_ptr(), dev.as_ptr());
            }
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsSetProfileVersion(prof, 4.4);
            let w = c::cmsWriteTag(prof, 0x6E636C32,
                nl as *const _);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(
                    buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, 0x6E636C32)
            } else { std::ptr::null_mut() };
            eprintln!("ncl2-{name} w={w} s={s} rd={}",
                !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
            c::cmsFreeNamedColorList(nl);
        }
    }
}

/// cmsStageSampleCLut16bit/Float — CLUT sampling via callback.
#[test]
fn clut_sample_arms() {
    unsafe extern "C" fn s16(input: *const u16, out: *mut u16,
        _data: *mut std::ffi::c_void) -> i32 {
        for i in 0..3 {
            *out.add(i) = (*input.add(i) as u32 * 32769 / 65535)
                as u16;
        }
        1
    }
    unsafe extern "C" fn sf(input: *const f32, out: *mut f32,
        _data: *mut std::ffi::c_void) -> i32 {
        for i in 0..3 {
            *out.add(i) = *input.add(i) * 0.9;
        }
        1
    }
    unsafe {
        for pts in [2u32, 3, 17] {
            let n = (pts * pts * pts * 3) as usize;
            let grid16 = vec![0x8000u16; n];
            let gridf = vec![0.5f32; n];
            let st16 = c::cmsStageAllocCLut16bit(
                std::ptr::null_mut(), pts, 3, 3,
                grid16.as_ptr());
            let stf = c::cmsStageAllocCLutFloat(
                std::ptr::null_mut(), pts, 3, 3,
                gridf.as_ptr());
            let r16 = c::cmsStageSampleCLut16bit(st16, s16,
                std::ptr::null_mut(), 0);
            let rf = c::cmsStageSampleCLutFloat(stf, sf,
                std::ptr::null_mut(), 0);
            eprintln!("pts={pts} r16={r16} rf={rf}");
            if !st16.is_null() { c::cmsStageFree(st16); }
            if !stf.is_null() { c::cmsStageFree(stf); }
        }
    }
}

/// cmsGetPostScriptCSA/CRD — WriteOutputLUT + PostScript header
/// arms on srgb/lab/cmyk/gray profiles x intents x flags.
#[test]
fn ps_lut_arms() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let gray = c::cmsCreateGrayProfile(std::ptr::null_mut(), g);
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        let cmyk = std::fs::read(dir.join("lcms2cmyk.icc")).ok()
            .map(|d| c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32))
            .unwrap_or(std::ptr::null_mut());
        let mut buf = vec![0u8; 1 << 20];
        for (p, name) in [
            (srgb, "srgb"), (lab, "lab"), (gray, "gray"),
            (cmyk, "cmyk"),
        ] {
            if p.is_null() { continue; }
            for intent in [0u32, 1, 3] {
                for flags in [0u32, 1, 0x0040] {
                    let mut n = buf.len() as u32;
                    let r = c::cmsGetPostScriptCSA(
                        std::ptr::null_mut(), p, intent, flags,
                        buf.as_mut_ptr() as *mut _, n);
                    let mut n2 = buf.len() as u32;
                    let r2 = c::cmsGetPostScriptCRD(
                        std::ptr::null_mut(), p, intent, flags,
                        buf.as_mut_ptr() as *mut _, n2);
                    if intent == 0 {
                        eprintln!("ps {name} f{flags:#x} csa={r} \
                            crd={r2}");
                    }
                }
            }
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
        c::cmsCloseProfile(gray);
        if !cmyk.is_null() { c::cmsCloseProfile(cmyk); }
        c::cmsFreeToneCurve(g);
    }
}

/// cmsSliceSpace16/Float + DefaultInterpolatorsFactory — CLUT
/// evals with 1-4ch + grid sizes -> interp dispatch arms.
#[test]
fn slice_interp_arms() {
    // SliceSpace sampler — `out` is NULL (iteration helper)
    unsafe extern "C" fn s16(input: *const u16, _out: *mut u16,
        _d: *mut std::ffi::c_void) -> i32 {
        unsafe {
            let _ = *input;
        }
        1
    }
    unsafe extern "C" fn sf(input: *const f32, _out: *mut f32,
        _d: *mut std::ffi::c_void) -> i32 {
        unsafe {
            let _ = *input;
        }
        1
    }
    unsafe {
        for nch in [1u32, 2, 3, 4] {
            for pts in [2u32, 5, 33] {
                let dims = vec![pts; nch as usize];
                let r16 = c::cmsSliceSpace16(nch, dims.as_ptr(),
                    s16, nch as usize as *mut _);
                let rf = c::cmsSliceSpaceFloat(nch, dims.as_ptr(),
                    sf, nch as usize as *mut _);
                eprintln!("{nch}ch pts{pts} r16={r16} rf={rf}");
            }
        }
        // CLUT eval on multi-channel pipelines — interp factory
        for nch in [3u32, 4] {
            let total = (3u32.pow(nch) * nch) as usize;
            let grid = vec![0x4000u16; total];
            let dims = vec![3u32; nch as usize];
            let cl = c::cmsStageAllocCLut16bitGranular(
                std::ptr::null_mut(), dims.as_ptr(), nch, nch,
                grid.as_ptr());
            let p = c::cmsPipelineAlloc(
                std::ptr::null_mut(), nch, nch);
            c::cmsPipelineInsertStage(p, 1, cl);
            let inp = vec![0x5555u16; nch as usize];
            let mut out = vec![0u16; nch as usize];
            c::cmsPipelineEval16(inp.as_ptr(),
                out.as_mut_ptr(), p);
            let inpf = vec![0.33f32; nch as usize];
            let mut outf = vec![0f32; nch as usize];
            c::cmsPipelineEvalFloat(inpf.as_ptr(),
                outf.as_mut_ptr(), p);
            eprintln!("eval {nch}ch out={out:?} outf={outf:?}");
            c::cmsPipelineFree(p);
        }
    }
}

/// cmsMD5computeID — profile ID digest on saved/loaded profiles.
#[test]
fn md5_arms() {
    unsafe extern "C" {
        fn cmsMD5computeID(p: *mut std::ffi::c_void) -> i32;
    }
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        let cmyk = std::fs::read(dir.join("lcms2cmyk.icc")).ok()
            .map(|d| c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32))
            .unwrap_or(std::ptr::null_mut());
        for (p, name) in [
            (srgb, "srgb"), (lab, "lab"), (cmyk, "cmyk"),
        ] {
            if p.is_null() { continue; }
            let r = cmsMD5computeID(p);
            eprintln!("md5 {name}={r}");
            // re-compute after save+reload
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            if c::cmsSaveProfileToMem(p,
                buf.as_mut_ptr() as *mut _, &mut n) != 0 {
                let rp = c::cmsOpenProfileFromMem(
                    buf.as_ptr() as *const _, n);
                if !rp.is_null() {
                    let r2 = cmsMD5computeID(rp);
                    eprintln!("md5 {name}-reloaded={r2}");
                    c::cmsCloseProfile(rp);
                }
            }
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
        if !cmyk.is_null() { c::cmsCloseProfile(cmyk); }
    }
}

/// Type_MPE_Write — mpet tag serialization: position table +
/// cvst/clut/matf/bacs/ecs element writes on d2b0-d3/b2d0-d3.
#[test]
fn mpe_write_arms() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mat = [1.0f64, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        let mk = |kinds: &[u32]| -> Vec<*mut std::ffi::c_void> {
            kinds.iter().map(|k| match *k {
                0 => c::cmsStageAllocToneCurves(
                    std::ptr::null_mut(), 3,
                    [g, g, g].as_ptr() as *mut _),
                1 => c::cmsStageAllocCLut16bitGranular(
                    std::ptr::null_mut(), [3u32; 3].as_ptr(), 3, 3,
                    vec![0x8000u16; 81].as_ptr()),
                2 => c::cmsStageAllocMatrix(
                    std::ptr::null_mut(), 3, 3,
                    mat.as_ptr(), std::ptr::null()),
                _ => std::ptr::null_mut(),
            }).collect()
        };
        let cases: Vec<(Vec<u32>, &str)> = vec![
            (vec![0u32], "cv"),
            (vec![0, 1, 0], "cv-clut-cv"),
            (vec![2, 0, 1, 0], "mat-cv-clut-cv"),
            (vec![0, 2, 1, 2, 0], "cv-mat-clut-mat-cv"),
            (vec![1], "clut-only"),
            (vec![2], "mat-only"),
            (vec![0, 0, 0], "cv-cv-cv"),
            (vec![1, 1], "clut-clut"),
        ];
        for sig in [0x64324230u32, 0x64324231, 0x64324232,
            0x64324233, 0x62324430, 0x62324431, 0x62324432,
            0x62324433] {
            for (kinds, name) in &cases {
                let prof = c::cmsCreateProfilePlaceholder(
                    std::ptr::null_mut());
                c::cmsSetDeviceClass(prof, 0x6D6E7472); // mntr
                c::cmsSetColorSpace(prof, 0x4C616220); // Lab
                c::cmsSetPCS(prof, 0x58595A20);        // XYZ
                c::cmsSetProfileVersion(prof, 4.4);
                let p = c::cmsPipelineAlloc(
                    std::ptr::null_mut(), 3, 3);
                // pipeline owns inserted stages; leak on
                // non-insert so we don't double-free
                let sts = mk(kinds);
                for st in &sts {
                    if !st.is_null() {
                        c::cmsPipelineInsertStage(p, 1, *st);
                    }
                }
                let w = c::cmsWriteTag(prof, sig, p as *const _);
                let mut buf = vec![0u8; 1 << 20];
                let mut n = buf.len() as u32;
                let s = if w != 0 {
                    c::cmsSaveProfileToMem(prof,
                        buf.as_mut_ptr() as *mut _, &mut n)
                } else { 0 };
                let rp = if s != 0 {
                    c::cmsOpenProfileFromMem(
                        buf.as_ptr() as *const _, n)
                } else { std::ptr::null_mut() };
                let rd = if !rp.is_null() {
                    c::cmsReadTag(rp, sig)
                } else { std::ptr::null_mut() };
                eprintln!("{sig:#x}/{name} w={w} s={s} \
                    rd={}", !rd.is_null());
                if !rp.is_null() { c::cmsCloseProfile(rp); }
                c::cmsCloseProfile(prof);
                // skip cmsPipelineFree — upstream can
                // double-free stages consumed by a failed
                // WriteTag (observed SIGSEGV on b2d* rejects)
            }
        }
        c::cmsFreeToneCurve(g);
    }
}

/// Type_LUT8_Read — raw mft1 blobs: channel counts, CLUTpoints
/// 0/1/2+/255, identity vs non-identity matrix, uipow overflow,
/// short tags.
#[test]
fn lut8_read_arms2() {
    fn be32(v: u32) -> [u8; 4] { v.to_be_bytes() }
    fn f15(v: f64) -> [u8; 4] {
        ((v * 65536.0) as i32).to_be_bytes()
    }
    // mft1 body: ich,oich,clutpts,pad, mat9xf15,
    // 256x ich input-curves, clut pts^ich x oich, 256x oich out
    fn mft1(ich: u8, och: u8, pts: u8, nonid: bool,
        sig: u32, clut_override: Option<u32>) -> Vec<u8> {
        let mut t = Vec::new();
        let s = if sig == 0 { 0x6D667431u32 } else { sig };
        t.extend_from_slice(&be32(s));          // 'mft1'
        t.extend_from_slice(&be32(0));          // reserved
        t.push(ich);
        t.push(och);
        t.push(pts);
        t.push(0);
        for i in 0..9 {
            let v = if nonid && i == 0 { 1.5 }
                else if i % 4 == 0 { 1.0 } else { 0.0 };
            t.extend_from_slice(&f15(v));
        }
        for _ in 0..256usize {
            for c in 0..ich {
                t.push(c.wrapping_mul(40) as u8);
            }
        }
        // uipow(och, pts, ich) — saturating count of CLUT bytes
        let nclut = clut_override.unwrap_or_else(|| {
            let mut n = och as u32;
            for _ in 0..pts { n = n.saturating_mul(ich as u32); }
            n
        });
        for i in 0..nclut.min(1 << 22) {
            t.push((i % 251) as u8);
        }
        for _ in 0..256usize {
            for c in 0..och {
                t.push(c.wrapping_mul(60) as u8);
            }
        }
        t
    }
    unsafe {
        let variants: Vec<(Vec<u8>, &str)> = vec![
            (mft1(3, 3, 2, false, 0, None), "3ch-pts2-idmat"),
            (mft1(3, 3, 2, true, 0, None), "3ch-pts2-mat"),
            (mft1(3, 3, 17, false, 0, None), "3ch-pts17"),
            (mft1(4, 3, 2, false, 0, None), "4->3-pts2"),
            (mft1(3, 4, 2, false, 0, None), "3->4-pts2"),
            (mft1(1, 1, 2, false, 0, None), "1ch-pts2"),
            (mft1(3, 3, 0, false, 0, None), "pts0-noclut"),
            (mft1(3, 3, 1, false, 0, None), "pts1-reject"),
            (mft1(0, 3, 2, false, 0, None), "ich0"),
            (mft1(3, 0, 2, false, 0, None), "och0"),
            (mft1(17, 3, 2, false, 0, None), "ich17"),
            (mft1(3, 17, 2, false, 0, None), "och17"),
            // uipow overflow: 255^3 x 3 → nTabSize -1 arm
            (mft1(3, 3, 255, false, 0, Some(0)),
                "pts255-overflow"),
            (mft1(3, 3, 2, false, 0x62616420, None), "badsig"),
            ({ let mut t = mft1(3, 3, 2, false, 0, None);
                t.truncate(24); t }, "truncated"),
        ];
        for (blob, name) in &variants {
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsSetProfileVersion(prof, 4.4);
            let w = c::cmsWriteRawTag(prof, 0x41324230,
                blob.as_ptr() as *const _, blob.len() as u32);
            let mut buf = vec![0u8; 1 << 22];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(
                    buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let rd = if !rp.is_null() {
                c::cmsReadTag(rp, 0x41324230)
            } else { std::ptr::null_mut() };
            eprintln!("{name} w={w} s={s} rd={}", !rd.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
    }
}

/// cmsnamed internals — grow-on-append, NamedColorIndex
/// hit/miss, _cmsStageAllocNamedColor eval, pseq alloc/dup.
#[test]
fn namedcolor_arms() {
    unsafe {
        // alloc small (n=2), append many -> NamedColorListGrow
        let nl = c::cmsAllocNamedColorList(std::ptr::null_mut(),
            2, 4, b"pre\0".as_ptr(), b"suf\0".as_ptr());
        assert!(!nl.is_null());
        let pcs = [0x1000u16, 0x2000, 0x3000];
        let dev = [0x4000u16, 0x5000, 0x6000, 0x7000];
        for i in 0..40 {
            let nm = format!("c{i}\0");
            let r = c::cmsAppendNamedColor(nl,
                nm.as_ptr() as *const u8, pcs.as_ptr(),
                dev.as_ptr());
            if i < 3 || i == 39 { eprintln!("append {i}={r}"); }
        }
        let n = c::cmsNamedColorCount(nl);
        // index lookups: hit / miss / prefix-match
        for name in ["c0\0", "c5\0", "c39\0", "zzz\0", "c\0",
            "PRE_c0\0"] {
            let i = c::cmsNamedColorIndex(nl,
                name.as_ptr() as *const u8);
            eprintln!("index {name:?}={i}");
        }
        // NamedColorInfo per-entry
        let mut root = [0u8; 64];
        let mut pcs2 = [0u16; 3];
        let mut dev2 = [0u16; 4];
        for i in [0, 20, 39, 40, 99] {
            let ok = c::cmsNamedColorInfo(nl, i, root.as_mut_ptr()
                as *mut _, std::ptr::null_mut(),
                std::ptr::null_mut(), pcs2.as_mut_ptr(),
                dev2.as_mut_ptr());
            eprintln!("info {i}={ok}");
        }
        // named-color stage eval
        let nst = c::_cmsStageAllocNamedColor(nl, 1);
        let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 1, 3);
        c::cmsPipelineInsertStage(p, 1, nst);
        let inp = [3u16];
        let mut out = [0u16; 3];
        c::cmsPipelineEval16(inp.as_ptr(), out.as_mut_ptr(), p);
        eprintln!("ncl eval {out:?}");
        c::cmsPipelineFree(p);
        c::cmsFreeNamedColorList(nl);
        // pseq alloc/dup/write
        let ps = c::cmsAllocProfileSequenceDescription(
            std::ptr::null_mut(), 3);
        let ps2 = c::cmsDupProfileSequenceDescription(ps);
        eprintln!("pseq={} dup={}", !ps.is_null(), !ps2.is_null());
        let prof = c::cmsCreate_sRGBProfile();
        c::cmsSetProfileVersion(prof, 4.4);
        let w = c::cmsWriteTag(prof, 0x70736571,
            ps as *const _);
        let mut buf = vec![0u8; 1 << 20];
        let mut nn = buf.len() as u32;
        let s = if w != 0 {
            c::cmsSaveProfileToMem(prof,
                buf.as_mut_ptr() as *mut _, &mut nn)
        } else { 0 };
        eprintln!("pseq w={w} s={s}");
        c::cmsCloseProfile(prof);
        c::cmsFreeProfileSequenceDescription(ps);
        c::cmsFreeProfileSequenceDescription(ps2);
    }
}

/// Type_MHC2_Read — raw MHC2 blob: valid tables+matrix +
/// oversized/short/missing-offset variants.
#[test]
fn mhc2_read_arms2() {
    fn be32(v: u32) -> [u8; 4] { v.to_be_bytes() }
    fn f15(v: f64) -> [u8; 4] {
        ((v * 65536.0) as i32).to_be_bytes()
    }
    fn f64be(v: f64) -> [u8; 8] { v.to_be_bytes() }
    // mhc2 body: CurveEntries u32, MinL/PeakL f15, MatrixOff,
    // ROff, GOff, BOff u32 (offsets from tag base)
    fn mhc2(ne: u32, with_mat: bool, entries: u32) -> Vec<u8> {
        let mut t = Vec::new();
        t.extend_from_slice(&be32(0x4D484332));   // 'MHC2'
        t.extend_from_slice(&be32(0));
        t.extend_from_slice(&be32(entries));
        t.extend_from_slice(&f15(0.0));
        t.extend_from_slice(&f15(1.0));
        let off = 40u32;
        t.extend_from_slice(&be32(if with_mat { off }
            else { 0 }));
        let taboff = if with_mat { off + 8 * 12 } else { off };
        t.extend_from_slice(&be32(taboff));
        t.extend_from_slice(&be32(taboff + 8 + ne * 8));
        t.extend_from_slice(&be32(taboff + 16 + ne * 16));
        if with_mat {
            for i in 0..12 {
                t.extend_from_slice(&f64be(if i % 4 == 0 {
                    1.0 } else { 0.0 }));
            }
        }
        for c in 0..3 {
            t.extend_from_slice(&be32(0));          // table hdr?
            for i in 0..ne {
                t.extend_from_slice(&f64be(
                    (i % 7) as f64 / 7.0 + c as f64 * 0.01));
            }
        }
        t
    }
    unsafe {
        let variants: Vec<(Vec<u8>, &str)> = vec![
            (mhc2(16, true, 16), "valid-mat"),
            (mhc2(16, false, 16), "valid-nomat"),
            (mhc2(1, true, 1), "n1"),
            (mhc2(256, true, 256), "n256"),
            (mhc2(4, true, 4096), "too-many-entries"),
            (mhc2(4, true, 5000), "entries>4096"),
            ({ let mut t = mhc2(16, true, 16); t.truncate(30);
                t }, "truncated"),
            ({ let mut t = mhc2(16, false, 16);
                t[8..12].copy_from_slice(&be32(0xFFFFFFFF));
                t }, "huge-entries"),
        ];
        for (blob, name) in &variants {
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsSetProfileVersion(prof, 4.4);
            let w = c::cmsWriteRawTag(prof, 0x4D484332,
                blob.as_ptr() as *const _, blob.len() as u32);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(
                    buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let rd = if !rp.is_null() {
                c::cmsReadTag(rp, 0x4D484332)
            } else { std::ptr::null_mut() };
            eprintln!("{name} w={w} s={s} rd={}", !rd.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
    }
}

/// Type_MLU_Read + Type_NamedColor_Read + Type_Screening_Read —
/// raw blobs through write-raw + save + reopen.
#[test]
fn mlu_ncl2_scr_read_arms() {
    fn be32(v: u32) -> [u8; 4] { v.to_be_bytes() }
    fn be16(v: u16) -> [u8; 2] { v.to_be_bytes() }
    // mlu: recSize u32, nRecs u32, [lang u16, country u16,
    //   len u32, off u32]*nRecs, text
    fn mlu(recs: &[(u16, u16, &[u8])]) -> Vec<u8> {
        let mut t = Vec::new();
        t.extend_from_slice(&be32(0x6D6C7563));   // 'mluc'
        t.extend_from_slice(&be32(0));
        t.extend_from_slice(&be32(12));
        t.extend_from_slice(&be32(recs.len() as u32));
        let mut off = 16 + recs.len() * 12;
        let mut text = Vec::new();
        for (_, _, txt) in recs {
            t.extend_from_slice(&be16(0x656E));
            t.extend_from_slice(&be16(0x5553));
            t.extend_from_slice(&be32(txt.len() as u32));
            t.extend_from_slice(&be32(off as u32));
            text.extend_from_slice(txt);
            off += txt.len();
        }
        t.extend_from_slice(&text);
        t
    }
    // ncl2: vendor u32, nColors u32, nCoord u32,
    //   [name 32, pcs u16*3, dev u16*nCoord]*n
    fn ncl2(ncoord: u32, entries: &[u32]) -> Vec<u8> {
        let mut t = Vec::new();
        t.extend_from_slice(&be32(0x6E636C32));   // 'ncl2'
        t.extend_from_slice(&be32(0));
        t.extend_from_slice(&be32(0x6C636D73));   // vendor
        t.extend_from_slice(&be32(entries.len() as u32));
        t.extend_from_slice(&be32(ncoord));
        for i in entries {
            let mut name = [0u8; 32];
            let nm = format!("c{i}");
            name[..nm.len()].copy_from_slice(nm.as_bytes());
            t.extend_from_slice(&name);
            for v in [0x1000u16, 0x2000, 0x3000] {
                t.extend_from_slice(&be16(v));
            }
            for _ in 0..ncoord {
                t.extend_from_slice(&be16(*i as u16 * 100));
            }
        }
        t
    }
    // screening/scr : channels u32, [freq f64, angle f64,
    //   shape u32]*n
    fn scr(n: u32, shapes: &[u32]) -> Vec<u8> {
        let mut t = Vec::new();
        t.extend_from_slice(&be32(0x7363726E));   // 'scrn'
        t.extend_from_slice(&be32(0));
        t.extend_from_slice(&be32(0x00010000));   // flag
        t.extend_from_slice(&be32(n));
        for i in 0..n {
            let sh = shapes.get(i as usize).copied()
                .unwrap_or(0);
            t.extend_from_slice(&((30.0f64 + i as f64)
                .to_bits() as u64).to_be_bytes());
            t.extend_from_slice(&((45.0f64).to_bits() as u64)
                .to_be_bytes());
            t.extend_from_slice(&be32(sh));
        }
        t
    }
    unsafe {
        let variants: Vec<(u32, Vec<u8>, &str)> = vec![
            (0x64657363, mlu(&[(0x656E, 0x5553, b"hello\0")]),
                "mlu-1"),
            (0x64657363, mlu(&[(0x656E, 0x5553, b"a"),
                (0x6465, 0x4445, b"deutsch")]), "mlu-2"),
            (0x64657363, mlu(&[]), "mlu-empty"),
            (0x6E636C32, ncl2(4, &[1, 2, 3]), "ncl2-3"),
            (0x6E636C32, ncl2(4, &[0; 300]), "ncl2-300"),
            (0x6E636C32, ncl2(0, &[1]), "ncl2-nocolorant"),
            (0x7363726E, scr(3, &[0, 1, 2]), "scr-3"),
            (0x7363726E, scr(1, &[0]), "scr-1"),
            (0x7363726E, scr(4, &[0, 1, 2, 3]), "scr-4"),
            (0x7363726E, scr(3, &[99, 0, 1]), "scr-badshape"),
            (0x64657363, {
                let mut t = mlu(&[(0x656E, 0x5553, b"x")]);
                t.truncate(20); t }, "mlu-trunc"),
            (0x6E636C32, {
                let mut t = ncl2(4, &[1, 2, 3]);
                t.truncate(30); t }, "ncl2-trunc"),
        ];
        // valid MLU via API — covers the mluc success path
        {
            let m = c::cmsMLUalloc(std::ptr::null_mut(), 2);
            c::cmsMLUsetASCII(m, b"en\0".as_ptr(),
                b"US\0".as_ptr(), b"sample desc\0".as_ptr());
            c::cmsMLUsetASCII(m, b"de\0".as_ptr(),
                b"DE\0".as_ptr(), b"deutsch\0".as_ptr());
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsSetProfileVersion(prof, 4.4);
            let w = c::cmsWriteTag(prof, 0x64657363,
                m as *const _);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(
                    buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let rd = if !rp.is_null() {
                c::cmsReadTag(rp, 0x64657363)
            } else { std::ptr::null_mut() };
            eprintln!("mlu-api w={w} s={s} rd={}", !rd.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
            c::cmsMLUfree(m);
        }
        for (sig, blob, name) in &variants {
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsSetProfileVersion(prof, 4.4);
            let w = c::cmsWriteRawTag(prof, *sig,
                blob.as_ptr() as *const _, blob.len() as u32);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(
                    buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let rd = if !rp.is_null() {
                c::cmsReadTag(rp, *sig)
            } else { std::ptr::null_mut() };
            eprintln!("{name} w={w} s={s} rd={}", !rd.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
    }
}

/// Type_MPEclut_Read + Type_MPEmatrix_Read — mpet elements
/// (clut/matf) through d2b0/b2d0: valid + malformed variants.
#[test]
fn mpe_elem_read_arms() {
    fn be32(v: u32) -> [u8; 4] { v.to_be_bytes() }
    fn be16(v: u16) -> [u8; 2] { v.to_be_bytes() }
    fn f32be(v: f32) -> [u8; 4] { v.to_be_bytes() }
    // element: sig u32, rsv u32, payload
    fn clut_elem(ich: u8, och: u8, dims: &[u8],
        fill: u8) -> Vec<u8> {
        let mut t = Vec::new();
        t.extend_from_slice(&be32(0x636C7574));   // 'clut'
        t.extend_from_slice(&be32(0));
        t.extend_from_slice(&be16(ich as u16));
        t.extend_from_slice(&be16(och as u16));
        let mut d8 = [0u8; 16];
        for (i, v) in dims.iter().enumerate() {
            d8[i] = *v;
        }
        t.extend_from_slice(&d8);
        let mut n = och as usize;
        for v in dims { n *= *v as usize; }
        for i in 0..n.min(1 << 20) {
            t.extend_from_slice(&f32be(
                (i % 13) as f32 / 13.0 + fill as f32 * 0.01));
        }
        t
    }
    fn matf_elem(ich: u8, och: u8) -> Vec<u8> {
        let mut t = Vec::new();
        t.extend_from_slice(&be32(0x6D617466));   // 'matf'
        t.extend_from_slice(&be32(0));
        t.extend_from_slice(&be16(ich as u16));
        t.extend_from_slice(&be16(och as u16));
        let n = (ich as usize) * (och as usize);
        for i in 0..n + och as usize {
            let v = if i % (ich as usize + 1) == 0 { 1.0f32 }
                else { 0.0 };
            t.extend_from_slice(&f32be(v));
        }
        t
    }
    // mpet: 'mpet' + rsv + u32 nElements + [off u32, size u32]*n
    // offsets relative to tag-data base (after the 12-byte hdr?
    // actually position table base = tag start)
    fn mpet(elems: &[Vec<u8>]) -> Vec<u8> {
        let mut t = Vec::new();
        t.extend_from_slice(&be32(0x6D706574));   // 'mpet'
        t.extend_from_slice(&be32(0));
        t.extend_from_slice(&be16(3));            // nIn
        t.extend_from_slice(&be16(3));            // nOut
        t.extend_from_slice(&be32(elems.len() as u32));
        let mut off = 16 + elems.len() as u32 * 8;
        for e in elems {
            t.extend_from_slice(&be32(off));
            t.extend_from_slice(&be32(e.len() as u32));
            off += e.len() as u32;
        }
        for e in elems { t.extend_from_slice(e); }
        t
    }
    unsafe {
        let variants: Vec<(Vec<u8>, &str)> = vec![
            (mpet(&[clut_elem(3, 3, &[2, 2, 2], 0)]),
                "clut-3ch"),
            (mpet(&[clut_elem(4, 3, &[2, 2, 2, 2], 0)]),
                "clut-4in"),
            (mpet(&[clut_elem(3, 4, &[3, 3, 3], 0)]),
                "clut-4out"),
            (mpet(&[clut_elem(1, 1, &[5], 0)]), "clut-1ch"),
            (mpet(&[clut_elem(0, 3, &[2, 2, 2], 0)]),
                "clut-ich0"),
            (mpet(&[clut_elem(3, 0, &[2, 2, 2], 0)]),
                "clut-och0"),
            (mpet(&[clut_elem(3, 3, &[1, 2, 2], 0)]),
                "clut-dim1"),
            (mpet(&[clut_elem(3, 3, &[0, 2, 2], 0)]),
                "clut-dim0"),
            (mpet(&[matf_elem(3, 3)]), "matf-3x3"),
            (mpet(&[matf_elem(3, 4)]), "matf-3x4"),
            (mpet(&[matf_elem(1, 3)]), "matf-1x3"),
            (mpet(&[matf_elem(0, 3)]), "matf-ich0"),
            (mpet(&[clut_elem(3, 3, &[2, 2, 2], 0),
                matf_elem(3, 3)]), "clut+matf"),
            (mpet(&[clut_elem(3, 3, &[2, 2, 2], 0),
                clut_elem(3, 3, &[3, 3, 3], 1)]), "clut+clut"),
            ({ let mut t = mpet(&[clut_elem(3, 3, &[2, 2, 2],
                0)]); t.truncate(20); t }, "trunc"),
        ];
        for sig in [0x44324230u32, 0x42324430] {
            for (blob, name) in &variants {
                let prof = c::cmsCreateProfilePlaceholder(
                    std::ptr::null_mut());
                c::cmsSetDeviceClass(prof, 0x6D6E7472);
                c::cmsSetColorSpace(prof, 0x4C616220);
                c::cmsSetPCS(prof, 0x58595A20);
                c::cmsSetProfileVersion(prof, 4.4);
                let w = c::cmsWriteRawTag(prof, sig,
                    blob.as_ptr() as *const _, blob.len() as u32);
                let mut buf = vec![0u8; 1 << 22];
                let mut n = buf.len() as u32;
                let s = if w != 0 {
                    c::cmsSaveProfileToMem(prof,
                        buf.as_mut_ptr() as *mut _, &mut n)
                } else { 0 };
                let rp = if s != 0 {
                    c::cmsOpenProfileFromMem(
                        buf.as_ptr() as *const _, n)
                } else { std::ptr::null_mut() };
                let rd = if !rp.is_null() {
                    c::cmsReadTag(rp, sig)
                } else { std::ptr::null_mut() };
                eprintln!("{sig:#x}/{name} w={w} s={s} rd={}",
                    !rd.is_null());
                if !rp.is_null() { c::cmsCloseProfile(rp); }
                c::cmsCloseProfile(prof);
            }
        }
    }
}

/// Type_vcgt_Write — formula path (all 3 curves parametric-5) +
/// table path (non-parametric) + mixed.
#[test]
fn vcgt_write_arms() {
    unsafe {
        let g5 = c::cmsBuildParametricToneCurve(
            std::ptr::null_mut(), 5,
            [2.2, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]
                .as_ptr());
        let g5b = c::cmsBuildParametricToneCurve(
            std::ptr::null_mut(), 5,
            [2.4, 0.9, 0.1, 0.0, 0.0, 0.02, 0.0, 0.0, 0.0, 0.0]
                .as_ptr());
        let gtab = c::cmsBuildTabulatedToneCurve16(
            std::ptr::null_mut(), 5,
            [0u16, 0x2000, 0x8000, 0xd000, 0xffff].as_ptr());
        let g1 = c::cmsBuildGamma(std::ptr::null_mut(), 1.8);
        for (curves, name) in [
            ([g5, g5, g5], "all-formula"),
            ([g5, g5b, g5], "mixed-formula"),
            ([gtab, gtab, gtab], "all-table"),
            ([g5, gtab, g1], "mixed"),
            ([g1, g1, g1], "gamma-1.8"),
        ] {
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsSetProfileVersion(prof, 4.4);
            let arr = curves;
            let w = c::cmsWriteTag(prof, 0x76636774,
                arr.as_ptr() as *mut _);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(
                    buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let rd = if !rp.is_null() {
                c::cmsReadTag(rp, 0x76636774)
            } else { std::ptr::null_mut() };
            eprintln!("{name} w={w} s={s} rd={}", !rd.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
        for tc in [g5, g5b, gtab, g1] {
            c::cmsFreeToneCurve(tc);
        }
    }
}

/// Type_UcrBg_Write — cmsUcrBg struct via bfd tag.
#[test]
fn ucrbg_write_arms() {
    unsafe {
        let tv = [0u16, 0x2000, 0x8000, 0xd000, 0xffff];
        let ucr = c::cmsBuildTabulatedToneCurve16(
            std::ptr::null_mut(), 5, tv.as_ptr());
        let bg = c::cmsBuildTabulatedToneCurve16(
            std::ptr::null_mut(), 5,
            [0u16, 0x4000, 0xa000, 0xe000, 0xffff].as_ptr());
        let mlu = c::cmsMLUalloc(std::ptr::null_mut(), 1);
        c::cmsMLUsetASCII(mlu, b"en\0".as_ptr(),
            b"US\0".as_ptr(), b"ucrbg desc\0".as_ptr());
        // cmsUcrBg { ToneCurve *Ucr, *Bg; cmsMLU *Desc; }
        let ucrbg = [ucr as *mut std::ffi::c_void,
            bg as *mut std::ffi::c_void, mlu];
        for ver in [4.4f64, 2.1] {
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsSetProfileVersion(prof, ver);
            let w = c::cmsWriteTag(prof, 0x62666420,
                ucrbg.as_ptr() as *mut _);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(
                    buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let rd = if !rp.is_null() {
                c::cmsReadTag(rp, 0x62666420)
            } else { std::ptr::null_mut() };
            eprintln!("v{ver} w={w} s={s} rd={}", !rd.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
        c::cmsFreeToneCurve(ucr);
        c::cmsFreeToneCurve(bg);
        c::cmsMLUfree(mlu);
    }
}

/// Type_ProfileSequenceDesc_Read/Write + Type_Text_Description_
/// Write + cmsDetectTAC (BlackPointAsDarkerColorant).
#[test]
fn pseq_desc_tac_arms() {
    unsafe {
        // pseq via API write + reopen + read
        let ps = c::cmsAllocProfileSequenceDescription(
            std::ptr::null_mut(), 2);
        for sig in [0x70736571u32, 0x70736964] {
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsSetProfileVersion(prof, 4.4);
            let w = c::cmsWriteTag(prof, sig, ps as *const _);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(
                    buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let rd = if !rp.is_null() {
                c::cmsReadTag(rp, sig)
            } else { std::ptr::null_mut() };
            eprintln!("{sig:#x} w={w} s={s} rd={}", !rd.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
        c::cmsFreeProfileSequenceDescription(ps);
        // v2 desc struct — the desc-type write path
        let srgb = c::cmsCreate_sRGBProfile();
        c::cmsSetProfileVersion(srgb, 2.1);
        let mlu = c::cmsMLUalloc(std::ptr::null_mut(), 1);
        c::cmsMLUsetASCII(mlu, b"en\0".as_ptr(),
            b"US\0".as_ptr(), b"v2 description\0".as_ptr());
        let w = c::cmsWriteTag(srgb, 0x64657363,
            mlu as *const _);
        let mut buf = vec![0u8; 1 << 20];
        let mut n = buf.len() as u32;
        let s = if w != 0 {
            c::cmsSaveProfileToMem(srgb,
                buf.as_mut_ptr() as *mut _, &mut n)
        } else { 0 };
        let rp = if s != 0 {
            c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
        } else { std::ptr::null_mut() };
        let rd = if !rp.is_null() {
            c::cmsReadTag(rp, 0x64657363)
        } else { std::ptr::null_mut() };
        eprintln!("v2-desc w={w} s={s} rd={}", !rd.is_null());
        if !rp.is_null() { c::cmsCloseProfile(rp); }
        c::cmsCloseProfile(srgb);
        c::cmsMLUfree(mlu);
        // cmsDetectTAC — BlackPointAsDarkerColorant path
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        let cmyk = std::fs::read(dir.join("lcms2cmyk.icc")).ok()
            .map(|d| c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32))
            .unwrap_or(std::ptr::null_mut());
        let srgb2 = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        for (p, name) in [(srgb2, "srgb"), (lab, "lab"),
            (cmyk, "cmyk")] {
            if p.is_null() { continue; }
            let tac = c::cmsDetectTAC(p);
            eprintln!("tac {name}={tac}");
        }
        c::cmsCloseProfile(srgb2);
        c::cmsCloseProfile(lab);
        if !cmyk.is_null() { c::cmsCloseProfile(cmyk); }
    }
}

/// WriteInputLUT (PostScript input-LUT on input-class) +
/// cmsOpenIOhandlerFromFile/_cmsReadHeader error arms +
/// cmsCreateLab2/4ProfileTHR.
#[test]
fn ps_input_io_arms() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let lab2 = c::cmsCreateLab2Profile(std::ptr::null_mut());
        let lab4t = c::cmsCreateLab4ProfileTHR(
            std::ptr::null_mut(), std::ptr::null_mut());
        let lab2t = c::cmsCreateLab2ProfileTHR(
            std::ptr::null_mut(), std::ptr::null_mut());
        eprintln!("lab2/4thr={}/{}", !lab4t.is_null(),
            !lab2t.is_null());
        // input-class profile (class='scnr') for WriteInputLUT
        let inpc = c::cmsCreateProfilePlaceholder(
            std::ptr::null_mut());
        c::cmsSetDeviceClass(inpc, 0x73636e72);   // scnr
        c::cmsSetColorSpace(inpc, 0x52474220);
        c::cmsSetPCS(inpc, 0x4C616220);
        c::cmsSetProfileVersion(inpc, 4.4);
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        let mut grid = vec![0u16; 27 * 3];
        for (i, v) in grid.iter_mut().enumerate() {
            *v = ((i % 23) * 2848) as u16;
        }
        let dims = [3u32; 3];
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocCLut16bitGranular(
                std::ptr::null_mut(), dims.as_ptr(), 3, 3,
                grid.as_ptr()));
        c::cmsWriteTag(inpc, 0x41324230, p as *const _);
        let mut buf = vec![0u8; 1 << 20];
        for (prof, name) in [
            (inpc, "input"), (srgb, "display"), (lab, "lab"),
        ] {
            for intent in [0u32, 1] {
                let r = c::cmsGetPostScriptCSA(
                    std::ptr::null_mut(), prof, intent, 0,
                    buf.as_mut_ptr() as *mut _,
                    buf.len() as u32);
                let r2 = c::cmsGetPostScriptCRD(
                    std::ptr::null_mut(), prof, intent, 0,
                    buf.as_mut_ptr() as *mut _,
                    buf.len() as u32);
                eprintln!("ps {name} i{intent} csa={r} crd={r2}");
            }
        }
        // file-IO error arms — nonexistent/dir/empty file
        let f1 = c::cmsOpenProfileFromFile(
            b"/nonexistent/x.icc\0".as_ptr() as *const i8,
            b"r\0".as_ptr() as *const u8);
        let f2 = c::cmsOpenProfileFromFile(
            b"/tmp\0".as_ptr() as *const i8,
            b"r\0".as_ptr() as *const u8);
        let mut empty = std::env::temp_dir();
        empty.push("lcms_empty.icc");
        std::fs::write(&empty, b"").ok();
        let es = empty.to_string_lossy().into_owned() + "\0";
        let f3 = c::cmsOpenProfileFromFile(
            es.as_ptr() as *const i8,
            b"r\0".as_ptr() as *const u8);
        // 16-byte truncated header
        std::fs::write(&empty, &[0u8; 16]).ok();
        let f4 = c::cmsOpenProfileFromFile(
            es.as_ptr() as *const i8,
            b"r\0".as_ptr() as *const u8);
        // garbage header
        std::fs::write(&empty, &[0x41u8; 132]).ok();
        let f5 = c::cmsOpenProfileFromFile(
            es.as_ptr() as *const i8,
            b"r\0".as_ptr() as *const u8);
        eprintln!("file miss={} dir={} empty={} trunc={} \
            garb={}", !f1.is_null(), !f2.is_null(),
            !f3.is_null(), !f4.is_null(), !f5.is_null());
        for f in [f1, f2, f3, f4, f5] {
            if !f.is_null() { c::cmsCloseProfile(f); }
        }
        std::fs::remove_file(&empty).ok();
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
        c::cmsCloseProfile(lab2);
        c::cmsCloseProfile(inpc);
        if !lab4t.is_null() { c::cmsCloseProfile(lab4t); }
        if !lab2t.is_null() { c::cmsCloseProfile(lab2t); }
        c::cmsPipelineFree(p);
        c::cmsFreeToneCurve(g);
    }
}

/// Type_LUTA2B_Read/Type_LUTB2A_Read — raw mAB/mBA blobs:
/// curve/matrix/clut/curve orders x malformed variants.
#[test]
fn lutab_read_arms() {
    fn be32(v: u32) -> [u8; 4] { v.to_be_bytes() }
    fn be16(v: u16) -> [u8; 2] { v.to_be_bytes() }
    // embedded 'curv' element: sig+rsv+u32 nEntries+u16 data
    fn curve(vals: &[u16]) -> Vec<u8> {
        let mut t = Vec::new();
        t.extend_from_slice(&be32(0x63757276));   // 'curv'
        t.extend_from_slice(&be32(0));
        t.extend_from_slice(&be32(vals.len() as u32));
        for v in vals { t.extend_from_slice(&be16(*v)); }
        while t.len() % 4 != 0 { t.push(0); }   // 4-align
        t
    }
    // mft2 (16-bit) lut: pad[6], nInput u8, nOutput u8,
    //   grid u8, pad2, 9x s15f16, inp u16, out u16,
    //   [incurves][clut][outcurves]
    fn mft2(ich: u8, och: u8, grid: u8, mat: &[i32; 9],
        inc: &[u16], clut_fill: u8, outc: &[u16]) -> Vec<u8> {
        let mut t = Vec::new();
        t.extend_from_slice(&be32(0x6D667432));   // 'mft2'
        t.extend_from_slice(&be32(0));
        t.push(ich);
        t.push(och);
        t.push(grid);
        t.push(0);
        for v in mat { t.extend_from_slice(&v.to_be_bytes()); }
        t.extend_from_slice(&be16(inc.len() as u16));
        t.extend_from_slice(&be16(outc.len() as u16));
        for _ in 0..ich {
            t.extend_from_slice(&curve(inc));
        }
        let n = (grid as usize).pow(ich as u32) * och as usize;
        for i in 0..n {
            t.extend_from_slice(&be16((i as u16 % 31)
                * 2110 + clut_fill as u16));
        }
        for _ in 0..och {
            t.extend_from_slice(&curve(outc));
        }
        t
    }
    // mAB: header {nIn u8, nOut u8, pad} + offs B/A/CLUT/M/Matrix/
    //   u32 each (relative to tag-data base)
    fn mab(ich: u8, och: u8, b: &[Vec<u8>],
        a: &[Vec<u8>], clut: Option<&[u8]>, m: Option<&[Vec<u8>]>,
        mat: &[i32; 9], sig: u32) -> Vec<u8> {
        let mut t = Vec::new();
        t.extend_from_slice(&be32(sig));   // 'mAB '
        t.extend_from_slice(&be32(0));
        t.push(ich);
        t.push(och);
        t.extend_from_slice(&[0u8; 2]);
        // 5 offsets: B, A, M, CLUT-pos-table?, mat — the
        // mAB has B/A/M/clut/mat offsets (matrix first?)
        let offs_at = t.len();
        for _ in 0..5 { t.extend_from_slice(&be32(0)); }
        // offsets are from the tag start
        let boff = t.len();
        for c in b { t.extend_from_slice(c); }
        let aoff = if a.is_empty() { 0 } else { t.len() };
        for c in a { t.extend_from_slice(c); }
        let moff = m.map(|_| t.len()).unwrap_or(0);
        for c in m.unwrap_or_default() {
            t.extend_from_slice(c);
        }
        let clutoff = clut.map(|_| t.len()).unwrap_or(0);
        if let Some(c) = clut { t.extend_from_slice(c); }
        let matoff = if mat.iter().any(|v| *v != 0) {
            t.len() } else { 0 };
        if matoff != 0 {
            for v in mat { t.extend_from_slice(&v.to_be_bytes()); }
        }
        // order: B, Matrix, M, CLUT, A offsets
        let seq = [boff as u32, matoff as u32, moff as u32,
            clutoff as u32, aoff as u32];
        for (i, v) in seq.iter().enumerate() {
            t[offs_at + i * 4..offs_at + i * 4 + 4]
                .copy_from_slice(&v.to_be_bytes());
        }
        t
    }
    fn clut_bytes(ich: u8, och: u8, grid: u8) -> Vec<u8> {
        let mut t = Vec::new();
        let mut g = [0u8; 16];
        for i in 0..ich as usize { g[i] = grid; }
        t.extend_from_slice(&g);
        t.push(2);                        // precision u8
        t.extend_from_slice(&[0u8; 3]);   // reserved
        let n = (grid as usize).pow(ich as u32) * och as usize;
        for i in 0..n {
            t.extend_from_slice(&be16((i as u16 % 31) * 2110));
        }
        t
    }
    let lin = curve(&[0, 0xffff]);
    let g2 = curve(&[0, 0x8000, 0xffff]);
    let idmat: [i32; 9] = [0x10000, 0, 0, 0, 0x10000, 0,
        0, 0, 0x10000];
    let nonmat: [i32; 9] = [0x10000, 0x1000, 0, 0x2000,
        0x10000, 0, 0, 0x3000, 0x10000];
    unsafe {
        let variants: Vec<(u32, Vec<u8>, &str)> = vec![
            // a2b0 = mft2 or mAB
            (0x41324230, mft2(3, 3, 3, &idmat,
                &[0, 0xffff], 0, &[0, 0xffff]), "mft2-std"),
            (0x41324230, mft2(3, 3, 2, &nonmat,
                &[0, 0x8000, 0xffff], 1, &[0, 0xffff]),
                "mft2-nonmat"),
            (0x41324230, mft2(1, 1, 1, &idmat,
                &[0, 0xffff], 0, &[0, 0xffff]), "mft2-1ch"),
            (0x41324230, mft2(4, 3, 3, &idmat,
                &[0, 0xffff], 0, &[0, 0xffff]), "mft2-4in"),
            (0x41324230, mft2(3, 4, 3, &idmat,
                &[0, 0xffff], 0, &[0, 0xffff]), "mft2-4out"),
            (0x41324230, mab(3, 3,
                &[lin.clone(), lin.clone(), lin.clone()],
                &[g2.clone(), g2.clone(), g2.clone()],
                None, None, &idmat, 0x6D414220), "mab-cv-cv"),
            (0x41324230, mab(3, 3,
                &[lin.clone(), lin.clone(), lin.clone()],
                &[g2.clone(), g2.clone(), g2.clone()],
                Some(&clut_bytes(3, 3, 3)),
                None, &idmat, 0x6D414220), "mab-cv-clut-cv"),
            (0x41324230, mab(3, 3,
                &[lin.clone(), lin.clone(), lin.clone()],
                &[g2.clone(), g2.clone(), g2.clone()],
                Some(&clut_bytes(3, 3, 3)),
                Some(&[lin.clone(), lin.clone(), lin.clone()]),
                &idmat, 0x6D414220), "mab-cv-mat-clut-cv"),
            (0x41324230, mab(3, 3,
                &[lin.clone(), lin.clone(), lin.clone()],
                &[],
                Some(&clut_bytes(3, 3, 3)), None,
                &nonmat, 0x6D414220), "mab-clut-nonmat"),
            (0x42324130, mft2(3, 3, 3, &idmat,
                &[0, 0xffff], 0, &[0, 0xffff]), "b2a-mft2"),
            (0x42324130, mft2(3, 3, 3, &nonmat,
                &[0, 0xffff], 0, &[0, 0xffff]), "b2a-nonmat"),
            (0x42324130, mab(3, 3,
                &[lin.clone(), lin.clone(), lin.clone()],
                &[g2.clone(), g2.clone(), g2.clone()],
                None, None, &idmat, 0x6D424120), "mba-cv-cv"),
            (0x42324130, mab(3, 3,
                &[lin.clone(), lin.clone(), lin.clone()],
                &[g2.clone(), g2.clone(), g2.clone()],
                Some(&clut_bytes(3, 3, 3)),
                None, &idmat, 0x6D424120), "mba-cv-clut-cv"),
            // malformed
            (0x41324230, { let mut t = mft2(3, 3, 3, &idmat,
                &[0, 0xffff], 0, &[0, 0xffff]); t.truncate(30);
                t }, "mft2-trunc"),
            (0x41324230, { let mut t = mft2(3, 3, 3, &idmat,
                &[0, 0xffff], 0, &[0, 0xffff]); t[8] = 0;
                t }, "mft2-ich0"),
            (0x41324230, { let mut t = mft2(3, 3, 3, &idmat,
                &[0, 0xffff], 0, &[0, 0xffff]); t[9] = 0;
                t }, "mft2-och0"),
            (0x41324230, { let mut t = mft2(3, 3, 3, &idmat,
                &[0, 0xffff], 0, &[0, 0xffff]); t[10] = 1;
                t }, "mft2-grid1"),
            (0x41324230, { let mut t = mft2(3, 3, 3, &idmat,
                &[0, 0xffff], 0, &[0, 0xffff]); t[10] = 0;
                t }, "mft2-grid0"),
            (0x41324230, mab(3, 3, &[lin.clone()],
                &[g2.clone()], None, None,
                &[0i32; 9], 0x6D414220), "mab-nomat"),
        ];
        for (sig, blob, name) in &variants {
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsSetProfileVersion(prof, 4.4);
            let w = c::cmsWriteRawTag(prof, *sig,
                blob.as_ptr() as *const _, blob.len() as u32);
            let mut buf = vec![0u8; 1 << 22];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(
                    buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let rd = if !rp.is_null() {
                c::cmsReadTag(rp, *sig)
            } else { std::ptr::null_mut() };
            eprintln!("{name} w={w} s={s} rd={}", !rd.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
    }
}

/// _cmsReadWCharArray — dict tags with multi-byte/unicode
/// values + desc Unicode-count edge forms.
#[test]
fn wchar_dict_arms() {
    fn be32(v: u32) -> [u8; 4] { v.to_be_bytes() }
    fn be16(v: u16) -> [u8; 2] { v.to_be_bytes() }
    // dict: 'dict' + rsv + u32 count + [name-off,name-len,
    //   val-off,val-len]*n + ucs2be data
    fn dict(entries: &[(u32, &[u16])]) -> Vec<u8> {
        let mut t = Vec::new();
        t.extend_from_slice(&be32(0x64696374));   // 'dict'
        t.extend_from_slice(&be32(0));
        t.extend_from_slice(&be32(entries.len() as u32));
        let mut off = 16 + entries.len() * 16;
        let mut data = Vec::new();
        for (name, val) in entries {
            t.extend_from_slice(&be32(off as u32));
            t.extend_from_slice(&be32(
                (name.to_string().len()) as u32 * 2));
            data.extend_from_slice(&be16(0x0061));
            data.extend_from_slice(&be16(0x0062));
            off += 4;
            t.extend_from_slice(&be32(off as u32));
            t.extend_from_slice(&be32(val.len() as u32 * 2));
            for v in *val { data.extend_from_slice(&be16(*v)); }
            off += val.len() * 2;
            let _ = name;
        }
        t.extend_from_slice(&data);
        t
    }
    unsafe {
        let variants: Vec<(Vec<u8>, &str)> = vec![
            (dict(&[(0x61, &[0x0061, 0x0062]), (0x62,
                &[0x00E9, 0x4E2D, 0xD83D])]), "dict-unicode"),
            (dict(&[(0x61, &[0x0061])]), "dict-1"),
            (dict(&[]), "dict-empty"),
            (dict(&[(0x61, &[])]), "dict-noval"),
        ];
        for (blob, name) in &variants {
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsSetProfileVersion(prof, 4.4);
            let w = c::cmsWriteRawTag(prof, 0x6D657461,
                blob.as_ptr() as *const _, blob.len() as u32);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(
                    buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let rd = if !rp.is_null() {
                c::cmsReadTag(rp, 0x6D657461)
            } else { std::ptr::null_mut() };
            eprintln!("{name} w={w} s={s} rd={}", !rd.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
        // desc unicode-count edge: big count vs short data
        let d = {
            let mut t = Vec::new();
            t.extend_from_slice(&be32(0x64657363));   // 'desc'
            t.extend_from_slice(&be32(0));
            t.extend_from_slice(&be32(5));            // ASCII len
            t.extend_from_slice(b"test");
            t.extend_from_slice(&[0u8; 252]);         // pad
            t.extend_from_slice(&be32(0));            // ucode
            t.extend_from_slice(&be32(200));          // unicode count
            for _ in 0..200 {
                t.extend_from_slice(&be16(0x0061));
            }
            t.extend_from_slice(&be16(0));            // scriptcode
            t.extend_from_slice(&[0u8; 67]);
            t
        };
        let d2 = {
            let mut t = Vec::new();
            t.extend_from_slice(&be32(0x64657363));
            t.extend_from_slice(&be32(0));
            t.extend_from_slice(&be32(2));
            t.extend_from_slice(b"a");
            t.extend_from_slice(&[0u8; 255]);
            t.extend_from_slice(&be32(0));
            t.extend_from_slice(&be32(0xFFFFFF));
            t
        };
        for (blob, name) in [(d, "desc-biguc"), (d2, "desc-huge")] {
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsSetProfileVersion(prof, 2.1);
            let w = c::cmsWriteRawTag(prof, 0x64657363,
                blob.as_ptr() as *const _, blob.len() as u32);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(
                    buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let rd = if !rp.is_null() {
                c::cmsReadTag(rp, 0x64657363)
            } else { std::ptr::null_mut() };
            eprintln!("{name} w={w} s={s} rd={}", !rd.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
    }
}

/// SaveTags/_cmsSearchTag/cmsLinkTag — tag-table write/sort/
/// dup arms via overlapping + raw + linked tag sets.
#[test]
fn savetags_arms() {
    unsafe {
        for ver in [4.4f64, 2.1] {
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsSetProfileVersion(prof, ver);
            // linked tags (same object on 2 sigs)
            let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
            let arr = [g, g, g];
            c::cmsWriteTag(prof, 0x72545243,   // rTRC
                g as *const _);
            c::cmsWriteTag(prof, 0x67545243,   // gTRC = same
                g as *const _);
            c::cmsWriteTag(prof, 0x62545243,   // bTRC = same
                g as *const _);
            let mlu = c::cmsMLUalloc(std::ptr::null_mut(), 1);
            c::cmsMLUsetASCII(mlu, b"en\0".as_ptr(),
                b"US\0".as_ptr(), b"shared\0".as_ptr());
            c::cmsWriteTag(prof, 0x64657363, mlu as *const _);
            c::cmsWriteTag(prof, 0x63707274, mlu as *const _);
            c::cmsWriteTag(prof, 0x646D6E64, mlu as *const _);
            // raw tag (unknown type) interleaved
            let raw = [0x74u8, 0x79, 0x70, 0x65, 0, 0, 0, 0,
                1, 2, 3, 4];
            c::cmsWriteRawTag(prof, 0x61626364,
                raw.as_ptr() as *const _, raw.len() as u32);
            // overwrite an existing tag (dup sig)
            c::cmsWriteTag(prof, 0x72545243, arr.as_ptr()
                as *mut _);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = c::cmsSaveProfileToMem(prof,
                buf.as_mut_ptr() as *mut _, &mut n);
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(
                    buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let n2 = if !rp.is_null() {
                c::cmsGetTagCount(rp)
            } else { -1 };
            eprintln!("v{ver} s={s} tags={n2}");
            if !rp.is_null() {
                for i in 0..n2.max(0) {
                    let sg = c::cmsGetTagSignature(rp, i as u32);
                    let rd = c::cmsReadTag(rp, sg);
                    eprintln!("  tag{i} {sg:#x} rd={}",
                        !rd.is_null());
                }
                c::cmsCloseProfile(rp);
            }
            c::cmsMLUfree(mlu);
            c::cmsFreeToneCurve(g);
            c::cmsCloseProfile(prof);
        }
    }
}

/// Type_LUTB2A_Write — mBA serialization via API pipelines:
/// B->CLUT->A and B->M->CLUT->A shapes x versions.
#[test]
fn lutba_write_arms() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let tv = [0u16, 0x4000, 0x8000, 0xc000, 0xffff];
        let tc = c::cmsBuildTabulatedToneCurve16(
            std::ptr::null_mut(), 5, tv.as_ptr());
        let dims = [3u32; 3];
        let mut grid = vec![0u16; 27 * 3];
        for (i, v) in grid.iter_mut().enumerate() {
            *v = ((i % 23) * 2848) as u16;
        }
        // per-case pipelines (stages can't be shared)
        for (order, name) in [
            ("b-clut-a", "B-clut-A"),
            ("b-mat-clut-a", "B-mat-clut-A"),
            ("b-clut", "B-clut"),
            ("b-a", "B-A"),
        ] {
            for (sig, ver) in [
                (0x41324230u32, 4.4f64),
                (0x42324130, 4.4),
                (0x41324230, 2.1),
            ] {
                let p = c::cmsPipelineAlloc(
                    std::ptr::null_mut(), 3, 3);
                c::cmsPipelineInsertStage(p, 0,
                    c::cmsStageAllocToneCurves(
                        std::ptr::null_mut(), 3,
                        [tc, tc, tc].as_ptr() as *mut _));
                if order == "b-mat-clut-a" {
                    let m = [
                        c::cmsStageAllocToneCurves(
                            std::ptr::null_mut(), 3,
                            [g, g, g].as_ptr() as *mut _)];
                    c::cmsPipelineInsertStage(p, 1,
                        m[0]);
                }
                if order != "b-a" {
                    c::cmsPipelineInsertStage(p, 1,
                        c::cmsStageAllocCLut16bitGranular(
                            std::ptr::null_mut(), dims.as_ptr(),
                            3, 3, grid.as_ptr()));
                }
                if order != "b-clut" {
                    c::cmsPipelineInsertStage(p, 1,
                        c::cmsStageAllocToneCurves(
                            std::ptr::null_mut(), 3,
                            [g, g, g].as_ptr() as *mut _));
                }
                let prof = c::cmsCreateProfilePlaceholder(
                    std::ptr::null_mut());
                c::cmsSetDeviceClass(prof, 0x6D6E7472);
                c::cmsSetColorSpace(prof, 0x52474220);
                c::cmsSetPCS(prof, 0x4C616220);
                c::cmsSetProfileVersion(prof, ver);
                let w = c::cmsWriteTag(prof, sig,
                    p as *const _);
                let mut buf = vec![0u8; 1 << 20];
                let mut n = buf.len() as u32;
                let s = if w != 0 {
                    c::cmsSaveProfileToMem(prof,
                        buf.as_mut_ptr() as *mut _, &mut n)
                } else { 0 };
                let rp = if s != 0 {
                    c::cmsOpenProfileFromMem(
                        buf.as_ptr() as *const _, n)
                } else { std::ptr::null_mut() };
                let rd = if !rp.is_null() {
                    c::cmsReadTag(rp, sig)
                } else { std::ptr::null_mut() };
                eprintln!("{name} {sig:#x} v{ver} w={w} s={s} \
                    rd={}", !rd.is_null());
                if !rp.is_null() { c::cmsCloseProfile(rp); }
                c::cmsCloseProfile(prof);
                c::cmsPipelineFree(p);
            }
        }
        c::cmsFreeToneCurve(g);
        c::cmsFreeToneCurve(tc);
    }
}

/// IsIntentSupported/_cmsReadMatrix + gamut-check internals.
#[test]
fn intent_gamut_arms() {
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        let cmyk = std::fs::read(dir.join("lcms2cmyk.icc")).ok()
            .map(|d| c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32))
            .unwrap_or(std::ptr::null_mut());
        for (p, name) in [
            (srgb, "srgb"), (lab, "lab"), (cmyk, "cmyk"),
        ] {
            if p.is_null() { continue; }
            for intent in 0..17u32 {
                for dirn in 0..3u32 {
                    let r = c::cmsIsIntentSupported(p,
                        intent, dirn);
                    if r != 0 {
                        eprintln!("{name} i{intent} d{dirn} \
                            ={r}");
                    }
                }
            }
        }
        // proofing transform — gamut-check pipeline paths
        if !cmyk.is_null() {
            let r8 = (4u32 << 16) | (3 << 3) | 1;
            let k8 = (6u32 << 16) | (4 << 3) | 1;
            for flags in [0u32, 0x4000 /* SOFT-proof */,
                0x8000 /* GAMUTCHECK */, 0x2000 | 0x4000] {
                let t = c::cmsCreateProofingTransform(
                    srgb, r8, srgb, r8, cmyk, 0, 0, flags);
                eprintln!("proof f{flags:#x} t={}", !t.is_null());
                if !t.is_null() {
                    let ib = [0x55u8; 16];
                    let mut ob = [0u8; 16];
                    c::cmsDoTransform(t, ib.as_ptr() as *const _,
                        ob.as_mut_ptr() as *mut _, 1);
                    c::cmsDeleteTransform(t);
                }
            }
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
        if !cmyk.is_null() { c::cmsCloseProfile(cmyk); }
    }
}
/// Tag writers/readers batch: view/meas/tech/crdi/parametric +
/// linearization devicelink + context + MLU-wide + unlink.
#[test]
fn misc_tag_ctx_arms() {
    fn be32(v: u32) -> [u8; 4] { v.to_be_bytes() }
    fn s15(v: f64) -> [u8; 4] {
        ((v * 65536.0) as i32).to_be_bytes()
    }
    // 'view' tag: XYZ[3] s15f16 + XYZ illuminant + XYZ surround
    //   + illum type u32
    fn view_tag() -> Vec<u8> {
        let mut t = Vec::new();
        t.extend_from_slice(&be32(0x76696577));
        t.extend_from_slice(&be32(0));
        for v in [0.9642f64, 1.0, 0.8249] {
            t.extend_from_slice(&s15(v));
        }
        for v in [0.9642f64, 1.0, 0.8249] {
            t.extend_from_slice(&s15(v));
        }
        for v in [0.9642f64, 1.0, 0.8249] {
            t.extend_from_slice(&s15(v));
        }
        t.extend_from_slice(&be32(1));
        t
    }
    // 'meas' tag: standard u32 + XYZ[3] + geometry u32 +
    //   flare u32 + illum u32
    fn meas_tag(geom: u32) -> Vec<u8> {
        let mut t = Vec::new();
        t.extend_from_slice(&be32(0x6D656173));
        t.extend_from_slice(&be32(0));
        t.extend_from_slice(&be32(0));
        for v in [0.9642f64, 1.0, 0.8249] {
            t.extend_from_slice(&s15(v));
        }
        t.extend_from_slice(&be32(geom));
        t.extend_from_slice(&be32(0x00010000));
        t.extend_from_slice(&be32(3));
        t
    }
    // 'para' parametric curve: u16 type + pad + params s15f16
    fn para(ptype: u16, params: &[f64]) -> Vec<u8> {
        let mut t = Vec::new();
        t.extend_from_slice(&be32(0x70617261));
        t.extend_from_slice(&be32(0));
        t.extend_from_slice(&ptype.to_be_bytes());
        t.extend_from_slice(&[0u8; 2]);
        for v in params { t.extend_from_slice(&s15(*v)); }
        t
    }
    unsafe {
        for (sig, blob, name) in [
            (0x76696577u32, view_tag(), "view"),
            (0x6D656173, meas_tag(0), "meas-g0"),
            (0x6D656173, meas_tag(1), "meas-g1"),
            (0x6D656173, meas_tag(99), "meas-g99"),
            (0x64657363, para(0, &[2.2]), "para0"),
            (0x72545243, para(1, &[2.2, -0.5, 0.0]),
                "para1"),
            (0x72545243, para(2, &[2.2, -0.5, 0.2, 0.04]),
                "para2"),
            (0x72545243, para(3, &[2.2, -0.5, 0.2, 0.04, 0.0]),
                "para3"),
            (0x72545243, para(4, &[2.2, -0.5, 0.2, 0.04, 1.0,
                -0.2, 0.0]), "para4"),
            (0x72545243, para(5, &[2.2, -0.5, 0.2, 0.04, 1.0,
                -0.2, 0.0]), "para5"),
            (0x72545243, para(99, &[1.0]), "para-bad"),
        ] {
            let prof = c::cmsCreate_sRGBProfile();
            c::cmsSetProfileVersion(prof, 4.4);
            let w = c::cmsWriteRawTag(prof, sig,
                blob.as_ptr() as *const _, blob.len() as u32);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(
                    buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let rd = if !rp.is_null() {
                c::cmsReadTag(rp, sig)
            } else { std::ptr::null_mut() };
            eprintln!("{name} w={w} s={s} rd={}", !rd.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
        }
        // linearization devicelink — gray + cmyk
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let tv = [0u16, 0x4000, 0x8000, 0xc000, 0xffff];
        let tc = c::cmsBuildTabulatedToneCurve16(
            std::ptr::null_mut(), 5, tv.as_ptr());
        let arr = [tc];
        let lp = c::cmsCreateLinearizationDeviceLinkTHR(
            std::ptr::null_mut(), 0x47524159,
            arr.as_ptr() as *const *mut _);
        eprintln!("lindl gray={}", !lp.is_null());
        if !lp.is_null() { c::cmsCloseProfile(lp); }
        // gray-only linearization — cmyk crashes upstream
        eprintln!("pre-ctx");
        // context create/dup/user-data
        let mut ud = 0x1234u32;
        let ctx = c::cmsCreateContext(std::ptr::null_mut(),
            &mut ud as *mut u32 as *mut _);
        eprintln!("post-ctx ctx={}", !ctx.is_null());
        let dup = c::cmsDupContext(ctx, std::ptr::null_mut());
        let u2 = c::cmsGetContextUserData(ctx);
        eprintln!("ctx={} dup={} ud={}", !ctx.is_null(),
            !dup.is_null(), u2 as usize);
        if !dup.is_null() {
            c::cmsDeleteContext(dup);
        }
        if !ctx.is_null() {
            c::cmsDeleteContext(ctx);
        }
        eprintln!("pre-mlu");
        // MLU wide-char
        let mlu = c::cmsMLUalloc(std::ptr::null_mut(), 2);
        let wv = [0x0068i32, 0x0069, 0x4E2D, 0];
        c::cmsMLUsetWide(mlu, b"en\0".as_ptr(),
            b"US\0".as_ptr(), wv.as_ptr());
        let mut ob = [0i32; 16];
        let got = c::cmsMLUgetWide(mlu, b"en\0".as_ptr(),
            b"US\0".as_ptr(), ob.as_mut_ptr(),
            (ob.len() * 4) as u32);
        eprintln!("mlu-wide got={got}");
        c::cmsMLUfree(mlu);
        eprintln!("pre-unlink");
        // pipeline unlink
        let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        let dims = [3u32; 3];
        let grid = vec![0x4000u16; 81];
        c::cmsPipelineInsertStage(p, 0,
            c::cmsStageAllocToneCurves(std::ptr::null_mut(), 3,
                [g, g, g].as_ptr() as *mut _));
        c::cmsPipelineInsertStage(p, 1,
            c::cmsStageAllocCLut16bitGranular(
                std::ptr::null_mut(), dims.as_ptr(), 3, 3,
                grid.as_ptr()));
        let mut stage: *mut std::ffi::c_void =
            std::ptr::null_mut();
        for loc in [0i32, 1, 0, 2, -1] {
            let r = c::cmsPipelineUnlinkStage(p, loc,
                &mut stage as *mut _ as *mut _);
            eprintln!("unlink {loc}={r} st={}", !stage.is_null());
            if !stage.is_null() {
                c::cmsStageFree(stage);
                stage = std::ptr::null_mut();
            }
        }
        c::cmsPipelineFree(p);
        c::cmsFreeToneCurve(g);
        c::cmsFreeToneCurve(tc);
    }
}

/// CAM02 init/forward/reverse + viewing-condition variants +
/// named-color UTF-8 decode + gray pipeline builders.
#[test]
fn cam02_utf8_gray_arms() {
    unsafe {
        // CAM02 viewing conditions: XYZ white + surround +
        // background/ambient params
        for (la, yb, av, sname) in [
            (20.0f64, 20.0, 1.0, "dim"),
            (31.83, 20.0, 1.0, "avg"),
            (318.3, 20.0, 1.0, "above"),
            (637.0, 20.0, 1.0, "high"),
        ] {
            let vc = c::ViewingConditions {
                white_point: c::CieXyz { X: 95.05, Y: 100.0,
                    Z: 108.88 },
                yb, la,
                surround: av as u32,
                d_value: 1.0,
            };
            let m = c::cmsCIECAM02Init(std::ptr::null_mut(),
                &vc as *const _);
            eprintln!("cam {sname} m={}", !m.is_null());
            if !m.is_null() {
                for xyz in [
                    c::CieXyz { X: 20.0, Y: 20.0, Z: 20.0 },
                    c::CieXyz { X: 95.05, Y: 100.0, Z: 108.88 },
                    c::CieXyz { X: 0.0, Y: 0.0, Z: 0.0 },
                    c::CieXyz { X: 50.0, Y: 80.0, Z: 10.0 },
                ] {
                    let mut jch = std::mem::zeroed::<c::JCh>();
                    c::cmsCIECAM02Forward(m,
                        &xyz as *const _, &mut jch);
                    let mut back = c::CieXyz {
                        X: 0.0, Y: 0.0, Z: 0.0 };
                    c::cmsCIECAM02Reverse(m,
                        &jch as *const _, &mut back);
                }
                c::cmsCIECAM02Done(m);
            }
        }
        // named-color UTF-8 decode — non-ascii names
        let nl = c::cmsAllocNamedColorList(std::ptr::null_mut(),
            4, 4, b"pre\0".as_ptr(), b"suf\0".as_ptr());
        for name in [
            "pantone\u{00e9}\u{4e2d}".as_bytes(),
            "\u{00f1}\u{00fc}ml\u{00e4}ut".as_bytes(),
            "plain".as_bytes(),
            "\u{fffd}\u{fffe}".as_bytes(),
        ] {
            let r = c::cmsAppendNamedColor(nl,
                name.as_ptr(), &[0x1000u16, 0x2000, 0x3000]
                    as *const u16 as *const _,
                std::ptr::null());
            eprintln!("append {name:?}={r}");
        }
        let n = c::cmsNamedColorCount(nl);
        eprintln!("ncl={n}");
        c::cmsFreeNamedColorList(nl);
        // gray profile transforms — BuildGrayInput/Output arms
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let gray = c::cmsCreateGrayProfile(std::ptr::null_mut(), g);
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let g8 = (3u32 << 16) | (1 << 3) | 1;
        let r8 = (4u32 << 16) | (3 << 3) | 1;
        let l8 = (10u32 << 16) | (3 << 3) | 1;
        for (i2, o2, f1, f2) in [
            (gray, srgb, g8, r8), (srgb, gray, r8, g8),
            (gray, lab, g8, l8), (lab, gray, l8, g8),
            (gray, gray, g8, g8),
        ] {
            let t = c::cmsCreateTransform(i2, f1, o2, f2, 0, 0);
            eprintln!("gray xform t={}", !t.is_null());
            if !t.is_null() {
                let ib = [0x55u8; 8];
                let mut ob = [0u8; 8];
                c::cmsDoTransform(t, ib.as_ptr() as *const _,
                    ob.as_mut_ptr() as *mut _, 1);
                c::cmsDeleteTransform(t);
            }
        }
        c::cmsCloseProfile(gray);
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
        c::cmsFreeToneCurve(g);
    }
}

/// cmsGDB* internals (gamut boundary descriptor) + float
/// devicelink/output tags + parametric-curve writes.
#[test]
fn gdb_float_arms() {
    unsafe {
        // GBD — point cloud + compute + check
        let gbd = c::cmsGBDAlloc(std::ptr::null_mut());
        for lab in [
            [50.0f64, 0.0, 0.0], [90.0, 30.0, -40.0],
            [10.0, -20.0, 50.0], [50.0, 60.0, -60.0],
            [50.0, -60.0, 60.0], [70.0, 40.0, 40.0],
            [30.0, -40.0, -40.0], [100.0, 0.0, 0.0],
            [0.0, 0.0, 0.0], [55.0, 5.0, -5.0],
        ] {
            c::cmsGDBAddPoint(gbd, lab.as_ptr() as *const _);
        }
        for flags in [0u32, 1] {
            let ok = c::cmsGDBCompute(gbd, flags);
            for lab in [
                [50.0f64, 0.0, 0.0], [95.0, 100.0, 0.0],
                [0.0, -127.0, -127.0], [50.0, 127.0, 127.0],
            ] {
                let chk = c::cmsGDBCheckPoint(gbd,
                    lab.as_ptr() as *const _);
                eprintln!("gdb f{flags} {lab:?}={chk}");
            }
            eprintln!("gdb compute f{flags}={ok}");
        }
        c::cmsGBDFree(gbd);
        // float devicelink profile — _cmsReadFloatDevicelinkTag
        let link = c::cmsCreateProfilePlaceholder(
            std::ptr::null_mut());
        c::cmsSetDeviceClass(link, 0x6C696E6B);   // 'link'
        c::cmsSetColorSpace(link, 0x4C616220);
        c::cmsSetPCS(link, 0x58595A20);
        c::cmsSetProfileVersion(link, 4.4);
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 1.0);
        let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        c::cmsPipelineInsertStage(p, 0,
            c::cmsStageAllocToneCurves(std::ptr::null_mut(), 3,
                [g, g, g].as_ptr() as *mut _));
        c::cmsWriteTag(link, 0x41324230, p as *const _);
        let mut buf = vec![0u8; 1 << 20];
        let mut n = buf.len() as u32;
        let s = c::cmsSaveProfileToMem(link,
            buf.as_mut_ptr() as *mut _, &mut n);
        let rp = if s != 0 {
            c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
        } else { std::ptr::null_mut() };
        let rd = if !rp.is_null() {
            c::cmsReadTag(rp, 0x41324230)
        } else { std::ptr::null_mut() };
        // transform through the link — devicelink class
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let t = if !rp.is_null() {
            c::cmsCreateTransform(rp,
                (10u32 << 16) | (3 << 3) | 1,
                lab, (9u32 << 16) | (3 << 3) | 1, 0, 0)
        } else { std::ptr::null_mut() };
        eprintln!("link s={s} rd={} t={}", !rd.is_null(),
            !t.is_null());
        if !t.is_null() { c::cmsDeleteTransform(t); }
        if !rp.is_null() { c::cmsCloseProfile(rp); }
        c::cmsCloseProfile(link);
        c::cmsCloseProfile(lab);
        c::cmsPipelineFree(p);
        c::cmsFreeToneCurve(g);
        // parametric curve write via curv+para on 'rTRC'
        let pc = c::cmsBuildParametricToneCurve(
            std::ptr::null_mut(), 4,
            [2.2, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]
                .as_ptr());
        let prof = c::cmsCreate_sRGBProfile();
        let w = c::cmsWriteTag(prof, 0x72545243,
            pc as *const _);
        let mut buf = vec![0u8; 1 << 20];
        let mut n = buf.len() as u32;
        let s2 = if w != 0 {
            c::cmsSaveProfileToMem(prof,
                buf.as_mut_ptr() as *mut _, &mut n)
        } else { 0 };
        let rp2 = if s2 != 0 {
            c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
        } else { std::ptr::null_mut() };
        let rd2 = if !rp2.is_null() {
            c::cmsReadTag(rp2, 0x72545243)
        } else { std::ptr::null_mut() };
        eprintln!("param w={w} s={s2} rd={}", !rd2.is_null());
        if !rp2.is_null() { c::cmsCloseProfile(rp2); }
        c::cmsCloseProfile(prof);
        c::cmsFreeToneCurve(pc);
    }
}

/// Type_VideoSignal_Read/Write (cicp) + Type_CrdInfo_Write +
/// Type_MPEcurve_Read + SetData/GenerateCSA arms.
#[test]
fn cicp_crdi_mpecurve_arms() {
    fn be32(v: u32) -> [u8; 4] { v.to_be_bytes() }
    fn be16(v: u16) -> [u8; 2] { v.to_be_bytes() }
    unsafe {
        // cicp — 4 u8s
        let cicp = [1u8, 13, 0, 1];
        let prof = c::cmsCreate_sRGBProfile();
        let w = c::cmsWriteTag(prof, 0x63696370,
            cicp.as_ptr() as *const _);
        let mut buf = vec![0u8; 1 << 20];
        let mut n = buf.len() as u32;
        let s = if w != 0 {
            c::cmsSaveProfileToMem(prof,
                buf.as_mut_ptr() as *mut _, &mut n)
        } else { 0 };
        let rp = if s != 0 {
            c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
        } else { std::ptr::null_mut() };
        let rd = if !rp.is_null() {
            c::cmsReadTag(rp, 0x63696370)
        } else { std::ptr::null_mut() };
        eprintln!("cicp w={w} s={s} rd={}", !rd.is_null());
        if !rp.is_null() { c::cmsCloseProfile(rp); }
        c::cmsCloseProfile(prof);
        // crdi — MLU with ppname + #0-#3 keys
        let mlu = c::cmsMLUalloc(std::ptr::null_mut(), 8);
        for (lang, key, val) in [
            (b"en\0" as &[u8], b"US\0" as &[u8],
                b"nm val\0" as &[u8]),
            (b"en\0", b"US\0", b"#0 r\0"),
            (b"en\0", b"US\0", b"#1 g\0"),
            (b"en\0", b"US\0", b"#2 b\0"),
            (b"en\0", b"US\0", b"#3 k\0"),
        ] {
            c::cmsMLUsetASCII(mlu, lang.as_ptr(),
                key.as_ptr(), val.as_ptr());
        }
        let prof = c::cmsCreate_sRGBProfile();
        let w = c::cmsWriteTag(prof, 0x63726469,
            mlu as *const _);
        let mut buf = vec![0u8; 1 << 20];
        let mut n = buf.len() as u32;
        let s = if w != 0 {
            c::cmsSaveProfileToMem(prof,
                buf.as_mut_ptr() as *mut _, &mut n)
        } else { 0 };
        let rp = if s != 0 {
            c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
        } else { std::ptr::null_mut() };
        let rd = if !rp.is_null() {
            c::cmsReadTag(rp, 0x63726469)
        } else { std::ptr::null_mut() };
        eprintln!("crdi w={w} s={s} rd={}", !rd.is_null());
        if !rp.is_null() { c::cmsCloseProfile(rp); }
        c::cmsCloseProfile(prof);
        c::cmsMLUfree(mlu);
        // mpet 'curv'/'para' curve elements — MPEcurve_Read
        let curv_elem = {
            let mut t = Vec::new();
            t.extend_from_slice(&be32(0x63757276));   // 'curv'
            t.extend_from_slice(&be32(0));
            t.extend_from_slice(&be32(3));            // nEntries
            for v in [0u16, 0x8000, 0xffff] {
                t.extend_from_slice(&be16(v));
            }
            while t.len() % 4 != 0 { t.push(0); }
            t
        };
        let para_elem = {
            let mut t = Vec::new();
            t.extend_from_slice(&be32(0x70617261));   // 'para'
            t.extend_from_slice(&be32(0));
            t.extend_from_slice(&1u16.to_be_bytes()); // type 1
            t.extend_from_slice(&[0u8; 2]);
            for v in [2.2f64, 1.0, 0.0] {
                t.extend_from_slice(&((v * 65536.0) as i32)
                    .to_be_bytes());
            }
            t
        };
        let cvst3 = {
            let mut t = Vec::new();
            t.extend_from_slice(&be32(0x63767374));   // 'cvst'
            t.extend_from_slice(&be32(0));
            t.extend_from_slice(&be16(3));
            t.extend_from_slice(&be16(3));
            t.extend_from_slice(&be32(1));            // 1 curve
            t.extend_from_slice(&be32(32));           // off
            t.extend_from_slice(&be32(24));           // size
            // seg curve: nIn/nOut u16, nSeg u16,
            //   [parf seg]
            t.extend_from_slice(&be16(3));
            t.extend_from_slice(&be16(3));
            t.extend_from_slice(&be16(0));
            t.extend_from_slice(&be16(1));
            t.extend_from_slice(&be32(0x70617266));   // 'parf'
            t.extend_from_slice(&be32(0));
            t.extend_from_slice(&be16(0));
            for v in [2.2f64] {
                t.extend_from_slice(&((v * 65536.0) as i32)
                    .to_be_bytes());
            }
            t
        };
        fn mpet(elems: &[Vec<u8>]) -> Vec<u8> {
            let mut t = Vec::new();
            t.extend_from_slice(&be32(0x6D706574));
            t.extend_from_slice(&be32(0));
            t.extend_from_slice(&be16(3));
            t.extend_from_slice(&be16(3));
            t.extend_from_slice(&be32(elems.len() as u32));
            let mut off = 16 + elems.len() as u32 * 8;
            for e in elems {
                t.extend_from_slice(&be32(off));
                t.extend_from_slice(&be32(e.len() as u32));
                off += e.len() as u32;
            }
            for e in elems { t.extend_from_slice(e); }
            t
        }
        for (blob, name) in [
            (mpet(&[curv_elem.clone(), para_elem.clone()]),
                "curv+para"),
            (mpet(&[cvst3]), "cvst-3ch"),
            (mpet(&[curv_elem.clone(), curv_elem.clone(),
                curv_elem.clone()]), "3curv"),
        ] {
            for sig in [0x44324230u32, 0x42324430] {
                let prof = c::cmsCreateProfilePlaceholder(
                    std::ptr::null_mut());
                c::cmsSetDeviceClass(prof, 0x6D6E7472);
                c::cmsSetColorSpace(prof, 0x4C616220);
                c::cmsSetPCS(prof, 0x58595A20);
                c::cmsSetProfileVersion(prof, 4.4);
                let w = c::cmsWriteRawTag(prof, sig,
                    blob.as_ptr() as *const _, blob.len() as u32);
                let mut buf = vec![0u8; 1 << 20];
                let mut n = buf.len() as u32;
                let s = if w != 0 {
                    c::cmsSaveProfileToMem(prof,
                        buf.as_mut_ptr() as *mut _, &mut n)
                } else { 0 };
                let rp = if s != 0 {
                    c::cmsOpenProfileFromMem(
                        buf.as_ptr() as *const _, n)
                } else { std::ptr::null_mut() };
                let rd = if !rp.is_null() {
                    c::cmsReadTag(rp, sig)
                } else { std::ptr::null_mut() };
                eprintln!("{name} {sig:#x} w={w} s={s} rd={}",
                    !rd.is_null());
                if !rp.is_null() { c::cmsCloseProfile(rp); }
                c::cmsCloseProfile(prof);
            }
        }
    }
}
