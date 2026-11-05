use lcms::io1;
use lcms::types::Mat3;
use lcms::virt;
fn main() {
    // sRGB D65->D50 adapted colorant matrix (standard values)
    // instead of read_icc_matrix_rgb2xyz (private), eval the output lut
    // on a pure X/Y/Z basis vector to extract the inverse matrix columns
    let mut srgb = virt::create_srgb_profile(None).unwrap();
    let ol = io1::read_output_lut(&mut srgb, 0).expect("out lut");
    // eval output lut on basis vectors -> the invTRC(invMat * basis)
    let mut o = [0u16; 3];
    for (nm, v) in [
        ("X", [0xffffu16, 0, 0]),
        ("Y", [0, 0xffff, 0]),
        ("Z", [0, 0, 0xffff]),
        ("mid", [0x4000; 3]),
    ] {
        ol.eval16(&v, &mut o);
        println!("{nm} -> {:04x} {:04x} {:04x}", o[0], o[1], o[2]);
    }
    let _ = Mat3::default();
}
