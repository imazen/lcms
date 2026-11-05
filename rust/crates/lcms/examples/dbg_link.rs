use lcms::cnvrt;
use lcms::profile;
use lcms::virt;
const PATH: &str = "/home/lilith/work/zen/lcms/reference/lcms2/testbed/ibm-t61.icc";
fn main() {
    let p_in = profile::open_profile_from_file(None, PATH, "r").unwrap();
    let srgb = virt::create_srgb_profile(None).unwrap();
    let mut profs = vec![p_in, srgb];
    let mut bpc = [false, false];
    let intents = [0u32, 0];
    let adapt = [1.0f64, 1.0];
    let lut = cnvrt::link_profiles(None, &mut profs, &intents, &mut bpc, &adapt, 0).expect("link");
    println!("bpc after link: {:?}", bpc);
    let mut o = [0u16; 3];
    lut.eval16(&[0, 0, 0], &mut o);
    println!("linked eval16 at 0: {:04x} {:04x} {:04x}", o[0], o[1], o[2]);
    lut.eval16(&[0x4000, 0x4000, 0x4000], &mut o);
    println!(
        "linked eval16 at mid: {:04x} {:04x} {:04x}",
        o[0], o[1], o[2]
    );
}
