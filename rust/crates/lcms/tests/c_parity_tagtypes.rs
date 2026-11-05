//! Differential test: `lcms::tagtypes` vs `cmstypes.c` — byte-exact
//! serialization both directions via the type-handler ABI.

use lcms::named::*;
use lcms::tagtypes::*;
use lcms_cref as c;
use std::ptr;

/// Get C's handler for a type signature.
fn c_handler(sig: u32) -> *mut c::CmsTagTypeHandler {
    let h = unsafe { c::_cmsGetTagTypeHandler(ptr::null_mut(), sig) };
    assert!(!h.is_null(), "no C handler for {sig:#x}");
    h
}

/// Get Rust's handler.
fn r_handler(sig: u32) -> TagTypeHandler {
    get_tag_type_handler(None, sig).expect("no rust handler")
}

/// Call C WritePtr on `obj` (C-side void*), return the bytes.
fn c_write(sig: u32, obj: *const u8, n_items: u32) -> Vec<u8> {
    let mut buf = vec![0u8; 65536];
    unsafe {
        let io = c::cmsOpenIOhandlerFromMem(
            ptr::null_mut(),
            buf.as_mut_ptr(),
            buf.len() as u32,
            b"w\0".as_ptr(),
        );
        let h = c_handler(sig);
        let ok = ((*h).write_ptr)(h, io, obj as *mut std::ffi::c_void, n_items);
        assert_eq!(ok, 1, "C write {sig:#x}");
        let used = (*(io as *mut CIoUsed)).used_space;
        c::cmsCloseIOhandler(io);
        buf.truncate(used as usize);
        buf
    }
}

#[repr(C)]
struct CIoUsed {
    stream: *mut std::ffi::c_void,
    context_id: c::Context,
    used_space: u32,
    reported_size: u32,
}

/// Call C ReadPtr on `bytes` (the tag BODY, post-type-base), return the raw
/// object pointer + nItems — caller must free via FreePtr or reuse via Write.
unsafe fn c_read(sig: u32, bytes: &[u8]) -> (c::Handle, u32) {
    let mut buf = bytes.to_vec();
    let io = c::cmsOpenIOhandlerFromMem(
        ptr::null_mut(),
        buf.as_mut_ptr(),
        buf.len() as u32,
        b"r\0".as_ptr(),
    );
    let h = c_handler(sig);
    let mut n_items = 0u32;
    let obj = ((*h).read_ptr)(h, io, &mut n_items, bytes.len() as u32);
    c::cmsCloseIOhandler(io);
    std::mem::forget(buf);
    (obj, n_items)
}

/// Call Rust read on `bytes`, then Rust write — return output bytes + nItems.
fn r_read_write(sig: u32, bytes: &[u8]) -> (Vec<u8>, u32) {
    let mut io = lcms::io::io_handler_from_mem(None, bytes.to_vec(), "r").unwrap();
    let h = r_handler(sig);
    let mut n_items = 0u32;
    let obj = (h.read)(&h, &mut io, &mut n_items, bytes.len() as u32).expect("rust read failed");
    let mut out = vec![0u8; 65536];
    let mut wio = lcms::io::io_handler_from_mem(None, std::mem::take(&mut out), "w").unwrap();
    assert!(
        (h.write)(&h, &mut wio, &obj, n_items),
        "rust write {sig:#x}"
    );
    let lcms::io::IoHandler {
        stream: lcms::io::IoStream::Mem(m),
        ..
    } = wio
    else {
        panic!()
    };
    let used = m.pointer as usize;
    let mut out = m.block;
    out.truncate(used);
    (out, n_items)
}

#[test]
fn xyz_chromaticity_signature() {
    let xyz = lcms::types::CieXyz {
        X: 0.9642,
        Y: 1.0,
        Z: 0.8249,
    };
    // C side
    let c_bytes = c_write(
        lcms::signatures::tagtype::SigXYZType,
        &xyz as *const _ as *const u8,
        1,
    );
    // Rust side
    let mut buf = vec![0u8; 256];
    let mut io = lcms::io::io_handler_from_mem(None, std::mem::take(&mut buf), "w").unwrap();
    let h = r_handler(lcms::signatures::tagtype::SigXYZType);
    assert!((h.write)(&h, &mut io, &TagObject::Xyz(xyz), 1));
    let lcms::io::IoHandler {
        stream: lcms::io::IoStream::Mem(m),
        ..
    } = io
    else {
        panic!()
    };
    let r_bytes = m.block[..m.pointer as usize].to_vec();
    assert_eq!(r_bytes, c_bytes, "XYZ bytes");

    // read back through rust
    let (rt, n) = r_read_write(lcms::signatures::tagtype::SigXYZType, &c_bytes);
    assert_eq!(n, 1);
    assert_eq!(rt, c_bytes, "XYZ rust-read-write roundtrip");

    // Signature
    let sig: u32 = 0x6c6d7320;
    let c_sig = c_write(
        lcms::signatures::tagtype::SigSignatureType,
        &sig as *const _ as *const u8,
        1,
    );
    let (r_sig, n) = r_read_write(lcms::signatures::tagtype::SigSignatureType, &c_sig);
    assert_eq!(n, 1);
    assert_eq!(r_sig, c_sig, "signature roundtrip");
}

#[test]
fn int_and_fixed_arrays() {
    let mut s = 3u64;
    let mut rng = move || {
        s = s
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        s >> 11
    };
    for n in [1usize, 7, 64] {
        // u8 array
        let a8: Vec<u8> = (0..n).map(|_| rng() as u8).collect();
        let cb = c_write(
            lcms::signatures::tagtype::SigUInt8ArrayType,
            a8.as_ptr() as *const u8,
            n as u32,
        );
        let (rb, items) = r_read_write(lcms::signatures::tagtype::SigUInt8ArrayType, &cb);
        assert_eq!((items, rb), (n as u32, cb.clone()), "u8 array n={n}");

        // u32
        let a32: Vec<u32> = (0..n).map(|_| rng() as u32).collect();
        let cb = c_write(
            lcms::signatures::tagtype::SigUInt32ArrayType,
            a32.as_ptr() as *const u8,
            n as u32,
        );
        let (rb, items) = r_read_write(lcms::signatures::tagtype::SigUInt32ArrayType, &cb);
        assert_eq!((items, rb), (n as u32, cb.clone()), "u32 array n={n}");

        // u64
        let a64: Vec<u64> = (0..n).map(|_| rng()).collect();
        let cb = c_write(
            lcms::signatures::tagtype::SigUInt64ArrayType,
            a64.as_ptr() as *const u8,
            n as u32,
        );
        let (rb, items) = r_read_write(lcms::signatures::tagtype::SigUInt64ArrayType, &cb);
        assert_eq!((items, rb), (n as u32, cb.clone()), "u64 array n={n}");

        // s15fixed16 (doubles)
        let af: Vec<f64> = (0..n)
            .map(|_| (rng() % 10000) as f64 / 17.0 - 250.0)
            .collect();
        let cb = c_write(
            lcms::signatures::tagtype::SigS15Fixed16ArrayType,
            af.as_ptr() as *const u8,
            n as u32,
        );
        let (rb, items) = r_read_write(lcms::signatures::tagtype::SigS15Fixed16ArrayType, &cb);
        assert_eq!((items, rb), (n as u32, cb.clone()), "s15f16 n={n}");

        // u16fixed16
        let cb = c_write(
            lcms::signatures::tagtype::SigU16Fixed16ArrayType,
            af.as_ptr() as *const u8,
            n as u32,
        );
        let (rb, items) = r_read_write(lcms::signatures::tagtype::SigU16Fixed16ArrayType, &cb);
        assert_eq!((items, rb), (n as u32, cb.clone()), "u16f16 n={n}");
    }
}

#[test]
fn text_and_desc_mlu() {
    unsafe {
        for text in ["hello", "ICC profile © 2024 — unicode test", "", "x"] {
            // Build identical MLUs on both sides
            let c_mlu = c::cmsMLUalloc(ptr::null_mut(), 1);
            let txt = std::ffi::CString::new(text).unwrap();
            c::cmsMLUsetASCII(
                c_mlu,
                b"\0\0".as_ptr(),
                b"\0\0".as_ptr(),
                txt.as_ptr() as *const u8,
            );
            let mut r_mlu = mlu_alloc(None, 1).unwrap();
            assert!(mlu_set_ascii(&mut r_mlu, b"\0\0", b"\0\0", txt.as_bytes()));

            for sig in [
                lcms::signatures::tagtype::SigTextType,
                lcms::signatures::tagtype::SigTextDescriptionType,
            ] {
                let cb = c_write(sig, c_mlu as *const u8, 1);
                // rust write
                let h = r_handler(sig);
                let obj = if sig == lcms::signatures::tagtype::SigTextType {
                    TagObject::Text(r_mlu.clone())
                } else {
                    TagObject::TextDescription(r_mlu.clone())
                };
                let mut io = lcms::io::io_handler_from_mem(None, vec![0u8; 4096], "w").unwrap();
                assert!(
                    (h.write)(&h, &mut io, &obj, 1),
                    "rust write {sig:#x} '{text}'"
                );
                let lcms::io::IoHandler {
                    stream: lcms::io::IoStream::Mem(m),
                    ..
                } = io
                else {
                    panic!()
                };
                let rb = m.block[..m.pointer as usize].to_vec();
                assert_eq!(rb, cb, "{sig:#x} '{text}' bytes");

                // rust read of C bytes → re-write → identical
                let (rt, _) = r_read_write(sig, &cb);
                assert_eq!(rt, cb, "{sig:#x} '{text}' roundtrip");
            }
            c::cmsMLUfree(c_mlu);
        }
    }
}

#[test]
fn curve_types() {
    unsafe {
        // parametric type 1 (gamma)
        let params = [2.2f64];
        let c_curve = c::cmsBuildParametricToneCurve(ptr::null_mut(), 1, params.as_ptr());
        let cb = c_write(
            lcms::signatures::tagtype::SigCurveType,
            c_curve as *const u8,
            1,
        );
        let (rb, n) = r_read_write(lcms::signatures::tagtype::SigCurveType, &cb);
        assert_eq!(n, 1);
        assert_eq!(rb, cb, "curve gamma-1");

        // parametric curve type — write through C, read through rust
        let p4 = [2.4f64, 1.0 / 1.055, 0.055 / 1.055, 1.0 / 12.92, 0.04045];
        let c_pc = c::cmsBuildParametricToneCurve(ptr::null_mut(), 4, p4.as_ptr());
        let cb = c_write(
            lcms::signatures::tagtype::SigParametricCurveType,
            c_pc as *const u8,
            1,
        );
        let (rb, n) = r_read_write(lcms::signatures::tagtype::SigParametricCurveType, &cb);
        assert_eq!(n, 1);
        assert_eq!(rb, cb, "parametric curve type 4");

        // tabulated
        let tab: Vec<u16> = (0..256).map(|i| (i * 257) as u16).collect();
        let c_tab = c::cmsBuildTabulatedToneCurve16(ptr::null_mut(), 256, tab.as_ptr());
        let cb = c_write(
            lcms::signatures::tagtype::SigCurveType,
            c_tab as *const u8,
            1,
        );
        let (rb, n) = r_read_write(lcms::signatures::tagtype::SigCurveType, &cb);
        assert_eq!(n, 1);
        assert_eq!(rb, cb, "tabulated curve 256");

        c::cmsFreeToneCurve(c_curve);
        c::cmsFreeToneCurve(c_pc);
        c::cmsFreeToneCurve(c_tab);
    }
}

#[test]
fn datetime_measurement_data() {
    // DateTime — C uses struct tm; craft a known timestamp block
    let bytes: [u8; 12] = [
        0x07, 0xE7, 0x00, 0x05, 0x00, 0x11, 0x00, 0x0A, 0x00, 0x1E, 0x00, 0x2D,
    ];
    let (rb, n) = r_read_write(lcms::signatures::tagtype::SigDateTimeType, &bytes);
    assert_eq!(n, 1);
    assert_eq!(rb, bytes, "datetime roundtrip");

    // Measurement — craft a block
    let mut mblock = Vec::new();
    mblock.extend_from_slice(&0u32.to_be_bytes()); // observer
    let xyz = lcms::types::CieXyz {
        X: 0.1,
        Y: 0.2,
        Z: 0.3,
    };
    let mut io = lcms::io::io_handler_from_mem(None, vec![0u8; 64], "w").unwrap();
    lcms::io::write_xyz(&mut io, &xyz);
    let lcms::io::IoHandler {
        stream: lcms::io::IoStream::Mem(m),
        ..
    } = io
    else {
        panic!()
    };
    mblock.extend_from_slice(&m.block[..12]);
    mblock.extend_from_slice(&1u32.to_be_bytes()); // geometry
    let flare: u32 = 0x00010000; // 1.0 in s15fixed16
    mblock.extend_from_slice(&flare.to_be_bytes());
    mblock.extend_from_slice(&0u32.to_be_bytes()); // illuminant D50
    let (rb, n) = r_read_write(lcms::signatures::tagtype::SigMeasurementType, &mblock);
    assert_eq!(n, 1);
    assert_eq!(rb, mblock, "measurement roundtrip");

    // Data type
    let mut dblock = Vec::new();
    dblock.extend_from_slice(&0xDEADBEEFu32.to_be_bytes());
    dblock.extend_from_slice(b"binary payload \x00\x01\x02");
    let (rb, n) = r_read_write(lcms::signatures::tagtype::SigDataType, &dblock);
    assert_eq!(n, 1);
    assert_eq!(rb, dblock, "data roundtrip");
}

/// C read → C write round-trip on a tag body — the oracle path.
/// Returns (bytes, n_items); n_items=0 on read failure.
fn c_read_write(sig: u32, bytes: &[u8]) -> (Vec<u8>, u32) {
    unsafe {
        let (obj, n_items) = c_read(sig, bytes);
        if obj.is_null() {
            return (Vec::new(), 0);
        }
        let out = c_write(sig, obj as *const u8, n_items);
        let h = c_handler(sig);
        ((*h).free_ptr)(h, obj);
        (out, n_items)
    }
}

/// Compare C read→write vs Rust read→write on the same body bytes.
fn cross_roundtrip(sig: u32, body: &[u8]) {
    let (c_bytes, c_n) = c_read_write(sig, body);
    let (r_bytes, r_n) = r_read_write(sig, body);
    assert_eq!(r_n, c_n, "n_items {sig:#x}");
    if c_n > 0 {
        assert_eq!(r_bytes, c_bytes, "bytes {sig:#x}");
    }
}

fn be32(v: u32) -> [u8; 4] {
    v.to_be_bytes()
}
fn be16(v: u16) -> [u8; 2] {
    v.to_be_bytes()
}

#[test]
fn mlu_multi_entry() {
    // craft an MLU tag body: 2 entries (en/US "hello", de/DE "welt&")
    let strings: Vec<(u16, u16, &[u16])> = vec![
        (0x656e, 0x5553, &[0x68, 0x65, 0x6c, 0x6c, 0x6f, 0]), // en US "hello\0"
        (0x6465, 0x4445, &[0x77, 0x65, 0x6c, 0x74, 0x26, 0]), // de DE "welt&\0"
    ];
    let mut body = Vec::new();
    body.extend_from_slice(&be32(2)); // count
    body.extend_from_slice(&be32(12)); // reclen
    // offsets are absolute incl 8-byte type base
    let mut off = 8 + 8 + 12 * 2u32;
    for (lang, country, utf16) in &strings {
        body.extend_from_slice(&be16(*lang));
        body.extend_from_slice(&be16(*country));
        body.extend_from_slice(&be32((utf16.len() * 2) as u32));
        body.extend_from_slice(&be32(off));
        off += (utf16.len() * 2) as u32;
    }
    for (_, _, utf16) in &strings {
        for &c in *utf16 {
            body.extend_from_slice(&be16(c));
        }
    }
    cross_roundtrip(
        lcms::signatures::tagtype::SigMultiLocalizedUnicodeType,
        &body,
    );

    // surrogate pair in utf16 → joined wchar
    let mut b2 = Vec::new();
    b2.extend_from_slice(&be32(1));
    b2.extend_from_slice(&be32(12));
    let utf = [0xD83Eu16, 0xDD1E, 0x41, 0]; // U+1F11E + 'A' + NUL
    b2.extend_from_slice(&be16(0x656e));
    b2.extend_from_slice(&be16(0x5553));
    b2.extend_from_slice(&be32((utf.len() * 2) as u32));
    b2.extend_from_slice(&be32(8 + 8 + 12));
    for c in utf {
        b2.extend_from_slice(&be16(c));
    }
    cross_roundtrip(lcms::signatures::tagtype::SigMultiLocalizedUnicodeType, &b2);
}

#[test]
fn named_color_and_colorant_table() {
    // namedColor2Type body: vendorFlag, count, nDeviceCoords, prefix[32], suffix[32], entries
    let mut body = Vec::new();
    body.extend_from_slice(&be32(0)); // vendorFlag
    body.extend_from_slice(&be32(2)); // count
    body.extend_from_slice(&be32(4)); // nDeviceCoords
    let mut pfx = [0u8; 32];
    pfx[..4].copy_from_slice(b"PANT");
    let mut sfx = [0u8; 32];
    sfx[..6].copy_from_slice(b"coated");
    body.extend_from_slice(&pfx);
    body.extend_from_slice(&sfx);
    for (name, pcs, col) in [
        (
            "Red 032 C",
            [0x1234u16, 0x5678, 0x9abc],
            [0x1111u16, 0x2222, 0x3333, 0x4444],
        ),
        (
            "Blue 072 C",
            [0xaaaa, 0xbbbb, 0xcccc],
            [0x5555, 0x6666, 0x7777, 0x8888],
        ),
    ] {
        let mut nm = [0u8; 32];
        nm[..name.len()].copy_from_slice(name.as_bytes());
        body.extend_from_slice(&nm);
        for p in pcs {
            body.extend_from_slice(&be16(p));
        }
        for cl in col {
            body.extend_from_slice(&be16(cl));
        }
    }
    cross_roundtrip(lcms::signatures::tagtype::SigNamedColor2Type, &body);

    // colorantTable: count + (name32 + pcs3) per entry
    let mut body = Vec::new();
    body.extend_from_slice(&be32(3));
    for (name, pcs) in [
        ("Cyan", [0xffffu16, 0, 0]),
        ("Magenta", [0, 0xffff, 0]),
        ("Yellow", [0, 0, 0xffff]),
    ] {
        let mut nm = [0u8; 32];
        nm[..name.len()].copy_from_slice(name.as_bytes());
        body.extend_from_slice(&nm);
        for p in pcs {
            body.extend_from_slice(&be16(p));
        }
    }
    cross_roundtrip(lcms::signatures::tagtype::SigColorantTableType, &body);
}

#[test]
fn profile_sequence_desc_and_id() {
    // pseq: count + per-entry (mfg,model,attr,tech, embedded text, embedded text)
    let mut body = Vec::new();
    body.extend_from_slice(&be32(1)); // count
    body.extend_from_slice(&be32(0x41414141)); // deviceMfg
    body.extend_from_slice(&be32(0x42424242)); // deviceModel
    body.extend_from_slice(&0x1122334455667788u64.to_be_bytes()); // attributes
    body.extend_from_slice(&be32(0x66546e6b)); // technology 'fTnlnk'? raw u32
    // embedded text ×2 — 'desc' type bases (bounded, like real pseq tags)
    for txt in [b"Manufacturer\0".as_slice(), b"Model\0".as_slice()] {
        body.extend_from_slice(&be32(0x64657363)); // 'desc'
        body.extend_from_slice(&be32(0)); // reserved
        body.extend_from_slice(&be32(txt.len() as u32)); // ascii count
        body.extend_from_slice(txt);
        // unicode block
        body.extend_from_slice(&be32(0)); // ucLangCode
        body.extend_from_slice(&be32(txt.len() as u32)); // ucCount
        for &c in txt {
            body.extend_from_slice(&be16(c as u16));
        }
        // scriptcode block: scCode u16 + scCount u8 + 67 bytes
        body.extend_from_slice(&be16(0));
        body.push(0);
        body.extend_from_slice(&[0u8; 67]);
    }
    cross_roundtrip(lcms::signatures::tagtype::SigProfileSequenceDescType, &body);

    // psid: count + position table + entries
    let mut body = Vec::new();
    body.extend_from_slice(&be32(1)); // count
    // element: 16B profileID + embedded text; offset absolute incl base
    let elem_off = 8 + 4 + 8; // base + count + dir entry
    let mut elem = Vec::new();
    elem.extend_from_slice(&[0xabu8; 16]);
    elem.extend_from_slice(&be32(0x64657363)); // 'desc'
    elem.extend_from_slice(&be32(0));
    let txt = b"Description\0".as_slice();
    elem.extend_from_slice(&be32(txt.len() as u32));
    elem.extend_from_slice(txt);
    elem.extend_from_slice(&be32(0));
    elem.extend_from_slice(&be32(txt.len() as u32));
    for &c in txt {
        elem.extend_from_slice(&be16(c as u16));
    }
    elem.extend_from_slice(&be16(0));
    elem.push(0);
    elem.extend_from_slice(&[0u8; 67]);
    body.extend_from_slice(&be32(elem_off));
    body.extend_from_slice(&be32(elem.len() as u32));
    body.extend_from_slice(&elem);
    cross_roundtrip(lcms::signatures::tagtype::SigProfileSequenceIdType, &body);
}

#[test]
fn ucrbg_crdinfo_screening_viewing() {
    // UcrBg: count+table16, count+table16, ascii tail
    let mut body = Vec::new();
    body.extend_from_slice(&be32(3));
    for v in [0u16, 0x8000, 0xffff] {
        body.extend_from_slice(&be16(v));
    }
    body.extend_from_slice(&be32(2));
    for v in [0u16, 0xffff] {
        body.extend_from_slice(&be16(v));
    }
    body.extend_from_slice(b"UCR/BG method\0");
    cross_roundtrip(lcms::signatures::tagtype::SigUcrBgType, &body);

    // CrdInfo: 5× (count + ascii)
    let mut body = Vec::new();
    for s in [
        b"ProductName\0".as_slice(),
        b"CRD0\0".as_slice(),
        b"CRD1\0".as_slice(),
        b"CRD2\0".as_slice(),
        b"CRD3\0".as_slice(),
    ] {
        body.extend_from_slice(&be32(s.len() as u32));
        body.extend_from_slice(s);
    }
    cross_roundtrip(lcms::signatures::tagtype::SigCrdInfoType, &body);

    // Screening: flag + nChannels + channels(freq,angle,spot)
    let mut body = Vec::new();
    body.extend_from_slice(&be32(1)); // flag
    body.extend_from_slice(&be32(2)); // nChannels
    for (f, a, s) in [
        (0x00018000u32, 0x00004000u32, 1u32),
        (0x00020000, 0x00008000, 2),
    ] {
        body.extend_from_slice(&be32(f));
        body.extend_from_slice(&be32(a));
        body.extend_from_slice(&be32(s));
    }
    cross_roundtrip(lcms::signatures::tagtype::SigScreeningType, &body);

    // ViewingConditions: 2 XYZ + u32
    let mut body = Vec::new();
    for v in [
        0x0000F6D6u32,
        0x00010000,
        0x0000D32D,
        0x0000F6D6,
        0x00010000,
        0x0000D32D,
    ] {
        body.extend_from_slice(&be32(v));
    }
    body.extend_from_slice(&be32(1)); // illuminant type
    cross_roundtrip(lcms::signatures::tagtype::SigViewingConditionsType, &body);
}
