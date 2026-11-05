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

pub mod context;
pub mod err;
pub mod fixed;
pub mod gamma;
pub mod half;
pub mod half_tables;
pub mod intrp;
pub mod lut;
pub mod md5;
pub mod mtrx;
pub mod pcs;
pub mod signatures;
pub mod types;
pub mod wtpnt;
