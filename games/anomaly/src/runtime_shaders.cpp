// SPDX-License-Identifier: MIT
#include "runtime_shaders.h"
#include "native_memory.h"
#include <d3dcompiler.h>
#include <cstring>

namespace rsf::anomaly::motion {
struct Includer {
    void* table;
    bool (*open)(void*, const char*, const void**, uint32_t*);
    bool (*copy)(void*, const void*, size_t);
    void (*close)(void*, const void*);
};
static_assert(sizeof(Includer) == 32);

extern "C" void* rsf_anomaly_runtime_pack_open(const uint16_t*, const uint16_t*, void*, const Includer*,
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

void* open_runtime_pack(const wchar_t* pack, const wchar_t* cache,
    std::byte* image, const MotionProfile& profile, std::string& error) {
    // The guarded D3DCompile setup loads the stateless includer's vtable with lea rax,[rip+disp32].
    const auto* lea = image + profile.d3dcompile_setup_rva + 6;
    unsigned char opcode[3]{};
    int32_t displacement{};
    if (!guarded_copy(opcode, lea, sizeof(opcode)) || std::memcmp(opcode, "\x48\x8d\x05", sizeof(opcode)) != 0 ||
        !guarded_copy(&displacement, lea + 3, sizeof(displacement))) {
        error = "include table instruction";
        return nullptr;
    }
    void* table = const_cast<std::byte*>(lea + 7 + displacement);
    void* functions[2]{};
    unsigned char open_bytes[9]{}, close_bytes[8]{};
    if (!guarded_copy(functions, table, sizeof(functions)) ||
        !guarded_copy(open_bytes, functions[0], sizeof(open_bytes)) ||
        !guarded_copy(close_bytes, functions[1], sizeof(close_bytes)) ||
        std::memcmp(open_bytes, "\x40\x53\x48\x81\xec\x40\x02\x00\x00", sizeof(open_bytes)) ||
        std::memcmp(close_bytes, "\x48\x83\xec\x28\x48\x85\xd2\x74", sizeof(close_bytes))) {
        error = "engine includer entries";
        return nullptr;
    }
    const auto compiler = GetModuleHandleW(engine_compiler_module);
    const auto fn = compiler ? GetProcAddress(compiler, "D3DCompile") : nullptr;
    const Includer includer{table, include_open, guarded_copy, include_close};
    char message[512]{};
    auto* result = rsf_anomaly_runtime_pack_open(reinterpret_cast<const uint16_t*>(pack),
        reinterpret_cast<const uint16_t*>(cache), reinterpret_cast<void*>(fn), &includer, message, sizeof(message));
    if (!result) error = message;
    return result;
}
}
