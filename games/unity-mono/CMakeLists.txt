add_library(rsf_game_unity_mono SHARED src/plugin.cpp)
target_link_libraries(rsf_game_unity_mono PRIVATE ReScaleFrame::GameSDK rsf_build_options)
set_target_properties(rsf_game_unity_mono PROPERTIES
    OUTPUT_NAME "ReScaleFrame.Game.UnityMono")
