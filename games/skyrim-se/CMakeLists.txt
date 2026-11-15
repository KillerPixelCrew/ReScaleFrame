add_library(rsf_game_skyrim_se SHARED src/plugin.cpp src/cpu_events.cpp)
target_link_libraries(rsf_game_skyrim_se PRIVATE
    ReScaleFrame::GameSDK ReScaleFrame::RuntimeCommon minhook rsf_build_options)
set_target_properties(rsf_game_skyrim_se PROPERTIES OUTPUT_NAME "ReScaleFrame.Game.SkyrimSE")
