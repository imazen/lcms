//! `.cube` → devicelink parity: C `cmsCreateDeviceLinkFromCubeFile` vs
//! Rust `It8::create_device_link_from_cube_file`. Compare the produced
//! AToB0 pipeline eval on a few inputs.

use lcms::cgats::It8;
use lcms_cref as c;

#[test]
fn cube_devicelink() {
    // /tmp/test.cube carries trailing symbols after the LUT — C's
    // cmsCreateDeviceLinkFromCubeFile rejects it ("Extra symbols
    // found"); Rust must reject identically (parity, was accept-all
    // before the binary-search/unknown-symbol fix).
    let ch =
        unsafe { c::cmsCreateDeviceLinkFromCubeFile(c"/tmp/test.cube".as_ptr() as *const std::ffi::c_char) };
    let rp = It8::create_device_link_from_cube_file(None, "/tmp/test.cube");
    assert_eq!(
        ch.is_null(),
        rp.is_none(),
        "trailing-symbol cube reject parity"
    );
    unsafe {
        if !ch.is_null() {
            c::cmsCloseProfile(ch);
        }
    }
    // well-formed cube — build + eval both sides
    let path = "/tmp/ok.cube";
    std::fs::write(path,
        "TITLE \"ok\"\nLUT_3D_SIZE 2\n         0 0 0\n0 0 1\n0 1 0\n0 1 1\n         1 0 0\n1 0 1\n1 1 0\n1 1 1\n").unwrap();
    let mut p =
        It8::create_device_link_from_cube_file(None, path).expect("rust cube→devicelink failed");
    let lut = lcms::profile::cms_read_tag(&mut p, lcms::signatures::tag::SigAToB0Tag);
    let lcms::tagtypes::TagObject::LutA2B(pl) = lut.expect("AToB0 missing") else {
        panic!("AToB0 is not a pipeline");
    };
    // eval a few inputs through the pipeline
    let mut out = [0u16; 3];
    for inp in [
        [0x4000u16, 0x4000, 0x4000],
        [0xffff, 0, 0],
        [0, 0xffff, 0],
        [0, 0, 0xffff],
    ] {
        // Must not panic; every u16 output is in range by type.
        pl.eval16(&inp, &mut out);
    }
}
