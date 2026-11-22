/* SPDX-License-Identifier: MIT */
#pragma once
/* C++ compatibility wrappers. Rust owns System32 compiler loading and blob lifetimes. */
#include <windows.h>
#include <d3d11.h>
#include <d3dcompiler.h>
#include <wrl/client.h>
#include <cstddef>
#include <cstdint>
#include <string>
#include <vector>
#include <rescaleframe/log.h>

extern "C" int32_t rsf_compile_system_shader(const uint8_t*, size_t, const char*, const char*,
    const char*, uint32_t, void*, int32_t (*)(void*, const uint8_t*, size_t),
    int32_t (*)(void*, const uint8_t*, size_t));

namespace rsf {
inline bool compile_shader(const char* source, size_t length, const char* name, const char* entry,
                           const char* profile, std::vector<uint8_t>& bytecode,
                           void (*log)(void*, const char*) = nullptr, void* user = nullptr,
                           UINT flags = D3DCOMPILE_ENABLE_STRICTNESS)
{
    bytecode.clear();
    struct Reply {
        std::vector<uint8_t>& code;
        std::string errors;
    } reply{bytecode, {}};
    const auto result = rsf_compile_system_shader(reinterpret_cast<const uint8_t*>(source), length,
        name, entry, profile, flags, &reply,
        [](void* user, const uint8_t* data, size_t size) noexcept -> int32_t {
            try { static_cast<Reply*>(user)->code.assign(data, data + size); }
            catch (...) { return -1; }
            return 0;
        },
        [](void* user, const uint8_t* data, size_t size) noexcept -> int32_t {
            try { static_cast<Reply*>(user)->errors.assign(reinterpret_cast<const char*>(data), size); }
            catch (...) { return -1; }
            return 0;
        });
    if (result == 0) return true;
    if (result == 1) {
        rsf_logf(log, user, "shader %s: d3dcompiler_47 is not available", name);
    } else if (result < 0) {
        rsf_logf(log, user, "shader %s (%s): compile failed 0x%08lx %s", name, entry,
            static_cast<unsigned long>(static_cast<uint32_t>(result)), reply.errors.c_str());
    }
    return false;
}
inline bool compile_shader(const char* source, const char* name, const char* entry, const char* profile,
                           std::vector<uint8_t>& bytecode, void (*log)(void*, const char*) = nullptr,
                           void* user = nullptr, UINT flags = D3DCOMPILE_ENABLE_STRICTNESS)
{
    return compile_shader(source, std::char_traits<char>::length(source), name, entry, profile, bytecode,
                          log, user, flags);
}
/* Compile and create in one step, by profile family. */
inline bool compile_compute(ID3D11Device* device, const char* source, const char* name, const char* entry,
                            ID3D11ComputeShader** out, void (*log)(void*, const char*) = nullptr,
                            void* user = nullptr, UINT flags = D3DCOMPILE_ENABLE_STRICTNESS)
{
    std::vector<uint8_t> code;
    return compile_shader(source, name, entry, "cs_5_0", code, log, user, flags) &&
        SUCCEEDED(device->CreateComputeShader(code.data(), code.size(), nullptr, out));
}
inline bool compile_vertex(ID3D11Device* device, const char* source, const char* name, const char* entry,
                           ID3D11VertexShader** out, void (*log)(void*, const char*) = nullptr,
                           void* user = nullptr, UINT flags = D3DCOMPILE_ENABLE_STRICTNESS)
{
    std::vector<uint8_t> code;
    return compile_shader(source, name, entry, "vs_5_0", code, log, user, flags) &&
        SUCCEEDED(device->CreateVertexShader(code.data(), code.size(), nullptr, out));
}
inline bool compile_pixel(ID3D11Device* device, const char* source, const char* name, const char* entry,
                          ID3D11PixelShader** out, void (*log)(void*, const char*) = nullptr,
                          void* user = nullptr, UINT flags = D3DCOMPILE_ENABLE_STRICTNESS)
{
    std::vector<uint8_t> code;
    return compile_shader(source, name, entry, "ps_5_0", code, log, user, flags) &&
        SUCCEEDED(device->CreatePixelShader(code.data(), code.size(), nullptr, out));
}
}
