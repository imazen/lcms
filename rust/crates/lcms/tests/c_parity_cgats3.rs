//! cmscgats IT8 property/data machinery: enum properties + data
//! formats, row/col get/set, patch name/by-name, data-format find —
//! beyond the writer/properties covered by cgats2.

use lcms::cgats;
use lcms_cref as c;
use std::ffi::c_void;

fn cs8(s: &str) -> *const i8 {
    thread_local! {
        static BUF: std::cell::RefCell<Vec<u8>> = std::cell::RefCell::new(Vec::new());
    }
    BUF.with(|b| {
        let mut b = b.borrow_mut();
        b.clear();
        b.extend_from_slice(s.as_bytes());
        b.push(0);
        b.as_ptr() as *const i8
    })
}

#[test]
fn it8_property_matrix() {
    unsafe {
        let ch = c::cmsIT8Alloc(std::ptr::null_mut());
        let mut rh = cgats::It8::alloc(None);
        // set/get property str/dbl/hex/multi/uncooked matrix
        for (prop, val) in [
            ("DESCRIPTOR", "Test IT8"),
            ("ORIGINATOR", "lcms"),
            ("MANUFACTURER", "imazen"),
        ] {
            assert_eq!(
                c::cmsIT8SetPropertyStr(ch, cs8(prop), cs8(val)) != 0,
                rh.set_property_str(prop, val),
                "set prop {prop}"
            );
        }
        for (prop, val) in [("LUMINANCE_XYZ", 0.9642f64), ("WHITE_POINT", 1.0)] {
            assert_eq!(
                c::cmsIT8SetPropertyDbl(ch, cs8(prop), val) != 0,
                rh.set_property_dbl(prop, val),
                "set dbl {prop}"
            );
        }
        for (prop, val) in [("PANTONE", 0x1234u32), ("COLOR", 0xABCD)] {
            assert_eq!(
                c::cmsIT8SetPropertyHex(ch, cs8(prop), val) != 0,
                rh.set_property_hex(prop, val),
                "set hex {prop}"
            );
        }
        assert_eq!(
            c::cmsIT8SetPropertyMulti(ch, cs8("M"), cs8("en"), cs8("hello")) != 0,
            rh.set_property_multi("M", "en", "hello"),
            "set multi"
        );
        assert_eq!(
            c::cmsIT8SetPropertyUncooked(ch, cs8("RAW"), cs8("  x  ")) != 0,
            rh.set_property_uncooked("RAW", "  x  "),
            "set uncooked"
        );
        // get-property parity
        // C's GetProperty only exposes dbl/hex-set simple properties —
        // reserved/uncooked/multi props live in separate stores
        for prop in ["LUMINANCE_XYZ", "PANTONE"] {
            let cp = c::cmsIT8GetProperty(ch, cs8(prop));
            let cstr = if cp.is_null() {
                String::new()
            } else {
                std::ffi::CStr::from_ptr(cp).to_string_lossy().into_owned()
            };
            let rstr = rh.get_property(prop).unwrap_or_default();
            assert_eq!(cstr, rstr, "get prop {prop}");
        }
        // enum properties — count parity
        let mut cslot: *mut *mut i8 = std::ptr::null_mut();
        let cn = c::cmsIT8EnumProperties(ch, &mut cslot);
        let rn = rh.enum_properties().len() as u32;
        assert_eq!(cn, rn, "enum properties count");
        c::cmsIT8Free(ch);
    }
}

#[test]
fn it8_data_matrix() {
    unsafe {
        let ch = c::cmsIT8Alloc(std::ptr::null_mut());
        let mut rh = cgats::It8::alloc(None);
        // data format set + find
        for (i, sample) in ["RGB_R", "RGB_G", "RGB_B", "LAB_L", "LAB_A"]
            .iter()
            .enumerate()
        {
            assert_eq!(
                c::cmsIT8SetDataFormat(ch, i as i32, cs8(sample)) != 0,
                rh.set_data_format_export(i, sample),
                "set fmt {sample}"
            );
        }
        for sample in ["RGB_R", "LAB_A", "MISSING"] {
            assert_eq!(
                c::cmsIT8FindDataFormat(ch, cs8(sample)),
                rh.find_data_format(sample),
                "find fmt {sample}"
            );
        }
        // enum data format — count parity
        let mut cslot: *mut *mut i8 = std::ptr::null_mut();
        let cn = c::cmsIT8EnumDataFormat(ch, &mut cslot);
        let rn = rh.enum_data_format().len() as i32;
        assert_eq!(cn, rn, "enum data format count");
        // row/col get/set — need a table first
        let _ = c::cmsIT8SetDataRowCol(ch, 0, 0, cs8("42.5"));
        let _ = rh.set_data_by_name("0", "RGB_R", "42.5");
        // row/col dbl get parity
        for row in 0..4 {
            for col in 0..5 {
                let cv = c::cmsIT8GetDataRowColDbl(ch, row, col);
                let rv = rh.get_data_row_col_dbl(row as usize, col as usize);
                assert_eq!(cv, rv, "rowcol dbl {row},{col}");
            }
        }
        // patch name/by-name parity on a table with patches
        let pn = c::cmsIT8GetPatchName(ch, 0, std::ptr::null_mut());
        let cpn = if pn.is_null() {
            String::new()
        } else {
            std::ffi::CStr::from_ptr(pn).to_string_lossy().into_owned()
        };
        let rpn = rh.get_patch_name(0).unwrap_or_default();
        assert_eq!(cpn, rpn, "patch name 0");
        c::cmsIT8Free(ch);
    }
}
