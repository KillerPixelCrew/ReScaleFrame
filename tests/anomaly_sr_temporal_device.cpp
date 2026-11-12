// SPDX-License-Identifier: GPL-3.0-only
// SR Stages 4 and 5 on a D3D11 device, through the adapter's production pieces.
//
//   decode    The Runtime graphics library's motion decode and dense resolve (motion_decode.cpp, motion_resolve.cpp) with
//             the adapter's exact parameters (sr_pass.h motion_decode_params, motion_to_uv, motion_camera_included = 0,
//             the camera matrices build_pass hands over): (0,0,1,0) decodes to a valid zero vector and stays zero through
//             the resolve; (0,0,0,0) and any validity <= 0.5 decode to the sentinel and are reconstructed from depth and
//             the camera; written vectors replace the reconstruction. The same texels with the stored-zero rule
//             (zero_means_unwritten, no validity) show why that rule cannot be used for this motion target.
//   prepare   UpscalePass::prepare and ::reinsert (shaders/sr/rsf_sr_upscale.hlsl): f^-1 of the FP16 twin, the motion
//             mapping with the first-person rule on and off, and the stock curve applied to a reconstruction.
//   provider  <vendor root> (or RSF_SR_VENDOR_ROOT): the package layout <root>/{streamline,fidelityfx,xess}. The adapter's
//             TemporalDriver binds the Runtime's rsf_get_d3d11_sr_api by name (as a carrier-loaded adapter does), starts
//             the shared pipeline on an NVIDIA adapter at the first boundary, and each backend (DLSS, FSR3, FSR4, XeSS)
//             evaluates a jittered synthetic scene through the same sequence as the seam: make_temporal_targets, prepare,
//             build_pass, the Runtime evaluate, reinsert into an engine-like UNORM8 generic0. 77 without a root or an
//             NVIDIA adapter (the pipeline only starts where DLSS is supported). FSR4 refused on non-RDNA4 hardware is
//             reported, not failed.
#include "sr_d3d11.h"
#include "sr_pass.h"
#include "sr_temporal.h"
#include "sr_tonemap.h"
#include <rescaleframe/motion_decode.h>
#include <rescaleframe/motion_resolve.h>

#include <d3d11.h>
#include <d3d11sdklayers.h>
#include <dxgi.h>
#include <wrl/client.h>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <limits>
#include <string>
#include <algorithm>
#include <array>
#include <string_view>
#include <vector>

namespace {
using namespace rsf::anomaly::sr;
using Microsoft::WRL::ComPtr;
int failures = 0;
#define CHECK(condition) do { if (!(condition)) { std::printf("FAIL %s:%d %s\n", __FILE__, __LINE__, #condition); ++failures; } } while (0)
bool close_to(double a, double b, double tolerance) { return std::fabs(a - b) <= tolerance; }

float half_to_float(uint16_t h) {
    const uint32_t sign = uint32_t(h & 0x8000) << 16, exponent = (h >> 10) & 0x1f, mantissa = h & 0x3ff;
    uint32_t bits;
    if (exponent == 0x1f) bits = sign | 0x7f800000 | (mantissa << 13);
    else if (exponent) bits = sign | ((exponent + 112) << 23) | (mantissa << 13);
    else if (!mantissa) bits = sign;
    else {
        int e = -1;
        uint32_t m = mantissa;
        do { ++e; m <<= 1; } while (!(m & 0x400));
        bits = sign | uint32_t(112 - e) << 23 | ((m & 0x3ff) << 13);
    }
    float value;
    std::memcpy(&value, &bits, sizeof(value));
    return value;
}
uint16_t float_to_half(float value) {
    uint32_t bits;
    std::memcpy(&bits, &value, sizeof(bits));
    const uint16_t sign = uint16_t((bits >> 16) & 0x8000);
    const uint32_t magnitude = bits & 0x7fffffff;
    if (magnitude >= 0x7f800000) return uint16_t(sign | 0x7c00 | (magnitude > 0x7f800000 ? 0x200 : 0));
    if (magnitude >= 0x477ff000) return uint16_t(sign | 0x7c00);
    if (magnitude < 0x38800000) return uint16_t(sign | uint16_t(std::nearbyint(std::fabs(value) * 16777216.0f)));
    const uint32_t rebased = magnitude - (112u << 23);
    return uint16_t(sign | ((rebased + 0xfff + ((rebased >> 13) & 1)) >> 13));
}
uint32_t d24(double depth, uint32_t stencil = 0) { return uint32_t(std::lround(depth * 16777215.0)) | (stencil << 24); }

void log_line(void*, const char* message) { std::printf("  | %s\n", message); }

struct Device {
    ComPtr<ID3D11Device> device;
    ComPtr<ID3D11DeviceContext> context;
    std::wstring name;
    uint32_t vendor{};
    bool create(uint32_t want_vendor, bool debug) {
        ComPtr<IDXGIFactory1> factory;
        if (FAILED(CreateDXGIFactory1(IID_PPV_ARGS(&factory)))) return false;
        const D3D_FEATURE_LEVEL levels[] = {D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0};
        for (UINT i = 0;; ++i) {
            ComPtr<IDXGIAdapter1> adapter;
            if (factory->EnumAdapters1(i, &adapter) == DXGI_ERROR_NOT_FOUND) break;
            DXGI_ADAPTER_DESC1 desc{};
            adapter->GetDesc1(&desc);
            if ((desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE) || (want_vendor && desc.VendorId != want_vendor)) continue;
            for (const UINT flags : {debug ? UINT(D3D11_CREATE_DEVICE_DEBUG) : 0u, 0u}) {
                if (SUCCEEDED(D3D11CreateDevice(adapter.Get(), D3D_DRIVER_TYPE_UNKNOWN, nullptr, flags, levels, 2, D3D11_SDK_VERSION,
                        &device, nullptr, &context))) {
                    name = desc.Description;
                    vendor = desc.VendorId;
                    std::printf("device: %ls (vendor 0x%04x, debug layer %s)\n", desc.Description, desc.VendorId, flags ? "on" : "off");
                    return true;
                }
            }
        }
        return false;
    }
};
ComPtr<ID3D11Texture2D> texture(ID3D11Device* device, uint32_t w, uint32_t h, DXGI_FORMAT format, UINT bind, const void* data = nullptr,
    UINT pitch = 0) {
    D3D11_TEXTURE2D_DESC desc{w, h, 1, 1, format, {1, 0}, D3D11_USAGE_DEFAULT, bind, 0, 0};
    D3D11_SUBRESOURCE_DATA initial{data, pitch, 0};
    ComPtr<ID3D11Texture2D> result;
    device->CreateTexture2D(&desc, data ? &initial : nullptr, &result);
    return result;
}
ComPtr<ID3D11ShaderResourceView> srv(ID3D11Device* device, ID3D11Texture2D* t, DXGI_FORMAT format = DXGI_FORMAT_UNKNOWN) {
    ComPtr<ID3D11ShaderResourceView> view;
    D3D11_SHADER_RESOURCE_VIEW_DESC desc{};
    desc.Format = format;
    desc.ViewDimension = D3D11_SRV_DIMENSION_TEXTURE2D;
    desc.Texture2D.MipLevels = 1;
    if (t) device->CreateShaderResourceView(t, format == DXGI_FORMAT_UNKNOWN ? nullptr : &desc, &view);
    return view;
}
ComPtr<ID3D11RenderTargetView> rtv(ID3D11Device* device, ID3D11Texture2D* t) {
    ComPtr<ID3D11RenderTargetView> view;
    if (t) device->CreateRenderTargetView(t, nullptr, &view);
    return view;
}
// A depth texture like the adapter's twin: R24G8_TYPELESS with a D24S8 DSV and an R24_UNORM_X8 SRV, filled from data
// (a depth-stencil resource takes no initial data, so it is copied from a shader-only twin of the same format).
struct Depth {
    ComPtr<ID3D11Texture2D> texture;
    ComPtr<ID3D11DepthStencilView> dsv;
    ComPtr<ID3D11ShaderResourceView> srv;
};
Depth depth_texture(Device& d, uint32_t w, uint32_t h, const std::vector<uint32_t>& values) {
    Depth out;
    auto staging = texture(d.device.Get(), w, h, DXGI_FORMAT_R24G8_TYPELESS, D3D11_BIND_SHADER_RESOURCE, values.data(), w * 4);
    out.texture = texture(d.device.Get(), w, h, DXGI_FORMAT_R24G8_TYPELESS, D3D11_BIND_DEPTH_STENCIL | D3D11_BIND_SHADER_RESOURCE);
    if (!staging || !out.texture) return {};
    d.context->CopyResource(out.texture.Get(), staging.Get());
    D3D11_DEPTH_STENCIL_VIEW_DESC dsv{};
    dsv.Format = DXGI_FORMAT_D24_UNORM_S8_UINT;
    dsv.ViewDimension = D3D11_DSV_DIMENSION_TEXTURE2D;
    d.device->CreateDepthStencilView(out.texture.Get(), &dsv, &out.dsv);
    out.srv = srv(d.device.Get(), out.texture.Get(), DXGI_FORMAT_R24_UNORM_X8_TYPELESS);
    return out;
}
template<class T> std::vector<T> read_back(Device& d, ID3D11Texture2D* source, uint32_t texel_values) {
    D3D11_TEXTURE2D_DESC desc{};
    source->GetDesc(&desc);
    desc.Usage = D3D11_USAGE_STAGING; desc.BindFlags = 0; desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ; desc.MiscFlags = 0;
    ComPtr<ID3D11Texture2D> staging;
    std::vector<T> out(std::size_t(desc.Width) * desc.Height * texel_values);
    if (FAILED(d.device->CreateTexture2D(&desc, nullptr, &staging))) return {};
    d.context->CopyResource(staging.Get(), source);
    D3D11_MAPPED_SUBRESOURCE mapped{};
    if (FAILED(d.context->Map(staging.Get(), 0, D3D11_MAP_READ, 0, &mapped))) return {};
    for (uint32_t y = 0; y < desc.Height; ++y)
        std::memcpy(&out[std::size_t(y) * desc.Width * texel_values], static_cast<const uint8_t*>(mapped.pData) + std::size_t(y) * mapped.RowPitch,
            std::size_t(desc.Width) * texel_values * sizeof(T));
    d.context->Unmap(staging.Get(), 0);
    return out;
}

// X-Ray build_projection_HAT and a camera at (px, 0, 0) looking down +Z.
Matrix projection(float fov = 1.2f, float aspect = 16.0f / 9.0f, float n = 0.2f, float f = 900.0f) {
    const float h = 1.0f / std::tan(fov * 0.5f), w = h / aspect, q = f / (f - n);
    return {w, 0, 0, 0, 0, h, 0, 0, 0, 0, q, 1, 0, 0, -q * n, 0};
}
Matrix view_at(float px) { return {1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, -px, 0, 0, 1}; }

// ---- decode ----------------------------------------------------------------------------------------------------------
void decode_case(Device& d) {
    constexpr uint32_t W = 8, H = 2;
    // Row 0 and row 1 hold the same texels.
    const float stored[W][4] = {{0, 0, 1, 0}, {0.01f, -0.02f, 1, 0}, {0, 0, 0, 0}, {0.3f, 0.3f, 0, 0},
                                {0, 0, 0.5f, 0}, {0, 0, 0.75f, 0}, {0.5f, 0.25f, 1, 0}, {0, 0, 0.25f, 0}};
    std::vector<uint16_t> halves(W * H * 4);
    for (uint32_t y = 0; y < H; ++y)
        for (uint32_t x = 0; x < W; ++x)
            for (int c = 0; c < 4; ++c) halves[(y * W + x) * 4 + c] = float_to_half(stored[x][c]);
    auto source = texture(d.device.Get(), W, H, DXGI_FORMAT_R16G16B16A16_FLOAT, D3D11_BIND_SHADER_RESOURCE, halves.data(), W * 8);
    CHECK(source != nullptr);
    rsf_motion_decode_setup setup{sizeof(setup), RSF_MOTION_DECODE_ABI_VERSION, W, H, DXGI_FORMAT_R32G32_FLOAT, nullptr, nullptr};
    rsf_motion_decode* decode = nullptr;
    CHECK(rsf_motion_decode_create(d.device.Get(), &setup, &decode) == RSF_MOTION_DECODE_OK && decode);
    if (!decode || !source) return;
    const auto params = motion_decode_params();
    CHECK(rsf_motion_decode_run(decode, d.context.Get(), source.Get(), &params) == RSF_MOTION_DECODE_OK);
    auto decoded = read_back<float>(d, static_cast<ID3D11Texture2D*>(rsf_motion_decode_texture(decode)), 2);
    CHECK(decoded.size() == W * H * 2);
    uint32_t valid_zero = 0, sentinels = 0;
    for (uint32_t x = 0; x < W && decoded.size() == W * H * 2; ++x) {
        float as_half[4];
        for (int c = 0; c < 4; ++c) as_half[c] = half_to_float(halves[x * 4 + c]);
        const auto expected = decode_reference(as_half);
        const bool match = decoded[x * 2] == expected.x && decoded[x * 2 + 1] == expected.y;
        if (!match) std::printf("  texel %u: decoded (%g, %g), expected (%g, %g)\n", x, decoded[x * 2], decoded[x * 2 + 1], expected.x, expected.y);
        CHECK(match);
        valid_zero += !expected.unwritten && expected.x == 0 && expected.y == 0;
        sentinels += expected.unwritten;
    }
    CHECK(valid_zero == 2 && sentinels == 4);   // (0,0,1,0) and (0,0,0.75,0) valid zero; validity 0 (twice), 0.25, 0.5 unwritten
    std::printf("decode: (0,0,1,0) -> (%g, %g) valid; (0,0,0,0) -> (%g, %g) sentinel\n", decoded[0], decoded[1], decoded[4], decoded[5]);
    // The stored-zero rule on the same texels: the valid stationary (0,0,1,0) is misread as unwritten.
    auto legacy = params;
    legacy.validity_enabled = 0;
    legacy.zero_means_unwritten = 1;
    CHECK(rsf_motion_decode_run(decode, d.context.Get(), source.Get(), &legacy) == RSF_MOTION_DECODE_OK);
    const auto legacy_decoded = read_back<float>(d, static_cast<ID3D11Texture2D*>(rsf_motion_decode_texture(decode)), 2);
    CHECK(legacy_decoded.size() == W * H * 2 && legacy_decoded[0] == motion_sentinel &&
        legacy_decoded[6] == half_to_float(float_to_half(0.3f)));   // validity 0 but nonzero motion: taken as written
    // V1 callers (shorter struct, no validity fields) keep the stored-zero rule.
    auto v1 = legacy;
    v1.struct_size = RSF_MOTION_DECODE_PARAMS_SIZE_V1;
    CHECK(rsf_motion_decode_run(decode, d.context.Get(), source.Get(), &v1) == RSF_MOTION_DECODE_OK);

    // Dense resolve, as the pipeline runs it for these frames (has_sentinel from the validity rule, decoded UV scaled to
    // pixels by -render size, the clip-to-previous matrix from build_pass). The camera moved 0.5 m along -X.
    CHECK(rsf_motion_decode_run(decode, d.context.Get(), source.Get(), &params) == RSF_MOTION_DECODE_OK);
    std::vector<float> depth_values(W * H, 0.9f);
    auto depth = texture(d.device.Get(), W, H, DXGI_FORMAT_R32_FLOAT, D3D11_BIND_SHADER_RESOURCE, depth_values.data(), W * 4);
    PassInputs in{};
    in.render_width = W; in.render_height = H; in.output_width = W; in.output_height = H;
    in.current = {view_at(0.0f), projection(1.2f, float(W) / H), true};
    in.previous = {view_at(0.5f), projection(1.2f, float(W) / H), true};
    in.jitter.applied = true; in.jitter.render_width = W; in.jitter.render_height = H;
    int resources[4];
    in.color = &resources[0]; in.color_output = &resources[1]; in.depth = &resources[2]; in.motion = &resources[3];
    rsf_game_render_pass pass;
    CHECK(build_pass(in, pass) == PassError::None);
    rsf_motion_resolve* resolve = nullptr;
    CHECK(rsf_motion_resolve_create(d.device.Get(), W, H, &resolve) && resolve);
    if (!resolve || !depth) { rsf_motion_decode_destroy(decode); return; }
    rsf_motion_resolve_params r{};
    r.struct_size = sizeof(r);
    std::memcpy(r.clip_to_previous, pass.camera.clip_to_previous_clip, sizeof(r.clip_to_previous));
    r.decoded_to_pixels[0] = -float(W) * pass.motion_to_uv[0];
    r.decoded_to_pixels[1] = -float(H) * pass.motion_to_uv[1];
    r.sentinel = params.invalid_value;
    r.has_sentinel = pass.motion_camera_included == 0 ? 1u : 0u;
    CHECK(rsf_motion_resolve_run(resolve, d.context.Get(), rsf_motion_decode_texture(decode), depth.Get(), &r));
    const auto resolved = read_back<uint16_t>(d, static_cast<ID3D11Texture2D*>(rsf_motion_resolve_motion(resolve)), 2);
    CHECK(resolved.size() == W * H * 2);
    for (uint32_t y = 0; y < H && resolved.size() == W * H * 2; ++y)
        for (uint32_t x = 0; x < W; ++x) {
            const float mx = half_to_float(resolved[(y * W + x) * 2]), my = half_to_float(resolved[(y * W + x) * 2 + 1]);
            const auto expected = decode_reference(stored[x]);
            double ex, ey;
            if (!expected.unwritten) { ex = -double(expected.x) * W; ey = -double(expected.y) * H; }   // written: replaces, previous-minus-current pixels
            else {
                const double ndc[2]{(x + 0.5) / W * 2 - 1, 1 - (y + 0.5) / H * 2};
                double prev[4]{};
                const double v[4]{ndc[0], ndc[1], 0.9, 1};
                for (int c = 0; c < 4; ++c) for (int k = 0; k < 4; ++k) prev[c] += v[k] * pass.camera.clip_to_previous_clip[k * 4 + c];
                ex = (prev[0] / prev[3] - ndc[0]) * W * 0.5;
                ey = (prev[1] / prev[3] - ndc[1]) * H * -0.5;
            }
            const bool ok = close_to(mx, ex, 2e-3 * std::max(1.0, std::fabs(ex))) && close_to(my, ey, 2e-3 * std::max(1.0, std::fabs(ey)));
            if (!ok) std::printf("  resolve (%u,%u): (%g, %g), expected (%g, %g)\n", x, y, mx, my, ex, ey);
            CHECK(ok);
            if (x == 0) CHECK(mx == 0 && my == 0);                 // valid stationary: no camera motion added
            if (x == 2) CHECK(std::fabs(mx) > 0.05);               // unwritten: camera reconstruction
        }
    std::printf("resolve: valid (0,0,1,0) -> (%g, %g) px; unwritten (0,0,0,0) -> (%g, %g) px from the camera\n",
        half_to_float(resolved[0]), half_to_float(resolved[1]), half_to_float(resolved[4]), half_to_float(resolved[5]));
    rsf_motion_resolve_destroy(resolve);
    rsf_motion_decode_destroy(decode);
}

// ---- thin static geometry and a mover, through the production decode and dense resolve -----------------------------
// A jittered, moving-camera frame of a thin pole (about 0.75 render px wide) in front of ground and sky, and an NPC-like
// slab that moves by itself. Every pixel is rasterized at its jittered sample: depth is the R24G8 device depth of the
// surface the jittered ray hits. The pole and, in one run, the mover carry written vectors (validity 1), exactly what the
// core PS writes (current-minus-previous unjittered UV); ground, sky and, in the other run, the mover are unwritten
// (0,0,0,0) and are reconstructed from depth and the camera. The decode and resolve run with the adapter's parameters,
// as dlss_pipeline.cpp calls them. Checks against the true previous-minus-current unjittered displacement:
//   - written pixels (pole, mover) match it, and the pole/background step equals the true parallax step, so the
//     camera reconstruction next to a thin static object does not disagree with the vectors on it (no edge shimmer
//     source in the motion field);
//   - ground and sky are reconstructed within 0.05 px (the pipeline omits the current jitter; with it, 0.01 px);
//   - an unwritten mover is reconstructed as if static and misses its own motion: the ghost of an invalid character.
namespace thin {
constexpr uint32_t W = 96, H = 48;
constexpr double fov = 1.2, near_plane = 0.2, far_plane = 900.0;
// Row-vector X-Ray matrices (sr_pass.h convention).
Matrix proj() { return projection(float(fov), float(W) / H, float(near_plane), float(far_plane)); }
// Camera at c with yaw a about +Y (looking down +Z when a = 0): world_to_view = T(-c) * Ry(-a).
Matrix view(double cx, double cy, double cz, double a) {
    const double c = std::cos(a), s = std::sin(a);
    // Rows are row-vector basis: v_view = (v - c) * R, R = rotation by -a about Y.
    const double r[3][3]{{c, 0, s}, {0, 1, 0}, {-s, 0, c}};
    Matrix m{};
    for (int i = 0; i < 3; ++i) for (int j = 0; j < 3; ++j) m[i * 4 + j] = float(r[i][j]);
    for (int j = 0; j < 3; ++j) m[12 + j] = float(-(cx * r[0][j] + cy * r[1][j] + cz * r[2][j]));
    m[15] = 1;
    return m;
}
std::array<double, 4> mul(const std::array<double, 4>& v, const Matrix& m) {
    std::array<double, 4> out{};
    for (int c = 0; c < 4; ++c) for (int k = 0; k < 4; ++k) out[c] += v[k] * m[k * 4 + c];
    return out;
}
// Unjittered pixel (x right, y down) of a world point.
std::array<double, 2> to_pixel(const std::array<double, 4>& world, const Matrix& v, const Matrix& p) {
    const auto clip = mul(mul(world, v), p);
    return {(clip[0] / clip[3] * 0.5 + 0.5) * W, (0.5 - clip[1] / clip[3] * 0.5) * H};
}
enum Surface : uint8_t { Sky, Ground, Pole, Mover };
struct Texel { Surface surface{}; double depth{}; double truth[2]{}; };
// The current camera is the world origin looking down +Z, so view space is world space.
std::vector<Texel> rasterize(double jx, double jy, const Matrix& previous_view, double mover_shift) {
    const Matrix p = proj();
    const double h = 1.0 / std::tan(fov / 2), w = h / (double(W) / H), q = far_plane / (far_plane - near_plane);
    std::vector<Texel> out(W * H);
    for (uint32_t y = 0; y < H; ++y)
        for (uint32_t x = 0; x < W; ++x) {
            // The jittered raster sample at this pixel centre sees the unjittered position centre - jitter.
            const double ux = x + 0.5 - jx, uy = y + 0.5 - jy;
            const double dir[3]{(ux / W * 2 - 1) / w, (1 - uy / H * 2) / h, 1};
            Texel t{Sky, 1.0, {}};
            double best = 1e30;
            const auto take = [&](Surface s, double z) { if (z > 0 && z < best) { best = z; t.surface = s; } };
            if (dir[1] < 0) take(Ground, -1.6 / dir[1]);
            { const double z = 14, px = dir[0] * z, py = dir[1] * z; if (std::fabs(px - 0.9) < 0.15 && py > -1.6 && py < 6) take(Pole, z); }
            { const double z = 6, px = dir[0] * z, py = dir[1] * z; if (std::fabs(px + 1.2) < 0.5 && py > -1.6 && py < 1.8) take(Mover, z); }
            std::array<double, 4> point{dir[0] * 1e6, dir[1] * 1e6, 1e6, 1};
            if (t.surface != Sky) {
                t.depth = q - q * near_plane / best;
                point = {dir[0] * best, dir[1] * best, best, 1};
            }
            if (t.surface == Mover) point[0] -= mover_shift;   // where this material point was last frame
            const auto previous = to_pixel(point, previous_view, p);
            t.truth[0] = previous[0] - ux;
            t.truth[1] = previous[1] - uy;
            out[y * W + x] = t;
        }
    return out;
}
struct Run { std::vector<float> resolved; bool ok{}; };
Run resolve_frame(Device& d, const std::vector<Texel>& texels, const PassInputs& in, bool write_mover, bool pass_jitter) {
    Run run;
    std::vector<uint16_t> motion(W * H * 4, 0);
    std::vector<uint32_t> depth(W * H);
    for (uint32_t i = 0; i < W * H; ++i) {
        const auto& t = texels[i];
        depth[i] = d24(t.depth);
        if (t.surface == Pole || (t.surface == Mover && write_mover)) {
            // The core PS output: current-minus-previous unjittered UV, Y down, validity 1.
            motion[i * 4 + 0] = float_to_half(float(-t.truth[0] / W));
            motion[i * 4 + 1] = float_to_half(float(-t.truth[1] / H));
            motion[i * 4 + 2] = float_to_half(1.0f);
        }
    }
    auto source = texture(d.device.Get(), W, H, DXGI_FORMAT_R16G16B16A16_FLOAT, D3D11_BIND_SHADER_RESOURCE, motion.data(), W * 8);
    const auto depth_twin = depth_texture(d, W, H, depth);
    rsf_motion_decode_setup setup{sizeof(setup), RSF_MOTION_DECODE_ABI_VERSION, W, H, DXGI_FORMAT_R32G32_FLOAT, nullptr, nullptr};
    rsf_motion_decode* decode = nullptr;
    rsf_motion_resolve* resolve = nullptr;
    rsf_game_render_pass pass;
    const bool built = build_pass(in, pass) == PassError::None;
    CHECK(built);
    if (source && depth_twin.texture && built && rsf_motion_decode_create(d.device.Get(), &setup, &decode) == RSF_MOTION_DECODE_OK &&
        rsf_motion_resolve_create(d.device.Get(), W, H, &resolve)) {
        const auto params = motion_decode_params();
        rsf_motion_resolve_params r{};
        r.struct_size = sizeof(r);
        std::memcpy(r.clip_to_previous, pass.camera.clip_to_previous_clip, sizeof(r.clip_to_previous));
        r.decoded_to_pixels[0] = -float(W) * pass.motion_to_uv[0];
        r.decoded_to_pixels[1] = -float(H) * pass.motion_to_uv[1];
        r.sentinel = params.invalid_value;
        r.has_sentinel = pass.motion_camera_included == 0 ? 1u : 0u;
        if (pass_jitter) { r.jitter_pixels[0] = pass.camera.jitter_pixels[0]; r.jitter_pixels[1] = pass.camera.jitter_pixels[1]; }
        if (rsf_motion_decode_run(decode, d.context.Get(), source.Get(), &params) == RSF_MOTION_DECODE_OK &&
            rsf_motion_resolve_run(resolve, d.context.Get(), rsf_motion_decode_texture(decode), depth_twin.texture.Get(), &r)) {
            const auto halves = read_back<uint16_t>(d, static_cast<ID3D11Texture2D*>(rsf_motion_resolve_motion(resolve)), 2);
            run.resolved.resize(halves.size());
            for (std::size_t i = 0; i < halves.size(); ++i) run.resolved[i] = half_to_float(halves[i]);
            run.ok = run.resolved.size() == W * H * 2;
        }
    }
    if (resolve) rsf_motion_resolve_destroy(resolve);
    if (decode) rsf_motion_decode_destroy(decode);
    CHECK(run.ok);
    return run;
}
}

void thin_case(Device& d) {
    using namespace thin;
    // The camera moved 0.35 m and turned 0.6 degrees since the previous frame; jitter is mid-sequence.
    const double jx = 0.3125, jy = -0.2037037;
    const Matrix previous_view = view(-0.35, 0.02, -0.1, -0.0105);
    PassInputs in{};
    in.render_width = in.output_width = W;
    in.render_height = in.output_height = H;
    in.current = {view(0, 0, 0, 0), proj(), true};
    in.previous = {previous_view, proj(), true};
    in.jitter.applied = in.jitter.previous_applied = true;
    in.jitter.render_width = W; in.jitter.render_height = H;
    in.jitter.current_x = float(jx); in.jitter.current_y = float(jy);
    in.jitter.previous_x = -0.4375f; in.jitter.previous_y = 0.2592593f;
    int resources[4];
    in.color = &resources[0]; in.color_output = &resources[1]; in.depth = &resources[2]; in.motion = &resources[3];
    const double mover_shift = 0.6;   // metres, along +X, between the frames
    const auto texels = rasterize(jx, jy, previous_view, mover_shift);
    uint32_t counts[4]{};
    for (const auto& t : texels) ++counts[t.surface];
    std::printf("thin: %ux%u sky=%u ground=%u pole=%u mover=%u\n", W, H, counts[Sky], counts[Ground], counts[Pole], counts[Mover]);
    CHECK(counts[Sky] > 100 && counts[Ground] > 100 && counts[Pole] >= 12 && counts[Pole] <= 2 * H && counts[Mover] > 50);
    for (const bool pass_jitter : {false, true}) {
        const auto written = resolve_frame(d, texels, in, true, pass_jitter);
        const auto unwritten = resolve_frame(d, texels, in, false, pass_jitter);
        if (!written.ok || !unwritten.ok) return;
        double worst[4]{}, worst_step = 0, mover_miss_min = 1e9;
        uint32_t steps = 0;
        for (uint32_t y = 0; y < H; ++y)
            for (uint32_t x = 0; x < W; ++x) {
                const uint32_t i = y * W + x;
                const auto& t = texels[i];
                const double ex = written.resolved[i * 2] - t.truth[0], ey = written.resolved[i * 2 + 1] - t.truth[1];
                worst[t.surface] = std::max(worst[t.surface], std::hypot(ex, ey));
                if (t.surface == Mover) {
                    const double mx = unwritten.resolved[i * 2] - t.truth[0], my = unwritten.resolved[i * 2 + 1] - t.truth[1];
                    mover_miss_min = std::min(mover_miss_min, std::hypot(mx, my));
                }
                // A pole pixel and its background neighbours: the resolved step must be the true step.
                if (t.surface == Pole)
                    for (const int dx : {-1, 1}) {
                        const int nx = int(x) + dx;
                        if (nx < 0 || nx >= int(W) || texels[y * W + nx].surface == Pole || texels[y * W + nx].surface == Mover) continue;
                        const uint32_t j = y * W + uint32_t(nx);
                        const auto& n = texels[j];
                        const double sx = (written.resolved[i * 2] - written.resolved[j * 2]) - (t.truth[0] - n.truth[0]);
                        const double sy = (written.resolved[i * 2 + 1] - written.resolved[j * 2 + 1]) - (t.truth[1] - n.truth[1]);
                        worst_step = std::max(worst_step, std::hypot(sx, sy));
                        ++steps;
                    }
            }
        std::printf("thin (%s): worst error px pole=%.4f mover=%.4f ground=%.4f sky=%.4f; pole edge step error %.4f px over %u edges; "
            "unwritten mover misses by >= %.2f px\n", pass_jitter ? "resolve given the jitter" : "pipeline: resolve without jitter",
            worst[Pole], worst[Mover], worst[Ground], worst[Sky], worst_step, steps, mover_miss_min);
        CHECK(worst[Pole] < 0.01 && worst[Mover] < 0.01);
        const double background = pass_jitter ? 0.01 : 0.05;
        CHECK(worst[Ground] < background && worst[Sky] < background + 0.02);
        // Pole neighbours include sky, whose reconstruction at the far plane (not infinity) is off by about 0.02 px.
        CHECK(steps > 20 && worst_step < 0.03);
        CHECK(mover_miss_min > 2.0);
    }
}

// ---- prepare / reinsert ----------------------------------------------------------------------------------------------
void prepare_case(Device& d) {
    constexpr uint32_t W = 16, H = 8, OW = 24, OH = 12;
    UpscalePass pass;
    CHECK(pass.create(d.device.Get(), log_line, nullptr) && pass.temporal_ready());
    TemporalTargets targets;
    CHECK(make_temporal_targets(d.device.Get(), W, H, OW, OH, DXGI_FORMAT_R16G16B16A16_FLOAT, targets) == TwinError::None);
    CHECK(targets.matches(d.device.Get(), W, H, OW, OH, DXGI_FORMAT_R16G16B16A16_FLOAT) &&
        !targets.matches(d.device.Get(), W + 1, H, OW, OH, DXGI_FORMAT_R16G16B16A16_FLOAT));
    // Colour twin in the engine's low space, motion target, depth twin (first person in the left quarter).
    std::vector<uint16_t> colour(W * H * 4), motion(W * H * 4);
    std::vector<uint32_t> depth(W * H);
    std::vector<double> linear(W * H);
    for (uint32_t y = 0; y < H; ++y)
        for (uint32_t x = 0; x < W; ++x) {
            const uint32_t i = y * W + x;
            linear[i] = 0.05 + 0.4 * x + 0.3 * y;   // reaches above W = 1.7
            for (int c = 0; c < 3; ++c) colour[i * 4 + c] = float_to_half(float(tonemap_forward(linear[i] * (1 + 0.1 * c))));
            colour[i * 4 + 3] = float_to_half(0.25f);   // no UNORM8 rounding tie
            const bool valid = y < H / 2;
            motion[i * 4 + 0] = float_to_half(valid ? 0.001f * x : 0.3f);
            motion[i * 4 + 1] = float_to_half(valid ? -0.002f * y : 0.3f);
            motion[i * 4 + 2] = float_to_half(valid ? 1.0f : 0.0f);
            motion[i * 4 + 3] = 0;
            depth[i] = d24(x < W / 4 ? 0.01 : 0.6);
        }
    auto twin = texture(d.device.Get(), W, H, DXGI_FORMAT_R16G16B16A16_FLOAT, D3D11_BIND_SHADER_RESOURCE | D3D11_BIND_RENDER_TARGET, colour.data(), W * 8);
    auto motion_target = texture(d.device.Get(), W, H, DXGI_FORMAT_R16G16B16A16_FLOAT, D3D11_BIND_SHADER_RESOURCE | D3D11_BIND_RENDER_TARGET, motion.data(), W * 8);
    auto depth_twin = depth_texture(d, W, H, depth);
    CHECK(twin && motion_target && depth_twin.srv);
    if (!twin || !motion_target || !depth_twin.srv) return;
    auto twin_srv = srv(d.device.Get(), twin.Get());
    auto motion_srv = srv(d.device.Get(), motion_target.Get());
    for (const bool first_person : {true, false}) {
        PrepareViews views{twin_srv.Get(), motion_srv.Get(), depth_twin.srv.Get(), W, H, targets.colour_rtv.Get(), targets.motion_rtv.Get(),
            first_person_depth_limit, first_person};
        CHECK(pass.prepare(d.context.Get(), views));
        const auto out_colour = read_back<uint16_t>(d, targets.colour.Get(), 4);
        const auto out_motion = read_back<uint16_t>(d, targets.motion.Get(), 4);
        CHECK(out_colour.size() == W * H * 4 && out_motion.size() == W * H * 4);
        if (out_colour.size() != W * H * 4 || out_motion.size() != W * H * 4) return;
        double worst = 0;
        uint32_t motion_errors = 0;
        for (uint32_t i = 0; i < W * H; ++i) {
            for (int c = 0; c < 3; ++c) {
                const double x = tonemap_inverse(half_to_float(colour[i * 4 + c]));
                worst = std::max(worst, std::fabs(half_to_float(out_colour[i * 4 + c]) - x) / std::max(x, 1e-3));
            }
            const uint32_t x = i % W, y = i / W;
            const bool valid = y < H / 2, fp = x < W / 4;
            float ex = 0, ey = 0, ez = 0;
            if (valid) { ex = half_to_float(motion[i * 4]); ey = half_to_float(motion[i * 4 + 1]); ez = 1; }
            else if (first_person && fp) ez = 1;
            if (half_to_float(out_motion[i * 4]) != ex || half_to_float(out_motion[i * 4 + 1]) != ey || half_to_float(out_motion[i * 4 + 2]) != ez ||
                out_motion[i * 4 + 3] != 0)
                ++motion_errors;
        }
        std::printf("prepare (first person %s): colour f^-1 worst relative error %.2e, motion mismatches %u of %u\n", first_person ? "on" : "off",
            worst, motion_errors, W * H);
        CHECK(worst < 2e-3 && motion_errors == 0);
    }
    // Reinsert: the reconstruction (linear, output size) through the stock curve into UNORM8 generic0, nearest position
    // and depth, bilinear generic1, exactly as the Stage 2 pass for everything but the colour.
    std::vector<uint16_t> reconstructed(OW * OH * 4);
    for (uint32_t i = 0; i < OW * OH; ++i) {
        const float x = 0.02f + 0.11f * (i % OW) + 0.05f * (i / OW);
        for (int c = 0; c < 3; ++c) reconstructed[i * 4 + c] = float_to_half(x);
        reconstructed[i * 4 + 3] = float_to_half(1.0f);
    }
    auto sr_output = texture(d.device.Get(), OW, OH, DXGI_FORMAT_R16G16B16A16_FLOAT, D3D11_BIND_SHADER_RESOURCE, reconstructed.data(), OW * 8);
    auto sr_view = srv(d.device.Get(), sr_output.Get());
    std::vector<uint16_t> position(W * H * 4, float_to_half(3.0f));
    auto position_twin = texture(d.device.Get(), W, H, DXGI_FORMAT_R16G16B16A16_FLOAT, D3D11_BIND_SHADER_RESOURCE, position.data(), W * 8);
    std::vector<uint32_t> distort(W * H, 0x80808080u);
    auto distort_twin = texture(d.device.Get(), W, H, DXGI_FORMAT_R8G8B8A8_UNORM, D3D11_BIND_SHADER_RESOURCE, distort.data(), W * 4);
    auto generic0 = texture(d.device.Get(), OW, OH, DXGI_FORMAT_R8G8B8A8_UNORM, D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE);
    auto engine_position = texture(d.device.Get(), OW, OH, DXGI_FORMAT_R16G16B16A16_FLOAT, D3D11_BIND_RENDER_TARGET);
    auto generic1 = texture(d.device.Get(), OW, OH, DXGI_FORMAT_R8G8B8A8_UNORM, D3D11_BIND_RENDER_TARGET);
    auto engine_depth = depth_texture(d, OW, OH, std::vector<uint32_t>(OW * OH, d24(1.0)));
    auto g0 = rtv(d.device.Get(), generic0.Get()), pos = rtv(d.device.Get(), engine_position.Get()), g1 = rtv(d.device.Get(), generic1.Get());
    auto pos_srv = srv(d.device.Get(), position_twin.Get()), dist_srv = srv(d.device.Get(), distort_twin.Get());
    const UpscaleViews uv{twin_srv.Get(), pos_srv.Get(), dist_srv.Get(), depth_twin.srv.Get(), W, H, g0.Get(), pos.Get(), g1.Get(),
        engine_depth.dsv.Get(), OW, OH};
    CHECK(pass.reinsert(d.context.Get(), uv, sr_view.Get()));
    CHECK(!pass.reinsert(d.context.Get(), uv, nullptr));
    const auto out = read_back<uint8_t>(d, generic0.Get(), 4);
    uint32_t worst_code = 0, alpha_errors = 0;
    for (uint32_t i = 0; i < OW * OH && out.size() == OW * OH * 4; ++i) {
        const int expected = unorm8(tonemap_forward(half_to_float(reconstructed[i * 4])));
        worst_code = std::max<uint32_t>(worst_code, uint32_t(std::abs(int(out[i * 4]) - expected)));
        alpha_errors += out[i * 4 + 3] != unorm8(0.25);
    }
    const auto out_position = read_back<uint16_t>(d, engine_position.Get(), 4);
    CHECK(out_position.size() == OW * OH * 4 && half_to_float(out_position[0]) == 3.0f);
    std::printf("reinsert: worst UNORM8 difference %u against saturate(f(x)), alpha mismatches %u\n", worst_code, alpha_errors);
    CHECK(out.size() == OW * OH * 4 && worst_code <= 1 && alpha_errors == 0);
}

// ---- provider ----------------------------------------------------------------------------------------------------------
struct Scene {
    uint32_t w{}, h{};
    ComPtr<ID3D11Texture2D> twin, motion, position, distort;
    ComPtr<ID3D11ShaderResourceView> twin_srv, motion_srv, position_srv, distort_srv;
    Depth depth;
};
// Exposure-scaled linear radiance of a static scene at continuous image position (u, v) in [0, 1].
double radiance(double u, double v) {
    return 0.15 + 0.35 * (0.5 + 0.5 * std::sin(u * 9.0)) * (0.5 + 0.5 * std::cos(v * 7.0)) + 0.6 * u * v;
}
bool make_scene(Device& d, uint32_t w, uint32_t h, Scene& s) {
    s = {};
    s.w = w; s.h = h;
    s.twin = texture(d.device.Get(), w, h, DXGI_FORMAT_R16G16B16A16_FLOAT, D3D11_BIND_SHADER_RESOURCE | D3D11_BIND_RENDER_TARGET);
    // Motion: valid zero (a static scene, static camera) in the top half, unwritten (0,0,0,0) below.
    std::vector<uint16_t> motion(std::size_t(w) * h * 4, 0);
    for (uint32_t y = 0; y < h / 2; ++y) for (uint32_t x = 0; x < w; ++x) motion[(std::size_t(y) * w + x) * 4 + 2] = float_to_half(1.0f);
    s.motion = texture(d.device.Get(), w, h, DXGI_FORMAT_R16G16B16A16_FLOAT, D3D11_BIND_SHADER_RESOURCE | D3D11_BIND_RENDER_TARGET, motion.data(), w * 8);
    std::vector<uint16_t> position(std::size_t(w) * h * 4, float_to_half(2.0f));
    s.position = texture(d.device.Get(), w, h, DXGI_FORMAT_R16G16B16A16_FLOAT, D3D11_BIND_SHADER_RESOURCE, position.data(), w * 8);
    std::vector<uint32_t> distort(std::size_t(w) * h, 0x80808080u);
    s.distort = texture(d.device.Get(), w, h, DXGI_FORMAT_R8G8B8A8_UNORM, D3D11_BIND_SHADER_RESOURCE, distort.data(), w * 4);
    std::vector<uint32_t> depth(std::size_t(w) * h);
    for (uint32_t y = 0; y < h; ++y) for (uint32_t x = 0; x < w; ++x) depth[std::size_t(y) * w + x] = d24(x < w / 8 && y > h * 3 / 4 ? 0.01 : 0.9);
    s.depth = depth_texture(d, w, h, depth);
    s.twin_srv = srv(d.device.Get(), s.twin.Get());
    s.motion_srv = srv(d.device.Get(), s.motion.Get());
    s.position_srv = srv(d.device.Get(), s.position.Get());
    s.distort_srv = srv(d.device.Get(), s.distort.Get());
    return s.twin && s.motion && s.position && s.distort && s.depth.srv && s.twin_srv && s.motion_srv;
}
// The rasterized frame under jitter (x, y): pixel p samples the scene at p + 0.5 - jitter (the image moves by +jitter).
void render_scene(Device& d, Scene& s, float jx, float jy) {
    std::vector<uint16_t> halves(std::size_t(s.w) * s.h * 4);
    for (uint32_t y = 0; y < s.h; ++y)
        for (uint32_t x = 0; x < s.w; ++x) {
            const double r = radiance((x + 0.5 - jx) / s.w, (y + 0.5 - jy) / s.h);
            const auto low = float_to_half(float(tonemap_forward(r)));
            auto* p = &halves[(std::size_t(y) * s.w + x) * 4];
            p[0] = p[1] = p[2] = low;
            p[3] = float_to_half(1.0f);
        }
    d.context->UpdateSubresource(s.twin.Get(), 0, nullptr, halves.data(), s.w * 8, 0);
}

int provider_case(const std::string& root) {
    Device d;
    if (!d.create(0x10de, false)) { std::printf("SKIP provider: no NVIDIA D3D11 adapter (the shared pipeline starts only where DLSS is supported)\n"); return 77; }
    // The carrier loads the Runtime before the adapter starts; the adapter finds it by name.
    HMODULE runtime = GetModuleHandleW(L"ReScaleFrame.Runtime.dll");
    if (!runtime) runtime = LoadLibraryW(L"ReScaleFrame.Runtime.dll");
    CHECK(runtime != nullptr && GetModuleHandleW(L"ReScaleFrame.Runtime.dll") == runtime);
    TemporalOptions options{};
    options.stage = 5;
    options.streamline = root + "\\streamline";
    options.fidelityfx = root + "\\fidelityfx";
    options.xess = root + "\\xess";
    options.initial = {true, backend_dlss, 1, 0};
    TemporalDriver driver;
    driver.configure(options, log_line, nullptr);
    CHECK(driver.bind(runtime));
    constexpr uint32_t OW = 960, OH = 540;
    driver.boundary(d.device.Get(), OW, OH);
    CHECK(driver.running());
    if (!driver.running()) return 1;
    UpscalePass pass;
    CHECK(pass.create(d.device.Get(), log_line, nullptr) && pass.temporal_ready());
    auto generic0 = texture(d.device.Get(), OW, OH, DXGI_FORMAT_R8G8B8A8_UNORM, D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE);
    auto engine_position = texture(d.device.Get(), OW, OH, DXGI_FORMAT_R16G16B16A16_FLOAT, D3D11_BIND_RENDER_TARGET);
    auto generic1 = texture(d.device.Get(), OW, OH, DXGI_FORMAT_R8G8B8A8_UNORM, D3D11_BIND_RENDER_TARGET);
    auto engine_depth = depth_texture(d, OW, OH, std::vector<uint32_t>(OW * OH, d24(1.0)));
    auto g0 = rtv(d.device.Get(), generic0.Get()), pos = rtv(d.device.Get(), engine_position.Get()), g1 = rtv(d.device.Get(), generic1.Get());
    struct Run { uint32_t backend; const char* name; bool exposure_auto; };
    const Run runs[] = {{backend_dlss, "DLSS", false}, {backend_dlss, "DLSS auto exposure", true}, {backend_fsr3, "FSR3", false},
        {backend_fsr4, "FSR4", false}, {backend_xess, "XeSS", false}, {backend_dlss, "DLSS again", false}};
    uint32_t providers_passed = 0;
    std::string summary;
    for (const auto& run : runs) {
        driver.request({true, run.backend, 1, 0});
        driver.boundary(d.device.Get(), OW, OH);
        // The first request for FSR4/XeSS/FSR3 in this process is only announced; the blocking select (the vendor loading its model,
        // FSR4: tens of seconds on a cold driver cache) runs at the next boundary.
        const auto wait_begin = GetTickCount64();
        for (uint32_t i = 0; i < 4 && driver.status().pending_backend; ++i) driver.boundary(d.device.Get(), OW, OH);
        CHECK(!driver.status().pending_backend);
        std::printf("%s: selected after %llu ms\n", run.name, static_cast<unsigned long long>(GetTickCount64() - wait_begin));
        const auto status = driver.status();
        if (status.applied.backend != run.backend) {
            std::printf("%s: refused by the pipeline on this GPU (select result %d)\n", run.name, status.select_result);
            CHECK(run.backend == backend_fsr4);   // only FSR4 may be unavailable here (RDNA4 only)
            summary += std::string(run.name) + "=refused ";
            continue;
        }
        uint32_t rw{}, rh{};
        CHECK(driver.planned_render(OW, OH, rw, rh));
        std::printf("%s: render %ux%u for %ux%u, pipeline output format %u\n", run.name, rw, rh, OW, OH, driver.output_format());
        Scene scene;
        CHECK(make_scene(d, rw, rh, scene));
        TemporalTargets targets;
        CHECK(make_temporal_targets(d.device.Get(), rw, rh, OW, OH, static_cast<DXGI_FORMAT>(driver.output_format()), targets) == TwinError::None);
        JitterSequence sequence;
        JitterState state{};
        const ViewCamera camera{view_at(0.0f), projection(1.2f, float(OW) / OH), true};
        uint32_t evaluated = 0;
        constexpr uint32_t frames = 24;
        for (uint32_t f = 0; f < frames; ++f) {
            const auto sample = sequence.next({rw, rh, OW, OH, 0}, true);
            state = next_state(state, false, true, sample, rw, rh, sequence.last_was_reset());
            render_scene(d, scene, state.current_x, state.current_y);
            const PrepareViews views{scene.twin_srv.Get(), scene.motion_srv.Get(), scene.depth.srv.Get(), rw, rh, targets.colour_rtv.Get(),
                targets.motion_rtv.Get(), first_person_depth_limit, true};
            CHECK(pass.prepare(d.context.Get(), views));
            PassInputs in{};
            in.native_frame = state.frame;
            in.generation = 1;
            in.render_width = rw; in.render_height = rh; in.output_width = OW; in.output_height = OH;
            in.current = camera; in.previous = camera;
            in.jitter = state;
            in.frame_time_seconds = 1.0f / 60;
            in.color = targets.colour.Get(); in.color_output = targets.output.Get(); in.depth = scene.depth.texture.Get();
            in.motion = targets.motion.Get();
            in.exposure = run.exposure_auto ? nullptr : targets.exposure.Get();
            ResetFacts facts{};
            facts.sequence = state.reset;
            const bool ok = driver.evaluate(d.context.Get(), in, facts);
            if (ok) {
                ++evaluated;
                const UpscaleViews uv{scene.twin_srv.Get(), scene.position_srv.Get(), scene.distort_srv.Get(), scene.depth.srv.Get(), rw, rh,
                    g0.Get(), pos.Get(), g1.Get(), engine_depth.dsv.Get(), OW, OH};
                CHECK(pass.reinsert(d.context.Get(), uv, targets.output_srv.Get()));
            }
        }
        // The converged output against the scene, re-tonemapped: centre region (away from edges), UNORM8 codes.
        const auto out = read_back<uint8_t>(d, generic0.Get(), 4);
        const auto linear = read_back<uint16_t>(d, targets.output.Get(), 4);
        double sum = 0, worst = 0, nonfinite = 0;
        uint32_t samples = 0;
        for (uint32_t y = OH / 8; y < OH * 7 / 8 && out.size() == std::size_t(OW) * OH * 4; y += 7)
            for (uint32_t x = OW / 8; x < OW * 7 / 8; x += 7) {
                const double expected = 255.0 * std::min(1.0, tonemap_forward(radiance((x + 0.5) / OW, (y + 0.5) / OH)));
                const double e = std::fabs(double(out[(std::size_t(y) * OW + x) * 4]) - expected);
                sum += e; worst = std::max(worst, e); ++samples;
                if (linear.size() == std::size_t(OW) * OH * 4 && !std::isfinite(half_to_float(linear[(std::size_t(y) * OW + x) * 4]))) ++nonfinite;
            }
        // End to end through the pipeline: one more DLSS frame after the camera moved 0.5 m along -X, with the pipeline's own
        // dump of the dense motion it submitted. Valid zero (top half) and the first-person corner must stay zero; unwritten
        // pixels (bottom half) must carry the camera reconstruction; no sentinel may reach the backend.
        if (std::string_view{run.name} == "DLSS again") {
            rsf_d3d11_sr_api api{sizeof(api), RSF_D3D11_SR_API_VERSION};
            const auto query = reinterpret_cast<rsf_get_d3d11_sr_api_fn>(reinterpret_cast<void*>(GetProcAddress(runtime, RSF_D3D11_SR_API_ENTRY_POINT)));
            CHECK(query && query(&api) == RSF_OK);
            char temp[MAX_PATH]{};
            GetTempPathA(MAX_PATH, temp);
            const std::string prefix = std::string(temp) + "rsf_anomaly_sr_motion_" + std::to_string(GetCurrentProcessId());
            CHECK(api.request_dump && api.request_dump(prefix.c_str()) == RSF_DLSS_PIPELINE_OK);
            const auto sample = sequence.next({rw, rh, OW, OH, 0}, true);
            state = next_state(state, false, true, sample, rw, rh, false);
            render_scene(d, scene, state.current_x, state.current_y);
            const PrepareViews views{scene.twin_srv.Get(), scene.motion_srv.Get(), scene.depth.srv.Get(), rw, rh, targets.colour_rtv.Get(),
                targets.motion_rtv.Get(), first_person_depth_limit, true};
            CHECK(pass.prepare(d.context.Get(), views));
            PassInputs in{};
            in.native_frame = state.frame; in.generation = 1;
            in.render_width = rw; in.render_height = rh; in.output_width = OW; in.output_height = OH;
            in.current = camera;
            in.previous = {view_at(0.5f), camera.projection, true};
            in.jitter = state; in.frame_time_seconds = 1.0f / 60;
            in.color = targets.colour.Get(); in.color_output = targets.output.Get(); in.depth = scene.depth.texture.Get();
            in.motion = targets.motion.Get(); in.exposure = targets.exposure.Get();
            CHECK(driver.evaluate(d.context.Get(), in, ResetFacts{}));
            rsf_game_render_pass built;
            CHECK(build_pass(in, built) == PassError::None);
            std::vector<uint16_t> submitted(std::size_t(rw) * rh * 2);
            FILE* file = std::fopen((prefix + "_motion_submitted.bin").c_str(), "rb");
            const bool read = file && std::fread(submitted.data(), 4, submitted.size() / 2, file) == submitted.size() / 2;
            if (file) std::fclose(file);
            std::string metadata(256, '\0');
            file = std::fopen((prefix + "_motion_submitted_raw.json").c_str(), "rb");
            if (file) { metadata.resize(std::fread(metadata.data(), 1, metadata.size(), file)); std::fclose(file); }
            CHECK(read && metadata.find("\"format\":34") != std::string::npos);   // R16G16_FLOAT: the dense resolve's output
            uint32_t zero_ok = 0, zero_bad = 0, camera_ok = 0, camera_bad = 0, sentinels = 0;
            for (uint32_t y = 0; y < rh && read; ++y)
                for (uint32_t x = 0; x < rw; ++x) {
                    const float mx = half_to_float(submitted[(std::size_t(y) * rw + x) * 2]), my = half_to_float(submitted[(std::size_t(y) * rw + x) * 2 + 1]);
                    if (std::fabs(mx) > 500 || std::fabs(my) > 500) ++sentinels;
                    const bool first_person = x < rw / 8 && y > rh * 3 / 4;
                    if (y < rh / 2 || first_person) { (mx == 0 && my == 0 ? zero_ok : zero_bad) += 1; continue; }
                    const double ndc[2]{(x + 0.5) / rw * 2 - 1, 1 - (y + 0.5) / rh * 2};
                    double prev[4]{};
                    const double v[4]{ndc[0], ndc[1], double(d24(0.9)) / 16777215.0, 1};
                    for (int c = 0; c < 4; ++c) for (int k = 0; k < 4; ++k) prev[c] += v[k] * built.camera.clip_to_previous_clip[k * 4 + c];
                    const double ex = (prev[0] / prev[3] - ndc[0]) * rw * 0.5, ey = (prev[1] / prev[3] - ndc[1]) * rh * -0.5;
                    (close_to(mx, ex, 0.02 + 2e-3 * std::fabs(ex)) && close_to(my, ey, 0.02) && std::fabs(ex) > 0.1 ? camera_ok : camera_bad) += 1;
                }
            std::printf("DLSS submitted motion (camera moved): valid/first-person zero %u ok %u wrong; unwritten reconstructed %u ok %u wrong; "
                "sentinels reaching DLSS %u\n", zero_ok, zero_bad, camera_ok, camera_bad, sentinels);
            CHECK(zero_ok > 0 && zero_bad == 0 && camera_ok > 0 && camera_bad == 0 && sentinels == 0);
            for (const char* suffix : {"_input", "_output", "_motion", "_motion_decoded", "_motion_submitted", "_depth", "_exposure"})
                for (const char* extension : {".bin", "_raw.json", ".png", ".json", ".tga"})
                    DeleteFileA((prefix + suffix + extension).c_str());
            DeleteFileA((prefix + "_frame.json").c_str());
        }
        const double mean = samples ? sum / samples : 1e9;
        std::printf("%s: %u of %u frames evaluated; converged output vs scene: mean %.2f, worst %.1f UNORM8 codes over %u samples, %g non-finite\n",
            run.name, evaluated, frames, mean, worst, samples, nonfinite);
        const bool passed = evaluated == frames && mean < 3.0 && worst < 24.0 && nonfinite == 0;
        CHECK(passed);
        if (passed) ++providers_passed;
        char item[96];
        std::snprintf(item, sizeof(item), "%s=%u/%u(mean %.2f) ", run.name, evaluated, frames, mean);
        summary += item;
    }
    const auto counters = driver.counters();
    std::printf("provider summary: %s| evaluated dlss=%llu fsr3=%llu fsr4=%llu xess=%llu, refused pipeline=%llu pass=%llu, resets first=%llu "
        "settings=%llu sequence=%llu\n", summary.c_str(), static_cast<unsigned long long>(counters.evaluated[backend_dlss]),
        static_cast<unsigned long long>(counters.evaluated[backend_fsr3]), static_cast<unsigned long long>(counters.evaluated[backend_fsr4]),
        static_cast<unsigned long long>(counters.evaluated[backend_xess]),
        static_cast<unsigned long long>(counters.refused[static_cast<std::size_t>(Refusal::Pipeline)]),
        static_cast<unsigned long long>(counters.refused[static_cast<std::size_t>(Refusal::Pass)]),
        static_cast<unsigned long long>(counters.resets[static_cast<std::size_t>(ResetReason::First)]),
        static_cast<unsigned long long>(counters.resets[static_cast<std::size_t>(ResetReason::Settings)]),
        static_cast<unsigned long long>(counters.resets[static_cast<std::size_t>(ResetReason::Sequence)]));
    rsf_overlay_sr_report report{};
    CHECK(driver.overlay_report(report) && report.effective_backend == backend_dlss && report.render_width && report.output_width == OW &&
        (report.inputs & RSF_OVERLAY_SR_INPUT_JITTER) && (report.inputs & RSF_OVERLAY_SR_INPUT_DEPTH) && report.reason[0] == '\0');
    CHECK(providers_passed >= 1);
    driver.stop();
    CHECK(!driver.running() && !driver.status().pipeline.running);
    return 0;
}
}

int main(int argc, char** argv) {
    const std::string_view selected = argc > 1 ? argv[1] : "";
    std::string root;
    if (argc > 2) root = argv[2];
    else if (const char* env = std::getenv("RSF_SR_VENDOR_ROOT")) root = env;
    int ran = 0, skipped = 0;
    if (selected.empty() || selected == "decode" || selected == "prepare" || selected == "thin") {
        Device d;
        // The NVIDIA adapter when there is one (common rule on this hybrid laptop), else any hardware adapter.
        if (!d.create(0x10de, true) && !d.create(0, true)) { std::printf("SKIP: no D3D11 hardware device\n"); return 77; }
        if (selected.empty() || selected == "decode") { decode_case(d); ++ran; }
        if (selected.empty() || selected == "thin") { thin_case(d); ++ran; }
        if (selected.empty() || selected == "prepare") { prepare_case(d); ++ran; }
        // Debug-layer errors from either case (when the layer is installed).
        ComPtr<ID3D11InfoQueue> info;
        if (SUCCEEDED(d.device.As(&info))) {
            uint64_t errors = 0;
            for (UINT64 i = 0, n = info->GetNumStoredMessages(); i < n; ++i) {
                SIZE_T size = 0;
                info->GetMessage(i, nullptr, &size);
                std::vector<char> storage(size);
                auto* message = reinterpret_cast<D3D11_MESSAGE*>(storage.data());
                if (SUCCEEDED(info->GetMessage(i, message, &size)) &&
                    (message->Severity == D3D11_MESSAGE_SEVERITY_ERROR || message->Severity == D3D11_MESSAGE_SEVERITY_CORRUPTION)) {
                    std::printf("  debug layer: %.*s\n", int(message->DescriptionByteLength), message->pDescription);
                    ++errors;
                }
            }
            std::printf("debug layer errors: %llu\n", static_cast<unsigned long long>(errors));
            CHECK(errors == 0);
        }
    }
    if (selected.empty() || selected == "provider") {
        if (root.empty()) { std::printf("SKIP provider: pass <vendor root> or set RSF_SR_VENDOR_ROOT\n"); ++skipped; }
        else if (provider_case(root) == 77) ++skipped;
        else ++ran;
    }
    if (!ran && !skipped) { std::printf("unknown case\n"); return 2; }
    std::printf("%d case(s), %d skipped, %d failure(s)\n", ran, skipped, failures);
    if (failures) return 1;
    return ran ? 0 : 77;
}
