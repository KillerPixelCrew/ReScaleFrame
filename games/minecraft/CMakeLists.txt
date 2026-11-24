# The Rust archive owns the JNI bridge and the observer used by the GL interop fixture. It carries the
# embedded Runtime, built with the Runtime archive's features, so the DLL takes the Runtime's C leaves
# without that archive (RSF_EMBEDDED_ARCHIVES).
get_property(rsf_runtime_embed_args GLOBAL PROPERTY RSF_RUNTIME_EMBED_ARGS)
rsf_add_rust_static(rsf_minecraft_rust rsf_rust_minecraft rsf-minecraft rsf_minecraft ${rsf_runtime_embed_args})
add_library(rsf_minecraft SHARED native/jni.def)
target_link_libraries(rsf_minecraft PRIVATE rsf_minecraft_rust ReScaleFrame::OrchestratorCore rsf_build_options)
set_target_properties(rsf_minecraft PROPERTIES OUTPUT_NAME "ReScaleFrame.Minecraft" LINKER_LANGUAGE CXX
    RSF_EMBEDDED_ARCHIVES "rsf_orchestrator_rust;rsf_common_rust")
