// SPDX-License-Identifier: MIT
//! The window subclass leaves import GetWindowSubclass and its siblings, which only Common
//! Controls 6 exports. Without the dependency the loader binds the system comctl32 5.82 and the
//! process fails with 0xc0000139. Cargo links only the unit-test and example executables of this
//! package; the staticlib the DLLs link is unaffected.
fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo::rustc-link-arg=/MANIFEST:EMBED");
        println!(
            "cargo::rustc-link-arg=/MANIFESTDEPENDENCY:type='win32' \
             name='Microsoft.Windows.Common-Controls' version='6.0.0.0' \
             processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'"
        );
    }
}
