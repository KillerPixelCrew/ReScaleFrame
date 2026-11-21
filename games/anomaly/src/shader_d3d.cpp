// SPDX-License-Identifier: MIT
#include "shader_d3d.h"
#include <d3d11shader.h>
#include <d3dcompiler.h>
#include <algorithm>
#include <exception>
#include <new>
#include <utility>

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
// Private synchronous ABI for shader_loader/reflection.rs.
enum class Signature : uint8_t { Input = 0, Output = 1, Patch = 2 };
enum class ReflectionStep : uint8_t { Copied = 0, Unavailable = 1, CopyFailed = 2 };

struct ReflectionCounts {
    uint32_t inputs, outputs, patch, bindings, buffers;
};
struct ReflectionOperations {
    ReflectionStep (*parameter)(void*, Signature, uint32_t) noexcept;
    ReflectionStep (*binding)(void*, uint32_t) noexcept;
    ReflectionStep (*begin_buffer)(void*, uint32_t, uint32_t*) noexcept;
    ReflectionStep (*variable)(void*, uint32_t) noexcept;
    ReflectionStep (*finish_buffer)(void*) noexcept;
};
static_assert(sizeof(ReflectionCounts) == 20 && sizeof(ReflectionOperations) == 40);

// COM owns the descriptor strings and child interfaces. Copy each row while the parent is alive.
struct ReflectionCopy {
    ID3D11ShaderReflection& reflection;
    Reflection& output;
    ID3D11ShaderReflectionConstantBuffer* buffer{};
    Buffer pending{};
    std::exception_ptr exception{};

    static ReflectionStep copy_failed(void* user) noexcept
    {
        // Rethrow on the native side so allocation failures never unwind through Rust.
        static_cast<ReflectionCopy*>(user)->exception = std::current_exception();
        return ReflectionStep::CopyFailed;
    }

    static ReflectionStep parameter(void* user, Signature which, uint32_t index) noexcept try
    {
        auto& self = *static_cast<ReflectionCopy*>(user);
        D3D11_SIGNATURE_PARAMETER_DESC desc{};
        const auto append = [&](HRESULT hr, std::vector<Parameter>& out) {
            if (FAILED(hr)) return ReflectionStep::Unavailable;
            out.push_back({desc.SemanticName ? desc.SemanticName : "", desc.SemanticIndex, desc.Register,
                static_cast<uint32_t>(desc.SystemValueType), static_cast<uint32_t>(desc.ComponentType), desc.Mask});
            return ReflectionStep::Copied;
        };
        switch (which) {
        case Signature::Input:
            return append(self.reflection.GetInputParameterDesc(index, &desc), self.output.inputs);
        case Signature::Output:
            return append(self.reflection.GetOutputParameterDesc(index, &desc), self.output.outputs);
        case Signature::Patch:
            return append(self.reflection.GetPatchConstantParameterDesc(index, &desc), self.output.patch);
        }
        return ReflectionStep::CopyFailed;
    } catch (...) { return copy_failed(user); }

    static ReflectionStep binding(void* user, uint32_t index) noexcept try
    {
        auto& self = *static_cast<ReflectionCopy*>(user);
        D3D11_SHADER_INPUT_BIND_DESC bind{};
        if (FAILED(self.reflection.GetResourceBindingDesc(index, &bind))) return ReflectionStep::Unavailable;
        self.output.bindings.push_back({bind.Name ? bind.Name : "", static_cast<uint32_t>(bind.Type), bind.BindPoint,
            bind.BindCount, static_cast<uint32_t>(bind.ReturnType), static_cast<uint32_t>(bind.Dimension)});
        return ReflectionStep::Copied;
    } catch (...) { return copy_failed(user); }

    static ReflectionStep begin_buffer(void* user, uint32_t index, uint32_t* variables) noexcept try
    {
        auto& self = *static_cast<ReflectionCopy*>(user);
        self.buffer = self.reflection.GetConstantBufferByIndex(index);
        D3D11_SHADER_BUFFER_DESC desc{};
        if (!self.buffer || FAILED(self.buffer->GetDesc(&desc))) return ReflectionStep::Unavailable;
        self.pending = {desc.Name ? desc.Name : "", desc.Size, {}};
        *variables = desc.Variables;
        return ReflectionStep::Copied;
    } catch (...) { return copy_failed(user); }

    static ReflectionStep variable(void* user, uint32_t index) noexcept try
    {
        auto& self = *static_cast<ReflectionCopy*>(user);
        auto* shader_variable = self.buffer->GetVariableByIndex(index);
        D3D11_SHADER_VARIABLE_DESC desc{};
        if (!shader_variable || FAILED(shader_variable->GetDesc(&desc))) return ReflectionStep::Unavailable;
        self.pending.variables.push_back({desc.Name ? desc.Name : "", desc.StartOffset, desc.Size});
        return ReflectionStep::Copied;
    } catch (...) { return copy_failed(user); }

    static ReflectionStep finish_buffer(void* user) noexcept try
    {
        auto& self = *static_cast<ReflectionCopy*>(user);
        self.output.buffers.push_back(std::move(self.pending));
        return ReflectionStep::Copied;
    } catch (...) { return copy_failed(user); }
};

constexpr ReflectionOperations reflection_operations{
    ReflectionCopy::parameter, ReflectionCopy::binding, ReflectionCopy::begin_buffer,
    ReflectionCopy::variable, ReflectionCopy::finish_buffer,
};
}
extern "C" bool rsf_anomaly_shader_enumerate_reflection(
    void*, const ReflectionCounts*, const ReflectionOperations*) noexcept;

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
    const ReflectionCounts counts{desc.InputParameters, desc.OutputParameters, desc.PatchConstantParameters,
        desc.BoundResources, desc.ConstantBuffers};
    ReflectionCopy copy{*reflection.p, out};
    const bool ok = rsf_anomaly_shader_enumerate_reflection(&copy, &counts, &reflection_operations);
    if (copy.exception) std::rethrow_exception(copy.exception);
    return ok;
}
}
