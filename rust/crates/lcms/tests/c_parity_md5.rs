//! Differential test: `lcms::md5` vs `cmsmd5.c`.

use lcms::md5::Md5;
use lcms_cref as c;

fn rng(state: &mut u64) -> u64 {
    *state = state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    *state >> 11
}

fn c_md5(data: &[u8], chunk_size: usize) -> [u8; 16] {
    unsafe {
        let h = c::cmsMD5alloc(std::ptr::null_mut());
        assert!(!h.is_null());
        let mut off = 0;
        while off < data.len() {
            let n = chunk_size.min(data.len() - off);
            c::cmsMD5add(h, data[off..].as_ptr(), n as u32);
            off += n;
        }
        let mut id = c::ProfileID::default();
        c::cmsMD5finish(&mut id, h);
        id.id8
    }
}

#[test]
fn md5_all_lengths_and_chuntings() {
    let mut s = 1u64;
    // Every boundary: 55/56/63/64/65 padding edges + empty + large
    for len in [
        0usize,
        1,
        3,
        55,
        56,
        57,
        63,
        64,
        65,
        127,
        128,
        129,
        200,
        1000,
        64 * 1024,
    ] {
        let data: Vec<u8> = (0..len).map(|_| rng(&mut s) as u8).collect();
        for chunk in [usize::MAX, 1, 7, 64, 100] {
            let mut m = Md5::new();
            let mut off = 0;
            while off < data.len() {
                let n = chunk.min(data.len() - off);
                m.add(&data[off..off + n]);
                off += n;
            }
            if data.is_empty() {
                m.add(&[]);
            }
            assert_eq!(m.finish(), c_md5(&data, chunk), "len={len} chunk={chunk}");
        }
    }
}

#[test]
fn md5_known_vectors() {
    // RFC 1321 vectors (sanity — the C gate is the real check)
    let mut m = Md5::new();
    m.add(b"abc");
    let id = m.finish();
    assert_eq!(
        id.iter().map(|b| format!("{b:02x}")).collect::<String>(),
        "900150983cd24fb0d6963f7d28e17f72"
    );
}
