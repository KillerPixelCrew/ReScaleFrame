# Session, log, GLFW message observer, camera packet, settings and status policy are Rust
# (native/lib.rs). jni.cpp keeps the JNI exports and lends the linked Runtime through
# native/minecraft_platform.h. The archive needs no JDK, so the GL interop test links it too.
rsf_add_rust_static(rsf_minecraft_rust rsf_rust_minecraft rsf-minecraft rsf_minecraft)
set(RSF_JAVA_HOME "$ENV{JAVA_HOME}" CACHE PATH "JDK providing JNI headers for Minecraft")
if(EXISTS "${RSF_JAVA_HOME}/include/jni.h")
    add_library(rsf_minecraft SHARED native/jni.cpp)
    target_include_directories(rsf_minecraft PRIVATE "${RSF_JAVA_HOME}/include" "${RSF_JAVA_HOME}/include/win32")
    target_link_libraries(rsf_minecraft PRIVATE rsf_minecraft_rust ReScaleFrame::OrchestratorCore rsf_build_options)
    set_target_properties(rsf_minecraft PROPERTIES OUTPUT_NAME "ReScaleFrame.Minecraft")
endif()
