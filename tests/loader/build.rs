// SPDX-License-Identifier: MIT
/// Binds the loader fixture's census facts header.
fn bindings() {
    let builder = rsf_bindgen::owner(
        &["tests/loader/census_facts.h"],
        &[],
        &["tests/loader"],
        &[],
        &[],
    );
    rsf_bindgen::write(builder, "sys.rs");
}

fn main() {
    bindings();
}
