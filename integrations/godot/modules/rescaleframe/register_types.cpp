// SPDX-License-Identifier: GPL-3.0-only
#include "register_types.h"
#include "rsf_godot_engine.h"
#include "core/config/engine.h"
#include "core/object/class_db.h"

static ReScaleFrameBridge *bridge = nullptr;

void initialize_rescaleframe_module(ModuleInitializationLevel level) {
    if (level == MODULE_INITIALIZATION_LEVEL_SERVERS) RSFGodot::boot();
    if (level == MODULE_INITIALIZATION_LEVEL_SCENE) {
        const ClassDB::APIType previous_api = ClassDB::get_current_api();
        ClassDB::set_current_api(ClassDB::API_NONE);
        GDREGISTER_CLASS(ReScaleFrameBridge);
        ClassDB::set_current_api(previous_api);
        bridge = memnew(ReScaleFrameBridge);
        Engine::get_singleton()->add_singleton(Engine::Singleton("ReScaleFrame", bridge));
    }
}
void uninitialize_rescaleframe_module(ModuleInitializationLevel level) {
    if (level == MODULE_INITIALIZATION_LEVEL_SCENE) {
        RSFGodot::shutdown();
        Engine::get_singleton()->remove_singleton("ReScaleFrame");
        memdelete(bridge);
        bridge = nullptr;
    }
}
