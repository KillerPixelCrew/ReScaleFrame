if(EXISTS "${RSF_UNITY_NATIVE_INCLUDE}/IUnityGraphicsD3D12.h")
    find_package(Vulkan REQUIRED)
    add_library(rsf_unity_package_bridge OBJECT ../../games/unity-mono/src/bridge.cpp)
    target_include_directories(rsf_unity_package_bridge PRIVATE ../../games/unity-mono/include)
    target_include_directories(rsf_unity_package_bridge SYSTEM PRIVATE "${RSF_UNITY_NATIVE_INCLUDE}")
    target_compile_definitions(rsf_unity_package_bridge PRIVATE RSF_HAVE_UNITY_NATIVE=1 RSF_UNITY_PACKAGE_BRIDGE)
    target_link_libraries(rsf_unity_package_bridge PRIVATE ReScaleFrame::GameSDK ReScaleFrame::RuntimeCommon rsf_build_options)
    add_library(rsf_unity_package SHARED native/bridge.def native/package.cpp native/bridge.cpp native/vulkan_bridge.cpp
        $<TARGET_OBJECTS:rsf_unity_package_bridge>)
    target_compile_definitions(rsf_unity_package PRIVATE VK_USE_PLATFORM_WIN32_KHR RSF_STREAMLINE_HOST_BUILD)
    target_include_directories(rsf_unity_package SYSTEM PRIVATE "${Vulkan_INCLUDE_DIRS}")
    target_link_libraries(rsf_unity_package PRIVATE Vulkan::Vulkan)
    if(MSVC)
        target_link_libraries(rsf_unity_package PRIVATE delayimp)
        target_link_options(rsf_unity_package PRIVATE /DELAYLOAD:vulkan-1.dll)
    endif()
    target_include_directories(rsf_unity_package PRIVATE native ../../games/unity-mono/include ../../games/unity-mono/src)
    target_include_directories(rsf_unity_package SYSTEM PRIVATE "${RSF_UNITY_NATIVE_INCLUDE}")
    # The shared D3D12 bridge keeps its frame state in the rsf-unity-mono archive (bridge_state.rs),
    # the same archive the game plugin links. native/bridge.cpp's D3D11 and Vulkan bridge uses that
    # packet store and policy. D3D11 and Vulkan each use a Runtime submission ring.
    # d3d11_state.rs owns the D3D11 graphics record and command slots, d3d11_pass.rs builds
    # common scalar pass fields, and vulkan_state.rs owns Vulkan admission and lifecycle. package.cpp keeps its settings, status, session
    # state, SR views, presentation and session services there, [UnitySR] paths included
    # (package_state.rs, package_views.rs, package_state/presentation.rs,
    # package_state/services.rs, runtime_config.rs, via native/package_state.h). Only those symbols are
    # referenced here; the game plugin's Runtime setup reaches the Runtime through its own native
    # leaves. The package's Rust Editor preview links directly to the embedded Runtime presenter.
    target_link_libraries(rsf_unity_package PRIVATE rsf_orchestrator_core rsf_unity_mono_rust ReScaleFrame::RuntimeCommon rsf_build_options d3d12 d3d11)
    set_target_properties(rsf_unity_package PROPERTIES OUTPUT_NAME "ReScaleFrame.Unity.Native")
endif()
