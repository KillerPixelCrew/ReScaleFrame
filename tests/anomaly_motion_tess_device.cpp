// SPDX-License-Identifier: GPL-3.0-only
// Why the motion stage never swaps only its VS/PS variant pair while a hull/domain shader is bound (motion_stage.cpp
// on_flush: a tessellated draw gets the whole VS+HS+DS+PS variant chain or nothing; the real chain is proven by
// anomaly_motion_tess_chain_device). With r4_enable_tessellation on, X-Ray draws PN-tessellated materials, skinned
// character models among them, as VS -> DX11\tess.hs -> DX11\tess.ds -> PS. The STOCK domain shader writes the stock
// v2p_bumped interface only. The motion PS variant reads two more interpolants (TEXCOORD12 previous clip, TEXCOORD13
// validity) that the motion VS writes but nothing carries through the stock hull and domain stages.
//
// This fixture builds that topology from first-party HLSL (no engine shader): a VS that writes the extra interpolants
// with a valid marker, an HS/DS pair that forwards only the stock-like interface, and a PS that encodes what it reads in
// those registers. On the device it reports what the PS receives and whether the debug layer calls the pipeline invalid.
// Either outcome means no defined motion: the debug layer's linkage error, or registers that do not hold the VS values.
// The same draw without HS/DS (what the variant pair is for) receives the VS values exactly.
#include <d3d11.h>
#include <d3d11sdklayers.h>
#include <d3dcompiler.h>
#include <dxgi.h>
#include <wrl/client.h>
#include <cstdio>
#include <cstring>
#include <string>
#include <vector>

namespace {
using Microsoft::WRL::ComPtr;
int failures = 0;
#define CHECK(condition) do { if (!(condition)) { std::printf("FAIL %s:%d %s\n", __FILE__, __LINE__, #condition); ++failures; } } while (0)

constexpr char source[] = R"(
struct VSOut { float4 pos : POSITION0; float4 tc0 : TEXCOORD0; float4 rsf_previous_clip : TEXCOORD12; nointerpolation float rsf_valid : TEXCOORD13; };
VSOut vs_main(uint id : SV_VertexID) {
    float2 p = float2((id << 1) & 2, id & 2);
    VSOut o;
    o.pos = float4(p * float2(2, -2) + float2(-1, 1), 0.5, 1);
    o.tc0 = float4(0.25, 0.5, 0.75, 1);
    o.rsf_previous_clip = float4(0.125, -0.25, 0.5, 1);
    o.rsf_valid = 1;
    return o;
}
struct DirectOut { float4 tc0 : TEXCOORD0; float4 rsf_previous_clip : TEXCOORD12; nointerpolation float rsf_valid : TEXCOORD13; float4 hpos : SV_Position; };
DirectOut vs_direct(uint id : SV_VertexID) {
    const VSOut v = vs_main(id);
    DirectOut o;
    o.tc0 = v.tc0; o.rsf_previous_clip = v.rsf_previous_clip; o.rsf_valid = v.rsf_valid; o.hpos = v.pos;
    return o;
}
struct HSIn { float4 pos : POSITION0; float4 tc0 : TEXCOORD0; };          // the stock control point: no RSF fields
struct HSConstants { float edges[3] : SV_TessFactor; float inside : SV_InsideTessFactor; };
HSConstants hs_constants(InputPatch<HSIn, 3> ip) { HSConstants c; c.edges[0] = c.edges[1] = c.edges[2] = 2; c.inside = 2; return c; }
[domain("tri")] [partitioning("integer")] [outputtopology("triangle_cw")] [outputcontrolpoints(3)] [patchconstantfunc("hs_constants")]
HSIn hs_main(InputPatch<HSIn, 3> ip, uint i : SV_OutputControlPointID) { return ip[i]; }
struct DSOut { float4 tc0 : TEXCOORD0; float4 hpos : SV_Position; };      // the stock v2p_bumped-like output
[domain("tri")]
DSOut ds_main(HSConstants c, float3 uvw : SV_DomainLocation, const OutputPatch<HSIn, 3> bp) {
    DSOut o;
    o.hpos = bp[0].pos * uvw.x + bp[1].pos * uvw.y + bp[2].pos * uvw.z;
    o.tc0 = bp[0].tc0 * uvw.x + bp[1].tc0 * uvw.y + bp[2].tc0 * uvw.z;
    return o;
}
struct PSIn { float4 tc0 : TEXCOORD0; float4 rsf_previous_clip : TEXCOORD12; nointerpolation float rsf_valid : TEXCOORD13; float4 hpos : SV_Position; };
float4 ps_main(PSIn i) : SV_Target { return float4(i.rsf_previous_clip.xy, i.rsf_valid, i.tc0.x); }
)";

ComPtr<ID3DBlob> compile(const char* entry, const char* target) {
    ComPtr<ID3DBlob> code, errors;
    if (FAILED(D3DCompile(source, sizeof(source) - 1, "rsf_tess_probe", nullptr, nullptr, entry, target, 0, 0, &code, &errors))) {
        std::printf("compile %s: %s\n", entry, errors ? static_cast<const char*>(errors->GetBufferPointer()) : "?");
        return {};
    }
    return code;
}

struct Probe { float value[4]{}; uint64_t errors{}; std::string first_error; };

Probe draw(ID3D11Device* device, ID3D11DeviceContext* context, bool tessellated) {
    Probe probe;
    const auto vs = compile(tessellated ? "vs_main" : "vs_direct", "vs_5_0"), hs = compile("hs_main", "hs_5_0"),
        ds = compile("ds_main", "ds_5_0"), ps = compile("ps_main", "ps_5_0");
    CHECK(vs && hs && ds && ps);
    if (!vs || !hs || !ds || !ps) return probe;
    ComPtr<ID3D11VertexShader> v;
    ComPtr<ID3D11HullShader> h;
    ComPtr<ID3D11DomainShader> d;
    ComPtr<ID3D11PixelShader> p;
    device->CreateVertexShader(vs->GetBufferPointer(), vs->GetBufferSize(), nullptr, &v);
    device->CreateHullShader(hs->GetBufferPointer(), hs->GetBufferSize(), nullptr, &h);
    device->CreateDomainShader(ds->GetBufferPointer(), ds->GetBufferSize(), nullptr, &d);
    device->CreatePixelShader(ps->GetBufferPointer(), ps->GetBufferSize(), nullptr, &p);
    D3D11_TEXTURE2D_DESC desc{8, 8, 1, 1, DXGI_FORMAT_R32G32B32A32_FLOAT, {1, 0}, D3D11_USAGE_DEFAULT, D3D11_BIND_RENDER_TARGET, 0, 0};
    ComPtr<ID3D11Texture2D> target, staging;
    ComPtr<ID3D11RenderTargetView> rtv;
    device->CreateTexture2D(&desc, nullptr, &target);
    device->CreateRenderTargetView(target.Get(), nullptr, &rtv);
    desc.Usage = D3D11_USAGE_STAGING; desc.BindFlags = 0; desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
    device->CreateTexture2D(&desc, nullptr, &staging);
    CHECK(v && h && d && p && rtv && staging);
    if (!v || !h || !d || !p || !rtv || !staging) return probe;
    ComPtr<ID3D11InfoQueue> info;
    device->QueryInterface(IID_PPV_ARGS(&info));
    if (info) info->ClearStoredMessages();
    const float clear[4]{-7, -7, -7, -7};
    context->ClearState();
    context->ClearRenderTargetView(rtv.Get(), clear);
    ID3D11RenderTargetView* views[]{rtv.Get()};
    context->OMSetRenderTargets(1, views, nullptr);
    const D3D11_VIEWPORT viewport{0, 0, 8, 8, 0, 1};
    context->RSSetViewports(1, &viewport);
    context->VSSetShader(v.Get(), nullptr, 0);
    context->PSSetShader(p.Get(), nullptr, 0);
    if (tessellated) {
        context->HSSetShader(h.Get(), nullptr, 0);
        context->DSSetShader(d.Get(), nullptr, 0);
        context->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_3_CONTROL_POINT_PATCHLIST);
    } else {
        context->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
    }
    context->Draw(3, 0);
    context->CopyResource(staging.Get(), target.Get());
    D3D11_MAPPED_SUBRESOURCE mapped{};
    if (SUCCEEDED(context->Map(staging.Get(), 0, D3D11_MAP_READ, 0, &mapped))) {
        // Texel (1,1) is inside the triangle (upper-left half of the target).
        std::memcpy(probe.value, static_cast<const std::byte*>(mapped.pData) + mapped.RowPitch + 16, 16);
        context->Unmap(staging.Get(), 0);
    }
    context->ClearState();
    if (info)
        for (UINT64 i = 0, n = info->GetNumStoredMessages(); i < n; ++i) {
            SIZE_T size = 0;
            info->GetMessage(i, nullptr, &size);
            std::vector<char> storage(size);
            auto* message = reinterpret_cast<D3D11_MESSAGE*>(storage.data());
            if (SUCCEEDED(info->GetMessage(i, message, &size)) &&
                (message->Severity == D3D11_MESSAGE_SEVERITY_ERROR || message->Severity == D3D11_MESSAGE_SEVERITY_CORRUPTION)) {
                if (!probe.errors++) probe.first_error.assign(message->pDescription, message->DescriptionByteLength);
            }
        }
    return probe;
}

bool create(UINT flags, ComPtr<ID3D11Device>& device, ComPtr<ID3D11DeviceContext>& context) {
    ComPtr<IDXGIFactory1> factory;
    if (FAILED(CreateDXGIFactory1(IID_PPV_ARGS(&factory)))) return false;
    // The NVIDIA adapter (the game's GPU on this machine), else the first hardware adapter.
    for (const UINT want : {0x10deu, 0u})
        for (UINT i = 0;; ++i) {
            ComPtr<IDXGIAdapter1> adapter;
            if (factory->EnumAdapters1(i, &adapter) == DXGI_ERROR_NOT_FOUND) break;
            DXGI_ADAPTER_DESC1 desc{};
            adapter->GetDesc1(&desc);
            if ((desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE) || (want && desc.VendorId != want)) continue;
            const D3D_FEATURE_LEVEL level = D3D_FEATURE_LEVEL_11_0;
            if (SUCCEEDED(D3D11CreateDevice(adapter.Get(), D3D_DRIVER_TYPE_UNKNOWN, nullptr, flags, &level, 1, D3D11_SDK_VERSION,
                    &device, nullptr, &context))) {
                std::printf("device: %ls (vendor 0x%04x, debug layer %s)\n", desc.Description, desc.VendorId, flags ? "on" : "off");
                return true;
            }
        }
    return false;
}
}

int main() {
    bool any = false;
    for (const UINT flags : {UINT(D3D11_CREATE_DEVICE_DEBUG), 0u}) {
        ComPtr<ID3D11Device> device;
        ComPtr<ID3D11DeviceContext> context;
        if (!create(flags, device, context)) continue;
        any = true;
        const auto direct = draw(device.Get(), context.Get(), false);
        const auto tess = draw(device.Get(), context.Get(), true);
        std::printf("  VS -> PS:            PS reads previous (%g, %g) validity %g; debug-layer errors %llu\n", direct.value[0],
            direct.value[1], direct.value[2], static_cast<unsigned long long>(direct.errors));
        std::printf("  VS -> HS -> DS -> PS: PS reads previous (%g, %g) validity %g; debug-layer errors %llu%s%s\n", tess.value[0],
            tess.value[1], tess.value[2], static_cast<unsigned long long>(tess.errors), tess.errors ? ": " : "",
            tess.first_error.substr(0, 220).c_str());
        // Without HS/DS the variant interface carries the VS values exactly.
        CHECK(direct.errors == 0 && direct.value[0] == 0.125f && direct.value[1] == -0.25f && direct.value[2] == 1.0f);
        // With HS/DS bound the motion is never defined: the debug layer rejects the linkage, or the registers do not hold
        // the VS's valid vector.
        const bool defined = tess.errors == 0 && tess.value[0] == 0.125f && tess.value[1] == -0.25f && tess.value[2] == 1.0f;
        CHECK(!defined);
        if (flags) CHECK(tess.errors > 0);
    }
    if (!any) { std::printf("SKIP: no D3D11 hardware device\n"); return 77; }
    std::printf("%d failure(s)\n", failures);
    return failures ? 1 : 0;
}
