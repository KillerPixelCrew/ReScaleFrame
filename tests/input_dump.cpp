// SPDX-License-Identifier: GPL-3.0-only
// Exercise a complete handoff with known pixels on WARP, then decode the PNGs independently.
#include <rescaleframe/input_dump.h>
#include <rescaleframe/native_window.h>
#include <d3d11.h>
#include <d3d11sdklayers.h>
#include <wincodec.h>
#include <wrl/client.h>
#include <array>
#include <cmath>
#include <cstdio>
#include <filesystem>
#include <fstream>
#include <iterator>
#include <limits>
#include <string>
#include <vector>

using Microsoft::WRL::ComPtr;
namespace {
bool passed = true;
void check(bool condition, const char* message)
{ if (!condition) { std::fprintf(stderr, "%s\n", message); passed = false; } }
std::string read(const std::filesystem::path& file)
{ std::ifstream in(file, std::ios::binary); return {std::istreambuf_iterator<char>(in), {}}; }
std::vector<unsigned char> png(const std::filesystem::path& file, UINT width, UINT height)
{
    ComPtr<IWICImagingFactory> factory;
    ComPtr<IWICBitmapDecoder> decoder;
    ComPtr<IWICBitmapFrameDecode> frame;
    ComPtr<IWICFormatConverter> converter;
    std::vector<unsigned char> pixels(size_t(width) * height * 4);
    UINT actual_width = 0, actual_height = 0;
    const bool ok = SUCCEEDED(CoCreateInstance(CLSID_WICImagingFactory, nullptr, CLSCTX_INPROC_SERVER, IID_PPV_ARGS(&factory))) &&
        SUCCEEDED(factory->CreateDecoderFromFilename(file.c_str(), nullptr, GENERIC_READ, WICDecodeMetadataCacheOnLoad, &decoder)) &&
        SUCCEEDED(decoder->GetFrame(0, &frame)) && SUCCEEDED(frame->GetSize(&actual_width, &actual_height)) &&
        actual_width == width && actual_height == height && SUCCEEDED(factory->CreateFormatConverter(&converter)) &&
        SUCCEEDED(converter->Initialize(frame.Get(), GUID_WICPixelFormat32bppRGBA, WICBitmapDitherTypeNone, nullptr, 0, WICBitmapPaletteTypeCustom)) &&
        SUCCEEDED(converter->CopyPixels(nullptr, width * 4, UINT(pixels.size()), pixels.data()));
    check(ok, "PNG must independently decode at the original dimensions.");
    for (size_t i = 3; i < pixels.size(); i += 4) check(pixels[i] == 255, "Every preview pixel must be opaque.");
    return pixels;
}
}
int main(int argc, char** argv)
{
    if (argc != 2) return 2;
    const HRESULT initialized = CoInitializeEx(nullptr, COINIT_MULTITHREADED);
    if (FAILED(initialized)) return 77;
    WNDCLASSW wc{}; wc.hInstance = GetModuleHandleW(nullptr); wc.lpfnWndProc = DefWindowProcW; wc.lpszClassName = L"RSF_InputDumpTest";
    RegisterClassW(&wc);
    const HWND window = CreateWindowW(wc.lpszClassName, L"", WS_OVERLAPPED, 0, 0, 32, 32, nullptr, nullptr, wc.hInstance, nullptr);
    DXGI_SWAP_CHAIN_DESC swap{}; swap.BufferDesc.Width = 4; swap.BufferDesc.Height = 2;
    swap.BufferDesc.Format = DXGI_FORMAT_R8G8B8A8_UNORM; swap.SampleDesc.Count = 1;
    swap.BufferCount = 1; swap.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT; swap.OutputWindow = window; swap.Windowed = TRUE;
    ComPtr<ID3D11Device> device; ComPtr<ID3D11DeviceContext> context; ComPtr<IDXGISwapChain> chain;
    HRESULT made = D3D11CreateDeviceAndSwapChain(nullptr, D3D_DRIVER_TYPE_WARP, nullptr, D3D11_CREATE_DEVICE_DEBUG, nullptr, 0,
        D3D11_SDK_VERSION, &swap, &chain, &device, nullptr, &context);
    if (FAILED(made)) made = D3D11CreateDeviceAndSwapChain(nullptr, D3D_DRIVER_TYPE_WARP, nullptr, 0, nullptr, 0,
        D3D11_SDK_VERSION, &swap, &chain, &device, nullptr, &context);
    if (FAILED(made)) { DestroyWindow(window); CoUninitialize(); return 77; }
    auto image = [&](DXGI_FORMAT format, const void* bytes, uint32_t pitch) {
        D3D11_TEXTURE2D_DESC desc{}; desc.Width = 4; desc.Height = 2; desc.Format = format;
        desc.MipLevels = desc.ArraySize = desc.SampleDesc.Count = 1;
        D3D11_SUBRESOURCE_DATA data{bytes, pitch, 0};
        ComPtr<ID3D11Texture2D> texture;
        check(SUCCEEDED(device->CreateTexture2D(&desc, &data, &texture)), "Fixture texture must create.");
        return texture;
    };
    const std::array<uint16_t, 32> hdr{{0x3c00,0x4000,0x3800,0, 0,0,0,0, 0x4400,0,0,0, 0,0x4400,0,0,
        0,0,0x4400,0, 0x3c00,0x3c00,0x3c00,0, 0x7c00,0x7e00,0,0, 0,0,0,0}};
    const std::array<uint32_t, 8> depth{{0,1,0x7fffff,0xffffff,0,100,1000,10000}};
    const std::array<float, 16> motion{{0,0, 1,-1, -1,1, -1000,-1000, 0,0, 2,3, 4,5, 6,7}};
    const std::array<uint8_t, 8> mask{{0,255,64,128,0,255,64,128}};
    const std::array<uint32_t, 8> hud{{0x000000ff,0x8000ff00,0xffff0000,0xffffffff,0,0,0,0}};
    auto color = image(DXGI_FORMAT_R16G16B16A16_FLOAT, hdr.data(), 32);
    auto z = image(DXGI_FORMAT_R24G8_TYPELESS, depth.data(), 16);
    auto vectors = image(DXGI_FORMAT_R32G32_FLOAT, motion.data(), 32);
    auto reactive = image(DXGI_FORMAT_R8_UNORM, mask.data(), 4);
    auto ui = image(DXGI_FORMAT_R8G8B8A8_UNORM, hud.data(), 16);
    if (!passed) return 1;
    const auto root = std::filesystem::path(argv[1]) / ("input-dump-" + std::to_string(GetCurrentProcessId()) + "-" + std::to_string(GetTickCount64()));
    rsf_input_dump_configure(root.string().c_str(), "AC7", "fixture", nullptr, nullptr);
    rsf_input_dump_request();
    rsf_game_render_pass pass{}; pass.struct_size = sizeof(pass); pass.role = RSF_GAME_RENDER_SR;
    pass.session_id = 9; pass.source_frame_id = 101; pass.native_frame = 33; pass.view_key = 11;
    pass.family_key = 12; pass.scope_id = 55; pass.viewport_key = 13; pass.flags = RSF_GAME_RENDER_PRIMARY;
    pass.render_rect[2] = pass.output_rect[2] = 4; pass.render_rect[3] = pass.output_rect[3] = 2;
    rsf_input_dump_pass(context.Get(), &pass, 1);
    check(rsf_input_dump_active() != 0, "Primary source frame must arm SR readback.");
    ComPtr<ID3D11Predicate> predicate;
    const D3D11_QUERY_DESC query{D3D11_QUERY_OCCLUSION_PREDICATE, 0};
    check(SUCCEEDED(device->CreatePredicate(&query, &predicate)), "Create capture predication fixture.");
    if (predicate) { context->Begin(predicate.Get()); context->End(predicate.Get()); context->SetPredication(predicate.Get(), TRUE); }
    rsf_input_dump_texture(context.Get(), "sr_source_jittered", "fixture HDR", color.Get(), RSF_DUMP_VIEW_HDR, 1);
    ComPtr<ID3D11Predicate> restored; BOOL restored_value = FALSE; context->GetPredication(&restored, &restored_value);
    check(restored.Get() == predicate.Get() && restored_value == TRUE, "Readback must restore the exact game predicate and value.");
    context->SetPredication(nullptr, FALSE);
    rsf_input_dump_texture(context.Get(), "sr_reactive_mask", "fixture mask", reactive.Get(), RSF_DUMP_VIEW_SCALAR, 1);
    rsf_input_dump_texture(context.Get(), "sr_output", "fixture output", color.Get(), RSF_DUMP_VIEW_HDR, 1);
    rsf_input_dump_texture(context.Get(), "sr_game_motion", "fixture game vectors", vectors.Get(), RSF_DUMP_VIEW_DECODED_MOTION, 1);
    rsf_input_dump_texture(context.Get(), "sr_motion_decoded", "fixture sparse motion", vectors.Get(), RSF_DUMP_VIEW_DECODED_MOTION, 1);
    rsf_dlss_frame frame{}; frame.struct_size = sizeof(frame); frame.color_in = color.Get(); frame.depth = z.Get(); frame.motion = vectors.Get();
    frame.render_width = frame.output_width = 4; frame.render_height = frame.output_height = 2;
    frame.far_plane = std::numeric_limits<float>::infinity(); frame.jitter_x = .25f; frame.jitter_y = -.125f;
    frame.motion_scale_x = .25f; frame.motion_scale_y = .5f; frame.camera_motion_included = frame.depth_inverted = 1;
    rsf_input_dump_sr(context.Get(), &frame, 1);
    rsf_input_dump_fg(context.Get(), &pass, &frame);
    rsf_input_dump_sr_result(0);
    rsf_input_dump_pass(context.Get(), &pass, 0);
    check(!rsf_input_dump_active(), "SR scope must end at its real callback boundary.");
    pass.role = RSF_GAME_RENDER_UI_COMPOSITE; pass.color_input = pass.color_output = ui.Get(); pass.ui_input = ui.Get();
    auto wrong = pass; wrong.source_frame_id = 102;
    rsf_input_dump_pass(context.Get(), &wrong, 1);
    rsf_input_dump_pass(context.Get(), &wrong, 0);
    std::filesystem::path bundle;
    for (const auto& entry : std::filesystem::directory_iterator(root / "website")) bundle = entry.path();
    check(!std::filesystem::exists(bundle / "hud_color.bin"), "A different source frame must not supply HUD data.");
    rsf_input_dump_pass(context.Get(), &pass, 1); rsf_input_dump_pass(context.Get(), &pass, 0);
    pass.role = RSF_GAME_RENDER_FINAL_SCENE; pass.scene_surface = ui.Get();
    rsf_input_dump_pass(context.Get(), &pass, 0);
    ComPtr<ID3D11Texture2D> buffer;
    check(SUCCEEDED(chain->GetBuffer(0, IID_PPV_ARGS(&buffer))), "Read swapchain buffer.");
    context->CopyResource(buffer.Get(), ui.Get());
    pass.role = RSF_GAME_RENDER_WINDOW; pass.window_key = 14; pass.rhi_viewport_key = 15; pass.swapchain = chain.Get();
    wrong = pass; wrong.source_frame_id = 102;
    check(rsf_native_window_scope(&wrong, 1) != 0, "Begin different window frame.");
    rsf_input_dump_present(chain.Get(), context.Get());
    rsf_native_window_scope(&wrong, 0);
    check(!std::filesystem::exists(bundle / "final_color.bin"), "A different presented frame must not complete the bundle.");
    check(rsf_native_window_scope(&pass, 1) != 0, "Begin matched window.");
    rsf_input_dump_present(chain.Get(), context.Get());
    rsf_frame_record record{}; record.struct_size = sizeof(record); record.session_id = 9; record.frame_id = 101;
    record.render_width = record.output_width = 4; record.render_height = record.output_height = 2; record.frame_time_ms = 16.67f;
    rsf_fg_frame fg{}; fg.struct_size = sizeof(fg); fg.record = &record; fg.interpolate = 1;
    fg.motion_scale_x = fg.motion_scale_y = 1;
    rsf_input_dump_fg_prepare(&pass, &fg, 0);
    rsf_input_dump_presented(chain.Get(), S_OK); rsf_native_window_scope(&pass, 0);
    const auto metadata = read(bundle / "manifest.json");
    check(metadata.find("\"status\":\"complete\"") != std::string::npos, "Matched handoffs must complete the manifest.");
    check(metadata.find("\"status\":\"unavailable\"") != std::string::npos, "Absent optional inputs must be explicit.");
    check(read(bundle / "sr_constants.json").find("\"far_plane\":null") != std::string::npos, "Infinite far plane must remain valid JSON.");
    check(read(bundle / "fg_constants.json").find("\"source_frame_id\":101") != std::string::npos, "FG preparation metadata must join the same source frame before Present completion.");
    check(read(bundle / "sr_source_jittered.bin") == std::string(reinterpret_cast<const char*>(hdr.data()), sizeof(hdr)), "HDR raw bytes must be bit exact, including alpha and nonfinite values.");
    const auto display = png(bundle / "sr_source_jittered.png", 4, 2);
    check(display[0] >= 187 && display[0] <= 189 && display[1] >= 212 && display[1] <= 214 && display[2] >= 155 && display[2] <= 157,
        "HDR preview must use Reinhard plus sRGB rather than clip linear HDR.");
    const auto alpha = png(bundle / "hud_alpha.png", 4, 2);
    check(alpha[0] == 0 && alpha[4] == 128 && alpha[8] == 255, "HUD alpha must be visible in its separate opaque preview.");
    const auto mask_png = png(bundle / "sr_reactive_mask.png", 4, 2);
    check(mask_png[4] == 255 && mask_png[5] == 255 && mask_png[6] == 255, "Scalar mask must replicate to RGB.");
    const auto depth_png = png(bundle / "sr_depth.png", 4, 2);
    check(depth_png[0] == 0 && depth_png[12] == 255 && depth_png[8] > depth_png[4], "Reversed-Z depth preview must expose its range.");
    const auto motion_png = png(bundle / "sr_motion_decoded.png", 4, 2);
    check(motion_png[0] == 128 && motion_png[1] == 128 && motion_png[12] == 0 && motion_png[14] == 220,
        "Valid zero motion and the unwritten sentinel must stay distinct.");
    const auto dense_png = png(bundle / "sr_motion.png", 4, 2);
    check(dense_png[12] == 0 && dense_png[13] == 0 && dense_png[14] == 128, "Large negative dense displacement must remain valid motion.");
    png(bundle / "hud_color.png", 4, 2); png(bundle / "fg_hudless_color.png", 4, 2); png(bundle / "final_color.png", 4, 2);
    rsf_texture_dump_options options{sizeof(options), RSF_TEXTURE_DUMP_ABI_VERSION, "unused", RSF_DUMP_VIEW_RAW, 1, nullptr, nullptr};
    ComPtr<ID3D11DeviceContext> deferred; device->CreateDeferredContext(0, &deferred);
    check(rsf_dump_texture_png(device.Get(), deferred.Get(), color.Get(), &options, nullptr) == RSF_TEXTURE_ERROR_INVALID_ARGUMENT,
        "Deferred contexts must refuse before readback.");
    ComPtr<ID3D11InfoQueue> info;
    if (SUCCEEDED(device.As(&info))) for (UINT64 i = 0; i < info->GetNumStoredMessages(); ++i) {
        SIZE_T length = 0; info->GetMessage(i, nullptr, &length); std::vector<unsigned char> storage(length);
        auto* message = reinterpret_cast<D3D11_MESSAGE*>(storage.data()); info->GetMessage(i, message, &length);
        check(message->Severity != D3D11_MESSAGE_SEVERITY_ERROR && message->Severity != D3D11_MESSAGE_SEVERITY_CORRUPTION, message->pDescription);
    }
    std::printf("Website fixture bundle: %s\n", bundle.string().c_str());
    rsf_native_window_reset(); buffer.Reset(); chain.Reset(); context->ClearState(); context.Reset(); device.Reset();
    DestroyWindow(window); CoUninitialize();
    return passed ? 0 : 1;
}
