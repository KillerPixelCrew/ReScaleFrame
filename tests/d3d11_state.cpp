// Set a pipeline up, save it, take it apart, and check that restoring puts every piece back.
//
// This exists because of where it is used. The reconstruction runs partway through the game's own
// frame now, and the engine will not rebind what it believes is still bound, so anything this
// misses is a pass that draws with the wrong state and no error anywhere. Each stage is checked
// separately rather than as one "did it work", because the failure that matters is one stage of
// several being forgotten.

#include <rescaleframe/d3d11_state.h>

#include <windows.h>

#include <d3d11.h>

#include <cstdio>

namespace {

bool passed = true;

void check(bool condition, const char* message)
{
    if (!condition) {
        std::fprintf(stderr, "%s\n", message);
        passed = false;
    }
}

void stage(const char* what)
{
    std::fprintf(stderr, "[stage] %s\n", what);
    std::fflush(stderr);
}

// Each of these takes a reference and drops it. Addresses are all that is compared, and holding on
// would change the lifetimes the test is checking.
template <typename T> T* dropped(T* value)
{
    if (value) {
        value->Release();
    }
    return value;
}

ID3D11Texture2D* make_target(ID3D11Device* device, UINT width, UINT height)
{
    D3D11_TEXTURE2D_DESC description{};
    description.Width = width;
    description.Height = height;
    description.MipLevels = 1;
    description.ArraySize = 1;
    description.Format = DXGI_FORMAT_B8G8R8A8_UNORM;
    description.SampleDesc.Count = 1;
    description.Usage = D3D11_USAGE_DEFAULT;
    description.BindFlags = D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE;
    ID3D11Texture2D* texture = nullptr;
    if (FAILED(device->CreateTexture2D(&description, nullptr, &texture))) {
        return nullptr;
    }
    return texture;
}

} // namespace

int main()
{
    stage("creating device");
    ID3D11Device* device = nullptr;
    ID3D11DeviceContext* context = nullptr;
    D3D_FEATURE_LEVEL level{};
    if (FAILED(D3D11CreateDevice(nullptr, D3D_DRIVER_TYPE_HARDWARE, nullptr, 0, nullptr, 0,
                                 D3D11_SDK_VERSION, &device, &level, &context)) ||
        !device || !context) {
        std::fprintf(stderr, "no D3D11 device available, skipping\n");
        return 0;
    }

    rsf_d3d11_state state{};
    check(rsf_d3d11_state_save(nullptr, &state) == 0, "Saving without a context must be refused.");
    check(rsf_d3d11_state_save(context, nullptr) == 0, "Saving without a state must be refused.");

    stage("creating resources");
    // Three textures, not two, and the render target is one nothing reads. A texture bound as an
    // input and as an output at the same time is unbound by the runtime with no error a shipping
    // game would see, so a test that did that would be checking the restore against a state D3D11
    // had already taken apart on its own.
    ID3D11Texture2D* first = make_target(device, 64, 64);
    ID3D11Texture2D* second = make_target(device, 32, 32);
    ID3D11Texture2D* drawn_into = make_target(device, 64, 64);
    check(first && second && drawn_into, "The test textures must be created.");
    if (!first || !second || !drawn_into) {
        return 1;
    }

    ID3D11RenderTargetView* first_target = nullptr;
    ID3D11ShaderResourceView* first_resource = nullptr;
    ID3D11ShaderResourceView* second_resource = nullptr;
    check(SUCCEEDED(device->CreateRenderTargetView(drawn_into, nullptr, &first_target)) &&
              SUCCEEDED(device->CreateShaderResourceView(first, nullptr, &first_resource)) &&
              SUCCEEDED(device->CreateShaderResourceView(second, nullptr, &second_resource)),
          "The test views must be created.");

    D3D11_SAMPLER_DESC sampler_description{};
    sampler_description.Filter = D3D11_FILTER_MIN_MAG_MIP_POINT;
    sampler_description.AddressU = D3D11_TEXTURE_ADDRESS_CLAMP;
    sampler_description.AddressV = D3D11_TEXTURE_ADDRESS_CLAMP;
    sampler_description.AddressW = D3D11_TEXTURE_ADDRESS_CLAMP;
    ID3D11SamplerState* sampler = nullptr;
    check(SUCCEEDED(device->CreateSamplerState(&sampler_description, &sampler)),
          "The test sampler must be created.");

    D3D11_BUFFER_DESC buffer_description{};
    buffer_description.ByteWidth = 256;
    buffer_description.Usage = D3D11_USAGE_DEFAULT;
    buffer_description.BindFlags = D3D11_BIND_CONSTANT_BUFFER;
    ID3D11Buffer* constants = nullptr;
    check(SUCCEEDED(device->CreateBuffer(&buffer_description, nullptr, &constants)),
          "The test constant buffer must be created.");

    D3D11_RASTERIZER_DESC raster_description{};
    raster_description.FillMode = D3D11_FILL_SOLID;
    raster_description.CullMode = D3D11_CULL_NONE;
    ID3D11RasterizerState* raster = nullptr;
    check(SUCCEEDED(device->CreateRasterizerState(&raster_description, &raster)),
          "The test rasteriser state must be created.");

    stage("setting a known state");
    D3D11_VIEWPORT viewport{};
    viewport.Width = 64.0f;
    viewport.Height = 48.0f;
    viewport.MaxDepth = 1.0f;
    const D3D11_RECT scissor{0, 0, 64, 48};

    context->OMSetRenderTargets(1, &first_target, nullptr);
    context->PSSetShaderResources(3, 1, &first_resource);
    context->CSSetShaderResources(1, 1, &second_resource);
    context->PSSetSamplers(0, 1, &sampler);
    context->PSSetConstantBuffers(2, 1, &constants);
    context->VSSetConstantBuffers(0, 1, &constants);
    context->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP);
    context->RSSetState(raster);
    context->RSSetViewports(1, &viewport);
    context->RSSetScissorRects(1, &scissor);

    stage("saving");
    check(rsf_d3d11_state_save(context, &state) == 1, "Saving the state must succeed.");

    stage("taking it apart");
    ID3D11ShaderResourceView* no_resource = nullptr;
    ID3D11SamplerState* no_sampler = nullptr;
    ID3D11Buffer* no_buffer = nullptr;
    D3D11_VIEWPORT other_viewport{};
    other_viewport.Width = 8.0f;
    other_viewport.Height = 8.0f;
    const D3D11_RECT other_scissor{0, 0, 8, 8};

    context->OMSetRenderTargets(0, nullptr, nullptr);
    context->PSSetShaderResources(3, 1, &no_resource);
    context->CSSetShaderResources(1, 1, &no_resource);
    context->PSSetSamplers(0, 1, &no_sampler);
    context->PSSetConstantBuffers(2, 1, &no_buffer);
    context->VSSetConstantBuffers(0, 1, &no_buffer);
    context->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_POINTLIST);
    context->RSSetState(nullptr);
    context->RSSetViewports(1, &other_viewport);
    context->RSSetScissorRects(1, &other_scissor);

    // The point of the test is that this is not what comes back, so it is worth checking that the
    // state really was disturbed. A save that silently did nothing would otherwise pass.
    ID3D11ShaderResourceView* cleared = nullptr;
    context->PSGetShaderResources(3, 1, &cleared);
    check(cleared == nullptr, "The state must actually be disturbed before it is restored.");

    stage("restoring");
    rsf_d3d11_state_restore(context, &state);

    ID3D11RenderTargetView* restored_target = nullptr;
    context->OMGetRenderTargets(1, &restored_target, nullptr);
    check(dropped(restored_target) == first_target, "The render target must come back.");

    ID3D11ShaderResourceView* restored_pixel = nullptr;
    context->PSGetShaderResources(3, 1, &restored_pixel);
    check(dropped(restored_pixel) == first_resource,
          "A pixel shader resource must come back, in its own slot.");

    ID3D11ShaderResourceView* restored_compute = nullptr;
    context->CSGetShaderResources(1, 1, &restored_compute);
    check(dropped(restored_compute) == second_resource,
          "A compute shader resource must come back: that is the stage a vendor runtime uses.");

    ID3D11SamplerState* restored_sampler = nullptr;
    context->PSGetSamplers(0, 1, &restored_sampler);
    check(dropped(restored_sampler) == sampler, "A sampler must come back.");

    ID3D11Buffer* restored_pixel_constants = nullptr;
    context->PSGetConstantBuffers(2, 1, &restored_pixel_constants);
    check(dropped(restored_pixel_constants) == constants,
          "A pixel constant buffer must come back, in its own slot.");

    ID3D11Buffer* restored_vertex_constants = nullptr;
    context->VSGetConstantBuffers(0, 1, &restored_vertex_constants);
    check(dropped(restored_vertex_constants) == constants,
          "A vertex constant buffer must come back.");

    D3D11_PRIMITIVE_TOPOLOGY restored_topology = D3D11_PRIMITIVE_TOPOLOGY_UNDEFINED;
    context->IAGetPrimitiveTopology(&restored_topology);
    check(restored_topology == D3D11_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP,
          "The primitive topology must come back.");

    ID3D11RasterizerState* restored_raster = nullptr;
    context->RSGetState(&restored_raster);
    check(dropped(restored_raster) == raster, "The rasteriser state must come back.");

    D3D11_VIEWPORT restored_viewport{};
    UINT viewport_count = 1;
    context->RSGetViewports(&viewport_count, &restored_viewport);
    check(viewport_count == 1 && restored_viewport.Width == 64.0f &&
              restored_viewport.Height == 48.0f,
          "The viewport must come back.");

    D3D11_RECT restored_scissor{};
    UINT scissor_count = 1;
    context->RSGetScissorRects(&scissor_count, &restored_scissor);
    check(scissor_count == 1 && restored_scissor.right == 64 && restored_scissor.bottom == 48,
          "The scissor rectangle must come back.");

    stage("releasing");
    // Unbound first, so the references the context holds are gone before the test drops its own and
    // a leak here is a leak in the restore rather than in the teardown.
    context->ClearState();
    raster->Release();
    constants->Release();
    sampler->Release();
    second_resource->Release();
    first_resource->Release();
    first_target->Release();
    drawn_into->Release();
    second->Release();
    first->Release();
    context->Release();
    device->Release();

    std::fprintf(stderr, passed ? "d3d11_state: pass\n" : "d3d11_state: fail\n");
    return passed ? 0 : 1;
}
