use lcms::profile;
const PATH: &str = "/home/lilith/work/zen/lcms/reference/lcms2/testbed/test2.icc";
fn main() {
    let mut p = profile::open_profile_from_file(None, PATH, "r").unwrap();
    let tac = lcms::cnvrt::dbg_detect_tac(&mut p);
    println!("detect_tac(test2) = {tac}");
}
