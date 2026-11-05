use lcms::virt;
fn main() {
    let p1 = virt::create_lab4_profile(None, None).expect("lab");
    let p2 = virt::create_srgb_profile(None).expect("srgb");
    let mut profs = vec![p1, p2];
    let mut bpc = [false, false];
    let intents = [0u32, 0];
    let adapt = [1.0f64, 1.0];
    let t = lcms::xform::create_extended_transform(
        None,
        &mut profs,
        &mut bpc,
        &intents,
        &adapt,
        None,
        0,
        10 << 16 | 3 << 3 | 2,
        4 << 16 | 3 << 3 | 1,
        0,
    );
    println!("transform: {:?}", t.is_some());
}
