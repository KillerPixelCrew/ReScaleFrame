set(RSF_UNITY_NATIVE_INCLUDE "${CMAKE_SOURCE_DIR}/vendor/unity-native/include" CACHE PATH
    "Unity native-plugin headers; untracked, see docs/dependencies.md")
# The package uses the shared state archive. Only the Mono plugin links the Runtime DLL imports.
rsf_add_rust_static(rsf_unity_mono_rust rsf_rust_unity_mono rsf-unity-mono rsf_unity_mono)
rsf_add_rust_static(rsf_unity_mono_plugin_rust rsf_rust_unity_mono_plugin rsf-unity-mono rsf_unity_mono
    --features mono-plugin)
add_library(rsf_game_unity_mono SHARED src/plugin.def src/bridge.c)
set(unity_mono_exports rsf_get_game_plugin_api rsf_unity_start rsf_unity_fg_start rsf_unity_sr_start rsf_unity_sr_stop
    rsf_unity_get_render_event rsf_unity_get_event_id rsf_unity_set_engine_spatial)
foreach(symbol IN LISTS unity_mono_exports)
    if(MSVC)
        target_link_options(rsf_game_unity_mono PRIVATE "LINKER:/INCLUDE:${symbol}")
    elseif(MINGW)
        target_link_options(rsf_game_unity_mono PRIVATE "LINKER:--undefined=${symbol}")
    endif()
endforeach()
target_include_directories(rsf_game_unity_mono PRIVATE include)
# minhook: the archive's rsf-windows hooks link the adapter's own MinHook instance.
target_link_libraries(rsf_game_unity_mono PRIVATE rsf_unity_mono_plugin_rust ReScaleFrame::GameSDK ReScaleFrame::RuntimeCommon minhook rsf_build_options)
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
