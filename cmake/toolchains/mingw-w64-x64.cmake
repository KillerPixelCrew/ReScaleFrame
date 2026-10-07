# Cross-compile the Windows x64 targets from Linux with mingw-w64.
#
# Development cross-build path; Windows/MSVC remains the reference CI configuration.

set(CMAKE_SYSTEM_NAME Windows)
set(CMAKE_SYSTEM_PROCESSOR x86_64)

set(RSF_MINGW_PREFIX "x86_64-w64-mingw32" CACHE STRING "mingw-w64 target triple")

set(CMAKE_C_COMPILER "${RSF_MINGW_PREFIX}-gcc")
set(CMAKE_CXX_COMPILER "${RSF_MINGW_PREFIX}-g++")
set(CMAKE_RC_COMPILER "${RSF_MINGW_PREFIX}-windres")

set(CMAKE_FIND_ROOT_PATH "/usr/${RSF_MINGW_PREFIX}")
# Build tools run on the host; headers/libraries/packages must resolve in the target sysroot.
set(CMAKE_FIND_ROOT_PATH_MODE_PROGRAM NEVER)
set(CMAKE_FIND_ROOT_PATH_MODE_LIBRARY ONLY)
set(CMAKE_FIND_ROOT_PATH_MODE_INCLUDE ONLY)
set(CMAKE_FIND_ROOT_PATH_MODE_PACKAGE ONLY)

# The test executables run under Wine, so ctest needs to know how to launch them.
find_program(RSF_WINE_EXECUTABLE NAMES wine64 wine)
if(RSF_WINE_EXECUTABLE)
    set(CMAKE_CROSSCOMPILING_EMULATOR "${RSF_WINE_EXECUTABLE}")
endif()
