if(EXISTS "${RSF_UNITY_NATIVE_INCLUDE}/IUnityGraphicsD3D12.h")
    find_package(Vulkan REQUIRED)
    # The package's Rust state (src/, the rsf-unity crate; the Mono plugin bundles its subset).
    rsf_add_rust_static(rsf_unity_rust rsf_rust_unity rsf-unity rsf_unity)
    add_library(rsf_unity_package_bridge OBJECT native/render_bridge.c)
    target_include_directories(rsf_unity_package_bridge PRIVATE include)
    target_include_directories(rsf_unity_package_bridge SYSTEM PRIVATE "${RSF_UNITY_NATIVE_INCLUDE}")
    target_compile_definitions(rsf_unity_package_bridge PRIVATE RSF_HAVE_UNITY_NATIVE=1 RSF_UNITY_PACKAGE_BRIDGE)
    target_link_libraries(rsf_unity_package_bridge PRIVATE ReScaleFrame::GameSDK ReScaleFrame::RuntimeCommon rsf_build_options)
    # The package's other C leaves, as objects: the package DLL links them all (UnityPluginLoad is among them),
    # and the Unity SR D3D12 fixture links them beside the rsf-unity code that calls them.
    add_library(rsf_unity_package_leaves OBJECT native/package.c native/bridge.c native/vulkan_bridge.c)
    target_compile_definitions(rsf_unity_package_leaves PRIVATE VK_USE_PLATFORM_WIN32_KHR RSF_STREAMLINE_HOST_BUILD)
    # The public header pin has no D3D11 swapchain getter; some provisioned headers declare it.
    include(CheckCSourceCompiles)
    set(CMAKE_REQUIRED_INCLUDES "${RSF_UNITY_NATIVE_INCLUDE}")
    check_c_source_compiles([[
#include <windows.h>
#include <d3d11.h>
#include <IUnityInterface.h>
#include <IUnityGraphicsD3D11.h>
int main(void) { IUnityGraphicsD3D11* unity = 0; return unity && unity->GetSwapChain ? 1 : 0; }
]] RSF_UNITY_D3D11_HAS_SWAPCHAIN)
    unset(CMAKE_REQUIRED_INCLUDES)
    if(RSF_UNITY_D3D11_HAS_SWAPCHAIN)
        target_compile_definitions(rsf_unity_package_leaves PRIVATE RSF_UNITY_D3D11_SWAPCHAIN=1)
    endif()
    target_include_directories(rsf_unity_package_leaves SYSTEM PRIVATE "${Vulkan_INCLUDE_DIRS}")
    target_include_directories(rsf_unity_package_leaves PRIVATE native include)
    target_include_directories(rsf_unity_package_leaves SYSTEM PRIVATE "${RSF_UNITY_NATIVE_INCLUDE}")
    target_link_libraries(rsf_unity_package_leaves PRIVATE rsf_orchestrator_core ReScaleFrame::RuntimeCommon rsf_build_options
        PUBLIC Vulkan::Vulkan d3d12 d3d11)
    add_library(rsf_unity_package SHARED native/bridge.def $<TARGET_OBJECTS:rsf_unity_package_leaves>
        $<TARGET_OBJECTS:rsf_unity_package_bridge>)
    target_link_libraries(rsf_unity_package PRIVATE Vulkan::Vulkan)
    if(MSVC)
        target_link_libraries(rsf_unity_package PRIVATE delayimp)
        target_link_options(rsf_unity_package PRIVATE /DELAYLOAD:vulkan-1.dll)
    endif()
    # The package's native sources are C11 leaves: Unity SDK calls and foreign exception guards.
    # native/unity_sdk.h keeps each leaf's copy of the Unity interface
    # GUIDs distinct so they link into one module. The shared D3D12 bridge keeps its frame state in
    # the rsf-unity archive (src/bridge_state.rs), whose mono-plugin subset the game plugin bundles; the D3D11
    # and Vulkan bridges use that packet store and policy and each use a Runtime submission ring.
    # d3d11_state.rs owns the D3D11 graphics record and command slots, d3d11_pass.rs builds common
    # scalar pass fields, and vulkan_state.rs and vulkan_bridge.rs own Vulkan admission, routing and
    # lifecycle. Settings, status, session state, SR views, presentation and session services live
    # there too, [UnitySR] paths included (package_state.rs, package_views.rs,
    # package_state/presentation.rs, package_state/services.rs, runtime_config.rs, via
    # native/package_state.h). The game plugin's Runtime setup imports the Runtime directly. The package's Rust Editor preview links directly to the embedded Runtime presenter.
    target_link_libraries(rsf_unity_package PRIVATE rsf_orchestrator_core rsf_unity_rust ReScaleFrame::RuntimeCommon rsf_build_options d3d12 d3d11)
    set_target_properties(rsf_unity_package PROPERTIES OUTPUT_NAME "ReScaleFrame.Unity.Native")
endif()
