//! `lcms::api::Plan` vs the C oracle's `cmsCreateTransform` /
//! `cmsCreateMultiprofileTransform` / `cmsCreateProofingTransform`:
//! byte-identical output across intents, flags (including black-point
//! compensation), formats and real testbed profiles.
//!
//! This gates the facade's own linking choices (per-profile BPC from the
//! flags, the global adaptation state, proofing layout), which the
//! `c_parity_*` gates don't see: they drive the parity layer directly.
//! Runs under default features; `lcms-cref` is a dev-dependency.

use lcms::api::{flags, Format, Intent, Plan, PlanOptions, Profile};
use lcms_cref as c;
use std::ffi::c_void;

fn testbed(name: &str) -> Vec<u8> {
    let p = format!("{}/../../../reference/lcms2/testbed/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&p).unwrap_or_else(|e| panic!("testbed profile {p}: {e}"))
}

/// A profile both sides open: from testbed bytes or a built-in.
#[derive(Clone, Copy)]
enum Src {
    File(&'static str),
    Srgb,
    Lab4,
}

impl Src {
    fn rust(self) -> Profile {
        match self {
            Src::File(n) => Profile::open(&testbed(n)).unwrap(),
            Src::Srgb => Profile::srgb(),
            Src::Lab4 => Profile::lab4(),
        }
    }
    fn c(self) -> CProfile {
        let h = match self {
            Src::File(n) => {
                let d = testbed(n);
                unsafe { c::cmsOpenProfileFromMem(d.as_ptr() as *const c_void, d.len() as u32) }
            }
            Src::Srgb => unsafe { c::cmsCreate_sRGBProfile() },
            Src::Lab4 => unsafe { c::cmsCreateLab4Profile(std::ptr::null()) },
        };
        assert!(!h.is_null());
        CProfile(h)
    }
    fn name(self) -> &'static str {
        match self {
            Src::File(n) => n,
            Src::Srgb => "sRGB",
            Src::Lab4 => "Lab4",
        }
    }
}

struct CProfile(*mut c_void);
impl Drop for CProfile {
    fn drop(&mut self) {
        unsafe { c::cmsCloseProfile(self.0) };
    }
}

fn c_run(t: *mut c_void, input: &[u8], n: usize, out_bpp: usize) -> Vec<u8> {
    assert!(!t.is_null(), "C transform failed");
    let mut out = vec![0u8; n * out_bpp];
    unsafe {
        c::cmsDoTransform(t, input.as_ptr() as *const c_void, out.as_mut_ptr() as *mut c_void, n as u32);
        c::cmsDeleteTransform(t);
    }
    out
}

/// Deterministic input sweep: `n` pixels of `fmt` (8- or 16-bit integer).
fn sweep(fmt: Format, n: usize) -> Vec<u8> {
    let bps = fmt.bytes_per_channel();
    let len = n * fmt.pixel_size();
    let mut v = Vec::with_capacity(len);
    let mut s = 0x1234_5678u32;
    while v.len() < len {
        s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        match bps {
            1 => v.push((s >> 24) as u8),
            2 => v.extend_from_slice(&((s >> 16) as u16).to_ne_bytes()),
            _ => unreachable!("integer formats only"),
        }
    }
    // include the extremes
    for b in v.iter_mut().take(fmt.pixel_size()) {
        *b = 0;
    }
    for b in v.iter_mut().skip(fmt.pixel_size()).take(fmt.pixel_size()) {
        *b = 0xff;
    }
    v
}

const N: usize = 2000;

fn check_pair(src: Src, in_fmt: Format, dst: Src, out_fmt: Format, intent: Intent, flags: u32) {
    let input = sweep(in_fmt, N);
    let (rs, rd) = (src.rust(), dst.rust());
    let opts = PlanOptions::new().intent(intent).flags(flags);
    let plan = Plan::with_options(&rs, &rd, in_fmt, out_fmt, &opts);
    let (cs, cd) = (src.c(), dst.c());
    let t = unsafe { c::cmsCreateTransform(cs.0, in_fmt.code(), cd.0, out_fmt.code(), intent.code(), flags) };
    let what = format!("{} {in_fmt:?} -> {} {out_fmt:?} {intent:?} flags {flags:#x}", src.name(), dst.name());
    match plan {
        Err(e) => {
            assert!(t.is_null(), "{what}: Rust failed ({e}) but C built a transform");
        }
        Ok(plan) => {
            let want = c_run(t, &input, N, out_fmt.pixel_size());
            let mut got = vec![0u8; N * out_fmt.pixel_size()];
            plan.transform(&input, &mut got);
            if got != want {
                let i = got.iter().zip(&want).position(|(a, b)| a != b).unwrap();
                panic!("{what}: first difference at byte {i}: rust {} vs C {}", got[i], want[i]);
            }
        }
    }
}

const INTENTS: [Intent; 4] =
    [Intent::Perceptual, Intent::RelativeColorimetric, Intent::Saturation, Intent::AbsoluteColorimetric];

#[test]
fn rgb_to_rgb_all_intents_and_flags() {
    let pairs = [
        (Src::File("test5.icc"), Src::Srgb),
        (Src::Srgb, Src::File("ibm-t61.icc")),
        (Src::File("test3.icc"), Src::File("test4.icc")),
    ];
    let flag_sets = [0, flags::BLACK_POINT_COMPENSATION, flags::NO_OPTIMIZE, flags::HIGH_RES_PRECALC];
    for (s, d) in pairs {
        for intent in INTENTS {
            for flags in flag_sets {
                check_pair(s, Format::RGB_8, d, Format::RGB_8, intent, flags);
                check_pair(s, Format::RGB_16, d, Format::RGB_16, intent, flags);
            }
        }
    }
}

#[test]
fn cmyk_links_all_intents_with_bpc() {
    for intent in INTENTS {
        for flags in [0, flags::BLACK_POINT_COMPENSATION] {
            check_pair(Src::File("test1.icc"), Format::CMYK_8, Src::Srgb, Format::RGB_8, intent, flags);
            check_pair(Src::Srgb, Format::RGB_8, Src::File("test2.icc"), Format::CMYK_8, intent, flags);
            check_pair(Src::File("test1.icc"), Format::CMYK_16, Src::File("test2.icc"), Format::CMYK_16, intent, flags);
        }
    }
}

#[test]
fn to_lab_and_gray() {
    for intent in INTENTS {
        check_pair(Src::File("test5.icc"), Format::RGB_8, Src::Lab4, Format::LAB_16, intent, 0);
        check_pair(Src::Srgb, Format::RGB_16, Src::File("graylcms2.icc"), Format::GRAY_16, intent, 0);
    }
}

#[test]
fn multiprofile_matches_c() {
    let chain = [Src::Srgb, Src::File("bchslcms2.icc"), Src::File("test1.icc")];
    for intent in [Intent::Perceptual, Intent::RelativeColorimetric] {
        for flags in [0, flags::BLACK_POINT_COMPENSATION] {
            let rust: Vec<Profile> = chain.iter().map(|s| s.rust()).collect();
            let refs: Vec<&Profile> = rust.iter().collect();
            let plan = Plan::new_multiprofile(&refs, Format::RGB_8, Format::CMYK_8, intent, flags).unwrap();
            let cps: Vec<CProfile> = chain.iter().map(|s| s.c()).collect();
            let mut hs: Vec<*mut c_void> = cps.iter().map(|p| p.0).collect();
            let t = unsafe {
                c::cmsCreateMultiprofileTransform(
                    hs.as_mut_ptr(),
                    hs.len() as i32,
                    Format::RGB_8.code(),
                    Format::CMYK_8.code(),
                    intent.code(),
                    flags,
                )
            };
            let input = sweep(Format::RGB_8, N);
            let want = c_run(t, &input, N, 4);
            let mut got = vec![0u8; N * 4];
            plan.transform(&input, &mut got);
            assert_eq!(got, want, "multiprofile {intent:?} flags {flags:#x}");
        }
    }
}

#[test]
fn proofing_matches_c() {
    let (src, dst, proof) = (Src::Srgb, Src::Srgb, Src::File("test1.icc"));
    for flags in [flags::SOFT_PROOFING, flags::GAMUT_CHECK, flags::SOFT_PROOFING | flags::BLACK_POINT_COMPENSATION] {
        for proofing_intent in [Intent::RelativeColorimetric, Intent::AbsoluteColorimetric] {
            let plan = Plan::new_proofing(
                &src.rust(),
                &dst.rust(),
                &proof.rust(),
                Format::RGB_8,
                Format::RGB_8,
                Intent::Perceptual,
                proofing_intent,
                flags,
            )
            .unwrap();
            let (cs, cd, cp) = (src.c(), dst.c(), proof.c());
            let t = unsafe {
                c::cmsCreateProofingTransform(
                    cs.0,
                    Format::RGB_8.code(),
                    cd.0,
                    Format::RGB_8.code(),
                    cp.0,
                    Intent::Perceptual.code(),
                    proofing_intent.code(),
                    flags,
                )
            };
            let input = sweep(Format::RGB_8, N);
            let want = c_run(t, &input, N, 3);
            let mut got = vec![0u8; N * 3];
            plan.transform(&input, &mut got);
            if got != want {
                let px: Vec<usize> = (0..N).filter(|&i| got[i * 3..i * 3 + 3] != want[i * 3..i * 3 + 3]).collect();
                let i = px[0];
                panic!(
                    "proofing flags {flags:#x} {proofing_intent:?}: {} of {N} pixels differ; first #{i}: in {:?} rust {:?} C {:?}",
                    px.len(), &input[i * 3..i * 3 + 3], &got[i * 3..i * 3 + 3], &want[i * 3..i * 3 + 3]
                );
            }
        }
    }
}

/// An explicit `PlanOptions::adaptation_state` matches C with the same
/// global `cmsSetAdaptationState` (absolute colorimetric uses it).
#[test]
fn adaptation_state_matches_c() {
    let (src, dst) = (Src::File("test5.icc"), Src::Lab4);
    for state in [0.0, 0.5] {
        let plan = Plan::with_options(
            &src.rust(),
            &dst.rust(),
            Format::RGB_8,
            Format::LAB_16,
            &PlanOptions::new().intent(Intent::AbsoluteColorimetric).adaptation_state(state),
        )
        .unwrap();
        // C reads the adaptation state from a context; use a private one so
        // the global default stays 1.0 for the other tests.
        let ctx = unsafe { c::cmsCreateContext(std::ptr::null_mut(), std::ptr::null_mut()) };
        assert!(!ctx.is_null());
        unsafe { c::cmsSetAdaptationStateTHR(ctx, state) };
        let (cs, cd) = (src.c(), dst.c());
        let t = unsafe {
            c::cmsCreateTransformTHR(
                ctx,
                cs.0,
                Format::RGB_8.code(),
                cd.0,
                Format::LAB_16.code(),
                Intent::AbsoluteColorimetric.code(),
                0,
            )
        };
        let input = sweep(Format::RGB_8, N);
        let want = c_run(t, &input, N, 6);
        unsafe { c::cmsDeleteContext(ctx) };
        let mut got = vec![0u8; N * 6];
        plan.transform(&input, &mut got);
        assert_eq!(got, want, "adaptation state {state}");
    }
}
