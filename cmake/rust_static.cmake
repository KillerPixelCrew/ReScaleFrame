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
#
# A binary links one Rust archive: two would each carry rsf-game-sys and the other shared crates, and
# their exports collide once Cargo has built the copies with different hashes and both are pulled in.
# `CARRIES "<archive targets>"` names the archive targets whose crates this archive embeds as rlibs.
# rsf_resolve_embedded_archives (run once at the end of the top-level CMakeLists.txt) lists, for each
# binary, every archive in its link closure that another archive in it carries in the binary's
# RSF_EMBEDDED_ARCHIVES; such an archive keeps its C leaves and system libraries there but not its
# archive file. An archive that carries rsf_orchestrator_rust builds with the Runtime archive's
# features and environment (RSF_RUNTIME_EMBED_ARGS). The archive path is the target's RSF_ARCHIVE.
function(rsf_add_rust_static target build_target package archive)
    cmake_parse_arguments(PARSE_ARGV 4 rsf "" "ENV;CARRIES" "")
    if("rsf_orchestrator_rust" IN_LIST rsf_CARRIES)
        get_property(rsf_embed GLOBAL PROPERTY RSF_RUNTIME_EMBED_ARGS)
        cmake_parse_arguments(rsf_embed "" "ENV" "" ${rsf_embed})
        list(APPEND rsf_UNPARSED_ARGUMENTS ${rsf_embed_UNPARSED_ARGUMENTS})
        if(rsf_embed_ENV)
            set(rsf_ENV "${rsf_embed_ENV}")
        endif()
    endif()
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
    set_target_properties(${target} PROPERTIES RSF_ARCHIVE "${rust_archive}" RSF_CARRIES "${rsf_CARRIES}")
    target_link_libraries(${target} INTERFACE
        "$<$<NOT:$<IN_LIST:${target},$<TARGET_PROPERTY:RSF_EMBEDDED_ARCHIVES>>>:${rust_archive}>"
        ntdll userenv ws2_32 dbghelp)
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

# The targets that CMake files under <directory> define, recursively.
function(rsf_directory_targets directory out)
    get_property(targets DIRECTORY "${directory}" PROPERTY BUILDSYSTEM_TARGETS)
    get_property(subdirectories DIRECTORY "${directory}" PROPERTY SUBDIRECTORIES)
    foreach(subdirectory IN LISTS subdirectories)
        rsf_directory_targets("${subdirectory}" more)
        list(APPEND targets ${more})
    endforeach()
    set(${out} ${targets} PARENT_SCOPE)
endfunction()

# The targets named in link items, inside generator expressions too (LINK_ONLY, LINK_LIBRARY and
# conditions), with aliases resolved. It errs on the side of including a conditional item.
function(rsf_link_item_targets items out)
    set(found)
    foreach(item IN LISTS items)
        string(REGEX MATCHALL "[A-Za-z0-9_.+-]+(::[A-Za-z0-9_.+-]+)?" names "${item}")
        foreach(name IN LISTS names)
            if(TARGET "${name}")
                get_target_property(aliased "${name}" ALIASED_TARGET)
                if(aliased)
                    set(name "${aliased}")
                endif()
                list(APPEND found "${name}")
            endif()
        endforeach()
    endforeach()
    set(${out} ${found} PARENT_SCOPE)
endfunction()

# See rsf_add_rust_static: drops, from every binary, the archives another archive it links carries.
function(rsf_resolve_embedded_archives)
    rsf_directory_targets("${PROJECT_SOURCE_DIR}" targets)
    foreach(head IN LISTS targets)
        get_target_property(type ${head} TYPE)
        if(NOT type MATCHES "^(EXECUTABLE|SHARED_LIBRARY|MODULE_LIBRARY)$")
            continue()
        endif()
        get_target_property(items ${head} LINK_LIBRARIES)
        rsf_link_item_targets("${items}" queue)
        set(closure)
        while(queue)
            list(POP_FRONT queue dependency)
            if(dependency IN_LIST closure)
                continue()
            endif()
            list(APPEND closure ${dependency})
            get_target_property(items ${dependency} INTERFACE_LINK_LIBRARIES)
            if(items)
                rsf_link_item_targets("${items}" more)
                list(APPEND queue ${more})
            endif()
        endwhile()
        set(embedded)
        foreach(dependency IN LISTS closure)
            get_target_property(carried ${dependency} RSF_CARRIES)
            foreach(archive IN LISTS carried)
                if(archive IN_LIST closure)
                    list(APPEND embedded ${archive})
                endif()
            endforeach()
        endforeach()
        if(embedded)
            list(REMOVE_DUPLICATES embedded)
            set_property(TARGET ${head} APPEND PROPERTY RSF_EMBEDDED_ARCHIVES ${embedded})
        endif()
    endforeach()
endfunction()