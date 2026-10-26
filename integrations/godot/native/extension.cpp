// SPDX-License-Identifier: GPL-3.0-only
#include "effect.h"
#include "runtime_control.h"
#include <godot_cpp/core/class_db.hpp>
#include <godot_cpp/godot.hpp>

namespace {
void initialize(godot::ModuleInitializationLevel level)
{
    if (level == godot::MODULE_INITIALIZATION_LEVEL_SCENE) {
        GDREGISTER_CLASS(rsf::ReScaleFrameEffect);
        GDREGISTER_CLASS(rsf::ReScaleFrameRuntime);
    }
}
}

extern "C" GDExtensionBool GDE_EXPORT rsf_godot_initialize(
    GDExtensionInterfaceGetProcAddress get_proc_address,
    GDExtensionClassLibraryPtr library, GDExtensionInitialization* initialization)
{
    godot::GDExtensionBinding::InitObject init(get_proc_address, library, initialization);
    init.register_initializer(initialize);
    init.set_minimum_library_initialization_level(godot::MODULE_INITIALIZATION_LEVEL_SCENE);
    return init.init();
}
