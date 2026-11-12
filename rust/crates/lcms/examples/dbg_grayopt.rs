use lcms::{
    cnvrt, gamma,
    opt::{self, OptData},
    virt,
};
fn main() {
    let g22 = gamma::build_gamma(None, 2.2).unwrap();
    let g30 = gamma::build_gamma(None, 3.0).unwrap();
    let p1 = virt::create_gray_profile(None, Some(&lcms::wtpnt::d50_xyy()), Some(&g22)).unwrap();
    let p2 = virt::create_gray_profile(None, Some(&lcms::wtpnt::d50_xyy()), Some(&g30)).unwrap();
    let mut profs = vec![p1, p2];
    let mut bpc = [false, false];
    let intents = [0u32, 0];
    let adapt = [1.0f64, 1.0];
    let mut lut =
        cnvrt::link_profiles(None, &mut profs, &intents, &mut bpc, &adapt, 0).expect("link");
    let mut iformat = 3u32 << 16 | 1 << 3 | 2;
    let mut oformat = 3u32 << 16 | 1 << 3 | 2;
    let mut flags = 0u32;
    opt::optimize_pipeline(None, &mut lut, 0, &mut iformat, &mut oformat, &mut flags);
    let name = match lut.opt_data {
        Some(OptData::Curves16(_)) => "Curves16",
        Some(OptData::MatShaper(_)) => "MatShaper",
        Some(OptData::Prelin16(_)) => "Prelin16",
        Some(OptData::Prelin8(_)) => "Prelin8",
        Some(OptData::ClutOnly(_)) => "ClutOnly",
        Some(OptData::Identity) => "Identity",
        None => "none",
    };
    println!("gray->gray opt picked: {name}");
}
