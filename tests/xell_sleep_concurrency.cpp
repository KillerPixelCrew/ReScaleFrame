// SPDX-License-Identifier: GPL-3.0-only
// Exercise the actual provider's begin/marker functions with bounded SDK doubles.
#include "../runtime/backends/xess/src/xess_generation.cpp"
#include <condition_variable>
#include <thread>
#include <chrono>
#include <cstdio>
namespace {
std::mutex gate;
std::condition_variable changed;
bool entered{}, marked{};
xefg_swapchain_result_t counting(xefg_swapchain_handle_t,uint32_t) { return XEFG_SWAPCHAIN_RESULT_SUCCESS; }
xefg_swapchain_result_t enabling(xefg_swapchain_handle_t,uint32_t) { return XEFG_SWAPCHAIN_RESULT_SUCCESS; }
xell_result_t sleeping(xell_context_handle_t,uint32_t) {
    std::unique_lock lock(gate); entered=true; changed.notify_all();
    return changed.wait_for(lock,std::chrono::seconds(2),[]{return marked;}) ? XELL_RESULT_SUCCESS : XELL_RESULT_ERROR_UNKNOWN;
}
xell_result_t marking(xell_context_handle_t,uint32_t id,xell_latency_marker_type_t kind) {
    if (id!=1 || kind!=XELL_PRESENT_END) return XELL_RESULT_ERROR_INVALID_ARGUMENT;
    std::lock_guard lock(gate);marked=true;changed.notify_all();return XELL_RESULT_SUCCESS;
}
}
int main() {
    XegSession session;session.xell=reinterpret_cast<xell_context_handle_t>(1);
    session.sleep=sleeping;session.marker=marking;session.last_begin=10;session.sdk_frame=1;
    session.frame_ids[10%session.frame_ids.size()]={10,1};
    rsf_backend_result result=RSF_BACKEND_ERROR_NOT_READY;
    std::thread worker([&]{result=begin(&session,11);});
    bool started=false;
    {std::unique_lock lock(gate);started=changed.wait_for(lock,std::chrono::seconds(2),[]{return entered;});}
    if (!started) {worker.join();return 1;}
    const auto premature=marker(&session,RSF_LATENCY_PRESENT_END,11,0);
    // configure only checks this pointer for presence; no COM method is called
    // when busy sleep refuses the change before any vendor mutation.
    session.chain.Attach(reinterpret_cast<IDXGISwapChain4*>(1));
    const rsf_fg_options changed_options{sizeof(rsf_fg_options),RSF_FG_ABI_VERSION,RSF_FG_OFF,1,0,RSF_REFLEX_OFF,0};
    const auto busy=configure(&session,&changed_options);
    session.reserved=2;session.count=counting;session.enabled=enabling;
    rsf_fg_options previous{sizeof(rsf_fg_options),RSF_FG_ABI_VERSION,RSF_FG_FIXED,1,0,RSF_REFLEX_ON,0};
    session.copied.configured(previous,RSF_FG_FIXED,1,RSF_REFLEX_ON,false);
    previous.generated_frames=2;
    // An FG-only change with identical sleep parameters must not wait for sleep
    // or reconfigure XeLL. Its function pointer is intentionally absent here.
    const auto unchanged_sleep=configure(&session,&previous);
    (void)session.chain.Detach();
    const auto old=marker(&session,RSF_LATENCY_PRESENT_END,10,0);
    worker.join();
    if (old!=RSF_BACKEND_OK || result!=RSF_BACKEND_OK || session.frame_ids[11%session.frame_ids.size()].sdk!=2) return 2;
    if (begin(&session,11)!=RSF_BACKEND_ERROR_INVALID_ARGUMENT) return 3;
    if (premature!=RSF_BACKEND_ERROR_NOT_READY || busy!=RSF_BACKEND_ERROR_NOT_READY) return 4;
    if (unchanged_sleep!=RSF_BACKEND_OK) return 5;
    std::puts("PASS prior-frame XeLL marker progresses during next-frame sleep; no graphics device created");
    return 0;
}
