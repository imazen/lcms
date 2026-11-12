use lcms::cnvrt;
use lcms::lut::StageData;
use lcms::profile;
const P1: &str = "/tmp/tc_run/test5.icc";
const P2: &str = "/tmp/tc_run/test3.icc";
fn main() {
    let p_in = profile::open_profile_from_file(None, P1, "r").expect("in");
    let p_out = profile::open_profile_from_file(None, P2, "r").expect("out");
    let mut profs = vec![p_in, p_out];
    let mut bpc = [false, false];
    let intents = [0u32, 0];
    let adapt = [1.0f64, 1.0];
    let lut = cnvrt::link_profiles(None, &mut profs, &intents, &mut bpc, &adapt, 0).expect("link");
    let mut s = lut.elements.as_deref();
    let mut i = 0;
    while let Some(st) = s {
        match &st.data {
            StageData::ToneCurves(c) => println!("stage {i}: curves({})", c.len()),
            StageData::CLut(d) => println!(
                "stage {i}: clut n_in={} n_out={} tab16={} tabf32={}",
                d.n_inputs,
                d.n_outputs,
                d.tab16.is_some(),
                d.tab_float.is_some()
            ),
            StageData::Matrix { .. } => println!("stage {i}: matrix"),
            _ => println!("stage {i}: other"),
        }
        s = st.next.as_deref();
        i += 1;
    }
}
