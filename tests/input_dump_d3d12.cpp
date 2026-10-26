// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/input_dump_d3d12.h>
#include "../runtime/backends/common/d3d12_helpers.h"
#include <d3d12sdklayers.h>
#include <wrl/client.h>
#include <dxgi1_4.h>
#include <array>
#include <cstdio>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <iterator>
#include <vector>

using Microsoft::WRL::ComPtr;
namespace {
bool passed = true;
void check(bool okay, const char* message) { if (!okay) { std::fprintf(stderr, "%s\n", message); passed = false; } }
std::string read(const std::filesystem::path& path) { std::ifstream in(path, std::ios::binary); return {std::istreambuf_iterator<char>(in), {}}; }
}
int main(int argc, char** argv)
{
    if (argc != 2) return 2;
    SetEnvironmentVariableW(L"RSF_FRAME_CAPTURE_DIR", nullptr);
    ComPtr<ID3D12Debug> debug;
    if (SUCCEEDED(D3D12GetDebugInterface(IID_PPV_ARGS(&debug)))) debug->EnableDebugLayer();
    ComPtr<IDXGIFactory4> factory; ComPtr<IDXGIAdapter> warp; ComPtr<ID3D12Device> device;
    if (FAILED(CreateDXGIFactory1(IID_PPV_ARGS(&factory))) || FAILED(factory->EnumWarpAdapter(IID_PPV_ARGS(&warp))) ||
        FAILED(D3D12CreateDevice(warp.Get(), D3D_FEATURE_LEVEL_11_0, IID_PPV_ARGS(&device)))) return 77;
    D3D12_COMMAND_QUEUE_DESC q{}; q.Type = D3D12_COMMAND_LIST_TYPE_DIRECT;
    ComPtr<ID3D12CommandQueue> queue; ComPtr<ID3D12CommandAllocator> allocator; ComPtr<ID3D12GraphicsCommandList> list;
    check(SUCCEEDED(device->CreateCommandQueue(&q, IID_PPV_ARGS(&queue))), "Create capture queue.");
    check(rsf::create_command_list(device.Get(), q.Type, &allocator, &list), "Create capture commands.");
    ComPtr<ID3D12Resource> source;
    check(rsf::create_uav_texture(device.Get(), 4, 2, DXGI_FORMAT_R8G8B8A8_UNORM, &source), "Create source texture.");
    if (!passed) return 1;
    const auto desc = source->GetDesc();
    D3D12_PLACED_SUBRESOURCE_FOOTPRINT footprint{}; UINT rows = 0; UINT64 row_bytes = 0, bytes = 0;
    device->GetCopyableFootprints(&desc, 0, 1, 0, &footprint, &rows, &row_bytes, &bytes);
    D3D12_HEAP_PROPERTIES heap{}; heap.Type = D3D12_HEAP_TYPE_UPLOAD;
    D3D12_RESOURCE_DESC buffer{}; buffer.Dimension = D3D12_RESOURCE_DIMENSION_BUFFER; buffer.Width = bytes;
    buffer.Height = 1; buffer.DepthOrArraySize = 1; buffer.MipLevels = 1; buffer.SampleDesc.Count = 1;
    buffer.Layout = D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    ComPtr<ID3D12Resource> upload;
    check(SUCCEEDED(device->CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &buffer, D3D12_RESOURCE_STATE_GENERIC_READ,
        nullptr, IID_PPV_ARGS(&upload))), "Create padded source upload.");
    const std::array<uint32_t, 8> pixels{{0x00ff0000, 0x8000ff00, 0xffff0000, 0xffffffff, 0, 0xff112233, 0xff445566, 0xff778899}};
    void* mapped = nullptr; check(SUCCEEDED(upload->Map(0, nullptr, &mapped)), "Map source upload.");
    if (!passed) return 1;
    for (uint32_t y = 0; y < 2; ++y) std::memcpy(static_cast<char*>(mapped) + footprint.Offset + size_t(y) * footprint.Footprint.RowPitch,
        pixels.data() + y * 4, 16);
    upload->Unmap(0, nullptr);
    check(SUCCEEDED(allocator->Reset()) && SUCCEEDED(list->Reset(allocator.Get(), nullptr)), "Begin source recording.");
    rsf::transition(list.Get(), source.Get(), D3D12_RESOURCE_STATE_UNORDERED_ACCESS, D3D12_RESOURCE_STATE_COPY_DEST);
    D3D12_TEXTURE_COPY_LOCATION from{}; from.pResource = upload.Get(); from.Type = D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT; from.PlacedFootprint = footprint;
    D3D12_TEXTURE_COPY_LOCATION to{}; to.pResource = source.Get(); to.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
    list->CopyTextureRegion(&to, 0, 0, 0, &from, nullptr);
    rsf::transition(list.Get(), source.Get(), D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
    const auto root = std::filesystem::path(argv[1]) / ("core-dump-dx12-" + std::to_string(GetCurrentProcessId()) + '-' + std::to_string(GetTickCount64()));
    rsf_input_dump_configure(root.string().c_str(), "FixtureD3D12", "fixture", nullptr, nullptr);
    rsf_input_dump_request();
    rsf_frame_record record{}; record.struct_size = sizeof(record); record.abi_version = RSF_GAME_FRAME_ABI_VERSION;
    record.session_id = 13; record.frame_id = 17; record.view_id = 3;
    record.render_width = record.output_width = 4; record.render_height = record.output_height = 2;
    check(rsf_input_dump_begin_record(&record, 3) != 0, "Core capture must accept a generic frame without an AC7 pass or swapchain.");
    auto other = record; ++other.view_id;
    check(!rsf_input_dump_wants(&other), "Another view must not fill a capture.");
    check(!rsf_input_dump_reserve(&record, "../escape", "invalid", 0, 1), "Image names must stay inside the bundle.");
    rsf_input_dump12* capture = nullptr;
    check(rsf_input_dump12_create(&capture) == 0 && capture, "Create the public C ABI reader.");
    check(rsf_input_dump12_record(capture, list.Get(), source.Get(), &record, "sr_source_jittered", D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE, 0, RSF_DUMP_VIEW_RAW, 1) == 0, "Record actual source readback.");
    check(rsf_input_dump12_record(capture, list.Get(), source.Get(), &record, "final_color", D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE, 0, RSF_DUMP_VIEW_RAW, 1) == 0, "Record actual final readback.");
    check(rsf_input_dump12_destroy(capture) == RSF_BACKEND_ERROR_NOT_READY, "Unsubmitted readback resources must remain leased.");
    source.Reset(); // The reader keeps the native resource alive through its queued copy.
    std::filesystem::path bundle;
    for (const auto& entry : std::filesystem::directory_iterator(root / "website")) bundle = entry.path();
    rsf_input_dump_complete(&record, 0);
    check(read(bundle / "manifest.json").find("waiting_for_gpu") != std::string::npos, "Present completion must not claim unexecuted copies are written.");
    check(!std::filesystem::exists(bundle / "final_color.bin"), "Readbacks must not be mapped before actual submission completion.");
    check(SUCCEEDED(list->Close()), "Close source commands.");
    ID3D12CommandList* commands[]{list.Get()}; queue->ExecuteCommandLists(1, commands);
    check(rsf_input_dump12_submitted(capture, queue.Get(), 0) == 0, "Fence the exact capture submission.");
    ComPtr<ID3D12Fence> completed; check(SUCCEEDED(device->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&completed))), "Create test completion fence.");
    const HANDLE event = CreateEventW(nullptr, FALSE, FALSE, nullptr);
    check(event && SUCCEEDED(queue->Signal(completed.Get(), 1)) && SUCCEEDED(completed->SetEventOnCompletion(1, event)) &&
        WaitForSingleObject(event, 15000) == WAIT_OBJECT_0, "Observe submitted GPU completion.");
    if (event) CloseHandle(event);
    check(rsf_input_dump12_poll(capture) == 0, "Write completed GPU snapshots through the C ABI.");
    check(read(bundle / "final_color.bin") == std::string(reinterpret_cast<const char*>(pixels.data()), sizeof(pixels)), "Pack GPU-padded rows without changing source bytes or alpha.");
    check(read(bundle / "manifest.json").find("\"status\":\"complete\"") != std::string::npos, "Complete only after all reservations retire.");
    const auto png = read(bundle / "final_color.png");
    check(png.size() > 8 && png.substr(1, 3) == "PNG", "Write the shared opaque PNG preview from the D3D12 readback.");
    const auto stale = rsf_input_dump_reserve(&record, "stale", "closed", 0, 1);
    check(!stale, "Closed requests refuse late reservations.");
    check(rsf_input_dump12_destroy(capture) == 0, "Retire completed readbacks without borrowing C++ ownership.");
    rsf_input_dump_request(); ++record.frame_id;
    check(rsf_input_dump_begin_record(&record, 0) != 0 && rsf_input_dump12_create(&capture) == 0, "Begin an abandoned-recording case.");
    check(rsf::create_uav_texture(device.Get(), 4, 2, DXGI_FORMAT_R8G8B8A8_UNORM, &source), "Create an unsubmitted source.");
    check(SUCCEEDED(allocator->Reset()) && SUCCEEDED(list->Reset(allocator.Get(), nullptr)), "Begin an unsubmitted slot.");
    check(rsf_input_dump12_record(capture, list.Get(), source.Get(), &record, "abandoned", D3D12_RESOURCE_STATE_UNORDERED_ACCESS, 1, RSF_DUMP_VIEW_RAW, 1) == 0,
        "Record an image that will be discarded before submission.");
    check(SUCCEEDED(list->Close()) && SUCCEEDED(allocator->Reset()) && SUCCEEDED(list->Reset(allocator.Get(), nullptr)) && SUCCEEDED(list->Close()),
        "Discard the actual unsubmitted command recording before releasing its readback.");
    rsf_input_dump12_abandon(capture, 1);
    check(rsf_input_dump12_destroy(capture) == 0, "An explicitly abandoned recording must not leak or claim GPU execution.");
    rsf_input_dump_cancel(record.session_id);
    ComPtr<ID3D12InfoQueue> info;
    if (SUCCEEDED(device.As(&info))) for (UINT64 i = 0; i < info->GetNumStoredMessages(); ++i) {
        SIZE_T size = 0; info->GetMessage(i, nullptr, &size); std::vector<unsigned char> bytes_message(size);
        auto* message = reinterpret_cast<D3D12_MESSAGE*>(bytes_message.data()); info->GetMessage(i, message, &size);
        check(message->Severity != D3D12_MESSAGE_SEVERITY_ERROR && message->Severity != D3D12_MESSAGE_SEVERITY_CORRUPTION, message->pDescription);
    }
    std::printf("D3D12 core dump fixture: %s\n", bundle.string().c_str());
    return passed ? 0 : 1;
}
