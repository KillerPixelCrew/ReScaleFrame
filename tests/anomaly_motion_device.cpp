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
#include "motion_d3d11.h"
#include "shader_d3d.h"
#include "shader_loader.h"
#include <d3d11.h>
#include <d3d11sdklayers.h>
#include <d3dcompiler.h>
#include <wrl/client.h>
#include <algorithm>
#include <bit>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <filesystem>
#include <fstream>
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
    request.target = root.stage == 'v' ? "vs_5_0" : "ps_5_0";
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
        HRESULT result = D3D11CreateDevice(nullptr, D3D_DRIVER_TYPE_HARDWARE, nullptr, D3D11_CREATE_DEVICE_DEBUG, nullptr, 0,
            D3D11_SDK_VERSION, &device, &level, &context);
        if (FAILED(result)) {
            debug = false;
            result = D3D11CreateDevice(nullptr, D3D_DRIVER_TYPE_HARDWARE, nullptr, 0, nullptr, 0, D3D11_SDK_VERSION, &device, &level, &context);
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
        draw_scene();
        std::printf("anomaly motion device: %d checks passed\n", checks);
        return 0;
    } catch (const std::exception& error) {
        std::fprintf(stderr, "FAILED %s\n", error.what());
        return 1;
    }
}
