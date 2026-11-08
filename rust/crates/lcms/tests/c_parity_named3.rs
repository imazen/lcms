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

/// Named-color profile machinery — 'ncl2' tag read/write +
/// the nmcl-class intent machinery.
#[test]
fn named_color_profile() {
    unsafe {
        // build a named-color profile via a named devicelink
        let ncl = c::cmsAllocNamedColorList(
            std::ptr::null_mut(), 2, 3,
            b"pre\0".as_ptr() as *const _,
            b"suf\0".as_ptr() as *const _);
        let mut pcs = [0u16; 3];
        let mut dev = [0u16; 16];
        c::cmsAppendNamedColor(ncl,
            b"c1\0".as_ptr() as *const u8,
            pcs.as_ptr(), dev.as_ptr());
        c::cmsAppendNamedColor(ncl,
            b"c2\0".as_ptr() as *const u8,
            pcs.as_ptr(), dev.as_ptr());
        // write the ncl2 list into a profile tag
        let h = c::cmsCreate_sRGBProfile();
        let ok = c::cmsWriteTag(h, 0x6E636C32,
            ncl as *mut std::ffi::c_void); // 'ncl2'
        eprintln!("ncl2 write={ok}");
        // resave + read
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut cbuf = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, cbuf.as_mut_ptr() as *mut _,
            &mut cn);
        let h2 = c::cmsOpenProfileFromMem(cbuf.as_ptr() as _, cn);
        let r = c::cmsReadTag(h2, 0x6E636C32);
        eprintln!("ncl2 read={}", r.is_null());
        if c::cmsIsTag(h2, 0x6E636C32) != 0 {
            let mut tb = [0u8; 4];
            c::cmsReadRawTag(h2, 0x6E636C32,
                tb.as_mut_ptr() as *mut _, 4);
            eprintln!("ncl2 type={:#010x}",
                u32::from_be_bytes(tb));
        }
        c::cmsCloseProfile(h2);
        c::cmsCloseProfile(h);
        c::cmsFreeNamedColorList(ncl);
    }
}

/// Named-color transform internals — the named-color
/// transform pipeline on more format combos.
#[test]
fn named_transform_formats() {
    unsafe {
        // build a named-color transform — input named-color
        // index, output Lab16
        let ncl = c::cmsAllocNamedColorList(
            std::ptr::null_mut(), 4, 3,
            b"pre\0".as_ptr() as *const _,
            b"suf\0".as_ptr() as *const _);
        for i in 0..4u32 {
            let mut pcs = [(0x8000u16) + i as u16 * 0x1000,
                           0x8000, 0x8000];
            let mut dev = [0u16; 16];
            dev[i as usize] = 0xFFFF;
            let name = [
                [b'c', b'1', 0],
                [b'c', b'2', 0],
                [b'c', b'3', 0],
                [b'c', b'4', 0],
            ][i as usize];
            c::cmsAppendNamedColor(ncl,
                name.as_ptr() as *const u8,
                pcs.as_ptr(), dev.as_ptr());
        }
        // a named-color PROFILE — write the ncl2 list into
        // a profile's ncl2 tag, then transform from it
        let h = c::cmsCreate_sRGBProfile();
        let ok = c::cmsWriteTag(h, 0x6E636C32,
            ncl as *mut std::ffi::c_void);
        c::cmsSetDeviceClass(h, 0x6E6D636C); // 'nmcl'
        c::cmsSetColorSpace(h, 0x6E6D636C); // cmsSigNamedData
        c::cmsSetPCS(h, 0x4C616220);        // Lab PCS
        // transform named->Lab through the profile's ncl2
        // single-profile named transform — cmscnvrt.c:576
        // fires on nmcl class + nProfiles==1 (multiprofile
        // with a 1-element array)
        let nc_fmt: u32 = (1 << 3) | 2; // TYPE_NAMED_COLOR_INDEX
        let lab_fmt: u32 = (10 << 16) | (3 << 3) | 2;
        let mut profs = [h];
        let ct = c::cmsCreateMultiprofileTransform(
            profs.as_mut_ptr(), 1, nc_fmt, lab_fmt, 0, 0);
        eprintln!("named write={ok} transform null={}",
            ct.is_null());
        if !ct.is_null() {
            let inp = [0u16];
            let mut co = [0u16; 3];
            c::cmsDoTransform(ct, inp.as_ptr() as _,
                co.as_mut_ptr() as *mut _, 1);
            eprintln!("named eval={:?}", co);
            c::cmsDeleteTransform(ct);
        }
        c::cmsCloseProfile(h);
        c::cmsFreeNamedColorList(ncl);
    }
}

/// Type_NamedColor2 internals — a larger ncl2 list (16
/// colors) through the element-dispatch machinery.
#[test]
fn ncl2_large() {
    unsafe {
        let ncl = c::cmsAllocNamedColorList(
            std::ptr::null_mut(), 16, 6,
            b"pre\0".as_ptr() as *const _,
            b"suf\0".as_ptr() as *const _);
        for i in 0..16u32 {
            let mut pcs = [(0x4000u16) + i as u16 * 0x400,
                           0x8000, 0x8000];
            let mut dev = [0u16; 16];
            for j in 0..6 {
                dev[j] = ((i as u32) * (j as u32) * 0x40) as u16;
            }
            let name = [b'n', 0, 0, 0, 0, 0, 0, 0];
            let mut nm = name;
            nm[1] = b'0' + (i / 10) as u8;
            nm[2] = b'0' + (i % 10) as u8;
            c::cmsAppendNamedColor(ncl,
                nm.as_ptr() as *const u8,
                pcs.as_ptr(), dev.as_ptr());
        }
        let h = c::cmsCreate_sRGBProfile();
        let ok = c::cmsWriteTag(h, 0x6E636C32,
            ncl as *mut std::ffi::c_void);
        let mut cn = 0u32;
        c::cmsSaveProfileToMem(h, std::ptr::null_mut(), &mut cn);
        let mut cbuf = vec![0u8; cn as usize];
        c::cmsSaveProfileToMem(h, cbuf.as_mut_ptr() as *mut _,
            &mut cn);
        eprintln!("ncl2 write={ok} save={cn}");
        let h2 = c::cmsOpenProfileFromMem(cbuf.as_ptr() as _, cn);
        let r = c::cmsReadTag(h2, 0x6E636C32);
        eprintln!("ncl2 read={}", r.is_null());
        if !r.is_null() {
            // read back the list's info
            let n2 = c::cmsNamedColorCount(r);
            eprintln!("read ncl2 count={n2}");
        }
        c::cmsCloseProfile(h2);
        c::cmsCloseProfile(h);
        c::cmsFreeNamedColorList(ncl);
    }
}
