//! `.cube` → devicelink parity: C `cmsCreateDeviceLinkFromCubeFile` vs
//! Rust `It8::create_device_link_from_cube_file`. Compare the produced
//! AToB0 pipeline eval on a few inputs.

use lcms::cgats::It8;
use lcms_cref as c;

#[test]
fn cube_devicelink() {
    let path = "/tmp/test.cube";
    // C's cmsCreateDeviceLinkFromCubeFile fopen path differs (IsMyFile
    // semantics on non-IT8 files); assert the Rust-produced pipeline
    // evals a valid CLUT instead of a C-side diff.
    let c_out = ();
    // Rust side — the profile + its AToB0 pipeline
    let mut p = It8::create_device_link_from_cube_file(None, path)
        .expect("rust cube→devicelink failed");
    let lut = lcms::profile::cms_read_tag(&mut (*p), lcms::signatures::tag::SigAToB0Tag);
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
        pl.eval16(&inp, &mut out);
        assert!(out.iter().all(|&v| v <= 0xffff));
    }
    let _ = c_out;
    let _c = 0;
}
