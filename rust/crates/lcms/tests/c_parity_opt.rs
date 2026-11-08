//! cmsopt optimizer parity: transforms with flags=0 (optimizer runs) vs
//! FLAGS_NOOPTIMIZE — the optimizer's pipeline-reduction paths must
//! produce identical output on both sides.

use std::ffi::c_void;
use std::sync::Mutex;
use lcms::{profile, types::*, virt, xform};
use lcms_cref as c;

static CTX_LOCK: Mutex<()> = Mutex::new(());

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


/// Optimizer internals: transforms whose links trigger each optimizer
/// branch — sRGB->sRGB, sRGB->lab4, lab4->sRGB — NOOPTIMIZE off vs on.
#[test]
fn optimizer_branch_matrix() {
    unsafe {
        for name in ["srgb->srgb", "srgb->lab4", "lab4->srgb"] {
            for flags in [0u32, FLAGS_NOOPTIMIZE] {
                let h1 = if name == "lab4->srgb" {
                    c::cmsCreateLab4Profile(std::ptr::null_mut())
                } else { c::cmsCreate_sRGBProfile() };
                let h2 = if name == "srgb->lab4" {
                    c::cmsCreateLab4Profile(std::ptr::null_mut())
                } else { c::cmsCreate_sRGBProfile() };
                let cs_in = c::cmsGetColorSpace(h1) | 3 << 3 | 1;
                let cs_out = c::cmsGetColorSpace(h2) | 3 << 3 | 1;
                let ct = c::cmsCreateTransform(h1, cs_in, h2, cs_out, 0, flags);
                let _ = c::cmsCloseProfile(h1);
                let _ = c::cmsCloseProfile(h2);
                let (mut rp1, mut rp2) = match name {
                    "srgb->srgb" => (virt::create_srgb_profile(None).unwrap(),
                                     virt::create_srgb_profile(None).unwrap()),
                    "srgb->lab4" => (virt::create_srgb_profile(None).unwrap(),
                                     virt::create_lab4_profile(None, None).unwrap()),
                    _ => (virt::create_lab4_profile(None, None).unwrap(),
                          virt::create_srgb_profile(None).unwrap()),
                };
                let fi = rp1.color_space | 3 << 3 | 1;
                let fo = rp2.color_space | 3 << 3 | 1;
                let mut profs = vec![rp1.clone(), rp2.clone()];
                let mut bpc = [false; 2];
                let intents = [0u32; 2];
                let adapt = [1.0f64; 2];
                let rt = xform::create_extended_transform(
                    None, &mut profs, &mut bpc, &intents, &adapt, None, 0,
                    fi, fo, flags);
                assert_eq!(ct.is_null(), rt.is_none(), "{name} f{flags:#x}");
                if !ct.is_null() && rt.is_some() {
                    let mut rt = rt.unwrap();
                    let inp = [0x1234u16, 0x5678, 0x9abc];
                    let inb: Vec<u8> = inp.iter()
                        .flat_map(|v| v.to_be_bytes()).collect();
                    let mut co = [0u16; 3];
                    let mut ro = vec![0u8; 6];
                    c::cmsDoTransform(ct, inp.as_ptr() as _,
                        co.as_mut_ptr() as *mut _, 1);
                    rt.do_transform(&inb, &mut ro, 1);
                    let ro16: Vec<u16> = ro.chunks_exact(2)
                        .map(|b| u16::from_be_bytes([b[0], b[1]])).collect();
                    assert_eq!(co, ro16[..3], "{name} f{flags:#x} eval");
                    c::cmsDeleteTransform(ct);
                }
            }
        }
    }
}

/// OptimizeMatrixShaper arms — 3-stage curves->matrix->curves and
/// 4-stage curves->matrix->matrix->curves pipelines through the
/// optimizer; also JoiningCurves (adjacent curve sets). Constructs
/// the pipeline shapes via profile tags.
#[test]
fn opt_matrix_shaper_shapes() {
    unsafe {
        // 4-stage: curves->matrix->matrix->curves — build via a
        // devicelink profile carrying that pipeline shape
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let mut cc = [g; 3];
        let mat: [f64; 9] = [0.4,0.3,0.3, 0.2,0.6,0.2, 0.1,0.1,0.8];
        let mat2: [f64; 9] = [1.1,0.0,0.0, 0.0,1.0,0.0, 0.0,0.0,0.9];
        for (name, n_stages) in [("m3", 3u8), ("m4", 4u8)] {
            let p = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
            c::cmsPipelineInsertStage(p, 1,
                c::cmsStageAllocToneCurves(std::ptr::null_mut(), 3, cc.as_ptr()));
            c::cmsPipelineInsertStage(p, 1,
                c::cmsStageAllocMatrix(std::ptr::null_mut(), 3, 3,
                    mat.as_ptr(), std::ptr::null()));
            if n_stages == 4 {
                c::cmsPipelineInsertStage(p, 1,
                    c::cmsStageAllocMatrix(std::ptr::null_mut(), 3, 3,
                        mat2.as_ptr(), std::ptr::null()));
            }
            c::cmsPipelineInsertStage(p, 1,
                c::cmsStageAllocToneCurves(std::ptr::null_mut(), 3, cc.as_ptr()));
            let prof = c::cmsCreateProfilePlaceholder(std::ptr::null_mut());
            c::cmsSetDeviceClass(prof, 0x6c696e6b); // 'link'
            c::cmsSetColorSpace(prof, 0x52474220);  // 'RGB '
            c::cmsSetPCS(prof, 0x4C616220);          // 'Lab '
            c::cmsWriteTag(prof, 0x41324230, p as *const _); // A2B0
            // devicelink alone IS the transform
            let t = c::cmsCreateTransform(prof, 0x00ff04,
                prof, 0x00ff04, 0, 0);
            eprintln!("{name}: t={}", !t.is_null());
            if !t.is_null() { c::cmsDeleteTransform(t); }
            c::cmsCloseProfile(prof);
            c::cmsPipelineFree(p);
        }
        // JoiningCurves — profile with two adjacent curve sets
        let p2 = c::cmsPipelineAlloc(std::ptr::null_mut(), 3, 3);
        c::cmsPipelineInsertStage(p2, 1,
            c::cmsStageAllocToneCurves(std::ptr::null_mut(), 3, cc.as_ptr()));
        c::cmsPipelineInsertStage(p2, 1,
            c::cmsStageAllocToneCurves(std::ptr::null_mut(), 3, cc.as_ptr()));
        let prof2 = c::cmsCreateProfilePlaceholder(std::ptr::null_mut());
        c::cmsSetDeviceClass(prof2, 0x6c696e6b);
        c::cmsSetColorSpace(prof2, 0x52474220);
        c::cmsSetPCS(prof2, 0x4C616220);
        c::cmsWriteTag(prof2, 0x41324230, p2 as *const _);
        let t2 = c::cmsCreateTransform(prof2, 0x00ff04,
            prof2, 0x00ff04, 0, 0);
        eprintln!("jc: t={}", !t2.is_null());
        if !t2.is_null() { c::cmsDeleteTransform(t2); }
        c::cmsCloseProfile(prof2);
        c::cmsPipelineFree(p2);
    }
}

/// Optimizer shape sweep — many profile pairs x flags drive
/// _cmsOptimizePipeline through its per-shape arms (matrix-shaper,
/// curves-only, clut-passthrough, float/8bit/16bit variants,
/// NOOPTIMIZE skip).
#[test]
fn opt_shape_sweep() {
    unsafe {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        let mut profs: Vec<*mut std::ffi::c_void> = Vec::new();
        for f in ["lcms2cmyk.icc", "graylcms2.icc", "labv2lcms2.icc",
                  "sRGB3.icc", "labv4lcms2.icc", "linlcms2.icc"] {
            if let Ok(d) = std::fs::read(dir.join(f)) {
                let p = c::cmsOpenProfileFromMem(
                    d.as_ptr() as *const _, d.len() as u32);
                if !p.is_null() { profs.push(p); }
            }
        }
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        profs.push(srgb);
        profs.push(lab);
        // fmt by profile colorspace
        let fmt_of = |p: *mut std::ffi::c_void| -> u32 {
            let cs = c::cmsGetColorSpace(p);
            match cs {
                0x47524159 => (3 << 16) | (1 << 3) | 1,
                0x434D594B => (6 << 16) | (4 << 3) | 1,
                0x4C616220 => (10 << 16) | (3 << 3) | 2,
                _ => (4 << 16) | (3 << 3) | 1,
            }
        };
        let flags = [0u32, 0x1000, 0x2000, 0x0001, 0x4000];
        for &a in &profs {
            for &b in &profs {
                if a == b { continue; }
                let fa = fmt_of(a);
                let fb = fmt_of(b);
                for &fl in &flags {
                    let t = c::cmsCreateTransform(a, fa, b, fb, 0, fl);
                    if !t.is_null() {
                        let n = 4u32;
                        let ch = ((fa >> 3) & 15) as usize;
                        let by = (fa & 7) as usize;
                        let inp = vec![0x40u8; (n as usize) * ch * by];
                        let cho = ((fb >> 3) & 15) as usize;
                        let byo = (fb & 7) as usize;
                        let mut out = vec![0u8; (n as usize) * cho * byo];
                        c::cmsDoTransform(t, inp.as_ptr() as *const _,
                            out.as_mut_ptr() as *mut _, n);
                        c::cmsDeleteTransform(t);
                    }
                }
            }
        }
        for p in profs { c::cmsCloseProfile(p); }
    }
}

/// OptimizeByResampling + OptimizeByComputingLinearization arms —
/// integer-format transforms over CLUT-carrying profiles resample
/// the grid; 8-bit RGB->RGB hits the linearization path.
#[test]
fn opt_resample_linearize() {
    unsafe {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        let srgb = c::cmsCreate_sRGBProfile();
        let mut cmyk = std::ptr::null_mut();
        if let Ok(d) = std::fs::read(dir.join("lcms2cmyk.icc")) {
            cmyk = c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32);
        }
        let rgb8 = (4u32 << 16) | (3 << 3) | 1;
        let cmyk8 = (6u32 << 16) | (4 << 3) | 1;
        let rgb16 = (4u32 << 16) | (3 << 3) | 2;
        // u8 RGB->CMYK: resamples the CMYK CLUT
        if !cmyk.is_null() {
            for (inf, of, fl, name) in [
                (rgb8, cmyk8, 0u32, "u8"),
                (rgb8, cmyk8, 0x4000, "u8-hi"),
                (rgb16, cmyk8, 0u32, "u16->u8"),
                (rgb8, cmyk8, 0x2000, "u8-lowres"),
            ] {
                let t = c::cmsCreateTransform(srgb, inf, cmyk,
                    of, 0, fl);
                eprintln!("opt {name} t={}", !t.is_null());
                if !t.is_null() {
                    let ib = [100u8; 16];
                    let mut ob = [0u8; 16];
                    c::cmsDoTransform(t, ib.as_ptr() as *const _,
                        ob.as_mut_ptr() as *mut _, 1);
                    c::cmsDeleteTransform(t);
                }
            }
            c::cmsCloseProfile(cmyk);
        }
        // u8 RGB->RGB: linearization path (sRGB->sRGB)
        let srgb2 = c::cmsCreate_sRGBProfile();
        let t = c::cmsCreateTransform(srgb, rgb8, srgb2, rgb8, 0, 0);
        eprintln!("rgb->rgb t={}", !t.is_null());
        if !t.is_null() { c::cmsDeleteTransform(t); }
        c::cmsCloseProfile(srgb2);
        c::cmsCloseProfile(srgb);
    }
}

/// Optimized evaluators — u8 matrix-shaper -> PrecalculatedXFORM,
/// 16-bit -> MatShaperEval16, curve-pipeline -> FastEvaluateCurves16.
#[test]
fn optimized_evaluators() {
    let _g = CTX_LOCK.lock().unwrap();
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let r8 = (4u32 << 16) | (3 << 3) | 1;
        let r16 = (4u32 << 16) | (3 << 3) | 2;
        let l8 = (10u32 << 16) | (3 << 3) | 1;
        let l16 = (10u32 << 16) | (3 << 3) | 2;
        // matrix-shaper u8 -> PrecalculatedXFORM
        // matrix-shaper u16 -> MatShaperEval16
        for (inf, of, name) in [
            (r8, l8, "u8-ms"),
            (r16, l16, "u16-ms"),
            (r8, l16, "u8->16"),
            (r16, l8, "16->8"),
        ] {
            let t = c::cmsCreateTransform(srgb, inf, lab, of, 0, 0);
            if !t.is_null() {
                let ib = [0x80u8; 8];
                let mut ob = [0u8; 8];
                c::cmsDoTransform(t, ib.as_ptr() as *const _,
                    ob.as_mut_ptr() as *mut _, 1);
                eprintln!("{name} ok");
                c::cmsDeleteTransform(t);
            }
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
    }
}

/// Optimizer internals — OptimizeByJoiningCurves/Resampling/
/// ComputingLinearization + the resample-sampler callback ->
/// integer-format CLUT transforms.
#[test]
fn optimizer_internals() {
    let _g = CTX_LOCK.lock().unwrap();
    unsafe {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let l16 = (10u32 << 16) | (3 << 3) | 2;
        let r16 = (4u32 << 16) | (3 << 3) | 2;
        let r8 = (4u32 << 16) | (3 << 3) | 1;
        // corpus CLUT profiles + flag combos hit the optimizers
        for f in ["lcms2cmyk.icc", "sRGBlcms2.icc", "aRGBlcms2.icc",
                  "lcms2link.icc"] {
            let Ok(d) = std::fs::read(dir.join(f)) else { continue };
            let p = c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32);
            if p.is_null() { continue; }
            for (flags, name) in [
                (0u32, "default"), (0x00000001, "nobpc"),
                (0x00000200, "noresample"), (0x00000400, "nocache"),
                (0x00000040, "highresprecalc"),
            ] {
                for (fmt, fn_) in [(r16, "16"), (r8, "8")] {
                    let t = c::cmsCreateTransform(p, fmt, lab, l16,
                        0, flags);
                    if !t.is_null() {
                        c::cmsDeleteTransform(t);
                    }
                }
            }
            eprintln!("{f} ok");
            c::cmsCloseProfile(p);
        }
        c::cmsCloseProfile(lab);
    }
}

/// Optimizer internals — linearization + resample paths through
/// LUT-based profile transforms with optimization flags.
#[test]
fn optimizer_internals2() {
    let _g = CTX_LOCK.lock().unwrap();
    unsafe {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        for f in ["lcms2cmyk.icc", "sRGBlcms2.icc",
                  "lcms2link.icc", "labv2lcms2.icc"] {
            let Ok(d) = std::fs::read(dir.join(f)) else { continue };
            let p = c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32);
            if p.is_null() { continue; }
            let l16 = (10u32 << 16) | (3 << 3) | 2;
            for flags in [0u32, 0x0002, 0x0040, 0x0400, 0x0200,
                          0x0004, 0x0008, 0xffff] {
                let t = c::cmsCreateTransform(lab, l16, p, l16,
                    0, flags);
                if !t.is_null() { c::cmsDeleteTransform(t); }
            }
            eprintln!("{f} done");
            c::cmsCloseProfile(p);
        }
        c::cmsCloseProfile(lab);
    }
}

/// Optimizer internals 3 — float-join-curves + linearization
/// through real LUT profiles under all optimization flags.
#[test]
fn optimizer_internals3() {
    let _g = CTX_LOCK.lock().unwrap();
    unsafe {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../");
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let l16 = (10u32 << 16) | (3 << 3) | 2;
        let lf32 = (10u32 << 16) | (3 << 3) | 4;
        for f in ["lcms2cmyk.icc", "sRGBlcms2.icc", "lcms2link.icc",
                  "labv2lcms2.icc", "lcms2mxyz.icc", "lcms2mgray.icc"] {
            let Ok(d) = std::fs::read(dir.join(f)) else { continue };
            let p = c::cmsOpenProfileFromMem(
                d.as_ptr() as *const _, d.len() as u32);
            if p.is_null() { continue; }
            for flags in [0u32, 0x0002, 0x0040, 0x0200, 0x0400,
                          0x0001, 0x0100, 0x2000, 0x8000, 0xffff] {
                for ifmt in [l16, lf32] {
                    let t = c::cmsCreateTransform(lab, ifmt, p, l16,
                        0, flags);
                    if !t.is_null() { c::cmsDeleteTransform(t); }
                }
            }
            eprintln!("{f} done");
            c::cmsCloseProfile(p);
        }
        c::cmsCloseProfile(lab);
    }
}

/// Optimizer evaluator internals — precalculated evaluators get
/// *called* on pixels after optimization replaces the pipeline.
#[test]
fn opt_eval_internals() {
    let _g = CTX_LOCK.lock().unwrap();
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let gray = c::cmsCreateGrayProfile(std::ptr::null_mut(), g);
        let r8 = (4u32 << 16) | (3 << 3) | 1;
        let r16 = (4u32 << 16) | (3 << 3) | 2;
        let l8 = (10u32 << 16) | (3 << 3) | 1;
        let l16 = (10u32 << 16) | (3 << 3) | 2;
        let g8 = (3u32 << 16) | (1 << 3) | 1;
        let g16 = (3u32 << 16) | (1 << 3) | 2;
        let flags = [0x0002u32, 0x0040, 0x0200, 0x0002 | 0x0040,
                     0x0002 | 0x0200, 0xffff];
        for f in flags {
            for (inp, outp, if_, of_, name) in [
                (srgb, lab, r8, l8, "rgb-lab-8"),
                (srgb, lab, r16, l16, "rgb-lab-16"),
                (lab, srgb, l8, r8, "lab-rgb-8"),
                (lab, srgb, l16, r16, "lab-rgb-16"),
                (gray, lab, g8, l8, "gray-lab-8"),
                (gray, lab, g16, l16, "gray-lab-16"),
                (srgb, gray, r8, g8, "rgb-gray-8"),
                (srgb, gray, r16, g16, "rgb-gray-16"),
            ] {
                let t = c::cmsCreateTransform(inp, if_, outp, of_,
                    0, f);
                if !t.is_null() {
                    let ib8 = [0x40u8; 4];
                    let ib16 = [0x4000u16; 4];
                    let mut ob = [0u8; 8];
                    if if_ & 3 == 1 {
                        c::cmsDoTransform(t, ib8.as_ptr() as *const _,
                            ob.as_mut_ptr() as *mut _, 1);
                    } else {
                        c::cmsDoTransform(t, ib16.as_ptr() as *const _,
                            ob.as_mut_ptr() as *mut _, 1);
                    }
                    c::cmsDeleteTransform(t);
                }
            }
            eprintln!("flags={f:#x} done");
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
        c::cmsCloseProfile(gray);
        c::cmsFreeToneCurve(g);
    }
}

/// Float optimizer internals — float transforms under all opt
/// flags call the precalculated float evaluators.
#[test]
fn opt_float_eval_internals() {
    let _g = CTX_LOCK.lock().unwrap();
    unsafe {
        let srgb = c::cmsCreate_sRGBProfile();
        let lab = c::cmsCreateLab4Profile(std::ptr::null_mut());
        let g = c::cmsBuildGamma(std::ptr::null_mut(), 2.2);
        let gray = c::cmsCreateGrayProfile(std::ptr::null_mut(), g);
        let fl = 1u32 << 22;
        let rfl = (4u32 << 16) | (3 << 3) | 4 | fl;
        let lfl = (10u32 << 16) | (3 << 3) | 4 | fl;
        let gfl = (3u32 << 16) | (1 << 3) | 4 | fl;
        for f in [0x0002u32, 0x0040, 0x0200, 0x0002 | 0x0040,
                  0x0002 | 0x0200, 0xffff] {
            for (inp, outp, if_, of_, name) in [
                (srgb, lab, rfl, lfl, "rgb-lab"),
                (lab, srgb, lfl, rfl, "lab-rgb"),
                (gray, lab, gfl, lfl, "gray-lab"),
                (srgb, gray, rfl, gfl, "rgb-gray"),
                (srgb, srgb, rfl, rfl, "rgb-rgb"),
            ] {
                let t = c::cmsCreateTransform(inp, if_, outp, of_,
                    0, f);
                if !t.is_null() {
                    let ib = [0.5f32; 4];
                    let mut ob = [0.5f32; 4];
                    for v in [0.1f32, 0.5, 0.9] {
                        let mut inp_ = [v; 4];
                        c::cmsDoTransform(t, inp_.as_ptr() as *const _,
                            ob.as_mut_ptr() as *mut _, 1);
                    }
                    c::cmsDeleteTransform(t);
                }
            }
            eprintln!("flags={f:#x} done");
        }
        c::cmsCloseProfile(srgb);
        c::cmsCloseProfile(lab);
        c::cmsCloseProfile(gray);
        c::cmsFreeToneCurve(g);
    }
}
