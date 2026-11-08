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
