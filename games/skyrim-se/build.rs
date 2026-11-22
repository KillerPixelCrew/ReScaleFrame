// SPDX-License-Identifier: MIT
/// Binds the Skyrim CPU call header (this binding had no generator before).
fn bindings() {
    fn derives(name: &str) -> &'static [&'static str] {
        match name {
            "rsf_skyrim_cpu_call" | "rsf_skyrim_menu_observation" => &["Debug"],
            _ => &[],
        }
    }
    let builder = rsf_bindgen::owner(
        &["games/skyrim-se/src/cpu_call.h"],
        &[],
        &["games/skyrim-se"],
        &[],
        &[],
    )
    .parse_callbacks(Box::new(rsf_bindgen::Derives(derives)))
    .prepend_enum_name(false)
    // The adapter declares the C calls it makes by hand.
    .ignore_functions();
    rsf_bindgen::write(builder, "sys.rs");
}

fn main() {
    bindings();
}
