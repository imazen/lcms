use lcms::io1;
use lcms::virt;
fn main() {
    let mut srgb = virt::create_srgb_profile(None).unwrap();
    println!("srgb pcs={:#x}", srgb.pcs);
    let ol = io1::read_output_lut(&mut srgb, 0).expect("out lut");
    let mut rgb = [0u16;3];
    // ibm-t61 input-lut PCS at 0 = XYZ [0x002b,0x002b,0x0058]
    ol.eval16(&[0x002b,0x002b,0x0058], &mut rgb);
    println!("sRGB out at XYZ[002b,002b,0058]: {:04x} {:04x} {:04x}", rgb[0],rgb[1],rgb[2]);
    // full-black XYZ
    ol.eval16(&[0,0,0], &mut rgb);
    println!("sRGB out at XYZ[0]: {:04x} {:04x} {:04x}", rgb[0],rgb[1],rgb[2]);
}
