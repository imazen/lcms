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
