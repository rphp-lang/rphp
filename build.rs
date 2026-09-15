fn main() {
    // Require the reentrant libxcrypt ABI, not the older platform crypt().
    // Resolve the trusted target installation at build time, but do not emit
    // a DT_NEEDED dependency: requests that never use bcrypt must not load it.
    // No PHP input or runtime search path chooses the native code to load.
    pkg_config::Config::new()
        .atleast_version("4.4")
        .statik(false)
        .cargo_metadata(false)
        .probe("libxcrypt")
        .expect("libxcrypt >= 4.4 development files are required; see README.md");
    let directory = pkg_config::get_variable("libxcrypt", "libdir")
        .expect("libxcrypt's target library directory must be available");
    let filename = if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        "libcrypt.dylib"
    } else {
        "libcrypt.so"
    };
    let library = std::path::Path::new(&directory).join(filename);
    assert!(
        library.is_absolute() && library.is_file(),
        "target libxcrypt shared library is required"
    );
    println!("cargo:rustc-env=RPHP_CRYPT_LIBRARY={}", library.display());
    println!("cargo:rerun-if-changed={}", library.display());
}
