# The C leaves of the adapter: the SEH trampolines and detours, the guarded engine calls and copies, the hook sets and
# the D3D11 pipeline-state snapshot. Rust names them directly, so every binary that links Rust from rsf-anomaly
# compiles them in: the plugin below and, through rsf_anomaly_fixtures_rust, each fixture executable
# (tests/CMakeLists.txt, anomaly_support.cmake, anomaly_fg.cmake, overlay_dll_api.cmake). An interface library, so the
# sources build with each consumer's definitions (rsf_anomaly_geometry_hooks_test sets RSF_ANOMALY_GEOMETRY_TESTING).
# The D3D11 state leaves they call come from rsf-graphics, which every rsf-anomaly archive carries, so no consumer
# links ReScaleFrame::GraphicsLeaves beside it.
add_library(rsf_anomaly_native_leaves INTERFACE)
target_sources(rsf_anomaly_native_leaves INTERFACE
    "${CMAKE_CURRENT_SOURCE_DIR}/src/native_observer_platform.c"
    "${CMAKE_CURRENT_SOURCE_DIR}/src/native_hooks_platform.c"
    "${CMAKE_CURRENT_SOURCE_DIR}/src/native_geometry_platform.c"
    "${CMAKE_CURRENT_SOURCE_DIR}/src/engine_slots.c"
    "${CMAKE_CURRENT_SOURCE_DIR}/src/sr_d3d11_platform.c"
    "${CMAKE_CURRENT_SOURCE_DIR}/src/sr_stage_platform.c"
    "${CMAKE_CURRENT_SOURCE_DIR}/src/motion_stage_platform.c"
    "${CMAKE_CURRENT_SOURCE_DIR}/src/fg_stage_platform.c")
target_include_directories(rsf_anomaly_native_leaves INTERFACE
    "${CMAKE_CURRENT_SOURCE_DIR}/src"
    "${PROJECT_SOURCE_DIR}/runtime/orchestrator/include"
    "${PROJECT_SOURCE_DIR}/runtime/backends/dlss/include"
    # The FG leaf names presentation-bridge and Streamline-host types; include paths only, nothing is linked.
    "$<TARGET_PROPERTY:rsf_orchestrator,INTERFACE_INCLUDE_DIRECTORIES>")
target_link_libraries(rsf_anomaly_native_leaves INTERFACE
    ReScaleFrame::GameSDK ReScaleFrame::Contract ReScaleFrame::RuntimeCommon
    ReScaleFrame::Graphics minhook d3d11 bcrypt dxguid)
# The C detour leaves retire their scopes during a C++ unwind: __finally under MSVC, cleanup handlers under GCC.
target_compile_options(rsf_anomaly_native_leaves INTERFACE
    $<$<AND:$<COMPILE_LANGUAGE:C>,$<NOT:$<C_COMPILER_ID:MSVC>>>:-fexceptions>)

# The CInput detour calls the native-entry bootstrap, so it is the plugin's own.
add_library(rsf_game_anomaly SHARED src/plugin.def src/bootstrap_platform.c)
rsf_add_rust_static(rsf_anomaly_plugin_rust rsf_rust_anomaly_plugin rsf-anomaly rsf_anomaly
    --features native-entry CARRIES rsf_graphics_rust)
# Binaries that carry rsf-anomaly bind Common Controls 6 (comctl6.manifest); the DLL's own manifest
# governs its imports whatever the host process declares.
if(MSVC)
    target_sources(rsf_anomaly_plugin_rust INTERFACE "${CMAKE_CURRENT_SOURCE_DIR}/comctl6.manifest")
endif()
foreach(symbol IN ITEMS rsf_anomaly_start rsf_anomaly_observation rsf_get_game_plugin_api)
    if(MSVC)
        target_link_options(rsf_game_anomaly PRIVATE "LINKER:/INCLUDE:${symbol}")
    elseif(MINGW)
        target_link_options(rsf_game_anomaly PRIVATE "LINKER:--undefined=${symbol}")
    endif()
endforeach()
target_link_libraries(rsf_game_anomaly PRIVATE rsf_anomaly_plugin_rust rsf_anomaly_native_leaves rsf_build_options)
set_target_properties(rsf_game_anomaly PROPERTIES OUTPUT_NAME "ReScaleFrame.Game.Anomaly")

# Motion MRT (src/motion_stage.h): verified shader variants at create_shader, b13 packers, RTV2 binder, draw-time
# history and the invalidation replay. Inert unless [Anomaly] Motion=1.
find_package(Python3 REQUIRED COMPONENTS Interpreter)
file(GLOB_RECURSE RSF_ANOMALY_PATCH_INPUTS CONFIGURE_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/patches/motion/*")
add_custom_target(rsf_anomaly_patch_pack ALL
    COMMAND "${Python3_EXECUTABLE}" "${PROJECT_SOURCE_DIR}/tools/rsf_shader_pack.py" build
        "${CMAKE_CURRENT_SOURCE_DIR}/patches/motion" "$<TARGET_FILE_DIR:rsf_game_anomaly>/patches/motion.rsfpatch"
    DEPENDS ${RSF_ANOMALY_PATCH_INPUTS} "${PROJECT_SOURCE_DIR}/tools/rsf_shader_pack.py"
    COMMENT "Building the rules-only Anomaly shader pack" VERBATIM)
