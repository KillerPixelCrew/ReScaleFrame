# SPDX-License-Identifier: MIT
# Keep the CTest executable and native linker; the Rust object uses the
# standard library supplied by the same toolchain's orchestrator staticlib.
function(rsf_add_g02_fixture target source)
    if(DEFINED ENV{RUSTC} AND NOT "$ENV{RUSTC}" STREQUAL "")
        set(fixture_rustc "$ENV{RUSTC}")
    else()
        find_program(fixture_rustc rustc REQUIRED)
    endif()
    if(MSVC)
        set(rust_target x86_64-pc-windows-msvc)
    else()
        set(rust_target x86_64-pc-windows-gnu)
    endif()
    set(dir "${CMAKE_CURRENT_FUNCTION_LIST_DIR}")
    set(object "${CMAKE_CURRENT_BINARY_DIR}/$<CONFIG>/${target}.obj")
    add_custom_command(OUTPUT "${object}"
        COMMAND "${CMAKE_COMMAND}" -E make_directory "${CMAKE_CURRENT_BINARY_DIR}/$<CONFIG>"
        COMMAND "${fixture_rustc}" --edition 2024 --target "${rust_target}"
            --crate-name "${target}" --crate-type lib --emit obj -C codegen-units=1
            -C panic=abort
            -C opt-level=$<IF:$<CONFIG:Debug>,0,3> ${RSF_FIXTURE_EXTERNS}
            -o "${object}" "${dir}/${source}"
        DEPENDS "${dir}/${source}" "${dir}/d3d11.rs" rsf_rust_fixture_rlibs
        WORKING_DIRECTORY "${PROJECT_SOURCE_DIR}"
        VERBATIM)
    set_source_files_properties("${object}" PROPERTIES GENERATED TRUE EXTERNAL_OBJECT TRUE)
    add_executable("${target}" "${dir}/../fixture_main.c" "${object}")
    target_compile_definitions("${target}" PRIVATE RSF_FIXTURE_ENTRY=rsf_fixture_main)
    set_target_properties("${target}" PROPERTIES LINKER_LANGUAGE CXX)
endfunction()
