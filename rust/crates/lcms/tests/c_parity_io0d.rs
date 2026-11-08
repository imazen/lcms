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
            ("mntr", 0x6D6E7472u32), ("prtr", 0x70727472),
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
            ("labv4", b"../../labv4lcms2.icc\0"),
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
            ("labv4", b"../../labv4lcms2.icc\0", 3),
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
            ("labv4", b"../../labv4lcms2.icc\0", 3),
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
            ("labv4", b"../../labv4lcms2.icc\0"),
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
            ("labv4", b"../../labv4lcms2.icc\0"),
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
