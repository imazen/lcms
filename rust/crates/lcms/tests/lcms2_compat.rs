//! lcms2-crate test suite, extracted with MIT credit — see `lcms2_crate/mod.rs`.
//! Gate for the `lcms2-api` shim: every test must compile+pass verbatim.
#![cfg(feature = "lcms2-api")]
pub use lcms::lcms2::*; // `crate::Profile` etc. in their tests resolve here
mod lcms2_crate;
