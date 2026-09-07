/* SPDX-License-Identifier: GPL-3.0-only */
/* The layer the interface is diverted into, and the alpha question that decides whether it works.
 *
 * The layer itself is simple. What is not simple, and what this test exists for, is that a
 * transparent surface only accumulates coverage if the draws landing on it write alpha. Unreal's
 * Slate blend does. Unreal's base pass translucent blend does not: its alpha factors are
 * `Zero / InvSrcAlpha`, so alpha stays at whatever the clear left, and a layer cleared to zero
 * stays at zero no matter how much colour is drawn onto it. A layer like that composites to
 * nothing and looks exactly like a broken divert.
 *
 * So the divert has to patch the alpha operations of the blends it moves, and this is where that
 * arithmetic is pinned.
 */
#include <rescaleframe/ui_layer.h>

#include <windows.h>

#include <d3d11.h>

#include <cstdio>
#include <cstdint>
#include <cmath>
#include <cstring>

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
    const uint8_t* p = static_cast<const uint8_t*>(mapped.pData);
    out.r = p[0] / 255.0f;
    out.g = p[1] / 255.0f;
    out.b = p[2] / 255.0f;
    out.a = p[3] / 255.0f;
    context->Unmap(readable, 0);
    readable->Release();
    return true;
}

bool near_enough(float actual, float expected, float budget)
{
    return std::fabs(actual - expected) <= budget;
}

/* A shader that emits a constant colour and alpha, which is all these cases need: what is being
   tested is the blend, not the pixel. */
const char* const source = R"(
cbuffer Params : register(b0) { float4 Colour; };
float4 vertex_main(uint id : SV_VertexID) : SV_Position
{
    float2 uv = float2((id << 1) & 2, id & 2);
    return float4(uv * float2(2.0, -2.0) + float2(-1.0, 1.0), 0.0, 1.0);
}
float4 pixel_main() : SV_Target { return Colour; }
)";

using compile_fn = HRESULT(WINAPI*)(LPCVOID, SIZE_T, LPCSTR, const D3D_SHADER_MACRO*, void*, LPCSTR,
                                    LPCSTR, UINT, UINT, ID3DBlob**, ID3DBlob**);

compile_fn load_compiler()
{
    const HMODULE module =
        LoadLibraryExW(L"d3dcompiler_47.dll", nullptr, LOAD_LIBRARY_SEARCH_SYSTEM32);
    if (!module) {
        return nullptr;
    }
    return reinterpret_cast<compile_fn>(
        reinterpret_cast<void*>(GetProcAddress(module, "D3DCompile")));
}

} // namespace

int main()
{
    stage("creating device");
    ID3D11Device* device = nullptr;
    ID3D11DeviceContext* context = nullptr;
    const D3D_FEATURE_LEVEL wanted[] = {D3D_FEATURE_LEVEL_11_0};
    D3D_FEATURE_LEVEL obtained{};
    HRESULT created = D3D11CreateDevice(nullptr, D3D_DRIVER_TYPE_HARDWARE, nullptr, 0, wanted, 1,
                                        D3D11_SDK_VERSION, &device, &obtained, &context);
    if (FAILED(created)) {
        created = D3D11CreateDevice(nullptr, D3D_DRIVER_TYPE_WARP, nullptr, 0, wanted, 1,
                                    D3D11_SDK_VERSION, &device, &obtained, &context);
    }
    if (FAILED(created) || !device || !context) {
        std::fprintf(stderr, "no D3D11 device available, skipping\n");
        return 77;
    }

    stage("creating the layer");
    rsf_ui_layer_setup setup{};
    setup.struct_size = sizeof(setup);
    setup.abi_version = RSF_UI_LAYER_ABI_VERSION + 1u;
    setup.width = 8;
    setup.height = 8;
    rsf_ui_layer* layer = nullptr;
    check(rsf_ui_layer_create(device, &setup, &layer) == RSF_UI_LAYER_ERROR_ABI_MISMATCH,
          "A layer built against another version of this header must be refused.");
    setup.abi_version = RSF_UI_LAYER_ABI_VERSION;
    setup.width = 0;
    check(rsf_ui_layer_create(device, &setup, &layer) == RSF_UI_LAYER_ERROR_INVALID_ARGUMENT,
          "A layer with no extent must be refused: it exists to match the back buffer.");
    setup.width = 8;

    check(rsf_ui_layer_create(device, &setup, &layer) == RSF_UI_LAYER_OK && layer,
          "The layer must be created.");
    if (!layer) {
        return 1;
    }

    stage("nothing is bound before a frame starts");
    check(rsf_ui_layer_target(layer) == nullptr && rsf_ui_layer_source(layer) == nullptr,
          "Before the first frame there is no current slot, and handing out one of them would be "
          "handing out a surface nothing has cleared this frame.");

    stage("a frame starts transparent");
    {
        check(rsf_ui_layer_begin_frame(layer, context) == RSF_UI_LAYER_OK, "A frame must begin.");
        auto* texture = static_cast<ID3D11Texture2D*>(rsf_ui_layer_texture(layer));
        check(texture != nullptr, "The frame must have a texture.");
        Pixel out{};
        check(read_pixel(device, context, texture, out), "The layer must be readable.");
        check(out.a == 0.0f && out.r == 0.0f,
              "A frame must begin cleared to zero, not to black. Alpha zero is what makes the "
              "composite leave the scene alone where there is no interface.");
    }

    stage("the ring alternates and keeps the finished frame");
    {
        rsf_ui_layer_status status{};
        status.struct_size = sizeof(status);
        rsf_ui_layer_get_status(layer, &status);
        const uint32_t first = status.slot;
        void* first_texture = rsf_ui_layer_texture(layer);

        rsf_ui_layer_begin_frame(layer, context);
        rsf_ui_layer_get_status(layer, &status);
        check(status.slot != first,
              "A second frame must use the other slot, so a vendor holding the previous layer "
              "until its next present is not reading the one being drawn.");
        check(rsf_ui_layer_texture(layer) != first_texture, "And a different texture.");

        rsf_ui_layer_begin_frame(layer, context);
        rsf_ui_layer_get_status(layer, &status);
        check(status.slot == first, "A third frame must come back round.");
    }

    stage("the written flag gates the composite");
    {
        rsf_ui_layer_begin_frame(layer, context);
        rsf_ui_layer_status status{};
        status.struct_size = sizeof(status);
        rsf_ui_layer_get_status(layer, &status);
        check(status.written_this_frame == 0,
              "A fresh frame has nothing written, so the composite is skipped and a frame with no "
              "interface costs nothing.");
        const uint64_t before = status.frames_written;

        rsf_ui_layer_mark_written(layer);
        rsf_ui_layer_mark_written(layer);
        rsf_ui_layer_get_status(layer, &status);
        check(status.written_this_frame == 1, "A divert must set it.");
        check(status.frames_written == before + 1,
              "And many diverts in one frame must count as one written frame, or the number "
              "describes draws rather than frames and cannot be compared with presents.");

        rsf_ui_layer_begin_frame(layer, context);
        rsf_ui_layer_get_status(layer, &status);
        check(status.written_this_frame == 0, "The next frame starts unwritten again.");
    }

    stage("alpha accumulates only when the blend writes it");
    {
        // This is the case the divert exists to handle, and the reason it has to patch blends.
        const compile_fn compile = load_compiler();
        if (!compile) {
            std::fprintf(stderr, "d3dcompiler_47.dll is not available, skipping the blend cases\n");
        } else {
            ID3DBlob* vertex_code = nullptr;
            ID3DBlob* pixel_code = nullptr;
            ID3DBlob* errors = nullptr;
            compile(source, std::strlen(source), "ui_layer_test", nullptr, nullptr, "vertex_main",
                    "vs_5_0", 0, 0, &vertex_code, &errors);
            if (errors) {
                errors->Release();
                errors = nullptr;
            }
            compile(source, std::strlen(source), "ui_layer_test", nullptr, nullptr, "pixel_main",
                    "ps_5_0", 0, 0, &pixel_code, &errors);
            if (errors) {
                errors->Release();
            }
            check(vertex_code && pixel_code, "The test shaders must compile.");

            if (vertex_code && pixel_code) {
                ID3D11VertexShader* vertex = nullptr;
                ID3D11PixelShader* pixel = nullptr;
                device->CreateVertexShader(vertex_code->GetBufferPointer(),
                                           vertex_code->GetBufferSize(), nullptr, &vertex);
                device->CreatePixelShader(pixel_code->GetBufferPointer(),
                                          pixel_code->GetBufferSize(), nullptr, &pixel);

                D3D11_BUFFER_DESC constants{};
                constants.ByteWidth = 16;
                constants.Usage = D3D11_USAGE_DYNAMIC;
                constants.BindFlags = D3D11_BIND_CONSTANT_BUFFER;
                constants.CPUAccessFlags = D3D11_CPU_ACCESS_WRITE;
                ID3D11Buffer* colour = nullptr;
                device->CreateBuffer(&constants, nullptr, &colour);

                // Unreal's base pass translucent blend. Colour is over; alpha is Zero/InvSrcAlpha,
                // so alpha is only ever multiplied by (1 - src.a) and never added to.
                D3D11_BLEND_DESC translucent{};
                translucent.RenderTarget[0].BlendEnable = TRUE;
                translucent.RenderTarget[0].SrcBlend = D3D11_BLEND_SRC_ALPHA;
                translucent.RenderTarget[0].DestBlend = D3D11_BLEND_INV_SRC_ALPHA;
                translucent.RenderTarget[0].BlendOp = D3D11_BLEND_OP_ADD;
                translucent.RenderTarget[0].SrcBlendAlpha = D3D11_BLEND_ZERO;
                translucent.RenderTarget[0].DestBlendAlpha = D3D11_BLEND_INV_SRC_ALPHA;
                translucent.RenderTarget[0].BlendOpAlpha = D3D11_BLEND_OP_ADD;
                translucent.RenderTarget[0].RenderTargetWriteMask = D3D11_COLOR_WRITE_ENABLE_ALL;

                // The same blend with its alpha operations patched, which is what the divert does:
                // colour factors untouched, alpha becomes One/InvSrcAlpha so coverage accumulates.
                D3D11_BLEND_DESC patched = translucent;
                patched.RenderTarget[0].SrcBlendAlpha = D3D11_BLEND_ONE;

                // And a write mask without its alpha bit, which is what AC7's interface blend
                // actually has and what the first three extraction runs tripped over. A target
                // whose alpha nobody reads is ordinarily drawn this way, so it is not a strange
                // case; it just makes every alpha operation above it decorative.
                D3D11_BLEND_DESC masked = patched;
                masked.RenderTarget[0].RenderTargetWriteMask =
                    D3D11_COLOR_WRITE_ENABLE_RED | D3D11_COLOR_WRITE_ENABLE_GREEN |
                    D3D11_COLOR_WRITE_ENABLE_BLUE;
                ID3D11BlendState* masked_state = nullptr;
                device->CreateBlendState(&masked, &masked_state);

                ID3D11BlendState* unpatched_state = nullptr;
                ID3D11BlendState* patched_state = nullptr;
                device->CreateBlendState(&translucent, &unpatched_state);
                device->CreateBlendState(&patched, &patched_state);

                auto draw_once = [&](ID3D11BlendState* blend, float r, float g, float b, float a) {
                    D3D11_MAPPED_SUBRESOURCE mapped{};
                    if (SUCCEEDED(context->Map(colour, 0, D3D11_MAP_WRITE_DISCARD, 0, &mapped))) {
                        float* values = static_cast<float*>(mapped.pData);
                        values[0] = r;
                        values[1] = g;
                        values[2] = b;
                        values[3] = a;
                        context->Unmap(colour, 0);
                    }
                    auto* target = static_cast<ID3D11RenderTargetView*>(rsf_ui_layer_target(layer));
                    context->OMSetRenderTargets(1, &target, nullptr);
                    context->VSSetShader(vertex, nullptr, 0);
                    context->PSSetShader(pixel, nullptr, 0);
                    context->PSSetConstantBuffers(0, 1, &colour);
                    context->IASetInputLayout(nullptr);
                    context->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
                    const FLOAT factor[4] = {0, 0, 0, 0};
                    context->OMSetBlendState(blend, factor, 0xffffffffu);
                    D3D11_VIEWPORT viewport{};
                    viewport.Width = 8.0f;
                    viewport.Height = 8.0f;
                    viewport.MaxDepth = 1.0f;
                    context->RSSetViewports(1, &viewport);
                    context->Draw(3, 0);
                };

                rsf_ui_layer_begin_frame(layer, context);
                draw_once(unpatched_state, 1.0f, 1.0f, 1.0f, 0.5f);
                Pixel out{};
                read_pixel(device, context,
                           static_cast<ID3D11Texture2D*>(rsf_ui_layer_texture(layer)), out);
                check(near_enough(out.a, 0.0f, 1.0f / 255.0f),
                      "Unreal's translucent blend leaves alpha at zero on a transparent layer. "
                      "This is why diverting draws unchanged produces a layer that composites to "
                      "nothing and looks exactly like a divert that never happened.");
                check(near_enough(out.r, 0.5f, 2.0f / 255.0f),
                      "While the colour lands normally, which is what makes the failure so easy "
                      "to misread.");

                rsf_ui_layer_begin_frame(layer, context);
                draw_once(patched_state, 1.0f, 1.0f, 1.0f, 0.5f);
                read_pixel(device, context,
                           static_cast<ID3D11Texture2D*>(rsf_ui_layer_texture(layer)), out);
                check(near_enough(out.a, 0.5f, 2.0f / 255.0f),
                      "With the alpha operations patched, coverage accumulates.");

                // Two draws, so the accumulation is a1 + a2 * (1 - a1) rather than a replacement.
                draw_once(patched_state, 1.0f, 1.0f, 1.0f, 0.5f);
                read_pixel(device, context,
                           static_cast<ID3D11Texture2D*>(rsf_ui_layer_texture(layer)), out);
                check(near_enough(out.a, 0.75f, 3.0f / 255.0f),
                      "And a second draw gives a1 + a2 (1 - a1), which is the over operator: the "
                      "interface is many overlapping draws and the layer has to end up with the "
                      "coverage they jointly have.");

                rsf_ui_layer_begin_frame(layer, context);
                draw_once(masked_state, 1.0f, 1.0f, 1.0f, 0.5f);
                read_pixel(device, context,
                           static_cast<ID3D11Texture2D*>(rsf_ui_layer_texture(layer)), out);
                check(near_enough(out.a, 0.0f, 1.0f / 255.0f),
                      "Patched alpha operations under a write mask that excludes alpha still "
                      "accumulate nothing. This is the failure that made an extracted interface "
                      "arrive black over a black scene and washed out over a lit one: the layer "
                      "had colour and no coverage, and a premultiplied composite of colour with "
                      "zero coverage contributes nothing.");
                check(near_enough(out.r, 0.5f, 2.0f / 255.0f),
                      "While the colour lands normally, which is why every counter said the "
                      "divert had worked.");

                if (masked_state) {
                    masked_state->Release();
                }
                if (unpatched_state) {
                    unpatched_state->Release();
                }
                if (patched_state) {
                    patched_state->Release();
                }
                if (colour) {
                    colour->Release();
                }
                if (vertex) {
                    vertex->Release();
                }
                if (pixel) {
                    pixel->Release();
                }
            }
            if (vertex_code) {
                vertex_code->Release();
            }
            if (pixel_code) {
                pixel_code->Release();
            }
        }
    }

    stage("arguments are checked");
    {
        check(rsf_ui_layer_begin_frame(nullptr, context) == RSF_UI_LAYER_ERROR_INVALID_ARGUMENT,
              "A null layer.");
        check(rsf_ui_layer_begin_frame(layer, nullptr) == RSF_UI_LAYER_ERROR_INVALID_ARGUMENT,
              "A null context.");
        rsf_ui_layer_status short_status{};
        short_status.struct_size = 4;
        check(rsf_ui_layer_get_status(layer, &short_status) == RSF_UI_LAYER_ERROR_INVALID_ARGUMENT,
              "A short status structure.");
        check(rsf_ui_layer_target(nullptr) == nullptr, "A null layer has no target.");
        rsf_ui_layer_mark_written(nullptr);
    }

    rsf_ui_layer_destroy(layer);
    rsf_ui_layer_destroy(nullptr);
    context->Release();
    device->Release();

    std::fprintf(stderr, "%s\n", passed ? "ui_layer: all checks passed" : "ui_layer: FAILED");
    return passed ? 0 : 1;
}
