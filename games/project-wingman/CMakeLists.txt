add_library(rsf_game_project_wingman SHARED src/plugin.cpp src/native_observer.cpp src/native_execution.cpp src/native_camera.cpp)
target_link_libraries(rsf_game_project_wingman PRIVATE
    ReScaleFrame::GameSDK ReScaleFrame::RuntimeCommon minhook rsf_build_options)
set_target_properties(rsf_game_project_wingman PROPERTIES
    OUTPUT_NAME "ReScaleFrame.Game.ProjectWingman")
