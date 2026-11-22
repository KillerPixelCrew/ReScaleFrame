// SPDX-License-Identifier: MIT
#include "register_types.h"
#include "rsf_godot_engine.h"
#include "host.h"
#include "core/config/engine.h"
#include "core/object/class_db.h"

namespace {
ReScaleFrameBridge *bridge = nullptr;

void register_bridge_class() {
    const ClassDB::APIType previous_api = ClassDB::get_current_api();
    ClassDB::set_current_api(ClassDB::API_NONE);
    GDREGISTER_CLASS(ReScaleFrameBridge);
    ClassDB::set_current_api(previous_api);
}

void publish_bridge() {
    bridge = memnew(ReScaleFrameBridge);
    Engine::get_singleton()->add_singleton(Engine::Singleton("ReScaleFrame", bridge));
}

void unpublish_bridge() {
    Engine::get_singleton()->remove_singleton("ReScaleFrame");
    memdelete(bridge);
    bridge = nullptr;
}
}

void initialize_rescaleframe_module(ModuleInitializationLevel level) {
    if (level == MODULE_INITIALIZATION_LEVEL_SCENE) {
        rsf_godot_module_initialize(register_bridge_class, publish_bridge);
    }
}
void uninitialize_rescaleframe_module(ModuleInitializationLevel level) {
    if (level == MODULE_INITIALIZATION_LEVEL_SCENE) {
        rsf_godot_module_uninitialize(RSFGodot::shutdown, unpublish_bridge);
    }
}
