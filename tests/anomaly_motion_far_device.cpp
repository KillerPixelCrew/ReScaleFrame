// SPDX-License-Identifier: GPL-3.0-only
// Far skinned characters: the G-buffer VS/PS pairs X-Ray draws a skinned model with on either side of r__dtex_range.
// Uses the LOCAL deployment from tools/anomaly_motion_deploy.py (licensed shader bodies, never tracked):
// RSF_ANOMALY_MOTION_DEPLOY, else <repo>/.local/anomaly/motion-deploy. Exits 77 without it, without d3dcompiler_43 or
// without a hardware D3D11 device. Runs on the NVIDIA adapter (vendor 0x10de) when present, with the debug layer when
// it is installed.
//
// Engine facts this covers (docs/research/anomaly-far-character-motion.md):
// * CRender::rimp_select_sh_dynamic (xrRenderPC_R4/r4.cpp) picks SE_R2_NORMAL_HQ when distance minus the bounding
//   radius is below r__dtex_range (50 m), else SE_R2_NORMAL_LQ. CBlender_deffer_model / CBlender_Model_EbB compile both
//   with uber_deffer (xrRender/uber_deffer.cpp): HQ is deffer_model_bump-hq / _d-hq (or the tessellation chain), LQ is
//   deffer_model_bump / deffer_model_flat with deffer_base_bump / _aref_bump / _flat / _aref_flat. A character beyond
//   50 m is therefore drawn ONLY through the LQ pairs; nothing in the game captures so far measured one.
// * Every SKIN_n permutation of those roots is in the user's shader cache (appdata/shaders_cache/r4).
//
// What it proves, per pair and SKIN mode, through the production ShaderLoader (original reproduced byte for byte),
// linkable(), decide_core, the b13 packers and MotionDevice:
// 1. The variant pair links; the stock VS cannot feed the variant PS.
// 2. A skinned mover whose previous palette (SKIN_1..4) or bone-composed previous world (SKIN_0) is a rigid motion of the
//    current one, under a different previous camera: the motion target equals the CPU model at every covered pixel with
//    validity 1, and the native MRT0/MRT1/depth are bit-identical to the stock pair.
// 3. The far case: the same mover as a 0.6 x 1.8 m character 100 m away under a 10 degree (scope-like) perspective
//    projection, moved 5 cm sideways: a few pixels of coverage and a sub-pixel vector. The vector must still match the
//    CPU model to 0.02 px and every covered pixel must be valid (no loss to coverage or rounding).
// 4. Missing history writes all-zero motion (validity 0) and leaves the native output unchanged.
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
void require_clean_debug_layer(const std::string& what)
{
    ComPtr<ID3D11InfoQueue> queue;
    if (FAILED(device.As(&queue))) return;
    std::string first;
    std::uint64_t errors = 0;
    for (UINT64 i = 0, n = queue->GetNumStoredMessages(); i < n; ++i) {
        SIZE_T length = 0;
        queue->GetMessage(i, nullptr, &length);
        std::vector<char> storage(length);
        auto* message = reinterpret_cast<D3D11_MESSAGE*>(storage.data());
        if (FAILED(queue->GetMessage(i, message, &length)) || message->Severity > D3D11_MESSAGE_SEVERITY_ERROR) continue;
        if (!errors++) first.assign(message->pDescription, message->DescriptionByteLength);
    }
    queue->ClearStoredMessages();
    require(errors == 0, what + ": debug layer error: " + first);
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

struct Built {
    std::vector<std::byte> stock;
    Prepared prepared;
};
std::vector<Define> engine_defines() { return {{"SMAP_size", "2048"}, {"USE_BRANCHING", "1"}, {"USE_VTF", "1"}, {"SM_5", "1"}}; }
Built build(char stage, const std::string& root, const std::vector<Define>& defines)
{
    ShaderRequest request;
    request.target = stage == 'v' ? "vs_5_0" : "ps_5_0";
    request.entry = "main";
    request.flags = 8;   // D3D10_SHADER_PACK_MATRIX_ROW_MAJOR, what CResourceManager passes for every stage
    request.defines = defines;
    request.source = file_bytes(deployment / "original" / "r3" / root);
    Built built;
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

// ---- engine bindings: every matrix identity, everything else 0.1 unless overridden; RSF b13 skipped ----------------
using Overrides = std::map<std::string, std::vector<float>>;
struct Bindings {
    std::vector<ComPtr<ID3D11Buffer>> buffers;
    std::vector<ComPtr<ID3D11ShaderResourceView>> views;
    std::vector<ComPtr<ID3D11SamplerState>> samplers;
};
void bind_stage(bool vertex, const std::vector<std::byte>& code, const Overrides& overrides, Bindings& keep)
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
                if (std::string(vd.Name) == "def_aref" || std::string(vd.Name) == "alpha_ref") out[0] = 0.5f;
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
            if (vertex) context->VSSetConstantBuffers(binding.BindPoint, 1, &p);
            else context->PSSetConstantBuffers(binding.BindPoint, 1, &p);
            keep.buffers.push_back(buffer);
        } else if (binding.Type == D3D_SIT_TEXTURE) {
            D3D11_TEXTURE2D_DESC td{};
            td.Width = td.Height = 2; td.MipLevels = td.ArraySize = 1; td.Format = DXGI_FORMAT_R32G32B32A32_FLOAT;
            td.SampleDesc.Count = 1; td.BindFlags = D3D11_BIND_SHADER_RESOURCE;
            const float texels[16]{0.5f, 0.4f, 0.6f, 1, 0.5f, 0.4f, 0.6f, 1, 0.5f, 0.4f, 0.6f, 1, 0.5f, 0.4f, 0.6f, 1};
            D3D11_SUBRESOURCE_DATA init{texels, 32};
            ComPtr<ID3D11Texture2D> t;
            hr(device->CreateTexture2D(&td, &init, &t), "texture");
            ComPtr<ID3D11ShaderResourceView> srv;
            hr(device->CreateShaderResourceView(t.Get(), nullptr, &srv), "srv");
            ID3D11ShaderResourceView* p = srv.Get();
            if (vertex) context->VSSetShaderResources(binding.BindPoint, 1, &p);
            else context->PSSetShaderResources(binding.BindPoint, 1, &p);
            keep.views.push_back(srv);
        } else if (binding.Type == D3D_SIT_SAMPLER) {
            D3D11_SAMPLER_DESC sd{};
            sd.Filter = D3D11_FILTER_MIN_MAG_MIP_LINEAR;
            sd.AddressU = sd.AddressV = sd.AddressW = D3D11_TEXTURE_ADDRESS_WRAP;
            sd.MaxLOD = D3D11_FLOAT32_MAX;
            ComPtr<ID3D11SamplerState> sampler;
            hr(device->CreateSamplerState(&sd, &sampler), "sampler");
            ID3D11SamplerState* p = sampler.Get();
            if (vertex) context->VSSetSamplers(binding.BindPoint, 1, &p);
            else context->PSSetSamplers(binding.BindPoint, 1, &p);
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
// D3D left-handed perspective (column-vector form of D3DXMatrixPerspectiveFovLH), as X-Ray builds its projection.
temporal::Mat4 perspective(double fov_y, double near_plane, double far_plane)
{
    temporal::Mat4 m{};
    const double ys = 1.0 / std::tan(fov_y / 2);
    m.values[0] = ys; m.values[5] = ys;
    m.values[10] = far_plane / (far_plane - near_plane);
    m.values[11] = -near_plane * far_plane / (far_plane - near_plane);
    m.values[14] = 1;
    return m;
}
std::array<double, 4> mul4(const temporal::Mat4& m, const std::array<double, 4>& v)
{
    std::array<double, 4> out{};
    for (int r = 0; r < 4; ++r) for (int c = 0; c < 4; ++c) out[r] += m.values[r * 4 + c] * v[c];
    return out;
}
// Inverse of a perspective/affine 4x4 by Gauss-Jordan, in double.
temporal::Mat4 invert(const temporal::Mat4& m)
{
    double a[4][8]{};
    for (int r = 0; r < 4; ++r) { for (int c = 0; c < 4; ++c) a[r][c] = m.values[r * 4 + c]; a[r][4 + r] = 1; }
    for (int c = 0; c < 4; ++c) {
        int pivot = c;
        for (int r = c + 1; r < 4; ++r) if (std::abs(a[r][c]) > std::abs(a[pivot][c])) pivot = r;
        std::swap(a[c], a[pivot]);
        const double d = a[c][c];
        for (int k = 0; k < 8; ++k) a[c][k] /= d;
        for (int r = 0; r < 4; ++r) {
            if (r == c) continue;
            const double f = a[r][c];
            for (int k = 0; k < 8; ++k) a[r][k] -= f * a[c][k];
        }
    }
    temporal::Mat4 out{};
    for (int r = 0; r < 4; ++r) for (int c = 0; c < 4; ++c) out.values[r * 4 + c] = a[r][4 + c];
    return out;
}
Vec3 normalize(Vec3 v)
{
    const double l = std::sqrt(v[0] * v[0] + v[1] * v[1] + v[2] * v[2]);
    return {v[0] / l, v[1] / l, v[2] / l};
}
Vec3 cross(const Vec3& a, const Vec3& b) { return {a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]}; }
std::vector<float> floats(const temporal::Mat4& m)
{
    std::vector<float> out(16);
    for (int i = 0; i < 16; ++i) out[i] = float(m.values[i]);
    return out;
}

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

// ---- scenes ------------------------------------------------------------------------------------------------------
// Near: an octahedron under the identity (orthographic) camera, object = view = clip, as the tessellation fixture.
// Far: a 0.6 x 1.8 x 0.6 m octahedron 100 m away under a 10 degree perspective (a scoped view of a distant NPC).
struct Scene {
    const char* name;
    Vec3 centre;
    Vec3 radius;
    temporal::Mat4 projection;            // current camera: view identity, this projection
    temporal::Mat4 previous_bone;         // previous palette / bone-composed world relative to the identity current one
    CameraSample previous_camera;
    double tolerance_px;
    unsigned min_covered;
};
Scene near_scene()
{
    Scene s{"near", {0.0, 0.0, 0.5}, {0.6, 0.6, 0.3}, identity()};
    const auto to_origin = translation(-s.centre[0], -s.centre[1], -s.centre[2]);
    const auto back = translation(s.centre[0] + 0.04, s.centre[1] + 0.02, s.centre[2]);
    s.previous_bone = compose(back, compose(compose(rotation_y(0.35), rotation_x(-0.2)), to_origin));
    s.previous_camera.view = translation(0.03, -0.02, 0.0);
    s.previous_camera.projection = identity();
    s.previous_camera.projection.values[0] = 1.1;
    s.previous_camera.projection.values[5] = 0.9;
    s.previous_camera.projection.values[3] = -0.01;
    s.tolerance_px = 3e-4 * size;
    s.min_covered = 600;
    return s;
}
Scene far_scene()
{
    constexpr double pi = 3.14159265358979323846;
    Scene s{"far 100 m, 10 deg", {0.0, 0.0, 100.0}, {0.3, 0.9, 0.3}, perspective(10.0 * pi / 180.0, 0.2, 1000.0)};
    // Walking: 5 cm sideways and a slight turn of the body since the previous frame; the camera turned a little.
    const auto to_origin = translation(-s.centre[0], -s.centre[1], -s.centre[2]);
    const auto back = translation(s.centre[0] - 0.05, s.centre[1], s.centre[2]);
    s.previous_bone = compose(back, compose(rotation_y(0.1), to_origin));
    s.previous_camera.view = rotation_y(0.0004);
    s.previous_camera.projection = s.projection;
    s.tolerance_px = 0.02;
    s.min_covered = 6;
    return s;
}

struct Pair {
    std::string vs, ps;
    int skin;
    const char* element;   // "LQ" (beyond r__dtex_range) or "HQ"
};
struct Shaders {
    Built vs, ps;
    ComPtr<ID3D11VertexShader> stock_vs, motion_vs;
    ComPtr<ID3D11PixelShader> stock_ps, motion_ps;
    ComPtr<ID3D11InputLayout> layout;
};
Shaders make_shaders(const Pair& p)
{
    auto vs_defines = engine_defines();
    vs_defines.push_back({"SKIN_" + std::to_string(p.skin), "1"});
    Shaders s{build('v', p.vs, vs_defines), build('p', p.ps, engine_defines())};
    require(s.vs.prepared.skin == p.skin, p.vs + ": VS skin mode");
    require(s.vs.prepared.entry && s.vs.prepared.entry->family == Family::CoreModel, p.vs + ": core_model family");
    hr(device->CreateVertexShader(s.vs.stock.data(), s.vs.stock.size(), nullptr, &s.stock_vs), "stock VS");
    hr(device->CreatePixelShader(s.ps.stock.data(), s.ps.stock.size(), nullptr, &s.stock_ps), "stock PS");
    const auto& v = s.vs.prepared.variant;
    const auto& q = s.ps.prepared.variant;
    hr(device->CreateVertexShader(v.data(), v.size(), nullptr, &s.motion_vs), "motion VS");
    hr(device->CreatePixelShader(q.data(), q.size(), nullptr, &s.motion_ps), "motion PS");
    return s;
}

struct Mesh {
    ComPtr<ID3D11Buffer> vertices;
    UINT stride{}, count{};
};
// The octahedron as 8 triangles with outward normals. The engine builds its input layout against the STOCK signature.
Mesh make_mesh(Shaders& shaders, const Scene& scene, int skin)
{
    ComPtr<ID3D11ShaderReflection> reflection;
    hr(D3DReflect(shaders.vs.stock.data(), shaders.vs.stock.size(), IID_PPV_ARGS(&reflection)), "reflect VS");
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
    shaders.layout.Reset();
    hr(device->CreateInputLayout(elements.data(), static_cast<UINT>(elements.size()), shaders.vs.stock.data(),
        shaders.vs.stock.size(), &shaders.layout), "layout");
    Mesh mesh;
    mesh.stride = desc.InputParameters * 16;
    const Vec3 axes[6]{{1, 0, 0}, {-1, 0, 0}, {0, 1, 0}, {0, -1, 0}, {0, 0, 1}, {0, 0, -1}};
    std::vector<int> corners;
    for (int sx = 0; sx < 2; ++sx) for (int sy = 0; sy < 2; ++sy) for (int sz = 0; sz < 2; ++sz) {
        corners.push_back(sx); corners.push_back(2 + sy); corners.push_back(4 + sz);
    }
    mesh.count = static_cast<UINT>(corners.size());
    std::vector<float> data(mesh.count * desc.InputParameters * 4, 0.0f);
    // skin.h unpacks D3DCOLOR normals: the shader swizzles .zyx and maps [0, 1] to [-1, 1].
    const auto packed = [](const Vec3& n) { return std::array<float, 3>{float((n[2] + 1) / 2), float((n[1] + 1) / 2), float((n[0] + 1) / 2)}; };
    for (UINT v = 0; v < mesh.count; ++v) {
        const Vec3 n = axes[corners[v]];
        const Vec3 position{scene.centre[0] + scene.radius[0] * n[0], scene.centre[1] + scene.radius[1] * n[1],
            scene.centre[2] + scene.radius[2] * n[2]};
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
                out[0] = float(0.5 + 0.45 * n[0] + 0.2 * n[2]); out[1] = float(0.5 + 0.45 * n[1] - 0.2 * n[2]);
                out[2] = 0; out[3] = 3;
            }
        }
    }
    D3D11_BUFFER_DESC bd{};
    bd.ByteWidth = static_cast<UINT>(data.size() * 4);
    bd.BindFlags = D3D11_BIND_VERTEX_BUFFER;
    D3D11_SUBRESOURCE_DATA init{data.data()};
    hr(device->CreateBuffer(&bd, &init, &mesh.vertices), "vertex buffer");
    return mesh;
}

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

struct Frame {
    std::vector<Half4> position, motion;
    std::vector<uint32_t> albedo;
    std::vector<float> depth;
};
Frame draw(const Pair& p, const Scene& scene, Shaders& s, const Mesh& mesh, Targets& t, MotionDevice* motion, bool history)
{
    Bindings keep;
    context->ClearState();
    const float sentinel[4]{-999, -999, -999, -999}, zero[4]{};
    context->ClearRenderTargetView(t.position_rtv.Get(), sentinel);
    context->ClearRenderTargetView(t.albedo_rtv.Get(), zero);
    context->ClearDepthStencilView(t.dsv.Get(), D3D11_CLEAR_DEPTH, 1, 0);
    if (motion) motion->clear(context.Get());
    context->IASetInputLayout(s.layout.Get());
    context->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
    UINT offset = 0;
    ID3D11Buffer* vb = mesh.vertices.Get();
    context->IASetVertexBuffers(0, 1, &vb, &mesh.stride, &offset);
    context->VSSetShader(motion ? s.motion_vs.Get() : s.stock_vs.Get(), nullptr, 0);
    context->PSSetShader(motion ? s.motion_ps.Get() : s.stock_ps.Get(), nullptr, 0);
    // Current palette: every bone identity (rows). Current camera: view identity, the scene projection (m_WVP).
    std::vector<float> bones(234 * 4, 0.0f);
    for (int b = 0; b < 78; ++b) for (int r = 0; r < 3; ++r) bones[(b * 3 + r) * 4 + r] = 1.0f;
    const Overrides overrides{{"sbones_array", bones}, {"m_WVP", floats(scene.projection)}, {"dt_params", {1.0f, 1.0f, 0.0f, 0.0f}}};
    bind_stage(true, s.vs.stock, overrides, keep);
    bind_stage(false, s.ps.stock, overrides, keep);
    if (motion) {
        // The production policy, packers and MotionDevice uploads, as on_flush runs them for a swapped pair.
        temporal::InstanceSnapshot previous;
        previous.world = p.skin == 0 ? scene.previous_bone : identity();
        if (p.skin != 0) previous.bones.assign(78, scene.previous_bone);
        CoreDrawInput in;
        in.family = Family::CoreModel;
        in.skin = p.skin;
        in.in_visual = true;
        in.verdict = rsf::anomaly::geometry::DrawVerdict::Identified;
        in.kind = p.skin == 0 ? rsf::anomaly::geometry::DrawKind::SingleBone : rsf::anomaly::geometry::DrawKind::HardwareSkinned;
        in.context = true;
        in.previous_camera = &scene.previous_camera;
        in.previous = &previous;
        const auto decision = decide_core(in);
        require(decision.reason == HistoryReason::Valid, "policy accepts the skinned mover");
        CoreVertexBytes vbytes;
        if (history) require(pack_core_vertex(decision.previous_wvp, decision.bones, decision.flags, vbytes), "pack VS b13");
        else pack_core_invalid(vbytes);
        PixelBytes pbytes;
        require(pack_pixel({0, 0, float(size), float(size), 0, 0, 0, 0}, pbytes), "pack PS b13");
        require(motion->set_core_vertex(context.Get(), vbytes) && motion->set_pixel(context.Get(), pbytes), "b13 uploads");
    }
    ID3D11RenderTargetView* views[3]{t.position_rtv.Get(), t.albedo_rtv.Get(), motion ? motion->rtv() : nullptr};
    context->OMSetRenderTargets(motion ? 3 : 2, views, t.dsv.Get());
    context->OMSetDepthStencilState(t.less.Get(), 0);
    context->RSSetState(t.raster.Get());
    const D3D11_VIEWPORT viewport{0, 0, float(size), float(size), 0, 1};
    context->RSSetViewports(1, &viewport);
    context->Draw(mesh.count, 0);
    Frame frame;
    frame.position = read_back<Half4>(t.position.Get());
    frame.albedo = read_back<uint32_t>(t.albedo.Get());
    frame.depth = read_back<float>(t.depth.Get());
    if (motion) frame.motion = read_back<Half4>(motion->texture());
    require_clean_debug_layer(p.vs + "+" + p.ps + (motion ? " motion draw" : " stock draw"));
    return frame;
}

struct Result {
    unsigned covered{};
    double worst_px{}, largest_px{};
};
Result run(const Pair& p, Shaders& s, const Scene& scene)
{
    const std::string name = std::string(p.element) + " " + p.vs + "+" + p.ps + " SKIN_" + std::to_string(p.skin) + " " + scene.name;
    const auto mesh = make_mesh(s, scene, p.skin);
    auto targets = make_targets();
    MotionDevice motion;
    require(motion.create(device.Get(), nullptr, nullptr), "MotionDevice");
    D3D11_TEXTURE2D_DESC position_desc{};
    targets.position->GetDesc(&position_desc);
    require(motion.ensure_target(position_desc) == TargetResult::Created, "motion target");

    const auto stock = draw(p, scene, s, mesh, targets, nullptr, true);
    const auto moved = draw(p, scene, s, mesh, targets, &motion, true);
    // Current clip -> object (the current palette is identity, view identity): inverse of the projection. Previous clip of
    // that object point: previous projection * previous view * previous bone.
    const auto to_object = invert(scene.projection);
    const auto previous_clip_of = compose(scene.previous_camera.projection, compose(scene.previous_camera.view, scene.previous_bone));
    Result r;
    for (UINT y = 0; y < size; ++y)
        for (UINT x = 0; x < size; ++x) {
            const auto i = y * size + x;
            require(std::memcmp(&moved.position[i], &stock.position[i], sizeof(Half4)) == 0 && moved.albedo[i] == stock.albedo[i] &&
                std::bit_cast<uint32_t>(moved.depth[i]) == std::bit_cast<uint32_t>(stock.depth[i]),
                name + ": native MRT0/MRT1/depth bit-identical at " + std::to_string(i));
            const auto& m = moved.motion[i];
            if (half(stock.position[i].c[0]) == -999) {
                require(m.c[0] == 0 && m.c[1] == 0 && m.c[2] == 0 && m.c[3] == 0, name + ": uncovered pixel stays cleared");
                continue;
            }
            ++r.covered;
            const double u = (x + 0.5) / size, v = (y + 0.5) / size;
            auto object = mul4(to_object, {u * 2 - 1, 1 - v * 2, double(stock.depth[i]), 1});
            for (auto& c : object) c /= object[3];
            const auto prev = mul4(previous_clip_of, object);
            const double expected_x = u - (prev[0] / prev[3] * 0.5 + 0.5), expected_y = v - (prev[1] / prev[3] * -0.5 + 0.5);
            const double mx = half(m.c[0]), my = half(m.c[1]);
            require(half(m.c[2]) == 1 && m.c[3] == 0, name + ": validity 1 at " + std::to_string(i));
            const double error_px = std::max(std::abs(mx - expected_x), std::abs(my - expected_y)) * size;
            r.worst_px = std::max(r.worst_px, error_px);
            r.largest_px = std::max({r.largest_px, std::abs(expected_x) * size, std::abs(expected_y) * size});
            require(error_px < scene.tolerance_px, name + ": motion equals the CPU model at " + std::to_string(i) + " (measured " +
                std::to_string(mx * size) + "," + std::to_string(my * size) + " px, expected " + std::to_string(expected_x * size) +
                "," + std::to_string(expected_y * size) + " px)");
        }
    require(r.covered >= scene.min_covered, name + ": coverage " + std::to_string(r.covered));
    require(r.largest_px > 4 * scene.tolerance_px, name + ": the scene moves more than the tolerance");

    const auto none = draw(p, scene, s, mesh, targets, &motion, false);
    for (UINT i = 0; i < size * size; ++i) {
        const auto& m = none.motion[i];
        require(m.c[0] == 0 && m.c[1] == 0 && m.c[2] == 0 && m.c[3] == 0, name + ": missing history is zero");
        require(std::memcmp(&none.position[i], &stock.position[i], sizeof(Half4)) == 0, name + ": native unchanged without history");
    }
    std::printf("  %-74s %4u px covered, motion up to %.3f px, worst error %.4f px\n", name.c_str(), r.covered, r.largest_px, r.worst_px);
    return r;
}

bool create_device()
{
    ComPtr<IDXGIFactory1> factory;
    if (FAILED(CreateDXGIFactory1(IID_PPV_ARGS(&factory)))) return false;
    for (const UINT want : {0x10deu, 0u})
        for (const UINT flags : {UINT(D3D11_CREATE_DEVICE_DEBUG), 0u})
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
}

int main()
{
    std::setvbuf(stdout, nullptr, _IONBF, 0);
    try {
        wchar_t buffer[32768]{};
        const DWORD length = GetEnvironmentVariableW(L"RSF_ANOMALY_MOTION_DEPLOY", buffer, 32768);
        deployment = length && length < 32768 ? std::filesystem::path(buffer)
                                              : std::filesystem::path(RSF_SOURCE_DIR) / ".local/anomaly/motion-deploy";
        if (!std::filesystem::exists(deployment / "manifest.txt")) {
            std::printf("SKIP: no local motion deployment at %s (run tools/anomaly_motion_deploy.py)\n", deployment.string().c_str());
            return 77;
        }
        const auto text = file_bytes(deployment / "manifest.txt");
        std::string error;
        auto manifest = Manifest::parse(std::string(reinterpret_cast<const char*>(text.data()), text.size()), error);
        require(manifest.has_value(), "manifest: " + error);
        if (!create_device()) { std::printf("SKIP: no hardware D3D11 device\n"); return 77; }
        D3DShaderApi shader_api(L"d3dcompiler_43.dll");
        if (!shader_api.ready()) { std::printf("SKIP: d3dcompiler_43.dll not available\n"); return 77; }
        api = &shader_api;
        ShaderLoader shader_loader(*manifest, deployment, {}, shader_api, shader_api);
        loader = &shader_loader;
        std::printf("deployment: %s\n", deployment.string().c_str());
        // The element pairs uber_deffer builds for a deferred model (bump or flat base texture, with or without alpha
        // reference), restricted to the SKIN modes the user's shader cache holds for each VS root.
        std::vector<Pair> pairs;
        for (int skin = 0; skin <= 4; ++skin) {
            pairs.push_back({"deffer_model_bump.vs", "deffer_base_bump.ps", skin, "LQ"});
            pairs.push_back({"deffer_model_bump.vs", "deffer_base_aref_bump.ps", skin, "LQ"});
            pairs.push_back({"deffer_model_flat.vs", "deffer_base_flat.ps", skin, "LQ"});
            pairs.push_back({"deffer_model_flat.vs", "deffer_base_aref_flat.ps", skin, "LQ"});
            pairs.push_back({"deffer_model_bump-hq.vs", "deffer_base_bump-hq.ps", skin, "HQ"});
            pairs.push_back({"deffer_model_bump-hq.vs", "deffer_base_steep-hq.ps", skin, "HQ"});
            pairs.push_back({"deffer_model_bump-hq.vs", "deffer_base_aref_bump-hq.ps", skin, "HQ"});
        }
        for (int skin = 0; skin <= 2; ++skin) {
            pairs.push_back({"deffer_model_bump_d-hq.vs", "deffer_base_bump_d-hq.ps", skin, "HQ"});
            pairs.push_back({"deffer_model_bump_d-hq.vs", "deffer_base_bump_db-hq.ps", skin, "HQ"});
        }
        const auto near_case = near_scene();
        const auto far_case = far_scene();
        unsigned lq = 0;
        double far_worst = 0;
        for (const auto& p : pairs) {
            auto shaders = make_shaders(p);
            require(linkable(shaders.vs.prepared.reflection, shaders.ps.prepared.reflection), p.vs + "+" + p.ps + ": the variant pair links");
            require(!linkable(reflection_of(shaders.vs.stock), shaders.ps.prepared.reflection), p.vs + ": stock VS cannot feed the motion PS");
            run(p, shaders, near_case);
            far_worst = std::max(far_worst, run(p, shaders, far_case).worst_px);
            if (std::string(p.element) == "LQ") ++lq;
        }
        std::printf("anomaly motion far device: %zu pairs (%u LQ), far worst error %.4f px, %d checks passed (debug layer %s)\n",
            pairs.size(), lq, far_worst, checks, debug_layer ? "on, clean" : "off");
        return 0;
    } catch (const std::exception& error) {
        std::fprintf(stderr, "FAILED %s\n", error.what());
        return 1;
    }
}
