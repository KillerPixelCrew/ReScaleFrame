# The plugin and linked carriers call the same Rust reader through the existing view C ABI.
rsf_add_rust_static(rsf_ac7_rust rsf_rust_ac7 rsf-ac7 rsf_ac7)
# The crate's guarded engine reads are the SEH leaf rsf_ac7_memory, so every consumer of the
# archive links it. The D3D11
# leaves it calls come from ReScaleFrame::GraphicsLeaves or, in carriers and fixtures that link the
# Runtime archive, from that archive; each binary links exactly one of the two.
target_link_libraries(rsf_ac7_rust INTERFACE rsf_ac7_memory)
target_include_directories(rsf_ac7_rust INTERFACE include)
add_library(ReScaleFrame::AC7View ALIAS rsf_ac7_rust)

# Exact-build identity, the GameSDK table and the renderer lifecycle are the Rust crate rsf-ac7
# (src/lib.rs, src/lifecycle.rs). plugin.c keeps the export and lends the renderer leaf snapshot
# through src/ac7_platform.h. The crate also owns the native renderer: its code sites, patch
# transactions, hook guards, prepare/start/quiesce/stop transaction, TrueSky patches and relays
# (src/sites.rs, patches.rs, guards.rs, renderer.rs, truesky.rs) and the renderer object, detours
# and pass leases (src/native_renderer.rs). That module reaches MinHook, the Windows input facts,
# the render scope C create call and the shader factory leaf only through the call table of
# src/native_renderer_platform.c (in the render scope library), which plugin.c and the native graph
# fixture bind at run time, so no other archive consumer needs those libraries. Only this DLL and
# the native graph fixture call the native renderer.
# The crate also owns the queued render scopes (src/render_scope.rs) and exports
# ac7_render_scope.h except its two C create calls, which pass the C11 SEH exception leaves
# of render_scope_entry.c (src/ac7_scope_owner.h). Every target that calls that header links
# rsf_ac7_rust (or the fixture archive) after rsf_ac7_render_scope, which links neither: this DLL and
# the render scope, D3D11 present bridge and native graph fixtures. The carriers use the crate through AC7View without pulling render_scope_entry.c.
add_library(rsf_game_ac7 SHARED src/plugin.c)
set_target_properties(rsf_game_ac7 PROPERTIES LINKER_LANGUAGE CXX)
target_link_libraries(rsf_game_ac7 PRIVATE ReScaleFrame::GameSDK rsf_ac7_render_scope rsf_ac7_rust
    ReScaleFrame::GraphicsLeaves rsf_build_options)
set_target_properties(rsf_game_ac7 PROPERTIES OUTPUT_NAME "ReScaleFrame.Game.AC7")

# The velocity replay stays with the game; graphics supplies only draw observations and the shared
# binding-state lease.
add_library(rsf_ac7_graphics INTERFACE)
target_link_libraries(rsf_ac7_graphics INTERFACE ReScaleFrame::Graphics rsf_ac7_rust)
add_library(ReScaleFrame::AC7VelocityPass ALIAS rsf_ac7_graphics)

add_library(rsf_ac7_memory STATIC src/ac7_memory.c)
target_include_directories(rsf_ac7_memory PUBLIC include)
target_link_libraries(rsf_ac7_memory PRIVATE rsf_build_options)

add_library(rsf_ac7_capture_trampolines STATIC
    src/motion_capture_trampolines.c src/ac7_hooks.c src/ac7_capture_io.c)
target_include_directories(rsf_ac7_capture_trampolines PRIVATE include)
target_link_libraries(rsf_ac7_capture_trampolines PRIVATE ReScaleFrame::GameSDK rsf_ac7_memory minhook rsf_build_options)
# Rust owns the public entry points and calls these native effects in return. Load the
# small trampoline archive in full so either side can resolve the other.
add_library(rsf_ac7_motion_capture INTERFACE)
target_include_directories(rsf_ac7_motion_capture INTERFACE include)
target_link_libraries(rsf_ac7_motion_capture INTERFACE
    ReScaleFrame::GameSDK "$<LINK_LIBRARY:WHOLE_ARCHIVE,rsf_ac7_capture_trampolines>"
    rsf_ac7_rust ReScaleFrame::Graphics)
add_library(ReScaleFrame::AC7MotionCapture ALIAS rsf_ac7_motion_capture)

add_library(rsf_ac7_render_scope STATIC src/render_scope_entry.c src/native_renderer_platform.c)
target_include_directories(rsf_ac7_render_scope PUBLIC include)
target_link_libraries(rsf_ac7_render_scope PUBLIC ReScaleFrame::GameSDK PRIVATE rsf_ac7_memory rsf_windows_input_facts minhook rsf_build_options ReScaleFrame::RuntimeCommon)

target_include_directories(rsf_ac7_render_scope PRIVATE "${CMAKE_SOURCE_DIR}/runtime/graphics/include")
add_library(ReScaleFrame::AC7RenderScope ALIAS rsf_ac7_render_scope)
