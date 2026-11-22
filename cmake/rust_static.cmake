# Every archive builds in the one target directory, so each dependency compiles once. Cargo writes a
# package's archive to <archive>.lib whatever its features, and a second build of the package
# (the plugin entry, the fixtures) would overwrite the first, so each build copies its archive to a
# file named for its own target. Builds of one package run one after another so that the copy is of
# its own build. An optional `ENV NAME=value` sets one variable for the Cargo build; the remaining
# arguments go to cargo build.
function(rsf_add_rust_static target build_target package archive)
    cmake_parse_arguments(PARSE_ARGV 4 rsf "" "ENV" "")
    find_program(RSF_CARGO cargo REQUIRED)
    if(MSVC)
        set(rust_target x86_64-pc-windows-msvc)
    else()
        set(rust_target x86_64-pc-windows-gnu)
    endif()
    set(cargo_dir "${CMAKE_BINARY_DIR}/cargo")
    set(profile "$<IF:$<CONFIG:Debug>,debug,release>")
    set(artifacts "${cargo_dir}/${rust_target}/${profile}")
    set(rust_archive "${artifacts}/archives/${CMAKE_STATIC_LIBRARY_PREFIX}${target}${CMAKE_STATIC_LIBRARY_SUFFIX}")
    add_custom_target(${build_target}
        COMMAND "${CMAKE_COMMAND}" -E env ${rsf_ENV} "${RSF_CARGO}" build --locked -p ${package} ${rsf_UNPARSED_ARGUMENTS}
            --target ${rust_target} --target-dir "${cargo_dir}" $<$<NOT:$<CONFIG:Debug>>:--release>
        COMMAND "${CMAKE_COMMAND}" -E make_directory "${artifacts}/archives"
        COMMAND "${CMAKE_COMMAND}" -E copy_if_different
            "${artifacts}/${CMAKE_STATIC_LIBRARY_PREFIX}${archive}${CMAKE_STATIC_LIBRARY_SUFFIX}" "${rust_archive}"
        WORKING_DIRECTORY "${PROJECT_SOURCE_DIR}"
        BYPRODUCTS "${rust_archive}"
        COMMAND_EXPAND_LISTS VERBATIM)
    get_property(previous GLOBAL PROPERTY RSF_RUST_BUILD_${package})
    if(previous)
        add_dependencies(${build_target} ${previous})
    endif()
    set_property(GLOBAL PROPERTY RSF_RUST_BUILD_${package} ${build_target})
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
