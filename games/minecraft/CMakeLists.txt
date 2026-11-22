# The Rust archive owns the JNI bridge and the observer used by the GL interop fixture.
rsf_add_rust_static(rsf_minecraft_rust rsf_rust_minecraft rsf-minecraft rsf_minecraft)
add_library(rsf_minecraft SHARED native/jni.def)
target_link_libraries(rsf_minecraft PRIVATE rsf_minecraft_rust ReScaleFrame::OrchestratorCore rsf_build_options)
set_target_properties(rsf_minecraft PROPERTIES OUTPUT_NAME "ReScaleFrame.Minecraft" LINKER_LANGUAGE CXX)
