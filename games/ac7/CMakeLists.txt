# The plugin and linked carriers call the same Rust reader through the existing view C ABI.
rsf_add_rust_static(rsf_ac7_rust rsf_rust_ac7 rsf-ac7 rsf_ac7)
# TrueSky's Rust D3D11 owner uses the shared System32 shader compiler entry.
target_link_libraries(rsf_ac7_rust INTERFACE rsf_shader_compiler)
add_library(rsf_ac7_view INTERFACE)
target_include_directories(rsf_ac7_view INTERFACE include)
target_link_libraries(rsf_ac7_view INTERFACE rsf_ac7_rust)
add_library(ReScaleFrame::AC7View ALIAS rsf_ac7_view)

# Exact-build identity, the GameSDK table and the renderer lifecycle are the Rust crate rsf-ac7
# (src/lib.rs, src/lifecycle.rs). plugin.cpp keeps the export and lends the guarded native
# renderer entry points through src/ac7_platform.h. The crate also owns the native renderer's
# code sites, patch transactions, hook guards, its prepare/start/quiesce/stop transaction and the
# TrueSky patches and relays (src/sites.rs, patches.rs, guards.rs, renderer.rs, truesky.rs),
# which native_renderer.cpp calls through
# src/ac7_sites.h with its native leaves, so the Rust archive follows the render scope
# on the link line. Only this DLL and the native graph fixture pull native_renderer.cpp.
# The crate also owns the queued render scopes (src/render_scope.rs) and exports
# ac7_render_scope.h except its two create calls, which render_scope.cpp keeps to lend its
# exception leaves (src/ac7_scope_owner.h). Every target that calls that header links
# rsf_ac7_rust after rsf_ac7_render_scope: this DLL and the render scope, D3D11 present bridge and
# native graph fixtures. The carriers use the crate through AC7View without pulling render_scope.cpp.
add_library(rsf_game_ac7 SHARED src/plugin.cpp)
target_link_libraries(rsf_game_ac7 PRIVATE ReScaleFrame::GameSDK rsf_ac7_view rsf_ac7_render_scope rsf_ac7_rust rsf_build_options)
set_target_properties(rsf_game_ac7 PROPERTIES OUTPUT_NAME "ReScaleFrame.Game.AC7")

# The Rust UI rules classify frame-tap facts and check the captured shader fingerprint.
add_library(rsf_ac7_ui_rules INTERFACE)
target_include_directories(rsf_ac7_ui_rules INTERFACE include)
target_link_libraries(rsf_ac7_ui_rules INTERFACE rsf_ac7_rust)
add_library(ReScaleFrame::AC7UIRules ALIAS rsf_ac7_ui_rules)

# Capture-derived scene selection stays with the game; graphics supplies only draw observations.
add_library(rsf_ac7_scene_color INTERFACE)
target_include_directories(rsf_ac7_scene_color INTERFACE include)
target_link_libraries(rsf_ac7_scene_color INTERFACE ReScaleFrame::Graphics rsf_ac7_rust)
add_library(ReScaleFrame::AC7SceneColor ALIAS rsf_ac7_scene_color)

add_library(rsf_ac7_capture_trampolines STATIC src/motion_capture_trampolines.cpp)
target_include_directories(rsf_ac7_capture_trampolines PRIVATE include)
target_link_libraries(rsf_ac7_capture_trampolines PRIVATE ReScaleFrame::GameSDK minhook rsf_build_options)
# Rust owns the public entry points and calls these native effects in return. Load the
# small trampoline archive in full so either side can resolve the other.
add_library(rsf_ac7_motion_capture INTERFACE)
target_include_directories(rsf_ac7_motion_capture INTERFACE include)
target_link_libraries(rsf_ac7_motion_capture INTERFACE
    ReScaleFrame::GameSDK "$<LINK_LIBRARY:WHOLE_ARCHIVE,rsf_ac7_capture_trampolines>"
    rsf_ac7_rust ReScaleFrame::Graphics)
add_library(ReScaleFrame::AC7MotionCapture ALIAS rsf_ac7_motion_capture)

add_library(rsf_ac7_render_scope STATIC src/render_scope.cpp src/native_renderer.cpp src/velocity_shader.cpp)
target_include_directories(rsf_ac7_render_scope PUBLIC include)
target_link_libraries(rsf_ac7_render_scope PUBLIC ReScaleFrame::GameSDK PRIVATE rsf_ac7_view rsf_windows_input_facts minhook rsf_build_options ReScaleFrame::RuntimeCommon)
# The shader compiler is available without linking the Graphics library.
target_link_libraries(rsf_ac7_render_scope PUBLIC rsf_shader_compiler)
target_include_directories(rsf_ac7_render_scope PRIVATE "${CMAKE_SOURCE_DIR}/runtime/graphics/include")
add_library(ReScaleFrame::AC7RenderScope ALIAS rsf_ac7_render_scope)
# The pass keeps its D3D11 work native. Its shader/layout observations and variant admission are
# src/velocity_pass_cache.rs, its model/view history decisions src/velocity_history.rs, its
# draw admission, record bounds and cbuffer packing src/velocity_stream.rs, its commit
# admission, target check, log budgets and CPU profile src/velocity_commit.rs and its per-device
# renderer publication and teardown src/velocity_renderer.rs, so the Rust archive follows the
# render scope on the link line.
add_library(rsf_ac7_velocity_pass STATIC src/velocity_pass.cpp)
target_include_directories(rsf_ac7_velocity_pass PUBLIC include)
target_link_libraries(rsf_ac7_velocity_pass PRIVATE ReScaleFrame::AC7RenderScope ReScaleFrame::Graphics rsf_ac7_rust rsf_build_options)
add_library(ReScaleFrame::AC7VelocityPass ALIAS rsf_ac7_velocity_pass)
