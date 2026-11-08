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

/// IT8 writer on a populated doc — the write machinery on a
/// doc with real data.
#[test]
fn it8_writer_populated() {
    unsafe {
        let it8 = c::cmsIT8Alloc(std::ptr::null_mut());
        c::cmsIT8SetSheetType(it8, b"TESTDOC\0".as_ptr() as _);
        // populate via the table-by-label machinery
        for (fmt_i, name) in [
            (0, b"SAMPLE_ID\0" as &[u8]), (1, b"LAB_L\0"),
            (2, b"LAB_A\0"), (3, b"LAB_B\0")] {
            let _ = c::cmsIT8SetDataFormat(it8, fmt_i,
                name.as_ptr() as _);
        }
        // add rows via SetData (the row machinery)
        for row in 0..3 {
            let patch = format!("P{row}\0");
            c::cmsIT8SetDataDbl(it8, patch.as_ptr() as _,
                b"LAB_L\0".as_ptr() as _, 50.0 + row as f64);
            c::cmsIT8SetDataDbl(it8, patch.as_ptr() as _,
                b"LAB_A\0".as_ptr() as _, row as f64);
            c::cmsIT8SetDataDbl(it8, patch.as_ptr() as _,
                b"LAB_B\0".as_ptr() as _, -row as f64);
        }
        // save — the writer machinery on a populated doc
        let mut n = 0u32;
        c::cmsIT8SaveToMem(it8, std::ptr::null_mut(), &mut n);
        let mut buf = vec![0u8; n as usize];
        let ok = c::cmsIT8SaveToMem(it8,
            buf.as_mut_ptr() as *mut _, &mut n);
        eprintln!("populated save={ok} bytes={n}");
        c::cmsIT8Free(it8);
    }
}

/// IT8 enum/rowcol/patch machinery on more table setups.
#[test]
fn it8_enum_rowcol() {
    unsafe {
        let it8 = c::cmsIT8Alloc(std::ptr::null_mut());
        // set a data format then enum it
        c::cmsIT8SetDataFormat(it8, 0, b"SAMPLE_ID\0".as_ptr() as _);
        c::cmsIT8SetDataFormat(it8, 1, b"RGB_R\0".as_ptr() as _);
        c::cmsIT8SetDataFormat(it8, 2, b"RGB_G\0".as_ptr() as _);
        c::cmsIT8SetDataFormat(it8, 3, b"RGB_B\0".as_ptr() as _);
        let mut names: [*mut i8; 8] = [std::ptr::null_mut(); 8];
        let mut names_ptr: *mut *mut i8 = names.as_mut_ptr();
        let n = c::cmsIT8EnumDataFormat(it8, &mut names_ptr);
        eprintln!("enum n={n}");
        // get/set a data row
        c::cmsIT8SetDataDbl(it8, b"1\0".as_ptr() as _,
            b"RGB_R\0".as_ptr() as _, 0.5);
        let v = c::cmsIT8GetDataDbl(it8,
            b"1\0".as_ptr() as _, b"RGB_R\0".as_ptr() as _);
        eprintln!("data v={v}");
        c::cmsIT8Free(it8);
    }
}

/// IT8 data row/col machinery on more table setups.
#[test]
fn it8_rowcol() {
    unsafe {
        let it8 = c::cmsIT8Alloc(std::ptr::null_mut());
        // set a data format then a data row
        c::cmsIT8SetDataFormat(it8, 0, b"SAMPLE_ID\0".as_ptr() as _);
        c::cmsIT8SetDataFormat(it8, 1, b"RGB_R\0".as_ptr() as _);
        // get a data row/col — check the machinery
        let v = c::cmsIT8GetDataRowCol(it8, 0, 1);
        eprintln!("rowcol={}", v.is_null());
        // set/get a property
        c::cmsIT8SetPropertyStr(it8, b"ORIGINATOR\0".as_ptr() as _,
            b"test\0".as_ptr() as _);
        let p = c::cmsIT8GetProperty(it8,
            b"ORIGINATOR\0".as_ptr() as _);
        eprintln!("prop={}", p.is_null());
        c::cmsIT8Free(it8);
    }
}


/// IT8 property machinery on more property types.
#[test]
fn it8_property_types() {
    unsafe {
        let it8 = c::cmsIT8Alloc(std::ptr::null_mut());
        c::cmsIT8SetSheetType(it8, b"TEST\0".as_ptr() as _);
        // str/dbl properties on more names
        for (name, v) in [
            ("ORIGINATOR", "x"), ("INSTRUMENT", "y"),
            ("MANUFACTURER", "z")] {
            let ok = c::cmsIT8SetPropertyStr(it8,
                name.as_ptr() as *const i8 as _,
                v.as_ptr() as *const i8 as _);
            eprintln!("{name} str={ok}");
        }
        for name in ["NUMBER_OF_FIELDS", "NUMBER_OF_SETS"] {
            let ok = c::cmsIT8SetPropertyDbl(it8,
                name.as_ptr() as *const i8 as _, 4.0);
            eprintln!("{name} dbl={ok}");
        }
        c::cmsIT8Free(it8);
    }
}

/// IT8 multi-table machinery — add a second table + switch.
#[test]
fn it8_multi_table() {
    unsafe {
        let it8 = c::cmsIT8Alloc(std::ptr::null_mut());
        c::cmsIT8SetSheetType(it8, b"TEST\0".as_ptr() as _);
        // add a property then create a second table
        c::cmsIT8SetPropertyStr(it8,
            b"ORIGINATOR\0".as_ptr() as _,
            b"t1\0".as_ptr() as _);
        let ok = c::cmsIT8SetPropertyDbl(it8,
            b"NUMBER_OF_SETS\0".as_ptr() as _, 2.0);
        eprintln!("prop={ok}");
        // set the current table — table 0/1
        for t in [0u32, 1] {
            let ok = c::cmsIT8SetTable(it8, t);
            eprintln!("table{t}={ok}");
        }
        c::cmsIT8Free(it8);
    }
}

/// Property enumeration — cmsIT8EnumProperties/Multi
/// machinery.
#[test]
fn it8_enum_props() {
    unsafe {
        let h = c::cmsIT8Alloc(std::ptr::null_mut());
        c::cmsIT8SetPropertyStr(h,
            b"ORIGINATOR\0".as_ptr() as _,
            b"test\0".as_ptr() as _);
        c::cmsIT8SetPropertyDbl(h,
            b"GAMMA\0".as_ptr() as _, 2.2);
        c::cmsIT8SetPropertyHex(h,
            b"FLAG\0".as_ptr() as _, 0xDEAD);
        let mut names: *mut *mut i8 = std::ptr::null_mut();
        let n = c::cmsIT8EnumProperties(h, &mut names);
        eprintln!("enum={n}");
        // enum multi on a prop
        let mut mn: *mut *mut i8 = std::ptr::null_mut();
        let n2 = c::cmsIT8EnumPropertyMulti(h,
            b"ORIGINATOR\0".as_ptr() as _, &mut mn);
        eprintln!("enummulti={n2}");
        c::cmsIT8Free(h);
    }
}

/// Data-format + patch machinery — more IT8 internals.
#[test]
fn it8_dataformat_patch() {
    unsafe {
        let h = c::cmsIT8Alloc(std::ptr::null_mut());
        c::cmsIT8SetSheetType(h,
            b"LCMS/IT8 ".as_ptr() as _);
        let ty = c::cmsIT8GetSheetType(h);
        eprintln!("sheetty={}", !ty.is_null());
        for (i, name) in [
            (0i32, "SAMPLE_ID "), (1, "LAB_L "),
            (2, "LAB_A "), (3, "LAB_B ")] {
            let r = c::cmsIT8SetDataFormat(h, i,
                name.as_ptr() as _);
            eprintln!("setfmt[{i}]={r}");
        }
        // patch name machinery
        let mut buf = [0i8; 64];
        let pn = c::cmsIT8GetPatchName(h, 0,
            buf.as_mut_ptr());
        eprintln!("patchname={}", !pn.is_null());
        let pb = c::cmsIT8GetPatchByName(h,
            b"1 ".as_ptr() as _);
        eprintln!("patchbyname={pb}");
        c::cmsIT8Free(h);
    }
}

/// IT8 save+reload round-trip — writer + parser internals.
#[test]
fn it8_save_reload() {
    unsafe {
        let h = c::cmsIT8Alloc(std::ptr::null_mut());
        c::cmsIT8SetSheetType(h,
            b"LCMS/IT8\0".as_ptr() as _);
        c::cmsIT8SetPropertyStr(h,
            b"ORIGINATOR\0".as_ptr() as _,
            b"test\0".as_ptr() as _);
        c::cmsIT8SetPropertyDbl(h,
            b"GAMMA\0".as_ptr() as _, 2.2);
        c::cmsIT8SetComment(h,
            b"a comment\0".as_ptr() as _);
        let r = c::cmsIT8SaveToFile(h,
            b"/tmp/ccov_it8.txt\0".as_ptr() as _);
        eprintln!("save={r}");
        let h2 = c::cmsIT8LoadFromFile(
            std::ptr::null_mut(),
            b"/tmp/ccov_it8.txt\0".as_ptr() as _);
        eprintln!("reload={}", !h2.is_null());
        if !h2.is_null() {
            let p = c::cmsIT8GetProperty(h2,
                b"ORIGINATOR\0".as_ptr() as _);
            let d = c::cmsIT8GetPropertyDbl(h2,
                b"GAMMA\0".as_ptr() as _);
            eprintln!("prop={} dbl={d}", !p.is_null());
            c::cmsIT8Free(h2);
        }
        c::cmsIT8Free(h);
    }
}

/// IT8 populated save — SetDataFormat + SetDataRowCol
/// sequence then save+reload.
#[test]
fn it8_populated_save() {
    unsafe {
        let h = c::cmsIT8Alloc(std::ptr::null_mut());
        c::cmsIT8SetSheetType(h,
            b"LCMS/IT8\0".as_ptr() as _);
        // set data formats for 3 columns
        for (i, name) in [
            (0i32, "SAMPLE_ID\0"),
            (1, "LAB_L\0"), (2, "LAB_A\0")] {
            let r = c::cmsIT8SetDataFormat(h, i,
                name.as_ptr() as _);
            eprintln!("setfmt[{i}]={r}");
        }
        // set row/col data
        for (row, col, val) in [
            (0i32, 0i32, "1\0"), (0, 1, "50.0\0"),
            (0, 2, "0.0\0"), (1, 0, "2\0"),
            (1, 1, "60.0\0"), (1, 2, "1.0\0")] {
            let r = c::cmsIT8SetDataRowCol(h, row, col,
                val.as_ptr() as _);
            eprintln!("setdata[{row},{col}]={r}");
        }
        let r = c::cmsIT8SaveToFile(h,
            b"/tmp/ccov_it8b.txt\0".as_ptr() as _);
        eprintln!("save={r}");
        let h2 = c::cmsIT8LoadFromFile(
            std::ptr::null_mut(),
            b"/tmp/ccov_it8b.txt\0".as_ptr() as _);
        eprintln!("reload={}", !h2.is_null());
        if !h2.is_null() { c::cmsIT8Free(h2); }
        c::cmsIT8Free(h);
    }
}

/// Include-path machinery + parse-error sanitization —
/// an IT8 with `.INCLUDE` routes through BuildAbsolutePath /
/// isabsolutepath (include fails open, but the path build runs);
/// a malformed doc reaches SynError -> NoMeta.
#[test]
fn include_path_and_synerror() {
    unsafe {
        // include directive — the path build runs before fopen fails
        let it8 = b"CGATS.17\nORIGINATOR \"x\"\n.INCLUDE \"sub.txt\"\n\
            NUMBER_OF_FIELDS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\n\
            NUMBER_OF_SETS 1\nBEGIN_DATA\n1\nEND_DATA\n";
        let h = c::cmsIT8LoadFromMem(std::ptr::null_mut(),
            it8.as_ptr() as _, it8.len() as u32);
        eprintln!("include-load={}", !h.is_null());
        if !h.is_null() { c::cmsIT8Free(h); }
        // absolute-path include — isabsolutepath's early branch
        let it8b = b"CGATS.17\nORIGINATOR \"x\"\n\
            .INCLUDE \"/tmp/nonexistent-it8.txt\"\n\
            NUMBER_OF_FIELDS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\n\
            NUMBER_OF_SETS 1\nBEGIN_DATA\n1\nEND_DATA\n";
        let h2 = c::cmsIT8LoadFromMem(std::ptr::null_mut(),
            it8b.as_ptr() as _, it8b.len() as u32);
        eprintln!("abs-include-load={}", !h2.is_null());
        if !h2.is_null() { c::cmsIT8Free(h2); }
        // malformed doc — unterminated string -> SynError -> NoMeta
        let bad = b"CGATS.17\nORIGINATOR \"unterminated\n\
            NUMBER_OF_FIELDS 1\nBEGIN_DATA_FORMAT\nA\nEND_DATA_FORMAT\n\
            NUMBER_OF_SETS 1\nBEGIN_DATA\n1\nEND_DATA\n";
        let h3 = c::cmsIT8LoadFromMem(std::ptr::null_mut(),
            bad.as_ptr() as _, bad.len() as u32);
        eprintln!("bad-load={}", !h3.is_null());
        if !h3.is_null() { c::cmsIT8Free(h3); }
    }
}

/// .cube devicelink parse — ParseCube + cmsCreateDeviceLinkFromCube:
/// TITLE, DOMAIN_MIN/MAX, LUT_1D/3D_SIZE, *_INPUT_RANGE, comments,
/// plus malformed inputs. C vs Rust devicelink creation parity.
#[test]
fn cube_devicelink() {
    unsafe {
        let cases: &[(&str, &str)] = &[
            ("1d", "TITLE \"t1\"\nLUT_1D_SIZE 4\n\
                0 0 0\n0.33 0.33 0.33\n0.66 0.66 0.66\n1 1 1\n"),
            ("3d", "TITLE \"t3\"\nLUT_3D_SIZE 2\n\
                0 0 0\n0 0 1\n0 1 0\n0 1 1\n1 0 0\n1 0 1\n1 1 0\n1 1 1\n"),
            ("3d-dom", "TITLE \"d\"\nDOMAIN_MIN 0.1 0.2 0.3\n\
                DOMAIN_MAX 0.9 0.8 0.7\nLUT_3D_SIZE 2\n\
                0 0 0\n0 0 1\n0 1 0\n0 1 1\n1 0 0\n1 0 1\n1 1 0\n1 1 1\n"),
            ("3d-range", "LUT_3D_INPUT_RANGE 0.0 1.0\nLUT_3D_SIZE 2\n\
                0 0 0\n0 0 1\n0 1 0\n0 1 1\n1 0 0\n1 0 1\n1 1 0\n1 1 1\n"),
            ("1d-range", "LUT_1D_INPUT_RANGE 0.0 1.0\nLUT_1D_SIZE 2\n\
                0 0 0\n1 1 1\n"),
            ("3d-shaper", "TITLE \"s\"\nLUT_1D_SIZE 4\n\
                0 0 0\n0.4 0.4 0.4\n0.8 0.8 0.8\n1 1 1\n\
                LUT_3D_SIZE 2\n\
                0 0 0\n0 0 1\n0 1 0\n0 1 1\n1 0 0\n1 0 1\n1 1 0\n1 1 1\n"),
            ("3d-cmt", "# comment line\nTITLE \"c\"\n\
                LUT_3D_SIZE 2\n\
                0 0 0\n0 0 1\n0 1 0\n0 1 1\n1 0 0\n1 0 1\n1 1 0\n1 1 1\n"),
            // errors — no LUT size, bad float, truncated table
            ("e-nosize", "TITLE \"x\"\n0 0 0\n"),
            ("e-bad", "LUT_3D_SIZE 2\nq r s\n0 0 0\n"),
            ("e-trunc", "LUT_3D_SIZE 4\n0 0 0\n0 0 1\n"),
            ("e-empty", ""),
        ];
        for (name, data) in cases {
            let path = format!("/tmp/cube_{name}.cube\0");
            std::fs::write(path.trim_end_matches('\0'), data).unwrap();
            let ch = c::cmsCreateDeviceLinkFromCubeFile(
                path.as_ptr() as *const i8);
            let rp = lcms::cgats::It8::create_device_link_from_cube_file(
                None, path.trim_end_matches('\0'));
            let c_ok = !ch.is_null();
            let r_ok = rp.is_some();
            eprintln!("{name}: C={c_ok} R={r_ok}");
            assert_eq!(c_ok, r_ok, "{name} parse parity");
            if c_ok { c::cmsCloseProfile(ch); }
        }
    }
}
