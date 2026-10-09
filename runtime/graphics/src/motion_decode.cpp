// SPDX-License-Identifier: GPL-3.0-only

#include <rescaleframe/motion_decode.h>

#include <rescaleframe/log.h>
#include <rescaleframe/shader_compile.h>
#include <rescaleframe/srv_cache.h>

#include "device_owner.h"

#include <windows.h>

#include <d3d11.h>
#include <wrl/client.h>

#include <cstring>

namespace {

// The decode itself. Kept as source and compiled at load rather than shipped as bytecode, because
// the cross build has no shader compiler and a pass that only exists in the MSVC build could not be
// tested here at all. d3dcompiler_47 is present both on Windows and in a Proton prefix.
const char* const decode_shader = R"(
Texture2D<float2>   Source : register(t0);
RWTexture2D<float2> Target : register(u0);

cbuffer Params : register(b0)
{
    float2 Scale;
    float2 Bias;
    float2 OutputScale;
    float  InvalidValue;
    uint   ZeroMeansUnwritten;
    uint2  Size;
    uint2  Padding;
};

[numthreads(8, 8, 1)]
void main(uint3 id : SV_DispatchThreadID)
{
    if (id.x >= Size.x || id.y >= Size.y)
    {
        return;
    }

    float2 stored = Source[id.xy];

    // The sentinel test runs before the decode and on the stored value, which is the only place it
    // is unambiguous. After the decode, zero is a value real motion can take.
    if (ZeroMeansUnwritten != 0 && stored.x == 0.0 && stored.y == 0.0)
    {
        Target[id.xy] = float2(InvalidValue, InvalidValue);
        return;
    }

    Target[id.xy] = (stored - Bias) * Scale * OutputScale;
}
)";

struct Constants {
    float scale[2];
    float bias[2];
    float output_scale[2];
    float invalid_value;
    uint32_t zero_means_unwritten;
    uint32_t size[2];
    uint32_t padding[2];
};
static_assert(sizeof(Constants) % 16 == 0, "constant buffers are bound in 16 byte registers");

} // namespace

struct rsf_motion_decode {
    Microsoft::WRL::ComPtr<ID3D11Device> device;
    Microsoft::WRL::ComPtr<ID3D11ComputeShader> shader;
    Microsoft::WRL::ComPtr<ID3D11Texture2D> target;
    Microsoft::WRL::ComPtr<ID3D11UnorderedAccessView> target_view;
    Microsoft::WRL::ComPtr<ID3D11Buffer> constants;
    // The source is an engine target that is the same texture every frame.
    rsf::SrvCache<2> sources;
    uint32_t width = 0;
    uint32_t height = 0;
    rsf_motion_decode_log_fn log = nullptr;
    void* log_user = nullptr;
};

extern "C" rsf_motion_decode_result rsf_motion_decode_create(void* device_pointer,
                                                             const rsf_motion_decode_setup* setup,
                                                             rsf_motion_decode** out)
{
    if (!device_pointer || !setup || !out ||
        setup->struct_size < sizeof(rsf_motion_decode_setup) || setup->width == 0 ||
        setup->height == 0) {
        return RSF_MOTION_DECODE_ERROR_INVALID_ARGUMENT;
    }
    if (setup->abi_version != RSF_MOTION_DECODE_ABI_VERSION) {
        return RSF_MOTION_DECODE_ERROR_ABI_MISMATCH;
    }
    *out = nullptr;

    auto* device = static_cast<ID3D11Device*>(device_pointer);
    auto* pass = new rsf_motion_decode();
    pass->device = device;
    pass->width = setup->width;
    pass->height = setup->height;
    pass->log = setup->log;
    pass->log_user = setup->log_user;

    // Flags 0, as this shader has always been compiled.
    if (!rsf::compile_compute(device, decode_shader, "motion_decode", "main", &pass->shader,
                              pass->log, pass->log_user, 0) ||
        !pass->shader) {
        rsf::say(pass->log, pass->log_user, "motion decode shader could not be created");
        rsf_motion_decode_destroy(pass);
        return RSF_MOTION_DECODE_ERROR_SHADER_FAILED;
    }

    D3D11_TEXTURE2D_DESC target{};
    target.Width = setup->width;
    target.Height = setup->height;
    target.MipLevels = 1;
    target.ArraySize = 1;
    target.Format = setup->output_format != 0
                        ? static_cast<DXGI_FORMAT>(setup->output_format)
                        : DXGI_FORMAT_R16G16_FLOAT;
    target.SampleDesc.Count = 1;
    target.Usage = D3D11_USAGE_DEFAULT;
    // Shader resource as well as unordered access: this target is written here and then read by a
    // backend, which binds it as a texture.
    target.BindFlags = D3D11_BIND_UNORDERED_ACCESS | D3D11_BIND_SHADER_RESOURCE;
    if (FAILED(device->CreateTexture2D(&target, nullptr, &pass->target)) || !pass->target) {
        rsf::say(pass->log, pass->log_user, "decoded motion target could not be created");
        rsf_motion_decode_destroy(pass);
        return RSF_MOTION_DECODE_ERROR_RESOURCE_FAILED;
    }
    if (FAILED(device->CreateUnorderedAccessView(pass->target.Get(), nullptr, &pass->target_view)) ||
        !pass->target_view) {
        rsf::say(pass->log, pass->log_user, "unordered access view could not be created");
        rsf_motion_decode_destroy(pass);
        return RSF_MOTION_DECODE_ERROR_RESOURCE_FAILED;
    }

    D3D11_BUFFER_DESC constants{};
    constants.ByteWidth = sizeof(Constants);
    constants.Usage = D3D11_USAGE_DYNAMIC;
    constants.BindFlags = D3D11_BIND_CONSTANT_BUFFER;
    constants.CPUAccessFlags = D3D11_CPU_ACCESS_WRITE;
    if (FAILED(device->CreateBuffer(&constants, nullptr, &pass->constants)) || !pass->constants) {
        rsf::say(pass->log, pass->log_user, "constant buffer could not be created");
        rsf_motion_decode_destroy(pass);
        return RSF_MOTION_DECODE_ERROR_RESOURCE_FAILED;
    }

    rsf::say(pass->log, pass->log_user, "motion decode ready for %ux%u", setup->width,
             setup->height);
    *out = pass;
    return RSF_MOTION_DECODE_OK;
}

extern "C" rsf_motion_decode_result rsf_motion_decode_run(rsf_motion_decode* pass,
                                                          void* context_pointer, void* source,
                                                          const rsf_motion_decode_params* params)
{
    if (!pass || !context_pointer || !source || !params ||
        params->struct_size < sizeof(rsf_motion_decode_params)) {
        return RSF_MOTION_DECODE_ERROR_INVALID_ARGUMENT;
    }

    auto* context = static_cast<ID3D11DeviceContext*>(context_pointer);
    auto* texture = static_cast<ID3D11Texture2D*>(source);

    // Same rule as the texture dump: a resource from another device faults rather than failing.
    if (!rsf::same_device(texture, pass->device.Get())) {
        rsf::say(pass->log, pass->log_user, "source belongs to another device");
        return RSF_MOTION_DECODE_ERROR_SOURCE_MISMATCH;
    }

    D3D11_TEXTURE2D_DESC description{};
    texture->GetDesc(&description);
    if (description.Width != pass->width || description.Height != pass->height) {
        rsf::say(pass->log, pass->log_user, "source is %ux%u but this pass was built for %ux%u",
                 description.Width, description.Height, pass->width, pass->height);
        return RSF_MOTION_DECODE_ERROR_SOURCE_MISMATCH;
    }

    ID3D11ShaderResourceView* source_view = pass->sources.get(pass->device.Get(), texture);
    if (!source_view) {
        rsf::say(pass->log, pass->log_user, "source view could not be created");
        return RSF_MOTION_DECODE_ERROR_RESOURCE_FAILED;
    }

    D3D11_MAPPED_SUBRESOURCE mapped{};
    if (FAILED(context->Map(pass->constants.Get(), 0, D3D11_MAP_WRITE_DISCARD, 0, &mapped))) {
        return RSF_MOTION_DECODE_ERROR_RESOURCE_FAILED;
    }
    Constants values{};
    values.scale[0] = params->scale_x;
    values.scale[1] = params->scale_y;
    values.bias[0] = params->bias_x;
    values.bias[1] = params->bias_y;
    values.output_scale[0] = params->output_scale_x;
    values.output_scale[1] = params->output_scale_y;
    values.invalid_value = params->invalid_value;
    values.zero_means_unwritten = params->zero_means_unwritten;
    values.size[0] = pass->width;
    values.size[1] = pass->height;
    std::memcpy(mapped.pData, &values, sizeof(values));
    context->Unmap(pass->constants.Get(), 0);

    // This dispatch happens inside a frame the game is in the middle of. It did not ask for its
    // compute bindings to change, so they are put back exactly as they were.
    Microsoft::WRL::ComPtr<ID3D11ComputeShader> previous_shader;
    ID3D11ClassInstance* previous_instances[D3D11_SHADER_MAX_INTERFACES] = {};
    UINT previous_instance_count = D3D11_SHADER_MAX_INTERFACES;
    Microsoft::WRL::ComPtr<ID3D11ShaderResourceView> previous_source;
    Microsoft::WRL::ComPtr<ID3D11UnorderedAccessView> previous_target;
    Microsoft::WRL::ComPtr<ID3D11Buffer> previous_constants;
    context->CSGetShader(&previous_shader, previous_instances, &previous_instance_count);
    context->CSGetShaderResources(0, 1, &previous_source);
    context->CSGetUnorderedAccessViews(0, 1, &previous_target);
    context->CSGetConstantBuffers(0, 1, &previous_constants);

    const UINT no_offset = static_cast<UINT>(-1);
    ID3D11UnorderedAccessView* target_view = pass->target_view.Get();
    ID3D11Buffer* constants = pass->constants.Get();
    context->CSSetShader(pass->shader.Get(), nullptr, 0);
    context->CSSetShaderResources(0, 1, &source_view);
    context->CSSetUnorderedAccessViews(0, 1, &target_view, &no_offset);
    context->CSSetConstantBuffers(0, 1, &constants);
    context->Dispatch((pass->width + 7) / 8, (pass->height + 7) / 8, 1);

    // Unbind our own views before restoring, so a target that was bound elsewhere is not left
    // fighting this one for the same slot.
    ID3D11ShaderResourceView* no_source = nullptr;
    ID3D11UnorderedAccessView* no_target = nullptr;
    context->CSSetShaderResources(0, 1, &no_source);
    context->CSSetUnorderedAccessViews(0, 1, &no_target, &no_offset);

    context->CSSetShader(previous_shader.Get(), previous_instances, previous_instance_count);
    ID3D11ShaderResourceView* restored_source = previous_source.Get();
    ID3D11UnorderedAccessView* restored_target = previous_target.Get();
    ID3D11Buffer* restored_constants = previous_constants.Get();
    context->CSSetShaderResources(0, 1, &restored_source);
    context->CSSetUnorderedAccessViews(0, 1, &restored_target, &no_offset);
    context->CSSetConstantBuffers(0, 1, &restored_constants);

    for (UINT index = 0; index < previous_instance_count; ++index) {
        if (previous_instances[index]) {
            previous_instances[index]->Release();
        }
    }
    return RSF_MOTION_DECODE_OK;
}

extern "C" void* rsf_motion_decode_texture(rsf_motion_decode* pass)
{
    return pass ? pass->target.Get() : nullptr;
}

extern "C" void rsf_motion_decode_destroy(rsf_motion_decode* pass)
{
    delete pass;
}
