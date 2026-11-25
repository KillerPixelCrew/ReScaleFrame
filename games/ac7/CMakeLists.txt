# The carriers' Rust (rsf-ac7: view reader, carrier policy, motion capture, velocity pass). It holds
# no reference to the renderer's MinHook, render scopes or engine relays. The carrier feature adds the
# Runtime (with the Runtime archive's features) and the loader the carrier proxy embeds, so the proxy
# links this one archive.
rsf_add_rust_static(rsf_ac7_rust rsf_rust_ac7 rsf-ac7 rsf_ac7 --features carrier
    CARRIES "rsf_orchestrator_rust;rsf_common_rust;rsf_graphics_rust;rsf_loader_rust")
# The crate's C leaves (rsf_ac7_leaves below), so every consumer of an archive that carries rsf-ac7
# links them. The D3D11 leaves it calls come from the archive that carries it: the embedded Runtime
# in the carrier archive, rsf-graphics in the adapter and view fixture archives.
target_link_libraries(rsf_ac7_rust INTERFACE rsf_ac7_leaves)
target_include_directories(rsf_ac7_rust INTERFACE include)
add_library(ReScaleFrame::AC7View ALIAS rsf_ac7_rust)

# The adapter DLL's Rust (rsf-ac7-plugin, games/ac7/plugin, which carries the rsf-ac7 code it uses,
# rsf-graphics and the rsf-windows input facts): exact-build identity, the GameSDK table, the
# renderer lifecycle and the native renderer (code-site admission, patch transactions, hook guards,
# the prepare/start/quiesce/stop transaction, TrueSky patches and relays, the renderer object,
# detours and pass leases). The detours call MinHook through rsf-minhook-sys; the Windows input
# facts, the render scope C create calls and the shader factory leaf come from the call table of
# src/native_renderer_platform.c (in the render scope library), which plugin.c and the native graph
# fixture bind at run time. The crate also owns the queued render scopes and exports
# ac7_render_scope.h except its two C create calls, which pass the C11 SEH exception leaves of
# render_scope_entry.c (src/ac7_scope_owner.h). Every target that calls that header links this
# archive (or one that carries the crate) after rsf_ac7_render_scope, which links none: this DLL
# and the render scope, D3D11 present bridge and native graph fixtures.
rsf_add_rust_static(rsf_ac7_plugin_rust rsf_rust_ac7_plugin rsf-ac7-plugin rsf_ac7_plugin CARRIES rsf_graphics_rust)
target_link_libraries(rsf_ac7_plugin_rust INTERFACE rsf_ac7_leaves ReScaleFrame::Graphics minhook bcrypt)
target_include_directories(rsf_ac7_plugin_rust INTERFACE include)
add_library(rsf_game_ac7 SHARED src/plugin.c)
set_target_properties(rsf_game_ac7 PROPERTIES LINKER_LANGUAGE CXX)
target_link_libraries(rsf_game_ac7 PRIVATE ReScaleFrame::GameSDK rsf_ac7_render_scope rsf_ac7_plugin_rust
    minhook rsf_build_options)
set_target_properties(rsf_game_ac7 PROPERTIES OUTPUT_NAME "ReScaleFrame.Game.AC7")

# The velocity replay stays with the game; graphics supplies only draw observations and the shared
# binding-state lease.
add_library(rsf_ac7_graphics INTERFACE)
target_link_libraries(rsf_ac7_graphics INTERFACE ReScaleFrame::Graphics rsf_ac7_rust)
add_library(ReScaleFrame::AC7VelocityPass ALIAS rsf_ac7_graphics)

add_library(rsf_ac7_memory STATIC src/ac7_memory.c)
target_include_directories(rsf_ac7_memory PUBLIC include)
target_link_libraries(rsf_ac7_memory PRIVATE ReScaleFrame::GameSDK rsf_build_options)

add_library(rsf_ac7_capture_trampolines STATIC
    src/motion_capture_trampolines.c src/ac7_hooks.c)
target_include_directories(rsf_ac7_capture_trampolines PRIVATE include)
target_link_libraries(rsf_ac7_capture_trampolines PRIVATE ReScaleFrame::GameSDK rsf_ac7_memory minhook rsf_build_options)
# The C leaves rsf-ac7 calls: the SEH memory reads and the motion capture trampolines. Rust owns the
# capture's public entry points and calls the trampolines in return, so the small trampoline archive
# loads in full and either side can resolve the other.
add_library(rsf_ac7_leaves INTERFACE)
target_link_libraries(rsf_ac7_leaves INTERFACE rsf_ac7_memory
    "$<LINK_LIBRARY:WHOLE_ARCHIVE,rsf_ac7_capture_trampolines>")
add_library(rsf_ac7_motion_capture INTERFACE)
target_include_directories(rsf_ac7_motion_capture INTERFACE include)
target_link_libraries(rsf_ac7_motion_capture INTERFACE ReScaleFrame::GameSDK rsf_ac7_rust ReScaleFrame::Graphics)
add_library(ReScaleFrame::AC7MotionCapture ALIAS rsf_ac7_motion_capture)

add_library(rsf_ac7_render_scope STATIC src/render_scope_entry.c src/native_renderer_platform.c)
target_include_directories(rsf_ac7_render_scope PUBLIC include)
target_link_libraries(rsf_ac7_render_scope PUBLIC ReScaleFrame::GameSDK PRIVATE rsf_ac7_memory rsf_build_options ReScaleFrame::RuntimeCommon)

target_include_directories(rsf_ac7_render_scope PRIVATE "${CMAKE_SOURCE_DIR}/runtime/graphics/include")
add_library(ReScaleFrame::AC7RenderScope ALIAS rsf_ac7_render_scope)

# The AC7 carrier. Its file name is the mechanism, so it is deliberately not ReScaleFrame.*.
# It keeps the DirectInput ABI and AC7's original entry path, and links the Runtime and the
# game's view buffer reader rather than loading them, so getting into the game stays one added file.
add_library(rsf_proxy_ac7 SHARED carrier/dinput8_proxy.c carrier/dlss_bridge.c)
target_link_libraries(rsf_proxy_ac7 PRIVATE
    ReScaleFrame::LoaderDiagnostics
    ReScaleFrame::Graphics
    ReScaleFrame::OrchestratorCore
    ReScaleFrame::AC7View
    ReScaleFrame::AC7MotionCapture
    ReScaleFrame::AC7VelocityPass
    bcrypt
    rsf_build_options)
target_compile_definitions(rsf_proxy_ac7 PRIVATE _CRT_SECURE_NO_WARNINGS)
set_target_properties(rsf_proxy_ac7 PROPERTIES OUTPUT_NAME "dinput8" PREFIX ""
    RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/ac7/bin/$<CONFIG>"
    ARCHIVE_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/ac7/lib/$<CONFIG>"
    PDB_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/ac7/bin/$<CONFIG>")
add_dependencies(rsf_proxy_ac7 rsf_game_ac7)
if(MSVC)
    target_sources(rsf_proxy_ac7 PRIVATE carrier/ac7_exports.def)
    # Optimized builds need exact private-function addresses for capture crash analysis.
    target_link_options(rsf_proxy_ac7 PRIVATE "/MAP:$<TARGET_FILE_DIR:rsf_proxy_ac7>/dinput8.map")
endif()
