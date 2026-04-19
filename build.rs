fn main() {
    // libgcc is only needed for libffi on Android (__clear_cache symbol).
    if std::env::var("CARGO_FEATURE_FFI").is_ok()
        && std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("android")
    {
        println!("cargo:rustc-link-lib=gcc");
    }
}
