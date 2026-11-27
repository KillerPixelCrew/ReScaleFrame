# Include first from tests/CMakeLists.txt.
include_guard(GLOBAL)

# A fixture executable: fixture_main.c calls the Rust export ENTRY (with ARGV, fixture_main_args.c
# hands it argc and argv). NAME also registers the CTest entry; SKIP77 and TIMEOUT set the properties
# the fixtures use, ARGS are the test's arguments. SOURCES and DEFINES add to the executable, CXX links
# it as C++ and MANUAL keeps it out of the default build. LIBS are linked with rsf_build_options.
function(rsf_add_fixture_test target)
    cmake_parse_arguments(PARSE_ARGV 1 fixture "ARGV;CXX;MANUAL;SKIP77" "NAME;ENTRY;TIMEOUT"
        "ARGS;DEFINES;LIBS;SOURCES")
    set(main fixture_main.c)
    if(fixture_ARGV)
        set(main fixture_main_args.c)
    endif()
    add_executable(${target} "${CMAKE_CURRENT_FUNCTION_LIST_DIR}/${main}" ${fixture_SOURCES})
    target_compile_definitions(${target} PRIVATE RSF_FIXTURE_ENTRY=${fixture_ENTRY} ${fixture_DEFINES})
    target_link_libraries(${target} PRIVATE ${fixture_LIBS} rsf_build_options)
    if(fixture_CXX)
        set_target_properties(${target} PROPERTIES LINKER_LANGUAGE CXX)
    endif()
    if(fixture_MANUAL)
        set_target_properties(${target} PROPERTIES EXCLUDE_FROM_ALL ON)
    endif()
    if(fixture_NAME)
        add_test(NAME ${fixture_NAME} COMMAND ${target} ${fixture_ARGS})
        set(properties)
        if(fixture_SKIP77)
            list(APPEND properties SKIP_RETURN_CODE 77)
        endif()
        if(fixture_TIMEOUT)
            list(APPEND properties TIMEOUT ${fixture_TIMEOUT})
        endif()
        if(properties)
            set_tests_properties(${fixture_NAME} PROPERTIES ${properties})
        endif()
    endif()
endfunction()

# A test client that imports ReScaleFrame.Runtime.dll the way a DLL-loaded adapter does. It takes the
# Runtime's public include directories but not OrchestratorCore and its public RSF_RUNTIME_BUILD define.
function(rsf_add_runtime_client target)
    add_executable(${target} ${ARGN})
    target_include_directories(${target} PRIVATE "$<TARGET_PROPERTY:rsf_orchestrator,INTERFACE_INCLUDE_DIRECTORIES>")
    target_link_libraries(${target} PRIVATE "$<TARGET_LINKER_FILE:rsf_orchestrator>" rsf_build_options)
    add_dependencies(${target} rsf_orchestrator)
endfunction()
# A public header compiled as strict C11 with no executable: the static asserts are the test.
function(rsf_add_c11_header_check target source)
    add_library(${target} OBJECT ${source})
    set_target_properties(${target} PROPERTIES C_STANDARD 11 C_STANDARD_REQUIRED ON C_EXTENSIONS OFF)
    target_include_directories(${target} PRIVATE "$<TARGET_PROPERTY:rsf_orchestrator,INTERFACE_INCLUDE_DIRECTORIES>")
    target_link_libraries(${target} PRIVATE rsf_build_options)
endfunction()
