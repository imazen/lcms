use lcms::cgats::It8;
fn main() {
    let d = b"IT8.7/2\nORIGINATOR\t\"lcms test\"\nNUMBER_OF_FIELDS\t4\n";
    // drive the lexer directly by hacking visibility via a tiny parser
    let mut it8 = It8::load_from_mem(None, d).expect("x");
    let _ = &mut it8;
}
