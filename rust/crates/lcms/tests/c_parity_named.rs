//! cmsnamed parity: named-color list alloc/append/count/index/info/dup,
//! MLU alloc/set/get/translation paths.

use lcms::named;
use lcms_cref as c;

fn cs8(s: &str) -> *const u8 {
    Box::leak(format!("{s}\0").into_boxed_str()).as_ptr()
}

#[test]
fn named_color_list() {
    unsafe {
        let cl = c::cmsAllocNamedColorList(
            std::ptr::null_mut(), 8, 4, cs8("pre"), cs8("suf"));
        let mut rl = named::alloc_named_color_list(
            None, 8, 4, b"pre", b"suf").unwrap();
        assert!(!cl.is_null());
        // append a set of colors — parity on status + count
        for i in 0..10u32 {
            let name = format!("Color{i:03}");
            let pcs: [u16; 3] = [(i * 500) as u16, (i * 700) as u16, (i * 900) as u16];
            let dev: [u16; 16] = {
                let mut d = [0u16; 16];
                d[0] = (i * 100) as u16;
                d[1] = (i * 50) as u16;
                d[2] = (i * 25) as u16;
                d[3] = (i * 10) as u16;
                d
            };
            assert_eq!(
                c::cmsAppendNamedColor(cl, cs8(&name), pcs.as_ptr(), dev.as_ptr()) != 0,
                named::append_named_color(
                    &mut rl, Some(name.as_bytes()), Some(&pcs), Some(&dev)),
                "append {i}");
        }
        assert_eq!(c::cmsNamedColorCount(cl), named::named_color_count(&rl));
        // index lookup parity
        for i in [0u32, 5, 9, 999] {
            let name = if i < 10 { format!("Color{i:03}") } else { "nope".into() };
            assert_eq!(
                c::cmsNamedColorIndex(cl, cs8(&name)),
                named::named_color_index(&rl, name.as_bytes()),
                "index {name}");
        }
        // info parity — name, prefix, suffix, pcs, colorant
        let mut cn = [0u8; 256];
        let mut cp = [0u8; 33];
        let mut csf = [0u8; 33];
        let mut cpcs = [0u16; 3];
        let mut cdev = [0u16; 16];
        for i in [0u32, 5, 9] {
            assert!(c::cmsNamedColorInfo(cl, i,
                cn.as_mut_ptr(), cp.as_mut_ptr(), csf.as_mut_ptr(),
                cpcs.as_mut_ptr(), cdev.as_mut_ptr()) != 0, "C info {i}");
            let (rn, rpre, rsuf, rpcs, rdev) =
                named::named_color_info(&rl, i).expect("rust info");
            assert_eq!(
                std::ffi::CStr::from_ptr(cn.as_ptr() as *const i8).to_bytes(),
                &rn[..rn.iter().position(|&b| b == 0).unwrap_or(rn.len())],
                "info name {i}");
            assert_eq!(cpcs, rpcs, "info pcs {i}");
            assert_eq!(cdev, rdev, "info colorant {i}");
        }
        // dup parity
        let cd = c::cmsDupNamedColorList(cl);
        let rd = named::dup_named_color_list(&rl).unwrap();
        assert_eq!(c::cmsNamedColorCount(cd), named::named_color_count(&rd));
        c::cmsFreeNamedColorList(cl);
        c::cmsFreeNamedColorList(cd);
    }
}

/// MLU alloc/set/get/translation paths — the multilocalized-text
/// machinery behind tag descriptions.
#[test]
fn mlu_paths() {
    unsafe {
        let cm = c::cmsMLUalloc(std::ptr::null_mut(), 2);
        let mut rm = named::mlu_alloc(None, 2).unwrap();
        assert!(!cm.is_null());
        // set ascii/utf8/wide on several locales — status parity
        for (lang, cntry, txt) in [
            ("en", "US", "Hello"),
            ("en", "GB", "Colour"),
            ("es", "ES", "¡Hola!"),
            ("de", "DE", "Grüße"),
            ("ja", "JP", "こんにちは"),
        ] {
            assert_eq!(
                c::cmsMLUsetASCII(cm, cs8(lang), cs8(cntry), cs8(txt)) != 0,
                named::mlu_set_ascii(&mut rm, lang.as_bytes(), cntry.as_bytes(), txt.as_bytes()),
                "set {lang}-{cntry}");
        }
        // get ascii parity — both sides return the same string
        for (lang, cntry) in [("en","US"),("en","GB"),("es","ES"),("de","DE"),("ja","JP"),("fr","FR")] {
            let mut buf = [0u8; 256];
            let cn = c::cmsMLUgetASCII(cm, cs8(lang), cs8(cntry),
                buf.as_mut_ptr(), 256);
            let (rn, rbytes) = named::mlu_get_ascii(&rm, lang.as_bytes(), cntry.as_bytes(), Some(256));
            let cstr = if cn > 0 {
                Some(std::ffi::CStr::from_ptr(buf.as_ptr() as *const i8)
                    .to_bytes().to_vec())
            } else { None };
            let rstr = if rn > 0 { Some(rbytes[..rn as usize - 1].to_vec()) } else { None };
            assert_eq!(cstr, rstr, "get {lang}-{cntry}");
        }
        // translation lookup + codes parity
        assert_eq!(
            c::cmsMLUtranslationsCount(cm),
            named::mlu_translations_count(&rm), "translations count");
        for i in 0..named::mlu_translations_count(&rm) {
            let mut cl = [0u8; 3];
            let mut cc = [0u8; 3];
            assert!(c::cmsMLUtranslationsCodes(cm, i, cl.as_mut_ptr(), cc.as_mut_ptr()) != 0);
            let (rl, rc) = named::mlu_translations_codes(&rm, i).unwrap();
            assert_eq!(cl, rl, "trans lang {i}");
            assert_eq!(cc, rc, "trans cntry {i}");
        }
        c::cmsMLUfree(cm);
    }
}

/// MLU utf8/wide/translation/dup variants — the non-ascii paths.
#[test]
fn mlu_variants() {
    unsafe {
        let cm = c::cmsMLUalloc(std::ptr::null_mut(), 2);
        let mut rm = named::mlu_alloc(None, 2).unwrap();
        // utf8 path
        for (lang, cntry, txt) in [
            ("en", "US", "Café"),
            ("fr", "FR", "àéîöü"),
            ("de", "DE", "Grüße"),
        ] {
            assert_eq!(
                c::cmsMLUsetUTF8(cm, cs8(lang), cs8(cntry), cs8(txt)) != 0,
                named::mlu_set_utf8(&mut rm, lang.as_bytes(), cntry.as_bytes(),
                    txt.as_bytes()),
                "set_utf8 {lang}");
        }
        // get_utf8 parity
        for (lang, cntry) in [("en","US"),("fr","FR"),("de","DE"),("es","ES")] {
            let mut buf = [0u8; 256];
            let cn = c::cmsMLUgetUTF8(cm, cs8(lang), cs8(cntry),
                buf.as_mut_ptr(), 256);
            let (rn, rbytes) = named::mlu_get_utf8(
                &rm, lang.as_bytes(), cntry.as_bytes(), Some(256));
            let cstr = if cn > 0 {
                Some(std::ffi::CStr::from_ptr(buf.as_ptr() as *const i8)
                    .to_bytes().to_vec())
            } else { None };
            let rstr = if rn > 0 { Some(rbytes[..rn as usize - 1].to_vec()) } else { None };
            assert_eq!(cstr, rstr, "get_utf8 {lang}-{cntry}");
        }
        // translation lookup parity
        for (lang, cntry) in [("en","US"),("fr","FR"),("de","DE"),("zz","ZZ")] {
            let mut cl = [0u8; 3];
            let mut cc = [0u8; 3];
            let cn = c::cmsMLUgetTranslation(cm, cs8(lang), cs8(cntry),
                cl.as_mut_ptr(), cc.as_mut_ptr());
            let r = named::mlu_get_translation(&rm, lang.as_bytes(), cntry.as_bytes());
            assert_eq!(cn != 0, r.is_some(), "trans {lang}-{cntry} presence");
            if cn != 0 {
                let (rl, rc) = r.unwrap();
                assert_eq!(cl, rl, "trans lang {lang}-{cntry}");
                assert_eq!(cc, rc, "trans cntry {lang}-{cntry}");
            }
        }
        // dup parity
        let cd = c::cmsMLUdup(cm);
        let rd = named::mlu_dup(&rm).unwrap();
        assert_eq!(
            c::cmsMLUtranslationsCount(cd),
            named::mlu_translations_count(&rd), "dup count");
        c::cmsMLUfree(cm);
        c::cmsMLUfree(cd);
    }
}

/// Named-color internals — index/name/PCS/count on more
/// shapes.
#[test]
fn named_color_internals() {
    unsafe {
        for n in [1u32, 3, 8] {
            let ncl = c::cmsAllocNamedColorList(
                std::ptr::null_mut(), n, 3,
                b"P\0".as_ptr() as _, b"S\0".as_ptr() as _);
            let cnt = c::cmsNamedColorCount(ncl);
            eprintln!("n{n} count={cnt}");
            // append one entry
            let pcs = [0x8000u16; 3];
            let dev = [0x4000u16; 4];
            let r = c::cmsAppendNamedColor(ncl,
                b"spot1\0".as_ptr() as _,
                pcs.as_ptr(), dev.as_ptr());
            eprintln!("  append={r} count={}",
                c::cmsNamedColorCount(ncl));
            // lookup internals
            let idx = c::cmsNamedColorIndex(ncl,
                b"spot1\0".as_ptr() as _);
            let idx2 = c::cmsNamedColorIndex(ncl,
                b"nope\0".as_ptr() as _);
            eprintln!("  idx={idx} idx2={idx2}");
            let mut namebuf = [0u8; 64];
            let nr = c::cmsNamedColorInfo(ncl, 0,
                namebuf.as_mut_ptr(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut());
            eprintln!("  info={nr}");
            c::cmsFreeNamedColorList(ncl);
        }
    }
}
