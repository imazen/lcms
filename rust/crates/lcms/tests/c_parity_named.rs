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

/// Type_MLU internals — multi-entry MLU tag serialization with
/// odd-length strings -> offset-table + padding arms.
#[test]
fn mlu_serialization() {
    unsafe {
        let m = c::cmsMLUalloc(std::ptr::null_mut(), 1);
        for (lang, ctry, txt) in [
            ("en\0", "US\0", "short"),
            ("de\0", "DE\0", "a much longer description string"),
            ("fr\0", "FR\0", "x"),
            ("ja\0", "JP\0", "unicode \u{4e2d}\u{6587}"),
        ] {
            c::cmsMLUsetUTF8(m,
                lang.as_ptr() as *const i8 as *const u8,
                ctry.as_ptr() as *const i8 as *const u8,
                txt.as_ptr());
        }
        let prof = c::cmsCreate_sRGBProfile();
        let w = c::cmsWriteTag(prof, 0x64657363, m as *const _);
        let mut buf = vec![0u8; 1 << 20];
        let mut n = buf.len() as u32;
        let s = if w != 0 {
            c::cmsSaveProfileToMem(prof,
                buf.as_mut_ptr() as *mut _, &mut n)
        } else { 0 };
        let rp = if s != 0 {
            c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
        } else { std::ptr::null_mut() };
        let tag = if !rp.is_null() {
            c::cmsReadTag(rp, 0x64657363)
        } else { std::ptr::null_mut() };
        eprintln!("mlu w={w} s={s} read={}", !tag.is_null());
        if !rp.is_null() { c::cmsCloseProfile(rp); }
        c::cmsCloseProfile(prof);
        c::cmsMLUfree(m);
    }
}

/// Type_MLU/Type_Dictionary internals — multi-translation MLU +
/// dict with varied name/value/display combos.
#[test]
fn mlu_dict_internals() {
    unsafe {
        // MLU with 6 translations of varied lengths
        let m = c::cmsMLUalloc(std::ptr::null_mut(), 1);
        for (l, c_, t) in [
            ("en\0","US\0","a"), ("de\0","DE\0","medium text"),
            ("fr\0","FR\0","x"), ("ja\0","JP\0","jp"),
            ("zh\0","CN\0","zh"), ("it\0","IT\0","it"),
        ] {
            c::cmsMLUsetUTF8(m, l.as_ptr() as *const _,
                c_.as_ptr() as *const _, t.as_ptr());
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
        eprintln!("mlu s={s} read={} entries={}",
            !tag.is_null(),
            if !tag.is_null() {
                c::cmsMLUtranslationsCount(tag) } else { 0 });
        if !rp.is_null() { c::cmsCloseProfile(rp); }
        c::cmsCloseProfile(prof);
        c::cmsMLUfree(m);
        // dict with name+value+display combos
        let d = c::cmsDictAlloc(std::ptr::null_mut());
        let w = |s: &str| s.chars().map(|c| c as i32)
            .chain([0]).collect::<Vec<i32>>();
        let dn = c::cmsMLUalloc(std::ptr::null_mut(), 1);
        c::cmsMLUsetUTF8(dn, b"en\0".as_ptr() as *const _,
            b"US\0".as_ptr() as *const _, b"d\0".as_ptr());
        c::cmsDictAddEntry(d, w("k1").as_ptr(), w("v1").as_ptr(),
            dn, dn);
        c::cmsDictAddEntry(d, w("k2").as_ptr(), w("v2").as_ptr(),
            std::ptr::null_mut(), dn);
        c::cmsDictAddEntry(d, w("k3").as_ptr(), w("v3").as_ptr(),
            dn, std::ptr::null_mut());
        c::cmsDictAddEntry(d, w("k4").as_ptr(), w("v4").as_ptr(),
            std::ptr::null_mut(), std::ptr::null_mut());
        let prof2 = c::cmsCreate_sRGBProfile();
        let wt = c::cmsWriteTag(prof2, 0x6d657461, d as *const _);
        let s2 = if wt != 0 {
            c::cmsSaveProfileToMem(prof2,
                buf.as_mut_ptr() as *mut _, &mut n)
        } else { 0 };
        let rp2 = if s2 != 0 {
            c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
        } else { std::ptr::null_mut() };
        let tag2 = if !rp2.is_null() {
            c::cmsReadTag(rp2, 0x6d657461)
        } else { std::ptr::null_mut() };
        eprintln!("dict s={s2} read={}", !tag2.is_null());
        if !rp2.is_null() { c::cmsCloseProfile(rp2); }
        c::cmsCloseProfile(prof2);
        c::cmsDictFree(d);
        c::cmsMLUfree(dn);
    }
}

/// Type_NamedColor_Write arms — ncl2 with varying colorant
/// counts, prefix/suffix strings and entry counts.
#[test]
fn named_color_write_arms() {
    unsafe {
        for (n_colorant, n_entries, pre, suf, name) in [
            (3u32, 1, b"p\0".as_ptr(), b"s\0".as_ptr(), "c3-e1"),
            (4, 2, b"pre\0".as_ptr(), b"suf\0".as_ptr(), "c4-e2"),
            (0, 1, b"\0".as_ptr(), b"\0".as_ptr(), "c0-e1"),
            (15, 3, b"long-prefix\0".as_ptr(),
             b"long-suffix\0".as_ptr(), "c15-e3"),
        ] {
            let ncl = c::cmsAllocNamedColorList(std::ptr::null_mut(),
                n_entries, n_colorant, pre, suf);
            let mut pcs = [0x8000u16; 3];
            let mut cols = [0x4000u16; 16];
            for i in 0..n_entries {
                let nm = format!("col{}\0", i);
                pcs[0] = (i * 1000) as u16;
                cols[0] = (i * 2000) as u16;
                c::cmsAppendNamedColor(ncl, nm.as_ptr(),
                    pcs.as_ptr(), cols.as_ptr());
            }
            let prof = c::cmsCreate_sRGBProfile();
            let w = c::cmsWriteTag(prof, 0x6e636c32, ncl as *const _);
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = if w != 0 {
                c::cmsSaveProfileToMem(prof,
                    buf.as_mut_ptr() as *mut _, &mut n)
            } else { 0 };
            let rp = if s != 0 {
                c::cmsOpenProfileFromMem(buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            let tag = if !rp.is_null() {
                c::cmsReadTag(rp, 0x6e636c32)
            } else { std::ptr::null_mut() };
            eprintln!("{name} w={w} s={s} read={}", !tag.is_null());
            if !rp.is_null() { c::cmsCloseProfile(rp); }
            c::cmsCloseProfile(prof);
            c::cmsFreeNamedColorList(ncl);
        }
    }
}

/// cmsnamed residual — decodeUTF8 on multibyte sequences in
///   named-color lists + GetNamedColorList internals.
#[test]
fn utf8_decode_arms() {
    unsafe {
        let nl = c::cmsAllocNamedColorList(
            std::ptr::null_mut(), 16, 4, b"p\0".as_ptr(),
            b"s\0".as_ptr());
        // UTF-8 sequences of every valid/invalid length
        let names: &[&[u8]] = &[
            b"ascii\0",
            b"caf\xc3\xa9\0",           // 2-byte
            b"\xe2\x82\xac\0",           // 3-byte euro
            b"\xf0\x9f\x98\x80\0",       // 4-byte emoji
            b"mix\xc3\xa9e2\x82\xac\0",  // mixed
            b"\xc3\0",                   // truncated 2-byte
            b"\xe2\x82\0",               // truncated 3-byte
            b"\xf0\x9f\x98\0",           // truncated 4-byte
            b"\x80\0",                   // stray cont
            b"\xff\0",                   // invalid byte
            b"\xc0\xaf\0",               // overlong
            b"\xed\xa0\x80\0",           // surrogate
            b"\xf4\x90\x80\x80\0",       // >U+10FFFF
            b"a\x00b\0",
        ];
        let mut pcs = [0x1000u16; 4];
        let mut col = [0x2000u16; 4];
        for (i, nm) in names.iter().enumerate() {
            pcs[0] = i as u16;
            let r = c::cmsAppendNamedColor(nl,
                nm.as_ptr() as *const u8,
                pcs.as_ptr(), col.as_ptr());
            eprintln!("utf8 i{i} append={r}");
        }
        // named-color profile write + reload round-trip
        let prof = c::cmsCreateProfilePlaceholder(
            std::ptr::null_mut());
        c::cmsSetDeviceClass(prof, 0x6E6D636C);
        c::cmsSetColorSpace(prof, 0x4E434C20);
        c::cmsSetPCS(prof, 0x4C616220);
        c::cmsSetProfileVersion(prof, 4.4);
        let w = c::cmsWriteTag(prof, 0x6E636C32,
            nl as *const _);
        let mut buf = vec![0u8; 1 << 20];
        let mut n = buf.len() as u32;
        let s = c::cmsSaveProfileToMem(prof,
            buf.as_mut_ptr() as *mut _, &mut n);
        let rp = if s != 0 {
            c::cmsOpenProfileFromMem(buf.as_ptr()
                as *const _, n)
        } else { std::ptr::null_mut() };
        let r = if !rp.is_null() {
            let t = c::cmsReadTag(rp, 0x6E636C32);
            let ok = !t.is_null();
            // read names back via NamedColorInfo
            if ok {
                let nl2 = t as *mut std::ffi::c_void;
                for i in 0..14u32 {
                    let mut nm = [0u8; 256];
                    let mut pcs2 = [0u16; 4];
                    let mut col2 = [0u16; 4];
                    let rr = c::cmsNamedColorInfo(nl2, i,
                        nm.as_mut_ptr(),
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        pcs2.as_mut_ptr(), col2.as_mut_ptr());
                    if rr != 0 {
                        eprintln!("  n{i}={:?}",
                            std::str::from_utf8(
                                &nm[..nm.iter().position(
                                    |&b| b == 0)
                                    .unwrap_or(0)])
                                .unwrap_or("<bad>"));
                    }
                }
            }
            c::cmsCloseProfile(rp);
            ok
        } else { false };
        eprintln!("ncl2-utf8 w={w} s={s} r={r}");
        c::cmsCloseProfile(prof);
        c::cmsFreeNamedColorList(nl);
    }
}

/// cmsnamed residual — decodeUTF8 edge sequences +
///   DupNamedColorList/DictDup internals.
#[test]
fn named_utf8_dup_arms() {
    unsafe {
        let ctx = c::cmsCreateContext(
            std::ptr::null_mut(), std::ptr::null_mut());
        // named-color list with all UTF-8 sequence classes
        for (n_, name) in [
            (1u32, "ascii"), (4, "multi"),
        ] {
            let nl = c::cmsAllocNamedColorList(ctx, n_,
                4, b"p\0".as_ptr(), b"s\0".as_ptr());
            let mut pcs = [0x1000u16; 16];
            let col = [0x2000u16; 16];
            // UTF-8 sequences: 1/2/3/4-byte valid, overlong,
            //   surrogate-range, truncated, continuation-
            //   only, invalid lead bytes
            let names: Vec<Vec<u8>> = vec![
                b"a\0".to_vec(),
                b"\xC3\xA9\0".to_vec(),           // é 2B
                b"\xE2\x82\xAC\0".to_vec(),       // € 3B
                b"\xF0\x9F\x98\x80\0".to_vec(),   // 😀 4B
                b"\xC0\x80\0".to_vec(),           // overlong /
                b"\xE0\x80\x80\0".to_vec(),       // overlong 3
                b"\xF0\x80\x80\x80\0".to_vec(),   // overlong 4
                b"\x80\0".to_vec(),               // cont-only
                b"\xC3\0".to_vec(),               // truncated 2
                b"\xE2\x82\0".to_vec(),           // truncated 3
                b"\xFF\0".to_vec(),               // invalid FF
                b"\xFE\0".to_vec(),               // invalid FE
                b"\xC2\x20\0".to_vec(),           // 2B + ASCII
                b"\xED\xA0\x80\0".to_vec(),       // surrogate
                b"\xF4\x8F\xBF\xBF\0".to_vec(),   // U+10FFFF
                b"\xF5\x80\x80\x80\0".to_vec(),   // >U+10FFFF
                b"abc\xE2\x82\xACxyz\0".to_vec(), // mixed
            ];
            for (i, nm) in names.iter().enumerate() {
                pcs[0] = i as u16;
                let r = c::cmsAppendNamedColor(nl,
                    nm.as_ptr(), pcs.as_ptr(),
                    col.as_ptr());
                eprintln!("{name} [{i}] ap={r}");
            }
            // read back → decodeUTF8 on each
            let cnt = c::cmsNamedColorCount(nl);
            eprintln!("  cnt={cnt}");
            for i in 0..cnt.min(4) {
                let mut nm = [0u8; 64];
                let mut pfx = [0u8; 64];
                let mut sfx = [0u8; 64];
                let mut pcs2 = [0u16; 16];
                let mut col2 = [0u16; 16];
                let info = c::cmsNamedColorInfo(nl, i,
                    nm.as_mut_ptr(),
                    pfx.as_mut_ptr(),
                    sfx.as_mut_ptr(),
                    pcs2.as_mut_ptr(),
                    col2.as_mut_ptr());
                eprintln!("  info[{i}]={info}");
            }
            // dup the list → DupNamedColorList
            let d = c::cmsDupNamedColorList(nl);
            eprintln!("  dup={}", !d.is_null());
            if !d.is_null() {
                c::cmsFreeNamedColorList(d); }
            c::cmsFreeNamedColorList(nl);
        }
        // dict dup via ctx-dup + DictDup directly
        let dict = c::cmsDictAlloc(ctx);
        let mut kw = [0i32; 4];
        let mut vw = [0i32; 4];
        for (j, ch) in "k\0".chars().enumerate() {
            kw[j] = ch as i32; }
        for (j, ch) in "v\0".chars().enumerate() {
            vw[j] = ch as i32; }
        c::cmsDictAddEntry(dict, kw.as_ptr(),
            vw.as_ptr(), std::ptr::null_mut(),
            std::ptr::null_mut());
        let dd = c::cmsDictDup(dict);
        eprintln!("dictdup={}", !dd.is_null());
        if !dd.is_null() { c::cmsDictFree(dd); }
        c::cmsDictFree(dict);
        c::cmsDeleteContext(ctx);
    }
}

/// cmsnamed residual — surrogate-pair decode paths in
///   decodeUTF8/strFrom16 via MLUsetWide on surrogate
///   and astral-plane sequences.
#[test]
fn named_surrogate_arms() {
    unsafe {
        let mlu = c::cmsMLUalloc(std::ptr::null_mut(), 4);
        // astral chars as surrogate pairs + bare codepoints
        for (seq, name) in [
            (&[0x0041i32, 0xD83D, 0xDE00, 0x0042, 0][..],
                "pair-smile"),
            (&[0xD800i32, 0xDC00, 0][..], "pair-min"),
            (&[0xDBFFi32, 0xDFFF, 0][..], "pair-max"),
            (&[0x10437i32, 0][..], "astral-direct"),
            (&[0xD800i32, 0][..], "lone-hi"),
            (&[0xDC00i32, 0][..], "lone-lo"),
            (&[0xD7FFi32, 0xE000, 0][..], "edge-ok"),
        ] {
            let ok = c::cmsMLUsetWide(mlu,
                b"en\0".as_ptr() as *const _,
                b"US\0".as_ptr() as *const _,
                seq.as_ptr());
            let mut buf = [0i32; 64];
            let n = c::cmsMLUgetWide(mlu,
                b"en\0".as_ptr() as *const _,
                b"US\0".as_ptr() as *const _,
                buf.as_mut_ptr(), 64 * 2);
            let mut abuf = [0u8; 128];
            let na = c::cmsMLUgetUTF8(mlu,
                b"en\0".as_ptr() as *const _,
                b"US\0".as_ptr() as *const _,
                abuf.as_mut_ptr(), 128);
            eprintln!("{name} set={ok} w={n} u8={na}");
        }
        c::cmsMLUfree(mlu);
        // NamedColorInfo with surrogate names
        let nl = c::cmsAllocNamedColorList(
            std::ptr::null_mut(), 4, 3,
            b"P\0".as_ptr() as *const _,
            b"S\0".as_ptr() as *const _);
        let pcs = [0x1000u16, 0x2000, 0x3000];
        let dev = [0x4000u16, 0x5000, 0x6000];
        // isolate: ASCII name first
        let ascii = b"plain\0";
        let oka = c::cmsAppendNamedColor(nl,
            ascii.as_ptr() as *const _,
            pcs.as_ptr(), dev.as_ptr());
        eprintln!("nc-ascii={oka}");
        let sur = [0xF0u8, 0x9F, 0x98, 0x80, 0]; // 😀 utf8
        let ok = c::cmsAppendNamedColor(nl,
            sur.as_ptr() as *const _,
            pcs.as_ptr(), dev.as_ptr());
        let mut nm = [0u8; 64];
        let mut pfx = [0u8; 8];
        let mut sfx = [0u8; 8];
        let mut pc = [0u16; 3];
        let mut dv = [0u16; 16];
        let r = if ok != 0 {
            c::cmsNamedColorInfo(nl, 0,
                nm.as_mut_ptr(), pfx.as_mut_ptr(),
                sfx.as_mut_ptr(), pc.as_mut_ptr(),
                dv.as_mut_ptr())
        } else { -1 };
        eprintln!("nc-sur append={ok} info={r} \
            nm0={:#x}", nm[0]);
        c::cmsFreeNamedColorList(nl);
    }
}

/// cmsnamed/cmsio0 residual — GrowNamedColorList via
///   appends past initial capacity, profile save with
///   ncl2+pseq+dict+mlu tags (SaveTags internals),
///   OpenProfileFromFile on the serialized output.
#[test]
fn named_grow_save_arms() {
    unsafe {
        // named list: alloc small, append past cap
        let nl = c::cmsAllocNamedColorList(
            std::ptr::null_mut(), 2, 4,
            b"P\0".as_ptr() as *const _,
            b"S\0".as_ptr() as *const _);
        let mut pcs = [0u16; 16];
        let mut dev = [0u16; 16];
        for i in 0..64u32 {
            let nm = format!("n{}\0", i);
            for k in 0..4 {
                pcs[k] = (i * 7 + k as u32) as u16;
                dev[k] = (i * 11 + k as u32) as u16;
            }
            let ok = c::cmsAppendNamedColor(nl,
                nm.as_ptr() as *const _,
                pcs.as_ptr(), dev.as_ptr());
            if i < 4 || i % 16 == 0 {
                eprintln!("append[{i}] {ok}"); }
        }
        eprintln!("count={}", c::cmsNamedColorCount(nl));
        // profile with ncl2 + pseq + dict tags -> Save
        let p = c::cmsCreateProfilePlaceholder(
            std::ptr::null_mut());
        c::cmsSetDeviceClass(p, 0x6E6D636C);
        c::cmsSetColorSpace(p, 0x4D434834);
        c::cmsSetPCS(p, 0x4C616220);
        c::cmsSetProfileVersion(p, 4.4);
        let xyz = [0x0000F6D6u32, 0x00010000, 0x0000D32D];
        c::cmsWriteTag(p, 0x77747074,
            xyz.as_ptr() as *const _);
        c::cmsWriteTag(p, 0x6E636C32,
            nl as *const _);
        let dict = c::cmsDictAlloc(std::ptr::null_mut());
        c::cmsDictAddEntry(dict,
            b"k1\0".as_ptr() as *const _,
            b"v1\0".as_ptr() as *const _,
            std::ptr::null_mut(), std::ptr::null_mut());
        c::cmsDictAddEntry(dict,
            b"k2\0".as_ptr() as *const _,
            b"v2\0".as_ptr() as *const _,
            std::ptr::null_mut(), std::ptr::null_mut());
        c::cmsWriteTag(p, 0x6D657461,
            dict as *const _);
        let mlu = c::cmsMLUalloc(std::ptr::null_mut(), 1);
        c::cmsMLUsetASCII(mlu,
            b"en\0".as_ptr() as *const _,
            b"US\0".as_ptr() as *const _,
            b"desc\0".as_ptr() as *const _);
        c::cmsWriteTag(p, 0x64657363,
            mlu as *const _);
        let path = std::ffi::CString::new(
            "/tmp/lcms_named_save.icc").unwrap();
        let sf = c::cmsSaveProfileToFile(p, path.as_ptr());
        eprintln!("save-file={sf}");
        // open the file back
        let pf = c::cmsOpenProfileFromFile(
            path.as_ptr(), b"r\0".as_ptr() as *const _);
        eprintln!("reopen-file={}", !pf.is_null());
        if !pf.is_null() {
            let rd = c::cmsReadTag(pf, 0x6E636C32);
            eprintln!("  ncl2 rd={}", !rd.is_null());
            let rd2 = c::cmsReadTag(pf, 0x6D657461);
            eprintln!("  dict rd={}", !rd2.is_null());
            let rd3 = c::cmsReadTag(pf, 0x64657363);
            eprintln!("  mlu rd={}", !rd3.is_null());
            c::cmsCloseProfile(pf);
        }
        // OpenProfileFromFile variants: missing, dir,
        //   mode combos
        for (pth, mode, name) in [
            ("/tmp/lcms_named_save.icc", "r", "real-r"),
            ("/tmp/lcms_named_save.icc", "rb", "real-rb"),
            ("/tmp/nope_lcms_x.icc", "r", "missing"),
            ("/tmp", "r", "isdir"),
            ("/tmp/lcms_named_save.icc", "w", "real-w"),
            ("/tmp/lcms_named_save.icc", "x", "badmode"),
        ] {
            let cp = std::ffi::CString::new(pth).unwrap();
            let cm = std::ffi::CString::new(mode).unwrap();
            let pp = c::cmsOpenProfileFromFile(
                cp.as_ptr(), cm.as_ptr() as *const u8);
            eprintln!("{name} prof={}", !pp.is_null());
            if !pp.is_null() {
                c::cmsCloseProfile(pp); }
        }
        let _ = std::fs::remove_file(
            "/tmp/lcms_named_save.icc");
        c::cmsCloseProfile(p);
        c::cmsDictFree(dict);
        c::cmsMLUfree(mlu);
        c::cmsFreeNamedColorList(nl);
    }
}

/// cmsnamed residual — NamedColorInfo full-field
///   reads + index-by-name + NamedColorIndex +
///   GrowNamedColorList to 256+.
#[test]
fn named_info_grow_arms() {
    unsafe {
        let nl = c::cmsAllocNamedColorList(
            std::ptr::null_mut(), 4, 4,
            b"PREF\0".as_ptr() as *const _,
            b"SUFF\0".as_ptr() as *const _);
        // grow past 256 (several grows)
        for i in 0..300u32 {
            let nm = format!("color{}\0", i);
            let pcs = [i as u16; 16];
            let dev = [(i * 2) as u16; 16];
            c::cmsAppendNamedColor(nl,
                nm.as_ptr() as *const _,
                pcs.as_ptr(), dev.as_ptr());
        }
        eprintln!("count={}", c::cmsNamedColorCount(nl));
        // NamedColorInfo on indices incl OOB
        for idx in [0u32, 1, 299, 300, 9999] {
            let mut name = [0u8; 256];
            let mut prefix = [0u8; 256];
            let mut suffix = [0u8; 256];
            let mut pcs = [0u16; 16];
            let mut dev = [0u16; 16];
            let ok = c::cmsNamedColorInfo(nl, idx,
                name.as_mut_ptr(),
                prefix.as_mut_ptr(),
                suffix.as_mut_ptr(),
                pcs.as_mut_ptr(), dev.as_mut_ptr());
            if idx <= 299 {
                let s = std::ffi::CStr::from_ptr(
                    name.as_ptr() as *const i8)
                    .to_string_lossy();
                let p = std::ffi::CStr::from_ptr(
                    prefix.as_ptr() as *const i8)
                    .to_string_lossy();
                let sf = std::ffi::CStr::from_ptr(
                    suffix.as_ptr() as *const i8)
                    .to_string_lossy();
                eprintln!("info[{idx}] {ok} '{s}' \
                    '{p}' '{sf}' pcs={}", pcs[0]);
            } else {
                eprintln!("info[{idx}] {ok}");
            }
        }
        // index-by-name
        for nm in ["color0\0", "color150\0", "color299\0",
                   "nope\0", "COLOR0\0"] {
            let i = c::cmsNamedColorIndex(nl,
                nm.as_ptr() as *const _);
            eprintln!("idx {nm:?} = {i}");
        }
        // dup + free the big list
        let d = c::cmsDupNamedColorList(nl);
        eprintln!("dup300={}", !d.is_null());
        if !d.is_null() {
            c::cmsFreeNamedColorList(d); }
        c::cmsFreeNamedColorList(nl);
    }
}
