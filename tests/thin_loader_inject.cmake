# Opt-in isolated registration without editing the root or tests CMakeLists.txt.
cmake_language(DEFER DIRECTORY "${CMAKE_SOURCE_DIR}" CALL include
    "${CMAKE_SOURCE_DIR}/tests/thin_loader.cmake")
