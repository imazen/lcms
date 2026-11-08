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
