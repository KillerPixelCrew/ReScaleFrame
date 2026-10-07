# SPDX-License-Identifier: MIT
# Header-only ABI target: exposes include paths without linking either DLL's implementation.
add_library(rsf_game_sdk INTERFACE)
add_library(ReScaleFrame::GameSDK ALIAS rsf_game_sdk)
target_include_directories(rsf_game_sdk INTERFACE "${CMAKE_CURRENT_SOURCE_DIR}/include")
