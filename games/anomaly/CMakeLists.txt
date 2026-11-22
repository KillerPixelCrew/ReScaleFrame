add_library(rsf_game_anomaly SHARED src/plugin.def src/bootstrap_platform.c src/native_observer_platform.c src/native_hooks_platform.c
    src/native_geometry_platform.c src/engine_slots.c
    src/sr_d3d11_platform.c src/sr_stage_platform.c)
rsf_add_rust_static(rsf_anomaly_rust rsf_rust_anomaly rsf-anomaly rsf_anomaly)
rsf_add_rust_static(rsf_anomaly_plugin_rust rsf_rust_anomaly_plugin rsf-anomaly rsf_anomaly
    --features native-entry)
foreach(symbol IN ITEMS rsf_anomaly_start rsf_anomaly_observation rsf_get_game_plugin_api)
    if(MSVC)
        target_link_options(rsf_game_anomaly PRIVATE "LINKER:/INCLUDE:${symbol}")
    elseif(MINGW)
        target_link_options(rsf_game_anomaly PRIVATE "LINKER:--undefined=${symbol}")
    endif()
endforeach()
target_include_directories(rsf_game_anomaly PRIVATE "${PROJECT_SOURCE_DIR}/runtime/orchestrator/include")
target_link_libraries(rsf_game_anomaly PRIVATE rsf_anomaly_plugin_rust ReScaleFrame::GameSDK ReScaleFrame::Contract ReScaleFrame::RuntimeCommon minhook rsf_build_options)
# Stage 1 uses only the generic immediate-context state scope (d3d11_state) and the texture-dump view enum header.
target_link_libraries(rsf_game_anomaly PRIVATE ReScaleFrame::Graphics ReScaleFrame::GraphicsLeaves d3d11)
set_target_properties(rsf_game_anomaly PROPERTIES OUTPUT_NAME "ReScaleFrame.Game.Anomaly")
# The C detour leaves retire their scopes during a C++ unwind: __finally under MSVC, cleanup handlers under GCC.
target_compile_options(rsf_game_anomaly PRIVATE $<$<AND:$<COMPILE_LANGUAGE:C>,$<NOT:$<C_COMPILER_ID:MSVC>>>:-fexceptions>)

# Motion MRT (src/motion_stage.h): verified shader variants at create_shader, b13 packers, RTV2 binder, draw-time
# history and the invalidation replay. Inert unless [Anomaly] Motion=1.
target_sources(rsf_game_anomaly PRIVATE src/motion_stage_platform.c)
target_link_libraries(rsf_game_anomaly PRIVATE bcrypt dxguid)
find_package(Python3 REQUIRED COMPONENTS Interpreter)
file(GLOB_RECURSE RSF_ANOMALY_PATCH_INPUTS CONFIGURE_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/patches/motion/*")
add_custom_target(rsf_anomaly_patch_pack ALL
    COMMAND "${Python3_EXECUTABLE}" "${PROJECT_SOURCE_DIR}/tools/rsf_shader_pack.py" build
        "${CMAKE_CURRENT_SOURCE_DIR}/patches/motion" "$<TARGET_FILE_DIR:rsf_game_anomaly>/patches/motion.rsfpatch"
    DEPENDS ${RSF_ANOMALY_PATCH_INPUTS} "${PROJECT_SOURCE_DIR}/tools/rsf_shader_pack.py"
    COMMENT "Building the rules-only Anomaly shader pack" VERBATIM)

target_include_directories(rsf_game_anomaly PRIVATE "${PROJECT_SOURCE_DIR}/runtime/backends/dlss/include")

# Frame generation (src/fg_stage.h): the Runtime's shared D3D11 presentation bridge and FG tagging path through
# rsf_get_d3d11_present_api and rsf_get_d3d11_fg_api. Inert unless [Anomaly] FG=1 or FGBridge=1.
target_sources(rsf_game_anomaly PRIVATE src/fg_stage_platform.c)
# The FG tables name presentation-bridge and Streamline-host types; include paths only, nothing is linked.
target_include_directories(rsf_game_anomaly PRIVATE "$<TARGET_PROPERTY:rsf_orchestrator,INTERFACE_INCLUDE_DIRECTORIES>")
