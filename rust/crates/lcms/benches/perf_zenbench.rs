//! Perf parity: Display P3→sRGB transforms, C vs Rust, zenbench paired-stat.
//! Display P3 = D65 + P3 primaries + sRGB transfer (cmsCreateRGBProfileTHR).

use lcms::types::{CiexyY, CiexyYTriple};
use lcms::{gamma, profile, virt, xform};
use std::ffi::c_void;
use zenbench::Throughput;
use zenbench::black_box;

const TYPE_RGB_8: u32 = 4 << 16 | 3 << 3 | 1;
const TYPE_RGB_16: u32 = 4 << 16 | 3 << 3 | 2;

const D65: CiexyY = CiexyY { x: 0.3127, y: 0.3290, Y: 1.0 };
// Display P3 (SMPTE RP 431-2) primaries
const P3: CiexyYTriple = CiexyYTriple {
    Red: CiexyY { x: 0.680, y: 0.320, Y: 1.0 },
    Green: CiexyY { x: 0.265, y: 0.690, Y: 1.0 },
    Blue: CiexyY { x: 0.150, y: 0.060, Y: 1.0 },
};

fn p3_rust() -> Box<profile::IccProfile> {
    let g = gamma::build_parametric_tone_curve(None, 4, &[2.4, 1.0/1.055, 0.055/1.055, 1.0/12.92, 0.04045]).unwrap();
    virt::create_rgb_profile(None, Some(&D65), Some(&P3), Some([&g, &g, &g])).unwrap()
}

fn rust_xform(intent: u32, fmt: u32) -> Box<xform::Transform> {
    let p1 = p3_rust();
    let p2 = virt::create_srgb_profile(None).unwrap();
    let mut profs = vec![p1, p2];
    let mut bpc = [false, false];
    let intents = [intent, intent];
    let adapt = [1.0f64, 1.0];
    xform::create_extended_transform(
        None, &mut profs, &mut bpc, &intents, &adapt, None, 0, fmt, fmt, 0,
    ).unwrap()
}

// C-side objects are used single-threaded inside the timed closure.
struct SendPtr { p: *mut c_void }
unsafe impl Send for SendPtr {}
unsafe impl Sync for SendPtr {}
impl SendPtr { fn ptr(&self) -> *mut c_void { self.p } }

fn c_xform(intent: u32, fmt: u32) -> SendPtr {
    unsafe {
        let g = lcms_cref::cmsBuildParametricToneCurve(std::ptr::null_mut(), 4,
            [2.4f64, 1.0/1.055, 0.055/1.055, 1.0/12.92, 0.04045].as_ptr());
        let prim: [*mut lcms_cref::ToneCurve; 3] = [g, g, g];
        let p3 = lcms_cref::cmsCreateRGBProfileTHR(
            std::ptr::null_mut(),
            &D65 as *const CiexyY as *const c_void,
            &P3 as *const CiexyYTriple as *const c_void,
            prim.as_ptr() as *const *mut c_void,
        );
        let srgb = lcms_cref::cmsCreate_sRGBProfile();
        SendPtr { p: lcms_cref::cmsCreateTransform(p3, fmt, srgb, fmt, intent, 0) }
    }
}

fn run_case(label: &str, intent: u32, fmt: u32, npx: usize, bytes: usize, suite: &mut zenbench::Suite) {
    let mk_input = move || -> Vec<u8> { (0..npx * bytes).map(|i| ((i * 37 + i / bytes * 11) % 251) as u8).collect() };
    suite.compare(label, |g| {
        g.throughput(Throughput::Elements(npx as u64));
        g.bench("c", move |b| {
            let tc = c_xform(intent, fmt);
            let input = mk_input();
            let mut out = vec![0u8; input.len()];
            b.iter(move || unsafe {
                lcms_cref::cmsDoTransform(tc.ptr(), input.as_ptr() as *const _, out.as_mut_ptr() as *mut _, npx as u32);
                black_box(&mut out);
            });
        });
        g.bench("rust", move |b| {
            let mut tr = rust_xform(intent, fmt);
            let input = mk_input();
            let mut out = vec![0u8; input.len()];
            b.iter(move || {
                tr.do_transform(&input, &mut out, npx as u32);
                black_box(&mut out);
            });
        });
    });
}

zenbench::main!(|suite| {
    let n = 200_000;
    // perceptual (0) and relative colorimetric (1), 8-bit and 16-bit
    run_case("p3_srgb_perceptual_u8",    0, TYPE_RGB_8,  n, 3, suite);
    run_case("p3_srgb_perceptual_u16",   0, TYPE_RGB_16, n, 6, suite);
    run_case("p3_srgb_relative_u8",      1, TYPE_RGB_8,  n, 3, suite);
    run_case("p3_srgb_relative_u16",     1, TYPE_RGB_16, n, 6, suite);
});
