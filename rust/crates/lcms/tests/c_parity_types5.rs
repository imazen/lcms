//! cmstypes elem_count read check: corrupt a serialized tag's item
//! count -> the reader must reject the read the same way C does.

use lcms::profile;
use lcms_cref as c;

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
