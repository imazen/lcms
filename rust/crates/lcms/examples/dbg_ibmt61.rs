use lcms::profile;
use lcms::signatures::tag;
use lcms::tagtypes::TagObject;

const PATH: &str = "/home/lilith/work/zen/lcms/reference/lcms2/testbed/ibm-t61.icc";

fn main() {
    let mut p = profile::open_profile_from_file(None, PATH, "r").expect("open");
    // rTRC eval at a few points
    let trc = profile::cms_read_tag(&mut p, tag::SigRedTRCTag);
    let tc = match &trc {
        Some(TagObject::Curve(t)) | Some(TagObject::ParametricCurve(t)) => Some(t),
        _ => None,
    };
    if let Some(tc) = tc {
        for v in [0u16, 256, 1024, 0x4000, 0xffff] {
            println!("rTRC[{:04x}] = {:04x}", v, tc.eval16(v));
        }
        // first few table entries
        if !tc.table16.is_empty() {
            println!(
                "rTRC table[0..4] = {:?}",
                &tc.table16[..4.min(tc.table16.len())]
            );
        }
        println!("rTRC n_entries={}", tc.table16.len());
    }
    // colorant matrix
    for sig in [
        tag::SigRedColorantTag,
        tag::SigGreenColorantTag,
        tag::SigBlueColorantTag,
    ] {
        if let Some(TagObject::Xyz(x)) = profile::cms_read_tag(&mut p, sig) {
            println!("{sig:?} = {:.6} {:.6} {:.6}", x.X, x.Y, x.Z);
        }
    }
}
