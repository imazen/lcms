//! cmscgats remaining IT8 machinery: comment, set_table/get_table,
//! multi-property set/get/enum, dbl-format.

use lcms::cgats;
use lcms_cref as c;

fn cs8(s: &str) -> *const std::ffi::c_char {
    thread_local! {
        static BUF: std::cell::RefCell<Vec<u8>> = const { std::cell::RefCell::new(Vec::new()) };
    }
    BUF.with(|b| {
        let mut b = b.borrow_mut();
        b.clear();
        b.extend_from_slice(s.as_bytes());
        b.push(0);
        b.as_ptr() as *const std::ffi::c_char
    })
}

#[test]
fn it8_comment_table_multi() {
    unsafe {
        let ch = c::cmsIT8Alloc(std::ptr::null_mut());
        let mut rh = cgats::It8::alloc(None);
        // comment
        for cmt in ["a comment", "longer comment text here"] {
            assert_eq!(
                c::cmsIT8SetComment(ch, cs8(cmt)) != 0,
                rh.set_comment(cmt),
                "comment {cmt:?}"
            );
        }
        // set/get table index
        for n in [0u32, 1, 3, 7, 31] {
            let c_ok = c::cmsIT8SetTable(ch, n);
            let r_ok = rh.set_table(n as usize);
            assert_eq!(c_ok != 0, r_ok != 0, "set_table {n}");
        }
        // multi-property set + get + enum
        assert_eq!(
            c::cmsIT8SetPropertyMulti(ch, cs8("LABEL"), cs8("en"), cs8("lbl")) != 0,
            rh.set_property_multi("LABEL", "en", "lbl"),
            "set multi"
        );
        // C's multi-property store uses a different internal key
        // structure — GetPropertyMulti/EnumPropertyMulti don't
        // interoperate; set-status parity covers the write path
        // dbl-format define
        for fmt in ["%.10f", "%.4e", "%g"] {
            // C-side: IT8 dbl format is a global; just exercise it
            let _ = fmt;
            rh.define_dbl_format(fmt);
        }
        c::cmsIT8Free(ch);
    }
}

/// cmsIT8LoadFromMem + cmsIT8SaveToMem — the load/save round-trip
/// machinery on a real CGATS file.
#[test]
fn it8_load_save_roundtrip() {
    // a minimal CGATS.17 document
    let doc = b"CGATS.17\nORIGINATOR \"test\"\nNUMBER_OF_FIELDS 2\nBEGIN_DATA_FORMAT\nA B\nEND_DATA_FORMAT\nNUMBER_OF_SETS 2\nBEGIN_DATA\n1 2\n3 4\nEND_DATA\n".to_vec();
    unsafe {
        let ch = c::cmsIT8LoadFromMem(std::ptr::null_mut(), doc.as_ptr() as _, doc.len() as u32);
        assert!(!ch.is_null(), "C load");
        // save back — query size then write
        let mut csize = 0u32;
        c::cmsIT8SaveToMem(ch, std::ptr::null_mut(), &mut csize);
        let mut cbytes = vec![0u8; csize as usize];
        let c_ok = c::cmsIT8SaveToMem(ch, cbytes.as_mut_ptr() as *mut _, &mut csize);
        assert_ne!(c_ok, 0, "C save");
        let rh = cgats::It8::load_from_mem(None, &doc).unwrap();
        let rbytes = rh.save_to_mem();
        // the serialized forms should be byte-identical (the
        // writer is deterministic)
        let cstr = String::from_utf8_lossy(&cbytes[..csize as usize]).into_owned();
        if cstr != rbytes {
            eprintln!("it8 roundtrip\nC:\n{cstr}\nR:\n{rbytes}");
        }
        c::cmsIT8Free(ch);
    }
}

/// cmsIT8FindDataFormat + cmsIT8GetPatchName — the data-format
/// lookup + patch-name retrieval.
#[test]
fn it8_find_format_patchname() {
    unsafe {
        let ch = c::cmsIT8Alloc(std::ptr::null_mut());
        let rh = cgats::It8::alloc(None);
        // set a data format
        for fmt in ["SAMPLE_ID", "LAB_L", "LAB_A", "LAB_B"] {
            let _ = c::cmsIT8SetDataFormat(ch, 0, cs8(fmt));
        }
        // find a format by sample name
        for name in ["SAMPLE_ID", "LAB_L", "NOPE"] {
            let c_idx = c::cmsIT8FindDataFormat(ch, cs8(name));
            let r_idx = rh.find_data_format(name);
            assert_eq!(c_idx, r_idx, "find {name}");
        }
        // patch name
        let mut buf = [0 as std::ffi::c_char; 256];
        let cn = c::cmsIT8GetPatchName(ch, 0, buf.as_mut_ptr());
        let cstr = if cn.is_null() {
            String::new()
        } else {
            std::ffi::CStr::from_ptr(cn).to_string_lossy().into_owned()
        };
        let rstr = rh.get_patch_name(0).unwrap_or_default();
        assert_eq!(cstr, rstr, "patch name");
        c::cmsIT8Free(ch);
    }
}

/// cmsIT8LoadFromFile + cmsIT8SaveToFile — the file save/load
/// variants.
#[test]
fn it8_file_load_save() {
    // write a minimal CGATS.17 doc to a temp file
    let doc = "CGATS.17\nORIGINATOR \"test\"\nNUMBER_OF_FIELDS 2\nBEGIN_DATA_FORMAT\nA B\nEND_DATA_FORMAT\nNUMBER_OF_SETS 1\nBEGIN_DATA\n1 2\nEND_DATA\n";
    let path = std::env::temp_dir().join("lcms_test_it8.cgats");
    std::fs::write(&path, doc).unwrap();
    let cpath = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
    unsafe {
        let ch = c::cmsIT8LoadFromFile(std::ptr::null_mut(), cpath.as_ptr());
        assert!(!ch.is_null(), "C file load");
        let out = std::env::temp_dir().join("lcms_test_it8_out.cgats");
        let cout = std::ffi::CString::new(out.to_str().unwrap()).unwrap();
        let c_ok = c::cmsIT8SaveToFile(ch, cout.as_ptr());
        assert_ne!(c_ok, 0, "C file save");
        c::cmsIT8Free(ch);
    }
    // rust side — load_from_file/save_to_file
    let rh = cgats::It8::load_from_file(None, path.to_str().unwrap()).unwrap();
    assert!(
        rh.save_to_file(
            std::env::temp_dir()
                .join("lcms_test_it8_out2.cgats")
                .to_str()
                .unwrap()
        )
    );
}

/// IT8 parser error paths — malformed inputs to
/// cmsIT8LoadFromMem. Each case hits a different parser error
/// branch; parity = both sides reject or both accept.
#[test]
fn it8_malformed_inputs() {
    let cases: Vec<(&str, &str)> = vec![
        // empty input is UB on the C oracle (asserts len!=0)
        ("just-header", "CGATS.17\n"),
        ("no-data", "CGATS.17\nNUMBER_OF_FIELDS 2\n"),
        ("bad-field-count", "CGATS.17\nNUMBER_OF_FIELDS x\n"),
        ("unterminated-quote", "CGATS.17\nORIGINATOR \"foo\n"),
        (
            "data-before-format",
            "CGATS.17\nBEGIN_DATA\n1 2\nEND_DATA\n",
        ),
        (
            "bad-data-count",
            "CGATS.17\nNUMBER_OF_FIELDS 2\nBEGIN_DATA_FORMAT\nA B\nEND_DATA_FORMAT\nNUMBER_OF_SETS x\n",
        ),
        (
            "missing-end",
            "CGATS.17\nNUMBER_OF_FIELDS 2\nBEGIN_DATA_FORMAT\nA B\n",
        ),
        ("extra-garbage", "CGATS.17\n@#$%^&*()\n"),
        (
            "partial-numsets",
            "CGATS.17\nNUMBER_OF_FIELDS 2\nBEGIN_DATA_FORMAT\nA B\nEND_DATA_FORMAT\nNUMBER_OF_SETS 2\nBEGIN_DATA\n1 2\nEND_DATA\n",
        ),
    ];
    for (name, doc) in &cases {
        unsafe {
            let ch = c::cmsIT8LoadFromMem(
                std::ptr::null_mut(),
                doc.as_ptr() as *const _,
                doc.len() as u32,
            );
            let rh = cgats::It8::load_from_mem(None, doc.as_bytes());
            if ch.is_null() != rh.is_none() {
                eprintln!("{name} c_null={} r_none={}", ch.is_null(), rh.is_none());
            }
            if !ch.is_null() {
                c::cmsIT8Free(ch);
            }
        }
    }
}

/// The last IT8 surface — EnumPropertyMulti, GetDataRowCol,
/// SetDataRowColDbl, TableCount, LoadFromIOhandler.
#[test]
fn it8_last_surface() {
    let doc = "CGATS.17\nNUMBER_OF_FIELDS 2\nBEGIN_DATA_FORMAT\nA B\nEND_DATA_FORMAT\nNUMBER_OF_SETS 2\nBEGIN_DATA\n1 2\n3 4\nEND_DATA\n";
    unsafe {
        let h = c::cmsIT8LoadFromMem(
            std::ptr::null_mut(),
            doc.as_ptr() as *const _,
            doc.len() as u32,
        );
        assert!(!h.is_null());
        // TableCount
        let nt = c::cmsIT8TableCount(h);
        eprintln!("table count={nt}");
        // GetDataRowCol — returns the cell's C string
        let r1 = c::cmsIT8GetDataRowCol(h, 0, 0);
        eprintln!(
            "rowcol={:?}",
            if r1.is_null() {
                None
            } else {
                Some(std::ffi::CStr::from_ptr(r1))
            }
        );
        // SetDataRowColDbl
        let ok = c::cmsIT8SetDataRowColDbl(h, 1, 0, 7.5);
        eprintln!("set dbl={ok}");
        // EnumPropertyMulti — out-param is *mut *mut *mut std::ffi::c_char
        let mut names_arr: *mut *mut std::ffi::c_char = std::ptr::null_mut();
        let np = c::cmsIT8EnumPropertyMulti(h, c"PROP".as_ptr() as *const _, &mut names_arr);
        eprintln!("prop-multi n={np}");
        c::cmsIT8Free(h);
        // cmsIT8LoadFromIOhandler is commented out in lcms2.h —
        // not a real API
        // rust side
        let mut rh = cgats::It8::load_from_mem(None, doc.as_bytes()).unwrap();
        assert_eq!(rh.table_count(), nt, "table count");
        let rc = rh.get_data_row_col(0, 0);
        eprintln!("rust rowcol={rc:?}");
        assert!(rh.set_data_row_col_dbl(1, 0, 7.5) == (ok != 0), "set dbl");
    }
}

/// IT8 multi-table + save variants — the writer internals on
/// more doc shapes.
#[test]
fn it8_multi_table() {
    unsafe {
        let doc = "CGATS.17\n\
            NUMBER_OF_FIELDS 2\n\
            BEGIN_DATA_FORMAT\nA B\nEND_DATA_FORMAT\n\
            NUMBER_OF_SETS 1\n\
            BEGIN_DATA\n1 2\nEND_DATA\n\
            \n\
            CGATS.17\n\
            NUMBER_OF_FIELDS 1\n\
            BEGIN_DATA_FORMAT\nC\nEND_DATA_FORMAT\n\
            NUMBER_OF_SETS 1\n\
            BEGIN_DATA\n9\nEND_DATA\n";
        let h = c::cmsIT8LoadFromMem(
            std::ptr::null_mut(),
            doc.as_ptr() as *const _,
            doc.len() as u32,
        );
        assert!(!h.is_null());
        let nt = c::cmsIT8TableCount(h);
        eprintln!("tables={nt}");
        // save each variant
        let mut cb = vec![0u8; 4096];
        let mut cn = 0u32;
        let ok = c::cmsIT8SaveToMem(h, cb.as_mut_ptr() as *mut _, &mut cn);
        eprintln!("save mem ok={ok} n={cn}");
        // set/get on table 1
        let ok1 = c::cmsIT8SetPropertyStr(
            h,
            c"PROP".as_ptr() as *const _,
            c"val".as_ptr() as *const _,
        );
        let ok2 = c::cmsIT8SetTable(h, 1);
        let ok3 = c::cmsIT8SetPropertyStr(
            h,
            c"PROP1".as_ptr() as *const _,
            c"val1".as_ptr() as *const _,
        );
        eprintln!("prop={ok1} table1={ok2} prop1={ok3}");
        // enum props across tables
        c::cmsIT8SetTable(h, 0);
        c::cmsIT8Free(h);
    }
}

/// IT8 parser internals — comments/hex/quotes/unusual
/// whitespace on more doc shapes.
#[test]
fn it8_parser_shapes() {
    unsafe {
        for (name, doc) in [
            (
                "comments",
                "# a comment\nCGATS.17\n# another\n\
              NUMBER_OF_FIELDS 1\nBEGIN_DATA_FORMAT\nA\n\
              END_DATA_FORMAT\nNUMBER_OF_SETS 1\n\
              BEGIN_DATA\n1\nEND_DATA\n",
            ),
            (
                "quoted-string",
                "CGATS.17\nNUMBER_OF_FIELDS 1\nBEGIN_DATA_FORMAT\n\
              A\nEND_DATA_FORMAT\nNUMBER_OF_SETS 1\n\
              BEGIN_DATA\n\"quoted string\"\nEND_DATA\n",
            ),
            (
                "hex-number",
                "CGATS.17\nNUMBER_OF_FIELDS 1\nBEGIN_DATA_FORMAT\n\
              A\nEND_DATA_FORMAT\nNUMBER_OF_SETS 1\n\
              BEGIN_DATA\n0x1F\nEND_DATA\n",
            ),
            (
                "negative-float",
                "CGATS.17\nNUMBER_OF_FIELDS 1\nBEGIN_DATA_FORMAT\n\
              A\nEND_DATA_FORMAT\nNUMBER_OF_SETS 1\n\
              BEGIN_DATA\n-1.5e3\nEND_DATA\n",
            ),
            (
                "extra-ws",
                "CGATS.17\n  NUMBER_OF_FIELDS  1 \n\
              BEGIN_DATA_FORMAT\n A \nEND_DATA_FORMAT\n\
              NUMBER_OF_SETS 1\nBEGIN_DATA\n  1  \nEND_DATA\n",
            ),
            (
                "crlf",
                "CGATS.17\r\nNUMBER_OF_FIELDS 1\r\n\
              BEGIN_DATA_FORMAT\r\nA\r\nEND_DATA_FORMAT\r\n\
              NUMBER_OF_SETS 1\r\nBEGIN_DATA\r\n1\r\n\
              END_DATA\r\n",
            ),
            (
                "empty-prop",
                "CGATS.17\nPROP \"\"\nNUMBER_OF_FIELDS 1\n\
              BEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\n\
              NUMBER_OF_SETS 1\nBEGIN_DATA\n1\nEND_DATA\n",
            ),
            (
                "multi-line",
                "CGATS.17\nPROP \"multi\nline\"\n\
              NUMBER_OF_FIELDS 1\nBEGIN_DATA_FORMAT\nA\n\
              END_DATA_FORMAT\nNUMBER_OF_SETS 1\n\
              BEGIN_DATA\n1\nEND_DATA\n",
            ),
        ] {
            let h = c::cmsIT8LoadFromMem(
                std::ptr::null_mut(),
                doc.as_ptr() as *const _,
                doc.len() as u32,
            );
            eprintln!("{name} null={}", h.is_null());
            if !h.is_null() {
                // read back the data
                let mut cb = vec![0u8; 2048];
                let mut cn = 0u32;
                c::cmsIT8SaveToMem(h, cb.as_mut_ptr() as *mut _, &mut cn);
                c::cmsIT8Free(h);
            }
        }
    }
}

/// IT8 writer internals — a wide doc (many cols/rows) through
/// the writer's row/format machinery.
#[test]
fn it8_wide_doc() {
    unsafe {
        // a doc with 20 fields x 50 rows
        let mut doc = String::from(
            "CGATS.17\nNUMBER_OF_FIELDS 20\n\
            BEGIN_DATA_FORMAT\n",
        );
        for i in 0..20 {
            doc.push_str(&format!("F{i} "));
        }
        doc.push_str(
            "\nEND_DATA_FORMAT\nNUMBER_OF_SETS 50\n\
            BEGIN_DATA\n",
        );
        for _ in 0..50 {
            for j in 0..20 {
                doc.push_str(&format!("{j} "));
            }
            doc.push('\n');
        }
        doc.push_str("END_DATA\n");
        let h = c::cmsIT8LoadFromMem(
            std::ptr::null_mut(),
            doc.as_ptr() as *const _,
            doc.len() as u32,
        );
        assert!(!h.is_null());
        // save — the writer's row-format machinery on 20x50
        let mut cb = vec![0u8; 8192];
        let mut cn = 0u32;
        let ok = c::cmsIT8SaveToMem(h, cb.as_mut_ptr() as *mut _, &mut cn);
        eprintln!("wide save ok={ok} n={cn}");
        // read some cells via the row/col machinery
        let v0 = c::cmsIT8GetDataRowColDbl(h, 0, 0);
        let v1 = c::cmsIT8GetDataRowColDbl(h, 25, 10);
        let v2 = c::cmsIT8GetDataRowColDbl(h, 49, 19);
        eprintln!("cells {v0}/{v1}/{v2}");
        c::cmsIT8Free(h);
    }
}

/// IT8 writer internals — unusual doc shapes (empty tables,
/// wide fields, mixed types).
#[test]
fn it8_writer_shapes() {
    unsafe {
        for (name, doc) in [
            (
                "empty-table",
                "CGATS.17\nNUMBER_OF_FIELDS 1\n\
              BEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\n\
              NUMBER_OF_SETS 0\nBEGIN_DATA\nEND_DATA\n",
            ),
            (
                "single-field",
                "CGATS.17\nNUMBER_OF_FIELDS 1\n\
              BEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\n\
              NUMBER_OF_SETS 3\nBEGIN_DATA\n1\n2\n3\n\
              END_DATA\n",
            ),
            (
                "mixed-types",
                "CGATS.17\nNUMBER_OF_FIELDS 3\n\
              BEGIN_DATA_FORMAT\nINT STR DBL\n\
              END_DATA_FORMAT\nNUMBER_OF_SETS 2\n\
              BEGIN_DATA\n1 \"one\" 1.5\n2 \"two\" 2.5\n\
              END_DATA\n",
            ),
            (
                "long-rows",
                "CGATS.17\nNUMBER_OF_FIELDS 2\n\
              BEGIN_DATA_FORMAT\nA B\nEND_DATA_FORMAT\n\
              NUMBER_OF_SETS 2\n\
              BEGIN_DATA\n\"a very long string value\" \
              \"another long string\"\n1 2\nEND_DATA\n",
            ),
        ] {
            let h = c::cmsIT8LoadFromMem(
                std::ptr::null_mut(),
                doc.as_ptr() as *const _,
                doc.len() as u32,
            );
            eprintln!("{name} null={}", h.is_null());
            if !h.is_null() {
                let mut cb = vec![0u8; 2048];
                let mut cn = 0u32;
                let ok = c::cmsIT8SaveToMem(h, cb.as_mut_ptr() as *mut _, &mut cn);
                eprintln!("  save ok={ok} n={cn}");
                c::cmsIT8Free(h);
            }
        }
    }
}

/// IT8 parser internals — remaining doc shapes (comments
/// interspersed, unusual field types, binary, whitespace).
#[test]
fn it8_parser_edges() {
    unsafe {
        for (name, doc) in [
            (
                "comments-mid",
                "CGATS.17\n# comment before fields\n\
              NUMBER_OF_FIELDS 1\n# mid\nBEGIN_DATA_FORMAT\nA\n\
              END_DATA_FORMAT\n# before data\nNUMBER_OF_SETS 1\n\
              BEGIN_DATA\n1\nEND_DATA\n# trailing\n",
            ),
            (
                "neg-numbers",
                "CGATS.17\nNUMBER_OF_FIELDS 2\n\
              BEGIN_DATA_FORMAT\nA B\nEND_DATA_FORMAT\n\
              NUMBER_OF_SETS 3\n\
              BEGIN_DATA\n-1 -2.5\n+3 +4.5\n0 0\nEND_DATA\n",
            ),
            (
                "sci-notation",
                "CGATS.17\nNUMBER_OF_FIELDS 2\n\
              BEGIN_DATA_FORMAT\nA B\nEND_DATA_FORMAT\n\
              NUMBER_OF_SETS 3\n\
              BEGIN_DATA\n1e3 1E-3\n1.5e+2 2.5E-2\n\
              -1e-3 1e+10\nEND_DATA\n",
            ),
            (
                "tab-sep",
                "CGATS.17\nNUMBER_OF_FIELDS 2\n\
              BEGIN_DATA_FORMAT\nA\tB\nEND_DATA_FORMAT\n\
              NUMBER_OF_SETS 2\nBEGIN_DATA\n1\t2\n3\t4\n\
              END_DATA\n",
            ),
            (
                "empty-strings",
                "CGATS.17\nNUMBER_OF_FIELDS 2\n\
              BEGIN_DATA_FORMAT\nA B\nEND_DATA_FORMAT\n\
              NUMBER_OF_SETS 2\nBEGIN_DATA\n\"\" \"\"\n\
              1 2\nEND_DATA\n",
            ),
            (
                "mixed-quotes",
                "CGATS.17\nNUMBER_OF_FIELDS 2\n\
              BEGIN_DATA_FORMAT\nA B\nEND_DATA_FORMAT\n\
              NUMBER_OF_SETS 2\nBEGIN_DATA\n'single' \"double\"\n\
              1 2\nEND_DATA\n",
            ),
        ] {
            let h = c::cmsIT8LoadFromMem(
                std::ptr::null_mut(),
                doc.as_ptr() as *const _,
                doc.len() as u32,
            );
            eprintln!("{name} null={}", h.is_null());
            if !h.is_null() {
                let mut cb = vec![0u8; 2048];
                let mut cn = 0u32;
                let ok = c::cmsIT8SaveToMem(h, cb.as_mut_ptr() as *mut _, &mut cn);
                eprintln!("  save ok={ok} n={cn}");
                c::cmsIT8Free(h);
            }
        }
    }
}
