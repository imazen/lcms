use lcms::cgats::It8;
const D: &[u8] = b"IT8.7/2\n\
\tORIGINATOR\t\"lcms test\"\n\
NUMBER_OF_FIELDS\t4\n\
BEGIN_DATA_FORMAT\n\
SAMPLE_ID\tLAB_L\tLAB_A\tLAB_B\n\
END_DATA_FORMAT\n\
NUMBER_OF_SETS\t3\n\
BEGIN_DATA\n\
1\t10.5\t20.5\t30.5\n\
2\t40.5\t50.5\t60.5\n\
3\t70.5\t80.5\t90.5\n\
END_DATA\n";
fn main() {
    let d = It8::load_from_mem(None, D).expect("parse");
    let t = d.get_table();
    eprintln!(
        "n_samples={} n_patches={} sample_id={}",
        t.n_samples, t.n_patches, t.sample_id
    );
    eprintln!("data_format={:?}", t.data_format);
    eprintln!("data={:?}", t.data);
    eprintln!("props={:?}", d.enum_properties());
}
