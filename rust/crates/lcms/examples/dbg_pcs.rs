use lcms::io1;
use lcms::profile;

const PATH: &str = "/home/lilith/work/zen/lcms/reference/lcms2/testbed/ibm-t61.icc";

fn main() {
    let mut p = profile::open_profile_from_file(None, PATH, "r").expect("open");
    println!("profile class={:#x} pcs={:#x} version={:.1}", p.device_class, p.pcs, p.version);
    let lut = io1::read_input_lut(&mut p, 0).expect("input lut");
    let mut out = [0u16; 3];
    for inp in [[0u16,0,0],[0x4000,0x4000,0x4000],[0xffff,0xffff,0xffff]] {
        lut.eval16(&inp, &mut out);
        println!("in {:04x} -> pcs {:04x} {:04x} {:04x}", inp[0], out[0], out[1], out[2]);
    }
}
