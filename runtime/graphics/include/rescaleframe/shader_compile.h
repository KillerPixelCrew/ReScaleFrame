/* SPDX-License-Identifier: GPL-3.0-only */
#pragma once
/* One HLSL compile, header-only so a module that does not link rsf_graphics (the AC7 render scope)
   can use it too.

   d3dcompiler_47 is loaded from System32 on each call rather than linked, because the callers sit in
   injected modules where the game's own search path must not decide which compiler runs. The
   bytecode comes back as a copy: the blobs live in the compiler's heap and vtables, so both are
   released before the module is freed, and nothing handed to the caller depends on it staying
   loaded.

   Flags are the caller's choice and have always differed: D3DCOMPILE_ENABLE_STRICTNESS for most
   passes (the default here), 0 where a shader leans on legacy behaviour the strict mode rejects.
   The default is the right start for new shaders. */
#include <windows.h>
#include <d3d11.h>
#include <d3dcompiler.h>
#include <cstddef>
#include <cstdint>
#include <string>
#include <vector>
#include <rescaleframe/log.h>

namespace rsf {
inline bool compile_shader(const char* source, size_t length, const char* name, const char* entry,
                           const char* profile, std::vector<uint8_t>& bytecode,
                           void (*log)(void*, const char*) = nullptr, void* user = nullptr,
                           UINT flags = D3DCOMPILE_ENABLE_STRICTNESS)
{
    bytecode.clear();
    HMODULE module = LoadLibraryExW(L"d3dcompiler_47.dll", nullptr, LOAD_LIBRARY_SEARCH_SYSTEM32);
    if (!module) {
        say(log, user, "shader %s: d3dcompiler_47 is not available", name);
        return false;
    }
    using Compile = decltype(&D3DCompile);
    const auto compile = reinterpret_cast<Compile>(reinterpret_cast<void*>(GetProcAddress(module, "D3DCompile")));
    bool ok = false;
    if (compile) {
        ID3DBlob* code = nullptr;
        ID3DBlob* errors = nullptr;
        const HRESULT result = compile(source, length, name, nullptr, nullptr, entry, profile, flags, 0,
                                       &code, &errors);
        if (SUCCEEDED(result) && code) {
            const auto* data = static_cast<const uint8_t*>(code->GetBufferPointer());
            bytecode.assign(data, data + code->GetBufferSize());
            ok = true;
        } else {
            say(log, user, "shader %s (%s): compile failed 0x%08lx %s", name, entry,
                static_cast<unsigned long>(result),
                errors ? static_cast<const char*>(errors->GetBufferPointer()) : "");
        }
        if (code) code->Release();
        if (errors) errors->Release();
    }
    FreeLibrary(module);
    return ok;
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
