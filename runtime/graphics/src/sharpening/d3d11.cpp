// SPDX-License-Identifier: MIT
#include "d3d11.h"
#include "rcas.h"
#include "da_rcas.h"
#include "da_das.h"
#include <rescaleframe/d3d11_state.h>
#include <rescaleframe/shader_compile.h>
#include <wrl/client.h>

struct rsf_sharpen11_draw {
    void* views[4];
    void* output;
    void* scratch;
    void* target;
    void* constants;
    void* shader;
    uint32_t extent[6];
    uint32_t exposure_texture;
    uint32_t mode;
};

struct rsf_sharpen11_platform {
    void* (*shader)(void*, uint32_t);
    void* (*constants)(void*);
    void (*scratch)(void*, uint32_t, uint32_t, uint32_t, uint32_t, void**);
    decltype(&rsf_d3d11_state_save) save;
    decltype(&rsf_d3d11_state_restore) restore;
    void (*dispatch)(void*, const void*, const void*, const rsf_sharpen11_draw*);
};

static_assert(sizeof(rsf_sharpen11_draw) == 104);
static_assert(sizeof(rsf_sharpen11_platform) == 6 * sizeof(void*));

extern "C" {
rsf_sharpen11* rsf_core_sharpen11_create();
uint32_t rsf_core_sharpen11_run(rsf_sharpen11*, const rsf_sharpen11_platform*, void*, void* const*,
    uint32_t, const void*, const void*);
uint64_t rsf_core_sharpen11_count(const rsf_sharpen11*);
void rsf_core_sharpen11_destroy(rsf_sharpen11*);
}

namespace rsf::sharpening {
namespace {
using Microsoft::WRL::ComPtr;
static_assert(uint32_t(Mode::rcas) == 1 && uint32_t(Mode::depth_rcas) == 2 && uint32_t(Mode::depth_das) == 3);

void* make_shader(void* native, uint32_t mode) noexcept try
{
    const char* sources[] = {rcas, da_rcas, da_das};
    auto* device = static_cast<ID3D11Device*>(native);
    ComPtr<ID3D11ComputeShader> shader;
    if (!rsf::compile_compute(device, sources[mode - 1], "Sharpening", "CSMain", &shader)) return nullptr;
    return shader.Detach();
} catch (...) { return nullptr; }

void* make_constants(void* native)
{
    auto* device = static_cast<ID3D11Device*>(native);
    D3D11_BUFFER_DESC desc{};
    desc.ByteWidth = sizeof(Constants);
    desc.BindFlags = D3D11_BIND_CONSTANT_BUFFER;
    ComPtr<ID3D11Buffer> buffer;
    if (FAILED(device->CreateBuffer(&desc, nullptr, &buffer))) return nullptr;
    return buffer.Detach();
}

void make_scratch(void* native, uint32_t width, uint32_t height, uint32_t format,
    uint32_t sample_quality, void** out)
{
    auto* device = static_cast<ID3D11Device*>(native);
    D3D11_TEXTURE2D_DESC desc{};
    desc.Width = width;
    desc.Height = height;
    desc.MipLevels = 1;
    desc.ArraySize = 1;
    desc.Format = static_cast<DXGI_FORMAT>(format);
    desc.SampleDesc = {1, sample_quality};
    desc.Usage = D3D11_USAGE_DEFAULT;
    desc.BindFlags = D3D11_BIND_UNORDERED_ACCESS;
    ComPtr<ID3D11Texture2D> texture;
    ComPtr<ID3D11UnorderedAccessView> view;
    if (FAILED(device->CreateTexture2D(&desc, nullptr, &texture)) || !texture ||
        FAILED(device->CreateUnorderedAccessView(texture.Get(), nullptr, &view)) || !view) return;
    out[0] = texture.Detach();
    out[1] = view.Detach();
}

void dispatch(void* native, const void* options, const void* frame, const rsf_sharpen11_draw* draw)
{
    auto* context = static_cast<ID3D11DeviceContext*>(native);
    auto configured = *static_cast<const Frame*>(frame);
    configured.width = draw->extent[0];
    configured.height = draw->extent[1];
    configured.motion_width = draw->extent[2];
    configured.motion_height = draw->extent[3];
    configured.depth_width = draw->extent[4];
    configured.depth_height = draw->extent[5];
    configured.exposure_texture = draw->exposure_texture;
    const auto data = constants(*static_cast<const Options*>(options), configured, static_cast<Mode>(draw->mode));

    // Detach inherited output bindings before binding their SRV.
    ID3D11UnorderedAccessView* empty_uavs[D3D11_PS_CS_UAV_REGISTER_COUNT]{};
    context->OMSetRenderTargetsAndUnorderedAccessViews(0, nullptr, nullptr, 0,
        D3D11_PS_CS_UAV_REGISTER_COUNT, empty_uavs, nullptr);
    context->CSSetUnorderedAccessViews(0, D3D11_PS_CS_UAV_REGISTER_COUNT, empty_uavs, nullptr);

    auto* buffer = static_cast<ID3D11Buffer*>(draw->constants);
    auto* target = static_cast<ID3D11UnorderedAccessView*>(draw->target);
    context->UpdateSubresource(buffer, 0, nullptr, &data, 0, 0);
    ID3D11ShaderResourceView* inputs[]{
        static_cast<ID3D11ShaderResourceView*>(draw->views[0]),
        static_cast<ID3D11ShaderResourceView*>(draw->views[1]),
        static_cast<ID3D11ShaderResourceView*>(draw->views[2]),
        static_cast<ID3D11ShaderResourceView*>(draw->views[3]),
    };
    context->CSSetShader(static_cast<ID3D11ComputeShader*>(draw->shader), nullptr, 0);
    context->CSSetConstantBuffers(0, 1, &buffer);
    context->CSSetShaderResources(0, 4, inputs);
    context->CSSetUnorderedAccessViews(0, 1, &target, nullptr);
    context->Dispatch((configured.width + 15) / 16, (configured.height + 15) / 16, 1);
    ID3D11ShaderResourceView* empty[4]{};
    context->CSSetShaderResources(0, 4, empty);
    context->CSSetUnorderedAccessViews(0, 1, empty_uavs, nullptr);
    context->CopyResource(static_cast<ID3D11Texture2D*>(draw->output),
        static_cast<ID3D11Texture2D*>(draw->scratch));
}

constexpr rsf_sharpen11_platform platform{make_shader, make_constants, make_scratch,
    rsf_d3d11_state_save, rsf_d3d11_state_restore, dispatch};
}

D3D11::~D3D11() { rsf_core_sharpen11_destroy(owner_); }

uint64_t D3D11::dispatches() const { return rsf_core_sharpen11_count(owner_); }

bool D3D11::run(ID3D11DeviceContext* context, ID3D11Texture2D* output, ID3D11Texture2D* motion,
    ID3D11Texture2D* depth, ID3D11Texture2D* exposure, const Options& request, Frame frame)
{
    const auto options = sanitize(request);
    if (!active(options)) return true;
    if (!owner_) owner_ = rsf_core_sharpen11_create();
    if (!owner_) return false;
    void* inputs[]{output, motion, depth, exposure};
    return rsf_core_sharpen11_run(owner_, &platform, context, inputs, uint32_t(options.mode), &options, &frame) != 0;
}
}
