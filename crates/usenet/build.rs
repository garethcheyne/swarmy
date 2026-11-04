fn main() {
    // unrar_sys's C++ reads the Windows registry but doesn't link advapi32.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rustc-link-lib=advapi32");
    }
}
