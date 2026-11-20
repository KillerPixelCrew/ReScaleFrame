// SPDX-License-Identifier: MIT
#include <rescaleframe/native_latency_vulkan.h>
#include <rescaleframe/native_streamline_vulkan.h>
#include <rescaleframe/native_fg_vulkan.h>
#include <rescaleframe/native_cpu.h>
#include <rescaleframe/frame_generation.h>
#include <array>
#include <cstdio>
#include <cstdlib>
#include <mutex>
#include <vector>
#include <windows.h>
#include <thread>
#include <condition_variable>
#include <chrono>
#include "../runtime/orchestrator/src/native_latency_vulkan_presentation.h"
namespace {
std::array<uint32_t, 6> sdk_tokens{};
uint32_t cursor = 0, slept = 0, options = 0;
uint32_t low_latency = 1;
bool shutdown_allowed=true, pcl_bind_fail=false;
uint32_t pcl_binds=0; DWORD pcl_bound_thread=0;
thread_local bool check_input_registration=false;
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
namespace {std::mutex sleep_guard;std::condition_variable sleep_changed;bool block_xell=false,xell_entered=false,release_xell=false;
bool block_reflex=false,reflex_entered=false,release_reflex=false;
uint32_t xell_sleeps{},xell_mode{}; std::vector<Marker> xell_markers;}
extern "C" rsf_backend_result rsf_fgvk_backend(const rsf_fgvk* context,uint32_t* backend) {
    if (!context || !backend) return RSF_BACKEND_ERROR_INVALID_ARGUMENT; *backend=5;return RSF_BACKEND_OK;
}
extern "C" rsf_backend_result rsf_fgvk_latency_options(rsf_fgvk*,uint32_t mode,uint32_t) {xell_mode=mode;return RSF_BACKEND_OK;}
extern "C" rsf_backend_result rsf_fgvk_latency_sleep(rsf_fgvk* context,uint64_t) {
    require(context!=nullptr,"XeLL sleep holds a real context lease");++xell_sleeps;
    std::unique_lock lock(sleep_guard);
    if(block_xell){xell_entered=true;sleep_changed.notify_all();sleep_changed.wait(lock,[]{return release_xell;});}
    return RSF_BACKEND_OK;
}
extern "C" rsf_backend_result rsf_fgvk_latency_marker(rsf_fgvk*,uint64_t frame,uint32_t kind) {
    xell_markers.push_back({uint32_t(frame),kind});return RSF_BACKEND_OK;
}
extern "C" rsf_backend_result rsf_fgvk_latency_get_status(rsf_fgvk*,rsf_fgvk_latency_status* value) {
    *value={sizeof(*value),RSF_FGVK_LATENCY_ABI_VERSION,xell_mode,1,xell_sleeps,xell_markers.size(),0};return RSF_BACKEND_OK;
}
// Synthetic SDK boundary. These tests validate ownership and identity, not GPU
// support, actual Reflex sleep, or the Streamline implementation.
extern "C" {
int VulkanStorySlInitialize(const wchar_t*, const char*, uint32_t, uint32_t) { return 0; }
int VulkanStorySlInitializeFeatures(const wchar_t*, const char*, uint32_t, uint32_t fg, uint32_t sr) { requested_sr = sr; requested_fg = fg; return 0; }
void* VulkanStorySlModule() { return reinterpret_cast<void*>(1); }
int VulkanStorySlShutdownChecked() { return shutdown_allowed ? 0 : -1; }
rsf_vk_function VulkanStorySlGetInstanceProcAddr(void*, const char*) { return nullptr; }
int VulkanStorySlBindReflex() { return 0; }
int VulkanStorySlBindPcl() { ++pcl_binds; if(pcl_bind_fail) return -17; pcl_bound_thread=GetCurrentThreadId(); return 0; }
uint32_t VulkanStorySlPclWindowMessage() { return 0xc123; }
int VulkanStorySlGetReflexState(uint32_t* available, uint32_t* reports) { *available = low_latency; *reports = 1; return 0; }
int VulkanStorySlSetReflexInterval(int mode, uint32_t interval) {
    require((mode==1 && interval==12345) || (xell_mode==1 && mode==0 && interval==0), "one selected owner and microsecond interval preserved");
    ++options;return 0;
}
int VulkanStorySlBeginFrame(uint32_t frame) {
    sdk_tokens[cursor] = frame; current = reinterpret_cast<uintptr_t>(&sdk_tokens[cursor]); cursor = (cursor + 1) % sdk_tokens.size(); return 0;
}
uintptr_t VulkanStorySlCurrentFrameToken() { return current; }
int VulkanStorySlTokenMatches(uintptr_t token, uint32_t index) { return token && *reinterpret_cast<uint32_t*>(token) == index; }
int VulkanStorySlReflexSleep() {
    if(check_input_registration) require(pcl_bound_thread==GetCurrentThreadId(),"input thread already registered before sleep");
    std::unique_lock lock(sleep_guard);
    if (block_reflex) {
        reflex_entered=true;
        sleep_changed.notify_all();
        sleep_changed.wait(lock,[]{return release_reflex;});
    }
    ++slept; return 0;
}
int VulkanStorySlMarkerForToken(uintptr_t token, uint32_t kind) { if(check_input_registration) require(pcl_bound_thread==GetCurrentThreadId(),"input thread already registered before SDK marker"); markers.push_back({*reinterpret_cast<uint32_t*>(token), kind}); return 0; }
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
    require(rsf_latencyvk_input(1, 4, 0xc124) == RSF_BACKEND_ERROR_INVALID_ARGUMENT && markers.size() == before_input,
        "an unrelated window message cannot partially emit a controller marker");
    require(rsf_latencyvk_input(1, 4, 0) == RSF_BACKEND_OK && rsf_latencyvk_input(1, 4, 0) == RSF_BACKEND_OK && markers.size() == before_input + 1, "controller sampled once per source");
    require(rsf_latencyvk_input(1, 0, 0xc124) == RSF_BACKEND_ERROR_INVALID_ARGUMENT, "foreign Windows message refused");
    require(rsf_latencyvk_input(1, 0, 0xc123) == RSF_BACKEND_OK, "registered latency ping");
    rsf_latencyvk_render(1); rsf_latencyvk_present(1); rsf_latencyvk_present(0);
    const size_t completed = markers.size();
    rsf_latencyvk_present(1); rsf_latencyvk_present(0);
    require(markers.size() == completed, "secondary Present cannot reuse completed source identity");
    require(markers.front().frame == 1 && markers.front().kind == 0 && markers.back().kind == 5, "simulation through Present on one source token");
    const auto before_oversize = current;
    cpu(uint64_t(UINT32_MAX) + 2, RSF_GAME_CPU_FRAME_BEGIN);
    require(current == before_oversize,"an out-of-range source cannot replace a 32-bit SDK token");
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
    const auto overlap = markers.size();
    rsf_latencyvk_render(1); rsf_latencyvk_present(1);
    cpu(2, RSF_GAME_CPU_FRAME_BEGIN);
    rsf_latencyvk_render(2); rsf_latencyvk_present(1); rsf_latencyvk_present(0);
    rsf_latencyvk_present(1); rsf_latencyvk_present(0);
    require(markers.size() == overlap + 6 &&
        markers[overlap + 3].frame == 1 && markers[overlap + 3].kind == 5 &&
        markers[overlap + 4].frame == 2 && markers[overlap + 4].kind == 4 &&
        markers[overlap + 5].frame == 2 && markers[overlap + 5].kind == 5,
        "legacy Present end keeps a newer render queued and duplicate begin cannot replace its source");
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
    low_latency=1;
    require(rsf_latencyvk_initialize(&setup)==RSF_BACKEND_OK,"keyed session initialized");
    require(rsf_latencyvk_presentation(reinterpret_cast<void*>(1))==RSF_BACKEND_OK,"core binds required XeLL owner");
    require(rsf_latencyvk_options(2,10000)==RSF_BACKEND_OK && xell_mode==1,"core maps Boost to required XeLL On");
    const auto reflex_sleeps=slept;
    rsf_game_cpu_event fact{sizeof(fact),0,77,100,1000,1000,1,0,0,1,RSF_GAME_CPU_INPUT_KEYED};
    rsf_latencyvk_cpu(&fact);
    // This sink has no prospective owner/token contract. Source v2 must remain
    // outside native_cpu adoption and cannot acquire a late pace or marker.
    const auto before_prospective_sleep = slept;
    const auto before_prospective_xell = xell_sleeps;
    const auto before_prospective_markers = xell_markers.size();
    rsf_game_cpu_source_event unsupported{};
    unsupported.header = {sizeof(unsupported), RSF_GAME_CPU_SOURCE_RESERVE, 77, 101, 1001, 1000,
        0, 0, 0, RSF_GAME_CPU_EVENT_VERSION_SOURCE, 0};
    unsupported.native_generation = 1;
    rsf_latencyvk_cpu(&unsupported.header);
    rsf_native_cpu_reservation absent{}; absent.struct_size = sizeof(absent);
    require(!rsf_native_cpu_reservation_read(77, 101, &absent) && slept == before_prospective_sleep &&
        xell_sleeps == before_prospective_xell && xell_markers.size() == before_prospective_markers,
        "Vulkan prospective source remains refused without its owner contract");
    fact.stage=RSF_GAME_CPU_INPUT_SAMPLE;fact.timestamp_qpc=1010;rsf_latencyvk_cpu(&fact);rsf_latencyvk_cpu(&fact);
    fact.stage=RSF_GAME_CPU_SIMULATION_BEGIN;fact.timestamp_qpc=1011;rsf_latencyvk_cpu(&fact);
    fact.stage=RSF_GAME_CPU_INPUT_EVENT;fact.timestamp_qpc=1012;fact.input_kind=7;rsf_latencyvk_cpu(&fact);rsf_latencyvk_cpu(&fact);
    fact.stage=RSF_GAME_CPU_SIMULATION_END;fact.timestamp_qpc=1020;fact.input_kind=0;rsf_latencyvk_cpu(&fact);
    require(xell_sleeps==1 && slept==reflex_sleeps,"exactly one selected pacer, no Reflex sleep stacked on XeLL");
    require(xell_markers.size()==3 && xell_markers[0].kind==6 && xell_markers[1].kind==0 && xell_markers[2].kind==1,
        "shared accepted input facts reach XeLL once in order");
    rsf_game_render_pass execution{}; execution.struct_size=sizeof(execution);
    execution.session_id=77; execution.source_frame_id=100; execution.role=RSF_GAME_RENDER_PRESENT;
    alignas(rsf_game_render_pass) uint32_t short_execution=sizeof(short_execution);
    require(rsf_latencyvk_execution(reinterpret_cast<const rsf_game_render_pass*>(&short_execution),1)==
        RSF_BACKEND_ERROR_INVALID_ARGUMENT,"size-only execution prefix is refused without reading its body");
    require(rsf_latencyvk_execution(nullptr,1)==RSF_BACKEND_ERROR_INVALID_ARGUMENT &&
        rsf_latencyvk_execution(&execution,1)==RSF_BACKEND_ERROR_NOT_READY,
        "explicit Present cannot invent a missing submission");
    execution.role=RSF_GAME_RENDER_SUBMISSION;
    auto foreign_execution=execution;foreign_execution.session_id=76;
    require(rsf_latencyvk_execution(&foreign_execution,1)==RSF_BACKEND_ERROR_NOT_READY,"foreign execution session refused");
    require(rsf_latencyvk_execution(&execution,1)==RSF_BACKEND_OK && rsf_latencyvk_execution(&execution,1)==RSF_BACKEND_OK &&
        rsf_latencyvk_execution(&execution,0)==RSF_BACKEND_OK && xell_markers.size()==5,
        "actual submission end closes its interval; duplicate begins emit once");
    fact={sizeof(fact),RSF_GAME_CPU_FRAME_BEGIN,77,101,2000,1000,1,0,0,1,RSF_GAME_CPU_INPUT_KEYED};
    rsf_latencyvk_cpu(&fact);
    fact.stage=RSF_GAME_CPU_INPUT_SAMPLE;fact.timestamp_qpc=2010;rsf_latencyvk_cpu(&fact);
    fact.stage=RSF_GAME_CPU_SIMULATION_BEGIN;fact.timestamp_qpc=2011;rsf_latencyvk_cpu(&fact);
    fact.stage=RSF_GAME_CPU_SIMULATION_END;fact.timestamp_qpc=2020;rsf_latencyvk_cpu(&fact);
    execution.source_frame_id=101;
    require(rsf_latencyvk_execution(&execution,1)==RSF_BACKEND_OK,"newer rendering may overlap previous presentation");
    execution.role=RSF_GAME_RENDER_PRESENT;
    require(rsf_latencyvk_execution(&execution,1)==RSF_BACKEND_ERROR_NOT_READY,
        "newer Present cannot synthesize the missing actual submission end");
    execution.source_frame_id=100;execution.role=RSF_GAME_RENDER_PRESENT;
    require(rsf_latencyvk_execution(&execution,0)==RSF_BACKEND_ERROR_NOT_READY,"unpaired explicit Present end refused");
    require(rsf_latencyvk_execution(&execution,1)==RSF_BACKEND_OK && rsf_latencyvk_execution(&execution,0)==RSF_BACKEND_OK,
        "previous output presents with its own source");
    const auto n=xell_markers.size();
    require(n>=2 && xell_markers[n-2].frame==100 && xell_markers[n-2].kind==4 && xell_markers[n-1].frame==100 && xell_markers[n-1].kind==5,
        "Present never takes the newer render token");
    require(rsf_latencyvk_execution(&execution,1)==RSF_BACKEND_OK && rsf_latencyvk_execution(&execution,0)==RSF_BACKEND_OK &&
        xell_markers.size()==n,"repeated explicit presentation is deduplicated");
    execution.role=RSF_GAME_RENDER_SUBMISSION;
    require(rsf_latencyvk_execution(&execution,1)==RSF_BACKEND_ERROR_NOT_READY,"late rendering cannot reopen a presented source");
    rsf_game_cpu_event duplicate_begin{sizeof(duplicate_begin),RSF_GAME_CPU_FRAME_BEGIN,77,100,1000,1000,1,0,0,0,0};
    rsf_latencyvk_cpu(&duplicate_begin);
    require(rsf_latencyvk_execution(&execution,1)==RSF_BACKEND_ERROR_NOT_READY,
        "a duplicate legacy CPU begin cannot reopen an already presented keyed source");
    require(rsf_latencyvk_presentation(nullptr)==RSF_BACKEND_OK && rsf_latencyvk_shutdown()==RSF_BACKEND_OK,"keyed owner retired");
    require(rsf_latencyvk_initialize(&setup)==RSF_BACKEND_OK,"new keyed session initialized");
    require(rsf_latencyvk_options(1,12345)==RSF_BACKEND_OK,"new session owns pacing");
    fact={sizeof(fact),RSF_GAME_CPU_FRAME_BEGIN,78,1,2000,1000,1,0,0,1,RSF_GAME_CPU_INPUT_KEYED};
    rsf_latencyvk_cpu(&fact);
    rsf_native_cpu_frame restarted{};restarted.struct_size=sizeof(restarted);
    require(rsf_native_cpu_read(78,1,&restarted)!=0,"shutdown releases previous CPU session before a new source starts at one");
    shutdown_allowed=false;
    require(rsf_latencyvk_shutdown()==RSF_BACKEND_ERROR_FEATURE_FAILED && rsf_native_latency_has_claim(2) &&
        rsf_native_cpu_read(78,1,&restarted),"failed SDK shutdown retains pacing lease and CPU session");
    shutdown_allowed=true;
    require(rsf_latencyvk_shutdown()==RSF_BACKEND_OK && !rsf_native_latency_has_claim(2),"new keyed session retired");
    require(rsf_native_latency_claim(1),"another graphics orchestrator owns process pacing");
    require(rsf_latencyvk_initialize(&setup)==RSF_BACKEND_OK,"contending Vulkan session initialized");
    require(rsf_latencyvk_presentation(reinterpret_cast<void*>(1))==RSF_BACKEND_OK,"presentation facts can be borrowed without claiming pacing");
    require(rsf_latencyvk_options(1,10000)==RSF_BACKEND_ERROR_NOT_SUPPORTED,"required XeLL cannot take another owner's lease");
    const auto denied_markers=xell_markers.size();
    rsf_latencyvk_render(999);rsf_latencyvk_submit_end(999);rsf_latencyvk_present(1);rsf_latencyvk_present(0);
    require(xell_markers.size()==denied_markers && xell_sleeps==2,"contending owner emits no XeLL marker or sleep");
    require(rsf_latencyvk_shutdown()==RSF_BACKEND_OK,"contending session retired");
    rsf_native_latency_release_claim(1);
    // Real Win32 thread queues, synthetic SDK only. No game, device or visible window.
    require(rsf_latencyvk_initialize(&setup)==RSF_BACKEND_OK,"ping session initialized");
    auto begin=[](uint64_t id) {
        LARGE_INTEGER now{},hz{};QueryPerformanceCounter(&now);QueryPerformanceFrequency(&hz);
        rsf_game_cpu_event e{sizeof(e),RSF_GAME_CPU_FRAME_BEGIN,909,id,
            static_cast<uint64_t>(now.QuadPart),static_cast<uint64_t>(hz.QuadPart),1,0,0,
            RSF_GAME_CPU_EVENT_VERSION,RSF_GAME_CPU_INPUT_KEYED};
        check_input_registration=true;rsf_latencyvk_cpu(&e);check_input_registration=false;
    };
    MSG queued{};(void)PeekMessageW(&queued,reinterpret_cast<HWND>(-1),WM_USER,WM_USER,PM_NOREMOVE);
    while(PeekMessageW(&queued,reinterpret_cast<HWND>(-1),0xc123,0xc123,PM_REMOVE)) {}
    require(PostThreadMessageW(GetCurrentThreadId(),0xc123,0,0)!=0,"pre-registration ping queued");
    const auto initial_binds=pcl_binds;begin(1);
    require(pcl_binds==initial_binds+1&&pcl_bound_thread==GetCurrentThreadId(),"first CPU binding registers current thread");
    require(!PeekMessageW(&queued,reinterpret_cast<HWND>(-1),0xc123,0xc123,PM_NOREMOVE),"pre-epoch ping and queue fence removed");
    const auto prior_markers=markers.size();begin(2);
    require(markers.size()==prior_markers,"pre-registration ping cannot become a new-session marker");
    require(PostThreadMessageW(GetCurrentThreadId(),0xc123,0,0)!=0,"fresh ping queued");begin(3);begin(4);
    require(markers.size()==prior_markers+1&&markers.back().frame==4&&markers.back().kind==8,"fresh epoch ping associated at following boundary");
    require(PostThreadMessageW(GetCurrentThreadId(),0xc124,0,0)!=0,"unrelated registered ID queued");
    // A message-only window has no UI/rendering. Its messages are not thread messages.
    HWND window=CreateWindowExW(0,L"STATIC",L"",0,0,0,0,0,HWND_MESSAGE,nullptr,GetModuleHandleW(nullptr),nullptr);
    require(window&&PostMessageW(window,0xc123,0,0),"same-ID window message queued");
    for(unsigned i=0;i<40;i++) require(PostThreadMessageW(GetCurrentThreadId(),0xc123,0,0)!=0,"ping flood queued");
    begin(5);
    unsigned remaining=0;while(PeekMessageW(&queued,reinterpret_cast<HWND>(-1),0xc123,0xc123,PM_REMOVE))++remaining;
    require(remaining==24,"ping drain is bounded to sixteen removals");
    require(PeekMessageW(&queued,reinterpret_cast<HWND>(-1),0xc124,0xc124,PM_REMOVE)&&queued.message==0xc124,"unrelated same-range ID remains queued");
    require(PeekMessageW(&queued,window,0xc123,0xc123,PM_REMOVE)&&queued.hwnd==window,"same-ID window message remains queued");
    DestroyWindow(window);
    const auto before_flood=markers.size();begin(6);
    require(markers.size()==before_flood+1,"flood emits no duplicate input marker");
    PostQuitMessage(37);begin(7);
    require(PeekMessageW(&queued,reinterpret_cast<HWND>(-1),0,0,PM_REMOVE)&&queued.message==WM_QUIT&&queued.wParam==37,"engine quit remains queued");
    require(PostThreadMessageW(GetCurrentThreadId(),0xc123,0,0)!=0,"old ping queued before rebind away");
    std::thread changed([&]{
        (void)PeekMessageW(&queued,reinterpret_cast<HWND>(-1),WM_USER,WM_USER,PM_NOREMOVE);
        require(PostThreadMessageW(GetCurrentThreadId(),0xc123,0,0)!=0,"new-thread ping queued");
        pcl_bind_fail=true;begin(8);
        require(PeekMessageW(&queued,reinterpret_cast<HWND>(-1),0xc123,0xc123,PM_NOREMOVE)!=0,"failed rebind cannot consume the new queue");
        rsf_latencyvk_status refused{sizeof(refused),RSF_LATENCYVK_ABI_VERSION};
        require(rsf_latencyvk_get_status(&refused)==RSF_BACKEND_OK&&refused.last_result==-17,"failed rebind is reported");
        pcl_bind_fail=false;begin(9);
        require(pcl_bound_thread==GetCurrentThreadId()&&!PeekMessageW(&queued,reinterpret_cast<HWND>(-1),0xc123,0xc123,PM_NOREMOVE),"successful thread change rebinds and pumps only its queue");
    });changed.join();
    begin(10);require(pcl_bound_thread==GetCurrentThreadId(),"returning input thread rebinds explicitly");
    const auto before_return=markers.size();begin(11);
    require(markers.size()==before_return,"old ping on returning thread is pre-epoch and cannot be associated");
    require(rsf_latencyvk_options(1,12345)==RSF_BACKEND_OK,"input sample pacing configured");
    std::thread sample_thread([&]{
        LARGE_INTEGER now{},hz{};QueryPerformanceCounter(&now);QueryPerformanceFrequency(&hz);
        rsf_game_cpu_event e{sizeof(e),RSF_GAME_CPU_INPUT_SAMPLE,909,11,
            static_cast<uint64_t>(now.QuadPart),static_cast<uint64_t>(hz.QuadPart),1,0,0,RSF_GAME_CPU_EVENT_VERSION,RSF_GAME_CPU_INPUT_KEYED};
        const auto binds_before=pcl_binds;
        auto malformed_input=e;malformed_input.struct_size--;rsf_latencyvk_cpu(&malformed_input);
        malformed_input=e;malformed_input.timestamp_qpc=0;rsf_latencyvk_cpu(&malformed_input);
        malformed_input=e;malformed_input.flags|=0x80000000u;rsf_latencyvk_cpu(&malformed_input);
        auto delivered=e;delivered.stage=RSF_GAME_CPU_INPUT_EVENT;delivered.source_frame_id=0;
        delivered.flags=RSF_GAME_CPU_INPUT_UNFRAMED;delivered.input_kind=1;rsf_latencyvk_cpu(&delivered);
        rsf_latencyvk_render(11);
        require(pcl_binds==binds_before,"malformed, event-only and render calls cannot bind the calling thread");
        check_input_registration=true;rsf_latencyvk_cpu(&e);check_input_registration=false;
        require(pcl_bound_thread==GetCurrentThreadId(),"input sample rebind completed before pacing");
    });sample_thread.join();
    begin(12);
    require(PostThreadMessageW(GetCurrentThreadId(),0xc123,0,0)!=0,"own ping queued before shutdown");
    require(rsf_latencyvk_shutdown()==RSF_BACKEND_OK,"ping session shutdown");
    const auto retired_binds=pcl_binds;
    require(rsf_latencyvk_initialize(&setup)==RSF_BACKEND_OK,"ping session reinitialized");begin(1);
    require(pcl_binds==retired_binds+1,"reinitialization cannot retain a registration thread");
    const auto after_restart=markers.size();begin(2);
    require(markers.size()==after_restart,"pre-shutdown ping cannot be assigned a fresh dequeue time in new session");
    require(PostThreadMessageW(GetCurrentThreadId(),0xc123,0,0)!=0,"fresh post-restart ping queued");begin(3);begin(4);
    require(markers.size()==after_restart+1&&markers.back().kind==8,"fresh post-restart ping accepted");
    require(rsf_latencyvk_shutdown()==RSF_BACKEND_OK,"ping reinitialized session retired");
    require(rsf_latencyvk_initialize(&setup)==RSF_BACKEND_OK&&rsf_latencyvk_options(1,12345)==RSF_BACKEND_OK,"Reflex request predates FG attachment");
    const auto reflex_before_attachment=slept;
    auto* first_context=reinterpret_cast<rsf_fgvk*>(1);
    auto* next_context=reinterpret_cast<rsf_fgvk*>(2);
    require(rsf::latency::presentation_attach(first_context)==RSF_BACKEND_OK,"automatic attach publishes opaque context without vendor calls");
    xell_mode=0;begin(1);
    block_xell=true;xell_entered=release_xell=false;
    std::thread sleeper([&]{
        LARGE_INTEGER now{},hz{};QueryPerformanceCounter(&now);QueryPerformanceFrequency(&hz);
        rsf_game_cpu_event input{sizeof(input),RSF_GAME_CPU_INPUT_SAMPLE,909,1,
            uint64_t(now.QuadPart),uint64_t(hz.QuadPart),1,0,0,RSF_GAME_CPU_EVENT_VERSION,RSF_GAME_CPU_INPUT_KEYED};
        rsf_latencyvk_cpu(&input);
    });
    {std::unique_lock lock(sleep_guard);require(sleep_changed.wait_for(lock,std::chrono::seconds(2),[]{return xell_entered;}),"XeLL CPU sleep entered");}
    require(xell_mode==1&&slept==reflex_before_attachment,"unchanged request selects/applies XeLL at actual input, without Reflex sleep");
    rsf_latencyvk_status during_sleep{sizeof(during_sleep),RSF_LATENCYVK_ABI_VERSION};
    require(rsf_latencyvk_get_status(&during_sleep)==RSF_BACKEND_OK&&during_sleep.owner==RSF_PACING_XELL,"SDK mutex is available while XeLL sleep blocks");
    require(rsf_latencyvk_shutdown()==RSF_BACKEND_ERROR_NOT_READY,"shutdown refuses an outstanding sleep without waiting");
    {rsf::latency::PresentationLease refused;require(!refused.get()&&refused.closing(),"shutdown admission prevents new CPU borrows");}
    require(rsf_latencyvk_presentation(nullptr)==RSF_BACKEND_ERROR_NOT_READY,"unbind refuses outstanding actual CPU sleep without waiting");
    {rsf::latency::PresentationLease refused;require(!refused.get()&&refused.closing(),"retirement prevents new context borrows");}
    require(rsf_latencyvk_presentation(next_context)==RSF_BACKEND_ERROR_NOT_READY&&rsf_latencyvk_shutdown()==RSF_BACKEND_ERROR_NOT_READY,"rebind/shutdown cannot lose a sleeping owner");
    {std::lock_guard lock(sleep_guard);release_xell=true;sleep_changed.notify_all();}sleeper.join();block_xell=false;
    require(rsf_latencyvk_presentation(nullptr)==RSF_BACKEND_OK,"unbind retry succeeds after CPU lease exits");
    require(rsf::latency::presentation_attach(next_context)==RSF_BACKEND_OK,"successor context can bind after retirement");
    shutdown_allowed=false;require(rsf_latencyvk_shutdown()==RSF_BACKEND_ERROR_FEATURE_FAILED,"checked shutdown failure is retained");
    {rsf::latency::PresentationLease retained;require(retained.get()==next_context,"failed SDK shutdown retains live context binding");}
    shutdown_allowed=true;require(rsf_latencyvk_shutdown()==RSF_BACKEND_OK,"checked shutdown retry releases binding");
    require(rsf_latencyvk_initialize(&setup)==RSF_BACKEND_OK && rsf_latencyvk_options(1,12345)==RSF_BACKEND_OK,
        "Reflex-only session initialized without a presentation owner");
    cpu(1,RSF_GAME_CPU_FRAME_BEGIN);
    cpu(2,RSF_GAME_CPU_FRAME_BEGIN);
    const auto pacing_token=current;
    block_reflex=true;
    std::thread reflex_sleeper([]{cpu(2,RSF_GAME_CPU_PACING);});
    {
        std::unique_lock lock(sleep_guard);
        require(sleep_changed.wait_for(lock,std::chrono::seconds(2),[]{return reflex_entered;}),"Reflex sleep entered");
    }
    require(rsf_latencyvk_shutdown()==RSF_BACKEND_ERROR_NOT_READY,"Reflex sleep protects its SDK session without a presentation context");
    cpu(3,RSF_GAME_CPU_FRAME_BEGIN);
    require(current==pacing_token,"a concurrent begin cannot replace an active Reflex pacing token");
    const auto before_present=markers.size();
    rsf_latencyvk_render(1); rsf_latencyvk_present(1); rsf_latencyvk_present(0);
    require(markers.size()==before_present+3 && markers.back().frame==1 && markers.back().kind==5,
        "the older source can finish Present while Reflex sleep is blocked");
    {std::lock_guard lock(sleep_guard);release_reflex=true;sleep_changed.notify_all();}
    reflex_sleeper.join();
    block_reflex=false;
    cpu(3,RSF_GAME_CPU_FRAME_BEGIN);
    require(VulkanStorySlTokenMatches(current,3),"the next token can be acquired after pacing completes");
    cpu(9,RSF_GAME_CPU_FRAME_BEGIN);
    cpu(3,RSF_GAME_CPU_FRAME_END,0);
    rsf_slvk_call newer{sizeof(newer),RSF_SLVK_ABI_VERSION,1,9,
        [](void*, uintptr_t token, void*) -> rsf_backend_result {
            return VulkanStorySlTokenMatches(token,9) ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_NOT_READY;
        },nullptr};
    require(rsf_slvk_invoke(&newer)==RSF_BACKEND_OK,"a delayed cancel cannot erase a newer token in the same slot");
    require(rsf_latencyvk_shutdown()==RSF_BACKEND_OK,"Reflex-only shutdown succeeds after sleep");
    return 0;
}
