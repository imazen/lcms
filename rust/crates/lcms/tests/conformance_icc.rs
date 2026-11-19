//! ICC conformance gate.
//!
//! Every profile the port can write is checked against the ICC
//! specifications by `tests/support/icc_spec.rs`, a checker written from the
//! spec texts (ICC.1:2022 v4.4, ICC.1:2001-04 v2.4 and the 2026 ICC.1
//! amendments, archived in imazen-private/standards-ark). The same profile
//! is produced by the C oracle, lcms2.20, and checked the same way.
//!
//! - A finding the port has and the oracle does not is a port bug: the
//!   test fails.
//! - A finding both have is lcms2.20 behaviour. Each one must be listed in
//!   [`KNOWN_LCMS2_DEVIATIONS`] with its reason (and in
//!   `docs/CONFORMANCE.md`); an unlisted one fails the test.
//!
//! The second half checks the port's arithmetic against values printed in
//! the specifications: number and PCS encodings, the parametric and segment
//! curve formulas, the Bradford matrix, the profile ID, and the CIEDE2000
//! test data of Sharma, Wu and Dalal (2005).
//!
//! `LCMS_CONFORMANCE_DUMP=<dir>` writes every checked profile there, for an
//! independent validator (`just icc-validate`).

#[path = "support/icc_spec.rs"]
mod icc_spec;

use icc_spec::{Finding, check, sig, sig_str};
use lcms::api::{self, Format, Intent, Plan, Profile, ToneCurve};
use lcms::profile::{self, IccProfile};
use lcms::types::{CieLab, CieXyz, CiexyY, CiexyYTriple};
use lcms::virt;
use lcms_cref as c;
use std::collections::BTreeMap;
use std::ffi::c_void;
use std::ptr;

// -----------------------------------------------------------------------
// Profiles under test
// -----------------------------------------------------------------------

fn testbed_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../reference/lcms2/testbed")
}

fn testbed(name: &str) -> Vec<u8> {
    let p = testbed_dir().join(name);
    std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

struct CProf(*mut c_void);
impl Drop for CProf {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { c::cmsCloseProfile(self.0) };
        }
    }
}

/// Save a C profile, optionally at another version and with a profile ID.
fn c_save(h: *mut c_void, version: Option<f64>, id: bool) -> Option<Vec<u8>> {
    if h.is_null() {
        return None;
    }
    let _g = CProf(h);
    unsafe {
        if let Some(v) = version {
            c::cmsSetProfileVersion(h, v);
        }
        if id && c::cmsMD5computeID(h) == 0 {
            return None;
        }
        let mut n = 0u32;
        if c::cmsSaveProfileToMem(h, ptr::null_mut(), &mut n) == 0 || n == 0 {
            return None;
        }
        let mut buf = vec![0u8; n as usize];
        if c::cmsSaveProfileToMem(h, buf.as_mut_ptr() as *mut c_void, &mut n) == 0 {
            return None;
        }
        buf.truncate(n as usize);
        Some(buf)
    }
}

/// Open in C, read every tag (so the save re-encodes each one with C's
/// writers instead of copying the bytes), save.
fn c_reencode(bytes: &[u8]) -> Option<Vec<u8>> {
    unsafe {
        let h = c::cmsOpenProfileFromMem(bytes.as_ptr() as *const c_void, bytes.len() as u32);
        if h.is_null() {
            return None;
        }
        for i in 0..c::cmsGetTagCount(h).max(0) as u32 {
            c::cmsReadTag(h, c::cmsGetTagSignature(h, i));
        }
        c_save(h, None, false)
    }
}

fn port_save(icc: &IccProfile, version: Option<f64>, id: bool) -> Option<Vec<u8>> {
    let mut icc = icc.clone();
    if let Some(v) = version {
        profile::set_profile_version(&mut icc, v);
    }
    if id && !lcms::md5::compute_id(&mut icc) {
        return None;
    }
    profile::save_profile_to_mem(&mut icc)
}

/// The port's counterpart of [`c_reencode`].
fn port_reencode(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut icc = profile::open_profile_from_mem(None, bytes)?;
    let sigs: Vec<u32> = icc.tag_names[..icc.tag_count as usize].to_vec();
    for s in sigs {
        let _ = profile::cms_read_tag(&mut icc, s);
    }
    profile::save_profile_to_mem(&mut icc)
}

struct Case {
    name: String,
    port: Option<Vec<u8>>,
    c: Option<Vec<u8>>,
    /// For re-encoded inputs: the input itself. Findings it already has
    /// belong to the input, not to either writer.
    source: Option<Vec<u8>>,
}

fn xyy(x: f64, y: f64) -> CiexyY {
    CiexyY { x, y, Y: 1.0 }
}
fn d50() -> CiexyY {
    lcms::wtpnt::d50_xyy()
}

const TYPE_RGB_8: u32 = (4 << 16) | (3 << 3) | 1;
const TYPE_LAB_16: u32 = (10 << 16) | (3 << 3) | 2;
const TYPE_CMYK_8: u32 = (6 << 16) | (4 << 3) | 1;
const TYPE_GRAY_8: u32 = (3 << 16) | (1 << 3) | 1;

fn gamma22_c() -> *mut c::ToneCurve {
    unsafe { c::cmsBuildGamma(ptr::null_mut(), 2.2) }
}

/// Every profile-producing entry point, through the port and through C.
fn generated_cases() -> Vec<Case> {
    let mut out = Vec::new();
    let g = ToneCurve::gamma(2.2).unwrap();
    let d65 = xyy(0.3127, 0.3290);
    let d50 = d50();
    let p3 = CiexyYTriple {
        Red: xyy(0.680, 0.320),
        Green: xyy(0.265, 0.690),
        Blue: xyy(0.150, 0.060),
    };
    let null = ptr::null_mut();

    // (name, port builder, C builder)
    type Pb<'a> = Box<dyn Fn() -> Option<Box<IccProfile>> + 'a>;
    type Cb<'a> = Box<dyn Fn() -> *mut c_void + 'a>;
    let builders: Vec<(&str, Pb, Cb)> = vec![
        (
            "sRGB",
            Box::new(|| virt::create_srgb_profile(None)),
            Box::new(|| unsafe { c::cmsCreate_sRGBProfile() }),
        ),
        (
            "Lab4",
            Box::new(|| virt::create_lab4_profile(None, None)),
            Box::new(|| unsafe { c::cmsCreateLab4Profile(ptr::null()) }),
        ),
        (
            "Lab4-D65",
            Box::new(|| virt::create_lab4_profile(None, Some(&d65))),
            Box::new(|| unsafe { c::cmsCreateLab4Profile(&d65 as *const CiexyY as *const c_void) }),
        ),
        (
            "Lab2",
            Box::new(|| virt::create_lab2_profile(None, None)),
            Box::new(|| unsafe { c::cmsCreateLab2Profile(ptr::null()) }),
        ),
        (
            "XYZ",
            Box::new(|| virt::create_xyz_profile(None)),
            Box::new(|| unsafe { c::cmsCreateXYZProfile() }),
        ),
        (
            "NULL",
            Box::new(|| virt::create_null_profile(None)),
            Box::new(|| unsafe { c::cmsCreateNULLProfile() }),
        ),
        (
            "Gray-D50",
            Box::new(|| virt::create_gray_profile(None, Some(&d50), Some(&g))),
            Box::new(|| unsafe {
                let cv = gamma22_c();
                let h = c::cmsCreateGrayProfile(&d50 as *const CiexyY as *const c_void, cv);
                c::cmsFreeToneCurve(cv);
                h
            }),
        ),
        (
            "Gray-D65",
            Box::new(|| virt::create_gray_profile(None, Some(&d65), Some(&g))),
            Box::new(|| unsafe {
                let cv = gamma22_c();
                let h = c::cmsCreateGrayProfile(&d65 as *const CiexyY as *const c_void, cv);
                c::cmsFreeToneCurve(cv);
                h
            }),
        ),
        (
            "RGB-P3-D65",
            Box::new(|| virt::create_rgb_profile(None, Some(&d65), Some(&p3), Some([&g, &g, &g]))),
            Box::new(|| unsafe {
                let cv = gamma22_c();
                let t = [cv as *mut c_void, cv as *mut c_void, cv as *mut c_void];
                let h = c::cmsCreateRGBProfile(
                    &d65 as *const CiexyY as *const c_void,
                    &p3 as *const CiexyYTriple as *const c_void,
                    t.as_ptr(),
                );
                c::cmsFreeToneCurve(cv);
                h
            }),
        ),
        (
            "RGB-P3-D50",
            Box::new(|| virt::create_rgb_profile(None, Some(&d50), Some(&p3), Some([&g, &g, &g]))),
            Box::new(|| unsafe {
                let cv = gamma22_c();
                let t = [cv as *mut c_void, cv as *mut c_void, cv as *mut c_void];
                let h = c::cmsCreateRGBProfile(
                    &d50 as *const CiexyY as *const c_void,
                    &p3 as *const CiexyYTriple as *const c_void,
                    t.as_ptr(),
                );
                c::cmsFreeToneCurve(cv);
                h
            }),
        ),
        (
            "LinLink-RGB",
            Box::new(|| virt::create_linearization_device_link(None, sig(b"RGB "), &[&g, &g, &g])),
            Box::new(|| unsafe {
                let cv = gamma22_c();
                let t = [cv as *mut c_void, cv as *mut c_void, cv as *mut c_void];
                let h = c::cmsCreateLinearizationDeviceLink(sig(b"RGB "), t.as_ptr());
                c::cmsFreeToneCurve(cv);
                h
            }),
        ),
        (
            "LinLink-CMYK",
            Box::new(|| {
                virt::create_linearization_device_link(None, sig(b"CMYK"), &[&g, &g, &g, &g])
            }),
            Box::new(|| unsafe {
                let cv = gamma22_c();
                let t = [cv as *mut c_void; 4];
                let h = c::cmsCreateLinearizationDeviceLink(sig(b"CMYK"), t.as_ptr());
                c::cmsFreeToneCurve(cv);
                h
            }),
        ),
        (
            "InkLimit-CMYK",
            Box::new(|| virt::create_ink_limiting_device_link(None, sig(b"CMYK"), 250.0)),
            Box::new(|| unsafe { c::cmsCreateInkLimitingDeviceLink(sig(b"CMYK"), 250.0) }),
        ),
        (
            "BCHSW",
            Box::new(|| {
                virt::create_bchsw_abstract_profile(None, 17, 10.0, 1.2, 5.0, 1.1, 5000, 6500)
            }),
            Box::new(|| unsafe {
                c::cmsCreateBCHSWabstractProfile(17, 10.0, 1.2, 5.0, 1.1, 5000, 6500)
            }),
        ),
        (
            "OkLab",
            Box::new(|| virt::create_oklab_profile(None)),
            Box::new(|| unsafe { c::cmsCreate_OkLabProfile(null) }),
        ),
    ];

    for (name, pb, cb) in &builders {
        // As built (v4.4 for all but Lab2), as v2.1, and v4 with a profile ID.
        for (suffix, version, id) in [
            ("", None, false),
            ("@2.1", Some(2.1), false),
            ("+ID", None, true),
        ] {
            let port = pb().and_then(|icc| port_save(&icc, version, id));
            let cb_out = c_save(cb(), version, id);
            out.push(Case {
                name: format!("{name}{suffix}"),
                port,
                c: cb_out,
                source: None,
            });
        }
    }

    // Device links from transforms (cmsTransform2DeviceLink).
    let open_c = |n: &'static str| -> Box<dyn Fn() -> *mut c_void> {
        Box::new(move || {
            let d = testbed(n);
            unsafe { c::cmsOpenProfileFromMem(d.as_ptr() as *const c_void, d.len() as u32) }
        })
    };
    // (name, port source, C source, input format, port dest, C dest, output format)
    type PortSide = Box<dyn Fn() -> Profile>;
    type CSide = Box<dyn Fn() -> *mut c_void>;
    let pairs: [(&str, PortSide, CSide, u32, PortSide, CSide, u32); 4] = [
        // test1.icc carries deviceMfgDesc/deviceModelDesc text, so the
        // profile sequence holds real descriptions.
        (
            "test1>sRGB",
            Box::new(|| Profile::open(&testbed("test1.icc")).unwrap()),
            open_c("test1.icc"),
            TYPE_CMYK_8,
            Box::new(Profile::srgb),
            Box::new(|| unsafe { c::cmsCreate_sRGBProfile() }),
            TYPE_RGB_8,
        ),
        (
            "sRGB>Lab4",
            Box::new(Profile::srgb),
            Box::new(|| unsafe { c::cmsCreate_sRGBProfile() }),
            TYPE_RGB_8,
            Box::new(Profile::lab4),
            Box::new(|| unsafe { c::cmsCreateLab4Profile(ptr::null()) }),
            TYPE_LAB_16,
        ),
        (
            "sRGB>CMYK",
            Box::new(Profile::srgb),
            Box::new(|| unsafe { c::cmsCreate_sRGBProfile() }),
            TYPE_RGB_8,
            Box::new(|| Profile::open(&testbed("lcms2cmyk.icc")).unwrap()),
            Box::new(|| {
                let d = testbed("lcms2cmyk.icc");
                unsafe { c::cmsOpenProfileFromMem(d.as_ptr() as *const c_void, d.len() as u32) }
            }),
            TYPE_CMYK_8,
        ),
        (
            "sRGB>Gray",
            Box::new(Profile::srgb),
            Box::new(|| unsafe { c::cmsCreate_sRGBProfile() }),
            TYPE_RGB_8,
            Box::new(Profile::gray),
            Box::new(|| unsafe {
                let cv = gamma22_c();
                let h = c::cmsCreateGrayProfile(c::cmsD50_xyY() as *const c_void, cv);
                c::cmsFreeToneCurve(cv);
                h
            }),
            TYPE_GRAY_8,
        ),
    ];
    for (name, ps, cs, fi, pd, cd, fo) in &pairs {
        for version in [4.3, 2.1] {
            for flags in [
                0u32,
                api::flags::KEEP_SEQUENCE,
                api::flags::KEEP_SEQUENCE | api::flags::DEVICELINK_8BITS,
            ] {
                // KEEP_SEQUENCE has to reach the transform: that is where the
                // profile sequence is recorded.
                let port = Plan::build(
                    &ps(),
                    &pd(),
                    Format::from_u32(*fi),
                    Format::from_u32(*fo),
                    Intent::Perceptual,
                    flags,
                )
                .ok()
                .and_then(|plan| plan.to_device_link_flags(version, flags).ok())
                .and_then(|p| port_save(p.icc(), None, false));
                let cb_out = unsafe {
                    let (s, d) = (CProf(cs()), CProf(cd()));
                    let t = c::cmsCreateTransform(s.0, *fi, d.0, *fo, 0, flags);
                    if t.is_null() {
                        None
                    } else {
                        let l = c::cmsTransform2DeviceLink(t, version, flags);
                        c::cmsDeleteTransform(t);
                        c_save(l, None, false)
                    }
                };
                out.push(Case {
                    name: format!("{name}@{version}/flags{flags:x}"),
                    port,
                    c: cb_out,
                    source: None,
                });
            }
        }
    }
    out
}

/// A profile carrying every tag type the port writes, as v4.4 and v2.1.
/// The C side re-encodes the port's bytes with C's own writers.
fn kitchen_sink_profiles(mode: api::BehaviorMode) -> Vec<(String, Profile)> {
    use api::{IccData, Mlu, Pipeline, Stage, Tag, TagData};
    let mut out = Vec::new();
    for version in [4.4, 2.1] {
        let mut p = Profile::with_mode(mode).placeholder();
        // Version first: tag types are chosen when a tag is written.
        profile::set_profile_version(p.icc_mut(), version);
        p.set_device_class(api::ProfileClass::Display);
        p.set_color_space(api::ColorSpace::Rgb);
        p.set_pcs(api::ColorSpace::Xyz);
        let g = ToneCurve::gamma(2.2).unwrap();
        let para =
            ToneCurve::parametric(4, &[2.4, 1.0 / 1.055, 0.055 / 1.055, 1.0 / 12.92, 0.04045])
                .unwrap();
        let tab = ToneCurve::tabulated16(&[0, 1000, 9000, 30000, 65535]).unwrap();
        let mlu = Mlu::from("conformance");
        let w = |p: &mut Profile, t: u32, d: TagData| {
            p.write_tag(Tag::from(t), d)
                .unwrap_or_else(|e| panic!("write '{}': {e}", sig_str(t)));
        };
        w(&mut p, sig(b"desc"), TagData::Mlu(mlu.clone()));
        w(
            &mut p,
            sig(b"cprt"),
            TagData::Mlu(Mlu::from("No copyright, use freely")),
        );
        w(&mut p, sig(b"dmnd"), TagData::Mlu(Mlu::from("maker")));
        w(&mut p, sig(b"dmdd"), TagData::Mlu(Mlu::from("model")));
        w(&mut p, sig(b"vued"), TagData::Mlu(Mlu::from("office")));
        w(
            &mut p,
            sig(b"wtpt"),
            TagData::Xyz(CieXyz {
                X: 0.9642,
                Y: 1.0,
                Z: 0.8249,
            }),
        );
        w(
            &mut p,
            sig(b"lumi"),
            TagData::Xyz(CieXyz {
                X: 0.0,
                Y: 160.0,
                Z: 0.0,
            }),
        );
        w(
            &mut p,
            sig(b"rXYZ"),
            TagData::Xyz(CieXyz {
                X: 0.4361,
                Y: 0.2225,
                Z: 0.0139,
            }),
        );
        w(
            &mut p,
            sig(b"gXYZ"),
            TagData::Xyz(CieXyz {
                X: 0.3851,
                Y: 0.7169,
                Z: 0.0971,
            }),
        );
        w(
            &mut p,
            sig(b"bXYZ"),
            TagData::Xyz(CieXyz {
                X: 0.1431,
                Y: 0.0606,
                Z: 0.7141,
            }),
        );
        w(&mut p, sig(b"rTRC"), TagData::Curve(g.clone()));
        if version >= 4.0 {
            w(&mut p, sig(b"gTRC"), TagData::ParametricCurve(para.clone()));
        } else {
            w(&mut p, sig(b"gTRC"), TagData::Curve(para.clone()));
        }
        w(&mut p, sig(b"bTRC"), TagData::Curve(tab.clone()));
        w(
            &mut p,
            sig(b"chad"),
            TagData::S15Fixed16(vec![
                1.0478, 0.0229, -0.0501, 0.0295, 0.9905, -0.0171, -0.0092, 0.0151, 0.7519,
            ]),
        );
        w(
            &mut p,
            sig(b"chrm"),
            TagData::Chromaticity(CiexyYTriple {
                Red: xyy(0.64, 0.33),
                Green: xyy(0.30, 0.60),
                Blue: xyy(0.15, 0.06),
            }),
        );
        w(&mut p, sig(b"targ"), TagData::Text("ICCHDAT FOGRA39"));
        w(&mut p, sig(b"tech"), TagData::Signature(sig(b"CRT ")));
        w(
            &mut p,
            sig(b"calt"),
            TagData::DateTime(api::DateTimeNumber {
                year: 2026,
                month: 11,
                day: 16,
                hours: 12,
                minutes: 0,
                seconds: 0,
            }),
        );
        w(
            &mut p,
            sig(b"meas"),
            TagData::Measurement(api::IccMeasurementConditions {
                observer: 1,
                backing: lcms::types::EncodedXyzNumber { X: 0, Y: 0, Z: 0 },
                geometry: 1,
                flare: 0.01,
                illuminant: 1,
            }),
        );
        w(
            &mut p,
            sig(b"view"),
            TagData::ViewingConditions(api::IccViewingConditions {
                illuminant_xyz: CieXyz {
                    X: 96.42,
                    Y: 100.0,
                    Z: 82.49,
                },
                surround_xyz: CieXyz {
                    X: 19.28,
                    Y: 20.0,
                    Z: 16.50,
                },
                illuminant_type: 1,
            }),
        );
        if version >= 4.0 {
            let mut order = [0xFFu8; 16];
            order[..3].copy_from_slice(&[2, 1, 0]);
            w(&mut p, sig(b"clro"), TagData::ColorantOrder(order));
            let mut colorants = api::NamedColorList::new(0, "", "").unwrap();
            for (name, lab) in [
                ("red", [0x8000u16, 0xD000, 0xB000]),
                ("green", [0xC000, 0x3000, 0xC000]),
                ("blue", [0x4000, 0x9000, 0x2000]),
            ] {
                colorants.push(name, lab, &[]).unwrap();
            }
            w(&mut p, sig(b"clrt"), TagData::NamedColors(colorants));
            w(&mut p, sig(b"ciis"), TagData::Signature(sig(b"scoe")));
            w(&mut p, sig(b"rig0"), TagData::Signature(sig(b"prmg")));
            w(
                &mut p,
                sig(b"cicp"),
                TagData::VideoSignal(api::VideoSignal {
                    colour_primaries: 1,
                    transfer_characteristics: 13,
                    matrix_coefficients: 0,
                    video_full_range_flag: 1,
                }),
            );
            let mut d = api::Dict::new();
            d.add("key", "value").unwrap();
            d.add("second", "").unwrap();
            w(&mut p, sig(b"meta"), TagData::Dict(d));
            // mAB as M, Matrix, B (10.12.1); the same pipeline as mpet in
            // D2B0; mBA as B, CLUT, A (10.13.1).
            let mat = [
                0.4361, 0.3851, 0.1431, 0.2225, 0.7169, 0.0606, 0.0139, 0.0971, 0.7141,
            ];
            let mut pl = Pipeline::new(3, 3).unwrap();
            pl.push(Stage::tone_curves(&[&g, &g, &g]).unwrap()).unwrap();
            pl.push(Stage::matrix(&mat, 3, 3, None).unwrap()).unwrap();
            pl.push(Stage::tone_curves(&[&tab, &tab, &tab]).unwrap())
                .unwrap();
            w(&mut p, sig(b"A2B0"), TagData::Pipeline(pl.clone()));
            // multiProcessElementsType curves are segmented (10.16.2.2):
            // formula below 0, sampled on (0, 1], formula above 1.
            let seg = {
                use lcms::gamma::{CurveSegment, build_segmented_tone_curve};
                let mut lo = CurveSegment {
                    x0: -1e22,
                    x1: 0.0,
                    r#type: 6,
                    ..Default::default()
                };
                lo.params[..4].copy_from_slice(&[1.0, 1.0, 0.0, 0.0]);
                let samples: Vec<f32> = (0..=16).map(|k| (k as f32 / 16.0).powf(2.2)).collect();
                let mid = CurveSegment {
                    x0: 0.0,
                    x1: 1.0,
                    r#type: 0,
                    n_grid_points: 17,
                    sampled_points: Some(samples),
                    ..Default::default()
                };
                let mut hi = CurveSegment {
                    x0: 1.0,
                    x1: 1e22,
                    r#type: 6,
                    ..Default::default()
                };
                hi.params[..4].copy_from_slice(&[1.0, 1.0, 0.0, 0.0]);
                build_segmented_tone_curve(None, &[lo, mid, hi]).unwrap()
            };
            let mut mpe = Pipeline::new(3, 3).unwrap();
            mpe.push(Stage::tone_curves(&[&seg, &seg, &seg]).unwrap())
                .unwrap();
            mpe.push(Stage::matrix(&mat, 3, 3, Some(&[0.01, 0.0, -0.01])).unwrap())
                .unwrap();
            mpe.push(Stage::clut_float(5, 3, 3, None).unwrap()).unwrap();
            w(&mut p, sig(b"D2B0"), TagData::Pipeline(mpe));
            let mut inv = Pipeline::new(3, 3).unwrap();
            inv.push(Stage::tone_curves(&[&g, &g, &g]).unwrap())
                .unwrap();
            inv.push(Stage::clut16(9, 3, 3, None).unwrap()).unwrap();
            inv.push(Stage::tone_curves(&[&g, &g, &g]).unwrap())
                .unwrap();
            w(&mut p, sig(b"B2A0"), TagData::Pipeline(inv));
        } else {
            let mut pl = Pipeline::new(3, 3).unwrap();
            pl.push(Stage::tone_curves(&[&g, &g, &g]).unwrap()).unwrap();
            pl.push(Stage::clut16(9, 3, 3, None).unwrap()).unwrap();
            pl.push(Stage::tone_curves(&[&g, &g, &g]).unwrap()).unwrap();
            w(&mut p, sig(b"A2B0"), TagData::Pipeline(pl.clone()));
            w(&mut p, sig(b"B2A0"), TagData::Pipeline(pl));
            w(
                &mut p,
                sig(b"ps2i"),
                TagData::Data(IccData::text("perceptual").unwrap()),
            );
            w(&mut p, sig(b"scrd"), TagData::Mlu(Mlu::from("screening")));
            w(
                &mut p,
                sig(b"scrn"),
                TagData::Screening(api::Screening {
                    flag: 0,
                    n_channels: 1,
                    channels: [api::ScreeningChannel {
                        frequency: 150.0,
                        angle: 45.0,
                        spot_shape: 2,
                    }; 16],
                }),
            );
            w(
                &mut p,
                sig(b"bfd "),
                TagData::UcrBg(api::UcrBg {
                    ucr: g.clone(),
                    bg: g.clone(),
                    desc: Mlu::from("ucr/bg"),
                }),
            );
            w(
                &mut p,
                sig(b"bkpt"),
                TagData::Xyz(CieXyz {
                    X: 0.0034,
                    Y: 0.0035,
                    Z: 0.0029,
                }),
            );
        }
        w(
            &mut p,
            sig(b"vcgt"),
            TagData::VcgtCurves([g.clone(), g.clone(), g.clone()]),
        );
        out.push((format!("kitchen-sink@{version}"), p));
    }
    // A NamedColor profile (8.9): CMYK device side, PCSLAB.
    for version in [4.4, 2.1] {
        let mut p = Profile::with_mode(mode).placeholder();
        profile::set_profile_version(p.icc_mut(), version);
        p.set_device_class(api::ProfileClass::NamedColor);
        p.set_color_space(api::ColorSpace::Cmyk);
        p.set_pcs(api::ColorSpace::Lab);
        let w = |p: &mut Profile, t: u32, d: TagData| {
            p.write_tag(Tag::from(t), d)
                .unwrap_or_else(|e| panic!("write '{}': {e}", sig_str(t)));
        };
        w(
            &mut p,
            sig(b"desc"),
            TagData::Mlu(Mlu::from("spot colours")),
        );
        w(
            &mut p,
            sig(b"cprt"),
            TagData::Mlu(Mlu::from("No copyright, use freely")),
        );
        w(
            &mut p,
            sig(b"wtpt"),
            TagData::Xyz(CieXyz {
                X: 0.9642,
                Y: 1.0,
                Z: 0.8249,
            }),
        );
        let mut list = api::NamedColorList::new(4, "PANTONE ", " C").unwrap();
        list.push(
            "Warm Red",
            [0x8B00, 0xC000, 0xB800],
            &[0, 0xD000, 0xFFFF, 0],
        )
        .unwrap();
        list.push(
            "Reflex Blue",
            [0x2E00, 0x9200, 0x2300],
            &[0xFFFF, 0xB000, 0, 0x0400],
        )
        .unwrap();
        w(&mut p, sig(b"ncl2"), TagData::NamedColors(list));
        out.push((format!("named-color@{version}"), p));
    }
    out
}

fn kitchen_sink_cases() -> Vec<Case> {
    kitchen_sink_profiles(api::BehaviorMode::Lcms2Exact)
        .into_iter()
        .map(|(name, p)| {
            let port = profile::save_profile_to_mem(&mut p.icc().clone());
            assert!(port.is_some(), "{name} saves");
            let c = port.as_deref().and_then(c_reencode);
            Case {
                name,
                port: port.and_then(|b| port_reencode(&b)),
                c,
                source: None,
            }
        })
        .collect()
}

/// Every testbed profile, re-encoded through each side's writers.
fn testbed_cases() -> Vec<Case> {
    let mut names: Vec<String> = std::fs::read_dir(testbed_dir())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".icc") || n.ends_with(".icm"))
        // Deliberately malformed inputs for the robustness gates.
        .filter(|n| n != "bad.icc" && n != "bad_mpe.icc")
        .collect();
    names.sort();
    names
        .into_iter()
        .map(|n| {
            let b = testbed(&n);
            Case {
                name: format!("testbed/{n}"),
                port: port_reencode(&b),
                c: c_reencode(&b),
                source: Some(b),
            }
        })
        .collect()
}

/// Cases neither side can write. `toosmall.icc` is a truncated testbed
/// input that neither side opens; lcms2's v2 writer cannot store the NULL
/// profile's 3-to-1 matrix-only B2A0 as a lut16Type (`cmsCreateNULLProfile`);
/// and lcms2.20 cannot save the OkLab virtual profile in any version.
const BOTH_FAIL: &[&str] = &[
    "NULL@2.1",
    "OkLab",
    "OkLab@2.1",
    "OkLab+ID",
    "testbed/toosmall.icc",
];

/// lcms2.20 behaviour that the checker flags. The port reproduces each one
/// byte for byte (the oracle comparison above), so none is a port bug.
/// IDs match `docs/CONFORMANCE.md`, which gives the C source location.
///
/// `(id, clause, tag (None = any), message substring, case-name substring)`.
const KNOWN_LCMS2_DEVIATIONS: &[(&str, &str, Option<&str>, &str, &str)] = &[
    // cmsTransform2DeviceLink writes 'pseq' only under cmsFLAGS_KEEP_SEQUENCE.
    (
        "D1",
        "ICC.1:2022 8.6",
        None,
        "required tag 'pseq' missing",
        "/flags0",
    ),
    (
        "D1",
        "ICC.1:2001-04 6.3.4.1",
        None,
        "required tag 'pseq' missing",
        "/flags0",
    ),
    // cmsCreateNULLProfile: an Output profile with only B2A0, a 3-to-1
    // lutBToAType through a matrix and no CLUT.
    ("D2", "ICC.1:2022 8.5.2", None, "required tag '", "NULL"),
    (
        "D2",
        "ICC.1:2022 10.13",
        Some("B2A0"),
        "3 in / 1 out channels without a CLUT",
        "NULL",
    ),
    // cmsCreateGrayProfile stores the white point unadapted as 'wtpt' of a
    // v4 Display profile.
    (
        "D3",
        "ICC.1:2022 9.2.36",
        Some("wtpt"),
        "display media white point",
        "Gray-D65",
    ),
    // textDescriptionType is written padded to 4 bytes inside the type.
    (
        "D4",
        "ICC.1:2001-04 6.2.3",
        None,
        "tag size includes trailing pad bytes",
        "",
    ),
    (
        "D4",
        "ICC.1:2022 10.19",
        Some("pseq"),
        "pad bytes between embedded descriptions",
        "@2.1",
    ),
    (
        "D4",
        "ICC.1:2022 7.3.5",
        Some("pseq"),
        "tag size includes trailing pad bytes",
        "@2.1",
    ),
    // profileSequenceIdentifierType structures are not 4-byte aligned.
    (
        "D5",
        "ICC.1:2022 10.20",
        Some("psid"),
        "identifier 1 at 98+",
        "/flags8",
    ),
    // Saving a v4 profile as v2 keeps v4-only tags ('psid').
    (
        "D6",
        "ICC.1:2022 7.3.1",
        Some("psid"),
        "tag not defined for this profile version",
        "@2.1",
    ),
    (
        "D6",
        "ICC.1:2022 10.20.3",
        Some("psid"),
        "profile description is not mluc",
        "@2.1",
    ),
    // A curves-only pipeline written as lut16Type has a 0-point CLUT.
    (
        "D7",
        "ICC.1:2022 10.10",
        Some("A2B0"),
        "CLUT with 0 grid points",
        "@2.1",
    ),
    // v2 text read into an MLU keeps lcms's 0xFFFF "v2 Unicode" locale,
    // which then reaches mluc records.
    (
        "D8",
        "ICC.1:2022 10.15",
        None,
        "language code [ff, ff]",
        "test1>",
    ),
    (
        "D8",
        "ICC.1:2022 10.15",
        None,
        "country code [ff, ff]",
        "test1>",
    ),
    // cmsMD5computeID also fills the reserved bytes of v2 headers.
    (
        "D9",
        "ICC.1:2001-04 6.1",
        None,
        "reserved header bytes 84..127",
        "+ID",
    ),
    // A profile ID is recorded only on request ("should" in 7.2.18).
    (
        "D10",
        "ICC.1:2022 7.2.18",
        None,
        "no profile ID recorded",
        "",
    ),
    // dictType strings and display elements are written unpadded, so later
    // offsets are not multiples of 4 (WriteOneWChar / WriteOneMLUC).
    (
        "D11",
        "ICC.1:2022 10.9",
        Some("meta"),
        "is not a multiple of 4",
        "kitchen-sink",
    ),
    (
        "D11",
        "ICC.1:2022 10.9",
        Some("meta"),
        "pad bytes are not zero",
        "kitchen-sink",
    ),
    // dictType display name/value elements are bare mluc bodies, without
    // the 'mluc' type signature and reserved field.
    (
        "D12",
        "ICC.1:2022 10.9",
        Some("meta"),
        "display name is not mluc",
        "kitchen-sink",
    ),
    (
        "D12",
        "ICC.1:2022 10.9",
        Some("meta"),
        "display value is not mluc",
        "kitchen-sink",
    ),
];

fn dump(case: &Case) {
    let Ok(dir) = std::env::var("LCMS_CONFORMANCE_DUMP") else {
        return;
    };
    std::fs::create_dir_all(&dir).unwrap();
    let base: String = case
        .name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if let Some(b) = &case.port {
        std::fs::write(format!("{dir}/{base}.port.icc"), b).unwrap();
    }
    if let Some(b) = &case.c {
        std::fs::write(format!("{dir}/{base}.c.icc"), b).unwrap();
    }
}

fn known(case: &str, f: &Finding) -> bool {
    KNOWN_LCMS2_DEVIATIONS.iter().any(|(_, cl, tag, msg, cp)| {
        f.clause == *cl
            && (tag.is_none() || f.tag.as_deref() == *tag)
            && f.msg.contains(msg)
            && case.contains(cp)
    })
}

fn run(cases: Vec<Case>) {
    let checked = cases
        .iter()
        .filter(|c| c.port.is_some() && c.c.is_some())
        .count();
    assert!(checked > 0, "no case produced profiles on both sides");
    let mut port_bugs = Vec::new();
    let mut unlisted = BTreeMap::<String, Vec<String>>::new();
    for case in &cases {
        dump(case);
        match (&case.port, &case.c) {
            // Both sides refuse: parity on the error path. Only the cases
            // in BOTH_FAIL may do this, so a new refusal is not silent.
            (None, None) => {
                if !BOTH_FAIL.contains(&case.name.as_str()) {
                    port_bugs.push(format!(
                        "{}: neither side saved it (not in BOTH_FAIL)",
                        case.name
                    ));
                }
                continue;
            }
            (Some(_), None) | (None, Some(_)) => {
                port_bugs.push(format!(
                    "{}: port {} / C {}",
                    case.name,
                    if case.port.is_some() {
                        "saved"
                    } else {
                        "failed"
                    },
                    if case.c.is_some() { "saved" } else { "failed" }
                ));
                continue;
            }
            (Some(pb), Some(cb)) => {
                let base = case.source.as_deref().map(check).unwrap_or_default();
                let (fp, fc): (Vec<Finding>, Vec<Finding>) = (
                    check(pb)
                        .into_iter()
                        .filter(|f| !base.contains(f))
                        .collect(),
                    check(cb)
                        .into_iter()
                        .filter(|f| !base.contains(f))
                        .collect(),
                );
                for f in fp.iter().filter(|f| !fc.contains(f)) {
                    port_bugs.push(format!("{}: port only: {f}", case.name));
                }
                for f in fc.iter().filter(|f| !fp.contains(f)) {
                    port_bugs.push(format!("{}: C only: {f}", case.name));
                }
                for f in fp
                    .iter()
                    .filter(|f| fc.contains(f) && !known(&case.name, f))
                {
                    unlisted
                        .entry(f.to_string())
                        .or_default()
                        .push(case.name.clone());
                }
            }
        }
    }
    let mut msg = String::new();
    for b in &port_bugs {
        msg += &format!("PORT DIVERGES  {b}\n");
    }
    for (f, cases) in &unlisted {
        msg += &format!("UNLISTED lcms2 {f}\n    in: {}\n", cases.join(", "));
    }
    assert!(msg.is_empty(), "\n{msg}");
}

#[test]
fn generated_profiles_conform() {
    run(generated_cases());
}

#[test]
fn every_written_tag_type_conforms() {
    run(kitchen_sink_cases());
}

#[test]
fn testbed_profiles_reencode_conformantly() {
    run(testbed_cases());
}

// -----------------------------------------------------------------------
// Values printed in the specifications
// -----------------------------------------------------------------------

/// ICC.1:2022 4.6 to 4.9, Tables 4, 6 and 7.
#[test]
fn number_encodings_match_tables_4_to_7() {
    use lcms::fixed::{
        double_to_s15_fixed16, double_to_u8_fixed8, s15_fixed16_to_double, u8_fixed8_to_double,
    };
    // Table 4: s15Fixed16Number.
    for (v, enc) in [
        (-32768.0, 0x8000_0000u32),
        (0.0, 0),
        (1.0, 0x0001_0000),
        (32767.0 + 65535.0 / 65536.0, 0x7FFF_FFFF),
    ] {
        assert_eq!(double_to_s15_fixed16(v) as u32, enc, "s15Fixed16 {v}");
        assert_eq!(s15_fixed16_to_double(enc as i32), v);
    }
    // Table 7: u8Fixed8Number.
    for (v, enc) in [
        (0.0, 0x0000u16),
        (1.0, 0x0100),
        (255.0 + 255.0 / 256.0, 0xFFFF),
    ] {
        assert_eq!(double_to_u8_fixed8(v), enc, "u8Fixed8 {v}");
        assert_eq!(u8_fixed8_to_double(enc), v);
    }
    // Table 6 (u1Fixed15Number) is the 16-bit PCSXYZ encoding: see below.
}

/// ICC.1:2022 6.3.4, Tables 11 to 16, and the legacy encoding of 10.10
/// (Tables 42, 43).
#[test]
fn pcs_encodings_match_tables_11_to_16_42_43() {
    use lcms::pcs::{
        float2lab_encoded, float2lab_encoded_v2, float2xyz_encoded, lab_encoded2float,
        lab_encoded2float_v2, xyz_encoded2float,
    };
    let lab = |l, a, b| CieLab { L: l, a, b };
    let xyz = |x, y, z| CieXyz { X: x, Y: y, Z: z };
    // Table 11 / Table 6: u1Fixed15.
    assert_eq!(
        float2xyz_encoded(&xyz(0.0, 1.0, 1.0 + 32767.0 / 32768.0)),
        [0x0000, 0x8000, 0xFFFF]
    );
    assert_eq!(
        xyz_encoded2float([0x0000, 0x8000, 0xFFFF]).Z,
        1.0 + 32767.0 / 32768.0
    );
    // Tables 12, 13: v4 16-bit PCSLAB.
    assert_eq!(
        float2lab_encoded(&lab(0.0, -128.0, -128.0)),
        [0x0000, 0x0000, 0x0000]
    );
    assert_eq!(
        float2lab_encoded(&lab(100.0, 0.0, 0.0)),
        [0xFFFF, 0x8080, 0x8080]
    );
    assert_eq!(
        float2lab_encoded(&lab(100.0, 127.0, 127.0)),
        [0xFFFF, 0xFFFF, 0xFFFF]
    );
    let l = lab_encoded2float([0xFFFF, 0x8080, 0xFFFF]);
    assert_eq!((l.L, l.a, l.b), (100.0, 0.0, 127.0));
    // Table 14: PCS white point; Table 15: perfect absorber.
    assert_eq!(
        float2xyz_encoded(&xyz(0.9642, 1.0, 0.8249)),
        [0x7B6B, 0x8000, 0x6996]
    );
    assert_eq!(float2xyz_encoded(&xyz(0.0, 0.0, 0.0)), [0, 0, 0]);
    // Table 16: perceptual reference medium black point.
    assert_eq!(
        float2lab_encoded(&lab(3.1373, 0.0, 0.0)),
        [0x0808, 0x8080, 0x8080]
    );
    assert_eq!(
        float2xyz_encoded(&xyz(0.003357, 0.003479, 0.002869)),
        [0x006E, 0x0072, 0x005E]
    );
    // Tables 42, 43: the legacy 16-bit PCSLAB of lut16Type / namedColor2Type.
    assert_eq!(
        float2lab_encoded_v2(&lab(0.0, -128.0, -128.0)),
        [0x0000, 0x0000, 0x0000]
    );
    assert_eq!(
        float2lab_encoded_v2(&lab(100.0, 0.0, 127.0)),
        [0xFF00, 0x8000, 0xFF00]
    );
    let l = lab_encoded2float_v2([0xFFFF, 0xFFFF, 0x8000]);
    assert_eq!(
        (l.L, l.a, l.b),
        (100.0 + 25500.0 / 65280.0, 127.0 + 255.0 / 256.0, 0.0)
    );
    // 6.3.4.1: integer encodings clip component-wise.
    assert_eq!(
        float2lab_encoded(&lab(120.0, -200.0, 300.0)),
        [0xFFFF, 0x0000, 0xFFFF]
    );
    assert_eq!(
        float2xyz_encoded(&xyz(-0.5, 2.5, 1.0)),
        [0x0000, 0xFFFF, 0x8000]
    );
}

/// ICC.1:2022 10.18, Table 68: the five parametric curves. The formulas
/// match exactly. The table's last rule, "any function value outside the
/// range shall be clipped", holds on the 16-bit path but not on the float
/// path: lcms2 evaluates parametric curves unbounded in float on purpose
/// (`docs/CONFORMANCE.md`, D13). The port matches C either way.
#[test]
fn parametric_curves_match_table_68() {
    // ICC function type -> lcms type is +1.
    let cases: [(u16, &[f64]); 5] = [
        (0, &[2.2]),
        (1, &[2.4, 1.2, -0.1]),
        (2, &[2.4, 1.2, -0.1, 0.05]),
        (3, &[2.4, 1.0 / 1.055, 0.055 / 1.055, 1.0 / 12.92, 0.04045]),
        (
            4,
            &[
                2.4,
                1.0 / 1.055,
                0.055 / 1.055,
                1.0 / 12.92,
                0.04045,
                0.02,
                0.01,
            ],
        ),
    ];
    let formula = |ft: u16, p: &[f64], x: f64| -> f64 {
        match ft {
            0 => x.powf(p[0]),
            1 => {
                if x >= -p[2] / p[1] {
                    (p[1] * x + p[2]).powf(p[0])
                } else {
                    0.0
                }
            }
            2 => {
                if x >= -p[2] / p[1] {
                    (p[1] * x + p[2]).powf(p[0]) + p[3]
                } else {
                    p[3]
                }
            }
            3 => {
                if x >= p[4] {
                    (p[1] * x + p[2]).powf(p[0])
                } else {
                    p[3] * x
                }
            }
            4 => {
                if x >= p[4] {
                    (p[1] * x + p[2]).powf(p[0]) + p[5]
                } else {
                    p[3] * x + p[6]
                }
            }
            _ => unreachable!(),
        }
    };
    let mut unclipped = 0;
    for (ft, p) in cases {
        let port = ToneCurve::parametric(ft as i32 + 1, p).unwrap();
        let cc =
            unsafe { c::cmsBuildParametricToneCurve(ptr::null_mut(), ft as i32 + 1, p.as_ptr()) };
        assert!(!cc.is_null());
        for i in 0..=1000 {
            let x = i as f64 / 1000.0;
            let f = formula(ft, p, x);
            let got = port.eval_float(x as f32);
            assert_eq!(
                got.to_bits(),
                unsafe { c::cmsEvalToneCurveFloat(cc, x as f32) }.to_bits(),
                "type {ft} at {x}: port vs C"
            );
            assert!(
                (got as f64 - f).abs() < 2e-6,
                "type {ft} at {x}: {got} vs Table 68 formula {f}"
            );
            if !(0.0..=1.0).contains(&f) {
                unclipped += 1;
            }
            // 16-bit evaluation clips, as the table requires. It interpolates
            // a 4096-entry table, so allow a few codes between nodes.
            let x16 = (x * 65535.0).round() as u16;
            let want16 =
                (formula(ft, p, x16 as f64 / 65535.0).clamp(0.0, 1.0) * 65535.0).round() as i32;
            let got16 = port.eval16(x16) as i32;
            assert!(
                (got16 - want16).abs() <= 16,
                "type {ft} at {x16}: eval16 {got16} vs clipped {want16}"
            );
        }
        unsafe { c::cmsFreeToneCurve(cc) };
    }
    assert!(unclipped > 0, "the fixtures exercise the clipping rule");
}

/// ICC.1:2022 10.16.2.2, Table 60: formula curve segments of the
/// multiProcessElementsType are lcms types 6, 7 and 8.
#[test]
fn segment_formulas_match_table_60() {
    use lcms::gamma::{CurveSegment, build_segmented_tone_curve};
    let cases: [(i32, &[f64]); 3] = [
        (6, &[2.2, 0.9, 0.05, 0.01]),
        (7, &[0.5, 0.3, 2.0, 1.2, 0.1]),
        (8, &[0.4, 1.8, 0.9, 0.1, 0.02]),
    ];
    let spec = |ty: i32, p: &[f64], x: f64| -> f64 {
        match ty {
            6 => (p[1] * x + p[2]).powf(p[0]) + p[3],
            7 => p[1] * (p[2] * x.powf(p[0]) + p[3]).log10() + p[4],
            8 => p[0] * p[1].powf(p[2] * x + p[3]) + p[4],
            _ => unreachable!(),
        }
    };
    for (ty, p) in cases {
        let mut seg = CurveSegment {
            x0: -1e22,
            x1: 1e22,
            r#type: ty,
            ..Default::default()
        };
        seg.params[..p.len()].copy_from_slice(p);
        let c = build_segmented_tone_curve(None, &[seg]).unwrap();
        for i in 1..=100 {
            let x = i as f64 / 100.0;
            let got = c.eval_float(x as f32) as f64;
            let want = spec(ty, p, x);
            assert!(
                (got - want).abs() < 1e-5 * want.abs().max(1.0),
                "type {ty} at {x}: {got} vs Table 60 {want}"
            );
        }
    }
}

/// ICC.1:2022 Annex E, Equations E.1 to E.4: the linear Bradford
/// adaptation from D65 to the PCS white.
#[test]
fn chromatic_adaptation_is_linear_bradford_e1() {
    use lcms::types::Mat3;
    let m = [
        [0.8951, 0.2664, -0.1614],
        [-0.7502, 1.7135, 0.0367],
        [0.0389, -0.0685, 1.0296],
    ];
    let mul = |a: &[[f64; 3]; 3], v: [f64; 3]| -> [f64; 3] {
        [0, 1, 2].map(|r| a[r][0] * v[0] + a[r][1] * v[1] + a[r][2] * v[2])
    };
    let inv = |a: &[[f64; 3]; 3]| -> [[f64; 3]; 3] {
        let det = a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1])
            - a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0])
            + a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0]);
        let c = |r: usize, k: usize| {
            let (r1, r2) = ((r + 1) % 3, (r + 2) % 3);
            let (k1, k2) = ((k + 1) % 3, (k + 2) % 3);
            a[r1][k1] * a[r2][k2] - a[r1][k2] * a[r2][k1]
        };
        [0, 1, 2].map(|i| [0, 1, 2].map(|j| c(j, i) / det))
    };
    let src = CieXyz {
        X: 0.95047,
        Y: 1.0,
        Z: 1.08883,
    };
    let pcs = CieXyz {
        X: 0.9642,
        Y: 1.0,
        Z: 0.8249,
    };
    let rs = mul(&m, [src.X, src.Y, src.Z]);
    let rp = mul(&m, [pcs.X, pcs.Y, pcs.Z]);
    let mi = inv(&m);
    let mut want = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            want[i][j] = (0..3).map(|k| mi[i][k] * (rp[k] / rs[k]) * m[k][j]).sum();
        }
    }
    let mut got = Mat3::default();
    assert!(lcms::wtpnt::adaptation_matrix(&mut got, None, &src, &pcs));
    for i in 0..3 {
        for j in 0..3 {
            assert!(
                (got.v[i].n[j] - want[i][j]).abs() < 1e-12,
                "[{i}][{j}] {} vs E.2 {}",
                got.v[i].n[j],
                want[i][j]
            );
        }
    }
}

/// ICC.1:2022 7.2.18: the profile ID is the RFC 1321 MD5 of the profile
/// with the flags, rendering intent and ID fields zeroed. Checked with an
/// independent MD5 (the `md-5` crate).
#[test]
fn profile_id_is_rfc1321_md5_7_2_18() {
    use md5::{Digest, Md5};
    let mut icc = virt::create_srgb_profile(None).unwrap();
    icc.flags = 3;
    icc.rendering_intent = 2;
    assert!(lcms::md5::compute_id(&mut icc));
    let bytes = profile::save_profile_to_mem(&mut icc).unwrap();
    let mut z = bytes.clone();
    z[44..48].fill(0);
    z[64..68].fill(0);
    z[84..100].fill(0);
    assert_eq!(&bytes[84..100], Md5::digest(&z).as_slice());
    assert_eq!(
        &bytes[44..48],
        &[0, 0, 0, 3],
        "flags survive the ID computation"
    );
    assert_eq!(
        &bytes[64..68],
        &[0, 0, 0, 2],
        "intent survives the ID computation"
    );
}

/// Sharma, Wu, Dalal (2005), Table I: CIEDE2000 test data, 34 pairs, to
/// the four decimals given. The formula is symmetric (their note 4).
#[test]
fn ciede2000_matches_sharma_table_1() {
    /// (pair, L1, a1, b1, L2, a2, b2, expected dE00)
    type Row = (u32, f64, f64, f64, f64, f64, f64, f64);
    #[rustfmt::skip]
    const ROWS: &[Row] = &[
    // (pair, L1, a1, b1, L2, a2, b2, dE00) — Sharma, Wu, Dalal (2005),
    // Color Res. Appl. 30(1):21-30, Table I. Generated from the paper text.
    (1, 50.0000, 2.6772, -79.7751, 50.0000, 0.0000, -82.7485, 2.0425),
    (2, 50.0000, 3.1571, -77.2803, 50.0000, 0.0000, -82.7485, 2.8615),
    (3, 50.0000, 2.8361, -74.0200, 50.0000, 0.0000, -82.7485, 3.4412),
    (4, 50.0000, -1.3802, -84.2814, 50.0000, 0.0000, -82.7485, 1.0000),
    (5, 50.0000, -1.1848, -84.8006, 50.0000, 0.0000, -82.7485, 1.0000),
    (6, 50.0000, -0.9009, -85.5211, 50.0000, 0.0000, -82.7485, 1.0000),
    (7, 50.0000, 0.0000, 0.0000, 50.0000, -1.0000, 2.0000, 2.3669),
    (8, 50.0000, -1.0000, 2.0000, 50.0000, 0.0000, 0.0000, 2.3669),
    (9, 50.0000, 2.4900, -0.0010, 50.0000, -2.4900, 0.0009, 7.1792),
    (10, 50.0000, 2.4900, -0.0010, 50.0000, -2.4900, 0.0010, 7.1792),
    (11, 50.0000, 2.4900, -0.0010, 50.0000, -2.4900, 0.0011, 7.2195),
    (12, 50.0000, 2.4900, -0.0010, 50.0000, -2.4900, 0.0012, 7.2195),
    (13, 50.0000, -0.0010, 2.4900, 50.0000, 0.0009, -2.4900, 4.8045),
    (14, 50.0000, -0.0010, 2.4900, 50.0000, 0.0010, -2.4900, 4.8045),
    (15, 50.0000, -0.0010, 2.4900, 50.0000, 0.0011, -2.4900, 4.7461),
    (16, 50.0000, 2.5000, 0.0000, 50.0000, 0.0000, -2.5000, 4.3065),
    (17, 50.0000, 2.5000, 0.0000, 73.0000, 25.0000, -18.0000, 27.1492),
    (18, 50.0000, 2.5000, 0.0000, 61.0000, -5.0000, 29.0000, 22.8977),
    (19, 50.0000, 2.5000, 0.0000, 56.0000, -27.0000, -3.0000, 31.9030),
    (20, 50.0000, 2.5000, 0.0000, 58.0000, 24.0000, 15.0000, 19.4535),
    (21, 50.0000, 2.5000, 0.0000, 50.0000, 3.1736, 0.5854, 1.0000),
    (22, 50.0000, 2.5000, 0.0000, 50.0000, 3.2972, 0.0000, 1.0000),
    (23, 50.0000, 2.5000, 0.0000, 50.0000, 1.8634, 0.5757, 1.0000),
    (24, 50.0000, 2.5000, 0.0000, 50.0000, 3.2592, 0.3350, 1.0000),
    (25, 60.2574, -34.0099, 36.2677, 60.4626, -34.1751, 39.4387, 1.2644),
    (26, 63.0109, -31.0961, -5.8663, 62.8187, -29.7946, -4.0864, 1.2630),
    (27, 61.2901, 3.7196, -5.3901, 61.4292, 2.2480, -4.9620, 1.8731),
    (28, 35.0831, -44.1164, 3.7933, 35.0232, -40.0716, 1.5901, 1.8645),
    (29, 22.7233, 20.0904, -46.6940, 23.0331, 14.9730, -42.5619, 2.0373),
    (30, 36.4612, 47.8580, 18.3852, 36.2715, 50.5065, 21.2231, 1.4146),
    (31, 90.8027, -2.0831, 1.4410, 91.1528, -1.6435, 0.0447, 1.4441),
    (32, 90.9257, -0.5406, -0.9208, 88.6381, -0.8985, -0.7239, 1.5381),
    (33, 6.7747, -0.2908, -2.4247, 5.8714, -0.0985, -2.2286, 0.6377),
    (34, 2.0776, 0.0795, -1.1350, 0.9033, -0.0636, -0.5514, 0.9082),
    ];
    let mut bad = Vec::new();
    for &(n, l1, a1, b1, l2, a2, b2, want) in ROWS {
        let (x, y) = (
            CieLab {
                L: l1,
                a: a1,
                b: b1,
            },
            CieLab {
                L: l2,
                a: a2,
                b: b2,
            },
        );
        let got = lcms::pcs::cie2000_delta_e(&x, &y, 1.0, 1.0, 1.0);
        let back = lcms::pcs::cie2000_delta_e(&y, &x, 1.0, 1.0, 1.0);
        if (got - want).abs() > 0.00005 || (back - want).abs() > 0.00005 {
            bad.push(format!(
                "pair {n}: {got:.6} / reversed {back:.6}, table {want:.4}"
            ));
        }
    }
    assert!(bad.is_empty(), "CIEDE2000 off Table I:\n{}", bad.join("\n"));
}

/// ICC.1:2022 10.15: "If the specific record for the desired region is not
/// stored in the tag, the record with the same language code should be
/// used. If the specific record for the desired language is not stored in
/// the tag, the first record in the tag is used".
#[test]
fn mluc_lookup_falls_back_as_10_15_says() {
    use lcms::named::{mlu_alloc, mlu_get_translation, mlu_set_ascii};
    let mut m = mlu_alloc(None, 4).unwrap();
    assert!(mlu_set_ascii(&mut m, b"de", b"DE", b"Farbe"));
    assert!(mlu_set_ascii(&mut m, b"en", b"US", b"color"));
    assert!(mlu_set_ascii(&mut m, b"en", b"GB", b"colour"));
    let got = |l: &[u8], c: &[u8]| {
        mlu_get_translation(&m, l, c).map(|(l, c)| (l[..2].to_vec(), c[..2].to_vec()))
    };
    assert_eq!(
        got(b"en", b"GB"),
        Some((b"en".to_vec(), b"GB".to_vec())),
        "exact record"
    );
    assert_eq!(
        got(b"en", b"CA").map(|v| v.0),
        Some(b"en".to_vec()),
        "same language"
    );
    assert_eq!(
        got(b"fr", b"FR"),
        Some((b"de".to_vec(), b"DE".to_vec())),
        "first record"
    );
}

/// ICC.1:2022 Annex F.1 a) (normative): a flat stretch not at the end of
/// the domain inverts to its highest x; one that reaches the end, to its
/// lowest x. lcms2's `cmsReverseToneCurve` lands inside the stretch
/// instead (`docs/CONFORMANCE.md`, D14). The port matches C bit for
/// bit; the spec gap is pinned so a change shows up here.
#[test]
fn curve_inversion_against_annex_f1() {
    let n = 4096usize;
    let table = |f: &dyn Fn(f64) -> f64| -> Vec<u16> {
        (0..n)
            .map(|i| (f(i as f64 / (n - 1) as f64) * 65535.0).round() as u16)
            .collect()
    };
    // 0..0.3 rises to 0.5, 0.3..0.6 is flat at 0.5, 0.6..1 rises to 1.
    let mid = table(&|x| {
        if x < 0.3 {
            x / 0.3 * 0.5
        } else if x <= 0.6 {
            0.5
        } else {
            0.5 + (x - 0.6) / 0.4 * 0.5
        }
    });
    // 0..0.5 rises to 0.8, then flat at 0.8 to the end.
    let end = table(&|x| if x < 0.5 { x / 0.5 * 0.8 } else { 0.8 });
    for (name, t, y, spec_x) in [
        ("flat mid", &mid, 0.5f32, 0.6f64),
        ("flat end", &end, 0.8, 0.5),
    ] {
        let port = ToneCurve::tabulated16(t)
            .unwrap()
            .inverse()
            .unwrap()
            .eval_float(y);
        let cv = unsafe {
            let fwd = c::cmsBuildTabulatedToneCurve16(ptr::null_mut(), n as u32, t.as_ptr());
            let inv = c::cmsReverseToneCurve(fwd);
            let v = c::cmsEvalToneCurveFloat(inv, y);
            c::cmsFreeToneCurve(inv);
            c::cmsFreeToneCurve(fwd);
            v
        };
        assert_eq!(
            port.to_bits(),
            cv.to_bits(),
            "{name}: port {port} vs C {cv}"
        );
        eprintln!("{name}: inverse({y}) = {port}; Annex F.1 a) gives {spec_x}");
        // lcms2 gives 0.459 (inside the stretch) and 1.0 (the highest x)
        // where F.1 a) gives 0.6 and 0.5.
        assert!(
            (port as f64 - spec_x).abs() > 0.05,
            "{name}: lcms2 now follows F.1 a); update CONFORMANCE.md D14"
        );
    }
}

/// ICC.1:2022 7.2.8 and 4.2: the creation date is the current UTC time,
/// stamped like C's `_cmsGetTime`.
#[test]
fn creation_date_is_utc_now_like_c() {
    let port = Profile::placeholder().creation_date();
    let cdt = unsafe {
        let h = CProf(c::cmsCreateProfilePlaceholder(ptr::null_mut()));
        let mut date = c::HeaderCreationDateTime::default();
        assert_ne!(c::cref_get_header_creation_datetime(h.0, &mut date), 0);
        date
    };
    let secs = |y: i32, mo: i32, d: i32, h: i32, mi: i32, s: i32| -> i64 {
        // days from civil (inverse of the conversion under test)
        let y = if mo <= 2 { y - 1 } else { y } as i64;
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let mp = (mo as i64 + 9) % 12;
        let doy = (153 * mp + 2) / 5 + d as i64 - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        (era * 146097 + doe - 719468) * 86400 + h as i64 * 3600 + mi as i64 * 60 + s as i64
    };
    let p = secs(
        port.year as i32,
        port.month as i32,
        port.day as i32,
        port.hours as i32,
        port.minutes as i32,
        port.seconds as i32,
    );
    let q = secs(
        cdt.year,
        cdt.month,
        cdt.day,
        cdt.hours,
        cdt.minutes,
        cdt.seconds,
    );
    assert!((p - q).abs() <= 2, "port {port:?} vs C {cdt:?}");
}

fn with_tag(base: &[u8], tag: &[u8; 4], data: &[u8]) -> Vec<u8> {
    // Rebuild `base` with the given tag replaced or appended.
    let n = u32::from_be_bytes(base[128..132].try_into().unwrap()) as usize;
    let mut tags: Vec<([u8; 4], Vec<u8>)> = (0..n)
        .map(|i| {
            let o = 132 + 12 * i;
            let s: [u8; 4] = base[o..o + 4].try_into().unwrap();
            let off = u32::from_be_bytes(base[o + 4..o + 8].try_into().unwrap()) as usize;
            let sz = u32::from_be_bytes(base[o + 8..o + 12].try_into().unwrap()) as usize;
            (s, base[off..off + sz].to_vec())
        })
        .collect();
    if let Some((_, payload)) = tags.iter_mut().find(|(signature, _)| signature == tag) {
        *payload = data.to_vec();
    } else {
        tags.push((*tag, data.to_vec()));
    }
    let mut out = base[..128].to_vec();
    let mut table = (tags.len() as u32).to_be_bytes().to_vec();
    let mut body = Vec::new();
    let mut off = 128 + 4 + 12 * tags.len();
    for (s, d) in &tags {
        table.extend_from_slice(s);
        table.extend_from_slice(&(off as u32).to_be_bytes());
        table.extend_from_slice(&(d.len() as u32).to_be_bytes());
        body.extend_from_slice(d);
        while body.len() % 4 != 0 {
            body.push(0);
        }
        off = 128 + 4 + 12 * tags.len() + body.len();
    }
    out.extend_from_slice(&table);
    out.extend_from_slice(&body);
    let len = out.len() as u32;
    out[0..4].copy_from_slice(&len.to_be_bytes());
    out
}

/// The cicp amendment approved 2026-10-19 lets a cicpType carry eight
/// s15Fixed16 chromaticities when ColourPrimaries is 2 (44 bytes instead of
/// 12). Pin what the port and lcms2.20 do with one.
#[test]
fn cicp_with_2026_chromaticity_extension() {
    let base = Profile::srgb().save().unwrap();
    let mut tag = b"cicp\0\0\0\0".to_vec();
    tag.extend_from_slice(&[2, 13, 0, 1]);
    for v in [0.64f64, 0.33, 0.30, 0.60, 0.15, 0.06, 0.3127, 0.3290] {
        tag.extend_from_slice(&((v * 65536.0).round() as i32).to_be_bytes());
    }
    let bytes = with_tag(&base, b"cicp", &tag);
    assert!(
        check(&bytes)
            .iter()
            .all(|f| f.tag.as_deref() != Some("cicp")),
        "fixture is valid per the amendment"
    );
    let mut icc = profile::open_profile_from_mem(None, &bytes).unwrap();
    let port_read = profile::cms_read_tag(&mut icc, sig(b"cicp")).is_some();
    let c_read = unsafe {
        let h = CProf(c::cmsOpenProfileFromMem(
            bytes.as_ptr() as *const c_void,
            bytes.len() as u32,
        ));
        !c::cmsReadTag(h.0, sig(b"cicp")).is_null()
    };
    assert_eq!(port_read, c_read, "port and lcms2.20 agree");
    // lcms2.20 predates the amendment and requires exactly 12 bytes.
    assert!(
        !c_read,
        "lcms2.20 rejects the 44-byte cicp; if this changes, update CONFORMANCE.md"
    );
}

/// Tags lcms2.20 has no handler for, like the 2026 amendment's 'HAGC', must
/// survive an open/save round trip unchanged (they are copied as raw
/// bytes), through the parity layer and through `lcms::api`.
#[test]
fn unknown_tags_survive_round_trips() {
    let base = Profile::srgb().save().unwrap();
    let mut tag = b"hagc\0\0\0\0".to_vec();
    tag.extend_from_slice(&[0, 0, 0, 5, 0x24, 0x80, 0x20, 0x00, 0x01]);
    let bytes = with_tag(&base, b"HAGC", &tag);
    let find = |p: &[u8]| -> Option<Vec<u8>> {
        let n = u32::from_be_bytes(p[128..132].try_into().unwrap()) as usize;
        (0..n).find_map(|i| {
            let o = 132 + 12 * i;
            (&p[o..o + 4] == b"HAGC").then(|| {
                let off = u32::from_be_bytes(p[o + 4..o + 8].try_into().unwrap()) as usize;
                let sz = u32::from_be_bytes(p[o + 8..o + 12].try_into().unwrap()) as usize;
                p[off..off + sz].to_vec()
            })
        })
    };
    // Parity layer, lazy like C.
    let mut icc = profile::open_profile_from_mem(None, &bytes).unwrap();
    let saved = profile::save_profile_to_mem(&mut icc).unwrap();
    assert_eq!(find(&saved).as_deref(), Some(&tag[..]), "parity layer");
    // C oracle.
    let c_saved = unsafe {
        c_save(
            c::cmsOpenProfileFromMem(bytes.as_ptr() as *const c_void, bytes.len() as u32),
            None,
            false,
        )
    }
    .unwrap();
    assert_eq!(find(&c_saved).as_deref(), Some(&tag[..]), "lcms2.20");
    // lcms::api materializes tags eagerly; the unknown one must still survive.
    let api_saved = Profile::open(&bytes).unwrap().save().unwrap();
    assert_eq!(find(&api_saved).as_deref(), Some(&tag[..]), "lcms::api");
}

/// The built-in sRGB profile against the ICC colour encoding registry entry
/// for sRGB (registry.color.org/rgb-registry/srgb, archived in
/// standards-ark): D65 white (0.3127, 0.3290), primaries (0.64, 0.33),
/// (0.30, 0.60), (0.15, 0.06), and the piecewise 12.92 / 1.055 / 2.4
/// transfer function. Colorants are derived here from those numbers with
/// the Annex E Bradford transform, independently of lcms's math.
#[test]
fn srgb_profile_matches_icc_registry() {
    use lcms::tagtypes::TagObject;
    let mut icc = virt::create_srgb_profile(None).unwrap();
    let xyz_of = |icc: &mut IccProfile, t: &[u8; 4]| match profile::cms_read_tag(icc, sig(t)) {
        Some(TagObject::Xyz(v)) => [v.X, v.Y, v.Z],
        _ => panic!("{}", String::from_utf8_lossy(t)),
    };
    // RGB -> XYZ(D65) from the chromaticities.
    let prim = [(0.64, 0.33), (0.30, 0.60), (0.15, 0.06)];
    let (wx, wy) = (0.3127, 0.3290);
    let col = |(x, y): (f64, f64)| [x / y, 1.0, (1.0 - x - y) / y];
    let p = prim.map(col);
    let w = col((wx, wy));
    // Solve P * s = W for the channel scales (Cramer).
    let det3 = |m: [[f64; 3]; 3]| {
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    };
    let pm = [
        [p[0][0], p[1][0], p[2][0]],
        [p[0][1], p[1][1], p[2][1]],
        [p[0][2], p[1][2], p[2][2]],
    ];
    let d = det3(pm);
    let s: Vec<f64> = (0..3)
        .map(|k| {
            let mut m = pm;
            for (r, wr) in w.iter().enumerate() {
                m[r][k] = *wr;
            }
            det3(m) / d
        })
        .collect();
    // Bradford D65 -> D50 (Annex E).
    let b = [
        [0.8951, 0.2664, -0.1614],
        [-0.7502, 1.7135, 0.0367],
        [0.0389, -0.0685, 1.0296],
    ];
    let mv = |m: &[[f64; 3]; 3], v: [f64; 3]| {
        [0, 1, 2].map(|r| m[r][0] * v[0] + m[r][1] * v[1] + m[r][2] * v[2])
    };
    let d50 = [0.9642, 1.0, 0.8249];
    let (cs, cp) = (mv(&b, w), mv(&b, d50));
    let binv = {
        let det = det3(b);
        let c = |r: usize, k: usize| {
            let (r1, r2, k1, k2) = ((r + 1) % 3, (r + 2) % 3, (k + 1) % 3, (k + 2) % 3);
            b[r1][k1] * b[r2][k2] - b[r1][k2] * b[r2][k1]
        };
        [0, 1, 2].map(|i| [0, 1, 2].map(|j| c(j, i) / det))
    };
    let adapt = |v: [f64; 3]| {
        let cone = mv(&b, v);
        mv(&binv, [0, 1, 2].map(|k| cone[k] * cp[k] / cs[k]))
    };
    for (k, t) in [b"rXYZ", b"gXYZ", b"bXYZ"].iter().enumerate() {
        let want = adapt(p[k].map(|v| v * s[k]));
        let got = xyz_of(&mut icc, t);
        for i in 0..3 {
            assert!(
                (got[i] - want[i]).abs() < 2.0 / 65536.0,
                "{} [{i}] {} vs registry-derived {}",
                String::from_utf8_lossy(*t),
                got[i],
                want[i]
            );
        }
    }
    let wt = xyz_of(&mut icc, b"wtpt");
    assert!(
        wt.iter()
            .zip(d50)
            .all(|(a, b)| (a - b).abs() < 1.0 / 65536.0),
        "v4 display wtpt is D50: {wt:?}"
    );
    // The TRCs decode the registry's transfer function.
    let Some(TagObject::Curve(trc)) = profile::cms_read_tag(&mut icc, sig(b"rTRC")) else {
        panic!("rTRC")
    };
    let trc = trc.clone();
    for i in 0..=255 {
        let v = i as f64 / 255.0;
        let want = if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        };
        let got = trc.eval_float(v as f32) as f64;
        assert!(
            (got - want).abs() < 1e-6,
            "rTRC({v}) = {got}, registry {want}"
        );
    }
}

/// The `lcms2` shim writes colorant tables from a `NamedColorList`, as the
/// lcms2 crate does (C uses one list type for ncl2, clrt and clot).
#[cfg(feature = "lcms2-api")]
#[test]
fn lcms2_shim_writes_colorant_tables() {
    use lcms::lcms2 as l2;
    let mut p = l2::Profile::new_placeholder();
    let mut list = l2::NamedColorList::new(3, 0, "", "").unwrap();
    assert!(list.append("red", [0x8000, 0xD000, 0xB000], [0; 16]));
    assert!(list.append("green", [0xC000, 0x3000, 0xC000], [0; 16]));
    assert!(p.write_tag(
        l2::TagSignature::ColorantTableTag,
        l2::Tag::NamedColorList(&list)
    ));
    let q = l2::Profile::new_icc(&p.icc().unwrap()).unwrap();
    match q.read_tag(l2::TagSignature::ColorantTableTag) {
        l2::Tag::NamedColorList(l) => assert_eq!(l.count(), 2),
        _ => panic!("colorant table did not round-trip"),
    }
}

fn spec_factory() -> api::ProfileFactory {
    Profile::with_mode(api::BehaviorMode::SpecExact)
}

fn check_spec_profile(name: &str, p: &Profile) -> Vec<u8> {
    let bytes = p.save().unwrap_or_else(|e| panic!("{name}: {e}"));
    let findings = check(&bytes);
    assert!(findings.is_empty(), "{name}: {findings:#?}",);
    if p.version() >> 24 == 4 {
        use md5::{Digest, Md5};
        let mut input = bytes.clone();
        for r in [44..48, 64..68, 84..100] {
            input[r].fill(0);
        }
        assert_eq!(&bytes[84..100], &Md5::digest(&input)[..], "{name}");
    } else {
        assert_eq!(&bytes[84..100], &[0; 16], "{name}");
    }
    let reopened = spec_factory()
        .open(&bytes)
        .unwrap_or_else(|e| panic!("{name} reopen: {e}"));
    assert_eq!(reopened.behavior_mode(), api::BehaviorMode::SpecExact);
    let again = reopened
        .save()
        .unwrap_or_else(|e| panic!("{name} resave: {e}"));
    assert!(
        bytes == again,
        "{name} re-encode changed {} bytes to {}, first difference at {:?}",
        bytes.len(),
        again.len(),
        bytes.iter().zip(&again).position(|(a, b)| a != b)
    );
    if let Some(dir) = std::env::var_os("LCMS_SPEC_EXACT_DUMP") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{name}.icc")), &bytes).unwrap();
    }
    bytes
}

#[test]
fn spec_exact_generators_and_writable_types() {
    use api::{ColorSpace, ErrorKind, Tag, TagData};
    let factory = spec_factory();
    let g = ToneCurve::gamma(2.2).unwrap();
    let d50 = d50();
    let d65 = api::D65;
    let p3 = CiexyYTriple {
        Red: xyy(0.680, 0.320),
        Green: xyy(0.265, 0.690),
        Blue: xyy(0.150, 0.060),
    };
    let generators = [
        ("sRGB", factory.srgb()),
        ("Lab4", factory.lab4()),
        ("Lab4-D65", factory.lab4_with_white_point(&d65).unwrap()),
        ("Lab2", factory.lab2()),
        ("XYZ", factory.xyz()),
        ("Gray-D50", factory.gray_curve(&d50, &g).unwrap()),
        ("Gray-D65", factory.gray_curve(&d65, &g).unwrap()),
        ("RGB-P3-D65", factory.rgb(&d65, &p3, [&g; 3]).unwrap()),
        ("RGB-P3-D50", factory.rgb(&d50, &p3, [&g; 3]).unwrap()),
        (
            "LinLink-RGB",
            factory
                .linearization_device_link(ColorSpace::Rgb, &[g.clone(), g.clone(), g.clone()])
                .unwrap(),
        ),
        (
            "LinLink-CMYK",
            factory
                .linearization_device_link(ColorSpace::Cmyk, &[g.clone(), g.clone(), g.clone(), g])
                .unwrap(),
        ),
        (
            "InkLimit-CMYK",
            factory
                .ink_limiting_device_link(ColorSpace::Cmyk, 250.0)
                .unwrap(),
        ),
        (
            "BCHSW",
            factory
                .bchsw_abstract(17, 10.0, 1.2, 5.0, 1.1, 5000, 6500)
                .unwrap(),
        ),
    ];
    for (name, mut p) in generators {
        check_spec_profile(name, &p);
        p.set_version(0x0210_0000).unwrap();
        if p.has_tag(Tag::from(sig(b"psid"))) {
            let e = p.save().unwrap_err();
            assert_eq!(e.kind(), ErrorKind::NotSuitable);
            assert!(e.message().contains("psid"), "{name}: {e}");
            assert!(p.remove_tag(Tag::from(sig(b"psid"))));
        }
        check_spec_profile(&format!("{name}-v2"), &p);
    }
    for (name, p) in [("NULL", factory.null()), ("OkLab", factory.oklab())] {
        for version in [0x0210_0000, 0x0440_0000] {
            let mut p = p.clone();
            p.set_version(version).unwrap();
            assert_eq!(
                p.save().unwrap_err().kind(),
                ErrorKind::NotSuitable,
                "{name}"
            );
        }
    }
    for (name, p) in kitchen_sink_profiles(api::BehaviorMode::SpecExact) {
        check_spec_profile(&name, &p);
    }
    let mut v2 = factory.lab2();
    assert!(!lcms::md5::compute_id(v2.icc_mut()));
    assert_eq!(v2.profile_id().id8, [0; 16]);
    let mut v4 = factory.srgb();
    let bytes = v4.save().unwrap();
    assert!(lcms::md5::compute_id(v4.icc_mut()));
    assert_eq!(&v4.profile_id().id8, &bytes[84..100]);
    let bad = ToneCurve::tabulated16(&[0, 50000, 10000, u16::MAX]).unwrap();
    v4.write_tag(Tag::RED_TRC, TagData::Curve(bad)).unwrap();
    assert!(Plan::new(&factory.lab4(), &v4, Format::LAB_FLT, Format::RGB_FLT).is_err());
}

#[test]
fn spec_exact_device_links_and_mode_isolation() {
    use api::{BehaviorMode, ErrorKind, ProfileFactory, Tag, flags};
    let factory: ProfileFactory = spec_factory();
    let rgb = factory.srgb();
    let lab = factory.lab4();
    let gray = factory
        .gray_curve(&api::D65, &ToneCurve::gamma(2.2).unwrap())
        .unwrap();
    let cmyk = factory.open(&testbed("test1.icc")).unwrap();
    let pairs = [
        ("CMYK-sRGB", &cmyk, &rgb, Format::CMYK_8, Format::RGB_8),
        ("sRGB-Lab4", &rgb, &lab, Format::RGB_8, Format::LAB_16),
        ("sRGB-CMYK", &rgb, &cmyk, Format::RGB_8, Format::CMYK_8),
        ("sRGB-Gray", &rgb, &gray, Format::RGB_8, Format::GRAY_8),
    ];
    for (name, src, dst, fi, fo) in pairs {
        for version in [2.1, 4.3] {
            for flags in [
                0,
                flags::KEEP_SEQUENCE,
                flags::KEEP_SEQUENCE | flags::DEVICELINK_8BITS,
            ] {
                let id = format!("{name}-{version}-{flags:x}");
                let plan = Plan::build(src, dst, fi, fo, Intent::Perceptual, flags)
                    .unwrap_or_else(|e| panic!("{id}: {e}"));
                assert_eq!(plan.behavior_mode(), BehaviorMode::SpecExact);
                let link = plan
                    .to_device_link_flags(version, flags)
                    .unwrap_or_else(|e| panic!("{id}: {e}"));
                assert_eq!(link.behavior_mode(), BehaviorMode::SpecExact);
                let seq = link.get(Tag::from(sig(b"pseq"))).unwrap().as_seq().unwrap();
                assert_eq!(seq.len(), 2);
                check_spec_profile(&id, &link);
            }
        }
    }
    let ordinary = Profile::srgb();
    let parity = Profile::with_mode(BehaviorMode::Lcms2Exact).srgb();
    assert_eq!(ordinary.behavior_mode(), BehaviorMode::Lcms2Exact);
    assert_eq!(ordinary.version(), parity.version());
    let e = Plan::new(&ordinary, &rgb, Format::RGB_8, Format::RGB_8).unwrap_err();
    assert_eq!(e.kind(), ErrorKind::NotSuitable);
    let same = Plan::new(&ordinary, &parity, Format::RGB_8, Format::RGB_8).unwrap();
    assert_eq!(same.behavior_mode(), BehaviorMode::Lcms2Exact);

    let proof = Plan::new_proofing(
        &rgb,
        &rgb,
        &rgb,
        Format::RGB_8,
        Format::RGB_8,
        Intent::RelativeColorimetric,
        Intent::RelativeColorimetric,
        flags::SOFT_PROOFING | flags::GAMUT_CHECK | flags::BLACK_POINT_COMPENSATION,
    )
    .unwrap();
    assert_eq!(proof.behavior_mode(), BehaviorMode::SpecExact);
    let mut out = [[0u8; 3]];
    proof.transform_pixels(&[[0u8, 121, 255]], &mut out);
}

#[test]
fn spec_exact_table_68_and_annex_f1() {
    use api::{BehaviorMode, ColorSpace};
    let bound = |curve: ToneCurve| curve.with_mode(BehaviorMode::SpecExact).unwrap();
    let g = ToneCurve::parametric(4, &[1.0, 2.0, 0.0, -1.0, 0.5]).unwrap();
    assert_eq!(g.eval_float(0.75), 1.5);
    assert_eq!(g.eval_float(0.25), -0.25);
    let exact = bound(g.clone());
    assert_eq!(exact.eval_float(0.75), 1.0);
    assert_eq!(exact.eval_float(0.25), 0.0);
    let offset = bound(ToneCurve::parametric(3, &[1.0, 1.0, -0.25, 0.125]).unwrap());
    assert_eq!(offset.eval_float(0.25), 0.125);
    let factory = spec_factory();
    let link = factory
        .linearization_device_link(ColorSpace::Rgb, &[g.clone(), g.clone(), g])
        .unwrap();
    for profile in [link.clone(), factory.open(&link.save().unwrap()).unwrap()] {
        let plan = Plan::new_multiprofile(
            &[&profile],
            Format::RGB_FLT,
            Format::RGB_FLT,
            Intent::Perceptual,
            0,
        )
        .unwrap();
        let mut out = [[0.0f32; 3]];
        plan.transform_pixels(&[[0.75f32, 0.25, 0.5]], &mut out);
        assert_eq!(out, [[1.0, 0.0, 1.0]]);
    }

    let size = 4096;
    let table: Vec<u16> = (0..size)
        .map(|i| {
            let x = i as f64 / f64::from(size - 1);
            let v = if x < 0.3 {
                x / 0.3 * 0.5
            } else if x <= 0.6 {
                0.5
            } else {
                0.5 + (x - 0.6) / 0.4 * 0.5
            };
            (v * 65535.0).round() as u16
        })
        .collect();
    let lo = table.iter().position(|&v| v == 32768).unwrap() as f32 / (size - 1) as f32;
    let hi = table.iter().rposition(|&v| v == 32768).unwrap() as f32 / (size - 1) as f32;
    let dense = bound(ToneCurve::tabulated16(&table).unwrap())
        .inverse()
        .unwrap();
    let stored = 32768.0f32 / 65535.0;
    assert_eq!(dense.eval_float(stored), hi);
    assert!(dense.eval_float(0.5) < lo);
    assert!(dense.eval_float(f32::from_bits(stored.to_bits() - 1)) <= lo);
    assert!(dense.eval_float(f32::from_bits(stored.to_bits() + 1)) >= hi);

    let y = 32768.0f32 / 65535.0;
    for (samples, ascending) in [(17, true), (4096, true), (257, false)] {
        let table = if ascending {
            [0, 32768, 32768, 32768, 65535]
        } else {
            [65535, 32768, 32768, 32768, 0]
        };
        let inverse = bound(ToneCurve::tabulated16(&table).unwrap())
            .inverse_sampled(samples)
            .unwrap();
        assert_eq!(inverse.eval_float(y), 0.75);
        assert_eq!(inverse.eval16(32768), 49151);
        let below = inverse.eval_float(f32::from_bits(y.to_bits() - 1));
        let above = inverse.eval_float(f32::from_bits(y.to_bits() + 1));
        if ascending {
            assert!(below <= 0.25 && above >= 0.75);
        } else {
            assert!(above <= 0.25 && below >= 0.75);
        }
    }
    for (table, at) in [
        ([0, 30000, 52428, 52428, 52428], 0.8),
        ([0, 30000, 65535, 65535, 65535], 1.0),
        ([65535, 30000, 0, 0, 0], 0.0),
    ] {
        let inverse = bound(ToneCurve::tabulated16(&table).unwrap())
            .inverse()
            .unwrap();
        assert_eq!(inverse.eval_float(at), 0.5);
    }
    let first = bound(ToneCurve::tabulated16(&[20000, 20000, 65535]).unwrap())
        .inverse()
        .unwrap();
    assert_eq!(first.eval_float(0.0), 0.5);
    let constant = bound(ToneCurve::tabulated16(&[20000; 3]).unwrap());
    assert!(constant.inverse().is_err());
    let clip = bound(ToneCurve::parametric(4, &[1.0, 2.0, 0.0, 2.0, 0.5]).unwrap());
    assert_eq!(clip.inverse().unwrap().eval_float(1.0), 0.5);
}

#[test]
fn spec_exact_cicp_text_and_malformed_boundaries() {
    use api::{ErrorKind, Locale, Mlu, Tag, TagValue, VideoSignal};
    let factory = spec_factory();
    let mut p = factory.srgb();
    let signal = VideoSignal {
        colour_primaries: 2,
        transfer_characteristics: 13,
        matrix_coefficients: 0,
        video_full_range_flag: 1,
    };
    let coords = [41943, 21627, 19661, 39322, 9830, -1, 20493, 21561];
    p.write_cicp_with_chromaticities(signal, coords).unwrap();
    let mut desc = Mlu::new();
    desc.set(Locale::new("en_US"), "Display 🐈").unwrap();
    desc.set(Locale::new("de_DE"), "Bildschirm").unwrap();
    p.write_tag(Tag::from(sig(b"desc")), desc).unwrap();
    let bytes = check_spec_profile("extended-cicp", &p);
    let p = factory.open(&bytes).unwrap();
    assert_eq!(p.cicp_chromaticities(), Some(coords));
    assert!(matches!(p.get(Tag::CICP), Some(TagValue::VideoSignal(v)) if *v == signal));
    assert_eq!(
        p.info_in(api::InfoType::Description, Locale::new("en_US"))
            .as_deref(),
        Some("Display 🐈")
    );
    assert_eq!(
        p.info_in(api::InfoType::Description, Locale::new("de_DE"))
            .as_deref(),
        Some("Bildschirm")
    );
    assert!(Profile::open(&bytes).unwrap().get(Tag::CICP).is_none());
    let mut truncated = bytes.clone();
    truncated.pop();
    assert_eq!(
        factory.open(&truncated).unwrap_err().kind(),
        ErrorKind::CorruptionDetected
    );
    let bad_cicp = with_tag(
        &factory.srgb().save().unwrap(),
        b"cicp",
        b"cicp\0\0\0\0\x02\x0d\0\x01\0",
    );
    assert_eq!(
        factory.open(&bad_cicp).unwrap_err().kind(),
        ErrorKind::CorruptionDetected
    );
    let opaque = with_tag(&bytes, b"xUnk", b"zRaw\0\0\0\0test");
    let resaved = factory.open(&opaque).unwrap().save().unwrap();
    let tag = |b: &[u8]| -> Vec<u8> {
        let n = u32::from_be_bytes(b[128..132].try_into().unwrap()) as usize;
        let data = b[132..132 + n * 12]
            .chunks_exact(12)
            .find(|r| &r[..4] == b"xUnk")
            .unwrap();
        let off = u32::from_be_bytes(data[4..8].try_into().unwrap()) as usize;
        let len = u32::from_be_bytes(data[8..].try_into().unwrap()) as usize;
        b[off..off + len].to_vec()
    };
    assert_eq!(tag(&resaved), tag(&opaque));
}

#[test]
fn spec_exact_legacy_translation_and_extended_mpe() {
    use api::{BehaviorMode, ErrorKind, InfoType, Locale, Pipeline, Stage, Tag, TagData};
    let factory = spec_factory();
    let mut desc = b"desc\0\0\0\0".to_vec();
    desc.extend(6u32.to_be_bytes());
    desc.extend(b"ASCII\0");
    desc.extend(0u32.to_be_bytes());
    desc.extend(2u32.to_be_bytes());
    desc.extend(0x03a9u16.to_be_bytes());
    desc.extend([0; 72]);
    let base = Profile::lab2().save().unwrap();
    let bytes = with_tag(&base, b"desc", &desc);
    let mut p = factory.open(&bytes).unwrap();
    assert_eq!(
        p.info_in(InfoType::Description, Locale::new("en_US"))
            .as_deref(),
        Some("ASCII")
    );
    p.set_version(0x0440_0000).unwrap();
    let out = check_spec_profile("v2-description-to-v4", &p);
    let p = factory.open(&out).unwrap();
    assert_eq!(
        p.info_in(InfoType::Description, Locale::NONE).as_deref(),
        Some("Ω")
    );
    assert_eq!(
        p.info_in(InfoType::Description, Locale::new("en_US"))
            .as_deref(),
        Some("ASCII")
    );

    let mut link = factory.srgb();
    let build_pipeline = |curve: &ToneCurve| {
        let mut pipe = Pipeline::new(3, 3).unwrap();
        pipe.push(Stage::tone_curves(&[curve; 3]).unwrap()).unwrap();
        pipe
    };
    let table = ToneCurve::tabulated16(&[0, 65535]).unwrap();
    let d2b0 = Tag::from(sig(b"D2B0"));
    link.write_tag(d2b0, TagData::Pipeline(build_pipeline(&table)))
        .unwrap();
    assert_eq!(link.save().unwrap_err().kind(), ErrorKind::NotSuitable);

    let extended = ToneCurve::parametric(6, &[1.0, 2.0, 0.0, 0.0]).unwrap();
    assert_eq!(extended.eval_float(0.75), 1.5);
    assert_eq!(
        extended
            .with_mode(BehaviorMode::SpecExact)
            .unwrap_err()
            .kind(),
        ErrorKind::NotSuitable
    );
    link.write_tag(d2b0, TagData::Pipeline(build_pipeline(&extended)))
        .unwrap();
    let bytes = check_spec_profile("extended-mpe", &link);
    for p in [link, factory.open(&bytes).unwrap()] {
        let pipe = p.get(d2b0).unwrap().as_pipeline().unwrap();
        let mut out = [0.0f32; 3];
        pipe.eval_float(&[0.75; 3], &mut out);
        assert_eq!(out, [1.5; 3]);
    }
}

fn spec_tag_payload<'a>(profile: &'a [u8], name: &[u8; 4]) -> &'a [u8] {
    let count = u32::from_be_bytes(profile[128..132].try_into().unwrap()) as usize;
    let entry = profile[132..132 + 12 * count]
        .chunks_exact(12)
        .find(|r| &r[..4] == name)
        .unwrap();
    let offset = u32::from_be_bytes(entry[4..8].try_into().unwrap()) as usize;
    let size = u32::from_be_bytes(entry[8..12].try_into().unwrap()) as usize;
    &profile[offset..offset + size]
}

fn spec_matrix_lut(matrix: &[f64; 9], offset: Option<&[f64; 3]>) -> api::Pipeline {
    let mut lut = api::Pipeline::new(3, 3).unwrap();
    lut.push(api::Stage::matrix(matrix, 3, 3, offset.map(|v| v.as_slice())).unwrap())
        .unwrap();
    lut
}

fn spec_constant_mpe(values: [f64; 3]) -> api::Pipeline {
    spec_matrix_lut(&[0.0; 9], Some(&values))
}

fn spec_unsupported_element(payload: &[u8]) -> Vec<u8> {
    let mut payload = payload.to_vec();
    let offset = u32::from_be_bytes(payload[16..20].try_into().unwrap()) as usize;
    payload[offset..offset + 4].copy_from_slice(b"test");
    payload
}

fn spec_with_tag(base: &[u8], tag: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = with_tag(base, tag, data);
    out[84..100].fill(0);
    out
}

fn spec_pixel(plan: &Plan, input: [f32; 3]) -> [f32; 3] {
    let mut output = [[0.0; 3]];
    plan.transform_pixels(&[input], &mut output);
    output[0]
}

fn assert_spec_pixel(got: [f32; 3], want: [f32; 3], tolerance: f32) {
    assert!(
        got.iter()
            .zip(want)
            .all(|(a, b)| (a - b).abs() <= tolerance),
        "got {got:?}, expected {want:?} within {tolerance}"
    );
}

#[test]
fn spec_exact_devicelink_and_abstract_d2b0_for_every_intent() {
    use api::{ColorSpace, Tag, TagData, flags};
    let f = spec_factory();
    let lin = ToneCurve::linear();
    let mut device_link = f
        .linearization_device_link(ColorSpace::Rgb, &[lin.clone(), lin.clone(), lin])
        .unwrap();
    device_link.set_pcs(ColorSpace::Xyz);
    for (name, mut profile, input, output, xyz_or_lab) in [
        (
            "device-link-d2b0",
            device_link,
            Format::RGB_FLT,
            Format::XYZ_FLT,
            [0.2, 0.3, 0.4],
        ),
        (
            "abstract-d2b0",
            f.lab4(),
            Format::LAB_FLT,
            Format::LAB_FLT,
            [40.0, -10.0, 15.0],
        ),
    ] {
        let control = f.open(&profile.save().unwrap()).unwrap();
        profile
            .write_tag(
                Tag::new(*b"D2B0"),
                TagData::Pipeline(spec_constant_mpe(xyz_or_lab)),
            )
            .unwrap();
        let bytes = check_spec_profile(name, &profile);
        let selected = f.open(&bytes).unwrap();
        let unsupported = spec_unsupported_element(spec_tag_payload(&bytes, b"D2B0"));
        let fallback = f.open(&spec_with_tag(&bytes, b"D2B0", &unsupported));
        let intents = [
            Intent::Perceptual,
            Intent::RelativeColorimetric,
            Intent::Saturation,
            Intent::AbsoluteColorimetric,
        ];
        let pixel = |p: &Profile, intent| {
            let plan =
                Plan::new_multiprofile(&[p], input, output, intent, flags::NO_OPTIMIZE).unwrap();
            spec_pixel(&plan, [0.2, 0.3, 0.4])
        };
        for intent in intents {
            assert_spec_pixel(pixel(&selected, intent), xyz_or_lab.map(|v| v as f32), 2e-5);
        }
        let fallback = fallback.unwrap();
        for intent in intents {
            assert_eq!(pixel(&fallback, intent), pixel(&control, intent));
        }
    }
}

#[test]
fn spec_exact_direct_absolute_luts_preserve_their_pcs() {
    use api::{ProfileClass, Tag, TagData, flags};
    let f = spec_factory();
    let mut src = f.srgb();
    src.set_device_class(ProfileClass::Input);
    src.write_tag(
        Tag::MEDIA_WHITE_POINT,
        CieXyz {
            X: 0.4821,
            Y: 0.5,
            Z: 0.41245,
        },
    )
    .unwrap();
    let src_relative = f.open(&src.save().unwrap()).unwrap();
    src.write_tag(
        Tag::new(*b"D2B3"),
        TagData::Pipeline(spec_constant_mpe([0.2, 0.3, 0.4])),
    )
    .unwrap();
    let src_bytes = check_spec_profile("direct-d2b3", &src);
    let src_direct = f.open(&src_bytes).unwrap();
    let unsupported = spec_unsupported_element(spec_tag_payload(&src_bytes, b"D2B3"));
    let src_fallback = f.open(&spec_with_tag(&src_bytes, b"D2B3", &unsupported));

    // A colour-space profile with an identity B2A0 in normalized PCS and
    // an identity B2D3 in floating-point XYZ. Give it an unequal white.
    let mut dst = f.srgb();
    dst.set_device_class(ProfileClass::ColorSpace);
    dst.write_tag(
        Tag::MEDIA_WHITE_POINT,
        CieXyz {
            X: 0.72315,
            Y: 0.75,
            Z: 0.618675,
        },
    )
    .unwrap();
    let lin = ToneCurve::linear();
    let mut curves = api::Pipeline::new(3, 3).unwrap();
    curves
        .push(api::Stage::tone_curves(&[&lin; 3]).unwrap())
        .unwrap();
    for tag in [*b"A2B0", *b"B2A0"] {
        dst.write_tag(Tag::new(tag), TagData::Pipeline(curves.clone()))
            .unwrap();
    }
    let dst_relative = f.open(&dst.save().unwrap()).unwrap();
    let identity = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
    dst.write_tag(
        Tag::new(*b"B2D3"),
        TagData::Pipeline(spec_matrix_lut(&identity, None)),
    )
    .unwrap();
    let dst_bytes = check_spec_profile("direct-b2d3", &dst);
    let dst_direct = f.open(&dst_bytes).unwrap();
    let unsupported = spec_unsupported_element(spec_tag_payload(&dst_bytes, b"B2D3"));
    let dst_fallback = f.open(&spec_with_tag(&dst_bytes, b"B2D3", &unsupported));

    let input = [0.08, 0.17, 0.26];
    let xyz = f.xyz();
    let run = |a: &Profile, b: &Profile, fmt, intent| {
        spec_pixel(
            &Plan::build(a, b, Format::RGB_FLT, fmt, intent, flags::NO_OPTIMIZE).unwrap(),
            input,
        )
    };
    let relative = run(
        &src_relative,
        &xyz,
        Format::XYZ_FLT,
        Intent::RelativeColorimetric,
    );
    let absolute = Intent::AbsoluteColorimetric;
    let rgb = Format::RGB_FLT;
    let normalize = 65535.0 / 32768.0;
    let direct = [0.2_f32, 0.3, 0.4];
    assert_spec_pixel(
        run(&src_direct, &xyz, Format::XYZ_FLT, absolute),
        direct,
        1e-6,
    );
    assert_spec_pixel(run(&src_direct, &dst_direct, rgb, absolute), direct, 1e-6);
    assert_spec_pixel(
        run(&src_direct, &dst_relative, rgb, absolute),
        direct.map(|v| v / (0.75 * normalize)),
        1e-5,
    );
    assert_spec_pixel(
        run(&src_relative, &dst_direct, rgb, absolute),
        relative.map(|v| v * 0.5),
        1e-5,
    );
    assert_spec_pixel(
        run(&src_relative, &dst_relative, rgb, absolute),
        relative.map(|v| v * 0.5 / (0.75 * normalize)),
        1e-5,
    );
    assert_eq!(
        run(&src_fallback.unwrap(), &dst_direct, rgb, absolute),
        run(&src_relative, &dst_direct, rgb, absolute)
    );
    assert_eq!(
        run(&src_direct, &dst_fallback.unwrap(), rgb, absolute),
        run(&src_direct, &dst_relative, rgb, absolute)
    );
}

#[test]
fn spec_exact_version_changes_preserve_legacy_lab_lut_encoding() {
    use api::{ColorSpace, ErrorKind, InfoType, ProfileClass, Tag, TagData, flags};
    let f = spec_factory();
    let mut p = f.placeholder();
    p.set_version(0x0210_0000).unwrap();
    p.set_device_class(ProfileClass::Input);
    p.set_color_space(ColorSpace::Rgb);
    p.set_pcs(ColorSpace::Lab);
    p.set_info(InfoType::Description, "Lab LUT").unwrap();
    p.set_info(InfoType::Copyright, "Test fixture").unwrap();
    p.write_tag(
        Tag::MEDIA_WHITE_POINT,
        CieXyz {
            X: 0.9642,
            Y: 1.0,
            Z: 0.8249,
        },
    )
    .unwrap();
    let half = ToneCurve::tabulated16(&[32768; 2]).unwrap();
    let mut lut = api::Pipeline::new(3, 3).unwrap();
    lut.push(api::Stage::tone_curves(&[&half; 3]).unwrap())
        .unwrap();
    p.write_tag(Tag::new(*b"A2B0"), TagData::Pipeline(lut.clone()))
        .unwrap();

    let lab = f.lab4();
    let pixel = |p: &Profile| {
        spec_pixel(
            &Plan::build(
                p,
                &lab,
                Format::RGB_FLT,
                Format::LAB_FLT,
                Intent::RelativeColorimetric,
                flags::NO_OPTIMIZE,
            )
            .unwrap(),
            [0.3, 0.4, 0.5],
        )
    };
    let original = check_spec_profile("legacy-lab-pcs-v2", &p);
    let mut p = f.open(&original).unwrap();
    let before = pixel(&p);
    assert_spec_pixel(before, [100.0 * 32768.0 / 65280.0, 0.0, 0.0], 2e-5);
    p.set_version(0x0440_0000).unwrap();
    let upgraded = check_spec_profile("legacy-lab-pcs-v4", &p);
    let encoded = spec_tag_payload(&original, b"A2B0");
    assert_eq!(&encoded[..4], b"mft2");
    assert_eq!(spec_tag_payload(&upgraded, b"A2B0"), encoded);
    let mut p = f.open(&upgraded).unwrap();
    assert_eq!(pixel(&p), before);
    p.set_version(0x0210_0000).unwrap();
    let downgraded = p.save().unwrap();
    assert_eq!(spec_tag_payload(&downgraded, b"A2B0"), encoded);
    assert_eq!(pixel(&f.open(&downgraded).unwrap()), before);

    p.set_version(0x0440_0000).unwrap();
    p.write_tag(Tag::new(*b"A2B0"), TagData::Pipeline(lut.clone()))
        .unwrap();
    let modern = p.save().unwrap();
    assert_eq!(&spec_tag_payload(&modern, b"A2B0")[..4], b"mAB ");
    let mut p = f.open(&modern).unwrap();
    p.set_version(0x0210_0000).unwrap();
    assert_eq!(p.save().unwrap_err().kind(), ErrorKind::NotSuitable);

    let half8 = ToneCurve::tabulated16(&[128 * 257; 256]).unwrap();
    let linear = ToneCurve::linear();
    let mut lut8 = api::Pipeline::new(3, 3).unwrap();
    lut8.push(api::Stage::tone_curves(&[&half8; 3]).unwrap())
        .unwrap();
    lut8.push(api::Stage::tone_curves(&[&linear; 3]).unwrap())
        .unwrap();
    lut8.set_save_as_8bits(true);
    p.write_tag(Tag::new(*b"A2B0"), TagData::Pipeline(lut8))
        .unwrap();
    let legacy = check_spec_profile("legacy-lab-pcs-mft1-v2", &p);
    let mut p = f.open(&legacy).unwrap();
    let before = pixel(&p);
    p.set_version(0x0440_0000).unwrap();
    let upgraded = check_spec_profile("legacy-lab-pcs-mft1-v4", &p);
    let encoded = spec_tag_payload(&legacy, b"A2B0");
    assert_eq!(&encoded[..4], b"mft1");
    assert_eq!(spec_tag_payload(&upgraded, b"A2B0"), encoded);
    assert_eq!(pixel(&f.open(&upgraded).unwrap()), before);
}

#[test]
fn spec_exact_unknown_mpe_is_opaque_and_malformed_framing_is_rejected() {
    use api::{Tag, TagData};
    let f = spec_factory();
    let mut p = f.srgb();
    let original = p.save().unwrap();
    p.write_tag(
        Tag::new(*b"D2B0"),
        TagData::Pipeline(spec_constant_mpe([0.2, 0.3, 0.4])),
    )
    .unwrap();
    let control = p.save().unwrap();
    let opaque = spec_unsupported_element(spec_tag_payload(&control, b"D2B0"));
    let bytes = spec_with_tag(&original, b"D2B0", &opaque);
    let p = f.open(&bytes).unwrap();
    assert!(p.has_tag(Tag::new(*b"D2B0")));
    assert!(p.get(Tag::new(*b"D2B0")).is_none());
    assert_eq!(spec_tag_payload(&p.save().unwrap(), b"D2B0"), opaque);
    let xyz = f.xyz();
    let input = [0.2, 0.3, 0.4];
    let pixel = |p: &Profile| {
        spec_pixel(
            &Plan::new(p, &xyz, Format::RGB_FLT, Format::XYZ_FLT).unwrap(),
            input,
        )
    };
    assert_eq!(pixel(&p), pixel(&f.open(&original).unwrap()));
    assert_ne!(pixel(&p), pixel(&f.open(&control).unwrap()));

    for (offset, invalid_value) in [(16, u32::MAX), (20, 11), (28, 1)] {
        let mut bad = opaque.clone();
        bad[offset..offset + 4].copy_from_slice(&invalid_value.to_be_bytes());
        assert!(
            f.open(&spec_with_tag(&original, b"D2B0", &bad)).is_err(),
            "word at {offset}"
        );
    }
}

#[test]
fn spec_exact_export_validates_lut_dimensions_and_encoded_chad() {
    use api::{ColorSpace, ErrorKind, Tag, TagData};
    let f = spec_factory();
    let mut p = f.srgb();
    p.set_color_space(ColorSpace::Cmyk);
    assert_eq!(p.save().unwrap_err().kind(), ErrorKind::NotSuitable);

    for tag in [*b"A2B0", *b"B2A0", *b"D2B0", *b"B2D0"] {
        let mut p = f.srgb();
        let mut one = api::Pipeline::new(1, 1).unwrap();
        one.push(api::Stage::matrix(&[1.0], 1, 1, None).unwrap())
            .unwrap();
        p.write_tag(Tag::new(tag), TagData::Pipeline(one)).unwrap();
        let error = p.save().unwrap_err();
        assert_eq!(error.kind(), ErrorKind::NotSuitable);
        assert!(error.message().contains("channel counts"), "{error}");
    }

    for values in [
        vec![0.0; 9],
        vec![1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0 / 262144.0],
        vec![1.0; 10],
        vec![f64::INFINITY, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
    ] {
        let mut p = f.srgb();
        p.write_tag(Tag::CHROMATIC_ADAPTATION, TagData::S15Fixed16(values))
            .unwrap();
        assert_eq!(p.save().unwrap_err().kind(), ErrorKind::NotSuitable);
    }
    // A determinant below a floating-point inversion tolerance can still
    // be nonzero in the encoded matrix.
    let mut p = f.srgb();
    let unit = 1.0 / 65536.0;
    p.write_tag(
        Tag::CHROMATIC_ADAPTATION,
        TagData::S15Fixed16(vec![unit, 0.0, 0.0, 0.0, unit, 0.0, 0.0, 0.0, unit]),
    )
    .unwrap();
    assert!(p.save().is_ok());
}
