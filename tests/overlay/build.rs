// SPDX-License-Identifier: MIT
/// Binds the overlay fixture's view of the Runtime's D3D11 overlay host header.
fn bindings() {
    let builder = rsf_bindgen::owner(
        &["runtime/orchestrator/src/overlay_host.h"],
        &[],
        &["runtime/orchestrator/src/overlay_host.h"],
        &[],
        &[],
    );
    rsf_bindgen::write(builder, "sys.rs");
}

fn main() {
    bindings();
}
