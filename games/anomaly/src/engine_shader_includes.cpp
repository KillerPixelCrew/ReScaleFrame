// SPDX-License-Identifier: MIT
#include "build_profile.h"
#include "motion_profile.h"
#include "motion_stage_platform.h"
#include "native_memory.h"
#include <d3dcompiler.h>

namespace rsf::anomaly::motion {
struct Includer {
    void* table;
    bool (*open)(void*, const char*, const void**, uint32_t*);
    bool (*copy)(void*, const void*, size_t);
    void (*close)(void*, const void*);
};
static_assert(sizeof(Includer) == 32);

extern "C" void* rsf_anomaly_runtime_pack_open(const uint16_t*, const uint16_t*, const uint16_t*, uintptr_t, uint32_t, const Includer*,
    char*, size_t) noexcept;

namespace {
bool include_open(void* table, const char* name, const void** data, uint32_t* size) noexcept {
    __try {
        auto* object = reinterpret_cast<ID3DInclude*>(&table);
        return SUCCEEDED(object->Open(D3D_INCLUDE_LOCAL, name, nullptr, data, size));
    } __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
}

void include_close(void* table, const void* data) noexcept {
    __try { reinterpret_cast<ID3DInclude*>(&table)->Close(data); }
    __except (EXCEPTION_EXECUTE_HANDLER) {}
}
}

void* open_shader_pack(void* image, const void* profile, const wchar_t* pack, const wchar_t* cache,
    char* error, size_t capacity) noexcept {
    const Includer includer{nullptr, include_open, guarded_copy, include_close};
    static_assert(sizeof(wchar_t) == sizeof(uint16_t));
    const auto& motion = *static_cast<const BuildProfile*>(profile)->motion;
    return rsf_anomaly_runtime_pack_open(reinterpret_cast<const uint16_t*>(pack),
        reinterpret_cast<const uint16_t*>(cache), reinterpret_cast<const uint16_t*>(engine_compiler_module),
        reinterpret_cast<uintptr_t>(image), motion.d3dcompile_setup_rva, &includer, error, capacity);
}
}
