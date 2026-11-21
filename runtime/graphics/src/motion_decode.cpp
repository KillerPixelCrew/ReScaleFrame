// SPDX-License-Identifier: MIT

#include "motion_decode_platform.h"

#include <rescaleframe/log.h>
#include <rescaleframe/shader_compile.h>

#include <windows.h>

#include <d3d11.h>

#include <cstring>

namespace {

// The decode itself. Kept as source and compiled at load rather than shipped as bytecode, because
// the cross build has no shader compiler and a pass that only exists in the MSVC build could not be
// tested here at all. d3dcompiler_47 is present both on Windows and in a Proton prefix.
const char* const decode_shader = R"(
Texture2D<float4>   Source : register(t0);
RWTexture2D<float2> Target : register(u0);

cbuffer Params : register(b0)
{
    float2 Scale;
    float2 Bias;
    float2 OutputScale;
    float  InvalidValue;
    uint   ZeroMeansUnwritten;
    uint2  Size;
    uint   ValidityEnabled;
    uint   ValidityChannel;
    float  ValidityThreshold;
    uint3  Padding;
};

[numthreads(8, 8, 1)]
void main(uint3 id : SV_DispatchThreadID)
{
    if (id.x >= Size.x || id.y >= Size.y)
    {
        return;
    }

    float4 raw = Source[id.xy];
    float2 stored = raw.xy;

    // The sentinel test runs before the decode and on the stored value, which is the only place it
    // is unambiguous. After the decode, zero is a value real motion can take. An explicit validity
    // channel replaces the zero test: validity then does not depend on the motion value at all.
    bool unwritten;
    if (ValidityEnabled != 0)
    {
        unwritten = !(raw[ValidityChannel & 3u] > ValidityThreshold);
    }
    else
    {
        unwritten = ZeroMeansUnwritten != 0 && stored.x == 0.0 && stored.y == 0.0;
    }
    if (unwritten)
    {
        Target[id.xy] = float2(InvalidValue, InvalidValue);
        return;
    }

    Target[id.xy] = (stored - Bias) * Scale * OutputScale;
}
)";

int32_t make_resources(void* raw, const rsf_motion_decode_setup* setup, uint32_t constant_bytes,
    rsf_motion_decode_resources* resources) noexcept try
{
    auto* device = static_cast<ID3D11Device*>(raw);
    if (!rsf::compile_compute(device, decode_shader, "motion_decode", "main", &resources->shader,
            setup->log, setup->log_user, 0) || !resources->shader) {
        rsf::say(setup->log, setup->log_user, "motion decode shader could not be created");
        return RSF_MOTION_DECODE_ERROR_SHADER_FAILED;
    }

    D3D11_TEXTURE2D_DESC target{};
    target.Width = setup->width;
    target.Height = setup->height;
    target.MipLevels = 1;
    target.ArraySize = 1;
    target.Format = setup->output_format != 0
        ? static_cast<DXGI_FORMAT>(setup->output_format) : DXGI_FORMAT_R16G16_FLOAT;
    target.SampleDesc.Count = 1;
    target.Usage = D3D11_USAGE_DEFAULT;
    target.BindFlags = D3D11_BIND_UNORDERED_ACCESS | D3D11_BIND_SHADER_RESOURCE;
    if (FAILED(device->CreateTexture2D(&target, nullptr, &resources->target)) || !resources->target) {
        rsf::say(setup->log, setup->log_user, "decoded motion target could not be created");
        return RSF_MOTION_DECODE_ERROR_RESOURCE_FAILED;
    }
    if (FAILED(device->CreateUnorderedAccessView(resources->target, nullptr, &resources->target_view)) ||
        !resources->target_view) {
        rsf::say(setup->log, setup->log_user, "unordered access view could not be created");
        return RSF_MOTION_DECODE_ERROR_RESOURCE_FAILED;
    }

    D3D11_BUFFER_DESC constants{};
    constants.ByteWidth = constant_bytes;
    constants.Usage = D3D11_USAGE_DYNAMIC;
    constants.BindFlags = D3D11_BIND_CONSTANT_BUFFER;
    constants.CPUAccessFlags = D3D11_CPU_ACCESS_WRITE;
    if (FAILED(device->CreateBuffer(&constants, nullptr, &resources->constants)) || !resources->constants) {
        rsf::say(setup->log, setup->log_user, "constant buffer could not be created");
        return RSF_MOTION_DECODE_ERROR_RESOURCE_FAILED;
    }
    return RSF_MOTION_DECODE_OK;
} catch (...) { return RSF_MOTION_DECODE_ERROR_RESOURCE_FAILED; }

int32_t dispatch(void* raw, const rsf_motion_decode_resources* resources, void* source_view,
    const void* constants, size_t bytes, uint32_t groups_x, uint32_t groups_y) noexcept try
{
    auto* context = static_cast<ID3D11DeviceContext*>(raw);
    D3D11_MAPPED_SUBRESOURCE mapped{};
    if (FAILED(context->Map(resources->constants, 0, D3D11_MAP_WRITE_DISCARD, 0, &mapped))) return 0;
    std::memcpy(mapped.pData, constants, bytes);
    context->Unmap(resources->constants, 0);

    ID3D11ShaderResourceView* sources[]{static_cast<ID3D11ShaderResourceView*>(source_view)};
    ID3D11UnorderedAccessView* targets[]{resources->target_view};
    const UINT no_offset = UINT32_MAX;
    return rsf::motion11::dispatch(context, resources->shader, resources->constants,
        sources, targets, &no_offset, groups_x, groups_y);
} catch (...) { return 0; }

constexpr rsf_motion_decode_platform platform{
    make_resources, rsf::motion11::context_matches, rsf::motion11::describe_source,
    rsf::motion11::source_view, dispatch,
    [](rsf_motion_decode_log_fn log, void* user, const char* text) noexcept {
        try { log(user, text); } catch (...) {}
    },
};

static_assert(sizeof(rsf_motion_decode_platform) == 6 * sizeof(void*));
static_assert(sizeof(rsf_motion_decode_resources) == 4 * sizeof(void*) && sizeof(rsf_motion_source_info) == 24);
static_assert(sizeof(rsf_motion_decode_setup) == 40 && sizeof(rsf_motion_decode_params) == 48 &&
    RSF_MOTION_DECODE_PARAMS_SIZE_V1 == 36);
static_assert(RSF_MOTION_DECODE_ABI_VERSION == 1 && RSF_MOTION_DECODE_OK == 0 &&
    RSF_MOTION_DECODE_ERROR_INVALID_ARGUMENT == -1 && RSF_MOTION_DECODE_ERROR_ABI_MISMATCH == -2 &&
    RSF_MOTION_DECODE_ERROR_RESOURCE_FAILED == -4 && RSF_MOTION_DECODE_ERROR_SOURCE_MISMATCH == -5);

} // namespace

extern "C" rsf_motion_decode_result rsf_motion_decode_create(
    void* device, const rsf_motion_decode_setup* setup, rsf_motion_decode** out)
{
    return rsf_core_motion_decode_create(device, setup, out, &platform);
}

extern "C" rsf_motion_decode_result rsf_motion_decode_run(
    rsf_motion_decode* pass, void* context, void* source, const rsf_motion_decode_params* params)
{
    return rsf_core_motion_decode_run(pass, context, source, params);
}

extern "C" void* rsf_motion_decode_texture(rsf_motion_decode* pass)
{
    return rsf_core_motion_decode_texture(pass);
}

extern "C" void rsf_motion_decode_destroy(rsf_motion_decode* pass)
{
    rsf_core_motion_decode_destroy(pass);
}
