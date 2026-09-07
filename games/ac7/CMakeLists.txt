# The view buffer reader is a library rather than part of the plugin so it can be tested without
# loading a DLL, and so the orchestrator can use it once the two are wired together.
add_library(rsf_ac7_view STATIC src/view_uniforms.cpp)
target_include_directories(rsf_ac7_view PUBLIC include)
target_link_libraries(rsf_ac7_view PRIVATE rsf_build_options)
add_library(ReScaleFrame::AC7View ALIAS rsf_ac7_view)

add_library(rsf_game_ac7 SHARED src/plugin.cpp)
target_link_libraries(rsf_game_ac7 PRIVATE ReScaleFrame::GameSDK rsf_ac7_view rsf_build_options)
set_target_properties(rsf_game_ac7 PROPERTIES OUTPUT_NAME "ReScaleFrame.Game.AC7")

# Capture-derived scene selection stays with the game; graphics supplies only draw observations.
add_library(rsf_ac7_scene_color STATIC src/scene_color.cpp)
target_include_directories(rsf_ac7_scene_color PUBLIC include)
target_link_libraries(rsf_ac7_scene_color PUBLIC ReScaleFrame::Graphics PRIVATE rsf_build_options)
add_library(ReScaleFrame::AC7SceneColor ALIAS rsf_ac7_scene_color)
