// SPDX-License-Identifier: GPL-3.0-only
// SR Stage 3 jitter against the real motion pixel shader writer on a D3D11 device (hardware with the debug layer when
// present, else WARP; 77 when neither exists). No game code and no engine shader runs.
//
// The pixel shader is games/anomaly/shaders/core/rsf_core_pixel.h's rsf_encode_motion, compiled unchanged; its PS b13 is
// packed by the production motion_constants.cpp pack_pixel from the production jitter state (sr_jitter.h: Halton
// sequence, row-vector projection jitter, published current/previous UV). A fixture VS writes SV_Position with this
// frame's jittered projection and the previous clip with the previous frame's jittered projection, as the core VS does
// with the history camera the motion stage records from RCache.
//   1. Static scene, jitter on, 18 consecutive frames (a whole sequence at scale 0.67): motion is zero on every pixel.
//   2. Same frames with the b13 jitter zeroed: motion equals the jitter difference (the fixture can see an error).
//   3. Camera pan of (+3, -2) render pixels between frames plus jitter: motion is exactly the pan.
#include "motion_constants.h"
#include "sr_jitter.h"

#include <d3d11.h>
#include <d3dcompiler.h>
#include <dxgi.h>
#include <wrl/client.h>

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

namespace {
using Microsoft::WRL::ComPtr;
using namespace rsf::anomaly;
using namespace rsf::anomaly::sr;
int checks = 0;
void require(bool ok, const std::string& message) {
    ++checks;
    if (!ok) throw std::runtime_error(message);
}
void hr(HRESULT value, const char* what) {
    if (FAILED(value)) throw std::runtime_error(std::string(what) + " failed: 0x" + std::to_string(static_cast<unsigned long>(value)));
}
float half(uint16_t h) {
    const uint32_t sign = (h & 0x8000u) << 16, exponent = (h >> 10) & 0x1f, mantissa = h & 0x3ff;
    if (!exponent) return (sign ? -1.0f : 1.0f) * std::ldexp(float(mantissa), -24);
    if (exponent == 31) return std::bit_cast<float>(sign | 0x7f800000u | (mantissa << 13));
    return std::bit_cast<float>(sign | ((exponent + 112) << 23) | (mantissa << 13));
}

ComPtr<ID3D11Device> device;
ComPtr<ID3D11DeviceContext> context;
ComPtr<ID3D11InfoQueue> info;
void debug_errors() {
    if (!info) return;
    const UINT64 count = info->GetNumStoredMessages();
    for (UINT64 i = 0; i < count; ++i) {
        SIZE_T size = 0;
        info->GetMessage(i, nullptr, &size);
        std::vector<char> storage(size);
        auto* message = reinterpret_cast<D3D11_MESSAGE*>(storage.data());
        info->GetMessage(i, message, &size);
        if (message->Severity <= D3D11_MESSAGE_SEVERITY_ERROR)
            throw std::runtime_error(std::string("debug layer: ") + std::string(message->pDescription, message->DescriptionByteLength));
    }
    info->ClearStoredMessages();
}

std::string file_text(const std::filesystem::path& path) {
    std::ifstream file(path, std::ios::binary);
    if (!file) throw std::runtime_error("cannot read " + path.string());
    std::stringstream text;
    text << file.rdbuf();
    return text.str();
}
ComPtr<ID3DBlob> compile(const std::string& source, const char* entry, const char* target) {
    ComPtr<ID3DBlob> code, errors;
    const HRESULT result = D3DCompile(source.data(), source.size(), "rsf_sr_jitter_fixture.hlsl", nullptr, nullptr, entry, target,
        D3DCOMPILE_ENABLE_STRICTNESS | D3DCOMPILE_OPTIMIZATION_LEVEL3, 0, &code, &errors);
    if (FAILED(result))
        throw std::runtime_error(std::string("compile ") + entry + ": " +
            (errors ? std::string(static_cast<const char*>(errors->GetBufferPointer()), errors->GetBufferSize()) : "no log"));
    return code;
}

// Fmatrix::build_projection(fov, aspect, near, far): LH, D3D depth, row vectors.
EngineMatrix perspective(float fov, float aspect, float near_plane = 0.2f, float far_plane = 1000.0f) {
    const float h = 1.0f / std::tan(fov * 0.5f), w = h / aspect, q = far_plane / (far_plane - near_plane);
    return {w, 0, 0, 0, 0, h, 0, 0, 0, 0, q, 1, 0, 0, -q * near_plane, 0};
}

constexpr uint32_t width = 96, height = 54;            // render size of 144x81 at scale 0.67
constexpr uint32_t output_width = 144, output_height = 81;

struct Fixture {
    ComPtr<ID3D11VertexShader> vs;
    ComPtr<ID3D11PixelShader> ps;
    ComPtr<ID3D11InputLayout> layout;
    ComPtr<ID3D11Buffer> vertices, matrices, pixel;
    ComPtr<ID3D11Texture2D> target, staging;
    ComPtr<ID3D11RenderTargetView> rtv;
    ComPtr<ID3D11RasterizerState> raster;
};

Fixture make_fixture(const std::filesystem::path& source_dir) {
    // The production writer, unchanged; the VS is a fixture standing in for the core VS's current/previous clip.
    const std::string source = std::string(R"(
cbuffer Fixture : register(b0)
{
    row_major float4x4 current_full;    // Fmatrix bytes: clip = v * M
    row_major float4x4 previous_full;
};
struct VSOut
{
    float4 position : SV_Position;
    float4 previous_clip : TEXCOORD12;
};
VSOut vs_main(float3 p : POSITION)
{
    VSOut o;
    o.position = mul(float4(p, 1), current_full);
    o.previous_clip = mul(float4(p, 1), previous_full);
    return o;
}
)") + file_text(source_dir / "games/anomaly/shaders/core/rsf_core_pixel.h") + R"(
float4 ps_main(VSOut i) : SV_Target0
{
    return rsf_encode_motion(i.position, i.previous_clip, 1);
}
)";
    Fixture f;
    const auto vs = compile(source, "vs_main", "vs_5_0");
    const auto ps = compile(source, "ps_main", "ps_5_0");
    hr(device->CreateVertexShader(vs->GetBufferPointer(), vs->GetBufferSize(), nullptr, &f.vs), "VS");
    hr(device->CreatePixelShader(ps->GetBufferPointer(), ps->GetBufferSize(), nullptr, &f.ps), "PS");
    const D3D11_INPUT_ELEMENT_DESC element{"POSITION", 0, DXGI_FORMAT_R32G32B32_FLOAT, 0, 0, D3D11_INPUT_PER_VERTEX_DATA, 0};
    hr(device->CreateInputLayout(&element, 1, vs->GetBufferPointer(), vs->GetBufferSize(), &f.layout), "layout");
    // A static plane z = 25 + 0.5 y (view space), tilted in depth, covering the whole view.
    const float quad[6][3]{{-50, -30, 10}, {-50, 30, 40}, {50, -30, 10}, {50, -30, 10}, {-50, 30, 40}, {50, 30, 40}};
    D3D11_BUFFER_DESC vb{sizeof(quad), D3D11_USAGE_DEFAULT, D3D11_BIND_VERTEX_BUFFER};
    D3D11_SUBRESOURCE_DATA init{quad};
    hr(device->CreateBuffer(&vb, &init, &f.vertices), "vertex buffer");
    D3D11_BUFFER_DESC cb{128, D3D11_USAGE_DEFAULT, D3D11_BIND_CONSTANT_BUFFER};
    hr(device->CreateBuffer(&cb, nullptr, &f.matrices), "matrices");
    cb.ByteWidth = motion::ps_bytes;
    hr(device->CreateBuffer(&cb, nullptr, &f.pixel), "PS b13");
    D3D11_TEXTURE2D_DESC td{};
    td.Width = width; td.Height = height; td.MipLevels = td.ArraySize = 1; td.Format = DXGI_FORMAT_R16G16B16A16_FLOAT;
    td.SampleDesc.Count = 1; td.BindFlags = D3D11_BIND_RENDER_TARGET;
    hr(device->CreateTexture2D(&td, nullptr, &f.target), "motion target");
    hr(device->CreateRenderTargetView(f.target.Get(), nullptr, &f.rtv), "rtv");
    td.BindFlags = 0; td.Usage = D3D11_USAGE_STAGING; td.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
    hr(device->CreateTexture2D(&td, nullptr, &f.staging), "staging");
    D3D11_RASTERIZER_DESC rd{};
    rd.FillMode = D3D11_FILL_SOLID; rd.CullMode = D3D11_CULL_NONE; rd.DepthClipEnable = TRUE;
    hr(device->CreateRasterizerState(&rd, &f.raster), "raster");
    return f;
}

struct Motion { float u, v, valid; };
std::vector<Motion> draw(Fixture& f, const EngineMatrix& current, const EngineMatrix& previous, const motion::PixelInputs& b13) {
    float matrices[32];
    std::memcpy(matrices, current.data(), 64);
    std::memcpy(matrices + 16, previous.data(), 64);
    context->UpdateSubresource(f.matrices.Get(), 0, nullptr, matrices, 0, 0);
    motion::PixelBytes bytes;
    require(motion::pack_pixel(b13, bytes), "pack_pixel");
    context->UpdateSubresource(f.pixel.Get(), 0, nullptr, bytes.data(), 0, 0);
    context->ClearState();
    const float clear[4]{-7, -7, -7, -7};
    context->ClearRenderTargetView(f.rtv.Get(), clear);
    context->IASetInputLayout(f.layout.Get());
    context->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
    const UINT stride = 12, offset = 0;
    ID3D11Buffer* vb = f.vertices.Get();
    context->IASetVertexBuffers(0, 1, &vb, &stride, &offset);
    ID3D11Buffer* b0 = f.matrices.Get();
    ID3D11Buffer* pixel_cb = f.pixel.Get();
    context->VSSetShader(f.vs.Get(), nullptr, 0);
    context->VSSetConstantBuffers(0, 1, &b0);
    context->PSSetShader(f.ps.Get(), nullptr, 0);
    context->PSSetConstantBuffers(13, 1, &pixel_cb);
    context->RSSetState(f.raster.Get());
    const D3D11_VIEWPORT viewport{0, 0, float(width), float(height), 0, 1};
    context->RSSetViewports(1, &viewport);
    ID3D11RenderTargetView* rtv = f.rtv.Get();
    context->OMSetRenderTargets(1, &rtv, nullptr);
    context->Draw(6, 0);
    context->CopyResource(f.staging.Get(), f.target.Get());
    D3D11_MAPPED_SUBRESOURCE mapped{};
    hr(context->Map(f.staging.Get(), 0, D3D11_MAP_READ, 0, &mapped), "map");
    std::vector<Motion> out(width * height);
    for (uint32_t y = 0; y < height; ++y) {
        const auto* row = reinterpret_cast<const uint16_t*>(static_cast<const std::byte*>(mapped.pData) + std::size_t(y) * mapped.RowPitch);
        for (uint32_t x = 0; x < width; ++x) out[y * width + x] = {half(row[4 * x]), half(row[4 * x + 1]), half(row[4 * x + 2])};
    }
    context->Unmap(f.staging.Get(), 0);
    debug_errors();
    return out;
}

void run(const std::filesystem::path& source_dir) {
    auto f = make_fixture(source_dir);
    const auto projection = perspective(1.2f, float(output_width) / float(output_height));
    const EngineMatrix view{1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1};
    JitterSequence sequence;
    JitterState state{};
    EngineMatrix previous_full{};
    JitterSample previous_sample{};
    double worst = 0, worst_control_error = 0, worst_pan_error = 0;
    uint64_t pixels = 0;
    constexpr double tolerance = 2e-4;   // UV; one render pixel is 1/96 = 1.04e-2
    for (int frame = 0; frame <= 18; ++frame) {
        const auto sample = sequence.next({width, height, output_width, output_height, 0}, true);
        require(sample.phase_count == 18, "18 phases at scale 0.67");
        state = next_state(state, false, true, sample, width, height, sequence.last_was_reset());
        const auto jittered = jitter_engine_projection(projection, sample.x, sample.y, width, height);
        require(jittered.has_value(), "jittered projection");
        const auto full = engine_mul(*jittered, view);
        if (frame > 0) {
            const auto uv = jitter_uv(state);
            require(uv.previous_u == previous_sample.x / float(width) && uv.previous_v == previous_sample.y / float(height), "previous UV");
            // 1. The motion stage's b13 for this frame: zero motion on the static scene.
            const auto cancelled = draw(f, full, previous_full,
                {0, 0, float(width), float(height), uv.current_u, uv.current_v, uv.previous_u, uv.previous_v});
            for (const auto& m : cancelled) {
                require(m.valid == 1.0f, "every pixel covered and valid");
                worst = std::fmax(worst, std::fmax(std::fabs(m.u), std::fabs(m.v)));
                ++pixels;
            }
            // 2. Control: without the jitter fields the static scene moves by the jitter difference.
            const auto raw = draw(f, full, previous_full, {0, 0, float(width), float(height), 0, 0, 0, 0});
            const double du = double(uv.current_u) - uv.previous_u, dv = double(uv.current_v) - uv.previous_v;
            require(std::fabs(du) > 5 * tolerance || std::fabs(dv) > 5 * tolerance, "consecutive samples differ visibly");
            for (const auto& m : raw)
                worst_control_error = std::fmax(worst_control_error, std::fmax(std::fabs(m.u - du), std::fabs(m.v - dv)));
            // 3. A camera pan of (+3, -2) render pixels (the previous image sat 3 px right and 2 px up): the jitter is
            // removed and the pan remains, motion = current - previous = (-3 / W, +2 / H).
            const auto panned = engine_mul(*jitter_engine_projection(projection, previous_sample.x + 3.0f, previous_sample.y - 2.0f,
                width, height), view);
            const auto pan = draw(f, full, panned, {0, 0, float(width), float(height), uv.current_u, uv.current_v, uv.previous_u, uv.previous_v});
            for (const auto& m : pan)
                worst_pan_error = std::fmax(worst_pan_error, std::fmax(std::fabs(m.u + 3.0 / width), std::fabs(m.v - 2.0 / height)));
        }
        previous_full = full;
        previous_sample = sample;
    }
    std::printf("static scene with jitter: %llu pixels over 18 frames, worst |motion| %.2e UV (%.4f render px)\n",
        static_cast<unsigned long long>(pixels), worst, worst * width);
    std::printf("control without b13 jitter: worst error vs the jitter difference %.2e UV; pan: worst error %.2e UV\n",
        worst_control_error, worst_pan_error);
    require(pixels == 18ull * width * height, "pixel count");
    require(worst < tolerance, "static scene under jitter has zero motion");
    require(worst_control_error < tolerance, "control shows exactly the jitter difference");
    require(worst_pan_error < tolerance, "pan survives the jitter removal");
}
}

int main() {
    try {
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
        if (debug) device.As(&info);
        ComPtr<IDXGIDevice> dxgi;
        ComPtr<IDXGIAdapter> adapter;
        DXGI_ADAPTER_DESC adapter_desc{};
        if (SUCCEEDED(device.As(&dxgi)) && SUCCEEDED(dxgi->GetAdapter(&adapter))) adapter->GetDesc(&adapter_desc);
        std::printf("device: %ls (%s, debug layer %s)\n", adapter_desc.Description, warp ? "WARP" : "hardware", debug ? "on" : "off");
        run(RSF_SOURCE_DIR);
        std::printf("anomaly sr jitter device: %d checks passed\n", checks);
        return 0;
    } catch (const std::exception& error) {
        std::fprintf(stderr, "FAILED %s\n", error.what());
        return 1;
    }
}
