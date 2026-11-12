// SPDX-License-Identifier: GPL-3.0-only
// Motion MRT device fixture. Uses the LOCAL deployment from tools/anomaly_motion_deploy.py (licensed shader bodies,
// never tracked; RSF_ANOMALY_MOTION_DEPLOY or <repo>/.local/anomaly/motion-deploy). Without it the test exits 77.
//
// 1. Every allowlisted root compiles through the production ShaderLoader with the engine's d3dcompiler_43: the stock
//    permutation is compiled from original/, handed in as "engine bytecode", reproduced byte for byte, and its motion
//    variant compiled from motion/ and reflection-checked. A one-byte change of the engine bytecode is refused.
// 2. deffer_base_flat VS/PS: the stock pair and the variant pair draw the same triangle into a real G-buffer
//    (RGBA16F position, RGBA8 albedo, D24S8) with the RGBA16F motion target from MotionDevice as RTV2 and b13 from the
//    production packers. Native MRT0/1 must be bit-identical; motion must equal the CPU value of the supplied previous
//    WVP; missing history must write all-zero motion.
// 3. The invalidation replay: a stock-shader occluder drawn over the valid motion leaves stale vectors; the replay
//    zeroes exactly the samples it won; an occluder behind the surface changes nothing; the OM state is restored.
// 4. Foliage. Every deployed tree/detail root exposes the constants the draw-time reader locates from the stock
//    reflection. A real deffer_tree_flat variant draws a sheared crown with wind history (production packer): the motion
//    equals the CPU model of the wind difference, the base does not move, an unchanged wind is valid zero, a camera pan
//    is the camera vector, and missing history is zero. A real deffer_detail_w_flat variant draws two plants whose
//    previous rows were batched into other groups: the production FoliageHistory remaps them to the current groups, the
//    motion equals the CPU model including the plant that grew, and a plant without history stays zero.
#include "motion_d3d11.h"
#include "motion_foliage.h"
#include "shader_d3d.h"
#include "shader_loader.h"
#include <d3d11.h>
#include <d3d11sdklayers.h>
#include <d3dcompiler.h>
#include <dxgi.h>
#include <wrl/client.h>
#include <algorithm>
#include <bit>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <map>
#include <sstream>
#include <stdexcept>
#include <string>
#include <vector>

using Microsoft::WRL::ComPtr;
using namespace rsf::anomaly::motion;
namespace temporal = rsf::anomaly::temporal;

namespace {
constexpr UINT size = 64;
ComPtr<ID3D11Device> device;
ComPtr<ID3D11DeviceContext> context;
int checks = 0;
void require(bool ok, const std::string& message)
{
    ++checks;
    if (!ok) throw std::runtime_error(message);
}
void hr(HRESULT value, const char* what)
{
    if (FAILED(value)) throw std::runtime_error(std::string(what) + " failed");
}
void debug_errors()
{
    ComPtr<ID3D11InfoQueue> queue;
    if (FAILED(device.As(&queue))) return;
    const UINT64 count = queue->GetNumStoredMessages();
    for (UINT64 i = 0; i < count; ++i) {
        SIZE_T length = 0;
        queue->GetMessage(i, nullptr, &length);
        std::vector<char> storage(length);
        auto* message = reinterpret_cast<D3D11_MESSAGE*>(storage.data());
        queue->GetMessage(i, message, &length);
        if (message->Severity <= D3D11_MESSAGE_SEVERITY_ERROR)
            throw std::runtime_error(std::string("debug layer: ") + std::string(message->pDescription, message->DescriptionByteLength));
    }
    queue->ClearStoredMessages();
}
std::vector<std::byte> file_bytes(const std::filesystem::path& path)
{
    std::ifstream file(path, std::ios::binary);
    require(file.good(), "missing " + path.string());
    std::vector<char> data((std::istreambuf_iterator<char>(file)), std::istreambuf_iterator<char>());
    std::vector<std::byte> out(data.size());
    std::memcpy(out.data(), data.data(), data.size());
    return out;
}
struct Root { char stage; std::string family, name; };
std::vector<Root> roots(const std::filesystem::path& manifest)
{
    std::ifstream file(manifest);
    std::vector<Root> result;
    std::string kind, stage, family, name, a, b;
    std::string line;
    while (std::getline(file, line)) {
        std::istringstream words(line);
        words >> kind;
        if (kind != "root") continue;
        words >> stage >> family >> name >> a >> b;
        result.push_back({stage[0], family, name});
    }
    return result;
}

D3DShaderApi* api{};
ShaderLoader* loader{};
std::filesystem::path deployment;

ShaderRequest request_for(const Root& root, std::vector<Define> defines)
{
    ShaderRequest request;
    request.target = root.stage == 'v' ? "vs_5_0" : root.stage == 'h' ? "hs_5_0" : root.stage == 'd' ? "ds_5_0" : "ps_5_0";
    request.entry = "main";
    request.flags = 8;   // D3D10_SHADER_PACK_MATRIX_ROW_MAJOR, what CResourceManager passes for every VS/PS
    request.defines = std::move(defines);
    request.source = file_bytes(deployment / "original" / "r3" / root.name);
    return request;
}
std::vector<Define> engine_defines(const Root& root)
{
    std::vector<Define> d{{"SMAP_size", "2048"}, {"USE_BRANCHING", "1"}, {"USE_VTF", "1"}, {"SM_5", "1"}};
    if (root.family == "core_model") d.push_back({"SKIN_NONE", "1"});
    // The tessellation pass roots (dx11/tess.hs, dx11/tess.ds) as uber_deffer.cpp r_TessPass compiles them for PN
    // materials; anomaly_motion_tess_chain_device draws the full chain.
    if (root.family == "tess_hull" || root.family == "tess_domain") d.push_back({"TESS_PN", "1"});
    return d;
}
struct Built {
    std::vector<std::byte> stock;
    Prepared prepared;
};
Built build(const Root& root, std::vector<Define> defines)
{
    auto request = request_for(root, std::move(defines));
    Built built;
    std::string error;
    require(api->compile(deployment / "original", request.source, request, built.stock, error), root.name + " stock: " + error);
    request.bytecode = built.stock;
    built.prepared = loader->prepare(request);
    return built;
}

void every_root(const std::vector<Root>& list)
{
    unsigned replaced = 0;
    for (const auto& root : list) {
        auto built = build(root, engine_defines(root));
        require(built.prepared.outcome == Outcome::Replaced,
            root.name + ": " + outcome_name(built.prepared.outcome) + " " + built.prepared.detail);
        ++replaced;
    }
    // Skinned and first-person-relevant permutations.
    for (const auto& root : list) {
        if (root.family != "core_model") continue;
        for (int skin = 0; skin <= 4; ++skin) {
            auto defines = engine_defines(root);
            defines.back() = {"SKIN_" + std::to_string(skin), "1"};
            auto built = build(root, defines);
            require(built.prepared.outcome == Outcome::Replaced && built.prepared.skin == skin,
                root.name + " SKIN_" + std::to_string(skin) + ": " + built.prepared.detail);
            ++replaced;
        }
    }
    // A single changed byte in the "engine" bytecode must refuse: the inputs read from the engine are unproven.
    auto request = request_for(list.front(), engine_defines(list.front()));
    std::vector<std::byte> stock;
    std::string error;
    api->compile(deployment / "original", request.source, request, stock, error);
    stock[stock.size() / 2] ^= std::byte{1};
    request.bytecode = stock;
    auto refused = loader->prepare(request);
    require(refused.outcome != Outcome::Replaced && refused.variant.empty(), "tampered engine bytecode refused");
    // Valid bytecode of another permutation (other root, same stage) for this root's source: the recompile differs.
    const auto other = std::find_if(list.begin() + 1, list.end(), [&](const Root& r) { return r.stage == list.front().stage; });
    require(other != list.end(), "second root of the stage");
    auto foreign = build(*other, engine_defines(*other));
    request.bytecode = foreign.stock;
    refused = loader->prepare(request);
    require(refused.outcome == Outcome::OriginalMismatch && refused.variant.empty(), "bytecode of another permutation refused");
    std::printf("roots compiled: %u variants through the production loader (%zu roots)\n", replaced, list.size());
}

struct Target {
    ComPtr<ID3D11Texture2D> texture;
    ComPtr<ID3D11RenderTargetView> rtv;
};
Target target(DXGI_FORMAT format)
{
    D3D11_TEXTURE2D_DESC desc{};
    desc.Width = desc.Height = size; desc.MipLevels = desc.ArraySize = 1; desc.Format = format; desc.SampleDesc.Count = 1;
    desc.BindFlags = D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE;
    Target t;
    hr(device->CreateTexture2D(&desc, nullptr, &t.texture), "target");
    hr(device->CreateRenderTargetView(t.texture.Get(), nullptr, &t.rtv), "rtv");
    return t;
}
template<class T> std::vector<T> read_back(ID3D11Texture2D* texture)
{
    D3D11_TEXTURE2D_DESC desc{};
    texture->GetDesc(&desc);
    desc.BindFlags = 0; desc.Usage = D3D11_USAGE_STAGING; desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ; desc.MiscFlags = 0;
    ComPtr<ID3D11Texture2D> staging;
    hr(device->CreateTexture2D(&desc, nullptr, &staging), "staging");
    context->CopyResource(staging.Get(), texture);
    D3D11_MAPPED_SUBRESOURCE mapped{};
    hr(context->Map(staging.Get(), 0, D3D11_MAP_READ, 0, &mapped), "map");
    std::vector<T> out(size * size);
    for (UINT y = 0; y < size; ++y)
        std::memcpy(out.data() + y * size, static_cast<const std::byte*>(mapped.pData) + y * mapped.RowPitch, size * sizeof(T));
    context->Unmap(staging.Get(), 0);
    return out;
}
struct Half4 { uint16_t c[4]; };
float half(uint16_t h)
{
    const uint32_t sign = (h & 0x8000u) << 16, exponent = (h >> 10) & 0x1f, mantissa = h & 0x3ff;
    uint32_t bits;
    if (!exponent) bits = mantissa ? 0 : sign;   // flush denormals, fine for these checks
    else if (exponent == 31) bits = sign | 0x7f800000u | (mantissa << 13);
    else bits = sign | ((exponent + 112) << 23) | (mantissa << 13);
    if (!exponent && mantissa) return (sign ? -1.0f : 1.0f) * std::ldexp(float(mantissa), -24);
    return std::bit_cast<float>(bits);
}

struct Scene {
    ComPtr<ID3D11VertexShader> stock_vs, motion_vs;
    ComPtr<ID3D11PixelShader> stock_ps, motion_ps;
    ComPtr<ID3D11InputLayout> layout;
    ComPtr<ID3D11Buffer> near_vb, behind_vb, full_vb;
    UINT stride{};
    std::vector<ComPtr<ID3D11Buffer>> constants;
    std::vector<ComPtr<ID3D11ShaderResourceView>> views;
    std::vector<ComPtr<ID3D11SamplerState>> samplers;
    Target position, albedo;
    ComPtr<ID3D11Texture2D> depth_texture;
    ComPtr<ID3D11DepthStencilView> depth;
    ComPtr<ID3D11DepthStencilState> less;
    ComPtr<ID3D11RasterizerState> raster;
};

ComPtr<ID3D11ShaderReflection> reflect(const std::vector<std::byte>& code)
{
    ComPtr<ID3D11ShaderReflection> r;
    hr(D3DReflect(code.data(), code.size(), IID_PPV_ARGS(&r)), "reflect");
    return r;
}
// Engine constant buffers: every matrix identity, everything else 0.1 (alpha ref 0.5), RSF b13 skipped.
void bind_engine_constants(Scene& s, ID3D11ShaderReflection* reflection, bool vertex)
{
    D3D11_SHADER_DESC desc{};
    reflection->GetDesc(&desc);
    for (UINT i = 0; i < desc.BoundResources; ++i) {
        D3D11_SHADER_INPUT_BIND_DESC binding{};
        reflection->GetResourceBindingDesc(i, &binding);
        if (binding.Type == D3D_SIT_CBUFFER) {
            if (std::string(binding.Name).starts_with("RSF")) continue;
            auto* cb = reflection->GetConstantBufferByName(binding.Name);
            D3D11_SHADER_BUFFER_DESC bd{};
            cb->GetDesc(&bd);
            std::vector<float> values((bd.Size + 15) / 16 * 4, 0.1f);
            for (UINT v = 0; v < bd.Variables; ++v) {
                auto* variable = cb->GetVariableByIndex(v);
                D3D11_SHADER_VARIABLE_DESC vd{};
                variable->GetDesc(&vd);
                D3D11_SHADER_TYPE_DESC td{};
                variable->GetType()->GetDesc(&td);
                float* out = values.data() + vd.StartOffset / 4;
                if (td.Class == D3D_SVC_MATRIX_COLUMNS || td.Class == D3D_SVC_MATRIX_ROWS)
                    for (UINT k = 0; k < vd.Size / 4; ++k) out[k] = (k % 5 == 0) ? 1.0f : 0.0f;
                if (std::string(vd.Name) == "def_aref" || std::string(vd.Name) == "alpha_ref") out[0] = 0.5f;
            }
            D3D11_BUFFER_DESC desc_b{};
            desc_b.ByteWidth = static_cast<UINT>(values.size() * 4);
            desc_b.BindFlags = D3D11_BIND_CONSTANT_BUFFER;
            D3D11_SUBRESOURCE_DATA init{values.data()};
            ComPtr<ID3D11Buffer> buffer;
            hr(device->CreateBuffer(&desc_b, &init, &buffer), "engine cb");
            ID3D11Buffer* p = buffer.Get();
            if (vertex) context->VSSetConstantBuffers(binding.BindPoint, 1, &p);
            else context->PSSetConstantBuffers(binding.BindPoint, 1, &p);
            s.constants.push_back(buffer);
        } else if (!vertex && binding.Type == D3D_SIT_TEXTURE) {
            D3D11_TEXTURE2D_DESC td{};
            td.Width = td.Height = 2; td.MipLevels = td.ArraySize = 1; td.Format = DXGI_FORMAT_R32G32B32A32_FLOAT;
            td.SampleDesc.Count = 1; td.BindFlags = D3D11_BIND_SHADER_RESOURCE;
            const float texels[16]{0.5f, 0.4f, 0.6f, 1, 0.5f, 0.4f, 0.6f, 1, 0.5f, 0.4f, 0.6f, 1, 0.5f, 0.4f, 0.6f, 1};
            D3D11_SUBRESOURCE_DATA init{texels, 32};
            ComPtr<ID3D11Texture2D> texture;
            hr(device->CreateTexture2D(&td, &init, &texture), "texture");
            ComPtr<ID3D11ShaderResourceView> srv;
            hr(device->CreateShaderResourceView(texture.Get(), nullptr, &srv), "srv");
            ID3D11ShaderResourceView* p = srv.Get();
            context->PSSetShaderResources(binding.BindPoint, 1, &p);
            s.views.push_back(srv);
        } else if (!vertex && binding.Type == D3D_SIT_SAMPLER) {
            D3D11_SAMPLER_DESC sd{};
            sd.Filter = D3D11_FILTER_MIN_MAG_MIP_LINEAR;
            sd.AddressU = sd.AddressV = sd.AddressW = D3D11_TEXTURE_ADDRESS_CLAMP;
            sd.MaxLOD = D3D11_FLOAT32_MAX;
            ComPtr<ID3D11SamplerState> sampler;
            hr(device->CreateSamplerState(&sd, &sampler), "sampler");
            ID3D11SamplerState* p = sampler.Get();
            context->PSSetSamplers(binding.BindPoint, 1, &p);
            s.samplers.push_back(sampler);
        }
    }
}
ComPtr<ID3D11Buffer> triangle(ID3D11ShaderReflection* reflection, float x0, float x1, float z)
{
    D3D11_SHADER_DESC desc{};
    reflection->GetDesc(&desc);
    // Full-screen-ish triangle restricted to x in [x0, x1] by a quad of two triangles.
    const float corners[6][2]{{x0, -1}, {x0, 1}, {x1, -1}, {x1, -1}, {x0, 1}, {x1, 1}};
    std::vector<float> data(6 * desc.InputParameters * 4, 0.5f);
    for (UINT v = 0; v < 6; ++v)
        for (UINT i = 0; i < desc.InputParameters; ++i) {
            D3D11_SIGNATURE_PARAMETER_DESC in{};
            reflection->GetInputParameterDesc(i, &in);
            float* out = data.data() + (v * desc.InputParameters + i) * 4;
            if (std::string(in.SemanticName) == "POSITION") { out[0] = corners[v][0]; out[1] = corners[v][1]; out[2] = z; out[3] = 1; }
            else if (std::string(in.SemanticName) == "TEXCOORD") { out[0] = 0.25f; out[1] = 0.75f; out[2] = 0; out[3] = 0; }
        }
    D3D11_BUFFER_DESC bd{};
    bd.ByteWidth = static_cast<UINT>(data.size() * 4);
    bd.BindFlags = D3D11_BIND_VERTEX_BUFFER;
    D3D11_SUBRESOURCE_DATA init{data.data()};
    ComPtr<ID3D11Buffer> buffer;
    hr(device->CreateBuffer(&bd, &init, &buffer), "vertex buffer");
    return buffer;
}

void draw_scene()
{
    const Root vs_root{'v', "core_static", "deffer_base_flat.vs"}, ps_root{'p', "core_pixel", "deffer_base_flat.ps"};
    auto vs = build(vs_root, engine_defines(vs_root));
    auto ps = build(ps_root, engine_defines(ps_root));
    require(vs.prepared.outcome == Outcome::Replaced && ps.prepared.outcome == Outcome::Replaced, "flat pair variants");
    require(linkable(vs.prepared.reflection, ps.prepared.reflection), "variant pair links");
    Reflection stock_vs_reflection;
    api->reflect(vs.stock, stock_vs_reflection);
    require(!linkable(stock_vs_reflection, ps.prepared.reflection), "stock VS cannot feed the motion PS");

    Scene s;
    hr(device->CreateVertexShader(vs.stock.data(), vs.stock.size(), nullptr, &s.stock_vs), "stock VS");
    hr(device->CreateVertexShader(vs.prepared.variant.data(), vs.prepared.variant.size(), nullptr, &s.motion_vs), "motion VS");
    hr(device->CreatePixelShader(ps.stock.data(), ps.stock.size(), nullptr, &s.stock_ps), "stock PS");
    hr(device->CreatePixelShader(ps.prepared.variant.data(), ps.prepared.variant.size(), nullptr, &s.motion_ps), "motion PS");
    auto vs_reflection = reflect(vs.stock);
    D3D11_SHADER_DESC desc{};
    vs_reflection->GetDesc(&desc);
    std::vector<D3D11_INPUT_ELEMENT_DESC> elements;
    std::vector<std::string> names;
    names.reserve(desc.InputParameters);
    for (UINT i = 0; i < desc.InputParameters; ++i) {
        D3D11_SIGNATURE_PARAMETER_DESC in{};
        vs_reflection->GetInputParameterDesc(i, &in);
        names.emplace_back(in.SemanticName);
        const DXGI_FORMAT format = in.ComponentType == D3D_REGISTER_COMPONENT_FLOAT32 ? DXGI_FORMAT_R32G32B32A32_FLOAT :
            in.ComponentType == D3D_REGISTER_COMPONENT_UINT32 ? DXGI_FORMAT_R32G32B32A32_UINT : DXGI_FORMAT_R32G32B32A32_SINT;
        elements.push_back({names.back().c_str(), in.SemanticIndex, format, 0, i * 16, D3D11_INPUT_PER_VERTEX_DATA, 0});
    }
    // The engine builds its input layout against the STOCK signature; the variant must accept it unchanged.
    hr(device->CreateInputLayout(elements.data(), static_cast<UINT>(elements.size()), vs.stock.data(), vs.stock.size(), &s.layout), "layout");
    s.stride = desc.InputParameters * 16;
    s.full_vb = triangle(vs_reflection.Get(), -1, 1, 0.5f);
    s.near_vb = triangle(vs_reflection.Get(), -1, 0, 0.25f);
    s.behind_vb = triangle(vs_reflection.Get(), 0, 1, 0.75f);
    s.position = target(DXGI_FORMAT_R16G16B16A16_FLOAT);
    s.albedo = target(DXGI_FORMAT_R8G8B8A8_UNORM);
    D3D11_TEXTURE2D_DESC dd{};
    dd.Width = dd.Height = size; dd.MipLevels = dd.ArraySize = 1; dd.Format = DXGI_FORMAT_D24_UNORM_S8_UINT;
    dd.SampleDesc.Count = 1; dd.BindFlags = D3D11_BIND_DEPTH_STENCIL;
    hr(device->CreateTexture2D(&dd, nullptr, &s.depth_texture), "depth");
    hr(device->CreateDepthStencilView(s.depth_texture.Get(), nullptr, &s.depth), "dsv");
    D3D11_DEPTH_STENCIL_DESC ds{};
    ds.DepthEnable = TRUE; ds.DepthWriteMask = D3D11_DEPTH_WRITE_MASK_ALL; ds.DepthFunc = D3D11_COMPARISON_LESS_EQUAL;
    hr(device->CreateDepthStencilState(&ds, &s.less), "depth state");
    D3D11_RASTERIZER_DESC rd{};
    rd.FillMode = D3D11_FILL_SOLID; rd.CullMode = D3D11_CULL_NONE; rd.DepthClipEnable = TRUE;
    hr(device->CreateRasterizerState(&rd, &s.raster), "raster");

    MotionDevice motion;
    require(motion.create(device.Get(), nullptr, nullptr), "MotionDevice create");
    D3D11_TEXTURE2D_DESC position_desc{};
    s.position.texture->GetDesc(&position_desc);
    require(motion.ensure_target(position_desc) == TargetResult::Created && motion.width() == size, "motion target follows RT0");
    require(motion.ensure_target(position_desc) == TargetResult::Ready, "reused");
    auto ms = position_desc;
    ms.SampleDesc.Count = 4;
    require(motion.ensure_target(ms) == TargetResult::Multisampled, "MSAA refused");

    const auto setup = [&](bool variant, ID3D11Buffer* vb, bool three) {
        context->ClearState();
        context->IASetInputLayout(s.layout.Get());
        context->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        UINT offset = 0;
        context->IASetVertexBuffers(0, 1, &vb, &s.stride, &offset);
        context->VSSetShader(variant ? s.motion_vs.Get() : s.stock_vs.Get(), nullptr, 0);
        context->PSSetShader(variant ? s.motion_ps.Get() : s.stock_ps.Get(), nullptr, 0);
        bind_engine_constants(s, vs_reflection.Get(), true);
        bind_engine_constants(s, reflect(ps.stock).Get(), false);
        ID3D11RenderTargetView* views[3]{s.position.rtv.Get(), s.albedo.rtv.Get(), three ? motion.rtv() : nullptr};
        context->OMSetRenderTargets(three ? 3 : 2, views, s.depth.Get());
        context->OMSetDepthStencilState(s.less.Get(), 0);
        context->RSSetState(s.raster.Get());
        const D3D11_VIEWPORT viewport{0, 0, float(size), float(size), 0, 1};
        context->RSSetViewports(1, &viewport);
    };
    const auto clear = [&] {
        const float sentinel[4]{-999, -999, -999, -999}, zero[4]{};
        context->ClearRenderTargetView(s.position.rtv.Get(), sentinel);
        context->ClearRenderTargetView(s.albedo.rtv.Get(), zero);
        context->ClearDepthStencilView(s.depth.Get(), D3D11_CLEAR_DEPTH | D3D11_CLEAR_STENCIL, 1, 0);
        motion.clear(context.Get());
    };
    const auto upload = [&](bool valid) {
        temporal::Mat4 previous = temporal::Mat4::identity();
        previous.values[3] = 0.1;    // previous clip x = x + 0.1
        previous.values[7] = -0.2;   // previous clip y = y - 0.2
        CoreVertexBytes vbytes;
        require(pack_core_vertex(previous, {}, {valid, false, false}, vbytes), "pack VS b13");
        PixelBytes pbytes;
        require(pack_pixel({0, 0, float(size), float(size), 0, 0, 0, 0}, pbytes), "pack PS b13");
        require(motion.set_core_vertex(context.Get(), vbytes) && motion.set_pixel(context.Get(), pbytes), "b13 upload");
    };

    // Stock reference.
    clear();
    setup(false, s.full_vb.Get(), false);
    context->Draw(6, 0);
    const auto stock_position = read_back<Half4>(s.position.texture.Get());
    const auto stock_albedo = read_back<uint32_t>(s.albedo.texture.Get());
    debug_errors();
    // Variant with history.
    clear();
    setup(true, s.full_vb.Get(), true);
    upload(true);
    context->Draw(6, 0);
    const auto position = read_back<Half4>(s.position.texture.Get());
    const auto albedo = read_back<uint32_t>(s.albedo.texture.Get());
    auto vectors = read_back<Half4>(motion.texture());
    debug_errors();
    unsigned covered = 0;
    for (UINT i = 0; i < size * size; ++i) {
        require(std::memcmp(&position[i], &stock_position[i], sizeof(Half4)) == 0 && albedo[i] == stock_albedo[i],
            "native MRT0/1 bit-identical at " + std::to_string(i));
        if (half(stock_position[i].c[0]) == -999) continue;
        ++covered;
        require(std::abs(half(vectors[i].c[0]) - (-0.05f)) < 2e-4f && std::abs(half(vectors[i].c[1]) - (-0.1f)) < 2e-4f &&
            half(vectors[i].c[2]) == 1 && half(vectors[i].c[3]) == 0,
            "motion (-0.05,-0.1,1,0) at " + std::to_string(i) + ": " + std::to_string(half(vectors[i].c[0])) + "," +
            std::to_string(half(vectors[i].c[1])) + "," + std::to_string(half(vectors[i].c[2])));
    }
    require(covered == size * size, "full coverage");

    // Stock occluder in front of the left half: the stock PS leaves stale vectors; the replay invalidates them.
    setup(false, s.near_vb.Get(), true);
    context->Draw(6, 0);
    auto stale = read_back<Half4>(motion.texture());
    require(half(stale[size / 2 * size + 4].c[2]) == 1, "fixture: a stock draw leaves the old vector (the reason for the replay)");
    const auto occluded_position = read_back<Half4>(s.position.texture.Get());
    require(motion.invalidate(context.Get(), [&](ID3D11DeviceContext* c) { c->Draw(6, 0); }) == ReplayResult::Done, "replay");
    ID3D11RenderTargetView* bound[3]{};
    ID3D11DepthStencilView* bound_depth{};
    context->OMGetRenderTargets(3, bound, &bound_depth);
    ComPtr<ID3D11PixelShader> bound_ps;
    context->PSGetShader(&bound_ps, nullptr, nullptr);
    require(bound[0] == s.position.rtv.Get() && bound[1] == s.albedo.rtv.Get() && bound[2] == motion.rtv() &&
        bound_depth == s.depth.Get() && bound_ps.Get() == s.stock_ps.Get(), "OM state and PS restored");
    for (auto* p : bound) if (p) p->Release();
    if (bound_depth) bound_depth->Release();
    // Behind the surface: the occluder fails the depth test, so the replay's EQUAL test fails too.
    setup(false, s.behind_vb.Get(), true);
    context->Draw(6, 0);
    require(motion.invalidate(context.Get(), [&](ID3D11DeviceContext* c) { c->Draw(6, 0); }) == ReplayResult::Done, "replay behind");
    vectors = read_back<Half4>(motion.texture());
    const auto after_position = read_back<Half4>(s.position.texture.Get());
    debug_errors();
    unsigned invalidated = 0, kept = 0;
    for (UINT y = 0; y < size; ++y)
        for (UINT x = 0; x < size; ++x) {
            const auto& v = vectors[y * size + x];
            require(std::memcmp(&after_position[y * size + x], &occluded_position[y * size + x], sizeof(Half4)) == 0,
                "the replay writes no native target");
            if (x < size / 2) {
                require(v.c[0] == 0 && v.c[1] == 0 && v.c[2] == 0 && v.c[3] == 0, "occluded sample invalidated");
                ++invalidated;
            } else {
                require(half(v.c[2]) == 1 && std::abs(half(v.c[0]) + 0.05f) < 2e-4f, "visible sample kept");
                ++kept;
            }
        }
    // Missing history: all-zero motion, native output unchanged.
    clear();
    setup(true, s.full_vb.Get(), true);
    upload(false);
    context->Draw(6, 0);
    vectors = read_back<Half4>(motion.texture());
    const auto invalid_position = read_back<Half4>(s.position.texture.Get());
    for (UINT i = 0; i < size * size; ++i) {
        require(vectors[i].c[0] == 0 && vectors[i].c[1] == 0 && vectors[i].c[2] == 0 && vectors[i].c[3] == 0, "missing history is zero");
        require(std::memcmp(&invalid_position[i], &stock_position[i], sizeof(Half4)) == 0, "native unchanged without history");
    }
    debug_errors();
    std::printf("draw: %u covered, %u invalidated by replay, %u kept, uploads=%llu skipped=%llu\n", covered, invalidated, kept,
        static_cast<unsigned long long>(motion.uploads()), static_cast<unsigned long long>(motion.upload_skips()));
}

// ---- foliage (trees and grass) ------------------------------------------------------------------------------------
// Every deployed foliage root must expose what the draw-time reader needs under the engine's compile flags.
void foliage_layouts(const std::vector<Root>& list)
{
    unsigned trees = 0, details = 0;
    for (const auto& root : list) {
        if (root.family != "foliage_tree" && root.family != "foliage_detail") continue;
        const auto built = build(root, engine_defines(root));
        require(built.prepared.outcome == Outcome::Replaced, root.name + ": " + built.prepared.detail);
        const bool tree = root.family == "foliage_tree";
        const auto layout = make_foliage_layout(tree ? Family::FoliageTree : Family::FoliageDetail, built.prepared.stock);
        require(layout.usable, root.name + ": the engine's staged constants cannot be located from the stock reflection");
        if (tree) {
            require(layout.xform.size >= 48 && layout.xform.present, root.name + " m_xform is three registers");
            // Which of wave, wind and consts survive depends on the permutation (the compiler drops unused uniforms); the
            // reader treats a missing one as zero, and consts is always needed for the texture coordinates.
            require(layout.consts.present, root.name + " consts");
            ++trees;
        } else {
            require(layout.array.size == 3904 && layout.consts.present, root.name + " array[61 * 4]");
            const bool still = root.name == "deffer_detail_s_flat.vs";
            require(layout.wave.present == !still && layout.wind.present == !still, root.name + " wave set");
            ++details;
        }
        // The variant keeps the stock buffer indices, so the engine's own table index addresses the same buffer.
        for (const auto& buffer : built.prepared.stock.buffers) {
            const auto found = std::find_if(built.prepared.reflection.buffers.begin(), built.prepared.reflection.buffers.end(),
                [&](const Buffer& b) { return b.name == buffer.name; });
            require(found != built.prepared.reflection.buffers.end() && found->size == buffer.size, root.name + " buffer kept");
        }
    }
    require(trees >= 10 && details == 2, "all deployed foliage roots checked");
    std::printf("foliage roots: %u tree, %u detail layouts usable\n", trees, details);
}

float cyclic(float x)
{
    const float f = 2.8284271f * (x - std::floor(x)) - 1.4142136f;
    return f * f - 1.0f;
}

struct FoliageRig {
    ComPtr<ID3D11VertexShader> stock_vs, motion_vs;
    ComPtr<ID3D11PixelShader> stock_ps, motion_ps;
    ComPtr<ID3D11InputLayout> layout;
    ComPtr<ID3D11ShaderReflection> vs_reflection, ps_reflection;
    Built vs, ps;
    UINT stride{};
    Target position, albedo;
    ComPtr<ID3D11Texture2D> depth_texture;
    ComPtr<ID3D11DepthStencilView> depth;
    ComPtr<ID3D11DepthStencilState> less;
    ComPtr<ID3D11RasterizerState> raster;
    Scene engine;   // keeps the engine constant buffers alive
};
FoliageRig make_rig(const Root& vs_root, const Root& ps_root)
{
    FoliageRig r;
    r.vs = build(vs_root, engine_defines(vs_root));
    r.ps = build(ps_root, engine_defines(ps_root));
    require(r.vs.prepared.outcome == Outcome::Replaced && r.ps.prepared.outcome == Outcome::Replaced, vs_root.name + " pair variants");
    require(linkable(r.vs.prepared.reflection, r.ps.prepared.reflection), vs_root.name + " variant links to the core pixel variant");
    hr(device->CreateVertexShader(r.vs.stock.data(), r.vs.stock.size(), nullptr, &r.stock_vs), "stock VS");
    hr(device->CreateVertexShader(r.vs.prepared.variant.data(), r.vs.prepared.variant.size(), nullptr, &r.motion_vs), "motion VS");
    hr(device->CreatePixelShader(r.ps.stock.data(), r.ps.stock.size(), nullptr, &r.stock_ps), "stock PS");
    hr(device->CreatePixelShader(r.ps.prepared.variant.data(), r.ps.prepared.variant.size(), nullptr, &r.motion_ps), "motion PS");
    r.vs_reflection = reflect(r.vs.stock);
    r.ps_reflection = reflect(r.ps.stock);
    D3D11_SHADER_DESC desc{};
    r.vs_reflection->GetDesc(&desc);
    std::vector<D3D11_INPUT_ELEMENT_DESC> elements;
    std::vector<std::string> names;
    names.reserve(desc.InputParameters);
    for (UINT i = 0; i < desc.InputParameters; ++i) {
        D3D11_SIGNATURE_PARAMETER_DESC in{};
        r.vs_reflection->GetInputParameterDesc(i, &in);
        names.emplace_back(in.SemanticName);
        const DXGI_FORMAT format = in.ComponentType == D3D_REGISTER_COMPONENT_FLOAT32 ? DXGI_FORMAT_R32G32B32A32_FLOAT :
            in.ComponentType == D3D_REGISTER_COMPONENT_UINT32 ? DXGI_FORMAT_R32G32B32A32_UINT : DXGI_FORMAT_R32G32B32A32_SINT;
        elements.push_back({names.back().c_str(), in.SemanticIndex, format, 0, i * 16, D3D11_INPUT_PER_VERTEX_DATA, 0});
    }
    hr(device->CreateInputLayout(elements.data(), static_cast<UINT>(elements.size()), r.vs.stock.data(), r.vs.stock.size(), &r.layout), "layout");
    r.stride = desc.InputParameters * 16;
    r.position = target(DXGI_FORMAT_R16G16B16A16_FLOAT);
    r.albedo = target(DXGI_FORMAT_R8G8B8A8_UNORM);
    D3D11_TEXTURE2D_DESC dd{};
    dd.Width = dd.Height = size; dd.MipLevels = dd.ArraySize = 1; dd.Format = DXGI_FORMAT_D24_UNORM_S8_UINT;
    dd.SampleDesc.Count = 1; dd.BindFlags = D3D11_BIND_DEPTH_STENCIL;
    hr(device->CreateTexture2D(&dd, nullptr, &r.depth_texture), "depth");
    hr(device->CreateDepthStencilView(r.depth_texture.Get(), nullptr, &r.depth), "dsv");
    D3D11_DEPTH_STENCIL_DESC ds{};
    ds.DepthEnable = TRUE; ds.DepthWriteMask = D3D11_DEPTH_WRITE_MASK_ALL; ds.DepthFunc = D3D11_COMPARISON_LESS_EQUAL;
    hr(device->CreateDepthStencilState(&ds, &r.less), "depth state");
    D3D11_RASTERIZER_DESC rd{};
    rd.FillMode = D3D11_FILL_SOLID; rd.CullMode = D3D11_CULL_NONE; rd.DepthClipEnable = TRUE;
    hr(device->CreateRasterizerState(&rd, &r.raster), "raster");
    return r;
}
// A quad of two triangles in the vertex layout of the foliage VS: POSITION float4, TEXCOORD0 int4 (0, 0, frac, row offset).
ComPtr<ID3D11Buffer> foliage_quad(const FoliageRig& rig, float x0, float x1, float y0, float y1, float z, int frac, int row)
{
    D3D11_SHADER_DESC desc{};
    rig.vs_reflection->GetDesc(&desc);
    const float corners[6][2]{{x0, y0}, {x0, y1}, {x1, y0}, {x1, y0}, {x0, y1}, {x1, y1}};
    std::vector<std::byte> data(6 * desc.InputParameters * 16);
    for (UINT v = 0; v < 6; ++v)
        for (UINT i = 0; i < desc.InputParameters; ++i) {
            D3D11_SIGNATURE_PARAMETER_DESC in{};
            rig.vs_reflection->GetInputParameterDesc(i, &in);
            float f[4]{0.5f, 0.5f, 0.5f, 0.5f};
            std::int32_t n[4]{};
            if (std::string(in.SemanticName) == "POSITION") { f[0] = corners[v][0]; f[1] = corners[v][1]; f[2] = z; f[3] = 1; }
            if (std::string(in.SemanticName) == "TEXCOORD") { n[2] = frac; n[3] = row; }
            auto* out = data.data() + (v * desc.InputParameters + i) * 16;
            if (in.ComponentType == D3D_REGISTER_COMPONENT_FLOAT32) std::memcpy(out, f, 16);
            else std::memcpy(out, n, 16);
        }
    D3D11_BUFFER_DESC bd{};
    bd.ByteWidth = static_cast<UINT>(data.size());
    bd.BindFlags = D3D11_BIND_VERTEX_BUFFER;
    D3D11_SUBRESOURCE_DATA init{data.data()};
    ComPtr<ID3D11Buffer> buffer;
    hr(device->CreateBuffer(&bd, &init, &buffer), "foliage vertex buffer");
    return buffer;
}
using Overrides = std::map<std::string, std::vector<float>>;
void apply_overrides(ID3D11ShaderReflection* reflection, const Overrides& overrides, std::vector<ComPtr<ID3D11Buffer>>& keep)
{
    D3D11_SHADER_DESC desc{};
    reflection->GetDesc(&desc);
    for (UINT i = 0; i < desc.BoundResources; ++i) {
        D3D11_SHADER_INPUT_BIND_DESC binding{};
        reflection->GetResourceBindingDesc(i, &binding);
        if (binding.Type != D3D_SIT_CBUFFER || std::string(binding.Name).starts_with("RSF")) continue;
        auto* cb = reflection->GetConstantBufferByName(binding.Name);
        D3D11_SHADER_BUFFER_DESC bd{};
        cb->GetDesc(&bd);
        // Read back what bind_engine_constants uploaded, patch the named variables, upload again.
        ID3D11Buffer* current{};
        context->VSGetConstantBuffers(binding.BindPoint, 1, &current);
        require(current != nullptr, "engine constant buffer bound");
        D3D11_BUFFER_DESC cd{};
        current->GetDesc(&cd);
        std::vector<float> values(cd.ByteWidth / 4);
        cd.Usage = D3D11_USAGE_STAGING; cd.BindFlags = 0; cd.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
        ComPtr<ID3D11Buffer> staging;
        hr(device->CreateBuffer(&cd, nullptr, &staging), "staging");
        context->CopyResource(staging.Get(), current);
        D3D11_MAPPED_SUBRESOURCE mapped{};
        hr(context->Map(staging.Get(), 0, D3D11_MAP_READ, 0, &mapped), "map cb");
        std::memcpy(values.data(), mapped.pData, cd.ByteWidth);
        context->Unmap(staging.Get(), 0);
        current->Release();
        for (UINT v = 0; v < bd.Variables; ++v) {
            D3D11_SHADER_VARIABLE_DESC vd{};
            cb->GetVariableByIndex(v)->GetDesc(&vd);
            const auto found = overrides.find(vd.Name);
            if (found == overrides.end()) continue;
            require(found->second.size() * 4 <= vd.Size, std::string("override fits ") + vd.Name);
            std::memcpy(values.data() + vd.StartOffset / 4, found->second.data(), found->second.size() * 4);
        }
        D3D11_BUFFER_DESC nb{};
        nb.ByteWidth = static_cast<UINT>(values.size() * 4);
        nb.BindFlags = D3D11_BIND_CONSTANT_BUFFER;
        D3D11_SUBRESOURCE_DATA init{values.data()};
        ComPtr<ID3D11Buffer> buffer;
        hr(device->CreateBuffer(&nb, &init, &buffer), "patched cb");
        ID3D11Buffer* p = buffer.Get();
        context->VSSetConstantBuffers(binding.BindPoint, 1, &p);
        keep.push_back(buffer);
    }
}
struct FoliageDraw {
    std::vector<Half4> position, vectors;
    std::vector<uint32_t> albedo;
};
// Draws `vertex_buffers` (one quad each, 6 vertices) with the stock pair or the variant pair and `overrides` as the
// engine's VS constants. The variant draw uploads `vbytes` as VS b13 and a unit-viewport PS b13.
FoliageDraw draw_foliage_pair(FoliageRig& rig, MotionDevice& motion, bool variant, const Overrides& overrides,
    const std::vector<ComPtr<ID3D11Buffer>>& quads, const FoliageVertexBytes* vbytes)
{
    context->ClearState();
    const float sentinel[4]{-999, -999, -999, -999}, zero[4]{};
    context->ClearRenderTargetView(rig.position.rtv.Get(), sentinel);
    context->ClearRenderTargetView(rig.albedo.rtv.Get(), zero);
    context->ClearDepthStencilView(rig.depth.Get(), D3D11_CLEAR_DEPTH | D3D11_CLEAR_STENCIL, 1, 0);
    motion.clear(context.Get());
    context->IASetInputLayout(rig.layout.Get());
    context->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
    context->VSSetShader(variant ? rig.motion_vs.Get() : rig.stock_vs.Get(), nullptr, 0);
    context->PSSetShader(variant ? rig.motion_ps.Get() : rig.stock_ps.Get(), nullptr, 0);
    bind_engine_constants(rig.engine, rig.vs_reflection.Get(), true);
    std::vector<ComPtr<ID3D11Buffer>> keep;
    apply_overrides(rig.vs_reflection.Get(), overrides, keep);
    bind_engine_constants(rig.engine, rig.ps_reflection.Get(), false);
    ID3D11RenderTargetView* views[3]{rig.position.rtv.Get(), rig.albedo.rtv.Get(), variant ? motion.rtv() : nullptr};
    context->OMSetRenderTargets(variant ? 3 : 2, views, rig.depth.Get());
    context->OMSetDepthStencilState(rig.less.Get(), 0);
    context->RSSetState(rig.raster.Get());
    const D3D11_VIEWPORT viewport{0, 0, float(size), float(size), 0, 1};
    context->RSSetViewports(1, &viewport);
    if (variant) {
        PixelBytes pbytes;
        require(pack_pixel({0, 0, float(size), float(size), 0, 0, 0, 0}, pbytes), "pack PS b13");
        require(vbytes && motion.set_foliage_vertex(context.Get(), *vbytes) && motion.set_pixel(context.Get(), pbytes), "foliage b13 upload");
    }
    for (const auto& quad : quads) {
        ID3D11Buffer* vb = quad.Get();
        UINT stride = rig.stride, offset = 0;
        context->IASetVertexBuffers(0, 1, &vb, &stride, &offset);
        context->Draw(6, 0);
    }
    FoliageDraw out;
    out.position = read_back<Half4>(rig.position.texture.Get());
    out.albedo = read_back<uint32_t>(rig.albedo.texture.Get());
    if (variant) out.vectors = read_back<Half4>(motion.texture());
    debug_errors();
    return out;
}
float ndc_x(UINT i) { return (float(i) + 0.5f) / size * 2 - 1; }
float ndc_y(UINT j) { return 1 - (float(j) + 0.5f) / size * 2; }

void draw_tree()
{
    const Root vs_root{'v', "foliage_tree", "deffer_tree_flat.vs"}, ps_root{'p', "core_pixel", "deffer_base_flat.ps"};
    auto rig = make_rig(vs_root, ps_root);
    const auto layout = make_foliage_layout(Family::FoliageTree, rig.vs.prepared.stock);
    require(layout.usable, "tree layout");
    MotionDevice motion;
    require(motion.create(device.Get(), nullptr, nullptr), "MotionDevice create");
    D3D11_TEXTURE2D_DESC position_desc{};
    rig.position.texture->GetDesc(&position_desc);
    require(motion.ensure_target(position_desc) == TargetResult::Created, "motion target");
    const std::vector<ComPtr<ID3D11Buffer>> quads{foliage_quad(rig, -1, 1, -1, 1, 0.5f, 1024, 0)};
    // Engine constants as FTreeVisual::Render stages them: scale 1/2048, wave (0, 0, 0, time), wind (amplitude, 0, 0, 0).
    // m_xform and the matrices stay identity, so the quad's y is the height above the trunk base (m_xform._24 = 0).
    const auto constants = [](float time, float amplitude) {
        FoliageConstants c;
        const float consts[4] = {1.0f / 2048, 1.0f / 2048, 0, 0}, wave[4] = {0, 0, 0, time}, dir[4] = {amplitude, 0, 0, 0};
        std::memcpy(c.consts, consts, sizeof(consts));
        std::memcpy(c.wave, wave, sizeof(wave));
        std::memcpy(c.wind, dir, sizeof(dir));
        return c;
    };
    const auto overrides = [](const FoliageConstants& c) {
        return Overrides{{"consts", {c.consts, c.consts + 4}}, {"wave", {c.wave, c.wave + 4}}, {"wind", {c.wind, c.wind + 4}}};
    };
    const float identity_xform[12] = {1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0};
    const auto now = constants(0.30f, 0.6f), before = constants(0.80f, 0.4f);

    const auto stock = draw_foliage_pair(rig, motion, false, overrides(now), quads, nullptr);
    FoliageVertexBytes bytes;
    // 1. Wind in the previous frame differs: the crown moves, the base (y = 0) does not, native targets are untouched.
    require(pack_foliage_tree(temporal::Mat4::identity(), identity_xform, before, bytes), "pack tree history");
    auto drawn = draw_foliage_pair(rig, motion, true, overrides(now), quads, &bytes);
    const float frac = 1024.0f * (1.0f / 2048);
    unsigned moving = 0, still = 0;
    float largest = 0;
    for (UINT j = 0; j < size; ++j)
        for (UINT i = 0; i < size; ++i) {
            const auto k = j * size + i;
            require(std::memcmp(&drawn.position[k], &stock.position[k], sizeof(Half4)) == 0 && drawn.albedo[k] == stock.albedo[k],
                "native MRT0/1 bit-identical for the tree");
            if (half(stock.position[k].c[0]) == -999) {   // the sway shears the quad, so a corner can stay uncovered
                require(drawn.vectors[k].c[2] == 0, "uncovered tree pixels stay invalid");
                continue;
            }
            const float y = ndc_y(j);
            const float expected = ((now.wind[0] * y * cyclic(now.wave[3]) * frac) - (before.wind[0] * y * cyclic(before.wave[3]) * frac)) / 2;
            const float mx = half(drawn.vectors[k].c[0]), my = half(drawn.vectors[k].c[1]);
            require(std::abs(mx - expected) < 4e-4f && std::abs(my) < 2e-4f && half(drawn.vectors[k].c[2]) == 1 && half(drawn.vectors[k].c[3]) == 0,
                "tree wind motion at " + std::to_string(i) + "," + std::to_string(j) + ": " + std::to_string(mx) + " vs " + std::to_string(expected) +
                " valid " + std::to_string(half(drawn.vectors[k].c[2])) + " y " + std::to_string(my));
            largest = std::max(largest, std::abs(mx));
            (std::abs(expected) > 2e-3f ? moving : still) += 1;
        }
    require(largest > 5e-3f && moving > size * size / 4, "the wind produced real motion across the crown");
    require(still >= 1, "rows near the trunk base barely move");
    // 2. Same wind and a static camera: valid and exactly zero. The trunk is not wind.
    require(pack_foliage_tree(temporal::Mat4::identity(), identity_xform, now, bytes), "pack still");
    drawn = draw_foliage_pair(rig, motion, true, overrides(now), quads, &bytes);
    unsigned covered = 0;
    for (UINT k = 0; k < size * size; ++k) {
        if (half(stock.position[k].c[0]) == -999) continue;
        ++covered;
        require(std::abs(half(drawn.vectors[k].c[0])) < 1e-4f && std::abs(half(drawn.vectors[k].c[1])) < 1e-4f &&
            half(drawn.vectors[k].c[2]) == 1, "static tree with unchanged wind is valid zero motion");
    }
    require(covered > size * size / 2, "the tree covers most of the target");
    // 3. A camera pan with the same wind is the camera vector (previous clip x = x + 0.1: -0.05), crown and base alike.
    auto panned = temporal::Mat4::identity();
    panned.values[3] = 0.1;
    require(pack_foliage_tree(panned, identity_xform, now, bytes), "pack pan");
    drawn = draw_foliage_pair(rig, motion, true, overrides(now), quads, &bytes);
    for (UINT k = 0; k < size * size; ++k) {
        if (half(stock.position[k].c[0]) == -999) continue;
        require(std::abs(half(drawn.vectors[k].c[0]) + 0.05f) < 2e-4f && std::abs(half(drawn.vectors[k].c[1])) < 2e-4f &&
            half(drawn.vectors[k].c[2]) == 1, "camera pan on a tree");
    }
    // 4. No history: native output unchanged, motion all zero.
    pack_foliage_invalid(bytes);
    drawn = draw_foliage_pair(rig, motion, true, overrides(now), quads, &bytes);
    for (UINT k = 0; k < size * size; ++k) {
        require(drawn.vectors[k].c[0] == 0 && drawn.vectors[k].c[1] == 0 && drawn.vectors[k].c[2] == 0 && drawn.vectors[k].c[3] == 0,
            "tree without history is zero");
        require(std::memcmp(&drawn.position[k], &stock.position[k], sizeof(Half4)) == 0, "native unchanged without history");
    }
    std::printf("tree: wind motion up to %.4f UV over %u crown pixels, static/pan/no-history cases pass\n", largest, moving);
}

void draw_detail()
{
    const Root vs_root{'v', "foliage_detail", "deffer_detail_w_flat.vs"}, ps_root{'p', "core_pixel", "deffer_base_flat.ps"};
    auto rig = make_rig(vs_root, ps_root);
    const auto layout = make_foliage_layout(Family::FoliageDetail, rig.vs.prepared.stock);
    require(layout.usable, "detail layout");
    MotionDevice motion;
    require(motion.create(device.Get(), nullptr, nullptr), "MotionDevice create");
    D3D11_TEXTURE2D_DESC position_desc{};
    rig.position.texture->GetDesc(&position_desc);
    require(motion.ensure_target(position_desc) == TargetResult::Created, "motion target");

    struct Plant { float tx, scale, sun, hemi; };
    const Plant a{-0.5f, 1.0f, 0.3f, 0.7f}, b{0.5f, 1.0f, 0.2f, 0.6f};
    const auto empty_batch_rows = [] {
        DetailRows rows;
        poison_detail_rows(rows);
        return rows;
    };
    const auto group = [](const Plant& p, float scale) {
        return std::array<float, 16>{scale, 0, 0, p.tx, 0, scale, 0, 0, 0, 0, scale, 0.5f, p.sun, p.sun, p.sun, p.hemi};
    };
    const auto put = [](DetailRows& rows, std::size_t slot, const std::array<float, 16>& g) {
        std::memcpy(rows.data() + slot * 16, g.data(), sizeof(float) * 16);
    };
    const auto constants = [](float time, float amplitude) {
        FoliageConstants c;
        const float consts[4] = {1.0f / 16384, 1.0f / 16384, 0.5f, 0.25f}, wave[4] = {0, 0, 0, time}, dir[4] = {amplitude, 0, 0, 0};
        std::memcpy(c.consts, consts, sizeof(consts));
        std::memcpy(c.wave, wave, sizeof(wave));
        std::memcpy(c.wind, dir, sizeof(dir));
        return c;
    };
    const auto overrides = [](const FoliageConstants& c, const DetailRows& rows) {
        return Overrides{{"consts", {c.consts, c.consts + 4}}, {"wave", {c.wave, c.wave + 4}}, {"dir2D", {c.wind, c.wind + 4}},
            {"array", {rows.begin(), rows.end()}}};
    };
    const auto now = constants(0.30f, 0.08f), before = constants(0.80f, 0.05f);
    const std::uint64_t route = 0xfeed;
    // The previous frame drew plant A in group 0 (scale 0.8: it has grown since) and plant B in group 1. This frame the
    // engine batches them the other way round: B in group 0, A in group 1. Quad B reads rows at offset 0, quad A at 4.
    FoliageHistory history;
    const auto upload_for = [&](bool include_b) {
        history.reset_all();
        history.begin_frame(temporal::ViewId::Main, 1, 0, size, size);
        DetailRows previous = empty_batch_rows();
        put(previous, 0, group(a, 0.8f));
        if (include_b) put(previous, 1, group(b, 1.0f));
        history.record_detail(temporal::ViewId::Main, route, before, previous);
        history.begin_frame(temporal::ViewId::Main, 2, 0, size, size);
        DetailRows current = empty_batch_rows();
        put(current, 0, group(b, 1.0f));
        put(current, 1, group(a, 1.0f));
        const auto remap = history.record_detail(temporal::ViewId::Main, route, now, current);
        return std::make_pair(current, remap);
    };
    const std::vector<ComPtr<ID3D11Buffer>> quads{foliage_quad(rig, -0.5f, 0.5f, -1, 1, 0, 8192, 0),   // plant B (offset 0)
        foliage_quad(rig, -0.5f, 0.5f, -1, 1, 0, 8192, 4)};                                          // plant A (offset 4)
    // Quad B must land on the right half and quad A on the left: shift each by its own translation via the rows (tx = 0.5 / -0.5).
    auto [current, remap] = upload_for(true);
    require(remap.live == 2 && remap.matched == 2 && remap.route_found, "both plants remapped");
    FoliageVertexBytes bytes;
    require(pack_foliage_detail(temporal::Mat4::identity(), remap.previous, remap.items, remap.live, bytes), "pack detail history");
    const auto stock = draw_foliage_pair(rig, motion, false, overrides(now, current), quads, nullptr);
    const auto drawn = draw_foliage_pair(rig, motion, true, overrides(now, current), quads, &bytes);
    const float frac = 8192.0f / 16384;
    unsigned left = 0, right = 0;
    float largest = 0;
    for (UINT j = 0; j < size; ++j)
        for (UINT i = 0; i < size; ++i) {
            const auto k = j * size + i;
            require(std::memcmp(&drawn.position[k], &stock.position[k], sizeof(Half4)) == 0 && drawn.albedo[k] == stock.albedo[k],
                "native MRT0/1 bit-identical for the grass");
            if (half(stock.position[k].c[0]) == -999) {
                require(drawn.vectors[k].c[2] == 0, "uncovered pixels stay invalid");
                continue;
            }
            const float X = ndc_x(i), Y = ndc_y(j);
            if (std::abs(X) < 0.1f) continue;   // the two quads may overlap there once the sway shears them
            const bool is_a = X < 0;   // plant A (tx = -0.5, previous scale 0.8) is the left half, B the right half
            const Plant& plant = is_a ? a : b;
            const float s_now = 1.0f, s_prev = is_a ? 0.8f : 1.0f;
            const float y_obj = Y / s_now;
            const float res_now = now.wind[0] * (s_now * y_obj) * cyclic(now.wave[3]) * frac;
            const float x_obj = (X - plant.tx - res_now) / s_now;
            const float prev_x = s_prev * x_obj + plant.tx + before.wind[0] * (s_prev * y_obj) * cyclic(before.wave[3]) * frac;
            const float prev_y = s_prev * y_obj;
            const float ex = (X - prev_x) / 2, ey = -(Y - prev_y) / 2;
            const float mx = half(drawn.vectors[k].c[0]), my = half(drawn.vectors[k].c[1]);
            require(std::abs(mx - ex) < 5e-4f && std::abs(my - ey) < 5e-4f && half(drawn.vectors[k].c[2]) == 1,
                "grass motion at " + std::to_string(i) + "," + std::to_string(j) + ": " + std::to_string(mx) + "," + std::to_string(my) +
                " vs " + std::to_string(ex) + "," + std::to_string(ey));
            largest = std::max(largest, std::abs(mx));
            ++(is_a ? left : right);
        }
    require(left > 100 && right > 100 && largest > 4e-3f, "both plants moved with the wind");

    // A plant the previous frame did not draw has no history; its neighbour keeps its own.
    auto [current2, remap2] = upload_for(false);
    require(remap2.live == 2 && remap2.matched == 1 && remap2.unmatched == 1, "only plant A has history");
    require(pack_foliage_detail(temporal::Mat4::identity(), remap2.previous, remap2.items, remap2.live, bytes), "pack partial history");
    const auto partial = draw_foliage_pair(rig, motion, true, overrides(now, current2), quads, &bytes);
    unsigned a_valid = 0, b_zero = 0;
    for (UINT j = 0; j < size; ++j)
        for (UINT i = 0; i < size; ++i) {
            const auto k = j * size + i;
            if (half(stock.position[k].c[0]) == -999 || std::abs(ndc_x(i)) < 0.1f) continue;
            if (ndc_x(i) < 0) { require(half(partial.vectors[k].c[2]) == 1, "plant A valid"); ++a_valid; }
            else {
                require(partial.vectors[k].c[0] == 0 && partial.vectors[k].c[1] == 0 && partial.vectors[k].c[2] == 0 && partial.vectors[k].c[3] == 0,
                    "a plant without history is zero, not a guess");
                require(std::memcmp(&partial.position[k], &stock.position[k], sizeof(Half4)) == 0, "native unchanged");
                ++b_zero;
            }
        }
    require(a_valid > 100 && b_zero > 100, "partial batch covered both plants");
    // Whole batch without history (first frame, route change): all zero.
    pack_foliage_invalid(bytes);
    const auto none = draw_foliage_pair(rig, motion, true, overrides(now, current), quads, &bytes);
    for (UINT k = 0; k < size * size; ++k)
        require(none.vectors[k].c[0] == 0 && none.vectors[k].c[1] == 0 && none.vectors[k].c[2] == 0 && none.vectors[k].c[3] == 0, "no history is zero");
    std::printf("grass: remapped batch motion up to %.4f UV (%u + %u pixels), partial and empty history cases pass\n", largest, left, right);
}
}

int main()
{
    try {
        wchar_t buffer[32768]{};
        const DWORD length = GetEnvironmentVariableW(L"RSF_ANOMALY_MOTION_DEPLOY", buffer, 32768);
        deployment = length && length < 32768 ? std::filesystem::path(buffer)
                                              : std::filesystem::path(RSF_SOURCE_DIR) / ".local/anomaly/motion-deploy";
        if (!std::filesystem::exists(deployment / "manifest.txt")) {
            std::printf("SKIP: no local motion deployment at %s (run tools/anomaly_motion_deploy.py)\n", deployment.string().c_str());
            return 77;
        }
        D3D_FEATURE_LEVEL level{};
        bool warp = false, debug = true;
        // The NVIDIA adapter (vendor 0x10de) when present: adapter 0 on the hybrid laptop is the Intel iGPU.
        ComPtr<IDXGIAdapter1> nvidia;
        {
            ComPtr<IDXGIFactory1> factory;
            if (SUCCEEDED(CreateDXGIFactory1(IID_PPV_ARGS(&factory))))
                for (UINT i = 0;; ++i) {
                    ComPtr<IDXGIAdapter1> candidate;
                    if (factory->EnumAdapters1(i, &candidate) == DXGI_ERROR_NOT_FOUND) break;
                    DXGI_ADAPTER_DESC1 desc{};
                    candidate->GetDesc1(&desc);
                    if (desc.VendorId == 0x10de && !(desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE)) { nvidia = candidate; break; }
                }
        }
        const auto create = [&](UINT flags) {
            return nvidia ? D3D11CreateDevice(nvidia.Get(), D3D_DRIVER_TYPE_UNKNOWN, nullptr, flags, nullptr, 0, D3D11_SDK_VERSION, &device,
                                &level, &context)
                          : D3D11CreateDevice(nullptr, D3D_DRIVER_TYPE_HARDWARE, nullptr, flags, nullptr, 0, D3D11_SDK_VERSION, &device,
                                &level, &context);
        };
        HRESULT result = create(D3D11_CREATE_DEVICE_DEBUG);
        if (FAILED(result)) {
            debug = false;
            result = create(0);
        }
        if (FAILED(result)) {
            warp = true;
            result = D3D11CreateDevice(nullptr, D3D_DRIVER_TYPE_WARP, nullptr, 0, nullptr, 0, D3D11_SDK_VERSION, &device, &level, &context);
        }
        if (FAILED(result)) { std::printf("SKIP: no D3D11 device\n"); return 77; }
        require(level >= D3D_FEATURE_LEVEL_11_0, "feature level 11");
        ComPtr<IDXGIDevice> dxgi;
        ComPtr<IDXGIAdapter> adapter;
        DXGI_ADAPTER_DESC adapter_desc{};
        if (SUCCEEDED(device.As(&dxgi)) && SUCCEEDED(dxgi->GetAdapter(&adapter))) adapter->GetDesc(&adapter_desc);
        std::printf("device: %ls (%s, debug layer %s)\n", adapter_desc.Description, warp ? "WARP" : "hardware", debug ? "on" : "off");
        // The engine's compiler (motion_profile.h); RSF_ANOMALY_MOTION_COMPILER overrides it for experiments.
        wchar_t compiler_name[MAX_PATH] = L"d3dcompiler_43.dll";
        GetEnvironmentVariableW(L"RSF_ANOMALY_MOTION_COMPILER", compiler_name, MAX_PATH);
        D3DShaderApi shader_api(compiler_name);
        if (!shader_api.ready()) { std::printf("SKIP: %ls not available\n", compiler_name); return 77; }
        std::printf("compiler: %ls\n", compiler_name);
        api = &shader_api;
        const auto text = file_bytes(deployment / "manifest.txt");
        std::string error;
        auto manifest = Manifest::parse(std::string(reinterpret_cast<const char*>(text.data()), text.size()), error);
        require(manifest.has_value(), "manifest: " + error);
        ShaderLoader shader_loader(*manifest, deployment, {}, shader_api, shader_api);
        loader = &shader_loader;
        const auto list = roots(deployment / "manifest.txt");
        require(list.size() == manifest->by_original.size(), "manifest roots");
        every_root(list);
        foliage_layouts(list);
        draw_scene();
        draw_tree();
        draw_detail();
        std::printf("anomaly motion device: %d checks passed\n", checks);
        return 0;
    } catch (const std::exception& error) {
        std::fprintf(stderr, "FAILED %s\n", error.what());
        return 1;
    }
}
