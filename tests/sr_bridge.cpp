// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/sr_bridge.h>
#include <d3d11.h>
#include <wrl/client.h>
#include <cstdio>
#include <cstring>
#include <cmath>
#include <cwchar>
#include <cstdlib>
#include <dxgi.h>
using Microsoft::WRL::ComPtr;
void log_line(void*, const char* message) { std::fprintf(stderr, "%s\n", message); }
int main(int argc, char** argv)
{
    // Hardware SDK evaluations are explicit, never part of routine verification or game deployment.
    if (argc != 3 && argc != 4) { std::puts("provide FFX and XeSS runtime directories [adapter index] to run hardware evaluation"); return 77; }
    ComPtr<ID3D11Device> device;
    ComPtr<ID3D11DeviceContext> context;
    ComPtr<IDXGIAdapter> selected_adapter;
    if (argc == 4) {
        ComPtr<IDXGIFactory> factory;
        if (FAILED(CreateDXGIFactory(IID_PPV_ARGS(&factory))) ||
            FAILED(factory->EnumAdapters(static_cast<UINT>(std::strtoul(argv[3], nullptr, 10)), &selected_adapter))) return 1;
    }
    if (FAILED(D3D11CreateDevice(selected_adapter.Get(), selected_adapter ? D3D_DRIVER_TYPE_UNKNOWN : D3D_DRIVER_TYPE_HARDWARE,
                                 nullptr, 0, nullptr, 0,
                                 D3D11_SDK_VERSION, &device, nullptr, &context))) return 77;
    ComPtr<IDXGIDevice> dxgi; device.As(&dxgi);
    ComPtr<IDXGIAdapter> adapter; dxgi->GetAdapter(&adapter);
    DXGI_ADAPTER_DESC adapter_desc{}; adapter->GetDesc(&adapter_desc);
    std::fwprintf(stderr, L"adapter: %s vendor %x device %x\n", adapter_desc.Description,
                  adapter_desc.VendorId, adapter_desc.DeviceId);
    rsf_sr_session_setup setup{};
    setup.struct_size = sizeof(setup); setup.abi_version = RSF_SR_SESSION_ABI_VERSION;
    setup.open.struct_size = sizeof(setup.open); setup.open.abi_version = RSF_BACKEND_ABI_VERSION;
    setup.open.api = RSF_API_D3D11; setup.open.device = device.Get();
    setup.open.output_width = 128; setup.open.output_height = 128;
    setup.open.hdr = 1; setup.open.inverted_depth = 1; setup.open.auto_exposure = 1;
    setup.open.dynamic_resolution = 1; setup.open.log = log_line;
    setup.fsr2_directory_utf8 = setup.fsr3_directory_utf8 = setup.fsr4_directory_utf8 = argv[1];
    setup.xess_directory_utf8 = argv[2];
    rsf_sr_bridge* bridge = nullptr;
    auto result = rsf_sr_bridge_create(&setup, &bridge);
    std::fprintf(stderr, "bridge create: %d\n", result);
    if (result != RSF_BACKEND_OK) return 1;
    ComPtr<ID3D11Texture2D> textures[4];
    for (uint32_t i = 0; i < 4; ++i) {
        D3D11_TEXTURE2D_DESC desc{};
        desc.Width = desc.Height = 128; desc.MipLevels = desc.ArraySize = desc.SampleDesc.Count = 1;
        desc.Format = i == 1 ? DXGI_FORMAT_R32_FLOAT : i == 2 ? DXGI_FORMAT_R16G16_FLOAT : DXGI_FORMAT_R16G16B16A16_FLOAT;
        desc.BindFlags = D3D11_BIND_SHADER_RESOURCE | D3D11_BIND_UNORDERED_ACCESS;
        if (FAILED(device->CreateTexture2D(&desc, nullptr, &textures[i]))) return 1;
        ComPtr<ID3D11UnorderedAccessView> view;
        if (FAILED(device->CreateUnorderedAccessView(textures[i].Get(), nullptr, &view))) return 1;
        const FLOAT values[4] = {i == 1 ? 0.5f : i == 2 ? 0.0f : 0.25f, 0.0f, 0.0f, 1.0f};
        context->ClearUnorderedAccessViewFloat(view.Get(), values);
    }
    rsf_frame_record record{};
    record.struct_size = sizeof(record); record.abi_version = RSF_GAME_FRAME_ABI_VERSION;
    record.session_id = 1; record.resource_generation = 1; record.frame_time_ms = 16.67f;
    record.render_width = record.render_height = record.output_width = record.output_height = 128;
    record.camera.struct_size = sizeof(record.camera); record.camera.abi_version = RSF_GAME_FRAME_ABI_VERSION;
    record.camera.near_plane = 0.1f; record.camera.far_plane = 10000; record.camera.vertical_fov_radians = 1;
    record.camera.depth_inverted = 1;
    rsf_sr_frame frame{};
    frame.struct_size = sizeof(frame); frame.record = &record;
    frame.pre_exposure = frame.view_space_to_meters = 1;
    frame.motion_scale_x = frame.motion_scale_y = 1;
    rsf_backend_resource* resources[] = {&frame.color, &frame.depth, &frame.motion, &frame.output};
    for (uint32_t i = 0; i < 4; ++i) {
        resources[i]->struct_size = sizeof(*resources[i]); resources[i]->resource = textures[i].Get();
        resources[i]->width = resources[i]->height = 128; resources[i]->generation = 1;
    }
    ComPtr<ID3D11Texture2D> exposure;
    D3D11_TEXTURE2D_DESC exposure_desc{};
    exposure_desc.Width = exposure_desc.Height = exposure_desc.MipLevels = exposure_desc.ArraySize = exposure_desc.SampleDesc.Count = 1;
    exposure_desc.Format = DXGI_FORMAT_R32_FLOAT; exposure_desc.BindFlags = D3D11_BIND_SHADER_RESOURCE;
    const float exposure_value = 1;
    D3D11_SUBRESOURCE_DATA exposure_data{&exposure_value, sizeof(float), 0};
    if (FAILED(device->CreateTexture2D(&exposure_desc, &exposure_data, &exposure))) return 1;
    bool passed = true;
    const rsf_sr_backend sequence[] = {RSF_SR_FSR2, RSF_SR_FSR3, RSF_SR_XESS, RSF_SR_FSR2,
                                      RSF_SR_FSR4, RSF_SR_XESS, RSF_SR_FSR4};
    const rsf_quality qualities[] = {RSF_QUALITY_NATIVE, RSF_QUALITY_QUALITY, RSF_QUALITY_NATIVE,
                                     RSF_QUALITY_PERFORMANCE, RSF_QUALITY_NATIVE,
                                     RSF_QUALITY_ULTRA_PERFORMANCE, RSF_QUALITY_ULTRA_PERFORMANCE};
    uint32_t sequence_index = 0;
    for (auto backend : sequence) {
        result = rsf_sr_bridge_select(bridge, backend, qualities[sequence_index++], 0);
        rsf_sr_session_status status{}; status.struct_size = sizeof(status);
        rsf_sr_bridge_get_status(bridge, &status);
        std::fprintf(stderr, "select %u: %d effective %u version %s\n", backend, result, status.effective, status.version_name);
        if (backend == RSF_SR_FSR4 && result == RSF_BACKEND_ERROR_NOT_SUPPORTED) continue;
        if (result != RSF_BACKEND_OK) { passed = false; continue; }
        record.render_width = status.render_width; record.render_height = status.render_height;
        ++record.resource_generation;
        for (uint32_t i = 0; i < 4; ++i) {
            resources[i]->generation = record.resource_generation;
            resources[i]->width = i == 3 ? 128 : record.render_width;
            resources[i]->height = i == 3 ? 128 : record.render_height;
        }
        // Reuse all command slots, then rebuild the SDK context while earlier work is queued.
        for (uint32_t i = 0; i < 12; ++i) {
            if (i == 6 || i == 9) {
                result = rsf_sr_bridge_set_auto_exposure(bridge, i == 9 ? 1u : 0u);
                passed &= result == RSF_BACKEND_OK;
                frame.exposure = {};
                if (i == 6) {
                    frame.exposure.struct_size = sizeof(frame.exposure);
                    frame.exposure.resource = exposure.Get();
                    frame.exposure.width = frame.exposure.height = 1;
                    frame.exposure.generation = record.resource_generation;
                }
            }
            ++record.frame_id;
            frame.jitter_x = i == 0 ? -0.25f : i == 1 ? 0.25f : 0;
            result = rsf_sr_bridge_evaluate(bridge, context.Get(), &frame);
            std::fprintf(stderr, "frame %llu: %d\n", record.frame_id, result);
            passed &= result == RSF_BACKEND_OK;
        }
        D3D11_TEXTURE2D_DESC staging_desc{}; textures[3]->GetDesc(&staging_desc);
        staging_desc.BindFlags = 0; staging_desc.Usage = D3D11_USAGE_STAGING;
        staging_desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
        ComPtr<ID3D11Texture2D> staging;
        if (FAILED(device->CreateTexture2D(&staging_desc, nullptr, &staging))) { passed = false; continue; }
        context->CopyResource(staging.Get(), textures[3].Get());
        D3D11_MAPPED_SUBRESOURCE mapped{};
        if (FAILED(context->Map(staging.Get(), 0, D3D11_MAP_READ, 0, &mapped))) { passed = false; continue; }
        auto* pixel = reinterpret_cast<const uint16_t*>(static_cast<const char*>(mapped.pData) + 64 * mapped.RowPitch) + 64 * 4;
        const uint16_t red_half = pixel[0];
        const float red = std::ldexp(float(1024 + (red_half & 1023)), int((red_half >> 10) & 31) - 25);
        std::fprintf(stderr, "output center red: %.6f (expected 0.25)\n", double(red));
        passed &= std::isfinite(red) && std::fabs(red - 0.25f) < 0.05f;
        context->Unmap(staging.Get(), 0);
    }
    rsf_sr_bridge_destroy(bridge);
    return passed ? 0 : 1;
}
