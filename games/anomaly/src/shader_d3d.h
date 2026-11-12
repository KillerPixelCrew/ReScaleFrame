// SPDX-License-Identifier: GPL-3.0-only
#pragma once
// d3dcompiler implementations of shader_loader.h's Compiler and Reflector. Use the compiler the engine itself
// imports (motion_profile.h engine_compiler_module, d3dcompiler_43.dll for 1.5.3): the loader proves its inputs by
// reproducing the engine's bytecode, which only that compiler does. The game has it loaded, so GetModuleHandle finds
// it; a host without it (tests) loads System32's copy. Reflection always uses System32's d3dcompiler_47.
#include "shader_loader.h"
#include <windows.h>

namespace rsf::anomaly::motion {
class D3DShaderApi final : public Compiler, public Reflector {
public:
    explicit D3DShaderApi(const wchar_t* module) noexcept;
    bool ready() const noexcept { return compile_ && reflect_; }
    // Compiles with pSourceName "" like the engine (motion_profile.h source_name_rva).
    bool compile(const std::filesystem::path& tree, std::span<const std::byte> root, const ShaderRequest&,
        std::vector<std::byte>& bytecode, std::string& error) override;
    bool reflect(std::span<const std::byte> bytecode, Reflection&) override;
private:
    FARPROC compile_{}, reflect_{};
};
}
