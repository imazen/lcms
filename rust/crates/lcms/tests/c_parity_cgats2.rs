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
