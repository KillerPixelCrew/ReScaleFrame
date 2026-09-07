/* SPDX-License-Identifier: GPL-3.0-only */
/* The composite, checked against arithmetic rather than against a screenshot.
 *
 * `final = ui.rgb + (1 - ui.a) * scene.rgb` is what all three frame generation SDKs ask for, so it
 * is the one piece of this runtime where being a little bit wrong would be invisible in motion and
 * wrong in every frame. Nothing covered the blit this pass was generalised from, which is how it
 * kept an unnoticed hardcoded alpha for as long as it did.
 */
#include <rescaleframe/fullscreen_pass.h>

#include <windows.h>

#include <d3d11.h>

#include <cstdio>
#include <cstdint>
#include <cmath>

namespace {

bool passed = true;

void check(bool condition, const char* what)
{
    if (!condition) {
        std::fprintf(stderr, "FAIL: %s\n", what);
        passed = false;
    }
}

void stage(const char* what)
{
    std::fprintf(stderr, "[stage] %s\n", what);
    std::fflush(stderr);
}

struct Pixel {
    float r, g, b, a;
};

/* Read one pixel back through a staging copy. Small and slow and exactly what a test wants. */
bool read_pixel(ID3D11Device* device, ID3D11DeviceContext* context, ID3D11Texture2D* texture,
                Pixel& out)
{
    D3D11_TEXTURE2D_DESC description{};
    texture->GetDesc(&description);
    D3D11_TEXTURE2D_DESC staging = description;
    staging.Usage = D3D11_USAGE_STAGING;
    staging.BindFlags = 0;
    staging.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
    staging.MiscFlags = 0;
    ID3D11Texture2D* readable = nullptr;
    if (FAILED(device->CreateTexture2D(&staging, nullptr, &readable)) || !readable) {
        return false;
    }
    context->CopyResource(readable, texture);
    D3D11_MAPPED_SUBRESOURCE mapped{};
    if (FAILED(context->Map(readable, 0, D3D11_MAP_READ, 0, &mapped)) || !mapped.pData) {
        readable->Release();
        return false;
    }
    bool ok = true;
    if (description.Format == DXGI_FORMAT_R8G8B8A8_UNORM) {
        const uint8_t* p = static_cast<const uint8_t*>(mapped.pData);
        out.r = p[0] / 255.0f;
        out.g = p[1] / 255.0f;
        out.b = p[2] / 255.0f;
        out.a = p[3] / 255.0f;
    } else if (description.Format == DXGI_FORMAT_R10G10B10A2_UNORM) {
        const uint32_t v = *static_cast<const uint32_t*>(mapped.pData);
        out.r = float(v & 0x3FFu) / 1023.0f;
        out.g = float((v >> 10) & 0x3FFu) / 1023.0f;
        out.b = float((v >> 20) & 0x3FFu) / 1023.0f;
        out.a = float((v >> 30) & 0x3u) / 3.0f;
    } else {
        ok = false;
    }
    context->Unmap(readable, 0);
    readable->Release();
    return ok;
}

ID3D11Texture2D* make_texture(ID3D11Device* device, UINT width, UINT height, DXGI_FORMAT format,
                              UINT bind)
{
    D3D11_TEXTURE2D_DESC description{};
    description.Width = width;
    description.Height = height;
    description.MipLevels = 1;
    description.ArraySize = 1;
    description.Format = format;
    description.SampleDesc.Count = 1;
    description.Usage = D3D11_USAGE_DEFAULT;
    description.BindFlags = bind;
    ID3D11Texture2D* texture = nullptr;
    device->CreateTexture2D(&description, nullptr, &texture);
    return texture;
}

bool near_enough(float actual, float expected, float budget)
{
    return std::fabs(actual - expected) <= budget;
}

} // namespace

int main()
{
    stage("creating device");
    ID3D11Device* device = nullptr;
    ID3D11DeviceContext* context = nullptr;
    const D3D_FEATURE_LEVEL wanted[] = {D3D_FEATURE_LEVEL_11_0};
    D3D_FEATURE_LEVEL obtained{};
    HRESULT created =
        D3D11CreateDevice(nullptr, D3D_DRIVER_TYPE_HARDWARE, nullptr, 0, wanted, 1,
                          D3D11_SDK_VERSION, &device, &obtained, &context);
    if (FAILED(created)) {
        created = D3D11CreateDevice(nullptr, D3D_DRIVER_TYPE_WARP, nullptr, 0, wanted, 1,
                                    D3D11_SDK_VERSION, &device, &obtained, &context);
    }
    if (FAILED(created) || !device || !context) {
        std::fprintf(stderr, "no D3D11 device available, skipping\n");
        return 77;
    }

    stage("creating the pass");
    rsf_fullscreen_setup setup{};
    setup.struct_size = sizeof(setup);
    setup.abi_version = RSF_FULLSCREEN_PASS_ABI_VERSION + 1u;
    rsf_fullscreen_pass* pass = nullptr;
    check(rsf_fullscreen_pass_create(device, &setup, &pass) == RSF_FULLSCREEN_ERROR_ABI_MISMATCH,
          "A pass built against another version of this header must be refused.");
    setup.abi_version = RSF_FULLSCREEN_PASS_ABI_VERSION;

    const rsf_fullscreen_result made = rsf_fullscreen_pass_create(device, &setup, &pass);
    if (made == RSF_FULLSCREEN_ERROR_SHADER_FAILED) {
        std::fprintf(stderr, "d3dcompiler_47.dll is not available, skipping\n");
        context->Release();
        device->Release();
        return 77;
    }
    check(made == RSF_FULLSCREEN_OK && pass, "The pass must be created.");
    if (!pass) {
        return 1;
    }

    stage("creating resources");
    /* The scene in the game's own back buffer format, so the composite is checked in the encoding
       it will actually run in rather than in a convenient one. */
    ID3D11Texture2D* scene = make_texture(device, 4, 4, DXGI_FORMAT_R10G10B10A2_UNORM,
                                          D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE);
    ID3D11Texture2D* layer = make_texture(device, 4, 4, DXGI_FORMAT_R8G8B8A8_UNORM,
                                          D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE);
    ID3D11Texture2D* readback = make_texture(device, 4, 4, DXGI_FORMAT_R8G8B8A8_UNORM,
                                             D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE);
    check(scene && layer && readback, "The test textures must be created.");
    if (!scene || !layer || !readback) {
        return 1;
    }

    ID3D11RenderTargetView* scene_target = nullptr;
    ID3D11RenderTargetView* layer_target = nullptr;
    ID3D11RenderTargetView* readback_target = nullptr;
    ID3D11ShaderResourceView* layer_source = nullptr;
    ID3D11ShaderResourceView* scene_source = nullptr;
    device->CreateRenderTargetView(scene, nullptr, &scene_target);
    device->CreateRenderTargetView(layer, nullptr, &layer_target);
    device->CreateRenderTargetView(readback, nullptr, &readback_target);
    device->CreateShaderResourceView(layer, nullptr, &layer_source);
    device->CreateShaderResourceView(scene, nullptr, &scene_source);
    check(scene_target && layer_target && readback_target && layer_source && scene_source,
          "The test views must be created.");

    rsf_fullscreen_draw parameters{};
    parameters.struct_size = sizeof(parameters);

    stage("a premultiplied layer composites over the scene");
    {
        // Half covered white: premultiplied, so the colour is already scaled by its own alpha.
        const float ui[4] = {0.5f, 0.5f, 0.5f, 0.5f};
        const float background[4] = {0.4f, 0.2f, 0.8f, 1.0f};
        context->ClearRenderTargetView(layer_target, ui);
        context->ClearRenderTargetView(scene_target, background);

        parameters.mode = RSF_FULLSCREEN_PREMULTIPLIED;
        check(rsf_fullscreen_pass_draw(pass, context, scene_target, layer_source, &parameters) ==
                  RSF_FULLSCREEN_OK,
              "The composite must draw.");

        Pixel out{};
        check(read_pixel(device, context, scene, out), "The composite must be readable.");
        // ui.rgb + (1 - ui.a) * scene.rgb
        const float budget = 2.0f / 1023.0f;
        check(near_enough(out.r, 0.5f + 0.5f * 0.4f, budget) &&
                  near_enough(out.g, 0.5f + 0.5f * 0.2f, budget) &&
                  near_enough(out.b, 0.5f + 0.5f * 0.8f, budget),
              "Half coverage must give ui.rgb + (1 - ui.a) * scene.rgb. This is the arithmetic "
              "every frame generation SDK asks for and getting it slightly wrong is invisible in "
              "motion and wrong in every frame.");
    }

    stage("an empty layer leaves the scene exactly as it was");
    {
        const float nothing[4] = {0.0f, 0.0f, 0.0f, 0.0f};
        const float background[4] = {0.4f, 0.2f, 0.8f, 1.0f};
        context->ClearRenderTargetView(layer_target, nothing);
        context->ClearRenderTargetView(scene_target, background);

        parameters.mode = RSF_FULLSCREEN_PREMULTIPLIED;
        rsf_fullscreen_pass_draw(pass, context, scene_target, layer_source, &parameters);

        Pixel out{};
        check(read_pixel(device, context, scene, out), "The scene must be readable.");
        const float budget = 1.0f / 1023.0f;
        check(near_enough(out.r, 0.4f, budget) && near_enough(out.g, 0.2f, budget) &&
                  near_enough(out.b, 0.8f, budget),
              "A layer cleared to zero must be invisible. The compositor runs on every frame "
              "including the ones with no interface on them, so this is the common case and not "
              "an edge.");
    }

    stage("a fully covered layer replaces the scene");
    {
        const float ui[4] = {1.0f, 0.0f, 0.0f, 1.0f};
        const float background[4] = {0.4f, 0.2f, 0.8f, 1.0f};
        context->ClearRenderTargetView(layer_target, ui);
        context->ClearRenderTargetView(scene_target, background);
        rsf_fullscreen_pass_draw(pass, context, scene_target, layer_source, &parameters);

        Pixel out{};
        check(read_pixel(device, context, scene, out), "The scene must be readable.");
        const float budget = 2.0f / 1023.0f;
        check(near_enough(out.r, 1.0f, budget) && near_enough(out.g, 0.0f, budget) &&
                  near_enough(out.b, 0.0f, budget),
              "Full coverage must leave nothing of the scene.");
    }

    stage("the copy mode is opaque and does not blend");
    {
        const float source[4] = {0.25f, 0.5f, 0.75f, 0.0f};
        const float background[4] = {1.0f, 1.0f, 1.0f, 1.0f};
        context->ClearRenderTargetView(layer_target, source);
        context->ClearRenderTargetView(readback_target, background);

        parameters.mode = RSF_FULLSCREEN_COPY;
        rsf_fullscreen_pass_draw(pass, context, readback_target, layer_source, &parameters);

        Pixel out{};
        check(read_pixel(device, context, readback, out), "The copy must be readable.");
        const float budget = 2.0f / 255.0f;
        check(near_enough(out.r, 0.25f, budget) && near_enough(out.g, 0.5f, budget) &&
                  near_enough(out.b, 0.75f, budget),
              "A copy must land as it is, whatever the source alpha says. The debug view uses "
              "this and a zero alpha source must not vanish.");
        check(near_enough(out.a, 1.0f, budget), "And must be written opaque.");
    }

    stage("the alpha mode shows coverage");
    {
        const float source[4] = {1.0f, 1.0f, 1.0f, 0.25f};
        context->ClearRenderTargetView(layer_target, source);
        parameters.mode = RSF_FULLSCREEN_ALPHA;
        rsf_fullscreen_pass_draw(pass, context, readback_target, layer_source, &parameters);

        Pixel out{};
        check(read_pixel(device, context, readback, out), "The alpha view must be readable.");
        const float budget = 2.0f / 255.0f;
        check(near_enough(out.r, 0.25f, budget) && near_enough(out.g, 0.25f, budget),
              "The alpha must come back as grey. A layer with no coverage composites to nothing "
              "and looks exactly like a broken one, and this is how the two are told apart.");
    }

    stage("state is put back, scissors included");
    {
        // A scissor the pass must not leave behind. The blit this was generalised from saved
        // viewports and not scissors, so a rectangle set here survived into the game's next draw.
        const D3D11_RECT scissor = {1, 2, 3, 4};
        context->RSSetScissorRects(1, &scissor);
        D3D11_VIEWPORT viewport{};
        viewport.TopLeftX = 5.0f;
        viewport.Width = 64.0f;
        viewport.Height = 32.0f;
        viewport.MaxDepth = 1.0f;
        context->RSSetViewports(1, &viewport);
        context->OMSetRenderTargets(1, &readback_target, nullptr);

        parameters.mode = RSF_FULLSCREEN_COPY;
        rsf_fullscreen_pass_draw(pass, context, scene_target, layer_source, &parameters);

        UINT count = 1;
        D3D11_RECT after{};
        context->RSGetScissorRects(&count, &after);
        check(count == 1 && after.left == 1 && after.top == 2 && after.right == 3 &&
                  after.bottom == 4,
              "The scissor rectangle must survive the pass.");

        count = 1;
        D3D11_VIEWPORT viewport_after{};
        context->RSGetViewports(&count, &viewport_after);
        check(count == 1 && viewport_after.Width == 64.0f && viewport_after.Height == 32.0f &&
                  viewport_after.TopLeftX == 5.0f,
              "And the viewport.");

        ID3D11RenderTargetView* target_after = nullptr;
        ID3D11DepthStencilView* depth_after = nullptr;
        context->OMGetRenderTargets(1, &target_after, &depth_after);
        check(target_after == readback_target,
              "And the render target the game had bound: this runs between the game's own draws.");
        if (target_after) {
            target_after->Release();
        }
        if (depth_after) {
            depth_after->Release();
        }
    }

    stage("arguments are checked");
    {
        parameters.mode = RSF_FULLSCREEN_COPY;
        check(rsf_fullscreen_pass_draw(nullptr, context, scene_target, layer_source, &parameters) ==
                  RSF_FULLSCREEN_ERROR_INVALID_ARGUMENT,
              "A null pass.");
        check(rsf_fullscreen_pass_draw(pass, context, nullptr, layer_source, &parameters) ==
                  RSF_FULLSCREEN_ERROR_INVALID_ARGUMENT,
              "A null target.");
        check(rsf_fullscreen_pass_draw(pass, context, scene_target, nullptr, &parameters) ==
                  RSF_FULLSCREEN_ERROR_INVALID_ARGUMENT,
              "A null source.");
        rsf_fullscreen_draw bad = parameters;
        bad.mode = 99;
        check(rsf_fullscreen_pass_draw(pass, context, scene_target, layer_source, &bad) ==
                  RSF_FULLSCREEN_ERROR_INVALID_ARGUMENT,
              "A mode that does not exist must be refused rather than drawn as something else.");
        bad = parameters;
        bad.struct_size = 4;
        check(rsf_fullscreen_pass_draw(pass, context, scene_target, layer_source, &bad) ==
                  RSF_FULLSCREEN_ERROR_INVALID_ARGUMENT,
              "A short parameter structure.");
    }

    stage("releasing");
    scene_source->Release();
    layer_source->Release();
    readback_target->Release();
    layer_target->Release();
    scene_target->Release();
    readback->Release();
    layer->Release();
    scene->Release();
    rsf_fullscreen_pass_destroy(pass);
    rsf_fullscreen_pass_destroy(nullptr);
    context->Release();
    device->Release();

    std::fprintf(stderr, "%s\n",
                 passed ? "fullscreen_pass: all checks passed" : "fullscreen_pass: FAILED");
    return passed ? 0 : 1;
}
