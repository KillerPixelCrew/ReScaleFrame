// SPDX-License-Identifier: MIT
//! Binds the RDR2 adapter's private headers.

fn main() {
    let headers = [
        "games/rdr2/src/asi_platform.h",
        "games/rdr2/src/runtime_setup.h",
    ];
    let builder = rsf_bindgen::owner(&headers, &[], &["games/rdr2"], &[], &[])
        // The adapter declares the C calls it makes by hand.
        .ignore_functions();
    rsf_bindgen::write(builder, "sys.rs");
}
