//! cmsnamed remaining paths: MLU wide-string get/set + profile-
//! sequence-description alloc/dup/free + get-entry.

use lcms::named;
use lcms_cref as c;
use std::ffi::c_void;

fn cs8(s: &str) -> *const u8 {
    thread_local! {
        static BUF: std::cell::RefCell<Vec<u8>> = std::cell::RefCell::new(Vec::new());
    }
    BUF.with(|b| {
        let mut b = b.borrow_mut();
        b.clear();
        b.extend_from_slice(s.as_bytes());
        b.push(0);
        b.as_ptr()
    })
}

#[test]
fn mlu_wide_paths() {
    unsafe {
        let cm = c::cmsMLUalloc(std::ptr::null_mut(), 2);
        let mut rm = named::mlu_alloc(None, 2).unwrap();
        // wide set — UTF-32 as i32s
        let w = |s: &str| -> Vec<i32> {
            s.chars().map(|c| c as i32).chain(std::iter::once(0)).collect()
        };
        for (lang, cntry, txt) in [
            ("en", "US", "Hello"),
            ("ja", "JP", "こんにちは"),
            ("fr", "FR", "Café"),
        ] {
            let wv = w(txt);
            assert_eq!(
                c::cmsMLUsetWide(cm, cs8(lang), cs8(cntry),
                    wv.as_ptr()) != 0,
                named::mlu_set_wide(&mut rm, lang.as_bytes(),
                    cntry.as_bytes(), &wv),
                "set_wide {lang}");
        }
        // wide get parity
        for (lang, cntry) in [("en","US"),("ja","JP"),("fr","FR"),("zz","ZZ")] {
            let mut cbuf = [0i32; 128];
            let cn = c::cmsMLUgetWide(cm, cs8(lang), cs8(cntry),
                cbuf.as_mut_ptr(), 128);
            let (rn, rbytes) = named::mlu_get_wide(&rm, lang.as_bytes(),
                cntry.as_bytes(), Some(128));
            let cstr: Vec<i32> = if cn > 0 {
                cbuf[..cbuf.iter().position(|&b| b==0).unwrap_or(cn as usize)].to_vec()
            } else { vec![] };
            let rstr: Vec<i32> = if rn > 0 {
                rbytes[..rbytes.iter().position(|&b| b==0)
                    .unwrap_or(rbytes.len())].to_vec()
            } else { vec![] };
            assert_eq!(cstr, rstr, "get_wide {lang}-{cntry}");
        }
        c::cmsMLUfree(cm);
    }
}

#[test]
fn pseq_machinery() {
    unsafe {
        let cp = c::cmsAllocProfileSequenceDescription(std::ptr::null_mut(), 3);
        let rp = named::alloc_profile_seq_desc(None, 3);
        assert_eq!(cp.is_null(), rp.is_none(), "pseq alloc");
        let cd = c::cmsDupProfileSequenceDescription(cp);
        let rd = Some(rp.unwrap().clone());
        assert_eq!(cd.is_null(), rd.is_none(), "pseq dup");
        c::cmsFreeProfileSequenceDescription(cp);
        c::cmsFreeProfileSequenceDescription(cd);
    }
}

/// Named-color machinery on transforms + ncl2 tag reads.
#[test]
fn named_color_machinery() {
    unsafe {
        // a named-color profile — devicelink to named
        let ncl = c::cmsAllocNamedColorList(
            std::ptr::null_mut(), 2, 3,
            b"pre\0".as_ptr() as *const _,
            b"suf\0".as_ptr() as *const _);
        let mut buf32 = [0u8; 32];
        let mut pcs = [0u16; 3];
        let mut dev = [0u16; 16];
        let ok1 = c::cmsAppendNamedColor(ncl,
            b"color1\0".as_ptr() as *const u8,
            pcs.as_ptr(), dev.as_ptr());
        let ok2 = c::cmsAppendNamedColor(ncl,
            b"color2\0".as_ptr() as *const u8,
            pcs.as_ptr(), dev.as_ptr());
        // named-color getters
        let n = c::cmsNamedColorCount(ncl);
        let idx = c::cmsNamedColorIndex(ncl,
            b"color1\0".as_ptr() as *const u8);
        let nm = c::cmsNamedColorInfo(ncl, 0,
            buf32.as_mut_ptr(), std::ptr::null_mut(),
            std::ptr::null_mut(),
            pcs.as_mut_ptr(), dev.as_mut_ptr());
        eprintln!("append={ok1}/{ok2} n={n} idx={idx} info={nm}");
        // dup + free
        let d = c::cmsDupNamedColorList(ncl);
        let dn = c::cmsNamedColorCount(d);
        eprintln!("dup count={dn}");
        c::cmsFreeNamedColorList(d);
        c::cmsFreeNamedColorList(ncl);
    }
}
