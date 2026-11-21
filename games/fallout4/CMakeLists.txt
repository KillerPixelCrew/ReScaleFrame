add_library(rsf_game_fallout4 SHARED src/plugin.cpp src/native_creation.cpp
    src/cpu_events.cpp src/render_inputs.cpp src/render_hooks.cpp src/resolution.cpp src/screen.cpp
    src/depth_domains.cpp)
# Identity, lifecycle, Runtime session, DPI startup, creation sequencing and the CPU event owner are
# Rust (src/lib.rs, src/cpu_events.rs). plugin.cpp and native_creation.cpp supply the native leaves
# through src/fallout4_platform.h; cpu_events.cpp supplies the CPU detour shells, MinHook
# trampolines and guarded host callback through src/cpu_events_platform.h. Rust also owns
# depth-domain admission and composition, producer resolution, the render hook lifecycle with
# World/Present admission, and render-input admission with its camera history and leased passes.
# Their C++ leaves provide typed native calls through the matching private platform headers.
rsf_add_rust_static(rsf_fallout4_rust rsf_rust_fallout4 rsf-fallout4 rsf_fallout4)
target_include_directories(rsf_game_fallout4 PRIVATE
    "${CMAKE_SOURCE_DIR}/runtime/orchestrator/include"
    "${CMAKE_SOURCE_DIR}/runtime/graphics/include"
    "${CMAKE_SOURCE_DIR}/runtime/presentation/include")
target_link_libraries(rsf_game_fallout4 PRIVATE rsf_fallout4_rust ReScaleFrame::GameSDK
    ReScaleFrame::RuntimeCommon rsf_steam_d3d11_chain minhook rsf_build_options)
set_target_properties(rsf_game_fallout4 PROPERTIES OUTPUT_NAME "ReScaleFrame.Game.Fallout4")

# Debug deployment set: the shared carrier embeds the game ABI too.
if(MSVC)
    add_custom_target(rsf_fallout4_trial DEPENDS rsf_game_fallout4 rsf_orchestrator rsf_proxy_plugin)
endif()
