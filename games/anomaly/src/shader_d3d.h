// SPDX-License-Identifier: MIT
#pragma once
#include "shader_loader_platform.h"
#include <windows.h>

namespace rsf::anomaly::motion {
struct ShaderD3DModules;
// Deployed-tree fixture facade. Rust owns the selected compiler and reflector references.
class D3DShaderApi final : public Compiler, public Reflector {
public:
    explicit D3DShaderApi(const wchar_t* module) noexcept;
    ~D3DShaderApi() override;
    D3DShaderApi(const D3DShaderApi&) = delete;
    D3DShaderApi& operator=(const D3DShaderApi&) = delete;
    bool ready() const noexcept { return compile_ && reflect_; }
    // Compiles with pSourceName "" like the engine (motion_profile.h source_name_rva).
    bool compile(const std::filesystem::path& tree, std::span<const std::byte> root, const ShaderRequest&,
        std::vector<std::byte>& bytecode, std::string& error) override;
    bool reflect(std::span<const std::byte> bytecode, Reflection&) override;
private:
    ShaderD3DModules* modules_{};
    FARPROC compile_{};
    FARPROC reflect_{};
};
}
