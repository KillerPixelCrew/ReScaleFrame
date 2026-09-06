# Research carrier DLL. Its file name is the mechanism, so it is deliberately not ReScaleFrame.*.
add_library(rsf_proxy_dinput8 SHARED src/dinput8_proxy.c)
target_link_libraries(rsf_proxy_dinput8 PRIVATE ReScaleFrame::LoaderDiagnostics rsf_build_options)
target_compile_definitions(rsf_proxy_dinput8 PRIVATE _CRT_SECURE_NO_WARNINGS)
set_target_properties(rsf_proxy_dinput8 PROPERTIES OUTPUT_NAME "dinput8" PREFIX "")
