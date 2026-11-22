// SPDX-License-Identifier: MIT
/// Binds the capture fixture header.
fn bindings() {
    let builder = rsf_bindgen::owner(
        &["tests/capture/capture_fixture_abi.h"],
        &[],
        &["tests/capture"],
        &[],
        &[],
    )
    .prepend_enum_name(false);
    rsf_bindgen::write(builder, "sys.rs");
}

fn main() {
    bindings();
}
