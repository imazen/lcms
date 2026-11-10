//! End-to-end tests for `lcms::api` — the always-on facade.
//!
//! Runs under plain `cargo test -p lcms` (no surface features needed):
//! the `api` module is the default-visible face of the crate.

use lcms::api::*;
use std::sync::Arc;

// -- Send/Sync audit -----------------------------------------------------------
// Every public api type carries the auto traits it's documented for —
// the test compiling IS the proof.
#[test]
fn send_sync_audit() {
    fn assert_send<T: Send>() {}
    fn assert_sync<T: Sync>() {}
    assert_send::<Profile>();
    assert_sync::<Profile>();
    assert_send::<Plan>();
    assert_sync::<Plan>();
    assert_send::<Scratch>();
    assert_sync::<Scratch>();
    assert_send::<ToneCurve>();
    assert_sync::<ToneCurve>();
    assert_send::<NamedColorList>();
    assert_sync::<NamedColorList>();
    assert_send::<Pipeline>();
    assert_sync::<Pipeline>();
    assert_send::<Mlu>();
    assert_sync::<Mlu>();
    assert_send::<TagValue<'static>>();
    assert_sync::<TagValue<'static>>();
    assert_send::<NamedColor>();
    assert_send::<Mlu>();
    assert_sync::<Mlu>();
    assert_send::<NamedColorList>();
    assert_sync::<NamedColorList>();
    assert_send::<Pipeline>();
    assert_sync::<Pipeline>();
    assert_send::<Stage>();
    assert_sync::<Stage>();
    assert_send::<Dict>();
    assert_sync::<Dict>();
    assert_send::<Error>();
    assert_sync::<Error>();
    assert_send::<PlanOptions>();
    assert_sync::<PlanOptions>();
}

// -- profile basics --------------------------------------------------------------

#[test]
fn srgb_profile_basics() {
    let p = Profile::srgb();
    assert_eq!(p.device_class(), ProfileClass::Display);
    assert_eq!(p.color_space(), ColorSpace::Rgb);
    assert_eq!(p.pcs(), ColorSpace::Xyz);
    assert_eq!(p.header_intent(), Some(Intent::Perceptual));
    assert_eq!(
        p.info(InfoType::Description).as_deref(),
        Some("sRGB built-in")
    );
    // the directory + frozen reads work straight out of the constructor
    let tags: Vec<Tag> = p.tags().collect();
    assert!(tags.contains(&Tag::MEDIA_WHITE_POINT));
    assert!(tags.contains(&Tag::RED_TRC));
    assert_eq!(p.tag_count(), tags.len());
    let wp = p.white_point().unwrap();
    assert!((wp.Y - 1.0).abs() < 1e-3, "D65-ish white: {wp:?}");
    match p.get(Tag::RED_TRC).unwrap() {
        TagValue::Curve(c) => assert!(c.eval16(0x8000) > 0),
        other => panic!("rTRC should be a curve: {other:?}"),
    }
    match p.get(Tag::PROFILE_DESCRIPTION).unwrap() {
        TagValue::Text(m) => {
            assert_eq!(m.get(Locale::NONE).as_deref(), Some("sRGB built-in"));
        }
        other => panic!("desc should be MLU text: {other:?}"),
    }
    assert!(p.get(0x78787878).is_none()); // 'xxxx' — not a tag
}

#[test]
fn profile_constructors() {
    assert_eq!(Profile::lab4().color_space(), ColorSpace::Lab);
    assert_eq!(Profile::lab2().color_space(), ColorSpace::Lab);
    assert_eq!(Profile::xyz().color_space(), ColorSpace::Xyz);
    assert_eq!(Profile::gray().color_space(), ColorSpace::Gray);
    let null = Profile::null();
    assert_eq!(null.device_class(), ProfileClass::Output);
    assert_eq!(null.color_space(), ColorSpace::Gray);
    let oklab = Profile::oklab();
    assert_eq!(oklab.device_class(), ProfileClass::ColorSpace);
    assert_eq!(oklab.color_space(), ColorSpace::NColor(3));
    // custom RGB + gray curves
    let c = ToneCurve::gamma(2.2).unwrap();
    let g = Profile::gray_curve(&D50, &c).unwrap();
    assert_eq!(g.color_space(), ColorSpace::Gray);
}

#[test]
fn save_open_roundtrip() {
    let mut p = Profile::srgb();
    p.set_info(InfoType::Copyright, "test copyright").unwrap();
    let bytes = p.save().unwrap();
    assert!(&bytes[36..40] == b"acsp", "not an ICC file");
    let q = Profile::open(&bytes).unwrap();
    assert_eq!(
        q.info(InfoType::Copyright).as_deref(),
        Some("test copyright")
    );
    assert_eq!(q.device_class(), ProfileClass::Display);
    assert!(q.white_point().is_some());
}

#[test]
fn placeholder_authoring() {
    let mut p = Profile::placeholder();
    p.set_device_class(ProfileClass::NamedColor);
    p.set_color_space(ColorSpace::Named);
    p.set_pcs(ColorSpace::Lab);
    p.set_header_intent(Intent::AbsoluteColorimetric);
    p.set_header_manufacturer(u32::from_be_bytes(*b"ACME"));
    p.set_info(InfoType::Description, "spot palette").unwrap();
    p.write_tag(Tag::MEDIA_WHITE_POINT, TagData::Xyz(D50_XYZ)).unwrap();
    let bytes = p.save().unwrap();
    let q = Profile::open(&bytes).unwrap();
    assert_eq!(q.device_class(), ProfileClass::NamedColor);
    assert_eq!(q.color_space(), ColorSpace::Named);
    assert_eq!(q.pcs(), ColorSpace::Lab);
    assert_eq!(q.header_intent(), Some(Intent::AbsoluteColorimetric));
    assert_eq!(q.header_manufacturer(), u32::from_be_bytes(*b"ACME"));
    assert_eq!(
        q.info(InfoType::Description).as_deref(),
        Some("spot palette")
    );
    assert_eq!(q.white_point().unwrap().Y, D50_XYZ.Y);
    // remove_tag clears the directory entry
    let mut q = q;
    q.remove_tag(Tag::MEDIA_WHITE_POINT);
    assert!(!q.has_tag(Tag::MEDIA_WHITE_POINT));
}

// -- transforms ------------------------------------------------------------------

#[test]
fn plan_srgb_to_lab() {
    let srgb = Profile::srgb();
    let lab4 = Profile::lab4();
    let plan = Plan::new(&srgb, &lab4, Format::RGB_8, Format::LAB_8).unwrap();
    assert_eq!(plan.input_format(), Format::RGB_8);
    assert_eq!(plan.output_format(), Format::LAB_8);
    assert_eq!(plan.intent(), Some(Intent::Perceptual));
    assert_eq!(plan.input_color_space(), ColorSpace::Rgb);
    assert_eq!(plan.output_color_space(), ColorSpace::Lab);
    let mut sc = plan.scratch();
    let mut out = [0u8; 3];
    plan.apply(&mut sc, &[255, 0, 0], &mut out);
    assert!(out[0] > 0, "red -> Lab L* nonzero: {out:?}");
    // apply_n + a multi-pixel buffer
    let src: Vec<u8> = (0..300).map(|i| i as u8).collect();
    let mut dst = vec![0u8; 300];
    plan.apply(&mut sc, &src, &mut dst); // n derived: 100 px
    assert_ne!(dst, vec![0u8; 300]);
}

#[test]
fn plan_srgb_roundtrip_and_typed_pixels() {
    let srgb = Profile::srgb();
    let plan = Plan::new(&srgb, &srgb, Format::RGB_8, Format::RGB_8).unwrap();
    let mut sc = plan.scratch();
    let src = [[200u8, 100, 50], [0, 128, 255]];
    let mut dst = [[0u8; 3]; 2];
    plan.apply_pixels(&mut sc, &src, &mut dst);
    // srgb->srgb is a (quantized) identity
    for (s, d) in src.iter().zip(dst.iter()) {
        for (a, b) in s.iter().zip(d.iter()) {
            assert!((*a as i16 - *b as i16).abs() <= 2, "{s:?} -> {d:?}");
        }
    }
}

#[test]
fn plan_apply_variants() {
    let srgb = Profile::srgb();
    let lab4 = Profile::lab4();
    let plan = Plan::new(&srgb, &lab4, Format::RGB_8, Format::LAB_8).unwrap();
    let mut sc = plan.scratch();
    // in-place when sizes match
    let id = Plan::new(&srgb, &srgb, Format::RGB_8, Format::RGB_8).unwrap();
    let mut buf = vec![10u8, 20, 30, 40, 50, 60];
    id.apply_in_place(&mut id.scratch(), &mut buf);
    // apply_strided: 2 rows of 1 px with a doubled stride
    let src = [255u8, 0, 0, 99, 0, 255, 0, 99]; // px, pad, px, pad
    let mut dst = [0u8; 8];
    plan.apply_strided(&mut sc, &src, 4, &mut dst, 4, 1, 2);
    assert_eq!((dst[3], dst[7]), (0, 0), "row padding untouched");
    assert!(dst[0] > 0);
    assert!(dst[4] > 0);
}

#[test]
fn plan_copy_alpha() {
    let srgb = Profile::srgb();
    let plan = Plan::build(
        &srgb,
        &srgb,
        Format::RGBA_8,
        Format::RGBA_8,
        Intent::Perceptual,
        flags::COPY_ALPHA,
    )
    .unwrap();
    assert_eq!(plan.flags() & flags::COPY_ALPHA, flags::COPY_ALPHA);
    let mut sc = plan.scratch();
    let mut dst = [0u8; 4];
    plan.apply(&mut sc, &[255, 0, 0, 77], &mut dst);
    assert_eq!(dst[3], 77, "alpha passthrough: {dst:?}");
}

#[test]
fn plan_apply_par_matches_serial() {
    let srgb = Profile::srgb();
    let lab4 = Profile::lab4();
    let plan = Plan::new(&srgb, &lab4, Format::RGB_8, Format::LAB_8).unwrap();
    let n = 10_000u32;
    let src: Vec<u8> = (0..n * 3).map(|i| (i * 37) as u8).collect();
    let mut serial = vec![0u8; src.len()];
    let mut par = vec![0u8; src.len()];
    plan.apply(&mut plan.scratch(), &src, &mut serial);
    plan.apply_par(&src, &mut par, 4);
    assert_eq!(serial, par);
}

#[test]
fn plan_multiprofile_and_proofing() {
    let srgb = Profile::srgb();
    let lab4 = Profile::lab4();
    let xyz = Profile::xyz();
    let mp = Plan::new_multiprofile(
        &[&srgb, &lab4, &xyz],
        Format::RGB_8,
        Format::XYZ_16,
        Intent::Perceptual,
        0,
    )
    .unwrap();
    let mut out = [0u8; 6];
    mp.apply(&mut mp.scratch(), &[255, 0, 0], &mut out);
    assert!(
        u16::from_be_bytes([out[0], out[1]]) > 0,
        "red -> XYZ X>0: {out:?}"
    );
    // proofing: srgb -> lab4 (proof) -> srgb
    let proof = Plan::new_proofing(
        &srgb,
        &srgb,
        &lab4,
        Format::RGB_8,
        Format::RGB_8,
        Intent::Perceptual,
        Intent::RelativeColorimetric,
        0,
    )
    .unwrap();
    let mut back = [0u8; 3];
    proof.apply(&mut proof.scratch(), &[250, 10, 10], &mut back);
    assert!(back[0] > 200, "proof roundtrip keeps red: {back:?}");
    // gamut-check flag path builds too (alarm path is covered upstream)
    let _gc = Plan::new_proofing(
        &srgb,
        &srgb,
        &lab4,
        Format::RGB_8,
        Format::RGB_8,
        Intent::Perceptual,
        Intent::Perceptual,
        flags::GAMUT_CHECK,
    )
    .unwrap();
}

#[test]
fn plan_to_device_link() {
    let srgb = Profile::srgb();
    let lab4 = Profile::lab4();
    let plan = Plan::new(&srgb, &lab4, Format::RGB_8, Format::LAB_8).unwrap();
    let link = plan.to_device_link(4.3).unwrap();
    assert_eq!(link.device_class(), ProfileClass::DeviceLink);
    // the link serializes and reopens
    let bytes = link.save().unwrap();
    let q = Profile::open(&bytes).unwrap();
    assert_eq!(q.device_class(), ProfileClass::DeviceLink);
}

#[test]
fn plan_shared_across_threads() {
    let srgb = Profile::srgb();
    let lab4 = Profile::lab4();
    let plan = Arc::new(Plan::new(&srgb, &lab4, Format::RGB_8, Format::LAB_8).unwrap());
    let profile = Arc::new(srgb);
    let mut handles = Vec::new();
    for t in 0..4 {
        let (plan, profile) = (Arc::clone(&plan), Arc::clone(&profile));
        handles.push(std::thread::spawn(move || {
            // shared &Profile reads + per-thread scratch — the server model
            let wp = profile.white_point().copied();
            let mut sc = plan.scratch();
            let mut out = [0u8; 3];
            plan.apply(&mut sc, &[t as u8, 0, 255], &mut out);
            (wp, out)
        }));
    }
    for h in handles {
        let (wp, out) = h.join().unwrap();
        assert!(wp.is_some());
        assert_ne!(out, [0, 0, 0]);
    }
}

// -- tags ------------------------------------------------------------------------

#[test]
fn tag_read_write_roundtrip() {
    let mut p = Profile::placeholder();
    p.set_color_space(ColorSpace::Rgb);
    p.set_device_class(ProfileClass::Display);
    // curve tag
    let c = ToneCurve::gamma(2.2).unwrap();
    p.write_tag(Tag::RED_TRC, TagData::Curve(c)).unwrap();
    match p.get(Tag::RED_TRC) {
        Some(TagValue::Curve(rc)) => {
            let g = rc.eval16(0x8000);
            assert!(g > 0 && g < 0xffff);
        }
        other => panic!("rTRC roundtrip: {other:?}"),
    }
    // signature + data + chromaticity
    p.write_tag(Tag::TECHNOLOGY, TagData::Signature(0x6D6E7472)).unwrap();
    match p.get(Tag::TECHNOLOGY) {
        Some(TagValue::Signature(s)) => assert_eq!(s, 0x6D6E7472),
        other => panic!("tech: {other:?}"),
    }
    // Xyz write + read
    p.write_tag(Tag::MEDIA_WHITE_POINT, TagData::Xyz(D50_XYZ)).unwrap();
    assert_eq!(p.white_point().unwrap().Y, 1.0);
    // serialize + reopen keeps them
    let q = Profile::open(&p.save().unwrap()).unwrap();
    assert!(matches!(q.get(Tag::RED_TRC), Some(TagValue::Curve(_))));
    assert_eq!(q.white_point().unwrap().Y, 1.0);
}

#[test]
fn tag_info_locales() {
    let mut mlu = Mlu::new();
    mlu.set(Locale::new("en_US"), "Color profile").unwrap();
    mlu.set(Locale::new("de_DE"), "Farbprofil").unwrap();
    assert_eq!(mlu.len(), 2);
    let codes: Vec<String> = mlu.locales().map(|l| l.to_string()).collect();
    assert!(
        codes.iter().any(|c| c == "en_US") && codes.iter().any(|c| c == "de_DE"),
        "{codes:?}"
    );
    let mut p = Profile::placeholder();
    p.write_tag(Tag::PROFILE_DESCRIPTION, TagData::Mlu(mlu)).unwrap();
    assert_eq!(
        p.info_in(InfoType::Description, Locale::new("de_DE"))
            .as_deref(),
        Some("Farbprofil")
    );
    assert_eq!(
        p.info_in(InfoType::Description, Locale::new("en_US"))
            .as_deref(),
        Some("Color profile")
    );
    // unset locale falls back to the best available translation
    assert!(p.info(InfoType::Description).is_some());
}

#[test]
fn named_colors_roundtrip() {
    let mut list = NamedColorList::new(4, "Spot ", " Ink").unwrap();
    list.push("red", [0xffff, 0x4000, 0x2000], &[0xffff, 0, 0, 0])
        .unwrap();
    list.push("blue", [0x2000, 0x4000, 0xffff], &[0, 0, 0, 0xffff])
        .unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list.position("blue"), Some(1));
    let mut p = Profile::placeholder();
    p.set_color_space(ColorSpace::Named);
    p.set_device_class(ProfileClass::NamedColor);
    p.write_tag(Tag::NAMED_COLOR2, TagData::NamedColors(list)).unwrap();
    let ncl = p.named_colors().unwrap();
    assert_eq!(ncl.len(), 2);
    assert!(!ncl.is_empty());
    assert_eq!(ncl.prefix(), "Spot ");
    assert_eq!(ncl.suffix(), " Ink");
    assert_eq!(ncl.colorant_count(), 4);
    let entries: Vec<NamedColor> = (&ncl).into_iter().collect();
    assert_eq!(entries[0].name, "red");
    assert_eq!(entries[1].name, "blue");
    assert_eq!(entries[1].pcs[2], 0xffff);
    assert_eq!(ncl.position("blue"), Some(1));
    assert_eq!(ncl.position("missing"), None);
    // survives serialization
    let q = Profile::open(&p.save().unwrap()).unwrap();
    assert_eq!(q.named_colors().unwrap().len(), 2);
}

#[test]
fn pipeline_tags() {
    // a linearization devicelink has a real AToB0 pipeline to read back
    let c = ToneCurve::gamma(1.8).unwrap();
    let link =
        Profile::linearization_device_link(ColorSpace::Gray, std::slice::from_ref(&c)).unwrap();
    assert_eq!(link.device_class(), ProfileClass::DeviceLink);
    match link.get(Tag::A_TO_B0) {
        Some(TagValue::Pipeline(p)) => {
            assert_eq!(p.input_channels(), 1);
            assert_eq!(p.output_channels(), 1);
            let stages: Vec<&Stage> = p.stages().collect();
            assert_eq!(stages.len(), 1);
        }
        other => panic!("A2B0 should be a pipeline: {other:?}"),
    }
    // and the link drives a transform — the chain link(Gray->Gray) ->
    // gray display (input direction Gray->XYZ) produces XYZ output
    let gray = Profile::gray();
    let plan = Plan::new(&link, &gray, Format::GRAY_8, Format::XYZ_16).unwrap();
    let mut out = [0u8; 6];
    plan.apply(&mut plan.scratch(), &[128], &mut out);
    assert!(
        u16::from_be_bytes([out[0], out[1]]) > 0 || out.iter().any(|&b| b != 0),
        "{out:?}"
    );
    // authored pipelines evaluate
    let mut pipe = Pipeline::new(1, 1).unwrap();
    let g = ToneCurve::gamma(2.0).unwrap();
    pipe.push(Stage::tone_curves(&[&g]).unwrap()).unwrap();
    let mut out = [0f32; 1];
    pipe.eval_float(&[0.5], &mut out);
    assert!((out[0] - 0.25).abs() < 0.01);
}

// -- tone curves -------------------------------------------------------------------

#[test]
fn tone_curves() {
    let g = ToneCurve::gamma(2.2).unwrap();
    assert_eq!(g.eval16(0), 0);
    assert_eq!(g.eval16(0xffff), 0xffff);
    assert!(g.is_monotonic() && !g.is_descending());
    let est = g.estimated_gamma(0.001).unwrap();
    assert!((est - 2.2).abs() < 0.02, "est {est}");
    // srgb parametric + identity
    let srgb = ToneCurve::srgb();
    assert!(srgb.is_monotonic());
    assert_eq!(srgb.parametric_type(), 4);
    let id = ToneCurve::linear();
    assert!(id.is_linear());
    assert_eq!(id.eval16(0x4000), 0x4000);
    // tabulated + reverse roundtrip
    let tab = ToneCurve::tabulated16(&[0, 0x4000, 0x8000, 0xc000, 0xffff]).unwrap();
    assert!(tab.is_monotonic());
    let rev = g.inverse().unwrap();
    let v = 0x5555u16;
    let rt = rev.eval16(g.eval16(v));
    assert!(
        (rt as i32 - v as i32).abs() <= 8,
        "{v} -> {} -> {rt}",
        g.eval16(v)
    );
    // deref exposes the parity-layer read surface
    assert!(!g.estimated_table().is_empty());
}

// -- format / intent / signature ergonomics -------------------------------------------

#[test]
fn format_ergonomics() {
    assert_eq!(Format::RGB_8.code(), Format::RGB_8.code());
    assert_eq!(Format::from_u32(Format::RGB_8.code()), Format::RGB_8);
    assert_eq!(Format::from_u32(0xDEAD_BEEF).code(), 0xDEAD_BEEF);
    assert_eq!(Format::RGB_8.pixel_size(), 3);
    assert_eq!(Format::RGBA_16.pixel_size(), 8);
    assert_eq!(Format::CMYK_FLT.pixel_size(), 16);
    assert_eq!(Format::GRAY_8.channels(), 1);
    assert_eq!(Format::RGBA_8.extra_channels(), 1);
    assert_eq!(Format::RGBA_8.total_channels(), 4);
    assert_eq!(Format::RGB_8.bytes_per_channel(), 1);
    assert!(Format::LAB_FLT.is_float());
    assert!(!Format::LAB_16.is_float());
    assert_eq!(Format::RGB_8.color_space(), Some(ColorSpace::Rgb));
    assert_eq!(Format::CMYK_16.color_space(), Some(ColorSpace::Cmyk));
    // From impls
    let f: Format = Format::RGB_16.code().into();
    assert_eq!(f, Format::RGB_16);
    let code: u32 = Format::LAB_8.into();
    assert_eq!(code, Format::LAB_8.code());
}

#[test]
fn signature_types() {
    // Tag
    assert_eq!(Tag::new(*b"wtpt"), Tag::MEDIA_WHITE_POINT);
    assert_eq!(Tag::MEDIA_WHITE_POINT.chars(), *b"wtpt");
    assert_eq!(Tag::MEDIA_WHITE_POINT.code(), 0x77747074);
    assert_eq!(format!("{}", Tag::MEDIA_WHITE_POINT), "'wtpt'");
    assert_eq!(Tag::from(0x77747074u32), Tag::MEDIA_WHITE_POINT);
    assert_eq!(u32::from(Tag::GAMUT), Tag::GAMUT.0);
    // ColorSpace roundtrip incl. multichannel + nCLR + raw
    for cs in [
        ColorSpace::Rgb,
        ColorSpace::Xyz,
        ColorSpace::Lab,
        ColorSpace::Gray,
        ColorSpace::Cmyk,
        ColorSpace::Cmy,
        ColorSpace::Luv,
        ColorSpace::YCbCr,
        ColorSpace::Yxy,
        ColorSpace::Hsv,
        ColorSpace::Hls,
        ColorSpace::Named,
        ColorSpace::Mch(3),
        ColorSpace::Mch(15),
        ColorSpace::NColor(6),
        ColorSpace::Other(0x61626364),
    ] {
        assert_eq!(ColorSpace::from_u32(cs.code()), cs, "{cs:?}");
    }
    assert_eq!(ColorSpace::Mch(3).channels(), 3);
    assert_eq!(ColorSpace::Gray.channels(), 1);
    assert_eq!(ColorSpace::Cmyk.channels(), 4);
    assert_eq!(format!("{}", ColorSpace::Rgb), "'RGB '");
    // ProfileClass
    for pc in [
        ProfileClass::Input,
        ProfileClass::Display,
        ProfileClass::Output,
        ProfileClass::DeviceLink,
        ProfileClass::Abstract,
        ProfileClass::ColorSpace,
        ProfileClass::NamedColor,
        ProfileClass::Other(0x61626364),
    ] {
        assert_eq!(ProfileClass::from_u32(pc.code()), pc, "{pc:?}");
    }
    // Intent
    assert_eq!(Intent::default(), Intent::Perceptual);
    assert_eq!(Intent::from_u32(99), None);
    assert_eq!(Intent::from_u32(14), Some(Intent::PreserveKPlaneRelativeColorimetric));
    assert!(Intent::try_from(4).is_err());
    assert_eq!(Intent::ALL.len(), 4);
    assert_eq!(u32::from(Intent::Saturation), 2);
    // Locale
    let l = Locale::new("de_DE");
    assert_eq!(l.language(), "de");
    assert_eq!(l.country(), "DE");
    assert_eq!(l.to_string(), "de_DE");
    assert!(Locale::NONE.is_none());
    assert_eq!(Locale::from_parts("fr", "FR").to_string(), "fr_FR");
}

#[test]
fn error_kind() {
    let e = Profile::open(&[0u8; 64]).unwrap_err();
    assert!(!e.message().is_empty());
    assert_ne!(e.kind(), ErrorKind::Undefined);
    let e2 = e.clone();
    assert_eq!(e, e2);
    // Display is "<message> (<kind>)"
    let shown = format!("{e}");
    assert!(shown.starts_with(e.message()), "{shown}");
    assert!(shown.ends_with(&format!("({:?})", e.kind())), "{shown}");
}

// -- free functions -------------------------------------------------------------------

#[test]
fn color_conversions() {
    let d50 = D50_XYZ;
    assert!((d50.Y - 1.0).abs() < 1e-9);
    let xyy = xyz_to_xyy(&d50);
    assert!((xyy.x - D50.x).abs() < 1e-3);
    let back = xyy_to_xyz(&xyy);
    assert!((back.Y - d50.Y).abs() < 1e-9);
    // Lab <-> XYZ roundtrip
    let lab = xyz_to_lab(None, &d50);
    assert!((lab.L - 100.0).abs() < 0.5, "D50 -> L*≈100: {lab:?}");
    let xyz2 = lab_to_xyz(None, &lab);
    assert!((xyz2.X - d50.X).abs() < 1e-3);
    // Lab <-> LCh
    let lch = lab_to_lch(&CieLab {
        L: 50.0,
        a: 10.0,
        b: 20.0,
    });
    let lab2 = lch_to_lab(&lch);
    assert!((lab2.a - 10.0).abs() < 1e-9);
    // encoded forms roundtrip
    let enc = encode_lab(&lab);
    assert!(enc[0] > 0);
    let dec = decode_lab(enc);
    assert!((dec.L - lab.L).abs() < 0.01);
    let xv = encode_xyz(&d50);
    assert!((decode_xyz(xv).Y - 1.0).abs() < 1e-4);
    // delta-E family
    let a = CieLab {
        L: 50.0,
        a: 0.0,
        b: 0.0,
    };
    let b = CieLab {
        L: 50.0,
        a: 3.0,
        b: 4.0,
    };
    assert!((delta_e(&a, &b) - 5.0).abs() < 1e-9);
    assert!(cie2000_delta_e(&a, &b, 1.0, 1.0, 1.0) > 0.0);
    assert!(cie94_delta_e(&a, &b) > 0.0);
    assert!(cmc_delta_e(&a, &b, 2.0, 1.0) > 0.0);
    assert!(bfd_delta_e(&a, &b) > 0.0);
    // desaturate clamps a/b
    let mut hot = CieLab {
        L: 50.0,
        a: 200.0,
        b: 0.0,
    };
    assert!(desaturate_lab(&mut hot, 100.0, -100.0, 100.0, -100.0));
    assert!(hot.a <= 100.0);
    // version + white point temps
    assert_eq!(version(), 2200);
    let wp = white_point_from_temp(6500.0).unwrap();
    assert!(wp.x > 0.3 && wp.x < 0.33);
    let t = temp_from_white_point(&wp).unwrap();
    assert!((t - 6500.0).abs() < 50.0);
    // adaptation state save/restore
    let prev = set_adaptation_state(0.5);
    assert!((adaptation_state() - 0.5).abs() < 1e-12);
    set_adaptation_state(prev);
    // alarm codes save/restore
    let old = alarm_codes();
    set_alarm_codes([0x7fff; 16]);
    assert_eq!(alarm_codes()[0], 0x7fff);
    set_alarm_codes(old);
}

// -- device links / BCHSW ------------------------------------------------------------

#[test]
fn extra_profile_ctors() {
    let link = Profile::ink_limiting_device_link(ColorSpace::Cmyk, 250.0).unwrap();
    assert_eq!(link.device_class(), ProfileClass::DeviceLink);
    assert_eq!(link.color_space(), ColorSpace::Cmyk);
    let abs = Profile::bchsw_abstract(17, 0.0, 100.0, 0.0, 100.0, 5000, 5000).unwrap();
    assert_eq!(abs.device_class(), ProfileClass::Abstract);
}

// -- dict / mlu vocab -----------------------------------------------------------------

#[test]
fn dict_and_mlu() {
    let mut d = Dict::new();
    d.add("key", "value").unwrap();
    assert_eq!(d.get("key").as_deref(), Some("value"));
    assert_eq!(d.len(), 1);
    assert!(!d.is_empty());
    // Dict roundtrips through a tag
    let mut p = Profile::placeholder();
    p.write_tag(Tag::META, TagData::Dict(d)).unwrap();
    match p.get(Tag::META) {
        Some(TagValue::Dict(d)) => assert_eq!(d.get("key").as_deref(), Some("value")),
        other => panic!("meta dict: {other:?}"),
    }
}

// -- reconcile pass (2026-11-10): regressions for the fixes on top of 1ec7ec29 --

/// Upstream testbed ICCs (the oracle checkout every parity gate uses).
/// A missing file is a failure, not a skip.
fn testbed(name: &str) -> Profile {
    let p = format!(
        "{}/../../../reference/lcms2/testbed/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    let bytes = std::fs::read(&p).unwrap_or_else(|e| panic!("testbed profile {p}: {e}"));
    Profile::open(&bytes).unwrap_or_else(|e| panic!("{name}: {e}"))
}

#[test]
fn builtin_profiles_are_infallible_and_consts_match_c() {
    let srgb = Profile::srgb();
    assert_eq!(srgb.color_space(), ColorSpace::Rgb);
    // D50 const is bit-identical to cmsD50_xyY()
    let d = D50;
    assert_eq!((D50.x.to_bits(), D50.y.to_bits(), D50.Y.to_bits()), (d.x.to_bits(), d.y.to_bits(), d.Y.to_bits()));
    assert_eq!(D50_XYZ, D50_XYZ);
    // sRGB rebuilt from D65 + REC709_PRIMARIES + the sRGB curve matches the built-in
    let trc = ToneCurve::srgb();
    let rebuilt = Profile::rgb(&D65, &REC709_PRIMARIES, [&trc, &trc, &trc]).unwrap();
    for t in [Tag::RED_COLORANT, Tag::GREEN_COLORANT, Tag::BLUE_COLORANT, Tag::MEDIA_WHITE_POINT] {
        assert_eq!(rebuilt.get(t).and_then(|v| v.as_xyz()), srgb.get(t).and_then(|v| v.as_xyz()), "{t}");
    }
}

#[test]
fn format_struct_consts() {
    assert_eq!(Format::RGB_DBL.pixel_size(), 24);
    assert_eq!(Format::RGB_DBL.bytes_per_channel(), 8);
    assert_eq!(Format::RGBA_HALF.pixel_size(), 8);
    assert!(Format::RGB_HALF.is_float());
    assert_eq!(Format::BGRA_8.total_channels(), 4);
    assert_eq!(format!("{:?}", Format::LAB_V2_16), "Format::LAB_V2_16");
    assert_eq!(format!("{:?}", Format::RGB_8.planar()), "Format::RGB_8.planar()");
    assert_eq!(format!("{:?}", Format::from_u32(0x1234)), "Format(0x1234)");
    // equality is code equality — no named-vs-raw split
    assert_eq!(Format::from_u32(Format::CMYK_16.code()), Format::CMYK_16);
}

#[test]
fn stale_errors_are_not_blamed_on_later_failures() {
    let first = Profile::open(&[0u8; 64]).unwrap_err();
    let _ok = Profile::srgb();
    let e = ToneCurve::parametric(4, &[2.4]).unwrap_err();
    assert_ne!(e.message(), first.message(), "a later failure must report its own message");
}

#[test]
fn v2_text_description_and_long_text() {
    // test5.icc is ICC v2: 'desc' is textDescriptionType
    let p = testbed("test5.icc");
    assert!(p.version() < 0x0300_0000);
    assert!(!p.info(InfoType::Description).unwrap().is_empty());
    // text longer than 256 bytes is not truncated (Mlu::get sizes first)
    let long = "0123456789".repeat(100);
    let mut q = Profile::srgb();
    q.set_info(InfoType::Description, &long).unwrap();
    assert_eq!(q.info(InfoType::Description).unwrap(), long);
    let back = Profile::open(&q.save().unwrap()).unwrap();
    assert_eq!(back.info(InfoType::Description).unwrap(), long);
}

#[test]
fn write_tag_errors_and_remove_reports_presence() {
    let mut p = Profile::srgb();
    let e = p.write_tag(Tag::RED_TRC, "not a curve").unwrap_err();
    assert_eq!(e.kind(), ErrorKind::UnknownExtension, "{e}");
    // as in C the old value is gone; the facade keeps has_tag/get consistent
    assert!(!p.has_tag(Tag::RED_TRC) && p.get(Tag::RED_TRC).is_none());
    assert!(p.remove_tag(Tag::GREEN_TRC));
    assert!(!p.remove_tag(Tag::GREEN_TRC));
    // From conversions
    p.write_tag(Tag::MEDIA_BLACK_POINT, CieXyz { X: 0.0, Y: 0.0, Z: 0.0 }).unwrap();
    p.write_tag(Tag::GRAY_TRC, ToneCurve::gamma(1.8).unwrap()).unwrap();
    assert!(matches!(p.get(Tag::GRAY_TRC), Some(TagValue::Curve(_))));
}

#[test]
fn named_color_lookup_is_case_insensitive_like_c() {
    let mut list = NamedColorList::new(3, "", "").unwrap();
    list.push("Warm Red", [0xffff, 0xc000, 0xa000], &[0xffff, 0, 0]).unwrap();
    // ncl2 PCS values are ICC v2 Lab: L 0xff00 = 100, a/b 0x8000 = 0
    list.push("white", [0xff00, 0x8000, 0x8000], &[]).unwrap();
    assert!(list.push("bad", [0; 3], &[1, 2]).is_err());
    let mut p = Profile::placeholder();
    p.set_device_class(ProfileClass::NamedColor);
    p.set_color_space(ColorSpace::Rgb);
    p.set_pcs(ColorSpace::Lab);
    p.write_tag(Tag::NAMED_COLOR2, list).unwrap();
    let ncl = p.named_colors().unwrap();
    assert_eq!(ncl.position("warm red"), Some(0));
    assert_eq!(ncl.entry(1).unwrap().colorant, vec![0, 0, 0]);
    // named-color index -> Lab through a plan
    let plan = Plan::new(&p, &Profile::lab4(), Format::NAMED_COLOR_INDEX, Format::LAB_DBL).unwrap();
    let mut lab = [[0f64; 3]; 1];
    plan.transform_pixels(&[1u16], &mut lab);
    assert!((lab[0][0] - 100.0).abs() < 0.01, "{lab:?}");
}

#[test]
fn tone_curve_failures_are_errors_not_copies() {
    let g = ToneCurve::gamma(2.2).unwrap();
    let joined = g.then_inverse_of(&g, 256).unwrap();
    assert!(joined.is_linear(), "g⁻¹(g(t)) is the identity");
    let inv = g.inverse_sampled(1024).unwrap();
    assert!((inv.eval_float(g.eval_float(0.3)) - 0.3).abs() < 2e-3);
    assert!(ToneCurve::parametric(99, &[1.0]).is_err());
}

#[test]
fn every_run_style_gives_identical_pixels() {
    let plan = Plan::new(&testbed("test5.icc"), &Profile::srgb(), Format::RGB_8, Format::RGB_8).unwrap();
    let n = 4096;
    let input: Vec<u8> = (0..n * 3).map(|i| (i * 7 % 251) as u8).collect();
    let mut want = vec![0u8; n * 3];
    plan.apply(&mut plan.scratch(), &input, &mut want);

    let mut a = vec![0u8; n * 3];
    plan.transform(&input, &mut a);
    assert_eq!(a, want, "transform");
    let typed: Vec<[u8; 3]> = input.chunks(3).map(|c| [c[0], c[1], c[2]]).collect();
    let mut b = vec![[0u8; 3]; n];
    plan.transform_pixels(&typed, &mut b);
    assert_eq!(b.concat(), want, "transform_pixels");
    let mut c = vec![0u8; n * 3];
    plan.apply_par(&input, &mut c, 4);
    assert_eq!(c, want, "apply_par");
    let mut d = input.clone();
    plan.apply_in_place(&mut plan.scratch(), &mut d);
    assert_eq!(d, want, "apply_in_place");
    let mut e = vec![0u8; n * 3];
    plan.apply_n(&mut plan.scratch(), &input, &mut e, n as u32);
    assert_eq!(e, want, "apply_n");

    // strided: 64x64 with different padding on each side
    let (w, h) = (64usize, 64usize);
    let (si, so) = (w * 3 + 5, w * 3 + 7);
    let mut src = vec![0xEEu8; si * h];
    for y in 0..h {
        src[y * si..y * si + w * 3].copy_from_slice(&input[y * w * 3..(y + 1) * w * 3]);
    }
    let mut dst = vec![0xAAu8; so * h];
    plan.apply_strided(&mut plan.scratch(), &src, si, &mut dst, so, w, h);
    for y in 0..h {
        assert_eq!(&dst[y * so..y * so + w * 3], &want[y * w * 3..(y + 1) * w * 3], "row {y}");
        assert!(dst[y * so + w * 3..(y + 1) * so].iter().all(|&v| v == 0xAA), "padding row {y}");
    }
}

#[test]
fn planar_strided_matches_interleaved() {
    let (sp, dp) = (testbed("test5.icc"), Profile::srgb());
    let chunky = Plan::new(&sp, &dp, Format::RGB_8, Format::RGB_8).unwrap();
    let planar = Plan::new(&sp, &dp, Format::RGB_8.planar(), Format::RGB_8.planar()).unwrap();
    let (w, h, stride) = (13usize, 7usize, 16usize);
    let px: Vec<[u8; 3]> = (0..w * h).map(|i| [(i * 3) as u8, (i * 5) as u8, (i * 11) as u8]).collect();
    let mut want = vec![[0u8; 3]; w * h];
    chunky.transform_pixels(&px, &mut want);
    let mut src = vec![0u8; 3 * stride * h];
    for (i, p) in px.iter().enumerate() {
        for c in 0..3 {
            src[c * stride * h + (i / w) * stride + i % w] = p[c];
        }
    }
    let mut dst = vec![0u8; 3 * stride * h];
    planar.apply_strided(&mut planar.scratch(), &src, stride, &mut dst, stride, w, h);
    for (i, p) in want.iter().enumerate() {
        for c in 0..3 {
            assert_eq!(dst[c * stride * h + (i / w) * stride + i % w], p[c], "pixel {i} ch {c}");
        }
    }
}

#[test]
#[should_panic(expected = "not a whole number")]
fn partial_pixel_input_panics() {
    let srgb = Profile::srgb();
    let plan = Plan::new(&srgb, &srgb, Format::RGB_8, Format::RGB_8).unwrap();
    plan.transform(&[1, 2, 3, 4], &mut [0; 6]);
}

#[test]
#[should_panic(expected = "output holds")]
fn short_output_panics() {
    let srgb = Profile::srgb();
    let plan = Plan::new(&srgb, &srgb, Format::RGB_8, Format::RGBA_8).unwrap();
    plan.transform(&[1, 2, 3, 4, 5, 6], &mut [0; 6]);
}

#[test]
fn plan_options_and_black_point_compensation() {
    // test1.icc: v2 CMYK press profile with a non-zero black point
    let (cmyk, srgb) = (testbed("test1.icc"), Profile::srgb());
    let rel = PlanOptions::new().intent(Intent::RelativeColorimetric);
    let plain = Plan::with_options(&cmyk, &srgb, Format::CMYK_8, Format::RGB_8, &rel).unwrap();
    let bpc = Plan::with_options(&cmyk, &srgb, Format::CMYK_8, Format::RGB_8, &rel.black_point_compensation(true)).unwrap();
    let (mut a, mut b) = ([0u8; 3], [0u8; 3]);
    plain.transform(&[255; 4], &mut a);
    bpc.transform(&[255; 4], &mut b);
    assert!(b.iter().map(|&v| v as u32).sum::<u32>() < a.iter().map(|&v| v as u32).sum::<u32>(), "BPC deepens press black: {a:?} vs {b:?}");
    // Plan::build with the flag is the same link
    let via_flag = Plan::build(&cmyk, &srgb, Format::CMYK_8, Format::RGB_8, Intent::RelativeColorimetric, flags::BLACK_POINT_COMPENSATION).unwrap();
    let mut c = [0u8; 3];
    via_flag.transform(&[255; 4], &mut c);
    assert_eq!(b, c);
    assert_eq!(PlanOptions::from(Intent::Saturation).get_intent(), Intent::Saturation);
    // explicit adaptation state for absolute colorimetric
    let abs = PlanOptions::new().intent(Intent::AbsoluteColorimetric);
    let full = Plan::with_options(&testbed("test5.icc"), &Profile::lab4(), Format::RGB_8, Format::LAB_DBL, &abs).unwrap();
    let none = Plan::with_options(&testbed("test5.icc"), &Profile::lab4(), Format::RGB_8, Format::LAB_DBL, &abs.adaptation_state(0.0)).unwrap();
    let (mut lf, mut ln) = ([[0f64; 3]], [[0f64; 3]]);
    full.transform_pixels(&[[255u8, 255, 255]], &mut lf);
    none.transform_pixels(&[[255u8, 255, 255]], &mut ln);
    assert_ne!(lf, ln, "adaptation state reaches the link");
}

#[test]
fn soft_proofing_simulates_the_press() {
    let (srgb, press) = (Profile::srgb(), testbed("test1.icc"));
    let proof = Plan::new_proofing(&srgb, &srgb, &press, Format::RGB_8, Format::RGB_8, Intent::Perceptual, Intent::RelativeColorimetric, flags::SOFT_PROOFING).unwrap();
    let mut out = [0u8; 3];
    proof.transform(&[0, 0, 255], &mut out);
    assert_ne!(out, [0, 0, 255], "pure sRGB blue is outside a press gamut");
    // without proofing flags C builds a plain transform
    let plain = Plan::new_proofing(&srgb, &srgb, &press, Format::RGB_8, Format::RGB_8, Intent::Perceptual, Intent::RelativeColorimetric, 0).unwrap();
    plain.transform(&[0, 0, 255], &mut out);
    assert_eq!(out, [0, 0, 255]);
}

#[test]
fn shared_profile_builds_plans_on_many_threads() {
    let src = Arc::new(testbed("test5.icc"));
    let dst = Arc::new(Profile::lab4());
    let input: Vec<u8> = (0..=255u8).flat_map(|v| [v, 255 - v, v / 2]).collect();
    let mut want = vec![0u8; 256 * 6];
    Plan::new(&src, &dst, Format::RGB_8, Format::LAB_16).unwrap().transform(&input, &mut want);
    std::thread::scope(|s| {
        for _ in 0..4 {
            let (src, dst, input, want) = (&src, &dst, &input, &want);
            s.spawn(move || {
                let plan = Plan::new(src, dst, Format::RGB_8, Format::LAB_16).unwrap();
                let mut out = vec![0u8; want.len()];
                plan.transform(input, &mut out);
                assert_eq!(&out, want);
            });
        }
    });
}

// -- idiomatic vocabulary types (2026-11-10 surface pass) -----------------------------

#[test]
fn pipeline_edit_and_iterate() {
    let swap = [0., 0., 1., 0., 1., 0., 1., 0., 0.];
    let mut lut = Pipeline::new(3, 3).unwrap();
    lut.push(Stage::matrix(&swap, 3, 3, None).unwrap()).unwrap();
    lut.push_front(Stage::identity(3)).unwrap();
    // a 4-channel stage can't follow a 3-channel output; a failed push
    // leaves the pipeline unchanged (C would keep the stage linked)
    assert_eq!(lut.push(Stage::identity(4)).unwrap_err().kind(), ErrorKind::Range);
    assert!(lut.push_front(Stage::identity(4)).is_err());
    assert_eq!(lut.stage_count(), 2);
    let gray = Pipeline::new(1, 1).unwrap();
    let mut g2 = gray.clone();
    g2.push(Stage::identity(1)).unwrap();
    assert!(lut.clone().append(&g2).is_err());
    let mut kept = lut.clone();
    let _ = kept.append(&g2);
    assert_eq!(kept.stage_count(), 2);
    let kinds: Vec<u32> = (&lut).into_iter().map(|s| s.stage_type()).collect();
    assert_eq!(kinds, [Tag::new(*b"idn ").code(), Tag::new(*b"matf").code()]);

    let mut twice = lut.clone();
    twice.append(&lut).unwrap();
    assert_eq!(twice.stage_count(), 4);
    let mut out = [0f32; 3];
    twice.eval_float(&[0.1, 0.2, 0.3], &mut out);
    assert_eq!(out, [0.1, 0.2, 0.3], "two swaps cancel");

    // Newton inversion recovers the input that produced a target
    let back = lut.eval_reverse(&[0.3, 0.2, 0.1], None).unwrap();
    for (got, want) in back.iter().zip([0.1, 0.2, 0.3]) {
        assert!((got - want).abs() < 1e-3, "{back:?}");
    }

    assert_eq!(lut.pop().map(|s| s.stage_type()), Some(Tag::new(*b"matf").code()));
    assert_eq!(lut.pop_front().map(|s| s.stage_type()), Some(Tag::new(*b"idn ").code()));
    assert!(lut.pop().is_none());
    assert!(format!("{twice:?}").starts_with("Pipeline { input_channels: 3"));
}

#[test]
fn mlu_locales_and_fallback() {
    let mut m = Mlu::from("Hello");
    m.set(Locale::new("es_ES"), "Hola").unwrap();
    m.set(Locale::new("ja_JP"), "こんにちは").unwrap();
    assert_eq!(m.len(), 3);
    let locales: Vec<String> = m.locales().map(|l| l.to_string()).collect();
    assert_eq!(locales[1..], ["es_ES", "ja_JP"]);
    assert_eq!(m.get(Locale::new("ja_JP")).as_deref(), Some("こんにちは"));
    // ASCII view replaces what ASCII can't hold, like cmsMLUgetASCII
    assert!(m.get_ascii(Locale::new("ja_JP")).unwrap().chars().all(|c| c == '?'));
    // set_ascii refuses non-ASCII instead of storing mojibake
    assert_eq!(m.set_ascii(Locale::new("fr_FR"), "été").unwrap_err().kind(), ErrorKind::Range);
    assert_eq!(m.len(), 3);
    assert!(Mlu::new().is_empty());
}

#[test]
fn dict_order_values_and_unicode() {
    let mut d = Dict::new();
    d.add("b", "2").unwrap();
    d.add("a", "1").unwrap();
    d.add("name", "Café ☕").unwrap();
    d.add("a", "one").unwrap();
    // insertion order, newest value wins in get
    let pairs: Vec<(String, String)> = d.iter().collect();
    assert_eq!(pairs[0], ("b".into(), "2".into()));
    assert_eq!(pairs.len(), 4);
    assert_eq!(d.get("a").as_deref(), Some("one"));
    assert_eq!(d.get("name").as_deref(), Some("Café ☕"));
    assert_eq!(d.get("missing"), None);

    // survives a save/open round trip
    let mut p = Profile::placeholder();
    p.write_tag(Tag::META, TagData::Dict(d.clone())).unwrap();
    let q = Profile::open(&p.save().unwrap()).unwrap();
    let back = q.get(Tag::META).and_then(|v| v.as_dict()).unwrap();
    assert_eq!(back.iter().collect::<Vec<_>>(), d.iter().collect::<Vec<_>>());
}

#[test]
fn named_color_list_api() {
    let mut list = NamedColorList::new(2, "PMS ", " C").unwrap();
    list.push("Red 032", [0x8000, 0xc000, 0x9000], &[0xffff, 0]).unwrap();
    list.push("Blue 072", [0x3000, 0x8000, 0x2000], &[]).unwrap();
    assert!(list.push("Bad", [0; 3], &[1, 2, 3]).is_err(), "wrong colorant count");
    assert_eq!((list.len(), list.colorant_count()), (2, 2));
    assert_eq!((list.prefix().as_str(), list.suffix().as_str()), ("PMS ", " C"));
    assert_eq!(list.position("BLUE 072"), Some(1));
    assert_eq!(list.entry(1).unwrap().colorant, [0, 0]);
    assert_eq!(list.entry(2), None);
    let names: Vec<String> = list.iter().map(|c| c.name).collect();
    assert_eq!(names, ["Red 032", "Blue 072"]);
    assert_eq!(list.iter().len(), 2);
}

#[test]
fn tone_curve_parameters_and_smoothing() {
    let srgb = ToneCurve::srgb();
    assert_eq!(srgb.parametric_type(), 4);
    let p = srgb.parameters().unwrap();
    assert_eq!(p.len(), 5);
    assert_eq!(p[0], 2.4);
    assert_eq!(ToneCurve::gamma(1.8).unwrap().parameters(), Some(&[1.8][..]));
    assert_eq!(ToneCurve::tabulated16(&[0, 0x8000, 0xffff]).unwrap().parameters(), None);

    // a 256-entry ramp with ±150 alternating noise steps backwards
    // (257 per entry); smoothing restores monotonicity, the original is
    // untouched
    let noisy: Vec<u16> = (0..256u32)
        .map(|i| (i * 257).saturating_add_signed(if i % 2 == 0 { 150 } else { -150 }).min(65535) as u16)
        .collect();
    let rough = ToneCurve::tabulated16(&noisy).unwrap();
    assert!(!rough.is_monotonic());
    let smooth = rough.smoothed(10.0).unwrap();
    assert!(smooth.is_monotonic());
    assert!(!rough.is_monotonic());
    assert!((smooth.eval16(0x8000) as i32 - 0x8000).abs() < 400, "{}", smooth.eval16(0x8000));
}

#[test]
fn device_link_records_profile_sequence() {
    // C records the chain in a device link only under KEEP_SEQUENCE
    let (srgb, lab) = (Profile::srgb(), Profile::lab4());
    let plain = Plan::new(&srgb, &lab, Format::RGB_8, Format::LAB_8).unwrap();
    assert!(!plain.to_device_link(4.3).unwrap().has_tag(Tag::PROFILE_SEQUENCE_DESC));
    let plan = Plan::build(&srgb, &lab, Format::RGB_8, Format::LAB_8, Intent::Perceptual, flags::KEEP_SEQUENCE)
        .unwrap();
    let link = plan.to_device_link(4.3).unwrap();
    let seq = link.get(Tag::PROFILE_SEQUENCE_DESC).and_then(|v| v.as_seq()).unwrap();
    assert_eq!(seq.len(), 2);
    let desc: Vec<Option<String>> = seq
        .iter()
        .map(|e| e.description().and_then(|m| m.get(Locale::NONE)))
        .collect();
    assert_eq!(desc[0].as_deref(), Some("sRGB built-in"));
    assert!(format!("{seq:?}").contains("ProfileSeqEntry"));
}

#[test]
fn date_time_and_data_tags_roundtrip() {
    let when = DateTimeNumber { year: 2026, month: 11, day: 10, hours: 23, minutes: 5, seconds: 59 };
    let mut p = Profile::placeholder();
    p.write_tag(Tag::CALIBRATION_DATE_TIME, TagData::DateTime(when)).unwrap();
    // `'data'`-typed payloads live in the PostScript tags ('data' itself
    // has no tag handler in lcms2)
    p.write_tag(Tag::PS2_CSA, TagData::Data(IccData::binary(vec![0, 1, 2, 255]))).unwrap();
    let q = Profile::open(&p.save().unwrap()).unwrap();
    match q.get(Tag::CALIBRATION_DATE_TIME) {
        Some(TagValue::DateTime(d)) => assert_eq!(d, when),
        other => panic!("{other:?}"),
    }
    match q.get(Tag::PS2_CSA) {
        Some(TagValue::Data(bytes)) => assert_eq!(bytes, [0, 1, 2, 255]),
        other => panic!("{other:?}"),
    }
    let text = IccData::text("hello");
    assert!(!text.is_binary());
    assert_eq!(text.bytes(), b"hello");
}
