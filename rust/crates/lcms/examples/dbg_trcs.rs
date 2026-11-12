use lcms::profile;
use lcms::signatures::tag;
use lcms::tagtypes::TagObject;
const PATH: &str = "/home/lilith/work/zen/lcms/reference/lcms2/testbed/ibm-t61.icc";
fn main() {
    let mut p = profile::open_profile_from_file(None, PATH, "r").expect("open");
    for (nm, sig) in [
        ("rTRC", tag::SigRedTRCTag),
        ("gTRC", tag::SigGreenTRCTag),
        ("bTRC", tag::SigBlueTRCTag),
    ] {
        if let Some(TagObject::Curve(t)) = profile::cms_read_tag(&mut p, sig) {
            println!(
                "{nm}[0]={} t16[0..3]={:?} eval16(0)={:04x}",
                t.table16.first().copied().unwrap_or(0),
                &t.table16[..3.min(t.table16.len())],
                t.eval16(0)
            );
        }
    }
    // white point
    if let Some(TagObject::Xyz(x)) = profile::cms_read_tag(&mut p, tag::SigMediaWhitePointTag) {
        println!("wtpt = {:?} {:?} {:?}", x.X, x.Y, x.Z);
    }
}
