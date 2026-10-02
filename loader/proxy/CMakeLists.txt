# Research carrier DLL. Its file name is the mechanism, so it is deliberately not ReScaleFrame.*.
add_library(rsf_proxy_dinput8 SHARED
    src/dinput8_proxy.c
    src/dlss_bridge.c
    src/preferences.c
    # C++ because it is the one part of the bridge that talks to COM directly.
    src/overlay_host.cpp)
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
    # Optimized builds need exact private-function addresses for capture crash analysis.
    target_link_options(rsf_proxy_dinput8 PRIVATE
        "/MAP:$<TARGET_FILE_DIR:rsf_proxy_dinput8>/dinput8.map")
endif()
