use lcms::cnvrt;
use lcms::opt::{self, OptData};
use lcms::profile;
use lcms::virt;
const PATH: &str = "/home/lilith/work/zen/lcms/reference/lcms2/testbed/ibm-t61.icc";
fn main() {
    let p_in = profile::open_profile_from_file(None, PATH, "r").expect("in");
    let srgb = virt::create_srgb_profile(None).unwrap();
    let mut profs = vec![p_in, srgb];
    let mut bpc = [false,false];
    let intents = [0u32,0];
    let adapt = [1.0f64,1.0];
    let mut lut = cnvrt::link_profiles(None, &mut profs, &intents, &mut bpc, &adapt, 0).expect("link");
    let mut iformat = 4u32<<16 | 3<<3 | 1;
    let mut oformat = 4u32<<16 | 3<<3 | 1;
    let mut flags = 0u32;
    opt::optimize_pipeline(None, &mut lut, 0, &mut iformat, &mut oformat, &mut flags);
    let mut o=[0u16;3];
    lut.eval16(&[0,0,0],&mut o);
    println!("optimized eval16 at 0: {:04x} {:04x} {:04x}",o[0],o[1],o[2]);
    let name = match lut.opt_data {
        Some(OptData::Curves16(_)) => "Curves16",
        Some(OptData::MatShaper(_)) => "MatShaper",
        Some(OptData::Prelin16(_)) => "Prelin16",
        Some(OptData::Prelin8(_)) => "Prelin8",
        Some(OptData::ClutOnly(_)) => "ClutOnly",
        Some(OptData::Identity) => "Identity",
        None => "none",
    };
    println!("opt picked: {name}");
}
