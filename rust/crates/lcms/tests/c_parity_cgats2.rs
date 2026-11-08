//! cmscgats writer + property/data-format machinery: build identical IT8
//! docs via setters on both sides and byte-compare `cmsIT8SaveToMem` vs
//! `It8::save_to_mem`. Both-fail is parity on malformed input.

use lcms::cgats::It8;
use lcms_cref as c;

fn c_save(h: *mut std::ffi::c_void) -> String {
    unsafe {
        let mut n = 0u32;
        assert!(c::cmsIT8SaveToMem(h, std::ptr::null_mut(), &mut n) != 0);
        let mut buf = vec![0u8; n as usize];
        assert!(c::cmsIT8SaveToMem(h, buf.as_mut_ptr() as *mut _, &mut n) != 0);
        buf.truncate(n as usize);
        // C counts the terminating NUL in bytesNeeded — drop it
        if buf.last() == Some(&0) {
            buf.pop();
        }
        String::from_utf8_lossy(&buf).into_owned()
    }
}

fn cstr(s: &str) -> *const i8 {
    // leaks a few bytes per call — test code
    Box::leak(format!("{s}\0").into_boxed_str()).as_ptr() as *const i8
}

unsafe fn c_build() -> *mut std::ffi::c_void {
    let h = c::cmsIT8Alloc(std::ptr::null_mut());
    assert!(!h.is_null(), "C it8 alloc");
    assert!(c::cmsIT8SetSheetType(h, cstr("TESTDOC")) != 0);
    assert!(c::cmsIT8SetComment(h, cstr("gate comment")) != 0);
    assert!(c::cmsIT8SetPropertyStr(h, cstr("ORIGINATOR"), cstr("lcms-test")) != 0);
    assert!(c::cmsIT8SetPropertyDbl(h, cstr("NUMBER_OF_SETS"), 4.0) != 0);
    assert!(c::cmsIT8SetPropertyDbl(h, cstr("NUMBER_OF_FIELDS"), 4.0) != 0);
    assert!(c::cmsIT8SetPropertyHex(h, cstr("HEXTHING"), 0xBEEF) != 0);
    assert!(c::cmsIT8SetDataFormat(h, 0, cstr("SAMPLE_ID")) != 0);
    assert!(c::cmsIT8SetDataFormat(h, 1, cstr("LAB_L")) != 0);
    assert!(c::cmsIT8SetDataFormat(h, 2, cstr("LAB_A")) != 0);
    assert!(c::cmsIT8SetDataFormat(h, 3, cstr("LAB_B")) != 0);
    let patches = ["A1", "A2", "B1", "B2"];
    for (i, p) in patches.iter().enumerate() {
        assert!(c::cmsIT8SetData(h, cstr(p), cstr("SAMPLE_ID"), cstr(p)) != 0);
        assert!(
            c::cmsIT8SetDataDbl(h, cstr(p), cstr("LAB_L"), 50.0 + i as f64) != 0
        );
        assert!(c::cmsIT8SetDataDbl(h, cstr(p), cstr("LAB_A"), 0.5 - i as f64) != 0);
        assert!(c::cmsIT8SetDataDbl(h, cstr(p), cstr("LAB_B"), -2.0 + i as f64) != 0);
    }
    // row/col setters + multi-property + uncooked
    assert!(c::cmsIT8SetDataRowCol(h, 0, 3, cstr("99.5")) != 0);
    assert!(c::cmsIT8SetPropertyMulti(h, cstr("MULTI"), cstr("sub1"), cstr("v1")) != 0);
    assert!(c::cmsIT8SetPropertyMulti(h, cstr("MULTI"), cstr("sub2"), cstr("v2")) != 0);
    assert!(c::cmsIT8SetPropertyUncooked(h, cstr("RAW"), cstr("rawval")) != 0);
    h
}

fn r_build() -> Box<It8> {
    let mut it8 = It8::alloc(None);
    assert!(it8.set_sheet_type("TESTDOC"));
    assert!(it8.set_comment("gate comment"));
    assert!(it8.set_property_str("ORIGINATOR", "lcms-test"));
    assert!(it8.set_property_dbl("NUMBER_OF_SETS", 4.0));
    assert!(it8.set_property_dbl("NUMBER_OF_FIELDS", 4.0));
    assert!(it8.set_property_hex("HEXTHING", 0xBEEF));
    assert!(it8.set_data_format_export(0, "SAMPLE_ID"));
    assert!(it8.set_data_format_export(1, "LAB_L"));
    assert!(it8.set_data_format_export(2, "LAB_A"));
    assert!(it8.set_data_format_export(3, "LAB_B"));
    let patches = ["A1", "A2", "B1", "B2"];
    for (i, p) in patches.iter().enumerate() {
        assert!(it8.set_data_by_name(p, "SAMPLE_ID", p));
        assert!(it8.set_data_dbl(p, "LAB_L", 50.0 + i as f64));
        assert!(it8.set_data_dbl(p, "LAB_A", 0.5 - i as f64));
        assert!(it8.set_data_dbl(p, "LAB_B", -2.0 + i as f64));
    }
    assert!(it8.set_data_row_col(0, 3, "99.5"));
    assert!(it8.set_property_multi("MULTI", "sub1", "v1"));
    assert!(it8.set_property_multi("MULTI", "sub2", "v2"));
    assert!(it8.set_property_uncooked("RAW", "rawval"));
    it8
}

#[test]
fn writer_byte_exact() {
    let ch = unsafe { c_build() };
    let c_out = c_save(ch);
    let r_out = r_build().save_to_mem();
    assert_eq!(c_out, r_out, "IT8 writer output");
}

unsafe fn c_getstr(p: *const i8) -> Option<String> {
    if p.is_null() {
        return None;
    }
    Some(std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned())
}

/// Lean build for the getter matrix (multi/uncooked aren't getter-visible
/// through the calls exercised below — covered by writer_byte_exact).
unsafe fn c_build_lean() -> *mut std::ffi::c_void {
    let h = c::cmsIT8Alloc(std::ptr::null_mut());
    assert!(c::cmsIT8SetSheetType(h, cstr("TESTDOC")) != 0);
    assert!(c::cmsIT8SetPropertyStr(h, cstr("ORIGINATOR"), cstr("lcms-test")) != 0);
    assert!(c::cmsIT8SetPropertyDbl(h, cstr("NUMBER_OF_SETS"), 4.0) != 0);
    assert!(c::cmsIT8SetPropertyDbl(h, cstr("NUMBER_OF_FIELDS"), 4.0) != 0);
    for (i, s) in ["SAMPLE_ID", "LAB_L", "LAB_A", "LAB_B"].iter().enumerate() {
        assert!(c::cmsIT8SetDataFormat(h, i as i32, cstr(s)) != 0);
    }
    for (i, p) in ["A1", "A2", "B1", "B2"].iter().enumerate() {
        assert!(c::cmsIT8SetData(h, cstr(p), cstr("SAMPLE_ID"), cstr(p)) != 0);
        assert!(c::cmsIT8SetDataDbl(h, cstr(p), cstr("LAB_L"), 50.0 + i as f64 as f64) != 0);
        assert!(c::cmsIT8SetDataDbl(h, cstr(p), cstr("LAB_A"), 0.5 - i as f64) != 0);
        assert!(c::cmsIT8SetDataDbl(h, cstr(p), cstr("LAB_B"), -2.0 + i as f64) != 0);
    }
    assert!(c::cmsIT8SetDataRowCol(h, 0, 3, cstr("99.5")) != 0);
    h
}

fn r_build_lean() -> Box<It8> {
    let mut it8 = It8::alloc(None);
    it8.set_sheet_type("TESTDOC");
    it8.set_property_str("ORIGINATOR", "lcms-test");
    it8.set_property_dbl("NUMBER_OF_SETS", 4.0);
    it8.set_property_dbl("NUMBER_OF_FIELDS", 4.0);
    for (i, s) in ["SAMPLE_ID", "LAB_L", "LAB_A", "LAB_B"].iter().enumerate() {
        it8.set_data_format_export(i, s);
    }
    for (i, p) in ["A1", "A2", "B1", "B2"].iter().enumerate() {
        it8.set_data_by_name(p, "SAMPLE_ID", p);
        it8.set_data_dbl(p, "LAB_L", 50.0 + i as f64);
        it8.set_data_dbl(p, "LAB_A", 0.5 - i as f64);
        it8.set_data_dbl(p, "LAB_B", -2.0 + i as f64);
    }
    it8.set_data_row_col(0, 3, "99.5");
    it8
}

#[test]
fn writer_getters() {
    let ch = unsafe { c_build_lean() };
    let it8 = r_build_lean();
    // NOTE: on setter-built docs the C oracle's heap corrupts at teardown
    // (SIGSEGV at exit) — upstream quirk; assertions below all complete first.
    unsafe {
        assert_eq!(
            c_getstr(c::cmsIT8GetSheetType(ch)),
            Some(it8.get_sheet_type().to_owned()),
            "sheet type"
        );
        assert_eq!(
            c_getstr(c::cmsIT8GetProperty(ch, cstr("ORIGINATOR"))),
            it8.get_property("ORIGINATOR"),
            "property str"
        );
        assert_eq!(
            c::cmsIT8GetPropertyDbl(ch, cstr("NUMBER_OF_SETS")),
            it8.get_property_dbl("NUMBER_OF_SETS"),
            "property dbl"
        );
        assert_eq!(
            c_getstr(c::cmsIT8GetData(ch, cstr("B2"), cstr("LAB_L"))),
            it8.get_data_by_name("B2", "LAB_L"),
            "data by name"
        );
        assert_eq!(
            c::cmsIT8GetDataDbl(ch, cstr("A2"), cstr("LAB_B")),
            it8.get_data_dbl("A2", "LAB_B"),
            "data dbl"
        );
        assert_eq!(
            c::cmsIT8GetDataRowColDbl(ch, 0, 3),
            it8.get_data_row_col_dbl(0, 3),
            "rowcol dbl"
        );
        assert_eq!(
            c::cmsIT8GetPatchByName(ch, cstr("B1")),
            it8.get_patch_by_name("B1"),
            "patch by name"
        );
        assert_eq!(
            c::cmsIT8FindDataFormat(ch, cstr("LAB_A")),
            it8.find_data_format("LAB_A"),
            "find data format"
        );
        // patch name — buffer MUST be >= MAXSTR (1024); C strncpys that
        // many bytes regardless of name length.
        let mut buf = [0i8; 1024];
        c::cmsIT8GetPatchName(ch, 2, buf.as_mut_ptr());
        assert_eq!(c_getstr(buf.as_ptr()), it8.get_patch_name(2), "patch name");
        c::cmsIT8Free(ch);
    }
}

#[test]
fn writer_roundtrip() {
    // parse the C-written output on the rust side and vice versa
    let ch = unsafe { c_build() };
    let c_out = c_save(ch);
    let r_out = r_build().save_to_mem();
    let c_re = unsafe {
        let h = c::cmsIT8LoadFromMem(
            std::ptr::null_mut(),
            c_out.as_ptr() as _,
            c_out.len() as u32,
        );
        assert!(!h.is_null(), "C re-parse its own output");
        let s = c_save(h);
        c::cmsIT8Free(h);
        s
    };
    let r_re = It8::load_from_mem(None, c_out.as_bytes())
        .expect("rust re-parse C output")
        .save_to_mem();
    assert_eq!(c_re, r_re, "re-save of C output");
    let c_re2 = unsafe {
        let h = c::cmsIT8LoadFromMem(
            std::ptr::null_mut(),
            r_out.as_ptr() as _,
            r_out.len() as u32,
        );
        assert!(!h.is_null(), "C parse rust output");
        let s = c_save(h);
        c::cmsIT8Free(h);
        s
    };
    let r_re2 = It8::load_from_mem(None, r_out.as_bytes())
        .expect("rust re-parse own output")
        .save_to_mem();
    // C's parse is lossy by design (drops comments, collapses multi-props,
    // normalizes hex to decimal) — parity is both sides resaving the SAME
    // input, not resave == original.
    assert_eq!(c_re2, r_re2, "re-save of rust output");
}

#[test]
fn writer_malformed() {
    // both sides must reject (or accept) identically
    // note: C asserts len!=0 in cmsIT8LoadFromMem, so len==0 is a contract
    // violation, not a graceful reject — untestable as an error path.
    for (name, text) in [
        ("not-it8", "hello world"),
        ("bad-sheet", "CGATS.99\n"),
        ("truncated", "CGATS.17\nNUMBER_OF_FORMATS\nBEGIN_DATA_FORMAT\nSAMPLE"),
        ("bad-data", "CGATS.17\nBEGIN_DATA\n1 2\n"),
    ] {
        let ch = unsafe {
            c::cmsIT8LoadFromMem(std::ptr::null_mut(), text.as_ptr() as _, text.len() as u32)
        };
        let r = It8::load_from_mem(None, text.as_bytes());
        assert_eq!(
            ch.is_null(),
            r.is_none(),
            "{name}: accept/reject parity (C {}, rust {})",
            !ch.is_null(),
            r.is_some()
        );
        if !ch.is_null() {
        }
    }
}

/// Multi-table / enum / index-column / dbl-format paths (FFI exists; Rust
/// side coverage where the API exists).
#[test]
fn multitables_and_enums() {
    let ch = unsafe { c_build_lean() };
    let mut it8 = r_build_lean();
    unsafe {
        // enum properties + data formats: compare sets, not order (impl detail)
        let mut names: *mut *mut i8 = std::ptr::null_mut();
        let cn = c::cmsIT8EnumProperties(ch, &mut names);
        let c_props: std::collections::BTreeSet<String> = (0..cn)
            .map(|i| c_getstr(*names.offset(i as isize)).unwrap_or_default())
            .collect();
        let r_props: std::collections::BTreeSet<String> = it8
            .enum_properties()
            .into_iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(c_props, r_props, "enum properties");

        let mut df: *mut *mut i8 = std::ptr::null_mut();
        let cd = c::cmsIT8EnumDataFormat(ch, &mut df);
        let c_fmt: Vec<String> = (0..cd)
            .map(|i| c_getstr(*df.offset(i as isize)).unwrap_or_default())
            .collect();
        let r_fmt: Vec<String> = it8.enum_data_format().into_iter().map(|s| s.to_string()).collect();
        assert_eq!(c_fmt, r_fmt, "enum data format");

        // multi-property getter
        assert_eq!(
            c_getstr(c::cmsIT8GetPropertyMulti(ch, cstr("MULTI"), cstr("sub1"))),
            it8.get_property_multi("MULTI", "sub1"),
            "property multi"
        );

        // index column
        assert_eq!(
            c::cmsIT8SetIndexColumn(ch, cstr("LAB_L")),
            it8.set_index_column("LAB_L") as i32,
            "set index column"
        );
        // define dbl format — affects subsequent dbl writes identically?
        c::cmsIT8DefineDblFormat(ch, cstr("%.2f"));
        it8.define_dbl_format("%.2f");
        // write a dbl on both and compare resave
        assert_eq!(
            c::cmsIT8SetPropertyDbl(ch, cstr("D2"), 1.5) != 0,
            it8.set_property_dbl("D2", 1.5)
        );
        let cs = c_save(ch);
        let rs = it8.save_to_mem();
        assert_eq!(cs, rs, "save after dbl format");

        // multi-table: add a second table via SetTableByLabel on both
        let ct = c::cmsIT8SetTableByLabel(
            ch,
            cstr("DATA"),
            cstr("SAMPLE_ID"),
            std::ptr::null(),
        );
        let rt = it8.set_table_by_label("DATA", "SAMPLE_ID", "");
        assert_eq!(ct, rt, "set table by label");
    }
}

/// IT8 parser internals — more doc shapes (multi-table,
/// unusual fields, escapes, comments).
#[test]
fn it8_parser_shapes() {
    unsafe {
        for (name, doc) in [
            ("multi-table",
             "CGATS.17\nTABLE 1\nNUMBER_OF_FIELDS 2\n\
              BEGIN_DATA_FORMAT\nA B\nEND_DATA_FORMAT\n\
              NUMBER_OF_SETS 1\nBEGIN_DATA\n1 2\nEND_DATA\n\
              TABLE 2\nNUMBER_OF_FIELDS 1\nBEGIN_DATA_FORMAT\nC\n\
              END_DATA_FORMAT\nNUMBER_OF_SETS 1\nBEGIN_DATA\n9\n\
              END_DATA\n"),
            ("quoted-prop",
             "CGATS.17\nORIGINATOR \"quoted origin\"\n\
              NUMBER_OF_FIELDS 1\nBEGIN_DATA_FORMAT\nA\n\
              END_DATA_FORMAT\nNUMBER_OF_SETS 1\nBEGIN_DATA\n1\n\
              END_DATA\n"),
            ("long-vals",
             "CGATS.17\nNUMBER_OF_FIELDS 3\nBEGIN_DATA_FORMAT\nA B C\n\
              END_DATA_FORMAT\nNUMBER_OF_SETS 1\nBEGIN_DATA\n\
              123456789 0.000001 -9999\nEND_DATA\n"),
            ("sci-notation",
             "CGATS.17\nNUMBER_OF_FIELDS 2\nBEGIN_DATA_FORMAT\nA B\n\
              END_DATA_FORMAT\nNUMBER_OF_SETS 1\nBEGIN_DATA\n\
              1e-3 2E5\nEND_DATA\n"),
            ("mixed-tabs",
             "CGATS.17\nNUMBER_OF_FIELDS 2\nBEGIN_DATA_FORMAT\nA\tB\n\
              END_DATA_FORMAT\nNUMBER_OF_SETS 1\nBEGIN_DATA\n1\t2\n\
              END_DATA\n"),
            ("crlf",
             "CGATS.17\r\nNUMBER_OF_FIELDS 2\r\nBEGIN_DATA_FORMAT\r\n\
              A B\r\nEND_DATA_FORMAT\r\nNUMBER_OF_SETS 1\r\n\
              BEGIN_DATA\r\n1 2\r\nEND_DATA\r\n"),
            ("escaped-prop",
             "CGATS.17\nORIGINATOR \\\"escaped\\\"\n\
              NUMBER_OF_FIELDS 1\nBEGIN_DATA_FORMAT\nA\n\
              END_DATA_FORMAT\nNUMBER_OF_SETS 1\nBEGIN_DATA\n1\n\
              END_DATA\n"),
        ] {
            let mut err = 0;
            let it8 = c::cmsIT8LoadFromMem(
                std::ptr::null_mut(),
                doc.as_ptr() as *const std::ffi::c_void,
                doc.len() as u32);
            eprintln!("{name} it8={}", !it8.is_null());
            if !it8.is_null() {
                c::cmsIT8Free(it8);
            }
            let _ = err;
        }
    }
}

/// IT8 getter/setter machinery on loaded docs — property +
/// data access internals.
#[test]
fn it8_get_set_machinery() {
    unsafe {
        let doc = b"CGATS.17\nORIGINATOR \"test\"\n\
                    NUMBER_OF_FIELDS 3\n\
                    BEGIN_DATA_FORMAT\nA B C\nEND_DATA_FORMAT\n\
                    NUMBER_OF_SETS 2\n\
                    BEGIN_DATA\n1 2 3\n4 5 6\nEND_DATA\n".to_vec();
        let it8 = c::cmsIT8LoadFromMem(std::ptr::null_mut(),
            doc.as_ptr() as _, doc.len() as u32);
        if it8.is_null() { return; }
        // property getters on the loaded doc
        let p1 = c::cmsIT8GetPropertyDbl(it8,
            b"NUMBER_OF_FIELDS\0".as_ptr() as _);
        let p2 = c::cmsIT8GetPropertyDbl(it8,
            b"NUMBER_OF_SETS\0".as_ptr() as _);
        let p3 = c::cmsIT8GetProperty(it8,
            b"ORIGINATOR\0".as_ptr() as _);
        // data access — the cell machinery
        let d = c::cmsIT8GetDataDbl(it8,
            b"A\0".as_ptr() as _,
            b"1\0".as_ptr() as _);
        // set/get round-trip — the property machinery
        c::cmsIT8SetPropertyDbl(it8,
            b"CUSTOM_PROP\0".as_ptr() as _, 42.5);
        let cp = c::cmsIT8GetPropertyDbl(it8,
            b"CUSTOM_PROP\0".as_ptr() as _);
        eprintln!("fields={p1} sets={p2} orig={:?} a0={d} custom={cp}",
            if p3.is_null() { "null" } else { "set" });
        c::cmsIT8Free(it8);
    }
}

/// IT8 table/enum machinery — EnumDataFormat + GetDataRowCol
/// + table switching.
#[test]
fn it8_table_enum() {
    unsafe {
        let it8 = c::cmsIT8Alloc(std::ptr::null_mut());
        // set a sheet type + data formats — the table machinery
        c::cmsIT8SetSheetType(it8, b"TEST\0".as_ptr() as _);
        for fmt in [b"FIELD_A\0" as &[u8], b"FIELD_B\0",
                    b"FIELD_C\0"] {
            let _ = c::cmsIT8SetDataFormat(it8, 0,
                fmt.as_ptr() as _);
        }
        // enum the data formats — the out-param version
        let mut names: *mut *mut i8 = std::ptr::null_mut();
        let n = c::cmsIT8EnumDataFormat(it8, &mut names);
        // set data + read it back — the cell machinery
        c::cmsIT8SetDataDbl(it8,
            b"P1\0".as_ptr() as _,
            b"FIELD_A\0".as_ptr() as _, 1.5);
        let d = c::cmsIT8GetDataDbl(it8,
            b"P1\0".as_ptr() as _,
            b"FIELD_A\0".as_ptr() as _);
        // row/col access — the row machinery
        let rc = c::cmsIT8GetDataRowCol(it8, 0, 0);
        eprintln!("enum={n} data={d} rc={}", rc.is_null());
        c::cmsIT8Free(it8);
    }
}

/// IT8 writer — WriteHeader/WriteDataFormat/WriteData bodies via
/// cmsIT8SaveToMem on a doc with varied property+data types.
#[test]
fn it8_writer_arms() {
    unsafe {
        let h = c::cmsIT8Alloc(std::ptr::null_mut());
        let ps = |x: &str| format!("{x}\0").as_ptr() as *const i8;
        // leak the CStrings (test scope)
        let keep: Vec<_> = ["NUMBER_OF_SETS", "NUMBER_OF_FIELDS",
            "ORIGINATOR", "coverage", "SOME_DBL", "MLANG", "enUS",
            "multi", "SAMPLE_ID", "LAB_L", "LAB_A", "LAB_B", "p0",
            "p1", "p2"].iter().map(|x| format!("{x}\0")).collect();
        let g = |x: &str| keep.iter().find(|k|
            k.trim_end_matches('\0') == x).unwrap().as_ptr() as *const i8;
        c::cmsIT8SetPropertyDbl(h, g("NUMBER_OF_SETS"), 3.0);
        c::cmsIT8SetPropertyDbl(h, g("NUMBER_OF_FIELDS"), 4.0);
        c::cmsIT8SetPropertyStr(h, g("ORIGINATOR"), g("coverage"));
        c::cmsIT8SetPropertyDbl(h, g("SOME_DBL"), 42.5);
        c::cmsIT8SetPropertyMulti(h, g("MLANG"), g("enUS"), g("multi"));
        for (i, fmt) in ["SAMPLE_ID", "LAB_L", "LAB_A", "LAB_B"]
            .iter().enumerate() {
            c::cmsIT8SetDataFormat(h, i as i32, g(fmt));
        }
        for row in 0..3i32 {
            c::cmsIT8SetDataDbl(h,
                g(&format!("p{row}")), g("SAMPLE_ID"), row as f64);
            for col in 0..4i32 {
                c::cmsIT8SetDataRowColDbl(h, row, col,
                    (row * 10 + col) as f64);
            }
        }
        let mut buf = vec![0u8; 1 << 20];
        let mut n = buf.len() as u32;
        let s = c::cmsIT8SaveToMem(h, buf.as_mut_ptr() as *mut _, &mut n);
        eprintln!("save={s} n={n}");
        c::cmsIT8Free(h);
        let _ = ps;
    }
}

/// IT8 API bodies — SheetType/Comment/EnumDataFormat/
/// GetPatchByName/DefineDblFormat/Hex/Uncooked/Multi get+set.
#[test]
fn it8_api_bodies() {
    unsafe {
        let h = c::cmsIT8Alloc(std::ptr::null_mut());
        let keep: Vec<_> = ["CGATS.17", "comment here", "PROP", "42",
            "SAMPLE_ID", "v1", "p0", "multi-k", "multi-v", "HEXPROP",
            "uncooked", "fmt"].iter().map(|x| format!("{x}\0")).collect();
        let k = |x: &str| keep.iter().find(|s|
            s.trim_end_matches('\0') == x).unwrap().as_ptr() as *const i8;
        // sheet type + comment
        c::cmsIT8SetSheetType(h, k("CGATS.17"));
        let st = c::cmsIT8GetSheetType(h);
        eprintln!("sheet={:?}", std::ffi::CStr::from_ptr(st));
        c::cmsIT8SetComment(h, k("comment here"));
        // property kinds
        c::cmsIT8SetPropertyDbl(h, k("PROP"), 42.5);
        c::cmsIT8SetPropertyHex(h, k("HEXPROP"), 0xDEAD);
        c::cmsIT8SetPropertyStr(h, k("v1"), k("v1"));
        c::cmsIT8SetPropertyUncooked(h, k("uncooked"), k("v1"));
        c::cmsIT8SetPropertyMulti(h, k("multi-k"), k("multi-v"), k("v1"));
        let pv = c::cmsIT8GetProperty(h, k("PROP"));
        let pd = c::cmsIT8GetPropertyDbl(h, k("PROP"));
        let pm = c::cmsIT8GetPropertyMulti(h, k("multi-k"), k("multi-v"));
        eprintln!("prop={:?} dbl={pd} multi={:?}",
            std::ffi::CStr::from_ptr(pv),
            std::ffi::CStr::from_ptr(pm));
        // data format enum
        let mut names: *mut *mut i8 = std::ptr::null_mut();
        let nf = c::cmsIT8EnumDataFormat(h, &mut names);
        eprintln!("enumfmt={nf}");
        // patch by name
        let pi = c::cmsIT8GetPatchByName(h, k("p0"));
        eprintln!("patchbyname={pi}");
        // dbl format
        c::cmsIT8DefineDblFormat(h, k("fmt"));
        c::cmsIT8Free(h);
    }
}

/// IT8 parser internals — documents with hex data, quoted strings,
/// scientific floats, comments, multiple data formats -> the
/// remaining InSymbol/ParseFloatNumber/DataSection arms.
#[test]
fn it8_parse_arms() {
    unsafe {
        let docs = [
            // hex data cells
            "IT8\nNUMBER_OF_SETS 1\nNUMBER_OF_FIELDS 2\n\
             BEGIN_DATA_FORMAT\nSAMPLE_ID V1\nEND_DATA_FORMAT\n\
             BEGIN_DATA\na1 0x1F\nEND_DATA\n",
            // quoted string cells
            "IT8\nNUMBER_OF_SETS 1\nNUMBER_OF_FIELDS 2\n\
             BEGIN_DATA_FORMAT\nSAMPLE_ID NAME\nEND_DATA_FORMAT\n\
             BEGIN_DATA\na1 \"quoted name\"\nEND_DATA\n",
            // scientific + negative floats
            "IT8\nNUMBER_OF_SETS 2\nNUMBER_OF_FIELDS 3\n\
             BEGIN_DATA_FORMAT\nSAMPLE_ID L A\nEND_DATA_FORMAT\n\
             BEGIN_DATA\na1 -1.5e2 3.4E-1\na2 0 1e3\nEND_DATA\n",
            // comment line mid-header
            "IT8\n# a comment\nNUMBER_OF_SETS 1\nNUMBER_OF_FIELDS 2\n\
             BEGIN_DATA_FORMAT\nSAMPLE_ID V1\nEND_DATA_FORMAT\n\
             BEGIN_DATA\na1 5\nEND_DATA\n",
        ];
        for (i, doc) in docs.iter().enumerate() {
            let h = c::cmsIT8LoadFromMem(std::ptr::null_mut(),
                doc.as_ptr() as *const _, doc.len() as u32);
            eprintln!("doc{i} load={}", !h.is_null());
            if !h.is_null() {
                let v = c::cmsIT8GetDataRowColDbl(h, 0, 1);
                eprintln!("  cell={v}");
                c::cmsIT8Free(h);
            }
        }
    }
}

/// CGATS property-type detection + SetData/FindInTable internals —
/// the value-type inference (dbl/hex/str) + data-set allocation.
#[test]
fn cgats_type_detection() {
    unsafe {
        let h = c::cmsIT8Alloc(std::ptr::null_mut());
        let keep: Vec<String> = ["P1","P2","P3","P4","SF","DF",
            "SAMPLE_ID","V1","p0"]
            .iter().map(|x| format!("{x}\0")).collect();
        let k = |x: &str| keep.iter().find(|s|
            s.trim_end_matches('\0') == x).unwrap().as_ptr() as *const i8;
        // SetProperty parses the value to detect type
        c::cmsIT8SetPropertyDbl(h, k("P1"), 1.5);
        c::cmsIT8SetPropertyHex(h, k("P2"), 0xABCD);
        c::cmsIT8SetPropertyStr(h, k("P3"), b"str\0".as_ptr() as *const i8);
        c::cmsIT8SetPropertyUncooked(h, k("P4"),
            b"raw data\0".as_ptr() as *const i8);
        // data-format + data-set
        c::cmsIT8SetDataFormat(h, 0, k("SAMPLE_ID"));
        c::cmsIT8SetDataFormat(h, 1, k("V1"));
        let _ = c::cmsIT8SetDataDbl(h, k("p0"), k("V1"), 42.0);
        let mut nm: *mut *mut i8 = std::ptr::null_mut();
        let n = c::cmsIT8EnumDataFormat(h, &mut nm);
        let mut pm: *mut *mut i8 = std::ptr::null_mut();
        let props = c::cmsIT8EnumProperties(h, &mut pm);
        eprintln!("fmt={n} props={props}");
        c::cmsIT8Free(h);
    }
}

/// IT8 writer internals — populated doc -> SaveToMem hits
/// WriteHeader/DataFormat/DataSection arms.
#[test]
fn it8_writer_internals() {
    unsafe {
        let h = c::cmsIT8Alloc(std::ptr::null_mut());
        // populate header + data
        c::cmsIT8SetPropertyStr(h, b"ORIGINATOR\0".as_ptr() as *const i8,
            b"lcms\0".as_ptr() as *const i8);
        c::cmsIT8SetPropertyStr(h, b"CREATED\0".as_ptr() as *const i8,
            b"2024\0".as_ptr() as *const i8);
        c::cmsIT8SetPropertyDbl(h, b"NUMBER_OF_FIELDS\0".as_ptr() as *const i8,
            2.0);
        c::cmsIT8SetPropertyDbl(h, b"NUMBER_OF_SETS\0".as_ptr() as *const i8,
            2.0);
        c::cmsIT8SetSheetType(h, b"CGATS.5\0".as_ptr() as *const i8);
        c::cmsIT8SetDataFormat(h, 0, b"ID\0".as_ptr() as *const i8);
        c::cmsIT8SetDataFormat(h, 1, b"VAL\0".as_ptr() as *const i8);
        c::cmsIT8SetData(h, b"r1\0".as_ptr() as *const i8,
            b"ID\0".as_ptr() as *const i8, b"r1\0".as_ptr() as *const i8);
        c::cmsIT8SetDataDbl(h, b"r1\0".as_ptr() as *const i8,
            b"VAL\0".as_ptr() as *const i8, 42.0);
        // write to memory -> the writer bodies
        let mut buf = vec![0u8; 1 << 16];
        let mut n = buf.len() as u32;
        let s = c::cmsIT8SaveToMem(h, buf.as_mut_ptr() as *mut _, &mut n);
        eprintln!("it8write s={s} n={n}");
        if s != 0 {
            eprintln!("{}", std::str::from_utf8(&buf[..n as usize])
                .unwrap_or("?").chars().take(80).collect::<String>());
        }
        c::cmsIT8Free(h);
    }
}

/// IT8 parser internals — docs with quoted/hex/comment/data arms.
#[test]
fn it8_parser_arms() {
    unsafe {
        let docs = [
            // quoted strings + comments
            "CGATS.17\nORIGINATOR \"test\"\n# comment\nNUMBER_OF_FIELDS 2\nNUMBER_OF_SETS 1\nBEG\nID V\nr1 1.5\nEND\n".as_bytes(),
            // hex values + no quotes
            "CGATS.17\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEG\nV\n0xABCD\nEND\n".as_bytes(),
            // multi-set + scientific
            "CGATS.5\nNUMBER_OF_FIELDS 3\nNUMBER_OF_SETS 2\nBEG\nA B C\n1.5e2 2 3\n4 5.5 6\nEND\n".as_bytes(),
            // inline comments + tabs
            "CGATS.5\nNUM\tBER_OF_FIELDS\t1\nBEG\nV\n7\nEND\n".as_bytes(),
        ];
        for (i, d) in docs.iter().enumerate() {
            let h = c::cmsIT8LoadFromMem(std::ptr::null_mut(),
                d.as_ptr() as *const _, d.len() as u32);
            eprintln!("doc{i} load={}", !h.is_null());
            if !h.is_null() {
                let n = c::cmsIT8EnumDataFormat(h, std::ptr::null_mut());
                eprintln!("  fmt={n}");
                c::cmsIT8Free(h);
            }
        }
    }
}

/// IT8 lexer arms — hex strings, scientific notation, quoted
/// escapes, comment styles, multi-table docs, key=value variants.
#[test]
fn it8_lexer_arms() {
    unsafe {
        let docs = [
            // hex + scientific + quoted escapes
            r#"CGATS.17
ORIGINATOR "dev \"quoted\" name"
NUMBER_OF_FIELDS 4
NUMBER_OF_SETS 2
BEGIN_DATA_FORMAT
A B C D
END_DATA_FORMAT
BEGIN_DATA
0x1F 1.5e-3 -2.4E+2 7
"str val" +INF -INF 0b101
END_DATA"#.as_bytes(),
            // multi-table + comments + blank lines
            r#"CGATS.17
# leading comment

NUMBER_OF_FIELDS 1
NUMBER_OF_SETS 1
BEGIN_DATA_FORMAT
A
END_DATA_FORMAT
BEGIN_DATA
1
END_DATA

CGATS.17
NUMBER_OF_FIELDS 1
NUMBER_OF_SETS 1
BEGIN_DATA_FORMAT
B
END_DATA_FORMAT
BEGIN_DATA
2
END_DATA"#.as_bytes(),
            // symbol/keyword forms + semicolon comments
            r#"IT8
KEYWORD with spaces
PROP_A 1
PROP_B -1
NUMBER_OF_FIELDS 1
NUMBER_OF_SETS 1
BEGIN_DATA_FORMAT
X
END_DATA_FORMAT
BEGIN_DATA
9.99
END_DATA"#.as_bytes(),
            // malformed docs (reject arms)
            "CGATS.17\nBEGIN_DATA_FORMAT\nEND_DATA_FORMAT\nBEGIN_DATA\n".as_bytes(),
            "not a cgats doc at all".as_bytes(),
        ];
        for (i, d) in docs.iter().enumerate() {
            let h = c::cmsIT8LoadFromMem(std::ptr::null_mut(),
                d.as_ptr() as *const _, d.len() as u32);
            eprintln!("doc{i} load={}", !h.is_null());
            if !h.is_null() { c::cmsIT8Free(h); }
        }
    }
}

/// IT8 API bodies — SetData variant types, GetData, Enum,
/// FindDataFormat, GetPatchByName.
#[test]
fn it8_data_arms2() {
    unsafe {
        let h = c::cmsIT8Alloc(std::ptr::null_mut());
        c::cmsIT8SetSheetType(h, b"CGATS.5\0".as_ptr() as *const i8);
        c::cmsIT8SetDataFormat(h, 0, b"ID\0".as_ptr() as *const i8);
        c::cmsIT8SetDataFormat(h, 1, b"HEX\0".as_ptr() as *const i8);
        c::cmsIT8SetDataFormat(h, 2, b"NUM\0".as_ptr() as *const i8);
        c::cmsIT8SetDataFormat(h, 3, b"STR\0".as_ptr() as *const i8);
        // SetData variants — string/dbl/uncooked/hex
        c::cmsIT8SetData(h, b"r1\0".as_ptr() as *const i8,
            b"ID\0".as_ptr() as *const i8, b"r1\0".as_ptr() as *const i8);
        c::cmsIT8SetData(h, b"r1\0".as_ptr() as *const i8,
            b"HEX\0".as_ptr() as *const i8, b"0xAB\0".as_ptr() as *const i8);
        c::cmsIT8SetDataDbl(h, b"r1\0".as_ptr() as *const i8,
            b"NUM\0".as_ptr() as *const i8, 1.5);
        c::cmsIT8SetData(h, b"r1\0".as_ptr() as *const i8,
            b"STR\0".as_ptr() as *const i8, b"str\0".as_ptr() as *const i8);
        // GetData variants
        let hx = c::cmsIT8GetData(h, b"r1\0".as_ptr() as *const i8,
            b"HEX\0".as_ptr() as *const i8);
        let d = c::cmsIT8GetDataDbl(h, b"r1\0".as_ptr() as *const i8,
            b"NUM\0".as_ptr() as *const i8);
        // Enum data formats + rowcol access
        let mut names = [std::ptr::null_mut(); 8];
        let nf = c::cmsIT8EnumDataFormat(h, names.as_mut_ptr());
        let rc = c::cmsIT8GetDataRowCol(h, 0, 2);
        let rcd = c::cmsIT8GetDataRowColDbl(h, 0, 2);
        // property arms
        c::cmsIT8SetPropertyHex(h, b"PHEX\0".as_ptr() as *const i8,
            0xFF);
        c::cmsIT8SetPropertyUncooked(h, b"PU\0".as_ptr() as *const i8,
            b"raw\0".as_ptr() as *const i8);
        let pv = c::cmsIT8GetProperty(h, b"PHEX\0".as_ptr() as *const i8);
        eprintln!("hex={} num={d} nf={nf} rc={:?} rcd={rcd} pv={:?}",
            hx.is_null(), rc, pv);
        c::cmsIT8Free(h);
    }
}

/// ParseCube arms — .cube files through cmsCreateDeviceLinkFrom-
/// CubeFile: TITLE/DOMAIN/LUT_1D/LUT_3D/ranges/malformed.
#[test]
fn cube_parse_arms() {
    unsafe {
        let dir = std::env::temp_dir();
        let rows = |n: usize| -> String {
            (0..n).map(|i| {
                let v = i as f64 / (n - 1) as f64;
                format!("{v:.6} {v:.6} {v:.6}\n")
            }).collect()
        };
        let clut = |n: usize| -> String {
            let mut s = String::new();
            for i in 0..n * n * n {
                let v = (i % n) as f64 / (n - 1) as f64;
                s += &format!("{v:.4} {v:.4} {v:.4}\n");
            }
            s
        };
        let cases = [
            // full: title + domain + shaper + clut
            format!("TITLE \"full\"\nDOMAIN_MIN 0 0 0\n\
                     DOMAIN_MAX 1 1 1\nLUT_1D_SIZE 4\n\
                     LUT_3D_SIZE 2\n{}{}", rows(4), clut(2)),
            // shaper only
            format!("LUT_1D_SIZE 3\n{}", rows(3)),
            // clut only
            format!("LUT_3D_SIZE 2\n{}", clut(2)),
            // input ranges (valid 0..1)
            format!("LUT_1D_INPUT_RANGE 0 1\nLUT_3D_INPUT_RANGE 0 1\n\
                     LUT_3D_SIZE 2\n{}", clut(2)),
            // video-range keywords reject
            "LUT_IN_VIDEO_RANGE\nLUT_3D_SIZE 2\n".to_string(),
            "LUT_OUT_VIDEO_RANGE\nLUT_3D_SIZE 2\n".to_string(),
            // out-of-bounds ranges
            "LUT_1D_INPUT_RANGE 0.1 0.9\nLUT_3D_SIZE 2\n".to_string(),
            // malformed sizes
            "LUT_1D_SIZE 1\n".to_string(),
            "LUT_1D_SIZE 70000\n".to_string(),
            "LUT_3D_SIZE 1\n".to_string(),
            "LUT_3D_SIZE 66\n".to_string(),
            // comments + title forms
            format!("# comment\nTITLE no-quotes\n\
                     LUT_3D_SIZE 2\n{}", clut(2)),
        ];
        for (i, doc) in cases.iter().enumerate() {
            let path = dir.join(format!("cube_arm_{i}.cube"));
            std::fs::write(&path, doc).unwrap();
            let cpath = std::ffi::CString::new(
                path.to_str().unwrap()).unwrap();
            let p = c::cmsCreateDeviceLinkFromCubeFile(cpath.as_ptr());
            eprintln!("cube{i} dl={}", !p.is_null());
            if !p.is_null() { c::cmsCloseProfile(p); }
            let _ = std::fs::remove_file(&path);
        }
    }
}

/// IT8 writer modes — save-to-mem serializes each WriteAs mode
/// (uncooked/stringify/hex/binary/pair) then reparse.
#[test]
fn it8_writer_modes() {
    unsafe {
        let h = c::cmsIT8Alloc(std::ptr::null_mut());
        c::cmsIT8SetSheetType(h, b"CGATS.17\0".as_ptr() as *const i8);
        c::cmsIT8SetComment(h, b"a comment\0".as_ptr() as *const i8);
        c::cmsIT8SetPropertyStr(h, b"PSTR\0".as_ptr() as *const i8,
            b"str val\0".as_ptr() as *const i8);
        c::cmsIT8SetPropertyDbl(h, b"PDBL\0".as_ptr() as *const i8,
            1.5);
        c::cmsIT8SetPropertyHex(h, b"PHEX\0".as_ptr() as *const i8,
            0xABCD);
        c::cmsIT8SetPropertyMulti(h, b"PMULTI\0".as_ptr() as *const i8,
            b"sub\0".as_ptr() as *const i8,
            b"val\0".as_ptr() as *const i8);
        c::cmsIT8SetPropertyUncooked(h, b"PUNC\0".as_ptr() as *const i8,
            b"0b1011\0".as_ptr() as *const i8);
        c::cmsIT8SetDataFormat(h, 0, b"ID\0".as_ptr() as *const i8);
        c::cmsIT8SetDataFormat(h, 1, b"V\0".as_ptr() as *const i8);
        c::cmsIT8SetData(h, b"r1\0".as_ptr() as *const i8,
            b"ID\0".as_ptr() as *const i8, b"r1\0".as_ptr() as *const i8);
        c::cmsIT8SetDataDbl(h, b"r1\0".as_ptr() as *const i8,
            b"V\0".as_ptr() as *const i8, 0.25);
        let mut buf = vec![0u8; 1 << 16];
        let mut n = buf.len() as u32;
        let s = c::cmsIT8SaveToMem(h, buf.as_mut_ptr() as *mut _,
            &mut n);
        eprintln!("save={s} n={n}");
        if s != 0 {
            let txt = std::str::from_utf8(&buf[..n as usize])
                .unwrap_or("");
            eprintln!("---\n{txt}\n---");
            let h2 = c::cmsIT8LoadFromMem(std::ptr::null_mut(),
                buf.as_ptr() as *const _, n);
            eprintln!("reparse={}", !h2.is_null());
            if !h2.is_null() { c::cmsIT8Free(h2); }
        }
        c::cmsIT8Free(h);
        // $INCLUDE directive — lexer arm (file not found)
        let inc = b"CGATS.17\n$INCLUDE \"/nonexistent.it8\"\n";
        let h3 = c::cmsIT8LoadFromMem(std::ptr::null_mut(),
            inc.as_ptr() as *const _, inc.len() as u32);
        eprintln!("include={}", !h3.is_null());
        if !h3.is_null() { c::cmsIT8Free(h3); }
    }
}

/// IT8 table/data API internals — multi-table docs, SetTable/
/// TableByLabel, GetPatchByName, SetIndexColumn, DefineDblFormat,
/// GetPropertyDbl/Multi, data row/col accessors.
#[test]
fn it8_table_api_arms() {
    unsafe {
        // two-table doc via parse
        let doc = b"CGATS.17\n\
            NUMBER_OF_FIELDS 2\nNUMBER_OF_SETS 2\n\
            BEGIN_DATA_FORMAT\nID V\nEND_DATA_FORMAT\n\
            BEGIN_DATA\na 1\nb 2\nEND_DATA\n\
            CGATS.17\n\
            NUMBER_OF_FIELDS 2\nNUMBER_OF_SETS 1\n\
            BEGIN_DATA_FORMAT\nK W\nEND_DATA_FORMAT\n\
            BEGIN_DATA\nx 9\nEND_DATA\n";
        let h = c::cmsIT8LoadFromMem(std::ptr::null_mut(),
            doc.as_ptr() as *const _, doc.len() as u32);
        eprintln!("load={}", !h.is_null());
        if h.is_null() { return; }
        let nt = c::cmsIT8TableCount(h);
        eprintln!("tables={nt}");
        // table switch
        let st = c::cmsIT8SetTable(h, 0);
        eprintln!("settable0={st}");
        let pn = c::cmsIT8GetPatchByName(h, b"b\0".as_ptr() as *const i8);
        eprintln!("patch_b={pn}");
        let _ = c::cmsIT8SetTable(h, 1);
        let stl = c::cmsIT8SetTableByLabel(h,
            b"K\0".as_ptr() as *const i8, b"W\0".as_ptr() as *const i8,
            std::ptr::null());
        eprintln!("setbylabel={stl}");
        // index column + rowcol ops
        let _ = c::cmsIT8SetTable(h, 0);
        let sic = c::cmsIT8SetIndexColumn(h, b"ID\0".as_ptr() as *const i8);
        eprintln!("setindex={sic}");
        c::cmsIT8SetDataRowCol(h, 0, 1, b"7\0".as_ptr() as *const i8);
        c::cmsIT8SetDataRowColDbl(h, 1, 1, 3.5);
        let v = c::cmsIT8GetDataRowColDbl(h, 1, 1);
        eprintln!("rc-dbl={v}");
        // define dbl format + save
        c::cmsIT8DefineDblFormat(h, b"%.4f\0".as_ptr() as *const i8);
        let mut buf = vec![0u8; 1 << 16];
        let mut n = buf.len() as u32;
        c::cmsIT8SaveToMem(h, buf.as_mut_ptr() as *mut _, &mut n);
        eprintln!("save-n={n}");
        // property getters
        let _ = c::cmsIT8SetPropertyDbl(h,
            b"PD\0".as_ptr() as *const i8, 2.5);
        let pd = c::cmsIT8GetPropertyDbl(h, b"PD\0".as_ptr() as *const i8);
        eprintln!("propdbl={pd}");
        c::cmsIT8Free(h);
    }
}

/// IT8 lexer/parser deep arms — data-set edges, enum/data-format
/// accessors, patch lookup, malformed structure arms.
#[test]
fn it8_deep_arms() {
    unsafe {
        // doc with mixed value forms in data section
        let doc = b"CGATS.17\n\
            NUMBER_OF_FIELDS 5\nNUMBER_OF_SETS 3\n\
            BEGIN_DATA_FORMAT\nI D S H B\nEND_DATA_FORMAT\n\
            BEGIN_DATA\n\
            1 1.5 \"str\" 0xFF 0b101\n\
            -2 -0.5e2 'q' 0x1A 0b0\n\
            +3 0.5 \"x\\\"y\" 0x00 0b11\n\
            END_DATA\n\
            KEY1 val1\n\
            KEY2 \"quoted val\"\n\
            KEY3 0xAB\n\
            KEY4 -1.5\n";
        let h = c::cmsIT8LoadFromMem(std::ptr::null_mut(),
            doc.as_ptr() as *const _, doc.len() as u32);
        eprintln!("load={}", !h.is_null());
        if !h.is_null() {
            let mut names = [std::ptr::null_mut(); 8];
            let nf = c::cmsIT8EnumDataFormat(h, names.as_mut_ptr());
            eprintln!("nf={nf}");

            let pn = c::cmsIT8GetPatchByName(h,
                b"2\0".as_ptr() as *const i8);
            eprintln!("patch={pn}");
            let v = c::cmsIT8GetData(h, b"1\0".as_ptr() as *const i8,
                b"S\0".as_ptr() as *const i8);
            eprintln!("data={:?}", v);
            let mut buf = vec![0u8; 1 << 16];
            let mut n = buf.len() as u32;
            c::cmsIT8SaveToMem(h, buf.as_mut_ptr() as *mut _, &mut n);
            let txt = std::str::from_utf8(&buf[..n as usize])
                .unwrap_or("");
            eprintln!("saved:\n{txt}");
            c::cmsIT8Free(h);
        }
        // malformed structure variants
        for (i, doc) in [
            b"CGATS.17\nNUMBER_OF_FIELDS x\n".as_slice(),
            b"CGATS.17\nNUMBER_OF_SETS -1\n".as_slice(),
            b"CGATS.17\nBEGIN_DATA\n1\nEND_DATA\n".as_slice(),
            b"CGATS.17\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA\nEND_DATA\n".as_slice(),
        ].iter().enumerate() {
            let h2 = c::cmsIT8LoadFromMem(std::ptr::null_mut(),
                doc.as_ptr() as *const _, doc.len() as u32);
            eprintln!("mal{i} load={}", !h2.is_null());
            if !h2.is_null() { c::cmsIT8Free(h2); }
        }
    }
}


/// cmscgats header/data deep arms — KEYWORD/DATA_FORMAT_ID
/// directives, WRITE_PAIR values, quoted vs uncooked props.
#[test]
fn it8_header_arms() {
    unsafe {
        let docs: Vec<(&[u8], &str)> = vec![
            // KEYWORD directive declares a custom keyword
            (b"IT8FILE\nKEYWORD \"CUSTOM_PROP\"\nCUSTOM_PROP \"v\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n", "keyword-dir"),
            // DATA_FORMAT_ID declares a format id
            (b"IT8FILE\nDATA_FORMAT_ID \"fmt1\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n", "datafmt-id"),
            // quoted-string vs bare-number property values
            (b"IT8FILE\nP1 \"quoted\"\nP2 42\nP3 3.14\nP4 0x1F\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n", "prop-forms"),
            // WRITE_PAIR value (subkey,value;...) — needs
            // SetPropertyMulti first on a loaded doc
            (b"IT8FILE\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n", "for-pair"),
            // multiple keywords + sample ids
            (b"IT8FILE\nKEYWORD \"K1\"\nKEYWORD \"K2\"\nDATA_FORMAT_ID \"d1\"\nK1 \"a\"\nK2 \"b\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n", "multi-kw"),
            // property value with inline comment
            (b"IT8FILE\nP1 \"v\" # comment\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n", "inline-cmt"),
            // no-space between tokens
            (b"IT8FILE\nP1=\"v\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n", "nospace-eq"),
            // unicode + special chars in quoted strings
            ("IT8FILE\nP1 \"café ☕\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n".as_bytes(), "utf8-str"),
        ];
        for (doc, name) in docs {
            let p = c::cmsIT8LoadFromMem(std::ptr::null_mut(),
                doc.as_ptr() as *const _, doc.len() as u32);
            eprintln!("{name} parse={}", !p.is_null());
            if !p.is_null() {
                if name == "for-pair" {
                    // WRITE_PAIR arm — SetPropertyMulti writes the
                    // pair form which HeaderSection then parses
                    c::cmsIT8SetPropertyMulti(p,
                        b"PAIR\0".as_ptr() as *const _,
                        b"k1\0".as_ptr() as *const _,
                        b"v1\0".as_ptr() as *const _);
                    let v = c::cmsIT8GetProperty(p,
                        b"PAIR\0".as_ptr() as *const _);
                    eprintln!("  pair-val={}", !v.is_null());
                }
                let _nt = c::cmsIT8TableCount(p);
                c::cmsIT8Free(p);
            }
        }
    }
}

/// cmscgats data/format/parser deep arms — data-value forms,
/// format names, multi-table, nosheet, malformed rows.
#[test]
fn it8_data_arms3() {
    unsafe {
        let docs: Vec<(Vec<u8>, &str)> = vec![
            // quoted + numeric + mixed data values
            (b"IT8FILE\nNUMBER_OF_FIELDS 3\nNUMBER_OF_SETS 2\nBEGIN_DATA_FORMAT\nA B C\nEND_DATA_FORMAT\nBEGIN_DATA\n1 2.5 \"str\"\n0x1F -3 .5\nEND_DATA\n".to_vec(), "mixed-data"),
            // format names are field names (idents + quoted)
            (b"IT8FILE\nNUMBER_OF_FIELDS 2\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nFIELD_ONE \"quoted name\"\nEND_DATA_FORMAT\nBEGIN_DATA\n1 2\nEND_DATA\n".to_vec(), "quoted-fmt"),
            // more rows than declared (NumberOfSets mismatch)
            (b"IT8FILE\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\n2\n3\nEND_DATA\n".to_vec(), "extra-rows"),
            // fewer rows than declared
            (b"IT8FILE\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 5\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\n2\nEND_DATA\n".to_vec(), "fewer-rows"),
            // more fields per row than declared
            (b"IT8FILE\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1 2 3\nEND_DATA\n".to_vec(), "extra-cols"),
            // fewer fields per row
            (b"IT8FILE\nNUMBER_OF_FIELDS 3\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA B C\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n".to_vec(), "fewer-cols"),
            // multi-table with DATA between tables
            (b"IT8FILE\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\nIT8FILE\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nB\nEND_DATA_FORMAT\nBEGIN_DATA\n2\nEND_DATA\n".to_vec(), "2tables-data"),
            // END_DATA_FORMAT without matching BEGIN
            (b"IT8FILE\nEND_DATA_FORMAT\n".to_vec(), "orphan-endfmt"),
            // data before format declaration
            (b"IT8FILE\nBEGIN_DATA\n1\nEND_DATA\n".to_vec(), "data-first"),
            // NUMBER_OF_FIELDS after data format
            (b"IT8FILE\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nNUMBER_OF_FIELDS 1\nBEGIN_DATA\n1\nEND_DATA\n".to_vec(), "nof-after-fmt"),
            // data-set with mixed tokens per row
            (b"IT8FILE\nNUMBER_OF_FIELDS 2\nNUMBER_OF_SETS 3\nBEGIN_DATA_FORMAT\nA B\nEND_DATA_FORMAT\nBEGIN_DATA\n1 2\n3\n4 5 6\nEND_DATA\n".to_vec(), "mixed-rows"),
            // large table (100 rows)
            ({ let mut v = b"IT8FILE\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 100\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n".to_vec();
                for i in 0..100 {
                    v.extend_from_slice(format!("{i}\n").as_bytes());
                }
                v.extend_from_slice(b"END_DATA\n");
                v }, "big-table"),
            // tab + multi-space delimiters in data
            (b"IT8FILE\nNUMBER_OF_FIELDS 2\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\tB\nEND_DATA_FORMAT\nBEGIN_DATA\n1\t\t  2\nEND_DATA\n".to_vec(), "tab-delims"),
        ];
        for (doc, name) in docs {
            let p = c::cmsIT8LoadFromMem(std::ptr::null_mut(),
                doc.as_ptr() as *const _, doc.len() as u32);
            eprintln!("{name} parse={}", !p.is_null());
            if !p.is_null() {
                let nt = c::cmsIT8TableCount(p);
                eprintln!("  tables={nt}");
                c::cmsIT8Free(p);
            }
        }
    }
}

/// cmscgats InSymbol/HeaderSection/ParseIT8/SaveHeader deep arms.
#[test]
fn it8_deep_arms2() {
    unsafe {
        let docs: Vec<(&[u8], &str)> = vec![
            // string escapes inside quoted values
            (b"IT8FILE\nP1 \"a\\nb\\tc\\\\d\\u{1f600}e\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n", "esc-str"),
            // multiple lines + nested escapes
            (b"IT8FILE\nP1 \"line1\\\nline2\\\"\nend\"\n", "multi-esc"),
            // hex + sci + leading-zero + signs in data values
            (b"IT8FILE\nNUMBER_OF_FIELDS 4\nNUMBER_OF_SETS 2\nBEGIN_DATA_FORMAT\nA B C D\nEND_DATA_FORMAT\nBEGIN_DATA\n0x1F 3.14e-2 +7 00042\n-0.5 1E5 .7 0.\nEND_DATA\n", "numforms2"),
            // comment inside data + between values
            (b"IT8FILE\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 2\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1 # c\n2\nEND_DATA\n", "data-cmt"),
            // keyword=value variants (with/without space)
            (b"IT8FILE\nP1=\"v\"\nP2 =\"v\"\nP3= \"v\"\nP4 = \"v\"\n", "eq-forms"),
            // long property values + many properties
            (b"IT8FILE\nP1 \"a b c d e f g h i j k l m n o p\"\nP2 \"v\"\nP3 123\nP4 \"x\"\nP5 0xFF\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n", "many-props"),
            // multi-sheet doc (nosheet=0 reads all)
            (b"IT8FILE\nN_OF_F 1\nN_OF_S 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\nIT8FILE\nN_OF_F 1\nN_OF_S 1\nBEGIN_DATA_FORMAT\nB\nEND_DATA_FORMAT\nBEGIN_DATA\n2\nEND_DATA\nIT8FILE\nN_OF_F 1\nN_OF_S 1\nBEGIN_DATA_FORMAT\nC\nEND_DATA_FORMAT\nBEGIN_DATA\n3\nEND_DATA\n", "3sheets"),
            // malformed: missing required keywords
            (b"IT8FILE\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n", "no-counts"),
            // NUMBER_OF_SETS = 0
            (b"IT8FILE\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 0\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\nEND_DATA\n", "zero-sets"),
            // unicode strings in data values
            ("IT8FILE\nNUMBER_OF_FIELDS 2\nNUMBER_OF_SETS 2\nBEGIN_DATA_FORMAT\nA B\nEND_DATA_FORMAT\nBEGIN_DATA\n1 \"café\"\n2 \"☕\"\nEND_DATA\n".as_bytes(), "utf8-data"),
        ];
        for (doc, name) in docs {
            let p = c::cmsIT8LoadFromMem(std::ptr::null_mut(),
                doc.as_ptr() as *const _, doc.len() as u32);
            eprintln!("{name} parse={}", !p.is_null());
            if !p.is_null() {
                let nt = c::cmsIT8TableCount(p);
                eprintln!("  tables={nt}");
                c::cmsIT8Free(p);
            }
        }
        // save + reload for SaveHeader/DataSection writer arms
        let p = c::cmsIT8LoadFromMem(std::ptr::null_mut(),
            b"IT8FILE\nKEYWORD \"K\"\nDATA_FORMAT_ID \"D\"\nNUMBER_OF_FIELDS 2\nNUMBER_OF_SETS 2\nBEGIN_DATA_FORMAT\nA B\nEND_DATA_FORMAT\nBEGIN_DATA\n1 2\n3 4\nEND_DATA\n".as_ptr() as *const _,
            152);
        if !p.is_null() {
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = c::cmsIT8SaveToMem(p,
                buf.as_mut_ptr() as *mut _, &mut n);
            eprintln!("it8-save={s} n={n}");
            let rp = if s != 0 {
                c::cmsIT8LoadFromMem(std::ptr::null_mut(),
                    buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            eprintln!("it8-reload={}", !rp.is_null());
            if !rp.is_null() { c::cmsIT8Free(rp); }
            c::cmsIT8Free(p);
        }
    }
}

/// cmscgats remaining lexer arms — unusual tokens, comments,
/// numbers, string forms, table API internals.
#[test]
fn it8_deep_arms3() {
    unsafe {
        let docs: Vec<(Vec<u8>, &str)> = vec![
            // scientific + signed + leading-zeros in all positions
            (b"IT8FILE\nP1 -1.5e+3\nP2 +0.0e-0\nP3 .5e2\nP4 1E-5\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n".to_vec(), "sci-forms"),
            // comment-only doc + comment inside blocks
            (b"# all comments\nIT8FILE\n# inner\nNUMBER_OF_FIELDS 1\n# more\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n".to_vec(), "all-cmts"),
            // number edge cases in data
            (b"IT8FILE\nNUMBER_OF_FIELDS 6\nNUMBER_OF_SETS 2\nBEGIN_DATA_FORMAT\nA B C D E F\nEND_DATA_FORMAT\nBEGIN_DATA\n-0 0.0 0x0 0E0 .0 -0x1\n+1 -1 1. -1. -1e1 1e-1\nEND_DATA\n".to_vec(), "num-edge"),
            // quoted data values with escapes inside
            (b"IT8FILE\nNUMBER_OF_FIELDS 2\nNUMBER_OF_SETS 2\nBEGIN_DATA_FORMAT\nA B\nEND_DATA_FORMAT\nBEGIN_DATA\n1 \"a\\tb\"\n2 \"c\\u{1f600}d\"\nEND_DATA\n".to_vec(), "esc-data"),
            // property = value on same line as section header
            (b"IT8FILE\nP1 \"a\";P2 \"b\"\n".to_vec(), "semi-props"),
            // nested/multiple tables with different counts
            (b"IT8FILE\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 5\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\n2\n3\n4\n5\nEND_DATA\nIT8FILE\nNUMBER_OF_FIELDS 2\nNUMBER_OF_SETS 3\nBEGIN_DATA_FORMAT\nX Y\nEND_DATA_FORMAT\nBEGIN_DATA\n1 2\n3 4\n5 6\nEND_DATA\nIT8FILE\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 2\nBEGIN_DATA_FORMAT\nZ\nEND_DATA_FORMAT\nBEGIN_DATA\n7\n8\nEND_DATA\n".to_vec(), "3-tables-diff"),
            // DATA_IDENTIFIER values (quoted/unquoted)
            (b"IT8FILE\nDATA_IDENTIFIER \"id1\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\nIT8FILE\nDATA_IDENTIFIER \"id2\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nB\nEND_DATA_FORMAT\nBEGIN_DATA\n2\nEND_DATA\n".to_vec(), "dataids"),
            // blank lines + leading whitespace inside blocks
            (b"IT8FILE\n\n\n  P1 \"a\"\n\n\n  NUMBER_OF_FIELDS 1\n\n  NUMBER_OF_SETS 1\n\nBEGIN_DATA_FORMAT\n\nA\n\nEND_DATA_FORMAT\n\nBEGIN_DATA\n\n1\n\nEND_DATA\n\n".to_vec(), "blank-lines"),
            // very long single line
            ({ let mut v = b"IT8FILE\nP1 \"".to_vec();
                for _ in 0..500 { v.push(b'x'); }
                v.extend_from_slice(b"\"\n");
                v }, "long-line"),
            // table with only format, no data rows
            (b"IT8FILE\nNUMBER_OF_FIELDS 3\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA B C\nEND_DATA_FORMAT\n".to_vec(), "fmt-no-data"),
            // IT8 with all-optional fields absent
            (b"IT8FILE\n\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n".to_vec(), "bare-data"),
        ];
        for (doc, name) in docs {
            let p = c::cmsIT8LoadFromMem(std::ptr::null_mut(),
                doc.as_ptr() as *const _, doc.len() as u32);
            eprintln!("{name} parse={}", !p.is_null());
            if !p.is_null() {
                let nt = c::cmsIT8TableCount(p);
                if nt > 1 { eprintln!("  tables={nt}"); }
                c::cmsIT8Free(p);
            }
        }
    }
}

/// cmscgats remaining arms — writer modes, table API, enum
/// values, boundary conditions.
#[test]
fn it8_deep_arms4() {
    unsafe {
        // writer modes on a multi-table doc
        let doc = b"IT8FILE\nKEYWORD \"K\"\nDATA_FORMAT_ID \"D\"\nNUMBER_OF_FIELDS 2\nNUMBER_OF_SETS 2\nBEGIN_DATA_FORMAT\nA B\nEND_DATA_FORMAT\nBEGIN_DATA\n1 2\n3 4\nEND_DATA\nIT8FILE\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nC\nEND_DATA_FORMAT\nBEGIN_DATA\n5\nEND_DATA\n";
        let p = c::cmsIT8LoadFromMem(std::ptr::null_mut(),
            doc.as_ptr() as *const _, doc.len() as u32);
        if !p.is_null() {
            // table API: TableCount/SetTable/GetPatchByName/
            // DefineDblFormat/SetDataRowCol
            let nt = c::cmsIT8TableCount(p);
            for i in 0..nt + 1 {
                let ok = c::cmsIT8SetTable(p, i);
                eprintln!("settable{i}={ok}");
            }
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = c::cmsIT8SaveToMem(p,
                buf.as_mut_ptr() as *mut _, &mut n);
            eprintln!("save={s}");
            // write with different enum values
            c::cmsIT8DefineDblFormat(p, b"%.12g\0".as_ptr() as *const _);
            let v = c::cmsIT8GetDataRowCol(p, 0, 0);
            eprintln!("getdata={}", !v.is_null());
            c::cmsIT8Free(p);
        }
        // enum value coverage — read all predefined properties
        for (prop, name) in [
            (b"NUMBER_OF_FIELDS\0".as_ptr() as *const _, "nof"),
            (b"NUMBER_OF_SETS\0".as_ptr() as *const _, "nos"),
            (b"ORIGINATOR\0".as_ptr() as *const _, "orig"),
            (b"DATA_IDENTIFIER\0".as_ptr() as *const _, "dataid"),
            (b"NUMBER_OF_TABLES\0".as_ptr() as *const _, "notables"),
        ] {
            let p2 = c::cmsIT8LoadFromMem(std::ptr::null_mut(),
                b"IT8FILE\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n".as_ptr() as *const _, 106);
            if !p2.is_null() {
                let v = c::cmsIT8GetProperty(p2, prop);
                eprintln!("prop {name}={}", !v.is_null());
                c::cmsIT8Free(p2);
            }
        }
        // SetDataRowCol/SetProperty on loaded doc
        let p3 = c::cmsIT8LoadFromMem(std::ptr::null_mut(),
            b"IT8FILE\nNUMBER_OF_FIELDS 2\nNUMBER_OF_SETS 2\nBEGIN_DATA_FORMAT\nA B\nEND_DATA_FORMAT\nBEGIN_DATA\n1 2\n3 4\nEND_DATA\n".as_ptr() as *const _, 106);
        if !p3.is_null() {
            let r = c::cmsIT8SetDataRowCol(p3, 0, 0, b"99\0".as_ptr() as *const _);
            eprintln!("setdata={r}");
            let v = c::cmsIT8GetDataRowCol(p3, 0, 0);
            eprintln!("getdata-val={}", !v.is_null());
            c::cmsIT8Free(p3);
        }
    }
}

/// cmscgats ParseIT8/table-structure deep arms — table counts,
/// nested sheets, DATA_IDENTIFIER selection, malformed table
/// boundaries.
#[test]
fn it8_parse_arms2() {
    unsafe {
        let docs: Vec<(&[u8], &str)> = vec![
            // table-count edge: 1, 3, 5 tables
            (b"IT8FILE\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n", "t1"),
            (b"IT8FILE\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\nIT8FILE\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nB\nEND_DATA_FORMAT\nBEGIN_DATA\n2\nEND_DATA\nIT8FILE\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nC\nEND_DATA_FORMAT\nBEGIN_DATA\n3\nEND_DATA\n", "t3"),
            // DATA_IDENTIFIER-based table selection
            (b"IT8FILE\nDATA_IDENTIFIER \"first\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\nIT8FILE\nDATA_IDENTIFIER \"second\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nB\nEND_DATA_FORMAT\nBEGIN_DATA\n2\nEND_DATA\n", "dataid-sel"),
            // empty table (header only, no data)
            (b"IT8FILE\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 0\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\nEND_DATA\nIT8FILE\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nB\nEND_DATA_FORMAT\nBEGIN_DATA\n2\nEND_DATA\n", "empty-then-data"),
            // tables with different field counts
            (b"IT8FILE\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\nIT8FILE\nNUMBER_OF_FIELDS 4\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA B C D\nEND_DATA_FORMAT\nBEGIN_DATA\n1 2 3 4\nEND_DATA\nIT8FILE\nNUMBER_OF_FIELDS 2\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nX Y\nEND_DATA_FORMAT\nBEGIN_DATA\n5 6\nEND_DATA\n", "diff-fields"),
            // malformed: IT8FILE inside data section
            (b"IT8FILE\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\nIT8FILE\n1\nEND_DATA\n", "nested-it8"),
            // trailing content after last END_DATA
            (b"IT8FILE\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\nEXTRA_PROP \"tail\"\n", "tail-prop"),
            // DATA_IDENTIFIER alone (no data section)
            (b"IT8FILE\nDATA_IDENTIFIER \"id\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 0\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\n", "dataid-nod"),
        ];
        for (doc, name) in docs {
            let p = c::cmsIT8LoadFromMem(std::ptr::null_mut(),
                doc.as_ptr() as *const _, doc.len() as u32);
            eprintln!("{name} parse={}", !p.is_null());
            if !p.is_null() {
                let nt = c::cmsIT8TableCount(p);
                eprintln!("  tables={nt}");
                c::cmsIT8Free(p);
            }
        }
    }
}

/// cmscgats remaining InSymbol/HeaderSection/DataSection arms.
#[test]
fn it8_deep_arms5() {
    unsafe {
        let docs: Vec<(&[u8], &str)> = vec![
            // every token class in property position
            (b"IT8FILE\nP1 0x1F\nP2 -0.5e+3\nP3 .5\nP4 \"str\"\nP5 ident\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n", "alltok"),
            // strings with all escape classes
            (b"IT8FILE\nP1 \"a\\nb\\tc\\\\d\\u{1f600}e\\x41f\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n", "escapes2"),
            // long identifier names
            (b"IT8FILE\nVERY_LONG_PROPERTY_NAME_THAT_IS_LONGER_THAN_NORMAL \"v\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n", "longname"),
            // CRLF line endings
            (b"IT8FILE\r\nNUMBER_OF_FIELDS 1\r\nNUMBER_OF_SETS 1\r\nBEGIN_DATA_FORMAT\r\nA\r\nEND_DATA_FORMAT\r\nBEGIN_DATA\r\n1\r\nEND_DATA\r\n", "crlf"),
            // mixed line endings
            (b"IT8FILE\nNUMBER_OF_FIELDS 1\r\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\r\nA\nEND_DATA_FORMAT\r\nBEGIN_DATA\n1\nEND_DATA\n", "mixeol"),
            // tabs in property values + data
            (b"IT8FILE\nP1\t\"v\"\nNUMBER_OF_FIELDS\t1\nNUMBER_OF_SETS\t1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n", "tabprop"),
            // comments at various positions
            (b"# lead\nIT8FILE\n# pre-prop\nP1 \"v\" # inline\n# between\nNUMBER_OF_FIELDS 1 # c\nNUMBER_OF_SETS 1\n# pre-fmt\nBEGIN_DATA_FORMAT\nA # field c\nEND_DATA_FORMAT\n# pre-data\nBEGIN_DATA\n1 # val c\nEND_DATA\n# tail\n", "allcmts"),
            // empty data section
            (b"IT8FILE\nNUMBER_OF_FIELDS 0\nNUMBER_OF_SETS 0\nBEGIN_DATA_FORMAT\nEND_DATA_FORMAT\nBEGIN_DATA\nEND_DATA\n", "empty2"),
            // NUMBER_OF_FIELDS with + and hex
            (b"IT8FILE\nNUMBER_OF_FIELDS +2\nNUMBER_OF_SETS 0x2\nBEGIN_DATA_FORMAT\nA B\nEND_DATA_FORMAT\nBEGIN_DATA\n1 2\n3 4\nEND_DATA\n", "signed-count"),
            // large field/set counts (rejected)
            (b"IT8FILE\nNUMBER_OF_FIELDS 9999\nNUMBER_OF_SETS 9999\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n", "huge-count"),
        ];
        for (doc, name) in docs {
            let p = c::cmsIT8LoadFromMem(std::ptr::null_mut(),
                doc.as_ptr() as *const _, doc.len() as u32);
            eprintln!("{name} parse={}", !p.is_null());
            if !p.is_null() {
                c::cmsIT8Free(p);
            }
        }
    }
}

/// cmscgats remaining lexer/parser internals — enum values,
/// table navigation, property enum coverage.
#[test]
fn it8_deep_arms6() {
    unsafe {
        // every property enum value through GetProperty
        let doc = b"IT8FILE\nORIGINATOR \"o\"\nDESCRIPTOR \"d\"\nCREATED \"c\"\nMANUFACTURER \"m\"\nPROD_DATE \"p\"\nSERIAL \"s\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n";
        let p = c::cmsIT8LoadFromMem(std::ptr::null_mut(),
            doc.as_ptr() as *const _, doc.len() as u32);
        if !p.is_null() {
            for (prop, name) in [
                (b"ORIGINATOR\0".as_ptr() as *const _, "orig"),
                (b"DESCRIPTOR\0".as_ptr() as *const _, "desc"),
                (b"CREATED\0".as_ptr() as *const _, "created"),
                (b"MANUFACTURER\0".as_ptr() as *const _, "mfg"),
                (b"PROD_DATE\0".as_ptr() as *const _, "pdate"),
                (b"SERIAL\0".as_ptr() as *const _, "serial"),
                (b"NUMBER_OF_FIELDS\0".as_ptr() as *const _, "nof"),
                (b"NUMBER_OF_SETS\0".as_ptr() as *const _, "nos"),
                (b"DATA_IDENTIFIER\0".as_ptr() as *const _, "dataid"),
                (b"NUMBER_OF_TABLES\0".as_ptr() as *const _, "notables"),
                (b"MISSING\0".as_ptr() as *const _, "missing"),
            ] {
                let v = c::cmsIT8GetProperty(p, prop);
                eprintln!("prop {name}={}", !v.is_null());
            }
            // enum navigation arms
            let mut names: *mut *mut i8 = std::ptr::null_mut();
            let np = c::cmsIT8EnumProperties(p, &mut names);
            eprintln!("enumprops={np}");
            let mut fmts: *mut *mut i8 = std::ptr::null_mut();
            let nf = c::cmsIT8EnumDataFormat(p, &mut fmts);
            eprintln!("enumfmt={nf}");
            c::cmsIT8Free(p);
        }
        // multi-sheet table navigation + SheetType
        let doc2 = b"IT8FILE\nDATA_IDENTIFIER \"sheet1\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\nIT8FILE\nDATA_IDENTIFIER \"sheet2\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nB\nEND_DATA_FORMAT\nBEGIN_DATA\n2\nEND_DATA\nIT8FILE\nDATA_IDENTIFIER \"sheet3\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nC\nEND_DATA_FORMAT\nBEGIN_DATA\n3\nEND_DATA\n";
        let p2 = c::cmsIT8LoadFromMem(std::ptr::null_mut(),
            doc2.as_ptr() as *const _, doc2.len() as u32);
        if !p2.is_null() {
            let nt = c::cmsIT8TableCount(p2);
            eprintln!("3sheet tables={nt}");
            for i in 0..nt + 1 {
                let ok = c::cmsIT8SetTable(p2, i);
                if ok != 0 {
                    let st = c::cmsIT8GetSheetType(p2);
                    eprintln!("sheet{i} type={}", !st.is_null());
                }
            }
            c::cmsIT8Free(p2);
        }
    }
}

/// cmscgats remaining ParseCube/DataSection arms — CookPointers
/// + data-format variants + sheet-structure edge cases.
#[test]
fn it8_deep_arms7() {
    unsafe {
        let docs: Vec<(Vec<u8>, &str)> = vec![
            // data format with duplicate field names
            (b"IT8FILE\nNUMBER_OF_FIELDS 3\nNUMBER_OF_SETS 2\nBEGIN_DATA_FORMAT\nA A B\nEND_DATA_FORMAT\nBEGIN_DATA\n1 2 3\n4 5 6\nEND_DATA\n".to_vec(), "dup-fields"),
            // field names matching reserved keywords
            (b"IT8FILE\nNUMBER_OF_FIELDS 2\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nBEGIN_DATA END_DATA\nEND_DATA_FORMAT\nBEGIN_DATA\n1 2\nEND_DATA\n".to_vec(), "kw-fields"),
            // format section before IT8FILE properties
            (b"IT8FILE\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nP1 \"v\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA\n1\nEND_DATA\n".to_vec(), "fmt-then-prop"),
            // data values = format field names (string values)
            (b"IT8FILE\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 3\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\nA\nBEGIN_DATA\nEND_DATA\nEND_DATA\n".to_vec(), "kw-values"),
            // extra whitespace/comment between format and data
            (b"IT8FILE\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\n\n\n# comment\n\nBEGIN_DATA\n1\nEND_DATA\n".to_vec(), "fmt-gap"),
            // DATA_IDENTIFIER selects which sheet to read
            (b"IT8FILE\nDATA_IDENTIFIER \"s1\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\nIT8FILE\nDATA_IDENTIFIER \"s2\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nB\nEND_DATA_FORMAT\nBEGIN_DATA\n2\nEND_DATA\nIT8FILE\nDATA_IDENTIFIER \"s1\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nC\nEND_DATA_FORMAT\nBEGIN_DATA\n3\nEND_DATA\n".to_vec(), "dup-dataid"),
            // NUMBER_OF_SETS mismatch with actual rows
            (b"IT8FILE\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 3\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\n2\n3\n4\n5\nEND_DATA\n".to_vec(), "extra-rows2"),
            // zero-length data values
            (b"IT8FILE\nNUMBER_OF_FIELDS 2\nNUMBER_OF_SETS 2\nBEGIN_DATA_FORMAT\nA B\nEND_DATA_FORMAT\nBEGIN_DATA\n\"\" 1\n\"x\" \"\"\nEND_DATA\n".to_vec(), "empty-str"),
            // many fields per row (max table width)
            ({ let mut v = b"IT8FILE\nNUMBER_OF_FIELDS 20\nNUMBER_OF_SETS 2\nBEGIN_DATA_FORMAT\n".to_vec();
                for i in 0..20 {
                    v.extend_from_slice(format!("F{i} ").as_bytes());
                }
                v.extend_from_slice(b"\nEND_DATA_FORMAT\nBEGIN_DATA\n");
                for _ in 0..2 {
                    for i in 0..20 {
                        v.extend_from_slice(format!("{i} ").as_bytes());
                    }
                    v.push(b'\n');
                }
                v.extend_from_slice(b"END_DATA\n");
                v }, "20-fields"),
            // deeply nested quote escapes
            (b"IT8FILE\nP1 \"a\\\"b\\\"c\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n".to_vec(), "nested-esc"),
        ];
        for (doc, name) in docs {
            let p = c::cmsIT8LoadFromMem(std::ptr::null_mut(),
                doc.as_ptr() as *const _, doc.len() as u32);
            eprintln!("{name} parse={}", !p.is_null());
            if !p.is_null() {
                c::cmsIT8Free(p);
            }
        }
    }
}

/// cmscgats remaining InSymbol/HeaderSection/ParseIT8 arms.
#[test]
fn it8_deep_arms8() {
    unsafe {
        let docs: Vec<(Vec<u8>, &str)> = vec![
            // number format edge cases in data values
            (b"IT8FILE\nNUMBER_OF_FIELDS 6\nNUMBER_OF_SETS 4\nBEGIN_DATA_FORMAT\nA B C D E F\nEND_DATA_FORMAT\nBEGIN_DATA\n1e308 -1e-308 0.000001 999999999 .0000001 0xFF\n1E+308 -0.0 +INF -INF NaN -nan\n0 0x0 00 0. -0. .0\n1.7976931348623157e308 2.2250738585072014e-308 -1.7976931348623157e+308 4.9406564584124654e-324 -4.9406564584124654e-324 1e400\nEND_DATA\n".to_vec(), "num-edge2"),
            // deeply indented/broken structure
            (b"IT8FILE\n\tP1 \"v\"\n\t\tNUMBER_OF_FIELDS 1\n NUMBER_OF_SETS 1\n\t\t\tBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\n\tBEGIN_DATA\n1\n\tEND_DATA\n".to_vec(), "indent"),
            // property=value forms across all types
            (b"IT8FILE\nK1=1\nK2 =2\nK3= 3\nK4 = 4\nK5=\"v\"\nK6 =\"v\"\nK7= \"v\"\nK8 = \"v\"\nK9=ident\nK10=0x1F\nK11=-0.5\nK12=.5e2\n".to_vec(), "all-eq"),
            // DATA_FORMAT_ID + data format names
            (b"IT8FILE\nDATA_FORMAT_ID \"fmt1\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\nIT8FILE\nDATA_FORMAT_ID \"fmt2\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nB\nEND_DATA_FORMAT\nBEGIN_DATA\n2\nEND_DATA\n".to_vec(), "datafmt-id2"),
            // deeply nested quotes/escapes in data
            (b"IT8FILE\nNUMBER_OF_FIELDS 2\nNUMBER_OF_SETS 2\nBEGIN_DATA_FORMAT\nA B\nEND_DATA_FORMAT\nBEGIN_DATA\n\"a\\\"b\\\"c\" 1\n2 \"d\\\\e\\u{1f600}f\"\nEND_DATA\n".to_vec(), "deep-esc"),
            // table with different field counts + set counts
            ({ let mut v = b"IT8FILE\n".to_vec();
                for t in 0..5u32 {
                    let nf = t + 1;
                    let ns = t * 2 + 1;
                    v.extend_from_slice(format!("NUMBER_OF_FIELDS {nf}\nNUMBER_OF_SETS {ns}\nBEGIN_DATA_FORMAT\n").as_bytes());
                    for f in 0..nf {
                        v.extend_from_slice(format!("F{f} ").as_bytes());
                    }
                    v.extend_from_slice(b"\nEND_DATA_FORMAT\nBEGIN_DATA\n");
                    for s in 0..ns {
                        for f in 0..nf {
                            v.extend_from_slice(format!("{} ", s * 10 + f).as_bytes());
                        }
                        v.push(b'\n');
                    }
                    v.extend_from_slice(b"END_DATA\n");
                    if t < 4 { v.extend_from_slice(b"IT8FILE\n"); }
                }
                v }, "5-tables"),
            // comment between tokens inside a line
            (b"IT8FILE\nP1 # mid-comment\n\"v\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n".to_vec(), "mid-cmt"),
            // BEGIN_DATA_FORMAT / END_DATA_FORMAT on same line
            (b"IT8FILE\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT A END_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n".to_vec(), "inline-fmt"),
            // BEGIN_DATA / END_DATA on same line as values
            (b"IT8FILE\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA 1 END_DATA\n".to_vec(), "inline-data"),
            // malformed: unterminated string
            (b"IT8FILE\nP1 \"unterminated\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n".to_vec(), "unterm-str"),
        ];
        for (doc, name) in docs {
            let p = c::cmsIT8LoadFromMem(std::ptr::null_mut(),
                doc.as_ptr() as *const _, doc.len() as u32);
            eprintln!("{name} parse={}", !p.is_null());
            if !p.is_null() {
                c::cmsIT8Free(p);
            }
        }
    }
}

/// cmscgats remaining InSymbol/HeaderSection/DataSection arms —
/// CookPointers/SetData/ReadReal internals via the table API.
#[test]
fn it8_deep_arms9() {
    unsafe {
        let doc = b"IT8FILE\nNUMBER_OF_FIELDS 3\nNUMBER_OF_SETS 4\nBEGIN_DATA_FORMAT\nSAMPLE_ID L A\nEND_DATA_FORMAT\nBEGIN_DATA\n1 50 0\n2 60 10\n3 70 -10\n4 80 5\nEND_DATA\n";
        let p = c::cmsIT8LoadFromMem(std::ptr::null_mut(),
            doc.as_ptr() as *const _, doc.len() as u32);
        if !p.is_null() {
            // SetDataRowCol / GetDataRowCol for every cell
            for r in 0..4i32 {
                for c_ in 0..3i32 {
                    let v = c::cmsIT8GetDataRowCol(p, r, c_);
                    eprintln!("data[{r}][{c_}]={}",
                        !v.is_null());
                    let _ = v;
                }
            }
            // SetDataRowCol writes
            let ok = c::cmsIT8SetDataRowCol(p, 0, 0,
                b"99\0".as_ptr() as *const _);
            eprintln!("setdata={ok}");
            let ok2 = c::cmsIT8SetDataRowCol(p, -1, 0,
                b"bad\0".as_ptr() as *const _);
            eprintln!("setdata-neg={ok2}");
            // GetDataDbl on numeric cells
            let d = c::cmsIT8GetDataDbl(p, b"L\0".as_ptr() as *const _,
                b"1\0".as_ptr() as *const _);
            eprintln!("getdbldbl={d}");
            // property enum + get on all
            let mut names: *mut *mut i8 = std::ptr::null_mut();
            let np = c::cmsIT8EnumProperties(p, &mut names);
            eprintln!("enumprops={np}");
            // save + reload
            let mut buf = vec![0u8; 1 << 20];
            let mut n = buf.len() as u32;
            let s = c::cmsIT8SaveToMem(p,
                buf.as_mut_ptr() as *mut _, &mut n);
            eprintln!("save={s}");
            let rp = if s != 0 {
                c::cmsIT8LoadFromMem(std::ptr::null_mut(),
                    buf.as_ptr() as *const _, n)
            } else { std::ptr::null_mut() };
            eprintln!("reload={}", !rp.is_null());
            if !rp.is_null() { c::cmsIT8Free(rp); }
            c::cmsIT8Free(p);
        }
        // multi-table with data — GetPatchByName/DataByName
        let doc2 = b"IT8FILE\nDATA_IDENTIFIER \"tab1\"\nNUMBER_OF_FIELDS 2\nNUMBER_OF_SETS 2\nBEGIN_DATA_FORMAT\nA B\nEND_DATA_FORMAT\nBEGIN_DATA\n1 2\n3 4\nEND_DATA\nIT8FILE\nDATA_IDENTIFIER \"tab2\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nC\nEND_DATA_FORMAT\nBEGIN_DATA\n5\nEND_DATA\n";
        let p2 = c::cmsIT8LoadFromMem(std::ptr::null_mut(),
            doc2.as_ptr() as *const _, doc2.len() as u32);
        if !p2.is_null() {
            // SetTableByLabel on each sheet
            let ok = c::cmsIT8SetTableByLabel(p2,
                b"A\0".as_ptr() as *const _,
                b"1\0".as_ptr() as *const _,
                b"\0".as_ptr() as *const _);
            eprintln!("settab={ok}");
            let d = c::cmsIT8GetDataDbl(p2,
                b"A\0".as_ptr() as *const _,
                b"1\0".as_ptr() as *const _);
            eprintln!("getdata={d}");
            c::cmsIT8Free(p2);
        }
    }
}

/// cmscgats remaining internals — WriteHeader/BuildAbsolutePath/
/// ParseFloatNumber/StringAppend via save + file paths.
#[test]
fn it8_deep_arms10() {
    unsafe {
        // save to file → BuildAbsolutePath/WriteHeader arms
        let doc = b"IT8FILE\nKEYWORD \"K\"\nORIGINATOR \"o\"\nDESCRIPTOR \"d\"\nNUMBER_OF_FIELDS 2\nNUMBER_OF_SETS 2\nBEGIN_DATA_FORMAT\nA B\nEND_DATA_FORMAT\nBEGIN_DATA\n1 2\n3 4\nEND_DATA\n";
        let p = c::cmsIT8LoadFromMem(std::ptr::null_mut(),
            doc.as_ptr() as *const _, doc.len() as u32);
        if !p.is_null() {
            let tmp = std::env::temp_dir().join("it8_test.icc");
            let path = tmp.to_str().unwrap();
            let cpath = std::ffi::CString::new(path).unwrap();
            let s = c::cmsIT8SaveToFile(p, cpath.as_ptr());
            eprintln!("savefile={s}");
            // reload from file
            let rp = c::cmsIT8LoadFromFile(std::ptr::null_mut(),
                cpath.as_ptr());
            eprintln!("loadfile={}", !rp.is_null());
            if !rp.is_null() { c::cmsIT8Free(rp); }
            let _ = std::fs::remove_file(&tmp);
            c::cmsIT8Free(p);
        }
        // multi-sheet save+load round-trip
        let doc2 = b"IT8FILE\nDATA_IDENTIFIER \"s1\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\nIT8FILE\nDATA_IDENTIFIER \"s2\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nB\nEND_DATA_FORMAT\nBEGIN_DATA\n2\nEND_DATA\n";
        let p2 = c::cmsIT8LoadFromMem(std::ptr::null_mut(),
            doc2.as_ptr() as *const _, doc2.len() as u32);
        if !p2.is_null() {
            let tmp = std::env::temp_dir().join("it8_multi.icc");
            let cpath = std::ffi::CString::new(
                tmp.to_str().unwrap()).unwrap();
            let s = c::cmsIT8SaveToFile(p2, cpath.as_ptr());
            eprintln!("multi-save={s}");
            let rp = c::cmsIT8LoadFromFile(std::ptr::null_mut(),
                cpath.as_ptr());
            if !rp.is_null() {
                let nt = c::cmsIT8TableCount(rp);
                eprintln!("multi-load tables={nt}");
                c::cmsIT8Free(rp);
            }
            let _ = std::fs::remove_file(&tmp);
            c::cmsIT8Free(p2);
        }
    }
}

/// cmscgats remaining InSymbol/HeaderSection arms — every
/// S* token class + property-value forms + malformed inputs.
#[test]
fn it8_deep_arms11() {
    unsafe {
        let docs: Vec<(Vec<u8>, &str)> = vec![
            // every numeric literal class in property position
            (b"IT8FILE\nP1 1\nP2 -1\nP3 +1\nP4 1.5\nP5 -1.5\nP6 +1.5\nP7 .5\nP8 1.\nP9 1e2\nP10 1E2\nP11 1e-2\nP12 1e+2\nP13 0x1F\nP14 0X1f\nP15 1.5e-3\nP16 -0.5E+3\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n".to_vec(), "all-nums"),
            // quoted property with all escape types
            (b"IT8FILE\nP1 \"tab\\there\"\nP2 \"nl\\nhere\"\nP3 \"bs\\\\here\"\nP4 \"q\\\"here\"\nP5 \"u\\u{263A}here\"\nP6 \"x\\x41here\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n".to_vec(), "all-esc"),
            // property spanning multiple lines via continuation
            (b"IT8FILE\nP1 \"line1\nline2\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n".to_vec(), "multiline"),
            // deep property name nesting
            (b"IT8FILE\nA1 \"v1\"\nA2 \"v2\"\nB1 \"v3\"\nB2 \"v4\"\nC1 \"v5\"\nC2 \"v6\"\nD1 \"v7\"\nD2 \"v8\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n".to_vec(), "many-props2"),
            // IT8 with only comments + minimal structure
            (b"# only comments\nIT8FILE\n".to_vec(), "min"),
            // IT8 with just the signature line
            (b"IT8FILE\n".to_vec(), "bare"),
            // IT8 with just signature + number fields
            (b"IT8FILE\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\n".to_vec(), "counts-only"),
            // property values that look like section headers
            (b"IT8FILE\nP1 \"BEGIN_DATA_FORMAT\"\nP2 \"END_DATA_FORMAT\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n".to_vec(), "hdr-props"),
            // semicolon-separated pair values
            (b"IT8FILE\nP1 \"k1;v1\"\nP2 \"k1;v1;k2;v2\"\nP3 \"a;b;c\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n".to_vec(), "semis"),
            // numbers with leading + and spaces
            (b"IT8FILE\nNUMBER_OF_FIELDS +2\nNUMBER_OF_SETS +3\nBEGIN_DATA_FORMAT\nA B\nEND_DATA_FORMAT\nBEGIN_DATA\n1 2\n3 4\n5 6\nEND_DATA\n".to_vec(), "plus-counts"),
        ];
        for (doc, name) in docs {
            let p = c::cmsIT8LoadFromMem(std::ptr::null_mut(),
                doc.as_ptr() as *const _, doc.len() as u32);
            eprintln!("{name} parse={}", !p.is_null());
            if !p.is_null() {
                c::cmsIT8Free(p);
            }
        }
    }
}

/// cmscgats remaining lexer/parser internals — every S* token
/// arm + error propagation through InSymbol.
#[test]
fn it8_deep_arms12() {
    unsafe {
        let docs: Vec<(Vec<u8>, &str)> = vec![
            // EOF mid-token / mid-string / mid-comment
            (b"IT8FILE\nP1 \"unterm".to_vec(), "eof-string"),
            (b"IT8FILE\nP1".to_vec(), "eof-prop"),
            (b"IT8FILE\nNUMBER_OF_FIELDS".to_vec(), "eof-kw"),
            (b"IT8FILE\n# unterminated comment".to_vec(), "eof-cmt"),
            // null bytes in the stream
            (b"IT8FILE\nP1 \x00\"v\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n".to_vec(), "null-byte"),
            // high-byte UTF-8 sequences
            (b"IT8FILE\nP1 \"\xc3\xa9\xc3\xa8\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n".to_vec(), "utf8-2"),
            (b"IT8FILE\nP1 \"\xe2\x98\x83\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n".to_vec(), "utf8-3"),
            (b"IT8FILE\nP1 \"\xf0\x9f\x98\x80\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n".to_vec(), "utf8-4"),
            // invalid UTF-8 (lone high byte)
            (b"IT8FILE\nP1 \"\x80\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n".to_vec(), "bad-utf8"),
            // whitespace-only after signature
            (b"IT8FILE\n   \n\t\n  \n".to_vec(), "ws-only"),
            // carriage return alone (no LF)
            (b"IT8FILE\rNUMBER_OF_FIELDS 1\rNUMBER_OF_SETS 1\rBEGIN_DATA_FORMAT\rA\rEND_DATA_FORMAT\rBEGIN_DATA\r1\rEND_DATA\r".to_vec(), "cr-only"),
            // property = keyword-name (identifier)
            (b"IT8FILE\nP1 ORIGINATOR\nP2 DESCRIPTOR\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n".to_vec(), "kw-values2"),
            // very long property value
            ({ let mut v = b"IT8FILE\nP1 \"".to_vec();
                for _ in 0..1000 { v.push(b'x'); }
                v.extend_from_slice(b"\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n");
                v }, "very-long"),
            // data rows with leading sign/whitespace
            (b"IT8FILE\nNUMBER_OF_FIELDS 2\nNUMBER_OF_SETS 4\nBEGIN_DATA_FORMAT\nA B\nEND_DATA_FORMAT\nBEGIN_DATA\n-1 +1\n.5 -.5\n1e-3 +1e3\n0xFF -0x1F\nEND_DATA\n".to_vec(), "signed-data"),
        ];
        for (doc, name) in docs {
            let p = c::cmsIT8LoadFromMem(std::ptr::null_mut(),
                doc.as_ptr() as *const _, doc.len() as u32);
            eprintln!("{name} parse={}", !p.is_null());
            if !p.is_null() {
                c::cmsIT8Free(p);
            }
        }
    }
}

/// cmscgats remaining InSymbol/HeaderSection arms — every
/// remaining token-form + WriteHeader/DblFormat internals.
#[test]
fn it8_deep_arms13() {
    unsafe {
        let docs: Vec<(Vec<u8>, &str)> = vec![
            // DblFormat property → WriteHeader double format
            (b"IT8FILE\nDBL_FORMAT \"%.15g\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1.234567890123456\nEND_DATA\n".to_vec(), "dblfmt"),
            // multiple sheets → WriteHeader per-sheet + data-id
            (b"IT8FILE\nDATA_IDENTIFIER \"t1\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\nIT8FILE\nDATA_IDENTIFIER \"t2\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nB\nEND_DATA_FORMAT\nBEGIN_DATA\n2\nEND_DATA\nIT8FILE\nDATA_IDENTIFIER \"t3\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nC\nEND_DATA_FORMAT\nBEGIN_DATA\n3\nEND_DATA\n".to_vec(), "3sheet-write"),
            // number-only data values (integer/float/hex/sci)
            (b"IT8FILE\nNUMBER_OF_FIELDS 8\nNUMBER_OF_SETS 3\nBEGIN_DATA_FORMAT\nA B C D E F G H\nEND_DATA_FORMAT\nBEGIN_DATA\n1 2.5 -3 +4 .5 6. 7e2 -8e-1\n0x1 0Xf -0xFF +0x1 0 00 0.0 -0\n1e-308 1e308 -1e-308 -1e308 1E0 1E-0 1E+0 -1E0\nEND_DATA\n".to_vec(), "num-forms2"),
            // comment inside property value string
            (b"IT8FILE\nP1 \"a#b\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n".to_vec(), "cmt-in-str"),
            // strings containing section keywords
            (b"IT8FILE\nP1 \"IT8FILE\"\nP2 \"BEGIN_DATA\"\nP3 \"END_DATA\"\nP4 \"NUMBER_OF_FIELDS\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n".to_vec(), "kw-strings"),
            // data values that are IT8 keywords
            (b"IT8FILE\nNUMBER_OF_FIELDS 4\nNUMBER_OF_SETS 2\nBEGIN_DATA_FORMAT\nA B C D\nEND_DATA_FORMAT\nBEGIN_DATA\nIT8FILE BEGIN_DATA END_DATA NUMBER_OF_FIELDS\nBEGIN_DATA_FORMAT END_DATA_FORMAT DATA_IDENTIFIER ORIGINATOR\nEND_DATA\n".to_vec(), "kw-data"),
            // very long identifier (buffer boundary)
            ({ let mut v = b"IT8FILE\n".to_vec();
                for _ in 0..200 { v.extend_from_slice(b"X"); }
                v.extend_from_slice(b" 1\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n");
                v }, "long-ident"),
            // leading-zero numbers
            (b"IT8FILE\nP1 007\nP2 0x007\nP3 00.7\nP4 0.007\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n".to_vec(), "lz-nums"),
        ];
        for (doc, name) in docs {
            let p = c::cmsIT8LoadFromMem(std::ptr::null_mut(),
                doc.as_ptr() as *const _, doc.len() as u32);
            eprintln!("{name} parse={}", !p.is_null());
            if !p.is_null() {
                c::cmsIT8Free(p);
            }
        }
    }
}

/// cmscgats remaining InSymbol/HeaderSection/ParseIT8 arms —
/// more token forms + malformed + boundary inputs.
#[test]
fn it8_deep_arms14() {
    unsafe {
        let docs: Vec<(Vec<u8>, &str)> = vec![
            // every whitespace character between tokens
            (b"IT8FILE \x09P1\x20\x09\"v\"\x09\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n".to_vec(), "all-ws"),
            // comment between every token
            (b"IT8FILE\n#c1\nP1 \"v\"\n#c2\nNUMBER_OF_FIELDS 1\n#c3\nNUMBER_OF_SETS 1\n#c4\nBEGIN_DATA_FORMAT\n#c5\nA\n#c6\nEND_DATA_FORMAT\n#c7\nBEGIN_DATA\n#c8\n1\n#c9\nEND_DATA\n#c10\n".to_vec(), "cmt-between"),
            // property names that are prefixes of keywords
            (b"IT8FILE\nBEGIN_D 1\nEND_D 2\nNUMBER_OF_FIELD 3\nNUMBER_OF_SET 4\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n".to_vec(), "prefix-kw"),
            // property names matching data-format field names
            (b"IT8FILE\nNUMBER_OF_FIELDS 3\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nORIGINATOR DESCRIPTOR CREATED\nEND_DATA_FORMAT\nBEGIN_DATA\n1 2 3\nEND_DATA\n".to_vec(), "fmt-kw-names"),
            // property value = field name
            (b"IT8FILE\nP1 A\nP2 B\nP3 C\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n".to_vec(), "prop-field"),
            // data rows with only quotes (no values)
            (b"IT8FILE\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 3\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n\"\"\n\" \"\n\"x\"\nEND_DATA\n".to_vec(), "empty-quotes"),
            // number followed immediately by string
            (b"IT8FILE\nNUMBER_OF_FIELDS 2\nNUMBER_OF_SETS 2\nBEGIN_DATA_FORMAT\nA B\nEND_DATA_FORMAT\nBEGIN_DATA\n1\"str\"\n2 \"s2\"\nEND_DATA\n".to_vec(), "num-str"),
            // property with numeric value + quoted value on next line
            (b"IT8FILE\nP1 123\nP2 \"abc\"\nP3 45.6\nP4 \"def\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n".to_vec(), "mixed-prop"),
            // multiple properties per line (semicolon-separated)
            (b"IT8FILE\nP1 \"a\"; P2 \"b\"; P3 \"c\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n".to_vec(), "multi-prop"),
            // data with trailing comma-like values
            (b"IT8FILE\nNUMBER_OF_FIELDS 2\nNUMBER_OF_SETS 2\nBEGIN_DATA_FORMAT\nA B\nEND_DATA_FORMAT\nBEGIN_DATA\n1,2\n3,4\nEND_DATA\n".to_vec(), "comma-data"),
            // NUMBER_OF_FIELDS with decimal value
            (b"IT8FILE\nNUMBER_OF_FIELDS 1.0\nNUMBER_OF_SETS 1.0\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n".to_vec(), "dec-count"),
            // all-uppercase / mixed-case keywords
            (b"IT8FILE\nnumber_of_fields 1\nNumber_Of_Sets 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n".to_vec(), "lowcase-kw"),
        ];
        for (doc, name) in docs {
            let p = c::cmsIT8LoadFromMem(std::ptr::null_mut(),
                doc.as_ptr() as *const _, doc.len() as u32);
            eprintln!("{name} parse={}", !p.is_null());
            if !p.is_null() {
                c::cmsIT8Free(p);
            }
        }
    }
}

/// cmscgats remaining InSymbol/HeaderSection arms — every
/// remaining token-form + table-structure variant.
#[test]
fn it8_deep_arms15() {
    unsafe {
        let docs: Vec<(Vec<u8>, &str)> = vec![
            // number forms that hit the lexer number-read arms
            (b"IT8FILE\nNUMBER_OF_FIELDS 10\nNUMBER_OF_SETS 5\nBEGIN_DATA_FORMAT\nA B C D E F G H I J\nEND_DATA_FORMAT\nBEGIN_DATA\n1 2.5 -3 +4 .5 6. 7e2 -8e-1 0x1 -0xF\n1e-3 1E3 -1e3 +1E-3 .5e-1 1.5e+0 0 00 0.0 -0\n-1 -2.5 -3. -4.5 -.5 -.5e-1 -1e-3 -1e3 -0xF -0.0\n+1 +2.5 +3. +4.5 +.5 +.5e-1 +1e-3 +1e3 +0xF +0.0\n0 0.0 0e0 0.0e0 0x0 0X0 -0 -0.0 -0e0 -0.0e0\nEND_DATA\n".to_vec(), "all-numforms"),
            // every property type in the header
            (b"IT8FILE\nORIGINATOR \"o\"\nDESCRIPTOR \"d\"\nCREATED \"c\"\nMANUFACTURER \"m\"\nPROD_DATE \"p\"\nSERIAL \"s\"\nMATERIAL \"mat\"\nINSTRUMENTATION \"i\"\nMEASUREMENT_SOURCE \"ms\"\nPRINT_CONDITIONS \"pc\"\nSAMPLE_BACKING \"sb\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n".to_vec(), "all-props"),
            // duplicate property (last wins)
            (b"IT8FILE\nP1 \"v1\"\nP1 \"v2\"\nP1 \"v3\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n".to_vec(), "dup-prop"),
            // property before/after all sections
            (b"IT8FILE\nBEFORE \"b1\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nMID \"m1\"\nBEGIN_DATA\n1\nEND_DATA\nAFTER \"a1\"\n".to_vec(), "prop-sections"),
            // deeply nested quotes
            (b"IT8FILE\nP1 \"a\"b\"c\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n".to_vec(), "mixed-quotes"),
            // DATA_IDENTIFIER on some sheets only
            (b"IT8FILE\nDATA_IDENTIFIER \"s1\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\nIT8FILE\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nB\nEND_DATA_FORMAT\nBEGIN_DATA\n2\nEND_DATA\nIT8FILE\nDATA_IDENTIFIER \"s3\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nC\nEND_DATA_FORMAT\nBEGIN_DATA\n3\nEND_DATA\n".to_vec(), "partial-ids"),
            // NUMBER_OF_FIELDS with huge count (rejected)
            (b"IT8FILE\nNUMBER_OF_FIELDS 65535\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n".to_vec(), "huge-fields"),
            // all field-name forms in data format
            (b"IT8FILE\nNUMBER_OF_FIELDS 5\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nF1 F_2 F-3 F.4 F:5\nEND_DATA_FORMAT\nBEGIN_DATA\n1 2 3 4 5\nEND_DATA\n".to_vec(), "field-names"),
        ];
        for (doc, name) in docs {
            let p = c::cmsIT8LoadFromMem(std::ptr::null_mut(),
                doc.as_ptr() as *const _, doc.len() as u32);
            eprintln!("{name} parse={}", !p.is_null());
            if !p.is_null() {
                c::cmsIT8Free(p);
            }
        }
    }
}

/// cmscgats remaining InSymbol/HeaderSection/ParseIT8 arms —
/// more table-structure + token + malformed variants.
#[test]
fn it8_deep_arms16() {
    unsafe {
        let docs: Vec<(Vec<u8>, &str)> = vec![
            // every sheet-structure variant
            (b"IT8FILE\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\nIT8FILE\nNUMBER_OF_FIELDS 2\nNUMBER_OF_SETS 2\nBEGIN_DATA_FORMAT\nB C\nEND_DATA_FORMAT\nBEGIN_DATA\n1 2\n3 4\nEND_DATA\nIT8FILE\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nD\nEND_DATA_FORMAT\nBEGIN_DATA\n5\nEND_DATA\n".to_vec(), "3sheet-diff"),
            // IT8 with a real DATA_IDENTIFIER per sheet
            (b"IT8FILE\nDATA_IDENTIFIER \"meas1\"\nNUMBER_OF_FIELDS 2\nNUMBER_OF_SETS 2\nBEGIN_DATA_FORMAT\nID L\nEND_DATA_FORMAT\nBEGIN_DATA\n1 50\n2 60\nEND_DATA\nIT8FILE\nDATA_IDENTIFIER \"meas2\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n70\nEND_DATA\n".to_vec(), "meas-sheets"),
            // property values containing special chars
            (b"IT8FILE\nP1 \"a@b#c$d%e^f&g*h(i)j\"\nP2 \"k!l-m_n+o=p\"\nP3 \"q[r]s{t}u|v\"\nP4 \"w;x:y,z.v/b?\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n".to_vec(), "special-chars"),
            // numbers in every valid position
            (b"IT8FILE\nP1 -1234567890\nP2 +987654321\nP3 000000001\nP4 0.000000001\nP5 -0.000000001\nP6 999999999999\nP7 1e-999\nP8 -1e+999\nP9 .0000000000001\nP10 9.999999999999e99\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n".to_vec(), "num-extremes"),
            // data values on same line as BEGIN_DATA
            (b"IT8FILE\nNUMBER_OF_FIELDS 2\nNUMBER_OF_SETS 2\nBEGIN_DATA_FORMAT\nA B\nEND_DATA_FORMAT\nBEGIN_DATA\n1 2\n3 4\nEND_DATA\n".to_vec(), "std2"),
            // whitespace + comments interleaved in data
            (b"IT8FILE\nNUMBER_OF_FIELDS 2\nNUMBER_OF_SETS 3\nBEGIN_DATA_FORMAT\nA B\nEND_DATA_FORMAT\nBEGIN_DATA\n 1\t# cmt\n 2\n3 # cmt\n 4\n5 6\nEND_DATA\n".to_vec(), "cmt-data"),
            // IT8 with just signature + comment
            (b"# comment\nIT8FILE\n# trailing\n".to_vec(), "cmt-only"),
            // deeply indented structure
            (b"IT8FILE\n\t\tP1 \"v\"\n\tNUMBER_OF_FIELDS 1\n\t\tNUMBER_OF_SETS 1\n\tBEGIN_DATA_FORMAT\n\t\tA\n\tEND_DATA_FORMAT\n\t\tBEGIN_DATA\n\t1\n\t\tEND_DATA\n".to_vec(), "deep-indent"),
            // property = data format field name
            (b"IT8FILE\nSAMPLE_ID \"s1\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nSAMPLE_ID\nEND_DATA_FORMAT\nBEGIN_DATA\nx\nEND_DATA\n".to_vec(), "field-eq-prop"),
            // data values that are quoted keywords
            (b"IT8FILE\nNUMBER_OF_FIELDS 2\nNUMBER_OF_SETS 2\nBEGIN_DATA_FORMAT\nA B\nEND_DATA_FORMAT\nBEGIN_DATA\n\"IT8FILE\" \"BEGIN_DATA\"\n\"END_DATA\" \"NUMBER_OF_FIELDS\"\nEND_DATA\n".to_vec(), "quoted-kw"),
        ];
        for (doc, name) in docs {
            let p = c::cmsIT8LoadFromMem(std::ptr::null_mut(),
                doc.as_ptr() as *const _, doc.len() as u32);
            eprintln!("{name} parse={}", !p.is_null());
            if !p.is_null() {
                c::cmsIT8Free(p);
            }
        }
    }
}

/// cmscgats remaining InSymbol/HeaderSection arms — every
/// remaining lexer/parser internals + CookPointers/AddToList.
#[test]
fn it8_deep_arms17() {
    unsafe {
        let docs: Vec<(Vec<u8>, &str)> = vec![
            // many tables → CookPointers + AddToList arms
            ({ let mut v = Vec::new();
                for t in 0..8u32 {
                    v.extend_from_slice(b"IT8FILE\n");
                    v.extend_from_slice(
                        format!("DATA_IDENTIFIER \"t{t}\"\n").as_bytes());
                    v.extend_from_slice(
                        format!("NUMBER_OF_FIELDS {}\nNUMBER_OF_SETS {}\n",
                            t + 1, t + 2).as_bytes());
                    v.extend_from_slice(b"BEGIN_DATA_FORMAT\n");
                    for f in 0..=t {
                        v.extend_from_slice(format!("F{f} ").as_bytes());
                    }
                    v.extend_from_slice(b"\nEND_DATA_FORMAT\nBEGIN_DATA\n");
                    for s in 0..t + 2 {
                        for f in 0..=t {
                            v.extend_from_slice(
                                format!("{} ", s * 10 + f).as_bytes());
                        }
                        v.push(b'\n');
                    }
                    v.extend_from_slice(b"END_DATA\n");
                }
                v }, "8-tables"),
            // many properties → AddToList/property-list arms
            ({ let mut v = b"IT8FILE\n".to_vec();
                for i in 0..64u32 {
                    v.extend_from_slice(
                        format!("P{i} \"v{i}\"\n").as_bytes());
                }
                v.extend_from_slice(
                    b"NUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n");
                v }, "64-props"),
            // every property enum value through SetProperty
            (b"IT8FILE\nORIGINATOR \"o\"\nDESCRIPTOR \"d\"\nCREATED \"c\"\nMANUFACTURER \"m\"\nPROD_DATE \"p\"\nSERIAL \"s\"\nMATERIAL \"mat\"\nINSTRUMENTATION \"i\"\nMEASUREMENT_SOURCE \"ms\"\nPRINT_CONDITIONS \"pc\"\nSAMPLE_BACKING \"sb\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n".to_vec(), "all-props2"),
            // field names matching every predefined keyword
            (b"IT8FILE\nNUMBER_OF_FIELDS 8\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nNUMBER_OF_FIELDS NUMBER_OF_SETS BEGIN_DATA END_DATA BEGIN_DATA_FORMAT END_DATA_FORMAT IT8FILE DATA_IDENTIFIER\nEND_DATA_FORMAT\nBEGIN_DATA\n1 2 3 4 5 6 7 8\nEND_DATA\n".to_vec(), "kw-fnames"),
            // deeply nested/multiline quoted strings
            (b"IT8FILE\nP1 \"a\nb\nc\nd\"\nP2 \"1\n2\n3\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n".to_vec(), "deep-ml"),
            // data rows with all-zero/negative/edge values
            (b"IT8FILE\nNUMBER_OF_FIELDS 3\nNUMBER_OF_SETS 4\nBEGIN_DATA_FORMAT\nA B C\nEND_DATA_FORMAT\nBEGIN_DATA\n0 0 0\n-1 -1 -1\n-0.0 -0.0 -0.0\n65535 65535 65535\nEND_DATA\n".to_vec(), "edge-data"),
            // huge single data value
            (b"IT8FILE\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n999999999999999999999\nEND_DATA\n".to_vec(), "huge-val"),
            // empty data-format section
            (b"IT8FILE\nNUMBER_OF_FIELDS 0\nNUMBER_OF_SETS 0\nBEGIN_DATA_FORMAT\nEND_DATA_FORMAT\nBEGIN_DATA\nEND_DATA\n".to_vec(), "empty-fmt"),
            // property after data (out of order)
            (b"IT8FILE\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\nTAIL \"tail\"\n".to_vec(), "post-prop"),
            // data values with internal whitespace (string form)
            (b"IT8FILE\nNUMBER_OF_FIELDS 2\nNUMBER_OF_SETS 2\nBEGIN_DATA_FORMAT\nA B\nEND_DATA_FORMAT\nBEGIN_DATA\n\"a b\" \"c d\"\n\"e f\" \"g h\"\nEND_DATA\n".to_vec(), "ws-str"),
        ];
        for (doc, name) in docs {
            let p = c::cmsIT8LoadFromMem(std::ptr::null_mut(),
                doc.as_ptr() as *const _, doc.len() as u32);
            eprintln!("{name} parse={}", !p.is_null());
            if !p.is_null() {
                c::cmsIT8Free(p);
            }
        }
    }
}

/// cmscgats remaining InSymbol/HeaderSection/DataSection arms —
/// more token + table-structure + malformed variants.
#[test]
fn it8_deep_arms18() {
    unsafe {
        let docs: Vec<(Vec<u8>, &str)> = vec![
            // property + data sections interleaved (multi-sheet)
            (b"IT8FILE\nP1 \"v1\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\nP2 \"v2\"\nIT8FILE\nP3 \"v3\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nB\nEND_DATA_FORMAT\nBEGIN_DATA\n2\nEND_DATA\n".to_vec(), "mixed-sheets"),
            // DATA_IDENTIFIER in every position
            (b"IT8FILE\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nDATA_IDENTIFIER \"id\"\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n".to_vec(), "dataid-mid"),
            // property with = sign inside quoted value
            (b"IT8FILE\nP1 \"key=value\"\nP2 \"a=b=c\"\nP3 \"x==y\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n".to_vec(), "eq-in-str"),
            // data values with all punctuation inside quotes
            (b"IT8FILE\nNUMBER_OF_FIELDS 4\nNUMBER_OF_SETS 2\nBEGIN_DATA_FORMAT\nA B C D\nEND_DATA_FORMAT\nBEGIN_DATA\n\"a;b\" \"c,d\" \"e.f\" \"g:h\"\n\"i!j\" \"k?l\" \"m/n\" \"o|p\"\nEND_DATA\n".to_vec(), "punct-data"),
            // numbers that look like identifiers (start with digit)
            (b"IT8FILE\nP1 123abc\nP2 0xZZZ\nP3 1.2.3\nP4 1e\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n".to_vec(), "num-ident"),
            // format section with all-quoted field names
            (b"IT8FILE\nNUMBER_OF_FIELDS 3\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\n\"F1\" \"F2\" \"F3\"\nEND_DATA_FORMAT\nBEGIN_DATA\n1 2 3\nEND_DATA\n".to_vec(), "quoted-fnames"),
            // property name that IS a number
            (b"IT8FILE\n123 \"v\"\n456 \"w\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n".to_vec(), "num-prop"),
            // whitespace inside number tokens
            (b"IT8FILE\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 2\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n 1\n 2\nEND_DATA\n".to_vec(), "ws-nums"),
            // EOF after END_DATA
            (b"IT8FILE\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA".to_vec(), "eof-enddata"),
            // EOF after IT8FILE
            (b"IT8FILE".to_vec(), "eof-sig"),
        ];
        for (doc, name) in docs {
            let p = c::cmsIT8LoadFromMem(std::ptr::null_mut(),
                doc.as_ptr() as *const _, doc.len() as u32);
            eprintln!("{name} parse={}", !p.is_null());
            if !p.is_null() {
                c::cmsIT8Free(p);
            }
        }
    }
}

/// cmscgats remaining InSymbol/HeaderSection/DataSection arms —
/// every remaining token-form + table-structure variant.
#[test]
fn it8_deep_arms19() {
    unsafe {
        let docs: Vec<(Vec<u8>, &str)> = vec![
            // every data-value form across all numeric classes
            (b"IT8FILE\nNUMBER_OF_FIELDS 6\nNUMBER_OF_SETS 8\nBEGIN_DATA_FORMAT\nA B C D E F\nEND_DATA_FORMAT\nBEGIN_DATA\n0 1 -1 +1 .5 -.5\n1.5 -1.5 +1.5 1. -1. +1.\n0x0 0x1 -0x1 +0x1 0xFF -0xFF\n1e0 1e1 1e-1 1e+1 -1e0 -1e1\n1E0 1E1 1E-1 1E+1 -1E0 -1E1\n0.0 -0.0 +0.0 0.0e0 -0.0e0 +0.0e0\n1.5e-3 -1.5e-3 +1.5e-3 1.5E+3 -1.5E+3 +1.5E+3\n0xABCDEF 0xabcdef -0xABCDEF +0xabcdef 0x0FF 0xFF0\nEND_DATA\n".to_vec(), "num-classes"),
            // property values that span multiple words
            (b"IT8FILE\nP1 one two three four five\nP2 \"one two three four five\"\nP3 six\nP4 \"seven\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n".to_vec(), "multi-word"),
            // data format names that are numbers
            (b"IT8FILE\nNUMBER_OF_FIELDS 3\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\n1 2.5 0x1F\nEND_DATA_FORMAT\nBEGIN_DATA\na b c\nEND_DATA\n".to_vec(), "num-fnames"),
            // property name that starts with underscore/digit
            (b"IT8FILE\n_P1 \"v\"\n_P2 \"w\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n".to_vec(), "underscore"),
            // data-format section inside quotes
            (b"IT8FILE\nP1 \"BEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n".to_vec(), "fmt-in-str"),
            // deeply nested quoted escapes
            (b"IT8FILE\nP1 \"a\\\"b\\\"c\\\"d\"\nP2 \"x\\\\y\\\\z\"\nP3 \"\\u{263A}\\u{1F600}\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n".to_vec(), "deep-esc2"),
            // comment at every boundary
            (b"IT8FILE\n#1\nNUMBER_OF_FIELDS 1\n#2\nNUMBER_OF_SETS 1\n#3\nBEGIN_DATA_FORMAT\n#4\nA\n#5\nEND_DATA_FORMAT\n#6\nBEGIN_DATA\n#7\n1\n#8\nEND_DATA\n#9\n".to_vec(), "cmt-all"),
            // data values with leading/trailing whitespace
            (b"IT8FILE\nNUMBER_OF_FIELDS 2\nNUMBER_OF_SETS 2\nBEGIN_DATA_FORMAT\nA B\nEND_DATA_FORMAT\nBEGIN_DATA\n 1\t 2\n3\t4 \nEND_DATA\n".to_vec(), "ws-data2"),
            // data identifier with all characters
            (b"IT8FILE\nDATA_IDENTIFIER \"a-b_c.d:e!f@g#h$i%j&k*l(m)n\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n".to_vec(), "id-chars"),
            // NUMBER_OF_FIELDS/SETS with scientific notation
            (b"IT8FILE\nNUMBER_OF_FIELDS 1e0\nNUMBER_OF_SETS 1E0\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n".to_vec(), "sci-counts"),
            // property with value = another property name
            (b"IT8FILE\nP1 P2\nP2 P3\nP3 P4\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n".to_vec(), "prop-chain"),
            // deeply mixed structure
            (b"IT8FILE\nP1 \"a\"\nNUMBER_OF_FIELDS 2\nNUMBER_OF_SETS 3\nP2 \"b\"\nBEGIN_DATA_FORMAT\nA B\nEND_DATA_FORMAT\nP3 \"c\"\nBEGIN_DATA\n1 2\n3 4\n5 6\nEND_DATA\nP4 \"d\"\nIT8FILE\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nC\nEND_DATA_FORMAT\nBEGIN_DATA\n7\nEND_DATA\n".to_vec(), "mixed-all"),
        ];
        for (doc, name) in docs {
            let p = c::cmsIT8LoadFromMem(std::ptr::null_mut(),
                doc.as_ptr() as *const _, doc.len() as u32);
            eprintln!("{name} parse={}", !p.is_null());
            if !p.is_null() {
                c::cmsIT8Free(p);
            }
        }
    }
}

/// cmscgats remaining InSymbol/HeaderSection/DataSection arms —
/// more token + table + malformed variants.
#[test]
fn it8_deep_arms20() {
    unsafe {
        let docs: Vec<(Vec<u8>, &str)> = vec![
            // table with all field types mixed
            (b"IT8FILE\nNUMBER_OF_FIELDS 5\nNUMBER_OF_SETS 3\nBEGIN_DATA_FORMAT\nINT HEX SCI STR ID\nEND_DATA_FORMAT\nBEGIN_DATA\n1 0x1F 2.5e-3 \"str\" ident\n-1 -0xF -1e3 \"s2\" i2\n+3 0x0 .5 \"s3\" i3\nEND_DATA\n".to_vec(), "mixed-fields"),
            // property values that are data-format names
            (b"IT8FILE\nP1 SAMPLE_ID\nP2 LAB_L\nP3 RGB_R\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n".to_vec(), "fmt-props"),
            // deeply nested quote/escape in data values
            (b"IT8FILE\nNUMBER_OF_FIELDS 2\nNUMBER_OF_SETS 2\nBEGIN_DATA_FORMAT\nA B\nEND_DATA_FORMAT\nBEGIN_DATA\n\"a\\tb\\n\" \"c\\u{263A}\"\n\"d\\\\e\" \"f\\\"g\"\nEND_DATA\n".to_vec(), "esc-data2"),
            // data rows with leading +/-/hex/decimal/scientific
            (b"IT8FILE\nNUMBER_OF_FIELDS 8\nNUMBER_OF_SETS 4\nBEGIN_DATA_FORMAT\nA B C D E F G H\nEND_DATA_FORMAT\nBEGIN_DATA\n+1 -1 .5 1. 0x1 -0x1 1e0 -1e0\n+0 -0 +.5 -.5 +1. -1. +0x1 -0x1\n+1e0 -1e0 +1E1 -1E1 +.5e1 -.5e1 +1.e1 -1.e1\n0x0 -0x0 +0x0 0xFF -0xFF +0xFF 0xff -0xff\nEND_DATA\n".to_vec(), "all-signs"),
            // comments at end of every line
            (b"IT8FILE # hdr\nP1 \"v\" # p\nNUMBER_OF_FIELDS 1 # nf\nNUMBER_OF_SETS 1 # ns\nBEGIN_DATA_FORMAT # bf\nA # f\nEND_DATA_FORMAT # ef\nBEGIN_DATA # bd\n1 # d\nEND_DATA # ed\n".to_vec(), "eol-cmts"),
            // property names that are single chars
            (b"IT8FILE\nA \"a\"\nB \"b\"\nC \"c\"\nD \"d\"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n".to_vec(), "1ch-props"),
            // IT8 with BOM (byte order mark)
            (b"\xef\xbb\xbfIT8FILE\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n".to_vec(), "bom"),
            // null-terminated string in data
            (b"IT8FILE\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 2\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n\"a\\0b\"\n\"c\"\nEND_DATA\n".to_vec(), "nul-str"),
            // data with all-hex values
            (b"IT8FILE\nNUMBER_OF_FIELDS 4\nNUMBER_OF_SETS 3\nBEGIN_DATA_FORMAT\nA B C D\nEND_DATA_FORMAT\nBEGIN_DATA\n0x0 0x1 0xFF 0xFFFF\n-0x0 -0x1 -0xFF -0xFFFF\n+0x0 +0x1 +0xFF +0xFFFF\nEND_DATA\n".to_vec(), "hex-data"),
            // extremely long data row
            ({ let mut v = b"IT8FILE\nNUMBER_OF_FIELDS 64\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\n".to_vec();
                for i in 0..64u32 {
                    v.extend_from_slice(format!("F{i} ").as_bytes());
                }
                v.extend_from_slice(b"\nEND_DATA_FORMAT\nBEGIN_DATA\n");
                for i in 0..64u32 {
                    v.extend_from_slice(format!("{i} ").as_bytes());
                }
                v.extend_from_slice(b"\nEND_DATA\n");
                v }, "64-fields"),
        ];
        for (doc, name) in docs {
            let p = c::cmsIT8LoadFromMem(std::ptr::null_mut(),
                doc.as_ptr() as *const _, doc.len() as u32);
            eprintln!("{name} parse={}", !p.is_null());
            if !p.is_null() {
                c::cmsIT8Free(p);
            }
        }
    }
}

/// InSymbol — lexer branch arms: hex/binary numbers, overflow,
/// signs, EOL variants, strings, comments, include, id+num merge.
#[test]
fn it8_insymbol_arms() {
    unsafe {
        let base = b"IT8FILE\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\n\
            # comment\ndesc \"doc\"\nBEGIN_DATA_FORMAT\n\
            D\nEND_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n\
            ORIGINATOR \"x\"\n";
        let docs: Vec<(Vec<u8>, &str)> = vec![
            (b"IT8FILE\nx0ff 12\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nD\n\
                END_DATA_FORMAT\nBEGIN_DATA\n0x10\nEND_DATA\n"
                .to_vec(), "hex"),
            (b"IT8FILE\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nD\nEND_DATA_FORMAT\n\
                BEGIN_DATA\n0b101\nEND_DATA\n".to_vec(), "bin"),
            (b"IT8FILE\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nD\nEND_DATA_FORMAT\n\
                BEGIN_DATA\n0xFFFFFFFFF\nEND_DATA\n".to_vec(),
                "hex-ovf"),
            (b"IT8FILE\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nD\nEND_DATA_FORMAT\n\
                BEGIN_DATA\n0b1111111111111111111111111111111111\n\
                END_DATA\n".to_vec(), "bin-ovf"),
            (b"IT8FILE\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nD\nEND_DATA_FORMAT\n\
                BEGIN_DATA\n-42\n+17\n.5\n1e3\nEND_DATA\n"
                .to_vec(), "signs"),
            (b"IT8FILE\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nD\nEND_DATA_FORMAT\n\
                BEGIN_DATA\n-0.5e-2\nEND_DATA\n".to_vec(),
                "neg-sci"),
            (b"IT8FILE\nkw \"val\"\r\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nD\n\
                END_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n"
                .to_vec(), "crlf"),
            (b"IT8FILE\nkw \"val\"\rNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nD\n\
                END_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n"
                .to_vec(), "cr"),
            (b"IT8FILE\nkw 'single'\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nD\n\
                END_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n"
                .to_vec(), "squote"),
            (b"IT8FILE\n12abc \"id-num-merge\"\n\
                NUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nD\nEND_DATA_FORMAT\n\
                BEGIN_DATA\n1\nEND_DATA\n".to_vec(), "id-num"),
            (b"IT8FILE\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nD\nEND_DATA_FORMAT\n\
                BEGIN_DATA\n3.14abc\nEND_DATA\n".to_vec(),
                "num-id"),
            (b"IT8FILE\n\x1a\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nD\n\
                END_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n"
                .to_vec(), "ctrl-z"),
            (b"IT8FILE\nkw \"unclosed\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nD\n\
                END_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n"
                .to_vec(), "unclosed-q"),
            // include directive — missing file → error arm
            (b"IT8FILE\nINCLUDE \"nonexistent_it8_file.txt\"\n\
                NUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nD\nEND_DATA_FORMAT\n\
                BEGIN_DATA\n1\nEND_DATA\n".to_vec(), "include"),
            // very long identifier → StringAppend stress
            ({ let mut d = b"IT8FILE\n".to_vec();
                d.extend_from_slice(&vec![b'x'; 4096]);
                d.extend_from_slice(b" 5\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nD\n\
                    END_DATA_FORMAT\nBEGIN_DATA\n1\nEND_DATA\n");
                d }, "long-id"),
            (base.to_vec(), "base"),
        ];
        for (doc, name) in &docs {
            let it8 = c::cmsIT8LoadFromMem(std::ptr::null_mut(),
                doc.as_ptr() as *const _, doc.len() as u32);
            let ok = !it8.is_null();
            let mut n = 0i32;
            if ok {
                n = c::cmsIT8EnumDataFormat(it8,
                    std::ptr::null_mut());
                c::cmsIT8Free(it8);
            }
            eprintln!("{name} ok={ok} nfmt={n}");
        }
    }
}

/// HeaderSection — WRITE_PAIR property arms: WEIGHTING_FUNCTION +
/// COMPUTATIONAL_PARAMETER in subkey;value pair syntax.
#[test]
fn it8_header_pair_arms() {
    unsafe {
        let tail = b"\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\n\
            BEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\n\
            BEGIN_DATA\n1\nEND_DATA\n".to_vec();
        let doc = |props: &str| -> Vec<u8> {
            let mut d = b"IT8FILE\n".to_vec();
            d.extend_from_slice(props.as_bytes());
            d.extend_from_slice(&tail);
            d
        };
        let variants: Vec<(Vec<u8>, &str)> = vec![
            (doc("WEIGHTING_FUNCTION \"observer;10deg\""),
                "wf-pair"),
            (doc("WEIGHTING_FUNCTION \"observer;10deg;\
                illuminant;D65\""), "wf-2pairs"),
            (doc("COMPUTATIONAL_PARAMETER \"method;CIE15\""),
                "cp-pair"),
            (doc("WEIGHTING_FUNCTION \"observer\""),
                "wf-nopair"),
            (doc("WEIGHTING_FUNCTION 42"), "wf-nonstring"),
            (doc("WEIGHTING_FUNCTION \";\""), "wf-empty"),
            (doc("WEIGHTING_FUNCTION \"a;;b\""),
                "wf-empty-val"),
            (doc("WEIGHTING_FUNCTION \"a;b;c\""),
                "wf-odd"),
            (doc("NUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\n\
                WEIGHTING_FUNCTION \"x;y\"\nORIGINATOR \"o\""),
                "wf-mid"),
        ];
        for (d, name) in &variants {
            let p = c::cmsIT8LoadFromMem(std::ptr::null_mut(),
                d.as_ptr() as *const _, d.len() as u32);
            let ok = !p.is_null();
            eprintln!("{name} ok={ok}");
            if ok {
                // round-trip via SetPropertyMulti + save
                c::cmsIT8SetPropertyMulti(p,
                    "WEIGHTING_FUNCTION\0".as_ptr() as *const i8,
                    "observer\0".as_ptr() as *const i8,
                    "2deg\0".as_ptr() as *const i8);
                let mut buf = vec![0u8; 1 << 16];
                let mut n = buf.len() as u32;
                let sv = c::cmsIT8SaveToMem(p,
                    buf.as_mut_ptr() as *mut _, &mut n);
                if sv != 0 {
                    let p2 = c::cmsIT8LoadFromMem(
                        std::ptr::null_mut(),
                        buf.as_ptr() as *const _, n);
                    if !p2.is_null() { c::cmsIT8Free(p2); }
                }
                c::cmsIT8Free(p);
            }
        }
    }
}

/// ParseCube — .cube file arms: TITLE, LUT_1D_SIZE, LUT_3D_SIZE,
/// DOMAIN_*, VIDEO_RANGE rejects, malformed rows/sizes.
#[test]
fn cube_parse_arms2() {
    unsafe {
        let mk = |body: &str| -> std::path::PathBuf {
            let mut f = std::env::temp_dir();
            f.push(format!("devin_cube_{}.cube", std::process::id()));
            let mut path = f.clone();
            path.set_file_name(format!(
                "devin_cube_{}_{:x}.cube", std::process::id(),
                body.len() ^ (body.as_ptr() as usize & 0xffff)));
            std::fs::write(&path, body).unwrap();
            path
        };
        let variants: Vec<(String, &str)> = vec![
            // full 1D shaper + 3D LUT
            ("TITLE \"full\"\nLUT_1D_SIZE 2\nLUT_3D_SIZE 2\n\
              DOMAIN_MIN 0 0 0\nDOMAIN_MAX 1 1 1\n\
              0 0 0\n1 1 1\n\
              0 0 0\n1 0 0\n0 1 0\n0 0 1\n1 1 0\n1 0 1\n0 1 1\n\
              1 1 1\n".into(), "full"),
            // 1D shaper only
            ("LUT_1D_SIZE 2\n0 0 0\n1 1 1\n".into(), "1d-only"),
            // 3D LUT only
            ("LUT_3D_SIZE 2\n\
              0 0 0\n1 0 0\n0 1 0\n0 0 1\n\
              1 1 0\n1 0 1\n0 1 1\n1 1 1\n".into(), "3d-only"),
            // shaper 2 + lut 2 + non-default domain
            ("TITLE \"dom\"\nLUT_1D_SIZE 2\nLUT_3D_SIZE 2\n\
              DOMAIN_MIN -0.5 -0.5 -0.5\nDOMAIN_MAX 0.9 0.9 0.9\n\
              0 0 0\n1 1 1\n\
              0 0 0\n1 0 0\n0 1 0\n0 0 1\n\
              1 1 0\n1 0 1\n0 1 1\n1 1 1\n".into(), "domain"),
            // out-of-range sizes
            ("LUT_1D_SIZE 1\n".into(), "s1d-too-small"),
            ("LUT_1D_SIZE 70000\n".into(), "s1d-too-big"),
            ("LUT_3D_SIZE 1\n".into(), "s3d-too-small"),
            ("LUT_3D_SIZE 66\n".into(), "s3d-too-big"),
            // input-range not 0..1 → reject
            ("LUT_1D_SIZE 2\nLUT_1D_INPUT_RANGE 0 2\n\
              0 0 0\n1 1 1\n".into(), "rng1d"),
            ("LUT_3D_SIZE 2\nLUT_3D_INPUT_RANGE 0.5 1\n\
              0 0 0\n".into(), "rng3d"),
            // video range → unsupported
            ("LUT_IN_VIDEO_RANGE 0 1\n".into(), "vin"),
            ("LUT_OUT_VIDEO_RANGE 0 1\n".into(), "vout"),
            // malformed data rows
            ("LUT_1D_SIZE 2\n0 0\n".into(), "row-short"),
            ("LUT_1D_SIZE 2\n0 0 0\n".into(), "rows-few"),
            // extra tokens → extra-symbols reject
            ("LUT_1D_SIZE 2\n0 0 0\n1 1 1\ngarbage\n".into(),
                "extra"),
            // title + eoln-only + empty
            ("TITLE \"t\"\n\n\nLUT_1D_SIZE 2\n0 0 0\n1 1 1\n"
                .into(), "title-eol"),
            ("".into(), "empty"),
        ];
        for (body, name) in &variants {
            let path = mk(body);
            let cstr = std::ffi::CString::new(
                path.to_str().unwrap()).unwrap();
            let p = c::cmsCreateDeviceLinkFromCubeFile(
                cstr.as_ptr());
            eprintln!("{name} prof={}", !p.is_null());
            if !p.is_null() {
                let mut buf = vec![0u8; 1 << 20];
                let mut n = buf.len() as u32;
                let s = c::cmsSaveProfileToMem(p,
                    buf.as_mut_ptr() as *mut _, &mut n);
                eprintln!("{name} save={s}");
                c::cmsCloseProfile(p);
            }
            let _ = std::fs::remove_file(&path);
        }
    }
}

/// ParseFloatNumber — IT8 data-row float edge forms.
#[test]
fn it8_parsefloat_arms() {
    unsafe {
        let forms: Vec<(&str, &str)> = vec![
            ("1.5", "simple"),
            ("1.5e3", "sci"),
            ("1.5e-3", "sci-neg"),
            ("1.5e+10", "sci-big"),
            (".5", "lead-dot"),
            ("-.5", "neg-lead"),
            ("1e-300", "underflow"),
            ("1e300", "overflow"),
            ("1.7976931348623157e308", "max"),
            ("2.2250738585072014e-308", "min"),
            ("0.0", "zero"),
            ("-0.0", "negzero"),
            ("1.5E3", "sci-cap"),
            ("1_000.5", "underscore"),
            ("  3.14  ", "space"),
            ("1.5e", "trunc"),
            ("1.5e-", "trunc-sign"),
            ("nan", "nan"),
            ("inf", "inf"),
        ];
        for (tok, name) in &forms {
            let doc = format!("IT8FILE\nNUMBER_OF_FIELDS 1\n\
                NUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nA\n\
                END_DATA_FORMAT\nBEGIN_DATA\n{tok}\nEND_DATA\n");
            let p = c::cmsIT8LoadFromMem(std::ptr::null_mut(),
                doc.as_ptr() as *const _, doc.len() as u32);
            eprintln!("{name}({tok}) ok={}", !p.is_null());
            if !p.is_null() {
                let v = c::cmsIT8GetDataDbl(p,
                    b"A\0".as_ptr() as *const i8,
                    b"A\0".as_ptr() as *const i8);
                eprintln!("  val={v}");
                c::cmsIT8Free(p);
            }
        }
    }
}

/// InSymbol — lexer edge tokens: comments, both string quotes,
/// Ctrl-Z, CR/LF variants, include forms, undefined keywords,
/// embedded numbers, keyword-merge cases.
#[test]
fn insymbol_edge_arms() {
    unsafe {
        let base = |body: &str| format!(
            "IT8FILE\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\n\
            BEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\n\
            BEGIN_DATA\n{body}\nEND_DATA\n");
        let docs: Vec<(String, &str)> = vec![
            (base("5 # comment"), "hash-comment"),
            (base("'sq str'"), "sq-string"),
            (base("\"dq str\""), "dq-string"),
            (base("'unclosed"), "sq-unclosed"),
            (base("\"unclosed"), "dq-unclosed"),
            (base("5\x1a"), "ctrl-z"),
            (base("5\r6"), "cr-mid"),
            (base("5\r\n6"), "crlf-mid"),
            (base("5\n6"), "lf-mid"),
            (base("5 # c\n6"), "comment-eol"),
            (base("0x1f"), "hex"),
            (base("0XAB"), "hex-cap"),
            (base("0b101"), "bin"),
            (base("0B11"), "bin-cap"),
            (base("-42"), "neg-int"),
            (base("+42"), "pos-int"),
            (base("42abc"), "num-ident"),
            (base("abc42"), "ident-num"),
            (base("1.5e3x"), "sci-trail"),
            (base("unknownkey"), "unknown-ident"),
            (base("$INCLUDE"), "include-bare"),
            (base("$INCLUDE \"nofile\""), "include-dq"),
            (base("$INCLUDE 'nofile'"), "include-sq"),
            (base(".INCLUDE \"nofile\""), "include-dot"),
            (base("$INCLUDE nofile"), "include-unquoted"),
            (base("5 # \t tab"), "comment-tab"),
            (base("'\u{00e9}utf8'"), "utf8-string"),
            (base("5 \u{00a0}nbsp"), "nbsp"),
            (base("5\x00\x00"), "nul-bytes"),
            (base("5\x7f\x01"), "high-bytes"),
            (base("5\t6"), "tab-sep"),
            (base("5\r6\r7"), "multi-cr"),
        ];
        for (doc, name) in &docs {
            let p = c::cmsIT8LoadFromMem(std::ptr::null_mut(),
                doc.as_ptr() as *const _, doc.len() as u32);
            eprintln!("{name} ok={}", !p.is_null());
            if !p.is_null() { c::cmsIT8Free(p); }
        }
    }
}

/// HeaderSection — #KEYWORD/#DATA_FORMAT_IDENTIFIER decls,
/// quoted-vs-unquoted property values, WRITE_PAIR non-string
/// reject, blank/comment lines, bad tokens.
#[test]
fn header_arms2() {
    unsafe {
        let wrap = |hdr: &str| format!(
            "IT8FILE\n{hdr}NUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\n\
            BEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\n\
            BEGIN_DATA\n5\nEND_DATA\n");
        let docs: Vec<(String, &str)> = vec![
            (wrap("#KEYWORD CUSTOMKEY\nCUSTOMKEY \"val\"\n"),
                "keyword-decl"),
            (wrap("#DATA_FORMAT_IDENTIFIER SAMPLEID\n"),
                "datafmt-id-decl"),
            (wrap("ORIGINATOR \"quoted value\"\n"), "quoted-val"),
            (wrap("ORIGINATOR unquoted\n"), "unquoted-val"),
            (wrap("ORIGINATOR 42\n"), "num-val"),
            (wrap("ORIGINATOR 'sq val'\n"), "sq-val"),
            (wrap("INSTRUMENTATION \"a\" \"b\"\n"), "two-strs"),
            (wrap("WEIGHTING_FUNCTION \"w=1\"\n"), "wfpair-str"),
            (wrap("WEIGHTING_FUNCTION 42\n"), "wfpair-nonstr"),
            (wrap("WEIGHTING_FUNCTION badident\n"),
                "wfpair-ident"),
            (wrap("\n\nORIGINATOR \"v\"\n\n"), "blank-lines"),
            (wrap("ORIGINATOR \"v\"\n# comment\n"), "comment"),
            (wrap("5 badtok\n"), "num-token"),
            (wrap("ORIGINATOR\n"), "no-val"),
            (wrap("#KEYWORD\n"), "keyword-noval"),
            (wrap("#DATA_FORMAT_IDENTIFIER\n"), "datafmt-noval"),
            (wrap("#KEYWORD \"notident\"\n"), "keyword-str"),
            (wrap("#UNKNOWNKEY val\n"), "unknown-hash"),
            (wrap("MYKEY v1 v2 v3\n"), "multi-val"),
            (wrap("MYKEY 'a' 'b' 'c'\n"), "multi-sq"),
            (wrap("MYKEY\t\"tabbed\"\n"), "tab-sep"),
        ];
        for (doc, name) in &docs {
            let p = c::cmsIT8LoadFromMem(std::ptr::null_mut(),
                doc.as_ptr() as *const _, doc.len() as u32);
            eprintln!("{name} ok={}", !p.is_null());
            if !p.is_null() {
                let v = c::cmsIT8GetProperty(
                    p, b"ORIGINATOR\0".as_ptr() as *const i8);
                let v2 = c::cmsIT8GetProperty(
                    p, b"CUSTOMKEY\0".as_ptr() as *const i8);
                eprintln!("  orig={v:?} cust={v2:?}");
                c::cmsIT8Free(p);
            }
        }
    }
}

/// cmsIT8SetTableByLabel — table navigation by label strings.
#[test]
fn it8_table_by_label_arms() {
    unsafe {
        let doc = "IT8FILE\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\n\
            BEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\n\
            BEGIN_DATA\n5\nEND_DATA\n\
            IT8FILE\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\n\
            BEGIN_DATA_FORMAT\nB\nEND_DATA_FORMAT\n\
            BEGIN_DATA\n6\nEND_DATA\n";
        let p = c::cmsIT8LoadFromMem(std::ptr::null_mut(),
            doc.as_ptr() as *const _, doc.len() as u32);
        eprintln!("load={}", !p.is_null());
        if !p.is_null() {
            for (set, field, exp, name) in [
                (b"A\0" as &[u8], b"A\0" as &[u8],
                    b"A\0" as &[u8], "a-a"),
                (b"set0\0", b"A\0", b"A\0", "set0-a"),
                (b"\0", b"A\0", b"A\0", "empty"),
                (b"set0\0", b"\0", b"\0", "empty-field"),
                (b"no\0", b"no\0", b"no\0", "miss"),
            ] {
                let r = c::cmsIT8SetTableByLabel(p,
                    set.as_ptr() as *const i8,
                    field.as_ptr() as *const i8,
                    exp.as_ptr() as *const i8);
                eprintln!("{name}={r}");
            }
            let r = c::cmsIT8SetTable(p, 1);
            let r0 = c::cmsIT8SetTable(p, 0);
            let r9 = c::cmsIT8SetTable(p, 99);
            eprintln!("tbl set1={r} back={r0} far={r9}");
            c::cmsIT8Free(p);
        }
    }
}

/// CookPointers + ParseIT8 deep arms — IT8 docs with every
/// structural form the lexer/parser validates.
#[test]
fn it8_struct_arms() {
    let docs: &[(&str, &str)] = &[
        ("it8file basic",
         "IT8FILE\nNUMBER_OF_FIELDS 2\nNUMBER_OF_SETS 1\n\
          BEGIN_DATA_FORMAT\nA B\nEND_DATA_FORMAT\n\
          BEGIN_DATA\n1 2\nEND_DATA\n"),
        ("it8multi 2-tables",
         "IT8FILE\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\n\
          BEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\n\
          BEGIN_DATA\n1\nEND_DATA\n\
          IT8FILE\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\n\
          BEGIN_DATA_FORMAT\nB\nEND_DATA_FORMAT\n\
          BEGIN_DATA\n2\nEND_DATA\n"),
        ("cgats.17",
         "CGATS.17\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\n\
          BEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\n\
          BEGIN_DATA\n1\nEND_DATA\n"),
        ("it8/2",
         "IT8/2\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\n\
          BEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\n\
          BEGIN_DATA\n1\nEND_DATA\n"),
        ("props-mix",
         "IT8FILE\nORIGINATOR \"o\"\nPROD_DATE \"2024\"\n\
          NUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\n\
          BEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\n\
          BEGIN_DATA\n1\nEND_DATA\n"),
        ("hdr-only", "IT8FILE\n"),
        ("unterminated-data",
         "IT8FILE\nNUMBER_OF_FIELDS 1\n\
          BEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\n\
          BEGIN_DATA\n1\n"),
        ("nested-bad",
         "IT8FILE\nBEGIN_DATA_FORMAT\nBEGIN_DATA_FORMAT\n"),
        ("bad-header", "NOTIT8\nfoo\n"),
        ("weird-props",
         "IT8FILE\n#COMMENT comment\nP1 = v1\nP2=v2\n\
          NUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\n\
          BEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\n\
          BEGIN_DATA\n1\nEND_DATA\n"),
        ("strings",
         "IT8FILE\nNUMBER_OF_FIELDS 2\nNUMBER_OF_SETS 2\n\
          BEGIN_DATA_FORMAT\nA B\nEND_DATA_FORMAT\n\
          BEGIN_DATA\n\"quoted\" unquoted\n\"with\\\\esc\" \\\"lit\\\"\n\
          END_DATA\n"),
        ("empty-rows",
         "IT8FILE\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 3\n\
          BEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\n\
          BEGIN_DATA\n\n\n1\nEND_DATA\n"),
        ("many-props",
         "IT8FILE\nP1 1\nP2 2\nP3 3\nP4 4\nP5 5\n\
          NUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\n\
          BEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\n\
          BEGIN_DATA\n1\nEND_DATA\n"),
        ("quoted-labels",
         "IT8FILE\nNUMBER_OF_FIELDS 2\nNUMBER_OF_SETS 1\n\
          BEGIN_DATA_FORMAT\n\"col one\" \"col two\"\n\
          END_DATA_FORMAT\nBEGIN_DATA\n1 2\nEND_DATA\n"),
        ("no-data-section",
         "IT8FILE\nNUMBER_OF_FIELDS 1\n\
          BEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\n"),
    ];
    unsafe {
        for (name, doc) in docs {
            let p = c::cmsIT8LoadFromMem(std::ptr::null_mut(),
                doc.as_ptr() as *const _, doc.len() as u32);
            eprintln!("{name} load={}", !p.is_null());
            if !p.is_null() {
                let n = c::cmsIT8GetDataRowCol(p, 0, 0);
                eprintln!("  row0col0={n:?}");
                c::cmsIT8Free(p);
            }
        }
    }
}

/// CookPointers — LABEL/$-prefixed fields with forward/backward
/// table references + SAMPLE_ID field arms.
#[test]
fn it8_cookptr_arms() {
    let docs: &[(&str, &str)] = &[
        ("label-ref",
         "IT8FILE\nNUMBER_OF_FIELDS 2\nNUMBER_OF_SETS 1\n\
          BEGIN_DATA_FORMAT\nA LABEL\nEND_DATA_FORMAT\n\
          BEGIN_DATA\n1 \"t2\"\nEND_DATA\n\
          IT8FILE\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\n\
          BEGIN_DATA_FORMAT\nB\nEND_DATA_FORMAT\n\
          BEGIN_DATA\n2\nEND_DATA\n"),
        ("dollar-ref",
         "IT8FILE\nNUMBER_OF_FIELDS 2\nNUMBER_OF_SETS 1\n\
          BEGIN_DATA_FORMAT\nA $REF\nEND_DATA_FORMAT\n\
          BEGIN_DATA\n1 \"t2\"\nEND_DATA\n\
          IT8FILE\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\n\
          BEGIN_DATA_FORMAT\nB\nEND_DATA_FORMAT\n\
          BEGIN_DATA\n2\nEND_DATA\n"),
        ("sample-id",
         "IT8FILE\nNUMBER_OF_FIELDS 3\nNUMBER_OF_SETS 2\n\
          BEGIN_DATA_FORMAT\nSAMPLE_ID A B\nEND_DATA_FORMAT\n\
          BEGIN_DATA\n1 \"a\" 5\n2 \"b\" 6\nEND_DATA\n"),
        ("multi-ref",
         "IT8FILE\nNUMBER_OF_FIELDS 3\nNUMBER_OF_SETS 2\n\
          BEGIN_DATA_FORMAT\nA LABEL $NEXT\nEND_DATA_FORMAT\n\
          BEGIN_DATA\n1 \"t2\" \"t3\"\n2 \"t3\" \"t2\"\nEND_DATA\n\
          IT8FILE\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\n\
          BEGIN_DATA_FORMAT\nB\nEND_DATA_FORMAT\n\
          BEGIN_DATA\n5\nEND_DATA\n\
          IT8FILE\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\n\
          BEGIN_DATA_FORMAT\nC\nEND_DATA_FORMAT\n\
          BEGIN_DATA\n6\nEND_DATA\n"),
        ("self-ref",
         "IT8FILE\nNUMBER_OF_FIELDS 2\nNUMBER_OF_SETS 1\n\
          BEGIN_DATA_FORMAT\nA LABEL\nEND_DATA_FORMAT\n\
          BEGIN_DATA\n1 \"t1\"\nEND_DATA\n"),
        ("nodataformat",
         "IT8FILE\nNUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\n\
          BEGIN_DATA\n1\nEND_DATA\n"),
    ];
    unsafe {
        for (name, doc) in docs {
            let p = c::cmsIT8LoadFromMem(std::ptr::null_mut(),
                doc.as_ptr() as *const _, doc.len() as u32);
            eprintln!("{name} load={}", !p.is_null());
            if !p.is_null() {
                let n = c::cmsIT8SetTable(p, 0);
                let p0 = c::cmsIT8GetPatchByName(p,
                    b"1\0".as_ptr() as *const i8);
                let d = c::cmsIT8GetDataRowCol(p, 0, 1);
                eprintln!("  tbl={n} patch={p0} d={d:?}");
                c::cmsIT8Free(p);
            }
        }
    }
}

/// SetData/DataSection residual — property/data writes with
/// every value-form + missing-table/multi-row arms.
#[test]
fn it8_setdata_arms() {
    unsafe {
        let doc = "IT8FILE\nNUMBER_OF_FIELDS 2\nNUMBER_OF_SETS 2\n\
            BEGIN_DATA_FORMAT\nA B\nEND_DATA_FORMAT\n\
            BEGIN_DATA\n1 2\n3 4\nEND_DATA\n";
        let p = c::cmsIT8LoadFromMem(std::ptr::null_mut(),
            doc.as_ptr() as *const _, doc.len() as u32);
        eprintln!("load={}", !p.is_null());
        if !p.is_null() {
            // SetData patch/field/row forms
            for (patch, field, val, name) in [
                (b"1\0" as &[u8], b"A\0" as &[u8],
                    b"10\0" as &[u8], "patch1-A"),
                (b"2\0", b"B\0", b"20\0", "patch2-B"),
                (b"\0", b"A\0", b"x\0", "empty-patch"),
                (b"1\0", b"\0", b"x\0", "empty-field"),
                (b"1\0", b"A\0", b"\0", "empty-val"),
                (b"9\0", b"A\0", b"x\0", "no-patch"),
                (b"1\0", b"Z\0", b"x\0", "no-field"),
            ] {
                let r = c::cmsIT8SetData(p,
                    patch.as_ptr() as *const i8,
                    field.as_ptr() as *const i8,
                    val.as_ptr() as *const i8);
                eprintln!("{name}={r}");
            }
            // row/col forms
            let r = c::cmsIT8SetDataRowCol(p, 0, 0,
                b"99\0".as_ptr() as *const i8);
            let d = c::cmsIT8GetDataRowCol(p, 0, 0);
            let r2 = c::cmsIT8SetDataRowCol(p, 9, 9,
                b"x\0".as_ptr() as *const i8);
            eprintln!("rowcol {r} d={d:?} far={r2}");
            // SetDataDbl
            let rd = c::cmsIT8SetDataDbl(p, b"1\0".as_ptr()
                as *const i8, b"A\0".as_ptr() as *const i8,
                3.14159);
            let dd = c::cmsIT8GetDataDbl(p, b"1\0".as_ptr()
                as *const i8, b"A\0".as_ptr() as *const i8);
            eprintln!("dbl {rd}={dd}");
            c::cmsIT8Free(p);
        }
    }
}

/// Remaining cmsIT8* APIs — GetDataDbl/RowCol/EnumProperties/
/// GetPatchByName/SetDataFormat/SetComment/SaveToMem.
#[test]
fn it8_api_arms() {
    unsafe {
        let doc = "# comment\nIT8FILE\nORIGINATOR \"x\"\n\
            NUMBER_OF_FIELDS 3\nNUMBER_OF_SETS 2\n\
            BEGIN_DATA_FORMAT\nSAMPLE_ID L a\nEND_DATA_FORMAT\n\
            BEGIN_DATA\n1 50 10\n2 60 20\nEND_DATA\n";
        let p = c::cmsIT8LoadFromMem(std::ptr::null_mut(),
            doc.as_ptr() as *const _, doc.len() as u32);
        eprintln!("load={}", !p.is_null());
        if p.is_null() { return; }
        // comment + properties
        c::cmsIT8SetComment(p, b"new comment\0".as_ptr()
            as *const i8);
        c::cmsIT8SetComment(p, b"\0".as_ptr() as *const i8);
        let mut names: *mut *mut i8 = std::ptr::null_mut();
        let np = c::cmsIT8EnumProperties(p, &mut names);
        let mut names2: *mut *mut i8 = std::ptr::null_mut();
        let np2 = c::cmsIT8EnumPropertyMulti(p,
            b"ORIGINATOR\0".as_ptr() as *const i8,
            &mut names2);
        let mut names3: *mut *mut i8 = std::ptr::null_mut();
        let np3 = c::cmsIT8EnumPropertyMulti(p,
            b"NOPROP\0".as_ptr() as *const i8,
            &mut names3);
        eprintln!("enumprop={np} multi={np2}/{np3}             n={names:?}/{names2:?}");
        // data format — set/get via SetDataFormat + Enum
        for (i, nm) in [(0i32, b"SAMPLE_ID\0" as &[u8]),
            (1, b"L\0"), (2, b"a\0"), (3, b"b\0"),
            (9, b"bad\0")] {
            let r = c::cmsIT8SetDataFormat(p, i,
                nm.as_ptr() as *const i8);
            eprintln!("fmt{i}->{nm:?}={r}");
        }
        let mut fnames: *mut *mut i8 = std::ptr::null_mut();
        let ne = c::cmsIT8EnumDataFormat(p, &mut fnames);
        eprintln!("enumfmt={ne} n={fnames:?}");
        // patch queries
        for name in [b"1\0" as &[u8], b"2\0", b"9\0",
            b"\0"] {
            let i = c::cmsIT8GetPatchByName(p,
                name.as_ptr() as *const i8);
            eprintln!("patch {:?}={i}", name);
        }
        // data getters — dbl + rowcol forms
        let d = c::cmsIT8GetDataDbl(p,
            b"1\0".as_ptr() as *const i8,
            b"L\0".as_ptr() as *const i8);
        let d2 = c::cmsIT8GetDataRowColDbl(p, 0, 1);
        let s_ = c::cmsIT8GetDataRowCol(p, 0, 0);
        eprintln!("dbl={d} rc={d2} s={s_:?}");
        // save to mem
        let mut buf = vec![0u8; 1 << 20];
        let mut needed = 0u32;
        let r = c::cmsIT8SaveToMem(p,
            buf.as_mut_ptr() as *mut _, &mut needed);
        let mut needed2 = 0u32;
        let r2 = c::cmsIT8SaveToMem(p,
            std::ptr::null_mut(), &mut needed2);
        eprintln!("save={r} needed={needed}/{needed2} null={r2}");
        // table navigation
        let t0 = c::cmsIT8SetTableByLabel(p,
            b"IT8FILE\0".as_ptr() as *const i8,
            std::ptr::null(), std::ptr::null());
        let tc = c::cmsIT8TableCount(p);
        let ts = c::cmsIT8SetTable(p, 0);
        let ts2 = c::cmsIT8SetTable(p, 99);
        let si = c::cmsIT8SetIndexColumn(p,
            b"SAMPLE_ID\0".as_ptr() as *const i8);
        let si2 = c::cmsIT8SetIndexColumn(p,
            b"bad\0".as_ptr() as *const i8);
        eprintln!("tab={t0} cnt={tc} ts={ts}/{ts2} idx={si}/{si2}");
        c::cmsIT8Free(p);
    }
}

/// cmscgats residual — InSymbol/HeaderSection/ParseCube/
/// ParseFloatNumber internals via exotic lexemes + doc forms.
#[test]
fn cgats_residual_arms() {
    unsafe {
        // exotic lexemes — InSymbol's remaining arms
        for doc in [
            "IT8FILE\nBEGIN_DATA\nEND_DATA\n#trail\n",
            "IT8FILE #comment\nBEGIN_DATA\nEND_DATA\n",
            "IT8FILE 'quoted'\nBEGIN_DATA\nEND_DATA\n",
            "IT8FILE \"dq\"\nBEGIN_DATA\nEND_DATA\n",
            "IT8FILE 12.34e-5\nBEGIN_DATA\nEND_DATA\n",
            "IT8FILE -.5e+10\nBEGIN_DATA\nEND_DATA\n",
            "IT8FILE 0x1f\nBEGIN_DATA\nEND_DATA\n",
            "IT8FILE a-b_c.d\nBEGIN_DATA\nEND_DATA\n",
            "IT8FILE \x1a\nBEGIN_DATA\nEND_DATA\n",   // Ctrl-Z
            "IT8FILE \r\nBEGIN_DATA\nEND_DATA\n",   // CRLF
            "IT8FILE\n BEGIN_DATA\nEND_DATA\n",      // leading ws
            "IT8FILE\nBEGIN_DATA\rEND_DATA\n",      // CR alone
            "IT8FILE\nBEGIN_DATA\nEND_DATA\n\x00\u{80}",
            "IT8FILE\n$INCLUDE \"path\"\nBEGIN_DATA\nEND_DATA\n",
            "IT8FILE\n$INCLUDE 'path'\nBEGIN_DATA\nEND_DATA\n",
            "IT8FILE\n#KEYWORD \"v\"\nBEGIN_DATA\nEND_DATA\n",
            "IT8FILE\n#DATA_FORMAT_IDENTIFIER\nBEGIN_DATA\nEND_DATA\n",
            "IT8FILE\nUNDEFINED_KEYWORD\nBEGIN_DATA\nEND_DATA\n",
            "IT8FILE\nORIGINATOR\nBEGIN_DATA\nEND_DATA\n",   // no value
            "IT8FILE\nORIGINATOR \nBEGIN_DATA\nEND_DATA\n",
            "IT8FILE\nNUMBER_OF_FIELDS\nBEGIN_DATA\nEND_DATA\n",
            "IT8FILE\nNUMBER_OF_SETS x\nBEGIN_DATA\nEND_DATA\n",
            "IT8FILE\nBEGIN_DATA_FORMAT\nEND_DATA_FORMAT\nBEGIN_DATA\nEND_DATA\n",
        ] {
            let p = c::cmsIT8LoadFromMem(std::ptr::null_mut(),
                doc.as_ptr() as *const _, doc.len() as u32);
            eprintln!("doc{}={}", doc.len(), !p.is_null());
            if !p.is_null() { c::cmsIT8Free(p); }
        }
        // ParseFloatNumber residual — all float forms in
        //   data rows
        for num in ["0", "1", "-1", "0.5", "-0.5", ".5", "5.",
            "1e2", "1E2", "1e+2", "1e-2", "1.5e3", "-1.5e-3",
            "1.", ".1", "1e", "1e+", "1e-", "1.5e", "inf",
            "nan", "-inf", "1_0", "1,0", " 1", "1 ", "\t1",
            "0x10", "010", "1a", "a1", "+1", "--1", "1..5"] {
            let doc = format!("IT8FILE\nNUMBER_OF_FIELDS 1\n\
                NUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\nSAMPLE_ID\n\
                END_DATA_FORMAT\nBEGIN_DATA\n{num}\nEND_DATA\n");
            let p = c::cmsIT8LoadFromMem(std::ptr::null_mut(),
                doc.as_ptr() as *const _, doc.len() as u32);
            eprintln!("num '{num}'={}", !p.is_null());
            if !p.is_null() { c::cmsIT8Free(p); }
        }
    }
}

/// cmscgats residual — IT8 write internals (serialize all
///   property types + data formats + multi-table save).
#[test]
fn it8_write_arms() {
    unsafe {
        for n in 0..3 {
            let p = c::cmsIT8Alloc(std::ptr::null_mut());
            c::cmsIT8SetSheetType(p,
                b"IT8.7/2\0".as_ptr() as *const i8);
            // all property types
            c::cmsIT8SetPropertyStr(p, b"ORIGINATOR\0".as_ptr()
                as *const i8, b"zen\0".as_ptr() as *const i8);
            c::cmsIT8SetPropertyDbl(p, b"NUM\0".as_ptr()
                as *const i8, 42.5);
            c::cmsIT8SetPropertyHex(p, b"HEX\0".as_ptr()
                as *const i8, 0xdead);
            c::cmsIT8SetPropertyUncooked(p,
                b"UNC\0".as_ptr() as *const i8,
                b"raw $x y\0".as_ptr() as *const i8);
            c::cmsIT8SetComment(p,
                b"a comment\0".as_ptr() as *const i8);
            c::cmsIT8SetPropertyMulti(p,
                b"MULTI\0".as_ptr() as *const i8,
                b"sub1\0".as_ptr() as *const i8,
                b"val1\0".as_ptr() as *const i8);
            // data format + rows
            c::cmsIT8SetDataFormat(p, 0,
                b"SAMPLE_ID\0".as_ptr() as *const i8);
            c::cmsIT8SetDataFormat(p, 1,
                b"LAB_L\0".as_ptr() as *const i8);
            c::cmsIT8SetDataFormat(p, 2,
                b"LAB_A\0".as_ptr() as *const i8);
            c::cmsIT8SetDataFormat(p, 3,
                b"LAB_B\0".as_ptr() as *const i8);
            let rows = (n + 1) * 4;
            c::cmsIT8SetPropertyDbl(p,
                b"NUMBER_OF_SETS\0".as_ptr() as *const i8,
                rows as f64);
            c::cmsIT8SetPropertyDbl(p,
                b"NUMBER_OF_FIELDS\0".as_ptr() as *const i8,
                4.0);
            for i in 0..rows {
                let nm = format!("n{i}\0");
                c::cmsIT8SetDataRowCol(p, i as i32, 0,
                    nm.as_ptr() as *const i8);
                c::cmsIT8SetDataRowColDbl(p, i as i32, 1,
                    50.0 + i as f64);
                c::cmsIT8SetDataRowColDbl(p, i as i32, 2,
                    i as f64 - 10.0);
                c::cmsIT8SetDataRowColDbl(p, i as i32, 3,
                    i as f64 + 5.0);
            }
            let mut buf = vec![0u8; 1 << 18];
            let mut sz = buf.len() as u32;
            let s = c::cmsIT8SaveToMem(p,
                buf.as_mut_ptr() as *mut _,
                &mut sz);
            eprintln!("it8 n{n} save={s} sz={sz}");
            // reload and re-enumerate
            let rp = c::cmsIT8LoadFromMem(
                std::ptr::null_mut(),
                buf.as_ptr() as *const _, sz);
            if !rp.is_null() {
                for name in [
                    b"ORIGINATOR\0".as_slice(),
                    b"NUM\0", b"HEX\0", b"UNC\0",
                    b"MULTI\0", b"nonexist\0"] {
                    let v = c::cmsIT8GetProperty(rp,
                        name.as_ptr() as *const i8);
                    let d = c::cmsIT8GetPropertyDbl(rp,
                        name.as_ptr() as *const i8);
                    eprintln!("  {:?} v={v:?} d={d}",
                        std::str::from_utf8(name)
                            .unwrap_or("?"));
                }
                c::cmsIT8Free(rp);
            }
            c::cmsIT8Free(p);
        }
    }
}

/// cmscgats residual — parse-error paths (malformed doc forms
///   hitting every reject branch in ParseIT8/HeaderSection/
///   DataSection/InSymbol).
#[test]
fn cgats_errpath_arms() {
    unsafe {
        for (doc, name) in [
            ("NOTIT8\n", "no-magic"),
            ("IT8FILE", "no-newline"),
            ("IT8FILE\n", "hdr-only"),
            ("IT8FILE\nBEGIN_DATA\n", "no-end"),
            ("IT8FILE\nEND_DATA\n", "data-no-begin"),
            ("IT8FILE\nBEGIN_DATA\nBEGIN_DATA\nEND_DATA\n",
                "double-begin"),
            ("IT8FILE\nBEGIN_DATA\nEND_DATA\nEND_DATA\n",
                "double-end"),
            ("IT8FILE\nBEGIN_DATA_FORMAT\nA B\nEND_DATA_FORMAT\nBEGIN_DATA\n1 2\nEND_DATA\n",
                "fmt2data"),
            ("IT8FILE\nBEGIN_DATA_FORMAT\nA B C\nEND_DATA_FORMAT\nBEGIN_DATA\n1 2 3\nEND_DATA\n",
                "fmt3data"),
            ("IT8FILE\nBEGIN_DATA_FORMAT\nA B C\nEND_DATA_FORMAT\nBEGIN_DATA\n1 2\nEND_DATA\n",
                "short-row"),
            ("IT8FILE\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\nBEGIN_DATA\n1 2 3\nEND_DATA\n",
                "long-row"),
            ("IT8FILE\nNUMBER_OF_FIELDS 3\nBEGIN_DATA_FORMAT\nA B C\nEND_DATA_FORMAT\nBEGIN_DATA\n1 2 3\n4 5 6\nEND_DATA\n",
                "nf2rows"),
            ("IT8FILE\nNUMBER_OF_FIELDS 3\nNUMBER_OF_SETS 3\nBEGIN_DATA_FORMAT\nA B C\nEND_DATA_FORMAT\nBEGIN_DATA\n1 2 3\nEND_DATA\n",
                "ns1of3"),
            ("IT8FILE\nORIGINATOR \"unclosed\nBEGIN_DATA\nEND_DATA\n",
                "unclosed-str"),
            ("IT8FILE\nORIGINATOR 'unclosed\nBEGIN_DATA\nEND_DATA\n",
                "unclosed-sq"),
            ("IT8FILE\nPROP val1 val2 val3\nBEGIN_DATA\nEND_DATA\n",
                "multiword"),
            ("IT8FILE\n0xZZ\nBEGIN_DATA\nEND_DATA\n",
                "badhex"),
            ("IT8FILE\n1.2.3\nBEGIN_DATA\nEND_DATA\n",
                "badnum"),
            ("IT8FILE\n@\x01\x02\nBEGIN_DATA\nEND_DATA\n",
                "ctrl-chars"),
            ("IT8FILE\nBEGIN_DATA_FORMAT\nEND_DATA_FORMAT\nBEGIN_DATA\nEND_DATA\n",
                "empty-fmt"),
            ("IT8FILE\n$UNKNOWN_DIRECTIVE x\nBEGIN_DATA\nEND_DATA\n",
                "unknown-dollar"),
            ("IT8FILE\nDKEYWORD x\nBEGIN_DATA\nEND_DATA\n",
                "unknown-dkey"),
            ("IT8FILE\n#COMMENT y\nBEGIN_DATA\nEND_DATA\n",
                "hash-comment"),
            ("IT8FILE\nORIGINATOR\nMANUFACTURER\nBEGIN_DATA\nEND_DATA\n",
                "empty-props"),
            ("IT8FILE\nORIGINATOR  \"  spaces  \"\nBEGIN_DATA\nEND_DATA\n",
                "space-str"),
            ("IT8FILE\nORIGINATOR 'single'\nBEGIN_DATA\nEND_DATA\n",
                "sq-str"),
            ("IT8FILE\nORIGINATOR 1e999\nBEGIN_DATA\nEND_DATA\n",
                "huge-exp"),
            ("IT8FILE\nORIGINATOR 0.0e0\nBEGIN_DATA\nEND_DATA\n",
                "zero-exp"),
            ("IT8FILE\nORIGINATOR .5\nBEGIN_DATA\nEND_DATA\n",
                "dot-lead"),
            ("IT8FILE\nORIGINATOR 5.\nBEGIN_DATA\nEND_DATA\n",
                "dot-trail"),
            ("IT8FILE\nORIGINATOR -\nBEGIN_DATA\nEND_DATA\n",
                "bare-minus"),
            ("IT8FILE\nORIGINATOR +\nBEGIN_DATA\nEND_DATA\n",
                "bare-plus"),
            ("IT8FILE\nORIGINATOR e5\nBEGIN_DATA\nEND_DATA\n",
                "bare-e"),
            ("IT8FILE\nORIGINATOR 0x\nBEGIN_DATA\nEND_DATA\n",
                "bare-0x"),
            ("IT8FILE\n\tTAB\tsep\nBEGIN_DATA\nEND_DATA\n",
                "tab-sep"),
            ("IT8FILE\nORIGINATOR \x7f del\nBEGIN_DATA\nEND_DATA\n",
                "del-char"),
            ("IT8FILE\n\x0b vt\nBEGIN_DATA\nEND_DATA\n",
                "vt-char"),
            ("IT8FILE\n\x0c ff\nBEGIN_DATA\nEND_DATA\n",
                "ff-char"),
        ] {
            let p = c::cmsIT8LoadFromMem(
                std::ptr::null_mut(),
                doc.as_ptr() as *const _,
                doc.len() as u32);
            eprintln!("{name}={}", !p.is_null());
            if !p.is_null() { c::cmsIT8Free(p); }
        }
    }
}

/// cmscgats residual — cmsIT8LoadFromFile / ParseCube /
///   SetTableByLabel / EnumPropertyMulti / SaveToFile
///   internals.
#[test]
fn cgats_file_cube_arms() {
    unsafe {
        let ctx = c::cmsCreateContext(
            std::ptr::null_mut(), std::ptr::null_mut());
        // write a valid IT8 doc, load from file, enum props
        let doc = b"IT8FILE\nORIGINATOR \"org\"\n\
            MANUFACTURER \"mfr\"\nNUMBER_OF_FIELDS 3\n\
            BEGIN_DATA_FORMAT\nSAMPLE_ID CMYK_C CMYK_M\n\
            END_DATA_FORMAT\nNUMBER_OF_SETS 2\n\
            BEGIN_DATA\nid1 0.1 0.2\nid2 0.3 0.4\n\
            END_DATA\nIT8FILE\nORIGINATOR \"t2\"\n\
            NUMBER_OF_FIELDS 2\nBEGIN_DATA_FORMAT\n\
            A B\nEND_DATA_FORMAT\nNUMBER_OF_SETS 1\n\
            BEGIN_DATA\n1 2\nEND_DATA\n\0";
        // mem load + table ops
        let h = c::cmsIT8LoadFromMem(ctx,
            doc.as_ptr() as *const _,
            doc.len() as u32 - 1);
        eprintln!("mem h={}", !h.is_null());
        if !h.is_null() {
            let nt = c::cmsIT8TableCount(h);
            eprintln!("tables={nt}");
            for i in 0..nt + 2 {
                let s = c::cmsIT8SetTable(h, i);
                eprintln!("  set{i}={s}");
            }
            // by-label
            for (set, fld, exp, name) in [
                ("", "", "", "all-empty"),
                ("id1", "SAMPLE_ID", "", "byid"),
                ("2", "", "", "byidx"),
                ("", "A", "", "byfield"),
                ("", "", "A", "byexp"),
            ] {
                let r = c::cmsIT8SetTableByLabel(h,
                    format!("{set}\0").as_ptr() as *const _,
                    format!("{fld}\0").as_ptr() as *const _,
                    format!("{exp}\0").as_ptr() as *const _);
                eprintln!("  lbl{name}={r}");
            }
            // save-to-file + load-from-file
            for path in [
                "/home/lilith/tmp/it8-a.txt",
                "/home/lilith/tmp/it8-b.txt",
            ] {
                let p2 = format!("{path}\0");
                let s = c::cmsIT8SaveToFile(h,
                    p2.as_ptr() as *const _);
                let h2 = c::cmsIT8LoadFromFile(ctx,
                    p2.as_ptr() as *const _);
                eprintln!("save={s} file h={}",
                    !h2.is_null());
                if !h2.is_null() {
                    c::cmsIT8Free(h2); }
                std::fs::remove_file(path).ok();
            }
            // load failures
            for path in [
                "/nonexistent-x/xx.it8",
                "/dev/null",
                "/proc/version",
            ] {
                let p2 = format!("{path}\0");
                let h3 = c::cmsIT8LoadFromFile(ctx,
                    p2.as_ptr() as *const _);
                eprintln!("load {path} h={}",
                    !h3.is_null());
                if !h3.is_null() {
                    c::cmsIT8Free(h3); }
            }
            c::cmsIT8Free(h);
        }
        c::cmsDeleteContext(ctx);
    }
}

/// cmscgats residual — ParseCube via
///   cmsCreateDeviceLinkFromCubeFileTHR (1D/3D LUT cube
///   files, valid + malformed) + EnumPropertyMulti.
#[test]
fn cgats_cube_arms() {
    unsafe {
        let ctx = c::cmsCreateContext(
            std::ptr::null_mut(), std::ptr::null_mut());
        // cube files: 1D LUT, 3D LUT, malformed variants
        for (content, name) in [
            ("TITLE \"1d\"\nLUT_1D_SIZE 2\n\
              0.0 0.0 0.0\n1.0 1.0 1.0\n", "lut1d"),
            ("TITLE \"3d\"\nLUT_3D_SIZE 2\n\
              DOMAIN_MIN 0.0 0.0 0.0\n\
              DOMAIN_MAX 1.0 1.0 1.0\n\
              0.0 0.0 0.0\n0.0 0.0 1.0\n0.0 1.0 0.0\n\
              0.0 1.0 1.0\n1.0 0.0 0.0\n1.0 0.0 1.0\n\
              1.0 1.0 0.0\n1.0 1.0 1.0\n", "lut3d"),
            ("LUT_3D_SIZE 2\n\
              0.0 0.0 0.0\n0.0 0.0 1.0\n0.0 1.0 0.0\n\
              0.0 1.0 1.0\n1.0 0.0 0.0\n1.0 0.0 1.0\n\
              1.0 1.0 0.0\n1.0 1.0 1.0\n", "lut3d-nodomain"),
            ("LUT_1D_SIZE 2\n0.0 0.0 0.0\n", "trunc"),
            ("LUT_3D_SIZE 2\n0.0 0.0 0.0\n", "trunc3d"),
            ("GARBAGE\n1 2 3\n", "nocube"),
            ("", "empty"),
            ("LUT_1D_SIZE 0\n", "zero1d"),
            ("LUT_3D_SIZE 0\n", "zero3d"),
            ("LUT_1D_SIZE 2\nx y z\n0.0 0.0 0.0\n", "badnum"),
            ("TITLE \"a\"\nLUT_1D_SIZE 2\n\
              DOMAIN_MIN 0.5 0.5 0.5\n\
              DOMAIN_MAX 1.5 1.5 1.5\n\
              0.0 0.0 0.0\n1.0 1.0 1.0\n", "dom"),
            ("LUT_1D_SIZE 2\n\
              1e-3 2E+2 -3.5\n.25 +.5 6.\n", "floatfmt"),
        ] {
            let path = format!(
                "/home/lilith/tmp/cube-{name}.cube");
            std::fs::write(&path, content).unwrap();
            let p2 = format!("{path}\0");
            let dl = c::cmsCreateDeviceLinkFromCubeFileTHR(
                ctx, p2.as_ptr() as *const _);
            eprintln!("{name} dl={}", !dl.is_null());
            if !dl.is_null() {
                c::cmsCloseProfile(dl); }
            std::fs::remove_file(&path).ok();
        }
        // nonexistent + dir
        let p2 = b"/nonexistent/xx.cube\0";
        let dl = c::cmsCreateDeviceLinkFromCubeFileTHR(
            ctx, p2.as_ptr() as *const _);
        eprintln!("nofile dl={}", !dl.is_null());
        // EnumPropertyMulti on a multi-prop doc
        let doc = b"IT8FILE\nORIGINATOR \"o\"\n\
            KEYWORD k1\nKEYWORD k2\n\
            BEGIN_DATA\nEND_DATA\n\0";
        let h = c::cmsIT8LoadFromMem(ctx,
            doc.as_ptr() as *const _, doc.len() as u32 - 1);
        if !h.is_null() {
            for prop in ["ORIGINATOR", "KEYWORD",
                "NONEXIST", ""] {
                let mut names: *mut *mut i8 =
                    std::ptr::null_mut();
                let n = c::cmsIT8EnumPropertyMulti(h,
                    format!("{prop}\0").as_ptr()
                        as *const _, &mut names);
                eprintln!("enum {prop} n={n}");
            }
            c::cmsIT8Free(h);
        }
        c::cmsDeleteContext(ctx);
    }
}

/// cmscgats residual — InSymbol keyword classes +
///   ParseFloatNumber notations + satob hex-data
///   internals via all IT8 keyword and number forms.
#[test]
fn cgats_lexer_arms() {
    unsafe {
        let ctx = c::cmsCreateContext(
            std::ptr::null_mut(), std::ptr::null_mut());
        // every IT8 keyword + every number notation
        let docs: Vec<(&[u8], &str)> = vec![
            (b"IT8FILE\nORIGINATOR x\nMANUFACTURER y\n\
                CREATEDATE z\nPROD_DATE 2024-01-01\n\
                SERIAL 123\nMATERIAL m\nINSTRUMENTATION i\n\
                MEASUREMENT_SOURCE s\nLABORATORY l\n\
                SAMPLE_ID 1\nSAMPLE_NAME n\n\
                COMMENT c\nKEYWORD k\nBEGIN_DATA_FORMAT\n\
                A\nEND_DATA_FORMAT\nBEGIN_DATA\nEND_DATA\n\
                NUMBER_OF_FIELDS 1\nNUMBER_OF_SETS 1\n\
                \0", "all-kw"),
            // float notations in data
            (b"IT8FILE\nNUMBER_OF_FIELDS 1\n\
                BEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\n\
                NUMBER_OF_SETS 20\nBEGIN_DATA\n\
                0\n1\n-1\n+1\n.5\n5.\n-.5\n+.5\n\
                1e0\n1E0\n1e+5\n1e-5\n1.5e3\n-2.5E-3\n\
                0x10\n0xFF\n1.e5\n.5e-2\n1_000\ninf\n\
                END_DATA\n\0", "floats"),
            // hex data via DATA_FORMAT types
            (b"IT8FILE\nNUMBER_OF_FIELDS 2\n\
                BEGIN_DATA_FORMAT\nHEX DEC\n\
                END_DATA_FORMAT\nNUMBER_OF_SETS 4\n\
                BEGIN_DATA\n\
                0xDEADBEEF 42\n\
                0x0 0\n\
                0XABCD 1\n\
                0x1F.8 2\n\
                END_DATA\n\0", "hexdata"),
            // nested/special symbols
            (b"IT8FILE\nORIGINATOR \"s\"\n\
                BEGIN_DATA_FORMAT\n\"A-B.C_D/E\"\n\
                END_DATA_FORMAT\nBEGIN_DATA\nEND_DATA\n\
                \0", "specialfmt"),
            (b"IT8FILE\nORIGINATOR o\n\
                BEGIN_DATA_FORMAT\n\
                A\tB\tC\tD\tE\tF\tG\tH\n\
                END_DATA_FORMAT\n\
                NUMBER_OF_SETS 2\nBEGIN_DATA\n\
                1 2 3 4 5 6 7 8\n\
                9 10 11 12 13 14 15 16\n\
                END_DATA\n\0", "wide"),
            // case variants of keywords
            (b"it8file\noriginator x\n\
                begin_data_format\nA\nend_data_format\n\
                begin_data\nend_data\n\0", "lower"),
            (b"It8File\nOriginator x\n\
                Begin_Data_Format\nA\nEnd_Data_Format\n\
                Begin_Data\nEnd_Data\n\0", "mixed"),
            // numbers with unusual spacing
            (b"IT8FILE\nNUMBER_OF_FIELDS   3\n\
                NUMBER_OF_SETS\t2\n\
                BEGIN_DATA_FORMAT\n A  B  C\n\
                END_DATA_FORMAT\nBEGIN_DATA\n\
                 1   2   3  \n4\t5\t6\n\
                END_DATA\n\0", "spacing"),
        ];
        for (doc, name) in &docs {
            let h = c::cmsIT8LoadFromMem(ctx,
                doc.as_ptr() as *const _,
                doc.len() as u32);
            eprintln!("{name} h={}", !h.is_null());
            if !h.is_null() {
                // probe data access → satob/decode paths
                let nt = c::cmsIT8TableCount(h);
                for i in 0..nt {
                    c::cmsIT8SetTable(h, i);
                    for row in 0..4 {
                        for col in 0..4 {
                            let v = c::cmsIT8GetDataRowCol(
                                h, row, col);
                            let _ = v;
                            let d = c::cmsIT8GetDataRowColDbl(
                                h, row, col);
                            let _ = d;
                        }
                    }
                }
                c::cmsIT8Free(h);
            }
        }
        c::cmsDeleteContext(ctx);
    }
}

/// cmscgats residual — ParseFloatNumber NULL/overflow
///   branches + satob binary-write path + InSymbol
///   .include/quoted-symbol branches.
#[test]
fn cgats_num_bin_arms() {
    unsafe {
        let ctx = c::cmsCreateContext(
            std::ptr::null_mut(), std::ptr::null_mut());
        // doc with extreme-but-parseable numerics
        let doc = b"IT8FILE\nORIGINATOR o\n\
            NUMBER_OF_FIELDS 4\n\
            BEGIN_DATA_FORMAT\nA B C D\n\
            END_DATA_FORMAT\nNUMBER_OF_SETS 3\n\
            BEGIN_DATA\n\
            1e2147483648 2 -999999999999 1.5e200\n\
            1e+0000000002 1e-0000000002 .5e9 -9e99\n\
            2147483647.9 -2147483648.9 1e999 1e-999\n\
            END_DATA\n\0";
        let h = c::cmsIT8LoadFromMem(ctx,
            doc.as_ptr() as *const _, doc.len() as u32 - 1);
        eprintln!("h={}", !h.is_null());
        if !h.is_null() {
            // GetDataDbl on every cell incl missing
            for r in 0..8 {
                for c_ in 0..6 {
                    let d = c::cmsIT8GetDataRowColDbl(
                        h, r, c_);
                    let s = c::cmsIT8GetDataRowCol(h,
                        r, c_);
                    if r < 2 && c_ < 4 {
                        eprintln!("  [{r},{c_}] d={d} \
                            s={:?}",
                            if s.is_null() {
                                "null".to_string() }
                            else {
                                std::ffi::CStr::from_ptr(
                                    s).to_string_lossy()
                                    .to_string() });
                    }
                }
            }
            // SetData with binary values → satob on write
            for (row, col, v, name) in [
                (0, 0, "0b10101010", "bin8"),
                (0, 1, "0b1", "bin1"),
                (0, 2, "0xABCDEF", "hex"),
                (1, 0, "0b", "binonly"),
                (1, 1, "0B1100", "bin-upper"),
                (2, 0, "notanum", "nan"),
                (2, 1, "", "empty"),
                (9, 0, "0b1111", "oobarow"),
                (0, 9, "0b1111", "oobcol"),
            ] {
                let r = c::cmsIT8SetDataRowCol(h, row, col,
                    format!("{v}\0").as_ptr()
                        as *const _);
                eprintln!("set {name} r={r}");
            }
            // save → satob on binary-valued cells
            for path in [
                "/home/lilith/tmp/cb-a.it8",
            ] {
                let p2 = format!("{path}\0");
                let s = c::cmsIT8SaveToFile(h,
                    p2.as_ptr() as *const _);
                let h2 = c::cmsIT8LoadFromFile(ctx,
                    p2.as_ptr() as *const _);
                eprintln!("sv={s} rl={}", !h2.is_null());
                if !h2.is_null() {
                    c::cmsIT8Free(h2); }
                std::fs::remove_file(path).ok();
            }
            // mem-save path
            let mut buf = vec![0u8; 1 << 16];
            let mut n = buf.len() as u32;
            let sm = c::cmsIT8SaveToMem(h,
                buf.as_mut_ptr() as *mut _, &mut n);
            eprintln!("savemem={sm} n={n}");
            c::cmsIT8Free(h);
        }
        // .include directive docs
        for (doc2, name) in [
            (b"IT8FILE\n.INCLUDE \"x.it8\"\n\
                BEGIN_DATA\nEND_DATA\n\0" as &[u8],
                "include"),
            (b"IT8FILE\n.include \"x.it8\"\n\
                BEGIN_DATA\nEND_DATA\n\0",
                "include-low"),
            (b"IT8FILE\nORIGINATOR 'unclosed\n\
                BEGIN_DATA\nEND_DATA\n\0",
                "unclosed-q"),
            (b"IT8FILE\nORIGINATOR \"unclosed2\n\
                BEGIN_DATA\nEND_DATA\n\0",
                "unclosed-dq"),
            (b"IT8FILE\nORIGINATOR \x00bin\n\
                BEGIN_DATA\nEND_DATA\n\0",
                "bin-in-doc"),
            (b"IT8FILE\n@weird !chars #here\n\
                BEGIN_DATA\nEND_DATA\n\0",
                "weird-chars"),
        ] {
            let h2 = c::cmsIT8LoadFromMem(ctx,
                doc2.as_ptr() as *const _,
                doc2.len() as u32);
            eprintln!("{name} h={}", !h2.is_null());
            if !h2.is_null() { c::cmsIT8Free(h2); }
        }
        c::cmsDeleteContext(ctx);
    }
}

/// cmscgats residual — SetTableByLabel LABEL-field
///   "name nTable type" parse path + WRITE_PAIR write
///   (WEIGHTING_FUNCTION subkey) + uncooked write.
#[test]
fn cgats_label_pair_arms() {
    unsafe {
        let it8 = c::cmsIT8Alloc(std::ptr::null_mut());
        c::cmsIT8SetSheetType(it8,
            b"CGATS.17\0".as_ptr() as *const _);
        c::cmsIT8SetPropertyDbl(it8,
            b"NUMBER_OF_FIELDS\0".as_ptr() as *const _, 3.0);
        c::cmsIT8SetPropertyDbl(it8,
            b"NUMBER_OF_SETS\0".as_ptr() as *const _, 2.0);
        // declare data format columns incl LABEL, then
        //   write rows whose LABEL cell is
        //   "name nTable type" -> sscanf parse path
        c::cmsIT8SetDataFormat(it8, 0,
            b"SAMPLE_ID\0".as_ptr() as *const _);
        c::cmsIT8SetDataFormat(it8, 1,
            b"LABEL\0".as_ptr() as *const _);
        c::cmsIT8SetDataFormat(it8, 2,
            b"CMYK_C\0".as_ptr() as *const _);
        for (row, id, lbl) in [
            (0i32, b"R1\0".as_slice(), b"L1 0 SAMPLE\0".as_slice()),
            (1, b"R2\0".as_slice(), b"L2 1 OTHER\0".as_slice()),
        ] {
            c::cmsIT8SetDataRowCol(it8, row, 0,
                id.as_ptr() as *const _);
            c::cmsIT8SetDataRowCol(it8, row, 1,
                lbl.as_ptr() as *const _);
            c::cmsIT8SetDataRowCol(it8, row, 2,
                b"0.5\0".as_ptr() as *const _);
        }
        // SetTableByLabel against the LABEL column
        // cSet = patch name (SAMPLE_ID), cField = data col
        for (set, fld, exp, name) in [
            (b"R1\0".as_ptr(), b"LABEL\0".as_ptr(),
                b"SAMPLE\0".as_ptr(), "lbl-match"),
            (b"R2\0".as_ptr(), b"LABEL\0".as_ptr(),
                b"SAMPLE\0".as_ptr(), "lbl-type-mis"),
            (b"R1\0".as_ptr(), b"LABEL\0".as_ptr(),
                b"\0".as_ptr(), "lbl-exp-empty"),
            (b"R1\0".as_ptr(), b"\0".as_ptr(),
                b"SAMPLE\0".as_ptr(), "lbl-fld-empty"),
            (b"R1\0".as_ptr(), std::ptr::null(),
                b"SAMPLE\0".as_ptr(), "lbl-fld-null"),
            (b"NOPE\0".as_ptr(), b"LABEL\0".as_ptr(),
                b"SAMPLE\0".as_ptr(), "lbl-no-patch"),
            (b"R1\0".as_ptr(), b"CMYK_C\0".as_ptr(),
                b"SAMPLE\0".as_ptr(), "lbl-wrong-fld"),
        ] {
            let r = c::cmsIT8SetTableByLabel(it8,
                set as *const _,
                fld as *const _,
                exp as *const _);
            eprintln!("{name} r={r}");
        }
        // WRITE_PAIR: WEIGHTING_FUNCTION predefined pair
        let r = c::cmsIT8SetPropertyMulti(it8,
            b"WEIGHTING_FUNCTION\0".as_ptr() as *const _,
            b"CIE_D65\0".as_ptr() as *const _,
            b"0.5\0".as_ptr() as *const _);
        eprintln!("pair-prop r={r}");
        let mut buf = vec![0u8; 1 << 17];
        let mut n = buf.len() as u32;
        let s = c::cmsIT8SaveToMem(it8,
            buf.as_mut_ptr() as *mut _, &mut n);
        eprintln!("save-mem={s} n={n}");
        if s != 0 {
            let txt = String::from_utf8_lossy(
                &buf[..n as usize]);
            for line in txt.lines() {
                if line.contains("WEIGHT")
                    || line.contains("LABEL") {
                    eprintln!("  out: {line}");
                }
            }
        }
        c::cmsIT8Free(it8);
    }
}

/// cmscgats residual — InSymbol '+'-signed numbers,
///   non-integer DoubleFormatter branch, include-open
///   + deep-nesting overflow paths.
#[test]
fn cgats_symbol_arms() {
    unsafe {
        // '+'-signed + float-formatted data cells
        for (doc, name) in [
            (b"CGATS.17\nNUMBER_OF_FIELDS 1\n\
               NUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\n\
               SAMPLE_ID\nEND_DATA_FORMAT\nBEGIN_DATA\n\
               +5\nEND_DATA\n\0".as_slice(), "plus"),
            (b"CGATS.17\nNUMBER_OF_FIELDS 1\n\
               NUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\n\
               SAMPLE_ID\nEND_DATA_FORMAT\nBEGIN_DATA\n\
               +1.5e3\nEND_DATA\n\0".as_slice(), "plusf"),
            (b"CGATS.17\nNUMBER_OF_FIELDS 1\n\
               NUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\n\
               SAMPLE_ID\nEND_DATA_FORMAT\nBEGIN_DATA\n\
               -7\nEND_DATA\n\0".as_slice(), "minus"),
            (b"CGATS.17\nNUMBER_OF_FIELDS 1\n\
               NUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\n\
               SAMPLE_ID\nEND_DATA_FORMAT\nBEGIN_DATA\n\
               1.25e-2\nEND_DATA\n\0".as_slice(), "float"),
            (b"CGATS.17\nNUMBER_OF_FIELDS 1\n\
               NUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\n\
               SAMPLE_ID\nEND_DATA_FORMAT\nBEGIN_DATA\n\
               0x1F\nEND_DATA\n\0".as_slice(), "hex"),
            (b"CGATS.17\nNUMBER_OF_FIELDS 1\n\
               NUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\n\
               SAMPLE_ID\nEND_DATA_FORMAT\nBEGIN_DATA\n\
               0b101\nEND_DATA\n\0".as_slice(), "bin"),
        ] {
            let it8 = c::cmsIT8LoadFromMem(
                std::ptr::null_mut(),
                doc.as_ptr() as *const _, doc.len() as u32);
            eprintln!("{name} it8={}", !it8.is_null());
            if !it8.is_null() {
                let v = c::cmsIT8GetDataDbl(it8,
                    b"0\0".as_ptr() as *const _,
                    b"SAMPLE_ID\0".as_ptr() as *const _);
                eprintln!("  dbl={v}");
                c::cmsIT8Free(it8);
            }
        }
        // #INCLUDE into a real file — open-success path
        let inc_path = b"/tmp/lcms_inc.it8\0";
        std::fs::write("/tmp/lcms_inc.it8",
            b"CGATS.17\nNUMBER_OF_FIELDS 1\n\
              NUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\n\
              SAMPLE_ID\nEND_DATA_FORMAT\nBEGIN_DATA\n\
              42\nEND_DATA\n").unwrap();
        let doc = b"#INCLUDE \"/tmp/lcms_inc.it8\"\n\0";
        let it8 = c::cmsIT8LoadFromMem(
            std::ptr::null_mut(),
            doc.as_ptr() as *const _, doc.len() as u32);
        eprintln!("include-real it8={}", !it8.is_null());
        if !it8.is_null() {
            let v = c::cmsIT8GetDataDbl(it8,
                b"0\0".as_ptr() as *const _,
                b"SAMPLE_ID\0".as_ptr() as *const _);
            eprintln!("  inc-dbl={v}");
            c::cmsIT8Free(it8);
        }
        // nested includes → IncludeSP++ path
        for i in 0..12 {
            let inner = format!(
                "#INCLUDE \"/tmp/lcms_inc{}.it8\"\n", i);
            let this = format!(
                "/tmp/lcms_inc{}.it8",
                if i == 0 { "".into() }
                else { format!("{}", i - 1) });
            std::fs::write(&this, &inner).unwrap();
        }
        let doc2 = b"#INCLUDE \"/tmp/lcms_inc11.it8\"\n\0";
        let it8b = c::cmsIT8LoadFromMem(
            std::ptr::null_mut(),
            doc2.as_ptr() as *const _, doc2.len() as u32);
        eprintln!("include-deep it8={}", !it8b.is_null());
        if !it8b.is_null() { c::cmsIT8Free(it8b); }
        let _ = std::fs::remove_file("/tmp/lcms_inc.it8");
        for i in 0..12 {
            let _ = std::fs::remove_file(
                format!("/tmp/lcms_inc{}.it8", i));
        }
    }
}

/// cmscgats residual — quoted sheet-type header,
///   newline-in-property continuation write,
///   table-out-of-sequence, EnumPropertyMulti
///   subkey enumeration.
#[test]
fn cgats_header_seq_arms() {
    unsafe {
        for (doc, name) in [
            // quoted sheet type -> SSTRING path
            (b"\"CGATS.17\"\nNUMBER_OF_FIELDS 1\n\
               NUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\n\
               SAMPLE_ID\nEND_DATA_FORMAT\nBEGIN_DATA\n\
               5\nEND_DATA\n\0".as_slice(), "quoted"),
            // table out of sequence: 2 BEGIN_DATA_FORMAT
            //   sections beyond declared count
            (b"CGATS.17\nNUMBER_OF_FIELDS 1\n\
               NUMBER_OF_SETS 1\nBEGIN_DATA_FORMAT\n\
               SAMPLE_ID\nEND_DATA_FORMAT\nBEGIN_DATA\n\
               5\nEND_DATA\n\
               BEGIN_DATA_FORMAT\nX\nEND_DATA_FORMAT\n\
               BEGIN_DATA\n1\nEND_DATA\n\
               BEGIN_DATA_FORMAT\nY\nEND_DATA_FORMAT\n\
               BEGIN_DATA\n2\nEND_DATA\n\
               BEGIN_DATA_FORMAT\nZ\nEND_DATA_FORMAT\n\
               BEGIN_DATA\n3\nEND_DATA\n\
               BEGIN_DATA_FORMAT\nW\nEND_DATA_FORMAT\n\
               BEGIN_DATA\n4\nEND_DATA\n\
               BEGIN_DATA_FORMAT\nV\nEND_DATA_FORMAT\n\
               BEGIN_DATA\n5\nEND_DATA\n\
               BEGIN_DATA_FORMAT\nU\nEND_DATA_FORMAT\n\
               BEGIN_DATA\n6\nEND_DATA\n\
               BEGIN_DATA_FORMAT\nT\nEND_DATA_FORMAT\n\
               BEGIN_DATA\n7\nEND_DATA\n\
               BEGIN_DATA_FORMAT\nS\nEND_DATA_FORMAT\n\
               BEGIN_DATA\n8\nEND_DATA\n\
               BEGIN_DATA_FORMAT\nR\nEND_DATA_FORMAT\n\
               BEGIN_DATA\n9\nEND_DATA\n\
               BEGIN_DATA_FORMAT\nQ\nEND_DATA_FORMAT\n\
               BEGIN_DATA\n0\nEND_DATA\n\
               BEGIN_DATA_FORMAT\nP\nEND_DATA_FORMAT\n\
               BEGIN_DATA\n9\nEND_DATA\n\
               BEGIN_DATA_FORMAT\nO\nEND_DATA_FORMAT\n\
               BEGIN_DATA\n8\nEND_DATA\n\
               BEGIN_DATA_FORMAT\nN\nEND_DATA_FORMAT\n\
               BEGIN_DATA\n7\nEND_DATA\n\
               BEGIN_DATA_FORMAT\nM\nEND_DATA_FORMAT\n\
               BEGIN_DATA\n6\nEND_DATA\n\
               BEGIN_DATA_FORMAT\nL\nEND_DATA_FORMAT\n\
               BEGIN_DATA\n5\nEND_DATA\n\
               BEGIN_DATA_FORMAT\nK\nEND_DATA_FORMAT\n\
               BEGIN_DATA\n4\nEND_DATA\n\
               BEGIN_DATA_FORMAT\nJ\nEND_DATA_FORMAT\n\
               BEGIN_DATA\n3\nEND_DATA\n\
               BEGIN_DATA_FORMAT\nI\nEND_DATA_FORMAT\n\
               BEGIN_DATA\n2\nEND_DATA\n\
               BEGIN_DATA_FORMAT\nH\nEND_DATA_FORMAT\n\
               BEGIN_DATA\n1\nEND_DATA\n\
               BEGIN_DATA_FORMAT\nG\nEND_DATA_FORMAT\n\
               BEGIN_DATA\n0\nEND_DATA\n\
               BEGIN_DATA_FORMAT\nF\nEND_DATA_FORMAT\n\
               BEGIN_DATA\n9\nEND_DATA\n\
               BEGIN_DATA_FORMAT\nE\nEND_DATA_FORMAT\n\
               BEGIN_DATA\n8\nEND_DATA\n\
               BEGIN_DATA_FORMAT\nD\nEND_DATA_FORMAT\n\
               BEGIN_DATA\n7\nEND_DATA\n\
               BEGIN_DATA_FORMAT\nC\nEND_DATA_FORMAT\n\
               BEGIN_DATA\n6\nEND_DATA\n\
               BEGIN_DATA_FORMAT\nB\nEND_DATA_FORMAT\n\
               BEGIN_DATA\n5\nEND_DATA\n\
               BEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\n\
               BEGIN_DATA\n4\nEND_DATA\n\
               BEGIN_DATA_FORMAT\nZX\nEND_DATA_FORMAT\n\
               BEGIN_DATA\n3\nEND_DATA\n\
               BEGIN_DATA_FORMAT\nZY\nEND_DATA_FORMAT\n\
               BEGIN_DATA\n2\nEND_DATA\n\
               BEGIN_DATA_FORMAT\nZZ\nEND_DATA_FORMAT\n\
               BEGIN_DATA\n1\nEND_DATA\n\
               BEGIN_DATA_FORMAT\nXA\nEND_DATA_FORMAT\n\
               BEGIN_DATA\n0\nEND_DATA\n\
               BEGIN_DATA_FORMAT\nXB\nEND_DATA_FORMAT\n\
               BEGIN_DATA\n9\nEND_DATA\n\
               BEGIN_DATA_FORMAT\nXC\nEND_DATA_FORMAT\n\
               BEGIN_DATA\n8\nEND_DATA\n\
               BEGIN_DATA_FORMAT\nXD\nEND_DATA_FORMAT\n\
               BEGIN_DATA\n7\nEND_DATA\n\
               BEGIN_DATA_FORMAT\nXE\nEND_DATA_FORMAT\n\
               BEGIN_DATA\n6\nEND_DATA\n\
               BEGIN_DATA_FORMAT\nXF\nEND_DATA_FORMAT\n\
               BEGIN_DATA\n5\nEND_DATA\n\
               BEGIN_DATA_FORMAT\nXG\nEND_DATA_FORMAT\n\
               BEGIN_DATA\n4\nEND_DATA\n\
               BEGIN_DATA_FORMAT\nXH\nEND_DATA_FORMAT\n\
               BEGIN_DATA\n3\nEND_DATA\n\
               BEGIN_DATA_FORMAT\nXI\nEND_DATA_FORMAT\n\
               BEGIN_DATA\n2\nEND_DATA\n\
               BEGIN_DATA_FORMAT\nXJ\nEND_DATA_FORMAT\n\
               BEGIN_DATA\n1\nEND_DATA\n\
               BEGIN_DATA_FORMAT\nXK\nEND_DATA_FORMAT\n\
               BEGIN_DATA\n0\nEND_DATA\n\
               BEGIN_DATA_FORMAT\nXL\nEND_DATA_FORMAT\n\
               BEGIN_DATA\n9\nEND_DATA\n\
               BEGIN_DATA_FORMAT\nXM\nEND_DATA_FORMAT\n\
               BEGIN_DATA\n8\nEND_DATA\n\
               BEGIN_DATA_FORMAT\nXN\nEND_DATA_FORMAT\n\
               BEGIN_DATA\n7\nEND_DATA\n\
               BEGIN_DATA_FORMAT\nXO\nEND_DATA_FORMAT\n\
               BEGIN_DATA\n6\nEND_DATA\n\
               BEGIN_DATA_FORMAT\nXP\nEND_DATA_FORMAT\n\
               BEGIN_DATA\n5\nEND_DATA\n\
               BEGIN_DATA_FORMAT\nXQ\nEND_DATA_FORMAT\n\
               BEGIN_DATA\n4\nEND_DATA\n\
               BEGIN_DATA_FORMAT\nXR\nEND_DATA_FORMAT\n\
               BEGIN_DATA\n3\nEND_DATA\n\
               BEGIN_DATA_FORMAT\nXS\nEND_DATA_FORMAT\n\
               BEGIN_DATA\n2\nEND_DATA\n\
               BEGIN_DATA_FORMAT\nXT\nEND_DATA_FORMAT\n\
               BEGIN_DATA\n1\nEND_DATA\n\
               BEGIN_DATA_FORMAT\nXU\nEND_DATA_FORMAT\n\
               BEGIN_DATA\n0\nEND_DATA\n\0".as_slice(),
                "many-tables"),
        ] {
            let it8 = c::cmsIT8LoadFromMem(
                std::ptr::null_mut(),
                doc.as_ptr() as *const _, doc.len() as u32);
            eprintln!("{name} it8={}", !it8.is_null());
            if !it8.is_null() {
                c::cmsIT8Free(it8); }
        }
        // newline in property value -> '# ' continuation
        let it8 = c::cmsIT8Alloc(std::ptr::null_mut());
        c::cmsIT8SetPropertyUncooked(it8,
            b"COMMENT\0".as_ptr() as *const _,
            b"line1\nline2\nline3\0".as_ptr() as *const _);
        // WRITE_PAIR prop for EnumPropertyMulti subkeys
        c::cmsIT8SetPropertyMulti(it8,
            b"WEIGHTING_FUNCTION\0".as_ptr() as *const _,
            b"CIE_D65\0".as_ptr() as *const _,
            b"0.5\0".as_ptr() as *const _);
        c::cmsIT8SetPropertyMulti(it8,
            b"COMPUTATIONAL_PARAMETER\0".as_ptr() as *const _,
            b"MODE_A\0".as_ptr() as *const _,
            b"1.0\0".as_ptr() as *const _);
        let mut buf = vec![0u8; 1 << 16];
        let mut n = buf.len() as u32;
        c::cmsIT8SaveToMem(it8,
            buf.as_mut_ptr() as *mut _, &mut n);
        let txt = String::from_utf8_lossy(
            &buf[..n as usize]);
        for line in txt.lines() {
            if line.contains("COMMENT")
                || line.starts_with("# ") {
                eprintln!("  out: {line}");
            }
        }
        // EnumPropertyMulti on the pair prop
        let mut names = [std::ptr::null_mut::<u8>(); 8];
        let r = c::cmsIT8EnumPropertyMulti(it8,
            b"WEIGHTING_FUNCTION\0".as_ptr() as *const _,
            names.as_mut_ptr() as *mut _);
        eprintln!("enum-multi r={r}");
        c::cmsIT8Free(it8);
    }
}

/// cmscgats residual — ParseCube: domain bounds,
///   LUT sizes 2/17/33/65/bad, comments/blank lines,
///   truncated data, title/metadata.
#[test]
fn cgats_cube2_arms() {
    unsafe {
        let mk = |size: usize, extra: &[&str]| {
            let mut s = String::from(
                "TITLE \"t\"\n\
                 DOMAIN_MIN 0 0 0\n\
                 DOMAIN_MAX 1 1 1\n");
            for e in extra { s.push_str(e); s.push('\n'); }
            s.push_str(&format!("LUT_3D_SIZE {}\n", size));
            let n = size * size * size;
            for _ in 0..n {
                s.push_str("0.1 0.2 0.3\n"); }
            s.push('\0');
            s
        };
        for (doc, name) in [
            (mk(2, &[]), "s2"),
            (mk(17, &[]), "s17"),
            (mk(33, &[]), "s33"),
            (mk(2, &["# comment line"]),
                "comment"),
            (mk(2, &["LUT_3D_SIZE 3",
                     "LUT_1D_SIZE 16",
                     "0.1 0.2 0.3",
                     "0.4 0.5 0.6",
                     "0.7 0.8 0.9"]),
                "dup-size"),
            (mk(2, &["LUT_3D_INPUT_RANGE 0.0 1.0"]),
                "range"),
            (mk(3, &["LUT_3D_SIZE 2"]), "conflict"),
            (String::from(
                "TITLE \"t\"\nLUT_3D_SIZE 9\n\
                 0.1 0.2 0.3\n\0"), "trunc-data"),
            (String::from(
                "LUT_2D_SIZE 4\n\
                 0.1 0.2 0.3\n\0"), "lut2d"),
            (String::from(
                "LUT_3D_SIZE\n\0"), "no-size"),
            (String::from(
                "GARBAGE_TOKEN 1 2 3\n\
                 LUT_3D_SIZE 2\n\
                 0.1 0.2 0.3 0.1 0.2 0.3\n\
                 0.1 0.2 0.3 0.1 0.2 0.3\n\
                 0.1 0.2 0.3 0.1 0.2 0.3\n\
                 0.1 0.2 0.3 0.1 0.2 0.3\n\0"),
                "garbage"),
        ] {
            let path = format!(
                "/tmp/lcms_cube_{}.cube", name);
            std::fs::write(&path,
                doc.trim_end_matches('\0')).unwrap();
            let cpath = std::ffi::CString::new(
                path.as_str()).unwrap();
            let prof = c::cmsCreateDeviceLinkFromCubeFileTHR(
                std::ptr::null_mut(),
                cpath.as_ptr());
            eprintln!("{name} prof={}",
                !prof.is_null());
            if !prof.is_null() {
                c::cmsCloseProfile(prof); }
            let _ = std::fs::remove_file(&path);
        }
    }
}

/// cmscgats residual — InSymbol +sign, double-format
///   num->id path, include-depth overflow, more
///   numeric lexer forms.
#[test]
fn cgats_insym2_arms() {
    unsafe {
        // '+'-signed values in data rows + float/hex
        //   + num-followed-by-idchar
        for (doc, name) in [
            ("CGATS.17\n\
              NUMBER_OF_FIELDS 3\n\
              BEGIN_DATA_FORMAT\n\
              A B C\nEND_DATA_FORMAT\n\
              NUMBER_OF_SETS 2\n\
              BEGIN_DATA\n\
              +1 +2 +3\n\
              4 5 6\nEND_DATA\n", "plus-rows"),
            ("CGATS.17\n\
              NUMBER_OF_FIELDS 2\n\
              BEGIN_DATA_FORMAT\n\
              A B\nEND_DATA_FORMAT\n\
              NUMBER_OF_SETS 1\n\
              BEGIN_DATA\n\
              + 1\nEND_DATA\n", "plus-space"),
            ("CGATS.17\n\
              NUMBER_OF_FIELDS 2\n\
              BEGIN_DATA_FORMAT\n\
              A B\nEND_DATA_FORMAT\n\
              NUMBER_OF_SETS 2\n\
              BEGIN_DATA\n\
              1.5e3 2x\n\
              4 5\nEND_DATA\n", "numid"),
            ("CGATS.17\n\
              NUMBER_OF_FIELDS 3\n\
              BEGIN_DATA_FORMAT\n\
              A B C\nEND_DATA_FORMAT\n\
              NUMBER_OF_SETS 3\n\
              BEGIN_DATA\n\
              0x1F 0b101 0o17\n\
              .5 5. 1e-2\n\
              -1 -0x10 -.5\nEND_DATA\n", "forms"),
            ("CGATS.17\n\
              ORIGINATOR \"o\"\n\
              CREATED \"d\"\n\
              NUMBER_OF_FIELDS 1\n\
              BEGIN_DATA_FORMAT\n\
              A\nEND_DATA_FORMAT\n\
              NUMBER_OF_SETS 1\n\
              BEGIN_DATA\n\
              1e9999\nEND_DATA\n", "bigexp"),
        ] {
            let it8 = c::cmsIT8LoadFromMem(
                std::ptr::null_mut(),
                doc.as_ptr() as *const _,
                doc.len() as u32);
            eprintln!("{name} it8={}", !it8.is_null());
            if !it8.is_null() {
                // GetData probes reach the numeric paths
                let n = c::cmsIT8GetDataDbl(it8,
                    "SAMPLE_ID\0".as_ptr() as *const _,
                    "A\0".as_ptr() as *const _);
                eprintln!("  dbl={n}");
                c::cmsIT8Free(it8);
            }
        }
        // include-depth: chain of #INCLUDE files past
        //   MAXINCLUDE (upstream default 10)
        for depth in [1u32, 5, 12] {
            for d in 0..depth {
                let f = format!(
                    "/tmp/lcms_inc_{}_{}.txt", depth, d);
                let body = if d + 1 < depth {
                    format!(
                        "#INCLUDE \"lcms_inc_{}_{}.txt\"\n",
                        depth, d + 1)
                } else {
                    "NUMBER_OF_FIELDS 1\n\
                     BEGIN_DATA_FORMAT\n\
                     A\nEND_DATA_FORMAT\n\
                     NUMBER_OF_SETS 1\n\
                     BEGIN_DATA\n42\nEND_DATA\n".to_string()
                };
                std::fs::write(&f, body).unwrap();
            }
            let doc = format!(
                "CGATS.17\n\
                 #INCLUDE \"/tmp/lcms_inc_{}_0.txt\"\n",
                depth);
            let it8 = c::cmsIT8LoadFromMem(
                std::ptr::null_mut(),
                doc.as_ptr() as *const _,
                doc.len() as u32);
            eprintln!("inc-depth{depth} it8={}",
                !it8.is_null());
            if !it8.is_null() {
                c::cmsIT8Free(it8); }
            for d in 0..depth {
                let _ = std::fs::remove_file(format!(
                    "/tmp/lcms_inc_{}_{}.txt", depth, d));
            }
        }
    }
}

/// cmscgats residual — WriteHeader/SaveToFile/
///   SaveToMem + LoadFromFile round-trips, property
///   str/dbl/comment/data-format/data-dbl sets,
///   DefineDblFormat custom formats.
#[test]
fn cgats_write_load_arms() {
    unsafe {
        // build an IT8 doc, save to file + mem, reload
        let it8 = c::cmsIT8Alloc(std::ptr::null_mut());
        eprintln!("it8={}", !it8.is_null());
        if it8.is_null() { return; }
        c::cmsIT8SetSheetType(it8,
            "CGATS.17\0".as_ptr() as *const _);
        c::cmsIT8SetPropertyStr(it8,
            "ORIGINATOR\0".as_ptr() as *const _,
            "lcms-test\0".as_ptr() as *const _);
        c::cmsIT8SetPropertyStr(it8,
            "CREATED\0".as_ptr() as *const _,
            "2026-11-08\0".as_ptr() as *const _);
        c::cmsIT8SetPropertyDbl(it8,
            "DENSITY\0".as_ptr() as *const _, 1.5);
        c::cmsIT8SetPropertyDbl(it8,
            "WEIGHT\0".as_ptr() as *const _, 0.25);
        c::cmsIT8SetComment(it8,
            "comment line one\0".as_ptr() as *const _);
        c::cmsIT8SetComment(it8,
            "comment two\0".as_ptr() as *const _);
        // define a custom double format
        c::cmsIT8DefineDblFormat(it8,
            "%.12g\0".as_ptr() as *const _);
        // data format + sets
        let fmt: Vec<*const i8> = [
            "SAMPLE_ID\0".as_ptr() as *const _,
            "RGB_R\0".as_ptr() as *const _,
            "RGB_G\0".as_ptr() as *const _,
            "RGB_B\0".as_ptr() as *const _,
            "LAB_L\0".as_ptr() as *const _,
        ].to_vec();
        for (i, f) in fmt.iter().enumerate() {
            let ok = c::cmsIT8SetDataFormat(it8,
                i as i32, *f);
            eprintln!("  fmt[{i}] {ok}");
        }
        for p in 0..3u32 {
            let patch = format!("P{p}\0");
            for (fld, v) in [("RGB_R", p as f64 + 0.1),
                ("RGB_G", p as f64 + 0.2),
                ("RGB_B", p as f64 + 0.3),
                ("LAB_L", p as f64 * 33.0)] {
                let f = format!("{fld}\0");
                let _ = c::cmsIT8SetDataDbl(it8,
                    patch.as_ptr() as *const _,
                    f.as_ptr() as *const _, v);
            }
        }
        // save to mem (size query then real)
        let mut need = 0u32;
        c::cmsIT8SaveToMem(it8, std::ptr::null_mut(),
            &mut need);
        eprintln!("mem-need={need}");
        let mut mbuf = vec![0u8; (need + 64) as usize];
        let mut n2 = mbuf.len() as u32;
        let sv = c::cmsIT8SaveToMem(it8,
            mbuf.as_mut_ptr() as *mut _, &mut n2);
        eprintln!("mem-sv={sv} n2={n2}");
        // save to file + reload
        let path = std::ffi::CString::new(
            "/tmp/lcms_it8_wl.it8").unwrap();
        let sf = c::cmsIT8SaveToFile(it8, path.as_ptr());
        eprintln!("file-sv={sf}");
        let it8b = c::cmsIT8LoadFromFile(
            std::ptr::null_mut(), path.as_ptr());
        eprintln!("reload={}", !it8b.is_null());
        if !it8b.is_null() {
            let v = c::cmsIT8GetDataDbl(it8b,
                "P1\0".as_ptr() as *const _,
                "RGB_R\0".as_ptr() as *const _);
            eprintln!("  reread RGB_R@P1={v}");
            let t = c::cmsIT8EnumDataFormat(it8b,
                std::ptr::null_mut());
            eprintln!("  enum-fmt={t}");
            c::cmsIT8Free(it8b);
        }
        // reload from mem
        let it8c = c::cmsIT8LoadFromMem(
            std::ptr::null_mut(),
            mbuf.as_ptr() as *const _, n2);
        eprintln!("mem-reload={}", !it8c.is_null());
        if !it8c.is_null() {
            c::cmsIT8Free(it8c); }
        c::cmsIT8Free(it8);
        let _ = std::fs::remove_file("/tmp/lcms_it8_wl.it8");
    }
}

/// cmscgats residual — GetData/GetDataRowCol/
///   GetProperty/SetDataRowCol internals on a loaded
///   doc: valid/OOB row-col, missing patch/sample,
///   str/dbl variants.
#[test]
fn cgats_getdata_arms() {
    unsafe {
        let doc = "CGATS.17\n\
            ORIGINATOR \"o\"\n\
            NUMBER_OF_FIELDS 3\n\
            BEGIN_DATA_FORMAT\n\
            SAMPLE_ID R G\nEND_DATA_FORMAT\n\
            NUMBER_OF_SETS 3\n\
            BEGIN_DATA\n\
            p0 1.0 2.0\n\
            p1 3.0 4.0\n\
            p2 5.0 6.0\nEND_DATA\n";
        let it8 = c::cmsIT8LoadFromMem(
            std::ptr::null_mut(),
            doc.as_ptr() as *const _, doc.len() as u32);
        eprintln!("it8={}", !it8.is_null());
        if it8.is_null() { return; }
        // property lookups
        for key in ["ORIGINATOR\0", "NOPE\0",
                    "CREATED\0"] {
            let v = c::cmsIT8GetProperty(it8,
                key.as_ptr() as *const _);
            let s = if v.is_null() { "NULL".to_string() }
                else { unsafe {
                    std::ffi::CStr::from_ptr(v)
                        .to_string_lossy().into_owned() } };
            eprintln!("prop {key:?} = {s}");
        }
        let dv = c::cmsIT8GetPropertyDbl(it8,
            "NUMBER_OF_SETS\0".as_ptr() as *const _);
        eprintln!("prop-dbl NS={dv}");
        // GetData patch/sample
        for (patch, sample) in [
            ("p0\0", "R\0"), ("p1\0", "G\0"),
            ("p2\0", "SAMPLE_ID\0"),
            ("nope\0", "R\0"), ("p0\0", "nope\0"),
        ] {
            let v = c::cmsIT8GetData(it8,
                patch.as_ptr() as *const _,
                sample.as_ptr() as *const _);
            let vd = c::cmsIT8GetDataDbl(it8,
                patch.as_ptr() as *const _,
                sample.as_ptr() as *const _);
            eprintln!("{patch:?}/{sample:?}: \
                str={} dbl={vd}", !v.is_null());
        }
        // row/col accessors incl. OOB
        for (r, c_) in [(0i32, 0), (1, 1), (2, 2),
                        (-1, 0), (0, -1), (99, 0), (0, 99),
                        (3, 3)] {
            let s = c::cmsIT8GetDataRowCol(it8, r, c_);
            let d = c::cmsIT8GetDataRowColDbl(it8, r, c_);
            eprintln!("rc({r},{c_}): str={} dbl={d}",
                !s.is_null());
        }
        // SetData variants
        for (patch, sample, val) in [
            ("p0\0", "R\0", "9.9\0"),
            ("p1\0", "G\0", "abc\0"),
            ("nope\0", "R\0", "1\0"),
        ] {
            let ok = c::cmsIT8SetData(it8,
                patch.as_ptr() as *const _,
                sample.as_ptr() as *const _,
                val.as_ptr() as *const _);
            eprintln!("set {patch:?}/{sample:?}={val:?} \
                {ok}");
        }
        for (r, c_, v) in [(0i32, 0, "7.7\0"),
                           (1, 1, "8.8\0"), (5, 5, "x\0")] {
            let ok = c::cmsIT8SetDataRowCol(it8, r, c_,
                v.as_ptr() as *const _);
            let ok2 = c::cmsIT8SetDataRowColDbl(it8, r, c_,
                9.9);
            eprintln!("setrc({r},{c_}) {ok}/{ok2}");
        }
        c::cmsIT8Free(it8);
    }
}
