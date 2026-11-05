//! Perf parity: sRGB→sRGB transform eval, C vs Rust, same pixels.

use lcms::virt;
use lcms::xform;
use std::time::Instant;

const TYPE_RGB_8: u32 = 4 << 16 | 3 << 3 | 1;
const TYPE_RGB_16: u32 = 4 << 16 | 3 << 3 | 2;

fn main() {
    let n = 1_000_000usize;
    let inp8 = vec![0x40u8; n * 3];
    let inp16 = vec![0x40u8; n * 6];
    let mut out8 = vec![0u8; n * 3];
    let mut out16 = vec![0u8; n * 6];

    // --- Rust 8-bit ---
    let p1 = virt::create_srgb_profile(None).unwrap();
    let p2 = virt::create_srgb_profile(None).unwrap();
    let mut profs = vec![p1, p2];
    let mut bpc = [false, false];
    let intents = [0u32, 0];
    let adapt = [1.0f64, 1.0];
    let mut t = xform::create_extended_transform(
        None, &mut profs, &mut bpc, &intents, &adapt, None, 0, TYPE_RGB_8, TYPE_RGB_8, 0,
    )
    .unwrap();
    let t0 = Instant::now();
    t.do_transform(&inp8, &mut out8, n as u32);
    let r8 = t0.elapsed();

    // --- Rust 16-bit ---
    let p1 = virt::create_srgb_profile(None).unwrap();
    let p2 = virt::create_srgb_profile(None).unwrap();
    let mut profs = vec![p1, p2];
    let mut t16 = xform::create_extended_transform(
        None,
        &mut profs,
        &mut bpc,
        &intents,
        &adapt,
        None,
        0,
        TYPE_RGB_16,
        TYPE_RGB_16,
        0,
    )
    .unwrap();
    let t0 = Instant::now();
    t16.do_transform(&inp16, &mut out16, n as u32);
    let r16 = t0.elapsed();

    // --- C via cref ---
    let (c8, c16) = unsafe {
        let prof = lcms_cref::cmsCreate_sRGBProfile();
        let t = lcms_cref::cmsCreateTransform(prof, TYPE_RGB_8, prof, TYPE_RGB_8, 0, 0);
        let t0 = Instant::now();
        lcms_cref::cmsDoTransform(
            t,
            inp8.as_ptr() as *const _,
            out8.as_mut_ptr() as *mut _,
            n as u32,
        );
        let c8 = t0.elapsed();
        let t16c = lcms_cref::cmsCreateTransform(prof, TYPE_RGB_16, prof, TYPE_RGB_16, 0, 0);
        let t0 = Instant::now();
        lcms_cref::cmsDoTransform(
            t16c,
            inp16.as_ptr() as *const _,
            out16.as_mut_ptr() as *mut _,
            n as u32,
        );
        let c16 = t0.elapsed();
        (c8, c16)
    };

    println!("{n} pixels sRGB→sRGB:");
    println!(
        "  8-bit : C {:>7.2?}  Rust {:>7.2?}  ratio {:.2}x",
        c8,
        r8,
        r8.as_secs_f64() / c8.as_secs_f64()
    );
    println!(
        "  16-bit: C {:>7.2?}  Rust {:>7.2?}  ratio {:.2}x",
        c16,
        r16,
        r16.as_secs_f64() / c16.as_secs_f64()
    );
}
