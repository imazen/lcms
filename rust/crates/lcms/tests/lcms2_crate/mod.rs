//! The `lcms2` crate's own test suite, extracted verbatim from
//! `lcms2` 6.2.0 (github.com/kornelski/lcms2-rs), MIT license.
//! Copyright (c) the lcms2-rs contributors. Used per MIT terms as a
//! compat gate: every test must pass against `lcms::lcms2` with only
//! the import changed (`lcms2::` → `lcms::lcms2::`).
#![allow(unused)]
use lcms::lcms2::*;

// -- profile.rs::tags_read
#[test]

fn tags_read() {
    let prof = Profile::new_srgb();
    assert!(prof.read_tag(TagSignature::BToD0Tag).is_none());
    assert_eq!(
        CIEXYZ::d50().X,
        match prof.read_tag(TagSignature::MediaWhitePointTag) {
            Tag::CIEXYZ(xyz) => xyz.X,
            _ => panic!(),
        }
    );
}

// -- profile.rs::tags_write
#[test]

fn tags_write() {
    let mut p = Profile::new_placeholder();
    let mut mlu = MLU::new(1);
    mlu.set_text_ascii("Testing", Locale::new("en_GB"));
    assert!(p.write_tag(TagSignature::CopyrightTag, Tag::MLU(&mlu)));

    let xyz = CIEXYZ {
        X: 1.,
        Y: 2.,
        Z: 3.,
    };
    assert!(p.write_tag(TagSignature::RedColorantTag, Tag::CIEXYZ(&xyz)));

    assert!(p.has_tag(TagSignature::CopyrightTag));
    assert!(p.has_tag(TagSignature::RedColorantTag));
    assert!(!p.has_tag(TagSignature::BlueColorantTag));

    assert_eq!(
        &xyz,
        match p.read_tag(TagSignature::RedColorantTag) {
            Tag::CIEXYZ(d) => d,
            _ => panic!(),
        }
    );

    assert_eq!(
        Ok("Testing".to_owned()),
        match p.read_tag(TagSignature::CopyrightTag) {
            Tag::MLU(mlu) => mlu.text(Locale::none()),
            _ => panic!(),
        }
    );
}

// -- profile.rs::setters
#[test]

fn setters() {
    let mut p = Profile::new_placeholder();
    assert_eq!(ColorSpaceSignature::Sig1colorData, p.color_space());
    p.set_color_space(ColorSpaceSignature::RgbData);
    assert_eq!(ColorSpaceSignature::RgbData, p.color_space());
}

// -- profile.rs::icc
#[test]

fn icc() {
    let prof = Profile::new_xyz();
    assert!(prof.icc().unwrap().len() > 300);
    assert!(format!("{prof:?}").contains("XYZ identity"));
}

// -- profile.rs::bad_icc
#[test]

fn bad_icc() {
    let err = Profile::new_icc(&[1, 2, 3]);
    assert!(err.is_err());
}

// -- profile.rs::unwind_safety
#[test]

fn unwind_safety() {
    let profile = &Profile::new_xyz();
    std::panic::catch_unwind(|| {
        let _p = profile;
    })
    .unwrap();
}

// -- mlu.rs::mlu
#[test]

fn mlu() {
    let _ = MLU::new(0);
    let mut m = MLU::new(1);
    assert!(m.set_text("Hello 世界！", Locale::none()));
    assert_eq!(Ok("Hello 世界！".to_owned()), m.text(Locale::none()));
    assert!(!m.set_text_ascii("エッロル", Locale::none()));

    assert!(m.set_text("a", Locale::new("en_US")));
    assert_eq!("a", m.text_ascii(Locale::new("en_US")).unwrap());

    let mut m = MLU::new(1);
    assert!(m.set_text_ascii("OK", Locale::none()));
    assert_eq!("OK", m.text_ascii(Locale::none()).unwrap());
}

// -- namedcolorlist.rs::named
#[test]

fn named() {
    let mut n = NamedColorList::new(10, 3, "hello", "world").unwrap();
    assert!(n.append(
        "yellow",
        [1, 2, 3],
        [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]
    ));
    assert_eq!(None, n.get(10000));

    let c = n.get(0).unwrap();
    assert_eq!("yellow", c.name);
    assert_eq!("hello", c.prefix);
    assert_eq!([1, 2, 3], c.pcs);
}

// -- pipeline.rs::pipeline
#[test]

fn pipeline() {
    let p = Pipeline::new(123, 12);
    assert!(p.is_err());

    let p = Pipeline::new(4, 3).unwrap();
    assert_eq!(0, p.stage_count());
    assert_eq!(4, p.input_channels());
    assert_eq!(3, p.output_channels());
}

// -- tonecurve.rs::tones
#[test]

fn tones() {
    let _ = ToneCurve::new(0.);
    let _ = ToneCurve::new(1230.);
    let _ = ToneCurve::new(-10.);

    let g = ToneCurve::new(1. / 2.2);
    let r: &ToneCurveRef = &g;
    let mut z: ToneCurve = r.to_owned();
    assert!(g.estimated_gamma(0.1).is_some());
    assert_eq!(1., g.eval(1.));
    assert_eq!(0, g.eval(0u16));
    assert!(!z.is_linear());
    assert!(z.is_monotonic());
    assert!(!z.is_descending());
    assert!(z.reversed().is_monotonic());
    assert!(z.smooth(0.5));

    assert_eq!(0, g.estimated_entries()[0]);
    assert_eq!(u16::MAX, *g.estimated_entries().last().unwrap());

    assert!(ToneCurve::new_parametric(7, &[0.]).is_err());
}

// -- context.rs::context
#[test]

fn context() {
    let mut c = ThreadContext::new();
    assert!(c.user_data().is_null());
    c.unregister_plugins();
    assert!(crate::Profile::new_icc_context(&c, &[]).is_err());

    assert!(
        c.supported_intents()
            .contains_key(&Intent::RelativeColorimetric)
    );

    let _ = GlobalContext;
}

// -- ext.rs::temp
#[test]

fn temp() {
    assert!(crate::white_point_from_temp(4000.).is_some());
}

// -- flags.rs::flags
#[test]

fn flags() {
    let _ = Flags::default();
    let mut t = Flags::COPY_ALPHA | Flags::NO_OPTIMIZE;
    t = t | Flags::CLUT_PRE_LINEARIZATION;
    assert!(t.has(Flags::CLUT_PRE_LINEARIZATION));
    assert!(t.has(Flags::COPY_ALPHA));
    assert!(t.has(Flags::NO_OPTIMIZE));
    assert!(!t.has(Flags::DEVICELINK_8BITS));
    assert!(!t.has(Flags::GAMUT_CHECK));
    let _ = Flags::default() | Flags::NO_CACHE;
    let _ = Flags::NO_CACHE | Flags::CLUT_PRE_LINEARIZATION;
}

// -- locale.rs::locale
#[test]

fn locale() {
    let l = Locale::new("");
    assert_eq!([0 as c_char; 3], l.language);
    assert_eq!([0 as c_char; 3], l.country);

    let l = Locale::none();
    assert_eq!([0 as c_char; 3], l.language);
    assert_eq!([0 as c_char; 3], l.country);

    let l = Locale::new("Ab");
    assert_eq!(['A' as c_char, 'b' as c_char, 0], l.language);
    assert_eq!([0 as c_char; 3], l.country);

    let l = Locale::new("Ab-X");
    assert_eq!(['A' as c_char, 'b' as c_char, 0], l.language);
    assert_eq!(['X' as c_char, 0, 0], l.country);

    let l = Locale::new("overlong");
    assert_eq!(['o' as c_char, 'v' as c_char, 0], l.language);
    assert_eq!(['r' as c_char, 'l' as c_char, 0], l.country);
    unsafe {
        assert_eq!('o' as c_char, *l.language_ptr());
    }
    unsafe {
        assert_eq!('r' as c_char, *l.country_ptr());
    }
}

// -- tag.rs::tone_curves_tag
#[test]

fn tone_curves_tag() {
    let mut icc = Profile::new_srgb();
    let _ = icc.read_tag(TagSignature::VcgtTag);

    icc.remove_tag(TagSignature::ProfileDescriptionTag);

    let mut desc = MLU::new(1);
    desc.set_text("Test ICC", Locale::none());
    icc.write_tag(TagSignature::ProfileDescriptionTag, Tag::MLU(&desc));

    let tc = ToneCurve::new(2.0);
    let tc_refs: [&ToneCurveRef; 3] = [&tc, &tc, &tc];
    let before = Tag::VcgtCurves(tc_refs);
    icc.write_tag(TagSignature::VcgtTag, before);
    let before = Tag::VcgtCurves(tc_refs);
    let after = icc.read_tag(TagSignature::VcgtTag);

    match (before, after) {
        (Tag::VcgtCurves(a), Tag::VcgtCurves(b)) => {
            assert_eq!(a[0].estimated_entries(), b[0].estimated_entries());
            assert_eq!(a[1].estimated_entries(), b[1].estimated_entries());
            assert_eq!(a[2].estimated_entries(), b[2].estimated_entries());
        }
        _ => panic!("bad tags"),
    }
    icc.icc().unwrap();
}
