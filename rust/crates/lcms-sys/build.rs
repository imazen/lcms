fn main() {
    // Compile the C shim with the SAME flags as the C oracle so the libm
    // calls inside resolve to the real glibc libm functions (not Rust's
    // compiler_builtins/fdlibm intrinsics, which differ by ~1 ULP).
    cc::Build::new()
        .file("shims/libm_shim.c")
        .opt_level(2)
        .flag("-fno-strict-aliasing")
        .compile("libm_shim");
    println!("cargo:rustc-link-lib=m");
}
