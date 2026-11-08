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
