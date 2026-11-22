# The AC7 carrier. Its file name is the mechanism, so it is deliberately not ReScaleFrame.*.
# It keeps the DirectInput ABI and AC7's original entry path, and links the backend path and the
# game's view buffer reader rather than loading them, so getting into the game stays one added
# file. Both belong on the other side of the ownership line in AGENTS.md and are here because this
# is what can be loaded today.
add_library(rsf_proxy_ac7 SHARED src/dinput8_proxy.c src/dlss_bridge.c)
target_link_libraries(rsf_proxy_ac7 PRIVATE
    ReScaleFrame::LoaderDiagnostics
    ReScaleFrame::Graphics
    ReScaleFrame::OrchestratorCore
    ReScaleFrame::AC7SceneColor
    ReScaleFrame::AC7UIRules
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
    target_sources(rsf_proxy_ac7 PRIVATE src/ac7_exports.def)
    # Optimized builds need exact private-function addresses for capture crash analysis.
    target_link_options(rsf_proxy_ac7 PRIVATE "/MAP:$<TARGET_FILE_DIR:rsf_proxy_ac7>/dinput8.map")
endif()

if(MSVC)
    enable_language(ASM_MASM)

    # Shared version.dll shim (Unity drop-in carrier): version and DirectInput forwarding plus the
    # configured entry. It has a different ordinal contract from the AC7 carrier.
    add_library(rsf_proxy_dinput8 SHARED
        src/shim_entry.c src/version_forwarders.asm src/shim_exports.def)
    target_link_libraries(rsf_proxy_dinput8 PRIVATE rsf_loader_rust bcrypt rsf_build_options)
    set_target_properties(rsf_proxy_dinput8 PROPERTIES OUTPUT_NAME "dinput8" PREFIX "")
    add_custom_command(TARGET rsf_proxy_dinput8 POST_BUILD
        COMMAND ${CMAKE_COMMAND} -E make_directory "$<TARGET_FILE_DIR:rsf_proxy_dinput8>/carriers"
        COMMAND ${CMAKE_COMMAND} -E copy_if_different "$<TARGET_FILE:rsf_proxy_dinput8>"
            "$<TARGET_FILE_DIR:rsf_proxy_dinput8>/ReScaleFrame.Loader.dll"
        COMMAND ${CMAKE_COMMAND} -E copy_if_different "$<TARGET_FILE:rsf_proxy_dinput8>"
            "$<TARGET_FILE_DIR:rsf_proxy_dinput8>/carriers/version.dll")

    add_library(rsf_proxy_plugin SHARED
        src/plugin_carrier.c src/xinput_forwarders.asm src/xinput_exports.def)
    target_link_libraries(rsf_proxy_plugin PRIVATE
        rsf_loader_rust bcrypt rsf_build_options)
    set_target_properties(rsf_proxy_plugin PROPERTIES OUTPUT_NAME "xinput1_3" PREFIX ""
        RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/plugin-carrier/bin/$<CONFIG>"
        ARCHIVE_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/plugin-carrier/lib/$<CONFIG>"
        PDB_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/plugin-carrier/bin/$<CONFIG>")
    foreach(config Debug Release RelWithDebInfo MinSizeRel)
        file(MAKE_DIRECTORY "${CMAKE_CURRENT_BINARY_DIR}/rsf_proxy_dinput8.dir/${config}/src")
        file(MAKE_DIRECTORY "${CMAKE_CURRENT_BINARY_DIR}/rsf_proxy_plugin.dir/${config}/src")
    endforeach()
endif()

# Standalone DirectInput carrier: OS forwarding and configured DLL entry only.
# Do not inherit the combined proxy's object libraries or dependency graph.
add_library(rsf_proxy_thin_dinput8 SHARED
    src/thin_entry.c src/thin_exports.def)
target_link_libraries(rsf_proxy_thin_dinput8 PRIVATE rsf_loader_rust rsf_build_options)
if(MSVC)
    # COM exports intentionally retain the system DirectInput ordinal contract.
    target_link_options(rsf_proxy_thin_dinput8 PRIVATE /INCREMENTAL:NO /OPT:REF /IGNORE:4222 "/PDBALTPATH:%_PDB%")
endif()
set_target_properties(rsf_proxy_thin_dinput8 PROPERTIES OUTPUT_NAME "dinput8" PREFIX ""
    RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/thin/bin/$<CONFIG>"
    ARCHIVE_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/thin/lib/$<CONFIG>"
    PDB_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/thin/bin/$<CONFIG>")
