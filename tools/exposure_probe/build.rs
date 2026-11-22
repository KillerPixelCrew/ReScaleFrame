// SPDX-License-Identifier: MIT
/// Binds the exposure probe's SDK and DLSS headers.
fn bindings() {
    let builder = rsf_bindgen::owner(
        &[
            "tools/exposure_probe/sdk.h",
            "tools/exposure_probe/dlss_abi.h",
        ],
        &[],
        &["tools/exposure_probe"],
        &[],
        &[],
    );
    rsf_bindgen::write(builder, "sys.rs");
}

fn main() {
    bindings();
}
