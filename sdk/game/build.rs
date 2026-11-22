// SPDX-License-Identifier: MIT
//! Binds every public `rescaleframe/*.h` contract header outside the games and integrations
//! once. Every other crate uses these items instead of binding the headers again.

/// Records compared or printed by the overlay panel and its tests.
fn derives(name: &str) -> &'static [&'static str] {
    match name {
        "rsf_overlay_host_option"
        | "rsf_overlay_saved_row"
        | "rsf_overlay_stats"
        | "rsf_overlay_intent"
        | "rsf_overlay_vertex"
        | "rsf_overlay_draw_call"
        | "rsf_sharpening_options" => &["Debug", "PartialEq"],
        _ => &[],
    }
}

fn main() {
    let public = rsf_bindgen::PUBLIC_INCLUDE_DIRS;
    let builder = rsf_bindgen::owner(&[], &[], public, &[], &[])
        .no_copy("rsf_d3d11_state")
        .parse_callbacks(Box::new(rsf_bindgen::Derives(derives)))
        // Implemented in Rust by this crate (frame_policy.rs, renderer_policy.rs).
        .blocklist_function(
            "rsf_frame_(has_rendered_source|allows_sr|allows_fg)|rsf_game_depth_domains_requested",
        );
    rsf_bindgen::write(builder, "sys.rs");
}
