#[cfg(feature = "inspect")]
use lcms::api::InfoType;
use lcms::api::{BehaviorMode, Profile};
#[cfg(not(feature = "inspect"))]
use lcms::api::{Format, Intent, Plan};
use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args_os().skip(1);
    let path = args.next().ok_or("pass an ICC profile path")?;
    let mode = if cfg!(feature = "spec-default") {
        BehaviorMode::SpecDefault
    } else {
        BehaviorMode::Lcms2Exact
    };
    let factory = Profile::with_mode(mode);
    let profile = factory.open_file(path)?;
    #[cfg(feature = "inspect")]
    println!(
        "{:08x}\t{:08x}\t{:08x}\t{}",
        profile.version(),
        profile.color_space().code(),
        profile.pcs().code(),
        profile.info(InfoType::Description).unwrap_or_default(),
    );
    #[cfg(not(feature = "inspect"))]
    {
        let input = std::fs::read(args.next().ok_or("pass a packed RGB8 file")?)?;
        if input.len() % 3 != 0 || input.len() / 3 > u32::MAX as usize {
            return Err("invalid RGB8 length".into());
        }
        let plan = Plan::build(
            &profile,
            &factory.srgb(),
            Format::RGB_8,
            Format::RGB_8,
            Intent::RelativeColorimetric,
            0,
        )?;
        let mut output = vec![0; input.len()];
        plan.apply(&mut plan.scratch(), &input, &mut output);
        let hash = output.iter().fold(2_166_136_261u32, |hash, &b| {
            (hash ^ u32::from(b)).wrapping_mul(16_777_619)
        });
        println!("{hash:08x}");
    }
    Ok(())
}
