// SPDX-License-Identifier: MIT
//! Binds the AC7 adapter headers, public and private. The test crates and standalone fixtures
//! use these items from this crate.

fn main() {
    let mut headers: Vec<String> = std::fs::read_dir("include/rescaleframe")
        .expect("AC7 public headers")
        .map(|entry| {
            entry
                .expect("directory entry")
                .file_name()
                .into_string()
                .unwrap()
        })
        .filter(|name| name.ends_with(".h"))
        .map(|name| format!("games/ac7/include/rescaleframe/{name}"))
        .collect();
    headers.sort();
    println!("cargo:rerun-if-changed=include/rescaleframe");
    for private in [
        "ac7_capture_io.h",
        "ac7_platform.h",
        "ac7_scope_owner.h",
        "ac7_sites.h",
        "cloud_depth_platform.h",
        "contact_shadow.h",
    ] {
        headers.push(format!("games/ac7/src/{private}"));
    }
    let headers: Vec<&str> = headers.iter().map(String::as_str).collect();
    let builder = rsf_bindgen::owner(&headers, &["games/ac7/include"], &["games/ac7"], &[], &[])
        // render_scope.rs defines these and native_renderer.rs declares them with Rust types.
        .blocklist_function("rsf_ac7_render_scope.*");
    rsf_bindgen::write(builder, "sys.rs");
}
