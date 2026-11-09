//! lcms2-crate test suite, extracted with MIT credit — see `lcms2_crate/mod.rs`.
//! Gate for the `lcms2-api` shim: every test must compile+pass verbatim.
#![cfg(feature = "lcms2-api")]
mod lcms2_crate;
