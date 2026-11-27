if(MSVC)
    enable_language(ASM_MASM)

    # Shared version.dll shim (Unity drop-in carrier): version and DirectInput forwarding plus the
    # configured entry. It has a different ordinal contract from the AC7 carrier.
    add_library(rsf_proxy_dinput8 SHARED
        src/shim_entry.c src/version_forwarders.asm src/shim_exports.def)
    target_link_libraries(rsf_proxy_dinput8 PRIVATE rsf_loader_rust bcrypt rsf_build_options)
    set_target_properties(rsf_proxy_dinput8 PROPERTIES OUTPUT_NAME "dinput8" PREFIX "")
    add_custom_command(TARGET rsf_proxy_dinput8 POST_BUILD
        COMMAND ${CMAKE_COMMAND} -E make_directory "$<TARGET_FILE_DIR:rsf_proxy_dinput8>/carriers"
        COMMAND ${CMAKE_COMMAND} -E copy_if_different "$<TARGET_FILE:rsf_proxy_dinput8>"
            "$<TARGET_FILE_DIR:rsf_proxy_dinput8>/ReScaleFrame.Loader.dll"
        COMMAND ${CMAKE_COMMAND} -E copy_if_different "$<TARGET_FILE:rsf_proxy_dinput8>"
            "$<TARGET_FILE_DIR:rsf_proxy_dinput8>/carriers/version.dll")

    add_library(rsf_proxy_plugin SHARED
        src/plugin_carrier.c src/xinput_forwarders.asm src/xinput_exports.def)
    target_link_libraries(rsf_proxy_plugin PRIVATE
        rsf_loader_rust bcrypt rsf_build_options)
    set_target_properties(rsf_proxy_plugin PROPERTIES OUTPUT_NAME "xinput1_3" PREFIX ""
        RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/plugin-carrier/bin/$<CONFIG>"
        ARCHIVE_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/plugin-carrier/lib/$<CONFIG>"
        PDB_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/plugin-carrier/bin/$<CONFIG>")
    foreach(config Debug Release RelWithDebInfo MinSizeRel)
        file(MAKE_DIRECTORY "${CMAKE_CURRENT_BINARY_DIR}/rsf_proxy_dinput8.dir/${config}/src")
        file(MAKE_DIRECTORY "${CMAKE_CURRENT_BINARY_DIR}/rsf_proxy_plugin.dir/${config}/src")
    endforeach()
endif()

# Standalone DirectInput carrier: OS forwarding and configured DLL entry only.
# Do not inherit the combined proxy's object libraries or dependency graph.
add_library(rsf_proxy_thin_dinput8 SHARED
    src/thin_entry.c src/thin_exports.def)
target_link_libraries(rsf_proxy_thin_dinput8 PRIVATE rsf_loader_rust rsf_build_options)
if(MSVC)
    # COM exports intentionally retain the system DirectInput ordinal contract.
    target_link_options(rsf_proxy_thin_dinput8 PRIVATE /INCREMENTAL:NO /OPT:REF /IGNORE:4222 "/PDBALTPATH:%_PDB%")
endif()
set_target_properties(rsf_proxy_thin_dinput8 PROPERTIES OUTPUT_NAME "dinput8" PREFIX ""
    RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/thin/bin/$<CONFIG>"
    ARCHIVE_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/thin/lib/$<CONFIG>"
    PDB_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/thin/bin/$<CONFIG>")
