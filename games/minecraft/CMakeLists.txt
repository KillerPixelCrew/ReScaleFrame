# The Rust archive owns the JNI bridge and the observer used by the GL interop fixture. It carries the
# embedded Runtime, built with the Runtime archive's features, so the DLL takes the Runtime's C leaves
# without that archive (CARRIES, cmake/rust_static.cmake).
rsf_add_rust_static(rsf_minecraft_rust rsf_rust_minecraft rsf-minecraft rsf_minecraft
    CARRIES "rsf_orchestrator_rust;rsf_common_rust;rsf_graphics_rust")
add_library(rsf_minecraft SHARED native/jni.def)
target_link_libraries(rsf_minecraft PRIVATE rsf_minecraft_rust ReScaleFrame::OrchestratorCore rsf_build_options)
set_target_properties(rsf_minecraft PROPERTIES OUTPUT_NAME "ReScaleFrame.Minecraft" LINKER_LANGUAGE CXX)
