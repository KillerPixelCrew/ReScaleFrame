add_library(rsf_game_ac7 SHARED src/plugin.cpp)
target_link_libraries(rsf_game_ac7 PRIVATE ReScaleFrame::GameSDK rsf_build_options)
set_target_properties(rsf_game_ac7 PROPERTIES OUTPUT_NAME "ReScaleFrame.Game.AC7")
