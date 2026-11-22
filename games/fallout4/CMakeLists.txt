add_library(rsf_game_fallout4 SHARED src/fallout4_exports.def src/host_boundary.c
    src/cpu_events.c src/render_inputs.c src/render_hooks.c src/resolution.c src/screen.c
    src/depth_domains.c)
# Identity, lifecycle, Runtime session, DPI startup, creation sequencing and the CPU event owner are
# Rust (src/lib.rs, src/cpu_events.rs), including MinHook trampoline ownership. The C11 leaves
# capture native return addresses, retain typed original calls and contain foreign C++ exceptions
# with structured exception handling (src/native_guard.h). Rust also owns
# depth-domain admission and composition, producer resolution, the render hook lifecycle with
# World/Present admission, reduced-render recovery, SR/HUDless publication, the FRAME,
# SUBMISSION and WINDOW scope, the imagespace, Effects and TAA seams and the viewport, DR and
# prepass seams, and render-input admission with its camera history and leased passes.
# Their C11 leaves provide typed native calls through the matching private platform headers.
# Nothing in this adapter is first-party C++.
rsf_add_rust_static(rsf_fallout4_rust rsf_rust_fallout4 rsf-fallout4 rsf_fallout4)
target_include_directories(rsf_game_fallout4 PRIVATE
    "${CMAKE_SOURCE_DIR}/runtime/orchestrator/include"
    "${CMAKE_SOURCE_DIR}/runtime/graphics/include"
    "${CMAKE_SOURCE_DIR}/runtime/presentation/include")
target_link_libraries(rsf_game_fallout4 PRIVATE rsf_fallout4_rust ReScaleFrame::GameSDK
    ReScaleFrame::RuntimeCommon rsf_steam_d3d11_chain minhook rsf_build_options)
set_target_properties(rsf_game_fallout4 PROPERTIES OUTPUT_NAME "ReScaleFrame.Game.Fallout4")
