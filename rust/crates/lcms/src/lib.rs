//! # lcms
//!
//! A pure-Rust, algorithm-for-algorithm port of
//! [Little CMS](https://github.com/mm2/Little-CMS) **2.20**, verified
//! bit-exact against the C library — which lives in `reference/lcms2` and is
//! compiled by the test-only `lcms-cref` crate as the differential oracle.
//!
//! Module layout mirrors the C source 1:1 (`pcs.rs` ↔ `cmspcs.c`, …);
//! `PORTING.md` at the repo root maps each file to its parity gate.
//!
//! `#![forbid(unsafe_code)]` — including the table-driven half-float code.

#![forbid(unsafe_code)]

#[cfg(feature = "little-cms-api")]
pub mod alpha;
#[cfg(not(feature = "little-cms-api"))]
#[allow(dead_code)]
pub(crate) mod alpha;
pub mod api;
#[cfg(feature = "lcms2-api")]
pub mod lcms2;
#[cfg(feature = "little-cms-api")]
pub mod cam02;
#[cfg(not(feature = "little-cms-api"))]
#[allow(dead_code)]
pub(crate) mod cam02;
#[cfg(feature = "little-cms-api")]
pub mod cgats;
#[cfg(not(feature = "little-cms-api"))]
#[allow(dead_code)]
pub(crate) mod cgats;
#[cfg(feature = "little-cms-api")]
pub mod context;
#[cfg(not(feature = "little-cms-api"))]
#[allow(dead_code)]
pub(crate) mod context;
#[cfg(feature = "little-cms-api")]
pub mod err;
#[cfg(not(feature = "little-cms-api"))]
#[allow(dead_code)]
pub(crate) mod err;
#[cfg(feature = "little-cms-api")]
pub mod fixed;
#[cfg(not(feature = "little-cms-api"))]
#[allow(dead_code)]
pub(crate) mod fixed;
#[cfg(feature = "little-cms-api")]
pub mod gamma;
#[cfg(not(feature = "little-cms-api"))]
#[allow(dead_code)]
pub(crate) mod gamma;
pub(crate) mod gmath;
#[cfg(feature = "little-cms-api")]
pub mod gbd;
#[cfg(not(feature = "little-cms-api"))]
#[allow(dead_code)]
pub(crate) mod gbd;
#[cfg(feature = "little-cms-api")]
pub mod half;
#[cfg(not(feature = "little-cms-api"))]
#[allow(dead_code)]
pub(crate) mod half;
#[cfg(feature = "little-cms-api")]
pub mod math;
#[cfg(not(feature = "little-cms-api"))]
#[allow(dead_code)]
pub(crate) mod math;
#[cfg(feature = "little-cms-api")]
pub mod half_tables;
#[cfg(not(feature = "little-cms-api"))]
#[allow(dead_code)]
pub(crate) mod half_tables;
#[cfg(feature = "little-cms-api")]
pub mod intrp;
#[cfg(not(feature = "little-cms-api"))]
#[allow(dead_code)]
pub(crate) mod intrp;
#[cfg(feature = "little-cms-api")]
pub mod io;
#[cfg(not(feature = "little-cms-api"))]
#[allow(dead_code)]
pub(crate) mod io;
#[cfg(feature = "little-cms-api")]
pub mod lut;
#[cfg(not(feature = "little-cms-api"))]
#[allow(dead_code)]
pub(crate) mod lut;
#[cfg(feature = "little-cms-api")]
pub mod md5;
#[cfg(not(feature = "little-cms-api"))]
#[allow(dead_code)]
pub(crate) mod md5;
#[cfg(feature = "little-cms-api")]
pub mod mtrx;
#[cfg(not(feature = "little-cms-api"))]
#[allow(dead_code)]
pub(crate) mod mtrx;
#[cfg(feature = "little-cms-api")]
pub mod named;
#[cfg(not(feature = "little-cms-api"))]
#[allow(dead_code)]
pub(crate) mod named;
#[cfg(feature = "little-cms-api")]
pub mod pcs;
#[cfg(not(feature = "little-cms-api"))]
#[allow(dead_code)]
pub(crate) mod pcs;
// `types` + `signatures` are pure ICC vocabulary — POD structs and
// signature constants, no C-API functions. Always pub so the `api` and
// `lcms2` facades can name shared data types.
pub mod signatures;
#[cfg(feature = "little-cms-api")]
pub mod tagtypes;
#[cfg(not(feature = "little-cms-api"))]
#[allow(dead_code)]
pub(crate) mod tagtypes;
pub mod types;
#[cfg(feature = "little-cms-api")]
pub mod wtpnt;
#[cfg(not(feature = "little-cms-api"))]
#[allow(dead_code)]
pub(crate) mod wtpnt;

#[cfg(feature = "little-cms-api")]
pub mod cnvrt;
#[cfg(not(feature = "little-cms-api"))]
#[allow(dead_code)]
pub(crate) mod cnvrt;
#[cfg(feature = "little-cms-api")]
pub mod io1;
#[cfg(not(feature = "little-cms-api"))]
#[allow(dead_code)]
pub(crate) mod io1;
#[cfg(feature = "little-cms-api")]
pub mod opt;
#[cfg(not(feature = "little-cms-api"))]
#[allow(dead_code)]
pub(crate) mod opt;
#[cfg(feature = "little-cms-api")]
pub mod pack;
#[cfg(not(feature = "little-cms-api"))]
#[allow(dead_code)]
pub(crate) mod pack;
#[cfg(feature = "little-cms-api")]
pub mod profile;
#[cfg(not(feature = "little-cms-api"))]
#[allow(dead_code)]
pub(crate) mod profile;
#[cfg(feature = "little-cms-api")]
pub mod ps2;
#[cfg(not(feature = "little-cms-api"))]
#[allow(dead_code)]
pub(crate) mod ps2;
#[cfg(feature = "little-cms-api")]
pub mod sm;
#[cfg(not(feature = "little-cms-api"))]
#[allow(dead_code)]
pub(crate) mod sm;
#[cfg(feature = "little-cms-api")]
pub mod virt;
#[cfg(not(feature = "little-cms-api"))]
#[allow(dead_code)]
pub(crate) mod virt;
#[cfg(feature = "little-cms-api")]
pub mod xform;
#[cfg(not(feature = "little-cms-api"))]
#[allow(dead_code)]
pub(crate) mod xform;
