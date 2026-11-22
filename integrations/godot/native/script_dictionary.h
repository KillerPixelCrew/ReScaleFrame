// SPDX-License-Identifier: MIT
#pragma once
#include "extension_abi.h"
#include <godot_cpp/classes/rendering_server.hpp>
#include <godot_cpp/variant/dictionary.hpp>
#include <godot_cpp/variant/projection.hpp>
#include <godot_cpp/variant/transform3d.hpp>
#include <godot_cpp/variant/vector2i.hpp>

namespace rsf {
struct ScriptDictionary {
    godot::Dictionary& result;
    godot::RenderingServer* rendering;

    static void emit(void* user, const char* key, uint32_t kind, const rsf_godot_property* value) noexcept try
    {
        using namespace godot;
        auto& writer = *static_cast<ScriptDictionary*>(user);
        auto& result = writer.result;
        switch (kind) {
        case RSF_GODOT_PROPERTY_INTEGER: result[key] = value->integer; break;
        case RSF_GODOT_PROPERTY_BOOL: result[key] = value->integer != 0; break;
        case RSF_GODOT_PROPERTY_REAL: result[key] = value->real; break;
        case RSF_GODOT_PROPERTY_TEXT: result[key] = value->text; break;
        case RSF_GODOT_PROPERTY_EXTENT: result[key] = Vector2i(value->extent[0], value->extent[1]); break;
        case RSF_GODOT_PROPERTY_TRANSFORM: {
            Transform3D transform;
            for (int row = 0; row < 3; ++row) {
                for (int column = 0; column < 3; ++column)
                    transform.basis[row][column] = static_cast<real_t>(value->matrix[row * 3 + column]);
                transform.origin[row] = static_cast<real_t>(value->matrix[9 + row]);
            }
            result[key] = transform;
            break;
        }
        case RSF_GODOT_PROPERTY_PROJECTION: {
            Projection projection;
            for (int column = 0; column < 4; ++column)
                for (int row = 0; row < 4; ++row)
                    projection[column][row] = static_cast<real_t>(value->matrix[column * 4 + row]);
            result[key] = projection;
            break;
        }
        case RSF_GODOT_PROPERTY_RENDERER:
            result[key] = writer.rendering ? writer.rendering->get_current_rendering_method() : String();
            break;
        case RSF_GODOT_PROPERTY_DRIVER:
            result[key] = writer.rendering ? writer.rendering->get_current_rendering_driver_name() : String();
            break;
        }
    } catch (...) {}
};
}
