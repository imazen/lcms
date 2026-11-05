//! Differential gate for cmspack.c — same pixels through C and Rust
//! unrollers/packers, compare wIn and output bytes.

use lcms::pack;
use lcms_cref as c;

struct Info {
    input_format: u32,
    output_format: u32,
}

const TYPE_RGB_8: u32 = 4 << 16 | 3 << 3 | 1;
const TYPE_BGR_8: u32 = 4 << 16 | 3 << 3 | 1 | 1 << 10;
const TYPE_RGBA_8: u32 = 4 << 16 | 1 << 7 | 3 << 3 | 1;
const TYPE_RGB_16: u32 = 4 << 16 | 3 << 3 | 2;
const TYPE_BGR_16: u32 = 4 << 16 | 3 << 3 | 2 | 1 << 10;
const TYPE_CMYK_8: u32 = 6 << 16 | 4 << 3 | 1;
const TYPE_CMYK_16: u32 = 6 << 16 | 4 << 3 | 2;
const TYPE_GRAY_8: u32 = 3 << 16 | 1 << 3 | 1;
const TYPE_GRAY_16: u32 = 3 << 16 | 1 << 3 | 2;
const TYPE_LABV2_8: u32 = 30 << 16 | 3 << 3 | 1;
const TYPE_RGB_8_PLANAR: u32 = 4 << 16 | 3 << 3 | 1 | 1 << 12;
const TYPE_GRAYA_8: u32 = 3 << 16 | 1 << 7 | 1 << 3 | 1;
const TYPE_GRAYA_8_PREMUL: u32 = 3 << 16 | 1 << 7 | 1 << 3 | 1 | 1 << 23;
const TYPE_RGB_16_SE: u32 = 4 << 16 | 3 << 3 | 2 | 1 << 11;
const TYPE_RGBA_16: u32 = 4 << 16 | 1 << 7 | 3 << 3 | 2;
const TYPE_RGBA_16_PREMUL: u32 = 4 << 16 | 1 << 7 | 3 << 3 | 2 | 1 << 23;

fn c_unroll16(fmt_ty: u32, pixels: &[u8], stride: u32) -> Vec<u16> {
    let f = unsafe { c::orc_get_input_fmt16(fmt_ty) };
    assert!(!f.is_null(), "no C fmt16 for {fmt_ty:#x}");
    let mut w = [0u16; 16];
    let info = Info {
        input_format: fmt_ty,
        output_format: 0,
    };
    unsafe {
        c::orc_call_fmt16(
            f,
            &info as *const _ as *const _,
            w.as_mut_ptr(),
            pixels.as_ptr() as *mut u8,
            stride,
        )
    };
    w.to_vec()
}

fn r_unroll16(fmt_ty: u32, pixels: &[u8], stride: u32) -> Vec<u16> {
    let f = match pack::get_stock_input_formatter(fmt_ty, pack::CMS_PACK_FLAGS_16BITS) {
        pack::Formatter::Unpack16(f) => f,
        _ => panic!("no rust fmt16 for {fmt_ty:#x}"),
    };
    let mut w = [0u16; 16];
    let info = pack::PackInfo {
        input_format: fmt_ty,
        output_format: 0,
        stride,
    };
    f(&info, &mut w, pixels, stride);
    w.to_vec()
}

#[test]
fn unroll16_all_formats() {
    let seed: Vec<u8> = (0..128u8)
        .map(|i| i.wrapping_mul(37).wrapping_add(11))
        .collect();
    let words: Vec<u8> = (0..128u8)
        .map(|i| i.wrapping_mul(13).wrapping_add(5))
        .collect();
    let cases: &[(u32, &[u8], u32)] = &[
        (TYPE_RGB_8, &seed, 0),
        (TYPE_BGR_8, &seed, 0),
        (TYPE_RGBA_8, &seed, 0),
        (TYPE_RGB_16, &words, 0),
        (TYPE_BGR_16, &words, 0),
        (TYPE_CMYK_8, &seed, 0),
        (TYPE_CMYK_16, &words, 0),
        (TYPE_GRAY_8, &seed, 0),
        (TYPE_GRAY_16, &words, 0),
        (TYPE_LABV2_8, &seed, 0),
        (TYPE_RGB_8_PLANAR, &seed, 16),
        (TYPE_GRAYA_8, &seed, 0),
        (TYPE_GRAYA_8_PREMUL, &seed, 0),
        (TYPE_RGB_16_SE, &words, 0),
        (TYPE_RGBA_16, &words, 0),
        (TYPE_RGBA_16_PREMUL, &words, 0),
    ];
    for &(fmt, pix, stride) in cases {
        let cw = c_unroll16(fmt, pix, stride);
        let rw = r_unroll16(fmt, pix, stride);
        assert_eq!(rw, cw, "fmt {fmt:#x}");
    }
}

fn c_pack16(fmt_ty: u32, w: &[u16], out: &mut [u8], stride: u32) -> usize {
    let f = unsafe { c::orc_get_output_fmt16(fmt_ty) };
    assert!(!f.is_null(), "no C pack16 for {fmt_ty:#x}");
    let info = Info {
        input_format: 0,
        output_format: fmt_ty,
    };
    let p = unsafe {
        c::orc_call_fmt16(
            f,
            &info as *const _ as *const _,
            w.as_ptr() as *mut u16,
            out.as_mut_ptr(),
            stride,
        )
    };
    (p as usize) - (out.as_mut_ptr() as usize)
}

fn r_pack16(fmt_ty: u32, w: &[u16], out: &mut [u8], stride: u32) -> usize {
    let f = match pack::get_stock_output_formatter(fmt_ty, pack::CMS_PACK_FLAGS_16BITS) {
        pack::Formatter::Pack16(f) => f,
        _ => panic!("no rust pack16 for {fmt_ty:#x}"),
    };
    let info = pack::PackInfo {
        input_format: 0,
        output_format: fmt_ty,
        stride,
    };
    let mut w16 = [0u16; 16];
    w16[..w.len()].copy_from_slice(w);
    f(&info, &w16, out, stride)
}

#[test]
fn pack16_all_formats() {
    let w = [0x1234u16, 0xABCD, 0x5678, 0xDEAD, 0x1111, 0x9999];
    let cases: &[u32] = &[
        TYPE_RGB_8,
        TYPE_BGR_8,
        TYPE_RGBA_8,
        TYPE_RGB_16,
        TYPE_BGR_16,
        TYPE_CMYK_8,
        TYPE_CMYK_16,
        TYPE_GRAY_8,
        TYPE_GRAY_16,
        TYPE_LABV2_8,
        TYPE_RGB_8_PLANAR,
        TYPE_GRAYA_8,
        TYPE_RGB_16_SE,
        TYPE_RGBA_16,
        6 << 16 | 4 << 3 | 1 | 1 << 10, // KYMC swap
        16 << 16 | 2 << 3 | 1,          // MCH2_8
        16 << 16 | 2 << 3 | 2,          // MCH2_16
    ];
    for &fmt in cases {
        let mut co = [0u8; 64];
        let mut ro = [0u8; 64];
        let cn = c_pack16(fmt, &w, &mut co, 16);
        let rn = r_pack16(fmt, &w, &mut ro, 16);
        assert_eq!(rn, cn, "fmt {fmt:#x} bytes written");
        assert_eq!(&ro[..cn], &co[..cn], "fmt {fmt:#x} bytes differ");
    }
}

fn c_unrollfl(fmt_ty: u32, pixels: &[u8], stride: u32) -> Vec<f32> {
    let f = unsafe { c::orc_get_input_fmtfl(fmt_ty) };
    assert!(!f.is_null(), "no C fmtfl for {fmt_ty:#x}");
    let mut w = [0f32; 16];
    let info = Info {
        input_format: fmt_ty,
        output_format: 0,
    };
    unsafe {
        c::orc_call_fmtfl(
            f,
            &info as *const _ as *const _,
            w.as_mut_ptr(),
            pixels.as_ptr() as *mut u8,
            stride,
        )
    };
    w.to_vec()
}

fn r_unrollfl(fmt_ty: u32, pixels: &[u8], stride: u32) -> Vec<f32> {
    let f = match pack::get_stock_input_formatter(fmt_ty, pack::CMS_PACK_FLAGS_FLOAT) {
        pack::Formatter::UnpackF32(f) => f,
        _ => panic!("no rust fmtfl for {fmt_ty:#x}"),
    };
    let mut w = [0f32; 16];
    let info = pack::PackInfo {
        input_format: fmt_ty,
        output_format: 0,
        stride,
    };
    f(&info, &mut w, pixels, stride);
    w.to_vec()
}

#[test]
fn unroll_float_formats() {
    let mut pix = [0u8; 128];
    // doubles
    for c in 0..16 {
        let v = (c as f64 + 0.25) * 0.31;
        pix[c * 8..c * 8 + 8].copy_from_slice(&v.to_le_bytes());
    }
    // floats at offset 0 for second case
    let mut pixf = [0u8; 128];
    for c in 0..16 {
        let v = (c as f32 + 0.5) * 0.17;
        pixf[c * 4..c * 4 + 4].copy_from_slice(&v.to_le_bytes());
    }
    let cases: &[(u32, &[u8], u32)] = &[
        (4 << 16 | 3 << 3 | 0 | 1 << 22, &pix, 0),  // RGB double
        (4 << 16 | 3 << 3 | 4 | 1 << 22, &pixf, 0), // RGB float
        (4 << 16 | 3 << 3 | 4 | 1 << 22 | 1 << 10, &pixf, 0), // BGR float
        (6 << 16 | 4 << 3 | 4 | 1 << 22, &pixf, 0), // CMYK float
        (10 << 16 | 3 << 3 | 0 | 1 << 22, &pix, 0), // Lab double
        (10 << 16 | 3 << 3 | 4 | 1 << 22, &pixf, 0), // Lab float
        (9 << 16 | 3 << 3 | 4 | 1 << 22, &pixf, 0), // XYZ float
        (4 << 16 | 3 << 3 | 1, &pix, 0),            // RGB_8 → float
        (4 << 16 | 3 << 3 | 2, &pix, 0),            // RGB_16 → float
    ];
    for &(fmt, px, st) in cases {
        let cw = c_unrollfl(fmt, px, st);
        let rw = r_unrollfl(fmt, px, st);
        for (i, (&a, &b)) in rw.iter().zip(&cw).enumerate() {
            assert_eq!(a.to_bits(), b.to_bits(), "fmt {fmt:#x} ch{i}: {a} vs {b}");
        }
    }
}
