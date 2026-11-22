# Runtime control, settings, session, status, Vulkan creation, compositor effect state and the
# GDExtension classes are Rust (native/lib.rs, crate rsf-godot). The archive is built only for a
# target that links it.
rsf_add_rust_static(rsf_godot_rust rsf_rust_godot rsf-godot rsf_godot)

# The GDExtension is written against Godot's C interface, so the build needs no godot-cpp checkout.
# eng/build-godot.ps1 enables it.
option(RSF_GODOT_NATIVE "Build ReScaleFrame.Godot.Native.dll, the optional Godot integration" OFF)
if(NOT RSF_GODOT_NATIVE)
    return()
endif()

# exports.def lists the GDExtension entry and the engine exports, all Rust. runtime_platform.c
# supplies the optional provider symbols, the project version, the layout checks and DLL unload.
add_library(rsf_godot SHARED native/exports.def native/runtime_platform.c)
if(EXISTS "${RSF_VULKAN_INCLUDE}/vulkan/vulkan.h")
    target_include_directories(rsf_godot SYSTEM PRIVATE "${RSF_VULKAN_INCLUDE}")
endif()
target_compile_definitions(rsf_godot PRIVATE RSF_GODOT_NATIVE_BUILD RSF_STREAMLINE_HOST_BUILD)
target_link_libraries(rsf_godot PRIVATE rsf_godot_rust rsf_orchestrator_core ReScaleFrame::NativeVulkan rsf_build_options)
set_target_properties(rsf_godot PROPERTIES OUTPUT_NAME "ReScaleFrame.Godot.Native")
