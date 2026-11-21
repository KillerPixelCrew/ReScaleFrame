// SPDX-License-Identifier: MIT
#include "shader_d3d.h"
#include <d3d11shader.h>
#include <d3dcompiler.h>
#include <algorithm>
#include <new>

namespace rsf::anomaly::motion {
struct ShaderIncludes;
using OpenFile = HRESULT (*)(const void* tree, const char* prefix, const char* name, HANDLE*);
extern "C" {
ShaderD3DModules* rsf_anomaly_shader_d3d_load(const uint16_t*, FARPROC*, FARPROC*) noexcept;
void rsf_anomaly_shader_d3d_unload(ShaderD3DModules*) noexcept;
ShaderIncludes* rsf_anomaly_shader_includes_new() noexcept;
void rsf_anomaly_shader_includes_free(ShaderIncludes*) noexcept;
HRESULT rsf_anomaly_shader_include_open(ShaderIncludes*, const void*, const char*, OpenFile, const void**, UINT*) noexcept;
HRESULT rsf_anomaly_shader_include_close(ShaderIncludes*, const void*) noexcept;
}

namespace {
HRESULT open_file(const void* tree, const char* prefix, const char* name, HANDLE* out) noexcept try {
    // Keep the STL's narrow filename conversion and Windows root/drive/UNC append rules.
    // make_preferred also converts slashes in include names for extended-length trees.
    auto path = *static_cast<const std::filesystem::path*>(tree);
    if (prefix) path /= prefix;
    path /= std::filesystem::path(name).make_preferred();
    // Match the previous binary stream's read access and sharing on this native path.
    const HANDLE file = CreateFileW(path.c_str(), GENERIC_READ, FILE_SHARE_READ | FILE_SHARE_WRITE,
        nullptr, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, nullptr);
    if (file == INVALID_HANDLE_VALUE) return S_FALSE;
    *out = file;
    return S_OK;
} catch (const std::bad_alloc&) {
    return E_OUTOFMEMORY;
} catch (...) {
    return E_FAIL;
}

class Includer final : public ID3DInclude {
public:
    explicit Includer(const std::filesystem::path& tree) noexcept : tree_(tree) {}
    ~Includer() { rsf_anomaly_shader_includes_free(includes_); }
    Includer(const Includer&) = delete;
    Includer& operator=(const Includer&) = delete;
    HRESULT __stdcall Open(D3D_INCLUDE_TYPE, LPCSTR name, LPCVOID, LPCVOID* data, UINT* bytes) noexcept override
    {
        if (!includes_) return E_OUTOFMEMORY;
        return rsf_anomaly_shader_include_open(includes_, &tree_, name, open_file, data, bytes);
    }
    HRESULT __stdcall Close(LPCVOID data) noexcept override
    {
        if (!includes_) return E_INVALIDARG;
        return rsf_anomaly_shader_include_close(includes_, data);
    }
private:
    const std::filesystem::path& tree_;
    ShaderIncludes* includes_ = rsf_anomaly_shader_includes_new();
};
template<class T> struct Releaser {
    T* p{};
    ~Releaser() { if (p) p->Release(); }
};
enum class Signature { Input, Output, Patch };
void parameters(ID3D11ShaderReflection* reflection, UINT count, Signature which, std::vector<Parameter>& out)
{
    for (UINT i = 0; i < count; ++i) {
        D3D11_SIGNATURE_PARAMETER_DESC desc{};
        const HRESULT hr = which == Signature::Input ? reflection->GetInputParameterDesc(i, &desc)
                         : which == Signature::Output ? reflection->GetOutputParameterDesc(i, &desc)
                         : reflection->GetPatchConstantParameterDesc(i, &desc);
        if (FAILED(hr)) continue;
        out.push_back({desc.SemanticName ? desc.SemanticName : "", desc.SemanticIndex, desc.Register,
            static_cast<std::uint32_t>(desc.SystemValueType), static_cast<std::uint32_t>(desc.ComponentType), desc.Mask});
    }
}
}

D3DShaderApi::D3DShaderApi(const wchar_t* name) noexcept : D3DShaderReflector(nullptr)
{
    static_assert(sizeof(wchar_t) == sizeof(uint16_t));
    modules_ = rsf_anomaly_shader_d3d_load(reinterpret_cast<const uint16_t*>(name), &compile_, &reflect_);
}

D3DShaderApi::~D3DShaderApi()
{
    rsf_anomaly_shader_d3d_unload(modules_);
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

bool D3DShaderReflector::reflect(std::span<const std::byte> bytecode, Reflection& out)
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
    parameters(reflection.p, desc.InputParameters, Signature::Input, out.inputs);
    parameters(reflection.p, desc.OutputParameters, Signature::Output, out.outputs);
    parameters(reflection.p, desc.PatchConstantParameters, Signature::Patch, out.patch);
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
