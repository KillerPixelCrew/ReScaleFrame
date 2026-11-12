// SPDX-License-Identifier: GPL-3.0-only
// Backend switching through the shared D3D11 SR pipeline on real hardware (NVIDIA adapter, vendor runtimes).
//
//   sr_backend_switch_device <vendor root> baseline
//       The synchronous rsf_dlss_pipeline_select_backend, timed per step on the calling thread. This is the "before" number.
//   sr_backend_switch_device <vendor root> select
//       The same selector now: the vendor first-use build happens inside the select (logged "preparing" / "prepared in N ms"),
//       and no frame after it stalls. A mode suffix "+exposure" offers a constant exposure texture, as Anomaly does.
//
// <vendor root> is the package layout <root>/{streamline,fidelityfx,xess}. Exit 77 when the machine cannot run it.
#include <rescaleframe/dlss_pipeline.h>
#include <rescaleframe/d3d11_state.h>
#include <d3d11.h>
#include <dxgi.h>
#include <wrl/client.h>
#include <algorithm>
#include <chrono>
#include <cstdio>
#include <cstring>
#include <string>
#include <vector>
using Microsoft::WRL::ComPtr;
namespace {
using Clock = std::chrono::steady_clock;
Clock::time_point origin = Clock::now();
double since_origin() { return std::chrono::duration<double>(Clock::now() - origin).count(); }
double milliseconds(Clock::time_point from, Clock::time_point to) { return std::chrono::duration<double, std::milli>(to - from).count(); }
int failures = 0;
#define CHECK(condition) do { if (!(condition)) { std::printf("FAIL %s:%d: %s\n", __FILE__, __LINE__, #condition); ++failures; } } while (0)
void log_line(void*, const char* message) { std::fprintf(stderr, "[%8.3f] %s\n", since_origin(), message); }

constexpr uint32_t kSize = 1280, kHeight = 720;
struct Rig {
    ComPtr<ID3D11Device> device;
    ComPtr<ID3D11DeviceContext> context;
    ComPtr<ID3D11Texture2D> textures[3];
    // The frames offer a 1x1 constant exposure texture, as Anomaly does unless SRExposure=auto (mode suffix "+exposure"). The
    // alternate backends then take their exposure from it and are created with that policy instead of reopened by the first frame.
    bool with_exposure = false;
    ComPtr<ID3D11Texture2D> exposure;
    rsf_pipeline_camera_frame camera{};
    rsf_dlss_pipeline_frame frame{};
    uint32_t serial = 0;
    double worst_call_ms = 0;          // the longest single pipeline call since reset_worst()
    std::string worst_call_name;
    void reset_worst() { worst_call_ms = 0; worst_call_name.clear(); }
    void note(const char* name, double ms) {
        if (ms > worst_call_ms) { worst_call_ms = ms; worst_call_name = name; }
    }
    // The three inputs at a render size, cleared to constants. The frame names them and the size.
    bool make_inputs(uint32_t width, uint32_t height) {
        for (uint32_t i = 0; i < 3; ++i) {
            D3D11_TEXTURE2D_DESC desc{};
            desc.Width = width; desc.Height = height; desc.MipLevels = desc.ArraySize = desc.SampleDesc.Count = 1;
            desc.Format = i == 0 ? DXGI_FORMAT_R16G16B16A16_FLOAT : i == 1 ? DXGI_FORMAT_R32_FLOAT : DXGI_FORMAT_R16G16_FLOAT;
            desc.BindFlags = D3D11_BIND_SHADER_RESOURCE | D3D11_BIND_UNORDERED_ACCESS;
            textures[i].Reset();
            if (FAILED(device->CreateTexture2D(&desc, nullptr, &textures[i]))) return false;
            ComPtr<ID3D11UnorderedAccessView> view;
            if (FAILED(device->CreateUnorderedAccessView(textures[i].Get(), nullptr, &view))) return false;
            const FLOAT values[] = {i == 0 ? 0.25f : i == 1 ? 0.5f : 0.0f, 0, 0, 1};
            context->ClearUnorderedAccessViewFloat(view.Get(), values);
        }
        frame.scene_color = textures[0].Get(); frame.depth = textures[1].Get(); frame.game_motion = textures[2].Get();
        frame.render_width = width; frame.render_height = height;
        if (with_exposure && !exposure) {
            D3D11_TEXTURE2D_DESC desc{};
            desc.Width = desc.Height = 1; desc.MipLevels = desc.ArraySize = desc.SampleDesc.Count = 1;
            desc.Format = DXGI_FORMAT_R32_FLOAT; desc.BindFlags = D3D11_BIND_SHADER_RESOURCE;
            const float one = 1.0f;
            D3D11_SUBRESOURCE_DATA data{&one, sizeof(one), 0};
            if (FAILED(device->CreateTexture2D(&desc, &data, &exposure))) return false;
        }
        frame.exposure = exposure.Get();
        return true;
    }
    bool create() {
        ComPtr<IDXGIFactory> factory; ComPtr<IDXGIAdapter> adapter;
        if (FAILED(CreateDXGIFactory(IID_PPV_ARGS(&factory)))) return false;
        for (uint32_t i = 0; ; ++i) {
            if (FAILED(factory->EnumAdapters(i, &adapter))) return false;
            DXGI_ADAPTER_DESC desc{}; adapter->GetDesc(&desc);
            if (desc.VendorId == 0x10de) break;
            adapter.Reset();
        }
        if (FAILED(D3D11CreateDevice(adapter.Get(), D3D_DRIVER_TYPE_UNKNOWN, nullptr, 0, nullptr, 0, D3D11_SDK_VERSION, &device, nullptr, &context)))
            return false;
        if (!make_inputs(kSize, kHeight)) return false;
        camera.struct_size = sizeof(camera); camera.abi_version = RSF_FRAME_ASSEMBLY_ABI_VERSION;
        camera.view_to_clip[0] = camera.view_to_clip[5] = 2; camera.view_to_clip[11] = 1; camera.view_to_clip[14] = 0.1f;
        camera.clip_to_view[0] = camera.clip_to_view[5] = 0.5f; camera.clip_to_view[11] = 10; camera.clip_to_view[14] = 1;
        camera.clip_to_prev_clip[0] = camera.clip_to_prev_clip[5] = camera.clip_to_prev_clip[10] = camera.clip_to_prev_clip[15] = 1;
        camera.prev_clip_to_clip[0] = camera.prev_clip_to_clip[5] = camera.prev_clip_to_clip[10] = camera.prev_clip_to_clip[15] = 1;
        camera.camera_forward[2] = camera.camera_up[1] = camera.camera_right[0] = 1;
        camera.near_plane = 0.1f; camera.vertical_fov = 1; camera.aspect_ratio = 1;
        camera.depth_inverted = camera.has_jitter = 1; camera.jitter_pixels[0] = 0.25f;
        frame.struct_size = sizeof(frame); frame.abi_version = RSF_DLSS_PIPELINE_ABI_VERSION;
        frame.scene_color = textures[0].Get(); frame.depth = textures[1].Get(); frame.game_motion = textures[2].Get();
        frame.render_width = kSize; frame.render_height = kHeight; frame.camera = &camera;
        return true;
    }
    rsf_dlss_pipeline_result run_frame(const char* name = "frame") {
        camera.jitter_pixels[0] = (serial++ % 2) ? -0.25f : 0.25f;
        rsf_d3d11_state saved{}; rsf_d3d11_state_save(context.Get(), &saved);
        const auto begin = Clock::now();
        const auto result = rsf_dlss_pipeline_on_frame(context.Get(), &frame);
        note(name, milliseconds(begin, Clock::now()));
        rsf_d3d11_state_restore(context.Get(), &saved);
        return result;
    }
    // A frame boundary: the engine's present. Keeps the GPU from running unboundedly ahead of the CPU.
    void present() { context->Flush(); }
};

bool start(Rig& rig, const std::string& root) {
    const std::string streamline = root + "\\streamline", fidelityfx = root + "\\fidelityfx", xess = root + "\\xess";
    rsf_dlss_pipeline_setup setup{};
    setup.struct_size = sizeof(setup); setup.abi_version = RSF_DLSS_PIPELINE_ABI_VERSION;
    setup.streamline_directory_utf8 = streamline.c_str(); setup.output_width = kSize; setup.output_height = kHeight;
    setup.quality = RSF_DLSS_QUALITY_NATIVE;
    setup.motion.struct_size = sizeof(setup.motion);
    setup.motion.scale_x = setup.motion.scale_y = 1;
    setup.motion.output_scale_x = setup.motion.output_scale_y = 1;
    setup.motion.invalid_value = -1000; setup.motion.zero_means_unwritten = 1;
    setup.engine = RSF_DLSS_ENGINE_CUSTOM; setup.engine_version_utf8 = "ReScaleFrame synthetic fixture";
    setup.project_id_utf8 = "a3ed1f08-3542-4698-b85c-e1a9908e861a";
    setup.fsr2_directory_utf8 = setup.fsr3_directory_utf8 = setup.fsr4_directory_utf8 = fidelityfx.c_str();
    setup.xess_directory_utf8 = xess.c_str(); setup.view_space_to_meters = 1; setup.log = log_line;
    const auto result = rsf_dlss_pipeline_start(rig.device.Get(), &setup);
    if (result != 0) { std::fprintf(stderr, "pipeline start refused: %d\n", result); return false; }
    return true;
}

// DLSS frames until the pipeline is warm, so that the first-feature cost is not read as a switch stall.
bool warm_up(Rig& rig) {
    for (uint32_t i = 0; i < 12; ++i) {
        const auto result = rig.run_frame("warm frame");
        rig.present();
        if (result != 0) { std::printf("warm-up frame %u refused: %d\n", i, result); return false; }
    }
    return true;
}

int baseline(Rig& rig) {
    CHECK(warm_up(rig));
    for (uint32_t backend : {RSF_SR_FSR4, RSF_SR_XESS, RSF_SR_FSR3, RSF_SR_DLSS, RSF_SR_FSR4}) {
        const auto begin = Clock::now();
        const auto result = rsf_dlss_pipeline_select_backend(backend, nullptr, nullptr);
        const double select_ms = milliseconds(begin, Clock::now());
        double first_ms = 0, rest_ms = 0;
        for (uint32_t i = 0; i < 8 && result == 0; ++i) {
            const auto start_frame = Clock::now();
            const auto frame_result = rig.run_frame();
            rig.present();
            const double ms = milliseconds(start_frame, Clock::now());
            (i == 0 ? first_ms : rest_ms) = i == 0 ? ms : std::max(rest_ms, ms);
            CHECK(frame_result == 0);
        }
        std::printf("BASELINE select backend %u (sync): result %d, render-thread stall %.1f ms; first frame %.1f ms, worst later frame %.1f ms\n",
            backend, result, select_ms, first_ms, rest_ms);
    }
    return 0;
}

// The synchronous selector with the vendor first-use build included in it: the select is the stall (logged by the pipeline as
// "preparing" and "prepared in N ms"), and the first frame after it must not be a second one. FSR4 on a cold driver cache spends about
// 25 s in the select, which is the vendor building its own pipelines; before the warm-up moved into select the same 25 s landed in the
// first frame, with nothing in the log naming it.
int select_stall(Rig& rig) {
    CHECK(warm_up(rig));
    double first_select_ms = 0;
    for (uint32_t backend : {RSF_SR_FSR4, RSF_SR_XESS, RSF_SR_FSR3, RSF_SR_DLSS, RSF_SR_FSR4}) {
        const auto begin = Clock::now();
        const auto result = rsf_dlss_pipeline_select_backend(backend, nullptr, nullptr);
        const double select_ms = milliseconds(begin, Clock::now());
        CHECK(result == 0);
        double first_ms = 0, worst_later = 0;
        for (uint32_t i = 0; i < 12 && result == 0; ++i) {
            const auto t = Clock::now();
            CHECK(rig.run_frame() == 0);
            rig.present();
            const double ms = milliseconds(t, Clock::now());
            if (i == 0) first_ms = ms; else worst_later = std::max(worst_later, ms);
        }
        std::printf("SELECT backend %u: select %.1f ms, first frame %.1f ms, worst later frame %.1f ms\n", backend, select_ms, first_ms, worst_later);
        if (!first_select_ms) first_select_ms = select_ms;
        // The stall is in the select and stays out of the frames: no frame after it is anywhere near a first-use build.
        CHECK(first_ms < 250.0 && worst_later < 250.0);
    }
    std::printf("SELECT first FSR4 select %.0f ms\n", first_select_ms);
    return 0;
}
}

int main(int argc, char** argv) {
    if (argc < 3) { std::puts("usage: sr_backend_switch_device <vendor root> baseline|select"); return 77; }
    const std::string root = argv[1];
    std::string mode = argv[2];
    Rig rig;
    const std::string suffix = "+exposure";
    if (mode.size() > suffix.size() && mode.compare(mode.size() - suffix.size(), suffix.size(), suffix) == 0) {
        rig.with_exposure = true;
        mode.resize(mode.size() - suffix.size());
    }
    if (!rig.create()) { std::puts("SKIP: no NVIDIA D3D11 adapter"); return 77; }
    if (!start(rig, root)) { std::puts("SKIP: the shared pipeline did not start (Streamline or DLSS unavailable)"); return 77; }
    int status = 0;
    if (mode == "baseline") status = baseline(rig);
    else if (mode == "select") status = select_stall(rig);
    else { std::printf("unknown mode %s\n", mode.c_str()); status = 2; }
    rsf_dlss_pipeline_stop();
    if (failures) return 1;
    return status;
}
