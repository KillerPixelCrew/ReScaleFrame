# Native Game SDK/Mono bridge builds with or without the optional licensed Unity headers.
# Without D3D12 plugin headers, runtime rendering discovery refuses rather than fabricating support.
set(RSF_UNITY_NATIVE_INCLUDE "${CMAKE_SOURCE_DIR}/vendor/unity-native/include" CACHE PATH
    "Unity native-plugin headers; untracked, see docs/dependencies.md")
add_library(rsf_game_unity_mono SHARED src/plugin.cpp src/bridge.cpp src/mono_runtime.cpp)
target_include_directories(rsf_game_unity_mono PRIVATE include)
target_link_libraries(rsf_game_unity_mono PRIVATE ReScaleFrame::GameSDK rsf_build_options)
if(EXISTS "${RSF_UNITY_NATIVE_INCLUDE}/IUnityGraphicsD3D12.h")
    target_include_directories(rsf_game_unity_mono SYSTEM PRIVATE "${RSF_UNITY_NATIVE_INCLUDE}")
    target_compile_definitions(rsf_game_unity_mono PRIVATE RSF_HAVE_UNITY_NATIVE=1)
else()
    target_compile_definitions(rsf_game_unity_mono PRIVATE RSF_HAVE_UNITY_NATIVE=0)
endif()
target_link_libraries(rsf_game_unity_mono PRIVATE d3d12 bcrypt)
set_target_properties(rsf_game_unity_mono PROPERTIES
    OUTPUT_NAME "ReScaleFrame.Game.UnityMono")
