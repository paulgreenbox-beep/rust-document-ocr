fn main() {
    // vcpkg's static Tesseract dependency graph uses Windows APIs through
    // libarchive, OpenSSL, and libcurl. These system import libraries are not
    // propagated by the current Rust sys crates on MSVC.
    #[cfg(target_os = "windows")]
    for library in ["crypt32", "secur32", "iphlpapi", "xmllite", "user32"] {
        println!("cargo:rustc-link-lib={library}");
    }
}
