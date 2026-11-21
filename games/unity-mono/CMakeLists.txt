set(RSF_UNITY_NATIVE_INCLUDE "${CMAKE_SOURCE_DIR}/vendor/unity-native/include" CACHE PATH
    "Unity native-plugin headers; untracked, see docs/dependencies.md")
add_library(rsf_game_unity_mono SHARED src/plugin.cpp src/bridge.cpp src/runtime_host.cpp src/runtime_config.cpp)
# Detection, the renderer lifecycle, the Mono helper bootstrap and the D3D12 bridge frame state are
# Rust; plugin.cpp lends the bridge. integrations/unity links the same archive for bridge.cpp.
rsf_add_rust_static(rsf_unity_mono_rust rsf_rust_unity_mono rsf-unity-mono rsf_unity_mono)
target_include_directories(rsf_game_unity_mono PRIVATE include)
target_link_libraries(rsf_game_unity_mono PRIVATE rsf_unity_mono_rust ReScaleFrame::GameSDK ReScaleFrame::RuntimeCommon rsf_build_options)
target_link_libraries(rsf_game_unity_mono PRIVATE rsf_orchestrator)
target_compile_definitions(rsf_game_unity_mono PRIVATE RSF_GAME_UNITY_HOST_BUILD _CRT_SECURE_NO_WARNINGS)
if(EXISTS "${RSF_UNITY_NATIVE_INCLUDE}/IUnityGraphicsD3D12.h")
    target_include_directories(rsf_game_unity_mono SYSTEM PRIVATE "${RSF_UNITY_NATIVE_INCLUDE}")
    target_compile_definitions(rsf_game_unity_mono PRIVATE RSF_HAVE_UNITY_NATIVE=1)
else()
    target_compile_definitions(rsf_game_unity_mono PRIVATE RSF_HAVE_UNITY_NATIVE=0)
endif()
target_link_libraries(rsf_game_unity_mono PRIVATE d3d12)
set_target_properties(rsf_game_unity_mono PROPERTIES
    OUTPUT_NAME "ReScaleFrame.Game.UnityMono")
