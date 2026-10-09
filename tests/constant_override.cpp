// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/frame_tap.h>
#include <rescaleframe/constant_twin.h>
#include <windows.h>
#include <d3d11.h>
#include <d3dcompiler.h>
#include <cstdio>
#include <initializer_list>
#include <cstring>

// What the constant watch was handed by an UpdateSubresource.
struct Watched {
    const void* contents = nullptr;
    uint32_t bytes = 0;
    unsigned calls = 0;
    bool write = false;
};

static void watch_constants(void* user, void*, void* contents, uint32_t bytes)
{
    auto& seen = *static_cast<Watched*>(user);
    seen.contents = contents;
    seen.bytes = bytes;
    ++seen.calls;
    if (seen.write) {
        static_cast<float*>(contents)[0] = 9.0f;
    }
}

struct Bindings {
    ID3D11Buffer *vs, *ps, *replacement_vs, *replacement_ps;
    unsigned calls = 0;
    ID3D11Buffer *extra = nullptr, *replacement_extra = nullptr;
};

static int replace_constants(void* user, const rsf_frame_tap_target_draw* draw,
                             uint32_t* slots, void** buffers)
{
    auto& b = *static_cast<Bindings*>(user);
    ++b.calls;
    if (draw->vertex_constants[13] != b.vs || draw->pixel_constants[7] != b.ps) {
        return 0;
    }
    slots[0] = 13; buffers[0] = b.replacement_vs;
    slots[1] = 14 + 7; buffers[1] = b.replacement_ps;
    // Duplicate slots must not corrupt the restore list.
    slots[2] = 13; buffers[2] = b.replacement_ps;
    if (b.extra && draw->geometry_constants[6] == b.extra &&
        draw->hull_constants[5] == b.extra && draw->domain_constants[4] == b.extra) {
        slots[3] = 28 + 6; buffers[3] = b.replacement_extra;
        slots[4] = 42 + 5; buffers[4] = b.replacement_extra;
        slots[5] = 56 + 4; buffers[5] = b.replacement_extra;
        return 6;
    }
    return 3;
}

int main()
{
    bool passed = true;
    auto check = [&](bool good, const char* why) {
        if (!good) { std::fprintf(stderr, "%s\n", why); passed = false; }
    };
    ID3D11Device* device = nullptr;
    ID3D11DeviceContext* context = nullptr;
    if (FAILED(D3D11CreateDevice(nullptr, D3D_DRIVER_TYPE_WARP, nullptr, 0, nullptr, 0,
                                D3D11_SDK_VERSION, &device, nullptr, &context))) {
        return 77;
    }
    const char* vs_text =
        "cbuffer V:register(b13){float4 value;} struct O{float4 p:SV_Position;float r:TEXCOORD0;};"
        "O main(uint id:SV_VertexID){O o;o.p=float4(id==2?3:-1,id==1?3:-1,0,1);o.r=value.x;return o;}";
    const char* ps_text =
        "cbuffer P:register(b7){float4 value;} float4 main(float4 p:SV_Position,float r:TEXCOORD0):SV_Target"
        "{return float4(r,value.x,0,1);}";
    ID3DBlob *vs_code = nullptr, *ps_code = nullptr;
    check(SUCCEEDED(D3DCompile(vs_text, std::strlen(vs_text), nullptr, nullptr, nullptr, "main",
                              "vs_5_0", 0, 0, &vs_code, nullptr)) &&
          SUCCEEDED(D3DCompile(ps_text, std::strlen(ps_text), nullptr, nullptr, nullptr, "main",
                              "ps_5_0", 0, 0, &ps_code, nullptr)), "Test shaders must compile.");
    if (!passed) { return 1; }
    ID3D11VertexShader* vs = nullptr;
    ID3D11PixelShader* ps = nullptr;
    check(SUCCEEDED(device->CreateVertexShader(vs_code->GetBufferPointer(), vs_code->GetBufferSize(), nullptr, &vs)) &&
          SUCCEEDED(device->CreatePixelShader(ps_code->GetBufferPointer(), ps_code->GetBufferSize(), nullptr, &ps)),
          "Test shaders must be created.");
    auto buffer = [&](float value) {
        D3D11_BUFFER_DESC desc{};
        desc.ByteWidth = 16; desc.BindFlags = D3D11_BIND_CONSTANT_BUFFER;
        float values[4] = {value, 0, 0, 0};
        D3D11_SUBRESOURCE_DATA data{}; data.pSysMem = values;
        ID3D11Buffer* result = nullptr;
        check(SUCCEEDED(device->CreateBuffer(&desc, &data, &result)), "Constant allocation must succeed.");
        return result;
    };
    Bindings bindings{buffer(0.1f), buffer(0.2f), buffer(0.7f), buffer(0.8f)};
    D3D11_TEXTURE2D_DESC desc{};
    desc.Width = desc.Height = 16; desc.MipLevels = desc.ArraySize = 1;
    desc.Format = DXGI_FORMAT_R8G8B8A8_UNORM; desc.SampleDesc.Count = 1;
    desc.BindFlags = D3D11_BIND_RENDER_TARGET;
    ID3D11Texture2D *target = nullptr, *staging = nullptr;
    ID3D11RenderTargetView* rtv = nullptr;
    check(SUCCEEDED(device->CreateTexture2D(&desc, nullptr, &target)) &&
          SUCCEEDED(device->CreateRenderTargetView(target, nullptr, &rtv)), "A target must be created.");
    desc.BindFlags = 0; desc.Usage = D3D11_USAGE_STAGING; desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
    check(SUCCEEDED(device->CreateTexture2D(&desc, nullptr, &staging)), "Readback must be created.");
    if (!passed) { return 1; }
    // Bind before installation to exercise inherited state on the very first overridden draw.
    context->VSSetConstantBuffers(13, 1, &bindings.vs);
    context->PSSetConstantBuffers(7, 1, &bindings.ps);
    rsf_frame_tap_options options{};
    options.struct_size = sizeof(options); options.abi_version = RSF_FRAME_TAP_ABI_VERSION;
    check(rsf_frame_tap_install(context, &options) == RSF_FRAME_TAP_OK, "The tap must install.");
    rsf_frame_tap_set_constant_override(replace_constants, &bindings);
    rsf_frame_tap_set_constant_override_format(DXGI_FORMAT_R8G8B8A8_UNORM);
    auto bind_draw = [&] {
        context->OMSetRenderTargets(1, &rtv, nullptr);
        context->VSSetShader(vs, nullptr, 0); context->PSSetShader(ps, nullptr, 0);
        context->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        const D3D11_VIEWPORT viewport{0, 0, 16, 16, 0, 1};
        context->RSSetViewports(1, &viewport);
    };
    auto pixels = [&](unsigned red, unsigned green) {
        context->CopyResource(staging, target);
        D3D11_MAPPED_SUBRESOURCE mapped{};
        check(SUCCEEDED(context->Map(staging, 0, D3D11_MAP_READ, 0, &mapped)), "Readback must map.");
        if (mapped.pData) {
            const auto* px = static_cast<const unsigned char*>(mapped.pData) + mapped.RowPitch * 8 + 8 * 4;
            check(px[0] + 1u >= red && px[0] <= red + 1u && px[1] + 1u >= green && px[1] <= green + 1u,
                  "VS and PS must both read their replacements during the draw.");
            context->Unmap(staging, 0);
        }
    };
    bind_draw();
    context->Draw(3, 0);
    pixels(179, 204);
    ID3D11Buffer *actual_vs = nullptr, *actual_ps = nullptr;
    context->VSGetConstantBuffers(13, 1, &actual_vs);
    context->PSGetConstantBuffers(7, 1, &actual_ps);
    check(actual_vs == bindings.vs && actual_ps == bindings.ps, "Both original stage bindings must be restored.");
    if (actual_vs) { actual_vs->Release(); }
    if (actual_ps) { actual_ps->Release(); }
    context->DrawInstanced(3, 1, 0, 0);
    pixels(179, 204);

    // A tessellated terrain projects after the VS. Each later stage contributes to the output so
    // missing HS/DS/GS overrides changes a measured pixel, rather than only a binding counter.
    const char* hs_text =
        "cbuffer H:register(b5){float4 value;} struct O{float4 p:SV_Position;float r:TEXCOORD0;};"
        "struct P{float e[3]:SV_TessFactor;float inside:SV_InsideTessFactor;};"
        "P patch(InputPatch<O,3> v){P p;p.e[0]=p.e[1]=p.e[2]=p.inside=1;return p;}"
        "[domain(\"tri\")][partitioning(\"integer\")][outputtopology(\"triangle_cw\")]"
        "[outputcontrolpoints(3)][patchconstantfunc(\"patch\")]"
        "O main(InputPatch<O,3> v,uint id:SV_OutputControlPointID){O o=v[id];o.r+=value.x;return o;}";
    const char* ds_text =
        "cbuffer D:register(b4){float4 value;} struct O{float4 p:SV_Position;float r:TEXCOORD0;};"
        "struct P{float e[3]:SV_TessFactor;float inside:SV_InsideTessFactor;};"
        "[domain(\"tri\")] O main(P p,float3 b:SV_DomainLocation,const OutputPatch<O,3> v){"
        "O o;o.p=v[0].p*b.x+v[1].p*b.y+v[2].p*b.z;"
        "o.r=v[0].r*b.x+v[1].r*b.y+v[2].r*b.z+value.x;return o;}";
    const char* gs_text =
        "cbuffer G:register(b6){float4 value;} struct O{float4 p:SV_Position;float r:TEXCOORD0;};"
        "[maxvertexcount(3)] void main(triangle O v[3],inout TriangleStream<O> s){"
        "for(uint i=0;i<3;i++){O o=v[i];o.r+=value.x;s.Append(o);}s.RestartStrip();}";
    ID3DBlob *hc = nullptr, *dc = nullptr, *gc = nullptr;
    check(SUCCEEDED(D3DCompile(hs_text, std::strlen(hs_text), nullptr, nullptr, nullptr, "main", "hs_5_0", 0, 0, &hc, nullptr)) &&
          SUCCEEDED(D3DCompile(ds_text, std::strlen(ds_text), nullptr, nullptr, nullptr, "main", "ds_5_0", 0, 0, &dc, nullptr)) &&
          SUCCEEDED(D3DCompile(gs_text, std::strlen(gs_text), nullptr, nullptr, nullptr, "main", "gs_5_0", 0, 0, &gc, nullptr)),
          "Tessellation and geometry test shaders must compile.");
    if (!passed) { return 1; }
    ID3D11HullShader* hs = nullptr;
    ID3D11DomainShader* ds = nullptr;
    ID3D11GeometryShader* gs = nullptr;
    check(SUCCEEDED(device->CreateHullShader(hc->GetBufferPointer(), hc->GetBufferSize(), nullptr, &hs)) &&
          SUCCEEDED(device->CreateDomainShader(dc->GetBufferPointer(), dc->GetBufferSize(), nullptr, &ds)) &&
          SUCCEEDED(device->CreateGeometryShader(gc->GetBufferPointer(), gc->GetBufferSize(), nullptr, &gs)),
          "Later-stage shaders must be created.");
    if (!passed) { return 1; }
    bindings.extra = buffer(0.01f); bindings.replacement_extra = buffer(0.05f);
    context->HSSetShader(hs, nullptr, 0); context->DSSetShader(ds, nullptr, 0); context->GSSetShader(gs, nullptr, 0);
    context->HSSetConstantBuffers(5, 1, &bindings.extra);
    context->DSSetConstantBuffers(4, 1, &bindings.extra);
    context->GSSetConstantBuffers(6, 1, &bindings.extra);
    context->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_3_CONTROL_POINT_PATCHLIST);
    context->Draw(3, 0);
    pixels(217, 204);
    ID3D11Buffer* restored[3]{};
    context->GSGetConstantBuffers(6, 1, &restored[0]);
    context->HSGetConstantBuffers(5, 1, &restored[1]);
    context->DSGetConstantBuffers(4, 1, &restored[2]);
    for (auto* item : restored) {
        check(item == bindings.extra, "Later-stage original constants must be restored after the draw.");
        if (item) { item->Release(); }
    }
    // Reproduce a stale cache entry naming a smaller material allocation at a former view address.
    rsf_constant_twins* cache = rsf_constant_twins_create(device, 4096);
    float view_data[1024]{};
    check(cache && rsf_constant_twins_write(cache, context, bindings.vs, view_data),
          "The cache fixture must contain an old view entry.");
    check(!rsf_constant_twins_find(cache, bindings.vs),
          "A 16-byte material buffer must not receive a stale 4096-byte view twin.");
    rsf_constant_twins_destroy(cache);

    // The constant watch sees UpdateSubresource uploads on a copy it may write to, because the
    // game's own memory is not its to change. A watch that says it only reads is shown that memory
    // itself, and pays for no copy.
    {
        ID3D11Buffer* uploaded = buffer(0.0f);
        D3D11_BUFFER_DESC read_desc{};
        read_desc.ByteWidth = 16;
        read_desc.Usage = D3D11_USAGE_STAGING;
        read_desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
        ID3D11Buffer* readback = nullptr;
        check(uploaded && SUCCEEDED(device->CreateBuffer(&read_desc, nullptr, &readback)),
              "The upload fixture must be created.");
        auto first_float = [&] {
            float value = -1.0f;
            context->CopyResource(readback, uploaded);
            D3D11_MAPPED_SUBRESOURCE mapped{};
            if (SUCCEEDED(context->Map(readback, 0, D3D11_MAP_READ, 0, &mapped))) {
                value = *static_cast<const float*>(mapped.pData);
                context->Unmap(readback, 0);
            }
            return value;
        };
        if (uploaded && readback) {
            Watched seen;
            float upload[4] = {1.0f, 0.0f, 0.0f, 0.0f};
            rsf_frame_tap_set_constant_watch(16, watch_constants, &seen);
            seen.write = true;
            context->UpdateSubresource(uploaded, 0, nullptr, upload, 16, 0);
            check(seen.calls == 1 && seen.bytes == 16 && seen.contents != upload,
                  "A writing watch must be handed a copy of the upload.");
            check(upload[0] == 1.0f, "The game's own memory must not be written by the watch.");
            check(first_float() == 9.0f, "What the watch writes must be what the buffer receives.");

            rsf_frame_tap_set_constant_watch_writable(0);
            seen.write = false;
            context->UpdateSubresource(uploaded, 0, nullptr, upload, 16, 0);
            check(seen.calls == 2 && seen.contents == upload,
                  "A watch that only reads must be handed the game's own memory.");
            check(first_float() == 1.0f, "An unwritten upload must arrive as the game made it.");

            // A buffer of another width is not the watch's business, and the verdict for it must
            // not be taken from the one remembered for the first.
            rsf_frame_tap_set_constant_watch_writable(1);
            rsf_frame_tap_set_constant_watch(32, watch_constants, &seen);
            context->UpdateSubresource(uploaded, 0, nullptr, upload, 16, 0);
            check(seen.calls == 2, "A buffer of another width than the one watched must be skipped.");

            // Arming a watch makes it a writer again, so a watch that wrote while the last one only
            // read cannot reach the game's memory.
            rsf_frame_tap_set_constant_watch_writable(0);
            rsf_frame_tap_set_constant_watch(16, watch_constants, &seen);
            seen.write = true;
            context->UpdateSubresource(uploaded, 0, nullptr, upload, 16, 0);
            check(seen.calls == 3 && seen.contents != upload && upload[0] == 1.0f,
                  "A newly armed watch must be handed a copy until it says it only reads.");
            rsf_frame_tap_set_constant_watch(0, nullptr, nullptr);
        }
        if (readback) { readback->Release(); }
        if (uploaded) { uploaded->Release(); }
    }
    hc->Release(); dc->Release(); gc->Release(); hs->Release(); ds->Release(); gs->Release();
    context->ClearState();
    bind_draw();
    context->Draw(3, 0);
    pixels(0, 0);
    check(bindings.calls == 4, "The first, instanced, tessellated and post-ClearState draws must each be offered.");
    rsf_frame_tap_set_constant_override(nullptr, nullptr);
    rsf_frame_tap_uninstall();
    context->ClearState();
    for (auto* item : {bindings.vs, bindings.ps, bindings.replacement_vs, bindings.replacement_ps}) { item->Release(); }
    bindings.extra->Release(); bindings.replacement_extra->Release();
    vs_code->Release(); ps_code->Release(); vs->Release(); ps->Release();
    rtv->Release(); target->Release(); staging->Release(); context->Release(); device->Release();
    return passed ? 0 : 1;
}
