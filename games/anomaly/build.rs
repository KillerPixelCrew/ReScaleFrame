// SPDX-License-Identifier: MIT
//! The window subclass leaves import GetWindowSubclass and its siblings, which only Common
//! Controls 6 exports. Without the dependency the loader binds the system comctl32 5.82 and the
//! process fails with 0xc0000139. Cargo links only the unit-test and example executables of this
//! package; the staticlib the DLLs link is unaffected.
/// Binds the Anomaly adapter's private headers. tests/game-sdk uses the observer records from here.
fn bindings() {
    const HEADERS: &[&str] = &[
        "engine_shader_includes.h",
        "fg_stage/fixture_platform.h",
        "fg_stage_platform.h",
        "geometry_registry_platform.h",
        "motion_device_platform.h",
        "motion_stage_platform.h",
        "motion_frame_target.h",
        "motion_target_platform.h",
        "native_geometry/fixture_platform.h",
        "native_geometry_platform.h",
        "native_hooks.h",
        "native_observer_platform.h",
        "overlay_platform.h",
        "shader_frame_layout.h",
        "sr_d3d11/temporal_device_platform.h",
        "sr_stage_platform.h",
        "sr_swap_platform.h",
        "../temporal/fixture_platform.h",
    ];
    fn derives(name: &str) -> &'static [&'static str] {
        match name {
            "rsf_anomaly_geometry_key" | "rsf_anomaly_plan_key" => {
                &["Eq", "Ord", "PartialEq", "PartialOrd"]
            }
            "rsf_anomaly_sr_settings" => &["PartialEq", "Eq"],
            "rsf_anomaly_geometry_engine" => &["PartialEq"],
            "rsf_anomaly_plan_matrix"
            | "rsf_anomaly_plan_extent"
            | "rsf_anomaly_plan_vec2"
            | "rsf_anomaly_plan_vec4"
            | "rsf_anomaly_plan_resolutions"
            | "rsf_anomaly_plan_camera"
            | "rsf_anomaly_plan_frame"
            | "rsf_anomaly_plan_token" => &["PartialEq"],
            _ => &[],
        }
    }
    let headers: Vec<String> = HEADERS
        .iter()
        .map(|h| format!("games/anomaly/src/{h}"))
        .collect();
    let headers: Vec<&str> = headers.iter().map(String::as_str).collect();
    let builder = rsf_bindgen::owner(&headers, &[], &["games/anomaly"], &[], &[])
        .parse_callbacks(Box::new(rsf_bindgen::Derives(derives)))
        // The adapter declares the C calls it makes by hand, with Rust types.
        .ignore_functions();
    rsf_bindgen::write(builder, "sys.rs");
}

fn main() {
    bindings();
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
