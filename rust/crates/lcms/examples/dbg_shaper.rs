use lcms::io1;
use lcms::profile;
use lcms::signatures::tag;
use lcms::tagtypes::TagObject;
const PATH: &str = "/home/lilith/work/zen/lcms/reference/lcms2/testbed/ibm-t61.icc";
fn main() {
    let mut p = profile::open_profile_from_file(None, PATH, "r").expect("open");
    // dump the matrix-shaper's colorant matrix + the encoded XYZ at 0
    let mut m = [[0f64; 3]; 3];
    let mut i = 0;
    for sig in [
        tag::SigRedColorantTag,
        tag::SigGreenColorantTag,
        tag::SigBlueColorantTag,
    ] {
        if let Some(TagObject::Xyz(x)) = profile::cms_read_tag(&mut p, sig) {
            m[i] = [x.X, x.Y, x.Z];
            i += 1;
        }
    }
    println!("colorant cols r/g/b = {:?}", m);
    // expected XYZ at input0 = trc[0]*sum(cols) each ch, encodeable = XYZ/1.97*0xffff
    let t0 = 53.0 / 65535.0;
    let exp: Vec<f64> = (0..3).map(|k| t0 * (m[0][k] + m[1][k] + m[2][k])).collect();
    println!("expected XYZ at 0 (per-channel trc=53): {:?}", exp);
    let lut = io1::read_input_lut(&mut p, 0).expect("lut");
    let mut out = [0u16; 3];
    lut.eval16(&[0, 0, 0], &mut out);
    println!(
        "input-lut PCS at 0: {:04x} {:04x} {:04x}",
        out[0], out[1], out[2]
    );
    lut.eval16(&[0xffff, 0xffff, 0xffff], &mut out);
    println!(
        "input-lut PCS at max: {:04x} {:04x} {:04x}",
        out[0], out[1], out[2]
    );
}
