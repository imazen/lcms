//! Port of `cmsmd5.c` — MD5 for profile IDs.
//!
//! C source: `reference/lcms2/src/cmsmd5.c`
//! Gate: `tests/c_parity_md5.rs`
//!
//! `cmsMD5computeID` is deferred — it needs `cmsio0`'s profile serializer.
//! The low-level `cmsMD5alloc`/`cmsMD5add`/`cmsMD5finish` triplet is ported as
//! the `Md5` struct (`alloc` = `Md5::new`, `finish` = `Md5::finish`).
//!
//! `byteReverse` compiles out on little-endian targets; the port expresses it
//! as `u32::from_le_bytes`/`to_le_bytes`, which is identity there and correct
//! on big-endian — same observable behaviour as C on both.

/// `cmsProfileID` — the 16-byte digest container.
pub type ProfileId = [u8; 16];

/// `_cmsMD5` state.
pub struct Md5 {
    buf: [u32; 4],
    bits: [u32; 2],
    i_n: [u8; 64],
}

#[inline]
fn f1(x: u32, y: u32, z: u32) -> u32 {
    z ^ (x & (y ^ z))
}
#[inline]
fn f2(x: u32, y: u32, z: u32) -> u32 {
    f1(z, x, y)
}
#[inline]
fn f3(x: u32, y: u32, z: u32) -> u32 {
    x ^ y ^ z
}
#[inline]
fn f4(x: u32, y: u32, z: u32) -> u32 {
    y ^ (x | !z)
}

macro_rules! step {
    ($f:ident, $w:ident, $x:ident, $y:ident, $z:ident, $data:expr, $s:literal) => {
        $w = $w.wrapping_add($f($x, $y, $z)).wrapping_add($data);
        $w = ($w << $s) | ($w >> (32 - $s));
        $w = $w.wrapping_add($x);
    };
}

/// `cmsMD5_Transform`
fn md5_transform(buf: &mut [u32; 4], i_n: &[u32; 16]) {
    let mut a = buf[0];
    let mut b = buf[1];
    let mut c = buf[2];
    let mut d = buf[3];

    step!(f1, a, b, c, d, i_n[0].wrapping_add(0xd76aa478), 7);
    step!(f1, d, a, b, c, i_n[1].wrapping_add(0xe8c7b756), 12);
    step!(f1, c, d, a, b, i_n[2].wrapping_add(0x242070db), 17);
    step!(f1, b, c, d, a, i_n[3].wrapping_add(0xc1bdceee), 22);
    step!(f1, a, b, c, d, i_n[4].wrapping_add(0xf57c0faf), 7);
    step!(f1, d, a, b, c, i_n[5].wrapping_add(0x4787c62a), 12);
    step!(f1, c, d, a, b, i_n[6].wrapping_add(0xa8304613), 17);
    step!(f1, b, c, d, a, i_n[7].wrapping_add(0xfd469501), 22);
    step!(f1, a, b, c, d, i_n[8].wrapping_add(0x698098d8), 7);
    step!(f1, d, a, b, c, i_n[9].wrapping_add(0x8b44f7af), 12);
    step!(f1, c, d, a, b, i_n[10].wrapping_add(0xffff5bb1), 17);
    step!(f1, b, c, d, a, i_n[11].wrapping_add(0x895cd7be), 22);
    step!(f1, a, b, c, d, i_n[12].wrapping_add(0x6b901122), 7);
    step!(f1, d, a, b, c, i_n[13].wrapping_add(0xfd987193), 12);
    step!(f1, c, d, a, b, i_n[14].wrapping_add(0xa679438e), 17);
    step!(f1, b, c, d, a, i_n[15].wrapping_add(0x49b40821), 22);

    step!(f2, a, b, c, d, i_n[1].wrapping_add(0xf61e2562), 5);
    step!(f2, d, a, b, c, i_n[6].wrapping_add(0xc040b340), 9);
    step!(f2, c, d, a, b, i_n[11].wrapping_add(0x265e5a51), 14);
    step!(f2, b, c, d, a, i_n[0].wrapping_add(0xe9b6c7aa), 20);
    step!(f2, a, b, c, d, i_n[5].wrapping_add(0xd62f105d), 5);
    step!(f2, d, a, b, c, i_n[10].wrapping_add(0x02441453), 9);
    step!(f2, c, d, a, b, i_n[15].wrapping_add(0xd8a1e681), 14);
    step!(f2, b, c, d, a, i_n[4].wrapping_add(0xe7d3fbc8), 20);
    step!(f2, a, b, c, d, i_n[9].wrapping_add(0x21e1cde6), 5);
    step!(f2, d, a, b, c, i_n[14].wrapping_add(0xc33707d6), 9);
    step!(f2, c, d, a, b, i_n[3].wrapping_add(0xf4d50d87), 14);
    step!(f2, b, c, d, a, i_n[8].wrapping_add(0x455a14ed), 20);
    step!(f2, a, b, c, d, i_n[13].wrapping_add(0xa9e3e905), 5);
    step!(f2, d, a, b, c, i_n[2].wrapping_add(0xfcefa3f8), 9);
    step!(f2, c, d, a, b, i_n[7].wrapping_add(0x676f02d9), 14);
    step!(f2, b, c, d, a, i_n[12].wrapping_add(0x8d2a4c8a), 20);

    step!(f3, a, b, c, d, i_n[5].wrapping_add(0xfffa3942), 4);
    step!(f3, d, a, b, c, i_n[8].wrapping_add(0x8771f681), 11);
    step!(f3, c, d, a, b, i_n[11].wrapping_add(0x6d9d6122), 16);
    step!(f3, b, c, d, a, i_n[14].wrapping_add(0xfde5380c), 23);
    step!(f3, a, b, c, d, i_n[1].wrapping_add(0xa4beea44), 4);
    step!(f3, d, a, b, c, i_n[4].wrapping_add(0x4bdecfa9), 11);
    step!(f3, c, d, a, b, i_n[7].wrapping_add(0xf6bb4b60), 16);
    step!(f3, b, c, d, a, i_n[10].wrapping_add(0xbebfbc70), 23);
    step!(f3, a, b, c, d, i_n[13].wrapping_add(0x289b7ec6), 4);
    step!(f3, d, a, b, c, i_n[0].wrapping_add(0xeaa127fa), 11);
    step!(f3, c, d, a, b, i_n[3].wrapping_add(0xd4ef3085), 16);
    step!(f3, b, c, d, a, i_n[6].wrapping_add(0x04881d05), 23);
    step!(f3, a, b, c, d, i_n[9].wrapping_add(0xd9d4d039), 4);
    step!(f3, d, a, b, c, i_n[12].wrapping_add(0xe6db99e5), 11);
    step!(f3, c, d, a, b, i_n[15].wrapping_add(0x1fa27cf8), 16);
    step!(f3, b, c, d, a, i_n[2].wrapping_add(0xc4ac5665), 23);

    step!(f4, a, b, c, d, i_n[0].wrapping_add(0xf4292244), 6);
    step!(f4, d, a, b, c, i_n[7].wrapping_add(0x432aff97), 10);
    step!(f4, c, d, a, b, i_n[14].wrapping_add(0xab9423a7), 15);
    step!(f4, b, c, d, a, i_n[5].wrapping_add(0xfc93a039), 21);
    step!(f4, a, b, c, d, i_n[12].wrapping_add(0x655b59c3), 6);
    step!(f4, d, a, b, c, i_n[3].wrapping_add(0x8f0ccc92), 10);
    step!(f4, c, d, a, b, i_n[10].wrapping_add(0xffeff47d), 15);
    step!(f4, b, c, d, a, i_n[1].wrapping_add(0x85845dd1), 21);
    step!(f4, a, b, c, d, i_n[8].wrapping_add(0x6fa87e4f), 6);
    step!(f4, d, a, b, c, i_n[15].wrapping_add(0xfe2ce6e0), 10);
    step!(f4, c, d, a, b, i_n[6].wrapping_add(0xa3014314), 15);
    step!(f4, b, c, d, a, i_n[13].wrapping_add(0x4e0811a1), 21);
    step!(f4, a, b, c, d, i_n[4].wrapping_add(0xf7537e82), 6);
    step!(f4, d, a, b, c, i_n[11].wrapping_add(0xbd3af235), 10);
    step!(f4, c, d, a, b, i_n[2].wrapping_add(0x2ad7d2bb), 15);
    step!(f4, b, c, d, a, i_n[9].wrapping_add(0xeb86d391), 21);

    buf[0] = buf[0].wrapping_add(a);
    buf[1] = buf[1].wrapping_add(b);
    buf[2] = buf[2].wrapping_add(c);
    buf[3] = buf[3].wrapping_add(d);
}

/// The `ctx->in` buffer viewed as u32 words (C casts it to
/// `cmsUInt32Number*` and byteReverse-swaps on BE).
#[inline]
fn in_as_words(i_n: &[u8; 64]) -> [u32; 16] {
    let mut w = [0u32; 16];
    for (k, chunk) in i_n.chunks_exact(4).enumerate() {
        w[k] = u32::from_le_bytes(chunk.try_into().unwrap());
    }
    w
}

impl Md5 {
    /// `cmsMD5alloc`
    pub fn new() -> Self {
        Md5 {
            buf: [0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476],
            bits: [0, 0],
            i_n: [0; 64],
        }
    }

    /// `cmsMD5add` — feed `len` bytes.
    pub fn add(&mut self, buf: &[u8]) {
        let mut len = buf.len() as u32;
        let mut off = 0usize;

        let mut t = self.bits[0];
        self.bits[0] = t.wrapping_add(len << 3);
        if self.bits[0] < t {
            self.bits[1] = self.bits[1].wrapping_add(1);
        }
        self.bits[1] = self.bits[1].wrapping_add(len >> 29);

        t = (t >> 3) & 0x3f;

        if t != 0 {
            let t0 = t as usize;
            let t = 64 - t0;
            if len < t as u32 {
                self.i_n[t0..t0 + len as usize].copy_from_slice(buf);
                return;
            }
            self.i_n[t0..64].copy_from_slice(&buf[..t]);
            off = t;
            len -= t as u32;

            let w = in_as_words(&self.i_n);
            md5_transform(&mut self.buf, &w);
        }

        while len >= 64 {
            self.i_n.copy_from_slice(&buf[off..off + 64]);
            off += 64;
            len -= 64;

            let w = in_as_words(&self.i_n);
            md5_transform(&mut self.buf, &w);
        }

        self.i_n[..len as usize].copy_from_slice(&buf[off..off + len as usize]);
    }

    /// `cmsMD5finish` — pad, transform, return the 16-byte ID.
    pub fn finish(mut self) -> ProfileId {
        let mut count = ((self.bits[0] >> 3) & 0x3f) as usize;

        self.i_n[count] = 0x80;
        count += 1;

        count = 64 - 1 - (count - 1); // 63 - original count; matches C arithmetic

        if count < 8 {
            for i in 0..count {
                self.i_n[((self.bits[0] >> 3) & 0x3f) as usize + 1 + i] = 0;
            }
            let w = in_as_words(&self.i_n);
            md5_transform(&mut self.buf, &w);
            self.i_n[..56].fill(0);
        } else {
            let start = ((self.bits[0] >> 3) & 0x3f) as usize + 1;
            for i in 0..count - 8 {
                self.i_n[start + i] = 0;
            }
        }

        // Append bit length (little-endian words 14 and 15)
        self.i_n[56..60].copy_from_slice(&self.bits[0].to_le_bytes());
        self.i_n[60..64].copy_from_slice(&self.bits[1].to_le_bytes());

        let w = in_as_words(&self.i_n);
        md5_transform(&mut self.buf, &w);

        let mut id = [0u8; 16];
        for (k, word) in self.buf.iter().enumerate() {
            id[k * 4..k * 4 + 4].copy_from_slice(&word.to_le_bytes());
        }
        id
    }
}

impl Default for Md5 {
    fn default() -> Self {
        Self::new()
    }
}

/// `cmsMD5computeID` — zero flags/intent/id, serialize, MD5 the bytes,
/// store the digest in ProfileID, restore the header.
pub fn compute_id(icc: &mut crate::profile::IccProfile) -> bool {
    let keep_flags = icc.flags;
    let keep_intent = icc.rendering_intent;
    let keep_id = icc.profile_id;

    icc.flags = 0;
    icc.rendering_intent = 0;
    icc.profile_id = crate::types::ProfileId::default();

    let Some(bytes) = crate::profile::save_profile_to_mem(icc) else {
        icc.flags = keep_flags;
        icc.rendering_intent = keep_intent;
        icc.profile_id = keep_id;
        return false;
    };

    let mut md5 = Md5::new();
    md5.add(&bytes);
    let digest = md5.finish();

    icc.flags = keep_flags;
    icc.rendering_intent = keep_intent;
    icc.profile_id = crate::types::ProfileId { id8: digest };
    true
}
