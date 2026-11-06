//! Differential test: `lcms::named` vs `cmsnamed.c`.

use lcms::named::*;
use lcms_cref as c;
use std::ffi::CString;
use std::ptr;

fn cstr(s: &str) -> CString {
    CString::new(s).unwrap()
}

/// Build identical MLUs on both sides via the same op sequence.
fn build_pair(ops: &[(u8, u8, &str, &str)]) -> (c::Handle, Mlu) {
    let c_mlu = unsafe { c::cmsMLUalloc(ptr::null_mut(), 0) };
    let mut r_mlu = mlu_alloc(None, 0).unwrap();
    for &(lang0, lang1, cntry, text) in ops {
        let lang = [lang0, lang1, 0];
        let cc = cstr(cntry);
        // try ASCII set first, fall back to UTF8 if text is non-ASCII
        if text.is_ascii() {
            let txt = cstr(text);
            assert_eq!(
                unsafe {
                    c::cmsMLUsetASCII(
                        c_mlu,
                        lang.as_ptr(),
                        cc.as_ptr() as *const u8,
                        txt.as_ptr() as *const u8,
                    )
                },
                mlu_set_ascii(&mut r_mlu, &lang, cc.as_bytes(), txt.as_bytes()) as i32
            );
        } else {
            let txt = CString::new(text).unwrap();
            assert_eq!(
                unsafe {
                    c::cmsMLUsetUTF8(
                        c_mlu,
                        lang.as_ptr(),
                        cc.as_ptr() as *const u8,
                        txt.as_ptr() as *const u8,
                    )
                },
                mlu_set_utf8(&mut r_mlu, &lang, cc.as_bytes(), txt.as_bytes()) as i32
            );
        }
    }
    (c_mlu, r_mlu)
}

#[test]
fn mlu_set_get_all_encodings() {
    let ops: Vec<(u8, u8, &str, &str)> = vec![
        (b'e', b'n', "US", "Hello, world"),
        (b'e', b'n', "GB", "Colourful description"),
        (b'd', b'e', "DE", "Grüße aus München"),
        (b'j', b'a', "JP", "システム"),
        (b'f', b'r', "FR", "éèê à accénts"),
        (b'e', b's', "ES", "español ñ"),
        (b'z', b'h', "CN", ""),
        (b'e', b'n', "AU", ""), // empty string → forced \0
    ];
    let (c_mlu, r_mlu) = build_pair(&ops);

    unsafe {
        assert_eq!(
            c::cmsMLUtranslationsCount(c_mlu),
            mlu_translations_count(&r_mlu)
        );
    }

    // Query every lang+country, plus misses (language fallback, first-entry fallback)
    let queries: Vec<([u8; 3], &str)> = ops
        .iter()
        .map(|&(l0, l1, c2, _)| ([l0, l1, 0], c2))
        .chain(vec![
            ([b'e', b'n', 0], "NO"), // lang match, country miss → first lang match
            ([b'x', b'x', 0], "XX"), // total miss → first entry
            ([b'd', b'e', 0], "US"),
        ])
        .collect::<Vec<_>>();

    for (lang, cntry) in &queries {
        let cc = cstr(*cntry);
        for get in ["ascii", "utf8", "wide"] {
            // First: size query (NULL buffer)
            let (c_len, r_len) = unsafe {
                match get {
                    "ascii" => (
                        c::cmsMLUgetASCII(
                            c_mlu,
                            lang.as_ptr(),
                            cc.as_ptr() as *const u8,
                            ptr::null_mut(),
                            0,
                        ),
                        mlu_get_ascii(&r_mlu, lang, cc.as_bytes(), None).0,
                    ),
                    "utf8" => (
                        c::cmsMLUgetUTF8(
                            c_mlu,
                            lang.as_ptr(),
                            cc.as_ptr() as *const u8,
                            ptr::null_mut(),
                            0,
                        ),
                        mlu_get_utf8(&r_mlu, lang, cc.as_bytes(), None).0,
                    ),
                    _ => (
                        c::cmsMLUgetWide(
                            c_mlu,
                            lang.as_ptr(),
                            cc.as_ptr() as *const u8,
                            ptr::null_mut(),
                            0,
                        ),
                        mlu_get_wide(&r_mlu, lang, cc.as_bytes(), None).0,
                    ),
                }
            };
            assert_eq!(r_len, c_len, "{get} len {lang:?}/{cntry}");

            // Second: full buffer
            let mut cbuf = [0u8; 1024];
            let mut rbuf = [0u8; 1024];
            let (c_ret, r_ret) = unsafe {
                match get {
                    "ascii" => {
                        let cr = c::cmsMLUgetASCII(
                            c_mlu,
                            lang.as_ptr(),
                            cc.as_ptr() as *const u8,
                            cbuf.as_mut_ptr(),
                            1024,
                        );
                        let (rr, b) = mlu_get_ascii(&r_mlu, lang, cc.as_bytes(), Some(1024));
                        rbuf[..b.len()].copy_from_slice(&b);
                        (cr, rr)
                    }
                    "utf8" => {
                        let cr = c::cmsMLUgetUTF8(
                            c_mlu,
                            lang.as_ptr(),
                            cc.as_ptr() as *const u8,
                            cbuf.as_mut_ptr(),
                            1024,
                        );
                        let (rr, b) = mlu_get_utf8(&r_mlu, lang, cc.as_bytes(), Some(1024));
                        rbuf[..b.len()].copy_from_slice(&b);
                        (cr, rr)
                    }
                    _ => {
                        let mut cwbuf = [0i32; 256];
                        let cr = c::cmsMLUgetWide(
                            c_mlu,
                            lang.as_ptr(),
                            cc.as_ptr() as *const u8,
                            cwbuf.as_mut_ptr(),
                            256 * 4,
                        );
                        let (rr, w) = mlu_get_wide(&r_mlu, lang, cc.as_bytes(), Some(256 * 4));
                        // compare wide bytes
                        for (i, &wv) in w.iter().enumerate() {
                            assert_eq!(wv, cwbuf[i], "{get} wchar[{i}] {lang:?}/{cntry}");
                        }
                        (cr, rr)
                    }
                }
            };
            assert_eq!(r_ret, c_ret, "{get} ret {lang:?}/{cntry}");
            if get != "wide" {
                assert_eq!(
                    &rbuf[..r_ret as usize],
                    &cbuf[..c_ret as usize],
                    "{get} bytes {lang:?}/{cntry}"
                );
            }
        }

        // getTranslation
        let mut cl = [0u8; 3];
        let mut ctry = [0u8; 3];
        let c_ok = unsafe {
            c::cmsMLUgetTranslation(
                c_mlu,
                lang.as_ptr(),
                cc.as_ptr() as *const u8,
                cl.as_mut_ptr(),
                ctry.as_mut_ptr(),
            )
        };
        let r_ok = mlu_get_translation(&r_mlu, lang, cc.as_bytes());
        assert_eq!(r_ok.is_some() as i32, c_ok, "{lang:?}/{cntry}");
        if let Some((rl, rc)) = r_ok {
            assert_eq!(rl, cl);
            assert_eq!(rc, ctry);
        }
    }

    // translationsCodes
    for i in 0..mlu_translations_count(&r_mlu) {
        let mut cl = [0u8; 3];
        let mut ctry = [0u8; 3];
        unsafe { c::cmsMLUtranslationsCodes(c_mlu, i, cl.as_mut_ptr(), ctry.as_mut_ptr()) };
        let (rl, rc) = mlu_translations_codes(&r_mlu, i).unwrap();
        assert_eq!(rl, cl, "lang idx {i}");
        assert_eq!(rc, ctry, "cntry idx {i}");
    }

    // dup → identical queries
    let c_dup = unsafe { c::cmsMLUdup(c_mlu) };
    let r_dup = mlu_dup(&r_mlu).unwrap();
    let lang = [b'e', b'n', 0];
    let cc = cstr("US");
    let mut cbuf = [0u8; 256];
    unsafe {
        c::cmsMLUgetUTF8(
            c_dup,
            lang.as_ptr(),
            cc.as_ptr() as *const u8,
            cbuf.as_mut_ptr(),
            256,
        )
    };
    let (_, rb) = mlu_get_utf8(&r_dup, &lang, cc.as_bytes(), Some(256));
    assert_eq!(rb, &cbuf[..rb.len()]);
    unsafe {
        c::cmsMLUfree(c_mlu);
        c::cmsMLUfree(c_dup);
    }
}

#[test]
fn named_color_list() {
    unsafe {
        let prefix = cstr("PANTONE");
        let suffix = cstr("coated");
        let c_list = c::cmsAllocNamedColorList(
            ptr::null_mut(),
            4,
            4,
            prefix.as_ptr() as *const u8,
            suffix.as_ptr() as *const u8,
        );
        assert!(!c_list.is_null());
        let mut r_list = alloc_named_color_list(None, 4, 4, b"PANTONE", b"coated").unwrap();

        let names = ["Red 032 C", "Blue 072 C", "Yellow C", "Black C", "Green C"];
        let mut s = 1u64;
        for name in names {
            let pcs: [u16; 3] = std::array::from_fn(|_| {
                s = s
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                (s >> 33) as u16
            });
            let mut cols = [0u16; 16];
            for i in 0..4 {
                cols[i] = (s >> (i * 7)) as u16;
            }
            let nm = cstr(name);
            assert_eq!(
                c::cmsAppendNamedColor(
                    c_list,
                    nm.as_ptr() as *const u8,
                    pcs.as_ptr(),
                    cols.as_ptr()
                ),
                append_named_color(&mut r_list, Some(nm.as_bytes()), Some(&pcs), Some(&cols))
                    as i32,
            );
        }
        assert_eq!(c::cmsNamedColorCount(c_list), named_color_count(&r_list));

        // info parity
        for i in 0..5 {
            let mut cn = [0u8; 256];
            let mut cp = [0u8; 33];
            let mut cs = [0u8; 33];
            let mut cpcs = [0u16; 3];
            let mut ccol = [0u16; 16];
            assert_eq!(
                c::cmsNamedColorInfo(
                    c_list,
                    i,
                    cn.as_mut_ptr(),
                    cp.as_mut_ptr(),
                    cs.as_mut_ptr(),
                    cpcs.as_mut_ptr(),
                    ccol.as_mut_ptr()
                ),
                1
            );
            let (rn, rp, rs, rpcs, rcol) = named_color_info(&r_list, i).unwrap();
            assert_eq!(rn, cn);
            assert_eq!(&rp, &cp);
            assert_eq!(&rs, &cs);
            assert_eq!(&rpcs, &cpcs);
            assert_eq!(&rcol, &ccol);
        }

        // index parity
        for &q in &["red 032 c", "BLUE 072 C", "nonexistent", ""] {
            let qn = cstr(q);
            assert_eq!(
                c::cmsNamedColorIndex(c_list, qn.as_ptr() as *const u8),
                named_color_index(&r_list, qn.as_bytes()),
                "index {q}"
            );
        }

        // dup parity
        let c_dup = c::cmsDupNamedColorList(c_list);
        let r_dup = dup_named_color_list(&r_list).unwrap();
        assert_eq!(c::cmsNamedColorCount(c_dup), named_color_count(&r_dup));

        // stage eval parity — named color PCS lookup
        let c_stage = c::_cmsStageAllocNamedColor(c_list, 1);
        let r_stage = stage_alloc_named_color(&r_list, true).unwrap();
        let c_lut = c::cmsPipelineAlloc(ptr::null_mut(), 1, 3);
        c::cmsPipelineInsertStage(c_lut, 1, c_stage);
        let mut r_lut = lcms::lut::pipeline_alloc(None, 1, 3).unwrap();
        assert!(r_lut.insert_stage(lcms::lut::StageLoc::AtEnd, r_stage));
        for i in 0..8u16 {
            let v = (i * 0x2000) as f32 / 65535.0;
            let mut co = [0.0f32; 3];
            let mut ro = [0.0f32; 3];
            c::cmsPipelineEvalFloat(&v, co.as_mut_ptr(), c_lut);
            r_lut.eval_float(&[v], &mut ro);
            assert_eq!(ro.map(f32::to_bits), co.map(f32::to_bits), "idx input {v}");
        }

        c::cmsPipelineFree(c_lut);
        c::cmsFreeNamedColorList(c_list);
        c::cmsFreeNamedColorList(c_dup);
    }
}

#[test]
fn dict_and_seq() {
    unsafe {
        let c_dict = c::cmsDictAlloc(ptr::null_mut());
        let mut r_dict = dict_alloc(None);

        let name1 = [0x64i32, 0x69, 0x73, 0x70, 0x6c, 0x61, 0x79, 0]; // "display"
        let val1 = [0x76i32, 0x61, 0x6c, 0x31, 0];
        let name2 = [0x6ei32, 0x61, 0x6d, 0x65, 0x32, 0];
        let val2 = [0x76i32, 0x61, 0x6c, 0x32, 0];
        c::cmsDictAddEntry(
            c_dict,
            name1.as_ptr(),
            val1.as_ptr(),
            ptr::null_mut(),
            ptr::null_mut(),
        );
        dict_add_entry(&mut r_dict, &name1, Some(&val1), None, None);
        c::cmsDictAddEntry(
            c_dict,
            name2.as_ptr(),
            val2.as_ptr(),
            ptr::null_mut(),
            ptr::null_mut(),
        );
        dict_add_entry(&mut r_dict, &name2, Some(&val2), None, None);

        assert_eq!(r_dict.entries.len(), 2);
        assert_eq!(r_dict.entries[0].name, name2); // prepend order

        let c_dup = c::cmsDictDup(c_dict);
        let r_dup = dict_dup(&r_dict);
        assert_eq!(r_dup.entries.len(), 2);
        // C DictDup reverses (prepending while walking head→tail)
        assert_eq!(r_dup.entries[0].name, name1);

        c::cmsDictFree(c_dict);
        c::cmsDictFree(c_dup);

        let c_seq = c::cmsAllocProfileSequenceDescription(ptr::null_mut(), 3);
        let r_seq = alloc_profile_seq_desc(None, 3).unwrap();
        assert!(!c_seq.is_null());
        assert_eq!(r_seq.seq.len(), 3);
        assert!(alloc_profile_seq_desc(None, 0).is_none());
        assert!(alloc_profile_seq_desc(None, 256).is_none());
        assert!(c::cmsAllocProfileSequenceDescription(ptr::null_mut(), 0).is_null());
        assert!(c::cmsAllocProfileSequenceDescription(ptr::null_mut(), 256).is_null());
        c::cmsFreeProfileSequenceDescription(c_seq);
    }
}
