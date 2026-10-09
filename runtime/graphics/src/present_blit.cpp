// SPDX-License-Identifier: GPL-3.0-only

#include <rescaleframe/present_blit.h>

#include <rescaleframe/fullscreen_pass.h>
#include <rescaleframe/log.h>

#include <windows.h>

#include <d3d11.h>
#include <dxgi.h>

#include <new>

// The blit is the fullscreen pass drawing into a swap chain's back buffer: copy or tonemap, sampled
// linearly because the source need not be the back buffer's size. It was the pass's ancestor and kept
// its own shader, state save and compile; this is what is left of it once those are shared.
struct rsf_present_blit {
    ID3D11Device* device = nullptr;
    rsf_fullscreen_pass* pass = nullptr;
    rsf_present_blit_log_fn log = nullptr;
    void* log_user = nullptr;
};

namespace {

rsf_present_blit_result from_pass(rsf_fullscreen_result result)
{
    switch (result) {
    case RSF_FULLSCREEN_OK:
        return RSF_PRESENT_BLIT_OK;
    case RSF_FULLSCREEN_ERROR_INVALID_ARGUMENT:
        return RSF_PRESENT_BLIT_ERROR_INVALID_ARGUMENT;
    case RSF_FULLSCREEN_ERROR_ABI_MISMATCH:
        return RSF_PRESENT_BLIT_ERROR_ABI_MISMATCH;
    case RSF_FULLSCREEN_ERROR_SHADER_FAILED:
        return RSF_PRESENT_BLIT_ERROR_SHADER_FAILED;
    default:
        return RSF_PRESENT_BLIT_ERROR_RESOURCE_FAILED;
    }
}

} // namespace

extern "C" rsf_present_blit_result rsf_present_blit_create(void* device_pointer,
                                                           const rsf_present_blit_setup* setup,
                                                           rsf_present_blit** out)
{
    if (!device_pointer || !setup || !out || setup->struct_size < sizeof(rsf_present_blit_setup)) {
        return RSF_PRESENT_BLIT_ERROR_INVALID_ARGUMENT;
    }
    if (setup->abi_version != RSF_PRESENT_BLIT_ABI_VERSION) {
        return RSF_PRESENT_BLIT_ERROR_ABI_MISMATCH;
    }
    *out = nullptr;

    auto* blit = new (std::nothrow) rsf_present_blit();
    if (!blit) {
        return RSF_PRESENT_BLIT_ERROR_RESOURCE_FAILED;
    }
    blit->device = static_cast<ID3D11Device*>(device_pointer);
    blit->device->AddRef();
    blit->log = setup->log;
    blit->log_user = setup->log_user;

    rsf_fullscreen_setup pass{};
    pass.struct_size = sizeof(pass);
    pass.abi_version = RSF_FULLSCREEN_PASS_ABI_VERSION;
    pass.log = setup->log;
    pass.log_user = setup->log_user;
    const rsf_fullscreen_result made = rsf_fullscreen_pass_create(device_pointer, &pass, &blit->pass);
    if (made != RSF_FULLSCREEN_OK) {
        rsf_present_blit_destroy(blit);
        return from_pass(made);
    }
    rsf_fullscreen_pass_set_filter(blit->pass, RSF_FULLSCREEN_FILTER_LINEAR);

    rsf::say(blit->log, blit->log_user, "present blit ready");
    *out = blit;
    return RSF_PRESENT_BLIT_OK;
}

extern "C" rsf_present_blit_result rsf_present_blit_draw(rsf_present_blit* blit,
                                                         void* context_pointer, void* swapchain,
                                                         void* source, uint32_t tonemap)
{
    if (!blit || !context_pointer || !swapchain || !source) {
        return RSF_PRESENT_BLIT_ERROR_INVALID_ARGUMENT;
    }
    auto* chain = static_cast<IDXGISwapChain*>(swapchain);

    ID3D11Texture2D* back_buffer = nullptr;
    if (FAILED(chain->GetBuffer(0, __uuidof(ID3D11Texture2D),
                                reinterpret_cast<void**>(&back_buffer))) ||
        !back_buffer) {
        return RSF_PRESENT_BLIT_ERROR_NO_BACK_BUFFER;
    }
    D3D11_TEXTURE2D_DESC target_description{};
    back_buffer->GetDesc(&target_description);

    ID3D11RenderTargetView* target = nullptr;
    const HRESULT made_target = blit->device->CreateRenderTargetView(back_buffer, nullptr, &target);
    back_buffer->Release();
    if (FAILED(made_target) || !target) {
        return RSF_PRESENT_BLIT_ERROR_RESOURCE_FAILED;
    }

    ID3D11ShaderResourceView* view = nullptr;
    if (FAILED(blit->device->CreateShaderResourceView(static_cast<ID3D11Resource*>(source), nullptr,
                                                      &view)) ||
        !view) {
        target->Release();
        return RSF_PRESENT_BLIT_ERROR_RESOURCE_FAILED;
    }

    rsf_fullscreen_draw draw{};
    draw.struct_size = sizeof(draw);
    draw.mode = tonemap ? RSF_FULLSCREEN_TONEMAP : RSF_FULLSCREEN_COPY;
    draw.exposure = 1.0f;
    draw.width = target_description.Width;
    draw.height = target_description.Height;
    const rsf_fullscreen_result drawn =
        rsf_fullscreen_pass_draw(blit->pass, context_pointer, target, view, &draw);
    view->Release();
    target->Release();
    return from_pass(drawn);
}

extern "C" void rsf_present_blit_destroy(rsf_present_blit* blit)
{
    if (!blit) {
        return;
    }
    rsf_fullscreen_pass_destroy(blit->pass);
    if (blit->device) {
        blit->device->Release();
    }
    delete blit;
}
