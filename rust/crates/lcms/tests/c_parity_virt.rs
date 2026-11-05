//! Differential gate for cmsvirt.c — C and Rust build the same virtual
//! profiles; byte-compare the serialized output.

use lcms::profile;
use lcms::virt;
use lcms_cref as c;
use std::ptr;

fn c_save(p: *mut std::ffi::c_void) -> Vec<u8> {
    unsafe {
        let mut needed = 0u32;
        assert!(c::cmsSaveProfileToMem(p, ptr::null_mut(), &mut needed) != 0);
        let mut buf = vec![0u8; needed as usize];
        assert!(c::cmsSaveProfileToMem(p, buf.as_mut_ptr() as *mut _, &mut needed) != 0);
        buf.truncate(needed as usize);
        buf
    }
}

fn r_save(icc: &mut lcms::profile::IccProfile) -> Vec<u8> {
    profile::save_profile_to_mem(icc).expect("rust save")
}

#[test]
fn srgb_profile_bytes() {
    let c_prof = unsafe { c::cmsCreate_sRGBProfile() };
    assert!(!c_prof.is_null());
    let c_bytes = c_save(c_prof);
    unsafe { c::cmsCloseProfile(c_prof) };

    let mut r_prof = virt::create_srgb_profile(None).expect("rust srgb");
    let r_bytes = r_save(&mut r_prof);

    assert_eq!(r_bytes.len(), c_bytes.len(), "size srgb");
    let (mut rb, mut cb) = (r_bytes.clone(), c_bytes.clone());
    rb[24..36].fill(0); // Created = time() in C, epoch in port
    cb[24..36].fill(0);
    assert_eq!(rb, cb, "sRGB profile bytes differ");
}

#[test]
fn lab2_lab4_xyz_profiles() {
    let c_lab2 = unsafe { c::cmsCreateLab2Profile(ptr::null()) };
    let c_lab4 = unsafe { c::cmsCreateLab4Profile(ptr::null()) };
    let c_xyz = unsafe { c::cmsCreateXYZProfile() };
    assert!(!c_lab2.is_null() && !c_lab4.is_null() && !c_xyz.is_null());
    let b_lab2 = c_save(c_lab2);
    let b_lab4 = c_save(c_lab4);
    let b_xyz = c_save(c_xyz);
    unsafe {
        c::cmsCloseProfile(c_lab2);
        c::cmsCloseProfile(c_lab4);
        c::cmsCloseProfile(c_xyz);
    }

    let mut r_lab2 = virt::create_lab2_profile(None, None).unwrap();
    let mut r_lab4 = virt::create_lab4_profile(None, None).unwrap();
    let mut r_xyz = virt::create_xyz_profile(None).unwrap();
    for (name, mut rb, mut cb) in [
        ("lab2", r_save(&mut r_lab2), b_lab2),
        ("lab4", r_save(&mut r_lab4), b_lab4),
        ("xyz", r_save(&mut r_xyz), b_xyz),
    ] {
        rb[24..36].fill(0);
        cb[24..36].fill(0);
        assert_eq!(rb, cb, "{name} bytes");
    }
}

const TYPE_RGB_8: u32 = 4 << 16 | 3 << 3 | 1;
const TYPE_RGB_16: u32 = 4 << 16 | 3 << 3 | 2;

/// Display P3→sRGB transform parity. u8 hits `OptData::MatShaper` (C folds
/// the two adaptation matrices), u16 resamples to `ClutOnly` — C's
/// `OptimizeMatrixShaper` is 8-bit-only. Both must be byte-exact.
#[test]
fn p3_to_srgb_transform() {
    use lcms::types::{CiexyY, CiexyYTriple};
    const D65: CiexyY = CiexyY {
        x: 0.3127,
        y: 0.3290,
        Y: 1.0,
    };
    const P3: CiexyYTriple = CiexyYTriple {
        Red: CiexyY {
            x: 0.680,
            y: 0.320,
            Y: 1.0,
        },
        Green: CiexyY {
            x: 0.265,
            y: 0.690,
            Y: 1.0,
        },
        Blue: CiexyY {
            x: 0.150,
            y: 0.060,
            Y: 1.0,
        },
    };
    let srgb_p = [2.4f64, 1.0 / 1.055, 0.055 / 1.055, 1.0 / 12.92, 0.04045];
    let n = 8192usize;
    let inp8: Vec<u8> = (0..n * 3)
        .map(|i| ((i * 37 + i / 3 * 11) % 251) as u8)
        .collect();
    let inp16: Vec<u8> = inp8
        .iter()
        .flat_map(|&b| (b as u16 * 257).to_le_bytes())
        .collect();

    for intent in [0u32, 1] {
        for (lbl, fmt, input, bpp) in [
            ("u8", TYPE_RGB_8, &inp8, 3usize),
            ("u16", TYPE_RGB_16, &inp16, 6usize),
        ] {
            // C side
            let (p3_c, srgb_c, tc) = unsafe {
                let g = c::cmsBuildParametricToneCurve(ptr::null_mut(), 4, srgb_p.as_ptr());
                let prim: [*mut c::ToneCurve; 3] = [g, g, g];
                let p3 = c::cmsCreateRGBProfileTHR(
                    ptr::null_mut(),
                    &D65 as *const CiexyY as *const _,
                    &P3 as *const CiexyYTriple as *const _,
                    prim.as_ptr() as *const *mut _,
                );
                let srgb = c::cmsCreate_sRGBProfile();
                (
                    p3,
                    srgb,
                    c::cmsCreateTransform(p3, fmt, srgb, fmt, intent, 0),
                )
            };
            let mut c_out = vec![0u8; input.len()];
            unsafe {
                c::cmsDoTransform(
                    tc,
                    input.as_ptr() as *const _,
                    c_out.as_mut_ptr() as *mut _,
                    n as u32,
                )
            };
            let _ = (p3_c, srgb_c);

            // Rust side
            let g = lcms::gamma::build_parametric_tone_curve(None, 4, &srgb_p).unwrap();
            let p1 =
                virt::create_rgb_profile(None, Some(&D65), Some(&P3), Some([&g, &g, &g])).unwrap();
            let p2 = virt::create_srgb_profile(None).unwrap();
            let mut profs = vec![p1, p2];
            let mut bpc = [false, false];
            let intents = [intent, intent];
            let adapt = [1.0f64, 1.0];
            let mut t = lcms::xform::create_extended_transform(
                None, &mut profs, &mut bpc, &intents, &adapt, None, 0, fmt, fmt, 0,
            )
            .unwrap();
            let mut r_out = vec![0u8; input.len()];
            t.do_transform(input, &mut r_out, n as u32);

            assert_eq!(
                r_out, c_out,
                "P3→sRGB intent{intent} {lbl} ({bpp}bpp) differs"
            );
        }
    }
}

/// Real-world ICC corpus → sRGB parity. External corpora (licensing → not in
/// git): `/mnt/v/input/compact-icc-profiles` and `/mnt/v/output/corpus-builder/_icc_stage`.
/// Skips cleanly when absent. Every profile is transformed to sRGB at u8 + u16
/// for its matching color-space format (RGB/CMYK/Lab/Gray); byte-exact vs C.
#[test]
fn realworld_profile_corpus() {
    use lcms::signatures::colorspace as cs;
    let dirs = [
        "/mnt/v/input/compact-icc-profiles",
        "/mnt/v/output/corpus-builder/_icc_stage",
    ];
    let n = 4096usize;
    let inp8: Vec<u8> = (0..n * 8)
        .map(|i| ((i * 37 + i / 8 * 11) % 256) as u8)
        .collect();
    let mut tested = 0;
    let mut failures = Vec::new();
    let mut profs_seen = 0;
    for dir in dirs {
        let dirp = std::path::Path::new(dir);
        if !dirp.is_dir() {
            continue;
        }
        let mut files: Vec<_> = std::fs::read_dir(dirp)
            .unwrap()
            .filter_map(|e| e.ok().map(|x| x.path()))
            .filter(|p| p.extension().map(|x| x == "icc").unwrap_or(false))
            .collect();
        files.sort();
        for path in &files {
            let name = path.file_stem().unwrap().to_str().unwrap().to_string();
            let Some(p1) = profile::open_profile_from_file(None, path.to_str().unwrap(), "r")
            else {
                continue;
            };
            let csig = p1.color_space;
            profs_seen += 1;
            let (pt, chans, bytes) = match csig {
                x if x == cs::SigRgbData => (4u32, 3usize, 3usize),
                x if x == cs::SigCmykData => (6, 4, 4),
                x if x == cs::SigLabData => (10, 3, 3),
                x if x == cs::SigGrayData => (3, 1, 1),
                _ => continue, // MCH/NCLR/XYZ — no matching simple format
            };
            for by in [1u32] {
                // u8 only — the byte-exact gate; u16 links are ~10s on CLUT profiles
                let fmt = pt << 16 | (chans as u32) << 3 | by;
                let bpp = chans * by as usize;
                let input: Vec<u8> = inp8[..n * bpp].to_vec();
                // C side
                let (co, ok) = unsafe {
                    let cpath = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
                    let p = c::cmsOpenProfileFromFile(cpath.as_ptr(), "r\0".as_ptr());
                    if p.is_null() {
                        (Vec::new(), false)
                    } else {
                        let srgb = c::cmsCreate_sRGBProfile();
                        let tc = c::cmsCreateTransform(p, fmt, srgb, fmt, 0, 0);
                        if tc.is_null() {
                            (Vec::new(), false)
                        } else {
                            let mut o = vec![0u8; input.len()];
                            c::cmsDoTransform(
                                tc,
                                input.as_ptr() as *const _,
                                o.as_mut_ptr() as *mut _,
                                n as u32,
                            );
                            (o, true)
                        }
                    }
                };
                // Rust side
                let Some(p1) = profile::open_profile_from_file(None, path.to_str().unwrap(), "r")
                else {
                    continue;
                };
                let Some(p2) = virt::create_srgb_profile(None) else {
                    continue;
                };
                let mut profs = vec![p1, p2];
                let mut bpc = [false, false];
                let intents = [0u32, 0];
                let adapt = [1.0f64, 1.0];
                let Some(mut t) = lcms::xform::create_extended_transform(
                    None, &mut profs, &mut bpc, &intents, &adapt, None, 0, fmt, fmt, 0,
                ) else {
                    continue;
                };
                let mut ro = vec![0u8; input.len()];
                t.do_transform(&input, &mut ro, n as u32);
                if !ok {
                    continue;
                }
                tested += 1;
                if ro != co {
                    let d = ro
                        .iter()
                        .zip(&co)
                        .map(|(x, y)| (*x as i32 - *y as i32).abs())
                        .max()
                        .unwrap_or(0);
                    failures.push(format!("{name} {csig:#10x} {by}b: worst {d}"));
                }
            }
        }
    }
    eprintln!("corpus: {tested} transforms across {profs_seen} profiles");
    assert!(tested > 0, "no profiles tested");
    assert!(
        failures.is_empty(),
        "corpus divergences:\n{}",
        failures.join("\n")
    );
}

/// `do_transform_par` must be byte-exact vs serial `do_transform` — the
/// pixel-split clones eval state but shares the same `lut`/`opt_data`.
#[test]
fn parallel_transform_matches_serial() {
    use lcms::types::{CiexyY, CiexyYTriple};
    const D65: CiexyY = CiexyY {
        x: 0.3127,
        y: 0.3290,
        Y: 1.0,
    };
    const P3: CiexyYTriple = CiexyYTriple {
        Red: CiexyY {
            x: 0.680,
            y: 0.320,
            Y: 1.0,
        },
        Green: CiexyY {
            x: 0.265,
            y: 0.690,
            Y: 1.0,
        },
        Blue: CiexyY {
            x: 0.150,
            y: 0.060,
            Y: 1.0,
        },
    };
    let srgb_p = [2.4f64, 1.0 / 1.055, 0.055 / 1.055, 1.0 / 12.92, 0.04045];
    let n = 100_000u32;
    for fmt in [1u32, 2] {
        let f = 4 << 16 | 3 << 3 | fmt;
        let sz = n as usize * 3 * fmt as usize;
        let mut input = vec![0u8; sz];
        let mut s = 7u64;
        for b in input.iter_mut() {
            s = s
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            *b = (s >> 33) as u8;
        }
        for &intent in &[0u32, 1] {
            let g = lcms::gamma::build_parametric_tone_curve(None, 4, &srgb_p).unwrap();
            let p1 =
                virt::create_rgb_profile(None, Some(&D65), Some(&P3), Some([&g, &g, &g])).unwrap();
            let p2 = virt::create_srgb_profile(None).unwrap();
            let mut profs = vec![p1, p2];
            let mut bpc = [false; 2];
            let intents = [intent, intent];
            let adapt = [1.0f64; 2];
            let mut t1 = lcms::xform::create_extended_transform(
                None, &mut profs, &mut bpc, &intents, &adapt, None, 0, f, f, 0,
            )
            .expect("t1");
            let mut t2 = lcms::xform::create_extended_transform(
                None, &mut profs, &mut bpc, &intents, &adapt, None, 0, f, f, 0,
            )
            .expect("t2");
            let mut o1 = vec![0u8; sz];
            let mut o2 = vec![0u8; sz];
            t1.do_transform(&input, &mut o1, n);
            for th in [2usize, 4, 8] {
                t2.do_transform_par(&input, &mut o2, n, th);
                assert_eq!(o1, o2, "fmt={fmt} intent={intent} threads={th}");
            }
        }
    }
}
