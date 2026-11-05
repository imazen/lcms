//! `cmssamp.c` (partial — sampling helpers) — black-point detection entry
//! points. The darker-colorant/perceptual-black paths build round-trip
//! transforms, so they land with `cmsxform`.
use crate::profile::IccProfile;
use crate::types::CieXyz;

/// `cmsDetectBlackPoint` — TODO(cmssamp): full port once xform exists
pub fn detect_black_point(_icc: &mut IccProfile, _intent: u32, _flags: u32) -> Option<CieXyz> {
    None
}

/// `cmsDetectDestinationBlackPoint`
pub fn detect_destination_black_point(
    _icc: &mut IccProfile,
    _intent: u32,
    _flags: u32,
) -> Option<CieXyz> {
    None
}
