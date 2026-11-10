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
    assert_send::<NamedColors<'static>>();
    assert_sync::<NamedColors<'static>>();
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
    assert_send::<Stride>();
    assert_sync::<Stride>();
}

// -- profile basics --------------------------------------------------------------

#[test]
fn srgb_profile_basics() {
    let p = Profile::srgb().unwrap();
    assert_eq!(p.device_class(), ProfileClass::Display);
    assert_eq!(p.color_space(), ColorSpace::Rgb);
    assert_eq!(p.pcs(), ColorSpace::Xyz);
    assert_eq!(p.header_intent(), Intent::Perceptual);
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
    assert_eq!(Profile::lab4().unwrap().color_space(), ColorSpace::Lab);
    assert_eq!(Profile::lab2().unwrap().color_space(), ColorSpace::Lab);
    assert_eq!(Profile::xyz().unwrap().color_space(), ColorSpace::Xyz);
    assert_eq!(Profile::gray().unwrap().color_space(), ColorSpace::Gray);
    let null = Profile::null().unwrap();
    assert_eq!(null.device_class(), ProfileClass::Output);
    assert_eq!(null.color_space(), ColorSpace::Gray);
    let oklab = Profile::oklab().unwrap();
    assert_eq!(oklab.device_class(), ProfileClass::ColorSpace);
    assert_eq!(oklab.color_space(), ColorSpace::NColor(3));
    // custom RGB + gray curves
    let c = ToneCurve::gamma(2.2).unwrap();
    let g = Profile::gray_curve(&d50_xyy(), &c).unwrap();
    assert_eq!(g.color_space(), ColorSpace::Gray);
}

#[test]
fn save_open_roundtrip() {
    let mut p = Profile::srgb().unwrap();
    assert!(p.set_info(InfoType::Copyright, "test copyright"));
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
    assert!(p.set_info(InfoType::Description, "spot palette"));
    assert!(p.write_tag(Tag::MEDIA_WHITE_POINT, TagData::Xyz(d50_xyz())));
    let bytes = p.save().unwrap();
    let q = Profile::open(&bytes).unwrap();
    assert_eq!(q.device_class(), ProfileClass::NamedColor);
    assert_eq!(q.color_space(), ColorSpace::Named);
    assert_eq!(q.pcs(), ColorSpace::Lab);
    assert_eq!(q.header_intent(), Intent::AbsoluteColorimetric);
    assert_eq!(q.header_manufacturer(), u32::from_be_bytes(*b"ACME"));
    assert_eq!(
        q.info(InfoType::Description).as_deref(),
        Some("spot palette")
    );
    assert_eq!(q.white_point().unwrap().Y, d50_xyz().Y);
    // remove_tag clears the directory entry
    let mut q = q;
    q.remove_tag(Tag::MEDIA_WHITE_POINT);
    assert!(!q.has_tag(Tag::MEDIA_WHITE_POINT));
}

// -- transforms ------------------------------------------------------------------

#[test]
fn plan_srgb_to_lab() {
    let srgb = Profile::srgb().unwrap();
    let lab4 = Profile::lab4().unwrap();
    let plan = Plan::new(&srgb, &lab4, Format::RgbU8, Format::LabU8).unwrap();
    assert_eq!(plan.input_format(), Format::RgbU8);
    assert_eq!(plan.output_format(), Format::LabU8);
    assert_eq!(plan.intent(), Intent::Perceptual);
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
    let srgb = Profile::srgb().unwrap();
    let plan = Plan::new(&srgb, &srgb, Format::RgbU8, Format::RgbU8).unwrap();
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
    let srgb = Profile::srgb().unwrap();
    let lab4 = Profile::lab4().unwrap();
    let plan = Plan::new(&srgb, &lab4, Format::RgbU8, Format::LabU8).unwrap();
    let mut sc = plan.scratch();
    // in-place when sizes match
    let id = Plan::new(&srgb, &srgb, Format::RgbU8, Format::RgbU8).unwrap();
    let mut buf = vec![10u8, 20, 30, 40, 50, 60];
    assert!(id.apply_in_place(&mut id.scratch(), &mut buf));
    // apply_strided: 2 rows of 1 px with a doubled stride
    let src = [255u8, 0, 0, 99, 0, 255, 0, 99]; // px, pad, px, pad
    let mut dst = [0u8; 8];
    plan.apply_strided(&mut sc, &src, &mut dst, 1, 2, &Stride::chunky(4));
    assert!(dst[0] > 0);
    assert!(dst[4] > 0);
}

#[test]
fn plan_copy_alpha() {
    let srgb = Profile::srgb().unwrap();
    let plan = Plan::build(
        &srgb,
        &srgb,
        Format::RgbaU8,
        Format::RgbaU8,
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
    let srgb = Profile::srgb().unwrap();
    let lab4 = Profile::lab4().unwrap();
    let plan = Plan::new(&srgb, &lab4, Format::RgbU8, Format::LabU8).unwrap();
    let n = 10_000u32;
    let src: Vec<u8> = (0..n * 3).map(|i| (i * 37) as u8).collect();
    let mut serial = vec![0u8; src.len()];
    let mut par = vec![0u8; src.len()];
    plan.apply(&mut plan.scratch(), &src, &mut serial);
    plan.apply_par(&src, &mut par, n, 4);
    assert_eq!(serial, par);
}

#[test]
fn plan_multiprofile_and_proofing() {
    let srgb = Profile::srgb().unwrap();
    let lab4 = Profile::lab4().unwrap();
    let xyz = Profile::xyz().unwrap();
    let mp = Plan::new_multiprofile(
        &[&srgb, &lab4, &xyz],
        Format::RgbU8,
        Format::XyzU16,
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
        Format::RgbU8,
        Format::RgbU8,
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
        Format::RgbU8,
        Format::RgbU8,
        Intent::Perceptual,
        Intent::Perceptual,
        flags::GAMUT_CHECK,
    )
    .unwrap();
}

#[test]
fn plan_to_device_link() {
    let srgb = Profile::srgb().unwrap();
    let lab4 = Profile::lab4().unwrap();
    let plan = Plan::new(&srgb, &lab4, Format::RgbU8, Format::LabU8).unwrap();
    let link = plan.to_device_link(4.3).unwrap();
    assert_eq!(link.device_class(), ProfileClass::DeviceLink);
    // the link serializes and reopens
    let bytes = link.save().unwrap();
    let q = Profile::open(&bytes).unwrap();
    assert_eq!(q.device_class(), ProfileClass::DeviceLink);
}

#[test]
fn plan_shared_across_threads() {
    let srgb = Profile::srgb().unwrap();
    let lab4 = Profile::lab4().unwrap();
    let plan = Arc::new(Plan::new(&srgb, &lab4, Format::RgbU8, Format::LabU8).unwrap());
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
    assert!(p.write_tag(Tag::RED_TRC, TagData::Curve(c)));
    match p.get(Tag::RED_TRC) {
        Some(TagValue::Curve(rc)) => {
            let g = rc.eval16(0x8000);
            assert!(g > 0 && g < 0xffff);
        }
        other => panic!("rTRC roundtrip: {other:?}"),
    }
    // signature + data + chromaticity
    assert!(p.write_tag(Tag::TECHNOLOGY, TagData::Signature(0x6D6E7472)));
    match p.get(Tag::TECHNOLOGY) {
        Some(TagValue::Signature(s)) => assert_eq!(s, 0x6D6E7472),
        other => panic!("tech: {other:?}"),
    }
    // Xyz write + read
    assert!(p.write_tag(Tag::MEDIA_WHITE_POINT, TagData::Xyz(d50_xyz())));
    assert_eq!(p.white_point().unwrap().Y, 1.0);
    // serialize + reopen keeps them
    let q = Profile::open(&p.save().unwrap()).unwrap();
    assert!(matches!(q.get(Tag::RED_TRC), Some(TagValue::Curve(_))));
    assert_eq!(q.white_point().unwrap().Y, 1.0);
}

#[test]
fn tag_info_locales() {
    let mut mlu = Mlu::new(2);
    assert!(mlu.set(Locale::new("en_US"), "Color profile"));
    assert!(mlu.set(Locale::new("de_DE"), "Farbprofil"));
    assert_eq!(mlu.translations(), 2);
    let codes: Vec<String> = (0..2)
        .filter_map(|i| mlu.translation_codes(i))
        .map(|l| l.to_string())
        .collect();
    assert!(
        codes.iter().any(|c| c == "en_US") && codes.iter().any(|c| c == "de_DE"),
        "{codes:?}"
    );
    let mut p = Profile::placeholder();
    assert!(p.write_tag(Tag::PROFILE_DESCRIPTION, TagData::Mlu(mlu)));
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
    let mut list = NamedColorList::new(0, 4, "Spot ", " Ink").unwrap();
    assert!(list.push(
        "red",
        [0xffff, 0x4000, 0x2000],
        &[0xffff, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
    ));
    assert!(list.push(
        "blue",
        [0x2000, 0x4000, 0xffff],
        &[0, 0, 0, 0xffff, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
    ));
    assert_eq!(list.len(), 2);
    assert_eq!(list.position("blue"), Some(1));
    let mut p = Profile::placeholder();
    p.set_color_space(ColorSpace::Named);
    p.set_device_class(ProfileClass::NamedColor);
    assert!(p.write_tag(Tag::NAMED_COLOR2, TagData::NamedColors(list)));
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
    let gray = Profile::gray().unwrap();
    let plan = Plan::new(&link, &gray, Format::GrayU8, Format::XyzU16).unwrap();
    let mut out = [0u8; 6];
    plan.apply(&mut plan.scratch(), &[128], &mut out);
    assert!(
        u16::from_be_bytes([out[0], out[1]]) > 0 || out.iter().any(|&b| b != 0),
        "{out:?}"
    );
    // authored pipelines evaluate
    let mut pipe = Pipeline::new(1, 1).unwrap();
    let g = ToneCurve::gamma(2.0).unwrap();
    assert!(pipe.push(Stage::tone_curves(&[&g]).unwrap()));
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
    let est = g.estimated_gamma().unwrap();
    assert!((est - 2.2).abs() < 0.02, "est {est}");
    // srgb parametric + identity
    let srgb = ToneCurve::srgb().unwrap();
    assert!(srgb.is_monotonic());
    assert_eq!(srgb.parametric_type(), 4);
    let id = ToneCurve::linear().unwrap();
    assert!(id.is_linear());
    assert_eq!(id.eval16(0x4000), 0x4000);
    // tabulated + reverse roundtrip
    let tab = ToneCurve::tabulated16(&[0, 0x4000, 0x8000, 0xc000, 0xffff]).unwrap();
    assert!(tab.is_monotonic());
    let rev = g.reversed();
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
    assert_eq!(Format::RgbU8.code(), Format::RgbU8.code());
    assert_eq!(Format::from_u32(Format::RgbU8.code()), Format::RgbU8);
    assert_eq!(Format::from_u32(0xDEAD_BEEF), Format::Raw(0xDEAD_BEEF));
    assert_eq!(Format::RgbU8.pixel_size(), 3);
    assert_eq!(Format::RgbaU16.pixel_size(), 8);
    assert_eq!(Format::CmykF32.pixel_size(), 16);
    assert_eq!(Format::GrayU8.channels(), 1);
    assert_eq!(Format::RgbaU8.extra_channels(), 1);
    assert_eq!(Format::RgbaU8.total_channels(), 4);
    assert_eq!(Format::RgbU8.bytes_per_channel(), 1);
    assert!(Format::LabF32.is_float());
    assert!(!Format::LabU16.is_float());
    assert_eq!(Format::RgbU8.color_space(), Some(ColorSpace::Rgb));
    assert_eq!(Format::CmykU16.color_space(), Some(ColorSpace::Cmyk));
    // From impls
    let f: Format = Format::RgbU16.code().into();
    assert_eq!(f, Format::RgbU16);
    let code: u32 = Format::LabU8.into();
    assert_eq!(code, Format::LabU8.code());
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
    assert_eq!(Intent::from_u32(99), Intent::AbsoluteColorimetric);
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
    assert!(!e.message.is_empty());
    assert_ne!(e.kind(), ErrorKind::Undefined);
    let e2 = e.clone();
    assert_eq!(e, e2);
    // Display is "lcms error 0x..: .."
    assert!(format!("{e}").contains("lcms error"));
}

// -- free functions -------------------------------------------------------------------

#[test]
fn color_conversions() {
    let d50 = d50_xyz();
    assert!((d50.Y - 1.0).abs() < 1e-9);
    let xyy = xyz_to_xyy(&d50);
    assert!((xyy.x - d50_xyy().x).abs() < 1e-3);
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
    assert!(d.add("key", "value"));
    assert_eq!(d.get("key").as_deref(), Some("value"));
    assert_eq!(d.len(), 1);
    assert!(!d.is_empty());
    // Dict roundtrips through a tag
    let mut p = Profile::placeholder();
    assert!(p.write_tag(Tag::META, TagData::Dict(d)));
    match p.get(Tag::META) {
        Some(TagValue::Dict(d)) => assert_eq!(d.get("key").as_deref(), Some("value")),
        other => panic!("meta dict: {other:?}"),
    }
}
