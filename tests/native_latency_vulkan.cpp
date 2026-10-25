// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/native_latency_vulkan.h>
#include <rescaleframe/native_streamline_vulkan.h>
#include <array>
#include <cstdio>
#include <cstdlib>
#include <vector>
namespace {
std::array<uint32_t, 6> sdk_tokens{};
uint32_t cursor = 0, slept = 0, options = 0;
uint32_t low_latency = 1;
uintptr_t current = 0;
uint32_t requested_sr = 0, requested_fg = 0, leased_calls = 0;
struct Marker { uint32_t frame, kind; };
std::vector<Marker> markers;
void require(bool condition, const char* message) {
    if (!condition) { std::fprintf(stderr, "%s\n", message); std::exit(1); }
}
void cpu(uint64_t frame, uint32_t stage, uint32_t expected = 1) {
    rsf_game_cpu_event event{}; event.struct_size = sizeof(event); event.session_id = 1;
    event.source_frame_id = frame; event.stage = stage; event.render_expected = expected;
    rsf_latencyvk_cpu(&event);
}
}
// Synthetic SDK boundary. These tests validate ownership and identity, not GPU
// support, actual Reflex sleep, or the Streamline implementation.
extern "C" {
int VulkanStorySlInitialize(const wchar_t*, const char*, uint32_t, uint32_t) { return 0; }
int VulkanStorySlInitializeFeatures(const wchar_t*, const char*, uint32_t, uint32_t fg, uint32_t sr) { requested_sr = sr; requested_fg = fg; return 0; }
void* VulkanStorySlModule() { return reinterpret_cast<void*>(1); }
int VulkanStorySlShutdownChecked() { return 0; }
rsf_vk_function VulkanStorySlGetInstanceProcAddr(void*, const char*) { return nullptr; }
int VulkanStorySlBindReflex() { return 0; }
int VulkanStorySlBindPcl() { return 0; }
uint32_t VulkanStorySlPclWindowMessage() { return 0xc123; }
int VulkanStorySlGetReflexState(uint32_t* available, uint32_t* reports) { *available = low_latency; *reports = 1; return 0; }
int VulkanStorySlSetReflexInterval(int mode, uint32_t interval) { require(mode == 1 && interval == 12345, "microsecond interval preserved"); ++options; return 0; }
int VulkanStorySlBeginFrame(uint32_t frame) {
    sdk_tokens[cursor] = frame; current = reinterpret_cast<uintptr_t>(&sdk_tokens[cursor]); cursor = (cursor + 1) % sdk_tokens.size(); return 0;
}
uintptr_t VulkanStorySlCurrentFrameToken() { return current; }
int VulkanStorySlTokenMatches(uintptr_t token, uint32_t index) { return token && *reinterpret_cast<uint32_t*>(token) == index; }
int VulkanStorySlReflexSleep() { ++slept; return 0; }
int VulkanStorySlMarkerForToken(uintptr_t token, uint32_t kind) { markers.push_back({*reinterpret_cast<uint32_t*>(token), kind}); return 0; }
int VulkanStorySlQueuePresentSynchronized(uintptr_t, void*, const void*) { return 0; }
int VulkanStorySlVulkanWsiCall(rsf_vk_wsi_call callback, void* user) { return callback(user); }
int VulkanStorySlWithoutDlssG(rsf_vk_wsi_call callback, void* user) { return callback(user); }
}
int main() {
    require(rsf_slvk_without_dlssg(nullptr, nullptr) == -3, "missing foreign WSI callback refused");
    int foreign_wsi = 7;
    require(rsf_slvk_without_dlssg([](void* user) -> int32_t { return *static_cast<int*>(user); }, &foreign_wsi) == 7,
        "foreign WSI callback result preserved");
    rsf_slvk_call lease{sizeof(lease), RSF_SLVK_ABI_VERSION, 1, 1,
        [](void* module, uintptr_t token, void*) -> rsf_backend_result {
            require(module == reinterpret_cast<void*>(1) && token && *reinterpret_cast<uint32_t*>(token) == 1, "matching source token leased");
            ++leased_calls; return RSF_BACKEND_ERROR_FEATURE_FAILED;
        }, nullptr};
    require(rsf_slvk_invoke(nullptr) == RSF_BACKEND_ERROR_INVALID_ARGUMENT &&
        rsf_slvk_invoke(&lease) == RSF_BACKEND_ERROR_NOT_READY, "no SDK lease before initialization");
    auto malformed = lease; malformed.struct_size--;
    require(rsf_slvk_invoke(&malformed) == RSF_BACKEND_ERROR_INVALID_ARGUMENT, "short SDK lease refused");
    malformed = lease; malformed.abi_version++;
    require(rsf_slvk_invoke(&malformed) == RSF_BACKEND_ERROR_ABI_MISMATCH, "SDK lease ABI mismatch refused");
    malformed = lease; malformed.callback = nullptr;
    require(rsf_slvk_invoke(&malformed) == RSF_BACKEND_ERROR_INVALID_ARGUMENT, "null SDK callback refused");
    malformed = lease; malformed.session_id = 0;
    require(rsf_slvk_invoke(&malformed) == RSF_BACKEND_ERROR_INVALID_ARGUMENT, "partial frame identity refused");
    require(rsf_latencyvk_initialize(nullptr) == RSF_BACKEND_ERROR_INVALID_ARGUMENT, "null setup refused");
    rsf_latencyvk_setup setup{sizeof(setup) - 1, RSF_LATENCYVK_ABI_VERSION, "unused", "synthetic"};
    require(rsf_latencyvk_initialize(&setup) == RSF_BACKEND_ERROR_INVALID_ARGUMENT, "short setup refused");
    setup.struct_size = sizeof(setup); setup.abi_version = 999;
    require(rsf_latencyvk_initialize(&setup) == RSF_BACKEND_ERROR_ABI_MISMATCH, "ABI mismatch refused");
    setup.abi_version = RSF_LATENCYVK_ABI_VERSION;
    require(rsf_latencyvk_initialize(&setup) == RSF_BACKEND_OK, "synthetic initialization");
    require(requested_sr == 0 && requested_fg == 0, "legacy latency ABI does not load SR or FG");
    require(rsf_latencyvk_options(3, 0) == RSF_BACKEND_ERROR_INVALID_ARGUMENT, "unknown mode refused");
    require(rsf_latencyvk_options(1, 12345) == RSF_BACKEND_OK, "options queued");
    cpu(1, RSF_GAME_CPU_FRAME_BEGIN); cpu(1, RSF_GAME_CPU_PACING); cpu(1, RSF_GAME_CPU_PACING);
    require(rsf_slvk_invoke(&lease) == RSF_BACKEND_ERROR_FEATURE_FAILED && leased_calls == 1, "callback failure preserved");
    auto foreign = lease; foreign.session_id = 2;
    require(rsf_slvk_invoke(&foreign) == RSF_BACKEND_ERROR_NOT_READY && leased_calls == 1, "foreign session lease refused");
    cpu(1, RSF_GAME_CPU_SIMULATION_BEGIN); cpu(1, RSF_GAME_CPU_SIMULATION_BEGIN); cpu(1, RSF_GAME_CPU_SIMULATION_END);
    require(slept == 1 && options == 1, "one pacing call per CPU frame");
    const size_t before_input = markers.size();
    require(rsf_latencyvk_input(1, 1, 0) == RSF_BACKEND_OK && rsf_latencyvk_input(1, 2, 0) == RSF_BACKEND_OK && markers.size() == before_input, "keyboard/mouse cannot spoof controller or deprecated markers");
    require(rsf_latencyvk_input(1, 4, 0) == RSF_BACKEND_OK && rsf_latencyvk_input(1, 4, 0) == RSF_BACKEND_OK && markers.size() == before_input + 1, "controller sampled once per source");
    require(rsf_latencyvk_input(1, 0, 0xc124) == RSF_BACKEND_ERROR_INVALID_ARGUMENT, "foreign Windows message refused");
    require(rsf_latencyvk_input(1, 0, 0xc123) == RSF_BACKEND_OK, "registered latency ping");
    rsf_latencyvk_render(1); rsf_latencyvk_present(1); rsf_latencyvk_present(0);
    const size_t completed = markers.size();
    rsf_latencyvk_present(1); rsf_latencyvk_present(0);
    require(markers.size() == completed, "secondary Present cannot reuse completed source identity");
    require(markers.front().frame == 1 && markers.front().kind == 0 && markers.back().kind == 5, "simulation through Present on one source token");
    cpu(2, RSF_GAME_CPU_FRAME_BEGIN); cpu(2, RSF_GAME_CPU_PACING); cpu(2, RSF_GAME_CPU_FRAME_END, 0);
    const auto newest = current;
    require(rsf_slvk_invoke(&lease) == RSF_BACKEND_ERROR_FEATURE_FAILED && current == newest, "older render lease cannot rewind pacing token");
    require(slept == 2 && options == 2, "unchanged options refreshed for a possible swapchain successor");
    rsf_latencyvk_render(2); rsf_latencyvk_present(1); rsf_latencyvk_present(0);
    require(markers.size() == completed, "nonrendered frame aborted");
    for (uint64_t frame = 3; frame < 10; ++frame) cpu(frame, RSF_GAME_CPU_FRAME_BEGIN);
    require(rsf_slvk_invoke(&lease) == RSF_BACKEND_ERROR_NOT_READY && leased_calls == 2, "expired render lease refused");
    cpu(8, RSF_GAME_CPU_PACING);
    require(slept == 2, "late pacing event cannot sleep with a newer frame token");
    rsf_latencyvk_render(1); rsf_latencyvk_present(1); rsf_latencyvk_present(0);
    require(markers.size() == completed, "expired SDK ring token refused");
    require(rsf_latencyvk_input(1, 1, 0) == RSF_BACKEND_ERROR_NOT_READY, "stale input identity refused");
    rsf_latencyvk_status status{sizeof(status), 999};
    require(rsf_latencyvk_get_status(&status) == RSF_BACKEND_ERROR_ABI_MISMATCH, "status ABI checked");
    status.abi_version = RSF_LATENCYVK_ABI_VERSION;
    require(rsf_latencyvk_get_status(&status) == RSF_BACKEND_OK && status.sleep_calls == 2 && status.marker_calls == completed, "accepted SDK calls counted");
    require(rsf_latencyvk_shutdown() == RSF_BACKEND_OK && rsf_latencyvk_shutdown() == RSF_BACKEND_OK, "idempotent shutdown");
    require(rsf_latencyvk_options(1, 0) == RSF_BACKEND_ERROR_NOT_SUPPORTED, "no latency claim after shutdown");
    require(rsf_latencyvk_initialize(&setup) == RSF_BACKEND_OK && rsf_latencyvk_options(1, 12345) == RSF_BACKEND_OK, "fresh session initialized");
    cpu(1, RSF_GAME_CPU_FRAME_BEGIN); cpu(1, RSF_GAME_CPU_PACING);
    require(slept == 3 && options == 3, "options reapplied after SDK reinitialization");
    require(rsf_latencyvk_get_status(&status) == RSF_BACKEND_OK && status.sleep_calls == 1 && status.marker_calls == 0, "fresh session counters");
    require(rsf_latencyvk_shutdown() == RSF_BACKEND_OK, "fresh session retired");
    low_latency = 0;
    require(rsf_latencyvk_initialize(&setup) == RSF_BACKEND_OK, "stats-only session initialized");
    require(rsf_latencyvk_options(1, 0) == RSF_BACKEND_ERROR_NOT_SUPPORTED && rsf_latencyvk_options(0, 12345) == RSF_BACKEND_ERROR_NOT_SUPPORTED, "unavailable Reflex mode and limiter refused");
    require(rsf_latencyvk_options(0, 0) == RSF_BACKEND_OK && rsf_latencyvk_shutdown() == RSF_BACKEND_OK, "stats-only PCL remains available");
    rsf_slvk_setup shared{sizeof(shared), RSF_SLVK_ABI_VERSION, "unused", "synthetic", RSF_SLVK_DLSS_SR};
    auto invalid_setup = shared; invalid_setup.struct_size--;
    require(rsf_slvk_initialize(&invalid_setup) == RSF_BACKEND_ERROR_INVALID_ARGUMENT, "short shared SDK setup refused");
    invalid_setup = shared; invalid_setup.abi_version++;
    require(rsf_slvk_initialize(&invalid_setup) == RSF_BACKEND_ERROR_ABI_MISMATCH, "shared SDK ABI mismatch refused");
    invalid_setup = shared; invalid_setup.features = 4;
    require(rsf_slvk_initialize(&invalid_setup) == RSF_BACKEND_ERROR_INVALID_ARGUMENT, "unknown shared SDK features refused");
    require(rsf_slvk_initialize(&shared) == RSF_BACKEND_OK && requested_sr == 1, "SR requested on shared SDK session");
    lease.session_id = lease.frame_id = 0;
    lease.callback = [](void* module, uintptr_t token, void*) -> rsf_backend_result {
        require(module && !token, "non-frame lease has no fabricated source token"); return RSF_BACKEND_OK;
    };
    require(rsf_slvk_invoke(&lease) == RSF_BACKEND_OK && rsf_latencyvk_shutdown() == RSF_BACKEND_OK, "non-frame SDK operations serialized");
    shared.features = RSF_SLVK_DLSS_SR | RSF_SLVK_DLSS_FG;
    require(rsf_slvk_initialize(&shared) == RSF_BACKEND_OK && requested_sr == 1 && requested_fg == 1,
        "SR and FG requested together on shared SDK session");
    require(rsf_latencyvk_shutdown() == RSF_BACKEND_OK, "shared SR and FG session retired");
    return 0;
}
