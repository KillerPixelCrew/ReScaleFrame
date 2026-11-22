// SPDX-License-Identifier: MIT
// Motion MRT device fixture. Uses the LOCAL deployment from tools/anomaly_motion_deploy.py (licensed shader bodies,
// never tracked; RSF_ANOMALY_MOTION_DEPLOY or <repo>/.local/anomaly/motion-deploy). Without it the test exits 77.
//
// 1. Every allowlisted root compiles through the Rust deployed-tree loader with the engine's d3dcompiler_43: the stock
//    permutation is compiled from original/, handed in as "engine bytecode", reproduced byte for byte, and its motion
//    variant compiled from motion/ and reflection-checked. A one-byte change of the engine bytecode is refused.
// 2. deffer_base_flat VS/PS: the stock pair and the variant pair draw the same triangle into a real G-buffer
//    (RGBA16F position, RGBA8 albedo, D24S8) with the RGBA16F motion target from Rust motion owner as RTV2 and b13 from the
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
#include "motion_device_platform.h"
#include "motion_fixture_platform.h"
#include "anomaly_motion_native.h"
#include "motion_constants.h"
#include "shader_loader_platform.h"
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
#include <memory>
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

const rsf_anomaly_shader_d3d* api{};
const Manifest* admitted{};
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
    const bool compiled = build_deployed(*api, *admitted, deployment, request, built.stock, built.prepared, error);
    require(compiled, root.name + " stock: " + error);
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
    require(compile_deployed(*api, deployment / "original", request, stock, error) && !stock.empty(),
        "compile original before tampering");
    stock[stock.size() / 2] ^= std::byte{1};
    request.bytecode = stock;
    auto refused = prepare_deployed(*api, *admitted, deployment, request);
    require(refused.outcome != Outcome::Replaced && refused.variant.empty(), "tampered engine bytecode refused");
    // Valid bytecode of another permutation (other root, same stage) for this root's source: the recompile differs.
    const auto other = std::find_if(list.begin() + 1, list.end(), [&](const Root& r) { return r.stage == list.front().stage; });
    require(other != list.end(), "second root of the stage");
    auto foreign = build(*other, engine_defines(*other));
    request.bytecode = foreign.stock;
    refused = prepare_deployed(*api, *admitted, deployment, request);
    require(refused.outcome == Outcome::OriginalMismatch && refused.variant.empty(), "bytecode of another permutation refused");
    std::printf("roots compiled: %u variants through the deployed-tree loader (%zu roots)\n", replaced, list.size());
}

template<class T> std::vector<T> read_back(ID3D11Texture2D* texture)
{
    return fixture_readback<T>(device.Get(), context.Get(), texture, size);
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

using Bindings = std::unique_ptr<rsf_anomaly_motion_fixture_bindings, decltype(&rsf_anomaly_motion_fixture_bindings_drop)>;

struct Scene {
    ComPtr<ID3D11VertexShader> stock_vs, motion_vs;
    ComPtr<ID3D11PixelShader> stock_ps, motion_ps;
    ComPtr<ID3D11InputLayout> layout;
    ComPtr<ID3D11Buffer> near_vb, behind_vb, full_vb;
    UINT stride{};
    Bindings constants{rsf_anomaly_motion_fixture_bindings_new(), rsf_anomaly_motion_fixture_bindings_drop};
    FixtureTargets targets;
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
    const auto* error = rsf_anomaly_motion_fixture_bind_core(s.constants.get(), device.Get(), context.Get(), reflection, vertex);
    if (error) throw std::runtime_error(error);
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
    require(reflect_deployed(*api, vs.stock, stock_vs_reflection), "reflect stock VS");
    require(!linkable(stock_vs_reflection, ps.prepared.reflection), "stock VS cannot feed the motion PS");

    Scene s;
    hr(device->CreateVertexShader(vs.stock.data(), vs.stock.size(), nullptr, &s.stock_vs), "stock VS");
    hr(device->CreateVertexShader(vs.prepared.variant.data(), vs.prepared.variant.size(), nullptr, &s.motion_vs), "motion VS");
    hr(device->CreatePixelShader(ps.stock.data(), ps.stock.size(), nullptr, &s.stock_ps), "stock PS");
    hr(device->CreatePixelShader(ps.prepared.variant.data(), ps.prepared.variant.size(), nullptr, &s.motion_ps), "motion PS");
    auto vs_reflection = reflect(vs.stock);
    const auto signature = fixture_signature(vs_reflection.Get());
    s.layout = fixture_layout(device.Get(), signature, vs.stock);
    s.stride = static_cast<UINT>(signature.size()) * 16;
    s.full_vb = triangle(vs_reflection.Get(), -1, 1, 0.5f);
    s.near_vb = triangle(vs_reflection.Get(), -1, 0, 0.25f);
    s.behind_vb = triangle(vs_reflection.Get(), 0, 1, 0.75f);
    s.targets = FixtureTargets(device.Get(), size, true);

    std::unique_ptr<rsf_anomaly_motion_device_owner, decltype(&rsf_anomaly_motion_device_drop)> owner{
        rsf_anomaly_motion_device_new(rsf_anomaly_motion_device_platform_get()), rsf_anomaly_motion_device_drop};
    auto* motion = owner.get();
    require(rsf_anomaly_motion_device_create(motion, device.Get(), nullptr, nullptr), "Rust motion owner create");
    D3D11_TEXTURE2D_DESC position_desc{};
    s.targets.position->GetDesc(&position_desc);
    require(rsf_anomaly_motion_device_target(motion, position_desc.Width, position_desc.Height,
        position_desc.SampleDesc.Count, position_desc.ArraySize) == RSF_MOTION_CREATED, "motion target created");
    rsf_anomaly_motion_device_view motion_view{};
    rsf_anomaly_motion_device_get(motion, &motion_view);
    require(motion_view.width == size, "motion target follows RT0");
    require(rsf_anomaly_motion_device_target(motion, position_desc.Width, position_desc.Height,
        position_desc.SampleDesc.Count, position_desc.ArraySize) == RSF_MOTION_READY, "reused");
    auto ms = position_desc;
    ms.SampleDesc.Count = 4;
    require(rsf_anomaly_motion_device_target(motion, ms.Width, ms.Height,
        ms.SampleDesc.Count, ms.ArraySize) == RSF_MOTION_MULTISAMPLED, "MSAA refused");
    rsf_anomaly_motion_device_get(motion, &motion_view);

    const auto setup = [&](bool variant, ID3D11Buffer* vb, bool three) {
        context->ClearState();
        const rsf_anomaly_motion_fixture_input input{s.layout.Get(), vb, variant ? s.motion_vs.Get() : s.stock_vs.Get(),
            nullptr, nullptr, variant ? s.motion_ps.Get() : s.stock_ps.Get(), s.stride, false};
        rsf_anomaly_motion_fixture_input_bind(context.Get(), &input);
        bind_engine_constants(s, vs_reflection.Get(), true);
        bind_engine_constants(s, reflect(ps.stock).Get(), false);
        const rsf_anomaly_motion_fixture_output targets{s.targets.position_rtv.Get(), s.targets.albedo_rtv.Get(),
            three ? motion_view.rtv : nullptr, s.targets.dsv.Get(), s.targets.less.Get(), s.targets.raster.Get(), size};
        rsf_anomaly_motion_fixture_output_bind(context.Get(), &targets);
    };
    const auto clear = [&] {
        const rsf_anomaly_motion_fixture_output targets{s.targets.position_rtv.Get(), s.targets.albedo_rtv.Get(),
            motion_view.rtv, s.targets.dsv.Get(), s.targets.less.Get(), s.targets.raster.Get(), size};
        rsf_anomaly_motion_fixture_clear(context.Get(), &targets, true);
    };
    const auto upload = [&](bool valid) {
        temporal::Mat4 previous = temporal::Mat4::identity();
        previous.values[3] = 0.1;    // previous clip x = x + 0.1
        previous.values[7] = -0.2;   // previous clip y = y - 0.2
        CoreVertexBytes vbytes;
        require(pack_core_vertex(previous, {}, {valid, false, false}, vbytes), "pack VS b13");
        PixelBytes pbytes;
        require(pack_pixel({0, 0, float(size), float(size), 0, 0, 0, 0}, pbytes), "pack PS b13");
        require(rsf_anomaly_motion_device_upload(motion, context.Get(), RSF_MOTION_CORE, vbytes.data(), vbytes.size()) &&
            rsf_anomaly_motion_device_upload(motion, context.Get(), RSF_MOTION_PIXEL, pbytes.data(), pbytes.size()), "b13 upload");
    };

    // Stock reference.
    clear();
    setup(false, s.full_vb.Get(), false);
    rsf_anomaly_motion_fixture_draw(context.Get(), 6);
    const auto stock_position = read_back<Half4>(s.targets.position.Get());
    const auto stock_albedo = read_back<uint32_t>(s.targets.albedo.Get());
    debug_errors();
    // Variant with history.
    clear();
    setup(true, s.full_vb.Get(), true);
    upload(true);
    rsf_anomaly_motion_fixture_draw(context.Get(), 6);
    const auto position = read_back<Half4>(s.targets.position.Get());
    const auto albedo = read_back<uint32_t>(s.targets.albedo.Get());
    auto vectors = read_back<Half4>(static_cast<ID3D11Texture2D*>(motion_view.texture));
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
    rsf_anomaly_motion_fixture_draw(context.Get(), 6);
    auto stale = read_back<Half4>(static_cast<ID3D11Texture2D*>(motion_view.texture));
    require(half(stale[size / 2 * size + 4].c[2]) == 1, "fixture: a stock draw leaves the old vector (the reason for the replay)");
    const auto occluded_position = read_back<Half4>(s.targets.position.Get());
    const auto check_restored = [&] {
        ID3D11RenderTargetView* bound[3]{};
        ComPtr<ID3D11DepthStencilView> bound_depth;
        context->OMGetRenderTargets(3, bound, &bound_depth);
        ComPtr<ID3D11PixelShader> bound_ps;
        context->PSGetShader(&bound_ps, nullptr, nullptr);
        ComPtr<ID3D11DepthStencilState> bound_state;
        UINT reference = 0;
        context->OMGetDepthStencilState(&bound_state, &reference);
        const bool restored = bound[0] == s.targets.position_rtv.Get() && bound[1] == s.targets.albedo_rtv.Get() && bound[2] == static_cast<ID3D11RenderTargetView*>(motion_view.rtv) &&
            bound_depth.Get() == s.targets.dsv.Get() && bound_ps.Get() == s.stock_ps.Get() && bound_state.Get() == s.targets.less.Get() && reference == 0;
        for (auto* p : bound) if (p) p->Release();
        require(restored, "OM state and PS restored");
    };
    // Keep the C++ unwind on this frame while Rust owns the saved graphics state.
    const auto replay = [&](auto&& redraw) {
        rsf_anomaly_motion_replay_scope scope;
        const auto result = rsf_anomaly_motion_replay_begin(rsf_anomaly_motion_replay_platform_get(),
            &motion_view, context.Get(), &scope);
        if (result == RSF_MOTION_REPLAY_READY) {
            struct Restore {
                rsf_anomaly_motion_replay_scope* scope;
                ~Restore() { rsf_anomaly_motion_replay_end(scope); }
            } restore{&scope};
            redraw(context.Get());
        }
        return result;
    };
    struct ReplayAborted {};
    bool unwound = false;
    try {
        (void)replay([](ID3D11DeviceContext*) { throw ReplayAborted{}; });
    } catch (const ReplayAborted&) {
        unwound = true;
    }
    require(unwound, "replay callback unwound on the native frame");
    check_restored();
    require(replay([&](ID3D11DeviceContext* c) { c->Draw(6, 0); }) == RSF_MOTION_REPLAY_READY, "replay");
    check_restored();
    // Behind the surface: the occluder fails the depth test, so the replay's EQUAL test fails too.
    setup(false, s.behind_vb.Get(), true);
    rsf_anomaly_motion_fixture_draw(context.Get(), 6);
    require(replay([&](ID3D11DeviceContext* c) { c->Draw(6, 0); }) == RSF_MOTION_REPLAY_READY, "replay behind");
    vectors = read_back<Half4>(static_cast<ID3D11Texture2D*>(motion_view.texture));
    const auto after_position = read_back<Half4>(s.targets.position.Get());
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
    rsf_anomaly_motion_fixture_draw(context.Get(), 6);
    vectors = read_back<Half4>(static_cast<ID3D11Texture2D*>(motion_view.texture));
    const auto invalid_position = read_back<Half4>(s.targets.position.Get());
    for (UINT i = 0; i < size * size; ++i) {
        require(vectors[i].c[0] == 0 && vectors[i].c[1] == 0 && vectors[i].c[2] == 0 && vectors[i].c[3] == 0, "missing history is zero");
        require(std::memcmp(&invalid_position[i], &stock_position[i], sizeof(Half4)) == 0, "native unchanged without history");
    }
    debug_errors();
    rsf_anomaly_motion_device_get(motion, &motion_view);
    std::printf("draw: %u covered, %u invalidated by replay, %u kept, uploads=%llu skipped=%llu\n", covered, invalidated, kept,
        static_cast<unsigned long long>(motion_view.uploads), static_cast<unsigned long long>(motion_view.upload_skips));
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
        const bool still = root.name == "deffer_detail_s_flat.vs";
        const auto family = tree ? Family::FoliageTree : Family::FoliageDetail;
        const char* error = rsf_anomaly_motion_foliage_fixture_layout(reflection_view(built.prepared.stock),
            static_cast<uint8_t>(family), true, still);
        require(error == nullptr, root.name + ": " + (error ? error : "usable foliage layout"));
        if (tree) ++trees;
        else ++details;
        require(keeps_buffer_sizes(built.prepared.stock, built.prepared.reflection), root.name + " buffers kept");
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
    FixtureTargets targets;
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
    const auto signature = fixture_signature(r.vs_reflection.Get());
    r.layout = fixture_layout(device.Get(), signature, r.vs.stock);
    r.stride = static_cast<UINT>(signature.size()) * 16;
    r.targets = FixtureTargets(device.Get(), size, true);
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
void apply_overrides(ID3D11ShaderReflection* reflection, const Overrides& overrides, const Bindings& keep)
{
    std::vector<rsf_anomaly_motion_fixture_override> rows;
    rows.reserve(overrides.size());
    for (const auto& [name, values] : overrides) rows.push_back({name.c_str(), values.data(), values.size()});
    const auto* error = rsf_anomaly_motion_fixture_patch_core(
        keep.get(), device.Get(), context.Get(), reflection, rows.data(), rows.size());
    if (error) throw std::runtime_error(error);
}
struct FoliageDraw {
    std::vector<Half4> position, vectors;
    std::vector<uint32_t> albedo;
};
// Draws `vertex_buffers` (one quad each, 6 vertices) with the stock pair or the variant pair and `overrides` as the
// engine's VS constants. The variant draw uploads `vbytes` as VS b13 and a unit-viewport PS b13.
FoliageDraw draw_foliage_pair(FoliageRig& rig, rsf_anomaly_motion_device_owner* motion, bool variant, const Overrides& overrides,
    const std::vector<ComPtr<ID3D11Buffer>>& quads, const FoliageVertexBytes* vbytes)
{
    context->ClearState();
    rsf_anomaly_motion_device_view motion_view{};
    rsf_anomaly_motion_device_get(motion, &motion_view);
    const rsf_anomaly_motion_fixture_output targets{rig.targets.position_rtv.Get(), rig.targets.albedo_rtv.Get(),
        motion_view.rtv, rig.targets.dsv.Get(), rig.targets.less.Get(), rig.targets.raster.Get(), size};
    rsf_anomaly_motion_fixture_clear(context.Get(), &targets, true);
    const rsf_anomaly_motion_fixture_input input{rig.layout.Get(), nullptr, variant ? rig.motion_vs.Get() : rig.stock_vs.Get(),
        nullptr, nullptr, variant ? rig.motion_ps.Get() : rig.stock_ps.Get(), rig.stride, false};
    rsf_anomaly_motion_fixture_input_bind(context.Get(), &input);
    bind_engine_constants(rig.engine, rig.vs_reflection.Get(), true);
    const Bindings keep{rsf_anomaly_motion_fixture_bindings_new(), rsf_anomaly_motion_fixture_bindings_drop};
    apply_overrides(rig.vs_reflection.Get(), overrides, keep);
    bind_engine_constants(rig.engine, rig.ps_reflection.Get(), false);
    auto bound = targets;
    if (!variant) bound.motion = nullptr;
    rsf_anomaly_motion_fixture_output_bind(context.Get(), &bound);
    if (variant) {
        PixelBytes pbytes;
        require(pack_pixel({0, 0, float(size), float(size), 0, 0, 0, 0}, pbytes), "pack PS b13");
        require(vbytes && rsf_anomaly_motion_device_upload(motion, context.Get(), RSF_MOTION_FOLIAGE, vbytes->data(), vbytes->size()) &&
            rsf_anomaly_motion_device_upload(motion, context.Get(), RSF_MOTION_PIXEL, pbytes.data(), pbytes.size()), "foliage b13 upload");
    }
    for (const auto& quad : quads)
        rsf_anomaly_motion_fixture_draw_quad(context.Get(), quad.Get(), rig.stride);
    FoliageDraw out;
    out.position = read_back<Half4>(rig.targets.position.Get());
    out.albedo = read_back<uint32_t>(rig.targets.albedo.Get());
    if (variant) out.vectors = read_back<Half4>(static_cast<ID3D11Texture2D*>(motion_view.texture));
    debug_errors();
    return out;
}
float ndc_x(UINT i) { return (float(i) + 0.5f) / size * 2 - 1; }
float ndc_y(UINT j) { return 1 - (float(j) + 0.5f) / size * 2; }

void draw_tree()
{
    const Root vs_root{'v', "foliage_tree", "deffer_tree_flat.vs"}, ps_root{'p', "core_pixel", "deffer_base_flat.ps"};
    auto rig = make_rig(vs_root, ps_root);
    const char* layout_error = rsf_anomaly_motion_foliage_fixture_layout(reflection_view(rig.vs.prepared.stock),
        static_cast<uint8_t>(Family::FoliageTree), false, false);
    require(layout_error == nullptr, layout_error ? layout_error : "foliage layout");
    std::unique_ptr<rsf_anomaly_motion_device_owner, decltype(&rsf_anomaly_motion_device_drop)> owner{
        rsf_anomaly_motion_device_new(rsf_anomaly_motion_device_platform_get()), rsf_anomaly_motion_device_drop};
    auto* motion = owner.get();
    require(rsf_anomaly_motion_device_create(motion, device.Get(), nullptr, nullptr), "Rust motion owner create");
    D3D11_TEXTURE2D_DESC position_desc{};
    rig.targets.position->GetDesc(&position_desc);
    require(rsf_anomaly_motion_device_target(motion, position_desc.Width, position_desc.Height,
        position_desc.SampleDesc.Count, position_desc.ArraySize) == RSF_MOTION_CREATED, "motion target");
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
    const char* layout_error = rsf_anomaly_motion_foliage_fixture_layout(reflection_view(rig.vs.prepared.stock),
        static_cast<uint8_t>(Family::FoliageDetail), false, false);
    require(layout_error == nullptr, layout_error ? layout_error : "foliage layout");
    std::unique_ptr<rsf_anomaly_motion_device_owner, decltype(&rsf_anomaly_motion_device_drop)> owner{
        rsf_anomaly_motion_device_new(rsf_anomaly_motion_device_platform_get()), rsf_anomaly_motion_device_drop};
    auto* motion = owner.get();
    require(rsf_anomaly_motion_device_create(motion, device.Get(), nullptr, nullptr), "Rust motion owner create");
    D3D11_TEXTURE2D_DESC position_desc{};
    rig.targets.position->GetDesc(&position_desc);
    require(rsf_anomaly_motion_device_target(motion, position_desc.Width, position_desc.Height,
        position_desc.SampleDesc.Count, position_desc.ArraySize) == RSF_MOTION_CREATED, "motion target");

    struct Plant { float tx, scale, sun, hemi; };
    const Plant a{-0.5f, 1.0f, 0.3f, 0.7f}, b{0.5f, 1.0f, 0.2f, 0.6f};
    using DetailRows = std::array<float, foliage_detail_floats>;
    const auto group = [](const Plant& p, float scale) {
        return std::array<float, 16>{scale, 0, 0, p.tx, 0, scale, 0, 0, 0, 0, scale, 0.5f, p.sun, p.sun, p.sun, p.hemi};
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
    // The previous frame drew plant A before plant B. The current frame reverses the group order.
    const std::array previous_groups{group(a, 0.8f), group(b, 1.0f)};
    const std::array current_groups{group(b, 1.0f), group(a, 1.0f)};
    FoliageVertexBytes bytes;
    const auto upload_for = [&](bool include_b) {
        DetailRows current;
        const char* error = rsf_anomaly_motion_detail_fixture_bytes(size, include_b, &before, &now,
            previous_groups.front().data(), current_groups.front().data(), current.data(), bytes.data());
        require(error == nullptr, error ? error : "packed detail history");
        return current;
    };
    const std::vector<ComPtr<ID3D11Buffer>> quads{foliage_quad(rig, -0.5f, 0.5f, -1, 1, 0, 8192, 0),   // plant B (offset 0)
        foliage_quad(rig, -0.5f, 0.5f, -1, 1, 0, 8192, 4)};                                          // plant A (offset 4)
    // Quad B must land on the right half and quad A on the left: shift each by its own translation via the rows (tx = 0.5 / -0.5).
    const auto current = upload_for(true);
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
    const auto current2 = upload_for(false);
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

extern "C" bool rsf_anomaly_motion_fixture_default(ID3D11Device**, ID3D11DeviceContext**, int32_t*) noexcept;

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
        int32_t level{};
        if (!rsf_anomaly_motion_fixture_default(&device, &context, &level)) {
            std::printf("SKIP: no D3D11 device\n");
            return 77;
        }
        require(level >= static_cast<int32_t>(D3D_FEATURE_LEVEL_11_0), "feature level 11");
        // The engine's compiler (motion_profile.h); RSF_ANOMALY_MOTION_COMPILER overrides it for experiments.
        wchar_t compiler_name[MAX_PATH] = L"d3dcompiler_43.dll";
        GetEnvironmentVariableW(L"RSF_ANOMALY_MOTION_COMPILER", compiler_name, MAX_PATH);
        std::unique_ptr<rsf_anomaly_shader_d3d, decltype(&rsf_anomaly_shader_d3d_unload)> shader_api{
            rsf_anomaly_shader_d3d_load(reinterpret_cast<const uint16_t*>(compiler_name)), rsf_anomaly_shader_d3d_unload};
        if (!shader_api) { std::printf("SKIP: %ls not available\n", compiler_name); return 77; }
        std::printf("compiler: %ls\n", compiler_name);
        api = shader_api.get();
        const auto text = file_bytes(deployment / "manifest.txt");
        std::string error;
        auto manifest = Manifest::parse(std::string(reinterpret_cast<const char*>(text.data()), text.size()), error);
        require(manifest.has_value(), "manifest: " + error);
        admitted = &*manifest;
        const auto list = roots(deployment / "manifest.txt");
        require(list.size() == manifest->entries().size(), "manifest roots");
        const auto status = rsf_anomaly_motion_core_schedule(&list, [](const void* roots, uint32_t phase) noexcept {
            try {
                const auto& entries = *static_cast<const std::vector<Root>*>(roots);
                switch (phase) {
                case RSF_ANOMALY_MOTION_ROOTS: every_root(entries); break;
                case RSF_ANOMALY_MOTION_FOLIAGE_LAYOUTS: foliage_layouts(entries); break;
                case RSF_ANOMALY_MOTION_CORE_DRAW: draw_scene(); break;
                case RSF_ANOMALY_MOTION_TREE_DRAW: draw_tree(); break;
                case RSF_ANOMALY_MOTION_DETAIL_DRAW: draw_detail(); break;
                default: return false;
                }
                return true;
            } catch (const std::exception& error) {
                std::fprintf(stderr, "FAILED %s\n", error.what());
                return false;
            }
        });
        if (status != 0) return status;
        std::printf("anomaly motion device: %d checks passed\n", checks);
        return 0;
    } catch (const std::exception& error) {
        std::fprintf(stderr, "FAILED %s\n", error.what());
        return 1;
    }
}
