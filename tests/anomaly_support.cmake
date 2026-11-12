# CPU history/planning contracts only; these tests do not prove game motion output.
add_executable(rsf_anomaly_temporal_test "${CMAKE_CURRENT_LIST_DIR}/anomaly_temporal_tests.cpp")
target_include_directories(rsf_anomaly_temporal_test PRIVATE "${PROJECT_SOURCE_DIR}")
target_link_libraries(rsf_anomaly_temporal_test PRIVATE rsf_build_options)
add_test(NAME anomaly_temporal_contract COMMAND rsf_anomaly_temporal_test)
add_test(NAME anomaly_temporal_unknown_selection COMMAND rsf_anomaly_temporal_test unknown-case)
set_tests_properties(anomaly_temporal_unknown_selection PROPERTIES WILL_FAIL TRUE)
set_tests_properties(anomaly_temporal_contract anomaly_temporal_unknown_selection PROPERTIES TIMEOUT 30)

find_package(Python3 COMPONENTS Interpreter QUIET)
if(Python3_Interpreter_FOUND)
    add_test(NAME anomaly_shader_tool_contract COMMAND "${Python3_EXECUTABLE}" -m unittest discover
        -s "${CMAKE_CURRENT_LIST_DIR}" -p anomaly_shader_test.py -v)
    set_tests_properties(anomaly_shader_tool_contract PROPERTIES
        WORKING_DIRECTORY "${PROJECT_SOURCE_DIR}" TIMEOUT 120)
endif()

# Geometry identity/lifetime producer. Pure CPU registry, draw scopes and history feed, then real x64
# detours on fixture functions plus the production guarded install against a synthetic image. No game code runs.
set(RSF_ANOMALY_GEOMETRY_SOURCES
    "${PROJECT_SOURCE_DIR}/games/anomaly/src/native_geometry.cpp"
    "${PROJECT_SOURCE_DIR}/games/anomaly/src/geometry_feed.cpp"
    "${PROJECT_SOURCE_DIR}/games/anomaly/src/native_hooks.cpp")
add_executable(rsf_anomaly_geometry_test "${CMAKE_CURRENT_LIST_DIR}/anomaly_geometry_tests.cpp" ${RSF_ANOMALY_GEOMETRY_SOURCES})
target_include_directories(rsf_anomaly_geometry_test PRIVATE "${PROJECT_SOURCE_DIR}/games/anomaly/src")
target_link_libraries(rsf_anomaly_geometry_test PRIVATE minhook rsf_build_options)
add_test(NAME anomaly_geometry_contract COMMAND rsf_anomaly_geometry_test)
add_executable(rsf_anomaly_geometry_hooks_test "${CMAKE_CURRENT_LIST_DIR}/anomaly_geometry_hooks.cpp" ${RSF_ANOMALY_GEOMETRY_SOURCES})
target_include_directories(rsf_anomaly_geometry_hooks_test PRIVATE "${PROJECT_SOURCE_DIR}/games/anomaly/src")
target_compile_definitions(rsf_anomaly_geometry_hooks_test PRIVATE RSF_ANOMALY_GEOMETRY_TESTING)
target_link_libraries(rsf_anomaly_geometry_hooks_test PRIVATE minhook rsf_build_options)
add_test(NAME anomaly_geometry_hooks_contract COMMAND rsf_anomaly_geometry_hooks_test)
set_tests_properties(anomaly_geometry_contract anomaly_geometry_hooks_contract PROPERTIES TIMEOUT 60)
