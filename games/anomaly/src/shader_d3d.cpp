// SPDX-License-Identifier: GPL-3.0-only
#include "shader_d3d.h"
#include <d3d11shader.h>
#include <d3dcompiler.h>
#include <algorithm>
#include <cstring>
#include <new>
#include <fstream>
#include <iterator>

namespace rsf::anomaly::motion {
namespace {
// Same lookup order as the engine includer (r4.cpp:1170): shader path ("r3\") + name, then the name directly.
class Includer final : public ID3DInclude {
public:
    explicit Includer(std::filesystem::path tree) : tree_(std::move(tree)) {}
    HRESULT __stdcall Open(D3D_INCLUDE_TYPE, LPCSTR name, LPCVOID, LPCVOID* data, UINT* bytes) override
    {
        if (!name || !data || !bytes) return E_INVALIDARG;
        for (const auto& path : {tree_ / "r3" / name, tree_ / name}) {
            std::ifstream file(path, std::ios::binary);
            if (!file) continue;
            std::vector<char> content((std::istreambuf_iterator<char>(file)), std::istreambuf_iterator<char>());
            auto* copy = new (std::nothrow) char[content.size() + 1];
            if (!copy) return E_OUTOFMEMORY;
            if (!content.empty()) std::memcpy(copy, content.data(), content.size());
            copy[content.size()] = 0;
            *data = copy;
            *bytes = static_cast<UINT>(content.size());
            return S_OK;
        }
        return E_FAIL;
    }
    HRESULT __stdcall Close(LPCVOID data) override
    {
        delete[] static_cast<const char*>(data);
        return S_OK;
    }
private:
    std::filesystem::path tree_;
};
template<class T> struct Releaser {
    T* p{};
    ~Releaser() { if (p) p->Release(); }
};
void parameters(ID3D11ShaderReflection* reflection, UINT count, bool input, std::vector<Parameter>& out)
{
    for (UINT i = 0; i < count; ++i) {
        D3D11_SIGNATURE_PARAMETER_DESC desc{};
        if (FAILED(input ? reflection->GetInputParameterDesc(i, &desc) : reflection->GetOutputParameterDesc(i, &desc))) continue;
        out.push_back({desc.SemanticName ? desc.SemanticName : "", desc.SemanticIndex, desc.Register,
            static_cast<std::uint32_t>(desc.SystemValueType), static_cast<std::uint32_t>(desc.ComponentType), desc.Mask});
    }
}
}

D3DShaderApi::D3DShaderApi(const wchar_t* name) noexcept
{
    if (!name) return;
    HMODULE module = GetModuleHandleW(name);
    if (!module) module = LoadLibraryExW(name, nullptr, LOAD_LIBRARY_SEARCH_SYSTEM32);
    if (module) compile_ = GetProcAddress(module, "D3DCompile");
    // Reflection only reads the DXBC container, which every compiler version writes the same way. d3dcompiler_43's
    // D3DReflect answers only the June 2010 IID_ID3D11ShaderReflection, so reflect with System32's _47 (the interface
    // this file is compiled against) regardless of which compiler produced the bytecode.
    if (HMODULE reflector = LoadLibraryExW(L"d3dcompiler_47.dll", nullptr, LOAD_LIBRARY_SEARCH_SYSTEM32))
        reflect_ = GetProcAddress(reflector, "D3DReflect");
}

bool D3DShaderApi::compile(const std::filesystem::path& tree, std::span<const std::byte> root, const ShaderRequest& request,
    std::vector<std::byte>& bytecode, std::string& error)
{
    bytecode.clear();
    if (!compile_) { error = "D3DCompile unavailable"; return false; }
    std::vector<D3D_SHADER_MACRO> macros;
    macros.reserve(request.defines.size() + 1);
    for (const auto& define : request.defines) macros.push_back({define.name.c_str(), define.value.c_str()});
    macros.push_back({nullptr, nullptr});
    Includer includer(tree);
    Releaser<ID3DBlob> code, errors;
    const auto compile = reinterpret_cast<decltype(&D3DCompile)>(reinterpret_cast<void*>(compile_));
    const HRESULT hr = compile(root.data(), root.size(), "", macros.data(), &includer, request.entry.c_str(),
        request.target.c_str(), request.flags, 0, &code.p, &errors.p);
    if (FAILED(hr) || !code.p) {
        error = "D3DCompile 0x" + std::to_string(static_cast<unsigned long>(hr));
        if (errors.p) error.append(": ").append(static_cast<const char*>(errors.p->GetBufferPointer()),
            std::min<std::size_t>(errors.p->GetBufferSize(), 400));
        return false;
    }
    const auto* data = static_cast<const std::byte*>(code.p->GetBufferPointer());
    bytecode.assign(data, data + code.p->GetBufferSize());
    return true;
}

bool D3DShaderApi::reflect(std::span<const std::byte> bytecode, Reflection& out)
{
    out = {};
    if (!reflect_ || bytecode.empty()) return false;
    const auto reflect = reinterpret_cast<decltype(&D3DReflect)>(reinterpret_cast<void*>(reflect_));
    Releaser<ID3D11ShaderReflection> reflection;
    if (FAILED(reflect(bytecode.data(), bytecode.size(), IID_ID3D11ShaderReflection, reinterpret_cast<void**>(&reflection.p))) ||
        !reflection.p) return false;
    D3D11_SHADER_DESC desc{};
    if (FAILED(reflection.p->GetDesc(&desc))) return false;
    out.version = desc.Version;
    parameters(reflection.p, desc.InputParameters, true, out.inputs);
    parameters(reflection.p, desc.OutputParameters, false, out.outputs);
    for (UINT i = 0; i < desc.BoundResources; ++i) {
        D3D11_SHADER_INPUT_BIND_DESC bind{};
        if (FAILED(reflection.p->GetResourceBindingDesc(i, &bind))) return false;
        out.bindings.push_back({bind.Name ? bind.Name : "", static_cast<std::uint32_t>(bind.Type), bind.BindPoint,
            bind.BindCount, static_cast<std::uint32_t>(bind.ReturnType), static_cast<std::uint32_t>(bind.Dimension)});
    }
    for (UINT i = 0; i < desc.ConstantBuffers; ++i) {
        auto* buffer = reflection.p->GetConstantBufferByIndex(i);
        D3D11_SHADER_BUFFER_DESC bdesc{};
        if (!buffer || FAILED(buffer->GetDesc(&bdesc))) return false;
        Buffer item{bdesc.Name ? bdesc.Name : "", bdesc.Size, {}};
        for (UINT v = 0; v < bdesc.Variables; ++v) {
            auto* variable = buffer->GetVariableByIndex(v);
            D3D11_SHADER_VARIABLE_DESC vdesc{};
            if (!variable || FAILED(variable->GetDesc(&vdesc))) return false;
            item.variables.push_back({vdesc.Name ? vdesc.Name : "", vdesc.StartOffset, vdesc.Size});
        }
        out.buffers.push_back(std::move(item));
    }
    return true;
}
}
