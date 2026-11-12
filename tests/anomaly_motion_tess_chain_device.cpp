// SPDX-License-Identifier: GPL-3.0-only
// Tessellated motion chain device fixture. Uses the LOCAL deployment from tools/anomaly_motion_deploy.py (licensed shader
// bodies, never tracked): RSF_ANOMALY_MOTION_DEPLOY, else <repo>/.local/anomaly/motion-deploy-tess, else
// <repo>/.local/anomaly/motion-deploy. Exits 77 when no deployment with the dx11/tess.hs and dx11/tess.ds roots exists,
// when d3dcompiler_43 is missing or when there is no hardware D3D11 device. Runs on the NVIDIA adapter (vendor 0x10de)
// when present (adapter 0 on the hybrid laptop is the Intel iGPU), with the debug layer when it is installed.
//
// What it proves, for X-Ray's G-buffer tessellation pass (r4_enable_tessellation: VS -> DX11\tess.hs -> DX11\tess.ds ->
// PS, uber_deffer.cpp r_TessPass):
// 1. The four real roots (deffer_model_bump.vs SKIN_n, dx11/tess.hs, dx11/tess.ds, deffer_base_bump.ps) of every
//    tessellation permutation in the user's shader cache (TESS_PN with SKIN_0..4, TESS_PN + USE_TDETAIL +
//    USE_TDETAIL_BUMP with SKIN_2/3) go through the production ShaderLoader with the engine's compiler: original
//    reproduced byte for byte, motion variant reflection-checked (compatible() for 'h'/'d').
// 2. Linkage: the all-variant chain links (tess_chain_linkable); a stock hull, a stock domain, a stock VS or the
//    non-tessellation VS variant in the chain is refused.
// 3. A skinned mover drawn through the chain: an octahedron whose vertex normals point outward, so PN tessellation (triLOD
//    8) bulges it towards a sphere (and, in the opt-in HM cases, displaces it along the PN normal by a varying height
//    texel). Its previous palette (SKIN_1..4) or bone-composed previous world (SKIN_0) is a rigid motion of the current
//    one (rotation about two axes plus a translation), the previous camera a different view and projection. PN
//    construction, displacement along the normal and the edge rule are all equivariant under rigid motion, so the exact
//    previous clip position of every visible sample is P_prev * V_prev * B_prev * x, with x reconstructed from the pixel
//    centre and the D32 depth. The motion target must equal that CPU model at every covered pixel with validity 1, the
//    native MRT0/MRT1 and depth must be bit-identical to the stock chain, the debug layer must stay silent, and no history
//    must write all-zero motion. The fixture also reports (and requires to be large) how far a FLAT-triangle previous
//    position would be from the measured vectors, so a chain that ignored the patch could not pass.
// Diagnostics (environment): RSF_TESS_HM=1 adds the TESS_HM cases; RSF_TESS_DIAG=1 lists misses instead of stopping;
// RSF_TESS_IDENTITY=1 makes the previous state equal the current one (all motion must be zero); RSF_TESS_DUMP=<dir>
// writes the stock and motion hull/domain bytecode; RSF_TESS_CREATE=stock|motion[-vhdp] only creates one shader set of the
// HM case; RSF_TESS_RAW=<file.hs.*|file.ds.*> creates one dumped shader on a fresh device; RSF_TESS_NODEBUG=1 skips the
// debug layer.
#include "motion_constants.h"
#include "motion_d3d11.h"
#include "motion_policy.h"
#include "shader_d3d.h"
#include "shader_loader.h"
#include <d3d11.h>
#include <d3d11sdklayers.h>
#include <d3dcompiler.h>
#include <dxgi.h>
#include <wrl/client.h>
#include <algorithm>
#include <array>
#include <bit>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <map>
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
bool debug_layer = false;
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
std::uint64_t debug_errors(std::string* first = nullptr)
{
    ComPtr<ID3D11InfoQueue> queue;
    if (FAILED(device.As(&queue))) return 0;
    std::uint64_t errors = 0;
    for (UINT64 i = 0, n = queue->GetNumStoredMessages(); i < n; ++i) {
        SIZE_T length = 0;
        queue->GetMessage(i, nullptr, &length);
        std::vector<char> storage(length);
        auto* message = reinterpret_cast<D3D11_MESSAGE*>(storage.data());
        if (FAILED(queue->GetMessage(i, message, &length)) || message->Severity > D3D11_MESSAGE_SEVERITY_ERROR) continue;
        if (!errors++ && first) first->assign(message->pDescription, message->DescriptionByteLength);
    }
    queue->ClearStoredMessages();
    return errors;
}
void require_clean_debug_layer(const char* what)
{
    std::string first;
    const auto errors = debug_errors(&first);
    require(errors == 0, std::string(what) + ": debug layer error: " + first);
}
std::vector<std::byte> file_bytes(const std::filesystem::path& path)
{
    std::ifstream file(path, std::ios::binary);
    require(file.good(), "missing " + path.string());
    std::vector<char> data((std::istreambuf_iterator<char>(file)), std::istreambuf_iterator<char>());
    std::vector<std::byte> out(data.size());
    if (!data.empty()) std::memcpy(out.data(), data.data(), data.size());
    return out;
}

std::filesystem::path deployment;
D3DShaderApi* api{};
ShaderLoader* loader{};

const char* target_of(char stage)
{
    switch (stage) {
    case 'v': return "vs_5_0";
    case 'h': return "hs_5_0";
    case 'd': return "ds_5_0";
    default: return "ps_5_0";
    }
}
struct Built {
    char stage{};
    std::vector<std::byte> stock;
    Prepared prepared;
};
Built build(char stage, const std::string& root, const std::vector<Define>& defines)
{
    ShaderRequest request;
    request.target = target_of(stage);
    request.entry = "main";
    request.flags = 8;   // D3D10_SHADER_PACK_MATRIX_ROW_MAJOR, what CResourceManager passes for every stage
    request.defines = defines;
    request.source = file_bytes(deployment / "original" / "r3" / root);
    Built built{stage};
    std::string error;
    require(api->compile(deployment / "original", request.source, request, built.stock, error), root + " stock: " + error);
    request.bytecode = built.stock;
    built.prepared = loader->prepare(request);
    require(built.prepared.outcome == Outcome::Replaced || built.prepared.outcome == Outcome::ReplacedFromCache,
        root + " " + request.target + ": " + outcome_name(built.prepared.outcome) + " " + built.prepared.detail);
    return built;
}
Reflection reflection_of(const std::vector<std::byte>& code)
{
    Reflection r;
    require(api->reflect(code, r), "reflect");
    return r;
}

// ---- engine bindings ---------------------------------------------------------------------------------------------
using Overrides = std::map<std::string, std::vector<float>>;
struct Bindings {
    std::vector<ComPtr<ID3D11Buffer>> buffers;
    std::vector<ComPtr<ID3D11ShaderResourceView>> views;
    std::vector<ComPtr<ID3D11SamplerState>> samplers;
};
ComPtr<ID3D11ShaderResourceView> texture(UINT width, const std::vector<float>& texels)
{
    D3D11_TEXTURE2D_DESC td{};
    td.Width = td.Height = width; td.MipLevels = td.ArraySize = 1; td.Format = DXGI_FORMAT_R32G32B32A32_FLOAT;
    td.SampleDesc.Count = 1; td.BindFlags = D3D11_BIND_SHADER_RESOURCE;
    D3D11_SUBRESOURCE_DATA init{texels.data(), width * 16};
    ComPtr<ID3D11Texture2D> t;
    hr(device->CreateTexture2D(&td, &init, &t), "texture");
    ComPtr<ID3D11ShaderResourceView> srv;
    hr(device->CreateShaderResourceView(t.Get(), nullptr, &srv), "srv");
    return srv;
}
// Height texels of the displacement map (s_tbumpX alpha): a 4x4 ramp, so the HM displacement differs across the mover.
std::vector<float> height_texels()
{
    std::vector<float> t(4 * 4 * 4);
    for (int i = 0; i < 16; ++i) { t[i * 4 + 0] = 0.5f; t[i * 4 + 1] = 0.5f; t[i * 4 + 2] = 1.0f; t[i * 4 + 3] = 0.15f + 0.05f * float(i); }
    return t;
}
// Engine constant buffers of the STOCK reflection: every matrix identity, everything else 0.1 unless overridden; RSF b13
// skipped (MotionDevice binds it). Textures 2x2 constant (s_tbumpX: the height ramp), samplers wrap/linear.
void bind_stage(char stage, const std::vector<std::byte>& code, const Overrides& overrides, Bindings& keep)
{
    ComPtr<ID3D11ShaderReflection> reflection;
    hr(D3DReflect(code.data(), code.size(), IID_PPV_ARGS(&reflection)), "reflect");
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
                if (const auto found = overrides.find(vd.Name); found != overrides.end())
                    std::copy_n(found->second.begin(), std::min<std::size_t>(found->second.size(), vd.Size / 4), out);
            }
            D3D11_BUFFER_DESC b{};
            b.ByteWidth = static_cast<UINT>(values.size() * 4);
            b.BindFlags = D3D11_BIND_CONSTANT_BUFFER;
            D3D11_SUBRESOURCE_DATA init{values.data()};
            ComPtr<ID3D11Buffer> buffer;
            hr(device->CreateBuffer(&b, &init, &buffer), "engine cb");
            ID3D11Buffer* p = buffer.Get();
            switch (stage) {
            case 'v': context->VSSetConstantBuffers(binding.BindPoint, 1, &p); break;
            case 'h': context->HSSetConstantBuffers(binding.BindPoint, 1, &p); break;
            case 'd': context->DSSetConstantBuffers(binding.BindPoint, 1, &p); break;
            default: context->PSSetConstantBuffers(binding.BindPoint, 1, &p); break;
            }
            keep.buffers.push_back(buffer);
        } else if (binding.Type == D3D_SIT_TEXTURE) {
            const auto srv = std::string(binding.Name) == "s_tbumpX"
                ? texture(4, height_texels())
                : texture(2, {0.5f, 0.4f, 0.6f, 1, 0.5f, 0.4f, 0.6f, 1, 0.5f, 0.4f, 0.6f, 1, 0.5f, 0.4f, 0.6f, 1});
            ID3D11ShaderResourceView* p = srv.Get();
            switch (stage) {
            case 'v': context->VSSetShaderResources(binding.BindPoint, 1, &p); break;
            case 'h': context->HSSetShaderResources(binding.BindPoint, 1, &p); break;
            case 'd': context->DSSetShaderResources(binding.BindPoint, 1, &p); break;
            default: context->PSSetShaderResources(binding.BindPoint, 1, &p); break;
            }
            keep.views.push_back(srv);
        } else if (binding.Type == D3D_SIT_SAMPLER) {
            D3D11_SAMPLER_DESC sd{};
            sd.Filter = D3D11_FILTER_MIN_MAG_MIP_LINEAR;
            sd.AddressU = sd.AddressV = sd.AddressW = D3D11_TEXTURE_ADDRESS_WRAP;
            sd.MaxLOD = D3D11_FLOAT32_MAX;
            ComPtr<ID3D11SamplerState> sampler;
            hr(device->CreateSamplerState(&sd, &sampler), "sampler");
            ID3D11SamplerState* p = sampler.Get();
            switch (stage) {
            case 'v': context->VSSetSamplers(binding.BindPoint, 1, &p); break;
            case 'h': context->HSSetSamplers(binding.BindPoint, 1, &p); break;
            case 'd': context->DSSetSamplers(binding.BindPoint, 1, &p); break;
            default: context->PSSetSamplers(binding.BindPoint, 1, &p); break;
            }
            keep.samplers.push_back(sampler);
        }
    }
}

// ---- math (column-vector, row-major storage like temporal::Mat4) ---------------------------------------------------
using Vec3 = std::array<double, 3>;
temporal::Mat4 identity() { return temporal::Mat4::identity(); }
temporal::Mat4 rotation_x(double a)
{
    auto m = identity();
    m.values[5] = std::cos(a); m.values[6] = -std::sin(a); m.values[9] = std::sin(a); m.values[10] = std::cos(a);
    return m;
}
temporal::Mat4 rotation_y(double a)
{
    auto m = identity();
    m.values[0] = std::cos(a); m.values[2] = std::sin(a); m.values[8] = -std::sin(a); m.values[10] = std::cos(a);
    return m;
}
temporal::Mat4 translation(double x, double y, double z)
{
    auto m = identity();
    m.values[3] = x; m.values[7] = y; m.values[11] = z;
    return m;
}
std::array<double, 4> apply(const temporal::Mat4& m, const std::array<double, 4>& v)
{
    std::array<double, 4> out{};
    for (int r = 0; r < 4; ++r) for (int c = 0; c < 4; ++c) out[r] += m.values[r * 4 + c] * v[c];
    return out;
}
Vec3 normalize(Vec3 v)
{
    const double l = std::sqrt(v[0] * v[0] + v[1] * v[1] + v[2] * v[2]);
    return {v[0] / l, v[1] / l, v[2] / l};
}
Vec3 cross(const Vec3& a, const Vec3& b) { return {a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]}; }

// ---- readback ----------------------------------------------------------------------------------------------------
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
    if (!exponent) return (sign ? -1.0f : 1.0f) * std::ldexp(float(mantissa), -24);
    if (exponent == 31) return std::bit_cast<float>(sign | 0x7f800000u | (mantissa << 13));
    return std::bit_cast<float>(sign | ((exponent + 112) << 23) | (mantissa << 13));
}

// ---- the chain -----------------------------------------------------------------------------------------------------
struct Case {
    const char* name;
    int skin;
    std::vector<Define> tess;   // what r_TessPass adds to all four stages
};
std::vector<Define> engine_defines() { return {{"SMAP_size", "2048"}, {"USE_BRANCHING", "1"}, {"USE_VTF", "1"}, {"SM_5", "1"}}; }

struct Chain {
    Built vs, hs, ds, ps;
    ComPtr<ID3D11VertexShader> stock_vs, motion_vs;
    ComPtr<ID3D11HullShader> stock_hs, motion_hs;
    ComPtr<ID3D11DomainShader> stock_ds, motion_ds;
    ComPtr<ID3D11PixelShader> stock_ps, motion_ps;
    ComPtr<ID3D11InputLayout> layout;
    ComPtr<ID3D11Buffer> vertices;
    UINT stride{}, count{};
};

// An octahedron (radius 0.6 in x/y, 0.3 in z, centre z 0.5) with outward vertex normals, 8 patches of 3 control points.
// Ortho identity camera: object = view = clip; visible x, y in [-1, 1], depth = z.
constexpr Vec3 centre{0.0, 0.0, 0.5};
constexpr double radius_xy = 0.6, radius_z = 0.3;
void make_vertices(Chain& chain, int skin)
{
    ComPtr<ID3D11ShaderReflection> reflection;
    hr(D3DReflect(chain.vs.stock.data(), chain.vs.stock.size(), IID_PPV_ARGS(&reflection)), "reflect VS");
    D3D11_SHADER_DESC desc{};
    reflection->GetDesc(&desc);
    std::vector<D3D11_INPUT_ELEMENT_DESC> elements;
    std::vector<std::string> names;
    names.reserve(desc.InputParameters);
    std::vector<D3D11_SIGNATURE_PARAMETER_DESC> inputs(desc.InputParameters);
    for (UINT i = 0; i < desc.InputParameters; ++i) {
        reflection->GetInputParameterDesc(i, &inputs[i]);
        names.emplace_back(inputs[i].SemanticName);
        const DXGI_FORMAT format = inputs[i].ComponentType == D3D_REGISTER_COMPONENT_FLOAT32 ? DXGI_FORMAT_R32G32B32A32_FLOAT :
            inputs[i].ComponentType == D3D_REGISTER_COMPONENT_UINT32 ? DXGI_FORMAT_R32G32B32A32_UINT : DXGI_FORMAT_R32G32B32A32_SINT;
        elements.push_back({names.back().c_str(), inputs[i].SemanticIndex, format, 0, i * 16, D3D11_INPUT_PER_VERTEX_DATA, 0});
    }
    // The engine builds its input layout against the STOCK signature; the variant must accept it unchanged.
    hr(device->CreateInputLayout(elements.data(), static_cast<UINT>(elements.size()), chain.vs.stock.data(), chain.vs.stock.size(),
        &chain.layout), "layout");
    chain.stride = desc.InputParameters * 16;
    const Vec3 axes[6]{{1, 0, 0}, {-1, 0, 0}, {0, 1, 0}, {0, -1, 0}, {0, 0, 1}, {0, 0, -1}};
    std::vector<int> corners;
    for (int sx = 0; sx < 2; ++sx) for (int sy = 0; sy < 2; ++sy) for (int sz = 0; sz < 2; ++sz) {
        corners.push_back(sx); corners.push_back(2 + sy); corners.push_back(4 + sz);
    }
    chain.count = static_cast<UINT>(corners.size());
    std::vector<float> data(chain.count * desc.InputParameters * 4, 0.0f);
    // skin.h unpacks D3DCOLOR normals: the shader swizzles .zyx and maps [0, 1] to [-1, 1].
    const auto packed = [](const Vec3& n) { return std::array<float, 3>{float((n[2] + 1) / 2), float((n[1] + 1) / 2), float((n[0] + 1) / 2)}; };
    for (UINT v = 0; v < chain.count; ++v) {
        const Vec3 n = axes[corners[v]];
        const Vec3 position{centre[0] + radius_xy * n[0], centre[1] + radius_xy * n[1], centre[2] + radius_z * n[2]};
        const Vec3 up = std::abs(n[1]) > 0.9 ? Vec3{1, 0, 0} : Vec3{0, 1, 0};
        const Vec3 t = normalize(cross(up, n)), b = cross(n, t);
        for (UINT i = 0; i < desc.InputParameters; ++i) {
            float* out = data.data() + (v * desc.InputParameters + i) * 4;
            const std::string semantic = inputs[i].SemanticName;
            if (semantic == "POSITION") { out[0] = float(position[0]); out[1] = float(position[1]); out[2] = float(position[2]); out[3] = 1; }
            else if (semantic == "NORMAL") { const auto p = packed(n); out[0] = p[0]; out[1] = p[1]; out[2] = p[2]; out[3] = skin == 2 ? 0.25f : 0.0f; }
            else if (semantic == "TANGENT") { const auto p = packed(t); out[0] = p[0]; out[1] = p[1]; out[2] = p[2]; out[3] = 0; }
            else if (semantic == "BINORMAL") { const auto p = packed(b); out[0] = p[0]; out[1] = p[1]; out[2] = p[2]; out[3] = 0; }
            else if (semantic == "TEXCOORD" && inputs[i].SemanticIndex == 0) {
                // u, v from the normal; SKIN_2 bone rows in z/w (palette row 0 and 3: both bones carry the same matrix).
                out[0] = float(0.5 + 0.45 * n[0] + 0.2 * n[2]); out[1] = float(0.5 + 0.45 * n[1] - 0.2 * n[2]);
                out[2] = 0; out[3] = 3;
            }
        }
    }
    D3D11_BUFFER_DESC bd{};
    bd.ByteWidth = static_cast<UINT>(data.size() * 4);
    bd.BindFlags = D3D11_BIND_VERTEX_BUFFER;
    D3D11_SUBRESOURCE_DATA init{data.data()};
    hr(device->CreateBuffer(&bd, &init, &chain.vertices), "vertex buffer");
}

Chain make_chain(const Case& c)
{
    auto common = engine_defines();
    for (const auto& d : c.tess) common.push_back(d);
    auto vs_defines = common;
    vs_defines.push_back({"SKIN_" + std::to_string(c.skin), "1"});
    Chain chain{build('v', "deffer_model_bump.vs", vs_defines), build('h', "dx11/tess.hs", common),
        build('d', "dx11/tess.ds", common), build('p', "deffer_base_bump.ps", common)};
    require(chain.vs.prepared.skin == c.skin, "VS skin mode");
    if (const char* dump = std::getenv("RSF_TESS_DUMP")) {
        for (const auto* b : {&chain.hs, &chain.ds}) {
            const std::string tag = std::string(dump) + "/" + c.name + (b->stage == 'h' ? ".hs" : ".ds");
            std::ofstream(tag + ".stock.dxbc", std::ios::binary).write(reinterpret_cast<const char*>(b->stock.data()), b->stock.size());
            std::ofstream(tag + ".motion.dxbc", std::ios::binary)
                .write(reinterpret_cast<const char*>(b->prepared.variant.data()), b->prepared.variant.size());
        }
    }
    require(chain.hs.prepared.entry->family == Family::TessHull && chain.ds.prepared.entry->family == Family::TessDomain,
        "hull/domain families from the manifest");
    hr(device->CreateVertexShader(chain.vs.stock.data(), chain.vs.stock.size(), nullptr, &chain.stock_vs), "stock VS");
    hr(device->CreateHullShader(chain.hs.stock.data(), chain.hs.stock.size(), nullptr, &chain.stock_hs), "stock HS");
    hr(device->CreateDomainShader(chain.ds.stock.data(), chain.ds.stock.size(), nullptr, &chain.stock_ds), "stock DS");
    hr(device->CreatePixelShader(chain.ps.stock.data(), chain.ps.stock.size(), nullptr, &chain.stock_ps), "stock PS");
    const auto& v = chain.vs.prepared.variant; const auto& h = chain.hs.prepared.variant;
    const auto& d = chain.ds.prepared.variant; const auto& p = chain.ps.prepared.variant;
    hr(device->CreateVertexShader(v.data(), v.size(), nullptr, &chain.motion_vs), "motion VS");
    hr(device->CreateHullShader(h.data(), h.size(), nullptr, &chain.motion_hs), "motion HS");
    hr(device->CreateDomainShader(d.data(), d.size(), nullptr, &chain.motion_ds), "motion DS");
    hr(device->CreatePixelShader(p.data(), p.size(), nullptr, &chain.motion_ps), "motion PS");
    make_vertices(chain, c.skin);
    return chain;
}

// Diagnostics (RSF_TESS_CREATE=stock|motion): only create one shader set of a case, tracing each driver call.
void create_probe(const Case& c, const std::string& which)
{
    auto common = engine_defines();
    for (const auto& d : c.tess) common.push_back(d);
    auto vs_defines = common;
    vs_defines.push_back({"SKIN_" + std::to_string(c.skin), "1"});
    const Built built[]{build('v', "deffer_model_bump.vs", vs_defines), build('h', "dx11/tess.hs", common),
        build('d', "dx11/tess.ds", common), build('p', "deffer_base_bump.ps", common)};
    const auto set = which.substr(0, which.find('-'));
    const auto only = which.find('-') == std::string::npos ? std::string("vhdp") : which.substr(which.find('-') + 1);
    for (const auto& b : built) {
        if (only.find(b.stage) == std::string::npos) continue;
        const auto& code = set == "stock" ? b.stock : b.prepared.variant;
        std::printf("    create %s %c (%zu bytes)\n", which.c_str(), b.stage, code.size());
        ComPtr<ID3D11DeviceChild> object;
        switch (b.stage) {
        case 'v': { ComPtr<ID3D11VertexShader> s; hr(device->CreateVertexShader(code.data(), code.size(), nullptr, &s), "VS"); object = s; break; }
        case 'h': { ComPtr<ID3D11HullShader> s; hr(device->CreateHullShader(code.data(), code.size(), nullptr, &s), "HS"); object = s; break; }
        case 'd': { ComPtr<ID3D11DomainShader> s; hr(device->CreateDomainShader(code.data(), code.size(), nullptr, &s), "DS"); object = s; break; }
        default: { ComPtr<ID3D11PixelShader> s; hr(device->CreatePixelShader(code.data(), code.size(), nullptr, &s), "PS"); object = s; break; }
        }
    }
    context->Flush();
    Sleep(500);   // let deferred driver compilation finish inside the process
    std::printf("    created %s set of %s\n", which.c_str(), c.name);
}

// Linkage rules on the real reflections.
void linkage(const Case& c, const Chain& chain)
{
    const auto& vs = chain.vs.prepared.reflection;
    const auto& hs = chain.hs.prepared.reflection;
    const auto& ds = chain.ds.prepared.reflection;
    const auto& ps = chain.ps.prepared.reflection;
    require(tess_chain_linkable(vs, hs, ds, ps), std::string(c.name) + ": the variant chain links");
    require(!tess_chain_linkable(vs, reflection_of(chain.hs.stock), ds, ps), "stock hull refused (it drops the RSF control data)");
    require(!tess_chain_linkable(vs, hs, reflection_of(chain.ds.stock), ps), "stock domain refused (the PS reads TEXCOORD12/13)");
    require(!tess_chain_linkable(reflection_of(chain.vs.stock), hs, ds, ps), "stock VS refused (no previous control data)");
    // The VS variant of the same root WITHOUT the tessellation defines (the far, non-tessellated element) does not write
    // TEXCOORD14/15 and must not feed the hull stage.
    auto plain = engine_defines();
    plain.push_back({"SKIN_" + std::to_string(c.skin), "1"});
    const auto untessellated = build('v', "deffer_model_bump.vs", plain);
    require(!tess_chain_linkable(untessellated.prepared.reflection, hs, ds, ps), "non-tessellation VS variant refused in the chain");
    // The stock domain shader cannot feed the motion PS (what the stage used to do with the VS/PS pair under HS/DS).
    require(!linkable(reflection_of(chain.ds.stock), ps), "stock domain cannot feed the motion PS");
}

struct Frame {
    std::vector<Half4> position, motion;
    std::vector<uint32_t> albedo;
    std::vector<float> depth;
};
struct Targets {
    ComPtr<ID3D11Texture2D> position, albedo, depth;
    ComPtr<ID3D11RenderTargetView> position_rtv, albedo_rtv;
    ComPtr<ID3D11DepthStencilView> dsv;
    ComPtr<ID3D11DepthStencilState> less;
    ComPtr<ID3D11RasterizerState> raster;
};
Targets make_targets()
{
    Targets t;
    D3D11_TEXTURE2D_DESC desc{};
    desc.Width = desc.Height = size; desc.MipLevels = desc.ArraySize = 1; desc.SampleDesc.Count = 1;
    desc.BindFlags = D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE;
    desc.Format = DXGI_FORMAT_R16G16B16A16_FLOAT;
    hr(device->CreateTexture2D(&desc, nullptr, &t.position), "position");
    hr(device->CreateRenderTargetView(t.position.Get(), nullptr, &t.position_rtv), "position rtv");
    desc.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
    hr(device->CreateTexture2D(&desc, nullptr, &t.albedo), "albedo");
    hr(device->CreateRenderTargetView(t.albedo.Get(), nullptr, &t.albedo_rtv), "albedo rtv");
    desc.Format = DXGI_FORMAT_D32_FLOAT; desc.BindFlags = D3D11_BIND_DEPTH_STENCIL;
    hr(device->CreateTexture2D(&desc, nullptr, &t.depth), "depth");
    hr(device->CreateDepthStencilView(t.depth.Get(), nullptr, &t.dsv), "dsv");
    D3D11_DEPTH_STENCIL_DESC ds{};
    ds.DepthEnable = TRUE; ds.DepthWriteMask = D3D11_DEPTH_WRITE_MASK_ALL; ds.DepthFunc = D3D11_COMPARISON_LESS;
    hr(device->CreateDepthStencilState(&ds, &t.less), "depth state");
    D3D11_RASTERIZER_DESC rd{};
    rd.FillMode = D3D11_FILL_SOLID; rd.CullMode = D3D11_CULL_NONE; rd.DepthClipEnable = TRUE;
    hr(device->CreateRasterizerState(&rd, &t.raster), "raster");
    return t;
}

// The current palette is identity (bone 0); the previous one a rigid motion about the mover's centre.
temporal::Mat4 previous_bone()
{
    if (std::getenv("RSF_TESS_IDENTITY")) return identity();
    const auto to_origin = translation(-centre[0], -centre[1], -centre[2]);
    const auto back = translation(centre[0] + 0.04, centre[1] + 0.02, centre[2]);
    return compose(back, compose(compose(rotation_y(0.35), rotation_x(-0.2)), to_origin));
}
CameraSample previous_camera()
{
    CameraSample camera;
    if (std::getenv("RSF_TESS_IDENTITY")) { camera.view = camera.projection = identity(); return camera; }
    camera.view = translation(0.03, -0.02, 0.0);   // the camera moved
    camera.projection = identity();
    camera.projection.values[0] = 1.1;             // and its (affine) projection differs: the DS must use it
    camera.projection.values[5] = 0.9;
    camera.projection.values[3] = -0.01;
    return camera;
}

Frame draw(const Case& c, Chain& chain, Targets& t, MotionDevice* motion, bool history)
{
    const bool variant = motion != nullptr;
    Bindings keep;
    context->ClearState();
    const float sentinel[4]{-999, -999, -999, -999}, zero[4]{};
    context->ClearRenderTargetView(t.position_rtv.Get(), sentinel);
    context->ClearRenderTargetView(t.albedo_rtv.Get(), zero);
    context->ClearDepthStencilView(t.dsv.Get(), D3D11_CLEAR_DEPTH, 1, 0);
    if (motion) motion->clear(context.Get());
    context->IASetInputLayout(chain.layout.Get());
    context->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_3_CONTROL_POINT_PATCHLIST);
    UINT offset = 0;
    ID3D11Buffer* vb = chain.vertices.Get();
    context->IASetVertexBuffers(0, 1, &vb, &chain.stride, &offset);
    context->VSSetShader(variant ? chain.motion_vs.Get() : chain.stock_vs.Get(), nullptr, 0);
    context->HSSetShader(variant ? chain.motion_hs.Get() : chain.stock_hs.Get(), nullptr, 0);
    context->DSSetShader(variant ? chain.motion_ds.Get() : chain.stock_ds.Get(), nullptr, 0);
    context->PSSetShader(variant ? chain.motion_ps.Get() : chain.stock_ps.Get(), nullptr, 0);
    // Current palette: bone 0 identity (rows), every other row identity too (SKIN_2 reads rows 0 and 3).
    std::vector<float> bones(234 * 4, 0.0f);
    for (int b = 0; b < 78; ++b) for (int r = 0; r < 3; ++r) bones[(b * 3 + r) * 4 + r] = 1.0f;
    const Overrides overrides{{"triLOD", {8.0f}}, {"sbones_array", bones}, {"dt_params", {1.0f, 1.0f, 0.0f, 0.0f}}};
    bind_stage('v', chain.vs.stock, overrides, keep);
    bind_stage('h', chain.hs.stock, overrides, keep);
    bind_stage('d', chain.ds.stock, overrides, keep);
    bind_stage('p', chain.ps.stock, overrides, keep);
    if (motion) {
        // The production policy, packers and MotionDevice uploads, as on_flush runs them for a swapped chain.
        // SKIN_0 (CSkeletonX RM_SINGLE): the engine composes the single bone into the world matrix, so the previous
        // bone-composed world carries the motion and there is no palette.
        temporal::InstanceSnapshot previous;
        previous.world = c.skin == 0 ? previous_bone() : identity();
        if (c.skin != 0) previous.bones.assign(78, previous_bone());
        const auto camera = previous_camera();
        CoreDrawInput in;
        in.family = Family::CoreModel;
        in.skin = c.skin;
        in.in_visual = true;
        in.verdict = rsf::anomaly::geometry::DrawVerdict::Identified;
        in.kind = c.skin == 0 ? rsf::anomaly::geometry::DrawKind::SingleBone : rsf::anomaly::geometry::DrawKind::HardwareSkinned;
        in.context = true;
        in.previous_camera = &camera;
        in.previous = &previous;
        const auto decision = decide_core(in);
        require(decision.reason == HistoryReason::Valid, "policy accepts the skinned mover");
        CoreVertexBytes vbytes;
        TessDomainBytes dbytes;
        if (history) {
            require(pack_core_vertex(decision.previous_wvp, decision.bones, decision.flags, vbytes), "pack VS b13");
            require(pack_tess_domain(decision.previous_wv, decision.previous_projection, true, dbytes), "pack DS b13");
        } else {
            pack_core_invalid(vbytes);
            pack_tess_invalid(dbytes);
        }
        PixelBytes pbytes;
        require(pack_pixel({0, 0, float(size), float(size), 0, 0, 0, 0}, pbytes), "pack PS b13");
        require(motion->set_core_vertex(context.Get(), vbytes) && motion->set_tess_domain(context.Get(), dbytes) &&
            motion->set_pixel(context.Get(), pbytes), "b13 uploads");
    }
    ID3D11RenderTargetView* views[3]{t.position_rtv.Get(), t.albedo_rtv.Get(), motion ? motion->rtv() : nullptr};
    context->OMSetRenderTargets(motion ? 3 : 2, views, t.dsv.Get());
    context->OMSetDepthStencilState(t.less.Get(), 0);
    context->RSSetState(t.raster.Get());
    const D3D11_VIEWPORT viewport{0, 0, float(size), float(size), 0, 1};
    context->RSSetViewports(1, &viewport);
    context->Draw(chain.count, 0);
    Frame frame;
    frame.position = read_back<Half4>(t.position.Get());
    frame.albedo = read_back<uint32_t>(t.albedo.Get());
    frame.depth = read_back<float>(t.depth.Get());
    if (motion) frame.motion = read_back<Half4>(motion->texture());
    require_clean_debug_layer(variant ? "motion chain draw" : "stock chain draw");
    return frame;
}

void run(const Case& c)
{
    auto chain = make_chain(c);
    linkage(c, chain);
    auto targets = make_targets();
    MotionDevice motion;
    require(motion.create(device.Get(), nullptr, nullptr), "MotionDevice");
    D3D11_TEXTURE2D_DESC position_desc{};
    targets.position->GetDesc(&position_desc);
    require(motion.ensure_target(position_desc) == TargetResult::Created, "motion target");

    const auto stock = draw(c, chain, targets, nullptr, true);
    const auto moved = draw(c, chain, targets, &motion, true);
    // The flat octahedron covers |x|/0.6 + |y|/0.6 <= 1; PN tessellation must bulge it beyond that.
    unsigned covered = 0, bulge = 0;
    for (UINT y = 0; y < size; ++y)
        for (UINT x = 0; x < size; ++x) {
            if (half(stock.position[y * size + x].c[0]) == -999) continue;
            ++covered;
            const double nx = (x + 0.5) / size * 2 - 1, ny = 1 - (y + 0.5) / size * 2;
            if (std::abs(nx) / radius_xy + std::abs(ny) / radius_xy > 1.08) ++bulge;
        }
    require(covered > 600 && bulge > 60, std::string(c.name) + ": PN tessellation active (" + std::to_string(covered) + " covered, " +
        std::to_string(bulge) + " outside the flat octahedron)");

    const auto bone = previous_bone();
    const auto camera = previous_camera();
    const auto previous_clip_of = compose(camera.projection, compose(camera.view, bone));
    double worst = 0, largest = 0, flat_worst = 0;
    for (UINT y = 0; y < size; ++y)
        for (UINT x = 0; x < size; ++x) {
            const auto i = y * size + x;
            require(std::memcmp(&moved.position[i], &stock.position[i], sizeof(Half4)) == 0 && moved.albedo[i] == stock.albedo[i] &&
                std::bit_cast<uint32_t>(moved.depth[i]) == std::bit_cast<uint32_t>(stock.depth[i]),
                std::string(c.name) + ": native MRT0/MRT1/depth bit-identical at " + std::to_string(i));
            const auto& m = moved.motion[i];
            if (half(stock.position[i].c[0]) == -999) {
                require(m.c[0] == 0 && m.c[1] == 0 && m.c[2] == 0 && m.c[3] == 0, "uncovered pixel stays cleared");
                continue;
            }
            const double u = (x + 0.5) / size, v = (y + 0.5) / size;
            const std::array<double, 4> current{u * 2 - 1, 1 - v * 2, double(stock.depth[i]), 1};
            const auto prev = apply(previous_clip_of, current);
            const double expected_x = u - (prev[0] / prev[3] * 0.5 + 0.5), expected_y = v - (prev[1] / prev[3] * -0.5 + 0.5);
            const double mx = half(m.c[0]), my = half(m.c[1]);
            require(half(m.c[2]) == 1 && m.c[3] == 0, std::string(c.name) + ": validity 1 at " + std::to_string(i));
            const double error = std::max(std::abs(mx - expected_x), std::abs(my - expected_y));
            worst = std::max(worst, error);
            largest = std::max({largest, std::abs(expected_x), std::abs(expected_y)});
            if (std::getenv("RSF_TESS_DIAG") && error >= 3e-4) {
                std::printf("    miss x=%u y=%u depth=%.5f measured %.5f,%.5f expected %.5f,%.5f\n", x, y, stock.depth[i], mx, my,
                    expected_x, expected_y);
                continue;
            }
            require(error < 3e-4, std::string(c.name) + ": motion equals the CPU model at " + std::to_string(i) + " (measured " +
                std::to_string(mx) + "," + std::to_string(my) + " expected " + std::to_string(expected_x) + "," +
                std::to_string(expected_y) + ")");
            // Flat model: the previous position of the FLAT triangle point below/above this sample (the VS's previous clip
            // interpolated without the patch) cannot be reconstructed per pixel exactly; as a bound, the motion of the
            // point projected back onto the octahedron along the current view ray.
            const double sum = std::abs(current[0]) / radius_xy + std::abs(current[1]) / radius_xy;
            if (sum < 1) {
                const double flat_z = centre[2] - radius_z * (1 - sum);   // the visible (near) faces
                const auto flat_prev = apply(previous_clip_of, {current[0], current[1], flat_z, 1});
                const double fx = u - (flat_prev[0] / flat_prev[3] * 0.5 + 0.5), fy = v - (flat_prev[1] / flat_prev[3] * -0.5 + 0.5);
                flat_worst = std::max(flat_worst, std::max(std::abs(fx - mx), std::abs(fy - my)));
            }
        }
    std::printf("  %s: %u covered (%u by the PN bulge), motion up to %.4f UV, worst error %.2e UV (%.4f px); a flat-triangle "
                "previous position would be off by up to %.4f UV (%.2f px)\n",
        c.name, covered, bulge, largest, worst, worst * size, flat_worst, flat_worst * size);
    require(flat_worst > 20 * worst, std::string(c.name) + ": the fixture distinguishes the PN previous surface from a flat one");

    // No history: all-zero motion, native output unchanged.
    const auto none = draw(c, chain, targets, &motion, false);
    for (UINT i = 0; i < size * size; ++i) {
        const auto& m = none.motion[i];
        require(m.c[0] == 0 && m.c[1] == 0 && m.c[2] == 0 && m.c[3] == 0, std::string(c.name) + ": missing history is zero");
        require(std::memcmp(&none.position[i], &stock.position[i], sizeof(Half4)) == 0, "native unchanged without history");
    }
}

bool create_device()
{
    ComPtr<IDXGIFactory1> factory;
    if (FAILED(CreateDXGIFactory1(IID_PPV_ARGS(&factory)))) return false;
    // The NVIDIA adapter (the game's GPU on this machine), else the first hardware adapter.
    for (const UINT want : {0x10deu, 0u})
        for (const UINT flags : {std::getenv("RSF_TESS_NODEBUG") ? 0u : UINT(D3D11_CREATE_DEVICE_DEBUG), 0u})
            for (UINT i = 0;; ++i) {
                ComPtr<IDXGIAdapter1> adapter;
                if (factory->EnumAdapters1(i, &adapter) == DXGI_ERROR_NOT_FOUND) break;
                DXGI_ADAPTER_DESC1 desc{};
                adapter->GetDesc1(&desc);
                if ((desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE) || (want && desc.VendorId != want)) continue;
                const D3D_FEATURE_LEVEL level = D3D_FEATURE_LEVEL_11_0;
                if (SUCCEEDED(D3D11CreateDevice(adapter.Get(), D3D_DRIVER_TYPE_UNKNOWN, nullptr, flags, &level, 1, D3D11_SDK_VERSION,
                        &device, nullptr, &context))) {
                    debug_layer = flags != 0;
                    std::printf("device: %ls (vendor 0x%04x, debug layer %s)%s\n", desc.Description, desc.VendorId,
                        debug_layer ? "on" : "off", want ? "" : " -- no NVIDIA adapter, first hardware adapter used");
                    return true;
                }
            }
    return false;
}
bool has_tess_roots(const Manifest& manifest)
{
    bool hull = false, domain = false;
    for (const auto& [hash, entry] : manifest.by_original) {
        hull |= entry.family == Family::TessHull;
        domain |= entry.family == Family::TessDomain;
    }
    return hull && domain;
}
}

// A crash inside the driver or the compiler would otherwise end the test with no output at all.
LONG WINAPI report_crash(EXCEPTION_POINTERS* info)
{
    HMODULE module{};
    char name[MAX_PATH]{};
    const auto* address = info && info->ExceptionRecord ? info->ExceptionRecord->ExceptionAddress : nullptr;
    if (address && GetModuleHandleExA(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            static_cast<LPCSTR>(address), &module))
        GetModuleFileNameA(module, name, MAX_PATH);
    std::fprintf(stderr, "CRASH 0x%08lx at %s+0x%llx\n", info && info->ExceptionRecord ? info->ExceptionRecord->ExceptionCode : 0ul,
        name[0] ? name : "?", static_cast<unsigned long long>(static_cast<const char*>(address) - reinterpret_cast<const char*>(module)));
    std::fflush(stderr);
    return EXCEPTION_CONTINUE_SEARCH;
}

int main()
{
    SetUnhandledExceptionFilter(&report_crash);
    std::setvbuf(stdout, nullptr, _IONBF, 0);
    try {
        // Diagnostics: RSF_TESS_RAW=<dxbc file> creates that one hull or domain shader on a fresh device and nothing else
        // (no compiler loaded), to tell a driver problem with the bytecode from a problem of this process.
        if (const char* raw = std::getenv("RSF_TESS_RAW")) {
            if (!create_device()) return 77;
            const auto code = file_bytes(raw);
            const std::string path = raw;
            if (path.find(".hs.") != std::string::npos) {
                ComPtr<ID3D11HullShader> s;
                hr(device->CreateHullShader(code.data(), code.size(), nullptr, &s), "raw HS");
            } else {
                ComPtr<ID3D11DomainShader> s;
                hr(device->CreateDomainShader(code.data(), code.size(), nullptr, &s), "raw DS");
            }
            context->Flush();
            Sleep(500);
            std::printf("raw create ok\n");
            return 0;
        }
        wchar_t buffer[32768]{};
        const DWORD length = GetEnvironmentVariableW(L"RSF_ANOMALY_MOTION_DEPLOY", buffer, 32768);
        const std::filesystem::path local = std::filesystem::path(RSF_SOURCE_DIR) / ".local/anomaly";
        deployment = length && length < 32768 ? std::filesystem::path(buffer)
            : std::filesystem::exists(local / "motion-deploy-tess" / "manifest.txt") ? local / "motion-deploy-tess"
            : local / "motion-deploy";
        if (!std::filesystem::exists(deployment / "manifest.txt")) {
            std::printf("SKIP: no local motion deployment at %s (run tools/anomaly_motion_deploy.py)\n", deployment.string().c_str());
            return 77;
        }
        const auto text = file_bytes(deployment / "manifest.txt");
        std::string error;
        auto manifest = Manifest::parse(std::string(reinterpret_cast<const char*>(text.data()), text.size()), error);
        require(manifest.has_value(), "manifest: " + error);
        if (!has_tess_roots(*manifest)) {
            std::printf("SKIP: %s has no dx11/tess.hs + dx11/tess.ds roots (regenerate it with tools/anomaly_motion_deploy.py)\n",
                deployment.string().c_str());
            return 77;
        }
        if (!create_device()) { std::printf("SKIP: no hardware D3D11 device\n"); return 77; }
        D3DShaderApi shader_api(L"d3dcompiler_43.dll");
        if (!shader_api.ready()) { std::printf("SKIP: d3dcompiler_43.dll not available\n"); return 77; }
        api = &shader_api;
        ShaderLoader shader_loader(*manifest, deployment, {}, shader_api, shader_api);
        loader = &shader_loader;
        std::printf("deployment: %s\n", deployment.string().c_str());
        // Every tessellation permutation in the user's shader cache (appdata/shaders_cache/r4: deffer_model_bump(tess_pn,)
        // _0.._4, deffer_model_bump(tess_pn,use_tdetail,use_tdetail_bump,)_2/_3 with the matching dx11\tess hs/ds and
        // deffer_base_bump ps). TESS_HM (displacement) is in no cached permutation, and the engine's own TESS_HM hull
        // shader (d3dcompiler_43 output, stock bytes) makes the NVIDIA driver crash in CreateHullShader in a few percent
        // of fresh processes (docs/research/anomaly-tess-motion.md), so the HM cases run only with RSF_TESS_HM=1.
        const std::vector<Define> pn{{"TESS_PN", "1"}};
        const std::vector<Define> detail{{"TESS_PN", "1"}, {"USE_TDETAIL", "1"}, {"USE_TDETAIL_BUMP", "1"}};
        const std::vector<Define> hm{{"TESS_PN", "1"}, {"TESS_HM", "1"}, {"USE_TDETAIL", "1"}};
        std::vector<Case> cases{{"SKIN_0 TESS_PN", 0, pn}, {"SKIN_1 TESS_PN", 1, pn}, {"SKIN_2 TESS_PN", 2, pn},
            {"SKIN_3 TESS_PN", 3, pn}, {"SKIN_4 TESS_PN", 4, pn}, {"SKIN_2 TESS_PN+USE_TDETAIL+USE_TDETAIL_BUMP", 2, detail},
            {"SKIN_3 TESS_PN+USE_TDETAIL+USE_TDETAIL_BUMP", 3, detail}};
        if (std::getenv("RSF_TESS_HM")) {
            cases.push_back({"SKIN_1 TESS_PN+TESS_HM+USE_TDETAIL", 1, hm});
            cases.push_back({"SKIN_2 TESS_PN+TESS_HM+USE_TDETAIL", 2, hm});
        }
        if (const char* which = std::getenv("RSF_TESS_CREATE")) {
            create_probe(Case{"SKIN_1 TESS_PN+TESS_HM+USE_TDETAIL", 1, hm}, which);
            return 0;
        }
        for (const auto& c : cases) {
            if (std::getenv("RSF_TESS_DIAG")) {
                try { run(c); } catch (const std::exception& e) { std::printf("  CASE FAILED %s\n", e.what()); }
            } else {
                run(c);
            }
        }
        std::printf("anomaly motion tess chain device: %d checks passed (debug layer %s)\n", checks, debug_layer ? "on, clean" : "off");
        return 0;
    } catch (const std::exception& error) {
        std::fprintf(stderr, "FAILED %s\n", error.what());
        return 1;
    }
}
