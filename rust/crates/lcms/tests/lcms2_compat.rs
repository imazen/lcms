//! lcms2-crate test suite, extracted with MIT credit — see `lcms2_crate/mod.rs`.
//! Gate for the `lcms2-api` shim: every test must compile+pass verbatim.
#![cfg(feature = "lcms2-api")]
pub use lcms::lcms2::*; // `crate::Profile` etc. in their tests resolve here
mod lcms2_crate;

// -- Send/Sync audit ---------------------------------------------------------
// The lcms2 crate `unsafe impl`s Send on its foreign-types handles (C's
// thread-safety contract). Our shim is `forbid(unsafe_code)` — the
// auto-traits must prove it themselves. This test is the audit:
// whatever compiles here IS Send+Sync; what's missing is documented.
mod sync_audit {
    use super::*;
    fn assert_send<T: Send>() {}
    fn assert_sync<T: Sync>() {}
    #[test]
    fn shim_types_send_sync() {
        assert_send::<Profile>();
        assert_sync::<Profile>();
        assert_send::<Transform<[u8; 3], [u8; 3]>>();
        assert_sync::<Transform<[u8; 3], [u8; 3]>>();
        assert_send::<ToneCurve>();
        assert_sync::<ToneCurve>();
        assert_send::<MLU>();
        assert_sync::<MLU>();
        assert_send::<NamedColorList>();
        assert_sync::<NamedColorList>();
        assert_send::<Pipeline>();
        assert_sync::<Pipeline>();
        assert_send::<Dict>();
        assert_sync::<Dict>();
        assert_send::<CIECAM02>();
        assert_send::<ThreadContext>();
        assert_sync::<ThreadContext>();
    }
}
