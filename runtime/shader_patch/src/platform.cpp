// SPDX-License-Identifier: MIT
// yyjson, D3DCompiler, D3D11 and MinHook leaves of the Rust shader patch owner.
#include "platform.h"
#include <windows.h>
#include <d3d11.h>
#include <d3d11shader.h>
#include <d3dcompiler.h>
#include <wrl/client.h>
#include <MinHook.h>
#include <climits>
#include <cstring>
#include <vector>

using Microsoft::WRL::ComPtr;

static_assert(RSF_PATCH_USAGE_DYNAMIC == static_cast<int>(D3D11_USAGE_DYNAMIC));
static_assert(RSF_PATCH_CPU_ACCESS_WRITE == static_cast<int>(D3D11_CPU_ACCESS_WRITE));
static_assert(RSF_PATCH_CONSTANT_SLOTS == static_cast<int>(D3D11_COMMONSHADER_CONSTANT_BUFFER_API_SLOT_COUNT));

extern "C" void rsf_shader_patch_bind(const rsf_patch_platform*);
extern "C" int32_t rsf_shader_patch_query(rsf_shader_patch_api*);
extern "C" int32_t rsf_shader_patch_transform(rsf_patch_bytes, const rsf_patch_bytes*,
    const rsf_patch_bytes*, size_t, void*, rsf_patch_file_sink, char*);
extern "C" int32_t rsf_shader_patch_rewrite_dxbc(rsf_patch_bytes, const char*, const uint32_t*,
    size_t, const uint32_t*, size_t, void*, rsf_patch_sink, char*);

namespace {
class Includes final : public ID3DInclude {
    const rsf_patch_compile& call_;
public:
    explicit Includes(const rsf_patch_compile& call) : call_(call) {}
    HRESULT __stdcall Open(D3D_INCLUDE_TYPE, LPCSTR name, LPCVOID, LPCVOID* data, UINT* size) override
    {
        if (!name || !data || !size) return E_INVALIDARG;
        const void* bytes{};
        size_t length{};
        if (call_.include(call_.user, name, &bytes, &length) != 0 || length > UINT_MAX) return E_FAIL;
        *data = bytes;
        *size = static_cast<UINT>(length);
        return S_OK;
    }
    HRESULT __stdcall Close(LPCVOID) override { return S_OK; }
};

int32_t compile(const rsf_patch_compile* call) noexcept try
{
    std::vector<D3D_SHADER_MACRO> defines;
    for (size_t i = 0; i < call->define_count; ++i)
        defines.push_back({call->defines[i].name, call->defines[i].value});
    defines.push_back({nullptr, nullptr});
    Includes includes(*call);
    ComPtr<ID3DBlob> code, errors;
    const auto compiler = reinterpret_cast<decltype(&D3DCompile)>(call->compiler);
    const auto hr = compiler(call->source.data, call->source.size, "", defines.data(), &includes,
        call->entry, call->target, call->flags, 0, &code, &errors);
    if (code) call->code(call->user, code->GetBufferPointer(), code->GetBufferSize());
    if (errors) call->errors(call->user, errors->GetBufferPointer(), errors->GetBufferSize());
    return hr;
} catch (...) { return E_FAIL; }

int32_t reflect(rsf_patch_bytes code, const rsf_patch_reflection* sink) noexcept try
{
    ComPtr<ID3D11ShaderReflection> reflection;
    D3D11_SHADER_DESC desc{};
    if (FAILED(D3DReflect(code.data, code.size, IID_ID3D11ShaderReflection, &reflection)) ||
        FAILED(reflection->GetDesc(&desc))) return -1;
    sink->shader(sink->user, desc.Version);
    for (UINT i = 0; i < desc.BoundResources; ++i) {
        D3D11_SHADER_INPUT_BIND_DESC b{};
        if (FAILED(reflection->GetResourceBindingDesc(i, &b))) return -1;
        const rsf_patch_binding binding{b.Name, static_cast<uint32_t>(b.Type), b.BindPoint, b.BindCount,
            static_cast<uint32_t>(b.ReturnType), static_cast<uint32_t>(b.Dimension)};
        sink->binding(sink->user, &binding);
    }
    for (UINT i = 0; i < desc.ConstantBuffers; ++i) {
        auto* buffer = reflection->GetConstantBufferByIndex(i);
        D3D11_SHADER_BUFFER_DESC b{};
        if (!buffer || FAILED(buffer->GetDesc(&b))) return -1;
        sink->buffer(sink->user, b.Name, b.Size);
        for (UINT j = 0; j < b.Variables; ++j) {
            D3D11_SHADER_VARIABLE_DESC v{};
            if (FAILED(buffer->GetVariableByIndex(j)->GetDesc(&v))) return -1;
            sink->variable(sink->user, v.Name, v.StartOffset, v.Size);
        }
    }
    const auto parameters = [&](uint32_t signature, UINT count, auto read) {
        for (UINT i = 0; i < count; ++i) {
            D3D11_SIGNATURE_PARAMETER_DESC p{};
            if (FAILED(read(i, &p))) return false;
            const rsf_patch_parameter parameter{p.SemanticName, p.SemanticIndex, p.Register,
                static_cast<uint32_t>(p.SystemValueType), static_cast<uint32_t>(p.ComponentType), p.Mask};
            sink->parameter(sink->user, signature, &parameter);
        }
        return true;
    };
    if (!parameters(RSF_PATCH_SIGNATURE_INPUT, desc.InputParameters,
            [&](UINT i, D3D11_SIGNATURE_PARAMETER_DESC* p) { return reflection->GetInputParameterDesc(i, p); }) ||
        !parameters(RSF_PATCH_SIGNATURE_OUTPUT, desc.OutputParameters,
            [&](UINT i, D3D11_SIGNATURE_PARAMETER_DESC* p) { return reflection->GetOutputParameterDesc(i, p); }) ||
        !parameters(RSF_PATCH_SIGNATURE_PATCH, desc.PatchConstantParameters,
            [&](UINT i, D3D11_SIGNATURE_PARAMETER_DESC* p) { return reflection->GetPatchConstantParameterDesc(i, p); }))
        return -1;
    return 0;
} catch (...) { return -1; }

int32_t disassemble(rsf_patch_bytes code, void* user, rsf_patch_sink text) noexcept
{
    ComPtr<ID3DBlob> assembly;
    if (FAILED(D3DDisassemble(code.data, code.size, D3D_DISASM_ENABLE_INSTRUCTION_NUMBERING, nullptr, &assembly)))
        return -1;
    text(user, assembly->GetBufferPointer(), assembly->GetBufferSize());
    return 0;
}

int32_t create_shader(void* device, uint32_t stage, rsf_patch_bytes code, void** shader) noexcept
{
    auto* d = static_cast<ID3D11Device*>(device);
    switch (stage) {
    case 'v': return d->CreateVertexShader(code.data, code.size, nullptr, reinterpret_cast<ID3D11VertexShader**>(shader));
    case 'p': return d->CreatePixelShader(code.data, code.size, nullptr, reinterpret_cast<ID3D11PixelShader**>(shader));
    case 'g': return d->CreateGeometryShader(code.data, code.size, nullptr, reinterpret_cast<ID3D11GeometryShader**>(shader));
    case 'h': return d->CreateHullShader(code.data, code.size, nullptr, reinterpret_cast<ID3D11HullShader**>(shader));
    case 'd': return d->CreateDomainShader(code.data, code.size, nullptr, reinterpret_cast<ID3D11DomainShader**>(shader));
    case 'c': return d->CreateComputeShader(code.data, code.size, nullptr, reinterpret_cast<ID3D11ComputeShader**>(shader));
    default: return E_INVALIDARG;
    }
}

int32_t create_constant_buffer(void* device, uint32_t size, void** buffer) noexcept
{
    D3D11_BUFFER_DESC desc{size, D3D11_USAGE_DYNAMIC, D3D11_BIND_CONSTANT_BUFFER, D3D11_CPU_ACCESS_WRITE, 0, 0};
    return static_cast<ID3D11Device*>(device)->CreateBuffer(&desc, nullptr, reinterpret_cast<ID3D11Buffer**>(buffer));
}

void buffer_desc(void* buffer, uint32_t* width, uint32_t* usage, uint32_t* cpu_access) noexcept
{
    D3D11_BUFFER_DESC desc{};
    static_cast<ID3D11Buffer*>(buffer)->GetDesc(&desc);
    *width = desc.ByteWidth;
    *usage = static_cast<uint32_t>(desc.Usage);
    *cpu_access = desc.CPUAccessFlags;
}

int32_t upload(void* context, void* buffer, rsf_patch_bytes data) noexcept
{
    auto* c = static_cast<ID3D11DeviceContext*>(context);
    auto* b = static_cast<ID3D11Buffer*>(buffer);
    D3D11_MAPPED_SUBRESOURCE mapped{};
    if (FAILED(c->Map(b, 0, D3D11_MAP_WRITE_DISCARD, 0, &mapped)) || !mapped.pData) return -1;
    std::memcpy(mapped.pData, data.data, data.size);
    c->Unmap(b, 0);
    return 0;
}

void bind_constant(void* context, uint32_t stage, uint32_t slot, void* buffer) noexcept
{
    auto* c = static_cast<ID3D11DeviceContext*>(context);
    auto* b = static_cast<ID3D11Buffer*>(buffer);
    switch (stage) {
    case 'v': c->VSSetConstantBuffers(slot, 1, &b); break;
    case 'p': c->PSSetConstantBuffers(slot, 1, &b); break;
    case 'g': c->GSSetConstantBuffers(slot, 1, &b); break;
    case 'h': c->HSSetConstantBuffers(slot, 1, &b); break;
    case 'd': c->DSSetConstantBuffers(slot, 1, &b); break;
    case 'c': c->CSSetConstantBuffers(slot, 1, &b); break;
    }
}

int32_t hooks_ready() noexcept
{
    const auto status = MH_Initialize();
    return status == MH_OK || status == MH_ERROR_ALREADY_INITIALIZED;
}
int32_t hook_create(void* target, void* detour, void** original) noexcept
{
    return MH_CreateHook(target, detour, original) == MH_OK;
}
int32_t hook_enable(void* target) noexcept { const auto status = MH_EnableHook(target);
    return status == MH_OK || status == MH_ERROR_ENABLED; }
void hook_disable(void* target) noexcept { MH_DisableHook(target); }
void hook_remove(void* target) noexcept { MH_RemoveHook(target); }

void bind()
{
    static const rsf_patch_platform platform{sizeof(platform), rsf_patch_parse_json, compile, reflect, disassemble,
        create_shader, create_constant_buffer, buffer_desc, upload, bind_constant, hooks_ready, hook_create,
        hook_enable, hook_disable, hook_remove};
    static const bool bound = (rsf_shader_patch_bind(&platform), true);
    (void)bound;
}
}

extern "C" int32_t rsf_patch_query(rsf_shader_patch_api* api)
{
    bind();
    return rsf_shader_patch_query(api);
}

extern "C" int32_t rsf_patch_transform(rsf_patch_bytes pack, const rsf_patch_bytes* names,
    const rsf_patch_bytes* files, size_t count, void* user, rsf_patch_file_sink output, char* reason)
{
    bind();
    return rsf_shader_patch_transform(pack, names, files, count, user, output, reason);
}

extern "C" int32_t rsf_patch_rewrite_dxbc(rsf_patch_bytes input, const char* expected_sha256,
    const uint32_t* instruction, size_t instruction_words, const uint32_t* replacement,
    size_t replacement_words, void* user, rsf_patch_sink output, char* reason)
{
    bind();
    return rsf_shader_patch_rewrite_dxbc(input, expected_sha256, instruction, instruction_words, replacement,
        replacement_words, user, output, reason);
}
