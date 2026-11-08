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

/// Named-color devicelink — an 'nmcl'-class input profile makes the
/// transform's LUT begin with a NamedColor stage, so
/// cmsTransform2DeviceLink dispatches into CreateNamedColorDevicelink
/// (cmsvirt.c:1081). The named-color transform output is compared
/// C-vs-Rust; the devicelink conversion runs C-side for coverage.
#[test]
fn named_color_devicelink() {
    unsafe {
        // C side: nmcl profile + 2-color list + Lab PCS
        let p = c::cmsCreateProfilePlaceholder(std::ptr::null_mut());
        c::cmsSetDeviceClass(p, 0x6e6d636c); // 'nmcl'
        c::cmsSetColorSpace(p, 0x6e636c32); // 'ncl2'
        c::cmsSetPCS(p, 0x4C616220); // 'Lab '
        let ncl = c::cmsAllocNamedColorList(
            std::ptr::null_mut(), 2, 4, b"\0".as_ptr(), b"\0".as_ptr());
        let pcs = [0x8000u16, 0x8000, 0x8000];
        let col = [0xFFFFu16, 0, 0, 0];
        c::cmsAppendNamedColor(ncl, b"Color1\0".as_ptr(),
            pcs.as_ptr(), col.as_ptr());
        c::cmsAppendNamedColor(ncl, b"Color2\0".as_ptr(),
            pcs.as_ptr(), col.as_ptr());
        assert_ne!(c::cmsWriteTag(p, 0x6e636c32, ncl as *const _), 0);
        let srgb = c::cmsCreate_sRGBProfile();
        let nc_fmt: u32 = (1 << 3) | 2; // TYPE_NAMED_COLOR_INDEX
        let rgb8: u32 = (4 << 16) | (3 << 3) | 1;
        let t = c::cmsCreateTransform(p, nc_fmt, srgb, rgb8, 0, 0);
        assert!(!t.is_null());
        let inp = [0u16, 1, 0, 1];
        let mut c_out = vec![0u8; 12];
        c::cmsDoTransform(t, inp.as_ptr() as _,
            c_out.as_mut_ptr() as _, 4);
        // CreateNamedColorDevicelink — the named-color stage head
        let dl = c::cmsTransform2DeviceLink(t, 4.3, 0);
        eprintln!("named-color devicelink={}", !dl.is_null());
        if !dl.is_null() { c::cmsCloseProfile(dl); }
        c::cmsDeleteTransform(t);
        // Rust side: same profile + transform
        let mut rp = lcms::profile::create_profile_placeholder(None);
        rp.device_class = lcms::signatures::profileclass::SigNamedColorClass;
        rp.color_space = 0x6e636c32;
        rp.pcs = 0x4C616220;
        let rp: lcms::profile::IccProfile = *rp;
        let mut rncl = lcms::named::alloc_named_color_list(
            None, 2, 4, b"", b"").unwrap();
        lcms::named::append_named_color(&mut rncl,
            Some(b"Color1"), Some(&pcs), Some(&col_pad(col)));
        lcms::named::append_named_color(&mut rncl,
            Some(b"Color2"), Some(&pcs), Some(&col_pad(col)));
        let mut rp = rp;
        assert!(lcms::profile::cms_write_tag(&mut rp, 0x6e636c32,
            Some(lcms::tagtypes::TagObject::NamedColor(rncl))));
        let mut rs = lcms::virt::create_srgb_profile(None).unwrap();
        let mut profs: Vec<Box<lcms::profile::IccProfile>> =
            vec![Box::new(rp), rs];
        let mut bpc = vec![false; 2];
        let intents = vec![0u32; 2];
        let adapt = vec![1.0f64; 2];
        let mut rt = lcms::xform::create_extended_transform(
            None, &mut profs, &mut bpc, &intents, &adapt,
            None, 0, nc_fmt, rgb8, 0);
        if let Some(rt) = rt.as_mut() {
            let mut r_out = vec![0u8; 12];
            let rin: Vec<u8> = inp.iter()
                .flat_map(|v| v.to_le_bytes()).collect();
            rt.do_transform(&rin, &mut r_out, 4);
            assert_eq!(c_out, r_out, "named-color transform");
        } else {
            panic!("rust named-color transform failed");
        }
        c::cmsCloseProfile(p);
        c::cmsCloseProfile(srgb);
        c::cmsFreeNamedColorList(ncl);
    }
}

fn col_pad(c: [u16; 4]) -> [u16; 16] {
    let mut v = [0u16; 16];
    v[..4].copy_from_slice(&c);
    v
}

/// UTF8 round-trip — MLUsetUTF8/MLUgetUTF8 with 1/2/3/4-byte
/// sequences exercises every encodeUTF8/decodeUTF8 branch.
#[test]
fn utf8_mlu_roundtrip() {
    unsafe {
        let strs: &[&[u8]] = &[
            b"ascii",
            "caf\u{e9}".as_bytes(),           // 2-byte é
            "\u{20ac}uro".as_bytes(),          // 3-byte €
            "\u{1f600}".as_bytes(),            // 4-byte emoji
            "a\u{e9}\u{20ac}\u{1f600}z".as_bytes(), // mixed
        ];
        for s in strs {
            let cm = c::cmsMLUalloc(std::ptr::null_mut(), 1);
            let mut cstr = s.to_vec(); cstr.push(0);
            let ok = c::cmsMLUsetUTF8(cm, b"en".as_ptr(),
                b"US".as_ptr(), cstr.as_ptr());
            let mut buf = vec![0u8; 256];
            let n = c::cmsMLUgetUTF8(cm, b"en".as_ptr(),
                b"US".as_ptr(), buf.as_mut_ptr(), buf.len() as u32);
            let cout = buf[..n as usize].to_vec();
            // rust side
            let mut rm = lcms::named::mlu_alloc(None, 1).unwrap();
            let rok = lcms::named::mlu_set_utf8(&mut rm,
                b"en", b"US", s);
            let (_n, rout) = lcms::named::mlu_get_utf8(&rm, b"en", b"US",
                Some(256));
            assert_eq!(ok != 0, rok, "set utf8 {:?} parity",
                String::from_utf8_lossy(s));
            assert_eq!(cout, rout, "get utf8 {:?} parity",
                String::from_utf8_lossy(s));
            c::cmsMLUfree(cm);
        }
    }
}

/// Named-color ops sweep — AppendNamedColor grow/NULL-default/name-
/// cap arms, NamedColorCount/Info, list dup/free, over-capacity.
#[test]
fn named_color_ops() {
    unsafe {
        let ncl = c::cmsAllocNamedColorList(
            std::ptr::null_mut(), 2, 4, b"pre\0".as_ptr(),
            b"suf\0".as_ptr());
        assert!(!ncl.is_null());
        // appends past initial capacity -> GrowNamedColorList
        for i in 0..20u32 {
            let name = format!("c{i}\0");
            let pcs = [0x1000u16, 0x2000, 0x3000];
            let col = [0xFFFFu16; 4];
            c::cmsAppendNamedColor(ncl, name.as_ptr(),
                pcs.as_ptr(), col.as_ptr());
        }
        // NULL-colorant / NULL-pcs / NULL-name defaults
        c::cmsAppendNamedColor(ncl, std::ptr::null(),
            std::ptr::null(), std::ptr::null());
        // over-cap name (strncpy cap at cmsMAX_PATH-1)
        let long = "x".repeat(300) + "\0";
        c::cmsAppendNamedColor(ncl, long.as_ptr(),
            std::ptr::null(), std::ptr::null());
        let n = c::cmsNamedColorCount(ncl);
        eprintln!("count={n}");
        // NamedColorInfo reads
        for i in [0u32, 10, 21] {
            let mut nm = [0u8; 256];
            let mut pcs = [0u16; 3];
            let mut col = [0u16; 16];
            let r = c::cmsNamedColorInfo(ncl, i, nm.as_mut_ptr(),
                std::ptr::null_mut(), std::ptr::null_mut(),
                pcs.as_mut_ptr(), col.as_mut_ptr());
            eprintln!("info[{i}]={r} name={:?} pcs={pcs:?}",
                std::ffi::CStr::from_ptr(nm.as_ptr() as *const i8)
                    .to_str().unwrap_or("?"));
        }
        let dp = c::cmsDupNamedColorList(ncl);
        if !dp.is_null() { c::cmsFreeNamedColorList(dp); }
        c::cmsFreeNamedColorList(ncl);
    }
}

/// ncl2 tag write/read — named-color list with prefix/suffix MLU
/// blocks through a profile tag -> ReadNamedColorList/
/// WriteNamedColorList bodies + UTF16 name paths.
#[test]
fn named_color_tag_cycle() {
    unsafe {
        let ncl = c::cmsAllocNamedColorList(
            std::ptr::null_mut(), 3, 4, b"pre\0".as_ptr(),
            b"suf\0".as_ptr());
        for (name, pcs) in [
            ("PANTONE Red\0", [0x9000u16, 0x5500, 0x5500]),
            ("Spot Blue\0", [0x3000, 0x2000, 0x9000]),
            ("Metallic\0", [0x7000, 0x7000, 0x6000]),
        ] {
            let col = [0xFFFFu16; 4];
            c::cmsAppendNamedColor(ncl, name.as_ptr(),
                pcs.as_ptr(), col.as_ptr());
        }
        let p = c::cmsCreateProfilePlaceholder(std::ptr::null_mut());
        c::cmsSetDeviceClass(p, 0x6e6d636c);
        c::cmsSetColorSpace(p, 0x6e636c32);
        c::cmsSetPCS(p, 0x4C616220);
        let w = c::cmsWriteTag(p, 0x6e636c32, ncl as *const _);
        let mut buf = vec![0u8; 1 << 20];
        let mut n = buf.len() as u32;
        let s = if w != 0 {
            c::cmsSaveProfileToMem(p,
                buf.as_mut_ptr() as *mut _, &mut n)
        } else { 0 };
        eprintln!("ncl2 write={w} save={s} n={n}");
        if s != 0 {
            let rp = c::cmsOpenProfileFromMem(
                buf.as_ptr() as *const _, n);
            let tag = c::cmsReadTag(rp, 0x6e636c32);
            eprintln!("ncl2 read={}", !tag.is_null());
            if !tag.is_null() {
                let cnt = c::cmsNamedColorCount(tag);
                eprintln!("  count={cnt}");
            }
            c::cmsCloseProfile(rp);
        }
        c::cmsCloseProfile(p);
        c::cmsFreeNamedColorList(ncl);
    }
}

/// Named-color MPE stage — _cmsStageAllocNamedColor (bAcs/eAcs
/// element) through pipeline eval + transform position.
#[test]
fn named_color_stage() {
    unsafe {
        let ncl = c::cmsAllocNamedColorList(
            std::ptr::null_mut(), 2, 4, b"pre\0".as_ptr(),
            b"suf\0".as_ptr());
        for (name, pcs) in [
            ("Spot1\0", [0x9000u16, 0x5500, 0x5500]),
            ("Spot2\0", [0x3000, 0x2000, 0x9000]),
        ] {
            let col = [0xFFFFu16; 4];
            c::cmsAppendNamedColor(ncl, name.as_ptr(),
                pcs.as_ptr(), col.as_ptr());
        }
        for use_pcs in [0i32, 1] {
            let st = c::_cmsStageAllocNamedColor(ncl, use_pcs);
            eprintln!("nstage pcs={use_pcs} = {}", !st.is_null());
            if !st.is_null() {
                let p = c::cmsPipelineAlloc(
                    std::ptr::null_mut(), 1, 3);
                c::cmsPipelineInsertStage(p, 1, st);
                let fi = [0.5f32];
                let mut fo = [0f32; 3];
                c::cmsPipelineEvalFloat(fi.as_ptr(),
                    fo.as_mut_ptr(), p);
                eprintln!("  eval={fo:?}");
                c::cmsPipelineFree(p);
            }
        }
        c::cmsFreeNamedColorList(ncl);
    }
}

/// MLU op branches — cmsMLUsetUTF8/getUTF8/setWide/getWide/setASCII/
/// getTranslation on multi-entry MLUs + growth arms.
#[test]
fn mlu_ops_branches() {
    unsafe {
        let m = c::cmsMLUalloc(std::ptr::null_mut(), 2);
        assert!(!m.is_null());
        // UTF8 + wide + ascii sets on multiple langs
        c::cmsMLUsetUTF8(m, b"en\0".as_ptr(), b"US\0".as_ptr(),
            b"English\0".as_ptr());
        let wstr: [i32; 5] = [0x57, 0x69, 0x64, 0x65, 0];
        c::cmsMLUsetWide(m, b"fr\0".as_ptr(), b"FR\0".as_ptr(),
            wstr.as_ptr() as *const i32);
        c::cmsMLUsetASCII(m, b"de\0".as_ptr(), b"DE\0".as_ptr(),
            b"Deutsch\0".as_ptr());
        // gets
        let mut buf = vec![0u8; 256];
        let n = c::cmsMLUgetUTF8(m, b"en\0".as_ptr(), b"US\0".as_ptr(),
            buf.as_mut_ptr(), buf.len() as u32);
        eprintln!("getUTF8 n={n} {:?}", 
            std::ffi::CStr::from_ptr(buf.as_ptr() as *const i8));
        let mut wbuf = vec![0i32; 256];
        let n2 = c::cmsMLUgetWide(m, b"fr\0".as_ptr(), b"FR\0".as_ptr(),
            wbuf.as_mut_ptr(), (wbuf.len() * 4) as u32);
        eprintln!("getWide n={n2}");
        let mut abuf = vec![0u8; 256];
        let n3 = c::cmsMLUgetASCII(m, b"de\0".as_ptr(), b"DE\0".as_ptr(),
            abuf.as_mut_ptr(), abuf.len() as u32);
        eprintln!("getASCII n={n3}");
        // getTranslation fallback (lang not present)
        let mut ol = [0u8; 4];
        let mut oc = [0u8; 4];
        let n4 = c::cmsMLUgetTranslation(m, b"zz\0".as_ptr(),
            b"ZZ\0".as_ptr(), ol.as_mut_ptr(), oc.as_mut_ptr());
        eprintln!("getTrans-miss n={n4}");
        let tc = c::cmsMLUtranslationsCount(m);
        eprintln!("translations={tc}");
        // dup + more entries (growth past initial 2)
        let dp = c::cmsMLUdup(m);
        c::cmsMLUsetUTF8(dp, b"ja\0".as_ptr(), b"JP\0".as_ptr(),
            b"j\0".as_ptr());
        let cnt = c::cmsMLUtranslationsCount(dp);
        eprintln!("dup entries={cnt}");
        if !dp.is_null() { c::cmsMLUfree(dp); }
        c::cmsMLUfree(m);
    }
}

/// MLU serialization — multi-entry MLU through a profile tag ->
/// ReadMlu/WriteMlu bodies (entry-count, offset-alignment, UTF16
/// blocks), padded/odd-length strings.
#[test]
fn mlu_serialization_blocks() {
    unsafe {
        let m = c::cmsMLUalloc(std::ptr::null_mut(), 1);
        // entries with varied lengths -> offset alignment arms
        for (lang, ctry, txt) in [
            ("en", "US", "a"),
            ("fr", "FR", "longer text here"),
            ("de", "DE", "x"),
            ("ja", "JP", "even"),
        ] {
            let langz = format!("{lang}\0");
            let ctryz = format!("{ctry}\0");
            let txtz = format!("{txt}\0");
            c::cmsMLUsetUTF8(m, langz.as_ptr(), ctryz.as_ptr(),
                txtz.as_ptr());
        }
        let prof = c::cmsCreate_sRGBProfile();
        c::cmsWriteTag(prof, 0x64657363, m as *const _); // 'desc'
        let mut buf = vec![0u8; 1 << 20];
        let mut n = buf.len() as u32;
        let s = c::cmsSaveProfileToMem(prof,
            buf.as_mut_ptr() as *mut _, &mut n);
        let rp = if s != 0 {
            c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
        } else { std::ptr::null_mut() };
        let tag = if !rp.is_null() {
            c::cmsReadTag(rp, 0x64657363)
        } else { std::ptr::null_mut() };
        eprintln!("mlu write->read={}", !tag.is_null());
        if !tag.is_null() {
            let cnt = c::cmsMLUtranslationsCount(tag);
            eprintln!("  entries={cnt}");
        }
        if !rp.is_null() { c::cmsCloseProfile(rp); }
        c::cmsCloseProfile(prof);
        c::cmsMLUfree(m);
    }
}

/// Dictionary tag — cmsDictAddEntry entries -> dict tag
/// write->read -> Type_Dictionary serializers + entry enumeration.
#[test]
fn dict_tag_cycle() {
    unsafe {
        let d = c::cmsDictAlloc(std::ptr::null_mut());
        let w = |x: &str| {
            let mut v: Vec<i32> = x.chars().map(|c| c as i32).collect();
            v.push(0);
            v
        };
        for (name, val) in [("k1", "v1"), ("key2", "longer-value"),
                            ("a", "b")] {
            let nz = w(name);
            let vz = w(val);
            c::cmsDictAddEntry(d, nz.as_ptr(),
                vz.as_ptr(),
                std::ptr::null_mut(), std::ptr::null_mut());
        }
        let prof = c::cmsCreate_sRGBProfile();
        c::cmsWriteTag(prof, 0x6d657461, d as *const _); // 'pimc'
        let mut buf = vec![0u8; 1 << 20];
        let mut n = buf.len() as u32;
        let s = c::cmsSaveProfileToMem(prof,
            buf.as_mut_ptr() as *mut _, &mut n);
        let rp = if s != 0 {
            c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
        } else { std::ptr::null_mut() };
        let tag = if !rp.is_null() {
            c::cmsReadTag(rp, 0x6d657461)
        } else { std::ptr::null_mut() };
        eprintln!("dict w s={s} read={}", !tag.is_null());
        if !tag.is_null() {
            let mut e = c::cmsDictGetEntryList(tag);
            let mut cnt = 0;
            while !e.is_null() {
                cnt += 1;
                e = c::cmsDictNextEntry(e);
            }
            eprintln!("  entries={cnt}");
        }
        if !rp.is_null() { c::cmsCloseProfile(rp); }
        c::cmsCloseProfile(prof);
        c::cmsDictFree(d);
    }
}

/// MLU offset-alignment internals — many-entry MLU with odd/even
/// string lengths -> the offset computation + padding arms.
#[test]
fn mlu_offset_alignment() {
    unsafe {
        let m = c::cmsMLUalloc(std::ptr::null_mut(), 1);
        // lengths 1,2,3,...,16 -> every offset-alignment arm
        for i in 1..=16usize {
            let txt = "x".repeat(i);
            let lang = b"en\0";
            let ctry = b"US\0";
            let tz = format!("{txt}\0");
            // vary lang/ctry to force distinct entries
            let lz = format!("e{}\0", (i as u8 + 96) as char);
            c::cmsMLUsetUTF8(m, lz.as_ptr(), ctry.as_ptr(), tz.as_ptr());
        }
        let prof = c::cmsCreate_sRGBProfile();
        c::cmsWriteTag(prof, 0x64657363, m as *const _);
        let mut buf = vec![0u8; 1 << 20];
        let mut n = buf.len() as u32;
        let s = c::cmsSaveProfileToMem(prof,
            buf.as_mut_ptr() as *mut _, &mut n);
        let rp = if s != 0 {
            c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
        } else { std::ptr::null_mut() };
        let tag = if !rp.is_null() {
            c::cmsReadTag(rp, 0x64657363)
        } else { std::ptr::null_mut() };
        eprintln!("mlu-align s={s} read={}", !tag.is_null());
        if !tag.is_null() {
            eprintln!("  entries={}", c::cmsMLUtranslationsCount(tag));
        }
        if !rp.is_null() { c::cmsCloseProfile(rp); }
        c::cmsCloseProfile(prof);
        c::cmsMLUfree(m);
    }
}

/// MLU translation internals — setWide/getASCII/getTranslation/
/// translationsCodes, entry-add growth arms.
#[test]
fn mlu_translation_internals() {
    unsafe {
        let m = c::cmsMLUalloc(std::ptr::null_mut(), 1);
        // setWide + setUTF8 + setASCII
        let w: Vec<i32> = "wide text".chars()
            .map(|c| c as i32).chain([0]).collect();
        c::cmsMLUsetWide(m, b"en\0".as_ptr(), b"US\0".as_ptr(),
            w.as_ptr());
        c::cmsMLUsetUTF8(m, b"fr\0".as_ptr(), b"FR\0".as_ptr(),
            b"utf8".as_ptr());
        let mut wb = [0i32; 64];
        let mut ab = [0u8; 64];
        let n1 = c::cmsMLUgetWide(m, b"en\0".as_ptr(), b"US\0".as_ptr(),
            wb.as_mut_ptr(), 64);
        let n2 = c::cmsMLUgetASCII(m, b"en\0".as_ptr(), b"US\0".as_ptr(),
            ab.as_mut_ptr(), 64);
        eprintln!("wide={n1} ascii={n2}");
        // translation enumeration
        let cnt = c::cmsMLUtranslationsCount(m);
        for i in 0..cnt {
            let mut lang = [0u8; 4];
            let mut ctry = [0u8; 4];
            let _ = c::cmsMLUtranslationsCodes(m, i,
                lang.as_mut_ptr(), ctry.as_mut_ptr());
        }
        // getTranslation — returns the entry for a lang/ctry
        let mut ol = [0u8; 4];
        let mut oc = [0u8; 4];
        let t = c::cmsMLUgetTranslation(m, b"en\0".as_ptr(), b"US\0".as_ptr(),
            ol.as_mut_ptr(), oc.as_mut_ptr());
        eprintln!("trans={cnt} gett={t}");
        c::cmsMLUfree(m);
    }
}

/// Named-color index/info internals — NamedColorIndex lookup,
/// NamedColorInfo, GetNamedColorList via transform.
#[test]
fn named_color_index_info() {
    unsafe {
        let ncl = c::cmsAllocNamedColorList(std::ptr::null_mut(), 3,
            3, b"pre\0".as_ptr(), b"suf\0".as_ptr());
        let lab = [0xF000u16, 0x8000, 0x8000];
        for n in [b"c1\0".as_ptr(), b"c2\0".as_ptr(), b"c3\0".as_ptr()] {
            c::cmsAppendNamedColor(ncl, n,
                lab.as_ptr(), lab.as_ptr());
        }
        eprintln!("count={}", c::cmsNamedColorCount(ncl));
        // index lookups — hit + miss
        for n in [b"c1\0".as_ptr(), b"c2\0".as_ptr(), b"zz\0".as_ptr()] {
            let i = c::cmsNamedColorIndex(ncl, n);
            eprintln!("idx={i}");
        }
        // NamedColorInfo — name/pcs/colorant out-params
        let mut name = [0u8; 64];
        let mut pcs = [0u16; 4];
        let mut col = [0u16; 4];
        for i in 0..3i32 {
            let mut pre = [0u8; 64];
            let mut suf = [0u8; 64];
            let r = c::cmsNamedColorInfo(ncl, i as u32,
                name.as_mut_ptr(), pre.as_mut_ptr(), suf.as_mut_ptr(),
                pcs.as_mut_ptr(), col.as_mut_ptr());
            eprintln!("info{i}={r}");
        }
        // GetNamedColorList via a transform on an nmcl profile
        let prof = c::cmsCreateProfilePlaceholder(std::ptr::null_mut());
        c::cmsSetDeviceClass(prof, 0x6e6d636c);
        c::cmsSetColorSpace(prof, 0x6e6d636c);
        c::cmsSetPCS(prof, 0x4C616220);
        c::cmsWriteTag(prof, 0x6e636c32, ncl as *const _);
        let t = c::cmsCreateTransform(prof,
            (0xFFu32 << 16) | (1 << 3) | 1,
            prof, (10u32 << 16) | (3 << 3) | 2, 0, 0);
        if !t.is_null() {
            let gl = c::cmsGetNamedColorList(t);
            eprintln!("getncl={}", !gl.is_null());
            c::cmsDeleteTransform(t);
        }
        c::cmsCloseProfile(prof);
        c::cmsFreeNamedColorList(ncl);
    }
}

/// Dictionary internals — entries with display-name/value/
/// encoding variants -> Type_Dictionary offset computation.
#[test]
fn dictionary_internals() {
    unsafe {
        let d = c::cmsDictAlloc(std::ptr::null_mut());
        // entries with name, value, display-name, display-value
        let w = |s: &str| s.chars().map(|c| c as i32)
            .chain([0]).collect::<Vec<i32>>();
        let dn = c::cmsMLUalloc(std::ptr::null_mut(), 1);
        c::cmsMLUsetUTF8(dn, b"en\0".as_ptr() as *const _,
            b"US\0".as_ptr() as *const _, b"disp\0".as_ptr());
        c::cmsDictAddEntry(d, w("k1").as_ptr(), w("v1").as_ptr(),
            std::ptr::null_mut(), std::ptr::null_mut());
        c::cmsDictAddEntry(d, w("k2").as_ptr(), w("v2").as_ptr(),
            dn, dn);
        c::cmsDictAddEntry(d, w("k3").as_ptr(), w("v3").as_ptr(),
            dn, std::ptr::null_mut());
        let head = c::cmsDictGetEntryList(d);
        // enumerate
        if !head.is_null() {
            let e1 = c::cmsDictNextEntry(head);
            let e2 = c::cmsDictNextEntry(e1);
            let e3 = c::cmsDictNextEntry(e2);
            eprintln!("head={} e2={} e3={}",
                !head.is_null(), !e2.is_null(), !e3.is_null());
        }
        // serialize into meta tag
        let prof = c::cmsCreate_sRGBProfile();
        let wtag = c::cmsWriteTag(prof, 0x6d657461, d as *const _);
        let mut buf = vec![0u8; 1 << 20];
        let mut n = buf.len() as u32;
        let s = if wtag != 0 {
            c::cmsSaveProfileToMem(prof,
                buf.as_mut_ptr() as *mut _, &mut n)
        } else { 0 };
        let rp = if s != 0 {
            c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
        } else { std::ptr::null_mut() };
        let tag = if !rp.is_null() {
            c::cmsReadTag(rp, 0x6d657461)
        } else { std::ptr::null_mut() };
        eprintln!("dict w={wtag} s={s} read={}", !tag.is_null());
        if !rp.is_null() { c::cmsCloseProfile(rp); }
        c::cmsCloseProfile(prof);
        c::cmsDictFree(d);
    }
}
