use lcms::cnvrt;
use lcms::opt;
use lcms::profile;
use lcms::virt;
const PATH: &str = "/home/lilith/work/zen/lcms/reference/lcms2/testbed/ibm-t61.icc";
fn dump(lut: &lcms::lut::Pipeline, tag: &str) {
    let mut s = lut.first_stage();
    let mut n = 0;
    print!("{tag}: ");
    while let Some(st) = s {
        let t = st.r#type;
        print!("{:08x} ", t);
        s = st.next.as_deref();
        n += 1;
        if n > 8 { break; }
    }
    println!(" [{n} stages]");
}
fn main() {
    let p_in = profile::open_profile_from_file(None, PATH, "r").expect("in");
    let srgb = virt::create_srgb_profile(None).unwrap();
    let mut profs = vec![p_in, srgb];
    let mut bpc = [false,false];
    let intents = [0u32,0];
    let adapt = [1.0f64,1.0];
    let mut lut = cnvrt::link_profiles(None, &mut profs, &intents, &mut bpc, &adapt, 0).expect("link");
    dump(&lut, "pre-opt");
    let mut a=0u32; let mut b=0u32; let mut f=0u32;
    opt::optimize_pipeline(None, &mut lut, 0, &mut a, &mut b, &mut f);
    dump(&lut, "post-opt");
}
