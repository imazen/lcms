use lcms::cgats::It8;
fn main() {
    let p = It8::create_device_link_from_cube_file(None, "/tmp/test.cube");
    println!("cube->devicelink: {:?}", p.is_some());
    if let Some(mut p) = p {
        let lut = lcms::profile::cms_read_tag(&mut p, lcms::signatures::tag::SigAToB0Tag);
        if let Some(lcms::tagtypes::TagObject::LutA2B(pl)) = lut {
            let mut out = [0u16; 3];
            pl.eval16(&[0x4000, 0x4000, 0x4000], &mut out);
            println!(
                "eval 0x4000 -> {:04x} {:04x} {:04x}",
                out[0], out[1], out[2]
            );
        }
    }
}
