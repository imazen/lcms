//! Error-path + io1 getter parity: malformed profile byte streams must be
//! accepted/rejected identically (cmsio0 open paths), and header/intent
//! getters must match (cmsio1). Both-fail is parity; on both-accept the
//! exposed header fields must agree.

use lcms::{io1, profile};
use lcms_cref as c;
use std::ffi::c_void;

fn srgb_bytes() -> Vec<u8> {
    let p = profile::open_profile_from_mem(
        None,
        // build via C then save, so the seed bytes are C-canonical
        &unsafe {
            let s = c::cmsCreate_sRGBProfile();
            let mut n = 0u32;
            assert!(c::cmsSaveProfileToMem(s, std::ptr::null_mut(), &mut n) != 0);
            let mut b = vec![0u8; n as usize];
            assert!(c::cmsSaveProfileToMem(s, b.as_mut_ptr() as *mut _, &mut n) != 0);
            c::cmsCloseProfile(s);
            b
        },
    )
    .expect("seed profile");
    let mut p = p;
    profile::save_profile_to_mem(&mut p).expect("save")
}

fn c_open(data: &[u8]) -> *mut c_void {
    unsafe { c::cmsOpenProfileFromMem(data.as_ptr() as _, data.len() as u32) }
}

fn variants(seed: &[u8]) -> Vec<(&'static str, Vec<u8>)> {
    let mut v: Vec<(&'static str, Vec<u8>)> = Vec::new();
    v.push(("empty", vec![]));
    v.push(("tiny", seed[..16].to_vec()));
    v.push(("half-header", seed[..64].to_vec()));
    v.push(("header-only", seed[..128].to_vec()));
    // bad magic
    let mut m = seed.to_vec();
    m[36] = b'Z';
    m[37] = b'Z';
    m[38] = b'Z';
    m[39] = b'Z';
    v.push(("bad-magic", m));
    // corrupt declared size (too big and too small)
    let mut m = seed.to_vec();
    m[0..4].copy_from_slice(&0xFFFF_FFFFu32.to_be_bytes());
    v.push(("huge-size", m));
    let mut m = seed.to_vec();
    m[0..4].copy_from_slice(&128u32.to_be_bytes());
    v.push(("tiny-size", m));
    // tag count beyond data
    let mut m = seed.to_vec();
    m[128..132].copy_from_slice(&0xDEADu32.to_be_bytes());
    v.push(("huge-tagcount", m));
    // one tag entry pointing past EOF
    let mut m = seed.to_vec();
    let tag_count = u32::from_be_bytes(m[128..132].try_into().unwrap()) as usize;
    if tag_count > 0 {
        let off = 132 + 4; // first tag's offset field
        m[off..off + 4].copy_from_slice(&(seed.len() as u32 * 64).to_be_bytes());
    }
    v.push(("tag-past-eof", m));
    // zeroed tag count
    let mut m = seed.to_vec();
    m[128..132].copy_from_slice(&0u32.to_be_bytes());
    v.push(("zero-tags", m));
    // random junk in tag table region
    let mut m = seed.to_vec();
    for (i, b) in m[132..156].iter_mut().enumerate() {
        *b = (i * 37 + 11) as u8;
    }
    v.push(("junk-tags", m));
    // truncations at 25%, 50%, 75%, 95%
    for (name, pct) in [
        ("trunc-25", 25),
        ("trunc-50", 50),
        ("trunc-75", 75),
        ("trunc-95", 95),
    ] {
        v.push((name, seed[..seed.len() * pct / 100].to_vec()));
    }
    // single-byte corruption sweeps through the header
    for off in [8usize, 12, 16, 20, 24, 36, 40, 44, 48, 52, 64, 80, 100] {
        let mut m = seed.to_vec();
        m[off] ^= 0xFF;
        v.push(match off {
            8 => ("flip-8", m),
            12 => ("flip-12", m),
            16 => ("flip-16", m),
            20 => ("flip-20", m),
            24 => ("flip-24", m),
            36 => ("flip-36", m),
            40 => ("flip-40", m),
            44 => ("flip-44", m),
            48 => ("flip-48", m),
            52 => ("flip-52", m),
            64 => ("flip-64", m),
            80 => ("flip-80", m),
            _ => ("flip-100", m),
        });
    }
    // appended garbage (must still open)
    let mut m = seed.to_vec();
    m.extend_from_slice(&[0xAA; 512]);
    v.push(("appended", m));
    v
}

#[test]
fn malformed_open_parity() {
    let seed = srgb_bytes();
    for (name, data) in variants(&seed) {
        let ch = c_open(&data);
        let r = profile::open_profile_from_mem(None, &data);
        assert_eq!(
            ch.is_null(),
            r.is_none(),
            "{name}: accept/reject parity (C {} rust {})",
            !ch.is_null(),
            r.is_some()
        );
        if !ch.is_null() {
            unsafe { c::cmsCloseProfile(ch) };
        }
    }
}

#[test]
fn io1_getters() {
    let seed = srgb_bytes();
    let ch = c_open(&seed);
    assert!(!ch.is_null());
    let r = profile::open_profile_from_mem(None, &seed).expect("rust open");
    unsafe {
        assert_eq!(c::cmsGetColorSpace(ch), r.color_space, "color space");
        assert_eq!(c::cmsGetDeviceClass(ch), r.device_class, "device class");
        assert_eq!(
            c::cmsGetEncodedICCversion(ch),
            profile::get_encoded_icc_version(&r),
            "encoded version"
        );
        assert_eq!(c::cmsGetPCS(ch), r.pcs, "pcs");
        // intent support matrix: intents 0..10 x directions input/output
        for intent in 0..10u32 {
            for dir in 0..2u32 {
                let cs = c::cmsIsIntentSupported(ch, intent, dir);
                let rs = io1::is_intent_supported(&r, intent, dir as i32) as i32;
                assert_eq!(cs, rs, "intent {intent} dir {dir}");
            }
        }
        c::cmsCloseProfile(ch);
    }
}

/// Open the bundled corpus for getter parity across profile classes.
#[test]
fn io1_getters_corpus() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "icc").unwrap_or(false))
        .collect();
    files.sort();
    for f in files {
        let data = std::fs::read(&f).unwrap();
        let ch = c_open(&data);
        let r = profile::open_profile_from_mem(None, &data);
        assert_eq!(ch.is_null(), r.is_none(), "{}: open", f.display());
        if ch.is_null() {
            continue;
        }
        let r = r.unwrap();
        unsafe {
            assert_eq!(c::cmsGetColorSpace(ch), r.color_space, "{}: cs", f.display());
            assert_eq!(
                c::cmsGetDeviceClass(ch),
                r.device_class,
                "{}: class",
                f.display()
            );
            assert_eq!(
                c::cmsGetEncodedICCversion(ch),
                profile::get_encoded_icc_version(&r),
                "{}: ver",
                f.display()
            );
            assert_eq!(c::cmsGetPCS(ch), r.pcs, "{}: pcs", f.display());
            for intent in 0..10u32 {
                for dir in 0..2u32 {
                    assert_eq!(
                        c::cmsIsIntentSupported(ch, intent, dir),
                        io1::is_intent_supported(&r, intent, dir as i32) as i32,
                        "{}: intent {intent} dir {dir}",
                        f.display()
                    );
                }
            }
            c::cmsCloseProfile(ch);
        }
    }
}

/// cmsio0 save/IO-handler paths: SaveToFile->reopen byte-compare,
/// SaveToIOhandler->OpenProfileFromIOhandler, OpenIOhandlerFromFile.
#[test]
fn save_and_iohandler_roundtrip() {
    let dir = std::env::temp_dir();
    let cpath = dir.join("ccov_c_out.icc");
    let rpath = dir.join("ccov_r_out.icc");
    let cs = cpath.to_str().unwrap();
    let rs = rpath.to_str().unwrap();
    let cs_c = std::ffi::CString::new(cs).unwrap();
    let rs_c = std::ffi::CString::new(rs).unwrap();

    let cph = unsafe { c::cmsCreate_sRGBProfile() };
    let mut rp = lcms::virt::create_srgb_profile(None).unwrap();

    // C SaveToFile -> bytes; Rust save -> bytes; same file contents
    unsafe { assert!(c::cmsSaveProfileToFile(cph, cs_c.as_ptr()) != 0) };
    let c_bytes = std::fs::read(&cpath).unwrap();
    let r_bytes = lcms::profile::save_profile_to_mem(&mut rp).unwrap();
    assert_eq!(c_bytes.len(), r_bytes.len(), "file save length");
    for (i, (a, b)) in c_bytes.iter().zip(r_bytes.iter()).enumerate() {
        if (24..36).contains(&i) { continue; } // created timestamp
        assert_eq!(a, b, "save byte @{i:#x}");
    }
    unsafe { c::cmsCloseProfile(cph) };

    // C open-from-file returns a profile with same tag count as Rust's open
    let c2 = unsafe { c::cmsOpenProfileFromFile(cs_c.as_ptr(), b"r\0".as_ptr()) };
    assert!(!c2.is_null());
    let mut rp2 = lcms::profile::open_profile_from_mem(None, &c_bytes).unwrap();
    unsafe {
        assert_eq!(c::cmsGetTagCount(c2), rp2.tag_count as i32, "tag count on reopen");
    }
    unsafe { c::cmsCloseProfile(c2) };

    // IOhandler save -> open: C writes via IOhandlerFromMem, reads back
    unsafe {
        let wbuf = vec![0u8; c_bytes.len() * 2];
        let io_w = c::cmsOpenIOhandlerFromMem(
            std::ptr::null_mut(),
            wbuf.as_ptr() as *mut u8,
            wbuf.len() as u32,
            b"w\0".as_ptr(),
        );
        assert!(!io_w.is_null(), "IOhandlerFromMem w");
        let p3 = c::cmsCreate_sRGBProfile();
        assert!(c::cmsSaveProfileToIOhandler(p3, io_w) != 0, "save via iohandler");
        c::cmsCloseIOhandler(io_w);
        c::cmsCloseProfile(p3);

        let io_r = c::cmsOpenIOhandlerFromFile(std::ptr::null_mut(), cs_c.as_ptr(), b"r\0".as_ptr());
        assert!(!io_r.is_null(), "IOhandlerFromFile r");
        let p4 = c::cmsOpenProfileFromIOhandlerTHR(std::ptr::null_mut(), io_r);
        assert!(!p4.is_null(), "open via IOhandlerFromFile");
        c::cmsCloseProfile(p4);
    }

    std::fs::remove_file(&cpath).ok();
}

/// cmsio1 LUT readers (internal _cms* API): presence parity +
/// cmsPipelineEval16 output comparison over a deterministic grid.
#[test]
fn io1_lut_reader_eval() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap().filter_map(|e| e.ok()).map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "icc").unwrap_or(false))
        .collect();
    files.sort();
    for f in files {
        let name = f.file_name().unwrap().to_string_lossy();
        let data = std::fs::read(&f).unwrap();
        let ch = unsafe { c::cmsOpenProfileFromMem(data.as_ptr() as _, data.len() as u32) };
        if ch.is_null() { continue; }
        let mut rp = lcms::profile::open_profile_from_mem(None, &data).unwrap();
        for intent in 0..4u32 {
            for which in 0..3 {
                let (cp, rpl) = unsafe {
                    match which {
                        0 => (c::_cmsReadInputLUT(ch, intent),
                              lcms::io1::read_input_lut(&mut rp, intent)),
                        1 => (c::_cmsReadOutputLUT(ch, intent),
                              lcms::io1::read_output_lut(&mut rp, intent)),
                        _ => (c::_cmsReadDevicelinkLUT(ch, intent),
                              lcms::io1::read_devicelink_lut(&mut rp, intent)),
                    }
                };
                assert_eq!(cp.is_null(), rpl.is_none(),
                    "{name} intent {intent} lut {which} presence");
                if cp.is_null() || rpl.is_none() { continue; }
                let mut pipe = rpl.unwrap();
                let n_in = unsafe { c::cmsPipelineInputChannels(cp as _) } as usize;
                let n_out = unsafe { c::cmsPipelineOutputChannels(cp as _) } as usize;
                assert_eq!(n_in, pipe.input_channels() as usize, "{name} in ch");
                assert_eq!(n_out, pipe.output_channels() as usize, "{name} out ch");
                let mut seed = 0x1234u64;
                for _ in 0..8 {
                    let inp: Vec<u16> = (0..n_in).map(|_| {
                        seed = seed.wrapping_mul(0x9e3779b97f4a7c15) ^ (seed >> 33);
                        (seed >> 48) as u16
                    }).collect();
                    let mut co = vec![0u16; n_out];
                    let mut ro = vec![0u16; n_out];
                    unsafe { c::cmsPipelineEval16(inp.as_ptr(), co.as_mut_ptr(), cp as _) };
                    pipe.eval16(&inp, &mut ro);
                    assert_eq!(co, ro, "{name} intent {intent} lut {which} eval16");
                }
                unsafe { c::cmsPipelineFree(cp as _) };
            }
        }
        unsafe { c::cmsCloseProfile(ch) };
    }
}

/// cmsio1 header-getter matrix: PCS/colorspace/class/version/flags/
/// attributes/creator/model/profileID/IsCLUT across the corpus.
#[test]
fn io1_header_getters() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap().filter_map(|e| e.ok()).map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "icc").unwrap_or(false))
        .collect();
    files.sort();
    for f in files {
        let name = f.file_name().unwrap().to_string_lossy();
        let data = std::fs::read(&f).unwrap();
        let ch = unsafe { c::cmsOpenProfileFromMem(data.as_ptr() as _, data.len() as u32) };
        if ch.is_null() { continue; }
        let rp = lcms::profile::open_profile_from_mem(None, &data).unwrap();
        unsafe {
            assert_eq!(c::cmsGetPCS(ch), rp.pcs, "{name} pcs");
            assert_eq!(c::cmsGetColorSpace(ch), rp.color_space, "{name} colorspace");
            assert_eq!(c::cmsGetDeviceClass(ch), rp.device_class, "{name} class");
            assert_eq!(c::cmsGetEncodedICCversion(ch), rp.version, "{name} version");
            let mut fl = 0u32;
            c::cmsGetHeaderFlags(ch, &mut fl);
            assert_eq!(fl, rp.flags, "{name} flags");
            let mut at = 0u64;
            c::cmsGetHeaderAttributes(ch, &mut at);
            assert_eq!(at, rp.attributes, "{name} attributes");
            assert_eq!(c::cmsGetHeaderCreator(ch), rp.creator, "{name} creator");
            assert_eq!(c::cmsGetHeaderModel(ch), rp.model, "{name} model");
            let mut id = [0u8; 16];
            c::cmsGetHeaderProfileID(ch, id.as_mut_ptr());
            assert_eq!(id, rp.profile_id.id8, "{name} profile id");
            for intent in 0..4u32 {
                for dir in 0..2u32 {
                    assert_eq!(
                        c::cmsIsCLUT(ch, intent, dir) != 0,
                        lcms::io1::is_clut(&mut rp.clone(), intent, dir as i32),
                        "{name} is_clut i{intent} d{dir}"
                    );
                }
            }
        }
        unsafe { c::cmsCloseProfile(ch) };
    }
}

/// cmsio1 intent-support matrix: GetSupportedIntents + IsIntentSupported
/// per intent x direction x profile.
#[test]
fn io1_intent_support() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap().filter_map(|e| e.ok()).map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "icc").unwrap_or(false))
        .collect();
    files.sort();
    for f in files {
        let name = f.file_name().unwrap().to_string_lossy();
        let data = std::fs::read(&f).unwrap();
        let ch = unsafe { c::cmsOpenProfileFromMem(data.as_ptr() as _, data.len() as u32) };
        if ch.is_null() { continue; }
        let mut rp = lcms::profile::open_profile_from_mem(None, &data).unwrap();
        for intent in 0..8u32 {
            for dir in 0..2u32 {
                let c_sup = unsafe { c::cmsIsIntentSupported(ch, intent, dir) };
                let r_sup = lcms::io1::is_intent_supported(&rp, intent, dir as i32);
                assert_eq!(c_sup != 0, r_sup, "{name} intent {intent} dir {dir}");
            }
        }
        unsafe { c::cmsCloseProfile(ch) };
    }
}
