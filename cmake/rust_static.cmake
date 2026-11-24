find_program(RSF_CARGO cargo REQUIRED)
if(MSVC)
    set(RSF_RUST_TARGET x86_64-pc-windows-msvc)
else()
    set(RSF_RUST_TARGET x86_64-pc-windows-gnu)
endif()
set(RSF_CARGO_DIR "${CMAKE_BINARY_DIR}/cargo")
set(RSF_CARGO_ARTIFACTS "${RSF_CARGO_DIR}/${RSF_RUST_TARGET}/$<IF:$<CONFIG:Debug>,debug,release>")

# Builds of one package run one after another: each waits for the previous build target of its package.
function(rsf_serialize_rust_build build_target package)
    get_property(previous GLOBAL PROPERTY RSF_RUST_BUILD_${package})
    if(previous)
        add_dependencies(${build_target} ${previous})
    endif()
    set_property(GLOBAL PROPERTY RSF_RUST_BUILD_${package} ${build_target})
endfunction()

# Every archive builds in the one target directory, so each dependency compiles once. Cargo writes a
# package's archive to <archive>.lib whatever its features, and a second build of the package
# (the plugin entry, the fixtures) would overwrite the first, so each build copies its archive to a
# file named for its own target. Builds of one package run one after another so that the copy is of
# its own build. An optional `ENV NAME=value` sets one variable for the Cargo build; the remaining
# arguments go to cargo build.
function(rsf_add_rust_static target build_target package archive)
    cmake_parse_arguments(PARSE_ARGV 4 rsf "" "ENV" "")
    set(rust_archive "${RSF_CARGO_ARTIFACTS}/archives/${CMAKE_STATIC_LIBRARY_PREFIX}${target}${CMAKE_STATIC_LIBRARY_SUFFIX}")
    add_custom_target(${build_target}
        COMMAND "${CMAKE_COMMAND}" -E env ${rsf_ENV} "${RSF_CARGO}" build --locked -p ${package} ${rsf_UNPARSED_ARGUMENTS}
            --target ${RSF_RUST_TARGET} --target-dir "${RSF_CARGO_DIR}" $<$<NOT:$<CONFIG:Debug>>:--release>
        COMMAND "${CMAKE_COMMAND}" -E make_directory "${RSF_CARGO_ARTIFACTS}/archives"
        COMMAND "${CMAKE_COMMAND}" -E copy_if_different
            "${RSF_CARGO_ARTIFACTS}/${CMAKE_STATIC_LIBRARY_PREFIX}${archive}${CMAKE_STATIC_LIBRARY_SUFFIX}" "${rust_archive}"
        WORKING_DIRECTORY "${PROJECT_SOURCE_DIR}"
        BYPRODUCTS "${rust_archive}"
        COMMAND_EXPAND_LISTS VERBATIM)
    rsf_serialize_rust_build(${build_target} ${package})
    add_library(${target} INTERFACE)
    add_dependencies(${target} ${build_target})
    target_link_libraries(${target} INTERFACE "${rust_archive}" ntdll userenv ws2_32 dbghelp)
    if(MSVC)
        # legacy_stdio_definitions: the bounded swprintf entry the settings store formats with.
        target_link_libraries(${target} INTERFACE synchronization legacy_stdio_definitions)
    endif()
endfunction()

# Builds the executables or DLLs of a Cargo package, which the tests run in a process of their own.
# FILES are the outputs, relative to Cargo's profile directory (a bin or cdylib name with its
# suffix); with a <directory> each is also copied there, where CTest and the fixtures look for it.
# ARGS go to cargo build (--example, --features). Builds of one package run one after another, as
# for the archives above.
function(rsf_add_rust_artifacts build_target package directory)
    cmake_parse_arguments(PARSE_ARGV 3 rsf "ALL" "" "FILES;ARGS")
    set(copy)
    set(outputs)
    if(directory)
        list(APPEND copy COMMAND "${CMAKE_COMMAND}" -E make_directory "${directory}")
    endif()
    foreach(file IN LISTS rsf_FILES)
        get_filename_component(name "${file}" NAME)
        list(APPEND outputs "${RSF_CARGO_ARTIFACTS}/${file}")
        if(directory)
            list(APPEND copy COMMAND "${CMAKE_COMMAND}" -E copy_if_different
                "${RSF_CARGO_ARTIFACTS}/${file}" "${directory}/${name}")
            list(APPEND outputs "${directory}/${name}")
        endif()
    endforeach()
    set(all)
    if(rsf_ALL)
        set(all ALL)
    endif()
    add_custom_target(${build_target} ${all}
        COMMAND "${RSF_CARGO}" build --locked -p ${package} ${rsf_ARGS}
            --target ${RSF_RUST_TARGET} --target-dir "${RSF_CARGO_DIR}" $<$<NOT:$<CONFIG:Debug>>:--release>
        ${copy}
        WORKING_DIRECTORY "${PROJECT_SOURCE_DIR}"
        BYPRODUCTS ${outputs}
        COMMAND_EXPAND_LISTS VERBATIM)
    rsf_serialize_rust_build(${build_target} ${package})
endfunction()