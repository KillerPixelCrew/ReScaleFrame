cmake_minimum_required(VERSION 3.25)

file(STRINGS "${CMAKE_CURRENT_SOURCE_DIR}/VERSION" RSF_VERSION LIMIT_COUNT 1)
project(ReScaleFrame VERSION "${RSF_VERSION}" LANGUAGES C CXX)

if(NOT WIN32 OR NOT CMAKE_SIZEOF_VOID_P EQUAL 8)
    message(FATAL_ERROR "The initial ReScaleFrame target is Windows x64.")
endif()

if(NOT MSVC)
    # mingw would otherwise emit libReScaleFrame.Bootstrap.dll. Windows DLL names are exact, and
    # a forwarder has to match the name the game imports, so the cross build keeps MSVC's naming.
    set(CMAKE_SHARED_LIBRARY_PREFIX "")
    set(CMAKE_SHARED_MODULE_PREFIX "")
endif()

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_C_STANDARD 11)
set(CMAKE_C_STANDARD_REQUIRED ON)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)
set(CMAKE_RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/bin")
set(CMAKE_LIBRARY_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/lib")
set(CMAKE_ARCHIVE_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/lib")

configure_file(cmake/version.h.in generated/rescaleframe/version.h @ONLY)
add_library(rsf_build_options INTERFACE)
target_include_directories(rsf_build_options INTERFACE "${CMAKE_BINARY_DIR}/generated")
target_compile_definitions(rsf_build_options INTERFACE WIN32_LEAN_AND_MEAN NOMINMAX)
if(MSVC)
    target_compile_options(rsf_build_options INTERFACE /W4 /WX /utf-8)
    # fopen and friends are used on purpose in the diagnostics. The secure-CRT deprecation is not
    # a defect and must not fail the warnings-are-errors build; it is what failed every CI run.
    target_compile_definitions(rsf_build_options INTERFACE _CRT_SECURE_NO_WARNINGS)
elseif(CMAKE_CXX_COMPILER_ID MATCHES "GNU|Clang")
    # The mingw cross build enforces the same warnings-are-errors rule as the MSVC reference build.
    target_compile_options(rsf_build_options INTERFACE -Wall -Wextra -Werror)
    # Nothing supplies the mingw runtime DLLs in a Wine prefix or a game process, so link them in.
    target_link_options(rsf_build_options INTERFACE -static -static-libgcc -static-libstdc++)
endif()

# MinHook, for hooking by detour where a vtable entry is contested. Steam's overlay hooks
# IDXGISwapChain::Present both in the table and at the function's entry and re-asserts either when
# displaced, so a table patch and the overlay chase each other; a detour with a trampoline built
# from the genuine bytes sits in the same chain without ever routing through the entry again.
# BSD-2-Clause, pinned to v1.3.4.
include(FetchContent)
FetchContent_Declare(minhook
    GIT_REPOSITORY https://github.com/TsudaKageyu/minhook.git
    GIT_TAG c3fcafdc10146beb5919319d0683e44e3c30d537 # v1.3.4
    GIT_SHALLOW FALSE)
FetchContent_MakeAvailable(minhook)

include(CTest)
add_subdirectory(sdk/game)
add_subdirectory(loader)
add_subdirectory(runtime/contract)
add_subdirectory(runtime/graphics)
add_subdirectory(runtime/presentation)
add_subdirectory(runtime/backends/dlss)
add_subdirectory(runtime/backends/fsr)
add_subdirectory(runtime/backends/xess)
add_subdirectory(runtime/orchestrator)
add_subdirectory(games/ac7)
add_subdirectory(games/unity-mono)
add_subdirectory(apps/launcher)
if(BUILD_TESTING)
    add_subdirectory(tests)
endif()
