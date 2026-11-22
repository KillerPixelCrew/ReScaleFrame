// SPDX-License-Identifier: MIT
/// Binds the generation fixture headers.
fn bindings() {
    let headers = [
        "fixture_abi.h",
        "session_abi.h",
        "dll_abi.h",
    ]
    .map(|header| format!("tests/generation/{header}"));
    let headers: Vec<&str> = headers.iter().map(String::as_str).collect();
    let builder = rsf_bindgen::owner(&headers, &[], &["tests/generation"], &[], &[]);
    rsf_bindgen::write(builder, "sys.rs");
}

fn main() {
    bindings();
}
