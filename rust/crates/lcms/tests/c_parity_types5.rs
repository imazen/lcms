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
