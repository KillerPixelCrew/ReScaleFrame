function(rsf_add_rust_static target build_target package archive)
    find_program(RSF_CARGO cargo REQUIRED)
    if(MSVC)
        set(rust_target x86_64-pc-windows-msvc)
    else()
        set(rust_target x86_64-pc-windows-gnu)
    endif()
    set(cargo_dir "${CMAKE_BINARY_DIR}/cargo")
    set(profile "$<IF:$<CONFIG:Debug>,debug,release>")
    set(rust_archive "${cargo_dir}/${rust_target}/${profile}/${CMAKE_STATIC_LIBRARY_PREFIX}${archive}${CMAKE_STATIC_LIBRARY_SUFFIX}")
    add_custom_target(${build_target}
        COMMAND "${RSF_CARGO}" build --locked -p ${package}
            --target ${rust_target} --target-dir "${cargo_dir}" $<$<NOT:$<CONFIG:Debug>>:--release>
        WORKING_DIRECTORY "${PROJECT_SOURCE_DIR}"
        BYPRODUCTS "${rust_archive}"
        COMMAND_EXPAND_LISTS VERBATIM)
    add_library(${target} INTERFACE)
    add_dependencies(${target} ${build_target})
    target_link_libraries(${target} INTERFACE "${rust_archive}" ntdll userenv ws2_32 dbghelp)
    if(MSVC)
        target_link_libraries(${target} INTERFACE synchronization)
    endif()
endfunction()
