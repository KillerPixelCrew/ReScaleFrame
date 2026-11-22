// SPDX-License-Identifier: MIT
#include <rescaleframe/fg_session.h>
#include <rescaleframe/frame_sequencer.h>
#include <cstdio>
#include <cstdlib>
#include <initializer_list>
#include <new>
namespace {
void check(bool ok, int line) { if (!ok) { std::fprintf(stderr, "FG contract failed at %d\n", line); std::exit(1); } }
#define CHECK(x) check((x), __LINE__)
struct Context { bool physical; };
bool fail_probe = false, fail_chain = false, fail_retire = false, quiescent = false;
uint32_t physical = 0, destroyed = 0, restored = 0, mock_pacing = RSF_PACING_REFLEX, begins = 0, markers = 0;
uint32_t preparations = 0;
rsf_fg_session* live_session = nullptr;
rsf_backend_result create(const rsf_generation_setup* setup, void** context, void** chain)
{
    CHECK(quiescent); *context = nullptr; *chain = nullptr;
    if (!setup->chain.hwnd && fail_probe) return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    if (setup->chain.hwnd && fail_chain) return RSF_BACKEND_ERROR_INIT_FAILED;
    if (setup->chain.hwnd) { CHECK(physical == 0); ++physical; }
    auto* c = new Context{setup->chain.hwnd != nullptr}; *context = c;
    if (c->physical) *chain = c;
    return RSF_BACKEND_OK;
}
rsf_backend_result configure(void*, const rsf_fg_options*) { CHECK(quiescent); return RSF_BACKEND_OK; }
rsf_backend_result begin(void*, uint64_t) { ++begins; return RSF_BACKEND_OK; }
rsf_backend_result marker(void*, rsf_latency_marker, uint64_t, uint32_t) { ++markers; return RSF_BACKEND_OK; }
rsf_backend_result prepare(void*, void*, const rsf_fg_frame*) { ++preparations; return RSF_BACKEND_OK; }
rsf_backend_result after(void*) { return RSF_BACKEND_OK; }
rsf_backend_result status(void*, rsf_fg_status* out)
{
    out->supported = 1; out->max_generated_frames = 1; out->pacing_owner = mock_pacing;
    return RSF_BACKEND_OK;
}
rsf_backend_result retire(void*, rsf_fg_retirement* out)
{
    out->fence = nullptr; out->value = 0;
    return fail_retire ? RSF_BACKEND_ERROR_NOT_READY : RSF_BACKEND_OK;
}
void destroy(void* c)
{
    CHECK(quiescent); auto* context = static_cast<Context*>(c);
    if (context->physical) { CHECK(physical == 1); --physical; ++destroyed; }
    delete context;
}
rsf_backend_result abort_frame(void*, uint64_t) { return RSF_BACKEND_OK; }
const rsf_generation_provider provider{sizeof(provider), create, configure, begin, marker, prepare, after, status, retire, destroy, abort_frame, nullptr};
rsf_backend_result quiesce(void*)
{
    // A host stopping CPU producers must be able to observe the transition without deadlocking.
    if (live_session) CHECK(rsf_fg_session_begin(live_session, 100) == RSF_BACKEND_ERROR_NOT_READY);
    quiescent = true; return RSF_BACKEND_OK;
}
void resume(void*)
{
    CHECK(quiescent);
    // Resume may synchronously wake a producer while the transition is still unpublished.
    if (live_session) CHECK(rsf_fg_session_begin(live_session, 100) == RSF_BACKEND_ERROR_NOT_READY);
    quiescent = false;
}
rsf_backend_result release(void*) { CHECK(quiescent); return RSF_BACKEND_OK; }
rsf_backend_result adopt(void*, void* chain) { CHECK(quiescent && chain && physical == 1); return RSF_BACKEND_OK; }
rsf_backend_result plain(void*) { CHECK(quiescent && !physical); ++restored; return RSF_BACKEND_OK; }
}
int main()
{
    rsf_fg_host host{sizeof(host), RSF_FG_ABI_VERSION, nullptr, quiesce, resume, release, adopt, plain};
    CHECK(rsf_fg_session_create(&host, &live_session) == 0);
    rsf_generation_setup setup{}; setup.struct_size = sizeof(setup); setup.abi_version = RSF_FG_ABI_VERSION;
    setup.chain.hwnd = reinterpret_cast<void*>(1);
    rsf_fg_options options{sizeof(options), RSF_FG_ABI_VERSION, RSF_FG_FIXED, 1, 0, RSF_REFLEX_ON, 0};
    CHECK(rsf_fg_session_select(live_session, &provider, &setup, &options) == 0);
    fail_probe = true;
    CHECK(rsf_fg_session_select(live_session, &provider, &setup, &options) == RSF_BACKEND_ERROR_NOT_SUPPORTED);
    CHECK(physical == 1 && destroyed == 0); fail_probe = false;
    options.mode = RSF_FG_DYNAMIC;
    CHECK(rsf_fg_session_select(live_session, &provider, &setup, &options) == RSF_BACKEND_ERROR_NOT_SUPPORTED);
    CHECK(physical == 1 && destroyed == 0); options.mode = RSF_FG_FIXED;
    CHECK(rsf_fg_session_begin_rendered(live_session, 1, 10) == RSF_BACKEND_ERROR_NOT_SUPPORTED);
    CHECK(rsf_fg_session_begin(live_session, 1) == 0);
    CHECK(rsf_fg_session_select(live_session, nullptr, nullptr, nullptr) == RSF_BACKEND_ERROR_NOT_READY);
    CHECK(rsf_fg_session_marker(live_session, 1, RSF_LATENCY_INPUT_SAMPLE, 10, 0) == 0);
    for (uint32_t i = 0; i < 4; ++i) CHECK(rsf_fg_session_marker(live_session, 1, i, 11 + i, 0) == 0);
    rsf_frame_record record{}; record.struct_size = sizeof(record); record.frame_id = 1; record.screen = RSF_SCREEN_FLIGHT;
    record.abi_version = RSF_GAME_FRAME_ABI_VERSION;
    rsf_fg_frame frame{}; frame.struct_size = sizeof(frame); frame.record = &record;
    frame.interpolate = 1;
    CHECK(rsf_fg_session_prepare(live_session, nullptr, &frame) == RSF_BACKEND_ERROR_INVALID_ARGUMENT);
    record.input_qpc = 10;
    CHECK(rsf_fg_session_prepare(live_session, nullptr, &frame) == 0);
    CHECK(rsf_fg_session_prepare(live_session, nullptr, &frame) == RSF_BACKEND_ERROR_NOT_READY);
    CHECK(rsf_fg_session_marker(live_session, 1, RSF_LATENCY_PRESENT_START, 15, 0) == 0);
    CHECK(rsf_fg_session_marker(live_session, 1, RSF_LATENCY_PRESENT_END, 16, 0) == 0);
    CHECK(rsf_fg_session_after_present(live_session, 1) == 0);
    CHECK(rsf_fg_session_begin(live_session, 1) == RSF_BACKEND_ERROR_NOT_READY);
    CHECK(rsf_fg_session_begin(live_session, 2) == 0);
    CHECK(rsf_fg_session_marker(live_session, 2, RSF_LATENCY_SIMULATION_START, 20, 0) == RSF_BACKEND_ERROR_INVALID_ARGUMENT);
    CHECK(rsf_fg_session_abort(live_session, 2) == 0);
    const auto after_abort_markers = markers, after_abort_preparations = preparations;
    record.frame_id = 2; record.input_qpc = 0;
    CHECK(rsf_fg_session_prepare(live_session,nullptr,&frame) == RSF_BACKEND_ERROR_NOT_READY);
    CHECK(rsf_fg_session_marker(live_session,2,RSF_LATENCY_SIMULATION_START,21,0) != RSF_BACKEND_OK);
    CHECK(markers == after_abort_markers && preparations == after_abort_preparations);
    record.frame_id = 1; record.input_qpc = 10;
    fail_retire = true;
    CHECK(rsf_fg_session_destroy(live_session) == RSF_BACKEND_ERROR_NOT_READY);
    CHECK(physical == 1 && destroyed == 0); fail_retire = false;
    fail_chain = true;
    CHECK(rsf_fg_session_select(live_session, &provider, &setup, &options) == RSF_BACKEND_ERROR_INIT_FAILED);
    CHECK(!physical && destroyed == 1 && restored == 1); fail_chain = false;
    rsf_fg_session_status state{}; state.struct_size = sizeof(state);
    CHECK(rsf_fg_session_get_status(live_session, &state) == 0);
    CHECK(!state.has_provider && state.plain_available);
    CHECK(rsf_fg_session_select(live_session, &provider, &setup, &options) == 0);
    CHECK(rsf_fg_session_destroy(live_session) == 0); live_session = nullptr;
    CHECK(!physical && destroyed == 2);
    mock_pacing = RSF_PACING_NONE; options.reflex_mode = RSF_REFLEX_OFF;
    CHECK(rsf_fg_session_create(&host, &live_session) == 0);
    CHECK(rsf_fg_session_select(live_session, &provider, &setup, &options) == 0);
    const auto old_begins = begins, old_markers = markers;
    CHECK(rsf_fg_session_begin_rendered(live_session, 1, 100) == 0);
    CHECK(begins == old_begins);
    CHECK(rsf_fg_session_marker(live_session, 1, RSF_LATENCY_RENDER_SUBMIT_START, 101, 0) == 0);
    CHECK(rsf_fg_session_marker(live_session, 1, RSF_LATENCY_RENDER_SUBMIT_END, 102, 0) == 0);
    record.input_qpc = 0; record.source_provenance = RSF_FRAME_SOURCE_RENDERED;
    record.source_qpc = 100; record.source_qpc_frequency = 1000;
    CHECK(rsf_fg_session_prepare(live_session, nullptr, &frame) == 0);
    CHECK(rsf_fg_session_marker(live_session, 1, RSF_LATENCY_PRESENT_START, 103, 0) == 0);
    CHECK(rsf_fg_session_marker(live_session, 1, RSF_LATENCY_PRESENT_END, 104, 0) == 0);
    CHECK(markers == old_markers);
    CHECK(rsf_fg_session_after_present(live_session, 1) == 0);
    CHECK(rsf_fg_session_destroy(live_session) == 0); live_session = nullptr;
    rsf_frame_sequencer* ledger = nullptr;
    CHECK(rsf_frame_sequencer_create(2, &ledger) == 0);
    CHECK(rsf_frame_sequencer_begin(ledger, UINT64_MAX - 2) == 0);
    CHECK(rsf_frame_sequencer_begin(ledger, UINT64_MAX) == RSF_BACKEND_ERROR_NOT_READY);
    CHECK(rsf_frame_sequencer_finish(ledger, UINT64_MAX - 2) == RSF_BACKEND_ERROR_NOT_READY);
    CHECK(rsf_frame_sequencer_begin(ledger, UINT64_MAX) == 0);
    CHECK(rsf_frame_sequencer_marker(ledger, UINT64_MAX, RSF_LATENCY_INPUT_SAMPLE, 100) == 0);
    CHECK(rsf_frame_sequencer_marker(ledger, UINT64_MAX, RSF_LATENCY_SIMULATION_START, 99) == RSF_BACKEND_ERROR_INVALID_ARGUMENT);
    CHECK(rsf_frame_sequencer_finish(ledger, UINT64_MAX) == RSF_BACKEND_ERROR_NOT_READY);
    rsf_frame_sequencer_destroy(ledger);
    CHECK(rsf_frame_sequencer_create(2, &ledger) == 0);
    CHECK(rsf_frame_sequencer_begin_rendered(ledger, 1, 0) == RSF_BACKEND_ERROR_INVALID_ARGUMENT);
    CHECK(rsf_frame_sequencer_begin_rendered(ledger, 1, 100) == 0);
    alignas(rsf_sequence_state) uint32_t short_state[] = {sizeof(uint32_t), 0xdeadbeef};
    CHECK(rsf_frame_sequencer_get(ledger, 1, reinterpret_cast<rsf_sequence_state*>(short_state))
          == RSF_BACKEND_ERROR_INVALID_ARGUMENT);
    CHECK(short_state[0] == sizeof(uint32_t) && short_state[1] == 0xdeadbeef);
    rsf_sequence_state rendered{sizeof(rendered)};
    CHECK(rsf_frame_sequencer_get(ledger, 1, &rendered) == 0);
    CHECK(rendered.source_provenance == RSF_FRAME_SOURCE_RENDERED && rendered.source_qpc == 100 && !rendered.timestamps[0]);
    CHECK(rsf_frame_sequencer_marker(ledger, 1, RSF_LATENCY_RENDER_SUBMIT_START, 101) == 0);
    CHECK(rsf_frame_sequencer_marker(ledger, 1, RSF_LATENCY_RENDER_SUBMIT_END, 102) == 0);
    CHECK(rsf_frame_sequencer_marker(ledger, 1, RSF_LATENCY_PRESENT_START, 103) == 0);
    CHECK(rsf_frame_sequencer_marker(ledger, 1, RSF_LATENCY_PRESENT_END, 104) == 0);
    CHECK(rsf_frame_sequencer_finish(ledger, 1) == 0);
    CHECK(rsf_frame_sequencer_begin_rendered(ledger, 2, 200) == 0);
    CHECK(rsf_frame_sequencer_marker(ledger, 2, RSF_LATENCY_INPUT_SAMPLE, 201) == RSF_BACKEND_ERROR_INVALID_ARGUMENT);
    CHECK(rsf_frame_sequencer_finish(ledger, 2) == RSF_BACKEND_ERROR_NOT_READY);
    rsf_frame_sequencer_destroy(ledger);
    std::puts("FG switching, fallback, retirement, and CPU identity contracts passed");
}
