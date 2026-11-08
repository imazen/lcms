//! cmsopt optimizer parity: transforms with flags=0 (optimizer runs) vs
//! FLAGS_NOOPTIMIZE — the optimizer's pipeline-reduction paths must
//! produce identical output on both sides.

use lcms::{profile, types::*, virt, xform};
use lcms_cref as c;

fn corpus() -> Vec<Vec<u8>> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
    let mut files: Vec<_> = std::fs::read_dir(&dir).unwrap()
        .filter_map(|e| e.ok()).map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "icc").unwrap_or(false))
        .collect();
    files.sort();
    files.iter().map(|f| std::fs::read(f).unwrap()).collect()
}

#[test]
fn optimizer_on_off() {
    let srgb = std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../aRGBlcms2.icc")
    ).unwrap();
    let mut seed = 0x1234u64;
    for flags in [0u32, FLAGS_NOOPTIMIZE] {
        for (i, data) in corpus().iter().enumerate() {
            for data2 in corpus().iter().take(3) {
                // profile -> sRGB transform
                let ct = unsafe {
                    let h1 = c::cmsOpenProfileFromMem(data.as_ptr() as _, data.len() as u32);
                    if h1.is_null() { continue; }
                    let h2 = c::cmsOpenProfileFromMem(data2.as_ptr() as _, data2.len() as u32);
                    let cs_in = c::cmsGetColorSpace(h1) | 3 << 3 | 1;
                    let cs_out = c::cmsGetColorSpace(h2) | 3 << 3 | 1;
                    let t = c::cmsCreateTransform(h1, cs_in, h2, cs_out, 0, flags);
                    c::cmsCloseProfile(h1);
                    c::cmsCloseProfile(h2);
                    t
                };
                let mut rp1 = profile::open_profile_from_mem(None, data).unwrap();
                let mut rp2 = profile::open_profile_from_mem(None, data2).unwrap();
                let fi = rp1.color_space | 3 << 3 | 1;
                let fo = rp2.color_space | 3 << 3 | 1;
                let mut profs = vec![rp1.clone(), rp2.clone()];
                let mut bpc = [false; 2];
                let intents = [0u32; 2];
                let adapt = [1.0f64; 2];
                let rt = xform::create_extended_transform(
                    None, &mut profs, &mut bpc, &intents, &adapt, None, 0, fi, fo, flags,
                );
                assert_eq!(ct.is_null(), rt.is_none(),
                    "profile {i} flags {flags:#x} presence");
                if ct.is_null() || rt.is_none() { continue; }
                let mut rt = rt.unwrap();
                let n_in = ((fi >> 3) & 15) as usize;
                let n_out = ((fo >> 3) & 15) as usize;
                let inp: Vec<u8> = (0..16 * n_in).map(|_| {
                    seed = seed.wrapping_mul(0x9e3779b97f4a7c15) ^ (seed >> 33);
                    (seed >> 48) as u8
                }).collect();
                let mut co = vec![0u8; 16 * n_out];
                let mut ro = vec![0u8; 16 * n_out];
                unsafe { c::cmsDoTransform(ct, inp.as_ptr() as _, co.as_mut_ptr() as _, 16) };
                rt.do_transform(&inp, &mut ro, 16);
                assert_eq!(co, ro, "profile {i} flags {flags:#x} out");
                unsafe { c::cmsDeleteTransform(ct) };
            }
        }
    }
}
