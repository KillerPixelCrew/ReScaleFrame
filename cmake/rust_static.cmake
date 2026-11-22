function(rsf_add_rust_static target build_target package archive)
    find_program(RSF_CARGO cargo REQUIRED)
    if(MSVC)
        set(rust_target x86_64-pc-windows-msvc)
    else()
        set(rust_target x86_64-pc-windows-gnu)
    endif()
    set(cargo_dir "${CMAKE_BINARY_DIR}/cargo")
    if(ARGN)
        set(cargo_dir "${cargo_dir}/${package}-${target}")
    endif()
    set(profile "$<IF:$<CONFIG:Debug>,debug,release>")
    set(rust_archive "${cargo_dir}/${rust_target}/${profile}/${CMAKE_STATIC_LIBRARY_PREFIX}${archive}${CMAKE_STATIC_LIBRARY_SUFFIX}")
    add_custom_target(${build_target}
        COMMAND "${RSF_CARGO}" build --locked -p ${package} ${ARGN}
            --target ${rust_target} --target-dir "${cargo_dir}" $<$<NOT:$<CONFIG:Debug>>:--release>
        WORKING_DIRECTORY "${PROJECT_SOURCE_DIR}"
        BYPRODUCTS "${rust_archive}"
        COMMAND_EXPAND_LISTS VERBATIM)
    add_library(${target} INTERFACE)
    add_dependencies(${target} ${build_target})
    target_link_libraries(${target} INTERFACE "${rust_archive}" ntdll userenv ws2_32 dbghelp)
    if(MSVC)
        # legacy_stdio_definitions: the bounded swprintf entry the settings store formats with.
        target_link_libraries(${target} INTERFACE synchronization legacy_stdio_definitions)
    endif()
endfunction()

# The standalone rustc fixtures take their C bindings from the crates that own the headers, which
# generate them at build time. They use only binding types and constants, so the fixtures need
# these rlibs' metadata, not their code. Sets <out> to the rustc arguments.
function(rsf_rust_fixture_externs out)
    find_program(RSF_CARGO cargo REQUIRED)
    if(MSVC)
        set(rust_target x86_64-pc-windows-msvc)
    else()
        set(rust_target x86_64-pc-windows-gnu)
    endif()
    set(cargo_dir "${CMAKE_BINARY_DIR}/cargo")
    set(profile "$<IF:$<CONFIG:Debug>,debug,release>")
    if(NOT TARGET rsf_rust_fixture_rlibs)
        add_custom_target(rsf_rust_fixture_rlibs
            COMMAND "${RSF_CARGO}" build --locked -p rsf-game-sys -p rsf-ac7 -p rsf-orchestrator
                --target ${rust_target} --target-dir "${cargo_dir}" $<$<NOT:$<CONFIG:Debug>>:--release>
            WORKING_DIRECTORY "${PROJECT_SOURCE_DIR}"
            VERBATIM)
    endif()
    set(rlibs "${cargo_dir}/${rust_target}/${profile}")
    set(${out}
        --extern "rsf_game_sys=${rlibs}/librsf_game_sys.rlib"
        --extern "rsf_ac7=${rlibs}/librsf_ac7.rlib"
        --extern "rsf_orchestrator=${rlibs}/librsf_orchestrator.rlib"
        # Host proc macros land outside the target directory.
        -L "dependency=${rlibs}/deps" -L "dependency=${cargo_dir}/${profile}/deps"
        PARENT_SCOPE)
endfunction()
