# The view buffer reader is a library rather than part of the plugin so it can be tested without
# loading a DLL, and so the orchestrator can use it once the two are wired together.
add_library(rsf_ac7_view STATIC src/view_uniforms.cpp)
target_include_directories(rsf_ac7_view PUBLIC include)
target_link_libraries(rsf_ac7_view PRIVATE rsf_build_options)
add_library(ReScaleFrame::AC7View ALIAS rsf_ac7_view)

add_library(rsf_game_ac7 SHARED src/plugin.cpp)
target_link_libraries(rsf_game_ac7 PRIVATE ReScaleFrame::GameSDK rsf_ac7_view rsf_ac7_render_scope rsf_build_options)
set_target_properties(rsf_game_ac7 PROPERTIES OUTPUT_NAME "ReScaleFrame.Game.AC7")

# Which draws are the interface is a game fact, so the rule lives here. It is a pure function over
# facts the frame tap shadows, which is what lets it be tested without a device.
add_library(rsf_ac7_ui_rules STATIC src/ui_rules.cpp)
target_include_directories(rsf_ac7_ui_rules PUBLIC include)
target_link_libraries(rsf_ac7_ui_rules PRIVATE ReScaleFrame::Graphics rsf_build_options)
add_library(ReScaleFrame::AC7UIRules ALIAS rsf_ac7_ui_rules)

# Capture-derived scene selection stays with the game; graphics supplies only draw observations.
add_library(rsf_ac7_scene_color STATIC src/scene_color.cpp)
target_include_directories(rsf_ac7_scene_color PUBLIC include)
target_link_libraries(rsf_ac7_scene_color PUBLIC ReScaleFrame::Graphics PRIVATE rsf_build_options)
add_library(ReScaleFrame::AC7SceneColor ALIAS rsf_ac7_scene_color)

add_library(rsf_ac7_motion_capture STATIC src/motion_capture.cpp)
target_include_directories(rsf_ac7_motion_capture PUBLIC include)
target_link_libraries(rsf_ac7_motion_capture PUBLIC ReScaleFrame::GameSDK PRIVATE ReScaleFrame::Graphics minhook rsf_build_options)
add_library(ReScaleFrame::AC7MotionCapture ALIAS rsf_ac7_motion_capture)

add_library(rsf_ac7_render_scope STATIC src/render_scope.cpp src/native_renderer.cpp src/truesky_depth.cpp src/truesky_motion.cpp src/contact_shadow.cpp)
target_include_directories(rsf_ac7_render_scope PUBLIC include)
target_link_libraries(rsf_ac7_render_scope PUBLIC ReScaleFrame::GameSDK PRIVATE rsf_ac7_view minhook rsf_build_options ReScaleFrame::RuntimeCommon)
# shader_compile.h is header-only, so the scope uses it without linking the Graphics library.
target_include_directories(rsf_ac7_render_scope PRIVATE "${CMAKE_SOURCE_DIR}/runtime/graphics/include")
add_library(ReScaleFrame::AC7RenderScope ALIAS rsf_ac7_render_scope)
