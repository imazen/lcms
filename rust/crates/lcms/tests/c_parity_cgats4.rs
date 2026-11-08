//! cmscgats IT8 sheet-type + patch machinery beyond cgats3.

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
fn it8_sheet_patch() {
    unsafe {
        let ch = c::cmsIT8Alloc(std::ptr::null_mut());
        let mut rh = cgats::It8::alloc(None);
        // sheet-type set/get parity
        for ty in ["CGATS.5", "IT8", "CUSTOM"] {
            assert_eq!(c::cmsIT8SetSheetType(ch, cs8(ty)) != 0,
                rh.set_sheet_type(ty), "sheet type {ty}");
        }
        // data-format then patch-name/by-name parity
        for (i, sample) in ["R", "G", "B"].iter().enumerate() {
            let _ = c::cmsIT8SetDataFormat(ch, i as i32, cs8(sample));
            let _ = rh.set_data_format_export(i, sample);
        }
        // patch-by-name on the empty table
        for patch in ["0", "1", "MISSING"] {
            assert_eq!(c::cmsIT8GetPatchByName(ch, cs8(patch)),
                rh.get_patch_by_name(patch), "patch {patch}");
        }
        // patch name for index 0
        let pn = c::cmsIT8GetPatchName(ch, 0, std::ptr::null_mut());
        let cpn = if pn.is_null() { String::new() } else {
            std::ffi::CStr::from_ptr(pn).to_string_lossy().into_owned()
        };
        assert_eq!(cpn, rh.get_patch_name(0).unwrap_or_default(),
            "patch name 0");
        c::cmsIT8Free(ch);
    }
}
