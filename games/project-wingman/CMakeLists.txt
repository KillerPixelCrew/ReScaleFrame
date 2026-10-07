# Detection and inactive Game SDK lifecycle scaffold; no renderer/backend libraries are linked.
add_library(rsf_game_project_wingman SHARED src/plugin.cpp)
target_link_libraries(rsf_game_project_wingman PRIVATE ReScaleFrame::GameSDK rsf_build_options)
set_target_properties(rsf_game_project_wingman PROPERTIES
    OUTPUT_NAME "ReScaleFrame.Game.ProjectWingman")
