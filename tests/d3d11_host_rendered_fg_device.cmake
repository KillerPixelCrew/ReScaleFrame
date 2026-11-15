# SPDX-License-Identifier: GPL-3.0-only
# Explicit hardware experiment; never part of ALL or routine CTest.
add_executable(rsf_d3d11_host_rendered_fg_device EXCLUDE_FROM_ALL
    "${CMAKE_CURRENT_LIST_DIR}/d3d11_host_rendered_fg_device.cpp")
target_link_libraries(rsf_d3d11_host_rendered_fg_device PRIVATE
    rsf_orchestrator rsf_build_options d3d11 dxgi)
# Coordinator integration: include this file from tests/CMakeLists.txt.
# cmake --build build/windows-x64 --config Release --target rsf_d3d11_host_rendered_fg_device
# build/windows-x64/bin/Release/rsf_d3d11_host_rendered_fg_device.exe <scratch-root>
# Scratch root must contain streamline/ and fidelityfx/ with matching SDK files.
# Exit 0: SR-Off generated callbacks, SR-On evaluations/tags and negative checks passed.
# Exit 77: no suitable hardware/SDK or no hidden-window generation receipt; incomplete.
# Exit 1: contract/API/debug-queue/Present/check failure. Never means game acceptance.
