//! Compare the public ICC modes on a caller-provided list of profile files.
use lcms::api::{BehaviorMode, ColorSpace, Format, Intent, Plan, Profile, ProfileClass, flags};
use std::error::Error;
use std::path::Path;

const INTENTS: [Intent; 4] = [
    Intent::Perceptual,
    Intent::RelativeColorimetric,
    Intent::Saturation,
    Intent::AbsoluteColorimetric,
];

fn format(space: ColorSpace) -> Option<Format> {
    Some(match space {
        ColorSpace::Gray => Format::GRAY_FLT,
        ColorSpace::Rgb => Format::RGB_FLT,
        ColorSpace::Cmy => Format::from_u32((1 << 22) | (5 << 16) | (3 << 3) | 4),
        ColorSpace::Cmyk => Format::CMYK_FLT,
        ColorSpace::Lab => Format::LAB_FLT,
        ColorSpace::Xyz => Format::XYZ_FLT,
        ColorSpace::Mch(n) | ColorSpace::NColor(n) if (1..=15).contains(&n) => {
            // The cmsFORMAT multichannel pixel types start with PT_MCH1 = 15.
            Format::from_u32((1 << 22) | ((u32::from(n) + 14) << 16) | (u32::from(n) << 3) | 4)
        }
        _ => return None,
    })
}

fn samples(space: ColorSpace, channels: usize) -> Vec<f32> {
    // Include both ends of the neutral axis, intermediate neutrals, channel
    // extrema and a deterministic sample of unequal channel combinations.
    let mut state = 0x2bc9_ac7bu32;
    (0..64)
        .flat_map(|pixel| {
            (0..channels)
                .map(|channel| {
                    let x = match pixel {
                        0..=8 => pixel as f32 / 8.0,
                        9..=23 => f32::from(pixel - 9 == channel),
                        _ => {
                            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                            (state >> 8) as f32 / 16_777_215.0
                        }
                    };
                    match space {
                        ColorSpace::Lab if channel > 0 => {
                            if pixel <= 8 {
                                0.0
                            } else {
                                x * 255.0 - 128.0
                            }
                        }
                        ColorSpace::Lab
                        | ColorSpace::Cmyk
                        | ColorSpace::Cmy
                        | ColorSpace::Mch(_)
                        | ColorSpace::NColor(_) => x * 100.0,
                        _ => x,
                    }
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

fn compare(compat: &[f32], spec: &[f32]) -> String {
    let mut changed = 0;
    let mut nonfinite = 0;
    let mut max = 0.0f32;
    let mut first = None;
    for (i, (&a, &b)) in compat.iter().zip(spec).enumerate() {
        if !a.is_finite() || !b.is_finite() {
            nonfinite += 1;
        } else {
            max = max.max((a - b).abs());
        }
        if a.to_bits() != b.to_bits() {
            changed += 1;
            first.get_or_insert((i, a, b));
        }
    }
    format!("{changed}\t{nonfinite}\t{max}\t{first:?}")
}

fn transform(
    profile: &Profile,
    srgb: &Profile,
    format_in: Format,
    format_out: Format,
    input: &[f32],
    intent: Intent,
    direction: &str,
) -> lcms::api::Result<Vec<f32>> {
    let flags = flags::NO_OPTIMIZE;
    let plan = match direction {
        "input" => Plan::build(profile, srgb, format_in, format_out, intent, flags)?,
        "output" => Plan::build(srgb, profile, format_in, format_out, intent, flags)?,
        _ => Plan::new_multiprofile(&[profile], format_in, format_out, intent, flags)?,
    };
    let sample_count = input.len() / format_in.channels() * format_out.channels();
    let mut output = vec![0.0; sample_count];
    plan.transform_pixels(input, &mut output);
    Ok(output)
}

fn audit(path: &Path, srgb: &[Profile; 3]) -> Result<(), Box<dyn Error>> {
    let data = std::fs::read(path)?;
    let name = path.display();
    let modes = [
        BehaviorMode::Lcms2Exact,
        BehaviorMode::SpecExact,
        BehaviorMode::SpecDefault,
    ];
    let profiles = modes.map(|mode| Profile::with_mode(mode).open(&data));
    let mut repaired = None;
    for (mode, profile) in modes.into_iter().zip(&profiles) {
        match profile {
            Ok(p) => {
                println!(
                    "OPEN\t{name}\t{mode:?}\tOK\t{:?}\t{}\t{}\t{:08x}",
                    p.device_class(),
                    p.color_space(),
                    p.pcs(),
                    p.version()
                );
                for warning in p.read_warnings() {
                    println!("WARNING\t{name}\t{mode:?}\t{warning:?}");
                }
                match p.save() {
                    Ok(buf) => match Profile::with_mode(mode).open(&buf) {
                        Ok(_) => {
                            println!("SAVE\t{name}\t{mode:?}\tOK\t{}", buf.len());
                            if !p.read_warnings().is_empty() {
                                repaired =
                                    Profile::with_mode(BehaviorMode::SpecExact).open(&buf).ok();
                            }
                        }
                        Err(e) => println!("SAVE\t{name}\t{mode:?}\tREOPEN: {e}"),
                    },
                    Err(e) => println!("SAVE\t{name}\t{mode:?}\t{e}"),
                }
            }
            Err(e) => println!("OPEN\t{name}\t{mode:?}\t{e}"),
        }
    }

    let Some(header) = profiles.iter().find_map(|p| p.as_ref().ok()) else {
        return Ok(());
    };
    let space = header.color_space();
    let pcs = header.pcs();
    let cases: &[(&str, ColorSpace, ColorSpace)] = match header.device_class() {
        ProfileClass::DeviceLink | ProfileClass::Abstract => &[("single", space, pcs)],
        ProfileClass::Input => &[("input", space, ColorSpace::Rgb)],
        ProfileClass::Output | ProfileClass::Display | ProfileClass::ColorSpace => &[
            ("input", space, ColorSpace::Rgb),
            ("output", ColorSpace::Rgb, space),
        ],
        _ => &[],
    };
    if cases.is_empty() {
        println!(
            "LINK\t{name}\tUNMEASURED\tclass {:?}",
            header.device_class()
        );
    }
    for &(direction, from, to) in cases {
        let (Some(fmt_in), Some(fmt_out)) = (format(from), format(to)) else {
            println!("LINK\t{name}\tUNMEASURED\tformat {from} to {to}");
            continue;
        };
        let input = samples(from, fmt_in.channels());
        for intent in INTENTS {
            let result = [0, 1, 2].map(|i| {
                let p = profiles[i].as_ref().map_err(|e| format!("open: {e}"))?;
                transform(p, &srgb[i], fmt_in, fmt_out, &input, intent, direction)
                    .map_err(|e| e.to_string())
            });
            for (mode, outcome) in modes.into_iter().zip(&result) {
                let status = match outcome {
                    Ok(_) => "OK",
                    Err(error) => error,
                };
                println!("LINK\t{name}\t{mode:?}\t{direction}\t{intent:?}\t{status}");
            }
            for (a, b) in [(0, 1), (1, 2)] {
                if let (Ok(a_pixels), Ok(b_pixels)) = (&result[a], &result[b]) {
                    println!(
                        "PIXELS\t{name}\t{:?}\t{:?}\t{direction}\t{intent:?}\t{}",
                        modes[a],
                        modes[b],
                        compare(a_pixels, b_pixels)
                    );
                }
            }
            if let (Some(canonical), Ok(default_pixels)) = (&repaired, &result[2]) {
                let pixels = transform(
                    canonical, &srgb[1], fmt_in, fmt_out, &input, intent, direction,
                )?;
                println!(
                    "REPAIR_PIXELS\t{name}\t{direction}\t{intent:?}\t{}",
                    compare(&pixels, default_pixels)
                );
            }
        }
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut files = std::env::args_os().skip(1).peekable();
    if files.peek().is_none() {
        return Err("pass one or more ICC profile paths".into());
    }
    let srgb = [
        BehaviorMode::Lcms2Exact,
        BehaviorMode::SpecExact,
        BehaviorMode::SpecDefault,
    ]
    .map(|mode| Profile::with_mode(mode).srgb());
    for file in files {
        audit(Path::new(&file), &srgb)?;
    }
    Ok(())
}
