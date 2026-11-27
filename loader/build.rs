// SPDX-License-Identifier: MIT
//! Binds the census and diagnostics platform headers the loader's C leaves share with it. The
//! public census and module-dump headers come from rsf-game-sys.

fn main() {
    let census = ["census", "wrapper", "d3d", "misc", "fsr"]
        .map(|name| format!("loader/census/src/{name}_platform.h"));
    let headers: Vec<&str> = census
        .iter()
        .map(String::as_str)
        .chain([
            "loader/diagnostics/src/frame_capture_platform.h",
            "loader/diagnostics/src/module_dump_records.h",
        ])
        .collect();
    let builder = rsf_bindgen::owner(
        &headers,
        &[],
        &["loader/census/src", "loader/diagnostics/src"],
        &[],
        &[],
    )
    .prepend_enum_name(false)
    // The loader declares the C calls it makes by hand.
    .ignore_functions();
    rsf_bindgen::write(builder, "sys.rs");
}
