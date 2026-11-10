//! Public-API surface snapshots for the lcms crate (docs/public-api/).
//! Shared implementation + format docs: the `zenutils-apidoc` crate.
#[test]
fn public_api_surface_docs_are_current() {
    zenutils_apidoc::ApiDoc::new()
        .workspace_dir("..")
        .crates(["lcms"])
        // the math providers are mutually exclusive — pin one.
        .exclude_features("lcms", ["math-rust", "math-zen"])
        .run();
}
