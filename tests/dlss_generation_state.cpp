// SPDX-License-Identifier: MIT
#include "../runtime/backends/dlss/src/dlss_generation_state.h"
#include <atomic>
#include <barrier>
#include <cstdio>
#include <latch>
#include <thread>

namespace {
int failures{};
void check(bool ok, const char* text) { if (!ok) { ++failures; std::printf("FAIL: %s\n",text); } }
rsf_fg_options fixed_options() {
    rsf_fg_options value{}; value.struct_size=sizeof(value); value.abi_version=RSF_FG_ABI_VERSION;
    value.mode=RSF_FG_FIXED; value.generated_frames=2; value.reflex_mode=RSF_REFLEX_ON; return value;
}
}
int main() {
    rsf::DlssGenerationState state;
    rsf_fg_status initial{}; initial.struct_size=sizeof(initial); initial.supported=1;
    initial.total_presented=100; initial.latency_paced_frames=7;
    state.initialize(initial);
    auto options=fixed_options(); state.configured(options,RSF_REFLEX_ON,2,true);
    auto source=state.read();
    check(!source.history_valid && !source.prepared_enabled,"generation configuration starts with invalid history");
    check(state.stage_prepare(source.revision,10,RSF_FG_FIXED,2) && state.complete_prepare(source.revision,10,true),
        "ordinary successful prepare publishes history");
    int fences[2]{};
    rsf_fg_retirement retired{sizeof(retired),&fences[0],20};
    auto observed=initial; observed.valid_statistics=RSF_FG_STAT_TOTAL_PRESENTED|RSF_FG_STAT_ACTIVITY;
    observed.max_generated_frames=4; observed.low_latency_available=1;
    auto published=state.polled(source.revision,observed,3,true,retired);
    check(published.active && published.total_presented==103,"ordinary active poll accumulates statistics");

    // The exact production helper remains callable while the presenting owner is
    // in its external SDK gap. A successful abort must win over late publication.
    std::latch sdk_entered(1), sdk_return(1);
    bool staged{}, late_success{};
    std::thread presenting([&] {
        const auto before=state.read();
        staged=state.stage_prepare(before.revision,11,RSF_FG_FIXED,2);
        sdk_entered.count_down(); sdk_return.wait();
        late_success=state.complete_prepare(before.revision,11,true);
    });
    sdk_entered.wait();
    const auto poll_before_abort=state.read();
    state.abort();
    const auto aborted=state.read();
    check(staged && !aborted.history_valid && !aborted.prepared_enabled && !aborted.status.active,
        "abort completes in SDK gap and immediately invalidates copied state");
    sdk_return.count_down(); presenting.join();
    check(!late_success && state.read().last_prepare==11 && !state.read().history_valid,
        "late SDK success is refused without resurrecting aborted history");
    retired.fence=&fences[1]; retired.value=21;
    published=state.polled(poll_before_abort.revision,observed,4,true,retired);
    const auto stale_poll=state.read();
    check(!published.active && !stale_poll.prepared_enabled && published.total_presented==107,
        "stale active poll cannot undo abort or lose cumulative statistics");
    check(stale_poll.retirement.fence==&fences[1] && stale_poll.retirement.value==21,
        "retirement snapshot still records actual observed completion pair after abort");
    check(!state.stage_prepare(source.revision,12,RSF_FG_FIXED,2),"abort also rejects stale pre-SDK prepare staging");
    source=state.read();
    check(!source.history_valid,"next actual source must assemble reset constants");
    check(state.stage_prepare(source.revision,12,RSF_FG_FIXED,2) && state.complete_prepare(source.revision,12,true),
        "fresh source can establish history after abort");
    const auto valid=state.read();
    options.reflex_mode=RSF_REFLEX_BOOST; options.frame_limit_us=1000;
    state.configured(options,RSF_REFLEX_BOOST,0,false);
    const auto latency_only=state.read();
    check(latency_only.revision==valid.revision && latency_only.history_valid && latency_only.prepared_enabled &&
        latency_only.status.effective_generated_frames==2,"latency-only configure preserves valid generation history");
    check(state.stage_prepare(latency_only.revision,13,RSF_FG_FIXED,2),"start prepare before real generation change");
    options.mode=RSF_FG_OFF; options.reflex_mode=RSF_REFLEX_OFF;
    state.configured(options,RSF_REFLEX_OFF,0,true);
    check(!state.complete_prepare(latency_only.revision,13,true),"configuration invalidation also rejects late prepare success");
    published=state.polled(latency_only.revision,observed,2,true,retired);
    check(!published.active && published.effective_mode==RSF_FG_OFF && published.configured_mode==RSF_FG_OFF &&
        published.effective_generated_frames==0 && published.total_presented==109,
        "old poll preserves newly configured mode and counters");
    state.paced(); check(state.read().status.latency_paced_frames==7,"Off pacing status is not incremented");
    state.abort(); options.reflex_mode=RSF_REFLEX_ON;
    state.configured(options,RSF_REFLEX_ON,0,false);
    check(!state.read().history_valid && !state.read().prepared_enabled,"latency-only commit cannot resurrect abort");

    // Three real CPU threads exercise the same production snapshot/publish/abort
    // methods. Coupled capability and retirement fields must never tear.
    rsf::DlssGenerationState concurrent; concurrent.initialize(initial);
    options=fixed_options(); concurrent.configured(options,RSF_REFLEX_ON,2,true);
    constexpr uint32_t iterations=4096;
    std::barrier<> start(3);
    std::atomic<bool> finished{}, coherent{true};
    std::thread publisher([&] {
        start.arrive_and_wait();
        for (uint32_t i=1;i<=iterations;++i) {
            const auto before=concurrent.read();
            auto current=initial; current.min_dimension=i; current.max_generated_frames=i*2;
            current.vendor_status=int32_t(i); current.valid_statistics=RSF_FG_STAT_TOTAL_PRESENTED;
            const rsf_fg_retirement completion{sizeof(completion),&fences[i%2],i};
            (void)concurrent.polled(before.revision,current,1,true,completion);
        }
        finished.store(true,std::memory_order_release);
    });
    std::thread aborting([&] {
        start.arrive_and_wait();
        for (uint32_t i=0;i<iterations;++i) { concurrent.abort(); concurrent.paced(); }
    });
    std::thread reader([&] {
        start.arrive_and_wait();
        do {
            const auto copy=concurrent.read();
            const auto sequence=copy.status.min_dimension;
            if (copy.status.struct_size!=sizeof(initial) || copy.status.active ||
                (sequence && (copy.status.max_generated_frames!=sequence*2 || copy.status.vendor_status!=int32_t(sequence) ||
                    copy.retirement.value!=sequence || copy.retirement.fence!=&fences[sequence%2]))) coherent.store(false);
        } while (!finished.load(std::memory_order_acquire));
    });
    publisher.join(); aborting.join(); reader.join();
    const auto final=concurrent.read();
    check(coherent.load(),"concurrent status and retirement copies are coherent during poll and abort");
    check(final.status.total_presented==100+iterations && final.status.latency_paced_frames==7+iterations,
        "poll/abort publication never overwrites concurrently accumulated counters");
    check(!final.history_valid && !final.prepared_enabled && !final.status.active,"concurrent abort preserves invalidation");
    std::puts(failures ? "DLSS copied-state concurrency FAILED" : "PASS: DLSS abort/prepare publication and copied status/retirement concurrency");
    return failures ? 1 : 0;
}
