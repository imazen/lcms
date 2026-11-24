use moxcms::ColorProfile;
#[cfg(feature = "inspect")]
use moxcms::ProfileText;
#[cfg(not(feature = "inspect"))]
use moxcms::{Layout, RenderingIntent, TransformOptions};
use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args_os().skip(1);
    let path = args.next().ok_or("pass an ICC profile path")?;
    let profile = ColorProfile::new_from_slice(&std::fs::read(path)?)?;
    #[cfg(feature = "inspect")]
    {
        let description = match &profile.description {
            Some(ProfileText::PlainString(text)) => text.as_str(),
            Some(ProfileText::Description(text)) => text.ascii_string.as_str(),
            Some(ProfileText::Localizable(text)) => text
                .iter()
                .find(|entry| entry.language == "en" && entry.country == "US")
                .or_else(|| text.iter().find(|entry| entry.language == "en"))
                .or_else(|| text.first())
                .map_or("", |entry| entry.value.as_str()),
            None => "",
        };
        println!(
            "{:08x}\t{:08x}\t{:08x}\t{}",
            u32::from(profile.version()),
            u32::from(profile.color_space),
            u32::from(profile.pcs),
            description,
        );
    }
    #[cfg(not(feature = "inspect"))]
    {
        let input = std::fs::read(args.next().ok_or("pass a packed RGB8 file")?)?;
        if input.len() % 3 != 0 || input.len() / 3 > u32::MAX as usize {
            return Err("invalid RGB8 length".into());
        }
        let transform = profile.create_transform_8bit(
            Layout::Rgb,
            &ColorProfile::new_srgb(),
            Layout::Rgb,
            TransformOptions {
                rendering_intent: RenderingIntent::RelativeColorimetric,
                ..TransformOptions::default()
            },
        )?;
        let mut output = vec![0; input.len()];
        transform.transform(&input, &mut output)?;
        let hash = output.iter().fold(2_166_136_261u32, |hash, &byte| {
            (hash ^ u32::from(byte)).wrapping_mul(16_777_619)
        });
        println!("{hash:08x}");
    }
    Ok(())
}
