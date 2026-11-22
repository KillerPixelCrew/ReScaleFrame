add_library(rsf_game_project_wingman SHARED src/native_platform.cpp)
rsf_add_rust_static(rsf_wingman_rust rsf_rust_wingman rsf-project-wingman rsf_project_wingman)
target_link_libraries(rsf_game_project_wingman PRIVATE
    rsf_wingman_rust ReScaleFrame::GameSDK ReScaleFrame::RuntimeCommon minhook rsf_build_options)
set_target_properties(rsf_game_project_wingman PROPERTIES
    OUTPUT_NAME "ReScaleFrame.Game.ProjectWingman")
