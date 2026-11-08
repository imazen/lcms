//! cmstypes elem_count read check: corrupt a serialized tag's item
//! count -> the reader must reject the read the same way C does.

use lcms::profile;
use lcms_cref as c;
use std::ffi::c_void;

fn read_tag_bytes(sig: u32) -> (Vec<u8>, usize) {
    // save an XYZ profile with the wtpt tag (XYZ type = 6-element
    // record: type+reserved+3 f64)
    unsafe {
        let h = c::cmsCreateXYZProfile();
        let mut out = vec![0u8; 1 << 20];
        let mut size = out.len() as u32;
        c::cmsSaveProfileToMem(h, out.as_mut_ptr() as *mut _, &mut size);
        out.truncate(size as usize);
        c::cmsCloseProfile(h);
        // find the tag's data offset: the tag table is at 128+n*12;
        // wtpt is the 3rd entry typically — scan the tag table for
        // the sig and read its offset
        let n_tags = u32::from_be_bytes([out[128],out[129],out[130],out[131]]) as usize;
        for i in 0..n_tags {
            let e = 132 + i * 12;
            let tsig = u32::from_be_bytes([out[e],out[e+1],out[e+2],out[e+3]]);
            if tsig == sig {
                let off = u32::from_be_bytes(
                    [out[e+4],out[e+5],out[e+6],out[e+7]]) as usize;
                return (out, off);
            }
        }
        (out, 0)
    }
}

#[test]
fn corrupt_elem_count() {
    // corrupt an XYZ tag's reserved field so the reader's record-size
    // check fails — a malformed tag must be rejected by both sides
    let (mut bytes, off) = read_tag_bytes(0x77747074); // wtpt
    assert!(off > 0, "wtpt not found");
    // bytes[off..off+4] = type sig 'XYZ '; [off+4..8] = reserved;
    // [off+8..] = 3 s15Fixed16. Overwrite the type sig with garbage
    // so the reader's type dispatch fails
    bytes[off] = 0x58; bytes[off+1] = 0x59;
    bytes[off+2] = 0x5A; bytes[off+3] = 0x21; // 'XYZ!' — wrong type
    unsafe {
        let h = c::cmsOpenProfileFromMem(bytes.as_ptr() as _, bytes.len() as u32);
        let c_tag = if h.is_null() { std::ptr::null_mut() }
            else { c::cmsReadTag(h, 0x77747074) };
        let rp = profile::open_profile_from_mem(None, &bytes);
        let r_tag = rp.map(|mut p| {
            profile::cms_read_tag(&mut p, 0x77747074).is_some()
        });
        assert_eq!(c_tag.is_null(), r_tag == Some(false) || r_tag == None,
            "corrupt type sig read");
        if !h.is_null() { c::cmsCloseProfile(h); }
    }
}

/// Type_* edge payloads — empty/zero/single-element contents
/// exercising the handlers' boundary branches.
#[test]
fn edge_payloads() {
    unsafe {
        let h = c::cmsCreateXYZProfile();
        // empty curve (0 points → identity line per ICC 10.3)
        let c_curve0 = c::cmsBuildTabulatedToneCurve16(
            std::ptr::null_mut(), 0, std::ptr::null_mut());
        if !c_curve0.is_null() {
            let ok = c::cmsWriteTag(h, 0x72545243,
                c_curve0 as *mut c_void); // 'rTRC'
            eprintln!("curve0 write={ok}");
            c::cmsFreeToneCurve(c_curve0);
        }
        // single-entry numeric array — U16Fixed16 on 'chad' needs 9
        let one = [1.0f64; 9];
        let c_arr = c::cmsStageAllocIdentity; // placeholder
        let _ = c_arr;
        // write 9-element U16Fixed16 to chad — the minimum-valid
        let s15 = c::cmsWriteTag(h, 0x63686164,
            one.as_ptr() as *mut c_void); // 'chad'
        eprintln!("chad write={s15}");
        // empty text tag
        let c_ok2 = c::cmsWriteTag(h, 0x63707274,
            b"\0".as_ptr() as *const c_void); // 'cprt'
        eprintln!("empty cprt={c_ok2}");
        // zero-entry Mlu
        let mlu = c::cmsMLUalloc(std::ptr::null_mut(), 0);
        let c_ok3 = c::cmsWriteTag(h, 0x64657363, mlu as *mut c_void);
        eprintln!("empty desc={c_ok3}");
        c::cmsMLUfree(mlu);
        c::cmsCloseProfile(h);
    }
}

/// v2 vs v4 stored-type variants — the same logical tag serializes
/// as 'desc'/'text'/'curv' in v2 vs 'mluc'/'para' in v4,
/// exercising both Type_* paths.
#[test]
fn v2_v4_type_variants() {
    unsafe {
        for ver in [2.2f64, 4.3] {
            let h = c::cmsCreate_sRGBProfile();
            c::cmsSetProfileVersion(h, ver);
            // 'cprt' takes a cmsMLU — build one
            let mlu = c::cmsMLUalloc(std::ptr::null_mut(), 1);
            c::cmsMLUsetASCII(mlu,
                b"en\0".as_ptr() as *const _,
                b"US\0".as_ptr() as *const _,
                b"v2v4test\0".as_ptr() as *const _);
            let ok = c::cmsWriteTag(h, 0x63707274,
                mlu as *mut c_void); // 'cprt'
            eprintln!("v{ver} cprt write={ok}");
            // resave → the stored type should differ
            let mut cn = 0u32;
            c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
            let mut cbuf = vec![0u8; cn as usize];
            c::cmsSaveProfileToMem(h, cbuf.as_mut_ptr() as *mut _,
                &mut cn);
            // find the cprt tag's stored type in the directory
            let h2 = c::cmsOpenProfileFromMem(cbuf.as_ptr() as _,
                cn);
            let n2 = c::cmsGetTagCount(h2);
            for i in 0..n2 {
                let s2 = c::cmsGetTagSignature(h2, i as u32);
                if s2 == 0x63707274 {
                    // read raw to see the type prefix
                    let mut tb = [0u8; 4];
                    c::cmsReadRawTag(h2, 0x63707274,
                        tb.as_mut_ptr() as *mut _, 4);
                    eprintln!("v{ver} cprt type={:#010x}",
                        u32::from_be_bytes(tb));
                }
            }
            c::cmsCloseProfile(h2);
            c::cmsCloseProfile(h);
        }
    }
}

/// More v2/v4 type variants — desc (mluc/desc), rTRC (curv/para),
/// plus the remaining exotic sigs.
#[test]
fn v2_v4_more_sigs() {
    unsafe {
        for ver in [2.2f64, 4.3] {
            let h = c::cmsCreate_sRGBProfile();
            c::cmsSetProfileVersion(h, ver);
            // desc — same MLU
            let mlu = c::cmsMLUalloc(std::ptr::null_mut(), 1);
            c::cmsMLUsetASCII(mlu,
                b"en\0".as_ptr() as *const _,
                b"US\0".as_ptr() as *const _,
                b"desc\0".as_ptr() as *const _);
            let okd = c::cmsWriteTag(h, 0x64657363,
                mlu as *mut c_void); // 'desc'
            // rTRC — a gamma curve (curv vs para selection)
            let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
            let okt = c::cmsWriteTag(h, 0x72545243,
                g as *mut c_void); // 'rTRC'
            // resave → check stored types
            let mut cn = 0u32;
            c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
            let mut cbuf = vec![0u8; cn as usize];
            c::cmsSaveProfileToMem(h, cbuf.as_mut_ptr() as *mut _,
                &mut cn);
            let h2 = c::cmsOpenProfileFromMem(cbuf.as_ptr() as _,
                cn);
            for sig in [0x64657363u32, 0x72545243] {
                if c::cmsIsTag(h2, sig) != 0 {
                    let mut tb = [0u8; 4];
                    c::cmsReadRawTag(h2, sig,
                        tb.as_mut_ptr() as *mut _, 4);
                    eprintln!("v{ver} {sig:#x} type={:#010x}",
                        u32::from_be_bytes(tb));
                }
            }
            eprintln!("v{ver} desc={okd} trc={okt}");
            c::cmsCloseProfile(h2);
            c::cmsCloseProfile(h);
        }
    }
}

/// Remaining exotic tag sigs — canonical payloads on chrm/meas/
/// tech/lumi/view/clro/ciis/targ.
#[test]
fn remaining_exotic_sigs() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        // chrm — cmsCIExyYTRIPLE
        #[repr(C)]
        struct CChrm { r: c::CiexyY, g: c::CiexyY, b: c::CiexyY }
        let chrm = CChrm {
            r: c::CiexyY { x: 0.64, y: 0.33, Y: 1.0 },
            g: c::CiexyY { x: 0.30, y: 0.60, Y: 1.0 },
            b: c::CiexyY { x: 0.15, y: 0.06, Y: 1.0 },
        };
        let ok1 = c::cmsWriteTag(h, 0x6368726D,
            &chrm as *const _ as *mut c_void); // 'chrm'
        // lumi — cmsCIEXYZ
        let lumi = c::CieXyz { X: 0.0, Y: 0.5, Z: 0.0 };
        let ok2 = c::cmsWriteTag(h, 0x6C756D69,
            &lumi as *const _ as *mut c_void); // 'lumi'
        // tech — signature (fota = Fogra39L)
        let sigv: u32 = 0x666F7461; // 'fota'
        let ok3 = c::cmsWriteTag(h, 0x74656368,
            &sigv as *const _ as *mut c_void); // 'tech'
        // ciis — signature
        let ok4 = c::cmsWriteTag(h, 0x63696973,
            &sigv as *const _ as *mut c_void); // 'ciis'
        // clro — u8 array of colorant indices
        let clro: [u8; 4] = [0, 1, 2, 3];
        let ok5 = c::cmsWriteTag(h, 0x636C726F,
            clro.as_ptr() as *mut c_void); // 'clro'
        // targ — text
        let targ = b"target ref\0";
        let ok6 = c::cmsWriteTag(h, 0x74617267,
            targ.as_ptr() as *const c_void); // 'targ'
        eprintln!("chrm={ok1} lumi={ok2} tech={ok3} ciis={ok4} \
            clro={ok5} targ={ok6}");
        // resave
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut cbuf = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, cbuf.as_mut_ptr() as *mut _,
            &mut cn);
        let h2 = c::cmsOpenProfileFromMem(cbuf.as_ptr() as _, cn);
        for sig in [0x6368726Du32, 0x6C756D69, 0x74656368,
                    0x63696973, 0x636C726F, 0x74617267] {
            if c::cmsIsTag(h2, sig) != 0 {
                let mut tb = [0u8; 4];
                c::cmsReadRawTag(h2, sig,
                    tb.as_mut_ptr() as *mut _, 4);
                eprintln!("{sig:#x} type={:#010x}",
                    u32::from_be_bytes(tb));
            }
        }
        c::cmsCloseProfile(h2);
        c::cmsCloseProfile(h);
    }
}

/// meas payload — Type_Measurement_Write on the 'meas' sig.
/// (view/calt/vcgt deferred — their write paths segfault on
/// this profile class.)
#[test]
fn meas_payload() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        #[repr(C)]
        struct CMeas {
            observer: u32,
            backing: c::CieXyz,
            geometry: u32,
            flare: f64,
            illuminant_type: u32,
        }
        let meas = CMeas {
            observer: 1,
            backing: c::CieXyz { X: 0.0, Y: 0.5, Z: 0.0 },
            geometry: 1,
            flare: 0.0,
            illuminant_type: 2, // D65
        };
        let ok1 = c::cmsWriteTag(h, 0x6D656173,
            &meas as *const _ as *mut c_void); // 'meas'
        eprintln!("meas write={ok1}");
        // resave → stored type check
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut cbuf = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, cbuf.as_mut_ptr() as *mut _,
            &mut cn);
        let h2 = c::cmsOpenProfileFromMem(cbuf.as_ptr() as _, cn);
        if c::cmsIsTag(h2, 0x6D656173) != 0 {
            let mut tb = [0u8; 4];
            c::cmsReadRawTag(h2, 0x6D656173,
                tb.as_mut_ptr() as *mut _, 4);
            eprintln!("meas type={:#010x}",
                u32::from_be_bytes(tb));
        }
        c::cmsCloseProfile(h2);
        c::cmsCloseProfile(h);
    }
}

/// More writeable sigs — scrn/gamt/pre/bTRC/clrt/rXYZ/kTRC/bkpt.
#[test]
fn more_writable_sigs() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        // scrn (screening desc) — cmsScreening struct
        #[repr(C)]
        #[derive(Clone, Copy)]
        struct CScreening {
            flag: u32,
            n_channels: u32,
            channels: [CScreeningChannel; 16],
        }
        #[repr(C)]
        #[derive(Clone, Copy)]
        struct CScreeningChannel {
            frequency: f64, screen_angle: f64, spot_shape: u32,
        }
        let mut scrn = CScreening {
            flag: 0, n_channels: 1,
            channels: [CScreeningChannel {
                frequency: 0.0, screen_angle: 0.0, spot_shape: 0
            }; 16],
        };
        scrn.channels[0] = CScreeningChannel {
            frequency: 60.0, screen_angle: 45.0, spot_shape: 1,
        };
        let ok1 = c::cmsWriteTag(h, 0x7363726E,
            &scrn as *const _ as *mut c_void); // 'scrn'
        // 'gamt' (gamut CLUT) needs an MPE/lut pipeline with a
        // serializable shape — the identity-stage pipeline
        // crashes the lut8 writer; defer
        let ok2 = 0i32;
        // rXYZ — CIEXYZ
        let xyz = c::CieXyz { X: 0.5, Y: 0.4, Z: 0.3 };
        let ok3 = c::cmsWriteTag(h, 0x7258595A,
            &xyz as *const _ as *mut c_void); // 'rXYZ'
        // kTRC — a curve
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let ok4 = c::cmsWriteTag(h, 0x6B545243,
            g as *mut c_void); // 'kTRC'
        // bkpt — CIEXYZ
        let ok5 = c::cmsWriteTag(h, 0x626B7074,
            &xyz as *const _ as *mut c_void); // 'bkpt'
        // clrt — colorant table (ncl2-style, but clrt is the out
        // table)
        eprintln!("scrn={ok1} gamt={ok2} rXYZ={ok3} kTRC={ok4} \
            bkpt={ok5}");
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut cbuf = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, cbuf.as_mut_ptr() as *mut _,
            &mut cn);
        let h2 = c::cmsOpenProfileFromMem(cbuf.as_ptr() as _, cn);
        for sig in [0x7363726Eu32, 0x7258595A,
                    0x6B545243, 0x626B7074] {
            if c::cmsIsTag(h2, sig) != 0 {
                let mut tb = [0u8; 4];
                c::cmsReadRawTag(h2, sig,
                    tb.as_mut_ptr() as *mut _, 4);
                eprintln!("{sig:#x} type={:#010x}",
                    u32::from_be_bytes(tb));
            }
        }
        c::cmsCloseProfile(h2);
        c::cmsCloseProfile(h);
    }
}

/// Crafted binary tag types — write raw bytes with each type
/// sig + read back, exercising Type_*_Read on the unusual
/// stored-type variants.
#[test]
fn crafted_type_reads() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let mut cases: Vec<(u32, Vec<u8>)> = Vec::new();
        // 'desc' on 'cprt'
        {
            let mut b = Vec::new();
            b.extend_from_slice(b"desc");
            b.extend_from_slice(&[0; 4]);
            b.extend_from_slice(&5u32.to_be_bytes());
            b.extend_from_slice(b"text\0");
            b.extend_from_slice(&0u32.to_be_bytes());
            b.extend_from_slice(&0u32.to_be_bytes());
            b.extend_from_slice(&[0; 67]);
            cases.push((0x63707274, b));
        }
        // 'text' on 'cprt'
        {
            let mut b = Vec::new();
            b.extend_from_slice(b"text");
            b.extend_from_slice(&[0; 4]);
            b.extend_from_slice(b"hello\0");
            cases.push((0x63707274, b));
        }
        // 'XYZ ' on 'wtpt'
        {
            let mut b = Vec::new();
            b.extend_from_slice(b"XYZ ");
            b.extend_from_slice(&[0; 4]);
            for v in [0x0000F6D6u32, 0x00010000, 0x0000D32D] {
                b.extend_from_slice(&v.to_be_bytes());
            }
            cases.push((0x77747074, b));
        }
        // 'curv' on 'rTRC'
        {
            let mut b = Vec::new();
            b.extend_from_slice(b"curv");
            b.extend_from_slice(&[0; 4]);
            b.extend_from_slice(&4u32.to_be_bytes());
            for v in [0u16, 0x4000, 0x8000, 0xFFFF] {
                b.extend_from_slice(&v.to_be_bytes());
            }
            cases.push((0x72545243, b));
        }
        // 'para' on 'rTRC'
        {
            let mut b = Vec::new();
            b.extend_from_slice(b"para");
            b.extend_from_slice(&[0; 4]);
            b.extend_from_slice(&1u16.to_be_bytes());
            b.extend_from_slice(&0u16.to_be_bytes());
            for v in [0x00023333u32, 0x00010000, 0x00000000] {
                b.extend_from_slice(&v.to_be_bytes());
            }
            cases.push((0x72545243, b));
        }
        // 'mluc' on 'desc'
        {
            let mut b = Vec::new();
            b.extend_from_slice(b"mluc");
            b.extend_from_slice(&[0; 4]);
            b.extend_from_slice(&1u32.to_be_bytes());
            b.extend_from_slice(&12u32.to_be_bytes());
            b.extend_from_slice(b"en");
            b.extend_from_slice(b"US");
            b.extend_from_slice(&8u32.to_be_bytes());
            b.extend_from_slice(&28u32.to_be_bytes());
            b.extend_from_slice(&[0, b't', 0, b'e', 0, b's',
                                  0, b't']);
            cases.push((0x64657363, b));
        }
        // 'sf32' on 'chad'
        {
            let mut b = Vec::new();
            b.extend_from_slice(b"sf32");
            b.extend_from_slice(&[0; 4]);
            for _ in 0..9 {
                b.extend_from_slice(&0x00010000u32.to_be_bytes());
            }
            cases.push((0x63686164, b));
        }
        // 'sig ' on 'tech'
        {
            let mut b = Vec::new();
            b.extend_from_slice(b"sig ");
            b.extend_from_slice(&[0; 4]);
            b.extend_from_slice(b"fota");
            cases.push((0x74656368, b));
        }
        // 'dtim' on 'calt'
        {
            let mut b = Vec::new();
            b.extend_from_slice(b"dtim");
            b.extend_from_slice(&[0; 4]);
            for v in [2026u16, 11, 9, 12, 0, 0] {
                b.extend_from_slice(&v.to_be_bytes());
            }
            cases.push((0x63616C74, b));
        }
        // 'meas' on 'meas'
        {
            let mut b = Vec::new();
            b.extend_from_slice(b"meas");
            b.extend_from_slice(&[0; 4]);
            b.extend_from_slice(&1u32.to_be_bytes());
            for v in [0x00008000u32, 0x00008000, 0x00008000] {
                b.extend_from_slice(&v.to_be_bytes());
            }
            b.extend_from_slice(&1u32.to_be_bytes());
            b.extend_from_slice(&0u32.to_be_bytes());
            b.extend_from_slice(&2u32.to_be_bytes());
            cases.push((0x6D656173, b));
        }
        // built profiles have no IOHANDLER for the read path —
        // write raw, save to mem, reopen, then Type_*_Read runs
        // 'clro' on 'clro' — UInt8 array
        {
            let mut b = Vec::new();
            b.extend_from_slice(b"clro");
            b.extend_from_slice(&[0; 4]);
            b.extend_from_slice(&4u32.to_be_bytes()); // n
            for v in [0u8, 1, 2, 3] { b.push(v); }
            cases.push((0x636C726F, b));
        }
        // 'ncl2' on 'ncl2' — named color
        {
            let mut b = Vec::new();
            b.extend_from_slice(b"ncl2");
            b.extend_from_slice(&[0; 4]);
            b.extend_from_slice(&0u16.to_be_bytes()); // vendor
            b.extend_from_slice(&1u32.to_be_bytes()); // ncolors
            b.extend_from_slice(&3u32.to_be_bytes()); // ncoord
            // prefix(32)+suffix(32)+root(32) chars
            for _ in 0..96 { b.extend_from_slice(&0u16.to_be_bytes()); }
            // 1 color record: root(32) + pcs(3) + device(3)
            for _ in 0..32 { b.extend_from_slice(&0u16.to_be_bytes()); }
            for _ in 0..6 { b.extend_from_slice(&0x8000u16.to_be_bytes()); }
            cases.push((0x6E636C32, b));
        }
        // 'rcs2' on 'rcs2' — response curve set (16ch variant)
        {
            let mut b = Vec::new();
            b.extend_from_slice(b"rcs2");
            b.extend_from_slice(&[0; 4]);
            b.extend_from_slice(&1u16.to_be_bytes()); // channels
            b.extend_from_slice(&0u16.to_be_bytes()); // reserved
            b.extend_from_slice(&0u16.to_be_bytes()); // nmeas
            b.extend_from_slice(&0u16.to_be_bytes());
            b.extend_from_slice(&0xFFFFu32.to_be_bytes()); // freq
            b.extend_from_slice(&0u32.to_be_bytes());
            b.extend_from_slice(&0u32.to_be_bytes());
            b.extend_from_slice(&0u32.to_be_bytes());
            cases.push((0x72637332, b));
        }
        // 'view' on 'view' — viewing conditions
        {
            let mut b = Vec::new();
            b.extend_from_slice(b"view");
            b.extend_from_slice(&[0; 4]);
            for v in [0x0000F6D6u32, 0x00010000, 0x0000D32D, // illum XYZ
                      0x00008000u32, 0x00008000, 0x00008000] { // surround
                b.extend_from_slice(&v.to_be_bytes());
            }
            b.extend_from_slice(&2u32.to_be_bytes()); // illum type
            cases.push((0x76696577, b));
        }
        // 'chrm' on 'chrm' — chromaticity
        {
            let mut b = Vec::new();
            b.extend_from_slice(b"chrm");
            b.extend_from_slice(&[0; 4]);
            b.extend_from_slice(&3u16.to_be_bytes()); // nch
            b.extend_from_slice(&0u16.to_be_bytes()); // reserved
            for v in [0x0000A3D7u32, 0x00005477, // r x,y
                      0x00004DA3u32, 0x0000999A, // g
                      0x00002666u32, 0x00000F5C] { // b
                b.extend_from_slice(&v.to_be_bytes());
            }
            cases.push((0x6368726D, b));
        }
        // 'data' on 'ps2i' — ps2 rendering intent
        {
            let mut b = Vec::new();
            b.extend_from_slice(b"data");
            b.extend_from_slice(&[0; 4]);
            b.extend_from_slice(&4u32.to_be_bytes()); // size
            b.extend_from_slice(&0u32.to_be_bytes()); // flag
            b.extend_from_slice(&1u32.to_be_bytes()); // data
            cases.push((0x70733269, b));
        }
        // 'uf16' on 'chad' — U16Fixed16 array
        {
            let mut b = Vec::new();
            b.extend_from_slice(b"uf16");
            b.extend_from_slice(&[0; 4]);
            for _ in 0..9 {
                b.extend_from_slice(&0x00010000u32.to_be_bytes());
            }
            cases.push((0x63686164, b));
        }
        // 'uf32' on 'chad' — U32Fixed32 array
        {
            let mut b = Vec::new();
            b.extend_from_slice(b"uf32");
            b.extend_from_slice(&[0; 4]);
            for _ in 0..9 {
                b.extend_from_slice(&0x0001000000000000u64
                    .to_be_bytes());
            }
            cases.push((0x63686164, b));
        }
        // 'u8f8' on 'chad' — U8Fixed8 array
        {
            let mut b = Vec::new();
            b.extend_from_slice(b"u8f8");
            b.extend_from_slice(&[0; 4]);
            for _ in 0..9 {
                b.extend_from_slice(&0x0100u16.to_be_bytes());
            }
            cases.push((0x63686164, b));
        }
        // 'u16f' on 'chad' — U16Fixed16 array (u16 each)
        {
            let mut b = Vec::new();
            b.extend_from_slice(b"u16f");
            b.extend_from_slice(&[0; 4]);
            for _ in 0..9 {
                b.extend_from_slice(&0xFFFFu16.to_be_bytes());
            }
            cases.push((0x63686164, b));
        }
        // 'u32f' on 'chad' — U32 array
        {
            let mut b = Vec::new();
            b.extend_from_slice(b"u32f");
            b.extend_from_slice(&[0; 4]);
            for _ in 0..9 {
                b.extend_from_slice(&42u32.to_be_bytes());
            }
            cases.push((0x63686164, b));
        }
        // 'nui8' on 'clro' — UInt8 array
        {
            let mut b = Vec::new();
            b.extend_from_slice(b"nui8");
            b.extend_from_slice(&[0; 4]);
            b.extend_from_slice(&4u32.to_be_bytes());
            for v in [0u8, 1, 2, 3] { b.push(v); }
            cases.push((0x636C726F, b));
        }
        // 'nui16'/'nui32'/'ns16'/'ns32' on 'clrt'
        for (ts, elems, val) in [
            ("nui16", 4u32, 42u32),
            ("nui32", 4, 42),
            ("ns16", 4, 42),
            ("ns32", 4, 42),
        ] {
            let mut b = Vec::new();
            b.extend_from_slice(ts.as_bytes());
            b.extend_from_slice(&[0; 4]);
            b.extend_from_slice(&elems.to_be_bytes());
            for _ in 0..elems {
                if ts.ends_with("16") {
                    b.extend_from_slice(&(val as u16).to_be_bytes());
                } else {
                    b.extend_from_slice(&val.to_be_bytes());
                }
            }
            cases.push((0x636C7274, b));
        }
        // 'bfd ' on 'bfd ' — UcrBg
        {
            let mut b = Vec::new();
            b.extend_from_slice(b"bfd ");
            b.extend_from_slice(&[0; 4]);
            // curve (0 points → identity)
            b.extend_from_slice(b"curv");
            b.extend_from_slice(&[0; 4]);
            b.extend_from_slice(&0u32.to_be_bytes());
            // curve (0 points)
            b.extend_from_slice(b"curv");
            b.extend_from_slice(&[0; 4]);
            b.extend_from_slice(&0u32.to_be_bytes());
            // mluc (0 records)
            b.extend_from_slice(b"mluc");
            b.extend_from_slice(&[0; 4]);
            b.extend_from_slice(&0u32.to_be_bytes());
            b.extend_from_slice(&12u32.to_be_bytes());
            cases.push((0x62666420, b));
        }
        // 'lut8' on 'B2A0' — Lut8Type read
        {
            let mut b = Vec::new();
            b.extend_from_slice(b"lut8");
            b.extend_from_slice(&[0; 4]);
            b.push(3); b.push(3); // ich, och
            b.push(2); // clut gridpoints
            b.push(0); // pad
            // matrix 9 elems s15fixed16
            for _ in 0..9 {
                b.extend_from_slice(&0x00010000u32.to_be_bytes());
            }
            // input tables 3x256 + clut 2^3*3 + output 3x256
            for _ in 0..768 { b.push(0x80u8); }
            for _ in 0..24 { b.push(0x80u8); }
            for _ in 0..768 { b.push(0x80u8); }
            cases.push((0x42324130, b));
        }
        // 'lut16' on 'B2A0' — Lut16Type read
        {
            let mut b = Vec::new();
            b.extend_from_slice(b"lut16");
            b.extend_from_slice(&[0; 4]);
            b.push(3); b.push(3); // ich, och
            b.push(2); // clut gridpoints
            b.push(0); // pad
            for _ in 0..9 {
                b.extend_from_slice(&0x00010000u32.to_be_bytes());
            }
            // input/output table sizes + entries
            b.extend_from_slice(&256u16.to_be_bytes()); // n_in
            b.extend_from_slice(&256u16.to_be_bytes()); // n_out
            for _ in 0..768 {
                b.extend_from_slice(&0x8000u16.to_be_bytes());
            }
            for _ in 0..24 {
                b.extend_from_slice(&0x8000u16.to_be_bytes());
            }
            for _ in 0..768 {
                b.extend_from_slice(&0x8000u16.to_be_bytes());
            }
            cases.push((0x42324130, b));
        }
        // 'mAB ' on 'A2B0' — LutAtoBType read
        {
            let mut b = Vec::new();
            b.extend_from_slice(b"mAB ");
            b.extend_from_slice(&[0; 4]);
            b.push(3); b.push(3); // ich, och
            b.extend_from_slice(&[0; 2]);
            // B-curve, matrix, M-curve, CLUT, A-curve offsets
            let mut off = 32u32; // header = 32
            for _ in 0..5 {
                b.extend_from_slice(&off.to_be_bytes());
                off += 12; // each "curve" stub = 12 bytes
            }
            // 5 curve stubs: 'curv' + reserved + n=0 (identity)
            for _ in 0..5 {
                b.extend_from_slice(b"curv");
                b.extend_from_slice(&[0; 4]);
                b.extend_from_slice(&0u32.to_be_bytes());
            }
            cases.push((0x41324230, b));
        }
        // 'dict' on 'meta' — dictionary type
        {
            let mut b = Vec::new();
            b.extend_from_slice(b"dict");
            b.extend_from_slice(&[0; 4]);
            b.extend_from_slice(&1u32.to_be_bytes()); // nrec
            b.extend_from_slice(&16u32.to_be_bytes()); // recsz
            // 1 record: name off/len, value off/len
            b.extend_from_slice(&32u32.to_be_bytes());
            b.extend_from_slice(&4u32.to_be_bytes());
            b.extend_from_slice(&36u32.to_be_bytes());
            b.extend_from_slice(&4u32.to_be_bytes());
            b.extend_from_slice(&[0, b'k', 0, b'e']);
            b.extend_from_slice(&[0, b'v', 0, b'a']);
            cases.push((0x6D657461, b));
        }
        // 'nui64' on 'clrt' — UInt64 array
        {
            let mut b = Vec::new();
            b.extend_from_slice(b"nui64");
            b.extend_from_slice(&[0; 4]);
            b.extend_from_slice(&2u32.to_be_bytes());
            for _ in 0..2 {
                b.extend_from_slice(&42u64.to_be_bytes());
            }
            cases.push((0x636C7274, b));
        }
        // 'vued' on 'vued' — viewing-cond description (mluc+XYZ)
        {
            let mut b = Vec::new();
            b.extend_from_slice(b"vued");
            b.extend_from_slice(&[0; 4]);
            // mluc (0 records)
            b.extend_from_slice(b"mluc");
            b.extend_from_slice(&[0; 4]);
            b.extend_from_slice(&0u32.to_be_bytes());
            b.extend_from_slice(&12u32.to_be_bytes());
            cases.push((0x76756564, b));
        }
        for (i, (sig, bytes)) in cases.iter().enumerate() {
            let h2 = c::cmsCreate_sRGBProfile();
            let ok = c::cmsWriteRawTag(h2, *sig,
                bytes.as_ptr() as *const c_void,
                bytes.len() as u32);
            let mut cn = 0u32;
            c::cmsSaveProfileToMem(h2, std::ptr::null_mut(),
                &mut cn);
            let mut cbuf = vec![0u8; cn as usize];
            c::cmsSaveProfileToMem(h2,
                cbuf.as_mut_ptr() as *mut _, &mut cn);
            let h3 = c::cmsOpenProfileFromMem(
                cbuf.as_ptr() as _, cn);
            let r = if !h3.is_null() {
                c::cmsReadTag(h3, *sig)
            } else {
                std::ptr::null_mut()
            };
            eprintln!("case {i} sig={sig:#x} write={ok} read={}",
                r.is_null());
            if !h3.is_null() { c::cmsCloseProfile(h3); }
            c::cmsCloseProfile(h2);
        }
        c::cmsCloseProfile(h);
    }
}

/// cmsDupProfile on profiles with exotic written tags — the
/// Type_*_Dup + _Free paths on every stored object.
#[test]
fn dup_exotic_tags() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        // write a spread of tag types
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        c::cmsWriteTag(h, 0x72545243, g as *mut _); // rTRC curv
        let mlu = c::cmsMLUalloc(std::ptr::null_mut(), 1);
        c::cmsMLUsetASCII(mlu,
            b"en\0".as_ptr() as *const u8,
            b"US\0".as_ptr() as *const u8,
            b"desc\0".as_ptr() as *const u8);
        c::cmsWriteTag(h, 0x64657363, mlu as *mut _); // desc mluc
        let xyz = c::CieXyz { X: 0.5, Y: 0.4, Z: 0.3 };
        c::cmsWriteTag(h, 0x7258595A,
            &xyz as *const _ as *mut _); // rXYZ
        // cmsDupProfile doesn't exist — Type_*_Free runs on
        // cmsCloseProfile over the exotic stored objects
        c::cmsCloseProfile(h);
    }
}

/// 'cicp' — the ICC.1:2022-05 CICP tag (cmsVideoSignalType:
/// 4 u8 fields, primaries/transfer/matrix/fullrange).
#[test]
fn cicp_tag() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        #[repr(C)]
        struct CCicp {
            colour_primaries: u8,
            transfer_characteristics: u8,
            matrix_coefficients: u8,
            video_full_range_flag: u8,
        }
        let cicp = CCicp {
            colour_primaries: 1,      // BT.709
            transfer_characteristics: 13, // sRGB
            matrix_coefficients: 0,
            video_full_range_flag: 0,
        };
        let ok = c::cmsWriteTag(h, 0x63696370,
            &cicp as *const _ as *mut std::ffi::c_void);
        eprintln!("cicp write={ok}");
        // resave + read back
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut cbuf = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, cbuf.as_mut_ptr() as *mut _,
            &mut cn);
        let h2 = c::cmsOpenProfileFromMem(cbuf.as_ptr() as _, cn);
        let mut tb = [0u8; 4];
        if c::cmsIsTag(h2, 0x63696370) != 0 {
            c::cmsReadRawTag(h2, 0x63696370,
                tb.as_mut_ptr() as *mut _, 4);
            eprintln!("cicp type={:#010x}", u32::from_be_bytes(tb));
        }
        let r = c::cmsReadTag(h2, 0x63696370);
        eprintln!("cicp read={}", r.is_null());
        c::cmsCloseProfile(h2);
        c::cmsCloseProfile(h);
    }
}

/// Remaining simple-payload sigs — gXYZ/bXYZ (XYZ), rig0/rigs
/// (sig), dscm/scrd (mluc/desc), clot (colorant order).
#[test]
fn remaining_simple_sigs() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let xyz = c::CieXyz { X: 0.5, Y: 0.4, Z: 0.3 };
        let sigv: u32 = 0x666F7461; // 'fota'
        let mut cases = Vec::new();
        // gXYZ/bXYZ — XYZ payloads
        cases.push(("gXYZ", 0x6758595Au32,
            c::cmsWriteTag(h, 0x6758595A,
                &xyz as *const _ as *mut std::ffi::c_void)));
        cases.push(("bXYZ", 0x6258595A,
            c::cmsWriteTag(h, 0x6258595A,
                &xyz as *const _ as *mut std::ffi::c_void)));
        // rig0/rigs — rendering-intent gamut (sig type)
        cases.push(("rig0", 0x72696730,
            c::cmsWriteTag(h, 0x72696730,
                &sigv as *const _ as *mut std::ffi::c_void)));
        cases.push(("rigs", 0x72696773,
            c::cmsWriteTag(h, 0x72696773,
                &sigv as *const _ as *mut std::ffi::c_void)));
        // dscm — profile desc ML (mluc)
        let mlu = c::cmsMLUalloc(std::ptr::null_mut(), 1);
        c::cmsMLUsetASCII(mlu,
            b"en\0".as_ptr() as *const u8,
            b"US\0".as_ptr() as *const u8,
            b"dscm\0".as_ptr() as *const u8);
        cases.push(("dscm", 0x6473636D,
            c::cmsWriteTag(h, 0x6473636D,
                mlu as *mut std::ffi::c_void)));
        // scrd — screening desc (mluc or desc)
        cases.push(("scrd", 0x73637264,
            c::cmsWriteTag(h, 0x73637264,
                mlu as *mut std::ffi::c_void)));
        // clot — colorant order out (u8 array)
        let clot: [u8; 4] = [3, 2, 1, 0];
        cases.push(("clot", 0x636C6F74,
            c::cmsWriteTag(h, 0x636C6F74,
                clot.as_ptr() as *mut std::ffi::c_void)));
        for (name, sig, ok) in &cases {
            eprintln!("{name} {sig:#x} write={ok}");
        }
        c::cmsCloseProfile(h);
    }
}

/// More crafted sig/type combos — the decideType + Type_*_Read
/// dispatch on unusual stored types per sig.
#[test]
fn more_crafted_type_reads() {
    unsafe {
        let mut cases: Vec<(u32, Vec<u8>)> = Vec::new();
        // 'XYZ ' on the colorant sigs
        for sig in [0x7258595Au32, 0x6758595A, 0x6258595A] {
            let mut b = Vec::new();
            b.extend_from_slice(b"XYZ ");
            b.extend_from_slice(&[0; 4]);
            for v in [0x0000F6D6u32, 0x00010000, 0x0000D32D] {
                b.extend_from_slice(&v.to_be_bytes());
            }
            cases.push((sig, b));
        }
        // 'curv'/'para' on kTRC/gTRC/bTRC
        for sig in [0x6B545243u32, 0x67545243, 0x62545243] {
            let mut b = Vec::new();
            b.extend_from_slice(b"curv");
            b.extend_from_slice(&[0; 4]);
            b.extend_from_slice(&4u32.to_be_bytes());
            for v in [0u16, 0x4000, 0x8000, 0xFFFF] {
                b.extend_from_slice(&v.to_be_bytes());
            }
            cases.push((sig, b));
        }
        // 'mluc' on cprt/dmnd/dmdd/dscm
        for sig in [0x63707274u32, 0x646D6E64, 0x646D6464,
                    0x6473636D] {
            let mut b = Vec::new();
            b.extend_from_slice(b"mluc");
            b.extend_from_slice(&[0; 4]);
            b.extend_from_slice(&1u32.to_be_bytes());
            b.extend_from_slice(&12u32.to_be_bytes());
            b.extend_from_slice(b"en");
            b.extend_from_slice(b"US");
            b.extend_from_slice(&8u32.to_be_bytes());
            b.extend_from_slice(&28u32.to_be_bytes());
            b.extend_from_slice(&[0, b't', 0, b'e', 0, b's',
                                  0, b't']);
            cases.push((sig, b));
        }
        // 'text'/'desc' variants on dmnd/dmdd/dscm (v2 types)
        for sig in [0x646D6E64u32, 0x646D6464] {
            let mut b = Vec::new();
            b.extend_from_slice(b"text");
            b.extend_from_slice(&[0; 4]);
            b.extend_from_slice(b"desc\0");
            cases.push((sig, b));
        }
        // 'para' on kTRC/gTRC/bTRC
        for sig in [0x6B545243u32, 0x67545243, 0x62545243] {
            let mut b = Vec::new();
            b.extend_from_slice(b"para");
            b.extend_from_slice(&[0; 4]);
            b.extend_from_slice(&1u16.to_be_bytes());
            b.extend_from_slice(&0u16.to_be_bytes());
            for v in [0x00023333u32, 0x00010000, 0x00000000] {
                b.extend_from_slice(&v.to_be_bytes());
            }
            cases.push((sig, b));
        }
        // 'sig ' on rig0/rigs/tech/ciis
        for sig in [0x72696730u32, 0x72696773, 0x74656368,
                    0x63696973] {
            let mut b = Vec::new();
            b.extend_from_slice(b"sig ");
            b.extend_from_slice(&[0; 4]);
            b.extend_from_slice(b"fota");
            cases.push((sig, b));
        }
        // numeric-array types on 'chad' (an sf32 sig)
        for ts in ["uf8 ", "uf16", "uf32", "uf64", "u8f8",
                   "u8fx", "u16f", "u16x", "u32f", "u32x",
                   "s15f", "s15x", "sf32", "sf64", "s64f",
                   "nui8", "nui6", "nui3", "nui4", "nsi8",
                   "nsi6", "nsi3", "nsi4", "nsf ", "ns16",
                   "ns32", "ns64", "ut8 ", "ut16", "ut32",
                   "ut64", "utf8", "utf1", "utf3", "utf4",
                   "utfl", "utin", "ut8m", "utfm", "utfi",
                   "utfx", "utif", "ut8f", "utf8", "mpe ",
                   "mAB ", "mBA ", "mft1", "mft2", "mpet",
                   "dict", "mluc", "scrd", "scrn", "vued",
                   "vuet", "calt", "clid", "ciid", "ceid",
                   "clot", "clro", "clrt", "gamt", "pre0",
                   "pre1", "pre2", "bfd ", "ndin", "resp",
                   "targ", "tech", "vued", "vcgt", "data",
                   "dtim", "meas", "sig ", "chrm", "XYZ ",
                   "curv", "para", "text", "desc", "mluc",
                   "lut8", "lut1", "lutA", "lutB", "mAB "] {
            let mut b = Vec::new();
            b.extend_from_slice(ts.as_bytes());
            b.extend_from_slice(&[0; 4]);
            // generic body — 64 zero bytes covers most parsers'
            // minimum; they reject on bad counts
            b.extend_from_slice(&[0u8; 64]);
            cases.push((0x6D657461u32, b)); // 'meta' accepts any
        }
        for (i, (sig, bytes)) in cases.iter().enumerate() {
            let h2 = c::cmsCreate_sRGBProfile();
            c::cmsWriteRawTag(h2, *sig,
                bytes.as_ptr() as *const std::ffi::c_void,
                bytes.len() as u32);
            let mut cn = 0u32;
            c::cmsSaveProfileToMem(h2, std::ptr::null_mut(),
                &mut cn);
            let mut cbuf = vec![0u8; cn as usize];
            c::cmsSaveProfileToMem(h2,
                cbuf.as_mut_ptr() as *mut _, &mut cn);
            let h3 = c::cmsOpenProfileFromMem(
                cbuf.as_ptr() as _, cn);
            let r = if !h3.is_null() {
                c::cmsReadTag(h3, *sig)
            } else { std::ptr::null_mut() };
            eprintln!("case {i} sig={sig:#x} read={}",
                r.is_null());
            if !h3.is_null() { c::cmsCloseProfile(h3); }
            c::cmsCloseProfile(h2);
        }
    }
}

/// 'meta' read-path check — proper dict body on 'meta'.
#[test]
fn meta_read_check() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let mut b = Vec::new();
        b.extend_from_slice(b"dict");
        b.extend_from_slice(&[0; 4]);
        b.extend_from_slice(&1u32.to_be_bytes()); // nrec
        b.extend_from_slice(&16u32.to_be_bytes()); // recsz
        b.extend_from_slice(&32u32.to_be_bytes()); // name off
        b.extend_from_slice(&4u32.to_be_bytes()); // name len
        b.extend_from_slice(&36u32.to_be_bytes()); // val off
        b.extend_from_slice(&4u32.to_be_bytes()); // val len
        b.extend_from_slice(&[0, b'k', 0, b'e']);
        b.extend_from_slice(&[0, b'v', 0, b'a']);
        let ok = c::cmsWriteRawTag(h, 0x6D657461,
            b.as_ptr() as *const std::ffi::c_void,
            b.len() as u32);
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut cbuf = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, cbuf.as_mut_ptr() as *mut _,
            &mut cn);
        let h3 = c::cmsOpenProfileFromMem(cbuf.as_ptr() as _, cn);
        let r = if !h3.is_null() {
            c::cmsReadTag(h3, 0x6D657461)
        } else { std::ptr::null_mut() };
        eprintln!("meta write={ok} read={}", r.is_null());
        if !h3.is_null() { c::cmsCloseProfile(h3); }
        c::cmsCloseProfile(h);
    }
}

/// Multi-type dispatch — sigs that accept several types
/// (desc→desc/mluc, cprt→text/mluc, TRC→curv/para).
#[test]
fn multi_type_dispatch() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        // 'desc' sig takes desc or mluc — write each form raw
        for (name, ty) in [("desc-as-desc", "desc"),
                           ("desc-as-mluc", "mluc"),
                           ("cprt-as-text", "text"),
                           ("cprt-as-mluc", "mluc"),
                           ("trc-as-curv", "curv"),
                           ("trc-as-para", "para")] {
            let mut b = Vec::new();
            b.extend_from_slice(ty.as_bytes());
            b.extend_from_slice(&[0; 4]);
            match ty {
                "desc" => {
                    b.extend_from_slice(&5u32.to_be_bytes());
                    b.extend_from_slice(b"text\0");
                    b.extend_from_slice(&0u32.to_be_bytes());
                    b.extend_from_slice(&0u32.to_be_bytes());
                    b.extend_from_slice(&[0; 67]);
                }
                "mluc" => {
                    b.extend_from_slice(&1u32.to_be_bytes());
                    b.extend_from_slice(&12u32.to_be_bytes());
                    b.extend_from_slice(b"enUS");
                    b.extend_from_slice(&8u32.to_be_bytes());
                    b.extend_from_slice(&28u32.to_be_bytes());
                    b.extend_from_slice(&[0, b't', 0, b'e',
                                          0, b's', 0, b't']);
                }
                "text" => b.extend_from_slice(b"text\0"),
                "curv" => {
                    b.extend_from_slice(&2u32.to_be_bytes());
                    for v in [0u16, 0xFFFF] {
                        b.extend_from_slice(&v.to_be_bytes());
                    }
                }
                "para" => {
                    b.extend_from_slice(&1u16.to_be_bytes());
                    b.extend_from_slice(&0u16.to_be_bytes());
                    for v in [0x00023333u32, 0x00010000, 0] {
                        b.extend_from_slice(&v.to_be_bytes());
                    }
                }
                _ => {}
            }
            let sig = match name {
                n if n.starts_with("desc") => 0x64657363u32,
                n if n.starts_with("cprt") => 0x63707274u32,
                _ => 0x72545243u32,
            };
            let ok = c::cmsWriteRawTag(h, sig,
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
            let r = if !h3.is_null() {
                c::cmsReadTag(h3, sig)
            } else { std::ptr::null_mut() };
            eprintln!("{name} write={ok} read={}", r.is_null());
            if !h3.is_null() { c::cmsCloseProfile(h3); }
        }
        c::cmsCloseProfile(h);
    }
}

/// Cross-profile tag copy — read a tag from one profile, write
/// it to another (exercises Type_*_Dup on the object).
#[test]
fn cross_profile_copy() {
    unsafe {
        let ha = c::cmsCreate_sRGBProfile();
        let hb = c::cmsCreateLab4Profile(std::ptr::null_mut());
        // copy several tag types across
        for sig in [0x72545243u32, // rTRC (curve)
                    0x64657363,    // desc (mluc)
                    0x63707274,    // cprt (text/mluc)
                    0x77747074,    // wtpt (XYZ)
                    0x63686164,    // chad (sf32 matrix)
                    0x6C756D69,    // lumi (XYZ)
                    0x7258595A] {  // rXYZ
            let obj = c::cmsReadTag(ha, sig);
            if obj.is_null() { continue; }
            let ok = c::cmsWriteTag(hb, sig, obj);
            eprintln!("{sig:#x} copy={ok}");
        }
        c::cmsCloseProfile(ha);
        c::cmsCloseProfile(hb);
    }
}

/// Overwrite-same-sig machinery — write a tag twice with
/// different-type payloads (Free + re-alloc paths).
#[test]
fn overwrite_same_sig() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        // 'desc' as mluc then overwrite as text (v2 form)
        let mlu = c::cmsMLUalloc(std::ptr::null_mut(), 1);
        c::cmsMLUsetASCII(mlu,
            b"en\0".as_ptr() as *const u8,
            b"US\0".as_ptr() as *const u8,
            b"v4\0".as_ptr() as *const u8);
        let ok1 = c::cmsWriteTag(h, 0x64657363,
            mlu as *mut std::ffi::c_void);
        let txt = b"v2text\0";
        let ok2 = c::cmsWriteTag(h, 0x64657363,
            txt.as_ptr() as *const std::ffi::c_void);
        // 'cprt' as mluc then text
        let ok3 = c::cmsWriteTag(h, 0x63707274,
            mlu as *mut std::ffi::c_void);
        let ok4 = c::cmsWriteTag(h, 0x63707274,
            txt.as_ptr() as *const std::ffi::c_void);
        // 'rTRC' as curve then overwrite as a different curve
        let g1 = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let g2 = c::cmsBuildGamma(std::ptr::null_mut(), 1.8);
        let ok5 = c::cmsWriteTag(h, 0x72545243,
            g1 as *mut std::ffi::c_void);
        let ok6 = c::cmsWriteTag(h, 0x72545243,
            g2 as *mut std::ffi::c_void);
        eprintln!("overwrite desc={ok1}/{ok2} cprt={ok3}/{ok4} \
            trc={ok5}/{ok6}");
        // resave — the replaced types serialize
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut cbuf = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, cbuf.as_mut_ptr() as *mut _,
            &mut cn);
        let h3 = c::cmsOpenProfileFromMem(cbuf.as_ptr() as _, cn);
        for sig in [0x64657363u32, 0x63707274, 0x72545243] {
            if c::cmsIsTag(h3, sig) != 0 {
                let mut tb = [0u8; 4];
                c::cmsReadRawTag(h3, sig,
                    tb.as_mut_ptr() as *mut _, 4);
                eprintln!("{sig:#x} final-type={:#010x}",
                    u32::from_be_bytes(tb));
            }
        }
        c::cmsCloseProfile(h3);
        c::cmsCloseProfile(h);
    }
}

/// Multi-locale MLU serialization — the Type_MLU internals on
/// several locales.
#[test]
fn multi_locale_mlu() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let mlu = c::cmsMLUalloc(std::ptr::null_mut(), 1);
        // several locales
        for (l, c, t) in [
            (b"en\0".as_ptr(), b"US\0".as_ptr(),
             b"English US\0".as_ptr()),
            (b"en\0".as_ptr(), b"GB\0".as_ptr(),
             b"English UK\0".as_ptr()),
            (b"fr\0".as_ptr(), b"FR\0".as_ptr(),
             b"Francais\0".as_ptr()),
            (b"de\0".as_ptr(), b"DE\0".as_ptr(),
             b"Deutsch\0".as_ptr()),
            (b"ja\0".as_ptr(), b"JP\0".as_ptr(),
             b"Nihongo\0".as_ptr()),
        ] {
            let ok = c::cmsMLUsetASCII(mlu, l, c, t);
            eprintln!("mlu set={ok}");
        }
        // read them back
        let mut buf = vec![0u8; 64];
        for (l, c) in [
            (b"en\0".as_ptr(), b"US\0".as_ptr()),
            (b"fr\0".as_ptr(), b"FR\0".as_ptr()),
            (b"ja\0".as_ptr(), b"JP\0".as_ptr()),
            (b"xx\0".as_ptr(), b"XX\0".as_ptr()),
        ] {
            let n = c::cmsMLUgetASCII(mlu, l, c,
                buf.as_mut_ptr(), 64);
            eprintln!("mlu get n={n}");
        }
        // write + resave — the mluc writer on a 5-locale MLU
        let ok = c::cmsWriteTag(h, 0x646D6E64,
            mlu as *mut std::ffi::c_void);
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut cbuf = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, cbuf.as_mut_ptr() as *mut _,
            &mut cn);
        let h2 = c::cmsOpenProfileFromMem(cbuf.as_ptr() as _, cn);
        let r = c::cmsReadTag(h2, 0x646D6E64);
        eprintln!("write={ok} read={}", r.is_null());
        c::cmsCloseProfile(h2);
        c::cmsCloseProfile(h);
        c::cmsMLUfree(mlu);
    }
}

/// Type_Dictionary internals — the dict read/write/dup
/// machinery via a 'meta'-type dict tag.
#[test]
fn dictionary_type() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let dict = c::cmsDictAlloc(std::ptr::null_mut());
        // add entries — Wchar* names/values
        for i in 0..4u32 {
            let name = [b'k' as i32 + i as i32, 0];
            let val = [b'v' as i32 + i as i32, 0];
            let ok = c::cmsDictAddEntry(dict,
                name.as_ptr(), val.as_ptr(),
                std::ptr::null_mut(), std::ptr::null_mut());
            eprintln!("dict add={ok}");
        }
        // dict machinery
        let mut e = c::cmsDictGetEntryList(dict);
        let mut cnt = 0u32;
        while !e.is_null() {
            cnt += 1;
            e = c::cmsDictNextEntry(e);
        }
        eprintln!("dict entries={cnt}");
        // write + resave + read — Type_Dictionary Write/Read
        let ok = c::cmsWriteTag(h, 0x6D657461,
            dict as *mut std::ffi::c_void); // 'meta' takes dict
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut cbuf = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, cbuf.as_mut_ptr() as *mut _,
            &mut cn);
        let h2 = c::cmsOpenProfileFromMem(cbuf.as_ptr() as _, cn);
        let r = c::cmsReadTag(h2, 0x6D657461);
        eprintln!("dict write={ok} read={}", r.is_null());
        c::cmsCloseProfile(h2);
        c::cmsCloseProfile(h);
        c::cmsDictFree(dict);
    }
}

/// Type_UcrBg internals — 'bfd ' takes a cmsUcrBg payload
/// (Ucr + Bg curves + KIntensity + desc MLU).
#[test]
fn ucrbg_type() {
    unsafe {
        #[repr(C)]
        struct UcrBg {
            ucr: *mut std::ffi::c_void,
            bg: *mut std::ffi::c_void,
            kint: *mut std::ffi::c_void,
            desc: *mut std::ffi::c_void,
        }
        let g1 = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let g2 = c::cmsBuildGamma(std::ptr::null_mut(), 1.8);
        let g3 = c::cmsBuildGamma(std::ptr::null_mut(), 1.0);
        let mlu = c::cmsMLUalloc(std::ptr::null_mut(), 1);
        c::cmsMLUsetASCII(mlu,
            b"en\0".as_ptr() as *const u8,
            b"US\0".as_ptr() as *const u8,
            b"ucrbg\0".as_ptr() as *const u8);
        let ub = UcrBg {
            ucr: g1 as *mut std::ffi::c_void,
            bg: g2 as *mut std::ffi::c_void,
            kint: g3 as *mut std::ffi::c_void,
            desc: mlu as *mut std::ffi::c_void,
        };
        let h = c::cmsCreate_sRGBProfile();
        let ok = c::cmsWriteTag(h, 0x62666420,
            &ub as *const _ as *mut std::ffi::c_void); // 'bfd '
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut cbuf = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, cbuf.as_mut_ptr() as *mut _,
            &mut cn);
        let h2 = c::cmsOpenProfileFromMem(cbuf.as_ptr() as _, cn);
        let r = c::cmsReadTag(h2, 0x62666420);
        eprintln!("bfd write={ok} read={}", r.is_null());
        c::cmsCloseProfile(h2);
        c::cmsCloseProfile(h);
        c::cmsMLUfree(mlu);
        c::cmsFreeToneCurve(g1);
        c::cmsFreeToneCurve(g2);
        c::cmsFreeToneCurve(g3);
    }
}

/// Type_vcgt — the 3-curve video-gamma payload (tabulated
/// curves, not formula-type-5).
#[test]
fn vcgt_type() {
    unsafe {
        // vcgt takes cmsToneCurve** — the write checks each
        // curve's type; tabulated curves produce type-5 data
        let vals = [0u16, 0x8000, 0xFFFF];
        let g1 = c::cmsBuildTabulatedToneCurve16(
            std::ptr::null_mut(), 3, vals.as_ptr());
        let g2 = c::cmsBuildTabulatedToneCurve16(
            std::ptr::null_mut(), 3, vals.as_ptr());
        let g3 = c::cmsBuildTabulatedToneCurve16(
            std::ptr::null_mut(), 3, vals.as_ptr());
        let curves: [*mut std::ffi::c_void; 3] = [
            g1 as *mut std::ffi::c_void,
            g2 as *mut std::ffi::c_void,
            g3 as *mut std::ffi::c_void];
        let h = c::cmsCreate_sRGBProfile();
        let ok = c::cmsWriteTag(h, 0x76636774,
            curves.as_ptr() as *mut std::ffi::c_void); // 'vcgt'
        eprintln!("vcgt write={ok}");
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut cbuf = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, cbuf.as_mut_ptr() as *mut _,
            &mut cn);
        let h2 = c::cmsOpenProfileFromMem(cbuf.as_ptr() as _, cn);
        let r = c::cmsReadTag(h2, 0x76636774);
        eprintln!("vcgt read={}", r.is_null());
        c::cmsCloseProfile(h2);
        c::cmsCloseProfile(h);
        c::cmsFreeToneCurve(g1);
        c::cmsFreeToneCurve(g2);
        c::cmsFreeToneCurve(g3);
    }
}

/// Type_Screening internals — 'scrn' takes a cmsScreening
/// payload (flag + channels).
#[test]
fn screening_type() {
    unsafe {
        #[repr(C)]
        #[derive(Copy, Clone)]
        struct ScrCh {
            frequency: f64,
            screen_angle: f64,
            spot_shape: u32,
        }
        #[repr(C)]
        struct Screening {
            flag: u32,
            n_channels: u32,
            channels: [ScrCh; 16],
        }
        let mut sc = Screening {
            flag: 0, n_channels: 3,
            channels: [ScrCh { frequency: 0.0,
                screen_angle: 0.0, spot_shape: 0 }; 16],
        };
        for i in 0..3 {
            sc.channels[i].frequency = 150.0;
            sc.channels[i].screen_angle = 45.0 * (i as f64);
            sc.channels[i].spot_shape = 1;
        }
        let h = c::cmsCreate_sRGBProfile();
        let ok = c::cmsWriteTag(h, 0x7363726E,
            &sc as *const _ as *mut std::ffi::c_void); // 'scrn'
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut cbuf = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, cbuf.as_mut_ptr() as *mut _,
            &mut cn);
        let h2 = c::cmsOpenProfileFromMem(cbuf.as_ptr() as _, cn);
        let r = c::cmsReadTag(h2, 0x7363726E);
        eprintln!("scrn write={ok} read={}", r.is_null());
        c::cmsCloseProfile(h2);
        c::cmsCloseProfile(h);
    }
}

/// Type_ProfileSequenceDesc — 'pseq'/'psid' take a cmsSEQ
/// payload (alloc + populate + write).
#[test]
fn pseq_type() {
    unsafe {
        #[repr(C)]
        struct PSeqDesc {
            device_mfg: u32,
            device_model: u32,
            attributes: u64,
            technology: u32,
            profile_id: [u8; 16],
            manufacturer: *mut std::ffi::c_void,
            model: *mut std::ffi::c_void,
            description: *mut std::ffi::c_void,
        }
        #[repr(C)]
        struct Seq {
            n: u32,
            context_id: *mut std::ffi::c_void,
            seq: *mut PSeqDesc,
        }
        let seq = c::cmsAllocProfileSequenceDescription(
            std::ptr::null_mut(), 2);
        assert!(!seq.is_null());
        // populate the descriptors via the struct
        let s = seq as *mut Seq;
        let mlu = c::cmsMLUalloc(std::ptr::null_mut(), 1);
        c::cmsMLUsetASCII(mlu,
            b"en\0".as_ptr() as *const u8,
            b"US\0".as_ptr() as *const u8,
            b"desc\0".as_ptr() as *const u8);
        for i in 0..2 {
            (*s).seq.add(i).write(PSeqDesc {
                device_mfg: 0x4D4647,
                device_model: 0x4D4F44,
                attributes: 0,
                technology: 0x6D667220,
                profile_id: [i as u8; 16],
                manufacturer: mlu,
                model: mlu,
                description: mlu,
            });
        }
        let h = c::cmsCreate_sRGBProfile();
        let ok1 = c::cmsWriteTag(h, 0x70736571,
            seq as *mut std::ffi::c_void); // 'pseq'
        let ok2 = c::cmsWriteTag(h, 0x70736964,
            seq as *mut std::ffi::c_void); // 'psid'
        eprintln!("pseq={ok1} psid={ok2}");
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut cbuf = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, cbuf.as_mut_ptr() as *mut _,
            &mut cn);
        let h2 = c::cmsOpenProfileFromMem(cbuf.as_ptr() as _, cn);
        for sig in [0x70736571u32, 0x70736964] {
            if c::cmsIsTag(h2, sig) != 0 {
                let r = c::cmsReadTag(h2, sig);
                eprintln!("{sig:#x} read={}", r.is_null());
            }
        }
        c::cmsCloseProfile(h2);
        c::cmsCloseProfile(h);
    }
}

/// Type_MPE internals — a crafted 'mpe '-typed tag through the
/// element-dispatch machinery.
#[test]
fn mpe_type() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        // 'mpe ' body — the MPE tag format: sig + channels +
        // element count + elements. A curve-set element.
        let mut b = Vec::new();
        b.extend_from_slice(b"mpe ");
        b.extend_from_slice(&[0; 4]);
        b.extend_from_slice(&1u16.to_be_bytes()); // inch
        b.extend_from_slice(&1u16.to_be_bytes()); // outch
        b.extend_from_slice(&1u32.to_be_bytes()); // nelem
        // element: 'cvst' (curve set)
        b.extend_from_slice(b"cvst");
        b.extend_from_slice(&[0; 4]);
        b.extend_from_slice(&1u32.to_be_bytes()); // ncurves
        // one curve: 'curf' float table
        b.extend_from_slice(b"curf");
        b.extend_from_slice(&[0; 4]);
        b.extend_from_slice(&2u32.to_be_bytes()); // npoints
        b.extend_from_slice(&0.0f32.to_be_bytes());
        b.extend_from_slice(&1.0f32.to_be_bytes());
        let ok = c::cmsWriteRawTag(h, 0x41324230,
            b.as_ptr() as *const std::ffi::c_void,
            b.len() as u32);
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut cbuf = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, cbuf.as_mut_ptr() as *mut _,
            &mut cn);
        let h3 = c::cmsOpenProfileFromMem(cbuf.as_ptr() as _, cn);
        let r = c::cmsReadTag(h3, 0x41324230);
        eprintln!("mpe write={ok} read={}", r.is_null());
        if !h3.is_null() { c::cmsCloseProfile(h3); }
        c::cmsCloseProfile(h);
    }
}

/// Type_ResponseCurveSet16 — 'rcs2'/'resp' takes a
/// cmsResponseCurveSet16 payload (offsets + values).
#[test]
fn response_curve_set() {
    unsafe {
        // cmsResponseCurveSet16 = {nChannels, nMeasurements,
        // offsets[], values[]}
        #[repr(C)]
        struct Rcs {
            n_channels: u32,
            n_measurements: u32,
            offsets: [u32; 16],
            values: [u16; 32],
        }
        let mut r = Rcs {
            n_channels: 3, n_measurements: 2,
            offsets: [0; 16],
            values: [0; 32],
        };
        // 3 channels x 2 measurements — offsets into values
        r.offsets[0] = 0;
        r.offsets[1] = 2;
        r.offsets[2] = 4;
        for i in 0..6 {
            r.values[i] = (i as u16) * 0x1000;
        }
        let h = c::cmsCreate_sRGBProfile();
        let ok1 = c::cmsWriteTag(h, 0x72637332,
            &r as *const _ as *mut std::ffi::c_void); // 'rcs2'
        let ok2 = c::cmsWriteTag(h, 0x72657370,
            &r as *const _ as *mut std::ffi::c_void); // 'resp'
        eprintln!("rcs2={ok1} resp={ok2}");
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut cbuf = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, cbuf.as_mut_ptr() as *mut _,
            &mut cn);
        let h2 = c::cmsOpenProfileFromMem(cbuf.as_ptr() as _, cn);
        for sig in [0x72637332u32, 0x72657370] {
            if c::cmsIsTag(h2, sig) != 0 {
                let rr = c::cmsReadTag(h2, sig);
                eprintln!("{sig:#x} read={}", rr.is_null());
            }
        }
        c::cmsCloseProfile(h2);
        c::cmsCloseProfile(h);
    }
}

/// More MPE element types — crafted 'mpe ' tags with matrix/
/// CLUT/bAcs elements through the dispatch.
#[test]
fn mpe_element_types() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        for (name, elem) in [
            ("matrix", b"matf" as &[u8]),
            ("clut", b"clut"),
            ("bacs", b"bACS"),
            ("eacs", b"eACS"),
        ] {
            let mut b = Vec::new();
            b.extend_from_slice(b"mpe ");
            b.extend_from_slice(&[0; 4]);
            b.extend_from_slice(&1u16.to_be_bytes()); // inch
            b.extend_from_slice(&1u16.to_be_bytes()); // outch
            b.extend_from_slice(&1u32.to_be_bytes()); // nelem
            b.extend_from_slice(elem);
            b.extend_from_slice(&[0; 4]);
            // element body — generic enough for each
            match elem {
                b"matf" => {
                    for _ in 0..12 {
                        b.extend_from_slice(
                            &0.5f32.to_be_bytes());
                    }
                }
                b"clut" => {
                    b.extend_from_slice(&[2u8; 16]); // grid
                    b.extend_from_slice(&4u8.to_be_bytes()); // prec
                    b.extend_from_slice(&[0; 3]);
                    for _ in 0..8 {
                        b.extend_from_slice(
                            &0.5f32.to_be_bytes());
                    }
                }
                _ => b.extend_from_slice(&[0; 4]),
            }
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

/// LUT-reader channel dispatch — crafted lut8/lut16 tags with
/// different in/out channel counts.
#[test]
fn lut_channel_dispatch() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        for (name, ty, inch, outch) in [
            ("lut8-3x3", b"lut8" as &[u8], 3u8, 3u8),
            ("lut8-1x1", b"lut8", 1, 1),
            ("lut8-4x4", b"lut8", 4, 4),
            ("lut8-2x3", b"lut8", 2, 3),
            ("lut16-3x3", b"lut16", 3, 3),
            ("lut16-1x1", b"lut16", 1, 1),
        ] {
            let mut b = Vec::new();
            b.extend_from_slice(ty);
            b.extend_from_slice(&[0; 4]);
            // lut8: inch, outch, clutpoints, e00-e20 (9 s15f16),
            // input-tables (256*inch u8), clut, output-tables
            b.push(inch);
            b.push(outch);
            b.push(2); // clut grid points
            b.push(0); // pad
            // e00-e20 — s15f16 identity
            for i in 0..9 {
                let v: i32 = if i % 4 == 0 { 0x10000 } else { 0 };
                b.extend_from_slice(&v.to_be_bytes());
            }
            let n_in = 256 * (inch as usize);
            let n_clut = (2usize).pow(inch as u32)
                * (outch as usize);
            let n_out = 256 * (outch as usize);
            if ty == b"lut8" {
                for i in 0..n_in {
                    b.push((i % 256) as u8);
                }
                for i in 0..n_clut {
                    b.push((i % 256) as u8);
                }
                for i in 0..n_out {
                    b.push((i % 256) as u8);
                }
            } else {
                // lut16: ntab-entries u16 at offset
                for _ in 0..n_in {
                    b.extend_from_slice(&0x5555u16.to_be_bytes());
                }
                for _ in 0..n_clut {
                    b.extend_from_slice(&0x5555u16.to_be_bytes());
                }
                for _ in 0..n_out {
                    b.extend_from_slice(&0x5555u16.to_be_bytes());
                }
            }
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

/// Type_XYZ array-length machinery — N-element XYZ bodies.
#[test]
fn xyz_array_types() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        for n in [1u32, 2, 4, 8] {
            let mut b = Vec::new();
            b.extend_from_slice(b"XYZ ");
            b.extend_from_slice(&[0; 4]);
            for i in 0..n {
                let x = (0x00010000i32) + i as i32 * 0x1000;
                let y = (0x00010000i32);
                let z = (0x00010000i32);
                b.extend_from_slice(&x.to_be_bytes());
                b.extend_from_slice(&y.to_be_bytes());
                b.extend_from_slice(&z.to_be_bytes());
            }
            let ok = c::cmsWriteRawTag(h, 0x7258595A,
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
            let r = c::cmsReadTag(h3, 0x7258595A);
            eprintln!("xyz n={n} write={ok} read={}",
                r.is_null());
            if !h3.is_null() { c::cmsCloseProfile(h3); }
        }
        c::cmsCloseProfile(h);
    }
}

/// Type_ParametricCurve internals — 'para' bodies on all
/// parametric types 0-8.
#[test]
fn parametric_curve_types() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        // parametric types: 0=g (1 param), 1=Cg+f (2),
        // 2=Cg+af+b (3), 3=Cg+ac+f+d (4), 4=Cg+abcdefg (7),
        // 5-8 more complex
        for ptype in [0u16, 1, 2, 3, 4, 5, 6, 7, 8] {
            let nparams = [1u16, 2, 4, 5, 7, 3, 4, 5, 5]
                [ptype as usize];
            let mut b = Vec::new();
            b.extend_from_slice(b"para");
            b.extend_from_slice(&[0; 4]);
            b.extend_from_slice(&ptype.to_be_bytes());
            b.extend_from_slice(&[0; 2]);
            for _ in 0..nparams {
                b.extend_from_slice(&0x00019999i32.to_be_bytes());
            }
            let ok = c::cmsWriteRawTag(h, 0x72545243,
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
            let r = c::cmsReadTag(h3, 0x72545243);
            eprintln!("para type={ptype} write={ok} read={}",
                r.is_null());
            if !h3.is_null() { c::cmsCloseProfile(h3); }
        }
        c::cmsCloseProfile(h);
    }
}

/// Type_Curve internals — 'curv' bodies on different point
/// counts (0=identity, 1=gamma, 2+ = table).
#[test]
fn curve_point_counts() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        for n in [0u32, 1, 2, 16, 256, 4096] {
            let mut b = Vec::new();
            b.extend_from_slice(b"curv");
            b.extend_from_slice(&[0; 4]);
            b.extend_from_slice(&n.to_be_bytes());
            for i in 0..n {
                let v = (0xFFFF * i) / n.max(1);
                b.extend_from_slice(&(v as u16).to_be_bytes());
            }
            let ok = c::cmsWriteRawTag(h, 0x72545243,
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
            let r = c::cmsReadTag(h3, 0x72545243);
            eprintln!("curv n={n} write={ok} read={}",
                r.is_null());
            if !h3.is_null() { c::cmsCloseProfile(h3); }
        }
        c::cmsCloseProfile(h);
    }
}

/// Type_Chromaticity internals — 'chrm' bodies on different
/// channel counts.
#[test]
fn chromaticity_types() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        for nch in [2u16, 3, 4, 6] {
            let mut b = Vec::new();
            b.extend_from_slice(b"chrm");
            b.extend_from_slice(&[0; 4]);
            b.extend_from_slice(&nch.to_be_bytes());
            b.extend_from_slice(&1u16.to_be_bytes()); // channels
            for i in 0..nch {
                // xy per channel — s15f16
                b.extend_from_slice(
                    &(0x5000u32 + (i as u32) * 0x1000)
                        .to_be_bytes());
                b.extend_from_slice(
                    &(0x5300u32).to_be_bytes());
            }
            let ok = c::cmsWriteRawTag(h, 0x6368726D,
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
            let r = c::cmsReadTag(h3, 0x6368726D);
            eprintln!("chrm nch={nch} write={ok} read={}",
                r.is_null());
            if !h3.is_null() { c::cmsCloseProfile(h3); }
        }
        c::cmsCloseProfile(h);
    }
}

/// Type_Data/Signature/DateTime on more sigs — variable-body
/// validation on the remaining sigs.
#[test]
fn data_sig_datetime_types() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        // 'data' body — flag + data bytes
        for (sig, name) in [(0x73637264u32, "scrd"),
                            (0x6473636Du32, "dscm"),
                            (0x76696C6Cu32, "vill"),
                            (0x72696730u32, "rig0")] {
            let mut b = Vec::new();
            b.extend_from_slice(b"data");
            b.extend_from_slice(&[0; 4]);
            b.extend_from_slice(&0u32.to_be_bytes()); // flag
            b.extend_from_slice(b"payload");
            let ok = c::cmsWriteRawTag(h, sig,
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
            let r = c::cmsReadTag(h3, sig);
            eprintln!("{name} write={ok} read={}", r.is_null());
            if !h3.is_null() { c::cmsCloseProfile(h3); }
        }
        // 'sig ' body on more sigs
        for sig in [0x74656368u32, 0x72696730, 0x63696973,
                    0x76756564, 0x76696577] {
            let mut b = Vec::new();
            b.extend_from_slice(b"sig ");
            b.extend_from_slice(&[0; 4]);
            b.extend_from_slice(&0x666F7461u32.to_be_bytes());
            let ok = c::cmsWriteRawTag(h, sig,
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
            let r = c::cmsReadTag(h3, sig);
            eprintln!("sig {sig:#x} write={ok} read={}",
                r.is_null());
            if !h3.is_null() { c::cmsCloseProfile(h3); }
        }
        c::cmsCloseProfile(h);
    }
}

/// Array-type internals — sf32/nui16/nui32/nui64/uf32/uf64 on
/// the array-accepting sigs.
#[test]
fn array_types() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        for (name, ty, elem, nelem, sig) in [
            ("sf32", b"sf32" as &[u8], 4u32, 9u32, 0x63686164u32),
            ("nui8", b"nui8", 1, 8, 0x6E64696E),
            ("nui16", b"nui6", 2, 8, 0x6E64696E),
            ("nui32", b"nui3", 4, 8, 0x6E64696E),
            ("nui64", b"nui4", 8, 8, 0x6E64696E),
            ("uf32", b"uf32", 4, 8, 0x6E64696E),
            ("uf64", b"uf64", 8, 8, 0x6E64696E),
            ("s15f", b"s15f", 4, 9, 0x63686164),
        ] {
            let mut b = Vec::new();
            b.extend_from_slice(ty);
            b.extend_from_slice(&[0; 4]);
            for i in 0..nelem {
                for _ in 0..elem {
                    b.push((i % 256) as u8);
                }
            }
            let ok = c::cmsWriteRawTag(h, sig,
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
            let r = c::cmsReadTag(h3, sig);
            eprintln!("{name} write={ok} read={}", r.is_null());
            if !h3.is_null() { c::cmsCloseProfile(h3); }
        }
        c::cmsCloseProfile(h);
    }
}

/// Type_TextDesc internals — 'desc' bodies on more sigs.
#[test]
fn text_desc_types() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        for (sig, name) in [(0x64657363u32, "desc"),
                            (0x646D6464u32, "dmdd"),
                            (0x646D6E64u32, "dmnd"),
                            (0x6473636Du32, "dscm"),
                            (0x76756564u32, "vued")] {
            let mut b = Vec::new();
            b.extend_from_slice(b"desc");
            b.extend_from_slice(&[0; 4]);
            // asciiCount + ascii + padding
            let text = b"textdesc\0";
            b.extend_from_slice(&(text.len() as u32)
                .to_be_bytes());
            b.extend_from_slice(text);
            // pad to 12-byte alignment then unicode+script
            while b.len() % 4 != 0 { b.push(0); }
            b.extend_from_slice(&[0; 67]); // unicode/scriptcode
            let ok = c::cmsWriteRawTag(h, sig,
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
            let r = c::cmsReadTag(h3, sig);
            eprintln!("{name} write={ok} read={}", r.is_null());
            if !h3.is_null() { c::cmsCloseProfile(h3); }
        }
        c::cmsCloseProfile(h);
    }
}

/// Type_MLU on the MLU-accepting sigs — 'mluc' bodies on
/// cprt/desc/dmdd/dmnd/dscm/vued/meas/tech/ciis.
#[test]
fn mlu_types() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        for (sig, name) in [(0x63707274u32, "cprt"),
                            (0x64657363u32, "desc"),
                            (0x646D6464u32, "dmdd"),
                            (0x646D6E64u32, "dmnd"),
                            (0x6473636Du32, "dscm"),
                            (0x76756564u32, "vued"),
                            (0x6D656173u32, "meas"),
                            (0x74656368u32, "tech"),
                            (0x63696973u32, "ciis"),
                            (0x636C6F74u32, "clot"),
                            (0x636C7274u32, "clrt")] {
            let mut b = Vec::new();
            b.extend_from_slice(b"mluc");
            b.extend_from_slice(&[0; 4]);
            b.extend_from_slice(&1u32.to_be_bytes()); // nrec
            b.extend_from_slice(&12u32.to_be_bytes()); // recsz
            b.extend_from_slice(b"enUS");
            b.extend_from_slice(&8u32.to_be_bytes()); // len
            b.extend_from_slice(&28u32.to_be_bytes()); // off
            b.extend_from_slice(&[0, b't', 0, b'e',
                                  0, b's', 0, b't']);
            let ok = c::cmsWriteRawTag(h, sig,
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
            let r = c::cmsReadTag(h3, sig);
            eprintln!("{name} write={ok} read={}", r.is_null());
            if !h3.is_null() { c::cmsCloseProfile(h3); }
        }
        c::cmsCloseProfile(h);
    }
}

/// Type_DateTime + Type_Measurement internals — 'dtim'/'meas'
/// bodies on the accepting sigs.
#[test]
fn datetime_measurement_types() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        // 'dtim' body — 6 u16 fields
        for sig in [0x63616C74u32, 0x63707274u32,
                    0x64657363u32] {
            let mut b = Vec::new();
            b.extend_from_slice(b"dtim");
            b.extend_from_slice(&[0; 4]);
            for v in [2024u16, 6, 15, 12, 30, 45] {
                b.extend_from_slice(&v.to_be_bytes());
            }
            let ok = c::cmsWriteRawTag(h, sig,
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
            let r = c::cmsReadTag(h3, sig);
            eprintln!("dtim {sig:#x} write={ok} read={}",
                r.is_null());
            if !h3.is_null() { c::cmsCloseProfile(h3); }
        }
        // 'meas' body — backing + geometry + measurand
        for sig in [0x6D656173u32] {
            let mut b = Vec::new();
            b.extend_from_slice(b"meas");
            b.extend_from_slice(&[0; 4]);
            b.extend_from_slice(&1u32.to_be_bytes()); // observer
            b.extend_from_slice(&0x00010000i32.to_be_bytes()); // X
            b.extend_from_slice(&0x00010000i32.to_be_bytes()); // Y
            b.extend_from_slice(&0x00010000i32.to_be_bytes()); // Z
            b.extend_from_slice(&1u32.to_be_bytes()); // geometry
            b.extend_from_slice(&1u32.to_be_bytes()); // flare
            b.extend_from_slice(&1u32.to_be_bytes()); // measurand
            let ok = c::cmsWriteRawTag(h, sig,
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
            let r = c::cmsReadTag(h3, sig);
            eprintln!("meas write={ok} read={}", r.is_null());
            if !h3.is_null() { c::cmsCloseProfile(h3); }
        }
        c::cmsCloseProfile(h);
    }
}

/// Final Type_*_Read sweep — every type body on 'meta' (the
/// full type-dispatch coverage).
#[test]
fn full_type_sweep() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        // every valid type signature — a generic 64-byte body
        // exercises each Type_*_Read's dispatch + reject
        let types: [&[u8; 4]; 65] = [
            b"XYZ ", b"curv", b"data", b"desc", b"dtim",
            b"meas", b"mluc", b"para", b"sig ", b"text",
            b"vcgt", b"chrm", b"clrt", b"scrn", b"ncl2",
            b"dict", b"cprt", b"dmnd", b"dmdd", b"dscm",
            b"vued", b"view", b"meas", b"ndin", b"resp",
            b"rcs2", b"targ", b"tech", b"scrd", b"bfd ",
            b"gamt", b"pre0", b"pre1", b"pre2", b"pre2",
            b"lut8", b"lut1", b"lutA", b"lutB", b"mAB ",
            b"mBA ", b"mft1", b"mft2", b"mpet", b"mpe ",
            b"ut8 ", b"ut16", b"ut32", b"ut64", b"utf8",
            b"uf8 ", b"u8f8", b"u16f", b"u32f", b"s15f",
            b"sf32", b"s64f", b"nui8", b"nui6", b"nui3",
            b"nui4", b"nsi8", b"nsi6", b"nsi3", b"nsi4",
        ];
        for ty in types {
            let mut b = Vec::new();
            b.extend_from_slice(ty);
            b.extend_from_slice(&[0; 4]);
            b.extend_from_slice(&[0u8; 64]);
            let ok = c::cmsWriteRawTag(h, 0x6D657461,
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
            let r = c::cmsReadTag(h3, 0x6D657461);
            eprintln!("{:?} write={ok} read={}",
                std::str::from_utf8(ty), r.is_null());
            if !h3.is_null() { c::cmsCloseProfile(h3); }
        }
        c::cmsCloseProfile(h);
    }
}

/// Type_ProfileSequenceDesc + Dup internals — read a devicelink's
/// pseq/psid, dup the SEQ, write to another profile.
#[test]
fn pseq_dup() {
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
            let pseq = c::cmsReadTag(dl, 0x70736571);
            let psid = c::cmsReadTag(dl, 0x70736964);
            if !pseq.is_null() {
                let dup = c::cmsDupProfileSequenceDescription(
                    pseq as *mut std::ffi::c_void);
                eprintln!("pseq dup={}", dup.is_null());
                if !dup.is_null() {
                    c::cmsFreeProfileSequenceDescription(
                        dup as *mut std::ffi::c_void);
                }
            }
            if !psid.is_null() {
                let dup2 = c::cmsDupProfileSequenceDescription(
                    psid as *mut std::ffi::c_void);
                eprintln!("psid dup={}", dup2.is_null());
                if !dup2.is_null() {
                    c::cmsFreeProfileSequenceDescription(
                        dup2 as *mut std::ffi::c_void);
                }
            }
            c::cmsCloseProfile(dl);
        }
        let _ = c::cmsCloseProfile(hi);
        let _ = c::cmsCloseProfile(ho);
    }
}

/// Type_Measurement internals — 'meas' takes a
/// cmsICCMeasurementConditions payload.
#[test]
fn measurement_type() {
    unsafe {
        #[repr(C)]
        struct Meas {
            observer: u32,
            backing: c::CieXyz,
            geometry: u32,
            flare: f64,
            illuminant: u32,
        }
        let m = Meas {
            observer: 1,
            backing: c::CieXyz { X: 0.5, Y: 0.5, Z: 0.5 },
            geometry: 1,
            flare: 0.5,
            illuminant: 1,
        };
        let h = c::cmsCreate_sRGBProfile();
        let ok = c::cmsWriteTag(h, 0x6D656173,
            &m as *const _ as *mut std::ffi::c_void); // 'meas'
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut cbuf = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, cbuf.as_mut_ptr() as *mut _,
            &mut cn);
        let h2 = c::cmsOpenProfileFromMem(cbuf.as_ptr() as _, cn);
        let r = c::cmsReadTag(h2, 0x6D656173);
        eprintln!("meas write={ok} read={}", r.is_null());
        c::cmsCloseProfile(h2);
        c::cmsCloseProfile(h);
    }
}

/// Type_ViewingConditions — 'view' takes a
/// cmsICCViewingConditions payload.
#[test]
fn viewing_conditions_type() {
    unsafe {
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
        let h = c::cmsCreate_sRGBProfile();
        // 'view' is an mntr-class tag — try on the sRGB profile
        let ok1 = c::cmsWriteTag(h, 0x76696577,
            &vc as *const _ as *mut std::ffi::c_void); // 'view'
        let ok2 = c::cmsWriteTag(h, 0x76756564,
            &vc as *const _ as *mut std::ffi::c_void); // 'vued'
        let ok3 = c::cmsWriteTag(h, 0x76756574,
            &vc as *const _ as *mut std::ffi::c_void); // 'vuet'
        eprintln!("view={ok1} vued={ok2} vuet={ok3}");
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut cbuf = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, cbuf.as_mut_ptr() as *mut _,
            &mut cn);
        let h2 = c::cmsOpenProfileFromMem(cbuf.as_ptr() as _, cn);
        for sig in [0x76696577u32, 0x76756564, 0x76756574] {
            if c::cmsIsTag(h2, sig) != 0 {
                let r = c::cmsReadTag(h2, sig);
                eprintln!("{sig:#x} read={}", r.is_null());
            }
        }
        c::cmsCloseProfile(h2);
        c::cmsCloseProfile(h);
    }
}

/// Comprehensive Type_*_Read sweep — every remaining type
/// signature on a generic sig.
#[test]
fn remaining_type_reads() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        // all lcms2.20 type signatures — a generic body
        // exercises each Type_*_Read's dispatch + reject
        let types: [&[u8; 4]; 40] = [
            b"XYZ ", b"curv", b"data", b"desc", b"dtim",
            b"meas", b"mluc", b"para", b"sig ", b"text",
            b"vcgt", b"chrm", b"clrt", b"scrn", b"ncl2",
            b"dict", b"cprt", b"dmnd", b"dmdd", b"dscm",
            b"vued", b"view", b"ndin", b"resp", b"rcs2",
            b"targ", b"tech", b"scrd", b"bfd ", b"gamt",
            b"pre0", b"pre1", b"pre2", b"lut8", b"lut1",
            b"lutA", b"lutB", b"mAB ", b"mBA ", b"mft1",
        ];
        for ty in types {
            let mut b = Vec::new();
            b.extend_from_slice(ty);
            b.extend_from_slice(&[0; 4]);
            // minimal valid body per type — the reader's
            // dispatch fires even when the body rejects
            b.extend_from_slice(&[0u8; 32]);
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
            eprintln!("{:?} write={ok} read={}",
                std::str::from_utf8(ty), r.is_null());
            if !h3.is_null() { c::cmsCloseProfile(h3); }
        }
        c::cmsCloseProfile(h);
    }
}


/// Type_Screening internals — 'scrn'/'scrd' with proper
/// cmsScreening payloads.
#[test]
fn screening_types() {
    unsafe {
        #[repr(C)]
        #[derive(Copy, Clone)]
        struct ScreeningChannel {
            freq: f64,
            angle: f64,
            spot: u32,
        }
        #[repr(C)]
        struct Screening {
            flag: u32,
            n_channels: u32,
            channels: [ScreeningChannel; 16],
        }
        let mut s = Screening {
            flag: 0,
            n_channels: 3,
            channels: [ScreeningChannel { freq: 0.0,
                angle: 0.0, spot: 0 }; 16],
        };
        for i in 0..3 {
            s.channels[i] = ScreeningChannel {
                freq: 45.0 + i as f64 * 10.0,
                angle: 15.0 + i as f64 * 30.0,
                spot: 1,
            };
        }
        let h = c::cmsCreate_sRGBProfile();
        for (sig, name) in [(0x7363726Eu32, "scrn"),
                            (0x73637264u32, "scrd")] {
            let ok = c::cmsWriteTag(h, sig,
                &s as *const _ as *mut std::ffi::c_void);
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

/// Type_UcrBg internals — 'bfd ' takes a cmsUcrBg payload
/// {Ucr, Bg, Desc}.
#[test]
fn ucrbg_type2() {
    unsafe {
        #[repr(C)]
        struct UcrBg {
            ucr: *mut std::ffi::c_void,  // cmsToneCurve*
            bg: *mut std::ffi::c_void,   // cmsToneCurve*
            desc: *mut std::ffi::c_void, // cmsMLU*
        }
        let ucr = c::cmsBuildGamma(std::ptr::null_mut(), 1.5);
        let bg = c::cmsBuildGamma(std::ptr::null_mut(), 0.8);
        let mlu = c::cmsMLUalloc(std::ptr::null_mut(), 1);
        c::cmsMLUsetASCII(mlu,
            b"en\0".as_ptr() as *const u8,
            b"US\0".as_ptr() as *const u8,
            b"ucrbg\0".as_ptr() as *const u8);
        let ub = UcrBg {
            ucr: ucr as *mut std::ffi::c_void,
            bg: bg as *mut std::ffi::c_void,
            desc: mlu as *mut std::ffi::c_void,
        };
        let h = c::cmsCreate_sRGBProfile();
        let ok = c::cmsWriteTag(h, 0x62666420,
            &ub as *const _ as *mut std::ffi::c_void); // 'bfd '
        eprintln!("bfd write={ok}");
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut cbuf = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, cbuf.as_mut_ptr() as *mut _,
            &mut cn);
        let h2 = c::cmsOpenProfileFromMem(cbuf.as_ptr() as _, cn);
        let r = c::cmsReadTag(h2, 0x62666420);
        eprintln!("bfd read={}", r.is_null());
        c::cmsCloseProfile(h2);
        c::cmsCloseProfile(h);
        c::cmsMLUfree(mlu);
        c::cmsFreeToneCurve(ucr);
        c::cmsFreeToneCurve(bg);
    }
}

/// Type_ProfileSequenceDesc internals — 'pseq'/'psid' with a
/// real cmsSEQ (via cmsAllocProfileSequenceDescription).
#[test]
fn pseq_type2() {
    unsafe {
        let seq = c::cmsAllocProfileSequenceDescription(
            std::ptr::null_mut(), 1);
        eprintln!("seq null={}", seq.is_null());
        let h = c::cmsCreate_sRGBProfile();
        if !seq.is_null() {
            for (sig, name) in [(0x70736571u32, "pseq"),
                                (0x70736964u32, "psid")] {
                let ok = c::cmsWriteTag(h, sig,
                    seq as *mut std::ffi::c_void);
                eprintln!("{name} write={ok}");
            }
            let mut cn = 0u32;
            c::cmsSaveProfileToMem(h, std::ptr::null_mut(),
                &mut cn);
            let mut cbuf = vec![0u8; cn as usize];
            c::cmsSaveProfileToMem(h,
                cbuf.as_mut_ptr() as *mut _, &mut cn);
            let h3 = c::cmsOpenProfileFromMem(
                cbuf.as_ptr() as _, cn);
            for sig in [0x70736571u32, 0x70736964] {
                if c::cmsIsTag(h3, sig) != 0 {
                    let r = c::cmsReadTag(h3, sig);
                    eprintln!("{sig:#x} read={}", r.is_null());
                }
            }
            c::cmsCloseProfile(h3);
            c::cmsFreeProfileSequenceDescription(
                seq as *mut std::ffi::c_void);
        }
        c::cmsCloseProfile(h);
    }
}

/// Type_Dictionary internals — 'meta' with a real cmsDict
/// payload (the dict machinery).
#[test]
fn dict_type() {
    unsafe {
        let dict = c::cmsDictAlloc(std::ptr::null_mut());
        eprintln!("dict null={}", dict.is_null());
        if !dict.is_null() {
            let name = [0x6Eu32, 0x61, 0x6D, 0x65, 0]; // "name" utf32
            let val = [0x76u32, 0x61, 0x6C, 0x75, 0x65, 0];
            let ok1 = c::cmsDictAddEntry(dict,
                name.as_ptr() as *const i32,
                val.as_ptr() as *const i32,
                std::ptr::null_mut(), std::ptr::null_mut());
            let h = c::cmsCreate_sRGBProfile();
            let ok2 = c::cmsWriteTag(h, 0x6D657461,
                dict as *mut std::ffi::c_void);
            eprintln!("dict add={ok1} write={ok2}");
            let mut cn = 0u32;
            c::cmsSaveProfileToMem(h, std::ptr::null_mut(),
                &mut cn);
            let mut cbuf = vec![0u8; cn as usize];
            c::cmsSaveProfileToMem(h,
                cbuf.as_mut_ptr() as *mut _, &mut cn);
            let h2 = c::cmsOpenProfileFromMem(
                cbuf.as_ptr() as _, cn);
            let r = c::cmsReadTag(h2, 0x6D657461);
            eprintln!("dict read={}", r.is_null());
            c::cmsCloseProfile(h2);
            c::cmsCloseProfile(h);
            c::cmsDictFree(dict);
        }
    }
}

/// Type_NamedColor2 internals — 'clot'/'clrt'/'ndin' with a
/// real ncl2 payload.
#[test]
fn ncl2_types() {
    unsafe {
        let ncl = c::cmsAllocNamedColorList(
            std::ptr::null_mut(), 2, 4,
            b"p\0".as_ptr() as *const _,
            b"s\0".as_ptr() as *const _);
        let pcs = [0x4000u16, 0x8000, 0x8000];
        let dev = [0x1000u16, 0x2000, 0x3000, 0x4000];
        c::cmsAppendNamedColor(ncl,
            b"n1\0".as_ptr() as *const u8,
            pcs.as_ptr(), dev.as_ptr());
        c::cmsAppendNamedColor(ncl,
            b"n2\0".as_ptr() as *const u8,
            pcs.as_ptr(), dev.as_ptr());
        let h = c::cmsCreate_sRGBProfile();
        for (sig, name) in [(0x636C6F74u32, "clot"),
                            (0x636C7274u32, "clrt"),
                            (0x6E64696Eu32, "ndin")] {
            let ok = c::cmsWriteTag(h, sig,
                ncl as *mut std::ffi::c_void);
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
        c::cmsFreeNamedColorList(ncl);
    }
}

/// Type_MLU internals — multi-locale MLU machinery on more
/// locale strings + fallback paths.
#[test]
fn mlu_machinery() {
    unsafe {
        let mlu = c::cmsMLUalloc(std::ptr::null_mut(), 3);
        // multiple locales — the MLU's locale-table machinery
        for (lang, country, text) in [
            (b"en\0" as &[u8], b"US\0", b"us-text\0" as &[u8]),
            (b"de\0", b"DE\0", b"de-text\0"),
            (b"fr\0", b"FR\0", b"fr-text\0"),
            (b"ja\0", b"JP\0", b"ja-text\0"),
            (b"zh\0", b"CN\0", b"zh-text\0"),
        ] {
            c::cmsMLUsetASCII(mlu,
                lang.as_ptr() as *const u8,
                country.as_ptr() as *const u8,
                text.as_ptr() as *const u8);
        }
        // read each locale back + a fallback (xx -> first)
        for (lang, country, _t) in [
            (b"en\0" as &[u8], b"US\0", 1),
            (b"de\0", b"DE\0", 2),
            (b"fr\0", b"FR\0", 3),
            (b"ja\0", b"JP\0", 4),
            (b"zh\0", b"CN\0", 5),
            (b"xx\0", b"XX\0", 6), // fallback
        ] {
            let mut buf = [0u8; 64];
            let n = c::cmsMLUgetASCII(mlu,
                lang.as_ptr() as *const u8,
                country.as_ptr() as *const u8,
                buf.as_mut_ptr(), 64);
            let _ = n;
        }
        // write to a profile + resave + read — the full MLU
        // serialization machinery
        let h = c::cmsCreate_sRGBProfile();
        c::cmsWriteTag(h, 0x64657363,
            mlu as *mut std::ffi::c_void);
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut cbuf = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, cbuf.as_mut_ptr() as *mut _,
            &mut cn);
        let h2 = c::cmsOpenProfileFromMem(cbuf.as_ptr() as _, cn);
        let r = c::cmsReadTag(h2, 0x64657363);
        eprintln!("mlu resave read={}", r.is_null());
        c::cmsCloseProfile(h2);
        c::cmsCloseProfile(h);
        c::cmsMLUfree(mlu);
    }
}

/// Type_Text internals — 'text' bodies on the text-accepting
/// sigs.
#[test]
fn text_types() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        for (sig, name) in [(0x63707274u32, "cprt"),
                            (0x74656368u32, "tech"),
                            (0x73637264u32, "scrd"),
                            (0x63696973u32, "ciis"),
                            (0x76756564u32, "vued"),
                            (0x76756574u32, "vuet")] {
            let mut b = Vec::new();
            b.extend_from_slice(b"text");
            b.extend_from_slice(&[0; 4]);
            b.extend_from_slice(b"sample-text\0");
            while b.len() % 4 != 0 { b.push(0); }
            let ok = c::cmsWriteRawTag(h, sig,
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
            let r = c::cmsReadTag(h3, sig);
            eprintln!("{name} write={ok} read={}", r.is_null());
            if !h3.is_null() { c::cmsCloseProfile(h3); }
        }
        c::cmsCloseProfile(h);
    }
}

/// Type_Signature internals — 'sig ' bodies on the
/// sig-accepting sigs.
#[test]
fn signature_types() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        for (sig, name) in [(0x74656368u32, "tech"),
                            (0x63696973u32, "ciis"),
                            (0x72696730u32, "rig0"),
                            (0x72696773u32, "rigs"),
                            (0x736D6D6Du32, "smmm"),
                            (0x636C6964u32, "clid"),
                            (0x63696964u32, "ciid"),
                            (0x63656964u32, "ceid")] {
            let mut b = Vec::new();
            b.extend_from_slice(b"sig ");
            b.extend_from_slice(&[0; 4]);
            b.extend_from_slice(&0x666F7461u32.to_be_bytes());
            let ok = c::cmsWriteRawTag(h, sig,
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
            let r = c::cmsReadTag(h3, sig);
            eprintln!("{name} write={ok} read={}", r.is_null());
            if !h3.is_null() { c::cmsCloseProfile(h3); }
        }
        c::cmsCloseProfile(h);
    }
}

/// Type_DateTime internals — 'calt' with a proper struct tm
/// payload (the write path takes tm*, not a raw body).
#[test]
fn calt_datetime() {
    unsafe {
        #[repr(C)]
        struct Tm {
            sec: i32, min: i32, hour: i32, mday: i32,
            mon: i32, year: i32, wday: i32, yday: i32,
            isdst: i32,
        }
        let tm = Tm {
            sec: 30, min: 45, hour: 12, mday: 15, mon: 5,
            year: 124, wday: 6, yday: 166, isdst: 0,
        };
        let h = c::cmsCreate_sRGBProfile();
        let ok = c::cmsWriteTag(h, 0x63616C74,
            &tm as *const _ as *mut std::ffi::c_void); // 'calt'
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut cbuf = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, cbuf.as_mut_ptr() as *mut _,
            &mut cn);
        let h2 = c::cmsOpenProfileFromMem(cbuf.as_ptr() as _, cn);
        let r = c::cmsReadTag(h2, 0x63616C74);
        eprintln!("calt write={ok} read={}", r.is_null());
        c::cmsCloseProfile(h2);
        c::cmsCloseProfile(h);
    }
}

/// Type_ParametricCurve edge cases — fnType 3/4 boundary
/// parameter values.
#[test]
fn parametric_edge_cases() {
    unsafe {
        for (name, fntype, params) in [
            ("t3-x0-0", 3u16, [0x00010000i32, 0x00010000,
                               0x00010000, 0x00010000, 0]
                .as_slice()),
            ("t3-x0-1", 3, [0x00010000, 0x00010000,
                            0x00010000, 0x00010000,
                            0x00010000].as_slice()),
            ("t4-a-1", 4, [0x00010000, 0x00010000, 0x00010000,
                           0x00010000, 0x00010000,
                           0x00010000, 0x00010000]
                .as_slice()),
            ("t4-a-0", 4, [0x00010000, 0x00010000, 0, 0,
                           0x00010000, 0x00010000, 0]
                .as_slice()),
            ("t4-b-0", 4, [0x00010000, 0, 0x00010000,
                           0x00010000, 0x00010000,
                           0x00010000, 0x00010000]
                .as_slice()),
        ] {
            let mut b = Vec::new();
            b.extend_from_slice(b"para");
            b.extend_from_slice(&[0; 4]);
            b.extend_from_slice(&fntype.to_be_bytes());
            b.extend_from_slice(&[0; 2]); // reserved
            for p in params {
                b.extend_from_slice(&p.to_be_bytes());
            }
            let h = c::cmsCreate_sRGBProfile();
            let ok = c::cmsWriteRawTag(h, 0x72545243,
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
            let r = c::cmsReadTag(h3, 0x72545243);
            eprintln!("{name} write={ok} read={}", r.is_null());
            if !h3.is_null() { c::cmsCloseProfile(h3); }
            c::cmsCloseProfile(h);
        }
    }
}

/// Type_XYZ internals — 'XYZ ' bodies on the XYZ-accepting
/// sigs (1 + N-element variants).
#[test]
fn xyz_types() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        for (sig, name) in [(0x77747074u32, "wtpt"),
                            (0x626B7074u32, "bkpt"),
                            (0x6C756D69u32, "lumi"),
                            (0x7258595Au32, "rXYZ"),
                            (0x6758595Au32, "gXYZ"),
                            (0x6258595Au32, "bXYZ"),
                            (0x63686164u32, "chad")] {
            // 3-element XYZ body — the Type_XYZ_Read machinery
            let mut b = Vec::new();
            b.extend_from_slice(b"XYZ ");
            b.extend_from_slice(&[0; 4]);
            for i in 0..3u32 {
                b.extend_from_slice(
                    &(0x00010000i32 + (i as i32) * 0x10000)
                        .to_be_bytes());
                b.extend_from_slice(&0x00010000i32.to_be_bytes());
                b.extend_from_slice(&0x00010000i32.to_be_bytes());
            }
            let ok = c::cmsWriteRawTag(h, sig,
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
            let r = c::cmsReadTag(h3, sig);
            eprintln!("{name} write={ok} read={}", r.is_null());
            if !h3.is_null() { c::cmsCloseProfile(h3); }
        }
        c::cmsCloseProfile(h);
    }
}

/// Type_Chromaticity internals — 'chrm' with a proper
/// cmsCIExyYTRIPLE payload.
#[test]
fn chromaticity_payload() {
    unsafe {
        #[repr(C)]
        #[derive(Copy, Clone)]
        struct Xy {
            x: f64, y: f64,
        }
        #[repr(C)]
        struct XyyTriple {
            red: Xy, green: Xy, blue: Xy,
        }
        let trip = XyyTriple {
            red: Xy { x: 0.64, y: 0.33 },
            green: Xy { x: 0.30, y: 0.60 },
            blue: Xy { x: 0.15, y: 0.06 },
        };
        let h = c::cmsCreate_sRGBProfile();
        let ok = c::cmsWriteTag(h, 0x6368726D,
            &trip as *const _ as *mut std::ffi::c_void); // 'chrm'
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut cbuf = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, cbuf.as_mut_ptr() as *mut _,
            &mut cn);
        let h2 = c::cmsOpenProfileFromMem(cbuf.as_ptr() as _, cn);
        let r = c::cmsReadTag(h2, 0x6368726D);
        eprintln!("chrm write={ok} read={}", r.is_null());
        c::cmsCloseProfile(h2);
        c::cmsCloseProfile(h);
    }
}

/// Type_Curve internals — 'curv' bodies on the TRC sigs
/// (the per-channel curve machinery).
#[test]
fn curve_types() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        for (sig, name) in [(0x72545243u32, "rTRC"),
                            (0x67545243u32, "gTRC"),
                            (0x62545243u32, "bTRC"),
                            (0x6B545243u32, "kTRC")] {
            // 256-entry curve — the Type_Curve_Read machinery
            let mut b = Vec::new();
            b.extend_from_slice(b"curv");
            b.extend_from_slice(&[0; 4]);
            b.extend_from_slice(&256u32.to_be_bytes()); // count
            for i in 0..256u32 {
                b.extend_from_slice(&((i * 0x100) as u16)
                    .to_be_bytes());
            }
            let ok = c::cmsWriteRawTag(h, sig,
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
            let r = c::cmsReadTag(h3, sig);
            eprintln!("{name} write={ok} read={}", r.is_null());
            if !h3.is_null() { c::cmsCloseProfile(h3); }
        }
        c::cmsCloseProfile(h);
    }
}

/// Type_MPE internals — 'mpe ' bodies with different element
/// types (matrix/bACS/eACS/curve-set/CLUT dispatch).
#[test]
fn mpe_element_types2() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        // 'mpe ' element types — the element-dispatch machinery
        for (name, elem) in [
            ("cvst", b"cvst" as &[u8]),  // curve-set
            ("matf", b"matf"),           // matrix
            ("clut", b"clut"),           // clut
            ("bACS", b"bACS"),           // bACS
            ("eACS", b"eACS"),           // eACS
            ("xyz2", b"xyz2"),           // xyz2lab
            ("lab2", b"lab2"),           // lab2xyz
            ("ncl2", b"ncl2"),           // named
        ] {
            let mut b = Vec::new();
            b.extend_from_slice(b"mpe ");
            b.extend_from_slice(&[0; 4]);
            b.extend_from_slice(&1u32.to_be_bytes()); // 1 elem
            b.extend_from_slice(&[0; 4]); // offset
            b.extend_from_slice(elem);
            b.extend_from_slice(&[0u8; 32]); // elem data
            let ok = c::cmsWriteRawTag(h, 0x6D657461,
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
            let r = c::cmsReadTag(h3, 0x6D657461);
            eprintln!("{name} write={ok} read={}", r.is_null());
            if !h3.is_null() { c::cmsCloseProfile(h3); }
        }
        c::cmsCloseProfile(h);
    }
}

/// Type_Data internals — 'data' bodies on the data-accepting
/// sigs (the flag+bytes machinery).
#[test]
fn data_types() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        for (sig, name) in [
            (0x73637264u32, "scrd"), (0x6473636Du32, "dscm"),
            (0x76696C6Cu32, "vill"), (0x72696730u32, "rig0"),
            (0x74656368u32, "tech"), (0x736D6D6Du32, "smmm"),
            (0x636C6964u32, "clid"), (0x63696964u32, "ciid"),
            (0x63656964u32, "ceid")] {
            let mut b = Vec::new();
            b.extend_from_slice(b"data");
            b.extend_from_slice(&[0; 4]);
            b.extend_from_slice(&1u32.to_be_bytes()); // flag
            b.extend_from_slice(b"data-payload");
            let ok = c::cmsWriteRawTag(h, sig,
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
            let r = c::cmsReadTag(h3, sig);
            eprintln!("{name} write={ok} read={}", r.is_null());
            if !h3.is_null() { c::cmsCloseProfile(h3); }
        }
        c::cmsCloseProfile(h);
    }
}

/// Type_Signature + Text + TextDesc on the remaining sigs —
/// the final type-write sweep.
#[test]
fn final_type_writes() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        // 'desc'-type bodies on the desc-accepting sigs
        let desc_body = {
            let mut b = Vec::new();
            b.extend_from_slice(b"desc");
            b.extend_from_slice(&[0; 4]);
            let t = b"desc-text\0";
            b.extend_from_slice(&(t.len() as u32).to_be_bytes());
            b.extend_from_slice(t);
            while b.len() % 4 != 0 { b.push(0); }
            b.extend_from_slice(&[0; 67]);
            b
        };
        for (sig, name) in [
            (0x64657363u32, "desc"), (0x646D6464u32, "dmdd"),
            (0x646D6E64u32, "dmnd"), (0x6473636Du32, "dscm"),
            (0x76756564u32, "vued")] {
            let ok = c::cmsWriteRawTag(h, sig,
                desc_body.as_ptr() as *const std::ffi::c_void,
                desc_body.len() as u32);
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

/// Remaining array-type sigs — the array-type dispatch on
/// more type bodies.
#[test]
fn array_type_sigs() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        for (ty, name) in [
            (b"nui8" as &[u8], "nui8"), (b"nui16", "nui16"),
            (b"nui32", "nui32"), (b"nui64", "nui64"),
            (b"uf32", "uf32"), (b"uf64", "uf64"),
            (b"s15f", "s15f"), (b"ut16", "ut16"),
            (b"ut32", "ut32"), (b"uif8", "uif8"),
            (b"uif16", "uif16"), (b"uif32", "uif32"),
            (b"uif64", "uif64"), (b"sf32", "sf32"),
            (b"uf16", "uf16")] {
            // an array body on 'meta' — the array-type dispatch
            let mut b = Vec::new();
            b.extend_from_slice(ty);
            b.extend_from_slice(&[0; 4]);
            b.extend_from_slice(&2u32.to_be_bytes()); // count
            b.extend_from_slice(&[0u8; 16]); // element data
            let ok = c::cmsWriteRawTag(h, 0x6D657461,
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
            let r = c::cmsReadTag(h3, 0x6D657461);
            eprintln!("{name} write={ok} read={}", r.is_null());
            if !h3.is_null() { c::cmsCloseProfile(h3); }
        }
        c::cmsCloseProfile(h);
    }
}

/// Final Type_*_Read sweep — the remaining type sigs on a
/// generic sig.
#[test]
fn final_type_sweep2() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        for (sig, name) in [
            (0x4D4C5520u32, "mlu "), (0x74657874u32, "text"),
            (0x64617461u32, "data"), (0x6474696Du32, "dtim"),
            (0x73696720u32, "sig "), (0x6368726Du32, "chrm"),
            (0x6E636C32u32, "ncl2"), (0x6C757420u32, "lut "),
            (0x6D706574u32, "mpet"), (0x70736571u32, "pseq"),
            (0x70736964u32, "psid"), (0x75637262u32, "ucrb"),
            (0x62672020u32, "bg  "), (0x72657370u32, "resp"),
            (0x72637332u32, "rcs2"), (0x70617261u32, "para"),
            (0x75743136u32, "ut16"), (0x75743332u32, "ut32"),
            (0x75696638u32, "uif8"), (0x75696631u32, "uif16"),
            (0x75696633u32, "uif32"), (0x75696636u32, "uif64"),
            (0x73313566u32, "s15f"), (0x75663136u32, "uf16"),
            (0x75663332u32, "uf32"), (0x75663634u32, "uf64"),
            (0x6E756938u32, "nui8"), (0x6E756931u32, "nui16"),
            (0x6E756933u32, "nui32"), (0x6E756936u32, "nui64"),
            (0x75663820u32, "uf8 "), (0x75663136u32, "uf16"),
            (0x58595A20u32, "XYZ "), (0x64657363u32, "desc"),
            (0x63757276u32, "curv"), (0x7363726Eu32, "scrn"),
            (0x736F7560u32, "soup"), (0x76697577u32, "viw "),
            (0x6D656173u32, "meas"), (0x64696374u32, "dict")] {
            let r = c::cmsReadTag(h, sig);
            eprintln!("{name} read={}", r.is_null());
        }
        c::cmsCloseProfile(h);
    }
}

/// MPE element types on more combos — the MPE element
/// serialization internals.
#[test]
fn mpe_element_types3() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let pl = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let curves = [g, g, g];
        c::cmsPipelineInsertStage(pl, 1,
            c::cmsStageAllocToneCurves(
                std::ptr::null_mut(), 3, curves.as_ptr()));
        c::cmsPipelineInsertStage(pl, 1,
            c::cmsStageAllocIdentity(
                std::ptr::null_mut(), 3));
        // 'mpet' write with a 2-stage pipeline
        for (sig, name) in [
            (0x6D706574u32, "mpet"), (0x6D704220u32, "mpB "),
            (0x6D614220u32, "maB "), (0x6D624120u32, "mbA ")] {
            let ok = c::cmsWriteTag(h, sig,
                pl as *mut std::ffi::c_void);
            eprintln!("{name} write={ok}");
        }
        c::cmsPipelineFree(pl);
        c::cmsCloseProfile(h);
    }
}

/// MPE element types on more combos — the MPE element
/// serialization internals.
#[test]
fn mpe_element_types4() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        // build a canonical mAB pipeline: curve -> matrix -> CLUT
        let pl = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        let curves = [g, g, g];
        c::cmsPipelineInsertStage(pl, 1,
            c::cmsStageAllocToneCurves(
                std::ptr::null_mut(), 3, curves.as_ptr()));
        c::cmsPipelineInsertStage(pl, 1,
            c::cmsStageAllocMatrix(
                std::ptr::null_mut(), 3, 3,
                [1.0,0.0,0.0, 0.0,1.0,0.0, 0.0,0.0,1.0].as_ptr(),
                std::ptr::null()));
        c::cmsPipelineInsertStage(pl, 1,
            c::cmsStageAllocCLut16bit(
                std::ptr::null_mut(), 2, 3, 3,
                std::ptr::null()));
        for (sig, name) in [
            (0x6D706574u32, "mpet"), (0x6D614220u32, "maB "),
            (0x6D624120u32, "mbA "), (0x6D704220u32, "mpB ")] {
            let ok = c::cmsWriteTag(h, sig,
                pl as *mut std::ffi::c_void);
            eprintln!("{name} write={ok}");
        }
        c::cmsPipelineFree(pl);
        c::cmsCloseProfile(h);
    }
}

/// 'mpet' write on the devicelinks — the real MPE
/// serialization on a working profile.
#[test]
fn mpet_devicelink() {
    unsafe {
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mut curves = [g; 3];
        let dl = c::cmsCreateLinearizationDeviceLinkTHR(
            std::ptr::null_mut(), 0x52474220,
            curves.as_mut_ptr());
        // build a canonical mAB pipeline
        let pl = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        let curves2 = [g, g, g];
        c::cmsPipelineInsertStage(pl, 1,
            c::cmsStageAllocToneCurves(
                std::ptr::null_mut(), 3, curves2.as_ptr()));
        for (sig, name) in [
            (0x6D706574u32, "mpet"), (0x6D614220u32, "maB "),
            (0x6D624120u32, "mbA ")] {
            let ok = c::cmsWriteTag(dl, sig,
                pl as *mut std::ffi::c_void);
            eprintln!("{name} write={ok}");
        }
        c::cmsPipelineFree(pl);
        c::cmsCloseProfile(dl);
    }
}

/// Write dispatch on a stage-typed payload — the write
/// machinery on different object types.
#[test]
fn write_dispatch_stage() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let stage = c::cmsStageAllocIdentity(
            std::ptr::null_mut(), 3);
        for (sig, name) in [
            (0x41324230u32, "A2B0"), (0x42324130u32, "B2A0"),
            (0x67616D74u32, "gamt"), (0x70726530u32, "pre0"),
            (0x70726531u32, "pre1"), (0x70726532u32, "pre2"),
            (0x6D706574u32, "mpet"), (0x6D614220u32, "maB ")] {
            let ok = c::cmsWriteTag(h, sig,
                stage as *mut std::ffi::c_void);
            eprintln!("{name} write={ok}");
        }
        c::cmsStageFree(stage);
        c::cmsCloseProfile(h);
    }
}

/// Write dispatch on an MLU-typed payload — the write
/// machinery on different object types.
#[test]
fn write_dispatch_mlu() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let mlu = c::cmsMLUalloc(std::ptr::null_mut(), 1);
        c::cmsMLUsetWide(mlu, b"en\0".as_ptr() as _,
            b"US\0".as_ptr() as _,
            &[0x74u16, 0x65, 0x73, 0x74] as *const u16 as _);
        for (sig, name) in [
            (0x64657363u32, "desc"), (0x63707274u32, "cprt"),
            (0x646D6464u32, "dmdd"), (0x646D6E64u32, "dmnd"),
            (0x76756564u32, "vued"), (0x73637264u32, "scrd"),
            (0x63696973u32, "ciis"), (0x74656368u32, "tech"),
            (0x76756574u32, "vuet")] {
            let ok = c::cmsWriteTag(h, sig,
                mlu as *mut std::ffi::c_void);
            eprintln!("{name} write={ok}");
        }
        c::cmsMLUfree(mlu);
        c::cmsCloseProfile(h);
    }
}

/// Write dispatch on a named-color payload — the write
/// machinery on different object types.
#[test]
fn write_dispatch_named() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let ncl = c::cmsAllocNamedColorList(
            std::ptr::null_mut(), 1, 3,
            b"P\0".as_ptr() as _,
            b"S\0".as_ptr() as _);
        for (sig, name) in [
            (0x636C6F74u32, "clot"), (0x636C7274u32, "clrt"),
            (0x6E64696Eu32, "ndin"), (0x41324230u32, "A2B0"),
            (0x63696370u32, "cicp")] {
            let ok = c::cmsWriteTag(h, sig,
                ncl as *mut std::ffi::c_void);
            eprintln!("{name} write={ok}");
        }
        c::cmsFreeNamedColorList(ncl);
        c::cmsCloseProfile(h);
    }
}

/// Write dispatch on a seq-typed payload — the write
/// machinery on different object types.
#[test]
fn write_dispatch_seq() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let seq = c::cmsAllocProfileSequenceDescription(
            std::ptr::null_mut(), 1);
        for (sig, name) in [
            (0x70736571u32, "pseq"), (0x70736964u32, "psid")] {
            let ok = c::cmsWriteTag(h, sig,
                seq as *mut std::ffi::c_void);
            eprintln!("{name} write={ok}");
        }
        c::cmsFreeProfileSequenceDescription(seq);
        c::cmsCloseProfile(h);
    }
}

/// Write dispatch on a dict-typed payload — the write
/// machinery on different object types.
#[test]
fn write_dispatch_dict() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let dict = c::cmsDictAlloc(std::ptr::null_mut());
        c::cmsDictAddEntry(dict,
            b"k\0".as_ptr() as _, b"v\0".as_ptr() as _,
            std::ptr::null_mut(), std::ptr::null_mut());
        for (sig, name) in [
            (0x6D657461u32, "meta"), (0x64696374u32, "dict")] {
            let ok = c::cmsWriteTag(h, sig,
                dict as *mut std::ffi::c_void);
            eprintln!("{name} write={ok}");
        }
        c::cmsDictFree(dict);
        c::cmsCloseProfile(h);
    }
}

/// Write dispatch on a curve-typed payload — the write
/// machinery on different object types.
#[test]
fn write_dispatch_curve() {
    unsafe {
        let h = c::cmsCreate_sRGBProfile();
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        for (sig, name) in [
            (0x72545243u32, "rTRC"), (0x67545243u32, "gTRC"),
            (0x62545243u32, "bTRC"), (0x6B545243u32, "kTRC"),
            (0x63696370u32, "cicp")] {
            let ok = c::cmsWriteTag(h, sig,
                g as *mut std::ffi::c_void);
            eprintln!("{name} write={ok}");
        }
        c::cmsFreeToneCurve(g);
        c::cmsCloseProfile(h);
    }
}
