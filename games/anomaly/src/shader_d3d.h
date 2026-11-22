// SPDX-License-Identifier: MIT
#pragma once
#include "shader_loader_platform.h"
#include <windows.h>

namespace rsf::anomaly::motion {
// C++ D3D facades for the deployed-tree fixtures. The live path uses Rust snapshots.
extern "C" bool rsf_anomaly_shader_reflect(FARPROC, rsf_anomaly_shader_bytes, void*,
    bool (*)(void*, const rsf_anomaly_shader_reflection*)) noexcept;

class D3DShaderReflector : public Reflector {
public:
    explicit D3DShaderReflector(FARPROC entry) noexcept : reflect_(entry) {}
    bool reflect(std::span<const std::byte> bytecode, Reflection&) override;
protected:
    FARPROC reflect_{};
};

struct ShaderD3DModules;
// Deployed-tree fixture facade. Rust owns the selected compiler and reflector references.
class D3DShaderApi final : public Compiler, public D3DShaderReflector {
public:
    explicit D3DShaderApi(const wchar_t* module) noexcept;
    ~D3DShaderApi() override;
    D3DShaderApi(const D3DShaderApi&) = delete;
    D3DShaderApi& operator=(const D3DShaderApi&) = delete;
    bool ready() const noexcept { return compile_ && reflect_; }
    // Compiles with pSourceName "" like the engine (motion_profile.h source_name_rva).
    bool compile(const std::filesystem::path& tree, std::span<const std::byte> root, const ShaderRequest&,
        std::vector<std::byte>& bytecode, std::string& error) override;
private:
    ShaderD3DModules* modules_{};
    FARPROC compile_{};
};
}
