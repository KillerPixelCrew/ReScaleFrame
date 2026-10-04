# Research carrier DLL. Its file name is the mechanism, so it is deliberately not ReScaleFrame.*.
add_library(rsf_proxy_dinput8 SHARED
    src/dinput8_proxy.c
    src/dlss_bridge.c
    src/preferences.c)
target_link_libraries(rsf_proxy_dinput8 PRIVATE
    ReScaleFrame::LoaderDiagnostics
    ReScaleFrame::Graphics
    # The backend path and the game's view buffer reader, linked in rather than loaded, so getting
    # into the game stays one added file. Both belong on the other side of the ownership line in
    # AGENTS.md and are here because this is what can be loaded today.
    ReScaleFrame::OrchestratorCore
    ReScaleFrame::AC7SceneColor
    ReScaleFrame::AC7UIRules
    ReScaleFrame::AC7View
    ReScaleFrame::AC7MotionCapture
    bcrypt
    rsf_build_options)
target_compile_definitions(rsf_proxy_dinput8 PRIVATE _CRT_SECURE_NO_WARNINGS)
set_target_properties(rsf_proxy_dinput8 PROPERTIES OUTPUT_NAME "dinput8" PREFIX "")
add_dependencies(rsf_proxy_dinput8 rsf_game_ac7)
if(MSVC)
    enable_language(ASM_MASM)
    target_sources(rsf_proxy_dinput8 PRIVATE src/shim_loader.cpp src/version_forwarders.asm src/shim_exports.def)
    foreach(config Debug Release RelWithDebInfo MinSizeRel)
        file(MAKE_DIRECTORY "${CMAKE_CURRENT_BINARY_DIR}/rsf_proxy_dinput8.dir/${config}/src")
    endforeach()
    add_custom_command(TARGET rsf_proxy_dinput8 POST_BUILD
        COMMAND ${CMAKE_COMMAND} -E make_directory "$<TARGET_FILE_DIR:rsf_proxy_dinput8>/carriers"
        COMMAND ${CMAKE_COMMAND} -E copy_if_different "$<TARGET_FILE:rsf_proxy_dinput8>"
            "$<TARGET_FILE_DIR:rsf_proxy_dinput8>/ReScaleFrame.Loader.dll"
        COMMAND ${CMAKE_COMMAND} -E copy_if_different "$<TARGET_FILE:rsf_proxy_dinput8>"
            "$<TARGET_FILE_DIR:rsf_proxy_dinput8>/carriers/version.dll")
    # Optimized builds need exact private-function addresses for capture crash analysis.
    target_link_options(rsf_proxy_dinput8 PRIVATE
        "/MAP:$<TARGET_FILE_DIR:rsf_proxy_dinput8>/dinput8.map")
endif()
if(NOT MSVC)
    # Existing cross-build carrier retains its AC7 entry until the x64 assembler forwarding
    # surface is ported. The shared Windows/MSVC binary contains both carrier export surfaces.
    target_compile_definitions(rsf_proxy_dinput8 PRIVATE rsf_ac7_proxy_process_event=DllMain)
endif()

# AC7's validated carrier keeps the DirectInput ABI and original entry path. The generic
# version shim has a different ordinal contract and can evolve independently.
add_library(rsf_proxy_ac7 SHARED src/dinput8_proxy.c src/dlss_bridge.c src/preferences.c)
get_target_property(rsf_ac7_proxy_links rsf_proxy_dinput8 LINK_LIBRARIES)
target_link_libraries(rsf_proxy_ac7 PRIVATE ${rsf_ac7_proxy_links})
target_compile_definitions(rsf_proxy_ac7 PRIVATE _CRT_SECURE_NO_WARNINGS rsf_ac7_proxy_process_event=DllMain)
set_target_properties(rsf_proxy_ac7 PROPERTIES OUTPUT_NAME "dinput8" PREFIX ""
    RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/ac7/bin/$<CONFIG>"
    ARCHIVE_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/ac7/lib/$<CONFIG>"
    PDB_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/ac7/bin/$<CONFIG>")
add_dependencies(rsf_proxy_ac7 rsf_game_ac7)
if(MSVC)
    target_sources(rsf_proxy_ac7 PRIVATE src/ac7_exports.def)
    target_link_options(rsf_proxy_ac7 PRIVATE "/MAP:$<TARGET_FILE_DIR:rsf_proxy_ac7>/dinput8.map")
endif()
