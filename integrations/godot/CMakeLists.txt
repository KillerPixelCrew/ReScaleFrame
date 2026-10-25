set(RSF_GODOT_CPP_DIR "" CACHE PATH "Pinned godot-cpp checkout for the optional Godot integration")
if(NOT RSF_GODOT_CPP_DIR)
    return()
endif()
if(NOT EXISTS "${RSF_GODOT_CPP_DIR}/CMakeLists.txt")
    message(FATAL_ERROR "RSF_GODOT_CPP_DIR must name a prepared godot-cpp checkout. Run eng/prepare-godot.ps1.")
endif()

set(GODOTCPP_API_VERSION "4.7" CACHE STRING "Godot API baseline" FORCE)
set(GODOTCPP_BUILD_PROFILE "${CMAKE_CURRENT_SOURCE_DIR}/build-profile.json" CACHE FILEPATH "Godot binding subset" FORCE)
set(GODOTCPP_USE_STATIC_CPP OFF CACHE BOOL "Use the native project's CRT" FORCE)
add_subdirectory("${RSF_GODOT_CPP_DIR}" "${CMAKE_CURRENT_BINARY_DIR}/godot-cpp" SYSTEM)
set_property(TARGET godot-cpp PROPERTY MSVC_RUNTIME_LIBRARY "MultiThreaded$<$<CONFIG:Debug>:Debug>DLL")

add_library(rsf_godot SHARED native/extension.cpp native/effect.cpp native/runtime.cpp native/runtime_control.cpp)
target_compile_definitions(rsf_godot PRIVATE RSF_GODOT_NATIVE_BUILD RSF_STREAMLINE_HOST_BUILD)
target_link_libraries(rsf_godot PRIVATE godot::cpp rsf_orchestrator_core rsf_build_options)
set_target_properties(rsf_godot PROPERTIES OUTPUT_NAME "ReScaleFrame.Godot.Native"
    MSVC_RUNTIME_LIBRARY "MultiThreaded$<$<CONFIG:Debug>:Debug>DLL")
