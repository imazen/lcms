//! IT8/CGATS data-access parity: C `cmsIT8LoadFromMem`+`cmsIT8GetData`
//! vs Rust `It8::load_from_mem`+`get_data_by_name`.

use lcms_cref as c;

const IT8_DOC: &[u8] = b"IT8.7/2\n\
\tORIGINATOR\t\"lcms test\"\n\
\tDESCRIPTOR\t\"test target\"\n\
\tCREATED\t\t\"2026-01-01\"\n\
NUMBER_OF_FIELDS\t4\n\
BEGIN_DATA_FORMAT\n\
SAMPLE_ID\tLAB_L\tLAB_A\tLAB_B\n\
END_DATA_FORMAT\n\
NUMBER_OF_SETS\t3\n\
BEGIN_DATA\n\
1\t10.5\t20.5\t30.5\n\
2\t40.5\t50.5\t60.5\n\
3\t70.5\t80.5\t90.5\n\
END_DATA\n";

#[test]
fn it8_parse_and_access() {
    let rust =
        lcms::cgats::It8::load_from_mem(None, IT8_DOC).expect("rust cmsIT8LoadFromMem failed");
    // C side — check each data cell
    unsafe {
        let h = c::cmsIT8LoadFromMem(
            std::ptr::null_mut(),
            IT8_DOC.as_ptr() as *const _,
            IT8_DOC.len() as u32,
        );
        assert!(!h.is_null(), "C cmsIT8LoadFromMem failed");
        for patch in ["1", "2", "3"] {
            for field in ["SAMPLE_ID", "LAB_L", "LAB_A", "LAB_B"] {
                let cp = std::ffi::CString::new(patch).unwrap();
                let cf = std::ffi::CString::new(field).unwrap();
                let p = c::cmsIT8GetData(h, cp.as_ptr(), cf.as_ptr());
                let cv = if p.is_null() {
                    String::new()
                } else {
                    std::ffi::CStr::from_ptr(p).to_string_lossy().to_string()
                };
                let rv = rust.get_data_by_name(patch, field).unwrap_or_default();
                assert_eq!(cv, rv, "{patch}.{field}");
            }
        }
        c::cmsIT8Free(h);
    }
}
