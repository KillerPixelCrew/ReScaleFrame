add_library(rsf_game_anomaly SHARED src/plugin.cpp src/bootstrap.cpp src/native_observer.cpp src/native_hooks.cpp
    src/native_geometry.cpp src/geometry_feed.cpp
    src/overlay_input_gate.cpp src/overlay_hooks.cpp src/overlay_service.cpp)
target_include_directories(rsf_game_anomaly PRIVATE "${PROJECT_SOURCE_DIR}/runtime/orchestrator/include")
target_link_libraries(rsf_game_anomaly PRIVATE ReScaleFrame::GameSDK ReScaleFrame::Contract ReScaleFrame::RuntimeCommon minhook rsf_build_options)
set_target_properties(rsf_game_anomaly PROPERTIES OUTPUT_NAME "ReScaleFrame.Game.Anomaly")
