# SPDX-License-Identifier: GPL-3.0-only
# Build-time step of the rsf_overlay_panel_stage target (see the top-level CMakeLists.txt).
#
# The Rust panel, rescaleframe_overlay.dll, is built by cargo into target/<profile>/. Packagers and
# deploy scripts take it from there. A copy next to the Runtime in bin/<Config>/ is only a
# convenience, and an old one is dangerous: it has loaded as the panel of a newer Runtime and broken
# a fixture. So this step either places the current panel there or removes whatever is there.
#
# Inputs: SOURCE (target/<profile>/rescaleframe_overlay.dll), DEST (bin/<Config>/ copy),
# ROOT (repository root, where the Rust sources are).
if(NOT SOURCE OR NOT DEST OR NOT ROOT)
    message(FATAL_ERROR "stage_overlay_panel.cmake needs -DSOURCE, -DDEST and -DROOT")
endif()

file(GLOB_RECURSE rsf_panel_inputs
    "${ROOT}/ui/overlay/src/*.rs" "${ROOT}/ui/overlay/Cargo.toml"
    "${ROOT}/Cargo.toml" "${ROOT}/Cargo.lock")

set(rsf_panel_problem "")
if(NOT EXISTS "${SOURCE}")
    set(rsf_panel_problem "${SOURCE} has not been built")
else()
    foreach(input IN LISTS rsf_panel_inputs)
        if("${input}" IS_NEWER_THAN "${SOURCE}")
            set(rsf_panel_problem "${SOURCE} is older than ${input}")
            break()
        endif()
    endforeach()
endif()

if(rsf_panel_problem)
    if(EXISTS "${DEST}")
        file(REMOVE "${DEST}")
        set(rsf_panel_removed " The stale copy ${DEST} was removed.")
    else()
        set(rsf_panel_removed "")
    endif()
    message(WARNING "rescaleframe_overlay.dll is not staged: ${rsf_panel_problem}.${rsf_panel_removed} "
        "Run: cargo build --release --locked -p rescaleframe-overlay")
else()
    execute_process(COMMAND "${CMAKE_COMMAND}" -E copy_if_different "${SOURCE}" "${DEST}"
        RESULT_VARIABLE rsf_panel_copy)
    if(NOT rsf_panel_copy EQUAL 0)
        message(FATAL_ERROR "could not copy ${SOURCE} to ${DEST}")
    endif()
    message(STATUS "rescaleframe_overlay.dll staged from ${SOURCE}")
endif()
