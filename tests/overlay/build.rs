// SPDX-License-Identifier: MIT
/// Binds the overlay fixture's view of the loader proxy's overlay host header.
fn bindings() {
    let builder = rsf_bindgen::owner(
        &["loader/proxy/src/overlay_host.h"],
        &[],
        &["loader/proxy/src/overlay_host.h"],
        &[],
        &[],
    );
    rsf_bindgen::write(builder, "sys.rs");
}

fn main() {
    bindings();
}
